//! SMAA 1x (Jimenez et al., "SMAA: Enhanced Subpixel Morphological Antialiasing", 2012) on the offscreen 3D scene,
//! for the Anti-aliasing option (config `antialias` 1; not retail). A WGSL port of the reference SMAA.hlsl at its
//! HIGH preset (threshold 0.1, 16 search steps, 8 diagonal steps, corner rounding 25), luma edges, no predication or
//! reprojection. Three full-screen passes at the scene's size: edges (RG8), blend weights (RGBA8, through the
//! precomputed area and search textures in smaa/, MIT, see smaa/LICENSE.txt) and neighbourhood blending into a
//! second scene-sized texture, which the scaling pass then reads in place of the scene.

use super::Gpu;
use super::Put;

const AREA: &[u8] = include_bytes!("smaa/area.bin");
const SEARCH: &[u8] = include_bytes!("smaa/search.bin");

const SHADER: &str = r#"
struct R { rt: vec4<f32> };
@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var samp: sampler;
// Edges pass: t0 = scene (gamma). Weights pass: t0 = edges, t1 = area, t2 = search. Blend pass: t0 = scene, t1 = weights.
@group(0) @binding(2) var t0: texture_2d<f32>;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;

struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

const THRESHOLD: f32 = 0.1;
const CONTRAST_ADAPTATION: f32 = 2.0;
const MAX_STEPS: f32 = 16.0;
const MAX_STEPS_DIAG: f32 = 8.0;
const CORNER_ROUNDING: f32 = 0.25;
const AREA_MAX: f32 = 16.0;
const AREA_MAX_DIAG: f32 = 20.0;
const AREA_PX: vec2<f32> = vec2<f32>(1.0 / 160.0, 1.0 / 560.0);
const AREA_SUB: f32 = 1.0 / 7.0;
const SEARCH_SIZE: vec2<f32> = vec2<f32>(66.0, 33.0);
const SEARCH_PACKED: vec2<f32> = vec2<f32>(64.0, 16.0);

fn px() -> vec2<f32> { return r.rt.xy; }

// ---- Pass 1: luma edge detection ----
fn lum(uv: vec2<f32>) -> f32 { return dot(textureSampleLevel(t0, samp, uv, 0.0).rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
@fragment fn fs_edges(i: VOut) -> @location(0) vec4<f32> {
    let uv = i.uv;
    let p = px();
    let l = lum(uv);
    let ll = lum(uv + vec2<f32>(-p.x, 0.0));
    let lt = lum(uv + vec2<f32>(0.0, -p.y));
    let d = abs(vec2<f32>(l) - vec2<f32>(ll, lt));
    var e = step(vec2<f32>(THRESHOLD), d);
    if (e.x + e.y == 0.0) { discard; }
    let lr = lum(uv + vec2<f32>(p.x, 0.0));
    let lb = lum(uv + vec2<f32>(0.0, p.y));
    var m = max(d, abs(vec2<f32>(l) - vec2<f32>(lr, lb)));
    let lll = lum(uv + vec2<f32>(-2.0 * p.x, 0.0));
    let ltt = lum(uv + vec2<f32>(0.0, -2.0 * p.y));
    m = max(m, abs(vec2<f32>(ll, lt) - vec2<f32>(lll, ltt)));
    let f = max(m.x, m.y);
    e *= step(vec2<f32>(f), CONTRAST_ADAPTATION * d);
    return vec4<f32>(e, 0.0, 0.0);
}

// ---- Pass 2: blending weights ----
fn ed(uv: vec2<f32>) -> vec2<f32> { return textureSampleLevel(t0, samp, uv, 0.0).rg; }

fn decode2(e: vec2<f32>) -> vec2<f32> { return round(vec2<f32>(e.x * abs(5.0 * e.x - 3.75), e.y)); }
fn decode4(e: vec4<f32>) -> vec4<f32> { return round(vec4<f32>(e.x * abs(5.0 * e.x - 3.75), e.y, e.z * abs(5.0 * e.z - 3.75), e.w)); }

// A diagonal search: returns (distance, last-step flag) and the last edges fetched.
struct Diag { d: vec2<f32>, e: vec2<f32> };
fn search_diag1(uv: vec2<f32>, dir: vec2<f32>) -> Diag {
    var c = vec4<f32>(uv, -1.0, 1.0);
    var e = vec2<f32>(0.0);
    while (c.z < MAX_STEPS_DIAG - 1.0 && c.w > 0.9) {
        c = vec4<f32>(c.xy + px() * dir, c.z + 1.0, c.w);
        e = ed(c.xy);
        c.w = dot(e, vec2<f32>(0.5));
    }
    return Diag(c.zw, e);
}
fn search_diag2(uv: vec2<f32>, dir: vec2<f32>) -> Diag {
    var c = vec4<f32>(uv, -1.0, 1.0);
    c.x += 0.25 * px().x;
    var e = vec2<f32>(0.0);
    while (c.z < MAX_STEPS_DIAG - 1.0 && c.w > 0.9) {
        c = vec4<f32>(c.xy + px() * dir, c.z + 1.0, c.w);
        e = decode2(ed(c.xy));
        c.w = dot(e, vec2<f32>(0.5));
    }
    return Diag(c.zw, e);
}
fn area_diag(dist: vec2<f32>, e: vec2<f32>, offset: f32) -> vec2<f32> {
    var tc = vec2<f32>(AREA_MAX_DIAG) * e + dist;
    tc = AREA_PX * tc + 0.5 * AREA_PX;
    tc.x += 0.5;
    tc.y += AREA_SUB * offset;
    return textureSampleLevel(t1, samp, tc, 0.0).rg;
}
fn diag_weights(uv: vec2<f32>, e: vec2<f32>) -> vec2<f32> {
    let p = px();
    var w = vec2<f32>(0.0);
    var d = vec4<f32>(0.0);
    if (e.x > 0.0) {
        let s = search_diag1(uv, vec2<f32>(-1.0, 1.0));
        d.x = s.d.x + f32(s.e.y > 0.9);
        d.z = s.d.y;
    }
    let s2 = search_diag1(uv, vec2<f32>(1.0, -1.0));
    d.y = s2.d.x;
    d.w = s2.d.y;
    if (d.x + d.y > 2.0) {
        let co = vec4<f32>(-d.x + 0.25, d.x, d.y, -d.y - 0.25) * p.xyxy + uv.xyxy;
        let a = ed(co.xy + vec2<f32>(-1.0, 0.0) * p);
        let b = ed(co.zw + vec2<f32>(1.0, 0.0) * p);
        let c = decode4(vec4<f32>(a, b)).yxwz;
        var cc = 2.0 * c.xz + c.yw;
        if (d.z >= 0.9) { cc.x = 0.0; }
        if (d.w >= 0.9) { cc.y = 0.0; }
        w += area_diag(d.xy, cc, 0.0);
    }
    let s3 = search_diag2(uv, vec2<f32>(-1.0, -1.0));
    d.x = s3.d.x;
    d.z = s3.d.y;
    if (ed(uv + vec2<f32>(p.x, 0.0)).x > 0.0) {
        let s4 = search_diag2(uv, vec2<f32>(1.0, 1.0));
        d.y = s4.d.x + f32(s4.e.y > 0.9);
        d.w = s4.d.y;
    } else {
        d.y = 0.0;
        d.w = 0.0;
    }
    if (d.x + d.y > 2.0) {
        let co = vec4<f32>(-d.x, -d.x, d.y, d.y) * p.xyxy + uv.xyxy;
        let zw = ed(co.zw + vec2<f32>(1.0, 0.0) * p);
        let c = vec4<f32>(ed(co.xy + vec2<f32>(-1.0, 0.0) * p).y, ed(co.xy + vec2<f32>(0.0, -1.0) * p).x, zw.y, zw.x);
        var cc = 2.0 * c.xz + c.yw;
        if (d.z >= 0.9) { cc.x = 0.0; }
        if (d.w >= 0.9) { cc.y = 0.0; }
        w += area_diag(d.xy, cc, 0.0).yx;
    }
    return w;
}

fn search_length(e: vec2<f32>, offset: f32) -> f32 {
    var scale = SEARCH_SIZE * vec2<f32>(0.5, -1.0) + vec2<f32>(-1.0, 1.0);
    var bias = SEARCH_SIZE * vec2<f32>(offset, 1.0) + vec2<f32>(0.5, -0.5);
    scale /= SEARCH_PACKED;
    bias /= SEARCH_PACKED;
    return textureSampleLevel(t2, samp, scale * e + bias, 0.0).r;
}
fn search_left(uv: vec2<f32>, end: f32) -> f32 {
    var tc = uv;
    var e = vec2<f32>(0.0, 1.0);
    while (tc.x > end && e.y > 0.8281 && e.x == 0.0) {
        e = ed(tc);
        tc -= vec2<f32>(2.0, 0.0) * px();
    }
    let off = -(255.0 / 127.0) * search_length(e, 0.0) + 3.25;
    return px().x * off + tc.x;
}
fn search_right(uv: vec2<f32>, end: f32) -> f32 {
    var tc = uv;
    var e = vec2<f32>(0.0, 1.0);
    while (tc.x < end && e.y > 0.8281 && e.x == 0.0) {
        e = ed(tc);
        tc += vec2<f32>(2.0, 0.0) * px();
    }
    let off = -(255.0 / 127.0) * search_length(e, 0.5) + 3.25;
    return -px().x * off + tc.x;
}
fn search_up(uv: vec2<f32>, end: f32) -> f32 {
    var tc = uv;
    var e = vec2<f32>(1.0, 0.0);
    while (tc.y > end && e.x > 0.8281 && e.y == 0.0) {
        e = ed(tc);
        tc -= vec2<f32>(0.0, 2.0) * px();
    }
    let off = -(255.0 / 127.0) * search_length(e.yx, 0.0) + 3.25;
    return px().y * off + tc.y;
}
fn search_down(uv: vec2<f32>, end: f32) -> f32 {
    var tc = uv;
    var e = vec2<f32>(1.0, 0.0);
    while (tc.y < end && e.x > 0.8281 && e.y == 0.0) {
        e = ed(tc);
        tc += vec2<f32>(0.0, 2.0) * px();
    }
    let off = -(255.0 / 127.0) * search_length(e.yx, 0.5) + 3.25;
    return -px().y * off + tc.y;
}
fn area(dist: vec2<f32>, e1: f32, e2: f32, offset: f32) -> vec2<f32> {
    var tc = vec2<f32>(AREA_MAX) * round(4.0 * vec2<f32>(e1, e2)) + dist;
    tc = AREA_PX * tc + 0.5 * AREA_PX;
    tc.y += AREA_SUB * offset;
    return textureSampleLevel(t1, samp, tc, 0.0).rg;
}
fn corner_h(w: vec2<f32>, tc: vec4<f32>, d: vec2<f32>) -> vec2<f32> {
    let p = px();
    let lr = step(d.xy, d.yx);
    let rnd = (1.0 - CORNER_ROUNDING) * lr / (lr.x + lr.y);
    var f = vec2<f32>(1.0);
    f.x -= rnd.x * ed(tc.xy + vec2<f32>(0.0, 1.0) * p).x;
    f.x -= rnd.y * ed(tc.zw + vec2<f32>(1.0, 1.0) * p).x;
    f.y -= rnd.x * ed(tc.xy + vec2<f32>(0.0, -2.0) * p).x;
    f.y -= rnd.y * ed(tc.zw + vec2<f32>(1.0, -2.0) * p).x;
    return w * clamp(f, vec2<f32>(0.0), vec2<f32>(1.0));
}
fn corner_v(w: vec2<f32>, tc: vec4<f32>, d: vec2<f32>) -> vec2<f32> {
    let p = px();
    let lr = step(d.xy, d.yx);
    let rnd = (1.0 - CORNER_ROUNDING) * lr / (lr.x + lr.y);
    var f = vec2<f32>(1.0);
    f.x -= rnd.x * ed(tc.xy + vec2<f32>(1.0, 0.0) * p).y;
    f.x -= rnd.y * ed(tc.zw + vec2<f32>(1.0, 1.0) * p).y;
    f.y -= rnd.x * ed(tc.xy + vec2<f32>(-2.0, 0.0) * p).y;
    f.y -= rnd.y * ed(tc.zw + vec2<f32>(-2.0, 1.0) * p).y;
    return w * clamp(f, vec2<f32>(0.0), vec2<f32>(1.0));
}
@fragment fn fs_weights(i: VOut) -> @location(0) vec4<f32> {
    let uv = i.uv;
    let p = px();
    let pix = uv * r.rt.zw;
    let o0 = p.xyxy * vec4<f32>(-0.25, -0.125, 1.25, -0.125) + uv.xyxy;
    let o1 = p.xyxy * vec4<f32>(-0.125, -0.25, -0.125, 1.25) + uv.xyxy;
    let o2 = p.xxyy * vec4<f32>(-2.0, 2.0, -2.0, 2.0) * MAX_STEPS + vec4<f32>(o0.x, o0.z, o1.y, o1.w);
    var w = vec4<f32>(0.0);
    var e = ed(uv);
    if (e.y > 0.0) {
        let dw = diag_weights(uv, e);
        if (dw.x == -dw.y) {
            let cx = search_left(o0.xy, o2.x);
            let cy = o1.y;
            let e1 = ed(vec2<f32>(cx, cy)).x;
            let cz = search_right(o0.zw, o2.y);
            let d = abs(round(r.rt.zz * vec2<f32>(cx, cz) - pix.xx));
            let e2 = ed(vec2<f32>(cz, cy) + vec2<f32>(p.x, 0.0)).x;
            let a = corner_h(area(sqrt(d), e1, e2, 0.0), vec4<f32>(cx, uv.y, cz, uv.y), d);
            w.x = a.x;
            w.y = a.y;
        } else {
            w.x = dw.x;
            w.y = dw.y;
            e.x = 0.0;
        }
    }
    if (e.x > 0.0) {
        let cy = search_up(o1.xy, o2.z);
        let cx = o0.x;
        let e1 = ed(vec2<f32>(cx, cy)).y;
        let cz = search_down(o1.zw, o2.w);
        let d = abs(round(r.rt.ww * vec2<f32>(cy, cz) - pix.yy));
        let e2 = ed(vec2<f32>(cx, cz) + vec2<f32>(0.0, p.y)).y;
        let a = corner_v(area(sqrt(d), e1, e2, 0.0), vec4<f32>(uv.x, cy, uv.x, cz), d);
        w.z = a.x;
        w.w = a.y;
    }
    return w;
}

// ---- Pass 3: neighbourhood blending ----
fn col(uv: vec2<f32>) -> vec4<f32> { return textureSampleLevel(t0, samp, uv, 0.0); }
@fragment fn fs_blend(i: VOut) -> @location(0) vec4<f32> {
    let uv = i.uv;
    let p = px();
    let here = textureSampleLevel(t1, samp, uv, 0.0);
    let a = vec4<f32>(textureSampleLevel(t1, samp, uv + vec2<f32>(p.x, 0.0), 0.0).a, textureSampleLevel(t1, samp, uv + vec2<f32>(0.0, p.y), 0.0).g, here.z, here.x);
    if (dot(a, vec4<f32>(1.0)) < 1e-5) { return col(uv); }
    let h = max(a.x, a.z) > max(a.y, a.w);
    var off = vec4<f32>(0.0, a.y, 0.0, a.w);
    var bw = a.yw;
    if (h) {
        off = vec4<f32>(a.x, 0.0, a.z, 0.0);
        bw = a.xz;
    }
    bw /= dot(bw, vec2<f32>(1.0));
    let bc = off * vec4<f32>(p, -p) + uv.xyxy;
    return bw.x * col(bc.xy) + bw.y * col(bc.zw);
}
"#;

struct Targets {
    size: [u32; 2],
    scene: wgpu::Texture,
    edges: wgpu::TextureView,
    weights: wgpu::TextureView,
    out_view: wgpu::TextureView,
    edges_group: wgpu::BindGroup,
    weights_group: wgpu::BindGroup,
    blend_group: wgpu::BindGroup,
}

pub struct Smaa {
    layout: wgpu::BindGroupLayout,
    edges: wgpu::RenderPipeline,
    weights: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    area: wgpu::TextureView,
    search: wgpu::TextureView,
    format: wgpu::TextureFormat,
    targets: Option<Targets>,
}

fn texture(gpu: &Gpu, label: &str, size: [u32; 2], format: wgpu::TextureFormat, view_formats: &[wgpu::TextureFormat]) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats,
    })
}

impl Smaa {
    /// `format`: the scene texture's format (the output texture matches it).
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("smaa"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("smaa"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex_entry(2),
                tex_entry(3),
                tex_entry(4),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("smaa"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipe = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let edges = pipe("fs_edges", wgpu::TextureFormat::Rg8Unorm);
        let weights = pipe("fs_weights", wgpu::TextureFormat::Rgba8Unorm);
        let blend = pipe("fs_blend", format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("smaa"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let lut = |label, size: [u32; 2], format, bytes: &[u8], bpp: u32| {
            let t = texture(gpu, label, size, format, &[]);
            gpu.queue.write_texture(
                t.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * bpp), rows_per_image: Some(size[1]) },
                wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            );
            t.create_view(&Default::default())
        };
        let area = lut("smaa area", [160, 560], wgpu::TextureFormat::Rg8Unorm, AREA, 2);
        let search = lut("smaa search", [64, 16], wgpu::TextureFormat::R8Unorm, SEARCH, 1);
        Self { layout, edges, weights, blend, sampler, uniform, area, search, format, targets: None }
    }

    fn group(&self, gpu: &Gpu, views: [&wgpu::TextureView; 3]) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smaa"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(views[0]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(views[2]) },
            ],
        })
    }

    /// Anti-aliases `scene` (`size`, in `format`) into the output texture, which it returns. Edges are found on the
    /// gamma-space values (a plain view); the blend reads and writes through the scene format's own views, as the
    /// reference does with sRGB views. The bind groups are remade when `size` or the scene texture changes.
    pub fn run(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, scene: &wgpu::Texture, size: [u32; 2]) -> &wgpu::TextureView {
        let size = [size[0].max(1), size[1].max(1)];
        if self.targets.as_ref().is_none_or(|t| t.size != size || t.scene != *scene) {
            let plain = self.format.remove_srgb_suffix();
            let edges = texture(gpu, "smaa edges", size, wgpu::TextureFormat::Rg8Unorm, &[]).create_view(&Default::default());
            let weights = texture(gpu, "smaa weights", size, wgpu::TextureFormat::Rgba8Unorm, &[]).create_view(&Default::default());
            let out_view = texture(gpu, "smaa out", size, self.format, &[plain]).create_view(&Default::default());
            let scene_plain = scene.create_view(&wgpu::TextureViewDescriptor { format: Some(plain), ..Default::default() });
            let scene_view = scene.create_view(&Default::default());
            let edges_group = self.group(gpu, [&scene_plain, &self.area, &self.search]);
            let weights_group = self.group(gpu, [&edges, &self.area, &self.search]);
            let blend_group = self.group(gpu, [&scene_view, &weights, &self.search]);
            self.targets = Some(Targets { size, scene: scene.clone(), edges, weights, out_view, edges_group, weights_group, blend_group });
        }
        let t = self.targets.as_ref().unwrap();
        let rt = [1.0 / size[0] as f32, 1.0 / size[1] as f32, size[0] as f32, size[1] as f32];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&rt));
        for (view, pipeline, group) in [(&t.edges, &self.edges, &t.edges_group), (&t.weights, &self.weights, &t.weights_group), (&t.out_view, &self.blend, &t.blend_group)] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("smaa"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        &t.out_view
    }
}
