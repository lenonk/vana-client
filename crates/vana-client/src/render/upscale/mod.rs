//! The scene's temporal upscaler (config `upscaler`, not retail). The world draws at a render size below the
//! background size with a Halton(2, 3) sub-pixel jitter; this turns the finished scene into the per-frame inputs a
//! temporal upscaler takes (linear colour, depth, motion vectors, reactive and composition masks, jitter, reset and
//! scalars), runs the upscaler, and hands the scaling pass its output at background size. The UI draws after, at
//! window size. Off, none of it runs and the scaling pass reads the scene as before.
//!
//! Upscalers: AMD FSR 3.1 (fsr.rs, FidelityFX SDK, MIT) and an external engine fed the same inputs through raw
//! Vulkan handles (external.rs). `capture` dumps one frame's inputs for offline work.

pub mod capture;
pub mod external;
pub mod fsr;
#[cfg(any(target_os = "linux", windows))]
pub mod vkdevice;

use super::{Gpu, Put};
use glam::{Mat4, Vec2, Vec3};

/// Whether neural rendering (config `neural`) can run: the device was made with the engine's features and the engine
/// library has vana_nr_dispatch; else why not. Set when the device is made.
static NEURAL: std::sync::Mutex<Option<Result<(), String>>> = std::sync::Mutex::new(None);

pub fn set_neural_status(s: Result<(), String>) {
    if let Ok(mut n) = NEURAL.lock() {
        *n = Some(s);
    }
}

pub fn neural_status() -> Result<(), String> {
    NEURAL.lock().ok().and_then(|n| n.clone()).unwrap_or_else(|| Err("no device yet".into()))
}

/// Output-to-render ratio by quality mode (config `upscale_quality`): Native AA, Quality, Balanced, Performance,
/// Ultra Performance (FSR 3.1's).
pub const RATIOS: [f32; 5] = [1.0, 1.5, 1.7, 2.0, 3.0];

/// Colour, motion and mask formats as the inputs are handed over.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// The moving actors' motion (rg, render pixels) where alpha is 1, drawn by the zone renderer.
pub const OBJECT_MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// What the zone renderer's actor motion pass draws into: the target, the steady matrices of this frame and the
/// last, the render size.
#[derive(Clone)]
pub struct MotionTarget {
    pub view: wgpu::TextureView,
    pub vp: Mat4,
    pub prev_vp: Mat4,
    pub size: [u32; 2],
}

/// FFXI's world units are metres (a Hume stands about 1.9 tall).
pub const METERS_PER_UNIT: f32 = 1.0;

/// The reactive mask: the largest channel change the effects made, times this. Composition: the blended scenery's.
const REACTIVE_SCALE: f32 = 1.5;
const COMPOSITION_SCALE: f32 = 4.0;

/// A camera move this far (units) or this much turn (cos of the angle) in one frame is a cut: the history resets.
const CUT_DISTANCE: f32 = 8.0;
const CUT_TURN: f32 = 0.87;

/// The render size for `output` at quality `quality` (FFX's GetRenderResolutionFromQualityMode: truncated).
pub fn render_size(output: [u32; 2], quality: u32) -> [u32; 2] {
    let r = RATIOS[quality.min(4) as usize];
    [((output[0] as f32 / r) as u32).max(1), ((output[1] as f32 / r) as u32).max(1)]
}

/// The jitter sequence length (FFX's GetJitterPhaseCount: 8 x the area ratio, truncated).
pub fn phase_count(render_w: u32, output_w: u32) -> u32 {
    ((8.0 * (output_w as f32 / render_w.max(1) as f32).powi(2)) as u32).max(1)
}

pub fn halton(mut index: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f32, 0.0f32);
    while index > 0 {
        f /= base as f32;
        r += f * (index % base) as f32;
        index /= base;
    }
    r
}

/// Frame `frame`'s jitter in render pixels, each axis in -0.5..0.5 (FFX's GetJitterOffset).
pub fn jitter(frame: u64, phases: u32) -> [f32; 2] {
    let i = (frame % phases.max(1) as u64) as u32 + 1;
    [halton(i, 2) - 0.5, halton(i, 3) - 0.5]
}

/// A render-pixel jitter as the projection's NDC offset (y up).
pub fn ndc_jitter(px: [f32; 2], render: [u32; 2]) -> Vec2 {
    Vec2::new(2.0 * px[0] / render[0].max(1) as f32, -2.0 * px[1] / render[1].max(1) as f32)
}

/// One frame's inputs, all at render size unless noted. Colour is linear (the scene's gamma 2.2 undone), motion is
/// previous minus current position in render pixels with the jitter taken out, depth is the scene's standard
/// (not reversed) 0..1 depth.
pub struct Inputs<'a> {
    pub color: &'a wgpu::Texture,
    pub depth: &'a wgpu::Texture,
    pub motion: &'a wgpu::Texture,
    pub reactive: &'a wgpu::Texture,
    pub composition: &'a wgpu::Texture,
    pub render: [u32; 2],
    /// The output (background) size.
    pub output: [u32; 2],
    /// Render pixels.
    pub jitter: [f32; 2],
    pub reset: bool,
    pub dt_ms: f32,
    pub near: f32,
    pub far: f32,
    pub fov_y: f32,
    pub reversed_z: bool,
    pub meters_per_unit: f32,
    pub exposure: f32,
    /// 0 none .. 1 most.
    pub sharpness: f32,
}

/// The upscale stage between the scene and the UI. `dispatch` records its work into `encoder` and returns its output
/// (linear, `COLOR_FORMAT`, at `inputs.output`), or None to fall back to the scaling pass alone.
pub trait Upscaler {
    fn name(&self) -> &'static str;
    fn dispatch(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, inputs: &Inputs)
    -> Option<wgpu::TextureView>;
}

/// Whether the device can run the inputs pass and FSR: the storage formats they write and read back.
pub fn supported(gpu: &Gpu) -> Result<(), String> {
    let need = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::FLOAT32_FILTERABLE
        | wgpu::Features::MEMORY_DECORATION_COHERENT
        | wgpu::Features::CLEAR_TEXTURE;
    if !gpu.device.features().contains(need) {
        return Err(format!("device lacks {:?}", need - gpu.device.features()));
    }
    if gpu.device.limits().max_storage_textures_per_shader_stage < 6 {
        return Err("fewer than 6 storage textures per stage".into());
    }
    use wgpu::TextureFormat as F;
    for f in [F::R8Unorm, F::Rg16Float, F::R16Float, F::Rgba8Unorm, F::Rgba16Float, F::R32Float, F::Rgba32Float] {
        let ff = gpu.adapter.get_texture_format_features(f);
        if !ff.allowed_usages.contains(wgpu::TextureUsages::STORAGE_BINDING)
            || !ff.flags.contains(wgpu::TextureFormatFeatureFlags::STORAGE_READ_WRITE)
        {
            return Err(format!("{f:?} has no read-write storage"));
        }
    }
    Ok(())
}

fn tex(
    gpu: &Gpu,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// The inputs at one render size.
pub struct Targets {
    pub size: [u32; 2],
    pub color: wgpu::Texture,
    pub motion: wgpu::Texture,
    pub reactive: wgpu::Texture,
    pub composition: wgpu::Texture,
    /// The scene after the opaque scenery, after the blended scenery, and before the effects (scene format).
    pub snap_opaque: wgpu::Texture,
    pub snap_blend: wgpu::Texture,
    pub snap_pre_fx: wgpu::Texture,
    /// A debug view's picture (`COLOR_FORMAT`).
    pub debug: wgpu::Texture,
    /// The actors' motion over the camera's (`OBJECT_MOTION_FORMAT`).
    pub object_motion: wgpu::Texture,
}

impl Targets {
    fn new(gpu: &Gpu, size: [u32; 2], scene: wgpu::TextureFormat) -> Self {
        use wgpu::TextureUsages as U;
        let io = U::STORAGE_BINDING | U::TEXTURE_BINDING | U::COPY_SRC;
        Targets {
            size,
            color: tex(gpu, "upscale colour", size, COLOR_FORMAT, io),
            // Render attachment: the moving actors' vectors are drawn over the camera's.
            motion: tex(gpu, "upscale motion", size, MOTION_FORMAT, io | U::RENDER_ATTACHMENT),
            reactive: tex(gpu, "upscale reactive", size, MASK_FORMAT, io),
            composition: tex(gpu, "upscale composition", size, MASK_FORMAT, io),
            snap_opaque: tex(gpu, "upscale snapshot", size, scene, U::COPY_DST | U::TEXTURE_BINDING),
            snap_blend: tex(gpu, "upscale snapshot", size, scene, U::COPY_DST | U::TEXTURE_BINDING),
            snap_pre_fx: tex(gpu, "upscale snapshot", size, scene, U::COPY_DST | U::TEXTURE_BINDING),
            debug: tex(gpu, "upscale debug", size, COLOR_FORMAT, U::STORAGE_BINDING | U::TEXTURE_BINDING),
            object_motion: tex(
                gpu,
                "upscale object motion",
                size,
                OBJECT_MOTION_FORMAT,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
            ),
        }
    }
}

/// Copies the scene colour into a snapshot (the world's passes call this between their passes).
pub fn snap(encoder: &mut wgpu::CommandEncoder, scene: &wgpu::Texture, dst: &wgpu::Texture) {
    if scene.size() == dst.size() && scene.format() == dst.format() {
        encoder.copy_texture_to_texture(scene.as_image_copy(), dst.as_image_copy(), dst.size());
    }
}

/// The scene camera of this frame, as the world drew it.
#[derive(Clone, Copy)]
pub struct FrameCamera {
    pub camera: super::Camera,
    pub aspect: f32,
}

/// The upscaler's frame state, held by the App across frames.
pub struct Upscale {
    pub targets: Option<Targets>,
    prep: wgpu::ComputePipeline,
    prep_uniform: wgpu::Buffer,
    debug: wgpu::ComputePipeline,
    debug_uniform: wgpu::Buffer,
    /// Frames since the last reset (the jitter index).
    frame: u64,
    /// The previous frame's steady view-projection, eye and forward.
    prev: Option<(Mat4, Vec3, Vec3)>,
    /// A reset asked for (zone change, resize, cut) for the next dispatch.
    reset: bool,
    /// The kind and size the upscaler was made for.
    kind: u32,
    engine: Option<Box<dyn Upscaler>>,
    /// The engine library (Upscaling: External, neural rendering), loaded on first use; Err logged once.
    lib: Option<Result<std::sync::Arc<external::Engine>, String>>,
    /// Neural rendering's output (render size).
    nr_out: Option<wgpu::Texture>,
    /// This frame's camera and jitter, from the world.
    pub frame_camera: Option<FrameCamera>,
    pub jitter_px: [f32; 2],
    pub render: [u32; 2],
    pub output: [u32; 2],
    /// A capture of the next frame's inputs into this folder (capture.rs).
    pub capture: Option<capture::Request>,
    pending_capture: Option<capture::Pending>,
    last: std::time::Instant,
    zone: Option<u16>,
    /// Logged once when the upscaler cannot run.
    pub failed: Option<String>,
}

impl Upscale {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = |label, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let pipe = |label, m: &wgpu::ShaderModule| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: m,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let prep = pipe("upscale inputs", &module("upscale inputs", include_str!("prep.wgsl")));
        let debug = pipe("upscale debug", &module("upscale debug", include_str!("debug.wgsl")));
        let buf = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Upscale {
            targets: None,
            prep,
            prep_uniform: buf("upscale inputs", 256),
            debug,
            debug_uniform: buf("upscale debug", 32),
            frame: 0,
            prev: None,
            reset: true,
            kind: 0,
            engine: None,
            lib: None,
            nr_out: None,
            frame_camera: None,
            jitter_px: [0.0; 2],
            render: [1, 1],
            output: [1, 1],
            capture: None,
            pending_capture: None,
            last: std::time::Instant::now(),
            zone: None,
            failed: None,
        }
    }

    /// Sets the frame up for output size `output` and returns the render size and the projection's NDC jitter.
    /// Without a temporal upscaler (`temporal` false: neural rendering alone) the render size is the output size and
    /// there is no jitter.
    pub fn begin(
        &mut self,
        gpu: &Gpu,
        output: [u32; 2],
        quality: u32,
        scene: wgpu::TextureFormat,
        temporal: bool,
    ) -> ([u32; 2], Vec2) {
        let render = if temporal { render_size(output, quality) } else { output };
        if self.targets.as_ref().is_none_or(|t| t.size != render) || self.output != output {
            self.targets = Some(Targets::new(gpu, render, scene));
            self.reset = true;
        }
        self.render = render;
        self.output = output;
        if self.reset {
            self.frame = 0;
        }
        self.jitter_px = if temporal { jitter(self.frame, phase_count(render[0], output[0])) } else { [0.0; 2] };
        self.frame_camera = None;
        (render, ndc_jitter(self.jitter_px, render))
    }

    /// The actor motion pass's target with last frame's matrix (NaN after a reset: the world then sets it to this
    /// frame's, as it sets `vp`).
    pub fn motion_target(&self) -> Option<MotionTarget> {
        let t = self.targets.as_ref()?;
        let prev = if self.reset { None } else { self.prev.map(|p| p.0) };
        Some(MotionTarget {
            view: t.object_motion.create_view(&Default::default()),
            vp: Mat4::IDENTITY,
            prev_vp: prev.unwrap_or(Mat4::NAN),
            size: t.size,
        })
    }

    /// False once upscaler `kind` failed to start: the scene then draws at full size again.
    pub fn usable(&self, kind: u32) -> bool {
        self.kind != kind || self.failed.is_none()
    }

    /// A zone change starts the history over.
    pub fn zone(&mut self, zone: Option<u16>) {
        if self.zone != zone {
            self.zone = zone;
            self.reset = true;
        }
    }

    /// The engine library, loaded once.
    fn lib(&mut self, gpu: &Gpu) -> Option<std::sync::Arc<external::Engine>> {
        let lib = self.lib.get_or_insert_with(|| {
            external::Engine::load(gpu).map(std::sync::Arc::new).inspect_err(|e| log::warn!("upscale engine: {e}"))
        });
        lib.as_ref().ok().cloned()
    }

    /// The upscaler for config `kind` (1 FSR, 2 external), made when it changes.
    fn engine(&mut self, gpu: &Gpu, kind: u32) -> Option<&mut Box<dyn Upscaler>> {
        if self.kind != kind {
            self.failed = None;
        }
        // A failed upscaler is not tried again until the setting changes.
        if self.kind != kind || (self.engine.is_none() && self.failed.is_none()) {
            self.kind = kind;
            self.reset = true;
            self.engine = match kind {
                1 => fsr::Fsr::new(gpu).map(|f| Box::new(f) as Box<dyn Upscaler>),
                2 => match self.lib(gpu) {
                    Some(lib) => external::External::new(lib).map(|e| Box::new(e) as Box<dyn Upscaler>),
                    None => Err("no engine library".into()),
                },
                _ => Err("off".into()),
            }
            .inspect(|e| log::info!("upscaler: {}", e.name()))
            .map_err(|e| {
                if self.failed.as_deref() != Some(e.as_str()) {
                    log::warn!("upscaler {kind}: {e}");
                }
                self.failed = Some(e);
            })
            .ok();
        }
        self.engine.as_mut()
    }

    /// The inputs made from the finished scene, the upscaler run, and its output view (linear) with its size (the
    /// output size). None: the scaling pass reads the scene itself. `debug` (config `upscale_debug`) shows an
    /// input instead, at render size. `neural` (config `neural`): the engine's neural rendering on the colour first,
    /// its output what the upscaler (or, with Upscaling off, the scaling pass) reads.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        kind: u32,
        sharpness: u32,
        debug: u32,
        neural: bool,
    ) -> Option<(wgpu::TextureView, [u32; 2])> {
        let depth_tex = depth.texture();
        let fc = self.frame_camera?;
        // The upscaler is made first: one that cannot run leaves the frame to the scaling pass.
        let mut engine = if debug == 0 && kind != 0 {
            self.engine(gpu, kind)?;
            Some(self.engine.take()?)
        } else {
            None
        };
        let nr = if neural && debug == 0 { self.lib(gpu).filter(|l| l.nr.is_some()) } else { None };
        let mut nr_out = self.nr_out.take();
        let now = std::time::Instant::now();
        let dt_ms = (now - self.last).as_secs_f32() * 1000.0;
        self.last = now;
        let cam = fc.camera;
        let steady = cam.steady();
        let vp = steady.view_proj(fc.aspect);
        let jvp = cam.view_proj(fc.aspect);
        let fwd = (cam.target - cam.eye).normalize_or_zero();
        if let Some((_, eye, f)) = self.prev
            && (eye.distance(cam.eye) > CUT_DISTANCE || f.dot(fwd) < CUT_TURN)
        {
            self.reset = true;
        }
        let reset = std::mem::take(&mut self.reset) || self.prev.is_none();
        let prev_vp = if reset { vp } else { self.prev.map_or(vp, |p| p.0) };
        self.prev = Some((vp, cam.eye, fwd));
        let t = self.targets.as_ref()?;
        let size = t.size;
        let mut u = Vec::with_capacity(64);
        u.extend_from_slice(&jvp.inverse().to_cols_array());
        u.extend_from_slice(&vp.to_cols_array());
        u.extend_from_slice(&prev_vp.to_cols_array());
        u.extend_from_slice(&[size[0] as f32, size[1] as f32, REACTIVE_SCALE, COMPOSITION_SCALE]);
        gpu.queue.put(&self.prep_uniform, 0, bytemuck::cast_slice(&u));
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("upscale inputs"),
            layout: &self.prep.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.prep_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view(&t.snap_opaque)),
                },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&view(&t.snap_blend)) },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&view(&t.snap_pre_fx)),
                },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&view(&t.color)) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&view(&t.motion)) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&view(&t.reactive)) },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&view(&t.composition)),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&view(&t.object_motion)),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("upscale inputs"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.prep);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
        let inputs = Inputs {
            color: &t.color,
            depth: depth_tex,
            motion: &t.motion,
            reactive: &t.reactive,
            composition: &t.composition,
            render: size,
            output: self.output,
            jitter: self.jitter_px,
            reset,
            dt_ms,
            near: super::NEAR,
            far: super::FAR,
            fov_y: cam.fov_y,
            reversed_z: false,
            meters_per_unit: METERS_PER_UNIT,
            exposure: 1.0,
            sharpness: sharpness.min(100) as f32 / 100.0,
        };
        if let Some(req) = self.capture.take() {
            self.pending_capture = capture::record(gpu, encoder, &inputs, req);
        }
        self.frame += 1;
        // Neural rendering at the render size; a frame it declines keeps the colour as it was.
        let mut neural_done = false;
        if let Some(lib) = &nr
            && let Some(f) = lib.nr
        {
            if nr_out.as_ref().is_none_or(|o| [o.width(), o.height()] != size) {
                nr_out = Some(external::output_texture(gpu, "neural rendering output", size));
            }
            if let Some(o) = &nr_out {
                neural_done = lib.run(gpu, encoder, &Inputs { output: size, ..inputs }, o, f);
            }
        }
        let inputs = match (&nr_out, neural_done) {
            (Some(o), true) => Inputs { color: o, ..inputs },
            _ => inputs,
        };
        let out = match engine.as_mut() {
            Some(e) => e.dispatch(gpu, encoder, &inputs).map(|v| (v, self.output)),
            None if debug != 0 => Some((self.debug_view(gpu, encoder, depth, debug), size)),
            None => neural_done.then(|| (inputs.color.create_view(&Default::default()), size)),
        };
        if engine.is_some() {
            self.engine = engine;
        }
        self.nr_out = nr_out;
        out
    }

    fn debug_view(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        mode: u32,
    ) -> wgpu::TextureView {
        let t = self.targets.as_ref().expect("targets");
        let size = t.size;
        let u = [size[0] as f32, size[1] as f32, mode.min(4) as f32, super::NEAR, super::FAR, 0.05, 500.0, 0.0];
        gpu.queue.put(&self.debug_uniform, 0, bytemuck::cast_slice(&u));
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let out = view(&t.debug);
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("upscale debug"),
            layout: &self.debug.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.debug_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view(&t.motion)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view(&t.reactive)) },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&view(&t.composition)),
                },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&out) },
            ],
        });
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("upscale debug"), timestamp_writes: None });
        pass.set_pipeline(&self.debug);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        drop(pass);
        out
    }

    /// After the frame's submit: a capture recorded this frame is read back and written out.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        if let Some(p) = self.pending_capture.take() {
            self.capture = capture::finish(gpu, p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_sequence() {
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(2, 2), 0.25);
        assert_eq!(halton(3, 2), 0.75);
        assert!((halton(1, 3) - 1.0 / 3.0).abs() < 1e-6);
        assert!((halton(2, 3) - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn sizes_and_phases() {
        assert_eq!(render_size([3840, 2160], 3), [1920, 1080]);
        assert_eq!(render_size([2560, 1440], 1), [1706, 960]);
        assert_eq!(phase_count(1920, 3840), 32);
        assert_eq!(phase_count(1706, 2560), 18);
        assert_eq!(phase_count(100, 100), 8);
    }

    /// The camera's motion vectors: a camera stepping right past a plane at distance 10 moves every pixel by the
    /// pinhole shift, previous minus current, whatever the jitter.
    #[test]
    fn camera_motion_vectors() {
        let Ok(gpu) = crate::render::Gpu::headless() else { return };
        if supported(&gpu).is_err() {
            return;
        }
        let mut up = Upscale::new(&gpu);
        let fmt = wgpu::TextureFormat::Rgba8Unorm;
        let (render, _) = up.begin(&gpu, [256, 144], 1, fmt, true);
        use wgpu::TextureUsages as U;
        let scene = tex(&gpu, "scene", render, fmt, U::TEXTURE_BINDING | U::COPY_SRC | U::RENDER_ATTACHMENT);
        let depth = tex(&gpu, "depth", render, crate::render::DEPTH_FORMAT, U::TEXTURE_BINDING | U::RENDER_ATTACHMENT);
        let (n, f, z) = (crate::render::NEAR, crate::render::FAR, 10.0f32);
        let d = f / (f - n) * (1.0 - n / z);
        let fov = 1.0f32;
        let aspect = render[0] as f32 / render[1] as f32;
        let mut out = Vec::new();
        for (k, x) in [0.0f32, 0.1].into_iter().enumerate() {
            let (_, jit) = up.begin(&gpu, [256, 144], 1, fmt, true);
            let cam = crate::render::Camera {
                eye: Vec3::new(x, 0.0, 0.0),
                target: Vec3::new(x, 0.0, -1.0),
                up: Vec3::Y,
                fov_y: fov,
                jitter: jit,
            };
            up.frame_camera = Some(FrameCamera { camera: cam, aspect });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.create_view(&Default::default()),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(d), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            let r = up.run(
                &gpu,
                &mut enc,
                &scene.create_view(&Default::default()),
                &depth.create_view(&Default::default()),
                0,
                0,
                1,
                false,
            );
            assert!(r.is_some(), "frame {k}");
            gpu.queue.submit([enc.finish()]);
            if k == 1 {
                out = read_motion(&gpu, &up.targets.as_ref().unwrap().motion);
            }
        }
        let shift = 0.1 * (render[1] as f32 / 2.0) / (fov / 2.0).tan() / z;
        for (x, y) in [(10usize, 10usize), (85, 48), (160, 90)] {
            let [mx, my] = out[y * render[0] as usize + x];
            assert!((mx - shift).abs() < 0.05 && my.abs() < 0.05, "({x}, {y}): ({mx}, {my}), want ({shift}, 0)");
        }
    }

    fn read_motion(gpu: &Gpu, t: &wgpu::Texture) -> Vec<[f32; 2]> {
        let (w, h) = (t.width(), t.height());
        let row = (w * 4).next_multiple_of(256);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            t.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            t.size(),
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = buf.slice(..).get_mapped_range().unwrap();
        let half = |o: usize| capture::f16_to_f32(u16::from_le_bytes([data[o], data[o + 1]]));
        (0..h as usize)
            .flat_map(|y| (0..w as usize).map(move |x| (y, x)))
            .map(|(y, x)| {
                let o = y * row as usize + x * 4;
                [half(o), half(o + 2)]
            })
            .collect()
    }

    /// The jitter moves a point by the NDC offset it names.
    #[test]
    fn projection_jitter() {
        let cam = crate::render::Camera {
            eye: Vec3::new(0.0, 0.0, 5.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y: 1.0,
            jitter: Vec2::new(0.01, -0.02),
        };
        let p = glam::Vec4::new(0.3, 0.2, 0.0, 1.0);
        let a = cam.view_proj(1.5) * p;
        let b = cam.steady().view_proj(1.5) * p;
        let d = a.truncate().truncate() / a.w - b.truncate().truncate() / b.w;
        assert!((d - cam.jitter).length() < 1e-5, "{d}");
    }
}
