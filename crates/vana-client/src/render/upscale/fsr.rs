//! AMD FSR 3.1 upscaling (FidelityFX SDK 1.1.4, MIT; vendor/fidelityfx/LICENSE.txt), the first Upscaler. The
//! passes are AMD's GLSL turned into WGSL by vendor/fidelityfx/gen.py; this is the host side of
//! ffx_fsr3upscaler.cpp (resources, the ping-pong tables, the constants and the dispatches) on wgpu. One
//! permutation: LDR input, render-size motion vectors without jitter, standard depth, 32-bit floats.

use super::{Inputs, Upscaler};
use crate::render::{Gpu, Put};
use std::collections::HashMap;

const SRC: [(&str, &str); 10] = [
    ("prepare_inputs", include_str!("fsr/prepare_inputs.wgsl")),
    ("luma_pyramid", include_str!("fsr/luma_pyramid.wgsl")),
    ("shading_change_pyramid", include_str!("fsr/shading_change_pyramid.wgsl")),
    ("shading_change", include_str!("fsr/shading_change.wgsl")),
    ("prepare_reactivity", include_str!("fsr/prepare_reactivity.wgsl")),
    ("luma_instability", include_str!("fsr/luma_instability.wgsl")),
    ("accumulate", include_str!("fsr/accumulate.wgsl")),
    ("accumulate_sharpen", include_str!("fsr/accumulate_sharpen.wgsl")),
    ("rcas", include_str!("fsr/rcas.wgsl")),
    ("debug_view", include_str!("fsr/debug_view.wgsl")),
];

/// A binding a pass declares: its number, name (naga's trailing `_` dropped) and layout type.
struct Binding {
    binding: u32,
    name: String,
    ty: wgpu::BindingType,
}

struct Pass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
}

fn storage_format(s: &str) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    Some(match s {
        "r8unorm" => F::R8Unorm,
        "rgba8unorm" => F::Rgba8Unorm,
        "r16float" => F::R16Float,
        "rg16float" => F::Rg16Float,
        "rgba16float" => F::Rgba16Float,
        "r32float" => F::R32Float,
        "rgba32float" => F::Rgba32Float,
        "r32uint" => F::R32Uint,
        _ => return None,
    })
}

/// The bindings of a generated pass, read from its declarations (`@group(0) @binding(n)` then `var...`).
fn bindings(src: &str) -> Result<Vec<Binding>, String> {
    let mut out = Vec::new();
    let mut lines = src.lines().peekable();
    while let Some(l) = lines.next() {
        let Some(rest) = l.trim_start().strip_prefix("@group(0) @binding(") else { continue };
        let (n, rest) = rest.split_once(')').ok_or("binding")?;
        let binding: u32 = n.parse().map_err(|_| format!("binding {n}"))?;
        let decl = if rest.contains("var") { rest.to_string() } else { lines.next().unwrap_or_default().to_string() };
        let decl = decl.trim().trim_start_matches("@coherent").trim();
        let (head, ty) = decl.split_once(':').ok_or_else(|| format!("decl {decl}"))?;
        let ty = ty.trim().trim_end_matches(';').trim();
        let name = head.rsplit(' ').next().unwrap_or_default().trim_end_matches('_').to_string();
        let kind = if head.contains("var<uniform>") {
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            }
        } else if head.contains("var<storage") {
            let read_only = !head.contains("read_write");
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            }
        } else if ty == "sampler" {
            wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
        } else if let Some(t) = ty.strip_prefix("texture_storage_2d<") {
            let (fmt, access) = t.trim_end_matches('>').split_once(',').ok_or("storage texture")?;
            let format = storage_format(fmt.trim()).ok_or_else(|| format!("format {fmt}"))?;
            let access = match access.trim() {
                "write" => wgpu::StorageTextureAccess::WriteOnly,
                "read" => wgpu::StorageTextureAccess::ReadOnly,
                _ => wgpu::StorageTextureAccess::ReadWrite,
            };
            wgpu::BindingType::StorageTexture { access, format, view_dimension: wgpu::TextureViewDimension::D2 }
        } else if ty.starts_with("texture_2d<") {
            let sample_type = match ty {
                "texture_2d<u32>" => wgpu::TextureSampleType::Uint,
                "texture_2d<i32>" => wgpu::TextureSampleType::Sint,
                // The scene's depth (a depth format) can only be read unfiltered.
                _ => wgpu::TextureSampleType::Float { filterable: name != "r_input_depth" },
            };
            wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            }
        } else {
            return Err(format!("binding type {ty}"));
        };
        out.push(Binding { binding, name, ty: kind });
    }
    Ok(out)
}

fn make_pass(gpu: &Gpu, label: &str, src: &str) -> Result<Pass, String> {
    let bindings = bindings(src)?;
    let device = &gpu.device;
    let entries: Vec<_> = bindings
        .iter()
        .map(|b| wgpu::BindGroupLayoutEntry {
            binding: b.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: b.ty,
            count: None,
        })
        .collect();
    let layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    Ok(Pass { pipeline, layout, bindings })
}

fn texture(gpu: &Gpu, label: &str, size: [u32; 2], format: wgpu::TextureFormat, mips: u32) -> wgpu::Texture {
    use wgpu::TextureUsages as U;
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: U::STORAGE_BINDING | U::TEXTURE_BINDING | U::COPY_DST | U::COPY_SRC,
        view_formats: &[],
    })
}

fn buffer(gpu: &Gpu, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
}

/// FSR's resources at one render / output size (FFX's fsr3upscalerCreate, max size = size).
struct Resources {
    render: [u32; 2],
    output: [u32; 2],
    accumulation: [wgpu::Texture; 2],
    luma: [wgpu::Texture; 2],
    /// Farthest depth, then luma instability (FFX's INTERMEDIATE_FP16x1, aliased).
    intermediate: wgpu::Texture,
    shading_change: wgpu::Texture,
    new_locks: wgpu::Texture,
    internal_upscaled: [wgpu::Texture; 2],
    spd_mips: wgpu::Texture,
    farthest_depth_mip1: wgpu::Texture,
    luma_history: [wgpu::Texture; 2],
    dilated_reactive_masks: wgpu::Texture,
    dilated_depth: wgpu::Texture,
    dilated_motion_vectors: wgpu::Texture,
    frame_info: wgpu::Texture,
    default_exposure: wgpu::Texture,
    reconstructed_depth: wgpu::Buffer,
    spd_atomic: wgpu::Buffer,
    spd_mip5_mirror: wgpu::Buffer,
    output_tex: wgpu::Texture,
}

impl Resources {
    fn new(gpu: &Gpu, render: [u32; 2], output: [u32; 2]) -> Self {
        use wgpu::TextureFormat as F;
        let half = [render[0] / 2, render[1] / 2];
        let mips = 32 - half[0].max(half[1]).max(1).leading_zeros();
        let t = |l, s, f| texture(gpu, l, s, f, 1);
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Resources {
            render,
            output,
            accumulation: [t("fsr accumulation 1", render, F::R8Unorm), t("fsr accumulation 2", render, F::R8Unorm)],
            luma: [t("fsr luma 1", render, F::R16Float), t("fsr luma 2", render, F::R16Float)],
            intermediate: t("fsr intermediate", render, F::R16Float),
            shading_change: t("fsr shading change", half, F::R8Unorm),
            new_locks: t("fsr new locks", output, F::R8Unorm),
            internal_upscaled: [
                t("fsr upscaled 1", output, F::Rgba16Float),
                t("fsr upscaled 2", output, F::Rgba16Float),
            ],
            spd_mips: texture(gpu, "fsr spd mips", half, F::Rg16Float, mips),
            farthest_depth_mip1: t("fsr farthest depth mip1", half, F::R16Float),
            luma_history: [
                t("fsr luma history 1", render, F::Rgba8Unorm),
                t("fsr luma history 2", render, F::Rgba8Unorm),
            ],
            dilated_reactive_masks: t("fsr dilated reactive", render, F::Rgba8Unorm),
            dilated_depth: t("fsr dilated depth", render, F::R32Float),
            dilated_motion_vectors: t("fsr dilated motion", render, F::Rg16Float),
            frame_info: t("fsr frame info", [1, 1], F::Rgba32Float),
            default_exposure: t("fsr default exposure", [1, 1], F::Rg32Float),
            reconstructed_depth: buffer(
                gpu,
                "fsr reconstructed depth",
                4 * render[0] as u64 * render[1] as u64,
                storage,
            ),
            spd_atomic: buffer(gpu, "fsr spd atomic", 4, storage),
            spd_mip5_mirror: buffer(gpu, "fsr spd mip5", 64 * 64 * 8, storage),
            output_tex: texture(gpu, "fsr output", output, F::Rgba16Float, 1),
        }
    }
}

/// FFX's Fsr3UpscalerConstants, as the passes' cbFSR3Upscaler (160 bytes with the tail pad).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Constants {
    render_size: [i32; 2],
    previous_render_size: [i32; 2],
    upscale_size: [i32; 2],
    previous_upscale_size: [i32; 2],
    max_render_size: [i32; 2],
    max_upscale_size: [i32; 2],
    device_to_view_depth: [f32; 4],
    jitter: [f32; 2],
    previous_jitter: [f32; 2],
    motion_vector_scale: [f32; 2],
    downscale_factor: [f32; 2],
    motion_vector_jitter_cancellation: [f32; 2],
    tan_half_fov: f32,
    jitter_phase_count: f32,
    delta_time: f32,
    delta_pre_exposure: f32,
    view_space_to_meters: f32,
    frame_index: f32,
    velocity_factor: f32,
    reactiveness_scale: f32,
    shading_change_scale: f32,
    accumulation_added_per_frame: f32,
    min_disocclusion_accumulation: f32,
    pad: [f32; 3],
}

pub struct Fsr {
    passes: HashMap<&'static str, Pass>,
    res: Option<Resources>,
    cb: wgpu::Buffer,
    cb_spd: wgpu::Buffer,
    cb_rcas: wgpu::Buffer,
    linear: wgpu::Sampler,
    point: wgpu::Sampler,
    fill: wgpu::ComputePipeline,
    fill_uniform: wgpu::Buffer,
    constants: Constants,
    first: bool,
    /// FFX's resourceFrameIndex: its parity picks the ping-pong halves.
    frame_index: u32,
    pre_exposure: f32,
}

/// Fills a u32 buffer with one value (the reconstructed depth's 1.0 each frame: wgpu clears only to zero).
const FILL: &str = r#"
struct U { value: u32, count: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read_write> b: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 256u;
    if (i < u.count) { b[i] = u.value; }
}
"#;

impl Fsr {
    pub fn new(gpu: &Gpu) -> Result<Self, String> {
        super::supported(gpu)?;
        let mut passes = HashMap::new();
        for (name, src) in SRC {
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let pass = make_pass(gpu, name, src)?;
            if let Some(e) = pollster::block_on(scope.pop()) {
                let mut msg = format!("FSR pass {name}: {e:?}");
                let mut src: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                while let Some(c) = src {
                    msg.push_str(&format!(": {c}"));
                    src = c.source();
                }
                return Err(msg);
            }
            passes.insert(name, pass);
        }
        let ub = |l, n| buffer(gpu, l, n, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let sampler = |f| {
            gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: f,
                min_filter: f,
                mipmap_filter: if f == wgpu::FilterMode::Linear {
                    wgpu::MipmapFilterMode::Linear
                } else {
                    wgpu::MipmapFilterMode::Nearest
                },
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        let fill_module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fsr fill"),
            source: wgpu::ShaderSource::Wgsl(FILL.into()),
        });
        let fill = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fsr fill"),
            layout: None,
            module: &fill_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Fsr {
            passes,
            res: None,
            cb: ub("fsr constants", std::mem::size_of::<Constants>() as u64),
            cb_spd: ub("fsr spd constants", 32),
            cb_rcas: ub("fsr rcas constants", 16),
            linear: sampler(wgpu::FilterMode::Linear),
            point: sampler(wgpu::FilterMode::Nearest),
            fill,
            fill_uniform: ub("fsr fill", 16),
            constants: Constants::default(),
            first: true,
            frame_index: 0,
            pre_exposure: 0.0,
        })
    }

    /// FFX's setupDeviceDepthToViewSpaceDepthParams for standard, finite depth.
    fn depth_params(near: f32, far: f32, fov_y: f32, render: [u32; 2]) -> [f32; 4] {
        let (mn, mx) = (near.min(far), near.max(far));
        let q = mx / (mn - mx);
        let aspect = render[0] as f32 / render[1] as f32;
        let cot = (0.5 * fov_y).cos() / (0.5 * fov_y).sin();
        [-q, q * mn, aspect / cot, 1.0 / cot]
    }

    /// FsrRcasCon on FFX's remapped sharpness (0 none .. 1 most).
    fn rcas_constants(sharpness: f32) -> [u32; 4] {
        let stops = -2.0 * sharpness + 2.0;
        let s = (-stops).exp2();
        let h = f16_bits(s) as u32;
        [s.to_bits(), h | (h << 16), 0, 0]
    }
}

/// f32 to IEEE half bits, rounded (FFX's ffxPackHalf2x16 for the RCAS constants).
fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp <= 0 {
        return sign;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | ((((exp as u32) << 10) + ((man + 0x1000) >> 13)) as u16)
}

impl Upscaler for Fsr {
    fn name(&self) -> &'static str {
        "FSR 3.1"
    }

    fn dispatch(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, inp: &Inputs) -> Option<wgpu::TextureView> {
        let (render, output) = (inp.render, inp.output);
        if render[0] < 64 || render[1] < 64 {
            return None;
        }
        if self.res.as_ref().is_none_or(|r| r.render != render || r.output != output) {
            self.res = Some(Resources::new(gpu, render, output));
            self.first = true;
            self.constants = Constants::default();
        }
        let r = self.res.as_ref()?;
        let odd = self.frame_index & 1 != 0;
        let (cur, other) = if odd { (1usize, 0usize) } else { (0, 1) };
        // FFX: SRV = [odd ? 2 : 1], UAV = [odd ? 1 : 2]; current luma = [odd ? 2 : 1], previous the other.
        let (srv, uav) = (cur, other);
        let reset = inp.reset || self.first;

        let full = wgpu::ImageSubresourceRange::default();
        if self.first {
            for t in r.accumulation.iter().chain(&r.luma) {
                encoder.clear_texture(t, &full);
            }
            encoder.clear_texture(&r.default_exposure, &full);
        }
        self.first = false;

        let c = &mut self.constants;
        c.previous_jitter = c.jitter;
        c.jitter = inp.jitter;
        c.previous_render_size = c.render_size;
        c.render_size = [render[0] as i32, render[1] as i32];
        c.max_render_size = c.render_size;
        c.max_upscale_size = [output[0] as i32, output[1] as i32];
        let aspect = render[0] as f32 / render[1] as f32;
        let h_fov = ((inp.fov_y / 2.0).tan() * aspect).atan() * 2.0;
        c.tan_half_fov = (h_fov * 0.5).tan();
        c.view_space_to_meters = if inp.meters_per_unit > 0.0 { inp.meters_per_unit } else { 1.0 };
        c.device_to_view_depth = Self::depth_params(inp.near, inp.far, inp.fov_y, render);
        c.previous_upscale_size = c.upscale_size;
        c.upscale_size = c.max_upscale_size;
        c.downscale_factor = [render[0] as f32 / output[0] as f32, render[1] as f32 / output[1] as f32];
        let prev_exposure = self.pre_exposure;
        self.pre_exposure = if inp.exposure != 0.0 { inp.exposure } else { 1.0 };
        c.delta_pre_exposure = if prev_exposure > 0.0 { self.pre_exposure / prev_exposure } else { 1.0 };
        // Motion vectors in render pixels: FFX's scale over the render size.
        c.motion_vector_scale = [1.0 / render[0] as f32, 1.0 / render[1] as f32];
        let phases = super::phase_count(render[0], output[0]) as f32;
        if reset || c.jitter_phase_count == 0.0 {
            c.jitter_phase_count = phases;
        } else if phases > c.jitter_phase_count {
            c.jitter_phase_count += 1.0;
        } else if phases < c.jitter_phase_count {
            c.jitter_phase_count -= 1.0;
        }
        c.delta_time = (inp.dt_ms / 1000.0).clamp(0.0, 1.0);
        c.frame_index = if reset { 0.0 } else { c.frame_index + 1.0 };
        c.velocity_factor = 1.0;
        c.reactiveness_scale = 1.0;
        c.shading_change_scale = 1.0;
        c.accumulation_added_per_frame = 1.0 / 3.0;
        c.min_disocclusion_accumulation = -1.0 / 3.0;
        gpu.queue.put(&self.cb, 0, bytemuck::bytes_of(&self.constants));

        // ffxSpdSetup over the render rect.
        let groups = [(render[0] - 1) / 64 + 1, (render[1] - 1) / 64 + 1];
        let spd_mips = (render[0].max(render[1]) as f32).log2().floor().min(12.0) as u32;
        let spd = [spd_mips, groups[0] * groups[1], 0, 0, render[0], render[1], 0, 0];
        gpu.queue.put(&self.cb_spd, 0, bytemuck::cast_slice(&spd));
        gpu.queue.put(&self.cb_rcas, 0, bytemuck::cast_slice(&Self::rcas_constants(inp.sharpness)));

        if reset {
            encoder.clear_texture(&r.accumulation[srv], &full);
        }
        // Every frame: the SPD mips and counter, and the reconstructed depth at 1.0 (the far plane, min-stored).
        encoder.clear_texture(&r.spd_mips, &full);
        encoder.clear_buffer(&r.spd_atomic, 0, None);
        let count = render[0] * render[1];
        gpu.queue.put(&self.fill_uniform, 0, bytemuck::cast_slice(&[1.0f32.to_bits(), count, 0, 0]));
        let fill_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fsr fill"),
            layout: &self.fill.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.fill_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: r.reconstructed_depth.as_entire_binding() },
            ],
        });
        // The frame info's exposure track starts at (-1, 1) on a reset (FFX's Clear Frame Info).
        if reset {
            gpu.queue.write_texture(
                r.frame_info.as_image_copy(),
                bytemuck::cast_slice(&[-1.0f32, 1.0, 0.0, 0.0]),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16), rows_per_image: None },
                wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            );
        }

        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let mip = |k: u32| {
            r.spd_mips.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: k.min(r.spd_mips.mip_level_count() - 1),
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let depth = inp
            .depth
            .create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let mut views: HashMap<&str, wgpu::TextureView> = HashMap::new();
        views.insert("r_input_color_jittered", v(inp.color));
        views.insert("r_input_depth", depth);
        views.insert("r_input_motion_vectors", v(inp.motion));
        views.insert("r_reactive_mask", v(inp.reactive));
        views.insert("r_transparency_and_composition_mask", v(inp.composition));
        views.insert("r_input_exposure", v(&r.default_exposure));
        for n in ["r_dilated_motion_vectors", "rw_dilated_motion_vectors"] {
            views.insert(n, v(&r.dilated_motion_vectors));
        }
        for n in ["r_dilated_depth", "rw_dilated_depth"] {
            views.insert(n, v(&r.dilated_depth));
        }
        for n in ["r_farthest_depth", "rw_farthest_depth", "r_luma_instability", "rw_luma_instability"] {
            views.insert(n, v(&r.intermediate));
        }
        views.insert("r_current_luma", v(&r.luma[cur]));
        views.insert("rw_current_luma", v(&r.luma[cur]));
        views.insert("r_previous_luma", v(&r.luma[other]));
        views.insert("r_spd_mips", v(&r.spd_mips));
        for k in 0..6 {
            views.insert(
                ["rw_spd_mip0", "rw_spd_mip1", "rw_spd_mip2", "rw_spd_mip3", "rw_spd_mip4", "rw_spd_mip5"][k],
                mip(k as u32),
            );
        }
        for n in ["r_farthest_depth_mip1", "rw_farthest_depth_mip1"] {
            views.insert(n, v(&r.farthest_depth_mip1));
        }
        views.insert("rw_frame_info", v(&r.frame_info));
        views.insert("r_frame_info", v(&r.frame_info));
        for n in ["r_shading_change", "rw_shading_change"] {
            views.insert(n, v(&r.shading_change));
        }
        views.insert("r_accumulation", v(&r.accumulation[srv]));
        views.insert("rw_accumulation", v(&r.accumulation[uav]));
        for n in ["r_new_locks", "rw_new_locks"] {
            views.insert(n, v(&r.new_locks));
        }
        for n in ["r_dilated_reactive_masks", "rw_dilated_reactive_masks"] {
            views.insert(n, v(&r.dilated_reactive_masks));
        }
        views.insert("r_internal_upscaled_color", v(&r.internal_upscaled[srv]));
        views.insert("rw_internal_upscaled_color", v(&r.internal_upscaled[uav]));
        views.insert("r_rcas_input", v(&r.internal_upscaled[uav]));
        views.insert("r_luma_history", v(&r.luma_history[srv]));
        views.insert("rw_luma_history", v(&r.luma_history[uav]));
        let out_view = v(&r.output_tex);
        views.insert("rw_upscaled_output", out_view.clone());

        let buffers: HashMap<&str, &wgpu::Buffer> = HashMap::from([
            ("cbFSR3Upscaler", &self.cb),
            ("cbSPD", &self.cb_spd),
            ("cbRCAS", &self.cb_rcas),
            ("rw_reconstructed_previous_nearest_depth", &r.reconstructed_depth),
            ("r_reconstructed_previous_nearest_depth", &r.reconstructed_depth),
            ("rw_spd_global_atomic", &r.spd_atomic),
            ("rw_spd_mip5_mirror", &r.spd_mip5_mirror),
        ]);
        let group = |name: &str| -> Option<wgpu::BindGroup> {
            let p = self.passes.get(name)?;
            let mut entries = Vec::with_capacity(p.bindings.len());
            for b in &p.bindings {
                let resource = match b.ty {
                    wgpu::BindingType::Sampler(_) => wgpu::BindingResource::Sampler(if b.name == "s_PointClamp" {
                        &self.point
                    } else {
                        &self.linear
                    }),
                    wgpu::BindingType::Buffer { .. } => match buffers.get(b.name.as_str()) {
                        Some(buf) => buf.as_entire_binding(),
                        None => {
                            log::warn!("fsr {name}: no buffer {}", b.name);
                            return None;
                        }
                    },
                    _ => match views.get(b.name.as_str()) {
                        Some(t) => wgpu::BindingResource::TextureView(t),
                        None => {
                            log::warn!("fsr {name}: no texture {}", b.name);
                            return None;
                        }
                    },
                };
                entries.push(wgpu::BindGroupEntry { binding: b.binding, resource });
            }
            Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &p.layout,
                entries: &entries,
            }))
        };

        let src = [render[0].div_ceil(8), render[1].div_ceil(8)];
        let dst = [output[0].div_ceil(8), output[1].div_ceil(8)];
        let shading = [(render[0] / 2).div_ceil(8), (render[1] / 2).div_ceil(8)];
        let sharpen = inp.sharpness > 0.0;
        let mut order = vec![
            ("prepare_inputs", src),
            ("luma_pyramid", groups),
            ("shading_change_pyramid", groups),
            ("shading_change", shading),
            ("prepare_reactivity", src),
            ("luma_instability", src),
            (if sharpen { "accumulate_sharpen" } else { "accumulate" }, dst),
        ];
        if sharpen {
            order.push(("rcas", [output[0].div_ceil(16), output[1].div_ceil(16)]));
        }
        let mut groups_made = Vec::with_capacity(order.len());
        for (name, n) in &order {
            groups_made.push((self.passes.get(name)?, group(name)?, *n));
        }
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fsr 3.1"), timestamp_writes: None });
        pass.set_pipeline(&self.fill);
        pass.set_bind_group(0, &fill_group, &[]);
        pass.dispatch_workgroups(count.div_ceil(256).min(65535), count.div_ceil(256 * 65535).max(1), 1);
        for (p, g, n) in &groups_made {
            pass.set_pipeline(&p.pipeline);
            pass.set_bind_group(0, g, &[]);
            pass.dispatch_workgroups(n[0], n[1], 1);
        }
        drop(pass);
        self.frame_index = (self.frame_index + 1) % 16;
        Some(out_view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated pass declares only bindings this side knows how to lay out.
    #[test]
    fn pass_bindings_parse() {
        for (name, src) in SRC {
            let b = bindings(src).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!b.is_empty(), "{name}");
        }
        let acc = bindings(SRC[6].1).unwrap();
        assert!(acc.iter().any(|b| b.name == "rw_upscaled_output"));
        assert!(acc.iter().any(|b| b.name == "r_farthest_depth_mip1"));
    }

    /// FFX's RCAS constants: sharpness 1 is 0 stops (1.0), 0 is 2 stops (0.25).
    #[test]
    fn rcas() {
        assert_eq!(f32::from_bits(Fsr::rcas_constants(1.0)[0]), 1.0);
        assert_eq!(f32::from_bits(Fsr::rcas_constants(0.0)[0]), 0.25);
    }
}

#[cfg(test)]
mod gpu_tests {
    /// The passes build on this machine's GPU (skipped without one that can run them).
    #[test]
    fn passes_build() {
        let Ok(gpu) = crate::render::Gpu::headless() else { return };
        if crate::render::upscale::supported(&gpu).is_err() {
            return;
        }
        if let Err(e) = super::Fsr::new(&gpu) {
            panic!("{e}");
        }
    }

    /// A flat grey frame upscaled twice comes out the same grey, with no validation error on the way.
    #[test]
    fn flat_frame() {
        use super::super::{Inputs, Upscaler};
        let Ok(gpu) = crate::render::Gpu::headless() else { return };
        if crate::render::upscale::supported(&gpu).is_err() {
            return;
        }
        let mut fsr = super::Fsr::new(&gpu).unwrap();
        let (render, output) = ([128u32, 96], [192u32, 144]);
        let usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT;
        let tex = |f, s: [u32; 2]| super::texture(&gpu, "test", s, f, 1);
        let color = tex(wgpu::TextureFormat::Rgba16Float, render);
        let motion = tex(wgpu::TextureFormat::Rg16Float, render);
        let reactive = tex(wgpu::TextureFormat::R8Unorm, render);
        let composition = tex(wgpu::TextureFormat::R8Unorm, render);
        let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test depth"),
            size: wgpu::Extent3d { width: render[0], height: render[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage,
            view_formats: &[],
        });
        let grey: Vec<u16> = (0..render[0] * render[1] * 4).map(|_| 0x3800).collect();
        gpu.queue.write_texture(
            color.as_image_copy(),
            bytemuck::cast_slice(&grey),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(render[0] * 8), rows_per_image: None },
            color.size(),
        );
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut out = None;
        for frame in 0..3 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.create_view(&Default::default()),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.5), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            let inputs = Inputs {
                color: &color,
                depth: &depth,
                motion: &motion,
                reactive: &reactive,
                composition: &composition,
                render,
                output,
                jitter: crate::render::upscale::jitter(frame, 8),
                reset: frame == 0,
                dt_ms: 16.0,
                near: 0.1,
                far: 1000.0,
                fov_y: 1.0,
                reversed_z: false,
                meters_per_unit: 1.0,
                exposure: 1.0,
                sharpness: if frame == 2 { 0.5 } else { 0.0 },
            };
            out = fsr.dispatch(&gpu, &mut enc, &inputs);
            gpu.queue.submit([enc.finish()]);
        }
        if let Some(e) = pollster::block_on(scope.pop()) {
            panic!("{e:?}");
        }
        let view = out.expect("output");
        let tex = view.texture();
        let row = (output[0] * 8).next_multiple_of(256);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * output[1]) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            tex.size(),
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = buf.slice(..).get_mapped_range().unwrap();
        let px = |x: u32, y: u32| {
            let o = (y * row + x * 8) as usize;
            u16::from_le_bytes([data[o], data[o + 1]])
        };
        for (x, y) in [(0, 0), (96, 72), (191, 143)] {
            let v = crate::render::upscale::capture::f16_to_f32(px(x, y));
            assert!((v - 0.5).abs() < 0.02, "({x}, {y}) = {v}");
        }
    }
}

#[cfg(test)]
mod bench {
    /// FSR's GPU time by output size (run by hand: cargo test --release fsr_cost -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn fsr_cost() {
        use super::super::{Inputs, Upscaler};
        let Ok(gpu) = crate::render::Gpu::headless() else { return };
        for (output, ratio) in
            [([4096u32, 4096], 3.0f32), ([4096, 4096], 1.5), ([2560, 1440], 3.0), ([2560, 1440], 1.5)]
        {
            let render = [(output[0] as f32 / ratio) as u32, (output[1] as f32 / ratio) as u32];
            let mut fsr = super::Fsr::new(&gpu).unwrap();
            let tex = |f, s| super::texture(&gpu, "bench", s, f, 1);
            let (color, motion) =
                (tex(wgpu::TextureFormat::Rgba16Float, render), tex(wgpu::TextureFormat::Rg16Float, render));
            let (reactive, composition) =
                (tex(wgpu::TextureFormat::R8Unorm, render), tex(wgpu::TextureFormat::R8Unorm, render));
            let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: render[0], height: render[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let mut run = |n: u64| {
                let t = std::time::Instant::now();
                for frame in 0..n {
                    let mut enc = gpu.device.create_command_encoder(&Default::default());
                    let inputs = Inputs {
                        color: &color,
                        depth: &depth,
                        motion: &motion,
                        reactive: &reactive,
                        composition: &composition,
                        render,
                        output,
                        jitter: crate::render::upscale::jitter(frame, 8),
                        reset: frame == 0,
                        dt_ms: 16.0,
                        near: 0.1,
                        far: 1000.0,
                        fov_y: 1.0,
                        reversed_z: false,
                        meters_per_unit: 1.0,
                        exposure: 1.0,
                        sharpness: 0.5,
                    };
                    fsr.dispatch(&gpu, &mut enc, &inputs);
                    gpu.queue.submit([enc.finish()]);
                }
                let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                t.elapsed().as_secs_f64() * 1000.0 / n as f64
            };
            run(5);
            println!("output {}x{} render {}x{}: {:.2} ms/frame", output[0], output[1], render[0], render[1], run(60));
        }
    }
}
