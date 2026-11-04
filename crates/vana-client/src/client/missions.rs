//! The social page's Missions row (FUN_1014e7b0 case 5 child 1; MODLOG "Missions"): "menu    missionm" (the
//! seven categories, over the social menu: group 0x81 suspends it) and "menu    miss00" (one log: the current mission,
//! the completed ones, a description), which leaves missionm drawn beside it unfocused.

use super::World;
use super::game_ui::GameUi;
use super::journal::{self, Row};
use crate::audio::Ui;
use std::sync::Arc;

pub const MENU: &str = "menu    missionm";
pub const LIST: &str = "menu    miss00";
const VISIBLE: usize = 8;
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;

#[derive(Clone, Debug)]
pub struct Missions {
    /// missionm's cursor child; None when /mission opened the list alone.
    pub menu: Option<u16>,
    pub list: Option<List>,
}

#[derive(Clone, Debug)]
pub struct List {
    pub log: usize,
    /// +0x64: 0 waiting for the store, 1 the list, 2 a description.
    pub state: u8,
    pub rows: Vec<Row>,
    pub top: usize,
    /// The cursor's row.
    pub cursor: usize,
    /// The paged-list base's remembered direction (+0x60): 0 none, 1 Down, 2 Up. Held moves do not wrap at the edge
    /// it remembers.
    pub dir: u8,
}

/// FUN_1022da40: children 2..5 and 7 need Zilart, Promathia, Aht Urhgan, Altana, Adoulin (bits 1..4, 0xb).
fn menu_off(enabled: u32) -> Vec<u16> {
    [(2u16, 1u32), (3, 2), (4, 3), (5, 4), (7, 0xb)].into_iter().filter(|&(_, b)| enabled >> b & 1 == 0).map(|(c, _)| c).collect()
}

/// The journal's name files, read on the first open.
pub(super) fn logs(w: &mut World) -> Option<Arc<crate::dat::journal::Logs>> {
    if w.logs.is_none() {
        let index = w.index.clone()?;
        w.logs = Some(Arc::new(crate::dat::journal::Logs::load(&index)));
    }
    w.logs.clone()
}

/// Social child 1: missionm with its cursor on child 1.
pub fn open_menu(w: &mut World) {
    logs(w);
    w.missions = Some(Missions { menu: Some(w.cursor_recall(MENU)), list: None });
}

/// FUN_1022dbd0 + miss00's open (FUN_1022dcf0): the list on `log`, or waiting while the store is incomplete.
pub(super) fn open_list(w: &mut World, log: usize) {
    let mut l = List { log, state: 0, rows: Vec::new(), top: 0, cursor: 1, dir: 0 };
    build(w, &mut l);
    if let Some(m) = w.missions.as_mut() {
        m.list = Some(l);
    }
}

/// The logs the modern page lists: the three nations, then those whose expansion is enabled (TVR once RoV is past
/// 340), as the Left / Right cycle allows.
pub(super) fn shown_logs(w: &World) -> Vec<usize> {
    (0..15).filter(|&l| (l < 3 || w.expansions >> journal::MISSION_BITS[l] & 1 != 0) && !(l == 11 && w.journal.rov < 0x154)).collect()
}

/// The modern page: a log with the cursor on an entry (else the first row that is not a header).
pub(super) fn show_log(w: &mut World, log: usize, entry: Option<usize>) {
    if w.missions.is_none() {
        w.missions = Some(Missions { menu: None, list: None });
    }
    open_list(w, log);
    if let Some(l) = w.missions.as_mut().and_then(|m| m.list.as_mut()) {
        let at = entry.and_then(|e| l.rows.iter().position(|r| matches!(r, Row::Current(i) | Row::Done(i) if *i == e)));
        if let Some(k) = at {
            (l.cursor, l.top) = (k, k);
        }
    }
}

/// FUN_1022dd30: the rows of `l.log`, cursor on row 1, top 0; only once the store is complete.
fn build(w: &mut World, l: &mut List) {
    let Some(logs) = logs(w) else { return };
    if !w.journal.complete() {
        l.state = 0;
        return;
    }
    let empty = crate::dat::journal::LogFile::default();
    let f = logs.missions.get(l.log).unwrap_or(&empty);
    (l.rows, l.state, l.top, l.cursor) = (journal::mission_rows(&w.journal, l.log, f), 1, 0, 1);
}

/// /mission (0x4e, FUN_1007d270): the list alone, on the log whose title (group 0xd) is the argument, else the nation's.
pub fn command(w: &mut World, gu: &GameUi, arg: Option<&str>) {
    logs(w);
    let nation = w.journal.nation as usize;
    let log = arg
        .and_then(|a| (0..15).filter(|&l| l < 3 || w.expansions >> journal::MISSION_BITS[l] & 1 != 0).find(|&l| gu.title_msg(journal::MISSION_TITLES[l]).as_deref() == Some(a)))
        .unwrap_or(nation.min(2));
    w.missions = Some(Missions { menu: None, list: None });
    open_list(w, log);
}

/// Per frame: a waiting list builds once the store is complete (state 0 polls byte 0x1047fe8f).
pub fn update(w: &mut World) {
    let Some(mut l) = w.missions.as_ref().and_then(|m| m.list.clone()).filter(|l| l.state == 0) else { return };
    build(w, &mut l);
    if l.state != 0
        && let Some(m) = w.missions.as_mut()
    {
        m.list = Some(l);
    }
}

/// The list is waiting: the busy strip (helpwind busy bit 1).
pub fn busy(w: &World) -> bool {
    w.missions.as_ref().and_then(|m| m.list.as_ref()).is_some_and(|l| l.state == 0)
}

/// Down / Up with wrap, then a header row is passed over (FUN_1022e760: "Completed:" repeats the move, "Current:"
/// too on a fresh press).
///
/// A header landed on by a *held* key is different (0x1022e9f4): the move is taken back by one row downwards and the
/// base's remembered direction (+0x60) is set to 2 (Up), which stops the next held move from wrapping at the top edge
/// (FUN_10222150's 0x10222300; +0x60 == 1 is the mirror at the bottom). So a held Up that reaches the "Current:"
/// header sits on the row under it, while a fresh press from there skips the header and wraps to the last row.
/// *(inf: the edge test is read as "no wrap", retail's "one page instead of the count")*
fn step(l: &mut List, down: bool, repeat: bool) -> bool {
    let n = l.rows.len();
    if n == 0 {
        return false;
    }
    let before = l.cursor;
    let head = |l: &List, k: usize| matches!(l.rows[k], Row::CurHead | Row::DoneHead);
    // At an edge a held move in the direction it already remembers does not wrap.
    let edge = repeat && if down { l.cursor + 1 == n && l.dir == 1 } else { l.cursor == 0 && l.dir == 2 };
    if !edge {
        l.cursor = if down { (l.cursor + 1) % n } else { (l.cursor + n - 1) % n };
        // FUN_10222150: 1 is Down, 2 Up (the held-header bounce calls it with 1, Down).
        l.dir = if down { 1 } else { 2 };
        if head(l, l.cursor) {
            if repeat {
                // Back off the header, downwards whichever way we came, and remember Up.
                (l.cursor, l.dir) = ((l.cursor + 1) % n, 2);
            } else {
                while head(l, l.cursor) {
                    let next = if down { (l.cursor + 1) % n } else { (l.cursor + n - 1) % n };
                    if next == before {
                        break;
                    }
                    l.cursor = next;
                }
                l.dir = 0;
            }
        }
    }
    show(l);
    l.cursor != before
}

/// The top follows the cursor (FUN_10221b90).
fn show(l: &mut List) {
    if l.cursor < l.top {
        l.top = l.cursor;
    } else if l.cursor >= l.top + VISIBLE {
        l.top = l.cursor + 1 - VISIBLE;
    }
}

/// A scroll bar or wheel message (FUN_10222150 0x16..0x1b; no header skip there).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    // A disabled list (the pointer was on a page arrow, last frame's hover) takes no bar or wheel message either.
    if w.list_arrow_hover {
        return false;
    }
    let Some(l) = w.missions.as_mut().and_then(|m| m.list.as_mut()).filter(|l| l.state == 1) else { return false };
    let n = l.rows.len();
    let mut row = l.cursor.saturating_sub(l.top);
    let moved = super::scrollbar::paged(m, &mut l.top, &mut row, n, VISIBLE);
    l.cursor = l.top + row;
    moved
}

/// Keys: miss00's when it is open, else missionm's.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key, page: bool) {
    let Some(m) = w.missions.clone() else { return };
    if let Some(mut l) = m.list {
        list_key(w, &mut l, key, page);
        if let Some(mm) = w.missions.as_mut()
            && mm.list.is_some()
        {
            mm.list = Some(l);
        }
        return;
    }
    let Some(sel) = m.menu else { return };
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let k = if key == egui::Key::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(MENU, sel).map_or(sel, |l| l[k] as u16);
            if to != sel && to != 0 && to != 0xff {
                w.missions.as_mut().unwrap().menu = Some(to);
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.missions = None;
        }
        egui::Key::Enter if menu_off(w.expansions).contains(&sel) => w.sounds.push(Ui::Error as u32),
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            // FUN_1022db00 / FUN_1022dbd0: the category's log.
            let log = match sel {
                1 => (w.journal.nation as usize).min(2),
                2 => 3,
                3 => 4,
                4 => 6,
                5 => 8,
                7 => 9,
                _ if w.journal.rov < 0x154 => 10,
                _ => 11,
            };
            open_list(w, log);
        }
        _ => {}
    }
}

fn list_key(w: &mut World, l: &mut List, key: egui::Key, page: bool) {
    // The list ignores every message while it is disabled by an arrow hover (list +0x5c bit 1).
    if w.list_arrow_hover {
        return;
    }
    match (l.state, key) {
        (0, egui::Key::Escape) | (1, egui::Key::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            match w.missions.as_mut() {
                Some(m) if m.menu.is_some() => m.list = None,
                _ => w.missions = None,
            }
        }
        (1, egui::Key::ArrowUp | egui::Key::ArrowDown) => {
            if step(l, key == egui::Key::ArrowDown, w.key_repeat) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // With 0xa8 (Ctrl / Alt) a page; else the next / previous log.
        (1, egui::Key::ArrowLeft | egui::Key::ArrowRight) if page => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            let (was, mut row) = (l.cursor, l.cursor.saturating_sub(l.top));
            if super::scrollbar::paged(m, &mut l.top, &mut row, l.rows.len(), VISIBLE) {
                l.cursor = l.top + row;
                // FUN_1022e710: a header landed on moves to the next row; SE 0001 only when the cursor ends elsewhere.
                if matches!(l.rows.get(l.cursor), Some(Row::CurHead | Row::DoneHead)) {
                    step(l, true, false);
                }
                if l.cursor != was {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
        }
        (1, egui::Key::ArrowLeft | egui::Key::ArrowRight) => {
            l.log = journal::next_log(&w.journal, l.log, w.expansions, key == egui::Key::ArrowRight);
            build(w, l);
            w.sounds.push(Ui::Cursor as u32);
        }
        (1, egui::Key::Enter) => {
            w.sounds.push(Ui::Select as u32);
            if matches!(l.rows.get(l.cursor), Some(Row::Current(_) | Row::Done(_))) {
                l.state = 2;
            }
        }
        (2, egui::Key::Enter | egui::Key::Escape) => {
            w.sounds.push(if key == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
            l.state = 1;
        }
        _ => {}
    }
}

/// Mouse on the list: hover moves the cursor to a row (headers passed over), a click on the cursor row is Enter, a
/// click on the arrows (children 9 / 10) Left / Right; on missionm the generic hover and click.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, page: bool) {
    let Some(m) = w.missions.clone() else { return };
    let inside = |x: f32, y: f32, e: &crate::dat::ui::MenuElement| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    };
    if let Some(l) = &m.list {
        let (Some((x, y)), Some(lay)) = (gu.window_xy(LIST, sw, sh), gu.layout(LIST)) else { return };
        let Some(e) = lay.elements.iter().skip(1).find(|e| inside(x, y, e)) else { return };
        // FUN_10221cd0: over the page arrows the list is disabled, so no hand and no list keys; a click on them is
        // Left / Right through the 0xa8 test, which with Ctrl / Alt held is a page on the disabled list, i.e. nothing.
        if matches!(e.id, 9 | 10) && l.state == 1 {
            // The click's Left / Right goes to the window (the log switch), not the list, so it is sent before the
            // hover disables the list.
            if clicked && !page {
                key(w, gu, if e.id == 9 { egui::Key::ArrowLeft } else { egui::Key::ArrowRight }, false);
            }
            w.list_arrow_hover = true;
            return;
        }
        gu.hover.set(true);
        match e.id {
            c @ 1..=8 if l.state == 1 => {
                let row = l.top + c as usize - 1;
                if row >= l.rows.len() || matches!(l.rows[row], Row::CurHead | Row::DoneHead) {
                    return;
                }
                if row != l.cursor && (moved || clicked) {
                    if let Some(ll) = w.missions.as_mut().and_then(|m| m.list.as_mut()) {
                        ll.cursor = row;
                    }
                    w.sounds.push(Ui::Cursor as u32);
                } else if clicked {
                    key(w, gu, egui::Key::Enter, false);
                }
            }
            _ if clicked && l.state == 2 => key(w, gu, egui::Key::Enter, false),
            _ => {}
        }
        return;
    }
    let (Some(sel), Some((x, y)), Some(lay)) = (m.menu, gu.window_xy(MENU, sw, sh), gu.layout(MENU)) else { return };
    let Some(e) = lay.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).find(|e| inside(x, y, e)) else { return };
    gu.hover.set(true);
    if e.id != sel && (moved || clicked) {
        w.missions.as_mut().unwrap().menu = Some(e.id);
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter, false);
    }
}

/// missionm: the focused child's help and title (frame string 0 is its help 468); miss00: its frame's title
/// "Missions", no help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(m) = &w.missions else { return };
    if m.list.is_some() {
        (bars.help[1], bars.title[1]) = (String::new(), gu.focus_text(LIST, 0)[1].clone().unwrap_or_default());
    } else if let Some(sel) = m.menu {
        let [help, title] = gu.focus_text(MENU, sel);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
}

/// Wrapped game-font lines 16 px apart.
pub(super) fn lines(gu: &mut GameUi, s: &str, x: f32, y: f32, width: f32, tint: u32) {
    for (k, line) in gu.wrap(s, width).iter().enumerate() {
        gu.text(line, x, y + 16.0 * k as f32, 0, tint.to_le_bytes());
    }
}

/// missionm (disabled rows by expansion), then miss00: the log's title at (60, 10), the rows in children 1..8
/// (FUN_1022ead0: headers at child x - 15, messages at + 3, entries with framesus 35 at (4 + 6 indent, 5) and the
/// name at (18 + 6 indent, 0), the cursor row 0x80803040), the scroll bar; state 0 "Preparing data."; state 2 the
/// description box (FUN_10233190: framesus 35 at (36, 25), the name at (50, 20), the text at (40, 50) wrapped at 327).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(m) = &w.missions else { return rects };
    if let Some(sel) = m.menu {
        let off = menu_off(w.expansions);
        let focused = m.list.is_none() && w.confirm.is_none();
        if let Some(win) = gu.lobby_window_off(MENU, sw, sh, (0.0, 0.0), Some(sel), focused, &off) {
            if let Some((x, y)) = gu.window_xy(MENU, sw, sh) {
                gu.disabled_children(MENU, x, y, &off, Some(sel).filter(|_| focused));
            }
            rects.push(win.frame);
        }
    }
    let Some(l) = &m.list else { return rects };
    let logs = w.logs.clone();
    let empty = crate::dat::journal::LogFile::default();
    let f = logs.as_ref().and_then(|g| g.missions.get(l.log)).unwrap_or(&empty);
    let shown: Vec<u16> = if l.state == 1 { (1..=VISIBLE as u16).filter(|&c| l.top + (c as usize) - 1 < l.rows.len()).collect() } else { Vec::new() };
    let hide: Vec<u16> = (1..=VISIBLE as u16).filter(|c| !shown.contains(c)).collect();
    let sel = (l.state == 1).then(|| (l.cursor - l.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, w.confirm.is_none(), &hide) else { return rects };
    let [fx, fy, fw, fh] = win.frame;
    let Some((x, y)) = gu.window_xy(LIST, sw, sh) else { return rects };
    rects.push(win.frame);
    let bt = w.battle.clone();
    match l.state {
        0 => {
            if let Some(bt) = &bt {
                gu.text(&bt.group9(0x61).unwrap_or_default(), x + 20.0, y + 20.0, 0, WHITE.to_le_bytes());
                gu.text(&bt.group9(0x62).unwrap_or_default(), x + 20.0, y + 40.0, 0, WHITE.to_le_bytes());
            }
        }
        1 => {
            if let Some(t) = gu.title_msg(journal::MISSION_TITLES[l.log]) {
                gu.text(&t, x + 60.0, y + 10.0, 0, WHITE.to_le_bytes());
            }
            for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=VISIBLE as u16).contains(id)) {
                let row = l.top + id as usize - 1;
                let Some(&r) = l.rows.get(row) else { continue };
                let tint = if row == l.cursor { CURSOR } else { WHITE };
                match r {
                    Row::CurHead | Row::DoneHead => {
                        let t = gu.lobby_msg(0xc, if r == Row::CurHead { 0x36 } else { 0x37 }, "");
                        gu.text(&t, cx - 15.0, cy, 0, WHITE.to_le_bytes());
                    }
                    Row::Msg(m) => {
                        let t = gu.lobby_msg(0xc, m, "");
                        gu.text(&t, cx + 3.0, cy, 0, tint.to_le_bytes());
                    }
                    Row::Current(i) | Row::Done(i) => {
                        let Some(e) = f.entries.get(i) else { continue };
                        let (name, n) = journal::indent(&e.name);
                        let n = if matches!(r, Row::Current(_)) { 0.0 } else { 6.0 * n as f32 };
                        gu.sprite("menu    framesus", 35, cx + 4.0 + n, cy + 5.0, WHITE);
                        lines(gu, name, cx + 18.0 + n, cy, 319.0 - 18.0 - n, tint);
                    }
                }
            }
            let (bx, total, top) = (fx + fw - 8.0, l.rows.len(), l.top);
            gu.scroll_bar(bx, fy, fh, total, VISIBLE, top);
            gu.bar_site(super::scrollbar::Owner::Journal, bx, fy, fh, total, VISIBLE, top);
        }
        _ => {
            let i = match l.rows.get(l.cursor) {
                Some(Row::Current(i) | Row::Done(i)) => *i,
                _ => return rects,
            };
            if let Some(e) = f.entries.get(i) {
                gu.sprite("menu    framesus", 35, x + 36.0, y + 25.0, WHITE);
                gu.text(journal::indent(&e.name).0, x + 50.0, y + 20.0, 0, WHITE.to_le_bytes());
                lines(gu, &e.desc, x + 40.0, y + 50.0, 327.0, WHITE);
            }
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(cursor: usize) -> List {
        // "Current:", one mission, "Completed:", two done.
        let rows = vec![Row::CurHead, Row::Current(0), Row::DoneHead, Row::Done(1), Row::Done(2)];
        List { log: 0, state: 1, rows, top: 0, cursor, dir: 0 }
    }

    /// FUN_1022e760: a fresh Up off row 1 skips the "Current:" header and wraps to the last row; held, it stops on
    /// row 1 instead and stays there.
    #[test]
    fn header_stops_a_held_key() {
        let mut l = list(1);
        assert!(step(&mut l, false, false));
        assert_eq!(l.cursor, 4);

        let mut l = list(1);
        assert!(!step(&mut l, false, true));
        assert_eq!((l.cursor, l.dir), (1, 2));
        assert!(!step(&mut l, false, true));
        assert_eq!(l.cursor, 1);

        // A held Down over the "Completed:" header still lands on the row under it.
        let mut l = list(1);
        assert!(step(&mut l, true, true));
        assert_eq!(l.cursor, 3);

        // At the bottom edge a held Down does not wrap once Down is remembered; a fresh press does.
        let mut l = list(4);
        l.dir = 1;
        assert!(!step(&mut l, true, true));
        assert_eq!(l.cursor, 4);
        assert!(step(&mut l, true, false));
        assert_eq!(l.cursor, 1);
    }
}
