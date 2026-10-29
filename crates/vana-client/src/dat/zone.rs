//! Zone scenery: MZB (0x1C, object placement), MMB (0x2E, meshes) and IMG (0x20, textures).
//! MMB/MZB payloads are obfuscated with two 256-byte key tables that live in FFXiMain.dll.

use super::{Chunk, chunks};
use anyhow::{Result, bail};
use md5::{Digest, Md5};

const KEY1_MD5: &str = "0bfa75c0e9ff6ff8497b370024b3b148";
const KEY2_MD5: &str = "44b34e48298e775a58552e7d9a8f931c";

/// File id of a zone's scene DAT (FUN_10171f00): 100 + zone, from zone 600 on zone + 0x144f7.
pub fn file_id(zone: u32) -> u32 {
    if zone >= 600 { zone + 0x144f7 } else { zone + 100 }
}

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
        let t = super::tables::of(dll)?;
        Ok(Self { t1: table(t.key1_at, KEY1_MD5)?, t2: table(t.key2_at, KEY2_MD5)? })
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

/// Inverts runs of 16..23 bytes chosen by the key (versions >= 0x1b; parse_mzb XORs their names with 0x55).
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
    /// An animated mesh's (type 2) sway offset at full swing (0011; zero otherwise).
    pub delta: [f32; 3],
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
    /// Header byte 4 = 2: an animated mesh whose vertices sway along their delta (FUN_10168eb0, config 0011).
    pub animated: bool,
    /// Header byte 4: 1 for the strip meshes bump mapping may turn into type 3 (config 0017).
    pub mesh_type: u8,
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
                    let delta = if wide_vertex { vec3_at(p, v + 12) } else { [0.0; 3] };
                    Vertex { pos: vec3_at(p, v), normal: vec3_at(p, n), color: u32_at(p, c), uv: [f32_at(p, c + 4), f32_at(p, c + 8)], delta }
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
    Ok(Mmb { name, min, max, models, animated: wide_vertex, mesh_type: p[4] })
}

/// The 20 weather tags FUN_10160700 gathers sway curves for, in its order.
pub const SWAY_TAGS: [&str; 20] = ["fine", "suny", "clod", "mist", "dryw", "heat", "rain", "sqal", "dust", "sand", "wind", "stom", "snow", "bliz", "thdr", "bolt", "aura", "ligt", "fogd", "dark"];

/// The environment-animation curves (FUN_10160830): per weather tag the type-0x19 chunks of `weat/<tag>/kfrm`, else
/// of `mode/kfrm`, at most 32 each; a curve is its (t, value) pairs up to the one at t = 1.
#[derive(Clone, Debug, Default)]
pub struct SwayCurves {
    pub by_tag: Vec<Vec<Vec<[f32; 2]>>>,
}

pub fn sway_curves(file: &[u8]) -> SwayCurves {
    let parse = |d: &[u8]| -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        for k in (0..d.len().saturating_sub(7)).step_by(8) {
            let pair = [f32_at(d, k), f32_at(d, k + 4)];
            out.push(pair);
            if pair[0] >= 1.0 {
                break;
            }
        }
        out
    };
    let (mut weat, mut mode): (std::collections::HashMap<String, Vec<Vec<[f32; 2]>>>, Vec<Vec<[f32; 2]>>) = (Default::default(), Vec::new());
    let mut stack: Vec<String> = Vec::new();
    for c in chunks(file).unwrap_or_default() {
        match c.kind {
            0x01 => stack.push(c.name()),
            0x00 => {
                stack.pop();
            }
            0x19 => {
                let n = stack.len();
                if n >= 3 && stack[n - 1] == "kfrm" && stack[n - 3] == "weat" {
                    weat.entry(stack[n - 2].clone()).or_default().push(parse(c.data));
                } else if n >= 2 && stack[n - 1] == "kfrm" && stack[n - 2] == "mode" {
                    mode.push(parse(c.data));
                }
            }
            _ => {}
        }
    }
    let pick = |v: &[Vec<[f32; 2]>]| v.iter().take(32).cloned().collect::<Vec<_>>();
    SwayCurves { by_tag: SWAY_TAGS.iter().map(|t| weat.get(*t).filter(|v| !v.is_empty()).map_or_else(|| pick(&mode), |v| pick(v))).collect() }
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub name: String,
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
    /// Record +0x54..+0x60 (version >= 0x12): 1-based refs into the MZB light list, as the keys they name (0 none).
    pub lights: [u32; 4],
    /// Record +0x50: the sub-area this placement stands in for (hidden while that one is loaded, FUN_1017a630).
    pub link: u32,
    /// Record +0x4c: the area (environment) 4cc it is lit by (inst+0xdc / +0xe0, FUN_10186cb0); 0 = the zone's.
    pub area: u32,
    /// Record +0x34: a group id (doors '_', platforms '@'; FUN_1017a950); placements with another nonzero id are
    /// not drawn by the scenery pass.
    pub id: u32,
    /// Record +0x38 / +0x3c: level-of-detail distances (high within the first, low beyond the larger; FUN_1017c9c0).
    pub lod: [f32; 2],
    /// From an MZB whose byte 0x1c is 2 (scene slot type 3, FUN_10179430): a voyage zone's world, drawn through the
    /// voyage matrix while the voyage runs (FUN_1017b510).
    pub world: bool,
    /// Record +0x40 when +0x46 bit 0 is set: its own draw distance, scaled by the Clipping factor instead of the
    /// zone's (FUN_1017c9c0).
    pub own_dist: Option<f32>,
}

impl Placement {
    /// The scenery pass draws it (FUN_1017b510 skips nonzero ids; '_' / '@' ids are drawn as door groups).
    pub fn drawn(&self) -> bool {
        self.id == 0 || matches!(self.id as u8, b'_' | b'@')
    }

    /// FUN_1017afe0: the low / medium / high meshes by name: a name ending in h / m / l looks for its siblings
    /// (high = name+h, else name+m, else itself; medium = name+m, else itself; low = name+l, else name+m, else itself).
    pub fn lod_names<'a>(&'a self, has: impl Fn(&str) -> bool) -> [std::borrow::Cow<'a, str>; 3] {
        use std::borrow::Cow;
        let exact = || Cow::Borrowed(self.name.as_str());
        let n = self.name.len();
        let last = self.name.as_bytes().last().copied().unwrap_or(0);
        if n < 2 || !matches!(last, b'h' | b'm' | b'l') {
            return [exact(), exact(), exact()];
        }
        let base = &self.name[..n - 1];
        let sib = |c: char| Some(format!("{base}{c}")).filter(|s| has(s)).map(Cow::Owned);
        let m = sib('m');
        let h = sib('h').or_else(|| m.clone()).unwrap_or_else(exact);
        let l = sib('l').or_else(|| m.clone()).unwrap_or_else(exact);
        [l, m.unwrap_or_else(exact), h]
    }
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

/// The camera lists (FUN_1017af30; potentially visible sets): version byte 3 >= 0x15 and u32 @0x14: u32 n, then n x
/// { u32 count; u32 placement[count] }, each by its MZB offset, which a collision object's +0xa8 names (a probe from
/// the camera eye picks one, FUN_101813f0). Placement indices are 0-based into the MZB's records.
pub fn camera_lists(p: &[u8]) -> std::collections::HashMap<u32, Vec<u32>> {
    let mut out = std::collections::HashMap::new();
    let pool = if p.len() >= 0x18 && p[3] >= 0x15 { u32_at(p, 0x14) as usize } else { 0 };
    if pool == 0 || pool + 4 > p.len() {
        return out;
    }
    let mut at = pool + 4;
    for _ in 0..u32_at(p, pool) {
        let Some(n) = p.get(at..at + 4).map(|_| u32_at(p, at) as usize) else { break };
        if at + 4 + n * 4 > p.len() {
            break;
        }
        out.insert(at as u32, (0..n).map(|k| u32_at(p, at + 4 + k * 4)).collect());
        at += 4 + n * 4;
    }
    out
}

/// FUN_10172530: version byte 3 > 0x11 and u32 @0x18: the light list, 256 records of 0x4c with the key at +0
/// (empty without one).
pub fn light_list(p: &[u8]) -> Vec<u32> {
    let list = u32_at(p, 0x18) as usize;
    if p.len() < 0x1c || p[3] <= 0x11 || list == 0 || list + 256 * 0x4c > p.len() {
        return Vec::new();
    }
    (0..256).map(|k| u32_at(p, list + k * 0x4c)).collect()
}

pub fn parse_mzb(p: &mut [u8]) -> Result<Vec<Placement>> {
    let count = (p[4] as usize) | (p[5] as usize) << 8 | (p[6] as usize) << 16;
    let end = u32_at(p, 20) as usize;
    let stride = [100usize, 84, 92].into_iter().find(|s| count * s + 32 == end).ok_or_else(|| anyhow::anyhow!("unknown MZB record size ({count} records end at {end})"))?;
    let version = p[3];
    let keys = light_list(p);
    let world = p.get(0x1c) == Some(&2);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let r = 32 + i * stride;
        // FUN_10179430: names are XORed only in the encrypted versions (> 0x1a; e.g. Lower Jeuno's 464 is 0x1a, plain).
        if version > 0x1a {
            for b in &mut p[r..r + 16] {
                *b ^= 0x55;
            }
        }
        let mut lights = [0u32; 4];
        if stride == 100 && version >= 0x12 {
            for (k, l) in lights.iter_mut().enumerate() {
                let rf = u32_at(p, r + 0x54 + 4 * k) as usize;
                *l = if rf >= 1 { keys.get(rf - 1).copied().unwrap_or(0) } else { 0 };
            }
        }
        out.push(Placement { name: name_at(p, r, 16), translate: vec3_at(p, r + 16), rotate: vec3_at(p, r + 28), scale: vec3_at(p, r + 40), lights, link: u32_at(p, r + 0x50), area: u32_at(p, r + 0x4c), id: u32_at(p, r + 0x34), lod: [f32_at(p, r + 0x38), f32_at(p, r + 0x3c)], world, own_dist: (p[r + 0x46] & 1 != 0).then(|| f32_at(p, r + 0x40)) });
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

/// IMG chunk: 57-byte header (flag, 16-char name, then a BITMAPINFOHEADER: width @21, height @25, bit count @31),
/// then DXT data, or rows bottom-up of 4- or 8-bit palette indices (a BGRA palette first) or 24- / 32-bit BGR(A).
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
    let bits = u16::from_le_bytes([p[31], p[32]]);
    // FUN_10039e50's source cases: 4 (two pixels a byte, high nibble first, 16 colours), 8, 24 (alpha 0x80), 32.
    let uncompressed = |at: usize| -> Result<Vec<u8>> {
        let (pal, row_bytes) = match bits {
            4 => (64, w.div_ceil(2) as usize),
            8 => (1024, w as usize),
            24 => (0, w as usize * 3),
            32 => (0, w as usize * 4),
            other => bail!("IMG {name}: {other}-bit pixels"),
        };
        let px_at = at + pal;
        if (bits == 4 && w % 2 != 0) || px_at + row_bytes * h as usize > p.len() {
            bail!("IMG {name}: pixels out of range");
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((h - 1 - y) * w + x) as usize;
                let bgra: [u8; 4] = match bits {
                    4 | 8 => {
                        let idx = if bits == 4 { (p[px_at + i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 0xf } else { p[px_at + i] } as usize;
                        p[at + idx * 4..at + idx * 4 + 4].try_into().unwrap()
                    }
                    24 => [p[px_at + i * 3], p[px_at + i * 3 + 1], p[px_at + i * 3 + 2], 0x80],
                    _ => p[px_at + i * 4..px_at + i * 4 + 4].try_into().unwrap(),
                };
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
            if p.len() < 69 {
                bail!("IMG {name}: DXT header cut short");
            }
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
        0x01 | 0x81 | 0x91 => uncompressed(57)?,
        0xB1 => uncompressed(61)?,
        other => bail!("IMG {name}: unknown format flag {other:#04x}"),
    };
    // Uncompressed and DXT3 alpha is PS2-style: 0x80 is opaque.
    if half_alpha {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = px[3].saturating_mul(2);
        }
    }
    Ok(Texture { name, format: flag, width: w, height: h, rgba, dxt })
}

/// A RID region: an oriented box. Names starting with 'z' are zone lines, '_' doors, 'E' events, 'm' / 'M'
/// sub-area triggers.
#[derive(Clone, Debug)]
pub struct Region {
    pub name: String,
    pub pos: [f32; 3],
    /// Only the yaw (rot[1]) is used; +0x0C is the rect class, see `class`.
    pub rot: [f32; 3],
    pub scale: [f32; 3],
    /// +0x0C as a u32: sub-area triggers are hit-tested only with class 0 (FUN_10173c10).
    pub class: u32,
    /// +0x2C: a sub-area trigger's sub-area id (0 unloads the current one).
    pub param: u32,
    /// +0x30: a lift's surface type in bits 4..7 (FUN_10173c10 -> +0xec).
    pub flags: u32,
    /// +0x34 / +0x36: a lift's floors 0 / 1 as pos.y + v / 256 (-> +0xb8 / +0xbc).
    pub floors: [i16; 2],
}

impl Region {
    /// FUN_101760f0: inside the box T(-pos) RotY(-yaw) S(1 / |size|) maps to [-0.5, 0.5]^3.
    pub fn contains(&self, p: [f32; 3]) -> bool {
        let (dx, dy, dz) = (p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]);
        let (s, c) = (-self.rot[1]).sin_cos();
        let (lx, lz) = (dx * c + dz * s, -dx * s + dz * c);
        lx.abs() <= self.scale[0].abs() / 2.0 && dy.abs() <= self.scale[1].abs() / 2.0 && lz.abs() <= self.scale[2].abs() / 2.0
    }

    /// FUN_10175910: the segment a..b touches the box: an end inside, or it crosses a face.
    pub fn touches(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let (la, lb) = (self.local(a), self.local(b));
        if [la, lb].iter().any(|p| p.iter().all(|v| v.abs() <= 0.5)) {
            return true;
        }
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for i in 0..3 {
            let d = lb[i] - la[i];
            if d.abs() < 1e-9 {
                if la[i].abs() > 0.5 {
                    return false;
                }
                continue;
            }
            let (u, v) = ((-0.5 - la[i]) / d, (0.5 - la[i]) / d);
            (t0, t1) = (t0.max(u.min(v)), t1.min(u.max(v)));
        }
        t0 <= t1
    }

    /// A point in the box's unit space (the box is [-0.5, 0.5]^3 there).
    fn local(&self, p: [f32; 3]) -> [f32; 3] {
        let (dx, dy, dz) = (p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]);
        let (s, c) = (-self.rot[1]).sin_cos();
        [(dx * c + dz * s) / self.scale[0].abs(), dy / self.scale[1].abs(), (-dx * s + dz * c) / self.scale[2].abs()]
    }

    /// A lift shaft ('@', collision_actors_spec 2.1).
    pub fn is_lift(&self) -> bool {
        self.name.starts_with('@')
    }

    /// A lift floor's height (y down: floor 1 is usually the upper one).
    pub fn floor_y(&self, i: usize) -> f32 {
        self.pos[1] + self.floors[i.min(1)] as f32 / 256.0
    }

    /// A lift's surface type for footsteps: +0x30 bits 4..7, sign-extended.
    pub fn surface(&self) -> i8 {
        (((self.flags << 24) as i32) >> 28) as i8
    }

    /// An 'm' / 'M' record.
    pub fn is_sub_area(&self) -> bool {
        self.name.starts_with(['m', 'M'])
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
            (e + 0x38 <= p.len()).then(|| Region {
                pos: vec3_at(p, e),
                rot: vec3_at(p, e + 12),
                scale: vec3_at(p, e + 24),
                name: name_at(p, e + 0x24, 8),
                class: u32_at(p, e + 12),
                param: u32_at(p, e + 0x2C),
                flags: u32_at(p, e + 0x30),
                floors: [0x34, 0x36].map(|o| i16::from_le_bytes([p[e + o], p[e + o + 1]])),
            })
        })
        .collect()
}

/// FUN_101760f0 over the kept sub-area triggers (class 0, list order): the first one holding `p`, its id.
pub fn sub_area_at(regions: &[Region], p: [f32; 3]) -> Option<u32> {
    regions.iter().filter(|r| r.is_sub_area() && r.class == 0).find(|r| r.contains(p)).map(|r| r.param)
}

/// FUN_101779b0: some 'm' / 'M' record (any class) names sub-area `id`.
pub fn declares_sub_area(regions: &[Region], id: u32) -> bool {
    regions.iter().any(|r| r.is_sub_area() && r.param == id)
}

pub struct ZoneScene {
    pub regions: Vec<Region>,
    pub collision: Option<super::collision::Collision>,
    pub placements: Vec<Placement>,
    pub meshes: Vec<Mmb>,
    pub textures: Vec<Texture>,
    pub warnings: Vec<String>,
    /// The light table's keys (every MZB light list's, once each; slot = index, FUN_101737d0).
    pub light_keys: Vec<u32>,
    /// The voyage paths `c000`..`c003` (type-6 chunks, the first of each name; FUN_10055040), payloads.
    pub paths: [Option<Vec<u8>>; 4],
    /// The MZB's camera lists by offset (camera_lists), as indices into `placements`.
    pub camera_lists: std::collections::HashMap<u32, Vec<u32>>,
    /// The animated meshes' curves per weather tag (sway_curves).
    pub sway: SwayCurves,
    /// The zone's bump maps (config 0017, dat/bump.rs), put in by the loader when bump mapping is on.
    pub bumps: Vec<super::bump::BumpMap>,
}

pub fn load_scene(keys: &Keys, file: &[u8]) -> Result<ZoneScene> {
    let mut scene = ZoneScene { regions: Vec::new(), collision: None, placements: Vec::new(), meshes: Vec::new(), textures: Vec::new(), warnings: Vec::new(), light_keys: Vec::new(), paths: Default::default(), camera_lists: Default::default(), sway: sway_curves(file), bumps: Vec::new() };
    let list: Vec<Chunk> = chunks(file)?;
    for c in list {
        let mut data = c.data.to_vec();
        let result = match c.kind {
            0x1C => decode_mzb(keys, &mut data).and_then(|_| parse_mzb(&mut data)).map(|p| {
                // As the collision, the last MZB's.
                let base = scene.placements.len() as u32;
                scene.camera_lists = camera_lists(&data).into_iter().map(|(at, l)| (at, l.into_iter().map(|i| i + base).collect())).collect();
                scene.placements.extend(p);
                for k in light_list(&data) {
                    if k != 0 && !scene.light_keys.contains(&k) {
                        scene.light_keys.push(k);
                    }
                }
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
            0x06 if c.name[..3] == *b"c00" && (b'0'..=b'3').contains(&c.name[3]) => {
                let k = (c.name[3] - b'0') as usize;
                scene.paths[k].get_or_insert_with(|| c.data.to_vec());
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            scene.warnings.push(format!("chunk {} at {:#x}: {e}", c.name(), c.offset));
        }
    }
    // Door boxes go into the grid once both the collision and the RID records are in.
    if let Some(c) = scene.collision.as_mut() {
        c.add_door_boxes(&scene.regions);
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(flag: u8, w: u32, h: u32, bits: u16, extra: usize) -> Vec<u8> {
        let mut p = vec![0u8; 57 + extra];
        p[0] = flag;
        p[1..17].copy_from_slice(b"test    img     ");
        p[17] = 0x28;
        p[21..25].copy_from_slice(&w.to_le_bytes());
        p[25..29].copy_from_slice(&h.to_le_bytes());
        p[29] = 1;
        p[31..33].copy_from_slice(&bits.to_le_bytes());
        p
    }

    #[test]
    fn img_4bit_and_32bit() {
        // 2 x 2, 16 colours: rows bottom-up, the high nibble first; alpha doubled.
        let mut p = header(0xB1, 2, 2, 4, 4);
        let mut pal = vec![0u8; 64];
        pal[4..8].copy_from_slice(&[10, 20, 30, 0x80]);
        pal[8..12].copy_from_slice(&[40, 50, 60, 0x40]);
        p.extend(pal);
        p.extend([0x12, 0x21]);
        let t = parse_img(&p).unwrap();
        assert_eq!(t.rgba, [60, 50, 40, 0x80, 30, 20, 10, 255, 30, 20, 10, 255, 60, 50, 40, 0x80]);
        // 1 x 2 BGRA.
        let mut p = header(0x81, 1, 2, 32, 0);
        p.extend([1, 2, 3, 0x40, 4, 5, 6, 0x80]);
        assert_eq!(parse_img(&p).unwrap().rgba, [6, 5, 4, 255, 3, 2, 1, 0x80]);
    }

    #[test]
    fn zone_line_segments() {
        // Ru'Lude Gardens' Mog House lift: the box under the floor, our place at the button, the event's pose below.
        let r = Region { name: "zmrmzmrn".into(), pos: [48.989, 17.19, -79.994], rot: [0.0, 3.1416, 0.0], scale: [12.0, 10.0, 10.0], class: 0, param: 0, flags: 0, floors: [0, 0] };
        let (button, down) = ([49.0, 9.65, -73.0], [51.531, 14.999, -77.941]);
        assert!(!r.touches(button, button));
        assert!(r.touches(button, down) && r.touches(down, down));
        // Through the box with both ends outside, and beside it.
        assert!(r.touches([40.0, 17.0, -80.0], [58.0, 17.0, -80.0]));
        assert!(!r.touches([40.0, 17.0, -70.0], [58.0, 17.0, -70.0]));
    }
}
