//! The game's text in egui, its private-use characters as the modern faces can show them: the element gaiji (SJIS
//! 0xEF1F..0xEF26, kept as U+E000..U+E007 by decode_sjis) as the pack's element icons, the ones the Equipment window's
//! resistances use; the phrase braces and the ON / OFF words (0xEF27..0xEF2E) as text; the rest as font::plain_char.

use super::{Modern, paint};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Context, FontId, Galley, Painter, Pos2, Sense, Ui, pos2};

/// The element icons in the gaiji's order, also s2c 0x061's resistances (fire .. dark).
pub const ELEMENTS: [&str; 8] = ["icons/fire", "icons/ice", "icons/wind", "icons/earth", "icons/lightning", "icons/water", "icons/light", "icons/dark"];

/// An icon's side against the font size.
const ICON: f32 = 1.15;

/// Gaiji 0xEF27.. drawn as words: the phrase braces (green and red, as the chat's) and the ON / OFF pairs.
const WORDS: [(&str, Option<Color32>); 8] = [
    ("{", Some(Color32::from_rgb(0x50, 0xd0, 0x50))),
    ("}", Some(Color32::from_rgb(0xe0, 0x50, 0x50))),
    ("ON", None),
    ("OFF", None),
    ("Oui", None),
    ("Non", None),
    ("EIN", None),
    ("AUS", None),
];

/// The element a character stands for.
fn element(c: char) -> Option<usize> {
    ('\u{E000}'..='\u{E007}').contains(&c).then(|| c as usize - 0xE000)
}

/// Appends `text` to `job` in `font` and `color`: each element gaiji an invisible glyph as wide as its icon, for
/// icons() to draw over once laid out.
pub fn append(ctx: &Context, job: &mut LayoutJob, text: &str, font: &FontId, color: Color32) {
    let mut run = String::new();
    let flush = |job: &mut LayoutJob, run: &mut String| {
        if !run.is_empty() {
            job.append(run, 0.0, TextFormat::simple(font.clone(), color));
            run.clear();
        }
    };
    for c in text.chars() {
        if element(c).is_some() {
            flush(job, &mut run);
            let s = c.to_string();
            // The placeholder's own advance makes the room (letter spacing goes before a glyph, and is lost at a
            // paragraph's or row's start): its font scaled to the icon's width, the row kept the text's height.
            let (natural, row) = ctx.fonts_mut(|f| (f.layout_no_wrap(s.clone(), font.clone(), color).size().x, f.row_height(font)));
            let size = if natural > 0.0 { font.size * font.size * ICON / natural } else { font.size };
            job.append(&s, 0.0, TextFormat { line_height: Some(row), ..TextFormat::simple(FontId::new(size, font.family.clone()), Color32::TRANSPARENT) });
        } else if let Some((word, tint)) = ('\u{E008}'..='\u{E00F}').contains(&c).then(|| WORDS[c as usize - 0xE008]) {
            match tint {
                Some(t) => {
                    flush(job, &mut run);
                    job.append(word, 0.0, TextFormat::simple(font.clone(), t));
                }
                None => run.push_str(word),
            }
        } else {
            run.extend(crate::dat::font::plain_char(c));
        }
    }
    flush(job, &mut run);
}

/// The element icons over `galley` drawn at `at`.
pub fn icons(painter: &Painter, m: &Modern, at: Pos2, galley: &Galley) {
    for row in &galley.rows {
        let r = row.rect().translate(at.to_vec2());
        for g in &row.row.glyphs {
            let Some(s) = element(g.chr).and_then(|k| m.art.sprite(ELEMENTS[k])) else { continue };
            let side = g.advance_width.min(r.height());
            painter.add(paint::image(&s, pos2(r.min.x + g.pos.x + (g.advance_width - side) / 2.0, r.center().y - side / 2.0), side, Color32::WHITE));
        }
    }
}

/// A label of game text wrapped to the room left, its element gaiji as icons.
pub fn label(ui: &mut Ui, m: &Modern, text: &str, size: f32, color: Color32) {
    let mut job = LayoutJob::default();
    append(ui.ctx(), &mut job, text, &FontId::proportional(size), color);
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (r, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(r.min, galley.clone(), color);
    icons(ui.painter(), m, r.min, &galley);
}
