//! Frame capture (`/upcapture`, ours): one or more frames' upscaler inputs written to
//! <data dir>/captures/<time>[-label]/frame_NNN/ as raw little-endian f32 files (row-major, top row first, channels
//! interleaved) and an inputs.json with the sizes, jitter and scalars, for offline work on an upscaler.

use super::Inputs;
use crate::render::Gpu;
use std::path::PathBuf;

/// A capture asked for: its folder and how many frames are still to come.
#[derive(Clone)]
pub struct Request {
    pub dir: PathBuf,
    pub frames: u32,
    pub index: u32,
}

impl Request {
    pub fn new(label: &str, frames: u32) -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let label: String = label.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        let name = if label.is_empty() { t.to_string() } else { format!("{t}-{label}") };
        Request { dir: crate::config::data_dir().join("captures").join(name), frames: frames.clamp(1, 240), index: 0 }
    }
}

struct Image {
    name: &'static str,
    buffer: wgpu::Buffer,
    size: [u32; 2],
    /// Bytes per texel as copied, the padded row, the channels and how a channel is stored (2 f16, 4 f32, 1 unorm8).
    texel: u32,
    row: u32,
    channels: u32,
}

pub struct Pending {
    req: Request,
    images: Vec<Image>,
    json: serde_json::Value,
}

/// Copies this frame's inputs out for reading after the submit.
pub fn record(gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, i: &Inputs, req: Request) -> Option<Pending> {
    let list: [(&'static str, &wgpu::Texture, u32, u32, wgpu::TextureAspect); 5] = [
        ("color", i.color, 8, 4, wgpu::TextureAspect::All),
        ("depth", i.depth, 4, 1, wgpu::TextureAspect::DepthOnly),
        ("motion", i.motion, 4, 2, wgpu::TextureAspect::All),
        ("reactive", i.reactive, 1, 1, wgpu::TextureAspect::All),
        ("composition", i.composition, 1, 1, wgpu::TextureAspect::All),
    ];
    let mut images = Vec::new();
    for (name, tex, texel, channels, aspect) in list {
        let size = [tex.width(), tex.height()];
        let row = (size[0] * texel).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upscale capture"),
            size: row as u64 * size[1] as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        images.push(Image { name, buffer, size, texel, row, channels });
    }
    let json = serde_json::json!({
        "frame": req.index,
        "render_size": i.render,
        "output_size": i.output,
        "jitter_px": i.jitter,
        "reset": i.reset,
        "frame_time_ms": i.dt_ms,
        "near": i.near,
        "far": i.far,
        "fov_y": i.fov_y,
        "reversed_z": i.reversed_z,
        "depth_range": "0..1, standard (near 0, far 1)",
        "meters_per_unit": i.meters_per_unit,
        "exposure": i.exposure,
        "sharpness": i.sharpness,
        "color": "linear RGBA, f32 x4 per pixel (from Rgba16Float)",
        "depth_file": "f32 x1 (Depth32Float)",
        "motion": "previous minus current position in render pixels, f32 x2, jitter removed (from Rg16Float)",
        "reactive": "f32 x1 in 0..1 (from R8Unorm)",
        "composition": "f32 x1 in 0..1 (from R8Unorm)",
        "layout": "row-major, top row first, little-endian",
    });
    Some(Pending { req, images, json })
}

pub fn f16_to_f32(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as u32, (h & 0x3ff) as u32);
    let bits = match e {
        0 if m == 0 => s << 31,
        0 => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = m;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            (s << 31) | ((e as u32) << 23) | ((m & 0x3ff) << 13)
        }
        31 => (s << 31) | 0x7f80_0000 | (m << 13),
        _ => (s << 31) | ((e + 127 - 15) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// Reads the copies back (waiting on the GPU) and writes the files; asks for the next frame of a sequence.
pub fn finish(gpu: &Gpu, p: Pending) -> Option<Request> {
    let dir = p.req.dir.join(format!("frame_{:03}", p.req.index));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("capture {}: {e}", dir.display());
        return None;
    }
    for img in &p.images {
        img.buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    }
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    for img in &p.images {
        let Ok(data) = img.buffer.slice(..).get_mapped_range() else { continue };
        let mut out: Vec<u8> = Vec::with_capacity((img.size[0] * img.size[1] * img.channels * 4) as usize);
        for y in 0..img.size[1] as usize {
            let row = &data[y * img.row as usize..][..(img.size[0] * img.texel) as usize];
            match img.texel / img.channels {
                2 => row
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .for_each(|c| out.extend(f16_to_f32(u16::from_le_bytes(*c)).to_le_bytes())),
                4 => out.extend_from_slice(row),
                _ => row.iter().for_each(|&b| out.extend((b as f32 / 255.0).to_le_bytes())),
            }
        }
        drop(data);
        img.buffer.unmap();
        if let Err(e) = std::fs::write(dir.join(format!("{}.f32", img.name)), &out) {
            log::warn!("capture {}: {e}", img.name);
        }
    }
    let mut json = p.json;
    json["files"] = serde_json::json!(
        p.images
            .iter()
            .map(|i| serde_json::json!({
                "name": format!("{}.f32", i.name), "width": i.size[0], "height": i.size[1], "channels": i.channels,
            }))
            .collect::<Vec<_>>()
    );
    let _ = std::fs::write(dir.join("inputs.json"), serde_json::to_string_pretty(&json).unwrap_or_default());
    log::info!("upscale capture: {}", dir.display());
    let next = Request { index: p.req.index + 1, ..p.req };
    (next.index < next.frames).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::f16_to_f32;

    #[test]
    fn halves() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x0001), 5.960_464_5e-8);
    }
}
