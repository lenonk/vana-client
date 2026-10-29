//! The opening movie's picture (config 0022): the decoder's Y, Cb and Cr planes as three R8 textures, turned to RGB
//! in the shader and drawn over a rectangle of the window on black (FUN_10213810's quad).

use super::Gpu;
use super::Put;

const SHADER: &str = r#"
// rect: the picture's corners in NDC (x0, y0 top, x1, y1 bottom); mode.x = 1 writes to an sRGB target.
struct U { rect: vec4<f32>, mode: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tcb: texture_2d<f32>;
@group(0) @binding(3) var tcr: texture_2d<f32>;
@group(0) @binding(4) var samp: sampler;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var o: VOut;
    o.clip = vec4<f32>(mix(u.rect.x, u.rect.z, p.x), mix(u.rect.y, u.rect.w, p.y), 0.0, 1.0);
    o.uv = p;
    return o;
}
fn linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
// ITU-R BT.601, studio range.
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let y = textureSampleLevel(ty, samp, i.uv, 0.0).r * 255.0 - 16.0;
    let cb = textureSampleLevel(tcb, samp, i.uv, 0.0).r * 255.0 - 128.0;
    let cr = textureSampleLevel(tcr, samp, i.uv, 0.0).r * 255.0 - 128.0;
    var c = clamp(vec3<f32>(1.164 * y + 1.596 * cr, 1.164 * y - 0.813 * cr - 0.391 * cb, 1.164 * y + 2.018 * cb) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    if (u.mode.x > 0.5) {
        c = linear(c);
    }
    return vec4<f32>(c, 1.0);
}
"#;

struct Planes {
    size: [u32; 2],
    tex: [wgpu::Texture; 3],
    group: wgpu::BindGroup,
}

pub struct Movie {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    srgb: bool,
    planes: Option<Planes>,
}

impl Movie {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("movie"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("movie"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1),
                tex(2),
                tex(3),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("movie"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("movie"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("movie"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { pipeline, layout, sampler, uniform, srgb: format.is_srgb(), planes: None }
    }

    /// A new picture: luma `width` x `height`, chroma half that each way.
    pub fn upload(&mut self, gpu: &Gpu, f: &crate::mpeg1::Frame) {
        let size = [f.width as u32, f.height as u32];
        if self.planes.as_ref().is_none_or(|p| p.size != size) {
            let plane = |w: u32, h: u32| {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("movie plane"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            let tex = [plane(size[0], size[1]), plane(size[0] / 2, size[1] / 2), plane(size[0] / 2, size[1] / 2)];
            let views = tex.each_ref().map(|t| t.create_view(&Default::default()));
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("movie"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.planes = Some(Planes { size, tex, group });
        }
        let p = self.planes.as_ref().unwrap();
        for (t, (data, w)) in p.tex.iter().zip([(&f.y, size[0]), (&f.cb, size[0] / 2), (&f.cr, size[0] / 2)]) {
            let h = data.len() as u32 / w;
            super::WRITTEN.fetch_add(data.len() as u64, std::sync::atomic::Ordering::Relaxed);
            gpu.queue.write_texture(t.as_image_copy(), data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        }
    }

    /// Clears `dst` to black and, with `picture`, draws the last one over `rect` (window pixels: x0, y0, x1, y1).
    pub fn draw(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, dst: &wgpu::TextureView, dst_size: [u32; 2], rect: [f32; 4], picture: bool) {
        let (w, h) = (dst_size[0].max(1) as f32, dst_size[1].max(1) as f32);
        let u = [rect[0] / w * 2.0 - 1.0, 1.0 - rect[1] / h * 2.0, rect[2] / w * 2.0 - 1.0, 1.0 - rect[3] / h * 2.0, self.srgb as u32 as f32, 0.0, 0.0, 0.0];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("movie"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(p) = self.planes.as_ref().filter(|_| picture) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &p.group, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}
