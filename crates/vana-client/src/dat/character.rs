//! Character models: skeleton (0x29), skinned mesh (0x2A), animation (0x2B) and textures (0x20).

use super::{DatIndex, chunks};
use super::zone::{Texture, parse_img};
use anyhow::{Result, bail};
use glam::{Quat, Vec3};

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}
fn vec3_at(b: &[u8], o: usize) -> Vec3 {
    Vec3::new(f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8))
}
fn name_at(b: &[u8], o: usize, len: usize) -> String {
    let s = &b[o..o + len];
    let end = s.iter().position(|&c| c == 0).unwrap_or(len);
    String::from_utf8_lossy(&s[..end]).trim_end().to_string()
}

#[derive(Clone, Debug)]
pub struct Bone {
    /// Unused for bone 0.
    pub parent: usize,
    pub term: u8,
    /// Rest rotation and translation relative to the parent.
    pub rot: Quat,
    pub pos: Vec3,
}

#[derive(Clone, Debug)]
pub struct Skeleton {
    pub name: String,
    pub bones: Vec<Bone>,
    /// Attach points after the bones (FUN_100343f0): (bone, translation); point 2 anchors the name plate and
    /// the target arrow (actor +0x8a6 / +0x8a7 default 2).
    pub points: Vec<(u16, Vec3)>,
    /// Each point's rotation (x, y, z radians), for the bone-frame attach modes 9..0xc.
    pub point_rots: Vec<Vec3>,
    /// Head look limits (FUN_10035270): {horizontal, vertical, depth} for the bones of points 3 and 7, at the end
    /// of the point table + 0x48.
    pub look_limits: [[f32; 3]; 2],
    /// Bounding boxes (FUN_100cfb40) at the end of the point table: 0 standing, 1 engaged / sitting / riding, 2
    /// dead; six floats each (y b0..b1, x +-b2 / b3, z b4..b5).
    pub boxes: Vec<[f32; 6]>,
}

/// u16 ?, u16 bone count, then 30-byte bones: parent u8, term u8, quat xyzw, translation xyz.
pub fn parse_skeleton(name: String, d: &[u8]) -> Result<Skeleton> {
    let n = u16_at(d, 2) as usize;
    if 4 + n * 30 > d.len() {
        bail!("skeleton {name}: {n} bones overrun the chunk");
    }
    let bones = (0..n)
        .map(|i| {
            let o = 4 + i * 30;
            Bone {
                parent: if i == 0 { 0 } else { d[o] as usize },
                term: d[o + 1],
                rot: Quat::from_xyzw(f32_at(d, o + 2), f32_at(d, o + 6), f32_at(d, o + 10), f32_at(d, o + 14)),
                pos: vec3_at(d, o + 18),
            }
        })
        .collect::<Vec<_>>();
    if bones.iter().enumerate().skip(1).any(|(i, b)| b.parent >= i) {
        bail!("skeleton {name}: a parent does not precede its child");
    }
    // u16 count, u16 0xffff, then 26-byte points {i16 bone, f32 rotation xyz, f32 translation xyz}.
    let at = 4 + n * 30;
    let count = if at + 4 <= d.len() { u16_at(d, at) as usize } else { 0 };
    let points = (0..count).map_while(|i| {
        let o = at + 4 + i * 26;
        (o + 26 <= d.len()).then(|| (u16_at(d, o), vec3_at(d, o + 14)))
    }).collect();
    let point_rots = (0..count).map_while(|i| {
        let o = at + 4 + i * 26;
        (o + 26 <= d.len()).then(|| vec3_at(d, o + 2))
    }).collect();
    let lim = at + 4 + count * 26 + 0x48;
    let look_limits = [0, 1].map(|i| [0, 1, 2].map(|k| if lim + 12 * i + 4 * k + 4 <= d.len() { f32_at(d, lim + 12 * i + 4 * k) } else { 0.0 }));
    let at_box = at + 4 + count * 26;
    let boxes = (0..3).take_while(|i| at_box + 0x18 * i + 0x18 <= d.len()).map(|i| std::array::from_fn(|k| f32_at(d, at_box + 0x18 * i + 4 * k))).collect();
    Ok(Skeleton { name, bones, points, point_rots, look_limits, boxes })
}

/// One bone's share of a vertex. `pos`/`normal` are pre-scaled by `weight` in the file.
#[derive(Clone, Copy, Debug, Default)]
pub struct Influence {
    pub bone: usize,
    pub pos: Vec3,
    pub normal: Vec3,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub vertex: u16,
    pub uv: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct Group {
    pub texture: String,
    pub triangles: Vec<[Corner; 3]>,
    /// The material's light factor (0x8010 record f32 @18; 1 for most, faces up to 1.6).
    pub light: f32,
}

/// One drawable copy of a 0x2A mesh; mirrored meshes yield a second copy for the other side.
#[derive(Clone, Debug)]
pub struct MeshPart {
    pub name: String,
    pub mirrored: bool,
    /// One or two influences per vertex.
    pub vertices: Vec<Vec<Influence>>,
    pub groups: Vec<Group>,
    /// Cloth vertices (position only): retail's FUN_100216a0 path.
    pub cloth: bool,
}

/// Header (offsets and counts in u16 units): flags @2 (0x80 = bone indices go through the table,
/// low 7 bits 1 = cloth), mirror @4, poly @6/@10, bone table @12/@16, weight counts @18,
/// bone refs @24, vertices @30.
pub fn parse_mesh(name: &str, d: &[u8]) -> Result<Vec<MeshPart>> {
    if d.len() < 0x40 {
        bail!("mesh {name}: too short");
    }
    let flags = d[2];
    let mirror = u16_at(d, 4) != 0;
    let words = |o: usize| u32_at(d, o) as usize * 2;
    let (poly, poly_len) = (words(6), u16_at(d, 10) as usize * 2);
    let (table_at, table_len) = (words(12), u16_at(d, 16) as usize);
    let weights_at = words(18);
    let bones_at = words(24);
    let (verts_at, verts_len) = (words(30), u16_at(d, 34) as usize * 2);
    if [poly + poly_len, table_at + table_len * 2, weights_at + 4, verts_at + verts_len].iter().any(|&e| e > d.len()) {
        bail!("mesh {name}: section out of range");
    }
    let table: Vec<u16> = (0..table_len).map(|i| u16_at(d, table_at + i * 2)).collect();
    let (one, two) = (u16_at(d, weights_at) as usize, u16_at(d, weights_at + 2) as usize);
    let count = one + two;
    if bones_at + count * 4 > d.len() {
        bail!("mesh {name}: bone refs out of range");
    }
    // Vertex size tells plain (pos+normal) from cloth (pos only).
    let cloth = if verts_len == one * 24 + two * 56 {
        false
    } else if verts_len == one * 12 + two * 32 {
        true
    } else {
        bail!("mesh {name}: vertex block {verts_len} B fits neither layout for {one}+{two} vertices");
    };
    let groups = parse_polys(name, &d[poly..poly + poly_len])?;

    let mut parts = Vec::new();
    for mirrored in [false, true] {
        if mirrored && !mirror {
            break;
        }
        // Bone ref u16: bits 0-6 bone, 7-13 bone for the mirrored copy, 14-15 axis to negate there.
        let resolve = |r: u16| -> Result<(usize, u16)> {
            let slot = if mirrored { (r >> 7) & 0x7F } else { r & 0x7F } as usize;
            let bone = if flags & 0x80 != 0 {
                *table.get(slot).ok_or_else(|| anyhow::anyhow!("mesh {name}: bone table index {slot} out of range"))? as usize
            } else {
                slot
            };
            Ok((bone, if mirrored { r >> 14 } else { 0 }))
        };
        let flip = |v: Vec3, axis: u16| match axis {
            1 => Vec3::new(-v.x, v.y, v.z),
            2 => Vec3::new(v.x, -v.y, v.z),
            3 => Vec3::new(v.x, v.y, -v.z),
            _ => v,
        };
        let mut vertices = Vec::with_capacity(count);
        for i in 0..count {
            let refs = [u16_at(d, bones_at + i * 4), u16_at(d, bones_at + i * 4 + 2)];
            if i < one {
                let (size, o) = (if cloth { 12 } else { 24 }, verts_at);
                let at = o + i * size;
                let (bone, axis) = resolve(refs[0])?;
                let normal = if cloth { Vec3::ZERO } else { vec3_at(d, at + 12) };
                vertices.push(vec![Influence { bone, pos: flip(vec3_at(d, at), axis), normal: flip(normal, axis), weight: 1.0 }]);
            } else {
                let size = if cloth { 32 } else { 56 };
                let at = verts_at + one * if cloth { 12 } else { 24 } + (i - one) * size;
                // Pairs: x1 x2 y1 y2 z1 z2 w1 w2 [nx1 nx2 ny1 ny2 nz1 nz2].
                let mut inf = Vec::with_capacity(2);
                for k in 0..2 {
                    let (bone, axis) = resolve(refs[k])?;
                    let g = |c: usize| f32_at(d, at + c * 8 + k * 4);
                    let pos = Vec3::new(g(0), g(1), g(2));
                    let normal = if cloth { Vec3::ZERO } else { Vec3::new(g(4), g(5), g(6)) };
                    inf.push(Influence { bone, pos: flip(pos, axis), normal: flip(normal, axis), weight: g(3) });
                }
                vertices.push(inf);
            }
        }
        if let Some(bad) = groups.iter().flat_map(|g| &g.triangles).flatten().find(|c| c.vertex as usize >= count) {
            bail!("mesh {name}: corner refers to vertex {} of {count}", bad.vertex);
        }
        let mut groups = groups.clone();
        if mirrored {
            for t in groups.iter_mut().flat_map(|g| g.triangles.iter_mut()) {
                t.swap(1, 2);
            }
        }
        parts.push(MeshPart { name: name.to_string(), mirrored, vertices, groups, cloth });
    }
    Ok(parts)
}

/// Poly stream of u16-tagged records: 0x8010 material (46 B, light factor f32 @18), 0x8000 texture name (18 B),
/// "ST" strip, "T\0" list; colour-only "SC" strip (3 idx, u32 colour, n-1 idx) and "C\0" list are
/// skipped; 0xFFFF ends it. Corners carry their own UV.
fn parse_polys(name: &str, p: &[u8]) -> Result<Vec<Group>> {
    let corner = |o: usize| Corner { vertex: u16_at(p, o), uv: [f32_at(p, o + 2), f32_at(p, o + 6)] };
    let tri = |o: usize| -> [Corner; 3] {
        let uv = |k: usize| [f32_at(p, o + 6 + k * 8), f32_at(p, o + 10 + k * 8)];
        [0, 1, 2].map(|k| Corner { vertex: u16_at(p, o + k * 2), uv: uv(k) })
    };
    let mut groups: Vec<Group> = Vec::new();
    let mut texture = String::new();
    let mut light = 1.0f32;
    let mut at = 0;
    while at + 4 <= p.len() {
        let (tag, n) = (u16_at(p, at), u16_at(p, at + 2) as usize);
        let need = |len: usize| if at + len > p.len() { bail!("mesh {name}: poly record {tag:#06x} overruns") } else { Ok(()) };
        if tag == 0xFFFF {
            break;
        } else if tag & 0x80F0 == 0x8010 {
            need(46)?;
            light = Some(f32_at(p, at + 18)).filter(|f| f.is_finite()).unwrap_or(1.0);
            at += 46;
        } else if tag & 0x80F0 == 0x8000 {
            need(18)?;
            texture = name_at(p, at + 2, 16);
            at += 18;
        } else if tag == 0x5453 {
            need(4 + 30 + n.saturating_sub(1) * 10)?;
            let first = tri(at + 4);
            let mut strip = first.to_vec();
            for k in 0..n.saturating_sub(1) {
                strip.push(corner(at + 34 + k * 10));
            }
            let mut tris = Vec::new();
            for i in 0..strip.len().saturating_sub(2) {
                let (a, b, c) = (strip[i], strip[i + 1], strip[i + 2]);
                if a.vertex == b.vertex || b.vertex == c.vertex || a.vertex == c.vertex {
                    continue;
                }
                tris.push(if i % 2 == 0 { [a, b, c] } else { [b, a, c] });
            }
            groups.push(Group { texture: texture.clone(), triangles: tris, light });
            at += 4 + 30 + n.saturating_sub(1) * 10;
        } else if tag == 0x0054 {
            need(4 + n * 30)?;
            groups.push(Group { texture: texture.clone(), triangles: (0..n).map(|k| tri(at + 4 + k * 30)).collect(), light });
            at += 4 + n * 30;
        } else if tag == 0x4353 {
            at += n * 2 + 12;
        } else if tag == 0x0043 {
            at += n * 10 + 4;
        } else {
            bail!("mesh {name}: unknown poly record {tag:#06x} at {at:#x}");
        }
    }
    Ok(groups)
}

/// A keyed channel: either one value or one per frame.
#[derive(Clone, Debug)]
pub enum Channel {
    Const(f32),
    Keys(Vec<f32>),
}

impl Channel {
    /// The keys either side of `frame` and the fraction between them (a constant twice, fraction 0).
    fn pair(&self, frame: f32) -> (f32, f32, f32) {
        match self {
            Channel::Const(v) => (*v, *v, 0.0),
            Channel::Keys(k) => {
                let last = k.len() - 1;
                let f = frame.clamp(0.0, last as f32);
                let i = (f as usize).min(last);
                let j = (i + 1).min(last);
                (k[i], k[j], f - i as f32)
            }
        }
    }

    fn at(&self, frame: f32) -> f32 {
        let (a, b, s) = self.pair(frame);
        a + (b - a) * s
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub bone: usize,
    /// x y z w
    pub rot: [Channel; 4],
    pub trans: [Channel; 3],
    pub scale: [Channel; 3],
}

#[derive(Clone, Debug)]
pub struct Animation {
    pub name: String,
    pub frames: u16,
    pub speed: f32,
    pub tracks: Vec<Track>,
}

impl Track {
    pub fn sample(&self, frame: f32) -> (Quat, Vec3, Vec3) {
        // FUN_10032400: the next key is negated when the dot is negative, then lerped (keys flip sign freely).
        let k = self.rot.each_ref().map(|c| c.pair(frame));
        let f = k.iter().fold(0f32, |m, p| m.max(p.2));
        let (a, b) = (glam::Vec4::from(k.map(|p| p.0)), glam::Vec4::from(k.map(|p| p.1)));
        let b = if a.dot(b) < 0.0 { -b } else { b };
        let t = [0, 1, 2].map(|i| self.trans[i].at(frame));
        let s = [0, 1, 2].map(|i| self.scale[i].at(frame));
        (Quat::from_vec4(a.lerp(b, f)).normalize(), Vec3::from(t), Vec3::from(s))
    }
}

/// 10-byte header (u8 ver, u8 ?, u16 tracks, u16 frames, f32 speed), 84-byte tracks:
/// i32 bone, then rot xyzw, trans xyz, scale xyz as (i32 index, f32 value) blocks. A nonzero
/// index points at `frames` floats at 10 + 4 * index; rot index with bit 31 set = no track.
pub fn parse_animation(name: String, d: &[u8]) -> Result<Animation> {
    if d.len() < 10 {
        bail!("animation {name}: too short");
    }
    let (count, frames, speed) = (u16_at(d, 2) as usize, u16_at(d, 4), f32_at(d, 6));
    if 10 + count * 84 > d.len() || frames == 0 {
        bail!("animation {name}: {count} tracks / {frames} frames do not fit");
    }
    let mut tracks = Vec::with_capacity(count);
    for t in 0..count {
        let o = 10 + t * 84;
        let channels = |idx_at: usize, val_at: usize, n: usize| -> Result<Vec<Channel>> {
            (0..n)
                .map(|i| {
                    let idx = i32_at(d, idx_at + i * 4);
                    if idx <= 0 {
                        return Ok(Channel::Const(f32_at(d, val_at + i * 4)));
                    }
                    let at = 10 + idx as usize * 4;
                    if at + frames as usize * 4 > d.len() {
                        bail!("animation {name}: keys out of range");
                    }
                    Ok(Channel::Keys((0..frames as usize).map(|k| f32_at(d, at + k * 4)).collect()))
                })
                .collect()
        };
        if i32_at(d, o + 4) < 0 {
            continue;
        }
        let rot: [Channel; 4] = channels(o + 4, o + 20, 4)?.try_into().unwrap();
        let trans: [Channel; 3] = channels(o + 36, o + 48, 3)?.try_into().unwrap();
        let scale: [Channel; 3] = channels(o + 60, o + 72, 3)?.try_into().unwrap();
        tracks.push(Track { bone: i32_at(d, o) as usize, rot, trans, scale });
    }
    Ok(Animation { name, frames, speed, tracks })
}

/// World-space bone transforms (rotation, translation) for a pose.
pub fn pose(skel: &Skeleton, anims: &[(&Animation, f32)]) -> Vec<(Quat, Vec3)> {
    let mut local: Vec<(Quat, Vec3)> = skel.bones.iter().map(|b| (b.rot, b.pos)).collect();
    for (anim, frame) in anims {
        for t in &anim.tracks {
            let Some(b) = skel.bones.get(t.bone) else { continue };
            let (r, tr, _scale) = t.sample(*frame);
            // Verified visually: r * rest poses correctly (rest * r folds the torso); translation adds
            // unrotated (shield bone 87 lands on the forearm only that way).
            local[t.bone] = (r * b.rot, b.pos + tr);
        }
    }
    let mut world: Vec<(Quat, Vec3)> = Vec::with_capacity(local.len());
    for (i, b) in skel.bones.iter().enumerate() {
        let (r, t) = local[i];
        if i == 0 {
            world.push((r, t));
        } else {
            let (pr, pt) = world[b.parent];
            world.push((pr * r, pt + pr * t));
        }
    }
    world
}

/// Skinned position and normal per vertex.
pub fn skin(part: &MeshPart, world: &[(Quat, Vec3)]) -> Vec<(Vec3, Vec3)> {
    part.vertices
        .iter()
        .map(|inf| {
            let (mut p, mut n) = (Vec3::ZERO, Vec3::ZERO);
            for i in inf {
                let (r, t) = world.get(i.bone).copied().unwrap_or((Quat::IDENTITY, Vec3::ZERO));
                p += r * i.pos + t * i.weight;
                n += r * i.normal;
            }
            (p, n.normalize_or_zero())
        })
        .collect()
}

/// Everything parsed from one DAT that matters for a character.
#[derive(Default)]
pub struct CharFile {
    pub skeleton: Option<Skeleton>,
    pub parts: Vec<MeshPart>,
    pub textures: Vec<Texture>,
    pub animations: Vec<Animation>,
    /// Payload of the 0x45 "info" chunk: per-part motion selectors, 0xFF = unset.
    pub info: Option<[u8; 16]>,
    /// A mount file's 0x45 "moun" chunk (FUN_100cf210): rider yaw offsets and riding variants by race.
    pub moun: Option<[u8; 16]>,
    /// Type-0x07 schedulers (name, data): attack, damage and death scripts (dat::sched).
    pub schedulers: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

pub fn load_file(file: &[u8]) -> Result<CharFile> {
    let mut out = CharFile::default();
    for c in chunks(file)? {
        let result = match c.kind {
            0x29 => parse_skeleton(c.name(), c.data).map(|s| out.skeleton = Some(s)),
            0x2A => parse_mesh(&c.name(), c.data).map(|p| out.parts.extend(p)),
            0x2B => parse_animation(c.name(), c.data).map(|a| out.animations.push(a)),
            0x20 => parse_img(c.data).map(|t| out.textures.push(t)),
            0x07 => {
                out.schedulers.push((c.name(), c.data.to_vec()));
                Ok(())
            }
            0x45 if c.data.len() >= 16 && c.name() == "moun" => {
                out.moun = c.data[..16].try_into().ok();
                Ok(())
            }
            0x45 if c.data.len() >= 16 && out.info.is_none() => {
                out.info = c.data[..16].try_into().ok();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            out.warnings.push(format!("chunk {} at {:#x}: {e}", c.name(), c.offset));
        }
    }
    Ok(out)
}

/// Model ids as the lobby sends them (GrapIDTbl without the slot nibble); face 0..15 = 1A..8B.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub race: u8,
    pub face: u16,
    pub head: u16,
    pub body: u16,
    pub hands: u16,
    pub legs: u16,
    pub feet: u16,
    pub main: u16,
    pub sub: u16,
    /// The range slot (GrapIDTbl[8]): its file's info byte 0xe is the ranged class (actor +0x886).
    pub range: u16,
}

/// Skeleton + leg motions per race 1..8 (FFXiMain .data 0x10353b70, u16 by race 0..36).
const SKELETON: [u32; 8] = [7072, 10248, 13424, 16600, 19776, 19776, 23176, 26352];

/// NPC-only races from the same tables: 29..31 children (skeleton file carries the motions),
/// 32..36 chocobos. Slots face..feet only; one range each (count 0 = slot unused).
#[rustfmt::skip]
const NPC_RACES: [(u8, u32, [(u32, u32); 6]); 8] = [
    (29, 30696, [(30376, 64), (30568, 64), (30312, 64), (30504, 64), (30632, 64), (30440, 64)]),
    (30, 29912, [(29592, 64), (29784, 64), (29528, 64), (29720, 64), (29848, 64), (29656, 64)]),
    (31, 30304, [(29984, 64), (30176, 64), (29920, 64), (30112, 64), (30240, 64), (30048, 64)]),
    (32, 55295, [(0, 0), (55313, 8), (55297, 8), (55321, 4), (55325, 4), (55305, 8)]),
    (33, 55329, [(0, 0), (55347, 8), (55331, 8), (55355, 4), (55359, 4), (55339, 8)]),
    (34, 55363, [(0, 0), (55381, 8), (55365, 8), (55389, 4), (55393, 4), (55373, 8)]),
    (35, 55397, [(0, 0), (55415, 8), (55399, 8), (55423, 4), (55427, 4), (55407, 8)]),
    (36, 55431, [(0, 0), (55449, 8), (55433, 8), (55457, 4), (55461, 4), (55441, 8)]),
];

/// Race 0 uses the Hume male tables (as the client does).
fn pc_race(race: u8) -> Option<usize> {
    match race {
        0 => Some(0),
        1..=8 => Some(race as usize - 1),
        _ => None,
    }
}

pub fn skeleton_file(race: u8) -> Option<u32> {
    match pc_race(race) {
        Some(r) => Some(SKELETON[r]),
        None => NPC_RACES.iter().find(|n| n.0 == race).map(|n| n.1),
    }
}

pub const SLOTS: [&str; 9] = ["face", "head", "body", "hands", "legs", "feet", "main", "sub", "range"];

/// Per race 1..8, per slot (face head body hands legs feet main sub range): up to six
/// (first file id, count) ranges that model ids walk in order. From FFXiMain .data: pointer per
/// race at 0x1035f378 (retail 30261006_0; 0x10353ad8 in 2019), 9 x 48-byte records; read by the PC model
/// loader FUN_100d2f60 (FUN_100d0ef0 in 2019).
#[rustfmt::skip]
const SLOT_RANGES: [[[(u32, u32); 6]; 9]; 8] = [
    [
        [(7080, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(7112, 256), (63323, 48), (63371, 16), (71247, 256), (98787, 32), (102961, 64)],
        [(7368, 256), (63387, 48), (63435, 16), (71503, 256), (98819, 32), (103025, 64)],
        [(7624, 256), (63451, 48), (63499, 16), (71759, 256), (98851, 32), (103089, 64)],
        [(7880, 256), (63515, 48), (63563, 16), (72015, 256), (98883, 32), (103153, 64)],
        [(8136, 256), (63579, 48), (63627, 16), (72271, 256), (98915, 32), (103217, 64)],
        [(8392, 512), (63643, 128), (0, 0), (72527, 256), (107301, 300), (0, 64)],
        [(41199, 512), (66459, 128), (0, 0), (81999, 256), (105201, 300), (0, 64)],
        [(9416, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(10256, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(10288, 256), (63771, 48), (63819, 16), (72783, 256), (98947, 32), (103281, 64)],
        [(10544, 256), (63835, 48), (63883, 16), (73039, 256), (98979, 32), (103345, 64)],
        [(10800, 256), (63899, 48), (63947, 16), (73295, 256), (99011, 32), (103409, 64)],
        [(11056, 256), (63963, 48), (64011, 16), (73551, 256), (99043, 32), (103473, 64)],
        [(11312, 256), (64027, 48), (64075, 16), (73807, 256), (99075, 32), (103537, 64)],
        [(11568, 512), (64091, 128), (0, 0), (74063, 256), (107601, 300), (0, 64)],
        [(42479, 512), (66587, 128), (0, 0), (82255, 256), (105501, 300), (0, 64)],
        [(12592, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(13432, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(13464, 256), (64219, 48), (64267, 16), (74319, 256), (99107, 32), (103601, 64)],
        [(13720, 256), (64283, 48), (64331, 16), (74575, 256), (99139, 32), (103665, 64)],
        [(13976, 256), (64347, 48), (64395, 16), (74831, 256), (99171, 32), (103729, 64)],
        [(14232, 256), (64411, 48), (64459, 16), (75087, 256), (99203, 32), (103793, 64)],
        [(14488, 256), (64475, 48), (64523, 16), (75343, 256), (99235, 32), (103857, 64)],
        [(14744, 512), (64539, 128), (0, 0), (75599, 256), (107901, 300), (0, 64)],
        [(43759, 512), (66715, 128), (0, 0), (82511, 256), (105801, 300), (0, 64)],
        [(15768, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(16608, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(16640, 256), (64667, 48), (64715, 16), (75855, 256), (99267, 32), (103921, 64)],
        [(16896, 256), (64731, 48), (64779, 16), (76111, 256), (99299, 32), (103985, 64)],
        [(17152, 256), (64795, 48), (64843, 16), (76367, 256), (99331, 32), (104049, 64)],
        [(17408, 256), (64859, 48), (64907, 16), (76623, 256), (99363, 32), (104113, 64)],
        [(17664, 256), (64923, 48), (64971, 16), (76879, 256), (99395, 32), (104177, 64)],
        [(17920, 512), (64987, 128), (0, 0), (77135, 256), (108201, 300), (0, 64)],
        [(45039, 512), (66843, 128), (0, 0), (82767, 256), (106101, 300), (0, 64)],
        [(18944, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(19784, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(19816, 256), (65115, 48), (65163, 16), (77391, 256), (99427, 32), (104241, 64)],
        [(20072, 256), (65179, 48), (65227, 16), (77647, 256), (99459, 32), (104305, 64)],
        [(20328, 256), (65243, 48), (65291, 16), (77903, 256), (99491, 32), (104369, 64)],
        [(20584, 256), (65307, 48), (65355, 16), (78159, 256), (99523, 32), (104433, 64)],
        [(20840, 256), (65371, 48), (65419, 16), (78415, 256), (99555, 32), (104497, 64)],
        [(21096, 512), (65435, 128), (0, 0), (78671, 256), (108501, 300), (0, 64)],
        [(46319, 512), (66971, 128), (0, 0), (83023, 256), (106401, 300), (0, 64)],
        [(22120, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(22952, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(19816, 256), (65115, 48), (65171, 16), (77391, 256), (99443, 32), (104241, 64)],
        [(20072, 256), (65179, 48), (65235, 16), (77647, 256), (99475, 32), (104305, 64)],
        [(20328, 256), (65243, 48), (65299, 16), (77903, 256), (99507, 32), (104369, 64)],
        [(20584, 256), (65307, 48), (65363, 16), (78159, 256), (99539, 32), (104433, 64)],
        [(20840, 256), (65371, 48), (65427, 16), (78415, 256), (99571, 32), (104497, 64)],
        [(21096, 512), (65435, 128), (0, 0), (78671, 256), (108501, 300), (0, 64)],
        [(46319, 512), (66971, 128), (0, 0), (83023, 256), (106401, 300), (0, 64)],
        [(22120, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(23184, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(23216, 256), (65563, 48), (65611, 16), (78927, 256), (99587, 32), (104561, 64)],
        [(23472, 256), (65627, 48), (65675, 16), (79183, 256), (99619, 32), (104625, 64)],
        [(23728, 256), (65691, 48), (65739, 16), (79439, 256), (99651, 32), (104689, 64)],
        [(23984, 256), (65755, 48), (65803, 16), (79695, 256), (99683, 32), (104753, 64)],
        [(24240, 256), (65819, 48), (65867, 16), (79951, 256), (99715, 32), (104817, 64)],
        [(24496, 512), (65883, 128), (0, 0), (80207, 256), (108801, 300), (0, 64)],
        [(47599, 512), (67099, 128), (0, 0), (83279, 256), (106701, 300), (0, 64)],
        [(25520, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
    [
        [(26360, 32), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        [(26392, 256), (66011, 48), (66059, 16), (80463, 256), (99747, 32), (104881, 64)],
        [(26648, 256), (66075, 48), (66123, 16), (80719, 256), (99779, 32), (104945, 64)],
        [(26904, 256), (66139, 48), (66187, 16), (80975, 256), (99811, 32), (105009, 64)],
        [(27160, 256), (66203, 48), (66251, 16), (81231, 256), (99843, 32), (105073, 64)],
        [(27416, 256), (66267, 48), (66315, 16), (81487, 256), (99875, 32), (105137, 64)],
        [(27672, 512), (66331, 128), (0, 0), (81743, 256), (109101, 300), (0, 64)],
        [(48879, 512), (67227, 128), (0, 0), (83535, 256), (107001, 300), (0, 64)],
        [(28696, 256), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    ],
];

/// File id of a model id in a slot, or None past the last range.
pub fn part_file(race: u8, slot: usize, id: u16) -> Option<u32> {
    let Some(r) = pc_race(race) else {
        let &(base, count) = NPC_RACES.iter().find(|n| n.0 == race)?.2.get(slot)?;
        return (base != 0 && (id as u32) < count).then_some(base + id as u32);
    };
    let ranges = SLOT_RANGES[r].get(slot)?;
    let mut id = id as u32;
    for &(base, count) in ranges {
        if id < count {
            return (base != 0).then_some(base + id);
        }
        id -= count;
    }
    None
}

/// File ids for a look: skeleton file first, then (slot, model id, file id) per part.
pub fn file_ids(look: &Look) -> Result<(u32, Vec<(&'static str, u16, u32)>)> {
    let Some(skel) = skeleton_file(look.race) else { bail!("race {} has no model tables (0..8, 29..36)", look.race) };
    let npc = pc_race(look.race).is_none();
    let ids = [look.face, look.head, look.body, look.hands, look.legs, look.feet, look.main, look.sub, look.range];
    let mut parts = Vec::new();
    for (slot, &id) in ids.iter().enumerate() {
        // Weapon id 0 is an empty dummy mesh; skip it rather than draw it. The range slot always loads, id 0 too
        // (retail loads all nine; its class byte drives songs). NPC races stop after feet (FUN_100d0ef0), and
        // chocobos have no face slot.
        if (matches!(slot, 6 | 7) && id == 0) || (npc && (slot >= 6 || (slot == 0 && look.race >= 32))) {
            continue;
        }
        match part_file(look.race, slot, id) {
            Some(f) => parts.push((SLOTS[slot], id, f)),
            None => bail!("{} model id {id} is outside the race {} table", SLOTS[slot], look.race),
        }
    }
    Ok((skel, parts))
}

/// A mount file's riding data (FUN_100cf210 into the actor's info block +0x878: `moun` at +0x14, info[15] / [14] at
/// +0x24 / +0x25).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountInfo {
    pub moun: [u8; 16],
    pub rate: u8,
    pub cam: u8,
}

impl MountInfo {
    /// DAT_1032a410: the race's column (Hume M, Hume F, Elvaan M, Elvaan F, Taru M, Taru F, Mithra, Galka).
    fn column(race: u8) -> Option<usize> {
        [0, 0, 1, 2, 3, 4, 5, 6, 7].get(race as usize).copied()
    }

    /// FUN_100222b0: the rider's yaw offset, radians (unsigned units of 2 pi / 255.5; 0xff none).
    pub fn yaw(&self, race: u8) -> f32 {
        match Self::column(race).map(|i| self.moun[i]) {
            Some(b) if b != 0xff => b as f32 * 0.024_591_723,
            _ => 0.0,
        }
    }

    /// FUN_10022340: the riding variant digit replacing the first letter of the rider's motions (0 none).
    pub fn variant(&self, race: u8) -> u8 {
        match Self::column(race).map(|i| self.moun[8 + i]) {
            Some(b) if b != 0xff => b,
            _ => 0,
        }
    }

    /// FUN_10022380: the animal's motion rate (info[15] / 100).
    pub fn rate(&self) -> f32 {
        if self.rate == 0xff { 1.0 } else { self.rate as f32 / 100.0 }
    }

    /// FUN_100223b0: added to the third-person look height (info[14] / 100).
    pub fn cam_add(&self) -> f32 {
        if self.cam == 0xff { 0.0 } else { self.cam as f32 / 100.0 }
    }
}

pub struct Character {
    pub skeleton: Skeleton,
    pub parts: Vec<MeshPart>,
    pub textures: Vec<Texture>,
    pub animations: Vec<Animation>,
    /// 0x45 info bytes 10..13: percent scale per GraphSize 0..3, 0xFF = unset (first file wins).
    pub size_info: [u8; 4],
    /// Stand/battle/walk/run motion names, as set by the model's init scheduler.
    pub motions: crate::motion::MotionNames,
    /// Type-0x07 schedulers of the model files, first file first.
    pub schedulers: Vec<(String, Vec<u8>)>,
    /// The model's files in load order, and the equipment part files face..range (actor vtable +0x1ec searches
    /// these; FUN_100cc440 the motion files, then the model's files).
    pub files: Vec<u32>,
    pub part_files: Vec<u32>,
    /// 0x45 "Cib" bytes 0..2 (FUN_100cd1d0, first file wins; the feet part file overwrites 1 and 2): leg mode
    /// (+0x878), footstep kind (+0x879), shoe (+0x87a); -1 = unset.
    pub cib: [i8; 3],
    /// Actor +0x886: the range file's info byte 0xe (FUN_100d0b50 case 8), else the first file's >= 0 (FUN_100cf210);
    /// -1 none. Ops 0x76 / 0x77 pick `lc` / `ls` + this in two digits (0 sing, 1 wind, 2 string, 3..6 shot, 11 bell).
    pub ranged: i8,
    /// PC looks: what picks the battle and event motion files.
    pub pc: Option<super::motionfile::PcMotion>,
    /// A mount (102705..): its `moun` chunk and info bytes 14 / 15.
    pub mount: Option<MountInfo>,
    /// The `wepN` hide bytes 0 / 1 the model's `init` leaves set (ops 0x75 / 0xa3): seeded on the actor so the first
    /// frames before `init` runs do not show them.
    pub init_hidden: [[bool; 2]; 10],
    pub log: Vec<String>,
    /// The animations as GPU words (`prepare`), else packed when first posed.
    pub packed: Option<std::sync::Arc<super::animpack::Packed>>,
    /// The skinned vertices for the GPU (`prepare`).
    pub skin: Option<std::sync::Arc<crate::render::SkinMesh>>,
}

impl Character {
    /// `init_hidden` as a `wepN` bit mask.
    pub fn init_hidden_mask(&self) -> u16 {
        (0..10).filter(|&n| self.init_hidden[n].iter().any(|&h| h)).fold(0, |m, n| m | 1 << n)
    }

    /// Loader-thread work for a world actor: the packed animations and skinned vertices (mips are made on the GPU).
    /// Skeleton +0x14 bit 0x80 (FUN_100251a0 / FUN_10035330): the bones a loaded mesh weights, and their ancestors.
    pub fn geometry_bones(&self) -> Vec<bool> {
        let mut flags = vec![false; self.skeleton.bones.len()];
        for v in self.parts.iter().flat_map(|p| p.vertices.iter()).flatten() {
            let mut b = v.bone;
            while b < flags.len() && !flags[b] {
                flags[b] = true;
                if b == 0 {
                    break;
                }
                b = self.skeleton.bones[b].parent;
            }
        }
        flags
    }

    pub fn prepare(&mut self) {
        self.packed = Some(std::sync::Arc::new(super::animpack::pack(&self.animations)));
        self.skin = Some(std::sync::Arc::new(crate::render::SkinMesh::build(self)));
    }

    /// Uniform model scale for the 0x00D/0x00E GraphSize (FUN_100212f0: info[10 + size] / 100, unset = 1).
    pub fn scale(&self, graph_size: u8) -> f32 {
        match self.size_info.get(graph_size as usize) {
            Some(&p) if p != 0xFF => p as f32 / 100.0,
            _ => 1.0,
        }
    }

    /// Fills unset size bytes from a file's info chunk; the client copies only bytes not yet set.
    pub fn merge_size_info(&mut self, info: Option<[u8; 16]>) {
        let Some(info) = info else { return };
        for (dst, &src) in self.cib.iter_mut().zip(&info[0..3]) {
            if *dst < 0 {
                *dst = src as i8;
            }
        }
        for (dst, &src) in self.size_info.iter_mut().zip(&info[10..14]) {
            if *dst == 0xFF {
                *dst = src;
            }
        }
        if self.ranged < 0 && info[14] as i8 >= 0 {
            self.ranged = info[14] as i8;
        }
    }

    /// The region chunks of a motion (FUN_100cb500): names sharing the first 3 letters, one per region (the 4th
    /// character), the first of each. "idl0" and "idl" both give idl0 idl1 idl2.
    pub fn find_animations(&self, name: &str) -> Vec<&Animation> {
        let prefix = name.get(..3).unwrap_or(name);
        let mut regions = std::collections::HashSet::new();
        self.animations
            .iter()
            .filter(|a| {
                let n = a.name.trim_end().as_bytes();
                n.len() == 4 && n.starts_with(prefix.as_bytes()) && (b'0'..=b'6').contains(&n[3]) && regions.insert(n[3])
            })
            .collect()
    }
}

/// Info byte as the client's signed char, 0xFF (unset) as 0.
fn selector(info: Option<[u8; 16]>, at: usize) -> u32 {
    info.map_or(0, |i| (i[at] as i8).max(0) as u32)
}

#[cfg(test)]
mod tests {
    /// Point 2 of the PC skeletons anchors the name plate: bone 0, (0, -h, 0) with h by race.
    #[test]
    fn plate_points() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        for (file, h) in [(7072u32, 2.0f32), (16600, 2.4), (26352, 2.6)] {
            let Ok(d) = index.read(file) else { return };
            let skel = super::load_file(&d).unwrap().skeleton.unwrap();
            let (bone, t) = skel.points[2];
            assert_eq!(bone, 0, "file {file}");
            assert!((t.y + h).abs() < 1e-3 && t.x.abs() < 1e-3, "file {file}: {t}");
        }
    }

    /// First person's eye (FUN_10020ba0): attach point 12 sits at head height in every PC's rest pose.
    #[test]
    fn eye_points() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        for (file, lo, hi) in [(7072u32, 1.4f32, 2.1), (16600, 1.7, 2.5), (19776, 0.7, 1.4), (26352, 1.8, 2.7)] {
            let Ok(d) = index.read(file) else { return };
            let skel = super::load_file(&d).unwrap().skeleton.unwrap();
            let rest = super::pose(&skel, &[]);
            let (bone, off) = skel.points[12];
            let (q, t) = rest[bone as usize];
            let h = -(t + q * off).y;
            println!("file {file}: point 12 bone {bone} height {h:.2}");
            assert!((lo..hi).contains(&h), "file {file}: {h}");
        }
    }

    /// Every PC skeleton's head look: head (0.24, 0.16, 0.5), neck (0.16, 0.06, 0.5); points 3 / 7 head / neck.
    #[test]
    fn look_limits() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        for (file, head) in [(7072u32, 51u16), (10248, 29), (19776, 6), (26352, 39)] {
            let Ok(d) = index.read(file) else { return };
            let skel = super::load_file(&d).unwrap().skeleton.unwrap();
            assert_eq!(skel.look_limits, [[0.24, 0.16, 0.5], [0.16, 0.06, 0.5]], "file {file}");
            assert_eq!((skel.points[3].0, skel.points[7].0), (head, head - 1), "file {file}");
        }
    }
}

pub fn load_character(index: &DatIndex, look: &Look) -> Result<Character> {
    let (skel_id, part_ids) = file_ids(look)?;
    let base = load_file(&index.read(skel_id)?)?;
    let Some(skeleton) = base.skeleton else { bail!("file {skel_id} has no skeleton") };
    let mut log = vec![format!("skeleton {skel_id}: {} bones, {} animations", skeleton.bones.len(), base.animations.len())];
    log.extend(base.warnings);
    let mut ch = Character { skeleton, parts: base.parts, textures: base.textures, animations: base.animations, size_info: [0xFF; 4], motions: Default::default(), schedulers: base.schedulers, pc: None, mount: None, log, packed: None, skin: None, files: vec![skel_id], part_files: Vec::new(), cib: [-1; 3], ranged: -1, init_hidden: Default::default() };
    ch.merge_size_info(base.info);
    let (mut body_info, mut sub_info, mut main_info) = (None, None, None);
    for (slot, model, id) in part_ids {
        let f = match index.read(id).and_then(|b| load_file(&b)) {
            Ok(f) => f,
            Err(e) => {
                ch.log.push(format!("{slot} {model} -> file {id}: {e}"));
                continue;
            }
        };
        ch.log.push(format!("{slot} {model} -> file {id}: {} mesh copies, {} textures", f.parts.len(), f.textures.len()));
        ch.log.extend(f.warnings.into_iter().map(|w| format!("{slot} {id}: {w}")));
        match slot {
            "body" => body_info = f.info,
            "sub" => sub_info = f.info,
            "feet" => {
                if let Some(i) = f.info {
                    ch.cib[1] = i[1] as i8;
                    ch.cib[2] = i[2] as i8;
                }
            }
            "range" => ch.ranged = f.info.map_or(-1, |i| i[14] as i8),
            _ => {}
        }
        ch.merge_size_info(f.info);
        // The range mesh (wep2) starts hidden, as `init`'s hwpc leaves it; ops 0x75 show it to shoot or play.
        if slot == "range" && !f.parts.is_empty() {
            ch.init_hidden[2][0] = true;
        }
        ch.parts.extend(f.parts);
        ch.textures.extend(f.textures);
        ch.schedulers.extend(f.schedulers);
        ch.files.push(id);
        ch.part_files.push(id);
    }
    // Motions are split by region: "xxx0" legs (skeleton file), "xxx1" upper body, "xxx2" skirt bones
    // 38-47. Client (FUN_100d0dc0 / FUN_100d0d00): upper = skeleton + 1 + sub info[5],
    // skirt = skeleton + 3 + max(body info[9] - 1, 0). NPC races keep their motions in the skeleton file.
    if pc_race(look.race).is_none() {
        return Ok(ch);
    }
    // Main weapon info[3] / [4] (+0x87b / +0x87c); the empty weapon 0 still has its info chunk.
    if main_info.is_none() {
        main_info = part_file(look.race, 6, look.main).and_then(|id| index.read(id).ok()).and_then(|b| load_file(&b).ok()).and_then(|f| f.info);
    }
    let byte = |i: Option<[u8; 16]>, at: usize| i.map_or(-1, |i| i[at] as i8);
    ch.pc = Some(super::motionfile::PcMotion { race: look.race, weapon: byte(main_info, 3), draw: byte(main_info, 4), skirt: byte(body_info, 9) });
    let upper = skel_id + 1 + selector(sub_info, 5).min(1);
    let skirt = skel_id + 3 + selector(body_info, 9).saturating_sub(1).min(1);
    for (what, id) in [("upper-body motions", upper), ("skirt motions", skirt)] {
        match index.read(id).and_then(|b| load_file(&b)) {
            Ok(f) => {
                ch.log.push(format!("{what}: file {id}, {} animations", f.animations.len()));
                ch.animations.extend(f.animations);
            }
            Err(e) => ch.log.push(format!("{what} {id}: {e}")),
        }
    }
    Ok(ch)
}
