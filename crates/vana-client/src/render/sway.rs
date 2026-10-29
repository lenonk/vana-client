//! Config-tool 0011 (Environment Animation): animated meshes (MMB type 2) sway along their per-vertex delta (FUN_10168eb0,
//! FUN_101734f0; research stored_settings_spec.md section 2). Each animated instance runs a 300-tick timer from a random
//! start with a random curve of the current weather; every frame (on the GPU here) the timer advances by the frame's
//! ticks, a wrap picks a new curve, and k = floor((N - 1) s) / (N - 1) with s = max(0, 1 - curve(timer / 300)) goes into
//! the instance record's area.y, which vs_main scales the delta by (retail binds position buffer trunc((N - 1) s)).

use super::{Gpu, Put};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    ticks: f32,
    steps: f32,
    count: u32,
    frame: u32,
    tag: u32,
    pad: [u32; 3],
}

const WGSL: &str = r#"
struct Params {
    ticks: f32,
    steps: f32,
    count: u32,
    frame: u32,
    tag: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};
struct Inst {
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
    m3: vec4<f32>,
    lights: vec4<u32>,
    area: vec4<u32>,
    band: vec4<f32>,
};
@group(0) @binding(0) var<uniform> pr: Params;
@group(0) @binding(1) var<storage, read> list: array<u32>;
@group(0) @binding(2) var<storage, read_write> state: array<vec2<u32>>;
@group(0) @binding(3) var<storage, read> heads: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read> pairs: array<vec2<f32>>;
@group(0) @binding(5) var<storage, read> tags: array<vec2<u32>>;
@group(0) @binding(6) var<storage, read_write> insts: array<Inst>;

fn hash(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

// FUN_101606a0: one of the current weather's curves at random (none: no sway).
fn pick(i: u32) -> u32 {
    let t = tags[pr.tag];
    if (t.y == 0u) { return 0xffffffffu; }
    return t.x + hash(i * 7919u + pr.frame * 104729u) % t.y;
}

// FUN_10053550: piecewise linear over the (t, value) pairs, t clamped to [0, 1].
fn curve(c: u32, t: f32) -> f32 {
    let h = heads[c];
    if (h.y == 0u) { return 0.0; }
    let x = clamp(t, 0.0, 1.0);
    var prev = pairs[h.x];
    if (x <= prev.x) { return prev.y; }
    for (var k = 1u; k < h.y; k = k + 1u) {
        let p = pairs[h.x + k];
        if (x <= p.x) {
            let d = p.x - prev.x;
            return select(p.y, mix(prev.y, p.y, (x - prev.x) / d), d > 1e-6);
        }
        prev = p;
    }
    return prev.y;
}

@compute @workgroup_size(64)
fn sway(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= pr.count) { return; }
    let st = state[i];
    var timer = bitcast<f32>(st.x);
    var c = st.y;
    if (c == 0xffffffffu) { c = pick(i); }
    timer = timer + pr.ticks;
    if (timer >= 300.0) {
        timer = timer - 300.0 * floor(timer / 300.0);
        c = pick(i);
    }
    var s = 0.0;
    if (c != 0xffffffffu) { s = max(1.0 - curve(c, timer / 300.0), 0.0); }
    var k = 0.0;
    if (pr.steps > 1.0) { k = floor((pr.steps - 1.0) * s) / (pr.steps - 1.0); }
    state[i] = vec2<u32>(bitcast<u32>(timer), c);
    insts[list[i]].area.y = bitcast<u32>(k);
}
"#;

/// The sway of one geometry's animated instances.
pub struct Sway {
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    count: u32,
    frame: std::cell::Cell<u32>,
    /// This frame's ticks, steps (1 / 16 / 32) and weather tag index.
    pub next: (f32, u32, u32),
}

impl Sway {
    /// `insts`: the animated instances; `curves` the zone's curves per weather tag (dat::zone::SWAY_TAGS order);
    /// `instances` the geometry's instance buffer (STORAGE).
    pub fn new(gpu: &Gpu, insts: &[u32], curves: &crate::dat::zone::SwayCurves, instances: &wgpu::Buffer) -> Option<Self> {
        if insts.is_empty() {
            return None;
        }
        let device = &gpu.device;
        let (mut heads, mut pairs, mut tags): (Vec<[u32; 2]>, Vec<[f32; 2]>, Vec<[u32; 2]>) = (Vec::new(), Vec::new(), Vec::new());
        for list in &curves.by_tag {
            tags.push([heads.len() as u32, list.len() as u32]);
            for c in list {
                heads.push([pairs.len() as u32, c.len() as u32]);
                pairs.extend_from_slice(c);
            }
        }
        tags.resize(crate::dat::zone::SWAY_TAGS.len().max(1), [0, 0]);
        heads.push([0, 0]);
        pairs.push([0.0, 0.0]);
        // FUN_101734f0's first call: the timer at rand() % 300, the curve picked on the GPU's first frame.
        let mut seed = 0x9e37_79b9u32;
        let state: Vec<[u32; 2]> = insts
            .iter()
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                [((seed % 300) as f32).to_bits(), u32::MAX]
            })
            .collect();
        let storage = |label, bytes: &[u8]| {
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (bytes.len() as u64).max(16), usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.put(&b, 0, bytes);
            b
        };
        let list = storage("sway instances", bytemuck::cast_slice(insts));
        let state = storage("sway state", bytemuck::cast_slice(&state));
        let heads = storage("sway curve heads", bytemuck::cast_slice(&heads));
        let pairs = storage("sway curve pairs", bytemuck::cast_slice(&pairs));
        let tags = storage("sway weather tags", bytemuck::cast_slice(&tags));
        let params = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sway params"), size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let st = |read_only| wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sway"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, st(true)),
                entry(2, st(false)),
                entry(3, st(true)),
                entry(4, st(true)),
                entry(5, st(true)),
                entry(6, st(false)),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sway"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sway"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sway"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 })),
            module: &module,
            entry_point: Some("sway"),
            compilation_options: Default::default(),
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sway"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: list.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: state.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: heads.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: pairs.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: tags.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: instances.as_entire_binding() },
            ],
        });
        Some(Sway { pipeline, group, params, count: insts.len() as u32, frame: Default::default(), next: (0.0, 1, 1) })
    }

    /// This frame's step, before the culling copies the instances.
    pub fn run(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        let (ticks, steps, tag) = self.next;
        self.frame.set(self.frame.get().wrapping_add(1));
        gpu.queue.put(&self.params, 0, bytemuck::bytes_of(&Params { ticks, steps: steps as f32, count: self.count, frame: self.frame.get(), tag, pad: [0; 3] }));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sway"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
    }
}
