//! The angler's model (fishing_spec.md 2.1 / 2.6): fishing motion files while at a fishing status (FUN_100d2a10), the
//! status's scheduler on the model and the rod (FUN_1008c6c0), and the rod as its own actor (entity +0xa4,
//! FUN_1008f740) at the angler's place, playing the angler's status motion and fishing schedulers.

use super::fishing::fishing_status;
use super::{EntityKind, Source, World};
use crate::dat::motionfile::PcMotion;

/// The rod's actor id: its angler's with this bit. Never an entity, a target, a bump or a pick.
pub const ROD: u32 = 0x1000_0000;

/// DAT_1035f598 by race: the rod files, + the range look.
const ROD_FILES: [u32; 9] = [39307, 39307, 39339, 39371, 39403, 39435, 39435, 39467, 39499];

/// FUN_1008c6c0's schedulers for 0x38..0x3e.
const STATUS_SCHED: [&[u8; 4]; 7] = [b"fsh0", b"fsh7", b"fsh2", b"fsh3", b"fsh4", b"fsh5", b"fsh6"];

/// FUN_100cae60's table 0x10330ef8 for 0x38..0x3e: the rod's line scheduler (1 `!f01`, 2 `!f02`) and its delay.
const LINE: [Option<(u8, f32)>; 7] = [Some((1, 136.0)), None, Some((2, 102.0)), Some((2, 40.0)), Some((2, 44.0)), Some((2, 68.0)), Some((2, 0.0))];

/// The status whose scheduler the model and the rod have had; the line's pending scheduler and ticks left (+0x81c /
/// +0x820) for the status it was set by.
#[derive(Default)]
pub struct Angler {
    model: u8,
    rod: u8,
    line: Option<(u8, f32)>,
    line_for: u8,
}

/// The angler's rod actor, once loaded.
pub fn rod_of(w: &World, id: u32) -> Option<u32> {
    let rod = ROD | id;
    w.actors.get(&rod).filter(|a| a.character.is_some()).map(|_| rod)
}

fn status_of(w: &World, id: u32) -> u8 {
    if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) }
}

fn range_look(w: &World, id: u32) -> u16 {
    match w.actors.get(&id).and_then(|a| a.source.as_ref()) {
        Some(Source::Pc(l)) => l.range,
        _ => 0,
    }
}

/// FUN_100d2a10(v): the fishing files (tag 8), the riding tables + 1 (+ 2 with the Goldfish Basket, look 0xc).
pub fn files(w: &World, id: u32, pc: &PcMotion) -> Vec<u32> {
    if !fishing_status(status_of(w, id)) {
        return Vec::new();
    }
    let v = if range_look(w, id) == 0xc { 2 } else { 1 };
    super::ride::motion_files(pc, v)
}

/// FUN_1008c6c0: a status 6 / 0x32 / 0x38 waits while the model still walks (FUN_100d65b0(1)) and is retried
/// *(inf: walking = its walk or run motion)*.
pub fn walking_hold(w: &World, id: u32, status: u8) -> bool {
    if !matches!(status, 6 | 0x32 | 0x38) {
        return false;
    }
    let Some((a, ch)) = w.actors.get(&id).and_then(|a| Some((a, a.character.as_ref()?))) else { return false };
    let names = a.motion_names.unwrap_or(ch.motions);
    a.motion == names.walk || a.motion == names.run
}

/// Per frame, before the motion files: each angler's rod made, its status scheduler once its files are in, the rod's
/// line; the schedulers stopped and the rod gone when it stops fishing.
pub fn update(w: &mut World, dt: f32) {
    let mut anglers: Vec<u32> = w.entities.iter().filter(|(id, e)| **id != w.my_id && e.kind == EntityKind::Player && fishing_status(e.motion.status)).map(|(id, _)| *id).collect();
    if fishing_status(w.my_motion.status) {
        anglers.push(w.my_id);
    }
    for &id in &anglers {
        let status = status_of(w, id);
        let Some((race, scale, files_in)) = w.actors.get(&id).and_then(|a| {
            let pc = a.character.as_ref()?.pc?;
            Some((pc.race, a.scale, !a.ride_on.is_empty() && a.ride_on == files(w, id, &pc)))
        }) else {
            continue;
        };
        // FUN_100c59b0: kind = look + 5 (5 from look 0x55), its file DAT_1035f598[race] + kind - 5.
        let look = range_look(w, id);
        let kind = if look >= 0x55 { 5 } else { look as u32 + 5 };
        let src = Source::File(ROD_FILES[(race as usize).min(8)] + kind - 5);
        let rod = ROD | id;
        if w.actors.get(&rod).is_none_or(|a| a.key != format!("{src:?}")) && !w.event_looks.iter().any(|(i, _)| *i == rod) {
            w.event_looks.push((rod, src));
        }
        // The rod at the angler's size *(inf)*.
        if let Some(a) = w.actors.get_mut(&rod) {
            a.event_scale = Some(scale);
        }
        let k = (status - 0x38) as usize;
        let name = u32::from_le_bytes(*STATUS_SCHED[k]);
        let rod_in = rod_of(w, id).is_some();
        let a = w.anglers.entry(id).or_default();
        if a.line_for != status {
            (a.line_for, a.line) = (status, LINE[k]);
        }
        // Only the rods of looks 14 / 15 have `!f01` / `!f02` *(inf: the others draw no line effect)*.
        let mut line = None;
        if let Some((mode, left)) = a.line.as_mut() {
            *left -= dt * 60.0;
            if *left <= 0.0 && rod_in {
                line = matches!(look, 14 | 15).then_some(*mode);
                a.line = None;
            }
        }
        let (model, rod_due) = (files_in && a.model != status, rod_in && a.rod != status);
        if model {
            a.model = status;
        }
        if rod_due {
            a.rod = status;
        }
        if model {
            w.effects.on_scheduler(id, id, name);
        }
        if rod_due {
            w.effects.on_scheduler(rod, rod, name);
        }
        if let Some(mode) = line {
            w.effects.on_scheduler(rod, rod, u32::from_le_bytes(if mode == 1 { *b"!f01" } else { *b"!f02" }));
        }
    }
    let gone: Vec<u32> = w.anglers.keys().filter(|id| !anglers.contains(id)).copied().collect();
    for id in gone {
        w.anglers.remove(&id);
        for s in [b"fsh0", b"fsh1", b"fsh2", b"fsh3", b"fsh4", b"fsh5", b"fsh6", b"fsh7", b"fsh8", b"fsh9"] {
            w.effects.stop_actor_scheduler(id, u32::from_le_bytes(*s));
        }
    }
    let stray: Vec<u32> = w.actors.keys().filter(|id| **id & ROD != 0 && !anglers.contains(&(**id & !ROD))).copied().collect();
    for id in stray {
        w.actors.remove(&id);
        w.effects.forget_actor(id);
    }
}
