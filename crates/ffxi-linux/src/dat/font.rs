//! The client's bitmap font (FFXiMain FUN_10118d60 and helpers): a 64 x 128 atlas of 16 px cells
//! ("font    moji" in the language's menu DAT, or a PNG for the uncompressed / high quality
//! settings), advance widths from FFXiMain, and the "font    usgaiji" shapes for SJIS 0xEF1F..0xEF2F.
//! Text is Shift-JIS bytes; `encode_sjis` / `decode_sjis` convert from / to UTF-8.

use super::{DatIndex, chunks};
use super::zone::{Texture, parse_img};
use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use std::collections::HashMap;

/// Glyph cell size in screen pixels (and in atlas texels for the 1024x2048 atlases).
pub const CELL: i32 = 16;
pub const COLUMNS: i32 = 64;
pub const ROWS: i32 = 128;
/// Glyph indices 0x2115..0x2126 are usgaiji shapes (SJIS 0xEF1F..).
pub const GAIJI_FIRST: i32 = 0x2115;
pub const GAIJI_END: i32 = 0x2126;
/// Width table for glyph indices below this; the rest advance 14.
const WIDTHS_LEN: usize = 0x440;
/// FFXiMain.dll file offsets (.data VA 0x103607b0, .rdata VA 0x10327d98, .data VA 0x1037e428...).
const WIDTHS_AT: usize = 0x42bb0;
const WIDTHS_MD5: &str = "592a3523505144cc5dd1314f41421fca";
const PNG_FONTS_AT: usize = 0xa198;
/// Per language (0 ja, 1 en, 2 fr, 3 de; FUN_10250de0): table whose first u32 is the menu DAT id.
const MENU_TABLE_AT: [usize; 4] = [0x60828, 0x60280, 0x600d0, 0x60438];

/// Config "font" setting (DAT_10444188): 0 the DXT3 atlas, 1 / 2 the PNG atlases (FUN_10119400).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Compressed,
    Uncompressed,
    High,
}

/// One quad of a usgaiji shape (type 0x31 menu shape chunk, 61 bytes per part).
#[derive(Clone, Debug)]
pub struct ShapePart {
    /// Corners top-left, top-right, bottom-left, bottom-right, relative to the pen.
    pub corners: [[i16; 2]; 4],
    /// Texel rect in `texture`: w, h, u, v.
    pub rect: [i16; 4],
    /// Per-corner colour, BGRA, 0x80 = 1.0.
    pub colors: [[u8; 4]; 4],
    /// PS2 blend (A, B, D) selectors, as for FUN_10118ed0.
    pub blend: [u8; 3],
    pub texture: String,
}

#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub parts: Vec<ShapePart>,
}

impl Shape {
    /// Advance: horizontal extent of all parts (invisible parts act as spacers).
    pub fn width(&self) -> i32 {
        let xs = self.parts.iter().flat_map(|p| p.corners.iter().map(|c| c[0] as i32));
        let (lo, hi) = xs.fold((i32::MAX, i32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        if lo > hi { 0 } else { hi - lo }
    }
}

/// What a quad samples.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Atlas,
    Texture(String),
}

/// One textured screen rectangle. Pixel coordinates (top-left origin, D3D's -0.5 already removed).
#[derive(Clone, Debug)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// u0, v0, u1, v1 in 0..1.
    pub uv: [f32; 4],
    /// BGRA vertex colour, 0x80 = 1.0 (MODULATE2X).
    pub color: [u8; 4],
    pub source: Source,
    /// Glyphs: additive (FUN_10118aa0 sets SRCBLEND = DESTBLEND = ONE). Shapes: their PS2 blend.
    pub blend: [u8; 3],
}

/// PS2 (A, B, D) = (Cs, Cd, Cd): ordinary alpha blending.
pub const BLEND_ALPHA: [u8; 3] = [0, 1, 1];
/// Marker for the glyph path's ONE/ONE.
pub const BLEND_ADD: [u8; 3] = [0xFF, 0xFF, 0xFF];

pub struct Font {
    pub atlas: Texture,
    /// Atlas texels per cell: 16, or 32 for the high quality PNG.
    pub cell_px: u32,
    pub widths: Vec<i8>,
    pub gaiji: Vec<Shape>,
    /// Shape textures by IMG name (e.g. "font    elfire").
    pub textures: HashMap<String, Texture>,
    pub lang: u8,
}

/// Shift-JIS lead byte as the client tests it (FUN_1011a3e0).
pub fn is_lead(b: u8) -> bool {
    (0x81..0xA0).contains(&b) || (0xE0..0xF0).contains(&b) || (0xFA..0xFD).contains(&b)
}

/// Two-byte Shift-JIS to glyph index (FUN_10119620): JIS rows packed one after another.
pub fn sjis_index(b0: u8, b1: u8) -> i32 {
    let mut row: i32 = (b0 as i32 + if b0 < 0xA0 { -0x71 } else { -0xB1 }) * 2 | 1;
    let mut c = b1 as i32;
    if c > 0x7F {
        c -= 1;
    }
    if c < 0x9E {
        c -= 0x1F;
    } else {
        row += 1;
        c -= 0x7D;
    }
    let u = row << 8 | c;
    match u {
        _ if u > 0x931F => 0xCB,
        _ if u > 0x7920 => c - 0xCD1 + row * 0x5E,
        _ if u > 0x5020 => c - 0xDEC + row * 0x5E,
        _ if u > 0x3020 => c - 0xDC1 + row * 0x5E,
        _ if u > 0x2F20 => c + 0x21E + row,
        _ if u > 0x2E20 => c + 0x271 + row,
        _ if u > 0x2D5E => c + 0x351 + row,
        _ if u > 0x2D3F => c + 0x359 + row,
        _ if u > 0x2D20 => c + 0x35A + row,
        _ if u > 0x2A20 => c + 0x223 + row,
        _ if u > 0x2920 => c + 0x276 + row,
        _ if u > 0x2820 => c + 0x203 + row,
        _ if u > 0x2750 => c + 0x1B3 + row,
        _ if u > 0x2720 => c + 0x1C2 + row,
        _ if u > 0x2640 => c + 0x18B + row,
        _ if u > 0x2620 => c + 0x193 + row,
        _ if u > 0x2520 => c + 0x13E + row,
        _ if u > 0x2420 => c + 0xEC + row,
        _ if u > 0x2360 => c + 0x93 + row,
        _ if u > 0x2340 => c + 0x99 + row,
        _ if u > 0x232F => c + 0xA0 + row,
        0x227F => 0xF2,
        _ if u > 0x2271 => c + 0x56 + row,
        _ if u > 0x225B => c + 0x5D + row,
        _ if u > 0x2249 => c + 0x68 + row,
        _ if u > 0x2239 => c + 0x70 + row,
        _ => c - 0xBDF + row * 0x5E,
    }
}

/// Glyph indices of a Shift-JIS string (FUN_10119830): single bytes map to byte - 0x20, so
/// control bytes come out negative.
pub fn glyphs(sjis: &[u8]) -> Vec<i32> {
    let mut out = Vec::with_capacity(sjis.len());
    let mut i = 0;
    while i < sjis.len() && sjis[i] != 0 {
        if is_lead(sjis[i]) && i + 1 < sjis.len() {
            out.push(sjis_index(sjis[i], sjis[i + 1]));
            i += 2;
        } else {
            out.push(sjis[i] as i32 - 0x20);
            i += 1;
        }
    }
    out
}

fn parse_shapes(d: &[u8]) -> Result<Vec<Shape>> {
    let ntex = *d.get(16).context("shape chunk too short")? as usize;
    let mut p = 17 + ntex * 16;
    let count = u16::from_le_bytes(d.get(p..p + 2).context("shape count")?.try_into().unwrap()) as usize;
    p += 2;
    let mut shapes = Vec::with_capacity(count);
    for _ in 0..count {
        let n = *d.get(p).context("shape parts")? as usize;
        p += 1;
        let mut shape = Shape::default();
        for _ in 0..n {
            let r = d.get(p..p + 61).context("shape part out of range")?;
            let s = |o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
            let name = &r[45..61];
            let end = name.iter().position(|&c| c == 0).unwrap_or(16);
            shape.parts.push(ShapePart {
                corners: [[s(0), s(2)], [s(4), s(6)], [s(8), s(10)], [s(12), s(14)]],
                rect: [s(16), s(18), s(20), s(22)],
                colors: [0, 1, 2, 3].map(|k| super::ui::cell_color(r[25 + k * 4..29 + k * 4].try_into().unwrap())),
                blend: [r[42], r[43], r[44]],
                texture: String::from_utf8_lossy(&name[..end]).trim_end().to_string(),
            });
            p += 61;
        }
        shapes.push(shape);
    }
    Ok(shapes)
}

fn decode_png(data: &[u8]) -> Result<Texture> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = dec.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size().context("PNG too large")?];
    let info = reader.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba {
        bail!("PNG font is {:?}, expected RGBA", info.color_type);
    }
    buf.truncate(info.buffer_size());
    Ok(Texture::new("font    moji".into(), 0, info.width, info.height, buf))
}

impl Font {
    /// `lang`: 0 Japanese, 1 English, 2 French, 3 German (selects the menu DAT, FUN_10250de0).
    pub fn load(index: &DatIndex, ffximain: &[u8], lang: u8, quality: Quality) -> Result<Self> {
        let widths = ffximain.get(WIDTHS_AT..WIDTHS_AT + WIDTHS_LEN).context("FFXiMain.dll too short")?;
        if hex::encode(Md5::digest(widths)) != WIDTHS_MD5 {
            bail!("glyph width table at {WIDTHS_AT:#x} does not match this FFXiMain.dll build");
        }
        let u32_at = |o: usize| u32::from_le_bytes(ffximain[o..o + 4].try_into().unwrap());
        let table = *MENU_TABLE_AT.get(lang as usize).context("language must be 0..3")?;
        let menu_id = u32_at(table);
        let menu = index.read(menu_id)?;
        let mut moji = None;
        let mut textures = HashMap::new();
        let mut gaiji = Vec::new();
        for c in chunks(&menu)? {
            match c.kind {
                0x20 if c.data.get(1..17).is_some_and(|n| n.starts_with(b"font    moji")) => moji = Some(parse_img(c.data)?),
                0x20 => {
                    if let Ok(t) = parse_img(c.data) {
                        textures.insert(t.name.clone(), t);
                    }
                }
                0x31 if c.data.starts_with(b"font    usgaiji ") => gaiji = parse_shapes(c.data)?,
                _ => {}
            }
        }
        let atlas = match quality {
            Quality::Compressed => moji.with_context(|| format!("no font moji in menu DAT {menu_id}"))?,
            Quality::Uncompressed => decode_png(&index.read(u32_at(PNG_FONTS_AT))?)?,
            Quality::High => decode_png(&index.read(u32_at(PNG_FONTS_AT + 12))?)?,
        };
        let cell_px = atlas.width / COLUMNS as u32;
        if cell_px == 0 || atlas.height != cell_px * ROWS as u32 {
            bail!("font atlas is {}x{}, expected 64x128 cells", atlas.width, atlas.height);
        }
        Ok(Self { atlas, cell_px, widths: widths.iter().map(|&b| b as i8).collect(), gaiji, textures, lang })
    }

    /// Pen advance of a glyph index (FUN_1011a1f0).
    pub fn advance(&self, idx: i32) -> i32 {
        match idx {
            _ if idx < 0 => 0,
            GAIJI_FIRST..GAIJI_END => self.gaiji.get((idx - GAIJI_FIRST) as usize).map_or(0, Shape::width),
            _ if (idx as usize) < WIDTHS_LEN => self.widths[idx as usize] as i32,
            _ => 14,
        }
    }

    pub fn text_width(&self, sjis: &[u8]) -> i32 {
        glyphs(sjis).into_iter().map(|g| self.advance(g)).sum()
    }

    /// One line at pen (x, y), as FUN_10118d60: 16 px cells, pen += round(advance * scale).
    /// `color` is BGRA with 0x80 = 1.0; gaiji shapes ignore it (drawn with 0x80808080).
    pub fn layout_line(&self, sjis: &[u8], x: i32, y: i32, color: [u8; 4], scale: [f32; 2], out: &mut Vec<Quad>) -> i32 {
        let mut pen = x;
        for g in glyphs(sjis) {
            // French layout pulls the full-width opening quote 8 px left.
            if g == 0x87 && self.lang == 2 && pen >= 8 {
                pen -= 8;
            }
            if (0..COLUMNS * ROWS).contains(&g) {
                let (col, row) = ((g % COLUMNS) as f32, (g / COLUMNS) as f32);
                out.push(Quad {
                    x: pen as f32,
                    y: y as f32,
                    w: CELL as f32 * scale[0],
                    h: CELL as f32 * scale[1],
                    uv: [col / COLUMNS as f32, row / ROWS as f32, (col + 1.0) / COLUMNS as f32, (row + 1.0) / ROWS as f32],
                    color,
                    source: Source::Atlas,
                    blend: BLEND_ADD,
                });
            } else if let Some(shape) = (GAIJI_FIRST..GAIJI_END).contains(&g).then(|| self.gaiji.get((g - GAIJI_FIRST) as usize)).flatten() {
                for part in &shape.parts {
                    let Some(tex) = self.textures.get(&part.texture) else { continue };
                    let [w, h, u, v] = part.rect.map(|c| c as f32);
                    let (x0, y0) = (part.corners[0][0] as f32, part.corners[0][1] as f32);
                    let (x1, y1) = (part.corners[3][0] as f32, part.corners[3][1] as f32);
                    out.push(Quad {
                        x: pen as f32 + x0,
                        y: y as f32 + y0,
                        w: x1 - x0,
                        h: y1 - y0,
                        uv: [u / tex.width as f32, v / tex.height as f32, (u + w) / tex.width as f32, (v + h) / tex.height as f32],
                        color: part.colors[0],
                        source: Source::Texture(part.texture.clone()),
                        blend: part.blend,
                    });
                }
            }
            pen += (self.advance(g) as f32 * scale[0] + 0.5) as i32;
        }
        pen
    }

    /// Lines split at '\n', 16 px apart (FUN_10211860). Returns the bounding size.
    pub fn layout(&self, sjis: &[u8], x: i32, y: i32, color: [u8; 4], out: &mut Vec<Quad>) -> (i32, i32) {
        let mut w = 0;
        let lines: Vec<&[u8]> = sjis.split(|&b| b == b'\n').collect();
        for (i, line) in lines.iter().enumerate() {
            w = w.max(self.layout_line(line, x, y + i as i32 * CELL, color, [1.0, 1.0], out) - x);
        }
        (w, lines.len() as i32 * CELL)
    }

    fn sample(tex: &Texture, u: f32, v: f32) -> [f32; 4] {
        // Bilinear with clamping, texel centres at +0.5 (D3D's linear filter).
        let (fx, fy) = (u * tex.width as f32 - 0.5, v * tex.height as f32 - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let px = |x: f32, y: f32| {
            let xi = (x as i32).clamp(0, tex.width as i32 - 1) as usize;
            let yi = (y as i32).clamp(0, tex.height as i32 - 1) as usize;
            let o = (yi * tex.width as usize + xi) * 4;
            [0, 1, 2, 3].map(|k| tex.rgba[o + k] as f32 / 255.0)
        };
        let (a, b, c, d) = (px(x0, y0), px(x0 + 1.0, y0), px(x0, y0 + 1.0), px(x0 + 1.0, y0 + 1.0));
        [0, 1, 2, 3].map(|k| (a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay) + (c[k] * (1.0 - ax) + d[k] * ax) * ay)
    }

    /// Software compositor for previews: draws quads onto an RGBA8 image as the client's D3D states
    /// do (MODULATE2X with the vertex colour, alpha test > 0; glyphs ONE/ONE unless `alpha_glyphs`).
    pub fn draw(&self, quads: &[Quad], img: &mut [u8], width: u32, height: u32, alpha_glyphs: bool) {
        for q in quads {
            let tex = match &q.source {
                Source::Atlas => &self.atlas,
                Source::Texture(n) => match self.textures.get(n) {
                    Some(t) => t,
                    None => continue,
                },
            };
            // MODULATE2X: texture * diffuse * 2.
            let vc = [q.color[2], q.color[1], q.color[0], q.color[3]].map(|c| c as f32 * 2.0 / 255.0);
            let (x0, y0) = (q.x.floor().max(0.0) as u32, q.y.floor().max(0.0) as u32);
            let (x1, y1) = (((q.x + q.w).ceil().max(0.0) as u32).min(width), ((q.y + q.h).ceil().max(0.0) as u32).min(height));
            for py in y0..y1 {
                for px in x0..x1 {
                    let s = (px as f32 + 0.5 - q.x) / q.w;
                    let t = (py as f32 + 0.5 - q.y) / q.h;
                    if !(0.0..1.0).contains(&s) || !(0.0..1.0).contains(&t) {
                        continue;
                    }
                    let texel = Self::sample(tex, q.uv[0] + s * (q.uv[2] - q.uv[0]), q.uv[1] + t * (q.uv[3] - q.uv[1]));
                    let src = [0, 1, 2, 3].map(|k| (texel[k] * vc[k]).min(1.0));
                    // The UI runs with ALPHATESTENABLE, ALPHAFUNC GREATER, ALPHAREF 0 (FUN_10159ae0).
                    if src[3] <= 0.0 {
                        continue;
                    }
                    let o = ((py * width + px) * 4) as usize;
                    let dst = [0, 1, 2].map(|k| img[o + k] as f32 / 255.0);
                    let blend = if q.blend == BLEND_ADD && alpha_glyphs { BLEND_ALPHA } else { q.blend };
                    let out = if blend == BLEND_ADD {
                        [0, 1, 2].map(|k| dst[k] + src[k])
                    } else {
                        // PS2: (A - B) * As + D with 0 = source, 1 = destination, 2 = zero.
                        let pick = |sel: u8, k: usize| match sel {
                            0 => src[k],
                            1 => dst[k],
                            _ => 0.0,
                        };
                        [0, 1, 2].map(|k| (pick(blend[0], k) - pick(blend[1], k)) * src[3] + pick(blend[2], k))
                    };
                    for k in 0..3 {
                        img[o + k] = (out[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
        }
    }
}

/// Shift-JIS (CP932 plus the client's gaiji) to UTF-8. Gaiji 0xEF1F.. map to U+E000..; bytes below
/// 0x20 pass through as control characters; undecodable pairs become U+FFFD.
pub fn decode_sjis(sjis: &[u8]) -> String {
    let mut out = String::with_capacity(sjis.len());
    let mut i = 0;
    while i < sjis.len() {
        let b = sjis[i];
        if is_lead(b) && i + 1 < sjis.len() {
            let pair = &sjis[i..i + 2];
            let g = sjis_index(pair[0], pair[1]);
            if (GAIJI_FIRST..GAIJI_END).contains(&g) {
                out.push(char::from_u32(0xE000 + (g - GAIJI_FIRST) as u32).unwrap());
            } else {
                let (s, bad) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(pair);
                out.push_str(if bad { "\u{FFFD}" } else { &s });
            }
            i += 2;
        } else {
            if b < 0x80 {
                out.push(b as char);
            } else {
                let (s, bad) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&sjis[i..i + 1]);
                out.push_str(if bad { "\u{FFFD}" } else { &s });
            }
            i += 1;
        }
    }
    out
}

/// UTF-8 to Shift-JIS; U+E000.. become the gaiji codes, unmappable characters '?'.
pub fn encode_sjis(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2);
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let cp = ch as u32;
        if (0xE000..0xE000 + (GAIJI_END - GAIJI_FIRST) as u32).contains(&cp) {
            out.extend_from_slice(&[0xEF, 0x1F + (cp - 0xE000) as u8]);
        } else if cp < 0x80 {
            out.push(cp as u8);
        } else {
            let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(ch.encode_utf8(&mut buf));
            if bad { out.push(b'?') } else { out.extend_from_slice(&bytes) }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sjis_rows() {
        // JIS 0x2121 (SJIS 0x8140) follows the 96 single-byte cells; kanji level 1 starts at row 16.
        assert_eq!(sjis_index(0x81, 0x40), 96);
        assert_eq!(sjis_index(0x88, 0x9F), 1024);
        // Gaiji: SJIS 0xEF1F.. are the usgaiji shapes.
        assert_eq!(sjis_index(0xEF, 0x1F), GAIJI_FIRST);
        assert_eq!(sjis_index(0xEF, 0x2F), GAIJI_END - 1);
    }

    #[test]
    fn sjis_round_trip() {
        let s = "Lion ライオン 獅子 \u{E000}";
        let b = encode_sjis(s);
        assert_eq!(&b[..5], b"Lion ");
        assert_eq!(decode_sjis(&b), s);
    }
}
