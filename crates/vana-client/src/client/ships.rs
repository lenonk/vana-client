//! Ships (0x00E SubKind 4 -> actor kind 5, the XiModelActor; spec ~/ffxi/lab/research/ship_actors_spec.md): the boats
//! and airships seen from the docks, the overhead airships, the pirate ship. One MMB from the ship's own DAT, placed
//! where the server first put it and then moved only by that file's sequences: states 0x12..0x19 play `seq0`..`seq7`
//! (DAT_1032d42c) from the server's stamp, caught up when more than 10 s late (FUN_10097a80 / FUN_100c4730).

use super::World;
use crate::dat::DatIndex;
use crate::dat::zone::{Keys, Placement, Texture};
use crate::render::ZoneGeometry;
use anyhow::{Result, anyhow};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// The model actor's state (+0x734 / +0x744 / +0xe4 / +0x770..+0x778, +0x170).
pub struct ModelActor {
    pub file: u32,
    pub pos: Vec3,
    /// Drawn (+0x744) and wanted (+0xe4) rotation: (x, yaw, pitch).
    pub rot: Vec3,
    pub target: Vec3,
    /// +0x778, a `fune` / `_fun` model: the heading eases 1/8 a tick toward the target.
    smooth: bool,
    /// +0x774: the first 4 draws are skipped (FUN_100c4c20).
    skip_draws: u8,
    /// The state in force (+0x170).
    pub state: u8,
    pub matrix: Mat4,
    pub shown: bool,
}

#[derive(Default)]
pub struct Ships {
    pub actors: HashMap<u32, ModelActor>,
    /// Loads in flight by entity, with their number (a later one supersedes it), and those done for the renderer.
    loading: HashMap<u32, u64>,
    seq: u64,
    pub ready: Vec<Loaded>,
    /// Entities whose model the renderer drops.
    pub removed: Vec<u32>,
}

pub struct Loaded {
    pub entity: u32,
    pub seq: u64,
    pub file: u32,
    pub mesh: String,
    pub geo: Arc<ZoneGeometry>,
    pub textures: Vec<Texture>,
}

/// A load the frame wants started: entity, its number, the model file.
pub type Job = (u32, u64, u32);

/// FUN_100c41f0: the model file. State 0x37 at creation takes the furniture table (FUN_100c51c0(2, id): the props),
/// else id + 0x791c.
fn model_file(id: u32, state: u8) -> Option<u32> {
    if state == 0x37 { crate::dat::furniture::model_file(id as u16) } else { Some(id + 0x791c) }
}

/// The state in force (all.c 110607): the server's, or the event's (+0x174, ops 0x4c / 0x4d / 0x4f / 0x8e / 0x8f)
/// while an event runs.
fn in_force(w: &World, id: u32) -> Option<u8> {
    let e = w.entities.get(&id)?;
    let bound = super::event::active(w).then(|| w.doors.event.get(&id).copied()).flatten();
    Some(bound.unwrap_or(e.motion.status))
}

/// DAT_1032d42c: states 0x12..0x19 name `seq0`..`seq7`; the others no sequence.
fn sequence(state: u8) -> Option<u32> {
    (0x12..=0x19).contains(&state).then(|| u32::from_le_bytes([b's', b'e', b'q', b'0' + state - 0x12]))
}

/// FUN_10097a80: `elapsed = now - start` in ticks (start 0: now); under 600 from the first op, else caught up.
fn start_seq(w: &mut World, id: u32, start: u32) {
    let Some(a) = w.ships.actors.get(&id) else { return };
    let Some(name) = sequence(a.state) else {
        w.effects.kill_sequence(id);
        return;
    };
    let now = super::vana_now(w) as i64 * 60;
    let s = if start == 0 { now } else { start as i64 * 60 };
    let elapsed = now - s;
    let skip = if elapsed < 600 { 0.0 } else { elapsed as f32 };
    log::debug!("ship {id:#x}: state {:#x} {}, elapsed {elapsed} ticks, skip {skip}", a.state, String::from_utf8_lossy(&name.to_le_bytes()));
    w.effects.start_sequence(id, a.file, name, skip);
}

/// Per frame, after the net events and doors: models to load for new ship entities (and rebuilt ones), gone ones
/// dropped, state changes played (FUN_1008c6c0 kind 5, quiet under event op 0xab 5).
pub fn update(w: &mut World) -> Vec<Job> {
    let gone: Vec<u32> = w.ships.actors.keys().copied().filter(|id| w.entities.get(id).is_none_or(|e| e.ship.is_none() || e.ship_rebuild)).collect();
    for id in gone {
        drop_actor(w, id);
    }
    w.ships.loading.retain(|id, _| w.entities.get(id).is_some_and(|e| e.ship.is_some()));
    let mut jobs = Vec::new();
    let ships: Vec<(u32, u32, u32)> = w.entities.iter().filter_map(|(id, e)| e.ship.map(|(ship, start)| (*id, ship, start))).collect();
    for (id, ship, start) in ships {
        if let Some(e) = w.entities.get_mut(&id) {
            e.ship_rebuild = false;
        }
        let Some(state) = in_force(w, id) else { continue };
        match w.ships.actors.get(&id).map(|a| a.state) {
            None if !w.ships.loading.contains_key(&id) => {
                let Some(file) = model_file(ship, state) else { continue };
                w.ships.seq += 1;
                w.ships.loading.insert(id, w.ships.seq);
                jobs.push((id, w.ships.seq, file));
            }
            Some(old) if old != state => {
                let quiet = w.entities.get(&id).is_some_and(|e| e.quiet_status);
                if let Some(a) = w.ships.actors.get_mut(&id) {
                    a.state = state;
                }
                if !quiet {
                    let bound = super::event::active(w) && w.doors.event.contains_key(&id);
                    start_seq(w, id, if bound { 0 } else { start });
                }
            }
            _ => {}
        }
    }
    jobs
}

fn drop_actor(w: &mut World, id: u32) {
    if w.ships.actors.remove(&id).is_some() {
        log::debug!("ship {id:#x}: model dropped");
        w.effects.kill_sequence(id);
        w.effects.forget_actor(id);
        w.ships.removed.push(id);
    }
    w.ships.loading.remove(&id);
}

/// A loaded model becomes the actor (FUN_1008f740 kind 5): at the entity's position and heading now, its sequence
/// started; ids 0, 1, 2, 5, 6 (the airship docks) keep their file's model generators off (entity +0x130 bit 1).
pub fn create(w: &mut World, l: &Loaded) -> Option<Mat4> {
    if w.ships.loading.get(&l.entity) != Some(&l.seq) {
        return None;
    }
    w.ships.loading.remove(&l.entity);
    let (ship, start) = w.entities.get(&l.entity)?.ship?;
    let state = in_force(w, l.entity)?;
    let e = w.entities.get_mut(&l.entity)?;
    if state != 0x37 && matches!(ship, 0 | 1 | 2 | 5 | 6) {
        e.gens_off = true;
    }
    let (pos, rot) = (e.pos, Vec3::new(0.0, super::event::rad_of(e.dir), 0.0));
    let smooth = l.mesh == "fune" || l.mesh == "_fun";
    let matrix = model_matrix(pos, rot);
    log::info!("ship {:#x}: id {ship}, file {}, mesh {}, state {state:#x} at {pos} yaw {:.3}", l.entity, l.file, l.mesh, rot.y);
    w.ships.actors.insert(l.entity, ModelActor { file: l.file, pos, rot, target: rot, smooth, skip_draws: 4, state, matrix, shown: false });
    let bound = super::event::active(w) && w.doors.event.contains_key(&l.entity);
    start_seq(w, l.entity, if bound { 0 } else { start });
    Some(matrix)
}

/// FUN_100c4b50: identity, then RotZ(z), RotY(y), RotX(x) post-multiplied, then the position: T * Rx * Ry * Rz.
fn model_matrix(pos: Vec3, rot: Vec3) -> Mat4 {
    Mat4::from_translation(pos) * Mat4::from_rotation_x(rot.x) * Mat4::from_rotation_y(rot.y) * Mat4::from_rotation_z(rot.z)
}

fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// After the effects tick (FUN_100c47f0): the drives' poses, the heading eased toward its target (1/8 a tick on
/// `fune` / `_fun`, y the short way *(ours: continuous, the same at 60 fps)*), and whether it draws. Returns the
/// actors whose matrix or visibility changed: (entity, matrix, shown).
pub fn apply(w: &mut World, ticks: f32, hidden: &std::collections::HashSet<u32>) -> Vec<(u32, Mat4, bool)> {
    let poses = &w.effects.actor_poses;
    let mut out = Vec::new();
    for (id, a) in w.ships.actors.iter_mut() {
        if let Some(p) = poses.get(id) {
            if let Some(at) = p.pos {
                a.pos = at;
            }
            if let Some(r) = p.snap {
                (a.rot, a.target) = (r, r);
            }
            if let Some(r) = p.target {
                a.target = r;
            }
        }
        if a.smooth {
            let mut d = a.target - a.rot;
            d.y = wrap_pi(d.y);
            a.rot += d * (1.0 - (7.0f32 / 8.0).powf(ticks));
            a.rot = Vec3::new(wrap_pi(a.rot.x), wrap_pi(a.rot.y), wrap_pi(a.rot.z));
        } else {
            a.rot = a.target;
        }
        let m = model_matrix(a.pos, a.rot);
        // The server's hide (flags1 bit 1) and the event hides; the first 4 draws count drawn frames only.
        let visible = w.entities.get(id).is_some_and(|e| e.flags & 2 == 0) && !hidden.contains(id);
        let show = visible && a.skip_draws == 0;
        if visible {
            a.skip_draws = a.skip_draws.saturating_sub(1);
        }
        if m != a.matrix || show != a.shown {
            (a.matrix, a.shown) = (m, show);
            out.push((*id, m, show));
        }
    }
    out
}

/// The ships for the effects: entity, model file, matrix, drawn rotation, hidden.
pub fn fx_actors(ships: &Ships, entities: &HashMap<u32, super::Entity>) -> Vec<(u32, u32, Mat4, Vec3, bool)> {
    ships.actors.iter().map(|(id, a)| (*id, a.file, a.matrix, a.rot, entities.get(id).is_none_or(|e| e.flags & 2 != 0))).collect()
}

/// FUN_100c4410's MMB: `fune`, else `_fun` (alpha tested), else the first.
fn pick_mesh(names: &[String]) -> Option<usize> {
    names.iter().position(|n| n == "fune").or_else(|| names.iter().position(|n| n == "_fun")).or((!names.is_empty()).then_some(0))
}

/// The loader thread: the file's scene, its one MMB placed at the origin (the instance takes the actor's matrix).
pub fn load(index: &DatIndex, keys: &Keys, (entity, seq, file): Job) -> Result<Loaded> {
    let bytes = index.read(file)?;
    let mut scene = crate::dat::zone::load_scene(keys, &bytes)?;
    let names: Vec<String> = scene.meshes.iter().map(|m| m.name.clone()).collect();
    let k = pick_mesh(&names).ok_or_else(|| anyhow!("ship file {file}: no MMB"))?;
    let mesh = scene.meshes.swap_remove(k);
    scene.meshes = vec![mesh];
    let name = scene.meshes[0].name.clone();
    scene.placements = vec![Placement { name: name.clone(), translate: [0.0; 3], rotate: [0.0; 3], scale: [1.0; 3], lights: [0; 4], link: 0, area: 0, id: 0, lod: [0.0; 2], world: false, own_dist: None }];
    let geo = Arc::new(ZoneGeometry::build(&scene));
    Ok(Loaded { entity, seq, file, mesh: name, geo, textures: std::mem::take(&mut scene.textures) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ship_actors_spec 8: ids 0..21 are files 31004..31025; `fune` for 0..6 and 13..21, `_fun` for 7..12; file
    /// 31020 (the barge) takes `fune` over `su00`.
    #[test]
    fn files() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let Ok(keys) = Keys::from_ffximain(&dll) else { return };
        for id in 0..22u32 {
            let file = model_file(id, 0x12).unwrap();
            assert_eq!(file, 31004 + id);
            let l = load(&index, &keys, (1, 1, file)).unwrap_or_else(|e| panic!("ship {id}: {e:#}"));
            let want = if (7..=12).contains(&id) { "_fun" } else { "fune" };
            assert_eq!(l.mesh, want, "ship {id}");
        }
    }

    #[test]
    fn sequences() {
        assert_eq!(sequence(0x12), Some(u32::from_le_bytes(*b"seq0")));
        assert_eq!(sequence(0x19), Some(u32::from_le_bytes(*b"seq7")));
        assert_eq!(sequence(0x37), None);
        assert_eq!(sequence(0x11), None);
    }
}
