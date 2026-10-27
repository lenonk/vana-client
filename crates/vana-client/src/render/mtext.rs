//! The modern UI's text inside the game UI's own pass (while Modern is on): the bundled Fira Sans faces' glyphs, each
//! rasterized once per size into one atlas uploaded as the UI image "mtext" (again whenever it grows); a string is
//! its glyphs' quads. Sizes come from the layout-to-screen scale of the last frame, so glyphs are never minified much.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

pub const TEXTURE: &str = "mtext";
/// The game font's text in the modern face: its em and its line top against retail's pen (16-px cells).
pub const TEXT_EM: f32 = 14.0;
pub const TEXT_DY: f32 = -0.5;
const REGULAR: &[u8] = include_bytes!("../../assets/fonts/FiraSans-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/FiraSans-SemiBold.ttf");
/// Raster sizes (em, px): the smallest at or above the size on screen.
const BUCKETS: [u16; 10] = [10, 12, 14, 16, 20, 24, 28, 34, 42, 56];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Bold,
}

/// A glyph in the atlas: its rect there (px), its offset from the pen on the baseline (px), its advance.
#[derive(Clone, Copy)]
struct Glyph {
    at: [u16; 4],
    off: [f32; 2],
    adv: f32,
}

struct Atlas {
    rgba: Vec<u8>,
    side: usize,
    pen: (usize, usize, usize),
    glyphs: HashMap<(char, Face, u16), Option<Glyph>>,
    dirty: bool,
}

fn atlas() -> &'static Mutex<Atlas> {
    static A: OnceLock<Mutex<Atlas>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(Atlas { rgba: vec![0; 1024 * 1024 * 4], side: 1024, pen: (1, 1, 0), glyphs: HashMap::new(), dirty: true }))
}

fn face(f: Face) -> FontRef<'static> {
    FontRef::try_from_slice(if f == Face::Bold { BOLD } else { REGULAR }).expect("bundled Inter")
}

/// Screen pixels per layout pixel, from the last frame.
static SCALE: AtomicU32 = AtomicU32::new(0x4040_0000);

pub fn set_scale(k: f32) {
    SCALE.store(k.max(0.25).to_bits(), Ordering::Relaxed);
}

fn scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

/// The atlas when it changed since last asked (side, pixels).
pub fn take_dirty() -> Option<(u32, Vec<u8>)> {
    let mut a = atlas().lock().ok()?;
    if !a.dirty {
        return None;
    }
    a.dirty = false;
    Some((a.side as u32, a.rgba.clone()))
}

impl Atlas {
    fn glyph(&mut self, c: char, f: Face, px: u16) -> Option<Glyph> {
        if let Some(g) = self.glyphs.get(&(c, f, px)) {
            return *g;
        }
        let g = self.raster(c, f, px);
        self.glyphs.insert((c, f, px), g);
        g
    }

    fn raster(&mut self, c: char, f: Face, px: u16) -> Option<Glyph> {
        let font = face(f);
        let scale = PxScale::from(px as f32);
        let id = font.glyph_id(c);
        let adv = font.as_scaled(scale).h_advance(id);
        let Some(o) = font.outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0))) else {
            return Some(Glyph { at: [0; 4], off: [0.0; 2], adv });
        };
        let b = o.px_bounds();
        let (w, h) = (b.width().ceil() as usize + 1, b.height().ceil() as usize + 1);
        if self.pen.0 + w + 1 > self.side {
            self.pen = (1, self.pen.1 + self.pen.2 + 1, 0);
        }
        if self.pen.1 + h + 1 > self.side {
            // Full: start over, larger (glyphs are made again as asked).
            self.side = (self.side * 2).min(4096);
            self.rgba = vec![0; self.side * self.side * 4];
            self.pen = (1, 1, 0);
            self.glyphs.clear();
        }
        let (x0, y0) = (self.pen.0, self.pen.1);
        let side = self.side;
        o.draw(|x, y, v| {
            let k = ((y0 + y as usize) * side + x0 + x as usize) * 4;
            if k + 3 < self.rgba.len() {
                self.rgba[k..k + 4].copy_from_slice(&[255, 255, 255, (v.clamp(0.0, 1.0) * 255.0) as u8]);
            }
        });
        self.pen = (x0 + w + 1, y0, self.pen.2.max(h));
        self.dirty = true;
        Some(Glyph { at: [x0 as u16, y0 as u16, w as u16, h as u16], off: [b.min.x, b.min.y], adv })
    }
}

/// The raster size for an em of `em` layout pixels, and layout pixels per raster pixel.
fn bucket(em: f32) -> (u16, f32) {
    let want = em * scale();
    let px = BUCKETS.iter().copied().find(|&b| b as f32 >= want).unwrap_or(BUCKETS[BUCKETS.len() - 1]);
    (px, em / px as f32)
}

/// A string's width at `em` layout pixels.
pub fn width(s: &str, f: Face, em: f32) -> f32 {
    let (px, k) = bucket(em);
    let font = face(f);
    let sf = font.as_scaled(PxScale::from(px as f32));
    let mut prev = None;
    let mut w = 0.0;
    for c in s.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w * k
}

/// A string's glyph quads (layout rect, atlas uv) from x with its line's top at `top`; and the end x.
pub fn layout(s: &str, f: Face, em: f32, x: f32, top: f32) -> (Vec<([f32; 4], [f32; 4])>, f32) {
    let (px, k) = bucket(em);
    let font = face(f);
    let sf = font.as_scaled(PxScale::from(px as f32));
    let base = top + sf.ascent() * k;
    let Ok(mut a) = atlas().lock() else { return (Vec::new(), x) };
    let mut out = Vec::with_capacity(s.len());
    let mut pen = 0.0f32;
    let mut prev = None;
    for c in s.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            pen += sf.kern(p, id);
        }
        prev = Some(id);
        let Some(g) = a.glyph(c, f, px) else { continue };
        if g.at[2] > 0 {
            let side = a.side as f32;
            let [gx, gy, gw, gh] = g.at.map(|v| v as f32);
            let rect = [x + (pen + g.off[0]) * k, base + g.off[1] * k, gw * k, gh * k];
            out.push((rect, [gx / side, gy / side, (gx + gw) / side, (gy + gh) / side]));
        }
        pen += g.adv;
    }
    (out, x + pen * k)
}
