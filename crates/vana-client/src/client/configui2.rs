//! The Config pages with their own code: the scrolling lists (Chat Filters cfilter, Effects fxfilter, Log
//! Settings conf11s, key assignment k1assign / k2assign), Font Colors (conftxtc, textcol1..3), the Windows pages
//! (conf5m, conf5win, conf5w1 / conf5w2), Log (conf11m, conf11l) and the lobby's Config window (lobycwin).

use super::configui::{Act, ConfigUi, Ctl, Mark, Out, Win, draw_page};
use super::game_ui::GameUi;
use super::scrollbar::Msg;
use crate::audio::Ui as Se;
use crate::gamecfg::Values;
use Ctl::*;

const fn p(c: u16, id: u16, v: u32) -> (u16, Ctl) {
    (c, Pick(id, v, Mark::Wide))
}
const fn p3(c: u16, id: u16, v: u32) -> (u16, Ctl) {
    (c, Pick(id, v, Mark::Narrow))
}
const fn ic(c: u16, id: u16, v: u32) -> (u16, Ctl) {
    (c, Pick(id, v, Mark::Icon))
}

/// Windows: Shared, Window 1, Window 2 (FUN_101919c0).
const CONF5M: &[(u16, Ctl)] = &[(1, Open("conf5win")), (2, Open("conf5w1")), (3, Open("conf5w2"))];

/// Window Settings (Shared) (FUN_10191ae0): log window 2 0xb4, timestamps 0xb5, frame style 0x12, open effect 0xa4.
const CONF5WIN: &[(u16, Ctl)] = &[
    p3(1, 0xb4, 1),
    p3(2, 0xb4, 2),
    p3(3, 0xb4, 0),
    p3(4, 0xb5, 1),
    p3(5, 0xb5, 2),
    p3(6, 0xb5, 0),
    ic(7, 0x12, 0),
    ic(8, 0x12, 1),
    ic(9, 0x12, 2),
    ic(10, 0x12, 3),
    ic(11, 0x12, 4),
    ic(12, 0x12, 5),
    ic(13, 0x12, 6),
    ic(14, 0x12, 7),
    p(15, 0xa4, 0),
    p(16, 0xa4, 1),
];

/// Window 1 (FUN_10191fd0): reactive 0x0e, max 0x0f, min 0x10, width 0x9c (step 10), resize time 0x11.
const CONF5W1: &[(u16, Ctl)] = &[p(1, 0x0e, 0), p(2, 0x0e, 1), (3, Slider(0x0f, 4, 20, 1)), (4, Slider(0x10, 0, 20, 1)), (5, Slider(0x9c, 0, 100, 10)), (6, Slider(0x11, 3, 30, 1))];
/// Window 2: the same through the pairs at 0x1036ddd8.
const CONF5W2: &[(u16, Ctl)] = &[p(1, 0xb6, 0), p(2, 0xb6, 1), (3, Slider(0xb7, 4, 20, 1)), (4, Slider(0xb8, 0, 20, 1)), (5, Slider(0xc5, 0, 100, 10)), (6, Slider(0xc6, 3, 30, 1))];

/// Log: Window 1 / Window 2 categories, Default (confyn mode 1).
const CONF11M: &[(u16, Ctl)] = &[(1, Act(Act::Custom(1))), (2, Act(Act::Custom(2))), (3, Act(Act::AskReset(1)))];
/// Chat, For Self, For Others, System.
const CONF11L: &[(u16, Ctl)] = &[(1, Act(Act::Custom(1))), (2, Act(Act::Custom(2))), (3, Act(Act::Custom(3))), (4, Act(Act::Custom(4)))];
/// Font Colors: four categories and Default (confyn mode 0).
const CONFTXTC: &[(u16, Ctl)] = &[(1, Act(Act::Custom(1))), (2, Act(Act::Custom(2))), (3, Act(Act::Custom(3))), (4, Act(Act::Custom(4))), (5, Act(Act::AskReset(0)))];
/// textcol3: R, G, B sliders (step 4), OK, Cancel.
const TEXTCOL3: &[(u16, Ctl)] = &[(1, Act(Act::Custom(1))), (2, Act(Act::Custom(2))), (3, Act(Act::Custom(3))), (4, Act(Act::Custom(4))), (5, Act(Act::Custom(5)))];
/// The shared yes/no box (FUN_102075f0) of conf6win's Default.
const YESNO: &[(u16, Ctl)] = &[(1, Act(Act::Custom(1))), (2, Act(Act::Custom(2)))];
/// The lobby's Config window (FUN_10196310): volume 0xa8, logout destination 0xac, aspect 0xab, title music 0xaf.
const LOBYCWIN: &[(u16, Ctl)] = &[
    (1, Slider(0xa8, 0, 100, 1)),
    (2, Pick(0xac, 0, Mark::Wide)),
    (3, Pick(0xac, 1, Mark::Wide)),
    (4, Slider(0xab, 0, 5, 1)),
    (5, Pick(0xaf, 0, Mark::Icon)),
    (7, Pick(0xaf, 2, Mark::Icon)),
    (8, Pick(0xaf, 3, Mark::Icon)),
    (9, Pick(0xaf, 4, Mark::Icon)),
    (10, Pick(0xaf, 5, Mark::Icon)),
    (11, Pick(0xaf, 6, Mark::Icon)),
    (12, Pick(0xaf, 7, Mark::Icon)),
    (6, Pick(0xaf, 1, Mark::Icon)),
];

/// List pages: rows 1..14 are scrolled; Enter acts on scroll + row - 1.
const LIST_ROWS: &[(u16, Ctl)] = &[(1, Act(Act::Custom(0))), (2, Act(Act::Custom(0))), (3, Act(Act::Custom(0))), (4, Act(Act::Custom(0))), (5, Act(Act::Custom(0))), (6, Act(Act::Custom(0))), (7, Act(Act::Custom(0))), (8, Act(Act::Custom(0))), (9, Act(Act::Custom(0))), (10, Act(Act::Custom(0))), (11, Act(Act::Custom(0))), (12, Act(Act::Custom(0))), (13, Act(Act::Custom(0))), (14, Act(Act::Custom(0)))];
/// textcol1: up to 13 category rows.
const TEXTCOL1: &[(u16, Ctl)] = &[(1, Act(Act::Custom(0))), (2, Act(Act::Custom(0))), (3, Act(Act::Custom(0))), (4, Act(Act::Custom(0))), (5, Act(Act::Custom(0))), (6, Act(Act::Custom(0))), (7, Act(Act::Custom(0))), (8, Act(Act::Custom(0))), (9, Act(Act::Custom(0))), (10, Act(Act::Custom(0))), (11, Act(Act::Custom(0))), (12, Act(Act::Custom(0))), (13, Act(Act::Custom(0)))];

pub fn page(name: &str) -> &'static [(u16, Ctl)] {
    match name {
        "conf5m" => CONF5M,
        "conf5win" => CONF5WIN,
        "conf5w1" => CONF5W1,
        "conf5w2" => CONF5W2,
        "conf11m" => CONF11M,
        "conf11l" => CONF11L,
        "conftxtc" => CONFTXTC,
        "textcol1" => TEXTCOL1,
        "textcol3" => TEXTCOL3,
        "yesno" => YESNO,
        "lobycwin" => LOBYCWIN,
        n if is_list(n) => LIST_ROWS,
        _ => &[],
    }
}

/// The list pages' bar or wheel (FUN_10197be0 and the others alike): a row moves the top and the hand against each
/// other so the selected row stays, a page is Left / Right's, the thumb sets the top and shifts the hand by as much,
/// clamped to 1..visible (silent). True when it moved with SE 0001.
pub(super) fn bar(ui: &mut ConfigUi, v: &Values, m: Msg) -> bool {
    let Some(name) = ui.stack.last().map(|w| w.name.clone()) else { return false };
    if !((is_list(&name) && name != "conf11s") || name == "textcol1") {
        return false;
    }
    let page = if name == "textcol1" { 11 } else { 14 };
    let n = rows(ui, v, &name);
    let (shown, max) = (n.min(page).max(1), n.saturating_sub(page));
    let top = ui.extra.scroll;
    let hand = ui.stack.last().map_or(1, |w| w.cursor as isize);
    let (t, h) = match m {
        Msg::Up => (top.saturating_sub(1), hand + (top > 0) as isize),
        Msg::Down => ((top + 1).min(max), hand - (top < max) as isize),
        Msg::PageUp => (top.saturating_sub(page), hand),
        Msg::PageDown => ((top + page).min(max), hand),
        Msg::Set(t) => {
            let t = t.min(max);
            (t, hand - (t as isize - top as isize))
        }
    };
    let h = h.clamp(1, shown as isize) as u16;
    ui.extra.scroll = t;
    let was = ui.stack.last().map_or(1, |w| w.cursor);
    if let Some(w) = ui.stack.last_mut() {
        w.cursor = h;
    }
    (t, h) != (top, was) && !matches!(m, Msg::Set(_))
}

fn is_list(name: &str) -> bool {
    matches!(name, "cfilter" | "fxfilter" | "conf11s" | "k1assign" | "k2assign")
}

/// cfilter rows (0x10379a28; FUN_10197f50 counts 0x37): (a, b, label in group 0xb). a = mask bit (< 30: 0x32, else
/// 0x33 bit a - 30) or NO_MASK; b = hold bit of 0x34, NO_HOLD = none, < 0 = system level of 0x35. Rows 5 / 6 are the
/// Assist J / E channels *(inferred from their labels)*.
pub const CFILTER: [(i8, i8, u16); 55] = [
    (0, 0, 0), (51, 2, 2), (51, 3, 3), (51, 4, 4), (51, 8, 5), (49, 9, 6), (50, 10, 7), (51, 7, 8), (3, 5, 9), (1, 1, 1),
    (46, 11, 195), (4, 11, 11), (5, 11, 12), (6, 11, 13), (7, 11, 14), (8, 11, 15), (9, 11, 16), (23, 11, 30), (13, 11, 20),
    (14, 11, 21), (15, 11, 22), (16, 11, 23), (17, 11, 24), (24, 11, 31), (18, 11, 25), (19, 11, 26), (20, 11, 27), (21, 11, 28),
    (22, 11, 29), (39, 11, 177), (38, 11, 178), (34, 11, 179), (35, 11, 180), (36, 11, 181), (37, 11, 182), (33, 11, 171),
    (32, 11, 172), (28, 11, 173), (29, 11, 174), (30, 11, 175), (31, 11, 176), (25, 11, 32), (10, 11, 17), (11, 11, 18),
    (12, 11, 19), (26, 11, 88), (27, 11, 144), (40, 11, 191), (41, 11, 192), (42, 11, 193), (45, 11, 194), (47, 6, 10),
    (51, -1, 33), (51, -2, 34), (51, -3, 35),
];
/// cfilter: '3', no mask bit; no hold bit (FUN_10198080 / FUN_10198240).
const NO_MASK: i8 = 51;
const NO_HOLD: i8 = 11;
/// conf11s categories (0x10379ef8 / f68 / fd8 / 0x1037a048): (label, bit of the window's 64-bit mask).
const LOG_ROWS: [&[(u16, u8)]; 4] = [
    &[(36, 0), (38, 1), (39, 2), (40, 3), (41, 4), (42, 12), (43, 13), (44, 6), (45, 7), (46, 8), (47, 9), (37, 10), (196, 11)],
    &[(48, 33), (49, 34), (50, 35), (51, 36), (52, 37), (53, 38)],
    &[(54, 39), (55, 40), (56, 41), (57, 42), (58, 43), (59, 44)],
    &[(202, 45), (203, 46), (62, 47)],
];
/// Font Colors categories (0x10379b50 / c30 / d10 / df0): (label, sample, config id), all group 0xb.
const COLOUR_ROWS: [&[(u16, u16, u16)]; 4] = [
    &[(0x24, 0x3f, 0x1b), (0x26, 0x41, 0x1d), (0x27, 0x42, 0x1e), (0x28, 0x43, 0x1f), (0x29, 0x44, 0xb3), (0x2a, 0xcc, 0xcb), (0x2b, 0xcd, 0xcc), (0x2c, 0x45, 0xb0), (0x2d, 0x46, 0x20), (0x2e, 0x47, 0x21), (0x2f, 0x48, 0x22), (0x25, 0x40, 0x1c), (0xc4, 0xc5, 0x9b)],
    &[(0x30, 0x49, 0x23), (0x31, 0x4a, 0x24), (0x32, 0x4b, 0x25), (0x33, 0x4c, 0x26), (0x34, 0x4d, 0x27), (0x35, 0x4e, 0x28)],
    &[(0x36, 0x4f, 0x29), (0x37, 0x50, 0x2a), (0x38, 0x51, 0x2b), (0x39, 0x52, 0x2c), (0x3a, 0x53, 0x2d), (0x3b, 0x54, 0x2e)],
    &[(0x3c, 0x55, 0x2f), (0x3d, 0x56, 0x30), (0x3e, 0x57, 0x31)],
];
/// k1assign labels (0x1037a128; rows 28 / 29 Turn Right / Left only with keyboard type 2) and k2assign (0x1037a218;
/// row 28 is key function 0xb4, config 0x86).
const K1_LABELS: [u16; 30] = [89, 90, 92, 91, 101, 102, 104, 103, 93, 94, 96, 95, 137, 136, 97, 98, 100, 99, 105, 106, 107, 108, 109, 110, 111, 138, 142, 141, 198, 199];
const K2_LABELS: [u16; 29] = [112, 113, 114, 115, 116, 133, 134, 135, 140, 139, 142, 141, 138, 137, 136, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 130, 129, 127, 128];

/// DIK scan codes: the character a key shows (FUN_101a7c20) and the egui key that makes it.
const DIK: &[(u8, &str, egui::Key)] = {
    use egui::Key::*;
    &[
        (0x02, "1", Num1), (0x03, "2", Num2), (0x04, "3", Num3), (0x05, "4", Num4), (0x06, "5", Num5), (0x07, "6", Num6), (0x08, "7", Num7), (0x09, "8", Num8),
        (0x0a, "9", Num9), (0x0b, "0", Num0), (0x0c, "-", Minus), (0x0d, "=", Equals), (0x10, "Q", Q), (0x11, "W", W), (0x12, "E", E), (0x13, "R", R),
        (0x14, "T", T), (0x15, "Y", Y), (0x16, "U", U), (0x17, "I", I), (0x18, "O", O), (0x19, "P", P), (0x1a, "[", OpenBracket), (0x1b, "]", CloseBracket),
        (0x1e, "A", A), (0x1f, "S", S), (0x20, "D", D), (0x21, "F", F), (0x22, "G", G), (0x23, "H", H), (0x24, "J", J), (0x25, "K", K), (0x26, "L", L),
        (0x27, ";", Semicolon), (0x28, "'", Quote), (0x29, "`", Backtick), (0x2b, "\\", Backslash), (0x2c, "Z", Z), (0x2d, "X", X), (0x2e, "C", C),
        (0x2f, "V", V), (0x30, "B", B), (0x31, "N", N), (0x32, "M", M), (0x33, ",", Comma), (0x34, ".", Period), (0x35, "/", Slash),
    ]
};

pub fn dik_char(code: u32) -> &'static str {
    DIK.iter().find(|d| d.0 as u32 == code).map_or("", |d| d.1)
}

pub fn dik_of(key: egui::Key) -> Option<u8> {
    DIK.iter().find(|d| d.2 == key).map(|d| d.0)
}

/// Page state that is not a config id.
#[derive(Default)]
pub struct Extra {
    /// List pages: first row shown.
    pub scroll: usize,
    /// conf11s / conftxtc category (0..3) and conf11s window (0 / 1).
    pub cat: usize,
    pub log_window: usize,
    /// textcol1 row and textcol3's working colour and slider in edit.
    pub row: usize,
    pub rgb: [u8; 3],
    pub rgb_edit: Option<u16>,
    /// conf6win Default asked (the yes/no box is open).
    pub ask_keys: Option<bool>,
}

fn rows(ui: &ConfigUi, v: &Values, name: &str) -> usize {
    match name {
        "cfilter" => CFILTER.len(),
        "fxfilter" => 18,
        "conf11s" => LOG_ROWS[ui.extra.cat.min(3)].len(),
        "k1assign" => {
            if v.get(0x42) == 2 { 30 } else { 28 }
        }
        "k2assign" => 29,
        "textcol1" => COLOUR_ROWS[ui.extra.cat.min(3)].len(),
        _ => 0,
    }
}

/// A window was pushed.
pub fn opened(ui: &mut ConfigUi, name: &str) {
    if is_list(name) || name == "textcol1" {
        ui.extra.scroll = 0;
    }
    if name == "lobycwin"
        && let Some(w) = ui.stack.last_mut()
    {
        w.cursor = 5;
    }
}

/// A window was closed: textcol1 takes textcol2 with it.
pub fn closed(ui: &mut ConfigUi, name: &str, _out: &mut Out) {
    if name == "textcol1" && ui.stack.last().is_some_and(|w| w.name == "textcol2") {
        ui.stack.pop();
    }
    if name == "yesno" {
        ui.extra.ask_keys = None;
    }
}

fn mask_bit(v: &Values, bit: u32) -> bool {
    let (id, b) = if bit < 30 { (0x32, bit) } else { (0x33, bit - 30) };
    v.get(id) & (1 << b) != 0
}

fn set_mask(v: &mut Values, out: &mut Out, bit: u32, on: bool) {
    let (id, b) = if bit < 30 { (0x32, bit) } else { (0x33, bit - 30) };
    set_bit(v, out, id, 1 << b, on);
}

fn set_bit(v: &mut Values, out: &mut Out, id: u16, mask: u32, on: bool) {
    let cur = v.get(id);
    if v.set(id, if on { cur | mask } else { cur & !mask }) {
        out.changed.push(id);
    }
}

/// cfilter row state sprite (framesus: 89 OFF, 88 ON, 83 Hold, 136 OFF + icon, 137 Hold + icon) (FUN_10190cb0).
fn cfilter_state(v: &Values, r: usize) -> usize {
    let (a, b, _) = CFILTER[r];
    if b < 0 {
        return if v.get_i(0x35) >= -b as i32 { 88 } else { 89 };
    }
    let hold = b != NO_HOLD && v.get(0x34) & (1 << b) != 0;
    let icon = b == 2 && v.get(0x47) != 0;
    if a != NO_MASK && mask_bit(v, a as u32) {
        88
    } else if hold {
        if icon { 137 } else { 83 }
    } else if icon {
        136
    } else {
        89
    }
}

/// FUN_10190af0: Enter on a cfilter row.
fn cfilter_toggle(v: &mut Values, out: &mut Out, r: usize) {
    let (a, b, _) = CFILTER[r];
    if b < 0 {
        let lv = (r as i32 - NO_MASK as i32).clamp(1, 3);
        let nv = if v.get_i(0x35) < lv { lv } else { lv - 1 };
        if v.set(0x35, nv as u32) {
            out.changed.push(0x35);
        }
        return;
    }
    let hold = b != NO_HOLD && v.get(0x34) & (1 << b) != 0;
    if a == NO_MASK {
        // OFF <-> Hold; Tell: OFF -> Hold -> OFF + icon -> Hold + icon -> OFF.
        if b == 2 {
            let icon = v.get(0x47) != 0;
            if hold {
                set_bit(v, out, 0x34, 1 << b, false);
                if v.set(0x47, (!icon) as u32) {
                    out.changed.push(0x47);
                }
            } else {
                set_bit(v, out, 0x34, 1 << b, true);
            }
        } else {
            set_bit(v, out, 0x34, 1 << b, !hold);
        }
        return;
    }
    let on = mask_bit(v, a as u32);
    if b == NO_HOLD {
        set_mask(v, out, a as u32, !on);
    } else if on {
        // ON -> Hold.
        set_mask(v, out, a as u32, false);
        set_bit(v, out, 0x34, 1 << b, true);
    } else if hold {
        set_bit(v, out, 0x34, 1 << b, false);
    } else {
        set_mask(v, out, a as u32, true);
    }
}

fn log_ids(window: usize) -> (u16, u16) {
    if window == 0 { (0xb9, 0xba) } else { (0xbb, 0xbc) }
}

fn log_bit(v: &Values, window: usize, bit: u8) -> bool {
    let (lo, hi) = log_ids(window);
    if bit < 32 { v.get(lo) & (1 << bit) != 0 } else { v.get(hi) & (1 << (bit - 32)) != 0 }
}

/// A list page's row: (label id in group 0xb, state sprite or None for key rows).
fn row_toggle(ui: &mut ConfigUi, v: &mut Values, name: &str, r: usize, out: &mut Out) {
    match name {
        "cfilter" => cfilter_toggle(v, out, r),
        "fxfilter" => {
            let on = v.get(0x92) & (1 << r) != 0;
            set_bit(v, out, 0x92, 1 << r, !on);
        }
        "conf11s" => {
            let (lo, hi) = log_ids(ui.extra.log_window);
            let bit = LOG_ROWS[ui.extra.cat.min(3)][r].1;
            let on = log_bit(v, ui.extra.log_window, bit);
            if bit < 32 { set_bit(v, out, lo, 1 << bit, !on) } else { set_bit(v, out, hi, 1 << (bit - 32), !on) }
        }
        _ => {}
    }
}

/// Key assignment (FUN_10194e10): a key assigns to the cursor row; the same key again unassigns; a key is held
/// by one row only. k2assign refuses the digits. Returns false for keys that are not assignable.
fn assign(v: &mut Values, out: &mut Out, name: &str, row: usize, key: egui::Key) -> bool {
    let Some(code) = dik_of(key) else { return false };
    let (base, n) = if name == "k1assign" { (0x4a, 30) } else { (0x6a, 29) };
    if name == "k2assign" && (0x02..=0x0b).contains(&code) {
        out.sounds.push(Se::Error);
        return true;
    }
    let id = base + row as u16;
    let unassign = v.get(id) == code as u32;
    for k in 0..n {
        let other = base + k;
        if v.get(other) == code as u32 && v.set(other, 0) {
            out.changed.push(other);
        }
    }
    if !unassign && v.set(id, code as u32) {
        out.changed.push(id);
    }
    out.sounds.push(Se::Select);
    true
}

/// Keys for windows with their own handling; true when taken.
pub fn key(ui: &mut ConfigUi, gu: &GameUi, v: &mut Values, key: egui::Key, out: &mut Out) -> bool {
    let Some(w) = ui.stack.last() else { return false };
    let name = w.name.clone();
    let cursor = w.cursor;
    if name == "textcol3"
        && let Some(ch) = ui.extra.rgb_edit
    {
        let c = &mut ui.extra.rgb[(ch as usize - 1).min(2)];
        match key {
            // FUN_10191740: +-4, capped at 0 / 255.
            egui::Key::ArrowRight => *c = c.saturating_add(4),
            egui::Key::ArrowLeft => *c = c.saturating_sub(4),
            egui::Key::Enter | egui::Key::Escape => {
                ui.extra.rgb_edit = None;
                out.sounds.push(if key == egui::Key::Enter { Se::Select } else { Se::Cancel });
            }
            _ => {}
        }
        return true;
    }
    if is_list(&name) || name == "textcol1" {
        let n = rows(ui, v, &name);
        // textcol1 shows 11 rows, the lists 14 (FUN_10198620 / FUN_10197b80); all scroll and page alike.
        let page = if name == "textcol1" { 11 } else { 14 };
        let shown = n.min(page);
        let at = ui.extra.scroll + cursor as usize - 1;
        let mut moved = false;
        let set_cursor = |ui: &mut ConfigUi, c: u16| {
            if let Some(w) = ui.stack.last_mut() {
                w.cursor = c;
            }
        };
        match key {
            egui::Key::ArrowDown => {
                if (cursor as usize) < shown {
                    set_cursor(ui, cursor + 1);
                    moved = true;
                } else if ui.extra.scroll + shown < n {
                    ui.extra.scroll += 1;
                    moved = true;
                }
            }
            egui::Key::ArrowUp => {
                if cursor > 1 {
                    set_cursor(ui, cursor - 1);
                    moved = true;
                } else if ui.extra.scroll > 0 {
                    ui.extra.scroll -= 1;
                    moved = true;
                }
            }
            // Right / Left page by 14 (conf11s: switch window).
            egui::Key::ArrowRight | egui::Key::ArrowLeft if name == "conf11s" => {
                ui.extra.log_window ^= 1;
                moved = true;
            }
            egui::Key::ArrowRight => {
                let s = (ui.extra.scroll + page).min(n.saturating_sub(page));
                moved = s != ui.extra.scroll;
                ui.extra.scroll = s;
            }
            egui::Key::ArrowLeft => {
                let s = ui.extra.scroll.saturating_sub(page);
                moved = s != ui.extra.scroll;
                ui.extra.scroll = s;
            }
            egui::Key::Enter if name == "textcol1" => {
                out.sounds.push(Se::Select);
                ui.extra.row = at;
                let id = COLOUR_ROWS[ui.extra.cat.min(3)].get(ui.extra.row).map_or(0x1b, |r| r.2);
                let c = v.get(id);
                ui.extra.rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                ui.extra.rgb_edit = None;
                ui.push("textcol3");
            }
            egui::Key::Enter if name.ends_with("assign") => out.sounds.push(Se::Error),
            egui::Key::Enter if at < n => {
                out.sounds.push(Se::Select);
                row_toggle(ui, v, &name, at, out);
            }
            egui::Key::Escape => return false,
            k if name.ends_with("assign") && at < n => {
                assign(v, out, &name, at, k);
            }
            _ => {}
        }
        if moved {
            out.sounds.push(Se::Cursor);
        }
        let _ = gu;
        return true;
    }
    false
}

/// Enter on an Act::Custom child.
pub fn custom(ui: &mut ConfigUi, v: &mut Values, name: &str, k: u8, out: &mut Out) {
    match (name, k) {
        ("conftxtc", 1..=4) => {
            ui.extra.cat = k as usize - 1;
            ui.push("textcol2");
            ui.push("textcol1");
        }
        ("conf11m", 1 | 2) => {
            ui.extra.log_window = k as usize - 1;
            ui.push("conf11l");
        }
        ("conf11l", 1..=4) => {
            ui.extra.cat = k as usize - 1;
            ui.push("conf11s");
        }
        ("textcol3", 1..=3) => ui.extra.rgb_edit = Some(k as u16),
        ("textcol3", 4) => {
            // OK: 0x80RRGGBB into the row's id (FUN_10191810), then close.
            let id = COLOUR_ROWS[ui.extra.cat.min(3)].get(ui.extra.row).map_or(0x1b, |r| r.2);
            let [r, g, b] = ui.extra.rgb;
            if v.set(id, 0x8000_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32) {
                out.changed.push(id);
            }
            ui.stack.pop();
        }
        ("textcol3", 5) => {
            ui.stack.pop();
        }
        ("yesno", 1) => {
            // FUN_10195460 / FUN_101957b0: every key back to its default.
            let ids: Vec<u16> = (0x4a..=0x89).collect();
            for id in ids {
                let d = crate::gamecfg::def(id).map_or(0, |d| d.default);
                if v.set(id, d) {
                    out.changed.push(id);
                }
            }
            ui.stack.pop();
            ui.extra.ask_keys = None;
        }
        ("yesno", 2) => {
            ui.stack.pop();
            ui.extra.ask_keys = None;
        }
        _ => {}
    }
}

/// After a set: max / min lines pull each other (FUN_101920c0 *(inf: same rule for window 2)*); window 2's -1
/// values follow window 1.
pub fn linked(v: &mut Values, id: u16, out: &mut Out) {
    let pair = match id {
        0x0f | 0x10 => (0x0f, 0x10),
        0xb7 | 0xb8 => (0xb7, 0xb8),
        _ => return,
    };
    let (max, min) = (v.get_i(pair.0), v.get_i(pair.1));
    if min > max {
        let (fix, to) = if id == pair.0 { (pair.1, max) } else { (pair.0, min) };
        if v.set(fix, to as u32) {
            out.changed.push(fix);
        }
    }
}

/// The pointer on windows with their own handling; true when taken.
#[allow(clippy::too_many_arguments)]
pub fn pointer(ui: &mut ConfigUi, gu: &GameUi, v: &mut Values, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32, out: &mut Out) -> bool {
    let Some(w) = ui.stack.last() else { return false };
    let name = w.name.clone();
    if !(is_list(&name) || name == "textcol1") {
        return false;
    }
    let Some((x, y)) = gu.window_xy(&w.full(), sw, sh) else { return true };
    let n = rows(ui, v, &name);
    let kids: Vec<(u16, [f32; 4])> = gu.menu_def(&w.full()).map(|m| m.elements.iter().skip(1).filter(|c| (1..=14).contains(&c.id)).map(|c| (c.id, [x + c.x as f32, y + c.y as f32, c.w as f32, c.h as f32])).collect()).unwrap_or_default();
    let Some((id, _)) = kids.iter().find(|(id, r)| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3] && ui.extra.scroll + *id as usize - 1 < n) else { return true };
    let id = *id;
    gu.hover.set(true);
    if (moved || clicked) && w.cursor != id {
        out.sounds.push(Se::Cursor);
        if let Some(w) = ui.stack.last_mut() {
            w.cursor = id;
        }
    }
    if clicked {
        key(ui, gu, v, egui::Key::Enter, out);
    }
    true
}

/// Draws windows with their own drawing; true when drawn.
#[allow(clippy::too_many_arguments)]
pub fn draw(ui: &ConfigUi, w: &Win, top: bool, gu: &mut GameUi, v: &Values, sw: f32, sh: f32, rects: &mut Vec<[f32; 4]>) -> bool {
    let name = w.full();
    let white = [0x80u8; 4];
    match w.name.as_str() {
        n if is_list(n) => {
            draw_page(ui, w, top, gu, v, sw, sh);
            let Some((x, y)) = gu.window_xy(&name, sw, sh) else { return true };
            // conf11s: the "Log Settings (Window 1 / 2)" header bar at the window position.
            if n == "conf11s" {
                gu.sprite("menu    windowps", 745 + ui.extra.log_window, x, y, 0x8080_8080);
            }
            let total = rows(ui, v, n);
            let kids: Vec<(u16, f32, f32)> = gu.menu_def(&name).map(|m| m.elements.iter().skip(1).filter(|c| (1..=14).contains(&c.id)).map(|c| (c.id, x + c.x as f32, y + c.y as f32)).collect()).unwrap_or_default();
            for (id, cx, cy) in kids {
                let r = ui.extra.scroll + id as usize - 1;
                if r >= total {
                    continue;
                }
                match n {
                    "cfilter" => {
                        gu.sprite("menu    framesus", cfilter_state(v, r), cx, cy, 0x8080_8080);
                        text_b(gu, CFILTER[r].2, cx + 33.0, cy, white);
                    }
                    "fxfilter" => {
                        gu.sprite("menu    framesus", if v.get(0x92) & (1 << r) != 0 { 88 } else { 89 }, cx, cy, 0x8080_8080);
                        text_b(gu, 153 + r as u16, cx + 33.0, cy, white);
                    }
                    "conf11s" => {
                        let (label, bit) = LOG_ROWS[ui.extra.cat.min(3)][r];
                        gu.sprite("menu    framesus", if log_bit(v, ui.extra.log_window, bit) { 88 } else { 89 }, cx, cy, 0x8080_8080);
                        text_b(gu, label, cx + 30.0, cy, white);
                    }
                    _ => {
                        // FUN_10194bd0: "[  ]" at x + 4, the key centred on x + 0x13, the label at x + 0x40.
                        let (label, id) = if n == "k1assign" { (K1_LABELS[r], 0x4a + r as u16) } else { (K2_LABELS[r], 0x6a + r as u16) };
                        gu.text("[  ]", cx + 4.0, cy, 0, white);
                        gu.text(dik_char(v.get(id)), cx + 19.0, cy, 1, white);
                        text_b(gu, label, cx + 64.0, cy, white);
                    }
                }
            }
            // FUN_10197b80: 14 rows at x1 - 8 (conf11s has none).
            if n != "conf11s"
                && let Some([fx, fy, fw, fh]) = gu.window_rect(&name, sw, sh)
            {
                gu.scroll_bar(fx + fw - 8.0, fy, fh, total, 14, ui.extra.scroll);
                if top {
                    gu.bar_site(super::scrollbar::Owner::Config, fx + fw - 8.0, fy, fh, total, 14, ui.extra.scroll);
                }
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        "textcol1" => {
            let rows_of = COLOUR_ROWS[ui.extra.cat.min(3)];
            let Some((x, y)) = gu.window(&name, sw, sh) else { return true };
            let kids: Vec<(u16, f32, f32)> = gu.menu_def(&name).map(|m| m.elements.iter().skip(1).map(|c| (c.id, x + c.x as f32, y + c.y as f32)).collect()).unwrap_or_default();
            for (id, cx, cy) in kids {
                let Some(row) = rows_of.get(ui.extra.scroll + id as usize - 1) else { continue };
                let on = top && id == w.cursor;
                gu.sprite("menu    windowps", 74, cx, cy, if on { 0x8080_5020 } else { 0x8080_8080 });
                text_b(gu, row.0, cx + 30.0, cy, white);
                if on {
                    gu.cursor(cx, cy);
                }
            }
            // FUN_10198620: 11 rows, total the category's colours.
            if let Some([fx, fy, fw, fh]) = gu.window_rect(&name, sw, sh) {
                gu.scroll_bar(fx + fw - 8.0, fy, fh, rows_of.len(), 11, ui.extra.scroll);
                if top {
                    gu.bar_site(super::scrollbar::Owner::Config, fx + fw - 8.0, fy, fh, rows_of.len(), 11, ui.extra.scroll);
                }
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        "textcol2" => {
            // FUN_101913b0: the row's sample in its colour at (x + 16, y + 20).
            let Some((x, y)) = gu.window(&name, sw, sh) else { return true };
            let row = top.then_some(()).map_or(ui.stack.iter().any(|s| s.name == "textcol1"), |_| true);
            let cursor = ui.stack.iter().find(|s| s.name == "textcol1").map_or(1, |s| s.cursor) as usize;
            if let (true, Some(r)) = (row, COLOUR_ROWS[ui.extra.cat.min(3)].get(ui.extra.scroll + cursor - 1)) {
                text_b(gu, r.1, x + 16.0, y + 20.0, crate::dat::battle::argb_bytes(v.get(r.2)));
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        "textcol3" => {
            draw_page(ui, w, top && ui.extra.rgb_edit.is_none(), gu, v, sw, sh);
            let Some((x, y)) = gu.window_xy(&name, sw, sh) else { return true };
            let [r, g, b] = ui.extra.rgb;
            let colour = 0x8000_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32;
            // FUN_10189620 with width 0x96: the sample is cut at 150 px.
            if let Some(row) = COLOUR_ROWS[ui.extra.cat.min(3)].get(ui.extra.row) {
                let mut t = gu.group_b(row.1 as usize).unwrap_or_default();
                while gu.text_width(&t) > 150.0 && !t.is_empty() {
                    t.pop();
                }
                gu.text(&t, x + 30.0, y + 20.0, 0, crate::dat::battle::argb_bytes(colour));
            }
            // FUN_10191580: windowps 98 at (child + 13, child + 2), x-scale v / 255 * 1.3.
            let tints = [0x7080_0000, 0x7000_8000, 0x7000_0080];
            let kids: Vec<(u16, f32, f32)> = gu.menu_def(&name).map(|m| m.elements.iter().skip(1).filter(|c| c.id <= 3).map(|c| (c.id, x + c.x as f32, y + c.y as f32)).collect()).unwrap_or_default();
            for (id, cx, cy) in kids {
                let c = ui.extra.rgb[id as usize - 1];
                if c > 0 {
                    gu.sprite_scaled("menu    windowps", 98, cx + 13.0, cy + 2.0, c as f32 / 255.0 * 1.3, 1.0, tints[id as usize - 1]);
                }
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        "conf5w1" | "conf5w2" => {
            draw_page(ui, w, top, gu, v, sw, sh);
            // FUN_10191f20: the max / min line counts ("%d", fontshp, right-aligned) at (x1 - 16, y0 - 14).
            let Some((x, y)) = gu.window_xy(&name, sw, sh) else { return true };
            let ids = if w.name == "conf5w1" { [0x0f, 0x10] } else { [0xb7, 0xb8] };
            let kids: Vec<(u16, f32, f32, f32)> = gu.menu_def(&name).map(|m| m.elements.iter().skip(1).filter(|c| c.id == 3 || c.id == 4).map(|c| (c.id, x + c.x as f32, y + c.y as f32, c.w as f32)).collect()).unwrap_or_default();
            for (id, cx, cy, cw) in kids {
                let editing = w.edit.filter(|e| e.0 == id).map(|e| e.1);
                let val = editing.unwrap_or(v.get_i(ids[id as usize - 3]));
                gu.shape_text(&val.to_string(), cx + cw - 16.0, cy - 14.0, 0x8080_8080, true);
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        "yesno" => {
            draw_page(ui, w, top, gu, v, sw, sh);
            // FUN_102075f0's text above the box *(inf: placement)*.
            if let Some((x, y)) = gu.window_xy(&name, sw, sh) {
                let t = gu.group_b(0x8f).unwrap_or_else(|| "Reset keyboard assignment to default?".into());
                let tw = gu.text_width(&t);
                gu.frame(x, y - 28.0, tw + 16.0, 24.0);
                gu.text(&t, x + 8.0, y - 24.0, 0, white);
            }
            rects.extend(gu.window_rect(&name, sw, sh));
            true
        }
        _ => false,
    }
}

/// The modern Config's list pages (cfilter, fxfilter, conf11s in category `cat` for the first log window): each row's
/// label (group 0xb) and state sprite (framesus 88 ON, 89 OFF, 83 Hold, 136 / 137 with the tell icon).
pub(super) fn modern_rows(v: &Values, page: &str, cat: usize) -> Vec<(u16, usize)> {
    match page {
        "cfilter" => (0..CFILTER.len()).map(|r| (CFILTER[r].2, cfilter_state(v, r))).collect(),
        "fxfilter" => (0..18u16).map(|r| (153 + r, if v.get(0x92) & (1 << r) != 0 { 88 } else { 89 })).collect(),
        "conf11s" => LOG_ROWS[cat.min(3)].iter().map(|&(label, bit)| (label, if log_bit(v, 0, bit) { 88 } else { 89 })).collect(),
        _ => Vec::new(),
    }
}

/// A row's Enter there (retail's toggles).
pub(super) fn modern_toggle(v: &mut Values, out: &mut Out, page: &str, cat: usize, r: usize) {
    match page {
        "cfilter" if r < CFILTER.len() => cfilter_toggle(v, out, r),
        "fxfilter" if r < 18 => {
            let on = v.get(0x92) & (1 << r) != 0;
            set_bit(v, out, 0x92, 1 << r, !on);
        }
        "conf11s" => {
            if let Some(&(_, bit)) = LOG_ROWS[cat.min(3)].get(r) {
                let on = log_bit(v, 0, bit);
                let (lo, hi) = log_ids(0);
                if bit < 32 { set_bit(v, out, lo, 1 << bit, !on) } else { set_bit(v, out, hi, 1 << (bit - 32), !on) }
            }
        }
        _ => {}
    }
}

/// Font Colors' rows of a category: (label, sample, config id).
pub(super) fn colour_rows(cat: usize) -> &'static [(u16, u16, u16)] {
    COLOUR_ROWS[cat.min(3)]
}

/// A colour set as textcol3's OK does: 0x80RRGGBB.
pub(super) fn set_colour(v: &mut Values, out: &mut Out, id: u16, [r, g, b]: [u8; 3]) {
    if v.set(id, 0x8000_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32) {
        out.changed.push(id);
    }
}

fn text_b(gu: &mut GameUi, id: u16, x: f32, y: f32, colour: [u8; 4]) {
    if let Some(t) = gu.group_b(id as usize) {
        gu.text(&t, x, y, 0, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfilter_cycles() {
        let mut v = Values::default();
        let mut out = Out::default();
        // Say: OFF -> ON -> Hold -> OFF.
        cfilter_toggle(&mut v, &mut out, 0);
        assert_eq!((v.get(0x32) & 1, cfilter_state(&v, 0)), (1, 88));
        cfilter_toggle(&mut v, &mut out, 0);
        assert_eq!((v.get(0x32) & 1, v.get(0x34) & 1, cfilter_state(&v, 0)), (0, 1, 83));
        cfilter_toggle(&mut v, &mut out, 0);
        assert_eq!(cfilter_state(&v, 0), 89);
        // Tell, from OFF + icon (0x47 defaults to 1): Hold + icon -> OFF -> Hold -> OFF + icon.
        let seq: Vec<usize> = (0..4)
            .map(|_| {
                cfilter_toggle(&mut v, &mut out, 1);
                cfilter_state(&v, 1)
            })
            .collect();
        assert_eq!(seq, vec![137, 89, 83, 136]);
        // Yell is bit 46 = 0x33 bit 16 (bits 11 / 12 are on by default).
        cfilter_toggle(&mut v, &mut out, 10);
        assert_eq!(v.get(0x33), 0x1800 | 1 << 16);
        // System levels.
        cfilter_toggle(&mut v, &mut out, 53);
        assert_eq!(v.get(0x35), 2);
        cfilter_toggle(&mut v, &mut out, 53);
        assert_eq!(v.get(0x35), 1);
    }

    #[test]
    fn key_assignment_is_unique() {
        let mut v = Values::default();
        let mut out = Out::default();
        // W (0x11) is Move Up by default; give it to row 1 (Move Down).
        assert_eq!(v.get(0x4a), 0x11);
        assign(&mut v, &mut out, "k1assign", 1, egui::Key::W);
        assert_eq!((v.get(0x4a), v.get(0x4b)), (0, 0x11));
        // The same key again unassigns.
        assign(&mut v, &mut out, "k1assign", 1, egui::Key::W);
        assert_eq!(v.get(0x4b), 0);
    }
}
