//! Bumping into characters (FUN_100a8770, collision_chars_spec): the local player's step into the nearest eligible
//! character it overlaps is held for 30 ticks, then passes through while it still overlaps; a frame without contact
//! (letting go, backing off) clears it. No slide, no push. Other players count only while active (FUN_100a57f0).

use super::{Actor, Entity, World};
use glam::Vec3;

/// +0x5a0 / +0x5ac / +0x5b0: the actor held against, ticks left, still holding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    blocker: Option<u32>,
    left: i16,
    holding: bool,
}

/// FUN_100a8770's tail for the nearest eligible actor and its gap: true while the step is held.
pub fn hold(s: &mut State, hit: Option<(u32, f32)>, ticks: f32) -> bool {
    let Some((id, gap)) = hit else { return false };
    if gap >= 0.0 {
        *s = State::default();
        return false;
    }
    if s.blocker != Some(id) {
        *s = State { blocker: Some(id), left: 30, holding: true };
        return true;
    }
    if !s.holding {
        return false;
    }
    s.left -= ticks.max(1.0) as i16;
    if s.left >= 0 {
        return true;
    }
    s.holding = false;
    false
}

/// FUN_100aaef0: 0.8 x the horizontal offset of skeleton point 44 (bone 0 + (0, 0, -r) for most: Hume 0.32, Taru
/// 0.24, Galka 0.64), in the rest pose *(inf: retail reads the current pose)*; 0 without a skeleton or the point.
pub fn radius(a: &Actor) -> f32 {
    let Some(ch) = a.character.as_ref() else { return 0.0 };
    let Some(&(bone, off)) = ch.skeleton.points.get(44) else { return 0.0 };
    let p = a.rest.get(bone as usize).map_or(off, |&(q, t)| t + q * off) * a.scale;
    0.8 * p.x.hypot(p.z)
}

/// FUN_100a57f0: a character is active for 1800 ticks after moving more than 0.02 a frame (0.01 a tick) or while its
/// status is not idle, /heal, /sit or a chair.
pub fn active_tick(e: &mut Entity, ticks: f32) {
    let moved = (e.pos - e.last_pos).length() > 0.01 * ticks;
    if moved || !matches!(e.motion.status, 0 | 0x21 | 0x2f | 0x3f..=0x53) {
        e.active = 1800.0;
    }
    e.active = (e.active - ticks.max(1.0)).max(0.0);
}

/// FUN_100a8770's tests, in order: the main body of a character (no door, lift or ship), not made passable by event
/// op 0xab 0x0b (not done), drawn and on screen last frame, not dead, not a SubKind 0 marker model (50..59), faded in,
/// and a player only while active.
fn eligible(w: &World, id: u32, e: &Entity) -> bool {
    id != w.my_id
        && !e.no_bump
        && e.door == 0
        && e.lift.is_none()
        && !matches!(e.sub_kind, 2..=4)
        && w.visible.contains(&id)
        && !matches!(e.motion.status, 2 | 3)
        && !(e.sub_kind == 0 && (50..=59).contains(&e.model))
        && w.actors.get(&id).is_some_and(|a| a.alpha >= super::actmotion::OPAQUE)
        && (id >> 24 != 0 || e.active > 0.0)
}

/// One player step from `from` by `delta` (also a zero one: standing still clears the hold): true when it is held.
/// Off at GM level 3 and up (0x037 @0x28 bits 29..31).
pub fn blocks(w: &mut World, from: Vec3, delta: Vec3, ticks: f32) -> bool {
    if (w.self_flags >> 29) & 7 >= 3 {
        return false;
    }
    let dest = from + delta;
    let mut best = 64.0;
    let mut hit = None;
    for (id, e) in &w.entities {
        if !eligible(w, *id, e) {
            continue;
        }
        let d2 = (e.pos - dest).length_squared();
        if d2 < best {
            (best, hit) = (d2, Some(*id));
        }
    }
    let r = |id: u32| w.actors.get(&id).map_or(0.0, radius);
    let hit = hit.map(|id| (id, best.sqrt() - r(w.my_id) - r(id)));
    let held = hold(&mut w.bump, hit, ticks);
    if held && std::env::var_os("FFXI_TRACE_MOVE").is_some() {
        log::info!("bump {:?} gap {:.3} left {} held", hit.map(|h| h.0), hit.map_or(0.0, |h| h.1), w.bump.left);
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 30 ticks held at 2 a step (16 steps), then through while overlapping; a gap clears it; a new actor holds again;
    /// no candidate leaves the state alone.
    #[test]
    fn hold_then_pass() {
        let mut s = State::default();
        let held: Vec<bool> = (0..18).map(|_| hold(&mut s, Some((7, -0.1)), 2.0)).collect();
        assert_eq!(held.iter().filter(|h| **h).count(), 16, "{held:?}");
        assert!(!held[16] && !held[17]);
        assert!(!hold(&mut s, None, 2.0));
        assert!(!s.holding && s.blocker == Some(7));
        assert!(!hold(&mut s, Some((7, 0.2)), 2.0));
        assert_eq!(s, State::default());
        assert!(hold(&mut s, Some((7, -0.1)), 2.0));
        assert!(hold(&mut s, Some((8, -0.1)), 2.0));
        assert_eq!(s.left, 30);
    }
}
