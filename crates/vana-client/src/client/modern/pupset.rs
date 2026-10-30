//! The modern automaton screen over client/pupset.rs, which keeps every rule and key: the automaton's status, the
//! head, frame and attachments with the element capacities, the owned parts beside them, the item's help under them;
//! Refine by Type (pupsibor) as a list in the parts' place while it is open.

use super::super::World;
use super::super::pupset;
use super::craft::tint;
use super::{Panels, STRONG, Title};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const COL_W: f32 = 280.0;
const STAT_W: f32 = 170.0;
const ROW: f32 = 28.0;
const ICON: f32 = 20.0;
const TALL: f32 = 14.0 * (ROW + 2.0);
const ELEMENTS: [&str; 8] = ["Fire", "Ice", "Wind", "Earth", "Lightning", "Water", "Light", "Dark"];

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(4.0);
}

/// A list's rows: the item's icon, the text in its tint; the cursor kept in view. The face lit only on the side with the keys
/// (`focused`), clicks where `clicks` The row clicked.
fn rows(ui: &mut Ui, m: &super::Modern, w: &World, salt: &str, rows: &[(String, Option<u16>, u32)], cursor: usize, (focused, clicks): (bool, bool)) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let seen = egui::Id::unique((salt, "pupset cursor seen"));
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
    ctx.data_mut(|d| d.insert_temp(seen, cursor));
    let mut click = None;
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, (text, item, t)) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(COL_W - 14.0, ROW), if clicks { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == cursor && focused => "controls/list_row_selected",
                _ if k == cursor || clicks && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            let y = rect.center().y;
            let x = rect.min.x + 18.0;
            if let Some(tex) = item.zip(w.item_info.as_ref()).and_then(|(id, info)| m.item_icon(&ctx, info, id)) {
                ui.painter().image(tex, Rect::from_center_size(pos2(x + ICON / 2.0, y), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let clip = Rect::from_min_max(rect.min, pos2(rect.max.x - 8.0, rect.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(x + ICON + 6.0, y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), tint(m, *t));
            if k == cursor && follow {
                ui.scroll_to_rect(rect, None);
            }
            if clicks && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    click
}

fn status(ui: &mut Ui, m: &super::Modern, v: &pupset::Look) {
    heading(ui, m, if v.name.is_empty() { "Automaton" } else { &v.name });
    for (label, value) in &v.status {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("muted")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).size(m.theme.small).color(m.theme.color("text")));
            });
        });
    }
    ui.add_space(8.0);
    heading(ui, m, "Capacity");
    for (e, &(used, cap)) in v.capacity.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(ELEMENTS[e]).size(m.theme.small).color(m.theme.color("muted")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let c = if cap < used { m.theme.color("hp") } else { m.theme.color("text") };
                ui.label(egui::RichText::new(format!("{used}/{cap}")).size(m.theme.small).color(c));
            });
        });
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    let Some(v) = pupset::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let open = free && v.sibo.is_none();
    let title = gu.title_msg(0x14).unwrap_or_else(|| "Equipment".into());
    let (mut grid, mut list, mut sibo) = (None, None, None);
    p.panel("window", Some(Title { icon: Some("icons/equipment"), text: &title, note: None }), STAT_W + 2.0 * COL_W + 32.0 + 44.0, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(STAT_W);
                status(ui, m, &v);
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                heading(ui, m, &gu.sprite_caption("menu    framesus", 125).unwrap_or_else(|| "Automaton".into()));
                // The other side's rows take a click only while nothing is picked (its focus moves first).
                grid = rows(ui, m, w, "pupset slots", &v.slots, v.slot_cursor, (open && v.on_grid, open && (v.on_grid || !v.picking)));
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                match &v.sibo {
                    Some((elements, r)) => {
                        heading(ui, m, &gu.focus_text("menu    pupsibor", 0)[1].clone().unwrap_or_else(|| "Element".into()));
                        let el: Vec<(String, Option<u16>, u32)> = elements.iter().enumerate().map(|(e, s)| (if s.is_empty() { ELEMENTS[e].to_string() } else { s.clone() }, None, crate::inventory::TINT_WHITE)).collect();
                        sibo = rows(ui, m, w, "pupset sibo", &el, *r as usize - 1, (free, free));
                    }
                    None => {
                        heading(ui, m, "Parts");
                        list = rows(ui, m, w, "pupset parts", &v.list, v.list_cursor, (open && !v.on_grid, open && (!v.on_grid || !v.picking)));
                    }
                }
            });
        });
        if let Some((name, costs, desc)) = &v.help {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).size(m.theme.small).family(FontFamily::Name(STRONG.into())).color(m.theme.color("gold")));
                let cost: Vec<&str> = costs.iter().map(|&e| ELEMENTS[e]).collect();
                if !cost.is_empty() {
                    ui.label(egui::RichText::new(cost.join(" ")).size(m.theme.small).color(m.theme.color("muted")));
                }
            });
            super::gametext::label(ui, m, desc, m.theme.small, m.theme.color("text"));
        }
    });
    if !free {
        return;
    }
    if let Some(k) = sibo {
        pupset::set_sibo(w, k as u16 + 1);
        pupset::key(w, gu, egui::Key::Enter);
        return;
    }
    for (side, on_grid) in [(grid, true), (list, false)] {
        if let Some(k) = side {
            pupset::set_row(w, on_grid, k);
            pupset::key(w, gu, egui::Key::Enter);
            return;
        }
    }
}
