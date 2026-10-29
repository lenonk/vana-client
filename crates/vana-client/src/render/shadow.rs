//! Retail's per-actor ground shadow (FUN_10032f80 from the actor draw; research actor_shadow_spec.md). Per caster
//! this frame: the projected blob (mode 0: "system  kage_tex" painted top-down on the collision faces of the 3 x 3
//! cells around it), the silhouette (mode 1: the actor drawn flat white from its light into a layer, then laid on the
//! same faces without the normal test) or the blob quad (mode 2: a flat square at its feet). Each darkens the frame
//! (dst x (1 - src)), depth tested and z-biased, no fog. The light (the strongest point light in range of the actor's
//! floor, else the directional light), its smoothing, the size and the silhouette's matrix are worked out per caster
//! on the GPU (`prep`).

use super::{ActorGpu, Gpu, Put, ZoneRenderer, DEPTH_FORMAT, NO_WEP};
use glam::Vec3;
use std::collections::HashMap;

/// The silhouette layers' format; each is the scene's size, as retail's one target (FUN_100328f0). High gives every
/// caster a layer, nearest first, while they fit in SIL_BUDGET bytes (at most SIL_LAYERS); the rest blob.
pub const SIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const SIL_LAYERS: u32 = 32;
const SIL_BUDGET: u64 = 128 << 20;
/// Casters a frame, and the light-state slots (one per actor while it casts).
pub const MAX_CASTERS: usize = 256;
/// A caster's output: 16 vec4s, 256 bytes (the silhouette pass reads its matrix at a dynamic offset).
const OUT_STRIDE: u64 = 256;

/// One actor's shadow this frame.
pub struct Caster<'a> {
    /// 0 projected blob, 1 silhouette, 2 quad.
    pub mode: u8,
    /// Its position (the drawn one) and skeleton box corners, world space.
    pub pos: Vec3,
    pub corners: [Vec3; 8],
    /// The floor's point lights (table slot + 1, 0 none).
    pub lights: [u8; 4],
    /// Its light-state slot, and whether the slot is new (the light starts at its target).
    pub state: u32,
    pub fresh: bool,
    /// The 3 x 3 cells around it (x0, x1, z0, z1), for modes 0 / 1.
    pub cells: Option<(usize, usize, usize, usize)>,
    /// Mode 1: the actor (its skin and pose region) and its silhouette layer.
    pub actor: Option<&'a ActorGpu>,
    pub layer: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CasterGpu {
    corners: [[f32; 4]; 8],
    /// xyz, mode.
    pos: [f32; 4],
    /// Light slots (4 bytes), state slot, layer, bit 0 fresh.
    info: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// The directional light's travel direction, w 1 when there is one.
    sun: [f32; 4],
    /// Ticks this frame (60 Hz), casters.
    misc: [f32; 4],
}

const WGSL: &str = r#"
struct CasterIn {
    corners: array<vec4<f32>, 8>,
    pos: vec4<f32>,
    info: vec4<u32>,
};
struct Params {
    sun: vec4<f32>,
    misc: vec4<f32>,
};

@group(0) @binding(0) var<uniform> pr: Params;
@group(0) @binding(1) var<storage, read> lights: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> casters: array<CasterIn>;
@group(0) @binding(3) var<storage, read_write> state: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outs: array<vec4<f32>>;

// D3DXMatrixLookAtRH / PerspectiveFovRH as column matrices (glam's look_at_rh / perspective_rh).
fn look_at(eye: vec3<f32>, at: vec3<f32>, up: vec3<f32>) -> mat4x4<f32> {
    let f = normalize(at - eye);
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    return mat4x4<f32>(vec4<f32>(s.x, u.x, -f.x, 0.0), vec4<f32>(s.y, u.y, -f.y, 0.0), vec4<f32>(s.z, u.z, -f.z, 0.0), vec4<f32>(-dot(eye, s), -dot(eye, u), dot(eye, f), 1.0));
}

fn perspective(fovy: f32, aspect: f32, zn: f32, zf: f32) -> mat4x4<f32> {
    let h = 1.0 / tan(fovy * 0.5);
    let r = zf / (zn - zf);
    return mat4x4<f32>(vec4<f32>(h / aspect, 0.0, 0.0, 0.0), vec4<f32>(0.0, h, 0.0, 0.0), vec4<f32>(0.0, 0.0, r, -1.0), vec4<f32>(0.0, 0.0, r * zn, 0.0));
}

@compute @workgroup_size(64)
fn prep(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u32(pr.misc.y)) { return; }
    let c = casters[i];
    let pos = c.pos.xyz;
    let mode = u32(c.pos.w);
    // FUN_100c94a0: the strongest point light in range, else the directional light.
    var best = 0.0;
    var l0 = vec3<f32>(0.0, -1.0, 0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        let s = (c.info.x >> (8u * k)) & 0xffu;
        if (s == 0u) { continue; }
        let a = lights[2u * (s - 1u)];
        let col = lights[2u * (s - 1u) + 1u];
        let d = a.xyz - pos;
        let d2 = dot(d, d);
        if (sqrt(d2) > a.w) { continue; }
        let att2 = select(1.0, 1.0 / col.w, col.w > 0.0);
        let wt = 1.0 / max(d2 * att2, 1e-4);
        if (wt > best) {
            best = wt;
            l0 = d;
        }
    }
    if (best == 0.0 && pr.sun.w > 0.5) { l0 = -pr.sun.xyz; }
    // FUN_10033080: below the horizon a fixed light; the raw and final directions eased at 0.04 a tick; lights under
    // 60 degrees held at sin(pi / 3) of the level length.
    if (l0.y > 0.0) { l0 = vec3<f32>(0.45, -0.9, 0.0); }
    let t = min(1.0, pr.misc.x * 0.04);
    let si = c.info.y;
    var r = state[2u * si].xyz;
    var lf = state[2u * si + 1u].xyz;
    if ((c.info.w & 1u) != 0u) {
        r = l0;
        lf = normalize(l0);
    }
    r = r + (l0 - r) * t;
    var l = normalize(r);
    let h = sqrt(l.x * l.x + l.z * l.z);
    if (h <= 1e-4) {
        l = vec3<f32>(0.0, -1.0, 0.0);
    } else if (l.y >= 0.0 || -atan2(l.y, h) < 1.0471976) {
        l.y = -0.8660254 * h;
        l = normalize(l);
    }
    lf = lf + (l - lf) * t;
    if (abs(lf.x) < 0.01 && abs(lf.z) < 0.01) { lf.x = 0.01; }
    lf = normalize(lf);
    state[2u * si] = vec4<f32>(r, 0.0);
    state[2u * si + 1u] = vec4<f32>(lf, 0.0);
    // The size: the box seen from the light, the larger side (5 / 9 of it for blobs).
    let v = look_at(pos - lf, pos, vec3<f32>(0.0, -1.0, 0.0));
    let rot = mat3x3<f32>(v[0].xyz, v[1].xyz, v[2].xyz);
    var lo = vec3<f32>(1e30);
    var hi = vec3<f32>(-1e30);
    for (var k = 0u; k < 8u; k = k + 1u) {
        let p = rot * c.corners[k].xyz;
        lo = min(lo, p);
        hi = max(hi, p);
    }
    var size = max(hi.x - lo.x, hi.y - lo.y);
    if (mode != 1u) { size = size * (5.0 / 9.0); }
    size = max(size, 0.01);
    let vp = perspective(0.19634954, 1.0, 0.1, 65535.0) * look_at(pos - 12.0 * size * lf, pos, vec3<f32>(0.0, -1.0, 0.0));
    let o = 16u * i;
    outs[o] = vp[0];
    outs[o + 1u] = vp[1];
    outs[o + 2u] = vp[2];
    outs[o + 3u] = vp[3];
    outs[o + 4u] = vec4<f32>(pos, f32(mode));
    outs[o + 5u] = vec4<f32>(lf, 0.0);
    outs[o + 6u] = vec4<f32>(size / 3.0, size * 2.4, f32(c.info.z), 0.0);
}

// ---- Drawing ----

struct U {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var kage: texture_2d<f32>;
@group(1) @binding(0) var sil: texture_2d_array<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var<storage, read> douts: array<vec4<f32>>;
@group(2) @binding(1) var<storage, read> inst: array<u32>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) c: f32,
    @location(2) @interpolate(flat) layer: u32,
};

// ZBIAS 15: as the effects' (MODLOG "D3DRS_ZBIAS 15").
fn to_clip(p: vec3<f32>) -> vec4<f32> {
    var c = u.view_proj * vec4<f32>(p, 1.0);
    c.z = c.z - 15.0 * 5e-6 * c.w;
    return c;
}

// FUN_1017e0b0: kage_tex top-down, diameter size_b; diffuse 0x60 - 48 |dy|.
@vertex
fn vs_blob(@location(0) p: vec3<f32>, @builtin(instance_index) ii: u32) -> VOut {
    let o = 16u * inst[ii];
    let pos = douts[o + 4u].xyz;
    let sb = douts[o + 6u].x;
    let d = p - pos;
    var out: VOut;
    out.clip = to_clip(p);
    out.uv = d.xz / sb + 0.5;
    out.c = max(0.0, 96.0 - trunc(48.0 * abs(d.y))) / 255.0;
    out.layer = 0u;
    return out;
}

// FUN_1017ecf0: the silhouette layer rotated to the light's azimuth, size_s across; diffuse 0x40 - 32 |dy|.
@vertex
fn vs_ground(@location(0) p: vec3<f32>, @builtin(instance_index) ii: u32) -> VOut {
    let o = 16u * inst[ii];
    let pos = douts[o + 4u].xyz;
    let l = douts[o + 5u].xyz;
    let s = douts[o + 6u];
    let hl = max(length(l.xz), 1e-6);
    let d = p - pos;
    var out: VOut;
    out.clip = to_clip(p);
    out.uv = vec2<f32>(dot(d, vec3<f32>(l.z, 0.0, -l.x) / hl), dot(d, vec3<f32>(l.x, 0.0, l.z) / hl)) / s.y + 0.5;
    out.c = max(0.0, 64.0 - trunc(32.0 * abs(d.y))) / 255.0;
    out.layer = u32(s.z);
    return out;
}

// FUN_1017dd50: a flat square of side 2 size_b, 0.001 above the feet, diffuse 0x60.
@vertex
fn vs_quad(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let o = 16u * inst[ii];
    let pos = douts[o + 4u].xyz;
    let sb = douts[o + 6u].x;
    let x = select(-1.0, 1.0, (vi & 1u) != 0u);
    let z = select(1.0, -1.0, (vi & 2u) != 0u);
    var out: VOut;
    out.clip = to_clip(pos + vec3<f32>(x * sb, -0.001, z * sb));
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - z * 0.5);
    out.c = 96.0 / 255.0;
    out.layer = 0u;
    return out;
}

// MODULATE2X(texture, diffuse) into ZERO / INVSRCCOLOR.
@fragment
fn fs_blob(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(kage, samp, i.uv).rgb;
    return vec4<f32>(clamp(t * i.c * 2.0, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

@fragment
fn fs_ground(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(sil, samp, i.uv, i.layer).r;
    return vec4<f32>(vec3<f32>(clamp(t * i.c * 2.0, 0.0, 1.0)), 1.0);
}
"#;

pub struct Shadows {
    prep: wgpu::ComputePipeline,
    prep_layout: wgpu::BindGroupLayout,
    prep_group: Option<wgpu::BindGroup>,
    blob: wgpu::RenderPipeline,
    ground: wgpu::RenderPipeline,
    quad: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    draw_group: Option<wgpu::BindGroup>,
    params: wgpu::Buffer,
    casters: wgpu::Buffer,
    state: wgpu::Buffer,
    outs: wgpu::Buffer,
    inst: wgpu::Buffer,
    kage: Option<wgpu::BindGroup>,
    arr_layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    sil: Option<Sil>,
    sil_out: wgpu::BindGroup,
    /// The collision faces of each cell as drawn (x, z, without the normal test), their range in `verts`.
    cells: HashMap<(usize, usize, bool), std::ops::Range<u32>>,
    verts: Vec<[f32; 3]>,
    vbuf: Option<(wgpu::Buffer, usize)>,
}

/// The silhouette layers: their size, count, one view each to draw into, and the array to lay them down from.
struct Sil {
    size: [u32; 2],
    layers: u32,
    views: Vec<wgpu::TextureView>,
    group: wgpu::BindGroup,
}

/// Instances a frame (a caster can appear in nine cells).
const MAX_INST: usize = MAX_CASTERS * 10;

impl Shadows {
    fn new(gpu: &Gpu, globals: &wgpu::BindGroupLayout, sil_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadows"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let buf_entry = |binding, read_only, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let cs = wgpu::ShaderStages::COMPUTE;
        let prep_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow prep"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: cs, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                buf_entry(1, true, cs),
                buf_entry(2, true, cs),
                buf_entry(3, false, cs),
                buf_entry(4, false, cs),
            ],
        });
        let prep = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("shadow prep"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&prep_layout)], immediate_size: 0 })),
            module: &module,
            entry_point: Some("prep"),
            compilation_options: Default::default(),
            cache: None,
        });
        let vs = wgpu::ShaderStages::VERTEX;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow draw"), entries: &[buf_entry(0, true, vs), buf_entry(1, true, vs)] });
        let tex_entry = |dim| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dim, multisampled: false },
            count: None,
        };
        let samp_entry = wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow texture"), entries: &[tex_entry(wgpu::TextureViewDimension::D2), samp_entry] });
        let arr_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow silhouettes"), entries: &[tex_entry(wgpu::TextureViewDimension::D2Array), samp_entry] });
        let layout = |t: &wgpu::BindGroupLayout| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadows"), bind_group_layouts: &[Some(globals), Some(t), Some(&draw_layout)], immediate_size: 0 });
        // dst x (1 - src.rgb); the target's alpha kept.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::OneMinusSrc, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let pipe = |label, vs: &str, fs: &str, t: &wgpu::BindGroupLayout, faces: bool| {
            let vbuf = [Some(wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3] })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout(t)),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers: if faces { &vbuf } else { &[] } },
                primitive: wgpu::PrimitiveState { topology: if faces { wgpu::PrimitiveTopology::TriangleList } else { wgpu::PrimitiveTopology::TriangleStrip }, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blob = pipe("shadow blob", "vs_blob", "fs_blob", &tex_layout, true);
        let ground = pipe("shadow silhouette ground", "vs_ground", "fs_ground", &arr_layout, true);
        let quad = pipe("shadow quad", "vs_quad", "fs_blob", &tex_layout, false);
        let b = |label, size: usize, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let s = wgpu::BufferUsages::STORAGE;
        let params = b("shadow params", std::mem::size_of::<Params>(), wgpu::BufferUsages::UNIFORM);
        let casters = b("shadow casters", MAX_CASTERS * std::mem::size_of::<CasterGpu>(), s);
        let state = b("shadow light state", MAX_CASTERS * 32, s);
        let outs = b("shadow outputs", MAX_CASTERS * OUT_STRIDE as usize, s);
        let inst = b("shadow instances", MAX_INST * 4, s);
        let linear = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let sil_out = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow silhouette matrices"),
            layout: sil_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &outs, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        Self {
            prep,
            prep_layout,
            prep_group: None,
            blob,
            ground,
            quad,
            tex_layout,
            draw_layout,
            draw_group: None,
            params,
            casters,
            state,
            outs,
            inst,
            kage: None,
            arr_layout,
            linear,
            sil: None,
            sil_out,
            cells: HashMap::new(),
            verts: Vec::new(),
            vbuf: None,
        }
    }

    /// Silhouette layers of `size` for `n` casters (made anew when the size changes or more are needed), cleared black
    /// per use (their edge clamps to nothing) and sampled LINEAR / CLAMP.
    fn layers(&mut self, gpu: &Gpu, size: [u32; 2], n: u32) {
        if self.sil.as_ref().is_some_and(|l| l.size == size && l.layers >= n) {
            return;
        }
        let layers = n.next_power_of_two().min(max_layers(size)).max(n);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow silhouettes"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let views = (0..layers)
            .map(|k| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: k, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        let all = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow silhouettes"),
            layout: &self.arr_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&all) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.linear) }],
        });
        self.sil = Some(Sil { size, layers, views, group });
    }

    /// The faces of a caster's cells go up once each (x, z, without the normal test); true when any is new.
    fn cache(&mut self, ground: &crate::client::ground::Ground, cells: (usize, usize, usize, usize), any: bool) -> bool {
        let mut new = false;
        for z in cells.2..=cells.3 {
            for x in cells.0..=cells.1 {
                if let std::collections::hash_map::Entry::Vacant(e) = self.cells.entry((x, z, any)) {
                    let start = self.verts.len() as u32;
                    self.verts.extend(ground.cell_tris(x, z, any).into_iter().map(|v| v.to_array()));
                    e.insert(start..self.verts.len() as u32);
                    new = true;
                }
            }
        }
        new
    }
}

/// The silhouette layers a scene of `size` can have.
fn max_layers(size: [u32; 2]) -> u32 {
    (SIL_BUDGET / (size[0].max(1) as u64 * size[1].max(1) as u64)).clamp(1, SIL_LAYERS as u64) as u32
}

impl ZoneRenderer {
    /// The silhouettes a frame can draw at this scene size (client/shadows.rs turns the rest into blobs).
    pub fn shadow_layers(&self, size: [u32; 2]) -> u32 {
        max_layers(size)
    }

    /// "system  kage_tex" (file 0x1b8c), the blob's image; POINT / CLAMP.
    pub fn set_shadow_texture(&mut self, gpu: &Gpu, t: &crate::dat::zone::Texture, format: wgpu::TextureFormat) {
        let s = self.shadows.get_or_insert_with(|| Shadows::new(gpu, &self.globals_layout, &self.sil_layout, format));
        let view = super::upload_texture_seq(gpu, t, false).0;
        let point = gpu.device.create_sampler(&wgpu::SamplerDescriptor::default());
        s.kage = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kage_tex"),
            layout: &s.tex_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&point) }],
        }));
    }

    pub fn has_shadow_texture(&self) -> bool {
        self.shadows.as_ref().is_some_and(|s| s.kage.is_some())
    }

    /// The ground changed (a sub-area merged in): the cached faces go.
    pub fn reset_shadow_faces(&mut self) {
        if let Some(s) = &mut self.shadows {
            s.cells.clear();
            s.verts.clear();
        }
    }

    /// This frame's shadows over the scene drawn so far (`size` the scene's; `sun`: the directional light's travel
    /// direction, w 1 when there is one; `ticks`: 60 Hz ticks this frame).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shadows(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, size: [u32; 2], ground: &crate::client::ground::Ground, casters: &[Caster], sun: [f32; 4], ticks: f32) {
        let Some(s) = self.shadows.as_mut() else { return };
        if casters.is_empty() || s.kage.is_none() {
            return;
        }
        let casters = &casters[..casters.len().min(MAX_CASTERS)];
        // Faces.
        let mut grew = false;
        for c in casters {
            if let (Some(cells), 0 | 1) = (c.cells, c.mode) {
                grew |= s.cache(ground, cells, c.mode == 1);
            }
        }
        if grew || s.vbuf.is_none() {
            let bytes: &[u8] = bytemuck::cast_slice(&s.verts);
            if s.vbuf.as_ref().is_none_or(|(_, cap)| *cap < bytes.len().max(12)) {
                let cap = bytes.len().max(12).next_power_of_two();
                s.vbuf = Some((gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("shadow faces"), size: cap as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), cap));
            }
            if !bytes.is_empty() {
                gpu.queue.put(&s.vbuf.as_ref().unwrap().0, 0, bytes);
            }
        }
        // The casters and their light, size and matrix (prep).
        let input: Vec<CasterGpu> = casters
            .iter()
            .map(|c| CasterGpu {
                corners: c.corners.map(|v| [v.x, v.y, v.z, 1.0]),
                pos: [c.pos.x, c.pos.y, c.pos.z, c.mode as f32],
                info: [u32::from_le_bytes(c.lights), c.state, c.layer, c.fresh as u32],
            })
            .collect();
        gpu.queue.put(&s.casters, 0, bytemuck::cast_slice(&input));
        gpu.queue.put(&s.params, 0, bytemuck::bytes_of(&Params { sun, misc: [ticks, casters.len() as f32, 0.0, 0.0] }));
        let lights = &self.lights;
        let group = s.prep_group.get_or_insert_with(|| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow prep"),
                layout: &s.prep_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: s.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: lights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: s.casters.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: s.state.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: s.outs.as_entire_binding() },
                ],
            })
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("shadow prep"), timestamp_writes: None });
            pass.set_pipeline(&s.prep);
            pass.set_bind_group(0, &*group, &[]);
            pass.dispatch_workgroups((casters.len() as u32).div_ceil(64), 1, 1);
        }
        // Silhouettes: each mode-1 caster's actor flat white into its layer.
        let sils = casters.iter().filter(|c| c.mode == 1).count() as u32;
        if sils > 0 {
            s.layers(gpu, size, sils);
        }
        if let (Some((_, poses, _)), Some(sil)) = (&self.pose_gpu, s.sil.as_ref().filter(|_| sils > 0)) {
            for (i, c) in casters.iter().enumerate().filter(|(_, c)| c.mode == 1) {
                let (Some(a), Some(view)) = (c.actor, sil.views.get(c.layer as usize)) else { continue };
                let Some(skin) = a.skin.as_ref().filter(|k| k.frame == self.pose_frame) else { continue };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow silhouette"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.sil_pipeline);
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_bind_group(2, poses, &[skin.offset]);
                pass.set_bind_group(3, &s.sil_out, &[(i as u64 * OUT_STRIDE) as u32]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                for (tex, range, wep) in &a.draws {
                    if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                        continue;
                    }
                    pass.set_bind_group(1, &a.textures[*tex], &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
        // The instance lists: quads, then per cell the blobs and the silhouettes laid down.
        let mut inst: Vec<u32> = Vec::new();
        let quads: Vec<u32> = (0..casters.len() as u32).filter(|&i| casters[i as usize].mode == 2).collect();
        let quad_range = 0..quads.len() as u32;
        inst.extend(quads);
        let mut per_cell: HashMap<(usize, usize, bool), Vec<u32>> = HashMap::new();
        for (i, c) in casters.iter().enumerate() {
            let (Some(cells), 0 | 1) = (c.cells, c.mode) else { continue };
            for z in cells.2..=cells.3 {
                for x in cells.0..=cells.1 {
                    per_cell.entry((x, z, c.mode == 1)).or_default().push(i as u32);
                }
            }
        }
        let mut draws: Vec<(bool, std::ops::Range<u32>, std::ops::Range<u32>)> = Vec::new();
        for (key, list) in per_cell {
            let Some(r) = s.cells.get(&key).filter(|r| !r.is_empty()) else { continue };
            let at = inst.len() as u32;
            inst.extend(list);
            draws.push((key.2, r.clone(), at..inst.len() as u32));
        }
        if inst.is_empty() {
            return;
        }
        inst.truncate(MAX_INST);
        gpu.queue.put(&s.inst, 0, bytemuck::cast_slice(&inst));
        let dg = s.draw_group.get_or_insert_with(|| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow draw"),
                layout: &s.draw_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: s.outs.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: s.inst.as_entire_binding() }],
            })
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadows"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_bind_group(2, &*dg, &[]);
        let kage = s.kage.as_ref().unwrap();
        if !quad_range.is_empty() {
            pass.set_pipeline(&s.quad);
            pass.set_bind_group(1, kage, &[]);
            pass.draw(0..4, quad_range);
        }
        let Some((vb, _)) = &s.vbuf else { return };
        pass.set_vertex_buffer(0, vb.slice(..));
        for (sil, verts, range) in draws.into_iter().filter(|d| d.2.end as usize <= MAX_INST) {
            if sil {
                let Some(l) = s.sil.as_ref() else { continue };
                pass.set_pipeline(&s.ground);
                pass.set_bind_group(1, &l.group, &[]);
            } else {
                pass.set_pipeline(&s.blob);
                pass.set_bind_group(1, kage, &[]);
            }
            pass.draw(verts, range);
        }
    }
}
