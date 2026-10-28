//! GPU time per frame stage for the F12 overlay (not part of retail): timestamps written between the frame's passes,
//! resolved into a readback buffer and read a few frames later, averaged over 2 s windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

const MARKS: u32 = 24;
const READS: usize = 3;

/// A readback buffer's state: free, its map asked, mapped, or failed.
const FREE: u8 = 0;
const ASKED: u8 = 1;
const MAPPED: u8 = 2;

struct Read {
    buf: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// The stage each timestamp ends, in order.
    labels: Vec<&'static str>,
    /// Copied into this frame, its map not asked yet.
    copied: bool,
}

pub struct GpuTime {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    reads: Vec<Read>,
    labels: Vec<&'static str>,
    /// Nanoseconds a tick.
    period: f64,
    /// This window's summed ms by stage, in first-seen order, and its frames.
    sums: Vec<(&'static str, f64)>,
    frames: u32,
    window: Instant,
    /// The last window's mean ms a frame by stage, and their total.
    pub ms: Vec<(&'static str, f32)>,
    pub total: f32,
    /// The first window went to the log; with FFXI_PERF every window does.
    logged: bool,
    log_all: bool,
}

impl GpuTime {
    /// None without timestamps between passes on this device.
    pub fn new(gpu: &super::Gpu) -> Option<Self> {
        let need = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !gpu.device.features().contains(need) {
            return None;
        }
        let set = gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu time"), ty: wgpu::QueryType::Timestamp, count: MARKS });
        let size = MARKS as u64 * 8;
        let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("gpu time resolve"), size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let reads = (0..READS)
            .map(|_| Read {
                buf: gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("gpu time read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
                state: Arc::new(AtomicU8::new(FREE)),
                labels: Vec::new(),
                copied: false,
            })
            .collect();
        Some(Self { set, resolve, reads, labels: Vec::new(), period: gpu.queue.get_timestamp_period() as f64, sums: Vec::new(), frames: 0, window: Instant::now(), ms: Vec::new(), total: 0.0, logged: false, log_all: std::env::var_os("FFXI_PERF").is_some() })
    }

    /// A timestamp ending stage `label` (the first of a frame starts it).
    pub fn mark(&mut self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        if (self.labels.len() as u32) < MARKS {
            encoder.write_timestamp(&self.set, self.labels.len() as u32);
            self.labels.push(label);
        }
    }

    /// Resolves this frame's timestamps into a free readback buffer; skipped while all are in flight.
    pub fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let labels = std::mem::take(&mut self.labels);
        if labels.len() < 2 {
            return;
        }
        let Some(r) = self.reads.iter_mut().find(|r| r.state.load(Ordering::Acquire) == FREE && !r.copied) else { return };
        let n = labels.len() as u32;
        encoder.resolve_query_set(&self.set, 0..n, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &r.buf, 0, n as u64 * 8);
        r.labels = labels;
        r.copied = true;
    }

    /// After the submit: maps what was copied and takes in what has come back.
    pub fn after_submit(&mut self) {
        for k in 0..self.reads.len() {
            let r = &mut self.reads[k];
            if r.copied {
                r.copied = false;
                r.state.store(ASKED, Ordering::Release);
                let state = r.state.clone();
                r.buf.slice(..).map_async(wgpu::MapMode::Read, move |res| state.store(if res.is_ok() { MAPPED } else { FREE }, Ordering::Release));
            } else if r.state.load(Ordering::Acquire) == MAPPED {
                let ticks: Vec<u64> = match r.buf.slice(..r.labels.len() as u64 * 8).get_mapped_range() {
                    Ok(v) => bytemuck::cast_slice(&v).to_vec(),
                    Err(_) => Vec::new(),
                };
                r.buf.unmap();
                r.state.store(FREE, Ordering::Release);
                let labels = std::mem::take(&mut r.labels);
                self.take(&labels, &ticks);
            }
        }
        if self.window.elapsed() >= Duration::from_secs(2) && self.frames > 0 {
            let n = self.frames as f64;
            self.ms = self.sums.iter().map(|(l, s)| (*l, (s / n) as f32)).collect();
            self.total = self.ms.iter().map(|(_, v)| v).sum();
            // The first window always (a check that the timestamps come back); every one with FFXI_PERF.
            if !self.logged || self.log_all {
                self.logged = true;
                log::info!("{}", self.line());
            }
            self.sums.clear();
            self.frames = 0;
            self.window = Instant::now();
        }
    }

    fn take(&mut self, labels: &[&'static str], ticks: &[u64]) {
        if ticks.len() != labels.len() {
            return;
        }
        for i in 1..ticks.len() {
            let ms = ticks[i].saturating_sub(ticks[i - 1]) as f64 * self.period / 1e6;
            match self.sums.iter_mut().find(|(l, _)| *l == labels[i]) {
                Some(s) => s.1 += ms,
                None => self.sums.push((labels[i], ms)),
            }
        }
        self.frames += 1;
    }

    /// The overlay line: total, then each stage over 0.05 ms.
    pub fn line(&self) -> String {
        let parts: Vec<String> = self.ms.iter().filter(|(_, v)| *v >= 0.05).map(|(l, v)| format!("{l} {v:.2}")).collect();
        format!("gpu {:.2} ms/frame: {}", self.total, parts.join("  "))
    }
}
