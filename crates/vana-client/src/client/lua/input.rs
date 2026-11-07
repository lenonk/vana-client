//! Keys and the mouse for mods (docs/lua-mods.md 4.5): filters that see each window event before egui and the game
//! do (taking it needs input.capture), key binds, the held state, and keys a mod presses.

use super::{World, lend};
use egui::Key as E;
use mlua::{Function, RegistryKey, Value};
use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode as K, ModifiersState, PhysicalKey};

/// Key names (lower case), the window's key, and the egui key egui-winit makes of it.
const KEYS: &[(&str, K, Option<E>)] = &[
    ("a", K::KeyA, Some(E::A)), ("b", K::KeyB, Some(E::B)), ("c", K::KeyC, Some(E::C)), ("d", K::KeyD, Some(E::D)),
    ("e", K::KeyE, Some(E::E)), ("f", K::KeyF, Some(E::F)), ("g", K::KeyG, Some(E::G)), ("h", K::KeyH, Some(E::H)),
    ("i", K::KeyI, Some(E::I)), ("j", K::KeyJ, Some(E::J)), ("k", K::KeyK, Some(E::K)), ("l", K::KeyL, Some(E::L)),
    ("m", K::KeyM, Some(E::M)), ("n", K::KeyN, Some(E::N)), ("o", K::KeyO, Some(E::O)), ("p", K::KeyP, Some(E::P)),
    ("q", K::KeyQ, Some(E::Q)), ("r", K::KeyR, Some(E::R)), ("s", K::KeyS, Some(E::S)), ("t", K::KeyT, Some(E::T)),
    ("u", K::KeyU, Some(E::U)), ("v", K::KeyV, Some(E::V)), ("w", K::KeyW, Some(E::W)), ("x", K::KeyX, Some(E::X)),
    ("y", K::KeyY, Some(E::Y)), ("z", K::KeyZ, Some(E::Z)),
    ("0", K::Digit0, Some(E::Num0)), ("1", K::Digit1, Some(E::Num1)), ("2", K::Digit2, Some(E::Num2)), ("3", K::Digit3, Some(E::Num3)),
    ("4", K::Digit4, Some(E::Num4)), ("5", K::Digit5, Some(E::Num5)), ("6", K::Digit6, Some(E::Num6)), ("7", K::Digit7, Some(E::Num7)),
    ("8", K::Digit8, Some(E::Num8)), ("9", K::Digit9, Some(E::Num9)),
    ("f1", K::F1, Some(E::F1)), ("f2", K::F2, Some(E::F2)), ("f3", K::F3, Some(E::F3)), ("f4", K::F4, Some(E::F4)),
    ("f5", K::F5, Some(E::F5)), ("f6", K::F6, Some(E::F6)), ("f7", K::F7, Some(E::F7)), ("f8", K::F8, Some(E::F8)),
    ("f9", K::F9, Some(E::F9)), ("f10", K::F10, Some(E::F10)), ("f11", K::F11, Some(E::F11)), ("f12", K::F12, Some(E::F12)),
    ("escape", K::Escape, Some(E::Escape)), ("enter", K::Enter, Some(E::Enter)), ("space", K::Space, Some(E::Space)),
    ("tab", K::Tab, Some(E::Tab)), ("backspace", K::Backspace, Some(E::Backspace)), ("insert", K::Insert, Some(E::Insert)),
    ("delete", K::Delete, Some(E::Delete)), ("home", K::Home, Some(E::Home)), ("end", K::End, Some(E::End)),
    ("pageup", K::PageUp, Some(E::PageUp)), ("pagedown", K::PageDown, Some(E::PageDown)),
    ("up", K::ArrowUp, Some(E::ArrowUp)), ("down", K::ArrowDown, Some(E::ArrowDown)),
    ("left", K::ArrowLeft, Some(E::ArrowLeft)), ("right", K::ArrowRight, Some(E::ArrowRight)),
    ("-", K::Minus, Some(E::Minus)), ("=", K::Equal, Some(E::Equals)), ("[", K::BracketLeft, Some(E::OpenBracket)),
    ("]", K::BracketRight, Some(E::CloseBracket)), ("\\", K::Backslash, Some(E::Backslash)), (";", K::Semicolon, Some(E::Semicolon)),
    ("'", K::Quote, Some(E::Quote)), ("`", K::Backquote, Some(E::Backtick)), (",", K::Comma, Some(E::Comma)),
    (".", K::Period, Some(E::Period)), ("/", K::Slash, Some(E::Slash)),
    ("numpad0", K::Numpad0, Some(E::Num0)), ("numpad1", K::Numpad1, Some(E::Num1)), ("numpad2", K::Numpad2, Some(E::Num2)),
    ("numpad3", K::Numpad3, Some(E::Num3)), ("numpad4", K::Numpad4, Some(E::Num4)), ("numpad5", K::Numpad5, Some(E::Num5)),
    ("numpad6", K::Numpad6, Some(E::Num6)), ("numpad7", K::Numpad7, Some(E::Num7)), ("numpad8", K::Numpad8, Some(E::Num8)),
    ("numpad9", K::Numpad9, Some(E::Num9)), ("numpad+", K::NumpadAdd, Some(E::Plus)), ("numpad-", K::NumpadSubtract, Some(E::Minus)),
    ("numpad*", K::NumpadMultiply, None), ("numpad/", K::NumpadDivide, Some(E::Slash)), ("numpad.", K::NumpadDecimal, Some(E::Period)),
    ("numpadenter", K::NumpadEnter, Some(E::Enter)),
    ("capslock", K::CapsLock, None), ("scrolllock", K::ScrollLock, None), ("numlock", K::NumLock, None), ("pause", K::Pause, None),
    ("printscreen", K::PrintScreen, None), ("apps", K::ContextMenu, None),
    ("lshift", K::ShiftLeft, None), ("rshift", K::ShiftRight, None), ("lctrl", K::ControlLeft, None), ("rctrl", K::ControlRight, None),
    ("lalt", K::AltLeft, None), ("ralt", K::AltRight, None), ("lwin", K::SuperLeft, None), ("rwin", K::SuperRight, None),
];

/// A key's name.
pub(super) fn name_of(code: K) -> Option<&'static str> {
    KEYS.iter().find(|k| k.1 == code).map(|k| k.0)
}

/// A key by name (a few aliases; "shift", "ctrl", "alt" and "win" are the left keys).
pub(super) fn code_of(name: &str) -> Option<K> {
    let name = name.trim().to_ascii_lowercase();
    let name = match name.as_str() {
        "return" => "enter",
        "esc" => "escape",
        "minus" => "-",
        "equals" | "equal" => "=",
        "shift" => "lshift",
        "ctrl" | "control" => "lctrl",
        "alt" => "lalt",
        "win" | "super" => "lwin",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        "ins" => "insert",
        "del" => "delete",
        n => n,
    };
    KEYS.iter().find(|k| k.0 == name).map(|k| k.1)
}

fn egui_of(code: K) -> Option<E> {
    KEYS.iter().find(|k| k.1 == code).and_then(|k| k.2)
}

/// A key with its modifiers: "ctrl+f1", "alt+shift+1", or Windower's "^f1" (^ Ctrl, ! Alt, ~ Shift, @ Win).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Combo {
    pub key: K,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Combo {
    pub fn parse(s: &str) -> Option<Self> {
        let mut s = s.trim().to_ascii_lowercase();
        let (mut ctrl, mut alt, mut shift, mut win) = (false, false, false, false);
        loop {
            if s.len() > 1 && let Some(c) = s.chars().next().filter(|c| "^!~@".contains(*c)) {
                match c {
                    '^' => ctrl = true,
                    '!' => alt = true,
                    '~' => shift = true,
                    _ => win = true,
                }
                s.remove(0);
                continue;
            }
            let prefixes: [(&str, u8); 7] = [("ctrl+", 0), ("control+", 0), ("alt+", 1), ("shift+", 2), ("win+", 3), ("super+", 3), ("cmd+", 3)];
            match prefixes.iter().find(|p| s.starts_with(p.0) && s.len() > p.0.len()) {
                Some(&(p, m)) => {
                    match m {
                        0 => ctrl = true,
                        1 => alt = true,
                        2 => shift = true,
                        _ => win = true,
                    }
                    s = s[p.len()..].to_string();
                }
                None => break,
            }
        }
        Some(Self { key: code_of(&s)?, ctrl, alt, shift, win })
    }

    /// The key with exactly these modifiers held.
    fn matches(&self, code: K, m: ModifiersState) -> bool {
        self.key == code && self.ctrl == m.control_key() && self.alt == m.alt_key() && self.shift == m.shift_key() && self.win == m.super_key()
    }
}

/// What a bind does: a line run as typed, or a function of the mod's.
pub(super) enum BindAction {
    Line(String),
    Func(RegistryKey),
}

pub(super) struct Bind {
    pub combo: Combo,
    /// The combo as the mod wrote it.
    pub text: String,
    /// Fires on the release instead of the press.
    pub up: bool,
    /// Fires while a text field (the chat line...) is open too.
    pub typing: bool,
    pub action: BindAction,
}

/// A key a mod presses: a tap (down and up in one frame) or one edge.
#[derive(Clone, Copy)]
pub(in crate::client) struct Synth {
    pub key: K,
    pub down: Option<bool>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Synth {
    /// The egui events the window would have made.
    pub(in crate::client) fn egui_events(&self) -> Vec<egui::Event> {
        let Some(key) = egui_of(self.key) else { return Vec::new() };
        let modifiers = egui::Modifiers { alt: self.alt, ctrl: self.ctrl, shift: self.shift, mac_cmd: false, command: self.ctrl };
        let ev = |pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers };
        match self.down {
            None => vec![ev(true), ev(false)],
            Some(d) => vec![ev(d)],
        }
    }

    /// The DIK codes of the key and its modifiers.
    pub(in crate::client) fn diks(&self) -> (Option<u8>, Vec<u8>) {
        let mods = [(self.ctrl, 0x1d), (self.alt, 0x38), (self.shift, 0x2a)].iter().filter(|m| m.0).map(|m| m.1).collect();
        (super::super::input::dik(self.key), mods)
    }
}

/// The keys and mouse as the window last told them, shared with every mod (app data).
#[derive(Default)]
pub(super) struct State {
    pub held: HashSet<K>,
    pub mods: ModifiersState,
    /// Window pixels.
    pub cursor: (f64, f64),
    /// Left, right, middle.
    pub buttons: [bool; 3],
    /// Presses a mod took: their repeats and releases are taken too.
    taken_keys: HashSet<K>,
    taken_buttons: Vec<MouseButton>,
    /// A press on a mod's HUD element: its mod, element, button, where it went down, and whether it is a drag yet.
    hud_press: Option<(String, String, MouseButton, (f64, f64), bool)>,
    /// The HUD element under the pointer (one with a handler or a tip) and since when.
    pub hud_hover: Option<(String, String, std::time::Instant)>,
}

/// A press moving this far (window pixels) is a drag.
const DRAG_START: f64 = 4.0;

impl State {
    pub fn held(&self, name: &str) -> bool {
        match name.trim().to_ascii_lowercase().as_str() {
            "shift" => self.mods.shift_key(),
            "ctrl" | "control" => self.mods.control_key(),
            "alt" => self.mods.alt_key(),
            "win" | "super" => self.mods.super_key(),
            n => code_of(n).is_some_and(|c| self.held.contains(&c)),
        }
    }
}

fn button_name(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
        MouseButton::Back => "back",
        MouseButton::Forward => "forward",
        MouseButton::Other(_) => "other",
    }
}

/// A window event before egui and the game see it: true when a mod took it (a filter with input.capture, or a bind).
pub(in crate::client) fn window_event(w: &mut World, event: &WindowEvent) -> bool {
    if !w.lua.started {
        return false;
    }
    let st = w.lua.input.clone();
    match event {
        WindowEvent::ModifiersChanged(m) => {
            st.borrow_mut().mods = m.state();
            false
        }
        WindowEvent::Focused(false) => {
            let mut s = st.borrow_mut();
            s.held.clear();
            s.buttons = [false; 3];
            s.taken_keys.clear();
            s.taken_buttons.clear();
            false
        }
        WindowEvent::KeyboardInput { event: k, .. } => {
            let PhysicalKey::Code(code) = k.physical_key else { return false };
            key_edge(w, code, k.state == ElementState::Pressed, k.repeat, None)
        }
        WindowEvent::CursorMoved { position, .. } => moved(w, position.x, position.y),
        WindowEvent::MouseInput { state, button, .. } => button_edge(w, *button, *state == ElementState::Pressed),
        WindowEvent::MouseWheel { delta, .. } => wheel(
            w,
            match delta {
                MouseScrollDelta::LineDelta(_, y) => *y as f64,
                MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
            },
        ),
        _ => false,
    }
}

/// A test drive's egui events through the mods, as the window's go: the ones no mod took. `ppp` is egui's pixels per
/// point, so a drive's pixel lands where the window's would.
pub(in crate::client) fn drive_events(w: &mut World, events: Vec<egui::Event>, ppp: f64) -> Vec<egui::Event> {
    if !w.lua.started {
        return events;
    }
    let mut keep = Vec::with_capacity(events.len());
    for e in events {
        let took = match &e {
            egui::Event::Key { key, pressed, repeat, modifiers, .. } => {
                let code = KEYS.iter().find(|k| k.2 == Some(*key)).map(|k| k.1);
                let mut m = ModifiersState::empty();
                m.set(ModifiersState::CONTROL, modifiers.ctrl);
                m.set(ModifiersState::ALT, modifiers.alt);
                m.set(ModifiersState::SHIFT, modifiers.shift);
                code.is_some_and(|c| key_edge(w, c, *pressed, *repeat, Some(m)))
            }
            egui::Event::PointerMoved(p) => moved(w, p.x as f64 * ppp, p.y as f64 * ppp),
            egui::Event::PointerButton { button, pressed, .. } => {
                let b = match button {
                    egui::PointerButton::Secondary => MouseButton::Right,
                    egui::PointerButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Left,
                };
                button_edge(w, b, *pressed)
            }
            egui::Event::MouseWheel { delta, .. } => wheel(w, delta.y as f64),
            _ => false,
        };
        if !took {
            keep.push(e);
        }
    }
    keep
}

/// A key going down, repeating or up: the mods' filters, then their binds. `mods` overrides the window's modifiers
/// (a drive's events carry their own).
fn key_edge(w: &mut World, code: K, down: bool, repeat: bool, mods: Option<ModifiersState>) -> bool {
    let st = w.lua.input.clone();
    let taken = {
        let mut s = st.borrow_mut();
        if down {
            s.held.insert(code);
        } else {
            s.held.remove(&code);
        }
        if down { repeat && s.taken_keys.contains(&code) } else { s.taken_keys.remove(&code) }
    };
    if taken && down {
        return true;
    }
    let Some(name) = name_of(code) else { return taken };
    let mods = mods.unwrap_or(st.borrow().mods);
    let typing = super::super::text_field_open(w);
    let e = serde_json::json!({"key": name, "down": down, "repeated": repeat, "ctrl": mods.control_key(), "alt": mods.alt_key(), "shift": mods.shift_key(), "typing": typing});
    let mut took = filter(w, false, e) || taken;
    if !repeat && bind(w, code, mods, down, typing) {
        took = true;
    }
    if took && down {
        st.borrow_mut().taken_keys.insert(code);
    }
    took
}

fn moved(w: &mut World, x: f64, y: f64) -> bool {
    w.lua.input.borrow_mut().cursor = (x, y);
    hud_drag(&mut w.lua, x, y);
    filter(w, true, serde_json::json!({"kind": "move", "x": x.round() as i64, "y": y.round() as i64}))
}

/// A HUD element's event: kind, button, window pixels, and the drag's offset from where it went down.
fn hud_event(kind: &str, button: MouseButton, (x, y): (f64, f64), (sx, sy): (f64, f64)) -> serde_json::Value {
    serde_json::json!({"kind": kind, "button": button_name(button), "x": x.round() as i64, "y": y.round() as i64, "dx": (x - sx).round() as i64, "dy": (y - sy).round() as i64})
}

/// The pointer moving with a HUD element's press: past DRAG_START it is a drag (drag_start where it went down, then
/// drag at most once a mod phase).
pub(super) fn hud_drag(host: &mut super::Host, x: f64, y: f64) {
    let st = host.input.clone();
    let mut s = st.borrow_mut();
    let Some((m, id, button, start, dragging)) = s.hud_press.as_mut() else { return };
    if !*dragging {
        if (x - start.0).hypot(y - start.1) < DRAG_START {
            return;
        }
        *dragging = true;
        host.hud_events.push((m.clone(), id.clone(), hud_event("drag_start", *button, *start, *start)));
    }
    let e = hud_event("drag", *button, (x, y), *start);
    match host.hud_events.last_mut() {
        Some(last) if last.0 == *m && last.1 == *id && last.2["kind"] == "drag" => last.2 = e,
        _ => host.hud_events.push((m.clone(), id.clone(), e)),
    }
}

/// A left or right press at the pointer on a HUD element with a handler: the element's (true), and its release too.
pub(super) fn hud_press(host: &mut super::Host, button: MouseButton) -> bool {
    let st = host.input.clone();
    if !matches!(button, MouseButton::Left | MouseButton::Right) || st.borrow().hud_press.is_some() {
        return false;
    }
    let (x, y) = st.borrow().cursor;
    let Some((m, id)) = super::hud::hit(host, x, y) else { return false };
    let mut s = st.borrow_mut();
    s.hud_press = Some((m, id, button, (x, y), false));
    s.taken_buttons.push(button);
    s.buttons[if button == MouseButton::Left { 0 } else { 1 }] = true;
    true
}

/// A HUD element's press released: a click, or the drag's end over the mod's element there (which gets a drop).
pub(super) fn hud_release(host: &mut super::Host, button: MouseButton) {
    let st = host.input.clone();
    let Some((m, id, _, start, dragging)) = st.borrow_mut().hud_press.take_if(|p| p.2 == button) else { return };
    let at = st.borrow().cursor;
    if !dragging {
        host.hud_events.push((m, id, hud_event("click", button, at, at)));
        return;
    }
    let over = super::hud::top_of(host, &m, at.0, at.1);
    let mut e = hud_event("drag_end", button, at, start);
    e["over"] = over.clone().into();
    host.hud_events.push((m.clone(), id.clone(), e));
    if let Some(o) = over.filter(|o| *o != id) {
        let mut d = hud_event("drop", button, at, start);
        d["from"] = id.into();
        host.hud_events.push((m, o, d));
    }
}

fn button_edge(w: &mut World, button: MouseButton, down: bool) -> bool {
    let st = w.lua.input.clone();
    // A press on a HUD element with a handler is its (and its release): a click, or a drag once it moves.
    if down && hud_press(&mut w.lua, button) {
        let (x, y) = st.borrow().cursor;
        // The filters still see the press, already taken, as they see its release.
        let e = serde_json::json!({"kind": "down", "button": button_name(button), "x": x.round() as i64, "y": y.round() as i64});
        filter_from(w, true, e, true);
        return true;
    }
    if !down {
        hud_release(&mut w.lua, button);
    }
    let (taken, (x, y)) = {
        let mut s = st.borrow_mut();
        if let Some(k) = [MouseButton::Left, MouseButton::Right, MouseButton::Middle].iter().position(|b| *b == button) {
            s.buttons[k] = down;
        }
        let taken = !down && s.taken_buttons.iter().position(|b| *b == button).map(|k| s.taken_buttons.remove(k)).is_some();
        (taken, s.cursor)
    };
    let took = filter(w, true, serde_json::json!({"kind": if down { "down" } else { "up" }, "button": button_name(button), "x": x.round() as i64, "y": y.round() as i64})) || taken;
    if took && down {
        st.borrow_mut().taken_buttons.push(button);
    }
    took
}

fn wheel(w: &mut World, d: f64) -> bool {
    let (x, y) = w.lua.input.borrow().cursor;
    filter(w, true, serde_json::json!({"kind": "wheel", "delta": d, "x": x.round() as i64, "y": y.round() as i64}))
}

/// Every running mod's key (or mouse) filters, in load order: a blocked event still reaches later ones (blocked =
/// true). True when one with input.capture returned true.
fn filter(w: &mut World, mouse: bool, e: serde_json::Value) -> bool {
    filter_from(w, mouse, e, false)
}

/// As `filter`, the event already taken (`blocked` from the first filter on) when `taken`.
fn filter_from(w: &mut World, mouse: bool, mut e: serde_json::Value, taken: bool) -> bool {
    let list = |st: &super::vm::ModState| if mouse { st.mouse_filters.len() } else { st.key_filters.len() };
    if w.lua.vms.iter().all(|v| list(&v.st.borrow()) == 0) {
        return taken;
    }
    let host = std::mem::take(&mut w.lua);
    let mut blocked = taken;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let funcs: Vec<Function> = {
            let st = vm.st.borrow();
            let keys = if mouse { &st.mouse_filters } else { &st.key_filters };
            keys.iter().filter_map(|(_, k)| vm.lua.registry_value(k).ok()).collect()
        };
        if funcs.is_empty() {
            continue;
        }
        let capture = vm.st.borrow().granted.iter().any(|g| g == "input.capture");
        lend(vm, w, |vm| {
            for f in funcs {
                e["blocked"] = blocked.into();
                let t = super::api::to_lua(&vm.lua, &e).unwrap_or(Value::Nil);
                // The player's input: actions may act from here.
                if let Some(r) = vm.call_filter(f, t, true)
                    && matches!(r.front(), Some(Value::Boolean(true)))
                {
                    if capture {
                        blocked = true;
                    } else {
                        vm.error(&mlua::Error::runtime("taking a key or the mouse needs input.capture"));
                    }
                }
            }
        });
    }
    w.lua = host;
    blocked
}

/// The first running mod's bind for this key edge (load order): queued for the mod phase.
fn bind(w: &mut World, code: K, mods: ModifiersState, down: bool, typing: bool) -> bool {
    for vm in w.lua.vms.iter().filter(|v| v.suspended.is_none()) {
        let (hit, on_release) = {
            let st = vm.st.borrow();
            let here = st.binds.iter().filter(|b| (b.typing || !typing) && b.combo.matches(code, mods));
            (here.clone().find(|b| b.up != down).map(|b| b.text.clone()), down && here.clone().any(|b| b.up))
        };
        if let Some(text) = hit {
            let id = vm.manifest.id.clone();
            w.lua.fired.push((id, text, !down));
            return true;
        }
        // A bind on the release takes the press too, so the game never sees the key.
        if on_release {
            return true;
        }
    }
    false
}

/// The binds that fired since the last mod phase: functions called as the player's input, lines run as typed (a
/// game command needs actions or automation). Returns the game lines.
pub(super) fn run_binds(host: &mut super::Host, w: &World, out: &mut Vec<(String, [u8; 4])>) -> Vec<String> {
    let mut lines = Vec::new();
    for (id, text, up) in std::mem::take(&mut host.fired) {
        let Some(vm) = host.vms.iter().find(|v| v.manifest.id == id && v.suspended.is_none()) else { continue };
        let act = vm.st.borrow().binds.iter().find(|b| b.text == text && b.up == up).map(|b| match &b.action {
            BindAction::Line(l) => Ok(l.clone()),
            BindAction::Func(k) => Err(vm.lua.registry_value::<Function>(k).ok()),
        });
        match act {
            Some(Ok(line)) => {
                let granted = |p: &str| vm.st.borrow().granted.iter().any(|g| g == p);
                let mods = granted("mods");
                if super::controls_mods(&line) && !mods {
                    out.push((format!("{}: the bind {text} loads or unloads mods, which needs mods.", vm.manifest.name), super::ERROR));
                } else if line.starts_with("//") || granted("actions") || granted("automation") {
                    lines.push((line, mods));
                } else {
                    out.push((format!("{}: the bind {text} runs a game command, which needs actions.", vm.manifest.name), super::ERROR));
                }
            }
            Some(Err(Some(f))) => lend(vm, w, |vm| vm.call_input(f, ())),
            _ => {}
        }
    }
    let mut game = Vec::new();
    for (line, mods) in lines {
        let line = line.trim().to_string();
        if line.starts_with('/') && host.takes(&line) {
            host.typed.push((line, super::From::Bind { mods }));
        } else {
            game.push(line);
        }
    }
    game
}

/// The pointer in window pixels.
pub(super) fn cursor(lua: &mlua::Lua) -> (f32, f32) {
    let c = shared(lua).borrow().cursor;
    (c.0 as f32, c.1 as f32)
}

fn shared(lua: &mlua::Lua) -> std::rc::Rc<std::cell::RefCell<State>> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<State>>>().map(|r| r.clone()).unwrap_or_default()
}

fn combo_arg(s: &str) -> mlua::Result<Combo> {
    Combo::parse(s).ok_or_else(|| mlua::Error::runtime(format!("no key {s:?} (e.g. \"f1\", \"ctrl+a\", \"^!1\", \"numpad+\")")))
}

/// The `input` and `binds` namespaces, and the chat line's reads and writes.
pub(super) fn install(lua: &mlua::Lua, g: &mlua::Table) -> mlua::Result<()> {
    use super::api::{need, next_id, state};
    let t = lua.create_table()?;
    for (name, mouse) in [("on_key", false), ("on_mouse", true)] {
        t.set(
            name,
            lua.create_function(move |lua, func: Function| {
                let st = state(lua);
                let id = next_id(&st);
                let key = lua.create_registry_value(func)?;
                let mut s = st.borrow_mut();
                if mouse { s.mouse_filters.push((id, key)) } else { s.key_filters.push((id, key)) }
                Ok(id)
            })?,
        )?;
    }
    t.set(
        "off",
        lua.create_function(|lua, id: u64| {
            let st = state(lua);
            let gone: Vec<RegistryKey> = {
                let mut guard = st.borrow_mut();
                let s: &mut super::vm::ModState = &mut guard;
                let mut gone = Vec::new();
                for list in [&mut s.key_filters, &mut s.mouse_filters] {
                    let (g, keep): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(|(i, _)| *i == id);
                    *list = keep;
                    gone.extend(g.into_iter().map(|(_, k)| k));
                }
                gone
            };
            for k in gone {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    t.set("held", lua.create_function(|lua, key: String| Ok(shared(lua).borrow().held(&key)))?)?;
    t.set(
        "modifiers",
        lua.create_function(|lua, ()| {
            let m = shared(lua).borrow().mods;
            let out = lua.create_table()?;
            out.set("ctrl", m.control_key())?;
            out.set("alt", m.alt_key())?;
            out.set("shift", m.shift_key())?;
            out.set("win", m.super_key())?;
            Ok(out)
        })?,
    )?;
    t.set(
        "mouse",
        lua.create_function(|lua, ()| {
            let s = shared(lua);
            let s = s.borrow();
            let out = lua.create_table()?;
            out.set("x", s.cursor.0.round() as i64)?;
            out.set("y", s.cursor.1.round() as i64)?;
            out.set("left", s.buttons[0])?;
            out.set("right", s.buttons[1])?;
            out.set("middle", s.buttons[2])?;
            Ok(out)
        })?,
    )?;
    t.set("typing", lua.create_function(|lua, ()| Ok(super::super::text_field_open(super::api::world(lua)?)))?)?;
    t.set(
        "press",
        lua.create_function(|lua, key: String| {
            need(lua, "input.capture")?;
            let c = combo_arg(&key)?;
            // The game reads no Windows key: a press with it would be a different one.
            if c.win {
                return Err(mlua::Error::runtime(format!("{key:?}: the Windows key cannot be pressed for the game")));
            }
            super::api::push_op(lua, super::Op::Key(Synth { key: c.key, down: None, ctrl: c.ctrl, alt: c.alt, shift: c.shift }))?;
            Ok(())
        })?,
    )?;
    t.set(
        "send",
        lua.create_function(|lua, (key, down): (String, bool)| {
            need(lua, "input.capture")?;
            let key = code_of(&key).ok_or_else(|| mlua::Error::runtime(format!("no key {key:?}")))?;
            super::api::push_op(lua, super::Op::Key(Synth { key, down: Some(down), ctrl: false, alt: false, shift: false }))?;
            let st = super::api::state(lua);
            let mut s = st.borrow_mut();
            s.keys_down.retain(|k| *k != key);
            if down {
                s.keys_down.push(key);
            }
            Ok(())
        })?,
    )?;
    g.set("input", t)?;

    let binds = lua.create_table()?;
    binds.set(
        "set",
        lua.create_function(|lua, (key, action, opts): (String, Value, Option<mlua::Table>)| {
            let combo = combo_arg(&key)?;
            let up = opts.as_ref().and_then(|o| o.get::<Option<bool>>("up").ok().flatten()).unwrap_or(false);
            let typing = opts.as_ref().and_then(|o| o.get::<Option<bool>>("typing").ok().flatten()).unwrap_or(false);
            let action = match action {
                Value::String(s) => BindAction::Line(s.to_string_lossy()),
                Value::Function(f) => BindAction::Func(lua.create_registry_value(f)?),
                _ => return Err(mlua::Error::runtime("binds.set(key, line or function[, {up, typing}])")),
            };
            let st = state(lua);
            let old = {
                let mut s = st.borrow_mut();
                let old = s.binds.iter().position(|b| b.combo == combo && b.up == up).map(|k| s.binds.remove(k));
                s.binds.push(Bind { combo, text: key, up, typing, action });
                old
            };
            if let Some(Bind { action: BindAction::Func(k), .. }) = old {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    binds.set(
        "unset",
        lua.create_function(|lua, key: String| {
            let combo = combo_arg(&key)?;
            let st = state(lua);
            let gone: Vec<Bind> = {
                let mut s = st.borrow_mut();
                let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut s.binds).into_iter().partition(|b| b.combo == combo);
                s.binds = keep;
                gone
            };
            for b in gone {
                if let BindAction::Func(k) = b.action {
                    lua.remove_registry_value(k)?;
                }
            }
            Ok(())
        })?,
    )?;
    binds.set(
        "list",
        lua.create_function(|lua, ()| {
            let st = state(lua);
            let out = lua.create_table()?;
            for b in st.borrow().binds.iter() {
                let t = lua.create_table()?;
                t.set("key", b.text.as_str())?;
                if let BindAction::Line(l) = &b.action {
                    t.set("line", l.as_str())?;
                }
                t.set("up", b.up)?;
                t.set("typing", b.typing)?;
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    g.set("binds", binds)?;

    let chat: mlua::Table = g.get("chat")?;
    chat.set(
        "line",
        lua.create_function(|lua, ()| {
            let w = super::api::world(lua)?;
            let out = lua.create_table()?;
            out.set("open", w.typing)?;
            out.set("text", if w.typing { w.chat_input.as_str() } else { "" })?;
            Ok(out)
        })?,
    )?;
    chat.set(
        "set_line",
        lua.create_function(|lua, (text, open): (Option<String>, Option<bool>)| {
            super::api::push_op(lua, super::Op::Line { text: text.unwrap_or_default(), open: open.unwrap_or(true) })?;
            Ok(())
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos() {
        let c = Combo::parse("ctrl+f1").unwrap();
        assert_eq!((c.key, c.ctrl, c.alt, c.shift), (K::F1, true, false, false));
        let c = Combo::parse("^!numpad+").unwrap();
        assert_eq!((c.key, c.ctrl, c.alt), (K::NumpadAdd, true, true));
        let c = Combo::parse("Alt+Shift+1").unwrap();
        assert_eq!((c.key, c.alt, c.shift, c.ctrl), (K::Digit1, true, true, false));
        assert_eq!(Combo::parse("-").unwrap().key, K::Minus);
        assert_eq!(Combo::parse("~").map(|c| c.key), None);
        assert!(Combo::parse("ctrl+nothing").is_none());
        let m = ModifiersState::CONTROL;
        assert!(Combo::parse("^a").unwrap().matches(K::KeyA, m));
        assert!(!Combo::parse("a").unwrap().matches(K::KeyA, m));
    }

    #[test]
    fn names_round_trip() {
        for (name, code, _) in KEYS {
            assert_eq!(code_of(name), Some(*code), "{name}");
            assert_eq!(name_of(*code), Some(*name));
        }
    }
}
