//! The modern Items window over the retail one's state (client/items.rs keeps the list, iuse, sortyn, itmsort2, Manual
//! Sort and every rule), and the item info panel for whatever item the game describes (the list's, a shop's, the
//! auction's, a trade's ...). The mouse is the kit's: hovering only lights a row, a click puts the cursor on it and
//! takes it; the wheel scrolls.

use super::super::items::{self as gitems, SortWin, Sub, Win};
use super::super::scrollbar::{Msg, Owner};
use super::super::{World, iteminfo};
use super::{Panels, Title, chat, lists};
use crate::inventory::{TINT_WHITE, VISIBLE_ROWS};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const ROW: f32 = 34.0;
const ICON: f32 = 24.0;

pub fn draw(p: &mut Panels, w: &mut World) {
    // The Maze Tabula editor takes the item list's place while it is up.
    if w.maze.screen.is_some() {
        return;
    }
    list(p, w);
    info(p, w);
}

/// A row's text colour: the game's tint, the theme's text for plain white.
fn tint(m: &super::Modern, t: u32) -> Color32 {
    if t == TINT_WHITE { m.theme.color("text") } else { chat::colour(t.to_le_bytes()) }
}

/// The kit's button, the keyboard cursor on it while `lit`; true when clicked (and not off).
fn button(ui: &mut Ui, m: &super::Modern, label: &str, lit: bool, off: bool) -> bool {
    let s = super::kit::Btn { lit: lit && !off, cursor: lit, off };
    super::kit::clicked(&super::kit::button(ui, m, label, 0.0, s), s)
}

/// What the mouse did this frame: a row clicked, a strip button, a question's answer, the wheel's notches.
#[derive(Default)]
struct Mouse {
    click: Option<usize>,
    sort: Option<u16>,
    ask: Option<bool>,
    iuse: Option<u16>,
    drop: Option<bool>,
    wheel: f32,
}

/// The Items list (or the action menu's direct-use list): the rows from the scroll with icons and quantities in the
/// game's colours, Manual Sort's held row; itmsort2 as a strip (Auto-sort and its question, Manual Sort, the Recycle
/// Bin); iuse (Use / Drop) and the drop question under the rows.
fn list(p: &mut Panels, w: &mut World) {
    let Some(Win::Items { scroll, cursor, sub, direct, sort, manual, store: None }) = w.win.as_ref() else { return };
    let (scroll, cursor, sub, direct, sort, manual) = (*scroll, *cursor, *sub, *direct, *sort, *manual);
    // The item's target prompt is up: retail hides the list meanwhile.
    if gitems::item_prompt(w) {
        return;
    }
    let Some(info) = w.item_info.clone() else { return };
    let m = p.m;
    let gu = p.gu;
    let shown = gitems::listed(w, direct);
    let rows = gitems::list_rows(w, &info, &shown, direct);
    let note = format!("{} / {}", shown.len(), w.inv.sizes[gitems::INVENTORY as usize].saturating_sub(1));
    let icons: Vec<Option<egui::TextureId>> = rows.iter().skip(scroll).take(VISIBLE_ROWS).map(|r| m.item_icon(p.ctx, &info, r.id)).collect();
    let held = manual.flatten();
    let msg = |g: u8, k: usize, or: &str| gu.map_or(or.to_string(), |gu| gu.lobby_msg(g, k, or));
    let mut mouse = Mouse::default();
    let search = lists::search_hits(p, w);
    let mut found = lists::Mouse::default();
    let title = Title { icon: None, text: "Items", note: (!direct).then_some(note.as_str()) };
    p.panel("window", Some(title), 380.0, |ui| {
        if let Some((text, sel, hits, icons)) = &search {
            found = lists::search_rows(ui, m, w, icons, text, *sel, hits);
            return;
        }
        if !direct {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (child, label) in [(1u16, "Auto-sort"), (2, "Manual Sort"), (3, "Recycle Bin")] {
                    let lit = sort.is_some_and(|s| s.cursor == child) || (child == 2 && manual.is_some());
                    if button(ui, m, label, lit, sub.is_some()) {
                        mouse.sort = Some(child);
                    }
                }
            });
            if let Some(yes) = sort.and_then(|s| s.ask) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(msg(0x0c, 0x3d, "Sort items?")).size(m.theme.small).color(m.theme.color("muted")));
                    for (answer, label) in [(true, "Yes"), (false, "No")] {
                        if button(ui, m, label, yes == answer, false) {
                            mouse.ask = Some(answer);
                        }
                    }
                });
            }
            ui.add_space(6.0);
        }
        let top = ui.cursor().min;
        if rows.is_empty() {
            ui.label(egui::RichText::new("No items.").size(m.theme.ui).color(m.theme.color("muted")));
        }
        for (k, (row, slot)) in rows.iter().zip(&shown).skip(scroll).take(VISIBLE_ROWS).enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let sprite = match () {
                _ if k == cursor && sort.is_none() => "controls/list_row_selected",
                // Hovering lights a row only while the list has the focus (iuse and itmsort2 take it).
                _ if held == Some(slot.index) || (resp.hovered() && sub.is_none() && sort.is_none()) => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            let painter = ui.painter();
            super::rows::face(painter, m, r, sprite, Color32::WHITE);
            if let Some(t) = icons.get(k).copied().flatten() {
                let at = pos2(r.min.x + 10.0, r.center().y - ICON / 2.0);
                painter.image(t, Rect::from_min_size(at, vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            painter.text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, &row.name, FontId::proportional(m.theme.ui), tint(m, row.tint));
            if let Some(n) = &row.count {
                painter.text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, n, FontId::proportional(m.theme.ui), m.theme.color("text"));
            }
            if resp.clicked() {
                mouse.click = Some(k);
            }
            ui.add_space(2.0);
        }
        // The bar: the rows shown out of all, at the right edge.
        let track = Rect::from_min_max(pos2(ui.max_rect().max.x + 6.0, top.y), pos2(ui.max_rect().max.x + 14.0, ui.cursor().min.y));
        super::kit::bar(ui.painter(), m, track, rows.len(), VISIBLE_ROWS, scroll);
        match sub {
            Some(Sub::Use { child, index, id }) => {
                let on = gitems::iuse_states(w, index, id);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (c, label) in [(1u16, "Use"), (2, "Drop")] {
                        if button(ui, m, label, child == c, !on[c as usize - 1]) {
                            mouse.iuse = Some(c);
                        }
                    }
                });
            }
            Some(Sub::Drop { yes, .. }) => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(msg(0x0c, 0x3e, "Dispose of item?")).size(m.theme.small).color(m.theme.color("muted")));
                    for (answer, label) in [(true, "Yes"), (false, "No")] {
                        if button(ui, m, label, yes == answer, false) {
                            mouse.drop = Some(answer);
                        }
                    }
                });
            }
            None => {}
        }
        if manual.is_some() && sort.is_none() {
            let hint = gu.map_or(String::new(), |gu| gu.menu_msg(if held.is_some() { 6 } else { 5 }, ""));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(hint).size(m.theme.small).color(m.theme.color("gold")));
        }
        if ui.rect_contains_pointer(ui.max_rect()) {
            // The wheel's own events (line notches; pixel deltas as 50 points a notch), not egui's smoothed scroll.
            mouse.wheel = ui.input(|i| {
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
    if let Some((_, _, hits, _)) = &search {
        lists::search_mouse(w, &found, hits);
        return;
    }
    apply(w, mouse, sort, sub);
}

fn enter(w: &mut World) {
    gitems::key(w, egui::Key::Enter, false, false);
}

fn win_mut(w: &mut World) -> Option<(&mut usize, &mut Option<Sub>, &mut Option<SortWin>)> {
    match w.win.as_mut() {
        Some(Win::Items { cursor, sub, sort, .. }) => Some((cursor, sub, sort)),
        _ => None,
    }
}

/// The list's mouse through retail's own keys: a button sets its window's cursor, then Enter.
fn apply(w: &mut World, mouse: Mouse, sort: Option<SortWin>, sub: Option<Sub>) {
    if let Some(yes) = mouse.drop {
        if let Some((_, Some(Sub::Drop { yes: y, .. }), _)) = win_mut(w) {
            *y = yes;
        }
        return enter(w);
    }
    if let Some(c) = mouse.iuse {
        if let Some((_, Some(Sub::Use { child, .. }), _)) = win_mut(w) {
            *child = c;
        }
        return enter(w);
    }
    if let Some(yes) = mouse.ask {
        if let Some((_, _, Some(s))) = win_mut(w) {
            s.ask = Some(yes);
        }
        return enter(w);
    }
    if sub.is_some() {
        return;
    }
    if let Some(c) = mouse.sort {
        if let Some((_, _, s)) = win_mut(w) {
            *s = Some(SortWin { cursor: c, ask: None });
        }
        return enter(w);
    }
    if sort.is_some() {
        return;
    }
    if let Some(k) = mouse.click
        && let Some((cursor, ..)) = win_mut(w)
    {
        *cursor = k;
        enter(w);
    }
    // A notch of the wheel is a row.
    if mouse.wheel != 0.0 {
        let msg = if mouse.wheel > 0.0 { Msg::Up } else { Msg::Down };
        for _ in 0..(mouse.wheel.abs().round() as usize).clamp(1, 5) {
            gitems::bar(w, Owner::Items, msg);
        }
    }
}

/// The item info panel for the item the game describes now: the icon, the name in its colour with the Aug / Rare / Ex
/// / Temp marks, the second line, the description, the jobs, the augments, the extra line, the signature and the item
/// level (iteminfo::build, wrapped by egui rather than paged); the equipped item it is compared with above it.
fn info(p: &mut Panels, w: &World) {
    let (Some(gu), Some(cache)) = (p.gu, w.item_info.clone()) else { return };
    let Some((id, ext, container, equipped, _, flags)) = super::super::item_info_source(w) else { return };
    let Some(it) = cache.item(id) else { return };
    let m = p.m;
    let now = super::super::vana_now(w) as u32;
    let aug = gu.aug_ctx();
    let build = |it: &crate::dat::items::Item, ext: Option<&[u8; 24]>, cx: &iteminfo::Ctx| iteminfo::build(it, ext, cx, &|g, k| gu.item_msg(g, k), &|_| 0.0, aug.as_ref());
    let voucher = super::super::maze::tabula_voucher(w, &it, ext.as_ref());
    let info = build(&it, ext.as_ref(), &iteminfo::Ctx { now, equipped, shop: false, container, flags, voucher });
    let cmp = gitems::compare(w).flatten().and_then(|(cid, cext, c)| {
        let cit = cache.item(cid)?;
        Some((cid, build(&cit, cext.as_ref(), &iteminfo::Ctx { now, equipped: true, shop: false, container: Some(c), flags: 0, voucher: None })))
    });
    let icon = m.item_icon(p.ctx, &cache, id);
    let cmp_icon = cmp.as_ref().and_then(|(cid, _)| m.item_icon(p.ctx, &cache, *cid));
    p.panel("iteminfo", None, 420.0, |ui| {
        if let Some((_, ci)) = &cmp {
            body(ui, m, ci, cmp_icon);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
        }
        body(ui, m, &info, icon);
    });
}

/// One item's description in the panel.
fn body(ui: &mut Ui, m: &super::Modern, info: &iteminfo::Info, icon: Option<egui::TextureId>) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    if let Some(t) = icon {
        ui.painter().image(t, Rect::from_min_size(pos2(r.min.x, r.center().y - 16.0), vec2(32.0, 32.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    let font = FontId::new(m.theme.title, FontFamily::Name(super::STRONG.into()));
    let name = ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, &info.name, font, tint(m, info.name_tint));
    // The marks as tags after the name (windowps 293 Aug, 294 Rare, 295 Ex, 296 Tmp, 842 Alt).
    let mut x = name.max.x + 8.0;
    for mark in &info.marks {
        let label = match mark {
            293 => "Aug",
            294 => "Rare",
            295 => "Ex",
            296 => "Temp",
            842 => "Alt",
            _ => continue,
        };
        let f = FontId::new(m.theme.small - 2.0, FontFamily::Name(super::STRONG.into()));
        let tw = ui.fonts_mut(|fo| fo.layout_no_wrap(label.to_string(), f.clone(), Color32::WHITE).size().x);
        let tag = Rect::from_min_size(pos2(x, r.center().y - 9.0), vec2(tw + 10.0, 18.0));
        ui.painter().rect_filled(tag, 4.0, m.theme.color("charcoal"));
        ui.painter().rect_stroke(tag, 4.0, egui::Stroke::new(1.0, m.theme.color("bronze")), egui::StrokeKind::Inside);
        ui.painter().text(tag.center(), Align2::CENTER_CENTER, label, f, m.theme.color("gold"));
        x = tag.max.x + 4.0;
    }
    // Game text: its element gaiji as the pack's icons.
    let line = |ui: &mut Ui, s: &str, size: f32, c: Color32| super::gametext::label(ui, m, s, size, c);
    if !info.line2.is_empty() {
        line(ui, &info.line2.join("  "), m.theme.small, m.theme.color("muted"));
    }
    ui.add_space(4.0);
    for d in &info.desc {
        line(ui, d, m.theme.ui, m.theme.color("text"));
    }
    if !info.jobs.is_empty() {
        ui.add_space(4.0);
        line(ui, &info.jobs.join(" "), m.theme.small, m.theme.color("muted"));
    }
    for a in &info.augs {
        line(ui, a, m.theme.small, m.theme.color("focus"));
    }
    for (s, c) in [(&info.extra, tint(m, info.extra_tint)), (&info.sig, m.theme.color("muted")), (&info.ilvl, m.theme.color("gold"))] {
        if !s.is_empty() {
            line(ui, s, m.theme.small, c);
        }
    }
}
