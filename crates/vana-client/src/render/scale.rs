//! Background resolution: the 3D scene drawn offscreen at its own size, then scaled to the window in one pass
//! (Config-tool values 0003 / 0004; the UI and text stay at window size). Downscaling averages the destination
//! pixel's footprint, upscaling is Catmull-Rom.

use super::Gpu;
use super::Put;

const SHADER: &str = r#"
struct U { src: vec2<f32>, dst: vec2<f32>, gamma: vec4<f32>, lcd: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}
fn tap(uv: vec2<f32>) -> vec3<f32> { return textureSampleLevel(tex, samp, uv, 0.0).rgb; }
// Box over the footprint: n x n bilinear taps per axis, n = ceil(src / dst).
fn down(uv: vec2<f32>) -> vec3<f32> {
    let n = clamp(ceil(u.src / u.dst), vec2<f32>(1.0), vec2<f32>(8.0));
    var sum = vec3<f32>(0.0);
    for (var j = 0.0; j < n.y; j += 1.0) {
        for (var i = 0.0; i < n.x; i += 1.0) {
            sum += tap(uv + ((vec2<f32>(i, j) + 0.5) / n - 0.5) / u.dst);
        }
    }
    return sum / (n.x * n.y);
}
// Catmull-Rom in 9 bilinear taps.
fn up(uv: vec2<f32>) -> vec3<f32> {
    let p = uv * u.src;
    let t1 = floor(p - 0.5) + 0.5;
    let f = p - t1;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (t1 - 1.0) / u.src;
    let t3 = (t1 + 2.0) / u.src;
    let t12 = (t1 + w2 / w12) / u.src;
    var c = tap(vec2<f32>(t0.x, t0.y)) * w0.x * w0.y + tap(vec2<f32>(t12.x, t0.y)) * w12.x * w0.y + tap(vec2<f32>(t3.x, t0.y)) * w3.x * w0.y;
    c += tap(vec2<f32>(t0.x, t12.y)) * w0.x * w12.y + tap(vec2<f32>(t12.x, t12.y)) * w12.x * w12.y + tap(vec2<f32>(t3.x, t12.y)) * w3.x * w12.y;
    c += tap(vec2<f32>(t0.x, t3.y)) * w0.x * w3.y + tap(vec2<f32>(t12.x, t3.y)) * w12.x * w3.y + tap(vec2<f32>(t3.x, t3.y)) * w3.x * w3.y;
    return max(c, vec3<f32>(0.0));
}
// Gamma: c^(1 / g) per channel (FUN_10016400's ramp exponent).
// 3D LCD Mode (lcd.x = 1, config 0030): the scene was drawn 0.6 % wider (lcd.z = its extra share, 0.006); eye 0
// (projection shifted -0.006 in NDC x) squeezed into the left half, eye 1 (+0.006) into the right, bilinear
// (FUN_1000b4f0). On the frames lcd.y marks, the bottom 10 rows carry FUN_1000afd0's strip: the 1 x 16 texture's
// black row 0, then (0x48, 0, 0).
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    if (u.lcd.x > 0.5) {
        let row = i.clip.y - (u.dst.y - 10.0);
        if (u.lcd.y > 0.5 && row >= 0.0) {
            if (floor(row / 10.0 * 16.0) < 1.0) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
            return vec4<f32>(72.0 / 255.0, 0.0, 0.0, 1.0);
        }
        let left = i.uv.x < 0.5;
        let x = select(i.uv.x - 0.5, i.uv.x, left) * 2.0;
        let ux = select(x, x + u.lcd.z, left) / (1.0 + u.lcd.z);
        let e = tap(vec2<f32>(ux, i.uv.y));
        return vec4<f32>(pow(clamp(e, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0 / u.gamma.rgb), 1.0);
    }
    var c: vec3<f32>;
    if (u.src.x > u.dst.x || u.src.y > u.dst.y) {
        c = down(i.uv);
    } else {
        c = up(i.uv);
    }
    return vec4<f32>(pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0 / u.gamma.rgb), 1.0);
}
"#;

struct Target {
    size: [u32; 2],
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    group: wgpu::BindGroup,
}

pub struct SceneScale {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    format: wgpu::TextureFormat,
    target: Option<Target>,
}

impl SceneScale {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("scene scale"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene scale"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("scene scale"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene scale"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("scene scale"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { pipeline, layout, sampler, uniform, format, target: None }
    }

    /// The offscreen colour and depth views at `size`, made on first use or a size change, and the colour texture.
    pub fn target(&mut self, gpu: &Gpu, size: [u32; 2]) -> (wgpu::TextureView, wgpu::TextureView, wgpu::Texture) {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("background"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                // The world draws through a plain view (gamma space, blended as D3D8); the blit reads the sRGB one.
                view_formats: &[self.format.remove_srgb_suffix()],
            });
            let view = tex.create_view(&Default::default());
            let depth = super::depth_view(&gpu.device, size[0], size[1]);
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene scale"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.target = Some(Target { size, tex, view, depth, group });
        }
        let t = self.target.as_ref().unwrap();
        (t.view.clone(), t.depth.clone(), t.tex.clone())
    }

    /// The scene texture through a plain (not sRGB) view: written and blended in gamma space.
    pub fn raw_view(&self) -> Option<wgpu::TextureView> {
        let t = self.target.as_ref()?;
        Some(t.tex.create_view(&wgpu::TextureViewDescriptor { format: Some(self.format.remove_srgb_suffix()), ..Default::default() }))
    }

    /// Scales the offscreen scene onto `dst` (window size) with a gamma per channel, replacing what is there. `lcd`:
    /// 3D LCD Mode's side-by-side eyes (the scene `extra` wider) and whether this frame carries the strip.
    pub fn blit(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, dst: &wgpu::TextureView, dst_size: [u32; 2], gamma: [f32; 3], lcd: Option<(f32, bool)>) {
        let Some(t) = &self.target else { return };
        let (on, strip, extra) = lcd.map_or((0.0, 0.0, 0.0), |(e, s)| (1.0, s as u32 as f32, e));
        let u = [t.size[0] as f32, t.size[1] as f32, dst_size[0].max(1) as f32, dst_size[1].max(1) as f32, gamma[0], gamma[1], gamma[2], 1.0, on, strip, extra, 0.0];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene scale"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &t.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        (0..h).flat_map(|y| d[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
    }

    /// A flat colour survives both directions; a 2x downscale of a checker averages to grey.
    #[test]
    fn scales_both_ways() {
        let Ok(gpu) = Gpu::headless() else { return };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut s = SceneScale::new(&gpu, format);
        for (src, dst) in [([8, 8], [4, 4]), ([4, 4], [8, 8])] {
            let (view, _, _) = s.target(&gpu, src);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let _clear = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.6, g: 0.4, b: 0.2, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            let out = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: dst[0], height: dst[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            s.blit(&gpu, &mut enc, &out.create_view(&Default::default()), dst, [1.0; 3], None);
            gpu.queue.submit([enc.finish()]);
            let px = read(&gpu, &out, dst[0], dst[1]);
            for p in px.chunks_exact(4) {
                assert!((p[0] as i32 - 153).abs() <= 2 && (p[1] as i32 - 102).abs() <= 2 && (p[2] as i32 - 51).abs() <= 2, "{p:?}");
            }
        }
        // Checker 8x8 -> 4x4: each output pixel covers two of each.
        let (view, _, _) = s.target(&gpu, [8, 8]);
        let tex_rgba: Vec<u8> = (0..64).flat_map(|i| if (i % 8 + i / 8) % 2 == 0 { [255u8, 255, 255, 255] } else { [0, 0, 0, 255] }).collect();
        let _ = view;
        let tex = &s.target.as_ref().unwrap().tex;
        gpu.queue.write_texture(tex.as_image_copy(), &tex_rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(32), rows_per_image: Some(8) }, wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 });
        let out = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        s.blit(&gpu, &mut enc, &out.create_view(&Default::default()), [4, 4], [1.0; 3], None);
        gpu.queue.submit([enc.finish()]);
        for p in read(&gpu, &out, 4, 4).chunks_exact(4) {
            assert!((p[0] as i32 - 128).abs() <= 3, "{p:?}");
        }
    }
}
