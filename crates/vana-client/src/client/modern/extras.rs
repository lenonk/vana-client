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

/// The Timers: a heading that folds the window, then per section its heading (a click folds it) and its rows: the
/// icon, the name, the time and a bar filling toward a recast or draining with an effect; the last 15 s of an effect
/// pulse as the status icons do.
fn timers_panel(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    if !w.extra_timers || hidden(w) {
        return;
    }
    let secs = timers::sections(w, gu);
    if secs.iter().all(|s| s.iter().all(|g| g.rows.is_empty())) {
        return;
    }
    let m = p.m;
    let ctx = p.ctx;
    let small = w.game.get(0x99) & 4 != 0;
    let t = 40.0 - (w.started.elapsed().as_secs_f32() * 60.0) % 40.0;
    let pulse = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
    let (out, folded) = (w.timers.out, w.timers.folded);
    let icon = |i: &Icon| match *i {
        Icon::Status(id) => m.status_icon(ctx, gu, small, id),
        Icon::List(lib, k) => m.sprite_icon(ctx, gu, lib, k),
        Icon::None => None,
    };
    let icons: Vec<Vec<Vec<Option<egui::TextureId>>>> = secs.iter().map(|s| s.iter().map(|g| g.rows.iter().map(|r| icon(&r.icon)).collect()).collect()).collect();
    let mut toggle = false;
    let mut fold: Option<usize> = None;
    p.panel("timers", None, 250.0, |ui| {
        if header(ui, m, "Timers", out).clicked() {
            toggle = true;
        }
        if !out {
            return;
        }
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

/// The minimap: round, north up, centred on us, `span` map pixels across; the radar's dots (the target larger with a
/// red ring), our arrow, N at the top; the zone and grid under it. The wheel zooms.
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
    // Unmoved, it goes where retail's goes: its right edge where retail's is (left of the windows at the edge).
    let (sw, sh) = w.ui_wh;
    let [rx, ry, rw, _] = f.rect;
    p.retail.insert("minimap".into(), super::Place { x: ((rx + rw) / sw.max(1.0) - 1.0).min(-0.0001), y: ry / sh.max(1.0), ..Default::default() });
    let label = p.gu.map(|gu| format!("{}  {}", crate::dat::font::decode_sjis(&gu.area_name(w.pos_zone)), m.grid(w, gu))).unwrap_or_default();
    let mut wheel = 0.0f32;
    p.framed("minimap", None, d, "none", |ui| {
        let (area, resp) = ui.allocate_exact_size(vec2(d, d), Sense::hover());
        let c = area.center();
        let r = d / 2.0 - 4.0;
        let painter = ui.painter();
        painter.circle_filled(c, r + 3.0, m.theme.color("charcoal"));
        let s = minimap::px_per_unit(&img.region);
        let z = 2.0 * r / span;
        let (px, py) = (pos.0 * s, -pos.1 * s);
        let uv = |q: Pos2| pos2((px + (q.x - c.x) / z - img.region.ox as f32) / 512.0, (py + (q.y - c.y) / z - img.region.oy as f32) / 512.0);
        // The map as a fan of triangles: the disc.
        let mut mesh = egui::Mesh::with_texture(tex);
        mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: uv(c), color: Color32::WHITE });
        const N: u32 = 72;
        for k in 0..=N {
            let a = k as f32 / N as f32 * std::f32::consts::TAU;
            let q = c + vec2(a.cos(), a.sin()) * r;
            mesh.vertices.push(egui::epaint::Vertex { pos: q, uv: uv(q), color: Color32::WHITE });
            if k > 0 {
                mesh.indices.extend([0, k, k + 1]);
            }
        }
        painter.add(Shape::mesh(mesh));
        for &(x, wz, colour, target) in &dots {
            let q = c + vec2((x * s - px) * z, (-wz * s - py) * z);
            if (q - c).length() > r - 3.0 {
                continue;
            }
            let col = Color32::from_rgb((colour >> 16) as u8, (colour >> 8) as u8, colour as u8);
            if target {
                painter.circle(q, 4.5, col, Stroke::new(2.0, Color32::from_rgb(220, 40, 40)));
            } else {
                painter.circle(q, 3.0, col, Stroke::new(1.0, Color32::from_black_alpha(200)));
            }
        }
        arrow(painter, c, facing, 9.0, m);
        // The ring and N at the top.
        painter.circle_stroke(c, r + 1.0, Stroke::new(3.0, m.theme.color("bronze")));
        painter.circle_stroke(c, r + 3.0, Stroke::new(1.0, Color32::from_black_alpha(200)));
        let n = pos2(c.x, c.y - r - 1.0);
        painter.circle(n, 9.0, m.theme.color("charcoal"), Stroke::new(1.5, m.theme.color("bronze")));
        painter.text(n, Align2::CENTER_CENTER, "N", FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
        if !label.is_empty() {
            ui.add_space(4.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(&label).size(m.theme.small).color(m.theme.color("text")));
            });
        }
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
    });
    if wheel != 0.0 {
        w.minimap_span = (w.minimap_span * 1.25f32.powf(-wheel)).clamp(minimap::SPAN.0, minimap::SPAN.1);
        w.minimap_dirty = true;
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
