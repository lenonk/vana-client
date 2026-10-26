//! The modern HUD panels: ourselves (job icon, name, jobs, HP / MP / TP, our effects with their times), the party (the
//! others: job icon, name, effects, HP; the party cursor, the target and hover marked by the list rows; a click
//! targets the member), the target (its relation's ring, name, HP), where we are (zone, grid, Vana'diel time and day,
//! weather, the compass) and the cast bar. What they show is gathered from the World first (the views), so the offscreen shot can
//! draw them too.

use super::super::{App, World, buffs, hudtop, items::JOBS, monstrosity, radar, self_entity, target, vana_now};
use super::{Look, Modern, Panels, Title, chat, paint};
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::f32::consts::PI;

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
    /// Locked on (retail's windowps 212 over its target window).
    pub locked: bool,
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
    /// None where retail has no compass (ships and airships).
    pub compass: Option<CompassView>,
}

/// The compass as retail's (hudtop draw_compass): turned so the camera's heading is up, in retail's local units
/// (P.x - E.x, E.z - P.z).
pub struct CompassView {
    /// h = atan2(f.x, -f.z) of the camera's flattened forward.
    pub heading: f32,
    /// The radar dots (local x, local y, colour) and the radar's range.
    pub dots: Vec<(f32, f32, Color32)>,
    pub range: f32,
    /// The arrow toward the tracked target or the call-for-help monster: its local direction and theme colour.
    pub arrow: Option<(f32, f32, &'static str)>,
}

/// The pack's gauge pair (`kind`_empty, `kind`_full) cut at `f` over `r`.
fn bar(ui: &Ui, m: &Modern, kind: &str, r: Rect, f: f32) {
    if let (Some(e), Some(full)) = (m.art.sprite(&format!("gauges/{kind}_empty")), m.art.sprite(&format!("gauges/{kind}_full"))) {
        paint::gauge(ui.painter(), &e, &full, r, f);
    }
}

/// A bar `BAR` high under the row's text; the labels over bars at `LABEL_SIZE`.
const BAR: f32 = 14.0;
const LABEL_SIZE: f32 = 18.0;

/// A bar with its label and value over its top edge, the label a little in from the bar's start.
fn labelled_bar(ui: &Ui, m: &Modern, kind: &str, r: Rect, f: f32, label: (&str, Color32), value: &str) {
    bar(ui, m, kind, r, f);
    let p = ui.painter();
    let font = FontId::new(LABEL_SIZE, FontFamily::Name(super::LABEL.into()));
    // The text's bottom (its descender room) half way down the bar.
    let y = r.min.y + BAR * 0.5;
    paint::outlined(p, pos2(r.min.x + 6.0, y), Align2::LEFT_BOTTOM, label.0, font.clone(), label.1);
    if !value.is_empty() {
        paint::outlined(p, pos2(r.max.x - 6.0, y), Align2::RIGHT_BOTTOM, value, font, m.theme.color("text"));
    }
}

/// A gauge row: the bar across the row, its label and value over it.
fn gauge_row(ui: &mut Ui, m: &Modern, label: &str, kind: &str, f: f32, value: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
    labelled_bar(ui, m, kind, Rect::from_min_max(pos2(r.min.x, r.max.y - BAR), r.max), f, (label, m.theme.color("text")), value);
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
    p.panel("status", Some(Title { icon: v.icon.as_deref(), text: &v.name, note: Some(&v.jobs) }), 340.0, |ui| {
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
    let row_h = 40.0;
    let mut clicked = None;
    p.panel("party", Some(Title { icon: None, text: "Party", note: Some(&note) }), 340.0, |ui| {
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
            // The name over the HP bar (away members: the name alone), their effects (s2c 0x076) at the bar's right end
            // above it, as many as fit.
            let bar_r = Rect::from_min_max(pos2(x, r.max.y - 8.0 - BAR), pos2(r.max.x - 14.0, r.max.y - 8.0));
            let name = if row.here {
                labelled_bar(ui, m, "hp", bar_r, row.hp, (&row.name, m.theme.color("text")), "");
                let font = FontId::new(LABEL_SIZE, FontFamily::Name(super::LABEL.into()));
                painter.layout_no_wrap(row.name.clone(), font, Color32::WHITE).size().x + bar_r.min.x + 6.0
            } else {
                painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &row.name, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), m.theme.color("muted")).max.x
            };
            let mut ex = bar_r.max.x - 15.0;
            for e in row.effects.iter().filter(|_| row.here) {
                if ex < name + 8.0 {
                    break;
                }
                effect_icon(look, painter, e, Rect::from_min_size(pos2(ex, bar_r.min.y - 16.0), vec2(14.0, 14.0)), small);
                ex -= 15.0;
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
    let frame = if v.locked { "window_alert" } else { "window_normal" };
    let r = p.framed("target", Some(Title { icon: Some(v.ring), text: &v.name, note: None }), 340.0, frame, |ui| {
        if let Some((pct, f)) = v.hp {
            gauge_row(ui, m, "HP", "hp", f, &format!("{pct}%"));
        }
    });
    if let Some(r) = r.filter(|_| v.locked) {
        locked(&p.over("target"), m, r);
    }
}

/// Retail's lock-on mark (windowps 212) over the target panel: red arrowheads glowing out of its left and right borders,
/// "Locked" in red between two red pointers on its bottom border.
fn locked(p: &Painter, m: &Modern, r: Rect) {
    let red = m.theme.color("hp");
    let edge = Stroke::new(1.0, Color32::from_black_alpha(200));
    let tri = |tip: Pos2, base: f32, half: f32, fill: Color32, stroke: Stroke| {
        p.add(egui::Shape::convex_polygon(vec![tip, pos2(base, tip.y - half), pos2(base, tip.y + half)], fill, stroke));
    };
    let cy = r.center().y;
    for (x, out) in [(r.min.x, -1.0), (r.max.x, 1.0)] {
        tri(pos2(x + 14.0 * out, cy), x - 4.0 * out, 20.0, red.gamma_multiply(0.3), Stroke::NONE);
        tri(pos2(x + 9.0 * out, cy), x - 3.0 * out, 13.0, red, edge);
    }
    let font = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    let w = p.layout_no_wrap("Locked".into(), font.clone(), red).size().x;
    let c = pos2(r.center().x, r.max.y - 4.0);
    p.rect_filled(Rect::from_center_size(c, vec2(w + 34.0, 16.0)), 3.0, m.theme.color("charcoal"));
    paint::outlined(p, c, Align2::CENTER_CENTER, "Locked", font, red);
    for out in [-1.0, 1.0] {
        let x = c.x + out * (w / 2.0 + 13.0);
        tri(pos2(x + 6.0 * -out, c.y), x - 2.0 * -out, 5.0, red, edge);
    }
}

/// The compass's radius in points, and its tilt as retail's (cos 1.0996).
const DIAL: f32 = 40.0;
const TILT: f32 = 0.4536;

pub fn where_panel(p: &mut Panels, v: &WhereView) {
    let m = p.m;
    p.panel("where", None, 430.0, |ui| {
        // The zone over the time and day at the left, the compass, the grid over the weather at the right.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
        let right = r.max.x - 66.0;
        let dial = pos2(right - 16.0 - DIAL, r.center().y);
        let (top, low) = (r.center().y - 12.0, r.center().y + 13.0);
        let painter = ui.painter().with_clip_rect(Rect::from_min_max(r.min, pos2(dial.x - DIAL - 14.0, r.max.y)));
        painter.text(pos2(r.min.x, top), Align2::LEFT_CENTER, &v.zone, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
        let mut x = r.min.x;
        if let Some(s) = m.art.sprite(ELEMENTS[v.day.0 % 8]) {
            painter.add(paint::image(&s, pos2(x, low - 10.0), 20.0, Color32::WHITE));
            x += 26.0;
        }
        let t = painter.text(pos2(x, low), Align2::LEFT_CENTER, format!("{}:{:02}", v.time.0, v.time.1), FontId::proportional(m.theme.ui), m.theme.color("text"));
        painter.text(pos2(t.max.x + 10.0, low), Align2::LEFT_CENTER, &v.day.1, FontId::proportional(m.theme.small), m.theme.color("muted"));
        let painter = ui.painter();
        if let Some(c) = &v.compass {
            compass(painter, m, dial, c);
        }
        painter.text(pos2(r.max.x, top), Align2::RIGHT_CENTER, &v.grid, FontId::proportional(m.theme.small), m.theme.color("muted"));
        if let Some((e, double)) = v.weather
            && let Some(s) = m.art.sprite(ELEMENTS[e % 8])
        {
            let n = if double { 2 } else { 1 };
            for k in 0..n {
                painter.add(paint::image(&s, pos2(r.max.x - 20.0 - 22.0 * k as f32, low - 10.0), 20.0, Color32::WHITE));
            }
        }
    });
}

/// The compass dial at `c` as retail's disk: turned so the camera's heading is up and tilted back (Rx(-1.0996)), with
/// a rim of some depth, N (red) E S W on it, ticks, the camera's view as a wedge from us at the centre, the radar
/// dots, and the arrow through the rim.
fn compass(painter: &Painter, m: &Modern, c: Pos2, v: &CompassView) {
    let (sn, cs) = v.heading.sin_cos();
    // A local point at `k` points per unit: retail's Rz(h) then the tilt.
    let at = |lx: f32, ly: f32, k: f32| c + vec2(lx * cs - ly * sn, (lx * sn + ly * cs) * TILT) * k;
    let flat = |r: f32| vec2(r, r * TILT);
    let (gold, bronze) = (m.theme.color("gold"), m.theme.color("bronze"));
    let faint = Stroke::new(0.75, bronze.gamma_multiply(0.3));
    // The disk's edge below its face, then the face and its rings.
    painter.add(egui::Shape::ellipse_filled(c + vec2(0.0, 3.0), flat(DIAL), bronze.gamma_multiply(0.45)));
    painter.add(egui::Shape::ellipse_filled(c, flat(DIAL), m.theme.color("charcoal")));
    painter.add(egui::Shape::ellipse_stroke(c, flat(DIAL * 0.5), faint));
    painter.add(egui::Shape::ellipse_stroke(c, flat(DIAL - 6.0), Stroke::new(0.75, bronze.gamma_multiply(0.7))));
    painter.add(egui::Shape::ellipse_stroke(c, flat(DIAL), Stroke::new(1.5, gold)));
    painter.line_segment([at(0.0, 1.0, DIAL - 6.0), at(0.0, -1.0, DIAL - 6.0)], faint);
    painter.line_segment([at(1.0, 0.0, DIAL - 6.0), at(-1.0, 0.0, DIAL - 6.0)], faint);
    // The view: a wedge up from the centre (the camera's heading is always up), on the disk.
    let fan: Vec<Pos2> = std::iter::once(c)
        .chain((0..=8).map(|i| {
            let a = -PI / 2.0 + (i as f32 / 8.0 - 0.5) * 1.2;
            c + vec2(a.cos(), a.sin() * TILT) * (DIAL - 6.0)
        }))
        .collect();
    painter.add(egui::Shape::convex_polygon(fan, m.theme.color("focus").gamma_multiply(0.16), Stroke::NONE));
    // Ticks every 15 degrees between the rings, longer every 45, none under the letters.
    for i in (0..24).filter(|i| i % 6 != 0) {
        let a = i as f32 * PI / 12.0;
        let long = if i % 3 == 0 { 5.0 } else { 2.5 };
        painter.line_segment([at(a.sin(), a.cos(), DIAL - 1.0), at(a.sin(), a.cos(), DIAL - 1.0 - long)], Stroke::new(1.0, bronze));
    }
    let k = (DIAL - 6.0) / v.range.max(1.0);
    for &(lx, ly, col) in &v.dots {
        painter.circle_filled(at(lx, ly, k), 1.8, col);
    }
    painter.circle_filled(c, 2.0, gold);
    // Retail's letters on the rim, upright: s4 N at local +y, s1 E at -x, s3 S at -y, s2 W at +x.
    let font = FontId::new(11.0, FontFamily::Name(super::STRONG.into()));
    for ((lx, ly), letter) in [((0.0, 1.0), "N"), ((-1.0, 0.0), "E"), ((0.0, -1.0), "S"), ((1.0, 0.0), "W")] {
        let p = at(lx, ly, DIAL);
        painter.circle_filled(p, 6.5, m.theme.color("charcoal"));
        let col = if letter == "N" { m.theme.color("hp") } else { gold };
        painter.text(p, Align2::CENTER_CENTER, letter, font.clone(), col);
    }
    if let Some((lx, ly, col)) = v.arrow {
        let (tip, base) = (at(lx, ly, DIAL + 5.0), at(lx, ly, DIAL - 5.0));
        let n = (tip - base).normalized().rot90() * 4.5;
        painter.add(egui::Shape::convex_polygon(vec![tip, base + n, base - n], m.theme.color(col), Stroke::new(1.0, Color32::from_black_alpha(180))));
    }
}

fn compass_view(w: &World) -> Option<CompassView> {
    if w.zone.is_some_and(hudtop::no_compass) {
        return None;
    }
    let cam = App::camera(w);
    let f = cam.target - cam.eye;
    let f = glam::Vec3::new(f.x, 0.0, f.z).normalize_or(glam::Vec3::Z);
    // The job fields as the open compass samples them (NIN sees further and monsters).
    let rs = w.stats.as_ref().map_or_else(|| radar::sample(0, 0, 0), |st| radar::sample(st.main.0, st.sub.0, st.sub.1));
    let dots = radar::dots(w, 0x80, &rs).into_iter().map(|(x, y, c)| (x as f32, y as f32, chat::colour(c.to_le_bytes()))).collect();
    let aim = match w.scan.tracking() {
        Some(t) => Some((t.pos[0], t.pos[2], "focus")),
        None => target::call_for_help(w).and_then(|id| w.entities.get(&id)).map(|e| (e.pos.x, e.pos.z, "hp")),
    };
    let arrow = aim.and_then(|(x, z, col)| {
        let d = vec2(w.pos.x - x, z - w.pos.z);
        (d.length() > 1e-3).then(|| (d.normalized().x, d.normalized().y, col))
    });
    Some(CompassView { heading: f.x.atan2(-f.z), dots, range: rs.range, arrow })
}

pub fn cast_panel(p: &mut Panels, f: f32) {
    let m = p.m;
    p.panel("cast", None, 300.0, |ui| {
        gauge_row(ui, m, "Casting", "cast", f, &format!("{}%", (f * 100.0) as u32));
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
        compass: compass_view(w),
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
    Some(TargetView { name, ring, locked: w.locked, hp: (!e.no_gauge).then_some((e.hpp, w.target_gauge.1 as f32 / 100.0)) })
}
