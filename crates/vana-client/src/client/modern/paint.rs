//! Drawing over egui's painter: nine-slice panels and plain sprites from the atlases, and the gauges.

use super::theme::Sprite;
use egui::{Color32, CornerRadius, Mesh, Painter, Rect, Shape, Stroke, StrokeKind, pos2};

/// A nine-slice sprite stretched over `dst`; the margins keep their size at `k` points per source pixel (less when
/// `dst` is too small for them).
pub fn nine_slice(s: &Sprite, dst: Rect, k: f32, tint: Color32) -> Shape {
    let [l, t, r, b] = s.margins.unwrap_or([0.0; 4]);
    let k = k.min(dst.width() / (l + r).max(1.0)).min(dst.height() / (t + b).max(1.0));
    let xs = [dst.min.x, dst.min.x + l * k, dst.max.x - r * k, dst.max.x];
    let ys = [dst.min.y, dst.min.y + t * k, dst.max.y - b * k, dst.max.y];
    let (uv, at) = (s.uv, s.atlas);
    let us = [uv.min.x, uv.min.x + l / at.x, uv.max.x - r / at.x, uv.max.x];
    let vs = [uv.min.y, uv.min.y + t / at.y, uv.max.y - b / at.y, uv.max.y];
    let mut m = Mesh::with_texture(s.tex);
    for j in 0..3 {
        for i in 0..3 {
            let rect = Rect::from_min_max(pos2(xs[i], ys[j]), pos2(xs[i + 1], ys[j + 1]));
            if rect.width() > 0.0 && rect.height() > 0.0 {
                m.add_rect_with_uv(rect, Rect::from_min_max(pos2(us[i], vs[j]), pos2(us[i + 1], vs[j + 1])), tint);
            }
        }
    }
    Shape::mesh(m)
}

/// A gauge as the pack's gauge art draws it (sprites/svg/gauge_*.svg), with the fill cut to `f`: a dark rounded
/// track with a grey edge, the fill inset 3 px with a light line along its top. The pack bakes a fixed fill into the
/// gauge sprites, so they are drawn here.
pub fn gauge(p: &Painter, r: Rect, f: f32, fill: Color32) {
    let h = r.height();
    p.rect(r, CornerRadius::same((h / 2.0).min(7.0) as u8), Color32::from_rgb(0x10, 0x18, 0x1e), Stroke::new(1.0, Color32::from_rgb(0x5f, 0x6c, 0x70)), StrokeKind::Inside);
    let inset = (h * 3.0 / 18.0).max(2.0);
    let inner = r.shrink(inset);
    let w = inner.width() * f.clamp(0.0, 1.0);
    if w >= 1.0 {
        let fr = Rect::from_min_size(inner.min, egui::vec2(w, inner.height()));
        p.rect_filled(fr, CornerRadius::same((inner.height() / 2.0).min(4.0) as u8), fill);
        if w > 6.0 {
            let y = fr.min.y + fr.height() * 0.25;
            p.line_segment([pos2(fr.min.x + 3.0, y), pos2(fr.max.x - 3.0, y)], Stroke::new(1.0, Color32::from_white_alpha(70)));
        }
    }
}
