//! Game controllers (gilrs; ours) through retail's pad path: FUN_10005a40's XInput branch makes the 64-bit pad word
//! (bit n = slot n, which the key functions test) and the stick bytes, from a padsin000 button map (MODLOG "Gamepad").
//! Our windows read egui keys, so the slots a window reads also send their keys (confirm Enter, cancel Escape, ...).

use crate::config::Config;
use gilrs::{Axis, Button, GamepadId, Gilrs};
use winit::keyboard::KeyCode;

/// FFXiPadConfig's XInput layout: A confirm, B cancel, X main menu, Y active window, LB autorun, LT / RT macros, L3
/// rest / lock on, R3 first person, RB slot 12, Start log out, Back hide windows, d-pad, left stick moves, right stick
/// turns the camera (Y axes inverted, XInput's up being positive).
pub const DEFAULT_PADSIN: &str = "8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1";
/// The alternate map used while slot 34 is held (padexsin000): the sticks and d-pad only.
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
                let a = if matches!(code.unsigned_abs(), 1 | 2 | 4 | 5) { (dev.axis)(code.unsigned_abs()) } else { 0.0 };
                v[d] = a * code.signum() as f32;
            }
        }
    }
    // Then a button on a stick slot pushes that way, over the axis.
    for (s, d, x) in [(17, 0, 127.0), (18, 0, -127.0), (19, 1, -127.0), (20, 1, 127.0), (21, 2, 127.0), (22, 2, -127.0), (23, 3, -127.0), (24, 3, 127.0)] {
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
    // Retail zeroes each stick axis under 42 on its own, which snaps a turning stick onto the axes. Ours: a round dead
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
    held |= if v[4] >= 64.0 { bit(slot::RIGHT) } else if v[4] <= -64.0 { bit(slot::LEFT) } else { 0 };
    held |= if v[5] >= 64.0 { bit(slot::DOWN) } else if v[5] <= -64.0 { bit(slot::UP) } else { 0 };
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
}

#[derive(Default)]
pub struct Gamepad {
    gilrs: Option<Gilrs>,
    tried: bool,
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
}

impl Gamepad {
    /// Reads the pads. `focused`: the window has the focus (a pad in the background does nothing, as padmode's
    /// foreground default, unless config gamepad_background).
    pub fn poll(&mut self, cfg: &Config, focused: bool, ctx: Ctx, dt: f32) -> Frame {
        let Ctx { menu, palette, tabs, field, keyboard } = ctx;
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
            Some(p) if focused || cfg.gamepad_background => p,
            _ => return self.release_all(),
        };
        let buttons: Vec<u8> = VIRTUAL.iter().filter(|(b, _)| pad.is_pressed(*b)).map(|(_, n)| *n).collect();
        let axis = |code: u8| {
            let a = match code {
                1 => Axis::LeftStickX,
                2 => Axis::LeftStickY,
                4 => Axis::RightStickX,
                5 => Axis::RightStickY,
                _ => return 0.0,
            };
            pad.value(a) * 127.0
        };
        let (_, _, map, alt) = self.maps.as_ref().unwrap();
        // FUN_10005320 adds the alternate map's mode flags to padsin's.
        let modes = (map.dpad_mode || alt.dpad_mode, map.camera_mode || alt.camera_mode);
        if !modes.0 && !modes.1 {
            self.mode = 0;
        }
        let (held, v) = read(map, alt, &Device { buttons: &buttons, axis: &axis }, self.mode, modes, cfg.gamepad_deadzone);
        if held & !self.prev & bit(slot::STICK_MODE) != 0 {
            self.mode = next_mode(self.mode, modes);
        }
        let menu = menu || palette || !field;
        let (keys, pad_bits) = if keyboard { (Vec::new(), held) } else { keys_of(held, menu, palette, tabs && menu) };
        let mut f = self.step(held, &keys, dt);
        self.prev = held;
        f.pad = pad_bits;
        f.used = held != 0 || v.iter().any(|&a| a != 0.0);
        // Outside the field the sticks do nothing (they never drive menus).
        f.stick = if field { [v[2] / 128.0, v[3] / 128.0, v[0] / 128.0, v[1] / 128.0] } else { [0.0; 4] };
        f
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

fn or_default<'a>(s: &'a str, d: &'a str) -> &'a str {
    if s.trim().is_empty() { d } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev<'a>(buttons: &'a [u8], axis: &'a dyn Fn(u8) -> f32) -> Device<'a> {
        Device { buttons, axis }
    }

    const DEAD: f32 = 42.0 / 127.0;

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
        let z = PadMap::parse("8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1".replace("32,32", "34,34").as_str());
        let full = |_: u8| 127.0;
        assert_eq!(read(&z, &alt, &dev(&[], &full), 0, (false, false), DEAD).1[2], 0.0);
    }

    /// Slot 25 takes the movement stick as the d-pad (its own d-pad bits dropped), over slot 26 and the modes; the
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
        assert_eq!((next_mode(0, (false, true)), next_mode(2, (false, true)), next_mode(0, (true, false)), next_mode(1, (true, false))), (2, 0, 1, 0));
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
        // A palette up takes confirm and the d-pad from the key functions (no action menu, no field targeting).
        assert_eq!(keys_of(held, true, true, false).1, 0);
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
        assert!(matches!(f.events[..], [egui::Event::Key { key: egui::Key::Tab, modifiers, .. }] if modifiers.shift), "{:?}", f.events);
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
        let f = g.step(bit(0) | bit(7) | bit(9), &[KeyCode::ControlRight, KeyCode::Enter, KeyCode::ArrowDown], 1.0 / 60.0);
        let ctrl = |key| f.events.iter().find_map(|e| matches!(e, egui::Event::Key { key: k, .. } if *k == key).then(|| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.ctrl))).unwrap();
        assert!(ctrl(egui::Key::ArrowDown) && !ctrl(egui::Key::Enter));
    }
}
