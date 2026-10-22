//! wgpu renderer for zone scenery.

pub mod anim;
pub mod mipgen;
pub mod upload;
pub mod character;
pub mod dancer;
pub mod ui;
pub mod screens;
pub mod invwin;
pub mod screenfx;
pub mod scale;

use crate::dat::zone::ZoneScene;
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Mip jobs waiting for a frame (render/mipgen.rs) and this frame's copy budget (render/upload.rs).
    pub mips: std::sync::Mutex<mipgen::MipGen>,
    pub uploads: std::sync::Mutex<upload::Budget>,
    pub pipes: std::sync::Arc<PipeCache>,
}

/// Pipelines made once per device, by any thread (Gpu::warm makes them at start-up).
pub struct PipeCache {
    device: wgpu::Device,
    mips: std::sync::OnceLock<mipgen::Pipes>,
    anim: std::sync::OnceLock<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    zone: std::sync::Mutex<HashMap<wgpu::TextureFormat, ZonePipes>>,
}

impl PipeCache {
    pub fn mips(&self) -> &mipgen::Pipes {
        self.mips.get_or_init(|| mipgen::Pipes::new(&self.device))
    }

    pub fn anim(&self) -> &(wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        self.anim.get_or_init(|| anim::pipeline(&self.device))
    }

    pub fn zone(&self, format: wgpu::TextureFormat) -> ZonePipes {
        let mut z = self.zone.lock().unwrap_or_else(|e| e.into_inner());
        z.entry(format).or_insert_with(|| ZonePipes::new(&self.device, format)).clone()
    }
}

/// The GPUs that can draw to `surface` (or any, headless), by name.
pub fn adapter_names(instance: &wgpu::Instance, surface: Option<&wgpu::Surface>) -> Vec<String> {
    let mut names: Vec<String> = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .map(|a| a.get_info().name)
        .collect();
    names.dedup();
    names
}

impl Gpu {
    pub fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface>) -> Result<Self> {
        Self::with_adapter(instance, surface, "")
    }

    /// The adapter named `preferred` when it can drive the surface; otherwise (or empty) the high-performance one.
    pub fn with_adapter(instance: wgpu::Instance, surface: Option<&wgpu::Surface>, preferred: &str) -> Result<Self> {
        let named = (!preferred.is_empty())
            .then(|| pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all())).into_iter().find(|a| a.get_info().name == preferred && surface.is_none_or(|s| a.is_surface_supported(s))))
            .flatten();
        if !preferred.is_empty() && named.is_none() {
            log::warn!("GPU {preferred:?} not found or can't drive the window; using the default");
        }
        let adapter = match named {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .context("no suitable GPU adapter")?,
        };
        log::info!("GPU: {}", adapter.get_info().name);
        // BC textures go up compressed when the adapter has them (zone and model DXT1 / DXT3).
        let required_features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() }))?;
        let pipes = std::sync::Arc::new(PipeCache { device: device.clone(), mips: Default::default(), anim: Default::default(), zone: Default::default() });
        Ok(Self { instance, adapter, device, queue, mips: Default::default(), uploads: Default::default(), pipes })
    }

    /// Makes the shared pipelines on a worker thread, so no frame compiles them later.
    pub fn warm(&self, formats: &[wgpu::TextureFormat]) {
        let (pipes, formats) = (self.pipes.clone(), formats.to_vec());
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            pipes.mips();
            pipes.anim();
            for f in formats {
                pipes.zone(f);
            }
            log::info!("pipelines ready in {:.0} ms", t.elapsed().as_secs_f32() * 1000.0);
        });
    }

    /// Records waiting mip jobs, up to `budget` level-0 texels.
    pub fn run_mips(&self, encoder: &mut wgpu::CommandEncoder, budget: u64) {
        if let Ok(mut m) = self.mips.lock()
            && m.pending()
        {
            m.run(&self.device, self.pipes.mips(), encoder, budget);
        }
    }

    /// Every waiting mip job, submitted now.
    pub fn flush_mips(&self) {
        if self.mips.lock().is_ok_and(|m| m.pending()) {
            let mut enc = self.device.create_command_encoder(&Default::default());
            self.run_mips(&mut enc, u64::MAX);
            self.queue.submit([enc.finish()]);
        }
    }

    pub fn mips_done(&self) -> u64 {
        self.mips.lock().map_or(0, |m| m.done())
    }

    pub fn headless() -> Result<Self> {
        Self::new(wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()), None)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    eye: [f32; 4],
    inv_view_proj: [[f32; 4]; 4],
    // Zone lighting (dat/weather.rs); sun[3] = 1 when enabled.
    sun: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    actor_sun: [f32; 4],
    actor_color: [f32; 4],
    actor_ambient: [f32; 4],
    sky_info: [f32; 4],
    clear: [f32; 4],
    sky_colors: [[f32; 4]; 8],
    sky_stops: [[f32; 4]; 2],
}

/// One scenery model: a range of the shared index buffer, its base vertex in the shared vertex buffer.
#[derive(Clone)]
struct Draw {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    instances: std::ops::Range<u32>,
    texture: usize,
    alpha_test: bool,
}

#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
}

impl Camera {
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        use glam::camera::rh::{proj::directx, view};
        directx::perspective(self.fov_y, aspect, 0.5, 2000.0) * view::look_at_mat4(self.eye, self.target, self.up)
    }
}

pub struct ZoneRenderer {
    pipeline: wgpu::RenderPipeline,
    alpha_pipeline: wgpu::RenderPipeline,
    actor_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    globals: wgpu::BindGroup,
    textures: Vec<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    /// All scenery in one vertex and one index buffer, uploaded once; draws keep the file order.
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draws: Vec<Draw>,
    skin_pipeline: wgpu::RenderPipeline,
    pose_layout: wgpu::BindGroupLayout,
    /// This frame's pose regions (256-byte aligned), and the buffer they go up in with its bind group.
    poses: Vec<[f32; 4]>,
    pose_gpu: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    pose_frame: u64,
    /// Motions evaluated on the GPU into the pose regions (render/anim.rs).
    anim: anim::GpuAnim,
    /// Skinned models on the GPU by model address (the Arc keeps it), and how many more this frame may add.
    models: HashMap<usize, (std::sync::Arc<crate::dat::character::Character>, ActorGpu)>,
    /// Models going up piece by piece, oldest first, and the magenta slot-0 texture of actors.
    builds: Vec<(usize, Build)>,
    magenta: wgpu::BindGroup,
    /// A streamed zone (the world's): mips wait for the frame budget, textures and geometry still to go up
    /// (geometry by byte offset), the last mip job of its textures, and (bytes before, RGBA bytes, count) for the log.
    budgeted: bool,
    pending: std::collections::VecDeque<(usize, crate::dat::zone::Texture, Option<upload::TexUp>)>,
    geo_pending: Option<(std::sync::Arc<ZoneGeometry>, u64)>,
    mips_seq: u64,
    tex_bytes: (u64, u64, usize),
    /// The target is not sRGB, so the shader applies gamma itself.
    encode_gamma: bool,
    pub fog_color: [f32; 3],
    /// Clear the colour target to the fog colour first (off to draw over something, e.g. the lobby art).
    pub clear_color: bool,
    /// Retail D3D8 lighting from the zone's weather keys; None keeps the placeholder shading.
    pub lighting: Option<crate::dat::weather::Lighting>,
}

/// Anisotropic filtering for scenery, actor and creator samplers (config `anisotropy`, 1 = off).
pub static ANISOTROPY: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(16);

/// Config-tool 0000 (MIP mapping: D3DTSS_MIPFILTER LINEAR, else NONE = level 0 only) and 0018 = 2 (textures
/// uploaded uncompressed); read when samplers and textures are made, so at the next zone load.
pub static MIPMAPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub static UNCOMPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// FFXI_TEX_OLD=1: the old upload (RGBA level 0 only, bilinear), for before / after shots.
fn old_textures() -> bool {
    static OLD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OLD.get_or_init(|| std::env::var_os("FFXI_TEX_OLD").is_some())
}

/// Trilinear sampler with the configured anisotropy, repeating (scenery, actors, the creator).
pub fn mip_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    if old_textures() {
        return device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
    }
    device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        lod_max_clamp: if MIPMAPS.load(std::sync::atomic::Ordering::Relaxed) { 32.0 } else { 0.0 },
        anisotropy_clamp: ANISOTROPY.load(std::sync::atomic::Ordering::Relaxed).clamp(1, 16),
        ..Default::default()
    })
}

/// Bytes of texture memory uploaded so far (the F12 overlay and zone logs).
pub static TEXTURE_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A texture with its full mip chain; see upload_texture_seq.
pub fn upload_texture(gpu: &Gpu, t: &crate::dat::zone::Texture, srgb: bool) -> wgpu::TextureView {
    upload_texture_seq(gpu, t, srgb).0
}

/// Level 0 goes up as stored: the file's DXT1 / DXT3 blocks as BC1 / BC2 when the device samples them (both
/// sides a multiple of 4), else RGBA. The lower levels are made on the GPU (render/mipgen.rs); returns the view
/// and the mip job to wait for.
pub fn upload_texture_seq(gpu: &Gpu, t: &crate::dat::zone::Texture, srgb: bool) -> (wgpu::TextureView, u64) {
    use wgpu::TextureFormat as F;
    let (w, h) = (t.width, t.height);
    let pick = |s: F, u: F| if srgb { s } else { u };
    let desc = |format, levels| wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    if old_textures() {
        TEXTURE_BYTES.fetch_add(t.rgba.len() as u64, std::sync::atomic::Ordering::Relaxed);
        let texture = gpu.device.create_texture_with_data(&gpu.queue, &desc(pick(F::Rgba8UnormSrgb, F::Rgba8Unorm), 1), wgpu::util::TextureDataOrder::LayerMajor, &t.rgba);
        return (texture.create_view(&Default::default()), 0);
    }
    let mut up = upload::TexUp::new(gpu, t, srgb);
    let seq = up.step(gpu, t, true).unwrap_or(0);
    (up.texture.create_view(&Default::default()), seq)
}

fn texture_bind_group(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32, rgba: &[u8]) -> wgpu::BindGroup {
    let view = upload_texture(gpu, &crate::dat::zone::Texture::new(String::new(), 0, w, h, rgba.to_vec()), true);
    view_bind_group(gpu, layout, sampler, &view)
}

fn view_bind_group(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, view: &wgpu::TextureView) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

/// A character's GPU side: its textures, and either a vertex buffer re-filled with a CPU-posed mesh
/// (update_actor) or a static skinned mesh posed on the GPU from its bones (new_skinned / pose_actor).
/// Clones share the GPU resources (skinned actors of one model share one copy).
#[derive(Clone)]
pub struct ActorGpu {
    textures: Vec<wgpu::BindGroup>,
    slot: HashMap<String, usize>,
    vertices: Option<(wgpu::Buffer, usize)>,
    draws: Vec<(usize, std::ops::Range<u32>)>,
    instance: wgpu::Buffer,
    skin: Option<Skinned>,
    /// Model colour alpha, 1 = opaque (actor fades); goes into the vertex alpha.
    pub alpha: f32,
    /// Target flash (client/flash.rs): ambient lerp toward 220 / 255 and fog colour scale.
    pub flash: [f32; 2],
    /// The last mip job of its textures; drawn once the GPU has it.
    mips: u64,
}

/// Bind-pose corners with up to two weighted influences, skinned in vs_skinned.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkinVertex {
    p0: [f32; 3],
    n0: [f32; 3],
    p1: [f32; 3],
    n1: [f32; 3],
    bones: [u32; 2],
    w: [f32; 2],
    uv: [f32; 2],
}

/// A model's skinned bind-pose corners grouped by texture name, built on the loader thread (Character::skin).
pub struct SkinMesh {
    verts: Vec<SkinVertex>,
    groups: Vec<(String, std::ops::Range<u32>)>,
    bones: usize,
}

impl SkinMesh {
    pub fn build(ch: &crate::dat::character::Character) -> Self {
        let bones = ch.skeleton.bones.len().min(MAX_BONES);
        let slot_of = |b: usize| if b < bones { b as u32 } else { bones as u32 };
        let mut verts: Vec<SkinVertex> = Vec::new();
        let mut groups = Vec::new();
        for part in &ch.parts {
            for g in &part.groups {
                let start = verts.len() as u32;
                for t in &g.triangles {
                    for c in t {
                        let inf = &part.vertices[c.vertex as usize];
                        let i0 = inf[0];
                        let i1 = inf.get(1).copied().unwrap_or_default();
                        let (w1, b1) = if inf.len() > 1 { (i1.weight, slot_of(i1.bone)) } else { (0.0, bones as u32) };
                        verts.push(SkinVertex {
                            p0: i0.pos.to_array(),
                            n0: i0.normal.to_array(),
                            p1: i1.pos.to_array(),
                            n1: i1.normal.to_array(),
                            bones: [slot_of(i0.bone), b1],
                            w: [i0.weight, w1],
                            uv: c.uv,
                        });
                    }
                }
                groups.push((g.texture.clone(), start..verts.len() as u32));
            }
        }
        if verts.is_empty() {
            verts.push(SkinVertex::zeroed());
        }
        SkinMesh { verts, groups, bones }
    }
}

/// A model going up: animations registered, bytes of its vertices in, next texture and the one in progress.
struct Build {
    ch: std::sync::Arc<crate::dat::character::Character>,
    mesh: std::sync::Arc<SkinMesh>,
    actor: ActorGpu,
    registered: bool,
    vert_at: u64,
    tex: usize,
    cur: Option<upload::TexUp>,
}

#[derive(Clone)]
struct Skinned {
    vertices: wgpu::Buffer,
    bones: usize,
    /// Dynamic offset of this frame's pose region, and the pose frame it was written in.
    offset: u32,
    frame: u64,
}

/// Bones a pose region holds; influences on later bones read the identity slot after them.
const MAX_BONES: usize = 255;
/// Pose region in vec4s: model matrix columns, (alpha, flash ambient, flash fog, 0), then (quat, translation) per bone.
const POSE_HEAD: usize = 5;
const POSE_BINDING: u64 = ((POSE_HEAD + 2 * (MAX_BONES + 1)) * 16) as u64;

/// Strip to list, dropping the degenerate triangles that join strips.
fn strip_to_list(strip: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(strip.len() * 3);
    for i in 0..strip.len().saturating_sub(2) {
        let (a, b, c) = (strip[i], strip[i + 1], strip[i + 2]);
        if a == b || b == c || a == c {
            continue;
        }
        if i % 2 == 0 { out.extend([a, b, c]) } else { out.extend([b, a, c]) }
    }
    out
}

/// Zone pipelines for one colour format, made once per device (PipeCache) and shared by every ZoneRenderer.
#[derive(Clone)]
pub struct ZonePipes {
    globals_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    pose_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    alpha_pipeline: wgpu::RenderPipeline,
    actor_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    skin_pipeline: wgpu::RenderPipeline,
}

impl ZonePipes {
    fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zone"),
            source: wgpu::ShaderSource::Wgsl(include_str!("zone.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
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
            label: Some("zone"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let vertex_layouts = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4],
            }),
        ];
        let make_pipeline = |fragment: &str, blend: Option<wgpu::BlendState>| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fragment),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_layouts,
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("poses"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(POSE_BINDING) },
                count: None,
            }],
        });
        let skin_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout), Some(&pose_layout)],
            immediate_size: 0,
        });
        let skin_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Uint32x2, 5 => Float32x2, 6 => Float32x2],
        })];
        let skin_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned actors"),
            layout: Some(&skin_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_skinned"), compilation_options: Default::default(), buffers: &skin_buffers },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_actor"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline = make_pipeline("fs_opaque", None);
        let alpha_pipeline = make_pipeline("fs_alpha_test", None);
        // Actors blend by their model alpha (fades); opaque ones output alpha 1.
        let actor_pipeline = make_pipeline("fs_actor", Some(wgpu::BlendState::ALPHA_BLENDING));
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_sky"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_sky"), compilation_options: Default::default(), targets: &[Some(color_format.into())] }),
            multiview_mask: None,
            cache: None,
        });
        Self { globals_layout, texture_layout, pose_layout, pipeline, alpha_pipeline, actor_pipeline, sky_pipeline, skin_pipeline }
    }
}

/// Scenery as the GPU takes it (one vertex, index and instance buffer, draws in file order), built from the
/// scene on the loader thread.
pub struct ZoneGeometry {
    matrices: Vec<[[f32; 4]; 4]>,
    draws: Vec<Draw>,
    verts: Vec<GpuVertex>,
    indices: Vec<u16>,
}

impl ZoneGeometry {
    pub fn build(scene: &ZoneScene) -> Self {
        // Slot 0 is the plain white texture; scene textures follow in order.
        let texture_slot: HashMap<&str, usize> = scene.textures.iter().enumerate().map(|(i, t)| (t.name.as_str(), i + 1)).collect();
        // One instance block per mesh, holding the matrices of every placement that uses it.
        let mut by_mesh: HashMap<&str, Vec<[[f32; 4]; 4]>> = HashMap::new();
        for p in &scene.placements {
            by_mesh.entry(&p.name).or_default().push(p.matrix());
        }
        let mut matrices: Vec<[[f32; 4]; 4]> = Vec::new();
        let mut draws = Vec::new();
        let mut all_verts: Vec<GpuVertex> = Vec::new();
        let mut all_indices: Vec<u16> = Vec::new();
        for mesh in &scene.meshes {
            let Some(mats) = by_mesh.remove(mesh.name.as_str()) else { continue };
            let start = matrices.len() as u32;
            matrices.extend(mats);
            let instances = start..matrices.len() as u32;
            for model in &mesh.models {
                let indices = if model.strip { strip_to_list(&model.indices) } else { model.indices.clone() };
                if indices.is_empty() || model.vertices.is_empty() {
                    continue;
                }
                let verts: Vec<GpuVertex> = model
                    .vertices
                    .iter()
                    .map(|v| {
                        let [b, g, r, a] = v.color.to_le_bytes();
                        GpuVertex { pos: v.pos, normal: v.normal, color: [r, g, b, a], uv: v.uv }
                    })
                    .collect();
                let count = (indices.len() / 3 * 3) as u32;
                let (first_index, base_vertex) = (all_indices.len() as u32, all_verts.len() as i32);
                all_indices.extend_from_slice(&indices[..count as usize]);
                all_verts.extend(verts);
                draws.push(Draw {
                    first_index,
                    index_count: count,
                    base_vertex,
                    instances: instances.clone(),
                    texture: texture_slot.get(model.texture.as_str()).copied().unwrap_or(0),
                    // Same rule as the reference viewer: '_' meshes and blend bit 0x8000 are cut out.
                    alpha_test: mesh.name.starts_with('_') || model.blending & 0x8000 != 0,
                });
            }
        }
        // A scene with no scenery (the character screen) still needs a non-empty buffer to bind.
        if matrices.is_empty() {
            matrices.push(Mat4::IDENTITY.to_cols_array_2d());
        }
        if all_verts.is_empty() {
            all_verts.push(GpuVertex::zeroed());
        }
        while all_indices.len() % 2 != 0 || all_indices.is_empty() {
            all_indices.push(0);
        }
        Self { matrices, draws, verts: all_verts, indices: all_indices }
    }

    fn parts(&self) -> [&[u8]; 3] {
        [bytemuck::cast_slice(&self.verts), bytemuck::cast_slice(&self.indices), bytemuck::cast_slice(&self.matrices)]
    }
}

/// Mip texels a frame (render/mipgen.rs).
pub const MIP_BUDGET: u64 = 4 << 20;

impl ZoneRenderer {
    /// Everything at once (lobby scenes, offscreen shots); their mips run before their first draw.
    pub fn new(gpu: &Gpu, scene: &ZoneScene, color_format: wgpu::TextureFormat) -> Self {
        let geo = ZoneGeometry::build(scene);
        let mut r = Self::empty(gpu, &geo, scene.textures.len(), color_format, false);
        for (buf, bytes) in [&r.vertices, &r.indices, &r.instances].into_iter().zip(geo.parts()) {
            gpu.queue.write_buffer(buf, 0, bytes);
        }
        for (i, t) in scene.textures.iter().enumerate() {
            r.put_texture(gpu, i + 1, t);
        }
        r.log_textures();
        r
    }

    /// The world's zone: its geometry and textures go up over frames under the copy budget (upload_step) and
    /// its mips under the mip budget; ready() once all of it is on the GPU.
    pub fn streamed(gpu: &Gpu, geo: std::sync::Arc<ZoneGeometry>, textures: Vec<crate::dat::zone::Texture>, color_format: wgpu::TextureFormat) -> Self {
        let mut r = Self::empty(gpu, &geo, textures.len(), color_format, true);
        r.pending = textures.into_iter().enumerate().map(|(i, t)| (i + 1, t, None)).collect();
        r.geo_pending = Some((geo, 0));
        r
    }

    fn empty(gpu: &Gpu, geo: &ZoneGeometry, textures: usize, color_format: wgpu::TextureFormat, budgeted: bool) -> Self {
        let device = &gpu.device;
        let p = gpu.pipes.zone(color_format);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &p.globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let sampler = mip_sampler(device);
        // Slot 0 is plain white for models whose texture lives elsewhere; the others fill in as they go up.
        let white = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 255, 255, 255]);
        // Slot 0 of actors: magenta, so a missing texture is obvious.
        let magenta = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 0, 255, 255]);
        let [v, i, m] = geo.parts().map(|b| b.len() as u64);
        let buffer = |label, size: u64, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self {
            pipeline: p.pipeline,
            alpha_pipeline: p.alpha_pipeline,
            actor_pipeline: p.actor_pipeline,
            sky_pipeline: p.sky_pipeline,
            texture_layout: p.texture_layout,
            sampler,
            uniforms,
            globals,
            textures: vec![white; textures + 1],
            magenta,
            instances: buffer("instances", m, wgpu::BufferUsages::VERTEX),
            vertices: buffer("scenery vertices", v, wgpu::BufferUsages::VERTEX),
            indices: buffer("scenery indices", i, wgpu::BufferUsages::INDEX),
            draws: geo.draws.clone(),
            skin_pipeline: p.skin_pipeline,
            pose_layout: p.pose_layout,
            poses: Vec::new(),
            pose_gpu: None,
            pose_frame: 0,
            anim: anim::GpuAnim::new(gpu),
            models: HashMap::new(),
            builds: Vec::new(),
            budgeted,
            pending: std::collections::VecDeque::new(),
            geo_pending: None,
            mips_seq: 0,
            tex_bytes: (TEXTURE_BYTES.load(std::sync::atomic::Ordering::Relaxed), 0, 0),
            encode_gamma: !color_format.is_srgb(),
            fog_color: [0.55, 0.65, 0.8],
            clear_color: true,
            lighting: None,
        }
    }

    fn put_texture(&mut self, gpu: &Gpu, slot: usize, t: &crate::dat::zone::Texture) {
        let (view, seq) = upload_texture_seq(gpu, t, true);
        self.set_texture(gpu, slot, t, &view, seq);
    }

    fn set_texture(&mut self, gpu: &Gpu, slot: usize, t: &crate::dat::zone::Texture, view: &wgpu::TextureView, seq: u64) {
        self.textures[slot] = view_bind_group(gpu, &self.texture_layout, &self.sampler, view);
        self.mips_seq = self.mips_seq.max(seq);
        self.tex_bytes.1 += (t.width * t.height * 4) as u64;
        self.tex_bytes.2 += 1;
    }

    fn log_textures(&self) {
        let (before, rgba, n) = self.tex_bytes;
        if n > 0 {
            let now = TEXTURE_BYTES.load(std::sync::atomic::Ordering::Relaxed);
            log::info!("zone textures: {n} on the GPU with mips ({:.1} MB; plain RGBA without mips was {:.1} MB)", now.saturating_sub(before) as f32 / 1e6, rgba as f32 / 1e6);
        }
    }

    /// One frame's copies (Gpu::begin_uploads set the room): a streamed zone's geometry and textures, then the
    /// models actors are waiting for, each piece by piece.
    pub fn upload_step(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        self.anim.upload_keys(gpu, encoder, false);
        if let Some((geo, mut at)) = self.geo_pending.take() {
            let parts = geo.parts();
            let bufs = [&self.vertices, &self.indices, &self.instances];
            let mut start = 0u64;
            for (k, part) in parts.iter().enumerate() {
                let len = part.len() as u64;
                if at < start + len {
                    let mut local = at - start;
                    upload::buffer_step(gpu, bufs[k], part, &mut local);
                    at = start + local;
                }
                start += len;
            }
            crate::client::perf::note(|| format!("zone geometry {at} / {start} bytes"));
            if at < start {
                self.geo_pending = Some((geo, at));
            }
        }
        let had = !self.pending.is_empty();
        while gpu.upload_room() > 0
            && let Some((_, t, up)) = self.pending.front_mut()
        {
            let up = up.get_or_insert_with(|| upload::TexUp::new(gpu, t, true));
            let Some(seq) = up.step(gpu, t, false) else { break };
            let view = up.texture.create_view(&Default::default());
            let (slot, t, _) = self.pending.pop_front().unwrap();
            self.set_texture(gpu, slot, &t, &view, seq);
        }
        if had {
            crate::client::perf::note(|| format!("zone textures, {} left", self.pending.len()));
            if self.pending.is_empty() {
                self.log_textures();
            }
        }
        let mut i = 0;
        while i < self.builds.len() && gpu.upload_room() > 0 {
            if self.build_step(gpu, i) {
                let (key, b) = self.builds.remove(i);
                self.models.insert(key, (b.ch, b.actor));
            } else {
                i += 1;
            }
        }
    }

    /// All of a streamed zone is on the GPU with its mips.
    pub fn ready(&self, gpu: &Gpu) -> bool {
        self.geo_pending.is_none() && self.pending.is_empty() && gpu.mips_done() >= self.mips_seq
    }

    /// A model's next pieces: its animations (copied by the posing pass), its skinned vertices, its textures.
    fn build_step(&mut self, gpu: &Gpu, i: usize) -> bool {
        let b = &mut self.builds[i].1;
        if !b.registered {
            self.anim.register(&b.ch);
            b.registered = true;
        }
        let b = &mut self.builds[i].1;
        let skin = b.actor.skin.as_ref().unwrap();
        if !upload::buffer_step(gpu, &skin.vertices, bytemuck::cast_slice(&b.mesh.verts), &mut b.vert_at) {
            return false;
        }
        while b.tex < b.ch.textures.len() && gpu.upload_room() > 0 {
            let t = &b.ch.textures[b.tex];
            let up = b.cur.get_or_insert_with(|| upload::TexUp::new(gpu, t, true));
            let Some(seq) = up.step(gpu, t, false) else { return false };
            let view = up.texture.create_view(&Default::default());
            b.actor.textures[b.tex + 1] = view_bind_group(gpu, &self.texture_layout, &self.sampler, &view);
            b.actor.mips = b.actor.mips.max(seq);
            b.cur = None;
            b.tex += 1;
        }
        b.tex == b.ch.textures.len() && self.anim.ready(&b.ch)
    }

    /// An actor skinned on the GPU: its bind-pose corners (SkinMesh) and textures go up once per model.
    pub fn new_skinned(&self, gpu: &Gpu, ch: &crate::dat::character::Character) -> ActorGpu {
        let mut a = self.new_actor(gpu, ch);
        let mesh = ch.skin.clone().unwrap_or_else(|| std::sync::Arc::new(SkinMesh::build(ch)));
        a.draws = mesh.groups.iter().map(|(tex, r)| (a.slot.get(tex.as_str()).copied().unwrap_or(0), r.clone())).collect();
        let vertices = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("skinned actor"), contents: bytemuck::cast_slice(&mesh.verts), usage: wgpu::BufferUsages::VERTEX });
        a.skin = Some(Skinned { vertices, bones: mesh.bones, offset: 0, frame: u64::MAX });
        a
    }

    /// A skinned actor of a model. A new model goes up over the next frames under the copy budget; None until
    /// then (the actor is not drawn).
    pub fn skinned_actor(&mut self, gpu: &Gpu, ch: &std::sync::Arc<crate::dat::character::Character>) -> Option<ActorGpu> {
        let key = std::sync::Arc::as_ptr(ch) as usize;
        if let Some(m) = self.models.get(&key) {
            return Some(m.1.clone());
        }
        if !self.builds.iter().any(|(k, _)| *k == key) {
            let mesh = ch.skin.clone().unwrap_or_else(|| std::sync::Arc::new(SkinMesh::build(ch)));
            let mut slot = HashMap::new();
            for (i, t) in ch.textures.iter().enumerate() {
                slot.entry(t.name.clone()).or_insert(i + 1);
            }
            let draws = mesh.groups.iter().map(|(tex, r)| (slot.get(tex.as_str()).copied().unwrap_or(0), r.clone())).collect();
            let size = (std::mem::size_of_val(mesh.verts.as_slice()) as u64).max(4);
            let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("skinned actor"), size, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let instance = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("actor instance"), size: 64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let actor = ActorGpu {
                textures: vec![self.magenta.clone(); ch.textures.len() + 1],
                slot,
                vertices: None,
                draws,
                instance,
                skin: Some(Skinned { vertices, bones: mesh.bones, offset: 0, frame: u64::MAX }),
                alpha: 1.0,
                flash: [0.0, 1.0],
                mips: 0,
            };
            self.builds.push((key, Build { ch: ch.clone(), mesh, actor, registered: false, vert_at: 0, tex: 0, cur: None }));
        }
        None
    }

    /// Starts this frame's poses; skinned actors not posed again are not drawn.
    pub fn begin_poses(&mut self) {
        self.poses.clear();
        self.pose_frame += 1;
        self.anim.begin();
    }

    /// Queues a skinned actor posed on the GPU from its motions (to, and from at blend `t`), and its placement.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn pose_actor_gpu(
        &mut self,
        actor: &mut ActorGpu,
        ch: &std::sync::Arc<crate::dat::character::Character>,
        files: &[std::sync::Arc<crate::dat::motionfile::MotionFile>],
        to: &[(&crate::dat::character::Animation, f32)],
        from: &[(&crate::dat::character::Animation, f32)],
        t: f32,
        model: Mat4,
        look: Option<anim::HeadLook>,
    ) {
        if actor.skin.is_none() {
            return;
        }
        self.pose_actor(actor, &[], model);
        if let Some(skin) = &actor.skin {
            let out = skin.offset / 16 + POSE_HEAD as u32;
            self.anim.job(ch, files, to, from, t, out, look);
        }
    }

    /// Queues a skinned actor's bones (world transforms from character::pose) and placement.
    pub fn pose_actor(&mut self, actor: &mut ActorGpu, bones: &[(glam::Quat, Vec3)], model: Mat4) {
        let alpha = actor.alpha.clamp(0.0, 1.0);
        let flash = actor.flash;
        let Some(skin) = actor.skin.as_mut() else { return };
        while self.poses.len() % 16 != 0 {
            self.poses.push([0.0; 4]);
        }
        skin.offset = (self.poses.len() * 16) as u32;
        skin.frame = self.pose_frame;
        self.poses.extend(model.to_cols_array_2d());
        self.poses.push([alpha, flash[0], flash[1], 0.0]);
        for i in 0..skin.bones {
            let (q, t) = bones.get(i).copied().unwrap_or((glam::Quat::IDENTITY, Vec3::ZERO));
            self.poses.push(q.to_array());
            self.poses.push([t.x, t.y, t.z, 0.0]);
        }
        self.poses.push([0.0, 0.0, 0.0, 1.0]);
        self.poses.push([0.0; 4]);
    }

    /// Writes the frame's poses in one upload (the buffer grows as needed), then runs the GPU posing.
    pub fn upload_poses(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        if self.poses.is_empty() {
            return;
        }
        let need = (self.poses.len() * 16) as u64 + POSE_BINDING;
        if self.pose_gpu.as_ref().is_none_or(|(_, _, cap)| *cap < need) {
            let cap = need.next_power_of_two().max(1 << 16);
            crate::client::perf::note(|| format!("pose buffer {cap} bytes"));
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("poses"), size: cap, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("poses"),
                layout: &self.pose_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buf, offset: 0, size: wgpu::BufferSize::new(POSE_BINDING) }) }],
            });
            self.pose_gpu = Some((buf, group, cap));
        }
        if let Some((buf, _, _)) = &self.pose_gpu {
            gpu.queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.poses));
            self.anim.dispatch(gpu, encoder, buf, !self.budgeted);
        }
    }

    pub fn new_actor(&self, gpu: &Gpu, ch: &crate::dat::character::Character) -> ActorGpu {
        // Slot 0: magenta, so a missing texture is obvious.
        let mut textures = vec![texture_bind_group(gpu, &self.texture_layout, &self.sampler, 1, 1, &[255, 0, 255, 255])];
        let mut slot = HashMap::new();
        let mut mips = 0;
        for t in &ch.textures {
            slot.entry(t.name.clone()).or_insert(textures.len());
            let (view, seq) = upload_texture_seq(gpu, t, true);
            mips = mips.max(seq);
            textures.push(view_bind_group(gpu, &self.texture_layout, &self.sampler, &view));
        }
        let instance = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("actor instance"),
            size: 64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ActorGpu { textures, slot, vertices: None, draws: Vec::new(), instance, skin: None, alpha: 1.0, flash: [0.0, 1.0], mips }
    }

    /// Uploads this frame's posed mesh and placement.
    pub fn update_actor(&self, gpu: &Gpu, actor: &mut ActorGpu, mesh: &character::PosedMesh, model: Mat4) {
        let mut verts: Vec<GpuVertex> = Vec::new();
        actor.draws.clear();
        // 0x80 = 1.0 as for scenery vertex colours.
        let a = (actor.alpha.clamp(0.0, 1.0) * 128.0).round() as u8;
        for (tex, tris) in &mesh.batches {
            let start = verts.len() as u32;
            verts.extend(tris.iter().map(|v| GpuVertex { pos: [v[0], v[1], v[2]], normal: [v[3], v[4], v[5]], color: [128, 128, 128, a], uv: [v[6], v[7]] }));
            actor.draws.push((actor.slot.get(tex.as_str()).copied().unwrap_or(0), start..verts.len() as u32));
        }
        let bytes: &[u8] = bytemuck::cast_slice(&verts);
        if actor.vertices.as_ref().is_none_or(|(_, cap)| *cap < bytes.len()) {
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("actor vertices"),
                size: bytes.len().max(64) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            actor.vertices = Some((buf, bytes.len().max(64)));
        }
        if let Some((buf, _)) = &actor.vertices {
            gpu.queue.write_buffer(buf, 0, bytes);
        }
        gpu.queue.write_buffer(&actor.instance, 0, bytemuck::bytes_of(&model.to_cols_array_2d()));
    }

    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, camera: &Camera, aspect: f32, actors: &[&ActorGpu]) {
        if !self.budgeted {
            gpu.run_mips(encoder, u64::MAX);
        }
        let done = gpu.mips_done();
        let vp = camera.view_proj(aspect);
        let v4 = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
        let mut u = Uniforms {
            view_proj: vp.to_cols_array_2d(),
            light: [0.3, -1.0, 0.2, 0.5],
            fog_color: [self.fog_color[0], self.fog_color[1], self.fog_color[2], 1.0],
            fog: [150.0, 600.0, self.encode_gamma as u32 as f32, 0.0],
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, 1.0],
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            sun: [0.0; 4],
            sun_color: [0.0; 4],
            ambient: [0.0; 4],
            actor_sun: [0.0; 4],
            actor_color: [0.0; 4],
            actor_ambient: [0.0; 4],
            sky_info: [0.0; 4],
            clear: [0.0; 4],
            sky_colors: [[0.0; 4]; 8],
            sky_stops: [[0.0; 4]; 2],
        };
        let mut clear = self.fog_color;
        if let Some(l) = &self.lighting {
            u.sun = v4(l.dir, 1.0);
            u.sun_color = v4(l.diffuse, 1.0);
            u.ambient = v4(l.ambient, 1.0);
            u.actor_sun = v4(l.actor_dir, 1.0);
            u.actor_color = v4(l.actor_diffuse, 1.0);
            u.actor_ambient = v4(l.actor_ambient, 1.0);
            u.fog_color = v4(l.fog, 1.0);
            u.fog[0] = l.fog_start;
            u.fog[1] = l.fog_end;
            u.clear = v4(l.clear, 1.0);
            if let Some((colors, stops)) = &l.sky {
                u.sky_info[0] = 1.0;
                for i in 0..8 {
                    u.sky_colors[i] = v4(colors[i], 1.0);
                    u.sky_stops[i / 4][i % 4] = stops[i];
                }
            }
            // Clear colours are gamma-space bytes; the target may be sRGB.
            let lin = |c: f32| if self.encode_gamma { c } else { c.powf(2.2) };
            clear = [lin(l.clear.x), lin(l.clear.y), lin(l.clear.z)];
        }
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let [r, g, b] = clear;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zone"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if self.clear_color { wgpu::LoadOp::Clear(wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: 1.0 }) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.globals, &[]);
        if self.lighting.is_some() && self.clear_color {
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(1, &self.textures[0], &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        for (pipeline, cutout) in [(&self.pipeline, false), (&self.alpha_pipeline, true)] {
            pass.set_pipeline(pipeline);
            let mut bound = usize::MAX;
            for d in self.draws.iter().filter(|d| d.alpha_test == cutout) {
                if d.texture != bound {
                    pass.set_bind_group(1, &self.textures[d.texture], &[]);
                    bound = d.texture;
                }
                pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, d.instances.clone());
            }
        }
        if let Some((_, group, _)) = &self.pose_gpu {
            pass.set_pipeline(&self.skin_pipeline);
            for a in actors.iter().filter(|a| a.mips <= done) {
                let Some(skin) = a.skin.as_ref().filter(|s| s.frame == self.pose_frame) else { continue };
                pass.set_bind_group(2, group, &[skin.offset]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                for (tex, range) in &a.draws {
                    pass.set_bind_group(1, &a.textures[*tex], &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
        pass.set_pipeline(&self.actor_pipeline);
        for a in actors.iter().filter(|a| a.mips <= done) {
            let Some((buf, _)) = &a.vertices else { continue };
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.set_vertex_buffer(1, a.instance.slice(..));
            for (tex, range) in &a.draws {
                pass.set_bind_group(1, &a.textures[*tex], &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
    }
}

pub fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Renders one frame offscreen and writes it as a PNG.
pub fn render_png(scene: &ZoneScene, camera: &Camera, width: u32, height: u32, path: &std::path::Path) -> Result<usize> {
    render_pngs(scene, &[(camera, path)], width, height)
}

/// Renders several frames of one scene offscreen, one PNG each.
pub fn render_pngs(scene: &ZoneScene, frames: &[(&Camera, &std::path::Path)], width: u32, height: u32) -> Result<usize> {
    render_pngs_with(scene, frames, width, height, &|_, _| Vec::new())
}

/// As render_pngs, with actors built once by `setup` and drawn in every frame.
pub fn render_pngs_with(
    scene: &ZoneScene,
    frames: &[(&Camera, &std::path::Path)],
    width: u32,
    height: u32,
    setup: &dyn Fn(&Gpu, &mut ZoneRenderer) -> Vec<ActorGpu>,
) -> Result<usize> {
    let lit: Vec<_> = frames.iter().map(|(c, p)| (*c, *p, None)).collect();
    render_pngs_lit(scene, &lit, width, height, setup)
}

/// As render_pngs_with, each frame with optional retail lighting (dat/weather.rs).
pub fn render_pngs_lit(
    scene: &ZoneScene,
    frames: &[(&Camera, &std::path::Path, Option<crate::dat::weather::Lighting>)],
    width: u32,
    height: u32,
    setup: &dyn Fn(&Gpu, &mut ZoneRenderer) -> Vec<ActorGpu>,
) -> Result<usize> {
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = ZoneRenderer::new(&gpu, scene, format);
    let actors = setup(&gpu, &mut renderer);
    let actor_refs: Vec<&ActorGpu> = actors.iter().collect();
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = depth_view(&gpu.device, width, height);
    let row = (width * 4).div_ceil(256) * 256;
    for (camera, path, lighting) in frames {
        renderer.lighting = *lighting;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.render(&gpu, &mut encoder, &view, &depth, camera, width as f32 / height as f32, &actor_refs);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
        let data = readback.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("mapping readback: {e:?}"))?;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            pixels.extend_from_slice(&data[(y * row) as usize..(y * row + width * 4) as usize]);
        }
        drop(data);
        readback.unmap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()?.write_image_data(&pixels)?;
    }
    Ok(renderer.draw_count())
}
