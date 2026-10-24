//! Combat in the client: battle messages into the chat log (dat::battle), 0x028 actions driving the
//! actors' attack / damage / death motions from their schedulers (dat::sched), and the action menu
//! (FUN_10127790) whose "Attack" sends 0x01A action 2.

use super::{Actor, World, actmotion};
use crate::client::net::{BattleMessage, NetCommand};
use crate::dat::battle::{self, Ctx, Relation, Who};
use crate::dat::character::Character;
use crate::dat::sched;
use crate::net::action::{Action, category};
use glam::Vec3;
use std::time::{Duration, Instant};

/// Chat log lines with their colour (BGRA, 0x80 = 1.0).
#[derive(Default)]
pub struct ChatLog {
    lines: Vec<(String, [u8; 4])>,
    /// Local time each line came in (hour, minute, second) and whether it is a chat line (retail stamps only those).
    times: Vec<([u8; 3], bool)>,
}

impl ChatLog {
    pub fn push(&mut self, s: impl Into<String>) {
        self.push_colour(s.into(), super::WHITE);
    }

    pub fn push_colour(&mut self, s: String, colour: [u8; 4]) {
        self.push_line(s, colour, false);
    }

    /// A chat line (s2c 0x017 or our sends' echoes), which config 0xb5 stamps.
    pub fn push_chat(&mut self, s: String, colour: [u8; 4]) {
        self.push_line(s, colour, true);
    }

    fn push_line(&mut self, s: String, colour: [u8; 4], chat: bool) {
        log::debug!(target: "chat", "{s}");
        self.times.push((super::cfgapply::local_hms(), chat));
        self.lines.push((s, colour));
    }

    /// Lines with the time they came in and whether they are chat lines.
    pub fn iter_timed(&self) -> impl Iterator<Item = (&(String, [u8; 4]), ([u8; 3], bool))> {
        self.lines.iter().zip(self.times.iter().copied())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (String, [u8; 4])> {
        self.lines.iter()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[cfg(test)]
    pub fn lines(&self) -> &[(String, [u8; 4])] {
        &self.lines
    }
}

/// A motion played once over the looping one (scheduler op 0x05): name, ticks played, length, speed.
#[derive(Clone, Debug)]
pub struct OneShot {
    pub name: String,
    pub ticks: f32,
    pub len: f32,
    pub speed: f32,
}

#[derive(Clone, Copy, Debug)]
struct Msg {
    id: u16,
    actor: u32,
    target: u32,
    work: [i32; 5],
}

/// Messages and a damage reaction waiting for the attacker's hit ("dada" in its attack scheduler).
pub(super) struct Pending {
    at: Instant,
    messages: Vec<Msg>,
    reactions: Vec<(u32, u32, u8, u8)>,
}

pub(super) fn who(w: &World, id: u32) -> Option<Who> {
    if id == w.my_id {
        // Our own gender from the race (inf: retail reads 0x40db0): 2, 4, 6 and 7 are female.
        return Some(Who { name: w.name.clone(), pc: true, female: matches!(w.my_race, 2 | 4 | 6 | 7), ..Default::default() });
    }
    w.entities.get(&id).map(|e| Who { name: e.name.clone(), pc: id < 0x0100_0000, named: e.named, plural: e.plural, female: e.female })
}

/// s2c 0x05A (LAB_100a06e0): dropped for emote 0x61+ or a caster / target not here. The caster plays it (FUN_100a0b90)
/// unless busy with one, not idle / sitting / seated, or Mode 1; /aim failing prints group 9 0x66 instead. The line
/// (file 7025, emote * 2 + no target) in mode 7 for our own untargeted emote, else 0x0f, unless Mode 2 or the Emotes
/// filter (config 0x32 bit 3). Not done: the block list, the pet copying the motion, trusts copying it.
pub(super) fn on_emote(w: &mut World, caster: u32, target: u32, emote: u16, param: u16, mode: u8) {
    let here = |id: u32| id == w.my_id || w.entities.contains_key(&id);
    if emote >= 0x61 || !here(caster) || (target != 0 && !here(target)) {
        return;
    }
    let status = if caster == w.my_id { w.my_motion.status } else { w.entities.get(&caster).map_or(0, |e| e.motion.status) };
    let class = if caster == w.my_id { main_weapon_class(w) } else { 6 };
    let mut failed = false;
    if mode != 1
        && matches!(status, 0 | 4 | 0x2f | 0x30 | 0x3f..=0x53)
        && let Some(a) = w.actors.get_mut(&caster).filter(|a| !actmotion::emoting(a) && a.pending.is_none() && a.character.is_some())
    {
        failed = actmotion::emote(a, emote as u32, param, status, class).is_err();
    }
    let Some(bt) = w.battle.clone() else { return };
    if failed {
        let colour = bt.colour_in(0x79, &w.game);
        w.chat.push_colour(bt.group9(0x66).unwrap_or_default(), colour);
        return;
    }
    if mode > 1 || w.game.get(0x32) & 8 != 0 {
        return;
    }
    let ctx = Ctx { actor: who(w, caster), target: (target != 0).then(|| who(w, target)).flatten(), me: w.name.clone(), work: [0; 5], ..Default::default() };
    let entry = emote as u32 * 2 + (target == 0) as u32;
    let Some(line) = bt.emote(entry, &ctx) else { return };
    let colour = bt.colour_in(if target == 0 && caster == w.my_id { 7 } else { 0x0f }, &w.game);
    for l in line.lines.into_iter().filter(|l| !l.is_empty()) {
        w.chat.push_colour(l, colour);
    }
}

/// The main weapon's motion class (item record byte 0x28, read by /hurray); 6 bare-handed.
fn main_weapon_class(w: &World) -> u8 {
    let Some((c, i)) = w.inv.equip[0] else { return 6 };
    let Some(&(id, _)) = w.inv.slots.get(&(c, i)) else { return 6 };
    w.names.as_ref().and_then(|n| n.items.record(id as u32)).and_then(|(_, r)| r.get(0x28).copied()).unwrap_or(6)
}

/// FUN_1009cda0 without party or alliance (we are never in one yet); a missing actor counts as others.
fn relation(w: &World, actor: u32, target: u32) -> Relation {
    let present = |id: u32| id == w.my_id || w.entities.contains_key(&id);
    if !present(actor) || !present(target) {
        Relation::Other
    } else if target == w.my_id {
        Relation::TargetIsMe
    } else if actor == w.my_id {
        Relation::CasterIsMe
    } else {
        Relation::Other
    }
}

/// FUN_100abf20's relation table for the chat filter: [1] target us, [0] caster us, [6] / [7] caster / target a
/// mob we claimed, [8] / [9] another PC, [10] otherwise (no party or alliance yet).
fn filter_relation(w: &World, actor: u32, target: u32) -> usize {
    let ours = |id: u32| w.entities.get(&id).is_some_and(|e| e.mob && e.claim == w.my_id);
    let pc = |id: u32| id != w.my_id && id < 0x0100_0000 && w.entities.contains_key(&id);
    if target == w.my_id {
        1
    } else if actor == w.my_id {
        0
    } else if ours(actor) {
        6
    } else if ours(target) {
        7
    } else if pc(actor) {
        8
    } else if pc(target) {
        9
    } else {
        10
    }
}

/// Chat lines of a message: hidden modes print nothing; a `1f` code overrides the mode's colour (with the
/// config's text colours).
pub fn render(bt: &battle::BattleText, id: u16, rel: Relation, ctx: &Ctx, cfg: &crate::gamecfg::Values) -> Vec<(String, [u8; 4])> {
    let mode = bt.mode(id, rel);
    if mode == battle::HIDDEN {
        return Vec::new();
    }
    let Some(line) = bt.expand(id, ctx) else { return Vec::new() };
    let colour = bt.colour_in(line.mode.unwrap_or(mode), cfg);
    line.lines.into_iter().filter(|l| !l.is_empty()).map(|l| (l, colour)).collect()
}

fn print(w: &mut World, m: Msg) {
    let Some(bt) = w.battle.clone() else {
        log::info!("battle message {} before the message tables loaded", m.id);
        return;
    };
    // Chat Filters (0x32 / 0x33) hide battle messages on the client.
    if bt.filtered(m.id, filter_relation(w, m.actor, m.target), &w.game) {
        return;
    }
    let ctx = Ctx { actor: who(w, m.actor), target: who(w, m.target), me: w.name.clone(), work: m.work, ..Default::default() };
    for (l, c) in render(&bt, m.id, relation(w, m.actor, m.target), &ctx, &w.game) {
        w.chat.push_colour(l, c);
    }
}

/// The messages of an action with their work arrays, in the client's record order.
pub fn action_messages(a: &Action) -> Vec<(u16, u32, [i32; 5])> {
    let mut out = Vec::new();
    for (target, r) in a.records() {
        let work = [a.arg as i32, r.value as i32, r.proc.as_ref().map_or(0, |p| p.value as i32), r.react.as_ref().map_or(0, |p| p.value as i32), r.bit as i32];
        let ids = [Some(r.message), r.proc.as_ref().map(|p| p.message), r.react.as_ref().map(|p| p.message)];
        out.extend(ids.into_iter().flatten().filter(|&id| id != 0).map(|id| (id, target, work)));
    }
    out
}

/// s2c 0x053 (LAB_100a06a0 -> FUN_100a0150): file 7031 text about ourselves, work[0..2] = para, para2; the
/// mode comes from its 1f code (e.g. #7 the logout countdown, mode 0x88).
pub(super) fn on_system_message(w: &mut World, id: u16, work: [i32; 2]) {
    let Some(bt) = w.battle.clone() else { return };
    let me = w.my_id;
    let ctx = Ctx { actor: who(w, me), target: who(w, me), me: w.name.clone(), work: [work[0], work[1], 0, 0, 0], ..Default::default() };
    match bt.system(id, &ctx) {
        Some(line) => {
            let colour = bt.colour_in(line.mode.unwrap_or(0), &w.game);
            for l in line.lines.into_iter().filter(|l| !l.is_empty()) {
                w.chat.push_colour(l, colour);
            }
        }
        None => log::info!("system message {id} has no text"),
    }
}

/// s2c 0x009 (LAB_100fbd40 -> FUN_100a1ab0): a 7031 line; the actor from CasUniqueNo (none for 0), no target, work
/// = Para0..3, string slots 2 / 3 = string2 / string3 (e.g. 88 "Dice roll! X rolls N!", 89 "X examines you.").
/// Not done: the block list (Attr 0x10), SysMesFilterLevel (s2c 0x0B4), the Mode key's event window.
pub(super) fn on_message_std(w: &mut World, id: u16, cas: u32, para: [i32; 4], strings: [String; 2]) {
    let Some(bt) = w.battle.clone() else { return };
    let ctx = Ctx { actor: if cas != 0 { who(w, cas) } else { None }, target: None, me: w.name.clone(), work: [para[0], para[1], para[2], para[3], 0], strings };
    match bt.system(id, &ctx) {
        Some(line) => {
            let colour = bt.colour_in(line.mode.unwrap_or(0), &w.game);
            for l in line.lines.into_iter().filter(|l| !l.is_empty()) {
                w.chat.push_colour(l, colour);
            }
        }
        None => log::info!("standard message {id} has no text"),
    }
}

/// FUN_1007f6d0 on a command that fails: ">> <line>" and group 9 0x78 "...A command error occurred.", mode 0x9d.
pub(super) fn command_error(w: &mut World, line: &str) {
    let Some(bt) = w.battle.clone() else { return };
    let colour = bt.colour_in(0x9d, &w.game);
    w.chat.push_colour(format!(">> {line}"), colour);
    w.chat.push_colour(bt.group9(0x78).unwrap_or_else(|| "...A command error occurred.".into()), colour);
}

/// The zone-in area line, once the name tables are in (mode 0, white).
pub(super) fn area_line(w: &mut World) {
    let Some(a) = w.area_line else { return };
    let (Some(names), Some(bt)) = (w.names.clone(), w.battle.clone()) else { return };
    w.area_line = None;
    let line = match a {
        super::net::AreaLine::Zone(z) => match crate::dat::names::NameSource::place(&*names, z as u32).filter(|n| !n.is_empty()) {
            // Group 10 0x62 "=== Area: %s ===" (0x10356a44 "=== Area: %d ===" without a name).
            Some(n) => format!("=== Area: {n} ==="),
            None => {
                log::error!("no place name (group 5) for zone {z}");
                format!("=== Area: {z} ===")
            }
        },
        super::net::AreaLine::MogHouse(floor) => bt.group9(0x126 + floor as usize).unwrap_or_default(),
    };
    if !line.is_empty() {
        w.chat.push(line);
    }
}

/// 0x029 / 0x02D: work[0..2] = Data, Data2.
pub(super) fn on_battle_message(w: &mut World, m: &BattleMessage) {
    print(w, Msg { id: m.message, actor: m.actor, target: m.target, work: [m.data[0], m.data[1], 0, 0, 0] });
}

/// 0x028. A melee round of several records replays "atk0" per record (op 0x32 jumps back for the next one): one swing,
/// message and reaction per hit, each after the last swing ends; ours splits the round and queues the rest.
pub(super) fn on_action(w: &mut World, a: &Action) {
    // A new round flushes what is left of the last one at once.
    if let Some((_, rest)) = w.rounds.remove(&a.actor) {
        for r in rest {
            one_action(w, &r);
        }
    }
    if a.category != category::BASIC_ATTACK || a.records().len() < 2 {
        one_action(w, a);
        return;
    }
    let mut subs: std::collections::VecDeque<Action> = a.records().into_iter().map(|(id, r)| Action { targets: vec![crate::net::action::ActionTarget { id, results: vec![r.clone()] }], ..a.clone() }).collect();
    let first = subs.pop_front().unwrap();
    one_action(w, &first);
    w.rounds.insert(a.actor, (Instant::now() + swing_left(w, a.actor), subs));
}

/// What is left of the actor's swing one-shot (the next swing of a round starts after it).
fn swing_left(w: &World, actor: u32) -> Duration {
    let ticks = w.actors.get(&actor).and_then(|x| x.oneshot.as_ref()).map_or(0.0, |o| (o.len - o.ticks).max(0.0));
    Duration::from_secs_f32(ticks / 60.0)
}

/// 0x028: messages per (target, result) record (FUN_1009c730: main, additional effect, reaction) with
/// work = [cmd_arg, value, proc value, react value, bit]; melee waits for the attacker's hit.
fn one_action(w: &mut World, a: &Action) {
    // Our cast finished (category 4, magic file 0xaf0): the spell's recast (asm 0x100a900d).
    if a.actor == w.my_id && a.category == category::MAGIC_FINISH {
        super::magic::set_recast(w, a.arg as u16, a.info);
    }
    // Our cast starts (category 8, cmd_arg "ca??"): FUN_1022b3d0 opens the cast bar for the spell in the result's
    // value; anything else in category 8 ("sp??", an interrupt) closes it (FUN_1022b420).
    if a.actor == w.my_id && a.category == category::MAGIC_START {
        if a.arg & 0xffff == 0x6163 {
            let spell = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.value as u16);
            super::magic::open_cast_bar(w, spell);
        } else {
            w.cast_bar = None;
        }
    }
    let messages: Vec<Msg> = action_messages(a).into_iter().map(|(id, target, work)| Msg { id, actor: a.actor, target, work }).collect();
    if a.actor == w.my_id || a.targets.iter().any(|t| t.id == w.my_id) {
        let recs: Vec<String> = a.records().iter().map(|(t, r)| format!("{t:#x} miss {} kind {} anim {} info {} scale {} value {} msg {}", r.miss, r.kind, r.animation, r.info, r.scale, r.value, r.message)).collect();
        log::debug!(target: "motion", "0x028 category {} arg {:#x} from {:#x}: {recs:?}", a.category, a.arg, a.actor);
    }
    let mut reactions = Vec::new();
    for (target, r) in a.records() {
        let hits = matches!(a.category, category::BASIC_ATTACK | category::RANGED_FINISH | category::SKILL_FINISH | category::MOB_SKILL_FINISH);
        // FUN_100cbd20 / FUN_100cbe30: a hit reacts only at a hit distortion level (scale & 3) above 0; a miss (sway)
        // plays nothing; guard / block and parry have their own motions.
        let reacts = match r.miss {
            0 => r.scale & 3 != 0,
            2..=4 => true,
            _ => false,
        };
        if hits && reacts {
            reactions.push((target, a.actor, r.miss, r.scale & 3));
        }
    }
    let first = a.targets.first().map(|t| t.id);
    // Skill, magic, item and mob TP actions run from effect files (FUN_100a9230): the caster and every target are held
    // from now until "main" starts (fx releases them; 1800-tick safety, +0x11e); the reactions and messages come from
    // its ops 0x25 / 0x2b.
    let by_file = matches!(a.category, 3..=6 | 0xb | 0xd..=0xf);
    if by_file {
        let until = Instant::now() + Duration::from_secs(30);
        for id in std::iter::once(a.actor).chain(a.records().iter().map(|r| r.0)) {
            let e = w.holds.entry(id).or_insert(until);
            *e = (*e).max(until);
        }
    }
    let attack = (a.category == category::BASIC_ATTACK).then(|| start_attack(w, a.actor, first)).flatten();
    // The swing's holds (ops 0x59 / 0x20): the attacker's status changes and reactions wait, the target's status too.
    if let Some((_, _, att, tgt)) = &attack {
        let now = Instant::now();
        let ticks = |t: u32| now + Duration::from_secs_f32(t as f32 / 60.0);
        if let Some(t) = att {
            w.holds.insert(a.actor, ticks(*t));
            w.noreact.insert(a.actor, ticks(*t));
        }
        if let (Some(t), Some(id)) = (tgt, first) {
            let until = ticks(*t);
            let e = w.holds.entry(id).or_insert(until);
            *e = (*e).max(until);
        }
    }
    // The action's schedulers drive its effects (fx/tasks.rs); op 0x24 plays the swing picked here.
    w.effects.on_action(a, attack.as_ref().map(|(_, n, _, _)| n.clone()));
    let delay = attack.map(|(d, _, _, _)| d);
    match delay {
        // dada at T: atpr waits 4, then op 0x2b prints; dam0 (op 0x01 wait 1) reacts at T + 5 (triggers.md 2.2).
        Some(d) => {
            let tick = Duration::from_secs_f32(1.0 / 60.0);
            let now = Instant::now();
            w.pending.push(Pending { at: now + d + tick * 4, messages, reactions: Vec::new() });
            w.pending.push(Pending { at: now + d + tick * 5, messages: Vec::new(), reactions });
        }
        None if by_file => {}
        None => {
            for m in messages {
                print(w, m);
            }
            for (t, by, miss, level) in reactions {
                react(w, t, by, miss, level);
            }
        }
    }
}

/// An effect file's message op (0x2b) or the action's end: record `k`'s messages (main, additional effect, reaction).
pub(super) fn print_record(w: &mut World, a: &Action, k: usize) {
    let Some((id, r)) = a.records().get(k).map(|(id, r)| (*id, (*r).clone())) else { return };
    let one = Action { targets: vec![crate::net::action::ActionTarget { id, results: vec![r] }], ..a.clone() };
    for (mid, target, work) in action_messages(&one) {
        print(w, Msg { id: mid, actor: a.actor, target, work });
    }
}

/// The skill holds from fx: op 0x20 / 0x59 for their ticks (the longest wins), and a load hold's end.
pub(super) fn fx_hold(w: &mut World, id: u32, ticks: f32, noreact: bool) {
    let until = Instant::now() + Duration::from_secs_f32(ticks.max(0.0) / 60.0);
    let e = w.holds.entry(id).or_insert(until);
    *e = (*e).max(until);
    if noreact {
        let e = w.noreact.entry(id).or_insert(until);
        *e = (*e).max(until);
    }
}

pub(super) fn fx_release(w: &mut World, id: u32) {
    w.holds.insert(id, Instant::now());
}

/// FUN_100cdd60 for op 0x25 / 0x21: none when the reactor is dead, riding, in its own no-reaction hold or the level
/// is 0; standing "dfi?" / "dbi?" (side 0 by where the other actor stands, 1 front, 2 back), moving "dfm?" unless side
/// 2 ("dbm?"); an overlay at amount x {0.25, 0.5, 1.0}[level] with the op's rate, count, blend-in and fade.
pub(super) fn react_op(w: &mut World, r: &crate::fx::React) {
    let status = if r.reactor == w.my_id { w.my_motion.status } else { w.entities.get(&r.reactor).map_or(0, |e| e.motion.status) };
    if r.level == 0 || w.noreact.get(&r.reactor).is_some_and(|t| *t > Instant::now()) || matches!(status, 2 | 3 | 5 | 0x55) {
        return;
    }
    let front = match r.side {
        1 => true,
        2 => false,
        _ => match (position(w, r.reactor), position(w, r.other)) {
            (Some((tp, tdir)), Some((op, _))) => super::forward_from_dir(tdir).dot(op - tp) >= 0.0,
            _ => true,
        },
    };
    let name = match (moving(w, r.reactor), front) {
        (true, _) => if r.side == 2 { "dbm" } else { "dfm" },
        (false, true) => "dfi",
        (false, false) => "dbi",
    };
    let Some(a) = w.actors.get_mut(&r.reactor) else { return };
    let Some(ch) = a.character.clone() else { return };
    if a.oneshot.as_ref().is_some_and(|o| o.name.starts_with("ded")) || actmotion::find(&a.extra, &ch, name).is_empty() {
        return;
    }
    let weight = r.amount * [0.0, 0.25, 0.5, 1.0][r.level.min(3) as usize];
    log::debug!(target: "motion", "actor {:#x}: op reaction {name} weight {weight} count {} blend {} fade {} side {}", r.reactor, r.count, r.blend_in, r.fade, r.side);
    a.overlay = Some(actmotion::Overlay { name: name.to_string(), ticks: 0.0, weight, cycles: r.count.clamp(1, 255) as u8, fade: r.fade, rate: if r.rate > 0.0 { r.rate } else { 1.0 }, blend_in: r.blend_in });
}

/// A server status taking effect on an actor (+0x170 from +0x16c): falling dead plays the death; ours goes to the
/// session state too. The server prints the defeat (0x029 #97 / #20).
pub(super) fn apply_status(w: &mut World, id: u32, status: u8) {
    let was = if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) };
    if matches!(status, 2 | 3)
        && !matches!(was, 2 | 3)
        && let Some(a) = w.actors.get_mut(&id)
    {
        die(a);
    }
    if id == w.my_id {
        w.my_motion.status = status;
    } else if let Some(e) = w.entities.get_mut(&id) {
        e.motion.status = status;
    }
}

/// Fires pending hits whose time has come, and applies statuses whose swing hold has ended.
pub(super) fn tick(w: &mut World) {
    let now = Instant::now();
    // The next swing of a round whose last swing has ended.
    let due: Vec<u32> = w.rounds.iter().filter(|(_, (at, _))| *at <= now).map(|(id, _)| *id).collect();
    for actor in due {
        let Some((_, mut rest)) = w.rounds.remove(&actor) else { continue };
        if let Some(next) = rest.pop_front() {
            one_action(w, &next);
            if !rest.is_empty() {
                w.rounds.insert(actor, (Instant::now() + swing_left(w, actor), rest));
            }
        }
    }
    let released: Vec<(u32, u8)> = w.held_status.iter().filter(|(id, _)| w.holds.get(id).is_none_or(|t| *t <= now)).map(|(id, s)| (*id, *s)).collect();
    for (id, s) in released {
        w.held_status.remove(&id);
        apply_status(w, id, s);
    }
    w.holds.retain(|_, t| *t > now);
    w.noreact.retain(|_, t| *t > now);
    let (due, rest): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut w.pending).into_iter().partition(|p| p.at <= now);
    w.pending = rest;
    for p in due {
        for m in p.messages {
            print(w, m);
        }
        for (t, by, miss, level) in p.reactions {
            react(w, t, by, miss, level);
        }
    }
}

fn position(w: &World, id: u32) -> Option<(Vec3, u8)> {
    if id == w.my_id {
        return Some((w.pos, w.net.me.lock().map(|m| m.dir).unwrap_or(0)));
    }
    w.entities.get(&id).map(|e| (e.pos, e.dir))
}

/// Ticks a motion takes at `speed`: frames / (speed * 0.5) (motion player 0x10019ab0).
fn motion_ticks(a: &Actor, ch: &Character, name: &str, speed: f32) -> f32 {
    actmotion::find(&a.extra, ch, name).iter().map(|x| x.frames as f32 / (x.speed * speed * 0.5).max(1e-3)).fold(0.0, f32::max)
}

/// Actor +0xf8: set when it moved more than a threshold this frame (0x100a4248); picks the moving
/// swing and reaction variants.
fn moving(w: &World, id: u32) -> bool {
    if id == w.my_id { w.moving } else { w.entities.get(&id).is_some_and(|e| e.ratio > 1e-4) }
}

fn play(actor: &mut Actor, name: &str, len: f32, speed: f32) {
    actor.oneshot = Some(OneShot { name: name.to_string(), ticks: 0.0, len, speed });
}

/// Melee (FUN_100d4600 / FUN_100d46f0): standing (+0xf8 clear) a random "ati?" avoiding the one just
/// used; moving, the first "atf?" ('r' / 'b' / 'l' by the move direction +0x598; we move forward only).
/// PCs find them in their battle files. Plays the op-0x05 motion; returns when its "dada" hit runs, and its name.
pub(super) fn start_attack(w: &mut World, actor: u32, _target: Option<u32>) -> Option<(Duration, String, Option<u32>, Option<u32>)> {
    let moving = moving(w, actor);
    let seed = Instant::now().duration_since(w.started).subsec_nanos() as usize;
    let a = w.actors.get_mut(&actor)?;
    let ch = a.character.clone()?;
    let names = actmotion::scheduler_names(&a.extra, &ch);
    let (first, other) = if moving { ("atf?", "ati?") } else { ("ati?", "atf?") };
    let mut pool = sched::matching(names.iter().copied(), first);
    if pool.is_empty() {
        pool = sched::matching(names.iter().copied(), other);
    }
    let name = if moving {
        *pool.first()?
    } else {
        if pool.len() > 1 {
            pool.retain(|n| *n != a.last_attack);
        }
        *pool.get(seed % pool.len().max(1))?
    };
    let plan = sched::plan(actmotion::scheduler(&a.extra, &ch, name)?, &["dada", "dcnt"]);
    let m = plan.motions.first()?;
    a.last_attack = name.to_string();
    let len = if m.ticks > 0 { m.ticks as f32 } else { motion_ticks(a, &ch, &m.name, m.speed) };
    play(a, &m.name, len, m.speed);
    Some((Duration::from_secs_f32(plan.hit_at.unwrap_or(0) as f32 / 60.0), a.last_attack.clone(), plan.attacker_hold, plan.target_hold))
}

/// Damage reaction (FUN_100cbd20): "dfi?" / "dbi?" (front / back) standing, "dfm?" / "dbm?" while
/// moving (+0xf8); guard "gdi?" / "gdm?" (FUN_100cbe30). Front or back is taken from where the attacker
/// stands (inferred).
/// FUN_100cdd60 / FUN_100cde70 / FUN_100cdf00: the reaction as an overlay over whatever plays: a hit at 25 / 50 /
/// 100 % by hit distortion `level`, 2 cycles, out over 24 ticks; guard / block / parry at full weight, 4 cycles, 16.
fn react(w: &mut World, target: u32, attacker: u32, miss: u8, level: u8) {
    // None during the reactor's own swing hold (op 0x59) or while it is dead or riding.
    let status = if target == w.my_id { w.my_motion.status } else { w.entities.get(&target).map_or(0, |e| e.motion.status) };
    if w.noreact.get(&target).is_some_and(|t| *t > Instant::now()) || matches!(status, 2 | 3 | 5 | 0x55) {
        return;
    }
    let (Some((tp, tdir)), Some((ap, _))) = (position(w, target), position(w, attacker)) else { return };
    let front = super::forward_from_dir(tdir).dot(ap - tp) >= 0.0;
    let moving = moving(w, target);
    let Some(a) = w.actors.get_mut(&target) else { return };
    let Some(ch) = a.character.clone() else { return };
    if a.oneshot.as_ref().is_some_and(|o| o.name.starts_with("ded")) {
        return;
    }
    let (i, m) = match (miss, front) {
        (2 | 4, _) => ("gdi", "gdm"),
        (3, _) => ("pyi", "pym"),
        (_, true) => ("dfi", "dfm"),
        _ => ("dbi", "dbm"),
    };
    let order = if moving { [m, i] } else { [i, m] };
    if let Some(name) = order.into_iter().find(|n| !actmotion::find(&a.extra, &ch, n).is_empty()) {
        let (weight, cycles, fade) = match miss {
            2..=4 => (1.0, 4, 16.0),
            _ => ([0.0, 0.25, 0.5, 1.0][level.min(3) as usize], 2, 24.0),
        };
        log::debug!(target: "motion", "actor {target:#x}: reaction {name} weight {weight} (miss {miss}, level {level}) from {attacker:#x}");
        a.overlay = Some(actmotion::Overlay { name: name.to_string(), ticks: 0.0, weight, cycles, fade, rate: 1.0, blend_in: 1.0 });
    }
}

/// Death: the actor's "dead" scheduler plays "ded?" for its op length, then the corpse loop takes over.
pub(super) fn die(actor: &mut Actor) {
    let Some(ch) = actor.character.clone() else { return };
    let plan = actmotion::scheduler(&actor.extra, &ch, "dead").map(|d| sched::plan(d, &[]));
    let (name, len, speed) = match plan.as_ref().and_then(|p| p.motions.first()) {
        Some(m) => (m.name.clone(), m.ticks as f32, m.speed),
        None => ("ded".to_string(), motion_ticks(actor, &ch, "ded", 1.0), 1.0),
    };
    if !actmotion::find(&actor.extra, &ch, &name).is_empty() && len > 0.0 {
        play(actor, &name, len, speed);
    }
}

/// 0x038: a scheduler by name on an actor ("kesu", LSB FadeOut before a despawn: its op 0x29 fades to alpha 0).
pub(super) fn on_scheduler(w: &mut World, actor: u32, name: [u8; 4]) {
    w.effects.on_scheduler(actor, actor, u32::from_le_bytes(name));
}

// --- Action menu (FUN_10127790 / FUN_101275b0 / FUN_10127230) ---

/// Command layouts at FFXiMain 0x10363648 (13 bytes each, 0x1c ends).
const LAYOUTS: [&[u8]; 8] = [
    &[4, 8, 7, 0x19, 9, 0xc, 0xd, 0x16, 0x18],
    &[4, 0xe, 0xc, 0xf],
    &[4, 8, 7, 0x19, 9, 0x13, 0x10, 0xc, 0x16, 0x18, 0x1b, 0xa],
    &[4, 0xe, 0x10, 0xc, 0xf, 0x1b, 0xa],
    &[1, 8, 7, 0x19, 9, 0xc, 0x16, 0x18, 0xb],
    &[0x15, 0xe, 0xc, 0xb],
    &[0x12, 8, 7, 0x19, 9, 3, 0xc, 0x18, 0xb],
    &[6, 8, 7, 0x19, 9, 0x14, 0x11, 0xc, 0x1a],
];

/// Command labels at 0x103634f8 + 12 * cmd: (sprite, library 0 = keytops3 / 1 = windowps; FUN_101270a0
/// stores FUN_101bf3a0(2) and (1) at +0x14 / +0x18).
const LABELS: [(u16, u8); 0x1c] = [
    (0, 0), (0x10, 0), (0x17, 0), (0x16, 0), (1, 0), (1, 0), (0x16b, 1), (4, 0), (7, 0), (0xa, 0), (0x13, 0), (0x13, 0), (0x19, 0), (0xd, 0),
    (0x26, 0), (0x27, 0), (0x4f, 1), (0x16a, 1), (6, 0), (0x1c, 1), (0x1c, 1), (0x29, 0), (0x164, 0), (0x165, 0), (0x166, 0), (0x1f9, 0), (0x29c, 1), (0x30f, 1),
];

pub mod cmd {
    pub const ATTACK: u8 = 1;
    pub const TALK: u8 = 6;
    pub const ITEMS: u8 = 9;
    pub const DISENGAGE: u8 = 3;
    pub const CHAT: u8 = 4;
    pub const TELL: u8 = 5;
    pub const CHECK: u8 = 0xb;
    pub const SWITCH_TARGET: u8 = 0x12;
    pub const CALL_HELP: u8 = 2;
    pub const MAGIC: u8 = 8;
    pub const TREASURE: u8 = 0xc;
    pub const TRADE_NPC: u8 = 0x14;
    pub const TRADE: u8 = 0x13;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionMenu {
    pub items: Vec<u8>,
    pub sel: usize,
    /// DAT_10578174: Right on Disengage swapped it for Call for help (reset when the layout leaves battle).
    pub call_help: bool,
}

/// Layout choice (FUN_10127790): engaged 6; an attackable target 4 (2 * 2); an NPC 7 (Talk first;
/// inferred: actor +0x1cc bits 2 and 0x100 taken as "not a monster"); another target 2; none 0.
fn layout(w: &World) -> usize {
    if w.my_motion.status == 1 {
        return 6;
    }
    let base = match w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e))) {
        Some((_, e)) if e.mob && !matches!(e.motion.status, 2 | 3) => 4,
        Some((t, e)) if t >> 24 != 0 && !e.mob => 7,
        Some(_) => 2,
        None => 0,
    };
    // FUN_1012b4b0: riding (status 5 chocobo, 0x55 mount) takes the odd layout (Dig, Dismount), but the NPC one.
    if matches!(w.my_motion.status, 5 | 0x55) && base != 7 { base + 1 } else { base }
}

/// FUN_10127230's filters, as far as they apply to us (no treasure pool, trust, fellow or party).
fn shown(target: Option<u32>, c: u8) -> Option<u8> {
    let npc = target.is_some_and(|t| t >= 0x0100_0000);
    match c {
        // Chat becomes Tell on a player, and is dropped on an NPC.
        cmd::CHAT if npc => None,
        cmd::CHAT if target.is_some() => Some(cmd::TELL),
        cmd::CHECK if target.is_none() => None,
        // Talk: the target is an NPC that is not engaged (FUN_10127230 case 6); NPC Trade by the same rule.
        cmd::TALK | cmd::TRADE_NPC if npc => Some(c),
        // Player Trade (layout 2): a player target (the costume bit +0x1d0 & 0x400 not checked).
        cmd::TRADE if target.is_some() && !npc => Some(c),
        0xa if target.is_none() || npc => None,
        6 | 0xc | 0xd | 0x10 | 0x11 | 0x13 | 0x14 | 0x16..=0x18 | 0x1a | 0x1b => None,
        _ if LABELS[c as usize].1 != 0 => None,
        _ => Some(c),
    }
}

/// The commands of a layout (0..8) for a target id.
pub fn menu_items(layout: usize, target: Option<u32>) -> Vec<u8> {
    LAYOUTS[layout.min(7)].iter().filter_map(|&c| shown(target, c)).collect()
}

/// The rows now (FUN_1012b2d0 over FUN_1012af30): engaged, Disengage becomes Call for help while toggled and
/// Treasure shows while the pool holds items (Sprint's area rule is not done).
fn current_items(w: &World, call_help: bool) -> Vec<u8> {
    let l = layout(w);
    let mut items = menu_items(l, w.target.filter(|t| w.entities.contains_key(t)));
    // FUN_1012af30 case 0xe: Dig only on a chocobo (not a mount); its further zone tests are not traced.
    if w.my_motion.status == 0x55 {
        items.retain(|&c| c != 0xe);
    }
    if l == 6 {
        if call_help && let Some(c) = items.iter_mut().find(|c| **c == cmd::DISENGAGE) {
            *c = cmd::CALL_HELP;
        }
        if w.treasure.iter().any(|t| t.0 != 0)
            && let Some(at) = items.iter().position(|&c| c == cmd::CHECK)
        {
            items.insert(at, cmd::TREASURE);
        }
    }
    items
}

pub(super) fn open_menu(w: &World) -> ActionMenu {
    ActionMenu { items: current_items(w, false), sel: 0, call_help: false }
}

/// FUN_1012b4b0 each frame: the rows are rebuilt (the window follows their count, the cursor kept in range); the
/// toggle resets outside battle.
pub(super) fn refresh_menu(w: &mut World) {
    let Some(m) = &w.action_menu else { return };
    let call_help = m.call_help && w.my_motion.status == 1;
    let items = current_items(w, call_help);
    if let Some(m) = &mut w.action_menu {
        m.call_help = call_help;
        if m.items != items {
            m.sel = m.sel.min(items.len().saturating_sub(1));
            m.items = items;
        }
    }
}

/// The window manager's status watch (FUN_1015ef60): our applied status turning 1 outside an event runs
/// FUN_1015f3e0(0x20): the sub-target prompt is cancelled, every window not kept in battle closes, then the battle
/// set opens focused (playermo, gaugewin; targetwi stays). Leaving 1 runs FUN_1015f3e0(0x40): playermo, gaugewin and
/// magselec close. A list chosen from the battle menu brings it back when it closes (FUN_101336d0 for items; inferred
/// for the magic / ability lists).
pub(super) fn battle_frame(w: &mut World) {
    let s = w.my_motion.status;
    if s == 1 && w.battle_prev != 1 && !super::event::active(w) {
        super::target::cancel_sub(w);
        (w.menu, w.win, w.config, w.settings, w.magic, w.ability, w.cats) = (None, None, None, None, None, None, None);
        (w.moogle, w.mogdoor, w.full_log, w.shop, w.handover) = (None, None, None, None, None);
        w.action_menu = Some(open_menu(w));
        w.gauge = Some(super::gauge::Gauge::default());
    } else if s != 1 && w.battle_prev == 1 {
        (w.action_menu, w.gauge, w.reopen_menu) = (None, None, false);
        if w.cats.as_ref().is_some_and(|c| c.menu == "menu    magselec") {
            w.cats = None;
        }
    }
    w.battle_prev = s;
    if let (Some(g), Some(v)) = (w.gauge.as_mut(), w.vitals) {
        g.tick(v.2);
    }
    let busy = w.win.is_some() || w.magic.is_some() || w.ability.is_some() || w.cats.is_some() || w.prompt.is_some();
    if w.reopen_menu && !busy {
        w.reopen_menu = false;
        if s == 1 && w.action_menu.is_none() {
            w.action_menu = Some(open_menu(w));
        }
    }
    refresh_menu(w);
}

/// FUN_1012b880: the commands that leave the menu up (Attack, Call for help, Disengage, Check, Switch Target); the
/// lists (Magic, Abilities, Trust, Items) close it and, engaged, bring it back when they are done; Chat, Tell, Talk
/// close it.
pub(super) fn keeps_menu(c: u8) -> bool {
    matches!(c, cmd::ATTACK | cmd::CALL_HELP | cmd::DISENGAGE | 0xa | cmd::CHECK | cmd::SWITCH_TARGET)
}

/// Window name per row count (table 0x103636b0: actionm1 .. actiom10).
pub fn window_name(rows: usize) -> String {
    match rows.clamp(1, 10) {
        10 => "menu    actiom10".into(),
        n => format!("menu    actionm{n}"),
    }
}

/// Row rects inside the window at (x, y): 88 x 16 at (21, 5 + 16 n) as in the PS2 battlemo layout (inferred).
pub fn rows(x: f32, y: f32, n: usize) -> Vec<[f32; 4]> {
    (0..n).map(|i| [x + 21.0, y + 5.0 + 16.0 * i as f32, 88.0, 16.0]).collect()
}

pub fn label(c: u8) -> (&'static str, usize) {
    let (sprite, lib) = LABELS[c as usize];
    (if lib == 1 { "menu    windowps" } else { "menu    keytops3" }, sprite as usize)
}

/// Runs a command; returns false when it is not implemented.
pub(super) fn choose(w: &mut World, c: u8) -> bool {
    let target = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.index)));
    match (c, target) {
        (cmd::ATTACK, Some((t, i))) => send(w, NetCommand::Act { target: t, index: i, action: 2 }),
        // Case 0x12: the sub-target prompt over monsters (FUN_10159440 flags 0x20), then "/attack <id>" of the pick.
        (cmd::SWITCH_TARGET, _) => {
            let line = super::target::SubThen::Line { stash: "/attack ".into(), quoted: false, from_macro: false };
            if !super::target::open_sub(w, 0x20, line, None, String::new(), 0) {
                w.sounds.push(crate::audio::Ui::Error as u32);
            }
        }
        // Case 2: "/help".
        (cmd::CALL_HELP, _) => super::textcmd::run(w, "/help", super::textcmd::Mode::Internal),
        (cmd::DISENGAGE, _) => {
            let (t, i) = target.unwrap_or((w.my_id, 0));
            send(w, NetCommand::Act { target: t, index: i, action: 4 });
        }
        (cmd::CHECK, Some((t, i))) => send(w, NetCommand::Check { target: t, index: i, kind: 0 }),
        (cmd::CHAT | cmd::TELL, _) => w.typing = true,
        // FUN_1012b880 cases 6 / 7 / 0x18: Abilities opens abiselec, Magic the list without trusts, Trust only them.
        (7, _) => super::magic::open_cats(w, "menu    abiselec"),
        (8, _) => super::magic::open_magic(w, 0, false, 0x100),
        (0x19, _) => super::magic::open_magic(w, 8, false, 0),
        // FUN_10127b60 case 5: FUN_1007aac0 on the target.
        (cmd::TALK, Some((t, _))) => {
            super::event::talk(w, t);
        }
        // FUN_1012b880 case 0x13: the hand-over (FUN_1013fd30).
        (cmd::TRADE_NPC | cmd::TRADE, Some(_)) => {
            super::trade::start(w);
        }
        // Case 8: the item list (FUN_101e9f10).
        // FUN_1012b880 case 8: the direct-use list; engaged, the battle menu comes back when it closes.
        (cmd::ITEMS, _) => {
            w.reopen_menu = w.my_motion.status == 1;
            super::items::open_direct(w)
        }
        // FUN_1012b880 cases 0xd / 0xe: Dig runs /dig; Dismount asks first (group 0xe 0x4d "Dismount?").
        (0xe, _) => super::textcmd::run(w, "/dig", super::textcmd::Mode::Internal),
        (0xf, _) => w.confirm = Some((super::Confirm::Dismount, false)),
        _ => return false,
    }
    true
}

fn send(w: &World, c: NetCommand) {
    let _ = w.net.commands.send(c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::net::{battle_message, battle_message2};
    use crate::dat::battle::BattleText;
    use crate::net::action::{ActionResult, ActionTarget, Side, pack, parse};
    use crate::net::map::session::SubPacket;

    const ME: u32 = 0x0001_0203;
    const RABBIT: u32 = 0x0100_4011;

    fn load() -> Option<BattleText> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?;
        Some(BattleText::load(&index, &dll).unwrap())
    }

    fn sub(data: Vec<u8>) -> SubPacket {
        SubPacket { id: data[0] as u16 | ((data[1] as u16 & 1) << 8), sync: 0, data }
    }

    /// GP_SERV_COMMAND_BATTLE_MESSAGE (0x029) / _MESSAGE2 (0x02D) as LSB lays them out.
    fn lsb_message(id: u8, cas: u32, tar: u32, data: [u32; 2], idx: [u16; 2], msg: u16) -> SubPacket {
        let mut b = vec![0u8; 0x1c];
        b[0] = id;
        b[1] = (0x1c / 4) << 1;
        b[4..8].copy_from_slice(&cas.to_le_bytes());
        b[8..12].copy_from_slice(&tar.to_le_bytes());
        let (d, i) = if id == 0x29 { (12, 0x14) } else { (0x10, 0xc) };
        b[d..d + 4].copy_from_slice(&data[0].to_le_bytes());
        b[d + 4..d + 8].copy_from_slice(&data[1].to_le_bytes());
        b[i..i + 2].copy_from_slice(&idx[0].to_le_bytes());
        b[i + 2..i + 4].copy_from_slice(&idx[1].to_le_bytes());
        b[0x18..0x1a].copy_from_slice(&msg.to_le_bytes());
        sub(b)
    }

    fn who(id: u32) -> Option<Who> {
        Some(match id {
            ME => Who { name: "Tuxedo".into(), pc: true, ..Default::default() },
            _ => Who { name: "Wild Rabbit".into(), ..Default::default() },
        })
    }

    fn rel(actor: u32, target: u32) -> Relation {
        if target == ME { Relation::TargetIsMe } else if actor == ME { Relation::CasterIsMe } else { Relation::Other }
    }

    /// Replays LSB-built 0x029 / 0x02D / 0x028 bytes through the client's parsers and the message table.
    #[test]
    fn replayed_packets() {
        let Some(bt) = load() else { return };
        let say = |id: u16, actor: u32, target: u32, work: [i32; 5]| render(&bt, id, rel(actor, target), &Ctx { actor: who(actor), target: who(target), me: "Tuxedo".into(), work, ..Default::default() }, &Default::default());
        let cfg = crate::gamecfg::Values::default();
        let (red, white) = (bt.colour_in(0x1c, &cfg), bt.colour_in(0x14, &cfg));

        let m = battle_message(&lsb_message(0x29, ME, RABBIT, [0, 0], [0x400, 0x011], 6));
        assert_eq!((m.actor_index, m.target_index, m.message), (0x400, 0x011, 6));
        assert_eq!(say(m.message, m.actor, m.target, [m.data[0], m.data[1], 0, 0, 0]), [("Tuxedo defeats the Wild Rabbit.".to_string(), bt.colour_in(bt.mode(6, Relation::CasterIsMe), &cfg))]);

        let m = battle_message2(&lsb_message(0x2d, ME, ME, [50, 0], [0x400, 0x400], 8));
        assert_eq!(say(m.message, m.actor, m.target, [m.data[0], m.data[1], 0, 0, 0])[0].0, "Tuxedo gains 50 experience points.");

        // The rabbit hits for 7 with a 3-point additional effect, then misses.
        let hit = ActionResult { kind: 1, scale: 1, value: 7, message: 1, proc: Some(Side { kind: 1, info: 0, value: 3, message: 163 }), ..Default::default() };
        let miss = ActionResult { miss: 1, kind: 1, message: 15, ..Default::default() };
        let a = Action { actor: RABBIT, category: category::BASIC_ATTACK, arg: u32::from_le_bytes(*b"atk0"), info: 0, targets: vec![ActionTarget { id: ME, results: vec![hit, miss] }] };
        let decoded = parse(&sub(pack(&a)).data);
        let lines: Vec<(String, [u8; 4])> = action_messages(&decoded).into_iter().flat_map(|(id, t, work)| say(id, decoded.actor, t, work)).collect();
        assert_eq!(lines[0], ("The Wild Rabbit hits Tuxedo for 7 points of damage.".to_string(), red));
        assert_eq!(lines[1].0, "Additional effect: 3 points of damage.");
        assert_eq!(lines[2].0, "The Wild Rabbit misses Tuxedo.");

        // Our hit on it is drawn in the "you hit" colour.
        let a = Action { actor: ME, targets: vec![ActionTarget { id: RABBIT, results: vec![ActionResult { message: 1, value: 12, ..Default::default() }] }], ..a };
        let d = parse(&pack(&a));
        let (id, t, work) = action_messages(&d)[0];
        assert_eq!(say(id, d.actor, t, work), [("Tuxedo hits the Wild Rabbit for 12 points of damage.".to_string(), white)]);
    }

    /// FUN_101275b0 over the layouts, filtered as FUN_10127230 does for a solo player.
    #[test]
    fn action_menus() {
        assert_eq!(menu_items(4, Some(RABBIT)), [cmd::ATTACK, 8, 7, 0x19, 9, cmd::CHECK]);
        assert_eq!(menu_items(6, Some(RABBIT)), [cmd::SWITCH_TARGET, 8, 7, 0x19, 9, cmd::DISENGAGE, cmd::CHECK]);
        assert_eq!(menu_items(0, None), [cmd::CHAT, 8, 7, 0x19, 9]);
        // Chat becomes Tell on a player; an NPC target drops it.
        assert_eq!(menu_items(2, Some(0x0000_0042))[0], cmd::TELL);
        assert_eq!(menu_items(2, Some(0x0100_0042))[0], 8);
    }

    #[test]
    fn window_names() {
        assert_eq!(window_name(1), "menu    actionm1");
        assert_eq!(window_name(10), "menu    actiom10");
        assert_eq!(label(cmd::ATTACK), ("menu    keytops3", 0x10));
    }
}
