//! Mog House layout mode (spec ~/ffxi/lab/research/layout_mode_spec.md): the cell grid (FUN_101cecf0 and kin), moving,
//! turning and placing pieces, the selection arrows, the layout camera and the c2s 0x0FA / 0x0FB bodies.

use crate::dat::furniture::Furn;
use glam::Vec3;

/// Grid size in cells: 20 columns (x) by 26 rows (z).
pub const COLS: usize = 20;
pub const ROWS: usize = 26;
/// Wall records (2F has 92 slots; 1F uses 0..4).
pub const WALLS: usize = 92;

/// One cell (8 bytes): the floor piece and the piece on top of it, free bits (1 floor, 2 top, 0x80 blocked) and the
/// surface height in 0.01 units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub floor: (u8, u8),
    pub top: (u8, u8),
    pub flags: u8,
    pub height: u16,
}

/// A wall slot's record: the piece and its state (4 free, 0 taken).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub piece: (u8, u8),
    pub state: u8,
}

/// A piece in the grid's terms (object +0x10 container, +5 slot, +0x14 x or wall slot, +0x18 level, +0x1c z,
/// +0x20 rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub c: u8,
    pub slot: u8,
    pub furn: Furn,
    pub x: i8,
    pub level: i8,
    pub z: i8,
    pub rot: u8,
}

impl Piece {
    fn key(&self) -> (u8, u8) {
        (self.slot, self.c)
    }

    fn cells(&self) -> impl Iterator<Item = (i32, i32)> + use<> {
        let (w, d) = self.furn.footprint(self.rot);
        let (x, z) = (self.x as i32, self.z as i32);
        (x..x + w as i32).flat_map(move |cx| (z..z + d as i32).map(move |cz| (cx, cz)))
    }

    /// FUN_101cebe0 flag 2: things can stand on it.
    pub fn surface(&self) -> bool {
        self.furn.flags & 2 != 0
    }

    /// Flag 4: it can stand on a surface.
    pub fn stackable(&self) -> bool {
        self.furn.flags & 4 != 0
    }
}

fn inside(x: i32, z: i32) -> bool {
    (0..COLS as i32).contains(&x) && (0..ROWS as i32).contains(&z)
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub cells: Vec<Cell>,
    pub walls: [Wall; WALLS],
}

impl Grid {
    /// FUN_101cecf0: every cell free (flags 3); on 1F the walkway x 6..13, z 0..17 is blocked (0x80).
    pub fn new(floor: u8) -> Self {
        let mut cells = vec![Cell { floor: (0, 0), top: (0, 0), flags: 3, height: 0 }; COLS * ROWS];
        if floor == 0 {
            for x in 6..=13 {
                for z in 0..=17 {
                    cells[x * ROWS + z].flags = 0x80;
                }
            }
        }
        Grid { cells, walls: [Wall { piece: (0, 0), state: 4 }; WALLS] }
    }

    pub fn cell(&self, x: i32, z: i32) -> Option<&Cell> {
        inside(x, z).then(|| &self.cells[x as usize * ROWS + z as usize])
    }

    fn cell_mut(&mut self, x: i32, z: i32) -> Option<&mut Cell> {
        inside(x, z).then(|| &mut self.cells[x as usize * ROWS + z as usize])
    }

    /// FUN_101cf0a0 / FUN_101cefd0: a wall slot free; on the floor every footprint cell floor-free; on a level every
    /// cell top-free over one and the same surface piece. Cells past the grid are not checked.
    pub fn fits(&self, p: &Piece) -> bool {
        if p.furn.wall() {
            return self.walls.get(p.x as u8 as usize).is_some_and(|w| w.state == 4);
        }
        let mask = if p.level > 0 { 2 } else { 1 };
        let mut under = None;
        for (x, z) in p.cells() {
            let Some(c) = self.cell(x, z) else { continue };
            if c.flags & mask == 0 || (mask == 2 && (c.flags & 1 != 0 || under.is_some_and(|u| u != c.floor))) {
                return false;
            }
            under = Some(c.floor);
        }
        true
    }

    /// FUN_101ced80: takes the piece's cells or wall slot when it fits; returns whether it did (object +0x2f).
    pub fn occupy(&mut self, p: &Piece) -> bool {
        if !self.fits(p) {
            return false;
        }
        if p.furn.wall() {
            if let Some(w) = self.walls.get_mut(p.x as u8 as usize) {
                *w = Wall { piece: p.key(), state: 0 };
            }
            return true;
        }
        let (surface, height) = (p.surface(), p.furn.height);
        for (x, z) in p.cells() {
            let Some(c) = self.cell_mut(x, z) else { continue };
            if p.level == 0 {
                c.floor = p.key();
                if surface {
                    c.height = height;
                    c.flags &= !1;
                } else {
                    c.flags = 0;
                }
            } else {
                c.top = p.key();
                c.flags &= !2;
            }
        }
        true
    }

    /// FUN_101ceef0: gives back what the piece holds.
    pub fn free(&mut self, p: &Piece) {
        if p.furn.wall() {
            for w in self.walls.iter_mut().filter(|w| w.piece == p.key()) {
                *w = Wall { piece: (0, 0), state: 4 };
            }
            return;
        }
        for c in self.cells.iter_mut() {
            if p.level == 0 && c.floor == p.key() {
                (c.floor, c.flags, c.height) = ((0, 0), 3, 0);
            } else if p.level > 0 && c.top == p.key() {
                c.top = (0, 0);
                c.flags = if c.floor != (0, 0) { c.flags | 2 } else { 3 };
            }
        }
    }

    /// FUN_101cec50: the pieces standing on a floor piece (slot, container), each once, at most 0x20.
    pub fn on(&self, slot: u8, c: u8) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        for cell in self.cells.iter().filter(|x| x.floor == (slot, c) && x.top != (0, 0)) {
            if !out.contains(&cell.top) && out.len() < 0x20 {
                out.push(cell.top);
            }
        }
        out
    }
}

/// DAT_1037d7d4[floor]: the move bounds {x0, x1, z0, z1}.
const BOUNDS: [[i32; 4]; 2] = [[0, 20, 0, 24], [0, 20, 0, 26]];

/// FUN_101cd060's flip table and the direction offsets of FUN_101cd0a0 (0 +z, 1 +x, 2 -z, 3 -x).
const FLIP: [u8; 4] = [2, 3, 0, 1];
const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

/// The wall slot moves (PTR_DAT_10337de4) and turns (PTR_DAT_10337dec): 1F, then 2F.
const MOVE_1F: [[u8; 4]; 5] = [[0, 0, 1, 4], [0, 1, 1, 2], [3, 1, 2, 2], [4, 0, 2, 3], [4, 0, 3, 4]];
const ROT_1F: [u8; 5] = [1, 1, 3, 3, 3];
#[rustfmt::skip]
const MOVE_2F: [[u8; 4]; 92] = [
    [4,1,84,0], [5,2,85,0], [6,3,86,1], [7,51,87,2], [8,5,0,4], [9,6,1,4], [10,7,2,5], [11,55,3,6], [12,9,4,8], [13,10,5,8],
    [14,11,6,9], [15,59,7,10], [16,13,8,12], [17,14,9,12], [18,15,10,13], [19,63,11,14], [20,17,12,16], [21,18,13,16],
    [22,19,14,17], [23,67,15,18], [24,21,16,20], [25,22,17,20], [26,23,18,21], [27,71,19,22], [28,25,20,24], [29,26,21,24],
    [30,27,22,25], [31,75,23,26], [32,29,24,28], [33,30,25,28], [34,31,26,29], [35,79,27,30], [36,33,28,32], [37,34,29,32],
    [38,35,30,33], [39,83,31,34], [36,40,37,32], [36,41,38,33], [37,42,39,34], [38,43,87,35], [40,44,41,36], [40,45,42,37],
    [41,46,43,38], [42,47,43,39], [44,80,45,40], [44,81,46,41], [45,82,47,42], [46,83,91,43], [52,48,88,49], [53,48,89,50],
    [54,49,90,51], [55,50,91,3], [56,52,48,53], [57,52,49,54], [58,53,50,55], [59,54,51,7], [60,56,52,57], [61,56,53,58],
    [62,57,54,59], [63,58,55,11], [64,60,56,61], [65,60,57,62], [66,61,58,63], [67,62,59,15], [68,64,60,65], [69,64,61,66],
    [70,65,62,67], [71,66,63,19], [72,68,64,69], [73,68,65,70], [74,69,66,71], [75,70,67,23], [76,72,68,73], [77,72,69,74],
    [78,73,70,75], [79,74,71,27], [80,76,72,77], [81,76,73,78], [82,77,74,79], [83,78,75,31], [44,80,76,81], [45,80,77,82],
    [46,81,78,83], [47,82,79,35], [85,88,84,0], [86,89,84,1], [87,90,85,2], [39,91,86,3], [89,48,88,84], [90,49,88,85],
    [91,50,89,86], [47,51,90,87],
];

/// A wall slot's turn on a floor.
pub fn wall_rot(floor: u8, slot: u8) -> u8 {
    if floor == 0 {
        ROT_1F.get(slot as usize).copied().unwrap_or(0)
    } else {
        match slot {
            0..=35 => 3,
            36..=47 => 0,
            48..=83 => 1,
            _ => 2,
        }
    }
}

/// FUN_101cd060: with /layoutctrl type 1 (config 0xc1 != 1) the arrows flip while the camera looks toward -z.
pub fn remap(dir: u8, ctrl: u32, eye: Vec3, look: Vec3) -> u8 {
    if ctrl != 1 && look.z <= eye.z { FLIP[dir as usize & 3] } else { dir & 3 }
}

/// The level a stackable piece takes over the cells under it (FUN_101cec20: the last raised cell, height / 10).
fn level_over(grid: &Grid, p: &Piece) -> i8 {
    let mut level = 0;
    for (x, z) in p.cells() {
        if let Some(c) = grid.cell(x, z).filter(|c| c.height > 0) {
            level = (c.height / 10) as i8;
        }
    }
    level
}

/// FUN_101cd150: moves a floor piece by (dx, dz) inside the floor's bounds; false when nothing changed. The pieces
/// on a surface are moved by the caller with the same delta (FUN_101cd290).
pub fn shift(grid: &Grid, p: &mut Piece, floor: u8, dx: i32, dz: i32) -> bool {
    let [x0, x1, z0, z1] = BOUNDS[(floor != 0) as usize];
    let (w, d) = p.furn.footprint(p.rot);
    let (mut x, mut z) = ((p.x as i32 + dx).max(x0), (p.z as i32 + dz).max(z0));
    if x + w as i32 >= x1 {
        x = x1 - w as i32;
    }
    if z + d as i32 >= z1 {
        z = z1 - d as i32;
    }
    if (x, z) == (p.x as i32, p.z as i32) {
        return false;
    }
    (p.x, p.z) = (x as i8, z as i8);
    p.level = if p.stackable() { level_over(grid, p) } else { 0 };
    true
}

/// FUN_101cd0a0: one arrow on a held piece: a floor piece by a cell (clamped), a wall piece to the next slot of the
/// table (always "moved"). Returns the delta for the pieces on a surface, or None when nothing moved.
pub fn step(grid: &Grid, p: &mut Piece, floor: u8, dir: u8) -> Option<(i32, i32)> {
    if p.furn.wall() {
        let s = p.x as u8 as usize;
        let next = if floor == 0 { MOVE_1F.get(s).map(|m| m[dir as usize & 3]) } else { MOVE_2F.get(s).map(|m| m[dir as usize & 3]) };
        let s = next.unwrap_or(s as u8);
        (p.x, p.level, p.z, p.rot) = (s as i8, 0, 0, wall_rot(floor, s));
        return Some((0, 0));
    }
    let (ox, oz) = (p.x, p.z);
    let (dx, dz) = DIRS[dir as usize & 3];
    shift(grid, p, floor, dx, dz).then(|| ((p.x - ox) as i32, (p.z - oz) as i32))
}

/// Num- (0x8d, -1) / 0x8e (+1): a floor piece turns about its centre (integer halves) and is clamped; a wall piece's
/// rotation changes but nothing else (retail quirk). Returns the shift for the pieces on it.
pub fn rotate(grid: &Grid, p: &mut Piece, floor: u8, by: i8) -> (i32, i32) {
    let (w0, d0) = p.furn.footprint(p.rot);
    p.rot = (p.rot as i8 + by).rem_euclid(4) as u8;
    if p.furn.wall() {
        return (0, 0);
    }
    let (w1, d1) = p.furn.footprint(p.rot);
    let (ox, oz) = (p.x, p.z);
    shift(grid, p, floor, w0 as i32 / 2 - w1 as i32 / 2, d0 as i32 / 2 - d1 as i32 / 2);
    if !p.stackable() {
        p.level = 0;
    }
    ((p.x - ox) as i32, (p.z - oz) as i32)
}

/// FUN_101cd3b0: a new piece's start: wall slot 0 (its turn); else (9, 12), or the selected floor piece's place,
/// centred on that cell.
pub fn start(grid: &Grid, p: &mut Piece, floor: u8, sel: Option<&Piece>) {
    if p.furn.wall() {
        (p.x, p.level, p.z, p.rot) = (0, 0, 0, wall_rot(floor, 0));
        return;
    }
    (p.x, p.level, p.z, p.rot) = (9, 0, 12, 0);
    if let Some(s) = sel.filter(|s| !s.furn.wall()) {
        (p.x, p.level, p.z) = (s.x, s.level, s.z);
    }
    let (w, d) = p.furn.footprint(0);
    shift(grid, p, floor, -(w as i32 / 2), -(d as i32 / 2));
    if p.stackable() {
        p.level = level_over(grid, p);
    }
}

/// A selection candidate for the arrows: its id, world position, level, and for the current one its wall flag,
/// turn and footprint, and whether pieces stand on it.
#[derive(Clone, Copy, Debug)]
pub struct Cand {
    pub id: u32,
    pub pos: Vec3,
    pub level: i8,
    pub wall: bool,
    pub rot: u8,
    pub size: (u8, u8),
    pub has_tops: bool,
}

/// FUN_101d5710: the next piece in an arrow's direction (dx -1 / +1, dz -1 / +1; view 2 negates both).
pub fn pick(cur: &Cand, others: &[Cand], dx: i32, dz: i32, view: u8) -> Option<u32> {
    if dx == 0 && dz == 0 {
        return None;
    }
    let (dx, dz) = if view == 2 { (-dx, -dz) } else { (dx, dz) };
    let mut p = Vec3::new(cur.pos.x, 0.0, cur.pos.z);
    if cur.wall {
        let d = -cur.pos.y;
        match cur.rot & 3 {
            0 => p.z += d,
            1 => p.x += d,
            2 => p.z -= d,
            _ => p.x -= d,
        }
    }
    let (ex, ez) = if cur.has_tops { (0.0, 0.0) } else { (cur.size.0 as f32 * 0.3, cur.size.1 as f32 * 0.3) };
    let mut best: Option<(f32, u32)> = None;
    let mut floor_best: Option<(f32, u32)> = None;
    for c in others.iter().filter(|c| c.id != cur.id) {
        let (rx, rz) = (c.pos.x - p.x, c.pos.z - p.z);
        if cur.level == 0 && rx.abs() + rz.abs() < 0.1 {
            return Some(c.id);
        }
        let ok_x = match dx {
            1 => rx > ex && rx.abs() >= rz.abs(),
            -1 => rx < -ex && rx.abs() >= rz.abs(),
            _ => true,
        };
        let ok_z = match dz {
            1 => rz > ez && rz.abs() >= rx.abs(),
            -1 => rz < -ez && rz.abs() >= rx.abs(),
            _ => true,
        };
        if !(ok_x && ok_z) {
            continue;
        }
        let d2 = rx * rx + rz * rz;
        if best.is_none_or(|b| d2 < b.0) {
            best = Some((d2, c.id));
        }
        if c.level == 0 && floor_best.is_none_or(|b| d2 < b.0) {
            floor_best = Some((d2, c.id));
        }
    }
    match (best, floor_best) {
        (Some(b), Some(f)) if (f.0 - b.0).abs() < 0.1 => Some(f.1),
        (b, _) => b.map(|b| b.1),
    }
}

/// FUN_101d4080's target eye and look for a view (0 top-down, 1 from the door side, 2 from the far side) and the
/// selected piece's position.
pub fn camera_target(view: u8, floor: u8, sel: Option<Vec3>) -> (Vec3, Vec3) {
    let b = if floor == 0 { [4.0, 7.0, 5.0] } else { [4.0, 9.0, 6.0] };
    let mut l = sel.unwrap_or(Vec3::ZERO);
    l.y = match sel {
        Some(s) if view != 0 => s.y - 0.3,
        _ => -0.3,
    };
    l.x = l.x.clamp(-b[0], b[0]);
    l.z = l.z.clamp(-b[2], b[2]);
    let e = match view {
        0 => Vec3::new(l.x, -5.0, l.z - 0.1),
        1 => Vec3::new(l.x * 0.7, -4.5, (l.z - 4.0).max(-5.2)),
        _ => Vec3::new(l.x * 0.7, -4.5, (l.z + 4.0).min(5.2)),
    };
    (e, l)
}

/// One frame of the layout camera: both points 0.3 of the way; view 0 then keeps the eye 7 (2F 9) from the look.
pub fn camera_step(eye: &mut Vec3, look: &mut Vec3, view: u8, floor: u8, sel: Option<Vec3>) {
    let (e, l) = camera_target(view, floor, sel);
    *look += (l - *look) * 0.3;
    *eye += (e - *eye) * 0.3;
    if view == 0 {
        let dist = if floor == 0 { 7.0 } else { 9.0 };
        *eye = *look + (*eye - *look).normalize_or(Vec3::NEG_Y) * dist;
    }
}

/// c2s 0x0FA's body after the header (FUN_100f0cc0): item, slot, container, floor, x, level, z, rotation, 3 zero.
pub fn layout_body(item: u16, slot: u8, c: u8, floor: u8, x: i8, level: i8, z: i8, rot: u8) -> [u8; 12] {
    let i = item.to_le_bytes();
    [i[0], i[1], slot, c, floor, x as u8, level as u8, z as u8, rot, 0, 0, 0]
}

/// c2s 0x0FB's body (FUN_100f0d40): item, slot, container.
pub fn bankin_body(item: u16, slot: u8, c: u8) -> [u8; 4] {
    let i = item.to_le_bytes();
    [i[0], i[1], slot, c]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(flags: u16, size: [u8; 2], height: u16, slot: u8, x: i8, z: i8) -> Piece {
        Piece { c: 1, slot, furn: Furn { flags, model: 1, size, height }, x, level: 0, z, rot: 0 }
    }

    /// Spec 7.8: the 1F walkway, a desk's surface, a pot on it and across two tables, freeing, a taken wall slot.
    #[test]
    fn grid() {
        let g = Grid::new(0);
        assert_eq!(g.cells.iter().filter(|c| c.flags == 0x80).count(), 144);
        assert_eq!(Grid::new(1).cells.iter().filter(|c| c.flags == 0x80).count(), 0);
        let mut g = Grid::new(1);
        let desk = piece(2, [3, 3], 50, 1, 0, 0);
        assert!(g.occupy(&desk));
        assert_eq!((g.cell(1, 1).unwrap().flags, g.cell(1, 1).unwrap().height), (2, 50));
        let mut pot = piece(0xc, [1, 1], 100, 2, 1, 1);
        pot.level = 5;
        assert!(g.fits(&pot));
        pot.x = 3;
        assert!(!g.fits(&pot));
        let desk2 = piece(2, [3, 3], 50, 3, 3, 0);
        assert!(g.occupy(&desk2));
        let mut wide = piece(0xc, [2, 1], 100, 4, 2, 1);
        wide.level = 5;
        assert!(!g.fits(&wide));
        pot.x = 1;
        assert!(g.occupy(&pot));
        assert_eq!(g.on(1, 1), vec![(2, 1)]);
        g.free(&pot);
        assert_eq!(g.cell(1, 1).unwrap().flags, 2);
        g.free(&desk);
        assert_eq!(g.cell(1, 1).unwrap().flags, 3);
        let flag = piece(1, [1, 1], 60, 5, 3, 0);
        assert!(g.occupy(&flag));
        assert!(!g.fits(&piece(1, [1, 1], 60, 6, 3, 0)));
    }

    /// Spec 7.8: a bed pushed into the right wall, a stackable pot over the desk and off it.
    #[test]
    fn moves() {
        let g = Grid::new(0);
        let mut bed = piece(0, [4, 6], 70, 1, 15, 18);
        assert_eq!(step(&g, &mut bed, 0, 1), Some((1, 0)));
        assert_eq!(bed.x, 16);
        assert_eq!(step(&g, &mut bed, 0, 1), None);
        let mut g = Grid::new(1);
        let mut far = piece(0, [1, 3], 70, 2, 0, 22);
        assert_eq!(step(&g, &mut far, 1, 0), Some((0, 1)));
        assert_eq!(step(&g, &mut far, 1, 0), None);
        assert_eq!(far.z, 23);
        assert!(g.occupy(&piece(2, [3, 3], 50, 3, 0, 0)));
        let mut pot = piece(0xc, [1, 1], 100, 4, 3, 1);
        assert!(step(&g, &mut pot, 1, 3).is_some());
        assert_eq!((pot.x, pot.level), (2, 5));
        assert!(step(&g, &mut pot, 1, 1).is_some());
        assert_eq!((pot.x, pot.level), (3, 0));
    }

    /// Spec 7.8: a bed turned at (2, 2); wall slots on 1F and 2F.
    #[test]
    fn rotate_and_walls() {
        let g = Grid::new(1);
        let mut bed = piece(0, [4, 6], 70, 1, 2, 2);
        rotate(&g, &mut bed, 1, -1);
        assert_eq!((bed.rot, bed.x, bed.z), (3, 1, 3));
        let mut flag = piece(1, [1, 1], 60, 2, 3, 0);
        assert!(step(&g, &mut flag, 0, 1).is_some());
        assert_eq!((flag.x, flag.rot), (0, 1));
        assert!(step(&g, &mut flag, 0, 0).is_some());
        assert_eq!(flag.x, 0);
        flag.x = 39;
        step(&g, &mut flag, 1, 2);
        assert_eq!((flag.x, flag.rot), (87, 2));
    }

    #[test]
    fn remap_dirs() {
        let (eye, look) = (Vec3::new(0.0, -4.5, 4.0), Vec3::ZERO);
        assert_eq!((0..4).map(|d| remap(d, 0, eye, look)).collect::<Vec<_>>(), [2, 3, 0, 1]);
        assert_eq!((0..4).map(|d| remap(d, 1, eye, look)).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(remap(0, 0, Vec3::new(0.0, -4.5, -4.0), look), 0);
    }

    /// Spec 7.8: pieces at (0, 0) current, (2, 0.5), (0.5, 2), (-3, 0); a pot at the table's centre; view 2 swaps.
    #[test]
    fn picks() {
        let c = |id, x, z, level| Cand { id, pos: Vec3::new(x, 0.0, z), level, wall: false, rot: 0, size: (1, 1), has_tops: false };
        let cur = c(1, 0.0, 0.0, 0);
        let others = [c(2, 2.0, 0.5, 0), c(3, 0.5, 2.0, 0), c(4, -3.0, 0.0, 0)];
        assert_eq!(pick(&cur, &others, 1, 0, 0), Some(2));
        assert_eq!(pick(&cur, &others, 0, 1, 0), Some(3));
        assert_eq!(pick(&cur, &others, -1, 0, 0), Some(4));
        assert_eq!(pick(&cur, &others, 1, 0, 2), Some(4));
        let table = Cand { size: (3, 3), ..c(5, 0.0, 0.0, 0) };
        assert_eq!(pick(&table, &[c(6, 0.0, 0.0, 5), c(2, 2.0, 0.5, 0)], 1, 0, 0), Some(6));
    }

    #[test]
    fn camera() {
        let sel = Some(Vec3::new(5.0, -0.5, 6.0));
        let (e, l) = camera_target(0, 0, sel);
        assert_eq!((l, e), (Vec3::new(4.0, -0.3, 5.0), Vec3::new(4.0, -5.0, 4.9)));
        let (e, l) = camera_target(1, 0, sel);
        assert_eq!((l, e), (Vec3::new(4.0, -0.8, 5.0), Vec3::new(2.8, -4.5, 1.0)));
        let (e, _) = camera_target(2, 0, sel);
        assert_eq!(e, Vec3::new(2.8, -4.5, 5.2));
    }

    #[test]
    fn packets() {
        assert_eq!(layout_body(0x1234, 5, 1, 0, 7, 5, 9, 3), [0x34, 0x12, 5, 1, 0, 7, 5, 9, 3, 0, 0, 0]);
        assert_eq!(bankin_body(0x1234, 5, 9), [0x34, 0x12, 5, 9]);
    }
}
