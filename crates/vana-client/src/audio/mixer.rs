//! The sound thread: one BGM stream and 12 effect voices, with the client's volume ramps (50 Hz ticks)
//! and DirectSound volume curve (FUN_102408b0, FUN_10240090, FUN_1023e290, FUN_1023e3d0, FUN_1023e1f0).

use super::format::Decoder;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Receiver;

/// Volume level 0..127 -> linear 0..4096 (FFXiMain 0x1037c338).
pub const LEVEL: [i32; 128] = [
    0, 2340, 2511, 2633, 2727, 2804, 2869, 2925, 2975, 3019, 3060, 3096, 3130, 3161, 3191, 3218, 3243, 3268, 3290, 3312, 3333, 3352, 3371, 3389,
    3406, 3423, 3439, 3454, 3469, 3483, 3497, 3510, 3523, 3536, 3548, 3560, 3572, 3583, 3594, 3605, 3615, 3625, 3635, 3645, 3654, 3664, 3673, 3682,
    3690, 3699, 3707, 3715, 3723, 3731, 3739, 3747, 3754, 3761, 3769, 3776, 3783, 3790, 3796, 3803, 3809, 3816, 3822, 3829, 3835, 3841, 3847, 3853,
    3858, 3864, 3870, 3875, 3881, 3886, 3892, 3897, 3902, 3908, 3913, 3918, 3923, 3928, 3933, 3937, 3942, 3947, 3951, 3956, 3961, 3965, 3970, 3974,
    3978, 3983, 3987, 3991, 3996, 4000, 4004, 4008, 4012, 4016, 4020, 4024, 4028, 4032, 4035, 4039, 4043, 4047, 4050, 4054, 4058, 4061, 4065, 4068,
    4072, 4075, 4079, 4082, 4085, 4089, 4092, 4096,
];

/// Pan distance from centre -> volume scale (FFXiMain 0x1037c238).
const PAN_SCALE: [i32; 64] = [
    4096, 4094, 4092, 4090, 4088, 4086, 4084, 4082, 4081, 4079, 4077, 4075, 4073, 4071, 4069, 4067, 4065, 4063, 4061, 4059, 4057, 4055, 4053, 4051,
    4050, 4048, 4046, 4044, 4042, 4040, 4038, 4036, 4034, 4032, 4030, 4028, 4026, 4024, 4022, 4020, 4019, 4017, 4015, 4013, 4011, 4009, 4007, 4005,
    4003, 4001, 3999, 3997, 3995, 3993, 3991, 3989, 3988, 3986, 3984, 3982, 3980, 3978, 3976, 3974,
];

/// Effect voices: the Config tool's 0029 allows up to 20 at once (FUN_10037510 gates on it).
const VOICES: usize = 20;

/// Linear 0..4096 and pan 0..127 (0x40 centre) -> DirectSound (volume, pan) in centibels (FUN_1023e1f0).
pub fn dsound(vol: i32, pan: u8) -> (i32, i32) {
    let p = (pan & 0x7f) as i32;
    let (pan_cb, idx) = if p == 0x40 {
        (0, 0x3f)
    } else if p < 0x40 {
        (-((((p & 0x3f) ^ 0x3f) * 10000) / 0x3f), p)
    } else {
        (((p & 0x3f) * 10000) / 0x3f, 0x7f - p)
    };
    let vol_cb = ((((PAN_SCALE[idx as usize] * vol) >> 12) * 10000) >> 12) - 10000;
    (vol_cb.clamp(-10000, 0), pan_cb.clamp(-10000, 10000))
}

/// DirectSound gains for (left, right) from centibels.
fn gains(vol_cb: i32, pan_cb: i32) -> (f32, f32) {
    let a = |cb: i32| 10f32.powf(cb as f32 / 2000.0);
    let g = if vol_cb <= -10000 { 0.0 } else { a(vol_cb) };
    let l = if pan_cb > 0 { a(-pan_cb) } else { 1.0 };
    let r = if pan_cb < 0 { a(pan_cb) } else { 1.0 };
    (g * l, g * r)
}

/// A level change over 60 Hz ticks becomes 50 Hz steps (`ticks * 50 / 60`).
fn steps(ticks60: i32) -> i32 {
    ticks60 * 0x32 / 0x3c
}

/// The client's fixed-point ramp: value << 12, steps left, step.
#[derive(Clone, Copy, Default)]
struct Ramp {
    cur: i32,
    left: i32,
    delta: i32,
}

impl Ramp {
    fn set(&mut self, level: u8, ticks60: i32) {
        let target = LEVEL[(level & 0x7f) as usize];
        let n = steps(ticks60);
        if n == 0 {
            *self = Ramp { cur: target << 12, left: 1, delta: 0 };
        } else {
            self.left = n;
            self.delta = ((target * 0x1000 - self.cur) + 0x800) / n;
        }
    }

    /// 0..4096 now.
    fn value(&self) -> i32 {
        self.cur >> 12
    }

    /// One 50 Hz step; true when this step finished the ramp.
    fn tick(&mut self) -> bool {
        if self.left > 0 {
            self.left -= 1;
            self.cur += self.delta;
            return self.left < 1;
        }
        false
    }
}

pub enum Cmd {
    /// Start music (cmd 7): ignored while the same id plays. `ticks` fades in from level 1.
    PlayBgm { id: u32, bytes: Arc<[u8]>, level: u8, ticks: i32 },
    /// Ramp the music to a level (cmd 8).
    BgmVolume { level: u8, ticks: i32 },
    /// Ramp to level 1 and stop (cmd 9; 0 ticks = silence now).
    BgmFadeOut { ticks: i32 },
    BgmStop,
    /// Remember where the music is, for when it comes back (cmd 10).
    SavePosition,
    /// Forget saved positions: one id or all (cmd 11).
    ClearPositions(Option<u32>),
    /// Start an effect (cmd 2); `handle` (0 = none) names the voice for the commands below.
    /// `limit`: busy voices above which a new one steals instead of taking a free slot (0 = every slot).
    PlaySe { bytes: Arc<[u8]>, level: u8, pan: u8, priority: i32, handle: u32, limit: u8 },
    /// Cmd 4 / 3: a voice's level / pan, at once.
    SeLevel { handle: u32, level: u8 },
    SePan { handle: u32, pan: u8 },
    /// Cmd 0 (ticks 0, stop now) / cmd 5 (fade to level 1 over ticks, then stop).
    SeStop { handle: u32, ticks: i32 },
}

struct Source {
    dec: Decoder,
    /// Read position in source frames (fractional) and the two frames around it.
    frac: f64,
    step: f64,
    prev: [f32; 2],
    next: [f32; 2],
    done: bool,
}

impl Source {
    fn new(dec: Decoder, out_rate: u32) -> Self {
        let step = dec.header.rate as f64 / out_rate as f64;
        let mut s = Self { dec, frac: 0.0, step, prev: [0.0; 2], next: [0.0; 2], done: false };
        s.advance();
        s.advance();
        s
    }

    fn advance(&mut self) {
        self.prev = self.next;
        let mut f = [0i16; 2];
        let ch = self.dec.channels();
        if self.dec.read(&mut f[..ch]) == 0 {
            self.next = [0.0; 2];
            self.done = true;
            return;
        }
        let l = f[0] as f32 / 32768.0;
        self.next = if ch == 2 { [l, f[1] as f32 / 32768.0] } else { [l, l] };
    }

    /// One output frame, linearly interpolated.
    fn frame(&mut self) -> [f32; 2] {
        let t = self.frac as f32;
        let out = [self.prev[0] + (self.next[0] - self.prev[0]) * t, self.prev[1] + (self.next[1] - self.prev[1]) * t];
        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            self.advance();
        }
        out
    }

    fn finished(&self) -> bool {
        self.done && self.dec.ended()
    }
}

struct Bgm {
    id: u32,
    src: Source,
    ramp: Ramp,
    stop_after: bool,
    file_level: u8,
}

struct Voice {
    src: Source,
    ramp: Ramp,
    pan: u8,
    priority: i32,
    started: u64,
    handle: u32,
    stop_after: bool,
    /// Flag-0x20 files (the ambient loops): +0xd0 = 0x40, never stolen (FUN_1024a2c0).
    kept: bool,
}

/// Shared with the game thread: what the mixer is doing (for tests and the music logic).
#[derive(Default)]
pub struct Status {
    /// Music id playing + 1 (0 = none).
    pub bgm: AtomicU32,
    pub voices: AtomicU32,
    /// Handles of the voices still playing (FUN_1023ba50's list), refreshed every mix.
    pub alive: std::sync::Mutex<Vec<u32>>,
    /// Handles whose play request the mixer (or the loader, on a failed read) has taken, since the last read.
    pub taken: std::sync::Mutex<Vec<u32>>,
}

pub struct Mixer {
    /// Play requests taken this mix, published to `Status::taken`.
    taken_now: Vec<u32>,
    rx: Receiver<Cmd>,
    rate: u32,
    bgm: Option<Bgm>,
    voices: Vec<Option<Voice>>,
    /// Saved music positions by id (units), cmd 10 / 11.
    resume: Vec<u64>,
    tick_len: u32,
    tick_left: u32,
    serial: u64,
    status: Arc<Status>,
}

impl Mixer {
    pub fn new(rx: Receiver<Cmd>, rate: u32, status: Arc<Status>) -> Self {
        let tick_len = (rate / 50).max(1);
        Self { rx, rate, bgm: None, voices: (0..VOICES).map(|_| None).collect(), resume: vec![0; 0x400], tick_len, tick_left: tick_len, serial: 0, status, taken_now: Vec::new() }
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::PlayBgm { id, bytes, level, ticks } => {
                if self.bgm.as_ref().is_some_and(|b| b.id == id && id != 0x400) {
                    return;
                }
                let Ok(mut dec) = Decoder::new(bytes) else { return };
                let slot = (id as usize) & 0x3ff;
                if self.resume[slot] != 0 {
                    dec.seek_unit(self.resume[slot]);
                    self.resume[slot] = 0;
                }
                let target = LEVEL[(level & 0x7f) as usize];
                let n = steps(ticks);
                let ramp = if n == 0 {
                    Ramp { cur: target << 12, left: 1, delta: 0 }
                } else {
                    Ramp { cur: LEVEL[1] << 12, left: n, delta: (((target - LEVEL[1]) * 0x1000) | 0x800) / n }
                };
                let file_level = dec.header.volume & 0x7f;
                self.bgm = Some(Bgm { id, src: Source::new(dec, self.rate), ramp, stop_after: false, file_level });
            }
            Cmd::BgmVolume { level, ticks } => {
                if let Some(b) = &mut self.bgm {
                    b.ramp.set(level, ticks);
                }
            }
            Cmd::BgmFadeOut { ticks } => {
                if let Some(b) = &mut self.bgm {
                    let n = steps(ticks);
                    if n == 0 {
                        b.ramp = Ramp { cur: LEVEL[0] << 12, left: 1, delta: 0 };
                    } else {
                        b.ramp.left = n;
                        b.ramp.delta = ((LEVEL[1] * 0x1000 - b.ramp.cur) + 0x800) / n;
                    }
                    b.stop_after = true;
                }
            }
            Cmd::BgmStop => self.bgm = None,
            Cmd::SavePosition => {
                if let Some(b) = &self.bgm
                    && b.id < 0x400
                {
                    self.resume[b.id as usize] = b.src.dec.unit_position();
                }
            }
            Cmd::ClearPositions(id) => match id {
                None => self.resume.iter_mut().for_each(|r| *r = 0),
                Some(i) if i < 0x400 => self.resume[i as usize] = 0,
                _ => {}
            },
            Cmd::SeLevel { handle, level } => {
                for v in self.voices.iter_mut().flatten().filter(|v| v.handle == handle && handle != 0) {
                    v.ramp.set(level, 0);
                }
            }
            Cmd::SePan { handle, pan } => {
                for v in self.voices.iter_mut().flatten().filter(|v| v.handle == handle && handle != 0) {
                    v.pan = pan;
                }
            }
            Cmd::SeStop { handle, ticks } => {
                for slot in self.voices.iter_mut().filter(|v| v.as_ref().is_some_and(|v| v.handle == handle && handle != 0)) {
                    let n = steps(ticks);
                    let v = slot.as_mut().unwrap();
                    if n == 0 {
                        *slot = None;
                    } else {
                        v.ramp.left = n;
                        v.ramp.delta = ((LEVEL[1] * 0x1000 - v.ramp.cur) + 0x800) / n;
                        v.stop_after = true;
                    }
                }
            }
            Cmd::PlaySe { bytes, level, pan, priority, handle, limit } => {
                if handle != 0 {
                    self.taken_now.push(handle);
                }
                let Ok(dec) = Decoder::new(bytes) else { return };
                let flags = dec.header.flags;
                let mut ramp = Ramp { cur: LEVEL[(level & 0x7f) as usize] << 12, left: 0, delta: 0 };
                if flags & 0x20 != 0 && flags & 0x80 == 0 {
                    // Fade in over 150 steps (FUN_10240090).
                    ramp = Ramp { cur: LEVEL[1] << 12, left: 0x96, delta: (((LEVEL[(level & 0x7f) as usize] - LEVEL[1]) * 0x1000) | 0x800) / 0x96 };
                }
                // Flag 0x20 (ambient loops): priority 20000 and kept from stealing.
                let kept = flags & 0x20 != 0;
                let priority = if kept { 20000 } else { priority };
                // A free voice from the end (FUN_1023fc40) while under the limit, else the lowest priority below ours,
                // oldest first (FUN_1023fb90).
                let busy = self.voices.iter().flatten().count();
                let free = limit == 0 || busy < limit as usize;
                let slot = (0..VOICES).rev().find(|&i| free && self.voices[i].is_none()).or_else(|| {
                    (0..VOICES).filter(|&i| self.voices[i].as_ref().is_some_and(|v| !v.kept && v.priority <= priority)).min_by_key(|&i| {
                        let v = self.voices[i].as_ref().unwrap();
                        (v.priority, v.started)
                    })
                });
                if let Some(i) = slot {
                    self.serial += 1;
                    self.voices[i] = Some(Voice { src: Source::new(dec, self.rate), ramp, pan, priority, started: self.serial, handle, stop_after: false, kept });
                }
            }
        }
    }

    fn tick(&mut self) {
        if let Some(b) = &mut self.bgm
            && b.ramp.tick()
            && b.stop_after
        {
            self.bgm = None;
        }
        for slot in self.voices.iter_mut() {
            if let Some(v) = slot
                && v.ramp.tick()
                && v.stop_after
            {
                *slot = None;
            }
        }
    }

    /// Mixes interleaved stereo f32 frames.
    pub fn render(&mut self, out: &mut [f32]) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.command(cmd);
        }
        out.fill(0.0);
        let mut at = 0;
        let frames = out.len() / 2;
        while at < frames {
            let run = (self.tick_left as usize).min(frames - at);
            let buf = &mut out[at * 2..(at + run) * 2];
            if let Some(b) = &mut self.bgm {
                let v = (LEVEL[b.file_level as usize] * b.ramp.value()) >> 12;
                let (cb, pan) = dsound(v, 0x40);
                let (gl, gr) = gains(cb, pan);
                for f in buf.chunks_exact_mut(2) {
                    let s = b.src.frame();
                    f[0] += s[0] * gl;
                    f[1] += s[1] * gr;
                }
                if b.src.finished() {
                    self.bgm = None;
                }
            }
            for slot in self.voices.iter_mut() {
                let Some(v) = slot else { continue };
                let (cb, pan) = dsound(v.ramp.value(), v.pan);
                let (gl, gr) = gains(cb, pan);
                for f in buf.chunks_exact_mut(2) {
                    let s = v.src.frame();
                    f[0] += s[0] * gl;
                    f[1] += s[1] * gr;
                }
                if v.src.finished() {
                    *slot = None;
                }
            }
            at += run;
            self.tick_left -= run as u32;
            if self.tick_left == 0 {
                self.tick_left = self.tick_len;
                self.tick();
            }
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        self.status.bgm.store(self.bgm.as_ref().map_or(0, |b| b.id + 1), Ordering::Relaxed);
        self.status.voices.store(self.voices.iter().flatten().count() as u32, Ordering::Relaxed);
        if let Ok(mut a) = self.status.alive.lock() {
            a.clear();
            a.extend(self.voices.iter().flatten().map(|v| v.handle).filter(|&h| h != 0));
        }
        // After `alive`, so a taken handle is always in the list it is checked against.
        if let Ok(mut t) = self.status.taken.lock() {
            t.extend(self.taken_now.drain(..));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_level_centre_is_about_minus_3_db() {
        // 3974 * 4096 >> 12 = 3974; 3974 * 10000 >> 12 = 9702 -> -298 cB.
        assert_eq!(dsound(4096, 0x40), (-298, 0));
        assert_eq!(dsound(0, 0x40).0, -10000);
        assert_eq!(dsound(4096, 0).1, -10000);
        assert_eq!(dsound(4096, 0x7f).1, 10000);
    }

    #[test]
    fn ramp_reaches_target() {
        let mut r = Ramp { cur: LEVEL[1] << 12, left: 0, delta: 0 };
        r.set(127, 60);
        let mut n = 0;
        while !r.tick() {
            n += 1;
        }
        assert_eq!(n + 1, 50);
        assert!((r.value() - 4096).abs() <= 1);
    }
}
