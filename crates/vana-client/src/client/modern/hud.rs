//! The modern HUD panels: ourselves (job icon, name, jobs, HP / MP / TP, our effects with their times), the party (the
//! others: job icon, name, effects, HP; the party cursor, the target and hover marked by the list rows; a click
//! targets the member), the target (its relation's ring, name, HP), where we are (zone, grid, Vana'diel time and day,
//! weather) and the cast bar. What they show is gathered from the World first (the views), so the offscreen shot can
//! draw them too.

use super::super::{World, buffs, items::JOBS, monstrosity, self_entity, target, vana_now};
use super::{Look, Modern, Panels, Title, paint};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

fn job(j: u8) -> &'static str {
    if j == 23 { "MON" } else { JOBS.get(j as usize).copied().unwrap_or("") }
}

/// The pack's pictogram for a job (jobs/war ...).
pub fn job_icon(j: u8) -> Option<String> {
    (1..=22).contains(&j).then(|| format!("jobs/{}", job(j).to_ascii_lowercase()))
}

/// The pack's element icons in day order (Firesday .. Darksday).
const ELEMENTS: [&str; 8] = ["icons/fire", "icons/earth", "icons/water", "icons/wind", "icons/ice", "icons/lightning", "icons/light", "icons/dark"];

/// An effect icon: the game's status icon by id, or one of the pack's (the offscreen shot).
#[derive(Clone)]
pub enum Icon {
    Game(u16),
    Pack(&'static str),
}

#[derive(Clone)]
pub struct Effect {
    pub icon: Icon,
    /// Its time left as the retail icons label it, and whether it blinks (its last 15 s).
    pub label: Option<String>,
    pub ending: bool,
}

pub struct StatusView {
    pub name: String,
    pub jobs: String,
    pub icon: Option<String>,
    /// (now, max, the bar's eased fraction).
    pub hp: (u32, u32, f32),
    pub mp: Option<(u32, u32, f32)>,
    pub tp: u32,
    pub effects: Vec<Effect>,
}

pub struct PartyRow {
    pub slot: usize,
    pub name: String,
    pub icon: Option<String>,
    pub here: bool,
    pub hp: f32,
    /// The party cursor or our target on this member.
    pub marked: bool,
    pub effects: Vec<Effect>,
}

pub struct TargetView {
    pub name: String,
    pub ring: &'static str,
    /// HP percent and the bar's eased fraction; none for a target without a gauge.
    pub hp: Option<(u8, f32)>,
}

pub struct WhereView {
    pub zone: String,
    pub grid: String,
    /// Vana'diel hours and minutes, the weekday (0 Firesday ..) and its name.
    pub time: (u32, u32),
    pub day: (usize, String),
    /// The weather's element (day order) and whether it is the double weather.
    pub weather: Option<(usize, bool)>,
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

/// An effect's icon at `r`, dimmed while it blinks.
fn effect_icon(p: Look, painter: &egui::Painter, e: &Effect, r: Rect, small: bool) {
    let t = p.ctx.input(|i| i.time) as f32;
    let tint = if e.ending { Color32::from_gray((140.0 + 115.0 * (t * 4.0).sin().abs()) as u8) } else { Color32::WHITE };
    match e.icon {
        Icon::Game(id) => {
            if let Some(tex) = p.gu.and_then(|gu| p.m.status_icon(p.ctx, gu, small, id)) {
                let mut mesh = egui::Mesh::with_texture(tex);
                mesh.add_rect_with_uv(r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        Icon::Pack(name) => {
            if let Some(s) = p.m.art.sprite(name) {
                painter.add(paint::image(&s, r.min, r.width(), tint));
            }
        }
    }
}

/// Effects in rows of `per` icons `size` points square, their times under them.
fn effects_grid(ui: &mut Ui, p: Look, effects: &[Effect], size: f32, per: usize, small: bool) {
    let m = p.m;
    for chunk in effects.chunks(per) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), size + 12.0), Sense::hover());
        for (k, e) in chunk.iter().enumerate() {
            let at = Rect::from_min_size(pos2(r.min.x + k as f32 * (size + 4.0), r.min.y), vec2(size, size));
            effect_icon(p, ui.painter(), e, at, small);
            if let Some(l) = &e.label {
                ui.painter().text(pos2(at.center().x, at.max.y + 1.0), Align2::CENTER_TOP, l, FontId::proportional(m.theme.small - 3.0), m.theme.color("text"));
            }
        }
    }
}

pub fn status_panel(p: &mut Panels, v: &StatusView, small: bool) {
    let (m, look) = (p.m, p.look());
    p.panel("status", Some(Title { icon: v.icon.as_deref(), text: &v.name, note: Some(&v.jobs) }), 340.0, [0.012, 0.07], |ui| {
        gauge_row(ui, m, "HP", "hp", v.hp.2, &format!("{} / {}", v.hp.0, v.hp.1));
        if let Some((now, max, f)) = v.mp {
            gauge_row(ui, m, "MP", "mp", f, &format!("{now} / {max}"));
        }
        gauge_row(ui, m, "TP", "tp", v.tp as f32 / 3000.0, &v.tp.to_string());
        if !v.effects.is_empty() {
            ui.add_space(6.0);
            effects_grid(ui, look, &v.effects, 22.0, 11, small);
        }
    });
}

/// The party's other members; the slot clicked, if any.
pub fn party_panel(p: &mut Panels, rows: &[PartyRow], small: bool) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let (m, look) = (p.m, p.look());
    let note = format!("{} / 6", rows.len() + 1);
    let row_h = 36.0;
    let mut clicked = None;
    p.panel("party", Some(Title { icon: None, text: "Party", note: Some(&note) }), 340.0, [0.76, 0.5], |ui| {
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
            let name = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &row.name, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), colour);
            // Their effects (s2c 0x076) between the name and the bar, as many as fit.
            let bar_x = r.max.x - 118.0;
            let mut ex = name.max.x + 8.0;
            for e in &row.effects {
                if ex + 14.0 > bar_x - 6.0 {
                    break;
                }
                effect_icon(look, painter, e, Rect::from_min_size(pos2(ex, r.center().y - 7.0), vec2(14.0, 14.0)), small);
                ex += 15.0;
            }
            if row.here {
                bar(ui, m, "hp", Rect::from_min_max(pos2(bar_x, r.center().y - 6.0), pos2(r.max.x - 14.0, r.center().y + 6.0)), row.hp);
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
    p.panel("target", Some(Title { icon: Some(v.ring), text: &v.name, note: None }), 340.0, [0.52, 0.5], |ui| {
        if let Some((pct, f)) = v.hp {
            gauge_row(ui, m, "HP", "hp", f, &format!("{pct}%"));
        }
    });
}

pub fn where_panel(p: &mut Panels, v: &WhereView) {
    let m = p.m;
    p.panel("where", None, 340.0, [0.012, 0.29], |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let painter = ui.painter();
        painter.text(r.left_center(), Align2::LEFT_CENTER, &v.zone, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
        painter.text(r.right_center(), Align2::RIGHT_CENTER, &v.grid, FontId::proportional(m.theme.small), m.theme.color("muted"));
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let painter = ui.painter();
        let mut x = r.min.x;
        if let Some(s) = m.art.sprite(ELEMENTS[v.day.0 % 8]) {
            painter.add(paint::image(&s, pos2(x, r.center().y - 10.0), 20.0, Color32::WHITE));
            x += 26.0;
        }
        let t = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, format!("{}:{:02}", v.time.0, v.time.1), FontId::proportional(m.theme.ui), m.theme.color("text"));
        painter.text(pos2(t.max.x + 10.0, r.center().y), Align2::LEFT_CENTER, &v.day.1, FontId::proportional(m.theme.small), m.theme.color("muted"));
        if let Some((e, double)) = v.weather
            && let Some(s) = m.art.sprite(ELEMENTS[e % 8])
        {
            let n = if double { 2 } else { 1 };
            for k in 0..n {
                painter.add(paint::image(&s, pos2(r.max.x - 20.0 - 22.0 * k as f32, r.center().y - 10.0), 20.0, Color32::WHITE));
            }
        }
    });
}

pub fn cast_panel(p: &mut Panels, f: f32) {
    let m = p.m;
    p.panel("cast", None, 300.0, [0.41, 0.58], |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Casting", FontId::proportional(m.theme.small), m.theme.color("muted"));
        ui.painter().text(r.right_center(), Align2::RIGHT_CENTER, format!("{}%", (f * 100.0) as u32), FontId::proportional(m.theme.small), m.theme.color("text"));
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
        bar(ui, m, "cast", r, f);
    });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let small = w.game.get(0x99) & 4 != 0;
    if let Some(v) = status_view(p.look(), w) {
        status_panel(p, &v, small);
    }
    if let Some(v) = where_view(p.look(), w) {
        where_panel(p, &v);
    }
    if let Some(k) = party_panel(p, &party_rows(w), small) {
        let t = target::slot_target(w, k);
        target::pick(w, t);
    }
    if let Some(v) = target_view(w) {
        target_panel(p, &v);
    }
    if let Some((total, left)) = w.cast_bar {
        cast_panel(p, ((total - left) / total).clamp(0.0, 1.0));
    }
}

/// Our effects as the retail icons have them (buffs::icons): the time left when config 0xc3 shows timers and the
/// effect has one; the last 15 s blink.
fn effects(p: Look, w: &World) -> Vec<Effect> {
    let small = w.game.get(0x99) & 4 != 0;
    let timers = w.game.get(0xc3) != 0;
    let now = (vana_now(w) as u32).wrapping_mul(60);
    buffs::icons(w)
        .into_iter()
        .map(|(id, end)| {
            let no_timer = p.gu.and_then(|gu| gu.status_entry(small, id)).is_some_and(|e| e.no_timer);
            let timed = timers && end != 0x7fff_ffff && !no_timer;
            let rem = (end.wrapping_sub(now).wrapping_add(59) as i32) / 60;
            Effect { icon: Icon::Game(id), label: buffs::label(rem).filter(|_| timed), ending: timed && rem <= 15 }
        })
        .collect()
}

fn status_view(p: Look, w: &World) -> Option<StatusView> {
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
        effects: effects(p, w),
    })
}

/// FFXI weather ids 4..19 by pairs: fire, water, earth, wind, ice, lightning, light, dark (the second of a pair the
/// double weather), as day-order elements.
fn weather_element(weather: u16) -> Option<(usize, bool)> {
    let e = [0, 2, 1, 3, 4, 5, 6, 7].get((weather.checked_sub(4)? / 2) as usize).copied()?;
    Some((e, weather % 2 == 1))
}

fn where_view(p: Look, w: &World) -> Option<WhereView> {
    let gu = p.gu?;
    let (t, at) = w.clock?;
    let m = crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32()) as u32;
    let secs = t as u64 + at.elapsed().as_secs();
    let day = ((secs % 27648) / 3456) as usize;
    let name = w.battle.as_ref().and_then(|b| b.day_name(day)).unwrap_or_default();
    Some(WhereView {
        zone: w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, w.pos_zone as u32)).unwrap_or_else(|| crate::dat::font::decode_sjis(&gu.area_name(w.pos_zone))),
        grid: p.m.grid(w, gu),
        time: (m / 60, m % 60),
        day: (day, name),
        weather: weather_element(w.weather),
    })
}

fn party_rows(w: &World) -> Vec<PartyRow> {
    let slots = w.party.slots();
    let hand = target::party_hand(w).map(|h| h.0);
    (1..6)
        .map_while(|k| slots.slots[k].as_ref().map(|m| (k, m)))
        .map(|(k, m)| {
            let targeted = w.target.is_some_and(|t| t == m.id || w.entities.get(&t).is_some_and(|e| e.index == m.index));
            let effects = w.party_buffs.get(&m.id).map_or(Vec::new(), |row| row.iter().filter(|&&b| b != 0xffff && b != 0).map(|&b| Effect { icon: Icon::Game(b), label: None, ending: false }).collect());
            PartyRow {
                slot: k,
                name: monstrosity::row_name(m).to_string(),
                icon: job_icon(m.jobs[0]),
                here: m.away == 0 && m.zone == w.pos_zone,
                hp: w.party_gauges.get(k).map_or(m.hpp as f32, |g| g.1 as f32) / 100.0,
                marked: hand == Some(k) || targeted,
                effects,
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
