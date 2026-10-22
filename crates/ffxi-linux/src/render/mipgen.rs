//! Mip chains made on the GPU. Level 0 is uploaded as the file's own blocks (BC1 / BC2) or RGBA; compute passes
//! box-filter each lower level in linear light, keep the alpha-tested coverage of level 0 (alpha scaled per level,
//! the smallest scale reaching level 0's share of texels over the 0.3 cut-off) and BC-encode the levels of BC
//! textures (bounding box inset by 1/16, nearest palette entry), then copy them in. Jobs run under a texel budget a
//! frame; a texture's lower levels are valid once `done()` reaches its sequence number.

use std::collections::VecDeque;

const WGSL: &str = r#"
struct P {
    src_w: u32, src_h: u32, w: u32, h: u32,
    // Source level in `pix` (NONE = the texture's level 0), this level in `pix`.
    src_off: u32, off: u32, level: u32,
    // 0 BC1, 1 BC2, 2 RGBA.
    fmt: u32,
    out_off: u32, row: u32, n0: u32,
    // 1 when the level-0 view is not sRGB, so its values are linearised here.
    lin: u32,
}
@group(0) @binding(0) var src0: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> pix: array<u32>;
@group(0) @binding(2) var<storage, read_write> stats: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> outw: array<u32>;
@group(0) @binding(4) var<uniform> p: P;

const NONE: u32 = 0xffffffffu;
const CUT: f32 = CUT_VALUE;
const CUT_BYTE: i32 = CUT_BYTEi;

// stats: [0] level-0 texels over the cut-off, [1] lowest level-0 alpha, then per level 256 bins and its scale.
fn hist(level: u32) -> u32 { return 2u + level * 257u; }
fn scale_of(level: u32) -> f32 {
    if (level == 0u) { return 1.0; }
    return bitcast<f32>(atomicLoad(&stats[hist(level) + 256u]));
}
fn to_lin(c: f32) -> f32 {
    if (c <= 0.04045) { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}
fn srgb_byte(l: f32) -> u32 {
    var c = l * 12.92;
    if (l > 0.0031308) { c = 1.055 * pow(max(l, 0.0), 1.0 / 2.4) - 0.055; }
    return u32(clamp(c, 0.0, 1.0) * 255.0 + 0.5);
}

// A source texel: linear colour and the alpha byte (scaled as its level was).
fn fetch(x: u32, y: u32) -> vec4<f32> {
    if (p.src_off == NONE) {
        let t = textureLoad(src0, vec2<i32>(i32(x), i32(y)), 0);
        var c = t.rgb;
        if (p.lin == 1u) { c = vec3<f32>(to_lin(c.r), to_lin(c.g), to_lin(c.b)); }
        return vec4<f32>(c, round(t.a * 255.0));
    }
    let u = pix[p.src_off + y * p.src_w + x];
    let a = min(floor(f32(u >> 24u) * scale_of(p.level - 1u)), 255.0);
    return vec4<f32>(to_lin(f32(u & 255u) / 255.0), to_lin(f32((u >> 8u) & 255u) / 255.0), to_lin(f32((u >> 16u) & 255u) / 255.0), a);
}

@compute @workgroup_size(8, 8)
fn cover0(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= p.w || g.y >= p.h) { return; }
    let a = u32(round(textureLoad(src0, vec2<i32>(i32(g.x), i32(g.y)), 0).a * 255.0));
    if (f32(a) >= CUT) { atomicAdd(&stats[0], 1u); }
    atomicMin(&stats[1], a);
}

@compute @workgroup_size(8, 8)
fn down(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= p.w || g.y >= p.h) { return; }
    var acc = vec4<f32>(0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        acc += fetch(min(g.x * 2u + (k & 1u), p.src_w - 1u), min(g.y * 2u + (k >> 1u), p.src_h - 1u));
    }
    let c = acc / 4.0;
    let a = u32(c.a + 0.5);
    pix[p.off + g.y * p.w + g.x] = srgb_byte(c.r) | (srgb_byte(c.g) << 8u) | (srgb_byte(c.b) << 16u) | (a << 24u);
    atomicAdd(&stats[hist(p.level) + a], 1u);
}

// The level's alpha scale: binary search for the smallest scale whose coverage reaches level 0's.
@compute @workgroup_size(1)
fn solve() {
    let goal = f32(atomicLoad(&stats[0])) / f32(max(p.n0, 1u));
    var s = 1.0;
    if (atomicLoad(&stats[1]) < 250u && goal > 0.0 && goal < 1.0) {
        let n = f32(max(p.w * p.h, 1u));
        var lo = 0.25;
        var hi = 8.0;
        for (var i = 0; i < 16; i = i + 1) {
            let mid = (lo + hi) / 2.0;
            var c = 0u;
            for (var a = 0u; a < 256u; a = a + 1u) {
                if (f32(a) * mid >= CUT) { c += atomicLoad(&stats[hist(p.level) + a]); }
            }
            if (f32(c) / n < goal) { lo = mid; } else { hi = mid; }
        }
        s = hi;
    }
    atomicStore(&stats[hist(p.level) + 256u], bitcast<u32>(s));
}

fn texel(x: u32, y: u32) -> vec4<i32> {
    let u = pix[p.off + y * p.w + x];
    let a = min(floor(f32(u >> 24u) * scale_of(p.level)), 255.0);
    return vec4<i32>(i32(u & 255u), i32((u >> 8u) & 255u), i32((u >> 16u) & 255u), i32(a));
}
fn rgb565(c: vec3<i32>) -> u32 {
    return ((u32(c.r) >> 3u) << 11u) | ((u32(c.g) >> 2u) << 5u) | (u32(c.b) >> 3u);
}
fn expand(c: u32) -> vec3<i32> {
    let r = i32((c >> 11u) & 31u);
    let g = i32((c >> 5u) & 63u);
    let b = i32(c & 31u);
    return vec3<i32>((r << 3u) | (r >> 2u), (g << 2u) | (g >> 4u), (b << 3u) | (b >> 2u));
}
fn dist(a: vec3<i32>, b: vec3<i32>) -> i32 {
    let d = a - b;
    return d.r * d.r + d.g * d.g + d.b * d.b;
}

@compute @workgroup_size(8, 8)
fn encode(@builtin(global_invocation_id) g: vec3<u32>) {
    if (p.fmt == 2u) {
        if (g.x >= p.w || g.y >= p.h) { return; }
        let t = vec4<u32>(texel(g.x, g.y));
        outw[p.out_off + g.y * p.row + g.x] = t.r | (t.g << 8u) | (t.b << 16u) | (t.a << 24u);
        return;
    }
    if (g.x >= (p.w + 3u) / 4u || g.y >= (p.h + 3u) / 4u) { return; }
    var px: array<vec4<i32>, 16>;
    for (var i = 0u; i < 16u; i = i + 1u) {
        px[i] = texel(min(g.x * 4u + (i & 3u), p.w - 1u), min(g.y * 4u + (i >> 2u), p.h - 1u));
    }
    let four = p.fmt == 1u;
    let punch = p.fmt == 0u && atomicLoad(&stats[1]) < 128u;
    var lo = vec3<i32>(255);
    var hi = vec3<i32>(0);
    var solid = false;
    var holes = false;
    for (var i = 0u; i < 16u; i = i + 1u) {
        if (!punch || px[i].a >= CUT_BYTE) {
            lo = min(lo, px[i].rgb);
            hi = max(hi, px[i].rgb);
            solid = true;
        }
        holes = holes || px[i].a < CUT_BYTE;
    }
    if (!solid) { lo = vec3<i32>(0); hi = vec3<i32>(0); }
    let inset = (hi - lo) >> vec3<u32>(4u);
    lo = min(lo + inset, vec3<i32>(255));
    hi = max(hi - inset, vec3<i32>(0));
    var c0 = rgb565(hi);
    var c1 = rgb565(lo);
    let three = punch && holes;
    if ((three && c0 > c1) || (!three && c0 < c1)) {
        let t = c0;
        c0 = c1;
        c1 = t;
    }
    let a = expand(c0);
    let b = expand(c1);
    var pal: array<vec3<i32>, 4>;
    pal[0] = a;
    pal[1] = b;
    var n = 4u;
    if (three || (!four && c0 <= c1)) {
        pal[2] = (a + b) / 2;
        n = 3u;
    } else {
        pal[2] = (2 * a + b) / 3;
        pal[3] = (a + 2 * b) / 3;
    }
    var bits = 0u;
    for (var i = 0u; i < 16u; i = i + 1u) {
        var idx = 3u;
        if (!(three && px[i].a < CUT_BYTE)) {
            idx = 0u;
            var best = dist(px[i].rgb, pal[0]);
            for (var j = 1u; j < n; j = j + 1u) {
                let d = dist(px[i].rgb, pal[j]);
                if (d < best) { best = d; idx = j; }
            }
        }
        bits |= idx << (2u * i);
    }
    var o = p.out_off + g.y * p.row + g.x * 2u;
    if (four) {
        o = p.out_off + g.y * p.row + g.x * 4u;
        var w0 = 0u;
        var w1 = 0u;
        for (var i = 0u; i < 16u; i = i + 1u) {
            let nib = min((u32(px[i].a) + 8u) / 17u, 15u);
            if (i < 8u) { w0 |= nib << (4u * i); } else { w1 |= nib << (4u * (i - 8u)); }
        }
        outw[o] = w0;
        outw[o + 1u] = w1;
        o = o + 2u;
    }
    outw[o] = c0 | (c1 << 16u);
    outw[o + 1u] = bits;
}
"#;

/// Output formats of the lower levels, as the shader numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bc1 = 0,
    Bc2 = 1,
    Rgba = 2,
}

/// Levels down to 1 x 1, as the CPU chain had them.
pub fn level_count(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

#[derive(Clone)]
pub struct Pipes {
    layout: wgpu::BindGroupLayout,
    cover0: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
    solve: wgpu::ComputePipeline,
    encode: wgpu::ComputePipeline,
}

/// Uniform block per level, at 256-byte dynamic offsets.
const PARAM_STRIDE: u64 = 256;
const PARAM_WORDS: usize = 12;
const NONE: u32 = u32::MAX;

impl Pipes {
    pub fn new(device: &wgpu::Device) -> Self {
        // The cut-off as the CPU used it: coverage at 255 x ALPHA_REF, block solidity at that truncated to a byte.
        let cut = crate::dat::texmip::ALPHA_REF * 255.0;
        let src = WGSL.replace("CUT_VALUE", &format!("{cut:?}")).replace("CUT_BYTEi", &format!("{}", cut as u8));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mipgen"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mipgen"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new((PARAM_WORDS * 4) as u64) },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mipgen"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let make = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: Some(&pl), module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: None })
        };
        Pipes { cover0: make("cover0"), down: make("down"), solve: make("solve"), encode: make("encode"), layout }
    }
}

struct Job {
    texture: wgpu::Texture,
    w: u32,
    h: u32,
    levels: u32,
    kind: Kind,
    lin: bool,
    seq: u64,
}

#[derive(Default)]
pub struct MipGen {
    jobs: VecDeque<Job>,
    queued: u64,
    done: u64,
}

impl MipGen {
    /// Queues the lower levels of `texture` (level 0 already written); returns the sequence number to wait for.
    pub fn push(&mut self, texture: wgpu::Texture, w: u32, h: u32, kind: Kind, lin: bool) -> u64 {
        let levels = texture.mip_level_count();
        if levels <= 1 {
            return self.done;
        }
        self.queued += 1;
        self.jobs.push_back(Job { texture, w, h, levels, kind, lin, seq: self.queued });
        self.queued
    }

    /// Every job up to this sequence number has been recorded (its commands run before later submissions).
    pub fn done(&self) -> u64 {
        self.done
    }

    pub fn pending(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Records queued jobs into `encoder` until `budget` level-0 texels are used (at least one job).
    pub fn run(&mut self, device: &wgpu::Device, pipes: &Pipes, encoder: &mut wgpu::CommandEncoder, budget: u64) {
        let mut used = 0u64;
        while let Some(job) = self.jobs.front() {
            let cost = job.w as u64 * job.h as u64;
            if used > 0 && used + cost > budget {
                break;
            }
            used += cost;
            let job = self.jobs.pop_front().unwrap();
            record(device, pipes, encoder, &job);
            self.done = job.seq;
        }
        if used > 0 {
            crate::client::perf::note(|| format!("mips {used} texels, {} jobs left", self.jobs.len()));
        }
    }
}

fn record(device: &wgpu::Device, pipes: &Pipes, encoder: &mut wgpu::CommandEncoder, job: &Job) {
    use wgpu::util::DeviceExt;
    let n = job.levels as usize;
    let size = |k: usize| ((job.w >> k).max(1), (job.h >> k).max(1));
    let block = match job.kind {
        Kind::Bc1 => 8u32,
        Kind::Bc2 => 16,
        Kind::Rgba => 0,
    };
    // Scratch texels of levels 1.. and the encoded levels, rows 256-byte aligned for the copies.
    let mut pix_off = vec![0u32; n];
    let mut out_off = vec![0u64; n];
    let mut rows = vec![(0u32, 0u32); n];
    let (mut pix_words, mut out_bytes) = (0u32, 0u64);
    for k in 1..n {
        let (w, h) = size(k);
        pix_off[k] = pix_words;
        pix_words += w * h;
        let (row, count) = if block == 0 { (w * 4, h) } else { (w.div_ceil(4) * block, h.div_ceil(4)) };
        let row = row.div_ceil(256) * 256;
        out_off[k] = out_bytes;
        rows[k] = (row, count);
        out_bytes += (row * count) as u64;
        out_bytes = out_bytes.div_ceil(256) * 256;
    }
    let mut params = vec![0u32; n * PARAM_STRIDE as usize / 4];
    for k in 0..n {
        let (w, h) = size(k);
        let (sw, sh) = if k == 0 { (w, h) } else { size(k - 1) };
        let src = if k <= 1 { NONE } else { pix_off[k - 1] };
        let p = [sw, sh, w, h, src, pix_off[k], k as u32, job.kind as u32, (out_off[k] / 4) as u32, rows[k].0 / 4, job.w * job.h, job.lin as u32];
        params[k * PARAM_STRIDE as usize / 4..][..PARAM_WORDS].copy_from_slice(&p);
    }
    let mut stats = vec![0u32; 2 + n * 257];
    stats[1] = 255;
    let storage = wgpu::BufferUsages::STORAGE;
    let pix = device.create_buffer(&wgpu::BufferDescriptor { label: Some("mip texels"), size: (pix_words.max(1) * 4) as u64, usage: storage, mapped_at_creation: false });
    let stats = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("mip stats"), contents: bytemuck::cast_slice(&stats), usage: storage });
    let out = device.create_buffer(&wgpu::BufferDescriptor { label: Some("mip levels"), size: out_bytes.max(4), usage: storage | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("mip params"), contents: bytemuck::cast_slice(&params), usage: wgpu::BufferUsages::UNIFORM });
    let view0 = job.texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: 0, mip_level_count: Some(1), ..Default::default() });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mipgen"),
        layout: &pipes.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view0) },
            wgpu::BindGroupEntry { binding: 1, resource: pix.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: stats.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: out.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: 0, size: wgpu::BufferSize::new((PARAM_WORDS * 4) as u64) }),
            },
        ],
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mipgen"), timestamp_writes: None });
        let mut run = |pipe: &wgpu::ComputePipeline, k: usize, x: u32, y: u32| {
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &group, &[(k as u64 * PARAM_STRIDE) as u32]);
            pass.dispatch_workgroups(x.div_ceil(8).max(1), y.div_ceil(8).max(1), 1);
        };
        run(&pipes.cover0, 0, job.w, job.h);
        for k in 1..n {
            let (w, h) = size(k);
            run(&pipes.down, k, w, h);
            run(&pipes.solve, k, 1, 1);
        }
        for k in 1..n {
            let (w, h) = size(k);
            if block == 0 { run(&pipes.encode, k, w, h) } else { run(&pipes.encode, k, w.div_ceil(4), h.div_ceil(4)) }
        }
    }
    for k in 1..n {
        let (w, h) = size(k);
        let extent = if block == 0 { wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 } } else { wgpu::Extent3d { width: w.div_ceil(4) * 4, height: h.div_ceil(4) * 4, depth_or_array_layers: 1 } };
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: out_off[k], bytes_per_row: Some(rows[k].0), rows_per_image: Some(rows[k].1) } },
            wgpu::TexelCopyTextureInfo { texture: &job.texture, mip_level: k as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            extent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::texmip::reference;
    use crate::dat::zone::{Texture, decode_dxt};
    use crate::render::Gpu;

    /// Mip `level` of `tex` as bytes, rows packed.
    fn read(gpu: &Gpu, tex: &wgpu::Texture, level: u32, row: u32, rows: u32, extent: wgpu::Extent3d) -> Vec<u8> {
        let pitch = row.div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (pitch * rows) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(rows) } },
            extent,
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let data = buf.slice(..).get_mapped_range().unwrap();
        (0..rows).flat_map(|y| data[(y * pitch) as usize..][..row as usize].to_vec()).collect()
    }

    fn texture(gpu: &Gpu, t: &Texture, format: wgpu::TextureFormat, kind: Kind) -> wgpu::Texture {
        let (w, h) = (t.width, t.height);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: level_count(w, h),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let (data, row, rows) = match &t.dxt {
            Some((_, b)) => (b.as_slice(), w / 4 * 8, h / 4),
            None => (t.rgba.as_slice(), w * 4, h),
        };
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.mips.lock().unwrap().push(tex.clone(), w, h, kind, false);
        gpu.flush_mips();
        tex
    }

    fn worst(a: &[u8], b: &[u8]) -> i32 {
        a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap_or(0)
    }

    /// The compute levels match the former CPU chain: a cut-out RGBA image keeps its coverage level by level,
    /// and a BC1 image's re-encoded levels decode to the CPU's within a few steps.
    #[test]
    fn gpu_mips_match_cpu_chain() {
        let Ok(gpu) = Gpu::headless() else { return };
        let (w, h) = (32u32, 32u32);
        // Leaves: opaque 2x2 clumps on a 4-texel grid plus a fade.
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let a = if x % 4 < 2 && y % 4 < 2 { 255 } else { (x * 6) as u8 };
                [(90 + x * 3) as u8, (120 + y * 2) as u8, 60, a]
            })
            .collect();
        let cpu = reference::rgba_chain(&img, w, h);
        let t = Texture::new(String::new(), 0, w, h, img.clone());
        let tex = texture(&gpu, &t, wgpu::TextureFormat::Rgba8UnormSrgb, Kind::Rgba);
        let cov = |p: &[u8]| p.chunks_exact(4).filter(|p| p[3] as f32 >= 76.5).count();
        for (k, (px, lw, lh)) in cpu.iter().enumerate().skip(1) {
            let got = read(&gpu, &tex, k as u32, lw * 4, *lh, wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 });
            assert!(worst(&got, px) <= 2, "level {k}: {}", worst(&got, px));
            assert!(cov(&got).abs_diff(cov(px)) <= (lw * lh / 16) as usize, "level {k} coverage {} vs {}", cov(&got), cov(px));
        }
        if !gpu.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            return;
        }
        let opaque: Vec<u8> = img.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let blocks = reference::encode_bc1(&opaque, w, h, false);
        let mut t = Texture::new(String::new(), 0, w, h, decode_dxt(&blocks, w, h, false));
        t.dxt = Some((false, blocks.clone()));
        let want = reference::levels(&t.rgba, w, h, Some((false, &blocks)));
        let tex = texture(&gpu, &t, wgpu::TextureFormat::Bc1RgbaUnormSrgb, Kind::Bc1);
        for k in 1..want.data.len() {
            let (lw, lh) = ((w >> k).max(1), (h >> k).max(1));
            let (bx, by) = (lw.div_ceil(4), lh.div_ceil(4));
            let got = read(&gpu, &tex, k as u32, bx * 8, by, wgpu::Extent3d { width: bx * 4, height: by * 4, depth_or_array_layers: 1 });
            let d = worst(&decode_dxt(&got, bx * 4, by * 4, false), &decode_dxt(&want.data[k], bx * 4, by * 4, false));
            assert!(d <= 12, "BC1 level {k}: {d}");
        }
    }
}
