//! Video capture (client.record, ours): the frames as drawn, copied into a ring of readback buffers, mapped without
//! waiting and piped raw into ffmpeg, which encodes them. A frame due with no buffer free sends the last one again,
//! so the video keeps time; the game never waits on the GPU or the encoder.

use crate::render::Gpu;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::{Duration, Instant};

const FREE: u8 = 0;
const PENDING: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

struct Slot {
    buf: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Copied this frame: map it after the submit; the frame's number, so frames mapped together go out in order.
    copied: bool,
    seq: u64,
}

pub struct Recorder {
    pub path: PathBuf,
    /// The mod that started it (it stops when the mod unloads).
    pub owner: String,
    pub size: [u32; 2],
    row: u32,
    every: Duration,
    next: Instant,
    ring: Vec<Slot>,
    tx: Option<SyncSender<Arc<Vec<u8>>>>,
    last: Option<Arc<Vec<u8>>>,
    /// Frames sent to the encoder (repeats included), and frames copied.
    pub frames: u64,
    copies: u64,
}

impl Recorder {
    /// Starts ffmpeg on `path` for frames of `size` in the surface's `format`.
    pub fn start(gpu: &Gpu, size: [u32; 2], format: wgpu::TextureFormat, path: PathBuf, fps: u32, owner: String) -> anyhow::Result<Self> {
        let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut child = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", if bgra { "bgra" } else { "rgba" }, "-s", &format!("{}x{}", size[0], size[1]), "-r", &fps.to_string(), "-i", "-"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p"])
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("ffmpeg: {e}"))?;
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("ffmpeg: no stdin"))?;
        let (tx, rx) = sync_channel::<Arc<Vec<u8>>>(16);
        std::thread::Builder::new().name("record".into()).spawn(move || {
            for f in rx {
                if stdin.write_all(&f).is_err() {
                    break;
                }
            }
            drop(stdin);
            let _ = child.wait();
        })?;
        let row = (size[0] * 4).div_ceil(256) * 256;
        let ring = (0..4)
            .map(|_| Slot {
                buf: gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("record"), size: (row * size[1]) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
                state: Arc::new(AtomicU8::new(FREE)),
                copied: false,
                seq: 0,
            })
            .collect();
        let every = Duration::from_secs_f64(1.0 / fps.clamp(1, 120) as f64);
        Ok(Self { path, owner, size, row, every, next: Instant::now(), ring, tx: Some(tx), last: None, frames: 0, copies: 0 })
    }

    /// Before the submit: a frame due goes into a free buffer, else the last one is sent again.
    pub fn copy(&mut self, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture) {
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = (self.next + self.every).max(now - self.every);
        match self.ring.iter_mut().find(|s| s.state.load(Ordering::Acquire) == FREE) {
            Some(s) => {
                encoder.copy_texture_to_buffer(
                    frame.as_image_copy(),
                    wgpu::TexelCopyBufferInfo { buffer: &s.buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.row), rows_per_image: Some(self.size[1]) } },
                    wgpu::Extent3d { width: self.size[0], height: self.size[1], depth_or_array_layers: 1 },
                );
                s.state.store(PENDING, Ordering::Release);
                s.copied = true;
                self.copies += 1;
                s.seq = self.copies;
            }
            None => {
                if let Some(l) = self.last.clone() {
                    self.send(l);
                }
            }
        }
    }

    fn send(&mut self, f: Arc<Vec<u8>>) {
        if let Some(tx) = &self.tx
            && tx.try_send(f).is_ok()
        {
            self.frames += 1;
        }
    }

    /// After the submit: this frame's copy is mapped, and each mapped buffer's pixels go to the encoder.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        for s in self.ring.iter_mut() {
            if !std::mem::take(&mut s.copied) {
                continue;
            }
            let state = s.state.clone();
            s.buf.slice(..).map_async(wgpu::MapMode::Read, move |r| state.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release));
        }
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        let (w, h, row) = (self.size[0] as usize * 4, self.size[1] as usize, self.row as usize);
        let mut out: Vec<(u64, Arc<Vec<u8>>)> = Vec::new();
        for s in &self.ring {
            match s.state.load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(d) = s.buf.slice(..).get_mapped_range() {
                        let mut px = Vec::with_capacity(w * h);
                        for y in 0..h {
                            px.extend_from_slice(&d[y * row..y * row + w]);
                        }
                        out.push((s.seq, Arc::new(px)));
                    }
                    s.buf.unmap();
                    s.state.store(FREE, Ordering::Release);
                }
                FAILED => s.state.store(FREE, Ordering::Release),
                _ => {}
            }
        }
        out.sort_by_key(|f| f.0);
        for (_, f) in out {
            self.last = Some(f.clone());
            self.send(f);
        }
    }

    /// Ends the input; ffmpeg finishes the file on its thread.
    pub fn stop(mut self) {
        self.tx = None;
        log::info!("record: {} frames to {}", self.frames, self.path.display());
    }
}
