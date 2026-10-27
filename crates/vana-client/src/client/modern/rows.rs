//! Menu row looks: the pack's framed rows, and lighter styles offered beside them (modern-shot --rows).

use super::{Modern, paint};
use egui::{Align2, Color32, FontFamily, FontId, Mesh, Painter, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// The pack's row art: framed rows, a diamond bullet, the boxed arrow.
    #[default]
    Framed,
    /// No frames: hairlines between rows, the cursor a blue band fading right with a gold edge.
    Hairline,
    /// Rounded rows with a faint border, the cursor filled blue with a gold border.
    Soft,
    /// Bare rows, the cursor between two fading gold rules with a gold diamond.
    Gilt,
    /// Raised rows (a light top, a dark bottom), the cursor blue with a soft glow.
    Bevel,
}

impl Style {
    pub const ALL: [Style; 5] = [Style::Framed, Style::Hairline, Style::Soft, Style::Gilt, Style::Bevel];

    pub fn name(self) -> &'static str {
        match self {
            Style::Framed => "framed",
            Style::Hairline => "hairline",
            Style::Soft => "soft",
            Style::Gilt => "gilt",
            Style::Bevel => "bevel",
        }
    }

    pub fn from_name(s: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// A row's state: the cursor's (or its open category), hovered, disabled.
#[derive(Clone, Copy, Default)]
pub struct State {
    pub on: bool,
    pub hover: bool,
    pub off: bool,
}

/// A left-to-right (`across`) or top-to-bottom gradient over `r`.
fn gradient(r: Rect, a: Color32, b: Color32, across: bool) -> Shape {
    let mut m = Mesh::default();
    let (tr, bl) = if across { (b, a) } else { (a, b) };
    m.colored_vertex(r.left_top(), a);
    m.colored_vertex(r.right_top(), tr);
    m.colored_vertex(r.right_bottom(), b);
    m.colored_vertex(r.left_bottom(), bl);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    Shape::mesh(m)
}

/// A line across `r`'s width at `y`, clear at both ends and `c` in the middle.
fn rule(p: &Painter, r: Rect, y: f32, c: Color32) {
    let mid = r.center().x;
    p.add(gradient(Rect::from_min_max(pos2(r.min.x, y), pos2(mid, y + 1.0)), Color32::TRANSPARENT, c, true));
    p.add(gradient(Rect::from_min_max(pos2(mid, y), pos2(r.max.x, y + 1.0)), c, Color32::TRANSPARENT, true));
}

/// The row's background.
fn back(p: &Painter, m: &Modern, r: Rect, s: State, style: Style) {
    let (gold, focus, bronze) = (m.theme.color("gold"), m.theme.color("focus"), m.theme.color("bronze"));
    match style {
        Style::Framed => {
            let sprite = match () {
                _ if s.off => "controls/list_row_disabled",
                _ if s.on => "controls/list_row_selected",
                _ if s.hover => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            if let Some(sp) = m.art.sprite(sprite) {
                p.add(paint::nine_slice(&sp, r, r.height() / sp.size.y, Color32::WHITE));
            }
        }
        Style::Hairline => {
            if s.on {
                p.add(gradient(r, focus.gamma_multiply(0.55), focus.gamma_multiply(0.0), true));
                p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.min.y + 3.0), pos2(r.min.x + 3.0, r.max.y - 3.0)), 1.5, gold);
            } else if s.hover {
                p.rect_filled(r, 0.0, Color32::from_white_alpha(14));
            }
            p.line_segment([pos2(r.min.x + 10.0, r.max.y + 1.0), pos2(r.max.x - 10.0, r.max.y + 1.0)], Stroke::new(1.0, bronze.gamma_multiply(0.35)));
        }
        Style::Soft => {
            let r = r.shrink2(vec2(2.0, 1.0));
            let (fill, edge) = match () {
                _ if s.off => (Color32::from_black_alpha(40), bronze.gamma_multiply(0.12)),
                _ if s.on => (focus.gamma_multiply(0.5), gold.gamma_multiply(0.75)),
                _ if s.hover => (Color32::from_white_alpha(18), bronze.gamma_multiply(0.7)),
                _ => (Color32::from_black_alpha(64), bronze.gamma_multiply(0.25)),
            };
            p.rect(r, 6.0, fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        }
        Style::Gilt => {
            if s.on {
                let mid = r.center().x;
                p.add(gradient(Rect::from_min_max(r.min, pos2(mid, r.max.y)), gold.gamma_multiply(0.02), gold.gamma_multiply(0.13), true));
                p.add(gradient(Rect::from_min_max(pos2(mid, r.min.y), r.max), gold.gamma_multiply(0.13), gold.gamma_multiply(0.02), true));
                rule(p, r, r.min.y, gold.gamma_multiply(0.9));
                rule(p, r, r.max.y - 1.0, gold.gamma_multiply(0.9));
                let c = pos2(r.min.x + 9.0, r.center().y);
                p.add(Shape::convex_polygon(vec![pos2(c.x, c.y - 4.5), pos2(c.x + 4.5, c.y), pos2(c.x, c.y + 4.5), pos2(c.x - 4.5, c.y)], gold, Stroke::NONE));
            } else if s.hover {
                p.rect_filled(r, 0.0, gold.gamma_multiply(0.06));
            }
        }
        Style::Bevel => {
            let (top, bottom) = match () {
                _ if s.on => (focus.gamma_multiply(0.8), focus.gamma_multiply(0.35)),
                _ if s.hover => (Color32::from_rgb(0x34, 0x3e, 0x4a), Color32::from_rgb(0x1a, 0x20, 0x27)),
                _ if s.off => (Color32::from_rgb(0x1d, 0x22, 0x28), Color32::from_rgb(0x14, 0x17, 0x1b)),
                _ => (Color32::from_rgb(0x2a, 0x32, 0x3c), Color32::from_rgb(0x14, 0x18, 0x1e)),
            };
            let r = r.shrink2(vec2(1.0, 0.0));
            p.add(gradient(r, top, bottom, false));
            p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, Color32::from_white_alpha(22)));
            p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(110)));
            if s.on {
                p.rect_stroke(r.expand(1.0), 3.0, Stroke::new(1.0, focus.gamma_multiply(0.5)), egui::StrokeKind::Outside);
                p.rect_stroke(r.expand(2.5), 4.0, Stroke::new(2.0, focus.gamma_multiply(0.18)), egui::StrokeKind::Outside);
            }
        }
    }
}

/// The mark of a row that opens a submenu, at its right.
fn more(p: &Painter, m: &Modern, r: Rect, s: State, style: Style) {
    let c = if s.on && style != Style::Bevel { m.theme.color("gold") } else if s.on { Color32::WHITE } else { m.theme.color("muted") };
    let at = pos2(r.max.x - 16.0, r.center().y);
    match style {
        Style::Framed => {
            if let Some(sp) = m.art.sprite("controls/arrow_right") {
                p.add(paint::image(&sp, pos2(r.max.x - 26.0, r.center().y - 8.0), 16.0, Color32::WHITE));
            }
        }
        Style::Bevel => {
            p.add(Shape::convex_polygon(vec![pos2(at.x - 3.0, at.y - 5.0), pos2(at.x + 3.0, at.y), pos2(at.x - 3.0, at.y + 5.0)], c, Stroke::NONE));
        }
        _ => {
            p.line_segment([pos2(at.x - 3.0, at.y - 5.0), pos2(at.x + 2.0, at.y)], Stroke::new(1.6, c));
            p.line_segment([pos2(at.x + 2.0, at.y), pos2(at.x - 3.0, at.y + 5.0)], Stroke::new(1.6, c));
        }
    }
}

/// The label's colour.
fn ink(m: &Modern, s: State, style: Style) -> Color32 {
    match () {
        _ if s.off => m.theme.color("muted"),
        _ if s.on && matches!(style, Style::Hairline | Style::Gilt) => m.theme.color("gold"),
        _ => m.theme.color("text"),
    }
}

/// A menu row in the current style (Modern::row_style): its icon, its label, the submenu mark when `sub`; hovered by
/// the pointer or `hover`. Its response.
pub fn menu_row(ui: &mut Ui, m: &Modern, icon: &str, label: &str, on: bool, off: bool, sub: bool, hover: bool) -> egui::Response {
    let style = m.row_style.get();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let s = State { on, off, hover: hover || resp.hovered() };
    let p = ui.painter();
    back(p, m, r, s, style);
    if let Some(sp) = m.art.sprite(icon) {
        p.add(paint::image(&sp, pos2(r.min.x + 20.0, r.center().y - 11.0), 22.0, if off { Color32::from_gray(110) } else { Color32::WHITE }));
    }
    p.text(pos2(r.min.x + 50.0, r.center().y), Align2::LEFT_CENTER, label, FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into())), ink(m, s, style));
    if sub {
        more(p, m, r, s, style);
    }
    ui.add_space(2.0);
    resp
}
