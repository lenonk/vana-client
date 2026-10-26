//! The Linkshell Concierge (ls_concierge_spec.md): s2c 0x048's sixteen slots (DAT_10488b08), the list event op 0x71
//! sub 0x40 opens (link5 in mode 1 with link6 mode 1 and the language filter "menu    lscswin"), and the pick sub 0x41
//! hands back. Text code `7f 81 n` prints a slot's shell name.

use super::World;
use super::game_ui::GameUi;
use super::linkshell::{LIST, ls_name};
use crate::audio::Ui;

const HEADER: &str = "menu    link6";
const FILTER: &str = "menu    lscswin";
const ROWS: usize = 10;
const PITCH: f32 = 18.0;
/// Table 0x1037f318: {0, 18, 280, 320, end 350}.
const COLS: [f32; 5] = [0.0, 18.0, 280.0, 320.0, 350.0];
const WHITE: u32 = 0x8080_8080;
const GREEN: u32 = 0x8050_9050;

/// A slot (32 bytes): +0 the row's item (its icon; 0 = no row), +4 the shell's extdata, +0x1c the attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub item: u16,
    pub ext: [u8; 24],
    pub attrs: u32,
}

/// The list and lscswin: the title (group 9 0x119 + n), the language filter (0 all, 1 / 2 / 5), lscswin's cursor while
/// it has the focus.
#[derive(Clone, Debug)]
pub struct Concierge {
    title: u8,
    pub top: usize,
    pub cursor: usize,
    filter: u8,
    lsc: Option<u16>,
}

/// s2c 0x048 (0x100a0180): per entry k, index 0xfe clears the table (FUN_10093b50), a signed index 0..15 fills that
/// slot, anything else is skipped.
pub fn on_packet(w: &mut World, p: &[u8]) {
    apply(&mut w.concierge, p);
}

fn apply(slots: &mut [Slot; 16], p: &[u8]) {
    let u16_at = |o: usize| p.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| p.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    for k in 0..4 {
        let index = p.get(4 + k).copied().unwrap_or(0xff);
        if index == 0xfe {
            *slots = [Slot::default(); 16];
        } else if (index as i8) >= 0 && index < 16 {
            let mut ext = [0u8; 24];
            if let Some(e) = p.get(0x10 + 0x18 * k..0x28 + 0x18 * k) {
                ext.copy_from_slice(e);
            }
            slots[index as usize] = Slot { item: u16_at(8 + 2 * k), ext, attrs: u32_at(0x70 + 4 * k) };
        }
    }
}

/// `7f 81 n` (FUN_100bda90 -> FUN_101cbac0): a slot's shell name while its extdata Flag is set.
pub fn names(w: &World) -> Vec<Option<String>> {
    w.concierge.iter().map(|s| (s.ext[8] != 0).then(|| ls_name(&s.ext[9..24]))).collect()
}

/// FUN_101e3650: the lowest of attribute bits 1..5 set (1 Japanese, 2 English, 5 Other), else 0.
fn language(attrs: u32) -> u8 {
    (1..=5).find(|b| attrs >> b & 1 != 0).unwrap_or(0)
}

/// FUN_101e3730: the header row (None), then each slot with an item, by the filter.
fn rows(w: &World) -> Vec<Option<usize>> {
    let filter = w.concierge_ui.as_ref().map_or(0, |c| c.filter);
    std::iter::once(None)
        .chain(w.concierge.iter().enumerate().filter(|(_, s)| s.item != 0 && (filter == 0 || language(s.attrs) == filter)).map(|(i, _)| Some(i)))
        .collect()
}

/// Op 0x71 sub 0x40 (FUN_101e4470(1, cb, n)): link5 / link6 and the item windows close, the pick is cleared, the list
/// opens on its header row.
pub fn open(w: &mut World, title: u8) {
    w.linkshell = None;
    w.concierge_pick = [0; 3];
    w.concierge_ui = Some(Concierge { title, top: 0, cursor: 0, filter: 0, lsc: None });
}

/// FUN_101e4570 after a pick or the close hook FUN_101e4380 (cb(0, 0): the pick stays as it was).
fn close(w: &mut World) {
    w.concierge_ui = None;
}

/// LAB_100b8460 through FUN_101e4690: a row with a shell (GroupId != 0) is the event's pick (index, GroupId, attrs).
fn pick(w: &mut World, index: usize) {
    let s = w.concierge[index];
    let group = u32::from_le_bytes([s.ext[0], s.ext[1], s.ext[2], s.ext[3]]);
    if group != 0 {
        w.concierge_pick = [index as u32, group, s.attrs];
    }
}

/// The list base (FUN_101f6c70) and +0x5c / +0x58: Up / Down stop at the ends, Left / Right page, Enter on the header
/// does nothing but SE 0002, on a shell picks it and closes, Escape cancels, Num+ hands the focus to lscswin (SE 0013).
/// lscswin: its ring, Enter sets the filter (1 all, 2 Japanese, 3 English, 4 Other) and gives the focus back, as do
/// Escape and Num+.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut c) = w.concierge_ui.clone() else { return };
    if let Some(sel) = c.lsc {
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                let links = gu.menu_links(FILTER, sel).unwrap_or([0; 6]);
                let to = links[if key == egui::Key::ArrowUp { 0 } else { 1 }] as u16;
                if to != 0 && to != sel {
                    c.lsc = Some(to);
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            egui::Key::Enter => {
                w.sounds.push(Ui::Select as u32);
                c.filter = [0, 0, 1, 2, 5][sel.min(4) as usize];
                (c.lsc, c.top, c.cursor) = (None, 0, 0);
            }
            egui::Key::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                c.lsc = None;
            }
            _ => {}
        }
        w.concierge_ui = Some(c);
        return;
    }
    let n = rows(w).len();
    match key {
        egui::Key::ArrowDown if c.cursor + 1 < n => {
            c.cursor += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowUp if c.cursor > 0 => {
            c.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowRight | egui::Key::ArrowLeft => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if super::scrollbar::rows_abs(m, &mut c.top, &mut c.cursor, n, ROWS) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if let Some(Some(i)) = rows(w).get(c.cursor).copied() {
                pick(w, i);
                close(w);
                return;
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
            return;
        }
        _ => {}
    }
    c.top = c.top.min(c.cursor).max((c.cursor + 1).saturating_sub(ROWS));
    w.concierge_ui = Some(c);
}

/// Message 8 (Num+) on the list: lscswin takes the focus on child 1, SE 0013; on lscswin it gives it back.
pub fn num_plus(w: &mut World) {
    let Some(c) = w.concierge_ui.as_mut() else { return };
    if c.lsc.is_some() {
        c.lsc = None;
    } else {
        c.lsc = Some(1);
        w.sounds.push(Ui::Close as u32);
    }
}

/// Mouse: link5's rows and lscswin's children as the generic handler takes them, a right click the focused window's
/// Escape (message 6); the pointer over lscswin while the list has the focus hands it over (FUN_101e4610, SE 0013).
#[allow(clippy::too_many_arguments)]
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(c) = w.concierge_ui.clone() else { return };
    if right {
        key(w, gu, egui::Key::Escape);
        return;
    }
    let hit = |name: &str| -> Option<u16> {
        let (x, y) = gu.window_xy(name, sw, sh)?;
        let lay = gu.layout(name)?;
        lay.elements.iter().skip(1).find(|e| {
            let (ex, ey) = (x + e.x as f32, y + e.y as f32);
            p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
        }).map(|e| e.id)
    };
    if let Some(id) = hit(FILTER) {
        gu.hover.set(true);
        match c.lsc {
            None => num_plus(w),
            Some(sel) if id != sel && (moved || clicked) => {
                if let Some(c) = w.concierge_ui.as_mut() {
                    c.lsc = Some(id);
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            Some(_) if clicked => key(w, gu, egui::Key::Enter),
            _ => {}
        }
        return;
    }
    if c.lsc.is_some() {
        return;
    }
    let Some(id) = hit(LIST) else { return };
    let row = c.top + id as usize - 1;
    if row >= rows(w).len() {
        return;
    }
    gu.hover.set(true);
    if row != c.cursor && (moved || clicked) {
        if let Some(c) = w.concierge_ui.as_mut() {
            c.cursor = row;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The list's bar (messages 0x16..0x1b).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let n = rows(w).len();
    let Some(c) = w.concierge_ui.as_mut().filter(|c| c.lsc.is_none()) else { return false };
    super::scrollbar::rows_abs(m, &mut c.top, &mut c.cursor, n, ROWS)
}

/// Help 0xe 0 on the list (0xe 0x374 on lscswin), title 0xd 0x47.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(c) = &w.concierge_ui else { return };
    let help = if c.lsc.is_some() { 0x374 } else { 0 };
    bars.help[1] = gu.menu_msg(help, "");
    bars.title[1] = gu.focus_text(LIST, 0)[1].clone().unwrap_or_default();
}

/// link6 mode 1 (FUN_101e5a80): group 9 0x119 + n at (+0x40, +0x14), the Linkshell's icon (item 0x200) at (+10, +0xc);
/// link5 (FUN_101f6a60 with FUN_101e3730's cells: the header sprite windowps 718 at the row's (x - 3, y); per shell the
/// keytops3 0x219 backdrop and the row's item icon at half scale in the shell's colour, the name fitted to its column
/// (green for our linkshell 1's group), "J" / "E" / "?", the members wanted); lscswin with its hand while it has the
/// focus (its draw marks no filter).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(c) = &w.concierge_ui else { return rects };
    let focused = w.confirm.is_none();
    if let Some(info) = &w.item_info {
        super::items::upload_icons(gu, info, w.concierge.iter().map(|s| s.item).filter(|&i| i != 0).chain(std::iter::once(0x200)));
    }
    if let Some(win) = gu.lobby_window_off(HEADER, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, ..] = win.frame;
        let title = w.battle.as_ref().and_then(|b| b.group9(0x119 + c.title as usize)).unwrap_or_default();
        gu.text(&title, x + 64.0, y + 20.0, 0, WHITE.to_le_bytes());
        super::linkshell::icon(gu, 0x200, x + 10.0, y + 12.0, 32.0, WHITE);
        rects.push(win.frame);
    }
    let rows = rows(w);
    let ls1 = super::linkshell::group_of(w, 1);
    let list_focus = focused && c.lsc.is_none();
    let sel = if list_focus { (c.cursor.saturating_sub(c.top) + 1) as u16 } else { 0 };
    if let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) {
        rects.extend(gu.window_rect(LIST, sw, sh));
        for (k, r) in rows.iter().enumerate().skip(c.top).take(ROWS) {
            let (rx, ry) = (wx + 3.0, wy + 3.0 + PITCH * (k - c.top) as f32);
            let Some(i) = r else {
                gu.sprite("menu    windowps", 718, rx - 3.0, ry, WHITE);
                continue;
            };
            let s = &w.concierge[*i];
            let group = u32::from_le_bytes([s.ext[0], s.ext[1], s.ext[2], s.ext[3]]);
            gu.sprite("menu    keytops3", 0x219, rx + COLS[0], ry + 2.0, WHITE);
            super::linkshell::icon(gu, s.item, rx + COLS[0], ry + 2.0, 16.0, super::linkshell::tint(s.ext[6], s.ext[6] >> 4, s.ext[7]));
            let tint = if ls1.is_some_and(|g| g == group) { GREEN } else { WHITE };
            gu.text_fit(&ls_name(&s.ext[9..24]), rx + COLS[1], ry + 2.0, COLS[2] - COLS[1] - 8.0, tint);
            let lang = match language(s.attrs) {
                1 => "J",
                2 => "E",
                5 => "?",
                _ => " ",
            };
            gu.text(lang, rx + COLS[2], ry + 2.0, 0, WHITE.to_le_bytes());
            gu.text(&(s.attrs >> 6 & 0xf).to_string(), rx + COLS[3], ry + 2.0, 0, WHITE.to_le_bytes());
        }
        if list_focus {
            gu.sprite("menu    windowps", 75, wx, wy + 4.0 + PITCH * c.cursor.saturating_sub(c.top) as f32, WHITE);
        }
        if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
            gu.scroll_bar(fx + fw - 8.0, fy, fh, rows.len(), ROWS, c.top);
            if list_focus {
                gu.bar_site(super::scrollbar::Owner::Linkshell, fx + fw - 8.0, fy, fh, rows.len(), ROWS, c.top);
            }
        }
    }
    let lsc_focus = focused && c.lsc.is_some();
    if let Some(win) = gu.lobby_window_off(FILTER, sw, sh, (0.0, 0.0), c.lsc.filter(|_| lsc_focus), lsc_focus, &[]) {
        rects.push(win.frame);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0x100a0180: 0xfe clears (the entries before it in the same packet too), a negative or >= 16 index is skipped.
    #[test]
    fn packet() {
        let mut p = vec![0u8; 0x80];
        p[4..8].copy_from_slice(&[3, 0xff, 0x10, 5]);
        p[8..10].copy_from_slice(&515u16.to_le_bytes());
        p[0x10..0x14].copy_from_slice(&0x1234u32.to_le_bytes());
        p[0x18] = 1;
        p[0x70..0x74].copy_from_slice(&0x84u32.to_le_bytes());
        p[0x0e..0x10].copy_from_slice(&515u16.to_le_bytes());
        let mut slots = [Slot::default(); 16];
        apply(&mut slots, &p);
        assert_eq!((slots[3].item, slots[3].attrs, slots[3].ext[8]), (515, 0x84, 1));
        assert_eq!(slots[5].item, 515);
        assert_eq!(slots.iter().filter(|s| s.item != 0).count(), 2);
        p[4] = 0xfe;
        apply(&mut slots, &p);
        assert_eq!(slots.iter().filter(|s| s.item != 0).count(), 1);
        assert_eq!(language(0x84), 2);
        assert_eq!(language(0x20), 5);
        assert_eq!(language(1), 0);
    }
}
