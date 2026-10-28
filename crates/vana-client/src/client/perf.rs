//! Frame timing for the F12 overlay and FFXI_PERF=1 log lines (not part of retail). With FFXI_PERF set, every
//! frame whose start-to-start gap is over FFXI_PERF_SPIKE ms (default 25) is logged with its spans and the
//! notes of what was loaded or created in it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Spans of one redraw, in the order they run.
pub const SPANS: [&str; 6] = ["update", "ui", "acquire", "actors", "encode", "present"];

static ON: AtomicBool = AtomicBool::new(false);
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Notes work done this frame for the spike log (only collected with FFXI_PERF).
pub fn note(what: impl FnOnce() -> String) {
    if ON.load(Ordering::Relaxed)
        && let Ok(mut n) = NOTES.lock()
    {
        n.push(what());
    }
}

/// Runs `f`; with FFXI_PERF, notes it when it took 0.5 ms or more.
pub fn timed<T>(what: impl FnOnce() -> String, f: impl FnOnce() -> T) -> T {
    if !ON.load(Ordering::Relaxed) {
        return f();
    }
    let t = Instant::now();
    let r = f();
    let ms = t.elapsed().as_secs_f32() * 1000.0;
    if ms >= 0.5 {
        note(|| format!("{} {ms:.1} ms", what()));
    }
    r
}

#[derive(Default, Clone, Copy)]
pub struct Counts {
    pub entities: usize,
    pub actors: usize,
    pub posed: usize,
    pub drawn: usize,
    pub zone_draws: usize,
    pub ui_vertices: usize,
}

pub struct Perf {
    window: Instant,
    frames: u32,
    busy: Duration,
    worst: Duration,
    spans: [Duration; SPANS.len()],
    /// Previous frame start, for the gap; spikes and the worst gap in this window.
    last_start: Option<Instant>,
    spike: Duration,
    spikes: u32,
    worst_gap: Duration,
    /// Last finished window: fps, mean / worst CPU ms per frame, mean ms per span.
    pub fps: f32,
    pub cpu_ms: f32,
    pub worst_ms: f32,
    pub span_ms: [f32; SPANS.len()],
    /// Buffer bytes written to the GPU (render::WRITTEN) in this window, and their mean KB per frame in the last.
    written: u64,
    pub upload_kb: f32,
    pub counts: Counts,
    /// The frame limit and present mode, for the overlay.
    pub limit: String,
    log: bool,
}

impl Default for Perf {
    fn default() -> Self {
        let log = std::env::var_os("FFXI_PERF").is_some();
        ON.store(log, Ordering::Relaxed);
        let spike = std::env::var("FFXI_PERF_SPIKE").ok().and_then(|v| v.parse().ok()).unwrap_or(25.0f32);
        Self {
            window: Instant::now(),
            frames: 0,
            busy: Duration::ZERO,
            worst: Duration::ZERO,
            spans: [Duration::ZERO; SPANS.len()],
            last_start: None,
            spike: Duration::from_secs_f32(spike / 1000.0),
            spikes: 0,
            worst_gap: Duration::ZERO,
            fps: 0.0,
            cpu_ms: 0.0,
            worst_ms: 0.0,
            span_ms: [0.0; SPANS.len()],
            written: 0,
            upload_kb: 0.0,
            counts: Counts::default(),
            limit: String::new(),
            log,
        }
    }
}

impl Perf {
    /// One redraw: its span end times from `start`; the CPU time excludes the acquire wait.
    pub fn frame(&mut self, start: Instant, marks: [Instant; SPANS.len()]) {
        let mut prev = start;
        let mut this = [Duration::ZERO; SPANS.len()];
        for (i, m) in marks.iter().enumerate() {
            this[i] = m.saturating_duration_since(prev);
            self.spans[i] += this[i];
            prev = *m;
        }
        let busy = marks[SPANS.len() - 1].saturating_duration_since(start) - marks[2].saturating_duration_since(marks[1]);
        self.busy += busy;
        self.worst = self.worst.max(busy);
        self.frames += 1;
        self.written += crate::render::WRITTEN.swap(0, Ordering::Relaxed);
        let gap = self.last_start.map_or(Duration::ZERO, |l| start.saturating_duration_since(l));
        self.last_start = Some(start);
        let notes = if self.log { NOTES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default() } else { Vec::new() };
        self.worst_gap = self.worst_gap.max(gap);
        if self.log && (gap > self.spike || busy > self.spike) {
            self.spikes += 1;
            let ms = |d: Duration| d.as_secs_f32() * 1000.0;
            let spans: Vec<String> = SPANS.iter().zip(this).map(|(s, d)| format!("{s} {:.1}", ms(d))).collect();
            // The gap less this redraw is time spent before it started (events, the pacing wait).
            let before = gap.saturating_sub(marks[SPANS.len() - 1].saturating_duration_since(start));
            log::warn!("spike: gap {:.1} ms (before redraw {:.1}, busy {:.1}); {}; {}", ms(gap), ms(before), ms(busy), spans.join(", "), if notes.is_empty() { "nothing noted".to_string() } else { notes.join("; ") });
        }
        let el = self.window.elapsed();
        if el >= Duration::from_secs(2) {
            let n = self.frames.max(1) as f32;
            self.fps = self.frames as f32 / el.as_secs_f32();
            self.cpu_ms = self.busy.as_secs_f32() * 1000.0 / n;
            self.worst_ms = self.worst.as_secs_f32() * 1000.0;
            self.span_ms = self.spans.map(|d| d.as_secs_f32() * 1000.0 / n);
            self.upload_kb = self.written as f32 / 1024.0 / n;
            if self.log {
                let spans: Vec<String> = SPANS.iter().zip(self.span_ms).map(|(s, v)| format!("{s} {v:.2}")).collect();
                let c = self.counts;
                log::warn!(
                    "perf: {:.1} fps ({}), cpu {:.2} ms (worst {:.2}), worst gap {:.1} ms, spikes {}; {}; entities {} actors {} posed {} drawn {} zone draws {} ui verts {}, upload {:.0} KB/frame",
                    self.fps,
                    self.limit,
                    self.cpu_ms,
                    self.worst_ms,
                    self.worst_gap.as_secs_f32() * 1000.0,
                    self.spikes,
                    spans.join(", "),
                    c.entities,
                    c.actors,
                    c.posed,
                    c.drawn,
                    c.zone_draws,
                    c.ui_vertices,
                    self.upload_kb
                );
            }
            self.window = Instant::now();
            self.frames = 0;
            self.written = 0;
            self.busy = Duration::ZERO;
            self.worst = Duration::ZERO;
            self.spans = [Duration::ZERO; SPANS.len()];
            self.spikes = 0;
            self.worst_gap = Duration::ZERO;
        }
    }

    /// Overlay lines.
    pub fn lines(&self) -> Vec<String> {
        let c = self.counts;
        let spans: Vec<String> = SPANS.iter().zip(self.span_ms).map(|(s, v)| format!("{s} {v:.1}")).collect();
        vec![
            format!("{:.1} fps ({})   cpu {:.2} ms/frame (worst {:.1})", self.fps, self.limit, self.cpu_ms, self.worst_ms),
            spans.join("  "),
            format!("entities {}  actors {} (posed {}, drawn {})  zone draws {}  ui vertices {}", c.entities, c.actors, c.posed, c.drawn, c.zone_draws, c.ui_vertices),
            format!("upload {:.0} KB/frame", self.upload_kb),
        ]
    }
}
