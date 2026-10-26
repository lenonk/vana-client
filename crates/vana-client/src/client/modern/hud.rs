//! The modern HUD panels: ourselves (job icon, name, jobs, HP / MP / TP), the party (the others: job icon, name, HP;
//! the party cursor, the target and hover marked by the list rows; a click targets the member), the target (its
//! relation's ring, name, HP). What they show is gathered from the World first (the views), so the offscreen shot
//! can draw them too.

use super::super::{World, items::JOBS, monstrosity, self_entity, target};
use super::{Modern, Panels, Title, paint};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

fn job(j: u8) -> &'static str {
    if j == 23 { "MON" } else { JOBS.get(j as usize).copied().unwrap_or("") }
}

/// The pack's pictogram for a job (jobs/war ...).
pub fn job_icon(j: u8) -> Option<String> {
    (1..=22).contains(&j).then(|| format!("jobs/{}", job(j).to_ascii_lowercase()))
}

pub struct StatusView {
    pub name: String,
    pub jobs: String,
    pub icon: Option<String>,
    /// (now, max, the bar's eased fraction).
    pub hp: (u32, u32, f32),
    pub mp: Option<(u32, u32, f32)>,
    pub tp: u32,
}

pub struct PartyRow {
    pub slot: usize,
    pub name: String,
    pub icon: Option<String>,
    pub here: bool,
    pub hp: f32,
    /// The party cursor or our target on this member.
    pub marked: bool,
}

pub struct TargetView {
    pub name: String,
    pub ring: &'static str,
    /// HP percent and the bar's eased fraction; none for a target without a gauge.
    pub hp: Option<(u8, f32)>,
}

/// The pack's gauge pair (`kind`_empty, `kind`_full) cut at `f` over `r`.
fn bar(ui: &Ui, m: &Modern, kind: &str, r: Rect, f: f32) {
    if let (Some(e), Some(full)) = (m.art.sprite(&format!("gauges/{kind}_empty")), m.art.sprite(&format!("gauges/{kind}_full"))) {
        paint::gauge(ui.painter(), &e, &full, r, f);
    }
}

/// A gauge row: the label, the bar, the value at the right.
fn gauge_row(ui: &mut Ui, m: &Modern, label: &str, kind: &str, f: f32, value: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let p = ui.painter();
    p.text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), m.theme.color("muted"));
    p.text(pos2(r.max.x, r.center().y), Align2::RIGHT_CENTER, value, FontId::proportional(m.theme.ui), m.theme.color("text"));
    bar(ui, m, kind, Rect::from_min_max(pos2(r.min.x + 30.0, r.center().y - 7.0), pos2(r.max.x - 92.0, r.center().y + 7.0)), f);
}

pub fn status_panel(p: &mut Panels, v: &StatusView) {
    let m = p.m;
    p.panel("status", Title { icon: v.icon.as_deref(), text: &v.name, note: Some(&v.jobs) }, 340.0, [0.012, 0.07], |ui| {
        gauge_row(ui, m, "HP", "hp", v.hp.2, &format!("{} / {}", v.hp.0, v.hp.1));
        if let Some((now, max, f)) = v.mp {
            gauge_row(ui, m, "MP", "mp", f, &format!("{now} / {max}"));
        }
        gauge_row(ui, m, "TP", "tp", v.tp as f32 / 3000.0, &v.tp.to_string());
    });
}

/// The party's other members; the slot clicked, if any.
pub fn party_panel(p: &mut Panels, rows: &[PartyRow]) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let m = p.m;
    let note = format!("{} / 6", rows.len() + 1);
    let row_h = 36.0;
    let mut clicked = None;
    p.panel("party", Title { icon: None, text: "Party", note: Some(&note) }, 340.0, [0.76, 0.5], |ui| {
        for row in rows {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let sprite = if row.marked {
                "controls/list_row_selected"
            } else if resp.hovered() {
                "controls/list_row_hover"
            } else if row.here {
                "controls/list_row_normal"
            } else {
                "controls/list_row_disabled"
            };
            let painter = ui.painter();
            if let Some(s) = m.art.sprite(sprite) {
                painter.add(paint::nine_slice(&s, r, row_h / s.size.y, Color32::WHITE));
            }
            let mut x = r.min.x + 22.0;
            if let Some(s) = row.icon.as_deref().and_then(|j| m.art.sprite(j)) {
                painter.add(paint::image(&s, pos2(x, r.center().y - 12.0), 24.0, Color32::WHITE));
                x += 30.0;
            }
            let colour = if row.here { m.theme.color("text") } else { m.theme.color("muted") };
            painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &row.name, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), colour);
            if row.here {
                bar(ui, m, "hp", Rect::from_min_max(pos2(r.max.x - 118.0, r.center().y - 6.0), pos2(r.max.x - 14.0, r.center().y + 6.0)), row.hp);
            }
            ui.add_space(4.0);
            if resp.clicked() {
                clicked = Some(row.slot);
            }
        }
    });
    clicked
}

pub fn target_panel(p: &mut Panels, v: &TargetView) {
    let m = p.m;
    p.panel("target", Title { icon: Some(v.ring), text: &v.name, note: None }, 340.0, [0.52, 0.5], |ui| {
        if let Some((pct, f)) = v.hp {
            gauge_row(ui, m, "HP", "hp", f, &format!("{pct}%"));
        }
    });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if let Some(v) = status_view(w) {
        status_panel(p, &v);
    }
    if let Some(k) = party_panel(p, &party_rows(w)) {
        let t = target::slot_target(w, k);
        target::pick(w, t);
    }
    if let Some(v) = target_view(w) {
        target_panel(p, &v);
    }
}

fn status_view(w: &World) -> Option<StatusView> {
    let st = w.stats.as_ref()?;
    let (hp, mp, tp, ..) = w.vitals.unwrap_or_default();
    let jobs = match st.sub.0 {
        0 => format!("{} {}", job(st.main.0), st.main.1),
        _ => format!("{} {} / {} {}", job(st.main.0), st.main.1, job(st.sub.0), st.sub.1),
    };
    // Our bars ease as the retail party window's row 0 does (FUN_101cad00); no MP row without MP.
    let (hpf, mpf) = w.party_gauges.first().map_or((1.0, 1.0), |g| (g.1 as f32 / 100.0, g.2 as f32 / 100.0));
    Some(StatusView {
        name: monstrosity::my_name(w).to_string(),
        jobs,
        icon: job_icon(st.main.0),
        hp: (hp, st.hp_max, hpf),
        mp: (st.mp_max > 0).then_some((mp, st.mp_max, mpf)),
        tp,
    })
}

fn party_rows(w: &World) -> Vec<PartyRow> {
    let slots = w.party.slots();
    let hand = target::party_hand(w).map(|h| h.0);
    (1..6)
        .map_while(|k| slots.slots[k].as_ref().map(|m| (k, m)))
        .map(|(k, m)| {
            let targeted = w.target.is_some_and(|t| t == m.id || w.entities.get(&t).is_some_and(|e| e.index == m.index));
            PartyRow {
                slot: k,
                name: monstrosity::row_name(m).to_string(),
                icon: job_icon(m.jobs[0]),
                here: m.away == 0 && m.zone == w.pos_zone,
                hp: w.party_gauges.get(k).map_or(m.hpp as f32, |g| g.1 as f32) / 100.0,
                marked: hand == Some(k) || targeted,
            }
        })
        .collect()
}

fn target_view(w: &World) -> Option<TargetView> {
    let id = w.target?;
    let me = (id == w.my_id).then(|| self_entity(w));
    let e = me.as_ref().or_else(|| w.entities.get(&id))?;
    let name = if id == w.my_id { monstrosity::my_name(w).to_string() } else { monstrosity::name(e).to_string() };
    // The pack's target rings by relation.
    let ring = if matches!(e.motion.status, 2 | 3) {
        "indicators/target_dead"
    } else if id == w.my_id || target::in_own_six(w, id) {
        "indicators/target_party"
    } else if e.claim != 0 && e.claim == w.my_id {
        "indicators/target_claimed"
    } else if target::hostile(w, id, e) {
        "indicators/target_hostile"
    } else {
        "indicators/target_neutral"
    };
    Some(TargetView { name, ring, hp: (!e.no_gauge).then_some((e.hpp, w.target_gauge.1 as f32 / 100.0)) })
}
