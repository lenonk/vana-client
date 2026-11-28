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

/// FFXiPadConfig's Pattern A (menu 0x410, record 0x6ef598, applied by FUN_0040dfc0): "for console-type gamepads
/// with 2 directional control devices and 16 buttons". The default for a DirectInput controller (not read through
/// XInput) when nothing is saved: the XInput layout's axis codes are XInput's, so on such a pad the camera would read
/// the triggers. Its flags (force feedback, sliders, hat switch on) are padmode000's defaults already. *(Not retail:
/// the game itself has no default per device; the tool applies a pattern only when the player picks it.)*
pub const DINPUT_PADSIN: &str = "4,6,11,7,10,1,0,2,3,8,-1,-1,9,33,33,32,32,37,37,34,34,41,41,40,40,5,-1,-1,-1,-1,-1,-1";
/// Pattern A's alternate map, made as DEFAULT_PADEXSIN is from DEFAULT_PADSIN: the sticks and d-pad only.
pub const DINPUT_PADEXSIN: &str =
    "-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,33,33,32,32,37,37,34,34,41,41,40,40,-1,-1,-1,-1,-1,-1,-1";

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

/// One poll's device state: the buttons down (XInput's virtual numbers, or a DirectInput device's 0 ..= 31) and the
/// axes by code (-127 ..= 127; XInput's signs, up positive, or DirectInput's, down positive). `di`: FUN_10005a40's
/// DirectInput branch reads it (axis codes 1 ..= 10, gamepad_dinput.rs), else its XInput branch.
pub struct Device<'a> {
    pub buttons: &'a [u8],
    pub axis: &'a dyn Fn(u8) -> f32,
    pub di: bool,
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
    // [cam X, cam Y, move X, move Y, d-pad X, d-pad Y]. Axes (jump tables 0x100077fc XInput, 0x10007788 DirectInput;
    // padsin's map only): each slot's code copies ±axis into its destination; in XInput code 3 is zero and 6 none.
    let mut v = [0f32; 6];
    for (s, d) in [(17, 0), (19, 1), (21, 2), (23, 3), (4, 4), (6, 5)] {
        for code in [map.axes[s], map.axes[s + 1]] {
            if code != 0 {
                let c = code.unsigned_abs();
                let a = if dev.di || matches!(c, 1 | 2 | 4 | 5) { (dev.axis)(c) } else { 0.0 };
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
    // Slot 25 (the XInput branch drops its d-pad bits), then slot 26, then mode 2, then mode 1: the movement stick
    // drives the d-pad or the camera.
    if held & bit(slot::STICK_DPAD) != 0 {
        if !dev.di {
            held &= !DPAD;
        }
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
    // The DirectInput branch then pushes the movement stick out to the square's corners (0x10006753).
    if dev.di {
        let (x, y) = super::gamepad_dinput::square(v[2] as i32, v[3] as i32);
        (v[2], v[3]) = (x as f32, y as f32);
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
    /// For mods (`input.pad`): `stick` before a mod took any of it, and the XInput buttons that hold the confirm,
    /// cancel and menu slots in the button settings. Zeroes and none while no XInput pad plays.
    pub mapped: [f32; 4],
    pub slots: [Option<u8>; 3],
}

/// What the mods take from the game this frame (lua/input.rs pad_filter): buttons (VIRTUAL's numbers) and the
/// movement and camera sticks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Taken {
    pub buttons: Vec<u8>,
    pub sticks: [bool; 2],
}

/// The pad word and `read`'s stick values (camera X / Y, movement X / Y) as the game gets them: a stick a mod took
/// (`sticks`: movement, camera) reads as centred, and the buttons on its slots (21 ..= 24 movement, 17 ..= 20 camera)
/// as up. The game reads the walk and the camera from those bits as well as from the stick bytes (Input::axis), so
/// taking a stick takes both.
fn untaken(pad: u64, v: [f32; 4], sticks: [bool; 2]) -> (u64, [f32; 4]) {
    let (m, c) = (if sticks[0] { 0.0 } else { 1.0 }, if sticks[1] { 0.0 } else { 1.0 });
    let slots = |taken: bool, from: u8| if taken { 0xf << from } else { 0u64 };
    (pad & !(slots(sticks[0], 21) | slots(sticks[1], 17)), [v[0] * c, v[1] * c, v[2] * m, v[3] * m])
}

/// The XInput button (VIRTUAL's number) that holds `slot` in a map, the lowest when several do.
fn slot_button(map: &PadMap, slot: u8) -> Option<u8> {
    (0..VIRTUAL.len() as u8).find(|&b| map.buttons[b as usize] == bit(slot))
}

/// The pad as the Controller window assigns from it (modern/padwin.rs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    /// Buttons that went down this frame (padsin's numbers).
    pub pressed: Vec<u8>,
    /// Axes by padsin's code, -127 ..= 127: an XInput pad's sticks (1, 2, 4, 5, up positive), or a DirectInput
    /// device's axes that moved since the window started waiting (1 ..= 10, down positive).
    pub axes: Vec<(u8, f32)>,
    /// The pad is a DirectInput device.
    pub di: bool,
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
    /// padmode000's [5] (an XInput controller through XInput), [2] (sliders) and [3] (hat switch).
    pub xinput: bool,
    pub sliders: bool,
    pub hat: bool,
    /// padguid000, the controller that plays ("" the first).
    pub padguid: String,
    pub source: Source,
    /// config.json holds a map (either): Use Retail Settings has something to clear.
    pub saved: bool,
}

impl Settings {
    /// With nothing saved and a DirectInput controller playing, FFXiPadConfig's Pattern A instead of its XInput
    /// setup (DINPUT_PADSIN).
    pub fn for_dinput(mut self, dinput: bool) -> Settings {
        if dinput && self.source == Source::Default {
            self.padsin = DINPUT_PADSIN.into();
            if self.padexsin == DEFAULT_PADEXSIN {
                self.padexsin = DINPUT_PADEXSIN.into();
            }
        }
        self
    }
}

/// What a frame's settings are. config.json's maps win (the Controller window writes them); else retail's registry
/// values, whose padmode000 also says whether the pad is on ([0]: off, FUN_10002840 leaves the maps empty) and plays in
/// the background ([4]) and rumbles ([1]); else FFXiPadConfig's XInput layout. config gamepad off always turns it
/// off, and so does gamepad_rumble the rumble.
/// A config.json gamepad_padexsin is used whatever the main map's source.
/// config.json's padguid and XInput / sliders / hat options win, else retail's whatever the maps' source (the game reads padguid000 on its own,
/// FUN_10002840).
pub fn settings(cfg: &Config, retail: Option<&crate::padreg::Retail>) -> Settings {
    let mut s = settings_of(cfg, retail);
    if !cfg.gamepad_padexsin.trim().is_empty() {
        (s.padexsin, s.saved) = (cfg.gamepad_padexsin.clone(), true);
    }
    // padmode000's XInput / sliders / hat: config.json's when the Controller window set them, whatever the maps' source.
    s.xinput = cfg.gamepad_xinput.unwrap_or(s.xinput);
    s.sliders = cfg.gamepad_sliders.unwrap_or(s.sliders);
    s.hat = cfg.gamepad_hat.unwrap_or(s.hat);
    s.padguid = if !cfg.gamepad_padguid.trim().is_empty() {
        cfg.gamepad_padguid.trim().to_string()
    } else {
        retail.and_then(|r| r.padguid.clone()).unwrap_or_default()
    };
    // The controller choice saved is a saved setting too: Use Retail Settings has something to take back.
    s.saved |= device_saved(cfg);
    s
}

/// The Controller window saved a controller or its options (config.json's padguid / XInput / sliders / hat).
pub fn device_saved(cfg: &Config) -> bool {
    !cfg.gamepad_padguid.trim().is_empty()
        || cfg.gamepad_xinput.is_some()
        || cfg.gamepad_sliders.is_some()
        || cfg.gamepad_hat.is_some()
}

fn settings_of(cfg: &Config, retail: Option<&crate::padreg::Retail>) -> Settings {
    let (on, background, rumble) = (cfg.gamepad, cfg.gamepad_background, cfg.gamepad_rumble);
    // padmode000's [5] / [2] / [3] from retail's settings whatever the maps' source (on without them).
    let flag = |i: usize| retail.and_then(|r| r.mode(i)) != Some(false);
    let (xinput, sliders, hat, padguid) = (flag(5), flag(2), flag(3), String::new());
    if !cfg.gamepad_padsin.trim().is_empty() {
        let padexsin = DEFAULT_PADEXSIN.to_string();
        return Settings {
            padsin: cfg.gamepad_padsin.clone(),
            padexsin,
            on,
            background,
            rumble,
            xinput,
            sliders,
            hat,
            padguid,
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
            xinput,
            sliders,
            hat,
            padguid,
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
        xinput,
        sliders,
        hat,
        padguid,
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
    /// A DirectInput device's axes by code 1 ..= 10 (gamepad_dinput::axis): its buttons are then its own numbers.
    pub di: Option<[f32; 10]>,
}

/// A gilrs pad as XInput reads one: the sticks unfiltered (gilrs's default filters are off: its dead zone rescales
/// radially, retail takes the raw value, v x 127 >> 15), and a trigger held at any value above 0 (FUN_10005a40 tests
/// the trigger byte against 0; gilrs's own press threshold is 0.75). A pad with no SDL mapping (gilrs's evdev
/// default, e.g. Sunshine's virtual Xbox 360 pad, GUID 0500..., which the DB lists only as USB 0300...) in the
/// XInput shape (xpad's, uinput's: the sticks on X / Y and RX / RY) reports its triggers as the Z axes (ABS_Z /
/// ABS_RZ, -1 at rest): those stand in for LT / RT. Without RX / RY the Z axes are the right stick (hid-generic's
/// "USB Gamepad" clones), resting at 0: never triggers, as retail reads such a pad through DirectInput, where Z / Rz
/// are only what padsin gives them.
fn gilrs_pad(pad: &gilrs::Gamepad) -> Pad {
    let xinput_shaped = pad.axis_code(Axis::RightStickX).is_some() && pad.axis_code(Axis::RightStickY).is_some();
    let trigger = |b: Button| {
        let z = match b {
            Button::LeftTrigger2 => Axis::LeftZ,
            _ => Axis::RightZ,
        };
        trigger_value(pad.button_data(b).map(|d| d.value()), pad.axis_data(z).map(|d| d.value()), xinput_shaped)
    };
    let held = |b: Button| match b {
        Button::LeftTrigger2 | Button::RightTrigger2 => trigger(b) > 0.0,
        _ => pad.is_pressed(b),
    };
    Pad {
        buttons: VIRTUAL.iter().filter(|(b, _)| held(*b)).map(|(_, n)| *n).collect(),
        axes: [
            pad.value(Axis::LeftStickX),
            pad.value(Axis::LeftStickY),
            pad.value(Axis::RightStickX),
            pad.value(Axis::RightStickY),
            trigger(Button::LeftTrigger2),
            trigger(Button::RightTrigger2),
        ],
        di: None,
    }
}

/// A trigger's 0 ..= 1: its mapped button's value, else its Z axis (-1 ..= 1) rescaled to XInput's byte. gilrs maps
/// an odd range (0 ..= 255) with a +1 offset, so a released trigger reads -1 + 2/256, not -1: the byte's floor makes
/// that 0 again (as XInput's trigger byte would be), else the trigger would never come up. An axis that has sent
/// nothing yet reads none, so an untouched trigger is not held. `z_triggers`: the Z axes are the triggers (the pad has
/// its right stick on RX / RY); else they are a stick's and the trigger is its button alone.
fn trigger_value(button: Option<f32>, z: Option<f32>, z_triggers: bool) -> f32 {
    let z = z.filter(|_| z_triggers).map(|v| (((v + 1.0) / 2.0).clamp(0.0, 1.0) * 255.0).floor() / 255.0);
    button.or(z).unwrap_or(0.0).clamp(0.0, 1.0)
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
    /// XInput's pads (Windows): started once when gilrs fails or finds none, played while gilrs has none; whether
    /// the pad that plays is XInput's (the rumble goes to it).
    #[cfg(windows)]
    xinput: Option<super::gamepad_xinput::XInput>,
    #[cfg(windows)]
    xinput_tried: bool,
    #[cfg(windows)]
    xinput_plays: bool,
    /// Retail's controller choice and DirectInput read (gamepad_dinput.rs), made once the window is known; gilrs
    /// is not started while it is up. The settings it was set up with (a change is DAT_10456df0), the focus last
    /// frame (WM_ACTIVATE acquires), whether its DirectInput device plays (the rumble goes to it), the window, and
    /// the ticks not yet a retail frame (the 0x5a frames after a loss).
    #[cfg(windows)]
    dinput: Option<Box<super::gamepad_dinput::DInput>>,
    #[cfg(windows)]
    dinput_tried: bool,
    #[cfg(windows)]
    di_key: Option<(String, bool, bool, bool, bool)>,
    #[cfg(windows)]
    di_focus: Option<bool>,
    #[cfg(windows)]
    di_plays: bool,
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    di_clock: f32,
    /// The axes when the Controller window started waiting (a DirectInput slider rests away from 0).
    cap_axes: Option<[f32; 10]>,
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
    /// are the mods' and the game does not see them, and the sticks it returns read as centred.
    pub fn poll(
        &mut self,
        cfg: &Config,
        focused: bool,
        ctx: Ctx,
        dt: f32,
        mods: &mut dyn FnMut(&[u8], [f32; 6], bool) -> Taken,
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
        let set = settings(cfg, self.retail.as_ref()).for_dinput(self.plays_dinput());
        if !set.on && !capture {
            mods(&[], [0.0; 6], false);
            // The pad turned off lets the DirectInput device go (a changed padmode000, DAT_10456df0), so a background
            // exclusive acquisition does not hold it from other programs; turned on again it is picked anew.
            #[cfg(windows)]
            if self.di_key.take().is_some()
                && let Some(d) = &mut self.dinput
            {
                d.reset();
            }
            return self.release_all();
        }
        self.connect();
        #[cfg(windows)]
        if let Some(d) = &mut self.dinput
            && self.di_focus.replace(focused).is_some_and(|f| f != focused)
        {
            d.acquire();
        }
        #[cfg(windows)]
        if let (true, Some(x)) = (self.xinput_plays, &mut self.xinput) {
            x.enable(focused || set.background);
        }
        let (padsin, padexsin) = (set.padsin.as_str(), set.padexsin.as_str());
        if self.maps.as_ref().is_none_or(|m| m.0 != padsin || m.1 != padexsin) {
            self.maps =
                Some((padsin.to_string(), padexsin.to_string(), PadMap::parse(padsin), PadMap::parse(padexsin)));
        }
        let pad = match self.read_pad(&set, dt) {
            Some(p) if focused || set.background => p,
            _ => {
                mods(&[], [0.0; 6], false);
                return self.release_all();
            }
        };
        let Pad { mut buttons, axes, di } = pad;
        if capture {
            // Buttons already down when it starts (the A that asked) are not presses.
            if !self.capturing {
                (self.capturing, self.cap_prev) = (true, buttons.clone());
                self.cap_axes = di;
            }
            let pressed = buttons.iter().copied().filter(|b| !self.cap_prev.contains(b)).collect();
            // A DirectInput device's axes that moved since it started (by code), else the sticks (XInput's codes).
            let axes = match (di, self.cap_axes) {
                (Some(a), Some(base)) => (0..10)
                    .filter(|&i| (a[i] - base[i]).abs() >= super::padcfg::PUSH)
                    .map(|i| (i as u8 + 1, a[i]))
                    .collect(),
                _ => vec![(1, axes[0] * 127.0), (2, axes[1] * 127.0), (4, axes[2] * 127.0), (5, axes[3] * 127.0)],
            };
            self.cap_prev = buttons;
            mods(&[], [0.0; 6], false);
            let mut f = self.release_all();
            f.used = true;
            f.capture = Some(Capture { pressed, axes, di: di.is_some() });
            return f;
        }
        if std::mem::take(&mut self.capturing) {
            self.mute = std::mem::take(&mut self.cap_prev);
        }
        self.mute.retain(|b| buttons.contains(b));
        buttons.retain(|b| !self.mute.contains(b));
        // Mods read XInput's button numbers (lua/input.rs pad_filter): a DirectInput device's own numbers are not
        // given to them, so its buttons all go to the game.
        let taken = if di.is_some() { mods(&[], [0.0; 6], false) } else { mods(&buttons, axes, true) };
        buttons.retain(|b| !taken.buttons.contains(b));
        let axis = |code: u8| match (di, code) {
            (Some(a), 1..=10) => a[code as usize - 1],
            (Some(_), _) => 0.0,
            (None, 1) => axes[0] * 127.0,
            (None, 2) => axes[1] * 127.0,
            (None, 4) => axes[2] * 127.0,
            (None, 5) => axes[3] * 127.0,
            _ => 0.0,
        };
        let (_, _, map, alt) = self.maps.as_ref().unwrap();
        // FUN_10005320 adds the alternate map's mode flags to padsin's.
        let modes = (map.dpad_mode || alt.dpad_mode, map.camera_mode || alt.camera_mode);
        if !modes.0 && !modes.1 {
            self.mode = 0;
        }
        let dev = Device { buttons: &buttons, axis: &axis, di: di.is_some() };
        let (held, v) = read(map, alt, &dev, self.mode, modes, cfg.gamepad_deadzone);
        if held & !self.prev & bit(slot::STICK_MODE) != 0 {
            self.mode = next_mode(self.mode, modes);
        }
        let slots = [slot::CONFIRM, slot::CANCEL, slot::MENU].map(|s| slot_button(map, s));
        let menu = menu || palette || party_cursor || !field;
        // The party cursor reads egui keys only: its confirm / cancel / d-pad leave the field's pad word.
        let (keys, pad_bits) =
            if keyboard { (Vec::new(), held) } else { keys_of(held, menu, palette || party_cursor, tabs && menu) };
        let mut f = self.step(held, &keys, dt);
        self.prev = held;
        // A stick a mod took: its bytes and the buttons on its slots stay from the game.
        let (pad_bits, game) = untaken(pad_bits, v, taken.sticks);
        f.pad = pad_bits;
        f.used = held != 0 || v.iter().any(|&a| a != 0.0);
        let sticks = |v: [f32; 4]| [v[2] / 128.0, v[3] / 128.0, v[0] / 128.0, v[1] / 128.0];
        // Outside the field the sticks do nothing (they never drive menus).
        f.stick = if field { sticks(game) } else { [0.0; 4] };
        // Mods are given an XInput pad only (above).
        if di.is_none() {
            f.mapped = sticks(v);
            f.slots = slots;
        }
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
        // A DirectInput device takes FUN_10007ae0's level itself (level x 10000 / 256, a quarter of XInput's).
        #[cfg(windows)]
        if self.di_plays {
            if let Some(d) = &mut self.dinput {
                d.force(level as i32 / 4);
            }
            return;
        }
        #[cfg(windows)]
        if self.xinput_plays {
            if let Some(x) = &mut self.xinput {
                x.motors(level);
            }
            return;
        }
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

    /// Starts gilrs, again every RETRY while it fails, and once when gilrs failed or found no pad SDL (Linux) or
    /// XInput (Windows).
    fn connect(&mut self) {
        #[cfg(windows)]
        if !self.dinput_tried && self.hwnd != 0 {
            self.dinput_tried = true;
            match super::gamepad_dinput::DInput::new(self.hwnd) {
                Ok(mut d) => {
                    for l in d.list() {
                        log::info!("gamepad (DirectInput): {} {}", l.guid.text(), l.name);
                    }
                    self.dinput = Some(d);
                }
                Err(e) => log::warn!("gamepad: {e}; gilrs instead"),
            }
        }
        #[cfg(windows)]
        if self.dinput.is_some() || (!self.dinput_tried && self.hwnd != 0) {
            return;
        }
        let now = Instant::now();
        if self.gilrs.is_none() && self.next_try.is_none_or(|t| now >= t) {
            self.next_try = Some(now + RETRY);
            // No default filters (dead zone, jitter): read_pad keeps the d-pad one itself.
            match gilrs::GilrsBuilder::new().with_default_filters(false).set_update_state(false).build() {
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
        #[cfg(windows)]
        if !self.xinput_tried && self.gilrs.as_ref().is_none_or(|g| g.gamepads().next().is_none()) {
            self.xinput_tried = true;
            match super::gamepad_xinput::XInput::new() {
                Ok(x) => self.xinput = Some(x),
                Err(e) => log::warn!("gamepad: XInput: {e}"),
            }
        }
    }

    /// The pad that plays: on Windows retail's (read_retail); else gilrs's (padguid's when connected, else the last
    /// one touched), else SDL's (Linux) or XInput's (Windows without DirectInput).
    fn read_pad(&mut self, set: &Settings, dt: f32) -> Option<Pad> {
        #[cfg(windows)]
        if self.dinput.is_some() {
            return self.read_retail(set, dt);
        }
        let _ = dt;
        #[cfg(target_os = "linux")]
        if let Some(s) = &mut self.sdl {
            s.update();
        }
        if let Some(g) = &mut self.gilrs {
            use gilrs::ev::filter::{Filter as _, axis_dpad_to_button};
            while let Some(ev) = g.next_event() {
                // A hat reported as axes is still the d-pad's buttons (gilrs's d-pad filter, the one kept).
                let Some(ev) = Some(ev).filter_ev(&axis_dpad_to_button, g).filter(|e| !e.is_dropped()) else {
                    continue;
                };
                g.update(&ev);
                if let gilrs::EventType::Connected = ev.event {
                    log::info!("gamepad {} connected: {}", ev.id, g.gamepad(ev.id).name());
                }
            }
            // FUN_100057d0's choice on gilrs's list: padguid's pad, else the first one (not the last one touched).
            // *(Not retail: padguid000 names a DirectInput instance, which gilrs has not; here it holds gilrs's pad
            // UUID as the Controller window saved it, and "first" is gilrs's order, not DirectInput's.)*
            let named = g.gamepads().find(|(_, p)| !set.padguid.is_empty() && uuid_text(p.uuid()) == set.padguid);
            self.active = named.or_else(|| g.gamepads().next()).map(|p| p.0);
            if let Some(p) = self.active.and_then(|id| g.connected_gamepad(id)).map(|p| gilrs_pad(&p)) {
                #[cfg(windows)]
                self.xinput_off();
                return Some(p);
            }
        }
        // XInput only while gilrs has no pad: a pad both see plays once.
        #[cfg(windows)]
        if let Some(x) = &mut self.xinput {
            x.update(false);
            if let Some(p) = x.state() {
                self.xinput_plays = true;
                return Some(p);
            }
        }
        #[cfg(windows)]
        self.xinput_off();
        #[cfg(target_os = "linux")]
        if let Some(p) = self.sdl.as_ref().and_then(|s| s.state()) {
            return Some(p);
        }
        None
    }

    /// FUN_10005a40 with retail's controller choice (gamepad_dinput.rs): a change of padguid000 or of the options
    /// that pick it lets the device go and picks again (DAT_10456df0); with no device the XInput branch reads the
    /// picked slot, or with nothing picked the pick runs again after 0x5a frames; else the DirectInput branch.
    #[cfg(windows)]
    fn read_retail(&mut self, set: &Settings, dt: f32) -> Option<Pad> {
        use super::gamepad_dinput as di;
        let key = (set.padguid.clone(), set.on, set.xinput, set.rumble, set.background);
        let frame = super::camera::retail_frame(&mut self.di_clock, dt, 8.0).is_some();
        let want = di::Guid::parse(&set.padguid);
        let again = if self.di_key.as_ref() != Some(&key) {
            self.di_key = Some(key);
            self.dinput.as_mut()?.reset();
            true
        } else {
            // Every 0x5b frames with nothing picked the enumeration runs again (a thread's, DInput::probe); the setup
            // follows once it found a controller it would pick.
            let d = self.dinput.as_mut()?;
            if !d.has_device() && !d.pick.xinput && frame && d.lost_frame() {
                d.probe();
            }
            d.probed().is_some_and(|found| di::would_pick(&want, &found))
        };
        if again {
            // FUN_10008d90 / FUN_10008e10: the XInput DLL, with the option.
            let loaded = set.xinput.then(|| {
                if self.xinput.is_none() && !self.xinput_tried {
                    self.xinput_tried = true;
                    match super::gamepad_xinput::XInput::new() {
                        Ok(x) => self.xinput = Some(x),
                        Err(e) => log::warn!("gamepad: XInput: {e}"),
                    }
                }
                self.xinput.is_some()
            });
            self.dinput.as_mut()?.setup(&want, loaded, set.rumble, set.background);
        }
        if self.dinput.as_ref()?.has_device() {
            self.xinput_off();
            let d = self.dinput.as_mut()?;
            let s = d.read();
            self.di_plays = d.has_device();
            let s = s?;
            let a: [f32; 10] = std::array::from_fn(|i| di::axis(&s, i as u8 + 1, set.sliders, set.hat) as f32);
            return Some(Pad {
                buttons: di::buttons(&s),
                axes: [a[0] / 127.0, -a[1] / 127.0, a[3] / 127.0, -a[4] / 127.0, 0.0, 0.0],
                di: Some(a),
            });
        }
        self.di_plays = false;
        // The XInput branch at the picked slot; *(not retail: with nothing picked, slot 0, #423's fallback for front
        // ends that hook XInput only)*.
        let pick = self.dinput.as_ref()?.pick;
        let slot = if pick.xinput { pick.slot.max(0) as u32 } else { 0 };
        if self.xinput.is_none() && !self.xinput_tried {
            self.xinput_tried = true;
            match super::gamepad_xinput::XInput::new() {
                Ok(x) => self.xinput = Some(x),
                Err(e) => log::warn!("gamepad: XInput: {e}"),
            }
        }
        let x = self.xinput.as_mut()?;
        x.set_slot(slot);
        x.update(pick.xinput);
        let p = x.state();
        self.xinput_plays = p.is_some();
        p
    }

    /// The window DirectInput's cooperative level is set on (retail: the game window, DAT_103d4d64 +0x18).
    #[cfg(windows)]
    pub fn set_window(&mut self, window: &winit::window::Window) {
        if self.hwnd == 0 {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = window.window_handle()
                && let RawWindowHandle::Win32(h) = h.as_raw()
            {
                self.hwnd = h.hwnd.get();
            }
        }
    }

    /// The controllers the Controller window lists: (padguid000 text, name). On Windows DirectInput's attached game
    /// controllers (FFXiPadConfig's list); else gilrs's pads by UUID *(not retail)*.
    pub fn devices(&mut self) -> Vec<(String, String)> {
        #[cfg(windows)]
        if let Some(d) = &mut self.dinput {
            return d.list().into_iter().map(|l| (l.guid.text(), l.name)).collect();
        }
        self.gilrs.iter().flat_map(|g| g.gamepads()).map(|(_, p)| (uuid_text(p.uuid()), p.name().to_string())).collect()
    }

    /// The pad that plays is a DirectInput device (its buttons and axes are DirectInput's numbers).
    pub fn plays_dinput(&self) -> bool {
        #[cfg(windows)]
        return self.di_plays;
        #[cfg(not(windows))]
        false
    }

    /// XInput's pad no longer plays: its motors stop.
    #[cfg(windows)]
    fn xinput_off(&mut self) {
        if let (true, Some(x)) = (std::mem::take(&mut self.xinput_plays), &mut self.xinput) {
            x.motors(0);
        }
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
        #[cfg(windows)]
        if let (true, Some(d)) = (self.di_plays, &self.dinput) {
            return vec![d.name.clone()];
        }
        let mut v: Vec<(bool, String)> = self
            .gilrs
            .iter()
            .flat_map(|g| g.gamepads())
            .map(|(id, p)| (Some(id) != self.active, p.name().to_string()))
            .collect();
        v.sort();
        let mut v: Vec<String> = v.into_iter().map(|p| p.1).collect();
        // SDL and XInput see gilrs's pads too: their list only when gilrs has none.
        #[cfg(target_os = "linux")]
        if let (true, Some(s)) = (v.is_empty(), &self.sdl) {
            v = s.names();
        }
        #[cfg(windows)]
        if let (true, Some(x)) = (v.is_empty(), &self.xinput) {
            v = x.names();
        }
        v
    }

    fn release_all(&mut self) -> Frame {
        // The DirectInput effect is infinite: it stops before the device stops taking the rumble.
        #[cfg(windows)]
        {
            if self.di_plays
                && let Some(d) = &mut self.dinput
            {
                d.force(0);
            }
            self.di_plays = false;
        }
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

/// A gilrs pad's UUID in padguid000's form.
fn uuid_text(u: [u8; 16]) -> String {
    let d = |i: usize| u[i];
    super::gamepad_dinput::Guid {
        data1: u32::from_le_bytes([d(0), d(1), d(2), d(3)]),
        data2: u16::from_le_bytes([d(4), d(5)]),
        data3: u16::from_le_bytes([d(6), d(7)]),
        data4: [d(8), d(9), d(10), d(11), d(12), d(13), d(14), d(15)],
    }
    .text()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev<'a>(buttons: &'a [u8], axis: &'a dyn Fn(u8) -> f32) -> Device<'a> {
        Device { buttons, axis, di: false }
    }

    const DEAD: f32 = 42.0 / 127.0;

    /// A stick a mod took (input.take_sticks: movement, camera) is centred for the game; the other is left alone.
    #[test]
    fn taken_sticks_are_centred() {
        let v = [10.0, -20.0, 30.0, -40.0];
        assert_eq!(untaken(0, v, [false, false]).1, v);
        assert_eq!(untaken(0, v, [true, false]).1, [10.0, -20.0, 0.0, 0.0]);
        assert_eq!(untaken(0, v, [false, true]).1, [0.0, 0.0, 30.0, -40.0]);
        assert_eq!(untaken(0, v, [true, true]).1, [0.0; 4]);
    }

    /// Movement and the camera on buttons (the default padsin with move right, slot 21, on d-pad right and camera
    /// right, slot 17, on d-pad left): the push is in the pad word as well as the stick bytes, and the game walks by
    /// the bit (Input::axis 4). A taken stick takes its slots' bits too; other buttons and the other stick stay.
    #[test]
    fn taken_sticks_take_their_buttons() {
        let m =
            PadMap::parse("8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,4,-36,-36,35,5,6,7,-1,-1,11,-1,-1,-1,-1,-1,-1");
        let alt = PadMap::parse("");
        // D-pad right and left, and A (confirm).
        let (held, v) = read(&m, &alt, &dev(&[4, 5, 3], &|_| 0.0), 0, (false, false), DEAD);
        assert_eq!(held, bit(21) | bit(17) | bit(slot::CONFIRM));
        assert_eq!(v, [127.0, 0.0, 127.0, 0.0]);
        assert_eq!(untaken(held, v, [false, false]), (held, v));
        assert_eq!(untaken(held, v, [true, false]), (bit(17) | bit(slot::CONFIRM), [127.0, 0.0, 0.0, 0.0]));
        assert_eq!(untaken(held, v, [false, true]), (bit(21) | bit(slot::CONFIRM), [0.0, 0.0, 127.0, 0.0]));
        assert_eq!(untaken(held, v, [true, true]), (bit(slot::CONFIRM), [0.0; 4]));
        // Every slot of a stick: 21 ..= 24 movement, 17 ..= 20 camera.
        let all = (17..=24).fold(0, |w, s| w | bit(s));
        assert_eq!(untaken(all, [0.0; 4], [true, false]).0, bit(17) | bit(18) | bit(19) | bit(20));
        assert_eq!(untaken(all, [0.0; 4], [false, true]).0, bit(21) | bit(22) | bit(23) | bit(24));
    }

    /// The default button settings' confirm, cancel and menu, for input.pad().map: A, B and X.
    #[test]
    fn slot_buttons_of_the_default_map() {
        let m = PadMap::parse(DEFAULT_PADSIN);
        assert_eq!([slot::CONFIRM, slot::CANCEL, slot::MENU].map(|s| slot_button(&m, s)), [Some(3), Some(0), Some(1)]);
        assert_eq!(slot_button(&PadMap::parse(""), slot::CANCEL), None);
    }

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

    /// A controller choice or option saved alone (no maps) counts as saved settings, so Use Retail Settings is offered
    /// and, cleared, retail's padmode000 decides again.
    #[test]
    fn device_options_count_as_saved() {
        let retail = crate::padreg::Retail {
            padmode: Some("1,0,0,0,0,1".into()),
            padsin: Some(DEFAULT_PADSIN.into()),
            ..Default::default()
        };
        let mut cfg = Config::default();
        let s = settings(&cfg, Some(&retail));
        assert!(!s.saved && s.xinput);
        cfg.gamepad_xinput = Some(false);
        let s = settings(&cfg, Some(&retail));
        assert!(s.saved && !s.xinput);
        cfg.gamepad_xinput = None;
        cfg.gamepad_padguid = "{55DDF31E-8F9C-11F1-8001-000044455354}".into();
        assert!(settings(&cfg, Some(&retail)).saved);
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
    /// FUN_10005a40's DirectInput branch on the same map: every axis code reads (9, the hat, too), a diagonal
    /// goes out to the corner, and slot 25 keeps the d-pad buttons.
    #[test]
    fn directinput_branch() {
        let mut m = PadMap::parse("");
        (m.axes[21], m.axes[23]) = (1, 2);
        (m.buttons[0], m.buttons[1]) = (bit(slot::STICK_DPAD), bit(slot::UP));
        let diag = |c: u8| if c == 1 || c == 2 { 90.0 } else { 0.0 };
        let di = Device { buttons: &[], axis: &diag, di: true };
        assert_eq!(read(&m, &m, &di, 0, (false, false), DEAD).1[2..], [127.0, 127.0]);
        assert_eq!(read(&m, &m, &dev(&[], &diag), 0, (false, false), DEAD).1[2..], [90.0, 90.0]);
        let still = |_: u8| 0.0;
        let di = Device { buttons: &[0, 1], axis: &still, di: true };
        assert_eq!(read(&m, &m, &di, 0, (false, false), DEAD).0 & DPAD, bit(slot::UP));
        assert_eq!(read(&m, &m, &dev(&[0, 1], &still), 0, (false, false), DEAD).0 & DPAD, 0);
        m.axes[21] = 9;
        let hat = |c: u8| if c == 9 { 127.0 } else { 0.0 };
        let di = Device { buttons: &[], axis: &hat, di: true };
        assert_eq!(read(&m, &m, &di, 0, (false, false), DEAD).1[2], 127.0);
        assert_eq!(read(&m, &m, &dev(&[], &hat), 0, (false, false), DEAD).1[2], 0.0);
    }

    /// padmode000's XInput, sliders and hat flags, and padguid000: config.json's guid first, else retail's.
    #[test]
    fn device_settings() {
        let retail = crate::padreg::Retail {
            padmode: Some("1,1,0,1,0,0".into()),
            padsin: Some(DEFAULT_PADSIN.into()),
            padguid: Some("{55DDF31E-8F9C-11F1-8001-000044455354}".into()),
            ..Default::default()
        };
        let mut cfg = Config::default();
        let s = settings(&cfg, Some(&retail));
        assert_eq!((s.xinput, s.sliders, s.hat), (false, false, true));
        // Saved maps keep retail's flags; the window's own choice wins over them either way.
        cfg.gamepad_padsin = DEFAULT_PADSIN.into();
        assert!(!settings(&cfg, Some(&retail)).xinput);
        cfg.gamepad_xinput = Some(true);
        assert!(settings(&cfg, Some(&retail)).xinput);
        cfg.gamepad_padsin.clear();
        cfg.gamepad_xinput = None;
        assert_eq!(s.padguid, "{55DDF31E-8F9C-11F1-8001-000044455354}");
        let s = settings(&cfg, None);
        assert_eq!((s.xinput, s.sliders, s.hat, s.padguid.as_str()), (true, true, true, ""));
        cfg.gamepad_padguid = "{00000001-0000-0000-0000-000000000000}".into();
        assert_eq!(settings(&cfg, Some(&retail)).padguid, cfg.gamepad_padguid);
    }

    /// A DirectInput controller with nothing saved takes the tool's Pattern A; saved or retail maps stay.
    #[test]
    fn dinput_default_pattern() {
        let cfg = Config::default();
        let s = settings(&cfg, None).for_dinput(true);
        assert_eq!((s.padsin.as_str(), s.padexsin.as_str()), (DINPUT_PADSIN, DINPUT_PADEXSIN));
        assert_eq!(settings(&cfg, None).for_dinput(false).padsin, DEFAULT_PADSIN);
        let saved = Config { gamepad_padsin: "1".into(), ..Default::default() };
        assert_eq!(settings(&saved, None).for_dinput(true).padsin, "1");
        // The alternate map: padsin's sticks and d-pad (positions 13..25), nothing else.
        let (m, alt): (Vec<&str>, Vec<&str>) =
            (DINPUT_PADSIN.split(',').collect(), DINPUT_PADEXSIN.split(',').collect());
        assert!((0..32).all(|i| alt[i] == if (13..25).contains(&i) { m[i] } else { "-1" }));
    }

    #[test]
    fn uuid_as_padguid() {
        let u = [0x1e, 0xf3, 0xdd, 0x55, 0x9c, 0x8f, 0xf1, 0x11, 0x80, 0x01, 0, 0, 0x44, 0x45, 0x53, 0x54];
        assert_eq!(uuid_text(u), "{55DDF31E-8F9C-11F1-8001-000044455354}");
    }

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
    fn unmapped_triggers_read_the_z_axes() {
        let t = |b, z| super::trigger_value(b, z, true);
        // Mapped: the button's value. Unmapped: the Z axis from -1 (rest) to 1 (full); none sent yet, not held.
        assert_eq!(t(Some(0.4), Some(1.0)), 0.4);
        assert_eq!(t(None, Some(-1.0)), 0.0);
        assert_eq!(t(None, Some(1.0)), 1.0);
        assert!(t(None, Some(-0.9)) > 0.0);
        assert_eq!(t(None, None), 0.0);
        // gilrs on a 0 ..= 255 axis: raw r reads (r + 1) / 256 x 2 - 1. Released (0) is not held; 1 and up are.
        let g = |r: f32| (r + 1.0) / 256.0 * 2.0 - 1.0;
        assert_eq!(t(None, Some(g(0.0))), 0.0);
        assert!(t(None, Some(g(1.0))) > 0.0);
        assert_eq!(t(None, Some(g(255.0))), 1.0);
    }

    #[test]
    fn z_axes_of_a_right_stick_are_no_triggers() {
        use super::trigger_value as t;
        // hid-generic's clones: the right stick on Z / RZ (no RX / RY), resting at 0 and pushed anywhere. Never held.
        for v in [0.0, -1.0, 0.4, 1.0] {
            assert_eq!(t(None, Some(v), false), 0.0);
        }
        // A trigger button it does have still counts.
        assert_eq!(t(Some(0.6), Some(0.0), false), 0.6);
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
