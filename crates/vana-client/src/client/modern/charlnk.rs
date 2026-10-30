//! The modern charlnk over client/charlnk.rs, which keeps its rules and keys: the line's name as the title, the nine
//! rows in the layout's order (the disabled ones dimmed), beside the full log at the clicked line. Hovering moves the
//! cursor, a click picks, a right click closes.

use super::super::World;
use super::super::charlnk::{self, CHARLNK};
use super::{Panels, Title};
use egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};

const W: f32 = 240.0;
const ROW: f32 = 28.0;

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    let Some(c) = w.charlnk.clone() else { return };
    let m = p.m;
    let off = charlnk::off(w, &c);
    let rows: Vec<(u16, String)> = charlnk::ORDER.iter().map(|&k| (k, gu.child_caption(CHARLNK, k).or_else(|| gu.focus_text(CHARLNK, k)[1].clone()).unwrap_or_default())).collect();
    let name = charlnk::shown_name(w, &c).to_string();
    let free = w.confirm.is_none();
    // Kept on the screen: last frame's height (a guess the first one).
    let screen = p.ctx.content_rect();
    let h = m.rects.borrow().get("charlnk").map_or(10.0 * (ROW + 2.0) + 60.0, |r| r.height());
    let at = m.charlnk_at.get().unwrap_or(pos2(screen.width() * 0.4, 120.0));
    let at = pos2(at.x.min(screen.max.x - W - 8.0), at.y.min(screen.max.y - h - 8.0).max(screen.min.y + 8.0));
    let (mut click, mut back) = (None, false);
    p.panel_at("charlnk", Some(Title { icon: Some("icons/party"), text: &name, note: None }), W, at, |ui| {
        for (k, text) in &rows {
            let (r, resp) = ui.allocate_exact_size(vec2(W - 44.0, ROW), Sense::click());
            let on = !off.contains(k);
            let face = match () {
                _ if *k == c.cursor && free => "controls/list_row_selected",
                _ if *k == c.cursor || resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let colour = if on { m.theme.color("text") } else { m.theme.color("muted").gamma_multiply(0.6) };
            let clip = Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 18.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), colour);
            if free && resp.clicked() {
                click = Some(*k);
            }
            back |= free && resp.secondary_clicked();
            ui.add_space(2.0);
        }
    });
    if let Some(k) = click {
        if let Some(s) = w.charlnk.as_mut() {
            s.cursor = k;
        }
        charlnk::key(w, egui::Key::Enter);
    } else if back {
        charlnk::key(w, egui::Key::Escape);
    }
}
