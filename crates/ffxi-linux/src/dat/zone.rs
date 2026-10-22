//! Zone scenery: MZB (0x1C, object placement), MMB (0x2E, meshes) and IMG (0x20, textures).
//! MMB/MZB payloads are obfuscated with two 256-byte key tables that live in FFXiMain.dll.

use super::{Chunk, chunks};
use anyhow::{Result, bail};
use md5::{Digest, Md5};

const KEY1_AT: usize = 0x4CE60;
const KEY2_AT: usize = 0x4D188;
const KEY1_MD5: &str = "0bfa75c0e9ff6ff8497b370024b3b148";
const KEY2_MD5: &str = "44b34e48298e775a58552e7d9a8f931c";

pub struct Keys {
    t1: [u8; 256],
    t2: [u8; 256],
}

impl Keys {
    pub fn from_ffximain(dll: &[u8]) -> Result<Self> {
        let table = |at: usize, want: &str| -> Result<[u8; 256]> {
            let t: [u8; 256] = dll.get(at..at + 256).and_then(|s| s.try_into().ok()).ok_or_else(|| anyhow::anyhow!("FFXiMain.dll too short"))?;
            if hex::encode(Md5::digest(t)) != want {
                bail!("model key table at {at:#x} does not match this FFXiMain.dll build");
            }
            Ok(t)
        };
        Ok(Self { t1: table(KEY1_AT, KEY1_MD5)?, t2: table(KEY2_AT, KEY2_MD5)? })
    }
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}
fn vec3_at(b: &[u8], o: usize) -> [f32; 3] {
    [f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8)]
}
fn name_at(b: &[u8], o: usize, len: usize) -> String {
    let s = &b[o..o + len];
    let end = s.iter().position(|&c| c == 0).unwrap_or(len);
    String::from_utf8_lossy(&s[..end]).trim_end().to_string()
}

/// Byte-wise key stream, then (when flagged 0xFFFF) a keyed swap of 8-byte blocks between halves.
pub fn decode_mmb(keys: &Keys, p: &mut [u8]) {
    let len = (p[0] as usize | (p[1] as usize) << 8 | (p[2] as usize) << 16).min(p.len());
    if p[3] >= 5 {
        let mut key = keys.t1[(p[5] ^ 0xF0) as usize] as u32;
        let mut counter = 0u32;
        for b in &mut p[8..len] {
            let x = ((key & 0xFF) << 8) | (key & 0xFF);
            counter += 1;
            key = key.wrapping_add(counter);
            *b ^= (x >> (key & 7)) as u8;
            counter += 1;
            key = key.wrapping_add(counter);
        }
    }
    if p[6] == 0xFF && p[7] == 0xFF {
        let mut key1 = (p[5] ^ 0xF0) as u32;
        let mut key2 = keys.t2[key1 as usize] as u32;
        let half = ((len - 8) & !0xF) / 2;
        for pos in (0..half).step_by(8) {
            if key2 & 1 != 0 {
                let (a, b) = (8 + pos, 8 + half + pos);
                for i in 0..8 {
                    p.swap(a + i, b + i);
                }
            }
            key1 = key1.wrapping_add(9);
            key2 = key2.wrapping_add(key1);
        }
    }
}

/// Inverts runs of 16..23 bytes chosen by the key; placement ids are additionally XORed with 0x55.
pub fn decode_mzb(keys: &Keys, p: &mut [u8]) -> Result<()> {
    if p[3] < 0x1B {
        return Ok(());
    }
    let len = p[0] as usize | (p[1] as usize) << 8 | (p[2] as usize) << 16;
    if len > p.len() {
        bail!("MZB length {len} exceeds its chunk");
    }
    let mut key = keys.t1[(p[7] ^ 0xFF) as usize] as u32;
    let mut counter = 0u32;
    let mut pos = 8;
    while pos < len {
        let run = (((key >> 4) & 7) + 16) as usize;
        if key & 1 != 0 && pos + run < len {
            for b in &mut p[pos..pos + run] {
                *b ^= 0xFF;
            }
        }
        counter += 1;
        key = key.wrapping_add(counter);
        pos += run;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// BGRA
    pub color: u32,
    pub uv: [f32; 2],
}

pub struct Model {
    pub texture: String,
    pub blending: u16,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub strip: bool,
}

pub struct Mmb {
    pub name: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub models: Vec<Model>,
}

pub fn parse_mmb(p: &[u8]) -> Result<Mmb> {
    if p.len() < 64 {
        bail!("MMB too short");
    }
    let is_mmb_id = &p[..3] == b"MMB";
    let list_type = p[3] & 0x7F;
    let wide_vertex = p[4] == 2;
    let triangle_list = (is_mmb_id && list_type == 0) || (!is_mmb_id && wide_vertex);

    let name = name_at(p, 16, 16);
    let pieces = u32_at(p, 32) as usize;
    let (min, max) = ([f32_at(p, 36), f32_at(p, 44), f32_at(p, 52)], [f32_at(p, 40), f32_at(p, 48), f32_at(p, 56)]);
    let first = u32_at(p, 60) as usize;

    let mut offsets = std::collections::VecDeque::new();
    if first == 0 {
        if pieces == 0 {
            offsets.push_back(64);
        } else {
            for i in 0..8 {
                let o = u32_at(p, 64 + i * 4) as usize;
                if o != 0 {
                    offsets.push_back(o);
                }
            }
        }
    } else {
        offsets.push_back(first);
        let mut at = 64;
        while at + 4 <= first {
            let o = u32_at(p, at) as usize;
            if o != 0 {
                offsets.push_back(o);
            }
            at += 4;
        }
    }

    let stride = if wide_vertex { 48 } else { 36 };
    let mut models = Vec::new();
    let mut at = 64;
    for _ in 0..pieces {
        if let Some(o) = offsets.pop_front() {
            at = o;
        }
        if at + 32 > p.len() {
            bail!("MMB {name}: piece header out of range");
        }
        let count = u32_at(p, at) as usize;
        at += 32;
        if count > 50 {
            bail!("MMB {name}: implausible model count {count}");
        }
        for _ in 0..count {
            if at + 20 > p.len() {
                break;
            }
            let texture = name_at(p, at, 16);
            let nverts = u16_at(p, at + 16) as usize;
            let blending = u16_at(p, at + 18);
            at += 20;
            if at + nverts * stride + 4 > p.len() {
                bail!("MMB {name}: vertices out of range");
            }
            let vertices = (0..nverts)
                .map(|i| {
                    let v = at + i * stride;
                    let n = if wide_vertex { v + 24 } else { v + 12 };
                    let c = n + 12;
                    Vertex { pos: vec3_at(p, v), normal: vec3_at(p, n), color: u32_at(p, c), uv: [f32_at(p, c + 4), f32_at(p, c + 8)] }
                })
                .collect();
            at += nverts * stride;
            let nidx = (u32_at(p, at) & 0xFFFF) as usize;
            at += 4;
            if at + nidx * 2 > p.len() {
                bail!("MMB {name}: indices out of range");
            }
            let indices = (0..nidx).map(|i| u16_at(p, at + i * 2)).collect();
            at += nidx * 2 + if nidx % 2 == 1 { 2 } else { 0 };
            models.push(Model { texture, blending, vertices, indices, strip: !triangle_list });
        }
    }
    Ok(Mmb { name, min, max, models })
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub name: String,
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
}

impl Placement {
    /// Column-major model matrix: scale, then rotate X then Y then Z, then translate.
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        let [rx, ry, rz] = self.rotate;
        let (sx, cx, sy, cy, sz, cz) = (rx.sin(), rx.cos(), ry.sin(), ry.cos(), rz.sin(), rz.cos());
        let [kx, ky, kz] = self.scale;
        let [tx, ty, tz] = self.translate;
        [
            [cy * cz * kx, cy * sz * kx, -sy * kx, 0.0],
            [(cz * sx * sy - cx * sz) * ky, (cx * cz + sx * sy * sz) * ky, cy * sx * ky, 0.0],
            [(cx * cz * sy + sx * sz) * kz, (-cz * sx + cx * sy * sz) * kz, cx * cy * kz, 0.0],
            [tx, ty, tz, 1.0],
        ]
    }
}

pub fn parse_mzb(p: &mut [u8]) -> Result<Vec<Placement>> {
    let count = (p[4] as usize) | (p[5] as usize) << 8 | (p[6] as usize) << 16;
    let end = u32_at(p, 20) as usize;
    let stride = [100usize, 84, 92].into_iter().find(|s| count * s + 32 == end).ok_or_else(|| anyhow::anyhow!("unknown MZB record size ({count} records end at {end})"))?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let r = 32 + i * stride;
        for b in &mut p[r..r + 16] {
            *b ^= 0x55;
        }
        out.push(Placement { name: name_at(p, r, 16), translate: vec3_at(p, r + 16), rotate: vec3_at(p, r + 28), scale: vec3_at(p, r + 40) });
    }
    Ok(out)
}

#[derive(Clone)]
pub struct Texture {
    pub name: String,
    pub format: u8,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The DXT blocks as stored (true = DXT3, its alpha doubled as the RGBA's is): the GPU's level 0.
    pub dxt: Option<(bool, Vec<u8>)>,
}

impl Texture {
    pub fn new(name: String, format: u8, width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Texture { name, format, width, height, rgba, dxt: None }
    }
}

fn rgb565(c: u16) -> [u32; 3] {
    let (r, g, b) = ((c >> 11) as u32 & 31, (c >> 5) as u32 & 63, c as u32 & 31);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// BC1 (DXT1) or BC2 (DXT3) blocks to RGBA8.
pub(crate) fn decode_dxt(data: &[u8], w: u32, h: u32, dxt3: bool) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let block = if dxt3 { 16 } else { 8 };
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    for by in 0..bh {
        for bx in 0..bw {
            let at = ((by * bw + bx) as usize) * block;
            if at + block > data.len() {
                return out;
            }
            let (alpha, color) = if dxt3 { (Some(&data[at..at + 8]), &data[at + 8..at + 16]) } else { (None, &data[at..at + 8]) };
            let (c0, c1) = (u16_at(color, 0), u16_at(color, 2));
            let (a, b) = (rgb565(c0), rgb565(c1));
            let mut palette = [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [0; 4], [0; 4]];
            if c0 > c1 || dxt3 {
                for i in 0..3 {
                    palette[2][i] = (2 * a[i] + b[i]) / 3;
                    palette[3][i] = (a[i] + 2 * b[i]) / 3;
                }
                palette[2][3] = 255;
                palette[3][3] = 255;
            } else {
                for i in 0..3 {
                    palette[2][i] = (a[i] + b[i]) / 2;
                }
                palette[2][3] = 255;
            }
            let bits = u32_at(color, 4);
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= w || y >= h {
                        continue;
                    }
                    let i = py * 4 + px;
                    let mut c = palette[((bits >> (i * 2)) & 3) as usize];
                    if let Some(alpha) = alpha {
                        let nib = (alpha[(i / 2) as usize] >> ((i % 2) * 4)) & 0xF;
                        c[3] = (nib * 17) as u32;
                    }
                    let o = ((y * w + x) * 4) as usize;
                    for k in 0..4 {
                        out[o + k] = c[k] as u8;
                    }
                }
            }
        }
    }
    out
}

/// IMG chunk: 57-byte header (flag, 16-char name, ..., width @21, height @25), then
/// either DXT data or a BGRA palette plus 8-bit indices stored bottom-up.
pub fn parse_img(p: &[u8]) -> Result<Texture> {
    if p.len() < 57 {
        bail!("IMG too short");
    }
    let flag = p[0];
    let name = name_at(p, 1, 16);
    let (w, h) = (u32_at(p, 21), u32_at(p, 25));
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        bail!("IMG {name}: bad size {w}x{h}");
    }
    let paletted = |pal_at: usize| -> Result<Vec<u8>> {
        let px_at = pal_at + 1024;
        if px_at + (w * h) as usize > p.len() {
            bail!("IMG {name}: pixels out of range");
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = p[px_at + ((h - 1 - y) * w + x) as usize] as usize;
                let bgra = &p[pal_at + idx * 4..pal_at + idx * 4 + 4];
                let o = ((y * w + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
            }
        }
        Ok(out)
    };
    let mut half_alpha = true;
    let mut dxt = None;
    let mut rgba = match flag {
        0xA1 => {
            let kind = &p[57..61];
            let size = u32_at(p, 61) as usize;
            let data = p.get(69..69 + size).ok_or_else(|| anyhow::anyhow!("IMG {name}: DXT data out of range"))?;
            let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
            match kind {
                b"1TXD" => {
                    half_alpha = false;
                    dxt = (data.len() >= blocks * 8).then(|| (false, data[..blocks * 8].to_vec()));
                    decode_dxt(data, w, h, false)
                }
                b"3TXD" => {
                    dxt = (data.len() >= blocks * 16).then(|| {
                        let mut d = data[..blocks * 16].to_vec();
                        super::texmip::double_bc2_alpha(&mut d);
                        (true, d)
                    });
                    decode_dxt(data, w, h, true)
                }
                other => bail!("IMG {name}: unsupported compression {:?}", String::from_utf8_lossy(other)),
            }
        }
        0x01 | 0x81 | 0x91 => paletted(57)?,
        0xB1 => paletted(61)?,
        other => bail!("IMG {name}: unknown format flag {other:#04x}"),
    };
    // Palette and DXT3 alpha is PS2-style: 0x80 is opaque.
    if half_alpha {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = px[3].saturating_mul(2);
        }
    }
    Ok(Texture { name, format: flag, width: w, height: h, rgba, dxt })
}

/// A RID region: an oriented box. Names starting with 'z' are zone lines, '_' doors, 'E' events.
#[derive(Clone, Debug)]
pub struct Region {
    pub name: String,
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    pub scale: [f32; 3],
}

impl Region {
    pub fn contains(&self, p: [f32; 3]) -> bool {
        let (dx, dy, dz) = (p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]);
        let (s, c) = (-self.rot[1]).sin_cos();
        let (lx, lz) = (dx * c + dz * s, -dx * s + dz * c);
        lx.abs() <= self.scale[0] / 2.0 && dy.abs() <= self.scale[1] / 2.0 && lz.abs() <= self.scale[2] / 2.0
    }

    /// The 4-byte tag the server knows a zone line by.
    pub fn tag(&self) -> u32 {
        let mut b = [0u8; 4];
        for (i, c) in self.name.bytes().take(4).enumerate() {
            b[i] = c;
        }
        u32::from_le_bytes(b)
    }
}

/// RID chunk (0x36): count @0x30, 0x40-byte entries from 0x40 (pos, rot, scale, name[8] @0x24).
pub fn parse_rid(p: &[u8]) -> Vec<Region> {
    let count = p.get(0x30..0x34).map(|_| u32_at(p, 0x30) as usize).unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let e = 0x40 + i * 0x40;
            (e + 0x2C <= p.len()).then(|| Region { pos: vec3_at(p, e), rot: vec3_at(p, e + 12), scale: vec3_at(p, e + 24), name: name_at(p, e + 0x24, 8) })
        })
        .collect()
}

pub struct ZoneScene {
    pub regions: Vec<Region>,
    pub collision: Option<super::collision::Collision>,
    pub placements: Vec<Placement>,
    pub meshes: Vec<Mmb>,
    pub textures: Vec<Texture>,
    pub warnings: Vec<String>,
}

pub fn load_scene(keys: &Keys, file: &[u8]) -> Result<ZoneScene> {
    let mut scene = ZoneScene { regions: Vec::new(), collision: None, placements: Vec::new(), meshes: Vec::new(), textures: Vec::new(), warnings: Vec::new() };
    let list: Vec<Chunk> = chunks(file)?;
    for c in list {
        let mut data = c.data.to_vec();
        let result = match c.kind {
            0x1C => decode_mzb(keys, &mut data).and_then(|_| parse_mzb(&mut data)).map(|p| {
                scene.placements.extend(p);
                match super::collision::parse(&data) {
                    Ok(c) => scene.collision = Some(c),
                    Err(e) => scene.warnings.push(format!("collision: {e}")),
                }
            }),
            0x2E => {
                decode_mmb(keys, &mut data);
                parse_mmb(&data).map(|m| scene.meshes.push(m))
            }
            0x20 => parse_img(&data).map(|t| scene.textures.push(t)),
            0x36 => {
                scene.regions.extend(parse_rid(&data));
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            scene.warnings.push(format!("chunk {} at {:#x}: {e}", c.name(), c.offset));
        }
    }
    Ok(scene)
}
