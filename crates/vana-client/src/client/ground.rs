//! Movement against the zone collision mesh, the way retail moves the player (CXiCollisionActor,
//! FUN_100a3560): the step is swept as a sphere against the faces (FUN_101679c0), then gravity, then a
//! floor ray. World Y points down.

use crate::dat::zone::ZoneScene;
use glam::Vec3;
use std::cell::Cell;
use std::collections::HashMap;

const CELL: f32 = 4.0;
/// Margin a triangle is registered with around its bounds (camera ray sampling).
const MARGIN: f32 = 0.4;
/// The swept sphere: radius, centre this far above the feet (FUN_10167b10).
const R: f32 = 0.5;
/// |n.y| above this is a floor or ceiling to the sweep, else a wall (_DAT_1032af84).
const FLOOR_NY: f32 = 0.708;
/// Fall speed gained per tick (_DAT_10322d74) and its cap.
const GRAVITY: f32 = 0.040833335;
const FALL_MAX: f32 = 1.0;
/// The player's physics step: one retail frame (2 ticks at its 30 fps, FUN_100889d0). The drawn position is
/// interpolated between steps.
pub const STEP_TICKS: f32 = 2.0;
/// walk() simulates steps at run speed 5.0.
const RUN_TICK: f32 = 5.0 / 60.0;
const RUN_STEP: f32 = RUN_TICK * STEP_TICKS;

/// Per-actor movement state of FUN_100a3560.
#[derive(Clone, Copy)]
struct Body {
    /// Where the last step left it; a body found elsewhere starts over.
    at: Option<Vec3>,
    /// Fall speed per frame (+0xf4).
    vel: f32,
    /// The last gravity sweep touched a floor (+0x5e0).
    grounded: bool,
    /// Normal of the floor under the feet (+0x160); only faces with |n.y| > 0.7 replace it.
    normal: Vec3,
    /// +0x5f4: the first fall is clamped to 0.4.
    fell: bool,
}

impl Body {
    const NEW: Body = Body { at: None, vel: 0.0, grounded: false, normal: Vec3::NEG_Y, fell: false };
}

pub struct Ground {
    tris: Vec<[Vec3; 3]>,
    normals: Vec<Vec3>,
    /// The object's y range (+0xb4 / +0xb8), tested before its triangles in the sweep.
    y_range: Vec<(f32, f32)>,
    /// Zero-length edge: FUN_10161130 marks these and the sweep skips them.
    degenerate: Vec<bool>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// The MZB's own grid, which sets the sweep's face order.
    grid: Option<Grid>,
    /// The player's body for walk_player().
    body: Cell<Body>,
}

/// 4-unit cells centred on the origin, each listing (first triangle, count, resolved late) runs.
struct Grid {
    w: usize,
    h: usize,
    cells: Vec<Vec<u32>>,
    entries: Vec<(u32, u32, bool)>,
}

impl Grid {
    /// FUN_10167b10's face order: the cells around the start's (row by row, ±1), each list's entries without
    /// object flag 2 and then those with it. A face listed in several cells comes up again each time.
    fn faces(&self, s: Vec3) -> Vec<u32> {
        let (cx, cz) = (((s.x + self.w as f32 * 2.0) * 0.25) as i32, ((s.z + self.h as f32 * 2.0) * 0.25) as i32);
        let mut out = Vec::new();
        if cx < 0 || cz < 0 || cx >= self.w as i32 || cz >= self.h as i32 {
            return out;
        }
        let rows = (cz - 1).max(0)..=(cz + 1).min(self.h as i32 - 1);
        for z in rows {
            for x in (cx - 1).max(0)..=(cx + 1).min(self.w as i32 - 1) {
                let list = &self.cells[z as usize * self.w + x as usize];
                for late in [false, true] {
                    for &e in list {
                        let (first, count, l) = self.entries[e as usize];
                        if l == late {
                            out.extend(first..first + count);
                        }
                    }
                }
            }
        }
        out
    }
}

fn xz(v: Vec3) -> glam::Vec2 {
    glam::Vec2::new(v.x, v.z)
}

fn cell(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

/// FUN_101688a0: `p` (on the plane) inside the triangle, per axis against the face normal.
fn inside(a: Vec3, b: Vec3, c: Vec3, p: Vec3, n: Vec3) -> bool {
    [(a, b), (b, c), (c, a)].iter().all(|&(s, e)| {
        let x = (e - s).cross(p - s);
        x.x * n.x <= 1e-4 && x.y * n.y <= 1e-4 && x.z * n.z <= 1e-4
    })
}

/// FUN_10168af0: the sphere at `p` (radius² `r2`) reaches the edge a-b.
fn touches_edge(a: Vec3, b: Vec3, p: Vec3, r2: f32) -> bool {
    let (d, e) = (p - a, b - a);
    let len = e.length();
    let t = d.dot(e) / len;
    t >= 0.0 && t <= len && r2 - (d.length_squared() - t * t) >= 0.0
}

/// A sweep candidate (FUN_101671b0's 0x68-byte record).
struct Cand {
    v: [Vec3; 3],
    n: Vec3,
    /// n along the move.
    nz: f32,
}

/// FUN_101682c0's working state.
struct Sweep {
    out: Vec3,
    first: bool,
    wall: bool,
    floor_fixed: bool,
    floor_touched: bool,
    best_floor: f32,
    best_wall: f32,
}

impl Sweep {
    /// FUN_10168500: one face against the sphere centred at `p`.
    fn face(&mut self, t: &Cand, p: Vec3) {
        const R2: f32 = R * R;
        let [a, b, c] = t.v;
        let n = t.n;
        if self.first && t.nz > 0.0 {
            return;
        }
        let nn = n.length_squared();
        if nn == 0.0 {
            return;
        }
        let foot = p + n * ((a - p).dot(n) / nn);
        if !(inside(a, b, c, foot, n) || touches_edge(a, b, p, R2) || touches_edge(b, c, p, R2) || touches_edge(c, a, p, R2)) {
            return;
        }
        let d2 = (foot - p).length_squared();
        if d2 > R2 - 0.001 {
            return;
        }
        if n.y > FLOOR_NY || n.y < -FLOOR_NY {
            // Floor or ceiling, unless the face lies wholly above head height (0.3 over the centre).
            let top = p.y - 0.3;
            if d2 < self.best_floor && (top < a.y || top < b.y || top < c.y) {
                if n.y < 0.0 {
                    self.out.y = foot.y - R;
                    self.floor_touched = true;
                    self.floor_fixed = true;
                }
                self.best_floor = d2;
            }
        } else {
            // Wall, if it reaches above the centre: lower faces are stepped over.
            let top = p.y - 0.002;
            if d2 < self.best_wall && (a.y < top || b.y < top || c.y < top) {
                let away = p - foot;
                let len = away.length();
                if len != 0.0 && (away / len).dot(n) >= 0.0 {
                    let push = (R2 - d2).sqrt();
                    let d = away / len * push;
                    self.out = Vec3::new(p.x + d.x, p.y + d.y * 0.2, p.z + d.z);
                    self.wall = true;
                    self.best_wall = d2;
                }
            }
        }
    }
}

impl Ground {
    pub fn build(scene: &ZoneScene) -> Self {
        let Some(c) = scene.collision.as_ref() else {
            return Self { tris: Vec::new(), normals: Vec::new(), y_range: Vec::new(), degenerate: Vec::new(), cells: HashMap::new(), grid: None, body: Cell::new(Body::NEW) };
        };
        let tris = c.tris.clone();
        let y_range = c.object.iter().map(|&o| c.objects[o as usize].y_range).collect();
        let degenerate = tris.iter().map(|[a, b, c]| *a == *b || *b == *c || *c == *a).collect();
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            let (a, b) = (cell(lo.x - MARGIN, lo.z - MARGIN), cell(hi.x + MARGIN, hi.z + MARGIN));
            for cx in a.0..=b.0 {
                for cz in a.1..=b.1 {
                    cells.entry((cx, cz)).or_default().push(i as u32);
                }
            }
        }
        let entries = c.entries.iter().map(|e| (e.first, e.count, c.objects[e.object as usize].flags & 2 != 0)).collect();
        let grid = Some(Grid { w: c.grid.0, h: c.grid.1, cells: c.cells.clone(), entries });
        Self { tris, normals: c.normals.clone(), y_range, degenerate, cells, grid, body: Cell::new(Body::NEW) }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    fn near(&self, x: f32, z: f32) -> impl Iterator<Item = usize> + '_ {
        self.cells.get(&cell(x, z)).into_iter().flatten().map(|&i| i as usize)
    }

    /// Highest face at (x, z) no more than `step` above `feet_y`, i.e. y >= feet_y - step.
    pub fn floor(&self, x: f32, z: f32, feet_y: f32, step: f32) -> Option<f32> {
        self.ray_down(x, z, feet_y - step, f32::INFINITY).map(|(y, _)| y)
    }

    /// Where a character placed at height `y` lands: the first floor below (zone-in points sit mid-air at the
    /// zone line's centre), allowing a small sink; failing that the nearest surface.
    pub fn settle(&self, x: f32, z: f32, y: f32) -> Option<f32> {
        self.floor(x, z, y, 0.5).or_else(|| self.heights(x, z).into_iter().map(|(h, _)| h).min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs())))
    }

    /// FUN_1017a400: a vertical segment from y - 2 down to y + 50, else from y + 2 up to y - 13, against every
    /// collision face near (x, z); the hit nearest the start wins. None when both miss (y stays).
    pub fn snap(&self, x: f32, z: f32, y: f32) -> Option<f32> {
        let hits = self.heights(x, z);
        let down = hits.iter().map(|h| h.0).filter(|&h| h >= y - 2.0 && h < y + 50.0).min_by(|a, b| a.total_cmp(b));
        down.or_else(|| hits.iter().map(|h| h.0).filter(|&h| h <= y + 2.0 && h > y - 13.0).max_by(|a, b| a.total_cmp(b)))
    }

    /// FUN_10162d20 as a vertical ray: the first face below `from_y` down to `to_y` (all faces, both sides).
    fn ray_down(&self, x: f32, z: f32, from_y: f32, to_y: f32) -> Option<(f32, usize)> {
        self.heights(x, z).into_iter().filter(|&(h, _)| h >= from_y && h <= to_y).min_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Heights of the faces over (x, z), with their triangle.
    fn heights(&self, x: f32, z: f32) -> Vec<(f32, usize)> {
        self.near(x, z)
            .filter_map(|i| {
                let [a, b, c] = self.tris[i];
                let d = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
                if d.abs() < 1e-8 {
                    return None;
                }
                let u = ((x - a.x) * (c.z - a.z) - (c.x - a.x) * (z - a.z)) / d;
                let v = ((b.x - a.x) * (z - a.z) - (x - a.x) * (b.z - a.z)) / d;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| (a.y + u * (b.y - a.y) + v * (c.y - a.y), i))
            })
            .collect()
    }

    /// Distance along the segment `from -> to` to the first collision triangle, if any.
    pub fn raycast(&self, from: Vec3, to: Vec3) -> Option<f32> {
        let dir = to - from;
        let len = dir.length();
        if len < 1e-4 {
            return None;
        }
        let d = dir / len;
        let mut checked = std::collections::HashSet::new();
        let mut best: Option<f32> = None;
        let steps = (len / (CELL * 0.5)).ceil() as i32;
        for s in 0..=steps {
            let p = from + d * (len * s as f32 / steps.max(1) as f32);
            for i in self.near(p.x, p.z) {
                if !checked.insert(i) {
                    continue;
                }
                let [a, b, c] = self.tris[i];
                // Moller-Trumbore
                let (e1, e2) = (b - a, c - a);
                let h = d.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-7 {
                    continue;
                }
                let inv = 1.0 / det;
                let sv = from - a;
                let u = inv * sv.dot(h);
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = sv.cross(e1);
                let v = inv * d.dot(q);
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let t = inv * e2.dot(q);
                if t > 0.0 && t < len && best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
        best
    }

    /// FUN_101679c0: sweeps the feet from `from` to `to`; true when a floor face was touched. Moves of 4 or more
    /// units go in parts, and the first part that touches a floor ends it there.
    fn sweep(&self, from: Vec3, to: Vec3) -> (Vec3, bool) {
        let n = 1 + (xz(to - from).length() / 4.0) as i32;
        if n < 2 {
            return self.sweep_once(from, to);
        }
        let step = (to - from) / n as f32;
        let mut start = from;
        for _ in 1..n {
            let (out, floor) = self.sweep_once(start, start + step);
            if floor {
                return (out, true);
            }
            start += step;
        }
        self.sweep_once(start, to)
    }

    /// FUN_10167b10 / FUN_101682c0: a radius-0.5 sphere riding 0.5 above the feet, marched along the move in
    /// 0.15 steps until a wall stops it, then resolved in place (up to four rounds, else it stays at `from`).
    fn sweep_once(&self, from: Vec3, to: Vec3) -> (Vec3, bool) {
        let s = from - Vec3::Y * R;
        let mut e = to - Vec3::Y * R;
        if s.y == e.y {
            e.y += 0.001;
        }
        let up = if s.x == e.x && s.z == e.z { Vec3::Z } else { Vec3::NEG_Y };
        let len = (e - s).length();
        let fz = (e - s) / len;
        let fx = up.cross(e - s).normalize();
        let fy = fz.cross(fx);
        let local = |p: Vec3| {
            let d = p - s;
            Vec3::new(d.dot(fx), d.dot(fy), d.dot(fz))
        };
        let (y_lo, y_hi) = (s.y.min(e.y) - R, s.y.max(e.y) + R);
        // FUN_101671b0: faces whose bounds in the move's frame meet the swept box, at most 0x3fc.
        let ids = match &self.grid {
            Some(g) => g.faces(s),
            None => {
                let (lo, hi) = (s.min(e) - Vec3::splat(R * 1.75), s.max(e) + Vec3::splat(R * 1.75));
                let (c0, c1) = (cell(lo.x, lo.z), cell(hi.x, hi.z));
                let mut ids: Vec<u32> = (c0.0..=c1.0).flat_map(|cx| (c0.1..=c1.1).map(move |cz| (cx, cz))).flat_map(|k| self.cells.get(&k).into_iter().flatten().copied()).collect();
                ids.sort_unstable();
                ids.dedup();
                ids
            }
        };
        let mut cands = Vec::new();
        for i in ids {
            if cands.len() >= 0x3fc {
                break;
            }
            let i = i as usize;
            let (ymin, ymax) = self.y_range[i];
            if self.degenerate[i] || ymin > y_hi || ymax < y_lo {
                continue;
            }
            let v = self.tris[i];
            let l = v.map(local);
            let (bmin, bmax) = (l[0].min(l[1]).min(l[2]), l[0].max(l[1]).max(l[2]));
            let (mid, half) = ((bmin + bmax) * 0.5, (bmax - bmin) * 0.5);
            let (hx, hy) = (half.x + R, half.y + R);
            if mid.x + hx > 0.0 && mid.y + hy > 0.0 && mid.z + half.z + R > 0.0 && hx - mid.x > 0.0 && hy - mid.y > 0.0 && half.z + len + R - mid.z > 0.0 {
                let n = self.normals[i];
                cands.push(Cand { v, n, nz: n.dot(fz) });
            }
        }
        let mut sw = Sweep { out: s, first: true, wall: false, floor_fixed: false, floor_touched: false, best_floor: 0.0, best_wall: 0.0 };
        let mut t = 0.0f32;
        loop {
            let p = s + fz * t;
            sw.best_floor = 9999999.0;
            sw.best_wall = 9999999.0;
            sw.out.x = p.x;
            if !sw.floor_fixed {
                sw.out.y = p.y;
            }
            sw.out.z = p.z;
            for c in &cands {
                sw.face(c, p);
            }
            if t >= len || sw.wall {
                break;
            }
            t = (t + R * 0.3).min(len);
        }
        sw.first = false;
        for round in 0.. {
            sw.wall = false;
            sw.floor_fixed = false;
            sw.best_floor = 9999999.0;
            sw.best_wall = 9999999.0;
            let p = sw.out;
            for c in &cands {
                sw.face(c, p);
            }
            if !sw.wall && !sw.floor_fixed {
                break;
            }
            if round > 2 {
                return (from, sw.floor_touched);
            }
        }
        (sw.out + Vec3::Y * R, sw.floor_touched)
    }

    /// One frame of FUN_100a3560 for the player: `to - from` is the frame's horizontal move (FUN_100a48d0),
    /// `ticks` the frame length in 1/60 s.
    fn step(&self, body: &mut Body, from: Vec3, to: Vec3, ticks: f32) -> Vec3 {
        if body.at != Some(from) {
            *body = Body::NEW;
        }
        if body.grounded {
            body.vel = 0.0;
        }
        let mut v = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
        let moving = v.length_squared() > 1e-14;
        if !moving && body.grounded {
            body.at = Some(from);
            return from;
        }
        // On the ground the move runs along the ground plane at the same length, never downwards.
        if moving && body.grounded {
            let len = v.length();
            let side = v.cross(body.normal).normalize_or_zero();
            let mut t = body.normal.cross(side).normalize_or_zero();
            if t.y > 0.0 {
                t.y = 0.0;
            }
            v = t * len;
        }
        let (mut pos, _) = self.sweep(from, from + v);
        if !pos.is_finite() {
            pos = from + v;
        }
        // Gravity (asm 0x100a3803): the speed grows by GRAVITY x ticks (at least 1, FUN_10014000), lowered to the
        // frame's travel when that is between 0.01 and it, and the fall moves the whole speed.
        let mut g = GRAVITY * ticks.max(1.0);
        let hd = xz(pos - from).length();
        if hd > 0.01 && hd < g {
            g = hd;
        }
        body.vel = (body.vel + g).min(FALL_MAX);
        if !body.fell {
            body.fell = true;
            body.vel = body.vel.min(0.4);
        }
        let (fallen, grounded) = self.sweep(pos, pos + Vec3::Y * body.vel);
        body.grounded = grounded;
        pos = fallen;
        // FUN_1017a5d0 mode 0: no face from 0.5 above the feet to 40 below, and the frame is undone.
        match self.ray_down(pos.x, pos.z, pos.y - 0.5, pos.y + 40.0) {
            Some((_, i)) => {
                let n = self.normals[i];
                if n.y > 0.7 || n.y < -0.7 {
                    body.normal = n;
                }
            }
            None => {
                pos = from;
                body.vel = 0.0;
            }
        }
        body.at = Some(pos);
        pos
    }

    /// The player's frame: walks from `from` towards `to` (horizontal), falling while in the air. Call it every
    /// frame the player could fall, also without input (`to == from`).
    pub fn walk_player(&self, from: Vec3, to: Vec3, ticks: f32) -> Vec3 {
        let mut body = self.body.get();
        let pos = self.step(&mut body, from, to, ticks);
        self.body.set(body);
        pos
    }

    /// Walks from `from` towards `to` as the player would at run speed, a frame at a time, until it gets
    /// there or stops getting closer. `_climb` is unused: retail has no climb limit, only the sweep's.
    pub fn walk(&self, from: Vec3, to: Vec3, _climb: f32) -> Vec3 {
        let mut body = Body::NEW;
        let mut pos = from;
        let mut stuck = 0;
        let frames = (xz(to - from).length() / RUN_STEP) as usize * 3 + 8;
        for _ in 0..frames {
            let left = Vec3::new(to.x - pos.x, 0.0, to.z - pos.z);
            let d = left.length();
            if d < 1e-3 {
                break;
            }
            let next = self.step(&mut body, pos, pos + left * (RUN_STEP.min(d) / d), STEP_TICKS);
            let gain = d - Vec3::new(to.x - next.x, 0.0, to.z - next.z).length();
            stuck = if gain < 0.002 { stuck + 1 } else { 0 };
            pos = next;
            if stuck > 4 {
                break;
            }
        }
        pos
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A ground from quads (a b c d), each wound so its stored normal is on the `facing` side.
    fn ground(quads: &[([Vec3; 4], Vec3)]) -> Ground {
        let mut tris = Vec::new();
        let mut normals = Vec::new();
        for &([a, b, c, d], facing) in quads {
            for t in [[a, b, c], [a, c, d]] {
                // Retail data: cross(b - a, c - a) points against the stored normal.
                let g = (t[1] - t[0]).cross(t[2] - t[0]);
                let t = if g.dot(facing) > 0.0 { [t[0], t[2], t[1]] } else { t };
                tris.push(t);
                normals.push(-(t[1] - t[0]).cross(t[2] - t[0]).normalize());
            }
        }
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            let (lo, hi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
            let (a, b) = (cell(lo.x - MARGIN, lo.z - MARGIN), cell(hi.x + MARGIN, hi.z + MARGIN));
            for cx in a.0..=b.0 {
                for cz in a.1..=b.1 {
                    cells.entry((cx, cz)).or_default().push(i as u32);
                }
            }
        }
        let n = tris.len();
        Ground { tris, normals, y_range: vec![(f32::MIN, f32::MAX); n], degenerate: vec![false; n], cells, grid: None, body: Cell::new(Body::NEW) }
    }

    fn quad(x0: f32, x1: f32, y0: f32, y1: f32) -> [Vec3; 4] {
        [Vec3::new(x0, y0, -5.0), Vec3::new(x1, y1, -5.0), Vec3::new(x1, y1, 5.0), Vec3::new(x0, y0, 5.0)]
    }

    /// Floor at y 0 for x < 0, a riser at x 0 and a top at y -h beyond.
    fn step(h: f32) -> Ground {
        let riser = [Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, -h, -5.0), Vec3::new(0.0, -h, 5.0), Vec3::new(0.0, 0.0, 5.0)];
        ground(&[(quad(-10.0, 0.0, 0.0, 0.0), Vec3::NEG_Y), (riser, Vec3::NEG_X), (quad(0.0, 10.0, -h, -h), Vec3::NEG_Y)])
    }

    /// Floor at y 0 for x < 0, then a slope rising `deg` degrees for 10 units.
    fn slope(deg: f32) -> Ground {
        let rise = 10.0 * deg.to_radians().tan();
        ground(&[(quad(-10.0, 0.0, 0.0, 0.0), Vec3::NEG_Y), (quad(0.0, 10.0, 0.0, -rise), Vec3::new(-1.0, -1.0, 0.0))])
    }

    /// Risers below the sphere's centre are stepped over. (Between 0.5 and 1.0 the result depends on face
    /// order: a wall push and a floor fix can both land in one pass, and the later face sets the height.)
    #[test]
    fn steps_up_to_half_a_unit() {
        let from = Vec3::new(-3.0, 0.0, 0.0);
        let got = step(0.45).walk(from, Vec3::new(4.0, 0.0, 0.0), 1.2);
        assert!((got.x - 4.0).abs() < 0.05 && (got.y + 0.45).abs() < 0.01, "{got}");
        let got = step(1.2).walk(from, Vec3::new(4.0, 0.0, 0.0), 1.2);
        assert!(got.x < 0.0 && got.y.abs() < 0.05, "{got}");
    }

    /// A 20 degree slope at run speed: uphill grounded every step; downhill (the move stays level, then gravity)
    /// at most one step in the air at a time, always descending. Per-tick gravity probed too little to reach the
    /// slope after a floor fix, so the player bounced.
    #[test]
    fn stays_grounded_on_a_slope() {
        let g = slope(20.0);
        let mut pos = g.walk_player(Vec3::new(2.0, -1.0, 0.0), Vec3::new(2.0, -1.0, 0.0), STEP_TICKS);
        for _ in 0..10 {
            pos = g.walk_player(pos, pos, STEP_TICKS);
        }
        for dir in [1.0, -1.0] {
            let (mut last, mut air) = (pos.y, 0);
            for i in 0..30 {
                pos = g.walk_player(pos, pos + Vec3::new(dir * RUN_STEP, 0.0, 0.0), STEP_TICKS);
                air = if g.body.get().grounded { 0 } else { air + 1 };
                assert!(air <= (dir < 0.0) as usize, "dir {dir} step {i} {pos}");
                assert!((pos.y - last) * dir < 0.0, "dir {dir} step {i}: y {last} -> {}", pos.y);
                last = pos.y;
            }
        }
    }

    #[test]
    fn slope_limit_is_45_degrees() {
        let from = Vec3::new(-3.0, 0.0, 0.0);
        // The feet ride above a slope: the centre goes 0.5 above the face point nearest it, not the one below.
        let got = slope(40.0).walk(from, Vec3::new(6.0, 0.0, 0.0), 1.2);
        let surface = -6.0 * 40f32.to_radians().tan();
        assert!((got.x - 6.0).abs() < 0.05 && got.y < surface && got.y > surface - 0.4, "{got}");
        let got = slope(50.0).walk(from, Vec3::new(6.0, 0.0, 0.0), 1.2);
        assert!(got.x < 0.6 && got.y > -0.6, "{got}");
    }

    /// Dev probe: P=zone,x,y,z,x2,z2 prints the faces along the line and 120 held frames.
    #[test]
    #[ignore]
    fn trace() {
        let Ok(p) = std::env::var("P") else { return };
        let v: Vec<f32> = p.split(',').map(|s| s.parse().unwrap()).collect();
        let g = zone(v[0] as u32).unwrap();
        let (s, e) = (Vec3::new(v[1], v[2], v[3]), Vec3::new(v[4], v[2], v[5]));
        for i in 0..=20 {
            let p = s + (e - s) * (i as f32 / 20.0);
            let hs: Vec<String> = g.heights(p.x, p.z).iter().filter(|h| (h.0 - s.y).abs() < 12.0).map(|&(h, t)| format!("{h:.2}({:.2})", g.normals[t].y)).collect();
            println!("p {:.2},{:.2}: {}", p.x, p.z, hs.join(" "));
        }
        let mut body = Body::NEW;
        let mut pos = s;
        let dir = (e - s).normalize();
        for f in 0..120 {
            pos = g.step(&mut body, pos, pos + dir * RUN_STEP, STEP_TICKS);
            println!("f{f} {pos} grounded {} normal {}", body.grounded, body.normal);
        }
    }

    #[test]
    fn walks_off_a_ledge_and_falls() {
        // Off the top of a 3-unit block: the fall goes on without input and ends on the floor.
        let g = step(3.0);
        let mut pos = Vec3::new(3.0, -3.0, 0.0);
        for _ in 0..30 {
            pos = g.walk_player(pos, pos + Vec3::new(-RUN_STEP, 0.0, 0.0), STEP_TICKS);
        }
        for _ in 0..30 {
            pos = g.walk_player(pos, pos, STEP_TICKS);
        }
        assert!(pos.x < -0.4 && pos.y.abs() < 0.01, "{pos}");
    }

    fn zone(id: u32) -> Option<Ground> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?).ok()?;
        Some(Ground::build(&crate::dat::zone::load_scene(&keys, &index.read(100 + id).ok()?).ok()?))
    }

    /// Holds the key from `from` towards `to` for `steps` physics steps.
    fn hold(g: &Ground, from: Vec3, to: Vec3, steps: usize) -> Vec3 {
        let dir = Vec3::new(to.x - from.x, 0.0, to.z - from.z).normalize();
        let mut pos = from;
        for _ in 0..steps {
            pos = g.walk_player(pos, pos + dir * RUN_STEP, STEP_TICKS);
        }
        pos
    }

    #[test]
    fn east_ronfaure_hillsides_block() {
        let Some(g) = zone(101) else { return };
        // South of the San d'Oria gate: 11 units of 49-64 degree hillside rise beyond these spots.
        // Holding the key slides along the foot of the slope (a wall push each frame) but gains no ground uphill.
        for (from, to) in [(Vec3::new(27.69, -52.28, 53.10), Vec3::new(30.16, -52.28, 62.79)), (Vec3::new(-48.73, -60.21, 102.19), Vec3::new(-44.25, -60.21, 95.55))] {
            let dir = (to - from).normalize();
            let got = g.walk(from, to, 1.2);
            assert!(from.y - got.y < 0.8 && (got - from).dot(dir) < 1.5, "walk {from} -> {got}");
            // Retail-rate gravity (per 30 fps frame) lets the slide along the foot carry a little further.
            let got = hold(&g, from, to, 150);
            assert!(from.y - got.y < 0.8 && (got - from).dot(dir) < 2.0, "hold {from} -> {got}");
        }
    }

    #[test]
    fn southern_san_doria_stairs_and_ramps() {
        let Some(g) = zone(230) else { return };
        // Stairs: 0.25 risers every 0.5, from -0.25 up to -3.
        let (from, to) = (Vec3::new(-22.0, -0.25, -36.0), Vec3::new(-14.0, -0.25, -36.0));
        let got = g.walk(from, to, 1.2);
        assert!((got.x - to.x).abs() < 0.2 && (got.y + 3.0).abs() < 0.05, "stairs {got}");
        let got = hold(&g, from, to, 48);
        assert!((got.x - to.x).abs() < 0.2 && (got.y + 3.0).abs() < 0.05, "stairs held {got}");
        // A 39 degree ramp, up 3 units and back down.
        let (top, bottom) = (Vec3::new(72.9, 0.0, 58.0), Vec3::new(80.9, 3.0, 58.0));
        let got = g.walk(bottom, top, 1.2);
        assert!((got.x - top.x).abs() < 0.2 && got.y.abs() < 0.1, "ramp up {got}");
        let got = g.walk(top, bottom, 1.2);
        assert!((got.x - bottom.x).abs() < 0.2 && (got.y - 3.0).abs() < 0.1, "ramp down {got}");
    }
}
