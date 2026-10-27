//! The modern treasure pool over client/treasure.rs, which keeps every rule and key: the pool's rows (icon, name, count,
//! the best lot and its lotter), Cast Lot / Pass on the cursor's row (lootope's choices, lit while it is open), and
//! lnowin's pass on the rest.

use super::super::{World, treasure};
use super::craft::button;
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};

const W: f32 = 420.0;
const ROW: f32 = 44.0;
const ICON: f32 = 28.0;
/// loot's row tints: lotted orange, passed grey.
const LOTTED: u32 = 0x8080_4020;

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(v), Some(info)) = (treasure::view(w), w.item_info.clone()) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let note = format!("{} / {}", v.rows.len(), treasure::SLOTS);
    let (mut hover, mut click, mut act, mut rest) = (None, None, None, false);
    p.panel("loot", Some(Title { icon: Some("icons/currency"), text: "Treasure Pool", note: Some(&note) }), W, |ui| {
        for (k, r) in v.rows.iter().enumerate() {
            let row = k as u16 + 1;
            let on = row == v.cursor && !v.done;
            let (rect, resp) = ui.allocate_exact_size(vec2(W - 28.0, ROW), Sense::click());
            let face = match () {
                _ if on => "controls/list_row_selected",
                _ if resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, rect, (rect.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            let it = info.item(r.item);
            if let Some(t) = m.item_icon(ctx, &info, r.item) {
                let tint = if r.own_lot == -1 { Color32::from_gray(110) } else { Color32::WHITE };
                ui.painter().image(t, Rect::from_min_size(pos2(rect.min.x + 10.0, rect.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
            }
            let colour = match r.own_lot {
                -1 => m.theme.color("muted"),
                0 => m.theme.color("text"),
                _ => super::chat::colour(LOTTED.to_le_bytes()),
            };
            let mut name = it.as_ref().map(|i| i.name.clone()).unwrap_or_default();
            if it.as_ref().is_some_and(|i| i.stack > 1) {
                name = format!("{name}  x{}", r.qty as u8);
            }
            let text_x = rect.min.x + 48.0;
            let clip = Rect::from_min_max(rect.min, pos2(rect.max.x - 150.0, rect.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(text_x, rect.min.y + 13.0), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), colour);
            let best = match &r.high {
                Some((lot, who)) if !who.is_empty() => format!("Best lot {lot} ({who})"),
                Some((lot, _)) => format!("Best lot {lot}"),
                None => "No lots yet".into(),
            };
            ui.painter().with_clip_rect(clip).text(pos2(text_x, rect.max.y - 12.0), Align2::LEFT_CENTER, best, FontId::proportional(m.theme.small), m.theme.color("muted"));
            // Our part at the right: the lot cast or passed, or on the cursor's row lootope's two choices.
            let buttons = on && r.own_lot != -1;
            let mine = match r.own_lot {
                -1 => Some(("Passed".to_string(), m.theme.color("muted"))),
                0 => None,
                n => Some((format!("Your lot {n}"), super::chat::colour(LOTTED.to_le_bytes()))),
            };
            if let Some((t, c)) = mine.as_ref().filter(|_| !buttons) {
                ui.painter().text(pos2(rect.max.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, t, FontId::proportional(m.theme.small), *c);
            }
            if buttons {
                let ope = v.ope.filter(|o| o.0 == row);
                let lotted = r.own_lot > 0;
                let at = Rect::from_min_size(pos2(rect.max.x - 146.0, rect.center().y - 14.0), vec2(140.0, 28.0));
                let mut b = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
                if lotted {
                    b.allocate_ui(vec2(70.0, 28.0), |ui| {
                        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(format!("Lot {}", r.own_lot)).size(m.theme.small).color(super::chat::colour(LOTTED.to_le_bytes()))));
                    });
                } else if button(&mut b, m, "Lot", 66.0, ope.is_some_and(|o| o.1 == 1), false) {
                    act = Some((row, true));
                }
                if button(&mut b, m, "Pass", 66.0, ope.is_some_and(|o| o.1 == 2), false) {
                    act = Some((row, false));
                }
            }
            if free && resp.hovered() && moved {
                hover = Some(row);
            }
            if free && resp.clicked() {
                click = Some(row);
            }
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if button(ui, m, "Pass on the rest", 170.0, v.done, false) {
                rest = true;
            }
        });
    });
    if !free {
        return;
    }
    if let Some((row, lot)) = act {
        treasure::act(w, row, lot);
        return;
    }
    if rest {
        treasure::pass_rest(w);
        return;
    }
    if let Some(row) = hover.filter(|_| v.ope.is_none())
        && treasure::set_cursor(w, row)
    {
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(row) = click {
        if treasure::set_cursor(w, row) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
}
