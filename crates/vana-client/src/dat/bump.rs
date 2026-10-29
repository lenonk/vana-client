//! Bump maps for scenery (config 0017; research stored_settings_spec.md section 3): the Bp chunks (type 0x5d) of file
//! 39831 + zone (83991 + zone from 600; FUN_10171f00), each made a normal map (FUN_1001d270).

/// One bump map: the name's bytes 8..15 (matched against a model texture's, FUN_10039f80) and its normal map, RGBA.
pub struct BumpMap {
    pub key: [u8; 8],
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn file_id(zone: u32) -> u32 {
    if zone >= 600 { 83991 + zone } else { 39831 + zone }
}

/// The file's Bp chunks: "bp\0\0", u16 width @4, u16 height @6, name[16] @16, width x height heights @32.
pub fn parse(file: &[u8]) -> Vec<BumpMap> {
    let mut out = Vec::new();
    for c in super::chunks(file).unwrap_or_default() {
        let p = c.data;
        if c.kind != 0x5d || p.len() < 32 || &p[..2] != b"bp" {
            continue;
        }
        let (w, h) = (u16::from_le_bytes([p[4], p[5]]) as usize, u16::from_le_bytes([p[6], p[7]]) as usize);
        if w == 0 || h == 0 || p.len() < 32 + w * h {
            continue;
        }
        let mut key = [0u8; 8];
        key.copy_from_slice(&p[24..32]);
        out.push(BumpMap { key, width: w as u32, height: h as u32, rgba: normal_map(w, h, &p[32..32 + w * h]) });
    }
    out
}

/// FUN_1001d270: neighbours wrap; gy from the rows above and below, gx from the columns either side;
/// s = 127 / sqrt(gx² + gy² + 255²); R = gy s + 128, G = gx s + 128, B = 255 s + 128 (truncated), A 0.
pub fn normal_map(w: usize, h: usize, hts: &[u8]) -> Vec<u8> {
    let at = |x: usize, y: usize| hts[y * w + x] as f32;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let (yp, yn) = ((y + h - 1) % h, (y + 1) % h);
        for x in 0..w {
            let (xp, xn) = ((x + w - 1) % w, (x + 1) % w);
            let gy = 2.0 * (at(x, yp) - at(x, yn)) + at(xp, yp) + at(xn, yp) - at(xp, yn) - at(xn, yn);
            let gx = 2.0 * (at(xp, y) - at(xn, y)) + at(xp, yp) + at(xp, yn) - at(xn, yp) - at(xn, yn);
            let s = 127.0 / (gx * gx + gy * gy + 255.0 * 255.0).sqrt();
            out.extend([((gy * s) as i32 + 128) as u8, ((gx * s) as i32 + 128) as u8, (128 + (255.0 * s) as i32) as u8, 0]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// A flat height field points straight out: (128, 128, 255).
    #[test]
    fn flat_is_up() {
        let n = super::normal_map(4, 4, &[7u8; 16]);
        assert_eq!(&n[..4], &[128, 128, 255, 0]);
    }
}
