//! The modern magic and ability windows over the retail ones' state (client/magic.rs keeps the category windows, the
//! lists, their sort windows and every rule): the category window, the magic, ability, mount and emote lists with
//! their sort strip and cost box. The mouse is the kit's: hovering only lights a row, a click puts the cursor on
//! it and takes it; the wheel scrolls.

use super::super::listsearch::{self, Hit};
use super::super::magic::{self, Cat, Cats, Kind, ListWin, VISIBLE};
use super::super::scrollbar::Msg;
use super::super::World;
use super::{Panels, Title, kit, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use std::time::Instant;

const ROW: f32 = 34.0;

/// A row's look: under the cursor, Manual Sort's picked one.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Normal,
    Cursor,
    Picked,
}

/// A row's frame (the pack's list rows), its icon and name, greyed when `off` (can't be used now); its response.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, m: &super::Modern, icon: Option<egui::TextureId>, pack: Option<&str>, name: &str, right: &str, look: Look, off: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let sprite = match look {
        Look::Cursor => "controls/list_row_selected",
        Look::Picked => "controls/list_row_hover",
        _ if resp.hovered() => "controls/list_row_hover",
        _ if off => "controls/list_row_disabled",
        Look::Normal => "controls/list_row_normal",
    };
    let p = ui.painter();
    super::rows::face(p, m, r, sprite, Color32::WHITE);
    let tint = if off { Color32::from_gray(110) } else { Color32::WHITE };
    let at = pos2(r.min.x + 10.0, r.center().y - 11.0);
    if let Some(t) = icon {
        p.image(t, Rect::from_min_size(at, vec2(22.0, 22.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
    } else if let Some(s) = pack.and_then(|i| m.art.sprite(i)) {
        p.add(paint::image(&s, at, 22.0, tint));
    }
    let colour = super::rows::label_colour(m, look == Look::Cursor, off);
    p.text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), colour);
    if !right.is_empty() {
        p.text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(m.theme.small), m.theme.color("muted"));
    }
    ui.add_space(2.0);
    resp
}

/// The pack's icon for a category.
fn cat_icon(cat: Cat) -> &'static str {
    match cat {
        Cat::Magic(1, _) => "icons/healing",
        Cat::Magic(2, _) => "icons/elemental",
        Cat::Magic(3, _) => "icons/summoning",
        Cat::Magic(4, _) => "icons/ninjutsu",
        Cat::Magic(5, _) => "icons/songs",
        Cat::Magic(7, _) => "icons/geomancy",
        Cat::Magic(8, _) => "icons/party",
        Cat::Magic(..) => "icons/magic",
        Cat::Ability(3) | Cat::Ability(0x14) => "icons/weaponskill",
        Cat::Ability(4) => "icons/job",
        Cat::Ability(_) => "icons/abilities",
        Cat::Ranged => "icons/bow",
        Cat::Mount => "icons/mount",
        Cat::Friends => "icons/party",
        Cat::Emote => "icons/emote",
    }
}

/// The row clicked: the cursor goes there and it is taken (Enter).
#[derive(Default)]
pub(super) struct Mouse {
    click: Option<usize>,
}

impl Mouse {
    fn take(&mut self, k: usize, resp: &egui::Response) {
        if resp.clicked() {
            self.click = Some(k);
        }
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    // Set Spells suspends the magic list and its sort window (the window walk), as retail's.
    if w.bluset.is_some() {
        return;
    }
    cats(p, w);
    list(p, w);
}

/// The search field and its hits in place of the window's rows: what was clicked, what the pointer is over.
pub(super) fn search_rows(ui: &mut Ui, m: &super::Modern, w: &World, icons: &[Option<egui::TextureId>], text: &str, sel: usize, hits: &[Hit]) -> Mouse {
    search_field(ui, m, text);
    let mut mouse = Mouse::default();
    if hits.is_empty() {
        ui.label(egui::RichText::new("Nothing matches.").size(m.theme.ui).color(m.theme.color("muted")));
    }
    for (k, h) in hits.iter().enumerate().take(listsearch::MAX) {
        // From a category window, the list each hit is in.
        let right = match (h.cat, w.cats) {
            (Some(c), Some(cats)) => magic::cat_rows(cats.menu).get(c).map_or("", |r| magic::cat_label(cats.menu, r.1)),
            _ => &h.note,
        };
        // A recent pick says so (it ranks first among the matches).
        let right = match (h.recent, right.is_empty()) {
            (true, true) => "recent".to_string(),
            (true, false) => format!("recent · {right}"),
            _ => right.to_string(),
        };
        let look = if k == sel { Look::Cursor } else { Look::Normal };
        // Spells and abilities grey as in their lists (items keep their window's rules).
        let dim = h.slot.is_none() && h.journal.is_none() && off(w, h.kind, h.id);
        let resp = row(ui, m, icons.get(k).copied().flatten(), None, &h.name, &right, look, dim);
        mouse.take(k, &resp);
    }
    mouse
}

/// The search field with the typed text and its caret.
pub(super) fn search_field(ui: &mut Ui, m: &super::Modern, text: &str) {
    kit::field(ui, m, text, Some(text.chars().count()), ui.available_width(), FontId::proportional(m.theme.ui), Some("controls/search"));
    ui.add_space(4.0);
}

/// The search's hits for the open window, their icons.
pub(super) fn search_hits(p: &Panels, w: &World) -> Option<(String, usize, Vec<Hit>, Vec<Option<egui::TextureId>>)> {
    let s = w.list_search.as_ref()?;
    let hits = listsearch::hits(w, &s.text);
    let icon = |h: &Hit| match h.slot {
        _ if matches!(h.journal, Some(listsearch::Journal::Auction(_))) => w.item_info.as_ref().and_then(|info| p.m.item_icon(p.ctx, info, h.id)),
        _ if h.journal.is_some() => None,
        Some(_) => w.item_info.as_ref().and_then(|info| p.m.item_icon(p.ctx, info, h.id)),
        None => p.gu.and_then(|gu| magic::row_icon(w, h.kind, h.id).and_then(|(lib, k)| p.m.sprite_icon(p.ctx, gu, lib, k))),
    };
    let icons = hits.iter().take(listsearch::MAX).map(icon).collect();
    // A journal hit's note: its quest area or mission log by the game's titles.
    let mut hits = hits;
    if let Some(gu) = p.gu {
        for h in &mut hits {
            match h.journal {
                Some(listsearch::Journal::Quest(area, _)) => h.note = format!("{} ({})", super::journal::area_title(gu, area), h.note),
                Some(listsearch::Journal::Mission(log, _)) => h.note = super::journal::log_title(gu, log),
                Some(listsearch::Journal::Auction(cat)) => h.note = super::super::auction::cat_label(gu, cat),
                _ => {}
            }
        }
    }
    Some((s.text.clone(), s.sel, hits, icons))
}

/// A search's mouse: a click picks.
pub(super) fn search_mouse(w: &mut World, mouse: &Mouse, hits: &[Hit]) {
    if let Some(h) = mouse.click.and_then(|k| hits.get(k)) {
        listsearch::pick(w, h);
    }
}

/// The category window (mgcmenu, magselec, abimenu, abiselec, commenu, Monstrosity's): its rows by name.
fn cats(p: &mut Panels, w: &mut World) {
    let Some(c) = w.cats else { return };
    let m = p.m;
    let rows = magic::cat_rows(c.menu);
    let title = match c.menu {
        "menu    mgcmenu" | "menu    magselec" => "Magic",
        "menu    commenu" => "Communication",
        _ => "Abilities",
    };
    let mut mouse = Mouse::default();
    let search = if w.magic.is_none() && w.ability.is_none() { search_hits(p, w) } else { None };
    let mut found = Mouse::default();
    p.panel("cats", Some(Title { icon: None, text: title, note: None }), if search.is_some() { 360.0 } else { 260.0 }, |ui| {
        if let Some((text, sel, hits, icons)) = &search {
            found = search_rows(ui, m, w, icons, text, *sel, hits);
            return;
        }
        for (k, &(_, cat)) in rows.iter().enumerate() {
            let look = if k == c.sel { Look::Cursor } else { Look::Normal };
            let resp = row(ui, m, None, Some(cat_icon(cat)), magic::cat_label(c.menu, cat), "", look, !magic::cat_ready(w, cat));
            mouse.take(k, &resp);
        }
    });
    if let Some((_, _, hits, _)) = &search {
        search_mouse(w, &found, hits);
        return;
    }
    if let Some(k) = mouse.click {
        w.cats = Some(Cats { sel: k, ..c });
        magic::key(w, egui::Key::Enter);
    }
}

/// Whether a spell or ability can't be used now (status, recast, MP / TP, effects we carry); traits never grey.
fn off(w: &World, kind: Kind, id: u16) -> bool {
    match kind {
        Kind::Magic => !magic::spell_ready(w, id),
        Kind::Ability => w.actions.as_ref().and_then(|a| a.abilities.get(id as usize)).is_some_and(|a| a.kind != 4) && !magic::ability_ready(w, id),
        Kind::Mount => magic::mount_check(w).is_some(),
        Kind::Emote => magic::emote_locked(w, id),
    }
}

/// A list row's name, value at the right (MP cost; a recast under way), whether it can't be used now.
fn row_view(w: &World, kind: Kind, id: u16) -> (String, String, bool) {
    match kind {
        Kind::Magic => {
            let mp = w.actions.as_ref().and_then(|a| a.spell(id)).map_or(0, |r| r.mp);
            let recast = magic::spell_recast(w, id);
            let right = if recast > 0.0 { format!("{}   {mp} MP", magic::clock(recast)) } else { format!("{mp} MP") };
            (magic::spell_name(w, id), right, off(w, kind, id))
        }
        Kind::Ability => {
            let a = w.actions.as_ref().and_then(|a| a.abilities.get(id as usize));
            let recast = a.map_or(0.0, |a| if a.timer == 900 { 0.0 } else { magic::ability_recast(w, a.timer) });
            (magic::ability_name(w, id), if recast > 0.0 { magic::clock(recast) } else { String::new() }, off(w, kind, id))
        }
        Kind::Mount => (magic::mount_name(w, id), String::new(), magic::mount_check(w).is_some()),
        Kind::Emote => (magic::emote_entry(w, id).and_then(|e| w.names.as_ref()?.emote_text(e.name as u32)).unwrap_or_default(), String::new(), magic::emote_locked(w, id)),
    }
}

/// The cost box's lines for the cursor's row (subwindo: magic MP Cost and Next; abilities by their kind).
fn cost_lines(w: &World, win: &ListWin) -> Vec<(&'static str, String)> {
    let at = win.rows.get(win.scroll + win.cursor).copied();
    match (win.kind, at) {
        (Kind::Magic, Some(s)) => {
            let mp = w.actions.as_ref().and_then(|a| a.spell(s)).map_or(0, |r| r.mp);
            vec![("MP Cost", mp.to_string()), ("Next", magic::clock(magic::spell_recast(w, s)))]
        }
        (Kind::Mount, _) => {
            let secs = w.mount_recast.map_or(0.0, |t| t.saturating_duration_since(Instant::now()).as_secs_f32());
            vec![("Next", magic::clock(secs))]
        }
        (Kind::Ability, Some(i)) => match w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)) {
            Some(a) => {
                let next = ("Next", magic::clock(if a.timer == 900 { 0.0 } else { magic::ability_recast(w, a.timer) }));
                match magic::ability_kind(a.kind) {
                    2 | 10 | 5 | 8 => vec![next],
                    3 => vec![("TP Cost", a.tp.to_string()), next],
                    4 | 9 => vec![("TP", w.vitals.map_or(0, |v| v.2).to_string())],
                    6 => vec![("MP Cost", a.cost.to_string())],
                    _ => Vec::new(),
                }
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The open list: its rows from the scroll, the sort strip (Num+ focuses it), the cost box, the bar.
fn list(p: &mut Panels, w: &mut World) {
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|x| !x.hidden).cloned() else { return };
    let m = p.m;
    let title = match win.kind {
        Kind::Magic => magic::type_label(win.ty),
        Kind::Ability => magic::ability_label(win.ty),
        Kind::Mount => "Mounts",
        Kind::Emote => "Emotes",
    };
    let note = win.rows.len().to_string();
    let icons: Vec<Option<egui::TextureId>> = win.rows.iter().skip(win.scroll).take(VISIBLE).map(|&id| p.gu.and_then(|gu| magic::row_icon(w, win.kind, id).and_then(|(lib, k)| m.sprite_icon(p.ctx, gu, lib, k)))).collect();
    let mut mouse = Mouse::default();
    let search = search_hits(p, w);
    let mut found = Mouse::default();
    let mut sort_click: Option<u16> = None;
    let mut ask_click: Option<bool> = None;
    let mut wheel = 0.0f32;
    let rect = p.panel("list", Some(Title { icon: None, text: title, note: Some(&note) }), 360.0, |ui| {
        // The sort strip: its window's rows as the kit's buttons, the cursor's ringed while it has the keys.
        if let Some(menu) = win.sort {
            let labels = sort_labels(menu);
            let cursor = win.sort_focus.filter(|_| win.ask.is_none()).map(|c| c as usize - 1);
            if let Some(k) = kit::buttons(ui, m, labels, cursor, cursor.map_or(0, |c| 1 << c), 0) {
                sort_click = Some(k as u16 + 1);
            }
            if let Some(yes) = win.ask {
                ui.add_space(4.0);
                ask_click = kit::yes_no(ui, m, "Sort them now?", ["Yes", "No"], yes, m.theme.small);
            }
            ui.add_space(6.0);
        }
        if let Some((text, sel, hits, icons)) = &search {
            found = search_rows(ui, m, w, icons, text, *sel, hits);
            return;
        }
        let top = ui.cursor().min;
        if win.rows.is_empty() {
            let empty = match (win.kind, win.list_mode) {
                (Kind::Mount, _) => "No mounts.",
                (Kind::Magic, true) => "No spells learned.",
                (Kind::Magic, false) => "No spells available.",
                _ => "Nothing available.",
            };
            ui.label(egui::RichText::new(empty).size(m.theme.ui).color(m.theme.color("muted")));
        }
        for (k, &id) in win.rows.iter().skip(win.scroll).take(VISIBLE).enumerate() {
            let (name, right, off) = row_view(w, win.kind, id);
            let picked = win.manual == 2 && win.scroll + k == win.picked;
            let look = if k == win.cursor && win.sort_focus.is_none() { Look::Cursor } else if picked { Look::Picked } else { Look::Normal };
            let resp = row(ui, m, icons.get(k).copied().flatten(), None, &name, &right, look, off);
            mouse.take(k, &resp);
        }
        // The bar: the rows shown out of all, at the right edge.
        let track = Rect::from_min_max(pos2(ui.max_rect().max.x + 6.0, top.y), pos2(ui.max_rect().max.x + 14.0, ui.cursor().min.y));
        kit::bar(ui.painter(), m, track, win.rows.len(), VISIBLE, win.scroll);
        // The cost box for the cursor's row, and Manual Sort's hint.
        let lines = cost_lines(w, &win);
        if !lines.is_empty() || win.manual > 0 {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                for (label, value) in &lines {
                    ui.label(egui::RichText::new(*label).size(m.theme.small).color(m.theme.color("muted")));
                    ui.label(egui::RichText::new(value).size(m.theme.ui).color(m.theme.color("text")));
                }
            });
            if win.manual > 0 {
                let hint = if win.manual == 1 { "Manual Sort: pick a row to move." } else { "Manual Sort: pick its new place." };
                ui.label(egui::RichText::new(hint).size(m.theme.small).color(m.theme.color("gold")));
            }
        }
        if ui.rect_contains_pointer(ui.max_rect()) {
            // The wheel's own events (line notches; pixel deltas as 50 points a notch), not egui's smoothed scroll.
            wheel = ui.input(|i| {
                i.events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y,
                        egui::Event::MouseWheel { delta, .. } => delta.y / 50.0,
                        _ => 0.0,
                    })
                    .sum::<f32>()
            });
        }
    });
    let Some(_) = rect else { return };
    if let Some((_, _, hits, _)) = &search {
        search_mouse(w, &found, hits);
        return;
    }
    // Mouse: the sort strip and its question go through the keys as retail's window does.
    if let Some(yes) = ask_click {
        if let Some(x) = list_mut(w) {
            x.ask = Some(yes);
        }
        magic::key(w, egui::Key::Enter);
        return;
    }
    if let Some(child) = sort_click {
        if let Some(x) = list_mut(w) {
            x.sort_focus = Some(child);
        }
        magic::key(w, egui::Key::Enter);
        return;
    }
    if let Some(k) = mouse.click {
        if let Some(x) = list_mut(w) {
            x.cursor = k;
        }
        magic::key(w, egui::Key::Enter);
    }
    // A notch of the wheel is a row.
    if wheel != 0.0 {
        let msg = if wheel > 0.0 { Msg::Up } else { Msg::Down };
        for _ in 0..(wheel.abs().round() as usize).clamp(1, 5) {
            magic::bar(w, msg);
        }
    }
}

/// The sort strip's buttons for a sort window.
fn sort_labels(menu: &str) -> &'static [&'static str] {
    match menu {
        "menu    blusortw" => &["Auto-sort", "Manual Sort", "Set Spells"],
        "menu    mgcsortw" => &["Auto-sort", "Manual Sort"],
        _ => &["Manual Sort"],
    }
}

/// The open list's arrows in the kit's model: the sort strip a row of buttons above the rows (Up from the first row
/// reaches it, Left / Right walk it, Down comes back), its question's Yes / No side by side. Enter, Escape and the
/// rest stay magic.rs's. True when taken.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    use egui::Key as K;
    if !w.modern || w.prompt.is_some() || w.cats.is_some() || w.list_search.is_some() || !matches!(key, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight) {
        return false;
    }
    let Some(win) = list_mut(w).filter(|x| !x.hidden && x.manual == 0) else { return false };
    let Some(menu) = win.sort else { return false };
    if let Some(yes) = win.ask {
        if matches!(key, K::ArrowLeft | K::ArrowRight) {
            win.ask = Some(!yes);
            w.sounds.push(Se::Cursor as u32);
        }
        return true;
    }
    let n = win.rows.len();
    let stops = [kit::Stop::Buttons { n: sort_labels(menu).len(), off: 0 }, kit::Stop::List { n, page: VISIBLE }];
    let mut at = match win.sort_focus {
        Some(c) => kit::At::new(0, c as usize - 1),
        None => kit::At::new(1, win.scroll + win.cursor),
    };
    let act = kit::key(&stops, &mut at, key, false);
    if at.stop == 0 {
        win.sort_focus = Some(at.sub as u16 + 1);
    } else {
        win.sort_focus = None;
        let i = at.sub.min(n.saturating_sub(1));
        if i < win.scroll {
            win.scroll = i;
        } else if i >= win.scroll + VISIBLE {
            win.scroll = i + 1 - VISIBLE;
        }
        win.cursor = i - win.scroll;
    }
    w.sounds.extend(act.sound().map(|s| s as u32));
    true
}

fn list_mut(w: &mut World) -> Option<&mut ListWin> {
    if w.ability.is_some() { w.ability.as_mut() } else { w.magic.as_mut() }
}
