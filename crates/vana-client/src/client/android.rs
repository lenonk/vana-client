//! NativeActivity glue: the activity handed over by `android_main`, the window that comes and goes with it, and
//! touch and mouse input as the client's mouse.

use super::{App, EventLoop, LoadState, resize_surface, updater, uses_playonline};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, Touch, TouchPhase};
use winit::event_loop::ActiveEventLoop;
use winit::platform::android::activity::AndroidApp;

/// The activity, kept for running on Java's main thread and for waking the event loop from JNI callbacks.
static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);
/// The activity's jobject, for JNI calls.
static ACTIVITY: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// winit allows one EventLoop a process; android_main runs again when another activity hands back.
static EVENT_LOOP_RUNNING: AtomicBool = AtomicBool::new(false);
/// Back from another activity (Settings, the browser, the folder picker): the surface is likely stale.
static RESUMED: AtomicBool = AtomicBool::new(false);
/// The native window the surface was made on (null until the first refresh, which startup's resume runs).
static NATIVE_WINDOW: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn set_android_app(app: AndroidApp) {
    ACTIVITY.store(app.activity_as_ptr().cast(), Ordering::Relaxed);
    *APP.lock().unwrap() = Some(app);
}

pub(crate) fn android_activity_ptr() -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    std::ptr::NonNull::new(ACTIVITY.load(Ordering::Relaxed))
}

pub(crate) fn android_event_loop_running() -> bool {
    EVENT_LOOP_RUNNING.load(Ordering::SeqCst)
}

pub(super) fn event_loop() -> anyhow::Result<EventLoop<()>> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let app =
        APP.lock().unwrap().clone().ok_or_else(|| anyhow::anyhow!("android_main did not hand over the activity"))?;
    let event_loop = EventLoop::builder().with_android_app(app).build()?;
    EVENT_LOOP_RUNNING.store(true, Ordering::SeqCst);
    Ok(event_loop)
}

/// Runs `f` on Java's main thread (activities start only from there).
pub(crate) fn android_run_on_ui_thread(f: impl FnOnce() + Send + 'static) {
    if let Some(app) = APP.lock().unwrap().as_ref() {
        app.run_on_java_main_thread(Box::new(f));
    }
}

/// Another activity handed back: the next wait refreshes the surface and draws.
pub(crate) fn android_resume_after_activity() {
    RESUMED.store(true, Ordering::Relaxed);
    crate::android_fs::stale_all_files_access();
    if let Some(app) = APP.lock().unwrap().as_ref() {
        app.create_waker().wake();
    }
}

/// A second `android_main` on an activity recreate must not return (NativeActivity would exit). The game loop already
/// runs in the first call; this thread waits until the process ends.
pub(crate) fn android_block_reentry() -> ! {
    loop {
        std::thread::park();
    }
}

impl App {
    /// Before the event loop waits: the window until it is up, the surface after another activity, and frames
    /// while a window of ours (first start, Game data) or the title's loading has nothing else to wake it.
    pub(super) fn android_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.resumed(event_loop);
            if self.gfx.is_none() {
                self.next_frame = Some(Instant::now() + Duration::from_millis(16));
                return;
            }
        }
        if RESUMED.swap(false, Ordering::Relaxed) {
            self.android_refresh_surface();
            self.android_unstick_game_data();
        } else if self.gfx.as_ref().is_some_and(|g| g.surface.is_none()) {
            self.android_refresh_surface();
        }
        if self.gfx.as_ref().is_some_and(|g| g.surface.is_none()) {
            return;
        }
        if self.next_frame.is_none() && (self.first_run.is_some() || self.game_data.is_some() || self.game_ui.is_none())
        {
            self.next_frame = Some(Instant::now() + Duration::from_millis(16));
        }
    }

    /// winit's `Suspended`: the native window is gone; drop the wgpu surface before returning (Android lifecycle).
    pub(super) fn android_drop_surface(&mut self) {
        let Some(gfx) = &mut self.gfx else { return };
        gfx.surface = None;
        NATIVE_WINDOW.store(std::ptr::null_mut(), Ordering::Relaxed);
        self.shown = false;
        self.next_frame = None;
    }

    /// The swapchain at the window's size, and a frame. Going to the background destroys the native window the
    /// surface drew to: a new one gets a new surface (the same one cannot have two).
    pub(super) fn android_refresh_surface(&mut self) {
        let Some(gfx) = &mut self.gfx else { return };
        let native = APP.lock().unwrap().as_ref().and_then(|a| a.native_window()).map(|w| w.ptr().as_ptr().cast());
        let Some(native) = native else {
            if gfx.surface.is_some() {
                gfx.surface = None;
                NATIVE_WINDOW.store(std::ptr::null_mut(), Ordering::Relaxed);
                self.shown = false;
                self.next_frame = None;
            }
            return;
        };
        let before = NATIVE_WINDOW.swap(native, Ordering::Relaxed);
        let need_surface = gfx.surface.is_none() || (!before.is_null() && before != native);
        if need_surface {
            match gfx.gpu.instance.create_surface(gfx.window.clone()) {
                Ok(surface) => gfx.surface = Some(surface),
                Err(e) => {
                    log::warn!("creating the surface: {e}");
                    return;
                }
            }
        }
        let size = gfx.window.inner_size();
        if size.width > 0 && size.height > 0 {
            resize_surface(gfx, size);
        }
        gfx.window.set_visible(true);
        gfx.window.request_redraw();
        self.shown = true;
        self.next_frame = Some(Instant::now());
    }

    /// After Settings or the folder picker: All files access may have changed — retry a load that failed without it.
    fn android_unstick_game_data(&mut self) {
        if !crate::android_fs::has_all_files_access() || self.index.is_some() {
            return;
        }
        if self.index_warm == LoadState::Failed {
            self.index_warm = LoadState::Idle;
        }
        if self.game_ui.is_none() && self.first_run.is_none() && self.game_ui_state == LoadState::Failed {
            self.game_ui_state = LoadState::Idle;
        }
        if self.flyover.is_none() && self.flyover_state == LoadState::Failed {
            self.flyover_state = LoadState::Idle;
        }
        if self.index_warm == LoadState::Idle && self.cfg.ffxi_dir().join("VTABLE.DAT").is_file() {
            self.start_index_warm();
        }
    }

    /// The Game data window's folder: saved, and the game files loaded again from it (as a first start's end).
    pub(super) fn android_use_game_dir(&mut self, game_dir: std::path::PathBuf) {
        self.cfg.game_dir = game_dir;
        self.cfg.installed = true;
        self.audio = None;
        self.audio_tried = false;
        self.index = None;
        self.keys_table = None;
        self.knock_table = None;
        self.search_words = None;
        self.blu_data = None;
        self.cnq_tables = None;
        self.game_ui_state = LoadState::Idle;
        self.flyover_state = LoadState::Idle;
        self.start_index_warm();
        if let Err(e) = self.cfg.save() {
            log::warn!("saving the config: {e:#}");
        }
        self.updates = Some(updater::start(self.cfg.ffxi_dir(), uses_playonline(&self.cfg)));
        self.gamepad.reload_retail(self.cfg.game_dir.clone());
    }

    /// A pointer move (and a button) for egui on Android. `window_event` does not call egui-winit for touch or mouse
    /// (that would duplicate each press); a move is sent before a button so a tap is not dropped.
    pub(super) fn android_egui_pointer(&mut self, physical: (f64, f64), button: Option<(MouseButton, bool)>) {
        let ppp = self.gfx.as_ref().map_or(1.0, |g| g.egui_ctx.pixels_per_point() as f64).max(0.01);
        let pos = egui::pos2((physical.0 / ppp) as f32, (physical.1 / ppp) as f32);
        self.synth_events.push(egui::Event::PointerMoved(pos));
        if let Some((button, pressed)) = button {
            let button = match button {
                MouseButton::Right => egui::PointerButton::Secondary,
                MouseButton::Middle => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            self.synth_events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: Default::default() });
        }
    }

    /// Over our windows: egui's hover alone stays set after a tap and would take every later one.
    fn android_over_ui(&self) -> bool {
        self.gfx.as_ref().is_some_and(|g| g.egui_ctx.is_pointer_over_egui() || g.egui_ctx.egui_is_using_pointer())
    }

    /// A mouse button (a real mouse, Scrcpy): every press and release reaches the world, which skips those over UI.
    pub(super) fn android_mouse_input(&mut self, button: MouseButton, state: ElementState) {
        let pressed = state == ElementState::Pressed;
        if let Some(at) = self.last_cursor {
            self.android_egui_pointer(at, Some((button, pressed)));
        }
        let over_ui = self.android_over_ui();
        self.mouse_button(button, pressed, over_ui);
    }

    /// A finger as the left mouse button. A drag with it does not look (a tap's jitter would eat the click); a
    /// right or middle drag still does.
    pub(super) fn android_touch(&mut self, touch: Touch) {
        let at = (touch.location.x, touch.location.y);
        self.cursor_in = true;
        if self.look_grab.is_none() {
            self.mouse.visible = true;
        }
        let over_ui = self.android_over_ui();
        match touch.phase {
            TouchPhase::Started | TouchPhase::Ended | TouchPhase::Cancelled => {
                let pressed = touch.phase == TouchPhase::Started;
                self.last_cursor = Some(at);
                self.android_egui_pointer(at, Some((MouseButton::Left, pressed)));
                self.mouse_button(MouseButton::Left, pressed, over_ui);
            }
            TouchPhase::Moved => {
                self.android_egui_pointer(at, None);
                if let (true, Some((x, y))) = (self.dragging || self.middle_down, self.last_cursor) {
                    if self.look_grab.is_none() {
                        self.grab_pointer();
                    }
                    if self.look_grab != Some(true) {
                        self.look(at.0 - x, at.1 - y);
                    }
                    if let (Some(false), Some(grab), Some(g)) = (self.look_grab, self.grab_at, self.gfx.as_ref()) {
                        let _ = g.window.set_cursor_position(winit::dpi::PhysicalPosition::new(grab.0, grab.1));
                        self.last_cursor = Some(grab);
                        return;
                    }
                }
                self.last_cursor = Some(at);
            }
        }
    }
}

/// A pinned certificate no longer matches what the server sent (login or lobby TLS).
pub(crate) fn tls_pin_mismatch_message(message: &str) -> bool {
    message.contains("the certificate changed") || message.contains("sent another certificate than the login server")
}

/// Which TLS port to forget for this certificate error (`auth_port` is the login port).
pub(crate) fn tls_trust_port(message: &str, auth_port: u16) -> Option<u16> {
    if message.contains("the certificate changed") {
        return Some(auth_port);
    }
    let rest = message.strip_prefix("port ")?;
    let port: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    port.parse().ok()
}

/// Trust-server: forgets pins the way `vana-client trust-server` does and returns its status lines.
/// JNI: `VanaClientNativeActivity.onResume` after Settings or the browser.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vanaclient_VanaClientNativeActivity_nativeAfterUiActivity(
    _env: jni::JNIEnv<'_>,
    _class: jni::objects::JClass<'_>,
) {
    let _ = std::panic::catch_unwind(android_resume_after_activity);
}

pub(crate) fn forget_server_tls_pins(host: &str, port: Option<u16>) -> anyhow::Result<Vec<String>> {
    let store = crate::config::known_servers_path();
    Ok(crate::net::tls::trust(&store, host.trim(), port, None)?)
}

/// Trust server / OK on the classic yes-no error window (menu ptc6yesn).
pub(crate) enum CertTrustAction {
    None,
    Dismiss,
    Trust,
}

pub(crate) fn cert_trust_dialog(
    lu: &mut super::game_ui::GameUi,
    text: &str,
    input: &super::UiInput,
    sw: f32,
    sh: f32,
) -> CertTrustAction {
    const TRUST: &str = "Trust server";
    const OK: &str = "OK";
    let Some(children) = lu.yes_no_window(text, true, false, sw, sh) else {
        return CertTrustAction::None;
    };
    let (mut trust_r, mut ok_r) = (None, None);
    for (id, r) in children {
        match id {
            1 => trust_r = Some(r),
            2 => ok_r = Some(r),
            _ => {}
        }
    }
    let (Some(tr), Some(or)) = (trust_r, ok_r) else {
        return CertTrustAction::None;
    };
    for (r, label, lit) in [(tr, TRUST, true), (or, OK, false)] {
        let tint = if lit { 0x8080_5020 } else { 0x8080_8080 };
        super::settings::button(lu, r, tint);
        let lx = r[0] + ((r[2] - lu.label_width(label)) / 2.0).round();
        lu.label(label, lx, r[1] + 1.0, tint, true);
    }
    lu.cursor(tr[0], tr[1]);
    if input.clicked {
        if let Some(p) = input.pointer {
            if super::inside(&tr, p) {
                return CertTrustAction::Trust;
            }
            if super::inside(&or, p) {
                return CertTrustAction::Dismiss;
            }
        }
    }
    if input
        .events
        .iter()
        .any(|ev| matches!(ev, egui::Event::Key { key: egui::Key::Escape | egui::Key::Enter, pressed: true, .. }))
    {
        return CertTrustAction::Dismiss;
    }
    CertTrustAction::None
}
