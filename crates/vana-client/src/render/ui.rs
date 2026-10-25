//! Menu sprites as screen-space textured quads, like the client's pre-transformed D3D8 vertices.

use super::Gpu;
use crate::dat::ui::{Cell, SpriteLib, UiFile};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// D3D8 blending, as FFXiMain maps the PS2 (A - B) * As + D fields (FUN_1011bc10).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Blend {
    Alpha,
    Add,
    Subtract,
    Opaque,
    /// Blending off: the source replaces the frame.
    Replace,
    /// Font glyphs: SRCBLEND = DESTBLEND = ONE (FUN_10118aa0).
    Glyph,
}

impl Blend {
    /// From the cell's (A, B, D) with 0 = source, 1 = frame, 2 = zero.
    pub fn from_ps2(a: u8, b: u8, d: u8) -> Self {
        match (a, b, d) {
            (0, 2, 1) => Blend::Add,
            (2, 0, 1) => Blend::Subtract,
            (0, 2, 2) => Blend::Opaque,
            (0, 2, 0) => Blend::Replace,
            _ => Blend::Alpha,
        }
    }
    /// A draw call's blend word: A bits 0-1, B bits 2-3, D bits 6-7 (0 = use the cell's own).
    pub fn from_word(w: u32) -> Self {
        Self::from_ps2((w & 3) as u8, (w >> 2 & 3) as u8, (w >> 6 & 3) as u8)
    }
    fn state(self) -> wgpu::BlendState {
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let c = |src, dst, operation| C { src_factor: src, dst_factor: dst, operation };
        match self {
            Blend::Alpha => wgpu::BlendState { color: c(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add), alpha: c(F::One, F::OneMinusSrcAlpha, O::Add) },
            Blend::Add => wgpu::BlendState { color: c(F::SrcAlpha, F::One, O::Add), alpha: c(F::Zero, F::One, O::Add) },
            Blend::Subtract => wgpu::BlendState { color: c(F::SrcAlpha, F::One, O::ReverseSubtract), alpha: c(F::Zero, F::One, O::Add) },
            Blend::Opaque => wgpu::BlendState { color: c(F::SrcAlpha, F::Zero, O::Add), alpha: c(F::One, F::Zero, O::Add) },
            Blend::Replace => wgpu::BlendState::REPLACE,
            Blend::Glyph => wgpu::BlendState { color: c(F::One, F::One, O::Add), alpha: c(F::Zero, F::One, O::Add) },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct UiVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// Depth tested against the scene (name plates: XYZRHW quads at the projected depth, ZWRITE on, FUN_10080100);
    /// negative = not tested.
    depth: f32,
}

struct Run {
    texture: Option<String>,
    blend: Blend,
    first: u32,
    count: u32,
    /// Scissor (x, y, w, h) in pixels, as the log's FUN_1015b390 viewport.
    clip: Option<[f32; 4]>,
}

/// Quads collected in draw order, split into runs by texture, blend and clip.
#[derive(Default)]
pub struct UiBatch {
    vertices: Vec<UiVertex>,
    runs: Vec<Run>,
    clip: Option<[f32; 4]>,
    depth: Option<f32>,
}

/// PS2 colour scale: 0x80 is 1.0.
fn ps2(c: u8) -> f32 {
    c as f32 / 128.0
}

impl UiBatch {
    fn push_quad(&mut self, texture: Option<&str>, blend: Blend, corners: [[f32; 2]; 4], uvs: [[f32; 2]; 4], colors: [[f32; 4]; 4]) {
        let first = self.vertices.len() as u32;
        for k in [0, 1, 2, 1, 3, 2] {
            self.vertices.push(UiVertex { pos: corners[k], uv: uvs[k], color: colors[k], depth: self.depth.unwrap_or(-1.0) });
        }
        if let Some(r) = self.runs.last_mut()
            && r.texture.as_deref() == texture
            && r.blend == blend
            && r.clip == self.clip
        {
            r.count += 6;
            return;
        }
        self.runs.push(Run { texture: texture.map(str::to_string), blend, first, count: 6, clip: self.clip });
    }

    /// Quads pushed from now on are hidden where the scene is nearer than `depth` (NDC z); None = no test.
    pub fn set_depth(&mut self, depth: Option<f32>) {
        self.depth = depth;
    }

    /// Clips the quads pushed from now on to (x, y, w, h); None = the whole target.
    pub fn set_clip(&mut self, clip: Option<[f32; 4]>) {
        self.clip = clip;
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Appends another batch's quads after this one's.
    pub fn append(&mut self, other: UiBatch) {
        let base = self.vertices.len() as u32;
        self.vertices.extend(other.vertices);
        for mut r in other.runs {
            r.first += base;
            self.runs.push(r);
        }
    }

    /// One cell at (x, y) scaled by (sx, sy); `tint` is ARGB with 0x80 = 1.0, as the client passes it.
    pub fn push_cell(&mut self, ui: &UiFile, cell: &Cell, x: f32, y: f32, sx: f32, sy: f32, tint: u32, blend_word: u32) {
        let corners = cell.corners.map(|c| [x + c[0] as f32 * sx, y + c[1] as f32 * sy]);
        self.cell_quad(ui, cell, corners, [cell.size[0] as f32, cell.size[1] as f32], tint, blend_word);
    }

    /// A cell's texture placed at (x, y) over w x h texels, tiled (FUN_1011c910 / FUN_1011b870,
    /// used for window frame pieces): screen size is (w * sx, h * sy); the cell's own corners are ignored.
    pub fn push_cell_rect(&mut self, ui: &UiFile, cell: &Cell, x: f32, y: f32, w: f32, h: f32, sx: f32, sy: f32, tint: u32, blend_word: u32) {
        let (x1, y1) = (x + w * sx, y + h * sy);
        self.cell_quad(ui, cell, [[x, y], [x1, y], [x, y1], [x1, y1]], [w, h], tint, blend_word);
    }

    /// One cell's whole texture area over a screen rect (w x h pixels), whatever its size in sprite units.
    pub fn push_cell_span(&mut self, ui: &UiFile, cell: &Cell, x: f32, y: f32, w: f32, h: f32, tint: u32, blend_word: u32) {
        let (x1, y1) = (x + w, y + h);
        self.cell_quad(ui, cell, [[x, y], [x1, y], [x, y1], [x1, y1]], [cell.size[0] as f32, cell.size[1] as f32], tint, blend_word);
    }

    /// One cell through a 2x2 matrix about (x, y): screen = (x + a cx + b cy, y + c cx + d cy) (FUN_1011c880).
    pub fn push_cell_affine(&mut self, ui: &UiFile, cell: &Cell, x: f32, y: f32, m: [f32; 4], tint: u32) {
        let corners = cell.corners.map(|c| {
            let (cx, cy) = (c[0] as f32, c[1] as f32);
            [x + m[0] * cx + m[1] * cy, y + m[2] * cx + m[3] * cy]
        });
        self.cell_quad(ui, cell, corners, [cell.size[0] as f32, cell.size[1] as f32], tint, 0);
    }

    fn cell_quad(&mut self, ui: &UiFile, cell: &Cell, corners: [[f32; 2]; 4], texels: [f32; 2], tint: u32, blend_word: u32) {
        let image = ui.images.get(&cell.texture);
        let (tw, th) = image.map(|i| (i.width as f32, i.height as f32)).unwrap_or((1.0, 1.0));
        let (mut u0, mut u1) = (cell.uv[0] as f32, cell.uv[0] as f32 + texels[0]);
        let (mut v0, mut v1) = (cell.uv[1] as f32, cell.uv[1] as f32 + texels[1]);
        if cell.flip & 1 != 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        if cell.flip & 2 != 0 {
            std::mem::swap(&mut v0, &mut v1);
        }
        let uvs = [[u0 / tw, v0 / th], [u1 / tw, v0 / th], [u0 / tw, v1 / th], [u1 / tw, v1 / th]];
        let t = [(tint >> 16) as u8, (tint >> 8) as u8, tint as u8, (tint >> 24) as u8];
        // FUN_1011c9c0: channel * tint >> 7, clamped to 255.
        let colors = cell.colors.map(|c| [0, 1, 2, 3].map(|i| ps2(((c[i] as u32 * t[i] as u32) >> 7).min(255) as u8)));
        let blend = if blend_word != 0 {
            Blend::from_word(blend_word)
        } else if cell.blend[0] != 0 {
            Blend::from_ps2(cell.blend[1], cell.blend[2], cell.blend[3])
        } else {
            // Byte 0 is the blend enable: 0 draws opaque (FUN_1011f300, ALPHABLENDENABLE FALSE), cut only by the alpha
            // test (the Equipment window's element balls, gauges, icons).
            Blend::Replace
        };
        self.push_quad(image.map(|_| cell.texture.as_str()), blend, corners, uvs, colors);
    }

    /// The cells of a sprite from `skip` on (FUN_1011c590; its last argument skips cells).
    pub fn push_sprite(&mut self, ui: &UiFile, lib: &SpriteLib, index: usize, x: f32, y: f32, sx: f32, sy: f32, tint: u32, blend_word: u32) {
        self.push_sprite_from(ui, lib, index, 0, x, y, sx, sy, tint, blend_word);
    }

    pub fn push_sprite_from(&mut self, ui: &UiFile, lib: &SpriteLib, index: usize, skip: usize, x: f32, y: f32, sx: f32, sy: f32, tint: u32, blend_word: u32) {
        if let Some(s) = lib.sprites.get(index) {
            for c in s.cells.iter().skip(skip) {
                self.push_cell(ui, c, x, y, sx, sy, tint, blend_word);
            }
        }
    }

    pub fn push_rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4]) {
        self.push_quad(None, Blend::Alpha, [[x, y], [x + w, y], [x, y + h], [x + w, y + h]], [[0.0; 2]; 4], [rgba; 4]);
    }

    /// A whole texture stretched over a rect, alpha blended.
    pub fn push_image(&mut self, texture: &str, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4]) {
        self.push_quad(Some(texture), Blend::Alpha, [[x, y], [x + w, y], [x, y + h], [x + w, y + h]], [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]], [rgba; 4]);
    }

    /// Game-font quads; `atlas` is the texture name the font atlas was loaded under.
    pub fn push_text(&mut self, quads: &[crate::dat::font::Quad], atlas: &str) {
        use crate::dat::font::{BLEND_ADD, Source};
        for q in quads {
            let texture = match &q.source {
                Source::Atlas => atlas.to_string(),
                Source::Texture(n) => crate::dat::ui::norm_name(n.as_bytes()),
            };
            let blend = if q.blend == BLEND_ADD { Blend::Glyph } else { Blend::from_ps2(q.blend[0], q.blend[1], q.blend[2]) };
            // BGRA, 0x80 = 1.0; the shader's texture * colour is MODULATE2X on that scale.
            let color = [ps2(q.color[2]), ps2(q.color[1]), ps2(q.color[0]), ps2(q.color[3])];
            let [u0, v0, u1, v1] = q.uv;
            let (x1, y1) = (q.x + q.w, q.y + q.h);
            self.push_quad(Some(&texture), blend, [[q.x, q.y], [x1, q.y], [q.x, y1], [x1, y1]], [[u0, v0], [u1, v0], [u0, v1], [u1, v1]], [color; 4]);
        }
    }

    /// A window frame of w x h from a style library ("win00", files 14..21), as FUN_1022a290:
    /// piece 0 background (9 when `alt_back`), 2/7 top/bottom edges, 4/5 sides, 1/3/6/8 corners.
    pub fn push_frame(&mut self, ui: &UiFile, style: &SpriteLib, x: f32, y: f32, w: f32, h: f32, sx: f32, sy: f32, tint: u32, blend_word: u32, alt_back: bool, no_edges: bool) {
        let mut piece = |k: usize, ox: f32, oy: f32, pw: f32, ph: f32| {
            if let Some(c) = style.sprites.get(k).and_then(|s| s.cells.first()) {
                self.push_cell_rect(ui, c, x + ox * sx, y + oy * sy, pw, ph, sx, sy, tint, blend_word);
            }
        };
        piece(if alt_back { 9 } else { 0 }, 0.0, 0.0, w, h);
        if no_edges {
            return;
        }
        piece(2, 24.0, 0.0, w - 48.0, 5.0);
        piece(7, 24.0, h - 5.0, w - 48.0, 5.0);
        if h > 48.0 {
            piece(4, 0.0, 24.0, 5.0, h - 48.0);
            piece(5, w - 5.0, 24.0, 5.0, h - 48.0);
        }
        piece(1, 0.0, 0.0, 24.0, 24.0);
        piece(3, w - 24.0, 0.0, 24.0, 24.0);
        piece(6, 0.0, h - 24.0, 24.0, 24.0);
        piece(8, w - 24.0, h - 24.0, 24.0, 24.0);
    }
}

const SHADER: &str = r#"
// screen.xy: the layout size the vertices are in; dst.xy: the pixels they land on.
struct U { screen: vec4<f32>, dst: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32>, @location(2) depth: f32 };
@vertex fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>, @location(3) depth: f32) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(pos.x / u.screen.x * 2.0 - 1.0, 1.0 - pos.y / u.screen.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    o.depth = depth;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    // Depth-tested quads (name plates) hide behind nearer scenery; the scene may be drawn at another size.
    if i.depth >= 0.0 {
        let dims = vec2<f32>(textureDimensions(scene_depth));
        let t = vec2<i32>(clamp(i.clip.xy / u.dst.xy * dims, vec2<f32>(0.0), dims - 1.0));
        if i.depth > textureLoad(scene_depth, t, 0) {
            discard;
        }
    }
    // u.screen.z: the 2D layer's colour (lobby 'sfin' / 'sfot', scheduler op 0x51), 1 = as drawn.
    let c = clamp(textureSample(tex, samp, i.uv) * i.color, vec4<f32>(0.0), vec4<f32>(1.0)) * u.screen.z;
    // The UI runs with ALPHATESTENABLE, ALPHAFUNC GREATER, ALPHAREF 0 (FUN_10159ae0).
    if c.a <= 0.0 {
        discard;
    }
    return c;
}
"#;

pub struct UiRenderer {
    /// One vertex buffer kept across frames; written only when the batch changed.
    vertices: Option<(wgpu::Buffer, u64)>,
    uploaded: Vec<UiVertex>,
    pipelines: HashMap<Blend, wgpu::RenderPipeline>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    globals: wgpu::BindGroup,
    /// Bound when no scene depth is given (nothing is depth tested then).
    no_depth: wgpu::TextureView,
    textures: HashMap<String, wgpu::BindGroup>,
    white: wgpu::BindGroup,
    /// The 2D layer's fade level (multiplies colour and alpha).
    pub fade: f32,
}

impl UiRenderer {
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ui"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui texture"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32],
        })];
        let mut pipelines = HashMap::new();
        for blend in [Blend::Alpha, Blend::Add, Blend::Subtract, Blend::Opaque, Blend::Replace, Blend::Glyph] {
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ui"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &buffers },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(blend.state()), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            });
            pipelines.insert(blend, p);
        }
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("ui uniforms"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let no_depth = super::depth_view(device, 1, 1);
        let globals = Self::globals_group(device, &globals_layout, &uniforms, &no_depth);
        // Sprite draws set ADDRESSU/V = WRAP (FUN_1011b290); frame pieces tile 1:1.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = Self::bind(gpu, &texture_layout, &sampler, 1, 1, &[255; 4]);
        Self { vertices: None, uploaded: Vec::new(), pipelines, texture_layout, sampler, uniforms, globals_layout, globals, no_depth, textures: HashMap::new(), white, fade: 1.0 }
    }

    fn globals_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniforms: &wgpu::Buffer, depth: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui globals"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) }],
        })
    }

    /// The scene depth that depth-tested quads (name plates) are tested against this frame; None = none.
    pub fn set_scene_depth(&mut self, gpu: &Gpu, depth: Option<&wgpu::TextureView>) {
        self.globals = Self::globals_group(&gpu.device, &self.globals_layout, &self.uniforms, depth.unwrap_or(&self.no_depth));
    }

    fn bind(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32, rgba: &[u8]) -> wgpu::BindGroup {
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("ui"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // D3D8 blends gamma-encoded values; so do we.
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            rgba,
        );
        let view = texture.create_view(&Default::default());
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }],
        })
    }

    /// Uploads every image of a UI file (once per name).
    pub fn load(&mut self, gpu: &Gpu, ui: &UiFile) {
        for (name, img) in &ui.images {
            self.load_image(gpu, name, img.width, img.height, &img.rgba);
        }
    }

    /// Binds an existing texture view (e.g. an offscreen frame) under `name`, replacing any old one.
    pub fn insert_view(&mut self, gpu: &Gpu, name: &str, view: &wgpu::TextureView) {
        let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
        });
        self.textures.insert(name.to_string(), bg);
    }

    /// Uploads one RGBA8 image under `name`, replacing any old one (window style changes).
    pub fn replace_image(&mut self, gpu: &Gpu, name: &str, width: u32, height: u32, rgba: &[u8]) {
        let bg = Self::bind(gpu, &self.texture_layout, &self.sampler, width, height, rgba);
        self.textures.insert(name.to_string(), bg);
    }

    /// Uploads one RGBA8 image under `name` unless one is already there.
    pub fn load_image(&mut self, gpu: &Gpu, name: &str, width: u32, height: u32, rgba: &[u8]) {
        if !self.textures.contains_key(name) {
            let bg = Self::bind(gpu, &self.texture_layout, &self.sampler, width, height, rgba);
            self.textures.insert(name.to_string(), bg);
        }
    }

    /// `layout`: the size the batch is laid out at (menu size 0037 / 0038); `size`: the target's pixels it is stretched
    /// over. Clips are in layout units.
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, layout: [u32; 2], size: [u32; 2], clear: Option<wgpu::Color>, batch: &UiBatch) {
        let [width, height] = size;
        let (kx, ky) = (width as f32 / layout[0].max(1) as f32, height as f32 / layout[1].max(1) as f32);
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&[layout[0] as f32, layout[1] as f32, self.fade.clamp(0.0, 1.0), 0.0, width as f32, height as f32, 0.0, 0.0]));
        let bytes: &[u8] = bytemuck::cast_slice(&batch.vertices);
        if !bytes.is_empty() {
            if self.vertices.as_ref().is_none_or(|(_, cap)| *cap < bytes.len() as u64) {
                let cap = (bytes.len() as u64).next_power_of_two().max(1 << 16);
                let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("ui vertices"), size: cap, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                self.vertices = Some((buf, cap));
                self.uploaded.clear();
            }
            if bytemuck::cast_slice::<UiVertex, u8>(&self.uploaded) != bytes
                && let Some((buf, _)) = &self.vertices
            {
                gpu.queue.write_buffer(buf, 0, bytes);
                self.uploaded.clear();
                self.uploaded.extend_from_slice(&batch.vertices);
            }
        }
        let vb = (!bytes.is_empty()).then(|| self.vertices.as_ref().map(|(b, _)| b)).flatten();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(vb) = vb else { return };
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_vertex_buffer(0, vb.slice(..bytes.len() as u64));
        for r in &batch.runs {
            let (x, y, w, h) = match r.clip {
                Some([x, y, w, h]) => {
                    let (x, y, w, h) = (x * kx, y * ky, w * kx, h * ky);
                    let x0 = x.max(0.0).min(width as f32) as u32;
                    let y0 = y.max(0.0).min(height as f32) as u32;
                    let x1 = (x + w).max(0.0).min(width as f32) as u32;
                    let y1 = (y + h).max(0.0).min(height as f32) as u32;
                    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
                }
                None => (0, 0, width, height),
            };
            if w == 0 || h == 0 {
                continue;
            }
            pass.set_scissor_rect(x, y, w, h);
            pass.set_pipeline(&self.pipelines[&r.blend]);
            let tex = r.texture.as_ref().and_then(|t| self.textures.get(t)).unwrap_or(&self.white);
            pass.set_bind_group(1, tex, &[]);
            pass.draw(r.first..r.first + r.count, 0..1);
        }
    }
}

/// Renders a batch offscreen over a solid colour and writes a PNG.
pub fn render_png(ui: &[&UiFile], batch: &UiBatch, width: u32, height: u32, clear: [f64; 3], path: &std::path::Path) -> Result<()> {
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = UiRenderer::new(&gpu, format);
    for f in ui {
        renderer.load(&gpu, f);
    }
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let row = (width * 4).div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (row * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.render(&gpu, &mut encoder, &view, [width, height], [width, height], Some(wgpu::Color { r: clear[0], g: clear[1], b: clear[2], a: 1.0 }), batch);
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
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
    write_png(path, width, height, &pixels)
}

pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}
