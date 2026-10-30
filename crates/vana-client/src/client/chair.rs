//! The chair under a /sitchair sitter (~/ffxi/lab/research/chairs_spec.md): the chair is the sitter's second actor
//! (entity +0xa4, kind status - 2, file 100507 + status - 0x3f; FUN_1008f740) at the sitter's place and facing
//! (FUN_100caf90), fading in by its own `init`. The sitter takes the chair motion files (tag 9, FUN_100d2bc0(4)), sits
//! down by `chi0` and stands by `chi2` on both (FUN_1008b460 states 0x17..0x1a), its weapons hidden while seated.

use std::time::{Duration, Instant};

use glam::Vec3;

use super::{EntityKind, Source, World};
use crate::dat::motionfile::PcMotion;

/// The chair's actor id: its sitter's with this bit. Never an entity, a target, a bump or a pick.
pub const CHAIR: u32 = 0x8000_0000;

/// The stand-up's busy hold (`chi2` op 0x1f, 186 ticks): the new status waits for it.
const RISE: Duration = Duration::from_millis(186 * 1000 / 60);

/// FUN_10095790: the chair statuses (/sitchair 0..20 = 63 + n).
pub fn is_chair(status: u8) -> bool {
    (0x3f..=0x53).contains(&status)
}

/// FUN_100c5280: the chair's model (13 in the install: chairs 0..12, statuses 0x3f..0x4b).
pub fn file(status: u8) -> Option<u32> {
    (0x3f..=0x4b).contains(&status).then(|| 100_507 + (status - 0x3f) as u32)
}

/// FUN_1008b460's chair states: 0x17 (sitting down), 0x18 (seated), 0x19 / 0x1a (standing up, until the hold ends).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Down,
    Seated,
    Rising(Instant),
}

/// A sitter's chair: its phase, the first chair status (a chair -> chair change keeps the first chair), and whether the
/// chair's `init` has run.
#[derive(Clone, Copy, Debug)]
pub struct Seat {
    phase: Phase,
    status: u8,
    init_done: bool,
}

fn status_of(w: &World, id: u32) -> u8 {
    if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) }
}

/// FUN_1008f740's gate: ActIndex <= 0x6ff and neither NpcPetFlag nor Flags3 bit 25 (every PC in practice).
fn eligible(w: &World, id: u32) -> bool {
    id == w.my_id || w.entities.get(&id).is_some_and(|e| e.kind == EntityKind::Player && e.index <= 0x6ff && !e.marks.npc_pet && !e.marks.b2b1)
}

/// FUN_100d2bc0(4): the chair motion files (`ci0` / `ci1` / `ci2`, `chi0` / `chi2`) while the sitter has a seat.
pub fn files(w: &World, id: u32, pc: &PcMotion) -> Vec<u32> {
    if w.seats.contains_key(&id) { super::ride::motion_files(pc, 4) } else { Vec::new() }
}

/// The stand-up holds the new status (FUN_1008e9a0 waits for the machine).
pub fn holding(w: &World, id: u32) -> bool {
    w.seats.get(&id).is_some_and(|s| matches!(s.phase, Phase::Rising(t) if Instant::now() < t))
}

/// A status change for a sitter: one that leaves the chair starts the stand-up (state 0x19) and is held until it
/// ends. True while it holds.
pub fn rise(w: &mut World, id: u32, status: u8) -> bool {
    let Some(seat) = w.seats.get(&id).copied() else { return false };
    if is_chair(status) {
        return false;
    }
    match seat.phase {
        Phase::Rising(t) => Instant::now() < t,
        _ => {
            start_rise(w, id);
            true
        }
    }
}

/// LAB_1008c2b5: `chi2` on the sitter and the chair, the weapons shown again (wep2 hidden).
pub fn start_rise(w: &mut World, id: u32) {
    let Some(seat) = w.seats.get_mut(&id) else { return };
    if matches!(seat.phase, Phase::Rising(_)) {
        return;
    }
    seat.phase = Phase::Rising(Instant::now() + RISE);
    let chi2 = u32::from_le_bytes(*b"chi2");
    w.effects.on_scheduler(id, id, chi2);
    if w.actors.get(&(CHAIR | id)).is_some_and(|a| a.character.is_some()) {
        w.effects.on_scheduler(CHAIR | id, CHAIR | id, chi2);
    }
    show_weapons(w, id);
}

/// FUN_100d5ef0(0) then FUN_100d60e0(2, 0).
fn show_weapons(w: &mut World, id: u32) {
    for n in 0..9 {
        super::wepfx::show(w, id, n, 0);
    }
    super::wepfx::hide(w, id, 2, 0);
}

/// FUN_100cf210 for a chair: height the sitter's size relative to medium; facing and side 1.0, Mithra 0.90 deep,
/// Galka 1.20 deep with its side at the relative size (its info[12] read signed); a sitter's event scale on all.
fn axes(w: &World, id: u32) -> Option<Vec3> {
    let a = w.actors.get(&id)?;
    if let Some(s) = a.event_scale {
        return Some(Vec3::splat(s));
    }
    let ch = a.character.as_ref()?;
    let rel = a.scale / ch.scale(1).max(1e-3);
    Some(match ch.pc.map(|pc| pc.race) {
        Some(7) => Vec3::new(0.90, rel, 1.0),
        Some(8) => Vec3::new(1.20, rel, rel),
        _ => Vec3::new(1.0, rel, 1.0),
    })
}

/// Per frame, before the motion files: seats for new sitters, each chair's model, `chi0` once the files are in, the
/// chair's `init` and scale; a finished stand-up removes the chair (ours then sends one more "/sitchair off").
pub fn update(w: &mut World) {
    let mut sitters: Vec<u32> = w.entities.iter().filter(|(id, e)| **id != w.my_id && is_chair(e.motion.status)).map(|(id, _)| *id).collect();
    if is_chair(w.my_motion.status) {
        sitters.push(w.my_id);
    }
    for id in sitters {
        if !w.seats.contains_key(&id) && eligible(w, id) {
            w.seats.insert(id, Seat { phase: Phase::Down, status: status_of(w, id), init_done: false });
        }
    }
    let now = Instant::now();
    let ids: Vec<u32> = w.seats.keys().copied().collect();
    for id in ids {
        let Some(mut seat) = w.seats.get(&id).copied() else { continue };
        let status = status_of(w, id);
        let chair = CHAIR | id;
        match seat.phase {
            // The hold over and the new status applied: the chair goes (FUN_10092870).
            Phase::Rising(t) if now >= t && !is_chair(status) => {
                remove(w, id);
                if id == w.my_id {
                    let mut b = vec![0u8; 8];
                    b[0..4].copy_from_slice(&2u32.to_le_bytes());
                    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x113, body: b });
                }
                continue;
            }
            // Refused (still seated a while after our own stand-up): seated again.
            Phase::Rising(t) if now >= t + Duration::from_secs(2) => {
                seat.phase = Phase::Seated;
                super::wepfx::hide_all(w, id);
            }
            Phase::Rising(_) => {}
            // Left the chair without a stand-up (zoning, a model rebuild, out of range).
            _ if !is_chair(status) => {
                show_weapons(w, id);
                remove(w, id);
                continue;
            }
            _ => {}
        }
        let Some(sitter) = w.actors.get(&id).filter(|a| a.character.as_ref().is_some_and(|c| c.pc.is_some())) else {
            w.seats.insert(id, seat);
            continue;
        };
        let files_in = {
            let pc = sitter.character.as_ref().and_then(|c| c.pc);
            pc.is_some_and(|pc| !sitter.ride_on.is_empty() && sitter.ride_on == files(w, id, &pc))
        };
        // The weapons stay hidden while seated (a model rebuilt meanwhile shows them: FUN_1008c480 hides them again).
        if !matches!(seat.phase, Phase::Rising(_)) && (0..9).any(|n| !sitter.wep_hide[n][0]) {
            super::wepfx::hide_all(w, id);
        }
        if let Some(f) = file(seat.status) {
            let src = Source::File(f);
            if w.actors.get(&chair).is_none_or(|a| a.key != format!("{src:?}")) && !w.event_looks.iter().any(|(i, _)| *i == chair) {
                w.event_looks.push((chair, src));
            }
        }
        // State 0x17: `chi0` once the chair files are in.
        if seat.phase == Phase::Down && files_in {
            w.effects.on_scheduler(id, id, u32::from_le_bytes(*b"chi0"));
            seat.phase = Phase::Seated;
        }
        let chair_in = w.actors.get(&chair).is_some_and(|a| a.character.is_some());
        if chair_in && !seat.init_done {
            w.effects.on_scheduler(chair, chair, u32::from_le_bytes(*b"init"));
            seat.init_done = true;
        }
        let ax = axes(w, id);
        if let Some(a) = w.actors.get_mut(&chair) {
            a.axes = ax;
        }
        w.seats.insert(id, seat);
    }
    let stray: Vec<u32> = w.actors.keys().filter(|id| **id & CHAIR != 0 && !w.seats.contains_key(&(**id & !CHAIR))).copied().collect();
    for id in stray {
        w.actors.remove(&id);
        w.effects.forget_actor(id);
    }
}

/// The seat and its chair actor go.
fn remove(w: &mut World, id: u32) {
    w.seats.remove(&id);
    w.actors.remove(&(CHAIR | id));
    w.effects.forget_actor(CHAIR | id);
}

/// /sitchair (FUN_1007c390): mode "on" 1 / "off" 2 / else 0 (toggle) with chair = the number, 0 outside 0..20 (on and
/// off send 0). None: nothing is sent (sitting down; already seated for "on"; not seated for "off"; standing in
/// another status). A toggle or "off" while seated starts our stand-up first (state 0x19) and is sent as "off"
/// *(not retail: retail sends the toggle {0, n}, which LSB takes as a move to chair n, not a stand-up)*.
pub fn command(w: &mut World, arg: Option<&str>) -> Option<(u32, u32)> {
    let (mode, chair) = match arg {
        Some("on") => (1, 0),
        Some("off") => (2, 0),
        a => (0, a.and_then(|a| a.parse::<u32>().ok()).filter(|n| *n <= 20).unwrap_or(0)),
    };
    let me = w.my_id;
    let seated = is_chair(w.my_motion.status);
    let phase = w.seats.get(&me).map(|s| s.phase);
    if phase == Some(Phase::Down) || (!seated && w.my_motion.status != 0) {
        return None;
    }
    match (mode, seated) {
        (1, true) | (2, false) => None,
        (0 | 2, true) if phase == Some(Phase::Seated) => {
            start_rise(w, me);
            Some((2, 0))
        }
        (m, true) if m != 2 && matches!(phase, Some(Phase::Rising(_))) => None,
        _ => Some((mode, chair)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chair_files() {
        assert_eq!(file(0x3f), Some(100_507));
        assert_eq!(file(0x4b), Some(100_519));
        assert_eq!(file(0x4c), None);
        assert!(is_chair(0x53) && !is_chair(0x54) && !is_chair(0x2f));
    }
}
