//! The main menu's Linkshell row (FUN_10140400 case 9; menu_rows2_spec part L): "menu    link5" lists the linkshell
//! items in the inventory, Mog Satchel, Sack and Case, "menu    link6" above it shows the cursor row's shell. Enter
//! opens link2 / link3 / link4 on an equipped holder / pearlsack / pearl, link13 on one not equipped, link1 on a new
//! linkshell (all group 0x81, in the main menu's place); link1's Name Link opens link11 in link6's place.

use std::time::{Duration, Instant};

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::ItemInfo;

pub const LIST: &str = "menu    link5";
const HEADER: &str = "menu    link6";
const NEW: &str = "menu    link1";
const HOLDER: &str = "menu    link2";
const SACK: &str = "menu    link3";
const PEARL: &str = "menu    link4";
const UNWORN: &str = "menu    link13";
const NAMING: &str = "menu    link11";
/// FUN_101f6700(10); row height 0x12; columns DAT_1037f304.
const ROWS: usize = 10;
const PITCH: f32 = 18.0;
const COLS: [f32; 4] = [0.0, 18.0, 160.0, 355.0];
const WHITE: u32 = 0x8080_8080;
const GREEN: u32 = 0x8050_9050;
const GREY: u32 = 0x8040_4040;
const RED: u32 = 0x8080_4040;
/// The 15 name bytes an equip sends (FUN_101e5d60): "dummy" and what follows it at 0x1037f448.
const DUMMY: [u8; 15] = [b'd', b'u', b'm', b'm', b'y', 0, 0, 0, 0x0b, 0, 0, 0, 0xc0, 0x14, 0xa8];
/// link11's channel bars (FUN_101e51b0 +0x20..+0x28).
const BAR_TINTS: [u32; 3] = [0x7080_8080, 0x7000_8000, 0x7000_0080];

/// One link5 row (FUN_101e3b40's 0x17-byte row data and its cells).
#[derive(Clone, Debug)]
pub struct Row {
    pub slot: u8,
    pub bag: u8,
    /// Extdata Flag @8: 0 new, 1 holder, 2 pearlsack, 3 pearl, 4 broken (any other value).
    pub state: u8,
    pub item: u16,
    pub name: String,
    /// The icon's tint (the shell's colour) and the item name's.
    pub tint: u32,
    pub name_tint: u32,
    /// Equipped as linkshell 1 or 2, else 0.
    pub num: u8,
}

#[derive(Clone, Debug)]
pub struct Linkshell {
    pub top: usize,
    pub cursor: usize,
    pub sub: Option<Sub>,
    pub naming: Option<Naming>,
}

/// A submenu over the main menu's place: its cursor child, the children disabled at open (FUN_101e5f90) and the
/// Equip row's help (group 0xe).
#[derive(Clone, Debug)]
pub struct Sub {
    pub name: &'static str,
    pub sel: u16,
    off: Vec<u16>,
    help: usize,
}

/// link11: +0x14 (0 the rows, 1 typing the name, 2..4 R / G / B edited), the channels 0..15 and the one edited's
/// backup, the name.
#[derive(Clone, Debug)]
pub struct Naming {
    pub sel: u16,
    pub edit: u8,
    pub rgb: [u8; 3],
    backup: u8,
    pub name: String,
}

/// The change sent last (0x10663510): linkshell n, the item's GroupId, ActiveFlg, the end of its 0x3c ticks.
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    n: u8,
    group: u32,
    active: bool,
    until: Instant,
}

/// The yes / no prompts (comyn) the windows raise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Group 9 0x07 "Switch to this linkshell group?" (Set as No.1 / No.2 while one is worn there).
    Switch { n: u8, bag: u8, slot: u8 },
    /// Drop: stage 0 group 9 0x08; a holder asks 0x157 then 0x158 (stages 1, 2).
    Drop { slot: u8, stage: u8 },
}

/// FUN_100e9730: linkshell n's (slot, bag, extdata) while its item is still in that slot.
fn equipped(w: &World, n: usize) -> Option<(u8, u8, [u8; 24])> {
    let &(index, bag) = w.linkshells.get(n)?;
    (index != 0 && w.inv.slots.get(&(bag, index)).is_some_and(|s| s.0 != 0)).then(|| (index, bag, ext(w, bag, index)))
}

fn ext(w: &World, bag: u8, slot: u8) -> [u8; 24] {
    w.inv.ext.get(&(bag, slot)).copied().unwrap_or([0; 24])
}

fn group(e: &[u8; 24]) -> u32 {
    u32::from_le_bytes([e[0], e[1], e[2], e[3]])
}

/// The shell's colour as a tint: R = [6] low, G = [6] high, B = [7] low nibble, each 0x38 + 13 per step.
fn tint(r: u8, g: u8, b: u8) -> u32 {
    let c = |v: u8| (v & 0xf) as u32 * 13 + 0x38;
    0x8000_0000 | c(r) << 16 | c(g) << 8 | c(b)
}

fn shell_tint(e: &[u8; 24]) -> u32 {
    tint(e[6], e[6] >> 4, e[7])
}

fn rgba(t: u32) -> [f32; 4] {
    [16, 8, 0, 24].map(|s| ((t >> s) & 0xff) as f32 / 128.0)
}

/// FUN_101e3a20: another item of this group is worn as linkshell 1 or 2.
fn worn_elsewhere(w: &World, slot: u8, bag: u8, g: u32) -> bool {
    [1, 2].into_iter().any(|n| equipped(w, n).is_some_and(|(s, b, e)| (s, b) != (slot, bag) && group(&e) == g))
}

/// FUN_100ed650: the 6-bit name (MSB first): 0x3f or past 0x34 ends it, 0x3e is a space, 0..26 '`'..'z', 27..52
/// 'A'..'Z'; 20 characters at most.
pub fn ls_name(b: &[u8]) -> String {
    let at = |i: usize| b.get(i).copied().unwrap_or(0) as u16;
    let mut out = String::new();
    if at(0) == 0 {
        return out;
    }
    for k in 0..20 {
        let bit = k * 6;
        let v = ((at(bit / 8) << 8 | at(bit / 8 + 1)) >> (10 - bit % 8)) & 0x3f;
        out.push(match v {
            0x3e => ' ',
            0..=0x1a => (v as u8 + 0x60) as char,
            0x1b..=0x34 => (v as u8 + 0x26) as char,
            _ => break,
        });
    }
    out
}

/// FUN_100ed590(16): letters to 1..52, anything else a space (0x3e), the end 0x3f, packed MSB first.
pub fn ls_encode(name: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let codes = name.bytes().map(|c| match c {
        b'a'..=b'z' => c - 0x60,
        b'A'..=b'Z' => c - 0x26,
        _ => 0x3e,
    });
    for (k, v) in codes.chain(std::iter::once(0x3f)).enumerate() {
        let bit = k * 6;
        if bit / 8 >= 15 {
            break;
        }
        let word = (v as u16) << (10 - bit % 8);
        out[bit / 8] |= (word >> 8) as u8;
        out[bit / 8 + 1] |= word as u8;
    }
    out[15] = 0;
    out
}

/// FUN_101e3b40, rebuilt every frame: bags 0, 5, 6, 7 in view order once s2c 0x01D has opened them (0x800 /
/// 0x200000), each Linkshell-type item (record type 6) not on bazaar (lock 0x19); 0x51 rows at most.
pub fn rows(w: &World) -> Vec<Row> {
    let mut out = Vec::new();
    let Some(info) = &w.item_info else { return out };
    if w.inv_flags & 0x0020_0800 == 0 {
        return out;
    }
    let worn = [equipped(w, 1), equipped(w, 2)];
    for bag in [0u8, 5, 6, 7] {
        for s in w.inv.viewed(bag) {
            if out.len() > 0x50 {
                break;
            }
            if s.index == 0 || w.inv.locks.get(&(bag, s.index)) == Some(&0x19) || info.kind(s.id) != 6 {
                continue;
            }
            let e = ext(w, bag, s.index);
            let num = worn.iter().position(|x| x.is_some_and(|(sl, b, _)| (sl, b) == (s.index, bag))).map_or(0, |k| k as u8 + 1);
            let mut name_tint = if num != 0 { GREEN } else { WHITE };
            if worn_elsewhere(w, s.index, bag, group(&e)) {
                name_tint = GREY;
            }
            let (state, name) = match e[8] {
                0 => {
                    if worn[0].is_some() {
                        name_tint = GREY;
                    }
                    (0, String::new())
                }
                f @ 1..=3 => (f, ls_name(&e[9..24])),
                _ => {
                    name_tint = GREY;
                    (4, "Broken".to_string())
                }
            };
            let tint = if state == 0 { WHITE } else { shell_tint(&e) };
            out.push(Row { slot: s.index, bag, state, item: s.id, name, tint, name_tint, num });
        }
    }
    out
}

/// FUN_101e34d0: the row under link5's cursor.
fn cursor_row(w: &World) -> Option<Row> {
    let l = w.linkshell.as_ref()?;
    rows(w).into_iter().nth(l.cursor)
}

/// FUN_10140400 case 9: in Conflict SE 0004 and 0002 only; else link5 (FUN_101e4470(0, 0, 0)) and link6, SE 0002.
pub fn open(w: &mut World) {
    if super::combat::conflict(w.my_team) {
        w.sounds.push(Ui::Error as u32);
        w.sounds.push(Ui::Select as u32);
        return;
    }
    w.sounds.push(Ui::Select as u32);
    w.linkshell = Some(Linkshell { top: 0, cursor: 0, sub: None, naming: None });
}

/// FUN_101e3370 (each frame while link5 is up, and on Enter): true once the change sent last has shown (linkshell n
/// worn with that group, or none worn after an unequip) or its 0x3c ticks ran out *(inf: 1 s)*.
fn settled(w: &mut World) -> bool {
    let Some(p) = w.ls_pending else { return true };
    let shown = match equipped(w, p.n as usize) {
        None => !p.active,
        Some((_, _, e)) => p.active && group(&e) == p.group,
    };
    let done = shown || Instant::now() >= p.until;
    if done {
        w.ls_pending = None;
    }
    done
}

/// FUN_100e95e0: c2s 0x0C4 {@4 0xf000 | b << 8 | g << 4 | r, ItemIndex, Category, ActiveFlg, name (15), LinkshellId};
/// refused for a slot past the bag's size. Then the pending change (FUN_101e5d60 / FUN_101e5490).
fn send_active(w: &mut World, n: u8, bag: u8, slot: u8, active: bool, name: &[u8], rgb: [u8; 3]) -> bool {
    if name.first().is_none_or(|&c| c == 0) || slot >= w.inv.sizes.get(bag as usize).copied().unwrap_or(0) {
        return false;
    }
    let mut b = vec![0u8; 0x18];
    let colour = 0xf000 | (rgb[2] as u16) << 8 | ((rgb[1] << 4) ^ (rgb[0] & 0xf)) as u16;
    b[0..2].copy_from_slice(&colour.to_le_bytes());
    (b[2], b[3], b[4]) = (slot, bag, active as u8);
    b[8..23].copy_from_slice(&name[..15]);
    b[0x17] = n;
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0C4, body: b });
    if active && let Some(a) = w.ls_msg.ask.get_mut(n as usize) {
        *a = 1;
    }
    let e = ext(w, bag, slot);
    w.ls_pending = Some(Pending { n, group: group(&e), active, until: Instant::now() + Duration::from_secs(1) });
    true
}

/// The linkshell message (spec L5): per linkshell n 1..3 the ask state S+0x9818 + 4n (0 fresh at login, 1 after our
/// own equip, 2 asked) and the reason S+0x9834 + 4n the reply prints by (0 typed, 1 / 2 asked by the client).
#[derive(Clone, Copy, Debug, Default)]
pub struct MsgState {
    pub ask: [u8; 4],
    reason: [u8; 4],
}

/// FUN_100eaab0 / FUN_100eac70: c2s 0x0E1 {@5 bits 6..7 = n - 1, @6 bag, @7 slot, seq 0}, keeping why it was asked.
fn ask_message(w: &mut World, n: u8, bag: u8, slot: u8, reason: u8) {
    if let Some(r) = w.ls_msg.reason.get_mut(n as usize) {
        *r = reason;
    }
    let mut b = vec![0u8; 0x8c];
    b[1] = 0x3f ^ n.wrapping_mul(0x40).wrapping_sub(1);
    (b[2], b[3]) = (bag, slot);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0E1, body: b });
}

/// LAB_100ea780 after the slot is stored: with an item there, ask for the message the first time (reason 1) or after
/// our own equip (reason 2); asked either way.
pub fn on_equip(w: &mut World, n: u8, index: u8, bag: u8) {
    let Some(&state) = w.ls_msg.ask.get(n as usize) else { return };
    if index != 0 && state < 2 {
        ask_message(w, n, bag, index, state + 1);
    }
    w.ls_msg.ask[n as usize] = 2;
}

/// The mode word's byte per linkshell (table 0x1032cf40) and the "[n]" prefix (FUN_100e3750).
fn mode_of(n: u8) -> u8 {
    match n {
        2 => 0xd9,
        3 => 0xda,
        _ => 0xcd,
    }
}

fn push_line(w: &mut World, line: String, mode: u8) {
    let Some(bt) = w.battle.clone() else { return };
    let names = w.names.clone();
    let line = bt.phrases.as_ref().map_or(line.clone(), |d| d.expand(&line, names.as_deref().map(|n| n as &dyn crate::dat::names::NameSource)));
    let colour = bt.colour_in(mode, &w.game);
    w.chat.push_colour(line, colour);
}

/// LAB_10080390: n 7 "An error occurred." (7 0x9c), no text "No linkshell message exists." (7 0x9d), else "[n]" + 10
/// 0x34 "< %s: %s >" (LS name, poster) and the text + " (" + the Earth date (FUN_101c8fd0(time, 1)) + ")".
fn message_line(w: &mut World, n: u8, time: u32, ls: &str, poster: &str, text: Option<&str>) {
    let Some(bt) = w.battle.clone() else { return };
    if n == 7 {
        return super::textcmd::refuse(w, 0x9c);
    }
    let Some(text) = text else { return super::textcmd::refuse(w, 0x9d) };
    let head = super::textcmd::printf(&bt.group10(0x34).unwrap_or_default(), &[ls.to_string(), poster.to_string()]);
    let g = |id: u16| bt.group10_raw(id as usize);
    let (date_key, h12) = ((w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0);
    let date = crate::dat::font::decode_sjis(&crate::dat::msgfmt::earth_datetime(&g, time as i64, 1, date_key, h12));
    let mode = mode_of(n);
    push_line(w, format!("[{n}]{head}"), mode);
    push_line(w, format!("{} ({date})", text.trim()), mode);
}

/// LAB_10080570: n 7 the error line, else who may edit (writeLevel 0 / 1 / 2: 9 0xbc / 0xbd / 0xbe) in mode 0xcd.
fn level_line(w: &mut World, n: u8, level: u8) {
    if n == 7 {
        return super::textcmd::refuse(w, 0x9c);
    }
    let Some(line) = w.battle.as_ref().and_then(|b| b.group9(0xbc + level as usize).filter(|_| level < 3)) else { return };
    push_line(w, line, 0xcd);
}

/// s2c 0x0CC (LAB_100ea810): @4 stat (low nibble) / attr, @5 levels and the slot (bits 6..7 = n - 1), @8 the text,
/// @0x88 the time, @0x8c the poster, @0x9c opType, @0xa0 the encoded LS name. Load replies print by the reason asked
/// (typed: the empty case too; asked by the client: only a message).
pub fn on_message(w: &mut World, p: &[u8]) {
    let at = |o: usize| p.get(o).copied().unwrap_or(0);
    let n = (at(5) >> 6) + 1;
    let (stat, posted) = (at(4) & 0xf, at(4) & 0x40 != 0);
    let str_at = |o: usize, len: usize| {
        let b = p.get(o..(o + len).min(p.len())).unwrap_or_default();
        crate::dat::font::decode_sjis(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
    };
    let (text, poster) = (str_at(8, 128), str_at(0x8c, 16));
    let time = u32::from_le_bytes([at(0x88), at(0x89), at(0x8a), at(0x8b)]);
    let ls = ls_name(p.get(0xa0..).unwrap_or_default());
    let levels = (at(5) & 3, (at(5) >> 2) & 3);
    let reason = w.ls_msg.reason.get(n as usize).copied().unwrap_or(0);
    match u16::from_le_bytes([at(0x9c), at(0x9d)]) {
        // Post (broadcast): only a message is printed.
        1 => match stat {
            0 if !posted => {}
            0 | 5 => {
                if !text.is_empty() {
                    message_line(w, n, time, &ls, &poster, Some(&text));
                }
            }
            3 => super::textcmd::refuse(w, 0x9e),
            _ => message_line(w, 7, 0, "", "", None),
        },
        2 => {
            let shown = match stat {
                0 if posted => Some(text.as_str()),
                0 => return,
                5 => None,
                _ => return message_line(w, 7, 0, "", "", None),
            };
            let shown = shown.filter(|t| !t.is_empty());
            match (reason, shown) {
                (0, s) => message_line(w, n, time, &ls, &poster, s),
                (1 | 2, Some(s)) => message_line(w, n, time, &ls, &poster, Some(s)),
                _ => {}
            }
        }
        3 => match stat {
            0 => level_line(w, n, levels.1),
            3 => super::textcmd::refuse(w, 0x9e),
            _ => level_line(w, 7, 0),
        },
        6 if at(4) & 0xf != 0 => level_line(w, 7, 0),
        6 => level_line(w, n, levels.1),
        _ => {}
    }
}

/// c2s 0x0E2 (FUN_100eace0 / FUN_100ead60) and 0x0E4 (FUN_100eabc0), linkshell 1 only: @4 0x40 set the text (@0x10,
/// 127 bytes), 0x20 the edit level (@5 bits 2..3, seq @0xa), 0xb0 ask the level (seq @0xa incremented, 15 bits, never
/// 0); @6 bag, @7 slot.
fn send_lsmes(w: &mut World, id: u16, flag: u8, bag: u8, slot: u8, text: &[u8], level: Option<u8>) {
    let mut b = vec![0u8; 0x8c];
    b[0] = flag;
    (b[2], b[3]) = (bag, slot);
    if let Some(l) = level {
        b[1] = (l << 2) & 0xc;
    }
    if flag == 0xb0 {
        w.ls_msg_seq = (w.ls_msg_seq.wrapping_add(1)) & 0x7fff;
        if w.ls_msg_seq == 0 {
            w.ls_msg_seq = 1;
        }
    }
    if flag != 0x40 {
        b[6..8].copy_from_slice(&w.ls_msg_seq.to_le_bytes());
    }
    let n = text.len().min(0x7f);
    b[0xc..0xc + n].copy_from_slice(&text[..n]);
    let _ = w.net.commands.send(NetCommand::Packet { id, body: b });
}

/// /lsmes (FUN_100805f0) and /ls2mes (FUN_100807d0): linkshell 1 (2) worn, else 7 0x71 *(inf: actor +0x124 bit 27, a
/// zone-in flag, is not modelled)*. No argument asks for the message (reason 0); /lsmes "level" asks the edit level,
/// "level ls|ps|all" sets it, "clear" empties the message, "set TEXT" or TEXT sets it.
pub fn lsmes(w: &mut World, n: usize, args: &[String], tail1: &str, tail2: &str) {
    let Some((slot, bag, _)) = equipped(w, n) else { return super::textcmd::refuse(w, 0x71) };
    if args.len() <= 1 {
        return ask_message(w, n as u8, bag, slot, 0);
    }
    if n != 1 {
        return;
    }
    match args[1].as_str() {
        "level" => {
            let level = match args.get(2).map(String::as_str) {
                None => return send_lsmes(w, 0x0E4, 0xb0, bag, slot, &[], None),
                Some("ls") => 0,
                Some("ps") => 1,
                Some("all") => 2,
                Some(_) => return,
            };
            send_lsmes(w, 0x0E2, 0x20, bag, slot, &[], Some(level));
        }
        "clear" => send_lsmes(w, 0x0E2, 0x40, bag, slot, &[], None),
        first => {
            let t = if first == "set" { tail2 } else { tail1 };
            send_lsmes(w, 0x0E2, 0x40, bag, slot, &crate::dat::font::encode_sjis(t), None);
        }
    }
}

/// FUN_101e5d60: Set as No.n (asking first while linkshell n is worn) or Unequip, for the cursor row.
fn set(w: &mut World, active: bool, n: u8) {
    let Some(r) = cursor_row(w) else { return };
    if active && equipped(w, n as usize).is_some() {
        w.confirm = Some((super::Confirm::Ls(Ask::Switch { n, bag: r.bag, slot: r.slot }), false));
        return;
    }
    send_active(w, n, r.bag, r.slot, active, &DUMMY, [0; 3]);
}

/// FUN_100eb6c0(bag, slot, 1): c2s 0x028 {count 1, bag, slot}.
fn drop_item(w: &World, bag: u8, slot: u8) {
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&1u32.to_le_bytes());
    (b[4], b[5]) = (bag, slot);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x028, body: b });
}

/// FUN_101e5ef0 / FUN_101e61b0 case 2: a holder asks 9 0x157, the others 9 0x08.
fn ask_drop(w: &mut World) {
    let Some(r) = cursor_row(w) else { return };
    let stage = if r.state == 1 { 1 } else { 0 };
    w.confirm = Some((super::Confirm::Ls(Ask::Drop { slot: r.slot, stage }), false));
}

/// The prompts' Yes (LAB_101e5cd0, LAB_101e5e50 / LAB_101e5ea0, LAB_101e6150).
pub fn answer(w: &mut World, a: Ask) {
    match a {
        Ask::Switch { n, bag, slot } => {
            send_active(w, n, bag, slot, true, &DUMMY, [0; 3]);
        }
        Ask::Drop { slot, stage: 1 } => w.confirm = Some((super::Confirm::Ls(Ask::Drop { slot, stage: 2 }), false)),
        Ask::Drop { slot, .. } => {
            if let Some(r) = cursor_row(w) {
                drop_item(w, r.bag, slot);
            }
        }
    }
}

pub fn ask_text(gu: &GameUi, a: Ask) -> String {
    let id = match a {
        Ask::Switch { .. } => 0x07,
        Ask::Drop { stage: 0, .. } => 0x08,
        Ask::Drop { stage: 1, .. } => 0x157,
        Ask::Drop { .. } => 0x158,
    };
    gu.item_msg(9, id).unwrap_or_default()
}

/// FUN_101e5f90 (and FUN_101e6510 for link13): the Equip row's help, 0xe 2 "Deactivate ..." for link2..4 (but 0xe 1
/// on a window's first open: the hook runs before FUN_101e4770 clears +0x16); a broken row's Equip off (link13 also
/// row 3); link2 / link3's Create Pearl off without a free inventory slot.
fn push(w: &mut World, name: &'static str, state: u8) {
    let bit = match name {
        HOLDER => 1,
        SACK => 2,
        PEARL => 4,
        _ => 0,
    };
    let help = if bit == 0 || w.ls_opened & bit == 0 { 1 } else { 2 };
    w.ls_opened |= bit;
    let mut off = Vec::new();
    if state == 4 {
        off.push(if name == UNWORN { 1 } else { 2 });
        if name == UNWORN {
            off.push(3);
        }
    }
    if matches!(name, HOLDER | SACK) && !super::synth::free_slot(w) {
        off.push(3);
    }
    if let Some(l) = w.linkshell.as_mut() {
        l.sub = Some(Sub { name, sel: 1, off, help });
    }
}

/// FUN_101e4770 (+0x5c): true SE 0002, false SE 0004.
fn enter(w: &mut World) -> bool {
    let r = cursor_row(w);
    if !settled(w) {
        return false;
    }
    let Some(r) = r else { return false };
    if r.num != 0 {
        let name = match r.state {
            1 => HOLDER,
            2 => SACK,
            3 => PEARL,
            _ => return false,
        };
        push(w, name, r.state);
        return true;
    }
    if worn_elsewhere(w, r.slot, r.bag, group(&ext(w, r.bag, r.slot))) {
        return false;
    }
    match r.state {
        0 if equipped(w, 1).is_none() => push(w, NEW, 0),
        0 => {}
        s => push(w, UNWORN, s),
    }
    true
}

/// A submenu's Enter (FUN_101e61b0, FUN_101e6250 / 6360 / 6470, FUN_101e6540); each closes its window after.
fn select(w: &mut World, name: &'static str, child: u16) {
    let n = cursor_row(w).map_or(0, |r| r.num);
    match (name, child) {
        (NEW, 1) => {
            if let Some(l) = w.linkshell.as_mut() {
                l.naming = Some(Naming { sel: 1, edit: 0, rgb: [7; 3], backup: 0, name: String::new() });
            }
        }
        (NEW, 2) | (UNWORN, 2) => ask_drop(w),
        (UNWORN, 1) => set(w, true, 1),
        (UNWORN, 3) => set(w, true, 2),
        (HOLDER | SACK | PEARL, 2) if n != 0 => set(w, false, n),
        // FUN_101e5c10: c2s 0x0C3 {State 0, LinkshellId n}.
        (HOLDER | SACK, 3) if n != 0 => {
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x0C3, body: vec![0, n] });
        }
        // FUN_101e5c50(n): the member list (link12); our rank is the shell's Flag @8.
        (HOLDER | SACK | PEARL, 1) if n != 0 => {
            if let Some(r) = cursor_row(w) {
                let g = group(&ext(w, r.bag, r.slot));
                super::linklist::open(w, n, g, r.state);
            }
        }
        _ => {}
    }
    if let Some(l) = w.linkshell.as_mut() {
        l.sub = None;
    }
}

/// The generic links (up, down, right, left = links 2..5), SE 0001 when the cursor moves.
fn step(w: &mut World, gu: &GameUi, name: &str, sel: &mut u16, key: egui::Key) {
    let k = match key {
        egui::Key::ArrowUp => 2,
        egui::Key::ArrowDown => 3,
        egui::Key::ArrowRight => 4,
        egui::Key::ArrowLeft => 5,
        _ => return,
    };
    let to = gu.menu_links(name, *sel).map_or(*sel, |l| l[k] as u16);
    if to != *sel && to != 0 && to != 0xff {
        *sel = to;
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// The list base (FUN_101f6c70): Up / Down stop at the ends, Right / Left turn a page of 10, Escape closes link5 and
/// every link window (FUN_101e4380).
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut l) = w.linkshell.clone() else { return };
    if l.naming.is_some() {
        naming_key(w, gu, key);
        return;
    }
    if let Some(mut s) = l.sub.clone() {
        match key {
            egui::Key::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                l.sub = None;
            }
            egui::Key::Enter if s.off.contains(&s.sel) => w.sounds.push(Ui::Error as u32),
            egui::Key::Enter => {
                w.sounds.push(Ui::Select as u32);
                select(w, s.name, s.sel);
                return;
            }
            _ => {
                step(w, gu, s.name, &mut s.sel, key);
                l.sub = Some(s);
            }
        }
        w.linkshell = Some(l);
        return;
    }
    let n = rows(w).len();
    match key {
        egui::Key::ArrowDown if l.cursor + 1 < n => {
            l.cursor += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowUp if l.cursor > 0 && n > 0 => {
            l.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowRight | egui::Key::ArrowLeft => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if super::scrollbar::rows_abs(m, &mut l.top, &mut l.cursor, n, ROWS) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.linkshell = None;
            return;
        }
        egui::Key::Enter => {
            w.linkshell = Some(l);
            let ok = enter(w);
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            return;
        }
        _ => {}
    }
    l.top = l.top.min(l.cursor).max((l.cursor + 1).saturating_sub(ROWS));
    w.linkshell = Some(l);
}

/// FUN_101e5490 / FUN_101e5280: the rows; a channel takes Left / Right (-1 / +1, 0..15) until Enter keeps it or
/// Escape restores it; OK sends the name (refused empty: SE 0004), Cancel reopens link5 from the top.
fn naming_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut nm) = w.linkshell.as_ref().and_then(|l| l.naming.clone()) else { return };
    let mut close = false;
    match nm.edit {
        1 => return,
        e @ 2..=4 => {
            let v = &mut nm.rgb[e as usize - 2];
            match key {
                egui::Key::ArrowLeft => *v = v.saturating_sub(1),
                egui::Key::ArrowRight => *v = (*v + 1).min(15),
                egui::Key::Enter => {
                    w.sounds.push(Ui::Select as u32);
                    nm.edit = 0;
                }
                egui::Key::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    *v = nm.backup;
                    nm.edit = 0;
                }
                _ => {}
            }
        }
        _ => match key {
            egui::Key::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                close = true;
            }
            egui::Key::Enter => {
                w.sounds.push(Ui::Select as u32);
                match nm.sel {
                    1 => nm.edit = 1,
                    c @ 2..=4 => (nm.edit, nm.backup) = (c as u8, nm.rgb[c as usize - 2]),
                    5 if nm.name.is_empty() => w.sounds.push(Ui::Error as u32),
                    5 => {
                        let n = if equipped(w, 1).is_some() { 2 } else { 1 };
                        if let Some(r) = cursor_row(w)
                            && !send_active(w, n, r.bag, r.slot, true, &ls_encode(&nm.name), nm.rgb)
                        {
                            w.sounds.push(Ui::Error as u32);
                        }
                        close = true;
                    }
                    _ => {
                        close = true;
                        if let Some(l) = w.linkshell.as_mut() {
                            (l.top, l.cursor) = (0, 0);
                        }
                    }
                }
            }
            _ => step(w, gu, NAMING, &mut nm.sel, key),
        },
    }
    if let Some(l) = w.linkshell.as_mut() {
        l.naming = (!close).then_some(nm);
    }
}

/// link11's name box (FUN_101c5be0(1, 0x13, ..)) takes the typed text.
pub fn typing(w: &World) -> bool {
    w.linkshell.as_ref().and_then(|l| l.naming.as_ref()).is_some_and(|n| n.edit == 1)
}

/// The box's keys: text (19 characters), Backspace; Enter commits, Escape ends it keeping the text (FUN_101e5130).
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    let Some(nm) = w.linkshell.as_mut().and_then(|l| l.naming.as_mut()) else { return };
    let mut sound = None;
    for ev in events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.chars().filter(|c| (' '..='~').contains(c)) {
                    if nm.name.chars().count() < 0x13 {
                        nm.name.push(c);
                    }
                }
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                nm.name.pop();
            }
            egui::Event::Key { key: k @ (egui::Key::Enter | egui::Key::Escape), pressed: true, .. } => {
                nm.edit = 0;
                sound = Some(if *k == egui::Key::Enter { Ui::Select } else { Ui::Cancel });
            }
            _ => {}
        }
    }
    if let Some(s) = sound {
        w.sounds.push(s as u32);
    }
}

/// Each frame while link5 is up (FUN_101e4610): the pending check, the cursor clamped to the rows (FUN_101f69d0).
pub fn update(w: &mut World) {
    if w.linkshell.is_none() {
        return;
    }
    settled(w);
    let n = rows(w).len();
    if let Some(l) = w.linkshell.as_mut() {
        l.top = l.top.min(n.saturating_sub(ROWS));
        l.cursor = l.cursor.min(n.saturating_sub(1));
    }
}

/// link5 has the keys while no submenu, link11 or prompt is up.
pub fn focused(w: &World) -> bool {
    w.linkshell.as_ref().is_some_and(|l| l.sub.is_none() && l.naming.is_none()) && w.confirm.is_none()
}

/// Mouse *(inf: the generic child hit test)*: hover moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(l) = w.linkshell.clone() else { return };
    let (name, sel) = match (&l.naming, &l.sub) {
        (Some(nm), _) if nm.edit == 0 => (NAMING, nm.sel),
        (Some(_), _) => return,
        (None, Some(s)) => (s.name, s.sel),
        (None, None) => (LIST, (l.cursor.saturating_sub(l.top) + 1) as u16),
    };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(name, sw, sh), gu.layout(name)) else { return };
    let n = rows(w).len();
    let Some(e) = lay.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).find(|e| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    }) else { return };
    if name == LIST && l.top + e.id as usize > n {
        return;
    }
    gu.hover.set(true);
    if e.id != sel && (moved || clicked) {
        if let Some(l) = w.linkshell.as_mut() {
            match (&mut l.naming, &mut l.sub) {
                (Some(nm), _) => nm.sel = e.id,
                (None, Some(s)) => s.sel = e.id,
                (None, None) => l.cursor = l.top + e.id as usize - 1,
            }
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The scroll bar (list messages 0x16..0x1b).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let n = rows(w).len();
    let Some(l) = w.linkshell.as_mut().filter(|l| l.sub.is_none() && l.naming.is_none()) else { return false };
    super::scrollbar::rows_abs(m, &mut l.top, &mut l.cursor, n, ROWS)
}

/// Help / title of the focused window: link5's row help 0xe 0 (0xe 0x126 without a row), title 0xd 0x47; a
/// submenu's children (the Equip row's help set at open).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(l) = &w.linkshell else { return };
    let (help, title) = match (&l.naming, &l.sub) {
        (Some(nm), _) => {
            let [h, t] = gu.focus_text(NAMING, nm.sel);
            (h, t)
        }
        (None, Some(s)) => {
            let [h, t] = gu.focus_text(s.name, s.sel);
            let equip = if s.name == UNWORN { 1 } else { 2 };
            let h = if s.sel == equip && s.name != NEW { Some(gu.menu_msg(s.help, "")) } else { h };
            (h, t)
        }
        (None, None) => {
            let row = l.cursor < rows(w).len();
            (Some(gu.menu_msg(if row { 0 } else { 0x126 }, "")), gu.focus_text(LIST, 0)[1].clone())
        }
    };
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

fn icon(gu: &mut GameUi, item: u16, x: f32, y: f32, size: f32, t: u32) {
    let name = crate::render::invwin::icon_texture(item);
    if gu.has_image(&name) {
        gu.image_rect(&name, x, y, size, size, rgba(t));
    }
}

/// link6 mode 0 (FUN_101e57b0): the cursor row's shell name (game font) at (+0x40, +0x14) and its icon at full scale at
/// (+10, +0xc) in its colour (grey 7 / 7 / 7 without a group); no row: group 7 0x71 in 0x80804040.
fn draw_header(gu: &mut GameUi, w: &World, row: Option<&Row>, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let win = gu.lobby_window_off(HEADER, sw, sh, (0.0, 0.0), None, false, &[])?;
    let (x, y) = gu.window_xy(HEADER, sw, sh)?;
    match row {
        None => {
            let s = w.battle.as_ref().and_then(|b| b.group7(0x71)).unwrap_or_default();
            gu.text(&s, x + 64.0, y + 20.0, 0, RED.to_le_bytes());
        }
        Some(r) => {
            let e = ext(w, r.bag, r.slot);
            let t = if group(&e) == 0 {
                tint(7, 7, 7)
            } else {
                gu.text(&ls_name(&e[9..24]), x + 64.0, y + 20.0, 0, WHITE.to_le_bytes());
                shell_tint(&e)
            };
            icon(gu, r.item, x + 10.0, y + 12.0, 32.0, t);
        }
    }
    Some(win.frame)
}

/// link11 (FUN_101e5310): the Linkshell icon in the chosen colour, the name (or the live box) at child 1 (+1, +8), a
/// bar per channel (windowps 97 at child (+12, +1), x scale value / 15).
fn draw_naming(gu: &mut GameUi, w: &World, nm: &Naming, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let focused = nm.edit == 0 && w.confirm.is_none();
    // The hand goes on after the contents: the icon would cover it.
    let win = gu.lobby_window_off(NAMING, sw, sh, (0.0, 0.0), Some(nm.sel), false, &[])?;
    let (x, y) = gu.window_xy(NAMING, sw, sh)?;
    if let Some(info) = &w.item_info {
        super::items::upload_icons(gu, info, std::iter::once(0x201));
    }
    icon(gu, 0x201, x + 10.0, y + 12.0, 32.0, tint(nm.rgb[0], nm.rgb[1], nm.rgb[2]));
    let child = |id: u16| win.children.iter().find(|c| c.0 == id).map(|c| c.1);
    if let Some([cx, cy, ..]) = child(1) {
        if nm.edit == 1 {
            let caret = if (w.started.elapsed().as_secs_f32() * 60.0) as u32 % 30 < 20 { "|" } else { "" };
            gu.text(&format!("{}{caret}", nm.name), cx + 1.0, cy + 8.0, 0, [0x80; 4]);
        } else {
            gu.shape_fit(&nm.name, cx + 1.0, cy + 8.0, 162.0, WHITE);
        }
    }
    for k in 0..3 {
        if let Some([cx, cy, ..]) = child(k as u16 + 2) {
            gu.sprite_scaled("menu    windowps", 97, cx + 12.0, cy + 1.0, nm.rgb[k] as f32 / 15.0, 1.0, BAR_TINTS[k]);
        }
    }
    if focused && let Some([cx, cy, ..]) = child(nm.sel) {
        gu.cursor(cx, cy);
    }
    Some(win.frame)
}

/// link6 (or link11 in its place), link5 (FUN_101f6a60: rows from (+3, +3) 18 apart, cells per FUN_101f4bb0 types
/// 0x46 / 0x19 / 0x27, the windowps 75 cursor bar at (+0, +4) while focused, the empty text 7 0x68 at (+35, +27)),
/// then the submenu.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(l) = &w.linkshell else { return rects };
    let rows = rows(w);
    if let Some(info) = &w.item_info {
        super::items::upload_icons(gu, info, rows.iter().map(|r| r.item));
    }
    let row = rows.get(l.cursor);
    rects.extend(match &l.naming {
        Some(nm) => draw_naming(gu, w, nm, sw, sh),
        None => draw_header(gu, w, row, sw, sh),
    });
    // link12 closes link5 and its submenus while it is up (FUN_101e4570); its close reopens them (FUN_101e5030).
    if w.linklist.is_some() {
        return rects;
    }
    let focused = focused(w);
    let sel = if focused && !rows.is_empty() { (l.cursor.saturating_sub(l.top) + 1) as u16 } else { 0 };
    if let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) {
        rects.extend(gu.window_rect(LIST, sw, sh));
        if rows.is_empty() {
            let s = w.battle.as_ref().and_then(|b| b.group7(0x68)).unwrap_or_default();
            gu.text(&s, wx + 35.0, wy + 27.0, 0, RED.to_le_bytes());
        }
        for (k, r) in rows.iter().enumerate().skip(l.top).take(ROWS) {
            let (rx, ry) = (wx + 3.0, wy + 3.0 + PITCH * (k - l.top) as f32);
            let x0 = rx + COLS[0];
            gu.sprite("menu    keytops3", 0x219, x0, ry + 2.0, WHITE);
            icon(gu, r.item, x0, ry + 2.0, 16.0, r.tint);
            if r.num != 0 {
                gu.shape_text(&r.num.to_string(), x0 + 6.0, ry + 1.0, r.tint, false);
            }
            let name = w.item_info.as_ref().and_then(|c| c.name(r.item)).unwrap_or_default();
            gu.text(&name, rx + COLS[1], ry + 2.0, 0, r.name_tint.to_le_bytes());
            gu.text_fit(&r.name, rx + COLS[2], ry + 2.0, COLS[3] - COLS[2] - 8.0, WHITE);
        }
        if focused && !rows.is_empty() {
            gu.sprite("menu    windowps", 75, wx, wy + 4.0 + PITCH * l.cursor.saturating_sub(l.top) as f32, WHITE);
        }
        if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
            gu.scroll_bar(fx + fw - 8.0, fy, fh, rows.len(), ROWS, l.top);
            if focused {
                gu.bar_site(super::scrollbar::Owner::Linkshell, fx + fw - 8.0, fy, fh, rows.len(), ROWS, l.top);
            }
        }
    }
    if let Some(s) = &l.sub {
        let focus = w.confirm.is_none();
        let swaps: &[(u16, usize)] = if matches!(s.name, HOLDER | SACK | PEARL) { &[(2, 122)] } else { &[] };
        if let Some(win) = gu.lobby_window_swaps(s.name, sw, sh, (0.0, 0.0), Some(s.sel), focus, &s.off, swaps) {
            gu.disabled_children(s.name, win.frame[0], win.frame[1], &s.off, Some(s.sel).filter(|_| focus));
            rects.push(win.frame);
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let e = ls_encode("Tux Club");
        assert_eq!(ls_name(&e[..15]), "Tux Club");
        let long = "Abcdefghijklmnopqrs";
        assert_eq!(ls_name(&ls_encode(long)[..15]), long);
        // Code 0 decodes to '`'; digits encode as spaces.
        assert_eq!(ls_name(&ls_encode("a1b")[..15]), "a b");
        assert_eq!(ls_name(&[0x04, 0x0f, 0xc0]), "a`");
        assert_eq!(ls_name(&[0; 15]), "");
    }

    #[test]
    fn tints() {
        assert_eq!(tint(7, 7, 7), 0x8093_9393);
        assert_eq!(tint(15, 0, 3), 0x80fb_385f);
    }
}
