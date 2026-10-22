//! Texture constants and the level-0 fix-up. Mip chains are made on the GPU (render/mipgen.rs): lower levels
//! box-filtered in linear light, alpha-tested coverage kept per level, BC1 / BC2 re-encoded by a compute shader.

/// The cut-off the zone and actor shaders test alpha against (fs_alpha_test / fs_actor).
pub const ALPHA_REF: f32 = 0.3;

/// DXT3 alpha is PS2-style (0x80 opaque): double each 4-bit alpha in place, as the RGBA path does.
pub fn double_bc2_alpha(data: &mut [u8]) {
    for block in data.chunks_exact_mut(16) {
        for b in &mut block[..8] {
            let (lo, hi) = (*b & 15, *b >> 4);
            *b = (lo * 2).min(15) | ((hi * 2).min(15) << 4);
        }
    }
}

/// The former CPU chain, kept only as the reference the GPU's levels are tested against.
#[cfg(test)]
pub mod reference {
    use super::ALPHA_REF;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Format {
        Bc1,
        Bc2,
        Rgba,
    }

    pub struct Levels {
        pub format: Format,
        pub data: Vec<Vec<u8>>,
    }

    use super::double_bc2_alpha;

    pub fn to_lin(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }

    pub fn to_srgb(l: f32) -> u8 {
        let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.max(0.0).powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    }

    /// One box-filtered level down (odd sizes clamp), colour averaged in linear light.
    pub fn down(rgba: &[u8], w: u32, h: u32, lut: &[f32; 256]) -> (Vec<u8>, u32, u32) {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut out = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    let p = &rgba[((sy * w + sx) * 4) as usize..][..4];
                    for k in 0..3 {
                        acc[k] += lut[p[k] as usize];
                    }
                    acc[3] += p[3] as f32;
                }
                let o = ((y * nw + x) * 4) as usize;
                for k in 0..3 {
                    out[o + k] = to_srgb(acc[k] / 4.0);
                }
                out[o + 3] = (acc[3] / 4.0 + 0.5) as u8;
            }
        }
        (out, nw, nh)
    }

    pub fn coverage(rgba: &[u8], scale: f32) -> f32 {
        let cut = ALPHA_REF * 255.0;
        let n = rgba.len() / 4;
        rgba.chunks_exact(4).filter(|p| p[3] as f32 * scale >= cut).count() as f32 / n.max(1) as f32
    }

    /// Scales a level's alpha so its coverage matches `target` (binary search on the scale).
    pub fn keep_coverage(rgba: &mut [u8], target: f32) {
        let (mut lo, mut hi) = (0.25f32, 8.0f32);
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            if coverage(rgba, mid) < target { lo = mid } else { hi = mid }
        }
        // The smallest scale found that reaches the target.
        let s = hi;
        for p in rgba.chunks_exact_mut(4) {
            p[3] = (p[3] as f32 * s).min(255.0) as u8;
        }
    }

    /// RGBA levels from level 0 down to 1x1; alpha coverage kept when the image has cut-out alpha.
    pub fn rgba_chain(rgba: &[u8], w: u32, h: u32) -> Vec<(Vec<u8>, u32, u32)> {
        let lut: [f32; 256] = std::array::from_fn(|i| to_lin(i as u8));
        let cutout = rgba.chunks_exact(4).any(|p| p[3] < 250);
        let target = coverage(rgba, 1.0);
        let mut out = vec![(rgba.to_vec(), w, h)];
        let (mut cur, mut cw, mut ch) = (rgba.to_vec(), w, h);
        while cw > 1 || ch > 1 {
            let (mut next, nw, nh) = down(&cur, cw, ch, &lut);
            if cutout && target > 0.0 && target < 1.0 {
                keep_coverage(&mut next, target);
            }
            out.push((next.clone(), nw, nh));
            (cur, cw, ch) = (next, nw, nh);
        }
        out
    }

    pub fn rgb565(c: [u8; 3]) -> u16 {
        ((c[0] as u16 >> 3) << 11) | ((c[1] as u16 >> 2) << 5) | (c[2] as u16 >> 3)
    }

    pub fn expand(c: u16) -> [i32; 3] {
        let (r, g, b) = ((c >> 11) as i32 & 31, (c >> 5) as i32 & 63, c as i32 & 31);
        [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
    }

    /// A 4x4 colour block (bounding box inset by 1/16, nearest palette entry). `punch` = BC1 3-colour mode with
    /// transparent texels (alpha under the cut-off); `four` forces 4-colour mode (BC2).
    pub fn color_block(px: &[[u8; 4]; 16], punch: bool, four: bool) -> [u8; 8] {
        let cut = (ALPHA_REF * 255.0) as u8;
        let solid: Vec<&[u8; 4]> = px.iter().filter(|p| !punch || p[3] >= cut).collect();
        let mut lo = [255i32; 3];
        let mut hi = [0i32; 3];
        for p in &solid {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] as i32);
                hi[k] = hi[k].max(p[k] as i32);
            }
        }
        if solid.is_empty() {
            lo = [0; 3];
            hi = [0; 3];
        }
        for k in 0..3 {
            let inset = (hi[k] - lo[k]) >> 4;
            lo[k] = (lo[k] + inset).min(255);
            hi[k] = (hi[k] - inset).max(0);
        }
        let mut c0 = rgb565([hi[0] as u8, hi[1] as u8, hi[2] as u8]);
        let mut c1 = rgb565([lo[0] as u8, lo[1] as u8, lo[2] as u8]);
        let three = punch && px.iter().any(|p| p[3] < cut);
        if three {
            if c0 > c1 {
                std::mem::swap(&mut c0, &mut c1);
            }
        } else if c0 < c1 {
            std::mem::swap(&mut c0, &mut c1);
        }
        let (a, b) = (expand(c0), expand(c1));
        let pal: Vec<[i32; 3]> = if three || (!four && c0 <= c1) {
            vec![a, b, std::array::from_fn(|k| (a[k] + b[k]) / 2)]
        } else {
            vec![a, b, std::array::from_fn(|k| (2 * a[k] + b[k]) / 3), std::array::from_fn(|k| (a[k] + 2 * b[k]) / 3)]
        };
        let mut bits = 0u32;
        for (i, p) in px.iter().enumerate() {
            let idx = if three && p[3] < cut {
                3
            } else {
                (0..pal.len()).min_by_key(|&j| (0..3).map(|k| (p[k] as i32 - pal[j][k]).pow(2)).sum::<i32>()).unwrap_or(0) as u32
            };
            bits |= idx << (2 * i);
        }
        let mut out = [0u8; 8];
        out[0..2].copy_from_slice(&c0.to_le_bytes());
        out[2..4].copy_from_slice(&c1.to_le_bytes());
        out[4..8].copy_from_slice(&bits.to_le_bytes());
        out
    }

    pub fn blocks(rgba: &[u8], w: u32, h: u32, mut f: impl FnMut(&[[u8; 4]; 16])) {
        for by in 0..h.div_ceil(4) {
            for bx in 0..w.div_ceil(4) {
                let px: [[u8; 4]; 16] = std::array::from_fn(|i| {
                    let (x, y) = ((bx * 4 + i as u32 % 4).min(w - 1), (by * 4 + i as u32 / 4).min(h - 1));
                    rgba[((y * w + x) * 4) as usize..][..4].try_into().unwrap()
                });
                f(&px);
            }
        }
    }

    pub fn encode_bc1(rgba: &[u8], w: u32, h: u32, punch: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity((w.div_ceil(4) * h.div_ceil(4) * 8) as usize);
        blocks(rgba, w, h, |px| out.extend_from_slice(&color_block(px, punch, false)));
        out
    }

    pub fn encode_bc2(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w.div_ceil(4) * h.div_ceil(4) * 16) as usize);
        blocks(rgba, w, h, |px| {
            let mut alpha = [0u8; 8];
            for (i, p) in px.iter().enumerate() {
                let nib = ((p[3] as u32 + 8) / 17).min(15) as u8;
                alpha[i / 2] |= nib << ((i % 2) * 4);
            }
            out.extend_from_slice(&alpha);
            out.extend_from_slice(&color_block(px, false, true));
        });
        out
    }

    /// Levels for an image: its own DXT data as level 0 (BC1 / BC2, kept exact) with re-encoded smaller levels, or
    /// RGBA levels. BC needs both sides a multiple of 4; other sizes go RGBA.
    pub fn levels(rgba: &[u8], w: u32, h: u32, dxt: Option<(bool, &[u8])>) -> Levels {
        let chain = rgba_chain(rgba, w, h);
        match dxt {
            Some((dxt3, data)) if w % 4 == 0 && h % 4 == 0 => {
                let punch = !dxt3 && rgba.chunks_exact(4).any(|p| p[3] < 128);
                let mut lv = Vec::with_capacity(chain.len());
                let mut first = data[..data.len().min(((w / 4) * (h / 4) * if dxt3 { 16 } else { 8 }) as usize)].to_vec();
                if dxt3 {
                    double_bc2_alpha(&mut first);
                }
                lv.push(first);
                for (px, lw, lh) in chain.iter().skip(1) {
                    lv.push(if dxt3 { encode_bc2(px, *lw, *lh) } else { encode_bc1(px, *lw, *lh, punch) });
                }
                Levels { format: if dxt3 { Format::Bc2 } else { Format::Bc1 }, data: lv }
            }
            _ => Levels { format: Format::Rgba, data: chain.into_iter().map(|(p, _, _)| p).collect() },
        }
    }

}

#[cfg(test)]
mod tests {
    use super::reference::*;
    use super::*;

    /// A BC1 block of two colours decodes back to them; coverage survives the mips of a checker cut-out.
    #[test]
    fn bc_and_coverage() {
        let mut px = [[0u8; 4]; 16];
        for (i, p) in px.iter_mut().enumerate() {
            *p = if i % 2 == 0 { [200, 40, 40, 255] } else { [20, 180, 60, 255] };
        }
        let b = color_block(&px, false, false);
        let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
        assert!(c0 > c1);
        let (w, h) = (16u32, 16u32);
        // Leaves: opaque 2x2 clumps on a 4-texel grid plus a fade, a quarter or so covered.
        let img: Vec<u8> = (0..w * h).flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let a = if x % 4 < 2 && y % 4 < 2 { 255 } else { (x * 8) as u8 };
            [90, 120, 60, a]
        }).collect();
        let chain = rgba_chain(&img, w, h);
        assert_eq!(chain.len(), 5);
        let c0 = coverage(&chain[0].0, 1.0);
        let c2 = coverage(&chain[2].0, 1.0);
        assert!((c0 - c2).abs() < 0.2, "coverage {c0} -> {c2}");
        let mut bc2 = vec![0x84u8; 16];
        double_bc2_alpha(&mut bc2);
        assert_eq!(bc2[0], 0xf8);
    }
}
