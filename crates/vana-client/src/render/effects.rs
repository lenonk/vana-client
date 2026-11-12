//! Effect elements on the GPU: the per-frame update pass (render/effects.wgsl `update`) over the element
//! buffer, then each live element drawn back to front with its blend mode (FUN_10045950) and its mesh or sprite.
//! Sort keys and expire requests are read back a frame or two late (fx::Effects::take_back).

use super::Put;
use super::{DEPTH_FORMAT, Gpu};
use crate::fx::elem::{BACK_WORDS, OUT_WORDS, PART_WORDS, SLOT_WORDS, w};
use crate::fx::{Draw, Effects, Frame, MAX_ELEMS, MAX_PARTS, Write};
use glam::Mat4;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    time: [f32; 4],
    view: [[f32; 4]; 4],
    view_rot: [[f32; 4]; 4],
    d10: [[f32; 4]; 4],
    d50: [[f32; 4]; 4],
    c10: [[f32; 4]; 4],
    inv_view_rot: [[f32; 4]; 4],
    eye: [f32; 4],
    /// moon phase, weekday, 0, 0
    extra: [f32; 4],
    /// The zone's area light (update op 0x69), 0
    area: [f32; 4],
    /// FUN_100467e0: the background light's travel direction, the light count
    light: [f32; 4],
    /// The scene projection, for update op 0x53's occlusion test against the scene depth.
    proj: [[f32; 4]; 4],
    /// The focal length on the 384-line reference (cam +0x2f4, update op 0x60), whether a scene depth is bound, 0, 0
    screen: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FlareU {
    proj: [[f32; 4]; 4],
    view_rot: [[f32; 4]; 4],
    eye: [f32; 4],
    info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawU {
    proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    clip: [f32; 4],
    /// "menu    dmgnum" sprites 0..10 ('0'..'9', ':'): UV rect, then (width, height) in units (dmgnum_glyphs).
    glyphs: [[f32; 4]; 22],
    view: [[f32; 4]; 4],
    /// The light colour (eased, x the light scale; w = the light count) and AMBIENT (trunc(min(a, 1) x 128) / 255).
    light_col: [f32; 4],
    ambient: [f32; 4],
}

/// The ground projections' images: 256x256 (FUN_10044270's DAT_1047bb88).
const GP_SIZE: u32 = 256;

/// FUN_10120730 for sprites 0..10 of "menu    dmgnum": the cell's UV rect over its texture and its size,
/// (corner x1 - x0 - 1, corner y2 - y0 - 1) (FUN_1011fa80).
pub fn dmgnum_glyphs(lib: &crate::dat::ui::SpriteLib, tw: f32, th: f32) -> [[f32; 4]; 22] {
    let mut g = [[0.0; 4]; 22];
    for k in 0..11 {
        let Some(c) = lib.sprites.get(k).and_then(|s| s.cells.first()) else { continue };
        let (u, v) = (c.uv[0] as f32, c.uv[1] as f32);
        g[2 * k] = [u / tw, v / th, (u + c.size[0] as f32) / tw, (v + c.size[1] as f32) / th];
        g[2 * k + 1] =
            [(c.corners[1][0] - c.corners[0][0] - 1) as f32, (c.corners[2][1] - c.corners[0][1] - 1) as f32, 0.0, 0.0];
    }
    g
}

/// Instances are element x PER_ELEM + (particle, or the shading variant for meshes).
const PER_ELEM: u32 = 512;

/// The camera matrices of FUN_100691d0 from the view V (MODLOG "Effect generators"), as column-major glam
/// matrices: b90 = rotation only; d50 = world x / z kept as unit axes, V's image of world y kept; d10 = yaw only
/// with world y as view y; c10 = d10 · d50.
pub fn view_set(v: Mat4) -> [Mat4; 4] {
    let mut b90 = v;
    b90.w_axis = glam::Vec4::W;
    let mut d50 = v;
    d50.x_axis = glam::Vec4::new(1.0, 0.0, 0.0, 0.0);
    d50.z_axis = glam::Vec4::new(0.0, 0.0, 1.0, 0.0);
    let mut d10 = v;
    d10.y_axis = glam::Vec4::new(0.0, 1.0, 0.0, 0.0);
    d10.x_axis.y = 0.0;
    d10.z_axis.y = 0.0;
    let c10 = d50 * d10;
    [b90, d10, d50, c10]
}

/// Dither shows as texels whose alpha is a local extreme along both axes: about 20 % of the covered texels in the
/// retail lamp glow 'light', under 10 % in hit sparks drawn with hard steps.
fn dithered(t: &crate::dat::zone::Texture) -> bool {
    let (w, h) = (t.width as usize, t.height as usize);
    let a = |x: usize, y: usize| t.rgba()[(y * w + x) * 4 + 3] as i32;
    let (mut covered, mut extreme) = (0usize, 0usize);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = a(x, y);
            if c == 0 && a(x - 1, y) == 0 && a(x + 1, y) == 0 {
                continue;
            }
            covered += 1;
            if (c - a(x - 1, y)) * (c - a(x + 1, y)) > 0 && (c - a(x, y - 1)) * (c - a(x, y + 1)) > 0 {
                extreme += 1;
            }
        }
    }
    extreme * 100 >= covered * 15
}

/// Not retail: an effect texture dithered between a few alpha levels (DXT1 on / off, DXT3 4-bit; the artists faked
/// soft edges that way) gets its colour spread into the clear texels and its alpha blurred, so glows fade smoothly at
/// today's resolutions. None = leave it as it is.
pub fn soften_alpha(t: &crate::dat::zone::Texture) -> Option<crate::dat::zone::Texture> {
    let (w, h) = (t.width as usize, t.height as usize);
    if w < 4 || h < 4 || t.rgba().len() < w * h * 4 {
        return None;
    }
    let levels: std::collections::BTreeSet<u8> = t.rgba().chunks(4).map(|p| p[3]).collect();
    let clear = t.rgba().chunks(4).filter(|p| p[3] == 0).count();
    if levels.len() > 8 || levels.len() < 2 || clear == 0 || clear == w * h || !dithered(t) {
        return None;
    }
    let mut px = t.rgba().to_vec();
    let at = |x: isize, y: isize| (y.clamp(0, h as isize - 1) as usize * w + x.clamp(0, w as isize - 1) as usize) * 4;
    // Colour into the clear texels, two rings out from the opaque ones.
    let mut filled: Vec<bool> = px.chunks(4).map(|p| p[3] != 0).collect();
    for _ in 0..2 {
        let (src, was) = (px.clone(), filled.clone());
        for y in 0..h as isize {
            for x in 0..w as isize {
                let i = at(x, y);
                if was[i / 4] {
                    continue;
                }
                let (mut sum, mut n) = ([0u32; 3], 0u32);
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let j = at(x + dx, y + dy);
                    if was[j / 4] {
                        (0..3).for_each(|c| sum[c] += src[j + c] as u32);
                        n += 1;
                    }
                }
                if n > 0 {
                    (0..3).for_each(|c| px[i + c] = (sum[c] / n) as u8);
                    filled[i / 4] = true;
                }
            }
        }
    }
    // Alpha: a 3 x 3 box three times (about 7 x 7, smooth).
    for _ in 0..3 {
        let src = px.clone();
        for y in 0..h as isize {
            for x in 0..w as isize {
                let mut s = 0u32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        s += src[at(x + dx, y + dy) + 3] as u32;
                    }
                }
                px[at(x, y) + 3] = (s / 9) as u8;
            }
        }
    }
    Some(crate::dat::zone::Texture::new(t.name.clone(), t.format, t.width, t.height, px))
}

/// FUN_10063c80 (0x10063c89..0x10063cc5): a CMoVuDist's draws each frame. N under 20 as is; from 20 up on a weather
/// generator (+0xde bit 0) N x the weather density (FUN_10002d80) x 0.5, truncated; otherwise N.
fn vu_dist_count(n: u32, weather: bool, density: f32) -> u32 {
    if n < 20 || !weather { n } else { (n as f32 * density * 0.5) as u32 }
}

/// FUN_10045950's blend modes as pipelines: 0 opaque, 1 alpha, 2 additive, 3 darken, 4 subtractive, 5 D3m 'I'
/// (DESTCOLOR / SRCALPHA). None = the mode has no case and keeps the previous element's blend.
fn blend_class(mode: u16, d3m: bool) -> Option<usize> {
    Some(match mode & 0xff {
        0x00 => 0,
        0x03 | 0x44 | 0x64 => 1,
        0x48 | 0x68 => 2,
        0x49 if d3m => 5,
        0x49 => 2,
        0x46 | 0x47 => 3,
        0x41 | 0x42 | 0x43 | 0x62 => 4,
        _ => return None,
    })
}

fn blend_state(class: usize) -> wgpu::BlendState {
    use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
    let c = |src, dst, operation| C { src_factor: src, dst_factor: dst, operation };
    let comp = match class {
        0 => c(F::One, F::Zero, O::Add),
        1 => c(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
        3 => c(F::Zero, F::OneMinusSrcAlpha, O::Add),
        // With BLENDOP caps: SRCALPHA / ONE, REVSUBTRACT (dst - src a).
        4 => c(F::SrcAlpha, F::One, O::ReverseSubtract),
        5 => c(F::Dst, F::SrcAlpha, O::Add),
        _ => c(F::SrcAlpha, F::One, O::Add),
    };
    wgpu::BlendState { color: comp, alpha: comp }
}

/// A pass into the 256x256 distortion copy that keeps what is there (retail never clears DAT_1047bb88).
fn dist_copy_pass<'e>(encoder: &'e mut wgpu::CommandEncoder, view: &wgpu::TextureView) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("effects dist copy"),
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
    })
}

/// DAT_1047bb88: the distortion draws' 256x256 copy, its view and bind groups.
struct DistCopy {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    mirror: wgpu::BindGroup,
    wrap: wgpu::BindGroup,
}

struct GpuMesh {
    vb: wgpu::Buffer,
    /// Texture, vertices, and whether the D3m named a texture for the run (the specular variants).
    groups: Vec<(Option<(usize, usize)>, std::ops::Range<u32>, bool)>,
    mmb: bool,
    dist: Vec<std::ops::Range<u32>>,
}

/// A staging buffer for the readback and whether it is free (0), copied (1) or mapping (2).
struct Staging {
    buf: wgpu::Buffer,
    state: u8,
    ready: Arc<AtomicBool>,
    words: usize,
}

pub struct EffectsGpu {
    update: wgpu::ComputePipeline,
    update_layout: wgpu::BindGroupLayout,
    /// The scene depth and projection the update pass tests op 0x53 against (set_depth), and the 1 x 1 stand-in bound
    /// without one.
    depth: Option<(wgpu::TextureView, Mat4)>,
    no_depth: wgpu::TextureView,
    /// Ours (the Lamp Lights setting, client/lamps.rs): each lamp's slot of the light table copied from its template
    /// light after the update, at the lamp's position. The lamps (count, then position + slot | template << 16),
    /// the pass's layout and its bind group (gone when the table changes).
    lamp_pipe: wgpu::ComputePipeline,
    lamp_layout: wgpu::BindGroupLayout,
    lamp_buf: wgpu::Buffer,
    lamp_group: Option<wgpu::BindGroup>,
    lamp_count: u32,
    draw_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    mesh_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    sprite_pipes: HashMap<usize, wgpu::RenderPipeline>,
    morph_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    ring_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    distmodel_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    distmorph_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    distring_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    spec_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    spec_layout: wgpu::BindGroupLayout,
    /// Specular images by (library, texture, cube, clamp), and the black stand-ins of the other dimension.
    spec_groups: HashMap<((usize, usize), bool, bool), wgpu::BindGroup>,
    spec_black: (wgpu::TextureView, wgpu::TextureView),
    clamp_sampler: wgpu::Sampler,
    wrap_sampler: wgpu::Sampler,
    /// CMoDist: SRCALPHA / INVSRCALPHA, z write by flag 12.
    dist_pipes: HashMap<bool, wgpu::RenderPipeline>,
    /// DamValueProg by blend class; no z write (flag 0x1000 clear in DAT 27).
    damv_pipes: HashMap<usize, wgpu::RenderPipeline>,
    /// "font    font" of "menu    dmgnum" (file 39542) and its glyph table, once the game UI is in.
    dmgnum: Option<(wgpu::BindGroup, [[f32; 4]; 22])>,
    /// The scene so far (device +0x194's picture), the source of the 256x256 distortion copies (MIRROR, linear),
    /// remade on a size or format change.
    scene_copy: Option<(wgpu::Texture, wgpu::BindGroup)>,
    copy_sampler: wgpu::Sampler,
    /// The distortion draws' shared 256x256 target (DAT_1047bb88) and its bind groups (linear; MIRROR, WRAP): FUN_10042d00
    /// squeezes the whole scene copy into it for DistRing / DistModel / DistMorph (render/squeeze.wgsl); FUN_100426f0
    /// draws CMoDist's diamond into it at the diamond's screen box (vs_dist_copy). Never cleared, as retail.
    dist_copy: Option<DistCopy>,
    squeeze: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// FUN_100426f0's copy pass (vs_dist_copy / fs_dist_copy) for the view format it was made for.
    dist_copy_pipe: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    draw_module: wgpu::ShaderModule,
    draw_pipe_layout: wgpu::PipelineLayout,
    /// Lens flares and their glare: additive, no depth test (FUN_1003ddb0, FUN_100125c0).
    flare_pipe: wgpu::RenderPipeline,
    glare_pipe: wgpu::RenderPipeline,
    flare_vis: wgpu::ComputePipeline,
    flare_layout: wgpu::BindGroupLayout,
    flare_u: wgpu::Buffer,
    params: wgpu::Buffer,
    draw_u: wgpu::Buffer,
    /// The same with gamma output off, for the ground projections' images; their bind group.
    draw_u_rt: wgpu::Buffer,
    draw_group_rt: Option<wgpu::BindGroup>,
    /// Ours (Settings' Lamp Lights): the scene depth copied before the elements draw, which glows (flag 27) fade
    /// against; a 1 x 1 stand-in until there is one.
    depth_copy: Option<(wgpu::Texture, wgpu::TextureView)>,
    depth_dummy: wgpu::TextureView,
    /// Soften glows against the scenery (the Lamp Lights setting).
    pub soft: bool,
    gp_rt_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    gp_decal_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    /// The image array (a layer per projection this frame), its layer views and groups (CLAMP), the shared depth.
    gp_rt: Option<(wgpu::Texture, Vec<wgpu::TextureView>, Vec<wgpu::BindGroup>)>,
    gp_depth: wgpu::TextureView,
    /// The decal faces of every projection in one buffer: its version, and (first vertex, count) per element.
    gp_vb: Option<(wgpu::Buffer, u64, HashMap<u32, (u32, u32)>)>,
    elems: wgpu::Buffer,
    outs: wgpu::Buffer,
    back: wgpu::Buffer,
    parts: wgpu::Buffer,
    staging: Vec<Staging>,
    ops: (wgpu::Buffer, usize),
    keys: (wgpu::Buffer, usize),
    key_desc: (wgpu::Buffer, usize),
    paths: (wgpu::Buffer, usize),
    /// The zone's point-light table (ZoneRenderer::lights), or a black one of ours until a zone gives it.
    lights: wgpu::Buffer,
    slots: (wgpu::Buffer, usize),
    sverts: (wgpu::Buffer, usize),
    sframes: (wgpu::Buffer, usize),
    mdata: (wgpu::Buffer, usize),
    /// Op 0x2c's after-images (fx/trail.rs gpu_words), their bind group (group 3) and pipeline.
    trails: (wgpu::Buffer, usize),
    trail_layout: wgpu::BindGroupLayout,
    trail_group: Option<wgpu::BindGroup>,
    trail_pipe: wgpu::RenderPipeline,
    update_group: Option<wgpu::BindGroup>,
    /// The depth `update_group` binds.
    update_depth: Option<wgpu::TextureView>,
    flare_group: super::BindCache<([wgpu::Buffer; 3], wgpu::TextureView)>,
    draw_group: Option<wgpu::BindGroup>,
    meshes: Vec<Option<GpuMesh>>,
    textures: HashMap<(usize, usize), wgpu::BindGroup>,
    white: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    count: u32,
    linearize: bool,
    /// The zone's background light for lit D3b / D3m elements: travel direction, colour, ambient (set_light).
    light: Option<(glam::Vec3, glam::Vec3, glam::Vec3)>,
}

const STAGES: usize = 3;

/// The lamps' copy (EffectsGpu::lamp_pipe): a lamp's slot takes its template's range, colour and intensity at its
/// position; a lamp with no template (0xffff) its glow's colour (after the n lamps) at range 8 and intensity 20,
/// the gate tunnel lights' (op 0x49 of glr1 .. gll2) *(inf: a stand-in)*.
const LAMP_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> lights: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> lamps: array<vec4<f32>>;

@compute @workgroup_size(64)
fn copy_lamps(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = bitcast<u32>(lamps[0].x);
    if (gid.x >= n) { return; }
    let l = lamps[1u + gid.x];
    let w = bitcast<u32>(l.w);
    let s = w & 0xffffu;
    let t = w >> 16u;
    if (t == 0xffffu) {
        lights[2u * s] = vec4<f32>(l.xyz, 8.0);
        lights[2u * s + 1u] = vec4<f32>(lamps[1u + n + gid.x].rgb, 20.0);
        return;
    }
    lights[2u * s] = vec4<f32>(l.xyz, lights[2u * t].w);
    lights[2u * s + 1u] = lights[2u * t + 1u];
}
"#;

impl EffectsGpu {
    /// The zone's background light (weather::Lighting dir / diffuse / ambient) for FUN_100467e0; None: unlit.
    pub fn set_light(&mut self, l: Option<(glam::Vec3, glam::Vec3, glam::Vec3)>) {
        self.light = l;
    }

    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let consts = crate::fx::elem::wgsl_consts();
        let src = include_str!("effects.wgsl");
        let (compute_src, draw_src) = src.split_once("// ---- Drawing ----").unwrap();
        let cmod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects update"),
            source: wgpu::ShaderSource::Wgsl(format!("{consts}{compute_src}").into()),
        });
        let dmod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects draw"),
            source: wgpu::ShaderSource::Wgsl(format!("{consts}{draw_src}").into()),
        });
        let storage = |binding, read_only, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform = |binding, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cs = wgpu::ShaderStages::COMPUTE;
        let update_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects update"),
            entries: &[
                uniform(0, cs),
                storage(1, false, cs),
                storage(2, true, cs),
                storage(3, true, cs),
                storage(4, true, cs),
                storage(5, true, cs),
                storage(6, false, cs),
                storage(7, false, cs),
                storage(8, true, cs),
                storage(9, false, cs),
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: cs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let update = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects update"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&update_layout)],
                immediate_size: 0,
            })),
            module: &cmod,
            entry_point: Some("update"),
            compilation_options: Default::default(),
            cache: None,
        });
        let lamp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects lamps"),
            entries: &[storage(0, false, cs), storage(1, true, cs)],
        });
        let lmod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects lamps"),
            source: wgpu::ShaderSource::Wgsl(LAMP_WGSL.into()),
        });
        let lamp_pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects lamps"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&lamp_layout)],
                immediate_size: 0,
            })),
            module: &lmod,
            entry_point: Some("copy_lamps"),
            compilation_options: Default::default(),
            cache: None,
        });
        let lamp_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects lamps"),
            size: (16 * (2 * crate::render::LIGHTS + 1)) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vs = wgpu::ShaderStages::VERTEX;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects draw"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                storage(1, true, vs),
                storage(2, true, vs),
                storage(3, true, vs),
                storage(4, true, vs),
                storage(5, true, vs),
                storage(6, true, vs),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(&draw_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let vbuf = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::fx::lib::MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2],
        })];
        let pipe_z = |vs_entry: &str,
                      buffers: &[Option<wgpu::VertexBufferLayout>],
                      class: usize,
                      zwrite: bool,
                      compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("effects"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &dmod,
                    entry_point: Some(vs_entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(zwrite),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    // ZBIAS is applied in to_clip (effects.wgsl).
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &dmod,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(class)),
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe = |vs_entry: &str, buffers: &[Option<wgpu::VertexBufferLayout>], class: usize, zwrite: bool| {
            pipe_z(vs_entry, buffers, class, zwrite, wgpu::CompareFunction::LessEqual)
        };
        let flare_pipe = pipe_z("vs_flare", &[], 2, false, wgpu::CompareFunction::Always);
        let glare_pipe = pipe_z("vs_glare", &[], 2, false, wgpu::CompareFunction::Always);
        let fmod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects flare"),
            source: wgpu::ShaderSource::Wgsl(format!("{consts}{}", include_str!("flare.wgsl")).into()),
        });
        let flare_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects flare"),
            entries: &[
                uniform(0, cs),
                storage(1, true, cs),
                storage(2, true, cs),
                storage(3, false, cs),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: cs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let flare_vis = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects flare"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&flare_layout)],
                immediate_size: 0,
            })),
            module: &fmod,
            entry_point: Some("flare_vis"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut mesh_pipes = HashMap::new();
        let mut sprite_pipes = HashMap::new();
        let mut morph_pipes = HashMap::new();
        let mut damv_pipes = HashMap::new();
        let mut ring_pipes = HashMap::new();
        let mut distmodel_pipes = HashMap::new();
        let mut distmorph_pipes = HashMap::new();
        // The after-image ribbon (0x1001c930): SRCALPHA / ONE, z test without z write, cull none.
        let trail_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects trails"),
            entries: &[storage(0, true, vs)],
        });
        let trail_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects trails"),
            bind_group_layouts: &[Some(&draw_layout), None, None, Some(&trail_layout)],
            immediate_size: 0,
        });
        let trail_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects trail"),
            layout: Some(&trail_pl),
            vertex: wgpu::VertexState {
                module: &dmod,
                entry_point: Some("vs_trail"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &dmod,
                entry_point: Some("fs_trail"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend_state(2)),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut distring_pipes = HashMap::new();
        // CMoD3mSpecularElem: group 2 the specular image (2D, cube) and its sampler.
        let spec_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects specular"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let spec_pipe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects specular"),
            bind_group_layouts: &[Some(&draw_layout), Some(&tex_layout), Some(&spec_layout)],
            immediate_size: 0,
        });
        let spec_pipe = |class: usize, zwrite: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("effects specular"),
                layout: Some(&spec_pipe_layout),
                vertex: wgpu::VertexState {
                    module: &dmod,
                    entry_point: Some("vs_mesh_spec"),
                    compilation_options: Default::default(),
                    buffers: &vbuf,
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(zwrite),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &dmod,
                    entry_point: Some("fs_spec"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(class)),
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mut spec_pipes = HashMap::new();
        // The ground projection: its image (the mesh pipeline into Rgba8Unorm, colour and alpha written) and its decal.
        let gp_pipe = |vs_entry: &str,
                       buffers: &[Option<wgpu::VertexBufferLayout>],
                       class: usize,
                       zwrite: bool,
                       target: wgpu::TextureFormat,
                       mask: wgpu::ColorWrites| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("effects ground projection"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &dmod,
                    entry_point: Some(vs_entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(zwrite),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &dmod,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: Some(blend_state(class)),
                        write_mask: mask,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let dbuf = [Some(wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        })];
        let mut gp_rt_pipes = HashMap::new();
        let mut gp_decal_pipes = HashMap::new();
        let dist_pipes: HashMap<bool, wgpu::RenderPipeline> =
            [false, true].into_iter().map(|z| (z, pipe("vs_dist", &[], 1, z))).collect();
        for class in 0..6 {
            for zwrite in [false, true] {
                mesh_pipes.insert((class, zwrite), pipe("vs_mesh", &vbuf, class, zwrite));
                morph_pipes.insert((class, zwrite), pipe("vs_morph", &[], class, zwrite));
                ring_pipes.insert((class, zwrite), pipe("vs_ring", &[], class, zwrite));
                distmodel_pipes.insert((class, zwrite), pipe("vs_distmodel", &vbuf, class, zwrite));
                distmorph_pipes.insert((class, zwrite), pipe("vs_distmorph", &[], class, zwrite));
                distring_pipes.insert((class, zwrite), pipe("vs_distring", &[], class, zwrite));
                spec_pipes.insert((class, zwrite), spec_pipe(class, zwrite));
                gp_rt_pipes.insert(
                    (class, zwrite),
                    gp_pipe(
                        "vs_mesh_rt",
                        &vbuf,
                        class,
                        zwrite,
                        wgpu::TextureFormat::Rgba8Unorm,
                        wgpu::ColorWrites::ALL,
                    ),
                );
                gp_decal_pipes.insert(
                    (class, zwrite),
                    gp_pipe("vs_decal", &dbuf, class, zwrite, format, wgpu::ColorWrites::COLOR),
                );
            }
            // D3a never writes z (FUN_1003db60).
            sprite_pipes.insert(class, pipe("vs_sprite", &[], class, false));
            damv_pipes.insert(class, pipe("vs_damv", &[], class, false));
        }
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        use wgpu::BufferUsages as U;
        let n = MAX_ELEMS as u64;
        let params = buf("effects params", std::mem::size_of::<Params>() as u64, U::UNIFORM | U::COPY_DST);
        let draw_u = buf("effects draw", std::mem::size_of::<DrawU>() as u64, U::UNIFORM | U::COPY_DST);
        let draw_u_rt =
            buf("effects draw, ground images", std::mem::size_of::<DrawU>() as u64, U::UNIFORM | U::COPY_DST);
        let gp_depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("effects ground image depth"),
                size: wgpu::Extent3d { width: GP_SIZE, height: GP_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let flare_u = buf("effects flare", std::mem::size_of::<FlareU>() as u64, U::UNIFORM | U::COPY_DST);
        let elems = buf("effects elements", n * w::WORDS as u64 * 4, U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let outs = buf("effects outs", n * OUT_WORDS as u64 * 4, U::STORAGE | U::COPY_SRC);
        let back = buf("effects back", n * BACK_WORDS as u64 * 4, U::STORAGE | U::COPY_SRC);
        let parts = buf("effects particles", MAX_PARTS as u64 * PART_WORDS as u64 * 4, U::STORAGE | U::COPY_DST);
        let staging = (0..STAGES)
            .map(|_| Staging {
                buf: buf("effects readback", n * BACK_WORDS as u64 * 4, U::MAP_READ | U::COPY_DST),
                state: 0,
                ready: Arc::new(AtomicBool::new(false)),
                words: 0,
            })
            .collect();
        let sampler = super::mip_sampler(device);
        let copy_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects scene copy"),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let wrap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects scene copy, wrapped"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects specular, clamped"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let spec_black_2d = super::upload_texture(
            gpu,
            &crate::dat::zone::Texture::new(String::new(), 0, 1, 1, vec![0, 0, 0, 0]),
            false,
        );
        let spec_black_cube = Self::cube_view(gpu, 1, 1, None);
        let white_view =
            super::upload_texture(gpu, &crate::dat::zone::Texture::new(String::new(), 0, 1, 1, vec![255; 4]), false);
        let white = Self::tex_group(device, &tex_layout, &sampler, &white_view);
        let table = |label: &str| (buf(label, 1024, U::STORAGE | U::COPY_DST), 1024usize);
        let no_depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("effects no depth"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            update,
            update_layout,
            depth: None,
            no_depth,
            draw_layout,
            tex_layout,
            mesh_pipes,
            sprite_pipes,
            morph_pipes,
            ring_pipes,
            distmodel_pipes,
            distmorph_pipes,
            trails: table("effects trails"),
            trail_layout,
            trail_group: None,
            trail_pipe,
            distring_pipes,
            spec_pipes,
            spec_layout,
            spec_groups: HashMap::new(),
            spec_black: (spec_black_2d, spec_black_cube),
            clamp_sampler,
            wrap_sampler,
            dist_pipes,
            damv_pipes,
            dmgnum: None,
            scene_copy: None,
            dist_copy: None,
            squeeze: None,
            dist_copy_pipe: None,
            draw_module: dmod,
            draw_pipe_layout: layout,
            copy_sampler,
            flare_pipe,
            glare_pipe,
            flare_vis,
            flare_layout,
            flare_u,
            params,
            draw_u,
            draw_u_rt,
            draw_group_rt: None,
            depth_copy: None,
            depth_dummy: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("effects depth stand-in"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: super::DEPTH_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
            soft: true,
            gp_rt_pipes,
            gp_decal_pipes,
            gp_rt: None,
            gp_depth,
            gp_vb: None,
            elems,
            outs,
            back,
            parts,
            staging,
            ops: table("effects ops"),
            keys: table("effects keys"),
            key_desc: table("effects key table"),
            paths: table("effects paths"),
            lights: buf("effects lights", (crate::render::LIGHTS * 32) as u64, U::STORAGE | U::COPY_DST),
            slots: table("effects slots"),
            sverts: table("effects sprite vertices"),
            sframes: table("effects sprite frames"),
            mdata: table("effects morph data"),
            update_group: None,
            update_depth: None,
            flare_group: Default::default(),
            lamp_pipe,
            lamp_layout,
            lamp_buf,
            lamp_group: None,
            lamp_count: 0,
            draw_group: None,
            meshes: Vec::new(),
            textures: HashMap::new(),
            white,
            sampler,
            count: 0,
            linearize: format.is_srgb(),
            light: None,
        }
    }

    pub fn has_dmgnum(&self) -> bool {
        self.dmgnum.is_some()
    }

    /// The damage number glyphs: the "menu    dmgnum" sheet's texture and its sprites 0..10.
    pub fn set_dmgnum(&mut self, gpu: &Gpu, lib: &crate::dat::ui::SpriteLib, image: &crate::dat::ui::UiImage) {
        let view = super::upload_texture(
            gpu,
            &crate::dat::zone::Texture::new(String::new(), 0, image.width, image.height, image.rgba.clone()),
            false,
        );
        let g = Self::tex_group(&gpu.device, &self.tex_layout, &self.sampler, &view);
        self.dmgnum = Some((g, dmgnum_glyphs(lib, image.width as f32, image.height as f32)));
    }

    /// The ground images' array: at least `n` layers (grown by powers of two), a view and a CLAMP group per layer.
    fn gp_layers(&mut self, gpu: &Gpu, n: u32) {
        if self.gp_rt.as_ref().is_some_and(|(t, _, _)| t.size().depth_or_array_layers >= n) {
            return;
        }
        let layers = n.next_power_of_two().max(4);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects ground images"),
            size: wgpu::Extent3d { width: GP_SIZE, height: GP_SIZE, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let views: Vec<wgpu::TextureView> = (0..layers)
            .map(|k| {
                tex.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: k,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let groups =
            views.iter().map(|v| Self::tex_group(&gpu.device, &self.tex_layout, &self.clamp_sampler, v)).collect();
        self.gp_rt = Some((tex, views, groups));
    }

    /// FUN_10039e50 for a `cubemap ` image: every face black (rgb 0), face -Z (layer 5) the image.
    fn cube_view(gpu: &Gpu, w: u32, h: u32, rgba: Option<&[u8]>) -> wgpu::TextureView {
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects cube"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 6 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let black = vec![0u8; (w * h * 4) as usize];
        for layer in 0..6 {
            let data = if layer == 5 { rgba.unwrap_or(&black) } else { &black };
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        })
    }

    /// A specular element's group 2: its image as 2D or cube (the other black), WRAP or CLAMP.
    fn spec_group(&mut self, gpu: &Gpu, fx: &Effects, s: crate::fx::SpecTex) {
        let key = (s.tex, s.cube, s.clamp);
        if self.spec_groups.contains_key(&key) {
            return;
        }
        let t = &fx.reg.libs[s.tex.0].textures[s.tex.1];
        let (d2, cube) = if s.cube {
            (None, Some(Self::cube_view(gpu, t.width, t.height, Some(t.rgba()))))
        } else {
            (Some(super::upload_texture(gpu, t, false)), None)
        };
        let sampler = if s.clamp { &self.clamp_sampler } else { &self.sampler };
        let g = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects specular"),
            layout: &self.spec_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(d2.as_ref().unwrap_or(&self.spec_black.0)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(cube.as_ref().unwrap_or(&self.spec_black.1)),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        self.spec_groups.insert(key, g);
    }

    /// FUN_10042d00's copy pass into DistRing's 256x256 target (render/squeeze.wgsl), drawn in `format`.
    fn squeeze_pipe(
        device: &wgpu::Device,
        tex_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects squeeze"),
            source: wgpu::ShaderSource::Wgsl(include_str!("squeeze.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects squeeze"),
            bind_group_layouts: &[Some(tex_layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects squeeze"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// FUN_100426f0's copy pass: the diamond's four tips as a quad into the 256x256 target at the diamond's screen box
    /// (RS 0x103530d0: no blend, no z, no fog; TSS 0x10353028: colour TEXTURE, alpha TFACTOR 0x80; linear, MIRROR).
    fn dist_copy_pipe_for(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects dist copy"),
            layout: Some(&self.draw_pipe_layout),
            vertex: wgpu::VertexState {
                module: &self.draw_module,
                entry_point: Some("vs_dist_copy"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.draw_module,
                entry_point: Some("fs_dist_copy"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn tex_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Grows a table buffer to hold `bytes`; true when it was replaced (the bind groups must be rebuilt).
    fn fit(gpu: &Gpu, t: &mut (wgpu::Buffer, usize), bytes: usize, label: &str) -> bool {
        if bytes <= t.1 {
            return false;
        }
        let size = bytes.next_power_of_two();
        t.0 = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        t.1 = size;
        true
    }

    /// New meshes and sprite textures only (while a zone goes up, so the first shown frame does not upload them).
    pub fn upload_pending(&mut self, gpu: &Gpu, fx: &mut Effects) {
        for id in std::mem::take(&mut fx.reg.new_meshes) {
            self.upload_mesh(gpu, fx, id);
        }
        for id in std::mem::take(&mut fx.reg.new_sprites) {
            let r = fx.reg.sprites[id as usize];
            if let Some(t) = fx.reg.libs[r.0].sprites.get(&r.1).and_then(|s| s.texture) {
                self.texture(gpu, fx, t);
            }
        }
    }

    /// The zone's point-light table for kind 0x47 elements to write (a change rebinds).
    pub fn set_lights(&mut self, lights: &wgpu::Buffer) {
        if self.lights != *lights {
            self.lights = lights.clone();
            self.update_group = None;
            self.lamp_group = None;
        }
    }

    /// The lamps (client/lamps.rs): position, slot, template slot (0xffff: its light) and the glow's colour
    /// (BGRA, 0x80 = 1.0).
    pub fn set_lamps(&mut self, gpu: &Gpu, lamps: &[(glam::Vec3, u32, u32, u32)]) {
        let n = lamps.len().min(crate::render::LIGHTS);
        let mut words: Vec<[f32; 4]> = vec![[f32::from_bits(n as u32), 0.0, 0.0, 0.0]];
        words.extend(lamps[..n].iter().map(|(p, slot, t, _)| [p.x, p.y, p.z, f32::from_bits(slot | t << 16)]));
        words.extend(lamps[..n].iter().map(|(.., c)| {
            [((c >> 16) & 0xff) as f32 / 128.0, ((c >> 8) & 0xff) as f32 / 128.0, (c & 0xff) as f32 / 128.0, 0.0]
        }));
        gpu.queue.put(&self.lamp_buf, 0, bytemuck::cast_slice(&words));
        self.lamp_count = n as u32;
    }

    /// The scene's depth (drawn this frame, before the update pass) and projection, for update op 0x53's occlusion
    /// test; without one the op leaves the element as it is.
    pub fn set_depth(&mut self, depth: Option<(&wgpu::TextureView, Mat4)>) {
        self.depth = depth.map(|(v, p)| (v.clone(), p));
    }

    /// Uploads this frame's element writes, tables and new meshes, then records the update pass and the copy
    /// of its readback.
    pub fn update(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, fx: &mut Effects, f: &Frame) {
        let q = &gpu.queue;
        let reg = &mut fx.reg;
        let mut rebind = self.update_group.is_none();
        let mut redraw = self.draw_group.is_none();
        if reg.ops_dirty {
            rebind |= Self::fit(gpu, &mut self.ops, reg.ops.len() * 4, "effects ops");
            q.put(&self.ops.0, 0, bytemuck::cast_slice(&reg.ops));
            reg.ops_dirty = false;
        }
        if reg.keys_dirty {
            rebind |= Self::fit(gpu, &mut self.keys, reg.key_data.len() * 8, "effects keys");
            rebind |= Self::fit(gpu, &mut self.key_desc, reg.key_desc.len() * 8, "effects key table");
            q.put(&self.keys.0, 0, bytemuck::cast_slice(&reg.key_data));
            q.put(&self.key_desc.0, 0, bytemuck::cast_slice(&reg.key_desc));
            reg.keys_dirty = false;
        }
        if reg.paths_dirty {
            rebind |= Self::fit(gpu, &mut self.paths, reg.path_data.len() * 16, "effects paths");
            q.put(&self.paths.0, 0, bytemuck::cast_slice(&reg.path_data));
            reg.paths_dirty = false;
        }
        if reg.sprites_dirty {
            redraw |= Self::fit(gpu, &mut self.sverts, reg.sprite_verts.len() * 36, "effects sprite vertices");
            redraw |= Self::fit(gpu, &mut self.sframes, reg.sprite_frame_table.len() * 8, "effects sprite frames");
            q.put(&self.sverts.0, 0, bytemuck::cast_slice(&reg.sprite_verts));
            q.put(&self.sframes.0, 0, bytemuck::cast_slice(&reg.sprite_frame_table));
            reg.sprites_dirty = false;
        }
        if reg.morphs_dirty {
            redraw |= Self::fit(gpu, &mut self.mdata, reg.morph_data.len() * 4, "effects morph data");
            q.put(&self.mdata.0, 0, bytemuck::cast_slice(&reg.morph_data));
            reg.morphs_dirty = false;
        }
        if fx.trails_dirty {
            let words: Vec<u32> = fx
                .trails
                .iter()
                .flat_map(|t| t.as_ref().map_or_else(|| vec![0u32; crate::fx::trail::WORDS], |t| t.gpu_words()))
                .collect();
            if !words.is_empty() {
                if Self::fit(gpu, &mut self.trails, words.len() * 4, "effects trails") {
                    self.trail_group = None;
                }
                q.put(&self.trails.0, 0, bytemuck::cast_slice(&words));
            }
            fx.trails_dirty = false;
        }
        if self.trail_group.is_none() {
            self.trail_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effects trails"),
                layout: &self.trail_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.trails.0.as_entire_binding() }],
            }));
        }
        rebind |= Self::fit(gpu, &mut self.slots, fx.slots.len() * SLOT_WORDS * 4, "effects slots");
        if !fx.slots.is_empty() {
            q.put(&self.slots.0, 0, bytemuck::cast_slice(&fx.slots));
        }
        for id in std::mem::take(&mut fx.reg.new_meshes) {
            self.upload_mesh(gpu, fx, id);
        }
        for id in std::mem::take(&mut fx.reg.new_sprites) {
            let r = fx.reg.sprites[id as usize];
            if let Some(t) = fx.reg.libs[r.0].sprites.get(&r.1).and_then(|s| s.texture) {
                self.texture(gpu, fx, t);
            }
        }
        for id in std::mem::take(&mut fx.reg.new_morphs) {
            if let Some(t) = fx.reg.morph_data_of(id).and_then(|m| m.texture) {
                self.texture(gpu, fx, t);
            }
        }
        for wr in std::mem::take(&mut fx.writes) {
            match wr {
                Write::Full(e, el) => {
                    q.put(&self.elems, (e as usize * w::WORDS * 4) as u64, bytemuck::cast_slice(&el.0))
                }
                Write::Word(e, off, v) => {
                    q.put(&self.elems, ((e as usize * w::WORDS + off) * 4) as u64, bytemuck::bytes_of(&v))
                }
                Write::Parts(first, recs) => {
                    q.put(&self.parts, (first as usize * PART_WORDS * 4) as u64, bytemuck::cast_slice(&recs))
                }
                Write::LightOff(s) if (s as usize) < crate::render::LIGHTS => {
                    q.put(&self.lights, s as u64 * 32 + 16, &[0u8; 16])
                }
                Write::LightOff(_) => {}
            }
        }
        // The scene depth changes with the window: bound afresh when it is another view.
        let depth = self.depth.as_ref().map_or(&self.no_depth, |d| &d.0);
        if rebind || self.update_depth.as_ref() != Some(depth) {
            self.update_depth = Some(depth.clone());
            self.update_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effects update"),
                layout: &self.update_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.elems.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.ops.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.keys.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.key_desc.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: self.slots.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: self.outs.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: self.back.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 8, resource: self.paths.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 9, resource: self.lights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(depth) },
                ],
            }));
        }
        if redraw {
            self.draw_groups(gpu);
        }
        self.count = fx.elems.len() as u32;
        let [b90, d10, d50, c10] = view_set(f.view);
        let m = |x: Mat4| x.to_cols_array_2d();
        let p = Params {
            time: [f.dt, f.seconds, f.dayfrac, self.count as f32],
            view: m(f.view),
            view_rot: m(b90),
            d10: m(d10),
            d50: m(d50),
            c10: m(c10),
            inv_view_rot: m(b90.inverse()),
            eye: [f.eye.x, f.eye.y, f.eye.z, 1.0],
            extra: [f.moon_phase as f32, f.weekday as f32, fx.weather_density, 0.0],
            area: [f.area_light.x, f.area_light.y, f.area_light.z, 0.0],
            light: self.light.map_or([0.0; 4], |(d, _, _)| [d.x, d.y, d.z, 1.0]),
            proj: m(self.depth.as_ref().map_or(Mat4::IDENTITY, |d| d.1)),
            screen: [f.focal, if self.depth.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        q.put(&self.params, 0, bytemuck::bytes_of(&p));
        if self.count == 0 {
            return;
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("effects update"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.update);
            pass.set_bind_group(0, self.update_group.as_ref().unwrap(), &[]);
            pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
        }
        if self.lamp_count > 0 {
            let group = self.lamp_group.get_or_insert_with(|| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("effects lamps"),
                    layout: &self.lamp_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.lights.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: self.lamp_buf.as_entire_binding() },
                    ],
                })
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("effects lamps"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.lamp_pipe);
            pass.set_bind_group(0, &*group, &[]);
            pass.dispatch_workgroups(self.lamp_count.div_ceil(64), 1, 1);
        }
        if let Some(s) = self.staging.iter_mut().find(|s| s.state == 0) {
            let bytes = (self.count as usize * BACK_WORDS * 4) as u64;
            encoder.copy_buffer_to_buffer(&self.back, 0, &s.buf, 0, bytes);
            s.state = 1;
            s.words = self.count as usize * BACK_WORDS;
        }
    }

    /// The draw bind groups (group 0): the elements' tables and the scene depth copy (or its stand-in).
    fn draw_groups(&mut self, gpu: &Gpu) {
        let depth = self.depth_copy.as_ref().map_or(&self.depth_dummy, |c| &c.1);
        let group = |label, u: &wgpu::Buffer| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.draw_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.outs.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.elems.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.parts.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.sverts.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: self.sframes.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: self.mdata.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(depth) },
                ],
            })
        };
        let (rt, main) = (group("effects draw, ground images", &self.draw_u_rt), group("effects draw", &self.draw_u));
        (self.draw_group_rt, self.draw_group) = (Some(rt), Some(main));
    }

    /// Ours: the scene depth copied for the glows' fade, when any glow (flag 27) draws and the target allows it.
    fn copy_depth(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        fx: &Effects,
        order: &[u32],
    ) -> bool {
        let t = depth.texture();
        let glows = order.iter().any(|&e| fx.elems[e as usize].as_ref().is_some_and(|c| c.flags & 0x800_0000 != 0));
        if !self.soft || !glows || !t.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return false;
        }
        if self.depth_copy.as_ref().is_none_or(|(c, _)| c.size() != t.size()) {
            let c = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("effects scene depth"),
                size: t.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: t.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let v = c.create_view(&Default::default());
            self.depth_copy = Some((c, v));
            self.draw_groups(gpu);
        }
        let c = &self.depth_copy.as_ref().unwrap().0;
        encoder.copy_texture_to_texture(t.as_image_copy(), c.as_image_copy(), t.size());
        true
    }

    /// After the frame's submit: starts mapping the readback copied this frame, and hands finished ones to the
    /// effect manager.
    pub fn after_submit(&mut self, gpu: &Gpu, fx: &mut Effects) {
        for s in &mut self.staging {
            if s.state == 1 {
                let ready = s.ready.clone();
                ready.store(false, Ordering::Release);
                s.buf.slice(..(s.words * 4).max(4) as u64).map_async(wgpu::MapMode::Read, move |r| {
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
                {
                    let data = s.buf.slice(..(s.words * 4).max(4) as u64).get_mapped_range();
                    if let Ok(data) = data {
                        fx.take_back(bytemuck::cast_slice(&data[..s.words * 4]));
                    }
                }
                s.buf.unmap();
                s.state = 0;
            }
        }
    }

    /// A texture's bind group, uploaded on first use; effect textures stay in gamma space (the shader does the
    /// stage maths there).
    fn texture(&mut self, gpu: &Gpu, fx: &Effects, t: (usize, usize)) {
        if !self.textures.contains_key(&t) {
            let src = &fx.reg.libs[t.0].textures[t.1];
            let soft = soften_alpha(src);
            let view = super::upload_texture(gpu, soft.as_ref().unwrap_or(src), false);
            self.textures.insert(t, Self::tex_group(&gpu.device, &self.tex_layout, &self.sampler, &view));
        }
    }

    fn upload_mesh(&mut self, gpu: &Gpu, fx: &Effects, id: u32) {
        use wgpu::util::DeviceExt;
        let (lib, node) = fx.reg.meshes[id as usize];
        let Some(m) = fx.reg.libs[lib].meshes.get(&node) else { return };
        if self.meshes.len() <= id as usize {
            self.meshes.resize_with(id as usize + 1, || None);
        }
        if m.verts.is_empty() {
            return;
        }
        let vb = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effect mesh"),
            contents: bytemuck::cast_slice(&m.verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let groups: Vec<_> = m.groups.iter().map(|g| (g.texture, g.range.clone(), g.name.is_some())).collect();
        for t in groups.iter().filter_map(|g| g.0) {
            self.texture(gpu, fx, t);
        }
        self.meshes[id as usize] = Some(GpuMesh { vb, groups, mmb: m.mmb, dist: m.dist.clone() });
    }

    /// Draws the live elements over the scene (same targets, depth tested, back to front). `color_tex` is the
    /// texture behind `color` (CMoDist copies it; without one, or without COPY_SRC, distortion is not drawn).
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: (&wgpu::TextureView, &wgpu::TextureView, [u32; 2]),
        color_tex: Option<&wgpu::Texture>,
        fx: &Effects,
        cam: (Mat4, Mat4, glam::Vec3),
        clip: [f32; 2],
        fog_color: [f32; 3],
        fog: [f32; 2],
    ) {
        self.draw_with_breaks(gpu, encoder, target, color_tex, fx, cam, clip, fog_color, fog, &[], &mut |_, _| {});
    }

    /// As draw, with other transparent things in the sorted list: `breaks` are their keys (view depth, as the
    /// elements', back to front); before the first element nearer than break i, the pass ends, `on_break(encoder, i)`
    /// draws it, and a new pass goes on. Returns how many breaks it ran (the caller draws the rest, all nearer).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_with_breaks(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: (&wgpu::TextureView, &wgpu::TextureView, [u32; 2]),
        color_tex: Option<&wgpu::Texture>,
        fx: &Effects,
        cam: (Mat4, Mat4, glam::Vec3),
        clip: [f32; 2],
        fog_color: [f32; 3],
        fog: [f32; 2],
        breaks: &[f32],
        on_break: &mut dyn FnMut(&mut wgpu::CommandEncoder, usize),
    ) -> usize {
        if self.draw_group.is_none() || (self.count == 0 && fx.trails.iter().all(|t| t.is_none())) {
            return 0;
        }
        let order = fx.draw_order();
        let any_dist = order.iter().any(|&e| {
            fx.elems[e as usize].as_ref().is_some_and(|c| {
                matches!(
                    c.draw,
                    Draw::Dist | Draw::VuDist(..) | Draw::DistModel(_) | Draw::DistMorph(_) | Draw::DistRing
                )
            })
        });
        let copy_src = color_tex.filter(|t| any_dist && t.usage().contains(wgpu::TextureUsages::COPY_SRC));
        if let Some(t) = copy_src
            && self.scene_copy.as_ref().is_none_or(|(c, _)| c.size() != t.size() || c.format() != t.format())
        {
            let c = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("effects scene copy"),
                size: t.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: t.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[t.format().remove_srgb_suffix()],
            });
            // Drawing in gamma space (not linearize), the copy is read as stored, without the sRGB decode.
            let vf = if self.linearize { t.format() } else { t.format().remove_srgb_suffix() };
            let view = c.create_view(&wgpu::TextureViewDescriptor { format: Some(vf), ..Default::default() });
            let g = Self::tex_group(&gpu.device, &self.tex_layout, &self.copy_sampler, &view);
            self.scene_copy = Some((c, g));
            let rc = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("effects dist copy"),
                size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: t.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[t.format().remove_srgb_suffix()],
            });
            let rv = rc.create_view(&wgpu::TextureViewDescriptor { format: Some(vf), ..Default::default() });
            let mirror = Self::tex_group(&gpu.device, &self.tex_layout, &self.copy_sampler, &rv);
            let wrap = Self::tex_group(&gpu.device, &self.tex_layout, &self.wrap_sampler, &rv);
            self.dist_copy = Some(DistCopy { _tex: rc, view: rv, mirror, wrap });
            if self.squeeze.as_ref().is_none_or(|(f, _)| *f != vf) {
                self.squeeze = Some((vf, Self::squeeze_pipe(&gpu.device, &self.tex_layout, vf)));
            }
            if self.dist_copy_pipe.as_ref().is_none_or(|(f, _)| *f != vf) {
                let pipe = self.dist_copy_pipe_for(&gpu.device, vf);
                self.dist_copy_pipe = Some((vf, pipe));
            }
        }
        for &e in &order {
            if let Some(sp) = fx.elems[e as usize].as_ref().and_then(|c| c.spec) {
                self.spec_group(gpu, fx, sp);
            }
        }
        // Ground projections: a layer each this frame, and the decal faces when any changed.
        let gps: Vec<u32> = order
            .iter()
            .copied()
            .filter(|&e| {
                e < self.count && fx.elems[e as usize].as_ref().is_some_and(|c| matches!(c.draw, Draw::GroundProj(_)))
            })
            .collect();
        if !gps.is_empty() {
            self.gp_layers(gpu, gps.len() as u32);
        }
        if self.gp_vb.as_ref().is_none_or(|v| v.1 != fx.decal_version) {
            use wgpu::util::DeviceExt;
            let mut verts: Vec<[f32; 3]> = Vec::new();
            let mut ranges = HashMap::new();
            for (e, c) in fx.elems.iter().enumerate() {
                let Some(gp) = c.as_ref().and_then(|c| c.gp.as_ref()) else { continue };
                ranges.insert(e as u32, (verts.len() as u32, gp.verts.len() as u32));
                verts.extend(gp.verts.iter().map(|v| v.to_array()));
            }
            if verts.is_empty() {
                verts.push([0.0; 3]);
            }
            let b = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("effects ground decal"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            self.gp_vb = Some((b, fx.decal_version, ranges));
        }
        let soft = self.copy_depth(gpu, encoder, target.1, fx, &order);
        let Some(group) = self.draw_group.as_ref() else { return 0 };
        let (color, depth, size) = target;
        let (proj, view, eye) = cam;
        let u = DrawU {
            proj: proj.to_cols_array_2d(),
            fog_color: [fog_color[0], fog_color[1], fog_color[2], 1.0],
            fog: [fog[0], fog[1].max(fog[0] + 1.0), self.linearize as u32 as f32, soft as u32 as f32],
            clip: [clip[0], clip[1], size[0] as f32, size[1] as f32],
            glyphs: self.dmgnum.as_ref().map_or([[0.0; 4]; 22], |d| d.1),
            view: view.to_cols_array_2d(),
            light_col: self.light.map_or([0.0; 4], |(_, c, _)| [c.x, c.y, c.z, 1.0]),
            ambient: self.light.map_or([0.0; 4], |(_, _, a)| {
                let g = crate::fx::elem::ambient_ref(a);
                [g.x, g.y, g.z, 0.0]
            }),
        };
        gpu.queue.put(&self.draw_u, 0, bytemuck::bytes_of(&u));
        let mut u_rt = u;
        u_rt.fog[2] = 0.0;
        u_rt.fog[3] = 0.0;
        gpu.queue.put(&self.draw_u_rt, 0, bytemuck::bytes_of(&u_rt));
        // FUN_10040e70 steps 2-4: each projection's image, cleared to 0 (depth 1), the mesh as its mode draws it.
        let mut gp_layer: HashMap<u32, usize> = HashMap::new();
        if let (Some((_, views, _)), Some(g_rt)) = (self.gp_rt.as_ref(), self.draw_group_rt.as_ref()) {
            for (k, &e) in gps.iter().enumerate() {
                let (Some(c), Some(view_k)) = (fx.elems[e as usize].as_ref(), views.get(k)) else { continue };
                let Draw::GroundProj(id) = c.draw else { continue };
                let Some(Some(m)) = self.meshes.get(id as usize) else { continue };
                gp_layer.insert(e, k);
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("effects ground image"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: view_k,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.gp_depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let class = blend_class(c.blend, !m.mmb).unwrap_or(2);
                rp.set_pipeline(&self.gp_rt_pipes[&(class, c.flags & 0x1000 != 0)]);
                rp.set_bind_group(0, g_rt, &[]);
                rp.set_vertex_buffer(0, m.vb.slice(..));
                let base = e * PER_ELEM;
                for (tex, range, _) in &m.groups {
                    let variant = if m.mmb { 2 } else { 0 } + tex.is_none() as u32;
                    rp.set_bind_group(1, tex.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                    rp.draw(range.clone(), base + variant..base + variant + 1);
                }
            }
        }
        // Lens flares count their visible sky samples against this frame's scene depth first.
        let flares: Vec<u32> = fx
            .draw_order()
            .into_iter()
            .filter(|&e| {
                fx.elems[e as usize]
                    .as_ref()
                    .is_some_and(|c| matches!(c.draw, Draw::Sprite(id) if fx.reg.sprite_is_flare(id)))
            })
            .collect();
        if !flares.is_empty() {
            let mut view_rot = view;
            view_rot.w_axis = glam::Vec4::W;
            let fu = FlareU {
                proj: proj.to_cols_array_2d(),
                view_rot: view_rot.to_cols_array_2d(),
                eye: [eye.x, eye.y, eye.z, 1.0],
                info: [self.count as f32, 0.0, 0.0, 0.0],
            };
            gpu.queue.put(&self.flare_u, 0, bytemuck::bytes_of(&fu));
            let key = ([self.elems.clone(), self.slots.0.clone(), self.outs.clone()], depth.clone());
            let fg = self.flare_group.get(key, || {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("effects flare"),
                    layout: &self.flare_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.flare_u.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: self.elems.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: self.slots.0.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: self.outs.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(depth) },
                    ],
                })
            });
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("effects flare"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.flare_vis);
            cp.set_bind_group(0, &fg, &[]);
            cp.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
        }
        let new_pass = |encoder: &mut wgpu::CommandEncoder| {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("effects"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            pass.set_bind_group(0, group, &[]);
            pass
        };
        // FUN_10042d00: the scene so far copied, then squeezed and flipped into the 256x256 target.
        let squeeze = |encoder: &mut wgpu::CommandEncoder| -> bool {
            let (Some(src), Some((copy, cg)), Some(dc), Some((_, sq))) =
                (copy_src, self.scene_copy.as_ref(), self.dist_copy.as_ref(), self.squeeze.as_ref())
            else {
                return false;
            };
            encoder.copy_texture_to_texture(src.as_image_copy(), copy.as_image_copy(), src.size());
            let mut sp = dist_copy_pass(encoder, &dc.view);
            sp.set_pipeline(sq);
            sp.set_bind_group(0, cg, &[]);
            sp.draw(0..3, 0..1);
            true
        };
        let mut pass = new_pass(encoder);
        // Unknown modes keep the previous element's blend (additive before the first).
        let mut class = 2usize;
        let mut bound: Option<(bool, usize, bool)> = None;
        let mut next_break = 0;
        for (key, e) in fx.draw_items_keyed() {
            // Another transparent thing farther than this item: it goes first, outside the pass.
            while let Some(i) = break_before(breaks, next_break, key) {
                drop(pass);
                on_break(encoder, i);
                next_break = i + 1;
                pass = new_pass(encoder);
                bound = None;
            }
            // An after-image among the elements (0x1001c930 from the shared depth list).
            if e & crate::fx::TRAIL_ITEM != 0 {
                let slot = e & !crate::fx::TRAIL_ITEM;
                let (Some(tr), Some(g)) =
                    (fx.trails.get(slot as usize).and_then(|t| t.as_ref()), self.trail_group.as_ref())
                else {
                    continue;
                };
                pass.set_pipeline(&self.trail_pipe);
                bound = None;
                pass.set_bind_group(3, g, &[]);
                pass.draw(0..6 * (tr.count as u32 - 1) * (1 + tr.split as u32), slot..slot + 1);
                continue;
            }
            let Some(c) = fx.elems[e as usize].as_ref() else { continue };
            if e >= self.count {
                continue;
            }
            let base = e * PER_ELEM;
            match c.draw {
                // FUN_100426f0 (CMoDist pass 1): the diamond copied into the 256x256 target at its screen box, 1:1 up
                // to 255 pixels a side and squeezed to 255 beyond (vs_dist_copy); pass 2 the shifted fan sampling it.
                // VuDist: the same diamond n times, each copy holding the last draw (the shift compounds).
                Draw::Dist | Draw::VuDist(..) => {
                    let (Some(src), Some((copy, cg)), Some(dc), Some((_, cp))) =
                        (copy_src, self.scene_copy.as_ref(), self.dist_copy.as_ref(), self.dist_copy_pipe.as_ref())
                    else {
                        continue;
                    };
                    let n = match c.draw {
                        Draw::VuDist(n, weather) => vu_dist_count(n, weather, fx.weather_density).min(64),
                        _ => 1,
                    };
                    for _ in 0..n {
                        drop(pass);
                        encoder.copy_texture_to_texture(src.as_image_copy(), copy.as_image_copy(), src.size());
                        {
                            let mut cpass = dist_copy_pass(encoder, &dc.view);
                            cpass.set_pipeline(cp);
                            cpass.set_bind_group(0, group, &[]);
                            cpass.set_bind_group(1, cg, &[]);
                            cpass.draw(0..6, base..base + 1);
                        }
                        pass = new_pass(encoder);
                        pass.set_pipeline(&self.dist_pipes[&(c.flags & 0x1000 != 0)]);
                        pass.set_bind_group(1, &dc.mirror, &[]);
                        pass.draw(0..12, base..base + 1);
                    }
                    bound = None;
                }
                // FUN_10182700: the faces around the element with its image (CLAMP), in op 0x43's blend.
                Draw::GroundProj(_) => {
                    let (Some(&k), Some((vb, _, ranges)), Some((_, _, groups)), Some(gp)) = (
                        gp_layer.get(&e),
                        self.gp_vb.as_ref(),
                        self.gp_rt.as_ref(),
                        c.gp.as_ref().filter(|g| !g.verts.is_empty()),
                    ) else {
                        continue;
                    };
                    let Some(&(first, n)) = ranges.get(&e).filter(|r| r.1 > 0) else { continue };
                    let cl = blend_class(gp.blend, false).unwrap_or(class);
                    pass.set_pipeline(&self.gp_decal_pipes[&(cl, c.flags & 0x1000 != 0)]);
                    bound = None;
                    pass.set_bind_group(1, &groups[k], &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(first..first + n, base..base + 1);
                }
                // FUN_10043720: the scene squeezed into the 256x256 copy (FUN_10042d00), which the ring samples at
                // its screen points; retail's soft, shifted band.
                Draw::DistRing => {
                    let Some(dc) = self.dist_copy.as_ref() else { continue };
                    drop(pass);
                    if !squeeze(encoder) {
                        pass = new_pass(encoder);
                        continue;
                    }
                    pass = new_pass(encoder);
                    class = blend_class(c.blend, false).unwrap_or(class);
                    pass.set_pipeline(&self.distring_pipes[&(class, c.flags & 0x1000 != 0)]);
                    bound = None;
                    pass.set_bind_group(1, &dc.mirror, &[]);
                    pass.draw(0..3 * 2 * 255 * 3, base..base + 1);
                }
                // FUN_10043320: the scene squeezed into the 256x256 copy (FUN_10042d00), then the mesh's DistModel ranges
                // over it (WRAP).
                Draw::DistModel(id) => {
                    let Some(Some(m)) = self.meshes.get(id as usize) else { continue };
                    let Some(dc) = self.dist_copy.as_ref() else { continue };
                    let g = &dc.wrap;
                    if m.dist.is_empty() {
                        continue;
                    }
                    drop(pass);
                    if !squeeze(encoder) {
                        pass = new_pass(encoder);
                        continue;
                    }
                    pass = new_pass(encoder);
                    class = blend_class(c.blend, false).unwrap_or(class);
                    pass.set_pipeline(&self.distmodel_pipes[&(class, c.flags & 0x1000 != 0)]);
                    bound = None;
                    pass.set_bind_group(1, g, &[]);
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    for r in &m.dist {
                        pass.draw(r.clone(), base..base + 1);
                    }
                }
                // FUN_10043590: the scene squeezed into the 256x256 copy (FUN_10042d00), then the morph's triangles over
                // it (WRAP; no texture).
                Draw::DistMorph(id) => {
                    let Some(m) = fx.reg.morph_data_of(id) else { continue };
                    let Some(dc) = self.dist_copy.as_ref() else { continue };
                    let g = &dc.wrap;
                    drop(pass);
                    if !squeeze(encoder) {
                        pass = new_pass(encoder);
                        continue;
                    }
                    pass = new_pass(encoder);
                    class = blend_class(c.blend, false).unwrap_or(class);
                    pass.set_pipeline(&self.distmorph_pipes[&(class, c.flags & 0x1000 != 0)]);
                    bound = None;
                    pass.set_bind_group(1, g, &[]);
                    pass.draw(0..m.t * 3, base..base + 1);
                }
                // CMoD3mSpecularElem: one pass with the specular stage; no 'I' blend, per-run variants (FUN_100412b0).
                Draw::Mesh(id)
                    if c.spec.is_some()
                        && self.meshes.get(id as usize).is_some_and(|m| m.as_ref().is_some_and(|m| !m.mmb)) =>
                {
                    let s = c.spec.unwrap();
                    let (Some(Some(m)), Some(sg)) =
                        (self.meshes.get(id as usize), self.spec_groups.get(&(s.tex, s.cube, s.clamp)))
                    else {
                        continue;
                    };
                    class = blend_class(c.blend, false).unwrap_or(class);
                    let key = (false, class + 32, c.flags & 0x1000 != 0);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.spec_pipes[&(class, key.2)]);
                        bound = Some(key);
                    }
                    pass.set_bind_group(2, sg, &[]);
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    let grouped = m.groups.iter().any(|g| g.2);
                    for (tex, range, named) in &m.groups {
                        let variant = if !grouped {
                            11
                        } else if *named {
                            10
                        } else {
                            12
                        };
                        pass.set_bind_group(1, tex.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                        pass.draw(range.clone(), base + variant..base + variant + 1);
                    }
                }
                Draw::Mesh(id) => {
                    let Some(Some(m)) = self.meshes.get(id as usize) else { continue };
                    class = blend_class(c.blend, !m.mmb).unwrap_or(class);
                    let key = (false, class, c.flags & 0x1000 != 0);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.mesh_pipes[&(class, key.2)]);
                        bound = Some(key);
                    }
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    for (tex, range, _) in &m.groups {
                        let variant = if m.mmb { 2 } else { 0 } + tex.is_none() as u32;
                        pass.set_bind_group(1, tex.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                        pass.draw(range.clone(), base + variant..base + variant + 1);
                    }
                }
                Draw::Sprite(id) if fx.reg.sprite_is_flare(id) => {
                    pass.set_pipeline(&self.flare_pipe);
                    bound = None;
                    let r = fx.reg.sprites[id as usize];
                    let tex = fx.reg.libs[r.0].sprites.get(&r.1).and_then(|s| s.texture);
                    pass.set_bind_group(1, tex.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                    pass.draw(0..6 * fx.reg.sprite_at[id as usize].1, base..base + 1);
                }
                Draw::Sprite(id) => {
                    class = blend_class(c.blend, false).unwrap_or(class);
                    let key = (true, class, false);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.sprite_pipes[&class]);
                        bound = Some(key);
                    }
                    let r = fx.reg.sprites[id as usize];
                    let tex = fx.reg.libs[r.0].sprites.get(&r.1).and_then(|s| s.texture);
                    pass.set_bind_group(1, tex.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                    let quads = fx.reg.sprite_at[id as usize].1;
                    match c.parts {
                        Some((_, n)) => pass.draw(0..6, base..base + n.min(PER_ELEM)),
                        None => pass.draw(0..6 * quads, base..base + 1),
                    }
                }
                Draw::Morph(id) => {
                    let Some(m) = fx.reg.morph_data_of(id) else { continue };
                    class = blend_class(c.blend, false).unwrap_or(class);
                    let key = (true, class + 8, c.flags & 0x1000 != 0);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.morph_pipes[&(class, key.2)]);
                        bound = Some(key);
                    }
                    pass.set_bind_group(1, m.texture.and_then(|t| self.textures.get(&t)).unwrap_or(&self.white), &[]);
                    pass.draw(0..m.t * 3, base..base + 1);
                }
                // Up to 3 ring pairs of 2S triangles (S <= 255); vs_ring hides the rest.
                Draw::Ring => {
                    class = blend_class(c.blend, true).unwrap_or(class);
                    let key = (true, class + 24, c.flags & 0x1000 != 0);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.ring_pipes[&(class, key.2)]);
                        bound = Some(key);
                    }
                    pass.set_bind_group(1, &self.white, &[]);
                    pass.draw(0..3 * 2 * 255 * 3, base..base + 1);
                }
                Draw::DamValue => {
                    let Some((g, _)) = self.dmgnum.as_ref() else { continue };
                    class = blend_class(c.blend, false).unwrap_or(class);
                    let key = (true, class + 16, false);
                    if bound != Some(key) {
                        pass.set_pipeline(&self.damv_pipes[&class]);
                        bound = Some(key);
                    }
                    pass.set_bind_group(1, g, &[]);
                    pass.draw(0..6 * 16, base..base + 1);
                }
                Draw::None => {}
            }
        }
        // FUN_100125c0: the flares' glare over the whole view, once the effects are drawn.
        if !flares.is_empty() {
            pass.set_pipeline(&self.glare_pipe);
            pass.set_bind_group(1, &self.white, &[]);
            for e in flares {
                pass.draw(0..3, e * PER_ELEM..e * PER_ELEM + 1);
            }
        }
        next_break
    }
}

/// The next break (from `next`) farther than an item at `key`, if any: breaks are back to front, so it is `next`
/// when its key is larger.
fn break_before(breaks: &[f32], next: usize, key: f32) -> Option<usize> {
    breaks.get(next).filter(|&&b| b > key).map(|_| next)
}

#[cfg(test)]
mod tests {
    /// A faded actor at view depth 10 between effects at 30, 12 (behind it) and 8, 2 (in front): it goes after the
    /// farther two and before the nearer two; a second one at 1 goes after all of them (the caller draws it).
    #[test]
    fn breaks_interleave_by_depth() {
        let items = [30.0f32, 12.0, 8.0, 2.0];
        let breaks = [10.0f32, 1.0];
        let (mut next, mut out) = (0usize, Vec::new());
        for (k, &key) in items.iter().enumerate() {
            while let Some(i) = super::break_before(&breaks, next, key) {
                out.push(format!("actor{i}"));
                next = i + 1;
            }
            out.push(format!("fx{k}"));
        }
        assert_eq!(out, ["fx0", "fx1", "actor0", "fx2", "fx3"]);
        assert_eq!(next, 1);
    }

    #[test]
    fn vu_dist_weather_density() {
        use super::vu_dist_count;
        assert_eq!(vu_dist_count(19, true, 0.0), 19);
        assert_eq!(vu_dist_count(40, false, 0.0), 40);
        assert_eq!(vu_dist_count(40, true, 1.0), 20);
        assert_eq!(vu_dist_count(41, true, 1.0), 20);
        assert_eq!(vu_dist_count(40, true, 0.0), 0);
    }

    use super::*;
    use crate::fx::elem::{Elem, update_ref};

    /// FUN_10042d00's copy pass (render/squeeze.wgsl) against a CPU reference: a 320x200 source whose texel (x, y)
    /// holds (x, y, 0), squeezed into 256x256; texel (i, j) is the linear, MIRROR sample at u = (i W / 256 - 0.5) / W,
    /// v = (H - 0.5) / H - j / 256 (so row 0 holds the bottom of the screen), alpha 0x80.
    #[test]
    fn squeeze_matches_retail_mapping() {
        let Ok(gpu) = Gpu::headless() else { return };
        let (w, h) = (320u32, 200u32);
        let fmt = wgpu::TextureFormat::Rgba8Unorm;
        let src = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texel = |x: u32, y: u32| [(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 0u8, 255u8];
        let px: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| texel(x, y))).collect();
        gpu.queue.write_texture(
            src.as_image_copy(),
            &px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            src.size(),
        );
        let eg = EffectsGpu::new(&gpu, fmt);
        let view = src.create_view(&Default::default());
        let g = EffectsGpu::tex_group(&gpu.device, &eg.tex_layout, &eg.copy_sampler, &view);
        let dst = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let dv = dst.create_view(&Default::default());
        let pipe = EffectsGpu::squeeze_pipe(&gpu.device, &eg.tex_layout, fmt);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(&pipe);
            p.set_bind_group(0, &g, &[]);
            p.draw(0..3, 0..1);
        }
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 4 * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            dst.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(256) },
            },
            dst.size(),
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let out = buf.slice(..).get_mapped_range().unwrap().to_vec();
        // Bilinear with MIRROR addressing, per channel.
        let mirror = |k: i32, n: i32| {
            let m = k.rem_euclid(2 * n);
            if m < n { m } else { 2 * n - 1 - m }
        };
        let sample = |u: f32, v: f32, c: usize| {
            let (x, y) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let at = |dx: i32, dy: i32| {
                let t = texel(mirror(x0 as i32 + dx, w as i32) as u32, mirror(y0 as i32 + dy, h as i32) as u32);
                t[c] as f32
            };
            (at(0, 0) * (1.0 - fx) + at(1, 0) * fx) * (1.0 - fy) + (at(0, 1) * (1.0 - fx) + at(1, 1) * fx) * fy
        };
        for (i, j) in [(0u32, 0u32), (1, 1), (128, 128), (255, 255), (37, 200), (200, 17)] {
            let u = (i as f32 * w as f32 / 256.0 - 0.5) / w as f32;
            let v = (h as f32 - 0.5) / h as f32 - j as f32 / 256.0;
            let o = (j * 1024 + i * 4) as usize;
            for c in 0..2 {
                let want = sample(u, v, c);
                assert!(
                    (out[o + c] as f32 - want).abs() <= 2.0,
                    "texel ({i}, {j}) channel {c}: {} vs {want}",
                    out[o + c]
                );
            }
            assert_eq!(out[o + 3], 128, "alpha at ({i}, {j})");
        }
    }

    impl EffectsGpu {
        fn read_elems(&self, gpu: &Gpu, n: usize) -> Vec<u32> {
            let size = (n * w::WORDS * 4) as u64;
            let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.elems, 0, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec()
        }
    }

    impl EffectsGpu {
        fn read_back(&self, gpu: &Gpu, n: usize) -> Vec<u32> {
            let size = (n * BACK_WORDS * 4) as u64;
            let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.back, 0, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec()
        }
    }

    impl EffectsGpu {
        fn read_outs(&self, gpu: &Gpu, n: usize) -> Vec<f32> {
            let size = (n * OUT_WORDS * 4) as u64;
            let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.outs, 0, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec()
        }
    }

    /// A timed element spawned this frame (ctl::BORN) is updated but not drawn: its t = 0 sample is the key's
    /// placeholder (Heavens Tower's waterfall sheets flashed at full strength each respawn). It shows from the next
    /// frame; an element without a life (t stays 0) shows at once, and so does a one-tick element (its only frame).
    #[test]
    fn gpu_born_hidden_one_frame() {
        let Ok(gpu) = Gpu::headless() else { return };
        let eye = glam::Vec3::new(0.0, 0.0, -10.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, glam::Vec3::NEG_Y);
        let mut view_rot = view;
        view_rot.w_axis = glam::Vec4::W;
        let f = Frame {
            dt: 1.0,
            seconds: 1.0 / 60.0,
            dayfrac: 0.5,
            view,
            view_rot,
            eye,
            draw_distance: 2000.0,
            area_light: glam::Vec3::ONE,
            focal: 350.0,
            moon_phase: 0,
            weekday: 0,
        };
        let mut fx = Effects::new();
        fx.reg.ops = vec![0];
        fx.reg.ops_dirty = true;
        for (k, life) in [60.0f32, 0.0, 1.0].into_iter().enumerate() {
            let mut el = Elem::default();
            el.0[w::STATE] = crate::fx::elem::st::ALIVE;
            el.0[w::COLOR] = 0x8080_8080;
            el.0[w::STREAM] = 0;
            el.0[w::SERIAL] = 1 + k as u32;
            el.0[w::PARENT] = crate::fx::elem::NONE;
            el.set(w::LIFE, life);
            el.set(w::MAX_LIFE, life);
            el.0[w::CTRL] = crate::fx::elem::ctl::LIFE_SET | crate::fx::elem::ctl::BORN;
            fx.writes.push(Write::Full(k as u32, Box::new(el)));
            fx.elems.push(None);
        }
        let mut eg = EffectsGpu::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut shown = Vec::new();
        for _ in 0..2 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            eg.update(&gpu, &mut enc, &mut fx, &f);
            gpu.queue.submit([enc.finish()]);
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let o = eg.read_outs(&gpu, 3);
            shown.push((o[23], o[OUT_WORDS + 23], o[2 * OUT_WORDS + 23]));
        }
        // The one-tick element expires on the GPU's second update (life 0).
        assert_eq!(shown, vec![(0.0, 1.0, 1.0), (1.0, 1.0, 0.0)]);
    }

    /// Update ops 0x53 and 0x60 as Tavnazian Safehold's thunder flash (t_ta/weat/thdr/wez4/frsh) carries them: an element 10 in
    /// front of the eye on the screen centre adds its colour to the flash when the scene depth leaves it visible, and
    /// nothing when the scene hides it (op 0x53 zeroes FADE, so op 0x60 skips).
    #[test]
    fn gpu_thunder_flash() {
        // One device and one EffectsGpu for every case: EffectsGpu::new compiles every effects pipeline, the bulk of
        // the test's time (once per case, this test took minutes on some drivers).
        let Ok(gpu) = Gpu::headless() else { return };
        let mut eg = EffectsGpu::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let eye = glam::Vec3::new(0.0, 0.0, -10.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, glam::Vec3::NEG_Y);
        let mut view_rot = view;
        view_rot.w_axis = glam::Vec4::W;
        let f = Frame {
            dt: 1.0,
            seconds: 1.0 / 60.0,
            dayfrac: 0.5,
            view,
            view_rot,
            eye,
            draw_distance: 2000.0,
            area_light: glam::Vec3::ONE,
            focal: 350.0,
            moon_phase: 0,
            weekday: 0,
        };
        let mut flash =
            |clear: f32, far_key: f32| flash_at(&gpu, &mut eg, f, (glam::Vec3::ZERO, 0), clear, (50.0, far_key));
        // Visible: B, G, R bytes of the colour at full strength.
        assert_eq!(flash(1.0, 150.0), 0x00c0_8040);
        // Behind scenery: nothing.
        assert_eq!(flash(0.0, 150.0), 0);
        // Beyond the far distance (10 > 5): nothing.
        assert_eq!(flash(1.0, 5.0), 0);
        // Camera-relative (flags bit 2, the Riverne bolts): the offset turned by the view rotation, wherever the eye is;
        // the eye distance still takes the offset as a world point (about 100.5 here, inside near 200).
        let eye = glam::Vec3::new(100.0, 0.0, 0.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, eye + glam::Vec3::Z, glam::Vec3::NEG_Y);
        let mut view_rot = view;
        view_rot.w_axis = glam::Vec4::W;
        let g = Frame { view, view_rot, eye, ..f };
        let mut flash = |pos: glam::Vec3| flash_at(&gpu, &mut eg, g, (pos, 4), 1.0, (200.0, 300.0));
        assert_eq!(flash(glam::Vec3::new(0.0, 0.0, 10.0)), 0x00c0_8040);
        assert_eq!(flash(glam::Vec3::new(0.0, 0.0, -10.0)), 0, "behind the eye");
    }

    /// One element with Tavnazian Safehold's frsh ops (op 0x53, then op 0x60 with near / far) at `pos` with `flags`,
    /// over a scene depth cleared to `clear`: its readback flash word after one update of `eg`. The element is written
    /// whole (slot 0) each call, so nothing carries over from an earlier call.
    fn flash_at(
        gpu: &Gpu,
        eg: &mut EffectsGpu,
        f: Frame,
        (pos, flags): (glam::Vec3, u32),
        clear: f32,
        (near, far): (f32, f32),
    ) -> u32 {
        let f32w = |v: f32| v.to_bits();
        let proj = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 1000.0);
        {
            let mut fx = Effects::new();
            fx.reg.ops = vec![
                0x353,
                f32w(8.0),
                f32w(1.0),
                0x760,
                f32w(near),
                f32w(far),
                f32w(4096.0),
                f32w(65536.0),
                f32w(0.0),
                1,
                0,
            ];
            fx.reg.ops_dirty = true;
            let mut el = Elem::default();
            el.0[w::STATE] = crate::fx::elem::st::ALIVE;
            el.0[w::COLOR] = 0xffc0_8040;
            el.0[w::STREAM] = 0;
            el.0[w::SERIAL] = 1;
            el.0[w::FLAGS] = flags;
            el.set_v3(w::POS, pos);
            fx.writes.push(Write::Full(0, Box::new(el)));
            fx.elems.push(None);
            let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::render::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let dv = depth.create_view(&Default::default());
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &dv,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            eg.set_depth(Some((&dv, proj)));
            eg.update(gpu, &mut enc, &mut fx, &f);
            gpu.queue.submit([enc.finish()]);
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            eg.read_back(gpu, 1)[crate::fx::elem::back::FLASH]
        }
    }

    /// Port San d'Oria's auto-start generators (sea, water tint, lamps) run for a while on the GPU and through
    /// the CPU reference of the update stream; every live element must agree.
    #[test]
    /// Op 0x6e on the GPU (MODLOG "Lit D3b / D3m effect elements"): DAT 129 `d_ba/effe/warp/aw01/gr01` at 5 /
    /// 12.5 / 18 from the eye: (w0, w1) = (0, 1), (0.5, 0.5), (1, 0).
    fn gpu_morph_crossfade() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let Ok(gpu) = Gpu::headless() else { return };
        let at = glam::Vec3::new(734.682, -4.18383, -703.631);
        for (d, want) in [(5.0, 0.0), (12.5, 0.5), (18.0, 1.0)] {
            let mut fx = Effects::new();
            fx.set_keys(Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
            fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
            fx.load_zone(crate::fx::lib::Library::load(129, index.read(129).unwrap()).unwrap());
            let mut eg = EffectsGpu::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
            let eye = at + glam::Vec3::new(0.0, 0.0, d);
            let view = glam::camera::rh::view::look_at_mat4(eye, at, glam::Vec3::NEG_Y);
            let mut view_rot = view;
            view_rot.w_axis = glam::Vec4::W;
            let f = Frame {
                dt: 1.0,
                seconds: 1.0 / 60.0,
                dayfrac: 0.5,
                view,
                view_rot,
                eye,
                draw_distance: 2000.0,
                area_light: glam::Vec3::ONE,
                focal: 350.0,
                moon_phase: 0,
                weekday: 0,
            };
            for _ in 0..4 {
                fx.tick(&f, &crate::fx::NoActors);
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                eg.update(&gpu, &mut enc, &mut fx, &f);
                gpu.queue.submit([enc.finish()]);
                gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
                eg.after_submit(&gpu, &mut fx);
            }
            let g = eg.read_elems(&gpu, fx.elems.len());
            let gr01: Vec<usize> = fx
                .elems
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.as_ref().is_some_and(|c| {
                        fx.insts.get(c.inst).and_then(|x| x.as_ref()).is_some_and(|x| x.name == *b"gr01")
                    })
                })
                .map(|(e, _)| e)
                .collect();
            let near = gr01
                .iter()
                .map(|&e| {
                    let gw = &g[e * w::WORDS..(e + 1) * w::WORDS];
                    let p = glam::Vec3::new(
                        f32::from_bits(gw[w::POS]),
                        f32::from_bits(gw[w::POS + 1]),
                        f32::from_bits(gw[w::POS + 2]),
                    );
                    ((p - at).length(), f32::from_bits(gw[w::MORPH_W]), f32::from_bits(gw[w::MORPH_W + 1]))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, w0, w1)) = near else { panic!("no gr01 element") };
            assert!((w0 - want).abs() < 0.05 && (w1 - (1.0 - want)).abs() < 0.05, "{d}: ({w0}, {w1})");
        }
    }

    #[test]
    fn gpu_update_matches_cpu() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let Ok(gpu) = Gpu::headless() else { return };
        let mut fx = Effects::new();
        fx.set_keys(Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(crate::fx::lib::Library::load(332, index.read(332).unwrap()).unwrap());
        let mut eg = EffectsGpu::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut cpu: HashMap<u32, Elem> = HashMap::new();
        for tick in 0..40 {
            let eye = glam::Vec3::new(20.0 + tick as f32 * 3.0, -12.0, -40.0 + tick as f32 * 9.0);
            let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::new(20.0, 0.0, 60.0), glam::Vec3::NEG_Y);
            let mut view_rot = view;
            view_rot.w_axis = glam::Vec4::W;
            let f = Frame {
                dt: 1.37,
                seconds: 1.37 / 60.0,
                dayfrac: 0.2 + tick as f32 * 0.01,
                view,
                view_rot,
                eye,
                draw_distance: 2000.0,
                area_light: glam::Vec3::ONE,
                focal: 350.0,
                moon_phase: 0,
                weekday: 0,
            };
            fx.tick(&f, &crate::fx::NoActors);
            for wr in &fx.writes {
                match wr {
                    Write::Full(e, el) => {
                        cpu.insert(*e, (**el).clone());
                    }
                    Write::Word(e, off, v) => {
                        if let Some(c) = cpu.get_mut(e) {
                            c.0[*off] = *v;
                        }
                    }
                    Write::Parts(..) | Write::LightOff(_) => {}
                }
            }
            let slots: Vec<u32> = fx.slots.iter().flatten().copied().collect();
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            eg.update(&gpu, &mut enc, &mut fx, &f);
            gpu.queue.submit([enc.finish()]);
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            eg.after_submit(&gpu, &mut fx);
            let reg = &fx.reg;
            let key = |id: u32, x: f32, eased: bool, v0: Option<f32>| -> f32 {
                let Some(d) = reg.key_desc.get(id as usize) else { return 0.0 };
                let k = crate::dat::generator::KeyFrame {
                    keys: reg.key_data[d[0] as usize..(d[0] + d[1]) as usize].iter().map(|p| (p[0], p[1])).collect(),
                };
                match v0 {
                    Some(v) => k.sample_v0(x, eased, v),
                    None => k.sample(x, eased),
                }
            };
            for c in cpu.values_mut() {
                update_ref(c, &reg.ops, &key, &slots, &f);
            }
        }
        let n = fx.elems.len();
        let g = eg.read_elems(&gpu, n);
        let mut checked = 0;
        for (e, c) in &cpu {
            if c.0[w::STATE] & crate::fx::elem::st::ALIVE == 0 {
                continue;
            }
            let gw = &g[*e as usize * w::WORDS..(*e as usize + 1) * w::WORDS];
            let gf = |k: usize| f32::from_bits(gw[k]);
            for k in [
                w::POS,
                w::POS + 1,
                w::POS + 2,
                w::ROT,
                w::ROT + 1,
                w::ROT + 2,
                w::SCALE,
                w::SCALE + 1,
                w::SCALE + 2,
                w::FADE,
                w::UV,
                w::UV + 1,
                w::LIFE,
                w::T,
            ] {
                assert!(
                    (gf(k) - c.f(k)).abs() <= 1e-3 * c.f(k).abs().max(1.0),
                    "element {e} word {k}: gpu {} cpu {}",
                    gf(k),
                    c.f(k)
                );
            }
            assert_eq!(gw[w::COLOR], c.0[w::COLOR], "element {e} colour");
            checked += 1;
        }
        println!("{checked} elements agree");
        assert!(checked > 20, "only {checked} elements");
    }

    /// Buburimu's fish (DAT 218 gyl1 / gyb1: no velocity, only the op 0x29 / 0x2b sway) swim on the GPU as the CPU
    /// reference has them.
    #[test]
    fn gpu_fish_sway() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let Ok(gpu) = Gpu::headless() else { return };
        let mut fx = Effects::new();
        fx.set_keys(Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(crate::fx::lib::Library::load(218, index.read(218).unwrap()).unwrap());
        let mut eg = EffectsGpu::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut cpu: HashMap<u32, Elem> = HashMap::new();
        let mut start: HashMap<u32, glam::Vec3> = HashMap::new();
        let eye = glam::Vec3::new(460.0, 15.0, -140.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::new(470.0, 22.0, -118.0), glam::Vec3::NEG_Y);
        let mut view_rot = view;
        view_rot.w_axis = glam::Vec4::W;
        let f = Frame {
            dt: 2.0,
            seconds: 2.0 / 60.0,
            dayfrac: 0.4,
            view,
            view_rot,
            eye,
            draw_distance: 2000.0,
            area_light: glam::Vec3::ONE,
            focal: 350.0,
            moon_phase: 0,
            weekday: 0,
        };
        for _ in 0..90 {
            fx.tick(&f, &crate::fx::NoActors);
            for wr in &fx.writes {
                match wr {
                    Write::Full(e, el) => {
                        cpu.insert(*e, (**el).clone());
                        start.insert(*e, el.v3(w::POS));
                    }
                    Write::Word(e, off, v) => {
                        if let Some(c) = cpu.get_mut(e) {
                            c.0[*off] = *v;
                        }
                    }
                    Write::Parts(..) | Write::LightOff(_) => {}
                }
            }
            let slots: Vec<u32> = fx.slots.iter().flatten().copied().collect();
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            eg.update(&gpu, &mut enc, &mut fx, &f);
            gpu.queue.submit([enc.finish()]);
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            eg.after_submit(&gpu, &mut fx);
            let reg = &fx.reg;
            let key = |id: u32, x: f32, eased: bool, v0: Option<f32>| -> f32 {
                let Some(d) = reg.key_desc.get(id as usize) else { return 0.0 };
                let k = crate::dat::generator::KeyFrame {
                    keys: reg.key_data[d[0] as usize..(d[0] + d[1]) as usize].iter().map(|p| (p[0], p[1])).collect(),
                };
                match v0 {
                    Some(v) => k.sample_v0(x, eased, v),
                    None => k.sample(x, eased),
                }
            };
            for c in cpu.values_mut() {
                update_ref(c, &reg.ops, &key, &slots, &f);
            }
        }
        let g = eg.read_elems(&gpu, fx.elems.len());
        let named = |e: u32, n: &[u8; 4]| {
            fx.elems
                .get(e as usize)
                .and_then(|c| c.as_ref())
                .and_then(|c| fx.insts.get(c.inst))
                .and_then(|x| x.as_ref())
                .is_some_and(|x| x.name == *n)
        };
        for n in [b"gyl1", b"gyb1"] {
            let fish: Vec<u32> = cpu.keys().copied().filter(|&e| named(e, n)).collect();
            assert!(!fish.is_empty(), "no {} element", String::from_utf8_lossy(n));
            for e in fish {
                let gw = &g[e as usize * w::WORDS..(e as usize + 1) * w::WORDS];
                let gp = glam::Vec3::new(
                    f32::from_bits(gw[w::POS]),
                    f32::from_bits(gw[w::POS + 1]),
                    f32::from_bits(gw[w::POS + 2]),
                );
                let cp = cpu[&e].v3(w::POS);
                assert!((gp - cp).length() < 1e-2, "{}: gpu {gp} cpu {cp}", String::from_utf8_lossy(n));
                assert!(
                    (gp - start[&e]).length() > 0.3,
                    "{} did not move: {} -> {gp}",
                    String::from_utf8_lossy(n),
                    start[&e]
                );
            }
        }
    }
}

#[cfg(test)]
mod soften_tests {
    use super::soften_alpha;
    use crate::dat::zone::Texture;

    /// A dithered 4-level ring (as the lamps' 'light' DXT3 texture) comes out with smooth alpha and no black edge.
    #[test]
    fn dithered_ring_softens() {
        let (w, h) = (16u32, 16u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
                let a = if d < 3.0 {
                    102
                } else if d < 5.0 && (x + y) % 2 == 0 {
                    34
                } else {
                    0
                };
                rgba.extend_from_slice(&if a > 0 { [240, 200, 120, a] } else { [0, 0, 0, 0] });
            }
        }
        let t = Texture::new("light".into(), 0, w, h, rgba);
        let s = soften_alpha(&t).expect("few levels");
        let levels: std::collections::BTreeSet<u8> = s.rgba().chunks(4).map(|p| p[3]).collect();
        assert!(levels.len() > 8, "{levels:?}");
        // Texels just outside the old edge took the glow's colour, not black.
        let i = ((7 * w + 13) * 4) as usize;
        assert!(s.rgba()[i] > 200 && s.rgba()[i + 3] > 0, "{:?}", &s.rgba()[i..i + 4]);
        // Many alpha levels already: left alone.
        let smooth: Vec<u8> = (0..w * h).flat_map(|k| [9, 9, 9, (k % 200) as u8]).collect();
        assert!(soften_alpha(&Texture::new("s".into(), 0, w, h, smooth)).is_none());
    }

    /// East Sarutabaruta's river wash keeps its clear specks; East Ronfaure's lamp glow still softens.
    #[test]
    fn speckled_wash_kept() {
        let Ok(index) = crate::dat::DatIndex::open_stock(&crate::config::Config::load().ffxi_dir()) else { return };
        let tex = |zone: u32, name: &str| {
            let id = crate::dat::zone::file_id(zone);
            let mut lib = crate::fx::lib::Library::load(id, index.read(id).unwrap()).unwrap();
            let k = lib.img_named(name).and_then(|i| lib.texture_at(i)).unwrap_or_else(|| panic!("{name}"));
            lib.textures.swap_remove(k)
        };
        assert!(soften_alpha(&tex(116, "effect  kaw1")).is_none());
        assert!(soften_alpha(&tex(101, "light   light")).is_some());
    }
}
