//! Lamp shadows (ours, not retail): with Real character shadows, the few point lights nearest our own character (the
//! zone's data lights, and the lamps of Real lamp lights, client/lamps.rs) each get a small perspective depth map of
//! it, seen from the light, so standing between two lights casts two shadows. Each map darkens the scenery behind the
//! character as the sun's does (a pass after all the scenery, before the actors), as dark as the sun's shadow and
//! fading out toward the light's reach. While a lamp casts our character's shadow, the sun's map leaves it out
//! (ActorGpu::lamp_lit): the main shadow turns to the lamp, and back to the sun past its reach. Other characters cast
//! none (ActorGpu::lamp_caster).

use super::Gpu;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Lamps shadowed at once (the map's layers).
pub const LAYERS: usize = 4;
/// Each layer's size in texels.
pub const SIZE: u32 = 512;
/// A character this close to a lamp casts from it (client/lamps.rs REACH, the floor lamp's reach).
pub const REACH: f32 = 16.0;
/// How much a full lamp shadow darkens a surface: the sun's (sunshadow::STRENGTH), so both look alike.
pub const STRENGTH: f32 = super::sunshadow::STRENGTH;
/// The shadow fades out between these distances of the surface from the lamp (as fractions of its light's range,
/// at most REACH).
const FADE_START: f32 = 0.6;
const FADE_END: f32 = 1.0;
/// Within this fraction of a lit lamp's range our character's main shadow is the lamp's (zone.wgsl
/// vs_skinned_sun_lamped); further out the sun's comes back, as the lamp's fades.
const TAKEOVER: f32 = 0.8;
/// A character as a sphere: its centre this far above the feet (FFXI's y points down), this radius.
const CENTRE: f32 = 0.9;
const RADIUS: f32 = 1.2;
const NEAR: f32 = 0.25;
/// The widest half-angle a lamp's map covers; characters outside it cast nothing from that lamp.
const MAX_HALF: f32 = 60.0 * std::f32::consts::PI / 180.0;
const MIN_HALF: f32 = 5.0 * std::f32::consts::PI / 180.0;

/// The zone shader's view (group 0 binding 5): per layer the lamp's light slot + 1 (0 = unused) and its matrix;
/// misc = (strength, PCF step in uv, REACH, TAKEOVER). The strength is 0: the receive pass darkens instead (zone.wgsl lamp_vis
/// would take the light out a second time).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LampShadowU {
    pub slots: [u32; 4],
    pub vp: [[[f32; 4]; 4]; LAYERS],
    pub misc: [f32; 4],
}

/// The light matrix of a lamp at `lamp` for the characters at `feet` (positions), aimed at their centre and widened
/// to take them all in (up to MAX_HALF). None when none is within REACH.
pub fn matrix(lamp: Vec3, feet: &[Vec3]) -> Option<Mat4> {
    let centres: Vec<Vec3> = feet.iter().map(|p| *p - Vec3::Y * CENTRE).filter(|c| c.distance(lamp) < REACH).collect();
    if centres.is_empty() {
        return None;
    }
    let mid = centres.iter().copied().sum::<Vec3>() / centres.len() as f32;
    // Casters around the lamp itself (a low lamp over a head) give no direction: look straight down (y is down).
    let dir = (mid - lamp).try_normalize().filter(|_| mid.distance(lamp) > RADIUS).unwrap_or(Vec3::Y);
    let half = centres
        .iter()
        .map(|c| {
            let d = c.distance(lamp);
            if d <= RADIUS {
                return MAX_HALF;
            }
            dir.dot((*c - lamp) / d).clamp(-1.0, 1.0).acos() + (RADIUS / d).min(1.0).asin()
        })
        .fold(0.0f32, f32::max)
        .clamp(MIN_HALF, MAX_HALF);
    let up = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    Some(glam::camera::rh::proj::directx::perspective(2.0 * half, 1.0, NEAR, REACH + RADIUS) * glam::camera::rh::view::look_at_mat4(lamp, lamp + dir, up))
}

/// Whether a character with its feet at `feet` casts into a shadowed lamp's map (`vp`, from `matrix`): its centre
/// within REACH of the `lamp` and inside the map's cone. The client then drops its retail shadow, whose light eases
/// toward the lamp over seconds.
pub fn casts(lamp: Vec3, vp: Mat4, feet: Vec3) -> bool {
    let c = feet - Vec3::Y * CENTRE;
    if c.distance(lamp) >= REACH {
        return false;
    }
    let q = vp * c.extend(1.0);
    if q.w <= 0.0 {
        return false;
    }
    let p = q.truncate() / q.w;
    p.x.abs() < 1.0 && p.y.abs() < 1.0 && p.z > 0.0 && p.z < 1.0
}

/// This frame's shadowed lights (light slot, matrix, position) from `lamps` (position, light slot): those with a character of `feet` in reach, the
/// nearest to `target` first, at most LAYERS.
pub fn pick(lamps: &[(Vec3, u32)], feet: &[Vec3], target: Vec3) -> Vec<(u32, Mat4, Vec3)> {
    let mut near: Vec<(f32, Vec3, u32)> = lamps.iter().filter(|(p, _)| feet.iter().any(|f| f.distance(*p) < REACH)).map(|&(p, s)| (p.distance(target), p, s)).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter().filter_map(|(_, p, s)| Some((s, matrix(p, feet)?, p))).take(LAYERS).collect()
}

/// The maps, their matrices for the depth passes (group 3 of the silhouette layout, 256 bytes apart) and the zone
/// shader's uniform, comparison sampler and array view (bound in the zone's globals).
pub struct LampShadows {
    pub layers: Vec<wgpu::TextureView>,
    pub array: wgpu::TextureView,
    /// A 1x1 map for the depth passes' group 0: a pass may not sample the array it draws a layer of.
    pub blank: wgpu::TextureView,
    pub cmp: wgpu::Sampler,
    pub uniform: wgpu::Buffer,
    pub matrices: wgpu::Buffer,
    pub matrix_group: wgpu::BindGroup,
    receive_layout: wgpu::BindGroupLayout,
    receive_pipeline: wgpu::RenderPipeline,
    receive_uniform: wgpu::Buffer,
}

impl LampShadows {
    /// `format`: the scene's colour format, which the receive pass darkens.
    pub fn new(gpu: &Gpu, sil_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lamp shadow maps"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: LAYERS as u32 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers = (0..LAYERS as u32)
            .map(|l| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: l, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        let array = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let blank = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("lamp shadow blank"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lamp shadow compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lamp shadows"), size: std::mem::size_of::<LampShadowU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&LampShadowU::zeroed()));
        let matrices = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lamp shadow matrices"), size: 256 * LAYERS as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let matrix_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lamp shadow matrices"),
            layout: sil_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &matrices, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        let (receive_layout, receive_pipeline) = receive_pipeline(device, format);
        let receive_uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lamp shadow receive"), size: std::mem::size_of::<ReceiveU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { layers, array, blank, cmp, uniform, matrices, matrix_group, receive_layout, receive_pipeline, receive_uniform }
    }

    /// Writes this frame's lamps (light slot, matrix) for the depth passes and the zone shader; none clears them.
    pub fn set(&self, gpu: &Gpu, lamps: &[(u32, Mat4, Vec3)]) {
        let mut u = LampShadowU::zeroed();
        for (k, (slot, m, _)) in lamps.iter().take(LAYERS).enumerate() {
            u.slots[k] = slot + 1;
            u.vp[k] = m.to_cols_array_2d();
            super::Put::put(&gpu.queue, &self.matrices, 256 * k as u64, bytemuck::cast_slice(&m.to_cols_array()));
        }
        u.misc = [0.0, 1.0 / SIZE as f32, REACH, TAKEOVER];
        super::Put::put(&gpu.queue, &self.uniform, 0, bytemuck::bytes_of(&u));
    }

    /// Darkens `color` where this frame's `lamps` (as given to `set`, whose maps are drawn) shadow the scenery in
    /// `depth` (the scene's). Nothing without lamps.
    #[allow(clippy::too_many_arguments)]
    /// `lights`: the zone's point lights (zone.wgsl plights), whose live range and intensity limit each shadow to
    /// where its light reaches.
    pub fn receive(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view_proj: Mat4, lamps: &[(u32, Mat4, Vec3)], eye: Vec3, lights: &wgpu::Buffer) {
        if lamps.is_empty() {
            return;
        }
        let mut u = ReceiveU::zeroed();
        u.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        for (k, (slot, m, p)) in lamps.iter().take(LAYERS).enumerate() {
            u.slots[k] = slot + 1;
            u.vp[k] = m.to_cols_array_2d();
            u.lamp[k] = [p.x, p.y, p.z, 1.0];
        }
        u.eye = [eye.x, eye.y, eye.z, STRENGTH];
        u.misc = [1.0 / SIZE as f32, FADE_START, FADE_END, REACH];
        super::Put::put(&gpu.queue, &self.receive_uniform, 0, bytemuck::bytes_of(&u));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lamp shadow receive"),
            layout: &self.receive_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.receive_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.array) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.cmp) },
                wgpu::BindGroupEntry { binding: 4, resource: lights.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lamp shadow receive"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.receive_pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The receive pass's uniform: per layer the light slot + 1 (0 = unused), the lamp's matrix and position, the eye
/// (w the strength) and misc = (PCF step in uv, fade start and end as fractions of the range, REACH).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ReceiveU {
    slots: [u32; 4],
    inv_view_proj: [[f32; 4]; 4],
    vp: [[[f32; 4]; 4]; LAYERS],
    lamp: [[f32; 4]; LAYERS],
    eye: [f32; 4],
    misc: [f32; 4],
}

const RECEIVE_WGSL: &str = r#"
struct U {
    slots: vec4<u32>,
    inv_view_proj: mat4x4<f32>,
    vp: array<mat4x4<f32>, 4>,
    lamp: array<vec4<f32>, 4>,
    eye: vec4<f32>,
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var maps: texture_depth_2d_array;
@group(0) @binding(3) var cmp: sampler_comparison;
// The zone's point lights (zone.wgsl plights): per slot (position, range), (colour, intensity).
@group(0) @binding(4) var<storage, read> plights: array<vec4<f32>>;

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
    var n = cross(dpdx(world), dpdy(world));
    let nl = length(n);
    n = select(vec3<f32>(0.0, 1.0, 0.0), n / max(nl, 1e-12), nl > 1e-12);
    n = select(n, -n, dot(n, u.eye.xyz - world) < 0.0);
    var lit = 1.0;
    for (var j = 0; j < 4; j++) {
        let s = u.slots[j];
        if (s == 0u) { continue; }
        // No shadow where the light does not reach: off (no intensity or colour) or past its range.
        let a = plights[2u * (s - 1u)];
        let col = plights[2u * (s - 1u) + 1u];
        if (col.w <= 0.0 || max(col.r, max(col.g, col.b)) <= 0.0) { continue; }
        let range = min(a.w, u.misc.w);
        let to_lamp = u.lamp[j].xyz - world;
        let dist = length(to_lamp);
        if (dist >= range) { continue; }
        let facing = dot(n, to_lamp / max(dist, 1e-6));
        let c = u.vp[j] * vec4<f32>(world + n * 0.05, 1.0);
        if (c.w <= 0.0) { continue; }
        let q = c.xyz / c.w;
        if (abs(q.x) >= 1.0 || abs(q.y) >= 1.0 || q.z <= 0.0 || q.z >= 1.0) { continue; }
        let uv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
        var vis = 0.0;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                vis = vis + textureSampleCompareLevel(maps, cmp, uv + vec2<f32>(f32(dx), f32(dy)) * u.misc.x, j, q.z - 0.0005);
            }
        }
        let shadow = 1.0 - vis / 9.0;
        let edge = 1.0 - smoothstep(0.8, 1.0, max(abs(q.x), abs(q.y)));
        let k = u.eye.w * shadow * edge * smoothstep(0.0, 0.25, facing) * (1.0 - smoothstep(u.misc.y * range, u.misc.z * range, dist));
        lit = lit * (1.0 - k);
    }
    lit = select(lit, 1.0, d >= 1.0);
    return vec4<f32>(vec3<f32>(lit), 1.0);
}
"#;

fn receive_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lamp shadow receive"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lamp shadow receive"), source: wgpu::ShaderSource::Wgsl(RECEIVE_WGSL.into()) });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("lamp shadow receive"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    // dst x src: the frame darkened, alpha kept (as the sun's).
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lamp shadow receive"),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })] }),
        multiview_mask: None,
        cache: None,
    });
    (layout, pipeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(m: Mat4, p: Vec3) -> Vec3 {
        let c = m * p.extend(1.0);
        c.truncate() / c.w
    }

    /// Only characters inside the lamp's map lose their retail shadow: two either side of a low lamp lie outside the
    /// 60-degree cone it aims between them.
    #[test]
    fn casts_only_inside_the_map() {
        let lamp = Vec3::new(0.0, -4.0, 0.0);
        let feet = [Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        let m = matrix(lamp, &feet).unwrap();
        assert!(feet.iter().all(|f| !casts(lamp, m, *f)));
        let one = [Vec3::new(3.0, 0.0, 1.0)];
        let m = matrix(lamp, &one).unwrap();
        assert!(casts(lamp, m, one[0]));
        assert!(!casts(lamp, m, Vec3::new(30.0, 0.0, 0.0)));
    }

    /// The map takes in every character near the lamp, in front of it and inside the depth range.
    #[test]
    fn matrix_frames_the_characters() {
        let lamp = Vec3::new(0.0, -4.0, 0.0);
        let feet = [Vec3::new(3.0, 0.0, 1.0), Vec3::new(-2.0, 0.0, 4.0), Vec3::new(1.0, 0.0, -3.0)];
        let m = matrix(lamp, &feet).unwrap();
        for f in feet {
            let p = project(m, f - Vec3::Y * CENTRE);
            assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..1.0).contains(&p.z), "{f} -> {p}");
        }
        // A low lamp right over a character still casts, looking down at it.
        let m = matrix(Vec3::new(0.0, -1.8, 0.0), &[Vec3::ZERO]).unwrap();
        let p = project(m, Vec3::ZERO);
        assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..1.0).contains(&p.z), "feet -> {p}");
        // Too far: none.
        assert!(matrix(lamp, &[Vec3::new(40.0, 0.0, 0.0)]).is_none());
    }

    /// The receive pass's shader parses and validates (no GPU needed).
    #[test]
    fn receive_shader_is_valid() {
        let m = wgpu::naga::front::wgsl::parse_str(RECEIVE_WGSL).expect("parse");
        wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all()).validate(&m).expect("valid");
    }

    /// The zone shader (its lamp-lit sun depth entry included) parses and validates.
    #[test]
    fn zone_shader_is_valid() {
        let m = wgpu::naga::front::wgsl::parse_str(include_str!("zone.wgsl")).expect("parse");
        wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all()).validate(&m).expect("valid");
    }

    /// Only lamps with a character in reach, nearest the camera first, at most LAYERS.
    #[test]
    fn pick_nearest_lamps_with_casters() {
        let feet = [Vec3::ZERO];
        let lamps: Vec<(Vec3, u32)> = (0..8).map(|i| (Vec3::new(i as f32 * 3.0, -3.0, 0.0), 10 + i)).chain([(Vec3::new(100.0, -3.0, 0.0), 99)]).collect();
        let got = pick(&lamps, &feet, Vec3::new(6.0, 0.0, 0.0));
        let slots: Vec<u32> = got.iter().map(|g| g.0).collect();
        assert_eq!(slots.len(), LAYERS);
        assert_eq!(slots[0], 12);
        assert!(!slots.contains(&99));
    }
}
