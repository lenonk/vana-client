//! The Lua mods' HUD elements in the modern UI, drawn as the modern layer draws its elements (on the layer under its
//! panels, where the built-in map overlay stood): text in the UI's faces, rects with rounded corners, the pack's
//! gauges, icons, PNGs, maps with our arrow. A mod's element looks as a built-in one does. Text in the game's font is
//! the game UI's (lua/hud.rs), as the classic UI draws every element.

use super::super::World;
use super::super::game_ui::{GameUi, HudFace};
use super::super::lua::hud::{Elem, Kind};
use super::{Modern, paint};
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// A bar's colour by kind where the pack has no gauge (as the game UI's HUD bars).
fn flat(kind: &str) -> Color32 {
    match kind {
        "mp" => Color32::from_rgb(200, 120, 70),
        "tp" => Color32::from_rgb(80, 160, 220),
        "cast" => Color32::from_rgb(90, 210, 210),
        "experience" => Color32::from_rgb(200, 110, 210),
        "recast" => Color32::from_rgb(160, 160, 160),
        _ => Color32::from_rgb(90, 200, 90),
    }
}

pub(super) fn draw(
    m: &Modern,
    ctx: &egui::Context,
    gu: &GameUi,
    w: &World,
    vis: &std::collections::HashMap<String, f32>,
) {
    let host = &w.lua;
    let ppp = ctx.pixels_per_point();
    host.set_modern_scale(Some(ppp));
    if !host.hud_shown() {
        return;
    }
    let (panels, win) = host.hud_frame();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let pt = |v: f32| v / ppp;
    let mut rects = Vec::new();
    for (mod_id, e) in host.hud_elements() {
        if matches!(e.kind, Kind::Text { font: Some(HudFace::Game), .. }) {
            continue;
        }
        let Some((px, py)) = super::super::lua::hud::place(&e, &panels, win) else { continue };
        let at = pos2(pt(px), pt(py));
        // Its HUD window's opacity and the UI mode's (/uimodes), over everything it draws.
        let key = format!("hud:{mod_id}:{}", e.window);
        let fade = vis.get(&key).copied().unwrap_or(1.0);
        if fade <= 0.01 {
            continue;
        }
        let mut painter = painter.clone();
        painter.multiply_opacity(host.window_opacity(&w.window_opacity, &key) * fade);
        let Some(r) = element(m, ctx, gu, w, &painter, &e, at, ppp) else { continue };
        rects.push((mod_id, e.id.clone(), [r.min.x * ppp, r.min.y * ppp, r.width() * ppp, r.height() * ppp]));
    }
    host.add_hud_rects(rects);
}

/// One element at `at` (points); the rect it takes.
fn element(
    m: &Modern,
    ctx: &egui::Context,
    gu: &GameUi,
    w: &World,
    painter: &egui::Painter,
    e: &Elem,
    at: Pos2,
    ppp: f32,
) -> Option<Rect> {
    let pt = |v: f32| v / ppp;
    Some(match &e.kind {
        Kind::Text { text, size, colour, bg, align, pad, font, outline, outline_w, shadow } => {
            let family = if *font == Some(HudFace::Bold) {
                FontFamily::Name(super::STRONG.into())
            } else {
                FontFamily::Proportional
            };
            let fid = FontId::new(m.theme.ui * size, family);
            let colour = colour.map_or(m.theme.color("text"), rgba);
            let galley = painter.layout_no_wrap(text.clone(), fid, colour);
            let anchor = match align {
                1 => Align2::CENTER_TOP,
                2 => Align2::RIGHT_TOP,
                _ => Align2::LEFT_TOP,
            };
            let r = anchor.anchor_size(at, galley.size());
            let p = pt(*pad);
            let boxed = if bg.is_some() {
                Rect::from_min_max(pos2(r.min.x - p, r.min.y - p / 2.0), pos2(r.max.x + p, r.max.y + p / 2.0))
            } else {
                r
            };
            if let Some(b) = bg {
                painter.rect_filled(boxed, CornerRadius::same(3), rgba(*b));
            }
            // An outline round the text in 8 directions, or a drop shadow, under it.
            if let Some(o) = outline {
                let d = pt(outline_w * size.max(1.0));
                for (dx, dy) in [
                    (-1.0, -1.0),
                    (0.0, -1.0),
                    (1.0, -1.0),
                    (-1.0, 0.0),
                    (1.0, 0.0),
                    (-1.0, 1.0),
                    (0.0, 1.0),
                    (1.0, 1.0),
                ] {
                    painter.galley_with_override_text_color(r.min + vec2(dx, dy) * d, galley.clone(), rgba(*o));
                }
            }
            if *shadow {
                let d = pt(1.5 * size.max(1.0));
                painter.galley_with_override_text_color(
                    r.min + vec2(d, d),
                    galley.clone(),
                    Color32::from_black_alpha(200),
                );
            }
            painter.galley(r.min, galley, colour);
            boxed
        }
        Kind::Rect { w: rw, h: rh, colour, line, round } => {
            let r = Rect::from_min_size(at, vec2(pt(*rw), pt(*rh)));
            let radius = CornerRadius::same(pt(*round).round().clamp(0.0, 255.0) as u8);
            match line {
                Some(l) => {
                    painter.rect_stroke(r, radius, Stroke::new(pt(*l), rgba(*colour)), StrokeKind::Inside);
                }
                None => {
                    painter.rect_filled(r, radius, rgba(*colour));
                }
            }
            r
        }
        Kind::Bar { w: bw, h: bh, value, colour, kind } => {
            let r = Rect::from_min_size(at, vec2(pt(*bw), pt(*bh)));
            // The pack's gauge for its kind, as the panels' bars; a colour of the mod's: flat.
            match (colour, m.art.sprite(&format!("gauges/{kind}_empty")), m.art.sprite(&format!("gauges/{kind}_full")))
            {
                (None, Some(empty), Some(full)) => paint::gauge(painter, &empty, &full, r, *value),
                _ => {
                    painter.rect_filled(r, CornerRadius::same(2), Color32::from_black_alpha(140));
                    if *value > 0.0 {
                        let fill = Rect::from_min_size(
                            r.min + vec2(1.0, 1.0) / ppp,
                            vec2((r.width() - 2.0 / ppp).max(0.0) * value, (r.height() - 2.0 / ppp).max(0.0)),
                        );
                        painter.rect_filled(fill, CornerRadius::same(2), colour.map_or_else(|| flat(kind), rgba));
                    }
                }
            }
            r
        }
        Kind::Icon { icon, size } => {
            let r = Rect::from_min_size(at, vec2(pt(*size), pt(*size)));
            if let Some((t, uv)) = super::modwin::icon_tex(m, ctx, Some(gu), Some(w), *icon) {
                painter.image(t, r, uv, Color32::WHITE);
            }
            r
        }
        Kind::Image { key, img, w: iw, h: ih, tint } => {
            let r = Rect::from_min_size(at, vec2(pt(*iw), pt(*ih)));
            let t = m.texture(ctx, key, || {
                Some(egui::ColorImage::from_rgba_unmultiplied([img.0 as usize, img.1 as usize], &img.2))
            });
            if let Some(t) = t {
                painter.image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), rgba(*tint));
            }
            r
        }
        Kind::Portrait { entity, w: iw, h: ih, tint, .. } => {
            let r = Rect::from_min_size(at, vec2(pt(*iw), pt(*ih)));
            if let Some((_, t)) = w.lua.portrait(*entity) {
                painter.image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), rgba(*tint));
            }
            r
        }
        Kind::Bubble { w: bw, h: bh, round, colour, edge, edge_w, tail } => {
            // Built in window pixels from its place, as the game UI's.
            let (px, py) = (at.x * ppp, at.y * ppp);
            let mut mesh = egui::Mesh::default();
            for tri in super::super::lua::hud::bubble_tris(
                [px, py, *bw, *bh],
                *round,
                *tail,
                *colour,
                edge.map(|e| (e, *edge_w)),
            ) {
                let base = mesh.vertices.len() as u32;
                for (p, c) in tri {
                    let c = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    mesh.colored_vertex(
                        pos2(pt(p[0]), pt(p[1])),
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                    );
                }
                mesh.add_triangle(base, base + 1, base + 2);
            }
            painter.add(egui::Shape::mesh(mesh));
            Rect::from_min_size(at, vec2(pt(*bw), pt(*bh)))
        }
        Kind::Map { w: mw, h: mh, centre, span, alpha, me } => {
            let r = Rect::from_min_size(at, vec2(pt(*mw), pt(*mh)));
            let img = w.lua.hud_map_image(w, gu.map_regions())?;
            let tex = super::extras::map_texture(m, ctx, &img);
            // As the built-in overlay drew it: points per map pixel, the image placed by the map pixel at the centre.
            let s = super::super::minimap::px_per_unit(&img.region);
            let (cx, cz) = centre.unwrap_or((w.pos.x, w.pos.z));
            let z = r.width() / span.max(1.0);
            let (mx, my) = (cx * s, -cz * s);
            let clipped = painter.with_clip_rect(r);
            let img_r = Rect::from_min_size(
                r.center() + vec2((img.region.ox as f32 - mx) * z, (img.region.oy as f32 - my) * z),
                vec2(512.0 * z, 512.0 * z),
            );
            let alpha = alpha.unwrap_or(w.map_opacity);
            clipped.image(
                tex,
                img_r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::from_white_alpha((255.0 * alpha.clamp(0.0, 1.0)) as u8),
            );
            if *me && img.marker {
                let facing = w.net.me.lock().map_or(0.0, |m| super::super::event::rad_of(m.dir));
                let p = r.center() + vec2((w.pos.x * s - mx) * z, (-w.pos.z * s - my) * z);
                super::extras::arrow_in(&clipped, p, facing, 11.0, super::extras::ME);
            }
            r
        }
    })
}
