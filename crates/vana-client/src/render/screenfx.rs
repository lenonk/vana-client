//! Full-screen passes over the drawn scene, before the 2D layer: the event crossfade (scheduler op 0x10: a
//! captured frame drawn SRCALPHA / INVSRCALPHA, FUN_10012250) and the scene colour multiply of op 0x0f (the
//! scene quad drawn with a vertex colour under MODULATE2X, 0x10011a26: scene x 2 * RGB / 255).

use super::Gpu;

const SHADER: &str = r#"
struct U { color: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
// One triangle over the screen.
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}
// Blend src = DST, dst = SRC: dst * 2 * c, D3D's MODULATE2X with the colour c = RGB / 255.
@fragment fn multiply() -> @location(0) vec4<f32> {
    return vec4<f32>(u.color.rgb, 1.0);
}
@fragment fn image(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(tex, samp, i.uv).rgb, u.color.a);
}
"#;

pub struct ScreenFx {
    multiply: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: [wgpu::Buffer; 2],
    format: wgpu::TextureFormat,
    /// The captured frame and the bind groups for the two passes.
    capture: Option<(wgpu::Texture, wgpu::TextureView, [wgpu::BindGroup; 2])>,
}

impl ScreenFx {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("screen fx"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen fx"),
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("screen fx"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = |entry: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("screen fx"),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::COLOR })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let modulate2x = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add };
        let multiply = pipeline("multiply", wgpu::BlendState { color: modulate2x, alpha: wgpu::BlendComponent::REPLACE });
        let image = pipeline("image", wgpu::BlendState::ALPHA_BLENDING);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let ub = || device.create_buffer(&wgpu::BufferDescriptor { label: Some("screen fx"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { multiply, image, layout, sampler, uniforms: [ub(), ub()], format, capture: None }
    }

    /// The texture a frame is captured into, made for this size.
    pub fn capture_target(&mut self, gpu: &Gpu, size: [u32; 2]) -> &wgpu::TextureView {
        let fits = self.capture.as_ref().is_some_and(|(t, _, _)| t.width() == size[0] && t.height() == size[1]);
        if !fits {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("event crossfade"),
                size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let group = |u: &wgpu::Buffer| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("screen fx"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: u.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                })
            };
            let groups = [group(&self.uniforms[0]), group(&self.uniforms[1])];
            self.capture = Some((tex, view, groups));
        }
        &self.capture.as_ref().unwrap().1
    }

    /// Draws the captured frame at `cross` alpha, then multiplies the scene by `m` (1.0 = unchanged).
    pub fn draw(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, size: [u32; 2], cross: Option<f32>, m: [f32; 3]) {
        let multiply = m.iter().any(|c| (c - 1.0).abs() > 0.005);
        if cross.is_none() && !multiply {
            return;
        }
        self.capture_target(gpu, size);
        let groups = &self.capture.as_ref().unwrap().2;
        gpu.queue.write_buffer(&self.uniforms[0], 0, bytemuck::cast_slice(&[0.0f32, 0.0, 0.0, cross.unwrap_or(0.0)]));
        gpu.queue.write_buffer(&self.uniforms[1], 0, bytemuck::cast_slice(&[m[0] * 0.5, m[1] * 0.5, m[2] * 0.5, 1.0]));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen fx"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if cross.is_some() {
            pass.set_pipeline(&self.image);
            pass.set_bind_group(0, &groups[0], &[]);
            pass.draw(0..3, 0..1);
        }
        if multiply {
            pass.set_pipeline(&self.multiply);
            pass.set_bind_group(0, &groups[1], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MODULATE2X: a grey scene under the fdo1 / fdi1 midpoint (m = 0.5) comes out halved, not lifted to grey.
    #[test]
    fn multiply_pass() {
        let Ok(gpu) = Gpu::headless() else { return };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let mut fx = ScreenFx::new(&gpu, format);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let _clear = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.8, g: 0.4, b: 0.2, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        fx.draw(&gpu, &mut enc, &view, [4, 4], None, [0.5; 3]);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 256 * 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(4) } },
            wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        let px = [d[0] as i32, d[1] as i32, d[2] as i32];
        let want = [(0.8f32 * 255.0 * 0.5).round() as i32, (0.4f32 * 255.0 * 0.5).round() as i32, (0.2f32 * 255.0 * 0.5).round() as i32];
        for i in 0..3 {
            assert!((px[i] - want[i]).abs() <= 2, "{px:?} vs {want:?}");
        }
    }
}
