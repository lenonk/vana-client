//! Drawing over egui's painter: windows of any size from the pack's frames, stretched nine-slices (rows, buttons,
//! bars), plain sprites, the gauges and the bars' outlined labels.

use super::theme::{Frame, Sprite};
use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, pos2, vec2};

/// A shape at `a` of its alpha (the frames' fade).
pub fn faded(s: Shape, a: f32) -> Shape {
    match s {
        Shape::Vec(v) => Shape::Vec(v.into_iter().map(|s| faded(s, a)).collect()),
        Shape::Mesh(mut m) => {
            for v in &mut std::sync::Arc::make_mut(&mut m).vertices {
                v.color = v.color.gamma_multiply(a);
            }
            Shape::Mesh(m)
        }
        Shape::Rect(mut r) => {
            r.fill = r.fill.gamma_multiply(a);
            r.stroke.color = r.stroke.color.gamma_multiply(a);
            Shape::Rect(r)
        }
        Shape::LineSegment { points, mut stroke } => {
            stroke.color = stroke.color.gamma_multiply(a);
            Shape::LineSegment { points, stroke }
        }
        s => s,
    }
}

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

/// How a window's frame is cut where flyouts join it: `clip` the part drawn (a flyout's open side), `gaps` spans of its
/// left (false) or right (true) band left out (a flyout joins there), `grad` the y span its gradient runs over (a joined
/// pair shares the target's).
#[derive(Clone, Default, Debug)]
pub struct Cut {
    pub clip: Option<Rect>,
    pub gaps: Vec<(bool, f32, f32)>,
    pub grad: Option<(f32, f32)>,
}

/// `quad` cut to `clip`, its uv cut alike.
fn quad_cut(m: &mut Mesh, r: Rect, uv: Rect, clip: Rect) {
    let c = r.intersect(clip);
    if c.width() <= 0.0 || c.height() <= 0.0 {
        return;
    }
    let lerp = |x: f32, a0: f32, a1: f32, b0: f32, b1: f32| b0 + (x - a0) / (a1 - a0) * (b1 - b0);
    let u = (lerp(c.min.x, r.min.x, r.max.x, uv.min.x, uv.max.x), lerp(c.max.x, r.min.x, r.max.x, uv.min.x, uv.max.x));
    let v = (lerp(c.min.y, r.min.y, r.max.y, uv.min.y, uv.max.y), lerp(c.max.y, r.min.y, r.max.y, uv.min.y, uv.max.y));
    m.add_rect_with_uv(c, Rect::from_min_max(pos2(u.0, v.0), pos2(u.1, v.1)), Color32::WHITE);
}

/// `quad_cut` without the y spans `gaps`.
fn quad_gapped(m: &mut Mesh, r: Rect, uv: Rect, clip: Rect, gaps: &[(f32, f32)]) {
    let mut parts = vec![(r.min.y, r.max.y)];
    for &(g0, g1) in gaps {
        parts = parts.into_iter().flat_map(|(a, b)| [(a, b.min(g0)), (a.max(g1), b)]).filter(|(a, b)| b > a).collect();
    }
    let lerp = |y: f32| uv.min.y + (y - r.min.y) / r.height() * uv.height();
    for (a, b) in parts {
        let pr = Rect::from_min_max(pos2(r.min.x, a), pos2(r.max.x, b));
        quad_cut(m, pr, Rect::from_min_max(pos2(uv.min.x, lerp(a)), pos2(uv.max.x, lerp(b))), clip);
    }
}

/// The interior's colour at `y` with its gradient over the span `y0..y1`.
pub fn grad_at(f: &Frame, y: f32, (y0, y1): (f32, f32), opacity: f32) -> Color32 {
    let t = ((y - y0) / (y1 - y0).max(1.0)).clamp(0.0, 1.0) * (f.gradient.len() - 1) as f32;
    let (i, k) = (t.floor() as usize, t.fract());
    let (a, b) = (f.gradient[i], f.gradient[(i + 1).min(f.gradient.len() - 1)]);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b())).gamma_multiply(opacity)
}

/// A window over `r` at `k` points per logical pixel (NINE_SLICE.md): the interior's gradient stretched top to
/// bottom, then the border: fixed corners, edges tiled from the frame's middle strips (their pattern repeats, so they
/// are never stretched), the last tile cut short.
pub fn window(f: &Frame, r: Rect, k: f32, opacity: f32) -> Shape {
    window_cut(f, r, k, opacity, &Cut::default())
}

/// `window` cut for its flyouts (Cut).
pub fn window_cut(f: &Frame, r: Rect, k: f32, opacity: f32, cut: &Cut) -> Shape {
    let k = k.min(r.width() / (2.0 * f.corner)).min(r.height() / (2.0 * f.corner));
    let inner = r.shrink(f.inset * k);
    let clip = cut.clip.unwrap_or(Rect::EVERYTHING);
    let span = cut.grad.unwrap_or((inner.top(), inner.bottom()));
    let mut g = Mesh::default();
    let shown = inner.intersect(clip);
    if shown.width() > 0.0 && shown.height() > 0.0 {
        let bands = 16;
        for i in 0..=bands {
            let y = shown.top() + shown.height() * i as f32 / bands as f32;
            let c = grad_at(f, y, span, opacity);
            g.colored_vertex(pos2(shown.left(), y), c);
            g.colored_vertex(pos2(shown.right(), y), c);
        }
        for i in 0..bands as u32 {
            let a = 2 * i;
            g.add_triangle(a, a + 1, a + 2);
            g.add_triangle(a + 1, a + 3, a + 2);
        }
    }
    let mut b = Mesh::with_texture(f.border.id());
    let (c, s) = (f.corner * k, (f.size - 2.0 * f.corner) * k);
    let (u0, u1) = (f.corner / f.size, 1.0 - f.corner / f.size);
    let uv = |a: f32, b: f32, c: f32, d: f32| Rect::from_min_max(pos2(a, b), pos2(c, d));
    // Corners.
    quad_cut(&mut b, Rect::from_min_size(r.min, vec2(c, c)), uv(0.0, 0.0, u0, u0), clip);
    quad_cut(&mut b, Rect::from_min_size(pos2(r.max.x - c, r.min.y), vec2(c, c)), uv(u1, 0.0, 1.0, u0), clip);
    quad_cut(&mut b, Rect::from_min_size(pos2(r.min.x, r.max.y - c), vec2(c, c)), uv(0.0, u1, u0, 1.0), clip);
    quad_cut(&mut b, Rect::from_min_size(pos2(r.max.x - c, r.max.y - c), vec2(c, c)), uv(u1, u1, 1.0, 1.0), clip);
    // Top and bottom edges, tiled across.
    let mut x = r.min.x + c;
    while x < r.max.x - c - 0.01 {
        let w = s.min(r.max.x - c - x);
        let ue = u0 + (u1 - u0) * w / s;
        quad_cut(&mut b, Rect::from_min_size(pos2(x, r.min.y), vec2(w, c)), uv(u0, 0.0, ue, u0), clip);
        quad_cut(&mut b, Rect::from_min_size(pos2(x, r.max.y - c), vec2(w, c)), uv(u0, u1, ue, 1.0), clip);
        x += w;
    }
    // Left and right edges, tiled down, without the spans where flyouts join.
    let gaps = |right: bool| cut.gaps.iter().filter(|g| g.0 == right).map(|g| (g.1, g.2)).collect::<Vec<_>>();
    let (gl, gr) = (gaps(false), gaps(true));
    let mut y = r.min.y + c;
    while y < r.max.y - c - 0.01 {
        let h = s.min(r.max.y - c - y);
        let ve = u0 + (u1 - u0) * h / s;
        quad_gapped(&mut b, Rect::from_min_size(pos2(r.min.x, y), vec2(c, h)), uv(0.0, u0, u0, ve), clip, &gl);
        quad_gapped(&mut b, Rect::from_min_size(pos2(r.max.x - c, y), vec2(c, h)), uv(u1, u0, 1.0, ve), clip, &gr);
        y += h;
    }
    Shape::Vec(vec![Shape::mesh(g), Shape::mesh(b)])
}

/// The fillets' radius where a flyout joins a window, and the band's middle from a frame's outer edge.
pub const FILLET: f32 = 8.0;
const BAND_MID: f32 = 3.0;

/// The span of window `t`'s side band a flyout over `fr` leaves out.
pub fn joint_gap(fr: Rect) -> (f32, f32) {
    (fr.min.y + BAND_MID - FILLET, fr.max.y - BAND_MID + FILLET)
}

/// The part of a flyout's frame drawn where it joins `t` on t's right side (`right`) or left: from the fillets out.
pub fn joint_clip(t: Rect, right: bool) -> Rect {
    if right {
        Rect::from_min_max(pos2(t.max.x - BAND_MID + FILLET, f32::NEG_INFINITY), pos2(f32::INFINITY, f32::INFINITY))
    } else {
        Rect::from_min_max(pos2(f32::NEG_INFINITY, f32::NEG_INFINITY), pos2(t.min.x + BAND_MID - FILLET, f32::INFINITY))
    }
}

/// Where flyout `fr` joins window `t` on its right side (`right`) or left: the interior carried across between the
/// two (`span` the gradient's, t's), and the fillets where the flyout's top and bottom bands curve into t's side band,
/// drawn as the bands (dark, middle, bright), t's (`band`) turning into the flyout's along the arc.
pub fn joint(f: &Frame, band: [Color32; 3], t: Rect, fr: Rect, right: bool, opacity: f32, span: (f32, f32)) -> Shape {
    let (s, r) = (if right { 1.0 } else { -1.0 }, FILLET);
    // The side band's middle, the fillets' centres' x, the interior's edge in t.
    let x_band = if right { t.max.x - BAND_MID } else { t.min.x + BAND_MID };
    let cx = x_band + s * r;
    let x_in = if right { t.max.x - f.inset } else { t.min.x + f.inset };
    let (y0, y1) = (fr.min.y + BAND_MID, fr.max.y - BAND_MID);
    let (c0, c1) = (pos2(cx, y0 - r), pos2(cx, y1 + r));
    // The carried interior's top and bottom at x: on the arcs between the band and the centres, flat inside t.
    let reach = |x: f32| {
        let d = (x - cx).abs();
        if d >= r { 0.0 } else { (r * r - d * d).sqrt() }
    };
    let cols = 14;
    let rows = 16;
    let mut m = Mesh::default();
    for j in 0..=cols {
        let x = x_in + (cx - x_in) * j as f32 / cols as f32;
        let inside_t = if right { x <= x_band } else { x >= x_band };
        let (top, bot) = if inside_t { (c0.y, c1.y) } else { (c0.y + reach(x), c1.y - reach(x)) };
        for i in 0..=rows {
            let y = top + (bot - top) * i as f32 / rows as f32;
            m.colored_vertex(pos2(x, y), grad_at(f, y, span, opacity));
        }
    }
    let n = rows as u32 + 1;
    for j in 0..cols as u32 {
        for i in 0..rows as u32 {
            let (a, b) = (j * n + i, (j + 1) * n + i);
            m.add_triangle(a, b, a + 1);
            m.add_triangle(b, b + 1, a + 1);
        }
    }
    // The fillets: a quarter circle each, from t's side band (vertical) to the flyout's top / bottom band.
    let arc = |c: Pos2, down: bool| -> Vec<Pos2> {
        let steps = 16;
        (0..=steps)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
                // From the band (x_band, c.y) to the flyout's band (cx, c.y +- r).
                let (dx, dy) = (-s * r * a.cos(), r * a.sin());
                pos2(c.x + dx, if down { c.y + dy } else { c.y - dy })
            })
            .collect()
    };
    let mut v = vec![Shape::mesh(m)];
    for (c, pts) in [(c0, arc(c0, true)), (c1, arc(c1, false))] {
        for (w, i) in [(5.0, 0), (2.0, 1), (1.0, 2)] {
            let (from, to) = (band[i], f.band[i]);
            // The angle round the arc's centre: 0 at t's band, a quarter turn at the flyout's.
            let blend = move |_: Rect, p: Pos2| {
                let a = (p.y - c.y).abs().atan2(-s * (p.x - c.x)).clamp(0.0, std::f32::consts::FRAC_PI_2);
                from.lerp_to_gamma(to, a / std::f32::consts::FRAC_PI_2)
            };
            v.push(Shape::line(pts.clone(), egui::epaint::PathStroke::new_uv(w, blend)));
        }
    }
    Shape::Vec(v)
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
/// A five-pointed star at `c` (outer radius `r`) with a dark edge: a fan from its centre, as it is not convex.
pub fn star(c: Pos2, r: f32, fill: Color32) -> Shape {
    let pts: Vec<Pos2> = (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let k = if i % 2 == 0 { r } else { r * 0.45 };
            c + vec2(a.cos(), a.sin()) * k
        })
        .collect();
    let mut m = Mesh::default();
    m.colored_vertex(c, fill);
    for p in &pts {
        m.colored_vertex(*p, fill);
    }
    for i in 0..10u32 {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
    }
    let edge = Shape::closed_line(pts, egui::Stroke::new(1.0, Color32::from_black_alpha(200)));
    Shape::Vec(vec![Shape::mesh(m), edge])
}

/// A sprite over `r`, turned clockwise by `quarters` quarter turns (an arrow pointing up made to point right or down).
pub fn turned(s: &Sprite, r: Rect, quarters: u8, tint: Color32) -> Shape {
    let uv = s.uv;
    // The sprite's corners clockwise from its top left, rotated onto the rect's.
    let corners = [uv.left_top(), uv.right_top(), uv.right_bottom(), uv.left_bottom()];
    let at = |k: usize| corners[(k + 4 - (quarters % 4) as usize) % 4];
    let mut m = Mesh::with_texture(s.tex);
    for (k, p) in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().enumerate() {
        m.vertices.push(egui::epaint::Vertex { pos: p, uv: at(k), color: tint });
    }
    m.indices.extend([0, 1, 2, 0, 2, 3]);
    Shape::mesh(m)
}

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
