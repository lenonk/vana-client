//! Title-screen flyover (file id 23, folder "titl"): type 0x07 schedulers and type 0x06 camera paths.
//! Played by the lobby loop (FUN_1023af40 -> FUN_1023ad60: scheduler "main") behind the title menu.

use anyhow::{Context, Result};
use glam::Vec3;

pub const TITLE_FILE: u32 = 23;

/// One camera key (0x30 bytes): eye, focal, target, roll, then an unused time.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub eye: Vec3,
    /// fov_y = 2 * atan(192 / focal) (FUN_100147a0); 350 is the default.
    pub focal: f32,
    pub target: Vec3,
    pub roll: f32,
}

#[derive(Clone, Debug)]
pub struct CameraPath {
    pub name: String,
    /// Payload u32 @0: the attach word (FUN_1003b900) that frames the keys on the scheduler's actor and target.
    pub attach: u32,
    /// Payload u32 @0x10: low byte key count; byte 1 / 255 how far the frame moves toward the actors each frame
    /// (FUN_1003d220); bit 19 puts the current camera first, bit 20 the player's follow camera last (FUN_1003be40).
    pub flags: u32,
    /// Payload u32 @0x14: 0 linear, 1 sin ease-out, 2 1-cos ease-in, 3 split sine, 4 ease in-out (FUN_1003c270).
    pub ease: u32,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug)]
pub enum Op {
    Zone(u32),
    Weather { tag: String, ticks: u16 },
    Time(u32),
    Env(String),
    Camera { path: String, wait: u16, ticks: u16 },
    Fade { rgba: [u8; 4], wait: u16, ticks: u16 },
    Crossfade { wait: u16, ticks: u16 },
    Call(String),
    Spawn(String),
    /// 0x3d (or 0xaf with an expansion bit): run one random entry up to the closing 0x3e.
    Random { expansion: Option<u32> },
    EndRandom,
    Other(u8),
}

fn tag(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// Scheduler ops after the 0x40-byte header: u8 op, u8 length in words, u16 @4 wait, u16 @6 duration (1/60 s).
pub fn parse_ops(pay: &[u8]) -> Vec<Op> {
    let mut out = Vec::new();
    let mut p = 0x40;
    while p + 2 <= pay.len() {
        let (op, len) = (pay[p], pay[p + 1] as usize);
        if len == 0 || p + 4 * len > pay.len() {
            break;
        }
        let b = &pay[p..p + 4 * len];
        let (wait, ticks) = if len >= 2 { (u16_at(b, 4), u16_at(b, 6)) } else { (0, 0) };
        out.push(match op {
            0x7b => Op::Zone(u32_at(b, 8)),
            0x7c => Op::Weather { tag: tag(&b[8..12]), ticks },
            0x7d => Op::Time(u32_at(b, 8)),
            0x7e => Op::Env(tag(&b[8..12])),
            0x04 => Op::Camera { path: tag(&b[8..12]), wait, ticks },
            0x0f => Op::Fade { rgba: b[8..12].try_into().unwrap(), wait, ticks },
            0x10 => Op::Crossfade { wait, ticks },
            0x3b => Op::Call(tag(&b[8..12])),
            0x73 => Op::Spawn(tag(&b[8..12])),
            0x3d => Op::Random { expansion: None },
            0xaf => Op::Random { expansion: Some(u32_at(b, 8)) },
            0x3e => Op::EndRandom,
            other => Op::Other(other),
        });
        p += 4 * len;
        if op == 0 {
            break;
        }
    }
    out
}

pub struct Title {
    pub schedulers: Vec<(String, Vec<Op>)>,
    pub paths: Vec<CameraPath>,
}

impl Title {
    pub fn parse(file: &[u8]) -> Result<Self> {
        let mut schedulers = Vec::new();
        let mut paths = Vec::new();
        for c in super::chunks(file)? {
            match c.kind {
                0x07 => schedulers.push((c.name(), parse_ops(c.data))),
                0x06 => {
                    let attach = u32_at(c.data, 0);
                    let flags = u32_at(c.data, 0x10);
                    let ease = u32_at(c.data, 0x14);
                    let keys = (0..(flags & 0xff) as usize)
                        .map(|i| {
                            let o = 0x20 + 0x30 * i;
                            let v = |o: usize| Vec3::new(f32_at(c.data, o), f32_at(c.data, o + 4), f32_at(c.data, o + 8));
                            Key { eye: v(o), focal: f32_at(c.data, o + 12), target: v(o + 16), roll: f32_at(c.data, o + 28) }
                        })
                        .collect();
                    paths.push(CameraPath { name: c.name(), attach, flags, ease, keys });
                }
                _ => {}
            }
        }
        Ok(Self { schedulers, paths })
    }

    pub fn path(&self, name: &str) -> Result<&CameraPath> {
        self.paths.iter().find(|p| p.name == name).with_context(|| format!("no camera path {name}"))
    }

    pub fn scheduler(&self, name: &str) -> Result<&[Op]> {
        self.schedulers.iter().find(|s| s.0 == name).map(|s| s.1.as_slice()).with_context(|| format!("no scheduler {name}"))
    }
}

/// Easing of the path parameter (FUN_1003c270).
pub fn ease(kind: u32, u: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match kind {
        1 => (u * FRAC_PI_2).sin(),
        2 => 1.0 - (u * FRAC_PI_2).cos(),
        3 => {
            if u * 2.0 * PI <= PI { (u * PI).sin() * 0.5 } else { 1.0 - (u * PI).sin() * 0.5 }
        }
        4 => 1.0 - ((u * PI).cos() * 0.5 + 0.5),
        _ => u,
    }
}

/// Chord-length cubic through points (FUN_10062600 build, FUN_100628e0 evaluate).
pub struct Spline {
    pts: Vec<Vec3>,
    lens: Vec<f32>,
    mats: Vec<[f32; 16]>,
}

impl Spline {
    pub fn new(keys: &[Vec3]) -> Self {
        let n = keys.len();
        let mut pts = Vec::with_capacity(n + 2);
        pts.push(keys[0] - (keys[1] - keys[0]));
        pts.extend_from_slice(keys);
        pts.push(keys[n - 1] + (keys[n - 1] - keys[n - 2]));
        let mut lens = vec![0f32; n + 1];
        let mut total = 0.0;
        for i in 1..n {
            let l = (pts[i + 1] - pts[i]).length();
            lens[i] = if l >= 0.01 { l } else { 0.01 };
            total += lens[i];
        }
        lens[0] = lens[1];
        lens[n] = lens[n - 1];
        let mut mats = Vec::with_capacity(n - 1);
        for i in 0..n - 1 {
            let f1 = lens[i] / (lens[i + 1] + lens[i]);
            let f2 = lens[i + 1] / (lens[i + 2] + lens[i + 1]);
            let (f3, f4, f5, f6) = (1.0 - f1, 1.0 / f1, f2 * f1, 1.0 - f2);
            let f8 = f3 * f3;
            let f7 = f2 * f2 / f6;
            mats.push([
                -f8 * f4, (f3 + f5) * f4, (-f3 - f5) / f6, f7,
                2.0 * f8 * f4, (f3 * -2.0 - f5) * f4, (2.0 * f3 - (f2 - 2.0 * f5)) / f6, -f7,
                -f8 * f4, (1.0 - 2.0 * f1) * f4, f1, 0.0,
                0.0, 1.0, 0.0, 0.0,
            ]);
        }
        for l in lens.iter_mut().take(n).skip(1) {
            *l /= total;
        }
        lens[0] = 0.0;
        Self { pts, lens, mats }
    }

    pub fn eval(&self, u: f32) -> Vec3 {
        let segs = self.mats.len();
        let mut c = 0.0;
        let mut seg = segs;
        let mut s = 1.0;
        for i in 0..segs {
            c += self.lens[i + 1];
            if u <= c {
                seg = i;
                s = (u - (c - self.lens[i + 1])) / self.lens[i + 1];
                break;
            }
        }
        if seg == segs {
            seg = segs - 1;
        }
        let m = &self.mats[seg];
        let v = [s * s * s, s * s, s, 1.0];
        let w: Vec<f32> = (0..4).map(|j| (0..4).map(|k| v[k] * m[k * 4 + j]).sum()).collect();
        self.pts[seg] * w[0] + self.pts[seg + 1] * w[1] + self.pts[seg + 2] * w[2] + self.pts[seg + 3] * w[3]
    }
}

pub const FROM_CURRENT: u32 = 1 << 19;
pub const TO_FOLLOW: u32 = 1 << 20;

/// Camera state on a path at normalized time t (0..1 of the op's duration), FUN_1003c3e0.
pub fn sample(path: &CameraPath, t: f32) -> Key {
    sample_keys(&path.keys, path.ease, t)
}

/// FUN_1003be40: with flag bit 19 the camera at the start is point 0 and with bit 20 the player's follow
/// camera (FUN_10020450) is the last; then lerp for 2 points, the chord-length splines from 3.
pub fn sample_from(path: &CameraPath, t: f32, current: Option<Key>, follow: Option<Key>) -> Key {
    let first = current.filter(|_| path.flags & FROM_CURRENT != 0);
    let last = follow.filter(|_| path.flags & TO_FOLLOW != 0);
    if first.is_none() && last.is_none() {
        return sample(path, t);
    }
    let keys: Vec<Key> = first.into_iter().chain(path.keys.iter().copied()).chain(last).collect();
    sample_keys(&keys, path.ease, t)
}

fn sample_keys(k: &[Key], ease_kind: u32, t: f32) -> Key {
    let u = ease(ease_kind, t.clamp(0.0, 1.0));
    if k.len() == 2 {
        let lerp = |a: f32, b: f32| a + (b - a) * u;
        return Key { eye: k[0].eye.lerp(k[1].eye, u), focal: lerp(k[0].focal, k[1].focal), target: k[0].target.lerp(k[1].target, u), roll: lerp(k[0].roll, k[1].roll) };
    }
    if k.len() < 2 {
        return k[0];
    }
    let eye = Spline::new(&k.iter().map(|k| k.eye).collect::<Vec<_>>()).eval(u);
    let target = Spline::new(&k.iter().map(|k| k.target).collect::<Vec<_>>()).eval(u);
    let third: Vec<Vec3> = k.iter().enumerate().map(|(i, k)| Vec3::new(k.focal, k.roll, i as f32)).collect();
    let f = Spline::new(&third).eval(u);
    Key { eye, focal: f.x, target, roll: f.y }
}

pub fn fov_y(focal: f32) -> f32 {
    2.0 * (192.0 / focal).atan()
}

pub fn focal_of(fov_y: f32) -> f32 {
    192.0 / (fov_y * 0.5).tan()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: f32) -> Key {
        Key { eye: Vec3::new(x, 0.0, 0.0), focal: 350.0, target: Vec3::new(x, 1.0, 0.0), roll: 0.0 }
    }

    /// A one-key bit-19 path lerps from the current camera; without the bit it holds its key.
    #[test]
    fn starts_from_current() {
        let p = CameraPath { name: "t".into(), attach: 0, flags: FROM_CURRENT | 1, ease: 0, keys: vec![key(10.0)] };
        assert_eq!(sample_from(&p, 0.0, Some(key(0.0)), None).eye.x, 0.0);
        assert_eq!(sample_from(&p, 0.5, Some(key(0.0)), None).eye.x, 5.0);
        assert_eq!(sample_from(&p, 1.0, Some(key(0.0)), None).eye.x, 10.0);
        let q = CameraPath { flags: 1, ..p.clone() };
        assert_eq!(sample_from(&q, 0.0, Some(key(0.0)), None).eye.x, 10.0);
        // Three points: the spline passes through the current camera first.
        let r = CameraPath { name: "t".into(), attach: 0, flags: FROM_CURRENT | 2, ease: 0, keys: vec![key(10.0), key(20.0)] };
        assert!((sample_from(&r, 0.0, Some(key(0.0)), None).eye.x).abs() < 1e-4);
        assert!((sample_from(&r, 1.0, Some(key(0.0)), None).eye.x - 20.0).abs() < 1e-3);
        assert!((focal_of(fov_y(350.0)) - 350.0).abs() < 1e-2);
    }
}

/// Vana'diel time value of op 0x7d: 216000 units per hour (FUN_10184f30).
pub fn hour_of(time: u32) -> f32 {
    (time % (216000 * 24)) as f32 / 216000.0
}
