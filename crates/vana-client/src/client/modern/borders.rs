//! Window borders: the pack's framed windows and thinner styles offered beside them (modern-shot --borders).

use super::{Modern, paint};
use egui::{Color32, CornerRadius, Mesh, Rect, Shape, Stroke, StrokeKind, epaint::RectShape, pos2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// The pack's frames: a wide bronze and gold border with ornamented corners.
    #[default]
    Pack,
    /// A single thin line with a faint inner highlight, gently rounded.
    Hairline,
    /// Gold rules along the top and bottom that fade toward the corners (as the Gilt rows), faint sides.
    Gilt,
    /// A faint thin outline with short gold marks at the corners.
    Corners,
    /// No drawn line: a soft shadow under the window and a light top edge.
    Shadow,
}

impl Style {
    pub const ALL: [Style; 5] = [Style::Pack, Style::Hairline, Style::Gilt, Style::Corners, Style::Shadow];

    pub fn name(self) -> &'static str {
        match self {
            Style::Pack => "pack",
            Style::Hairline => "hairline",
            Style::Gilt => "gilt",
            Style::Corners => "corners",
            Style::Shadow => "shadow",
        }
    }

    pub fn from_name(s: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// A frame's line colour by its name (the focused window blue, an alert red, an inactive one dimmed).
fn line(m: &Modern, frame: &str) -> Color32 {
    match frame {
        "window_focused" => m.theme.color("focus"),
        "window_alert" => m.theme.color("hp"),
        "window_inactive" => m.theme.color("bronze").gamma_multiply(0.45),
        _ => m.theme.color("bronze"),
    }
}

/// The accent drawn on the line (the Gilt rules, the corner marks): gold, or the frame's own colour when it has one.
fn accent(m: &Modern, frame: &str) -> Color32 {
    match frame {
        "window_focused" | "window_alert" => line(m, frame),
        "window_inactive" => m.theme.color("gold").gamma_multiply(0.4),
        _ => m.theme.color("gold"),
    }
}

/// The pack frame's interior colour (its gradient's middle), for the styles that fill a rounded rect.
fn fill(m: &Modern, frame: &str) -> Color32 {
    m.bg(m.art.frame(frame).or_else(|| m.art.frame("window_normal")).map_or(m.theme.color("surface"), |f| f.gradient[f.gradient.len() / 2]))
}

/// A horizontal line from `a` to `b` at `y`, clear at both ends and `c` over its middle half.
fn rule(v: &mut Vec<Shape>, a: f32, b: f32, y: f32, c: Color32) {
    let mut m = Mesh::default();
    let w = b - a;
    let xs = [a, a + w * 0.25, b - w * 0.25, b];
    let cs = [Color32::TRANSPARENT, c, c, Color32::TRANSPARENT];
    for (x, col) in xs.iter().zip(cs) {
        m.colored_vertex(pos2(*x, y - 0.5), col);
        m.colored_vertex(pos2(*x, y + 0.5), col);
    }
    for i in 0..3u32 {
        let k = 2 * i;
        m.add_triangle(k, k + 1, k + 2);
        m.add_triangle(k + 1, k + 3, k + 2);
    }
    v.push(Shape::mesh(m));
}

/// Short L marks at `r`'s four corners.
fn corner_marks(v: &mut Vec<Shape>, r: Rect, c: Color32) {
    let (l, s) = (11.0, Stroke::new(1.5, c));
    let r = r.shrink(0.75);
    for (p, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        v.push(Shape::line_segment([p, p + vec2(l * dx, 0.0)], s));
        v.push(Shape::line_segment([p, p + vec2(0.0, l * dy)], s));
    }
}

/// The border and interior in `style` over `body` (`round` its corners), `open` the rect it is drawn on when one side
/// runs past the screen (the edge windows), else `body` itself.
fn shapes(m: &Modern, frame: &str, body: Rect, round: CornerRadius, style: Style) -> Vec<Shape> {
    let mut v = Vec::new();
    match style {
        Style::Pack => {}
        Style::Hairline => {
            v.push(Shape::Rect(RectShape::filled(body, round, fill(m, frame))));
            v.push(Shape::Rect(RectShape::stroke(body.shrink(1.0), round, Stroke::new(1.0, Color32::from_white_alpha(12)), StrokeKind::Inside)));
            v.push(Shape::Rect(RectShape::stroke(body, round, Stroke::new(1.0, line(m, frame).gamma_multiply(0.85)), StrokeKind::Inside)));
        }
        Style::Gilt => {
            v.push(Shape::Rect(RectShape::filled(body, round, fill(m, frame))));
            v.push(Shape::Rect(RectShape::stroke(body, round, Stroke::new(1.0, line(m, frame).gamma_multiply(0.35)), StrokeKind::Inside)));
            let c = accent(m, frame).gamma_multiply(0.9);
            rule(&mut v, body.min.x, body.max.x, body.min.y + 0.5, c);
            rule(&mut v, body.min.x, body.max.x, body.max.y - 0.5, c);
        }
        Style::Corners => {
            v.push(Shape::Rect(RectShape::filled(body, round, fill(m, frame))));
            v.push(Shape::Rect(RectShape::stroke(body, round, Stroke::new(1.0, line(m, frame).gamma_multiply(0.45)), StrokeKind::Inside)));
            corner_marks(&mut v, body, accent(m, frame));
        }
        Style::Shadow => {
            let shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(150) };
            v.push(Shape::Rect(shadow.as_shape(body, round)));
            v.push(Shape::Rect(RectShape::filled(body, round, fill(m, frame))));
            let edge = if frame == "window_normal" || frame == "window_tooltip" { Color32::from_black_alpha(120) } else { line(m, frame) };
            v.push(Shape::Rect(RectShape::stroke(body, round, Stroke::new(1.0, edge), StrokeKind::Inside)));
            v.push(Shape::line_segment([pos2(body.min.x + 6.0, body.min.y + 1.0), pos2(body.max.x - 6.0, body.min.y + 1.0)], Stroke::new(1.0, Color32::from_white_alpha(26))));
        }
    }
    v
}

/// A window's frame over `r` in the current style (Modern::border_style); `frame` is the pack's name for it
/// ("window_normal", "window_focused" ...).
pub fn window(m: &Modern, frame: &str, r: Rect) -> Shape {
    // A frameless panel ("none": the status icons) stays bare in every style.
    let Some(f) = m.art.frame(frame) else { return Shape::Noop };
    let style = m.border_style.get();
    if style == Style::Pack {
        return paint::window(f, r, 1.0, m.opacity.get());
    }
    Shape::Vec(shapes(m, frame, r, CornerRadius::same(6), style))
}

/// An edge window's frame in the current style: `ext` runs past the screen's edge on the anchored side, whose corners
/// `round` leaves square. None in the pack style (the edge windows draw their own there).
pub fn edge(m: &Modern, ext: Rect, round: CornerRadius) -> Option<Shape> {
    let style = m.border_style.get();
    (style != Style::Pack).then(|| Shape::Vec(shapes(m, "window_normal", ext, round, style)))
}
