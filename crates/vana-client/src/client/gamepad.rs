//! Game controllers (not retail; the README's "Gamepad support"). Retail's PC client handed pads to an external
//! config tool (configwi row 13, FUN_101bcb60) that we do not have, so a pad here works as a keyboard: each button
//! holds a key (by its DIK code into `Input`, and as an egui key event for the windows), the left stick is the
//! movement axes and the right stick the camera axes (`Input::set_stick`).
//!
//! While a window has the keys (or outside the field: title, lobby) the left stick steps the cursor as the arrows.
//! A button bound to Ctrl or Alt (the triggers by default) turns the d-pad and the face buttons into the macro keys
//! 1 ..= 8 while held, so Ctrl / Alt + 1 ..= 8 run the palettes' macros.
//!
//! Buttons are rebound in config.json `gamepad_buttons` by gilrs's button name and winit's key name, e.g.
//! `{"West": "Tab", "Select": ""}` (an empty key unbinds the button).

use crate::config::Config;
use gilrs::{Axis, Button, GamepadId, Gilrs};
use std::collections::BTreeMap;
use winit::keyboard::KeyCode;

/// The default layout. Face buttons by position (South = Xbox A, PlayStation Cross).
const DEFAULTS: &[(Button, KeyCode)] = &[
    (Button::South, KeyCode::Enter),
    (Button::East, KeyCode::Escape),
    // Num* : lock-on, or /heal with no target (key function 0x44).
    (Button::West, KeyCode::NumpadMultiply),
    // Num- : the main menu.
    (Button::North, KeyCode::NumpadSubtract),
    (Button::DPadUp, KeyCode::ArrowUp),
    (Button::DPadDown, KeyCode::ArrowDown),
    (Button::DPadLeft, KeyCode::ArrowLeft),
    (Button::DPadRight, KeyCode::ArrowRight),
    // Tab: the next target.
    (Button::LeftTrigger, KeyCode::Tab),
    // Num+ : the other window / the next tab (message 8).
    (Button::RightTrigger, KeyCode::NumpadAdd),
    (Button::LeftTrigger2, KeyCode::ControlLeft),
    (Button::RightTrigger2, KeyCode::AltLeft),
    // F8: the nearest monster.
    (Button::Select, KeyCode::F8),
    (Button::Start, KeyCode::NumpadSubtract),
    // Num7: autorun; Num5: first / third person.
    (Button::LeftThumb, KeyCode::Numpad7),
    (Button::RightThumb, KeyCode::Numpad5),
];

/// The macro keys a button presses while Ctrl or Alt is held from the pad.
const MACRO: &[(Button, KeyCode)] = &[
    (Button::DPadUp, KeyCode::Digit1),
    (Button::DPadRight, KeyCode::Digit2),
    (Button::DPadDown, KeyCode::Digit3),
    (Button::DPadLeft, KeyCode::Digit4),
    (Button::North, KeyCode::Digit5),
    (Button::East, KeyCode::Digit6),
    (Button::South, KeyCode::Digit7),
    (Button::West, KeyCode::Digit8),
];

const BUTTONS: &[Button] = &[
    Button::South,
    Button::East,
    Button::North,
    Button::West,
    Button::C,
    Button::Z,
    Button::LeftTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::Mode,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

/// Every key `input::dik` knows, for the names in `gamepad_buttons`.
const KEYS: &[KeyCode] = {
    use KeyCode::*;
    &[
        Escape, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0, Minus, Equal, Backspace,
        Tab, KeyQ, KeyW, KeyE, KeyR, KeyT, KeyY, KeyU, KeyI, KeyO, KeyP, BracketLeft, BracketRight, Enter, ControlLeft,
        KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL, Semicolon, Quote, Backquote, ShiftLeft, Backslash, KeyZ,
        KeyX, KeyC, KeyV, KeyB, KeyN, KeyM, Comma, Period, Slash, ShiftRight, NumpadMultiply, AltLeft, Space, CapsLock,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, NumLock, ScrollLock, Numpad7, Numpad8, Numpad9, NumpadSubtract, Numpad4,
        Numpad5, Numpad6, NumpadAdd, Numpad1, Numpad2, Numpad3, Numpad0, NumpadDecimal, IntlBackslash, F11, F12,
        NumpadEnter, ControlRight, NumpadComma, NumpadDivide, PrintScreen, AltRight, Pause, Home, ArrowUp, PageUp,
        ArrowLeft, ArrowRight, End, ArrowDown, PageDown, Insert, Delete, SuperLeft, SuperRight, ContextMenu,
    ]
};

/// Window cursor repeat while a d-pad direction or the stick is held: first after this long, then every
/// `REPEAT_EVERY` (seconds; about a keyboard's).
const REPEAT_AFTER: f32 = 0.4;
const REPEAT_EVERY: f32 = 0.08;
/// How far the left stick goes before it steps a window's cursor.
const NAV_TILT: f32 = 0.5;

pub fn key_by_name(name: &str) -> Option<KeyCode> {
    KEYS.iter().copied().find(|k| format!("{k:?}").eq_ignore_ascii_case(name))
}

pub fn button_by_name(name: &str) -> Option<Button> {
    BUTTONS.iter().copied().find(|b| format!("{b:?}").eq_ignore_ascii_case(name))
}

/// The button layout: the defaults with config.json's overrides.
pub fn layout(overrides: &BTreeMap<String, String>) -> Vec<(Button, KeyCode)> {
    let mut map: Vec<(Button, KeyCode)> = DEFAULTS.to_vec();
    for (b, k) in overrides {
        let Some(b) = button_by_name(b) else {
            log::warn!("gamepad_buttons: no button {b:?}");
            continue;
        };
        map.retain(|&(x, _)| x != b);
        if k.is_empty() {
            continue;
        }
        match key_by_name(k) {
            Some(k) => map.push((b, k)),
            None => log::warn!("gamepad_buttons: no key {k:?}"),
        }
    }
    map
}

fn modifier(k: KeyCode) -> bool {
    matches!(k, KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::AltLeft | KeyCode::AltRight)
}

fn nav(k: KeyCode) -> bool {
    matches!(k, KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight)
}

/// Stick travel past the dead zone, rescaled to 0 ..= 1 (radial, so diagonals keep their direction).
pub fn dead_zone(x: f32, y: f32, dz: f32) -> (f32, f32) {
    let dz = dz.clamp(0.0, 0.9);
    let m = (x * x + y * y).sqrt();
    if m <= dz {
        return (0.0, 0.0);
    }
    let s = ((m - dz) / (1.0 - dz)).min(1.0) / m;
    (x * s, y * s)
}

/// The keys held from the buttons and the left stick this frame. `menu`: the stick steps the cursor.
pub fn held_keys(map: &[(Button, KeyCode)], pressed: &[Button], left: (f32, f32), menu: bool) -> Vec<KeyCode> {
    let layer = pressed.iter().any(|b| map.iter().any(|&(x, k)| x == *b && modifier(k)));
    let mut keys = Vec::new();
    for b in pressed {
        let macro_key = MACRO.iter().find(|m| m.0 == *b).map(|m| m.1).filter(|_| layer);
        let k = macro_key.or_else(|| map.iter().find(|m| m.0 == *b).map(|m| m.1));
        if let Some(k) = k
            && !keys.contains(&k)
        {
            keys.push(k);
        }
    }
    if menu {
        let (x, y) = left;
        let k = if x.abs().max(y.abs()) < NAV_TILT {
            None
        } else if x.abs() > y.abs() {
            Some(if x > 0.0 { KeyCode::ArrowRight } else { KeyCode::ArrowLeft })
        } else {
            Some(if y > 0.0 { KeyCode::ArrowDown } else { KeyCode::ArrowUp })
        };
        if let Some(k) = k
            && !keys.contains(&k)
        {
            keys.push(k);
        }
    }
    keys
}

/// A frame's worth of pad input for the client.
#[derive(Default)]
pub struct Frame {
    /// Keys going down (true) or up, for `Input::set`.
    pub keys: Vec<(KeyCode, bool)>,
    /// The same as egui key events (with repeats for the cursor keys), for the windows.
    pub events: Vec<egui::Event>,
    /// Left X / Y, right X / Y for `Input::set_stick`.
    pub stick: [f32; 4],
    /// A button went down (hides the mouse cursor, as a key does).
    pub pressed: bool,
}

#[derive(Default)]
pub struct Gamepad {
    gilrs: Option<Gilrs>,
    tried: bool,
    active: Option<GamepadId>,
    map: Vec<(Button, KeyCode)>,
    map_from: Option<BTreeMap<String, String>>,
    /// Keys held now, with the time to their next cursor repeat.
    down: Vec<(KeyCode, f32)>,
}

impl Gamepad {
    /// Reads the pads. `focused`: the window has the focus (a pad in the background does nothing).
    pub fn poll(&mut self, cfg: &Config, focused: bool, menu: bool, dt: f32) -> Frame {
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
        if self.map_from.as_ref() != Some(&cfg.gamepad_buttons) {
            self.map = layout(&cfg.gamepad_buttons);
            self.map_from = Some(cfg.gamepad_buttons.clone());
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
        let pressed: Vec<Button> = BUTTONS.iter().copied().filter(|&b| pad.is_pressed(b)).collect();
        let dz = cfg.gamepad_deadzone;
        // gilrs: up is positive; the client's axes: down is positive.
        let (lx, ly) = dead_zone(pad.value(Axis::LeftStickX), -pad.value(Axis::LeftStickY), dz);
        let (rx, ry) = dead_zone(pad.value(Axis::RightStickX), -pad.value(Axis::RightStickY), dz);
        let ry = if cfg.gamepad_invert_y { -ry } else { ry };
        let keys = held_keys(&self.map, &pressed, (lx, ly), menu);
        let mut f = self.step(&keys, dt);
        f.stick = if menu { [0.0, 0.0, rx, ry] } else { [lx, ly, rx, ry] };
        f
    }

    fn release_all(&mut self) -> Frame {
        let mut f = self.step(&[], 0.0);
        f.stick = [0.0; 4];
        f
    }

    /// Key edges against last frame's keys, and the cursor keys' repeats.
    fn step(&mut self, keys: &[KeyCode], dt: f32) -> Frame {
        let mut f = Frame::default();
        let mods = egui::Modifiers {
            ctrl: keys.iter().any(|k| matches!(k, KeyCode::ControlLeft | KeyCode::ControlRight)),
            alt: keys.iter().any(|k| matches!(k, KeyCode::AltLeft | KeyCode::AltRight)),
            ..Default::default()
        };
        let event = |k: KeyCode, pressed: bool, repeat: bool| {
            egui::Key::from_name(&format!("{k:?}")).map(|key| egui::Event::Key { key, physical_key: Some(key), pressed, repeat, modifiers: mods })
        };
        for &(k, _) in &self.down {
            if !keys.contains(&k) {
                f.keys.push((k, false));
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
                f.keys.push((k, true));
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

    #[test]
    fn names_and_overrides() {
        assert_eq!(key_by_name("NumpadSubtract"), Some(KeyCode::NumpadSubtract));
        assert_eq!(key_by_name("enter"), Some(KeyCode::Enter));
        assert_eq!(button_by_name("LeftTrigger2"), Some(Button::LeftTrigger2));
        let o: BTreeMap<String, String> = [("West".into(), "Tab".into()), ("Select".into(), String::new()), ("Nope".into(), "Tab".into())].into();
        let m = layout(&o);
        assert!(m.contains(&(Button::West, KeyCode::Tab)));
        assert!(!m.iter().any(|x| x.0 == Button::Select));
        assert_eq!(m.len(), DEFAULTS.len() - 1);
        // Every default key has a DIK code and every one but the modifiers and Num* (egui has no name for it; only the
        // key functions read it) an egui key.
        for &(_, k) in DEFAULTS.iter().chain(MACRO) {
            assert!(crate::client::input::dik(k).is_some(), "{k:?}");
            assert!(modifier(k) || k == KeyCode::NumpadMultiply || egui::Key::from_name(&format!("{k:?}")).is_some(), "{k:?}");
        }
    }

    #[test]
    fn macro_layer_and_stick_nav() {
        let m = layout(&BTreeMap::new());
        assert_eq!(held_keys(&m, &[Button::South], (0.0, 0.0), false), vec![KeyCode::Enter]);
        let k = held_keys(&m, &[Button::LeftTrigger2, Button::South, Button::DPadUp], (0.0, 0.0), false);
        assert_eq!(k, vec![KeyCode::ControlLeft, KeyCode::Digit7, KeyCode::Digit1]);
        assert_eq!(held_keys(&m, &[], (0.2, 0.9), true), vec![KeyCode::ArrowDown]);
        assert!(held_keys(&m, &[], (0.2, 0.9), false).is_empty());
        assert!(held_keys(&m, &[], (0.3, 0.1), true).is_empty());
    }

    #[test]
    fn dead_zone_rescales() {
        assert_eq!(dead_zone(0.1, 0.1, 0.2), (0.0, 0.0));
        let (x, y) = dead_zone(1.0, 0.0, 0.2);
        assert!((x - 1.0).abs() < 1e-6 && y == 0.0);
        let (x, _) = dead_zone(0.6, 0.0, 0.2);
        assert!((x - 0.5).abs() < 1e-6);
    }

    #[test]
    fn edges_and_repeats() {
        let mut g = Gamepad::default();
        let f = g.step(&[KeyCode::ArrowDown], 0.016);
        assert_eq!(f.keys, vec![(KeyCode::ArrowDown, true)]);
        assert!(f.pressed && f.events.len() == 1);
        // Held: no new edge, a repeat after REPEAT_AFTER, then every REPEAT_EVERY.
        let mut repeats = 0;
        for _ in 0..30 {
            let f = g.step(&[KeyCode::ArrowDown], 0.02);
            assert!(f.keys.is_empty());
            repeats += f.events.len();
        }
        assert_eq!(repeats, 3);
        let f = g.step(&[], 0.02);
        assert_eq!(f.keys, vec![(KeyCode::ArrowDown, false)]);
    }
}
