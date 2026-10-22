//! The effect generator system (FFXiMain's CMoGenerator / CMoElem; spec ~/ffxi/lab/research/effects/). The CPU
//! runs generators and builds elements (emit.rs); the GPU runs each element's update stream every frame and
//! draws it (render/effects.rs). Sort keys and expire requests come back from the GPU a frame late.

pub mod elem;
pub mod emit;
pub mod lib;
pub mod math;
pub mod sched;
mod tasks;

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
}

/// Who started an instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Owner {
    Zone,
    /// The weather set (index into Effects::weathers' serials).
    Weather(u32),
    Element(usize),
    /// A scheduler task (op 0x02).
    Task(usize),
    /// An actor's model files (FUN_10083970).
    Actor(u32),
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
}

/// A word range of an element for the GPU this frame.
pub enum Write {
    Full(u32, Box<Elem>),
    Words(u32, usize, Vec<u32>),
    /// Particle records from this index of the pool.
    Parts(u32, Vec<[f32; 9]>),
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
    load_tx: std::sync::mpsc::Sender<(u32, Option<Library>)>,
    load_rx: std::sync::mpsc::Receiver<(u32, Option<Library>)>,
    /// Actors whose model generators run, by the model files they were started for.
    models: HashMap<u32, Vec<u32>>,
    /// Scheduler motions for the client to play: op 0x05 (FUN_100cb8e0), or None for op 0x28 (back to the stance).
    pub motions: Vec<(u32, Option<crate::dat::sched::Motion>)>,
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
            load_tx,
            load_rx,
            models: HashMap::new(),
            motions: Vec::new(),
        }
    }

    pub fn set_keys(&mut self, keys: Arc<zone::Keys>) {
        self.reg.zone_keys = Some(keys);
    }

    /// DAT 0 (the shared meshes and keyframes every file falls back to).
    pub fn set_global(&mut self, lib: Library) {
        if self.reg.global.is_none() {
            self.reg.add(lib);
        }
    }

    pub fn has_global(&self) -> bool {
        self.reg.global.is_some()
    }

    /// A new zone: the old zone's instances go, and its auto-start world generators start (FUN_10046d30).
    pub fn load_zone(&mut self, lib: Library) {
        for i in 0..self.insts.len() {
            if self.insts[i].as_ref().is_some_and(|x| matches!(x.owner, Owner::Zone | Owner::Weather(_))) {
                self.kill(i);
            }
        }
        self.weathers.clear();
        self.tasks.clear();
        self.dolls.clear();
        self.pending.clear();
        self.models.clear();
        let l = self.reg.add(lib);
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
        for node in starts {
            self.start((l, node), Owner::Zone);
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
        let gens: Vec<usize> = self.reg.libs[l].gens.iter().filter(|g| g.flags & 0x1000 != 0 && within(g.node)).map(|g| g.node).collect();
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
        if c.mslot != NONE {
            self.slot_free.push(c.mslot);
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
        // Lives first, as each element's Exec does before its update stream.
        for e in 0..self.elems.len() {
            let Some(c) = self.elems[e].as_mut() else { continue };
            if c.max_life == 0.0 {
                continue;
            }
            c.life -= dt;
            if c.life < 1.0 {
                self.expire(e as u32);
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
            let Some(inst) = self.insts[i].as_ref() else { continue };
            let (m, a, b, wgt, ss, ms) = (inst.matrix, inst.scale_a, inst.scale_b, inst.weather, inst.size_scaled, inst.mslot);
            self.put_slot(ms, m, a, b, wgt, ss);
            self.update_instance(i, f);
        }
        // Finished or stopped instances go once their elements have.
        for i in 0..self.insts.len() {
            if let Some(x) = &self.insts[i]
                && (x.finished || x.stopping)
                && x.live.is_empty()
                && !matches!(x.owner, Owner::Zone | Owner::Actor(_))
            {
                self.kill(i);
            }
        }
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
        self.writes.push(Write::Full(e, Box::new(elem)));
        for ch in s.children {
            match ch {
                emit::Child::Clone { src, op } => {
                    // Ops 0x53 / 0x6a: the clone's matrix is the element's rotation (through the emitter's
                    // rotation, and the camera's for 0x6a) at the element's world position, its scales the
                    // element's; 0x44 leaves the clone's own placement.
                    let (emitter, size, actors) = self.insts[inst].as_ref().map_or((None, Vec3::ONE, [sched::Who::None; 2]), |x| (x.matrix, x.scale_b, x.actors));
                    if let Some(ci) = self.start(src, Owner::Element(e as usize)) {
                        if let Some(x) = self.insts[ci].as_mut() {
                            x.parent_elem = Some(e);
                            x.actors = actors;
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
                emit::Child::Burst { src } => {
                    if let Some(ci) = self.start(src, Owner::Element(e as usize)) {
                        if let Some(x) = self.insts[ci].as_mut() {
                            x.parent_elem = Some(e);
                        }
                        self.update_instance(ci, f);
                        if let Some(x) = self.insts[ci].as_mut() {
                            x.stopping = true;
                        }
                    }
                }
            }
        }
    }

    /// FUN_10043ca0 once life < 1: the end stream (FUN_1004d770), then delete unless held.
    fn expire(&mut self, e: u32) {
        let Some(c) = self.elems[e as usize].as_ref() else { return };
        let inst = c.inst;
        let ops = self.insts.get(inst).and_then(|x| x.as_ref()).map(|x| x.streams[3].clone()).unwrap_or_default();
        for op in &ops {
            match op.code {
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
                Draw::Sprite(id) => format!("sprite {id}"),
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
