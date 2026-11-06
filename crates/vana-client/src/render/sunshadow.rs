//! Real character shadows (ours, not retail; Settings > Effects > Character Shadows, config `actor_shadows`): the
//! skinned actors drawn into a depth map from the zone's sun (one orthographic map around the camera's target), then
//! laid over the scenery as a darkening pass after all the scenery (the blended ground layers too, which would
//! otherwise cover it) and before the actors, so
//! characters never shadow themselves or each other and their painted light is untouched. Only surfaces facing the
//! sun are darkened: the baked lighting already darkens the rest. Without a sun (interiors, some nights) the client
//! keeps retail's shadows (render/shadow.rs).

use super::Gpu;
use glam::{Mat4, Vec3, Vec4};

/// The map's size in texels by default (config `shadow_map`: 1024, 2048 or 4096), and half the side of the square it
/// covers, in world units.
pub const MAP_SIZE: u32 = 2048;
pub const MAP_SIZES: [u32; 3] = [1024, 2048, 4096];
pub const HALF_EXTENT: f32 = 40.0;
/// How far the light's eye sits back from the target, and the depth range it covers.
const BACK: f32 = 150.0;
const DEPTH: f32 = 300.0;
/// The most a shadow takes off a surface (multiplied by 1 - this), at full sun: retail's silhouette's, MODULATE2X
/// of diffuse 0x40 into ZERO / INVSRCCOLOR (render/shadow.rs vs_ground), so a shadowed surface keeps half its light.
pub const STRENGTH: f32 = 2.0 * 64.0 / 255.0;
/// Below this strength (dawn and dusk, a dim light) retail's shadows draw instead, so no character goes without one.
pub const HANDOFF: f32 = 0.25 * STRENGTH;
/// How far below its caster's feet (world units, straight down) a shadow stays full, and where it has faded out: a
/// character atop a wall casts nothing onto the ground far below. Measured from the feet rather than along the light,
/// so a low sun or a small step in the ground (a seam between terrain pieces) does not cut a shadow off.
const FADE_START: f32 = 3.0;
const FADE_END: f32 = 7.0;

/// The format of the map's companion target: per texel, the feet height (world y) of the caster nearest the sun.
pub const FEET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

const WGSL: &str = r#"
struct U {
    inv_view_proj: mat4x4<f32>,
    light_vp: mat4x4<f32>,
    // xyz: the direction the light travels; w: strength (0..1).
    sun: vec4<f32>,
    // xyz: the eye; w: one map texel in world units.
    eye: vec4<f32>,
    // x: the PCF taps' spacing in texels (wider is softer: the moon's); y: unused;
    // z, w: the drops below the caster's feet where the shadow starts and ends fading.
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var map: texture_depth_2d;
@group(0) @binding(3) var cmp: sampler_comparison;
@group(0) @binding(4) var feet: texture_2d<f32>;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(clamp(i.pos.xy, vec2<f32>(0.0), vec2<f32>(textureDimensions(scene_depth)) - 1.0));
    let d = textureLoad(scene_depth, px, 0);
    let ndc = vec4<f32>(i.uv.x * 2.0 - 1.0, 1.0 - i.uv.y * 2.0, d, 1.0);
    let h = u.inv_view_proj * ndc;
    let world = h.xyz / h.w;
    // The surface normal from the reconstructed positions, turned toward the eye (derivatives before any branch).
    var n = cross(dpdx(world), dpdy(world));
    let nl = length(n);
    n = select(vec3<f32>(0.0, 1.0, 0.0), n / max(nl, 1e-12), nl > 1e-12);
    n = select(n, -n, dot(n, u.eye.xyz - world) < 0.0);
    let facing = dot(n, -u.sun.xyz);
    // Pushed out along the normal by a texel or two against acne.
    let lp = u.light_vp * vec4<f32>(world + n * (1.5 * u.eye.w), 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    let texel = u.misc.x / f32(textureDimensions(map).x);
    let dim = vec2<f32>(textureDimensions(map));
    var dark = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            // Bilinear over the tap's four texels, each compared and faded by how far the surface lies below its own
            // caster's feet (a filtered comparison would mix a high caster's edge into the cleared texels beside it,
            // unfaded). The world's y points down, so the drop is the surface's y less the feet's.
            let f = (uv + vec2<f32>(f32(x), f32(y)) * texel) * dim - 0.5;
            let b = floor(f);
            let w = f - b;
            var c = vec4<f32>(0.0);
            for (var k = 0; k < 4; k++) {
                let o = vec2<f32>(f32(k & 1), f32(k >> 1u));
                let t = vec2<i32>(clamp(b + o, vec2<f32>(0.0), dim - 1.0));
                let caster = textureLoad(map, t, 0);
                let drop = world.y - textureLoad(feet, t, 0).x;
                c[k] = select(0.0, 1.0 - smoothstep(u.misc.z, u.misc.w, drop), lp.z > caster);
            }
            dark += mix(mix(c.x, c.y, w.x), mix(c.z, c.w, w.x), w.y);
        }
    }
    let shadow = dark / 9.0;
    // Fades: toward the map's edge, on surfaces turning away from the sun, and off the sky (depth 1).
    let edge = 1.0 - smoothstep(0.8, 1.0, max(abs(lp.x), abs(lp.y)));
    let k = u.sun.w * shadow * edge * smoothstep(0.0, 0.25, facing) * select(1.0, 0.0, d >= 1.0 || lp.z > 1.0 || lp.z < 0.0);
    return vec4<f32>(vec3<f32>(1.0 - k), 1.0);
}
"#;

/// The light's view-projection (orthographic, depth 0..1) for a sun travelling along `dir`, around `target`, the
/// centre snapped to whole texels of a `size` map so the shadows do not shimmer as the camera moves. None without a
/// usable direction.
pub fn light_matrix(dir: Vec3, target: Vec3, size: u32) -> Option<Mat4> {
    let d = sun_direction(dir)?;
    let up = if d.x.abs() < 1e-3 && d.z.abs() < 1e-3 { Vec3::Z } else { Vec3::Y };
    let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, d, up);
    // Snap the target in light space.
    let texel = 2.0 * HALF_EXTENT / size.max(1) as f32;
    let t = view.transform_point3(target);
    let t = Vec3::new((t.x / texel).round() * texel, (t.y / texel).round() * texel, t.z);
    // Light space: x right, y up, looking down -z; the box is [t.x +- E] x [t.y +- E] x [t.z + BACK .. t.z + BACK - DEPTH].
    let (l, r, b, tp) = (t.x - HALF_EXTENT, t.x + HALF_EXTENT, t.y - HALF_EXTENT, t.y + HALF_EXTENT);
    let (zn, zf) = (-(t.z + BACK), -(t.z + BACK) + DEPTH);
    let proj = Mat4::from_cols(
        Vec4::new(2.0 / (r - l), 0.0, 0.0, 0.0),
        Vec4::new(0.0, 2.0 / (tp - b), 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0 / (zf - zn), 0.0),
        Vec4::new(-(r + l) / (r - l), -(tp + b) / (tp - b), -zn / (zf - zn), 1.0),
    );
    Some(proj * view)
}

/// Whether an actor at `pos` with a box of `radius` lies wholly inside the map `light_vp` covers; the client keeps
/// retail's shadow for the others. Near the edge the real shadow has faded (from 0.8 of the half side) before retail's
/// comes back, so the two barely overlap.
pub fn covers(light_vp: Mat4, pos: Vec3, radius: f32) -> bool {
    let p = light_vp.project_point3(pos);
    let r = radius / HALF_EXTENT;
    p.x.abs() + r < 1.0 && p.y.abs() + r < 1.0 && p.z > 0.0 && p.z < 1.0
}

/// The sun's travel direction with retail's silhouette rule (FUN_10033080, render/shadow.rs): a light under 60 degrees
/// is held at 40.9 (y = sin(pi / 3) of the level length), so shadows keep the game's length. The world's y points
/// down (a light toward -y is above), so a sun shining down travels toward +y.
pub fn sun_direction(dir: Vec3) -> Option<Vec3> {
    let d = dir.try_normalize()?;
    let h = (d.x * d.x + d.z * d.z).sqrt();
    if h < 1e-4 {
        return Some(Vec3::Y);
    }
    if d.y <= 0.0 || d.y.atan2(h) < 60f32.to_radians() {
        return Vec3::new(d.x, 0.866_025_4 * h, d.z).try_normalize();
    }
    Some(d)
}

/// How much of the shadow shows at Vana'diel `minute`: none at 6:00 and 18:00, when the light changes between sun
/// and moon, rising to full over the 45 minutes either side, so the change of direction is never seen.
pub fn twilight(minute: f32) -> f32 {
    let m = minute.rem_euclid(1440.0);
    let gap = |at: f32| {
        let d = (m - at).abs();
        d.min(1440.0 - d)
    };
    let f = |d: f32| {
        let t = (d / 45.0).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    f(gap(360.0)) * f(gap(1080.0))
}

/// The shadow's strength for a sun of this colour: full in daylight, none for a dim light (night, dusk).
pub fn strength(diffuse: Vec3) -> f32 {
    let lum = diffuse.dot(Vec3::new(0.299, 0.587, 0.114));
    STRENGTH * ((lum - 0.05) / 0.25).clamp(0.0, 1.0)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    inv_view_proj: [[f32; 4]; 4],
    light_vp: [[f32; 4]; 4],
    sun: [f32; 4],
    eye: [f32; 4],
    misc: [f32; 4],
}

pub struct SunShadow {
    pub map: wgpu::TextureView,
    /// Beside the map: each texel's caster's feet height (FEET_FORMAT), written by the same pass.
    pub feet: wgpu::TextureView,
    /// The map's size in texels.
    pub size: u32,
    /// The light matrix for the depth pass (group 3 of the silhouette layout, 256 bytes).
    pub matrix: wgpu::Buffer,
    pub matrix_group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    cmp: wgpu::Sampler,
}

impl SunShadow {
    /// `format`: the scene's colour format (the plain view the world draws through).
    pub fn new(gpu: &Gpu, sil_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat, size: u32) -> Self {
        let device = &gpu.device;
        let map = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("sun shadow map"),
                size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let feet = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("sun shadow feet"),
                size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FEET_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let matrix = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sun shadow matrix"), size: 256, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let matrix_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun shadow matrix"),
            layout: sil_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &matrix, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        let tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sun shadow receive"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1),
                tex(2),
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sun shadow receive"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sun shadow receive"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        // dst x src: the frame darkened, alpha kept.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sun shadow receive"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sun shadow receive"), size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sun shadow compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        Self { map, feet, size, matrix, matrix_group, layout, pipeline, uniform, cmp }
    }

    /// Writes this frame's light matrix for the depth pass.
    pub fn set_light(&self, gpu: &Gpu, light_vp: Mat4) {
        super::Put::put(&gpu.queue, &self.matrix, 0, bytemuck::cast_slice(&light_vp.to_cols_array()));
    }

    /// Darkens `color` where the map shadows the scenery in `depth` (the scene's). `soft`: the PCF spacing in texels.
    #[allow(clippy::too_many_arguments)]
    pub fn receive(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view_proj: Mat4, light_vp: Mat4, sun: Vec3, strength: f32, eye: Vec3, soft: f32) {
        let p = Params {
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            light_vp: light_vp.to_cols_array_2d(),
            sun: [sun.x, sun.y, sun.z, strength],
            eye: [eye.x, eye.y, eye.z, 2.0 * HALF_EXTENT / self.size as f32],
            misc: [soft, 0.0, FADE_START, FADE_END],
        };
        super::Put::put(&gpu.queue, &self.uniform, 0, bytemuck::bytes_of(&p));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun shadow receive"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.map) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.cmp) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.feet) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sun shadow receive"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point under the target maps to the map's centre at a depth inside 0..1; a low sun is raised to 30 degrees (y is down).
    #[test]
    fn light_matrix_frames_the_target() {
        let target = Vec3::new(100.0, -5.0, 40.0);
        let m = light_matrix(Vec3::new(0.3, 1.0, 0.2), target, MAP_SIZE).unwrap();
        let c = m.project_point3(target);
        assert!(c.x.abs() < 0.01 && c.y.abs() < 0.01, "{c}");
        assert!(c.z > 0.0 && c.z < 1.0, "{c}");
        // A point a step toward the light is nearer (smaller depth).
        let toward = m.project_point3(target - Vec3::new(0.3, 1.0, 0.2).normalize() * 5.0);
        assert!(toward.z < c.z);
        // Under 60 degrees the light is held at 40.9 (retail's silhouette rule); above, it is left alone.
        let low = sun_direction(Vec3::new(1.0, 0.01, 0.0)).unwrap();
        assert!((low.y / (low.x * low.x + low.z * low.z).sqrt() - 0.866_025_4).abs() < 1e-4);
        let high = Vec3::new(0.2, 1.0, 0.0).normalize();
        assert!((sun_direction(high).unwrap() - high).length() < 1e-6);
        assert!(sun_direction(Vec3::ZERO).is_none());
        assert_eq!((twilight(360.0), twilight(1080.0), twilight(720.0), twilight(0.0)), (0.0, 0.0, 1.0, 1.0));
        assert!(twilight(380.0) > 0.0 && twilight(380.0) < 1.0 && twilight(1060.0) < 1.0);
        assert!(covers(m, target, 2.0) && !covers(m, target + Vec3::new(60.0, 0.0, 0.0), 2.0));
        assert_eq!(strength(Vec3::ZERO), 0.0);
        assert!((strength(Vec3::ONE) - STRENGTH).abs() < 1e-6);
    }

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

    /// The receive pass over a floor seen from above, lit from above: an empty map leaves it as it was, a map that
    /// shadows everything just above it darkens it by the strength, a caster standing on a ledge far above leaves it
    /// lit, and one standing a small step above stays dark however far along the light its body is (a terrain seam).
    #[test]
    fn receive_darkens_shadowed_floor() {
        let Ok(gpu) = Gpu::headless() else { return };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let silly = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(64) },
                count: None,
            }],
        });
        let sh = SunShadow::new(&gpu, &silly, format, 1024);
        let n = 16u32;
        // y is down: the eye 10 above the floor at y = 0, looking down.
        let cam = crate::render::Camera { eye: Vec3::new(0.0, -10.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_y: 0.8 };
        let vp = cam.view_proj(1.0);
        let floor_z = vp.project_point3(Vec3::ZERO).z;
        let sun = Vec3::Y;
        let light = light_matrix(sun, Vec3::ZERO, MAP_SIZE).unwrap();
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
        // The floor's depth in the map: a caster just above it shadows, one far above does not.
        let floor_lz = light.project_point3(Vec3::ZERO).z;
        let run = |map_depth: f32, feet_y: f32| {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let _p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &cv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.8, g: 0.8, b: 0.8, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(floor_z), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            {
                let _p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &sh.feet, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: feet_y as f64, g: 0.0, b: 0.0, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &sh.map, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(map_depth), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            sh.receive(&gpu, &mut enc, &cv, &depth, vp, light, sun, STRENGTH, cam.eye, 1.0);
            gpu.queue.submit([enc.finish()]);
            read(&gpu, &color, n, n)
        };
        let lit = (0.8f32 * 255.0).round() as i32;
        let dark = (0.8 * (1.0 - STRENGTH) * 255.0).round() as i32;
        for p in run(1.0, 0.0).chunks_exact(4) {
            assert!((p[0] as i32 - lit).abs() <= 2, "unshadowed {p:?}");
        }
        for p in run(floor_lz - 1.0 / DEPTH, 0.0).chunks_exact(4) {
            assert!((p[0] as i32 - dark).abs() <= 3, "shadowed {p:?} want {dark}");
        }
        // y is down: feet 20 above the floor.
        for p in run(floor_lz - 20.0 / DEPTH, -20.0).chunks_exact(4) {
            assert!((p[0] as i32 - lit).abs() <= 2, "caster far above {p:?}");
        }
        for p in run(floor_lz - 20.0 / DEPTH, -1.0).chunks_exact(4) {
            assert!((p[0] as i32 - dark).abs() <= 3, "caster a step above {p:?} want {dark}");
        }
    }
}
