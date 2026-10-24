//! The main menu's Search row (FUN_10140400 case 8; menu_rows2_spec part S): "menu    searchma" in the main menu's
//! place and the criterion windows in its place (all group 0x81: the walk shows only the newest). Each pick searches
//! the current zone with that one criterion (FUN_1020d310 resets the condition whenever searchma has the focus) and
//! hands over to the results window (client/search.rs).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::net::search::{COMMENT, Cond, JOB, LEVEL, LS1, LS2, MLEVEL, NAME, NATION, RACE, RANK, STATUS};

pub const MENU: &str = "menu    searchma";
const JOBS: &str = "menu    searchjo";
const NATIONS: &str = "menu    searchna";
const RACES: &str = "menu    searchra";
const RANKS: &str = "menu    searchle";
const LEVELS: &str = "menu    scchar";
const MLEVELS: &str = "menu    scmlev";
const NAMES: &str = "menu    scname";
const GENRES: &str = "menu    comgenre";

#[derive(Clone, Debug)]
pub struct Win {
    pub name: &'static str,
    pub sel: u16,
    /// scchar / scmlev: the field being edited (+0x14 / +0x16), the two values (+0x18 / +0x1a); scname: the box open.
    pub edit: Option<u16>,
    pub vals: (u8, u8),
}

impl Win {
    fn new(name: &'static str, sel: u16) -> Win {
        Win { name, sel, edit: None, vals: (1, 1) }
    }
}

#[derive(Clone, Debug)]
pub struct SearchMenu {
    /// searchma, then a criterion window over it.
    pub stack: Vec<Win>,
    /// searchma's disabled rows (FUN_1020d250).
    off: Vec<u16>,
    return_menu: Option<usize>,
}

/// An equipped linkshell's id (FUN_100e9730 / FUN_10107f40): extdata u32 @0 of the shell at its slot.
fn ls_id(w: &World, n: usize) -> u32 {
    w.linkshells.get(n).filter(|l| l.0 != 0).and_then(|&(index, bag)| w.inv.ext.get(&(bag, index))).map_or(0, |e| u32::from_le_bytes([e[0], e[1], e[2], e[3]]))
}

/// FUN_10140400 case 8: searchma (SE 0002) in the main menu's place. Disabled: Friend (no POL friend list), Linkshell /
/// Linkshell 2 without one equipped, Ballista (S+0x40dac bit 15; LSB never sets it).
pub fn open(w: &mut World) {
    let mut off = vec![8, 10];
    if ls_id(w, 1) == 0 {
        off.push(9);
    }
    if ls_id(w, 2) == 0 {
        off.push(15);
    }
    let sel = w.cursor_recall(MENU);
    w.searchmenu = Some(SearchMenu { stack: vec![Win::new(MENU, sel)], off, return_menu: w.menu });
    w.menu = None;
}

fn disabled<'a>(m: &'a SearchMenu, name: &str) -> &'a [u16] {
    if name == MENU { &m.off } else { &[] }
}

fn close_top(w: &mut World) {
    let Some(m) = w.searchmenu.as_mut() else { return };
    m.stack.pop();
    if m.stack.is_empty() {
        let back = m.return_menu;
        w.searchmenu = None;
        w.menu = back;
    }
}

fn push(w: &mut World, name: &'static str) {
    let mut win = Win::new(name, w.cursor_recall(name));
    // scchar / scmlev ask for fresh stats when shown (c2s 0x061); the reply sets both values to our main level (or the
    // master level, 0 without one).
    if matches!(name, LEVELS | MLEVELS) {
        let _ = w.net.commands.send(super::net::NetCommand::StatusRequest(0));
        let st = w.stats.as_ref();
        let v = if name == LEVELS { st.map_or(1, |s| s.main.1) } else { st.filter(|s| s.ml.1 & 1 != 0).map_or(0, |s| s.ml.0) };
        win.vals = (v, v);
    }
    if let Some(m) = w.searchmenu.as_mut() {
        m.stack.push(win);
    }
}

/// FUN_1020db80 / FUN_1020dec0: a field steps within 1..99 (master level 0..99); raising the minimum pushes the maximum
/// up, lowering the maximum pushes the minimum down.
fn step_field(vals: &mut (u8, u8), field: u16, up: bool, low: u8) {
    let v = if field == 1 { &mut vals.0 } else { &mut vals.1 };
    *v = if up { (*v + 1).min(99) } else { v.saturating_sub(1).max(low) };
    if field == 1 && vals.0 > vals.1 {
        vals.1 = vals.0;
    }
    if field == 2 && vals.1 < vals.0 {
        vals.0 = vals.1;
    }
}

/// "run" (FUN_1020a9d0(1, cond, 1, 0, 0, p7)): the current zone and this criterion; then the criterion window closes.
fn run(w: &mut World, set: impl FnOnce(&mut Cond)) {
    let mut c = Cond::zone(w.pos_zone);
    set(&mut c);
    super::search::run(w, c, super::search::Scope::Area);
    if let Some(m) = w.searchmenu.as_mut()
        && m.stack.len() > 1
    {
        m.stack.truncate(1);
    }
}

/// FUN_1020d340 / the criterion windows' Enter (FUN_1020d670, 6e0, 750, 5e0).
fn enter(w: &mut World, name: &'static str, sel: u16) {
    match (name, sel) {
        (MENU, 3) => push(w, JOBS),
        (MENU, 4) => push(w, NATIONS),
        (MENU, 5) => push(w, RACES),
        (MENU, 7) => push(w, RANKS),
        (MENU, 6) => push(w, LEVELS),
        (MENU, 17) => push(w, MLEVELS),
        (MENU, 2) => push(w, NAMES),
        (MENU, 11) => push(w, GENRES),
        // scchar / scmlev: OK runs with the pair, Cancel closes.
        (LEVELS | MLEVELS, 3) => {
            let vals = w.searchmenu.as_ref().and_then(|m| m.stack.last()).map_or((1, 1), |t| t.vals);
            let flag = if name == LEVELS { LEVEL } else { MLEVEL };
            run(w, |c| (c.flags, c.level) = (c.flags | flag, vals));
        }
        (LEVELS | MLEVELS | NAMES, 4) | (NAMES, 3) => close_top(w),
        (NAMES, 2) => {
            let n = w.search_name.clone();
            if !n.is_empty() {
                run(w, |c| (c.flags, c.name) = (c.flags | NAME, n));
            }
        }
        // comgenre mode 1 (a comment search): genre << 4.
        (GENRES, g) => run(w, |c| (c.flags, c.comment) = (c.flags | COMMENT, (g as u32) << 4)),
        // Request: status 0x10000000.
        (MENU, 16) => run(w, |c| (c.flags, c.status) = (c.flags | STATUS, 0x1000_0000)),
        (MENU, 9) | (MENU, 15) => {
            let (n, flag) = if sel == 9 { (1, LS1) } else { (2, LS2) };
            let id = ls_id(w, n);
            run(w, |c| {
                c.flags |= flag;
                c.ls[n - 1] = id;
            });
        }
        (JOBS, j) => run(w, |c| (c.flags, c.job) = (c.flags | JOB, j as u8)),
        (NATIONS, n) => run(w, |c| (c.flags, c.nation) = (c.flags | NATION, n as u8 - 1)),
        // Children 1..5: Hume, Elvaan, Tarutaru, Mithra, Galka (DAT_10385a0f -> FUN_10107d50).
        (RACES, r) => run(w, |c| (c.flags, c.race) = (c.flags | RACE, r as u8 - 1)),
        (RANKS, r) => run(w, |c| (c.flags, c.rank) = (c.flags | RANK, (r as u8, r as u8))),
        _ => log::info!("search menu: {} row {sel} is not implemented yet", name.trim_start_matches("menu    ")),
    }
}

/// The generic window events: links (SE 0001), Enter (SE 0002; a disabled row or a query still out SE 0004), Escape
/// (SE 0003) closes the top window; searchma's close brings the main menu back.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(m) = w.searchmenu.clone() else { return };
    let Some(top) = m.stack.last().cloned() else { return };
    // An edited number field (FUN_1020dd50): Up / Down step it, Left / Right edit the other one, Enter / Escape stop.
    if let (LEVELS | MLEVELS, Some(f)) = (top.name, top.edit) {
        let low = if top.name == LEVELS { 1 } else { 0 };
        let Some(t) = w.searchmenu.as_mut().and_then(|m| m.stack.last_mut()) else { return };
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                step_field(&mut t.vals, f, key == egui::Key::ArrowUp, low);
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                let other = if f == 1 { 2 } else { 1 };
                (t.edit, t.sel) = (Some(other), other);
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Enter | egui::Key::Escape => {
                t.edit = None;
                w.sounds.push(if key == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
            }
            _ => {}
        }
        return;
    }
    // Enter on a number field or the name field starts editing it.
    if key == egui::Key::Enter && matches!((top.name, top.sel), (LEVELS | MLEVELS, 1 | 2) | (NAMES, 1)) {
        w.sounds.push(Ui::Select as u32);
        if let Some(t) = w.searchmenu.as_mut().and_then(|m| m.stack.last_mut()) {
            t.edit = Some(t.sel);
        }
        return;
    }
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            let k = match key {
                egui::Key::ArrowUp => 2,
                egui::Key::ArrowDown => 3,
                egui::Key::ArrowRight => 4,
                _ => 5,
            };
            let to = gu.menu_links(top.name, top.sel).map_or(top.sel, |l| l[k] as u16);
            if to != top.sel && to != 0 && to != 0xff
                && let Some(t) = w.searchmenu.as_mut().and_then(|m| m.stack.last_mut())
            {
                t.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close_top(w);
        }
        egui::Key::Enter if disabled(&m, top.name).contains(&top.sel) => w.sounds.push(Ui::Error as u32),
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if w.search.as_ref().is_some_and(|r| r.busy) {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            enter(w, top.name, top.sel);
        }
        _ => {}
    }
}

/// scname's box takes the typed text (15 characters; FUN_101c5be0(1, 15, ..)).
pub fn typing(w: &World) -> bool {
    w.searchmenu.as_ref().and_then(|m| m.stack.last()).is_some_and(|t| t.name == NAMES && t.edit.is_some())
}

/// The box's keys: text, Backspace; Enter commits, Escape closes it keeping the text (FUN_1020e290(1)).
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    for ev in events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.chars().filter(|c| (' '..='~').contains(c)) {
                    if w.search_name.chars().count() < 15 {
                        w.search_name.push(c);
                    }
                }
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                w.search_name.pop();
            }
            egui::Event::Key { key: k @ (egui::Key::Enter | egui::Key::Escape), pressed: true, .. } => {
                if let Some(t) = w.searchmenu.as_mut().and_then(|m| m.stack.last_mut()) {
                    t.edit = None;
                }
                w.sounds.push(if *k == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
            }
            _ => {}
        }
    }
}

/// The top window takes the keys unless the results window is up over it.
pub fn focused(w: &World) -> bool {
    w.searchmenu.is_some() && w.search.is_none() && w.confirm.is_none()
}

/// Mouse: hover moves the cursor, a click on the cursor row is Enter *(inf: the generic child hit test)*.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(top) = w.searchmenu.as_ref().and_then(|m| m.stack.last()).cloned() else { return };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(top.name, sw, sh), gu.layout(top.name)) else { return };
    let Some(e) = lay.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).find(|e| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    }) else { return };
    gu.hover.set(true);
    if e.id != top.sel && (moved || clicked) {
        if let Some(t) = w.searchmenu.as_mut().and_then(|m| m.stack.last_mut()) {
            t.sel = e.id;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// Help / title from the cursor row (slot 1); searchma puts 0xd 0 "Search" in title slot 2 while open.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(top) = w.searchmenu.as_ref().and_then(|m| m.stack.last()) else { return };
    let [help, title] = gu.focus_text(top.name, top.sel);
    if w.search.is_none() {
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    bars.title[2] = gu.title_msg(0).unwrap_or_default();
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(m) = &w.searchmenu else { return rects };
    let n = m.stack.len();
    for (k, win) in m.stack.iter().enumerate() {
        let focused = k + 1 == n && focused(w);
        let off = disabled(m, win.name);
        if let Some(lw) = gu.lobby_window_off(win.name, sw, sh, (0.0, 0.0), Some(win.sel), focused, off) {
            if let Some((x, y)) = gu.window_xy(win.name, sw, sh) {
                gu.disabled_children(win.name, x, y, off, Some(win.sel).filter(|_| focused));
            }
            let child = |id: u16| lw.children.iter().find(|c| c.0 == id).map(|c| c.1);
            match win.name {
                // FUN_1020dc90: each value "%d" right-aligned at (child right - 2, top + 2), 0x80a06040 while edited.
                LEVELS | MLEVELS => {
                    for (f, v) in [(1u16, win.vals.0), (2, win.vals.1)] {
                        if let Some([cx, cy, cw, _]) = child(f) {
                            let tint = if win.edit == Some(f) { 0x80a0_6040 } else { 0x8080_8080 };
                            gu.shape_text(&v.to_string(), cx + cw - 2.0, cy + 2.0, tint, true);
                        }
                    }
                }
                // FUN_1020e310: the live box with its caret, or the stored name in the shape font, at (x + 2, y + 2).
                NAMES => {
                    if let Some([cx, cy, ..]) = child(1) {
                        if win.edit.is_some() {
                            let caret = if (w.started.elapsed().as_secs_f32() * 60.0) as u32 % 30 < 20 { "|" } else { "" };
                            gu.text(&format!("{}{caret}", w.search_name), cx + 2.0, cy + 2.0, 0, [0x80; 4]);
                        } else {
                            gu.shape_text(&w.search_name, cx + 2.0, cy + 2.0, 0x8080_8080, false);
                        }
                    }
                }
                _ => {}
            }
            rects.push(lw.frame);
        }
    }
    rects
}
