//! Tabs (ours): a strip of tabs on a baseline. The open tab stands taller in the panel's colour with a gold edge and
//! covers the baseline, opening into what is below it; the others sit back, darker and shorter. Drawn from the theme:
//! the pack's tab and button sprites read as buttons.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// A strip row's height; inactive tabs start this much lower.
pub const H: f32 = 26.0;
const DROP: f32 = 4.0;
const GAP: f32 = 3.0;
const PAD: f32 = 12.0;

/// One tab's state.
#[derive(Clone, Copy, Default)]
pub struct State {
    pub open: bool,
    pub hover: bool,
    /// The keyboard cursor is on it (a list's tab row with the focus).
    pub cursor: bool,
}

/// The tab width for a label in `font`, at least `min`.
pub fn width(ui: &Ui, label: &str, font: &FontId, min: f32) -> f32 {
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x);
    (tw + 2.0 * PAD).max(min)
}

/// The baseline under a row of tabs (drawn before them, so the open tab covers it).
pub fn baseline(p: &Painter, m: &super::Modern, row: Rect, alpha: f32) {
    let y = row.max.y - 0.5;
    p.line_segment([pos2(row.min.x, y), pos2(row.max.x, y)], Stroke::new(1.0, m.theme.color("bronze").gamma_multiply(0.6 * alpha)));
}

/// A tab whose column is `r` (the row's full height): its face, edge and label.
pub fn paint(p: &Painter, m: &super::Modern, r: Rect, label: &str, font: &FontId, s: State, alpha: f32) {
    let radius = CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 };
    let (face, edge, text) = if s.open {
        (m.theme.color("surface"), m.theme.color("gold").gamma_multiply(0.9), m.theme.color("gold"))
    } else if s.hover {
        (m.theme.color("surface").gamma_multiply(0.85), m.theme.color("bronze").gamma_multiply(0.8), m.theme.color("text"))
    } else {
        (m.theme.color("charcoal").gamma_multiply(0.9), m.theme.color("bronze").gamma_multiply(0.5), m.theme.color("muted"))
    };
    let t = if s.open { r } else { Rect::from_min_max(pos2(r.min.x, r.min.y + DROP), r.max) };
    p.rect(t, radius, face.gamma_multiply(alpha), Stroke::new(1.0, edge.gamma_multiply(alpha)), StrokeKind::Inside);
    if s.open {
        // Open into the content: the bottom edge and the baseline under it painted over in the face.
        p.line_segment([pos2(t.min.x + 1.0, t.max.y - 0.5), pos2(t.max.x - 1.0, t.max.y - 0.5)], Stroke::new(2.0, face.gamma_multiply(alpha)));
    }
    let c = t.center() + vec2(0.0, if s.open { 1.0 } else { 0.5 });
    p.text(c, Align2::CENTER_CENTER, label, font.clone(), text.gamma_multiply(alpha));
    if s.cursor {
        let w = t.width() - 2.0 * PAD + 6.0;
        let y = t.max.y - 4.0;
        p.line_segment([pos2(c.x - w / 2.0, y), pos2(c.x + w / 2.0, y)], Stroke::new(2.0, m.theme.color("focus").gamma_multiply(alpha)));
    }
}

/// A strip of `labels` across the available width, wrapping into more rows when it must: `open` the open tab,
/// `cursor` the keyboard cursor's; each tab's response in order (hovered / clicked).
pub fn strip(ui: &mut Ui, m: &super::Modern, salt: &str, labels: &[&str], open: Option<usize>, cursor: Option<usize>) -> Vec<Response> {
    let font = FontId::proportional(m.theme.small);
    let avail = ui.available_width();
    let widths: Vec<f32> = labels.iter().map(|l| width(ui, l, &font, 56.0)).collect();
    // Rows of tab indices, each as wide as fits.
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut x = 0.0;
    for (k, w) in widths.iter().enumerate() {
        if x > 0.0 && x + w > avail {
            rows.push(Vec::new());
            x = 0.0;
        }
        rows.last_mut().unwrap().push(k);
        x += w + GAP;
    }
    let mut out: Vec<Option<Response>> = (0..labels.len()).map(|_| None).collect();
    for row_tabs in rows {
        let (row, _) = ui.allocate_exact_size(vec2(avail, H), Sense::hover());
        let mut rects = Vec::new();
        let mut x = row.min.x;
        for &k in &row_tabs {
            let r = Rect::from_min_size(pos2(x, row.min.y), vec2(widths[k], H));
            x = r.max.x + GAP;
            let resp = ui.interact(r, ui.scope_id().with((salt, k)), Sense::click());
            rects.push((k, r, resp.hovered()));
            out[k] = Some(resp);
        }
        let p = ui.painter();
        baseline(p, m, row, 1.0);
        // The open tab last, over its neighbours' edges and the baseline.
        rects.sort_by_key(|&(k, ..)| open == Some(k));
        for (k, r, hover) in rects {
            paint(p, m, r, labels[k], &font, State { open: open == Some(k), hover, cursor: cursor == Some(k) }, 1.0);
        }
    }
    out.into_iter().flatten().collect()
}
