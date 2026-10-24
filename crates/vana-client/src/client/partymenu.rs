//! The main menu's Party row (FUN_102016e0; spec ~/ffxi/lab/research/party_menu_spec.md): prty1 solo, prty2 a member,
//! prty3 the leader, prty7 a leader in someone else's alliance; over them prty4 (Alliance), prty8 (Distribution),
//! prty5us (Languages) and alarm; prtyjoin answers a pending invite (s2c 0x0DC).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use super::target::SubThen;
use crate::audio::Ui;

const PRTY1: &str = "menu    prty1";
const PRTY2: &str = "menu    prty2";
const PRTY3: &str = "menu    prty3";
const PRTY4: &str = "menu    prty4";
const PRTY7: &str = "menu    prty7";
const PRTY8: &str = "menu    prty8";
const LANGS: &str = "menu    prty5us";
const ALARM: &str = "menu    alarm";
const JOIN: &str = "menu    prtyjoin";

/// An open window and its cursor child.
#[derive(Clone, Debug)]
pub struct Win {
    pub name: &'static str,
    pub sel: u16,
}

#[derive(Clone, Debug)]
pub struct PartyMenu {
    /// Bottom first: the row's window, then the one opened over it.
    pub stack: Vec<Win>,
    /// menuwind's cursor, back when the bottom window closes (FUN_10205660).
    return_menu: Option<usize>,
    /// prty5us +0x2c: the language mask while it is open.
    langs: u32,
}

/// A pending invite (DAT_106643a6 / DAT_106643a7, the name at prtyjoin +0x14).
#[derive(Clone, Debug)]
pub struct Invite {
    pub alliance: bool,
    pub name: String,
}

/// A party-list pick the menu started (FUN_10202d30's callbacks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// LAB_10204fa0: c2s 0x071 on anyone but us; the menu stays.
    Kick,
    /// LAB_10205020: c2s 0x077 change 0 on anyone; the menu closes (f1).
    Leader,
    /// prty8's Quartermaster: c2s 0x077 change 4; prty8 stays.
    Quartermaster,
}

/// FUN_10201690: 1 an invite pending, 2 solo, 3 in a party, 4 a party of us and trusts.
fn state(w: &World) -> u8 {
    if w.invite.is_some() { 1 } else { w.party.slots().state() }
}

/// The state a window closes itself on leaving ([7]; 0 none).
fn watch(name: &str) -> u8 {
    match name {
        PRTY1 | LANGS => 2,
        PRTY2 | PRTY3 | PRTY7 | PRTY4 | PRTY8 => 3,
        _ => 0,
    }
}

/// The Find Member / Invite child ([5]).
fn find_child(name: &str) -> Option<u16> {
    match name {
        PRTY1 => Some(2),
        PRTY3 | PRTY7 => Some(1),
        _ => None,
    }
}

/// FUN_100e47c0 on the s2c 0x0B4 bytes: 2 seeking a party, 6 autogroup, 0xb request mode.
fn conf(w: &World, which: u8) -> bool {
    let c = &w.conf_sys;
    match which {
        2 => c[0] & 1 != 0,
        6 => c[1] & 0x80 != 0,
        _ => c[3] & 0x20 != 0,
    }
}

/// FUN_102056c0's [6]: the target is someone else seeking a party or in autogroup (0x00D @0x20 bit 11, @0x24 bit 31).
fn seeker(w: &World) -> Option<(u32, u16)> {
    let t = w.target.filter(|&t| t != w.my_id)?;
    let e = w.entities.get(&t).filter(|e| e.marks.lfg || e.marks.auto)?;
    Some((t, e.index))
}

/// FUN_102053f0: a target that is not an NPC (id >> 24 != 1); ourselves too.
fn target_pc(w: &World) -> Option<(u32, u16)> {
    let t = w.target.filter(|&t| t != 0 && t >> 24 != 1)?;
    if t == w.my_id {
        return Some((t, w.my_index));
    }
    w.entities.get(&t).map(|e| (t, e.index))
}

/// A member of our party with level sync on (GAttr 0x100, FUN_10203190).
fn synced(w: &World) -> bool {
    w.party.party().any(|m| m.flags & 0x100 != 0)
}

/// FUN_102016e0: prtyjoin while an invite is pending, prty3 for the alliance leader or a party leader with no
/// alliance, prty7 for a party leader in someone else's alliance, prty2 in a party, else prty1. The main menu closes
/// (FUN_10205570) and comes back on the Party row when it closes.
pub fn open(w: &mut World) {
    let s = w.party.slots();
    let name = if w.invite.is_some() {
        JOIN
    } else if s.alliance_leader == w.my_id {
        PRTY3
    } else if s.leaders[0] == w.my_id {
        if s.alliance_leader != 0 { PRTY7 } else { PRTY3 }
    } else if s.leaders[0] != 0 {
        PRTY2
    } else {
        PRTY1
    };
    w.partymenu = Some(PartyMenu { stack: vec![Win { name, sel: 1 }], return_menu: w.menu, langs: 0 });
    w.menu = None;
}

fn push(w: &mut World, name: &'static str) {
    if let Some(m) = w.partymenu.as_mut() {
        m.stack.push(Win { name, sel: 1 });
    }
}

/// The disabled children per frame (FUN_10205890, FUN_10205ff0, FUN_102061e0, FUN_102062c0).
fn disabled(w: &World, name: &str) -> Vec<u16> {
    let s = w.party.slots();
    let alliance = s.alliance_leader != 0;
    let alone = s.leaders[0] != 0 && s.counts[0] == 1;
    let mut off = Vec::new();
    match name {
        PRTY1 if w.search_words.as_ref().is_some_and(|sw| sw.zone_flags(w.pos_zone) & 8 == 0) => off.push(3),
        PRTY3 | PRTY7 => {
            let form = if name == PRTY3 { alliance || target_pc(w).is_some() } else { alliance };
            if !form {
                off.push(7);
            }
            if alliance {
                off.push(6);
            }
            if alone {
                off.extend([4, 5]);
            }
        }
        PRTY4 => {
            if target_pc(w).is_none() {
                off.push(1);
            }
            if !alliance {
                off.push(2);
            }
        }
        _ => {}
    }
    off
}

/// The label sprites (windowps) swapped in per frame (FUN_10205890, FUN_10205d40, FUN_10205e70, FUN_102054a0).
fn swaps(w: &World, name: &str) -> Vec<(u16, usize)> {
    let request = |c: u16| (c, if conf(w, 0xb) { 780 } else { 778 });
    match name {
        PRTY1 => {
            let auto = if disabled(w, PRTY1).contains(&3) {
                485
            } else if conf(w, 6) {
                484
            } else {
                474
            };
            vec![(1, if conf(w, 2) { 257 } else { 78 }), (3, auto), request(5)]
        }
        PRTY2 => vec![request(6)],
        PRTY3 | PRTY7 => vec![request(11), (8, if synced(w) { 613 } else { 611 })],
        _ => Vec::new(),
    }
}

/// The help line (slot 1) where the window sets its own: Find Member / Invite (0xe 0x10 / 0xf), Lv. Sync Off (0xe
/// 0x2a3), prtyjoin's question (group 10 0x00 / 0x01 with the name, FUN_10206350).
fn help(gu: &GameUi, w: &World, win: &Win) -> Option<String> {
    if find_child(win.name) == Some(win.sel) {
        return Some(gu.menu_msg(if seeker(w).is_some() { 0xf } else { 0x10 }, ""));
    }
    match win.name {
        PRTY3 | PRTY7 if win.sel == 8 && synced(w) => Some(gu.menu_msg(0x2a3, "")),
        JOIN => {
            let inv = w.invite.as_ref()?;
            let q = w.battle.as_ref()?.group10(if inv.alliance { 1 } else { 0 })?;
            Some(q.replacen("%s", &inv.name, 1))
        }
        _ => None,
    }
}

/// Per frame (slot 10, FUN_102056c0): a window watching the party state closes when it changes (an invite, a join, a
/// leave), and the whole menu with it *(inf: retail closes each watching window, the others stay)*.
pub fn update(w: &mut World) {
    let Some(m) = w.partymenu.as_ref() else { return };
    let s = state(w);
    if m.stack.iter().any(|win| watch(win.name) != 0 && watch(win.name) != s) {
        while w.partymenu.is_some() {
            close_top(w);
        }
    }
}

/// The top window closes (FUN_10205660 and the windows' own close hooks): prty5us sends a changed language mask; the
/// bottom one closes the search results and the party-list cursor and brings the main menu back.
fn close_top(w: &mut World) {
    let Some(m) = w.partymenu.as_mut() else { return };
    let Some(top) = m.stack.pop() else { return };
    let (empty, langs, back) = (m.stack.is_empty(), m.langs, m.return_menu);
    if top.name == LANGS && langs != w.game.get(0x98) {
        super::textcmd::cfg(w, 0x98, langs);
        send_langs(w, langs);
    }
    if top.name != ALARM && top.name != JOIN {
        w.search = None;
        if w.party_cursor.as_ref().is_some_and(|pc| matches!(pc.then, SubThen::Party(_))) {
            w.party_cursor = None;
        }
    }
    if empty {
        w.partymenu = None;
        w.menu = back;
    }
}

/// c2s 0x0DB kind 1 (FUN_100e4630): @8..@0x14 the three config words, @0x24 the party languages.
fn send_langs(w: &World, mask: u32) {
    let mut b = vec![0u8; 0x24];
    b[2] = 1;
    b[4..16].copy_from_slice(&w.conf_sys);
    b[0x20..0x24].copy_from_slice(&mask.to_le_bytes());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0DB, body: b });
}

/// Table 0x10385388 for an English client: prty5us row 1..5 (English, French, German, Japanese, Other) -> bit.
const LANG_BITS: [u32; 5] = [1, 3, 2, 0, 4];

fn packet(w: &World, id: u16, body: Vec<u8>) {
    let _ = w.net.commands.send(NetCommand::Packet { id, body });
}

/// FUN_10205280: the treasure setting in the log (mode 0x79): group 10 0x02 with the quartermaster (in our party: GAttr
/// 0x10, FUN_10203130), else group 9 0x00. Not done: the auto-pool area's group 9 0xc2 (actor +0x18c).
fn show_distribution(w: &mut World) {
    let Some(bt) = w.battle.clone() else { return };
    let qm = w.party.party().find(|m| m.flags & 0x10 != 0).map(|m| m.name.clone());
    let line = match qm {
        Some(n) => bt.group10(2).map(|s| s.replacen("%s", &n, 1)),
        None => bt.group9(0),
    };
    let colour = bt.colour_in(0x79, &w.game);
    w.chat.push_colour(line.unwrap_or_default(), colour);
}

/// Find Member / Invite: c2s 0x06E {target, ActIndex 0, kind 0} on a seeker (FUN_10204f70), else the seeker search
/// (FUN_10204c60, `/sea [region] inv`: the region when the zone has flag 2 and one, else this area).
fn find_member(w: &mut World) {
    if let Some((id, _)) = seeker(w) {
        packet(w, 0x06E, super::party::invite_body(id, 0, 0));
        return;
    }
    if w.search.is_some() || !w.recasts_seen {
        return;
    }
    use crate::net::search::{AREA, Cond, STATUS};
    let region = w.search_words.as_ref().filter(|sw| sw.zone_flags(w.pos_zone) & 2 != 0).and_then(|sw| sw.region_of(w.pos_zone)).filter(|r| !r.is_empty());
    let (mut cond, scope) = match region {
        Some(r) => {
            let last = *r.last().unwrap() as i16;
            (Cond { flags: AREA, areas: r, last_area: last, area_given: true, ..Default::default() }, super::search::Scope::Region)
        }
        None => (Cond::zone(w.pos_zone), super::search::Scope::Area),
    };
    cond.flags |= STATUS;
    cond.status = 0x8020;
    super::search::run(w, cond, scope);
    // Title slot 2: 0xd 0x13 "Party".
    if let Some(r) = w.search.as_mut() {
        r.title = 0x13;
    }
}

/// The party-list cursor over our six slots for a pick (FUN_10202d30 mode 0).
fn pick(w: &mut World, p: Pick) {
    super::target::open_party_cursor(w, 6, SubThen::Party(p), None);
}

/// A pick confirmed on slot member (id, name); None on Escape. Kick refuses us; Leader closes the menu after.
pub fn picked(w: &mut World, p: Pick, who: Option<(u32, String)>) {
    let Some((id, name)) = who else { return };
    match p {
        Pick::Kick if id != w.my_id => packet(w, 0x071, super::party::kick_body(0, &name)),
        Pick::Kick => {}
        Pick::Leader => {
            packet(w, 0x077, super::party::change_body(&name, 0, 0));
            while w.partymenu.is_some() {
                close_top(w);
            }
        }
        Pick::Quartermaster => packet(w, 0x077, super::party::change_body(&name, 0, 4)),
    }
}

fn not_done(name: &str, sel: u16) {
    log::info!("party menu: {} row {sel} is not implemented yet", name.trim_start_matches("menu    "));
}

/// The row actions (FUN_102059a0, FUN_10205da0, FUN_10205ed0, FUN_10206110, FUN_10206280, FUN_10206330, FUN_10205ba0,
/// FUN_10205cc0, FUN_102063d0).
fn enter(w: &mut World, name: &'static str, sel: u16) {
    let request = |w: &mut World| super::textcmd::config_flag(w, 0xb, !conf(w, 0xb));
    match (name, sel) {
        (PRTY1, 1) => super::textcmd::config_flag(w, 2, !conf(w, 2)),
        (PRTY1, 2) | (PRTY3 | PRTY7, 1) => find_member(w),
        (PRTY1, 5) | (PRTY2, 6) | (PRTY3 | PRTY7, 11) => request(w),
        (PRTY1, 3) => super::textcmd::config_flag(w, 6, !conf(w, 6)),
        // Languages: the mask from config 0x98 (none: the client language's own), sent once on the first show.
        (PRTY1, 4) => {
            let mut mask = w.game.get(0x98);
            if mask == 0 {
                mask = 1 << LANG_BITS[0];
            }
            if let Some(m) = w.partymenu.as_mut() {
                m.langs = mask;
            }
            send_langs(w, mask);
            push(w, LANGS);
        }
        (PRTY1, 7) => push(w, ALARM),
        (PRTY2, 2) | (PRTY7, 3) => show_distribution(w),
        (PRTY2, 3) => packet(w, 0x06F, vec![0, 0, 0, 0]),
        (PRTY3, 3) => push(w, PRTY8),
        (PRTY3 | PRTY7, 4) => pick(w, Pick::Leader),
        (PRTY3 | PRTY7, 5) => pick(w, Pick::Kick),
        // FUN_100e9090: 0x070 only with our own leader flag (GAttr 4 party, 8 alliance).
        (PRTY3 | PRTY7, 6) if w.party.own_flags & 4 != 0 => packet(w, 0x070, vec![0, 0, 0, 0]),
        (PRTY3, 7) => push(w, PRTY4),
        (PRTY7, 7) => packet(w, 0x06F, vec![5, 0, 0, 0]),
        (PRTY4, 1) => {
            if let Some((id, index)) = target_pc(w) {
                packet(w, 0x06E, super::party::invite_body(id, index, 5));
            }
            close_top(w);
        }
        (PRTY4, 2) => {
            if w.party.own_flags & 8 != 0 {
                packet(w, 0x070, vec![5, 0, 0, 0]);
            }
            close_top(w);
        }
        (PRTY8, 1) => {
            let qms: Vec<String> = w.party.party().filter(|m| m.flags & 0x10 != 0).map(|m| m.name.clone()).collect();
            for n in qms {
                packet(w, 0x077, super::party::change_body(&n, 0, 5));
            }
            close_top(w);
        }
        (PRTY8, 2) => pick(w, Pick::Quartermaster),
        // An emptied mask gets the first row's bit back (SE 0004 when that row was the one cleared).
        (LANGS, 1..=5) => {
            let Some(m) = w.partymenu.as_mut() else { return };
            m.langs ^= 1 << LANG_BITS[sel as usize - 1];
            if m.langs == 0 {
                m.langs = 1 << LANG_BITS[0];
                if sel == 1 {
                    w.sounds.push(Ui::Error as u32);
                }
            }
        }
        // Config 199 = the row - 1 (0 OFF: SE 0002), then that alarm plays.
        (ALARM, 1..=8) => {
            let n = sel as u32 - 1;
            super::textcmd::cfg(w, 0xc7, n);
            if n == 0 {
                w.sounds.push(Ui::Select as u32);
            }
            play_alarm(w, n);
        }
        (JOIN, _) => {
            if matches!(sel, 1 | 2) {
                packet(w, 0x074, vec![(sel == 1) as u8, 0, 0, 0]);
            }
            w.invite = None;
            close_top(w);
        }
        _ => not_done(name, sel),
    }
}

/// FUN_10038070: alarm n (table 0x1032a8e4; 0 none).
fn play_alarm(w: &mut World, n: u32) {
    const SE: [u32; 8] = [0, 27, 31, 33, 34, 38, 29, 22];
    if let Some(&id) = SE.get(n as usize).filter(|&&id| id != 0) {
        w.sounds.push(id);
    }
}

/// FUN_101cc590: the pronoun by race (0 male, 1 female, 2 other).
fn pronoun(race: u16) -> i32 {
    match race {
        1 | 3 | 5 | 8 => 0,
        2 | 4 | 6 | 7 => 1,
        _ => 2,
    }
}

/// s2c 0x0DC (LAB_10203a50): unless AnonFlag bit 1, group 10 0x04 (party) / 0x03 (alliance) in mode 0x87 with the
/// name and pronoun (2 when AnonFlag bit 0); a party invite plays the alarm (config 199). The invite is then pending;
/// nothing opens by itself. Not done: the 0x7F 0xFC / 0xFB codes around the name in the party line (the log draws no
/// 0x7F codes yet).
pub fn invited(w: &mut World, anon: u8, kind: u8, name: String, race: u16) {
    if matches!(kind, 0 | 5) && anon & 2 == 0
        && let Some(bt) = w.battle.clone()
        && let Some(raw) = bt.group10_raw(if kind == 0 { 4 } else { 3 })
    {
        let p = if anon & 1 != 0 { 2 } else { pronoun(race) };
        let sjis = crate::dat::font::encode_sjis(&name);
        let text = crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&sjis], &[p], &|_, _, _| None));
        let colour = bt.colour_in(0x87, &w.game);
        w.chat.push_colour(text, colour);
    }
    if kind == 0 {
        play_alarm(w, w.game.get(0xc7));
    }
    w.invite = Some(Invite { alliance: kind != 0, name });
}

/// The window events after the generic part (FUN_10118260): Up / Down along the links (SE 0001), Escape (SE 0003)
/// closes the top window, Enter on a disabled row SE 0004 only, else SE 0002 (not alarm: +0x86 = 0) and the row.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(top) = w.partymenu.as_ref().and_then(|m| m.stack.last()).cloned() else { return };
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let k = if key == egui::Key::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(top.name, top.sel).map_or(top.sel, |l| l[k] as u16);
            if to != top.sel && to != 0 && to != 0xff
                && let Some(t) = w.partymenu.as_mut().and_then(|m| m.stack.last_mut())
            {
                t.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close_top(w);
        }
        egui::Key::Enter if disabled(w, top.name).contains(&top.sel) => w.sounds.push(Ui::Error as u32),
        egui::Key::Enter => {
            if top.name != ALARM {
                w.sounds.push(Ui::Select as u32);
            }
            enter(w, top.name, top.sel);
        }
        _ => {}
    }
}

/// The top window takes the keys unless the results window, a prompt or a party-list pick is up over it.
pub fn focused(w: &World) -> bool {
    w.partymenu.is_some() && w.search.is_none() && w.confirm.is_none() && w.party_cursor.is_none()
}

/// Mouse: hover over a row moves the cursor, a click on the cursor row is Enter *(inf: the generic child hit test)*.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(top) = w.partymenu.as_ref().and_then(|m| m.stack.last()).cloned() else { return };
    let Some((wx, wy)) = gu.window_xy(top.name, sw, sh) else { return };
    let Some(layout) = gu.layout(top.name) else { return };
    let hit = layout.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).find(|e| {
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
    });
    let Some(e) = hit else { return };
    gu.hover.set(true);
    if (moved || clicked) && e.id != top.sel
        && let Some(t) = w.partymenu.as_mut().and_then(|m| m.stack.last_mut())
    {
        t.sel = e.id;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The top window's help (slot 1, its own where it sets one) and title (frame string 1, group 0xd).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(top) = w.partymenu.as_ref().and_then(|m| m.stack.last()) else { return };
    let [h, title] = gu.focus_text(top.name, top.sel);
    bars.help[1] = help(gu, w, top).or(h).unwrap_or_default();
    bars.title[1] = title.unwrap_or_default();
}

/// Every open window, bottom first: the layout with its swapped labels and disabled rows; the Find Member child drawn
/// by the window (FUN_10205790: 82 / 79 at its place, 0x80805020 under the cursor); prty5us's check marks (keytops3
/// 431, FUN_10205b10) and alarm's mark (windowps 483 at child config 199 + 1, FUN_10205c50).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(m) = &w.partymenu else { return rects };
    let n = m.stack.len();
    for (k, win) in m.stack.iter().enumerate() {
        let focused = k + 1 == n && focused(w);
        let off = disabled(w, win.name);
        let find = find_child(win.name);
        let mut hide = off.clone();
        hide.extend(find);
        let Some(lw) = gu.lobby_window_swaps(win.name, sw, sh, (0.0, 0.0), Some(win.sel), focused, &hide, &swaps(w, win.name)) else { continue };
        let Some((wx, wy)) = gu.window_xy(win.name, sw, sh) else { continue };
        gu.disabled_children(win.name, wx, wy, &off, Some(win.sel).filter(|_| focused));
        let at: Vec<(u16, f32, f32)> = gu.layout(win.name).map_or(Vec::new(), |l| l.elements.iter().skip(1).map(|e| (e.id, wx + e.x as f32, wy + e.y as f32)).collect());
        let child = |id: u16| at.iter().find(|c| c.0 == id).map(|c| (c.1, c.2));
        if let Some(c) = find
            && let Some((cx, cy)) = child(c)
        {
            let on = win.sel == c;
            gu.sprite("menu    windowps", if seeker(w).is_some() { 79 } else { 82 }, cx, cy, if on { 0x8080_5020 } else { 0x8080_8080 });
            if on && focused {
                gu.cursor(cx, cy);
            }
        }
        match win.name {
            LANGS => {
                for (row, bit) in LANG_BITS.iter().enumerate() {
                    if m.langs & 1 << bit != 0
                        && let Some((cx, cy)) = child(row as u16 + 1)
                    {
                        gu.sprite("menu    keytops3", 431, cx, cy, 0x8080_8080);
                    }
                }
            }
            ALARM => {
                if let Some((cx, cy)) = child(w.game.get(0xc7) as u16 + 1) {
                    gu.sprite("menu    windowps", 483, cx, cy, 0x8080_8080);
                }
            }
            _ => {}
        }
        rects.push(lw.frame);
    }
    rects
}

/// While an invite is pending the party window shows keytops3 69 centred on its top-left corner (FUN_102041d0).
pub fn draw_invite_icon(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    if w.invite.is_none() {
        return;
    }
    let name = gu.party_window_name().to_string();
    if let (Some([x, y, ..]), Some(b)) = (gu.window_rect(&name, sw, sh), gu.cell_bounds("menu    keytops3", 69, 0)) {
        gu.sprite("menu    keytops3", 69, x - ((b[2] - b[0]) / 2.0).trunc(), y - ((b[3] - b[1]) / 2.0).trunc(), 0x8080_8080);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pronouns() {
        assert_eq!([0, 1, 2, 3, 7, 8, 9].map(pronoun), [2, 0, 1, 0, 1, 0, 2]);
    }
}
