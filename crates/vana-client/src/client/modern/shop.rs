//! The modern NPC and guild shops and Set Bazaar's prices over client/shop.rs, which keeps every rule and key: Buy /
//! Sell as tabs (shopmain / guildsho), the whole list with its prices and requirements, the quantity, the price and the
//! confirm asked inside the panel.

use super::super::game_ui::GameUi;
use super::super::items::ItemCache;
use super::super::shop::{self, Look, Req, View};
use super::super::World;
use super::craft::{button, caption, count_row, money_row, tint};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

const W: f32 = 470.0;
const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
const TALL: f32 = 12.0 * (ROW + 2.0);
const SHOPMAIN: &str = "menu    shopmain";
const GUILDSHO: &str = "menu    guildsho";
const SHOPBUY: &str = "menu    shopbuy";
const SHOPSELL: &str = "menu    shopsell";

/// What the mouse did this frame.
#[derive(Default)]
struct Mouse {
    tab: Option<u16>,
    hover: Option<usize>,
    click: Option<usize>,
    confirm: Option<u16>,
    key: Option<egui::Key>,
    money: Option<u32>,
}

/// A requirement cell's words (FUN_101f4bb0): the rank's word, the job's name and level, or the line of group 0xc.
fn req_text(m: &super::Modern, gu: &GameUi, r: Req) -> String {
    match r {
        Req::Rank { need, .. } => m.caption(gu, "menu    keytops3", 192 + need as usize).unwrap_or_default(),
        Req::Job { job, level } => format!("{} {level}", m.caption(gu, "menu    windowps", 212 + job as usize).unwrap_or_default()),
        Req::Text(info) => gu.item_msg(0xc, 0x11d + info as usize).unwrap_or_default(),
    }
}

/// The list: every row (icon, name in its tint, the requirement, the price or count at the right), the cursor's kept
/// in view as it moves.
fn list(ui: &mut Ui, m: &super::Modern, gu: &GameUi, info: &ItemCache, v: &Look, focused: bool, mouse: &mut Mouse) {
    let ctx = ui.ctx().clone();
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let seen = egui::Id::unique("modern shop cursor seen");
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(v.cursor);
    ctx.data_mut(|d| d.insert_temp(seen, v.cursor));
    egui::ScrollArea::vertical().id_salt("shop rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, r) in v.rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(W - 42.0, ROW), Sense::click());
            let face = match () {
                _ if k == v.cursor && focused => "controls/list_row_selected",
                _ if k == v.cursor || resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            let name_tint = tint(m, r.tint);
            if r.id != 0
                && let Some(t) = m.item_icon(&ctx, info, r.id)
            {
                let dim = if r.tint == crate::inventory::TINT_DIM { Color32::from_gray(110) } else { Color32::WHITE };
                ui.painter().image(t, Rect::from_min_size(pos2(rect.min.x + 10.0, rect.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), dim);
            }
            let name = r.text.clone().unwrap_or_else(|| info.item(r.id).map(|it| it.name.clone()).unwrap_or_default());
            let right = rect.max.x - 110.0;
            let clip = Rect::from_min_max(rect.min, pos2(right - 6.0, rect.max.y));
            let x = if r.id != 0 { rect.min.x + 42.0 } else { rect.min.x + 14.0 };
            ui.painter().with_clip_rect(clip).text(pos2(x, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), name_tint);
            if let Some(req) = r.req {
                ui.painter().text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, req_text(m, gu, req), FontId::proportional(m.theme.small), name_tint);
            }
            let cell = r.price.clone().or_else(|| r.count.map(|n| n.to_string()));
            if let Some(c) = cell {
                ui.painter().text(pos2(rect.max.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, c, FontId::proportional(m.theme.ui), tint(m, r.price_tint));
            }
            if k == v.cursor && follow {
                ui.scroll_to_rect(rect, None);
            }
            if focused && resp.hovered() && moved {
                mouse.hover = Some(k);
            }
            if focused && resp.clicked() {
                mouse.click = Some(k);
            }
            ui.add_space(2.0);
        }
        let note = if v.loading { Some("Preparing data...") } else if v.rows.is_empty() { Some("Nothing to list.") } else { None };
        if let Some(n) = note {
            ui.label(egui::RichText::new(n).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
}

/// The confirm (shopbuy / shopsell): what it is for, then Yes / No with the cursor's lit.
fn confirm(ui: &mut Ui, m: &super::Modern, gu: &GameUi, info: &ItemCache, v: &Look, (n, child): (u32, u16), mouse: &mut Mouse) {
    let menu = if v.view == Some(View::Buy) { SHOPBUY } else { SHOPSELL };
    let gil = |g: u64| crate::inventory::gil_text(g as i64);
    let (item, unit) = v.busy.unwrap_or((0, None));
    let name = info.item(item).map(|it| it.name.clone()).unwrap_or_default();
    let line = match v.view {
        Some(View::Bazaar) => format!("Put {name} up for {}?", gil(n as u64)),
        Some(View::Buy) => match unit {
            Some(p) => format!("Buy {n} x {name} for {}?", gil(p as u64 * n as u64)),
            None => format!("Buy {n} x {name}?"),
        },
        _ => match unit {
            Some(p) => format!("Sell {n} x {name} for {}?", gil(p as u64 * n as u64)),
            None => format!("Sell {n} x {name}?"),
        },
    };
    ui.label(egui::RichText::new(line).size(m.theme.ui).color(m.theme.color("gold")));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (k, d) in [(1u16, "Yes"), (2, "No")] {
            if button(ui, m, &caption(gu, menu, k, d), 110.0, child == k, false) {
                mouse.confirm = Some(k);
            }
        }
    });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    let Some(v) = shop::look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let asking = v.qty.is_some() || v.money.is_some() || v.confirm.is_some();
    let title = match () {
        _ if v.bazaar => gu.title_msg(0x0f).unwrap_or_else(|| "Set Bazaar".into()),
        _ if v.guild => gu.title_msg(0xd).unwrap_or_else(|| "Guild Shop".into()),
        _ => gu.title_msg(0xc).unwrap_or_else(|| "Shop".into()),
    };
    let gil = crate::inventory::gil_text(w.inv.gil() as i64);
    let mut mouse = Mouse::default();
    p.panel("window", Some(Title { icon: Some("icons/currency"), text: &title, note: Some(&gil) }), W, |ui| {
        // shopmain / guildsho: Buy and Sell as tabs; the open list's lit, else the cursor's.
        if !v.bazaar {
            let main = if v.guild { GUILDSHO } else { SHOPMAIN };
            ui.horizontal(|ui| {
                for (k, d) in [(1u16, "Buy"), (2, "Sell")] {
                    let open = match v.view {
                        Some(View::Buy) => k == 1,
                        Some(_) => k == 2,
                        None => v.main == k,
                    };
                    if button(ui, m, &caption(gu, main, k, d), 120.0, open, false) {
                        mouse.tab = Some(k);
                    }
                }
            });
            ui.add_space(6.0);
        }
        if v.view.is_some() {
            list(ui, m, gu, &info, &v, free && !asking, &mut mouse);
        } else {
            ui.label(egui::RichText::new("Buy or sell?").size(m.theme.small).color(m.theme.color("muted")));
        }
        if let Some((n, max)) = v.qty {
            ui.add_space(8.0);
            mouse.key = count_row(ui, m, gu, &gu.title_msg(0x3e).unwrap_or_default(), n, max);
        }
        if let Some(money) = v.money {
            ui.add_space(8.0);
            (mouse.key, mouse.money) = money_row(ui, m, gu, money);
        }
        if let Some(c) = v.confirm {
            ui.add_space(8.0);
            confirm(ui, m, gu, &info, &v, c, &mut mouse);
        }
    });
    if !free {
        return;
    }
    let cursor = |w: &mut World| w.sounds.push(Se::Cursor as u32);
    if let Some(k) = mouse.tab {
        // Another tab while a list is open goes back to shopmain first (Escape), then opens it.
        if v.view.is_some() && !asking {
            shop::key(w, egui::Key::Escape);
        }
        if shop::set_main(w, k) {
            cursor(w);
        }
        if shop::look(w).is_some_and(|l| l.view.is_none() && l.confirm.is_none()) {
            shop::key(w, egui::Key::Enter);
        }
        return;
    }
    if let Some(k) = mouse.confirm {
        if shop::set_confirm(w, k) {
            cursor(w);
        }
        shop::key(w, egui::Key::Enter);
        return;
    }
    if let Some(val) = mouse.money {
        shop::set_money(w, val);
        cursor(w);
    }
    if let Some(k) = mouse.key {
        shop::key(w, k);
        return;
    }
    if let Some(k) = mouse.hover
        && shop::set_row(w, k)
    {
        cursor(w);
    }
    if let Some(k) = mouse.click {
        shop::set_row(w, k);
        shop::key(w, egui::Key::Enter);
    }
}
