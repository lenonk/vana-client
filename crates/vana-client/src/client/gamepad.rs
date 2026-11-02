//! Game controllers (not retail; the README's "Gamepad support"). Retail's PC client handed pads to an external
//! config tool (configwi row 13, FUN_101bcb60) that we do not have, so a pad here works as a keyboard: each button
//! holds a key (by its DIK code into `Input`, and as an egui key event for the windows), the left stick is the
//! movement axes and the right stick the camera axes (`Input::set_stick`).
//!
//! The defaults follow the official PC pad layout. The d-pad runs game actions instead of keys in the field: left /
//! right the next actor on screen, up / down the party members.
//!
//! While a window has the keys (a menu, the action menu, a target prompt) or outside the field (title, lobby) the
//! d-pad steps the cursor as the arrows. The sticks never drive menus: the left stick walks even with a menu up, so a
//! target can be approached from the action menu. A button bound to Ctrl or Alt (the triggers by default) turns the d-pad and the face
//! buttons into the macro keys 1 ..= 8 while held, so Ctrl / Alt + 1 ..= 8 run the palettes' macros.
//!
//! Buttons are rebound in config.json `gamepad_buttons` by gilrs's button name and winit's key name or an action
//! name, e.g. `{"West": "Tab", "LeftThumb": "Numpad7", "Select": ""}` (an empty key unbinds the button). Actions:
//! `TargetNext`, `TargetPrev`, `PartyUp`, `PartyDown`, `Rest`.

use crate::config::Config;
use gilrs::{Axis, Button, GamepadId, Gilrs};
use std::collections::BTreeMap;
use winit::keyboard::KeyCode;

/// A game action a button runs on its press (the field only; with a window up a d-pad button is its arrow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The next / previous actor on screen (Tab / Shift + Tab, key functions 0x84 / 0x97).
    TargetNext,
    TargetPrev,
    /// The party member above / below the target (F1 ..= F6 in steps).
    PartyUp,
    PartyDown,
    /// /heal (on, or off while resting).
    Rest,
}

const ACTIONS: &[Action] = &[Action::TargetNext, Action::TargetPrev, Action::PartyUp, Action::PartyDown, Action::Rest];

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    Key(KeyCode),
    Act(Action),
}

/// The default layout: the official PC layout for an Xbox 360 pad (PlayOnline's new-player tips,
/// ff11newplayer/tips/en/list.html), each function through the key that does it. Face buttons by position (South =
/// Xbox A, PlayStation Cross).
const DEFAULTS: &[(Button, Bind)] = &[
    // A: confirm, the action menu. B: cancel.
    (Button::South, Bind::Key(KeyCode::Enter)),
    (Button::East, Bind::Key(KeyCode::Escape)),
    // X: the main menu (Num-). Y: the active window (Num+, message 8).
    (Button::West, Bind::Key(KeyCode::NumpadSubtract)),
    (Button::North, Bind::Key(KeyCode::NumpadAdd)),
    // The d-pad: targets in the field (up / down the party, left / right the actors on screen), the cursor in a window.
    (Button::DPadUp, Bind::Act(Action::PartyUp)),
    (Button::DPadDown, Bind::Act(Action::PartyDown)),
    (Button::DPadLeft, Bind::Act(Action::TargetPrev)),
    (Button::DPadRight, Bind::Act(Action::TargetNext)),
    // LB: autorun (Num7). RB: retail's software keyboard (not done); a screenshot with the windows hidden (Print Screen).
    (Button::LeftTrigger, Bind::Key(KeyCode::Numpad7)),
    (Button::RightTrigger, Bind::Key(KeyCode::PrintScreen)),
    // LT / RT: the macro palettes (Ctrl / Alt).
    (Button::LeftTrigger2, Bind::Key(KeyCode::ControlLeft)),
    (Button::RightTrigger2, Bind::Key(KeyCode::AltLeft)),
    // Back: hide / show the windows (Scroll Lock). Start: log out (Pause, the prompt).
    (Button::Select, Bind::Key(KeyCode::ScrollLock)),
    (Button::Start, Bind::Key(KeyCode::Pause)),
    // L3: rest, or lock on with a target (Num*, key function 0x44). R3: first / third person (Num5).
    (Button::LeftThumb, Bind::Key(KeyCode::NumpadMultiply)),
    (Button::RightThumb, Bind::Key(KeyCode::Numpad5)),
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

pub fn key_by_name(name: &str) -> Option<KeyCode> {
    KEYS.iter().copied().find(|k| format!("{k:?}").eq_ignore_ascii_case(name))
}

pub fn action_by_name(name: &str) -> Option<Action> {
    ACTIONS.iter().copied().find(|a| format!("{a:?}").eq_ignore_ascii_case(name))
}

/// The arrow a d-pad button is in a window.
fn arrow(b: Button) -> Option<KeyCode> {
    Some(match b {
        Button::DPadUp => KeyCode::ArrowUp,
        Button::DPadDown => KeyCode::ArrowDown,
        Button::DPadLeft => KeyCode::ArrowLeft,
        Button::DPadRight => KeyCode::ArrowRight,
        _ => return None,
    })
}

pub fn button_by_name(name: &str) -> Option<Button> {
    BUTTONS.iter().copied().find(|b| format!("{b:?}").eq_ignore_ascii_case(name))
}

/// The button layout: the defaults with config.json's overrides.
pub fn layout(overrides: &BTreeMap<String, String>) -> Vec<(Button, Bind)> {
    let mut map: Vec<(Button, Bind)> = DEFAULTS.to_vec();
    for (b, k) in overrides {
        let Some(b) = button_by_name(b) else {
            log::warn!("gamepad_buttons: no button {b:?}");
            continue;
        };
        map.retain(|&(x, _)| x != b);
        if k.is_empty() {
            continue;
        }
        match key_by_name(k).map(Bind::Key).or_else(|| action_by_name(k).map(Bind::Act)) {
            Some(bind) => map.push((b, bind)),
            None => log::warn!("gamepad_buttons: no key or action {k:?}"),
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

/// The keys held from the buttons this frame, and the buttons whose action runs. `menu`: the d-pad steps the cursor
/// and no action runs.
pub fn held_keys(map: &[(Button, Bind)], pressed: &[Button], menu: bool) -> (Vec<KeyCode>, Vec<(Button, Action)>) {
    let layer = pressed.iter().any(|b| map.iter().any(|&(x, k)| x == *b && matches!(k, Bind::Key(k) if modifier(k))));
    let mut keys = Vec::new();
    let mut acts = Vec::new();
    for b in pressed {
        let macro_key = MACRO.iter().find(|m| m.0 == *b).map(|m| Bind::Key(m.1)).filter(|_| layer);
        let menu_key = arrow(*b).map(Bind::Key).filter(|_| menu);
        let bind = macro_key.or(menu_key).or_else(|| map.iter().find(|m| m.0 == *b).map(|m| m.1));
        match bind {
            Some(Bind::Key(k)) if !keys.contains(&k) => keys.push(k),
            Some(Bind::Act(a)) if !menu => acts.push((*b, a)),
            _ => {}
        }
    }
    (keys, acts)
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
    /// Actions whose button went down this frame.
    pub actions: Vec<Action>,
    /// A button went down (hides the mouse cursor, as a key does).
    pub pressed: bool,
}

#[derive(Default)]
pub struct Gamepad {
    gilrs: Option<Gilrs>,
    tried: bool,
    active: Option<GamepadId>,
    map: Vec<(Button, Bind)>,
    map_from: Option<BTreeMap<String, String>>,
    /// The buttons whose action ran, until they are let go.
    acted: Vec<Button>,
    /// Keys held now, with the time to their next cursor repeat.
    down: Vec<(KeyCode, f32)>,
}

impl Gamepad {
    /// Reads the pads. `focused`: the window has the focus (a pad in the background does nothing); `menu`: a window
    /// has the keys; `field`: in the world (else the d-pad is always the arrows and the left stick does nothing).
    pub fn poll(&mut self, cfg: &Config, focused: bool, menu: bool, field: bool, dt: f32) -> Frame {
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
        let menu = menu || !field;
        let (keys, acts) = held_keys(&self.map, &pressed, menu);
        let mut f = self.step(&keys, dt);
        self.acted.retain(|b| pressed.contains(b));
        for (b, a) in acts {
            if !self.acted.contains(&b) {
                self.acted.push(b);
                f.actions.push(a);
                f.pressed = true;
            }
        }
        // A button held into a window does not fire when the window closes.
        if menu {
            self.acted = pressed.clone();
        }
        f.stick = if field { [lx, ly, rx, ry] } else { [0.0, 0.0, rx, ry] };
        f
    }

    fn release_all(&mut self) -> Frame {
        self.acted.clear();
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
        assert!(m.contains(&(Button::West, Bind::Key(KeyCode::Tab))));
        let m2 = layout(&[("South".into(), "rest".into())].into());
        assert!(m2.contains(&(Button::South, Bind::Act(Action::Rest))));
        assert!(!m.iter().any(|x| x.0 == Button::Select));
        assert_eq!(m.len(), DEFAULTS.len() - 1);
        // Every default key has a DIK code (egui has no name for some, such as Num* or Print Screen: only the key
        // functions read those).
        let keys = DEFAULTS.iter().filter_map(|d| if let Bind::Key(k) = d.1 { Some(k) } else { None });
        for k in keys.chain(MACRO.iter().map(|m| m.1)) {
            assert!(crate::client::input::dik(k).is_some(), "{k:?}");
        }
    }

    #[test]
    fn macro_layer_and_dpad() {
        let m = layout(&BTreeMap::new());
        assert_eq!(held_keys(&m, &[Button::South], false).0, vec![KeyCode::Enter]);
        let k = held_keys(&m, &[Button::LeftTrigger2, Button::South, Button::DPadUp], false).0;
        assert_eq!(k, vec![KeyCode::ControlLeft, KeyCode::Digit7, KeyCode::Digit1]);
        // The d-pad targets in the field and steps the cursor in a window; L3 is Num* (rest / lock on).
        let (k, a) = held_keys(&m, &[Button::DPadLeft, Button::LeftThumb], false);
        assert!(k == vec![KeyCode::NumpadMultiply] && a == vec![(Button::DPadLeft, Action::TargetPrev)]);
        let (k, a) = held_keys(&m, &[Button::DPadLeft, Button::LeftThumb], true);
        assert!(k == vec![KeyCode::ArrowLeft, KeyCode::NumpadMultiply] && a.is_empty());
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
