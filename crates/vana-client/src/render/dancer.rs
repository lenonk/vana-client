//! Dancer (creator preview) shading: the sqsd shaders' passes and per-vertex texgen, done on the CPU
//! as FFXiMain does, then drawn with the matching D3D8 blend states (gamma-space, Rgba8Unorm target).
//! Generic FUN_10286450, Sampling FUN_1028e980, Aniso FUN_1028eca0; texgen dispatch FUN_102808d0.

use super::{Gpu, depth_view};
use crate::dat::dancer::{Model, Skinned};
use crate::dat::zone::Texture;
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const BAD: u32 = 0xEFBE_DDBA;

/// Directional light: unit vector towards the light and grey intensity.
#[derive(Clone, Copy)]
pub struct Light {
    pub dir: Vec3,
    pub color: f32,
    /// Light node rotation; columns = node x, y, z (z = dir). Light 0's frame drives the phong texgen.
    pub frame: Mat3,
}

/// D3D8 blend setups used by the shaders (stage-0 path of FUN_10277fb0..FUN_10278a00).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Blend {
    /// No blending, all channels written (dest alpha keeps the texture alpha).
    Opaque,
    /// SRCALPHA / INVSRCALPHA (FUN_10278480).
    Alpha,
    /// ONE / ONE (FUN_10277fb0, FUN_102780c0).
    Add,
    /// DESTALPHA / ONE (FUN_102786b0).
    DstAlphaAdd,
    /// Blending off, COLORWRITEENABLE = alpha only (phong mask pass).
    AlphaOnly,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct DVertex {
    pub pos: [f32; 3],
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    /// D3DCOLOR-style 8-bit RGBA.
    pub col: [u8; 4],
}

fn col8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
}

pub struct DDraw {
    /// One vertex per shape vertex, drawn through `indices` (the shape's triangle corners, shared by its passes).
    pub verts: Vec<DVertex>,
    pub indices: std::sync::Arc<Vec<u32>>,
    pub tex0: Option<String>,
    pub tex1: Option<String>,
    pub blend: Blend,
    pub zwrite: bool,
    /// Stage 0: 0 = texture x vertex colour (alpha = texture), 1 = texture only, 2 = coverage (alpha 1).
    pub op0: u32,
    /// Stage 1: 0 = off, 1 = current + vertex alpha x texture1 (MULTIPLYADD, FUN_102780c0 stage path).
    pub op1: u32,
    /// Alpha test reference (GREATEREQUAL), negative = off.
    pub aref: f32,
}

fn wf(w: &[u32], i: usize) -> f32 {
    f32::from_bits(w[i])
}
fn wt(w: &[u32], i: usize) -> Option<usize> {
    (w[i] != BAD).then_some(w[i] as usize)
}

/// D3D fixed-function lighting (FUN_10278d10 material, FUN_10279d00 lights): emissive = colour x ambient,
/// diffuse = kd x colour (or the vertex colour), no material ambient, no specular; clamped to [0, 1].
fn d3d_light(n: Vec3, emissive: Vec3, diffuse: Vec3, lights: &[Light]) -> Vec3 {
    let mut c = emissive;
    for l in lights {
        c += diffuse * l.color * n.dot(l.dir).max(0.0);
    }
    c.clamp(Vec3::ZERO, Vec3::ONE)
}

/// Reflection of the eye ray in model space: v = normalize(P - eye), r = v - 2(v.n)n.
fn reflect(p: Vec3, n: Vec3, eye: Vec3) -> Vec3 {
    let v = (p - eye).normalize_or_zero();
    v - 2.0 * v.dot(n) * n
}

/// Env (sphere) map texgen FUN_10291f00: uv = (r.xy + 1) / 2, no matrix.
fn env_uv(p: Vec3, n: Vec3, eye: Vec3) -> [f32; 2] {
    let r = reflect(p, n, eye);
    [(r.x + 1.0) * 0.5, (r.y + 1.0) * 0.5]
}

/// Phong texgen FUN_10292240: r in light 0's frame (matrix 0x248), uv = (q.xy * k + 1) / 2 with
/// k = (1 - phongSize) + 0.3; colour and alpha = 1 when q.z >= 0 else 0.
fn phong_uv(p: Vec3, n: Vec3, eye: Vec3, l0: &Light, size: f32) -> ([f32; 2], f32) {
    let q = l0.frame.transpose() * reflect(p, n, eye);
    let k = (1.0 - size) + 0.3;
    ([(q.x * k + 1.0) * 0.5, (q.y * k + 1.0) * 0.5], if q.z < 0.0 { 0.0 } else { 1.0 })
}

/// Builds the passes of every shape of one model (file order).
/// `fwd` is the camera's viewing direction in model space (sort key of "_sort" shapes).
pub fn build_draws(m: &Model, sk: &[Skinned], eye: Vec3, fwd: Vec3, lights: &[Light], out: &mut Vec<DDraw>) {
    let has_alpha = |t: Option<usize>| t.and_then(|i| m.textures.get(i)).is_some_and(|t| t.format == 1);
    let tname = |t: Option<usize>| t.and_then(|i| m.textures.get(i)).map(|t| t.name.clone());
    for (s, k) in m.shapes.iter().zip(sk) {
        let Some(sh) = m.shaders.get(s.shader) else { continue };
        let w = &sh.words;
        // "_sort" shapes: each listed-triangle run in descending (P0 + P1 + P2 - 3 eye) . fwd, i.e. back to front.
        let mut tris = s.tris.clone();
        if s.sorted {
            let key = |t: &[u32; 3]| (k.pos[t[0] as usize] + k.pos[t[1] as usize] + k.pos[t[2] as usize] - 3.0 * eye).dot(fwd);
            for &(a, n) in &s.list_runs {
                tris[a..a + n].sort_by(|x, y| key(y).total_cmp(&key(x)));
            }
        }
        let indices = std::sync::Arc::new(tris.iter().flatten().copied().collect::<Vec<u32>>());
        let n = k.pos.len();
        let vert = |i: usize, uv1: [f32; 2], col: [f32; 4]| DVertex { pos: k.pos[i].to_array(), uv0: s.uv[i], uv1, col: col8(col) };
        let push = |out: &mut Vec<DDraw>, verts, tex0, tex1, blend, zwrite, op0, op1, aref| {
            out.push(DDraw { verts, indices: indices.clone(), tex0, tex1, blend, zwrite, op0, op1, aref })
        };
        let col = Vec3::new(wf(w, 0xa), wf(w, 0xb), wf(w, 0xc));
        let (amb, kd) = (wf(w, 0xd), wf(w, 0xe));
        match sh.kind {
            0xF0001 => {
                if w[0x23] == 0 || wf(w, 0x11) >= 0.999 {
                    continue;
                }
                let uam = w[0x28];
                let alpha = 1.0 - wf(w, 0x11);
                let lit: Vec<[f32; 4]> = (0..k.pos.len())
                    .map(|i| {
                        let c = if w[0x26] != 0 { d3d_light(k.normal[i], col * amb, col * kd, lights) } else { Vec3::ONE };
                        [c.x, c.y, c.z, alpha]
                    })
                    .collect();
                let ct = wt(w, 0x13);
                let blend = if has_alpha(ct) && uam & 1 != 0 { Blend::Alpha } else { Blend::Opaque };
                push(out, (0..n).map(|i| vert(i, [0.0; 2], lit[i])).collect(), tname(ct), None, blend, true, 0, 0, -1.0);
                let mut op = 0;
                if let Some(env) = wt(w, 0x14) {
                    let b = if uam & 4 != 0 { Blend::DstAlphaAdd } else { Blend::Add };
                    let v = (0..n).map(|i| DVertex { uv0: env_uv(k.pos[i], k.normal[i], eye), ..vert(i, [0.0; 2], [1.0; 4]) }).collect();
                    push(out, v, tname(Some(env)), None, b, true, 1, 0, -1.0);
                    op = 1;
                }
                let mask = wt(w, 0x17);
                if mask.is_some() {
                    push(out, (0..n).map(|i| vert(i, [0.0; 2], [1.0; 4])).collect(), tname(mask), None, Blend::AlphaOnly, true, 1, 0, -1.0);
                    op = 1;
                }
                if let Some(ph) = wt(w, 0x16) {
                    let b = if uam & 2 == 0 && mask.is_none() { Blend::Add } else { Blend::DstAlphaAdd };
                    let v = (0..n)
                        .map(|i| {
                            let (uv, h) = phong_uv(k.pos[i], k.normal[i], eye, &lights[0], wf(w, 0x20));
                            DVertex { uv0: uv, ..vert(i, [0.0; 2], [h; 4]) }
                        })
                        .collect();
                    push(out, v, tname(Some(ph)), None, b, false, op, 0, -1.0);
                }
            }
            0xF0002 => {
                if w[0x13] & 6 == 6 {
                    continue;
                }
                let uam = w[0x59];
                // Ramp lighting FUN_10291640: x = 15/(back + 1.0001) * (N.L + back), lerp ramp[floor x], ramp[+1].
                let back = wf(w, 0x16);
                let sc = 15.0 / (back + 1.0001);
                let ramp = |j: usize| Vec3::new(wf(w, 0x17 + 4 * j), wf(w, 0x18 + 4 * j), wf(w, 0x19 + 4 * j));
                let lit: Vec<[f32; 4]> = (0..k.pos.len())
                    .map(|i| {
                        let mut c = Vec3::ZERO;
                        for l in lights {
                            let x = (sc * k.normal[i].dot(l.dir) + sc * back).max(0.0);
                            let j = (x as usize).min(14);
                            let f = x - j as f32;
                            c += (ramp(j) * (1.0 - f) + ramp(j + 1) * f) * l.color;
                        }
                        let c = c.clamp(Vec3::ZERO, Vec3::ONE);
                        [c.x, c.y, c.z, 1.0]
                    })
                    .collect();
                let ct = wt(w, 0x14);
                let blend = if has_alpha(ct) && uam & 1 != 0 { Blend::Alpha } else { Blend::Opaque };
                push(out, (0..n).map(|i| vert(i, [0.0; 2], lit[i])).collect(), tname(ct), None, blend, true, 0, 0, -1.0);
                if let Some(ph) = wt(w, 0x15).filter(|_| wf(w, 0x58) > 0.001) {
                    let b = if uam & 2 != 0 { Blend::DstAlphaAdd } else { Blend::Add };
                    let v = (0..n)
                        .map(|i| {
                            let (uv, h) = phong_uv(k.pos[i], k.normal[i], eye, &lights[0], wf(w, 0x57));
                            DVertex { uv0: uv, ..vert(i, [0.0; 2], [h; 4]) }
                        })
                        .collect();
                    push(out, v, tname(Some(ph)), None, b, false, 0, 0, -1.0);
                }
            }
            0xF0005 => {
                if w[0x14] == 0 || w[0x13] & 6 == 6 {
                    continue;
                }
                // One pass, two stages: tex0 x lit + (N.L0) x aniso(u, V.T - L0.T); vertex colour is the
                // diffuse material source (kd x colour, alpha N.L0), FUN_10291d70.
                let mode = w[0x16];
                let by_v = mode != 1; // FUN_10291d50: modes 0/2 use dP/dv and uv.u, mode 1 dP/du and uv.v
                let l0 = lights[0].dir;
                let ct = wt(w, 0x18);
                let v = (0..n)
                    .map(|i| {
                        let (p, n) = (k.pos[i], k.normal[i]);
                        let t = if by_v { k.binormal[i] } else { k.tangent[i] };
                        let vv = (p - eye).normalize_or_zero();
                        let uv1 = [if by_v { s.uv[i][0] } else { s.uv[i][1] }, vv.dot(t) - l0.dot(t)];
                        let c = d3d_light(n, col * amb, col * kd, lights);
                        vert(i, uv1, [c.x, c.y, c.z, n.dot(l0).max(0.0)])
                    })
                    .collect();
                let blend = if has_alpha(ct) { Blend::Alpha } else { Blend::Opaque };
                let aref = if w[0x15] != 0 { 0x5f as f32 / 255.0 } else { -1.0 };
                push(out, v, tname(ct), tname(wt(w, 0x19)), blend, true, 0, 1, aref);
            }
            _ => {}
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DUniforms {
    view_proj: [[f32; 4]; 4],
    /// op0, op1, alpha ref, has texture 0 (shadow: the multiplier)
    op: [f32; 4],
    /// Shadow light direction (towards the light), dancer world.
    light: [f32; 4],
}

const WGSL: &str = r#"
struct U { view_proj: mat4x4<f32>, op: vec4<f32>, light: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
struct VIn { @location(0) pos: vec3<f32>, @location(1) uv0: vec2<f32>, @location(2) uv1: vec2<f32>, @location(3) col: vec4<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv0: vec2<f32>, @location(1) uv1: vec2<f32>, @location(2) col: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv0 = v.uv0; o.uv1 = v.uv1; o.col = v.col;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    var c0 = textureSample(t0, samp, i.uv0);
    let c1 = textureSample(t1, samp, i.uv1);
    if (u.op.w < 0.5) { c0 = vec4<f32>(1.0); }
    var o = vec4<f32>(c0.rgb * i.col.rgb, c0.a);
    if (u.op.x > 1.5) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    if (u.op.x > 0.5) { o = c0; }
    if (u.op.y > 0.5) { o = vec4<f32>(min(o.rgb + i.col.a * c1.rgb, vec3<f32>(1.0)), o.a); }
    if (u.op.z >= 0.0 && o.a < u.op.z) { discard; }
    return o;
}
// Planar shadow (FUN_10297e20): untextured, colour = 1 - c multiplied into the target. Depth is pinned near
// the far plane so a pixel is darkened once (the code uses stencil EQUAL 0 / INCR for that).
@vertex fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    // FUN_1028dc40 with the plane y = 0: p' = (x - y Lx/Ly, 0, z - y Lz/Ly).
    let l = u.light.xyz;
    let p = vec3<f32>(v.pos.x - v.pos.y * l.x / l.y, 0.0, v.pos.z - v.pos.y * l.z / l.y);
    var c = u.view_proj * vec4<f32>(p, 1.0);
    c.z = c.w * 0.99999;
    return c;
}
@fragment fn fs_shadow() -> @location(0) vec4<f32> {
    return vec4<f32>(u.op.xyz, 1.0);
}
"#;

fn blend_state(b: Blend) -> (Option<wgpu::BlendState>, wgpu::ColorWrites) {
    use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
    let comp = |s, d| C { src_factor: s, dst_factor: d, operation: O::Add };
    let st = |s, d| Some(wgpu::BlendState { color: comp(s, d), alpha: comp(s, d) });
    match b {
        Blend::Opaque => (None, wgpu::ColorWrites::ALL),
        Blend::Alpha => (st(F::SrcAlpha, F::OneMinusSrcAlpha), wgpu::ColorWrites::ALL),
        Blend::Add => (st(F::One, F::One), wgpu::ColorWrites::ALL),
        Blend::DstAlphaAdd => (st(F::DstAlpha, F::One), wgpu::ColorWrites::ALL),
        Blend::AlphaOnly => (None, wgpu::ColorWrites::ALPHA),
    }
}

/// The creator's ground shadow (FUN_1028dc40 / FUN_10297e20): every caster shape flattened onto y = 0
/// along  (dancer world, towards the light), multiplying the target by  = 1 - c.
#[derive(Clone, Copy)]
pub struct Shadow {
    pub light: Vec3,
    pub mult: [f32; 3],
}

/// Shadow light 4 (FUN_10245bc0: rotation (0, pi/2, 1.25664), not camera-attached): 72 degrees up toward +x.
pub const SHADOW_LIGHT: Vec3 = Vec3::new(0.30902, 0.95106, 0.0);

/// What the dancer draws over: it clears only depth (FUN_10245ab0), so the lobby scene shows through.
pub enum Background {
    Color([f64; 3]),
    /// RGBA8 image of the output size (e.g. the zone-133 backdrop).
    Image(Vec<u8>),
    /// Alpha 0 outside the model (a coverage pass sets alpha 1 on the model afterwards).
    Transparent,
}

/// The dancer's D3D8 passes on the GPU: pipelines per (blend, z-write), textures by name.
pub struct DancerRenderer {
    shader: wgpu::ShaderModule,
    ubl: wgpu::BindGroupLayout,
    tbl: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    pipelines: HashMap<(Blend, bool), wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
    views: HashMap<String, wgpu::TextureView>,
    white: wgpu::TextureView,
    tex_groups: HashMap<TexKey, wgpu::BindGroup>,
    prepared: Option<Prepared>,
    shadow_pipe: Option<wgpu::RenderPipeline>,
    belt: Option<wgpu::util::StagingBelt>,
    /// Draw the planar shadow before the model (set by the creator).
    pub shadow: Option<Shadow>,
    /// The GPU-skinned path (DancerGpu): storage layout, shader and layout, pipelines.
    sbl: Option<wgpu::BindGroupLayout>,
    gpu_mod: Option<(wgpu::ShaderModule, wgpu::PipelineLayout)>,
    gpu_pipes: HashMap<(Blend, bool), wgpu::RenderPipeline>,
    gpu_shadow: Option<wgpu::RenderPipeline>,
}

impl DancerRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("dancer"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let ubl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
                count: None,
            }],
        });
        let tex_entry = |b| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let tbl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[tex_entry(0), tex_entry(1), wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&ubl), Some(&tbl)], immediate_size: 0 });
        let sampler = super::mip_sampler(device);
        let white = Self::make(gpu, 1, 1, &[255; 4]);
        Self {
            shader,
            ubl,
            tbl,
            layout,
            format,
            pipelines: HashMap::new(),
            sampler,
            views: HashMap::new(),
            white,
            tex_groups: HashMap::new(),
            prepared: None,
            shadow_pipe: None,
            belt: None,
            shadow: None,
            sbl: None,
            gpu_mod: None,
            gpu_pipes: HashMap::new(),
            gpu_shadow: None,
        }
    }

    fn make(gpu: &Gpu, w: u32, h: u32, rgba: &[u8]) -> wgpu::TextureView {
        gpu.device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                Default::default(),
                rgba,
            )
            .create_view(&Default::default())
    }

    /// Uploads the models' textures with their mips (replacing any of the same name). The dancer works in gamma
    /// space, so these stay Rgba8Unorm.
    pub fn load_textures(&mut self, gpu: &Gpu, textures: &[Texture]) {
        for t in textures {
            self.views.insert(t.name.clone(), super::upload_texture(gpu, t, false));
        }
        gpu.flush_mips();
        self.tex_groups.clear();
    }

    fn tex_group(&mut self, gpu: &Gpu, key: &TexKey) {
        if self.tex_groups.contains_key(key) {
            return;
        }
        let t0 = key.0.as_ref().and_then(|n| self.views.get(n)).unwrap_or(&self.white);
        let t1 = key.1.as_ref().and_then(|n| self.views.get(n)).unwrap_or(&self.white);
        let tb = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tbl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(t0) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(t1) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.tex_groups.insert(key.clone(), tb);
    }

    /// Shadow pass state: no cull, colour = dest x src (ZERO / SRCCOLOR, FUN_10278a00), no Z test in the code.
    fn shadow_pipeline(&mut self, gpu: &Gpu) -> wgpu::RenderPipeline {
        let (shader, layout, format) = (&self.shader, &self.layout, self.format);
        self.shadow_pipe
            .get_or_insert_with(|| {
                use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
                let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Unorm8x4];
                let blend = wgpu::BlendState { color: C { src_factor: F::Zero, dst_factor: F::Src, operation: O::Add }, alpha: C { src_factor: F::Zero, dst_factor: F::One, operation: O::Add } };
                gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("dancer shadow"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs_shadow"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<DVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                    },
                    primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: super::DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs_shadow"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })] }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone()
    }

    fn pipeline(&mut self, gpu: &Gpu, b: Blend, zwrite: bool) -> wgpu::RenderPipeline {
        let (shader, layout, format) = (&self.shader, &self.layout, self.format);
        self.pipelines
            .entry((b, zwrite))
            .or_insert_with(|| {
                let (blend, write_mask) = blend_state(b);
                let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Unorm8x4];
                gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("dancer"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<DVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                    },
                    // cull = 1 on every creator shader: no face culling (FUN_1027a410 CULLMODE).
                    primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: super::DEPTH_FORMAT,
                        depth_write_enabled: Some(zwrite),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask })] }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone()
    }

    /// Draws over what is in `view`, clearing only depth as the dancer does (FUN_10245ab0).
    /// `coverage` adds the alpha-only pass that marks the model for a transparent background.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, depth: &wgpu::TextureView, view_proj: Mat4, draws: &[DDraw], coverage: bool) {
        self.prepare(gpu, encoder, view_proj, draws, coverage);
        self.draw_prepared(encoder, view, depth);
    }

    /// Uploads one pose's passes: all vertices in one buffer, uniforms in one buffer at dynamic offsets,
    /// texture bind groups cached by name. `draw_prepared` then replays them until the next prepare.
    pub fn prepare(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, view_proj: Mat4, draws: &[DDraw], coverage: bool) {
        let mut list: Vec<(&DDraw, u32, Blend, bool)> = draws.iter().map(|d| (d, d.op0, d.blend, d.zwrite)).collect();
        if coverage {
            list.extend(draws.iter().filter(|d| d.op0 == 0 && d.zwrite).map(|d| (d, 2, Blend::AlphaOnly, false)));
        }
        list.retain(|(d, ..)| !d.verts.is_empty());
        let device = &gpu.device;
        let mut verts: Vec<DVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut ubytes: Vec<u8> = Vec::new();
        let mut calls = Vec::with_capacity(list.len());
        let mut shadow_calls = Vec::new();
        let mut last_idx: Option<(&std::sync::Arc<Vec<u32>>, std::ops::Range<u32>)> = None;
        let uniform = |ubytes: &mut Vec<u8>, op: [f32; 4], light: [f32; 4]| {
            let off = ubytes.len() as u32;
            ubytes.extend_from_slice(bytemuck::bytes_of(&DUniforms { view_proj: view_proj.to_cols_array_2d(), op, light }));
            ubytes.resize(ubytes.len().next_multiple_of(UNIFORM_STRIDE), 0);
            off
        };
        let shadow_u = self.shadow.map(|sh| uniform(&mut ubytes, [sh.mult[0], sh.mult[1], sh.mult[2], 0.0], [sh.light.x, sh.light.y, sh.light.z, 0.0]));
        for (d, op0, blend, zwrite) in list {
            let key = (d.tex0.clone().filter(|n| self.views.contains_key(n)), d.tex1.clone().filter(|n| self.views.contains_key(n)));
            let uoff = uniform(&mut ubytes, [op0 as f32, d.op1 as f32, d.aref, if key.0.is_some() { 1.0 } else { 0.0 }], [0.0; 4]);
            self.tex_group(gpu, &key);
            // Passes of one shape share its index list.
            let idx = match &last_idx {
                Some((a, r)) if std::sync::Arc::ptr_eq(a, &d.indices) => r.clone(),
                _ => {
                    let r = indices.len() as u32..(indices.len() + d.indices.len()) as u32;
                    indices.extend_from_slice(&d.indices);
                    last_idx = Some((&d.indices, r.clone()));
                    r
                }
            };
            let base = verts.len() as i32;
            verts.extend_from_slice(&d.verts);
            // The shadow flattens every caster shape once (its first pass, FUN_10297e20).
            if let Some(su) = shadow_u
                && zwrite
                && op0 == 0
                && blend != Blend::AlphaOnly
            {
                let none: TexKey = (None, None);
                self.tex_group(gpu, &none);
                shadow_calls.push(Call { pipe: self.shadow_pipeline(gpu), key: none, uoff: su, idx: idx.clone(), base });
            }
            calls.push(Call { pipe: self.pipeline(gpu, blend, zwrite), key, uoff, idx, base });
        }
        shadow_calls.append(&mut calls);
        let calls = shadow_calls;
        if calls.is_empty() {
            if let Some(p) = &mut self.prepared {
                p.calls.clear();
            }
            return;
        }
        // Buffers persist and grow; a staging belt keeps its upload memory mapped (fresh staging buffers from
        // mapped_at_creation / queue.write_buffer cost ~16 ms per 2.5 MB pose here).
        let vbytes: &[u8] = bytemuck::cast_slice(&verts);
        let ibytes: &[u8] = bytemuck::cast_slice(&indices);
        let old = self.prepared.take().filter(|p| p.vb.size() >= vbytes.len() as u64 && p.ib.size() >= ibytes.len() as u64 && p.ub.size() >= ubytes.len() as u64);
        let (vb, ib, ub, ug) = match old {
            Some(p) => (p.vb, p.ib, p.ub, p.ug),
            None => {
                let buf = |label, len: usize, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (len.next_power_of_two().max(256)) as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                let vb = buf("dancer verts", vbytes.len(), wgpu::BufferUsages::VERTEX);
                let ib = buf("dancer indices", ibytes.len(), wgpu::BufferUsages::INDEX);
                let ub = buf("dancer uniforms", ubytes.len(), wgpu::BufferUsages::UNIFORM);
                let size = std::num::NonZeroU64::new(std::mem::size_of::<DUniforms>() as u64);
                let ug = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.ubl,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &ub, offset: 0, size }) }],
                });
                (vb, ib, ub, ug)
            }
        };
        let belt = self.belt.get_or_insert_with(|| wgpu::util::StagingBelt::new(gpu.device.clone(), 4 << 20));
        for (buf, bytes) in [(&vb, vbytes), (&ib, ibytes), (&ub, &ubytes[..])] {
            if let Some(n) = wgpu::BufferSize::new(bytes.len().next_multiple_of(4) as u64) {
                let mut view = belt.write_buffer(encoder, buf, 0, n);
                view.slice(..bytes.len()).copy_from_slice(bytes);
            }
        }
        belt.finish_and_recall_on_submit(encoder);
        self.prepared = Some(Prepared { vb, ib, ub, ug, calls });
    }

    /// Replays the last `prepare` over `view`, clearing depth only.
    pub fn draw_prepared(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, depth: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dancer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(p) = &self.prepared else { return };
        pass.set_vertex_buffer(0, p.vb.slice(..));
        pass.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
        for c in &p.calls {
            let Some(tg) = self.tex_groups.get(&c.key) else { continue };
            pass.set_pipeline(&c.pipe);
            pass.set_bind_group(0, &p.ug, &[c.uoff]);
            pass.set_bind_group(1, tg, &[]);
            pass.draw_indexed(c.idx.clone(), c.base, 0..1);
        }
    }
}

/// Uniform slots are 256 bytes apart (minUniformBufferOffsetAlignment).
const UNIFORM_STRIDE: usize = 256;

type TexKey = (Option<String>, Option<String>);

struct Call {
    pipe: wgpu::RenderPipeline,
    key: TexKey,
    uoff: u32,
    idx: std::ops::Range<u32>,
    base: i32,
}

struct Prepared {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    ub: wgpu::Buffer,
    ug: wgpu::BindGroup,
    calls: Vec<Call>,
}

/// Renders the draws to an RGBA PNG.
pub fn render_png(textures: &[Texture], draws: &[DDraw], view_proj: Mat4, bg: &Background, width: u32, height: u32, path: &std::path::Path) -> Result<()> {
    let gpu = Gpu::headless()?;
    let device = &gpu.device;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = DancerRenderer::new(&gpu, format);
    r.load_textures(&gpu, textures);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    if let Background::Image(rgba) = bg {
        anyhow::ensure!(rgba.len() == (width * height * 4) as usize, "backdrop size does not match the output");
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }
    let view = target.create_view(&Default::default());
    let depth = depth_view(device, width, height);
    let mut enc = device.create_command_encoder(&Default::default());
    let clear = match bg {
        Background::Color(c) => Some(wgpu::Color { r: c[0], g: c[1], b: c[2], a: 1.0 }),
        Background::Transparent => Some(wgpu::Color::TRANSPARENT),
        Background::Image(_) => None,
    };
    if let Some(c) = clear {
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dancer clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(c), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    r.draw(&gpu, &mut enc, &view, &depth, view_proj, draws, matches!(bg, Background::Transparent));
    let row = (width * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
    let data = readback.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("mapping readback: {e:?}"))?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        pixels.extend_from_slice(&data[(y * row) as usize..(y * row + width * 4) as usize]);
    }
    // The target alpha holds shader masks; the PNG is opaque unless the background is transparent.
    if !matches!(bg, Background::Transparent) {
        pixels.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    }
    let mut pe = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), width, height);
    pe.set_color(png::ColorType::Rgba);
    pe.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

// ---- The dancer on the GPU: bind-pose shapes go up once, each frame only the joint matrices and the pass
// uniforms (camera, lights). Skinning, the per-vertex lighting and the texgen of build_draws run in vs_skin;
// the passes and their states are the same. "_sort" shapes keep their CPU back-to-front order (30 Hz).

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 3],
    binormal: [f32; 3],
    uv: [f32; 2],
    /// First influence and count in the influence buffer (0 = not skinned: the bind pose as is).
    inf: [u32; 2],
}

/// One call's uniform: the D3D8 pass of build_draws, by kind (0 generic lit, 1 env, 2 white mask, 3 phong,
/// 4 ramp, 5 aniso) and its shader words, plus the frame's camera and lights.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GUniforms {
    view_proj: [[f32; 4]; 4],
    op: [f32; 4],
    light: [f32; 4],
    /// kind, first joint, body scale, colour alpha
    kind: [f32; 4],
    root: [f32; 4],
    /// colour rgb, ambient
    p0: [f32; 4],
    /// kd, lit (word 0x26), phong size, by_v (aniso)
    p1: [f32; 4],
    /// ramp back, ramp scale
    p2: [f32; 4],
    eye: [f32; 4],
    /// Directions towards the lights, w = colour.
    ldir: [[f32; 4]; 4],
    /// Light 0's frame columns.
    lframe: [[f32; 4]; 3],
    ramp: [[f32; 4]; 16],
}

const GSTRIDE: usize = 1024;

const GWGSL: &str = r#"
struct U {
    view_proj: mat4x4<f32>, op: vec4<f32>, light: vec4<f32>, kind: vec4<f32>, root: vec4<f32>,
    p0: vec4<f32>, p1: vec4<f32>, p2: vec4<f32>, eye: vec4<f32>,
    ldir: array<vec4<f32>, 4>, lframe: array<vec4<f32>, 3>, ramp: array<vec4<f32>, 16>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(2) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;
@group(2) @binding(1) var<storage, read> infl: array<vec2<u32>>;
struct SIn { @location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) tangent: vec3<f32>, @location(3) binormal: vec3<f32>, @location(4) uv: vec2<f32>, @location(5) inf: vec2<u32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv0: vec2<f32>, @location(1) uv1: vec2<f32>, @location(2) col: vec4<f32> };
struct Sk { p: vec3<f32>, n: vec3<f32>, t: vec3<f32>, b: vec3<f32> };

fn nz(v: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    return select(vec3<f32>(0.0), v / l, l > 0.0);
}

// skin_shapes_with: weighted sum of (cur * bind_inv) per joint; the body scales about its root.
fn skin(v: SIn) -> Sk {
    var s: Sk;
    if (v.inf.y == 0u) {
        s.p = v.pos; s.n = v.normal; s.t = v.tangent; s.b = v.binormal;
    } else {
        s.p = vec3<f32>(0.0); s.n = vec3<f32>(0.0); s.t = vec3<f32>(0.0); s.b = vec3<f32>(0.0);
        let base = u32(u.kind.y);
        for (var i = 0u; i < v.inf.y; i = i + 1u) {
            let e = infl[v.inf.x + i];
            let m = joints[base + e.x];
            let w = bitcast<f32>(e.y);
            s.p += (m * vec4<f32>(v.pos, 1.0)).xyz * w;
            s.n += (m * vec4<f32>(v.normal, 0.0)).xyz * w;
            s.t += (m * vec4<f32>(v.tangent, 0.0)).xyz * w;
            s.b += (m * vec4<f32>(v.binormal, 0.0)).xyz * w;
        }
        s.n = nz(s.n); s.t = nz(s.t); s.b = nz(s.b);
    }
    s.p = u.root.xyz + (s.p - u.root.xyz) * u.kind.z;
    return s;
}

fn d3d_light(n: vec3<f32>, em: vec3<f32>, diff: vec3<f32>) -> vec3<f32> {
    var c = em;
    for (var i = 0u; i < 4u; i = i + 1u) {
        c += diff * u.ldir[i].w * max(dot(n, u.ldir[i].xyz), 0.0);
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn refl(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let v = nz(p - u.eye.xyz);
    return v - 2.0 * dot(v, n) * n;
}

// D3DCOLOR: 8 bits a channel.
fn q8(c: vec4<f32>) -> vec4<f32> {
    return round(clamp(c, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0) / 255.0;
}

@vertex fn vs_skin(v: SIn) -> VOut {
    let s = skin(v);
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(s.p, 1.0);
    o.uv0 = v.uv;
    o.uv1 = vec2<f32>(0.0);
    var c = vec4<f32>(1.0);
    let kind = u32(u.kind.x + 0.5);
    let col = u.p0.xyz;
    if (kind == 0u) {
        var rgb = vec3<f32>(1.0);
        if (u.p1.y > 0.5) { rgb = d3d_light(s.n, col * u.p0.w, col * u.p1.x); }
        c = vec4<f32>(rgb, u.kind.w);
    } else if (kind == 1u) {
        let r = refl(s.p, s.n);
        o.uv0 = vec2<f32>((r.x + 1.0) * 0.5, (r.y + 1.0) * 0.5);
    } else if (kind == 3u) {
        let r = refl(s.p, s.n);
        let q = vec3<f32>(dot(u.lframe[0].xyz, r), dot(u.lframe[1].xyz, r), dot(u.lframe[2].xyz, r));
        let k = (1.0 - u.p1.z) + 0.3;
        o.uv0 = vec2<f32>((q.x * k + 1.0) * 0.5, (q.y * k + 1.0) * 0.5);
        c = vec4<f32>(select(1.0, 0.0, q.z < 0.0));
    } else if (kind == 4u) {
        var rgb = vec3<f32>(0.0);
        for (var i = 0u; i < 4u; i = i + 1u) {
            let x = max(u.p2.y * dot(s.n, u.ldir[i].xyz) + u.p2.y * u.p2.x, 0.0);
            let j = min(u32(x), 14u);
            let f = x - f32(j);
            rgb += (u.ramp[j].xyz * (1.0 - f) + u.ramp[j + 1u].xyz * f) * u.ldir[i].w;
        }
        c = vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
    } else if (kind == 5u) {
        let by_v = u.p1.w > 0.5;
        let l0 = u.ldir[0].xyz;
        let t = select(s.t, s.b, by_v);
        let vv = nz(s.p - u.eye.xyz);
        o.uv1 = vec2<f32>(select(v.uv.y, v.uv.x, by_v), dot(vv, t) - dot(l0, t));
        c = vec4<f32>(d3d_light(s.n, col * u.p0.w, col * u.p1.x), max(dot(s.n, l0), 0.0));
    }
    o.col = q8(c);
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    var c0 = textureSample(t0, samp, i.uv0);
    let c1 = textureSample(t1, samp, i.uv1);
    if (u.op.w < 0.5) { c0 = vec4<f32>(1.0); }
    var o = vec4<f32>(c0.rgb * i.col.rgb, c0.a);
    if (u.op.x > 1.5) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    if (u.op.x > 0.5) { o = c0; }
    if (u.op.y > 0.5) { o = vec4<f32>(min(o.rgb + i.col.a * c1.rgb, vec3<f32>(1.0)), o.a); }
    if (u.op.z >= 0.0 && o.a < u.op.z) { discard; }
    return o;
}

@vertex fn vs_skin_shadow(v: SIn) -> @builtin(position) vec4<f32> {
    let s = skin(v);
    let l = u.light.xyz;
    let p = vec3<f32>(s.p.x - s.p.y * l.x / l.y, 0.0, s.p.z - s.p.y * l.z / l.y);
    var c = u.view_proj * vec4<f32>(p, 1.0);
    c.z = c.w * 0.99999;
    return c;
}
@fragment fn fs_shadow() -> @location(0) vec4<f32> {
    return vec4<f32>(u.op.xyz, 1.0);
}
"#;

/// A pass of one shape: what build_draws pushes, minus the vertices.
struct GPass {
    model: usize,
    shape: usize,
    kind: u32,
    tex: TexKey,
    blend: Blend,
    zwrite: bool,
    op0: u32,
    op1: u32,
    aref: f32,
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
    alpha: f32,
    ramp: [[f32; 4]; 16],
    /// Shadow caster (its first pass, FUN_10297e20).
    shadow: bool,
}

/// What a frame needs: camera, eye and lights in dancer space, the joint matrices (cur * bind_inv) per model,
/// the body's root and scale, the "_sort" shapes' index order.
pub struct GFrame<'a> {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub lights: &'a [Light],
    pub joints: [&'a [Mat4]; 2],
    pub root: Vec3,
    pub scale: f32,
    /// (model, shape, triangles in draw order) for sorted shapes that changed.
    pub sorted: Vec<(usize, usize, Vec<[u32; 3]>)>,
}

/// The dancer's static GPU side and its passes.
pub struct DancerGpu {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    inf: wgpu::Buffer,
    joints: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    ub: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    /// Per (model, shape): base vertex and index range.
    ranges: Vec<Vec<(i32, std::ops::Range<u32>)>>,
    passes: Vec<GPass>,
    joint_base: [u32; 2],
    calls: Vec<Call>,
}

impl DancerGpu {
    /// The shapes of body and head (in that order) and their passes, as build_draws would emit them.
    pub fn new(gpu: &Gpu, models: [&Model; 2], preps: [&crate::dat::dancer::SkinPrep; 2]) -> Self {
        let mut verts: Vec<SVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut inf: Vec<[u32; 2]> = Vec::new();
        let mut ranges = Vec::new();
        let mut passes = Vec::new();
        for (mi, (m, prep)) in models.iter().zip(preps).enumerate() {
            let mut r = Vec::new();
            for (si, s) in m.shapes.iter().enumerate() {
                let (t0, b0) = &prep.tangents[si];
                let n = s.pos.len();
                let mut lists: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
                for (j, ids, w) in &s.skin {
                    for (&v, &wt) in ids.iter().zip(w) {
                        if (v as usize) < n {
                            lists[v as usize].push((*j as u32, wt));
                        }
                    }
                }
                let base = verts.len() as i32;
                for v in 0..n {
                    let at = inf.len() as u32;
                    inf.extend(lists[v].iter().map(|(j, w)| [*j, w.to_bits()]));
                    // Unskinned shapes (no skin lists) keep the bind pose; a skinned vertex with no list sums to zero.
                    let count = if s.skin.is_empty() { 0 } else { lists[v].len() as u32 };
                    let (at, count) = if !s.skin.is_empty() && count == 0 { (at, u32::MAX) } else { (at, count) };
                    verts.push(SVertex { pos: s.pos[v].to_array(), normal: s.normal[v].to_array(), tangent: t0[v].to_array(), binormal: b0[v].to_array(), uv: s.uv[v], inf: [at, count] });
                }
                let first = indices.len() as u32;
                indices.extend(s.tris.iter().flatten());
                r.push((base, first..indices.len() as u32));
                passes.extend(shape_passes(m, mi, si));
            }
            ranges.push(r);
        }
        // A skinned vertex with no influences is (0, 0, 0): give it an empty list that sums to zero.
        for v in &mut verts {
            if v.inf[1] == u32::MAX {
                *v = SVertex { uv: v.uv, ..SVertex::zeroed() };
            }
        }
        if inf.is_empty() {
            inf.push([0, 0]);
        }
        if verts.is_empty() {
            verts.push(SVertex::zeroed());
        }
        if indices.is_empty() {
            indices.push(0);
        }
        let d = &gpu.device;
        let buf = |label, contents: &[u8], usage| d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
        let vb = buf("dancer shapes", bytemuck::cast_slice(&verts), wgpu::BufferUsages::VERTEX);
        let ib = buf("dancer triangles", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST);
        let inf = buf("dancer influences", bytemuck::cast_slice(&inf), wgpu::BufferUsages::STORAGE);
        let joint_base = [0, models[0].bones.len() as u32];
        DancerGpu { vb, ib, inf, joints: None, ub: None, ranges, passes, joint_base, calls: Vec::new() }
    }

    /// Writes the frame's joints, uniforms and sorted triangle orders (all small).
    pub fn update(&mut self, gpu: &Gpu, r: &mut DancerRenderer, f: &GFrame, coverage: bool) {
        let d = &gpu.device;
        let mut mats: Vec<[[f32; 4]; 4]> = f.joints[0].iter().chain(f.joints[1]).map(|m| m.to_cols_array_2d()).collect();
        if mats.is_empty() {
            mats.push(Mat4::IDENTITY.to_cols_array_2d());
        }
        self.joint_base = [0, f.joints[0].len() as u32];
        let jbytes: &[u8] = bytemuck::cast_slice(&mats);
        if self.joints.as_ref().is_none_or(|(_, _, c)| *c < jbytes.len() as u64) {
            let cap = (jbytes.len() as u64).next_power_of_two().max(4096);
            let jb = d.create_buffer(&wgpu::BufferDescriptor { label: Some("dancer joints"), size: cap, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let g = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: r.storage_layout(gpu),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: jb.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: self.inf.as_entire_binding() }],
            });
            self.joints = Some((jb, g, cap));
        }
        gpu.queue.write_buffer(&self.joints.as_ref().unwrap().0, 0, jbytes);
        for (m, s, tris) in &f.sorted {
            if let Some((_, range)) = self.ranges.get(*m).and_then(|r| r.get(*s)) {
                let flat: Vec<u32> = tris.iter().flatten().copied().collect();
                if flat.len() == range.len() {
                    gpu.queue.write_buffer(&self.ib, range.start as u64 * 4, bytemuck::cast_slice(&flat));
                }
            }
        }
        let shadow = r.shadow;
        let mut ldir = [[0f32; 4]; 4];
        for (i, l) in f.lights.iter().take(4).enumerate() {
            ldir[i] = [l.dir.x, l.dir.y, l.dir.z, l.color];
        }
        let fr = f.lights.first().map_or(Mat3::IDENTITY, |l| l.frame);
        let lframe = [fr.x_axis, fr.y_axis, fr.z_axis].map(|c| [c.x, c.y, c.z, 0.0]);
        let mut ubytes = Vec::new();
        let mut calls = Vec::new();
        let mut shadow_calls = Vec::new();
        let mut list: Vec<(&GPass, u32, Blend, bool)> = self.passes.iter().map(|p| (p, p.op0, p.blend, p.zwrite)).collect();
        if coverage {
            list.extend(self.passes.iter().filter(|p| p.op0 == 0 && p.zwrite).map(|p| (p, 2, Blend::AlphaOnly, false)));
        }
        let mut uniform = |p: &GPass, op: [f32; 4], light: [f32; 4]| {
            let off = ubytes.len() as u32;
            let u = GUniforms {
                view_proj: f.view_proj.to_cols_array_2d(),
                op,
                light,
                kind: [p.kind as f32, self.joint_base[p.model] as f32, if p.model == 0 { f.scale } else { 1.0 }, p.alpha],
                root: [f.root.x, f.root.y, f.root.z, 0.0],
                p0: p.p0,
                p1: p.p1,
                p2: p.p2,
                eye: [f.eye.x, f.eye.y, f.eye.z, 1.0],
                ldir,
                lframe,
                ramp: p.ramp,
            };
            ubytes.extend_from_slice(bytemuck::bytes_of(&u));
            ubytes.resize(ubytes.len().next_multiple_of(GSTRIDE), 0);
            off
        };
        for (p, op0, blend, zwrite) in list {
            let key = (p.tex.0.clone().filter(|n| r.views.contains_key(n)), p.tex.1.clone().filter(|n| r.views.contains_key(n)));
            let Some((base, idx)) = self.ranges.get(p.model).and_then(|x| x.get(p.shape)).cloned() else { continue };
            if idx.is_empty() {
                continue;
            }
            if let Some(sh) = shadow.filter(|_| p.shadow && op0 == 0 && blend != Blend::AlphaOnly && zwrite) {
                let none: TexKey = (None, None);
                r.tex_group(gpu, &none);
                let uoff = uniform(p, [sh.mult[0], sh.mult[1], sh.mult[2], 0.0], [sh.light.x, sh.light.y, sh.light.z, 0.0]);
                shadow_calls.push(Call { pipe: r.gpu_shadow_pipeline(gpu), key: none, uoff, idx: idx.clone(), base });
            }
            let uoff = uniform(p, [op0 as f32, p.op1 as f32, p.aref, if key.0.is_some() { 1.0 } else { 0.0 }], [0.0; 4]);
            r.tex_group(gpu, &key);
            calls.push(Call { pipe: r.gpu_pipeline(gpu, blend, zwrite), key, uoff, idx, base });
        }
        shadow_calls.append(&mut calls);
        if self.ub.as_ref().is_none_or(|(_, _, c)| *c < ubytes.len() as u64) {
            let cap = (ubytes.len() as u64).next_power_of_two().max(GSTRIDE as u64);
            let ub = d.create_buffer(&wgpu::BufferDescriptor { label: Some("dancer pass uniforms"), size: cap, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let g = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &r.ubl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &ub, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<GUniforms>() as u64) }) }],
            });
            self.ub = Some((ub, g, cap));
        }
        if !ubytes.is_empty() {
            gpu.queue.write_buffer(&self.ub.as_ref().unwrap().0, 0, &ubytes);
        }
        self.calls = shadow_calls;
    }

    /// Draws over `view`, clearing only depth (FUN_10245ab0).
    pub fn draw(&self, r: &DancerRenderer, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, depth: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dancer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let (Some((_, ug, _)), Some((_, jg, _))) = (&self.ub, &self.joints) else { return };
        pass.set_vertex_buffer(0, self.vb.slice(..));
        pass.set_index_buffer(self.ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_bind_group(2, jg, &[]);
        for c in &self.calls {
            let Some(tg) = r.tex_groups.get(&c.key) else { continue };
            pass.set_pipeline(&c.pipe);
            pass.set_bind_group(0, ug, &[c.uoff]);
            pass.set_bind_group(1, tg, &[]);
            pass.draw_indexed(c.idx.clone(), c.base, 0..1);
        }
    }
}

/// build_draws' passes for one shape (the same skips, kinds, blends and texture links).
fn shape_passes(m: &Model, model: usize, shape: usize) -> Vec<GPass> {
    let s = &m.shapes[shape];
    let mut out = Vec::new();
    let Some(sh) = m.shaders.get(s.shader) else { return out };
    let w = &sh.words;
    let has_alpha = |t: Option<usize>| t.and_then(|i| m.textures.get(i)).is_some_and(|t| t.format == 1);
    let tname = |t: Option<usize>| t.and_then(|i| m.textures.get(i)).map(|t| t.name.clone());
    let col = [wf(w, 0xa), wf(w, 0xb), wf(w, 0xc)];
    let (amb, kd) = (wf(w, 0xd), wf(w, 0xe));
    let p0 = [col[0], col[1], col[2], amb];
    let pass = |kind, tex: TexKey, blend, zwrite, op0, op1, aref, p1: [f32; 4], p2: [f32; 4], alpha, shadow| GPass { model, shape, kind, tex, blend, zwrite, op0, op1, aref, p0, p1, p2, alpha, ramp: [[0.0; 4]; 16], shadow };
    match sh.kind {
        0xF0001 => {
            if w[0x23] == 0 || wf(w, 0x11) >= 0.999 {
                return out;
            }
            let uam = w[0x28];
            let lit = if w[0x26] != 0 { 1.0 } else { 0.0 };
            let ct = wt(w, 0x13);
            let blend = if has_alpha(ct) && uam & 1 != 0 { Blend::Alpha } else { Blend::Opaque };
            out.push(pass(0, (tname(ct), None), blend, true, 0, 0, -1.0, [kd, lit, 0.0, 0.0], [0.0; 4], 1.0 - wf(w, 0x11), true));
            let mut op = 0;
            if let Some(env) = wt(w, 0x14) {
                let b = if uam & 4 != 0 { Blend::DstAlphaAdd } else { Blend::Add };
                out.push(pass(1, (tname(Some(env)), None), b, true, 1, 0, -1.0, [0.0; 4], [0.0; 4], 1.0, false));
                op = 1;
            }
            let mask = wt(w, 0x17);
            if mask.is_some() {
                out.push(pass(2, (tname(mask), None), Blend::AlphaOnly, true, 1, 0, -1.0, [0.0; 4], [0.0; 4], 1.0, false));
                op = 1;
            }
            if let Some(ph) = wt(w, 0x16) {
                let b = if uam & 2 == 0 && mask.is_none() { Blend::Add } else { Blend::DstAlphaAdd };
                out.push(pass(3, (tname(Some(ph)), None), b, false, op, 0, -1.0, [0.0, 0.0, wf(w, 0x20), 0.0], [0.0; 4], 1.0, false));
            }
        }
        0xF0002 => {
            if w[0x13] & 6 == 6 {
                return out;
            }
            let uam = w[0x59];
            let back = wf(w, 0x16);
            let sc = 15.0 / (back + 1.0001);
            let ct = wt(w, 0x14);
            let blend = if has_alpha(ct) && uam & 1 != 0 { Blend::Alpha } else { Blend::Opaque };
            let mut base = pass(4, (tname(ct), None), blend, true, 0, 0, -1.0, [0.0; 4], [back, sc, 0.0, 0.0], 1.0, true);
            for j in 0..16 {
                base.ramp[j] = [wf(w, 0x17 + 4 * j), wf(w, 0x18 + 4 * j), wf(w, 0x19 + 4 * j), 0.0];
            }
            out.push(base);
            if let Some(ph) = wt(w, 0x15).filter(|_| wf(w, 0x58) > 0.001) {
                let b = if uam & 2 != 0 { Blend::DstAlphaAdd } else { Blend::Add };
                out.push(pass(3, (tname(Some(ph)), None), b, false, 0, 0, -1.0, [0.0, 0.0, wf(w, 0x57), 0.0], [0.0; 4], 1.0, false));
            }
        }
        0xF0005 => {
            if w[0x14] == 0 || w[0x13] & 6 == 6 {
                return out;
            }
            let by_v = if w[0x16] != 1 { 1.0 } else { 0.0 };
            let ct = wt(w, 0x18);
            let blend = if has_alpha(ct) { Blend::Alpha } else { Blend::Opaque };
            let aref = if w[0x15] != 0 { 0x5f as f32 / 255.0 } else { -1.0 };
            out.push(pass(5, (tname(ct), tname(wt(w, 0x19))), blend, true, 0, 1, aref, [kd, 0.0, 0.0, by_v], [0.0; 4], 1.0, true));
        }
        _ => {}
    }
    out
}

impl DancerRenderer {
    fn storage_layout(&mut self, gpu: &Gpu) -> &wgpu::BindGroupLayout {
        self.sbl.get_or_insert_with(|| {
            let e = |binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("dancer skin"), entries: &[e(0), e(1)] })
        })
    }

    fn gpu_layout(&mut self, gpu: &Gpu) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        if self.gpu_mod.is_none() {
            let sbl = self.storage_layout(gpu).clone();
            let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("dancer gpu"), source: wgpu::ShaderSource::Wgsl(GWGSL.into()) });
            let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&self.ubl), Some(&self.tbl), Some(&sbl)], immediate_size: 0 });
            self.gpu_mod = Some((module, layout));
        }
        self.gpu_mod.clone().unwrap()
    }

    fn gpu_vertex_layout() -> [wgpu::VertexAttribute; 6] {
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Float32x2, 5 => Uint32x2]
    }

    fn gpu_pipeline(&mut self, gpu: &Gpu, b: Blend, zwrite: bool) -> wgpu::RenderPipeline {
        if let Some(p) = self.gpu_pipes.get(&(b, zwrite)) {
            return p.clone();
        }
        let (module, layout) = self.gpu_layout(gpu);
        let (blend, write_mask) = blend_state(b);
        let attrs = Self::gpu_vertex_layout();
        let p = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dancer gpu"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_skin"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: super::DEPTH_FORMAT, depth_write_enabled: Some(zwrite), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: self.format, blend, write_mask })] }),
            multiview_mask: None,
            cache: None,
        });
        self.gpu_pipes.insert((b, zwrite), p.clone());
        p
    }

    fn gpu_shadow_pipeline(&mut self, gpu: &Gpu) -> wgpu::RenderPipeline {
        if let Some(p) = &self.gpu_shadow {
            return p.clone();
        }
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let (module, layout) = self.gpu_layout(gpu);
        let attrs = Self::gpu_vertex_layout();
        let blend = wgpu::BlendState { color: C { src_factor: F::Zero, dst_factor: F::Src, operation: O::Add }, alpha: C { src_factor: F::Zero, dst_factor: F::One, operation: O::Add } };
        let p = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dancer gpu shadow"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_skin_shadow"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: super::DEPTH_FORMAT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_shadow"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: self.format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        });
        self.gpu_shadow = Some(p.clone());
        p
    }
}
