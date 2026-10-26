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
    // The badges right to left from the name: the first at the text's height, those after it smaller; the job master
    // stars above the name.
    let size = EM * 0.95 * scale[1];
    let mut bx = x - text_w / 2.0 - 4.0 * scale[0];
    let cy = top + raster.baseline * k - 0.36 * EM * scale[1];
    for (i, b) in badges(prefix).into_iter().enumerate() {
        let Some(key) = badge(gu, b) else { continue };
        let d = if i == 0 { size } else { size * 0.75 };
        bx -= d;
        // The pearl takes its linkshell's colour (retail's icon tint for 0x92).
        let c = if b == 0x92 { [16, 8, 0].map(|sh| ((icon >> sh & 0xff) as f32 / 128.0).min(1.0)) } else { [1.0; 3] };
        gu.image_rect(&key, bx.round(), (cy - d / 2.0).round(), d, d, [c[0], c[1], c[2], pulse.clamp(0.0, 1.0)]);
        bx -= 1.0 * scale[0];
    }
    if stars && let Some(key) = badge(gu, 0xad) {
        let d = size * 0.6;
        for (dx, big) in [(-1.0, false), (0.0, true), (1.0, false)] {
            let s = if big { d * 1.25 } else { d };
            gu.image_rect(&key, x + dx * d * 1.1 - s / 2.0, top - s + 2.0 * scale[1], s, s, [1.0, 1.0, 1.0, pulse.clamp(0.0, 1.0)]);
        }
    }
}

/// The modern badges by retail's glyph byte (assets/modern/badges, from the VectorCraft set the user approved).
const BADGES: [(u8, &[u8]); 32] = [
    (0x8e, include_bytes!("../../../assets/modern/badges/badge_8e.png")),
    (0x8f, include_bytes!("../../../assets/modern/badges/badge_8f.png")),
    (0x90, include_bytes!("../../../assets/modern/badges/badge_90.png")),
    (0x91, include_bytes!("../../../assets/modern/badges/badge_91.png")),
    (0x92, include_bytes!("../../../assets/modern/badges/badge_92.png")),
    (0x93, include_bytes!("../../../assets/modern/badges/badge_93.png")),
    (0x94, include_bytes!("../../../assets/modern/badges/badge_94.png")),
    (0x95, include_bytes!("../../../assets/modern/badges/badge_95.png")),
    (0x96, include_bytes!("../../../assets/modern/badges/badge_96.png")),
    (0x99, include_bytes!("../../../assets/modern/badges/badge_99.png")),
    (0x9c, include_bytes!("../../../assets/modern/badges/badge_9c.png")),
    (0x9d, include_bytes!("../../../assets/modern/badges/badge_9d.png")),
    (0x9e, include_bytes!("../../../assets/modern/badges/badge_9e.png")),
    (0x9f, include_bytes!("../../../assets/modern/badges/badge_9f.png")),
    (0xa0, include_bytes!("../../../assets/modern/badges/badge_a0.png")),
    (0xa1, include_bytes!("../../../assets/modern/badges/badge_a1.png")),
    (0xa2, include_bytes!("../../../assets/modern/badges/badge_a2.png")),
    (0xa3, include_bytes!("../../../assets/modern/badges/badge_a3.png")),
    (0xa4, include_bytes!("../../../assets/modern/badges/badge_a4.png")),
    (0xa5, include_bytes!("../../../assets/modern/badges/badge_a5.png")),
    (0xa6, include_bytes!("../../../assets/modern/badges/badge_a6.png")),
    (0xa7, include_bytes!("../../../assets/modern/badges/badge_a7.png")),
    (0xa8, include_bytes!("../../../assets/modern/badges/badge_a8.png")),
    (0xa9, include_bytes!("../../../assets/modern/badges/badge_a9.png")),
    (0xaa, include_bytes!("../../../assets/modern/badges/badge_aa.png")),
    (0xab, include_bytes!("../../../assets/modern/badges/badge_ab.png")),
    (0xac, include_bytes!("../../../assets/modern/badges/badge_ac.png")),
    (0xad, include_bytes!("../../../assets/modern/badges/badge_ad.png")),
    (0xae, include_bytes!("../../../assets/modern/badges/badge_ae.png")),
    (0xaf, include_bytes!("../../../assets/modern/badges/badge_af.png")),
    (0xb0, include_bytes!("../../../assets/modern/badges/badge_b0.png")),
    (0xb1, include_bytes!("../../../assets/modern/badges/badge_b1.png")),
];

/// The prefix's badges in order: rank bytes 0xc8..0xcd as the sword and shield with their 0..2 stars (FUN_10086210),
/// the GM levels' shared triangle, the recruiter's star already in its orb; letters left out.
fn badges(prefix: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in prefix {
        match c {
            0xc8..=0xcd => {
                out.push(0xa9);
                out.extend((0..(c - 0xc8) % 3).map(|_| 0xaa));
            }
            0x96..=0x98 => out.push(0x96),
            0x8e..=0xb1 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A badge's image in the UI pass, uploaded once; None for a byte without one.
fn badge(gu: &mut GameUi, b: u8) -> Option<String> {
    let key = format!("badge {b:02x}");
    if gu.has_image(&key) {
        return Some(key);
    }
    let png = BADGES.iter().find(|(k, _)| *k == b)?.1;
    let mut d = png::Decoder::new(std::io::Cursor::new(png));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    gu.image(&key, info.width, info.height, buf);
    Some(key)
}
