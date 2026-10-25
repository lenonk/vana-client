//! Riding (mount_model_spec.md): a rider at status 5 / 0x55 sits on a second actor, the chocobo or mount
//! (FUN_1008f740, entity +0xa4), drawn at the rider's place; the rider gets its riding motion files and names and, on a
//! mount, is moved onto the saddle (FUN_100c9330).

use super::{EntityKind, Source, World, actmotion};
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

/// FUN_1008b460's states for a rider: on a mount the rider waits hidden for it (state 0 -> 0xe, at most 150 updates),
/// riding, then leaving (states 0xf / 0x10: `coff` on both, the animal unlinked where it stood). Our own rider first
/// comes to a stand (`Stopping`, state 0xf's FUN_100d65b0 wait).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Mounting(f32),
    Riding,
    Stopping,
    /// The animal's place, its op 0x6e run (ticks left, units a tick), and ticks since.
    Leaving { pos: Vec3, forward: Vec3, run: Option<(f32, f32)>, ticks: f32 },
}

/// A rider's state.
pub struct Riding {
    pub model: Model,
    pub phase: Phase,
    race: u8,
    /// FUN_100d2800's v: 0 a chocobo, 3 a chocobo in the [S] zones, 5 a mount.
    v: u32,
    /// FUN_100c9330: the draw translation (mount point 0x2f + race - rider point 0x6b, both from a past frame's pose)
    /// and the yaw offset (FUN_100222b0).
    pub offset: Vec3,
    pub yaw: f32,
    points: [Option<Vec3>; 2],
    /// The rider's `wepN` byte 0 before the mount hid them (FUN_100d5ef0 clears them all; ours keeps an event's).
    weps: [bool; 9],
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

/// Per frame, before the motion files: each rider's animal made and kept in step, the rider's names and weapons, the
/// animal's names, size and rate; a rider no longer riding leaves through `coff`.
pub fn update(w: &mut World, dt: f32) {
    let zone = w.zone.unwrap_or(0);
    let mut riders: Vec<(u32, u8, Ride)> = w.entities.iter().filter(|(id, e)| **id != w.my_id && e.kind == EntityKind::Player).map(|(id, e)| (*id, e.motion.status, e.ride)).collect();
    riders.push((w.my_id, w.my_motion.status, w.my_ride));
    // Bound actors an event put on a chocobo or mount (op 0x7e, op 0xac sub 1).
    for id in super::event::riding(w) {
        if !riders.iter().any(|r| r.0 == id) {
            riders.push((id, 0, w.entities.get(&id).map_or_else(Ride::default, |e| e.ride)));
        }
    }
    let mut keep = HashSet::new();
    for (id, status, ride) in riders {
        // A dismount runs to its end whatever the status does meanwhile.
        if w.rides.get(&id).is_some_and(|r| matches!(r.phase, Phase::Leaving { .. })) {
            keep.insert(id);
            continue;
        }
        // A bound actor rides by its event status (FUN_1008e9a0 takes +0x174), on op 0x7e's animal or its own.
        let bound = super::event::pose(w, id).is_some();
        let m = if bound {
            let st = super::event::event_status(w, id);
            super::event::ride_model(w, id).filter(|_| matches!(st, 5 | 0x55)).or_else(|| model(st, ride, zone, true))
        } else {
            model(status, ride, zone, false)
        };
        let Some(m) = m else {
            // Still coming to a stand: riding goes on.
            if w.rides.get(&id).is_some_and(|r| r.phase == Phase::Stopping) {
                keep.insert(id);
            }
            continue;
        };
        if let Some(r) = w.rides.get_mut(&id).filter(|r| r.phase == Phase::Stopping) {
            r.phase = Phase::Riding;
        }
        let Some(rider) = w.actors.get(&id).filter(|a| a.character.as_ref().is_some_and(|c| c.pc.is_some())) else { continue };
        let (ch, rider_scale) = (rider.character.clone().unwrap(), rider.scale);
        let pc = ch.pc.unwrap();
        keep.insert(id);
        let animal_id = ANIMAL | id;
        let src = m.source();
        if w.actors.get(&animal_id).is_none_or(|a| a.key != format!("{src:?}")) && !w.event_looks.iter().any(|(i, _)| *i == animal_id) {
            w.event_looks.push((animal_id, src));
        }
        let animal = w.actors.get(&animal_id).filter(|a| a.key == format!("{src:?}")).and_then(|a| a.character.clone());
        let mi = animal.as_ref().and_then(|c| c.mount);
        let mount = matches!(m, Model::Mount(_));
        let weps = rider.wep_hide;
        let fresh = |weps: [bool; 9]| Riding { model: m, phase: if mount { Phase::Mounting(0.0) } else { Phase::Riding }, race: pc.race, v: 0, offset: Vec3::ZERO, yaw: 0.0, points: [None; 2], weps };
        let r = w.rides.entry(id).or_insert_with(|| fresh(std::array::from_fn(|n| weps[n][0])));
        if r.model != m || r.race != pc.race {
            *r = fresh(r.weps);
        }
        // State 0xe: the counter runs to 150 updates, or jumps there once the mount is in; then the rider's fade
        // restarts (+0x59c = 0) and it shows.
        let mut shown = false;
        if let Phase::Mounting(t) = &mut r.phase {
            *t += dt * 60.0;
            if animal.is_some() || *t >= 150.0 {
                (r.phase, shown) = (Phase::Riding, true);
            }
        }
        r.v = if mount { 5 } else if past(zone) && !bound { 3 } else { 0 };
        r.yaw = mi.map_or(0.0, |mi| mi.yaw(pc.race));
        if let Some(a) = w.actors.get_mut(&id) {
            if shown {
                a.appear = 0.0;
            }
            a.motion_names = Some(names(mi.map_or(0, |mi| mi.variant(pc.race))));
        }
        // FUN_100d60c0(0): no weapons on a mount.
        if mount {
            super::wepfx::hide_all(w, id);
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
    let gone: Vec<u32> = w.rides.iter().filter(|(id, r)| !keep.contains(id) && !matches!(r.phase, Phase::Leaving { .. })).map(|(id, _)| *id).collect();
    for id in gone {
        // State 0xf: our own rider with its animal waits until a `?hi` stand plays (FUN_100d65b0(0)).
        let animal = w.actors.get(&(ANIMAL | id)).is_some_and(|a| a.character.is_some());
        if id == w.my_id && animal && !standing(w, id) {
            if let Some(r) = w.rides.get_mut(&id) {
                r.phase = Phase::Stopping;
            }
        } else {
            leave(w, id);
        }
    }
    let stopped: Vec<u32> = w.rides.iter().filter(|(id, r)| r.phase == Phase::Stopping && keep.contains(id) && standing(w, **id)).map(|(id, _)| *id).collect();
    for id in stopped {
        leave(w, id);
    }
    let leaving: Vec<u32> = w.rides.iter().filter(|(_, r)| matches!(r.phase, Phase::Leaving { .. })).map(|(id, _)| *id).collect();
    for id in leaving {
        step_leave(w, id, dt);
    }
    // An animal whose rider stopped riding before it loaded.
    let stray: Vec<u32> = w.actors.keys().filter(|id| **id & ANIMAL != 0 && !w.rides.contains_key(&(**id & !ANIMAL))).copied().collect();
    for id in stray {
        w.actors.remove(&id);
    }
}

/// State 0xf: a mount's rider goes to colour 0x00808080; `coff` on both (the rider's from its riding files, which stay
/// on until it ends); unlinked, the rider's own names back. The animal stays where it stood. Without a loaded animal or a
/// `coff`, the ride ends at once.
fn leave(w: &mut World, id: u32) {
    let animal_id = ANIMAL | id;
    let my_dir = w.net.me.lock().map(|m| m.dir).unwrap_or(0);
    let place = super::base_place(w, id, my_dir);
    let (Some(rch), Some(ach), Some(p)) = (w.actors.get(&id).and_then(|a| a.character.clone()), w.actors.get(&animal_id).and_then(|a| a.character.clone()), place) else {
        finish(w, id);
        return;
    };
    let Some(r) = w.rides.get_mut(&id) else { return };
    let mount = matches!(r.model, Model::Mount(_));
    (r.phase, r.offset, r.yaw, r.points) = (Phase::Leaving { pos: p.pos, forward: p.forward, run: None, ticks: 0.0 }, Vec3::ZERO, 0.0, [None; 2]);
    if let Some(a) = w.actors.get_mut(&id) {
        (a.motion_names, a.rebase) = (None, true);
    }
    let base = actmotion::status_motion(w, id, &rch);
    let started = w.actors.get_mut(&id).is_some_and(|a| actmotion::start(a, &rch, "coff", &base));
    if started {
        if mount && let Some(a) = w.actors.get_mut(&id) {
            (a.alpha, a.rgb, a.fade) = (0.0, [actmotion::OPAQUE; 3], None);
        }
        actmotion::fx_too(&mut w.effects, id, "coff");
    }
    let base = actmotion::status_motion(w, animal_id, &ach);
    if w.actors.get_mut(&animal_id).is_some_and(|a| actmotion::start(a, &ach, "coff", &base)) {
        actmotion::fx_too(&mut w.effects, animal_id, "coff");
    }
    log::info!("rider {id:#x}: dismount, coff {}", if started { "playing" } else { "missing" });
    if !started {
        finish(w, id);
    }
}

/// State 0x10: the animal runs (op 0x6e); the ride ends with the rider's `coff` (or after 600 ticks).
fn step_leave(w: &mut World, id: u32, dt: f32) {
    let over = w.actors.get(&id).is_none_or(|a| a.play.as_ref().is_none_or(|p| p.name != "coff"));
    let Some(Riding { phase: Phase::Leaving { pos, forward, run, ticks }, .. }) = w.rides.get_mut(&id) else { return };
    *ticks += dt * 60.0;
    if let Some((left, speed)) = run {
        let n = (dt * 60.0).min(*left);
        *pos += *forward * *speed * n;
        *left -= n;
    }
    if over || *ticks > 600.0 {
        finish(w, id);
    }
}

/// FUN_1008c6c0 / FUN_10092870: the riding files go (ride::files has no ride), the animal is deleted, the weapons
/// come back.
fn finish(w: &mut World, id: u32) {
    let Some(r) = w.rides.remove(&id) else { return };
    w.actors.remove(&(ANIMAL | id));
    if let Some(a) = w.actors.get_mut(&id) {
        (a.motion_names, a.rebase) = (None, true);
        if matches!(r.model, Model::Mount(_)) {
            // Ours: the 600-tick cap can end a ride before coff's fade in.
            if a.fade.is_none() && a.alpha < actmotion::OPAQUE {
                a.alpha = actmotion::OPAQUE;
            }
        }
    }
    // FUN_100d5ef0 (ours puts back what the mount hid, keeping an event's hides), each with its glow.
    if matches!(r.model, Model::Mount(_)) && w.actors.contains_key(&id) {
        for (n, was) in r.weps.into_iter().enumerate() {
            if was {
                super::wepfx::hide(w, id, n, 0);
            } else {
                super::wepfx::show(w, id, n, 0);
            }
        }
    }
}

/// Op 0x6e on a leaving animal: it runs along its facing.
pub fn self_move(w: &mut World, id: u32, ticks: f32, speed: f32) {
    if id & ANIMAL != 0
        && let Some(Riding { phase: Phase::Leaving { run, .. }, .. }) = w.rides.get_mut(&(id & !ANIMAL))
    {
        *run = Some((ticks, speed));
    }
}

/// A leaving animal's own place.
pub fn left_at(w: &World, animal: u32) -> Option<(Vec3, Vec3)> {
    match w.rides.get(&(animal & !ANIMAL))?.phase {
        Phase::Leaving { pos, forward, .. } => Some((pos, forward)),
        _ => None,
    }
}

/// Riders waiting hidden for their mount (+0xb2 bit 6).
pub fn hidden(w: &World) -> Vec<u32> {
    w.rides.iter().filter(|(_, r)| matches!(r.phase, Phase::Mounting(_))).map(|(id, _)| *id).collect()
}

/// FUN_100c63d0: a rider's load fade waits for its animal (ours: not for one whose model failed or is not asked for).
pub fn fade_held(w: &World, id: u32) -> bool {
    id & ANIMAL == 0
        && w.rides.get(&id).is_some_and(|r| !matches!(r.phase, Phase::Leaving { .. }))
        && w.actors.get(&(ANIMAL | id)).is_some_and(|a| a.character.is_none() && !a.failed)
}

/// FUN_100d65b0(0) while riding: a motion matching the idle name "chi " on letters 2..3 plays (`chi0`, `6hi0`, ...).
fn standing(w: &World, id: u32) -> bool {
    w.actors.get(&id).is_some_and(|a| stand_motion(&a.motion))
}

fn stand_motion(m: &str) -> bool {
    m.get(1..3) == Some("hi")
}

/// FUN_1008b460 holds the applied status (+0x170) at the riding one while it comes to a stand and `coff` runs.
pub fn applied(w: &World, id: u32, status: u8) -> u8 {
    match w.rides.get(&id) {
        Some(r) if matches!(r.phase, Phase::Leaving { .. } | Phase::Stopping) => {
            if matches!(r.model, Model::Mount(_)) {
                0x55
            } else {
                5
            }
        }
        _ => status,
    }
}

/// A riding rider's draw offset (+0x60c), zero otherwise.
pub fn offset(w: &World, id: u32) -> Vec3 {
    w.rides.get(&id).filter(|r| matches!(r.phase, Phase::Riding | Phase::Stopping)).map_or(Vec3::ZERO, |r| r.offset)
}

/// The linked mount's riding data.
fn linked(w: &World, id: u32) -> Option<(&Riding, crate::dat::character::MountInfo)> {
    let r = w.rides.get(&id).filter(|r| matches!(r.phase, Phase::Riding | Phase::Stopping))?;
    Some((r, w.actors.get(&(ANIMAL | id))?.character.as_ref()?.mount?))
}

/// FUN_100d4560's d for a point on bone 0 by the applied status: 0x55 by the linked mount's variant k (-1.3
/// unlinked or k 0 / 6, the Buffalo 0x1d and the Craklaw 0x25 by race, else -0.5), 5 -1.3, chairs 0x3f..0x53 -0.5.
/// `mount` = (k, mount id); `race` the rider's (FUN_10084420).
pub fn drop_for(status: u8, mount: Option<(u8, u8)>, race: u8) -> f32 {
    const BUFFALO: [f32; 9] = [0.0, 0.5, 0.5, 0.5, 1.1, -0.7, -0.7, 0.5, 0.05];
    const CRAKLAW: [f32; 9] = [0.0, 0.4, 0.4, 0.4, 0.4, -0.8, -0.8, 0.4, 0.4];
    let r = (race as usize).min(8);
    match (status, mount) {
        (0x55, Some((k, id))) if k != 0 && k != 6 => match id {
            0x1d => BUFFALO[r],
            0x25 => CRAKLAW[r],
            _ => -0.5,
        },
        (0x55 | 5, _) => -1.3,
        (0x3f..=0x53, _) => -0.5,
        _ => 0.0,
    }
}

/// The drop of actor `id` this frame (none for the animals, kind 0x54 / 1..4).
pub fn bone0_drop(w: &World, id: u32, status: u8) -> f32 {
    if id & ANIMAL != 0 {
        return 0.0;
    }
    let mount = linked(w, id).and_then(|(r, mi)| match r.model {
        Model::Mount(m) => Some((mi.variant(r.race), m)),
        _ => None,
    });
    let race = w.rides.get(&id).map_or(0, |r| r.race);
    drop_for(applied(w, id, status), mount, race)
}

/// FUN_1001ee60 riding: the height the camera looks at 0.6 of, the riding box (vtable +0x3c8) plus the linked mount's
/// camera add (info[14] / 100); None when not riding.
pub fn look_height(w: &World, id: u32, status: u8) -> Option<f32> {
    if !matches!(applied(w, id, status), 5 | 0x55) {
        return None;
    }
    let b = *w.actors.get(&id)?.character.as_ref()?.skeleton.boxes.get(1)?;
    Some(b[1] - b[0] + linked(w, id).map_or(0.0, |(_, mi)| mi.cam_add()))
}

/// The animal's size (FUN_10020ba0's first-person eye), 0 without one.
pub fn animal_scale(w: &World, id: u32) -> f32 {
    w.actors.get(&(ANIMAL | id)).filter(|a| a.character.is_some()).map_or(0.0, |a| a.scale)
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
    let r = rides.get(&rider).filter(|r| matches!(r.model, Model::Mount(_)) && r.phase == Phase::Riding)?;
    Some(if id & ANIMAL != 0 { (key(rider, 1), 0x2f + r.race.max(1) as usize) } else { (key(rider, 0), 0x6b) })
}

/// The points read back (relative to each actor's drawn root, world axes): offset = saddle - rider point.
pub fn points(w: &mut World, pts: Vec<(u64, Vec3)>) {
    for (k, p) in pts {
        if let Some(r) = w.rides.get_mut(&((k >> 8) as u32)).filter(|r| r.phase == Phase::Riding) {
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
    use super::drop_for;

    #[test]
    fn bone0_drops() {
        assert_eq!(drop_for(5, None, 1), -1.3);
        assert_eq!(drop_for(0x55, None, 1), -1.3);
        assert_eq!(drop_for(0x55, Some((6, 2)), 1), -1.3);
        assert_eq!(drop_for(0x55, Some((0, 34)), 1), -1.3);
        assert_eq!(drop_for(0x55, Some((3, 3)), 2), -0.5);
        assert_eq!(drop_for(0x55, Some((3, 0x1d)), 5), -0.7);
        assert_eq!(drop_for(0x55, Some((2, 0x1d)), 4), 1.1);
        assert_eq!(drop_for(0x55, Some((7, 0x1d)), 8), 0.05);
        assert_eq!(drop_for(0x55, Some((2, 0x25)), 8), 0.4);
        assert_eq!(drop_for(0x55, Some((3, 0x25)), 6), -0.8);
        assert_eq!((drop_for(0x3f, None, 1), drop_for(0x53, None, 1)), (-0.5, -0.5));
        assert_eq!((drop_for(0x54, None, 1), drop_for(0, Some((3, 3)), 1)), (0.0, 0.0));
    }

    /// FUN_100d65b0's riding test: letters 2..3 of the idle name "chi ".
    #[test]
    fn stand_motions() {
        for m in ["chi", "chi0", "6hi", "6hi0"] {
            assert!(super::stand_motion(m), "{m}");
        }
        for m in ["6un", "run", "idl", "chw", ""] {
            assert!(!super::stand_motion(m), "{m}");
        }
    }

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
