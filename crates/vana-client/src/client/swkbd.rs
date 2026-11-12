//! The software keyboard ("menu    keypadus", handler DAT_10662dac, vtable 0x10337940; MODLOG "Software keyboard"):
//! the pad's RB with config 0xc9 on opens it over the open text field (or the chat line), and its keys type there.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const NAME: &str = "menu    keypadus";
const CAPS: &str = "menu    key1topu";
const HAND: &str = "menu    yubi";

/// A page of table 0x1037c258: each key's cap (key1topu) and the byte it types (0: none), the page after SHIFT ([5])
/// and after the pad's X ([6]).
struct Page {
    caps: [u16; 75],
    keys: [u8; 75],
    shift: u8,
    x: u8,
}

/// The English pages 6..=12, copied from FFXiMain 30261006_0 (pages 11 / 12 converted from their Shift-JIS by
/// FUN_101b4450's tables; the European 0x85 caps type nothing).
const PAGES: [Page; 7] = [
    // Page 6.
    Page {
        caps: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 21, 21, 21, 21, 21, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 21, 21, 21, 21, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 21, 21, 21, 21],
        keys: *b"1234567890     abcdefghijklmnopqrstuvwxyz    ABCDEFGHIJKLMNOPQRSTUVWXYZ    ",
        shift: 6,
        x: 6,
    },
    // Page 7.
    Page {
        caps: [21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 402, 403, 21, 21, 21, 21, 21, 21, 21, 21, 404, 405, 406, 407, 21, 408, 409, 400, 400, 319, 320, 21, 21, 323, 324, 36, 412, 21, 21, 21, 21, 21, 21, 21, 21, 21, 92, 93, 21, 104, 410, 399, 21, 21, 21, 108, 374, 401, 411, 426, 373, 64, 65, 106, 320, 398, 425, 21, 21, 21],
        keys: *b"               <>        []{} ()`'+-  =%/*         ,. :;\"   @&^#\\$!?_-~|   ",
        shift: 8,
        x: 8,
    },
    // Page 8.
    Page {
        caps: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 410, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 320, 64, 65, 21, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 426, 21],
        keys: *b"1234567890:;_/@abcdefghijklmnopqrstuvwxyz-!? ABCDEFGHIJKLMNOPQRSTUVWXYZ,.\\ ",
        shift: 7,
        x: 7,
    },
    // Page 9.
    Page {
        caps: [21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 21, 21, 21, 21, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 21, 21, 21, 21],
        keys: *b"               abcdefghijklmnopqrstuvwxyz    ABCDEFGHIJKLMNOPQRSTUVWXYZ    ",
        shift: 9,
        x: 9,
    },
    // Page 10.
    Page {
        caps: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21],
        keys: *b"1234567890                                                                 ",
        shift: 10,
        x: 10,
    },
    // Page 11.
    Page {
        caps: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 93, 92, 400, 399, 374, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 108, 411, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 408, 409, 346, 347, 348, 349, 350, 351, 352, 353, 354, 355, 356, 357, 358, 404, 405, 359, 360, 361, 362, 363, 364, 365, 366, 367, 369, 368, 306, 307, 64, 65],
        keys: *b"1234567890.,'\"&abcdefghijklm@#nopqrstuvwxyz()\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00[]\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00!?",
        shift: 12,
        x: 12,
    },
    // Page 12.
    Page {
        caps: [319, 320, 412, 36, 323, 373, 426, 324, 104, 410, 401, 398, 106, 425, 276, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 430, 431, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 406, 407, 375, 376, 377, 378, 379, 380, 381, 382, 383, 384, 385, 386, 387, 402, 403, 388, 389, 390, 391, 392, 393, 394, 395, 396, 397, 368, 432, 432, 370, 371],
        keys: *b"+-*/=$\\%:;^~_|\x00ABCDEFGHIJKLM\x00\x00NOPQRSTUVWXYZ{}\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00<>\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        shift: 11,
        x: 11,
    },
];

fn page(p: u8) -> &'static Page {
    &PAGES[(p.clamp(6, 12) - 6) as usize]
}

/// FUN_101aec30: the European caps, drawn dim (they type nothing).
fn dim(sprite: u16) -> bool {
    matches!(sprite, 0x15a..=0x173 | 0x177..=0x18d | 0x1ae..=0x1b0 | 0x114 | 0x132 | 0x133)
}

/// FUN_101af580 with the IME off: a field's character class (0 letters, 1 digits, 3 alphanumeric, 4 printable, 2 / 5
/// any) to its first page. Ours: every field but the chat line is taken as class 5 until their classes are mapped.
fn first_page(class: u8) -> u8 {
    match class {
        0 => 9,
        1 => 10,
        3 => 6,
        4 => 8,
        _ => 11,
    }
}

/// The open keyboard: its page (6..=12; reset at each open), the cursor child, and where it stands (fixed when it
/// first draws, FUN_101aedc0).
#[derive(Clone, Debug)]
pub struct Keyboard {
    page: u8,
    pub cursor: u16,
    at: std::cell::Cell<Option<(f32, f32)>>,
}

/// The keys right of the grid: Backspace, Tab, Config, SHIFT, SPACE, Enter.
const SPECIAL: [u16; 6] = [6, 7, 0x53, 0x55, 0x56, 0x57];

/// FUN_101b13f0, the pad's RB (key function 0x24): with config 0xc9 on, the keyboard over the open text field, or
/// over the chat line it opens when none is. Nothing with 0xc9 off, or while it is up.
pub fn rb(w: &mut World) {
    if w.game.get(0xc9) == 0 || w.swkbd.is_some() {
        return;
    }
    if !super::text_field_open(w) {
        w.typing = true;
    }
    w.swkbd = Some(Keyboard { page: first_page(5), cursor: 8, at: Default::default() });
}

fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::default() }
}

/// What a key types into the field, as the keyboard would type it (T vt+4 kinds 5 and 3): next frame's input.
fn send_key(w: &mut World, key: egui::Key) {
    w.swkbd_out.extend([key_event(key, true), key_event(key, false)]);
}

fn send_char(w: &mut World, b: u8) {
    w.swkbd_out.push(egui::Event::Text(char::from(b).to_string()));
}

/// Kind 8, close (FUN_101b5cd0): the chat line closes with it, its text kept; a window field only loses the keyboard.
fn close(w: &mut World) {
    if w.typing {
        w.typing = false;
    }
    w.swkbd = None;
}

/// FUN_101af860: A (or a click) on child `c`.
fn press(w: &mut World, c: u16) {
    let Some(k) = w.swkbd.as_mut() else { return };
    match c {
        6 => send_key(w, egui::Key::Backspace),
        7 => send_key(w, egui::Key::Tab),
        // FUN_101afc40, Config: in the field with no event, the field closes and configwi opens (SE 0002 again).
        0x53 => {
            if super::event::active(w) {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            w.sounds.push(Ui::Select as u32);
            w.typing = false;
            w.swkbd = None;
            let mut cfg = super::configui::ConfigUi::open("configwi");
            cfg.mon = super::monstrosity::active(w);
            cfg.cam_keys = w.cam_keys;
            cfg.anon = w.self_flags & 1 << 5 != 0;
            cfg.recall(w.cursor_recall(super::configui::CONFIGWI_MENU));
            w.config = Some(cfg);
        }
        // SHIFT: the page's [5]; one that names itself buzzes.
        0x55 => {
            let to = page(k.page).shift;
            if to == k.page {
                w.sounds.push(Ui::Error as u32);
            } else {
                k.page = to;
            }
        }
        0x56 => send_char(w, b' '),
        0x57 => send_key(w, egui::Key::Enter),
        8..=82 => {
            let b = page(k.page).keys[(c - 8) as usize];
            if b != 0 {
                send_char(w, b);
            }
        }
        _ => {}
    }
}

/// One frame of the open keyboard (FUN_101ab820's events, then the generic handler and FUN_101af190): the pad's
/// key functions in window context. It goes when its field does, and on any key of the keyboard (FUN_101b29f0; the
/// key still goes to the field).
pub fn update(w: &mut World, gu: Option<&GameUi>, input: &mut super::input::Input) {
    use super::input::mode::{PRESS, REPEAT};
    if w.swkbd.is_none() {
        return;
    }
    if !super::text_field_open(w) || input.key_pressed_any() {
        w.swkbd = None;
        return;
    }
    // The d-pad, one a frame (Up, Down, Left, Right): the layout's links [2] Up, [3] Down, [5] Left, [4] Right.
    if let Some(&(_, l)) = [(0x2b, 2), (0x2c, 3), (0x2e, 5), (0x2d, 4)].iter().find(|(f, _)| input.test(*f, REPEAT)) {
        let cursor = w.swkbd.as_ref().map_or(8, |k| k.cursor);
        let to = gu.and_then(|g| g.menu_links(NAME, cursor)).map_or(0xff, |links| links[l]);
        if to != 0xff && to as u16 != cursor {
            if let Some(k) = w.swkbd.as_mut() {
                k.cursor = to as u16;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
    }
    let cursor = w.swkbd.as_ref().map_or(8, |k| k.cursor);
    if input.test(0x93, PRESS) {
        w.sounds.push(Ui::Select as u32);
        if let Some(g) = gu {
            g.press_on(NAME, cursor);
        }
        press(w, cursor);
    }
    // The pad's other buttons (FUN_101af190, English).
    if input.test(0x94, PRESS) {
        w.sounds.push(Ui::Cancel as u32);
        send_key(w, egui::Key::Escape);
    }
    for (f, key) in [
        (0x86, egui::Key::ArrowLeft),
        (0x85, egui::Key::ArrowRight),
        (0x22, egui::Key::Enter),
        (0x14, egui::Key::Tab),
        (0x1f, egui::Key::Backspace),
    ] {
        if input.test(f, PRESS) {
            send_key(w, key);
        }
    }
    if input.test(0x1e, PRESS) {
        send_char(w, b' ');
    }
    // X: the page's [6], silently nothing where it names itself.
    if input.test(0x25, PRESS)
        && let Some(k) = w.swkbd.as_mut()
    {
        k.page = page(k.page).x;
    }
    if input.test(0x2f, PRESS) {
        close(w);
    }
    // Its window has the focus: the field's key functions don't see this frame's presses (FUN_101202a0).
    input.consume();
}

/// Ours: the keyboard drawn at a size that reads (retail's 334 x 88 is a sliver of a large screen): about a third of
/// the UI's width, in whole steps so its pixel art stays sharp.
fn scale(sw: f32) -> f32 {
    (sw * 0.35 / 334.0).round().max(1.0)
}

/// FUN_101aedc0's placement, as the window's top left (`kh` its height): below the macro editor, above the
/// comment editor, above the log window (its bottom 2 px over the log's top, or 34 px over its bottom while it has no
/// lines), above the chat line (32 more), else (32, 200).
fn place(gu: &GameUi, w: &World, kh: f32, sh: f32) -> (f32, f32) {
    let drawn = |name: &str| w.win_hits.borrow().iter().find(|h| h.0 == name).map(|h| h.1);
    let d = kh + 2.0;
    if !w.typing {
        if let Some(r) = drawn(super::macroedit::MACRO) {
            return (r[0], r[1] + r[3] + 2.0);
        }
        if let Some(r) = drawn(super::seacom::EDITOR) {
            return (r[0], r[1] - d);
        }
    }
    if let (Some(top), Some(bottom)) = (gu.log_top, gu.log_bottom) {
        let x = gu.window_xy("menu    logwindo", 0.0, 0.0).map_or(16.0, |p| p.0);
        return (x, (bottom - 32.0).min(top) - d);
    }
    if w.typing {
        return (16.0, sh - 38.0 - d - 32.0);
    }
    (32.0, 200.0)
}

/// The European caps' glyphs (they type nothing), by key, for the modern layer's labels.
fn glyph(page: u8, i: usize) -> Option<char> {
    let row = |s: &str, from: usize| s.chars().nth(i.checked_sub(from)?);
    match (page, i / 15) {
        (11, 3) => row("àáâäçèéêëìíîï", 45),
        (11, 4) => row("ñòóôöùúûüœß«»", 60),
        (12, 0) => row("°", 14),
        (12, 1) => row("€£", 28),
        (12, 3) => row("ÀÁÂÄÇÈÉÊËÌÍÎÏ", 45),
        (12, 4) => row("ÑÒÓÔÖÙÚÛÜŒß–—¡¿", 60),
        _ => None,
    }
}

/// Ours (the modern layer): key `i`'s label and whether it is inert (a European cap).
pub fn label(k: &Keyboard, i: usize) -> (String, bool) {
    match page(k.page).keys[i] {
        0 => (glyph(k.page, i).map(String::from).unwrap_or_default(), true),
        b' ' => (String::new(), false),
        b => (char::from(b).to_string(), false),
    }
}

/// Where it stands: fixed the first time it is asked (`place` gives it).
pub fn place_once(k: &Keyboard, place: impl FnOnce() -> (f32, f32)) -> (f32, f32) {
    match k.at.get() {
        Some(a) => a,
        None => {
            let a = place();
            k.at.set(Some(a));
            a
        }
    }
}

/// SHIFT's page is up (12 over 11, 7 over 8).
pub fn shifted(k: &Keyboard) -> bool {
    matches!(k.page, 12 | 7)
}

/// The pointer on child `c` (the modern layer): the cursor moves there (SE 0001).
pub fn hover(w: &mut World, c: u16) {
    if let Some(k) = w.swkbd.as_mut().filter(|k| k.cursor != c) {
        k.cursor = c;
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// A click on child `c` (the modern layer): A on it.
pub fn tap(w: &mut World, c: u16) {
    if let Some(k) = w.swkbd.as_mut() {
        k.cursor = c;
    }
    w.sounds.push(Ui::Select as u32);
    press(w, c);
}

/// FUN_101af440 over the generic draw: the special keys by the window, the page's 75 caps at their child + (1, 1)
/// (the cursor's tinted 0x80805020, the European ones dim), the yubi hand on the cursor. Returns the frame.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let k = w.swkbd.as_ref().filter(|_| !w.modern)?;
    let s = scale(sw);
    let kh = gu.menu_def(NAME)?.elements[0].h as f32 * s;
    let at = place_once(k, || place(gu, w, kh, sh));
    let hide: Vec<u16> = (1..=5).chain(8..=82).chain([0x54]).collect();
    let sel = SPECIAL.contains(&k.cursor).then_some(k.cursor);
    let win = gu.window_scaled(NAME, at, s, sel, &hide)?;
    let kids: Vec<(u16, [f32; 4])> = gu
        .menu_def(NAME)?
        .elements
        .iter()
        .skip(1)
        .filter(|e| !matches!(e.id, 1..=5 | 0x54))
        .map(|e| (e.id, [at.0 + e.x as f32 * s, at.1 + e.y as f32 * s, e.w as f32 * s, e.h as f32 * s]))
        .collect();
    let p = page(k.page);
    for (id, r) in kids.iter().filter(|(id, _)| (8..=82).contains(id)) {
        let cap = p.caps[(*id - 8) as usize];
        let tint = if *id == k.cursor {
            0x8080_5020
        } else if dim(cap) {
            0x4040_2810
        } else {
            0x8080_8080
        };
        gu.sprite_scaled(CAPS, cap as usize, r[0] + s, r[1] + s, s, s, tint);
    }
    // The hand at the cursor child + the window header's (14, 14) *(inf)*.
    if let Some((_, r)) = kids.iter().find(|(id, _)| *id == k.cursor) {
        gu.sprite_scaled(HAND, 0, r[0] + 14.0 * s, r[1] + 14.0 * s, s, s, 0x8080_8080);
    }
    super::winhits::record_rects(w, NAME, win.frame, kids);
    Some(win.frame)
}

/// The mouse on the keyboard (the generic window's): hovering a key moves the cursor, a click presses it; the right
/// click does nothing here.
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool) -> bool {
    let Some(cursor) = w.swkbd.as_ref().filter(|_| !w.modern).map(|k| k.cursor) else { return false };
    if !super::winhits::over(w, NAME, p) {
        return false;
    }
    super::winhits::generic(
        w,
        gu,
        NAME,
        cursor,
        6..=0x57,
        p,
        moved,
        clicked,
        false,
        |w, id| {
            if let Some(k) = w.swkbd.as_mut() {
                k.cursor = id;
            }
        },
        |w, gu, key| {
            if key == egui::Key::Enter {
                let c = w.swkbd.as_ref().map_or(8, |k| k.cursor);
                w.sounds.push(Ui::Select as u32);
                gu.press_on(NAME, c);
                press(w, c);
            }
        },
    );
    true
}

/// A zone change (FUN_101af500 message 8): the keyboard closes, and comes back with the chat line (message 4).
pub fn zone_out(w: &mut World) {
    w.swkbd_reopen = w.swkbd.take().is_some() && w.typing;
}

pub fn zone_in(w: &mut World) {
    if std::mem::take(&mut w.swkbd_reopen) && w.typing {
        w.swkbd = Some(Keyboard { page: first_page(5), cursor: 8, at: Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Page 11 is the chat line's: digits, lower case and its punctuation, the European caps dim and silent; SHIFT and
    /// X go to page 12 and back; the restricted pages keep themselves.
    #[test]
    fn pages() {
        let p = page(11);
        assert_eq!(&p.keys[..15], b"1234567890.,'\"&");
        assert_eq!(&p.keys[15..30], b"abcdefghijklm@#");
        assert_eq!((p.keys[45], p.keys[58], p.keys[73], p.keys[74]), (0, b'[', b'!', b'?'));
        assert!(dim(p.caps[45]) && !dim(p.caps[15]));
        assert_eq!((p.shift, p.x, page(12).shift), (12, 12, 11));
        assert_eq!(&page(12).keys[15..28], b"ABCDEFGHIJKLM");
        assert_eq!((page(6).shift, page(8).shift, page(7).x), (6, 7, 8));
        assert_eq!((first_page(5), first_page(1), first_page(3)), (11, 10, 6));
    }
}
