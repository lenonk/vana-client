//! The Extras in the modern layer: the Timers window (timers.rs's sections: recasts, our effects and those we put on
//! others, folding as retail's), the minimap (round, north up, centred on us, the radar's dots; the wheel zooms) and
//! the map overlay (Ctrl / Alt + O: the zone map in a box at its opacity; the game's mouse still pans and zooms it).

use super::super::map::MapImage;
use super::super::timers::{self, Group, Icon, SECTIONS};
use super::super::{World, event, minimap};
use super::Panels;
use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

pub fn draw(p: &mut Panels, w: &mut World) {
    timers_panel(p, w);
    minimap_panel(p, w);
    widescan_panel(p, w);
    overlay(p, w);
}

/// Hidden in events, under the map, the Region Info map.
fn hidden(w: &World) -> bool {
    event::active(w) || w.map.is_some() || w.cnq.is_some()
}

/// The time left as retail's clock ("1:05", "1:00:00").
fn clock(left: f32) -> String {
    let s = left.max(0.0).ceil() as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The Ashita Timers colours by time left: green from 30 s, yellow from 20, orange from 10, else red.
fn bar_colour(left: f32) -> Color32 {
    match left {
        l if l >= 30.0 => Color32::from_rgb(70, 190, 90),
        l if l >= 20.0 => Color32::from_rgb(220, 200, 60),
        l if l >= 10.0 => Color32::from_rgb(230, 140, 60),
        _ => Color32::from_rgb(220, 60, 60),
    }
}

/// The Timers window on a screen edge (edge_window): a heading that folds it (retail's drawer) to a tab; per section
/// a heading (a click folds it) and its rows: the icon, the name, the time and a bar filling toward a recast or
/// draining with an effect; the last 15 s of an effect pulse as the status icons do.
fn timers_panel(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    if !w.extra_timers || hidden(w) {
        return;
    }
    let secs = timers::sections(w, gu);
    if secs.iter().all(|s| s.iter().all(|g| g.rows.is_empty())) {
        return;
    }
    let (m, ctx) = (p.m, p.ctx);
    let small = w.game.get(0x99) & 4 != 0;
    let t = 40.0 - (w.started.elapsed().as_secs_f32() * 60.0) % 40.0;
    let pulse = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
    let (out, folded) = (w.timers.out, w.timers.folded);
    let icon = |i: &Icon| match *i {
        Icon::Status(id) => m.status_icon(ctx, gu, small, id),
        Icon::List(lib, k) => m.sprite_icon(ctx, gu, lib, k),
        Icon::None => None,
    };
    let icons: Vec<Vec<Vec<Option<egui::TextureId>>>> = if out { secs.iter().map(|s| s.iter().map(|g| g.rows.iter().map(|r| icon(&r.icon)).collect()).collect()).collect() } else { Vec::new() };
    let mut fold: Option<usize> = None;
    let toggle = edge_window(p, "timers", WIDTH, !out, &[], "Timers", |ui, left| {
        let toggle = flyout_header(ui, m, "Timers", left).clicked();
        for (k, groups) in secs.iter().enumerate() {
            let n: usize = groups.iter().map(|g| g.rows.len()).sum();
            if n == 0 {
                continue;
            }
            ui.add_space(4.0);
            let shut = folded & (1 << k) != 0;
            if header(ui, m, &format!("{} ({n})", SECTIONS[k]), !shut).clicked() {
                fold = Some(k);
            }
            if shut {
                continue;
            }
            for (gi, g) in groups.iter().enumerate().filter(|(_, g)| !g.rows.is_empty()) {
                group(ui, m, g, &icons[k][gi], pulse);
            }
        }
        toggle
    });
    if toggle {
        w.timers.toggle();
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = fold {
        w.timers.folded ^= 1 << k;
        w.timers.dirty = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// A window on the screen's left or right edge (its place: x 0 or above the left, below 0 the right; y its top): its
/// frame open on that side; folded, a small tab out of the edge with `label` running down it beside a double chevron. Open, it stands clear of `avoid`
/// (the windows at the same edge) by moving off the edge. Dragged it moves up and down, and let go on the screen's
/// other half it goes to that edge. `add` draws the contents (told the side) and says whether its fold button was
/// clicked; the tab's click counts too. Returns that.
#[allow(clippy::too_many_arguments)]
fn edge_window(p: &mut Panels, key: &str, width: f32, folded: bool, avoid: &[Rect], label: &str, add: impl FnOnce(&mut Ui, bool) -> bool) -> bool {
    let (m, ctx) = (p.m, p.ctx);
    let screen = ctx.content_rect();
    let mut place = p.place(key);
    let left = place.x >= 0.0;
    let top = (screen.height() * place.y).clamp(screen.min.y, screen.max.y - TAB_H);
    let id = egui::Id::unique(("modern edge", key));
    let mut toggle = false;
    if folded {
        // The label runs down the tab by the screen's edge; the double chevron points into the screen.
        let font = FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into()));
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, Color32::WHITE));
        let gw = galley.size().x;
        let size = vec2(TAB_W, (gw + 16.0).max(TAB_H));
        let r = if left { Rect::from_min_size(pos2(screen.min.x, top), size) } else { Rect::from_min_size(pos2(screen.max.x - TAB_W, top), size) };
        let mut rect = r;
        let mut dy = None;
        egui::Area::new(id).order(egui::Order::Middle).fixed_pos(r.min).constrain(false).show(ctx, |ui| {
            // A click opens it; a drag slides it along the edge.
            let (rr, resp) = ui.allocate_exact_size(r.size(), Sense::click_and_drag());
            if resp.dragged() {
                dy = Some(resp.drag_delta().y);
            }
            rect = rr;
            ui.painter().add(edge_frame_shape(rr, left, true, m, false));
            let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
            // The word turned toward the screen: a quarter counter-clockwise on the left (read bottom up), clockwise on
            // the right (read top down).
            let (at, angle) = if left { (pos2(rr.min.x + 3.0, rr.center().y + gw / 2.0), -std::f32::consts::FRAC_PI_2) } else { (pos2(rr.max.x - 3.0, rr.center().y - gw / 2.0), std::f32::consts::FRAC_PI_2) };
            ui.painter().add(egui::epaint::TextShape::new(at, galley, c).with_angle(angle));
            let cx = if left { rr.max.x - 8.0 } else { rr.min.x + 8.0 };
            for d in [-3.0, 3.0] {
                chevron(ui.painter(), pos2(cx + d, rr.center().y), !left, c);
            }
            toggle = resp.clicked();
        });
        p.this.insert(key.to_string(), rect);
        if let Some(dy) = dy {
            place.y = ((top + dy) / screen.height().max(1.0)).clamp(0.0, 0.95);
            m.arranging.set(true);
            p.layout.insert(key.to_string(), place);
            *p.changed = true;
        }
        return toggle;
    }
    let h = m.rects.borrow().get(key).map_or(TAB_H, |r| r.height());
    let edge_x = if left { screen.min.x } else { screen.max.x - width };
    // Stepped clear of each window in its way, until none is (one moved past may meet the next).
    let mut x = edge_x;
    for _ in 0..avoid.len() {
        let before = x;
        for r in avoid.iter().filter(|r| r.min.y < top + h && r.max.y > top) {
            if left && r.min.x < x + width && r.max.x > x {
                x = x.max(r.max.x + 4.0);
            } else if !left && r.max.x > x && r.min.x < x + width {
                x = x.min(r.min.x - 4.0 - width);
            }
        }
        if x == before {
            break;
        }
    }
    let attached = x == edge_x;
    let margin = egui::Margin { left: if left && attached { 12 } else { 18 }, right: if !left && attached { 12 } else { 18 }, top: 12, bottom: 14 };
    let mut moved: Option<(f32, Option<Pos2>)> = None;
    let shown = egui::Area::new(id).order(egui::Order::Middle).fixed_pos(pos2(x, top)).constrain(false).show(ctx, |ui| {
        let bg = ui.painter().add(Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
            ui.set_width(width - margin.sum().x);
            ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
            toggle = add(ui, left);
        });
        let r = inner.response.rect;
        ui.painter().set(bg, edge_frame_shape(r, left, attached, m, true));
        // The background moves it (the contents keep their clicks).
        let drag = ui.interact(r, id.with("drag"), Sense::drag());
        if drag.dragged() {
            moved = Some((drag.drag_delta().y, None));
        }
        if drag.drag_stopped() {
            moved = Some((0.0, drag.interact_pointer_pos().or(ctx.pointer_latest_pos())));
        }
        r
    });
    p.this.insert(key.to_string(), shown.inner);
    if let Some((dy, released)) = moved {
        place.y = ((top + dy) / screen.height().max(1.0)).clamp(0.0, 0.95);
        if let Some(at) = released {
            place.x = if at.x < screen.center().x { 0.0 } else { -0.001 };
        }
        m.arranging.set(true);
        p.layout.insert(key.to_string(), place);
        *p.changed = true;
    }
    toggle
}

/// The window's width and its folded tab's size.
const WIDTH: f32 = 250.0;
const TAB_W: f32 = 34.0;
const TAB_H: f32 = 56.0;

/// A chevron at `c` pointing left or right.
fn chevron(p: &egui::Painter, c: Pos2, left: bool, col: Color32) {
    let pts = if left { vec![pos2(c.x + 3.5, c.y - 6.0), pos2(c.x + 3.5, c.y + 6.0), pos2(c.x - 3.5, c.y)] } else { vec![pos2(c.x - 3.5, c.y - 6.0), pos2(c.x - 3.5, c.y + 6.0), pos2(c.x + 3.5, c.y)] };
    p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// The edge window's frame over `r`: no border on the anchored side (it runs past the screen's edge), the other
/// corners rounded, a bronze line with a thin gold one inside it, and on the inner side a small diamond when `jewel`.
fn edge_frame_shape(r: Rect, left: bool, attached: bool, m: &super::Modern, jewel: bool) -> Shape {
    let ext = match (attached, left) {
        (false, _) => r,
        (true, true) => Rect::from_min_max(pos2(r.min.x - 10.0, r.min.y), r.max),
        (true, false) => Rect::from_min_max(r.min, pos2(r.max.x + 10.0, r.max.y)),
    };
    let round = |k: u8| match (attached, left) {
        (false, _) => egui::CornerRadius::same(k),
        (true, true) => egui::CornerRadius { nw: 0, sw: 0, ne: k, se: k },
        (true, false) => egui::CornerRadius { nw: k, sw: k, ne: 0, se: 0 },
    };
    let mut v = vec![
        Shape::Rect(egui::epaint::RectShape::filled(ext, round(16), m.theme.color("surface").gamma_multiply(0.95))),
        Shape::Rect(egui::epaint::RectShape::stroke(ext, round(16), Stroke::new(2.0, m.theme.color("bronze")), egui::StrokeKind::Inside)),
        Shape::Rect(egui::epaint::RectShape::stroke(ext.shrink(4.0), round(12), Stroke::new(1.0, m.theme.color("gold").gamma_multiply(0.45)), egui::StrokeKind::Inside)),
    ];
    if jewel {
        let c = pos2(if left { ext.max.x - 1.0 } else { ext.min.x + 1.0 }, ext.center().y);
        let d = 5.0;
        v.push(Shape::convex_polygon(vec![pos2(c.x, c.y - d), pos2(c.x + d, c.y), pos2(c.x, c.y + d), pos2(c.x - d, c.y)], m.theme.color("gold"), Stroke::new(1.0, Color32::from_black_alpha(200))));
    }
    Shape::Vec(v)
}

/// The Timers heading: the title, and on the inner side a chevron toward the edge (a click folds it there).
fn flyout_header(ui: &mut Ui, m: &super::Modern, text: &str, left: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
    let font = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    let cy = r.center().y;
    if left {
        ui.painter().text(pos2(r.min.x, cy), Align2::LEFT_CENTER, text, font, c);
        chevron(ui.painter(), pos2(r.max.x - 6.0, cy), true, c);
    } else {
        ui.painter().text(pos2(r.min.x + 18.0, cy), Align2::LEFT_CENTER, text, font, c);
        chevron(ui.painter(), pos2(r.min.x + 5.0, cy), false, c);
    }
    resp
}

/// A heading with its fold chevron; the response takes clicks.
fn header(ui: &mut Ui, m: &super::Modern, text: &str, open: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
    ui.painter().text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, text, FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into())), c);
    // A chevron: down while open, right while folded.
    let (cx, cy) = (r.max.x - 8.0, r.center().y);
    let pts = if open { vec![pos2(cx - 5.0, cy - 2.5), pos2(cx + 5.0, cy - 2.5), pos2(cx, cy + 3.5)] } else { vec![pos2(cx - 2.5, cy - 5.0), pos2(cx - 2.5, cy + 5.0), pos2(cx + 3.5, cy)] };
    ui.painter().add(Shape::convex_polygon(pts, c, Stroke::NONE));
    resp
}

fn group(ui: &mut Ui, m: &super::Modern, g: &Group, icons: &[Option<egui::TextureId>], pulse: f32) {
    if let Some(name) = &g.name {
        ui.label(egui::RichText::new(name).size(m.theme.small).color(m.theme.color("muted")));
    }
    for (r, icon) in g.rows.iter().zip(icons) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let waiting = r.left <= 0.0;
        let k = if !r.fills && r.left > 0.0 && r.left <= 15.0 { 0.5 + 0.5 * pulse } else { 1.0 };
        let tint = Color32::from_white_alpha((255.0 * k) as u8);
        if let Some(t) = icon {
            ui.painter().image(*t, Rect::from_min_size(pos2(rect.min.x, rect.min.y + 3.0), vec2(22.0, 22.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        let text = if waiting { m.theme.color("muted") } else { m.theme.color("text") };
        let x = rect.min.x + 30.0;
        ui.painter().text(pos2(x, rect.min.y + 2.0), Align2::LEFT_TOP, &r.name, FontId::proportional(m.theme.small), text);
        let time = if r.total <= 0.0 { "--:--".to_string() } else { clock(r.left) };
        ui.painter().text(pos2(rect.max.x, rect.min.y + 2.0), Align2::RIGHT_TOP, time, FontId::monospace(m.theme.small), text);
        if r.total > 0.0 {
            let f = (r.left / r.total).clamp(0.0, 1.0);
            let f = if r.fills { 1.0 - f } else { f };
            let bar = Rect::from_min_max(pos2(x, rect.max.y - 7.0), pos2(rect.max.x, rect.max.y - 3.0));
            ui.painter().rect_filled(bar, 2.0, m.theme.color("charcoal"));
            if f > 0.0 {
                let fill = Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f, bar.max.y));
                ui.painter().rect_filled(fill, 2.0, bar_colour(r.left).gamma_multiply(k));
            }
        }
    }
}

/// The map image as an egui texture, made once per image.
fn map_texture(m: &super::Modern, ctx: &egui::Context, img: &MapImage) -> egui::TextureId {
    let mut images = m.images.borrow_mut();
    let t = images.entry(img.name.clone()).or_insert_with(|| Some(ctx.load_texture(img.name.clone(), egui::ColorImage::from_rgba_unmultiplied(img.size, &img.rgba), egui::TextureOptions::LINEAR)));
    t.as_ref().map_or(egui::TextureId::default(), |t| t.id())
}

/// Our arrow at `c` pointing along `facing` (radians; 0 east, turning toward south on the map).
fn arrow(painter: &egui::Painter, c: Pos2, facing: f32, size: f32, m: &super::Modern) {
    let d = vec2(facing.cos(), facing.sin());
    let n = vec2(-d.y, d.x);
    let pts = vec![c + d * size, c - d * size * 0.55 + n * size * 0.65, c - d * size * 0.25, c - d * size * 0.55 - n * size * 0.65];
    painter.add(Shape::convex_polygon(pts.clone(), m.theme.color("gold"), Stroke::new(1.5, Color32::from_black_alpha(220))));
}

/// The minimap on a screen edge (edge_window): the zone and grid over a square map, north up, centred on us,
/// `span` map pixels across; the radar's dots (the target larger with a red ring), our arrow, N at the top. Open, it
/// keeps clear of the command menu and the windows at its edge (retail's minimap does); folded, a tab under them.
/// The wheel zooms.
fn minimap_panel(p: &mut Panels, w: &mut World) {
    let Some(f) = w.minimap_frame.as_ref() else { return };
    if !w.extra_minimap || hidden(w) {
        return;
    }
    let Some(img) = w.minimap.as_mut().and_then(|o| o.modern_image()) else { return };
    let (m, ctx) = (p.m, p.ctx);
    let tex = map_texture(m, ctx, &img);
    let d = minimap::SIZES[w.minimap_size.min(2) as usize];
    let (pos, facing, span) = (f.pos, f.facing, f.span);
    let dots = f.dots.clone();
    let avoid = column(p, w);
    let label = p.gu.map(|gu| format!("{}  {}", crate::dat::font::decode_sjis(&gu.area_name(w.pos_zone)), m.grid(w, gu))).unwrap_or_default();
    let folded = p.folded.get("minimap").copied().unwrap_or(false);
    let mut wheel = 0.0f32;
    let toggle = edge_window(p, "minimap", d + 30.0, folded, &avoid, "Map", |ui, left| {
        // The heading: the zone and grid, the fold chevron on the inner side.
        let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
        let c = if hresp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
        let font = FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into()));
        if left {
            ui.painter().text(pos2(hr.min.x, hr.center().y), Align2::LEFT_CENTER, &label, font, c);
            chevron(ui.painter(), pos2(hr.max.x - 6.0, hr.center().y), true, c);
        } else {
            ui.painter().text(pos2(hr.min.x + 18.0, hr.center().y), Align2::LEFT_CENTER, &label, font, c);
            chevron(ui.painter(), pos2(hr.min.x + 5.0, hr.center().y), false, c);
        }
        ui.add_space(4.0);
        let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_width()), Sense::hover());
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, m.theme.color("charcoal"));
        let s = minimap::px_per_unit(&img.region);
        let z = area.width() / span;
        let (px, py) = (pos.0 * s, -pos.1 * s);
        let u0 = pos2((px - span / 2.0 - img.region.ox as f32) / 512.0, (py - span / 2.0 - img.region.oy as f32) / 512.0);
        painter.image(tex, area, Rect::from_min_size(u0, vec2(span / 512.0, span / 512.0)), Color32::WHITE);
        let c = area.center();
        for &(x, wz, colour, target) in &dots {
            let q = c + vec2((x * s - px) * z, (-wz * s - py) * z);
            if !area.shrink(2.0).contains(q) {
                continue;
            }
            let col = Color32::from_rgb((colour >> 16) as u8, (colour >> 8) as u8, colour as u8);
            if target {
                painter.circle(q, 4.5, col, Stroke::new(2.0, Color32::from_rgb(220, 40, 40)));
            } else {
                painter.circle(q, 3.0, col, Stroke::new(1.0, Color32::from_black_alpha(200)));
            }
        }
        arrow(&painter, c, facing, 9.0, m);
        ui.painter().rect_stroke(area, 2.0, Stroke::new(1.5, m.theme.color("bronze")), egui::StrokeKind::Outside);
        // N at the top.
        let n = pos2(area.center().x, area.min.y + 10.0);
        painter.circle(n, 8.0, m.theme.color("charcoal").gamma_multiply(0.85), Stroke::new(1.0, m.theme.color("bronze")));
        painter.text(n, Align2::CENTER_CENTER, "N", FontId::new(m.theme.small - 1.0, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
        if resp.contains_pointer() {
            wheel = ui.input(|i| {
                i.events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y,
                        egui::Event::MouseWheel { delta, .. } => delta.y / 50.0,
                        _ => 0.0,
                    })
                    .sum::<f32>()
            });
        }
        hresp.clicked()
    });
    if toggle {
        p.folded.insert("minimap".into(), !folded);
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if wheel != 0.0 {
        w.minimap_span = (w.minimap_span * 1.25f32.powf(-wheel)).clamp(minimap::SPAN.0, minimap::SPAN.1);
        w.minimap_dirty = true;
    }
}

/// What the windows at the right edge take (the command column as last drawn, and the retail windows there: where
/// retail puts its own minimap's right edge); edge windows open there keep clear of it.
fn column(p: &Panels, w: &World) -> Vec<Rect> {
    let screen = p.ctx.content_rect();
    let mut avoid: Vec<Rect> = super::COLUMN.iter().chain(&["command sub"]).filter_map(|c| p.m.rects.borrow().get(*c).copied()).collect();
    if let Some(f) = w.minimap_frame.as_ref() {
        let right = (f.rect[0] + f.rect[2]) / w.ui_wh.0.max(1.0) * screen.width();
        if right < screen.max.x - 2.0 {
            avoid.push(Rect::from_min_max(pos2(right + 4.0, screen.min.y), screen.max));
        }
    }
    avoid
}

/// Wide Scan as an edge window under the map (folded by default): the results by distance in their colours (players
/// blue, NPCs green, monsters red) with their level and how far and which way they were; a click tracks one, a click
/// on the tracked one asks to stop. Retail asks for a list only as its map opens; open, this asks every 10 s while
/// nothing is tracked, and shows the last complete list until the next one is all in.
fn widescan_panel(p: &mut Panels, w: &mut World) {
    if !w.extra_minimap || hidden(w) {
        return;
    }
    let (m, ctx) = (p.m, p.ctx);
    let folded = p.folded.get("widescan").copied().unwrap_or(true);
    // Unmoved, under the map on its edge.
    if !p.layout.contains_key("widescan")
        && let Some(r) = p.this.get("minimap").copied()
    {
        let mp = p.place("minimap");
        p.retail.insert("widescan".into(), super::Place { x: mp.x, y: (r.max.y + 6.0) / ctx.content_rect().height().max(1.0), ..Default::default() });
    }
    if w.scan.busy && w.scan.list_state == 2 {
        w.scan.busy = false;
    }
    let now = std::time::Instant::now();
    if !folded && w.scan.track.index == 0 && w.scan.list_state != 1 && m.scan_at.get().is_none_or(|t| now.duration_since(t).as_secs_f32() >= 10.0) {
        m.scan_at.set(Some(now));
        super::super::scan_request(w);
    }
    // Named as retail's list (FUN_101fb4b0): players by their own name, NPCs and monsters from the zone's NPC list by
    // index & 0x3ff; nameless ones and ourselves left out.
    let names: std::collections::HashMap<u16, &String> = w.npc_names.iter().map(|(id, n)| ((id & 0x3ff) as u16, n)).collect();
    let mut rows: Vec<(super::super::widescan::Entry, String)> = w
        .scan
        .entries
        .values()
        .filter(|e| !(e.kind == 0 && e.index == w.my_index))
        .filter_map(|e| {
            let name = if e.kind == 0 { e.name.clone() } else { names.get(&(e.index & 0x3ff)).map(|n| n.to_string()).unwrap_or_default() };
            (!name.is_empty()).then(|| (e.clone(), name))
        })
        .collect();
    rows.sort_by(|a, b| (a.0.dx as f32).hypot(a.0.dz as f32).total_cmp(&(b.0.dx as f32).hypot(b.0.dz as f32)));
    // While a list streams in (state 1, or asked and not begun) the last complete one stays.
    let streaming = w.scan.list_state == 1 || (w.scan.busy && w.scan.list_state != 2);
    if !streaming {
        *m.scan_rows.borrow_mut() = rows;
    }
    let rows = m.scan_rows.borrow().clone();
    let tracked = w.scan.track.index;
    let avoid = column(p, w);
    let mut pick: Option<u16> = None;
    let toggle = edge_window(p, "widescan", 290.0, folded, &avoid, "Wide Scan", |ui, left| {
        let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
        let c = if hresp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
        let font = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
        let tx = if left { hr.min.x } else { hr.min.x + 18.0 };
        ui.painter().text(pos2(tx, hr.center().y), Align2::LEFT_CENTER, format!("Wide Scan ({})", rows.len()), font, c);
        chevron(ui.painter(), pos2(if left { hr.max.x - 6.0 } else { hr.min.x + 5.0 }, hr.center().y), left, c);
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(320.0).min_scrolled_height(320.0).show(ui, |ui| {
            for (e, name) in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                let on = e.index == tracked;
                if on || resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, m.theme.color("focus").gamma_multiply(if on { 0.35 } else { 0.15 }));
                }
                let col = match e.kind {
                    0 => Color32::from_rgb(110, 170, 255),
                    1 => Color32::from_rgb(110, 210, 120),
                    _ => Color32::from_rgb(235, 95, 95),
                };
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), col);
                let d = (e.dx as f32).hypot(e.dz as f32);
                let info = if e.level > 0 { format!("Lv{} {d:.0}", e.level) } else { format!("{d:.0}") };
                ui.painter().text(pos2(r.max.x - 26.0, r.center().y), Align2::RIGHT_CENTER, info, FontId::monospace(m.theme.small - 1.0), m.theme.color("muted"));
                arrow(ui.painter(), pos2(r.max.x - 12.0, r.center().y), (-(e.dz as f32)).atan2(e.dx as f32), 6.0, m);
                // Players are not tracked (retail's list).
                if resp.clicked() && e.kind != 0 {
                    pick = Some(e.index);
                }
            }
        });
        hresp.clicked()
    });
    if toggle {
        p.folded.insert("widescan".into(), !folded);
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    match pick {
        Some(i) if i == tracked => w.confirm = Some((super::super::Confirm::StopTracking, false)),
        Some(i) => super::super::scan_track(w, i),
        None => {}
    }
}

/// The map overlay: the zone map in its centred box at its opacity, a bronze edge, our arrow; under every panel.
fn overlay(p: &mut Panels, w: &mut World) {
    let (sw, sh) = w.ui_wh;
    let opacity = w.map_opacity;
    let facing = w.net.me.lock().map_or(0.0, |m| event::rad_of(m.dir));
    let pos = (w.pos.x, w.pos.z);
    let Some(o) = w.overlay.as_mut().filter(|o| o.has_map()) else { return };
    let (Some(img), Some((rect, view, zoom))) = (o.modern_image(), o.modern_view(sw, sh)) else { return };
    let (m, ctx) = (p.m, p.ctx);
    let tex = map_texture(m, ctx, &img);
    let screen = ctx.content_rect();
    let k = Vec2::new(screen.width() / sw.max(1.0), screen.height() / sh.max(1.0));
    let boxr = Rect::from_min_size(pos2(rect[0] * k.x, rect[1] * k.y), vec2(rect[2] * k.x, rect[3] * k.y));
    let z = vec2(zoom * k.x, zoom * k.y);
    let painter = ctx.layer_painter(egui::LayerId::background()).with_clip_rect(boxr);
    let img_r = Rect::from_min_size(boxr.min + vec2((img.region.ox as f32 - view.0) * z.x, (img.region.oy as f32 - view.1) * z.y), vec2(512.0 * z.x, 512.0 * z.y));
    painter.image(tex, img_r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha((255.0 * opacity.clamp(0.0, 1.0)) as u8));
    let (px, py) = img.region.px(pos.0, pos.1);
    let me = boxr.min + vec2((px as f32 - view.0) * z.x, (py as f32 - view.1) * z.y);
    arrow(&painter, me, facing, 11.0, m);
    let edge = ctx.layer_painter(egui::LayerId::background());
    edge.rect_stroke(boxr, 6.0, Stroke::new(2.0, m.theme.color("bronze").gamma_multiply(0.8)), egui::StrokeKind::Outside);
}
