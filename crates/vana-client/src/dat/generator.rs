//! Effect generators (chunk 0x05, "Generater") and the resources they name: keyframes (0x19), D3m meshes (0x1f),
//! with names resolved by folder scope as FUN_10071310 does. Spec with addresses:
//! ~/ffxi/lab/research/effects/generators.md (sections 1-3, D.5).

use super::chunks;
use anyhow::{Result, bail};

/// Chunk types the effect system uses (names from the table at 0x10349110).
pub mod kind {
    pub const END: u8 = 0x00;
    pub const FOLDER: u8 = 0x01;
    pub const GENERATOR: u8 = 0x05;
    pub const SCHEDULER: u8 = 0x07;
    pub const TEXINFO: u8 = 0x0a;
    pub const KEYFRAME: u8 = 0x19;
    pub const MOTION: u8 = 0x2b;
    pub const D3M: u8 = 0x1f;
    pub const IMG: u8 = 0x20;
    pub const D3A: u8 = 0x21;
    pub const MMB: u8 = 0x2e;
    pub const VTX: u8 = 0x3e;
    pub const SPH: u8 = 0x4a;
}

/// One chunk of a DAT in its folder: type-1 chunks open a folder, type-0 chunks close it.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: [u8; 4],
    pub kind: u8,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// The payload's byte range in the file (after the 16-byte header).
    pub data: std::ops::Range<usize>,
}

impl Node {
    pub fn name(&self) -> String {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(4);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }
}

/// A DAT's chunks as a folder tree.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

/// A 4cc from a string ("sea1"), zero-padded.
pub fn fourcc(s: &str) -> [u8; 4] {
    let mut n = [0u8; 4];
    for (d, b) in n.iter_mut().zip(s.bytes()) {
        *d = b;
    }
    n
}

/// A 4cc read from a u32 in the file.
pub fn fourcc_u32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

impl Tree {
    pub fn build(file: &[u8]) -> Result<Self> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        for c in chunks(file)? {
            if c.kind == kind::END {
                open.pop();
                continue;
            }
            let i = nodes.len();
            let parent = open.last().copied();
            nodes.push(Node { name: c.name, kind: c.kind, parent, children: Vec::new(), data: c.offset + 16..c.offset + 16 + c.data.len() });
            if let Some(p) = parent {
                nodes[p].children.push(i);
            }
            if c.kind == kind::FOLDER {
                open.push(i);
            }
        }
        Ok(Self { nodes })
    }

    /// FUN_100712e0 -> FUN_10071310: from the folder holding `from` (or `from` if it is a folder), its whole subtree
    /// in child order, then each enclosing folder's; kind 0 = any type, name 0 = the first of the type.
    pub fn find(&self, from: usize, kind: u8, name: [u8; 4]) -> Option<usize> {
        let mut folder = if self.nodes[from].kind == self::kind::FOLDER { Some(from) } else { self.nodes[from].parent };
        let mut visited = vec![false; self.nodes.len()];
        while let Some(f) = folder {
            if let Some(hit) = self.search(f, kind, name, &mut visited) {
                return Some(hit);
            }
            folder = self.nodes[f].parent;
        }
        // Chunks outside any folder count as the root's children.
        self.nodes.iter().enumerate().find(|(_, n)| n.parent.is_none() && n.kind != self::kind::FOLDER && matches(n, kind, name)).map(|(i, _)| i)
    }

    fn search(&self, folder: usize, kind: u8, name: [u8; 4], visited: &mut [bool]) -> Option<usize> {
        if visited[folder] {
            return None;
        }
        visited[folder] = true;
        for &c in &self.nodes[folder].children {
            let n = &self.nodes[c];
            if matches(n, kind, name) {
                return Some(c);
            }
            if n.kind == self::kind::FOLDER
                && let Some(hit) = self.search(c, kind, name, visited)
            {
                return Some(hit);
            }
        }
        None
    }

    /// Every node of a type, in file order.
    pub fn of_kind(&self, kind: u8) -> impl Iterator<Item = usize> + '_ {
        self.nodes.iter().enumerate().filter(move |(_, n)| n.kind == kind).map(|(i, _)| i)
    }

    /// The folder names from the root down to `i`'s folder ("t_sa", "effe", "sea").
    pub fn path(&self, i: usize) -> Vec<[u8; 4]> {
        let mut out = Vec::new();
        let mut at = self.nodes[i].parent;
        while let Some(p) = at {
            out.push(self.nodes[p].name);
            at = self.nodes[p].parent;
        }
        out.reverse();
        out
    }
}

fn matches(n: &Node, kind: u8, name: [u8; 4]) -> bool {
    (kind == 0 || n.kind == kind) && (name == [0; 4] || n.name == name)
}

/// One op of a generator stream: u32 word0 = `op | len_dwords << 8 | slot << 13`, then its operands.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub code: u8,
    pub slot: u8,
    /// All its dwords, word0 included (so operand k is `words[k]`, as the spec numbers them).
    pub words: Vec<u32>,
}

impl Op {
    pub fn f32(&self, k: usize) -> f32 {
        self.words.get(k).map_or(0.0, |&w| f32::from_bits(w))
    }

    pub fn u32(&self, k: usize) -> u32 {
        self.words.get(k).copied().unwrap_or(0)
    }

    /// The byte at byte offset `o` of the op (word0 is bytes 0..4).
    pub fn byte(&self, o: usize) -> u8 {
        self.words.get(o / 4).map_or(0, |w| w.to_le_bytes()[o % 4])
    }

    pub fn i16_at(&self, o: usize) -> i16 {
        i16::from_le_bytes([self.byte(o), self.byte(o + 1)])
    }
}

/// The four op streams, in the order of their offsets at data 0x70..0x7c.
pub mod stream {
    pub const GENERATOR: usize = 0;
    pub const EMISSION: usize = 1;
    pub const ELEMENT: usize = 2;
    pub const END: usize = 3;
}

/// A generator chunk (FUN_10046c80 copies the data into the object; for data offsets d >= 0x50 the object
/// offset is d + 0x70).
#[derive(Clone, Debug)]
pub struct Generator {
    pub name: [u8; 4],
    /// The tree node it came from (its folder scope for name lookups).
    pub node: usize,
    /// Data 0x00: attach mode `(byte2 & 1) << 4 | (byte0 & 0xf)`, caster / target points and the actor-size
    /// scale sources (section 5).
    pub attach: u32,
    /// Data 0x10 / 0x14: blend factors for the actor-size scales A / B (0 = 1.0).
    pub blend: [f32; 2],
    /// Data 0x60: life in ticks (0 = endless); 0x64 / 0x66 random and base emission interval in ticks.
    pub life: f32,
    pub interval_rand: i16,
    pub interval: i16,
    /// Data 0x68: bits 0..8 extra elements per emission, 0x400 single element, 0x1000 auto-start, 0x20000000
    /// one element per emission.
    pub flags: u32,
    /// Data 0x1c (gen +0x98): the two-point modes take emission op 0x7b's point offsets (FUN_10053e60).
    pub offsets: bool,
    pub streams: [Vec<Op>; 4],
}

impl Generator {
    pub fn parse(tree: &Tree, node: usize, file: &[u8]) -> Result<Self> {
        let d = &file[tree.nodes[node].data.clone()];
        if d.len() < 0x80 {
            bail!("generator {}: {} bytes", tree.nodes[node].name(), d.len());
        }
        let u32_at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let f32_at = |o: usize| f32::from_bits(u32_at(o));
        let i16_at = |o: usize| i16::from_le_bytes([d[o], d[o + 1]]);
        let mut streams: [Vec<Op>; 4] = Default::default();
        for (s, out) in streams.iter_mut().enumerate() {
            // Offsets count from the chunk start, i.e. data + 0x10 (FUN_10046d30: ptr = obj + 0x60 + off).
            let off = u32_at(0x70 + 4 * s) as usize;
            if off >= 0x10 {
                *out = parse_ops(d, off - 0x10);
            }
        }
        Ok(Self {
            name: tree.nodes[node].name,
            node,
            attach: u32_at(0x00),
            blend: [f32_at(0x10), f32_at(0x14)],
            offsets: u32_at(0x1c) != 0,
            life: f32_at(0x60),
            interval_rand: i16_at(0x64),
            interval: i16_at(0x66),
            flags: u32_at(0x68),
            streams,
        })
    }

    /// Attach mode 0..0x1b (FUN_1003afd0).
    pub fn attach_mode(&self) -> u8 {
        attach_mode(self.attach)
    }

    /// The emission stream's op 0x01: the element template.
    pub fn template(&self) -> Option<&Op> {
        self.streams[stream::EMISSION].first().filter(|o| o.code == 0x01)
    }
}

/// The attach mode of an attach word: `(byte2 & 1) << 4 | (byte0 & 0xf)`.
pub fn attach_mode(w: u32) -> u8 {
    (((w >> 16) & 1) << 4) as u8 | (w & 0xf) as u8
}

/// Ops from `at` until op 0 or the end (FUN_10048300 and kin walk them the same way).
pub fn parse_ops(d: &[u8], mut at: usize) -> Vec<Op> {
    let mut out = Vec::new();
    while at + 4 <= d.len() {
        let w0 = u32::from_le_bytes(d[at..at + 4].try_into().unwrap());
        let code = (w0 & 0xff) as u8;
        let len = ((w0 >> 8) & 0x1f) as usize;
        if code == 0 || len == 0 || at + 4 * len > d.len() {
            break;
        }
        let words = (0..len).map(|k| u32::from_le_bytes(d[at + 4 * k..at + 4 * k + 4].try_into().unwrap())).collect();
        out.push(Op { code, slot: ((w0 >> 13) & 0x3f) as u8, words });
        at += 4 * len;
    }
    out
}

/// A keyframe chunk: (t, v) pairs with t ascending over 0..1; the walk stops at the first t >= 1.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyFrame {
    pub keys: Vec<(f32, f32)>,
}

impl KeyFrame {
    pub fn parse(d: &[u8]) -> Self {
        let mut keys = Vec::new();
        for c in d.chunks_exact(8) {
            let t = f32::from_le_bytes(c[0..4].try_into().unwrap());
            let v = f32::from_le_bytes(c[4..8].try_into().unwrap());
            keys.push((t, v));
            if t >= 1.0 {
                break;
            }
        }
        Self { keys }
    }

    /// The pair around x: k0.t < x <= k1.t, starting at keys 0 and 1.
    fn bracket(&self, x: f32) -> Option<((f32, f32), (f32, f32))> {
        let k = &self.keys;
        match k.len() {
            0 => None,
            1 => Some((k[0], k[0])),
            _ => {
                let i = k.windows(2).position(|w| x <= w[1].0).unwrap_or(k.len() - 2);
                Some((k[i], k[i + 1]))
            }
        }
    }

    /// FUN_10053550: linear, x clamped to 0..1; equal times give k1's value.
    pub fn linear(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let Some((a, b)) = self.bracket(x) else { return 0.0 };
        if b.0 == a.0 { b.1 } else { a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0) }
    }

    /// FUN_100537f0: quadratic ease-in-out between the pair, x not clamped.
    pub fn eased(&self, x: f32) -> f32 {
        let Some((a, b)) = self.bracket(x) else { return 0.0 };
        if b.0 == a.0 {
            return b.1;
        }
        let f = (x - a.0) / (b.0 - a.0);
        if f < 0.5 {
            let g = 2.0 * f * f;
            b.1 * g + a.1 * (1.0 - g)
        } else {
            let g = 2.0 * (1.0 - f) * (1.0 - f);
            a.1 * g + b.1 * (1.0 - g)
        }
    }

    /// The sampler an op picks by `op[3] & 0xf` (0 linear, else eased).
    pub fn sample(&self, x: f32, eased: bool) -> f32 {
        if eased { self.eased(x) } else { self.linear(x) }
    }

    /// FUN_10053520 / FUN_10053740: sampled with the first key's value replaced by `v0`.
    pub fn sample_v0(&self, x: f32, eased: bool, v0: f32) -> f32 {
        let mut k = self.clone();
        if let Some(first) = k.keys.first_mut() {
            first.1 = v0;
        }
        k.sample(x, eased)
    }
}

/// A D3m mesh (chunk 0x1f, FUN_1003f0a0): a triangle list of 36-byte vertices (pos, normal, BGRA colour, uv) and
/// its textures by 16-byte name.
#[derive(Clone, Debug)]
pub struct D3m {
    pub format: u8,
    pub textures: Vec<String>,
    /// Triangles per texture (u16s at +8 in every format, FUN_1003fce0).
    pub per_texture: Vec<u16>,
    /// Header byte 5 (res+0x35): the triangles after the textured runs are drawn untextured.
    pub untextured_tail: bool,
    pub vertices: Vec<D3mVertex>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct D3mVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub bgra: [u8; 4],
    pub uv: [f32; 2],
}

impl D3m {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.len() < 8 {
            bail!("d3m: {} bytes", d.len());
        }
        let format = (d[0] & 0xf) as u8;
        let (n, m, tris) = (d[4] as usize, d[5] as usize, u16::from_le_bytes([d[6], d[7]]) as usize);
        let counts: Vec<u16> = (0..n).map(|k| d.get(8 + 2 * k..10 + 2 * k).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))).collect();
        let table = if (5..=7).contains(&format) {
            if format == 7 && n == 0 {
                8
            } else if (n + m) & 3 != 0 {
                8 + ((n + m) & !3) * 2 + 6
            } else {
                8 + (n + m) * 2
            }
        } else {
            0x10
        };
        let textures: Vec<String> = (0..n).map(|k| d.get(table + 16 * k..table + 16 * k + 16).map_or(String::new(), |b| String::from_utf8_lossy(b).into_owned())).collect();
        let first = if (5..=7).contains(&format) { table + 16 * n } else { 0x50 };
        let need = first + tris * 3 * 36;
        if need > d.len() {
            bail!("d3m: {tris} triangles need {need} bytes, have {}", d.len());
        }
        let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let vertices = (0..tris * 3)
            .map(|k| {
                let o = first + 36 * k;
                D3mVertex { pos: [f(o), f(o + 4), f(o + 8)], normal: [f(o + 12), f(o + 16), f(o + 20)], bgra: d[o + 24..o + 28].try_into().unwrap(), uv: [f(o + 28), f(o + 32)] }
            })
            .collect();
        Ok(Self { format, textures, per_texture: counts, untextured_tail: m != 0, vertices })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(id: u32) -> Option<Vec<u8>> {
        crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?.read(id).ok()
    }

    /// Zone 232 (DAT 332) effe/sea sea1 (spec E.1): world, auto-start, one persistent element; op 1 names 'sea1',
    /// which is no D3m but the MMB in the same folder; op 0x63 binds keyframe 'seen'.
    #[test]
    fn port_san_doria_sea() {
        let Some(file) = read(332) else { return };
        let tree = Tree::build(&file).unwrap();
        let trim = |n: &[u8; 4]| String::from_utf8_lossy(n).trim_end_matches(['\0', ' ']).to_string();
        let node = tree.of_kind(kind::GENERATOR).find(|&i| tree.nodes[i].name == fourcc("sea1") && tree.path(i).iter().map(trim).collect::<Vec<_>>().ends_with(&["effe".to_string(), "sea".to_string()])).unwrap();
        let g = Generator::parse(&tree, node, &file).unwrap();
        assert_eq!((g.attach_mode(), g.life, g.flags & 0x1000), (0, 0.0, 0x1000));
        let t = g.template().unwrap();
        assert_eq!((t.byte(0x21), fourcc_u32(t.u32(3)), t.i16_at(0x22)), (0x0b, fourcc("sea1"), 0));
        assert!(t.f32(5).abs() < 1e-4 && (t.f32(6) - 1.5).abs() < 1e-4 && (t.f32(7) - 100.0).abs() < 1e-4);
        let codes: Vec<u8> = g.streams[stream::EMISSION].iter().map(|o| o.code).collect();
        assert_eq!(codes, [0x01, 0x30, 0x09, 0x0f, 0x16, 0x63]);
        let codes: Vec<u8> = g.streams[stream::ELEMENT].iter().map(|o| o.code).collect();
        assert_eq!(codes, [0x2e, 0x0e, 0x27, 0x28, 0x3f]);
        assert!(tree.find(node, kind::D3M, fourcc("sea1")).is_none());
        let mmb = tree.find(node, kind::MMB, fourcc("sea1")).unwrap();
        assert_eq!(tree.nodes[mmb].parent, tree.nodes[node].parent);
        let seen = tree.find(node, kind::KEYFRAME, fourcc("seen")).unwrap();
        let k = KeyFrame::parse(&file[tree.nodes[seen].data.clone()]);
        assert_eq!(k.keys.len(), 6);
        assert!((k.linear(0.5) - 1.172).abs() < 1e-3 && (k.linear(0.0) - 0.703).abs() < 1e-3);
        assert!((k.linear(0.275) - (0.703 + (1.172 - 0.703) * 0.5)).abs() < 1e-3);
    }

    /// Spell file 2944 (fai0, spec D.6): 8 generators; g003 draws D3m fa04 (format 6, one texture, a quad); g005
    /// names hi17, which is not in the file (it resolves from DAT 0).
    #[test]
    fn fire_effect_file() {
        let Some(file) = read(2944) else { return };
        let tree = Tree::build(&file).unwrap();
        let gens: Vec<Generator> = tree.of_kind(kind::GENERATOR).map(|i| Generator::parse(&tree, i, &file).unwrap()).collect();
        assert_eq!(gens.len(), 8);
        let g003 = gens.iter().find(|g| g.name == fourcc("g003")).unwrap();
        let t = g003.template().unwrap();
        assert_eq!((t.byte(0x21), fourcc_u32(t.u32(3)), t.i16_at(0x22)), (0x0b, fourcc("fa04"), 90));
        let fa04 = tree.find(g003.node, kind::D3M, fourcc("fa04")).unwrap();
        let m = D3m::parse(&file[tree.nodes[fa04].data.clone()]).unwrap();
        assert_eq!((m.format, m.textures.len(), m.vertices.len()), (6, 1, 6));
        let g005 = gens.iter().find(|g| g.name == fourcc("g005")).unwrap();
        assert!(tree.find(g005.node, kind::D3M, fourcc_u32(g005.template().unwrap().u32(3))).is_none());
    }

    #[test]
    fn keyframe_samplers() {
        let k = KeyFrame { keys: vec![(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)] };
        assert_eq!(k.linear(0.25), 0.5);
        assert_eq!(k.linear(2.0), 0.0);
        // Eased: a quarter of the way into a pair is 2 x 0.5^2 = 0.5 of the rise.
        assert!((k.eased(0.125) - 0.125).abs() < 1e-6);
        assert!((k.eased(0.375) - 0.875).abs() < 1e-6);
    }
}
