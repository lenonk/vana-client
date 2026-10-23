//! Windows cursor files from the game folder (mousenor.ani, mousehit.ani, arw_*.ani, oarw_*.ani; mousenor.cur): what
//! retail hands to LoadCursorFromFileA. `.cur` is an icon directory (type 2) whose entries carry the hotspot and hold a
//! DIB (XOR image with an AND mask under it) or a PNG; `.ani` is RIFF "ACON": "anih" (frames, steps, jiffies per step,
//! flags), optional "rate" (jiffies per step) and "seq " (frame per step), and LIST "fram" of "icon" chunks, each a .cur.

use anyhow::{Result, bail};

/// One cursor image, RGBA, with its hotspot.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub hotspot: (u16, u16),
}

/// A cursor: its images and the steps that show them, each (frame, jiffies of 1/60 s).
#[derive(Clone, Debug)]
pub struct Cursor {
    pub frames: Vec<Frame>,
    pub steps: Vec<(usize, u32)>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

/// A `.cur` (or `.ico`) file: its first image.
pub fn parse_cur(b: &[u8]) -> Result<Frame> {
    let (Some(0), Some(kind), Some(count)) = (u16_at(b, 0), u16_at(b, 2), u16_at(b, 4)) else { bail!("not an icon directory") };
    if !matches!(kind, 1 | 2) || count == 0 {
        bail!("icon directory type {kind} with {count} images");
    }
    let e = 6;
    let (w, h) = (b[e] as u32, b[e + 1] as u32);
    let hotspot = if kind == 2 { (u16_at(b, e + 4).unwrap_or(0), u16_at(b, e + 6).unwrap_or(0)) } else { (0, 0) };
    let (Some(len), Some(at)) = (u32_at(b, e + 8), u32_at(b, e + 12)) else { bail!("short icon entry") };
    let Some(img) = b.get(at as usize..(at + len) as usize) else { bail!("icon image out of range") };
    let (width, height, rgba) = if img.starts_with(b"\x89PNG") { png(img)? } else { dib(img, w, h)? };
    Ok(Frame { width, height, rgba, hotspot })
}

fn png(img: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::Cursor::new(img));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = d.read_info()?;
    let mut buf = vec![0; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf)?;
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..(w * h * 4) as usize].to_vec(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).take((w * h) as usize).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => bail!("cursor PNG colour type {other:?}"),
    };
    Ok((w, h, rgba))
}

/// A DIB as in icon files: BITMAPINFOHEADER whose height covers the XOR image and the AND mask, palette for <= 8 bpp,
/// rows bottom-up padded to 4 bytes. 32 bpp carries its own alpha; otherwise the AND mask's set bits are transparent.
fn dib(img: &[u8], w0: u32, h0: u32) -> Result<(u32, u32, Vec<u8>)> {
    let (Some(hdr), Some(w), Some(h2), Some(bpp)) = (u32_at(img, 0), u32_at(img, 4), u32_at(img, 8), u16_at(img, 14)) else { bail!("short DIB header") };
    let w = if w > 0 { w } else if w0 > 0 { w0 } else { 256 };
    let h = if h2 > 0 { h2 / 2 } else if h0 > 0 { h0 } else { 256 };
    let colors = match u32_at(img, 32).unwrap_or(0) {
        0 if bpp <= 8 => 1usize << bpp,
        n => n as usize,
    };
    let pal_at = hdr as usize;
    let px_at = pal_at + if bpp <= 8 { colors * 4 } else { 0 };
    let stride = ((w as usize * bpp as usize + 31) / 32) * 4;
    let mask_at = px_at + stride * h as usize;
    let mask_stride = ((w as usize + 31) / 32) * 4;
    let mut out = vec![0u8; (w * h * 4) as usize];
    let mut any_alpha = false;
    for y in 0..h as usize {
        let row = px_at + (h as usize - 1 - y) * stride;
        let mrow = mask_at + (h as usize - 1 - y) * mask_stride;
        for x in 0..w as usize {
            let (r, g, b, a) = match bpp {
                32 => {
                    let p = img.get(row + x * 4..row + x * 4 + 4).unwrap_or(&[0; 4]);
                    any_alpha |= p[3] != 0;
                    (p[2], p[1], p[0], p[3])
                }
                24 => {
                    let p = img.get(row + x * 3..row + x * 3 + 3).unwrap_or(&[0; 3]);
                    (p[2], p[1], p[0], 255)
                }
                1 | 4 | 8 => {
                    let bit = x * bpp as usize;
                    let byte = img.get(row + bit / 8).copied().unwrap_or(0);
                    let idx = (byte >> (8 - bpp as usize - bit % 8)) & ((1u16 << bpp) - 1) as u8;
                    let p = img.get(pal_at + idx as usize * 4..pal_at + idx as usize * 4 + 4).unwrap_or(&[0; 4]);
                    (p[2], p[1], p[0], 255)
                }
                other => bail!("cursor DIB with {other} bits per pixel"),
            };
            let masked = img.get(mrow + x / 8).is_some_and(|m| m & (0x80 >> (x % 8)) != 0);
            let o = (y * w as usize + x) * 4;
            out[o..o + 4].copy_from_slice(&[r, g, b, a]);
            if bpp != 32 && masked {
                out[o + 3] = 0;
            }
        }
    }
    // A 32 bpp image without any alpha leans on its mask.
    if bpp == 32 && !any_alpha {
        for y in 0..h as usize {
            let mrow = mask_at + (h as usize - 1 - y) * mask_stride;
            for x in 0..w as usize {
                let masked = img.get(mrow + x / 8).is_some_and(|m| m & (0x80 >> (x % 8)) != 0);
                out[(y * w as usize + x) * 4 + 3] = if masked { 0 } else { 255 };
            }
        }
    }
    Ok((w, h, out))
}

/// A `.ani` (RIFF ACON) or a plain `.cur`.
pub fn parse(b: &[u8]) -> Result<Cursor> {
    if !b.starts_with(b"RIFF") {
        return Ok(Cursor { frames: vec![parse_cur(b)?], steps: vec![(0, 0)] });
    }
    if b.get(8..12) != Some(b"ACON") {
        bail!("RIFF file is not ACON");
    }
    let (mut frames, mut rate, mut seq) = (Vec::new(), Vec::new(), Vec::new());
    let (mut steps, mut jiffies) = (0usize, 10u32);
    let mut walk = vec![(12usize, b.len())];
    while let Some((mut o, end)) = walk.pop() {
        while o + 8 <= end {
            let id = &b[o..o + 4];
            let n = u32_at(b, o + 4).unwrap_or(0) as usize;
            let body = o + 8;
            match id {
                b"LIST" => walk.push((body + 4, (body + n).min(end))),
                b"anih" => {
                    steps = u32_at(b, body + 8).unwrap_or(0) as usize;
                    jiffies = u32_at(b, body + 28).unwrap_or(10);
                }
                b"rate" => rate = (0..n / 4).filter_map(|i| u32_at(b, body + i * 4)).collect(),
                b"seq " => seq = (0..n / 4).filter_map(|i| u32_at(b, body + i * 4).map(|v| v as usize)).collect(),
                b"icon" => frames.push(parse_cur(b.get(body..body + n).unwrap_or(&[]))?),
                _ => {}
            }
            o = body + n + (n & 1);
        }
    }
    if frames.is_empty() {
        bail!("animated cursor without frames");
    }
    let steps = steps.max(if seq.is_empty() { frames.len() } else { seq.len() });
    let steps = (0..steps).map(|i| (seq.get(i).copied().unwrap_or(i).min(frames.len() - 1), rate.get(i).copied().unwrap_or(jiffies))).collect();
    Ok(Cursor { frames, steps })
}

#[cfg(test)]
mod tests {
    /// Retail's mousenor.ani: 4 frames of 32 x 32, hotspot (19, 19), 9 jiffies a step; the corner pixel transparent.
    #[test]
    fn retail_normal_cursor() {
        let dir = crate::config::Config::load().ffxi_dir();
        let Ok(b) = std::fs::read(dir.join("mousenor.ani")) else { return };
        let c = super::parse(&b).unwrap();
        assert_eq!(c.frames.len(), 4);
        assert_eq!(c.steps, vec![(0, 9), (1, 9), (2, 9), (3, 9)]);
        let f = &c.frames[0];
        assert_eq!((f.width, f.height, f.hotspot), (32, 32, (19, 19)));
        assert_eq!(f.rgba[3], 0);
        assert!(f.rgba.chunks(4).any(|p| p[3] == 255));
        // Every cursor file in the game folder and the mods parses.
        let mods = crate::config::mods_dir();
        let dirs = std::iter::once(dir).chain(std::fs::read_dir(&mods).into_iter().flatten().flatten().map(|e| e.path()));
        for d in dirs {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ani")) {
                    let c = super::parse(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:#}", p.display()));
                    assert!(!c.frames.is_empty() && c.frames.iter().all(|f| f.rgba.len() == (f.width * f.height * 4) as usize), "{}", p.display());
                }
            }
        }
    }
}
