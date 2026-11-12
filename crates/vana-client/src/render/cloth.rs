//! Cloth on the GPU: client/cloth.rs's step (FUN_10023290: the skin snap or carry, FUN_10023700's Verlet and constraint
//! passes, FUN_10023bd0's sphere sweeps, FUN_10024900's normals) for every cloth sheet, one invocation a sheet running
//! the same serial loops on particle state kept per actor, then each frame the particles into the actor's pose region
//! (model space, after its bones) where vs_skinned reads them. Each model's sheets go up once.
//! client/cloth.rs stays as the reference the GPU is tested against (and cloth-shot's).

use super::{Gpu, MAX_CLOTH, POSE_HEAD, POSE_PER_BONE, Put};
use crate::dat::character::{MeshPart, Skeleton};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A sheet record's words (offsets into the model's block, or into the actor's state in vec4s).
mod rec {
    pub const N: usize = 0;
    pub const COUNT0: usize = 1;
    pub const PINS0: usize = 2;
    pub const PINS1: usize = 3;
    pub const EDGES: usize = 4;
    pub const GROUP1: usize = 5;
    pub const TRIS: usize = 6;
    pub const SPHERES: usize = 7;
    pub const INF_AT: usize = 8;
    pub const EDGE_AT: usize = 9;
    pub const TRI_AT: usize = 10;
    pub const SPH_AT: usize = 11;
    pub const DAMPING: usize = 12;
    pub const GRAVITY: usize = 13;
    pub const INERTIA: usize = 14;
    pub const K0: usize = 15;
    pub const K1: usize = 16;
    /// The sheet's first particle among the actor's numbered ones (SkinMesh's numbering).
    pub const BASE: usize = 17;
    /// The actor's state: particles (cur, prev, normal), the spheres' last centres, their centres this step, the edges'
    /// spheres (four a vec4).
    pub const ST_P: usize = 18;
    pub const ST_SPREV: usize = 19;
    pub const ST_SCUR: usize = 20;
    pub const ST_OWN: usize = 21;
    pub const WORDS: usize = 24;
}

/// Influence words (bone, position, weight) and sphere words (flags, table index, bone, offset, radius).
const INF_WORDS: usize = 5;
const SPH_WORDS: usize = 7;
/// Job words: flags, sheet record, state base, pose region, bones, moved, wind dir, strength, ride, model block, then
/// the inverse model matrix.
const JOB_WORDS: usize = 32;
const JOB_HEAD: usize = 4;

const STEP: u32 = 1;
const CARRY: u32 = 2;
const RESET: u32 = 4;
const WIND: u32 = 8;
const OUT: u32 = 16;

/// A model's cloth sheets as the GPU reads them, built with its skin (SkinMesh::build).
pub struct ClothModel {
    /// Every sheet record and its tables; offsets relative to the block's start.
    words: Vec<u32>,
    /// Each sheet's record offset in `words`.
    sheets: Vec<u32>,
    /// Vec4s of state one actor needs.
    state: u32,
}

impl ClothModel {
    /// The sheets SkinMesh numbers (the same MAX_CLOTH rule), with the skeleton's spheres resolved; None without any.
    pub fn build(parts: &[MeshPart], skeleton: &Skeleton) -> Option<Self> {
        let mut words: Vec<u32> = Vec::new();
        let mut sheets = Vec::new();
        let (mut particles, mut state) = (0usize, 0u32);
        for part in parts {
            let Some(s) = part.sim.as_deref().filter(|s| particles + s.particles() <= MAX_CLOTH) else { continue };
            let base = particles;
            particles += s.particles();
            let n = s.particles();
            let at = words.len();
            sheets.push(at as u32);
            words.resize(at + rec::WORDS, 0);
            let f = |v: f32| v.to_bits();
            let hdr = [
                (rec::N, n as u32),
                (rec::COUNT0, s.counts[0] as u32),
                (rec::PINS0, s.pins[0] as u32),
                (rec::PINS1, s.pins[1] as u32),
                (rec::EDGES, s.edges.len() as u32),
                (rec::GROUP1, s.group1 as u32),
                (rec::TRIS, s.tris.len() as u32),
                (rec::SPHERES, s.spheres.len() as u32),
                (rec::DAMPING, f(s.damping)),
                (rec::GRAVITY, f(s.gravity)),
                (rec::INERTIA, f(s.inertia)),
                (rec::K0, f(s.k[0])),
                (rec::K1, f(s.k[1])),
                (rec::BASE, base as u32),
            ];
            for (k, v) in hdr {
                words[at + k] = v;
            }
            // Per particle (start, count) into its influences, then the influences.
            words[at + rec::INF_AT] = words.len() as u32;
            let table = words.len();
            words.resize(table + 2 * n, 0);
            for i in 0..n {
                let inf = part.vertices.get(i).map(Vec::as_slice).unwrap_or_default();
                (words[table + 2 * i], words[table + 2 * i + 1]) = (words.len() as u32, inf.len() as u32);
                for x in inf {
                    words.extend([x.bone as u32, f(x.pos.x), f(x.pos.y), f(x.pos.z), f(x.weight)]);
                }
            }
            words[at + rec::EDGE_AT] = words.len() as u32;
            for e in &s.edges {
                words.extend([e.a as u32, e.b as u32, f(e.rest)]);
            }
            words[at + rec::TRI_AT] = words.len() as u32;
            for t in &s.tris {
                words.extend(t.map(u32::from));
            }
            // As collide: a slot past the table is no sphere; an unused one (0xCD fill) takes no edges.
            words[at + rec::SPH_AT] = words.len() as u32;
            for &si in &s.spheres {
                let (flags, bone, off, radius) = match skeleton.spheres.get(si as usize) {
                    None => (0, 0, Vec3::ZERO, 0.0),
                    Some(r) => {
                        let used = r.bone >= 0
                            && (r.bone as usize) < skeleton.bones.len()
                            && r.radius > 0.0
                            && r.radius.is_finite();
                        (1 | (used as u32) << 1, r.bone.max(0) as u32, r.offset, r.radius)
                    }
                };
                words.extend([flags, si as u32, bone, f(off.x), f(off.y), f(off.z), f(radius)]);
            }
            let sph = s.spheres.len() as u32;
            words[at + rec::ST_P] = state;
            words[at + rec::ST_SPREV] = state + 3 * n as u32;
            words[at + rec::ST_SCUR] = state + 3 * n as u32 + sph;
            words[at + rec::ST_OWN] = state + 3 * n as u32 + 2 * sph;
            state += 3 * n as u32 + 2 * sph + s.edges.len().div_ceil(4) as u32;
        }
        (!sheets.is_empty()).then_some(Self { words, sheets, state })
    }
}

/// What an actor's cloth does this frame.
#[derive(Clone, Copy, Debug)]
pub enum Tick {
    /// Nothing new (between the 30 Hz steps).
    None,
    /// FUN_10023290 from this frame's pose: the actor's move since the last step and the wind (None on a covered floor).
    Step { moved: Vec3, wind: Option<(Vec3, f32)> },
    /// FUN_100231f0, off screen: every particle moves with the actor.
    Carry { moved: Vec3 },
}

/// An actor's cloth state in the shared buffer, given back when the actor's GPU side goes.
struct Slot {
    at: u32,
    len: u32,
    /// The first step (the snap to the skin) has run: the particles hold positions.
    placed: bool,
    free: Arc<Mutex<Vec<(u32, u32)>>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut f) = self.free.lock() {
            f.push((self.at, self.len));
        }
    }
}

/// An actor's cloth state. A clone (another actor of the model) starts without one and then gets one.
#[derive(Default)]
pub struct ClothState(Option<Slot>);

impl Clone for ClothState {
    fn clone(&self) -> Self {
        Self(None)
    }
}

pub struct ClothGpu {
    step: wgpu::ComputePipeline,
    out: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Model blocks not yet on the GPU, after `uploaded` words that are.
    data: Vec<u32>,
    uploaded: usize,
    data_buf: Option<wgpu::Buffer>,
    /// Each model's block (kept alive, so its address is never another model's).
    models: HashMap<usize, (Arc<ClothModel>, u32)>,
    /// The state buffer (vec4s), the high-water mark, and the ranges given back. First fit with no coalescing, and the
    /// buffer only grows: a few KB an actor, so neither is worth more.
    state_buf: Option<wgpu::Buffer>,
    top: u32,
    free: Arc<Mutex<Vec<(u32, u32)>>>,
    jobs: Vec<u32>,
    job_buf: Option<(wgpu::Buffer, u64)>,
    group: super::BindCache<[wgpu::Buffer; 4]>,
}

pub fn pipelines(device: &wgpu::Device) -> (wgpu::ComputePipeline, wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let src = WGSL
        .replace("JOB_WORDSu", &format!("{JOB_WORDS}u"))
        .replace("JOB_HEADu", &format!("{JOB_HEAD}u"))
        .replace("POSE_HEADu", &format!("{POSE_HEAD}u"))
        .replace("PER_BONEu", &format!("{POSE_PER_BONE}u"))
        .replace("INF_WORDSu", &format!("{INF_WORDS}u"))
        .replace("SPH_WORDSu", &format!("{SPH_WORDS}u"));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("cloth"),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cloth"),
        entries: &[entry(0, true), entry(1, true), entry(2, false), entry(3, false)],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cloth"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let make = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cloth"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    (make("cloth_step"), make("cloth_out"), layout)
}

impl ClothGpu {
    pub fn new(gpu: &Gpu) -> Self {
        let (step, out, layout) = gpu.pipes.cloth().clone();
        Self {
            step,
            out,
            layout,
            data: Vec::new(),
            uploaded: 0,
            data_buf: None,
            models: HashMap::new(),
            state_buf: None,
            top: 0,
            free: Default::default(),
            jobs: Vec::new(),
            job_buf: None,
            group: Default::default(),
        }
    }

    /// Starts a frame's jobs.
    pub fn begin(&mut self) {
        self.jobs.clear();
    }

    /// A model's block offset, queued for upload the first time.
    fn model(&mut self, m: &Arc<ClothModel>) -> u32 {
        let key = Arc::as_ptr(m) as usize;
        if let Some((_, at)) = self.models.get(&key) {
            return *at;
        }
        let at = (self.uploaded + self.data.len()) as u32;
        // Offsets in the block are relative: the shader adds the block's start.
        self.data.extend_from_slice(&m.words);
        self.models.insert(key, (m.clone(), at));
        at
    }

    fn alloc(&mut self, len: u32) -> Slot {
        let reuse = self.free.lock().ok().and_then(|mut f| {
            let k = f.iter().position(|r| r.1 >= len)?;
            let (at, have) = f.swap_remove(k);
            if have > len {
                f.push((at + len, have - len));
            }
            Some(at)
        });
        let at = reuse.unwrap_or_else(|| {
            let at = self.top;
            self.top += len;
            at
        });
        Slot { at, len, placed: false, free: self.free.clone() }
    }

    /// Queues one actor's cloth: its tick, and (`out`: posed this frame at pose vec4 `region` with `bones`, its model
    /// matrix, and the actor's move since the last step) the particles into its pose region. Before the first step
    /// nothing is written and the cloth draws skinned. True when the tick was taken: a step needs this frame's pose
    /// and the model's sheets on the GPU, a carry a placed state; an untaken tick is the caller's to give again.
    pub fn job(
        &mut self,
        st: &mut ClothState,
        m: &Arc<ClothModel>,
        tick: Tick,
        region: Option<(u32, u32)>,
        out: Option<(Mat4, Vec3)>,
    ) -> bool {
        let block = self.model(m);
        // Its sheets go up under the frames' copy budget (dispatch); until they are all there it draws skinned.
        if block as usize + m.words.len() > self.uploaded {
            return false;
        }
        // A slot from another renderer's buffer (our actor keeps its model across a zone load) is not ours: take
        // a new one, which starts with the snap to the skin.
        if st.0.as_ref().is_none_or(|s| s.len != m.state || !Arc::ptr_eq(&s.free, &self.free)) {
            st.0 = Some(self.alloc(m.state));
        }
        let Some(slot) = st.0.as_mut() else { return false };
        let (mut flags, mut moved, mut wind) = (0, Vec3::ZERO, None);
        match tick {
            Tick::Step { moved: d, wind: w } if region.is_some() => {
                flags = STEP | if slot.placed { 0 } else { RESET };
                (moved, wind) = (d, w);
                slot.placed = true;
            }
            Tick::Carry { moved: d } if slot.placed => (flags, moved) = (CARRY, d),
            _ => {}
        }
        let taken = flags & (STEP | CARRY) != 0;
        let (inv, ride) = match out.filter(|_| slot.placed && region.is_some()) {
            Some((model, ride)) => {
                flags |= OUT;
                // A step or carry this frame puts the particles where the actor stands: nothing rides yet.
                (model.inverse(), if taken { Vec3::ZERO } else { ride })
            }
            None => (Mat4::IDENTITY, Vec3::ZERO),
        };
        if flags == 0 {
            return false;
        }
        if wind.is_some() {
            flags |= WIND;
        }
        let (region, bones) = region.unwrap_or((0, 0));
        let (dir, strength) = wind.unwrap_or((Vec3::ZERO, 0.0));
        if self.jobs.is_empty() {
            self.jobs.resize(JOB_HEAD, 0);
        }
        for &sh in &m.sheets {
            let mut j = [0u32; JOB_WORDS];
            (j[0], j[1], j[2], j[3], j[4]) = (flags, block + sh, slot.at, region, bones);
            for (k, v) in moved.to_array().into_iter().chain(dir.to_array()).chain([strength]).enumerate() {
                j[5 + k] = v.to_bits();
            }
            for (k, v) in ride.to_array().into_iter().enumerate() {
                j[12 + k] = v.to_bits();
            }
            j[15] = block;
            for (k, v) in inv.to_cols_array().into_iter().enumerate() {
                j[16 + k] = v.to_bits();
            }
            self.jobs.extend(j);
        }
        self.jobs[0] = ((self.jobs.len() - JOB_HEAD) / JOB_WORDS) as u32;
        taken
    }

    /// New model blocks up (under the frame's copy budget, or all of them with `all`, as the posing's keys), the state
    /// buffer grown (its contents carried over), then the steps and the particles' writes into `pose`, after the
    /// bones are posed into it.
    pub fn dispatch(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, pose: &wgpu::Buffer, all: bool) {
        let device = &gpu.device;
        if !self.data.is_empty() {
            let need = ((self.uploaded + self.data.len()) * 4) as u64;
            if self.data_buf.as_ref().is_none_or(|b| b.size() < need) {
                let size = need.next_power_of_two().max(1 << 16);
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("cloth models"),
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                if let Some(old) = &self.data_buf
                    && self.uploaded > 0
                {
                    encoder.copy_buffer_to_buffer(old, 0, &buf, 0, (self.uploaded * 4) as u64);
                }
                self.data_buf = Some(buf);
            }
            if all || gpu.upload_room() > 0 {
                let n = if all { self.data.len() } else { (gpu.piece(self.data.len() as u64 * 4, 4) / 4) as usize };
                gpu.upload(n as u64 * 4, || {
                    gpu.queue.put(
                        self.data_buf.as_ref().unwrap(),
                        (self.uploaded * 4) as u64,
                        bytemuck::cast_slice(&self.data[..n]),
                    )
                });
                self.uploaded += n;
                self.data.drain(..n);
            }
        }
        let need = (self.top.max(1) as u64) * 16;
        if self.state_buf.as_ref().is_none_or(|b| b.size() < need) {
            let size = need.next_power_of_two().max(1 << 16);
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cloth state"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            if let Some(old) = &self.state_buf {
                encoder.copy_buffer_to_buffer(old, 0, &buf, 0, old.size());
            }
            self.state_buf = Some(buf);
        }
        if self.jobs.is_empty() || self.data_buf.is_none() {
            return;
        }
        let jsize = (self.jobs.len() * 4) as u64;
        if self.job_buf.as_ref().is_none_or(|(_, c)| *c < jsize) {
            let c = jsize.next_power_of_two().max(4096);
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cloth jobs"),
                size: c,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.job_buf = Some((buf, c));
        }
        let (jb, _) = self.job_buf.as_ref().unwrap();
        gpu.queue.put(jb, 0, bytemuck::cast_slice(&self.jobs));
        let (data, state) = (self.data_buf.as_ref().unwrap(), self.state_buf.as_ref().unwrap());
        let group = self.group.get([data.clone(), jb.clone(), pose.clone(), state.clone()], || {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cloth"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: data.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: jb.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: pose.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: state.as_entire_binding() },
                ],
            })
        });
        let groups = (self.jobs[0]).div_ceil(64);
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("cloth"), timestamp_writes: None });
        pass.set_bind_group(0, &group, &[]);
        // The particles' writes are a separate dispatch: they see every step's.
        pass.set_pipeline(&self.step);
        pass.dispatch_workgroups(groups, 1, 1);
        pass.set_pipeline(&self.out);
        pass.dispatch_workgroups(groups, 1, 1);
    }
}

const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> data: array<u32>;
@group(0) @binding(1) var<storage, read> jobs: array<u32>;
@group(0) @binding(2) var<storage, read_write> pose: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> state: array<vec4<f32>>;

const STEP: u32 = 1u;
const CARRY: u32 = 2u;
const RESET: u32 = 4u;
const WIND: u32 = 8u;
const OUT: u32 = 16u;

// Record words (render/cloth.rs `rec`).
const R_N: u32 = 0u;
const R_COUNT0: u32 = 1u;
const R_PINS0: u32 = 2u;
const R_PINS1: u32 = 3u;
const R_EDGES: u32 = 4u;
const R_GROUP1: u32 = 5u;
const R_TRIS: u32 = 6u;
const R_SPHERES: u32 = 7u;
const R_INF_AT: u32 = 8u;
const R_EDGE_AT: u32 = 9u;
const R_TRI_AT: u32 = 10u;
const R_SPH_AT: u32 = 11u;
const R_DAMPING: u32 = 12u;
const R_GRAVITY: u32 = 13u;
const R_INERTIA: u32 = 14u;
const R_K0: u32 = 15u;
const R_K1: u32 = 16u;
const R_BASE: u32 = 17u;
const R_ST_P: u32 = 18u;
const R_ST_SPREV: u32 = 19u;
const R_ST_SCUR: u32 = 20u;
const R_ST_OWN: u32 = 21u;

fn fl(i: u32) -> f32 { return bitcast<f32>(data[i]); }
fn jf(j: u32, k: u32) -> f32 { return bitcast<f32>(jobs[j + k]); }
fn jv3(j: u32, k: u32) -> vec3<f32> { return vec3<f32>(jf(j, k), jf(j, k + 1u), jf(j, k + 2u)); }

// glam's Quat * Vec3, Vec3::length and FUN_100274b0 (a zero vector stays zero).
fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let b = q.xyz;
    let b2 = dot(b, b);
    return v * (q.w * q.w - b2) + b * (dot(v, b) * 2.0) + cross(b, v) * (q.w * 2.0);
}
fn vlen(v: vec3<f32>) -> f32 { return sqrt(dot(v, v)); }
fn norm(v: vec3<f32>) -> vec3<f32> {
    let l = vlen(v);
    if (l > 0.0) { return v / l; }
    return vec3<f32>(0.0);
}

// The job's sheet: its record and the block its offsets count from.
struct Sheet { r: u32, b: u32, st: u32 }

fn rw(s: Sheet, k: u32) -> u32 { return data[s.r + k]; }
fn rf(s: Sheet, k: u32) -> f32 { return fl(s.r + k); }

// Particle i's state vec4s: cur, prev, normal.
fn pi(s: Sheet, i: u32) -> u32 { return s.st + rw(s, R_ST_P) + 3u * i; }

fn pinned(s: Sheet, i: u32) -> bool {
    let c0 = rw(s, R_COUNT0);
    if (i < c0) { return i < rw(s, R_PINS0); }
    return i - c0 < rw(s, R_PINS1);
}

// A bone of the pose region (quat, translation, scale); one past the posed bones is the identity slot.
fn bone_q(region: u32, bones: u32, b: u32) -> vec4<f32> { return pose[region + POSE_HEADu + PER_BONEu * min(b, bones)]; }
fn bone_t(region: u32, bones: u32, b: u32) -> vec3<f32> { return pose[region + POSE_HEADu + PER_BONEu * min(b, bones) + 1u].xyz; }
fn bone_s(region: u32, bones: u32, b: u32) -> vec3<f32> { return pose[region + POSE_HEADu + PER_BONEu * min(b, bones) + 2u].xyz; }

// Mat4::transform_point3 by the region's model matrix.
fn model_point(region: u32, p: vec3<f32>) -> vec3<f32> {
    var r = pose[region] * p.x;
    r = pose[region + 1u] * p.y + r;
    r = pose[region + 2u] * p.z + r;
    r = pose[region + 3u] + r;
    return r.xyz;
}

// skin (FUN_100235c0 / FUN_10023620): each bone adds its rotation of the position and its translation x weight.
fn skin(s: Sheet, region: u32, bones: u32, i: u32) -> vec3<f32> {
    let t = s.b + rw(s, R_INF_AT) + 2u * i;
    let at = s.b + data[t];
    let n = data[t + 1u];
    var p = vec3<f32>(0.0);
    for (var k = 0u; k < n; k++) {
        let x = at + INF_WORDSu * k;
        let b = data[x];
        let pos = vec3<f32>(fl(x + 1u), fl(x + 2u), fl(x + 3u));
        p = p + (qrot(bone_q(region, bones, b), bone_s(region, bones, b) * pos) + bone_t(region, bones, b) * fl(x + 4u));
    }
    return model_point(region, p);
}

fn edge_a(s: Sheet, e: u32) -> u32 { return data[s.b + rw(s, R_EDGE_AT) + 3u * e]; }
fn edge_b(s: Sheet, e: u32) -> u32 { return data[s.b + rw(s, R_EDGE_AT) + 3u * e + 1u]; }
fn edge_rest(s: Sheet, e: u32) -> f32 { return fl(s.b + rw(s, R_EDGE_AT) + 3u * e + 2u); }

// FUN_10023700: Verlet, gravity, wind, then four passes over the edges in order.
fn integrate(s: Sheet, wind: bool, dir: vec3<f32>, strength: f32) {
    let n = rw(s, R_N);
    let damping = rf(s, R_DAMPING);
    let gravity = rf(s, R_GRAVITY);
    for (var i = 0u; i < n; i++) {
        if (pinned(s, i)) { continue; }
        let k = pi(s, i);
        var cur = state[k].xyz;
        let vel = (cur - state[k + 1u].xyz) * damping;
        state[k + 1u] = vec4<f32>(cur, 0.0);
        cur = cur + vel;
        cur.y = cur.y + gravity * 0.1;
        if (wind) {
            let normal = state[k + 2u].xyz;
            var w = normal + dir;
            if (dot(w, w) < 0.01) { w = dir - normal; }
            cur = cur + w * (strength * 0.1);
        }
        state[k] = vec4<f32>(cur, 0.0);
    }
    let edges = rw(s, R_EDGES);
    let group1 = rw(s, R_GROUP1);
    let k0 = rf(s, R_K0);
    let k1 = rf(s, R_K1);
    for (var it = 0u; it < 4u; it++) {
        for (var e = 0u; e < edges; e++) {
            let k = select(k0, k1, e >= group1);
            let ea = edge_a(s, e);
            let eb = edge_b(s, e);
            let a = pi(s, ea & 0x7fffu);
            let b = pi(s, eb & 0x7fffu);
            var d = state[b].xyz - state[a].xyz;
            let len = vlen(d);
            if (len > 1e-4) {
                d = d * ((edge_rest(s, e) - len) * k / len);
                if ((ea & 0x8000u) == 0u) { state[a] = vec4<f32>(state[a].xyz - d, 0.0); }
                if ((eb & 0x8000u) == 0u) { state[b] = vec4<f32>(state[b].xyz + d, 0.0); }
            }
        }
    }
}

struct Hit { k: u32, push: vec3<f32> }

// FUN_10024410: the segment A-B against the sphere (c, r). 0 both inside, 4 apart; 1 / 2 / 3 push B / A / both.
fn out_of(v: vec3<f32>, r: f32) -> vec3<f32> { return norm(v) * (r - vlen(v)); }
fn segment_sphere(r: f32, c: vec3<f32>, a: vec3<f32>, b: vec3<f32>) -> Hit {
    let r2 = r * r;
    let da = a - c;
    let db = b - c;
    let u = norm(b - a);
    let bits = select(0u, 1u, r2 < dot(da, da)) | select(0u, 2u, r2 < dot(db, db));
    if (bits == 1u) {
        let nu = -u;
        let q = nu * dot(c - b, nu) + b;
        if (1e-4 <= dot(norm(q - b), nu)) {
            let l = q - c;
            if (r2 < dot(l, l)) { return Hit(4u, vec3<f32>(0.0)); }
            return Hit(1u, out_of(l, r));
        }
        return Hit(1u, out_of(db, r));
    }
    if (bits == 2u) {
        let q = u * dot(c - a, u) + a;
        if (dot(norm(q - a), u) < 1e-4) { return Hit(2u, out_of(da, r)); }
        let l = q - c;
        if (r2 < dot(l, l)) { return Hit(4u, vec3<f32>(0.0)); }
        return Hit(2u, out_of(l, r));
    }
    if (bits == 3u) {
        let q = u * dot(c - a, u) + a;
        if (dot(norm(q - a), u) < 1e-4 || 0.0 < dot(norm(q - b), u)) { return Hit(4u, vec3<f32>(0.0)); }
        let l = q - c;
        if (r2 < dot(l, l)) { return Hit(4u, vec3<f32>(0.0)); }
        return Hit(3u, out_of(l, r));
    }
    return Hit(0u, vec3<f32>(0.0));
}

// FUN_10024020: the edge a-b against a sphere of radius r swept from *c to `to` in steps of r.
fn sweep(s: Sheet, ea: u32, eb: u32, c: ptr<function, vec3<f32>>, to: vec3<f32>, r: f32) {
    let ai = pi(s, ea & 0x7fffu);
    let bi = pi(s, eb & 0x7fffu);
    let a_free = (ea & 0x8000u) == 0u;
    let b_free = (eb & 0x8000u) == 0u;
    let a_prev = state[ai + 1u].xyz;
    let a_cur = state[ai].xyz;
    let b_cur = state[bi].xyz;
    let b_prev = select(b_cur, state[bi + 1u].xyz, b_free);
    let l1 = a_cur - b_prev;
    let l2 = b_cur - a_prev;
    let f1 = dot(l1, l1);
    let f2 = dot(l2, l2);
    var mid_e = a_prev + 0.5 * l2;
    var r_e = f2 * 0.5;
    if (f2 < f1) {
        mid_e = b_prev + 0.5 * l1;
        r_e = f1 * 0.5;
    }
    let d = to - *c;
    let fd = dot(d, d);
    let mid_s = *c + 0.5 * d;
    let r_s = select(r * r + fd, r * r, fd < 1e-6);
    let m = mid_e - mid_s;
    if (dot(m, m) > r_s + r_e) { return; }
    var dist = vlen(d);
    let dir = norm(d);
    // Ours: a cap a sane sphere never reaches (one step a radius).
    for (var guard = 0u; guard < 4096u; guard++) {
        let h = segment_sphere(r, *c, state[ai].xyz, state[bi].xyz);
        if (h.k == 0u || h.k > 3u) { return; }
        if ((h.k == 1u || h.k == 3u) && b_free) { state[bi] = vec4<f32>(state[bi].xyz + h.push, 0.0); }
        if ((h.k == 2u || h.k == 3u) && a_free) { state[ai] = vec4<f32>(state[ai].xyz + h.push, 0.0); }
        dist = dist - r;
        if (dist >= 1e-6) {
            *c = *c + dir * r;
        } else {
            *c = to;
        }
        if (dist <= 0.0) { return; }
    }
}

// FUN_10023bd0: each edge takes the last sphere its first particle is near; each sphere sweeps its edges both ways.
fn collide(s: Sheet, region: u32, bones: u32) {
    let ns = rw(s, R_SPHERES);
    let sph = s.b + rw(s, R_SPH_AT);
    let scur = s.st + rw(s, R_ST_SCUR);
    // The centres this step (w: the radius; 0 for a slot that takes no edges).
    for (var i = 0u; i < ns; i++) {
        let x = sph + SPH_WORDSu * i;
        var c = vec4<f32>(0.0);
        if ((data[x] & 2u) != 0u) {
            let b = data[x + 2u];
            let off = vec3<f32>(fl(x + 3u), fl(x + 4u), fl(x + 5u));
            let at = bone_t(region, bones, b) + qrot(bone_q(region, bones, b), bone_s(region, bones, b) * off);
            c = vec4<f32>(model_point(region, at), fl(x + 6u));
        }
        state[scur + i] = c;
    }
    // DAT_10456e38: the edge's sphere as a signed char of its table index, -1 none (stored as floats).
    let edges = rw(s, R_EDGES);
    let own = s.st + rw(s, R_ST_OWN);
    for (var e = 0u; e < edges; e++) {
        let a = state[pi(s, edge_a(s, e) & 0x7fffu)].xyz;
        var o = -1;
        for (var k = ns; k > 0u; k--) {
            let i = k - 1u;
            let x = sph + SPH_WORDSu * i;
            if ((data[x] & 1u) == 0u) { continue; }
            let c = state[scur + i];
            if (c.w > 0.0) {
                let d = a - c.xyz;
                if (dot(d, d) <= c.w * c.w * 1.5) {
                    let t = data[x + 1u] & 0xffu;
                    o = select(i32(t), i32(t) - 256, t >= 128u);
                    break;
                }
            }
        }
        state[own + e / 4u][e % 4u] = f32(o);
    }
    let sprev = s.st + rw(s, R_ST_SPREV);
    for (var i = 0u; i < ns; i++) {
        let x = sph + SPH_WORDSu * i;
        let cur = state[scur + i];
        if ((data[x] & 3u) != 3u || cur.w <= 0.0) { continue; }
        let si = i32(data[x + 1u]);
        let p = state[sprev + i];
        var c = select(cur.xyz, p.xyz, p.w == 1.0);
        for (var e = 0u; e < edges; e++) {
            let ea = edge_a(s, e);
            if (i32(state[own + e / 4u][e % 4u]) != si || (ea & 0x8000u) != 0u) { continue; }
            let eb = edge_b(s, e);
            sweep(s, ea, eb, &c, cur.xyz, cur.w);
            if ((eb & 0x8000u) == 0u) { sweep(s, eb, ea, &c, cur.xyz, cur.w); }
        }
        state[sprev + i] = vec4<f32>(cur.xyz, 1.0);
    }
}

// FUN_10024900: (0, 1, 0) plus every triangle's cross(v2 - v0, v1 - v0), normalised.
fn normals(s: Sheet) {
    let n = rw(s, R_N);
    for (var i = 0u; i < n; i++) { state[pi(s, i) + 2u] = vec4<f32>(0.0, 1.0, 0.0, 0.0); }
    let tris = rw(s, R_TRIS);
    let t0 = s.b + rw(s, R_TRI_AT);
    for (var t = 0u; t < tris; t++) {
        let i0 = pi(s, data[t0 + 3u * t]);
        let i1 = pi(s, data[t0 + 3u * t + 1u]);
        let i2 = pi(s, data[t0 + 3u * t + 2u]);
        let nn = cross(state[i2].xyz - state[i0].xyz, state[i1].xyz - state[i0].xyz);
        state[i0 + 2u] = vec4<f32>(state[i0 + 2u].xyz + nn, 0.0);
        state[i1 + 2u] = vec4<f32>(state[i1 + 2u].xyz + nn, 0.0);
        state[i2 + 2u] = vec4<f32>(state[i2 + 2u].xyz + nn, 0.0);
    }
    for (var i = 0u; i < n; i++) {
        let k = pi(s, i) + 2u;
        state[k] = vec4<f32>(norm(state[k].xyz), 0.0);
    }
}

fn sheet_of(j: u32) -> Sheet { return Sheet(jobs[j + 1u], jobs[j + 15u], jobs[j + 2u]); }

// FUN_10023290 (STEP) or FUN_100231f0 (CARRY) for one sheet of one actor.
@compute @workgroup_size(64)
fn cloth_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= jobs[0]) { return; }
    let j = JOB_HEADu + gid.x * JOB_WORDSu;
    let flags = jobs[j];
    let s = sheet_of(j);
    let n = rw(s, R_N);
    let moved = jv3(j, 5u);
    if ((flags & CARRY) != 0u) {
        for (var i = 0u; i < n; i++) {
            let k = pi(s, i);
            state[k] = vec4<f32>(state[k].xyz + moved, 0.0);
            state[k + 1u] = vec4<f32>(state[k + 1u].xyz + moved, 0.0);
        }
        return;
    }
    if ((flags & STEP) == 0u) { return; }
    let region = jobs[j + 3u];
    let bones = jobs[j + 4u];
    if ((flags & RESET) != 0u) {
        for (var i = 0u; i < n; i++) {
            let k = pi(s, i);
            let p = vec4<f32>(skin(s, region, bones, i), 0.0);
            state[k] = p;
            state[k + 1u] = p;
        }
        // A new slot's last centres are unset (the CPU's start as None).
        let sprev = s.st + rw(s, R_ST_SPREV);
        for (var i = 0u; i < rw(s, R_SPHERES); i++) { state[sprev + i] = vec4<f32>(0.0); }
    } else {
        let inertia = rf(s, R_INERTIA);
        let d_cur = (0.75 - inertia) * moved;
        let d_prev = 0.75 * moved;
        for (var i = 0u; i < n; i++) {
            let k = pi(s, i);
            if (pinned(s, i)) {
                state[k] = vec4<f32>(skin(s, region, bones, i), 0.0);
            } else {
                state[k] = vec4<f32>(state[k].xyz + d_cur, 0.0);
                state[k + 1u] = vec4<f32>(state[k + 1u].xyz + d_prev, 0.0);
            }
        }
        integrate(s, (flags & WIND) != 0u, jv3(j, 8u), jf(j, 11u));
        collide(s, region, bones);
    }
    normals(s);
}

// The sheet's particles into the actor's pose region in model space (two vec4s each, w 1 = set), as they ride
// along with the actor between steps.
@compute @workgroup_size(64)
fn cloth_out(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= jobs[0]) { return; }
    let j = JOB_HEADu + gid.x * JOB_WORDSu;
    if ((jobs[j] & OUT) == 0u) { return; }
    let s = sheet_of(j);
    let region = jobs[j + 3u];
    let bones = jobs[j + 4u];
    let ride = jv3(j, 12u);
    let inv = mat4x4<f32>(
        vec4<f32>(jf(j, 16u), jf(j, 17u), jf(j, 18u), jf(j, 19u)),
        vec4<f32>(jf(j, 20u), jf(j, 21u), jf(j, 22u), jf(j, 23u)),
        vec4<f32>(jf(j, 24u), jf(j, 25u), jf(j, 26u), jf(j, 27u)),
        vec4<f32>(jf(j, 28u), jf(j, 29u), jf(j, 30u), jf(j, 31u)));
    let first = region + POSE_HEADu + PER_BONEu * (bones + 1u) + 2u * rw(s, R_BASE);
    let n = rw(s, R_N);
    for (var i = 0u; i < n; i++) {
        let k = pi(s, i);
        let p = (inv * vec4<f32>(state[k].xyz + ride, 1.0)).xyz;
        // Vec3::normalize_or_zero.
        let v = (inv * vec4<f32>(state[k + 2u].xyz, 0.0)).xyz;
        let l2 = dot(v, v);
        var nn = vec3<f32>(0.0);
        if (l2 > 0.0 && l2 < 3.0e38) { nn = v * (1.0 / sqrt(l2)); }
        pose[first + 2u * i] = vec4<f32>(p, 1.0);
        pose[first + 2u * i + 1u] = vec4<f32>(nn, 1.0);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::cloth::{Cloth, Frame};
    use crate::dat::character::BoneXf;

    /// 40 steps of model 1367 (King Destin's cape: 8 spheres) standing in the wind, then walking, on the CPU and the GPU;
    /// with `same`, each GPU step starts from the CPU's state. Returns each step's worst particle distance.
    fn run(same: bool) -> Option<Vec<f32>> {
        let gpu = Gpu::headless().ok()?;
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        let ch = crate::dat::npc::load_file_model(&index, 1367).ok()?;
        let m = ClothModel::build(&ch.parts, &ch.skeleton).map(Arc::new)?;
        let bones_n = ch.skeleton.bones.len().min(super::super::MAX_BONES);
        let bones = crate::dat::character::pose(&ch.skeleton, &[]);
        let mut cpu = Cloth::new(&ch.parts).unwrap();
        let mut gc = ClothGpu::new(&gpu);
        let mut st = ClothState::default();
        // A pose region as pose_actor writes it: model matrix, two header vec4s, the bones, the identity, particles.
        let region_len = POSE_HEAD + POSE_PER_BONE * (bones_n + 1) + 2 * MAX_CLOTH;
        let pose = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (region_len * 2 * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let region = 16u32;
        let size = (region_len * 16) as u64;
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let first = POSE_HEAD + POSE_PER_BONE * (bones_n + 1);
        let wind = Some((Vec3::new(-1.0, 0.0, 0.0), 0.03));
        let mut at = Vec3::new(10.0, 0.0, -5.0);
        let mut out = Vec::new();
        for k in 0..40 {
            let moved = if k < 20 { Vec3::ZERO } else { Vec3::new(0.0, 0.0, 0.12) };
            at += moved;
            let model = Mat4::from_rotation_translation(glam::Quat::from_rotation_y(0.3), at);
            let mut words: Vec<[f32; 4]> = model.to_cols_array_2d().to_vec();
            words.extend([[1.0, 0.0, 1.0, 0.0], [0.0, 1.0, 1.0, 1.0]]);
            for i in 0..bones_n {
                let b = bones.get(i).copied().unwrap_or(BoneXf::IDENTITY);
                words.extend([b.q.to_array(), b.t.extend(0.0).to_array(), b.s.extend(0.0).to_array()]);
            }
            words.extend([[0.0, 0.0, 0.0, 1.0], [0.0; 4], [1.0, 1.0, 1.0, 0.0]]);
            gpu.queue.put(&pose, region as u64 * 16, bytemuck::cast_slice(&words));
            if same && let (Some(slot), Some(buf)) = (st.0.as_ref(), gc.state_buf.as_ref()) {
                for (sh, &rec_at) in cpu.sheets.iter().zip(&m.sheets) {
                    let r = |k: usize| m.words[rec_at as usize + k];
                    let ps: Vec<[f32; 4]> =
                        sh.p.iter()
                            .flat_map(|q| [q.cur.extend(0.0), q.prev.extend(0.0), q.normal.extend(0.0)])
                            .map(|v| v.to_array())
                            .collect();
                    gpu.queue.put(buf, (slot.at + r(rec::ST_P)) as u64 * 16, bytemuck::cast_slice(&ps));
                    let sp: Vec<[f32; 4]> =
                        sh.sphere_prev.iter().map(|c| c.map_or([0.0; 4], |c| c.extend(1.0).to_array())).collect();
                    if !sp.is_empty() {
                        gpu.queue.put(buf, (slot.at + r(rec::ST_SPREV)) as u64 * 16, bytemuck::cast_slice(&sp));
                    }
                }
            }
            cpu.step(&ch.parts, &Frame { skeleton: &ch.skeleton, bones: &bones, model, moved, wind });
            gc.begin();
            if k == 0 {
                // The model's sheets up first (a job waits for them).
                let _ = gc.job(&mut st, &m, Tick::None, None, None);
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                gc.dispatch(&gpu, &mut enc, &pose, true);
                gpu.queue.submit([enc.finish()]);
            }
            let tick = Tick::Step { moved, wind };
            assert!(gc.job(&mut st, &m, tick, Some((region, bones_n as u32)), Some((model, Vec3::ZERO))));
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            gc.dispatch(&gpu, &mut enc, &pose, true);
            enc.copy_buffer_to_buffer(&pose, region as u64 * 16, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let got: Vec<[f32; 4]> = bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec();
            rb.unmap();
            let inv = model.inverse();
            let (mut n, mut worst) = (0, 0f32);
            for sh in &cpu.sheets {
                for q in &sh.p {
                    let g = got[first + 2 * n];
                    assert_eq!(g[3], 1.0, "step {k}: particle {n} not written");
                    worst = worst.max((Vec3::new(g[0], g[1], g[2]) - inv.transform_point3(q.cur)).length());
                    n += 1;
                }
            }
            out.push(worst);
        }
        Some(out)
    }

    fn sci(e: &[f32]) -> Vec<String> {
        e.iter().map(|v| format!("{v:.1e}")).collect()
    }

    /// Each GPU step from the CPU's state: the same loops (reset, skin, Verlet, constraint passes, sweeps, normals)
    /// give the same particles to f32 rounding, but on the steps where a rounding-sized nudge also moves the CPU off
    /// itself (1367 walking: two of the forty).
    #[test]
    fn a_step_matches_the_cpu() {
        let (Some(e), Some(n)) = (run(true), nudged()) else {
            eprintln!("skipped: needs a GPU and the game's files (model 1367)");
            return;
        };
        let ok = e.iter().zip(&n).all(|(&e, &n)| e < 0.05 && (e < 1e-4 || n > 1e-4));
        assert!(ok, "gpu {:?}\nnudged cpu {:?}", sci(&e), sci(&n));
    }

    /// Run separately, the two drift apart by rounding (a sweep's inside / outside test can flip), never visibly.
    #[test]
    fn steps_stay_with_the_cpu() {
        let Some(e) = run(false) else {
            eprintln!("skipped: needs a GPU and the game's files (model 1367)");
            return;
        };
        assert!(e.iter().all(|&e| e < 0.05), "{:?}", sci(&e));
    }

    /// The same 40 steps on the CPU against itself, one copy nudged by ~1e-6 before each step: where a sweep's
    /// inside / outside test sits on its threshold (a particle pushed onto a sphere), rounding alone changes the step.
    fn nudged() -> Option<Vec<f32>> {
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        let ch = crate::dat::npc::load_file_model(&index, 1367).ok()?;
        let bones = crate::dat::character::pose(&ch.skeleton, &[]);
        let mut a = Cloth::new(&ch.parts).unwrap();
        let wind = Some((Vec3::new(-1.0, 0.0, 0.0), 0.03));
        let mut at = Vec3::new(10.0, 0.0, -5.0);
        let mut out = Vec::new();
        for k in 0..40 {
            let moved = if k < 20 { Vec3::ZERO } else { Vec3::new(0.0, 0.0, 0.12) };
            at += moved;
            let model = Mat4::from_rotation_translation(glam::Quat::from_rotation_y(0.3), at);
            let mut b = a.clone();
            for sh in &mut b.sheets {
                for (i, q) in sh.p.iter_mut().enumerate() {
                    q.cur += Vec3::splat(1e-6 * ((i as f32) * 1.7).sin());
                }
            }
            let f = Frame { skeleton: &ch.skeleton, bones: &bones, model, moved, wind };
            a.step(&ch.parts, &f);
            b.step(&ch.parts, &f);
            let mut worst = 0f32;
            for (sa, sb) in a.sheets.iter().zip(&b.sheets) {
                for (qa, qb) in sa.p.iter().zip(&sb.p) {
                    worst = worst.max((qa.cur - qb.cur).length());
                }
            }
            out.push(worst);
        }
        Some(out)
    }

    /// The record layout on a made-up sheet (no GPU, no game files): pins, influences, edges with their fixed-end bits,
    /// triangles, the spheres resolved (a slot past the table none, an unused one no edges), the state offsets.
    #[test]
    fn model_records() {
        use crate::dat::character::{Bone, ClothEdge, ClothSpec, ClothSphere, Influence, MeshPart};
        let inf =
            |bone: usize, x: f32| vec![Influence { bone, pos: Vec3::new(x, 0.0, 0.0), normal: Vec3::Y, weight: 1.0 }];
        let spec = ClothSpec {
            counts: [2, 1],
            pins: [1, 0],
            edges: vec![ClothEdge { a: 0x8000, b: 1, rest: 0.5 }, ClothEdge { a: 1, b: 2, rest: 0.25 }],
            group1: 1,
            k: [0.6, 0.3],
            damping: 0.85,
            gravity: 0.1,
            inertia: 0.05,
            spheres: vec![0, 1, 99],
            tris: vec![[0, 1, 2]],
        };
        let part = MeshPart {
            name: "cape".into(),
            mirrored: false,
            vertices: vec![inf(0, 1.0), inf(1, 2.0), inf(1, 3.0)],
            groups: Vec::new(),
            cloth: true,
            sim: Some(Arc::new(spec)),
        };
        let bone = Bone { parent: 0, term: 0, rot: glam::Quat::IDENTITY, pos: Vec3::ZERO };
        let skeleton = Skeleton {
            name: "test".into(),
            bones: vec![bone.clone(), bone],
            points: Vec::new(),
            point_rots: Vec::new(),
            look_limits: [[0.0; 3]; 2],
            boxes: Vec::new(),
            spheres: vec![
                ClothSphere { bone: 1, radius: 0.2, offset: Vec3::new(0.0, 1.0, 0.0) },
                ClothSphere { bone: -12851, radius: -4.3e8, offset: Vec3::ZERO },
            ],
        };
        let m = ClothModel::build(&[part], &skeleton).unwrap();
        assert_eq!(m.sheets, [0]);
        let w = |k: usize| m.words[k];
        let f = |k: usize| f32::from_bits(m.words[k]);
        assert_eq!([w(rec::N), w(rec::COUNT0), w(rec::PINS0), w(rec::PINS1)], [3, 2, 1, 0]);
        assert_eq!([w(rec::EDGES), w(rec::GROUP1), w(rec::TRIS), w(rec::SPHERES), w(rec::BASE)], [2, 1, 1, 3, 0]);
        assert_eq!([f(rec::DAMPING), f(rec::K0), f(rec::K1)], [0.85, 0.6, 0.3]);
        // Particle 1's influence: one, bone 1 at (2, 0, 0) weight 1.
        let t = w(rec::INF_AT) as usize;
        let (at, n) = (w(t + 2) as usize, w(t + 3));
        assert_eq!((w(at), f(at + 1), f(at + 4), n), (1, 2.0, 1.0, 1));
        let e = w(rec::EDGE_AT) as usize;
        assert_eq!([w(e), w(e + 1), w(e + 3), w(e + 4)], [0x8000, 1, 1, 2]);
        assert_eq!(f(e + 2), 0.5);
        let tr = w(rec::TRI_AT) as usize;
        assert_eq!([w(tr), w(tr + 1), w(tr + 2)], [0, 1, 2]);
        // Spheres: slot 0 used (flags 3), slot 1 the 0xCD fill (in the table, takes no edges: flags 1), 99 none (0).
        let s = w(rec::SPH_AT) as usize;
        assert_eq!([w(s), w(s + 1), w(s + 2)], [3, 0, 1]);
        assert_eq!((f(s + 4), f(s + 6)), (1.0, 0.2));
        assert_eq!([w(s + SPH_WORDS), w(s + 2 * SPH_WORDS)], [1, 0]);
        // State: 3 particles x 3 vec4s, then 3 last centres, 3 centres, one vec4 of edge owners.
        assert_eq!([w(rec::ST_P), w(rec::ST_SPREV), w(rec::ST_SCUR), w(rec::ST_OWN)], [0, 9, 12, 15]);
        assert_eq!(m.state, 16);
    }
}
