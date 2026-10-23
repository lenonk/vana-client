//! Text commands as FUN_10080dc0 runs them: the tag pass, the 150-byte cut, tokens, the command table from
//! FFXiMain (0x10355408: name, id, flags), the state gate and the handlers. MODLOG "Text commands".

use super::net::NetCommand;
use super::{World, combat, event, target};
use std::collections::HashMap;
use std::sync::OnceLock;

/// FUN_10080dc0's mode: 0 menus, 1 typed, 2 macro (3..7, the sub-target re-runs, are not here yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Internal,
    Typed,
    Macro,
}

/// Command flags (u16 @0x16).
mod flag {
    pub const UI_FREE: u16 = 0x10;
    pub const TARGET: u16 = 0x100;
    pub const NO_GATE: u16 = 0x1000;
}

/// Command ids.
mod id {
    pub const SAY: u16 = 0x01;
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
    pub const LOCKON: u16 = 0x5c;
    pub const CHARAEFFECT: u16 = 0x8a;
    pub const WAIT: u16 = 0x10a;
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
fn name_case(s: &str) -> Option<String> {
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
            "<pet>" => pet(w),
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
        if mode == Mode::Internal
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

/// A group 7 line (FUN_10129e30, mode 0x7b).
fn refuse(w: &mut World, id: usize) {
    let Some(bt) = w.battle.clone() else { return };
    let Some(line) = bt.group7(id) else { return };
    let colour = bt.colour_in(0x7b, &w.game);
    w.chat.push_colour(line, colour);
}

/// FUN_101cc0d0(mask): the status checks of action commands, the first that applies prints its line.
fn status_gate(w: &mut World, mask: u8) -> bool {
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
    // A line without a command goes to the chat mode (ours: always say).
    let line = if line.starts_with('/') { line.to_string() } else { format!("/say {line}") };
    let first = line.split([' ', '\t']).next().unwrap_or("");
    let target_cmd = table().get(first).is_some_and(|e| e.1 & flag::TARGET != 0);
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
    if !state_gate(w, flags) {
        return;
    }
    if handle(w, cid, &args, &expanded, mode).is_err() {
        error(w, &expanded, mode);
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
        id::SAY | id::SHOUT | id::YELL | id::PARTY | id::LS | id::LS2 | id::UNITY | id::ASSISTJ | id::ASSISTE => chat(w, cid, tail(line, 1)),
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
                w.shutdown = shutdown && w.logging_out;
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
        id::CHECK | id::CHECKNAME | id::CHECKPARAM => {
            let t = resolve(w, arg(1), true, false, mode).ok_or(())?;
            send(w, NetCommand::Check { target: t.0, index: t.1, kind: (cid - id::CHECK) as u8 });
            Ok(())
        }
        id::SHOOT => {
            // FUN_1007a730: no gate, target required.
            let t = resolve(w, arg(1), false, false, mode).ok_or(())?;
            act(w, t, cat::SHOOT, 0);
            Ok(())
        }
        id::MAGIC | id::NINJUTSU | id::SONG => magic(w, cid, args, mode),
        id::WS => weapon_skill(w, args, mode),
        id::JA | id::PET => ability(w, cid == id::PET, args, mode),
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
        _ => 0x23,
    };
    let me = w.name.clone();
    let line = match cid {
        id::YELL => {
            let zone = w.zone.and_then(|z| w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, z as u32))).unwrap_or_default();
            format!("{me}[{zone}]: {msg}")
        }
        id::LS => format!("[1]<{me}> {msg}"),
        id::LS2 => format!("[2]<{me}> {msg}"),
        _ => super::chat_line(kind, &me, msg),
    };
    let line = super::stamped(w, line);
    let c = super::cfgapply::chat_colour(&w.game, kind);
    w.chat.push_colour(line, c);
    send(w, NetCommand::Chat { kind, text: msg.to_string() });
    Ok(())
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
    let echo = super::stamped(w, format!(">>{name} : {msg}"));
    let c = super::cfgapply::chat_colour(&w.game, 3);
    w.chat.push_colour(echo, c);
    send(w, NetCommand::Tell { name, text: msg });
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
    let hidden = |i: u16| w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)).is_some_and(|a| a.hidden);
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
