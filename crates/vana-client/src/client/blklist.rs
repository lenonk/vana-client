//! The block list (spec ~/ffxi/lab/research/blklist_spec.md): "menu    blklist", the session's blacklist one name a row
//! (FUN_101def10, from /blacklist with under 3 words), and "menu    blkmain", its one Delete button (FUN_101defc0).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const LIST: &str = "menu    blklist";
const MAIN: &str = "menu    blkmain";
const ROWS: usize = 15;
const WHITE: u32 = 0x8080_8080;

pub struct BlkList {
    rows: Vec<(u32, String)>,
    top: usize,
    cursor: usize,
    /// blkmain is up over the list.
    main: bool,
}

/// FUN_101def10: a list already up is closed and made again, at the top.
pub fn open(w: &mut World) {
    w.blklist = Some(BlkList { rows: Vec::new(), top: 0, cursor: 0, main: false });
    rebuild(w);
}

/// FUN_101decc0: the rows from the list in its order (none while it is not in, asking for it), the view back at the
/// top with the hand on its screen row, pulled onto the last row when past the end (FUN_101f69d0).
pub fn rebuild(w: &mut World) {
    if w.blklist.is_none() {
        return;
    }
    if !w.partyreq.black_loaded {
        super::partyreq::ask_blacklist(w);
    }
    let rows = if w.partyreq.black_loaded { w.partyreq.blacklist.clone() } else { Vec::new() };
    if let Some(b) = w.blklist.as_mut() {
        let hand = b.cursor - b.top;
        b.rows = rows;
        b.top = 0;
        b.cursor = hand.min(b.rows.len().saturating_sub(1));
    }
}

/// FUN_101f6c70 (the list base): Up / Down stop at the ends, Left / Right page by 15 with the hand on its row (at the
/// ends it goes to the first / last row); Enter opens blkmain (FUN_101dee40), nothing on an empty list; Escape closes
/// both. blkmain: Enter sends the delete (FUN_101defc0: c2s 0x03D mode 1, no prompt, no line) and closes it.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(b) = w.blklist.as_mut() else { return };
    let n = b.rows.len();
    if b.main {
        match key {
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                b.main = false;
                if let Some((id, name)) = b.rows.get(b.cursor).cloned()
                    && !super::partyreq::black_delete(w, id, &name)
                {
                    rebuild(w);
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                b.main = false;
            }
            _ => {}
        }
        return;
    }
    let last = n.saturating_sub(1);
    let max_top = n.saturating_sub(ROWS);
    let (top, cursor) = (b.top, b.cursor);
    let (top, cursor) = match key {
        K::ArrowDown if cursor < last => (top.max((cursor + 2).saturating_sub(ROWS)), cursor + 1),
        K::ArrowUp if cursor > 0 => (top.min(cursor - 1), cursor - 1),
        K::ArrowRight if n > 0 && top < max_top => {
            let t = (top + ROWS).min(max_top);
            (t, cursor + t - top)
        }
        K::ArrowRight if n > 0 => (top, last),
        K::ArrowLeft if top > 0 => {
            let t = top.saturating_sub(ROWS);
            (t, cursor - (top - t))
        }
        K::ArrowLeft => (top, 0),
        K::Enter => {
            if n > 0 {
                w.sounds.push(Ui::Select as u32);
                b.main = true;
            }
            return;
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.blklist = None;
            return;
        }
        _ => return,
    };
    if (top, cursor) != (b.top, b.cursor) {
        (b.top, b.cursor) = (top, cursor);
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// The mouse (the list base, message 9 / 5 / 6): hovering a row (not past the count) moves the hand there, a click on
/// the hand's row or on Delete is Enter, a right click on the window is Escape.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(b) = w.blklist.as_ref() else { return };
    let (win, child) = if b.main { (MAIN, None) } else { (LIST, Some(())) };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(win, sw, sh), gu.layout(win)) else { return };
    let inside = |e: &crate::dat::ui::MenuElement| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    };
    if gu.window_rect(win, sw, sh).is_some_and(|[fx, fy, fw, fh]| p.0 >= fx && p.0 < fx + fw && p.1 >= fy && p.1 < fy + fh) {
        gu.hover.set(true);
        if right {
            key(w, egui::Key::Escape);
            return;
        }
    }
    let Some(e) = lay.elements.iter().skip(1).find(|e| inside(e)) else { return };
    if child.is_none() {
        if e.id == 1 && clicked {
            key(w, egui::Key::Enter);
        }
        return;
    }
    let row = b.top + e.id as usize - 1;
    if row >= b.rows.len() {
        return;
    }
    if row != b.cursor && (moved || clicked) {
        if let Some(b) = w.blklist.as_mut() {
            b.cursor = row;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter);
    }
}

/// FUN_101f6a60: the names in the shape font at (21, 4 + 16 i); empty, group 7 0x67 in the game font at (35, 28); the
/// cursor band windowps 75 while the list has the focus; the scroll bar; blkmain from its layout.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(b) = &w.blklist else { return rects };
    let focused = !b.main && w.confirm.is_none();
    let n = b.rows.len();
    let sel = if focused && n > 0 { (b.cursor - b.top + 1) as u16 } else { 0 };
    let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) else { return rects };
    rects.extend(gu.window_rect(LIST, sw, sh));
    if n == 0 {
        let s = w.battle.as_ref().and_then(|bt| bt.group7(0x67)).unwrap_or_default();
        gu.text(&s, wx + 35.0, wy + 28.0, 0, crate::dat::battle::argb_bytes(0x8080_4040));
    }
    for (i, (_, name)) in b.rows.iter().enumerate().skip(b.top).take(ROWS) {
        gu.shape_text(name, wx + 21.0, wy + 4.0 + 16.0 * (i - b.top) as f32, WHITE, false);
    }
    if focused && n > 0 {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (b.cursor - b.top) as f32, WHITE);
    }
    if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
        gu.scroll_bar(fx + fw - 8.0, fy, fh, n, ROWS, b.top);
    }
    if b.main
        && let Some(win) = gu.lobby_window_off(MAIN, sw, sh, (0.0, 0.0), Some(1), w.confirm.is_none(), &[])
    {
        rects.push(win.frame);
    }
    rects
}

/// The bars from the focused window's strings: blklist "Blacklist" with no help, blkmain "Delete from blacklist.".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(b) = &w.blklist else { return };
    let [help, title] = if b.main { gu.focus_text(MAIN, 1) } else { gu.focus_text(LIST, (b.cursor - b.top + 1) as u16) };
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    bars.help[2] = String::new();
}
