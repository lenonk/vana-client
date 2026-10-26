//! The modern Command Menu (the main menu, "-"): the retail menu's rows, organised in categories with the pack's
//! icons; a category opens its rows in a submenu beside it. Arrows and Enter or the mouse; a pick runs the retail row
//! (client/mod.rs's menu dispatch) through `World::menu_click`.

use super::super::{World, menu_disabled_on, monstrosity};
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Sense, vec2};

/// A row: its label, the pack's icon, and the retail row it runs (page, child) or the submenu it opens.
pub struct Entry {
    label: &'static str,
    icon: &'static str,
    kind: Kind,
}

enum Kind {
    Row(u8, u16),
    Sub(&'static [Entry]),
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
const JOURNAL: &[Entry] = &[row("Missions", "icons/quest", 1, 1), row("Quests", "icons/quest_log", 1, 2), row("Key Items", "icons/key", 1, 3)];
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

/// The menu's cursor: the top row, and the submenu's row while one is open.
#[derive(Clone, Copy, Debug, Default)]
pub struct CmdMenu {
    pub top: usize,
    pub sub: Option<usize>,
}

/// A row's label where the retail menu swaps it (FUN_10140070 / FUN_1013ff90): Customization in Monstrosity, Mog
/// House in the Mog House.
fn label(w: &World, e: &Entry) -> &'static str {
    match e.kind {
        Kind::Row(0, 2) if monstrosity::active(w) => "Customization",
        Kind::Row(0, 12) if w.myroom.is_some() && !monstrosity::active(w) => "Mog House",
        _ => e.label,
    }
}

fn disabled(w: &World, e: &Entry) -> bool {
    match e.kind {
        Kind::Row(page, id) => menu_disabled_on(w, page).contains(&id),
        Kind::Sub(_) => false,
    }
}

/// Up / Down move (wrapping, 0001), Right / Enter open a category (0002), Left / Escape go back (0003), Enter on a row
/// runs it; Escape on the top level closes the menu.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let mut c = w.cmd;
    let sub = match TOP[c.top].kind {
        Kind::Sub(s) => Some(s),
        Kind::Row(..) => None,
    };
    match (c.sub, key) {
        (None, K::ArrowDown | K::ArrowUp) => {
            let n = TOP.len();
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
        (None, K::Enter) => pick(w, &TOP[c.top]),
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

/// A row: refused with 0004 while disabled, else the retail row runs.
fn pick(w: &mut World, e: &Entry) {
    if let Kind::Row(page, id) = e.kind {
        if disabled(w, e) {
            w.sounds.push(Se::Error as u32);
        } else {
            w.menu_click = Some((page, id));
        }
    }
}

/// One list of rows; the row clicked or hovered.
fn rows(ui: &mut egui::Ui, m: &super::Modern, w: &World, list: &[Entry], cursor: Option<usize>, open: Option<usize>) -> (Option<usize>, Option<usize>) {
    let (mut clicked, mut hovered) = (None, None);
    for (k, e) in list.iter().enumerate() {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        let off = disabled(w, e);
        let sprite = if off {
            "controls/list_row_disabled"
        } else if cursor == Some(k) || open == Some(k) {
            "controls/list_row_selected"
        } else if resp.hovered() {
            "controls/list_row_hover"
        } else {
            "controls/list_row_normal"
        };
        let p = ui.painter();
        if let Some(s) = m.art.sprite(sprite) {
            p.add(paint::nine_slice(&s, r, r.height() / s.size.y, Color32::WHITE));
        }
        if let Some(s) = m.art.sprite(e.icon) {
            p.add(paint::image(&s, egui::pos2(r.min.x + 20.0, r.center().y - 11.0), 22.0, if off { Color32::from_gray(110) } else { Color32::WHITE }));
        }
        let colour = if off { m.theme.color("muted") } else { m.theme.color("text") };
        p.text(egui::pos2(r.min.x + 50.0, r.center().y), Align2::LEFT_CENTER, label(w, e), FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), colour);
        if matches!(e.kind, Kind::Sub(_))
            && let Some(s) = m.art.sprite("controls/arrow_right")
        {
            p.add(paint::image(&s, egui::pos2(r.max.x - 26.0, r.center().y - 8.0), 16.0, Color32::WHITE));
        }
        if resp.clicked() {
            clicked = Some(k);
        }
        if resp.hovered() {
            hovered = Some(k);
        }
        ui.add_space(2.0);
    }
    (clicked, hovered)
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if w.menu.is_none() {
        return;
    }
    let m = p.m;
    let c = w.cmd;
    let (mut top_click, mut top_hover) = (None, None);
    let rect = p.panel("command", Some(Title { icon: None, text: "Menu", note: None }), 250.0, |ui| {
        (top_click, top_hover) = rows(ui, m, w, TOP, c.sub.is_none().then_some(c.top), c.sub.map(|_| c.top));
    });
    let mut c = w.cmd;
    // Hovering a row moves the cursor there (0001) when the pointer moves; a category under it opens beside the menu.
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    if let Some(k) = top_hover.filter(|_| moved)
        && (k != c.top || c.sub.is_some() && !matches!(TOP[k].kind, Kind::Sub(_)))
    {
        c.top = k;
        c.sub = matches!(TOP[k].kind, Kind::Sub(_)).then_some(0).filter(|_| c.sub.is_some());
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(k) = top_click {
        c.top = k;
        match TOP[k].kind {
            Kind::Sub(_) => {
                c.sub = Some(0);
                w.sounds.push(Se::Select as u32);
            }
            Kind::Row(..) => pick(w, &TOP[k]),
        }
    }
    w.cmd = c;
    let Kind::Sub(list) = TOP[c.top].kind else { return };
    let Some(sub) = c.sub else { return };
    let Some(r) = rect else { return };
    // The submenu opens at the left of the menu, level with its category.
    let (mut sub_click, mut sub_hover) = (None, None);
    p.panel_at("command sub", Some(Title { icon: Some(TOP[c.top].icon), text: TOP[c.top].label, note: None }), 250.0, egui::pos2(r.min.x - 262.0, r.min.y), |ui| {
        (sub_click, sub_hover) = rows(ui, m, w, list, Some(sub), None);
    });
    if let Some(k) = sub_hover.filter(|&k| k != sub && moved) {
        w.cmd.sub = Some(k);
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(e) = sub_click.and_then(|k| list.get(k)) {
        pick(w, e);
    }
}
