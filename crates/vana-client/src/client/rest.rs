//! Resting's sit-down and stand-up (FUN_1008b460 states 7 / 8; MODLOG "/heal: sitting down and standing up"):
//! `res0` once the model is in, `res2` when the status leaves healing, its op 0x2e holding the walk.

use std::time::{Duration, Instant};

use super::World;

/// `res2`'s op 0x2e: the stand-up's move lock, 56 ticks.
const RISE: Duration = Duration::from_millis(56 * 1000 / 60);

/// The machine's states for a resting actor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// State 7: healing applied, `res0` not yet started (it plays even if healing has ended since).
    Down,
    /// State 8: `res0` started; leaving healing plays `res2`.
    Resting,
    /// `res2` playing, until its move lock ends (ours: op 0x2e's lock as a time).
    Rising(Instant),
}

fn healing(status: u8) -> bool {
    matches!(status, 0x21 | 7)
}

fn status_of(w: &World, id: u32) -> u8 {
    if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) }
}

/// The scheduler `name` from the actor's files, its motions here and the rest (holds, locks) in fx. False when the
/// actor has no model or no such scheduler yet.
fn play(w: &mut World, id: u32, name: &str) -> bool {
    let Some(ch) = w.actors.get(&id).and_then(|a| a.character.clone()) else { return false };
    let base = super::actmotion::status_motion(w, id, &ch);
    let Some(a) = w.actors.get_mut(&id) else { return false };
    if !super::actmotion::start(a, &ch, name, &base) {
        return false;
    }
    super::actmotion::fx_too(&mut w.effects, id, name);
    true
}

/// FUN_100d12d0 and the `waot` check (vt+0x2a0): the actor's model is in and no `waot` waits or runs on it.
fn ready(w: &World, id: u32) -> bool {
    let waot = u32::from_le_bytes(*b"waot");
    w.actors.get(&id).is_some_and(|a| a.character.is_some() && !a.play.as_ref().is_some_and(|p| p.name == "waot" && p.busy()))
        && !w.effects.actor_scheduler_running(id, waot)
}

/// What the machine does next.
#[derive(Debug, PartialEq)]
enum Step {
    /// Start the scheduler, then take the phase.
    Play(&'static str, Phase),
    Stay(Phase),
    End,
}

/// FUN_1008b460's states 7 / 8 for an applied status `healing` or not. State 7 does not test the status.
fn transition(phase: Phase, healing: bool, now: Instant) -> Step {
    match phase {
        Phase::Down => Step::Play("res0", Phase::Resting),
        Phase::Resting if !healing => Step::Play("res2", Phase::Rising(now + RISE)),
        Phase::Resting => Step::Stay(Phase::Resting),
        Phase::Rising(t) if now < t => Step::Stay(phase),
        Phase::Rising(_) => Step::End,
    }
}

/// Per frame, after the statuses are applied and before the motions: new rests, `res0` once ready, `res2` on leaving.
pub fn update(w: &mut World) {
    let now = Instant::now();
    let mut ids: Vec<u32> = w.entities.iter().filter(|(id, e)| **id != w.my_id && healing(e.motion.status)).map(|(id, _)| *id).collect();
    if healing(w.my_motion.status) {
        ids.push(w.my_id);
    }
    for id in ids {
        if !matches!(w.rests.get(&id), Some(Phase::Down | Phase::Resting)) {
            w.rests.insert(id, Phase::Down);
        }
    }
    let ids: Vec<u32> = w.rests.keys().copied().collect();
    for id in ids {
        let gone = id != w.my_id && !w.entities.contains_key(&id);
        let status = status_of(w, id);
        let phase = w.rests[&id];
        let next = match transition(phase, healing(status), now) {
            _ if gone => None,
            // `res2` goes at once and holds the walk only when it played; `res0` waits for the model and `waot`.
            Step::Play("res2", then) => play(w, id, "res2").then_some(then),
            Step::Play(name, then) => Some(if ready(w, id) && play(w, id, name) { then } else { phase }),
            Step::Stay(p) => Some(p),
            Step::End => None,
        };
        match next {
            Some(p) => w.rests.insert(id, p),
            None => w.rests.remove(&id),
        };
    }
}

/// Our own stand-up still holds the walk (`res2`'s op 0x2e).
pub fn rising(w: &World) -> bool {
    matches!(w.rests.get(&w.my_id), Some(Phase::Rising(t)) if Instant::now() < *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sit_rest_rise() {
        let now = Instant::now();
        assert_eq!(transition(Phase::Down, true, now), Step::Play("res0", Phase::Resting));
        assert_eq!(transition(Phase::Down, false, now), Step::Play("res0", Phase::Resting));
        assert_eq!(transition(Phase::Resting, true, now), Step::Stay(Phase::Resting));
        assert_eq!(transition(Phase::Resting, false, now), Step::Play("res2", Phase::Rising(now + RISE)));
        // The walk stays locked for res2's 56 ticks, then the machine is idle.
        let up = Phase::Rising(now + RISE);
        assert_eq!(transition(up, false, now + RISE / 2), Step::Stay(up));
        assert_eq!(transition(up, false, now + RISE), Step::End);
    }
}
