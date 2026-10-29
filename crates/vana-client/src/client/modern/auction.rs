//! The modern auction house over client/auction.rs, which keeps every rule and key: the stack of its windows side by
//! side (the last three), the command menus as the Social pages draw retail's, the item list, the price history and the
//! inventory for sale as lists of every row; the price and shopsell asked under them.

use super::super::auction::{self, Look, Pane, PaneRow};
use super::super::game_ui::GameUi;
use super::super::items::ItemCache;
use super::super::World;
use super::craft::{button, caption, money_row, tint};
use super::{MenuWin, Panels, Title, social};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const MENU_W: f32 = 230.0;
const LIST_W: f32 = 400.0;
const HIST_W: f32 = 460.0;
const ROW: f32 = 28.0;
const ICON: f32 = 22.0;
const TALL: f32 = 12.0 * (ROW + 2.0);

/// What the mouse did on the top pane, the price and shopsell this frame.
#[derive(Default)]
struct Mouse {
    menu: (Option<u16>, Option<u16>),
    row: (Option<usize>, Option<usize>),
    key: Option<egui::Key>,
    money: Option<u32>,
    shopsell: Option<u16>,
}

fn width(p: &Pane) -> f32 {
    match p {
        Pane::Menu { .. } => MENU_W,
        Pane::List { .. } | Pane::Sell { .. } => LIST_W,
        Pane::History { .. } => HIST_W,
    }
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(4.0);
}

/// Item rows (the list, the inventory for sale): icon, the stack size, the name in its tint, the cell at the right; the
/// cursor's kept in view. (hovered, clicked) on the pane with the keys.
#[allow(clippy::too_many_arguments)]
fn item_rows(ui: &mut Ui, m: &super::Modern, info: &ItemCache, salt: &str, rows: &[PaneRow], cur: usize, focused: bool, note: Option<&str>) -> (Option<usize>, Option<usize>) {
    let ctx = ui.ctx().clone();
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let seen = egui::Id::unique((salt, "auction cursor seen"));
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(cur);
    ctx.data_mut(|d| d.insert_temp(seen, cur));
    let (mut hover, mut click) = (None, None);
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, r) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), Sense::click());
            let face = match () {
                _ if k == cur && focused => "controls/list_row_selected",
                _ if k == cur || resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            if let Some(t) = m.item_icon(&ctx, info, r.item) {
                ui.painter().image(t, Rect::from_min_size(pos2(rect.min.x + 8.0, rect.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let mut name = info.item(r.item).map(|it| it.name.clone()).unwrap_or_default();
            if let Some(n) = r.stack {
                name = format!("{name} x{n}");
            }
            let clip = Rect::from_min_max(rect.min, pos2(rect.max.x - 100.0, rect.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(rect.min.x + 38.0, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), tint(m, r.tint));
            ui.painter().text(pos2(rect.max.x - 10.0, rect.center().y), Align2::RIGHT_CENTER, &r.right, FontId::proportional(m.theme.ui), m.theme.color("text"));
            if k == cur && follow {
                ui.scroll_to_rect(rect, None);
            }
            if focused && resp.hovered() && moved {
                hover = Some(k);
            }
            if focused && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
        if let Some(n) = note {
            ui.label(egui::RichText::new(n).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
    (hover, click)
}

/// The price history: date, seller → buyer, price per sale.
fn history_rows(ui: &mut Ui, m: &super::Modern, rows: &[[String; 4]], cur: usize, focused: bool, note: Option<&str>) -> (Option<usize>, Option<usize>) {
    let moved = ui.ctx().input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (mut hover, mut click) = (None, None);
    for (k, r) in rows.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(HIST_W - 14.0, ROW - 4.0), Sense::click());
        let face = match () {
            _ if k == cur && focused => "controls/list_row_selected",
            _ if resp.hovered() => "controls/list_row_hover",
            _ => "controls/list_row_normal",
        };
        super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
        let (font, text) = (FontId::proportional(m.theme.small), m.theme.color("text"));
        let y = rect.center().y;
        ui.painter().text(pos2(rect.min.x + 10.0, y), Align2::LEFT_CENTER, &r[0], font.clone(), m.theme.color("muted"));
        ui.painter().text(pos2(rect.min.x + 150.0, y), Align2::LEFT_CENTER, format!("{}  →  {}", r[1], r[2]), font.clone(), text);
        ui.painter().text(pos2(rect.max.x - 10.0, y), Align2::RIGHT_CENTER, &r[3], font, text);
        if focused && resp.hovered() && moved {
            hover = Some(k);
        }
        if focused && resp.clicked() {
            click = Some(k);
        }
        ui.add_space(2.0);
    }
    if let Some(n) = note {
        ui.label(egui::RichText::new(n).size(m.theme.small).color(m.theme.color("muted")));
    }
    (hover, click)
}

/// The idle search field: "Search the auction house" until it is clicked or typed into. True when clicked.
fn search_box(ui: &mut Ui, m: &super::Modern) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(LIST_W), 30.0), Sense::click());
    let face = if resp.hovered() { "controls/input_focused" } else { "controls/input_normal" };
    if let Some(s) = m.art.sprite(face).or_else(|| m.art.sprite("controls/input_focused")) {
        ui.painter().add(super::paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    if let Some(s) = m.art.sprite("controls/search") {
        ui.painter().add(super::paint::image(&s, pos2(r.min.x + 8.0, r.center().y - 9.0), 18.0, Color32::WHITE));
    }
    ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, "Search the auction house", FontId::proportional(m.theme.ui), m.theme.color("muted"));
    resp.clicked()
}

/// One pane as a column.
fn pane(ui: &mut Ui, m: &super::Modern, gu: &GameUi, info: &ItemCache, p: &Pane, focused: bool, mouse: &mut Mouse) {
    let moved = ui.ctx().input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    ui.vertical(|ui| {
        ui.set_width(width(p));
        match p {
            Pane::Menu { name, sel, off, labels } => {
                let v = MenuWin { name, sel: *sel, off: off.clone(), labels: labels.clone(), marked: Vec::new(), values: Vec::new() };
                let got = social::column(ui, m, gu, &v, focused, moved);
                if focused {
                    mouse.menu = got;
                }
            }
            Pane::List { sales, rows, cursor, note } => {
                let t = if *sales { caption(gu, "menu    auc1", 3, "Sales Status") } else { gu.title_msg(0x79).unwrap_or_else(|| "Bid".into()) };
                heading(ui, m, &t);
                let got = item_rows(ui, m, info, "auction list", rows, *cursor, focused, note.as_deref());
                if focused {
                    mouse.row = got;
                }
            }
            Pane::History { item, rows, cursor, note } => {
                let name = info.item(*item).map(|it| it.name.clone()).unwrap_or_default();
                heading(ui, m, &format!("{}: {name}", gu.title_msg(0x77).unwrap_or_else(|| "Price History".into())));
                let got = history_rows(ui, m, rows, *cursor, focused, note.as_deref());
                if focused {
                    mouse.row = got;
                }
            }
            Pane::Sell { rows, cursor } => {
                heading(ui, m, &caption(gu, "menu    auc1", 2, "Sell"));
                let got = item_rows(ui, m, info, "auction sell", rows, *cursor, focused, rows.is_empty().then_some("Nothing to sell."));
                if focused {
                    mouse.row = got;
                }
            }
        }
    });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    let Some(v): Option<Look> = auction::look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let asking = v.money.is_some() || v.shopsell.is_some();
    let shown = v.panes.len().saturating_sub(3);
    let panes = &v.panes[shown..];
    let total: f32 = panes.iter().map(width).sum::<f32>() + 16.0 * panes.len().saturating_sub(1) as f32 + 44.0;
    let title = gu.title_msg(0x12).unwrap_or_else(|| "Auction".into());
    let gil = crate::inventory::gil_text(w.inv.gil() as i64);
    let mut mouse = Mouse::default();
    // Not retail: the search over every bid category while the bid menus have the keys (typing starts it too).
    let searchable = free && !v.busy && auction::searchable(w);
    let search = if searchable { super::lists::search_hits(p, w) } else { None };
    if search.is_some() {
        auction::index_build(w);
    }
    let loading = auction::index(w).is_none();
    let (mut found, mut start) = (super::lists::Mouse::default(), false);
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let wide = if search.is_some() { LIST_W + 44.0 } else { 0.0 };
    p.panel("window", Some(Title { icon: Some("icons/currency"), text: &title, note: Some(&gil) }), total.max(MENU_W + 44.0).max(wide), |ui| {
        if let Some((text, sel, hits, icons)) = search.as_ref().filter(|s| !s.0.is_empty()) {
            if loading {
                // Every category is asked once, slowly (the server limits its sessions); later searches are instant.
                super::lists::search_field(ui, m, text);
                let dots = ".".repeat(1 + (ui.input(|i| i.time) * 2.0) as usize % 3);
                ui.label(egui::RichText::new(format!("Searching{dots}")).size(m.theme.ui).color(m.theme.color("muted")));
                return;
            }
            found = super::lists::search_rows(ui, m, w, icons, text, *sel, hits, moved);
            return;
        }
        if searchable {
            // The field reached with Up and still empty: focused, the pages under it as they were.
            match &search {
                Some(_) => super::lists::search_field(ui, m, ""),
                None => start = search_box(ui, m),
            }
            ui.add_space(6.0);
        }
        ui.horizontal_top(|ui| {
            for (k, pn) in panes.iter().enumerate() {
                if k > 0 {
                    ui.add_space(16.0);
                }
                let top = k + 1 == panes.len();
                pane(ui, m, gu, &info, pn, top && free && !asking && !v.busy && search.is_none(), &mut mouse);
            }
        });
        if let Some((step, max, val, item)) = v.money {
            ui.add_space(10.0);
            let name = info.item(item).map(|it| it.name.clone()).unwrap_or_default();
            ui.label(egui::RichText::new(name).size(m.theme.ui).color(m.theme.color("gold")));
            (mouse.key, mouse.money) = money_row(ui, m, gu, (step, max, val), v.money_btn);
        }
        if let Some((sel, item, price)) = v.shopsell {
            ui.add_space(10.0);
            let name = info.item(item).map(|it| it.name.clone()).unwrap_or_default();
            ui.label(egui::RichText::new(format!("Put {name} up for {}?", crate::inventory::gil_text(price as i64))).size(m.theme.ui).color(m.theme.color("gold")));
            ui.horizontal(|ui| {
                for (k, d) in [(1u16, "Sell"), (2, "Cancel")] {
                    if button(ui, m, &caption(gu, "menu    shopsell", k, d), 110.0, sel == k, false) {
                        mouse.shopsell = Some(k);
                    }
                }
            });
        }
    });
    if !free {
        return;
    }
    if let Some((_, _, hits, _)) = search.as_ref().filter(|s| !s.0.is_empty()) {
        super::lists::search_mouse(w, &found, hits);
        return;
    }
    if search.is_some() {
        return;
    }
    if start {
        w.list_search = Some(Default::default());
        return;
    }
    let cursor = |w: &mut World| w.sounds.push(Se::Cursor as u32);
    if let Some(k) = mouse.shopsell {
        if auction::set_shopsell(w, k) {
            cursor(w);
        }
        auction::key(w, egui::Key::Enter);
        return;
    }
    if let Some(val) = mouse.money {
        auction::set_money(w, val);
        cursor(w);
    }
    if let Some(k) = mouse.key {
        auction::key(w, k);
        return;
    }
    if let Some(sel) = mouse.menu.0
        && auction::set_menu(w, sel)
    {
        cursor(w);
    }
    if let Some(sel) = mouse.menu.1 {
        auction::set_menu(w, sel);
        auction::key(w, egui::Key::Enter);
        return;
    }
    if let Some(k) = mouse.row.0
        && auction::set_row(w, k)
    {
        cursor(w);
    }
    if let Some(k) = mouse.row.1 {
        auction::set_row(w, k);
        auction::key(w, egui::Key::Enter);
    }
}
