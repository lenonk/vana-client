//! The modern combat window: the retail action menu (`World::action_menu`, playermo / mp_pmode) in the pack's
//! buttons, with icons; its keys stay the retail menu's. Hover moves the cursor, a click runs the command (through
//! `World::action_click`), the arrow of a command with a Right function (Call for Help / Disengage swap, Magic's
//! magselec, Chat's chat mode) runs that.

use super::super::magic::Kind;
use super::super::suggest::Action;
use super::super::{World, combat, listsearch, magic, monstrosity, suggest};
use super::{Panels, Title, paint};
use egui::{Align2, Color32, FontFamily, FontId, Sense, pos2, vec2};

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
    suggest::refresh_root(w);
    let Some(menu) = w.action_menu.clone().filter(|_| w.prompt.is_none()) else { return };
    let m = p.m;
    // Ours: the suggestions above the commands (suggest.rs); the cursor rides them as one ring with the commands.
    let sugg = w.root_sugg.clone();
    let on_sugg = w.suggest_sel.filter(|&k| k < sugg.len());
    let shown: Vec<(String, Option<egui::TextureId>, bool)> = sugg
        .iter()
        .map(|s| {
            let (kind, id) = match s.action {
                Action::Spell(id) => (Kind::Magic, id),
                Action::Ability(id) => (Kind::Ability, id),
            };
            // A family row reads the family's name ("Fire", "Protect"); its spells are in its submenu.
            let name = s.label.clone().unwrap_or_else(|| if kind == Kind::Magic { magic::spell_name(w, id) } else { magic::ability_name(w, id) });
            let label = match &s.target {
                Some(t) => format!("{name}  \u{2192} {t}"),
                None => name,
            };
            let icon = p.gu.and_then(|gu| magic::row_icon(w, kind, id).and_then(|(lib, i)| m.sprite_icon(p.ctx, gu, lib, i)));
            let off = match s.action {
                Action::Spell(id) => magic::magic_check(w, id).is_some(),
                Action::Ability(id) => magic::ability_check(w, id).is_some(),
            };
            (label, icon, off)
        })
        .collect();
    let mut s_click = None;
    let sub = on_sugg.zip(w.suggest_sub).filter(|(k, _)| sugg.get(*k).is_some_and(|s| s.group.len() > 1));
    let mut sub_row = None;
    let engaged = w.my_motion.status == 1;
    let target = w.target.and_then(|t| if t == w.my_id { Some(monstrosity::my_name(w).to_string()) } else { w.entities.get(&t).map(|e| monstrosity::name(e).to_string()) });
    let title = if engaged { "Battle" } else { "Commands" };
    let (mut clicked, mut arrow) = (None, None);
    let width = if sugg.is_empty() { 270.0 } else { 360.0 };
    let panel = p.panel("combat", Some(Title { icon: Some("icons/attack"), text: title, note: target.as_deref(), name: true }), width, |ui| {
        if !sugg.is_empty() {
            ui.label(egui::RichText::new("Suggested").size(m.theme.small).color(m.theme.color("gold")));
            for (k, (s, (label, icon, off))) in sugg.iter().zip(&shown).enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                let on = on_sugg == Some(k);
                let face = if on {
                    "controls/list_row_selected"
                } else if resp.hovered() {
                    "controls/list_row_hover"
                } else if *off {
                    "controls/list_row_disabled"
                } else {
                    "controls/list_row_normal"
                };
                let painter = ui.painter();
                super::rows::face(painter, m, r, face, Color32::WHITE);
                if let Some(t) = icon {
                    painter.image(*t, egui::Rect::from_min_size(pos2(r.min.x + 16.0, r.min.y + 6.0), vec2(24.0, 24.0)), super::FULL_UV, Color32::WHITE);
                }
                let room = r.max.x - r.min.x - 58.0;
                let label = super::one_line(painter, label, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), super::rows::label_colour(m, on, *off), room);
                painter.galley(pos2(r.min.x + 50.0, r.min.y + 5.0), label, Color32::WHITE);
                let reason = super::one_line(painter, &s.reason, FontId::proportional(m.theme.small), m.theme.color("muted"), room);
                painter.galley(pos2(r.min.x + 50.0, r.min.y + 27.0), reason, Color32::WHITE);
                if s.group.len() > 1 {
                    super::rows::more_mark(painter, m, r, on);
                }
                if sub.is_some_and(|(sk, _)| sk == k) {
                    sub_row = Some(r);
                }
                if resp.clicked() {
                    s_click = Some(k);
                }
                ui.add_space(2.0);
            }
            ui.add_space(8.0);
        }
        for (k, &c) in menu.items.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
            let on = k == menu.sel && on_sugg.is_none();
            // Nothing in it can be used now (status, recasts, MP / TP): greyed, still opens as retail's.
            let off = !magic::command_ready(w, c);
            let face = if on {
                "controls/list_row_selected"
            } else if resp.hovered() {
                "controls/list_row_hover"
            } else if off {
                "controls/list_row_disabled"
            } else {
                "controls/list_row_normal"
            };
            let painter = ui.painter();
            super::rows::face(painter, m, r, face, Color32::WHITE);
            let (label, icon) = command(c);
            // Disengage reads Call for Help once swapped (DAT_10578174).
            let (label, icon) = if c == 3 && menu.call_help { command(2) } else { (label, icon) };
            if let Some(s) = m.art.sprite(icon) {
                painter.add(paint::image(&s, pos2(r.min.x + 20.0, r.center().y - 12.0), 24.0, if off { Color32::from_gray(110) } else { Color32::WHITE }));
            }
            painter.text(pos2(r.min.x + 52.0, r.center().y), Align2::LEFT_CENTER, label, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), super::rows::label_colour(m, on, off));
            let mut on_arrow = false;
            if has_right(c) {
                let a = super::rows::more_mark(painter, m, r, on);
                on_arrow = resp.hover_pos().is_some_and(|p| a.expand(4.0).contains(p));
            }
            if resp.clicked() {
                if on_arrow { arrow = Some(k) } else { clicked = Some(k) }
            }
            ui.add_space(2.0);
        }
    });
    // The open family's spells beside the panel, level with their row: icon, name, MP.
    let mut sub_click = None;
    if let (Some((k, c)), Some(row), Some(panel)) = (sub, sub_row, panel)
        && let Some(s) = sugg.get(k)
    {
        let rows: Vec<(String, String, Option<egui::TextureId>)> = s
            .group
            .iter()
            .map(|&id| {
                let mp = w.actions.as_ref().and_then(|a| a.spell(id)).map_or(String::new(), |r| format!("{} MP", r.mp));
                let icon = p.gu.and_then(|gu| magic::row_icon(w, Kind::Magic, id).and_then(|(lib, i)| m.sprite_icon(p.ctx, gu, lib, i)));
                (magic::spell_name(w, id), mp, icon)
            })
            .collect();
        let follow = m.moved("suggest family", k * 1000 + c);
        p.panel_at("suggest family", None, 260.0, pos2(panel.max.x + 6.0, row.min.y - 8.0), |ui| {
            let out = super::kit::list(ui, m, "suggest family", rows.len(), Some(c), true, follow, 34.0, 360.0, |_| false, |ui, r| {
                let (name, mp, icon) = &rows[r.index];
                let painter = ui.painter();
                if let Some(t) = icon {
                    painter.image(*t, egui::Rect::from_min_size(pos2(r.rect.min.x + 10.0, r.rect.center().y - 12.0), vec2(24.0, 24.0)), super::FULL_UV, Color32::WHITE);
                }
                painter.text(pos2(r.rect.min.x + 42.0, r.rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), super::kit::ink(m, r));
                painter.text(pos2(r.rect.max.x - 10.0, r.rect.center().y), Align2::RIGHT_CENTER, mp, FontId::proportional(m.theme.small), m.theme.color("muted"));
            });
            sub_click = out.clicked;
        });
    }
    if let (Some((k, _)), Some(i)) = (sub, sub_click)
        && let Some(&id) = sugg.get(k).and_then(|s| s.group.get(i))
    {
        let target = sugg[k].target.clone();
        listsearch::pick_root(w, Action::Spell(id), target.as_deref());
        return;
    }
    // A click on a family row opens (or closes) its submenu; on any other row it takes the suggestion.
    if let Some(k) = s_click
        && let Some(s) = sugg.get(k)
    {
        if s.group.len() > 1 {
            let open = w.suggest_sel == Some(k) && w.suggest_sub.is_some();
            (w.suggest_sel, w.suggest_sub) = (Some(k), (!open).then(|| suggest::family_start(s)));
            w.sounds.push(crate::audio::Ui::Select as u32);
        } else {
            listsearch::pick_root(w, s.action, s.target.as_deref());
        }
        return;
    }
    let Some(am) = w.action_menu.as_mut() else { return };
    if let Some(k) = arrow {
        am.sel = k;
        w.suggest_sel = None;
        combat::right(w);
    } else if let Some(k) = clicked {
        w.suggest_sel = None;
        w.action_click = Some(k);
    }
}
