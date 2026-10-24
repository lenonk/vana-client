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

/// FUN_10129e80 / FUN_10129de0: mode word 0x379.
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
/// page by 15, Escape closes (FUN_101f67c0); Num+ (list message 8) flips the job cells.
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
        // A page of 15 (+0x18).
        K::ArrowRight | K::ArrowLeft => {
            let c = if key == K::ArrowRight { (res.cursor + ROWS).min(n - 1) } else { res.cursor.saturating_sub(ROWS) };
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

/// Status icons, keytops3 by FUN_1020a740 index k (W+0x898; 0 / 1 draw nothing).
const STATUS: [usize; 12] = [0, 0, 68, 69, 352, 70, 65, 67, 362, 550, 365, 364];
/// Comment-genre dots, windowps by index k - 12.
const GENRE: [usize; 10] = [492, 495, 493, 496, 494, 499, 498, 71, 70, 70];
/// pname race words by model race (DAT_1037d36f).
const RACE: [usize; 9] = [0, 0, 0, 1, 1, 2, 2, 3, 4];

const SCRESULT: &str = "menu    scresult";
const SCOPTION: &str = "menu    scoption";

/// FUN_1020a740's status icon index and the Ballista team (ncol, icon index) by the record flags.
fn status_icon(f: u32) -> (usize, Option<(usize, usize)>) {
    let team = match f & 0xe0000 {
        0x20000 => Some((0x12, 0x16)),
        0x40000 => Some((0x13, 0x17)),
        0x60000 => Some((0x14, 0x18)),
        0x80000 | 0xc0000 => Some((0x15, 0x0b)),
        0xa0000 | 0xe0000 => Some((0x16, 0x0a)),
        _ => None,
    };
    let k = if f & 0x4000 != 0 {
        7
    } else if f & 0x400 != 0 {
        5
    } else if f & 0x800 != 0 {
        2
    } else if let Some((_, icon)) = team {
        icon
    } else if f & 0x100 != 0 {
        6
    } else if f & 0x20 != 0 {
        4
    } else if f & 0x8000 != 0 {
        3
    } else if f & 0x1000_0000 != 0 {
        9
    } else if f & 1 != 0 {
        8
    } else {
        1
    };
    (k, team)
}

/// The icon array W+0x898 by index: keytops3 0..11, windowps 12..21, pname 22..24.
fn icon(k: usize) -> Option<(&'static str, usize)> {
    match k {
        0..=11 => Some(("menu    keytops3", STATUS[k])),
        12..=21 => Some(("menu    windowps", GENRE[k - 12])),
        22..=24 => Some(("menu    pname", 27 + k - 22)),
        _ => None,
    }
}

/// Two pname digits; the tens drawn always (`lead`) or only from 10.
fn digits(gu: &mut super::game_ui::GameUi, v: u8, x: f32, y: f32, step: f32, lead: bool, c: u32) {
    if lead || v >= 10 {
        gu.sprite("menu    pname", 30 + (v as usize / 10) % 10, x, y, c);
    }
    gu.sprite("menu    pname", 30 + v as usize % 10, x + step, y, c);
}

/// FUN_101ca6c0 (default) / FUN_101caa10 (Num+): the job / level cell at (x, y) = (row + 18, row + 2).
fn job_cell(gu: &mut super::game_ui::GameUi, r: &Record, x: f32, y: f32, c: u32, alt: bool) {
    const G: u32 = 0x8080_8080;
    gu.sprite("menu    pname", if alt { 45 } else { 0 }, x, y, G);
    if r.present & 1 << 6 == 0 || r.flags & 0x4000 != 0 {
        return;
    }
    let job = |j: u8| (1..=22).contains(&j).then_some(j as usize);
    let rank = if r.rank == 0 { 1 } else { r.rank };
    if alt {
        if let Some(j) = job(r.job.0) {
            gu.sprite("menu    pname", j, x + 17.0, y + 1.0, c);
        }
        if r.flags & 0x4000_0000 != 0 {
            digits(gu, r.master_level, x + 40.0, y + 1.0, 7.0, true, c);
        } else {
            gu.sprite("menu    pname", 46, x + 40.0, y + 1.0, G);
        }
        if let Some(j) = job(r.job.1) {
            gu.sprite("menu    pname", j, x + 59.0, y + 1.0, c);
            digits(gu, r.level.1, x + 82.0, y + 1.0, 7.0, true, c);
        }
        gu.sprite("menu    pname", 27 + r.nation as usize, x + 103.0, y + 1.0, G);
        digits(gu, rank, x + 115.0, y + 3.0, 6.0, false, G);
        return;
    }
    if let Some(j) = job(r.job.0) {
        gu.sprite("menu    pname", j, x + 4.0, y + 1.0, c);
        digits(gu, r.level.0, x + 27.0, y + 1.0, 7.0, true, c);
    }
    if let Some(j) = job(r.job.1) {
        gu.sprite("menu    pname", j, x + 45.0, y + 1.0, c);
        digits(gu, r.level.1, x + 68.0, y + 1.0, 7.0, true, c);
    }
    let mut pen = x + 85.0;
    if (r.race as usize) < RACE.len() && r.job.0 != 0x17 && r.job.1 != 0x17 {
        gu.sprite("menu    pname", 40 + RACE[r.race as usize], pen, y + 1.0, G);
    }
    pen += 18.0;
    gu.sprite("menu    pname", 27 + r.nation as usize, pen, y + 1.0, G);
    pen += 12.0;
    digits(gu, rank, pen, y + 3.0, 6.0, false, G);
}

/// scresult (FUN_101f6a60 with FUN_1020c060 / FUN_1020a740 rows, scresult_draw_spec) and scoption over it.
pub fn draw(gu: &mut super::game_ui::GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const G: u32 = 0x8080_8080;
    let Some(res) = &w.search else { return Vec::new() };
    let mut rects = Vec::new();
    let focused = res.option.is_none();
    let sel = if focused { (res.cursor - res.scroll + 1) as u16 } else { 0 };
    let Some((wx, wy)) = gu.menu_sel(SCRESULT, sw, sh, sel) else { return rects };
    rects.extend(gu.window_rect(SCRESULT, sw, sh));
    let bt = w.battle.clone();
    let rx = wx + 4.0;
    if res.failed {
        // "Search failed." (group 7 0x65) in the game font, the list cleared.
        let s = bt.as_ref().and_then(|b| b.group7(0x65)).unwrap_or_default();
        gu.text(&s, wx + 36.0, wy + 28.0, 0, crate::dat::battle::argb_bytes(0x8080_4040));
        return rects;
    }
    let count = res.rows.len() + 1;
    for i in res.scroll..count.min(res.scroll + ROWS) {
        let ry = wy + 4.0 + 16.0 * (i - res.scroll) as f32;
        if i == 0 {
            // The header: windowps 323 and " [Searching]" / " [%d]" at +150 in 0x60808080.
            gu.sprite("menu    windowps", 323, rx - 3.0, ry, G);
            let s = if res.busy { bt.as_ref().and_then(|b| b.group9(0x27)).unwrap_or_default() } else { format!(" [{}]", res.total) };
            gu.shape_text(&s, rx + 150.0, ry, 0x6080_8080, false);
            continue;
        }
        let r = &res.rows[i - 1];
        let f = r.flags;
        let (k, team) = status_icon(f);
        let tc = team.map_or(G, |t| gu.ncol(t.0));
        if let Some((lib, s)) = icon(k).filter(|_| k > 1) {
            if f & 0x10000 != 0 || team.is_some() {
                let c = if team.is_some() { 0xffff_ffff } else { G };
                gu.sprite_scaled(lib, s, rx - 4.0, ry + 2.0, 0.9, 0.9, c);
            } else {
                gu.sprite(lib, s, rx, ry + 2.0, G);
            }
        }
        // The Ballista case's second icon at 0.8, centred on its bounds.
        if team.is_some() {
            let k2 = if f & 0x20 != 0 {
                4
            } else if f & 0x8000 != 0 {
                3
            } else if f & 0x1000_0000 != 0 {
                9
            } else {
                1
            };
            if let Some((lib, s)) = icon(k2).filter(|_| k2 > 1) {
                let b = gu.cell_bounds(lib, s, 0).unwrap_or([0.0; 4]);
                gu.sprite_scaled(lib, s, rx + ((b[2] - b[0]) / 2.0).trunc(), ry + 2.0 + ((b[3] - b[1]) / 2.0).trunc(), 0.8, 0.8, G);
            }
        }
        job_cell(gu, r, rx + 18.0, ry + 2.0, tc, res.alt);
        if f & 0x10 != 0 {
            let k = ((r.comment >> 4) & 0xf) as usize + 0xb;
            if let Some((lib, s)) = icon(k).filter(|_| k < 0x19) {
                gu.sprite(lib, s, rx + 146.0, ry + 2.0, G);
            }
        }
        let nc = if f & 8 != 0 {
            gu.ncol(5)
        } else if f & 0x2000 != 0 {
            gu.ncol(1)
        } else {
            G
        };
        gu.shape_fit(&r.name, rx + 150.0, ry + 2.0, 80.0, nc);
        if f & 0x1000_8021 != 0 {
            for b in 0..5 {
                if r.languages >> b & 1 != 0 {
                    gu.sprite("menu    windowps", 595 + b, rx + 238.0 + 6.0 * b as f32, ry + 5.0, G);
                }
            }
        }
        if r.zone != w.pos_zone {
            let mut s = crate::dat::font::decode_sjis(&gu.area_name(r.zone));
            if s.is_empty() {
                s = bt.as_ref().and_then(|b| b.group9(0x40)).unwrap_or_default();
            }
            gu.shape_text(&s, rx + 270.0, ry + 2.0, G, false);
        }
    }
    // The cursor bar (windowps 75) while the list has the focus.
    if focused {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (res.cursor - res.scroll) as f32, G);
    }
    // The scroll bar (menu scroll 0..3) past 15 rows: caps, thumb, track at x1 - 8.
    if count > ROWS {
        let (x, y, h) = (wx + 358.0, wy, 248.0);
        let len = (((h - 9.0) * ROWS as f32 + count as f32) / count as f32).trunc();
        let off = ((h - 8.0 - len) * res.scroll as f32 / (count - ROWS) as f32).trunc();
        gu.cell_stretch("menu    scroll", 0, 0, x, y, 8.0, 4.0, G);
        gu.cell_stretch("menu    scroll", 3, 0, x + 1.0, y + 4.0 + off, 6.0, len, G);
        gu.cell_stretch("menu    scroll", 1, 0, x, y + 4.0, 8.0, h - 8.0, G);
        gu.cell_stretch("menu    scroll", 2, 0, x, y + h - 4.0, 8.0, 4.0, G);
    }
    if let Some(c) = res.option {
        let r = res.cursor.checked_sub(1).and_then(|i| res.rows.get(i));
        let off: Vec<u16> = OPTION_ORDER.iter().copied().filter(|&o| r.is_some_and(|r| !option_enabled(w, o, r))).collect();
        if let Some(win) = gu.lobby_window_off(SCOPTION, sw, sh, (0.0, 0.0), Some(c), !off.contains(&c), &off) {
            gu.disabled_children(SCOPTION, win.frame[0], win.frame[1], &off, Some(c));
            rects.push(win.frame);
        }
    }
    rects
}

/// Title slot 2 "Search" (group 0xd 0); help slot 2 the row's text: the header 0xe 0x13, a mentor 0xe 0x259; scoption's
/// rows their own.
pub fn bars(gu: &super::game_ui::GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(res) = &w.search else { return };
    bars.title[2] = gu.title_msg(0).unwrap_or_default();
    if let Some(c) = res.option {
        let [help, title] = gu.focus_text(SCOPTION, c);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        return;
    }
    bars.help[2] = match res.cursor {
        0 => gu.menu_msg(0x13, ""),
        i => res.rows.get(i - 1).filter(|r| status_icon(r.flags).0 == 8).map(|_| gu.menu_msg(0x259, "")).unwrap_or_default(),
    };
}
