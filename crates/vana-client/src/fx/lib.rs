//! One effect DAT as the generator system sees it: its folder tree, generators, keyframes and the meshes, sprites
//! and textures their elements draw. Meshes, sprites and textures are parsed only when a generator names them.

use crate::dat::generator::{D3m, Generator, KeyFrame, Tree, fourcc_u32, kind};
use crate::dat::zone;
use anyhow::{Result, bail};
use std::collections::HashMap;

/// A vertex as every effect mesh is drawn: the D3m layout (FVF 0x152), colour as RGBA bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub rgba: [u8; 4],
    pub uv: [f32; 2],
}

/// A run of triangles with one texture: its 16-byte IMG name (None = untextured) and, once resolved, the
/// texture as (library, index).
#[derive(Clone, Debug)]
pub struct MeshGroup {
    pub name: Option<String>,
    pub texture: Option<(usize, usize)>,
    pub range: std::ops::Range<u32>,
}

/// A triangle-list mesh: a D3m, or an MMB taken in its place (FUN_100485c0's fallback; drawn by the model
/// renderer FUN_1017c840, element +0x179 = 1).
#[derive(Clone, Debug)]
pub struct Mesh {
    pub verts: Vec<MeshVertex>,
    pub groups: Vec<MeshGroup>,
    pub mmb: bool,
}

/// A D3a sprite animation (chunk 0x21, FUN_1003d870; d3a_particles.md 1): frames of quads as triangle lists, one
/// texture. `frames` = (first vertex, quad count).
#[derive(Clone, Debug)]
pub struct Sprite {
    pub verts: Vec<MeshVertex>,
    pub frames: Vec<(u32, u32)>,
    pub name: String,
    pub texture: Option<(usize, usize)>,
    /// h0 >= 1 and byte 7 == 0: UVs are in texels of the loaded texture (divided once it resolves).
    pub texel_uv: bool,
    /// The lens-flare layout (byte 4 != 0, FUN_1003ddb0): one frame of every group's quads, each vertex's group t
    /// in normal.x.
    pub flare: bool,
}

impl Sprite {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.len() < 0x18 {
            bail!("d3a: {} bytes", d.len());
        }
        let h0 = i16::from_le_bytes([d[0], d[1]]);
        let count = i16::from_le_bytes([d[2], d[3]]).max(0) as usize;
        let name = String::from_utf8_lossy(&d[8..24]).into_owned();
        let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        if d[4] != 0 {
            // Groups (count byte 5): {u8, u8, quads, u8, f32 t, 12 bytes} then quads x 6 vertices.
            let mut verts = Vec::new();
            let mut at = 0x18;
            for _ in 0..d[5] {
                if at + 0x14 > d.len() {
                    break;
                }
                let (q, t) = (d[at + 2] as usize, f(at + 4));
                at += 0x14;
                if at + q * 0x90 > d.len() {
                    bail!("d3a flare: group out of range");
                }
                for k in 0..q * 6 {
                    let o = at + 24 * k;
                    let c = &d[o + 12..o + 16];
                    verts.push(MeshVertex { pos: [f(o), f(o + 4), f(o + 8)], normal: [t, 0.0, 0.0], rgba: [c[2], c[1], c[0], c[3]], uv: [f(o + 16), f(o + 20)] });
                }
                at += q * 0x90;
            }
            let quads = (verts.len() / 6) as u32;
            return Ok(Self { verts, frames: vec![(0, quads)], name, texture: None, texel_uv: false, flare: true });
        }
        let mut verts = Vec::new();
        let mut frames = Vec::new();
        let mut at = 0x18;
        for _ in 0..count {
            if at + 4 > d.len() {
                break;
            }
            let q = d[at + 2] as usize;
            at += 4;
            if at + q * 0x90 > d.len() {
                bail!("d3a: frame out of range");
            }
            frames.push((verts.len() as u32, q as u32));
            for k in 0..q * 6 {
                let o = at + 24 * k;
                let c = &d[o + 12..o + 16];
                verts.push(MeshVertex { pos: [f(o), f(o + 4), f(o + 8)], normal: [0.0, 0.0, 1.0], rgba: [c[2], c[1], c[0], c[3]], uv: [f(o + 16), f(o + 20)] });
            }
            at += q * 0x90;
        }
        Ok(Self { verts, frames, name, texture: None, texel_uv: h0 >= 1 && d[7] == 0, flare: false })
    }
}

pub struct Library {
    pub file_id: u32,
    pub tree: Tree,
    pub gens: Vec<Generator>,
    pub gen_at: HashMap<usize, usize>,
    pub keys: HashMap<usize, KeyFrame>,
    pub meshes: HashMap<usize, Mesh>,
    pub sprites: HashMap<usize, Sprite>,
    pub textures: Vec<zone::Texture>,
    tex_of_node: HashMap<usize, usize>,
    file: Vec<u8>,
}

/// IMG names compared as stored: 16 bytes (NULs and spaces kept apart only at the end).
pub fn tex_name(raw: &str) -> String {
    raw.trim_end_matches(['\0', ' ']).to_string()
}

impl Library {
    pub fn load(file_id: u32, file: Vec<u8>) -> Result<Self> {
        let tree = Tree::build(&file)?;
        let mut gens = Vec::new();
        let mut gen_at = HashMap::new();
        for i in tree.of_kind(kind::GENERATOR) {
            match Generator::parse(&tree, i, &file) {
                Ok(g) => {
                    gen_at.insert(i, gens.len());
                    gens.push(g);
                }
                Err(e) => log::warn!("effects: DAT {file_id}: {e:#}"),
            }
        }
        let keys = tree.of_kind(kind::KEYFRAME).map(|i| (i, KeyFrame::parse(&file[tree.nodes[i].data.clone()]))).collect();
        Ok(Self { file_id, tree, gens, gen_at, keys, meshes: HashMap::new(), sprites: HashMap::new(), textures: Vec::new(), tex_of_node: HashMap::new(), file })
    }

    pub fn generator(&self, node: usize) -> Option<&Generator> {
        self.gen_at.get(&node).map(|&g| &self.gens[g])
    }

    /// The node's payload.
    pub fn data(&self, node: usize) -> &[u8] {
        &self.file[self.tree.nodes[node].data.clone()]
    }

    /// Parses the mesh at `node` (a D3m, or an MMB with the zone keys) once; textures stay names until the
    /// registry resolves them.
    pub fn mesh(&mut self, node: usize, keys: &zone::Keys) -> Option<&mut Mesh> {
        if !self.meshes.contains_key(&node) {
            let mesh = match self.tree.nodes[node].kind {
                kind::D3M => D3m::parse(self.data(node)).map(|m| from_d3m(&m)),
                kind::MMB => {
                    let mut d = self.data(node).to_vec();
                    zone::decode_mmb(keys, &mut d);
                    // Type 2 (48-byte vertices) draws nothing on the effect path (its VB stays empty).
                    if d.get(4) == Some(&2) {
                        Ok(Mesh { verts: Vec::new(), groups: Vec::new(), mmb: true })
                    } else {
                        zone::parse_mmb(&d).map(|m| from_mmb(&m))
                    }
                }
                _ => return None,
            };
            match mesh {
                Ok(m) => {
                    self.meshes.insert(node, m);
                }
                Err(e) => {
                    log::warn!("effects: DAT {} mesh {}: {e:#}", self.file_id, self.tree.nodes[node].name());
                    return None;
                }
            }
        }
        self.meshes.get_mut(&node)
    }

    pub fn sprite(&mut self, node: usize) -> Option<&mut Sprite> {
        if !self.sprites.contains_key(&node) {
            match Sprite::parse(self.data(node)) {
                Ok(s) => {
                    self.sprites.insert(node, s);
                }
                Err(e) => {
                    log::debug!("effects: DAT {} sprite {}: {e:#}", self.file_id, self.tree.nodes[node].name());
                    return None;
                }
            }
        }
        self.sprites.get_mut(&node)
    }

    /// Loader-thread work: the meshes, sprites and textures the file's own generators name (op 1), so the first
    /// frame that emits them does not parse them.
    pub fn prewarm(&mut self, keys: &zone::Keys) {
        let wanted: Vec<(usize, u8, u32)> = self.gens.iter().filter_map(|g| g.template().map(|o| (g.node, o.byte(0x21), o.u32(3)))).collect();
        for (from, k, name) in wanted {
            let node = match k {
                0x0b | 0x0c | 0x1f => self.find(from, kind::D3M, name).or_else(|| self.find(from, kind::MMB, name)),
                0x0e | 0x21 | 0x39 => self.find(from, kind::D3A, name),
                _ => None,
            };
            let Some(node) = node else { continue };
            let names: Vec<String> = match self.tree.nodes[node].kind {
                kind::D3A => self.sprite(node).map(|s| vec![s.name.clone()]).unwrap_or_default(),
                _ => self.mesh(node, keys).map(|m| m.groups.iter().filter_map(|g| g.name.clone()).collect()).unwrap_or_default(),
            };
            for n in names {
                if let Some(i) = self.find_texture(node, &n) {
                    self.texture_at(i);
                }
            }
        }
    }

    /// FUN_1003a0d0: the IMG whose 16-byte name matches, from the folder holding `node`: the first IMG of that
    /// subtree (file order), then the rest of the folder that IMG is in.
    pub fn find_texture(&self, node: usize, name16: &str) -> Option<usize> {
        let want = tex_name(name16);
        let folder = if self.tree.nodes[node].kind == kind::FOLDER { node } else { self.tree.nodes[node].parent? };
        let mut sub = Vec::new();
        self.walk(folder, &mut sub);
        let first = *sub.iter().find(|&&i| self.tree.nodes[i].kind == kind::IMG)?;
        let mut rest = Vec::new();
        match self.tree.nodes[first].parent {
            Some(p) => self.walk(p, &mut rest),
            None => rest = (0..self.tree.nodes.len()).filter(|&i| self.tree.nodes[i].parent.is_none()).collect(),
        }
        let from = rest.iter().position(|&i| i == first)?;
        rest[from..].iter().copied().find(|&i| self.tree.nodes[i].kind == kind::IMG && self.img_name(i) == want)
    }

    /// The fallback's candidates: IMGs by their last 8 name bytes (FUN_10074570 step 2).
    pub fn find_texture_tail(&self, name16: &str) -> Option<usize> {
        let tail = |s: &str| s.as_bytes().get(8..16).map(|b| tex_name(&String::from_utf8_lossy(b)));
        let want = tail(&format!("{name16:<16}"))?;
        self.tree.of_kind(kind::IMG).find(|&i| tail(&format!("{:<16}", self.img_name(i))).as_ref() == Some(&want))
    }

    fn img_name(&self, i: usize) -> String {
        self.data(i).get(1..17).map(|b| tex_name(&String::from_utf8_lossy(b))).unwrap_or_default()
    }

    /// A folder's descendants in file order.
    fn walk(&self, folder: usize, out: &mut Vec<usize>) {
        for &c in &self.tree.nodes[folder].children {
            out.push(c);
            if self.tree.nodes[c].kind == kind::FOLDER {
                self.walk(c, out);
            }
        }
    }

    /// The texture of IMG node `i`, parsed once.
    pub fn texture_at(&mut self, i: usize) -> Option<usize> {
        if let Some(&t) = self.tex_of_node.get(&i) {
            return Some(t);
        }
        match zone::parse_img(self.data(i)) {
            Ok(t) => {
                let k = self.textures.len();
                self.textures.push(t);
                self.tex_of_node.insert(i, k);
                Some(k)
            }
            Err(e) => {
                log::warn!("effects: DAT {} texture: {e:#}", self.file_id);
                None
            }
        }
    }

    /// FUN_10071310 from the generator's folder, by type and 4cc.
    pub fn find(&self, from: usize, kind: u8, name: u32) -> Option<usize> {
        self.tree.find(from, kind, fourcc_u32(name))
    }

    /// The whole file from its root (how DAT 0 is searched once a file's own scope runs out).
    pub fn find_global(&self, kind: u8, name: u32) -> Option<usize> {
        let n = fourcc_u32(name);
        self.tree.nodes.iter().position(|x| x.kind == kind && (name == 0 || x.name == n))
    }

    /// Whether IMG node `i` sits in a `syst/effe` folder (texture entry flag 0x100, FUN_1001cab0).
    pub fn in_syst_effe(&self, i: usize) -> bool {
        let p = self.tree.nodes[i].parent;
        p.is_some_and(|p| self.tree.nodes[p].name == crate::dat::generator::fourcc("effe") && self.tree.nodes[p].parent.is_some_and(|q| self.tree.nodes[q].name == crate::dat::generator::fourcc("syst")))
    }
}

/// FUN_1003f3e0's groups: formats 5..7 give triangles per texture (textured first, then the untextured tail);
/// other formats draw everything untextured.
fn from_d3m(m: &D3m) -> Mesh {
    let verts: Vec<MeshVertex> = m.vertices.iter().map(|v| MeshVertex { pos: v.pos, normal: v.normal, rgba: [v.bgra[2], v.bgra[1], v.bgra[0], v.bgra[3]], uv: v.uv }).collect();
    let total = verts.len() as u32;
    let mut groups = Vec::new();
    let mut at = 0u32;
    for (k, &tris) in m.per_texture.iter().enumerate().take(m.textures.len()) {
        let end = (at + tris as u32 * 3).min(total);
        if end > at {
            groups.push(MeshGroup { name: m.textures.get(k).cloned(), texture: None, range: at..end });
        }
        at = end;
    }
    if at < total {
        groups.push(MeshGroup { name: None, texture: None, range: at..total });
    }
    Mesh { verts, groups, mmb: false }
}

/// The MMB node of FUN_10168eb0: every model de-indexed into a triangle list with its texture.
fn from_mmb(m: &zone::Mmb) -> Mesh {
    let mut verts = Vec::new();
    let mut groups = Vec::new();
    for model in &m.models {
        let indices = if model.strip { strip_to_list(&model.indices) } else { model.indices.clone() };
        let start = verts.len() as u32;
        for &i in &indices[..indices.len() / 3 * 3] {
            let Some(v) = model.vertices.get(i as usize) else { continue };
            let [b, g, r, a] = v.color.to_le_bytes();
            verts.push(MeshVertex { pos: v.pos, normal: v.normal, rgba: [r, g, b, a], uv: v.uv });
        }
        let end = verts.len() as u32;
        if end > start {
            groups.push(MeshGroup { name: Some(model.texture.clone()), texture: None, range: start..end });
        }
    }
    Mesh { verts, groups, mmb: true }
}

/// Strip to list, dropping the degenerate triangles that join strips (as render/mod.rs does for scenery).
fn strip_to_list(strip: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(strip.len() * 3);
    for i in 0..strip.len().saturating_sub(2) {
        let (a, b, c) = (strip[i], strip[i + 1], strip[i + 2]);
        if a == b || b == c || a == c {
            continue;
        }
        if i % 2 == 0 { out.extend([a, b, c]) } else { out.extend([b, a, c]) }
    }
    out
}
