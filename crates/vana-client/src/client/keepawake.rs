//! Keeps the screen awake while a game controller plays: pad input never reaches the idle timer on many desktops,
//! so without this the screen blanks mid-fight. The hold lasts `HOLD` after the pad's last input, then the desktop's
//! timeout applies again.

use std::time::{Duration, Instant};

/// How long the screen stays awake after the pad's last input.
const HOLD: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct KeepAwake {
    last: Option<Instant>,
    on: bool,
    #[cfg(target_os = "linux")]
    tx: Option<std::sync::mpsc::Sender<bool>>,
    #[cfg(target_os = "linux")]
    failed: bool,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    sent: Option<Instant>,
    #[cfg(target_os = "macos")]
    held: Option<macos::Hold>,
    #[cfg(windows)]
    poked: Option<Instant>,
}

impl KeepAwake {
    /// Once a frame. `used`: the pad gave input this frame.
    pub fn update(&mut self, used: bool) {
        let now = Instant::now();
        if used {
            self.last = Some(now);
        }
        let on = self.last.is_some_and(|t| now - t < HOLD);
        #[cfg(windows)]
        if on {
            self.poke(now);
        }
        // Linux: send on the edges and once a minute through the hold. If the worker is gone, leave it alone until
        // the next hold instead of respawning it every minute.
        #[cfg(target_os = "linux")]
        if !on {
            self.failed = false;
            self.sent = None;
            if self.on {
                let dead = self.tx.as_ref().is_some_and(|tx| tx.send(false).is_err());
                if dead {
                    self.tx = None;
                }
            }
        } else if !self.failed {
            let retry = self.sent.is_none_or(|t| now - t >= Duration::from_secs(60));
            if on != self.on || retry {
                self.sent = Some(now);
                let failed = self.tx.get_or_insert_with(linux::spawn).send(true).is_err();
                if failed {
                    self.tx = None;
                    self.failed = true;
                }
            }
        }
        // macOS: the assertions persist, so only the edges (and a retry after a failed create) need work.
        #[cfg(target_os = "macos")]
        if on != self.on || (on && self.held.is_none() && self.sent.is_none_or(|t| now - t >= Duration::from_secs(60)))
        {
            self.sent = Some(now);
            self.held = if on { macos::Hold::acquire() } else { None };
        }
        self.on = on;
    }

    /// Windows: one call resets the display and sleep timers, as a key would; every 10 s through the hold is plenty.
    #[cfg(windows)]
    fn poke(&mut self, now: Instant) {
        use windows_sys::Win32::System::Power::{ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
        if self.poked.is_none_or(|t| now - t >= Duration::from_secs(10)) {
            self.poked = Some(now);
            // SAFETY: plain flags in, the previous state out.
            unsafe { SetThreadExecutionState(ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED) };
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_core_foundation::CFString;
    use objc2_io_kit::{
        IOPMAssertionCreateWithName, IOPMAssertionID, IOPMAssertionRelease, kIOPMAssertionLevelOn, kIOReturnSuccess,
    };

    const REASON: &str = "Vana Client: Playing with a game controller";

    /// Both idle-sleep assertions held for one pad session; dropping releases them.
    pub struct Hold {
        system: IOPMAssertionID,
        display: IOPMAssertionID,
    }

    impl Hold {
        pub fn acquire() -> Option<Self> {
            let system = assertion("PreventUserIdleSystemSleep")?;
            let display = match assertion("PreventUserIdleDisplaySleep") {
                Some(id) => id,
                None => {
                    release(system);
                    return None;
                }
            };
            Some(Self { system, display })
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            release(self.system);
            release(self.display);
        }
    }

    fn assertion(kind: &'static str) -> Option<IOPMAssertionID> {
        let ty = CFString::from_static_str(kind);
        let name = CFString::from_static_str(REASON);
        let mut id = 0;
        // SAFETY: `ty` and `name` are valid CFStrings, and `id` is a valid out-pointer.
        let r = unsafe { IOPMAssertionCreateWithName(Some(&*ty), kIOPMAssertionLevelOn, Some(&*name), &mut id) };
        if r == kIOReturnSuccess {
            Some(id)
        } else {
            log::warn!("keep awake: IOPMAssertionCreateWithName({kind}) failed: {r}");
            None
        }
    }

    fn release(id: IOPMAssertionID) {
        let r = IOPMAssertionRelease(id);
        if r != kIOReturnSuccess {
            log::warn!("keep awake: IOPMAssertionRelease failed: {r}");
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::Duration;
    use zbus::blocking::{Connection, connection::Builder};
    use zbus::zvariant::{OwnedObjectPath, Value};

    const REASON: &str = "Playing with a game controller";

    /// An inhibit held now; closing the bus connection (the client quits) also drops it.
    enum Held {
        ScreenSaver(u32),
        Portal(OwnedObjectPath),
    }

    /// The bus calls run on a separate thread, so a slow or missing bus never stalls a frame.
    pub fn spawn() -> mpsc::Sender<bool> {
        let (tx, rx) = mpsc::channel::<bool>();
        let spawned = std::thread::Builder::new().name("keep-awake".into()).spawn(move || {
            let conn = match Builder::session().and_then(|b| b.method_timeout(Duration::from_secs(5)).build()) {
                Ok(c) => c,
                Err(e) => return log::warn!("keep awake: no session bus: {e}"),
            };
            let mut held = None;
            for on in rx {
                if on && held.is_none() {
                    held = inhibit(&conn);
                    // Hang up so the next use starts over on a fresh connection (the bus may have gone away).
                    if held.is_none() {
                        return;
                    }
                } else if !on && let Some(h) = held.take() {
                    release(&conn, h);
                }
            }
            if let Some(h) = held {
                release(&conn, h);
            }
        });
        if let Err(e) = spawned {
            log::warn!("keep awake: {e}");
        }
        tx
    }

    fn inhibit(conn: &Connection) -> Option<Held> {
        let ss = conn
            .call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Inhibit",
                &("Vana Client", REASON),
            )
            .and_then(|m| m.body().deserialize::<u32>());
        let e = match ss {
            Ok(cookie) => return Some(Held::ScreenSaver(cookie)),
            Err(e) => e,
        };
        // Flag 8: idle. The window is left empty (the portal then does not tie the inhibit to one).
        let options: HashMap<&str, Value> = HashMap::from([("reason", Value::from(REASON))]);
        let portal = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Inhibit"),
                "Inhibit",
                &("", 8u32, options),
            )
            .and_then(|m| m.body().deserialize::<OwnedObjectPath>());
        match portal {
            Ok(handle) => Some(Held::Portal(handle)),
            Err(p) => {
                log::warn!("keep awake: screen saver inhibit failed ({e}), portal too ({p})");
                None
            }
        }
    }

    fn release(conn: &Connection, held: Held) {
        let r = match &held {
            Held::ScreenSaver(cookie) => conn.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "UnInhibit",
                &(*cookie,),
            ),
            Held::Portal(handle) => conn.call_method(
                Some("org.freedesktop.portal.Desktop"),
                handle.as_ref(),
                Some("org.freedesktop.portal.Request"),
                "Close",
                &(),
            ),
        };
        if let Err(e) = r {
            log::warn!("keep awake: release failed: {e}");
        }
    }
}
