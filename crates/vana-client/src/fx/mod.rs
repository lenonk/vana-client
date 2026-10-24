//! The effect generator system (FFXiMain's CMoGenerator / CMoElem; spec ~/ffxi/lab/research/effects/). The CPU
//! runs generators and builds elements (emit.rs); the GPU runs each element's update stream every frame and
//! draws it (render/effects.rs). Sort keys and expire requests come back from the GPU a frame late.

pub mod elem;
pub mod emit;
pub mod lib;
pub mod math;
pub mod sched;
pub mod sound;
pub mod spline;
mod tasks;
pub use tasks::event_action_file;

use crate::dat::generator::{KeyFrame, Op, kind};
use crate::dat::zone;
use elem::{Elem, NONE, SLOT_WORDS, ctl, slot, w};
use glam::{Mat4, Vec3};
use lib::Library;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// (library, tree node).
pub type Ref = (usize, usize);

/// What the effect system asks of the world's actors (CXiActor vtable +0x1c4, +0x1ec, FUN_100cc440).
pub trait Actors {
    /// Placement: rotation and translation (model space faces +x).
    fn matrix(&self, id: u32) -> Option<Mat4>;
    /// Attach point `k` in model space, model scale included.
    fn point(&self, id: u32, k: u16) -> Option<Vec3>;
    /// The size the scale vectors read (vtable +0x98 / +0x9c *(inf: the model scale)*).
    fn scale(&self, id: u32) -> f32;
    /// Equipment part files (vtable +0x1ec), and the motion files newest first followed by the model's files.
    fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>);
    /// Hidden actors (+0xb2 bit 1) get no attached effects.
    fn hidden(&self, id: u32) -> bool;
    /// Actors whose model is up, with the model's files (FUN_10083970 reads their generators).
    fn models(&self) -> Vec<(u32, Vec<u32>)>;
    /// PC skeletons (CXiSkeletonActor): race code and body info byte 9 (+0x881).
    fn pc(&self, _id: u32) -> Option<(u8, i8)> {
        None
    }
    /// The last foot contact in the world (actor+0x63c, attach point 0x34).
    fn foot(&self, _id: u32) -> Option<Vec3> {
        None
    }
    /// The floor normal under it, pointing into the floor (-vtable +0x210).
    fn floor(&self, _id: u32) -> Option<Vec3> {
        None
    }
    /// Emission op 0x8f: the tint of the area environment the actor stands in (None = the zone's).
    fn area_light(&self, _id: u32) -> Option<Vec3> {
        None
    }
    /// FUN_100ac560's class of one actor: 1 you (or your pet), 2 monster, 4 party, 8 not in your party.
    fn fx_class(&self, _id: u32) -> u8 {
        8
    }
    /// Moving this frame (actor +0xf8, FUN_1005e5d0).
    fn moving(&self, _id: u32) -> bool {
        false
    }
    /// The model's info bytes +0x878..+0x87a: leg mode, footstep kind, shoe.
    fn cib(&self, _id: u32) -> Option<[i8; 3]> {
        None
    }
    /// The model's sub-animation (+0x8a5, vtable +0xa4): the `shi<n>` / `ska<n>` sound and swing letter variants.
    fn sub_anim(&self, _id: u32) -> u8 {
        0
    }
    /// Our own actor (FUN_10081550), for op 0x53's nearest target.
    fn player(&self) -> Option<u32> {
        None
    }
    /// Actor +0x886 (vtable +0x3d8 byte 0xe): the range part file's class, -1 none.
    fn ranged_class(&self, _id: u32) -> Option<i8> {
        None
    }
    /// Vtable +0x124 (entity +0x130 bit 1, op 0x9a): the model file's 0x1000 generators stay off.
    fn model_gens_off(&self, _id: u32) -> bool {
        false
    }
    /// FUN_100d4600's choice: the actor's swing schedulers, `atf?` moving and `ati?` standing (the other set when
    /// that one is empty); None while its model is not in.
    fn swings(&self, _id: u32, _moving: bool) -> Option<Vec<String>> {
        None
    }
}

/// No actors (offscreen shots).
pub struct NoActors;

impl Actors for NoActors {
    fn matrix(&self, _: u32) -> Option<Mat4> {
        None
    }
    fn point(&self, _: u32, _: u16) -> Option<Vec3> {
        None
    }
    fn scale(&self, _: u32) -> f32 {
        1.0
    }
    fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
        (Vec::new(), Vec::new())
    }
    fn hidden(&self, _: u32) -> bool {
        false
    }
    fn models(&self) -> Vec<(u32, Vec<u32>)> {
        Vec::new()
    }
}

/// What an element draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Draw {
    None,
    /// A registered mesh (D3m, or an MMB in its place).
    Mesh(u32),
    /// A registered D3a sprite animation.
    Sprite(u32),
    /// A registered D3b morph.
    Morph(u32),
    /// CMoDist (kind 0x22): the background under a diamond redrawn shifted (heat haze).
    Dist,
    /// DamValueProg (kind 0x5c): a damage number's glyphs over its target (render/effects.rs vs_damv).
    DamValue,
}

/// A damage number for kind 0x5c elements: glyphs ('0'..'9' = 0..9, ':' = 10, up to 16) and the message colour
/// 0xRRGGBB (elem +0x198 / +0x194).
#[derive(Clone, Debug, PartialEq)]
pub struct DamValue {
    pub glyphs: Vec<u8>,
    pub rgb: u32,
}

/// This frame's inputs.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Ticks of 1/60 s (DAT_10469678+0xeb0, at least 1 per frame in retail) and seconds (+0xeac).
    pub dt: f32,
    pub seconds: f32,
    /// Vana'diel day fraction DAT_106175bc.
    pub dayfrac: f32,
    pub view: Mat4,
    /// The view with its translation zeroed (DAT_10469678+0xb90).
    pub view_rot: Mat4,
    pub eye: Vec3,
    pub draw_distance: f32,
    /// The brighter of sun + ambient and moon + ambient (ops 0x8f / 0x90), 1.0 = unchanged.
    pub area_light: Vec3,
    /// Camera focal length on the 384-line reference (cam+0x2f4: fovy = 2 atan(192 / focal)), for sound pan.
    pub focal: f32,
    /// FUN_10184dc0: the moon phase 0..11; FUN_10184d70: the weekday 0..7.
    pub moon_phase: u32,
    pub weekday: u32,
}

/// The CPU side of a live element (the GPU holds the rest).
#[derive(Clone, Debug)]
pub struct ElemCpu {
    pub inst: usize,
    pub serial: u32,
    pub life: f32,
    pub max_life: f32,
    pub flags: u32,
    pub blend: u16,
    pub draw: Draw,
    /// Sort key +0x12c from the GPU (the bias until the first readback).
    pub key: f32,
    pub drawn: bool,
    pub back: Option<emit::Parent>,
    /// Generator instance linked to this element (+0x164).
    pub child: Option<usize>,
    /// Flag 0x800000: the emitter frame at spawn, in its own matrix slot.
    pub snapshot: Option<Mat4>,
    pub mslot: u32,
    /// Batched D3a: its particles in the pool (first, count).
    pub parts: Option<(u32, u32)>,
    /// A point light's table slot.
    pub light: Option<u32>,
}

/// Op 0x25 (reactor actor2) / 0x21 (actor1): amount f32 @8, rate @0xc, count u32 @0x10, blend-in ticks @0x14, fade
/// @0x18, side u32 @0x1c (0 by position, 1 front, 2 back); level = the current record's scale & 3.
#[derive(Clone, Copy, Debug)]
pub struct React {
    pub reactor: u32,
    pub other: u32,
    pub amount: f32,
    pub rate: f32,
    pub count: u32,
    pub blend_in: f32,
    pub fade: f32,
    pub side: u32,
    pub level: u8,
}

/// Op 0x5a (reactor actor1) / 0x5b (actor2): guard (`gdi?` / `gdm?`) or parry (`pyi?` / `pym?`) with rate f32 @8,
/// count i16 @0xc, parry u16 @0xe, blend-in f32 @0x10, fade f32 @0x14.
#[derive(Clone, Copy, Debug)]
pub struct Guard {
    pub reactor: u32,
    pub parry: bool,
    pub rate: f32,
    pub count: i16,
    pub blend_in: f32,
    pub fade: f32,
}

/// Op 0x5e / 0xbf: push `reactor` away from `from` (the pusher's position; toward it when `speed` < 0) at `speed`
/// units a tick for `ticks`, then decaying by `decay` a tick; `motion` "bf0" / "bb0" with `count` (0 loops).
#[derive(Clone, Debug)]
pub struct Knock {
    pub reactor: u32,
    pub from: Vec3,
    pub speed: f32,
    pub decay: f32,
    pub ticks: f32,
    pub motion: String,
    pub count: u8,
}

/// Scheduler op 0x0c (translate) / 0x0d (rotate) on a door (FUN_10043bb0 / FUN_100439c0): part `part` of actor1's
/// door group goes linearly to `to` over `ticks`.
#[derive(Clone, Debug)]
pub struct DoorMove {
    pub actor: u32,
    pub rotate: bool,
    pub part: u32,
    pub to: [f32; 3],
    pub ticks: f32,
}

/// Scheduler op 0x1d (FUN_10057fb0 case 0x1d): actor1, a lift, goes to `floor` (the running scheduler's name byte 3,
/// FUN_100c3ce0) by its packet's end time (client/lifts.rs).
#[derive(Clone, Copy, Debug)]
pub struct LiftMove {
    pub actor: u32,
    pub floor: u8,
}

/// Who started an instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Owner {
    Zone,
    /// The loaded sub-area's auto-start generators.
    SubArea,
    /// The weather set (index into Effects::weathers' serials).
    Weather(u32),
    Element(usize),
    /// A scheduler task (op 0x02).
    Task(usize),
    /// An actor's model files (FUN_10083970).
    Actor(u32),
    /// A footstep's dust burst on an actor (FUN_100d41a0).
    Step(u32),
}

/// A running generator (the CMoGenerator object, or a CMoGeneratorClone of it): its own copy of the streams
/// (generator-update ops rewrite emission operands) and the FUN_10048740 counters.
pub struct Instance {
    pub src: Ref,
    pub name: [u8; 4],
    pub streams: [Vec<Op>; 4],
    /// +0xd8.
    pub flags: u32,
    /// +0xdc bit 0: soft-stopped (no emission; live elements finish).
    pub stopping: bool,
    /// +0xde weather bits.
    pub de: u8,
    pub life: f32,
    pub elapsed: f32,
    pub countdown: f32,
    pub interval: i16,
    pub interval_rand: i16,
    /// +0x9c.
    pub emitted: u32,
    pub mode: u8,
    /// +0xa4 attach matrix (None = world).
    pub matrix: Option<Mat4>,
    /// +0x78 / +0x84 scale vectors, +0x90 two-point distance.
    pub scale_a: Vec3,
    pub scale_b: Vec3,
    pub dist: f32,
    pub size_scaled: bool,
    pub mslot: u32,
    pub weather: f32,
    pub live: Vec<u32>,
    pub finished: bool,
    pub owner: Owner,
    pub parent_elem: Option<u32>,
    /// Actor1 / actor2 (gen +0x30: +0x3c / +0x40).
    pub actors: [sched::Who; 2],
    /// A clone linked to an element (+0x164): not a task of its own; the element runs it (FUN_10048740 from
    /// emission ops 0x44 / 0x53 / 0x6a once, then update ops 0x25 / 0x33 / 0x46 every frame).
    pub driven: Option<u32>,
    /// Emission op 0x8e: the source actor's foot contact and ground frame (FUN_100d3750), when the stream has it.
    pub ground: Option<(Vec3, [Vec3; 3])>,
    /// Emission op 0x8f: the source actor's area tint, when the stream has it and the actor is in an area.
    pub area_light: Option<Vec3>,
    /// The damage number of the task that started it (gen +0x64 -> task +0xa8 / +0xa4); clones inherit it.
    pub dam: Option<DamValue>,
}

/// A word range of an element for the GPU this frame.
pub enum Write {
    Full(u32, Box<Elem>),
    Words(u32, usize, Vec<u32>),
    /// Particle records from this index of the pool.
    Parts(u32, Vec<[f32; 9]>),
    /// A point light switched off (the CMoPointLightProgElem dtor: a black light).
    LightOff(u32),
}

/// Libraries and the tables the GPU reads (keyframes, update streams, meshes).
pub struct Registry {
    pub libs: Vec<Library>,
    by_file: HashMap<u32, usize>,
    /// DAT 0, searched once a file's own scope runs out.
    pub global: Option<usize>,
    zone_keys: Option<Arc<zone::Keys>>,
    /// Keyframes: (start, count) into `key_data`, by global id.
    pub key_data: Vec<[f32; 2]>,
    pub key_desc: Vec<[u32; 2]>,
    key_id: HashMap<Ref, u32>,
    pub keys_dirty: bool,
    /// The zone's point-light table keys by slot (render ZoneGeometry::light_keys).
    pub light_keys: Vec<u32>,
    /// Vtx paths as splines (fx/spline.rs gpu_words), back to back; an element slot holds first vec4 + 1.
    pub path_data: Vec<[f32; 4]>,
    path_id: HashMap<Ref, (u32, Vec<spline::Seg>)>,
    pub paths_dirty: bool,
    /// Element update streams back to back (op words).
    pub ops: Vec<u32>,
    stream_at: HashMap<Ref, (u32, Option<usize>)>,
    pub ops_dirty: bool,
    /// Meshes and sprites by GPU id; ids not yet uploaded wait in `new_*`.
    pub meshes: Vec<Ref>,
    mesh_id: HashMap<Ref, u32>,
    pub new_meshes: Vec<u32>,
    pub sprites: Vec<Ref>,
    sprite_id: HashMap<Ref, u32>,
    pub new_sprites: Vec<u32>,
    /// Every sprite's vertices (MeshVertex words) and frames (first vertex, quads) back to back for the GPU; per
    /// sprite its first frame entry and most quads in a frame.
    pub sprite_verts: Vec<[u32; 9]>,
    pub sprite_frame_table: Vec<[u32; 2]>,
    pub sprite_at: Vec<(u32, u32)>,
    pub sprites_dirty: bool,
    /// Morphs: their GPU words back to back ([K, V, N, T], corners, key positions, key normals) and first word.
    pub morphs: Vec<Ref>,
    morph_id: HashMap<Ref, u32>,
    pub morph_data: Vec<u32>,
    pub morph_at: Vec<u32>,
    pub morphs_dirty: bool,
    pub new_morphs: Vec<u32>,
    zone: Option<usize>,
    logged: HashSet<(u8, u8)>,
    missed: HashSet<(usize, u32)>,
}

impl Registry {
    fn new() -> Self {
        Self {
            libs: Vec::new(),
            by_file: HashMap::new(),
            global: None,
            zone_keys: None,
            key_data: Vec::new(),
            key_desc: Vec::new(),
            key_id: HashMap::new(),
            keys_dirty: false,
            light_keys: Vec::new(),
            path_data: Vec::new(),
            path_id: HashMap::new(),
            paths_dirty: false,
            ops: vec![0],
            stream_at: HashMap::new(),
            ops_dirty: true,
            meshes: Vec::new(),
            mesh_id: HashMap::new(),
            new_meshes: Vec::new(),
            sprites: Vec::new(),
            sprite_id: HashMap::new(),
            new_sprites: Vec::new(),
            sprite_verts: Vec::new(),
            sprite_frame_table: Vec::new(),
            sprite_at: Vec::new(),
            sprites_dirty: false,
            morphs: Vec::new(),
            morph_id: HashMap::new(),
            morph_data: Vec::new(),
            morph_at: Vec::new(),
            morphs_dirty: false,
            new_morphs: Vec::new(),
            zone: None,
            logged: HashSet::new(),
            missed: HashSet::new(),
        }
    }

    pub fn lib_of(&self, file_id: u32) -> Option<usize> {
        self.by_file.get(&file_id).copied()
    }

    fn add(&mut self, l: Library) -> usize {
        let i = self.libs.len();
        self.by_file.insert(l.file_id, i);
        if l.file_id == 0 {
            self.global = Some(i);
        }
        self.libs.push(l);
        i
    }

    /// FUN_100712e0: the file's folder scope, then DAT 0.
    pub fn resolve(&self, lib: usize, from: usize, kind: u8, name: u32) -> Option<Ref> {
        if let Some(n) = self.libs[lib].find(from, kind, name) {
            return Some((lib, n));
        }
        let g = self.global.filter(|&g| g != lib)?;
        self.libs[g].find_global(kind, name).map(|n| (g, n))
    }

    pub fn keyframe(&self, r: Ref) -> Option<&KeyFrame> {
        self.libs[r.0].keys.get(&r.1)
    }

    /// A keyframe's GPU id by 4cc from the generator's scope (NONE when it does not resolve).
    pub fn key_ref(&mut self, lib: usize, from: usize, name: u32) -> u32 {
        let Some(r) = self.resolve(lib, from, kind::KEYFRAME, name) else { return NONE };
        if let Some(&id) = self.key_id.get(&r) {
            return id;
        }
        let Some(k) = self.keyframe(r) else { return NONE };
        let start = self.key_data.len() as u32;
        let keys = k.keys.clone();
        self.key_data.extend(keys.iter().map(|&(t, v)| [t, v]));
        let id = self.key_desc.len() as u32;
        self.key_desc.push([start, keys.len() as u32]);
        self.key_id.insert(r, id);
        self.keys_dirty = true;
        id
    }

    /// A Vtx path by 4cc from the generator's scope: its GPU reference (first vec4 + 1) and p(0); None when it does
    /// not resolve or has fewer than two points.
    pub fn path_ref(&mut self, lib: usize, from: usize, name: u32) -> Option<(u32, Vec3)> {
        let r = self.resolve(lib, from, kind::VTX, name)?;
        if !self.path_id.contains_key(&r) {
            let segs = spline::build(&spline::points(self.libs[r.0].data(r.1)))?;
            let at = self.path_data.len() as u32 + 1;
            self.path_data.extend(spline::gpu_words(&segs));
            self.path_id.insert(r, (at, segs));
            self.paths_dirty = true;
        }
        let (at, segs) = &self.path_id[&r];
        Some((*at, spline::eval(segs, 0.0)))
    }

    /// The generator's element update stream in the ops buffer: (first word, slot of its op 0x02).
    pub fn stream(&mut self, src: Ref) -> (u32, Option<usize>) {
        if let Some(&s) = self.stream_at.get(&src) {
            return s;
        }
        let Some(g) = self.libs[src.0].generator(src.1) else { return (0, None) };
        let at = self.ops.len() as u32;
        let mut op2 = None;
        for op in &g.streams[2] {
            if op.code == 0x02 && op2.is_none() {
                op2 = Some(op.slot as usize);
            }
            self.ops.extend_from_slice(&op.words);
        }
        self.ops.push(0);
        self.stream_at.insert(src, (at, op2));
        self.ops_dirty = true;
        (at, op2)
    }

    /// FUN_10048380: the slot of the first `code` op in the generator's element update stream.
    pub fn update_op_slot(&self, src: Ref, code: u8) -> Option<usize> {
        self.libs[src.0].generator(src.1)?.streams[2].iter().find(|o| o.code == code).map(|o| o.slot as usize)
    }

    /// FUN_10074570: a texture name from a resource's folder (16 bytes), else by its last 8 bytes among the
    /// zone's and DAT 0's `syst/effe` textures (newest first: the zone's).
    fn texture(&mut self, lib: usize, node: usize, name: &str) -> Option<(usize, usize)> {
        if let Some(i) = self.libs[lib].find_texture(node, name) {
            return self.libs[lib].texture_at(i).map(|t| (lib, t));
        }
        for l in [self.zone, self.global].into_iter().flatten() {
            let hit = self.libs[l].find_texture_tail(name).filter(|&i| Some(l) == self.zone || self.libs[l].in_syst_effe(i));
            if let Some(i) = hit {
                return self.libs[l].texture_at(i).map(|t| (l, t));
            }
        }
        None
    }

    /// A mesh's GPU id, parsing it and resolving its textures on first use.
    pub fn mesh(&mut self, r: Ref) -> Option<u32> {
        if let Some(&id) = self.mesh_id.get(&r) {
            return Some(id);
        }
        let keys = self.zone_keys.clone()?;
        let names: Vec<Option<String>> = self.libs[r.0].mesh(r.1, &keys)?.groups.iter().map(|g| g.name.clone()).collect();
        let textures: Vec<Option<(usize, usize)>> = names.iter().map(|n| n.as_ref().and_then(|n| self.texture(r.0, r.1, n))).collect();
        if let Some(m) = self.libs[r.0].meshes.get_mut(&r.1) {
            for (g, t) in m.groups.iter_mut().zip(textures) {
                g.texture = t;
            }
        }
        let id = self.meshes.len() as u32;
        self.meshes.push(r);
        self.mesh_id.insert(r, id);
        self.new_meshes.push(id);
        Some(id)
    }

    /// A D3a's GPU id; its texture resolves as a mesh's, and texel UVs are divided by the texture size.
    pub fn sprite(&mut self, r: Ref) -> Option<u32> {
        if let Some(&id) = self.sprite_id.get(&r) {
            return Some(id);
        }
        let name = self.libs[r.0].sprite(r.1)?.name.clone();
        let tex = self.texture(r.0, r.1, &name);
        let size = tex.map(|(l, t)| (self.libs[l].textures[t].width as f32, self.libs[l].textures[t].height as f32));
        let s = self.libs[r.0].sprites.get_mut(&r.1)?;
        s.texture = tex;
        if s.texel_uv
            && let Some((w, h)) = size
        {
            for v in &mut s.verts {
                v.uv = [v.uv[0] / w, v.uv[1] / h];
            }
            s.texel_uv = false;
        }
        let id = self.sprites.len() as u32;
        let base = self.sprite_verts.len() as u32;
        self.sprite_verts.extend(s.verts.iter().map(|v| bytemuck::cast::<lib::MeshVertex, [u32; 9]>(*v)));
        let first = self.sprite_frame_table.len() as u32;
        self.sprite_frame_table.extend(s.frames.iter().map(|&(start, q)| [base + start, q]));
        self.sprite_at.push((first, s.frames.iter().map(|f| f.1).max().unwrap_or(0)));
        self.sprites_dirty = true;
        self.sprites.push(r);
        self.sprite_id.insert(r, id);
        self.new_sprites.push(id);
        Some(id)
    }

    /// A D3b's GPU id; its texture resolves as a mesh's.
    pub fn morph(&mut self, r: Ref) -> Option<u32> {
        if let Some(&id) = self.morph_id.get(&r) {
            return Some(id);
        }
        let name = self.libs[r.0].morph(r.1)?.name.clone();
        let tex = self.texture(r.0, r.1, &name);
        let m = self.libs[r.0].morphs.get_mut(&r.1)?;
        m.texture = tex;
        let id = self.morphs.len() as u32;
        let at = self.morph_data.len() as u32;
        self.morph_data.extend([m.k, m.v, m.n, m.t]);
        self.morph_data.extend(m.corners.iter().flatten());
        self.morph_data.extend(m.positions.iter().flatten().map(|f| f.to_bits()));
        self.morph_data.extend(&m.normals);
        self.morph_at.push(at);
        self.morphs_dirty = true;
        self.morphs.push(r);
        self.morph_id.insert(r, id);
        self.new_morphs.push(id);
        Some(id)
    }

    pub fn morph_data_of(&self, id: u32) -> Option<&lib::Morph> {
        let r = self.morphs[id as usize];
        self.libs[r.0].morphs.get(&r.1)
    }

    pub fn sprite_is_flare(&self, id: u32) -> bool {
        let r = self.sprites[id as usize];
        self.libs[r.0].sprites.get(&r.1).is_some_and(|s| s.flare)
    }

    pub fn sprite_frames(&self, id: u32) -> u32 {
        let r = self.sprites[id as usize];
        self.libs[r.0].sprites.get(&r.1).map_or(0, |s| s.frames.len() as u32)
    }


    pub fn miss(&mut self, lib: usize, from: usize, name: u32, k: u8) {
        if self.missed.insert((lib, name)) {
            log::debug!("effects: DAT {} {}: {} (kind {k:#x}) not found", self.libs[lib].file_id, self.libs[lib].tree.nodes[from].name(), String::from_utf8_lossy(&name.to_le_bytes()));
        }
    }

    /// Logs an op of `stream` (0 generator, 1 emission) once.
    pub fn unhandled(&mut self, stream: u8, code: u8) {
        if self.logged.insert((stream, code)) {
            log::debug!("effects: stream {stream} op {code:#04x} not handled");
        }
    }
}

/// Every running generator and live element.
pub struct Effects {
    pub reg: Registry,
    pub insts: Vec<Option<Instance>>,
    pub elems: Vec<Option<ElemCpu>>,
    free: Vec<u32>,
    pub writes: Vec<Write>,
    pub slots: Vec<[u32; SLOT_WORDS]>,
    slot_free: Vec<u32>,
    serial: u32,
    rng: math::Rng,
    zone_lib: Option<usize>,
    /// The loaded sub-area's library.
    sub_lib: Option<usize>,
    /// Free runs of the particle pool (first, count).
    part_free: Vec<(u32, u32)>,
    /// Weather objects (YmCombineWeather), oldest first.
    weathers: Vec<WeatherSet>,
    weather_serial: u32,
    tasks: Vec<Option<sched::Task>>,
    /// The task being run (its slot is empty meanwhile).
    stepping: Option<usize>,
    dolls: Vec<tasks::Doll>,
    pending: Vec<tasks::Pending>,
    /// Actor +0xa0: the cast 4cc stored by FUN_10083d60.
    casting: HashMap<u32, u32>,
    sched_cache: HashMap<Ref, std::rc::Rc<Vec<sched::SOp>>>,
    index: Option<Arc<crate::dat::DatIndex>>,
    loading: HashSet<u32>,
    /// Files whose load failed: lookups pass them by instead of waiting.
    failed: HashSet<u32>,
    load_tx: std::sync::mpsc::Sender<(u32, Option<Library>)>,
    load_rx: std::sync::mpsc::Receiver<(u32, Option<Library>)>,
    /// Actors whose model generators run, by the model files they were started for.
    models: HashMap<u32, Vec<u32>>,
    /// Scheduler motions for the client to play: op 0x05 (FUN_100cb8e0), or None for op 0x28 (back to the stance).
    pub motions: Vec<(u32, Option<crate::dat::sched::Motion>)>,
    /// Op 0x29 colour drives for the client: actor, target alpha, frames.
    pub fades: Vec<(u32, u8, f32)>,
    /// Op 0x9a for the client: actor, entity +0x130 bit 1.
    pub gens_off: Vec<(u32, bool)>,
    /// Skill / magic actions' holds for the client: op 0x20 (actor2) / 0x59 (actor1, no reactions) as (actor, ticks,
    /// no reactions), and the actors whose load hold ends (their "main" started or was given up, FUN_100a9610).
    pub holds: Vec<(u32, f32, bool)>,
    pub releases: Vec<u32>,
    /// Load holds taken here (op 0x80's skillchain burst, FUN_100a8e30); released like the action's.
    pub loads: Vec<u32>,
    /// Op 0x25 / 0x21 hit reactions (FUN_100cdd60).
    pub reacts: Vec<React>,
    /// Op 0x5a / 0x5b guard and parry overlays (FUN_100cde70 / FUN_100cdf00).
    pub guards: Vec<Guard>,
    /// Op 0x5e pushes (FUN_100aa940), and op 0xbf's waiting for their delay (ticks left).
    pub knocks: Vec<Knock>,
    knocks_later: Vec<(f32, Knock)>,
    /// DAT_1035bc60: knockback (speed per tick, decay, ticks) by level.
    knock_table: [[f32; 3]; 8],
    /// Model +0x800: the footstep folder an actor's last step used (true = 'fser', the rain set), for op 0x78.
    foot_folder: HashMap<u32, bool>,
    /// Messages that print now: (action, record, which: 0 main (op 0x2b, the action's end), 1 proc / 2 react (op 0x71)).
    pub action_prints: Vec<(crate::net::action::Action, usize, u8)>,
    /// Motion files to put on an actor's list (FUN_100d22a0 kind 6: weapon-skill files).
    pub motion_files: Vec<(u32, Vec<u32>)>,
    /// Door part moves from scheduler ops 0x0c / 0x0d this frame, for the renderer.
    pub door_moves: Vec<DoorMove>,
    pub lift_moves: Vec<LiftMove>,
    /// W+0x1bc: the camera is under a covered object (FUN_101845c0).
    covered: bool,
    /// Sound elements and scheduler sounds; this frame's commands for the audio; voices the audio reports alive.
    sounds: Vec<sound::Sound>,
    /// Config 0x92 (the Effects filter) and whether the player's status is 1 or 3 (FUN_100ac4b0's bit-0 gate).
    pub filter: (u32, bool),
    /// Footstep dust to start next tick: generator and actor; actors that left a footprint.
    dust: Vec<(Ref, u32)>,
    prints: Vec<u32>,
    pub sound_cmds: Vec<sound::SoundCmd>,
    alive: HashSet<u32>,
    next_voice: u32,
}

/// One weather's generators (FUN_10183780): its tag, crossfade length and what is left of it (seconds).
struct WeatherSet {
    serial: u32,
    tag: [u8; 4],
    duration: f32,
    remaining: f32,
}

/// Elements and batched particles the GPU buffers hold.
pub const MAX_ELEMS: usize = 8192;
pub const MAX_PARTS: usize = 65536;

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

impl Effects {
    pub fn new() -> Self {
        let (load_tx, load_rx) = std::sync::mpsc::channel();
        Self {
            reg: Registry::new(),
            insts: Vec::new(),
            elems: Vec::new(),
            free: Vec::new(),
            writes: Vec::new(),
            slots: Vec::new(),
            slot_free: Vec::new(),
            serial: 0,
            rng: math::Rng::new(0x5eed_f00d),
            zone_lib: None,
            part_free: vec![(0, MAX_PARTS as u32)],
            weathers: Vec::new(),
            weather_serial: 0,
            tasks: Vec::new(),
            stepping: None,
            dolls: Vec::new(),
            pending: Vec::new(),
            casting: HashMap::new(),
            sched_cache: HashMap::new(),
            index: None,
            loading: HashSet::new(),
            failed: HashSet::new(),
            load_tx,
            load_rx,
            models: HashMap::new(),
            motions: Vec::new(),
            fades: Vec::new(),
            gens_off: Vec::new(),
            holds: Vec::new(),
            releases: Vec::new(),
            loads: Vec::new(),
            reacts: Vec::new(),
            guards: Vec::new(),
            knocks: Vec::new(),
            knocks_later: Vec::new(),
            knock_table: [[0.0; 3]; 8],
            foot_folder: HashMap::new(),
            action_prints: Vec::new(),
            motion_files: Vec::new(),
            door_moves: Vec::new(),
            lift_moves: Vec::new(),
            sub_lib: None,
            covered: false,
            sounds: Vec::new(),
            filter: (0, false),
            dust: Vec::new(),
            prints: Vec::new(),
            sound_cmds: Vec::new(),
            alive: HashSet::new(),
            next_voice: 0,
        }
    }

    pub fn set_knock_table(&mut self, t: [[f32; 3]; 8]) {
        self.knock_table = t;
    }

    pub fn set_keys(&mut self, keys: Arc<zone::Keys>) {
        self.reg.zone_keys = Some(keys);
    }

    /// DAT 0 (the shared meshes and keyframes every file falls back to).
    pub fn set_global(&mut self, lib: Library) {
        if self.reg.global.is_none() {
            self.reg.add(lib);
        }
        // FUN_10072ed0(0) keeps DAT 27 (the damage number schedulers) resident with DAT 0.
        self.preload(&[27]);
    }

    /// The zone library's registry index.
    pub fn zone_lib(&self) -> Option<usize> {
        self.zone_lib
    }

    pub fn has_global(&self) -> bool {
        self.reg.global.is_some()
    }

    /// A new zone: the old zone's instances go, and its auto-start world generators start (FUN_10046d30).
    /// Returns the library's registry index, for [`Effects::switch_zone`].
    pub fn load_zone(&mut self, lib: Library) -> usize {
        let l = self.reg.add(lib);
        self.switch_zone(l);
        l
    }

    /// Makes an already registered library the zone's (event op 0x34 swaps areas and back).
    pub fn switch_zone(&mut self, l: usize) {
        // Actors are rebuilt at zone-in: no cast carries over (inf).
        self.casting.clear();
        for i in 0..self.insts.len() {
            if self.insts[i].as_ref().is_some_and(|x| matches!(x.owner, Owner::Zone | Owner::SubArea | Owner::Weather(_))) {
                self.kill(i);
            }
        }
        self.weathers.clear();
        self.tasks.clear();
        self.dolls.clear();
        self.pending.clear();
        self.models.clear();
        self.zone_lib = Some(l);
        self.reg.zone = Some(l);
        let weat = crate::dat::generator::fourcc("weat");
        let starts: Vec<usize> = self.reg.libs[l]
            .gens
            .iter()
            .filter(|g| g.flags & 0x1000 != 0 && matches!(g.attach_mode(), 0 | 0xe | 0xf | 0x14))
            .filter(|g| !self.reg.libs[l].tree.path(g.node).contains(&weat))
            .map(|g| g.node)
            .collect();
        log::info!("effects: zone DAT {}: {} generators, {} start at load", self.reg.libs[l].file_id, self.reg.libs[l].gens.len(), starts.len());
        // What the loader parsed goes to the GPU with the zone (render/effects.rs upload_pending).
        let meshes: Vec<usize> = self.reg.libs[l].meshes.keys().copied().collect();
        let sprites: Vec<usize> = self.reg.libs[l].sprites.keys().copied().collect();
        for n in meshes {
            self.reg.mesh((l, n));
        }
        for n in sprites {
            self.reg.sprite((l, n));
        }
        for node in starts {
            self.start((l, node), Owner::Zone);
        }
    }

    /// A sub-area DAT (an interior) loaded beside the zone: the old one's generators go and its auto-start world
    /// generators start, as a zone's do *(inf: the chunk loader starts them the same way)*. A file loaded before is
    /// reused.
    pub fn load_sub_area(&mut self, lib: Library) {
        self.drop_sub_area();
        let l = self.reg.lib_of(lib.file_id).unwrap_or_else(|| self.reg.add(lib));
        self.sub_lib = Some(l);
        let weat = crate::dat::generator::fourcc("weat");
        let starts: Vec<usize> = self.reg.libs[l]
            .gens
            .iter()
            .filter(|g| g.flags & 0x1000 != 0 && matches!(g.attach_mode(), 0 | 0xe | 0xf | 0x14))
            .filter(|g| !self.reg.libs[l].tree.path(g.node).contains(&weat))
            .map(|g| g.node)
            .collect();
        log::info!("effects: sub-area DAT {}: {} generators, {} start at load", self.reg.libs[l].file_id, self.reg.libs[l].gens.len(), starts.len());
        let meshes: Vec<usize> = self.reg.libs[l].meshes.keys().copied().collect();
        let sprites: Vec<usize> = self.reg.libs[l].sprites.keys().copied().collect();
        for n in meshes {
            self.reg.mesh((l, n));
        }
        for n in sprites {
            self.reg.sprite((l, n));
        }
        for node in starts {
            self.start((l, node), Owner::SubArea);
        }
    }

    /// FUN_10072290 / FUN_100724c0: scheduler `name` inside the folder named `folder` (a door id) of the zone or the
    /// sub-area, run with actor1 = actor2 = the door; false when there is none.
    pub fn door_scheduler(&mut self, actor: u32, folder: u32, name: u32) -> bool {
        let (folder, name) = (folder.to_le_bytes(), name.to_le_bytes());
        for l in [self.zone_lib, self.sub_lib].into_iter().flatten() {
            let tree = &self.reg.libs[l].tree;
            let Some(f) = tree.nodes.iter().position(|n| n.kind == kind::FOLDER && n.name == folder) else { continue };
            let inside = |mut i: usize| loop {
                match tree.nodes[i].parent {
                    Some(p) if p == f => return true,
                    Some(p) => i = p,
                    None => return false,
                }
            };
            if let Some(s) = tree.nodes.iter().enumerate().position(|(i, n)| n.kind == kind::SCHEDULER && n.name == name && inside(i)) {
                let who = sched::Who::Actor(actor);
                self.spawn_task((l, s), who, who, None, 1.0, None);
                return true;
            }
        }
        false
    }

    /// The sub-area unloads: its generators stop.
    pub fn drop_sub_area(&mut self) {
        self.sub_lib = None;
        for i in 0..self.insts.len() {
            if self.insts[i].as_ref().is_some_and(|x| matches!(x.owner, Owner::SubArea)) {
                self.kill(i);
            }
        }
    }

    /// FUN_101804d0 / FUN_10183780: the zone's `weat/<tag>` generators start (fallback `suny`, then `weat`
    /// itself) and crossfade with the current set over `seconds` (op 0x50 reads the weight); the same tag does
    /// nothing, and 0 seconds replaces the old set at once.
    pub fn set_weather(&mut self, tag: &str, seconds: f32) {
        let Some(l) = self.zone_lib else { return };
        let tag4 = crate::dat::generator::fourcc(tag);
        if self.weathers.last().is_some_and(|w| w.tag == tag4) {
            return;
        }
        if seconds <= 0.0 {
            self.drop_weathers(usize::MAX);
        }
        let tree = &self.reg.libs[l].tree;
        let named = |parent: Option<usize>, n: &str| {
            let n = crate::dat::generator::fourcc(n);
            tree.nodes.iter().enumerate().find(|(_, x)| x.kind == kind::FOLDER && x.name == n && (parent.is_none() || x.parent == parent)).map(|(i, _)| i)
        };
        let Some(weat) = named(None, "weat") else { return };
        let folder = named(Some(weat), tag).or_else(|| named(Some(weat), "suny")).unwrap_or(weat);
        let within = |mut i: usize| loop {
            match tree.nodes[i].parent {
                Some(p) if p == folder => return true,
                Some(p) => i = p,
                None => return false,
            }
        };
        let covered = self.covered;
        let gens: Vec<usize> = self.reg.libs[l].gens.iter().filter(|g| g.flags & 0x1000 != 0 && within(g.node) && !(covered && roofed(g))).map(|g| g.node).collect();
        self.weather_serial += 1;
        let serial = self.weather_serial;
        log::info!("effects: weather {tag} over {seconds:.0} s: {} generators", gens.len());
        for node in gens {
            if let Some(i) = self.start((l, node), Owner::Weather(serial)) {
                if let Some(x) = self.insts[i].as_mut() {
                    x.weather = if seconds > 0.0 { 0.0 } else { 1.0 };
                }
            }
        }
        self.weathers.push(WeatherSet { serial, tag: tag4, duration: seconds, remaining: seconds });
    }

    /// The zone's ambient loop (FUN_10189ca0 via FUN_1018a220, played by FUN_10036170): a SeSep (type 0x3d) under the
    /// weather's folder (as set_weather picks it), inside its "indo" folder when covered and outside it otherwise,
    /// named HHMM (FUN_10189bd0: four digits, the first 0 or 1, the third at most 5, the parent not "extr"); the
    /// latest at or before the Vana'diel minute, else the last valid one in file order (e.g. 1800 before 06:00).
    pub fn ambient(&self, minute: f32) -> Option<u32> {
        let l = self.zone_lib?;
        let lib = &self.reg.libs[l];
        let tree = &lib.tree;
        let named = |parent: Option<usize>, n: [u8; 4]| tree.nodes.iter().enumerate().find(|(_, x)| x.kind == kind::FOLDER && x.name == n && (parent.is_none() || x.parent == parent)).map(|(i, _)| i);
        let fc = crate::dat::generator::fourcc;
        let weat = named(None, fc("weat"))?;
        let tag = self.weathers.last().map_or(fc("suny"), |w| w.tag);
        let folder = named(Some(weat), tag).or_else(|| named(Some(weat), fc("suny"))).unwrap_or(weat);
        let under = |mut i: usize, f: usize| loop {
            match tree.nodes[i].parent {
                Some(p) if p == f => return true,
                Some(p) => i = p,
                None => return false,
            }
        };
        let indo = tree.nodes.iter().enumerate().find(|(i, x)| x.kind == kind::FOLDER && x.name == fc("indo") && under(*i, folder)).map(|(i, _)| i);
        let time = |n: &[u8; 4]| -> Option<u32> {
            let d = |c: u8| c.is_ascii_digit().then(|| (c - b'0') as u32);
            let (a, b, c, e) = (d(n[0])?, d(n[1])?, d(n[2])?, d(n[3])?);
            (a <= 1 && c <= 5).then_some((a * 10 + b) * 60 + c * 10 + e)
        };
        let now = minute.rem_euclid(1440.0) as u32;
        let mut best: Option<(u32, usize)> = None;
        let mut last = None;
        for (i, n) in tree.nodes.iter().enumerate() {
            if n.kind != 0x3d || !under(i, folder) || n.parent.is_some_and(|p| tree.nodes[p].name == fc("extr")) {
                continue;
            }
            if let Some(f) = indo
                && under(i, f) != self.covered
            {
                continue;
            }
            let Some(t) = time(&n.name) else { continue };
            last = Some(i);
            if t <= now && best.is_none_or(|(b, _)| t >= b) {
                best = Some((t, i));
            }
        }
        let node = best.map(|b| b.1).or(last)?;
        sound::sesep(lib.data(node)).map(|(se, _)| se)
    }

    /// FUN_101845c0: on a change, covered stops the transient weather emitters that spawn at the camera (op-1
    /// flags 4 or 0x4000000: rain, snow, dust) and kills their elements (FUN_10183b90); uncovered starts them again.
    pub fn set_covered(&mut self, covered: bool) {
        if covered == self.covered {
            return;
        }
        self.covered = covered;
        for i in 0..self.insts.len() {
            let Some(x) = self.insts[i].as_ref() else { continue };
            if !matches!(x.owner, Owner::Weather(_)) {
                continue;
            }
            let Some(g) = self.reg.libs[x.src.0].generator(x.src.1) else { continue };
            let transient = !(g.flags & 0x400 != 0 || g.template().is_some_and(|o| o.i16_at(0x22) == 0));
            if !transient || !roofed(g) {
                continue;
            }
            if covered {
                let live = std::mem::take(&mut self.insts[i].as_mut().unwrap().live);
                for e in live {
                    self.drop_elem(e);
                }
                self.insts[i].as_mut().unwrap().stopping = true;
            } else if let Some(x) = self.insts[i].as_mut() {
                x.stopping = false;
                x.countdown = 0.0;
                x.elapsed = 0.0;
                x.emitted = 0;
            }
        }
    }

    /// FUN_10183bd0 for the oldest `n` weather sets: their elements go now.
    fn drop_weathers(&mut self, n: usize) {
        let n = n.min(self.weathers.len());
        let gone: Vec<u32> = self.weathers.drain(..n).map(|w| w.serial).collect();
        for i in 0..self.insts.len() {
            if let Some(Owner::Weather(s)) = self.insts[i].as_ref().map(|x| x.owner)
                && gone.contains(&s)
            {
                self.kill(i);
            }
        }
    }

    /// The newest weather counts down; its weight is (duration - remaining) / duration, the older ones get the
    /// rest, and they go when it is done.
    fn weather_step(&mut self, seconds: f32) {
        let Some(new) = self.weathers.last_mut() else { return };
        new.remaining = (new.remaining - seconds).max(0.0);
        let wgt = if new.duration > 0.0 { (new.duration - new.remaining) / new.duration } else { 1.0 };
        let (serial, done) = (new.serial, new.remaining <= 0.0);
        for x in self.insts.iter_mut().flatten() {
            if let Owner::Weather(s) = x.owner {
                x.weather = if s == serial { wgt } else { 1.0 - wgt };
            }
        }
        if done && self.weathers.len() > 1 {
            self.drop_weathers(self.weathers.len() - 1);
        }
    }

    /// FUN_10052220: a new running instance of the generator.
    pub fn start(&mut self, src: Ref, owner: Owner) -> Option<usize> {
        let g = self.reg.libs[src.0].generator(src.1)?.clone();
        let mut de = 0u8;
        if g.name[0] == b'~' {
            de = (g.name[1].wrapping_sub(b'0') & 0xfb) | 0x80;
        }
        let weat = crate::dat::generator::fourcc("weat");
        if self.reg.libs[src.0].tree.path(src.1).contains(&weat) {
            de |= 0x83;
        }
        let mslot = self.alloc_slot();
        let inst = Instance {
            src,
            name: g.name,
            streams: g.streams.clone(),
            flags: g.flags | 0x800,
            stopping: false,
            de,
            life: g.life,
            elapsed: 0.0,
            countdown: 0.0,
            interval: g.interval,
            interval_rand: g.interval_rand,
            emitted: 0,
            mode: g.attach_mode(),
            matrix: None,
            scale_a: Vec3::ONE,
            scale_b: Vec3::ONE,
            dist: 0.0,
            size_scaled: (g.attach >> 24) & 0xf != 0,
            mslot,
            weather: 1.0,
            live: Vec::new(),
            finished: false,
            owner,
            parent_elem: None,
            actors: [sched::Who::None; 2],
            driven: None,
            ground: None,
            area_light: None,
            dam: None,
        };
        let i = match self.insts.iter().position(|x| x.is_none()) {
            Some(i) => {
                self.insts[i] = Some(inst);
                i
            }
            None => {
                self.insts.push(Some(inst));
                self.insts.len() - 1
            }
        };
        Some(i)
    }

    fn alloc_slot(&mut self) -> u32 {
        let mut s = [0u32; SLOT_WORDS];
        let id = Mat4::IDENTITY.to_cols_array();
        for k in 0..16 {
            s[slot::MATRIX + k] = id[k].to_bits();
        }
        for k in 0..3 {
            s[slot::SCALE_A + k] = 1f32.to_bits();
            s[slot::SCALE_B + k] = 1f32.to_bits();
        }
        s[slot::WEATHER] = 1f32.to_bits();
        if let Some(i) = self.slot_free.pop() {
            self.slots[i as usize] = s;
            i
        } else {
            self.slots.push(s);
            (self.slots.len() - 1) as u32
        }
    }

    fn put_slot(&mut self, i: u32, m: Option<Mat4>, a: Vec3, b: Vec3, weather: f32, size_scaled: bool) {
        let s = &mut self.slots[i as usize];
        let arr = m.unwrap_or(Mat4::IDENTITY).to_cols_array();
        for k in 0..16 {
            s[slot::MATRIX + k] = arr[k].to_bits();
        }
        for k in 0..3 {
            s[slot::SCALE_A + k] = a[k].to_bits();
            s[slot::SCALE_B + k] = b[k].to_bits();
        }
        s[slot::WEATHER] = weather.to_bits();
        s[slot::SIZE_SCALED] = size_scaled as u32;
        s[slot::HAS_MATRIX] = m.is_some() as u32;
    }

    /// FUN_10052930(0): the instance and its elements go now.
    pub fn kill(&mut self, i: usize) {
        let Some(inst) = self.insts[i].take() else { return };
        for e in inst.live {
            self.drop_elem(e);
        }
        self.slot_free.push(inst.mslot);
    }

    fn drop_elem(&mut self, e: u32) {
        let Some(c) = self.elems.get_mut(e as usize).and_then(|x| x.take()) else { return };
        if let Some(s) = c.light {
            self.writes.push(Write::LightOff(s));
        }
        if c.mslot != NONE {
            self.slot_free.push(c.mslot);
        }
        if let Some(k) = self.sounds.iter().position(|s| s.elem == Some(e)) {
            let s = self.sounds.swap_remove(k);
            if s.handle != 0 {
                self.sound_cmds.push(sound::SoundCmd::Stop { handle: s.handle, ticks: s.fade as i32 });
            }
        }
        if let Some(run) = c.parts {
            self.part_free.push(run);
            self.part_free.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::with_capacity(self.part_free.len());
            for r in self.part_free.drain(..) {
                match merged.last_mut() {
                    Some(m) if m.0 + m.1 == r.0 => m.1 += r.1,
                    _ => merged.push(r),
                }
            }
            self.part_free = merged;
        }
        if let Some(ch) = c.child
            && self.insts.get(ch).is_some_and(|x| x.is_some())
        {
            // The linked clone stops emitting; its own elements finish.
            if let Some(x) = self.insts[ch].as_mut() {
                x.stopping = true;
            }
        }
        self.writes.push(Write::Words(e, w::STATE, vec![0]));
        self.free.push(e);
    }

    /// Readback records from the GPU (a frame or two late): sort keys, expire requests, positions.
    pub fn take_back(&mut self, words: &[u32]) {
        use elem::{BACK_WORDS, back};
        for (i, r) in words.chunks_exact(BACK_WORDS).enumerate() {
            let Some(Some(c)) = self.elems.get_mut(i) else { continue };
            if r[back::SERIAL] != c.serial {
                continue;
            }
            let f = |k: usize| f32::from_bits(r[k]);
            c.key = f(back::KEY);
            c.drawn = r[back::STATUS] & back::DRAWN != 0;
            c.back = Some(emit::Parent {
                pos: Vec3::ZERO,
                world: Vec3::new(f(back::WORLD), f(back::WORLD + 1), f(back::WORLD + 2)),
                rot: Vec3::new(f(back::ROT), f(back::ROT + 1), f(back::ROT + 2)),
                scale: Vec3::new(f(back::SCALE), f(back::SCALE + 1), f(back::SCALE + 2)),
                color: r[back::COLOR],
                life: c.life,
                max_life: c.max_life,
                uv: [0.0; 2],
                bias: 0.0,
            });
            // Op 0x2e / 0x48 with the kill flag: life = 1, so it expires now.
            if r[back::STATUS] & back::KILL != 0 && c.max_life != 0.0 && c.life >= 1.0 {
                c.life = 0.5;
            }
        }
    }

    /// One frame: attachments, generator updates and emissions, element lives (FUN_10043ca0) and expiry.
    pub fn tick(&mut self, f: &Frame, actors: &dyn Actors) {
        let dt = f.dt;
        self.weather_step(f.seconds);
        self.poll_loads();
        self.model_generators(actors);
        self.run_pending(f, actors);
        self.step_tasks(f, actors);
        // Op 0xbf: the push starts when its delay has counted down (its task's destructor).
        for k in self.knocks_later.iter_mut() {
            k.0 -= dt;
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.knocks_later).into_iter().partition(|k| k.0 <= 0.0);
        self.knocks_later = later;
        self.knocks.extend(due.into_iter().map(|k| k.1));
        self.start_dust(f, actors);
        // Lives first, as each element's Exec does before its update stream.
        for e in 0..self.elems.len() {
            let Some(c) = self.elems[e].as_mut() else { continue };
            if c.max_life == 0.0 {
                continue;
            }
            c.life -= dt;
            if c.life < 1.0 {
                self.expire(e as u32, f);
            }
        }
        for i in 0..self.insts.len() {
            let Some(inst) = self.insts[i].as_mut() else { continue };
            attach(inst, f);
            // Generator op 0x11: follow the actors every frame.
            if let Some(word) = inst.streams[0].iter().find(|o| o.code == 0x11).map(|o| o.u32(1))
                && !matches!(inst.mode, 0 | 0xe | 0xf | 0x14)
            {
                self.follow(i, word, f, actors);
            }
            self.ground_frame(i, actors);
            let driven = self.drive(i, f);
            let Some(inst) = self.insts[i].as_ref() else { continue };
            let (m, a, b, wgt, ss, ms) = (inst.matrix, inst.scale_a, inst.scale_b, inst.weather, inst.size_scaled, inst.mslot);
            self.put_slot(ms, m, a, b, wgt, ss);
            if driven {
                self.update_instance(i, f);
            }
        }
        self.step_sounds(f);
        // Finished or stopped instances go once their elements have.
        for i in 0..self.insts.len() {
            if let Some(x) = &self.insts[i]
                && (x.finished || x.stopping)
                && x.live.is_empty()
                && !matches!(x.owner, Owner::Zone | Owner::SubArea | Owner::Actor(_) | Owner::Weather(_))
            {
                self.kill(i);
            }
        }
    }

    /// Whether instance `i` runs this frame: its own task always; an element's clone while the element lives and its
    /// update stream has op 0x25 (emit every frame, life != 0), or 0x33 / 0x46 (the clone follows the element:
    /// its rotation, scale and world position, 0x46 also turned to the camera). The clone of a dead element stops.
    fn drive(&mut self, i: usize, f: &Frame) -> bool {
        let Some(e) = self.insts[i].as_ref().and_then(|x| x.driven) else { return true };
        let elem = self.elems.get(e as usize).and_then(|c| c.as_ref()).filter(|c| c.child == Some(i));
        let Some(c) = elem else {
            if let Some(x) = self.insts[i].as_mut() {
                x.stopping = true;
            }
            return false;
        };
        let (life, back, owner) = (c.life, c.back, self.insts.get(c.inst).and_then(|x| x.as_ref()).map(|x| x.src));
        let Some(owner) = owner else { return false };
        let has = |reg: &Registry, code: u8| reg.update_op_slot(owner, code).is_some();
        let follow = if has(&self.reg, 0x46) { Some(true) } else if has(&self.reg, 0x33) { Some(false) } else { None };
        if let (Some(cam), Some(b)) = (follow, back)
            && let Some(x) = self.insts[i].as_mut()
        {
            let mut m = math::euler(b.rot);
            if cam {
                let [_, _, _, c10] = crate::render::effects::view_set(f.view);
                let mut c = c10;
                c.w_axis = glam::Vec4::W;
                m = math::then(m, c);
            }
            m.w_axis = b.world.extend(1.0);
            x.matrix = Some(m);
            x.scale_a = b.scale;
            return true;
        }
        has(&self.reg, 0x25) && life != 0.0
    }

    /// FUN_10048740 for one instance.
    fn update_instance(&mut self, i: usize, f: &Frame) {
        let Some(mut inst) = self.insts[i].take() else { return };
        if inst.stopping || inst.finished {
            self.insts[i] = Some(inst);
            return;
        }
        let allowed = emit::generator_stream(&mut self.reg, &mut inst, f);
        let mut dt = f.dt.min(10.0);
        if inst.life > 0.0 && inst.elapsed >= inst.life {
            inst.elapsed += dt;
            inst.finished = true;
            self.insts[i] = Some(inst);
            return;
        }
        let persistent = inst.flags & 0x400 != 0 || inst.streams[1].first().is_some_and(|o| o.i16_at(0x22) == 0);
        let mut emissions = 0u32;
        if persistent {
            if inst.emitted == 0 && allowed {
                emissions = 1;
            }
        } else {
            if inst.de & 2 != 0 {
                dt *= 1.0; // weather density (config 0x3b) is 1.0 *(inf: the setting is not wired)*
            }
            inst.countdown -= dt;
            let mut budget = inst.life;
            while inst.countdown < 0.0 {
                if allowed && inst.flags & 0x800 != 0 {
                    emissions += if inst.flags & 0x2000_0000 != 0 { 1 } else { (inst.flags & 0x1ff) + 1 };
                }
                let interval = self.rng.un(inst.interval_rand as u16 as u32) as f32 + 1.0 + inst.interval as f32;
                inst.countdown += interval;
                if inst.life > 0.0 {
                    budget -= interval;
                    if budget <= inst.elapsed {
                        break;
                    }
                }
            }
        }
        let parent = inst.parent_elem.and_then(|p| self.elems.get(p as usize)?.as_ref()?.back);
        let mut spawns = Vec::new();
        for _ in 0..emissions.min(512) {
            if let Some(s) = emit::emit(&mut self.reg, &mut inst, parent.as_ref(), f, &mut self.rng) {
                spawns.push(s);
            }
        }
        inst.elapsed += dt;
        if inst.life > 0.0 && inst.elapsed >= inst.life {
            inst.finished = true;
        }
        self.insts[i] = Some(inst);
        for s in spawns {
            self.add_elem(i, s, f);
        }
    }

    fn add_elem(&mut self, inst: usize, s: emit::Spawn, f: &Frame) {
        let Some(e) = self.free.pop().or_else(|| (self.elems.len() < MAX_ELEMS).then(|| {
            self.elems.push(None);
            (self.elems.len() - 1) as u32
        })) else {
            return;
        };
        self.serial = self.serial.wrapping_add(1);
        let mut elem = s.elem;
        let mut cpu = s.cpu;
        if !s.parts.is_empty() {
            let n = s.parts.len() as u32;
            let Some(k) = self.part_free.iter().position(|r| r.1 >= n) else {
                self.free.push(e);
                return;
            };
            let (first, len) = self.part_free[k];
            if len == n {
                self.part_free.swap_remove(k);
            } else {
                self.part_free[k] = (first + n, len - n);
            }
            cpu.parts = Some((first, n));
            elem.0[w::PART_BASE] = first;
            self.writes.push(Write::Parts(first, s.parts));
        }
        cpu.inst = inst;
        cpu.serial = self.serial;
        if let Some(mut snd) = s.sound {
            snd.elem = Some(e);
            snd.pos = elem.v3(w::POS);
            snd.fade = cpu.max_life;
            if let Some(x) = self.insts[inst].as_ref() {
                snd.gen_persistent = x.flags & 0x400 != 0 || x.streams[1].first().is_some_and(|o| o.i16_at(0x22) == 0);
                if let Some(k) = self.reg.update_op_slot(x.src, 0x43) {
                    snd.vol_key = Some((elem.s(k), elem.s(k + 1)));
                }
            }
            // FUN_10035a90: generator sound elements never run out of life.
            cpu.max_life = 0.0;
            self.sounds.push(snd);
        }
        elem.0[w::SERIAL] = self.serial;
        if let Some(m) = cpu.snapshot {
            let ms = self.alloc_slot();
            let (a, b, ss) = self.insts[inst].as_ref().map_or((Vec3::ONE, Vec3::ONE, false), |x| (x.scale_a, x.scale_b, x.size_scaled));
            self.put_slot(ms, Some(m), a, b, 1.0, ss);
            cpu.mslot = ms;
            elem.0[w::PARENT] = ms;
        }
        let parent = emit::Parent {
            pos: elem.v3(w::POS),
            world: self.insts[inst].as_ref().and_then(|x| x.matrix).map_or(elem.v3(w::POS), |m| m.transform_point3(elem.v3(w::POS))),
            rot: elem.v3(w::ROT),
            scale: elem.v3(w::SCALE),
            color: elem.0[w::COLOR],
            life: cpu.life,
            max_life: cpu.max_life,
            uv: [elem.f(w::UV), elem.f(w::UV + 1)],
            bias: elem.f(w::BIAS),
        };
        cpu.back = Some(parent);
        self.elems[e as usize] = Some(cpu);
        if let Some(x) = self.insts[inst].as_mut() {
            x.live.push(e);
        }
        // The CPU's lives ran before this spawn: the GPU skips its first decrement too, or it would run a frame's dt
        // ahead and sample t >= 1 on the last frame (keys ending in a spike at 1.0 flash white).
        elem.0[w::CTRL] |= ctl::LIFE_SET;
        self.writes.push(Write::Full(e, Box::new(elem)));
        for ch in s.children {
            match ch {
                emit::Child::Clone { src, op } => {
                    // Ops 0x53 / 0x6a: the clone's matrix is the element's rotation (through the emitter's
                    // rotation, and the camera's for 0x6a) at the element's world position, its scales the
                    // element's; 0x44 leaves the clone's own placement.
                    let (emitter, size, actors) = self.insts[inst].as_ref().map_or((None, Vec3::ONE, [sched::Who::None; 2]), |x| (x.matrix, x.scale_b, x.actors));
                    let dam = self.insts[inst].as_ref().and_then(|x| x.dam.clone());
                    if let Some(ci) = self.start(src, Owner::Element(e as usize)) {
                        if let Some(x) = self.insts[ci].as_mut() {
                            x.parent_elem = Some(e);
                            x.driven = Some(e);
                            x.actors = actors;
                            x.dam = dam;
                            if op != 0x44 {
                                let mut m = math::euler(parent.rot);
                                if let Some(a) = emitter {
                                    let mut a = a;
                                    a.w_axis = glam::Vec4::W;
                                    m = math::then(m, a);
                                }
                                if op == 0x6a {
                                    let [_, _, _, c10] = crate::render::effects::view_set(f.view);
                                    let mut c = c10;
                                    c.w_axis = glam::Vec4::W;
                                    m = math::then(m, c);
                                }
                                m.w_axis = parent.world.extend(1.0);
                                x.matrix = Some(m);
                                x.scale_a = parent.scale;
                                x.scale_b = size;
                            }
                        }
                        if let Some(c) = self.elems[e as usize].as_mut() {
                            c.child = Some(ci);
                        }
                        self.update_instance(ci, f);
                    }
                }
                emit::Child::Burst { src } => self.burst(src, e, f),
            }
        }
    }

    /// FUN_100d41a0: the dust generator cloned onto the actor (both slots), started, one emission now (life 10000,
    /// elapsed 10001 after), so it ends; attach point 0x34 puts it at the foot contact. Retail keeps one clone per
    /// actor and restarts it *(inf: a new instance per step, same emissions)*.
    fn start_dust(&mut self, f: &Frame, actors: &dyn Actors) {
        for id in std::mem::take(&mut self.prints) {
            self.start_print(id, f, actors);
        }
        for (g, id) in std::mem::take(&mut self.dust) {
            let Some(i) = self.start(g, Owner::Step(id)) else { continue };
            log::debug!("footstep dust {} on {id:#x}", String::from_utf8_lossy(&self.reg.libs[g.0].tree.nodes[g.1].name));
            if let Some(x) = self.insts[i].as_mut() {
                x.actors = [sched::Who::Actor(id); 2];
                x.life = 10000.0;
            }
            self.attach_now(i, actors);
            self.update_instance(i, f);
            if let Some(x) = self.insts[i].as_mut() {
                x.elapsed = 10001.0;
                x.finished = true;
            }
        }
    }

    /// FUN_100d3750 for an instance whose emission stream has op 0x8e: down = into the floor, front = the actor's
    /// (0, 0, 1) (model x is its facing), side = down x front, front = side x down.
    fn ground_frame(&mut self, i: usize, actors: &dyn Actors) {
        let Some(x) = self.insts[i].as_mut() else { return };
        if x.streams[1].iter().any(|o| o.code == 0x8f) {
            x.area_light = x.actors[0].id().and_then(|id| actors.area_light(id));
        }
        if !x.streams[1].iter().any(|o| o.code == 0x8e) {
            return;
        }
        let Some(id) = x.actors[0].id() else { return };
        let (Some(foot), Some(m)) = (actors.foot(id), actors.matrix(id)) else { return };
        let down = actors.floor(id).unwrap_or(Vec3::Y);
        let side = down.cross(m.z_axis.truncate().normalize_or(Vec3::Z)).normalize_or(Vec3::X);
        let front = side.cross(down).normalize_or(Vec3::Z);
        x.ground = Some((foot, [side, down, front]));
    }

    /// Footprint (FUN_100d2970 step 7): 'fmrk' from the actor's part files, else DAT 0; sized by the actor, source
    /// actor set, one element straight from the template (FUN_1004d920, no generator update).
    fn start_print(&mut self, id: u32, f: &Frame, actors: &dyn Actors) {
        let name = u32::from_le_bytes(*b"fmrk");
        let from_files = actors.files(id).0.iter().find_map(|&file| {
            let l = self.reg.lib_of(file)?;
            self.reg.libs[l].find_global(kind::GENERATOR, name).map(|n| (l, n))
        });
        let Some(g) = from_files.or_else(|| self.reg.global.and_then(|l| self.reg.libs[l].find_global(kind::GENERATOR, name).map(|n| (l, n)))) else { return };
        let Some(i) = self.start(g, Owner::Step(id)) else { return };
        let s = actors.scale(id);
        if let Some(x) = self.insts[i].as_mut() {
            x.actors = [sched::Who::Actor(id), sched::Who::None];
            x.scale_b = Vec3::splat(s);
        }
        self.ground_frame(i, actors);
        let Some(mut inst) = self.insts[i].take() else { return };
        let spawn = emit::emit(&mut self.reg, &mut inst, None, f, &mut self.rng);
        inst.finished = true;
        self.insts[i] = Some(inst);
        if let Some(s) = spawn {
            self.add_elem(i, s, f);
        }
    }

    /// One emission of generator `src` with element `e` as parent, now (emission op 0x3c, end-stream op 1).
    fn burst(&mut self, src: Ref, e: u32, f: &Frame) {
        let actors = self.elems.get(e as usize).and_then(|c| c.as_ref()).and_then(|c| self.insts.get(c.inst)?.as_ref()).map_or([sched::Who::None; 2], |x| x.actors);
        // Retail runs the generator resource itself (FUN_10048740), whose task link +0x64 is 0: no damage number.
        if let Some(ci) = self.start(src, Owner::Element(e as usize)) {
            if let Some(x) = self.insts[ci].as_mut() {
                x.parent_elem = Some(e);
                x.actors = actors;
            }
            self.update_instance(ci, f);
            if let Some(x) = self.insts[ci].as_mut() {
                x.stopping = true;
            }
        }
    }

    /// FUN_10043ca0 once life < 1: the end stream (FUN_1004d770), then delete unless held.
    fn expire(&mut self, e: u32, f: &Frame) {
        let Some(c) = self.elems[e as usize].as_ref() else { return };
        let inst = c.inst;
        let (ops, src, actors) = self.insts.get(inst).and_then(|x| x.as_ref()).map(|x| (x.streams[3].clone(), x.src, x.actors)).unwrap_or_default();
        for op in &ops {
            match op.code {
                // Burst a generator at the element / start a scheduler with the element's actor pair.
                0x01 => {
                    if let Some(g) = self.reg.resolve(src.0, src.1, kind::GENERATOR, op.u32(2)) {
                        self.burst(g, e, f);
                    }
                }
                0x02 => {
                    if let Some(r) = self.reg.resolve(src.0, src.1, kind::SCHEDULER, op.u32(2)) {
                        self.spawn_task(r, actors[0], actors[1], None, 1.0, None);
                    }
                }
                0x05 => {
                    let c = self.elems[e as usize].as_mut().unwrap();
                    c.life = c.max_life;
                    let life = c.life;
                    self.writes.push(Write::Words(e, w::LIFE, vec![life.to_bits()]));
                    self.writes.push(Write::Words(e, w::CTRL, vec![ctl::LIFE_SET]));
                }
                _ => self.reg.unhandled(3, op.code),
            }
        }
        let Some(c) = self.elems[e as usize].as_mut() else { return };
        if c.life < 1.0 {
            if c.flags & 0x2000_0000 != 0 {
                c.life = 0.0;
                self.writes.push(Write::Words(e, w::LIFE, vec![0]));
                self.writes.push(Write::Words(e, w::CTRL, vec![ctl::LIFE_SET]));
            } else {
                if let Some(x) = self.insts.get_mut(inst).and_then(|x| x.as_mut()) {
                    x.live.retain(|&l| l != e);
                }
                self.drop_elem(e);
            }
        }
    }

    /// A footstep (FUN_100d2970 step 7): SeSep `name` in the zone's 'fses' folder ('fser' in the rain weathers when
    /// not covered), else DAT 0's 'foot'; one shot at `pos`, centred, at `level` (101 walking, 127 running). Then
    /// the dust generator `dust` of that folder, if any, bursts once on actor `id`.
    pub fn footstep(&mut self, id: u32, pos: Vec3, name: [u8; 4], dust: Option<[u8; 4]>, print: bool, rain: bool, level: i32) {
        let Some(l) = self.zone_lib else { return };
        self.foot_folder.insert(id, rain);
        let folder = crate::dat::generator::fourcc(if rain { "fser" } else { "fses" });
        let lib = &self.reg.libs[l];
        let Some(f) = lib.tree.nodes.iter().position(|n| n.kind == kind::FOLDER && n.name == folder) else { return };
        if let Some(g) = dust.and_then(|d| lib.find_in(f, kind::GENERATOR, d)) {
            self.dust.push(((l, g), id));
        }
        if print {
            self.prints.push(id);
        }
        let found = lib.find_in(f, 0x3d, name).map(|n| (l, n)).or_else(|| {
            let g = self.reg.global?;
            self.reg.libs[g].find_global(0x3d, u32::from_le_bytes(*b"foot")).map(|n| (g, n))
        });
        let Some(r) = found else { return };
        let Some((se, loops)) = sound::sesep(self.reg.libs[r.0].data(r.1)) else { return };
        let mut s = sound::Sound::new(se, loops);
        s.pos = pos;
        s.pan_in = 0.0;
        s.vol = level;
        self.sounds.push(s);
    }

    /// Voices the audio reports playing (FUN_1023ba50), before tick.
    pub fn set_alive(&mut self, alive: Vec<u32>) {
        self.alive = alive.into_iter().collect();
    }

    /// The manager pass FUN_10035410 / FUN_100356c0 over every sound: volume and pan for the eye, voices started,
    /// updated, restarted (persistent) or let go; at most 20 distinct sounds start a frame, repeats of one free
    /// (FUN_10036460).
    fn step_sounds(&mut self, f: &Frame) {
        let mut started: Vec<u32> = Vec::new();
        let mut gone = Vec::new();
        for k in 0..self.sounds.len() {
            let s = &self.sounds[k];
            // Element sounds sit where their element is (through the generator's matrix).
            let mut pos = s.pos;
            if let Some(e) = s.elem
                && let Some(c) = self.elems.get(e as usize).and_then(|c| c.as_ref())
            {
                pos = self.insts.get(c.inst).and_then(|x| x.as_ref()).and_then(|x| x.matrix).map_or(pos, |m| m.transform_point3(pos));
            }
            if let Some(caps) = &s.sph {
                pos = f.eye + sound::nearest(caps, f.eye);
            }
            let vol = match s.vol_key {
                Some((id, prm)) => {
                    let n = (prm >> 5) & 0x1ff;
                    let x = if n != 0 { (f.dayfrac * n as f32).fract() } else { f.dayfrac };
                    let v = self.reg.key_desc.get(id as usize).map_or(0.0, |d| {
                        crate::dat::generator::KeyFrame { keys: self.reg.key_data[d[0] as usize..(d[0] + d[1]) as usize].iter().map(|p| (p[0], p[1])).collect() }.sample(x, prm & 0xf != 0)
                    });
                    (v * 127.0).round() as i32
                }
                None => s.vol,
            };
            let s = &mut self.sounds[k];
            if s.grace > 0.0 {
                s.grace -= f.dt;
            }
            let r = if s.stream { Some((vol, 64)) } else { sound::attenuate(s.pan_in, vol, pos, f.eye, f.view, s.near, s.far, s.keep, s.ymode, f.focal) };
            let Some((v, pan)) = r else {
                if s.persistent() {
                    if s.handle != 0 {
                        self.sound_cmds.push(sound::SoundCmd::Stop { handle: s.handle, ticks: 0 });
                        s.handle = 0;
                    }
                } else {
                    gone.push(k);
                }
                continue;
            };
            if s.handle == 0 {
                if !started.contains(&s.se) && started.len() >= 20 {
                    continue;
                }
                started.push(s.se);
                self.next_voice = self.next_voice.wrapping_add(1).max(1);
                s.handle = self.next_voice;
                s.last_vol = v;
                s.last_pan = pan;
                s.grace = sound::GRACE_TICKS;
                self.sound_cmds.push(sound::SoundCmd::Play { handle: s.handle, se: s.se, vol: v, pan, stream: s.stream });
                continue;
            }
            if s.grace > 0.0 || self.alive.contains(&s.handle) {
                if pan != s.last_pan {
                    s.last_pan = pan;
                    self.sound_cmds.push(sound::SoundCmd::Pan { handle: s.handle, pan });
                }
                if v != s.last_vol {
                    s.last_vol = v;
                    self.sound_cmds.push(sound::SoundCmd::Level { handle: s.handle, vol: v });
                }
            } else if s.persistent() {
                // Over: a persistent sound starts again next pass (loop by re-trigger).
                s.handle = 0;
            } else {
                gone.push(k);
            }
        }
        // FUN_10035cd0: a generator's sound element that goes (voice over, out of range) fades its voice over the
        // element's life; drop_elem does it and removes the sound. Scheduler sounds play out.
        for k in gone.into_iter().rev() {
            match self.sounds[k].elem.filter(|&e| self.elems.get(e as usize).is_some_and(|c| c.is_some())) {
                Some(e) => {
                    let inst = self.elems[e as usize].as_ref().unwrap().inst;
                    if let Some(x) = self.insts.get_mut(inst).and_then(|x| x.as_mut()) {
                        x.live.retain(|&l| l != e);
                    }
                    self.drop_elem(e);
                }
                None => {
                    self.sounds.swap_remove(k);
                }
            }
        }
    }

    /// Live elements to draw, back to front (the CMoOT walk: larger keys first, insertion order within one).
    pub fn draw_order(&self) -> Vec<u32> {
        let mut v: Vec<(f32, u32)> = self
            .elems
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_ref().filter(|c| c.draw != Draw::None).map(|c| (c.key.clamp(0.0, 512.0), i as u32)))
            .collect();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        v.into_iter().map(|(_, i)| i).collect()
    }

    /// One line per live element: generator path, what it draws, blend, flags, key, life.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, c) in self.elems.iter().enumerate() {
            let Some(c) = c else { continue };
            let Some(inst) = self.insts.get(c.inst).and_then(|x| x.as_ref()) else { continue };
            let l = &self.reg.libs[inst.src.0];
            let path: Vec<String> = l.tree.path(inst.src.1).iter().map(|n| String::from_utf8_lossy(n).trim_end_matches(['\0', ' ']).to_string()).collect();
            let what = match c.draw {
                Draw::Mesh(id) => {
                    let r = self.reg.meshes[id as usize];
                    format!("mesh {}:{}", self.reg.libs[r.0].file_id, self.reg.libs[r.0].tree.nodes[r.1].name())
                }
                Draw::Sprite(id) => format!("sprite {id}{}", if self.reg.sprite_is_flare(id) { " (flare)" } else { "" }),
                Draw::Morph(id) => format!("morph {id}"),
                Draw::Dist => "dist".into(),
                Draw::DamValue => "damv".into(),
                Draw::None => "none".into(),
            };
            let pos = c.back.map(|b| b.world).unwrap_or_default();
            out.push(format!(
                "{i:4} {}/{} {what} blend {:#06x} flags {:#010x} key {:.1} life {:.0}/{:.0} at {:.1},{:.1},{:.1} argb {:08x}{}",
                path.join("/"),
                String::from_utf8_lossy(&inst.name),
                c.blend,
                c.flags,
                c.key,
                c.life,
                c.max_life,
                pos.x,
                pos.y,
                pos.z,
                c.back.map_or(0, |b| b.color),
                if c.drawn { format!(" drawn scale {:?}", c.back.map(|b| b.scale)) } else { String::new() }
            ));
        }
        out
    }

    pub fn live_count(&self) -> usize {
        self.elems.iter().filter(|e| e.is_some()).count()
    }
}

/// Op-1 flags 4 (camera-relative) or 0x4000000 (spawned in view space): stopped under a roof (LAB_10183a30 /
/// LAB_10183b40).
fn roofed(g: &crate::dat::generator::Generator) -> bool {
    g.template().is_some_and(|o| o.u32(1) & 0x400_0004 != 0)
}

/// FUN_10052eb0 / FUN_1003afd0 for the modes the zone uses; actor modes come with the battle triggers.
fn attach(inst: &mut Instance, f: &Frame) {
    use std::f32::consts::PI;
    match inst.mode {
        0 => inst.matrix = None,
        // Sun / moon (FUN_10180700): a = day fraction x 2pi; moon dir (sin a, cos a, 0), the sun opposite;
        // placed 1000 / 900 units from the eye against the direction *(inf: the rotation of DAT_105ec978 is
        // left out)*.
        0xe | 0xf => {
            let a = f.dayfrac * 2.0 * PI;
            let (dir, d) = if inst.mode == 0xe { (Vec3::new((a + PI).sin(), (a + PI).cos(), 0.0), 1000.0) } else { (Vec3::new(a.sin(), a.cos(), 0.0), 900.0) };
            inst.matrix = Some(Mat4::from_translation(f.eye - dir * d));
        }
        _ => {}
    }
}
