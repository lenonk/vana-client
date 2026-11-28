//! Outstanding player actions (c2s 0x01A / 0x037) until the server answers — the same rules as the Pending Actions mod.

use crate::net::action::Action;
use std::time::{Duration, Instant};

const SHOW_AFTER: Duration = Duration::from_millis(150);
const TIMEOUT: Duration = Duration::from_secs(5);
const REFUSED_WITHIN: Duration = Duration::from_secs(2);
const KEEP: Duration = Duration::from_secs(4);
const BLOCKING_MSG: u16 = 225;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Spell,
    Ws,
    Ability,
    Item,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AskState {
    /// Ask sent; waiting for the server to accept it (start packet or instant finish).
    Waiting,
    /// Server accepted the action (cast / skill start); waiting for finish or interrupt.
    Casting,
    Refused,
    Quiet,
}

/// How the last ask ended (for `actions.when_done` after the ask is gone).
#[derive(Clone, Debug)]
pub enum LastResult {
    Answered {
        kind: Kind,
        id: u32,
        target: u32,
    },
    Refused {
        kind: Kind,
        id: u32,
        target: u32,
        why: String,
    },
    Quiet {
        kind: Kind,
        id: u32,
        target: u32,
    },
    Interrupted {
        kind: Kind,
        id: u32,
        target: u32,
    },
    /// Another ask superseded this one before it resolved.
    Replaced {
        kind: Kind,
        id: u32,
        target: u32,
    },
}

#[derive(Clone, Debug)]
struct Ask {
    serial: u64,
    kind: Kind,
    id: u32,
    target: u32,
    at: Instant,
    state: AskState,
    since: Instant,
    why: String,
}

#[derive(Default)]
pub struct Tracker {
    ask: Option<Ask>,
    last: Option<LastResult>,
    /// Set when an ask leaves `Waiting` (answered, refused, timed out); cleared when waiters run.
    pub resolve: bool,
    /// Generation counter; each `start` assigns `Ask::gen` from a new value.
    next_gen: u64,
    /// `LastResult` that triggered `resolve` (for `when_done` matching).
    resolved_gen: u64,
}

impl Tracker {
    pub fn is_waiting(&self) -> bool {
        self.ask.as_ref().is_some_and(|a| matches!(a.state, AskState::Waiting | AskState::Casting))
    }

    /// For `game.action_ready`: no cast or event timer blocking command input, and no outstanding ask.
    pub fn action_ready(w: &super::World) -> bool {
        if w.pending_action.is_waiting() {
            return false;
        }
        if w.cast_bar.is_some() || w.event_timer.is_some() {
            return false;
        }
        if w.event.is_some() || super::event::active(w) {
            return false;
        }
        true
    }

    pub fn waiting_gen(&self) -> Option<u64> {
        self.ask.as_ref().filter(|a| matches!(a.state, AskState::Waiting | AskState::Casting)).map(|a| a.serial)
    }

    pub fn resolved_generation(&self) -> Option<u64> {
        self.resolve.then_some(self.resolved_gen)
    }

    pub fn start(&mut self, kind: Kind, id: u32, target: u32) {
        let now = Instant::now();
        let replaced = self.ask.take().filter(|a| matches!(a.state, AskState::Waiting | AskState::Casting));
        if let Some(old) = replaced {
            self.resolved_gen = old.serial;
            self.last = Some(LastResult::Replaced { kind: old.kind, id: old.id, target: old.target });
            self.resolve = true;
        } else {
            self.last = None;
            self.resolve = false;
        }
        self.next_gen = self.next_gen.wrapping_add(1);
        self.ask = Some(Ask {
            serial: self.next_gen,
            kind,
            id,
            target,
            at: now,
            state: AskState::Waiting,
            since: now,
            why: String::new(),
        });
    }

    pub fn tick(&mut self, now: Instant) {
        let Some(a) = &mut self.ask else { return };
        if matches!(a.state, AskState::Waiting | AskState::Casting) && now.duration_since(a.at) > TIMEOUT {
            let snap = a.clone();
            a.state = AskState::Quiet;
            a.since = now;
            self.resolved_gen = snap.serial;
            self.last = Some(LastResult::Quiet { kind: snap.kind, id: snap.id, target: snap.target });
            self.resolve = true;
        } else if a.state != AskState::Waiting && now.duration_since(a.since) > KEEP {
            self.ask = None;
        }
    }

    pub fn on_action(&mut self, my_id: u32, a: &Action) {
        if a.actor != my_id {
            return;
        }
        let Some(ask) = &self.ask else { return };
        match ask.state {
            AskState::Waiting | AskState::Casting if matches_interrupt(ask, a) => {
                let snap = self.ask.take().unwrap();
                self.resolved_gen = snap.serial;
                self.last = Some(LastResult::Interrupted { kind: snap.kind, id: snap.id, target: snap.target });
                self.resolve = true;
            }
            AskState::Waiting => {
                if ask.kind == Kind::Ws && a.category == 11 {
                    self.resolve_answered();
                } else if matches_start(ask, a) {
                    if matches!(ask.kind, Kind::Spell | Kind::Ws) {
                        if let Some(ask) = self.ask.as_mut() {
                            ask.state = AskState::Casting;
                            ask.since = Instant::now();
                        }
                    } else {
                        self.resolve_answered();
                    }
                } else if matches_finish(ask, a) {
                    self.resolve_answered();
                }
            }
            AskState::Casting if matches_finish(ask, a) => {
                self.resolve_answered();
            }
            _ => {}
        }
    }

    fn resolve_answered(&mut self) {
        let snap = self.ask.take().unwrap();
        self.resolved_gen = snap.serial;
        self.last = Some(LastResult::Answered { kind: snap.kind, id: snap.id, target: snap.target });
        self.resolve = true;
    }

    pub fn on_battle_message(&mut self, my_id: u32, actor: u32, msg: u16, p1: u32, why: String) {
        let Some(ask) = &mut self.ask else { return };
        if ask.state != AskState::Waiting
            || actor != my_id
            || Instant::now().duration_since(ask.at) >= REFUSED_WITHIN
            || !refusal(ask, msg, p1)
        {
            return;
        }
        ask.state = AskState::Refused;
        ask.since = Instant::now();
        ask.why = why;
        self.resolved_gen = ask.serial;
        self.last = Some(LastResult::Refused { kind: ask.kind, id: ask.id, target: ask.target, why: ask.why.clone() });
        self.resolve = true;
    }

    pub fn on_system_message(&mut self, id: u16) {
        let Some(ask) = &mut self.ask else { return };
        if ask.state != AskState::Waiting
            || ask.kind == Kind::Ws
            || id != BLOCKING_MSG
            || Instant::now().duration_since(ask.at) >= REFUSED_WITHIN
        {
            return;
        }
        ask.state = AskState::Refused;
        ask.since = Instant::now();
        ask.why.clear();
        self.resolved_gen = ask.serial;
        self.last = Some(LastResult::Refused { kind: ask.kind, id: ask.id, target: ask.target, why: String::new() });
        self.resolve = true;
    }

    /// Snapshot for `game.pending()` (nil while waiting inside SHOW_AFTER, like the chip).
    pub fn view(&self, now: Instant) -> Option<PendingView<'_>> {
        let a = self.ask.as_ref()?;
        if a.state == AskState::Waiting && now.duration_since(a.at) < SHOW_AFTER {
            return None;
        }
        Some(PendingView {
            kind: a.kind,
            id: a.id,
            target: a.target,
            state: a.state,
            why: a.why.as_str(),
            waiting: matches!(a.state, AskState::Waiting | AskState::Casting),
            age: now.duration_since(a.at).as_secs_f64(),
        })
    }

    pub fn result_table(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        let t = lua.create_table()?;
        if self.resolve {
            if let Some(r) = &self.last {
                fill_last(lua, &t, r)?;
                return Ok(t);
            }
        }
        if let Some(a) = &self.ask {
            fill_pending(lua, &t, a)?;
            return Ok(t);
        }
        t.set("state", "idle")?;
        Ok(t)
    }

    pub fn clear_resolve(&mut self) {
        self.resolve = false;
    }

    /// Callback argument when nothing was waiting at `actions.when_done` registration.
    pub fn idle_table(lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        let t = lua.create_table()?;
        t.set("state", "idle")?;
        Ok(t)
    }
}

pub struct PendingView<'a> {
    pub kind: Kind,
    pub id: u32,
    pub target: u32,
    pub state: AskState,
    pub why: &'a str,
    pub waiting: bool,
    pub age: f64,
}

pub fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Spell => "spell",
        Kind::Ws => "ws",
        Kind::Ability => "ability",
        Kind::Item => "item",
    }
}

pub fn state_str(s: AskState) -> &'static str {
    match s {
        AskState::Waiting => "waiting",
        AskState::Casting => "casting",
        AskState::Refused => "refused",
        AskState::Quiet => "quiet",
    }
}

pub fn on_out_01a(w: &mut super::World, target: u32, category: u16, param: u32) {
    let kind = match category {
        3 => Some(Kind::Spell),
        7 => Some(Kind::Ws),
        9 => Some(Kind::Ability),
        _ => None,
    };
    let Some(kind) = kind else { return };
    let id = if kind == Kind::Ability { param.wrapping_add(0x200) } else { param };
    w.pending_action.start(kind, id, target);
}

pub fn on_out_item(w: &mut super::World, item_id: u32, target: u32) {
    w.pending_action.start(Kind::Item, item_id, target);
}

/// Battle-message text for a refusal (same expansion as `res.message`).
pub(super) fn refusal_text(w: &super::World, actor: u32, target: u32, msg: u16, p1: u32, p2: u32) -> String {
    let Some(bt) = w.battle.as_ref() else { return String::new() };
    let ctx = crate::dat::battle::Ctx {
        actor: super::combat::who(w, actor),
        target: super::combat::who(w, target),
        work: [p1 as i32, p2 as i32, 0, 0, 0],
        ..super::combat::ctx(w)
    };
    let Some(line) = bt.expand(msg, &ctx) else { return String::new() };
    let text = line.lines.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    text.trim().to_string()
}

fn packet_id(ask: &Ask) -> u32 {
    if ask.kind == Kind::Ability { ask.id.wrapping_sub(0x200) } else { ask.id }
}

fn action_id(a: &Action) -> Option<u16> {
    const FINISH: &[u8] = &[3, 4, 5, 6, 11, 13, 14, 15];
    const START: &[u8] = &[7, 8, 9, 10, 12];
    if FINISH.contains(&a.category) {
        return Some((a.arg & 0xffff) as u16);
    }
    if !START.contains(&a.category) {
        return None;
    }
    if matches!(a.category, 7 | 8) {
        let t = a.targets.first()?;
        let r = t.results.first()?;
        return Some((r.value & 0xffff) as u16);
    }
    if (a.arg & 0xffff) == 0x6163 {
        let t = a.targets.first()?;
        let r = t.results.first()?;
        return Some((r.value & 0xffff) as u16);
    }
    None
}

fn is_ca_start(a: &Action) -> bool {
    const START: &[u8] = &[7, 8, 9, 10, 12];
    START.contains(&a.category) && (a.arg & 0xffff) == 0x6163
}

fn packet_matches(ask: &Ask, a: &Action) -> bool {
    if ask.kind == Kind::Item || (ask.kind == Kind::Ws && a.category == 11) {
        return true;
    }
    action_id(a).is_some_and(|id| u32::from(id) == packet_id(ask))
}

fn matches_start(ask: &Ask, a: &Action) -> bool {
    match ask.kind {
        Kind::Spell => a.category == 8 && is_ca_start(a) && packet_matches(ask, a),
        Kind::Ws => a.category == 7 && is_ca_start(a) && packet_matches(ask, a),
        Kind::Item => a.category == 9 && packet_matches(ask, a),
        Kind::Ability => false,
    }
}

fn matches_finish(ask: &Ask, a: &Action) -> bool {
    match ask.kind {
        Kind::Spell => a.category == 4 && packet_matches(ask, a),
        Kind::Ws => a.category == 3 && packet_matches(ask, a),
        Kind::Ability => matches!(a.category, 6 | 14 | 15) && packet_matches(ask, a),
        Kind::Item => a.category == 5 && packet_matches(ask, a),
    }
}

fn matches_interrupt(ask: &Ask, a: &Action) -> bool {
    if ask.state != AskState::Casting {
        return false;
    }
    match ask.kind {
        Kind::Spell => a.category == 8 && !is_ca_start(a) && packet_matches(ask, a),
        Kind::Ws => a.category == 7 && !is_ca_start(a) && packet_matches(ask, a),
        Kind::Ability | Kind::Item => false,
    }
}

fn refusal(ask: &Ask, id: u16, param: u32) -> bool {
    let named = param == packet_id(ask);
    match ask.kind {
        Kind::Spell => spell_refusal(id, param, named),
        Kind::Ws => ws_refusal(id, param),
        Kind::Ability => ability_refusal(id, param, named),
        Kind::Item => item_refusal(id, param, named),
    }
}

macro_rules! ids {
    ($n:ident, $($v:expr),+ $(,)?) => {
        matches!($n, $($v)|+)
    };
}

fn spell_refusal(id: u16, param: u32, named: bool) -> bool {
    if ids!(id, 12, 16, 18, 155, 446) && param == 0 {
        return true;
    }
    if ids!(id, 34, 35, 40, 47, 48, 49, 71, 72, 78, 155, 235, 313, 315, 316, 569, 581, 661, 700, 717) && named {
        return true;
    }
    false
}

fn ws_refusal(id: u16, param: u32) -> bool {
    ids!(id, 12, 71, 89, 155, 190, 191, 192, 216, 217, 446) && param == 0
}

fn ability_refusal(id: u16, param: u32, named: bool) -> bool {
    ids!(
        id, 4, 5, 12, 71, 72, 78, 87, 88, 94, 154, 155, 192, 199, 215, 216, 235, 307, 315, 316, 328, 336, 337, 347,
        356, 428, 429, 446, 523, 524, 525, 561, 574, 662, 666, 745
    ) && (param == 0 || named)
}

fn item_refusal(id: u16, param: u32, named: bool) -> bool {
    if ids!(id, 95, 96) {
        return true;
    }
    if id == 0 && named {
        return true;
    }
    ids!(
        id, 12, 55, 56, 62, 78, 92, 111, 155, 215, 246, 247, 308, 316, 356, 410, 443, 444, 445, 446, 514, 517, 518,
        523, 580, 759
    ) && (param == 0 || named)
}

fn fill_last(_lua: &mlua::Lua, t: &mlua::Table, r: &LastResult) -> mlua::Result<()> {
    match r {
        LastResult::Answered { kind, id, target } => {
            t.set("state", "answered")?;
            t.set("kind", kind_str(*kind))?;
            t.set("id", *id)?;
            t.set("target", *target)?;
        }
        LastResult::Refused { kind, id, target, why } => {
            t.set("state", "refused")?;
            t.set("kind", kind_str(*kind))?;
            t.set("id", *id)?;
            t.set("target", *target)?;
            if !why.is_empty() {
                t.set("why", why.as_str())?;
            }
        }
        LastResult::Quiet { kind, id, target } => {
            t.set("state", "quiet")?;
            t.set("kind", kind_str(*kind))?;
            t.set("id", *id)?;
            t.set("target", *target)?;
        }
        LastResult::Interrupted { kind, id, target } => {
            t.set("state", "interrupted")?;
            t.set("kind", kind_str(*kind))?;
            t.set("id", *id)?;
            t.set("target", *target)?;
        }
        LastResult::Replaced { kind, id, target } => {
            t.set("state", "replaced")?;
            t.set("kind", kind_str(*kind))?;
            t.set("id", *id)?;
            t.set("target", *target)?;
        }
    }
    Ok(())
}

fn fill_pending(_lua: &mlua::Lua, t: &mlua::Table, a: &Ask) -> mlua::Result<()> {
    t.set("kind", kind_str(a.kind))?;
    t.set("id", a.id)?;
    t.set("target", a.target)?;
    t.set("state", state_str(a.state))?;
    if !a.why.is_empty() {
        t.set("why", a.why.as_str())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::action::{Action, ActionResult, ActionTarget};

    #[test]
    fn magic_finish_answers_spell_ask() {
        let mut t = Tracker::default();
        t.start(Kind::Spell, 247, 1);
        let a = Action { actor: 1, category: 4, arg: 247, ..Action::default() };
        t.on_action(1, &a);
        assert!(!t.is_waiting());
        assert!(matches!(t.last, Some(LastResult::Answered { id: 247, .. })));
    }

    #[test]
    fn wrong_spell_id_does_not_answer() {
        let mut t = Tracker::default();
        t.start(Kind::Spell, 247, 1);
        let a = Action { actor: 1, category: 4, arg: 100, ..Action::default() };
        t.on_action(1, &a);
        assert!(t.is_waiting());
    }

    #[test]
    fn start_replaces_waiting_ask() {
        let mut t = Tracker::default();
        t.start(Kind::Spell, 247, 1);
        let cure = t.waiting_gen().unwrap();
        t.start(Kind::Ability, 0x201, 1);
        assert!(t.resolve);
        assert_eq!(t.resolved_generation(), Some(cure));
        assert!(matches!(t.last, Some(LastResult::Replaced { id: 247, .. })));
        assert!(t.is_waiting());
        assert_ne!(t.waiting_gen(), Some(cure));
    }

    #[test]
    fn magic_start_ca_parses_id() {
        let mut t = Tracker::default();
        t.start(Kind::Spell, 100, 1);
        let a = Action {
            actor: 1,
            category: 8,
            arg: 0x6163,
            targets: vec![ActionTarget {
                id: 1,
                results: vec![ActionResult { value: 100, ..ActionResult::default() }],
            }],
            ..Action::default()
        };
        t.on_action(1, &a);
        assert!(t.is_waiting());
        assert!(!t.resolve);
        let now = Instant::now();
        assert_eq!(t.view(now).map(|v| v.state), Some(AskState::Casting));
        t.on_action(1, &Action { actor: 1, category: 4, arg: 100, ..Action::default() });
        assert!(!t.is_waiting());
        assert!(matches!(t.last, Some(LastResult::Answered { id: 100, .. })));
    }

    #[test]
    fn magic_interrupt_after_start() {
        let mut t = Tracker::default();
        t.start(Kind::Spell, 1, 1);
        let start = Action {
            actor: 1,
            category: 8,
            arg: 0x6163,
            targets: vec![ActionTarget { id: 1, results: vec![ActionResult { value: 1, ..ActionResult::default() }] }],
            ..Action::default()
        };
        t.on_action(1, &start);
        let interrupt = Action {
            actor: 1,
            category: 8,
            arg: 0x6f737073,
            targets: vec![ActionTarget { id: 1, results: vec![ActionResult { value: 1, ..ActionResult::default() }] }],
            ..Action::default()
        };
        t.on_action(1, &interrupt);
        assert!(!t.is_waiting());
        assert!(matches!(t.last, Some(LastResult::Interrupted { id: 1, .. })));
    }
}

pub(super) fn pending_to_lua(lua: &mlua::Lua, v: &PendingView<'_>) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    t.set("kind", kind_str(v.kind))?;
    t.set("id", v.id)?;
    t.set("target", v.target)?;
    t.set("state", state_str(v.state))?;
    if !v.why.is_empty() {
        t.set("why", v.why)?;
    }
    t.set("waiting", v.waiting)?;
    t.set("age", v.age)?;
    Ok(t)
}
