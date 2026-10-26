//! Mog House furniture (spec ~/ffxi/lab/research/furniture_spec.md): the installed pieces of Mog Safe and Mog Safe 2
//! (the scan FUN_101d2af0) drawn as static models at their grid places (FUN_101cdd90 / FUN_101cdf50), only in a
//! furnished room (FUN_100f9650), a flowerpot's plant above it (FUN_101cda60).

use super::World;
use crate::dat::DatIndex;
use crate::dat::furniture::{Furn, FurnTable, model_file};
use crate::dat::zone::{Keys, Placement, Texture};
use crate::render::ZoneGeometry;
use anyhow::Result;
use glam::Vec3;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A piece as the scan reads it: container, slot, item id, exdata 0..10.
pub type Key = (u8, u8, u16, [u8; 10]);

#[derive(Default)]
pub struct State {
    /// The pieces the last load was asked for, and its number (a result for an older one is dropped).
    want: Vec<Key>,
    seq: u64,
    /// The number of the set on screen.
    applied: u64,
    /// A loaded set waiting for the renderer.
    pub ready: Option<Loaded>,
    /// The drawn pieces for the effects (FUN_10085060's model generators, 'init'): id, model file, matrix.
    pub pieces: Vec<(u32, u32, glam::Mat4)>,
    /// Skinned pieces (CXiSklFurnitureActor): actor id, position, yaw.
    pub skinned: Vec<(u32, Vec3, f32)>,
    /// The set on screen by record (as `pieces` and `skinned`), and its room.
    drawn: Vec<Drawn>,
    drawn_skinned: Vec<((u8, u8), Furn)>,
    room: (u16, u8),
    /// Pieces moved in place for the renderer: file index, the matrix drawn, the new one.
    pub moves: Vec<(usize, glam::Mat4, glam::Mat4)>,
}

/// A drawn piece (or a pot's plant): its record, its file (DAT id, index in the loaded set), how it is drawn.
#[derive(Clone, Debug)]
pub struct Drawn {
    pub key: (u8, u8),
    pub furn: Furn,
    pub plant: bool,
    pub file_id: u32,
    pub file: usize,
    pub matrix: glam::Mat4,
    pub init: bool,
    /// The box size (none for a plant) and the mesh triangles in model space.
    pub size: Option<Vec3>,
    pub local: Arc<Vec<[Vec3; 3]>>,
}

impl State {
    /// A new scene: its renderer has no furniture; the set loads again.
    pub fn reset(&mut self) {
        self.want.clear();
        self.ready = None;
        self.pieces.clear();
        self.skinned.clear();
        self.drawn.clear();
        self.drawn_skinned.clear();
        self.moves.clear();
        self.seq += 1;
    }

    /// A skinned piece's place.
    pub fn place_of(&self, id: u32) -> Option<(Vec3, f32)> {
        self.skinned.iter().find(|s| s.0 == id).map(|s| (s.1, s.2))
    }

    /// A loaded set goes up: its pieces get ids (XiFurnitureActor's entity: 0x04000000 | an index down from 0x3ff)
    /// and their 'init' schedulers start (FUN_100c47f0); returns the geometry.
    pub fn apply(w: &mut World, l: Loaded) -> Vec<(Arc<ZoneGeometry>, Vec<Texture>)> {
        for (id, _, _) in std::mem::take(&mut w.furniture.pieces) {
            w.effects.stop_actor_scheduler(id, u32::from_le_bytes(*b"init"));
        }
        for (k, d) in l.pieces.iter().enumerate() {
            let id = 0x0400_0000 | (0x3ff - k as u32);
            w.furniture.pieces.push((id, d.file_id, d.matrix));
            if d.init {
                w.effects.on_scheduler_quiet(id, u32::from_le_bytes(*b"init"));
            }
        }
        // FUN_100ab2f0 kind 2: a skinned model of the same file formula, an actor at the piece's place.
        for (id, _, _) in std::mem::take(&mut w.furniture.skinned) {
            w.actors.remove(&id);
        }
        w.furniture.drawn_skinned.clear();
        for (k, &(key, furn, file, pos, yaw)) in l.skinned.iter().enumerate() {
            let id = 0x0400_0000 | (0x3ff - (l.pieces.len() + k) as u32);
            w.furniture.skinned.push((id, pos, yaw));
            w.furniture.drawn_skinned.push((key, furn));
            w.event_looks.push((id, super::Source::File(file)));
        }
        (w.furniture.drawn, w.furniture.room, w.furniture.applied) = (l.pieces, l.room, l.seq);
        w.furniture.moves.clear();
        set_ground(w);
        l.files
    }

    pub fn current(&self, seq: u64) -> bool {
        seq == self.seq
    }
}

pub struct Loaded {
    pub seq: u64,
    pub files: Vec<(Arc<ZoneGeometry>, Vec<Texture>)>,
    /// Every piece (plants too), in file order.
    pub pieces: Vec<Drawn>,
    /// Skinned pieces: record, entry, model file, position, yaw.
    pub skinned: Vec<((u8, u8), Furn, u32, Vec3, f32)>,
    /// Map and floor.
    pub room: (u16, u8),
}

/// The pieces' boxes (matrix and size (x, height, z) = (size x x 0.6, height x 0.01, size z x 0.6)) and mesh
/// triangles in world space (the camera ray) to the ground.
fn set_ground(w: &mut World) {
    let d = &w.furniture.drawn;
    let boxes = d.iter().filter_map(|p| Some((p.matrix, p.size?))).collect();
    let meshes = d.iter().map(|p| p.local.iter().map(|t| t.map(|v| p.matrix.transform_point3(v))).collect()).collect();
    if let Some(g) = w.ground.as_mut() {
        g.set_furniture(boxes, meshes);
    }
}

/// The matrix of a piece placed at `pos` turned `yaw` (as an MZB placement).
fn matrix(pos: Vec3, yaw: f32) -> glam::Mat4 {
    let p = Placement { name: String::new(), translate: pos.to_array(), rotate: [0.0, yaw, 0.0], scale: [1.0; 3], lights: [0; 4], link: 0, area: 0, id: 0, lod: [0.0; 2], world: false };
    glam::Mat4::from_cols_array_2d(&p.matrix())
}

/// Only places changed (exdata 6..9 of the same pieces) and the set on screen is the last asked for: the pieces move
/// in place (FUN_101cdd90 per piece) instead of loading again.
fn move_in_place(w: &mut World, want: &[Key]) -> bool {
    let f = &w.furniture;
    let same = |a: &Key, b: &Key| (a.0, a.1, a.2) == (b.0, b.1, b.2) && a.3[..6] == b.3[..6];
    if f.applied != f.seq || f.ready.is_some() || want.len() != f.want.len() || !want.iter().zip(&f.want).all(|(a, b)| same(a, b)) {
        return false;
    }
    let (map, floor) = f.room;
    let place_of = |key: (u8, u8), furn: &Furn| {
        let e = want.iter().find(|k| (k.0, k.1) == key)?.3;
        place(furn, map, floor, e[6] as i8, e[7] as i8, e[8] as i8, e[9])
    };
    let mut moves = Vec::new();
    for (k, d) in w.furniture.drawn.iter_mut().enumerate() {
        let Some((pos, yaw)) = place_of(d.key, &d.furn) else { continue };
        let m = matrix(if d.plant { pos - Vec3::new(0.0, 0.3, 0.0) } else { pos }, yaw);
        if m != d.matrix {
            moves.push((d.file, d.matrix, m));
            d.matrix = m;
            w.furniture.pieces[k].2 = m;
        }
    }
    for (k, (key, furn)) in w.furniture.drawn_skinned.iter().enumerate() {
        if let Some((pos, yaw)) = place_of(*key, furn) {
            (w.furniture.skinned[k].1, w.furniture.skinned[k].2) = (pos, yaw);
        }
    }
    if !moves.is_empty() {
        w.furniture.moves.extend(moves);
        set_ground(w);
    }
    w.furniture.want = want.to_vec();
    true
}

/// FUN_101d2b10's item types: furnishing 10, flowerpot 12 (mannequins, 14, are a race body: not drawn yet).
const TYPES: [u16; 2] = [10, 12];

/// DAT_1037d7b4[floor]: the grid origin (column, level, row).
pub const ORIGIN: [[f32; 3]; 2] = [[10.0, 0.0, 12.0], [10.0, 0.0, 13.0]];

/// FUN_101cce90(floor): the wall slots (0x1037d9c8; San d'Oria's house 0x1037da18; 2F 0x1037da68).
const WALL_1F: [[f32; 3]; 5] = [[6.5, -3.0, 4.55], [6.5, -3.0, -4.8], [-6.5, -3.0, -4.8], [-6.5, -3.0, 0.0], [-6.5, -3.0, 4.55]];
const WALL_1F_SANDORIA: [[f32; 3]; 5] = [[6.2325, -3.0, 4.55], [6.2325, -3.0, -4.8], [-6.2325, -3.0, -4.8], [-6.2325, -3.0, 0.0], [-6.2325, -3.0, 4.55]];
#[rustfmt::skip]
const WALL_2F: [[f32; 3]; 92] = [
    [-6.2, -5.6, -6.2], [-6.2, -3.2, -6.2], [-6.2, -2.5, -6.2], [-6.2, -1.8, -6.2], [-6.2, -5.6, -5.15],
    [-6.2, -3.2, -5.15], [-6.2, -2.5, -5.15], [-6.2, -1.8, -5.15], [-6.2, -5.6, -4.1], [-6.2, -3.2, -4.1],
    [-6.2, -2.5, -4.1], [-6.2, -1.8, -4.1], [-6.2, -5.6, -1.0], [-6.2, -3.2, -1.0], [-6.2, -2.5, -1.0],
    [-6.2, -1.8, -1.0], [-6.2, -5.6, 0.0], [-6.2, -3.2, 0.0], [-6.2, -2.5, 0.0], [-6.2, -1.8, 0.0],
    [-6.2, -5.6, 1.0], [-6.2, -3.2, 1.0], [-6.2, -2.5, 1.0], [-6.2, -1.8, 1.0], [-6.2, -5.6, 4.1], [-6.2, -3.2, 4.1],
    [-6.2, -2.5, 4.1], [-6.2, -1.8, 4.1], [-6.2, -5.6, 5.15], [-6.2, -3.2, 5.15], [-6.2, -2.5, 5.15],
    [-6.2, -1.8, 5.15], [-6.2, -5.6, 6.2], [-6.2, -3.2, 6.2], [-6.2, -2.5, 6.2], [-6.2, -1.8, 6.2],
    [-4.0, -5.6, 8.0], [-4.0, -3.2, 8.0], [-4.0, -2.5, 8.0], [-4.0, -1.8, 8.0], [0.0, -5.6, 8.0], [0.0, -3.2, 8.0],
    [0.0, -2.5, 8.0], [0.0, -1.8, 8.0], [4.0, -5.6, 8.0], [4.0, -3.2, 8.0], [4.0, -2.5, 8.0], [4.0, -1.8, 8.0],
    [6.2, -5.6, -6.2], [6.2, -3.2, -6.2], [6.2, -2.5, -6.2], [6.2, -1.8, -6.2], [6.2, -5.6, -5.15],
    [6.2, -3.2, -5.15], [6.2, -2.5, -5.15], [6.2, -1.8, -5.15], [6.2, -5.6, -4.1], [6.2, -3.2, -4.1],
    [6.2, -2.5, -4.1], [6.2, -1.8, -4.1], [6.2, -5.6, -1.0], [6.2, -3.2, -1.0], [6.2, -2.5, -1.0], [6.2, -1.8, -1.0],
    [6.2, -5.6, 0.0], [6.2, -3.2, 0.0], [6.2, -2.5, 0.0], [6.2, -1.8, 0.0], [6.2, -5.6, 1.0], [6.2, -3.2, 1.0],
    [6.2, -2.5, 1.0], [6.2, -1.8, 1.0], [6.2, -5.6, 4.1], [6.2, -3.2, 4.1], [6.2, -2.5, 4.1], [6.2, -1.8, 4.1],
    [6.2, -5.6, 5.15], [6.2, -3.2, 5.15], [6.2, -2.5, 5.15], [6.2, -1.8, 5.15], [6.2, -5.6, 6.2], [6.2, -3.2, 6.2],
    [6.2, -2.5, 6.2], [6.2, -1.8, 6.2], [-4.0, -5.6, -8.0], [-4.0, -3.2, -8.0], [-4.0, -2.5, -8.0],
    [-4.0, -1.8, -8.0], [4.0, -5.6, -8.0], [4.0, -3.2, -8.0], [4.0, -2.5, -8.0], [4.0, -1.8, -8.0],
];

/// FUN_101cdd90 / FUN_101cdf50: a piece's position (the centre of its turned footprint, 0.6-unit cells, its level in
/// 0.1 units up) or its wall slot's (none past the table), and its yaw (rot x pi / 2).
pub fn place(f: &Furn, map: u16, floor: u8, col: i8, level: i8, row: i8, rot: u8) -> Option<(Vec3, f32)> {
    let yaw = rot as f32 * 1.570_796;
    if f.wall() {
        return wall_pos(map, floor, col as u8).map(|p| (p, yaw));
    }
    let o = ORIGIN[(floor != 0) as usize];
    let (w, d) = f.footprint(rot);
    Some((Vec3::new((col as f32 - o[0]) * 0.6 + w as f32 * 0.3, -(level as f32 - o[1]) * 0.1, (row as f32 - o[2]) * 0.6 + d as f32 * 0.3), yaw))
}

/// A wall slot's place (FUN_101cce90).
pub fn wall_pos(map: u16, floor: u8, slot: u8) -> Option<Vec3> {
    let t: &[[f32; 3]] = if floor != 0 {
        &WALL_2F
    } else if map == 0x121 {
        &WALL_1F_SANDORIA
    } else {
        &WALL_1F
    };
    t.get(slot as usize).map(|&p| Vec3::from(p))
}

/// FUN_101d2af0 / FUN_101d2b10 (every frame): the installed pieces (exdata[1] bit 6) of this floor (exdata[1] & 3) in
/// containers 1 and 9, slots 1..0x50; none outside a furnished room (FUN_101d5fe0 hides them) or in the Mog Garden
/// (zone 280). A changed set gives the load to start: (its number, the pieces, map, floor).
pub fn changed(w: &mut World) -> Option<(u64, Vec<Key>, u16, u8)> {
    // /furcc (config 0x99 bit 6 clear): the camera stops at furniture.
    if let Some(g) = w.ground.as_mut() {
        g.furcc = w.game.get(0x99) & 0x40 == 0;
    }
    let r = w.myroom.filter(|_| w.in_scene() && w.zone != Some(280))?;
    let want: Vec<Key> = match r {
        r if super::myroom::has_floors(&r) => {
            let ty = |id: u16| w.item_info.as_ref().and_then(|i| i.item(id)).map_or(0, |it| it.item_type);
            w.inv
                .slots
                .iter()
                .filter(|((c, s), _)| matches!(c, 1 | 9) && (1..0x51).contains(s))
                .filter_map(|(&(c, s), &(id, _))| {
                    let ext = w.inv.ext.get(&(c, s))?;
                    (TYPES.contains(&ty(id)) && ext[1] & 0x40 != 0 && ext[1] & 3 == r.floor).then(|| (c, s, id, ext[..10].try_into().unwrap()))
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let mut want = want;
    super::layout::overrides(w, &mut want);
    if want == w.furniture.want || move_in_place(w, &want) {
        return None;
    }
    w.furniture.want = want.clone();
    w.furniture.seq += 1;
    Some((w.furniture.seq, want, r.map, r.floor))
}

/// A model's triangles (a strip's in order, degenerate ones dropped).
fn model_tris(m: &crate::dat::zone::Model) -> Vec<[Vec3; 3]> {
    let v = |i: u16| m.vertices.get(i as usize).map(|v| Vec3::from(v.pos));
    let idx: Vec<[u16; 3]> = if m.strip { m.indices.windows(3).map(|w| [w[0], w[1], w[2]]).collect() } else { m.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect() };
    idx.into_iter().filter(|[a, b, c]| a != b && b != c && a != c).filter_map(|[a, b, c]| Some([v(a)?, v(b)?, v(c)?])).collect()
}

/// The loader thread: each piece's model file (the 'furn' entry, FUN_101cce00) and place, a pot's plant model (kind
/// exdata[4], stage exdata[0]) 0.3 above it; then per file its first MMB (FUN_100c4410) with a placement per piece.
/// Skinned pieces (models 0x108..0x12b: statues, campfires) become actors.
pub fn load(index: &DatIndex, keys: &Keys, seq: u64, want: &[Key], map: u16, floor: u8) -> Result<Loaded> {
    let table = FurnTable::load(index)?;
    // Per file: each piece's record, entry, place, and whether it is a plant.
    let mut by_file: BTreeMap<u32, Vec<((u8, u8), Furn, Vec3, f32, bool)>> = BTreeMap::new();
    let mut skinned = Vec::new();
    for &(c, slot, item, ext) in want {
        let Some(f) = table.entry(item) else { continue };
        let Some(file) = model_file(f.model) else { continue };
        let Some((pos, yaw)) = place(&f, map, floor, ext[6] as i8, ext[7] as i8, ext[8] as i8, ext[9]) else { continue };
        if (0x108..=0x12b).contains(&f.model) {
            skinned.push(((c, slot), f, file, pos, yaw));
            continue;
        }
        log::debug!("furniture {item:#x}: file {file}, {pos:?} yaw {yaw:.2}");
        by_file.entry(file).or_default().push(((c, slot), f, pos, yaw, false));
        if f.pot()
            && let Some(pf) = table.plant_model(ext[4], ext[0]).and_then(model_file)
        {
            by_file.entry(pf).or_default().push(((c, slot), f, pos - Vec3::new(0.0, 0.3, 0.0), yaw, true));
        }
    }
    let mut files = Vec::new();
    let mut pieces = Vec::new();
    for (file, places) in by_file {
        let bytes = index.read(file);
        let init = bytes.as_ref().is_ok_and(|b| crate::dat::chunks(b).is_ok_and(|c| c.iter().any(|c| c.kind == 0x07 && c.name() == "init")));
        let scene = bytes.and_then(|b| crate::dat::zone::load_scene(keys, &b));
        let mut scene = match scene {
            Ok(s) => s,
            Err(e) => {
                log::warn!("furniture file {file}: {e:#}");
                continue;
            }
        };
        scene.meshes.truncate(1);
        let Some(name) = scene.meshes.first().map(|m| m.name.clone()) else { continue };
        scene.placements = places
            .iter()
            .map(|&(_, _, pos, yaw, _)| Placement { name: name.clone(), translate: pos.to_array(), rotate: [0.0, yaw, 0.0], scale: [1.0; 3], lights: [0; 4], link: 0, area: 0, id: 0, lod: [0.0; 2], world: false })
            .collect();
        let local = Arc::new(scene.meshes[0].models.iter().flat_map(model_tris).collect::<Vec<_>>());
        for (p, &(key, furn, _, _, plant)) in scene.placements.iter().zip(&places) {
            let size = (!plant).then(|| Vec3::new(furn.size[0] as f32 * 0.6, furn.height as f32 * 0.01, furn.size[1] as f32 * 0.6));
            let matrix = glam::Mat4::from_cols_array_2d(&p.matrix());
            pieces.push(Drawn { key, furn, plant, file_id: file, file: files.len(), matrix, init, size, local: local.clone() });
        }
        let geo = ZoneGeometry::build(&scene);
        files.push((Arc::new(geo), std::mem::take(&mut scene.textures)));
    }
    log::info!("furniture: {} pieces in {} files", want.len(), files.len());
    Ok(Loaded { seq, files, pieces, skinned, room: (map, floor) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// furniture_spec.md 6.6: a Simple Bed (4 x 6) at column 0 row 0 turned 0 / 1, a pot on a level-5 surface, a 1 x 1
    /// piece on 2F, wall slot 3 (and San d'Oria's), 2F slot 40.
    #[test]
    fn places() {
        let bed = Furn { flags: 0, model: 2, size: [4, 6], height: 70 };
        let near = |(p, y): (Vec3, f32), q: [f32; 3], yaw: f32| (p - Vec3::from(q)).length() < 1e-4 && (y - yaw).abs() < 1e-4;
        let at = |f: &Furn, map, floor, col, level, row, rot| place(f, map, floor, col, level, row, rot).unwrap();
        assert!(near(at(&bed, 0x122, 0, 0, 0, 0, 0), [-4.8, 0.0, -5.4], 0.0));
        assert!(near(at(&bed, 0x122, 0, 0, 0, 0, 1), [-4.2, 0.0, -6.0], 1.570_796));
        let pot = Furn { flags: 0xc, model: 216, size: [1, 1], height: 100 };
        assert!(near(at(&pot, 0x122, 0, 17, 5, 5, 0), [4.5, -0.5, -3.9], 0.0));
        assert!(near(at(&pot, 0x267, 1, 10, 0, 13, 0), [0.3, 0.0, 0.3], 0.0));
        let flag = Furn { flags: 1, model: 129, size: [1, 1], height: 60 };
        assert!(near(at(&flag, 0x122, 0, 3, 0, 0, 3), [-6.5, -3.0, 0.0], 3.0 * 1.570_796));
        assert!(near(at(&flag, 0x121, 0, 3, 0, 0, 3), [-6.2325, -3.0, 0.0], 3.0 * 1.570_796));
        assert!(near(at(&flag, 0x267, 1, 40, 0, 0, 0), [0.0, -5.6, 8.0], 0.0));
        assert_eq!(place(&flag, 0x122, 0, 5, 0, 0, 0), None);
    }
}
