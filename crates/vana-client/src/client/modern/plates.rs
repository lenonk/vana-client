//! The modern name plates: the name in Inter Bold Italic with a dark outline, in retail's name colour and the target's
//! pulse, at retail's place and scale, depth-tested in the same UI pass so walls still hide them; retail's badges left
//! of the name and the master stars above it. No bars over heads (user, 2026-10-26). Each name is rasterized once per
//! size bucket and uploaded as a UI image; a frame only draws quads.

use super::super::game_ui::GameUi;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Raster heights (em, px): the smallest at or above the drawn size is used, so a plate is never minified much.
const BUCKETS: [f32; 4] = [16.0, 28.0, 48.0, 80.0];
/// The drawn em in retail plate units (times the plate's y scale): capitals as tall as retail's letters (about 10.6
/// units); the baseline below the anchor so the text's middle sits where retail's does.
const EM: f32 = 15.5;
const BASELINE: f32 = 8.8;
/// Extra space between letters, in em: retail's plates are widely set.
const TRACKING: f32 = 0.08;

/// A rasterized name: its image size, the text's left, right and baseline in it.
#[derive(Clone, Copy)]
struct Raster {
    w: u32,
    h: u32,
    left: f32,
    right: f32,
    baseline: f32,
}

fn metrics() -> &'static Mutex<HashMap<String, Raster>> {
    static M: OnceLock<Mutex<HashMap<String, Raster>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// White text with a soft dark outline (straight alpha), padded for the italic overhang and the outline.
fn rasterize(text: &str, px: f32) -> Option<(Raster, Vec<u8>)> {
    let font = FontRef::try_from_slice(super::theme::font("Inter-BoldItalic")?).ok()?;
    let scale = PxScale::from(px);
    let sf = font.as_scaled(scale);
    let r = (px / 10.0).max(1.5);
    let pad = (r + px * 0.25).ceil();
    let mut caret = 0.0f32;
    let mut prev = None;
    let mut glyphs = Vec::new();
    for c in text.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            caret += sf.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(scale, point(pad + caret, pad + sf.ascent())));
        caret += sf.h_advance(id) + TRACKING * px;
        prev = Some(id);
    }
    let (w, h) = ((caret + 2.0 * pad).ceil() as usize, (sf.ascent() - sf.descent() + 2.0 * pad).ceil() as usize);
    let mut cov = vec![0f32; w * h];
    for g in glyphs {
        let Some(o) = font.outline_glyph(g) else { continue };
        let b = o.px_bounds();
        o.draw(|x, y, c| {
            let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let k = py as usize * w + px as usize;
                cov[k] = (cov[k] + c).min(1.0);
            }
        });
    }
    // The outline: coverage spread over a disc of radius r, fading over its last pixel.
    let ri = r.ceil() as i32;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut o = 0f32;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    let fall = (r + 1.0 - d).clamp(0.0, 1.0);
                    o = o.max(cov[sy as usize * w + sx as usize] * fall);
                }
            }
            let f = cov[y as usize * w + x as usize];
            let o = o * 0.85;
            let a = f + o * (1.0 - f);
            let k = (y as usize * w + x as usize) * 4;
            if a > 0.0 {
                // White over a near-black outline.
                let c = (255.0 * f + 12.0 * o * (1.0 - f)) / a;
                out[k..k + 4].copy_from_slice(&[c as u8, c as u8, (c + 2.0 * o * (1.0 - f)).min(255.0) as u8, (a * 255.0) as u8]);
            }
        }
    }
    Some((Raster { w: w as u32, h: h as u32, left: pad, right: pad + caret - TRACKING * px, baseline: pad + sf.ascent() }, out))
}

/// A plate: the name centred on x with its baseline BASELINE units below y, tinted (ARGB, 0x80 = 1.0) and pulsed;
/// retail's badges just left of it and its master stars above.
#[allow(clippy::too_many_arguments)]
pub fn draw(gu: &mut GameUi, prefix: &[u8], name: &str, x: f32, y: f32, scale: [f32; 2], tint: u32, pulse: f32, icon: u32, stars: bool) {
    if name.is_empty() {
        return;
    }
    let em = EM * scale[1];
    let px = BUCKETS.iter().copied().find(|&b| b >= em).unwrap_or(BUCKETS[3]);
    let key = format!("plate {px} {name}");
    let raster = {
        let mut m = metrics().lock().unwrap();
        match m.get(&key).copied().filter(|_| gu.has_image(&key)) {
            Some(r) => r,
            None => {
                let Some((r, rgba)) = rasterize(name, px) else { return };
                gu.image(&key, r.w, r.h, rgba);
                m.insert(key.clone(), r);
                r
            }
        }
    };
    let k = em / px;
    let text_w = (raster.right - raster.left) * k;
    let left = x - text_w / 2.0 - raster.left * k;
    let top = y + BASELINE * scale[1] - raster.baseline * k;
    let c = |sh: u32| ((tint >> sh & 0xff) as f32 / 128.0).min(1.0);
    gu.image_rect(&key, left.round(), top.round(), raster.w as f32 * k, raster.h as f32 * k, [c(16), c(8), c(0), pulse.clamp(0.0, 1.0)]);
    // Retail's badges (GM, new adventurer, linkshell ...) end just left of the name; the stars stay above.
    // Retail's badges at retail's size and height, ending just left of the name.
    if !prefix.is_empty() {
        gu.name_plate(prefix, "", x - text_w / 2.0 - 6.8 * scale[0], y, scale, tint, pulse, icon, false);
    }
    if stars {
        gu.name_plate(&[], "", x, y, scale, tint, pulse, icon, true);
    }
}
