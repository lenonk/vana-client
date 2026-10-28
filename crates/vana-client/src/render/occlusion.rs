//! FUN_1006c280's occlusion test for actors, read back a frame or two late: an actor's screen rect at a quarter of
//! the way into its box's depth is tested against the scene depth on an 8 x 8 grid; no passing sample = hidden
//! (actor +0xb2 bit 0), so the target keys skip it.

use super::Gpu;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use super::Put;

const WGSL: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<storage, read> queries: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> out: array<u32>;

// Query: NDC rect (x0, y0, x1, y1), then (reference depth, count, 0, 0) in the first entry's slot 0.
@compute @workgroup_size(64)
fn occlusion(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = u32(queries[0].x);
    let i = gid.x;
    if (i >= n) { return; }
    let r = queries[1u + 2u * i];
    let z = queries[2u + 2u * i].x;
    let dims = vec2<f32>(textureDimensions(depth));
    // NDC to pixels (y down), clamped to the target; an empty rect is hidden.
    let p0 = clamp(vec2<f32>((r.x + 1.0) * 0.5, (1.0 - r.w) * 0.5) * dims, vec2<f32>(0.0), dims);
    let p1 = clamp(vec2<f32>((r.z + 1.0) * 0.5, (1.0 - r.y) * 0.5) * dims, vec2<f32>(0.0), dims);
    var hits = 0u;
    if (p1.x - p0.x >= 1.0 && p1.y - p0.y >= 1.0) {
        for (var y = 0u; y < 8u; y = y + 1u) {
            for (var x = 0u; x < 8u; x = x + 1u) {
                let p = p0 + (vec2<f32>(f32(x), f32(y)) + 0.5) / 8.0 * (p1 - p0);
                let d = textureLoad(depth, vec2<i32>(min(p, dims - 1.0)), 0);
                if (z <= d) { hits = hits + 1u; }
            }
        }
    }
    out[i] = hits;
}
"#;

struct Staging {
    buf: wgpu::Buffer,
    ids: Vec<u32>,
    /// 0 free, 1 copied this frame, 2 mapping.
    state: u8,
    ready: Arc<AtomicBool>,
}

pub struct Occlusion {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    queries: Vec<[f32; 4]>,
    ids: Vec<u32>,
    query_buf: Option<(wgpu::Buffer, u64)>,
    out_buf: Option<(wgpu::Buffer, u64)>,
    staging: Vec<Staging>,
    /// The last result per actor: visible or not.
    pub results: HashMap<u32, bool>,
}

const MAX: usize = 2048;

impl Occlusion {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("occlusion"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let buffer = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let depth = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("occlusion"), entries: &[depth, buffer(1, true), buffer(2, false)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("occlusion"), layout: Some(&pl), module: &module, entry_point: Some("occlusion"), compilation_options: Default::default(), cache: None });
        let staging = (0..3)
            .map(|_| Staging { buf: device.create_buffer(&wgpu::BufferDescriptor { label: Some("occlusion readback"), size: (MAX * 4) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), ids: Vec::new(), state: 0, ready: Arc::new(AtomicBool::new(false)) })
            .collect();
        Self { pipeline, layout, queries: Vec::new(), ids: Vec::new(), query_buf: None, out_buf: None, staging, results: HashMap::new() }
    }

    /// Tests actor `id`'s NDC rect at depth `z` (0 near, 1 far) this frame.
    pub fn query(&mut self, id: u32, rect: [f32; 4], z: f32) {
        if self.ids.len() < MAX {
            self.ids.push(id);
            self.queries.push(rect);
            self.queries.push([z, 0.0, 0.0, 0.0]);
        }
    }

    /// After the scene is drawn: the queries against its depth, into a readback copy.
    pub fn run(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, depth: &wgpu::TextureView) {
        let n = self.ids.len();
        let Some(s) = self.staging.iter_mut().find(|s| s.state == 0).filter(|_| n > 0) else {
            self.queries.clear();
            self.ids.clear();
            return;
        };
        let device = &gpu.device;
        let mut words = vec![[n as f32, 0.0, 0.0, 0.0]];
        words.append(&mut self.queries);
        let qsize = (words.len() * 16) as u64;
        if self.query_buf.as_ref().is_none_or(|(_, c)| *c < qsize) {
            let c = qsize.next_power_of_two().max(1024);
            self.query_buf = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("occlusion queries"), size: c, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), c));
        }
        let osize = (n * 4) as u64;
        if self.out_buf.as_ref().is_none_or(|(_, c)| *c < osize) {
            let c = osize.next_power_of_two().max(1024);
            self.out_buf = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("occlusion"), size: c, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }), c));
        }
        let (qb, ob) = (&self.query_buf.as_ref().unwrap().0, &self.out_buf.as_ref().unwrap().0);
        gpu.queue.put(qb, 0, bytemuck::cast_slice(&words));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("occlusion"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 1, resource: qb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: ob.as_entire_binding() },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("occlusion"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(ob, 0, &s.buf, 0, osize);
        s.ids = std::mem::take(&mut self.ids);
        s.state = 1;
    }

    /// After the frame's submit: maps this frame's copy and takes finished results.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        for s in &mut self.staging {
            if s.state == 1 {
                let ready = s.ready.clone();
                ready.store(false, Ordering::Release);
                s.buf.slice(..(s.ids.len() * 4) as u64).map_async(wgpu::MapMode::Read, move |r| {
                    if r.is_ok() {
                        ready.store(true, Ordering::Release);
                    }
                });
                s.state = 2;
            }
        }
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        for s in &mut self.staging {
            if s.state == 2 && s.ready.load(Ordering::Acquire) {
                if let Ok(data) = s.buf.slice(..(s.ids.len() * 4) as u64).get_mapped_range() {
                    let v: &[u32] = bytemuck::cast_slice(&data);
                    self.results.extend(s.ids.iter().zip(v).map(|(&id, &hits)| (id, hits > 0)));
                }
                s.buf.unmap();
                s.state = 0;
            }
        }
    }
}
