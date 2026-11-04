//! Cloth: the wind gusts (FUN_10188a50) and the cloth meshes' particles (FUN_10023290 / FUN_10023700 /
//! FUN_10023bd0 / FUN_10024900). Retail steps both once a frame at its 30 fps; ours steps them on a 30 Hz clock.

use crate::dat::character::{BoneXf, ClothSpec, Influence, MeshPart, Skeleton};
use glam::{Mat4, Vec3};

/// Retail's frame: every step assumes 1/30 s.
pub const STEP: f32 = 1.0 / 30.0;

/// The environment object's wind (DAT_1065cb04 +0x124..+0x144, FUN_10187050).
#[derive(Clone, Debug)]
pub struct Wind {
    /// +0x124: (-1, 0, 0) for good (the constructor is its only writer).
    pub dir: Vec3,
    /// +0x134.
    pub strength: f32,
    /// +0x138 / +0x13c: event op 0x97 sets them.
    pub base: i32,
    pub spread: i32,
    /// +0x140 / +0x144: gusting, and ticks (1/60 s) left in this state.
    pub on: bool,
    pub timer: f32,
    /// FUN_100ba040: the values before the event's first 0x97 (DAT_104882b0 / DAT_104897ec), for its end.
    pub saved: Option<(i32, i32)>,
    /// Steps taken; actors step when it moves on.
    pub tick: u64,
    acc: f32,
    rng: u32,
}

impl Default for Wind {
    fn default() -> Self {
        Self { dir: Vec3::new(-1.0, 0.0, 0.0), strength: 0.0, base: 4, spread: 10, on: false, timer: 0.0, saved: None, tick: 0, acc: 0.0, rng: 1 }
    }
}

impl Wind {
    /// MSVC rand().
    fn rand(&mut self) -> i32 {
        self.rng = self.rng.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.rng >> 16) & 0x7fff) as i32
    }

    /// Runs the 30 Hz clock: at most one step a frame (retail steps once a frame, slower below 30 fps).
    pub fn advance(&mut self, dt: f32) {
        self.acc = (self.acc + dt).min(STEP * 2.0);
        if self.acc >= STEP {
            self.acc -= STEP;
            self.step(2.0);
            self.tick += 1;
        }
    }

    /// FUN_10188a50 with `ticks` 1/60 s gone (FUN_10014cf0: 2 at 30 fps).
    pub fn step(&mut self, ticks: f32) {
        self.timer -= ticks.max(1.0);
        if self.timer < 0.0 {
            let r = self.rand();
            self.on = !self.on;
            self.timer = (r % 60) as f32;
            if self.on {
                self.timer *= 1.2;
            }
        }
        if self.on {
            // Retail divides by the spread: an event's 0 would crash it; ours treats it as 1.
            let r = self.rand();
            self.strength = ((r % self.spread.max(1)) + self.base) as f32 * 0.0025;
        } else {
            self.strength = (self.strength - 0.01).max(0.0);
        }
    }

    /// Op 0x97 (FUN_100ba0c0): the first in an event saves the values (FUN_100ba040).
    pub fn set(&mut self, base: i32, spread: i32) {
        if self.saved.is_none() {
            self.saved = Some((self.base, self.spread));
        }
        (self.base, self.spread) = (base, spread);
    }

    /// The event's end (FUN_100ae3f0 -> FUN_100ba080).
    pub fn restore(&mut self) {
        if let Some((b, s)) = self.saved.take() {
            (self.base, self.spread) = (b, s);
        }
    }

    /// The event's start (FUN_100aed00): forgets what was saved without putting it back.
    pub fn forget(&mut self) {
        self.saved = None;
    }
}

/// A particle (0x3c bytes in retail): position, last position, normal, in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Particle {
    pub cur: Vec3,
    pub prev: Vec3,
    pub normal: Vec3,
}

/// One cloth mesh of a model: its particles and its spheres' last centres (instance +0x48).
#[derive(Clone, Debug)]
pub struct Sheet {
    pub part: usize,
    pub p: Vec<Particle>,
    sphere_prev: Vec<Option<Vec3>>,
}

/// A model's cloth, built when the model is (FUN_10025010 sets the reset flag only there).
#[derive(Clone, Debug, Default)]
pub struct Cloth {
    pub sheets: Vec<Sheet>,
    reset: bool,
    /// The wind step it last took, and where the model stood then.
    pub tick: u64,
    pub at: Vec3,
}

/// What a step reads from the actor.
pub struct Frame<'a> {
    pub skeleton: &'a Skeleton,
    /// Model-space bones (character::pose) and the model matrix; together the retail world bone matrices.
    pub bones: &'a [BoneXf],
    pub model: Mat4,
    /// The actor's move since the last step (model +0x34, FUN_100c63d0).
    pub moved: Vec3,
    /// Wind direction and strength, or None: actor +0x17c, set while it stands on a covered floor (FUN_100a4ed0:
    /// the ground hit's MZB object flags bit 2).
    pub wind: Option<(Vec3, f32)>,
}

impl Cloth {
    pub fn new(parts: &[MeshPart]) -> Option<Self> {
        let sheets: Vec<Sheet> = parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                p.sim.as_ref().map(|s| Sheet { part: i, p: vec![Particle::default(); s.particles()], sphere_prev: vec![None; s.spheres.len()] })
            })
            .collect();
        (!sheets.is_empty()).then_some(Self { sheets, reset: true, tick: u64::MAX, at: Vec3::ZERO })
    }

    /// FUN_10023290 for each cloth mesh: the first step snaps every particle to the skin; later ones skin the pinned
    /// particles, carry the free ones by the actor's move, integrate them, then the normals.
    pub fn step(&mut self, parts: &[MeshPart], f: &Frame) {
        for sh in &mut self.sheets {
            let (Some(part), Some(spec)) = (parts.get(sh.part), parts.get(sh.part).and_then(|p| p.sim.as_deref())) else { continue };
            if self.reset {
                for (i, q) in sh.p.iter_mut().enumerate() {
                    q.cur = skin(f, &part.vertices[i]);
                    q.prev = q.cur;
                }
            } else {
                let (d_cur, d_prev) = ((0.75 - spec.inertia) * f.moved, 0.75 * f.moved);
                for (i, q) in sh.p.iter_mut().enumerate() {
                    if spec.pinned(i) {
                        q.cur = skin(f, &part.vertices[i]);
                    } else {
                        q.cur += d_cur;
                        q.prev += d_prev;
                    }
                }
                integrate(spec, f, &mut sh.p);
                collide(spec, f, &mut sh.p, &mut sh.sphere_prev);
            }
            normals(spec, &mut sh.p);
        }
        self.reset = false;
    }

    /// The particles hold positions: the first step (the snap to the skin) has run.
    pub fn placed(&self) -> bool {
        !self.reset
    }

    /// FUN_100231f0 (the actor off screen): every particle moves with the actor, nothing simulated.
    pub fn carry(&mut self, moved: Vec3) {
        if self.reset {
            return;
        }
        for q in self.sheets.iter_mut().flat_map(|s| s.p.iter_mut()) {
            q.cur += moved;
            q.prev += moved;
        }
    }
}

/// The world position the skin gives a vertex (FUN_100235c0 / FUN_10023620): the file's weights are in the positions,
/// so each bone adds its rotation of the position and its translation x weight.
fn skin(f: &Frame, inf: &[Influence]) -> Vec3 {
    let p = inf
        .iter()
        .map(|i| {
            let b = f.bones.get(i.bone).copied().unwrap_or(BoneXf::IDENTITY);
            b.q * (b.s * i.pos) + b.t * i.weight
        })
        .sum::<Vec3>();
    f.model.transform_point3(p)
}

/// FUN_10023700's first half (Verlet, gravity, wind) and the four constraint passes.
fn integrate(spec: &ClothSpec, f: &Frame, p: &mut [Particle]) {
    for (i, q) in p.iter_mut().enumerate() {
        if spec.pinned(i) {
            continue;
        }
        let vel = (q.cur - q.prev) * spec.damping;
        q.prev = q.cur;
        q.cur += vel;
        q.cur.y += spec.gravity * 0.1;
        if let Some((dir, strength)) = f.wind {
            // Last step's normal plus the wind, not normalised; a normal against the wind takes the difference.
            let mut w = q.normal + dir;
            if w.dot(w) < 0.01 {
                w = dir - q.normal;
            }
            q.cur += w * (strength * 0.1);
        }
    }
    for _ in 0..4 {
        for (n, e) in spec.edges.iter().enumerate() {
            let k = spec.k[usize::from(n >= spec.group1)];
            let (a, b) = ((e.a & 0x7fff) as usize, (e.b & 0x7fff) as usize);
            let mut d = p[b].cur - p[a].cur;
            let len = d.length();
            if len > 1e-4 {
                d *= (e.rest - len) * k / len;
                if e.a & 0x8000 == 0 {
                    p[a].cur -= d;
                }
                if e.b & 0x8000 == 0 {
                    p[b].cur += d;
                }
            }
        }
    }
}

/// FUN_100274b0: a zero vector stays zero.
fn norm(v: Vec3) -> Vec3 {
    let l = v.length();
    if l > 0.0 { v / l } else { Vec3::ZERO }
}

/// FUN_10023bd0: each edge takes the last of the mesh's spheres its first particle is near (|a - c|^2 <= 1.5 r^2);
/// then each sphere sweeps from last step's centre to this one against its edges, both ways. The sweep's start moves
/// along as it goes, so a sphere's later edges start where the earlier ones stopped.
fn collide(spec: &ClothSpec, f: &Frame, p: &mut [Particle], prev: &mut [Option<Vec3>]) {
    // An unused table slot (0xCD fill: bone -12851, radius -4.3e8) reads a bone matrix from before the table in
    // retail, so what it does is down to that memory; ours skips it (1367's cape then drapes over the body instead
    // of losing all its edges to slot 1). An index past the 64 slots is skipped too.
    let spheres: Vec<Option<(i32, Option<(Vec3, f32)>)>> = spec
        .spheres
        .iter()
        .map(|&s| {
            let r = f.skeleton.spheres.get(s as usize)?;
            let used = r.bone >= 0 && (r.bone as usize) < f.skeleton.bones.len() && r.radius > 0.0 && r.radius.is_finite();
            let at = used.then(|| {
                let b = f.bones.get(r.bone as usize).copied().unwrap_or(BoneXf::IDENTITY);
                (f.model.transform_point3(b.at(r.offset)), r.radius)
            });
            Some((s as i32, at))
        })
        .collect();
    // DAT_10456e38: a byte an edge, 0xff none; compared as a signed char with the sphere index.
    let owner: Vec<i32> = spec
        .edges
        .iter()
        .map(|e| {
            let a = p[(e.a & 0x7fff) as usize].cur;
            spheres
                .iter()
                .flatten()
                .rev()
                .find(|(_, at)| at.is_some_and(|(c, r)| (a - c).length_squared() <= r * r * 1.5))
                .map_or(-1, |(s, _)| *s as u8 as i8 as i32)
        })
        .collect();
    for (i, sph) in spheres.iter().enumerate() {
        let Some((s, Some((cur, r)))) = *sph else { continue };
        // Retail's last centres (instance +0x48) start as whatever the pool allocator left there (FUN_10028930 does
        // not clear); ours start at the current centre.
        let mut c = prev[i].unwrap_or(cur);
        for (n, e) in spec.edges.iter().enumerate() {
            if owner[n] != s || e.a & 0x8000 != 0 {
                continue;
            }
            sweep(p, e.a, e.b, &mut c, cur, r);
            if e.b & 0x8000 == 0 {
                sweep(p, e.b, e.a, &mut c, cur, r);
            }
        }
        prev[i] = Some(cur);
    }
}

/// FUN_10024020: the edge a-b against a sphere of radius r swept from `c` to `to`, in steps of r.
fn sweep(p: &mut [Particle], a: u16, b: u16, c: &mut Vec3, to: Vec3, r: f32) {
    let (ai, bi) = ((a & 0x7fff) as usize, (b & 0x7fff) as usize);
    let (a_free, b_free) = (a & 0x8000 == 0, b & 0x8000 == 0);
    let (a_prev, a_cur) = (p[ai].prev, p[ai].cur);
    let b_cur = p[bi].cur;
    let b_prev = if b_free { p[bi].prev } else { b_cur };
    // FUN_10024220 / FUN_10024330 / FUN_100243c0: the two bounding spheres must meet.
    let (l1, l2) = (a_cur - b_prev, b_cur - a_prev);
    let (f1, f2) = (l1.length_squared(), l2.length_squared());
    let (mid_e, r_e) = if f2 < f1 { (b_prev + 0.5 * l1, f1 * 0.5) } else { (a_prev + 0.5 * l2, f2 * 0.5) };
    let d = to - *c;
    let fd = d.length_squared();
    let mid_s = *c + 0.5 * d;
    let r_s = if fd < 1e-6 { r * r } else { r * r + fd };
    if (mid_e - mid_s).length_squared() > r_s + r_e {
        return;
    }
    let mut dist = d.length();
    let dir = norm(d);
    loop {
        let (k, push) = segment_sphere(r, *c, p[ai].cur, p[bi].cur);
        match k {
            1 if b_free => p[bi].cur += push,
            2 if a_free => p[ai].cur += push,
            3 => {
                if b_free {
                    p[bi].cur += push;
                }
                if a_free {
                    p[ai].cur += push;
                }
            }
            1..=3 => {}
            _ => return,
        }
        dist -= r;
        if dist >= 1e-6 {
            *c += dir * r;
        } else {
            *c = to;
        }
        if dist <= 0.0 {
            return;
        }
    }
}

/// FUN_10024410: the segment A-B against the sphere (c, r). 0 both ends inside (no push), 4 apart; 1 / 2 / 3 push B /
/// A / both by the returned vector.
fn segment_sphere(r: f32, c: Vec3, a: Vec3, b: Vec3) -> (u8, Vec3) {
    let r2 = r * r;
    let (da, db) = (a - c, b - c);
    let u = norm(b - a);
    let bits = u8::from(r2 < da.length_squared()) | u8::from(r2 < db.length_squared()) << 1;
    let out = |v: Vec3| norm(v) * (r - v.length());
    match bits {
        1 => {
            let nu = -u;
            let q = nu * (c - b).dot(nu) + b;
            if 1e-4 <= norm(q - b).dot(nu) {
                let l = q - c;
                if r2 < l.length_squared() {
                    return (4, Vec3::ZERO);
                }
                (1, out(l))
            } else {
                (1, out(db))
            }
        }
        2 => {
            let q = u * (c - a).dot(u) + a;
            if norm(q - a).dot(u) < 1e-4 {
                return (2, out(da));
            }
            let l = q - c;
            if r2 < l.length_squared() {
                return (4, Vec3::ZERO);
            }
            (2, out(l))
        }
        3 => {
            let q = u * (c - a).dot(u) + a;
            if norm(q - a).dot(u) < 1e-4 || 0.0 < norm(q - b).dot(u) {
                return (4, Vec3::ZERO);
            }
            let l = q - c;
            if r2 < l.length_squared() {
                return (4, Vec3::ZERO);
            }
            (3, out(l))
        }
        _ => (0, Vec3::ZERO),
    }
}

/// FUN_10024900: (0, 1, 0) plus every triangle's cross(v2 - v0, v1 - v0), normalised.
fn normals(spec: &ClothSpec, p: &mut [Particle]) {
    for q in p.iter_mut() {
        q.normal = Vec3::Y;
    }
    for t in &spec.tris {
        let [i0, i1, i2] = t.map(usize::from);
        let n = (p[i2].cur - p[i0].cur).cross(p[i1].cur - p[i0].cur);
        for i in [i0, i1, i2] {
            p[i].normal += n;
        }
    }
    for q in p.iter_mut() {
        q.normal = norm(q.normal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::character::ClothEdge;

    #[test]
    fn gust_strength_follows_base_and_spread() {
        let mut w = Wind::default();
        let mut seen_on = false;
        for _ in 0..2000 {
            w.step(2.0);
            if w.on {
                seen_on = true;
                assert!((0.01..=0.0325 + 1e-6).contains(&w.strength), "{}", w.strength);
            }
            assert!(w.strength >= 0.0);
        }
        assert!(seen_on);
        w.set(100, 1);
        loop {
            w.step(2.0);
            if w.on {
                assert!((w.strength - 0.25).abs() < 1e-6, "{}", w.strength);
                break;
            }
        }
    }

    #[test]
    fn calm_decays_a_hundredth_a_step() {
        let mut w = Wind { strength: 0.025, timer: 100.0, ..Wind::default() };
        w.step(2.0);
        assert!((w.strength - 0.015).abs() < 1e-6);
        w.step(2.0);
        w.step(2.0);
        assert_eq!(w.strength, 0.0);
    }

    #[test]
    fn op_97_saves_once_and_the_end_restores() {
        let mut w = Wind::default();
        w.set(1, 2);
        w.set(3, 4);
        assert_eq!((w.base, w.spread), (3, 4));
        w.restore();
        assert_eq!((w.base, w.spread, w.saved), (4, 10, None));
        // A new event forgets an override left set: it becomes the original.
        w.set(7, 8);
        w.forget();
        w.set(9, 9);
        w.restore();
        assert_eq!((w.base, w.spread), (7, 8));
    }

    fn spec(edges: Vec<ClothEdge>) -> ClothSpec {
        ClothSpec {
            counts: [2, 0],
            pins: [1, 0],
            group1: edges.len(),
            edges,
            k: [0.5, 0.5],
            damping: 0.9,
            gravity: 0.1,
            inertia: 0.0,
            spheres: Vec::new(),
            tris: Vec::new(),
        }
    }

    #[test]
    fn constraint_pulls_the_free_end_back() {
        let s = spec(vec![ClothEdge { a: 0x8000, b: 1, rest: 1.0 }]);
        let mut p = vec![Particle::default(); 2];
        p[1].cur = Vec3::new(2.0, 0.0, 0.0);
        p[1].prev = p[1].cur;
        let skel = Skeleton {
            name: String::new(),
            bones: Vec::new(),
            points: Vec::new(),
            point_rots: Vec::new(),
            look_limits: [[0.0; 3]; 2],
            boxes: Vec::new(),
            spheres: Vec::new(),
        };
        let f = Frame { skeleton: &skel, bones: &[], model: Mat4::IDENTITY, moved: Vec3::ZERO, wind: None };
        integrate(&s, &f, &mut p);
        // Gravity 0.01 down, then four passes at k 0.5 halve the stretch each time: 1 + 1/16 along x.
        assert_eq!(p[0].cur, Vec3::ZERO);
        assert!((p[1].cur.length() - 1.0625).abs() < 1e-3, "{:?}", p[1].cur);
    }

    #[test]
    fn wind_pushes_along_normal_plus_direction() {
        let s = spec(Vec::new());
        let mut p = vec![Particle::default(); 2];
        p[1].normal = Vec3::Y;
        let skel = Skeleton {
            name: String::new(),
            bones: Vec::new(),
            points: Vec::new(),
            point_rots: Vec::new(),
            look_limits: [[0.0; 3]; 2],
            boxes: Vec::new(),
            spheres: Vec::new(),
        };
        let f = Frame { skeleton: &skel, bones: &[], model: Mat4::IDENTITY, moved: Vec3::ZERO, wind: Some((Vec3::NEG_X, 0.02)) };
        integrate(&s, &f, &mut p);
        assert!((p[1].cur - Vec3::new(-0.002, 0.01 + 0.002, 0.0)).length() < 1e-6, "{:?}", p[1].cur);
        // Against the wind: the difference instead.
        let mut p = vec![Particle::default(); 2];
        p[1].normal = Vec3::X;
        integrate(&s, &f, &mut p);
        assert!((p[1].cur - Vec3::new(-0.004, 0.01, 0.0)).length() < 1e-6, "{:?}", p[1].cur);
    }

    #[test]
    fn segment_sphere_cases() {
        // Both ends outside, the middle inside: both pushed out along the closest point's direction.
        let (k, push) = segment_sphere(1.0, Vec3::ZERO, Vec3::new(-2.0, 0.5, 0.0), Vec3::new(2.0, 0.5, 0.0));
        assert_eq!(k, 3);
        assert!((push - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-5, "{push:?}");
        // Both inside: nothing.
        assert_eq!(segment_sphere(1.0, Vec3::ZERO, Vec3::splat(0.1), Vec3::splat(0.2)).0, 0);
        // Far apart.
        assert_eq!(segment_sphere(1.0, Vec3::ZERO, Vec3::new(-2.0, 3.0, 0.0), Vec3::new(2.0, 3.0, 0.0)).0, 4);
        // B inside, A outside: B goes out.
        let (k, push) = segment_sphere(1.0, Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 0.5, 0.0));
        assert_eq!(k, 1);
        assert!((push - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-5, "{push:?}");
    }

    #[test]
    fn normals_face_up_for_a_flat_sheet() {
        let mut s = spec(Vec::new());
        s.counts = [3, 0];
        s.tris = vec![[0, 1, 2]];
        let mut p = vec![Particle::default(); 3];
        p[1].cur = Vec3::X;
        p[2].cur = Vec3::Z;
        normals(&s, &mut p);
        // cross(Z, X) = +Y.
        assert!(p.iter().all(|q| (q.normal - Vec3::Y).length() < 1e-6));
    }
}
