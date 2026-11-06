//! The software keyboard in the modern layer (ours): retail's keys, pages and pad buttons (client/swkbd.rs) as a
//! panel of kit keys over the chat window; the mouse hovers and clicks them.

use super::super::World;
use super::super::swkbd;
use super::{Panels, Title, kit};
use egui::{FontId, Sense, pos2, vec2};

const KEY: f32 = 48.0;
const GAP: f32 = 6.0;

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(k) = w.swkbd.clone() else { return };
    let m = p.m;
    let screen = p.ctx.content_rect();
    let wide = 2.0 * KEY + 1.5 * GAP + 20.0;
    let width = 15.0 * KEY + 14.0 * GAP + 2.0 * GAP + wide + 24.0;
    // Over the chat window, centred, placed once as it opens (it stays put while it is up, as retail's).
    let height = 5.0 * KEY + 4.0 * GAP + 56.0;
    let (x, y) = w.swkbd.as_ref().map_or((0.0, 0.0), |live| {
        swkbd::place_once(live, || {
            let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
            let cx = m.chat_edge.get().map_or(screen.center().x, |(r, _)| r.center().x);
            ((cx - width / 2.0).max(screen.min.x), (edge - height - 6.0).max(screen.min.y))
        })
    });
    let at = pos2(x, y);
    let (mut hovered, mut clicked) = (None, None);
    let font = FontId::proportional(m.theme.ui * 1.25);
    let small = FontId::proportional(m.theme.small);
    p.panel_at("keyboard", Some(Title { icon: None, text: "Keyboard", note: None, name: false }), width, at, |ui| {
        let (area, _) = ui.allocate_exact_size(vec2(width - 24.0, 5.0 * KEY + 4.0 * GAP), Sense::hover());
        let o = area.min;
        let mut key = |ui: &mut egui::Ui, id: u16, r: egui::Rect, label: &str, font: &FontId, lit: bool, off: bool| {
            let resp = ui.interact(r, egui::Id::new(("swkbd key", id)), Sense::click());
            kit::paint_button(ui.painter(), m, r, label, font, kit::Btn { lit: lit || k.cursor == id, cursor: k.cursor == id, off }, resp.hovered());
            if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                hovered = Some(id);
            }
            if resp.clicked() {
                clicked = Some(id);
            }
        };
        for i in 0..75usize {
            let (row, col) = ((i / 15) as f32, (i % 15) as f32);
            let r = egui::Rect::from_min_size(pos2(o.x + col * (KEY + GAP), o.y + row * (KEY + GAP)), vec2(KEY, KEY));
            let (label, inert) = swkbd::label(&k, i);
            key(ui, 8 + i as u16, r, &label, &font, false, inert);
        }
        // The keys right of the grid, as retail's: Backspace and Config, then Tab, SHIFT, SPACE, Enter.
        let x = o.x + 15.0 * (KEY + GAP) + GAP;
        let half = (wide - GAP) / 2.0;
        let rows: [&[(u16, &str, f32)]; 5] = [&[(6, "Back", half), (0x53, "Config", half)], &[(7, "Tab", wide)], &[(0x55, "Shift", wide)], &[(0x56, "Space", wide)], &[(0x57, "Enter", wide)]];
        for (row, keys) in rows.iter().enumerate() {
            let mut kx = x;
            for &(id, label, kw) in keys.iter() {
                let r = egui::Rect::from_min_size(pos2(kx, o.y + row as f32 * (KEY + GAP)), vec2(kw, KEY));
                key(ui, id, r, label, &small, id == 0x55 && swkbd::shifted(&k), false);
                kx += kw + GAP;
            }
        }
    });
    if let Some(id) = hovered {
        swkbd::hover(w, id);
    }
    if let Some(id) = clicked {
        swkbd::tap(w, id);
    }
}
