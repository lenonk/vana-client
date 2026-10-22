//! Retail key functions (FUN_1011ff30): a DIK key state built each frame (FUN_10005520), the binding table
//! 0x103619f0 with its context masks 0x103612e8, and the compact keyboard's virtual pad (FUN_1011fbc0). The tables
//! come from FFXiMain; Key Assignment rewrites them as FUN_10195590 / FUN_10195940 / FUN_10195910 do.

use winit::keyboard::KeyCode;

/// Key function ids 0x00..0xb3.
pub const IDS: usize = 0xb4;
const BINDINGS_VA: u32 = 0x103619f0;
const MASKS_VA: u32 = 0x103612e8;
const COMPACT_VA: u32 = 0x103623c8;
const K1_VA: u32 = 0x1036e0a8;
const K2_VA: u32 = 0x1036e190;
const K2_ROWS: usize = 28;
/// Key repeat (input +0x94 = 16, FUN_10005210): past 16 ticks held it fires and drops back to 14. Counted in
/// retail frames of 2 ticks, as retail ran.
const REPEAT_DELAY: f32 = 16.0;
const REPEAT_BACK: f32 = 2.0;
const FRAME_TICKS: f32 = 2.0;

/// Key state bits (FUN_10120340..FUN_10120530).
const REPEAT: u8 = 1;
const PRESS: u8 = 2;
const HELD: u8 = 4;
const RELEASE: u8 = 8;

/// Key-state modes of FUN_1011f850.
pub mod mode {
    pub const REPEAT: i32 = 0;
    pub const PRESS: i32 = 1;
    pub const REPEAT_CONSUME: i32 = 2;
    pub const PRESS_CONSUME: i32 = 3;
    pub const HELD: i32 = 4;
}

/// Pad bit each key function's callback 0 tests (PTR_FUN_10361e28; kspec dump.txt), i.e. the compact keys.
const PAD: &[(u8, u64)] = &[
    (0x00, 0x40), (0x27, 0x40), (0x2b, 0x40), (0x31, 0x40),
    (0x01, 0x80), (0x28, 0x80), (0x2c, 0x80), (0x32, 0x80),
    (0x02, 0x10), (0x29, 0x10), (0x2d, 0x10), (0x33, 0x10),
    (0x03, 0x20), (0x2a, 0x20), (0x2e, 0x20), (0x34, 0x20),
    (0x0d, 1), (0x0e, 1), (0x8f, 1), (0x91, 1), (0x93, 1),
    (0x0f, 8), (0x90, 8), (0x92, 8), (0x94, 8), (0x95, 8), (0x9c, 8),
    (0x11, 4), (0x22, 4), (0x78, 4),
    (0x18, 2), (0x19, 2), (0x25, 2), (0x35, 2), (0x8d, 2),
    (0x10, 0x4000), (0x7d, 0x4000), (0xa7, 0x4000),
    (0x12, 0x100), (0x15, 0x100), (0x1c, 0x100), (0x1f, 0x100),
    (0x13, 0x8000), (0x14, 0x8000),
    (0x16, 0x200), (0x21, 0x200), (0x86, 0x200), (0x89, 0x200),
    (0x17, 0x2000), (0x20, 0x2000), (0x85, 0x2000), (0x8a, 0x2000), (0x9a, 0x2000),
    (0x1a, 0x1000), (0x1e, 0x1000), (0x24, 0x1000),
    (0x26, 0x10000), (0x2f, 0x10000), (0x56, 0x10000),
    (0x30, 0x400), (0x44, 0x400), (0x7e, 0x400), (0xa6, 0x400),
    (0x45, 0x2000_0000),
    (0x4f, 0x800_0000), (0xa4, 0x800_0000),
    (0x50, 0x1000_0000), (0xa5, 0x1000_0000),
    (0x55, 0x4000_0000),
];

/// A compact keyboard record (0x103623c8, 0x10 bytes): key, modifier rule, state mask, pad bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compact {
    pub key: u8,
    pub rule: u8,
    pub states: u8,
    pub bits: u64,
}

/// The game around the keys this frame.
#[derive(Clone, Copy, Default)]
pub struct Frame {
    /// Frame length in 1/60 s ticks.
    pub ticks: f32,
    /// The chat line is open (FUN_101adbd0).
    pub typing: bool,
    /// Keyboard type, config 0x42: 0 full, 1 / 2 compact.
    pub keyboard: u32,
    /// A window is active (FUN_101584c0).
    pub window: bool,
    /// Key Assignment (k1assign / k2assign) is open: the compact keys are off and flag 0x10 fails.
    pub assigning: bool,
}

#[derive(Default)]
struct Pad {
    held: u64,
    edge: u64,
    repeat: u64,
    /// +0x248 and the first-frame flag (+0xda8).
    counter: f32,
    armed: bool,
}

pub struct Input {
    /// Table defaults from FFXiMain and the live copies Key Assignment rewrites.
    base_bindings: Vec<[u16; 3]>,
    base_compact: Vec<Compact>,
    pub bindings: Vec<[u16; 3]>,
    masks: Vec<u16>,
    pub compact: Vec<Compact>,
    k1: Vec<Vec<u8>>,
    k2: Vec<Vec<u16>>,
    /// The config values the tables were last rewritten from.
    assigned: Option<Vec<u32>>,
    keys: [u8; 256],
    /// Device state from the window, and presses seen since the last frame (a tap shorter than a frame counts).
    down: [bool; 256],
    tapped: [bool; 256],
    counters: [f32; 256],
    clock: f32,
    pad: Pad,
    /// The input context (DAT_10362678): 0xffff in the field, 0x26 with the map open, 0x42 laying out the Mog House.
    pub context: u16,
    typing: bool,
    assigning: bool,
    window: bool,
    /// DAT_104cf22c: a handler took Enter / Esc this frame (FUN_101202a0); only mode 4 tests still pass.
    consumed: bool,
    /// NumLock, from the last numpad digit's logical key (winit reports no toggle state).
    numlock: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            base_bindings: vec![[0; 3]; IDS],
            base_compact: Vec::new(),
            bindings: vec![[0; 3]; IDS],
            masks: vec![0; IDS],
            compact: Vec::new(),
            k1: Vec::new(),
            k2: Vec::new(),
            assigned: None,
            keys: [0; 256],
            down: [false; 256],
            tapped: [false; 256],
            counters: [0.0; 256],
            clock: 0.0,
            pad: Pad::default(),
            context: 0xffff,
            typing: false,
            assigning: false,
            window: false,
            consumed: false,
            numlock: true,
        }
    }
}

impl Input {
    /// The tables from FFXiMain.dll. Missing data leaves them empty: no key function fires.
    pub fn load(&mut self, ffximain: &[u8]) {
        let va = |a: u32, n: usize| crate::dat::battle::va_bytes(ffximain, a, n);
        let u16s = |b: &[u8]| b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<u16>>();
        if let (Some(b), Some(m)) = (va(BINDINGS_VA, IDS * 6), va(MASKS_VA, IDS * 2)) {
            self.base_bindings = u16s(b).chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            self.masks = u16s(m);
        }
        self.base_compact.clear();
        let mut at = COMPACT_VA;
        while let Some(r) = va(at, 16) {
            let bits = u64::from_le_bytes(r[8..16].try_into().unwrap());
            if bits == 0 {
                break;
            }
            self.base_compact.push(Compact { key: r[0], rule: r[1], states: r[2], bits });
            at += 16;
        }
        self.k1.clear();
        let mut at = K1_VA;
        while let Some(r) = va(at, 4).filter(|r| r[0] != 0xff) {
            self.k1.push(r.iter().copied().take_while(|&i| i != 0xff).collect());
            at += 4;
        }
        self.k2 = va(K2_VA, K2_ROWS * 10).map_or(Vec::new(), |b| u16s(b).chunks(5).map(|r| r.iter().copied().take_while(|&i| i != 0xffff).collect()).collect());
        self.bindings = self.base_bindings.clone();
        self.compact = self.base_compact.clone();
        self.assigned = None;
    }

    /// Key Assignment (k1 config 0x4a.., k2 0x6a..) rewrites slot 0 of the k2 ids with Ctrl / Alt + key, the
    /// compact records' keys, and with keyboard type 2 the turn ids 0xaa / 0xa9 (no Ctrl / Alt).
    pub fn assign(&mut self, game: &crate::gamecfg::Values) {
        let now: Vec<u32> = std::iter::once(game.get(0x42)).chain((0x4a..0x8a).map(|id| game.get(id))).collect();
        if self.assigned.as_ref() == Some(&now) {
            return;
        }
        self.bindings = self.base_bindings.clone();
        self.compact = self.base_compact.clone();
        let slot0 = |b: &mut Vec<[u16; 3]>, id: usize, flag: u16, key: u32| {
            if let Some(s) = b.get_mut(id) {
                s[0] = if key & 0xff != 0 { flag << 8 | (key & 0xff) as u16 } else { 0 };
            }
        };
        for (row, ids) in self.k2.iter().enumerate() {
            for &id in ids {
                slot0(&mut self.bindings, id as usize, 0x10, game.get(0x6a + row as u16));
            }
        }
        for (row, recs) in self.k1.iter().enumerate() {
            for &r in recs {
                if let Some(c) = self.compact.get_mut(r as usize) {
                    c.key = game.get(0x4a + row as u16) as u8;
                }
            }
        }
        if game.get(0x42) == 2 {
            slot0(&mut self.bindings, 0xaa, 0x80, game.get(0x66));
            slot0(&mut self.bindings, 0xa9, 0x80, game.get(0x67));
        }
        self.assigned = Some(now);
    }

    /// A key from the window. `logical` tells NumLock: a numpad digit types a digit only with it on.
    pub fn key(&mut self, code: KeyCode, pressed: bool, logical: &winit::keyboard::Key) {
        let Some(k) = dik(code) else { return };
        if matches!(k, 0x47..=0x49 | 0x4b..=0x4d | 0x4f..=0x52) {
            self.numlock = matches!(logical, winit::keyboard::Key::Character(_));
        }
        self.set(k, pressed);
    }

    /// A key going down or up by DIK code (the window, and test drives act:down / act:up).
    pub fn set(&mut self, k: u8, pressed: bool) {
        self.down[k as usize] = pressed;
        if pressed {
            self.tapped[k as usize] = true;
        }
    }

    /// A press and release within one frame (test drives, `act:` keys).
    pub fn tap(&mut self, k: u8) {
        self.tapped[k as usize] = true;
    }

    /// A drive's egui key event: the key and its modifiers count as tapped this frame.
    pub fn drive_event(&mut self, e: &egui::Event) {
        if let egui::Event::Key { key, pressed: true, modifiers, .. } = e
            && let Some(k) = egui_dik(*key)
        {
            self.tap(k);
            for (on, m) in [(modifiers.ctrl, 0x1d), (modifiers.shift, 0x2a), (modifiers.alt, 0x38)] {
                if on {
                    self.tap(m);
                }
            }
        }
    }

    /// Drops every key (focus lost: the window gets no releases).
    pub fn clear(&mut self) {
        self.down = [false; 256];
        self.tapped = [false; 256];
    }

    /// The frame's key state (FUN_10005520), the chat line's Enter / Esc, then the compact pad (FUN_1011fbc0).
    pub fn begin_frame(&mut self, f: Frame) {
        self.consumed = false;
        self.typing = f.typing;
        self.assigning = f.assigning;
        self.window = f.window;
        self.clock += f.ticks;
        let frames = (self.clock / FRAME_TICKS).floor();
        self.clock -= frames * FRAME_TICKS;
        for k in 0..256 {
            let now = self.down[k] || self.tapped[k];
            self.tapped[k] = false;
            let s = &mut self.keys[k];
            if now {
                if *s & HELD == 0 {
                    *s = HELD | PRESS | REPEAT;
                    self.counters[k] = 0.0;
                } else {
                    *s &= !(REPEAT | PRESS | RELEASE);
                    for _ in 0..frames as u32 {
                        self.counters[k] += FRAME_TICKS;
                        if self.counters[k] > REPEAT_DELAY {
                            *s |= REPEAT;
                            self.counters[k] = REPEAT_DELAY - REPEAT_BACK;
                        }
                    }
                }
            } else {
                *s = if *s & HELD != 0 { RELEASE } else { 0 };
                self.counters[k] = 0.0;
            }
        }
        // The chat line takes Enter / Esc before any key function sees them.
        if f.typing {
            for k in [0x1c, 0x9c, 0x01] {
                if self.keys[k] & PRESS != 0 {
                    self.keys[k] &= !(REPEAT | PRESS | RELEASE);
                }
            }
        }
        self.pad_frame(&f, frames as u32);
    }

    fn pad_frame(&mut self, f: &Frame, frames: u32) {
        let prev = self.pad.held;
        let held = if matches!(f.keyboard, 1 | 2) && !f.typing && !f.assigning { self.compact_bits(f.window) } else { 0 };
        let mut edge = held & !prev;
        let mut repeat = edge;
        // Up or down drops left / right from the edges.
        if edge & 0xc0 != 0 {
            edge &= !0x30;
        }
        if repeat & 0xc0 != 0 {
            repeat &= !0x30;
        }
        if edge != 0 {
            self.pad.counter = 0.0;
            self.pad.armed = false;
        } else if held != 0 {
            for _ in 0..frames {
                self.pad.counter += FRAME_TICKS;
                if !self.pad.armed {
                    self.pad.armed = true;
                } else if self.pad.counter > REPEAT_DELAY {
                    repeat |= held;
                    self.pad.counter = REPEAT_DELAY - REPEAT_BACK;
                }
            }
        }
        self.pad.held = held;
        self.pad.edge = edge;
        self.pad.repeat = repeat;
    }

    /// FUN_1011fbc0: the bits of every record whose key is held under its modifier rule and state mask.
    fn compact_bits(&self, window: bool) -> u64 {
        let (ctrl, alt, shift) = (self.ctrl(), self.alt(), self.shift());
        let states = if window { 5 } else { 6 };
        self.compact
            .iter()
            .filter(|c| c.key != 0 && c.states & states == c.states && self.held(c.key))
            .filter(|c| match (ctrl, alt) {
                (false, false) => matches!(c.rule, 0 | 7) || (c.rule == 4 && shift) || (c.rule == 8 && !shift),
                _ => (ctrl && !alt && c.rule == 1) || (alt && !ctrl && c.rule == 2) || c.rule == 7 || (c.rule == 0 && !self.k2_key(c.key)),
            })
            .fold(0, |b, c| b | c.bits)
    }

    /// FUN_101953b0: the key is some k2 row's (slot 0 of its first id).
    fn k2_key(&self, key: u8) -> bool {
        self.k2.iter().filter_map(|r| r.first()).any(|&id| self.bindings.get(id as usize).is_some_and(|b| b[0] as u8 == key))
    }

    /// The compact pad's held bits (the stick directions W / A / S / D and I / J / K / L, among others).
    #[cfg(test)]
    pub fn pad_held(&self) -> u64 {
        self.pad.held
    }

    /// FUN_1011fe30 for the movement (4 X, 5 Y) and camera (6 X, 7 Y) axes: the larger of the raw keys
    /// (FUN_1011f300: numpad 6 / 4, 2 / 8, arrows) and the compact stick (W / A / S / D, I / J / K / L; byte
    /// (b - 0x80) / 128, the later record winning). While typing only numpad movement with NumLock off counts;
    /// with a window active the arrows need Shift (FUN_1011f7b0 / FUN_1011f910).
    pub fn axis(&self, id: usize) -> f32 {
        if !(4..=7).contains(&id) || self.masks[id] & self.context == 0 {
            return 0.0;
        }
        let key = |p: u8, n: u8| self.held(p) as i32 as f32 - self.held(n) as i32 as f32;
        let stick = |p: u64, n: u64| -> f32 { if self.pad.held & p != 0 { 127.0 / 128.0 } else if self.pad.held & n != 0 { -1.0 } else { 0.0 } };
        let arrows = !self.typing && (!self.window || self.shift());
        let (keys, pad) = match id {
            4 => (if self.typing && self.numlock { 0.0 } else { key(0x4d, 0x4b) }, stick(0x20_0000, 0x40_0000)),
            5 => (if self.typing && self.numlock { 0.0 } else { key(0x50, 0x48) }, stick(0x100_0000, 0x80_0000)),
            6 => (if arrows { key(0xcd, 0xcb) } else { 0.0 }, stick(0x2_0000, 0x4_0000)),
            _ => (if arrows { key(0xd0, 0xc8) } else { 0.0 }, stick(0x10_0000, 0x8_0000)),
        };
        if keys.abs() >= pad.abs() { keys } else { pad }
    }

    fn held(&self, k: u8) -> bool {
        self.keys[k as usize] & HELD != 0
    }

    fn ctrl(&self) -> bool {
        self.held(0x1d) || self.held(0x9d)
    }

    fn alt(&self) -> bool {
        self.held(0x38) || self.held(0xb8)
    }

    fn shift(&self) -> bool {
        self.held(0x2a) || self.held(0x36)
    }

    /// FUN_101202a0: Enter / Esc were taken; key functions other than mode 4 fail for the rest of the frame.
    pub fn consume(&mut self) {
        self.consumed = true;
    }

    /// FUN_1011ff30: key function `id` in `mode` (see `mode`).
    pub fn test(&mut self, id: usize, mode: i32) -> bool {
        if (self.consumed && mode != mode::HELD) || id >= IDS {
            return false;
        }
        let m = self.masks[id];
        if m == 0 || m & self.context == 0 || (m & 0x41 != 0 && self.typing) {
            return false;
        }
        if self.pad_test(id, mode) {
            return true;
        }
        for slot in 0..3 {
            let b = self.bindings[id][slot];
            if b != 0 && self.modifiers_ok((b >> 8) as u8) && self.key_test(b as u8, mode) {
                return true;
            }
        }
        false
    }

    fn modifiers_ok(&self, fl: u8) -> bool {
        let (ctrl, alt, shift) = (self.ctrl(), self.alt(), self.shift());
        !(fl & 0x40 != 0 && self.typing)
            && (fl & 0x80 == 0 || (!ctrl && !alt))
            && (fl & 0x01 == 0 || shift)
            && (fl & 0x02 == 0 || ctrl)
            && (fl & 0x04 == 0 || alt)
            && (fl & 0x08 == 0 || (ctrl && alt))
            && (fl & 0x10 == 0 || ((ctrl || alt) && !self.assigning))
            && (fl & 0x20 == 0 || !shift)
    }

    /// FUN_1011f850, with LCtrl / LShift / LAlt bindings meaning "either one held", in mode 4 only.
    fn key_test(&mut self, k: u8, mode: i32) -> bool {
        match k {
            0x1d => return mode == mode::HELD && self.ctrl(),
            0x2a => return mode == mode::HELD && self.shift(),
            0x38 => return mode == mode::HELD && self.alt(),
            _ => {}
        }
        // With NumLock on the chat line types the numpad digits.
        if self.typing && self.numlock && matches!(k, 0x47..=0x49 | 0x4b..=0x4d | 0x4f..=0x52) {
            return false;
        }
        let s = &mut self.keys[k as usize];
        let (bit, consume) = match mode {
            mode::REPEAT => (REPEAT, false),
            mode::PRESS => (PRESS, false),
            mode::REPEAT_CONSUME => (REPEAT, true),
            mode::PRESS_CONSUME => (PRESS, true),
            mode::HELD => (HELD, false),
            _ => return false,
        };
        let hit = *s & bit != 0;
        if hit && consume {
            *s &= !bit;
        }
        hit
    }

    /// The pad callback's test (FUN_1011da00 and kin): +0x238 repeat, +0x228 edge, +0x220 held. Modes 2 / 3 keep
    /// only this bit's edge, as retail does.
    fn pad_test(&mut self, id: usize, mode: i32) -> bool {
        let Some(&(_, bit)) = PAD.iter().find(|p| p.0 as usize == id) else { return false };
        let p = &mut self.pad;
        match mode {
            mode::REPEAT => p.repeat & bit != 0,
            mode::PRESS => p.edge & bit != 0,
            mode::REPEAT_CONSUME => {
                p.edge &= bit;
                p.repeat & bit != 0
            }
            mode::PRESS_CONSUME => {
                p.edge &= bit;
                p.edge != 0
            }
            mode::HELD => p.held & bit != 0,
            _ => false,
        }
    }
}

/// The DirectInput code of a key.
pub fn dik(code: KeyCode) -> Option<u8> {
    use KeyCode as K;
    Some(match code {
        K::Escape => 0x01,
        K::Digit1 => 0x02,
        K::Digit2 => 0x03,
        K::Digit3 => 0x04,
        K::Digit4 => 0x05,
        K::Digit5 => 0x06,
        K::Digit6 => 0x07,
        K::Digit7 => 0x08,
        K::Digit8 => 0x09,
        K::Digit9 => 0x0a,
        K::Digit0 => 0x0b,
        K::Minus => 0x0c,
        K::Equal => 0x0d,
        K::Backspace => 0x0e,
        K::Tab => 0x0f,
        K::KeyQ => 0x10,
        K::KeyW => 0x11,
        K::KeyE => 0x12,
        K::KeyR => 0x13,
        K::KeyT => 0x14,
        K::KeyY => 0x15,
        K::KeyU => 0x16,
        K::KeyI => 0x17,
        K::KeyO => 0x18,
        K::KeyP => 0x19,
        K::BracketLeft => 0x1a,
        K::BracketRight => 0x1b,
        K::Enter => 0x1c,
        K::ControlLeft => 0x1d,
        K::KeyA => 0x1e,
        K::KeyS => 0x1f,
        K::KeyD => 0x20,
        K::KeyF => 0x21,
        K::KeyG => 0x22,
        K::KeyH => 0x23,
        K::KeyJ => 0x24,
        K::KeyK => 0x25,
        K::KeyL => 0x26,
        K::Semicolon => 0x27,
        K::Quote => 0x28,
        K::Backquote => 0x29,
        K::ShiftLeft => 0x2a,
        K::Backslash => 0x2b,
        K::KeyZ => 0x2c,
        K::KeyX => 0x2d,
        K::KeyC => 0x2e,
        K::KeyV => 0x2f,
        K::KeyB => 0x30,
        K::KeyN => 0x31,
        K::KeyM => 0x32,
        K::Comma => 0x33,
        K::Period => 0x34,
        K::Slash => 0x35,
        K::ShiftRight => 0x36,
        K::NumpadMultiply => 0x37,
        K::AltLeft => 0x38,
        K::Space => 0x39,
        K::CapsLock => 0x3a,
        K::F1 => 0x3b,
        K::F2 => 0x3c,
        K::F3 => 0x3d,
        K::F4 => 0x3e,
        K::F5 => 0x3f,
        K::F6 => 0x40,
        K::F7 => 0x41,
        K::F8 => 0x42,
        K::F9 => 0x43,
        K::F10 => 0x44,
        K::NumLock => 0x45,
        K::ScrollLock => 0x46,
        K::Numpad7 => 0x47,
        K::Numpad8 => 0x48,
        K::Numpad9 => 0x49,
        K::NumpadSubtract => 0x4a,
        K::Numpad4 => 0x4b,
        K::Numpad5 => 0x4c,
        K::Numpad6 => 0x4d,
        K::NumpadAdd => 0x4e,
        K::Numpad1 => 0x4f,
        K::Numpad2 => 0x50,
        K::Numpad3 => 0x51,
        K::Numpad0 => 0x52,
        K::NumpadDecimal => 0x53,
        K::IntlBackslash => 0x56,
        K::F11 => 0x57,
        K::F12 => 0x58,
        K::NumpadEnter => 0x9c,
        K::ControlRight => 0x9d,
        K::NumpadComma => 0xb3,
        K::NumpadDivide => 0xb5,
        K::PrintScreen => 0xb7,
        K::AltRight => 0xb8,
        K::Pause => 0xc5,
        K::Home => 0xc7,
        K::ArrowUp => 0xc8,
        K::PageUp => 0xc9,
        K::ArrowLeft => 0xcb,
        K::ArrowRight => 0xcd,
        K::End => 0xcf,
        K::ArrowDown => 0xd0,
        K::PageDown => 0xd1,
        K::Insert => 0xd2,
        K::Delete => 0xd3,
        K::SuperLeft => 0xdb,
        K::SuperRight => 0xdc,
        K::ContextMenu => 0xdd,
        _ => return None,
    })
}

/// The DirectInput code of an egui key (test drives; egui does not tell the numpad apart).
fn egui_dik(key: egui::Key) -> Option<u8> {
    use egui::Key as E;
    Some(match key {
        E::Escape => 0x01,
        E::Minus => 0x0c,
        E::Equals => 0x0d,
        E::Backspace => 0x0e,
        E::Tab => 0x0f,
        E::OpenBracket => 0x1a,
        E::CloseBracket => 0x1b,
        E::Enter => 0x1c,
        E::Semicolon => 0x27,
        E::Quote => 0x28,
        E::Backtick => 0x29,
        E::Backslash => 0x2b,
        E::Comma => 0x33,
        E::Period => 0x34,
        E::Slash => 0x35,
        E::Space => 0x39,
        E::Home => 0xc7,
        E::ArrowUp => 0xc8,
        E::PageUp => 0xc9,
        E::ArrowLeft => 0xcb,
        E::ArrowRight => 0xcd,
        E::End => 0xcf,
        E::ArrowDown => 0xd0,
        E::PageDown => 0xd1,
        E::Insert => 0xd2,
        E::Delete => 0xd3,
        _ => {
            let n = key.name();
            let letters = b"QWERTYUIOP\0\0\0\0ASDFGHJKL\0\0\0\0\0ZXCVBNM";
            if n.len() == 1 && let Some(i) = letters.iter().position(|&c| c == n.as_bytes()[0]) {
                return Some(0x10 + i as u8);
            }
            if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() {
                return Some(match n.as_bytes()[0] {
                    b'0' => 0x0b,
                    d => 0x02 + (d - b'1'),
                });
            }
            let f = n.strip_prefix('F').and_then(|s| s.parse::<u8>().ok())?;
            return match f {
                1..=10 => Some(0x3b + f - 1),
                11 => Some(0x57),
                12 => Some(0x58),
                _ => None,
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Option<Input> {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).ok()?;
        let mut i = Input::default();
        i.load(&dll);
        Some(i)
    }

    fn frame() -> Frame {
        Frame { ticks: 1.0, keyboard: 2, ..Default::default() }
    }

    /// The tables match the kspec dump: Num* on 0x44 (not typing, no Ctrl / Alt), Ctrl / Alt + F and Num+ on 0x78,
    /// 42 compact records, k1 / k2 rows.
    #[test]
    fn tables_from_ffximain() {
        let Some(i) = real() else { return };
        assert_eq!(i.bindings[0x44], [0xc037, 0, 0]);
        assert_eq!(i.bindings[0x78], [0x1021, 0x804e, 0]);
        assert_eq!((i.masks[0x78], i.masks[0x37], i.masks[0x44]), (0xffff, 0x3e, 0x08));
        assert_eq!(i.compact.len(), 42);
        assert_eq!(i.compact[6], Compact { key: 0x23, rule: 0, states: 4, bits: 0x400 });
        assert_eq!((i.k1.len(), i.k1[4].as_slice()), (28, &[11u8, 15, 19, 23][..]));
        assert_eq!((i.k2.len(), i.k2[0].as_slice()), (K2_ROWS, &[0x3bu16][..]));
    }

    #[test]
    fn press_hold_repeat_release() {
        let Some(mut i) = real() else { return };
        let nl = winit::keyboard::Key::Named(winit::keyboard::NamedKey::NumLock);
        i.key(KeyCode::NumpadAdd, true, &nl);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS) && i.test(0x78, mode::REPEAT));
        // Held at 60 fps: retail frames of 2 ticks, past 16 the first repeat (17 or 18 ticks by the frame clock's
        // phase), then every 4.
        let mut repeats = Vec::new();
        for t in 1..=30 {
            i.begin_frame(frame());
            assert!(!i.test(0x78, mode::PRESS) && i.test(0x78, mode::HELD));
            if i.test(0x78, mode::REPEAT) {
                repeats.push(t);
            }
        }
        assert!((17..=18).contains(&repeats[0]) && repeats.windows(2).all(|w| w[1] - w[0] == 4) && repeats.len() == 4, "{repeats:?}");
        i.key(KeyCode::NumpadAdd, false, &nl);
        i.begin_frame(frame());
        assert!(!i.test(0x78, mode::HELD));
        // A tap inside one frame still presses.
        i.key(KeyCode::NumpadAdd, true, &nl);
        i.key(KeyCode::NumpadAdd, false, &nl);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS));
    }

    #[test]
    fn modifiers_typing_and_consume() {
        let Some(mut i) = real() else { return };
        let k = winit::keyboard::Key::Character("f".into());
        // Ctrl + F is 0x78 (flag 0x10); with a full keyboard bare F is not.
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        assert!(!i.test(0x78, mode::PRESS));
        i.key(KeyCode::ControlLeft, true, &k);
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        assert!(i.test(0x78, mode::PRESS));
        // Not while Key Assignment is open (the gate), nor typing (mask 0xffff).
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, assigning: true, ..frame() });
        assert!(!i.test(0x78, mode::PRESS));
        i.key(KeyCode::ControlLeft, false, &k);
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(frame());
        // Ctrl / Alt + T (0x38, mask 0x3e) works while typing; Num+ (0x78) does not.
        i.key(KeyCode::AltLeft, true, &k);
        i.key(KeyCode::KeyT, true, &k);
        i.key(KeyCode::NumpadAdd, true, &k);
        i.begin_frame(Frame { typing: true, ..frame() });
        assert!(i.test(0x38, mode::PRESS) && !i.test(0x78, mode::PRESS));
        // A consumed frame fails everything but mode 4; mode 3 eats the press.
        i.begin_frame(frame());
        i.key(KeyCode::AltLeft, false, &k);
        i.key(KeyCode::KeyT, false, &k);
        i.key(KeyCode::Escape, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x92, mode::PRESS_CONSUME) && !i.test(0x92, mode::PRESS));
        i.consume();
        assert!(!i.test(0x94, mode::PRESS) && i.test(0x94, mode::HELD));
    }

    /// Compact keyboard: bare F is the pad's 4 bit (0x78 fires); W holds the stick; Ctrl + I (a k2 key) is the
    /// Items shortcut, not the camera.
    #[test]
    fn compact_keys() {
        let Some(mut i) = real() else { return };
        let k = winit::keyboard::Key::Character("x".into());
        i.key(KeyCode::KeyF, true, &k);
        i.key(KeyCode::KeyW, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS));
        assert_eq!(i.pad_held() & 0x800004, 0x800004);
        i.begin_frame(frame());
        assert!(!i.test(0x78, mode::PRESS) && i.test(0x78, mode::HELD));
        i.key(KeyCode::KeyF, false, &k);
        i.key(KeyCode::KeyW, false, &k);
        i.key(KeyCode::ControlLeft, true, &k);
        i.key(KeyCode::KeyI, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x39, mode::PRESS));
        assert_eq!(i.pad_held() & 0x80000, 0);
        // Typing turns the compact keys off.
        i.key(KeyCode::ControlLeft, false, &k);
        i.key(KeyCode::KeyI, false, &k);
        i.key(KeyCode::KeyH, true, &k);
        i.begin_frame(Frame { typing: true, ..frame() });
        assert!(!i.test(0x44, mode::PRESS));
    }

    /// Key Assignment: k2 row 0 (Attack, 0x3b) to G; k1 row 6 (Menu Cursor & Camera Left) to U; turns on type 2.
    #[test]
    fn key_assignment() {
        let Some(mut i) = real() else { return };
        let mut v = crate::gamecfg::Values::default();
        v.set(0x6a, 0x22);
        v.set(0x50, 0x16);
        i.assign(&v);
        assert_eq!(i.bindings[0x3b][0], 0x1022);
        assert!([13usize, 17, 21, 25].iter().all(|&r| i.compact[r].key == 0x16));
        assert_eq!((i.bindings[0xaa][0], i.bindings[0xa9][0]), (0x8012, 0x8010));
        v.set(0x42, 0);
        i.assign(&v);
        assert_eq!(i.bindings[0xaa][0], i.base_bindings[0xaa][0]);
    }
}
