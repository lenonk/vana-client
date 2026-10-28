//! The modern Set Spells screen over client/bluset.rs, which keeps every rule and key: the points in the title, the
//! slots (their numbers in the band colours) and the known spells side by side with every row, the spell's help under
//! them; Refine by Type (bluesibo) as a list in the spells' place while it is open.

use super::super::bluset::{self, RowLook};
use super::super::World;
use super::super::game_ui::GameUi;
use super::craft::tint;
use super::{Panels, STRONG, Title};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const COL_W: f32 = 300.0;
const ROW: f32 = 28.0;
const ICON: f32 = 20.0;
const TALL: f32 = 12.0 * (ROW + 2.0);

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(4.0);
}

/// A list's rows: the slot number, the spell's icon (`icons`: the room for one), the text in its tint; the cursor's
/// kept in view. Hover only on the list with the keys (`focused`), clicks where `clicks`. (hovered, clicked).
fn rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, salt: &str, rows: &[RowLook], cursor: usize, (focused, clicks, icons): (bool, bool, bool)) -> (Option<usize>, Option<usize>) {
    let ctx = ui.ctx().clone();
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let seen = egui::Id::unique((salt, "bluset cursor seen"));
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
    ctx.data_mut(|d| d.insert_temp(seen, cursor));
    let (mut hover, mut click) = (None, None);
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, r) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(COL_W - 14.0, ROW), if clicks { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == cursor && focused => "controls/list_row_selected",
                _ if k == cursor || clicks && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            let y = rect.center().y;
            let mut x = rect.min.x + 24.0;
            if let Some((n, band)) = r.number {
                ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, n.to_string(), FontId::monospace(m.theme.small), tint(m, band));
                x += 22.0;
            }
            if let Some(t) = r.icon.and_then(|i| m.sprite_icon(&ctx, gu, "menu    magiconw", i)) {
                ui.painter().image(t, Rect::from_center_size(pos2(x + ICON / 2.0, y), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            if icons {
                x += ICON + 6.0;
            }
            let clip = Rect::from_min_max(rect.min, pos2(rect.max.x - 8.0, rect.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(x, y), Align2::LEFT_CENTER, &r.text, FontId::proportional(m.theme.small), tint(m, r.tint));
            if k == cursor && follow {
                ui.scroll_to_rect(rect, None);
            }
            if focused && resp.hovered() && moved {
                hover = Some(k);
            }
            if clicks && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    (hover, click)
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    let Some(v) = bluset::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let open = free && v.sibo.is_none();
    let note = format!("{}/{}", v.used, v.total);
    let title = gu.title_msg(0x8d).unwrap_or_else(|| "Blue Magic".into());
    let (mut equip, mut inven, mut sibo) = ((None, None), (None, None), (None, None));
    p.panel("window", Some(Title { icon: Some("icons/magic"), text: &title, note: Some(&note) }), 2.0 * COL_W + 16.0 + 44.0, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                heading(ui, m, "Set Spells");
                // The other list's rows take a click only while nothing is picked (its focus moves first).
                equip = rows(ui, m, gu, "bluset slots", &v.equip.0, v.equip.1, (open && v.on_equip, open && (v.on_equip || !v.picking), true));
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                match &v.sibo {
                    Some((families, r)) => {
                        heading(ui, m, &gu.focus_text("menu    bluesibo", 0)[1].clone().unwrap_or_else(|| "Type".into()));
                        let fam: Vec<RowLook> = families.iter().map(|f| RowLook { number: None, icon: None, text: f.clone(), tint: crate::inventory::TINT_WHITE }).collect();
                        sibo = rows(ui, m, gu, "bluset sibo", &fam, *r as usize - 1, (free, free, false));
                    }
                    None => {
                        heading(ui, m, "Spells");
                        inven = rows(ui, m, gu, "bluset spells", &v.inven.0, v.inven.1, (open && !v.on_equip, open && (!v.on_equip || !v.picking), true));
                    }
                }
            });
        });
        if let Some([name, head, desc, foot]) = &v.help {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).size(m.theme.small).family(FontFamily::Name(STRONG.into())).color(m.theme.color("gold")));
                ui.label(egui::RichText::new(head).size(m.theme.small).color(m.theme.color("muted")));
            });
            super::gametext::label(ui, m, desc, m.theme.small, m.theme.color("text"));
            ui.label(egui::RichText::new(foot).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
    if !free {
        return;
    }
    let cursor = |w: &mut World| w.sounds.push(Se::Cursor as u32);
    if let Some(k) = sibo.0
        && bluset::set_sibo(w, k as u16 + 1)
    {
        cursor(w);
    }
    if let Some(k) = sibo.1 {
        bluset::set_sibo(w, k as u16 + 1);
        bluset::key(w, egui::Key::Enter);
        return;
    }
    for (list, on_equip) in [(equip, true), (inven, false)] {
        if let Some(k) = list.0
            && bluset::set_row(w, on_equip, k)
        {
            cursor(w);
        }
        if let Some(k) = list.1 {
            bluset::set_row(w, on_equip, k);
            bluset::key(w, egui::Key::Enter);
            return;
        }
    }
}
