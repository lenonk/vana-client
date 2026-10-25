//! wgpu renderer for zone scenery.

pub mod anim;
pub mod effects;
pub mod points;
pub mod occlusion;
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
    // One per card: each backend (Vulkan, GL, ...) lists it, with its own driver part ("... (RADV NAVI21)", "...
    // (radeonsi, navi21, ...)"); the first (the preferred backend's) stays.
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.split(" (").next().unwrap_or(n).trim().to_string()));
    names
}

/// With WGPU_BACKEND set, the best adapter (discrete first, software last) of its backends.
fn preferred_backend(instance: &wgpu::Instance, surface: Option<&wgpu::Surface>) -> Option<wgpu::Adapter> {
    let backends = wgpu::Backends::from_env()?;
    let rank = |t: wgpu::DeviceType| match t {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    pollster::block_on(instance.enumerate_adapters(backends))
        .into_iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a.get_info().device_type))
}

impl Gpu {
    pub fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface>) -> Result<Self> {
        Self::with_adapter(instance, surface, "")
    }

    /// The adapter named `preferred` when it can drive the surface; otherwise (or empty) the high-performance one.
    pub fn with_adapter(instance: wgpu::Instance, surface: Option<&wgpu::Surface>, preferred: &str) -> Result<Self> {
        // By name, WGPU_BACKEND's copy of the card first.
        let first = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all());
        let named = (!preferred.is_empty())
            .then(|| {
                pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
                    .into_iter()
                    .filter(|a| a.get_info().name == preferred && surface.is_none_or(|s| a.is_surface_supported(s)))
                    .min_by_key(|a| !first.contains(a.get_info().backend.into()))
            })
            .flatten();
        if !preferred.is_empty() && named.is_none() {
            log::warn!("GPU {preferred:?} not found or can't drive the window; using the default");
        }
        let adapter = match named.or_else(|| preferred_backend(&instance, surface)) {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .context("no suitable GPU adapter")?,
        };
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, {:?})", info.name, info.backend, info.device_type);
        // BC textures go up compressed when the adapter has them (zone and model DXT1 / DXT3).
        let required_features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
        // The effects update pass binds 9 storage buffers (the default limit is 8; desktop GPUs allow far more).
        let required_limits = wgpu::Limits { max_storage_buffers_per_shader_stage: adapter.limits().max_storage_buffers_per_shader_stage.min(16), ..Default::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features, required_limits, ..Default::default() }))?;
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
    /// Bit 0: the MMB model's flag 0x1000 (header u32 +0x10 bit 28): lit by the sub-area's 'ev10' while one is in.
    flags: u32,
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
    /// x: the area slot of 'ev10' while a sub-area is shown (0 none).
    area_info: [f32; 4],
}

/// Area environments (DAT_10631f24 area objects) the scenery is lit by, slot 0 unused.
pub const AREAS: usize = 64;

/// One area's lighting as the shader takes it: sun (w = 1 when it has an environment), sun colour, ambient, fog
/// colour, fog (start, end).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AreaGpu {
    sun: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    actor_sun: [f32; 4],
    actor_color: [f32; 4],
    actor_ambient: [f32; 4],
}

/// Door parts the GPU animates (slot 0 unused).
pub const PARTS: usize = 512;

/// A door part's two channels (scheduler ops 0x0d rotate / 0x0c translate): from, to, (start tick, ticks).
#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub struct PartGpu {
    rot_from: [f32; 4],
    rot_to: [f32; 4],
    rot_time: [f32; 4],
    pos_from: [f32; 4],
    pos_to: [f32; 4],
    pos_time: [f32; 4],
}

impl PartGpu {
    /// A channel's value at tick `now` (linear, exact at the end: FUN_10043b00 / FUN_10043ab0).
    fn at(from: [f32; 4], to: [f32; 4], time: [f32; 4], now: f32) -> [f32; 3] {
        let k = if time[1] <= 0.0 { 1.0 } else { ((now - time[0]) / time[1]).clamp(0.0, 1.0) };
        std::array::from_fn(|i| from[i] + (to[i] - from[i]) * k)
    }
}

/// One scenery model: a range of the shared index buffer, its base vertex in the shared vertex buffer.
#[derive(Clone)]
struct Draw {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    instances: std::ops::Range<u32>,
    texture: usize,
    /// Instances placed mirrored (scale.x * scale.z < 0, inst+0xd4): culled the other way.
    mirrored: std::ops::Range<u32>,
    /// Back faces culled (CCW, or CW when mirrored); MMB model bit 29 draws both sides.
    cull: bool,
    /// FUN_1017c9c0's states: bit 0 alpha test (placements named '_', inst+0xd6), bit 1 alpha blend (MMB model bit 31:
    /// no depth write, ZBIAS 8). Drawn in that order: opaque, cut out, blended.
    mode: u8,
}

/// The scene projection's near and far planes. Near is retail's 0.1 (FUN_100146e0): the eye keeps only 0.17 off the
/// floor, and a deeper near plane cut into slopes beside it.
pub const NEAR: f32 = 0.1;
pub const FAR: f32 = 2000.0;

#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
}

impl Camera {
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov_y, aspect, NEAR, FAR)
    }
}

pub struct ZoneRenderer {
    /// The point-light table the scenery reads, and its keys by slot.
    pub lights: wgpu::Buffer,
    pub light_keys: Vec<u32>,
    scenery: [[wgpu::RenderPipeline; 3]; 4],
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
    shine_pipeline: wgpu::RenderPipeline,
    shine_buf: wgpu::Buffer,
    shine_group: wgpu::BindGroup,
    refract_pipeline: wgpu::RenderPipeline,
    refract_buf: wgpu::Buffer,
    refract_group: wgpu::BindGroup,
    /// The refraction pass's scene copy (DAT_10456e6c: scene size, one per frame per refracting actor).
    refract_copy: Option<(wgpu::Texture, wgpu::BindGroup)>,
    /// Shine images by (effect file, IMG node).
    shine_tex: HashMap<(u32, usize), wgpu::BindGroup>,
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
    /// Posed actor points read back (footsteps).
    pub points: points::Points,
    /// The actors' occlusion test (FUN_1006c280), for the target keys.
    pub occl: occlusion::Occlusion,
    /// The loaded sub-area (an interior), drawn after the zone's scenery.
    sub: Option<SubGeometry>,
    /// Mog House furniture (client/furniture.rs): a geometry per model file, its textures after `furn_base`.
    furniture: Vec<SubGeometry>,
    furn_base: Option<usize>,
    /// Zone instances standing in for a sub-area (instance, link, as built), the link hidden now, and how many of
    /// `light_keys` are the zone's own.
    links: Vec<(u32, u32, Instance)>,
    hidden: u32,
    zone_keys: usize,
    /// Area 4ccs by slot - 1, how many are the zone's, and each slot's lighting for this frame (set_area_lights).
    area_keys: Vec<u32>,
    zone_areas: usize,
    area_lights: Vec<Option<crate::dat::weather::Lighting>>,
    areas: wgpu::Buffer,
    /// Door parts: slots by door id, their base matrices, their animations (CPU copy of the GPU table) and the tick
    /// they run against.
    pub door_parts: HashMap<u32, Vec<u32>>,
    pub door_bases: HashMap<u32, Vec<glam::Mat4>>,
    parts: Vec<PartGpu>,
    parts_gpu: wgpu::Buffer,
    pub now: f32,
}

/// A sub-area's scenery (FUN_101787c0 loads it into zone slot 1): its own buffers and draws, its textures in the
/// slots after the zone's; drawn once all of it is on the GPU.
struct SubGeometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    draws: Vec<Draw>,
    tex_base: usize,
    link: u32,
    shown: bool,
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
    draws: Vec<(usize, std::ops::Range<u32>, u8)>,
    instance: wgpu::Buffer,
    /// `wepN` meshes not drawn (bit N; scheduler ops 0x75 / 0xa3, MODLOG "Weapon meshes hidden and shown").
    pub hidden_weps: u16,
    /// The shine overlay this frame (skinned actors).
    pub shine: Option<Shine>,
    /// The refraction pass this frame (skinned actors; drawn by render_refraction, not render).
    pub refract: Option<Refract>,
    skin: Option<Skinned>,
    /// Model colour alpha, 1 = opaque (actor fades); goes into the vertex alpha.
    pub alpha: f32,
    /// Model colour RGB, 1 = 0x80 (scheduler op 0x29): the material colour of the lit terms.
    pub tint: [f32; 3],
    /// Target flash (client/flash.rs): ambient lerp toward 220 / 255 and fog colour scale.
    pub flash: [f32; 2],
    /// The floor's four point lights (table slot + 1, 0 none): FUN_100c94a0 picks the strongest.
    pub lights: [u8; 4],
    /// The area 4cc it stands in (vtable +0x188, the floor's +0xb0): its environment lights it (0 = the zone's).
    pub area: u32,
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
    /// The material's light factor and cloth flag (0 / 1).
    mat: [f32; 2],
}

/// A draw group that is no `wepN` mesh.
const NO_WEP: u8 = 0xff;

/// The actor shine overlay (scheduler ops 0x40..0x47): an effect image by (file, node), its U / V scroll and the
/// TFACTOR colour (RGBA, 0..1).
#[derive(Clone, Copy, Debug)]
pub struct Shine {
    pub key: (u32, usize),
    pub u: f32,
    pub v: f32,
    pub tf: [f32; 4],
}

/// The actor refraction pass (scheduler ops 0x48 / 0x49): the level L (+0x744) and the TFACTOR alpha r = ((0x80 -
/// model colour alpha) & 0xff) / 255 (FUN_100d6d30).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refract {
    pub level: f32,
    pub r: f32,
}

impl Refract {
    /// FUN_100d6d30's TFACTOR alpha from the model colour alpha byte: (0x80 - a) & 0xff, over 255 (0 for an opaque model).
    pub fn tf_alpha(colour_alpha: u32) -> f32 {
        (0x80u32.wrapping_sub(colour_alpha) & 0xff) as f32 / 255.0
    }
}

/// Shined actors drawn a frame at most, and the stride of their parameters in the shine uniform buffer.
const SHINE_SLOTS: usize = 64;
const SHINE_STRIDE: u64 = 256;

/// A model's skinned bind-pose corners grouped by texture name, built on the loader thread (Character::skin).
pub struct SkinMesh {
    verts: Vec<SkinVertex>,
    /// Texture, vertices, and N of a `wepN` part (NO_WEP otherwise).
    groups: Vec<(String, std::ops::Range<u32>, u8)>,
    bones: usize,
}

impl SkinMesh {
    pub fn build(ch: &crate::dat::character::Character) -> Self {
        let bones = ch.skeleton.bones.len().min(MAX_BONES);
        let slot_of = |b: usize| if b < bones { b as u32 } else { bones as u32 };
        let mut verts: Vec<SkinVertex> = Vec::new();
        let mut groups = Vec::new();
        for part in &ch.parts {
            let n = part.name.trim_end_matches(['\0', ' ']);
            let wep = n.strip_prefix("wep").and_then(|d| d.parse::<u8>().ok()).filter(|d| *d < 16).unwrap_or(NO_WEP);
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
                            mat: [g.light, part.cloth as u32 as f32],
                        });
                    }
                }
                groups.push((g.texture.clone(), start..verts.len() as u32, wep));
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
const POSE_HEAD: usize = 6;
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
    /// Scenery by Draw::mode (opaque, cut out, blend, blend + cut out) and cull (none, back, front).
    scenery: [[wgpu::RenderPipeline; 3]; 4],
    actor_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    skin_pipeline: wgpu::RenderPipeline,
    shine_layout: wgpu::BindGroupLayout,
    shine_pipeline: wgpu::RenderPipeline,
    refract_pipeline: wgpu::RenderPipeline,
}

impl ZonePipes {
    fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zone"),
            source: wgpu::ShaderSource::Wgsl(include_str!("zone.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
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
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2, 9 => Uint32],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32x4, 10 => Uint32x4, 11 => Float32x4],
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
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Uint32x2, 5 => Float32x2, 6 => Float32x2, 7 => Float32x2],
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
        // Scenery: front faces clockwise as D3D's; normal placements cull CCW (back), mirrored ones CW (front).
        // Bit-31 models: SRCALPHA / INVSRCALPHA, depth tested (LESSEQUAL) but not written, ZBIAS 8 *(inf: as a constant
        // depth bias of -8)*.
        let make_scenery = |fragment: &str, blend: bool, cull: Option<wgpu::Face>| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fragment),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &vertex_layouts },
            primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, cull_mode: cull, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(!blend),
                depth_compare: Some(if blend { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less }),
                stencil: Default::default(),
                bias: if blend { wgpu::DepthBiasState { constant: -8, slope_scale: 0.0, clamp: 0.0 } } else { Default::default() },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let culls = [None, Some(wgpu::Face::Back), Some(wgpu::Face::Front)];
        let scenery = [("fs_opaque", false), ("fs_alpha_test", false), ("fs_blend", true), ("fs_blend_test", true)].map(|(f, b)| culls.map(|c| make_scenery(f, b, c)));
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
        // The shine overlay (FUN_10024e10's second pass): additive (SRCALPHA / ONE), z tested (as drawn) but not written.
        let shine_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shine"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(32) },
                count: None,
            }],
        });
        let shine_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actor shine"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("actor shine"),
                bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout), Some(&pose_layout), Some(&shine_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_skinned_shine"), compilation_options: Default::default(), buffers: &skin_buffers },
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
                module: &shader,
                entry_point: Some("fs_actor_shine"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The refraction pass (FUN_10025330): the scene copy sampled through D3DTS_TEXTURE0, SRCALPHA / ONE, z tested but
        // not written, cull as the base pass (none).
        let refract_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actor refraction"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("actor refraction"),
                bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout), Some(&pose_layout), Some(&shine_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_skinned_refract"), compilation_options: Default::default(), buffers: &skin_buffers },
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
                module: &shader,
                entry_point: Some("fs_actor_refract"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { globals_layout, texture_layout, pose_layout, scenery, actor_pipeline, sky_pipeline, skin_pipeline, shine_layout, shine_pipeline, refract_pipeline }
    }
}

/// Point-light table entries (DAT_105ecae8: 256).
pub const LIGHTS: usize = 256;

/// A scenery instance: the placement matrix and its four point lights (table slot + 1, 0 = none).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    model: [[f32; 4]; 4],
    lights: [u32; 4],
    /// x: the area slot it is lit by (0 = the zone's).
    area: [u32; 4],
    /// Level of detail: drawn while near² < d² <= far² from the camera to its origin; far 0 = always.
    band: [f32; 4],
}

/// Instance stride (actors with a CPU-posed mesh use the same layout, lights 0).
const INSTANCE_BYTES: u64 = std::mem::size_of::<Instance>() as u64;

/// Scenery as the GPU takes it (one vertex, index and instance buffer, draws in file order), built from the
/// scene on the loader thread.
pub struct ZoneGeometry {
    matrices: Vec<Instance>,
    /// Instances of placements that stand in for a sub-area (MZB +0x50): instance, link, as built.
    links: Vec<(u32, u32, Instance)>,
    /// The point-light table's keys (MZB light lists; slot = index), for the effects that light them.
    pub light_keys: Vec<u32>,
    /// Area 4ccs by slot - 1 (placements' MZB +0x4c), after those passed to build_keyed.
    pub area_keys: Vec<u32>,
    /// Door groups (FUN_1017a950): an id's parts (placements with MZB +0x34 = id, record order, at most 4) as part
    /// slots.
    pub door_parts: HashMap<u32, Vec<u32>>,
    /// Each door part's placement matrix, in part order.
    pub door_bases: HashMap<u32, Vec<glam::Mat4>>,
    draws: Vec<Draw>,
    verts: Vec<GpuVertex>,
    indices: Vec<u16>,
}

impl ZoneGeometry {
    pub fn build(scene: &ZoneScene) -> Self {
        Self::build_keyed(scene, &scene.light_keys, &[])
    }

    /// With the point-light table `keys` (a sub-area's light refs resolve into the zone's table, FUN_1017a5e0).
    /// `areas`: area keys already in slots (the zone's, for a sub-area); new ones follow.
    pub fn build_keyed(scene: &ZoneScene, keys: &[u32], areas: &[u32]) -> Self {
        // Slot 0 is the plain white texture; scene textures follow in order.
        let texture_slot: HashMap<&str, usize> = scene.textures.iter().enumerate().map(|(i, t)| (t.name.as_str(), i + 1)).collect();
        // A placement's refs become their table slot + 1, except keys starting with 'c', which the main scenery
        // path skips (FUN_10175ac0).
        let light_keys: Vec<u32> = keys.iter().copied().take(LIGHTS).collect();
        let slot_of = |key: u32, part: bool| -> u32 {
            match light_keys.iter().position(|&k| k == key) {
                Some(s) if key != 0 && (part || key as u8 != b'c') => s as u32 + 1,
                _ => 0,
            }
        };
        // One instance block per mesh, holding the matrices of every placement that uses it.
        let mut area_keys = areas.to_vec();
        let mut door_parts: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut door_bases: HashMap<u32, Vec<glam::Mat4>> = HashMap::new();
        let mut next_part = 1u32;
        let mesh_names: std::collections::HashSet<&str> = scene.meshes.iter().map(|m| m.name.as_str()).collect();
        let mut by_mesh: HashMap<String, Vec<(Instance, u32, bool)>> = HashMap::new();
        for p in scene.placements.iter().filter(|p| p.drawn()) {
            let area = match area_keys.iter().position(|&k| k == p.area) {
                _ if p.area == 0 => 0,
                Some(s) => s as u32 + 1,
                None if area_keys.len() + 1 < AREAS => {
                    area_keys.push(p.area);
                    area_keys.len() as u32
                }
                None => 0,
            };
            // FUN_1017c9c0: high to near², medium to far², low beyond (near = +0x38, far = max(+0x38, +0x3c)); door
            // groups always take high (FUN_10183da0).
            let [l, m, h] = p.lod_names(|n| mesh_names.contains(n));
            let (near, far) = (p.lod[0] * p.lod[0], p.lod[0].max(p.lod[1]).powi(2));
            let bands: Vec<(std::borrow::Cow<str>, [f32; 2])> = if p.id != 0 || (l == m && m == h) {
                vec![(h, [0.0, 0.0])]
            } else {
                let mut v: Vec<(std::borrow::Cow<str>, [f32; 2])> = vec![(h, [-1.0, near]), (m, [near, far]), (l, [far, f32::MAX])];
                // A mesh in two neighbouring slots draws across both bands.
                let mut k = 1;
                while k < v.len() {
                    if v[k].0 == v[k - 1].0 {
                        v[k - 1].1[1] = v[k].1[1];
                        v.remove(k);
                    } else {
                        k += 1;
                    }
                }
                v
            };
            let mut part = 0;
            if p.id != 0 && (next_part as usize) < PARTS {
                let parts = door_parts.entry(p.id).or_default();
                if parts.len() < 4 {
                    part = next_part;
                    parts.push(part);
                    next_part += 1;
                    door_bases.entry(p.id).or_default().push(glam::Mat4::from_cols_array_2d(&p.matrix()));
                }
            }
            // Door parts draw through FUN_1017ce60 (2019), which takes every ref, 'c' keys too.
            let lights = p.lights.map(|k| slot_of(k, part != 0));
            for (name, b) in bands {
                let inst = Instance { model: p.matrix(), lights, area: [area, 0, 0, 0], band: [b[0], b[1], part as f32, 0.0] };
                by_mesh.entry(name.into_owned()).or_default().push((inst, p.link, p.scale[0] * p.scale[2] < 0.0));
            }
        }
        let mut matrices: Vec<Instance> = Vec::new();
        let mut links = Vec::new();
        let mut draws = Vec::new();
        let mut all_verts: Vec<GpuVertex> = Vec::new();
        let mut all_indices: Vec<u16> = Vec::new();
        for mesh in &scene.meshes {
            let Some(mut mats) = by_mesh.remove(mesh.name.as_str()) else { continue };
            // Normal placements first, then the mirrored ones (culled the other way).
            mats.sort_by_key(|m| m.2);
            let start = matrices.len() as u32;
            let split = start + mats.iter().filter(|m| !m.2).count() as u32;
            for (inst, link, _) in mats {
                if link != 0 {
                    links.push((matrices.len() as u32, link, inst));
                }
                matrices.push(inst);
            }
            let (instances, mirrored) = (start..split, split..matrices.len() as u32);
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
                        GpuVertex { pos: v.pos, normal: v.normal, color: [r, g, b, a], uv: v.uv, flags: (model.blending & 0x1000 != 0) as u32 }
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
                    mirrored: mirrored.clone(),
                    cull: model.blending & 0x2000 == 0,
                    texture: texture_slot.get(model.texture.as_str()).copied().unwrap_or(0),
                    // Same rule as the reference viewer: '_' meshes and blend bit 0x8000 are cut out.
                    mode: mesh.name.starts_with('_') as u8 | ((model.blending & 0x8000 != 0) as u8) << 1,
                });
            }
        }
        // A scene with no scenery (the character screen) still needs a non-empty buffer to bind.
        if matrices.is_empty() {
            matrices.push(Instance { model: Mat4::IDENTITY.to_cols_array_2d(), lights: [0; 4], area: [0; 4], band: [0.0; 4] });
        }
        if all_verts.is_empty() {
            all_verts.push(GpuVertex::zeroed());
        }
        while all_indices.len() % 2 != 0 || all_indices.is_empty() {
            all_indices.push(0);
        }
        Self { matrices, links, light_keys, area_keys, door_parts, door_bases, draws, verts: all_verts, indices: all_indices }
    }

    fn parts(&self) -> [&[u8]; 3] {
        [bytemuck::cast_slice(&self.verts), bytemuck::cast_slice(&self.indices), bytemuck::cast_slice(&self.matrices)]
    }
}

/// Mip texels a frame (render/mipgen.rs): while a zone goes up, and in the world (new models spread over frames).
pub const MIP_BUDGET: u64 = 4 << 20;
pub const MIP_BUDGET_WORLD: u64 = 1 << 20;

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
        // Two vec4s a light: (position, range), (colour x intensity, 0); written by the effects update pass.
        let lights = device.create_buffer(&wgpu::BufferDescriptor { label: Some("point lights"), size: (LIGHTS * 32) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let areas = device.create_buffer(&wgpu::BufferDescriptor { label: Some("area lighting"), size: (AREAS * std::mem::size_of::<AreaGpu>()) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let parts_gpu = device.create_buffer(&wgpu::BufferDescriptor { label: Some("door parts"), size: (PARTS * std::mem::size_of::<PartGpu>()) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &p.globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: lights.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: areas.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: parts_gpu.as_entire_binding() },
            ],
        });
        let sampler = mip_sampler(device);
        // Slot 0 is plain white for models whose texture lives elsewhere; the others fill in as they go up.
        let white = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 255, 255, 255]);
        // Slot 0 of actors: magenta, so a missing texture is obvious.
        let magenta = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 0, 255, 255]);
        let shine_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("actor shine"), size: SHINE_SLOTS as u64 * SHINE_STRIDE, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let shine_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actor shine"),
            layout: &p.shine_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &shine_buf, offset: 0, size: wgpu::BufferSize::new(32) }) }],
        });
        let refract_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("actor refraction"), size: SHINE_SLOTS as u64 * SHINE_STRIDE, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let refract_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actor refraction"),
            layout: &p.shine_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &refract_buf, offset: 0, size: wgpu::BufferSize::new(32) }) }],
        });
        let [v, i, m] = geo.parts().map(|b| b.len() as u64);
        let buffer = |label, size: u64, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self {
            scenery: p.scenery,
            actor_pipeline: p.actor_pipeline,
            sky_pipeline: p.sky_pipeline,
            texture_layout: p.texture_layout,
            sampler,
            uniforms,
            globals,
            textures: vec![white; textures + 1],
            magenta,
            instances: buffer("instances", m, wgpu::BufferUsages::VERTEX),
            lights,
            light_keys: geo.light_keys.clone(),
            vertices: buffer("scenery vertices", v, wgpu::BufferUsages::VERTEX),
            indices: buffer("scenery indices", i, wgpu::BufferUsages::INDEX),
            draws: geo.draws.clone(),
            skin_pipeline: p.skin_pipeline,
            shine_pipeline: p.shine_pipeline,
            shine_buf,
            shine_group,
            refract_pipeline: p.refract_pipeline,
            refract_buf,
            refract_group,
            refract_copy: None,
            shine_tex: HashMap::new(),
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
            points: points::Points::new(gpu),
            occl: occlusion::Occlusion::new(gpu),
            sub: None,
            furniture: Vec::new(),
            furn_base: None,
            links: geo.links.clone(),
            hidden: 0,
            zone_keys: geo.light_keys.len(),
            area_keys: geo.area_keys.clone(),
            zone_areas: geo.area_keys.len(),
            area_lights: Vec::new(),
            areas,
            door_parts: geo.door_parts.clone(),
            door_bases: geo.door_bases.clone(),
            parts: vec![PartGpu::default(); PARTS],
            parts_gpu,
            now: 0.0,
        }
    }

    /// Scheduler op 0x0d (rotate) / 0x0c (translate) on door part `slot`: from its value now to `to` over `ticks`
    /// (FUN_100439c0 / FUN_10043bb0; angles wrapped to [-3.1415, 3.1415] as FUN_100acdb0 stores them).
    pub fn move_part(&mut self, gpu: &Gpu, slot: u32, rotate: bool, to: [f32; 3], ticks: f32) {
        let Some(p) = self.parts.get_mut(slot as usize) else { return };
        let now = self.now;
        let wrap = |a: f32| if (-3.1415..=3.1415).contains(&a) { a } else { a - (a / 6.283).round() * 6.283 };
        let to = if rotate { to.map(wrap) } else { to };
        let (from, dst, time) = if rotate { (&mut p.rot_from, &mut p.rot_to, &mut p.rot_time) } else { (&mut p.pos_from, &mut p.pos_to, &mut p.pos_time) };
        let cur = PartGpu::at(*from, *dst, *time, now);
        *from = [cur[0], cur[1], cur[2], 0.0];
        *dst = [to[0], to[1], to[2], 0.0];
        *time = [now, ticks.max(0.0), 0.0, 0.0];
        let at = slot as u64 * std::mem::size_of::<PartGpu>() as u64;
        gpu.queue.write_buffer(&self.parts_gpu, at, bytemuck::bytes_of(&*p));
    }

    /// Area 4ccs by slot - 1 (zone's, then the shown sub-area's).
    pub fn area_keys(&self) -> &[u32] {
        &self.area_keys
    }

    /// This frame's lighting per area slot - 1 (None: no environment, lit as the zone).
    pub fn set_area_lights(&mut self, lights: Vec<Option<crate::dat::weather::Lighting>>) {
        self.area_lights = lights;
    }

    /// A sub-area's scenery replaces the loaded one (FUN_101787c0 state 2); `light_keys` is the zone's table with
    /// the sub-area's keys after it. The stand-ins for `link` hide once it is on the GPU (FUN_1017a630).
    pub fn set_sub(&mut self, gpu: &Gpu, link: u32, geo: &ZoneGeometry, textures: Vec<crate::dat::zone::Texture>, light_keys: Vec<u32>) {
        // Furniture textures sit after the zone's; no Mog House has a sub-area.
        self.clear_furniture();
        self.clear_sub();
        let device = &gpu.device;
        let buffer = |label, bytes: &[u8], usage| {
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (bytes.len() as u64).max(4), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.write_buffer(&b, 0, bytes);
            b
        };
        let [v, i, m] = geo.parts();
        let tex_base = self.textures.len();
        let white = self.textures[0].clone();
        self.textures.extend(std::iter::repeat_n(white, textures.len()));
        self.pending.extend(textures.into_iter().enumerate().map(|(k, t)| (tex_base + k, t, None)));
        let draws = geo.draws.iter().map(|d| Draw { texture: if d.texture == 0 { 0 } else { tex_base + d.texture - 1 }, instances: d.instances.clone(), mirrored: d.mirrored.clone(), ..*d }).collect();
        self.sub = Some(SubGeometry {
            vertices: buffer("sub-area vertices", v, wgpu::BufferUsages::VERTEX),
            indices: buffer("sub-area indices", i, wgpu::BufferUsages::INDEX),
            instances: buffer("sub-area instances", m, wgpu::BufferUsages::VERTEX),
            draws,
            tex_base,
            link,
            shown: false,
        });
        self.light_keys = light_keys;
        self.area_keys = geo.area_keys.clone();
        // Offscreen renderers take everything at once.
        if !self.budgeted {
            for (slot, t, _) in std::mem::take(&mut self.pending) {
                self.put_texture(gpu, slot, &t);
            }
            self.hide_links(gpu, link);
            if let Some(s) = &mut self.sub {
                s.shown = true;
            }
        }
    }

    /// Mog House furniture replaces the set before it: each model file is its own geometry with its own textures
    /// (names repeat across files), drawn once all of the set's textures are up.
    pub fn set_furniture(&mut self, gpu: &Gpu, files: Vec<(std::sync::Arc<ZoneGeometry>, Vec<crate::dat::zone::Texture>)>) {
        self.clear_furniture();
        let device = &gpu.device;
        let buffer = |label, bytes: &[u8], usage| {
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (bytes.len() as u64).max(4), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.write_buffer(&b, 0, bytes);
            b
        };
        self.furn_base = Some(self.textures.len());
        for (geo, textures) in files {
            let [v, i, m] = geo.parts();
            let tex_base = self.textures.len();
            let white = self.textures[0].clone();
            self.textures.extend(std::iter::repeat_n(white, textures.len()));
            self.pending.extend(textures.into_iter().enumerate().map(|(k, t)| (tex_base + k, t, None)));
            let draws = geo.draws.iter().map(|d| Draw { texture: if d.texture == 0 { 0 } else { tex_base + d.texture - 1 }, instances: d.instances.clone(), mirrored: d.mirrored.clone(), ..*d }).collect();
            self.furniture.push(SubGeometry {
                vertices: buffer("furniture vertices", v, wgpu::BufferUsages::VERTEX),
                indices: buffer("furniture indices", i, wgpu::BufferUsages::INDEX),
                instances: buffer("furniture instances", m, wgpu::BufferUsages::VERTEX),
                draws,
                tex_base,
                link: 0,
                shown: false,
            });
        }
        if !self.budgeted {
            let base = self.furn_base.unwrap_or(0);
            let (now, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending).into_iter().partition(|p| p.0 >= base);
            self.pending = keep.into();
            for (slot, t, _) in now {
                self.put_texture(gpu, slot, &t);
            }
            for f in &mut self.furniture {
                f.shown = true;
            }
        }
    }

    pub fn clear_furniture(&mut self) {
        if let Some(base) = self.furn_base.take() {
            self.textures.truncate(base);
            self.pending.retain(|p| p.0 < base);
            self.furniture.clear();
        }
    }

    /// The sub-area goes (FUN_10178ac0). Its stand-ins stay hidden: unloading does not run FUN_1017a630 again.
    pub fn clear_sub(&mut self) {
        if let Some(s) = self.sub.take() {
            self.textures.truncate(s.tex_base);
            self.pending.retain(|p| p.0 < s.tex_base);
            self.light_keys.truncate(self.zone_keys);
            self.area_keys.truncate(self.zone_areas);
        }
    }

    /// FUN_1017a630: zone instances whose link is `link` draw nothing, the others as built.
    fn hide_links(&mut self, gpu: &Gpu, link: u32) {
        if link == self.hidden {
            return;
        }
        for (i, l, inst) in &self.links {
            if *l == link || *l == self.hidden {
                let v = if *l == link { Instance::zeroed() } else { *inst };
                gpu.queue.write_buffer(&self.instances, *i as u64 * INSTANCE_BYTES, bytemuck::bytes_of(&v));
            }
        }
        self.hidden = link;
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
        let sub_up = self.sub.as_ref().filter(|s| !s.shown && self.geo_pending.is_none() && !self.pending.iter().any(|p| p.0 >= s.tex_base) && gpu.mips_done() >= self.mips_seq).map(|s| s.link);
        if let Some(link) = sub_up {
            self.hide_links(gpu, link);
            if let Some(s) = &mut self.sub {
                s.shown = true;
            }
        }
        if let Some(base) = self.furn_base
            && self.furniture.iter().any(|f| !f.shown)
            && self.geo_pending.is_none()
            && !self.pending.iter().any(|p| p.0 >= base)
            && gpu.mips_done() >= self.mips_seq
        {
            for f in &mut self.furniture {
                f.shown = true;
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
        a.draws = mesh.groups.iter().map(|(tex, r, wep)| (a.slot.get(tex.as_str()).copied().unwrap_or(0), r.clone(), *wep)).collect();
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
            let draws = mesh.groups.iter().map(|(tex, r, wep)| (slot.get(tex.as_str()).copied().unwrap_or(0), r.clone(), *wep)).collect();
            let size = (std::mem::size_of_val(mesh.verts.as_slice()) as u64).max(4);
            let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("skinned actor"), size, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let instance = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("actor instance"), size: INSTANCE_BYTES, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let actor = ActorGpu {
                textures: vec![self.magenta.clone(); ch.textures.len() + 1],
                slot,
                vertices: None,
                draws,
                instance,
                hidden_weps: ch.init_hidden_mask(),
                shine: None,
                refract: None,
                skin: Some(Skinned { vertices, bones: mesh.bones, offset: 0, frame: u64::MAX }),
                alpha: 1.0,
                tint: [1.0; 3],
                flash: [0.0, 1.0],
                lights: [0; 4],
                area: 0,
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
        overlay: Option<(&crate::dat::character::Animation, f32, f32)>,
    ) {
        if actor.skin.is_none() {
            return;
        }
        self.pose_actor(actor, &[], model);
        if let Some(skin) = &actor.skin {
            let out = skin.offset / 16 + POSE_HEAD as u32;
            self.anim.job(ch, files, to, from, t, out, look, overlay);
        }
    }

    /// Queues a skinned actor's bones (world transforms from character::pose) and placement.
    pub fn pose_actor(&mut self, actor: &mut ActorGpu, bones: &[(glam::Quat, Vec3)], model: Mat4) {
        let alpha = actor.alpha.clamp(0.0, 1.0);
        let tint = actor.tint;
        let flash = actor.flash;
        let area = match self.area_keys.iter().position(|&k| k == actor.area) {
            Some(s) if actor.area != 0 => (s + 1) as f32,
            _ => 0.0,
        };
        let Some(skin) = actor.skin.as_mut() else { return };
        while self.poses.len() % 16 != 0 {
            self.poses.push([0.0; 4]);
        }
        skin.offset = (self.poses.len() * 16) as u32;
        skin.frame = self.pose_frame;
        self.poses.extend(model.to_cols_array_2d());
        self.poses.push([alpha, flash[0], flash[1], f32::from_bits(u32::from_le_bytes(actor.lights))]);
        self.poses.push([area, tint[0], tint[1], tint[2]]);
        for i in 0..skin.bones {
            let (q, t) = bones.get(i).copied().unwrap_or((glam::Quat::IDENTITY, Vec3::ZERO));
            self.poses.push(q.to_array());
            self.poses.push([t.x, t.y, t.z, 0.0]);
        }
        self.poses.push([0.0, 0.0, 0.0, 1.0]);
        self.poses.push([0.0; 4]);
    }

    /// Asks for an attach point (bone, offset) of an actor posed this frame, relative to its root in world axes,
    /// read back later by key (points::Points::take).
    pub fn query_point(&mut self, actor: &ActorGpu, key: u64, bone: u16, off: Vec3) {
        if let Some(skin) = actor.skin.as_ref().filter(|s| s.frame == self.pose_frame) {
            self.points.query(key, skin.offset / 16, bone, off);
        }
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
            self.points.run(gpu, encoder, buf);
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
            size: INSTANCE_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ActorGpu { textures, slot, vertices: None, draws: Vec::new(), instance, hidden_weps: 0, shine: None, refract: None, skin: None, alpha: 1.0, tint: [1.0; 3], flash: [0.0, 1.0], lights: [0; 4], area: 0, mips }
    }

    /// A shine image goes up once (effect file, IMG node).
    pub fn has_shine(&self, key: (u32, usize)) -> bool {
        self.shine_tex.contains_key(&key)
    }

    pub fn add_shine(&mut self, gpu: &Gpu, key: (u32, usize), t: &crate::dat::zone::Texture) {
        let view = upload_texture(gpu, t, true);
        self.shine_tex.insert(key, view_bind_group(gpu, &self.texture_layout, &self.sampler, &view));
    }

    /// Uploads this frame's posed mesh and placement.
    pub fn update_actor(&self, gpu: &Gpu, actor: &mut ActorGpu, mesh: &character::PosedMesh, model: Mat4) {
        let mut verts: Vec<GpuVertex> = Vec::new();
        actor.draws.clear();
        // 0x80 = 1.0 as for scenery vertex colours.
        let a = (actor.alpha.clamp(0.0, 1.0) * 128.0).round() as u8;
        for (tex, mat, tris) in &mesh.batches {
            let start = verts.len() as u32;
            // As vs_skinned: red the light factor (0x80 = 1.0), green the cloth flag.
            let color = [(mat[0] * 128.0).round().clamp(0.0, 255.0) as u8, if mat[1] > 0.5 { 255 } else { 0 }, 128, a];
            verts.extend(tris.iter().map(|v| GpuVertex { pos: [v[0], v[1], v[2]], normal: [v[3], v[4], v[5]], color, uv: [v[6], v[7]], flags: 0 }));
            actor.draws.push((actor.slot.get(tex.as_str()).copied().unwrap_or(0), start..verts.len() as u32, NO_WEP));
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
            area_info: [0.0; 4],
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
        // FUN_10183da0: instances with an area are lit by its environment; flagged models by 'ev10' while a sub-area
        // is in (+0x50, FUN_101787c0).
        u.area_info[1] = self.now;
        let ev10 = u32::from_le_bytes(*b"ev10");
        if self.sub.as_ref().is_some_and(|s| s.shown)
            && let Some(s) = self.area_keys.iter().position(|&k| k == ev10)
        {
            u.area_info[0] = (s + 1) as f32;
        }
        let mut areas = [AreaGpu::zeroed(); AREAS];
        for (slot, l) in self.area_lights.iter().enumerate().take(AREAS - 1) {
            if let Some(l) = l {
                areas[slot + 1] = AreaGpu { sun: v4(l.dir, 1.0), sun_color: v4(l.diffuse, 1.0), ambient: v4(l.ambient, 1.0), fog_color: v4(l.fog, 1.0), fog: [l.fog_start, l.fog_end, 0.0, 0.0], actor_sun: v4(l.actor_dir, 1.0), actor_color: v4(l.actor_diffuse, 1.0), actor_ambient: v4(l.actor_ambient, 1.0) };
            }
        }
        gpu.queue.write_buffer(&self.areas, 0, bytemuck::cast_slice(&areas));
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
        for mode in 0..4u8 {
            let mut bound = usize::MAX;
            let mut pipe = usize::MAX;
            for d in self.draws.iter().filter(|d| d.mode == mode) {
                self.draw_scenery(&mut pass, d, mode, &mut bound, &mut pipe);
            }
            if let Some(s) = self.sub.as_ref().filter(|s| s.shown) {
                pass.set_vertex_buffer(1, s.instances.slice(..));
                pass.set_vertex_buffer(0, s.vertices.slice(..));
                pass.set_index_buffer(s.indices.slice(..), wgpu::IndexFormat::Uint16);
                for d in s.draws.iter().filter(|d| d.mode == mode) {
                    self.draw_scenery(&mut pass, d, mode, &mut bound, &mut pipe);
                }
                pass.set_vertex_buffer(1, self.instances.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
            for f in self.furniture.iter().filter(|f| f.shown) {
                pass.set_vertex_buffer(1, f.instances.slice(..));
                pass.set_vertex_buffer(0, f.vertices.slice(..));
                pass.set_index_buffer(f.indices.slice(..), wgpu::IndexFormat::Uint16);
                for d in f.draws.iter().filter(|d| d.mode == mode) {
                    self.draw_scenery(&mut pass, d, mode, &mut bound, &mut pipe);
                }
            }
            if self.furniture.iter().any(|f| f.shown) {
                pass.set_vertex_buffer(1, self.instances.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
        }
        if let Some((_, group, _)) = &self.pose_gpu {
            pass.set_pipeline(&self.skin_pipeline);
            for a in actors.iter().filter(|a| a.mips <= done && a.refract.is_none()) {
                let Some(skin) = a.skin.as_ref().filter(|s| s.frame == self.pose_frame) else { continue };
                pass.set_bind_group(2, group, &[skin.offset]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                for (tex, range, wep) in &a.draws {
                    if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                        continue;
                    }
                    pass.set_bind_group(1, &a.textures[*tex], &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
            // The shine overlay: every batch of the actor again, its image mapped by the world normal.
            let mut slot = 0u64;
            for a in actors.iter().filter(|a| a.mips <= done && a.refract.is_none()) {
                let (Some(s), Some(skin)) = (a.shine, a.skin.as_ref().filter(|s| s.frame == self.pose_frame)) else { continue };
                let Some(tex) = self.shine_tex.get(&s.key) else { continue };
                if slot >= SHINE_SLOTS as u64 {
                    break;
                }
                gpu.queue.write_buffer(&self.shine_buf, slot * SHINE_STRIDE, bytemuck::cast_slice(&[s.u, s.v, 0.0, 0.0, s.tf[0], s.tf[1], s.tf[2], s.tf[3]]));
                pass.set_pipeline(&self.shine_pipeline);
                pass.set_bind_group(3, &self.shine_group, &[(slot * SHINE_STRIDE) as u32]);
                pass.set_bind_group(2, group, &[skin.offset]);
                pass.set_bind_group(1, tex, &[]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                for (_, range, wep) in &a.draws {
                    if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                        continue;
                    }
                    pass.draw(range.clone(), 0..1);
                }
                slot += 1;
            }
        }
        pass.set_pipeline(&self.actor_pipeline);
        for a in actors.iter().filter(|a| a.mips <= done) {
            let Some((buf, _)) = &a.vertices else { continue };
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.set_vertex_buffer(1, a.instance.slice(..));
            for (tex, range, _) in &a.draws {
                pass.set_bind_group(1, &a.textures[*tex], &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
    }
}

impl ZoneRenderer {
    /// Actors with the refraction pass (scheduler ops 0x48 / 0x49), in the given order (farthest first, retail's sorted
    /// list): per actor the scene so far is copied whole (FUN_10025870 into DAT_10456e6c), then its batches, its shine
    /// and the refraction pass (FUN_1002dc10 -> FUN_10025330) sampling that copy at the pixel's own screen point shifted
    /// by (L P11, L P22) / w. Without a copyable target they draw plainly.
    #[allow(clippy::too_many_arguments)]
    pub fn render_refraction(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, color_tex: Option<&wgpu::Texture>, depth: &wgpu::TextureView, camera: &Camera, aspect: f32, actors: &[&ActorGpu]) {
        let done = gpu.mips_done();
        let list: Vec<&ActorGpu> = actors.iter().copied().filter(|a| a.mips <= done && a.refract.is_some() && a.skin.as_ref().is_some_and(|s| s.frame == self.pose_frame)).take(SHINE_SLOTS).collect();
        if list.is_empty() || self.pose_gpu.is_none() {
            return;
        }
        let src = color_tex.filter(|t| t.usage().contains(wgpu::TextureUsages::COPY_SRC));
        if let Some(t) = src
            && self.refract_copy.as_ref().is_none_or(|(c, _)| c.size() != t.size() || c.format() != t.format())
        {
            let c = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("actor refraction copy"),
                size: t.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: t.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[t.format().remove_srgb_suffix()],
            });
            // Read as the zone pass wrote it: raw bytes when the shaders encode gamma themselves.
            let vf = if self.encode_gamma { t.format().remove_srgb_suffix() } else { t.format() };
            let view = c.create_view(&wgpu::TextureViewDescriptor { format: Some(vf), ..Default::default() });
            let g = view_bind_group(gpu, &self.texture_layout, &self.sampler, &view);
            self.refract_copy = Some((c, g));
        }
        let proj = camera.proj(aspect);
        let (p00, p11) = (proj.x_axis.x, proj.y_axis.y);
        let Some((_, group, _)) = &self.pose_gpu else { return };
        for (slot, a) in list.iter().enumerate() {
            let (Some(rf), Some(skin)) = (a.refract, a.skin.as_ref()) else { continue };
            let off = (slot as u64 * SHINE_STRIDE) as u32;
            gpu.queue.write_buffer(&self.refract_buf, off as u64, bytemuck::cast_slice(&[rf.level, rf.r, a.alpha, 0.0, p00, p11, 0.0, 0.0]));
            if let (Some(t), Some((c, _))) = (src, self.refract_copy.as_ref()) {
                encoder.copy_texture_to_texture(t.as_image_copy(), c.as_image_copy(), t.size());
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("actor refraction"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_bind_group(2, group, &[skin.offset]);
            pass.set_vertex_buffer(0, skin.vertices.slice(..));
            let shown = |wep: u8| wep == NO_WEP || a.hidden_weps & (1 << wep) == 0;
            pass.set_pipeline(&self.skin_pipeline);
            for (tex, range, wep) in &a.draws {
                if shown(*wep) {
                    pass.set_bind_group(1, &a.textures[*tex], &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
            // Its shine in a slot from the top of the shine buffer (render's own take the bottom).
            if let Some(s) = a.shine
                && let Some(tex) = self.shine_tex.get(&s.key)
            {
                let so = (SHINE_SLOTS - 1 - slot) as u64 * SHINE_STRIDE;
                gpu.queue.write_buffer(&self.shine_buf, so, bytemuck::cast_slice(&[s.u, s.v, 0.0, 0.0, s.tf[0], s.tf[1], s.tf[2], s.tf[3]]));
                pass.set_pipeline(&self.shine_pipeline);
                pass.set_bind_group(3, &self.shine_group, &[so as u32]);
                pass.set_bind_group(1, tex, &[]);
                for (_, range, wep) in &a.draws {
                    if shown(*wep) {
                        pass.draw(range.clone(), 0..1);
                    }
                }
            }
            if let Some((_, g)) = self.refract_copy.as_ref().filter(|_| src.is_some()) {
                pass.set_pipeline(&self.refract_pipeline);
                pass.set_bind_group(1, g, &[]);
                pass.set_bind_group(3, &self.refract_group, &[off]);
                for (_, range, wep) in &a.draws {
                    if shown(*wep) {
                        pass.draw(range.clone(), 0..1);
                    }
                }
            }
        }
    }

    /// One model: both-sided models in one draw; otherwise the normal instances culled back, the mirrored front.
    fn draw_scenery(&self, pass: &mut wgpu::RenderPass, d: &Draw, mode: u8, bound: &mut usize, pipe: &mut usize) {
        if d.texture != *bound {
            pass.set_bind_group(1, &self.textures[d.texture], &[]);
            *bound = d.texture;
        }
        let runs = if d.cull { [(1, d.instances.clone()), (2, d.mirrored.clone())] } else { [(0, d.instances.start..d.mirrored.end), (0, 0..0)] };
        for (c, range) in runs {
            if range.is_empty() {
                continue;
            }
            if *pipe != c {
                pass.set_pipeline(&self.scenery[mode as usize][c]);
                *pipe = c;
            }
            pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, range);
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
    render_pngs_post(scene, frames, width, height, setup, &mut |_, _, _, _, _, _, _| {})
}

/// Draws over the scene in each frame (after the zone pass, same targets): gpu, encoder, colour, depth, camera,
/// lighting.
pub type PostDraw<'a> = dyn FnMut(&Gpu, &mut wgpu::CommandEncoder, (&wgpu::TextureView, &wgpu::Texture), &wgpu::TextureView, &Camera, Option<&crate::dat::weather::Lighting>, &ZoneRenderer) + 'a;

/// As render_pngs_lit, with `post` drawing over each frame.
pub fn render_pngs_post(
    scene: &ZoneScene,
    frames: &[(&Camera, &std::path::Path, Option<crate::dat::weather::Lighting>)],
    width: u32,
    height: u32,
    setup: &dyn Fn(&Gpu, &mut ZoneRenderer) -> Vec<ActorGpu>,
    post: &mut PostDraw,
) -> Result<usize> {
    let gpu = Gpu::headless()?;
    // A plain (not sRGB) target: the scene is written and blended in gamma space, as D3D8 did; its bytes are the PNG's.
    let format = wgpu::TextureFormat::Rgba8Unorm;
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
        renderer.render_refraction(&gpu, &mut encoder, &view, Some(&target), &depth, camera, width as f32 / height as f32, &actor_refs);
        post(&gpu, &mut encoder, (&view, &target), &depth, camera, lighting.as_ref(), &renderer);
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

#[cfg(test)]
mod refract_tests {
    use super::Refract;

    /// FUN_100d6d30: TFACTOR alpha 0x80 -> 0, 0x40 -> 0x40, 0x0b -> 0x75, 0xff -> 0x81 (it wraps).
    #[test]
    fn refract_tf_alpha() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert_eq!(Refract::tf_alpha(0x80), 0.0);
        assert!(near(Refract::tf_alpha(0x40), 0.250_980) && near(Refract::tf_alpha(0x0b), 0.458_824) && near(Refract::tf_alpha(0xff), 0.505_882));
    }
}
