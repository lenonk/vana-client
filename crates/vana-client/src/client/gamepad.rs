//! Game controllers, through retail's own pad path (docs/re/gamepad/findings.md). FFXiMain reads the pad each frame
//! (FUN_10005a40) into the input object's 64-bit pad word, bit n = "slot" n, which the key functions test (confirm
//! slot 0, cancel slot 3, the d-pad slots 4 ..= 7 ...), and into four stick bytes: camera X / Y (+0x250 / +0x251) and
//! movement X / Y (+0x252 / +0x253). Which physical button or axis feeds which slot is the registry's `padsin000`
//! (FFXiPadConfig.exe's button settings), read here from config.json `gamepad_padsin` in the same format.
//!
//! vana-client draws its windows with egui keys, not key functions, so a slot whose keyboard key a window reads also
//! sends that key's egui event, as the key itself would: confirm Enter, cancel Escape, the main menu Num-, and with a
//! window or a macro palette up (or outside the field) the d-pad the arrows. The macro triggers (slots 9 / 13, LT / RT)
//! are key function 0xa8, which retail reads together with Ctrl / Alt: in a window they ride on the arrows as Ctrl
//! (one held: page) or Alt (both: 0xb5 / 0xb6, the next / previous bag); in the field a press toggles the Ctrl / Alt
//! palette (FUN_101a2e80), whose cursor the d-pad, A and B then drive (docs/re/gamepad/window_fns.md).

use crate::config::Config;
use gilrs::{Axis, Button, GamepadId, Gilrs};
use winit::keyboard::KeyCode;

/// FFXiPadConfig's XInput layout (Steven's registry, 2026-11-02): A confirm, B cancel, X main menu, Y active window,
/// LB autorun, LT / RT macros, L3 rest / lock on, R3 first person, RB slot 12, Start log out, Back hide windows, d-pad,
/// left stick moves, right stick turns the camera (Y axes inverted, XInput's up being positive).
pub const DEFAULT_PADSIN: &str = "8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1";
/// The alternate map used while slot 34 is held (padexsin000; Steven's: the sticks and d-pad only).
pub const DEFAULT_PADEXSIN: &str = "-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-33,-33,32,32,-36,-36,35,35,6,7,5,4,-1,-1,-1,-1,-1,-1,-1";

/// padsin position -> slot (0x1034f930).
const PERM: [u8; 32] = [8, 9, 14, 13, 10, 3, 1, 0, 2, 16, 25, 26, 15, 23, 24, 22, 21, 19, 20, 18, 17, 6, 7, 5, 4, 12, 31, 32, 33, 34, 35, 36];

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

/// Slots the key functions name (keyfn_table.md).
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

/// A parsed padsin (FUN_10005130): the pad bits of each virtual button, and the signed axis code (1 lX, 2 lY, 3 lZ,
/// 4 lRx, 5 lRy, 6 lRz; negative inverted; 0 none) of each slot that takes one (4 ..= 7, 17 ..= 24).
#[derive(Clone, Debug, PartialEq)]
pub struct PadMap {
    pub buttons: [u64; 32],
    pub axes: [i8; 37],
}

impl PadMap {
    pub fn parse(s: &str) -> PadMap {
        let mut slotval = [-1i32; 37];
        for (p, tok) in s.split([',', ' ']).filter(|t| !t.is_empty()).take(PERM.len()).enumerate() {
            slotval[PERM[p] as usize] = tok.trim().parse().unwrap_or(-1);
        }
        let mut m = PadMap { buttons: [0; 32], axes: [0; 37] };
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
        m
    }
}

/// One poll's device state: the virtual buttons down and the axes (-127 ..= 127, XInput's signs: up positive).
pub struct Device<'a> {
    pub buttons: &'a [u8],
    pub axis: &'a dyn Fn(u8) -> f32,
}

/// What FUN_10005a40 makes of a poll: the held pad word and the stick values (-127 ..= 127): camera X / Y, movement
/// X / Y. `mode` is DAT_10455c3e (slot 31 cycles it).
pub fn read(map: &PadMap, alt: &PadMap, dev: &Device, mode: u8, dead: f32) -> (u64, [f32; 4]) {
    let use_alt = dev.buttons.iter().any(|&b| map.buttons.get(b as usize) == Some(&bit(slot::ALT_MAP)));
    let mut held = 0u64;
    for &b in dev.buttons {
        let m = if use_alt && map.buttons[b as usize] != bit(slot::ALT_MAP) { alt } else { map };
        held |= m.buttons.get(b as usize).copied().unwrap_or(0);
    }
    // [cam X, cam Y, move X, move Y, d-pad X, d-pad Y]
    let mut v = [0f32; 6];
    // Buttons on a stick slot push that way.
    for (s, d, x) in [(17, 0, 127.0), (18, 0, -127.0), (19, 1, -127.0), (20, 1, 127.0), (21, 2, 127.0), (22, 2, -127.0), (23, 3, -127.0), (24, 3, 127.0), (4, 4, 127.0), (5, 4, -127.0), (6, 5, -127.0), (7, 5, 127.0)] {
        if held & bit(s) != 0 {
            v[d] = x;
        }
    }
    // Axes (jump table 0x10007788): each slot's code copies ±axis into its destination (17 / 18 camera X, 19 / 20
    // camera Y, 21 / 22 movement X, 23 / 24 movement Y, 4 / 5 d-pad X, 6 / 7 d-pad Y); a negative code inverts.
    let m = if use_alt { alt } else { map };
    for (s, d) in [(17, 0), (19, 1), (21, 2), (23, 3), (4, 4), (6, 5)] {
        for code in [m.axes[s], m.axes[s + 1]] {
            if code != 0 {
                v[d] = (dev.axis)(code.unsigned_abs()) * code.signum() as f32;
            }
        }
    }
    // Slot 25 / mode 1: the movement stick drives the d-pad; slot 26 / mode 2: the camera.
    if held & bit(slot::STICK_DPAD) != 0 || mode == 1 {
        (v[4], v[5], v[2], v[3]) = (v[2], v[3], 0.0, 0.0);
    } else if held & bit(slot::STICK_CAMERA) != 0 || mode == 2 {
        (v[0], v[1], v[2], v[3]) = (v[2], v[3], 0.0, 0.0);
    }
    let dead = dead * 127.0;
    for x in &mut v {
        if x.abs() < dead {
            *x = 0.0;
        }
    }
    // Movement past 9677 (98.4²) is rescaled (factor not decoded; ours: to the edge, 127) and clamped.
    let len2 = v[2] * v[2] + v[3] * v[3];
    if len2 > 9677.0 {
        let s = 127.0 / len2.sqrt();
        v[2] *= s;
        v[3] *= s;
    }
    for x in &mut v {
        *x = x.clamp(-127.0, 127.0);
    }
    // The d-pad from an axis.
    held |= if v[4] >= 64.0 { bit(slot::RIGHT) } else if v[4] <= -64.0 { bit(slot::LEFT) } else { 0 };
    held |= if v[5] >= 64.0 { bit(slot::DOWN) } else if v[5] <= -64.0 { bit(slot::UP) } else { 0 };
    (held, [v[0], v[1], v[2], v[3]])
}

/// The egui keys a held pad word stands for, and the pad word left for the key functions. `menu`: the d-pad is the
/// arrows; `palette`: a macro palette is up, and takes confirm / cancel / the d-pad from the key functions; `tabs`: a
/// window has the keys, so LB / RB (slots 8 / 12, nothing in retail's list windows) are Shift+Tab / Tab, the previous /
/// next tab of the modern windows.
pub fn keys_of(held: u64, menu: bool, palette: bool, tabs: bool) -> (Vec<KeyCode>, u64) {
    let mut keys = Vec::new();
    let (ctrl, alt) = (held & bit(slot::MACRO_CTRL) != 0, held & bit(slot::MACRO_ALT) != 0);
    // 0xa8 (either trigger) is the page modifier, 0xb5 / 0xb6 (both, as Alt+Right / Left) the bag switch. The right-hand
    // codes only mark the egui modifier.
    if menu && (ctrl || alt) {
        keys.push(if ctrl && alt { KeyCode::AltRight } else { KeyCode::ControlRight });
    }
    for (s, k) in [(slot::CONFIRM, KeyCode::Enter), (slot::CANCEL, KeyCode::Escape), (slot::MENU, KeyCode::NumpadSubtract)] {
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
        for (s, k) in [(slot::UP, KeyCode::ArrowUp), (slot::DOWN, KeyCode::ArrowDown), (slot::LEFT, KeyCode::ArrowLeft), (slot::RIGHT, KeyCode::ArrowRight)] {
            if held & bit(s) != 0 {
                keys.push(k);
            }
        }
    }
    let taken = if palette { bit(slot::CONFIRM) | bit(slot::CANCEL) | bit(slot::UP) | bit(slot::DOWN) | bit(slot::LEFT) | bit(slot::RIGHT) } else { 0 };
    (keys, held & !taken)
}

fn nav(k: KeyCode) -> bool {
    matches!(k, KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight)
}

/// Retail's pad repeat (mgr +0x94 = 16 ticks, then every 2), for the windows' arrows.
const REPEAT_AFTER: f32 = 16.0 / 60.0;
const REPEAT_EVERY: f32 = 2.0 / 60.0;

/// A frame's worth of pad input for the client.
#[derive(Default)]
pub struct Frame {
    /// The pad word for the key functions (`Input::set_pad`).
    pub pad: u64,
    /// Movement X / Y, camera X / Y in -1 ..= 1 (the stick bytes' (b - 0x80) / 128), for `Input::set_stick`.
    pub stick: [f32; 4],
    /// A macro trigger went down with the macro gate open: 1 LT (the Ctrl palette), 2 RT (Alt).
    pub palette: u8,
    /// egui key events for the windows (with repeats for the arrows).
    pub events: Vec<egui::Event>,
    /// A button went down (hides the mouse cursor, as a key does).
    pub pressed: bool,
}

#[derive(Default)]
pub struct Gamepad {
    gilrs: Option<Gilrs>,
    tried: bool,
    active: Option<GamepadId>,
    maps: Option<(String, String, PadMap, PadMap)>,
    /// DAT_10455c3e: the left stick's mode (0 move, 1 d-pad, 2 camera), cycled by slot 31.
    mode: u8,
    prev: u64,
    /// Keys held now, with the time to their next arrow repeat.
    down: Vec<(KeyCode, f32)>,
}

impl Gamepad {
    /// Reads the pads. `focused`: the window has the focus (a pad in the background does nothing, as padmode's
    /// foreground default); `menu`: a window has the keys; `field`: in the world.
    /// `macros`: retail's macro gate is open (field, no window but the action menu), so LT / RT toggle the palettes;
    /// `palette`: one is up; `tabs`: a window has the keys (LB / RB turn its tabs).
    #[allow(clippy::too_many_arguments)]
    pub fn poll(&mut self, cfg: &Config, focused: bool, menu: bool, macros: bool, palette: bool, tabs: bool, field: bool, dt: f32) -> Frame {
        if !cfg.gamepad {
            return self.release_all();
        }
        if !self.tried {
            self.tried = true;
            match Gilrs::new() {
                Ok(g) => {
                    for (id, pad) in g.gamepads() {
                        log::info!("gamepad {id}: {}", pad.name());
                        self.active.get_or_insert(id);
                    }
                    self.gilrs = Some(g);
                }
                Err(e) => log::warn!("gamepad: {e}"),
            }
        }
        let (padsin, padexsin) = (or_default(&cfg.gamepad_padsin, DEFAULT_PADSIN), or_default(&cfg.gamepad_padexsin, DEFAULT_PADEXSIN));
        if self.maps.as_ref().is_none_or(|m| m.0 != padsin || m.1 != padexsin) {
            self.maps = Some((padsin.to_string(), padexsin.to_string(), PadMap::parse(padsin), PadMap::parse(padexsin)));
        }
        let Some(g) = &mut self.gilrs else { return Frame::default() };
        // The pad last touched is the one that plays.
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
        let pad = match self.active.and_then(|id| g.connected_gamepad(id)) {
            Some(p) if focused => p,
            _ => return self.release_all(),
        };
        let buttons: Vec<u8> = VIRTUAL.iter().filter(|(b, _)| pad.is_pressed(*b)).map(|(_, n)| *n).collect();
        let axis = |code: u8| {
            let a = match code {
                1 => Axis::LeftStickX,
                2 => Axis::LeftStickY,
                3 => Axis::LeftZ,
                4 => Axis::RightStickX,
                5 => Axis::RightStickY,
                6 => Axis::RightZ,
                _ => return 0.0,
            };
            pad.value(a) * 127.0
        };
        let (_, _, map, alt) = self.maps.as_ref().unwrap();
        let (held, v) = read(map, alt, &Device { buttons: &buttons, axis: &axis }, self.mode, cfg.gamepad_deadzone);
        let edge = held & !self.prev;
        if edge & bit(slot::STICK_MODE) != 0 {
            self.mode = (self.mode + 1) % 3;
        }
        self.prev = held;
        let menu = menu || palette || !field;
        let (keys, pad_bits) = keys_of(held, menu, palette, tabs && menu);
        let mut f = self.step(&keys, dt);
        f.pad = pad_bits;
        if field && (macros || palette) {
            f.palette = if edge & bit(slot::MACRO_CTRL) != 0 { 1 } else if edge & bit(slot::MACRO_ALT) != 0 { 2 } else { 0 };
        }
        let cam_y = if cfg.gamepad_invert_y { -v[1] } else { v[1] };
        // Outside the field the left stick does nothing (it never drives menus).
        f.stick = if field { [v[2] / 128.0, v[3] / 128.0, v[0] / 128.0, cam_y / 128.0] } else { [0.0; 4] };
        f
    }

    fn release_all(&mut self) -> Frame {
        self.prev = 0;
        self.step(&[], 0.0)
    }

    /// Key edges against last frame's keys, and the arrows' repeats.
    fn step(&mut self, keys: &[KeyCode], dt: f32) -> Frame {
        let mut f = Frame::default();
        let mods = egui::Modifiers { ctrl: keys.contains(&KeyCode::ControlRight), alt: keys.contains(&KeyCode::AltRight), ..Default::default() };
        let shift = egui::Modifiers { shift: keys.contains(&KeyCode::ShiftRight), ..Default::default() };
        let event = |k: KeyCode, pressed: bool, repeat: bool| {
            // The modifier markers are modifiers only, not keys of their own.
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
            egui::Key::from_name(&format!("{k:?}")).map(|key| egui::Event::Key { key, physical_key: Some(key), pressed, repeat, modifiers: mods })
        };
        for &(k, _) in &self.down {
            if !keys.contains(&k) {
                f.events.extend(event(k, false, false));
            }
        }
        self.down.retain(|(k, _)| keys.contains(k));
        for (k, t) in &mut self.down {
            if nav(*k) {
                *t -= dt;
                if *t <= 0.0 {
                    *t += REPEAT_EVERY;
                    f.events.extend(event(*k, true, true));
                }
            }
        }
        for &k in keys {
            if !self.down.iter().any(|d| d.0 == k) {
                self.down.push((k, REPEAT_AFTER));
                f.events.extend(event(k, true, false));
                f.pressed = true;
            }
        }
        f
    }
}

fn or_default<'a>(s: &'a str, d: &'a str) -> &'a str {
    if s.trim().is_empty() { d } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev<'a>(buttons: &'a [u8], axis: &'a dyn Fn(u8) -> f32) -> Device<'a> {
        Device { buttons, axis }
    }

    /// Steven's padsin: A confirm (slot 0), B cancel (3), X slot 1, Y slot 2, LB 8, LT 9, L3 10, RB 12, RT 13, R3 14,
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
        let (held, v) = read(&m, &alt, &dev(&[], &ax), 0, 42.0 / 127.0);
        assert_eq!(held, 0);
        assert_eq!((v[2], v[3], v[0]), (0.0, -127.0, 127.0));
        // Under 42 is nothing.
        let small = |c: u8| if c == 1 { 40.0 } else { 0.0 };
        assert_eq!(read(&m, &alt, &dev(&[], &small), 0, 42.0 / 127.0).1, [0.0; 4]);
        // A diagonal is rescaled to 127.
        let diag = |c: u8| if c <= 2 { 127.0 } else { 0.0 };
        let v = read(&m, &alt, &dev(&[], &diag), 0, 42.0 / 127.0).1;
        assert!(((v[2] * v[2] + v[3] * v[3]).sqrt() - 127.0).abs() < 0.01);
        // Mode 1: the left stick is the d-pad.
        let (held, v) = read(&m, &alt, &dev(&[], &ax), 1, 42.0 / 127.0);
        assert_eq!((held, v[3]), (bit(slot::UP), 0.0));
    }

    #[test]
    fn keys_and_triggers() {
        let m = PadMap::parse(DEFAULT_PADSIN);
        let none = |_: u8| 0.0;
        // A and the d-pad up: Enter; the arrow only in a window; the bits stay for the key functions.
        let (held, _) = read(&m, &m, &dev(&[3, 6], &none), 0, 0.33);
        assert_eq!(keys_of(held, false, false, false), (vec![KeyCode::Enter], bit(0) | bit(6)));
        assert_eq!(keys_of(held, true, false, false).0, vec![KeyCode::Enter, KeyCode::ArrowUp]);
        // A palette up takes confirm and the d-pad from the key functions (no action menu, no field targeting).
        assert_eq!(keys_of(held, true, true, false).1, 0);
        // LT in a window: Ctrl on the arrows (0xa8, page); the trigger's bit stays (0x89 item compare).
        let (held, _) = read(&m, &m, &dev(&[9, 6], &none), 0, 0.33);
        assert_eq!(keys_of(held, true, false, false), (vec![KeyCode::ControlRight, KeyCode::ArrowUp], held));
        // Both triggers: Alt (0xb5 / 0xb6, the bag switch). In the field neither is a key.
        let (held, _) = read(&m, &m, &dev(&[9, 12, 4], &none), 0, 0.33);
        assert_eq!(keys_of(held, true, false, false).0, vec![KeyCode::AltRight, KeyCode::ArrowRight]);
        assert!(keys_of(held, false, false, false).0.is_empty());
        // LB / RB with a window: Shift+Tab / Tab; without one, nothing (LB autoruns, RB opens the chat line).
        let (held, _) = read(&m, &m, &dev(&[8], &none), 0, 0.33);
        assert_eq!(keys_of(held, true, false, true), (vec![KeyCode::ShiftRight, KeyCode::Tab], held));
        assert!(keys_of(held, false, false, false).0.is_empty());
        let (held, _) = read(&m, &m, &dev(&[11], &none), 0, 0.33);
        assert_eq!(keys_of(held, true, false, true).0, vec![KeyCode::Tab]);
        let mut g = Gamepad::default();
        let f = g.step(&[KeyCode::ShiftRight, KeyCode::Tab], 1.0 / 60.0);
        assert!(matches!(f.events[..], [egui::Event::Key { key: egui::Key::Tab, modifiers, .. }] if modifiers.shift), "{:?}", f.events);
        // Every key has a DIK code.
        for k in [KeyCode::Enter, KeyCode::Escape, KeyCode::NumpadSubtract, KeyCode::ArrowUp] {
            assert!(crate::client::input::dik(k).is_some(), "{k:?}");
        }
    }

    #[test]
    fn edges_and_repeats() {
        let mut g = Gamepad::default();
        let f = g.step(&[KeyCode::ArrowDown], 1.0 / 60.0);
        assert!(f.pressed && f.events.len() == 1);
        // Held: a repeat after 16 ticks, then every 2.
        let mut repeats = 0;
        for _ in 0..24 {
            repeats += g.step(&[KeyCode::ArrowDown], 1.0 / 60.0).events.len();
        }
        assert!((4..=5).contains(&repeats), "{repeats}");
        assert_eq!(g.step(&[], 1.0 / 60.0).events.len(), 1);
        // A window's page modifier: Ctrl on the arrow, not on Enter.
        let mut g = Gamepad::default();
        let f = g.step(&[KeyCode::ControlRight, KeyCode::Enter, KeyCode::ArrowDown], 1.0 / 60.0);
        let ctrl = |key| f.events.iter().find_map(|e| matches!(e, egui::Event::Key { key: k, .. } if *k == key).then(|| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.ctrl))).unwrap();
        assert!(ctrl(egui::Key::ArrowDown) && !ctrl(egui::Key::Enter));
    }
}
