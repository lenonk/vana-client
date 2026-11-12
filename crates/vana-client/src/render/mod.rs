//! wgpu renderer for zone scenery.

pub mod anim;
pub mod character;
pub mod cloth;
pub mod cull;
pub mod dancer;
pub mod effects;
pub mod gputime;
pub mod invwin;
pub mod lampshadow;
pub mod leak;
pub mod mipgen;
pub mod movie;
pub mod mtext;
pub mod occlusion;
pub mod points;
pub mod portrait;
pub mod postfx;
pub mod scale;
pub mod screenfx;
pub mod screens;
pub mod shadow;
pub mod smaa;
pub mod ssao;
pub mod sunshadow;
pub mod sway;
pub mod ui;
pub mod upload;

use crate::dat::zone::ZoneScene;
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Gpu {
    /// The instance the surface came from (held, never read).
    #[allow(dead_code)]
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
    cloth: std::sync::OnceLock<(wgpu::ComputePipeline, wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    zone: std::sync::Mutex<HashMap<wgpu::TextureFormat, ZonePipes>>,
}

impl PipeCache {
    pub fn mips(&self) -> &mipgen::Pipes {
        self.mips.get_or_init(|| mipgen::Pipes::new(&self.device))
    }

    pub fn anim(&self) -> &(wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        self.anim.get_or_init(|| anim::pipeline(&self.device))
    }

    pub fn cloth(&self) -> &(wgpu::ComputePipeline, wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        self.cloth.get_or_init(|| cloth::pipelines(&self.device))
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
    // One per card: each backend (Vulkan, GL, ...) lists it, with its driver part ("... (RADV NAVI21)", "...
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
        // BC textures go up compressed when the adapter has them (zone and model DXT1 / DXT3); timestamps between
        // passes give the F12 overlay its GPU times (render/gputime.rs); indirect draws from any instance let the
        // scenery be culled on the GPU (render/cull.rs).
        let mut wanted = wgpu::Features::TEXTURE_COMPRESSION_BC
            | wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | wgpu::Features::INDIRECT_FIRST_INSTANCE
            | wgpu::Features::POLYGON_MODE_LINE;
        // FFXI_NO_GPU_TIME / FFXI_NO_CULL: without the timestamps or the culling's indirect draws (driver checks).
        if std::env::var_os("FFXI_NO_GPU_TIME").is_some() {
            wanted.remove(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        }
        if std::env::var_os("FFXI_NO_CULL").is_some() {
            wanted.remove(wgpu::Features::INDIRECT_FIRST_INSTANCE);
        }
        // Not BC on GL: wgpu's GL backend drops a compressed copy from a buffer whose rows are padded (it passes the
        // padded size to glCompressedTexSubImage2D), so the GPU-made mips of BC textures (render/mipgen.rs) would
        // stay black. GL takes the RGBA path instead.
        if info.backend == wgpu::Backend::Gl {
            wanted.remove(wgpu::Features::TEXTURE_COMPRESSION_BC);
        }
        let required_features = adapter.features() & wanted;
        log::info!("device features {required_features:?}");
        BC.store(
            required_features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            std::sync::atomic::Ordering::Relaxed,
        );
        set_uncompressed(uncompressed());
        // The effects update pass binds 9 storage buffers (the default limit is 8; desktop GPUs allow far more).
        let required_limits = wgpu::Limits {
            max_storage_buffers_per_shader_stage: adapter.limits().max_storage_buffers_per_shader_stage.min(16),
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features,
            required_limits,
            ..Default::default()
        }))?;
        let pipes = std::sync::Arc::new(PipeCache {
            device: device.clone(),
            mips: Default::default(),
            anim: Default::default(),
            cloth: Default::default(),
            zone: Default::default(),
        });
        Ok(Self { instance, adapter, device, queue, mips: Default::default(), uploads: Default::default(), pipes })
    }

    /// Makes the shared pipelines on a worker thread, so no frame compiles them later.
    pub fn warm(&self, formats: &[wgpu::TextureFormat]) {
        let (pipes, formats) = (self.pipes.clone(), formats.to_vec());
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            pipes.mips();
            pipes.anim();
            pipes.cloth();
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
    /// Bit 1: a bump-mapped corner (config 0017). Bits 8..31: ZoneGeometry::deltas index + 1 (0 none): an animated
    /// mesh's delta (render/sway.rs), or with bit 1 the corner's tangent.
    flags: u32,
}

/// An overlay line end: world position and colour (gamma-space RGBA).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Debug)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
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
    /// The voyage matrix (identity off a ship's voyage).
    voyage: [[f32; 4]; 4],
    /// Config 0017 off (FUN_10181b50): the sun and the moon as two lights, (direction, colour) each.
    two: [[f32; 4]; 4],
    /// x: 1 with bump mapping on (one eased light, vertex fog), 0 off (two lights, table fog).
    modes: [f32; 4],
    /// Windowed mode's light boost (light_boost): rgb factors, w = 1 on.
    boost: [f32; 4],
}

/// Windowed mode's light boost (FUN_1000be50, the SetLight wrapper FUN_1000bf30's helper; only while config 0034 !=
/// 0, where no gamma ramp is set): a light whose diffuse r, g and b are all below 0.8 has each multiplied by 2.3 x
/// ramp[i] / 65535 of its channel's gamma ramp (FUN_10017100: red entry 120, green 116, blue 128) and capped at 1.
/// The ramp is FUN_100171b0's, built whether or not it is set: ftol(65535 (i / 255)^(1 / max(g, 0.01))), g the
/// channel's gamma (base 0028 + 1 + Gameplay R / G / B / 100). Returns the factors, w = 1.
pub fn light_boost(gamma: [f32; 3]) -> [f32; 4] {
    let ramp = |i: u32, g: f32| -> u16 {
        let g = g.max(0.01) as f64;
        ((i as f64 * (1.0f32 / 255.0) as f64).powf(1.0 / g) * 65535.0) as u16
    };
    let f = |i: u32, g: f32| ramp(i, g) as f32 * f32::from_bits(0x381333c6);
    [f(120, gamma[0]), f(116, gamma[1]), f(128, gamma[2]), 1.0]
}

/// A colour through the windowed boost (zone.wgsl `boost`, FUN_1000be50 for lights, FUN_10188e00 for the ambient):
/// with the boost on and every part below 0.8, each part times its factor, capped at 1.
pub fn boosted(c: glam::Vec3, boost: [f32; 4]) -> glam::Vec3 {
    if boost[3] > 0.5 && c.max_element() < 0.8 {
        (c * glam::Vec3::new(boost[0], boost[1], boost[2])).min(glam::Vec3::ONE)
    } else {
        c
    }
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
    /// The two lights of bump-off mode, as Uniforms::two.
    two: [[f32; 4]; 4],
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
    /// The indirect argument slots of its two runs (draw_runs), when the scenery is culled.
    args: [u32; 2],
    /// A bump-mapped model's normal map (ZoneGeometry::bumps index; config 0017).
    bump: Option<usize>,
}

impl Draw {
    /// Its runs: the normal placements culled back and the mirrored ones front, or all of them in one draw.
    fn runs(&self) -> [(usize, std::ops::Range<u32>); 2] {
        if self.cull {
            [(1, self.instances.clone()), (2, self.mirrored.clone())]
        } else {
            [(0, self.instances.start..self.mirrored.end), (0, 0..0)]
        }
    }
}

/// The scene projection's near and far planes. Near is retail's 0.1 (FUN_100146e0): the eye keeps only 0.17 off the
/// floor, and a deeper near plane cut into slopes beside it.
pub const NEAR: f32 = 0.1;
/// A sub-area draw's texture from this up is the zone's slot (texture - ZONE_TEXTURE).
const ZONE_TEXTURE: usize = 1 << 20;
pub const FAR: f32 = 2000.0;

#[derive(Clone, Copy, Default)]
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
    /// Modes 2 / 3 (the blended classes) for furniture: depth writes on, no bias (batch 7 item 23).
    scenery_furn: [[wgpu::RenderPipeline; 3]; 2],
    scenery_bump: [wgpu::RenderPipeline; 3],
    /// The scenery as lines by cull, where the device draws lines (a mod's wireframe switch).
    scenery_wire: Option<[wgpu::RenderPipeline; 3]>,
    pub wireframe: bool,
    actor_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    /// Layout mode's overlay lines (set_lines): the buffer and its capacity in vertices, and what it holds.
    line_buf: Option<(wgpu::Buffer, usize)>,
    lines: Vec<LineVertex>,
    /// The Lua mods' world shapes (set_world_shapes): line and triangle lists tested against the scene's depth.
    world_pipelines: [wgpu::RenderPipeline; 2],
    world_bufs: [Option<(wgpu::Buffer, usize)>; 2],
    world_shapes: [Vec<LineVertex>; 2],
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    globals: wgpu::BindGroup,
    textures: Vec<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    /// The animated meshes' sway (config 0011, render/sway.rs), run before the culling.
    sway: Option<sway::Sway>,
    /// Config 0017: the bumped draws' normal maps, the bumped pipelines, and the lighting mode (on: one eased light
    /// and vertex fog; off: two lights and table fog).
    bump_groups: Vec<wgpu::BindGroup>,
    pub bump_mode: bool,
    /// Settings' Light Shading (ours): false Retail (the scenery's point lights per vertex, hard cut at the range),
    /// true Smooth (per pixel, soft edge; not retail).
    pub smooth_lights: bool,
    /// Windowed mode's light boost (light_boost; zero = off).
    pub light_boost: [f32; 4],
    /// The zone's scenery culled on the GPU (render/cull.rs) when the device draws indirectly, and its passes.
    cull: Option<cull::Cull>,
    cull_pipes: [wgpu::ComputePipeline; 2],
    cull_layout: wgpu::BindGroupLayout,
    /// Ours (the Lamp Lights setting): each zone instance's light slots and world sphere, the light slots
    /// written over them now (instance -> slots), and lamps waiting for the streamed geometry (set_lamps).
    inst_lights: Vec<[u32; 4]>,
    spheres: Vec<[f32; 4]>,
    lamp_patch: HashMap<u32, [u32; 4]>,
    lamps_wanted: Option<Vec<(Vec3, u32)>>,
    /// The lamps last applied (a sub-area loading after them takes them too).
    lamps_last: Vec<(Vec3, u32)>,
    /// The zone's instances' placements and camera lists (ZoneGeometry), and the list in force (MZB offset, 0 none).
    inst_placement: Vec<u32>,
    camera_lists: HashMap<u32, Vec<u32>>,
    /// The point-light slot leak replayed per frame (render/leak.rs), when a scenery placement has a 'c' ref.
    leak: Option<leak::LeakState>,
    /// The list in force and, beyond retail's draw distance, the reach past which unlisted placements draw (ours).
    camera_list: (u32, Option<f32>),
    /// Per placement its variant group and whether it is the group's simplest mesh (ZoneScene::variant_groups).
    variants: Vec<Option<(u32, bool)>>,
    /// All scenery in one vertex and one index buffer, uploaded once; draws keep the file order.
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draws: Vec<Draw>,
    skin_pipeline: wgpu::RenderPipeline,
    shine_pipeline: wgpu::RenderPipeline,
    shine_buf: wgpu::Buffer,
    shine_wrap_layout: wgpu::BindGroupLayout,
    refract_pipeline: wgpu::RenderPipeline,
    /// Actor ground shadows (render/shadow.rs): the zone layouts they draw with, the silhouette pass and its layout,
    /// and their state (made at the first shadow).
    globals_layout: wgpu::BindGroupLayout,
    sil_layout: wgpu::BindGroupLayout,
    sil_pipeline: wgpu::RenderPipeline,
    shadows: Option<shadow::Shadows>,
    /// Real character shadows (render/sunshadow.rs; prepare_sun_shadow), and whether render drew them this frame
    /// (the client then leaves retail's out).
    sun_depth_pipeline: wgpu::RenderPipeline,
    sun_depth_lamped_pipeline: wgpu::RenderPipeline,
    lamp_depth_pipeline: wgpu::RenderPipeline,
    sun_shadow: Option<sunshadow::SunShadow>,
    /// Ambient occlusion on the scenery (render/ssao.rs; prepare_ssao).
    ssao: Option<ssao::Ssao>,
    /// Lamp shadows (render/lampshadow.rs): the maps (always made) and this frame's lamps (light slot, matrix,
    /// position; empty = none), which the client sets.
    lamp_shadow: lampshadow::LampShadows,
    pub lamp_shadows: Vec<(u32, Mat4, Vec3)>,
    /// The real shadows' level from the Vana'diel clock (sunshadow::twilight; 0 at the sun / moon change) and their PCF
    /// spacing (softer by the moon).
    pub shadow_level: f32,
    pub shadow_soft: f32,
    refract_buf: wgpu::Buffer,
    refract_group: wgpu::BindGroup,
    /// The refraction pass's scene copy (DAT_10456e6c: scene size, one per frame per refracting actor).
    refract_copy: Option<(wgpu::Texture, wgpu::BindGroup)>,
    skin_solid_pipeline: wgpu::RenderPipeline,
    faded_pipeline: wgpu::RenderPipeline,
    /// Zeroes the offscreen target inside a faded actor's scissor rect before it is drawn there.
    faded_clear_pipeline: wgpu::RenderPipeline,
    /// The faded actors' fade alpha per slot (refr.a.z), and the offscreen target (FUN_1001d240's helper target, scene
    /// size) set by prepare_faded; without one render blends faded actors triangle by triangle.
    faded_buf: wgpu::Buffer,
    faded_group: wgpu::BindGroup,
    faded_rt: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
    color_format: wgpu::TextureFormat,
    /// Shine images by (effect file, IMG node).
    shine_tex: HashMap<(u32, usize), wgpu::BindGroup>,
    pose_layout: wgpu::BindGroupLayout,
    /// This frame's pose regions (256-byte aligned), and the buffer they go up in with its bind group.
    poses: Vec<[f32; 4]>,
    pose_gpu: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    pose_frame: u64,
    /// Motions evaluated on the GPU into the pose regions (render/anim.rs).
    anim: anim::GpuAnim,
    /// The actors' cloth, stepped on the GPU after the bones (render/cloth.rs).
    cloth: cloth::ClothGpu,
    /// Skinned models on the GPU by model address (the Arc keeps it).
    models: HashMap<usize, (std::sync::Arc<crate::dat::character::Character>, ActorGpu)>,
    /// Models going up piece by piece, oldest first, and the magenta slot-0 texture of actors.
    builds: Vec<(usize, Build)>,
    magenta: wgpu::BindGroup,
    /// A streamed zone (the world's): mips wait for the frame budget, textures and geometry still to go up
    /// (geometry by byte offset), the last mip job of its textures, and (bytes before, RGBA bytes, count) for the log.
    budgeted: bool,
    /// Faded actors are composited later, by render_faded_after (the world: after the effects, as retail's sorted
    /// transparent list puts them over what is behind them); others draw them at the end of render.
    pub defer_faded: bool,
    /// The shine slots render used, for render_faded_after's.
    faded_slot: std::cell::Cell<u64>,
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
    /// Mog House furniture (client/furniture.rs): a geometry per model file, its textures after `furn_base`, and each
    /// file's instances as written (move_furniture).
    furniture: Vec<SubGeometry>,
    furn_base: Option<usize>,
    furn_inst: Vec<Vec<Instance>>,
    /// Ship model actors (client/ships.rs) by entity.
    model_actors: HashMap<u32, ModelGeo>,
    /// Zone instances standing in for a sub-area (instance, link, as built), the link hidden now, and how many of
    /// `light_keys` are the zone's.
    links: Vec<(u32, u32, Instance)>,
    hidden: u32,
    /// Layout mode's top-down view hides the link-999 placements (show_link999).
    link999_off: bool,
    zone_keys: usize,
    /// Area 4ccs by slot - 1, how many are the zone's, and each slot's lighting for this frame (set_area_lights).
    area_keys: Vec<u32>,
    zone_areas: usize,
    area_lights: Vec<Option<crate::dat::weather::Lighting>>,
    areas: wgpu::Buffer,
    /// The sway deltas (globals binding 4), and each portrait's uniforms and globals (render_portrait).
    deltas_gpu: wgpu::Buffer,
    portrait_globals: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// Door parts: slots by door id, their base matrices, their animations (CPU copy of the GPU table) and the tick
    /// they run against.
    pub door_parts: HashMap<u32, Vec<u32>>,
    pub door_bases: HashMap<u32, Vec<glam::Mat4>>,
    parts: Vec<PartGpu>,
    parts_gpu: wgpu::Buffer,
    pub now: f32,
    /// FUN_10178b30's V for the world slot (zone +0x38e1c).
    pub voyage: Mat4,
    /// A Lua mod's scene background colour and ambient light in place of the zone's (gamma-space RGB).
    pub mod_clear: Option<[f32; 3]>,
    pub mod_ambient: Option<[f32; 3]>,
    /// The scenery's draw distance D and Clipping factor K (dat/weather.rs clipping); D 0 draws at any distance.
    pub clip: [f32; 2],
    /// Retail's D whatever the Draw Distance setting: where the camera list's reach ends (set_camera_list).
    pub retail_reach: f32,
}

/// A sub-area's scenery (FUN_101787c0 loads it into zone slot 1): its buffers and draws, its textures in the
/// slots after the zone's; drawn once all of it is on the GPU.
struct SubGeometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    /// Culled as the zone's scenery is (render/cull.rs).
    cull: Option<cull::Cull>,
    draws: Vec<Draw>,
    tex_base: usize,
    link: u32,
    shown: bool,
    /// Ours (Lamp Lights, a sub-area only): each instance's light slots and world sphere, and the lamp slots
    /// written over them now.
    inst_lights: Vec<[u32; 4]>,
    spheres: Vec<[f32; 4]>,
    lamp_patch: HashMap<u32, [u32; 4]>,
}

/// A ship's model (FUN_100c4c20 -> FUN_101844c0, drawn as furniture is): its buffers, one instance that moves with
/// the actor, and its textures (slot 0 white), since the shared list is cut back by sub-areas and furniture; drawn
/// once they and their mips are up and while the actor shows.
struct ModelGeo {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    draws: Vec<Draw>,
    inst: Instance,
    textures: Vec<wgpu::BindGroup>,
    pending: std::collections::VecDeque<(usize, crate::dat::zone::Texture, Option<upload::TexUp>)>,
    mips: u64,
    up: bool,
    shown: bool,
}

/// Anisotropic filtering for scenery, actor and creator samplers (config `anisotropy`, 1 = off).
pub static ANISOTROPY: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(16);

/// A bind group kept until one of the resources it binds changes (they were made afresh every frame).
pub struct BindCache<K>(std::sync::Mutex<Option<(K, wgpu::BindGroup)>>);

impl<K> Default for BindCache<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K> BindCache<K> {
    /// Lets go of the kept group and what it binds (a pass turned off or its targets remade).
    pub fn clear(&self) {
        if let Ok(mut c) = self.0.lock() {
            *c = None;
        }
    }
}

impl<K: PartialEq> BindCache<K> {
    pub fn get(&self, key: K, make: impl FnOnce() -> wgpu::BindGroup) -> wgpu::BindGroup {
        let Ok(mut c) = self.0.lock() else { return make() };
        match &*c {
            Some((k, g)) if *k == key => g.clone(),
            _ => {
                let g = make();
                *c = Some((key, g.clone()));
                g
            }
        }
    }
}

/// Config-tool 0000 (MIP mapping: D3DTSS_MIPFILTER LINEAR, else NONE = level 0 only) and 0018 = 2 (textures
/// uploaded uncompressed); read when samplers and textures are made, so at the next zone load.
pub static MIPMAPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static UNCOMPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the device samples BC textures.
static BC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Config-tool 0018 = 2; with the device's BC support, whether loaders decode DXT to RGBA up front.
pub fn set_uncompressed(on: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    UNCOMPRESSED.store(on, Relaxed);
    crate::dat::zone::EAGER_RGBA.store(on || !BC.load(Relaxed) || old_textures(), Relaxed);
}

pub fn uncompressed() -> bool {
    UNCOMPRESSED.load(std::sync::atomic::Ordering::Relaxed)
}
/// Config-tool 0040 = 0 (Graphics Stabilization Off, the default): the shine pass wraps its coordinates (D3DRS_WRAP0 = 7).
pub static SHINE_WRAP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Config-tool 0019 = 0 (2-D Mapping Compressed): the palettized map images go through DXT3 at load (client/map.rs).
pub static MAP_COMPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

/// Bytes given to Queue::write_buffer (Put), read and cleared once a frame for the F12 overlay: what crosses to the
/// GPU every frame, the limit behind a narrow eGPU link.
pub static WRITTEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Queue::write_buffer, counted in WRITTEN.
pub trait Put {
    fn put(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]);
}

impl Put for wgpu::Queue {
    fn put(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        WRITTEN.fetch_add(data.len() as u64, std::sync::atomic::Ordering::Relaxed);
        self.write_buffer(buffer, offset, data);
    }
}

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
        TEXTURE_BYTES.fetch_add(t.rgba().len() as u64, std::sync::atomic::Ordering::Relaxed);
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &desc(pick(F::Rgba8UnormSrgb, F::Rgba8Unorm), 1),
            wgpu::util::TextureDataOrder::LayerMajor,
            t.rgba(),
        );
        return (texture.create_view(&Default::default()), 0);
    }
    let mut up = upload::TexUp::new(gpu, t, srgb);
    let seq = up.step(gpu, t, true).unwrap_or(0);
    (up.texture.create_view(&Default::default()), seq)
}

fn texture_bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> wgpu::BindGroup {
    let view = upload_texture(gpu, &crate::dat::zone::Texture::new(String::new(), 0, w, h, rgba.to_vec()), true);
    view_bind_group(gpu, layout, sampler, &view)
}

fn view_bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

/// Which shadow depth pass draw_actor_depth draws.
#[derive(Clone, Copy)]
enum Depth {
    /// The sun's: all but the lamp-lit (ActorGpu::lamp_lit).
    Sun,
    /// The sun's, the lamp-lit, through the pipeline that leaves them out near a lit lamp.
    SunLamped,
    /// A lamp's: only the lamp casters (ActorGpu::lamp_caster).
    Lamp,
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
    /// render left it for the faded composite (`defer_faded`): render_faded_after draws it.
    faded_pending: std::cell::Cell<bool>,
    /// Retail's no-shadow latch (+0x12c bits 10 / 11): no real shadow either.
    pub shadow_off: bool,
    /// Model colour RGB, 1 = 0x80 (scheduler op 0x29): the material colour of the lit terms.
    pub tint: [f32; 3],
    /// Target flash (client/flash.rs): ambient lerp toward 220 / 255 and fog colour scale.
    pub flash: [f32; 2],
    /// The floor's four point lights (table slot + 1, 0 none): FUN_100c94a0 picks the strongest.
    pub lights: [u8; 4],
    /// The area 4cc it stands in (vtable +0x188, the floor's +0xb0): its environment lights it (0 = the zone's).
    pub area: u32,
    /// Cloth particles written from the CPU (only cloth-shot does: the game's step on the GPU, render/cloth.rs), two
    /// vec4s each in model space: position (w 1 = set) and normal (w 1 = set), in SkinMesh's order; missing ones
    /// draw skinned.
    pub cloth: Vec<[f32; 4]>,
    /// Its cloth's particle state on the GPU (render/cloth.rs), one per actor: a clone starts without.
    cloth_state: cloth::ClothState,
    /// The last mip job of its textures; drawn once the GPU has it.
    mips: u64,
    /// Where pose_actor last placed it (world), for the faded actors' back-to-front order.
    origin: Vec3,
    /// Its box's screen rect this frame (NDC x0, y0, x1, y1; FUN_100c63d0's), so a faded actor's passes cover only
    /// it; none: the whole screen.
    pub screen: Option<[f32; 4]>,
    /// Casts lamp shadows this frame (ours: only our character does; render/lampshadow.rs).
    pub lamp_caster: bool,
    /// A lamp map takes this actor in this frame (render/lampshadow.rs): the sun's map leaves it out while one of
    /// those lamps is lit and near (zone.wgsl vs_skinned_sun_lamped).
    pub lamp_lit: bool,
}

impl ActorGpu {
    /// Drawn into the real shadows' depth maps this frame (its mips in, opaque enough, skinned, no refraction, no
    /// no-shadow latch); `done` the mips finished. The others keep retail's shadow.
    pub fn casts_shadow(&self, done: u64) -> bool {
        self.mips <= done && self.refract.is_none() && self.alpha >= 0.5 && !self.shadow_off && self.skin.is_some()
    }

    /// Its key among the effects' sorted elements: the view depth of its origin, clamped as theirs.
    pub fn sort_key(&self, view: &Mat4) -> f32 {
        (-view.transform_point3(self.origin).z).clamp(0.0, 512.0)
    }
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
    /// A cloth particle's position vec4 in the pose region (0 none; the normal follows); bit 31: a free particle,
    /// placed by the simulation rather than the skin.
    cloth: u32,
}

/// Cloth particles a pose region holds after the bones (two vec4s each).
pub const MAX_CLOTH: usize = 1024;
/// SkinVertex.cloth's free-particle bit.
const CLOTH_FREE: u32 = 1 << 31;

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
    /// Cloth particles numbered (SkinVertex.cloth).
    cloth: usize,
    /// Those particles' sheets for the GPU step (render/cloth.rs).
    sim: Option<std::sync::Arc<cloth::ClothModel>>,
}

impl SkinMesh {
    pub fn build(ch: &crate::dat::character::Character) -> Self {
        let bones = ch.skeleton.bones.len().min(MAX_BONES);
        let slot_of = |b: usize| if b < bones { b as u32 } else { bones as u32 };
        let mut verts: Vec<SkinVertex> = Vec::new();
        let mut groups = Vec::new();
        // Cloth particles in part order, after the bones and the identity slot.
        let mut particles = 0usize;
        for part in &ch.parts {
            let sim = part.sim.as_ref().filter(|s| particles + s.particles() <= MAX_CLOTH);
            let base = particles;
            particles += sim.map_or(0, |s| s.particles());
            let cloth_of = |v: u16| {
                sim.map_or(0, |s| {
                    (POSE_HEAD + POSE_PER_BONE * (bones + 1) + 2 * (base + v as usize)) as u32
                        | if s.pinned(v as usize) { 0 } else { CLOTH_FREE }
                })
            };
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
                            cloth: cloth_of(c.vertex),
                        });
                    }
                }
                groups.push((g.texture.clone(), start..verts.len() as u32, wep));
            }
        }
        if verts.is_empty() {
            verts.push(SkinVertex::zeroed());
        }
        let sim = cloth::ClothModel::build(&ch.parts, &ch.skeleton).map(std::sync::Arc::new);
        SkinMesh { verts, groups, bones, cloth: particles, sim }
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

/// The shine uniform's wrap flag (uv.z): 1 while 0040 is Off.
fn shine_wrap() -> f32 {
    if SHINE_WRAP.load(std::sync::atomic::Ordering::Relaxed) { 1.0 } else { 0.0 }
}

impl ZoneRenderer {
    /// The wireframe pipeline for a cull while a mod has it on.
    fn wire(&self, c: usize) -> Option<&wgpu::RenderPipeline> {
        self.scenery_wire.as_ref().filter(|_| self.wireframe).map(|w| &w[c])
    }

    /// A skinned actor's shine group: the shared uniform slots and its vertices.
    fn shine_group<'a>(&self, gpu: &Gpu, skin: &'a Skinned) -> &'a wgpu::BindGroup {
        skin.shine.get_or_init(|| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("actor shine"),
                layout: &self.shine_wrap_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.shine_buf,
                            offset: 0,
                            size: wgpu::BufferSize::new(32),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: skin.vertices.as_entire_binding() },
                ],
            })
        })
    }
}

#[derive(Clone)]
struct Skinned {
    vertices: wgpu::Buffer,
    /// The shine pass's group 3 over these vertices (read as storage for the wrap), made at the first shine.
    shine: std::sync::Arc<std::sync::OnceLock<wgpu::BindGroup>>,
    bones: usize,
    /// Cloth particles after the bones, and their sheets.
    cloth: usize,
    sim: Option<std::sync::Arc<cloth::ClothModel>>,
    /// Dynamic offset of this frame's pose region, and the pose frame it was written in.
    offset: u32,
    frame: u64,
}

/// Bones a pose region holds; influences on later bones read the identity slot after them.
const MAX_BONES: usize = 255;
/// Pose region in vec4s: model matrix columns, (alpha, flash ambient, flash fog, 0), then (quat, translation, scale)
/// per bone, the identity slot, then two per cloth particle (position, normal).
const POSE_HEAD: usize = 6;
pub const POSE_PER_BONE: usize = 3;
const POSE_BINDING: u64 = ((POSE_HEAD + POSE_PER_BONE * (MAX_BONES + 1) + 2 * MAX_CLOTH) * 16) as u64;

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
    scenery_furn: [[wgpu::RenderPipeline; 3]; 2],
    scenery_bump: [wgpu::RenderPipeline; 3],
    scenery_wire: Option<[wgpu::RenderPipeline; 3]>,
    actor_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    skin_pipeline: wgpu::RenderPipeline,
    shine_layout: wgpu::BindGroupLayout,
    shine_wrap_layout: wgpu::BindGroupLayout,
    shine_pipeline: wgpu::RenderPipeline,
    refract_pipeline: wgpu::RenderPipeline,
    /// A faded actor drawn opaque into the offscreen target, and that target blended over the scene (render_faded).
    skin_solid_pipeline: wgpu::RenderPipeline,
    faded_pipeline: wgpu::RenderPipeline,
    /// Zeroes the offscreen target inside a faded actor's scissor rect before it is drawn there.
    faded_clear_pipeline: wgpu::RenderPipeline,
    /// The colour target format the pipelines draw to.
    color_format: wgpu::TextureFormat,
    line_pipeline: wgpu::RenderPipeline,
    world_pipelines: [wgpu::RenderPipeline; 2],
    /// The shadow silhouette pass (render/shadow.rs): its group 3 layout and pipeline.
    sil_layout: wgpu::BindGroupLayout,
    sil_pipeline: wgpu::RenderPipeline,
    /// Real character shadows (render/sunshadow.rs, render/lampshadow.rs): the actors' depth from the sun (with
    /// their feet heights) and from a lamp.
    sun_depth_pipeline: wgpu::RenderPipeline,
    sun_depth_lamped_pipeline: wgpu::RenderPipeline,
    lamp_depth_pipeline: wgpu::RenderPipeline,
    /// The scenery's frustum culling (render/cull.rs): its group 3 layout, the visibility and compaction passes.
    cull_layout: wgpu::BindGroupLayout,
    cull: [wgpu::ComputePipeline; 2],
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
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX.union(wgpu::ShaderStages::COMPUTE),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Lamp shadows (render/lampshadow.rs): their uniform (the sun's depth pass reads which lamps take over).
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
        let make_pipeline = |fragment: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("poses"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(POSE_BINDING),
                },
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
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Uint32x2, 5 => Float32x2, 6 => Float32x2, 7 => Float32x2, 8 => Uint32],
        })];
        let skin_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned actors"),
            layout: Some(&skin_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_skinned"),
                compilation_options: Default::default(),
                buffers: &skin_buffers,
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
                entry_point: Some("fs_actor"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Scenery: front faces clockwise as D3D's; normal placements cull CCW (back), mirrored ones CW (front).
        // Bit-31 models: SRCALPHA / INVSRCALPHA, depth tested (LESSEQUAL) but not written, ZBIAS 8 *(inf: as a constant
        // depth bias of -8)*.
        // `furn`: furniture's pass (FUN_101844c0) sets neither ZWRITEENABLE nor ZBIAS, so its blended meshes keep
        // the actor pass's depth writes and bias 0 (batch 7 item 23).
        let make_scenery_z = |fragment: &str, blend: bool, cull: Option<wgpu::Face>, wire: bool, furn: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &vertex_layouts,
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: cull,
                    polygon_mode: if wire { wgpu::PolygonMode::Line } else { wgpu::PolygonMode::Fill },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!blend || furn),
                    depth_compare: Some(if blend && !furn {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Less
                    }),
                    stencil: Default::default(),
                    bias: if blend && !furn {
                        wgpu::DepthBiasState { constant: -8, slope_scale: 0.0, clamp: 0.0 }
                    } else {
                        Default::default()
                    },
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let culls = [None, Some(wgpu::Face::Back), Some(wgpu::Face::Front)];
        let make_scenery = |f: &str, b: bool, c: Option<wgpu::Face>, w: bool| make_scenery_z(f, b, c, w, false);
        let scenery = [("fs_opaque", false), ("fs_alpha_test", false), ("fs_blend", true), ("fs_blend_test", true)]
            .map(|(f, b)| culls.map(|c| make_scenery(f, b, c, false)));
        // The same two blended classes for furniture, with depth writes on.
        let scenery_furn = [("fs_blend", true), ("fs_blend_test", true)]
            .map(|(f, b)| culls.map(|c| make_scenery_z(f, b, c, false, true)));
        let scenery_wire = device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_LINE)
            .then(|| culls.map(|c| make_scenery("fs_opaque", false, c, true)));
        // Config 0017's type-3 meshes (FUN_10175ac0 case 3): the colour texture, then the normal map at group 2.
        let bump_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zone bumped"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let scenery_bump = culls.map(|cull| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fs_bump"),
                layout: Some(&bump_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &vertex_layouts,
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: cull,
                    ..Default::default()
                },
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
                    entry_point: Some("fs_bump"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        // Actors blend by their model alpha (fades); opaque ones output alpha 1.
        let actor_pipeline = make_pipeline("fs_actor", Some(wgpu::BlendState::ALPHA_BLENDING));
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &[Some(color_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The shadow silhouette (FUN_100329e0): flat white into a shadow layer, no depth, no culling.
        let sil_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow silhouette"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let sil_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow silhouette"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow silhouette"),
                bind_group_layouts: &[
                    Some(&globals_layout),
                    Some(&texture_layout),
                    Some(&pose_layout),
                    Some(&sil_layout),
                ],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_skinned_silhouette"),
                compilation_options: Default::default(),
                buffers: &skin_buffers,
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_silhouette"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shadow::SIL_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Real character shadows (ours): the actors' depth from the sun or a lamp through the light matrix at group 3,
        // both sides, textures ignored as the silhouette's. The sun's also writes each caster's feet height
        // (sunshadow::FEET_FORMAT), which its shadow fades against.
        let shadow_depth = |label: &str, feet: bool, entry: &str| {
            let targets = [Some(wgpu::ColorTargetState {
                format: sunshadow::FEET_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: &[
                        Some(&globals_layout),
                        Some(&texture_layout),
                        Some(&pose_layout),
                        Some(&sil_layout),
                    ],
                    immediate_size: 0,
                })),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: &skin_buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
                }),
                multisample: Default::default(),
                fragment: feet.then(|| wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_sun_feet"),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sun_depth_pipeline = shadow_depth("sun shadow depth", true, "vs_skinned_sun");
        let sun_depth_lamped_pipeline = shadow_depth("sun shadow depth (lamp lit)", true, "vs_skinned_sun_lamped");
        let lamp_depth_pipeline = shadow_depth("lamp shadow depth", false, "vs_skinned_silhouette");
        // The shine overlay (FUN_10024e10's second pass): additive (SRCALPHA / ONE), z tested (as drawn) but not written.
        let shine_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shine"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            }],
        });
        let shine_wrap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shine with vertices"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shine_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actor shine"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("actor shine"),
                bind_group_layouts: &[
                    Some(&globals_layout),
                    Some(&texture_layout),
                    Some(&pose_layout),
                    Some(&shine_wrap_layout),
                ],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_skinned_shine"),
                compilation_options: Default::default(),
                buffers: &skin_buffers,
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
                module: &shader,
                entry_point: Some("fs_actor_shine"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
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
                bind_group_layouts: &[
                    Some(&globals_layout),
                    Some(&texture_layout),
                    Some(&pose_layout),
                    Some(&shine_layout),
                ],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_skinned_refract"),
                compilation_options: Default::default(),
                buffers: &skin_buffers,
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
                module: &shader,
                entry_point: Some("fs_actor_refract"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // A faded actor's offscreen pass (FUN_1002b6e0): ZWRITE on, ALPHABLEND off, alpha test only.
        let skin_solid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("faded actor offscreen"),
            layout: Some(&skin_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_skinned"),
                compilation_options: Default::default(),
                buffers: &skin_buffers,
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
                entry_point: Some("fs_actor_solid"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // FUN_1001d3b0: the offscreen target as one viewport quad, SRCALPHA / INVSRCALPHA, ZENABLE and ZWRITE off.
        let faded_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("faded actor composite"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("faded actor composite"),
                bind_group_layouts: &[
                    Some(&globals_layout),
                    Some(&texture_layout),
                    Some(&pose_layout),
                    Some(&shine_layout),
                ],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_faded"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_faded"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let faded_clear_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("faded actor clear"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("faded actor clear"),
                bind_group_layouts: &[],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_faded"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_faded_clear"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Layout mode's overlay: line lists over everything (ZENABLE off), alpha blended.
        let lines = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lines"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lines.wgsl").into()),
        });
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lines"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("lines"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &lines,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &lines,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The Lua mods' world shapes: the same shader, tested against the scene's depth (not written), alpha blended.
        let world_pipelines =
            [wgpu::PrimitiveTopology::LineList, wgpu::PrimitiveTopology::TriangleList].map(|topology| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("world shapes"),
                    layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("world shapes"),
                        bind_group_layouts: &[Some(&globals_layout)],
                        immediate_size: 0,
                    })),
                    vertex: wgpu::VertexState {
                        module: &lines,
                        entry_point: Some("vs_line"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<LineVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                        })],
                    },
                    primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &lines,
                        entry_point: Some("fs_line"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: color_format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            });
        // Culling binds the globals (camera, voyage, door parts) at group 0 and its buffers at group 3.
        let cull_layout = cull::layout(device);
        let cull_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cull"),
            bind_group_layouts: &[Some(&globals_layout), None, None, Some(&cull_layout)],
            immediate_size: 0,
        });
        let make_cull = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&cull_pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let cull = [make_cull("cs_cull_vis"), make_cull("cs_cull_compact")];
        Self {
            globals_layout,
            texture_layout,
            pose_layout,
            scenery,
            scenery_furn,
            scenery_bump,
            scenery_wire,
            actor_pipeline,
            sky_pipeline,
            skin_pipeline,
            shine_layout,
            shine_wrap_layout,
            shine_pipeline,
            refract_pipeline,
            skin_solid_pipeline,
            faded_pipeline,
            faded_clear_pipeline,
            sun_depth_pipeline,
            sun_depth_lamped_pipeline,
            lamp_depth_pipeline,
            color_format,
            line_pipeline,
            world_pipelines,
            sil_layout,
            sil_pipeline,
            cull_layout,
            cull,
        }
    }
}

/// Point-light table entries (DAT_105ecae8: 256).
pub const LIGHTS: usize = 256;
/// Ours: how far past an instance's bounding sphere a lamp's light is offered to it (set_lamps; the light's range
/// cuts it in the shader).
const LAMP_REACH: f32 = 16.0;

/// A scenery instance: the placement matrix and its four point lights (table slot + 1, 0 = none).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    model: [[f32; 4]; 4],
    lights: [u32; 4],
    /// x: the area slot it is lit by (0 = the zone's); y: the sway step (sway.rs); z: 1 with NORMALIZENORMALS.
    area: [u32; 4],
    /// Level of detail: drawn while near² < d² <= far² from the camera to its origin; far 0 = always. z: the part,
    /// w: 1 for a voyage zone's world (drawn through the voyage matrix).
    band: [f32; 4],
}

/// FUN_101844c0: furniture is lit as scenery with one point light, the zone light keyed 'l_01' (slot +0x394b0; every
/// Mog House scene lists only it); none when the zone has no such light.
fn furniture_lights(keys: &[u32]) -> [u32; 4] {
    [keys.iter().position(|&k| k == u32::from_le_bytes(*b"l_01")).map_or(0, |s| s as u32 + 1), 0, 0, 0]
}

/// The slot leak's replay data (render/leak.rs) for a geometry whose scenery has a 'c' ref, else None.
fn leak_data(
    scene: &ZoneScene,
    refs: &HashMap<u32, [leak::Ref; 4]>,
    inst_placement: &[u32],
    matrices: &[Instance],
    bounds: &[[f32; 8]],
) -> Option<leak::LeakData> {
    let mut leakers: Vec<u32> = refs.iter().filter(|(_, r)| leak::LeakData::has_keep(r)).map(|(p, _)| *p).collect();
    if leakers.is_empty() {
        return None;
    }
    leakers.sort_unstable();
    let mut places: Vec<([leak::Ref; 4], bool, Vec<u32>)> = scene
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| (refs.get(&(i as u32)).copied().unwrap_or([leak::Ref::Off; 4]), p.deferred, Vec::new()))
        .collect();
    for (i, &p) in inst_placement.iter().enumerate() {
        if refs.contains_key(&p) {
            places[p as usize].2.push(i as u32);
        }
    }
    let insts = matrices.iter().copied().zip(bounds.iter().copied()).collect();
    Some(leak::LeakData { places, insts, leakers })
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
    /// Each instance's mesh box and the draws' runs, for culling (render/cull.rs).
    cull: cull::CullData,
    /// Each instance's placement (its MZB record; u32::MAX for door parts), and the camera lists by MZB offset
    /// (zone::camera_lists).
    inst_placement: Vec<u32>,
    camera_lists: HashMap<u32, Vec<u32>>,
    /// The point-light slot leak's replay data (render/leak.rs), when a scenery placement has a 'c' ref.
    leak: Option<leak::LeakData>,
    variants: Vec<Option<(u32, bool)>>,
    /// Each instance's world bounding sphere (centre, radius), for the lamps' lights (set_lamps).
    spheres: Vec<[f32; 4]>,
    /// Config 0011: the animated meshes' vertex deltas (GpuVertex flags >> 8, minus 1), their instances and the
    /// zone's curves (render/sway.rs). None for a sub-area, which shares the zone's globals.
    deltas: Vec<[f32; 4]>,
    sway_insts: Vec<u32>,
    sway_curves: crate::dat::zone::SwayCurves,
    /// Config 0017: the normal maps the bumped draws use.
    bumps: Vec<(u32, u32, Vec<u8>)>,
}

impl ZoneGeometry {
    pub fn build(scene: &ZoneScene) -> Self {
        Self::build_with(scene, &scene.light_keys, &[], &[], true)
    }

    /// With the point-light table `keys` (a sub-area's light refs resolve into the zone's table, FUN_1017a5e0).
    /// `areas`: area keys already in slots (the zone's, for a sub-area); new ones follow. `zone_textures`: the zone's
    /// texture names in slot order, which a sub-area's meshes also name (textures are found by name across the loaded
    /// files; Ru'Aun's sub-areas take their stone from the zone's file).
    pub fn build_keyed(scene: &ZoneScene, keys: &[u32], areas: &[u32], zone_textures: &[String]) -> Self {
        Self::build_with(scene, keys, areas, zone_textures, false)
    }

    /// `animate`: the animated meshes keep their deltas (not for a sub-area).
    fn build_with(scene: &ZoneScene, keys: &[u32], areas: &[u32], zone_textures: &[String], animate: bool) -> Self {
        // Slot 0 is the plain white texture; scene textures follow in order; a name only the zone has is
        // ZONE_TEXTURE + its zone slot (ZoneRenderer::set_sub).
        let mut texture_slot: HashMap<&str, usize> =
            zone_textures.iter().enumerate().map(|(i, t)| (t.as_str(), ZONE_TEXTURE + i + 1)).collect();
        texture_slot.extend(scene.textures.iter().enumerate().map(|(i, t)| (t.name.as_str(), i + 1)));
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
        // Per instance: the placement's link, whether mirrored, its distance rule for the culling (cull.rs) and its
        // record.
        let mut by_mesh: HashMap<String, Vec<(Instance, u32, bool, [f32; 2], u32)>> = HashMap::new();
        // The scenery placements' refs as the slot leak replays them.
        let mut place_refs: HashMap<u32, [leak::Ref; 4]> = HashMap::new();
        for (pi, p) in scene.placements.iter().enumerate().filter(|(_, p)| p.drawn()) {
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
                let mut v: Vec<(std::borrow::Cow<str>, [f32; 2])> =
                    vec![(h, [-1.0, near]), (m, [near, far]), (l, [far, f32::MAX])];
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
            if part == 0 {
                place_refs.insert(
                    pi as u32,
                    p.lights.map(|k| match slot_of(k, false) {
                        0 if k != 0 && k as u8 == b'c' => leak::Ref::Keep,
                        0 => leak::Ref::Off,
                        s => leak::Ref::Set(s),
                    }),
                );
            }
            let reach = cull::reach(p.own_dist, p.id != 0);
            // Door groups draw whatever the camera list (FUN_1017ade0): no record for it.
            let rec = if part != 0 { u32::MAX } else { pi as u32 };
            for (name, b) in bands {
                let inst = Instance {
                    model: p.matrix(),
                    lights,
                    area: [area, 0, p.normalize as u32, 0],
                    band: [b[0], b[1], part as f32, p.world as u32 as f32],
                };
                by_mesh.entry(name.into_owned()).or_default().push((
                    inst,
                    p.link,
                    p.scale[0] * p.scale[2] < 0.0,
                    reach,
                    rec,
                ));
            }
        }
        let mut matrices: Vec<Instance> = Vec::new();
        let mut inst_placement: Vec<u32> = Vec::new();
        let mut bounds: Vec<[f32; 8]> = Vec::new();
        let mut spheres: Vec<[f32; 4]> = Vec::new();
        let mut links = Vec::new();
        let mut draws = Vec::new();
        let mut all_verts: Vec<GpuVertex> = Vec::new();
        let mut all_indices: Vec<u16> = Vec::new();
        let (mut deltas, mut sway_insts): (Vec<[f32; 4]>, Vec<u32>) = (Vec::new(), Vec::new());
        // FUN_10039f80: a texture's bump map has the same name bytes 8..15.
        let trim = |mut t: Vec<u8>| -> Vec<u8> {
            while t.last().is_some_and(|c| *c == b' ' || *c == 0) {
                t.pop();
            }
            t
        };
        let bump_keys: Vec<Vec<u8>> = scene.bumps.iter().map(|b| trim(b.key.to_vec())).collect();
        let bump_of = |texture: &str| -> Option<usize> {
            let t = trim(texture.as_bytes().iter().skip(8).take(8).copied().collect());
            (!t.is_empty()).then(|| bump_keys.iter().position(|k| *k == t)).flatten()
        };
        let mut bumps: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        let mut bump_used: HashMap<usize, usize> = HashMap::new();
        for mesh in &scene.meshes {
            let Some(mut mats) = by_mesh.remove(mesh.name.as_str()) else { continue };
            // Normal placements first, then the mirrored ones (culled the other way).
            mats.sort_by_key(|m| m.2);
            let start = matrices.len() as u32;
            let split = start + mats.iter().filter(|m| !m.2).count() as u32;
            let mut reaches = Vec::with_capacity(mats.len());
            for (inst, link, _, reach, pi) in mats {
                if link != 0 {
                    links.push((matrices.len() as u32, link, inst));
                }
                matrices.push(inst);
                inst_placement.push(pi);
                reaches.push(reach);
            }
            let (instances, mirrored) = (start..split, split..matrices.len() as u32);
            // The mesh's box over every model drawn, for each of its placements.
            // Swaying meshes reach as far as pos + delta.
            let (lo, hi) = mesh.models.iter().filter(|m| !m.indices.is_empty()).flat_map(|m| m.vertices.iter()).fold(
                (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                |(lo, hi), v| {
                    let (p, q) = (Vec3::from(v.pos), Vec3::from(v.pos) + Vec3::from(v.delta));
                    (lo.min(p).min(q), hi.max(p).max(q))
                },
            );
            let sways =
                animate && mesh.animated && mesh.models.iter().any(|m| m.vertices.iter().any(|v| v.delta != [0.0; 3]));
            // FUN_10168eb0: a strip mesh (type 1) whose every model texture has a bump map becomes type 3.
            let bumped: Option<Vec<usize>> = (animate && mesh.mesh_type == 1 && !scene.bumps.is_empty())
                .then(|| mesh.models.iter().map(|m| bump_of(&m.texture)).collect::<Option<Vec<usize>>>())
                .flatten();
            if sways {
                sway_insts.extend(start..matrices.len() as u32);
            }
            let (c, h) = if lo.x <= hi.x { ((lo + hi) / 2.0, (hi - lo) / 2.0) } else { (Vec3::ZERO, Vec3::ZERO) };
            bounds.extend(reaches.iter().map(|r| [c.x, c.y, c.z, r[0], h.x, h.y, h.z, r[1]]));
            for inst in &matrices[start as usize..] {
                let m = Mat4::from_cols_array_2d(&inst.model);
                let k =
                    m.x_axis.truncate().length().max(m.y_axis.truncate().length()).max(m.z_axis.truncate().length());
                let at = m.transform_point3(c);
                spheres.push([at.x, at.y, at.z, h.length() * k]);
            }
            for (mi, model) in mesh.models.iter().enumerate() {
                let indices = if model.strip { strip_to_list(&model.indices) } else { model.indices.clone() };
                if indices.is_empty() || model.vertices.is_empty() {
                    continue;
                }
                // Type 3: every triangle unrolled to its corners, each with its tangent (FUN_10169d10).
                if let Some(slot) = bumped
                    .as_ref()
                    .map(|b| b[mi])
                    .filter(|_| indices.len() <= 0xffff && deltas.len() + indices.len() < 0xff_ffff)
                {
                    let bi = *bump_used.entry(slot).or_insert_with(|| {
                        let b = &scene.bumps[slot];
                        bumps.push((b.width, b.height, b.rgba.clone()));
                        bumps.len() - 1
                    });
                    let (first_index, base_vertex) = (all_indices.len() as u32, all_verts.len() as i32);
                    let count = (indices.len() / 3 * 3) as u32;
                    for tri in indices[..count as usize].as_chunks::<3>().0 {
                        let corner = |k: usize| model.vertices[tri[k] as usize];
                        for k in 0..3 {
                            let (a, b, c) = (corner(k), corner((k + 1) % 3), corner((k + 2) % 3));
                            let t = corner_tangent(&a, &b, &c);
                            deltas.push([t.x, t.y, t.z, 0.0]);
                            let [bb, g, r, al] = a.color.to_le_bytes();
                            let flags = (model.blending & 0x1000 != 0) as u32 | 2 | (deltas.len() as u32) << 8;
                            all_indices.push((all_verts.len() as i32 - base_vertex) as u16);
                            all_verts.push(GpuVertex {
                                pos: a.pos,
                                normal: a.normal,
                                color: [r, g, bb, al],
                                uv: a.uv,
                                flags,
                            });
                        }
                    }
                    draws.push(Draw {
                        first_index,
                        index_count: count,
                        base_vertex,
                        instances: instances.clone(),
                        mirrored: mirrored.clone(),
                        cull: model.blending & 0x2000 == 0,
                        texture: texture_slot.get(model.texture.as_str()).copied().unwrap_or(0),
                        mode: mesh.name.starts_with('_') as u8 | ((model.blending & 0x8000 != 0) as u8) << 1,
                        args: [u32::MAX; 2],
                        bump: Some(bi),
                    });
                    continue;
                }
                let verts: Vec<GpuVertex> = model
                    .vertices
                    .iter()
                    .map(|v| {
                        let [b, g, r, a] = v.color.to_le_bytes();
                        let mut flags = (model.blending & 0x1000 != 0) as u32;
                        if sways && v.delta != [0.0; 3] && deltas.len() < 0xff_ffff {
                            deltas.push([v.delta[0], v.delta[1], v.delta[2], 0.0]);
                            flags |= (deltas.len() as u32) << 8;
                        }
                        GpuVertex { pos: v.pos, normal: v.normal, color: [r, g, b, a], uv: v.uv, flags }
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
                    args: [u32::MAX; 2],
                    bump: None,
                });
            }
        }
        let subs: Vec<Vec<(std::ops::Range<u32>, u32, u32, i32)>> = draws
            .iter()
            .map(|d: &Draw| {
                d.runs().into_iter().map(|(_, r)| (r, d.index_count, d.first_index, d.base_vertex)).collect()
            })
            .collect();
        let cull_bounds = if place_refs.values().any(leak::LeakData::has_keep) { bounds.clone() } else { Vec::new() };
        let (cull, slots) = cull::CullData::build(bounds, &matrices, &subs);
        let leak = leak_data(scene, &place_refs, &inst_placement, &matrices, &cull_bounds);
        for (d, s) in draws.iter_mut().zip(slots) {
            d.args = [s[0], s[1]];
        }
        // A scene with no scenery (the character screen) still needs a non-empty buffer to bind.
        if matrices.is_empty() {
            spheres.push([0.0; 4]);
            matrices.push(Instance {
                model: Mat4::IDENTITY.to_cols_array_2d(),
                lights: [0; 4],
                area: [0; 4],
                band: [0.0; 4],
            });
        }
        if all_verts.is_empty() {
            all_verts.push(GpuVertex::zeroed());
        }
        while !all_indices.len().is_multiple_of(2) || all_indices.is_empty() {
            all_indices.push(0);
        }
        let sway_curves = if sway_insts.is_empty() { Default::default() } else { scene.sway.clone() };
        Self {
            matrices,
            links,
            light_keys,
            area_keys,
            door_parts,
            door_bases,
            draws,
            verts: all_verts,
            indices: all_indices,
            cull,
            inst_placement,
            camera_lists: scene.camera_lists.clone(),
            leak,
            variants: if scene.camera_lists.is_empty() { Vec::new() } else { scene.variant_groups() },
            spheres,
            deltas,
            sway_insts,
            sway_curves,
            bumps,
        }
    }

    fn parts(&self) -> [&[u8]; 3] {
        [bytemuck::cast_slice(&self.verts), bytemuck::cast_slice(&self.indices), bytemuck::cast_slice(&self.matrices)]
    }
}

/// A leaker's resolved slots (render/leak.rs) with the lamps its Lamp Lights patch added (slots that were free in
/// `own`), in their order: each where it was if that is still free, else in the first free slot; none past four.
fn with_lamps(resolved: [u32; 4], own: [u32; 4], patch: Option<[u32; 4]>) -> [u32; 4] {
    let mut v = resolved;
    let Some(patch) = patch else { return v };
    for k in (0..4).filter(|&k| own[k] == 0 && patch[k] != 0) {
        if v.contains(&patch[k]) {
            continue;
        }
        if let Some(f) = std::iter::once(k).chain(0..4).find(|&f| v[f] == 0) {
            v[f] = patch[k];
        }
    }
    v
}

/// Each instance's free light places filled with the nearest lamps within LAMP_REACH of its sphere (instance -> slots,
/// only the instances that take any).
fn lamp_slots(own: &[[u32; 4]], spheres: &[[f32; 4]], lamps: &[(Vec3, u32)]) -> HashMap<u32, [u32; 4]> {
    let mut next = HashMap::new();
    for (i, (own, s)) in own.iter().zip(spheres).enumerate() {
        let c = Vec3::new(s[0], s[1], s[2]);
        let mut near: Vec<(f32, u32)> = lamps
            .iter()
            .map(|(p, slot)| (p.distance(c) - s[3], *slot))
            .filter(|(d, slot)| *d < LAMP_REACH && !own.contains(&(slot + 1)))
            .collect();
        if near.is_empty() {
            continue;
        }
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut l = *own;
        let mut more = near.into_iter();
        for k in l.iter_mut().filter(|k| **k == 0) {
            if let Some((_, slot)) = more.next() {
                *k = slot + 1;
            }
        }
        if l != *own {
            next.insert(i as u32, l);
        }
    }
    next
}

/// FUN_10169d10: corner a's tangent from the triangle's corners in their drawn order (a, b, c): d = (Pc - Pb)(ua - ub)
/// - (Pa - Pb)(uc - ub) normalised ((0, 0, 1) when zero), tangent = d x Na.
fn corner_tangent(a: &crate::dat::zone::Vertex, b: &crate::dat::zone::Vertex, c: &crate::dat::zone::Vertex) -> Vec3 {
    let (pa, pb, pc) = (Vec3::from(a.pos), Vec3::from(b.pos), Vec3::from(c.pos));
    let d = (pc - pb) * (a.uv[0] - b.uv[0]) - (pa - pb) * (c.uv[0] - b.uv[0]);
    let d = if d.length_squared() != 0.0 { d.normalize() } else { Vec3::Z };
    d.cross(Vec3::from(a.normal))
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
            gpu.queue.put(buf, 0, bytes);
        }
        for (i, t) in scene.textures.iter().enumerate() {
            r.put_texture(gpu, i + 1, t);
        }
        r.log_textures();
        r
    }

    /// The world's zone: its geometry and textures go up over frames under the copy budget (upload_step) and
    /// its mips under the mip budget; ready() once all of it is on the GPU.
    pub fn streamed(
        gpu: &Gpu,
        geo: std::sync::Arc<ZoneGeometry>,
        textures: Vec<crate::dat::zone::Texture>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let mut r = Self::empty(gpu, &geo, textures.len(), color_format, true);
        r.pending = textures.into_iter().enumerate().map(|(i, t)| (i + 1, t, None)).collect();
        r.geo_pending = Some((geo, 0));
        r
    }

    fn empty(
        gpu: &Gpu,
        geo: &ZoneGeometry,
        textures: usize,
        color_format: wgpu::TextureFormat,
        budgeted: bool,
    ) -> Self {
        let device = &gpu.device;
        let p = gpu.pipes.zone(color_format);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Two vec4s a light: (position, range), (colour x intensity, 0); written by the effects update pass.
        let lights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point lights"),
            size: (LIGHTS * 32) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let areas = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("area lighting"),
            size: (AREAS * std::mem::size_of::<AreaGpu>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let parts_gpu = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("door parts"),
            size: (PARTS * std::mem::size_of::<PartGpu>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let deltas = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sway deltas"),
            size: (geo.deltas.len().max(1) * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !geo.deltas.is_empty() {
            gpu.queue.put(&deltas, 0, bytemuck::cast_slice(&geo.deltas));
        }
        let lamp_shadow = lampshadow::LampShadows::new(gpu, &p.sil_layout, color_format);
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &p.globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: lights.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: areas.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: parts_gpu.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: deltas.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: lamp_shadow.uniform.as_entire_binding() },
            ],
        });
        let sampler = mip_sampler(device);
        // Slot 0 is plain white for models whose texture lives elsewhere; the others fill in as they go up.
        let white = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 255, 255, 255]);
        // Slot 0 of actors: magenta, so a missing texture is obvious.
        let magenta = texture_bind_group(gpu, &p.texture_layout, &sampler, 1, 1, &[255, 0, 255, 255]);
        let shine_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("actor shine"),
            size: SHINE_SLOTS as u64 * SHINE_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let refract_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("actor refraction"),
            size: SHINE_SLOTS as u64 * SHINE_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let refract_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actor refraction"),
            layout: &p.shine_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &refract_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            }],
        });
        let faded_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("faded actors"),
            size: SHINE_SLOTS as u64 * SHINE_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let faded_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("faded actors"),
            layout: &p.shine_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &faded_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            }],
        });
        let [v, i, m] = geo.parts().map(|b| b.len() as u64);
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        // The culling reads the instances as storage.
        let instances = buffer("instances", m, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE);
        let cull = cull::Cull::new(gpu, &p.cull_layout, &geo.cull, &instances);
        let sway = sway::Sway::new(gpu, &geo.sway_insts, &geo.sway_curves, &instances);
        // Config 0017: the normal maps, uploaded at once (few and small).
        if !geo.bumps.is_empty() {
            log::info!(
                "bump mapping: {} normal maps on {} draws",
                geo.bumps.len(),
                geo.draws.iter().filter(|d| d.bump.is_some()).count()
            );
        }
        let bump_groups: Vec<wgpu::BindGroup> = geo
            .bumps
            .iter()
            .map(|(w, h, rgba)| {
                let view =
                    upload_texture(gpu, &crate::dat::zone::Texture::new(String::new(), 0, *w, *h, rgba.clone()), false);
                view_bind_group(gpu, &p.texture_layout, &sampler, &view)
            })
            .collect();
        if sway.is_some() {
            log::info!(
                "environment animation: {} instances, {} vertex deltas, curves per tag {:?}",
                geo.sway_insts.len(),
                geo.deltas.len(),
                geo.sway_curves.by_tag.iter().map(Vec::len).collect::<Vec<_>>()
            );
        }
        Self {
            sway,
            bump_groups,
            bump_mode: true,
            smooth_lights: false,
            light_boost: [0.0; 4],
            scenery_bump: p.scenery_bump,
            scenery: p.scenery,
            scenery_furn: p.scenery_furn,
            scenery_wire: p.scenery_wire,
            wireframe: false,
            actor_pipeline: p.actor_pipeline,
            sky_pipeline: p.sky_pipeline,
            line_pipeline: p.line_pipeline,
            line_buf: None,
            lines: Vec::new(),
            world_pipelines: p.world_pipelines,
            world_bufs: [None, None],
            world_shapes: [Vec::new(), Vec::new()],
            texture_layout: p.texture_layout,
            sampler,
            uniforms,
            globals,
            textures: vec![white; textures + 1],
            magenta,
            instances,
            cull,
            cull_pipes: p.cull,
            cull_layout: p.cull_layout,
            inst_lights: geo.matrices.iter().map(|m| m.lights).collect(),
            spheres: geo.spheres.clone(),
            lamp_patch: HashMap::new(),
            lamps_wanted: None,
            lamps_last: Vec::new(),
            inst_placement: geo.inst_placement.clone(),
            camera_lists: geo.camera_lists.clone(),
            leak: geo.leak.clone().map(leak::LeakState::new),
            camera_list: (0, None),
            variants: geo.variants.clone(),
            lights,
            light_keys: geo.light_keys.clone(),
            vertices: buffer("scenery vertices", v, wgpu::BufferUsages::VERTEX),
            indices: buffer("scenery indices", i, wgpu::BufferUsages::INDEX),
            draws: geo.draws.clone(),
            skin_pipeline: p.skin_pipeline,
            shine_pipeline: p.shine_pipeline,
            shine_buf,
            shine_wrap_layout: p.shine_wrap_layout.clone(),
            refract_pipeline: p.refract_pipeline,
            globals_layout: p.globals_layout.clone(),
            sil_layout: p.sil_layout,
            sil_pipeline: p.sil_pipeline,
            shadows: None,
            sun_depth_pipeline: p.sun_depth_pipeline,
            sun_depth_lamped_pipeline: p.sun_depth_lamped_pipeline,
            lamp_depth_pipeline: p.lamp_depth_pipeline,
            sun_shadow: None,
            ssao: None,
            lamp_shadow,
            lamp_shadows: Vec::new(),
            shadow_level: 1.0,
            shadow_soft: 1.0,
            refract_buf,
            refract_group,
            refract_copy: None,
            skin_solid_pipeline: p.skin_solid_pipeline,
            faded_pipeline: p.faded_pipeline,
            faded_clear_pipeline: p.faded_clear_pipeline,
            faded_buf,
            faded_group,
            faded_rt: None,
            color_format: p.color_format,
            shine_tex: HashMap::new(),
            pose_layout: p.pose_layout,
            poses: Vec::new(),
            pose_gpu: None,
            pose_frame: 0,
            anim: anim::GpuAnim::new(gpu),
            cloth: cloth::ClothGpu::new(gpu),
            models: HashMap::new(),
            builds: Vec::new(),
            budgeted,
            defer_faded: false,
            faded_slot: Default::default(),
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
            furn_inst: Vec::new(),
            model_actors: HashMap::new(),
            links: geo.links.clone(),
            hidden: 0,
            link999_off: false,
            zone_keys: geo.light_keys.len(),
            area_keys: geo.area_keys.clone(),
            zone_areas: geo.area_keys.len(),
            area_lights: Vec::new(),
            areas,
            deltas_gpu: deltas,
            portrait_globals: Vec::new(),
            door_parts: geo.door_parts.clone(),
            door_bases: geo.door_bases.clone(),
            parts: vec![PartGpu::default(); PARTS],
            parts_gpu,
            now: 0.0,
            voyage: Mat4::IDENTITY,
            mod_clear: None,
            mod_ambient: None,
            clip: [0.0, 1.0],
            retail_reach: 0.0,
        }
    }

    /// Scheduler op 0x0d (rotate) / 0x0c (translate) on door part `slot`: from its value now to `to` over `ticks`
    /// (FUN_100439c0 / FUN_10043bb0; angles wrapped to [-3.1415, 3.1415] as FUN_100acdb0 stores them).
    #[allow(clippy::approx_constant, reason = "FUN_100acdb0's truncated pi values, kept as written")]
    pub fn move_part(&mut self, gpu: &Gpu, slot: u32, rotate: bool, to: [f32; 3], ticks: f32) {
        let Some(p) = self.parts.get_mut(slot as usize) else { return };
        let now = self.now;
        let wrap = |a: f32| if (-3.1415..=3.1415).contains(&a) { a } else { a - (a / 6.283).round() * 6.283 };
        let to = if rotate { to.map(wrap) } else { to };
        let (from, dst, time) = if rotate {
            (&mut p.rot_from, &mut p.rot_to, &mut p.rot_time)
        } else {
            (&mut p.pos_from, &mut p.pos_to, &mut p.pos_time)
        };
        let cur = PartGpu::at(*from, *dst, *time, now);
        *from = [cur[0], cur[1], cur[2], 0.0];
        *dst = [to[0], to[1], to[2], 0.0];
        *time = [now, ticks.max(0.0), 0.0, 0.0];
        let at = slot as u64 * std::mem::size_of::<PartGpu>() as u64;
        gpu.queue.put(&self.parts_gpu, at, bytemuck::bytes_of(&*p));
    }

    /// Area 4ccs by slot - 1 (zone's, then the shown sub-area's).
    pub fn area_keys(&self) -> &[u32] {
        &self.area_keys
    }

    /// This frame's lighting per area slot - 1 (None: no environment, lit as the zone).
    pub fn set_area_lights(&mut self, lights: Vec<Option<crate::dat::weather::Lighting>>) {
        self.area_lights = lights;
    }

    /// FUN_10179390: the camera list the eye's probe found (MZB offset, 0 none; Ground::camera_list). While one is in
    /// force the zone's scenery draws only its placements (FUN_1017b510; door groups and a sub-area's scenery always).
    /// `far` (ours, Draw Distance past Retail): beyond that distance, where retail draws nothing, the unlisted
    /// placements draw too, but of a variant group (ZoneScene::variant_groups) only its simplest mesh, and none when
    /// the list holds another of its variants: two variants at one spot fight for depth.
    pub fn set_camera_list(&mut self, gpu: &Gpu, list: u32, far: Option<f32>) {
        let same = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= a.max(b) * 0.01,
            (a, b) => a.is_none() && b.is_none(),
        };
        if list == self.camera_list.0 && same(far, self.camera_list.1) {
            return;
        }
        self.camera_list = (list, far);
        if let Some(l) = &mut self.leak {
            l.set_list(self.camera_lists.get(&list).map(Vec::as_slice));
        }
        let Some(c) = self.cull.as_mut() else { return };
        match self.camera_lists.get(&list) {
            Some(l) => {
                let listed: std::collections::HashSet<u32> = l.iter().copied().collect();
                let variant = |p: u32| self.variants.get(p as usize).copied().flatten();
                let groups: std::collections::HashSet<u32> =
                    l.iter().filter_map(|&p| variant(p)).map(|v| v.0).collect();
                let allowed: Vec<bool> =
                    self.inst_placement.iter().map(|p| *p == u32::MAX || listed.contains(p)).collect();
                let beyond: Vec<bool> = match far {
                    Some(_) => self
                        .inst_placement
                        .iter()
                        .map(|&p| {
                            !listed.contains(&p)
                                && variant(p).is_none_or(|(g, simplest)| simplest && !groups.contains(&g))
                        })
                        .collect(),
                    None => Vec::new(),
                };
                c.set_mask(gpu, Some((&allowed, &beyond, far.unwrap_or(0.0))));
            }
            None => c.set_mask(gpu, None),
        }
    }

    /// Ours (the Lamp Lights setting, client/lamps.rs): the lamps' lights (position, table slot) go into the free light
    /// slots of the zone instances near them, nearest first; an empty list puts the instances' back. Waits for a
    /// streamed zone's geometry.
    pub fn set_lamps(&mut self, gpu: &Gpu, lamps: &[(Vec3, u32)]) {
        self.lamps_wanted = Some(lamps.to_vec());
        if self.geo_pending.is_none() {
            self.apply_lamps(gpu);
        }
    }

    fn apply_lamps(&mut self, gpu: &Gpu) {
        let Some(lamps) = self.lamps_wanted.take() else { return };
        let next = lamp_slots(&self.inst_lights, &self.spheres, &lamps);
        let full = self
            .inst_lights
            .iter()
            .zip(&self.spheres)
            .filter(|(own, s)| {
                !own.contains(&0)
                    && lamps.iter().any(|(p, _)| p.distance(Vec3::new(s[0], s[1], s[2])) - s[3] < LAMP_REACH)
            })
            .count();
        // A sub-area's instances (its buffer) take them the same way.
        if let Some(sub) = &mut self.sub {
            let snext = lamp_slots(&sub.inst_lights, &sub.spheres, &lamps);
            let put = |i: u32, l: &[u32; 4]| {
                gpu.queue.put(&sub.instances, i as u64 * INSTANCE_BYTES + 64, bytemuck::cast_slice(l))
            };
            for (&i, l) in snext.iter().filter(|(i, l)| sub.lamp_patch.get(i) != Some(*l)) {
                put(i, l);
            }
            for &i in sub.lamp_patch.keys().filter(|i| !snext.contains_key(i)) {
                if let Some(own) = sub.inst_lights.get(i as usize) {
                    put(i, own);
                }
            }
            log::info!("lamps: {} sub-area instances lit by lamps", snext.len());
            sub.lamp_patch = snext;
        }
        log::info!(
            "lamps: {} lamp lights on {} instances; {full} near a lamp have no free slot",
            lamps.len(),
            next.len()
        );
        self.lamps_last = lamps;
        let put = |i: u32, l: &[u32; 4]| {
            gpu.queue.put(&self.instances, i as u64 * INSTANCE_BYTES + 64, bytemuck::cast_slice(l))
        };
        for (&i, l) in &next {
            if self.lamp_patch.get(&i) != Some(l)
                && !self.links.iter().any(|(k, link, _)| *k == i && *link == self.hidden)
            {
                put(i, l);
            }
        }
        for &i in self.lamp_patch.keys().filter(|i| !next.contains_key(i)) {
            if let Some(own) = self.inst_lights.get(i as usize) {
                put(i, own);
            }
        }
        self.lamp_patch = next;
        // The slot leak writes its instances again over the new patch.
        if let Some(l) = &mut self.leak {
            l.written.clear();
        }
    }

    /// The point-light slot leak (render/leak.rs) for this frame's view: the instances of placements with 'c' refs
    /// take the slots their draw order leaves them, over the lamps' patch where that is off. Waits for a streamed
    /// zone's geometry.
    pub fn update_leak(&mut self, gpu: &Gpu, camera: &Camera, aspect: f32) {
        if self.geo_pending.is_some() {
            return;
        }
        let Some(l) = &self.leak else { return };
        let view = leak::View {
            view_proj: camera.view_proj(aspect),
            eye: camera.eye,
            far: self.clip[0],
            k: self.clip[1],
            voyage: self.voyage,
        };
        let hidden = |i: u32| self.links.iter().any(|(k, link, _)| *k == i && *link == self.hidden);
        let next = l.resolve(&view, hidden);
        let mut writes = Vec::new();
        for (&i, slots) in &next {
            let own = self.inst_lights.get(i as usize).copied().unwrap_or([0; 4]);
            let v = with_lamps(*slots, own, self.lamp_patch.get(&i).copied());
            if l.written.get(&i) != Some(&v) && !hidden(i) {
                writes.push((i, v));
            }
        }
        for (i, v) in &writes {
            gpu.queue.put(&self.instances, *i as u64 * INSTANCE_BYTES + 64, bytemuck::cast_slice(v));
        }
        if let Some(l) = &mut self.leak {
            l.written.extend(writes);
        }
    }

    /// A sub-area's scenery replaces the loaded one (FUN_101787c0 state 2); `light_keys` is the zone's table with
    /// the sub-area's keys after it. The stand-ins for `link` hide once it is on the GPU (FUN_1017a630).
    pub fn set_sub(
        &mut self,
        gpu: &Gpu,
        link: u32,
        geo: &ZoneGeometry,
        textures: Vec<crate::dat::zone::Texture>,
        light_keys: Vec<u32>,
    ) {
        // Furniture textures sit after the zone's; no Mog House has a sub-area.
        self.clear_furniture();
        self.clear_sub();
        let device = &gpu.device;
        let buffer = |label, bytes: &[u8], usage| {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (bytes.len() as u64).max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.put(&b, 0, bytes);
            b
        };
        let [v, i, m] = geo.parts();
        let tex_base = self.textures.len();
        let white = self.textures[0].clone();
        self.textures.extend(std::iter::repeat_n(white, textures.len()));
        self.pending.extend(textures.into_iter().enumerate().map(|(k, t)| (tex_base + k, t, None)));
        let texture = |t: usize| match t {
            0 => 0,
            t if t > ZONE_TEXTURE => t - ZONE_TEXTURE,
            t => tex_base + t - 1,
        };
        let draws = geo
            .draws
            .iter()
            .map(|d| Draw {
                texture: texture(d.texture),
                instances: d.instances.clone(),
                mirrored: d.mirrored.clone(),
                ..*d
            })
            .collect();
        let instances = buffer("sub-area instances", m, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE);
        let cull = cull::Cull::new(gpu, &self.cull_layout, &geo.cull, &instances);
        self.sub = Some(SubGeometry {
            vertices: buffer("sub-area vertices", v, wgpu::BufferUsages::VERTEX),
            indices: buffer("sub-area indices", i, wgpu::BufferUsages::INDEX),
            instances,
            cull,
            draws,
            tex_base,
            link,
            shown: false,
            inst_lights: geo.matrices.iter().map(|m| m.lights).collect(),
            spheres: geo.spheres.clone(),
            lamp_patch: HashMap::new(),
        });
        self.light_keys = light_keys;
        // The lamps in force reach the new interior's instances too.
        if self.lamps_wanted.is_none() && !self.lamps_last.is_empty() {
            self.lamps_wanted = Some(self.lamps_last.clone());
        }
        if self.geo_pending.is_none() {
            self.apply_lamps(gpu);
        }
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

    /// Mog House furniture replaces the set before it: each model file is its geometry with its textures
    /// (names repeat across files), drawn once all of the set's textures are up.
    pub fn set_furniture(
        &mut self,
        gpu: &Gpu,
        files: Vec<(std::sync::Arc<ZoneGeometry>, Vec<crate::dat::zone::Texture>)>,
    ) {
        self.clear_furniture();
        let device = &gpu.device;
        let buffer = |label, bytes: &[u8], usage| {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (bytes.len() as u64).max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.put(&b, 0, bytes);
            b
        };
        self.furn_base = Some(self.textures.len());
        let lights = furniture_lights(&self.light_keys);
        for (geo, textures) in files {
            let [v, i, _] = geo.parts();
            let inst: Vec<Instance> = geo.matrices.iter().map(|m| Instance { lights, ..*m }).collect();
            let tex_base = self.textures.len();
            let white = self.textures[0].clone();
            self.textures.extend(std::iter::repeat_n(white, textures.len()));
            self.pending.extend(textures.into_iter().enumerate().map(|(k, t)| (tex_base + k, t, None)));
            let texture = |t: usize| match t {
                0 => 0,
                t if t > ZONE_TEXTURE => t - ZONE_TEXTURE,
                t => tex_base + t - 1,
            };
            let draws = geo
                .draws
                .iter()
                .map(|d| Draw {
                    texture: texture(d.texture),
                    instances: d.instances.clone(),
                    mirrored: d.mirrored.clone(),
                    ..*d
                })
                .collect();
            self.furniture.push(SubGeometry {
                vertices: buffer("furniture vertices", v, wgpu::BufferUsages::VERTEX),
                indices: buffer("furniture indices", i, wgpu::BufferUsages::INDEX),
                instances: buffer("furniture instances", bytemuck::cast_slice(&inst), wgpu::BufferUsages::VERTEX),
                // A few pieces that layout mode moves: drawn unculled.
                cull: None,
                draws,
                tex_base,
                link: 0,
                shown: false,
                inst_lights: Vec::new(),
                spheres: Vec::new(),
                lamp_patch: HashMap::new(),
            });
            self.furn_inst.push(inst);
        }
        if !self.budgeted {
            let base = self.furn_base.unwrap_or(0);
            let (now, keep): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.pending).into_iter().partition(|p| p.0 >= base);
            self.pending = keep.into();
            for (slot, t, _) in now {
                self.put_texture(gpu, slot, &t);
            }
            for f in &mut self.furniture {
                f.shown = true;
            }
        }
    }

    /// The overlay lines (pairs of ends); written only when they change.
    pub fn set_lines(&mut self, gpu: &Gpu, mut lines: Vec<LineVertex>) {
        if !self.encode_gamma {
            for v in &mut lines {
                for c in &mut v.color[..3] {
                    *c = c.powf(2.2);
                }
            }
        }
        if lines == self.lines {
            return;
        }
        if !lines.is_empty() {
            if self.line_buf.as_ref().is_none_or(|b| b.1 < lines.len()) {
                let cap = lines.len().next_power_of_two().max(256);
                let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lines"),
                    size: (cap * std::mem::size_of::<LineVertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.line_buf = Some((buf, cap));
            }
            if let Some((buf, _)) = &self.line_buf {
                gpu.queue.put(buf, 0, bytemuck::cast_slice(&lines));
            }
        }
        self.lines = lines;
    }

    /// The Lua mods' world shapes: line pairs and triangles (gamma-space colours); written only when they change.
    pub fn set_world_shapes(&mut self, gpu: &Gpu, mut shapes: [Vec<LineVertex>; 2]) {
        if !self.encode_gamma {
            for v in shapes.iter_mut().flatten() {
                for c in &mut v.color[..3] {
                    *c = c.powf(2.2);
                }
            }
        }
        for (k, list) in shapes.into_iter().enumerate() {
            if list == self.world_shapes[k] {
                continue;
            }
            if !list.is_empty() {
                if self.world_bufs[k].as_ref().is_none_or(|b| b.1 < list.len()) {
                    let cap = list.len().next_power_of_two().max(256);
                    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("world shapes"),
                        size: (cap * std::mem::size_of::<LineVertex>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.world_bufs[k] = Some((buf, cap));
                }
                if let Some((buf, _)) = &self.world_bufs[k] {
                    gpu.queue.put(buf, 0, bytemuck::cast_slice(&list));
                }
            }
            self.world_shapes[k] = list;
        }
    }

    /// The Lua mods' world shapes over the finished scene, behind what stands in front of them.
    pub fn render_world_shapes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        if self.world_shapes.iter().all(|s| s.is_empty()) {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world shapes"),
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
        });
        pass.set_bind_group(0, &self.globals, &[]);
        // Triangles first, so outlines draw over their fills.
        for k in [1, 0] {
            if let (Some((buf, _)), false) = (self.world_bufs[k].as_ref(), self.world_shapes[k].is_empty()) {
                pass.set_pipeline(&self.world_pipelines[k]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.world_shapes[k].len() as u32, 0..1);
            }
        }
    }

    /// The overlay over the finished scene (after effects); uses this frame's globals from render().
    pub fn render_lines(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView) {
        let Some((buf, _)) = self.line_buf.as_ref().filter(|_| !self.lines.is_empty()) else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lines"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.line_pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..self.lines.len() as u32, 0..1);
    }

    pub fn clear_furniture(&mut self) {
        if let Some(base) = self.furn_base.take() {
            self.textures.truncate(base);
            self.pending.retain(|p| p.0 < base);
            self.furniture.clear();
            self.furn_inst.clear();
        }
    }

    /// A piece moved in place (layout mode): the instance of furniture file `file` drawn with `old` takes `new`.
    pub fn move_furniture(&mut self, gpu: &Gpu, file: usize, old: Mat4, new: Mat4) {
        let (Some(f), Some(list)) = (self.furniture.get(file), self.furn_inst.get_mut(file)) else { return };
        let old = old.to_cols_array_2d();
        let Some(i) = list.iter().position(|x| x.model == old) else { return };
        list[i].model = new.to_cols_array_2d();
        gpu.queue.put(&f.instances, i as u64 * INSTANCE_BYTES, bytemuck::bytes_of(&list[i]));
    }

    /// A ship's model goes up at `matrix`, lit as furniture (the zone's 'l_01' only), area 0, drawn at any distance.
    pub fn set_model_actor(
        &mut self,
        gpu: &Gpu,
        id: u32,
        geo: &ZoneGeometry,
        textures: Vec<crate::dat::zone::Texture>,
        matrix: Mat4,
    ) {
        let device = &gpu.device;
        let buffer = |label, bytes: &[u8], usage| {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (bytes.len() as u64).max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.put(&b, 0, bytes);
            b
        };
        let [v, i, _] = geo.parts();
        let inst = Instance {
            model: matrix.to_cols_array_2d(),
            lights: furniture_lights(&self.light_keys),
            area: [0; 4],
            band: [0.0; 4],
        };
        let white = self.textures[0].clone();
        let n = textures.len();
        self.model_actors.insert(
            id,
            ModelGeo {
                vertices: buffer("ship vertices", v, wgpu::BufferUsages::VERTEX),
                indices: buffer("ship indices", i, wgpu::BufferUsages::INDEX),
                instances: buffer("ship instance", bytemuck::bytes_of(&inst), wgpu::BufferUsages::VERTEX),
                draws: geo.draws.iter().map(|d| Draw { instances: 0..1, mirrored: 1..1, ..d.clone() }).collect(),
                inst,
                textures: vec![white; n + 1],
                pending: textures.into_iter().enumerate().map(|(k, t)| (k + 1, t, None)).collect(),
                mips: 0,
                up: false,
                shown: false,
            },
        );
    }

    /// A ship moved or turned, shown or hidden.
    pub fn move_model_actor(&mut self, gpu: &Gpu, id: u32, matrix: Mat4, shown: bool) {
        let Some(m) = self.model_actors.get_mut(&id) else { return };
        m.shown = shown;
        let model = matrix.to_cols_array_2d();
        if m.inst.model != model {
            m.inst.model = model;
            gpu.queue.put(&m.instances, 0, bytemuck::bytes_of(&m.inst));
        }
    }

    pub fn remove_model_actor(&mut self, id: u32) {
        self.model_actors.remove(&id);
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

    /// FUN_1017a590: the link-999 placements (a nation house's ceiling) shown or not.
    pub fn show_link999(&mut self, gpu: &Gpu, shown: bool) {
        if shown != self.link999_off {
            return;
        }
        for (i, _, inst) in self.links.iter().filter(|x| x.1 == 999) {
            let v = if shown { *inst } else { Instance::zeroed() };
            gpu.queue.put(&self.instances, *i as u64 * INSTANCE_BYTES, bytemuck::bytes_of(&v));
        }
        self.link999_off = !shown;
    }

    /// FUN_1017a630: zone instances whose link is `link` draw nothing, the others as built.
    fn hide_links(&mut self, gpu: &Gpu, link: u32) {
        if link == self.hidden {
            return;
        }
        for (i, l, inst) in &self.links {
            if *l == link || *l == self.hidden {
                let lights = self.lamp_patch.get(i).copied().unwrap_or(inst.lights);
                let v = if *l == link { Instance::zeroed() } else { Instance { lights, ..*inst } };
                gpu.queue.put(&self.instances, *i as u64 * INSTANCE_BYTES, bytemuck::bytes_of(&v));
            }
        }
        self.hidden = link;
        // The slot leak writes its instances again over the restored ones.
        if let Some(l) = &mut self.leak {
            l.written.clear();
        }
    }

    fn put_texture(&mut self, gpu: &Gpu, slot: usize, t: &crate::dat::zone::Texture) {
        let (view, seq) = upload_texture_seq(gpu, t, true);
        self.set_texture(gpu, slot, t, &view, seq);
    }

    fn set_texture(
        &mut self,
        gpu: &Gpu,
        slot: usize,
        t: &crate::dat::zone::Texture,
        view: &wgpu::TextureView,
        seq: u64,
    ) {
        self.textures[slot] = view_bind_group(gpu, &self.texture_layout, &self.sampler, view);
        self.mips_seq = self.mips_seq.max(seq);
        self.tex_bytes.1 += (t.width * t.height * 4) as u64;
        self.tex_bytes.2 += 1;
    }

    fn log_textures(&self) {
        let (before, rgba, n) = self.tex_bytes;
        if n > 0 {
            let now = TEXTURE_BYTES.load(std::sync::atomic::Ordering::Relaxed);
            log::info!(
                "zone textures: {n} on the GPU with mips ({:.1} MB; plain RGBA without mips was {:.1} MB)",
                now.saturating_sub(before) as f32 / 1e6,
                rgba as f32 / 1e6
            );
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
            } else {
                self.apply_lamps(gpu);
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
        let sub_up = self
            .sub
            .as_ref()
            .filter(|s| {
                !s.shown
                    && self.geo_pending.is_none()
                    && !self.pending.iter().any(|p| p.0 >= s.tex_base)
                    && gpu.mips_done() >= self.mips_seq
            })
            .map(|s| s.link);
        if let Some(link) = sub_up {
            self.hide_links(gpu, link);
            if let Some(s) = &mut self.sub {
                s.shown = true;
            }
        }
        for m in self.model_actors.values_mut() {
            while gpu.upload_room() > 0
                && let Some((_, t, up)) = m.pending.front_mut()
            {
                let up = up.get_or_insert_with(|| upload::TexUp::new(gpu, t, true));
                let Some(seq) = up.step(gpu, t, false) else { break };
                let view = up.texture.create_view(&Default::default());
                let (slot, _, _) = m.pending.pop_front().unwrap();
                m.textures[slot] = view_bind_group(gpu, &self.texture_layout, &self.sampler, &view);
                m.mips = m.mips.max(seq);
            }
            m.up = m.pending.is_empty() && gpu.mips_done() >= m.mips;
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
        a.draws = mesh
            .groups
            .iter()
            .map(|(tex, r, wep)| (a.slot.get(tex.as_str()).copied().unwrap_or(0), r.clone(), *wep))
            .collect();
        let vertices = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("skinned actor"),
            contents: bytemuck::cast_slice(&mesh.verts),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
        });
        a.skin = Some(Skinned {
            vertices,
            shine: Default::default(),
            bones: mesh.bones,
            cloth: mesh.cloth,
            sim: mesh.sim.clone(),
            offset: 0,
            frame: u64::MAX,
        });
        a
    }

    /// A skinned actor of a model. A new model goes up over the next frames under the copy budget; None until
    /// then (the actor is not drawn).
    pub fn skinned_actor(
        &mut self,
        gpu: &Gpu,
        ch: &std::sync::Arc<crate::dat::character::Character>,
    ) -> Option<ActorGpu> {
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
            let draws = mesh
                .groups
                .iter()
                .map(|(tex, r, wep)| (slot.get(tex.as_str()).copied().unwrap_or(0), r.clone(), *wep))
                .collect();
            let size = (std::mem::size_of_val(mesh.verts.as_slice()) as u64).max(4);
            let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("skinned actor"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let instance = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("actor instance"),
                size: INSTANCE_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let actor = ActorGpu {
                faded_pending: Default::default(),
                textures: vec![self.magenta.clone(); ch.textures.len() + 1],
                slot,
                vertices: None,
                draws,
                instance,
                hidden_weps: ch.init_hidden_mask(),
                shine: None,
                refract: None,
                skin: Some(Skinned {
                    vertices,
                    shine: Default::default(),
                    bones: mesh.bones,
                    cloth: mesh.cloth,
                    sim: mesh.sim.clone(),
                    offset: 0,
                    frame: u64::MAX,
                }),
                alpha: 1.0,
                shadow_off: false,
                tint: [1.0; 3],
                flash: [0.0, 1.0],
                lights: [0; 4],
                area: 0,
                cloth: Vec::new(),
                cloth_state: Default::default(),
                mips: 0,
                origin: Vec3::ZERO,
                screen: None,
                lamp_caster: false,
                lamp_lit: false,
            };
            self.builds
                .push((key, Build { ch: ch.clone(), mesh, actor, registered: false, vert_at: 0, tex: 0, cur: None }));
        }
        None
    }

    /// Starts this frame's poses; skinned actors not posed again are not drawn.
    pub fn begin_poses(&mut self) {
        self.poses.clear();
        self.pose_frame += 1;
        self.anim.begin();
        self.cloth.begin();
    }

    /// Queues a skinned actor posed on the GPU from its motions (to, and from at blend `t`), and its placement.
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
        grips: &[(u16, u16)],
    ) {
        if actor.skin.is_none() {
            return;
        }
        self.pose_actor(actor, &[], model);
        if let Some(skin) = &actor.skin {
            let out = skin.offset / 16 + POSE_HEAD as u32;
            self.anim.job(ch, files, to, from, t, out, look, overlay, grips);
        }
    }

    /// Queues a skinned actor's bones (world transforms from character::pose) and placement.
    pub fn pose_actor(&mut self, actor: &mut ActorGpu, bones: &[crate::dat::character::BoneXf], model: Mat4) {
        let alpha = actor.alpha.clamp(0.0, 1.0);
        let tint = actor.tint;
        let flash = actor.flash;
        let area = match self.area_keys.iter().position(|&k| k == actor.area) {
            Some(s) if actor.area != 0 => (s + 1) as f32,
            _ => 0.0,
        };
        actor.origin = model.w_axis.truncate();
        let Some(skin) = actor.skin.as_mut() else { return };
        while !self.poses.len().is_multiple_of(16) {
            self.poses.push([0.0; 4]);
        }
        skin.offset = (self.poses.len() * 16) as u32;
        skin.frame = self.pose_frame;
        self.poses.extend(model.to_cols_array_2d());
        self.poses.push([alpha, flash[0], flash[1], f32::from_bits(u32::from_le_bytes(actor.lights))]);
        self.poses.push([area, tint[0], tint[1], tint[2]]);
        for i in 0..skin.bones {
            let b = bones.get(i).copied().unwrap_or(crate::dat::character::BoneXf::IDENTITY);
            self.poses.push(b.q.to_array());
            self.poses.push(b.t.extend(0.0).to_array());
            self.poses.push(b.s.extend(0.0).to_array());
        }
        self.poses.push([0.0, 0.0, 0.0, 1.0]);
        self.poses.push([0.0; 4]);
        self.poses.push([1.0, 1.0, 1.0, 0.0]);
        // Unset particles (w 0) draw skinned.
        let n = skin.cloth * 2;
        self.poses.extend(actor.cloth.iter().copied().chain(std::iter::repeat([0.0; 4])).take(n));
    }

    /// The actor's cloth this frame (render/cloth.rs): its tick, and with `drawn` (its model matrix and its move since
    /// the last step) the particles into the pose region it was posed in this frame.
    /// True when the tick was taken (render/cloth.rs `ClothGpu::job`).
    pub fn cloth(&mut self, actor: &mut ActorGpu, tick: cloth::Tick, drawn: Option<(Mat4, Vec3)>) -> bool {
        let Some(skin) = actor.skin.as_ref() else { return false };
        let Some(sim) = skin.sim.clone() else { return false };
        let region = (skin.frame == self.pose_frame).then_some((skin.offset / 16, skin.bones as u32));
        self.cloth.job(&mut actor.cloth_state, &sim, tick, region, drawn)
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
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("poses"),
                size: cap,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("poses"),
                layout: &self.pose_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(POSE_BINDING),
                    }),
                }],
            });
            self.pose_gpu = Some((buf, group, cap));
        }
        if let Some((buf, _, _)) = &self.pose_gpu {
            gpu.queue.put(buf, 0, bytemuck::cast_slice(&self.poses));
            self.anim.dispatch(gpu, encoder, buf, !self.budgeted);
            self.cloth.dispatch(gpu, encoder, buf, !self.budgeted);
            self.points.run(gpu, encoder, buf);
        }
    }

    pub fn new_actor(&self, gpu: &Gpu, ch: &crate::dat::character::Character) -> ActorGpu {
        // Slot 0: magenta, so a missing texture is obvious.
        let mut textures =
            vec![texture_bind_group(gpu, &self.texture_layout, &self.sampler, 1, 1, &[255, 0, 255, 255])];
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
        ActorGpu {
            faded_pending: Default::default(),
            textures,
            slot,
            vertices: None,
            draws: Vec::new(),
            instance,
            hidden_weps: 0,
            shine: None,
            refract: None,
            skin: None,
            alpha: 1.0,
            shadow_off: false,
            tint: [1.0; 3],
            flash: [0.0, 1.0],
            lights: [0; 4],
            area: 0,
            cloth: Vec::new(),
            cloth_state: Default::default(),
            mips,
            origin: Vec3::ZERO,
            screen: None,
            lamp_caster: false,
            lamp_lit: false,
        }
    }

    /// A shine image goes up once (effect file, IMG node).
    pub fn has_shine(&self, key: (u32, usize)) -> bool {
        self.shine_tex.contains_key(&key)
    }

    pub fn add_shine(&mut self, gpu: &Gpu, key: (u32, usize), t: &crate::dat::zone::Texture) {
        let view = upload_texture(gpu, t, true);
        self.shine_tex.insert(key, view_bind_group(gpu, &self.texture_layout, &self.sampler, &view));
    }

    /// This frame's environment animation (config 0011): 60 Hz ticks, steps 1 / 16 / 32, the weather tag's index
    /// in dat::zone::SWAY_TAGS (None: the "suny" curves).
    pub fn set_sway(&mut self, ticks: f32, steps: u32, tag: Option<usize>) {
        if let Some(s) = &mut self.sway {
            s.next = (ticks, steps, tag.unwrap_or(1) as u32);
        }
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
            verts.extend(tris.iter().map(|v| GpuVertex {
                pos: [v[0], v[1], v[2]],
                normal: [v[3], v[4], v[5]],
                color,
                uv: [v[6], v[7]],
                flags: 0,
            }));
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
            gpu.queue.put(buf, 0, bytes);
        }
        gpu.queue.put(&actor.instance, 0, bytemuck::bytes_of(&model.to_cols_array_2d()));
    }

    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }

    /// The scene's uniforms seen from `camera` (this frame's lighting, fog and areas) and its clear colour.
    fn frame_uniforms(&self, camera: &Camera, aspect: f32) -> (Uniforms, [f32; 3]) {
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
            voyage: self.voyage.to_cols_array_2d(),
            two: [[0.0; 4]; 4],
            modes: [self.bump_mode as u32 as f32, self.smooth_lights as u32 as f32, 0.0, 0.0],
            boost: self.light_boost,
        };
        let mut clear = self.fog_color;
        if let Some(l) = &self.lighting {
            u.sun = v4(l.dir, 1.0);
            u.sun_color = v4(l.diffuse, 1.0);
            u.two = l.two.map(|v| v4(v, 1.0));
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
        if let Some(c) = self.mod_clear {
            let lin = |c: f32| if self.encode_gamma { c } else { c.powf(2.2) };
            clear = c.map(lin);
            u.clear = [c[0], c[1], c[2], 1.0];
        }
        if let Some(a) = self.mod_ambient {
            (u.ambient, u.actor_ambient) = ([a[0], a[1], a[2], 1.0], [a[0], a[1], a[2], 1.0]);
        }
        // FUN_10183da0: instances with an area are lit by its environment; flagged models by 'ev10' while a sub-area
        // is in (+0x50, FUN_101787c0).
        u.area_info[1] = self.now;
        (u.area_info[2], u.area_info[3]) = (self.clip[0], self.clip[1]);
        let ev10 = u32::from_le_bytes(*b"ev10");
        if self.sub.as_ref().is_some_and(|s| s.shown)
            && let Some(s) = self.area_keys.iter().position(|&k| k == ev10)
        {
            u.area_info[0] = (s + 1) as f32;
        }
        (u, clear)
    }

    pub fn render(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        camera: &Camera,
        aspect: f32,
        actors: &[&ActorGpu],
    ) {
        if !self.budgeted {
            gpu.run_mips(encoder, u64::MAX);
        }
        let done = gpu.mips_done();
        let vp = camera.view_proj(aspect);
        let v4 = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
        let (u, clear) = self.frame_uniforms(camera, aspect);
        let mut areas = [AreaGpu::zeroed(); AREAS];
        for (slot, l) in self.area_lights.iter().enumerate().take(AREAS - 1) {
            if let Some(l) = l {
                areas[slot + 1] = AreaGpu {
                    sun: v4(l.dir, 1.0),
                    sun_color: v4(l.diffuse, 1.0),
                    ambient: v4(l.ambient, 1.0),
                    fog_color: v4(l.fog, 1.0),
                    fog: [l.fog_start, l.fog_end, 0.0, 0.0],
                    actor_sun: v4(l.actor_dir, 1.0),
                    actor_color: v4(l.actor_diffuse, 1.0),
                    actor_ambient: v4(l.actor_ambient, 1.0),
                    two: l.two.map(|v| v4(v, 1.0)),
                };
                if let Some(a) = self.mod_ambient {
                    (areas[slot + 1].ambient, areas[slot + 1].actor_ambient) =
                        ([a[0], a[1], a[2], 1.0], [a[0], a[1], a[2], 1.0]);
                }
            }
        }
        gpu.queue.put(&self.areas, 0, bytemuck::cast_slice(&areas));
        gpu.queue.put(&self.uniforms, 0, bytemuck::bytes_of(&u));
        if let Some(s) = &self.sway {
            s.run(gpu, encoder);
        }
        // The scenery this camera sees (render/cull.rs), from the uniforms just written.
        if let Some(c) = &self.cull {
            c.run(encoder, &self.globals, &self.cull_pipes);
        }
        if let Some(c) = self.sub.as_ref().filter(|s| s.shown).and_then(|s| s.cull.as_ref()) {
            c.run(encoder, &self.globals, &self.cull_pipes);
        }
        // Real character shadows (render/sunshadow.rs): the actors' depth from the zone's sun, laid on the scenery
        // below after all the scenery (the blended too) and before the actors.
        let sun = match (&self.sun_shadow, self.sun_shadow_light(camera.target), &self.pose_gpu) {
            (Some(s), Some((m, d, st)), Some((_, group, _))) => Some((s, m, d, st, group)),
            _ => None,
        };
        if let Some((s, m, _, _, group)) = &sun {
            s.set_light(gpu, *m);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sun shadow depth"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &s.feet,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &s.map,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.sun_depth_pipeline);
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_bind_group(3, &s.matrix_group, &[0]);
            self.draw_actor_depth(&mut pass, actors, group, done, Depth::Sun);
            // Those a lamp map takes in leave the sun's map on the GPU, where the lights' live state is.
            pass.set_pipeline(&self.sun_depth_lamped_pipeline);
            self.draw_actor_depth(&mut pass, actors, group, done, Depth::SunLamped);
        }
        // Lamp shadows (render/lampshadow.rs): each shadowed lamp's characters into its layer; its receive pass reads them.
        let lamps: &[(u32, Mat4, Vec3)] = if self.pose_gpu.is_some() { &self.lamp_shadows } else { &[] };
        self.lamp_shadow.set(gpu, lamps);
        if let Some((_, group, _)) = &self.pose_gpu {
            for (k, _) in lamps.iter().enumerate().take(lampshadow::LAYERS) {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lamp shadow depth"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.lamp_shadow.layers[k],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.lamp_depth_pipeline);
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_bind_group(3, &self.lamp_shadow.matrix_group, &[256 * k as u32]);
                self.draw_actor_depth(&mut pass, actors, group, done, Depth::Lamp);
            }
        }
        let [r, g, b] = clear;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zone"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if self.clear_color {
                        wgpu::LoadOp::Clear(wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: 1.0 })
                    } else {
                        wgpu::LoadOp::Load
                    },
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
        // The zone's scenery draws from its culled instances when it is culled, only runs that may show.
        let shown = self.cull.as_ref().map(|c| c.runs_shown(vp, camera.eye, self.clip)).unwrap_or_default();
        let (zone_inst, zone_args) = match &self.cull {
            Some(c) => (&c.out, Some((c, &c.args, shown.as_slice()))),
            None => (&self.instances, None),
        };
        let sub_shown = self
            .sub
            .as_ref()
            .and_then(|s| s.cull.as_ref())
            .map(|c| c.runs_shown(vp, camera.eye, self.clip))
            .unwrap_or_default();
        pass.set_vertex_buffer(1, zone_inst.slice(..));
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        // Mode 4 is no scenery: the point after the blended scenery, before the actors.
        for mode in 0..5u8 {
            // The ambient occlusion goes on the opaque and cut-out scenery (mode 2). The real shadows wait until the
            // blended scenery is down too (mode 4): many ground pieces are blended layers laid over the terrain, which
            // would otherwise cover the shadow and cut it off along their edges. They write no depth, so the pass
            // reads the ground under them. Then the pass resumes.
            let ssao_now = mode == 2 && self.ssao.is_some();
            let sun_now = mode == 4 && (sun.is_some() || !lamps.is_empty());
            if ssao_now || sun_now {
                drop(pass);
                if let Some(a) = self.ssao.as_ref().filter(|_| ssao_now) {
                    a.draw(gpu, encoder, color, depth, vp, camera.eye);
                }
                if let Some((s, m, d, st, _)) = sun.as_ref().filter(|_| sun_now) {
                    s.receive(gpu, encoder, color, depth, vp, *m, *d, *st, camera.eye, self.shadow_soft);
                }
                if sun_now {
                    self.lamp_shadow.receive(gpu, encoder, color, depth, vp, lamps, camera.eye, &self.lights);
                }
                pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("zone (after shadows)"),
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
                });
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_vertex_buffer(1, zone_inst.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
            if mode == 4 {
                break;
            }
            let mut bound = usize::MAX;
            let mut pipe = usize::MAX;
            for d in self.draws.iter().filter(|d| d.mode == mode) {
                self.draw_scenery(&mut pass, d, mode, &mut bound, &mut pipe, zone_args);
            }
            if let Some(s) = self.sub.as_ref().filter(|s| s.shown) {
                let (inst, args) = match &s.cull {
                    Some(c) => (&c.out, Some((c, &c.args, sub_shown.as_slice()))),
                    None => (&s.instances, None),
                };
                pass.set_vertex_buffer(1, inst.slice(..));
                pass.set_vertex_buffer(0, s.vertices.slice(..));
                pass.set_index_buffer(s.indices.slice(..), wgpu::IndexFormat::Uint16);
                for d in s.draws.iter().filter(|d| d.mode == mode) {
                    self.draw_scenery(&mut pass, d, mode, &mut bound, &mut pipe, args);
                }
                pass.set_vertex_buffer(1, zone_inst.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
            for f in self.furniture.iter().filter(|f| f.shown) {
                pass.set_vertex_buffer(1, f.instances.slice(..));
                pass.set_vertex_buffer(0, f.vertices.slice(..));
                pass.set_index_buffer(f.indices.slice(..), wgpu::IndexFormat::Uint16);
                for d in f.draws.iter().filter(|d| d.mode == mode) {
                    // FUN_101844c0 leaves depth writes on for the blended meshes (batch 7 item 23), so modes 2 / 3
                    // take the furniture pipelines; `pipe` is invalidated either way by the different set.
                    self.draw_scenery_pipes(&mut pass, d, mode, &mut bound, &mut pipe, None, mode >= 2);
                }
            }
            if self.furniture.iter().any(|f| f.shown) {
                pass.set_vertex_buffer(1, zone_inst.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
            let ships: Vec<&ModelGeo> = self.model_actors.values().filter(|m| m.up && m.shown).collect();
            for m in &ships {
                pass.set_vertex_buffer(1, m.instances.slice(..));
                pass.set_vertex_buffer(0, m.vertices.slice(..));
                pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint16);
                for d in m.draws.iter().filter(|d| d.mode == mode) {
                    for (c, range) in d.runs() {
                        if range.is_empty() {
                            continue;
                        }
                        pass.set_bind_group(1, &m.textures[d.texture.min(m.textures.len() - 1)], &[]);
                        // A ship's model is drawn through FUN_101844c0 too, so its blended meshes keep depth writes.
                        let want = if mode >= 2 { 16 + c } else { c };
                        if pipe != want {
                            let normal = if mode >= 2 {
                                &self.scenery_furn[mode as usize - 2][c]
                            } else {
                                &self.scenery[mode as usize][c]
                            };
                            pass.set_pipeline(self.wire(c).unwrap_or(normal));
                            pipe = want;
                        }
                        pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, range);
                    }
                }
            }
            if !ships.is_empty() {
                pass.set_vertex_buffer(1, zone_inst.slice(..));
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            }
        }
        // Faded actors go through the offscreen target after this pass when prepare_faded made one (FUN_1002b6e0 with
        // FUN_1001d240): at most SHINE_SLOTS of them, the rest blend triangle by triangle here.
        let faded: Vec<usize> = match (&self.faded_rt, &self.pose_gpu) {
            (Some(_), Some(_)) => actors
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.mips <= done
                        && a.refract.is_none()
                        && a.alpha < 1.0
                        && a.skin.as_ref().is_some_and(|s| s.frame == self.pose_frame)
                })
                .map(|(i, _)| i)
                .take(SHINE_SLOTS)
                .collect(),
            _ => Vec::new(),
        };
        let solid = |i: usize| !faded.contains(&i);
        for (i, a) in actors.iter().enumerate() {
            a.faded_pending.set(self.defer_faded && faded.contains(&i));
        }
        let mut slot = 0u64;
        if let Some((_, group, _)) = &self.pose_gpu {
            pass.set_pipeline(&self.skin_pipeline);
            for (_, a) in actors.iter().enumerate().filter(|(i, a)| a.mips <= done && a.refract.is_none() && solid(*i))
            {
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
            for (_, a) in actors.iter().enumerate().filter(|(i, a)| a.mips <= done && a.refract.is_none() && solid(*i))
            {
                let (Some(s), Some(skin)) = (a.shine, a.skin.as_ref().filter(|s| s.frame == self.pose_frame)) else {
                    continue;
                };
                let Some(tex) = self.shine_tex.get(&s.key) else { continue };
                if slot >= SHINE_SLOTS as u64 {
                    break;
                }
                gpu.queue.put(
                    &self.shine_buf,
                    slot * SHINE_STRIDE,
                    bytemuck::cast_slice(&[s.u, s.v, shine_wrap(), 0.0, s.tf[0], s.tf[1], s.tf[2], s.tf[3]]),
                );
                pass.set_pipeline(&self.shine_pipeline);
                pass.set_bind_group(3, self.shine_group(gpu, skin), &[(slot * SHINE_STRIDE) as u32]);
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
        drop(pass);
        if self.defer_faded {
            self.faded_slot.set(slot);
            return;
        }
        let mut list: Vec<&ActorGpu> = faded.iter().map(|&i| actors[i]).collect();
        list.sort_by(|a, b| b.origin.distance(camera.eye).total_cmp(&a.origin.distance(camera.eye)));
        self.render_faded(gpu, encoder, color, depth, &list, slot);
    }

    /// The faded actors render left out (`defer_faded`), farthest first, with their sort keys (view depth, as the
    /// effect elements'): the caller composites each where it falls among the effects (render_faded_one), so what is
    /// behind it shows through and what is in front stays in front. `actors`: any list holding them.
    pub fn faded_pending<'a>(&self, camera: &Camera, actors: &[&'a ActorGpu]) -> Vec<(f32, &'a ActorGpu)> {
        let view = camera.view();
        let mut list: Vec<(f32, &ActorGpu)> =
            actors.iter().copied().filter(|a| a.faded_pending.replace(false)).map(|a| (a.sort_key(&view), a)).collect();
        list.sort_by(|a, b| b.0.total_cmp(&a.0));
        list
    }

    /// One of faded_pending's actors (`k`: its place in that list).
    pub fn render_faded_one(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        a: &ActorGpu,
        k: usize,
    ) {
        let mut slot = self.faded_slot.get();
        self.faded_one(gpu, encoder, color, depth, a, k, &mut slot);
        self.faded_slot.set(slot);
    }

    /// One actor posed this frame drawn alone into `color` / `depth` (cleared to `clear`, premultiplied) from `camera`,
    /// lit as the scene: a Lua HUD portrait. `slot` picks its uniforms, one per portrait a frame. False when it is not
    /// posed this frame or its textures are not all up.
    pub fn render_portrait(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        camera: &Camera,
        aspect: f32,
        clear: [f32; 4],
        a: &ActorGpu,
    ) -> bool {
        let (Some((_, poses, _)), Some(skin)) =
            (&self.pose_gpu, a.skin.as_ref().filter(|s| s.frame == self.pose_frame))
        else {
            return false;
        };
        if a.mips > gpu.mips_done() || a.refract.is_some() {
            return false;
        }
        while self.portrait_globals.len() <= slot {
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("portrait uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("portrait globals"),
                layout: &self.globals_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.lights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.areas.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.parts_gpu.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.deltas_gpu.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: self.lamp_shadow.uniform.as_entire_binding() },
                ],
            });
            self.portrait_globals.push((buf, group));
        }
        let (u, _) = self.frame_uniforms(camera, aspect);
        let (buf, globals) = &self.portrait_globals[slot];
        let clear = wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: clear[3] as f64 };
        gpu.queue.put(buf, 0, bytemuck::bytes_of(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("portrait"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.skin_pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(2, poses, &[skin.offset]);
        pass.set_vertex_buffer(0, skin.vertices.slice(..));
        for (tex, range, wep) in &a.draws {
            if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                continue;
            }
            pass.set_bind_group(1, &a.textures[*tex], &[]);
            pass.draw(range.clone(), 0..1);
        }
        true
    }

    /// The faded actors, farthest first (retail's translucent list DAT_1047bc94, back to front): each drawn opaque with
    /// z written into the offscreen target cleared to 0 (FUN_1001d240), its shine too, then that target blended over
    /// the scene as one viewport quad at the actor's alpha (FUN_1001d3b0), so the model does not show through itself.
    /// `shine_slot`: the first free slot of the shine buffer.
    fn render_faded(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        list: &[&ActorGpu],
        mut shine_slot: u64,
    ) {
        for (k, a) in list.iter().enumerate() {
            self.faded_one(gpu, encoder, color, depth, a, k, &mut shine_slot);
        }
    }

    /// One faded actor: drawn opaque into the helper target inside its rect, then blended over `color` at its alpha.
    /// `k` picks its uniforms (one per actor a frame).
    #[allow(clippy::too_many_arguments)]
    fn faded_one(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        a: &ActorGpu,
        k: usize,
        shine_slot: &mut u64,
    ) {
        let (Some((rt_tex, rt, rt_group)), Some((_, group, _))) = (&self.faded_rt, &self.pose_gpu) else { return };
        let size = rt_tex.size();
        {
            let Some(skin) = a.skin.as_ref() else { return };
            // Ours: both passes only inside the actor's rect; the composite reads nothing outside it, so the target is
            // zeroed there alone (retail clears and blends the whole viewport per actor).
            let Some([sx, sy, sw, sh]) = faded_rect(a.screen, size.width, size.height) else { return };
            let off = k as u64 * SHINE_STRIDE;
            gpu.queue.put(
                &self.faded_buf,
                off,
                bytemuck::cast_slice(&[0.0f32, 0.0, a.alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0, 0.0, 0.0]),
            );
            let shown = |wep: u8| wep == NO_WEP || a.hidden_weps & (1 << wep) == 0;
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("faded actor offscreen"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: rt,
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
                });
                pass.set_scissor_rect(sx, sy, sw, sh);
                pass.set_pipeline(&self.faded_clear_pipeline);
                pass.draw(0..3, 0..1);
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_bind_group(2, group, &[skin.offset]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                pass.set_pipeline(&self.skin_solid_pipeline);
                for (tex, range, wep) in &a.draws {
                    if shown(*wep) {
                        pass.set_bind_group(1, &a.textures[*tex], &[]);
                        pass.draw(range.clone(), 0..1);
                    }
                }
                if let Some(s) = a.shine
                    && let Some(tex) = self.shine_tex.get(&s.key)
                    && *shine_slot < SHINE_SLOTS as u64
                {
                    gpu.queue.put(
                        &self.shine_buf,
                        *shine_slot * SHINE_STRIDE,
                        bytemuck::cast_slice(&[s.u, s.v, shine_wrap(), 0.0, s.tf[0], s.tf[1], s.tf[2], s.tf[3]]),
                    );
                    pass.set_pipeline(&self.shine_pipeline);
                    pass.set_bind_group(3, self.shine_group(gpu, skin), &[(*shine_slot * SHINE_STRIDE) as u32]);
                    pass.set_bind_group(1, tex, &[]);
                    for (_, range, wep) in &a.draws {
                        if shown(*wep) {
                            pass.draw(range.clone(), 0..1);
                        }
                    }
                    *shine_slot += 1;
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("faded actor composite"),
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
            });
            pass.set_scissor_rect(sx, sy, sw, sh);
            pass.set_pipeline(&self.faded_pipeline);
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_bind_group(1, rt_group, &[]);
            pass.set_bind_group(2, group, &[skin.offset]);
            pass.set_bind_group(3, &self.faded_group, &[off as u32]);
            pass.draw(0..3, 0..1);
        }
    }

    /// The skinned actors into a shadow depth pass (`which`: the sun's or a lamp's; its pipeline and groups 0 and 3 set).
    fn draw_actor_depth(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        actors: &[&ActorGpu],
        group: &wgpu::BindGroup,
        done: u64,
        which: Depth,
    ) {
        let pick = |a: &ActorGpu| match which {
            Depth::Sun => !a.lamp_lit,
            Depth::SunLamped => a.lamp_lit,
            Depth::Lamp => a.lamp_caster,
        };
        for a in actors.iter().filter(|a| a.casts_shadow(done) && pick(a)) {
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
    }

    /// The real shadows' light this frame around `target` (the camera's): matrix, sun direction, strength. None when
    /// they are off, there is no lit sun or moon (no zone lighting, a dim outdoor light) or no poses; indoors the room's
    /// fixed light casts. By night `lighting` carries the moon, whose dimmer colour gives fainter shadows.
    pub fn sun_shadow_light(&self, target: Vec3) -> Option<(Mat4, Vec3, f32)> {
        let s = self.sun_shadow.as_ref().filter(|_| self.pose_gpu.is_some())?;
        let l = self.lighting.as_ref()?;
        // A light too dim to cast, or the dawn / dusk fade near zero, falls back to retail's (sunshadow::HANDOFF).
        // Indoors (no sun or moon) a dark room light still casts, from its data direction at a fixed strength.
        let base = sunshadow::strength(l.diffuse);
        let st = if l.fixed {
            if base >= sunshadow::HANDOFF { base } else { sunshadow::INDOOR_STRENGTH }
        } else {
            base * self.shadow_level.clamp(0.0, 1.0)
        };
        (st >= sunshadow::HANDOFF).then_some(())?;
        Some((sunshadow::light_matrix(l.dir, target, s.size)?, sunshadow::sun_direction(l.dir)?, st))
    }

    /// Why the real shadows are off this frame, or how `actor` stands in their map (diagnostics, log target
    /// "sunshadow"): the light's state, then the actor's (mips pending, too faded, not posed).
    pub fn sun_shadow_state(&self, gpu: &Gpu, target: Vec3, actor: Option<&ActorGpu>) -> String {
        let light = match (&self.sun_shadow, &self.pose_gpu, &self.lighting) {
            (None, _, _) => "off".to_string(),
            (_, None, _) => "no poses".to_string(),
            (_, _, None) => "no zone lighting".to_string(),
            (_, _, Some(l)) => match self.sun_shadow_light(target) {
                Some((_, _, st)) => format!("on (strength {st:.2})"),
                None => format!("dim light (diffuse {:.3}, strength {:.2})", l.diffuse, sunshadow::strength(l.diffuse)),
            },
        };
        let Some(a) = actor else { return format!("{light}; no actor") };
        let done = gpu.mips_done();
        let posed = a.skin.as_ref().is_some_and(|s| s.frame == self.pose_frame);
        format!(
            "{light}; actor mips {} (done {done}), alpha {:.2}, posed {posed}, refract {}, latched off {}",
            a.mips,
            a.alpha,
            a.refract.is_some(),
            a.shadow_off
        )
    }

    /// Ambient occlusion (ours) on or off at the scene's `size`; made at the first use.
    pub fn prepare_ssao(&mut self, gpu: &Gpu, on: bool, size: [u32; 2]) {
        if !on {
            self.ssao = None;
            return;
        }
        let format = self.color_format;
        self.ssao.get_or_insert_with(|| ssao::Ssao::new(gpu, format)).resize(gpu, size);
    }

    /// Real character shadows (ours) on or off with a `size` map; made at the first use or a size change. render draws
    /// them only with a lit sun or moon.
    pub fn prepare_sun_shadow(&mut self, gpu: &Gpu, on: bool, size: u32) {
        let size = if sunshadow::MAP_SIZES.contains(&size) { size } else { sunshadow::MAP_SIZE };
        if !on {
            self.sun_shadow = None;
        } else if self.sun_shadow.as_ref().is_none_or(|s| s.size != size) {
            self.sun_shadow = Some(sunshadow::SunShadow::new(gpu, &self.sil_layout, self.color_format, size));
        }
    }

    /// The faded actors' offscreen target at the scene target's size (`scene`: the texture render draws into), in the
    /// pipelines' colour format. Without it render blends a faded actor triangle by triangle.
    pub fn prepare_faded(&mut self, gpu: &Gpu, scene: &wgpu::Texture) {
        let format = self.color_format;
        if self.faded_rt.as_ref().is_some_and(|(t, _, _)| t.size() == scene.size() && t.format() == format) {
            return;
        }
        let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("faded actor target"),
            size: scene.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = t.create_view(&Default::default());
        let g = view_bind_group(gpu, &self.texture_layout, &self.sampler, &view);
        self.faded_rt = Some((t, view, g));
    }
}

/// A faded actor's pixels in a w x h target: its box's NDC rect grown by a quarter of its size and 8 px each way (the
/// motion reaches past the box), as x, y, width, height; the whole target without a rect, none when it is off it.
fn faded_rect(screen: Option<[f32; 4]>, w: u32, h: u32) -> Option<[u32; 4]> {
    let Some([x0, y0, x1, y1]) = screen else { return Some([0, 0, w, h]) };
    let (mx, my) = ((x1 - x0) * 0.25, (y1 - y0) * 0.25);
    let (fw, fh) = (w as f32, h as f32);
    let l = (((x0 - mx) * 0.5 + 0.5) * fw - 8.0).floor().clamp(0.0, fw) as u32;
    let r = (((x1 + mx) * 0.5 + 0.5) * fw + 8.0).ceil().clamp(0.0, fw) as u32;
    let t = ((0.5 - (y1 + my) * 0.5) * fh - 8.0).floor().clamp(0.0, fh) as u32;
    let b = ((0.5 - (y0 - my) * 0.5) * fh + 8.0).ceil().clamp(0.0, fh) as u32;
    (r > l && b > t).then(|| [l, t, r - l, b - t])
}

impl ZoneRenderer {
    /// Actors with the refraction pass (scheduler ops 0x48 / 0x49), in the given order (farthest first, retail's sorted
    /// list): per actor the scene so far is copied whole (FUN_10025870 into DAT_10456e6c), then its batches, its shine
    /// and the refraction pass (FUN_1002dc10 -> FUN_10025330) sampling that copy at the pixel's screen point shifted
    /// by (L P11, L P22) / w. Without a copyable target they draw plainly.
    pub fn render_refraction(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        color_tex: Option<&wgpu::Texture>,
        depth: &wgpu::TextureView,
        camera: &Camera,
        aspect: f32,
        actors: &[&ActorGpu],
    ) {
        self.render_refraction_at(gpu, encoder, color, color_tex, depth, camera, aspect, actors, 0);
    }

    /// render_refraction from uniform slot `base` on: one actor at a time among the effects, each its slot.
    pub fn render_refraction_at(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        color_tex: Option<&wgpu::Texture>,
        depth: &wgpu::TextureView,
        camera: &Camera,
        aspect: f32,
        actors: &[&ActorGpu],
        base: usize,
    ) {
        let done = gpu.mips_done();
        let list: Vec<&ActorGpu> = actors
            .iter()
            .copied()
            .filter(|a| {
                a.mips <= done && a.refract.is_some() && a.skin.as_ref().is_some_and(|s| s.frame == self.pose_frame)
            })
            .take(SHINE_SLOTS.saturating_sub(base))
            .collect();
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
        for (i, a) in list.iter().enumerate() {
            let slot = base + i;
            let (Some(rf), Some(skin)) = (a.refract, a.skin.as_ref()) else { continue };
            let off = (slot as u64 * SHINE_STRIDE) as u32;
            gpu.queue.put(
                &self.refract_buf,
                off as u64,
                bytemuck::cast_slice(&[rf.level, rf.r, a.alpha, 0.0, p00, p11, 0.0, 0.0]),
            );
            if let (Some(t), Some((c, _))) = (src, self.refract_copy.as_ref()) {
                encoder.copy_texture_to_texture(t.as_image_copy(), c.as_image_copy(), t.size());
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("actor refraction"),
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
            // Its shine in a slot from the top of the shine buffer (render's take the bottom).
            if let Some(s) = a.shine
                && let Some(tex) = self.shine_tex.get(&s.key)
            {
                let so = (SHINE_SLOTS - 1 - slot) as u64 * SHINE_STRIDE;
                gpu.queue.put(
                    &self.shine_buf,
                    so,
                    bytemuck::cast_slice(&[s.u, s.v, shine_wrap(), 0.0, s.tf[0], s.tf[1], s.tf[2], s.tf[3]]),
                );
                pass.set_pipeline(&self.shine_pipeline);
                pass.set_bind_group(3, self.shine_group(gpu, skin), &[so as u32]);
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
    /// `args`: the culled draws' indirect arguments (render/cull.rs), each run's at its slot.
    fn draw_scenery(
        &self,
        pass: &mut wgpu::RenderPass,
        d: &Draw,
        mode: u8,
        bound: &mut usize,
        pipe: &mut usize,
        culled: Option<(&cull::Cull, &wgpu::Buffer, &[bool])>,
    ) {
        self.draw_scenery_pipes(pass, d, mode, bound, pipe, culled, false)
    }

    /// `furn`: the furniture pipelines for a blended mode (depth writes on, no bias).
    fn draw_scenery_pipes(
        &self,
        pass: &mut wgpu::RenderPass,
        d: &Draw,
        mode: u8,
        bound: &mut usize,
        pipe: &mut usize,
        culled: Option<(&cull::Cull, &wgpu::Buffer, &[bool])>,
        furn: bool,
    ) {
        let furn = furn && mode >= 2;
        let runs = d.runs();
        let live = |k: usize| !runs[k].1.is_empty() && culled.is_none_or(|(c, _, shown)| shown[c.run_of(d.args[k])]);
        if !live(0) && !live(1) {
            return;
        }
        if d.texture != *bound {
            pass.set_bind_group(1, &self.textures[d.texture], &[]);
            *bound = d.texture;
        }
        let bump = d.bump.filter(|_| mode == 0).and_then(|b| self.bump_groups.get(b));
        if let Some(g) = bump {
            pass.set_bind_group(2, g, &[]);
        }
        for (k, (c, range)) in runs.clone().into_iter().enumerate() {
            if !live(k) {
                continue;
            }
            let want = if bump.is_some() {
                8 + c
            } else if furn {
                16 + c
            } else {
                c
            };
            if *pipe != want {
                let normal =
                    if furn { &self.scenery_furn[mode as usize - 2][c] } else { &self.scenery[mode as usize][c] };
                pass.set_pipeline(self.wire(c).unwrap_or(if bump.is_some() { &self.scenery_bump[c] } else { normal }));
                *pipe = want;
            }
            match culled {
                Some((_, a, _)) => pass.draw_indexed_indirect(a, d.args[k] as u64 * 20),
                None => pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, range),
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
            // COPY_SRC: the effects copy it to soften glows against the scenery (render/effects.rs).
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Renders several frames of one scene offscreen, one PNG each.
pub fn render_pngs(
    scene: &ZoneScene,
    frames: &[(&Camera, &std::path::Path)],
    width: u32,
    height: u32,
) -> Result<usize> {
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
pub type PostDraw<'a> = dyn FnMut(
        &Gpu,
        &mut wgpu::CommandEncoder,
        (&wgpu::TextureView, &wgpu::Texture),
        &wgpu::TextureView,
        &Camera,
        Option<&crate::dat::weather::Lighting>,
        &mut ZoneRenderer,
    ) + 'a;

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
    // FFXI_BENCH=N: the first view N more times after a warm-up, its time a frame printed (GPU-bound offscreen).
    if let (Some(n), Some((camera, _, lighting))) =
        (std::env::var("FFXI_BENCH").ok().and_then(|v| v.parse::<u32>().ok()), frames.first())
    {
        renderer.lighting = *lighting;
        // With timestamps, the GPU's time for render() too (one pair a frame, up to 2048 frames).
        let ts = gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let n = n.min(2048);
        let qs = ts.then(|| {
            gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("bench"),
                ty: wgpu::QueryType::Timestamp,
                count: n * 2,
            })
        });
        let run = |k: u32, timed: bool| -> Result<(f32, Option<f32>)> {
            let t = std::time::Instant::now();
            for f in 0..k {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                let q = qs.as_ref().filter(|_| timed);
                if let Some(q) = q {
                    encoder.write_timestamp(q, f * 2);
                }
                renderer.render(&gpu, &mut encoder, &view, &depth, camera, width as f32 / height as f32, &actor_refs);
                if let Some(q) = q {
                    encoder.write_timestamp(q, f * 2 + 1);
                }
                gpu.queue.submit([encoder.finish()]);
            }
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
            let wall = t.elapsed().as_secs_f32() * 1000.0 / k as f32;
            let Some(q) = qs.as_ref().filter(|_| timed) else { return Ok((wall, None)) };
            let size = k as u64 * 16;
            let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut e = gpu.device.create_command_encoder(&Default::default());
            e.resolve_query_set(q, 0..k * 2, &resolve, 0);
            e.copy_buffer_to_buffer(&resolve, 0, &read, 0, size);
            gpu.queue.submit([e.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
            let ticks: Vec<u64> =
                bytemuck::cast_slice(&read.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("{e:?}"))?)
                    .to_vec();
            let period = gpu.queue.get_timestamp_period() as f64;
            let total: f64 = ticks.chunks(2).map(|p| p[1].saturating_sub(p[0]) as f64 * period).sum();
            Ok((wall, Some((total / 1e6 / k as f64) as f32)))
        };
        run(20, false)?;
        let (wall, gpu_ms) = run(n, true)?;
        println!(
            "bench: {wall:.3} ms a frame over {n} frames at {width}x{height} ({} draws), gpu {}",
            renderer.draw_count(),
            gpu_ms.map_or("n/a".into(), |g| format!("{g:.3} ms"))
        );
        if let Some(c) = &renderer.cull {
            let (live, all, inst) = c.stats(&gpu);
            println!("cull: {live} of {all} indirect draws have an instance; {inst} instances drawn");
        }
    }
    for (camera, path, lighting) in frames {
        renderer.lighting = *lighting;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        renderer.update_leak(&gpu, camera, width as f32 / height as f32);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.render(&gpu, &mut encoder, &view, &depth, camera, width as f32 / height as f32, &actor_refs);
        renderer.render_refraction(
            &gpu,
            &mut encoder,
            &view,
            Some(&target),
            &depth,
            camera,
            width as f32 / height as f32,
            &actor_refs,
        );
        post(&gpu, &mut encoder, (&view, &target), &depth, camera, lighting.as_ref(), &mut renderer);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
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
        assert!(
            near(Refract::tf_alpha(0x40), 0.250_980)
                && near(Refract::tf_alpha(0x0b), 0.458_824)
                && near(Refract::tf_alpha(0xff), 0.505_882)
        );
    }
}

#[cfg(test)]
mod furniture_tests {
    /// A lamp the leak's light displaces moves to a free slot; one already there is not doubled.
    #[test]
    fn leak_keeps_lamps() {
        assert_eq!(super::with_lamps([9, 0, 0, 0], [0, 0, 0, 0], Some([4, 0, 0, 0])), [9, 4, 0, 0]);
        assert_eq!(super::with_lamps([0, 0, 0, 0], [0, 0, 0, 0], Some([4, 5, 0, 0])), [4, 5, 0, 0]);
        assert_eq!(super::with_lamps([4, 0, 0, 0], [0, 3, 0, 0], Some([4, 3, 0, 0])), [4, 0, 0, 0]);
        assert_eq!(super::with_lamps([1, 2, 3, 6], [0, 0, 0, 0], Some([4, 5, 7, 8])), [1, 2, 3, 6]);
    }

    /// FUN_101844c0: l_01 at table slot 0 is light 1; no l_01, no light.
    #[test]
    fn furniture_lights() {
        assert_eq!(super::furniture_lights(&[u32::from_le_bytes(*b"l_01")]), [1, 0, 0, 0]);
        assert_eq!(super::furniture_lights(&[]), [0; 4]);
    }
}

#[cfg(test)]
mod faded_tests {
    /// A centred box a quarter of the target across grows by a quarter of its size and 8 px; none is the whole target,
    /// and a rect off the target is skipped.
    #[test]
    fn faded_rect_grows_and_clamps() {
        assert_eq!(super::faded_rect(Some([-0.25, -0.25, 0.25, 0.25]), 800, 400), Some([242, 117, 316, 166]));
        assert_eq!(super::faded_rect(None, 800, 400), Some([0, 0, 800, 400]));
        assert_eq!(super::faded_rect(Some([-1.5, -0.2, 1.5, 0.2]), 800, 400).map(|r| (r[0], r[2])), Some((0, 800)));
        assert_eq!(super::faded_rect(Some([2.0, 0.0, 2.5, 0.5]), 800, 400), None);
    }

    /// The zone pipelines, the faded actors' offscreen and composite passes among them, build and validate (WGSL
    /// included) for the scene formats in use.
    #[test]
    fn zone_pipes_build() {
        let Ok(gpu) = super::Gpu::headless() else {
            eprintln!("no GPU: skipped");
            return;
        };
        for f in [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb] {
            let p = gpu.pipes.zone(f);
            assert_eq!(p.color_format, f);
        }
    }

    /// An empty zone builds its globals (the lamp shadow maps bound in them) and renders a frame with a lamp shadow
    /// set, without validation errors.
    #[test]
    fn zone_renders_with_lamp_shadows() {
        let Ok(gpu) = super::Gpu::headless() else { return };
        let scene = crate::dat::zone::ZoneScene {
            regions: vec![],
            collision: None,
            placements: vec![],
            meshes: vec![],
            textures: vec![],
            warnings: vec![],
            light_keys: vec![],
            paths: Default::default(),
            camera_lists: Default::default(),
            sway: Default::default(),
            bumps: vec![],
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = super::ZoneRenderer::new(&gpu, &scene, format);
        r.lamp_shadows = vec![(0, glam::Mat4::IDENTITY, glam::Vec3::ZERO)];
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = super::depth_view(&gpu.device, 8, 8);
        let camera = super::Camera {
            eye: glam::Vec3::new(0.0, -2.0, 5.0),
            target: glam::Vec3::ZERO,
            up: glam::Vec3::NEG_Y,
            fov_y: 1.0,
        };
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render(&gpu, &mut enc, &tex.create_view(&Default::default()), &depth, &camera, 1.0, &[]);
        gpu.queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "{err:?}");
    }
}

#[cfg(test)]
mod boost_tests {
    /// FUN_1000be50 at the default gamma (1.5 per channel): red x 1.3915, green x 1.3604, blue x 1.4527; at 1.0 the
    /// ramp is linear (2.3 x 120 / 255 ..).
    #[test]
    fn light_boost_factors() {
        let near = |a: [f32; 4], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5) && a[3] == 1.0;
        assert!(near(super::light_boost([1.5; 3]), [1.391_511, 1.360_417, 1.452_683]));
        assert!(near(super::light_boost([1.0; 3]), [1.082_353, 1.046_274, 1.154_510]));
    }

    /// FUN_10188e00 / FUN_1000be50: boosted only while every part is below 0.8, then capped at 1; the ambient's
    /// bytes after it (fx::elem::ambient_ref, zone.wgsl ambient_rs) are about half the env's.
    #[test]
    fn boosted_ambient() {
        use glam::Vec3;
        let b = super::light_boost([1.5; 3]);
        let a = super::boosted(Vec3::splat(0.5), b);
        assert!((a.x - 0.695_755).abs() < 1e-5 && (a.z - 0.726_341).abs() < 1e-5);
        assert_eq!(super::boosted(Vec3::new(0.5, 0.5, 0.8), b), Vec3::new(0.5, 0.5, 0.8));
        assert_eq!(super::boosted(Vec3::splat(0.75), b).max_element(), 1.0);
        assert_eq!(super::boosted(Vec3::splat(0.5), [0.0; 4]), Vec3::splat(0.5));
        // 0xa0 / 255 -> trunc(0.627 x 128) = 80 bytes: 80 / 255, not 160 / 255.
        assert_eq!(crate::fx::elem::ambient_ref(Vec3::splat(160.0 / 255.0)).x * 255.0, 80.0);
    }
}
