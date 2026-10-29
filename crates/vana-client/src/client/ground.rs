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
    /// +0x5e2: on a lift, with its surface type.
    riding: Option<i8>,
}

impl Body {
    const NEW: Body = Body { at: None, vel: 0.0, grounded: false, normal: Vec3::NEG_Y, fell: false, riding: None };
}

/// The lift test (FUN_10181560 -> FUN_10176620): a lift box holding the frame's start or end gives its height and
/// surface type (client/lifts.rs).
pub type Ride<'a> = &'a dyn Fn(Vec3, Vec3) -> Option<(f32, i8)>;

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
    /// Per triangle: its object is covered (+0xa4 bit 2), and its surface type (the four bit-15s).
    covers: Vec<bool>,
    /// Per triangle: its object's map sub-key (map_key_of).
    map_keys: Vec<u8>,
    kinds: Vec<u8>,
    /// Per triangle: its object's area 4cc (+0xb0), and its object's index.
    areas: Vec<u32>,
    objs: Vec<u32>,
    /// Per object: its four point lights (table slot + 1, 0 = none or past slot 254), and its type-9 water height.
    obj_lights: Vec<[u8; 4]>,
    obj_water: Vec<f32>,
    /// Per object: its camera list's MZB offset (+0xa8, 0 none).
    obj_vis: Vec<u32>,
    /// Per triangle: the door whose box it is (0 none), and the doors open now (their boxes are skipped).
    doors: Vec<u32>,
    open: std::cell::RefCell<std::collections::HashSet<u32>>,
    /// Per triangle: the camera's rays pass it (cam_ray).
    cam_pass: Vec<bool>,
    /// Per triangle: effect ground projections paint it (Collision::decal).
    decal: Vec<bool>,
    /// Mog House furniture boxes (FUN_1016d580): each piece's position and its 12 triangles with normals.
    furniture: Vec<(Vec3, Vec<([Vec3; 3], Vec3)>)>,
    /// The furniture meshes the camera ray tests (FUN_1016b1d0), each with its bounds; while `furcc` (config 0x99 bit 6
    /// clear, /furcc on).
    furn_mesh: Vec<(Vec3, Vec3, Vec<([Vec3; 3], Vec3)>)>,
    pub furcc: bool,
}

/// FUN_1016d580's box: corners c0..c7 at local x0 / x1 (+-width / 2), the fixed top y -1.0 or y1 (+height / 2), z1 /
/// z0, triangles from table 0x103769c8 (the x0 face's second is (2, 6, 5) in the data) with the normals of 0x10376a28,
/// all turned by the piece's matrix.
pub fn furniture_box(m: glam::Mat4, size: Vec3) -> Vec<([Vec3; 3], Vec3)> {
    let (x0, x1, y1, z0, z1) = (-size.x / 2.0, size.x / 2.0, size.y / 2.0, -size.z / 2.0, size.z / 2.0);
    let c = [
        Vec3::new(x0, -1.0, z1),
        Vec3::new(x1, -1.0, z1),
        Vec3::new(x0, -1.0, z0),
        Vec3::new(x1, -1.0, z0),
        Vec3::new(x0, y1, z1),
        Vec3::new(x1, y1, z1),
        Vec3::new(x0, y1, z0),
        Vec3::new(x1, y1, z0),
    ]
    .map(|p| m.transform_point3(p));
    const N: [Vec3; 6] = [Vec3::NEG_Y, Vec3::Y, Vec3::Z, Vec3::NEG_Z, Vec3::X, Vec3::NEG_X];
    const T: [(usize, usize, usize, usize); 12] = [(0, 1, 2, 0), (1, 3, 2, 0), (5, 4, 7, 1), (4, 6, 7, 1), (0, 2, 4, 5), (2, 6, 5, 5), (1, 0, 5, 2), (0, 4, 5, 2), (3, 1, 7, 4), (1, 5, 7, 4), (2, 3, 6, 3), (3, 7, 6, 3)];
    T.iter().map(|&(a, b, d, n)| ([c[a], c[b], c[d]], m.transform_vector3(N[n]))).collect()
}

/// 4-unit cells centred on the origin, each listing (first triangle, count, resolved late) runs.
struct Grid {
    w: usize,
    h: usize,
    cells: Vec<Vec<u32>>,
    entries: Vec<(u32, u32, bool)>,
    /// Per entry: its triangles' bounds (ours: entries a segment's box misses are skipped, the result is the same).
    bounds: Vec<(Vec3, Vec3)>,
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

/// The map sub-key in an MZB object's flags (@0xa4): ((f >> 26) & 3) << 3 | ((f >> 3) & 7), matched against the
/// region table's key (the floor or sub-map).
pub fn map_key_of(flags: u32) -> u8 {
    ((((flags >> 26) & 3) << 3) | ((flags >> 3) & 7)) as u8
}

impl Ground {
    /// The furniture boxes of the pieces standing on the floor (FUN_1016e930: pos.y >= -0.1), and every piece's mesh
    /// triangles (world space) for the camera.
    pub fn set_furniture(&mut self, boxes: Vec<(glam::Mat4, Vec3)>, meshes: Vec<Vec<[Vec3; 3]>>) {
        self.furniture = boxes.into_iter().filter(|(m, _)| m.w_axis.y >= -0.1).map(|(m, size)| (m.w_axis.truncate(), furniture_box(m, size))).collect();
        // Normals as the zone's: against cross(b - a, c - a), which inside() expects.
        self.furn_mesh = meshes
            .into_iter()
            .filter(|t| !t.is_empty())
            .map(|t| {
                let lo = t.iter().flatten().fold(Vec3::splat(f32::MAX), |a, &p| a.min(p));
                let hi = t.iter().flatten().fold(Vec3::splat(f32::MIN), |a, &p| a.max(p));
                (lo, hi, t.into_iter().map(|[a, b, c]| ([a, b, c], (c - a).cross(b - a).normalize_or_zero())).filter(|(_, n)| *n != Vec3::ZERO).collect())
            })
            .collect();
    }

    pub fn build(scene: &ZoneScene) -> Self {
        match scene.collision.as_ref() {
            Some(c) => Self::from_collision(c, &scene.light_keys),
            None => Self { tris: Vec::new(), normals: Vec::new(), y_range: Vec::new(), degenerate: Vec::new(), cells: HashMap::new(), grid: None, body: Cell::new(Body::NEW), covers: Vec::new(), map_keys: Vec::new(), kinds: Vec::new(), areas: Vec::new(), objs: Vec::new(), obj_lights: Vec::new(), obj_water: Vec::new(), obj_vis: Vec::new(), doors: Vec::new(), open: Default::default(), cam_pass: Vec::new(), decal: Vec::new(), furniture: Vec::new(), furn_mesh: Vec::new(), furcc: true },
        }
    }

    /// Takes over the player's movement state from the ground it replaces (a sub-area came or went).
    pub fn keep_body(&self, from: &Ground) {
        self.body.set(from.body.get());
    }

    /// `light_keys`: the point-light table the objects' light keys resolve against.
    pub fn from_collision(c: &crate::dat::collision::Collision, light_keys: &[u32]) -> Self {
        let tris = c.tris.clone();
        let y_range = c.object.iter().map(|&o| c.objects[o as usize].y_range).collect();
        let covers = c.object.iter().map(|&o| c.objects[o as usize].flags & 4 != 0).collect();
        let map_keys = c.object.iter().map(|&o| map_key_of(c.objects[o as usize].flags)).collect();
        let areas = c.object.iter().map(|&o| c.objects[o as usize].area).collect();
        // FUN_1017aba0: keys with bytes 1..3 'lgb' are skipped.
        let slot = |key: u32| match light_keys.iter().position(|&k| k == key) {
            Some(s) if key != 0 && key & 0xffff_ff00 != 0x6267_6c00 && s < 255 => s as u8 + 1,
            _ => 0,
        };
        let obj_lights = c.objects.iter().map(|o| o.lights.map(slot)).collect();
        let obj_water = c.objects.iter().map(|o| crate::dat::collision::water_of(o.flags)).collect();
        let obj_vis = c.objects.iter().map(|o| o.vis).collect();
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
        let bounds = c.entries.iter().map(|e| tris[e.first as usize..(e.first + e.count) as usize].iter().flatten().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)))).collect();
        let grid = Some(Grid { w: c.grid.0, h: c.grid.1, cells: c.cells.clone(), entries, bounds });
        let doors = c.object.iter().map(|&o| c.objects[o as usize].door).collect();
        Self { tris, normals: c.normals.clone(), y_range, degenerate, cells, grid, body: Cell::new(Body::NEW), covers, map_keys, kinds: c.kinds.clone(), areas, objs: c.object.clone(), obj_lights, obj_water, obj_vis, doors, open: Default::default(), cam_pass: c.cam_pass.clone(), decal: c.decal.clone(), furniture: Vec::new(), furn_mesh: Vec::new(), furcc: true }
    }

    /// FUN_10182700's cells: the MZB cell of `c` (truncated) and its neighbours clamped to the grid; None without one.
    fn decal_range(&self, c: Vec3) -> Option<(usize, usize, usize, usize)> {
        let g = self.grid.as_ref()?;
        let (cx, cz) = (((c.x + g.w as f32 * 2.0) * 0.25) as i32, ((c.z + g.h as f32 * 2.0) * 0.25) as i32);
        let clamp = |v: i32, n: usize| if v < 0 { 0 } else { (v as usize).min(n.saturating_sub(1)) };
        Some((clamp(cx - 1, g.w), clamp(cx + 1, g.w), clamp(cz - 1, g.h), clamp(cz + 1, g.h)))
    }

    /// The cells a ground projection at `c` paints (changes only when `c` moves to another cell).
    pub fn decal_cell(&self, c: Vec3) -> Option<(usize, usize)> {
        self.decal_range(c).map(|(x0, _, z0, _)| (x0, z0))
    }

    /// FUN_10182700's faces around `c`: z outer, x inner, each cell's listed runs in order (repeats kept), the
    /// eligible triangles of each, at most 1501 per listing; world vertices, three a triangle.
    pub fn decal_tris(&self, c: Vec3) -> Vec<Vec3> {
        let (Some(g), Some((x0, x1, z0, z1))) = (self.grid.as_ref(), self.decal_range(c)) else { return Vec::new() };
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &k in g.cells.get(z * g.w + x).map_or(&[][..], |v| &v[..]) {
                    let Some(&(first, count, _)) = g.entries.get(k as usize) else { continue };
                    let mut n = 0;
                    for t in first as usize..(first + count) as usize {
                        if self.decal.get(t) != Some(&true) {
                            continue;
                        }
                        out.extend_from_slice(&self.tris[t]);
                        n += 1;
                        if n > 1500 {
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    /// FUN_1017ac40: a door's box is skipped while it is open (state 0), solid while closed.
    pub fn set_door(&self, door: u32, solid: bool) {
        let mut open = self.open.borrow_mut();
        if solid { open.remove(&door) } else { open.insert(door) };
    }

    /// FUN_10176dd0(0): every door box solid (a sub-area load starts).
    pub fn close_all_doors(&self) {
        self.open.borrow_mut().clear();
    }

    /// Triangle `i` counts: not the box of an open door.
    fn live(&self, i: usize) -> bool {
        match self.doors.get(i) {
            Some(&d) if d != 0 => !self.open.borrow().contains(&d),
            _ => true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// The floor under feet at `p` (FUN_10162120): its surface type (0 when the face is more than 0.4 below the
    /// feet, FUN_10162470) and whether its object is covered (actor+0x17c).
    /// The same face's normal, turned to +y (into the floor; -vt+0x210 *(inf: the sign, from flat ground giving
    /// FUN_100d3750 the identity)*).
    pub fn floor_normal(&self, p: Vec3) -> Option<Vec3> {
        let (_, i) = self.heights(p.x, p.z).into_iter().filter(|h| h.0 >= p.y - 0.5).min_by(|a, b| a.0.total_cmp(&b.0))?;
        let n = *self.normals.get(i)?;
        Some(if n.y < 0.0 { -n } else { n })
    }

    /// The point lights of the floor under `p` (ground info +0x20..+0x23, FUN_1017aba0).
    pub fn lights(&self, p: Vec3) -> [u8; 4] {
        let floor = self.heights(p.x, p.z).into_iter().filter(|h| h.0 >= p.y - 0.5).min_by(|a, b| a.0.total_cmp(&b.0));
        floor.and_then(|(_, i)| self.objs.get(i)).and_then(|&o| self.obj_lights.get(o as usize)).copied().unwrap_or([0; 4])
    }

    /// The area 4cc of the floor under `p` (FUN_100a3560: the ground info +0x34), None without a floor.
    pub fn area(&self, p: Vec3) -> Option<u32> {
        let (_, i) = self.heights(p.x, p.z).into_iter().filter(|h| h.0 >= p.y - 0.5).min_by(|a, b| a.0.total_cmp(&b.0))?;
        self.areas.get(i).copied()
    }

    /// The wade height under `p` (FUN_100d4bc0: the floor's type 9 and its object's water, actor +0x174 / +0x178);
    /// None off a type-9 floor (the face `surface` takes).
    pub fn wade(&self, p: Vec3) -> Option<f32> {
        let (h, i) = self.heights(p.x, p.z).into_iter().filter(|h| h.0 >= p.y - 0.5).min_by(|a, b| a.0.total_cmp(&b.0))?;
        (h - p.y <= 0.4 && self.kinds.get(i) == Some(&9)).then(|| self.objs.get(i).and_then(|&o| self.obj_water.get(o as usize)).copied().unwrap_or(0.0))
    }

    pub fn surface(&self, p: Vec3) -> (u8, bool) {
        let floor = self.heights(p.x, p.z).into_iter().filter(|h| h.0 >= p.y - 0.5).min_by(|a, b| a.0.total_cmp(&b.0));
        match floor {
            Some((h, i)) if h - p.y <= 0.4 => (self.kinds.get(i).copied().unwrap_or(0), self.covers.get(i).copied().unwrap_or(false)),
            Some((_, i)) => (0, self.covers.get(i).copied().unwrap_or(false)),
            None => (0, false),
        }
    }

    /// FUN_100136a0 / FUN_1017a4f0(eye, 100): a ray from just above the eye 100 down, else from just below it 15
    /// up; the nearest face hit says whether its object is covered (+0xa4 bit 2), which stops weather.
    pub fn covered(&self, eye: Vec3) -> bool {
        let hits = self.heights(eye.x, eye.z);
        let down = hits.iter().filter(|h| h.0 >= eye.y - 0.51 && h.0 <= eye.y + 100.0).min_by(|a, b| a.0.total_cmp(&b.0));
        let up = || hits.iter().filter(|h| h.0 <= eye.y + 0.51 && h.0 >= eye.y - 15.0).max_by(|a, b| a.0.total_cmp(&b.0));
        down.or_else(up).is_some_and(|h| self.covers.get(h.1).copied().unwrap_or(false))
    }

    /// FUN_101813f0 / FUN_1016a2d0 for the camera lists (FUN_10179390): a ray from 0.51 above the eye 100 down, else
    /// from 0.51 below it 15 up; the nearest face's object names the list (+0xa8, the MZB offset; 0 none or a miss).
    /// *(inf: door boxes name none; retail links them in lists of their own)*
    pub fn camera_list(&self, eye: Vec3) -> u32 {
        let hits = self.heights(eye.x, eye.z);
        let down = hits.iter().filter(|h| h.0 >= eye.y - 0.51 && h.0 <= eye.y + 100.0).min_by(|a, b| a.0.total_cmp(&b.0));
        let up = || hits.iter().filter(|h| h.0 <= eye.y + 0.51 && h.0 >= eye.y - 15.0).max_by(|a, b| a.0.total_cmp(&b.0));
        down.or_else(up).and_then(|h| self.objs.get(h.1)).and_then(|&o| self.obj_vis.get(o as usize)).copied().unwrap_or(0)
    }

    /// FUN_101811d0: the map sub-key under a point, from the nearest face a ray from 0.51 above it 10 down hits,
    /// else one from 0.51 below it 15 up; 0 when both miss.
    pub fn map_key(&self, p: Vec3) -> u8 {
        let hits = self.heights(p.x, p.z);
        let down = hits.iter().filter(|h| h.0 >= p.y - 0.51 && h.0 <= p.y - 0.51 + 10.0).min_by(|a, b| a.0.total_cmp(&b.0));
        let up = || hits.iter().filter(|h| h.0 <= p.y + 0.51 && h.0 >= p.y + 0.51 - 15.0).max_by(|a, b| a.0.total_cmp(&b.0));
        down.or_else(up).and_then(|h| self.map_keys.get(h.1).copied()).unwrap_or(0)
    }

    fn near(&self, x: f32, z: f32) -> impl Iterator<Item = usize> + '_ {
        self.cells.get(&cell(x, z)).into_iter().flatten().map(|&i| i as usize).filter(|&i| self.live(i))
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

    /// The heights of every face over (x, z), top first (diagnostics).
    pub fn floors(&self, x: f32, z: f32) -> Vec<f32> {
        let mut h: Vec<f32> = self.heights(x, z).into_iter().map(|h| (h.0 * 100.0).round() / 100.0).collect();
        h.sort_by(|a, b| a.total_cmp(b));
        h.dedup();
        h
    }

    /// FUN_101815d0: no wall within 4 ahead of 1 above the feet (FUN_1016b1d0), and the first face under that point
    /// (2 above the feet down to 98 below) is water (type 9).
    pub fn water_ahead(&self, pos: Vec3, forward: Vec3) -> bool {
        let start = pos - Vec3::Y;
        let p = start + forward * 4.0;
        if self.grid.as_ref().is_some_and(|g| self.cam_piece(g, start, p).is_some()) {
            return false;
        }
        self.ray_down(p.x, p.z, p.y - 1.0, p.y + 99.0).is_some_and(|(_, i)| self.kinds.get(i) == Some(&9))
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

    /// FUN_10169140: the camera's segment test. A segment of 4 or more goes in 1 + trunc(len / 4) equal pieces, tested
    /// in order; the first piece with a hit gives its point.
    pub fn cam_ray(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let g = self.grid.as_ref()?;
        let n = 1 + ((to - from).length() * 0.25) as usize;
        (0..n).find_map(|k| self.cam_piece(g, from.lerp(to, k as f32 / n as f32), from.lerp(to, (k + 1) as f32 / n as f32)))
    }

    /// FUN_1016b1d0: the MZB cells within 2 of the start's (none when the start is off the grid), objects whose y range
    /// meets the piece's, closed doors' boxes; faces the camera passes are skipped. Both sides count, 0 <= t < 1, edges
    /// inclusive (FUN_10165570, 1e-4); the hit strictly nearest the start.
    fn cam_piece(&self, g: &Grid, s: Vec3, e: Vec3) -> Option<Vec3> {
        let (cx, cz) = (((s.x + g.w as f32 * 2.0) * 0.25) as i32, ((s.z + g.h as f32 * 2.0) * 0.25) as i32);
        if cx < 0 || cz < 0 || cx >= g.w as i32 || cz >= g.h as i32 {
            return None;
        }
        let (lo, hi) = (s.min(e), s.max(e));
        let mut seen = std::collections::HashSet::new();
        let (mut best, mut hit) = (1e6f32, None);
        for z in (cz - 2).max(0)..=(cz + 2).min(g.h as i32 - 1) {
            for x in (cx - 2).max(0)..=(cx + 2).min(g.w as i32 - 1) {
                for &en in &g.cells[z as usize * g.w + x as usize] {
                    let (bl, bh) = g.bounds[en as usize];
                    if !seen.insert(en) || bh.x < lo.x || bl.x > hi.x || bh.y < lo.y || bl.y > hi.y || bh.z < lo.z || bl.z > hi.z {
                        continue;
                    }
                    let (first, count, _) = g.entries[en as usize];
                    for i in first as usize..(first + count) as usize {
                        let (ylo, yhi) = self.y_range[i];
                        if self.cam_pass[i] || yhi < lo.y || ylo > hi.y || !self.live(i) {
                            continue;
                        }
                        let [a, b, c] = self.tris[i];
                        let n = self.normals[i];
                        let d = n.dot(e - s);
                        if d == 0.0 {
                            continue;
                        }
                        let t = n.dot(a - s) / d;
                        if !(0.0..1.0).contains(&t) {
                            continue;
                        }
                        let p = s + (e - s) * t;
                        let d2 = (p - s).length_squared();
                        if d2 < best && inside(a, b, c, p, n) {
                            (best, hit) = (d2, Some(p));
                        }
                    }
                }
            }
        }
        // Then the furniture meshes (FUN_100c4bc0), both sides.
        if self.furcc {
            for (bl, bh, tris) in &self.furn_mesh {
                if bh.x < lo.x || bl.x > hi.x || bh.y < lo.y || bl.y > hi.y || bh.z < lo.z || bl.z > hi.z {
                    continue;
                }
                for &([a, b, c], n) in tris {
                    let d = n.dot(e - s);
                    if d == 0.0 {
                        continue;
                    }
                    let t = n.dot(a - s) / d;
                    if !(0.0..1.0).contains(&t) {
                        continue;
                    }
                    let p = s + (e - s) * t;
                    let d2 = (p - s).length_squared();
                    if d2 < best && inside(a, b, c, p, n) {
                        (best, hit) = (d2, Some(p));
                    }
                }
            }
        }
        hit
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
            if self.degenerate[i] || ymin > y_hi || ymax < y_lo || !self.live(i) {
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
        // FUN_1016e930 / FUN_1016d580: the boxes of furniture within 4 units (xz) of the start, 12 each while under 0x3fc.
        for (pos, tris) in &self.furniture {
            if xz(*pos - from).length() > 4.0 {
                continue;
            }
            if cands.len() + 12 > 0x3fc {
                break;
            }
            cands.extend(tris.iter().map(|&(v, n)| Cand { v, n, nz: n.dot(fz) }));
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
    /// `ride` is FUN_100a4ed0's step 3: inside a lift box the feet go to its height, grounded, and the missed-floor
    /// undo is skipped.
    fn step(&self, body: &mut Body, from: Vec3, to: Vec3, ticks: f32, ride: Option<Ride>) -> Vec3 {
        if body.at != Some(from) {
            *body = Body::NEW;
        }
        if body.grounded {
            body.vel = 0.0;
        }
        let mut v = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
        let moving = v.length_squared() > 1e-14;
        if !moving && body.grounded {
            let mut pos = from;
            if let Some((h, surf)) = ride.and_then(|r| r(from, from)) {
                pos.y = h;
                body.riding = Some(surf);
            } else {
                body.riding = None;
            }
            body.at = Some(pos);
            return pos;
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
        let floor = self.ray_down(pos.x, pos.z, pos.y - 0.5, pos.y + 40.0);
        if let Some((_, i)) = floor {
            let n = self.normals[i];
            if n.y > 0.7 || n.y < -0.7 {
                body.normal = n;
            }
        }
        // Then the lift: its height, grounded, the normal (0, -1, 0) and its surface; the undo only outside one.
        match ride.and_then(|r| r(from, pos)) {
            Some((h, surf)) => {
                pos.y = h;
                (body.grounded, body.normal, body.riding) = (true, Vec3::NEG_Y, Some(surf));
            }
            None => {
                body.riding = None;
                if floor.is_none() {
                    pos = from;
                    body.vel = 0.0;
                }
            }
        }
        body.at = Some(pos);
        pos
    }

    /// The last player step's lift surface type, when it stood on a lift.
    #[cfg(test)]
    pub fn riding(&self) -> Option<i8> {
        self.body.get().riding
    }

    /// The player's frame: walks from `from` towards `to` (horizontal), falling while in the air. Call it every
    /// frame the player could fall, also without input (`to == from`).
    /// The player's ground normal (actor +0x160: y < 0 up, only faces with |y| > 0.7, (0, -1, 0) on a lift).
    pub fn player_normal(&self) -> Vec3 {
        self.body.get().normal
    }

    pub fn walk_player(&self, from: Vec3, to: Vec3, ticks: f32, ride: Option<Ride>) -> Vec3 {
        let mut body = self.body.get();
        let pos = self.step(&mut body, from, to, ticks, ride);
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
            let next = self.step(&mut body, pos, pos + left * (RUN_STEP.min(d) / d), STEP_TICKS, None);
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
        // One MZB entry listed in every cell of a 256-unit grid, for cam_ray.
        let bounds = vec![tris.iter().flatten().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)))];
        let grid = Some(Grid { w: 64, h: 64, cells: vec![vec![0]; 64 * 64], entries: vec![(0, n as u32, false)], bounds });
        Ground { tris, normals, y_range: vec![(f32::MIN, f32::MAX); n], degenerate: vec![false; n], cells, grid, body: Cell::new(Body::NEW), covers: vec![false; n], map_keys: vec![0; n], kinds: vec![0; n], areas: vec![0; n], objs: vec![0; n], obj_lights: vec![[0; 4]], obj_water: vec![0.0], obj_vis: vec![0], doors: vec![0; n], open: Default::default(), cam_pass: vec![false; n], decal: vec![true; n], furniture: Vec::new(), furn_mesh: Vec::new(), furcc: true }
    }

    fn quad(x0: f32, x1: f32, y0: f32, y1: f32) -> [Vec3; 4] {
        [Vec3::new(x0, y0, -5.0), Vec3::new(x1, y1, -5.0), Vec3::new(x1, y1, 5.0), Vec3::new(x0, y0, 5.0)]
    }

    /// A type-9 face gives its object's water height within 0.4 below the point; 0.5 above it, none.
    #[test]
    fn wade_on_type9() {
        let mut g = ground(&[(quad(-10.0, 10.0, -6.5, -6.5), Vec3::NEG_Y)]);
        g.obj_water = vec![-7.0];
        assert_eq!(g.wade(Vec3::new(0.0, -6.5, 0.0)), None);
        g.kinds = vec![9; g.kinds.len()];
        assert_eq!(g.wade(Vec3::new(0.0, -6.5, 0.0)), Some(-7.0));
        assert_eq!(g.wade(Vec3::new(0.0, -7.0, 0.0)), None);
    }

    /// Furniture: a bed's box stops a walk into it, its mesh stops the camera ray while /furcc is on.
    #[test]
    fn furniture_box_and_camera() {
        let mut g = ground(&[(quad(-10.0, 10.0, 0.0, 0.0), Vec3::NEG_Y)]);
        let bed = glam::Mat4::from_translation(Vec3::new(-4.8, 0.0, 0.0));
        let wall = vec![[Vec3::new(-3.6, -1.0, -1.0), Vec3::new(-3.6, 0.0, -1.0), Vec3::new(-3.6, 0.0, 1.0)], [Vec3::new(-3.6, -1.0, -1.0), Vec3::new(-3.6, 0.0, 1.0), Vec3::new(-3.6, -1.0, 1.0)]];
        g.set_furniture(vec![(bed, Vec3::new(2.4, 0.7, 3.6))], vec![wall]);
        let (to, _) = g.sweep(Vec3::new(-2.0, 0.0, 0.0), Vec3::new(-3.4, 0.0, 0.0));
        assert!(to.x > -3.2, "walked into the bed: {to:?}");
        let hit = g.cam_ray(Vec3::new(0.0, -0.5, 0.0), Vec3::new(-6.0, -0.5, 0.0));
        assert!(hit.is_some_and(|h| (h.x + 3.6).abs() < 1e-3), "{hit:?}");
        g.furcc = false;
        assert_eq!(g.cam_ray(Vec3::new(0.0, -0.5, 0.0), Vec3::new(-6.0, -0.5, 0.0)), None);
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
    /// FUN_100136a0: La Theine (102) has covered collision objects (+0xa4 bit 2) near
    /// (-175.5, -509); the open field at the origin is not covered.
    #[test]
    fn la_theine_covered() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let g = Ground::build(&crate::dat::zone::load_scene(&keys, &index.read(202).unwrap()).unwrap());
        assert!(g.covered(Vec3::new(-175.5, 31.0, -509.0)));
        assert!(!g.covered(Vec3::new(0.0, -4.0, 0.0)));
    }

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
        let mut pos = g.walk_player(Vec3::new(2.0, -1.0, 0.0), Vec3::new(2.0, -1.0, 0.0), STEP_TICKS, None);
        for _ in 0..10 {
            pos = g.walk_player(pos, pos, STEP_TICKS, None);
        }
        for dir in [1.0, -1.0] {
            let (mut last, mut air) = (pos.y, 0);
            for i in 0..30 {
                pos = g.walk_player(pos, pos + Vec3::new(dir * RUN_STEP, 0.0, 0.0), STEP_TICKS, None);
                air = if g.body.get().grounded { 0 } else { air + 1 };
                assert!(air <= (dir < 0.0) as usize, "dir {dir} step {i} {pos}");
                assert!((pos.y - last) * dir < 0.0, "dir {dir} step {i}: y {last} -> {}", pos.y);
                last = pos.y;
            }
        }
    }

    /// A lift (FUN_100a4ed0 step 3): no floor under the shaft, yet the feet hold its height without the undo, standing
    /// still too; out of it gravity is back.
    #[test]
    fn rides_a_lift() {
        let g = ground(&[(quad(-10.0, 0.0, 0.0, 0.0), Vec3::NEG_Y)]);
        let shaft = |_: Vec3, b: Vec3| (b.x > -0.5).then_some((-5.0, 3i8));
        let mut pos = g.walk_player(Vec3::new(-2.0, 0.0, 0.0), Vec3::new(-2.0, 0.0, 0.0), STEP_TICKS, Some(&shaft));
        for _ in 0..20 {
            pos = g.walk_player(pos, pos + Vec3::new(RUN_STEP, 0.0, 0.0), STEP_TICKS, Some(&shaft));
        }
        assert!(pos.x > 1.0 && (pos.y + 5.0).abs() < 1e-5, "{pos}");
        assert_eq!(g.riding(), Some(3));
        assert_eq!(g.walk_player(pos, pos, STEP_TICKS, Some(&shaft)), pos);
        for _ in 0..60 {
            pos = g.walk_player(pos, pos + Vec3::new(-RUN_STEP, 0.0, 0.0), STEP_TICKS, Some(&shaft));
        }
        assert!(pos.x < -0.5 && pos.y.abs() < 0.05 && g.riding().is_none(), "{pos}");
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
            pos = g.step(&mut body, pos, pos + dir * RUN_STEP, STEP_TICKS, None);
            println!("f{f} {pos} grounded {} normal {}", body.grounded, body.normal);
        }
    }

    #[test]
    fn walks_off_a_ledge_and_falls() {
        // Off the top of a 3-unit block: the fall goes on without input and ends on the floor.
        let g = step(3.0);
        let mut pos = Vec3::new(3.0, -3.0, 0.0);
        for _ in 0..30 {
            pos = g.walk_player(pos, pos + Vec3::new(-RUN_STEP, 0.0, 0.0), STEP_TICKS, None);
        }
        for _ in 0..30 {
            pos = g.walk_player(pos, pos, STEP_TICKS, None);
        }
        assert!(pos.x < -0.4 && pos.y.abs() < 0.01, "{pos}");
    }

    fn zone(id: u32) -> Option<Ground> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?).ok()?;
        Some(Ground::build(&crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(id)).ok()?).ok()?))
    }

    /// Holds the key from `from` towards `to` for `steps` physics steps.
    fn hold(g: &Ground, from: Vec3, to: Vec3, steps: usize) -> Vec3 {
        let dir = Vec3::new(to.x - from.x, 0.0, to.z - from.z).normalize();
        let mut pos = from;
        for _ in 0..steps {
            pos = g.walk_player(pos, pos + dir * RUN_STEP, STEP_TICKS, None);
        }
        pos
    }

    /// A zone's collision as loaded (by hand: `cargo test --release collision_of -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn collision_of() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).unwrap()).unwrap();
        for id in [194u32, 116] {
            let s = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(id)).unwrap()).unwrap();
            let c = s.collision.as_ref();
            println!("zone {id}: file {}, warnings {:?}, collision tris {:?}", crate::dat::zone::file_id(id), s.warnings, c.map(|c| c.tris.len()));
            if let Some(c) = c {
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for t in &c.tris {
                    for v in t {
                        lo = lo.min(*v);
                        hi = hi.max(*v);
                    }
                }
                println!("  bounds {lo} .. {hi}");
            }
            let g = Ground::build(&s);
            println!("  snap at the zone-in: {:?}", g.snap(578.9, -637.0, -25.6));
        }
    }

    /// Every zone's collision parses (by hand: `cargo test --release all_zones_collide -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn all_zones_collide() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).unwrap()).unwrap();
        let mut bad = Vec::new();
        for id in 0..300u32 {
            let Ok(file) = index.read(crate::dat::zone::file_id(id)) else { continue };
            let Ok(s) = crate::dat::zone::load_scene(&keys, &file) else { continue };
            if let Some(w) = s.warnings.iter().find(|w| w.starts_with("collision")) {
                bad.push((id, w.clone()));
            }
        }
        println!("collision failures: {bad:?}");
    }

    #[test]
    fn east_ronfaure_river_is_fishable() {
        let Some(g) = zone(101) else { return };
        // Standing in the river (fishing_spec.md 6): some facings have water 4 ahead; a dry bank point has none.
        let at = |x: f32, z: f32, y0: f32| {
            let y = g.snap(x, z, y0).unwrap_or(y0);
            let pos = Vec3::new(x, y, z);
            (0..16).filter(|k| { let a = *k as f32 * std::f32::consts::TAU / 16.0; g.water_ahead(pos, Vec3::new(a.sin(), 0.0, a.cos())) }).collect::<Vec<_>>()
        };
        let river = at(458.558, -316.667, -16.5);
        println!("river facings (k x 22.5 deg from +z): {river:?}");
        assert!(!river.is_empty());
        assert_eq!(at(27.69, 53.10, -52.28), Vec::<i32>::new());
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
