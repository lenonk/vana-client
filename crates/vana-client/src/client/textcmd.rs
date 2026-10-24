//! Text commands as FUN_10080dc0 runs them: the tag pass, the 150-byte cut, tokens, the command table from
//! FFXiMain (0x10355408: name, id, flags), the state gate and the handlers. MODLOG "Text commands".

use super::net::NetCommand;
use super::{World, combat, event, target};
use std::collections::HashMap;
use std::sync::OnceLock;

/// FUN_10080dc0's mode: 0 menus, 1 typed, 2 macro, 3..7 a sub-target tag's re-run with the pick's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Internal,
    Typed,
    Macro,
    SubTarget,
}

/// Command flags (u16 @0x16).
mod flag {
    pub const IN_PROMPT: u16 = 0x02;
    pub const NAME_TAGS: u16 = 0x08;
    pub const UI_FREE: u16 = 0x10;
    pub const TARGET: u16 = 0x100;
    pub const NO_GATE: u16 = 0x1000;
}

/// Command ids.
mod id {
    pub const SAY: u16 = 0x01;
    pub const EMOTE_TEXT: u16 = 0x0b;
    pub const MAP: u16 = 0x4f;
    pub const MISSION: u16 = 0x4e;
    pub const QUEST: u16 = 0x4d;
    pub const KEYITEM: u16 = 0x4c;
    pub const RECYCLE: u16 = 0x7a;
    pub const MOUNT: u16 = 0x28;
    pub const DISMOUNT: u16 = 0x38;
    pub const DIG: u16 = 0x37;
    pub const SITCHAIR: u16 = 0x34;
    pub const JUMP: u16 = 0x35;
    pub const BANK: u16 = 0x6c;
    pub const LOCKER: u16 = 0x6d;
    pub const STORAGE: u16 = 0x6e;
    pub const SATCHEL: u16 = 0x6f;
    pub const BANK2: u16 = 0x7b;
    pub const MAILBOX: u16 = 0x7c;
    pub const LASTSYNTH: u16 = 0x9d;
    pub const SENDPOST: u16 = 0x7e;
    pub const SHOUT: u16 = 0x02;
    pub const YELL: u16 = 0x03;
    pub const TELL: u16 = 0x04;
    pub const PARTY: u16 = 0x05;
    pub const LS: u16 = 0x06;
    pub const LS2: u16 = 0x07;
    pub const UNITY: u16 = 0x08;
    pub const ASSISTJ: u16 = 0x09;
    pub const ASSISTE: u16 = 0x0a;
    pub const ECHO: u16 = 0x13;
    pub const RANDOM: u16 = 0x14;
    pub const MACRO: u16 = 0x16;
    pub const TARGET: u16 = 0x17;
    pub const SHOOT: u16 = 0x1c;
    pub const ATTACK: u16 = 0x1d;
    pub const ATTACKOFF: u16 = 0x1e;
    pub const ASSIST: u16 = 0x20;
    pub const HELP: u16 = 0x21;
    pub const ITEM: u16 = 0x22;
    pub const EQUIP: u16 = 0x24;
    pub const MAGIC: u16 = 0x25;
    pub const WS: u16 = 0x26;
    pub const NINJUTSU: u16 = 0x29;
    pub const SONG: u16 = 0x2a;
    pub const JA: u16 = 0x2b;
    pub const PET: u16 = 0x2c;
    pub const HEAL: u16 = 0x32;
    pub const SIT: u16 = 0x33;
    pub const LOGOUT: u16 = 0x43;
    pub const SHUTDOWN: u16 = 0x44;
    pub const CHECK: u16 = 0x45;
    pub const CHECKNAME: u16 = 0x46;
    pub const CHECKPARAM: u16 = 0x47;
    pub const PCMD: u16 = 0x56;
    pub const ACMD: u16 = 0x57;
    pub const JOIN: u16 = 0x58;
    pub const DECLINE: u16 = 0x59;
    pub const INVITE: u16 = 0x5d;
    pub const ANON: u16 = 0x62;
    pub const ONLINE: u16 = 0x69;
    pub const AWAY: u16 = 0x6a;
    pub const TIMESTAMP: u16 = 0x9f;
    pub const RECAST: u16 = 0x2e;
    pub const CLOCK: u16 = 0x3f;
    pub const SERVMES: u16 = 0x107;
    pub const LOCKON: u16 = 0x5c;
    pub const CHARAEFFECT: u16 = 0x8a;
    pub const WAIT: u16 = 0x10a;
    pub const QUARRY: u16 = 0x8c;
    pub const SCOUT: u16 = 0x8d;
    pub const SPRINT: u16 = 0x8e;
    pub const BLOCKHELP: u16 = 0x96;
    pub const NAMES: u16 = 0x1a8;
    pub const IGNOREPET: u16 = 0x1a9;
    pub const IGNOREFAITH: u16 = 0x1aa;
    pub const IGNORETRUST: u16 = 0x1ab;
    pub const AOE: u16 = 0x1ac;
    pub const VER: u16 = 0x1ae;
    pub const CHATMODE: u16 = 0x1af;
    pub const LOCALSETTINGS: u16 = 0x1c4;
    pub const HIDEFAITH: u16 = 0x65;
    pub const HIDETRUST: u16 = 0x66;
    pub const EMOTEFAITH: u16 = 0x67;
    pub const EMOTETRUST: u16 = 0x68;
    pub const HIDE: u16 = 0x6b;
    pub const PLAYTIME: u16 = 0x3e;
    pub const FURCC: u16 = 0x84;
    pub const LAYOUTCTRL: u16 = 0x85;
    pub const STATUSPARTY: u16 = 0x86;
    pub const STATUSTIMER: u16 = 0x88;
    pub const GTST: u16 = 0x89;
    pub const YELLSW: u16 = 0x91;
    pub const ASSISTJSW: u16 = 0x92;
    pub const ASSISTESW: u16 = 0x93;
    pub const BLOCKAID: u16 = 0x95;
    pub const BATTLEBGM: u16 = 0x97;
    pub const BELLSW: u16 = 0x98;
    pub const MUTEBGM: u16 = 0x99;
    pub const MUTESE: u16 = 0x9a;
    pub const SEARCH: u16 = 0x3a;
    pub const LSMES: u16 = 0x108;
    pub const LS2MES: u16 = 0x109;
}

/// 0x01A categories (LSB GP_CLI_COMMAND_ACTION ActionID).
mod cat {
    pub const ATTACK: u16 = 0x02;
    pub const MAGIC: u16 = 0x03;
    pub const DISENGAGE: u16 = 0x04;
    pub const HELP: u16 = 0x05;
    pub const WS: u16 = 0x07;
    pub const JA: u16 = 0x09;
    pub const ASSIST: u16 = 0x0c;
    pub const CHANGE_TARGET: u16 = 0x0f;
    pub const SHOOT: u16 = 0x10;
}

/// Name -> (id, flags).
pub type Table = HashMap<String, (u16, u16)>;

/// The table, found by its "/say" entry (id 1, flags 0xc3) with "/ver" before it; it ends at a negative id.
pub fn parse_table(dll: &[u8]) -> Option<Table> {
    let mut say = [0u8; 0x18];
    say[..4].copy_from_slice(b"/say");
    (say[0x14], say[0x16]) = (1, 0xc3);
    let at = dll.windows(0x18).position(|w| w == say)?.checked_sub(0x18)?;
    let mut t = HashMap::new();
    for e in dll[at..].chunks_exact(0x18) {
        let id = i16::from_le_bytes([e[0x14], e[0x15]]);
        if id < 0 {
            break;
        }
        let end = e[..0x14].iter().position(|&b| b == 0).unwrap_or(0x14);
        if end > 0 {
            t.insert(String::from_utf8_lossy(&e[..end]).into_owned(), (id as u16, u16::from_le_bytes([e[0x16], e[0x17]])));
        }
    }
    Some(t)
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).unwrap_or_default();
        parse_table(&dll).unwrap_or_else(|| {
            log::warn!("text command table not found in FFXiMain.dll");
            Table::new()
        })
    })
}

/// FUN_10076830: tokens split at spaces (and tabs outside typed lines); a token opening with a quote runs to the
/// matching one, and the quotes go (FUN_10076a70: only when the last char matches the first). At most 17; the
/// 17th takes the rest.
pub fn tokens(line: &str, tabs: bool) -> Vec<String> {
    let sep = |c: char| c == ' ' || (tabs && c == '\t');
    let strip = |t: &str| -> String {
        let q = t.chars().next().filter(|c| matches!(c, '"' | '\''));
        match q {
            Some(q) if t.len() >= 2 && t.ends_with(q) => t[1..t.len() - 1].to_string(),
            _ => t.to_string(),
        }
    };
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        rest = rest.trim_start_matches(sep);
        if rest.is_empty() {
            break;
        }
        if out.len() == 16 {
            out.push(strip(rest));
            break;
        }
        let q = rest.chars().next().filter(|c| matches!(c, '"' | '\''));
        let end = match q {
            Some(q) => rest[1..].find(q).map_or(rest.len(), |i| i + 2),
            None => rest.find(sep).unwrap_or(rest.len()),
        };
        out.push(strip(&rest[..end]));
        rest = &rest[end..];
    }
    out
}

/// FUN_100769f0: cut to 150 bytes without splitting a character.
fn cut(s: &mut String) {
    let mut n = s.len().min(150);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s.truncate(n);
}

/// FUN_10077c30: a name of 3..15 ASCII letters becomes Xxxx; anything else stays as typed (None).
pub(super) fn name_case(s: &str) -> Option<String> {
    if !(3..=15).contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = s.to_ascii_lowercase();
    out[..1].make_ascii_uppercase();
    Some(out)
}

/// FUN_10079a00: decimal, "0x" / "$" hex, "%" binary.
fn number(s: &str) -> Option<i64> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix('$')) {
        i64::from_str_radix(h, 16).ok()
    } else if let Some(b) = s.strip_prefix('%') {
        i64::from_str_radix(b, 2).ok()
    } else {
        s.parse().ok()
    }
}

fn by_index(w: &World, index: u16) -> Option<(u32, &super::Entity)> {
    w.entities.iter().find(|(_, e)| e.index == index).map(|(id, e)| (*id, e))
}

fn current_target(w: &World) -> Option<(u32, u16)> {
    let t = w.target?;
    if t == w.my_id {
        return Some((w.my_id, w.my_index));
    }
    w.entities.get(&t).map(|e| (t, e.index))
}

/// FUN_10077d80: the first living actor claimed by a party member.
fn battle_target(w: &World) -> Option<(u32, u16)> {
    let party: Vec<u32> = w.party.members().map(|m| m.id).filter(|&id| id != 0).chain([w.my_id]).collect();
    let mut found: Vec<(u32, u16)> = w.entities.iter().filter(|(_, e)| e.hpp > 0 && e.claim != 0 && party.contains(&e.claim)).map(|(id, e)| (*id, e.index)).collect();
    found.sort_by_key(|f| f.1);
    found.first().copied()
}

fn pet(w: &World) -> Option<(u32, u16)> {
    (w.my_pet != 0).then(|| by_index(w, w.my_pet).map(|(id, _)| (id, w.my_pet))).flatten()
}

/// The name a flag-0x08 command gets for a pick (`"%s\"%s\""`).
pub fn display_name(w: &World, id: u32) -> String {
    name_of(w, (id, 0)).unwrap_or_default()
}

fn name_of(w: &World, t: (u32, u16)) -> Option<String> {
    if t.0 == w.my_id {
        return Some(w.name.clone());
    }
    w.entities.get(&t.0).map(|e| e.name.clone())
}

/// FUN_10076ff0: a tag's text, None to leave it literal. In a target command the targets stay for the resolver.
fn tag(w: &World, tag: &str, target_cmd: bool) -> Option<String> {
    let party_slot = |i: usize| w.party.slots().slots.get(i).cloned().flatten().map(|m| m.name).filter(|n| !n.is_empty());
    let stats = w.stats.as_ref();
    let vit = w.vitals;
    let job = |(j, lv): (u8, u8)| (j != 0).then(|| format!("{}{lv}", super::items::JOBS.get(j as usize).copied().unwrap_or("")));
    match tag {
        "<t>" if !target_cmd => current_target(w).and_then(|t| name_of(w, t)),
        "<me>" if !target_cmd => Some(w.name.clone()),
        "<bt>" if !target_cmd => battle_target(w).and_then(|t| name_of(w, t)),
        "<pet>" if !target_cmd => pet(w).and_then(|t| name_of(w, t)),
        // FUN_10076ff0 type 0x16: the zone NPC-list name of the call-for-help monster.
        "<ht>" if !target_cmd => super::target::call_for_help(w).and_then(|id| w.entities.get(&id)).and_then(|e| w.npc_names.iter().find(|(n, _)| (*n & 0x3ff) as u16 == e.index & 0x3ff)).map(|(_, n)| n.clone()),
        "<r>" => w.tells.first().cloned(),
        "<tp>" => vit.map(|v| v.2.to_string()),
        "<hp>" => Some(format!("{}/{}", vit?.0, stats?.hp_max)),
        "<mp>" => Some(format!("{}/{}", vit?.1, stats?.mp_max)),
        "<hpp>" => vit.map(|v| format!("{}%", v.3)),
        "<mpp>" => vit.map(|v| format!("{}%", v.4)),
        "<mjob>" => job(stats?.main),
        "<sjob>" => job(stats?.sub),
        "<job>" => {
            let s = stats?;
            Some(match job(s.sub) {
                Some(sub) => format!("{}/{sub}", job(s.main)?),
                None => job(s.main)?,
            })
        }
        "<pos>" if !w.room_flag => Some(w.hud_top.grid().to_string()).filter(|g| !g.is_empty()),
        // FUN_10076ff0 type 0x15: the tracked target's name from the zone NPC list.
        "<scan>" => w.scan.tracking().and_then(|t| w.npc_names.iter().find(|(id, _)| (*id & 0x3ff) as u16 == t.index & 0x3ff).map(|(_, n)| n.clone())),
        _ => {
            // <p0>..<p5>, <a10>..<a15>, <a20>..<a25>: party window slots 0..17.
            let inner = tag.strip_prefix('<')?.strip_suffix('>')?;
            let slot = match inner.as_bytes() {
                [b'p', d @ b'0'..=b'5'] => (d - b'0') as usize,
                [b'a', p @ b'1'..=b'2', d @ b'0'..=b'5'] => (p - b'0') as usize * 6 + (d - b'0') as usize,
                _ => return None,
            };
            party_slot(slot)
        }
    }
}

/// FUN_10076ab0: every <...> (a space ends it outside double quotes, a second '<' gives up) is replaced by its
/// text, or left as it is.
fn expand(w: &World, line: &str, target_cmd: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while let Some(off) = line[i..].find('<') {
        let at = i + off;
        out.push_str(&line[i..at]);
        let mut quoted = false;
        let mut close = None;
        for (k, c) in line[at + 1..].char_indices() {
            match c {
                '"' => quoted = !quoted,
                ' ' if !quoted => break,
                '<' => break,
                '>' => {
                    close = Some(at + 1 + k);
                    break;
                }
                _ => {}
            }
        }
        match close.and_then(|j| Some((j, tag(w, &line[at..=j], target_cmd)?))) {
            Some((j, text)) => {
                out.push_str(&text);
                i = j + 1;
            }
            None => {
                out.push('<');
                i = at + 1;
            }
        }
    }
    out.push_str(&line[i..]);
    out
}

/// FUN_1007a1c0(text, fallback, wide): a target tag, a player's name (with `wide` also our trusts and pet), a
/// server id from a menu line, else with `fallback` the current target.
fn resolve(w: &World, text: Option<&str>, fallback: bool, wide: bool, mode: Mode) -> Option<(u32, u16)> {
    if let Some(s) = text {
        let t = match s {
            "<t>" => current_target(w),
            "<me>" => Some((w.my_id, w.my_index)),
            "<bt>" => battle_target(w),
            // FUN_1007a1c0: the tracked actor while tracking, when it is here.
            "<scan>" => w.scan.tracking().and_then(|t| w.entities.iter().find(|(_, e)| e.index == t.index).map(|(id, e)| (*id, e.index))),
            "<pet>" => pet(w),
            "<ht>" => super::target::call_for_help(w).and_then(|id| w.entities.get(&id).map(|e| (id, e.index))),
            _ => None,
        };
        if t.is_some() {
            return t;
        }
        let name = name_case(s).unwrap_or_else(|| s.chars().take(23).collect());
        if name == w.name {
            return Some((w.my_id, w.my_index));
        }
        // PCs are 0x400..0x6ff; 0x700..0x8ff (trusts, pets) only with `wide` and when ours or the party's (inf:
        // +0x12c bit 22 / FUN_1008e260).
        let ours = |id: u32, index: u16| index == w.my_pet || w.party.members().any(|m| m.id == id);
        let hit = w.entities.iter().find(|(id, e)| {
            let range = (0x400..0x700).contains(&e.index) || (wide && (0x700..0x900).contains(&e.index) && ours(**id, e.index));
            let same = e.name == name || (wide && name_case(&e.name).is_some_and(|n| n == name));
            range && e.targetable() && same
        });
        if let Some((id, e)) = hit {
            return Some((*id, e.index));
        }
        if matches!(mode, Mode::Internal | Mode::SubTarget)
            && let Some(n) = number(s)
        {
            let id = n as u32;
            if id == w.my_id {
                return Some((w.my_id, w.my_index));
            }
            if let Some(e) = w.entities.get(&id) {
                return Some((id, e.index));
            }
        }
    }
    if fallback { current_target(w) } else { None }
}

fn send(w: &World, c: NetCommand) {
    let _ = w.net.commands.send(c);
}

/// FUN_100780d0: 0x01A {u32 id, u16 ActIndex, u16 category, u32 param, 3 x f32}.
fn act(w: &World, t: (u32, u16), category: u16, param: u32) {
    let mut body = super::net::action_body(t.0, t.1, category);
    body[8..12].copy_from_slice(&param.to_le_bytes());
    log::info!("0x01A category {category:#x} param {param} on {:#x} / {:#x}", t.0, t.1);
    send(w, NetCommand::Packet { id: 0x01A, body });
}

/// The chat mode of a group 7 refusal (table 0x103763b0, 4-byte entries {id, 3, 0, mode}): the target / recast lines
/// 0x56..0x5f, 0xb5, 0xb7, 0xbd, 0xbf are 0x7a, the status lines and the rest 0x7b.
pub(super) fn refusal_mode(id: usize) -> u8 {
    match id {
        0x56..=0x5f | 0xb5 | 0xb7 | 0xbd | 0xbf => 0x7a,
        _ => 0x7b,
    }
}

/// A group 7 line (FUN_10129e30) in its chat mode.
pub(super) fn refuse(w: &mut World, id: usize) {
    let Some(bt) = w.battle.clone() else { return };
    let Some(line) = bt.group7(id) else { return };
    let colour = bt.colour_in(refusal_mode(id), &w.game);
    w.chat.push_colour(line, colour);
}

/// FUN_101cc0d0(mask): the status checks of action commands, the first that applies prints its line.
pub(super) fn status_gate(w: &mut World, mask: u8) -> bool {
    let s = w.my_motion.status;
    let line = if mask & 0x10 != 0 && matches!(s, 0x21 | 7) {
        0x51
    } else if mask & 0x20 != 0 && (s == 0x2f || (0x3f..=0x53).contains(&s)) {
        0xa0
    } else if mask & 0x01 != 0 && matches!(s, 5 | 0x55) {
        0x4b
    } else if mask & 0x02 != 0 && s == 6 {
        0x4d
    } else if mask & 0x04 != 0 && s == 0x2c {
        0x4e
    } else if mask & 0x08 != 0 && w.room_flag {
        0x44
    } else {
        return true;
    };
    refuse(w, line);
    false
}

/// FUN_10081230: dead (0x20), mounted (0x40) and event (0x80) states a command's flags don't allow; then UI-free.
fn state_gate(w: &mut World, flags: u16) -> bool {
    if flags & flag::NO_GATE != 0 {
        return true;
    }
    let s = w.my_motion.status;
    let state = if w.dead() { 0x20 } else { 0 } | if matches!(s, 5 | 0x55) { 0x40 } else { 0 } | if event::active(w) { 0x80 } else { 0 };
    let bad = state & !flags;
    if bad != 0 {
        if bad & 0x20 != 0 {
            refuse(w, 0x4f);
        } else if bad & 0x40 != 0 {
            refuse(w, 0x4b);
        } else if s == 6 {
            refuse(w, 0x4d);
        } else {
            refuse(w, 0x43);
        }
        return false;
    }
    if flags & flag::UI_FREE != 0 && w.full_log.is_some() {
        refuse(w, 0x42);
        return false;
    }
    true
}

/// Runs a line (FUN_10080dc0).
pub fn run(w: &mut World, line: &str, mode: Mode) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    // A line without a command goes to the chat mode (/chatmode).
    let line = if line.starts_with('/') { line.to_string() } else { format!("{}{line}", w.chat_prefix) };
    let first = line.split([' ', '\t']).next().unwrap_or("");
    let first_flags = table().get(first).map_or(0, |e| e.1);
    let target_cmd = first_flags & flag::TARGET != 0;
    // With the prompt up a typed line runs only with flag 0x02 (else group 7 0xa4); a macro line is dropped.
    if target::sub_up(w) && matches!(mode, Mode::Typed | Mode::Macro) && first_flags & flag::IN_PROMPT == 0 {
        if mode == Mode::Typed {
            refuse(w, 0xa4);
        }
        return;
    }
    // A sub-target tag in a target command (or a flag-0x08 one) opens the prompt with the text before it.
    if (target_cmd || first_flags & flag::NAME_TAGS != 0)
        && mode != Mode::SubTarget
        && let Some((at, mask)) = sub_tag(&line)
    {
        open_prompt(w, &line[..at], mask, target_cmd, first_flags & flag::NAME_TAGS != 0, mode);
        return;
    }
    let mut expanded = expand(w, &line, target_cmd);
    cut(&mut expanded);
    let args = tokens(&expanded, mode != Mode::Typed);
    let Some(name) = args.first() else { return };
    // Unknown commands print nothing.
    let Some(&(cid, flags)) = table().get(name.as_str()) else {
        log::info!("{name}: no such command");
        return;
    };
    log::info!("text command {name} ({mode:?})");
    // Ours: LSB GM commands (a say starting with '!') skip the K.O. / mounted / event gate.
    let gm = cid == id::SAY && args.get(1).is_some_and(|a| a.starts_with('!'));
    if !gm && !state_gate(w, flags) {
        return;
    }
    if handle(w, cid, &args, &expanded, mode).is_err() {
        error(w, &expanded, mode);
    }
}

/// FUN_10076ab0's sub-target tags (prefix compare at each '<'): `<st>` 0x7f, `<stpc>` 0x1d, `<stnpc>` 0xa0,
/// `<stpt>` 5, `<stal>` 0xd: where the first one starts, and its target flags.
fn sub_tag(line: &str) -> Option<(usize, u32)> {
    const TAGS: [(&str, u32); 5] = [("<st>", 0x7f), ("<stpc>", 0x1d), ("<stnpc>", 0xa0), ("<stpt>", 5), ("<stal>", 0xd)];
    line.match_indices('<').find_map(|(i, _)| TAGS.iter().find(|(t, _)| line[i..].starts_with(t)).map(|(_, m)| (i, *m)))
}

/// The tag's prompt: refused in an event (group 7 0x43) or a place without actions (0x44); the stash must be a
/// command. A /ma line's dry run gives the arrow (or the party hand) its spell range. Nothing fits: 0x46 "You must
/// specify a valid target." and a macro stops. `<stpt>` / `<stal>` (masks 5 / 0xd, which retail never reads) open
/// the party-list cursor instead of the 3D prompt (FUN_10076ab0 modes 6 / 7 -> FUN_10202d30).
fn open_prompt(w: &mut World, stash: &str, mask: u32, target_cmd: bool, quoted: bool, mode: Mode) {
    if event::active(w) {
        refuse(w, 0x43);
        return;
    }
    if w.room_flag {
        refuse(w, 0x44);
        return;
    }
    let stash = expand(w, stash, target_cmd);
    if !stash.starts_with('/') {
        return;
    }
    let args = tokens(&stash, mode != Mode::Typed);
    let spell = match args.first().and_then(|a| table().get(a.as_str())).map(|e| e.0) {
        Some(id::MAGIC | id::NINJUTSU | id::SONG) => args.get(1).and_then(|n| find_spell(w, n, 0).ok()),
        _ => None,
    };
    let range = spell.and_then(|s| w.actions.as_ref()?.spell(s)).map(|r| crate::dat::actions::RANGES[(r.range & 0xf) as usize] as f32);
    let then = target::SubThen::Line { stash, quoted, from_macro: mode == Mode::Macro };
    if matches!(mask, 5 | 0xd) {
        target::open_party_cursor(w, if mask == 5 { 6 } else { 18 }, then, range);
        return;
    }
    if !target::open_sub(w, mask, then, range, String::new(), 0) {
        refuse(w, 0x46);
        if mode == Mode::Macro
            && let Some(m) = w.macros.as_mut()
        {
            m.stop();
        }
    }
}

/// A failed command: ">> line" and group 9 0x78 for typed and macro lines, only 0x78 for a menu's (mode 0x9d).
fn error(w: &mut World, expanded: &str, mode: Mode) {
    if mode == Mode::Internal {
        let Some(bt) = w.battle.clone() else { return };
        let colour = bt.colour_in(0x9d, &w.game);
        w.chat.push_colour(bt.group9(0x78).unwrap_or_default(), colour);
    } else {
        combat::command_error(w, expanded);
    }
}

/// The text after the command word (FUN_10078350).
fn tail(line: &str, words: usize) -> &str {
    let mut rest = line;
    for _ in 0..words {
        rest = rest.trim_start().split_once(' ').map_or("", |(_, r)| r);
    }
    rest
}

fn on_off(arg: Option<&String>) -> u32 {
    match arg.map(String::as_str) {
        Some("on") => 1,
        Some("off") => 2,
        _ => 0,
    }
}

type Done = Result<(), ()>;

fn handle(w: &mut World, cid: u16, args: &[String], line: &str, mode: Mode) -> Done {
    let arg = |i: usize| args.get(i).map(String::as_str);
    match cid {
        id::SAY | id::SHOUT | id::YELL | id::PARTY | id::LS | id::LS2 | id::UNITY | id::ASSISTJ | id::ASSISTE | id::EMOTE_TEXT => chat(w, cid, tail(line, 1)),
        id::TELL => tell(w, args, line),
        id::ECHO => {
            let text = tail(line, 1).to_string();
            if !text.is_empty()
                && let Some(bt) = w.battle.clone()
            {
                let colour = bt.colour_in(0xce, &w.game);
                w.chat.push_colour(text, colour);
            }
            Ok(())
        }
        id::WAIT => Ok(()),
        // FUN_1007d2c0: the map as the main menu opens it, unless in a room (group 7 0x44) or already open.
        // FUN_1007d220: evitem.
        id::KEYITEM => {
            super::keyitems::open(w);
            Ok(())
        }
        // FUN_1007d240: quest00 in mode 0.
        id::QUEST => {
            super::quests::open(w);
            Ok(())
        }
        // FUN_1007d270: miss00 alone, on the log titled by the one argument, else the nation's.
        id::MISSION => {
            w.mission_cmd = Some(if args.len() == 2 { args.get(1).cloned() } else { None });
            Ok(())
        }
        id::MAP => {
            if w.room_flag {
                refuse(w, 0x44);
            } else if w.map.is_none() {
                w.map_cmds.push(super::map::Cmd::Menu);
            }
            Ok(())
        }
        // FUN_1007bb90: /mount <name>: gate 0x7f (prints and counts as done), the mount by its 55681 name (any case),
        // c2s 0x01A category 0x1A with the mount index on ourselves.
        id::MOUNT => {
            let Some(name) = arg(1) else { return Err(()) };
            if !status_gate(w, 0x7f) {
                return Ok(());
            }
            let names = w.names.clone().ok_or(())?;
            let want = name.trim_matches('"').to_ascii_lowercase();
            let idx = (0..names.mount_count() as u32).find(|&i| names.mount(i).is_some_and(|m| m.0.to_ascii_lowercase() == want)).ok_or(())?;
            act(w, (w.my_id, w.my_index), 0x1a, idx);
            Ok(())
        }
        // FUN_1007aaf0: /dig on a chocobo (status 5): c2s 0x01A category 0x11 (LSB ChocoboDig) through FUN_1009fa70;
        // its place checks (refusal index 8 = group 7 0x4a) are left to the server here (inferred).
        id::DIG => {
            if w.my_motion.status == 5 {
                act(w, (w.my_id, w.my_index), 0x11, 0);
            }
            Ok(())
        }
        // /sitchair [n]: c2s 0x113 {u32 mode 0 (toggle), u32 chair n (0 the stool)} (LSB's layout; the handler's own
        // checks are not traced).
        id::SITCHAIR => {
            let n = arg(1).and_then(|a| a.parse::<u32>().ok()).unwrap_or(0);
            let mut b = vec![0u8; 8];
            b[4..8].copy_from_slice(&n.to_le_bytes());
            send(w, NetCommand::Packet { id: 0x113, body: b });
            Ok(())
        }
        // /jump: a client-only motion (not done here).
        // FUN_1007c4f0: after SendResRdy, status gate 0x77, standing or engaged; else the handler fails.
        id::JUMP => {
            if !w.res_rdy.sent {
                return Err(());
            }
            if status_gate(w, 0x77) {
                if !matches!(w.my_motion.status, 0 | 1) {
                    return Err(());
                }
                super::jump(w);
            }
            Ok(())
        }
        // FUN_1007aa80: /dismount: category 0x12 on ourselves, no client checks.
        id::DISMOUNT => {
            act(w, (w.my_id, w.my_index), 0x12, 0);
            Ok(())
        }
        // FUN_1007e250 .. FUN_1007e440 (flags 0x51): the storage views; a 0 from the opener is the failure path.
        id::BANK | id::LOCKER | id::STORAGE => {
            let c = match cid {
                id::BANK => 1,
                id::LOCKER => 4,
                _ => 2,
            };
            if super::items::open_dae10(w, c, false) { Ok(()) } else { Err(()) }
        }
        // /satchel needs ItemNum[5] >= 30; /sack, /case, /wardrobe, /wardrobe2..8, /recycle (0x7a, the bin).
        id::SATCHEL..=id::RECYCLE => {
            let c = match cid {
                0x6f..=0x72 => (cid - 0x6a) as u8,
                0x73..=0x79 => (cid - 0x69) as u8,
                _ => 17,
            };
            if super::items::open_daf70(w, c, c == 5, false) { Ok(()) } else { Err(()) }
        }
        // /bank2 (FUN_1007e440): without Safe 2 (s2c 0x067 @0x27 bit 0) group 7 0xa8, counted as done.
        // FUN_1007e490 / FUN_1007e560 (/mailbox, /deliverybox, /sendpost): one's own Mog House, not visiting (0xbc),
        // the status gate, the job-change cooldown, the moogle home (0x48); then c2s 0x04D PostOpen / DeliOpen.
        id::LASTSYNTH => {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            super::synth::lastsynth(w, &a)
        }
        id::MAILBOX | id::SENDPOST => {
            if w.myroom.is_none() {
                return Err(());
            }
            if w.mh_owner != 0 {
                refuse(w, 0xbc);
                return Err(());
            }
            if !status_gate(w, 0x44) {
                return Ok(());
            }
            if super::moogle::job_cooling(w) {
                return Err(());
            }
            if !w.zone.is_some_and(super::items::moogle_zone) {
                refuse(w, 0x48);
                return Ok(());
            }
            super::post::open_cmd(w, if cid == id::MAILBOX { 0xe } else { 0xd });
            Ok(())
        }
        id::BANK2 if !w.mh_floors => {
            refuse(w, 0xa8);
            Ok(())
        }
        id::BANK2 => {
            if super::items::open_dae10(w, 9, false) { Ok(()) } else { Err(()) }
        }
        id::RANDOM => {
            let v = arg(1).map_or(0, |a| number(a).unwrap_or(-1)) as u32;
            send(w, NetCommand::Packet { id: 0x0A2, body: v.to_le_bytes().to_vec() });
            Ok(())
        }
        id::MACRO => {
            // FUN_10079f10: "/macro set|book N", exactly three tokens; out of range wraps once.
            let n = arg(2).and_then(|a| a.parse::<usize>().ok()).filter(|n| *n > 0);
            match (args.len(), arg(1), n, w.macros.as_mut()) {
                (3, Some("set"), Some(n), Some(m)) => {
                    let book = m.book;
                    m.switch(book, n - 1);
                    Ok(())
                }
                (3, Some("book"), Some(n), Some(m)) => {
                    let set = m.set;
                    m.switch(n - 1, set);
                    Ok(())
                }
                _ => Err(()),
            }
        }
        id::LOCKON => {
            let on = !w.locked;
            target::lock(w, on, "/lockon");
            Ok(())
        }
        id::CHARAEFFECT => {
            // FUN_1007ed40: config 0x99 bit 7, which gates only the forced flash; group 9 0x137 / 0x136.
            let v = w.game.get(0x99);
            let off = v & 0x80 == 0;
            w.game.set(0x99, if off { v | 0x80 } else { v & !0x80 });
            w.game_save = true;
            if let Some(bt) = w.battle.clone() {
                w.chat.push(bt.group9(if off { 0x137 } else { 0x136 }).unwrap_or_default());
            }
            Ok(())
        }
        // FUN_1007cf80: not while making deliveries (group 7 0x54) or checking the box (0x55).
        id::LOGOUT | id::SHUTDOWN if super::post::logout_refusal(w).is_some() => {
            refuse(w, super::post::logout_refusal(w).unwrap_or(0x54));
            Ok(())
        }
        id::LOGOUT | id::SHUTDOWN => {
            // c2s 0x0E7 {Mode, Kind}: none 0 toggle, "on" 1 (3 for shutdown), "off" 2; status 0, 7 or 0x21 only.
            let shutdown = cid == id::SHUTDOWN;
            let m = match (on_off(args.get(1)), shutdown) {
                (1, true) => 3,
                (m, _) => m as u16,
            };
            if matches!(w.my_motion.status, 0 | 7 | 0x21) {
                w.logging_out = match m {
                    0 => !w.logging_out,
                    2 => false,
                    _ => true,
                };
                // The kind the last request asked for decides what the server's logout does (exit or the login).
                if m != 2 {
                    w.shutdown = shutdown;
                }
                send(w, NetCommand::Logout { mode: m, kind: if shutdown { 3 } else { 1 } });
            }
            Ok(())
        }
        id::HEAL => {
            // FUN_1007c220: gate 0x60; standing, status 7 or healing; c2s 0x0E8.
            if status_gate(w, 0x60) && matches!(w.my_motion.status, 0 | 7 | 0x21) {
                send(w, NetCommand::Heal(on_off(args.get(1))));
            }
            Ok(())
        }
        id::SIT => {
            // FUN_1007c2f0: gate 0x50; standing or sitting; c2s 0x0EA.
            if status_gate(w, 0x50) && matches!(w.my_motion.status, 0 | 0x2f) {
                send(w, NetCommand::Packet { id: 0x0EA, body: on_off(args.get(1)).to_le_bytes().to_vec() });
            }
            Ok(())
        }
        id::ATTACK => attack(w, args, mode),
        id::ATTACKOFF => {
            act(w, (w.my_id, w.my_index), cat::DISENGAGE, 0);
            Ok(())
        }
        id::HELP => {
            act(w, (w.my_id, w.my_index), cat::HELP, 0);
            Ok(())
        }
        id::ASSIST => {
            // FUN_1007aa30: nothing while engaged; never an error.
            if w.my_motion.status != 1
                && let Some(t) = resolve(w, arg(1), true, false, mode)
            {
                act(w, t, cat::ASSIST, 0);
            }
            Ok(())
        }
        id::TARGET => {
            // FUN_1007a150: nothing while engaged; no target resets it.
            if w.my_motion.status != 1 {
                let t = resolve(w, arg(1), true, false, mode).map(|t| t.0);
                target::set(w, t);
            }
            Ok(())
        }
        // FUN_1007d000 / FUN_1007d050: through FUN_101347b0 (repeat guard, players and monsters only).
        id::CHECK | id::CHECKNAME => {
            let t = resolve(w, arg(1), true, false, mode).ok_or(())?;
            super::combat::check(w, Some(t.0), (cid - id::CHECK) as u8, true);
            Ok(())
        }
        id::CHECKPARAM => {
            let t = resolve(w, arg(1), true, false, mode).ok_or(())?;
            send(w, NetCommand::Check { target: t.0, index: t.1, kind: 2 });
            Ok(())
        }
        // FUN_1007ef50 and its neighbours: Conflict only (else group 7 0xa7); c2s 0x01A on us, 0x15 / 0x17 / 0x16.
        id::QUARRY | id::SCOUT | id::SPRINT => {
            if super::combat::conflict(w.my_team) {
                let action = match cid {
                    id::QUARRY => 0x15,
                    id::SCOUT => 0x17,
                    _ => 0x16,
                };
                let index = w.my_index;
                send(w, NetCommand::Act { target: w.my_id, index, action });
            } else {
                refuse(w, 0xa7);
            }
            Ok(())
        }
        // FUN_1007f480: exactly one "on" / "off", else a toggle; group 9 0xf7 / 0xf8.
        id::BLOCKHELP => {
            let cur = w.block_help;
            toggle(w, only_on_off(args), cur, |w, on| w.block_help = on, [0xf7, 0xf8]);
            Ok(())
        }
        // FUN_100790e0: on shows, off hides, else toggles; hidden with "otherpc" (the last word) only other players'.
        id::NAMES => {
            let hide = match on_off(args.get(1)) {
                1 => false,
                2 => true,
                _ => w.names_hidden.is_none(),
            };
            let last = args.get(if args.len() == 2 { 1 } else { 2 });
            w.names_hidden = hide.then(|| last.is_some_and(|a| a.eq_ignore_ascii_case("otherpc")));
            Ok(())
        }
        // FUN_100791d0: config 0x97 (Tab skips others' pets: not applied, ours has no pet owner).
        id::IGNOREPET => {
            let cur = w.game.get(0x97) != 0;
            switch(w, on_off(args.get(1)), cur, |w, on| cfg(w, 0x97, on as u32), [0xea, 0xeb], [0xe8, 0xe9]);
            Ok(())
        }
        // FUN_10079270: config 0xaa (Tab skips other players' trusts: not applied, as above).
        id::IGNOREFAITH | id::IGNORETRUST => {
            let cur = w.game.get(0xaa) != 0;
            switch(w, on_off(args.get(1)), cur, |w, on| cfg(w, 0xaa, on as u32), [0x10d, 0x10e], [0x10b, 0x10c]);
            Ok(())
        }
        // FUN_100793a0: config 0xc0, the prompt's area-of-effect display.
        id::AOE => {
            let cur = w.game.get(0xc0) == 1;
            toggle(w, on_off(args.get(1)), cur, |w, on| cfg(w, 0xc0, on as u32), [0x129, 0x12a]);
            Ok(())
        }
        // FUN_1007de50: config 0x99 bit 5 hides other players' trusts (not applied yet).
        id::HIDEFAITH | id::HIDETRUST => {
            let cur = bit(w, 0x99, 5);
            switch(w, on_off(args.get(1)), cur, |w, on| cfg_bit(w, 0x99, 5, on), [0x111, 0x112], [0x10f, 0x110]);
            Ok(())
        }
        // FUN_1007dee0: config 0x99 bit 9, set = trust emotes not linked.
        id::EMOTEFAITH | id::EMOTETRUST => {
            let cur = !bit(w, 0x99, 9);
            switch(w, on_off(args.get(1)), cur, |w, on| cfg_bit(w, 0x99, 9, !on), [0x14b, 0x14c], [0x14b, 0x14c]);
            Ok(())
        }
        // FUN_1007e9b0: config 0x99 bit 6, set = no camera collision with furniture.
        id::FURCC => {
            let cur = !bit(w, 0x99, 6);
            switch(w, on_off(args.get(1)), cur, |w, on| cfg_bit(w, 0x99, 6, !on), [0x12d, 0x12e], [0x12f, 0x130]);
            Ok(())
        }
        // FUN_1007ea40: config 0xc1, type1 0 / type2 1; anything else reports.
        id::LAYOUTCTRL => {
            match arg(1) {
                Some("type1") => {
                    cfg(w, 0xc1, 0);
                    log9(w, 0x131);
                }
                Some("type2") => {
                    cfg(w, 0xc1, 1);
                    log9(w, 0x132);
                }
                _ => log9(w, if w.game.get(0xc1) == 0 { 0x133 } else { 0x134 }),
            }
            Ok(())
        }
        // FUN_1007eae0 / FUN_1007ec10: configs 0xc2 / 0xc3.
        id::STATUSPARTY | id::STATUSTIMER => {
            let (cfg_id, lines) = if cid == id::STATUSPARTY { (0xc2, [0x138, 0x139]) } else { (0xc3, [0x13c, 0x13d]) };
            let cur = w.game.get(cfg_id) != 0;
            toggle(w, on_off(args.get(1)), cur, |w, on| cfg(w, cfg_id, on as u32), lines);
            Ok(())
        }
        // FUN_1007ec90: config 0xc4 (atoi, 1..100), or the current value.
        id::GTST => {
            let Some(bt) = w.battle.clone() else { return Ok(()) };
            let (line, n) = match arg(1) {
                Some(v) => {
                    let n = atoi(v).clamp(1, 100) as u32;
                    cfg(w, 0xc4, n);
                    (0x13e, n)
                }
                None => (0x13f, w.game.get(0xc4)),
            };
            let colour = bt.colour_in(0x79, &w.game);
            w.chat.push_colour(printf(&bt.group9(line).unwrap_or_default(), &[n.to_string()]), colour);
            Ok(())
        }
        // FUN_1007f0e0 / FUN_1007f1a0 / FUN_1007f260: chat filter bits 0x2e / 0x31 / 0x32 (the Chat Filters page's).
        id::YELLSW | id::ASSISTJSW | id::ASSISTESW => {
            let (b, lines) = match cid {
                id::YELLSW => (0x2e, [0xf9, 0xfa]),
                id::ASSISTJSW => (0x31, [0x153, 0x154]),
                _ => (0x32, [0x155, 0x156]),
            };
            let (cfg_id, b) = if b < 30 { (0x32, b) } else { (0x33, b - 30) };
            let cur = bit(w, cfg_id, b);
            toggle(w, on_off(args.get(1)), cur, |w, on| cfg_bit(w, cfg_id, b, on), lines);
            Ok(())
        }
        // FUN_1007f3e0: c2s 0x01A category 0x18 on us, off 0 / on 1 / else 2 (toggle); the server answers.
        id::BLOCKAID => {
            let param = match only_on_off(args) {
                1 => 1,
                2 => 0,
                _ => 2,
            };
            act(w, (w.my_id, w.my_index), 0x18, param);
            Ok(())
        }
        // FUN_1007f500: config 0x99 bit 0, battle music for our own engagement only (ours always plays it so).
        id::BATTLEBGM => {
            let cur = bit(w, 0x99, 0);
            toggle(w, only_on_off(args), cur, |w, on| cfg_bit(w, 0x99, 0, on), [0xf2, 0xf3]);
            Ok(())
        }
        // FUN_1007f5c0: config 0x99 bit 1, bell sounds (scheduler op 0xb3).
        id::BELLSW => {
            let cur = bit(w, 0x99, 1);
            toggle(w, only_on_off(args), cur, |w, on| cfg_bit(w, 0x99, 1, on), [0xf4, 0xf5]);
            Ok(())
        }
        // FUN_1007f680 / FUN_1007f750: exactly one "on" mutes, "off" unmutes, another word reports; none toggles.
        id::MUTEBGM | id::MUTESE => {
            let bgm = cid == id::MUTEBGM;
            let cur = if bgm { w.bgm_mute } else { w.se_mute };
            let mute = match (args.len(), arg(1)) {
                (2, Some("on")) => true,
                (2, Some("off")) => false,
                (2, _) => cur,
                _ => !cur,
            };
            if bgm {
                w.bgm_mute = mute;
            } else {
                w.se_mute = mute;
            }
            w.volume_dirty = true;
            log9(w, if bgm { if mute { 0x147 } else { 0x148 } } else if mute { 0x149 } else { 0x14a });
            Ok(())
        }
        // FUN_10078170: "version %s" in mode 0xbf.
        id::VER => {
            if let Some(bt) = w.battle.clone() {
                let colour = bt.colour_in(0xbf, &w.game);
                w.chat.push_colour(format!("version {}", w.client_version), colour);
            }
            Ok(())
        }
        id::PLAYTIME => playtime(w),
        id::SEARCH => {
            super::search::command(w, tail(line, 1));
            Ok(())
        }
        id::LSMES | id::LS2MES => {
            let n = if cid == id::LSMES { 1 } else { 2 };
            super::linkshell::lsmes(w, n, args, tail(line, 1), tail(line, 2));
            Ok(())
        }
        id::CHATMODE => chat_mode(w, args),
        // FUN_1007e120: the away flag cleared, like /online, and Msg std 0x79 (no PlayOnline friend list here).
        id::HIDE => {
            config_flag(w, 1, false);
            system_line(w, 0x79);
            Ok(())
        }
        id::LOCALSETTINGS => local_settings(w, args),
        id::SHOOT => {
            // FUN_1007a730: no gate, target required.
            let t = resolve(w, arg(1), false, false, mode).ok_or(())?;
            act(w, t, cat::SHOOT, 0);
            Ok(())
        }
        id::MAGIC | id::NINJUTSU | id::SONG => magic(w, cid, args, mode),
        id::EQUIP => equip(w, args),
        id::PCMD | id::ACMD => party(w, if cid == id::PCMD { 0 } else { 5 }, args, mode),
        id::JOIN | id::DECLINE => {
            // FUN_1007d640 / FUN_1007d670: c2s 0x074 {u8 Res 1 / 0}; the invite is no longer pending.
            send(w, NetCommand::Packet { id: 0x074, body: vec![(cid == id::JOIN) as u8, 0, 0, 0] });
            w.invite = None;
            Ok(())
        }
        id::ANON | id::INVITE => {
            // FUN_1007d7e0 / FUN_1007d6a0: "on" sets, "off" clears, no argument toggles, anything else does nothing.
            // /invite (the seek-party flag) only outside a party (inf: FUN_10201690() == 2).
            if cid == id::INVITE && w.party.members().count() > 1 {
                return Err(());
            }
            let (which, bit) = if cid == id::ANON { (0, 5) } else { (2, 4) };
            let set = match (args.len(), args.get(1).map(String::as_str)) {
                (2, Some("on")) => true,
                (2, Some("off")) => false,
                (2, _) => return Ok(()),
                _ => w.self_flags >> bit & 1 == 0,
            };
            config_flag(w, which, set);
            Ok(())
        }
        id::AWAY | id::ONLINE => {
            // FUN_1007e080 / FUN_1007e0d0: the away flag and Msg std 0x77 / 0x76.
            config_flag(w, 1, cid == id::AWAY);
            system_line(w, if cid == id::AWAY { 0x77 } else { 0x76 });
            Ok(())
        }
        id::RECAST => recast(w, args),
        id::CLOCK => clock(w, args),
        // FUN_10080340 (/servmes, /smes): a transfer unless one runs; no line of its own.
        id::SERVMES => {
            super::servmes::start(w);
            Ok(())
        }
        id::TIMESTAMP => {
            // FUN_1007b620: "off" / "min" / "sec" set config 0xb5 = 0 / 1 / 2, none cycles; group 9 0x122 + it.
            let v = match args.get(1).map(String::as_str) {
                Some("off") => 0,
                Some("min") => 1,
                Some("sec") => 2,
                Some(_) => return Err(()),
                None => (w.game.get(0xb5) + 1) % 3,
            };
            w.game.set(0xb5, v);
            w.game_save = true;
            log9(w, 0x122 + v as usize);
            Ok(())
        }
        id::ITEM => item(w, args, mode),
        id::WS => weapon_skill(w, args, mode),
        id::JA | id::PET => ability(w, cid == id::PET, args, mode),
        c if is_emote(c) => emote(w, c, args, mode),
        _ => {
            log::info!("{}: not supported yet", args[0]);
            Ok(())
        }
    }
}

/// Chat channels (FUN_10078260 and the others): c2s 0x0B5 and the local echo, which the server does not send.
fn chat(w: &mut World, cid: u16, msg: &str) -> Done {
    let msg = msg.trim();
    if msg.is_empty() {
        return Err(());
    }
    let kind: u8 = match cid {
        id::SAY => 0,
        id::SHOUT => 1,
        id::YELL => 0x1a,
        id::PARTY => 4,
        id::LS => 5,
        id::LS2 => 0x1b,
        id::UNITY => 0x21,
        id::ASSISTJ => 0x22,
        id::EMOTE_TEXT => 8,
        _ => 0x23,
    };
    // FUN_100e3400: /party with fewer than 2 in the party (FUN_100e8f70(5)) prints 7031 0x24 and sends nothing; the
    // caller still echoes the line.
    // FUN_100e9730: a linkshell channel needs its linkshell equipped (an item still at its slot), else 7031 0x25.
    let ls = match kind {
        5 => Some(1),
        0x1b => Some(2),
        0x1e => Some(3),
        _ => None,
    };
    let no_ls = ls.is_some_and(|n: usize| {
        let (index, bag) = w.linkshells[n];
        index == 0 || !w.inv.slots.get(&(bag, index)).is_some_and(|s| s.0 != 0)
    });
    let send_it = !(kind == 4 && w.party.party().count() < 2) && !no_ls;
    if kind == 4 && w.party.party().count() < 2 {
        system_line(w, 0x24);
    } else if no_ls {
        system_line(w, 0x25);
    }
    // The echoes (all.c 90072..90740): our name and the text as typed, in the sender's mode. Not done: the assist
    // channels' rank icon (s2c 0x01B ranks, MentorFlg).
    let me = w.name.clone();
    let (line, mode) = match cid {
        id::YELL => {
            let zone = w.zone.and_then(|z| w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, z as u32))).unwrap_or_default();
            (format!("{me}[{zone}]: {msg}"), 3)
        }
        id::SHOUT => (format!("{me} : {msg}"), 2),
        id::PARTY => (format!("({me}) {msg}"), 5),
        id::LS => (format!("[1]<{me}> {msg}"), 6),
        id::LS2 => (format!("[2]<{me}> {msg}"), 0xd5),
        id::UNITY => (format!("{{{me}}} {msg}"), 0xd3),
        id::ASSISTJ => (format!("{me}(J) : {msg}"), 0xdb),
        id::ASSISTE => (format!("{me}(E) : {msg}"), 0xdd),
        // FUN_10078f20 (/emote): "%s %s", mode 7.
        id::EMOTE_TEXT => (format!("{me} {msg}"), 7),
        _ => (format!("{me} : {msg}"), 1),
    };
    super::push_chat_mode(w, line, mode, kind);
    if send_it {
        send(w, NetCommand::Chat { kind, text: msg.to_string() });
        restore_temp_mode(w);
    }
    Ok(())
}

/// FUN_101adc30: the chat mode's line prefix (1 custom is kept as typed; 4 tell takes the name).
fn mode_prefix(mode: u8) -> &'static str {
    match mode {
        3 => "/shout ",
        5 => "/party ",
        6 => "/linkshell ",
        7 => "/emote ",
        8 => "/message ",
        9 => "/yell ",
        0xa => "//worldmes ",
        0xb => "/unity ",
        0xc => "/linkshell2 ",
        0xe => "/assistj ",
        0xf => "/assiste ",
        _ => "/say ",
    }
}

/// FUN_101ad670(mode, name): the previous mode is kept unless the current one is shout or message (one line only);
/// tell takes "/tell Name " and the current tell name, any other mode clears it.
pub(super) fn set_chat_mode(w: &mut World, mode: u8, name: Option<&str>) {
    if !matches!(w.chat_mode, 3 | 8) {
        w.chat_prev = (w.chat_mode, w.chat_prefix.clone());
    }
    w.chat_mode = mode;
    if mode == 4 {
        w.chat_prefix = name.map_or(String::new(), |n| format!("/tell {n} "));
        w.tell_name = name.map(str::to_string);
    } else {
        w.chat_prefix = mode_prefix(mode).into();
        w.tell_name = None;
    }
}

/// FUN_101adb80, after a channel send: a shout or message mode goes back to the previous mode.
fn restore_temp_mode(w: &mut World) {
    if matches!(w.chat_mode, 3 | 8) {
        (w.chat_mode, w.chat_prefix) = w.chat_prev.clone();
    }
}

/// FUN_100785a0: "/tell Name text"; a malformed name prints group 9 0x65 and fails.
fn tell(w: &mut World, args: &[String], line: &str) -> Done {
    if args.len() < 3 {
        return Err(());
    }
    let Some(name) = name_case(&args[1]) else {
        if let Some(bt) = w.battle.clone() {
            let colour = bt.colour_in(0x79, &w.game);
            w.chat.push_colour(bt.group9(0x65).unwrap_or_default(), colour);
        }
        return Err(());
    };
    let msg = tail(line, 2).trim().to_string();
    super::push_chat_mode(w, format!(">>{name} : {msg}"), 4, 3);
    // FUN_1018d6e0: the sent-tell names, newest first, 8 at most (exact match moves to the front).
    w.tell_hist.retain(|n| *n != name);
    w.tell_hist.insert(0, name.clone());
    w.tell_hist.truncate(8);
    send(w, NetCommand::Tell { name, text: msg });
    restore_temp_mode(w);
    Ok(())
}

/// FUN_1007a7a0: "/attack" toggles, "on" / "off", or "/attack <target>" attacks or switches to it.
fn attack(w: &mut World, args: &[String], mode: Mode) -> Done {
    if !status_gate(w, 0x44) {
        return Ok(());
    }
    let engaged = w.my_motion.status == 1;
    let me = (w.my_id, w.my_index);
    let start = |w: &mut World, t: (u32, u16)| {
        act(w, t, cat::ATTACK, 0);
        target::set(w, Some(t.0));
    };
    match on_off(args.get(1)) {
        2 => act(w, me, cat::DISENGAGE, 0),
        1 => {
            if let Some(t) = current_target(w).or_else(|| resolve(w, args.get(2).map(String::as_str), true, false, mode)) {
                start(w, t);
            }
        }
        _ if args.len() == 2 => {
            let t = resolve(w, Some(&args[1]), true, false, mode).ok_or(())?;
            // FUN_101cc220 (inf: a living monster).
            if !w.entities.get(&t.0).is_some_and(|e| e.mob && e.hpp > 0) {
                refuse(w, 0x95);
                return Ok(());
            }
            act(w, t, if engaged { cat::CHANGE_TARGET } else { cat::ATTACK }, 0);
            target::set(w, Some(t.0));
        }
        _ if engaged => act(w, me, cat::DISENGAGE, 0),
        _ => {
            if let Some(t) = current_target(w) {
                start(w, t);
            }
        }
    }
    Ok(())
}

/// Table 0x1036f7a8 {group, id, slot}, group 0x17's rows: slot name id -> equipment slot. (Group 0x18's alternate
/// spellings, ids 0..0x13, have no file mapped in ours.)
const SLOT_OF: [u8; 16] = [0, 1, 2, 3, 4, 9, 11, 12, 5, 6, 13, 14, 15, 10, 7, 8];

/// The bags /equip looks in, and its third argument's choices ("0" inventory, "1" wardrobe 8, "2".."8" 10..16).
const EQUIP_BAGS: [u8; 9] = [0, 8, 10, 11, 12, 13, 14, 15, 16];

/// A bag slot holding an item named `name` (record name, case-insensitive), in the bags' order.
fn find_item(w: &World, name: &str, bags: &[u8]) -> Option<(u8, u8, u16)> {
    let info = w.item_info.as_ref()?;
    bags.iter().find_map(|&bag| w.inv.slots.iter().filter(|((c, i), _)| *c == bag && *i != 0).find(|(_, (id, _))| info.item(*id).is_some_and(|it| it.name.eq_ignore_ascii_case(name))).map(|((c, i), (id, _))| (*c, *i, *id)))
}

/// FUN_1007bc10: /recast NAME: a spell (FUN_1022b430, then FUN_1022b2f0: (ticks + 59) / 60) or an ability
/// (FUN_10222e20 / FUN_10222c90) as "[%s] %s" with FUN_101c93c0's time ("%d:%02d", "%d:%02d:%02d" from an hour),
/// the name as typed, mode 0x79; a name known but not usable (not learned, out of the jobs' reach) "[%s] --:--".
/// Not done: an ability's charges ("[%s] %s (%d)"), which we do not keep.
/// printf's %s / %d / %% over string arguments (the group 10 lines that go through sprintf).
pub(super) fn printf(f: &str, args: &[String]) -> String {
    let (mut out, mut k, mut it) = (String::new(), 0, f.chars().peekable());
    while let Some(c) = it.next() {
        match (c, it.peek()) {
            ('%', Some('%')) => {
                it.next();
                out.push('%');
            }
            ('%', Some('s' | 'd')) => {
                it.next();
                out.push_str(args.get(k).map_or("", |a| a.as_str()));
                k += 1;
            }
            _ => out.push(c),
        }
    }
    out
}

/// FUN_1007cba0: two lines in mode 0x8c, group 10 0x42 "Vana'diel: date, weekday, time / moon phase (percent)" and
/// 0x3a "Earth: date time" (formats by config 0x95 date order and 0x96 12-hour); then no argument flips the compass
/// clock, "on" / "off" (exact) set it.
fn clock(w: &mut World, args: &[String]) -> Done {
    use crate::dat::msgfmt;
    let Some(bt) = w.battle.clone() else { return Ok(()) };
    let g = |id: u16| bt.group10_raw(id as usize);
    let (date_key, h12) = ((w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0);
    let t = super::vana_now(w);
    let p = msgfmt::VanaParts::of(t);
    let dec = |b: Vec<u8>| crate::dat::font::decode_sjis(&b);
    let date = dec(msgfmt::vana_datetime(&g, t, 2, date_key));
    let time = dec(g(0x46).map(|s| msgfmt::format(&s, &[], &[p.hour, p.minute], &|_, _, _| None)).unwrap_or_default());
    let vana = printf(&bt.group10(0x42).unwrap_or_default(), &[date, bt.day_name(p.weekday).unwrap_or_default(), time, bt.moon_name(p.phase).unwrap_or_default(), p.moon.to_string()]);
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let earth = printf(&bt.group10(0x3a).unwrap_or_default(), &[dec(msgfmt::earth_datetime(&g, unix, 1, date_key, h12))]);
    let colour = bt.colour_in(0x8c, &w.game);
    w.chat.push_colour(vana, colour);
    w.chat.push_colour(earth, colour);
    match (args.len(), args.get(1).map(String::as_str)) {
        (1, _) => w.compass_clock = !w.compass_clock,
        (2, Some("on")) => w.compass_clock = true,
        (2, Some("off")) => w.compass_clock = false,
        _ => {}
    }
    Ok(())
}

fn recast(w: &mut World, args: &[String]) -> Done {
    let name = args.get(1).ok_or(())?.clone();
    let left = |t: Option<&std::time::Instant>| t.map_or(0, |t| t.saturating_duration_since(std::time::Instant::now()).as_secs_f32().ceil() as u32);
    let mut unusable = false;
    let mut secs = None;
    let spell = w.names.as_ref().zip(w.actions.as_ref()).and_then(|(names, acts)| {
        acts.spells
            .iter()
            .filter(|s| s.id != 0 && s.levels.iter().any(|&l| l >= 0))
            .find(|s| crate::dat::names::NameSource::spell(&**names, s.id as u32).is_some_and(|n| n.eq_ignore_ascii_case(&name)))
            .map(|s| s.id)
    });
    if let Some(id) = spell {
        match find_spell(w, &name, 0) {
            Ok(_) => secs = Some(left(w.spell_recast.get(&id))),
            Err(()) => unusable = true,
        }
    }
    if secs.is_none()
        && let Some(id) = ability_named(w, &name, 0x200..0x600)
    {
        let rec = w.actions.as_ref().and_then(|a| a.abilities.get(id as usize)).map(|a| a.timer);
        let bit = id.wrapping_sub(0x200) as usize;
        let known = id >= 0x400 || w.commands_known.get(64 + bit / 8).is_some_and(|b| b & (1 << (bit % 8)) != 0);
        match rec {
            Some(t) if known => secs = Some(if t == 900 { 0 } else { left(w.ability_recast.get(&t)) }),
            _ => unusable = true,
        }
    }
    let line = match secs {
        Some(s) if s >= 3600 => format!("[{name}] {}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60),
        Some(s) => format!("[{name}] {}:{:02}", s / 60, s % 60),
        None if unusable => format!("[{name}] --:--"),
        None => return Err(()),
    };
    if let Some(bt) = w.battle.clone() {
        let colour = bt.colour_in(0x79, &w.game);
        w.chat.push_colour(line, colour);
    }
    Ok(())
}

pub(super) fn log9(w: &mut World, id: usize) {
    if let Some(bt) = w.battle.clone() {
        let colour = bt.colour_in(0x79, &w.game);
        w.chat.push_colour(bt.group9(id).unwrap_or_default(), colour);
    }
}

/// FUN_1007b390: /equip slot (unequip, group 9 0x44 "Equipment removed.") or /equip slot item [bag] (c2s 0x050, 0x43
/// "Equipment changed."); gate 0x46; already worn: nothing. Not done: the client's job / Dual Wield checks (group 7
/// 0x8b / 0x98 / 0x99, the server refuses those too), the equipment-message setting.
fn equip(w: &mut World, args: &[String]) -> Done {
    if !status_gate(w, 0x46) {
        return Ok(());
    }
    let bt = w.battle.clone().ok_or(())?;
    let want = args.get(1).ok_or(())?;
    let k = (0..16).find(|&k| bt.slot_name(k).is_some_and(|n| n.eq_ignore_ascii_case(want))).ok_or(())?;
    let slot = SLOT_OF[k];
    let Some(name) = args.get(2) else {
        if let Some((c, i)) = w.inv.equip[slot as usize] {
            let name = w.inv.slots.get(&(c, i)).and_then(|(id, _)| w.item_info.as_ref()?.item(*id)).map(|it| it.name.clone());
            log::info!("/equip: removing {name:?} from slot {slot}");
            send(w, NetCommand::Equip { index: 0, slot, container: c });
            log9(w, 0x44);
        }
        return Ok(());
    };
    let bags: Vec<u8> = match args.get(3).map(String::as_str) {
        None => EQUIP_BAGS.to_vec(),
        Some(b) if b.len() == 1 && b.as_bytes()[0].is_ascii_digit() && b.as_bytes()[0] <= b'8' => vec![EQUIP_BAGS[(b.as_bytes()[0] - b'0') as usize]],
        Some(_) => return Err(()),
    };
    let Some((bag, index, _)) = find_item(w, name, &bags) else { return Ok(()) };
    if w.inv.equip[slot as usize] == Some((bag, index)) {
        return Ok(());
    }
    send(w, NetCommand::Equip { index, slot, container: bag });
    log9(w, 0x43);
    Ok(())
}

/// FUN_1007adc0: /item name target, from the temporary bag, the inventory, then the wardrobes; the target is required.
/// An item that isn't usable given to an NPC is a trade (c2s 0x036, inventory only), else c2s 0x037 {target, 0,
/// ActIndex, slot, bag}, with the cast bar for an item with a use time (mode 1, its byte x 20).
fn item(w: &mut World, args: &[String], mode: Mode) -> Done {
    if !status_gate(w, 0x7f) {
        return Ok(());
    }
    let name = args.get(1).ok_or(())?;
    let (bag, slot, id) = find_item(w, name, &[3, 0, 8, 10, 11, 12, 13, 14, 15, 16]).ok_or(())?;
    let t = resolve(w, args.get(2).map(String::as_str), false, false, mode).ok_or(())?;
    let _ = id;
    super::items::use_slot(w, bag, slot, t.0);
    Ok(())
}

/// FUN_100e46a0(which, set): c2s 0x0DC, flag bytes +4..+7 and SetFlg @0x10 (1 set, 2 clear): which 0 anonymous (+4
/// bit 2), 1 away (bit 1), 2 seeking a party (bit 0), 6 autogroup (+5 bit 7), 0xb request mode (+7 bit 5).
pub(super) fn config_flag(w: &World, which: u8, set: bool) {
    let mut b = vec![0u8; 16];
    match which {
        0 => b[0] = 4,
        1 => b[0] = 2,
        6 => b[1] = 0x80,
        0xb => b[3] = 0x20,
        _ => b[0] = 1,
    }
    b[12] = if set { 1 } else { 2 };
    send(w, NetCommand::Packet { id: 0x0DC, body: b });
}

/// Sets a game config value (FUN_10193830): saved with cnf.dat and applied.
pub(super) fn cfg(w: &mut World, id: u16, v: u32) {
    if w.game.set(id, v) {
        w.game_save = true;
        super::cfgapply::apply(w, id);
    }
}

fn cfg_bit(w: &mut World, id: u16, b: u32, on: bool) {
    let v = w.game.get(id);
    cfg(w, id, if on { v | 1 << b } else { v & !(1 << b) });
}

fn bit(w: &World, id: u16, b: u32) -> bool {
    w.game.get(id) >> b & 1 != 0
}

/// The inline compare some handlers use: "on" / "off" only as the one argument (1 / 2), else 0.
fn only_on_off(args: &[String]) -> u32 {
    if args.len() == 2 { on_off(args.get(1)) } else { 0 }
}

/// "on" / "off" set the state (`on` = the state "on" asks for) and print `lines[0]` / `[1]`; else report `cur`.
fn switch(w: &mut World, word: u32, cur: bool, set: impl FnOnce(&mut World, bool), lines: [usize; 2], report: [usize; 2]) {
    if word == 0 {
        log9(w, report[!cur as usize]);
    } else {
        set(w, word == 1);
        log9(w, lines[(word - 1) as usize]);
    }
}

/// "on" / "off" set the state, anything else flips it; then `lines[0]` for on, `[1]` for off.
fn toggle(w: &mut World, word: u32, cur: bool, set: impl FnOnce(&mut World, bool), lines: [usize; 2]) {
    let on = match word {
        1 => true,
        2 => false,
        _ => !cur,
    };
    set(w, on);
    log9(w, lines[!on as usize]);
}

/// C atoi (FUN_103128b2): leading digits, junk 0.
fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, d) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let n = d.bytes().take_while(u8::is_ascii_digit).take(18).fold(0i64, |a, c| a * 10 + (c - b'0') as i64);
    if neg { -n } else { n }
}

/// FUN_1007cab0: PlayTime from the 0x00A plus the time since, group 10 0x2e (h, m, s) or 0x2f (with days), mode 0x8b.
fn playtime(w: &mut World) -> Done {
    use crate::dat::msgfmt;
    let (Some(bt), Some((base, at))) = (w.battle.clone(), w.play_time) else { return Ok(()) };
    let t = base as i64 + at.elapsed().as_secs() as i64;
    let (d, h, m, s) = ((t / 86400) as i32, (t % 86400 / 3600) as i32, (t % 3600 / 60) as i32, (t % 60) as i32);
    let (id, ints) = if d == 0 { (0x2e, vec![h, m, s]) } else { (0x2f, vec![d, h, m, s]) };
    let Some(raw) = bt.group10_raw(id) else { return Ok(()) };
    let text = crate::dat::font::decode_sjis(&msgfmt::format(&raw, &[], &ints, &|_, _, _| None));
    let colour = bt.colour_in(0x8b, &w.game);
    w.chat.push_colour(text, colour);
    Ok(())
}

/// FUN_10080830: no argument prints "(chatmode : prefix)" (mode 0x9d); else the mode by its word (table 0x10357ac8),
/// tell with a name (bad spelling: group 9 0x65 and a failure). Unknown words do nothing.
fn chat_mode(w: &mut World, args: &[String]) -> Done {
    let Some(bt) = w.battle.clone() else { return Ok(()) };
    let colour = bt.colour_in(0x9d, &w.game);
    let Some(word) = args.get(1) else {
        let line = format!("(chatmode : {})", w.chat_prefix);
        w.chat.push_colour(line, colour);
        return Ok(());
    };
    let mode = match word.as_str() {
        "say" | "s" => 2,
        "shout" | "sh" => 3,
        "party" | "p" => 5,
        "linkshell" | "l" => 6,
        "emote" | "em" => 7,
        "message" | "me" => 8,
        "unity" | "u" => 0xb,
        "linkshell2" | "l2" => 0xc,
        "assistj" | "aj" => 0xe,
        "assiste" | "ae" => 0xf,
        "tell" | "t" => {
            let Some(name) = args.get(2).and_then(|n| name_case(n)) else {
                w.chat.push_colour(bt.group9(0x65).unwrap_or_default(), colour);
                return Err(());
            };
            set_chat_mode(w, 4, Some(&name));
            return Ok(());
        }
        _ => return Ok(()),
    };
    set_chat_mode(w, mode, None);
    Ok(())
}

/// FUN_100808e0: Config values by name (table 0x10357b80); a value needs exactly three words, hidearmor and blureffect
/// toggle without one and are the only ones that print. Anything else is a failure.
fn local_settings(w: &mut World, args: &[String]) -> Done {
    let (three, val) = (args.len() == 3, args.get(2).map(String::as_str));
    match (args.get(1).map(String::as_str), val) {
        (Some("shadows"), Some(v)) if three => {
            let n = match v {
                "normal" => 0,
                "off" => 1,
                "high" => 2,
                _ => return Err(()),
            };
            cfg(w, 0x3a, n);
        }
        (Some("charanum"), Some(v)) if three => cfg(w, 0x3c, atoi(v).clamp(25, 50) as u32),
        (Some("distance"), Some(v)) if three => cfg(w, 0x3e, (atoi(v) as f32 * 2.0).clamp(0.0, 25.0) as u32),
        (Some("footeffect"), Some(v)) if three => {
            let n = match v {
                "on" => 0,
                "off" => 1,
                _ => return Err(()),
            };
            cfg(w, 0x3f, n);
        }
        (Some(which @ ("hidearmor" | "blureffect")), _) if args.len() <= 3 => {
            let (id, lines) = if which == "hidearmor" { (0xa5, [0xff, 0x100]) } else { (0xc8, [0x145, 0x146]) };
            let on = match (three, val) {
                (true, Some("on")) => true,
                (true, Some("off")) => false,
                (false, _) => w.game.get(id) == 0,
                _ => return Err(()),
            };
            cfg(w, id, on as u32);
            log9(w, lines[!on as usize]);
        }
        _ => return Err(()),
    }
    Ok(())
}

/// A message-std line printed locally (FUN_100a1ab0, kind 6).
pub(super) fn system_line(w: &mut World, id: u16) {
    let Some(bt) = w.battle.clone() else { return };
    let ctx = crate::dat::battle::Ctx { me: w.name.clone(), ..Default::default() };
    if let Some(line) = bt.system(id, &ctx) {
        let colour = bt.colour_in(line.mode.unwrap_or(0x79), &w.game);
        for l in line.lines.into_iter().filter(|l| !l.is_empty()) {
            w.chat.push_colour(l, colour);
        }
    }
}

/// FUN_1007d3e0: /pcmd (kind 0) and /acmd (kind 5): add [target] (0x06E; an NPC gets group 7 0x9f), leave (0x06F),
/// breakup (0x070, leaders only), kick / leader / looter Name (0x071 / 0x077 ChangeKind 0 or 1 / 4). A party of only
/// us and trusts refuses them (group 7 0x9e). An unknown or missing subcommand does nothing. An add whose name isn't
/// here goes through the search server (client/search.rs).
fn party(w: &mut World, kind: u8, args: &[String], mode: Mode) -> Done {
    let others: Vec<u16> = w.party.members().filter(|m| m.id != w.my_id && m.id != 0).map(|m| m.index).collect();
    if !others.is_empty() && others.iter().all(|&i| i >= 0x700) {
        refuse(w, 0x9e);
        return Ok(());
    }
    let named = |w: &mut World, i: usize| -> Result<String, ()> {
        match args.get(i).and_then(|n| name_case(n)) {
            Some(n) => Ok(n),
            None => {
                log9(w, 0x65);
                Err(())
            }
        }
    };
    match args.get(1).map(String::as_str) {
        Some("add") => {
            let Some(t) = resolve(w, args.get(2).map(String::as_str), true, false, mode) else {
                // Nobody here by that name: the search server's name lookup (FUN_1007c740, LAB_1007d5b0).
                let Some(name) = args.get(2) else { return Err(()) };
                super::search::lookup(w, name, super::search::Then::PartyAdd);
                return Ok(());
            };
            if t.0 >> 24 == 1 {
                refuse(w, 0x9f);
                return Ok(());
            }
            send(w, NetCommand::Packet { id: 0x06E, body: super::party::invite_body(t.0, t.1, kind) });
        }
        Some("leave") => send(w, NetCommand::Packet { id: 0x06F, body: vec![kind, 0, 0, 0] }),
        Some("breakup") => {
            let leader = if kind == 0 { 4 } else { 8 };
            if w.party.own_flags & leader != 0 {
                send(w, NetCommand::Packet { id: 0x070, body: vec![kind, 0, 0, 0] });
            }
        }
        Some("kick") => {
            let name = named(w, 2)?;
            send(w, NetCommand::Packet { id: 0x071, body: super::party::kick_body(kind, &name) });
        }
        Some(sub @ ("leader" | "looter")) => {
            let name = named(w, 2)?;
            let change = if sub == "looter" { 4 } else { (kind == 5) as u8 };
            send(w, NetCommand::Packet { id: 0x077, body: super::party::change_body(&name, kind, change) });
        }
        _ => {}
    }
    Ok(())
}

/// FUN_1007fcc0's command ids.
fn is_emote(c: u16) -> bool {
    matches!(c, 0xa2..=0xc8 | 0xcd | 0xce | 0xe3..=0xe6 | 0xeb | 0xec | 0x102)
}

/// /bell's notes (table 0x1032ca18, _stricmp): the param, 6..0x1e.
const BELL: [(&str, u16); 51] = [
    ("c3", 19), ("c3+", 26), ("d3-", 26), ("d3", 20), ("d3+", 27), ("e3-", 27), ("e3", 21), ("f3-", 21), ("f3", 22), ("e3+", 22),
    ("f3+", 28), ("g3-", 28), ("g3", 23), ("g3+", 29), ("a3-", 29), ("a3", 24), ("a3+", 30), ("b3-", 30), ("b3", 25), ("c4-", 25),
    ("b3+", 6), ("c4", 6), ("c4+", 14), ("d4-", 14), ("d4", 7), ("d4+", 15), ("e4-", 15), ("e4", 8), ("f4-", 8), ("e4+", 9),
    ("f4", 9), ("f4+", 16), ("g4-", 16), ("g4", 10), ("g4+", 17), ("a4-", 17), ("a4", 11), ("a4+", 18), ("b4-", 18), ("b4", 12),
    ("c5-", 12), ("b4+", 13), ("c5", 13), ("1", 6), ("2", 7), ("3", 8), ("4", 9), ("5", 10), ("6", 11), ("7", 12), ("8", 13),
];

/// FUN_1007fcc0: emote (command id - 0xa2), an optional mode word ("text" 1, "motion" 2, else 0), then the target
/// (a tag or a PC, else the current one; ourselves or none sends 0); c2s 0x05D {u32 id, u16 ActIndex, u8 emote,
/// u8 mode, u16 param}. Nothing plays here: the server's 0x05A does it. /jobemote takes the job first (param job +
/// 0x1e, mode 0; abbreviations only here, retail also takes group 2's full names).
fn emote(w: &mut World, cid: u16, args: &[String], mode: Mode) -> Done {
    let number = ((cid + 0x5e) & 0xff) as u8;
    let mut rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let mut param: u16 = 0;
    match number {
        43 => param = 1,
        65..=68 => param = (number - 63) as u16,
        96 => param = 0x35,
        73 => {
            param = match rest.first() {
                None => 6,
                Some(n) => BELL.iter().find(|(b, _)| b.eq_ignore_ascii_case(n)).map(|b| b.1).ok_or(())?,
            };
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x05D, body: emote_body(0, 0, number, 0, param) });
            return Ok(());
        }
        74 => {
            let job = rest.first().ok_or(())?;
            let j = super::items::JOBS.iter().position(|n| !n.is_empty() && n.eq_ignore_ascii_case(job)).ok_or(())?;
            param = j as u16 + 0x1e;
            rest.remove(0);
        }
        _ => {}
    }
    let word = |s: &str| match s {
        _ if s.starts_with("text") => Some(1u8),
        _ if s.starts_with("motion") => Some(2),
        _ if s.starts_with("both") => Some(0),
        _ => None,
    };
    let (m, target) = match rest.first().map(|a| (word(a), a)) {
        Some((Some(m), _)) => (m, rest.get(1).copied()),
        Some((None, t)) => (0, Some(*t)),
        None => (0, None),
    };
    let m = if number == 74 { 0 } else { m };
    let t = resolve(w, target, true, false, mode).filter(|t| t.0 != w.my_id).unwrap_or((0, 0));
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x05D, body: emote_body(t.0, t.1, number, m, param) });
    Ok(())
}

fn emote_body(target: u32, index: u16, number: u8, mode: u8, param: u16) -> Vec<u8> {
    let mut b = vec![0u8; 12];
    b[0..4].copy_from_slice(&target.to_le_bytes());
    b[4..6].copy_from_slice(&index.to_le_bytes());
    (b[6], b[7]) = (number, mode);
    b[8..10].copy_from_slice(&param.to_le_bytes());
    b
}

/// FUN_1022b430: a spell by name (case-insensitive, exact) of `kind` (0 any); its id, or Err when unknown, not
/// learned (s2c 0x0AA) or out of the jobs' reach.
fn find_spell(w: &World, name: &str, kind: u16) -> Result<u16, ()> {
    let (Some(names), Some(acts)) = (w.names.as_ref(), w.actions.as_ref()) else { return Err(()) };
    let s = acts
        .spells
        .iter()
        .filter(|s| s.id != 0 && (kind == 0 || s.kind == kind) && s.levels.iter().any(|&l| l >= 0))
        .find(|s| crate::dat::names::NameSource::spell(&**names, s.id as u32).is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .ok_or(())?;
    let learned = w.spells_known.get(s.id as usize / 8).is_some_and(|b| b & (1 << (s.id % 8)) != 0);
    let st = w.stats.as_ref().ok_or(())?;
    if !learned || !acts.can_cast(s.id, st.main, st.sub, 0) {
        log::info!("spell {}: learned {learned}, jobs {:?} / {:?}", s.id, st.main, st.sub);
        return Err(());
    }
    Ok(s.id)
}

/// FUN_1007b710: /ma (any), /nin (4), /so (5): 0x01A category 3 with the spell id.
fn magic(w: &mut World, cid: u16, args: &[String], mode: Mode) -> Done {
    if !status_gate(w, 0x7f) {
        return Ok(());
    }
    let kind = match cid {
        id::SONG => 5,
        id::NINJUTSU => 4,
        _ => 0,
    };
    let spell = find_spell(w, args.get(1).ok_or(())?, kind)?;
    let t = resolve(w, args.get(2).map(String::as_str), false, true, mode).ok_or(())?;
    act(w, t, cat::MAGIC, spell as u32);
    Ok(())
}

/// The ability name table entry `id` (weapon skills 0..0x1ff, job abilities 0x200.., pet commands 0x400..).
fn ability_named(w: &World, name: &str, ids: impl Iterator<Item = u16>) -> Option<u16> {
    let names = w.names.as_ref()?;
    let acts = w.actions.as_ref()?;
    let mut hit = None;
    for i in ids {
        let Some(a) = acts.abilities.get(i as usize).filter(|a| a.id != 0) else { continue };
        if crate::dat::names::NameSource::ability(&**names, a.id as u32).is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            hit = Some(a.id);
        }
    }
    hit
}

/// FUN_1007b820: engaged only (else group 7 0x52); records 0..0x1ff; 0x01A category 7. Retail sends ids above
/// 0xff as id + 0x2000; LSB reads the id as is, so it goes unchanged.
fn weapon_skill(w: &mut World, args: &[String], mode: Mode) -> Done {
    if !status_gate(w, 0x7f) {
        return Ok(());
    }
    if w.my_motion.status != 1 {
        refuse(w, 0x52);
        return Ok(());
    }
    let ws = ability_named(w, args.get(1).ok_or(())?, 0..0x200).ok_or(())?;
    let t = resolve(w, args.get(2).map(String::as_str), false, true, mode).ok_or(())?;
    act(w, t, cat::WS, ws as u32);
    Ok(())
}

/// FUN_1007b900: /ja and /pet: records 0x200..0x3ff (not hidden), then 0x400..0x5ff, the later match winning;
/// 0x01A category 9 with id - 0x200. A job ability must be in s2c 0x0AC's bits (pet commands: not checked, inf).
fn ability(w: &mut World, _pet: bool, args: &[String], mode: Mode) -> Done {
    if !status_gate(w, 0x7f) {
        return Ok(());
    }
    let name = args.get(1).ok_or(())?;
    let hidden = |i: u16| w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)).is_some_and(|a| a.sub != 0);
    let ja = ability_named(w, name, (0x200..0x400).filter(|&i| !hidden(i)));
    let id = ability_named(w, name, 0x400..0x600).or(ja).ok_or(())?;
    if (0x200..0x400).contains(&id) {
        let bit = (id - 0x200) as usize;
        if !w.commands_known.get(64 + bit / 8).is_some_and(|b| b & (1 << (bit % 8)) != 0) {
            return Err(());
        }
    }
    let t = resolve(w, args.get(2).map(String::as_str), false, true, mode).ok_or(())?;
    act(w, t, cat::JA, (id - 0x200) as u32);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_tokens() {
        assert_eq!(tokens("/ma \"Cure II\" <t>", false), ["/ma", "Cure II", "<t>"]);
        assert_eq!(tokens("/ma Cure II <t>", false), ["/ma", "Cure", "II", "<t>"]);
        assert_eq!(tokens("/ja 'Provoke' <t>", false), ["/ja", "Provoke", "<t>"]);
        assert_eq!(tokens("/echo\tx", false), ["/echo\tx"]);
        assert_eq!(tokens("/echo\tx", true), ["/echo", "x"]);
        assert_eq!(tokens("/ma \"Cure", false), ["/ma", "\"Cure"]);
    }

    /// /wave is emote 8, /dance3 param 4, /bell's notes; the 0x05D body layout.
    #[test]
    fn emote_ids() {
        assert!(is_emote(0xa2) && is_emote(0x102) && !is_emote(0xa1));
        assert_eq!(((0xaa_u16 + 0x5e) & 0xff) as u8, 8);
        assert_eq!(BELL.iter().find(|b| b.0 == "a4").map(|b| b.1), Some(11));
        assert_eq!(emote_body(0x0100_0005, 0x405, 8, 1, 0x35), [5, 0, 0, 1, 5, 4, 8, 1, 0x35, 0, 0, 0]);
    }

    #[test]
    fn names_and_numbers() {
        assert_eq!(name_case("shantotto").as_deref(), Some("Shantotto"));
        assert_eq!(name_case("ab"), None);
        assert_eq!(name_case("Ajido-Marujido"), None);
        assert_eq!((number("0x10"), number("$10"), number("%101"), number("42"), number("x")), (Some(16), Some(16), Some(5), Some(42), None));
    }

    #[test]
    fn cut_keeps_characters() {
        let mut s = "é".repeat(100);
        cut(&mut s);
        assert_eq!(s.len(), 150);
        let mut s = format!("a{}", "é".repeat(100));
        cut(&mut s);
        assert_eq!(s.len(), 149);
    }

    /// The installed FFXiMain's table: /ma is magic, a target command; /y is no command; /h is /help.
    #[test]
    fn table_from_ffximain() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let t = parse_table(&dll).unwrap();
        assert_eq!((t.len(), t["/ver"].0, t["/say"].0), (277, 0x1ae, 1));
        assert_eq!(t["/ma"], (id::MAGIC, 0x0101));
        assert_eq!(t["/magic"].0, id::MAGIC);
        assert_eq!(t["/h"].0, id::HELP);
        assert_eq!(t["/a"], (id::ATTACK, 0x0141));
        assert!(!t.contains_key("/y") && !t.contains_key("/MA"));
    }
}
