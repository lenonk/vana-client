//! Ambient occlusion (ours, not retail; Settings > Effects > Ambient Occlusion, config `ssao`): screen-space AO from
//! the scene's depth, worked out after the opaque and cut-out scenery and before anything else draws (as the real
//! shadows, render/sunshadow.rs), so it darkens creases and corners of the world but never the characters' painted
//! light. Two passes: the occlusion into a half-size R8 target (12 samples in the hemisphere about the normal rebuilt
//! from depth, turned per pixel), then a 4 x 4 blur of it, weighted by linear depth, that multiplies the frame.

use super::Gpu;
use glam::{Mat4, Vec3};

/// The sampling radius in world units, how much the darkest corner loses, and the eye distance over which it fades
/// out (far scenery is fogged and its samples too coarse).
pub const RADIUS: f32 = 1.5;
pub const STRENGTH: f32 = 0.55;
pub const FADE: [f32; 2] = [40.0, 80.0];

const WGSL: &str = r#"
struct U {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz: the eye; w: the radius.
    eye: vec4<f32>,
    // x: strength; y, z: the fade's start and end.
    params: vec4<f32>,
    // x, y: the projection's near and far planes (depth 0..1 to the eye distance).
    lin: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var ao: texture_2d<f32>;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn size() -> vec2<f32> { return vec2<f32>(textureDimensions(depth)); }
fn depth_at(px: vec2<f32>) -> f32 {
    return textureLoad(depth, vec2<i32>(clamp(px, vec2<f32>(0.0), size() - 1.0)), 0);
}
// The distance along the view axis of a depth value (a 0..1 perspective projection).
fn linear(d: f32) -> f32 {
    return u.lin.x * u.lin.y / (u.lin.y - d * (u.lin.y - u.lin.x));
}
fn world_at(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let h = u.inv_view_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return h.xyz / h.w;
}

// At half the scene's size: each pixel reads the depth of the top-left full-size pixel under it, and rebuilds the
// position on that same pixel's ray (the half-size pixel's own centre is a corner between four of them).
@fragment fn fs_ao(i: VOut) -> @location(0) vec4<f32> {
    let px = floor(i.pos.xy) * 2.0;
    let d = depth_at(px);
    let p = world_at((px + 0.5) / size(), d);
    var n = cross(dpdx(p), dpdy(p));
    let nl = length(n);
    n = select(vec3<f32>(0.0, -1.0, 0.0), n / max(nl, 1e-12), nl > 1e-12);
    n = select(n, -n, dot(n, u.eye.xyz - p) < 0.0);
    // A basis about the normal, turned by interleaved gradient noise.
    let noise = fract(52.9829189 * fract(dot(i.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    let a = noise * 6.2831853;
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let t0 = normalize(cross(helper, n));
    let b0 = cross(n, t0);
    let t = t0 * cos(a) + b0 * sin(a);
    let b = cross(n, t);
    let r = u.eye.w;
    var occ = 0.0;
    for (var k = 0u; k < 12u; k++) {
        let f = (f32(k) + 0.5) / 12.0;
        let ang = f32(k) * 2.3999632;
        let rr = sqrt(f);
        let h = vec3<f32>(cos(ang) * rr, sin(ang) * rr, sqrt(max(1.0 - f, 0.0)));
        let s = p + (t * h.x + b * h.y + n * h.z) * r * mix(0.15, 1.0, f * f);
        let c = u.view_proj * vec4<f32>(s, 1.0);
        let q = c.xyz / c.w;
        let suv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
        let sd = depth_at(suv * size());
        // Occluded when the scene there is nearer than the sample, within the radius of this pixel (no halos off far
        // edges); samples off the screen have no depth to test and count as open. The scene's point lies on the ray
        // from the eye through the sample, so its linear depth over the sample's places it without a matrix.
        let on = all(suv >= vec2<f32>(0.0)) && all(suv <= vec2<f32>(1.0));
        let near = select(0.0, 1.0, on && sd < q.z - 1e-6);
        let hit = u.eye.xyz + (s - u.eye.xyz) * (linear(sd) / linear(q.z));
        let range = 1.0 - smoothstep(0.5 * r, r, distance(hit, p));
        occ += near * range;
    }
    let dist = distance(p, u.eye.xyz);
    let fade = 1.0 - smoothstep(u.params.y, u.params.z, dist);
    let v = select(occ / 12.0 * fade, 0.0, d >= 1.0);
    return vec4<f32>(v, 0.0, 0.0, 1.0);
}

// A 4 x 4 blur of the half-size occlusion, weighted against depth jumps along the view, and the frame multiplied by
// 1 - strength x AO.
@fragment fn fs_apply(i: VOut) -> @location(0) vec4<f32> {
    let d0 = depth_at(i.pos.xy);
    let l0 = linear(d0);
    let half = vec2<f32>(textureDimensions(ao));
    let c = i.pos.xy * 0.5;
    var sum = 0.0;
    var w = 0.0;
    for (var y = -2; y < 2; y++) {
        for (var x = -2; x < 2; x++) {
            let q = vec2<i32>(clamp(c + vec2<f32>(f32(x) + 0.5, f32(y) + 0.5), vec2<f32>(0.0), half - 1.0));
            let dq = depth_at(vec2<f32>(q) * 2.0);
            let wq = 1.0 - smoothstep(0.0, 0.5 * u.eye.w, abs(linear(dq) - l0));
            sum += textureLoad(ao, q, 0).r * wq;
            w += wq;
        }
    }
    let v = select(sum / max(w, 1e-4), 0.0, d0 >= 1.0);
    return vec4<f32>(vec3<f32>(1.0 - u.params.x * v), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    params: [f32; 4],
    lin: [f32; 4],
}

pub struct Ssao {
    layout: wgpu::BindGroupLayout,
    /// The AO pass's group and the apply pass's, by the depth and the texture each binds.
    groups: [super::BindCache<(wgpu::TextureView, wgpu::TextureView)>; 2],
    ao_pipeline: wgpu::RenderPipeline,
    apply_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// The occlusion target and its size.
    target: Option<([u32; 2], wgpu::TextureView)>,
    /// A 1 x 1 stand-in for the occlusion texture while the occlusion pass writes it.
    blank: wgpu::TextureView,
}

impl Ssao {
    /// `format`: the scene's colour format.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = |entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ssao"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // dst x src: the frame darkened, alpha kept.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Src,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let ao_pipeline = pipe("fs_ao", wgpu::TextureFormat::R8Unorm, None);
        let apply_pipeline = pipe("fs_apply", format, Some(multiply));
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blank = Self::ao_texture(device, [1, 1]);
        Self { groups: Default::default(), layout, ao_pipeline, apply_pipeline, uniform, target: None, blank }
    }

    fn ao_texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("ssao"),
                size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// The occlusion target at the scene's size, made on first use or a size change.
    pub fn resize(&mut self, gpu: &Gpu, size: [u32; 2]) {
        if self.target.as_ref().is_none_or(|t| t.0 != size) {
            self.target = Some((size, Self::ao_texture(&gpu.device, [size[0].div_ceil(2), size[1].div_ceil(2)])));
        }
    }

    /// Darkens `color` by the occlusion of the scenery in `depth` (the scene's, the size given to `resize`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        view_proj: Mat4,
        eye: Vec3,
    ) {
        let Some((_, ao)) = &self.target else { return };
        let p = Params {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, RADIUS],
            params: [STRENGTH, FADE[0], FADE[1], 0.0],
            lin: [super::NEAR, super::FAR, 0.0, 0.0],
        };
        super::Put::put(&gpu.queue, &self.uniform, 0, bytemuck::bytes_of(&p));
        let group = |k: usize, tex: &wgpu::TextureView| {
            self.groups[k].get((depth.clone(), tex.clone()), || {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ssao"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(tex) },
                    ],
                })
            })
        };
        let (g_ao, g_apply) = (group(0, &self.blank), group(1, ao));
        for (view, pipeline, g) in [(ao, &self.ao_pipeline, &g_ao), (color, &self.apply_pipeline, &g_apply)] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ssao"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, g, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        (0..h).flat_map(|y| d[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
    }

    /// A flat floor seen from above is not occluded; the sky (depth 1) is left alone.
    #[test]
    fn flat_floor_and_sky_untouched() {
        let Ok(gpu) = Gpu::headless() else { return };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut ao = Ssao::new(&gpu, format);
        let n = 32u32;
        ao.resize(&gpu, [n, n]);
        let cam =
            crate::render::Camera { eye: Vec3::new(0.0, -10.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_y: 0.8 };
        let vp = cam.view_proj(1.0);
        let depth = crate::render::depth_view(&gpu.device, n, n);
        let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let cv = color.create_view(&Default::default());
        for z in [vp.project_point3(Vec3::ZERO).z, 1.0] {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let _p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &cv,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.8, g: 0.8, b: 0.8, a: 1.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(z), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            ao.draw(&gpu, &mut enc, &cv, &depth, vp, cam.eye);
            gpu.queue.submit([enc.finish()]);
            for p in read(&gpu, &color, n, n).chunks_exact(4) {
                assert!((p[0] as i32 - 204).abs() <= 3, "depth {z}: {p:?}");
            }
        }
    }
}
