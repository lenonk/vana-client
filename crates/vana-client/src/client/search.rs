//! The search server's users: /search and its results window "menu    scresult" (W = DAT_10664494, FUN_1020a9d0,
//! callback LAB_1020ac30), the actions on a person "menu    scoption" (FUN_1020c540 / FUN_1020c9a0), and
//! FUN_1007c740's name lookups (all areas, name only; the callbacks take an exact match).

use super::World;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::net::search::{self, Cond, Record, Reply};

/// What a finished query is for.
pub enum Then {
    /// LAB_1007d5b0: /pcmd or /acmd add for someone not in the zone.
    PartyAdd(String),
    /// LAB_1020ac30: the results window's rows.
    Results,
}

/// Which summary line the results get (list +0x16): one area, several, none (all areas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    Area,
    Region,
    World,
}

/// The results window (W) and its list (DAT_106644dc).
pub struct Results {
    /// DAT_10385a04 != 4: a query is out.
    pub busy: bool,
    pub failed: bool,
    pub scope: Scope,
    pub rows: Vec<Record>,
    pub total: u16,
    /// Row 0 is the header.
    pub cursor: usize,
    pub scroll: usize,
    /// List message 8 (Num+): the job cells' other view (row +0x50 bit 0).
    pub alt: bool,
    /// scoption's cursor (child id) while it is open.
    pub option: Option<u16>,
}

/// The rows the window shows at once (its 15 row children).
pub const ROWS: usize = 15;

/// FUN_1007c740: the name, first letter upper and the rest lower (FUN_101c8ef0), at most 15 chars, searched in every
/// area by name only.
pub fn lookup(w: &mut World, name: &str, then: impl FnOnce(String) -> Then) {
    let mut chars = name.chars().take(15);
    let name: String = chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars.map(|c| c.to_ascii_lowercase())).collect();
    log::info!("search: name lookup {name}");
    let rx = search::spawn(w.search_addr, Cond::Name(name.clone()));
    w.searches.push((rx, then(name)));
}

/// FUN_1007c570 (/search, /sea): nothing while the results window is up; no arguments search the current zone, "all"
/// every area. (The full grammar, FUN_10104f00, is not done: other words still search the current zone.)
pub fn command(w: &mut World, args: &[String]) {
    if w.search.is_some() {
        return;
    }
    let (cond, scope) = match args.get(1).map(|a| a.to_ascii_uppercase()).as_deref() {
        Some("ALL") => (Cond::All, Scope::World),
        _ => (Cond::Zone(w.pos_zone), Scope::Area),
    };
    run(w, cond, scope);
}

/// FUN_1020a9d0 mode 1: the query, and the window open on " [Searching]".
pub fn run(w: &mut World, cond: Cond, scope: Scope) {
    log::info!("search: {cond:?}");
    let rx = search::spawn(w.search_addr, cond);
    w.searches.push((rx, Then::Results));
    w.search = Some(Results { busy: true, failed: false, scope, rows: Vec::new(), total: 0, cursor: 0, scroll: 0, alt: false, option: None });
}

/// FUN_100e0d50's delivery for finished queries (a failure or the 30 s timeout delivers nothing).
pub fn poll(w: &mut World) {
    let mut i = 0;
    while i < w.searches.len() {
        match w.searches[i].0.try_recv() {
            Ok(r) => {
                let (_, then) = w.searches.remove(i);
                finish(w, r.ok(), then);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
            Err(_) => {
                let (_, then) = w.searches.remove(i);
                finish(w, None, then);
            }
        }
    }
}

fn finish(w: &mut World, reply: Option<Reply>, then: Then) {
    match then {
        Then::PartyAdd(name) => {
            let Some(reply) = reply else { return };
            // An exact match: c2s 0x06E with its id, ActIndex 0, kind 5 to a party leader (rec+0x30 bit 3), else 0.
            let Some(r) = reply.records.iter().find(|r| r.name == name) else {
                log::info!("search: {name} not found ({} of {})", reply.records.len(), reply.total);
                return;
            };
            log::info!("search: {name} is {:#x} in zone {}", r.id, r.zone);
            invite(w, r.id, r.flags);
        }
        Then::Results => {
            let Some(res) = w.search.as_mut() else { return };
            res.busy = false;
            let Some(reply) = reply else {
                res.failed = true;
                return;
            };
            res.total = reply.total;
            res.rows = reply.records;
            let (scope, shown, total) = (res.scope, res.rows.len(), reply.total as usize);
            summary(w, scope, total, shown);
        }
    }
}

/// The summary line: group 10 0x05 (one area) / 0x35 (a region) / 0x06 (all areas) with the count, then 9 0xbf when
/// fewer rows came than were found (LSB sends 40 at most).
fn summary(w: &mut World, scope: Scope, total: usize, shown: usize) {
    use crate::dat::msgfmt;
    let Some(bt) = w.battle.clone() else { return };
    let id = match scope {
        Scope::Area => 0x05,
        Scope::Region => 0x35,
        Scope::World => 0x06,
    };
    let colour = bt.colour_in(SUMMARY_MODE, &w.game);
    if let Some(raw) = bt.group10_raw(id) {
        let text = crate::dat::font::decode_sjis(&msgfmt::format(&raw, &[], &[total as i32], &|_, _, _| None));
        w.chat.push_colour(text, colour);
    }
    if shown < total {
        w.chat.push_colour(bt.group9(0xbf).unwrap_or_default(), colour);
    }
}

/// The summary lines' chat mode *(inf: the system colour)*.
const SUMMARY_MODE: u8 = 0x79;

/// FUN_100e8fd0: c2s 0x06E {UniqueNo, ActIndex 0, kind 5 to a party leader (flag 8), else 0}.
fn invite(w: &World, id: u32, flags: u32) {
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&id.to_le_bytes());
    b[6] = if flags & 8 != 0 { 5 } else { 0 };
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x06E, body: b });
}

/// scoption's rows by child id (the layout's order top to bottom: 1, 4, 5, 2, 3).
pub const OPTION_ORDER: [u16; 5] = [1, 4, 5, 2, 3];

/// Keys while the results window (or scoption over it) is up. The list base: Up / Down stop at the ends, Left / Right
/// page by 10, Escape closes (FUN_101f67c0); Num+ (list message 8) flips the job cells.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(mut res) = w.search.take() else { return };
    if let Some(c) = res.option {
        let at = OPTION_ORDER.iter().position(|&o| o == c).unwrap_or(0);
        match key {
            K::ArrowDown => {
                res.option = Some(OPTION_ORDER[(at + 1) % OPTION_ORDER.len()]);
                w.sounds.push(Ui::Cursor as u32);
            }
            K::ArrowUp => {
                res.option = Some(OPTION_ORDER[(at + OPTION_ORDER.len() - 1) % OPTION_ORDER.len()]);
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                res.option = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter => {
                if let Some(r) = res.cursor.checked_sub(1).and_then(|i| res.rows.get(i)).cloned() {
                    if option_enabled(w, c, &r) {
                        w.sounds.push(Ui::Select as u32);
                        res.option = None;
                        option(w, c, &r);
                        // Tell leaves the window for the chat line; the others keep it.
                        if c == 1 {
                            return;
                        }
                    } else {
                        w.sounds.push(Ui::Error as u32);
                    }
                }
            }
            _ => {}
        }
        w.search = Some(res);
        return;
    }
    let n = res.rows.len() + 1;
    match key {
        K::ArrowDown if res.cursor + 1 < n => {
            res.cursor += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowUp if res.cursor > 0 => {
            res.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowRight | K::ArrowLeft => {
            let c = if key == K::ArrowRight { (res.cursor + 10).min(n - 1) } else { res.cursor.saturating_sub(10) };
            if c != res.cursor {
                res.cursor = c;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            return;
        }
        // FUN_1020c250: a person opens scoption (the comment fetch for flag 0x10 is not done); the header would open
        // scsibori (not done).
        K::Enter if !res.busy && res.cursor > 0 => {
            res.option = Some(1);
            w.sounds.push(Ui::Select as u32);
        }
        K::Enter => w.sounds.push(Ui::Error as u32),
        _ => {}
    }
    res.scroll = res.scroll.min(res.cursor).max((res.cursor + 1).saturating_sub(ROWS));
    w.search = Some(res);
}

/// Num+ in the list (list message 8).
pub fn toggle_view(w: &mut World) {
    if let Some(res) = w.search.as_mut() {
        res.alt = !res.alt;
    }
}

/// FUN_1020c540's disabled rows: 2 Befriend (the Friend List is PlayOnline's), 5 on ourselves, 3 when the person is in
/// our party, a party member but not its leader, recruiting, or on MON.
pub fn option_enabled(w: &World, c: u16, r: &Record) -> bool {
    match c {
        2 => false,
        5 => r.name != w.name,
        3 => {
            let in_party = w.party.members().any(|m| m.id == r.id && m.id != 0);
            !(in_party || (r.flags & 0x2000 != 0 && r.flags & 8 == 0) || r.flags & 0x1000_0000 != 0 || r.job.0 == 0x17 || r.name == w.name)
        }
        _ => true,
    }
}

/// FUN_1020c9a0: 1 the chat line opened on "/tell Name ", 3 Invite (0x06E), 4 / 5 "/partyrequestcmd add / remove Name".
fn option(w: &mut World, c: u16, r: &Record) {
    match c {
        1 => {
            w.chat_input = format!("/tell {} ", r.name);
            w.typing = true;
        }
        3 => invite(w, r.id, r.flags),
        4 | 5 => {
            let line = format!("/partyrequestcmd {} {}", if c == 4 { "add" } else { "remove" }, r.name);
            super::textcmd::run(w, &line, super::textcmd::Mode::Internal);
        }
        _ => {}
    }
}
