//! Effect elements on the GPU: the per-frame update pass (render/effects.wgsl `update`) over the element
//! buffer, then each live element drawn back to front with its blend mode (FUN_10045950) and its mesh or sprite.
//! Sort keys and expire requests are read back a frame or two late (fx::Effects::take_back).

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
}

/// Instances are element x PER_ELEM + (particle, or the shading variant for meshes).
const PER_ELEM: u32 = 512;

/// The camera matrices of FUN_100691d0 from the view V (model_draw.md 5), as column-major glam matrices: b90 =
/// rotation only; d50 = world x / z kept as unit axes, V's image of world y kept; d10 = yaw only with world y as
/// view y; c10 = d10 · d50.
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

struct GpuMesh {
    vb: wgpu::Buffer,
    groups: Vec<(Option<(usize, usize)>, std::ops::Range<u32>)>,
    mmb: bool,
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
    draw_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    mesh_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    sprite_pipes: HashMap<usize, wgpu::RenderPipeline>,
    morph_pipes: HashMap<(usize, bool), wgpu::RenderPipeline>,
    /// CMoDist: SRCALPHA / INVSRCALPHA, z write by flag 12.
    dist_pipes: HashMap<bool, wgpu::RenderPipeline>,
    /// The scene copy CMoDist samples (MIRROR, linear), remade on a size or format change.
    scene_copy: Option<(wgpu::Texture, wgpu::BindGroup)>,
    copy_sampler: wgpu::Sampler,
    /// Lens flares and their glare: additive, no depth test (FUN_1003ddb0, FUN_100125c0).
    flare_pipe: wgpu::RenderPipeline,
    glare_pipe: wgpu::RenderPipeline,
    flare_vis: wgpu::ComputePipeline,
    flare_layout: wgpu::BindGroupLayout,
    flare_u: wgpu::Buffer,
    params: wgpu::Buffer,
    draw_u: wgpu::Buffer,
    elems: wgpu::Buffer,
    outs: wgpu::Buffer,
    back: wgpu::Buffer,
    parts: wgpu::Buffer,
    staging: Vec<Staging>,
    ops: (wgpu::Buffer, usize),
    keys: (wgpu::Buffer, usize),
    key_desc: (wgpu::Buffer, usize),
    paths: (wgpu::Buffer, usize),
    slots: (wgpu::Buffer, usize),
    sverts: (wgpu::Buffer, usize),
    sframes: (wgpu::Buffer, usize),
    mdata: (wgpu::Buffer, usize),
    update_group: Option<wgpu::BindGroup>,
    draw_group: Option<wgpu::BindGroup>,
    meshes: Vec<Option<GpuMesh>>,
    textures: HashMap<(usize, usize), wgpu::BindGroup>,
    white: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    count: u32,
    linearize: bool,
}

const STAGES: usize = 3;

impl EffectsGpu {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let consts = crate::fx::elem::wgsl_consts();
        let src = include_str!("effects.wgsl");
        let (compute_src, draw_src) = src.split_once("// ---- Drawing ----").unwrap();
        let cmod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("effects update"), source: wgpu::ShaderSource::Wgsl(format!("{consts}{compute_src}").into()) });
        let dmod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("effects draw"), source: wgpu::ShaderSource::Wgsl(format!("{consts}{draw_src}").into()) });
        let storage = |binding, read_only, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let uniform = |binding, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let cs = wgpu::ShaderStages::COMPUTE;
        let update_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects update"),
            entries: &[uniform(0, cs), storage(1, false, cs), storage(2, true, cs), storage(3, true, cs), storage(4, true, cs), storage(5, true, cs), storage(6, false, cs), storage(7, false, cs), storage(8, true, cs)],
        });
        let update = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects update"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&update_layout)], immediate_size: 0 })),
            module: &cmod,
            entry_point: Some("update"),
            compilation_options: Default::default(),
            cache: None,
        });
        let vs = wgpu::ShaderStages::VERTEX;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects draw"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT), storage(1, true, vs), storage(2, true, vs), storage(3, true, vs), storage(4, true, vs), storage(5, true, vs), storage(6, true, vs)],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("effects"), bind_group_layouts: &[Some(&draw_layout), Some(&tex_layout)], immediate_size: 0 });
        let vbuf = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::fx::lib::MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2],
        })];
        let pipe_z = |vs_entry: &str, buffers: &[Option<wgpu::VertexBufferLayout>], class: usize, zwrite: bool, compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("effects"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &dmod, entry_point: Some(vs_entry), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(zwrite),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &dmod,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend_state(class)), write_mask: wgpu::ColorWrites::COLOR })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe = |vs_entry: &str, buffers: &[Option<wgpu::VertexBufferLayout>], class: usize, zwrite: bool| pipe_z(vs_entry, buffers, class, zwrite, wgpu::CompareFunction::LessEqual);
        let flare_pipe = pipe_z("vs_flare", &[], 2, false, wgpu::CompareFunction::Always);
        let glare_pipe = pipe_z("vs_glare", &[], 2, false, wgpu::CompareFunction::Always);
        let fmod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("effects flare"), source: wgpu::ShaderSource::Wgsl(format!("{consts}{}", include_str!("flare.wgsl")).into()) });
        let flare_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects flare"),
            entries: &[
                uniform(0, cs),
                storage(1, true, cs),
                storage(2, true, cs),
                storage(3, false, cs),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: cs, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            ],
        });
        let flare_vis = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects flare"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&flare_layout)], immediate_size: 0 })),
            module: &fmod,
            entry_point: Some("flare_vis"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut mesh_pipes = HashMap::new();
        let mut sprite_pipes = HashMap::new();
        let mut morph_pipes = HashMap::new();
        let dist_pipes: HashMap<bool, wgpu::RenderPipeline> = [false, true].into_iter().map(|z| (z, pipe("vs_dist", &[], 1, z))).collect();
        for class in 0..6 {
            for zwrite in [false, true] {
                mesh_pipes.insert((class, zwrite), pipe("vs_mesh", &vbuf, class, zwrite));
                morph_pipes.insert((class, zwrite), pipe("vs_morph", &[], class, zwrite));
            }
            // D3a never writes z (FUN_1003db60).
            sprite_pipes.insert(class, pipe("vs_sprite", &[], class, false));
        }
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false });
        use wgpu::BufferUsages as U;
        let n = MAX_ELEMS as u64;
        let params = buf("effects params", std::mem::size_of::<Params>() as u64, U::UNIFORM | U::COPY_DST);
        let draw_u = buf("effects draw", std::mem::size_of::<DrawU>() as u64, U::UNIFORM | U::COPY_DST);
        let flare_u = buf("effects flare", std::mem::size_of::<FlareU>() as u64, U::UNIFORM | U::COPY_DST);
        let elems = buf("effects elements", n * w::WORDS as u64 * 4, U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let outs = buf("effects outs", n * OUT_WORDS as u64 * 4, U::STORAGE);
        let back = buf("effects back", n * BACK_WORDS as u64 * 4, U::STORAGE | U::COPY_SRC);
        let parts = buf("effects particles", MAX_PARTS as u64 * PART_WORDS as u64 * 4, U::STORAGE | U::COPY_DST);
        let staging = (0..STAGES).map(|_| Staging { buf: buf("effects readback", n * BACK_WORDS as u64 * 4, U::MAP_READ | U::COPY_DST), state: 0, ready: Arc::new(AtomicBool::new(false)), words: 0 }).collect();
        let sampler = super::mip_sampler(device);
        let copy_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects scene copy"),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white_view = super::upload_texture(gpu, &crate::dat::zone::Texture::new(String::new(), 0, 1, 1, vec![255; 4]), false);
        let white = Self::tex_group(device, &tex_layout, &sampler, &white_view);
        let table = |label: &str| (buf(label, 1024, U::STORAGE | U::COPY_DST), 1024usize);
        Self {
            update,
            update_layout,
            draw_layout,
            tex_layout,
            mesh_pipes,
            sprite_pipes,
            morph_pipes,
            dist_pipes,
            scene_copy: None,
            copy_sampler,
            flare_pipe,
            glare_pipe,
            flare_vis,
            flare_layout,
            flare_u,
            params,
            draw_u,
            elems,
            outs,
            back,
            parts,
            staging,
            ops: table("effects ops"),
            keys: table("effects keys"),
            key_desc: table("effects key table"),
            paths: table("effects paths"),
            slots: table("effects slots"),
            sverts: table("effects sprite vertices"),
            sframes: table("effects sprite frames"),
            mdata: table("effects morph data"),
            update_group: None,
            draw_group: None,
            meshes: Vec::new(),
            textures: HashMap::new(),
            white,
            sampler,
            count: 0,
            linearize: format.is_srgb(),
        }
    }

    fn tex_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }],
        })
    }

    /// Grows a table buffer to hold `bytes`; true when it was replaced (the bind groups must be rebuilt).
    fn fit(gpu: &Gpu, t: &mut (wgpu::Buffer, usize), bytes: usize, label: &str) -> bool {
        if bytes <= t.1 {
            return false;
        }
        let size = bytes.next_power_of_two();
        t.0 = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
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

    /// Uploads this frame's element writes, tables and new meshes, then records the update pass and the copy
    /// of its readback.
    pub fn update(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, fx: &mut Effects, f: &Frame) {
        let q = &gpu.queue;
        let reg = &mut fx.reg;
        let mut rebind = self.update_group.is_none();
        let mut redraw = self.draw_group.is_none();
        if reg.ops_dirty {
            rebind |= Self::fit(gpu, &mut self.ops, reg.ops.len() * 4, "effects ops");
            q.write_buffer(&self.ops.0, 0, bytemuck::cast_slice(&reg.ops));
            reg.ops_dirty = false;
        }
        if reg.keys_dirty {
            rebind |= Self::fit(gpu, &mut self.keys, reg.key_data.len() * 8, "effects keys");
            rebind |= Self::fit(gpu, &mut self.key_desc, reg.key_desc.len() * 8, "effects key table");
            q.write_buffer(&self.keys.0, 0, bytemuck::cast_slice(&reg.key_data));
            q.write_buffer(&self.key_desc.0, 0, bytemuck::cast_slice(&reg.key_desc));
            reg.keys_dirty = false;
        }
        if reg.paths_dirty {
            rebind |= Self::fit(gpu, &mut self.paths, reg.path_data.len() * 16, "effects paths");
            q.write_buffer(&self.paths.0, 0, bytemuck::cast_slice(&reg.path_data));
            reg.paths_dirty = false;
        }
        if reg.sprites_dirty {
            redraw |= Self::fit(gpu, &mut self.sverts, reg.sprite_verts.len() * 36, "effects sprite vertices");
            redraw |= Self::fit(gpu, &mut self.sframes, reg.sprite_frame_table.len() * 8, "effects sprite frames");
            q.write_buffer(&self.sverts.0, 0, bytemuck::cast_slice(&reg.sprite_verts));
            q.write_buffer(&self.sframes.0, 0, bytemuck::cast_slice(&reg.sprite_frame_table));
            reg.sprites_dirty = false;
        }
        if reg.morphs_dirty {
            redraw |= Self::fit(gpu, &mut self.mdata, reg.morph_data.len() * 4, "effects morph data");
            q.write_buffer(&self.mdata.0, 0, bytemuck::cast_slice(&reg.morph_data));
            reg.morphs_dirty = false;
        }
        rebind |= Self::fit(gpu, &mut self.slots, fx.slots.len() * SLOT_WORDS * 4, "effects slots");
        if !fx.slots.is_empty() {
            q.write_buffer(&self.slots.0, 0, bytemuck::cast_slice(&fx.slots));
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
                Write::Full(e, el) => q.write_buffer(&self.elems, (e as usize * w::WORDS * 4) as u64, bytemuck::cast_slice(&el.0)),
                Write::Words(e, off, words) => q.write_buffer(&self.elems, ((e as usize * w::WORDS + off) * 4) as u64, bytemuck::cast_slice(&words)),
                Write::Parts(first, recs) => q.write_buffer(&self.parts, (first as usize * PART_WORDS * 4) as u64, bytemuck::cast_slice(&recs)),
            }
        }
        if rebind {
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
                ],
            }));
        }
        if redraw {
            self.draw_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effects draw"),
                layout: &self.draw_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.draw_u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.outs.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.elems.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.parts.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.sverts.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: self.sframes.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: self.mdata.0.as_entire_binding() },
                ],
            }));
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
            extra: [f.moon_phase as f32, f.weekday as f32, 0.0, 0.0],
            area: [f.area_light.x, f.area_light.y, f.area_light.z, 0.0],
        };
        q.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
        if self.count == 0 {
            return;
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("effects update"), timestamp_writes: None });
            pass.set_pipeline(&self.update);
            pass.set_bind_group(0, self.update_group.as_ref().unwrap(), &[]);
            pass.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
        }
        if let Some(s) = self.staging.iter_mut().find(|s| s.state == 0) {
            let bytes = (self.count as usize * BACK_WORDS * 4) as u64;
            encoder.copy_buffer_to_buffer(&self.back, 0, &s.buf, 0, bytes);
            s.state = 1;
            s.words = self.count as usize * BACK_WORDS;
        }
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
            let view = super::upload_texture(gpu, &fx.reg.libs[t.0].textures[t.1], false);
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
        let vb = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("effect mesh"), contents: bytemuck::cast_slice(&m.verts), usage: wgpu::BufferUsages::VERTEX });
        let groups: Vec<_> = m.groups.iter().map(|g| (g.texture, g.range.clone())).collect();
        for t in groups.iter().filter_map(|g| g.0) {
            self.texture(gpu, fx, t);
        }
        self.meshes[id as usize] = Some(GpuMesh { vb, groups, mmb: m.mmb });
    }

    /// Draws the live elements over the scene (same targets, depth tested, back to front).
    #[allow(clippy::too_many_arguments)]
    /// `color_tex` is the texture behind `color` (CMoDist copies it; without one, or without COPY_SRC, distortion
    /// is not drawn).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: (&wgpu::TextureView, &wgpu::TextureView, [u32; 2]), color_tex: Option<&wgpu::Texture>, fx: &Effects, cam: (Mat4, Mat4, glam::Vec3), clip: [f32; 2], fog_color: [f32; 3], fog: [f32; 2]) {
        if self.draw_group.is_none() || self.count == 0 {
            return;
        }
        let order = fx.draw_order();
        let any_dist = order.iter().any(|&e| fx.elems[e as usize].as_ref().is_some_and(|c| c.draw == Draw::Dist));
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
                view_formats: &[],
            });
            let g = Self::tex_group(&gpu.device, &self.tex_layout, &self.copy_sampler, &c.create_view(&Default::default()));
            self.scene_copy = Some((c, g));
        }
        let Some(group) = self.draw_group.as_ref() else { return };
        let (color, depth, size) = target;
        let (proj, view, eye) = cam;
        let u = DrawU {
            proj: proj.to_cols_array_2d(),
            fog_color: [fog_color[0], fog_color[1], fog_color[2], 1.0],
            fog: [fog[0], fog[1].max(fog[0] + 1.0), self.linearize as u32 as f32, 0.0],
            clip: [clip[0], clip[1], size[0] as f32, size[1] as f32],
        };
        gpu.queue.write_buffer(&self.draw_u, 0, bytemuck::bytes_of(&u));
        // Lens flares count their visible sky samples against this frame's scene depth first.
        let flares: Vec<u32> = fx.draw_order().into_iter().filter(|&e| fx.elems[e as usize].as_ref().is_some_and(|c| matches!(c.draw, Draw::Sprite(id) if fx.reg.sprite_is_flare(id)))).collect();
        if !flares.is_empty() {
            let mut view_rot = view;
            view_rot.w_axis = glam::Vec4::W;
            let fu = FlareU { proj: proj.to_cols_array_2d(), view_rot: view_rot.to_cols_array_2d(), eye: [eye.x, eye.y, eye.z, 1.0], info: [self.count as f32, 0.0, 0.0, 0.0] };
            gpu.queue.write_buffer(&self.flare_u, 0, bytemuck::bytes_of(&fu));
            let fg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effects flare"),
                layout: &self.flare_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.flare_u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.elems.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.slots.0.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.outs.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(depth) },
                ],
            });
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("effects flare"), timestamp_writes: None });
            cp.set_pipeline(&self.flare_vis);
            cp.set_bind_group(0, &fg, &[]);
            cp.dispatch_workgroups(self.count.div_ceil(64), 1, 1);
        }
        let new_pass = |encoder: &mut wgpu::CommandEncoder| {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("effects"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            pass.set_bind_group(0, group, &[]);
            pass
        };
        let mut pass = new_pass(encoder);
        // Unknown modes keep the previous element's blend (additive before the first).
        let mut class = 2usize;
        let mut bound: Option<(bool, usize, bool)> = None;
        for e in order {
            let Some(c) = fx.elems[e as usize].as_ref() else { continue };
            if e >= self.count {
                continue;
            }
            let base = e * PER_ELEM;
            match c.draw {
                // FUN_100418b0 pass 1: the scene so far is copied (retail: the diamond's screen box into a 256x256
                // texture, a 2002 limit; here the whole target at full size), then the shifted redraw.
                Draw::Dist => {
                    let (Some(src), Some((copy, g))) = (copy_src, self.scene_copy.as_ref()) else { continue };
                    drop(pass);
                    encoder.copy_texture_to_texture(src.as_image_copy(), copy.as_image_copy(), src.size());
                    pass = new_pass(encoder);
                    pass.set_pipeline(&self.dist_pipes[&(c.flags & 0x1000 != 0)]);
                    bound = None;
                    pass.set_bind_group(1, g, &[]);
                    pass.draw(0..12, base..base + 1);
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
                    for (tex, range) in &m.groups {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::elem::{Elem, update_ref};

    impl EffectsGpu {
        fn read_elems(&self, gpu: &Gpu, n: usize) -> Vec<u32> {
            let size = (n * w::WORDS * 4) as u64;
            let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.elems, 0, &rb, 0, size);
            gpu.queue.submit([enc.finish()]);
            rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let v = bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec();
            v
        }
    }

    /// Port San d'Oria's auto-start generators (sea, water tint, lamps) run for a while on the GPU and through
    /// the CPU reference of the update stream; every live element must agree.
    #[test]
    fn gpu_update_matches_cpu() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
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
            let f = Frame { dt: 1.37, seconds: 1.37 / 60.0, dayfrac: 0.2 + tick as f32 * 0.01, view, view_rot, eye, draw_distance: 2000.0, area_light: glam::Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
            fx.tick(&f, &crate::fx::NoActors);
            for wr in &fx.writes {
                match wr {
                    Write::Full(e, el) => {
                        cpu.insert(*e, (**el).clone());
                    }
                    Write::Words(e, off, ws) => {
                        if let Some(c) = cpu.get_mut(e) {
                            c.0[*off..*off + ws.len()].copy_from_slice(ws);
                        }
                    }
                    Write::Parts(..) => {}
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
                let k = crate::dat::generator::KeyFrame { keys: reg.key_data[d[0] as usize..(d[0] + d[1]) as usize].iter().map(|p| (p[0], p[1])).collect() };
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
            for k in [w::POS, w::POS + 1, w::POS + 2, w::ROT, w::ROT + 1, w::ROT + 2, w::SCALE, w::SCALE + 1, w::SCALE + 2, w::FADE, w::UV, w::UV + 1, w::LIFE, w::T] {
                assert!((gf(k) - c.f(k)).abs() <= 1e-3 * c.f(k).abs().max(1.0), "element {e} word {k}: gpu {} cpu {}", gf(k), c.f(k));
            }
            assert_eq!(gw[w::COLOR], c.0[w::COLOR], "element {e} colour");
            checked += 1;
        }
        println!("{checked} elements agree");
        assert!(checked > 20, "only {checked} elements");
    }
}
