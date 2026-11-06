//! The pending action's chip (client/pending.rs): what we asked for and on whom while the server has not answered
//! ("waiting" after a moment, so a quick answer never flashes it), the server's refusal in its own words, or that no
//! answer came. Not drawn once the server confirms (the cast bar and the effect take over).

use super::super::World;
use super::super::pending::{self, State};
use super::Panels;
use egui::{Align2, FontId, Sense, pos2, vec2};

/// The chip as drawn: the action, the target's name, the state.
pub struct View {
    pub what: String,
    pub on: Option<String>,
    pub state: State,
    /// Seconds waited (the dots' beat).
    pub secs: f32,
}

pub fn view(w: &World) -> Option<View> {
    let p = w.asked.as_ref()?;
    if p.state == State::Waiting && p.at.elapsed() < pending::SHOW_AFTER {
        return None;
    }
    let on = match p.target {
        0 => None,
        t if t == w.my_id => None,
        t => w.entities.get(&t).map(|e| e.name.clone()).filter(|n| !n.is_empty()),
    };
    Some(View { what: pending::name(w, p.ask), on, state: p.state.clone(), secs: p.at.elapsed().as_secs_f32() })
}

pub fn draw(p: &mut Panels, w: &World) {
    if let Some(v) = view(w) {
        panel(p, &v);
    }
}

pub fn panel(p: &mut Panels, v: &View) {
    let m = p.m;
    p.panel("pending", None, 300.0, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), m.theme.ui + 6.0), Sense::hover());
        let head = match &v.on {
            Some(n) => format!("{} \u{2192} {n}", v.what),
            None => v.what.clone(),
        };
        let font = FontId::proportional(m.theme.ui);
        let (note, colour) = match &v.state {
            State::Waiting => (format!("waiting{}", ".".repeat(1 + (v.secs * 3.0) as usize % 3)), m.theme.color("muted")),
            State::Refused(_) => ("refused".to_string(), m.theme.color("hp")),
            State::NoReply => ("no reply".to_string(), m.theme.color("muted")),
        };
        // A dot in the state's colour, the action, the state at the right.
        ui.painter().circle_filled(pos2(r.min.x + 5.0, r.center().y), 4.0, if v.state == State::Waiting { m.theme.color("focus") } else { colour });
        ui.painter().text(pos2(r.min.x + 16.0, r.center().y), Align2::LEFT_CENTER, head, font.clone(), m.theme.color("text"));
        ui.painter().text(pos2(r.max.x, r.center().y), Align2::RIGHT_CENTER, note, FontId::proportional(m.theme.small), colour);
        let why = match &v.state {
            State::Refused(t) if !t.is_empty() => Some(t.clone()),
            State::NoReply => Some("The server has not answered.".to_string()),
            _ => None,
        };
        if let Some(t) = why {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(t).size(m.theme.small).color(colour));
        }
    });
}
