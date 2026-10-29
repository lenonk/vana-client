//! The opening movie before the title (config 0022; CXiOpening, FUN_10017a90): mov999.pmv decoded on its own thread
//! (mpeg1.rs) and shown on black, music999.bgw as the music; any input skips it.

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

use crate::mpeg1::Frame;

/// The movie's music, `<dir>\music999.bgw` at volume 0x7f (FUN_1023bce0; audio/format.rs music_path).
pub const MUSIC: u32 = 0x400;

/// The skip's wait: 240 ticks (case 3 of FUN_10017a90), the music fading over the first 60.
const SKIP_WAIT: Duration = Duration::from_secs(4);
pub const SKIP_FADE: i32 = 60;

pub struct Opening {
    /// Decoded pictures in display order; dropped on a skip (the movie object released, nothing drawn).
    frames: Option<Receiver<Frame>>,
    fps: f64,
    /// The picture's rows (the sequence header's height).
    pub rows: usize,
    opened: Instant,
    /// The movie clock: from the music's start, or a second after opening without it.
    clock: Option<Instant>,
    /// Pictures taken so far.
    taken: u64,
    /// The picture to put up this frame.
    pub fresh: Option<Frame>,
    skipped: Option<Instant>,
    ended: bool,
}

impl Opening {
    /// Opens `<dir>/mov999.pmv` and starts its decoder (FUN_10213720); None (done quietly) when it won't open.
    pub fn open(dir: &Path) -> Option<Self> {
        let mut d = crate::mpeg1::open_pmv(&dir.join("mov999.pmv"))?;
        let first = d.next_frame()?;
        let fps = if d.fps > 0.0 { d.fps } else { 30000.0 / 1001.0 };
        let d_rows = d.height;
        let (tx, rx) = sync_channel(8);
        std::thread::Builder::new()
            .name("opening movie".into())
            .spawn(move || {
                let mut next = Some(first);
                while let Some(f) = next {
                    if tx.send(f).is_err() {
                        return;
                    }
                    next = d.next_frame();
                }
            })
            .ok()?;
        Some(Self { frames: Some(rx), fps, rows: d_rows, opened: Instant::now(), clock: None, taken: 0, fresh: None, skipped: None, ended: false })
    }

    /// Any input (FUN_10007c70(0)): the movie stops; the caller fades the music over SKIP_FADE ticks.
    pub fn skip(&mut self) -> bool {
        if self.skipped.is_some() || self.ended {
            return false;
        }
        self.skipped = Some(Instant::now());
        self.frames = None;
        self.fresh = None;
        true
    }

    /// Whether a picture still shows (nothing after a skip).
    pub fn showing(&self) -> bool {
        self.frames.is_some() && self.taken > 0
    }

    /// Advances the clock and takes the picture due; false once the opening is over (the movie's end, or the wait
    /// after a skip).
    pub fn update(&mut self, music_started: bool) -> bool {
        let now = Instant::now();
        if let Some(t) = self.skipped {
            return now.duration_since(t) < SKIP_WAIT;
        }
        if self.ended {
            return false;
        }
        if self.clock.is_none() && (music_started || now.duration_since(self.opened) > Duration::from_secs(1)) {
            log::info!("opening: the movie starts at {:.2} fps (music {})", self.fps, if music_started { "playing" } else { "not started" });
            self.clock = Some(now);
        }
        let (Some(clock), Some(rx)) = (self.clock, &self.frames) else { return true };
        let due = now.duration_since(clock).as_secs_f64() * self.fps;
        // Late pictures are dropped; a decoder behind holds the last one.
        while self.taken as f64 <= due {
            match rx.try_recv() {
                Ok(f) => {
                    self.taken += 1;
                    self.fresh = Some(f);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The end: the movie object is released, nothing drawn.
                    self.ended = true;
                    self.frames = None;
                    break;
                }
            }
        }
        !self.ended
    }
}

/// FUN_10213810's quad in window pixels: x 0..W, y from (off) k to (512 + off) k with k = H x 0.00214534 and off -110
/// (English, no subtitles). The bottom-up RGB24 samples fill texture rows 0..335 and v runs 1 at the top to 0, so the
/// picture is the quad's bottom 336 of 512 rows: 0.142 H..0.862 H.
pub fn picture_rect(size: [u32; 2], rows: usize) -> [f32; 4] {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let k = h * 0.002_145_34;
    let bottom = (512.0 - 110.0) * k;
    [0.0, bottom - rows as f32 * k, w, bottom]
}

#[cfg(test)]
mod tests {
    /// The install's movie: a picture once the clock runs, none after a skip, and the wait still running.
    #[test]
    fn skip_stops_the_picture() {
        let cfg = crate::config::Config::load();
        let Some(mut o) = super::Opening::open(&cfg.ffxi_dir().join("mov")) else { return };
        assert_eq!(o.rows, 336);
        for _ in 0..100 {
            assert!(o.update(true));
            if o.fresh.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(o.showing() && o.fresh.is_some());
        assert!(o.skip());
        assert!(!o.skip());
        assert!(!o.showing() && o.fresh.is_none());
        assert!(o.update(true));
        assert_eq!(super::picture_rect([1280, 720], 336).map(|v| v.round()), [0.0, 102.0, 1280.0, 621.0]);
    }
}
