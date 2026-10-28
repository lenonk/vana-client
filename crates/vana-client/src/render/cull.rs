//! Frustum culling of scenery on the GPU (ours: retail walks the MZB's tree on the CPU, FUN_1017c8b0). One compute
//! pass marks each instance visible (retail's draw distance and its LOD band to the origin, its mesh's box against the
//! view frustum with the far plane at the draw distance), a second copies each run's visible instances, in order,
//! into a second instance buffer and writes its draws' indirect arguments; the scenery then draws from those.

use super::{Gpu, INSTANCE_BYTES, Instance, Put};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

/// A block of instances drawn together (a mesh's normal or mirrored placements, or both): where its visible copies
/// go, and its draws' arguments (`args_start..+args_count` in the run-args list).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Run {
    inst_start: u32,
    inst_end: u32,
    out_start: u32,
    args_start: u32,
    args_count: u32,
    pad: [u32; 3],
}

/// A draw's fixed arguments: index count, first index, base vertex.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ArgBase {
    index_count: u32,
    first_index: u32,
    base_vertex: i32,
    pad: u32,
}

/// A run as the CPU tests it before submitting its draws: the world box around its placements' geometry, the box
/// around their origins (the LOD band is measured to the origin), the union of their bands (None: one draws at any
/// distance), whether any keeps the zone's draw distance and the largest own one squared, and whether it moves (door
/// parts, a voyage's world), which is never skipped.
#[derive(Clone, Copy)]
struct RunBox {
    lo: Vec3,
    hi: Vec3,
    org_lo: Vec3,
    org_hi: Vec3,
    band: Option<(f32, f32)>,
    plain: bool,
    own: f32,
    moves: bool,
}

/// A placement's distance rule (FUN_1017c9c0, doors FUN_10183da0) as cs_cull_vis takes it: its own distance squared,
/// and 0 = within the zone's draw distance D, 1 = within its own distance times K, 2 (a door group's) = within both.
pub fn reach(own: Option<f32>, door: bool) -> [f32; 2] {
    match own {
        Some(k) => [k * k, if door { 2.0 } else { 1.0 }],
        None => [0.0, 0.0],
    }
}

/// What the culling needs of a geometry, built with it (ZoneGeometry::build_keyed).
#[derive(Default)]
pub struct CullData {
    /// Per instance: its mesh's box in model space, centre then half extents; the fourth words its reach().
    bounds: Vec<[f32; 8]>,
    runs: Vec<Run>,
    boxes: Vec<RunBox>,
    run_args: Vec<u32>,
    args: Vec<ArgBase>,
    /// Each argument slot's run.
    arg_run: Vec<u32>,
    /// Instances the visible copies can take (the runs' lengths summed).
    out_len: u32,
}

impl CullData {
    /// `bounds`: each instance's box; `draws`: each draw's sub-draws (instance range, index count, first index, base
    /// vertex). Returns each sub-draw's argument slot.
    pub fn build(bounds: Vec<[f32; 8]>, instances: &[Instance], draws: &[Vec<(std::ops::Range<u32>, u32, u32, i32)>]) -> (Self, Vec<Vec<u32>>) {
        let mut runs: Vec<(std::ops::Range<u32>, Vec<u32>)> = Vec::new();
        let mut by_range: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
        let mut args = Vec::new();
        let mut slots = Vec::with_capacity(draws.len());
        for subs in draws {
            let mut s = Vec::with_capacity(subs.len());
            for (range, index_count, first_index, base_vertex) in subs {
                let run = *by_range.entry((range.start, range.end)).or_insert_with(|| {
                    runs.push((range.clone(), Vec::new()));
                    runs.len() - 1
                });
                let slot = args.len() as u32;
                args.push(ArgBase { index_count: *index_count, first_index: *first_index, base_vertex: *base_vertex, pad: 0 });
                runs[run].1.push(slot);
                s.push(slot);
            }
            slots.push(s);
        }
        let (mut out, mut run_args, mut gpu_runs, mut boxes) = (0u32, Vec::new(), Vec::with_capacity(runs.len()), Vec::with_capacity(runs.len()));
        let mut arg_run = vec![0u32; args.len()];
        for (r, (range, a)) in runs.into_iter().enumerate() {
            gpu_runs.push(Run { inst_start: range.start, inst_end: range.end, out_start: out, args_start: run_args.len() as u32, args_count: a.len() as u32, pad: [0; 3] });
            boxes.push(run_box(&bounds, instances, range.clone()));
            out += range.end - range.start;
            for &slot in &a {
                arg_run[slot as usize] = r as u32;
            }
            run_args.extend(a);
        }
        (Self { bounds, runs: gpu_runs, boxes, run_args, args, arg_run, out_len: out }, slots)
    }
}

/// A run's boxes from its instances (each mesh box through its placement matrix).
fn run_box(bounds: &[[f32; 8]], instances: &[Instance], range: std::ops::Range<u32>) -> RunBox {
    let mut b = RunBox { lo: Vec3::splat(f32::MAX), hi: Vec3::splat(f32::MIN), org_lo: Vec3::splat(f32::MAX), org_hi: Vec3::splat(f32::MIN), band: Some((f32::MAX, f32::MIN)), plain: false, own: 0.0, moves: false };
    for i in range {
        let (Some(inst), Some(bd)) = (instances.get(i as usize), bounds.get(i as usize)) else { continue };
        b.moves |= inst.band[2] != 0.0 || inst.band[3] != 0.0;
        if bd[7] == 0.0 {
            b.plain = true;
        } else {
            b.own = b.own.max(bd[3]);
        }
        let m = Mat4::from_cols_array_2d(&inst.model);
        let centre = m.transform_point3(Vec3::new(bd[0], bd[1], bd[2]));
        let reach = m.x_axis.truncate().abs() * bd[4] + m.y_axis.truncate().abs() * bd[5] + m.z_axis.truncate().abs() * bd[6];
        (b.lo, b.hi) = (b.lo.min(centre - reach), b.hi.max(centre + reach));
        let origin = m.w_axis.truncate();
        (b.org_lo, b.org_hi) = (b.org_lo.min(origin), b.org_hi.max(origin));
        b.band = match b.band {
            Some((near, far)) if inst.band[1] > 0.0 => Some((near.min(inst.band[0]), far.max(inst.band[1]))),
            _ => None,
        };
    }
    b
}

/// The six planes of the view frustum (clip x, y in -w..w, z in 0..w) from the view-projection matrix.
fn planes(view_proj: Mat4) -> [Vec4; 6] {
    let r = [view_proj.row(0), view_proj.row(1), view_proj.row(2), view_proj.row(3)];
    [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]]
}

impl RunBox {
    /// Could any of its placements be drawn: its geometry box (a little larger, so the GPU's own test decides at the
    /// edges) inside the frustum, and its origins within reach of the draw distance (`dk`: D, K; D 0 = none) and of
    /// its bands.
    fn maybe_visible(&self, planes: &[Vec4; 6], eye: Vec3, dk: [f32; 2]) -> bool {
        if self.moves {
            return true;
        }
        let (c, e) = ((self.lo + self.hi) / 2.0, (self.hi - self.lo) / 2.0 * 1.02 + Vec3::splat(0.5));
        if planes.iter().any(|p| p.truncate().dot(c) + p.w < -(p.truncate().abs().dot(e))) {
            return false;
        }
        let nearest = eye.clamp(self.org_lo, self.org_hi).distance_squared(eye);
        if dk[0] > 0.0 {
            let limit = (if self.plain { dk[0] * dk[0] } else { 0.0 }).max(dk[1] * self.own);
            if nearest * 0.98 - 1.0 > limit {
                return false;
            }
        }
        let Some((near, far)) = self.band else { return true };
        let farthest = (eye - self.org_lo).abs().max((eye - self.org_hi).abs()).length_squared();
        farthest * 1.02 + 1.0 > near && nearest * 0.98 - 1.0 <= far
    }
}

/// The culling of one geometry on the GPU: its buffers and the bind group over them and the source instances.
pub struct Cull {
    group: wgpu::BindGroup,
    instances: u32,
    runs: u32,
    boxes: Vec<RunBox>,
    arg_run: Vec<u32>,
    /// The visible instances, run by run (the scenery's instance vertex buffer), and the draws' indirect arguments.
    pub out: wgpu::Buffer,
    pub args: wgpu::Buffer,
}

impl Cull {
    /// None without indirect draws that start past instance 0, with nothing to draw, or with FFXI_NO_CULL set (a
    /// check against the unculled scene).
    pub fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, data: &CullData, instances: &wgpu::Buffer) -> Option<Self> {
        if !gpu.device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE) || data.runs.is_empty() || data.bounds.is_empty() || std::env::var_os("FFXI_NO_CULL").is_some() {
            return None;
        }
        let device = &gpu.device;
        let with = |label, bytes: &[u8], usage: wgpu::BufferUsages| {
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (bytes.len() as u64).max(16), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.put(&b, 0, bytes);
            b
        };
        let empty = |label, size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false });
        let storage = wgpu::BufferUsages::STORAGE;
        let runs = with("cull runs", bytemuck::cast_slice(&data.runs), storage);
        let bounds = with("cull bounds", bytemuck::cast_slice(&data.bounds), storage);
        let run_args = with("cull run args", bytemuck::cast_slice(&data.run_args), storage);
        let arg_base = with("cull arg base", bytemuck::cast_slice(&data.args), storage);
        let out = empty("culled instances", data.out_len as u64 * INSTANCE_BYTES, storage | wgpu::BufferUsages::VERTEX);
        let args = empty("cull args", data.args.len() as u64 * 20, storage | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC);
        let vis = empty("cull visibility", data.bounds.len() as u64 * 4, storage);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cull"),
            layout,
            entries: &[&runs, &bounds, instances, &out, &run_args, &arg_base, &args, &vis]
                .iter()
                .enumerate()
                .map(|(k, b)| wgpu::BindGroupEntry { binding: k as u32, resource: b.as_entire_binding() })
                .collect::<Vec<_>>(),
        });
        log::info!("scenery culled on the GPU: {} instances in {} runs, {} draws", data.bounds.len(), data.runs.len(), data.args.len());
        Some(Self { group, instances: data.bounds.len() as u32, runs: data.runs.len() as u32, boxes: data.boxes.clone(), arg_run: data.arg_run.clone(), out, args })
    }

    /// The last culling's draws with an instance, of all, and the instances drawn (a blocking readback, for the
    /// offscreen benchmark).
    pub fn stats(&self, gpu: &Gpu) -> (u32, u32, u32) {
        let size = self.args.size();
        let read = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut e = gpu.device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&self.args, 0, &read, 0, size);
        gpu.queue.submit([e.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let Ok(v) = read.slice(..).get_mapped_range() else { return (0, 0, 0) };
        let words: &[u32] = bytemuck::cast_slice(&v);
        let counts: Vec<u32> = words.chunks(5).map(|a| a[1]).collect();
        (counts.iter().filter(|&&c| c > 0).count() as u32, counts.len() as u32, counts.iter().sum())
    }

    /// Per run, whether its draws go in this frame (RunBox::maybe_visible); the GPU counts what they draw.
    pub fn runs_shown(&self, view_proj: Mat4, eye: Vec3, dk: [f32; 2]) -> Vec<bool> {
        let p = planes(view_proj);
        self.boxes.iter().map(|b| b.maybe_visible(&p, eye, dk)).collect()
    }

    /// The run of an argument slot.
    pub fn run_of(&self, slot: u32) -> usize {
        self.arg_run[slot as usize] as usize
    }

    /// Both passes, with the scenery's globals (camera, voyage, door parts) bound at group 0.
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, globals: &wgpu::BindGroup, pipes: &[wgpu::ComputePipeline; 2]) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("cull"), timestamp_writes: None });
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(3, &self.group, &[]);
        pass.set_pipeline(&pipes[0]);
        pass.dispatch_workgroups(self.instances.div_ceil(64), 1, 1);
        pass.set_pipeline(&pipes[1]);
        pass.dispatch_workgroups(self.runs, 1, 1);
    }
}

/// The bind group layout of group 3: runs, bounds, source instances, culled instances, run args, arg bases, args,
/// visibility.
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cull"),
        entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, false), entry(4, true), entry(5, true), entry(6, false), entry(7, false)],
    })
}
