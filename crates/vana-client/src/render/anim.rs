//! Motion evaluation on the GPU: animation keys and skeletons go up once per model, and a compute pass
//! poses every world actor each frame (dat/character.rs `pose`, plus the cross-fade of
//! render/character.rs `pose_blend`) straight into the pose regions vs_skinned reads.

use super::Gpu;
use crate::dat::character::{Animation, Character};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// Bones a pose region holds (render/mod.rs MAX_BONES) and the per-animation bone table size.
const TABLE: usize = crate::dat::animpack::TABLE;
/// (animation, frame) pairs per pose; regions are filled by later files first, so a few suffice.
pub const MAX_SET: usize = 8;
/// Job header: skeleton, output, counts, blend, then the head look (weight, two bones, model-space point).
const HEAD: usize = 12;
const JOB_WORDS: usize = HEAD + 4 * MAX_SET;
const NO_BONE: u32 = u32::MAX;

/// A head look for the compute pass (FUN_100d38c0 / FUN_10029e40): the bones of attach points 3 and 7 turn
/// toward a model-space point, half each, at `weight` *(inf: retail's per-bone aim is not decoded; the turn
/// from the body's forward is shared out)*.
#[derive(Clone, Copy, Debug)]
pub struct HeadLook {
    pub bones: [Option<u16>; 2],
    pub weight: f32,
    pub point: glam::Vec3,
}
/// Scratch vec4s per job: local rotation and translation for the target and the blended-from pose.
const SCRATCH: usize = TABLE * 4;

const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> data: array<u32>;
@group(0) @binding(1) var<storage, read> jobs: array<u32>;
@group(0) @binding(2) var<storage, read_write> pose: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> scratch: array<vec4<f32>>;

fn fl(i: u32) -> f32 { return bitcast<f32>(data[i]); }

// Channel::pair: the keys either side of `frame` (clamped to the last) and the fraction, a constant twice with 0;
// key offsets count from the animation `an`.
fn keys(an: u32, c: u32, frame: f32) -> vec3<f32> {
    let n = data[c];
    let v = data[c + 1u];
    if (n == 0u) { return vec3<f32>(bitcast<f32>(v), bitcast<f32>(v), 0.0); }
    let last = n - 1u;
    let f = clamp(frame, 0.0, f32(last));
    let i = min(u32(f), last);
    let j = min(i + 1u, last);
    return vec3<f32>(fl(an + v + i), fl(an + v + j), f - f32(i));
}

fn chan(an: u32, c: u32, frame: f32) -> f32 {
    let k = keys(an, c, frame);
    return k.x + (k.y - k.x) * k.z;
}

// Track::sample's rotation: FUN_10032400 negates the next key when the dot is negative, then lerps.
fn rot_at(an: u32, tr: u32, frame: f32) -> vec4<f32> {
    let x = keys(an, tr, frame);
    let y = keys(an, tr + 2u, frame);
    let z = keys(an, tr + 4u, frame);
    let w = keys(an, tr + 6u, frame);
    let a = vec4<f32>(x.x, y.x, z.x, w.x);
    var b = vec4<f32>(x.y, y.y, z.y, w.y);
    if (dot(a, b) < 0.0) { b = -b; }
    return normalize(mix(a, b, max(max(x.z, y.z), max(z.z, w.z))));
}

fn qmul(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(
        a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z);
}

fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// Shortest rotation taking unit u to unit v.
fn arc(u: vec3<f32>, v: vec3<f32>) -> vec4<f32> {
    let c = dot(u, v);
    if (c < -0.9999) { return vec4<f32>(0.0, 1.0, 0.0, 0.0); }
    return normalize(vec4<f32>(cross(u, v), 1.0 + c));
}

// glam Quat::slerp.
fn slerp(a: vec4<f32>, b0: vec4<f32>, s: f32) -> vec4<f32> {
    var b = b0;
    var d = dot(a, b);
    if (d < 0.0) { b = -b; d = -d; }
    if (d > 1.0 - 1.1920929e-7) { return normalize(a + (b - a) * s); }
    let theta = acos(d);
    let x = 1.0 / sin(theta);
    return (a * sin(theta * (1.0 - s)) + b * sin(theta * s)) * x;
}

// Local transform of bone `b` from the rest pose and a set of (animation, frame): the latest track wins.
fn local_tf(skel: u32, b: u32, job: u32, first: u32, count: u32, q: ptr<function, vec4<f32>>, t: ptr<function, vec3<f32>>) {
    let s = skel + 1u + b * 8u;
    let rq = vec4<f32>(fl(s + 1u), fl(s + 2u), fl(s + 3u), fl(s + 4u));
    let rt = vec3<f32>(fl(s + 5u), fl(s + 6u), fl(s + 7u));
    *q = rq;
    *t = rt;
    for (var k = 0u; k < count; k = k + 1u) {
        let a = jobs[job + first + 2u * k];
        let frame = bitcast<f32>(jobs[job + first + 2u * k + 1u]);
        let ti = data[a + 2u + b];
        if (ti != 0u) {
            let tr = a + 2u + 256u + (ti - 1u) * 20u;
            let r = rot_at(a, tr, frame);
            *q = qmul(r, rq);
            *t = rt + vec3<f32>(chan(a, tr + 8u, frame), chan(a, tr + 10u, frame), chan(a, tr + 12u, frame));
        }
    }
}

@compute @workgroup_size(64)
fn pose_actors(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let job = wg.x * JOB_WORDSu;
    let skel = jobs[job];
    let out = jobs[job + 1u];
    let n_to = jobs[job + 2u];
    let n_from = jobs[job + 3u];
    let blend = bitcast<f32>(jobs[job + 4u]);
    let look_w = bitcast<f32>(jobs[job + 5u]);
    let look_a = jobs[job + 6u];
    let look_b = jobs[job + 7u];
    let look_p = vec3<f32>(bitcast<f32>(jobs[job + 8u]), bitcast<f32>(jobs[job + 9u]), bitcast<f32>(jobs[job + 10u]));
    let n = data[skel];
    let sc = wg.x * SCRATCHu;
    for (var b = lid; b < n; b = b + 64u) {
        var q: vec4<f32>;
        var t: vec3<f32>;
        local_tf(skel, b, job, HEADu, n_to, &q, &t);
        scratch[sc + 4u * b] = q;
        scratch[sc + 4u * b + 1u] = vec4<f32>(t, 0.0);
        if (n_from > 0u) {
            local_tf(skel, b, job, HEADu + 2u * MAX_SETu, n_from, &q, &t);
            scratch[sc + 4u * b + 2u] = q;
            scratch[sc + 4u * b + 3u] = vec4<f32>(t, 0.0);
        }
    }
    storageBarrier();
    workgroupBarrier();
    if (lid != 0u) { return; }
    // Parents precede children: chain in place, both poses.
    for (var b = 0u; b < n; b = b + 1u) {
        if (b > 0u) {
            let p = data[skel + 1u + b * 8u];
            for (var k = 0u; k < 4u; k = k + 2u) {
                if (k == 2u && n_from == 0u) { break; }
                let pq = scratch[sc + 4u * p + k];
                let pt = scratch[sc + 4u * p + k + 1u].xyz;
                let lq = scratch[sc + 4u * b + k];
                let lt = scratch[sc + 4u * b + k + 1u].xyz;
                scratch[sc + 4u * b + k] = qmul(pq, lq);
                scratch[sc + 4u * b + k + 1u] = vec4<f32>(pt + qrot(pq, lt), 0.0);
            }
        }
        // Head look: turn this bone (and so its children) toward the point, its share of the way.
        if (look_w > 0.0 && (b == look_a || b == look_b)) {
            let share = select(1.0, 0.5, look_a != 0xffffffffu && look_b != 0xffffffffu);
            for (var k = 0u; k < 4u; k = k + 2u) {
                if (k == 2u && n_from == 0u) { break; }
                let hp = scratch[sc + 4u * b + k + 1u].xyz;
                let d = look_p - hp;
                if (length(d) > 1e-4) {
                    let dq = slerp(vec4<f32>(0.0, 0.0, 0.0, 1.0), arc(vec3<f32>(1.0, 0.0, 0.0), normalize(d)), look_w * share);
                    scratch[sc + 4u * b + k] = normalize(qmul(dq, scratch[sc + 4u * b + k]));
                }
            }
        }
        var q = scratch[sc + 4u * b];
        var t = scratch[sc + 4u * b + 1u].xyz;
        if (n_from > 0u) {
            q = slerp(scratch[sc + 4u * b + 2u], q, blend);
            t = mix(scratch[sc + 4u * b + 3u].xyz, t, blend);
        }
        pose[out + 2u * b] = q;
        pose[out + 2u * b + 1u] = vec4<f32>(t, 0.0);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::character::{Look, load_character, pose};

    /// The compute pass gives the CPU pose (and its cross-fade) for a Hume run blended from idle.
    #[test]
    fn gpu_pose_matches_cpu() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let look = Look { race: 1, face: 0, head: 0, body: 8, hands: 8, legs: 8, feet: 8, main: 0, sub: 0 };
        let Ok(ch) = load_character(&index, &look) else { return };
        let ch = Arc::new(ch);
        let Ok(gpu) = Gpu::headless() else { return };
        let to: Vec<(&Animation, f32)> = ch.find_animations("run").into_iter().map(|a| (a, 3.37)).collect();
        let from: Vec<(&Animation, f32)> = ch.find_animations("idl").into_iter().map(|a| (a, 7.5)).collect();
        assert!(!to.is_empty() && !from.is_empty());
        check(&gpu, &ch, &to, &from);
        // Pugil idle: its keys flip sign between frames (the short-way lerp).
        let Ok(pugil) = crate::dat::npc::load_model(&index, 0x15c) else { return };
        let pugil = Arc::new(pugil);
        let idle: Vec<(&Animation, f32)> = pugil.find_animations("idl").into_iter().map(|a| (a, 4.5)).collect();
        assert!(!idle.is_empty());
        check(&gpu, &pugil, &idle, &[]);
    }

    fn check(gpu: &Gpu, ch: &Arc<Character>, to: &[(&Animation, f32)], from: &[(&Animation, f32)]) {
        for (f, t) in [(from, 0.3f32), (&[][..], 0.0)] {
            if f.is_empty() && t != 0.0 {
                continue;
            }
            let cpu = if f.is_empty() { pose(&ch.skeleton, &to) } else { crate::render::character::pose_blend(&ch, f, &to, t) };
            let mut ga = GpuAnim::new(&gpu);
            ga.begin();
            ga.job(&ch, &[], &to, f, t, 0, None);
            let n = cpu.len().min(TABLE - 1);
            let size = (n * 32) as u64;
            let out = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            ga.dispatch(&gpu, &mut enc, &out, true);
            enc.copy_buffer_to_buffer(&out, 0, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let data = rb.slice(..).get_mapped_range().unwrap();
            let v: &[f32] = bytemuck::cast_slice(&data);
            let mut worst = 0f32;
            for (i, (q, p)) in cpu.iter().take(n).enumerate() {
                let g = &v[i * 8..i * 8 + 8];
                let gq = glam::Quat::from_slice(&g[0..4]);
                let dq = (gq - *q).length().min((gq + *q).length());
                let dp = (glam::Vec3::from_slice(&g[4..7]) - *p).length();
                worst = worst.max(dq).max(dp);
            }
            assert!(worst < 1e-3, "blend {t}: worst difference {worst}");
        }
    }
}

/// Keys and skeletons in one u32 buffer (floats as bits), jobs per frame, and the compute pipeline.
pub struct GpuAnim {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Words added since the last upload; they go to `uploaded` and up.
    data: Vec<u32>,
    uploaded: usize,
    data_buf: Option<wgpu::Buffer>,
    /// Uploaded sources by Arc address (the Arc is kept so the address stays theirs): skeleton offset and
    /// animation offsets in order.
    /// Uploaded sources by Arc address: skeleton offset, animation offsets, and the end of their words.
    sources: HashMap<usize, (Arc<dyn Any + Send + Sync>, Option<u32>, Vec<u32>, u32)>,
    /// Where each animation block (by Packed::key) went: start and end.
    blocks: HashMap<u64, (u32, u32)>,
    jobs: Vec<u32>,
    job_buf: Option<(wgpu::Buffer, u64)>,
    scratch: Option<(wgpu::Buffer, u64)>,
}

/// The posing pipeline and its layout (made once per device, render::PipeCache).
pub fn pipeline(device: &wgpu::Device) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    {
        let src = WGSL.replace("JOB_WORDSu", &format!("{JOB_WORDS}u")).replace("SCRATCHu", &format!("{SCRATCH}u")).replace("MAX_SETu", &format!("{MAX_SET}u")).replace("HEADu", &format!("{HEAD}u"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pose actors"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pose actors"), entries: &[entry(0, true), entry(1, true), entry(2, false), entry(3, false)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pose actors"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("pose actors"), layout: Some(&pl), module: &module, entry_point: Some("pose_actors"), compilation_options: Default::default(), cache: None });
        (pipeline, layout)
    }
}

impl GpuAnim {
    pub fn new(gpu: &Gpu) -> Self {
        let (pipeline, layout) = gpu.pipes.anim().clone();
        Self { pipeline, layout, data: Vec::new(), uploaded: 0, data_buf: None, sources: HashMap::new(), blocks: HashMap::new(), jobs: Vec::new(), job_buf: None, scratch: None }
    }

    /// Offsets of a source's skeleton (if any) and animations, uploading them the first time. The loader
    /// threads pack them (`packed`); packing here is only for sources that skipped that.
    fn source(&mut self, key: usize, keep: Arc<dyn Any + Send + Sync>, skel: Option<&crate::dat::character::Skeleton>, anims: &[Animation], packed: Option<&crate::dat::animpack::Packed>) -> (Option<u32>, &[u32]) {
        if !self.sources.contains_key(&key) {
            let at = |d: &Vec<u32>, up: usize| (up + d.len()) as u32;
            let s = skel.map(|sk| {
                let s = at(&self.data, self.uploaded);
                self.data.push(sk.bones.len().min(TABLE - 1) as u32);
                for (i, b) in sk.bones.iter().take(TABLE - 1).enumerate() {
                    self.data.push(if i == 0 { 0 } else { b.parent as u32 });
                    self.data.extend(b.rot.to_array().map(f32::to_bits));
                    self.data.extend(b.pos.to_array().map(f32::to_bits));
                }
                s
            });
            let local;
            let p = match packed {
                Some(p) => p,
                None => {
                    local = crate::dat::animpack::pack(anims);
                    &local
                }
            };
            let (base, block_end) = match self.blocks.get(&p.key) {
                Some(&b) => b,
                None => {
                    let b = at(&self.data, self.uploaded);
                    self.data.extend_from_slice(&p.words);
                    let e = at(&self.data, self.uploaded);
                    self.blocks.insert(p.key, (b, e));
                    crate::client::perf::note(|| format!("anim block {} words", p.words.len()));
                    (b, e)
                }
            };
            let offs = p.offsets.iter().map(|o| base + o).collect();
            let end = block_end.max(s.map_or(0, |_| at(&self.data, self.uploaded)));
            self.sources.insert(key, (keep, s, offs, end));
        }
        let e = &self.sources[&key];
        (e.1, &e.2)
    }

    /// Adds a model's skeleton and animations ahead of its first job.
    pub fn register(&mut self, ch: &Arc<Character>) {
        self.source(Arc::as_ptr(ch) as usize, ch.clone(), Some(&ch.skeleton), &ch.animations, ch.packed.as_deref());
    }

    /// A registered model's words are all on the GPU.
    pub fn ready(&self, ch: &Arc<Character>) -> bool {
        self.sources.get(&(Arc::as_ptr(ch) as usize)).is_some_and(|s| s.3 as usize <= self.uploaded)
    }

    /// Starts a frame's jobs.
    pub fn begin(&mut self) {
        self.jobs.clear();
    }

    /// Queues one actor: its model, the motion files on it, the pose (and the one it blends from at `t`),
    /// written to pose vec4 index `out`. Animations are found in the model or its files by address.
    #[allow(clippy::too_many_arguments)]
    pub fn job(&mut self, ch: &Arc<Character>, extra: &[Arc<crate::dat::motionfile::MotionFile>], to: &[(&Animation, f32)], from: &[(&Animation, f32)], t: f32, out: u32, look: Option<HeadLook>) {
        let (Some(skel), _) = self.source(Arc::as_ptr(ch) as usize, ch.clone(), Some(&ch.skeleton), &ch.animations, ch.packed.as_deref()) else { return };
        let mut lookup = |a: &Animation| -> Option<u32> {
            let find = |list: &[Animation]| list.iter().position(|x| std::ptr::eq(x, a));
            if let Some(i) = find(&ch.animations) {
                return self.sources[&(Arc::as_ptr(ch) as usize)].2.get(i).copied();
            }
            for f in extra {
                if let Some(i) = find(&f.animations) {
                    let key = Arc::as_ptr(f) as usize;
                    self.source(key, f.clone(), None, &f.animations, f.packed.as_deref());
                    // A motion file whose words are not up yet sits this frame out.
                    let e = &self.sources[&key];
                    return if e.3 as usize <= self.uploaded { e.2.get(i).copied() } else { None };
                }
            }
            None
        };
        let mut words = vec![0u32; JOB_WORDS];
        words[0] = skel;
        words[1] = out;
        words[4] = t.to_bits();
        words[6] = NO_BONE;
        words[7] = NO_BONE;
        if let Some(l) = look.filter(|l| l.weight > 0.0) {
            words[5] = l.weight.to_bits();
            for (i, b) in l.bones.iter().enumerate() {
                words[6 + i] = b.map_or(NO_BONE, |b| b as u32);
            }
            words[8..11].copy_from_slice(&l.point.to_array().map(f32::to_bits));
        }
        for (set, list, count_at) in [(HEAD, to, 2usize), (HEAD + 2 * MAX_SET, from, 3)] {
            let mut k = 0;
            for (a, frame) in list.iter() {
                if k == MAX_SET {
                    break;
                }
                if let Some(off) = lookup(a) {
                    words[set + 2 * k] = off;
                    words[set + 2 * k + 1] = frame.to_bits();
                    k += 1;
                }
            }
            words[count_at] = k as u32;
        }
        self.jobs.extend(words);
    }

    /// Words registered but not yet on the GPU go up (under the frame copy budget, or all with `all`), whether or
    /// not anything is posed this frame: models wait for theirs before they are drawn.
    pub fn upload_keys(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, all: bool) {
        let device = &gpu.device;
        // Only new words go up; a bigger buffer takes the old contents by a GPU copy.
        if !self.data.is_empty() || self.data_buf.is_none() {
            let need = ((self.uploaded + self.data.len()).max(4) * 4) as u64;
            if self.data_buf.as_ref().is_none_or(|b| b.size() < need) {
                let size = need.next_power_of_two().max(1 << 20);
                crate::client::perf::note(|| format!("anim keys buffer {size} bytes"));
                let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("anim keys"), size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
                if let Some(old) = &self.data_buf
                    && self.uploaded > 0
                {
                    encoder.copy_buffer_to_buffer(old, 0, &buf, 0, (self.uploaded * 4) as u64);
                }
                self.data_buf = Some(buf);
            }
            // As much of the tail as the frame's copy budget allows (render/upload.rs).
            if !self.data.is_empty() && (all || gpu.upload_room() > 0) {
                let n = if all { self.data.len() } else { (gpu.piece(self.data.len() as u64 * 4, 4) / 4) as usize };
                gpu.upload(n as u64 * 4, || gpu.queue.write_buffer(self.data_buf.as_ref().unwrap(), (self.uploaded * 4) as u64, bytemuck::cast_slice(&self.data[..n])));
                self.uploaded += n;
                self.data.drain(..n);
            }
        }
    }

    /// Uploads new keys (under the frame copy budget, or all of them with `all`) and this frame's jobs, then
    /// poses them into `pose` (the ZoneRenderer pose buffer).
    pub fn dispatch(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, pose: &wgpu::Buffer, all: bool) {
        self.upload_keys(gpu, encoder, all);
        if self.jobs.is_empty() {
            return;
        }
        let device = &gpu.device;
        let n = self.jobs.len() / JOB_WORDS;
        let jsize = (self.jobs.len() * 4) as u64;
        if self.job_buf.as_ref().is_none_or(|(_, c)| *c < jsize) {
            let c = jsize.next_power_of_two().max(4096);
            self.job_buf = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("anim jobs"), size: c, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), c));
        }
        let ssize = (n * SCRATCH * 16) as u64;
        if self.scratch.as_ref().is_none_or(|(_, c)| *c < ssize) {
            let c = ssize.next_power_of_two();
            self.scratch = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("anim scratch"), size: c, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false }), c));
        }
        let (jb, _) = self.job_buf.as_ref().unwrap();
        gpu.queue.write_buffer(jb, 0, bytemuck::cast_slice(&self.jobs));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pose actors"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.data_buf.as_ref().unwrap().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: jb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: pose.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.scratch.as_ref().unwrap().0.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pose actors"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(n as u32, 1, 1);
    }
}
