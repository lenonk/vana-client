//! The modern Command Menu (the main menu, "-"): the retail menu's rows, organised in categories with the pack's
//! icons; a category opens its rows in a submenu beside it. Arrows and Enter or the mouse; a pick runs the retail row
//! (client/mod.rs's menu dispatch) through `World::menu_click`.

use super::super::{World, menu_disabled_on, monstrosity};
use super::{Panels, Title};
use crate::audio::Ui as Se;

/// A row: its label, the pack's icon, and the retail row it runs (page, child) or the submenu it opens.
pub struct Entry {
    label: &'static str,
    icon: &'static str,
    kind: Kind,
}

enum Kind {
    Row(u8, u16),
    Sub(&'static [Entry]),
    /// Ours: the Journal on its Records of Eminence tab (retail reaches them through Quests).
    Roe,
}

const fn row(label: &'static str, icon: &'static str, page: u8, id: u16) -> Entry {
    Entry { label, icon, kind: Kind::Row(page, id) }
}

const SOCIAL: &[Entry] = &[
    row("Party", "icons/party", 0, 6),
    row("Trade", "icons/trade", 0, 7),
    row("Linkshell", "icons/linkshell", 0, 9),
    row("Search", "icons/find", 0, 8),
    row("Communication", "icons/emote", 1, 9),
    row("View House", "icons/mog_house", 1, 4),
    row("Set Bazaar", "icons/currency", 1, 5),
];
const JOURNAL: &[Entry] = &[
    row("Missions", "icons/quest", 1, 1),
    row("Quests", "icons/quest_log", 1, 2),
    Entry { label: "Records of Eminence", icon: "icons/quest_log", kind: Kind::Roe },
    row("Key Items", "icons/key", 1, 3),
];
const WORLD: &[Entry] = &[row("Map", "icons/map", 0, 12), row("Region Info", "icons/find", 0, 11), row("Current Time", "icons/timer", 1, 10)];
const SYSTEM: &[Entry] = &[
    row("Macros", "icons/macro", 1, 6),
    row("Config", "icons/config", 1, 7),
    row("Help Desk", "icons/help", 1, 8),
    row("Log Out", "icons/cancel", 1, 12),
    row("Shut Down", "icons/system", 1, 11),
];
const TOP: &[Entry] = &[
    row("Status", "icons/status", 0, 1),
    row("Equipment", "icons/equipment", 0, 2),
    row("Items", "icons/items", 0, 4),
    row("Magic", "icons/magic", 0, 3),
    row("Abilities", "icons/abilities", 0, 5),
    Entry { label: "Social", icon: "icons/party", kind: Kind::Sub(SOCIAL) },
    Entry { label: "Journal", icon: "icons/quest_log", kind: Kind::Sub(JOURNAL) },
    Entry { label: "Map", icon: "icons/map", kind: Kind::Sub(WORLD) },
    row("Synthesis", "icons/craft", 0, 10),
    Entry { label: "System", icon: "icons/system", kind: Kind::Sub(SYSTEM) },
];
/// In the Mog House retail's row 12 is Mog House, not Map (FUN_1013ff90): a row of its own here, the map's
/// category keeping Region Info and Current Time.
const WORLD_MH: &[Entry] = &[row("Region Info", "icons/find", 0, 11), row("Current Time", "icons/timer", 1, 10)];
const TOP_MH: &[Entry] = &[
    row("Status", "icons/status", 0, 1),
    row("Equipment", "icons/equipment", 0, 2),
    row("Items", "icons/items", 0, 4),
    row("Magic", "icons/magic", 0, 3),
    row("Abilities", "icons/abilities", 0, 5),
    row("Mog House", "icons/mog_house", 0, 12),
    Entry { label: "Social", icon: "icons/party", kind: Kind::Sub(SOCIAL) },
    Entry { label: "Journal", icon: "icons/quest_log", kind: Kind::Sub(JOURNAL) },
    Entry { label: "World", icon: "icons/map", kind: Kind::Sub(WORLD_MH) },
    row("Synthesis", "icons/craft", 0, 10),
    Entry { label: "System", icon: "icons/system", kind: Kind::Sub(SYSTEM) },
];

/// The top level: the Mog House's in the Mog House (not in Monstrosity, whose row 12 stays the map).
fn top(w: &World) -> &'static [Entry] {
    if w.myroom.is_some() && !monstrosity::active(w) { TOP_MH } else { TOP }
}

/// The top level's and Journal's rows (label, icon, opens a submenu), for modern-shot's row comparison.
pub(super) fn sample() -> [Vec<(&'static str, &'static str, bool)>; 2] {
    let rows = |l: &[Entry]| l.iter().map(|e| (e.label, e.icon, matches!(e.kind, Kind::Sub(_)))).collect();
    [rows(TOP), rows(JOURNAL)]
}

/// The menu's cursor: the top row, and the submenu's row while one is open.
#[derive(Clone, Copy, Debug, Default)]
pub struct CmdMenu {
    pub top: usize,
    pub sub: Option<usize>,
}

/// A row's label where the retail menu swaps it (FUN_10140070): Customization in Monstrosity.
fn label(w: &World, e: &Entry) -> &'static str {
    match e.kind {
        Kind::Row(0, 2) if monstrosity::active(w) => "Customization",
        _ => e.label,
    }
}

fn disabled(w: &World, e: &Entry) -> bool {
    match e.kind {
        Kind::Row(page, id) => menu_disabled_on(w, page).contains(&id),
        // As Quests.
        Kind::Roe => menu_disabled_on(w, 1).contains(&2),
        Kind::Sub(_) => false,
    }
}

/// Up / Down move (wrapping, 0001), Right / Enter open a category (0002), Left / Escape go back (0003), Enter on a row
/// runs it; Escape on the top level closes the menu.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let mut c = w.cmd;
    let top = top(w);
    c.top = c.top.min(top.len() - 1);
    let sub = match top[c.top].kind {
        Kind::Sub(s) => Some(s),
        Kind::Row(..) | Kind::Roe => None,
    };
    match (c.sub, key) {
        (None, K::ArrowDown | K::ArrowUp) => {
            let n = top.len();
            c.top = if key == K::ArrowDown { (c.top + 1) % n } else { (c.top + n - 1) % n };
            w.sounds.push(Se::Cursor as u32);
        }
        (Some(k), K::ArrowDown | K::ArrowUp) => {
            let n = sub.map_or(1, |s| s.len());
            c.sub = Some(if key == K::ArrowDown { (k + 1) % n } else { (k + n - 1) % n });
            w.sounds.push(Se::Cursor as u32);
        }
        (None, K::ArrowRight | K::Enter) if sub.is_some() => {
            c.sub = Some(0);
            w.sounds.push(Se::Select as u32);
        }
        (None, K::Enter) => pick(w, &top[c.top]),
        (Some(k), K::Enter) => {
            if let Some(e) = sub.and_then(|s| s.get(k)) {
                pick(w, e);
            }
        }
        (Some(_), K::ArrowLeft | K::Escape) => {
            c.sub = None;
            w.sounds.push(Se::Cancel as u32);
        }
        (None, K::Escape) => w.menu = None,
        _ => {}
    }
    w.cmd = c;
}

/// What the top bar shows for the cursor: a retail row's (page, child), else a title and help of ours.
pub enum BarFocus {
    Row(u8, u16),
    Text(&'static str, String),
}

pub fn bar_focus(w: &World) -> BarFocus {
    let c = w.cmd;
    let list = top(w);
    let top = &list[c.top.min(list.len() - 1)];
    let e = match (&top.kind, c.sub) {
        (Kind::Sub(s), Some(k)) => s.get(k).unwrap_or(top),
        _ => top,
    };
    match e.kind {
        Kind::Row(page, id) => BarFocus::Row(page, id),
        Kind::Roe => BarFocus::Text("Records of Eminence", "View your Records of Eminence objectives and progress.".into()),
        Kind::Sub(s) => BarFocus::Text(e.label, s.iter().map(|r| label(w, r)).collect::<Vec<_>>().join(", ") + "."),
    }
}

/// A row: refused with 0004 while disabled, else the retail row runs.
fn pick(w: &mut World, e: &Entry) {
    if disabled(w, e) && !matches!(e.kind, Kind::Sub(_)) {
        w.sounds.push(Se::Error as u32);
        return;
    }
    match e.kind {
        Kind::Row(page, id) => w.menu_click = Some((page, id)),
        Kind::Roe => {
            w.menu_click = Some((1, 2));
            w.journal_roe = Some(0);
        }
        Kind::Sub(_) => {}
    }
}

/// One list of rows; the row clicked.
fn rows(ui: &mut egui::Ui, m: &super::Modern, w: &World, list: &[Entry], cursor: Option<usize>, open: Option<usize>) -> Option<usize> {
    let mut clicked = None;
    for (k, e) in list.iter().enumerate() {
        let on = cursor == Some(k) || open == Some(k);
        let resp = super::rows::menu_row(ui, m, e.icon, label(w, e), on, disabled(w, e), matches!(e.kind, Kind::Sub(_)), false);
        if resp.clicked() {
            clicked = Some(k);
        }
    }
    clicked
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if w.menu.is_none() || w.main_off {
        return;
    }
    let m = p.m;
    let list_top = top(w);
    w.cmd.top = w.cmd.top.min(list_top.len() - 1);
    let c = w.cmd;
    let mut top_click = None;
    let rect = p.panel("command", Some(Title { icon: None, text: "Menu", note: None }), 250.0, |ui| {
        top_click = rows(ui, m, w, list_top, c.sub.is_none().then_some(c.top), c.sub.map(|_| c.top));
    });
    let mut c = w.cmd;
    // A click takes a row (a category opens beside the menu); hovering only lights it.
    if let Some(k) = top_click {
        c.top = k;
        match list_top[k].kind {
            Kind::Sub(_) => {
                c.sub = Some(0);
                w.sounds.push(Se::Select as u32);
            }
            Kind::Row(..) | Kind::Roe => pick(w, &list_top[k]),
        }
    }
    w.cmd = c;
    let Kind::Sub(list) = list_top[c.top].kind else { return };
    let Some(sub) = c.sub else { return };
    let Some(r) = rect else { return };
    // The submenu opens at the left of the menu, level with its category.
    let mut sub_click = None;
    p.panel_at("command sub", Some(Title { icon: Some(list_top[c.top].icon), text: list_top[c.top].label, note: None }), 250.0, egui::pos2(r.min.x - 262.0, r.min.y), |ui| {
        sub_click = rows(ui, m, w, list, Some(sub), None);
    });
    if let Some(k) = sub_click
        && let Some(e) = list.get(k)
    {
        w.cmd.sub = Some(k);
        pick(w, e);
    }
}
