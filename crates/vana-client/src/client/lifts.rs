//! Lifts (XiLiftActor, collision_actors_spec 2): a lift NPC (0x00E SubKind 3) names its shaft ('@' RID box and door
//! group) at @0x34. A floor change plays the folder's `mv<old><new>`, whose op 0x1d takes the group's parts and the
//! shaft's height to that floor by the packet's end time; whoever stands in the shaft rides at that height
//! (FUN_100a4ed0).

use super::World;
use crate::dat::zone::Region;
use glam::{Mat3, Vec3};
use std::collections::HashMap;

/// A shaft ('@' record): its box, the floors' heights, the surface type and the height now (RID +0xdc).
#[derive(Clone)]
pub struct Shaft {
    tag: u32,
    region: Region,
    floors: [f32; 2],
    surface: i8,
    height: f32,
}

/// Op 0x1d's move: from, to over `total` ticks from `t0` (the zone's tick clock, as the door parts).
#[derive(Clone, Copy)]
struct Move {
    from: f32,
    to: f32,
    t0: f32,
    total: f32,
    floor: u8,
}

/// A lift actor: its shaft, the state in force (+0x170), its floor (+0x5f8), the move's end (+0x5fc) and the move.
struct Lift {
    tag: u32,
    state: u8,
    floor: u8,
    end: u32,
    mv: Option<Move>,
}

#[derive(Default)]
pub struct Lifts {
    shafts: Vec<Shaft>,
    lifts: HashMap<u32, Lift>,
    /// DAT_10482f45: a lift actor exists (the zone-in fade waits for one while we stand in a shaft).
    pub spawned: bool,
}

/// FUN_10176620: the first shaft holding `new` or `old` (inclusive; the face test is dead in retail, world-space
/// coordinates against a unit cube at the origin), its height and surface type.
pub fn ride_in(shafts: &[Shaft], old: Vec3, new: Vec3) -> Option<(f32, i8)> {
    shafts.iter().find(|s| s.region.contains(new.to_array()) || s.region.contains(old.to_array())).map(|s| (s.height, s.surface))
}

fn tag_of(name: &str) -> u32 {
    let b = name.as_bytes();
    u32::from_le_bytes(std::array::from_fn(|i| b.get(i).copied().unwrap_or(0)))
}

impl Lifts {
    /// The zone's '@' records in RID order (FUN_10173c10); a height starts at its box centre.
    pub fn load(regions: &[Region]) -> Self {
        let shafts = regions.iter().filter(|r| r.is_lift()).map(|r| Shaft { tag: tag_of(&r.name), region: r.clone(), floors: [r.floor_y(0), r.floor_y(1)], surface: r.surface(), height: r.pos[1] }).collect();
        Self { shafts, ..Default::default() }
    }

    pub fn ride(&self, old: Vec3, new: Vec3) -> Option<(f32, i8)> {
        ride_in(&self.shafts, old, new)
    }

    /// The shafts as they stand this frame, for the physics' ride query.
    pub fn shafts(&self) -> Vec<Shaft> {
        self.shafts.clone()
    }
}

fn mv(from: u8, to: u8) -> u32 {
    u32::from_le_bytes([b'm', b'v', b'0' + from, b'0' + to])
}

/// Per frame, before the physics: new lift actors (FUN_1008f740 kind 4: FUN_100c3a60 floor 0, then FUN_100c3cf0's
/// `mv0<floor>`), floor changes once their start time has come (FUN_1008b460 -> FUN_100c3d50), and the heights now.
pub fn update(w: &mut World) {
    let now_vana = super::vana_now(w) as u32;
    let lifts: Vec<(u32, u32, u8, u32, u32)> = w.entities.iter().filter_map(|(id, e)| e.lift.map(|(tag, start, end)| (*id, tag, e.motion.status, start, end))).collect();
    w.lifts.lifts.retain(|id, _| w.entities.get(id).is_some_and(|e| e.lift.is_some()));
    for (id, tag, status, start, end) in lifts {
        let floor_of = |s: u8| s.saturating_sub(10).min(7);
        match w.lifts.lifts.get(&id).map(|l| (l.state, l.floor)) {
            None => {
                // Without a door group nothing else happens.
                let grouped = w.renderer.as_ref().is_some_and(|r| r.door_parts.contains_key(&tag));
                let Some(s) = w.lifts.shafts.iter_mut().find(|s| s.tag == tag).filter(|_| grouped) else { continue };
                s.height = s.floors[0];
                let floor = floor_of(status);
                w.lifts.lifts.insert(id, Lift { tag, state: status, floor, end: 0, mv: None });
                w.lifts.spawned = true;
                let played = w.effects.door_scheduler(id, tag, mv(0, floor));
                log::debug!("lift {:#x}: first sight, state {status}, mv0{floor} {played}", tag);
            }
            // Before the start time it waits.
            Some((state, floor)) if (10..=17).contains(&state) && status != state && now_vana >= start => {
                let new = floor_of(status);
                if let Some(l) = w.lifts.lifts.get_mut(&id) {
                    (l.end, l.floor, l.state) = (end, new, status);
                }
                let played = w.effects.door_scheduler(id, tag, mv(floor, new));
                log::debug!("lift {:#x}: state {state} -> {status}, mv{floor}{new} {played}, end {end} (now {now_vana})", tag);
            }
            Some(_) => {}
        }
    }
    let now = w.started.elapsed().as_secs_f32() * 60.0;
    let Lifts { shafts, lifts, .. } = &mut w.lifts;
    for l in lifts.values_mut() {
        let Some(m) = l.mv else { continue };
        let k = if m.total <= 0.0 { 1.0 } else { ((now - m.t0) / m.total).clamp(0.0, 1.0) };
        if let Some(s) = shafts.iter_mut().find(|s| s.tag == l.tag) {
            s.height = m.from + (m.to - m.from) * k;
        }
        if k >= 1.0 {
            (l.floor, l.mv) = (m.floor, None);
        }
    }
}

/// The frame's op 0x1d moves: from the height now to the floor by the move's end time (whole seconds, FUN_101039e0;
/// a move started at or after it snaps), on the group's parts (FUN_100c3920: each part's world y = h) and the shaft.
pub fn moves(w: &mut World, gpu: &crate::render::Gpu) {
    let moves = std::mem::take(&mut w.effects.lift_moves);
    if moves.is_empty() {
        return;
    }
    let now_vana = super::vana_now(w) as i64;
    let now = w.started.elapsed().as_secs_f32() * 60.0;
    for m in moves {
        let Some(l) = w.lifts.lifts.get_mut(&m.actor) else { continue };
        let Some(s) = w.lifts.shafts.iter().find(|s| s.tag == l.tag) else { continue };
        let to = s.floors[(m.floor as usize).min(1)];
        let total = ((l.end as i64 - now_vana).max(0) * 60) as f32;
        l.mv = Some(Move { from: s.height, to, t0: now, total, floor: m.floor });
        log::debug!("lift {:#x}: y {} -> {to} over {total} ticks", l.tag, s.height);
        let Some(r) = w.renderer.as_mut() else { continue };
        let (Some(slots), Some(bases)) = (r.door_parts.get(&l.tag).cloned(), r.door_bases.get(&l.tag).cloned()) else { continue };
        for (slot, base) in slots.iter().zip(&bases) {
            // base * T(t): the part's world translation y becomes `to`.
            let t = Mat3::from_mat4(*base).inverse() * Vec3::new(0.0, to - base.w_axis.y, 0.0);
            r.move_part(gpu, *slot, false, t.to_array(), total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Metalworks (237) '@6l0': floors 1.962 / -9.983, surface 0; the shaft holds the lower floor, not above it.
    #[test]
    fn metalworks_shaft() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(keys) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).map_err(anyhow::Error::from).and_then(|d| crate::dat::zone::Keys::from_ffximain(&d)) else { return };
        let file = index.read(crate::dat::zone::file_id(237)).unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &file).unwrap();
        let mut lifts = Lifts::load(&scene.regions);
        let s = lifts.shafts.iter_mut().find(|s| s.tag == tag_of("@6l0")).unwrap();
        assert!((s.floors[0] - 1.962).abs() < 0.01 && (s.floors[1] + 9.983).abs() < 0.01, "{:?}", s.floors);
        assert_eq!(s.surface, 0);
        s.height = s.floors[0];
        let at = Vec3::new(-55.98, 1.962, -12.02);
        assert_eq!(lifts.ride(at, at).map(|r| r.0), Some(lifts.shafts.iter().find(|s| s.tag == tag_of("@6l0")).unwrap().floors[0]));
        assert!(lifts.ride(Vec3::new(-55.98, 3.0, -12.02), Vec3::new(-55.98, 3.0, -12.02)).is_none());
        // The group's schedulers.
        let mut fx = crate::fx::Effects::new();
        fx.load_zone(crate::fx::lib::Library::load(crate::dat::zone::file_id(237), file).unwrap());
        assert!(fx.door_scheduler(1, tag_of("@6l0"), mv(0, 1)));
        let geo = crate::render::ZoneGeometry::build(&scene);
        assert!(geo.door_parts.get(&tag_of("@6l0")).is_some_and(|p| !p.is_empty()));
    }
}
