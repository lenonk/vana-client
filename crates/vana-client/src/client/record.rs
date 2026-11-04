//! Video capture (client.record, ours): the frames as drawn, copied into a ring of readback buffers, mapped without
//! waiting and piped raw into ffmpeg, which encodes them. A frame due with no buffer free sends the last one again,
//! and a frame drawn late fills every interval it covers, so the video keeps time; frames go out in the order they
//! were drawn whatever order their buffers map in. The game never waits on the GPU or the encoder.

use crate::render::Gpu;
use std::io::Write;
use std::collections::BTreeMap;
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
    /// Copied this frame: map it after the submit; the frame's number (the order it goes out in), and how many
    /// intervals it fills.
    copied: bool,
    seq: u64,
    times: u32,
}

/// What a frame number sends: its pixels, or the last frame again (no buffer was free, or the map failed); each
/// `times` times.
enum Out {
    Frame(Arc<Vec<u8>>, u32),
    Again(u32),
}

/// At most a second of intervals filled by one late frame (a long stall drops the rest instead of a burst).
fn due(late: Duration, every: Duration, fps: u32) -> u32 {
    ((late.as_secs_f64() / every.as_secs_f64()) as u32 + 1).min(fps.max(1))
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
    /// Frames for the encoder thread, each with how many intervals it fills (that thread writes it that many times, so
    /// a late frame is one item in the bounded queue, not a burst).
    tx: Option<SyncSender<(Arc<Vec<u8>>, u32)>>,
    last: Option<Arc<Vec<u8>>>,
    fps: u32,
    /// Frame numbers done but not yet sent (an earlier one is still mapping), and the next number to send.
    ready: BTreeMap<u64, Out>,
    next_seq: u64,
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
        let (tx, rx) = sync_channel::<(Arc<Vec<u8>>, u32)>(16);
        std::thread::Builder::new().name("record".into()).spawn(move || {
            'out: for (f, times) in rx {
                for _ in 0..times {
                    if stdin.write_all(&f).is_err() {
                        break 'out;
                    }
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
                times: 1,
            })
            .collect();
        let fps = fps.clamp(1, 120);
        let every = Duration::from_secs_f64(1.0 / fps as f64);
        Ok(Self { path, owner, size, row, every, next: Instant::now(), ring, tx: Some(tx), last: None, fps, ready: BTreeMap::new(), next_seq: 1, frames: 0, copies: 0 })
    }

    /// Before the submit: a frame due goes into a free buffer, else the last one is sent again; either fills every
    /// interval since the last frame (rendering slower than the recording's rate).
    pub fn copy(&mut self, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture) {
        let now = Instant::now();
        if now < self.next {
            return;
        }
        let times = due(now - self.next, self.every, self.fps);
        self.next = (self.next + self.every * times).max(now - self.every);
        self.copies += 1;
        let seq = self.copies;
        match self.ring.iter_mut().find(|s| s.state.load(Ordering::Acquire) == FREE) {
            Some(s) => {
                encoder.copy_texture_to_buffer(
                    frame.as_image_copy(),
                    wgpu::TexelCopyBufferInfo { buffer: &s.buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.row), rows_per_image: Some(self.size[1]) } },
                    wgpu::Extent3d { width: self.size[0], height: self.size[1], depth_or_array_layers: 1 },
                );
                s.state.store(PENDING, Ordering::Release);
                s.copied = true;
                s.seq = seq;
                s.times = times;
            }
            None => {
                self.ready.insert(seq, Out::Again(times));
            }
        }
    }

    fn send(&mut self, f: Arc<Vec<u8>>, times: u32) {
        if times > 0
            && let Some(tx) = &self.tx
            && tx.try_send((f, times)).is_ok()
        {
            self.frames += times as u64;
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
        self.collect();
        // In drawing order: a later frame waits for an earlier one still mapping.
        self.drain();
    }

    /// The buffers whose map finished: their pixels (or, when it failed, the last frame again) wait in `ready`.
    fn collect(&mut self) {
        let (w, h, row) = (self.size[0] as usize * 4, self.size[1] as usize, self.row as usize);
        for s in &self.ring {
            match s.state.load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(d) = s.buf.slice(..).get_mapped_range() {
                        let mut px = Vec::with_capacity(w * h);
                        for y in 0..h {
                            px.extend_from_slice(&d[y * row..y * row + w]);
                        }
                        self.ready.insert(s.seq, Out::Frame(Arc::new(px), s.times));
                    } else {
                        self.ready.insert(s.seq, Out::Again(s.times));
                    }
                    s.buf.unmap();
                    s.state.store(FREE, Ordering::Release);
                }
                FAILED => {
                    self.ready.insert(s.seq, Out::Again(s.times));
                    s.state.store(FREE, Ordering::Release);
                }
                _ => {}
            }
        }
    }

    /// Sends the frames done from the next number on, stopping at the first one not done yet.
    fn drain(&mut self) {
        while let Some(o) = self.ready.remove(&self.next_seq) {
            self.next_seq += 1;
            let (f, times) = match o {
                Out::Frame(f, n) => {
                    self.last = Some(f.clone());
                    (f, n)
                }
                Out::Again(n) => match self.last.clone() {
                    Some(l) => (l, n),
                    None => continue,
                },
            };
            self.send(f, times);
        }
    }

    /// Ends the input; ffmpeg finishes the file on its thread.
    pub fn stop(mut self) {
        // The frames already mapped go out; a copy still mapping keeps its time with the last frame.
        self.collect();
        let pending: Vec<(u64, u32)> = self.ring.iter().filter(|s| s.copied || s.state.load(Ordering::Acquire) == PENDING).map(|s| (s.seq, s.times)).collect();
        for (seq, times) in pending {
            self.ready.entry(seq).or_insert(Out::Again(times));
        }
        if let Some(&last) = self.ready.keys().next_back() {
            for seq in self.next_seq..=last {
                self.ready.entry(seq).or_insert(Out::Again(0));
            }
        }
        self.drain();
        self.tx = None;
        log::info!("record: {} frames to {}", self.frames, self.path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::due;
    use std::time::Duration;

    #[test]
    fn late_frames_fill_their_intervals() {
        let every = Duration::from_secs_f64(1.0 / 30.0);
        // On time: one interval.
        assert_eq!(due(Duration::ZERO, every, 30), 1);
        // Rendering at 15 fps into a 30 fps recording: each frame covers two intervals.
        assert_eq!(due(every, every, 30), 2);
        // A long stall fills at most a second.
        assert_eq!(due(Duration::from_secs(5), every, 30), 30);
    }
}
