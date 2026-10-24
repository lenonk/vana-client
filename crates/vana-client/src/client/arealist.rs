//! "menu    arealist" (DAT_106644c0; arealist_spec section 1): the area picker of searchma's Area, Monstrosity and
//! Unity rows and scsibori's Area / Unity. The list base (FUN_101f67c0) with 5 rows, no scroll bar and the generic
//! window sounds; regions fold out to their zones; registry group 0, so searchma stays drawn beside it.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::dat::names::NameSource;
use crate::net::search::Cond;

pub const WINDOW: &str = "menu    arealist";
/// FUN_101f6700(5); columns DAT_103859b4 from the row origin (+3, +4).
const ROWS: usize = 5;
const COLS: [f32; 3] = [20.0, 245.0, 320.0];
const WHITE: u32 = 0x8080_8080;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// 0: the search.
    Search,
    /// 3: status 0x300000, the summary line only; the window stays.
    Monstrosity,
    /// 4: the twelve unity rows (FUN_1020f290).
    Unity,
}

/// Column 1: "(%s)" (type 0x21), "[%d]" (type 9), a level text (type 0x34).
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Paren(String),
    Count(usize),
    Level(String),
}

/// A row: its value (a zone, 0 all areas, -1 the current region, -2 - i region i; mode 4 the unity index), the name
/// (type 1), column 1 and its help (group 0xe).
#[derive(Clone, Debug)]
pub struct Row {
    pub value: i32,
    pub name: String,
    pub cell: Cell,
    pub help: usize,
}

#[derive(Clone, Debug)]
pub struct AreaList {
    pub mode: Mode,
    /// The condition the picks change (W+0x5c as the opener left it).
    cond: Cond,
    /// +0x58: the region folded out, and the absolute row to come back to (+0x5a).
    expanded: Option<(usize, usize)>,
    pub rows: Vec<Row>,
    pub top: usize,
    pub cursor: usize,
}

/// FUN_100fc6e0: the zone's s2c 0x008 bit.
fn known(w: &World, z: u16) -> bool {
    z < 0x180 && w.known_zones.as_ref().is_some_and(|k| k[z as usize >> 3] >> (z & 7) & 1 != 0)
}

fn place(w: &World, z: u16) -> Option<String> {
    w.names.as_ref().and_then(|n| n.place(z as u32)).filter(|s| !s.is_empty())
}

/// FUN_1020a680: the region holding the current zone.
fn region_index(w: &World) -> Option<usize> {
    w.search_words.as_ref()?.regions.iter().position(|r| r.contains(&w.pos_zone))
}

/// FUN_1020ebf0 (modes 0 / 3) or FUN_1020f290 (mode 4).
fn build(w: &World, gu: &GameUi, mode: Mode, expanded: Option<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(words) = w.search_words.as_ref() else { return rows };
    if mode == Mode::Unity {
        for i in 0..12 {
            let name = gu.lobby_msg(0xc, if i == 0 { 0x1ba } else { 0x192 + i }, "");
            let short = words.unity_names.iter().find(|u| u.0 == (i as i32) << 22).map_or(String::new(), |u| u.1.clone());
            rows.push(Row { value: i as i32, name, cell: Cell::Paren(short), help: 0x37c });
        }
        return rows;
    }
    let level = |z: u16| words.levels.iter().find(|l| l.0 == z).copied();
    // FUN_1020e890 / FUN_1020ea30's filter: known, a group 0x14 name, zone flags 1 and 2; Monstrosity: a max level.
    let pass = |z: u16| known(w, z) && place(w, z).is_some() && words.zone_flags(z) & 3 == 3 && (mode != Mode::Monstrosity || level(z).is_some_and(|l| l.3 != 0));
    let zone_row = |z: u16| {
        let cell = match mode {
            Mode::Monstrosity => Cell::Level(level(z).map_or(String::new(), |l| format!("~Lv{:2}", l.3))),
            _ => Cell::Paren(gu.short_area(z).unwrap_or_else(|| gu.item_msg(9, 0x40).unwrap_or_default())),
        };
        Row { value: z as i32, name: place(w, z).unwrap_or_default(), cell, help: 0x14 }
    };
    let zones = |i: usize| words.regions.get(i).map_or(Vec::new(), |r| r.iter().copied().filter(|&z| pass(z)).collect::<Vec<u16>>());
    if let Some(i) = expanded {
        return zones(i).into_iter().map(zone_row).collect();
    }
    // FUN_1020e570: Current Area, Current Region (a region with more than one searchable zone), All Areas.
    let msg = |id: usize| gu.item_msg(9, id).unwrap_or_default();
    rows.push(Row { value: w.pos_zone as i32, name: msg(0x23), cell: Cell::Paren(msg(0x24)), help: 0x14 });
    if let Some(r) = region_index(w).and_then(|i| words.regions.get(i))
        && r.iter().filter(|&&z| words.zone_flags(z) & 2 != 0).count() > 1
    {
        rows.push(Row { value: -1, name: msg(0xba), cell: Cell::Paren(msg(0xbb)), help: 0x62 });
    }
    rows.push(Row { value: 0, name: msg(0x25), cell: Cell::Paren(msg(0x26)), help: 0x15 });
    let groups = gu.region_groups();
    for i in 0..words.regions.len() {
        let z = zones(i);
        if mode == Mode::Search && w.arealist_by_region {
            if !z.is_empty() {
                let name = groups.get(i).and_then(|g| gu.item_msg(0x13, g.0 as usize)).unwrap_or_default();
                rows.push(Row { value: -2 - i as i32, name, cell: Cell::Count(z.len()), help: 0x14 });
            }
        } else {
            rows.extend(z.into_iter().map(zone_row));
        }
    }
    rows
}

/// FUN_1020ee70 / FUN_1020f210 / FUN_1020f3a0: the window on child 1, top 0, picks applied to `cond`.
pub fn open(w: &mut World, gu: &GameUi, mode: Mode, cond: Cond) {
    let rows = build(w, gu, mode, None);
    w.arealist = Some(AreaList { mode, cond, expanded: None, rows, top: 0, cursor: 0 });
}

fn rebuild(w: &mut World, gu: &GameUi) {
    let Some(a) = w.arealist.as_ref() else { return };
    let rows = build(w, gu, a.mode, a.expanded.map(|e| e.0));
    if let Some(a) = w.arealist.as_mut() {
        a.rows = rows;
    }
}

/// FUN_1020eed0 and scsibori by name.
fn close_all(w: &mut World) {
    w.arealist = None;
    super::searchmenu::close_refine(w);
}

/// FUN_1020ef00 msg 5 (1.6 / 1.8): false when nothing could be picked (the list's SE 0004).
fn pick(w: &mut World, gu: &GameUi) -> bool {
    let Some(a) = w.arealist.clone() else { return false };
    let Some(r) = a.rows.get(a.cursor) else { return false };
    let mut c = a.cond.clone();
    match (a.mode, r.value) {
        (Mode::Unity, i) => {
            // FUN_1020f440: (i & 0x3f) << 22; row 0 is our own unity (none: 0x0f800000).
            let own = w.stats.as_ref().map_or(0, |s| s.unity & 0x1f);
            let bits = match i {
                0 if own != 0 => (own & 0x3f) << 22,
                0 => 0x0f80_0000,
                i => (i as u32 & 0x3f) << 22,
            };
            c.clear_status();
            c.set_status_if_none(bits);
        }
        (_, v) if v <= -2 => {
            let back = a.cursor;
            if let Some(l) = w.arealist.as_mut() {
                (l.expanded, l.top, l.cursor) = (Some(((-2 - v) as usize, back)), 0, 0);
            }
            rebuild(w, gu);
            w.sounds.push(Ui::Select as u32);
            return true;
        }
        (_, -1) => {
            let Some(zones) = region_index(w).and_then(|i| w.search_words.as_ref()?.regions.get(i).cloned()) else { return false };
            let flags = |z: u16| w.search_words.as_ref().map_or(0, |s| s.zone_flags(z));
            c.clear_area();
            for z in zones.into_iter().filter(|&z| flags(z) & 2 != 0) {
                c.add_area(z);
            }
        }
        (_, 0) => c.clear_area(),
        (_, z) => {
            c.clear_area();
            c.single_area(z as u16);
        }
    }
    // FUN_1020a9d0 returns at once while a query is out; the windows still close.
    let scope = super::search::scope_of(&c);
    match a.mode {
        Mode::Monstrosity => {
            c.clear_status();
            c.set_status_if_none(0x30_0000);
            if !super::search::busy(w) {
                super::search::run_quiet(w, c, scope);
            }
        }
        _ => {
            if !super::search::busy(w) {
                super::search::run(w, c, scope);
            }
            close_all(w);
        }
    }
    w.sounds.push(Ui::Select as u32);
    true
}

/// The generic handler then the list (1.5): Up / Down stop at the ends (0001 also when the cursor would go past the
/// last of fewer than 5 rows), Right / Left page by 5, Enter picks (0002, a failed pick 0004), Escape folds a region
/// back or closes (0003).
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut a) = w.arealist.clone() else { return };
    let n = a.rows.len();
    match key {
        egui::Key::ArrowDown => {
            if a.cursor + 1 < n {
                a.cursor += 1;
                w.sounds.push(Ui::Cursor as u32);
            } else if a.cursor - a.top + 1 < ROWS {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::ArrowUp if a.cursor > 0 => {
            a.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowRight | egui::Key::ArrowLeft => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if super::scrollbar::rows_abs(m, &mut a.top, &mut a.cursor, n, ROWS) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if !pick(w, gu) {
                w.sounds.push(Ui::Error as u32);
            }
            return;
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            match a.expanded {
                // FUN_101f7080(+0x5a, 0): the region row again.
                Some((_, back)) => {
                    a.expanded = None;
                    w.arealist = Some(a);
                    rebuild(w, gu);
                    if let Some(a) = w.arealist.as_mut() {
                        a.cursor = back.min(a.rows.len().saturating_sub(1));
                        a.top = a.cursor.min(a.rows.len().saturating_sub(ROWS));
                    }
                }
                None => w.arealist = None,
            }
            return;
        }
        _ => {}
    }
    a.top = a.top.min(a.cursor).max((a.cursor + 1).saturating_sub(ROWS));
    w.arealist = Some(a);
}

/// Message 8 (Num+): modes 3 / 4 nothing; else flat / by region (flat folds back), top 0, child 1, SE 0013. Kept for
/// the session only (config 0x45 is never read).
pub fn num_plus(w: &mut World, gu: &GameUi) {
    let Some(a) = w.arealist.as_mut().filter(|a| a.mode == Mode::Search) else { return };
    w.arealist_by_region = !w.arealist_by_region;
    if !w.arealist_by_region {
        a.expanded = None;
    }
    (a.top, a.cursor) = (0, 0);
    rebuild(w, gu);
    w.sounds.push(Ui::Close as u32);
}

/// Mouse *(inf: the generic child hit test)*: hover moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(a) = w.arealist.clone() else { return };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(WINDOW, sw, sh), gu.layout(WINDOW)) else { return };
    let Some(e) = lay.elements.iter().skip(1).find(|e| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    }) else { return };
    let row = a.top + e.id as usize - 1;
    if row >= a.rows.len() {
        return;
    }
    gu.hover.set(true);
    if row != a.cursor && (moved || clicked) {
        if let Some(a) = w.arealist.as_mut() {
            a.cursor = row;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// Slot 1 the layout header (help 0xe 0x14, title 0xd 0x26 "Search by Area"); help slot 2 the cursor row's help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(a) = &w.arealist else { return };
    let [help, title] = gu.focus_text(WINDOW, 0);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    bars.help[2] = a.rows.get(a.cursor).map_or(String::new(), |r| gu.menu_msg(r.help, ""));
}

/// FUN_101f6a60 with arealist's cells (1.4): rows from (+3, +4) 16 apart, names in the game font at y + 2, "[n]" in
/// the shape font right-aligned at the column's end - 2 (y + 4), "(s)" and the level texts in the shape font at y + 2;
/// the windowps 75 cursor bar at (+0, +4) while focused; nothing recoloured.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(a) = &w.arealist else { return rects };
    let focused = w.confirm.is_none();
    let sel = (!a.rows.is_empty()).then(|| (a.cursor - a.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), sel, focused, &[]) else { return rects };
    rects.push(win.frame);
    let Some((wx, wy)) = gu.window_xy(WINDOW, sw, sh) else { return rects };
    for (k, r) in a.rows.iter().enumerate().skip(a.top).take(ROWS) {
        let (rx, ry) = (wx + 3.0, wy + 4.0 + 16.0 * (k - a.top) as f32);
        gu.text(&r.name, rx + COLS[0], ry + 2.0, 0, WHITE.to_le_bytes());
        match &r.cell {
            Cell::Count(n) => {
                gu.shape_text(&format!("[{n}]"), rx + COLS[2] - 2.0, ry + 4.0, WHITE, true);
            }
            Cell::Paren(s) => {
                gu.shape_text(&format!("({s})"), rx + COLS[1], ry + 2.0, WHITE, false);
            }
            Cell::Level(s) => {
                gu.shape_text(s, rx + COLS[1], ry + 2.0, WHITE, false);
            }
        }
    }
    if focused && !a.rows.is_empty() {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (a.cursor - a.top) as f32, WHITE);
    }
    rects
}
