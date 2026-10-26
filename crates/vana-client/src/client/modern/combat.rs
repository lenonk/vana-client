//! The modern combat window: the retail action menu (`World::action_menu`, playermo / mp_pmode) in the pack's
//! buttons, with icons; its keys stay the retail menu's. Hover moves the cursor, a click runs the command (through
//! `World::action_click`), the arrow of a command with a Right function (Call for Help / Disengage swap, Magic's
//! magselec, Chat's chat mode) runs that.

use super::super::{World, combat, monstrosity};
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, pos2, vec2};

/// A command's label (the retail label sprites, LABELS at 0x103634f8) and the pack's icon.
fn command(c: u8) -> (&'static str, &'static str) {
    match c {
        1 | 0x15 | 0x41 => ("Attack", "icons/attack"),
        2 => ("Call for Help", "icons/warning"),
        3 | 0x45 => ("Disengage", "icons/disengage"),
        4 | 5 | 0x40 => ("Chat", "icons/chat"),
        6 => ("Talk", "icons/emote"),
        7 | 0x44 => ("Abilities", "icons/abilities"),
        8 | 0x43 => ("Magic", "icons/magic"),
        9 => ("Items", "icons/items"),
        0xa | 0xb | 0x46 => ("Check", "icons/check"),
        0xc | 0x47 => ("Treasure", "icons/currency"),
        0xd => ("Fish", "icons/craft"),
        0xe => ("Dig", "icons/materials"),
        0xf => ("Dismount", "icons/mount"),
        0x10 => ("Invite", "icons/party_invite"),
        0x11 => ("Disband", "icons/cancel"),
        0x12 | 0x42 => ("Switch Target", "icons/target_next"),
        0x13 | 0x14 => ("Trade", "icons/trade"),
        0x16 => ("Quarry", "icons/aggro"),
        0x17 => ("Scout", "icons/find"),
        0x18 => ("Sprint", "icons/haste"),
        0x19 => ("Trust", "icons/party"),
        0x1a => ("Release", "icons/cancel"),
        0x1b => ("Request", "icons/party_invite"),
        _ => ("", "icons/help"),
    }
}

/// The commands with a Right function (the retail labels' arrows).
fn has_right(c: u8) -> bool {
    matches!(c, 2 | 3 | 4 | 5 | 8 | 0x40 | 0x43)
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(menu) = w.action_menu.clone().filter(|_| w.prompt.is_none()) else { return };
    let m = p.m;
    let engaged = w.my_motion.status == 1;
    let target = w.target.and_then(|t| if t == w.my_id { Some(monstrosity::my_name(w).to_string()) } else { w.entities.get(&t).map(|e| monstrosity::name(e).to_string()) });
    let title = if engaged { "Battle" } else { "Commands" };
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (mut hovered, mut clicked, mut arrow) = (None, None, None);
    p.panel("combat", Some(Title { icon: Some("icons/attack"), text: title, note: target.as_deref() }), 270.0, [0.62, 0.3], |ui| {
        for (k, &c) in menu.items.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
            let sprite = if k == menu.sel {
                "controls/button_selected"
            } else if resp.hovered() {
                "controls/button_hover"
            } else {
                "controls/button_normal"
            };
            let painter = ui.painter();
            if let Some(s) = m.art.sprite(sprite) {
                painter.add(paint::nine_slice(&s, r, r.height() / s.size.y, Color32::WHITE));
            }
            let (label, icon) = command(c);
            // Disengage reads Call for Help once swapped (DAT_10578174).
            let (label, icon) = if c == 3 && menu.call_help { command(2) } else { (label, icon) };
            if let Some(s) = m.art.sprite(icon) {
                painter.add(paint::image(&s, pos2(r.min.x + 8.0, r.center().y - 12.0), 24.0, Color32::WHITE));
            }
            painter.text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, label, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), m.theme.color("text"));
            let mut on_arrow = false;
            if has_right(c)
                && let Some(s) = m.art.sprite("controls/arrow_right")
            {
                let a = Rect::from_min_size(pos2(r.max.x - 30.0, r.center().y - 9.0), vec2(18.0, 18.0));
                painter.add(paint::image(&s, a.min, 18.0, Color32::WHITE));
                on_arrow = resp.hover_pos().is_some_and(|p| a.expand(4.0).contains(p));
            }
            if resp.hovered() {
                hovered = Some(k);
            }
            if resp.clicked() {
                if on_arrow { arrow = Some(k) } else { clicked = Some(k) }
            }
            ui.add_space(2.0);
        }
    });
    let Some(am) = w.action_menu.as_mut() else { return };
    if let Some(k) = hovered.filter(|&k| moved && k != am.sel) {
        am.sel = k;
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(k) = arrow {
        am.sel = k;
        combat::right(w);
    } else if let Some(k) = clicked {
        w.action_click = Some(k);
    }
}
