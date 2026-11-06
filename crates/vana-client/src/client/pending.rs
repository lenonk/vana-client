//! Pending actions (not retail; the plan's "state changes are server-confirmed" and its open question "how latency,
//! failed transfers and invalid targets are shown"): a spell, weapon skill, job ability or item we asked the server
//! for (c2s 0x01A / 0x037) is pending until the server answers. An action packet of ours (0x028) of its kind confirms
//! it; a battle message of ours (0x029) soon after the ask and before that is its refusal, kept with the message's
//! text a few seconds; no answer for a while says so. The modern layer shows it (modern/pending.rs chip);
//! retail's lines print as ever.

use super::World;
use std::time::{Duration, Instant};

/// What was asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    Spell(u16),
    WeaponSkill(u16),
    /// The ability's name-table id (the 0x01A param + 0x200).
    Ability(u16),
    Item(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Waiting,
    /// The server's refusal (its line, plain).
    Refused(String),
    /// Nothing came back.
    NoReply,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub ask: Ask,
    /// On whom (0: none named).
    pub target: u32,
    pub at: Instant,
    pub state: State,
    /// When the state last changed.
    pub since: Instant,
}

/// The chip shows only past this (a quick answer never flashes it).
pub const SHOW_AFTER: Duration = Duration::from_millis(150);
/// No answer within this is NoReply.
const TIMEOUT: Duration = Duration::from_secs(5);
/// A refusal comes at once (the server answers a refused ask as it reads it); a message of ours later is something
/// else (an effect wearing off ...). Inferred, with room for lag.
const REFUSED_WITHIN: Duration = Duration::from_secs(2);
/// A refusal or NoReply stays this long.
const KEEP: Duration = Duration::from_secs(4);

/// Battle messages of ours that are not answers (experience, limit and capacity points, a defeat, a level).
const NOT_ANSWERS: [u16; 8] = [6, 8, 105, 253, 371, 372, 718, 735];

/// An action asked for.
pub fn start(w: &mut World, ask: Ask, target: u32) {
    let now = Instant::now();
    w.asked = Some(Pending { ask, target, at: now, state: State::Waiting, since: now });
}

/// Whether a 0x028 category of ours answers `ask`: its start or its finish (a melee round or another kind does not).
fn answers(ask: Ask, category: u8) -> bool {
    match ask {
        Ask::Spell(_) => matches!(category, 4 | 8),
        // Monster skills (Monstrosity) finish as category 11.
        Ask::WeaponSkill(_) => matches!(category, 3 | 7 | 11),
        Ask::Ability(_) => matches!(category, 6 | 14 | 15),
        Ask::Item(_) => matches!(category, 5 | 9),
    }
}

/// 0x028 of ours: the action waiting is confirmed by one of its kind (the cast bar or the effect takes over).
pub fn confirm(w: &mut World, category: u8) {
    if w.asked.as_ref().is_some_and(|p| p.state == State::Waiting && answers(p.ask, category)) {
        w.asked = None;
    }
}

/// 0x029 of ours soon after the ask, while waiting: its line is the refusal.
pub fn message(w: &mut World, id: u16, line: &str) {
    if NOT_ANSWERS.contains(&id) {
        return;
    }
    if let Some(p) = w.asked.as_mut().filter(|p| p.state == State::Waiting && p.at.elapsed() < REFUSED_WITHIN) {
        p.state = State::Refused(crate::dat::font::strip_codes(line).trim().to_string());
        p.since = Instant::now();
    }
}

/// Per frame: the wait runs out, an answer shown long enough goes.
pub fn update(w: &mut World) {
    let Some(p) = w.asked.as_mut() else { return };
    match p.state {
        State::Waiting if p.at.elapsed() > TIMEOUT => {
            p.state = State::NoReply;
            p.since = Instant::now();
        }
        State::Refused(_) | State::NoReply if p.since.elapsed() > KEEP => w.asked = None,
        _ => {}
    }
}

/// The action's name.
pub fn name(w: &World, ask: Ask) -> String {
    match ask {
        Ask::Spell(id) => super::magic::spell_name(w, id),
        Ask::WeaponSkill(id) | Ask::Ability(id) => super::magic::ability_name(w, id),
        Ask::Item(id) => w.item_info.as_ref().and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Ask, NOT_ANSWERS, answers};

    #[test]
    fn experience_is_not_an_answer() {
        assert!(NOT_ANSWERS.contains(&8) && NOT_ANSWERS.contains(&105));
    }

    #[test]
    fn only_its_kind_confirms() {
        assert!(answers(Ask::Spell(1), 8) && answers(Ask::Spell(1), 4));
        assert!(!answers(Ask::Spell(1), 1) && !answers(Ask::Spell(1), 6));
        assert!(answers(Ask::Item(1), 9) && !answers(Ask::Item(1), 2));
    }
}
