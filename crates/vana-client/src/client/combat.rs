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
}

impl ChatLog {
    pub fn push(&mut self, s: impl Into<String>) {
        self.lines.push((s.into(), super::WHITE));
    }

    pub fn push_colour(&mut self, s: String, colour: [u8; 4]) {
        self.lines.push((s, colour));
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
    reactions: Vec<(u32, u32, u8)>,
}

fn who(w: &World, id: u32) -> Option<Who> {
    if id == w.my_id {
        return Some(Who { name: w.name.clone(), pc: true, ..Default::default() });
    }
    w.entities.get(&id).map(|e| Who { name: e.name.clone(), pc: id < 0x0100_0000, named: e.named, plural: e.plural })
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
    let ctx = Ctx { actor: who(w, m.actor), target: who(w, m.target), me: w.name.clone(), work: m.work };
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

/// A client-side battle message about ourselves (FUN_100a0150), e.g. 9 when a talk target is too far.
pub(super) fn system_message(w: &mut World, id: u16) {
    let me = w.my_id;
    print(w, Msg { id, actor: me, target: me, work: [0; 5] });
}

/// s2c 0x053 (LAB_100a06a0 -> FUN_100a0150): file 7031 text about ourselves, work[0..2] = para, para2; the
/// mode comes from its 1f code (e.g. #7 the logout countdown, mode 0x88).
pub(super) fn on_system_message(w: &mut World, id: u16, work: [i32; 2]) {
    let Some(bt) = w.battle.clone() else { return };
    let me = w.my_id;
    let ctx = Ctx { actor: who(w, me), target: who(w, me), me: w.name.clone(), work: [work[0], work[1], 0, 0, 0] };
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

/// 0x028: messages per (target, result) record (FUN_1009c730: main, additional effect, reaction) with
/// work = [cmd_arg, value, proc value, react value, bit]; melee waits for the attacker's hit.
pub(super) fn on_action(w: &mut World, a: &Action) {
    let messages: Vec<Msg> = action_messages(a).into_iter().map(|(id, target, work)| Msg { id, actor: a.actor, target, work }).collect();
    let mut reactions = Vec::new();
    for (target, r) in a.records() {
        let hits = matches!(a.category, category::BASIC_ATTACK | category::RANGED_FINISH | category::SKILL_FINISH | category::MOB_SKILL_FINISH);
        if hits && (r.miss == 0 && r.value > 0 || r.miss == 2) {
            reactions.push((target, a.actor, r.miss));
        }
    }
    let first = a.targets.first().map(|t| t.id);
    let attack = (a.category == category::BASIC_ATTACK).then(|| start_attack(w, a.actor, first)).flatten();
    // The action's schedulers drive its effects (fx/tasks.rs); op 0x24 plays the swing picked here.
    w.effects.on_action(a, attack.as_ref().map(|(_, n)| n.clone()));
    let delay = attack.map(|(d, _)| d);
    match delay {
        // dada at T: atpr waits 4, then op 0x2b prints; dam0 (op 0x01 wait 1) reacts at T + 5 (triggers.md 2.2).
        Some(d) => {
            let tick = Duration::from_secs_f32(1.0 / 60.0);
            let now = Instant::now();
            w.pending.push(Pending { at: now + d + tick * 4, messages, reactions: Vec::new() });
            w.pending.push(Pending { at: now + d + tick * 5, messages: Vec::new(), reactions });
        }
        None => {
            for m in messages {
                print(w, m);
            }
            for (t, by, miss) in reactions {
                react(w, t, by, miss);
            }
        }
    }
}

/// Fires pending hits whose time has come.
pub(super) fn tick(w: &mut World) {
    let now = Instant::now();
    let (due, rest): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut w.pending).into_iter().partition(|p| p.at <= now);
    w.pending = rest;
    for p in due {
        for m in p.messages {
            print(w, m);
        }
        for (t, by, miss) in p.reactions {
            react(w, t, by, miss);
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
pub(super) fn start_attack(w: &mut World, actor: u32, _target: Option<u32>) -> Option<(Duration, String)> {
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
    Some((Duration::from_secs_f32(plan.hit_at.unwrap_or(0) as f32 / 60.0), a.last_attack.clone()))
}

/// Damage reaction (FUN_100cbd20): "dfi?" / "dbi?" (front / back) standing, "dfm?" / "dbm?" while
/// moving (+0xf8); guard "gdi?" / "gdm?" (FUN_100cbe30). Front or back is taken from where the attacker
/// stands (inferred).
fn react(w: &mut World, target: u32, attacker: u32, miss: u8) {
    let (Some((tp, tdir)), Some((ap, _))) = (position(w, target), position(w, attacker)) else { return };
    let front = super::forward_from_dir(tdir).dot(ap - tp) >= 0.0;
    let moving = moving(w, target);
    let Some(a) = w.actors.get_mut(&target) else { return };
    let Some(ch) = a.character.clone() else { return };
    if a.oneshot.as_ref().is_some_and(|o| o.name.starts_with("ded")) {
        return;
    }
    let (i, m) = match (miss, front) {
        (2, _) => ("gdi", "gdm"),
        (_, true) => ("dfi", "dfm"),
        _ => ("dbi", "dbm"),
    };
    let order = if moving { [m, i] } else { [i, m] };
    if let Some(name) = order.into_iter().find(|n| !actmotion::find(&a.extra, &ch, n).is_empty()) {
        // dfi0 is a 2-key pose; hold it 24 ticks (the 24.0 in "damg"'s op 0x21 is our guess at its time).
        let len = motion_ticks(a, &ch, name, 1.0).max(24.0);
        play(a, name, len, 1.0);
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

/// 0x038 "kesu" (LSB FadeOut before a despawn): DAT 0's scheduler fades the actor to colour 0x808080
/// alpha 0 over 90 ticks (op 0x29).
pub(super) fn on_scheduler(w: &mut World, actor: u32, name: [u8; 4]) {
    w.effects.on_scheduler(actor, actor, u32::from_le_bytes(name));
    if &name == b"kesu"
        && let Some(a) = w.actors.get_mut(&actor)
    {
        a.fade = Some(actmotion::Fade::to(a.alpha, 0, 90));
    }
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionMenu {
    pub items: Vec<u8>,
    pub sel: usize,
}

/// Layout choice (FUN_10127790): engaged 6; an attackable target 4 (2 * 2); an NPC 7 (Talk first;
/// inferred: actor +0x1cc bits 2 and 0x100 taken as "not a monster"); another target 2; none 0.
fn layout(w: &World) -> usize {
    if w.my_motion.status == 1 {
        return 6;
    }
    match w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e))) {
        Some((_, e)) if e.mob && !matches!(e.motion.status, 2 | 3) => 4,
        Some((t, e)) if t >> 24 != 0 && !e.mob => 7,
        Some(_) => 2,
        None => 0,
    }
}

/// FUN_10127230's filters, as far as they apply to us (no treasure pool, trust, fellow or party).
fn shown(target: Option<u32>, c: u8) -> Option<u8> {
    let npc = target.is_some_and(|t| t >= 0x0100_0000);
    match c {
        // Chat becomes Tell on a player, and is dropped on an NPC.
        cmd::CHAT if npc => None,
        cmd::CHAT if target.is_some() => Some(cmd::TELL),
        cmd::CHECK if target.is_none() => None,
        // Talk: the target is an NPC that is not engaged (FUN_10127230 case 6).
        cmd::TALK if npc => Some(c),
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

pub(super) fn open_menu(w: &World) -> ActionMenu {
    ActionMenu { items: menu_items(layout(w), w.target.filter(|t| w.entities.contains_key(t))), sel: 0 }
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
        (cmd::SWITCH_TARGET, Some((t, i))) => send(w, NetCommand::Act { target: t, index: i, action: 0x0f }),
        (cmd::DISENGAGE, _) => {
            let (t, i) = target.unwrap_or((w.my_id, 0));
            send(w, NetCommand::Act { target: t, index: i, action: 4 });
        }
        (cmd::CHECK, Some((t, i))) => send(w, NetCommand::Check { target: t, index: i }),
        (cmd::CHAT | cmd::TELL, _) => w.typing = true,
        // FUN_10127b60 case 5: FUN_1007aac0 on the target.
        (cmd::TALK, Some((t, _))) => {
            super::event::talk(w, t);
        }
        // Case 8: the item list (FUN_101e9f10).
        (cmd::ITEMS, _) => super::items::open_items(w),
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
        let say = |id: u16, actor: u32, target: u32, work: [i32; 5]| render(&bt, id, rel(actor, target), &Ctx { actor: who(actor), target: who(target), me: "Tuxedo".into(), work }, &Default::default());
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
