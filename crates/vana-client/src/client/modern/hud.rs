//! The modern HUD panels (mockups exploration / combat): ourselves (name, jobs, HP / MP / TP), the party (the others'
//! HP; the party cursor and the target marked; a click targets the member), the target (name, HP).

use super::super::{World, items::JOBS, monstrosity, self_entity, target};
use super::{Panels, paint};
use egui::{Color32, FontFamily, RichText, Sense, Ui, vec2};

fn job(j: u8) -> &'static str {
    if j == 23 { "MON" } else { JOBS.get(j as usize).copied().unwrap_or("") }
}

/// A row: `label` in the label column, then a gauge to the right edge.
fn gauge_row(ui: &mut Ui, label: &str, f: f32, fill: Color32, label_w: f32) {
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], egui::Label::new(RichText::new(label)).halign(egui::Align::LEFT));
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
        paint::gauge(ui.painter(), r, f, fill);
    });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    status(p, w);
    party(p, w);
    target_panel(p, w);
}

fn status(p: &mut Panels, w: &World) {
    let Some(st) = w.stats.as_ref() else { return };
    let (hp, mp, tp, ..) = w.vitals.unwrap_or_default();
    let jobs = match st.sub.0 {
        0 => format!("{} {}", job(st.main.0), st.main.1),
        _ => format!("{} {} / {} {}", job(st.main.0), st.main.1, job(st.sub.0), st.sub.1),
    };
    let title = format!("{}  •  {jobs}", monstrosity::my_name(w));
    // Our bars ease as the retail party window's row 0 does (FUN_101cad00).
    let (hpf, mpf) = w.party_gauges.first().map_or((1.0, 1.0), |g| (g.1 as f32 / 100.0, g.2 as f32 / 100.0));
    let t = &p.m.theme;
    let (red, blue, gold) = (t.color("danger"), t.color("blue"), t.color("gold"));
    p.panel("status", &title, 300.0, [0.012, 0.07], |ui| {
        gauge_row(ui, &format!("HP  {hp} / {}", st.hp_max), hpf, red, 140.0);
        // No MP row for a job without MP.
        if st.mp_max > 0 {
            gauge_row(ui, &format!("MP  {mp} / {}", st.mp_max), mpf, blue, 140.0);
        }
        gauge_row(ui, &format!("TP  {tp}"), tp as f32 / 3000.0, gold, 140.0);
    });
}

fn party(p: &mut Panels, w: &mut World) {
    let slots = w.party.slots();
    let rows: Vec<(usize, super::super::party::Member)> = (1..6).map_while(|k| slots.slots[k].clone().map(|m| (k, m))).collect();
    if rows.is_empty() {
        return;
    }
    let hand = target::party_hand(w).map(|h| h.0);
    let m = p.m;
    let (good, muted) = (m.theme.color("good"), m.theme.color("muted"));
    let row_h = 30.0;
    let mut clicked = None;
    p.panel("party", "Party", 320.0, [0.76, 0.52], |ui| {
        for (k, mem) in &rows {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let here = mem.away == 0 && mem.zone == w.pos_zone;
            let targeted = w.target.is_some_and(|t| t == mem.id || w.entities.get(&t).is_some_and(|e| e.index == mem.index));
            let sprite = if hand == Some(*k) {
                Some("row_focus")
            } else if targeted {
                Some("row_active")
            } else if resp.hovered() {
                Some("row_normal")
            } else {
                None
            };
            if let Some(s) = sprite.and_then(|s| m.art.sprite(s)) {
                ui.painter().add(paint::nine_slice(&s, r, 1.0, Color32::WHITE));
            }
            let name = monstrosity::row_name(mem);
            let colour = if here { ui.visuals().override_text_color.unwrap_or(Color32::WHITE) } else { muted };
            ui.painter().text(r.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::new(m.theme.ui, FontFamily::Proportional), colour);
            let bar = egui::Rect::from_min_size(r.min + vec2(r.width() * 0.44, (row_h - 10.0) / 2.0), vec2(r.width() * 0.56 - 8.0, 10.0));
            let f = w.party_gauges.get(*k).map_or(mem.hpp as f32, |g| g.1 as f32) / 100.0;
            paint::gauge(ui.painter(), bar, if here { f } else { 0.0 }, good);
            if resp.clicked() {
                clicked = Some(*k);
            }
        }
    });
    if let Some(k) = clicked {
        let t = target::slot_target(w, k);
        target::pick(w, t);
    }
}

fn target_panel(p: &mut Panels, w: &World) {
    let Some(id) = w.target else { return };
    let me = (id == w.my_id).then(|| self_entity(w));
    let Some(e) = me.as_ref().or_else(|| w.entities.get(&id)) else { return };
    let name = if id == w.my_id { monstrosity::my_name(w).to_string() } else { monstrosity::name(e).to_string() };
    let red = p.m.theme.color("danger");
    let (gauge, hpp) = (!e.no_gauge, e.hpp);
    let eased = w.target_gauge.1 as f32 / 100.0;
    p.panel("target", &format!("Target: {name}"), 340.0, [0.52, 0.52], |ui| {
        if gauge {
            gauge_row(ui, &format!("HP  {hpp}%"), eased, red, 90.0);
        }
    });
}
