//! Small retail windows in the modern style; their modules keep the rules and keys: the block list (blklist.rs, its
//! names and blkmain's Delete) and the chat mode window (chatctrl.rs, its seven rows with the tell name, the shells
//! and the Unity, the cursor row's range under them), the comment editor (seacom.rs) and the Linkshell Concierge
//! (concierge.rs), Mog House Remodel (remodel.rs), the Adventuring Primer's lists (primer.rs) and layout mode's
//! windows (layout.rs), pupequip (equipwin.rs), the fishing gauge (fishing.rs) and the Fishing Ranking (ranking.rs).

use super::super::listsearch::{Entry, count_note};
use super::super::{World, blklist, chatctrl, concierge, equipwin, fishing, garden, layout, map, primer, ranking, remodel, seacom};
use super::craft::{button, caption};
use super::{MenuWin, Panels, Title, social};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Sense, pos2, vec2};

const W: f32 = 300.0;
const ROW: f32 = 28.0;
const TALL: f32 = 15.0 * (ROW + 2.0);

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    block_list(p, w);
    chat_mode(p, w);
    comment(p, w);
    concierge_list(p, w);
    fishing_ranking(p, w);
    remodel_list(p, w);
    primer_lists(p, w);
    layout_windows(p, w);
    garden_windows(p, w);
    pup_menu(p, w);
    fish_gauge(p, w);
    map_menu(p, w);
    map_markers(p, w);
    map_list(p, w);
    map_scan(p, w);
}

/// The map's menu (map0: Markers, Wide Scan, Change Map) as a column; Wide Scan dim until a scan is possible.
fn map_menu(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((sel, scan)) = w.map.as_ref().filter(|m| m.has_menu()).map(|m| (m.cursor, m.scan_ok())) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.focus_text("menu    map0", sel)[1].clone().unwrap_or_else(|| "Map".into());
    let mut got = None;
    p.panel("map0", Some(Title { icon: None, text: &title, note: None }), social::width(1), |ui| {
        let col = MenuWin { name: "menu    map0", sel, off: if scan { Vec::new() } else { vec![2] }, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
        got = social::column(ui, m, gu, &col, free);
    });
    if free {
        social::act(w, gu, got, |w, k| w.map.as_mut().is_some_and(|m| m.set_cursor(k)), |w, _| map::key(w, egui::Key::Enter));
    }
}

/// Markers (mapv3): the set strip (the four sets' icons, the shown one lit, those with nothing to show dim), the ten
/// rows with their icons and names, and mapv2 (View, Place / Remove, Rename) at the left of its row, as retail's.
fn map_markers(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = w.map.as_ref().and_then(|m| m.markers_look()) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let full = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let (mut set_click, mut row_click, mut v2_row) = (None, None, None);
    let follow = v.cursor.is_some_and(|c| m.moved("map markers", c));
    let mut found = small_found(p, w, &[Small::Markers]);
    p.panel("mapv3", Some(Title { icon: None, text: "Markers", note: None }), W, |ui| {
        ui.horizontal(|ui| {
            for (s, &on) in v.sets.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 26.0), Sense::click());
                let face = match () {
                    _ if s == v.set => "controls/list_row_selected",
                    _ if !on => "controls/list_row_disabled",
                    _ if resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                if let Some(t) = m.sprite_icon(ui.ctx(), gu, "menu    keytops3", map::SET_ICONS[s]) {
                    let tint = if on { Color32::WHITE } else { Color32::from_white_alpha(90) };
                    ui.painter().image(t, egui::Rect::from_center_size(r.center(), vec2(16.0, 16.0)), full, tint);
                }
                if resp.clicked() && on {
                    set_click = Some(s);
                }
            }
        });
        ui.add_space(4.0);
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        let out = super::kit::list(ui, m, "map markers", v.rows.len(), v.cursor, free, follow, ROW, TALL, |_| false, |ui, row| {
            let (icon, name) = &v.rows[row.index];
            if let Some(t) = icon.and_then(|k| m.sprite_icon(ui.ctx(), gu, "menu    keytops3", k)) {
                ui.painter().image(t, egui::Rect::from_min_size(row.rect.min + vec2(8.0, (ROW - 16.0) / 2.0), vec2(16.0, 16.0)), full, Color32::WHITE);
            }
            ui.painter().text(row.rect.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), super::kit::ink(m, row));
            if v.v2.is_some_and(|x| x.0 == row.index) {
                v2_row = Some(row.rect);
            }
        });
        row_click = out.clicked;
    });
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    let mut menu_click = None;
    if let (Some((_, c, placed)), Some(r)) = (v.v2, v2_row) {
        let width = social::width(1);
        p.panel_at("mapv2", None, width, pos2(r.min.x - width - 20.0, r.min.y - 8.0), |ui| {
            // View needs a marker placed.
            let col = MenuWin { name: "menu    mapv2", sel: c, off: if placed { Vec::new() } else { vec![1] }, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            menu_click = social::column(ui, m, gu, &col, free);
        });
    }
    if free && !v.placing {
        if let Some(s) = set_click {
            map::marker_set_click(w, s);
        }
        if let Some(k) = row_click {
            map::marker_click(w, k);
        }
        if let Some(c) = menu_click {
            map::marker_menu_click(w, c);
        }
    }
}

/// Wide Scan from the map (scanlist): a row per name with its icon (player, NPC, monster); on a pick mapscan (Track,
/// Remote Scan) at map0's place, Track dim for a player as retail's.
fn map_scan(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = w.map.as_ref().and_then(|m| m.scan_look()) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let full = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let follow = m.moved("map scan", v.cursor);
    let title = gu.focus_text("menu    scanlist", 1)[1].clone().unwrap_or_else(|| "Wide Scan".into());
    let mut click = None;
    let mut found = small_found(p, w, &[Small::MapScan]);
    p.panel("scanlist", Some(Title { icon: None, text: &title, note: None }), W, |ui| {
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        let out = super::kit::list(ui, m, "map scan", v.rows.len(), Some(v.cursor), free && v.mapscan.is_none(), follow, ROW, TALL, |_| false, |ui, row| {
            let (icon, name) = &v.rows[row.index];
            if let Some(t) = m.sprite_icon(ui.ctx(), gu, "menu    keytops3", *icon) {
                ui.painter().image(t, egui::Rect::from_min_size(row.rect.min + vec2(8.0, (ROW - 16.0) / 2.0), vec2(16.0, 16.0)), full, Color32::WHITE);
            }
            ui.painter().text(row.rect.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), super::kit::ink(m, row));
        });
        click = out.clicked;
    });
    let mut got = None;
    if let Some((c, track)) = v.mapscan {
        let title = gu.focus_text("menu    mapscan", c)[1].clone().unwrap_or_else(|| "Map".into());
        p.panel("mapscan", Some(Title { icon: None, text: &title, note: None }), social::width(1), |ui| {
            let col = MenuWin { name: "menu    mapscan", sel: c, off: if track { Vec::new() } else { vec![1] }, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            got = social::column(ui, m, gu, &col, free);
        });
    }
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    if let Some(k) = click {
        map::scan_click(w, k);
    }
    social::act(w, gu, got, |w, k| w.map.as_mut().is_some_and(|m| m.set_mapscan(k)), |w, _| map::key(w, egui::Key::Enter));
}

/// Change Map's list (maplist): a row per area or map, the name at the left and its count or area at the right.
fn map_list(p: &mut Panels, w: &mut World) {
    let Some(v) = w.map.as_ref().and_then(|m| m.maplist_look()) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let follow = m.moved("map list", v.cursor);
    let mut click = None;
    let mut found = small_found(p, w, &[Small::MapList]);
    p.panel("maplist", Some(Title { icon: None, text: "Change Map", note: None }), W + 80.0, |ui| {
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        let out = super::kit::list(ui, m, "map list", v.rows.len(), Some(v.cursor), free, follow, ROW, TALL, |_| false, |ui, row| {
            let (name, value) = &v.rows[row.index];
            let font = FontId::proportional(m.theme.small);
            ui.painter().text(row.rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, name, font.clone(), super::kit::ink(m, row));
            ui.painter().text(row.rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, value, font, m.theme.color("muted"));
        });
        click = out.clicked;
    });
    if let Some(f) = found {
        f.pick(w, p.gu);
        return;
    }
    if free && let Some(k) = click {
        map::maplist_click(w, k);
    }
}

/// Plain rows (text, grey), the cursor's lit while focused and kept in view; the row clicked while focused.
fn plain_rows(ui: &mut egui::Ui, m: &super::Modern, salt: &str, rows: &[(String, bool)], cursor: usize, focused: bool) -> Option<usize> {
    let seen = egui::Id::unique((salt, "plain cursor seen"));
    let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
    ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
    let mut click = None;
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, (text, grey)) in rows.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == cursor && focused => "controls/list_row_selected",
                _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                _ if *grey => "controls/list_row_disabled",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let c = if *grey { m.theme.color("muted") } else { m.theme.color("text") };
            let clip = egui::Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), c);
            if k == cursor && follow {
                ui.scroll_to_rect(r, None);
            }
            if focused && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    click
}

/// Type-to-search (modern/search.rs) in the small windows' lists: whichever has the keys.
#[derive(Clone, Copy, PartialEq)]
enum Small {
    MapScan,
    MapList,
    Markers,
    PrimerCats,
    PrimerTopics,
    Concierge,
    Ranking,
    Blocks,
    Garden,
    Layout,
}

fn small(w: &World) -> Option<Small> {
    if w.confirm.is_some() {
        return None;
    }
    if let Some(mw) = w.map.as_ref() {
        if mw.scan_look().is_some_and(|v| v.mapscan.is_none()) {
            return Some(Small::MapScan);
        }
        if mw.maplist_look().is_some() {
            return Some(Small::MapList);
        }
        if mw.markers_look().is_some_and(|v| v.cursor.is_some() && !v.placing) {
            return Some(Small::Markers);
        }
    }
    if let Some(v) = primer::look(w).filter(|v| !v.page) {
        return Some(if v.list.is_some() { Small::PrimerTopics } else { Small::PrimerCats });
    }
    if concierge::look(w).is_some_and(|v| v.lsc.is_none()) {
        return Some(Small::Concierge);
    }
    if ranking::look(w).is_some() {
        return Some(Small::Ranking);
    }
    if blklist::view(w).is_some_and(|v| !v.2) {
        return Some(Small::Blocks);
    }
    if garden::look(w).is_some_and(|v| v.list.is_some()) {
        return Some(Small::Garden);
    }
    layout::look(w).is_some_and(|v| v.list.is_some()).then_some(Small::Layout)
}

pub(super) fn search_open(w: &World) -> bool {
    small(w).is_some()
}

/// The search's hits for the small window's list `s`, only while it is the one with the keys (`any`: either of two).
fn small_found(p: &Panels, w: &World, any: &[Small]) -> Option<super::lists::Found> {
    small(w).is_some_and(|s| any.contains(&s)).then(|| super::lists::found(p, w, "small")).flatten()
}

pub(super) fn search_rows(w: &World) -> Vec<Entry> {
    let slots = |list: Option<(Vec<crate::inventory::Slot>, Vec<u32>, usize)>| {
        list.map_or(Vec::new(), |(rows, tints, _)| rows.iter().zip(&tints).map(|(s, &t)| Entry::item(w, s.id).note(count_note(s.count)).dim(t == crate::inventory::TINT_DIM)).collect())
    };
    let named = |v: Vec<String>| v.into_iter().map(Entry::named).collect::<Vec<_>>();
    match small(w) {
        Some(Small::MapScan) => w.map.as_ref().and_then(|m| m.scan_look()).map_or(Vec::new(), |v| named(v.rows.into_iter().map(|r| r.1).collect())),
        Some(Small::MapList) => w.map.as_ref().and_then(|m| m.maplist_look()).map_or(Vec::new(), |v| v.rows.into_iter().map(|(a, b)| Entry::named(a).note(b)).collect()),
        Some(Small::Markers) => w.map.as_ref().and_then(|m| m.markers_look()).map_or(Vec::new(), |v| named(v.rows.into_iter().map(|r| r.1).collect())),
        Some(Small::PrimerCats) => primer::look(w).map_or(Vec::new(), |v| named(v.cats)),
        Some(Small::PrimerTopics) => primer::look(w).and_then(|v| v.list).map_or(Vec::new(), |(_, t, _)| named(t)),
        Some(Small::Concierge) => concierge::look(w).map_or(Vec::new(), |v| v.rows.into_iter().map(|r| r.map_or(Entry::default(), |r| Entry::named(r.name).note(r.wanted.to_string()))).collect()),
        Some(Small::Ranking) => ranking::look(w).map_or(Vec::new(), |(_, _, rows)| rows.into_iter().map(|r| Entry::named(r.name).note(format!("#{}", r.rank))).collect()),
        Some(Small::Blocks) => blklist::view(w).map_or(Vec::new(), |v| named(v.0)),
        Some(Small::Garden) => slots(garden::look(w).and_then(|v| v.list)),
        Some(Small::Layout) => slots(layout::look(w).and_then(|v| v.list)),
        None => Vec::new(),
    }
}

/// A pick: the list's cursor onto row `k`, then Enter (as a click on the row; the ranking only moves its cursor).
pub(super) fn search_pick(w: &mut World, gu: Option<&super::super::game_ui::GameUi>, k: usize) {
    match small(w) {
        Some(Small::MapScan) => map::scan_click(w, k),
        Some(Small::MapList) => map::maplist_click(w, k),
        Some(Small::Markers) => map::marker_click(w, k),
        Some(Small::PrimerCats) => act(w, Some(k), primer::set_cat, |w| primer::key(w, egui::Key::Enter, false)),
        Some(Small::PrimerTopics) => act(w, Some(k), primer::set_topic, |w| primer::key(w, egui::Key::Enter, false)),
        Some(Small::Concierge) => {
            let Some(gu) = gu else { return };
            concierge::set_row(w, k);
            concierge::key(w, gu, egui::Key::Enter);
        }
        Some(Small::Ranking) => ranking::set_cursor(w, k),
        Some(Small::Blocks) => act(w, Some(k), blklist::set_cursor, |w| blklist::key(w, egui::Key::Enter)),
        Some(Small::Garden) => act(w, Some(k), garden::set_row, |w| garden::key(w, egui::Key::Enter)),
        Some(Small::Layout) => act(w, Some(k), layout::set_row, |w| {
            layout::key(w, egui::Key::Enter);
        }),
        None => {}
    }
}

/// A list's mouse: a click moves the cursor and is Enter.
fn act(w: &mut World, click: Option<usize>, set: impl Fn(&mut World, usize) -> bool, enter: impl Fn(&mut World)) {
    if let Some(k) = click {
        set(w, k);
        enter(w);
    }
}

/// Mog House Remodel (remodel.rs): "No changes" and the 2F styles, the patio grey without its key item.
fn remodel_list(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((rows, sel)) = remodel::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.focus_text(remodel::WINDOW, 0)[1].clone().unwrap_or_else(|| "Remodel".into());
    let mut got = None;
    p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: None }), W + 44.0, |ui| {
        got = plain_rows(ui, m, "remodel rows", &rows, sel, free);
    });
    act(w, got, remodel::set_sel, |w| remodel::key(w, egui::Key::Enter));
}

/// The Adventuring Primer (primer.rs): guide00's categories, guide01's topics beside them with its arrows (the other
/// categories); hidden while a page is up in the map window.
fn primer_lists(p: &mut Panels, w: &mut World) {
    let Some(v) = primer::look(w).filter(|v| !v.page) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = w.battle.as_ref().and_then(|b| b.group9(0xc5)).unwrap_or_else(|| "Help Desk".into());
    let width = W + if v.list.is_some() { 16.0 + W } else { 0.0 } + 44.0;
    let (mut cats, mut topics, mut turn) = (None, None, None);
    let mut found = small_found(p, w, &[Small::PrimerCats, Small::PrimerTopics]);
    p.panel("window", Some(Title { icon: Some("icons/help"), text: &title, note: None }), width, |ui| {
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(W);
                let rows: Vec<(String, bool)> = v.cats.iter().map(|c| (c.clone(), false)).collect();
                cats = plain_rows(ui, m, "primer cats", &rows, v.cursor, free && v.list.is_none());
            });
            let Some((name, list, cursor)) = &v.list else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(W);
                ui.horizontal(|ui| {
                    if button(ui, m, "<", 28.0, false, false) && free {
                        turn = Some(egui::Key::ArrowLeft);
                    }
                    ui.label(egui::RichText::new(name).size(m.theme.small).color(m.theme.color("gold")));
                    if button(ui, m, ">", 28.0, false, false) && free {
                        turn = Some(egui::Key::ArrowRight);
                    }
                });
                ui.add_space(4.0);
                let rows: Vec<(String, bool)> = list.iter().map(|t| (t.clone(), false)).collect();
                topics = plain_rows(ui, m, "primer topics", &rows, *cursor, free);
            });
        });
    });
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, p.gu);
        return;
    }
    if let Some(k) = turn {
        primer::key(w, k, false);
        return;
    }
    act(w, cats, primer::set_cat, |w| primer::key(w, egui::Key::Enter, false));
    act(w, topics, primer::set_topic, |w| primer::key(w, egui::Key::Enter, false));
}

/// A retail tint (0x80 is full) as a colour.
fn tint_colour(t: u32) -> Color32 {
    let c = |s: u32| ((t >> s & 0xff) * 2).min(255) as u8;
    Color32::from_rgb(c(16), c(8), c(0))
}

/// The Linkshell Concierge (concierge.rs): the event's title, lscswin's filters as tabs, the header and every shell
/// (its icon in the shell's colour, the name, green for ours, the language, the members wanted).
fn concierge_list(p: &mut Panels, w: &mut World) {
    const LIST_W: f32 = 460.0;
    let Some(gu) = p.gu else { return };
    let Some(v) = concierge::look(w) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let info = w.item_info.clone();
    let free = w.confirm.is_none();
    let focused = free && v.lsc.is_none();
    let (mut tab, mut click) = (None, None);
    let mut found = small_found(p, w, &[Small::Concierge]);
    let title = gu.focus_text(super::super::linkshell::LIST, 0)[1].clone().unwrap_or_else(|| "Linkshell".into());
    p.panel("window", Some(Title { icon: Some("icons/linkshell"), text: &title, note: None }), LIST_W + 44.0, |ui| {
        if !v.title.is_empty() {
            ui.label(egui::RichText::new(&v.title).size(m.theme.ui).color(m.theme.color("gold")));
            ui.add_space(6.0);
        }
        let labels: Vec<String> = [715usize, 92, 93, 574].iter().map(|&s| m.caption(gu, "menu    windowps", s).unwrap_or_default()).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let tabs = super::tabs::strip(ui, m, "concierge", &labels, Some((v.filter as usize).saturating_sub(1)), v.lsc.map(|s| (s as usize).saturating_sub(1)));
        if let Some(k) = tabs.iter().position(|r| r.clicked())
            && free
        {
            tab = Some(k as u16 + 1);
        }
        ui.add_space(6.0);
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        let seen = egui::Id::unique("concierge cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(v.cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, v.cursor));
        egui::ScrollArea::vertical().id_salt("concierge rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
            for (k, r) in v.rows.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == v.cursor && focused => "controls/list_row_selected",
                    _ if k == v.cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
                let y = rect.center().y;
                let small = FontId::proportional(m.theme.small);
                match r {
                    None => {
                        let head = m.caption(gu, "menu    windowps", 718).unwrap_or_default();
                        ui.painter().text(pos2(rect.min.x + 24.0, y), Align2::LEFT_CENTER, head, small, m.theme.color("gold"));
                    }
                    Some(r) => {
                        if let Some(t) = info.as_ref().and_then(|i| m.item_icon(ctx, i, r.item)) {
                            let ir = egui::Rect::from_center_size(pos2(rect.min.x + 30.0, y), vec2(20.0, 20.0));
                            ui.painter().image(t, ir, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint_colour(r.tint));
                        }
                        let nc = if r.ours { tint_colour(0x8050_9050) } else { m.theme.color("text") };
                        ui.painter().text(pos2(rect.min.x + 48.0, y), Align2::LEFT_CENTER, &r.name, small.clone(), nc);
                        ui.painter().text(pos2(rect.max.x - 70.0, y), Align2::CENTER_CENTER, r.lang, small.clone(), m.theme.color("text"));
                        ui.painter().text(pos2(rect.max.x - 14.0, y), Align2::RIGHT_CENTER, r.wanted.to_string(), FontId::monospace(m.theme.small), m.theme.color("text"));
                    }
                }
                if k == v.cursor && follow {
                    ui.scroll_to_rect(rect, None);
                }
                if focused && resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
        });
    });
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    if let Some(k) = tab {
        concierge::pick_filter(w, gu, k);
        return;
    }
    if let Some(k) = click {
        if concierge::set_row(w, k) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
    if click.is_some() {
        concierge::key(w, gu, egui::Key::Enter);
    }
}

/// The Fishing Ranking (ranking.rs): every row with the type's text cells (the rank, name, score and the date or
/// "[a-b-c]"; the job cell and the fishing rank's icon as text), the cursor's kept in view; a click moves the cursor.
/// Escape closes it as in retail.
fn fishing_ranking(p: &mut Panels, w: &mut World) {
    const LIST_W: f32 = 460.0;
    let Some((kind, cursor, rows)) = ranking::look(w) else { return };
    let (_, cells) = ranking::layout(kind);
    let lines: Vec<(u8, String, String, String)> = rows
        .iter()
        .map(|r| {
            let score = cells.iter().flatten().find(|c| matches!(c, ranking::Cell::Score(_))).and_then(|&c| ranking::text(w, c, r)).unwrap_or_default();
            let extra = cells
                .iter()
                .flatten()
                .filter_map(|&c| match c {
                    ranking::Cell::Job => {
                        let j = |k: u8| super::super::items::JOBS.get(k as usize).copied().unwrap_or("");
                        Some(format!("{}{}/{}{}  R{}", j(r.job.0), r.level.0, j(r.job.1), r.level.1, r.fish_rank))
                    }
                    ranking::Cell::Icon => Some(format!("R{}", r.fish_rank)),
                    ranking::Cell::Triple | ranking::Cell::VanaDate | ranking::Cell::EarthDate { .. } => ranking::text(w, c, r),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("  ");
            (r.rank, r.name.clone(), score, extra)
        })
        .collect();
    let m = p.m;
    let focused = w.confirm.is_none();
    let mut click = None;
    let mut found = small_found(p, w, &[Small::Ranking]);
    p.panel("window", Some(Title { icon: None, text: "Fishing Ranking", note: None }), LIST_W + 44.0, |ui| {
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        let seen = egui::Id::unique("ranking cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
        egui::ScrollArea::vertical().id_salt("ranking rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
            for (k, (rank, name, score, extra)) in lines.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == cursor && focused => "controls/list_row_selected",
                    _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
                let y = rect.center().y;
                let small = FontId::proportional(m.theme.small);
                let mono = FontId::monospace(m.theme.small);
                let rc = if *rank > 10 { m.theme.color("muted") } else { m.theme.color("gold") };
                ui.painter().text(pos2(rect.min.x + 36.0, y), Align2::RIGHT_CENTER, rank.to_string(), mono.clone(), rc);
                let clip = egui::Rect::from_min_max(rect.min, pos2(rect.min.x + 190.0, rect.max.y));
                ui.painter().with_clip_rect(clip).text(pos2(rect.min.x + 48.0, y), Align2::LEFT_CENTER, name, small.clone(), m.theme.color("text"));
                ui.painter().text(pos2(rect.min.x + 196.0, y), Align2::LEFT_CENTER, extra, small.clone(), m.theme.color("muted"));
                ui.painter().text(pos2(rect.max.x - 14.0, y), Align2::RIGHT_CENTER, score, mono, m.theme.color("text"));
                if k == cursor && follow {
                    ui.scroll_to_rect(rect, None);
                }
                if focused && resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
        });
    });
    if let Some(f) = found {
        f.pick(w, p.gu);
        return;
    }
    if let Some(k) = click.filter(|&k| k != cursor) {
        ranking::set_cursor(w, k);
        w.sounds.push(Se::Cursor as u32);
    }
}

/// The comment editor (seacom.rs): three lines of 40, the edited one live with its caret; a click picks a line, OK
/// closes as Escape does (the text kept and sent).
fn comment(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((mode, lines, line)) = seacom::view(w) else { return };
    let m = p.m;
    let blink = (p.ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
    let title = if mode == 0 { gu.title_msg(0x0f).unwrap_or_else(|| "Set Bazaar".into()) } else { "Comment".to_string() };
    let help = gu.menu_msg(0x32, "");
    let (mut pick, mut done) = (None, false);
    p.panel("comment", Some(Title { icon: None, text: &title, note: None }), 420.0 + 44.0, |ui| {
        if !help.is_empty() {
            ui.label(egui::RichText::new(&help).size(m.theme.small).color(m.theme.color("muted")));
            ui.add_space(4.0);
        }
        for (k, l) in lines.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(420.0, ROW), Sense::click());
            let face = if k == line { "controls/list_row_selected" } else { "controls/list_row_normal" };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let text = if k == line && blink { format!("{l}|") } else { l.clone() };
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, FontId::monospace(m.theme.small), m.theme.color("text"));
            if resp.clicked() {
                pick = Some(k);
            }
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
        let ok = m.caption(gu, "menu    keytops3", 53).unwrap_or_else(|| "OK".into());
        done = button(ui, m, &ok, 110.0, false, false);
    });
    if let Some(k) = pick {
        seacom::set_edit_line(w, k);
    }
    if done {
        seacom::right_click(w);
    }
}

/// The names, the cursor kept in view; Delete under them while blkmain is up.
fn block_list(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((rows, cursor, main, empty)) = blklist::view(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.focus_text(blklist::LIST, 1)[1].clone().unwrap_or_else(|| "Blacklist".into());
    let (mut click, mut delete) = (None, false);
    let mut found = small_found(p, w, &[Small::Blocks]);
    p.panel("blklist", Some(Title { icon: Some("icons/party"), text: &title, note: None }), W + 44.0, |ui| {
        if super::lists::rows_or(ui, m, &mut found) {
            return;
        }
        if !empty.is_empty() {
            ui.label(egui::RichText::new(&empty).size(m.theme.small).color(m.theme.color("muted")));
        }
        let seen = egui::Id::unique("blklist cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
        let focused = free && !main;
        egui::ScrollArea::vertical().id_salt("blklist rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
            for (k, name) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == cursor && focused => "controls/list_row_selected",
                    _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), m.theme.color("text"));
                if k == cursor && follow {
                    ui.scroll_to_rect(r, None);
                }
                if focused && resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
        });
        if main {
            ui.add_space(8.0);
            if button(ui, m, &caption(gu, blklist::MAIN, 1, "Delete"), 130.0, true, false) && free {
                delete = true;
            }
        }
    });
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    if delete {
        blklist::key(w, egui::Key::Enter);
        return;
    }
    if let Some(k) = click {
        if blklist::set_cursor(w, k) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
    if click.is_some() {
        blklist::key(w, egui::Key::Enter);
    }
}

/// chatctrl's rows as a column (its links walk them as drawn), the values at the right; subwindo's range under them.
fn chat_mode(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = chatctrl::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.focus_text(chatctrl::WINDOW, v.sel)[1].clone().unwrap_or_else(|| "Chat".into());
    let mut got = None;
    p.panel("chatctrl", Some(Title { icon: None, text: &title, note: None }), social::width(1), |ui| {
        let col = MenuWin { name: chatctrl::WINDOW, sel: v.sel, off: v.off.clone(), labels: Vec::new(), marked: Vec::new(), values: v.values.clone() };
        got = social::column(ui, m, gu, &col, free);
        ui.add_space(6.0);
        let head = m.caption(gu, "menu    windowps", 283).unwrap_or_default();
        let range = m.caption(gu, "menu    windowps", v.range).unwrap_or_default();
        ui.label(egui::RichText::new(head).size(m.theme.small).color(m.theme.color("gold")));
        ui.label(egui::RichText::new(range).size(m.theme.small).color(m.theme.color("text")));
    });
    if free {
        social::act(w, gu, got, chatctrl::set_sel, |w, _| chatctrl::key(w, egui::Key::Enter));
    }
}

/// Gardening (garden.rs): the plnt window as a column, bankmenu beside it, the pot / seed / crystal list in their place
/// while it is open.
fn garden_windows(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = garden::look(w) else { return };
    if v.win.is_none() && v.bank.is_none() && v.list.is_none() {
        return;
    }
    let m = p.m;
    let ctx = p.ctx;
    let info = w.item_info.clone();
    let free = w.confirm.is_none();
    // 0xd 0x59 "Gardening".
    let title = gu.title_msg(0x59).filter(|t| !t.is_empty()).unwrap_or_else(|| "Gardening".into());
    let wide = if v.list.is_some() { 360.0 } else { social::width(1 + v.bank.is_some() as usize) - 44.0 };
    let (mut win, mut bank, mut row) = (None, None, None);
    let mut found = small_found(p, w, &[Small::Garden]);
    p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: None }), wide + 44.0, |ui| {
        if let (Some((rows, tints, cur)), Some(info)) = (&v.list, &info) {
            if super::lists::rows_or(ui, m, &mut found) {
                return;
            }
            row = super::craft::list(ui, m, ctx, info, "garden list", rows, tints, *cur, free, &[]);
            return;
        }
        ui.horizontal_top(|ui| {
            if let Some((name, sel)) = v.win {
                let col = MenuWin { name, sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                win = social::column(ui, m, gu, &col, free && v.bank.is_none());
            }
            if let Some((sel, off)) = &v.bank {
                ui.add_space(12.0);
                let col = MenuWin { name: garden::BANK, sel: *sel, off: off.clone(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                bank = social::column(ui, m, gu, &col, free);
            }
        });
    });
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    let enter = |w: &mut World, _: &super::super::game_ui::GameUi| garden::key(w, egui::Key::Enter);
    social::act(w, gu, bank, garden::set_bank, enter);
    social::act(w, gu, win, garden::set_win, enter);
    act(w, row, garden::set_row, |w| garden::key(w, egui::Key::Enter));
}

/// Layout mode (layout.rs): the rmlo window as a column, bankmenu beside it, the furniture list (installed here green,
/// locked grey) in their place while it is open.
fn layout_windows(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = layout::look(w) else { return };
    if v.win.is_none() && v.bank.is_none() && v.list.is_none() {
        return;
    }
    let m = p.m;
    let ctx = p.ctx;
    let info = w.item_info.clone();
    let free = w.confirm.is_none();
    // Title slot 0 in layout mode (layout::bars).
    let title = gu.title_msg(4).filter(|t| !t.is_empty()).unwrap_or_else(|| "Layout".into());
    let wide = if v.list.is_some() { 360.0 } else { social::width(1 + v.bank.is_some() as usize) - 44.0 };
    let (mut win, mut bank, mut row) = (None, None, None);
    let mut found = small_found(p, w, &[Small::Layout]);
    p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: None }), wide + 44.0, |ui| {
        if let (Some((rows, tints, cur)), Some(info)) = (&v.list, &info) {
            if super::lists::rows_or(ui, m, &mut found) {
                return;
            }
            row = super::craft::list(ui, m, ctx, info, "layout furniture", rows, tints, *cur, free, &[]);
            return;
        }
        ui.horizontal_top(|ui| {
            if let Some((name, sel, off)) = &v.win {
                let col = MenuWin { name, sel: *sel, off: off.clone(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                win = social::column(ui, m, gu, &col, free && v.bank.is_none());
            }
            if let Some((sel, off)) = &v.bank {
                ui.add_space(12.0);
                let col = MenuWin { name: layout::BANK, sel: *sel, off: off.clone(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                bank = social::column(ui, m, gu, &col, free);
            }
        });
    });
    if !free {
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    let enter = |w: &mut World, _: &super::super::game_ui::GameUi| {
        layout::key(w, egui::Key::Enter);
    };
    social::act(w, gu, bank, layout::set_bank, enter);
    social::act(w, gu, win, layout::set_win, enter);
    act(w, row, layout::set_row, |w| {
        layout::key(w, egui::Key::Enter);
    });
}

/// pupequip (a PUP's main menu Equipment row): Equipment and Customize automaton as a column.
fn pup_menu(p: &mut Panels, w: &mut World) {
    // The automaton screen suspends it (the window walk).
    if w.pupset.is_some() {
        return;
    }
    let Some(gu) = p.gu else { return };
    let Some((name, sel)) = equipwin::pup_look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.title_msg(0x14).unwrap_or_else(|| "Equipment".into());
    let mut got = None;
    p.panel("window", Some(Title { icon: Some("icons/equipment"), text: &title, note: None }), social::width(1), |ui| {
        let col = MenuWin { name, sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
        got = social::column(ui, m, gu, &col, free);
    });
    if free {
        social::act(w, gu, got, equipwin::set_pup, |w, gu| equipwin::pup_key(w, gu, egui::Key::Enter));
    }
}

/// The fish's gauge while fishing (FUN_10202af0): "Fish" over its bar at retail's place, the screen's centre at 0.2 H;
/// grey at 0.
fn fish_gauge(p: &mut Panels, w: &mut World) {
    const GW: f32 = 240.0;
    let Some(pct) = fishing::gauge(w) else { return };
    let m = p.m;
    let screen = p.ctx.content_rect();
    let at = pos2(screen.center().x - (GW + 44.0) / 2.0, screen.min.y + screen.height() * 0.2);
    p.panel_at("fishgauge", None, GW + 44.0, at, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(GW, 33.0), Sense::hover());
        let bar = egui::Rect::from_min_max(pos2(r.min.x, r.max.y - super::hud::BAR), r.max);
        let c = if pct > 0 { m.theme.color("text") } else { m.theme.color("muted") };
        super::hud::labelled_bar(ui, m, "hp", bar, pct as f32 / 100.0, ("Fish", c), ("", c));
    });
}
