//! The modern Synthesis and trade windows over client/synth.rs and trade.rs, which keep every rule and key: the slots
//! as tiles, the inventory picker as one list of every row, the count or gil asked inside the panel.

use super::super::game_ui::GameUi;
use super::super::items::ItemCache;
use super::super::{World, synth, trade};
use super::{MenuWin, Panels, Title, paint, social};
use crate::inventory::{Slot, TINT_DIM, TINT_WHITE};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

const TILE: f32 = 52.0;
const GAP: f32 = 8.0;
const GRID_W: f32 = 4.0 * TILE + 3.0 * GAP;
const LIST_W: f32 = 330.0;
const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
/// The crystal list's yellow (FUN_101e2a40, item type 8).
const CRYSTAL: u32 = 0x8090_9040;
const ELEMENTS: [&str; 8] = ["icons/fire", "icons/ice", "icons/wind", "icons/earth", "icons/lightning", "icons/water", "icons/light", "icons/dark"];

/// What the mouse did this frame: a tile or a list row hovered (the pointer moved) or clicked, a button's key.
#[derive(Default)]
struct Mouse {
    tile: (Option<u16>, Option<u16>),
    theirs: Option<u16>,
    row: (Option<usize>, Option<usize>),
    key: Option<egui::Key>,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    if synth::active(w) {
        synthesis(p, w, gu, &info);
    } else if trade::active(w) {
        trading(p, w, gu, &info);
    }
}

/// A row's text colour: the module's tint, the theme's text for plain white, muted for grey.
pub(super) fn tint(m: &super::Modern, t: u32) -> Color32 {
    match t {
        TINT_WHITE => m.theme.color("text"),
        TINT_DIM => m.theme.color("muted"),
        t => super::chat::colour(t.to_le_bytes()),
    }
}

/// A slot tile: its item's icon and count, the cursor's ring (bronze while another part has the focus), a red cross
/// over a material not held.
#[allow(clippy::too_many_arguments)]
pub(super) fn tile(ui: &mut Ui, m: &super::Modern, r: Rect, id: egui::Id, icon: Option<egui::TextureId>, count: u32, cursor: Option<bool>, missing: bool) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    let face = match () {
        _ if missing => "controls/item_slot_invalid",
        _ if cursor.is_some() => "controls/item_slot_selected",
        _ if resp.hovered() => "controls/item_slot_hover",
        _ => "controls/item_slot_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
    }
    if let Some(focus) = cursor {
        let c = if focus { m.theme.color("focus") } else { m.theme.color("bronze") };
        ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, c), egui::StrokeKind::Inside);
    }
    if let Some(t) = icon {
        ui.painter().image(t, r.shrink(10.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if count > 1 {
        super::paint::outlined(ui.painter(), r.max - vec2(6.0, 4.0), Align2::RIGHT_BOTTOM, &count.to_string(), FontId::proportional(m.theme.small), m.theme.color("text"));
    }
    if missing {
        let c = r.shrink(16.0);
        let s = egui::Stroke::new(2.5, m.theme.color("hp"));
        ui.painter().line_segment([c.left_top(), c.right_bottom()], s);
        ui.painter().line_segment([c.right_top(), c.left_bottom()], s);
    }
    resp
}

/// Eight tiles in two rows of four: children 1..8. (hovered, clicked).
#[allow(clippy::too_many_arguments)]
fn grid(ui: &mut Ui, m: &super::Modern, ctx: &egui::Context, info: &ItemCache, salt: &str, slots: &[(u16, u32, bool); 8], cursor: Option<(u16, bool)>, sense: bool) -> (Option<u16>, Option<u16>) {
    let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 2.0 * TILE + GAP), Sense::hover());
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let mut out = (None, None);
    for (k, &(id, count, missing)) in slots.iter().enumerate() {
        let r = Rect::from_min_size(area.min + vec2((k % 4) as f32 * (TILE + GAP), (k / 4) as f32 * (TILE + GAP)), vec2(TILE, TILE));
        let child = k as u16 + 1;
        let icon = (id != 0).then(|| m.item_icon(ctx, info, id)).flatten();
        let at = cursor.filter(|c| c.0 == child).map(|c| c.1);
        let resp = tile(ui, m, r, egui::Id::unique((salt, k)), icon, count, at, missing);
        if sense && resp.hovered() && moved {
            out.0 = Some(child);
        }
        if sense && resp.clicked() {
            out.1 = Some(child);
        }
    }
    out
}

/// A button in the pack's frame: lit under the cursor, grey when off; true when clicked.
pub(super) fn button(ui: &mut Ui, m: &super::Modern, label: &str, width: f32, lit: bool, off: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    // The cursor still shows on a disabled button (dimmed), as retail's hand does.
    let (face, tint) = match () {
        _ if off && lit => ("controls/button_selected", Color32::from_gray(140)),
        _ if off => ("controls/button_disabled", Color32::WHITE),
        _ if lit => ("controls/button_selected", Color32::WHITE),
        _ if resp.hovered() => ("controls/button_hover", Color32::WHITE),
        _ => ("controls/button_normal", Color32::WHITE),
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, tint));
    }
    let c = if off { m.theme.color("muted") } else if lit { m.theme.color("gold") } else { m.theme.color("text") };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(m.theme.small), c);
    resp.clicked() && !off
}

/// An item list: every row with its icon, name, count and colour, the cursor's row kept in view. (hovered, clicked).
#[allow(clippy::too_many_arguments)]
pub(super) fn list(ui: &mut Ui, m: &super::Modern, ctx: &egui::Context, info: &ItemCache, salt: &str, rows: &[Slot], tints: &[u32], cur: usize, focused: bool, stars: &[bool]) -> (Option<usize>, Option<usize>) {
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (mut hover, mut click) = (None, None);
    let seen = egui::Id::unique((salt, "cursor seen"));
    let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cur);
    ui.ctx().data_mut(|d| d.insert_temp(seen, cur));
    let tall = 12.0 * (ROW + 2.0);
    egui::ScrollArea::vertical().id_salt(salt).max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
        for (k, s) in rows.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), Sense::click());
            let face = match () {
                _ if k == cur && focused => "controls/list_row_selected",
                _ if resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let t = tints.get(k).copied().unwrap_or(TINT_WHITE);
            if let Some(tex) = m.item_icon(ctx, info, s.id) {
                let dim = if t == TINT_DIM { Color32::from_gray(110) } else { Color32::WHITE };
                ui.painter().image(tex, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), dim);
            }
            let name = info.item(s.id).map(|it| it.name.clone()).unwrap_or_default();
            let clip = Rect::from_min_max(r.min, pos2(r.max.x - 44.0, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), tint(m, t));
            if s.count > 1 {
                ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, s.count.to_string(), FontId::proportional(m.theme.ui), m.theme.color("text"));
            }
            if stars.get(k).copied().unwrap_or(false) {
                ui.painter().add(paint::star(pos2(r.max.x - 20.0, r.center().y), 7.0, m.theme.color("gold")));
            }
            if k == cur && follow {
                ui.scroll_to_rect(r, None);
            }
            if resp.hovered() && moved {
                hover = Some(k);
            }
            if resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
        if rows.is_empty() {
            ui.label(egui::RichText::new("Empty.").size(m.theme.ui).color(m.theme.color("muted")));
        }
    });
    (hover, click)
}

/// The count asked (itemctrl's for a stack, or the amount to make): the value of most and its keys as buttons (itemctrl's
/// own "-", "+", "1", "All"), OK and Cancel.
pub(super) fn count_row(ui: &mut Ui, m: &super::Modern, gu: &GameUi, label: &str, v: u32, max: u32) -> Option<egui::Key> {
    use egui::Key as K;
    let mut key = None;
    ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("gold")));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{v} / {max}")).size(m.theme.ui).family(egui::FontFamily::Monospace).color(m.theme.color("text")));
        ui.add_space(6.0);
        for (text, k) in [("-", K::ArrowDown), ("+", K::ArrowUp), ("1", K::ArrowRight), ("All", K::ArrowLeft)] {
            if button(ui, m, text, 32.0, false, false) {
                key = Some(k);
            }
        }
    });
    ui.horizontal(|ui| {
        let (ok, cancel) = (caption(gu, trade::HANDOVER, 9, "OK"), caption(gu, trade::HANDOVER, 10, "Cancel"));
        if button(ui, m, &ok, 96.0, true, false) {
            key = Some(K::Enter);
        }
        if button(ui, m, &cancel, 96.0, false, false) {
            key = Some(K::Escape);
        }
    });
    key
}

/// The gil asked (moneyctr): the amount with the digit the arrows step underlined; its keys as buttons ("-" / "+" by
/// the step, the step x10 / /10), All and 0 outright, OK and Cancel. A key, or an amount to set.
/// `focus`: the button with the keys (0..5 the value buttons, 6 OK, 7 Cancel), None the value.
pub(super) fn money_row(ui: &mut Ui, m: &super::Modern, gu: &GameUi, (step, max, v): (u32, u32, u32), focus: Option<u8>) -> (Option<egui::Key>, Option<u32>) {
    use egui::Key as K;
    let (mut key, mut set) = (None, None);
    ui.label(egui::RichText::new("Gil").size(m.theme.small).color(m.theme.color("gold")));
    ui.horizontal(|ui| {
        let text = format!("{v:>width$}", width = max.to_string().len().max(step.to_string().len()));
        let font = FontId::monospace(m.theme.ui);
        let g = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), m.theme.color("text")));
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        let cw = g.size().x / text.chars().count().max(1) as f32;
        ui.painter().galley(r.min, g, m.theme.color("text"));
        // The step's digit, counted from the right.
        let k = step.max(1).ilog10() as f32;
        let x = r.max.x - (k + 1.0) * cw;
        let mark = if focus.is_none() { m.theme.color("focus") } else { m.theme.color("muted") };
        ui.painter().line_segment([pos2(x + 1.0, r.max.y), pos2(x + cw - 1.0, r.max.y)], egui::Stroke::new(2.0, mark));
        ui.label(egui::RichText::new(format!(" / {}", crate::inventory::gil_text(max as i64))).size(m.theme.small).color(m.theme.color("muted")));
    });
    ui.horizontal(|ui| {
        for (b, (text, k)) in [("-", K::ArrowDown), ("+", K::ArrowUp), ("x10", K::ArrowLeft), ("/10", K::ArrowRight)].into_iter().enumerate() {
            if button(ui, m, text, 40.0, focus == Some(b as u8), false) {
                key = Some(k);
            }
        }
        for (b, (text, to)) in [("All", max), ("0", 0)].into_iter().enumerate() {
            if button(ui, m, text, 40.0, focus == Some(4 + b as u8), false) {
                set = Some(to);
            }
        }
    });
    ui.horizontal(|ui| {
        let (ok, cancel) = (caption(gu, trade::HANDOVER, 9, "OK"), caption(gu, trade::HANDOVER, 10, "Cancel"));
        if button(ui, m, &ok, 96.0, focus.is_none_or(|b| b == 6), false) {
            key = Some(K::Enter);
        }
        if button(ui, m, &cancel, 96.0, focus == Some(7), false) {
            key = Some(K::Escape);
        }
    });
    (key, set)
}

pub(super) fn caption(gu: &GameUi, menu: &str, child: u16, fallback: &str) -> String {
    gu.child_caption(menu, child).unwrap_or_else(|| fallback.to_string())
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str, colour: Color32) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(egui::FontFamily::Name(super::STRONG.into())).color(colour));
    ui.add_space(4.0);
}

/// Synthesis: cmbmenu and the crystal list; the history with cmbhwin; tskill1 (the crystal, eight materials, OK and
/// Cancel) beside the picker.
fn synthesis(p: &mut Panels, w: &mut World, gu: &GameUi, info: &ItemCache) {
    let m = p.m;
    let ctx = p.ctx;
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let v = synth::view(w);
    let mut mouse = Mouse::default();
    let mut menu_got = (None, None);
    let mut fav_got: (Option<u16>, Option<u16>) = (None, None);
    let mut fav_open = false;
    let wide = v.crystals.is_some() || v.win.as_ref().is_some_and(|x| x.picker.is_some()) || v.history.is_some();
    let width = if wide { GRID_W + LIST_W + 70.0 } else { social::width(1) };
    let title = Title { icon: Some("icons/craft"), text: "Synthesis", note: None };
    p.panel("window", Some(title), width, |ui| {
        if let Some((sel, off, focused)) = v.menu {
            ui.horizontal_top(|ui| {
                let mw = MenuWin { name: synth::CMBMENU, sel, off: if off { vec![1, 2] } else { Vec::new() }, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                menu_got = social::stack(ui, m, gu, std::slice::from_ref(&mw), focused, moved);
                if let Some((rows, cur)) = &v.crystals {
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.set_width(LIST_W);
                        heading(ui, m, &caption(gu, synth::CMBMENU, 1, "Crystal Synthesis"), m.theme.color("gold"));
                        let tints = vec![CRYSTAL; rows.len()];
                        mouse.row = list(ui, m, ctx, info, "synth crystals", rows, &tints, *cur, w.confirm.is_none(), &[]);
                    });
                }
            });
            return;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(GRID_W);
                let Some(x) = &v.win else { return };
                // The crystal and its element.
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::hover());
                    let icon = (x.crystal != 0).then(|| m.item_icon(ctx, info, x.crystal)).flatten();
                    tile(ui, m, r, egui::Id::unique("synth crystal"), icon, 0, None, x.history && !x.found && x.crystal != 0);
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        let name = info.item(x.crystal).map(|it| it.name.clone()).unwrap_or_default();
                        ui.label(egui::RichText::new(name).size(m.theme.ui).color(m.theme.color("text")));
                        if let Some(s) = x.element.and_then(|e| ELEMENTS.get(e as usize)).and_then(|i| m.art.sprite(i)) {
                            let (er, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                            ui.painter().add(paint::image(&s, er.min, 20.0, if x.history && !x.found { Color32::from_gray(110) } else { Color32::WHITE }));
                        }
                    });
                });
                ui.add_space(GAP);
                let cursor = (1..=8).contains(&x.cursor).then_some((x.cursor, x.on_slots));
                mouse.tile = grid(ui, m, ctx, info, "synth slot", &x.slots, cursor, w.confirm.is_none());
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (ok, cancel) = (caption(gu, synth::TSKILL1, 9, "OK"), caption(gu, synth::TSKILL1, 10, "Cancel"));
                    if button(ui, m, &ok, 110.0, x.cursor == 9 && x.on_slots, !x.complete) {
                        mouse.tile.1 = Some(9);
                    }
                    if button(ui, m, &cancel, 110.0, x.cursor == 10 && x.on_slots, false) {
                        mouse.tile.1 = Some(10);
                    }
                });
                if let Some((n, max, amount)) = x.count {
                    ui.add_space(10.0);
                    let label = if amount { "How many to make".to_string() } else { gu.title_msg(0x3e).unwrap_or_default() };
                    mouse.key = count_row(ui, m, gu, &label, n, max);
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                // Mode 2: the history (newest first, the favourites starred) and cmbhwin under it.
                if let Some((records, cur, fav, focused)) = &v.history {
                    heading(ui, m, &caption(gu, synth::CMBMENU, 2, "History"), m.theme.color("gold"));
                    let rows: Vec<Slot> = records.iter().map(|r| Slot { index: 0, id: r.item, count: 0, equipped: false, equipped_ex: false }).collect();
                    // The favourites starred (retail's keytops3 101).
                    let stars: Vec<bool> = records.iter().map(|r| r.crystal & 0x8000 != 0).collect();
                    mouse.row = list(ui, m, ctx, info, "synth history", &rows, &vec![TINT_WHITE; rows.len()], (*cur as usize).saturating_sub(1), *focused && fav.is_none(), &stars);
                    // cmbhwin's two rows as buttons (Num+ gives them the keys).
                    ui.add_space(8.0);
                    fav_open = fav.is_some();
                    ui.horizontal(|ui| {
                        for (c, fallback) in [(1u16, "Favorite"), (2, "Unfavorite")] {
                            if button(ui, m, &caption(gu, synth::CMBHWIN, c, fallback), 140.0, *focused && *fav == Some(c), records.is_empty()) {
                                fav_got = (None, Some(c));
                            }
                        }
                    });
                } else if let Some((rows, tints, cur)) = v.win.as_ref().and_then(|x| x.picker.as_ref()) {
                    heading(ui, m, "Inventory", m.theme.color("gold"));
                    let focused = v.win.as_ref().is_some_and(|x| x.on_list);
                    mouse.row = list(ui, m, ctx, info, "synth picker", rows, tints, *cur, focused, &[]);
                }
            });
        });
    });
    let enter = |w: &mut World| synth::key(w, gu, egui::Key::Enter);
    if v.menu.is_some() {
        social::act(w, gu, menu_got, synth::set_menu, |w, gu| synth::key(w, gu, egui::Key::Enter));
        if let Some(k) = mouse.row.0
            && synth::set_crystal(w, k)
        {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        if let Some(k) = mouse.row.1 {
            synth::set_crystal(w, k);
            enter(w);
        }
        return;
    }
    // cmbhwin takes the focus as Num+ gives it, then its row.
    if let Some(c) = fav_got.1 {
        if !fav_open {
            synth::num_plus(w);
        }
        synth::set_history(w, c);
        enter(w);
    } else if let Some(c) = fav_got.0.filter(|_| fav_open)
        && synth::set_history(w, c)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if v.history.is_some() {
        // A history row: hover follows it as the keys do, a click is Enter (tskill1 on OK).
        if let Some(k) = mouse.row.0
            && synth::set_history(w, k as u16 + 1)
        {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        if let Some(k) = mouse.row.1 {
            synth::set_history(w, k as u16 + 1);
            enter(w);
        }
    } else {
        // A picker row puts its item in the cursor's slot (the first empty one from OK or Cancel).
        if let Some(k) = mouse.row.0.filter(|_| v.win.as_ref().is_some_and(|x| x.on_list))
            && synth::set_row(w, k)
        {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        if let Some(k) = mouse.row.1 {
            synth::set_row(w, k);
            enter(w);
        }
    }
    if let Some(c) = mouse.tile.0.filter(|_| v.win.as_ref().is_some_and(|x| x.on_slots))
        && synth::set_slot(w, c)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(c) = mouse.tile.1 {
        synth::set_slot(w, c);
        enter(w);
    }
    if let Some(k) = mouse.key {
        synth::key(w, gu, k);
    }
}

/// The hand-over and player trade: our eight slots, gil, OK and Cancel; theirs beside them; the picker.
fn trading(p: &mut Panels, w: &mut World, gu: &GameUi, info: &ItemCache) {
    let m = p.m;
    let ctx = p.ctx;
    let Some(v) = trade::view(w) else { return };
    let mut mouse = Mouse::default();
    let (mut gil_click, mut money_set) = (false, None);
    let name = if v.player { trade::TRADE } else { trade::HANDOVER };
    let gil_text = |g: u32| crate::inventory::gil_text(g as i64);
    let title = Title { icon: Some("icons/trade"), text: "Trade", note: Some(&v.with) };
    let free = w.confirm.is_none();
    let mine: [(u16, u32, bool); 8] = std::array::from_fn(|i| (v.mine[i].0, v.mine[i].1, false));
    p.panel("window", Some(title), GRID_W * if v.player { 2.0 } else { 1.0 } + LIST_W + if v.player { 96.0 } else { 70.0 }, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(GRID_W);
                heading(ui, m, &w.name, if v.our_ok { m.theme.color("gold") } else { m.theme.color("text") });
                let cursor = (1..=8).contains(&v.cursor).then_some((v.cursor, v.on_window));
                mouse.tile = grid(ui, m, ctx, info, "trade ours", &mine, cursor, free);
                ui.add_space(8.0);
                // Child 11: the gil, a click asks the amount.
                ui.horizontal(|ui| {
                    if let Some(s) = m.art.sprite("icons/currency") {
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        ui.painter().add(paint::image(&s, r.min, 22.0, Color32::WHITE));
                    }
                    if button(ui, m, &gil_text(v.gil), GRID_W - 30.0, v.cursor == 11 && v.on_window, v.our_ok) {
                        gil_click = true;
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (ok, cancel) = (caption(gu, name, 9, "OK"), caption(gu, name, 10, "Cancel"));
                    if button(ui, m, &ok, 110.0, v.cursor == 9 && v.on_window, v.our_ok) {
                        mouse.tile.1 = Some(9);
                    }
                    if button(ui, m, &cancel, 110.0, v.cursor == 10 && v.on_window, false) {
                        mouse.tile.1 = Some(10);
                    }
                });
                if let Some((n, max)) = v.qty {
                    ui.add_space(10.0);
                    mouse.key = count_row(ui, m, gu, &gu.title_msg(0x3e).unwrap_or_default(), n, max);
                }
                if let Some(money) = v.money {
                    ui.add_space(10.0);
                    (mouse.key, money_set) = money_row(ui, m, gu, money, None);
                }
            });
            // Their offer: their OK lights their name.
            if let Some((theirs, gil, ok)) = &v.theirs {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(GRID_W);
                    heading(ui, m, &v.with, if *ok { m.theme.color("gold") } else { m.theme.color("text") });
                    let slots: [(u16, u32, bool); 8] = std::array::from_fn(|i| (theirs[i].0, theirs[i].1, false));
                    let cursor = v.gift.map(|g| (g, free));
                    let got = grid(ui, m, ctx, info, "trade theirs", &slots, cursor, free && !v.our_ok);
                    mouse.theirs = got.0.or(got.1);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(gil_text(*gil)).size(m.theme.ui).color(m.theme.color("text")));
                    if *ok {
                        ui.label(egui::RichText::new("Ready.").size(m.theme.small).color(m.theme.color("gold")));
                    }
                });
            }
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                heading(ui, m, "Inventory", m.theme.color("gold"));
                let (rows, tints, cur) = &v.picker;
                mouse.row = list(ui, m, ctx, info, "trade picker", rows, tints, *cur, v.on_list, &[]);
            });
        });
    });
    let enter = |w: &mut World| trade::key(w, gu, egui::Key::Enter);
    if let Some(c) = mouse.tile.0.filter(|_| v.on_window)
        && trade::set_cursor(w, c)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(c) = mouse.tile.1 {
        if trade::set_cursor(w, c) || v.cursor == c {
            enter(w);
        }
    } else if gil_click && trade::set_cursor(w, 11) | (v.cursor == 11) {
        enter(w);
    }
    if let Some(g) = mouse.theirs
        && trade::set_gift(w, g)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = mouse.row.0.filter(|_| v.on_list)
        && trade::set_row(w, k)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = mouse.row.1
        && (trade::set_row(w, k) || v.on_list)
    {
        enter(w);
    }
    if let Some(v) = money_set {
        trade::set_money(w, v);
    }
    if let Some(k) = mouse.key {
        trade::key(w, gu, k);
    }
}
