//! The weapon after-image (scheduler op 0x2c, CYyAfterImage FUN_1001bc80; research fx_afterimage_spec.md): an Afb's
//! two rails (blade tip A, grip B) placed through the actor's root, smoothed into an 80-sample ring that shrinks and
//! fades. The CPU takes the Afb points and pushes samples once a retail frame (2 ticks), as retail's update does; each
//! sample's aging is a closed form of its birth, evaluated per vertex on the GPU (effects.wgsl `vs_trail`) and here
//! only for the trim.

use glam::{Mat4, Vec3};

pub const RING: usize = 80;
/// GPU words per trail: a 32-word header, then 8 words a sample (a0, birth, b0, 0).
pub const HEAD_WORDS: usize = 32;
pub const WORDS: usize = HEAD_WORDS + RING * 8;

/// Afb chunk (type 0x54): N point pairs, R points per 30 Afb units from `start`, `len` the Afb length (rate numerator).
#[derive(Clone, Debug)]
pub struct Afb {
    pub r: u8,
    pub start: u8,
    pub len: u8,
    pub a: Vec<Vec3>,
    pub b: Vec<Vec3>,
}

impl Afb {
    /// FUN_1001cfa0 / FUN_1001d000: rails at u32 @0x14 / @0x18 of {i16 x, y, z, u8, u8}, scaled by f32 @4..@0xc;
    /// version byte 0x1f must be 0.
    pub fn parse(d: &[u8]) -> Option<Afb> {
        if d.len() < 0x20 || d[0x1f] != 0 || d[1] == 0 {
            return None;
        }
        let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize;
        let s = Vec3::new(f(4), f(8), f(0xc));
        let n = d[0] as usize;
        let rail = |at: usize| -> Option<Vec<Vec3>> {
            (0..n)
                .map(|i| {
                    let o = at + i * 8;
                    let v = |k: usize| d.get(o + k..o + k + 2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32);
                    Some(Vec3::new(v(0)?, v(2)?, v(4)?) * s)
                })
                .collect()
        };
        Some(Afb { r: d[1], start: d[0x10], len: d[0x12], a: rail(u(0x14))?, b: rail(u(0x18))? })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub a0: Vec3,
    pub b0: Vec3,
    /// Agings before it was pushed.
    pub birth: u32,
}

/// One sample after its agings: rails, colours (0..255 floats, RGBA), and whether a rail is NaN in retail.
pub struct Aged {
    pub a: Vec3,
    pub b: Vec3,
    pub ca: [f32; 4],
    pub cb: [f32; 4],
    pub void: [bool; 2],
}

pub struct Trail {
    pub actor: u32,
    afb: Afb,
    t: f32,
    rate: f32,
    next: usize,
    ctrl: Vec<(Vec3, Vec3)>,
    pub ring: [Sample; RING],
    pub head: usize,
    pub count: usize,
    pub aged: u32,
    /// The first aging whose delay had run out (RGB fades from it).
    pub fade_from: Option<u32>,
    delay: f32,
    pub ca: [f32; 4],
    pub cb: [f32; 4],
    /// Op V18: RGB decrement for both colours, alpha decrement of A; V28.w: alpha decrement of B.
    pub dec: [f32; 4],
    pub dec_b: f32,
    pub shrink: (f32, f32),
    pub split: bool,
    /// Samples born before these are NaN on rail A / B (retail's smoothing with |d| = 0).
    pub void_below: [u32; 2],
    carry: f32,
}

impl Trail {
    /// Op 0x2c's record (0x58 bytes): colours @0x10 / @0x14 (R G B A), V18 @0x18, V28 @0x28, shrink @0x38 / @0x3c,
    /// delay @0x40, split @0x44, speed % @0x56; rate = 2 len / max(dur, 1) x (u56 ? u56 x 0.005 : 0.5).
    pub fn new(actor: u32, afb: Afb, raw: &[u8]) -> Trail {
        let f = |o: usize| raw.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
        let c = |o: usize| std::array::from_fn(|i| raw.get(o + i).copied().unwrap_or(0) as f32);
        let dur = raw.get(6..8).map_or(0, |b| i16::from_le_bytes([b[0], b[1]])) as f32;
        let u56 = raw.get(0x56..0x58).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        let factor = if u56 != 0 { u56 as f32 * 0.005 } else { 0.5 };
        let rate = 2.0 * afb.len as f32 / dur.max(1.0) * factor;
        Trail {
            actor,
            afb,
            t: 0.0,
            rate,
            next: 0,
            ctrl: Vec::new(),
            ring: [Sample::default(); RING],
            head: 0,
            count: 0,
            aged: 0,
            fade_from: None,
            delay: f(0x40),
            ca: c(0x10),
            cb: c(0x14),
            dec: [f(0x18), f(0x1c), f(0x20), f(0x24)],
            dec_b: f(0x34),
            shrink: (f(0x38), f(0x3c)),
            split: raw.get(0x44).is_some_and(|b| *b == 1),
            void_below: [0; 2],
            carry: 0.0,
        }
    }

    /// Still taking Afb points (it needs the actor's posed root).
    pub fn placing(&self) -> bool {
        self.next < self.afb.a.len()
    }

    /// Ticks pass; a retail frame (2 ticks) runs each time they add up. `w` places Afb points in the world. False once
    /// the after-image is done.
    pub fn step(&mut self, dt: f32, w: impl Fn() -> Option<Mat4>) -> (bool, bool) {
        self.carry += dt;
        let mut changed = false;
        while self.carry >= 2.0 {
            self.carry -= 2.0;
            changed = true;
            if !self.frame(&w) {
                return (false, true);
            }
        }
        (true, changed)
    }

    /// FUN_1001bc80 with 2 frame ticks.
    fn frame(&mut self, w: &dyn Fn() -> Option<Mat4>) -> bool {
        const FT: f32 = 2.0;
        self.t += FT * self.rate;
        let n = self.afb.a.len();
        let step = 30.0 / self.afb.r as f32;
        let at = |j: usize| j as f32 * step + self.afb.start as f32;
        let mut placed = false;
        if self.next < n && at(self.next) <= self.t {
            let mut due = 0;
            while at(self.next + due) <= self.t {
                due += 1;
            }
            if due > 1 {
                // No skeleton: nothing this frame (retail returns before the trim).
                let Some(m) = w() else { return true };
                for _ in 0..due - 1 {
                    let i = self.next;
                    self.ctrl.push((m.transform_point3(self.afb.a[i]), m.transform_point3(self.afb.b[i])));
                    self.next += 1;
                    if self.ctrl.len() < 3 {
                        if self.count != 0 {
                            self.age(FT);
                        }
                    } else {
                        if self.count == 0 {
                            self.push(self.ctrl[0]);
                        }
                        for u in [0.25, 0.5, 0.75, 1.0] {
                            let s = |k: usize, rail: fn(&(Vec3, Vec3)) -> Vec3| rail(&self.ctrl[k]);
                            let sp = |rail: fn(&(Vec3, Vec3)) -> Vec3| spline(u, s(0, rail), s(1, rail), s(2, rail));
                            self.push((sp(|c| c.0), sp(|c| c.1)));
                            self.age(FT);
                        }
                        self.ctrl.remove(0);
                    }
                    if self.next >= n {
                        break;
                    }
                }
                placed = true;
            }
        }
        if !placed {
            if self.count == 0 {
                return true;
            }
            self.age(FT);
        }
        if self.count != 0 {
            self.count -= self.dead();
            if self.count <= 1 {
                return false;
            }
        }
        true
    }

    /// FUN_1001c260's counters (the per-sample work is the closed form of `aged`).
    fn age(&mut self, ft: f32) {
        self.aged += 1;
        self.delay -= ft;
        if self.delay <= 0.0 && self.fade_from.is_none() {
            self.fade_from = Some(self.aged);
        }
    }

    /// FUN_1001b990; a sample equal to the last on a rail that does not shrink makes the older ones NaN there.
    fn push(&mut self, (a0, b0): (Vec3, Vec3)) {
        if self.count > 0 {
            let prev = self.ring[(self.head + RING - 1) % RING];
            if self.shrink.0 == 0.0 && prev.a0 == a0 {
                self.void_below[0] = prev.birth + 1;
            }
            if self.shrink.1 == 0.0 && prev.b0 == b0 {
                self.void_below[1] = prev.birth + 1;
            }
        }
        self.ring[self.head] = Sample { a0, b0, birth: self.aged };
        self.head = (self.head + 1) % RING;
        self.count = (self.count + 1).min(RING);
    }

    /// Sample `k` back from the newest.
    pub fn sample(&self, k: usize) -> Sample {
        self.ring[(self.head + RING - 1 - k % RING) % RING]
    }

    /// FUN_1001c260 applied (aged - birth) times, in closed form (spec 6.3): both rails close along their line (A by
    /// p, B by q, then swapping once they cross), alpha down per aging, RGB down per aging after the delay.
    pub fn aged_sample(&self, s: &Sample) -> Aged {
        let n = self.aged.saturating_sub(s.birth);
        let (p, q) = self.shrink;
        let d = s.a0 - s.b0;
        let l0 = d.length();
        let (mut sa, mut sb) = (l0, 0.0);
        if p + q > 0.0 && l0 > 0.0 {
            let n1 = (l0 / (p + q)).floor();
            let nf = n as f32;
            if nf <= n1 {
                (sa, sb) = (l0 - nf * p, nf * q);
            } else {
                let (sa1, sb1) = (l0 - n1 * p, n1 * q);
                let l1 = sa1 - sb1;
                let (x, y) = (sa1 - p.min(l1), sb1 + q.min(l1));
                (sa, sb) = if ((nf - n1 - 1.0) as u32).is_multiple_of(2) { (x, y) } else { (y, x) };
            }
        }
        let u0 = if l0 > 0.0 { d / l0 } else { Vec3::ZERO };
        let m = match self.fade_from {
            Some(f) if self.aged >= f => self.aged - s.birth.max(f - 1),
            _ => 0,
        } as f32;
        let n = n as f32;
        let col = |c: [f32; 4], da: f32| -> [f32; 4] { [(c[0] - m * self.dec[0]).clamp(0.0, 255.0), (c[1] - m * self.dec[1]).clamp(0.0, 255.0), (c[2] - m * self.dec[2]).clamp(0.0, 255.0), (c[3] - n * da).clamp(0.0, 255.0)] };
        Aged { a: s.b0 + u0 * sa, b: s.b0 + u0 * sb, ca: col(self.ca, self.dec[3]), cb: col(self.cb, self.dec_b), void: [s.birth < self.void_below[0], s.birth < self.void_below[1]] }
    }

    /// The trim (asm:28827..28976): pairs (newer P, older Q) with both edges under 0.005 (a NaN edge never is), or
    /// no alpha, or no RGB left.
    fn dead(&self) -> usize {
        let aged: Vec<Aged> = (0..self.count).map(|k| self.aged_sample(&self.sample(k))).collect();
        let short = |x: &Aged| !x.void[0] && !x.void[1] && (x.a - x.b).length() < 0.005;
        aged.windows(2)
            .filter(|w| {
                let (p, q) = (&w[0], &w[1]);
                (short(q) && short(p)) || q.ca[3] + q.cb[3] + p.cb[3] + p.ca[3] <= 0.0 || [q.ca, q.cb, p.ca, p.cb].iter().map(|c| c[0] + c[1] + c[2]).sum::<f32>() <= 0.0
            })
            .count()
    }

    /// The depth key (the newest tip's view depth, as an element key).
    pub fn key(&self, view: &Mat4) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let a = self.aged_sample(&self.sample(0)).a;
        (-view.transform_point3(a).z).clamp(0.0, 512.0)
    }

    /// The GPU record: count, head, aged, fade_from (or !0), split, void A / B; CA, CB, V18, (p, q, V28.w); samples.
    pub fn gpu_words(&self) -> Vec<u32> {
        let mut w = vec![0u32; WORDS];
        w[..7].copy_from_slice(&[self.count as u32, self.head as u32, self.aged, self.fade_from.unwrap_or(u32::MAX), self.split as u32, self.void_below[0], self.void_below[1]]);
        let floats = [self.ca, self.cb, self.dec, [self.shrink.0, self.shrink.1, self.dec_b, 0.0]];
        for (i, v) in floats.iter().flatten().enumerate() {
            w[8 + i] = v.to_bits();
        }
        for (k, s) in self.ring.iter().enumerate() {
            let o = HEAD_WORDS + k * 8;
            w[o..o + 8].copy_from_slice(&[s.a0.x.to_bits(), s.a0.y.to_bits(), s.a0.z.to_bits(), s.birth, s.b0.x.to_bits(), s.b0.y.to_bits(), s.b0.z.to_bits(), 0]);
        }
        w
    }
}

/// FUN_1001c8e0: the uniform quadratic B-spline from mid(p0, p1) (u 0) to mid(p1, p2) (u 1).
fn spline(u: f32, p0: Vec3, p1: Vec3, p2: Vec3) -> Vec3 {
    0.5 * u * u * p2 + 0.5 * (1.0 - u) * (1.0 - u) * p0 + ((1.0 - u) * u + 0.5) * p1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn afb(file: u32, name: &[u8; 3]) -> Option<Afb> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let lib = crate::fx::lib::Library::load(file, index.read(file).unwrap()).unwrap();
        let n = lib.tree.nodes.iter().position(|x| x.kind == crate::dat::generator::kind::AFB && x.name[..3] == name[..]).expect("no Afb");
        Some(Afb::parse(lib.data(n)).expect("Afb parse"))
    }

    /// Spec 2.4: 33228 `b0a0` (Hume M Fast Blade).
    #[test]
    fn parse_fast_blade() {
        let Some(a) = afb(33228, b"b0a") else { return };
        assert_eq!((a.a.len(), a.r, a.start, a.len), (5, 30, 14, 45));
        assert!(a.a[0].distance(Vec3::new(-1.0234, -1.1406, 0.0)) < 1e-3, "{:?}", a.a[0]);
        assert!(a.b[4].distance(Vec3::new(1.180, -0.227, 0.094)) < 2e-3, "{:?}", a.b[4]);
    }

    /// Spec 5.1: Fast Blade's op 7 at 30 fps with the actor at the origin: 9 / 13 samples at ticks 28 / 30, done at 92.
    #[test]
    fn fast_blade_timeline() {
        let Some(a) = afb(33228, b"b0a") else { return };
        let mut raw = vec![0u8; 0x58];
        raw[..8].copy_from_slice(&[0x2c, 0x16, 0, 0, 0x14, 0, 0x46, 0]);
        raw[0x10..0x18].copy_from_slice(&[0x8a, 0x8a, 0x8a, 0x40, 0x8a, 0x8a, 0x8a, 0x00]);
        for (o, v) in [(0x24, 2.0f32), (0x34, 2.0), (0x38, 0.001), (0x3c, 0.05), (0x40, 20.0)] {
            raw[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        raw[0x44] = 1;
        raw[0x56] = 0;
        let mut t = Trail::new(1, a, &raw);
        let mut counts = Vec::new();
        let mut done = None;
        for tick in (2..200).step_by(2) {
            let (live, _) = t.step(2.0, || Some(Mat4::IDENTITY));
            counts.push((tick, t.count));
            if !live {
                done = Some(tick);
                break;
            }
        }
        let at = |k: u32| counts.iter().find(|c| c.0 == k).map(|c| c.1);
        assert_eq!((at(26), at(28), at(30)), (Some(0), Some(9), Some(13)), "{counts:?}");
        assert_eq!(done, Some(92), "{counts:?}");
    }
}
