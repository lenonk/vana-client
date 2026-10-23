//! Posed attach points of actors, read back a frame or two late (FUN_100d25b0: the skeleton point minus the actor
//! root, in world axes; footsteps test their height). A compute pass reads the frame's pose buffer.

use super::Gpu;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> pose: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> jobs: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;

fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// Job: (pose region in vec4s, bone, count, 0), then the point's offset from its bone.
@compute @workgroup_size(64)
fn points(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= jobs[0].z) { return; }
    let j = jobs[1u + 2u * i];
    let off = bitcast<vec3<f32>>(jobs[2u + 2u * i].xyz);
    let base = j.x;
    let q = pose[base + 6u + 2u * j.y];
    let t = pose[base + 7u + 2u * j.y].xyz;
    let m = mat4x4<f32>(pose[base], pose[base + 1u], pose[base + 2u], pose[base + 3u]);
    let w = m * vec4<f32>(t + qrot(q, off), 1.0);
    out[i] = vec4<f32>(w.xyz - m[3].xyz, 0.0);
}
"#;

struct Staging {
    buf: wgpu::Buffer,
    keys: Vec<u64>,
    /// 0 free, 1 copied this frame, 2 mapping.
    state: u8,
    ready: Arc<AtomicBool>,
}

pub struct Points {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    jobs: Vec<[u32; 4]>,
    keys: Vec<u64>,
    job_buf: Option<(wgpu::Buffer, u64)>,
    out_buf: Option<(wgpu::Buffer, u64)>,
    staging: Vec<Staging>,
    done: Vec<(u64, glam::Vec3)>,
}

const MAX: usize = 4096;

impl Points {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("actor points"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("actor points"), entries: &[entry(0, true), entry(1, true), entry(2, false)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("actor points"), layout: Some(&pl), module: &module, entry_point: Some("points"), compilation_options: Default::default(), cache: None });
        let staging = (0..3)
            .map(|_| Staging { buf: device.create_buffer(&wgpu::BufferDescriptor { label: Some("points readback"), size: (MAX * 16) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), keys: Vec::new(), state: 0, ready: Arc::new(AtomicBool::new(false)) })
            .collect();
        Self { pipeline, layout, jobs: Vec::new(), keys: Vec::new(), job_buf: None, out_buf: None, staging, done: Vec::new() }
    }

    /// Asks for a point of the actor whose pose region starts at vec4 `region`: bone, offset, and a key to find
    /// the answer by.
    pub fn query(&mut self, key: u64, region: u32, bone: u16, off: glam::Vec3) {
        if self.keys.len() < MAX {
            self.keys.push(key);
            self.jobs.push([region, bone as u32, 0, 0]);
            self.jobs.push([off.x.to_bits(), off.y.to_bits(), off.z.to_bits(), 0]);
        }
    }

    /// After the frame's posing: the queries' points into a readback copy.
    pub fn run(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, pose: &wgpu::Buffer) {
        let n = self.keys.len();
        let Some(s) = self.staging.iter_mut().find(|s| s.state == 0).filter(|_| n > 0) else {
            self.jobs.clear();
            self.keys.clear();
            return;
        };
        let device = &gpu.device;
        let mut words = vec![[0, 0, n as u32, 0]];
        words.append(&mut self.jobs);
        let jsize = (words.len() * 16) as u64;
        if self.job_buf.as_ref().is_none_or(|(_, c)| *c < jsize) {
            let c = jsize.next_power_of_two().max(1024);
            self.job_buf = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("point jobs"), size: c, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), c));
        }
        let osize = (n * 16) as u64;
        if self.out_buf.as_ref().is_none_or(|(_, c)| *c < osize) {
            let c = osize.next_power_of_two().max(1024);
            self.out_buf = Some((device.create_buffer(&wgpu::BufferDescriptor { label: Some("points"), size: c, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }), c));
        }
        let (jb, ob) = (&self.job_buf.as_ref().unwrap().0, &self.out_buf.as_ref().unwrap().0);
        gpu.queue.write_buffer(jb, 0, bytemuck::cast_slice(&words));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actor points"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pose.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: jb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: ob.as_entire_binding() },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("actor points"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(ob, 0, &s.buf, 0, osize);
        s.keys = std::mem::take(&mut self.keys);
        s.state = 1;
    }

    /// After the frame's submit: maps this frame's copy and collects finished ones.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        for s in &mut self.staging {
            if s.state == 1 {
                let ready = s.ready.clone();
                ready.store(false, Ordering::Release);
                s.buf.slice(..(s.keys.len() * 16) as u64).map_async(wgpu::MapMode::Read, move |r| {
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
                if let Ok(data) = s.buf.slice(..(s.keys.len() * 16) as u64).get_mapped_range() {
                    let v: &[[f32; 4]] = bytemuck::cast_slice(&data);
                    self.done.extend(s.keys.iter().zip(v).map(|(&k, p)| (k, glam::Vec3::new(p[0], p[1], p[2]))));
                }
                s.buf.unmap();
                s.state = 0;
            }
        }
    }

    /// Points that came back since the last call, by key.
    pub fn take(&mut self) -> Vec<(u64, glam::Vec3)> {
        std::mem::take(&mut self.done)
    }
}
