//! statcom2 row 7, Merit Points (spec ~/ffxi/lab/research/status_rows_spec.md 1): merit1 (Categories / Mode Switch)
//! with levmerit in place of level, merit2 (EXP / Limit Points), meritcat (the categories, then a category's merits)
//! with meritinf and bluehelp, merit3 (Raise / Lower) asking through sortyn and merityn.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::dat::merits::MeritTables;
use std::sync::Arc;

pub const MENU: &str = "menu    merit1";
const MODE: &str = "menu    merit2";
const EDIT: &str = "menu    merit3";
const LIST: &str = "menu    meritcat";
const LEV: &str = "menu    levmerit";
const INFO: &str = "menu    meritinf";
pub const HELP: &str = "menu    bluehelp";
pub const SORTYN: &str = "menu    sortyn";
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const BLUE: u32 = 0x8020_60a0;
const CURSOR: u32 = 0x8080_3040;
const ROWS: usize = 11;
/// DAT_10371858: the merit name colours by group (a job's second group yellow, a third green).
const GROUP: [u32; 3] = [0x8080_8080, 0x8090_9040, 0x8050_9050];
/// DAT_10371694: meritcat's cell columns.
const COLS: [f32; 8] = [0.0, 6.0, 16.0, 220.0, 240.0, 260.0, 320.0, 342.0];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    /// The category list: a position and its category id.
    Cat { pos: u16, id: u8 },
    /// A job's group header (windowps 592 / 593) with the group's cap.
    Header { group: u8, cat: u8 },
    Merit { index: u16, group: u8 },
}

struct List {
    /// +0x50: the category shown (0 = the category list).
    cat: u8,
    rows: Vec<Row>,
    top: usize,
    cursor: usize,
    /// The 0x08C stamp the rows were built from.
    stamp: u32,
    /// meritinf's position and category (the category list's last cursor row).
    info: Option<(u16, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub index: u16,
    pub raise: bool,
}

struct Edit {
    sel: u16,
    /// +0x14: 1 raising, 2 lowering; sortyn's cursor while it asks (true = Yes).
    doing: u8,
    sortyn: Option<bool>,
}

pub struct MeritUi {
    menu: u16,
    /// merit2's cursor and the mode its rows were last set for.
    mode: Option<u16>,
    list: Option<List>,
    edit: Option<Edit>,
}

/// The merit tables and names, loaded on the first open (FUN_10160e40).
pub(super) fn tables(w: &mut World) -> Option<Arc<MeritTables>> {
    if w.merit_tables.is_none() {
        let index = w.index.clone()?;
        w.merit_tables = Some(Arc::new(MeritTables::load(&index, w.point_files[0])));
    }
    w.merit_tables.clone()
}

/// s2c 0x08C: insert, replace or (id bit 0) delete each merit; a change stamps the list.
pub fn on_list(w: &mut World, recs: &[(u16, u8, u8)]) {
    for &(id, next, level) in recs {
        if id & 1 != 0 {
            w.merit_list.remove(&(id >> 1));
        } else if id != 0 {
            w.merit_list.insert(id >> 1, (next, level));
        }
    }
    w.merit_stamp = w.merit_stamp.wrapping_add(1);
}

/// statcom2 Enter on row 7 (FUN_10148d00 k 6): merit1 and levmerit.
pub fn open(w: &mut World) {
    tables(w);
    w.merits_ui = Some(MeritUi { menu: w.cursor_recall(MENU), mode: None, list: None, edit: None });
}

/// Merit points (0x063 type 2 @0xa & 0x7f) and the most (@0xc).
fn points(w: &World) -> Option<(u16, u8)> {
    w.merits.map(|(_, bits, max)| (bits & 0x7f, max))
}

fn main_job(w: &World) -> (u8, u8) {
    w.stats.as_ref().map_or((0, 0), |s| s.main)
}

/// FUN_10097d50: in Monstrosity.
fn monstrosity(w: &World) -> bool {
    w.my_plate.monstrosity != 0
}

/// A category's cap: file 89 @4; Weapon Skills +5 for each of key items 0xc98 / 0xc99, at most 100.
fn cap(w: &World, t: &MeritTables, cat: u8) -> u8 {
    let c = t.cat(cat).map_or(0, |c| c.cap);
    if cat != 0x1a {
        return c;
    }
    let bonus = 5 * (w.has_key_item(0xc98) as u8 + w.has_key_item(0xc99) as u8);
    (c + bonus).min(100)
}

/// FUN_10161010: the levels spent in category `cat`.
fn spent(w: &World, cat: u8) -> u32 {
    w.merit_list.iter().filter(|(i, _)| (**i >> 5) as u8 == cat).map(|(_, &(_, l))| l as u32).sum()
}

/// A merit row's colours (FUN_101491a0): name, level (None = none), cost (None = no cost cell).
fn colours(w: &World, t: &MeritTables, index: u16, group: u8) -> (u32, u32, Option<u32>) {
    let cat = (index >> 5) as u8;
    let (job, level) = main_job(w);
    let entry = t.entry(index).copied().unwrap_or_default();
    let ok = w.myroom.is_some() && entry.jobs >> job & 1 != 0 && (cat != 0x1a || (level >= 96 && w.has_key_item(0x80b)));
    let c = if ok { GROUP[group as usize % 3] } else { GREY };
    let Some(&(next, lv)) = w.merit_list.get(&index) else { return (c, c, None) };
    if lv >= entry.max {
        return (c, BLUE, None);
    }
    let affordable = ok && points(w).is_some_and(|(m, _)| next as u16 <= m) && spent(w, cat) < cap(w, t, cat) as u32;
    (c, c, Some(if affordable { WHITE } else { GREY }))
}

/// FUN_10149490 -> FUN_101491a0: the category list (positions 1..31 with a cap), or a category's merits; a job's two
/// groups under their headers.
fn rows(t: &MeritTables, cat: u8) -> Vec<Row> {
    let merits = |id: u8, group: u8, out: &mut Vec<Row>| {
        let base = id as u16 * 32;
        out.extend((base..base + 32).filter(|&i| t.entry(i).is_some()).map(|index| Row::Merit { index, group }));
    };
    let mut out = Vec::new();
    if cat == 0 {
        out.extend((1..32).filter_map(|p| t.at(p).filter(|c| c.cap != 0).map(|c| Row::Cat { pos: p, id: c.id })));
    } else if cat <= 5 || cat == 0x1a {
        merits(cat, 0, &mut out);
    } else {
        for (group, id) in [(0u8, cat), (1, cat + 0x1a)] {
            if t.cat(id).is_some() {
                out.push(Row::Header { group, cat: id });
                merits(id, group, &mut out);
            }
        }
    }
    out
}

/// FUN_10149040(cat): the category list on its remembered row, or a category from its top.
fn show(w: &mut World, cat: u8) {
    let Some(t) = w.merit_tables.clone() else { return };
    let rows = rows(&t, cat);
    let cursor = if cat == 0 { w.merit_mem.min(rows.len().saturating_sub(1)) } else { 0 };
    let stamp = w.merit_stamp;
    let Some(m) = w.merits_ui.as_mut() else { return };
    let info = m.list.as_ref().and_then(|l| l.info);
    let top = cursor.saturating_sub(ROWS - 1);
    m.list = Some(List { cat, rows, top, cursor, stamp, info });
}

/// Per frame (meritcat +0x50): a new 0x08C rebuilds the rows on the same cursor; meritinf follows the category list.
pub fn update(w: &mut World) {
    let Some(t) = w.merit_tables.clone() else { return };
    let stamp = w.merit_stamp;
    let Some(l) = w.merits_ui.as_mut().and_then(|m| m.list.as_mut()) else { return };
    if l.stamp != stamp {
        l.stamp = stamp;
        l.rows = rows(&t, l.cat);
        l.cursor = l.cursor.min(l.rows.len().saturating_sub(1));
    }
    if let Some(&Row::Cat { pos, id }) = l.rows.get(l.cursor) {
        l.info = Some((pos, id));
    }
}

/// FUN_101490c0 / FUN_10149130: the cursor merit can be raised (a record with a cost, its cost cell white) / lowered
/// (a level, its name not grey).
fn can(w: &World, index: u16, group: u8) -> (bool, bool) {
    let Some(t) = &w.merit_tables else { return (false, false) };
    let (c, _, cost) = colours(w, t, index, group);
    let rec = w.merit_list.get(&index).copied();
    (rec.is_some_and(|r| r.0 != 0) && cost == Some(WHITE), rec.is_some_and(|r| r.1 != 0) && c != GREY)
}

fn cursor_merit(w: &World) -> Option<(u16, u8)> {
    match w.merits_ui.as_ref()?.list.as_ref().and_then(|l| l.rows.get(l.cursor))? {
        Row::Merit { index, group } => Some((*index, *group)),
        _ => None,
    }
}

/// FUN_1021e670 over our effects: level restriction (0x8f) or level sync (0x101, 0x10d).
fn restricted(w: &World) -> bool {
    w.buffs.iter().any(|&b| matches!(b, 0x8f | 0x101 | 0x10d))
}

/// The 0x0BE packet: u8 @4 kind (2 mode, 3 edit), u8 @5 flag, u16 @6 the merit index, u32 @8 0.
fn send(w: &World, kind: u8, flag: u8, index: u16) {
    let mut body = vec![kind, flag];
    body.extend(index.to_le_bytes());
    body.extend([0; 4]);
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0BE, body });
}

/// merityn's Yes (LAB_10149c40 / LAB_10149d10): raise or lower by one; merit3 closes.
pub fn answer(w: &mut World, a: Ask) {
    send(w, 3, a.raise as u8, a.index);
    if let Some(m) = w.merits_ui.as_mut() {
        m.edit = None;
    }
}

/// merityn's question: group 10 0x50 "Use <n> merit point(s) to raise your "<name>" by one level?" / 0x51 "Lower your
/// "<name>" ...".
pub fn ask_text(w: &World, a: Ask) -> String {
    let (Some(bt), Some(t)) = (&w.battle, &w.merit_tables) else { return String::new() };
    let Some(raw) = bt.group10_raw(if a.raise { 0x50 } else { 0x51 }) else { return String::new() };
    let name = crate::dat::font::encode_sjis(&t.name(a.index));
    let cost = w.merit_list.get(&a.index).map_or(0, |r| r.0 as i32);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&name], &[cost], &|_, _, _| None))
}

fn links(gu: &GameUi, name: &str, sel: u16, up: bool) -> u16 {
    let to = gu.menu_links(name, sel).map_or(sel, |l| l[if up { 2 } else { 3 }] as u16);
    if to == 0 || to == 0xff { sel } else { to }
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(m) = w.merits_ui.as_ref() else { return };
    let (menu, mode, has_list, edit) = (m.menu, m.mode, m.list.is_some(), m.edit.as_ref().map(|e| (e.sel, e.sortyn)));
    // sortyn over merit3: Left / Right / Up / Down switch, Enter answers (Yes: merityn), Escape is No.
    if let Some((sel, Some(yes))) = edit {
        match key {
            K::ArrowLeft | K::ArrowRight | K::ArrowUp | K::ArrowDown => {
                if let Some(e) = w.merits_ui.as_mut().and_then(|m| m.edit.as_mut()) {
                    e.sortyn = Some(!yes);
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter | K::Escape => {
                let yes = key == K::Enter && yes;
                w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
                if let Some(e) = w.merits_ui.as_mut().and_then(|m| m.edit.as_mut()) {
                    e.sortyn = None;
                    if !yes {
                        e.doing = 0;
                    }
                }
                if yes && let Some((index, _)) = cursor_merit(w) {
                    w.confirm = Some((super::Confirm::Merit(Ask { index, raise: sel == 1 }), false));
                }
            }
            _ => {}
        }
        return;
    }
    // merit3: Raise / Lower.
    if let Some((sel, None)) = edit {
        let Some((index, group)) = cursor_merit(w) else {
            if let Some(m) = w.merits_ui.as_mut() {
                m.edit = None;
            }
            return;
        };
        let (raise, lower) = can(w, index, group);
        match key {
            K::ArrowUp | K::ArrowDown => {
                let to = links(gu, EDIT, sel, key == K::ArrowUp);
                if to != sel {
                    if let Some(e) = w.merits_ui.as_mut().and_then(|m| m.edit.as_mut()) {
                        e.sel = to;
                    }
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                if let Some(m) = w.merits_ui.as_mut() {
                    m.edit = None;
                }
            }
            K::Enter if (sel == 1 && !raise) || (sel == 2 && !lower) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                if let Some(e) = w.merits_ui.as_mut().and_then(|m| m.edit.as_mut()) {
                    (e.doing, e.sortyn) = (sel as u8, Some(false));
                }
            }
            _ => {}
        }
        return;
    }
    if has_list {
        list_key(w, key);
        return;
    }
    // merit2: EXP / Limit Points; the current mode's row is disabled.
    if let Some(sel) = mode {
        let off = mode_off(w);
        match key {
            K::ArrowUp | K::ArrowDown => {
                let to = links(gu, MODE, sel, key == K::ArrowUp);
                if to != sel {
                    if let Some(m) = w.merits_ui.as_mut() {
                        m.mode = Some(to);
                    }
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                if let Some(m) = w.merits_ui.as_mut() {
                    m.mode = None;
                }
            }
            K::Enter if sel == off => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                send(w, 2, (sel == 2) as u8, 0);
            }
            _ => {}
        }
        return;
    }
    // merit1.
    match key {
        K::ArrowUp | K::ArrowDown => {
            let to = links(gu, MENU, menu, key == K::ArrowUp);
            if to != menu {
                if let Some(m) = w.merits_ui.as_mut() {
                    m.menu = to;
                }
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.merits_ui = None;
        }
        K::Enter if menu == 2 && monstrosity(w) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            if menu == 1 {
                show(w, 0);
            } else if restricted(w) {
                super::textcmd::refuse(w, 0xbb);
            } else {
                let sel = w.cursor_recall(MODE);
                if let Some(m) = w.merits_ui.as_mut() {
                    m.mode = Some(sel);
                }
            }
        }
        _ => {}
    }
}

/// merit2's disabled row: 2 (Limit Points) in merit mode (0x063 type 2 bit 15), else 1 (EXP).
fn mode_off(w: &World) -> u16 {
    if w.merits.is_some_and(|m| m.1 & 0x8000 != 0) { 2 } else { 1 }
}

/// The list base's keys (FUN_101f6c70) and meritcat's hook (FUN_10149750).
fn list_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(l) = w.merits_ui.as_ref().and_then(|m| m.list.as_ref()) else { return };
    let (n, cat, cursor) = (l.rows.len(), l.cat, l.cursor);
    let row = l.rows.get(cursor).copied();
    match key {
        // The base's page (+0x47 set): the top by a page, the screen row kept.
        K::ArrowLeft | K::ArrowRight => {
            let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if let Some(l) = w.merits_ui.as_mut().and_then(|m| m.list.as_mut())
                && super::scrollbar::rows_abs(m, &mut l.top, &mut l.cursor, n, ROWS)
            {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowUp | K::ArrowDown => {
            let to = if key == K::ArrowDown { (cursor + 1).min(n.saturating_sub(1)) } else { cursor.saturating_sub(1) };
            if to != cursor && let Some(l) = w.merits_ui.as_mut().and_then(|m| m.list.as_mut()) {
                l.cursor = to;
                l.top = l.top.min(to).max((to + 1).saturating_sub(ROWS));
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            if cat != 0 {
                show(w, 0);
            } else if let Some(m) = w.merits_ui.as_mut() {
                w.merit_mem = cursor;
                m.list = None;
            }
        }
        K::Enter => match row {
            Some(Row::Header { .. }) | None => {}
            Some(Row::Cat { id, .. }) => {
                w.sounds.push(Ui::Select as u32);
                w.merit_mem = cursor;
                show(w, id);
            }
            Some(Row::Merit { index, group }) => {
                if w.myroom.is_none() {
                    super::textcmd::refuse(w, 0xb2);
                    w.sounds.push(Ui::Error as u32);
                    w.sounds.push(Ui::Error as u32);
                    return;
                }
                let (raise, lower) = can(w, index, group);
                if !raise && !lower {
                    w.sounds.push(Ui::Error as u32);
                    w.sounds.push(Ui::Error as u32);
                    return;
                }
                w.sounds.push(Ui::Select as u32);
                if let Some(m) = w.merits_ui.as_mut() {
                    m.edit = Some(Edit { sel: if raise { 1 } else { 2 }, doing: 0, sortyn: None });
                }
            }
        },
        _ => {}
    }
}

/// Mouse: hover moves the cursor over the focused window's rows, a click is Enter, a right click Escape *(inf: the
/// generic window and list events)*.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(m) = w.merits_ui.as_ref() else { return };
    if w.confirm.is_some() || m.edit.as_ref().is_some_and(|e| e.sortyn.is_some()) {
        return;
    }
    let focus = if m.edit.is_some() { EDIT } else if m.list.is_some() { LIST } else if m.mode.is_some() { MODE } else { MENU };
    let inside = |r: [f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    if right {
        if gu.window_rect(focus, sw, sh).is_some_and(inside) {
            key(w, gu, egui::Key::Escape);
        }
        return;
    }
    let Some((wx, wy)) = gu.window_xy(focus, sw, sh) else { return };
    let Some(layout) = gu.layout(focus) else { return };
    let hit = layout.elements.iter().skip(1).find(|e| inside([wx + e.x as f32, wy + e.y as f32, e.w as f32, e.h as f32])).map(|e| e.id);
    let Some(id) = hit else { return };
    gu.hover.set(true);
    let cur = match focus {
        EDIT => m.edit.as_ref().map_or(0, |e| e.sel),
        LIST => m.list.as_ref().map_or(0, |l| (l.cursor - l.top + 1) as u16),
        MODE => m.mode.unwrap_or(0),
        _ => m.menu,
    };
    if focus == LIST && m.list.as_ref().is_some_and(|l| l.top + id as usize > l.rows.len()) {
        return;
    }
    if (moved || clicked) && id != cur {
        if let Some(m) = w.merits_ui.as_mut() {
            match focus {
                EDIT => m.edit.iter_mut().for_each(|e| e.sel = id),
                LIST => m.list.iter_mut().for_each(|l| l.cursor = l.top + id as usize - 1),
                MODE => m.mode = Some(id),
                _ => m.menu = id,
            }
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The list's bar (FUN_101f6c70) while the list has the focus.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(l) = w.merits_ui.as_mut().filter(|m| m.edit.is_none()).and_then(|m| m.list.as_mut()) else { return false };
    super::scrollbar::rows_abs(m, &mut l.top, &mut l.cursor, l.rows.len(), ROWS)
}

/// Help and title. merit1 / merit2 / merit3: their rows' strings; meritcat 0xe 0x261 and "Categories" (0xd 0x82);
/// sortyn: 0xe 0x264 / 0x265 and "Categories".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(m) = &w.merits_ui else { return };
    let categories = gu.title_msg(0x82).unwrap_or_default();
    if let Some(e) = &m.edit {
        if e.sortyn.is_some() {
            bars.help[1] = gu.menu_msg(if e.sel == 1 { 0x264 } else { 0x265 }, "");
            bars.title[2] = categories;
        } else {
            bars.help[1] = gu.focus_text(EDIT, e.sel)[0].clone().unwrap_or_default();
        }
    } else if m.list.is_some() {
        bars.help[1] = gu.menu_msg(0x261, "");
        bars.help[2] = String::new();
        bars.title[2] = categories;
    } else if let Some(sel) = m.mode {
        bars.help[1] = gu.focus_text(MODE, sel)[0].clone().unwrap_or_default();
    } else {
        bars.help[1] = gu.focus_text(MENU, m.menu)[0].clone().unwrap_or_default();
    }
}

fn right(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) -> f32 {
    gu.shape_text(s, x, y, tint, true)
}

/// bluehelp (FUN_101ec500): the name at (4, 4), the description from (20, 20) wrapped to w - 24, 16 px lines; the
/// frame 16 per line + 24 high, at least the layout's 56, growing down.
pub fn bluehelp(gu: &mut GameUi, sw: f32, sh: f32, name: &str, desc: &str) -> Option<[f32; 4]> {
    let [x, y, fw, fh] = gu.window_rect(HELP, sw, sh)?;
    let lines = if desc.is_empty() { Vec::new() } else { desc.split('\n').flat_map(|l| gu.wrap(l, fw - 24.0)).collect() };
    let h = (16.0 * lines.len() as f32 + 24.0).max(fh);
    if !gu.window_sized(HELP, x, y, fw, h) {
        return None;
    }
    if !name.is_empty() {
        gu.text(name, x + 4.0, y + 4.0, 0, WHITE.to_le_bytes());
    }
    for (i, l) in lines.iter().enumerate() {
        gu.text(l, x + 20.0, y + 20.0 + 16.0 * i as f32, 0, WHITE.to_le_bytes());
    }
    Some([x, y, fw, h])
}

/// A plain Yes / No window (sortyn) with the cursor on Yes or No.
pub fn sortyn(gu: &mut GameUi, sw: f32, sh: f32, yes: bool) -> Option<[f32; 4]> {
    gu.lobby_window_off(SORTYN, sw, sh, (0.0, 0.0), Some(if yes { 1 } else { 2 }), true, &[]).map(|w| w.frame)
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(m) = &w.merits_ui else { return rects };
    let Some(t) = w.merit_tables.clone() else { return rects };
    let free = w.confirm.is_none();
    // levmerit (FUN_10149f20): limit points / 10000 at (x1 - 8, 0x1c), merit points / most at (x1 - 8, 0x44).
    if let Some(win) = gu.lobby_window_off(LEV, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, fw, _] = win.frame;
        let x1 = x + fw - 8.0;
        match w.merits {
            Some((limit, ..)) => right(gu, &format!("{limit}/10000"), x1, y + 28.0, WHITE),
            None => right(gu, "----/10000", x1, y + 28.0, WHITE),
        };
        if let Some((n, max)) = points(w) {
            right(gu, &format!("{n}/{max} P"), x1, y + 68.0, if n >= max as u16 { BLUE } else { WHITE });
        }
        rects.push(win.frame);
    }
    // merit3 in merit1's place, or merit2, or merit1.
    if let Some(e) = &m.edit {
        let (raise, lower) = cursor_merit(w).map_or((false, false), |(i, g)| can(w, i, g));
        let off: Vec<u16> = [(1u16, raise), (2, lower)].into_iter().filter(|c| !c.1).map(|c| c.0).collect();
        if let Some(win) = gu.lobby_window_off(EDIT, sw, sh, (0.0, 0.0), Some(e.sel), free && e.sortyn.is_none(), &[]) {
            gu.disabled_children(EDIT, win.frame[0], win.frame[1], &off, Some(e.sel));
            rects.push(win.frame);
        }
        if let Some(yes) = e.sortyn {
            rects.extend(sortyn(gu, sw, sh, yes));
        }
    } else if let Some(sel) = m.mode {
        let off = [mode_off(w)];
        if let Some(win) = gu.lobby_window_off(MODE, sw, sh, (0.0, 0.0), Some(sel), free, &[]) {
            gu.disabled_children(MODE, win.frame[0], win.frame[1], &off, Some(sel));
            rects.push(win.frame);
        }
    } else {
        let off: Vec<u16> = if monstrosity(w) { vec![2] } else { Vec::new() };
        if let Some(win) = gu.lobby_window_off(MENU, sw, sh, (0.0, 0.0), Some(m.menu), free && m.list.is_none(), &[]) {
            gu.disabled_children(MENU, win.frame[0], win.frame[1], &off, Some(m.menu));
            rects.push(win.frame);
        }
    }
    let Some(l) = &m.list else { return rects };
    let focused = free && m.edit.is_none();
    let hide: Vec<u16> = (1..=ROWS as u16).filter(|&k| l.top + k as usize > l.rows.len()).collect();
    let sel = (!l.rows.is_empty()).then(|| (l.cursor - l.top + 1) as u16);
    if let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, focused, &hide) {
        let [x0, y0, fw, fh] = win.frame;
        for (i, row) in l.rows.iter().enumerate().skip(l.top).take(ROWS) {
            let y = y0 + 4.0 + 16.0 * (i - l.top) as f32;
            let on = i == l.cursor;
            match *row {
                Row::Cat { pos, .. } => {
                    let name = t.at(pos).map_or(String::new(), |c| t.name(c.name));
                    gu.text(&name, x0 + COLS[1], y + 2.0, 0, if on { CURSOR } else { WHITE }.to_le_bytes());
                }
                Row::Header { group, cat } => {
                    gu.sprite("menu    windowps", if group == 0 { 592 } else { 593 }, x0 + COLS[0], y + 2.0, WHITE);
                    right(gu, &cap(w, &t, cat).to_string(), x0 + COLS[7] - 2.0, y + 4.0, WHITE);
                }
                Row::Merit { index, group } => {
                    let (c, lc, cost) = colours(w, &t, index, group);
                    gu.text(&t.name(index), x0 + COLS[1], y + 2.0, 0, if on { CURSOR } else { c }.to_le_bytes());
                    let (next, level) = w.merit_list.get(&index).copied().unwrap_or((0, 0));
                    right(gu, &level.to_string(), x0 + COLS[4] - 2.0, y + 4.0, lc);
                    if let Some(cc) = cost.filter(|_| next != 0) {
                        gu.shape_text(&gu.lobby_msg(0xc, 0x5a, "NEXT"), x0 + COLS[5], y + 4.0, cc, false);
                        right(gu, &format!("{next} P"), x0 + COLS[6] - 2.0, y + 4.0, cc);
                    }
                }
            }
        }
        if l.rows.len() > ROWS {
            gu.scroll_bar(x0 + fw - 8.0, y0, fh, l.rows.len(), ROWS, l.top);
            if focused {
                gu.bar_site(super::scrollbar::Owner::Points, x0 + fw - 8.0, y0, fh, l.rows.len(), ROWS, l.top);
            }
        }
        rects.push(win.frame);
    }
    // meritinf (FUN_10149e60): merit points at (x1 - 8, 0xe); the category's spent / cap at (x1 - 8, 0x28), a job's
    // second group at x1 - 0x40 in yellow.
    if let Some(win) = gu.lobby_window_off(INFO, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, fw, _] = win.frame;
        let x1 = x + fw - 8.0;
        let n = points(w).map_or("- P".to_string(), |(n, _)| format!("{n} P"));
        right(gu, &n, x1, y + 14.0, WHITE);
        if let Some((_, id)) = l.info {
            let total = |gu: &mut GameUi, cat: u8, x: f32, tint: u32| {
                let (s, c) = (spent(w, cat), cap(w, &t, cat) as u32);
                let x = right(gu, &format!("/{c}"), x, y + 40.0, tint);
                right(gu, &s.to_string(), x, y + 40.0, if s >= c { BLUE } else { tint });
            };
            total(gu, id, x1, WHITE);
            if id >= 6 && id != 0x1a {
                total(gu, id + 0x1a, x + fw - 64.0, 0x8090_9040);
            }
        }
        rects.push(win.frame);
    }
    // bluehelp: the cursor row's name and description.
    let id = match l.rows.get(l.cursor) {
        Some(Row::Cat { pos, .. }) => t.at(*pos).map(|c| c.name),
        Some(Row::Merit { index, .. }) => Some(*index),
        _ => None,
    };
    let (name, desc) = id.map_or((String::new(), String::new()), |i| (t.name(i), t.desc(i)));
    rects.extend(bluehelp(gu, sw, sh, &name, &desc));
    rects
}
