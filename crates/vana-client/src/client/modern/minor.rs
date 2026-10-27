//! Small retail windows in the modern style; their modules keep the rules and keys: the block list (blklist.rs, its
//! names and blkmain's Delete) and the chat mode window (chatctrl.rs, its seven rows with the tell name, the shells
//! and the Unity, the cursor row's range under them).

use super::super::{World, blklist, chatctrl};
use super::craft::{button, caption};
use super::{MenuWin, Panels, Title, paint, social};
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
}

/// The names, the cursor kept in view; Delete under them while blkmain is up.
fn block_list(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((rows, cursor, main, empty)) = blklist::view(w) else { return };
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let title = gu.focus_text(blklist::LIST, 1)[1].clone().unwrap_or_else(|| "Blacklist".into());
    let (mut hover, mut click, mut delete) = (None, None, false);
    p.panel("blklist", Some(Title { icon: Some("icons/party"), text: &title, note: None }), W + 44.0, |ui| {
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
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                }
                ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), m.theme.color("text"));
                if k == cursor && follow {
                    ui.scroll_to_rect(r, None);
                }
                if focused && resp.hovered() && moved {
                    hover = Some(k);
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
    if delete {
        blklist::key(w, egui::Key::Enter);
        return;
    }
    for k in hover.into_iter().chain(click) {
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
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let title = gu.focus_text(chatctrl::WINDOW, v.sel)[1].clone().unwrap_or_else(|| "Chat".into());
    let mut got = (None, None);
    p.panel("chatctrl", Some(Title { icon: None, text: &title, note: None }), social::width(1), |ui| {
        let col = MenuWin { name: chatctrl::WINDOW, sel: v.sel, off: v.off.clone(), labels: Vec::new(), marked: Vec::new(), values: v.values.clone() };
        got = social::column(ui, m, gu, &col, free, moved);
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
