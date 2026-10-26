//! Drawing over egui's painter: windows of any size from the pack's frames, stretched nine-slices (rows, buttons,
//! bars), plain sprites, the gauges and the bars' outlined labels.

use super::theme::{Frame, Sprite};
use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, pos2, vec2};

/// Text with a black border a pixel wide (the bars' labels), placed by `align` at `at`; its rect.
pub fn outlined(p: &Painter, at: Pos2, align: Align2, text: &str, font: FontId, colour: Color32) -> Rect {
    let galley = p.layout_no_wrap(text.to_string(), font, colour);
    let r = align.anchor_size(at, galley.size());
    let px = 1.0 / p.pixels_per_point();
    for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
        p.galley_with_override_text_color(r.min + vec2(dx, dy) * px, galley.clone(), Color32::BLACK);
    }
    p.galley(r.min, galley, colour);
    r
}

fn quad(m: &mut Mesh, r: Rect, uv: Rect) {
    if r.width() > 0.0 && r.height() > 0.0 {
        m.add_rect_with_uv(r, uv, Color32::WHITE);
    }
}

/// A window over `r` at `k` points per logical pixel (NINE_SLICE.md): the interior's gradient stretched top to
/// bottom, then the border: fixed corners, edges tiled from the frame's middle strips (their pattern repeats, so they
/// are never stretched), the last tile cut short.
pub fn window(f: &Frame, r: Rect, k: f32) -> Shape {
    let k = k.min(r.width() / (2.0 * f.corner)).min(r.height() / (2.0 * f.corner));
    let inner = r.shrink(f.inset * k);
    let mut g = Mesh::default();
    let bands = 16;
    for i in 0..=bands {
        let t = i as f32 / bands as f32;
        let c = f.gradient[((f.gradient.len() - 1) as f32 * t).round() as usize];
        let y = inner.top() + inner.height() * t;
        g.colored_vertex(pos2(inner.left(), y), c);
        g.colored_vertex(pos2(inner.right(), y), c);
    }
    for i in 0..bands as u32 {
        let a = 2 * i;
        g.add_triangle(a, a + 1, a + 2);
        g.add_triangle(a + 1, a + 3, a + 2);
    }
    let mut b = Mesh::with_texture(f.border.id());
    let (c, s) = (f.corner * k, (f.size - 2.0 * f.corner) * k);
    let (u0, u1) = (f.corner / f.size, 1.0 - f.corner / f.size);
    let uv = |a: f32, b: f32, c: f32, d: f32| Rect::from_min_max(pos2(a, b), pos2(c, d));
    // Corners.
    quad(&mut b, Rect::from_min_size(r.min, vec2(c, c)), uv(0.0, 0.0, u0, u0));
    quad(&mut b, Rect::from_min_size(pos2(r.max.x - c, r.min.y), vec2(c, c)), uv(u1, 0.0, 1.0, u0));
    quad(&mut b, Rect::from_min_size(pos2(r.min.x, r.max.y - c), vec2(c, c)), uv(0.0, u1, u0, 1.0));
    quad(&mut b, Rect::from_min_size(pos2(r.max.x - c, r.max.y - c), vec2(c, c)), uv(u1, u1, 1.0, 1.0));
    // Top and bottom edges, tiled across.
    let mut x = r.min.x + c;
    while x < r.max.x - c - 0.01 {
        let w = s.min(r.max.x - c - x);
        let ue = u0 + (u1 - u0) * w / s;
        quad(&mut b, Rect::from_min_size(pos2(x, r.min.y), vec2(w, c)), uv(u0, 0.0, ue, u0));
        quad(&mut b, Rect::from_min_size(pos2(x, r.max.y - c), vec2(w, c)), uv(u0, u1, ue, 1.0));
        x += w;
    }
    // Left and right edges, tiled down.
    let mut y = r.min.y + c;
    while y < r.max.y - c - 0.01 {
        let h = s.min(r.max.y - c - y);
        let ve = u0 + (u1 - u0) * h / s;
        quad(&mut b, Rect::from_min_size(pos2(r.min.x, y), vec2(c, h)), uv(0.0, u0, u0, ve));
        quad(&mut b, Rect::from_min_size(pos2(r.max.x - c, y), vec2(c, h)), uv(u1, u0, 1.0, ve));
        y += h;
    }
    Shape::Vec(vec![Shape::mesh(g), Shape::mesh(b)])
}

/// A nine-slice sprite stretched over `dst`; its insets keep `k` points per logical pixel (less when `dst` is too
/// small for them).
pub fn nine_slice(s: &Sprite, dst: Rect, k: f32, tint: Color32) -> Shape {
    let [l, t, r, b] = s.margins.unwrap_or([0.0; 4]);
    let k = k.min(dst.width() / (l + r).max(1.0)).min(dst.height() / (t + b).max(1.0));
    let xs = [dst.min.x, dst.min.x + l * k, dst.max.x - r * k, dst.max.x];
    let ys = [dst.min.y, dst.min.y + t * k, dst.max.y - b * k, dst.max.y];
    let (uv, at, px) = (s.uv, s.atlas, s.px);
    let us = [uv.min.x, uv.min.x + l * px / at.x, uv.max.x - r * px / at.x, uv.max.x];
    let vs = [uv.min.y, uv.min.y + t * px / at.y, uv.max.y - b * px / at.y, uv.max.y];
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

/// A sprite whole, `size` points wide at `at`.
pub fn image(s: &Sprite, at: Pos2, size: f32, tint: Color32) -> Shape {
    let mut m = Mesh::with_texture(s.tex);
    m.add_rect_with_uv(Rect::from_min_size(at, vec2(size, size * s.size.y / s.size.x)), s.uv, tint);
    Shape::mesh(m)
}

/// A gauge: the empty bar, then the full one cut at `f` of the way across (the pack's *_empty / *_full pairs).
pub fn gauge(p: &Painter, empty: &Sprite, full: &Sprite, r: Rect, f: f32) {
    let k = r.height() / empty.size.y;
    p.add(nine_slice(empty, r, k, Color32::WHITE));
    let f = f.clamp(0.0, 1.0);
    if f > 0.0 {
        let clip = Rect::from_min_max(r.min, pos2(r.min.x + r.width() * f, r.max.y));
        p.with_clip_rect(clip.intersect(p.clip_rect())).add(nine_slice(full, r, k, Color32::WHITE));
    }
}
