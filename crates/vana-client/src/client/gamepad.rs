//! Game controllers (gilrs; ours) through retail's pad path: FUN_10005a40's XInput branch makes the 64-bit pad word
//! (bit n = slot n, which the key functions test) and the stick bytes, from a padsin000 button map (MODLOG "Gamepad").
//! Our windows read egui keys, so the slots a window reads also send their keys (confirm Enter, cancel Escape, ...).

use crate::config::Config;
use gilrs::{Axis, Button, GamepadId, Gilrs};
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

/// FFXiPadConfig's XInput layout: A confirm, B cancel, X main menu, Y active window, LB autorun, LT / RT macros, L3
/// rest / lock on, R3 first person, RB slot 12, Start log out, Back hide windows, d-pad, left stick moves, right stick
/// turns the camera (Y axes inverted, XInput's up being positive).
pub const DEFAULT_PADSIN: &str =
    "8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1";
/// The alternate map used while slot 34 is held (padexsin000): the sticks and d-pad only.
pub const DEFAULT_PADEXSIN: &str =
    "-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-33,-33,32,32,-36,-36,35,35,6,7,5,4,-1,-1,-1,-1,-1,-1,-1";

/// padsin position -> slot (0x1034f930).
const PERM: [u8; 32] = [
    8, 9, 14, 13, 10, 3, 1, 0, 2, 16, 25, 26, 15, 23, 24, 22, 21, 19, 20, 18, 17, 6, 7, 5, 4, 12, 31, 32, 33, 34, 35,
    36,
];

/// XInput's virtual button numbers as padsin uses them (table 0x10329810), by gilrs button.
const VIRTUAL: &[(Button, u8)] = &[
    (Button::East, 0),
    (Button::West, 1),
    (Button::North, 2),
    (Button::South, 3),
    (Button::DPadRight, 4),
    (Button::DPadLeft, 5),
    (Button::DPadUp, 6),
    (Button::DPadDown, 7),
    (Button::LeftTrigger, 8),
    (Button::LeftTrigger2, 9),
    (Button::LeftThumb, 10),
    (Button::RightTrigger, 11),
    (Button::RightTrigger2, 12),
    (Button::RightThumb, 13),
    (Button::Start, 14),
    (Button::Select, 15),
];

pub const fn bit(slot: u8) -> u64 {
    1 << slot
}

/// Slots the key functions name.
pub mod slot {
    pub const CONFIRM: u8 = 0;
    pub const MENU: u8 = 1;
    pub const TAB_BACK: u8 = 8;
    pub const TAB_NEXT: u8 = 12;
    pub const CANCEL: u8 = 3;
    pub const RIGHT: u8 = 4;
    pub const LEFT: u8 = 5;
    pub const UP: u8 = 6;
    pub const DOWN: u8 = 7;
    pub const MACRO_CTRL: u8 = 9;
    pub const MACRO_ALT: u8 = 13;
    pub const STICK_DPAD: u8 = 25;
    pub const STICK_CAMERA: u8 = 26;
    pub const STICK_MODE: u8 = 31;
    pub const ALT_MAP: u8 = 34;
}

const DPAD: u64 = bit(slot::RIGHT) | bit(slot::LEFT) | bit(slot::UP) | bit(slot::DOWN);

/// A parsed padsin (FUN_10005130) or padexsin (FUN_10005320): the pad bits of each virtual button, the signed axis
/// code (1 lX, 2 lY, 4 lRx, 5 lRy; negative inverted) of each slot that takes one (4 ..= 7, 17 ..= 24), and whether
/// slot 31 may switch the movement stick to the d-pad (DAT_10455c3b: slot 4 on the movement X axis) or the camera
/// (DAT_10455c3c: slot 17 on it).
#[derive(Clone, Debug, PartialEq)]
pub struct PadMap {
    pub buttons: [u64; 32],
    pub axes: [i8; 37],
    pub dpad_mode: bool,
    pub camera_mode: bool,
}

impl PadMap {
    pub fn parse(s: &str) -> PadMap {
        let mut slotval = [-1i32; 37];
        for (p, tok) in s.split([',', ' ']).filter(|t| !t.is_empty()).take(PERM.len()).enumerate() {
            slotval[PERM[p] as usize] = tok.trim().parse().unwrap_or(-1);
        }
        let mut m = PadMap { buttons: [0; 32], axes: [0; 37], dpad_mode: false, camera_mode: false };
        for (slot, &v) in slotval.iter().enumerate() {
            if v == -1 {
                continue;
            }
            if v.abs() < 32 {
                m.buttons[v.unsigned_abs() as usize] = bit(slot as u8);
            } else if matches!(slot, 4..=7 | 17..=24) {
                m.axes[slot] = (if v > 0 { v - 31 } else { v + 31 }) as i8;
            }
        }
        if slotval[31] != -1 && slotval[21] != -1 {
            let x = slotval[21].abs();
            (m.dpad_mode, m.camera_mode) = (slotval[4].abs() == x, slotval[17].abs() == x);
        }
        m
    }
}

/// One poll's device state: the virtual buttons down and the axes (-127 ..= 127, XInput's signs: up positive).
pub struct Device<'a> {
    pub buttons: &'a [u8],
    pub axis: &'a dyn Fn(u8) -> f32,
}

/// What FUN_10005a40's XInput branch makes of a poll: the held pad word and the stick values (-127 ..= 127): camera
/// X / Y, movement X / Y. `mode` is DAT_10455c3e; `modes` the maps' (dpad, camera) flags; `dead` our round dead zone.
pub fn read(map: &PadMap, alt: &PadMap, dev: &Device, mode: u8, modes: (bool, bool), dead: f32) -> (u64, [f32; 4]) {
    // While slot 34 is held the buttons (only) come from the alternate map.
    let use_alt = dev.buttons.iter().any(|&b| map.buttons.get(b as usize) == Some(&bit(slot::ALT_MAP)));
    let mut held = 0u64;
    for &b in dev.buttons {
        let m = if use_alt && map.buttons[b as usize] != bit(slot::ALT_MAP) { alt } else { map };
        held |= m.buttons.get(b as usize).copied().unwrap_or(0);
    }
    // [cam X, cam Y, move X, move Y, d-pad X, d-pad Y]. Axes (jump table 0x100077fc, padsin's map only): each slot's
    // code copies ±axis into its destination; code 3 is zero and 6 none in XInput.
    let mut v = [0f32; 6];
    for (s, d) in [(17, 0), (19, 1), (21, 2), (23, 3), (4, 4), (6, 5)] {
        for code in [map.axes[s], map.axes[s + 1]] {
            if code != 0 {
                let a =
                    if matches!(code.unsigned_abs(), 1 | 2 | 4 | 5) { (dev.axis)(code.unsigned_abs()) } else { 0.0 };
                v[d] = a * code.signum() as f32;
            }
        }
    }
    // Then a button on a stick slot pushes that way, over the axis.
    for (s, d, x) in [
        (17, 0, 127.0),
        (18, 0, -127.0),
        (19, 1, -127.0),
        (20, 1, 127.0),
        (21, 2, 127.0),
        (22, 2, -127.0),
        (23, 3, -127.0),
        (24, 3, 127.0),
    ] {
        if held & bit(s) != 0 {
            v[d] = x;
        }
    }
    // A d-pad or camera that shares the movement axis takes the stick only through the modes.
    if modes.0 {
        (v[4], v[5]) = (0.0, 0.0);
    }
    if modes.1 {
        (v[0], v[1]) = (0.0, 0.0);
    }
    // Slot 25 (its d-pad bits dropped), then slot 26, then mode 2, then mode 1: the movement stick drives the d-pad
    // or the camera.
    if held & bit(slot::STICK_DPAD) != 0 {
        held &= !DPAD;
        (v[4], v[5], v[2], v[3]) = (v[2], v[3], 0.0, 0.0);
    } else if held & bit(slot::STICK_CAMERA) != 0 || mode == 2 {
        (v[0], v[1], v[2], v[3]) = (v[2], v[3], 0.0, 0.0);
    } else if mode == 1 {
        (v[4], v[5], v[2], v[3]) = (v[2], v[3], 0.0, 0.0);
    }
    // Retail zeroes each stick axis under 42 separately, which snaps a turning stick onto the axes. Ours: a round dead
    // zone, both axes of a stick zeroed while its push is under it.
    let dead = dead * 127.0;
    for (x, y) in [(0, 1), (2, 3)] {
        if v[x].hypot(v[y]) < dead {
            (v[x], v[y]) = (0.0, 0.0);
        }
    }
    for x in &mut v {
        *x = x.clamp(-127.0, 127.0);
    }
    // The d-pad from an axis (no dead zone there: ±64).
    held |= if v[4] >= 64.0 {
        bit(slot::RIGHT)
    } else if v[4] <= -64.0 {
        bit(slot::LEFT)
    } else {
        0
    };
    held |= if v[5] >= 64.0 {
        bit(slot::DOWN)
    } else if v[5] <= -64.0 {
        bit(slot::UP)
    } else {
        0
    };
    (held, [v[0], v[1], v[2], v[3]])
}

/// The next stick mode after slot 31 (FUN_10005a40's end): only the ones the maps allow, back to 0 after 2.
fn next_mode(mode: u8, modes: (bool, bool)) -> u8 {
    let mut m = mode;
    loop {
        m += 1;
        if m > 2 {
            return 0;
        }
        if (m == 1 && modes.0) || (m == 2 && modes.1) {
            return m;
        }
    }
}

/// The egui keys a held pad word stands for, and the pad word left for the key functions. `menu`: the d-pad is the
/// arrows; `palette`: a macro palette is up, and takes confirm / cancel / the d-pad from the key functions; `tabs`: a
/// window has the keys, so LB / RB (slots 8 / 12) are Shift+Tab / Tab (ours: the modern windows' tabs), and not their
/// key functions (LB would toggle autorun, 0x1c).
pub fn keys_of(held: u64, menu: bool, palette: bool, tabs: bool) -> (Vec<KeyCode>, u64) {
    let mut keys = Vec::new();
    let (ctrl, alt) = (held & bit(slot::MACRO_CTRL) != 0, held & bit(slot::MACRO_ALT) != 0);
    // 0xa8 (either trigger) is the page modifier, 0xb5 / 0xb6 (both, as Alt+Right / Left) the bag switch. The right-hand
    // codes only mark the egui modifier.
    if menu && (ctrl || alt) {
        keys.push(if ctrl && alt { KeyCode::AltRight } else { KeyCode::ControlRight });
    }
    for (s, k) in
        [(slot::CONFIRM, KeyCode::Enter), (slot::CANCEL, KeyCode::Escape), (slot::MENU, KeyCode::NumpadSubtract)]
    {
        if held & bit(s) != 0 {
            keys.push(k);
        }
    }
    if tabs && held & bit(slot::TAB_BACK) != 0 {
        keys.extend([KeyCode::ShiftRight, KeyCode::Tab]);
    } else if tabs && held & bit(slot::TAB_NEXT) != 0 {
        keys.push(KeyCode::Tab);
    }
    if menu {
        for (s, k) in [
            (slot::UP, KeyCode::ArrowUp),
            (slot::DOWN, KeyCode::ArrowDown),
            (slot::LEFT, KeyCode::ArrowLeft),
            (slot::RIGHT, KeyCode::ArrowRight),
        ] {
            if held & bit(s) != 0 {
                keys.push(k);
            }
        }
    }
    let mut taken = if palette { bit(slot::CONFIRM) | bit(slot::CANCEL) | DPAD } else { 0 };
    if tabs {
        taken |= bit(slot::TAB_BACK) | bit(slot::TAB_NEXT);
    }
    (keys, held & !taken)
}

fn nav(k: KeyCode) -> bool {
    matches!(k, KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight)
}

/// A frame's worth of pad input for the client.
#[derive(Default)]
pub struct Frame {
    /// The pad word for the key functions (`Input::set_pad`).
    pub pad: u64,
    /// Movement X / Y, camera X / Y in -1 ..= 1 (the stick bytes' (b - 0x80) / 128), for `Input::set_stick`.
    pub stick: [f32; 4],
    /// egui key events for the windows (with repeats for the arrows).
    pub events: Vec<egui::Event>,
    /// A button went down (hides the mouse cursor, as a key does).
    pub pressed: bool,
    /// A button or a stick past the dead zone is held (keeps the screen awake, client/keepawake.rs).
    pub used: bool,
    /// While the Controller window waits for a button (`Ctx::capture`): what the pad did, and nothing else is sent.
    pub capture: Option<Capture>,
}

/// The pad as the Controller window assigns from it (modern/padwin.rs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    /// Buttons that went down this frame (padsin's numbers).
    pub pressed: Vec<u8>,
    /// The sticks: left X / Y, right X / Y, -127 ..= 127 in XInput's signs (up positive).
    pub axes: [f32; 4],
}

/// Where the button settings in use come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// config.json (gamepad_padsin): the Controller window saved them.
    Config,
    /// Retail's registry values (FFXiPadConfig.exe), padreg.rs.
    Retail,
    /// Neither: FFXiPadConfig's XInput layout.
    Default,
}

/// The settings in use: the button maps, the pad on, input in the background, and their source.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub padsin: String,
    pub padexsin: String,
    pub on: bool,
    pub background: bool,
    /// The pad rumbles (padmode000's second flag, "Enable Force Feedback": DAT_10455c39).
    pub rumble: bool,
    pub source: Source,
    /// config.json holds a map (either): Use Retail Settings has something to clear.
    pub saved: bool,
}

/// What a frame's settings are. config.json's maps win (the Controller window writes them); else retail's registry
/// values, whose padmode000 also says whether the pad is on ([0]: off, FUN_10002840 leaves the maps empty) and plays in
/// the background ([4]) and rumbles ([1]); else FFXiPadConfig's XInput layout. config gamepad off always turns it
/// off, and so does gamepad_rumble the rumble.
/// A config.json gamepad_padexsin is used whatever the main map's source.
pub fn settings(cfg: &Config, retail: Option<&crate::padreg::Retail>) -> Settings {
    let mut s = settings_of(cfg, retail);
    if !cfg.gamepad_padexsin.trim().is_empty() {
        (s.padexsin, s.saved) = (cfg.gamepad_padexsin.clone(), true);
    }
    s
}

fn settings_of(cfg: &Config, retail: Option<&crate::padreg::Retail>) -> Settings {
    let (on, background, rumble) = (cfg.gamepad, cfg.gamepad_background, cfg.gamepad_rumble);
    if !cfg.gamepad_padsin.trim().is_empty() {
        let padexsin = DEFAULT_PADEXSIN.to_string();
        return Settings {
            padsin: cfg.gamepad_padsin.clone(),
            padexsin,
            on,
            background,
            rumble,
            source: Source::Config,
            saved: true,
        };
    }
    if let Some(r) = retail.filter(|r| r.padsin.is_some()) {
        return Settings {
            padsin: r.padsin.clone().unwrap_or_default(),
            // With no padexsin000 retail's alternate map stays empty (FUN_10005320(0, 0)).
            padexsin: r.padexsin.clone().unwrap_or_default(),
            on: on && r.mode(0) != Some(false),
            background: background || r.mode(4) == Some(true),
            rumble: rumble && r.mode(1) != Some(false),
            source: Source::Retail,
            saved: false,
        };
    }
    Settings {
        padsin: DEFAULT_PADSIN.into(),
        padexsin: DEFAULT_PADEXSIN.into(),
        on,
        background,
        rumble,
        source: Source::Default,
        saved: false,
    }
}

/// What has the pad this frame.
#[derive(Clone, Copy, Default)]
pub struct Ctx {
    /// A window has the keys.
    pub menu: bool,
    /// A macro palette is up.
    pub palette: bool,
    /// A window has the keys (not the chat line): LB / RB turn its tabs.
    pub tabs: bool,
    /// In the world.
    pub field: bool,
    /// The software keyboard is up: it reads the buttons as key functions, so none becomes a key for the field.
    pub keyboard: bool,
    /// The `<stpt>` / `<stal>` party cursor has the keys.
    pub party_cursor: bool,
    /// The Controller window waits for a button or a stick: the pad goes to it (Frame::capture), not to the game.
    pub capture: bool,
}

/// How often a failed Gilrs::new is tried again (a pad backend that comes up late, e.g. udev in a container).
const RETRY: Duration = Duration::from_secs(5);

/// One poll of the pad that plays, from gilrs or SDL: the buttons held (VIRTUAL's numbers, sorted) and the sticks and
/// triggers (lx, ly, rx, ry in -1 ..= 1, up positive; lt, rt in 0 ..= 1).
pub struct Pad {
    pub buttons: Vec<u8>,
    pub axes: [f32; 6],
}

fn gilrs_pad(pad: &gilrs::Gamepad) -> Pad {
    let trigger = |b: Button| pad.button_data(b).map_or(0.0, |d| d.value());
    Pad {
        buttons: VIRTUAL.iter().filter(|(b, _)| pad.is_pressed(*b)).map(|(_, n)| *n).collect(),
        axes: [
            pad.value(Axis::LeftStickX),
            pad.value(Axis::LeftStickY),
            pad.value(Axis::RightStickX),
            pad.value(Axis::RightStickY),
            trigger(Button::LeftTrigger2),
            trigger(Button::RightTrigger2),
        ],
    }
}

#[derive(Default)]
pub struct Gamepad {
    gilrs: Option<Gilrs>,
    /// When Gilrs::new may be tried (again), and its last error (logged once, not every retry).
    next_try: Option<Instant>,
    gilrs_err: Option<String>,
    /// SDL's pads (Linux): started once when gilrs fails or finds none.
    #[cfg(target_os = "linux")]
    sdl: Option<super::gamepad_sdl::Sdl>,
    #[cfg(target_os = "linux")]
    sdl_tried: bool,
    active: Option<GamepadId>,
    maps: Option<(String, String, PadMap, PadMap)>,
    /// DAT_10455c3e: the left stick's mode (0 move, 1 d-pad, 2 camera), stepped by slot 31.
    mode: u8,
    prev: u64,
    /// The keys held now.
    down: Vec<KeyCode>,
    /// The pad repeat (FUN_10005a40 +0x248, as Input's): ticks not yet a 2-tick frame, the counter, armed.
    clock: f32,
    counter: f32,
    armed: bool,
    /// Retail's registry values (padreg.rs), read once on a thread (system.reg files are megabytes).
    retail: Option<crate::padreg::Retail>,
    retail_rx: Option<std::sync::mpsc::Receiver<Option<crate::padreg::Retail>>>,
    /// The buttons held last frame while capturing, whether it was, and the buttons still held from it (they do
    /// nothing until let go: the A that assigned must not press Enter in the window).
    cap_prev: Vec<u8>,
    capturing: bool,
    mute: Vec<u8>,
    /// The rumble: retail's level and hold (FUN_10007ae0), the ticks not yet a retail frame, the pad's effect
    /// (ours: one constant effect whose gain is the motors' level), the level it plays at, and a pad that refused.
    rumble: Rumble,
    rumble_clock: f32,
    ff: Option<(GamepadId, gilrs::ff::Effect)>,
    ff_level: u16,
    ff_refused: Option<GamepadId>,
}

/// FUN_10007ae0, the XInput branch, once a retail frame from the pad poll (FUN_10005a40): the frame's rumble sum
/// (pad +0xdb4: effect op 0x5f's v, the chat call's 60) doubled, nothing under 10, at most 256; the motors change at
/// most every 5 ticks (+0xdb8), and a frame that comes sooner loses its level. Both motors get min(level x 10000 /
/// 256 x 4, 0xffff). *(Retail's DirectInput pads take level x 10000 / 256 as a constant force instead, full at 256.)*
#[derive(Default)]
pub struct Rumble {
    hold: i32,
}

impl Rumble {
    /// One frame of `ticks` (FUN_10014cf0, whole): the motors' new level, when they are set.
    pub fn frame(&mut self, sum: i32, ticks: i32) -> Option<u16> {
        let level = match sum.max(0).saturating_mul(2) {
            l if l < 10 => 0,
            l => l.min(0x100),
        };
        if self.hold > 0 {
            self.hold -= ticks;
            if self.hold > 0 {
                return None;
            }
        }
        self.hold = 5;
        Some((level * 10000 / 256 * 4).min(0xffff) as u16)
    }
}

impl Gamepad {
    /// Reads the pads. `focused`: the window has the focus (a pad in the background does nothing, as padmode's
    /// foreground default, unless config gamepad_background). `mods` sees the buttons held (VIRTUAL's numbers), the
    /// sticks and triggers (lx, ly, rx, ry, lt, rt) and whether a pad plays, before the game: the buttons it returns
    /// are the mods' and the game does not see them.
    pub fn poll(
        &mut self,
        cfg: &Config,
        focused: bool,
        ctx: Ctx,
        dt: f32,
        mods: &mut dyn FnMut(&[u8], [f32; 6], bool) -> Vec<u8>,
    ) -> Frame {
        let Ctx { menu, palette, tabs, field, keyboard, party_cursor, capture } = ctx;
        if self.retail_rx.is_none() {
            self.reload_retail(cfg.game_dir.clone());
        }
        if let Some(r) = self.retail_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match &r {
                Some(r) => log::info!("gamepad: retail settings in {}", r.source),
                None => log::info!("gamepad: no retail settings (FFXiPadConfig) found"),
            }
            self.retail = r;
        }
        let set = settings(cfg, self.retail.as_ref());
        if !set.on && !capture {
            mods(&[], [0.0; 6], false);
            return self.release_all();
        }
        self.connect();
        let (padsin, padexsin) = (set.padsin.as_str(), set.padexsin.as_str());
        if self.maps.as_ref().is_none_or(|m| m.0 != padsin || m.1 != padexsin) {
            self.maps =
                Some((padsin.to_string(), padexsin.to_string(), PadMap::parse(padsin), PadMap::parse(padexsin)));
        }
        let pad = match self.read_pad() {
            Some(p) if focused || set.background => p,
            _ => {
                mods(&[], [0.0; 6], false);
                return self.release_all();
            }
        };
        let Pad { mut buttons, axes } = pad;
        if capture {
            // Buttons already down when it starts (the A that asked) are not presses.
            if !self.capturing {
                (self.capturing, self.cap_prev) = (true, buttons.clone());
            }
            let pressed = buttons.iter().copied().filter(|b| !self.cap_prev.contains(b)).collect();
            let axes = [axes[0] * 127.0, axes[1] * 127.0, axes[2] * 127.0, axes[3] * 127.0];
            self.cap_prev = buttons;
            mods(&[], [0.0; 6], false);
            let mut f = self.release_all();
            f.used = true;
            f.capture = Some(Capture { pressed, axes });
            return f;
        }
        if std::mem::take(&mut self.capturing) {
            self.mute = std::mem::take(&mut self.cap_prev);
        }
        self.mute.retain(|b| buttons.contains(b));
        buttons.retain(|b| !self.mute.contains(b));
        let taken = mods(&buttons, axes, true);
        buttons.retain(|b| !taken.contains(b));
        let axis = |code: u8| match code {
            1 => axes[0] * 127.0,
            2 => axes[1] * 127.0,
            4 => axes[2] * 127.0,
            5 => axes[3] * 127.0,
            _ => 0.0,
        };
        let (_, _, map, alt) = self.maps.as_ref().unwrap();
        // FUN_10005320 adds the alternate map's mode flags to padsin's.
        let modes = (map.dpad_mode || alt.dpad_mode, map.camera_mode || alt.camera_mode);
        if !modes.0 && !modes.1 {
            self.mode = 0;
        }
        let (held, v) =
            read(map, alt, &Device { buttons: &buttons, axis: &axis }, self.mode, modes, cfg.gamepad_deadzone);
        if held & !self.prev & bit(slot::STICK_MODE) != 0 {
            self.mode = next_mode(self.mode, modes);
        }
        let menu = menu || palette || party_cursor || !field;
        // The party cursor reads egui keys only: its confirm / cancel / d-pad leave the field's pad word.
        let (keys, pad_bits) =
            if keyboard { (Vec::new(), held) } else { keys_of(held, menu, palette || party_cursor, tabs && menu) };
        let mut f = self.step(held, &keys, dt);
        self.prev = held;
        f.pad = pad_bits;
        f.used = held != 0 || v.iter().any(|&a| a != 0.0);
        // Outside the field the sticks do nothing (they never drive menus).
        f.stick = if field { [v[2] / 128.0, v[3] / 128.0, v[0] / 128.0, v[1] / 128.0] } else { [0.0; 4] };
        f
    }

    /// The frame's rumble `sum` (retail's pad +0xdb4) to the pad that plays, in retail frames. Nothing while the pad
    /// is off, the rumble is off, or the game is in the background without the background option (then it stops).
    pub fn rumble(&mut self, cfg: &Config, focused: bool, sum: i32, dt: f32) {
        let set = settings(cfg, self.retail.as_ref());
        let on = set.on && set.rumble && (focused || set.background);
        let Some(n) = super::camera::retail_frame(&mut self.rumble_clock, dt, 20.0) else { return };
        if let Some(level) = self.rumble.frame(if on { sum } else { 0 }, n as i32) {
            self.motors(level);
        }
    }

    /// Both motors at `level` (XInputSetState's scale).
    fn motors(&mut self, level: u16) {
        let Some(g) = &mut self.gilrs else { return };
        if self.ff.as_ref().is_some_and(|(id, _)| Some(*id) != self.active) {
            // Another pad plays now: the old one's effect goes with its handle.
            (self.ff, self.ff_level) = (None, 0);
        }
        if level == self.ff_level {
            return;
        }
        if self.ff.is_none() && level != 0 {
            let Some(id) = self.active.filter(|id| self.ff_refused != Some(*id)) else { return };
            use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
            let scheduling = Replay { play_for: Ticks::from_ms(1000), ..Default::default() };
            let motor = |kind| BaseEffect { kind, scheduling, ..Default::default() };
            match EffectBuilder::new()
                .add_effect(motor(BaseEffectType::Strong { magnitude: u16::MAX }))
                .add_effect(motor(BaseEffectType::Weak { magnitude: u16::MAX }))
                .gamepads(&[id])
                .gain(0.0)
                .finish(g)
            {
                Ok(e) => self.ff = Some((id, e)),
                Err(e) => {
                    log::info!("gamepad {id}: no rumble ({e})");
                    self.ff_refused = Some(id);
                    return;
                }
            }
        }
        let Some((_, e)) = &self.ff else { return };
        let r = e.set_gain(level as f32 / u16::MAX as f32).and_then(|_| match (self.ff_level, level) {
            (0, _) => e.play(),
            (_, 0) => e.stop(),
            _ => Ok(()),
        });
        if let Err(e) = r {
            log::debug!("gamepad rumble: {e}");
        }
        self.ff_level = level;
    }

    /// Starts gilrs, again every RETRY while it fails, and on Linux SDL once when gilrs failed or found no pad.
    fn connect(&mut self) {
        let now = Instant::now();
        if self.gilrs.is_none() && self.next_try.is_none_or(|t| now >= t) {
            self.next_try = Some(now + RETRY);
            match Gilrs::new() {
                Ok(g) => {
                    for (id, pad) in g.gamepads() {
                        log::info!("gamepad {id}: {}", pad.name());
                        self.active.get_or_insert(id);
                    }
                    self.gilrs = Some(g);
                    self.gilrs_err = None;
                }
                Err(e) => {
                    let e = e.to_string();
                    if self.gilrs_err.as_ref() != Some(&e) {
                        log::warn!("gamepad: {e} (trying again every {}s)", RETRY.as_secs());
                        self.gilrs_err = Some(e);
                    }
                }
            }
        }
        #[cfg(target_os = "linux")]
        if !self.sdl_tried && self.gilrs.as_ref().is_none_or(|g| g.gamepads().next().is_none()) {
            self.sdl_tried = true;
            match super::gamepad_sdl::Sdl::new() {
                Ok(s) => {
                    log::info!("gamepad: SDL {} as the fallback", sdl2::version::version());
                    self.sdl = Some(s);
                }
                Err(e) => log::warn!("gamepad: SDL: {e}"),
            }
        }
    }

    /// The pad that plays (the last one touched): gilrs's, else SDL's.
    fn read_pad(&mut self) -> Option<Pad> {
        #[cfg(target_os = "linux")]
        if let Some(s) = &mut self.sdl {
            s.update();
        }
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                match ev.event {
                    gilrs::EventType::Connected => {
                        log::info!("gamepad {} connected: {}", ev.id, g.gamepad(ev.id).name());
                        self.active.get_or_insert(ev.id);
                    }
                    gilrs::EventType::Disconnected => {
                        if self.active == Some(ev.id) {
                            self.active = g.gamepads().map(|(id, _)| id).find(|&id| id != ev.id);
                        }
                    }
                    gilrs::EventType::ButtonPressed(..) => self.active = Some(ev.id),
                    _ => {}
                }
            }
            if let Some(p) = self.active.and_then(|id| g.connected_gamepad(id)) {
                return Some(gilrs_pad(&p));
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(p) = self.sdl.as_ref().and_then(|s| s.state()) {
            return Some(p);
        }
        None
    }

    /// Reads retail's registry values again, on a thread (at start, and when the Controller window asks).
    pub fn reload_retail(&mut self, game_dir: std::path::PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::padreg::find(&game_dir));
        });
        self.retail_rx = Some(rx);
    }

    /// Retail's registry values, once read.
    pub fn retail(&self) -> Option<&crate::padreg::Retail> {
        self.retail.as_ref()
    }

    /// The connected pads' names (gilrs's, else SDL's), the one that plays first.
    pub fn pads(&self) -> Vec<String> {
        let mut v: Vec<(bool, String)> = self
            .gilrs
            .iter()
            .flat_map(|g| g.gamepads())
            .map(|(id, p)| (Some(id) != self.active, p.name().to_string()))
            .collect();
        v.sort();
        let mut v: Vec<String> = v.into_iter().map(|p| p.1).collect();
        // SDL sees gilrs's pads too: its list only when gilrs has none.
        #[cfg(target_os = "linux")]
        if let (true, Some(s)) = (v.is_empty(), &self.sdl) {
            v = s.names();
        }
        v
    }

    fn release_all(&mut self) -> Frame {
        let f = self.step(0, &[], 0.0);
        self.prev = 0;
        f
    }

    /// Key edges against last frame's keys, and the arrows' repeats on retail's pad counter: a new button starts it
    /// over; held, it adds 2 ticks a 2-tick frame and past 16 repeats every held key and drops to 14 (18 ticks, then
    /// every 4), as Input's pad word does.
    fn step(&mut self, held: u64, keys: &[KeyCode], dt: f32) -> Frame {
        let mut f = Frame::default();
        let mods = egui::Modifiers {
            ctrl: keys.contains(&KeyCode::ControlRight),
            alt: keys.contains(&KeyCode::AltRight),
            ..Default::default()
        };
        let shift = egui::Modifiers { shift: keys.contains(&KeyCode::ShiftRight), ..Default::default() };
        let event = |k: KeyCode, pressed: bool, repeat: bool| {
            // The modifier markers are modifiers only, not keys themselves.
            if matches!(k, KeyCode::ShiftRight | KeyCode::ControlRight | KeyCode::AltRight) {
                return None;
            }
            // The page modifier rides on the arrows only (Ctrl + Minus would zoom egui), LB's Shift on its Tab.
            let mods = if nav(k) {
                mods
            } else if k == KeyCode::Tab {
                shift
            } else {
                egui::Modifiers::default()
            };
            // egui reads the numpad's minus as Minus, as egui-winit maps it.
            egui::Key::from_name(&format!("{k:?}")).map(|key| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat,
                modifiers: mods,
            })
        };
        for &k in &self.down {
            if !keys.contains(&k) {
                f.events.extend(event(k, false, false));
            }
        }
        self.down.retain(|k| keys.contains(k));
        let mut repeat = false;
        if held & !self.prev != 0 {
            (self.clock, self.counter, self.armed) = (0.0, 0.0, false);
        } else if held != 0 {
            self.clock += dt * 60.0;
            while self.clock >= 2.0 {
                self.clock -= 2.0;
                self.counter += 2.0;
                if !self.armed {
                    self.armed = true;
                } else if self.counter > 16.0 {
                    repeat = true;
                    self.counter = 14.0;
                }
            }
        }
        if repeat {
            for &k in self.down.iter().filter(|k| nav(**k)) {
                f.events.extend(event(k, true, true));
            }
        }
        for &k in keys {
            if !self.down.contains(&k) {
                self.down.push(k);
                f.events.extend(event(k, true, false));
                f.pressed = true;
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev<'a>(buttons: &'a [u8], axis: &'a dyn Fn(u8) -> f32) -> Device<'a> {
        Device { buttons, axis }
    }

    const DEAD: f32 = 42.0 / 127.0;

    /// FUN_10007ae0 at stock retail's 2 ticks a frame: the motors are set on the first frame and then every third
    /// (the 5-tick hold), with that frame's sum alone. File 3445 gob_/vib1's v 239 is past the 256 cap: 40000 of
    /// 65535; the chat call's 60 gives 18748; a sum under 5 nothing.
    #[test]
    fn rumble_levels_and_hold() {
        let mut r = Rumble::default();
        let out: Vec<Option<u16>> = [239, 239, 239, 60, 4, 4, 4, 0].into_iter().map(|v| r.frame(v, 2)).collect();
        assert_eq!(out, [Some(40000), None, None, Some(18748), None, None, Some(0), None]);
        // A slow frame's ticks clear the hold at once; the sum is never negative.
        let mut r = Rumble::default();
        assert_eq!((r.frame(5, 2), r.frame(-3, 5), r.frame(1000, 8)), (Some(1560), Some(0), Some(40000)));
    }

    /// A connected pad takes the effect: the chat call's level for 0.4 s, then off (run by hand: it needs a pad that
    /// rumbles, and buzzes it).
    #[test]
    #[ignore]
    fn rumble_reaches_a_pad() {
        let g = Gilrs::new().unwrap();
        let active = g.gamepads().map(|(id, _)| id).next();
        let mut pad = Gamepad { gilrs: Some(g), active, ..Default::default() };
        pad.motors(18748);
        assert!(pad.ff.is_some(), "no pad that rumbles (refused: {:?})", pad.ff_refused);
        std::thread::sleep(std::time::Duration::from_millis(400));
        pad.motors(0);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(pad.ff_level, 0);
    }

    /// Retail's padmode000 turns the rumble off with its second flag, as the pad with its first; config's maps
    /// leave it to config.json.
    #[test]
    fn rumble_setting() {
        let retail = |mode: &str| crate::padreg::Retail {
            padmode: Some(mode.into()),
            padsin: Some(DEFAULT_PADSIN.into()),
            ..Default::default()
        };
        let mut cfg = Config::default();
        assert!(settings(&cfg, None).rumble);
        assert!(settings(&cfg, Some(&retail("1,1,0,0,0,1"))).rumble);
        assert!(!settings(&cfg, Some(&retail("1,0,0,0,0,1"))).rumble);
        cfg.gamepad_padsin = DEFAULT_PADSIN.into();
        assert!(settings(&cfg, Some(&retail("1,0,0,0,0,1"))).rumble);
        cfg.gamepad_rumble = false;
        assert!(!settings(&cfg, None).rumble);
    }

    /// The default padsin: A confirm (slot 0), B cancel (3), X slot 1, Y slot 2, LB 8, LT 9, L3 10, RB 12, RT 13, R3 14,
    /// Start 15, Back 16, the d-pad 4 ..= 7, lY inverted on 23 / 24, lX on 21 / 22, lRy inverted on 19 / 20, lRx on
    /// 17 / 18.
    #[test]
    fn parse_default() {
        let m = PadMap::parse(DEFAULT_PADSIN);
        let b = |n: usize| m.buttons[n].trailing_zeros();
        assert_eq!((b(3), b(0), b(1), b(2)), (0, 3, 1, 2));
        assert_eq!((b(8), b(9), b(10), b(11), b(12), b(13), b(14), b(15)), (8, 9, 10, 12, 13, 14, 15, 16));
        assert_eq!((b(4), b(5), b(6), b(7)), (4, 5, 6, 7));
        assert_eq!((m.axes[23], m.axes[24], m.axes[21], m.axes[19], m.axes[17]), (-2, -2, 1, -5, 4));
        // No slot 31: no stick modes.
        assert!(!m.dpad_mode && !m.camera_mode);
        let alt = PadMap::parse(DEFAULT_PADEXSIN);
        assert_eq!((alt.buttons[3], alt.axes[23]), (0, -2));
    }

    #[test]
    fn sticks_and_dead_zone() {
        let (m, alt) = (PadMap::parse(DEFAULT_PADSIN), PadMap::parse(DEFAULT_PADEXSIN));
        // Left stick up (XInput +) is forward (move Y -127), right stick right is camera right.
        let ax = |c: u8| match c {
            2 => 127.0,
            4 => 127.0,
            _ => 0.0,
        };
        let (held, v) = read(&m, &alt, &dev(&[], &ax), 0, (false, false), DEAD);
        assert_eq!(held, 0);
        assert_eq!((v[2], v[3], v[0]), (0.0, -127.0, 127.0));
        // Under 42 is nothing.
        let small = |c: u8| if c == 1 { 40.0 } else { 0.0 };
        assert_eq!(read(&m, &alt, &dev(&[], &small), 0, (false, false), DEAD).1, [0.0; 4]);
        // The dead zone is round: a push near an axis keeps its small side, so the direction does not snap.
        let near = |c: u8| match c {
            1 => 120.0,
            2 => 30.0,
            _ => 0.0,
        };
        let v = read(&m, &alt, &dev(&[], &near), 0, (false, false), DEAD).1;
        assert!(v[3] < -29.0 && ((v[3] / v[2]) + 0.25).abs() < 1e-4, "{v:?}");
        // No rescale in the XInput branch: a diagonal keeps its length.
        let diag = |c: u8| if c <= 2 { 80.0 } else { 0.0 };
        let v = read(&m, &alt, &dev(&[], &diag), 0, (false, false), DEAD).1;
        assert_eq!((v[2], v[3]), (80.0, -80.0));
        // Code 3 (lZ) reads nothing in XInput.
        let z = PadMap::parse(
            "8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1"
                .replace("32,32", "34,34")
                .as_str(),
        );
        let full = |_: u8| 127.0;
        assert_eq!(read(&z, &alt, &dev(&[], &full), 0, (false, false), DEAD).1[2], 0.0);
    }

    /// Slot 25 takes the movement stick as the d-pad (its d-pad bits dropped), over slot 26 and the modes; the
    /// alternate map changes buttons, never axes; slot 31 steps only through the modes a map allows.
    #[test]
    fn stick_slots_and_modes() {
        // Slot 25 on LB (button 8), slot 26 on RB (11), slot 34 on Back (15).
        let mut s: Vec<String> = DEFAULT_PADSIN.split(',').map(str::to_string).collect();
        let pos = |slot: u8| PERM.iter().position(|&x| x == slot).unwrap();
        s[pos(25)] = "8".into();
        s[pos(26)] = "11".into();
        s[pos(34)] = "15".into();
        s[pos(8)] = "-1".into();
        s[pos(12)] = "-1".into();
        s[pos(16)] = "-1".into();
        let m = PadMap::parse(&s.join(","));
        let alt = PadMap::parse(DEFAULT_PADEXSIN.replace("-33,-33", "-1,-1").as_str());
        let up = |c: u8| if c == 2 { 127.0 } else { 0.0 };
        let (held, v) = read(&m, &alt, &dev(&[8, 7], &up), 2, (false, true), DEAD);
        assert_eq!((held & DPAD, v[3], v[1]), (bit(slot::UP), 0.0, 0.0));
        let (_, v) = read(&m, &alt, &dev(&[11], &up), 1, (true, false), DEAD);
        assert_eq!((v[1], v[3]), (-127.0, 0.0));
        // Slot 34 held: alt's buttons, padsin's axes (alt has none on movement Y).
        let (held, v) = read(&m, &alt, &dev(&[15, 3], &up), 0, (false, false), DEAD);
        assert_eq!((held & bit(slot::CONFIRM), v[3]), (0, -127.0));
        assert_eq!(
            (
                next_mode(0, (false, true)),
                next_mode(2, (false, true)),
                next_mode(0, (true, false)),
                next_mode(1, (true, false))
            ),
            (2, 0, 1, 0)
        );
        // padsin with slot 31 set and slot 17 on the movement X axis allows the camera mode.
        s[pos(31)] = "13".into();
        s[pos(17)] = "32".into();
        let m = PadMap::parse(&s.join(","));
        assert!(m.camera_mode && !m.dpad_mode);
    }

    #[test]
    fn keys_and_triggers() {
        let m = PadMap::parse(DEFAULT_PADSIN);
        let none = |_: u8| 0.0;
        let rd = |b: &[u8]| read(&m, &m, &dev(b, &none), 0, (false, false), 0.33).0;
        // A and the d-pad up: Enter; the arrow only in a window; the bits stay for the key functions.
        let held = rd(&[3, 6]);
        assert_eq!(keys_of(held, false, false, false), (vec![KeyCode::Enter], bit(0) | bit(6)));
        assert_eq!(keys_of(held, true, false, false).0, vec![KeyCode::Enter, KeyCode::ArrowUp]);
        // The palette and party cursor take confirm and the d-pad from the key functions.
        assert_eq!(keys_of(held, true, true, false), (vec![KeyCode::Enter, KeyCode::ArrowUp], 0));
        // LT in a window: Ctrl on the arrows (0xa8, page); the trigger's bit stays (0x89 item compare).
        let held = rd(&[9, 6]);
        assert_eq!(keys_of(held, true, false, false), (vec![KeyCode::ControlRight, KeyCode::ArrowUp], held));
        // Both triggers: Alt (0xb5 / 0xb6, the bag switch). In the field neither is a key.
        let held = rd(&[9, 12, 4]);
        assert_eq!(keys_of(held, true, false, false).0, vec![KeyCode::AltRight, KeyCode::ArrowRight]);
        assert!(keys_of(held, false, false, false).0.is_empty());
        // LB / RB with a window: Shift+Tab / Tab, and not their key functions (LB's autorun); without one, nothing.
        let held = rd(&[8]);
        assert_eq!(keys_of(held, true, false, true), (vec![KeyCode::ShiftRight, KeyCode::Tab], 0));
        assert_eq!(keys_of(held, false, false, false), (vec![], held));
        let held = rd(&[11]);
        assert_eq!(keys_of(held, true, false, true), (vec![KeyCode::Tab], 0));
        let mut g = Gamepad::default();
        let f = g.step(bit(8), &[KeyCode::ShiftRight, KeyCode::Tab], 1.0 / 60.0);
        assert!(
            matches!(f.events[..], [egui::Event::Key { key: egui::Key::Tab, modifiers, .. }] if modifiers.shift),
            "{:?}",
            f.events
        );
        // Every key has a DIK code.
        for k in [KeyCode::Enter, KeyCode::Escape, KeyCode::NumpadSubtract, KeyCode::ArrowUp] {
            assert!(crate::client::input::dik(k).is_some(), "{k:?}");
        }
    }

    /// Held, an arrow repeats at 18 ticks and every 4 after, at any frame rate.
    #[test]
    fn repeats_at_retail_rate() {
        for fps in [30.0f32, 60.0, 300.0] {
            let mut g = Gamepad::default();
            let dt = 1.0 / fps;
            let f = g.step(bit(7), &[KeyCode::ArrowDown], dt);
            g.prev = bit(7);
            assert!(f.pressed && f.events.len() == 1);
            // 40 ticks held: repeats at 18, 22, ..., 38.
            let mut repeats = 0;
            for _ in 0..(40.0 * fps / 60.0).round() as usize {
                repeats += g.step(bit(7), &[KeyCode::ArrowDown], dt).events.len();
            }
            assert_eq!(repeats, 6, "{fps} fps");
            assert_eq!(g.step(0, &[], dt).events.len(), 1);
        }
        // A window's page modifier: Ctrl on the arrow, not on Enter.
        let mut g = Gamepad::default();
        let f =
            g.step(bit(0) | bit(7) | bit(9), &[KeyCode::ControlRight, KeyCode::Enter, KeyCode::ArrowDown], 1.0 / 60.0);
        let ctrl = |key| {
            f.events
                .iter()
                .find_map(|e| {
                    matches!(e, egui::Event::Key { key: k, .. } if *k == key)
                        .then(|| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.ctrl))
                })
                .unwrap()
        };
        assert!(ctrl(egui::Key::ArrowDown) && !ctrl(egui::Key::Enter));
    }
}
