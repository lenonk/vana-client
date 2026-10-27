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
    /// Local time each line came in (hour, minute, second), whether it is a chat line (retail stamps only those) and
    /// whether its continuation lines are indented (the mode's flag, FUN_10127870).
    times: Vec<([u8; 3], bool, bool)>,
    /// Each line's chat mode (record byte 0: its colour and the full log's tab filters, FUN_101299f0), NO_MODE for the
    /// lines printed without one.
    modes: Vec<u16>,
    /// Lines changed in place (condensed battle lines) so far.
    edits: u64,
}

pub const NO_MODE: u16 = 0xffff;
/// Battle messages (print): the modern chat's Battle tab (not retail).
pub const BATTLE_MODE: u16 = 0xfffe;

impl ChatLog {
    pub fn push(&mut self, s: impl Into<String>) {
        self.push_colour(s.into(), super::WHITE);
    }

    pub fn push_colour(&mut self, s: String, colour: [u8; 4]) {
        self.push_line(s, colour, false, false, NO_MODE);
    }

    /// A battle message's line.
    pub fn push_battle(&mut self, s: String, colour: [u8; 4]) {
        self.push_line(s, colour, false, false, BATTLE_MODE);
    }

    /// A line whose continuation lines are indented.
    pub fn push_indented(&mut self, s: String, colour: [u8; 4]) {
        self.push_line(s, colour, false, true, NO_MODE);
    }

    /// A chat line (s2c 0x017 or our sends' echoes), which config 0xb5 stamps.
    pub fn push_chat(&mut self, s: String, colour: [u8; 4], indent: bool) {
        self.push_line(s, colour, true, indent, NO_MODE);
    }

    /// A chat line with its mode.
    pub fn push_mode(&mut self, s: String, colour: [u8; 4], indent: bool, mode: u8) {
        self.push_line(s, colour, true, indent, mode as u16);
    }

    fn push_line(&mut self, s: String, colour: [u8; 4], chat: bool, indent: bool, mode: u16) {
        log::debug!(target: "chat", "{}", crate::dat::font::strip_codes(&s));
        self.times.push((super::cfgapply::local_hms(), chat, indent));
        self.modes.push(mode);
        self.lines.push((s, colour));
    }

    /// Line `k`'s text replaced (a condensed battle line grown).
    pub fn set_text(&mut self, k: usize, s: String) {
        if let Some(l) = self.lines.get_mut(k) {
            l.0 = s;
            self.edits += 1;
        }
    }

    /// How many lines have been changed in place: the logs wrap again when it moves.
    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// The lines' modes, in the lines' order.
    pub fn modes(&self) -> &[u16] {
        &self.modes
    }

    /// Lines with the time they came in, whether they are chat lines and whether they indent.
    pub fn iter_timed(&self) -> impl Iterator<Item = (&(String, [u8; 4]), ([u8; 3], bool, bool))> {
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
    /// A 0x028 record's message (0 main, 1 proc, 2 react): it may also put its number on screen (FUN_1009e090).
    which: Option<u8>,
    /// Its action's serial (Action::seq).
    seq: u32,
}

/// Main lines waiting for the attacker's hit ("dada" in its attack scheduler).
pub(super) struct Pending {
    at: Instant,
    messages: Vec<Msg>,
}

/// The name tables for a message's key items.
fn keys(w: &World) -> Option<crate::dat::battle::Keys> {
    w.names.clone().map(crate::dat::battle::Keys)
}

pub(super) fn who(w: &World, id: u32) -> Option<Who> {
    if id == w.my_id {
        // Our own gender from the race (inf: retail reads 0x40db0): 2, 4, 6 and 7 are female.
        return Some(Who { name: super::monstrosity::my_name(w).to_string(), pc: true, female: matches!(w.my_race, 2 | 4 | 6 | 7), ..Default::default() });
    }
    // A Monipulator's shown name (FUN_100c1190 slots 0 / 1); still a PC id: no article.
    w.entities.get(&id).map(|e| Who { name: super::monstrosity::name(e).to_string(), pc: id < 0x0100_0000, named: e.named, plural: e.plural, female: e.female })
}

/// s2c 0x05A (LAB_100a06e0): dropped for emote 0x61+ or a caster / target not here. The caster plays it (FUN_100a0b90)
/// unless busy with one, not idle / sitting / seated, or Mode 1; /aim failing prints group 9 0x66 instead. The line
/// (file 7025, emote * 2 + no target) in mode 7 for our own untargeted emote, else 0x0f, unless Mode 2 or the Emotes
/// filter (config 0x32 bit 3), and dropped for a caster on the block list (FUN_100a1a00 kind 2). Not done: the pet
/// copying the motion, trusts copying it.
pub(super) fn on_emote(w: &mut World, caster: u32, target: u32, emote: u16, param: u16, mode: u8) {
    let here = |id: u32| id == w.my_id || w.entities.contains_key(&id);
    if emote >= 0x61 || !here(caster) || (target != 0 && !here(target)) || super::partyreq::sender_blocked(w, caster, emote, 2) {
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
    let ctx = Ctx { actor: who(w, caster), target: (target != 0).then(|| who(w, target)).flatten(), me: w.name.clone(), work: [0; 5], keys: keys(w), ..Default::default() };
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

/// FUN_1009e700 (30261006_0): us, then our party (mode 0), then the alliance (mode 5); a missing actor counts as
/// others.
fn relation(w: &World, actor: u32, target: u32) -> Relation {
    relation_in(w.my_id, |id| w.entities.contains_key(&id), &w.party, actor, target)
}

fn relation_in(me: u32, known: impl Fn(u32) -> bool, roster: &super::party::Roster, actor: u32, target: u32) -> Relation {
    let present = |id: u32| id == me || known(id);
    let party = |id: u32| id != me && roster.party().any(|m| m.id == id);
    let alliance = |id: u32| id != me && roster.members().any(|m| m.id == id);
    if !present(actor) || !present(target) {
        Relation::Other
    } else if target == me {
        Relation::TargetIsMe
    } else if actor == me {
        Relation::CasterIsMe
    } else if party(actor) {
        Relation::PartyCaster
    } else if party(target) {
        Relation::PartyTarget
    } else if alliance(actor) {
        Relation::AllianceCaster
    } else if alliance(target) {
        Relation::AllianceTarget
    } else {
        Relation::Other
    }
}

/// FUN_100abf20's relation table for the chat filter: [1] target us or our pet, [0] caster us or our pet, [2] / [3]
/// caster / target in our party, [4] / [5] in the alliance, [6] / [7] a mob our party claimed, [8] / [9] a PC, [10]
/// otherwise or a missing actor. Other members' pets are not modelled (entity +0x1f6).
fn filter_relation(w: &World, actor: u32, target: u32) -> usize {
    let present = |id: u32| id == w.my_id || w.entities.contains_key(&id);
    let ours = |id: u32| {
        w.entities.get(&id).is_some_and(|e| {
            e.mob && e.claim != 0 && (e.claim == w.my_id || w.party.party().any(|m| m.id == e.claim))
        })
    };
    let pc = |id: u32| id < 0x0100_0000;
    let me = |id: u32| id == w.my_id || (w.my_pet != 0 && w.entities.get(&id).is_some_and(|e| e.index == w.my_pet));
    let party = |id: u32| id != w.my_id && w.party.party().any(|m| m.id == id);
    let alliance = |id: u32| id != w.my_id && w.party.members().any(|m| m.id == id);
    if !present(actor) || !present(target) {
        10
    } else if me(target) {
        1
    } else if me(actor) {
        0
    } else if party(actor) {
        2
    } else if party(target) {
        3
    } else if alliance(actor) {
        4
    } else if alliance(target) {
        5
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

/// A battle line the later records of its action merge into (modern chat, not retail): the action, the message, the
/// line with its number as SLOT, where it is in the log and the numbers so far.
pub(super) struct Condensed {
    seq: u32,
    id: u16,
    template: String,
    line: usize,
    values: Vec<i32>,
}

const SLOT: i32 = 2_000_000_003;

/// A record's line as its template: the text with work[`slot`] as SLOT, when the number is all that differs.
fn template_of(bt: &battle::BattleText, id: u16, rel: Relation, ctx: &Ctx, slot: usize, line: &str, cfg: &crate::gamecfg::Values) -> Option<String> {
    let mut tc = ctx.clone();
    tc.work[slot] = SLOT;
    let template = render(bt, id, rel, &tc, cfg).into_iter().next()?.0;
    let own = template.replacen(&SLOT.to_string(), &ctx.work[slot].to_string(), 1);
    (own == line).then_some(template)
}

/// A condensed line: the template with the numbers listed, or with no number its count.
fn condensed_text(template: &str, values: &[i32]) -> String {
    let slot = SLOT.to_string();
    if template.contains(&slot) {
        let list: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        template.replacen(&slot, &list.join(", "), 1)
    } else if values.len() > 1 {
        format!("{template} (x{})", values.len())
    } else {
        template.to_string()
    }
}

/// A record's line merges into its action's earlier line of the same text but the number ("hits the Hare for 12, 15
/// points of damage"); a line with no number into its twin ("misses the Hare. (x2)"). True when it merged.
fn condense(w: &mut World, bt: &battle::BattleText, m: &Msg, which: u8, line: &str, ctx: &Ctx) -> bool {
    let slot = 1 + which.min(2) as usize;
    let Some(template) = template_of(bt, m.id, relation(w, m.actor, m.target), ctx, slot, line, &w.game) else { return false };
    let value = m.work[slot];
    if let Some(g) = w.condensed.iter_mut().find(|g| g.seq == m.seq && g.id == m.id && g.template == template) {
        g.values.push(value);
        w.chat.set_text(g.line, condensed_text(&g.template, &g.values));
        return true;
    }
    if w.condensed.len() >= 64 {
        w.condensed.remove(0);
    }
    w.condensed.push(Condensed { seq: m.seq, id: m.id, template, line: w.chat.len(), values: vec![value] });
    false
}

fn print(w: &mut World, m: Msg) {
    let Some(bt) = w.battle.clone() else {
        log::info!("battle message {} before the message tables loaded", m.id);
        return;
    };
    // Chat Filters (0x32 / 0x33) hide battle messages on the client, number and line.
    if bt.filtered(m.id, filter_relation(w, m.actor, m.target), &w.game) {
        return;
    }
    // FUN_1009e090: a record's line (and number) needs its caster and target to exist still.
    if m.which.is_some() && (who(w, m.actor).is_none() || who(w, m.target).is_none()) {
        return;
    }
    if let Some(which) = m.which
        && !bt.off_channel(m.id, 1, &w.game)
    {
        damage_number(w, &bt, &m, which);
    }
    // Screen mode (config 0x13 = 2): lines with an on-screen kind leave the log, 0x029 / 0x02D ones too.
    if bt.off_channel(m.id, 0, &w.game) {
        return;
    }
    let ctx = Ctx { actor: who(w, m.actor), target: who(w, m.target), me: w.name.clone(), work: m.work, keys: keys(w), ..Default::default() };
    let lines = render(&bt, m.id, relation(w, m.actor, m.target), &ctx, &w.game);
    if w.modern
        && w.condense
        && let (Some(which), [(line, _)]) = (m.which, lines.as_slice())
        && condense(w, &bt, &m, which, line, &ctx)
    {
        return;
    }
    for (l, c) in lines {
        w.chat.push_battle(l, c);
    }
}

/// FUN_1009e090's number: DAT 27's scheduler for the message's kind on (caster, target), (target, caster) for a
/// reaction (spikes, counters show over the attacker), with the record's value of that message and the colour its
/// log line has for this relation.
fn damage_number(w: &mut World, bt: &battle::BattleText, m: &Msg, which: u8) {
    let kind = bt.screen_kind(m.id);
    let mode = bt.mode(m.id, relation(w, m.actor, m.target));
    if kind == 0 || mode == battle::HIDDEN {
        return;
    }
    let [b, g, r, _] = bt.colour_in(mode, &w.game);
    let rgb = u32::from_le_bytes([b, g, r, 0]);
    let (a1, a2) = if which == 2 { (m.target, m.actor) } else { (m.actor, m.target) };
    w.effects.on_damage_number(kind, a1, a2, m.work[1 + which.min(2) as usize], rgb);
}

/// A 0x028 action's messages in the client's record order: main, proc, react per record.
fn record_messages(a: &Action) -> Vec<Msg> {
    let mut out = Vec::new();
    for (target, r) in a.records() {
        let work = [a.arg as i32, r.value as i32, r.proc.as_ref().map_or(0, |p| p.value as i32), r.react.as_ref().map_or(0, |p| p.value as i32), r.bit as i32];
        let ids = [Some(r.message), r.proc.as_ref().map(|p| p.message), r.react.as_ref().map(|p| p.message)];
        for (which, id) in ids.into_iter().enumerate() {
            if let Some(id) = id.filter(|&id| id != 0) {
                out.push(Msg { id, actor: a.actor, target, work, which: Some(which as u8), seq: a.seq });
            }
        }
    }
    out
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
    let ctx = Ctx { actor: who(w, me), target: who(w, me), me: w.name.clone(), work: [work[0], work[1], 0, 0, 0], keys: keys(w), ..Default::default() };
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
/// A PC's messages 0x59 / 0x5a / 0x64..0x67 go by the block list; SysMesFilterLevel drops some (textcmd::sys_filtered).
/// Not done: the Mode key's event window.
pub(super) fn on_message_std(w: &mut World, id: u16, cas: u32, para: [i32; 4], strings: [String; 2]) {
    if cas != 0 && cas >> 24 == 0 && super::partyreq::sender_blocked(w, cas, id, 0) || super::textcmd::sys_filtered(w, id) {
        return;
    }
    let Some(bt) = w.battle.clone() else { return };
    let ctx = Ctx { actor: if cas != 0 { who(w, cas) } else { None }, target: None, me: w.name.clone(), work: [para[0], para[1], para[2], para[3], 0], strings, keys: keys(w) };
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
    if m.actor == w.my_id || m.target == w.my_id {
        log::debug!("battle message {} {:#x} -> {:#x} data {:?}", m.message, m.actor, m.target, m.data);
    }
    // The block list (FUN_100a1a00 kind 1): the handler drops the caster's message, effect and all.
    if super::partyreq::sender_blocked(w, m.actor, m.message, 1) {
        return;
    }
    // FUN_100ad660 -> FUN_100c3300: into the queue, which FUN_100c33d0 steps from the next frame.
    let wait = MsgWait::new(m.queued, msg_target_status(w, m));
    w.msg_queue.push(QueuedMsg { m: m.clone(), wait, fresh: true });
}

/// A battle message in FUN_100c3300's queue.
pub(super) struct QueuedMsg {
    m: BattleMessage,
    wait: MsgWait,
    fresh: bool,
}

/// FUN_100c3480's wait (+0x54 state, +0x58 counter; ours a float, as frame ticks are under 1 at high frame rates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MsgWait {
    state: u8,
    left: f32,
}

impl MsgWait {
    /// State 0: the queue messages that wait for their target (600 ticks, or 60 once it is gone).
    pub fn new(queued: u16, target_status: Option<u8>) -> MsgWait {
        match target_status {
            _ if !matches!(queued, 1 | 9 | 0xb | 0x32 | 0x2cf | 0x32b | 0x32d) => MsgWait { state: 2, left: 0.0 },
            None => MsgWait { state: 2, left: 60.0 },
            Some(_) => MsgWait { state: 1, left: 600.0 },
        }
    }

    /// One frame; true when the entry is due. State 1 counts down and then looks at the target: gone ends the wait,
    /// dead (applied status 2 / 3) leaves 60 ticks; state 2 counts down to due.
    pub fn step(&mut self, ticks: f32, target_status: Option<u8>) -> bool {
        if self.state == 1 {
            self.left -= ticks;
            if self.left <= 0.0 {
                (self.state, self.left) = (2, 0.0);
            }
            match target_status {
                None => (self.state, self.left) = (2, 0.0),
                Some(2 | 3) => (self.state, self.left) = (2, 60.0),
                Some(_) => {}
            }
            return false;
        }
        self.left -= ticks;
        self.left <= 0.0
    }
}

/// DAT_1035d570 by the queue message's kind (the four language columns are equal); Monstrosity (+0x210) picks 0x388
/// for a level up and 0x389 for message 0x2d.
fn effect_id(queued: u16, monstrosity: bool) -> Option<u32> {
    Some(match queued {
        9 if monstrosity => 0x388,
        9 => 0x1fe,
        0xb => 0x1fd,
        0x32 => 0x2fe,
        0x2cf => 0x3a0,
        0x2d if monstrosity => 0x389,
        0x32b => 0x3f8,
        0x32d => 0x3f9,
        _ => return None,
    })
}

/// The queue's target (+0x48): its applied status, None when its entity is gone; ours when the packet names none.
fn msg_target_status(w: &World, m: &BattleMessage) -> Option<u8> {
    if m.target == w.my_id || m.target == 0 {
        return Some(w.my_motion.status);
    }
    w.entities.get(&m.target).map(|e| e.motion.status)
}

/// FUN_100c33d0: every queued message steps on its own; a due one runs its effect (file 0xaf0 + id `main` on the
/// caster's model, FUN_100a9700) and prints.
pub(super) fn message_queue(w: &mut World, dt: f32) {
    let mut due = Vec::new();
    for mut q in std::mem::take(&mut w.msg_queue) {
        if std::mem::take(&mut q.fresh) {
            w.msg_queue.push(q);
            continue;
        }
        let st = msg_target_status(w, &q.m);
        if q.wait.step(dt * 60.0, st) {
            due.push(q.m);
        } else {
            w.msg_queue.push(q);
        }
    }
    for m in due {
        let mon = if m.actor == w.my_id { w.my_plate.monstrosity != 0 } else { w.entities.get(&m.actor).is_some_and(|e| e.monstrosity != 0) };
        if let Some(id) = effect_id(m.queued, mon)
            && w.actors.get(&m.actor).is_some_and(|a| a.character.is_some())
        {
            let who = crate::fx::sched::Who::Actor(m.actor);
            log::info!("battle message {}: effect DAT {} on {:#x}", m.message, 0xaf0 + id, m.actor);
            w.effects.on_event_scheduler(0xaf0 + id, u32::from_le_bytes(*b"main"), who, who);
        }
        print(w, Msg { id: m.message, actor: m.actor, target: m.target, work: [m.data[0], m.data[1], 0, 0, 0], which: None, seq: 0 });
    }
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
    w.action_seq = w.action_seq.wrapping_add(1).max(1);
    let a = &Action { seq: w.action_seq, ..a.clone() };
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

/// A spell being cast or a skill being readied, seen in 0x028 category 8 / 7 (the modern target panel; not retail).
#[derive(Clone, Copy, Debug)]
pub struct Casting {
    pub skill: bool,
    pub id: u16,
    pub start: Instant,
    /// The spell's base cast time (the record's; the caster's own speed is not sent): none for a skill.
    pub secs: Option<f32>,
}

/// Category 8 "ca" starts a cast, 7 readies a skill (the result's value); any other arg there is the interrupt; a
/// finish (3, 4, 11) ends it.
fn track_cast(w: &mut World, a: &Action) {
    let value = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.value as u16);
    let start = a.arg & 0xffff == 0x6163;
    match a.category {
        category::MAGIC_START | category::SKILL_START if start => {
            let skill = a.category == category::SKILL_START;
            let secs = (!skill).then(|| w.actions.as_ref().and_then(|x| x.spell(value)).map(|r| r.cast as f32 / 3.0)).flatten().filter(|&s| s > 0.0);
            w.casts.insert(a.actor, Casting { skill, id: value, start: Instant::now(), secs });
        }
        category::MAGIC_START | category::SKILL_START | category::MAGIC_FINISH | category::SKILL_FINISH | category::MOB_SKILL_FINISH => {
            w.casts.remove(&a.actor);
        }
        _ => {}
    }
}

/// 0x028: messages per (target, result) record (FUN_1009c730: main, additional effect, reaction) with
/// work = [cmd_arg, value, proc value, react value, bit]; melee waits for the attacker's hit.
fn one_action(w: &mut World, a: &Action) {
    track_cast(w, a);
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
    let mut messages = record_messages(a);
    // Melee / ranged proc and react lines (added effects, spikes, counters) print from their files' op 0x71.
    if matches!(a.category, category::BASIC_ATTACK | category::RANGED_FINISH) {
        messages.retain(|m| m.which == Some(0));
    }
    if a.actor == w.my_id || a.targets.iter().any(|t| t.id == w.my_id) {
        let recs: Vec<String> = a.records().iter().map(|(t, r)| format!("{t:#x} miss {} kind {} anim {} info {} scale {} value {} msg {}", r.miss, r.kind, r.animation, r.info, r.scale, r.value, r.message)).collect();
        log::debug!(target: "motion", "0x028 category {} arg {:#x} from {:#x}: {recs:?}", a.category, a.arg, a.actor);
    }
    // Hit, guard, parry and block reactions come from the files (dam0 -> damg / gurd / pary / gur1: ops 0x21 / 0x5a).
    let first = a.targets.first().map(|t| t.id);
    // Skill, magic, item and mob TP actions run from effect files (FUN_100a9230): the caster and every target are held
    // from now until "main" starts (fx releases them; 1800-tick safety, +0x11e); the reactions and messages come from
    // its ops 0x25 / 0x2b.
    let by_file = matches!(a.category, 3..=6 | 0xb | 0xd..=0xf);
    if by_file {
        for id in std::iter::once(a.actor).chain(a.records().iter().map(|r| r.0)) {
            fx_load(w, id);
        }
    }
    let record = a.records().first().map(|(_, r)| (r.kind, r.animation));
    let attack = (a.category == category::BASIC_ATTACK).then(|| start_attack(w, a.actor, first, record)).flatten();
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
        // dada at T: atpr waits 4, then op 0x2b prints (triggers.md 2.2).
        Some(d) => {
            let tick = Duration::from_secs_f32(1.0 / 60.0);
            w.pending.push(Pending { at: Instant::now() + d + tick * 4, messages });
        }
        None if by_file => {}
        None => {
            for m in messages {
                print(w, m);
            }
        }
    }
}

/// FUN_1009e090: record `k`'s message `which` (0 main from op 0x2b / the action's end, 1 proc / 2 react from op 0x71).
pub(super) fn print_record(w: &mut World, a: &Action, k: usize, which: u8) {
    let Some((target, r)) = a.records().get(k).map(|(id, r)| (*id, (*r).clone())) else { return };
    let work = [a.arg as i32, r.value as i32, r.proc.as_ref().map_or(0, |p| p.value as i32), r.react.as_ref().map_or(0, |p| p.value as i32), r.bit as i32];
    let id = match which {
        0 => Some(r.message),
        1 => r.proc.as_ref().map(|p| p.message),
        _ => r.react.as_ref().map(|p| p.message),
    };
    if let Some(id) = id.filter(|&id| id != 0) {
        print(w, Msg { id, actor: a.actor, target, work, which: Some(which), seq: a.seq });
    }
}

/// An actor waits while held by a swing / op 0x20 / 0x59, a skill action's file loading, or a knockback.
pub(super) fn held(w: &World, id: u32) -> bool {
    let now = Instant::now();
    w.holds.get(&id).is_some_and(|t| *t > now) || w.loads.get(&id).is_some_and(|l| l.1 > now) || w.knocks.contains_key(&id)
}

/// A running knockback (actor +0x102): velocity per tick, signed speed, decay, constant-phase ticks left, the
/// pusher's position, the motion, and the part tick carried to the next frame.
pub(super) struct KnockState {
    vel: Vec3,
    speed: f32,
    decay: f32,
    left: f32,
    origin: Vec3,
    motion: String,
    carry: f32,
}

/// Applied statuses whose actor plays the knockback motions (op 0x5e step 4, FUN_100ca8c0).
fn knock_shows(status: u8) -> bool {
    matches!(status, 0 | 1 | 4 | 7 | 0x21 | 0x2f | 0x30 | 0x3f..=0x53)
}

/// FUN_100aa940: a push starts (or replaces the running one) unless the reactor is dead, riding or in an event pose;
/// its no-reaction and busy counters hold while it runs (held()).
pub(super) fn knock(w: &mut World, k: &crate::fx::Knock) {
    let status = if k.reactor == w.my_id { w.my_motion.status } else { w.entities.get(&k.reactor).map_or(0, |e| e.motion.status) };
    if matches!(status, 2 | 3 | 5 | 6 | 0x26..=0x2b | 0x32..=0x35 | 0x38..=0x3e | 0x55) {
        return;
    }
    let Some((pos, _)) = position(w, k.reactor) else { return };
    let vel = (pos - k.from).normalize_or_zero() * k.speed;
    let left = if k.ticks == 0.0 { 1.0 } else { k.ticks };
    log::debug!(target: "motion", "actor {:#x}: knockback {} speed {} decay {} ticks {left}", k.reactor, k.motion, k.speed, k.decay);
    w.knocks.insert(k.reactor, KnockState { vel, speed: k.speed, decay: k.decay, left, origin: k.from, motion: k.motion.clone(), carry: 0.0 });
    if knock_shows(status) {
        actmotion::play_op(w, k.reactor, Some(sched::Motion { name: k.motion.clone(), at: 0, speed: 1.0, ticks: 0, count: k.count, blend: 12 }));
    }
}

/// FUN_100aab70: one frame of a push from `pos`: whole ticks move by the velocity (a pull stops 0.9 from the
/// puller), the velocity decays each tick once the constant phase is over. Returns the position, whether the
/// constant phase ended this frame, and whether the push is over (|v|^2 < 9e-6).
pub(super) fn knock_step(k: &mut KnockState, pos: Vec3, ticks: f32) -> (Vec3, bool, bool) {
    let prev = k.left;
    k.left -= ticks;
    k.carry += ticks;
    let mut p = pos;
    while k.carry >= 1.0 {
        k.carry -= 1.0;
        let cand = p + k.vel;
        if k.speed >= 0.0 || (cand - k.origin).length_squared() >= 0.81 {
            p = cand;
        }
        // Ticks come in frame parts here: a small tolerance keeps the phase end on its whole tick.
        if k.left <= 1e-3 {
            k.vel -= k.vel * k.decay;
        }
    }
    (p, prev > 1e-3 && k.left <= 1e-3, k.vel.length_squared() < 9e-6)
}

/// FUN_100ca8c0: when the constant phase ends, "bf1?" / "bb1?" once (blend 12), then the status motion.
pub(super) fn knock_recover(w: &mut World, id: u32) {
    let Some(name) = w.knocks.get(&id).map(|k| k.motion.replacen('0', "1", 1)) else { return };
    let status = if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) };
    if !knock_shows(status) {
        return;
    }
    let Some(a) = w.actors.get(&id) else { return };
    let Some(ch) = a.character.clone() else { return };
    let len = actmotion::motion_len(a, &ch, &name, 1.0);
    actmotion::play_op(w, id, Some(sched::Motion { name, at: 0, speed: 1.0, ticks: len.ceil() as u16, count: 1, blend: 12 }));
}

/// FUN_100a4140 for a skill action's load (busy +1; 1800-tick safety, +0x11e).
pub(super) fn fx_load(w: &mut World, id: u32) {
    let e = w.loads.entry(id).or_insert((0, Instant::now()));
    e.0 += 1;
    e.1 = Instant::now() + Duration::from_secs(30);
}

/// The skill holds from fx (ops 0x20 / 0x1f / 0x56 / 0x59 / 0x07 / 0x08) for their ticks, the longest winning.
pub(super) fn fx_hold(w: &mut World, id: u32, ticks: f32, kind: u8) {
    let until = Instant::now() + Duration::from_secs_f32(ticks.max(0.0) / 60.0);
    if kind & crate::fx::HOLD_STATUS != 0 {
        let e = w.holds.entry(id).or_insert(until);
        *e = (*e).max(until);
    }
    if kind & crate::fx::HOLD_REACT != 0 {
        let e = w.noreact.entry(id).or_insert(until);
        *e = (*e).max(until);
    }
}

/// FUN_100a9610: one load hold ends (busy -1); op 0x20 holds run on.
pub(super) fn fx_release(w: &mut World, id: u32) {
    if let Some(l) = w.loads.get_mut(&id) {
        l.0 = l.0.saturating_sub(1);
        if l.0 == 0 {
            w.loads.remove(&id);
        }
    }
}

/// FUN_100cdd60 for op 0x25 / 0x21: none when the reactor is dead, riding, in its own no-reaction hold or the level
/// is 0; standing "dfi?" / "dbi?" (side 0 by where the other actor stands, 1 front, 2 back), moving "dfm?" unless side
/// 2 ("dbm?"); an overlay at amount x {0.25, 0.5, 1.0}[level] with the op's rate, count, blend-in and fade.
pub(super) fn react_op(w: &mut World, r: &crate::fx::React) {
    let status = if r.reactor == w.my_id { w.my_motion.status } else { w.entities.get(&r.reactor).map_or(0, |e| e.motion.status) };
    // FUN_1005e5f0: none while the reactor is being knocked back.
    if r.level == 0 || w.noreact.get(&r.reactor).is_some_and(|t| *t > Instant::now()) || w.knocks.contains_key(&r.reactor) || matches!(status, 2 | 3 | 5 | 0x55) {
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

/// FUN_1008c6c0: `sdep` for a trust (+0x12c bit 22) or with entity +0x12f bit 0, else table 0x1032d5f0 by entity
/// +0x130 bits 5..6 (op 0xac).
pub(super) fn death_scheduler(pick: u8, sdep: bool) -> &'static str {
    if sdep { "sdep" } else { ["dead", "1ead", "2ead", "3ead"][(pick & 3) as usize] }
}

/// The entity's `sdep` flags (0x00D / 0x00E u8 @0x28 bits 0 and 3).
pub(super) fn sdep(w: &World, id: u32) -> bool {
    w.entities.get(&id).is_some_and(|e| e.marks.trust || e.marks.death_flag)
}

/// A server status taking effect on an actor (+0x170 from +0x16c): falling dead plays the death; ours goes to the
/// session state too. The server prints the defeat (0x029 #97 / #20).
/// FUN_1008c6c0: the scheduler an applied status change runs: a model-id actor leaving battle `bton`, engaging
/// `ntob`; a death the pick's (death_scheduler; its ded? / cor? are die()'s, so the task's motions are off), or
/// `sdep` with its own motions (vtable +0x298). Not done: fishing.
pub(super) fn status_schedulers(fx: &mut crate::fx::Effects, actors: &std::collections::HashMap<u32, Actor>, id: u32, old: u8, new: u8, sdep: bool) {
    if old == new {
        return;
    }
    let model = actors.get(&id).and_then(|a| a.character.as_ref()).is_some_and(|c| c.pc.is_none());
    if model && old == 1 {
        fx.on_scheduler(id, id, u32::from_le_bytes(*b"bton"));
    }
    if model && new == 1 {
        fx.on_scheduler(id, id, u32::from_le_bytes(*b"ntob"));
    }
    if matches!(new, 2 | 3) && !matches!(old, 2 | 3) {
        let name = death_scheduler(actors.get(&id).map_or(0, |a| a.death_pick), sdep);
        let n = u32::from_le_bytes(name.as_bytes().try_into().unwrap());
        if sdep {
            fx.on_scheduler(id, id, n);
        } else {
            fx.on_scheduler_quiet(id, n);
        }
    }
}

pub(super) fn apply_status(w: &mut World, id: u32, status: u8) {
    let was = if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) };
    let sd = sdep(w, id);
    status_schedulers(&mut w.effects, &w.actors, id, was, status, sd);
    if matches!(status, 2 | 3)
        && !matches!(was, 2 | 3)
        && let Some(a) = w.actors.get_mut(&id)
    {
        die(a, sd);
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
    w.loads.retain(|_, l| l.1 > now);
    let released: Vec<(u32, u8)> = w.held_status.iter().filter(|(id, s)| !held(w, **id) && !super::fishrod::walking_hold(w, **id, **s)).map(|(id, s)| (*id, *s)).collect();
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
/// FUN_100d4600's name patterns: `atf?` moving / `ati?` standing (the other first), the second letter
/// `"t123t123"[sub-animation]` (forms of multi-form monsters).
pub(super) fn swing_patterns(moving: bool, sub: u8) -> (String, String) {
    let c = b"t123t123"[(sub & 7) as usize] as char;
    let (f, i) = (format!("a{c}f?"), format!("a{c}i?"));
    if moving { (f, i) } else { (i, f) }
}

/// FUN_100d46f0 by the hit's record (FUN_10185ea0: kind @4, animation @6): kind 1 animation 1 (the off hand) tries the
/// sub weapon's `b..?` swings first, 2 / 3 `c..?` / `d..?` (4 "ls05" not handled), then the `a..?` ones.
pub(super) fn swing_hand(record: Option<(u8, u16)>) -> Option<char> {
    match record {
        Some((1, 1)) => Some('b'),
        Some((1, 2)) => Some('c'),
        Some((1, 3)) => Some('d'),
        _ => None,
    }
}

pub(super) fn start_attack(w: &mut World, actor: u32, _target: Option<u32>, record: Option<(u8, u16)>) -> Option<(Duration, String, Option<u32>, Option<u32>)> {
    let moving = moving(w, actor);
    let sub = w.entities.get(&actor).map_or(0, |e| e.sub[2]);
    let seed = Instant::now().duration_since(w.started).subsec_nanos() as usize;
    let a = w.actors.get_mut(&actor)?;
    let ch = a.character.clone()?;
    let names = actmotion::scheduler_names(&a.extra, &ch);
    let (first, other) = swing_patterns(moving, sub);
    let hand = |p: &str| swing_hand(record).map(|h| format!("{h}{}", &p[1..]));
    let mut pool = Vec::new();
    for p in [hand(&first), Some(first.clone()), hand(&other), Some(other.clone())].into_iter().flatten() {
        pool = sched::matching(names.iter().copied(), &p);
        if !pool.is_empty() {
            break;
        }
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
    let len = if m.ticks > 0 { m.ticks as f32 } else { motion_ticks(a, &ch, &m.name, 1.0) };
    let rate = actmotion::op_rate(actmotion::find(&a.extra, &ch, &m.name).first().copied(), len);
    play(a, &m.name, len, rate);
    Some((Duration::from_secs_f32(plan.hit_at.unwrap_or(0) as f32 / 60.0), a.last_attack.clone(), plan.attacker_hold, plan.target_hold))
}

/// FUN_100cde70 / FUN_100cdf00 for ops 0x5a / 0x5b: none while the reactor is in its own no-reaction hold or riding;
/// standing `gdi?` / `pyi?`, moving `gdm?` / `pym?`, no fallback between them; an overlay at amount 1.0 with the op's
/// rate, count, blend-in and fade (no level, no direction, no dead check).
pub(super) fn guard_op(w: &mut World, g: &crate::fx::Guard) {
    let status = if g.reactor == w.my_id { w.my_motion.status } else { w.entities.get(&g.reactor).map_or(0, |e| e.motion.status) };
    if w.noreact.get(&g.reactor).is_some_and(|t| *t > Instant::now()) || w.knocks.contains_key(&g.reactor) || matches!(status, 5 | 0x55) {
        return;
    }
    let name = match (g.parry, moving(w, g.reactor)) {
        (false, false) => "gdi",
        (false, true) => "gdm",
        (true, false) => "pyi",
        (true, true) => "pym",
    };
    let Some(a) = w.actors.get_mut(&g.reactor) else { return };
    let Some(ch) = a.character.clone() else { return };
    if actmotion::find(&a.extra, &ch, name).is_empty() {
        return;
    }
    log::debug!(target: "motion", "actor {:#x}: {name} count {} fade {}", g.reactor, g.count, g.fade);
    a.overlay = Some(actmotion::Overlay { name: name.to_string(), ticks: 0.0, weight: 1.0, cycles: g.count.clamp(1, 255) as u8, fade: g.fade, rate: if g.rate > 0.0 { g.rate } else { 1.0 }, blend_in: g.blend_in });
}

/// Death: the actor's death scheduler (dead, or the op 0xac pick's) plays "ded?" for its op length, then the corpse
/// loop takes over; a picked one without a motion (the Ghrah's `1ead`) plays none.
/// The death's motion from the death scheduler's plan; `sdep` plays its own through fx.
pub(super) fn die(actor: &mut Actor, sdep: bool) {
    let Some(ch) = actor.character.clone() else { return };
    if sdep {
        return;
    }
    let which = death_scheduler(actor.death_pick, false);
    let plan = actmotion::scheduler(&actor.extra, &ch, which).map(|d| sched::plan(d, &[]));
    let (name, len, speed) = match plan.as_ref().and_then(|p| p.motions.first()) {
        Some(m) => (m.name.clone(), m.ticks as f32, actmotion::op_rate(actmotion::find(&actor.extra, &ch, &m.name).first().copied(), m.ticks as f32)),
        None if actor.death_pick != 0 => return,
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

/// mp_pmode (Monstrosity): command n of table 0x103701e8 {help, Enter, Right} is 0x40 + n here.
pub mod mon {
    pub const CHAT: u8 = 0x40;
    pub const ATTACK: u8 = 0x41;
    pub const SWITCH: u8 = 0x42;
    pub const MAGIC: u8 = 0x43;
    pub const ABILITIES: u8 = 0x44;
    pub const DISENGAGE: u8 = 0x45;
    pub const CHECK: u8 = 0x46;
    pub const TREASURE: u8 = 0x47;
}

/// Table 0x103701a8: the commands of layouts 0 (no target), 1 (us), 2 (another), 3 (engaged).
const MON_LAYOUTS: [&[u8]; 4] = [&[0, 3, 4, 7], &[0, 3, 4, 7], &[1, 3, 4, 7, 6], &[2, 3, 4, 5, 7, 6]];
/// Table 0x103701e8's help ids (group 0xe).
const MON_HELP: [usize; 8] = [0x16, 0x1f, 0x25, 0x19, 0x2b5, 0x20, 0x1d, 0x1b];

/// An mp_pmode command's help (FUN_101411a0).
pub fn mon_help(c: u8) -> Option<usize> {
    MON_HELP.get(c.checked_sub(0x40)? as usize).copied()
}

/// FUN_10141070 / FUN_101410d0: engaged 3, a target 2 (1 when it is us), none 0; Treasure only while the pool holds
/// items (FUN_1013eff0).
fn mon_items(w: &World) -> Vec<u8> {
    let layout = match w.target {
        _ if w.my_motion.status == 1 => 3,
        Some(t) if t == w.my_id => 1,
        Some(t) if w.entities.contains_key(&t) => 2,
        _ => 0,
    };
    let pool = super::treasure::count(w) > 0;
    MON_LAYOUTS[layout].iter().filter(|&&c| c != 7 || pool).map(|c| 0x40 + c).collect()
}

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
    /// FUN_1012b4b0's hold: since when the left button is down on the cursor row (60 ticks run its Right function).
    pub hold: Option<Instant>,
}

/// The cursor row's Right function (table 0x1036f240 @8): Disengage / Call for help swap (FUN_1012acc0, plays nothing;
/// blocked by /blockhelp: group 9 0xf7 and SE 0004), Magic opens magselec (FUN_1012ab00). False for a row without one.
pub(super) fn right(w: &mut World) -> bool {
    match w.action_menu.as_ref().and_then(|m| m.items.get(m.sel).copied()) {
        Some(cmd::DISENGAGE | cmd::CALL_HELP) => {
            if w.block_help {
                w.sounds.push(crate::audio::Ui::Error as u32);
                super::textcmd::log9(w, 0xf7);
            } else if let Some(m) = w.action_menu.as_mut() {
                m.call_help = !m.call_help;
            }
            true
        }
        Some(cmd::MAGIC | mon::MAGIC) => {
            super::magic::open_cats(w, "menu    magselec");
            true
        }
        Some(mon::CHAT) => {
            super::chatctrl::open(w);
            true
        }
        // FUN_1012aae0: the chat mode window; the action menu stays.
        Some(cmd::CHAT | cmd::TELL) => {
            super::chatctrl::open(w);
            true
        }
        _ => false,
    }
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
fn shown(target: Option<u32>, c: u8, treasure: bool) -> Option<u8> {
    let npc = target.is_some_and(|t| t >= 0x0100_0000);
    match c {
        // FUN_1012af30 case 0xc: every layout while the pool holds items, not in Conflict.
        cmd::TREASURE if treasure => Some(c),
        // Chat becomes Tell on a player, and is dropped on an NPC.
        cmd::CHAT if npc => None,
        cmd::CHAT if target.is_some() => Some(cmd::TELL),
        cmd::CHECK if target.is_none() => None,
        // Talk: the target is an NPC that is not engaged (FUN_10127230 case 6); NPC Trade by the same rule.
        cmd::TALK | cmd::TRADE_NPC if npc => Some(c),
        // Player Trade (layout 2): a player target (the costume bit +0x1d0 & 0x400 not checked).
        cmd::TRADE if target.is_some() && !npc => Some(c),
        0xa if target.is_none() || npc => None,
        6 | 0xc | 0xd | 0x10 | 0x11 | 0x13 | 0x14 | 0x1a | 0x1b => None,
        _ if LABELS[c as usize].1 != 0 => None,
        _ => Some(c),
    }
}

/// The commands of a layout (0..8) for a target id.
pub fn menu_items(layout: usize, target: Option<u32>) -> Vec<u8> {
    menu_rows(layout, target, false)
}

fn menu_rows(layout: usize, target: Option<u32>, treasure: bool) -> Vec<u8> {
    LAYOUTS[layout.min(7)].iter().filter_map(|&c| shown(target, c, treasure)).collect()
}

/// The rows now (FUN_1012b2d0 over FUN_1012af30): engaged, Disengage becomes Call for help while toggled and
/// Treasure shows while the pool holds items (Sprint's area rule is not done).
fn current_items(w: &World, call_help: bool) -> Vec<u8> {
    // In Monstrosity the engage broadcast and every opener reach mp_pmode instead (registry flag 0x4000000).
    if super::monstrosity::active(w) {
        return mon_items(w);
    }
    let l = layout(w);
    let treasure = super::treasure::count(w) > 0 && !conflict(w.my_team);
    let mut items = menu_rows(l, w.target.filter(|t| w.entities.contains_key(t)), treasure);
    // FUN_1012af30 case 0xe: Dig only on a chocobo (not a mount); its further zone tests are not traced.
    if w.my_motion.status == 0x55 {
        items.retain(|&c| c != 0xe);
    }
    // FUN_1012af30 cases 0x16..0x18: Quarry / Scout / Sprint in Conflict only (Scout needs Petra, not parsed: Sprint).
    if !conflict(w.my_team) {
        items.retain(|c| !matches!(c, 0x16..=0x18));
    }
    // Check: not on a pet, trust or charmed monster (FUN_1008e150: not a PC, team byte 1).
    if let Some(t) = w.target.filter(|t| *t >> 24 != 0)
        && w.entities.get(&t).is_some_and(|e| e.marks.alleg == 1)
    {
        items.retain(|&c| c != cmd::CHECK);
    }
    if l == 6
        && call_help
        && let Some(c) = items.iter_mut().find(|c| **c == cmd::DISENGAGE)
    {
        *c = cmd::CALL_HELP;
    }
    items
}

/// FUN_101cc350: a Conflict team byte (2..99, not 8 / 9 or 0x28..0x2b).
pub(super) fn conflict(team: u8) -> bool {
    (2..=99).contains(&team) && !matches!(team, 8 | 9 | 0x28..=0x2b)
}

/// FUN_101347b0: a check on `id` (else the target): not the same id again within 2 s (DAT_105781c4), and sent (c2s
/// 0x0DD) only for a player or a monster (+0x120 bit 13); any other NPC gets nothing.
pub(super) fn check(w: &mut World, id: Option<u32>, kind: u8, typed: bool) {
    // Nothing while the inspect window is up (holder +0x74).
    if w.inspect.up() {
        return;
    }
    let Some(id) = id.or(w.target) else { return };
    if w.last_check.is_some_and(|(last, at)| last == id && at.elapsed() < Duration::from_secs(2)) {
        return;
    }
    let index = if id == w.my_id {
        w.my_index
    } else {
        match w.entities.get(&id) {
            Some(e) if id >> 24 == 0 || e.mob => e.index,
            _ => return,
        }
    };
    let fresh = w.last_check.is_none();
    super::inspect::sent(w, id, index, fresh, typed);
    w.last_check = Some((id, Instant::now()));
    send(w, NetCommand::Check { target: id, index, kind });
}

pub(super) fn open_menu(w: &World) -> ActionMenu {
    ActionMenu { items: current_items(w, false), sel: 0, call_help: false, hold: None }
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
        super::inspect::drop(w);
        super::treasure::close(w);
        w.chatctrl = None;
        // FUN_10202f00: the party-list cursor (<stpt> / <stal>) is cancelled too, stopping its macro (no sound *(inf)*).
        if let Some(pc) = w.party_cursor.take()
            && let super::target::SubThen::Line { from_macro: true, .. } = pc.then
            && let Some(m) = w.macros.as_mut()
        {
            m.stop();
        }
        (w.menu, w.win, w.config, w.settings, w.magic, w.ability, w.cats) = (None, None, None, None, None, None, None);
        (w.moogle, w.mogdoor, w.full_log, w.shop, w.handover) = (None, None, None, None, None);
        // Reopened by name (FUN_1015e350, registry flag 0x10000): on its first row.
        (w.action_menu, w.held_menu) = (Some(open_menu(w)), None);
        w.gauge = Some(super::gauge::Gauge::default());
    } else if s != 1 && w.battle_prev == 1 {
        (w.action_menu, w.gauge, w.held_menu) = (None, None, None);
        if w.cats.as_ref().is_some_and(|c| c.menu == "menu    magselec") {
            w.cats = None;
        }
    }
    w.battle_prev = s;
    if let (Some(g), Some(v)) = (w.gauge.as_mut(), w.vitals) {
        g.tick(v.2);
    }
    let busy = w.win.is_some() || w.magic.is_some() || w.ability.is_some() || w.cats.is_some() || w.prompt.is_some();
    // The walk resumes the suspended menu once the lists are gone (in battle or not), its cursor kept.
    if w.held_menu.is_some() && !busy && w.action_menu.is_none() {
        w.action_menu = w.held_menu.take();
    }
    refresh_menu(w);
}

/// FUN_1012b880: the commands that leave the menu up (Attack, Call for help, Disengage, Check, Switch Target); the
/// lists (Magic, Abilities, Trust, Items) close it and, engaged, bring it back when they are done; Chat, Tell, Talk
/// close it.
pub(super) fn keeps_menu(c: u8) -> bool {
    matches!(c, cmd::ATTACK | cmd::CALL_HELP | cmd::DISENGAGE | 0xa | cmd::CHECK | cmd::SWITCH_TARGET | cmd::TREASURE | 0x16..=0x18 | mon::ATTACK | mon::SWITCH | mon::DISENGAGE | mon::CHECK | mon::TREASURE)
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
            // No candidate: nothing (the result is ignored).
            let line = super::target::SubThen::Line { stash: "/attack ".into(), quoted: false, from_macro: false };
            super::target::open_sub(w, 0x20, line, None, String::new(), 0);
        }
        // Case 2: "/help".
        (cmd::CALL_HELP, _) => super::textcmd::run(w, "/help", super::textcmd::Mode::Internal),
        // Case 0xb: the pool windows over the action menu (FUN_1013f060).
        (cmd::TREASURE, _) => super::treasure::open(w),
        (cmd::DISENGAGE, _) => {
            let (t, i) = target.unwrap_or((w.my_id, 0));
            send(w, NetCommand::Act { target: t, index: i, action: 4 });
        }
        // FUN_1012b880 cases 9 / 0xa: engaged, the sub-target prompt over monsters first (LAB_1012bec0); else at once.
        (cmd::CHECK, Some(_)) if w.my_motion.status == 1 => {
            super::target::open_sub(w, 0x20, super::target::SubThen::Check, None, String::new(), 0);
        }
        (cmd::CHECK, Some(_)) => check(w, None, 0, false),
        (0x16..=0x18, _) => super::textcmd::run(w, ["/quarry", "/scout", "/sprint"][(c - 0x16) as usize], super::textcmd::Mode::Internal),
        // FUN_1012b880 cases 3 / 4: the chat line, "/tell <target> " on a target (string 0x1036f5c8).
        (cmd::CHAT | cmd::TELL, _) => {
            w.chat_input = match w.target.and_then(|t| w.entities.get(&t)).map(|e| super::monstrosity::name(e).to_string()).filter(|n| !n.is_empty() && c == cmd::TELL) {
                Some(n) => format!("/tell {n} "),
                None => String::new(),
            };
            w.chat_caret = None;
            w.typing = true;
        }
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
        // FUN_1012b880 case 8: the direct-use list; the menu comes back when it closes.
        (cmd::ITEMS, _) => super::items::open_direct(w),
        // FUN_1012b880 cases 0xd / 0xe: Dig runs /dig; Dismount asks first (group 0xe 0x4d "Dismount?").
        (0xe, _) => super::textcmd::run(w, "/dig", super::textcmd::Mode::Internal),
        (0xf, _) => w.confirm = Some((super::Confirm::Dismount, false)),
        // mp_pmode (FUN_101414b0 .. FUN_101416e0): Chat "/tell <shown name> " on any target, then closed; Magic in cast
        // mode with every type; Abilities mp_abisc; Check as playermo's; Treasure only with items.
        (mon::CHAT, _) => {
            w.chat_input = super::monstrosity::name_of(w, w.target.unwrap_or(0)).filter(|_| w.target.is_some()).map_or(String::new(), |n| format!("/tell {n} "));
            w.chat_caret = None;
            w.typing = true;
        }
        (mon::ATTACK, Some((t, i))) => send(w, NetCommand::Act { target: t, index: i, action: 2 }),
        (mon::SWITCH, _) => return choose(w, cmd::SWITCH_TARGET),
        (mon::MAGIC, _) => super::magic::open_magic(w, 0, false, 0),
        (mon::ABILITIES, _) => super::magic::open_cats(w, "menu    mp_abisc"),
        (mon::DISENGAGE, _) => return choose(w, cmd::DISENGAGE),
        (mon::CHECK, _) if w.my_motion.status == 1 => {
            super::target::open_sub(w, 0x20, super::target::SubThen::Check, None, String::new(), 0);
        }
        (mon::CHECK, _) => check(w, None, 0, false),
        (mon::TREASURE, _) if super::treasure::count(w) > 0 => super::treasure::open(w),
        (mon::TREASURE | mon::ATTACK, _) => {}
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

    /// Table 0x1032d5f0 by the pick's low two bits.
    #[test]
    fn death_schedulers() {
        assert_eq!([0, 1, 2, 3, 5].map(|p| death_scheduler(p, false)), ["dead", "1ead", "2ead", "3ead", "1ead"]);
        assert_eq!((death_scheduler(0, true), death_scheduler(1, true)), ("sdep", "sdep"));
    }

    /// FUN_1009e700: us before the party, the party (same party number) before the alliance, the caster first.
    #[test]
    fn relations() {
        use super::super::party::{ListUpdate, Roster};
        const ME: u32 = 0x100;
        let mut r = Roster::default();
        r.refresh_self(ME, 0x400, "Me", 230);
        r.table(&[(ME, 0x400, 4, 230), (0x200, 0x401, 0, 230), (0x300, 0x402, 0, 230)]);
        let list = |id, index, flags| ListUpdate { id, index, flags, zone: 230, name: "X".into(), ..Default::default() };
        r.list(&list(ME, 0x400, 4), ME, false);
        r.list(&list(0x200, 0x401, 0), ME, false);
        // Party number 1 (flags & 3): an alliance member.
        r.list(&list(0x300, 0x402, 1), ME, false);
        let known = |id: u32| id != 0x0100_0999;
        let rel = |a, t| relation_in(ME, known, &r, a, t);
        let mob = 0x0100_0001;
        assert_eq!(rel(0x200, ME), Relation::TargetIsMe);
        assert_eq!(rel(ME, 0x200), Relation::CasterIsMe);
        assert_eq!(rel(0x200, mob), Relation::PartyCaster);
        assert_eq!(rel(mob, 0x200), Relation::PartyTarget);
        assert_eq!(rel(0x300, 0x200), Relation::PartyTarget);
        assert_eq!(rel(0x300, mob), Relation::AllianceCaster);
        assert_eq!(rel(mob, 0x300), Relation::AllianceTarget);
        assert_eq!(rel(mob, 0x500), Relation::Other);
        // A missing actor counts as others, even against us.
        assert_eq!(rel(0x0100_0999, ME), Relation::Other);
    }

    /// knockback_spec 1.2: from DAT_1035bc60, level 1 runs 47 ticks over 1.41 units (bf1 at tick 5), level 7 123 ticks
    /// over 10.61 (bf1 at 45); the same at 60 and 300 fps.
    #[test]
    fn knockback_levels() {
        let cfg = crate::config::Config::load();
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let t = crate::dat::tables::knockback(&dll).unwrap();
        for dt in [1.0f32, 0.2] {
            for (level, total, dist, recover) in [(1, 47.0, 1.41, 5.0), (7, 123.0, 10.61, 45.0)] {
                let row = t[level];
                let mut k = KnockState { vel: Vec3::X * row[0], speed: row[0], decay: row[1], left: row[2], origin: Vec3::ZERO, motion: "bf0".into(), carry: 0.0 };
                let (mut p, mut ticks, mut at) = (Vec3::ZERO, 0.0f32, None);
                loop {
                    ticks += dt;
                    let (q, r, done) = knock_step(&mut k, p, dt);
                    p = q;
                    if r {
                        at = Some(ticks);
                    }
                    if done || ticks > 400.0 {
                        break;
                    }
                }
                assert!((ticks - total).abs() <= 1.0 && (p.x - dist).abs() < 0.02, "level {level} dt {dt}: {ticks} ticks, {}", p.x);
                assert!(at.is_some_and(|a| (a - recover).abs() <= dt + 1e-3), "level {level} dt {dt}: bf1 at {at:?}");
            }
        }
    }
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
        let a = Action { actor: RABBIT, category: category::BASIC_ATTACK, arg: u32::from_le_bytes(*b"atk0"), info: 0, targets: vec![ActionTarget { id: ME, results: vec![hit, miss] }], seq: 0 };
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

        // Condensed (modern chat): the hits of a round as one line, misses counted.
        let ctx = |work: [i32; 5]| Ctx { actor: who(ME), target: who(RABBIT), me: "Tuxedo".into(), work, ..Default::default() };
        let line = say(1, ME, RABBIT, [0, 12, 0, 0, 0])[0].0.clone();
        let t = template_of(&bt, 1, rel(ME, RABBIT), &ctx([0, 12, 0, 0, 0]), 1, &line, &cfg).unwrap();
        assert_eq!(condensed_text(&t, &[12, 15, 9]), "Tuxedo hits the Wild Rabbit for 12, 15, 9 points of damage.");
        let miss = say(15, ME, RABBIT, [0; 5])[0].0.clone();
        let t = template_of(&bt, 15, rel(ME, RABBIT), &ctx([0; 5]), 1, &miss, &cfg).unwrap();
        assert_eq!(condensed_text(&t, &[0, 0]), "Tuxedo misses the Wild Rabbit. (x2)");
        // A line whose text is not the template with its number in is left alone.
        assert_eq!(template_of(&bt, 1, rel(ME, RABBIT), &ctx([0, 12, 0, 0, 0]), 1, "something else", &cfg), None);
    }

    /// FUN_101275b0 over the layouts, filtered as FUN_10127230 does for a solo player.
    #[test]
    fn action_menus() {
        // Outside Conflict (current_items' rule) Quarry / Scout / Sprint go.
        let peace = |v: Vec<u8>| v.into_iter().filter(|c| !matches!(c, 0x16..=0x18)).collect::<Vec<_>>();
        assert_eq!(peace(menu_items(4, Some(RABBIT))), [cmd::ATTACK, 8, 7, 0x19, 9, cmd::CHECK]);
        assert_eq!(peace(menu_items(6, Some(RABBIT))), [cmd::SWITCH_TARGET, 8, 7, 0x19, 9, cmd::DISENGAGE, cmd::CHECK]);
        assert_eq!(peace(menu_items(0, None)), [cmd::CHAT, 8, 7, 0x19, 9]);
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

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn battle_message_effects() {
        assert_eq!((effect_id(9, false), effect_id(9, true)), (Some(0x1fe), Some(0x388)));
        assert_eq!((effect_id(0xb, false), effect_id(0xb, true)), (Some(0x1fd), Some(0x1fd)));
        assert_eq!((effect_id(0x2d, true), effect_id(0x2d, false)), (Some(0x389), None));
        assert_eq!((effect_id(50, false), effect_id(719, false), effect_id(811, false), effect_id(813, true)), (Some(0x2fe), Some(0x3a0), Some(0x3f8), Some(0x3f9)));
        assert_eq!((effect_id(8, true), effect_id(1, false)), (None, None));
    }

    /// FUN_100c3480: a kill's level up waits for the mob's death, then 60 ticks; a GM level up 600; a gone target 60;
    /// other messages print on the first step.
    #[test]
    fn battle_message_waits() {
        let mut w = MsgWait::new(9, Some(1));
        assert_eq!(w, MsgWait { state: 1, left: 600.0 });
        for _ in 0..10 {
            assert!(!w.step(1.0, Some(1)));
        }
        assert!(!w.step(1.0, Some(3)));
        assert_eq!(w, MsgWait { state: 2, left: 60.0 });
        for _ in 0..59 {
            assert!(!w.step(1.0, Some(3)));
        }
        assert!(w.step(1.0, Some(3)));
        let mut w = MsgWait::new(9, Some(0));
        for _ in 0..600 {
            assert!(!w.step(1.0, Some(0)));
        }
        assert!(w.step(1.0, Some(0)));
        let mut w = MsgWait::new(9, None);
        for _ in 0..59 {
            assert!(!w.step(1.0, None));
        }
        assert!(w.step(1.0, None));
        assert!(MsgWait::new(8, Some(1)).step(1.0, Some(1)));
        // At 300 fps (0.2 ticks a frame) the 60 ticks after the death take 300 frames.
        let mut w = MsgWait::new(9, Some(1));
        assert!(!w.step(0.2, Some(2)));
        let n = (1..=400).find(|_| w.step(0.2, Some(2))).unwrap();
        assert!((299..=301).contains(&n), "{n}");
    }

    /// The handlers queue 0x32b / 0x32d as themselves only from 0x02D; else the Type byte (LSB: 0).
    #[test]
    fn queued_messages() {
        let pkt = |id: u8, msg: u16| {
            let mut b = vec![0u8; 0x1c];
            b[0] = id;
            b[1] = (0x1c / 4) << 1;
            b[0x18..0x1a].copy_from_slice(&msg.to_le_bytes());
            crate::net::map::session::SubPacket { id: id as u16, sync: 0, data: b }
        };
        assert_eq!(crate::client::net::battle_message(&pkt(0x29, 9)).queued, 9);
        assert_eq!(crate::client::net::battle_message(&pkt(0x29, 0x32b)).queued, 0);
        assert_eq!(crate::client::net::battle_message2(&pkt(0x2d, 0x32b)).queued, 0x32b);
        assert_eq!(crate::client::net::battle_message2(&pkt(0x2d, 6)).queued, 0);
    }
}
