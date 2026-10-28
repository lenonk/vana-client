//! CPU-to-GPU copies under a per-frame time budget. Every write goes through wgpu's mapped staging memory, which
//! the allocator puts in device-local, host-visible memory when there is any (gpu-allocator CpuToGpu): over an
//! eGPU's Thunderbolt link that runs at a few hundred MB/s, so a frame may only copy what fits its share of time.
//! The rate is measured as the copies go, so faster machines get more a frame.

use super::Gpu;
use std::time::Instant;
use super::Put;

/// Copy time a frame in the world, and while the zone indicator is up.
pub const FRAME_MS: f64 = 2.0;
pub const LOADING_MS: f64 = 8.0;
/// The smallest piece worth a call.
const MIN_PIECE: u64 = 64 << 10;

pub struct Budget {
    left: i64,
    /// Measured bytes per ms (starts at 1 GB/s).
    rate: f64,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { left: 0, rate: 1e6 }
    }
}

impl Gpu {
    /// Starts a frame's copies: `ms` of them at the measured rate.
    pub fn begin_uploads(&self, ms: f64) {
        if let Ok(mut b) = self.uploads.lock() {
            b.left = ((b.rate * ms) as i64).clamp(MIN_PIECE as i64, 64 << 20);
        }
    }

    /// Bytes this frame may still copy.
    pub fn upload_room(&self) -> u64 {
        self.uploads.lock().map_or(0, |b| b.left.max(0) as u64)
    }

    /// Runs one copy of `bytes`, charging it and measuring the rate.
    pub fn upload<T>(&self, bytes: u64, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if let Ok(mut b) = self.uploads.lock() {
            b.left -= bytes as i64;
            if bytes >= MIN_PIECE && ms > 0.05 {
                b.rate = b.rate * 0.8 + bytes as f64 / ms * 0.2;
            }
        }
        r
    }

    /// Room for a piece: up to the budget, rounded down to `unit` (at least one unit, so work always moves).
    pub fn piece(&self, remaining: u64, unit: u64) -> u64 {
        let room = self.upload_room().max(unit.max(1));
        remaining.min(room / unit.max(1) * unit.max(1)).max(unit.min(remaining))
    }
}

/// Level 0 of an image as it goes up: the file's DXT blocks as BC1 / BC2 when the device samples them (both sides
/// a multiple of 4), else RGBA. (kind, format, data, bytes per block row, block rows, texel rows per block row).
pub fn level0<'a>(gpu: &Gpu, t: &'a crate::dat::zone::Texture, srgb: bool) -> (super::mipgen::Kind, wgpu::TextureFormat, &'a [u8], u32, u32, u32) {
    use super::mipgen::Kind;
    use wgpu::TextureFormat as F;
    let (w, h) = (t.width, t.height);
    let pick = |s: F, u: F| if srgb { s } else { u };
    let bc = gpu.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC) && !super::UNCOMPRESSED.load(std::sync::atomic::Ordering::Relaxed);
    match &t.dxt {
        Some((true, b)) if bc && w % 4 == 0 && h % 4 == 0 => (Kind::Bc2, pick(F::Bc2RgbaUnormSrgb, F::Bc2RgbaUnorm), b.as_slice(), w / 4 * 16, h / 4, 4),
        Some((false, b)) if bc && w % 4 == 0 && h % 4 == 0 => (Kind::Bc1, pick(F::Bc1RgbaUnormSrgb, F::Bc1RgbaUnorm), b.as_slice(), w / 4 * 8, h / 4, 4),
        _ => (Kind::Rgba, pick(F::Rgba8UnormSrgb, F::Rgba8Unorm), t.rgba.as_slice(), w * 4, h, 1),
    }
}

/// A texture going up in bands of rows; its mips are queued once level 0 is complete.
pub struct TexUp {
    pub texture: wgpu::Texture,
    kind: super::mipgen::Kind,
    srgb: bool,
    row: u32,
    rows: u32,
    block_h: u32,
    done: u32,
}

impl TexUp {
    pub fn new(gpu: &Gpu, t: &crate::dat::zone::Texture, srgb: bool) -> Self {
        let (kind, format, _, row, rows, block_h) = level0(gpu, t, srgb);
        let levels = super::mipgen::level_count(t.width, t.height);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: u64 = (0..levels).map(|k| level_bytes(kind, (t.width >> k).max(1), (t.height >> k).max(1))).sum();
        super::TEXTURE_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
        TexUp { texture, kind, srgb, row, rows, block_h, done: 0 }
    }

    /// Copies bands of `t` (the same image `new` saw) as the budget allows, all of it when `all`; once level 0 is
    /// in, queues the mips and returns their sequence number.
    pub fn step(&mut self, gpu: &Gpu, t: &crate::dat::zone::Texture, all: bool) -> Option<u64> {
        let (_, _, data, ..) = level0(gpu, t, self.srgb);
        while self.done < self.rows && (all || gpu.upload_room() > 0) {
            let left = self.rows - self.done;
            let n = if all { left } else { (gpu.piece(left as u64 * self.row as u64, self.row as u64) / self.row as u64) as u32 };
            let at = (self.done * self.row) as usize;
            let len = (n * self.row) as usize;
            gpu.upload(len as u64, || {
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: self.done * self.block_h, z: 0 }, aspect: wgpu::TextureAspect::All },
                    &data[at..at + len],
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.row), rows_per_image: Some(n) },
                    wgpu::Extent3d { width: t.width, height: n * self.block_h, depth_or_array_layers: 1 },
                )
            });
            self.done += n;
        }
        (self.done >= self.rows).then(|| gpu.mips.lock().map(|mut m| m.push(self.texture.clone(), t.width, t.height, self.kind, !self.srgb)).unwrap_or(0))
    }
}

fn level_bytes(kind: super::mipgen::Kind, w: u32, h: u32) -> u64 {
    use super::mipgen::Kind;
    let (w, h) = (w as u64, h as u64);
    match kind {
        Kind::Rgba => w * h * 4,
        Kind::Bc1 => w.div_ceil(4) * h.div_ceil(4) * 8,
        Kind::Bc2 => w.div_ceil(4) * h.div_ceil(4) * 16,
    }
}

/// Writes `data` into `buf` from byte `*at` as the budget allows; true when all of it is in.
pub fn buffer_step(gpu: &Gpu, buf: &wgpu::Buffer, data: &[u8], at: &mut u64) -> bool {
    let total = data.len() as u64;
    while *at < total && gpu.upload_room() > 0 {
        let n = gpu.piece(total - *at, 4);
        gpu.upload(n, || gpu.queue.put(buf, *at, &data[*at as usize..(*at + n) as usize]));
        *at += n;
    }
    *at >= total
}
