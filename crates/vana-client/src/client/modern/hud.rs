//! The modern HUD panels: ourselves (job icon, name, jobs, HP / MP / TP, our effects with their times), the party (the
//! others: job icon, name, effects, HP; the party cursor, the target and hover marked by the list rows; a click
//! targets the member), the target (its relation's ring, name, HP), where we are (zone, grid, Vana'diel time and day,
//! weather, the compass) and the cast bar. What they show is gathered from the World first (the views), so the offscreen shot can
//! draw them too.

use super::super::{App, World, buffs, hudtop, items::JOBS, monstrosity, radar, self_entity, target, vana_now};
use super::{Look, Modern, Panels, Title, chat, paint};
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::f32::consts::PI;

pub(super) fn job(j: u8) -> &'static str {
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

/// Our effects (the effects panel).
pub struct StatusView {
    pub effects: Vec<Effect>,
}

/// A party or alliance row as retail's (FUN_10202530 / FUN_10202720): ours first, from our own vitals.
#[derive(Clone)]
pub struct PartyRow {
    pub slot: usize,
    pub name: String,
    /// The main and sub jobs' pictograms.
    pub icon: Option<String>,
    pub sub_icon: Option<String>,
    /// In our zone: the numbers and bars; else the zone the member is in, shown instead.
    pub here: Option<RowVitals>,
    pub zone: String,
    /// The party cursor or our target on this member, and the hand's tint (retail's party_hand: plain on our target,
    /// by range on the party cursor).
    pub marked: bool,
    pub hand: Option<u32>,
    /// 1 the party's leader, 2 the alliance's.
    pub marks: u8,
    pub effects: Vec<Effect>,
}

/// A row's numbers and eased bars: HP in its colour by HP percent (menu hpcol) over its max (ours known, a member's
/// from when it was last at 100 %, else HP / HP %), MP (no MP bar on alliance rows), TP only with config 0xae bit 0
/// and never on trusts.
#[derive(Clone)]
pub struct RowVitals {
    pub hp: u32,
    pub hp_max: Option<u32>,
    pub mp: u32,
    pub tp: Option<u32>,
    pub hpf: f32,
    pub mpf: Option<f32>,
    pub hp_colour: Color32,
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
pub(super) const BAR: f32 = 14.0;
const LABEL_SIZE: f32 = 18.0;

/// A bar with its label and value over its top edge, the label a little in from the bar's start.
pub(super) fn labelled_bar(ui: &Ui, m: &Modern, kind: &str, r: Rect, f: f32, label: (&str, Color32), value: (&str, Color32)) {
    bar(ui, m, kind, r, f);
    let p = ui.painter();
    let font = FontId::new(LABEL_SIZE, FontFamily::Name(super::LABEL.into()));
    // The text's bottom (its descender room) half way down the bar.
    let y = r.min.y + BAR * 0.5;
    paint::outlined(p, pos2(r.min.x + 6.0, y), Align2::LEFT_BOTTOM, label.0, font.clone(), label.1);
    if !value.0.is_empty() {
        paint::outlined(p, pos2(r.max.x - 6.0, y), Align2::RIGHT_BOTTOM, value.0, font, value.1);
    }
}

/// A gauge row: the bar across the row, its label and value over it.
fn gauge_row(ui: &mut Ui, m: &Modern, label: &str, kind: &str, f: f32, value: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
    labelled_bar(ui, m, kind, Rect::from_min_max(pos2(r.min.x, r.max.y - BAR), r.max), f, (label, m.theme.color("text")), (value, m.theme.color("text")));
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

/// The effects panel's icons: their size, the step between them, how many a row.
const FX: f32 = 28.0;
const FX_STEP: f32 = 36.0;
const FX_ROW: usize = 10;

/// Effects in rows of `per` icons `size` points square, `step` apart, their times under them.
fn effects_grid(ui: &mut Ui, p: Look, effects: &[Effect], size: f32, step: f32, per: usize, small: bool) {
    let m = p.m;
    for chunk in effects.chunks(per) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), size + 16.0), Sense::hover());
        for (k, e) in chunk.iter().enumerate() {
            let at = Rect::from_min_size(pos2(r.min.x + k as f32 * step, r.min.y), vec2(size, size));
            effect_icon(p, ui.painter(), e, at, small);
            if let Some(l) = &e.label {
                paint::outlined(ui.painter(), pos2(at.center().x, at.max.y + 1.0), Align2::CENTER_TOP, l, FontId::proportional(m.theme.small), m.theme.color("text"));
            }
        }
    }
}

/// Our effects alone (the name, jobs and bars are the party window's): their icons and times, shown while any.
pub fn status_panel(p: &mut Panels, v: &StatusView, small: bool) {
    if v.effects.is_empty() {
        return;
    }
    // Retail's icons are layer 0, under every window: a retail window over where they stand (the hand-over, the item
    // pickers at the top left) hides them until it closes.
    if let Some(r) = p.m.rects.borrow().get("status").copied()
        && super::extras::under_retail(p.m, r)
    {
        p.this.insert("status".into(), r);
        return;
    }
    let look = p.look();
    let per = v.effects.len().min(FX_ROW);
    let width = super::MARGIN.sum().x + (per - 1) as f32 * FX_STEP + FX;
    // No frame: the icons alone (movable all the same).
    p.framed("status", None, width, "none", |ui| effects_grid(ui, look, &v.effects, FX, FX_STEP, FX_ROW, small));
}

/// Row heights: party rows carry an MP bar under the HP bar, alliance rows not.
const PARTY_ROW: f32 = 60.0;
const ALLY_ROW: f32 = 50.0;

/// The party window (ours first; "Solo" outside a party) or an alliance's, as retail's ptwN / raid1 / raid2: per row
/// the job, leader marks, the name over the HP bar with the HP number in its colour, the MP bar and number and TP
/// (party rows), a member elsewhere greyed with the zone's name, the hand on the party cursor's or our target's row.
/// Its height is exact, so the party window grows upward with no frame's lag. The slot clicked, if any.
pub fn party_panel(p: &mut Panels, id: &str, title: &str, rows: &[PartyRow], small: bool) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let (m, look) = (p.m, p.look());
    let full = id == "party";
    let row_h = if full { PARTY_ROW } else { ALLY_ROW };
    let note = (title != "Solo").then(|| format!("{} / 6", rows.len()));
    // Margins, the title row (28, egui's 3 under it, our 6), the rows 4 apart.
    let n = rows.len() as f32;
    let height = super::MARGIN.sum().y + 37.0 + n * row_h + (n - 1.0) * 4.0;
    let mut clicked = None;
    let show = super::Show { title: Some(Title { icon: None, text: title, note: note.as_deref() }), size: vec2(340.0, 0.0), height: Some(height), ..Default::default() };
    p.show(id, show, |ui, _| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(4.0);
            }
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let sprite = if row.marked {
                "controls/list_row_selected"
            } else if resp.hovered() {
                "controls/list_row_hover"
            } else if row.here.is_some() {
                "controls/list_row_normal"
            } else {
                "controls/list_row_disabled"
            };
            let painter = ui.painter();
            if let Some(s) = m.art.sprite(sprite) {
                painter.add(paint::nine_slice(&s, r, (row_h / s.size.y).min(1.0), Color32::WHITE));
            }
            // The hand at the row's left, reaching over the frame (retail's anc2s over the log's edge): the pack's arrow
            // turned to point at the row.
            if let Some(tint) = row.hand {
                let (name, colour) = super::pointer_look(tint);
                if let Some(s) = m.art.sprite(name) {
                    let a = Rect::from_min_size(pos2(r.min.x - 40.0, r.center().y - 14.0), vec2(28.0, 28.0));
                    ui.ctx().layer_painter(painter.layer_id()).add(paint::turned(&s, a, 1, colour));
                }
            }
            // Past the row frame's left ornament.
            let mut x = r.min.x + 22.0;
            let dim = if row.here.is_some() { Color32::WHITE } else { Color32::from_gray(120) };
            if let Some(s) = row.icon.as_deref().and_then(|j| m.art.sprite(j)) {
                painter.add(paint::image(&s, pos2(x, r.center().y - 12.0), 24.0, dim));
            }
            // The sub job small on the main's upper right corner (the leader stars take the lower left).
            if let Some(s) = row.sub_icon.as_deref().and_then(|j| m.art.sprite(j)) {
                painter.add(paint::image(&s, pos2(x + 15.0, r.center().y - 17.0), 15.0, dim));
            }
            // Leader marks on the job's corner: gold the party's, blue the alliance's.
            for (k, (bit, col)) in [(1u8, m.theme.color("gold")), (2u8, m.theme.color("focus"))].into_iter().enumerate() {
                if row.marks & bit != 0 {
                    painter.add(paint::star(pos2(x + 1.0 + 11.0 * k as f32, r.max.y - 9.0), 6.5, col));
                }
            }
            x += 32.0;
            let right = r.max.x - 10.0;
            let Some(v) = &row.here else {
                let font = FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into()));
                painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, &row.name, font, m.theme.color("muted"));
                painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, format!("({})", row.zone), FontId::proportional(m.theme.small), m.theme.color("muted"));
                if resp.clicked() {
                    clicked = Some(row.slot);
                }
                continue;
            };
            // Room inside the row's frame lines: the labels start well under its top, the MP line ends well over its
            // bottom.
            let hp_r = Rect::from_min_max(pos2(x, r.min.y + 26.0), pos2(right, r.min.y + 38.0));
            let hp = v.hp_max.map_or(v.hp.to_string(), |max| format!("{} / {max}", v.hp));
            labelled_bar(ui, m, "hp", hp_r, v.hpf, (&row.name, m.theme.color("text")), (&hp, v.hp_colour));
            let small_font = FontId::new(14.0, FontFamily::Name(super::LABEL.into()));
            if let Some(mpf) = v.mpf {
                // The MP bar under it, its number at its right; TP (config 0xae bit 0) before it, blue from 1000.
                let my = hp_r.max.y + 9.0;
                let mut mx = x;
                if let Some(tp) = v.tp {
                    let col = if tp > 999 { m.theme.color("focus") } else { m.theme.color("muted") };
                    mx = paint::outlined(painter, pos2(x + 2.0, my), Align2::LEFT_CENTER, &tp.to_string(), small_font.clone(), col).max.x + 8.0;
                }
                let mp_w = painter.layout_no_wrap("88888".into(), small_font.clone(), Color32::WHITE).size().x;
                bar(ui, m, "mp", Rect::from_min_max(pos2(mx, my - 3.5), pos2(right - mp_w - 6.0, my + 3.5)), mpf);
                paint::outlined(painter, pos2(right, my), Align2::RIGHT_CENTER, &v.mp.to_string(), small_font, m.theme.color("text"));
            } else if let Some(tp) = v.tp {
                let col = if tp > 999 { m.theme.color("focus") } else { m.theme.color("muted") };
                paint::outlined(painter, pos2(x + 2.0, hp_r.max.y + 6.0), Align2::LEFT_CENTER, &tp.to_string(), small_font, col);
            }
            // Their effects (s2c 0x076) above the bar at its right, as many as fit beside the name.
            if full {
                let font = FontId::new(LABEL_SIZE, FontFamily::Name(super::LABEL.into()));
                let name_end = x + 6.0 + painter.layout_no_wrap(row.name.clone(), font.clone(), Color32::WHITE).size().x;
                let hp_w = painter.layout_no_wrap(v.hp_max.map_or(v.hp.to_string(), |max| format!("{} / {max}", v.hp)), font, Color32::WHITE).size().x;
                let mut ex = right - 6.0 - hp_w - 12.0 - 14.0;
                for e in &row.effects {
                    if ex < name_end + 8.0 {
                        break;
                    }
                    effect_icon(look, painter, e, Rect::from_min_size(pos2(ex, r.min.y + 9.0), vec2(14.0, 14.0)), small);
                    ex -= 15.0;
                }
            }
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
    let font = FontId::new(12.5, FontFamily::Name(super::STRONG.into()));
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
    let (party, allies) = party_rows(p.look(), w);
    // ptw0 ("Solo") while our party has no leader.
    let title = if party.len() <= 1 && w.party.slots().leaders[0] == 0 { "Solo" } else { "Party" };
    let mut picked = party_panel(p, "party", title, &party, small);
    for (id, rows) in ["alliance1", "alliance2"].into_iter().zip(&allies) {
        if let Some(rows) = rows {
            picked = picked.or(party_panel(p, id, "Alliance", rows, small));
        }
    }
    if let Some(k) = picked {
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
    w.stats.as_ref()?;
    Some(StatusView { effects: effects(p, w) })
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
        zone: w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, w.pos_zone as u32)).unwrap_or_else(|| crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.area_name(w.pos_zone)))),
        grid: p.m.grid(w, gu),
        time: (m / 60, m % 60),
        day: (day, name),
        weather: weather_element(w.weather),
        compass: compass_view(w),
    })
}

/// The party window's rows (ours first, from our own vitals; the others from the roster, slots 1..5) and the
/// alliances' (slots 6..17), the latter hidden while a window retail hides them under is up.
fn party_rows(p: Look, w: &World) -> (Vec<PartyRow>, [Option<Vec<PartyRow>>; 2]) {
    let slots = w.party.slots();
    let hand_at = target::party_hand(w);
    let hand = hand_at.map(|h| h.0);
    let hand_of = |slot: usize| hand_at.filter(|h| h.0 == slot).map(|h| h.1);
    let hp_colour = |hpp: u8| p.gu.map_or(p.m.theme.color("text"), |gu| super::chat::colour(gu.hp_tint(hpp).to_le_bytes()));
    let marked = |slot: usize, id: u32, index: u16| hand == Some(slot) || w.target.is_some_and(|t| t == id || (id != w.my_id && w.entities.get(&t).is_some_and(|e| e.index == index)));
    let marks = |id: u32, party: usize| (id != 0 && id == slots.leaders[party]) as u8 | ((id != 0 && id == slots.alliance_leader) as u8) << 1;
    let zone = |z: u16| p.gu.map(|gu| crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.area_name(z)))).unwrap_or_default();
    let effects = |id: u32| w.party_buffs.get(&id).map_or(Vec::new(), |row| row.iter().filter(|&&b| b != 0xffff && b != 0).map(|&b| Effect { icon: Icon::Game(b), label: None, ending: false }).collect());
    let gauge = |k: usize, hpp: u8, mpp: u8| w.party_gauges.get(k).map_or((hpp, mpp), |g| (g.1, g.2));
    // A member's max HP: as last seen at 100 %, else HP / HP %.
    let hp_max = |id: u32, hp: u32, hpp: u8| match w.party_hp_max.get(&id) {
        Some(&max) if hp <= max => Some(max),
        _ => (hpp > 0).then(|| ((hp as f32 * 100.0 / hpp as f32).round() as u32).max(hp)),
    };
    let mut party = Vec::new();
    if let Some((hp, mp, tp, hpp, mpp)) = w.vitals {
        let own = w.party.own();
        let name = if own.id == w.my_id { monstrosity::row_name(own).to_string() } else { w.name.clone() };
        let (gh, gm) = gauge(0, hpp, mpp);
        party.push(PartyRow {
            slot: 0,
            name,
            icon: w.stats.as_ref().and_then(|s| job_icon(s.main.0)),
            sub_icon: w.stats.as_ref().and_then(|s| job_icon(s.sub.0)),
            here: Some(RowVitals { hp, hp_max: w.stats.as_ref().map(|s| s.hp_max), mp, tp: w.party_tp.then_some(tp), hpf: gh as f32 / 100.0, mpf: Some(gm as f32 / 100.0), hp_colour: hp_colour(hpp) }),
            zone: String::new(),
            marked: marked(0, w.my_id, 0),
            hand: hand_of(0),
            marks: marks(w.my_id, 0),
            effects: Vec::new(),
        });
    }
    for (k, m) in (1..6).map_while(|k| slots.slots[k].as_ref().map(|m| (k, m))) {
        let (gh, gm) = gauge(k, m.hpp, m.mpp);
        let trust = m.index >= 0x700;
        party.push(PartyRow {
            slot: k,
            name: if m.away == 0 { monstrosity::row_name(m) } else { &m.name }.to_string(),
            icon: job_icon(m.jobs[0]),
            sub_icon: job_icon(m.jobs[2]),
            here: (m.away == 0 && m.zone == w.pos_zone).then(|| RowVitals { hp: m.hp, hp_max: hp_max(m.id, m.hp, m.hpp), mp: m.mp, tp: (w.party_tp && !trust).then_some(m.tp), hpf: gh as f32 / 100.0, mpf: Some(gm as f32 / 100.0), hp_colour: hp_colour(m.hpp) }),
            zone: zone(m.zone),
            marked: marked(k, m.id, m.index),
            hand: hand_of(k),
            marks: marks(m.id, 0),
            effects: effects(m.id),
        });
    }
    let hidden = super::super::alliance_hidden(w);
    let allies = std::array::from_fn(|a| {
        (slots.counts[a + 1] > 0 && !hidden).then(|| {
            (0..6)
                .filter_map(|i| {
                    let s = 6 * (a + 1) + i;
                    let m = slots.slots[s].as_ref()?;
                    let gh = w.ally_gauges.get(s - 6).map_or(m.hpp, |g| g.1);
                    Some(PartyRow {
                        slot: s,
                        name: m.name.clone(),
                        icon: job_icon(m.jobs[0]),
                        sub_icon: job_icon(m.jobs[2]),
                        here: (m.away == 0 && m.zone == w.pos_zone).then(|| RowVitals { hp: m.hp, hp_max: hp_max(m.id, m.hp, m.hpp), mp: m.mp, tp: (w.party_tp && m.index < 0x700).then_some(m.tp), hpf: gh as f32 / 100.0, mpf: None, hp_colour: hp_colour(m.hpp) }),
                        zone: zone(m.zone),
                        marked: marked(s, m.id, m.index),
                        hand: hand_of(s),
                        marks: marks(m.id, a + 1),
                        effects: Vec::new(),
                    })
                })
                .collect()
        })
    });
    (party, allies)
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
