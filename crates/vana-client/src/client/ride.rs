//! Riding (mount_model_spec.md): a rider at status 5 / 0x55 sits on a second actor, the chocobo or mount
//! (FUN_1008f740, entity +0xa4), drawn at the rider's place; the rider gets its riding motion files and names and, on a
//! mount, is moved onto the saddle (FUN_100c9330).

use super::{EntityKind, Source, World};
use crate::client::net::Ride;
use crate::dat::character::Look;
use crate::dat::motionfile::PcMotion;
use crate::motion::MotionNames;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// The animal's actor id: its rider's with this bit. Never an entity, a target, a bump or a pick.
pub const ANIMAL: u32 = 0x4000_0000;

/// What a rider sits on (FUN_1008f740's kinds, FUN_100d2f60's files).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Model {
    /// Kinds 1 / 2 (ChocoboIndex 0 / 1): one file, 39276 in the [S] zones.
    Chocobo(u32),
    /// Kinds 3 / 4 (ChocoboIndex 2..4): built like a PC from race 32..36.
    Custom(Look),
    /// Kind 0x54: the mount id (LSB m_mountId, the mount list index + 1).
    Mount(u8),
}

impl Model {
    fn source(self) -> Source {
        match self {
            Model::Chocobo(f) => Source::File(f),
            Model::Custom(l) => Source::Pc(l),
            // FUN_100c51c0(4, id - 1) = 0x19131 + n.
            Model::Mount(id) => Source::File(102_705 + id.max(1) as u32 - 1),
        }
    }
}

/// A rider's state.
pub struct Riding {
    pub model: Model,
    race: u8,
    /// FUN_100d2800's v: 0 a chocobo, 3 a chocobo in the [S] zones, 5 a mount.
    v: u32,
    /// FUN_100c9330: the draw translation (mount point 0x2f + race - rider point 0x6b, both from a past frame's pose)
    /// and the yaw offset (FUN_100222b0).
    pub offset: Vec3,
    pub yaw: f32,
    points: [Option<Vec3>; 2],
}

/// The [S] zones (FUN_100fd3a0).
fn past(zone: u16) -> bool {
    matches!(zone, 80..=99 | 136 | 137)
}

/// FUN_1009ab60: a personal chocobo's look from CustomProperties (colour, beak, talons, tail), or the Noble Chocobo's
/// (cp1 bit 0).
pub fn choco_look(cp: [u32; 2]) -> Look {
    if cp[1] & 1 != 0 {
        return Look { race: 0x20, head: 3, body: 3, hands: 3, legs: 1, feet: 3, ..Default::default() };
    }
    let c = cp[0];
    Look { race: 0x20 + (c >> 9 & 7) as u8, head: (c & 7) as u16, body: 7, hands: (c >> 6 & 7) as u16, legs: 0, feet: (c >> 3 & 7) as u16, ..Default::default() }
}

/// FUN_1008f740's gates: status 5 a chocobo (a personal one only once its look is a chocobo race), 0x55 a mount by
/// its id (none: no animal).
pub fn model(status: u8, ride: Ride, zone: u16, in_event: bool) -> Option<Model> {
    match status {
        5 if (2..=4).contains(&ride.chocobo) => Some(choco_look(ride.custom)).filter(|l| (0x20..=0x24).contains(&l.race)).map(Model::Custom),
        5 => Some(Model::Chocobo(if past(zone) && !in_event { 39_276 } else { 39_275 })),
        0x55 if ride.mount != 0 => Some(Model::Mount(ride.mount)),
        _ => None,
    }
}

/// DAT_1035f5ac by race: legs and upper body.
const FILES: [u32; 9] = [38603, 38603, 38699, 38795, 38891, 38987, 38987, 39083, 39179];
/// DAT_1035f5c0 by race * 2 + (skirt set 1 ? 0 : 1).
const SKIRT: [u32; 18] = [38635, 38667, 38635, 38667, 38731, 38763, 38827, 38859, 38923, 38955, 39019, 39051, 39019, 39051, 39115, 39147, 39211, 39243];

/// FUN_100d2800: the rider's riding files (tag 7), `DAT_1035f5ac[race] + v` and, when the body sets a skirt, its file
/// `DAT_1035f5c0[..] + v`.
pub fn files(w: &World, id: u32, pc: &PcMotion) -> Vec<u32> {
    let Some(r) = w.rides.get(&id) else { return Vec::new() };
    let race = (pc.race as usize).min(8);
    let mut out = vec![FILES[race] + r.v];
    if pc.skirt > 0 {
        out.push(SKIRT[race * 2 + if pc.skirt == 1 { 0 } else { 1 }] + r.v);
    }
    out
}

/// FUN_100c8bb0: stand chi, walk wlk, run run while riding; a mount's variant digit replaces the first letter
/// (FUN_100c83c0: "6hi", "3un").
pub fn names(k: u8) -> MotionNames {
    const N: [[&str; 3]; 10] = [
        ["chi", "wlk", "run"],
        ["1hi", "1lk", "1un"],
        ["2hi", "2lk", "2un"],
        ["3hi", "3lk", "3un"],
        ["4hi", "4lk", "4un"],
        ["5hi", "5lk", "5un"],
        ["6hi", "6lk", "6un"],
        ["7hi", "7lk", "7un"],
        ["8hi", "8lk", "8un"],
        ["9hi", "9lk", "9un"],
    ];
    let [idle, walk, run] = N[if k > 9 { 0 } else { k as usize }];
    MotionNames { idle, battle: idle, walk, run }
}

/// Per frame, before the motion files: each rider's animal made, kept in step or dropped (at once: the dismount's
/// `coff` is not done), the rider's names and weapons, the animal's names, size and rate.
pub fn update(w: &mut World) {
    let zone = w.zone.unwrap_or(0);
    let mut riders: Vec<(u32, u8, Ride)> = w.entities.iter().filter(|(id, e)| **id != w.my_id && e.kind == EntityKind::Player).map(|(id, e)| (*id, e.motion.status, e.ride)).collect();
    riders.push((w.my_id, w.my_motion.status, w.my_ride));
    let mut keep = HashSet::new();
    for (id, status, ride) in riders {
        let bound = super::event::pose(w, id).is_some();
        let Some(m) = model(status, ride, zone, bound) else { continue };
        let Some(rider) = w.actors.get(&id).filter(|a| a.character.as_ref().is_some_and(|c| c.pc.is_some())) else { continue };
        let (ch, rider_scale) = (rider.character.clone().unwrap(), rider.scale);
        let pc = ch.pc.unwrap();
        keep.insert(id);
        let animal_id = ANIMAL | id;
        let src = m.source();
        if w.actors.get(&animal_id).is_none_or(|a| a.key != format!("{src:?}")) && !w.event_looks.iter().any(|(i, _)| *i == animal_id) {
            w.event_looks.push((animal_id, src));
        }
        let animal = w.actors.get(&animal_id).and_then(|a| a.character.clone());
        let mi = animal.as_ref().and_then(|c| c.mount);
        let r = w.rides.entry(id).or_insert(Riding { model: m, race: pc.race, v: 0, offset: Vec3::ZERO, yaw: 0.0, points: [None; 2] });
        if r.model != m || r.race != pc.race {
            *r = Riding { model: m, race: pc.race, v: 0, offset: Vec3::ZERO, yaw: 0.0, points: [None; 2] };
        }
        let mount = matches!(m, Model::Mount(_));
        r.v = if mount { 5 } else if past(zone) && !bound { 3 } else { 0 };
        r.yaw = mi.map_or(0.0, |mi| mi.yaw(pc.race));
        if let Some(a) = w.actors.get_mut(&id) {
            a.motion_names = Some(names(mi.map_or(0, |mi| mi.variant(pc.race))));
            // FUN_100d60c0(0): no weapons on a mount.
            if mount {
                for h in &mut a.wep_hide[..=8] {
                    h[0] = true;
                }
            }
        }
        // FUN_100cf210: the animal's size info[10 + rider info[13]] / 100 x the rider's; its motions the rider's names
        // at info[15] / 100 (a chocobo keeps step).
        if let (Some(an), Some(a)) = (animal, w.actors.get_mut(&animal_id)) {
            let big = if ch.size_info[3] <= 3 { ch.size_info[3] } else { 0 };
            a.event_scale = Some(an.scale(big) * rider_scale);
            a.motion_names = Some(names(0));
            a.rate_scale = mi.map_or(1.0, |mi| mi.rate());
        }
    }
    let gone: Vec<u32> = w.rides.keys().filter(|id| !keep.contains(id)).copied().collect();
    for id in gone {
        let r = w.rides.remove(&id).unwrap();
        w.actors.remove(&(ANIMAL | id));
        if let Some(a) = w.actors.get_mut(&id) {
            a.motion_names = None;
            if matches!(r.model, Model::Mount(_)) {
                for h in &mut a.wep_hide[..=8] {
                    h[0] = false;
                }
            }
        }
    }
    // An animal whose rider stopped riding before it loaded.
    let stray: Vec<u32> = w.actors.keys().filter(|id| **id & ANIMAL != 0 && !w.rides.contains_key(&(**id & !ANIMAL))).copied().collect();
    for id in stray {
        w.actors.remove(&id);
    }
}

/// The readback key of a rider's point (slot 0) or its mount's saddle point (slot 1).
fn key(rider: u32, slot: u64) -> u64 {
    1 << 62 | (rider as u64) << 8 | slot
}

/// Whether a readback key is a riding point.
pub fn is_key(k: u64) -> bool {
    k >> 62 == 1
}

/// FUN_100c9330's points for an actor drawn this frame: a mounted rider's point 0x6b, its mount's saddle point
/// DAT_10330ed4[race] = 0x2f + race (0x30 for race 0); as (key, point index).
pub fn query(rides: &HashMap<u32, Riding>, id: u32) -> Option<(u64, usize)> {
    let rider = id & !ANIMAL;
    let r = rides.get(&rider).filter(|r| matches!(r.model, Model::Mount(_)))?;
    Some(if id & ANIMAL != 0 { (key(rider, 1), 0x2f + r.race.max(1) as usize) } else { (key(rider, 0), 0x6b) })
}

/// The points read back (relative to each actor's drawn root, world axes): offset = saddle - rider point.
pub fn points(w: &mut World, pts: Vec<(u64, Vec3)>) {
    for (k, p) in pts {
        if let Some(r) = w.rides.get_mut(&((k >> 8) as u32)) {
            r.points[(k & 1) as usize] = Some(p);
            if let [Some(rp), Some(mp)] = r.points {
                if r.offset == Vec3::ZERO {
                    log::info!("rider {:#x}: saddle offset {:?} (point 0x6b {rp:?}, saddle {mp:?})", k >> 8 & 0xffff_ffff, mp - rp);
                }
                r.offset = mp - rp;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chocobo_looks() {
        // Blue (2), beak 1, talons 3, tail 5.
        let l = choco_look([2 << 9 | 5 << 6 | 3 << 3 | 1, 0]);
        assert_eq!((l.race, l.head, l.body, l.hands, l.legs, l.feet), (0x22, 1, 7, 5, 0, 3));
        let n = choco_look([0, 1]);
        assert_eq!((n.race, n.head, n.body, n.hands, n.legs, n.feet), (0x20, 3, 3, 3, 1, 3));
    }

    #[test]
    fn models_and_names() {
        let ride = |chocobo, mount| Ride { chocobo, mount, custom: [0; 2] };
        assert_eq!(model(5, ride(1, 0), 230, false), Some(Model::Chocobo(39_275)));
        assert_eq!(model(5, ride(1, 0), 87, false), Some(Model::Chocobo(39_276)));
        assert_eq!(model(5, ride(1, 0), 87, true), Some(Model::Chocobo(39_275)));
        assert_eq!(model(0x55, ride(3, 0), 100, false), None);
        assert_eq!(model(0x55, ride(3, 2), 100, false).map(|m| format!("{:?}", m.source())), Some("File(102706)".into()));
        // A personal chocobo's colour 7 is no chocobo race: nothing until its look comes.
        assert_eq!(model(5, Ride { chocobo: 2, mount: 0, custom: [7 << 9, 0] }, 100, false), None);
        assert_eq!((names(6).idle, names(3).run, names(0).walk), ("6hi", "3un", "wlk"));
    }

    #[test]
    fn riding_files() {
        assert_eq!((FILES[1], FILES[8], SKIRT[2], SKIRT[3]), (38603, 39179, 38635, 38667));
    }
}
