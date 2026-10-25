//! The search server's users: /search and its results window "menu    scresult" (W = DAT_10664494, FUN_1020a9d0,
//! callback LAB_1020ac30), the actions on a person "menu    scoption" (FUN_1020c540 / FUN_1020c9a0), and
//! FUN_1007c740's name lookups (all areas, name only; the callbacks take an exact match).

use super::World;
use super::net::NetCommand;
use super::scrollbar::{Msg, rows_abs};
use crate::audio::Ui;
use crate::dat::tables::SearchWords;
use crate::net::search::{self, Cond, Record, Reply};

/// What a finished query is for.
pub enum Then {
    /// LAB_1007d5b0: /pcmd or /acmd add for someone not in the zone.
    PartyAdd(String),
    /// LAB_1020ac30: the results window's rows.
    Results,
    /// FUN_1020a9d0 with p6 = 1: the summary line only (arealist's Monstrosity picks).
    Summary(Scope),
    /// LAB_10204da0's query: the rows of "menu    levelsyn".
    LevelSync,
    /// A linkshell's group list: the rows of "menu    link12" (client/linklist.rs).
    LinkList,
    /// LAB_1007dc00 (kind 0) / LAB_1007dd40 (kind 1): /prcmd add / remove for someone not in the zone.
    Request(String, u8),
    /// LAB_1007c7c0 / LAB_1007c920: /blacklist add (0) / delete (1).
    Black(String, u8),
}

/// Which summary line the results get (list +0x16): one area, several, none (all areas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    Area,
    Region,
    World,
}

/// The results window (W) and its list (DAT_106644dc).
/// A scroll bar or wheel message to the results (FUN_101f6c70; row 0 is the summary line). True when it moved.
pub(super) fn bar(w: &mut World, m: Msg) -> bool {
    let Some(res) = w.search.as_mut().filter(|r| r.option.is_none()) else { return false };
    let n = res.rows.len() + 1;
    rows_abs(m, &mut res.scroll, &mut res.cursor, n, ROWS)
}

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
    /// Title slot 2 (group 0xd): 0 "Search", 0x13 "Party" from the party menu's Find Member (FUN_10204c60).
    pub title: usize,
    /// W+0x58: 1 a search, 2 the request list (languages on every row), 3 a group list (no header help, no summary
    /// line, Enter on the header does nothing).
    pub mode: u8,
}

/// The rows the window shows at once (its 15 row children).
pub const ROWS: usize = 15;

/// FUN_1007c740: the name, first letter upper and the rest lower (FUN_101c8ef0), at most 15 chars, searched in every
/// area by name only.
pub fn lookup(w: &mut World, name: &str, then: impl FnOnce(String) -> Then) {
    let mut chars = name.chars().take(15);
    let name: String = chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars.map(|c| c.to_ascii_lowercase())).collect();
    log::info!("search: name lookup {name}");
    let rx = search::spawn(w.search_addr, Cond::name(name.clone()));
    w.searches.push((rx, then(name)));
}

/// FUN_1007c570 (/search, /sea): nothing while the results window is up; the words after the command through
/// FUN_10104f00, which stops quietly at the first word it cannot use and searches with what it has.
pub fn command(w: &mut World, tail: &str) {
    if w.search.is_some() {
        return;
    }
    w.search_status0 = 0;
    let Some(words) = w.search_words.clone() else {
        log::warn!("search: no word tables in FFXiMain.dll; searching this area");
        run(w, Cond::zone(w.pos_zone), Scope::Area);
        return;
    };
    let ls = |num: usize| w.linkshells.get(num).filter(|l| l.0 != 0).and_then(|&(index, bag)| w.inv.ext.get(&(bag, index))).map_or(0, |e| u32::from_le_bytes([e[0], e[1], e[2], e[3]]));
    let ctx = Ctx { zone: w.pos_zone, region: words.region_of(w.pos_zone), ls: [ls(1), ls(2)], unity: NO_UNITY, known: w.known_zones.as_ref().map(|k| &k[..]) };
    let cond = parse(tail, &words, &ctx);
    let scope = scope_of(&cond);
    run(w, cond, scope);
}

/// FUN_100e3200's unity bits when there is none (ours does not track the own unity yet).
const NO_UNITY: u32 = 0x0f80_0000;

/// FUN_10104f00's inputs besides the text: the current zone, its region's searchable zones (FUN_1020c1c0), the
/// linkshell 1 / 2 ids, the own unity bits and the known-zone bits (s2c 0x008).
pub struct Ctx<'a> {
    pub zone: u16,
    pub region: Option<Vec<u16>>,
    pub ls: [u32; 2],
    pub unity: u32,
    pub known: Option<&'a [u8]>,
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_ascii_whitespace())
}

/// FUN_10105520: the token at `s` (quoted, or up to whitespace) is `word`, case-insensitive and whole; the text
/// after it (and after a closing quote).
fn word<'s>(s: &'s str, word: &str) -> Option<&'s str> {
    let (b, w) = (s.as_bytes(), word.as_bytes());
    if b.first().is_none_or(|c| c.is_ascii_whitespace()) {
        return None;
    }
    let quoted = b[0] == b'"';
    let (mut i, mut j) = (quoted as usize, 0);
    while i < b.len() && !(quoted && b[i] == b'"') && !(!quoted && b[i].is_ascii_whitespace()) {
        if w.get(j) != Some(&b[i].to_ascii_uppercase()) {
            return None;
        }
        (i, j) = (i + 1, j + 1);
    }
    if j != w.len() {
        return None;
    }
    Some(&s[if quoted && i < b.len() { i + 1 } else { i }..])
}

/// The first entry of a word list matching the token: its value and the text after it.
fn find_word<'s>(list: &[(i32, String)], s: &'s str) -> Option<(i32, &'s str)> {
    list.iter().find_map(|(v, w)| word(s, w).map(|rest| (*v, rest)))
}

/// FUN_10105bd0's token: digits with at most one `-` / `<`: (first, second, separator seen, rest), -1 for a missing
/// number.
fn range(s: &str) -> Option<(i32, i32, bool, &str)> {
    let s = skip_ws(s);
    let b = s.as_bytes();
    let (mut v, mut k, mut i) = ([-1i32, -1], 0, 0);
    while i < b.len() && !b[i].is_ascii_whitespace() {
        if b[i].is_ascii_digit() {
            v[k] = 0;
            while i < b.len() && b[i].is_ascii_digit() {
                v[k] = v[k].wrapping_mul(10).wrapping_add((b[i] - b'0') as i32);
                i += 1;
            }
        } else if matches!(b[i], b'-' | b'<') && k == 0 {
            (k, i) = (1, i + 1);
        } else {
            return None;
        }
    }
    (v[0] >= 0 || v[1] >= 0).then_some((v[0], v[1], k == 1, &s[i..]))
}

/// FUN_10105930: an area word, refused for zones 1..0x17f we have no known-zone bit for (none at all without
/// s2c 0x008); ALL is -1.
fn area<'s>(s: &'s str, words: &SearchWords, known: Option<&[u8]>) -> Option<(i32, &'s str)> {
    let (id, rest) = find_word(&words.areas, s)?;
    let bit = |z: i32| known.and_then(|k| k.get(z as usize >> 3)).is_some_and(|b| b >> (z & 7) & 1 != 0);
    (!(0 < id && id < 0x180 && !bit(id))).then_some((id, rest))
}

/// FUN_10105640: a category's value at `s` into the condition; the text after it, None when it does not parse.
fn value<'s>(c: &mut Cond, t: u8, unity: bool, s: &'s str, words: &SearchWords, ctx: &Ctx) -> Option<&'s str> {
    match t {
        // Quotes allowed; the first 15 characters as typed.
        0 => {
            let s = skip_ws(s);
            let (quoted, body) = s.strip_prefix('"').map_or((false, s), |r| (true, r));
            let end = body.find(|ch: char| if quoted { ch == '"' } else { ch.is_ascii_whitespace() }).unwrap_or(body.len());
            c.name = body[..end].chars().take(15).collect();
            let rest = &body[end..];
            Some(if quoted { rest.strip_prefix('"').unwrap_or(rest) } else { rest })
        }
        // Put first unless listed already (16 at most).
        1 => {
            let found = area(s, words, ctx.known);
            c.last_area = found.map_or(-1, |f| f.0 as i16);
            let (id, rest) = found?;
            if id >= 0 && !c.areas.contains(&(id as u16)) {
                c.areas.insert(0, id as u16);
                c.areas.truncate(16);
            }
            Some(rest)
        }
        2 => find_word(&words.nations, s).map(|(v, rest)| {
            c.nation = v as u8;
            rest
        }),
        3 => find_word(&words.jobs, s).map(|(v, rest)| {
            c.job = v as u8;
            rest
        }),
        5 => find_word(&words.races, s).map(|(v, rest)| {
            c.race = v as u8;
            rest
        }),
        // A single level over 255 is 0; a pair clamps, unless its out-of-range side is the larger.
        4 | 7 => {
            let (a, b, pair, rest) = range(s)?;
            c.level = if !pair {
                let v = if a > 255 { 0 } else { a.max(0) as u8 };
                (v, v)
            } else if (a < 256 || b < a) && (b < 256 || a < b) {
                (a.clamp(0, 255) as u8, if (0..=255).contains(&b) { b as u8 } else { 255 })
            } else {
                (0, 0)
            };
            Some(rest)
        }
        // FUN_10105de0: a pair clamps to 0..100, a single value is kept as a byte.
        0x10 => {
            let (a, b, pair, rest) = range(s)?;
            c.rank = if pair { ((if a > 99 { 100 } else { a.max(0) }) as u8, if (0..=99).contains(&b) { b as u8 } else { 100 }) } else { (a as u8, a as u8) };
            Some(rest)
        }
        // FUN_10105ab0: a unity leader, the own unity for UNITY or nothing; an unknown word sets 0x0f800000 and is
        // read again as the next token.
        6 if unity => {
            let (v, rest) = if s.is_empty() { (0, s) } else { find_word(&words.unity, s).unwrap_or((-1, "")) };
            if v < 0 {
                c.status |= NO_UNITY;
                return Some(s);
            }
            let v = if v == 0 { ctx.unity as i32 } else { v };
            if v > 0 {
                c.status |= v as u32;
            }
            Some(rest)
        }
        6 => {
            let (v, rest) = find_word(&words.statuses, s)?;
            (v > 0).then(|| {
                c.status |= v as u32;
                rest
            })
        }
        _ => None,
    }
}

/// FUN_10104f00: `+` / `-` sorts, category keywords and their values, and bare words tried as a status, an area,
/// a level, a master level, a job, a nation, a race and a name, in that order; the first word that fits nothing
/// ends it. No area: the current zone.
pub fn parse(text: &str, words: &SearchWords, ctx: &Ctx) -> Cond {
    use crate::net::search::{AREA, FRIEND, JOB, LEVEL, LS1, LS2, MLEVEL, NAME, NATION, RACE, STATUS};
    let mut c = Cond { last_area: -1, ..Default::default() };
    let mut s = text;
    loop {
        s = skip_ws(s);
        let Some(&first) = s.as_bytes().first() else { break };
        let sign = first == b'+' || (first == b'-' && !s.as_bytes().get(1).is_some_and(u8::is_ascii_digit));
        if sign {
            // STATUS / UNITY, LINKSHELL, LINKSHELL2, FRIEND, a set or sorted category: the end.
            let Some((t, rest)) = find_word(&words.categories, &s[1..]) else { break };
            let t = t as u8;
            if matches!(t, 6 | 0xb | 0xc | 0x13 | 0x14) || c.flags & (1 << t) != 0 || c.sorted(t) {
                break;
            }
            c.sorts.push(if first == b'-' { 0x80 | t } else { t });
            s = skip_ws(rest);
            if let Some(r) = value(&mut c, t, false, s, words, ctx) {
                c.flags |= 1 << t;
                s = r;
            }
            continue;
        }
        if let Some((t, mut rest)) = find_word(&words.categories, s) {
            let t = t as u8;
            let unity = t == 6 && find_word(&words.unity_word, s).is_some();
            if t != 6 && c.flags & (1 << t) != 0 {
                break;
            }
            rest = skip_ws(rest);
            match t {
                0xc => c.flags |= FRIEND,
                0xb => {
                    c.ls[0] = ctx.ls[0];
                    c.flags |= LS1;
                }
                0x13 => {
                    c.ls[1] = ctx.ls[1];
                    c.flags |= LS2;
                }
                // The region's searchable zones, when no area is set.
                0x12 => {
                    if c.flags & AREA == 0
                        && let Some(r) = ctx.region.as_ref().filter(|r| !r.is_empty())
                    {
                        c.areas = r.iter().copied().take(16).collect();
                        c.last_area = *c.areas.last().unwrap() as i16;
                        c.flags |= AREA;
                    }
                }
                _ => match value(&mut c, t, unity, rest, words, ctx) {
                    Some(r) => {
                        c.flags |= 1 << t;
                        rest = r;
                    }
                    None => break,
                },
            }
            s = rest;
            continue;
        }
        if let Some(r) = value(&mut c, 6, false, s, words, ctx) {
            c.flags |= STATUS;
            s = r;
            continue;
        }
        if c.flags & AREA == 0
            && !c.sorted(1)
            && !first.is_ascii_digit()
            && let Some(r) = value(&mut c, 1, false, s, words, ctx)
        {
            c.flags |= AREA;
            s = r;
            continue;
        }
        let mut next = None;
        for (flag, t) in [(LEVEL, 4), (MLEVEL, 7), (JOB, 3), (NATION, 2), (RACE, 5), (NAME, 0)] {
            if c.flags & flag == 0
                && !c.sorted(t)
                && let Some(r) = value(&mut c, t, false, s, words, ctx)
            {
                c.flags |= flag;
                next = Some(r);
                break;
            }
        }
        match next {
            Some(r) => s = r,
            None => break,
        }
    }
    if c.flags & AREA == 0 {
        (c.areas, c.last_area, c.area_given) = (vec![ctx.zone], ctx.zone as i16, true);
        c.flags |= AREA;
    } else if c.last_area > 0 {
        c.area_given = true;
    }
    c
}

/// FUN_1020a9d0 mode 1: the query, and the window open on " [Searching]"; W+0x5c keeps the condition. A request-list
/// condition takes the history's names at every send (FUN_100e29c0), refines too.
pub fn run(w: &mut World, mut cond: Cond, scope: Scope) {
    if cond.flags & search::REQUEST != 0 {
        cond.requests = super::partyreq::names(w);
    }
    log::info!("search: {cond:?}");
    w.search_cond = cond.clone();
    if w.search_status0 == 0 {
        w.search_status0 = cond.status;
    }
    let rx = search::spawn(w.search_addr, cond);
    w.searches.push((rx, Then::Results));
    w.search = Some(Results { busy: true, failed: false, scope, rows: Vec::new(), total: 0, cursor: 0, scroll: 0, alt: false, option: None, title: 0, mode: 1 });
}

/// FUN_1020c390 (View Request, /recruitlist): once s2c 0x119 came, the request list in every area (mode 2, title
/// "Search"), replacing open results; nothing while a query is out.
pub fn run_requests(w: &mut World) {
    if !w.recasts_seen || busy(w) {
        return;
    }
    run(w, Cond { flags: search::REQUEST, last_area: -1, ..Default::default() }, Scope::World);
    if let Some(r) = w.search.as_mut() {
        r.mode = 2;
    }
}

/// scoption+0x20: the request list's rows 4 / 5 (decided by the condition, so a refine keeps them).
fn request_mode(w: &World, res: &Results) -> bool {
    res.mode != 3 && w.search_cond.flags & search::REQUEST != 0
}

/// FUN_1020a9d0 mode 3 (FUN_100e2f10): the group list of the ids (party, alliance, LS1, LS2) in the results window,
/// title slot 2 from group 0xd; nothing while a query is out.
pub fn run_group(w: &mut World, ids: [u32; 4], title: usize) {
    if busy(w) {
        return;
    }
    log::info!("search: group list {ids:?}");
    let cond = Cond::group(ids);
    w.search_cond = cond.clone();
    let rx = search::spawn(w.search_addr, cond);
    w.searches.push((rx, Then::Results));
    w.search = Some(Results { busy: true, failed: false, scope: Scope::Area, rows: Vec::new(), total: 0, cursor: 0, scroll: 0, alt: false, option: None, title, mode: 3 });
}

/// The same with p6 = 1: the old results window closes (FUN_1020b3a0), none opens; only the summary line comes.
pub fn run_quiet(w: &mut World, cond: Cond, scope: Scope) {
    log::info!("search (summary only): {cond:?}");
    w.search_cond = cond.clone();
    if w.search_status0 == 0 {
        w.search_status0 = cond.status;
    }
    w.search = None;
    let rx = search::spawn(w.search_addr, cond);
    w.searches.push((rx, Then::Summary(scope)));
}

/// DAT_10385a04 != 4: a query for the results window or a summary is out.
pub fn busy(w: &World) -> bool {
    w.searches.iter().any(|s| matches!(s.1, Then::Results | Then::Summary(_) | Then::LinkList))
}

/// The summary scope from the areas searched: one area, several, none (all areas).
pub fn scope_of(c: &Cond) -> Scope {
    match c.areas.len() {
        0 => Scope::World,
        1 => Scope::Area,
        _ => Scope::Region,
    }
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
        // 3.5: an exact name; a request checks the blacklist first (no self check here, LSB refuses it).
        Then::Request(name, kind) => {
            let Some(id) = reply.and_then(|r| r.records.iter().find(|r| r.name == name).map(|r| r.id)) else { return };
            match if kind == 0 { super::partyreq::blacklisted(w, &name) } else { 0 } {
                1 => super::textcmd::refuse(w, 0xc5),
                2 => super::textcmd::refuse(w, 0xc6),
                _ => super::partyreq::send(w, id, 0, kind, &name),
            }
        }
        // An exact name: c2s 0x03D; none: group 9 0x75 / 0x77.
        Then::Black(name, mode) => match reply.and_then(|r| r.records.iter().find(|r| r.name == name).map(|r| r.id)) {
            Some(id) => super::partyreq::black_edit(w, id, &name, mode),
            None => super::textcmd::log9(w, if mode == 0 { 0x75 } else { 0x77 }),
        },
        Then::LevelSync => super::levelsync::filled(w, reply),
        Then::LinkList => super::linklist::filled(w, reply),
        Then::Summary(scope) => {
            if let Some(r) = reply {
                summary(w, scope, r.total as usize, r.records.len());
            }
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
            // Retail shows the cursor on the first person once rows are in (seen in the user's retail shot).
            if res.cursor == 0 && !res.rows.is_empty() {
                res.cursor = 1;
            }
            let (scope, shown, total, mode) = (res.scope, res.rows.len(), reply.total as usize, res.mode);
            if mode != 3 {
                summary(w, scope, total, shown);
            }
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
                        match c {
                            // Clear History: the prompt over scoption, which stays open.
                            4 if request_mode(w, &res) => w.confirm = Some((super::Confirm::Request(super::partyreq::Ask::Clear), false)),
                            // Delete Request: delete (9 0x141), everything closes (FUN_1020c010), View Request again.
                            5 if request_mode(w, &res) => {
                                super::partyreq::delete_named(w, &r.name);
                                w.search_status0 = 0;
                                run_requests(w);
                                return;
                            }
                            _ => {
                                res.option = None;
                                option(w, c, &r);
                            }
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
        // FUN_1020c250: a person opens scoption (the comment fetch for flag 0x10 is not done); the header (or no row)
        // opens scsibori, no busy check (its rows check).
        K::Enter if res.cursor == 0 && res.mode == 3 => {}
        K::Enter if res.cursor == 0 => {
            super::searchmenu::open_refine(w);
            w.sounds.push(Ui::Select as u32);
        }
        K::Enter if !res.busy => {
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

/// FUN_1020c540's disabled rows: 2 Befriend (the Friend List is PlayOnline's; retail dims it on ourselves), 3 when the
/// person is in our party, a party member but not its leader, recruiting, or on MON. 4 / 5 never.
pub fn option_enabled(w: &World, c: u16, r: &Record) -> bool {
    match c {
        2 => false,
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
            (w.chat_input, w.chat_caret) = (format!("/tell {} ", r.name), None);
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
pub(super) const GENRE: [usize; 10] = [492, 495, 493, 496, 494, 499, 498, 71, 70, 70];
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
pub(super) fn icon(k: usize) -> Option<(&'static str, usize)> {
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
pub(super) fn job_cell(gu: &mut super::game_ui::GameUi, r: &Record, x: f32, y: f32, c: u32, alt: bool) {
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
    // The bar and hand only while it has the focus (scoption, scsibori or arealist over it take it).
    let focused = res.option.is_none() && !super::searchmenu::refining(w) && w.arealist.is_none();
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
        if f & 0x1000_8021 != 0 || res.mode == 2 {
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
    // The scroll bar past 15 rows at x1 - 8.
    gu.scroll_bar(wx + 358.0, wy, 248.0, count, ROWS, res.scroll);
    if focused {
        gu.bar_site(super::scrollbar::Owner::Search, wx + 358.0, wy, 248.0, count, ROWS, res.scroll);
    }
    if let Some(c) = res.option {
        let r = res.cursor.checked_sub(1).and_then(|i| res.rows.get(i));
        let off: Vec<u16> = OPTION_ORDER.iter().copied().filter(|&o| r.is_some_and(|r| !option_enabled(w, o, r))).collect();
        // FUN_1020c7a0: Clear History / Delete Request in the request list.
        let swaps: &[(u16, usize)] = if request_mode(w, res) { &[(4, 785), (5, 786)] } else { &[] };
        if let Some(win) = gu.lobby_window_swaps(SCOPTION, sw, sh, (0.0, 0.0), Some(c), !off.contains(&c), &off, swaps) {
            gu.disabled_children(SCOPTION, win.frame[0], win.frame[1], &off, Some(c));
            rects.push(win.frame);
        }
    }
    rects
}

/// Title slot 2 "Search" (group 0xd 0) or the opener's; help slot 2 the row's text: the header 0xe 0x13, a mentor 0xe 0x259; scoption's
/// rows their own.
pub fn bars(gu: &super::game_ui::GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(res) = &w.search else { return };
    bars.title[2] = gu.title_msg(res.title).unwrap_or_default();
    if let Some(c) = res.option {
        let [mut help, title] = gu.focus_text(SCOPTION, c);
        if request_mode(w, res) && matches!(c, 4 | 5) {
            help = Some(gu.menu_msg(if c == 4 { 0x3bc } else { 0x3bd }, ""));
        }
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        return;
    }
    bars.help[2] = match res.cursor {
        0 if res.mode == 3 => String::new(),
        0 => gu.menu_msg(0x13, ""),
        i => res.rows.get(i - 1).filter(|r| status_icon(r.flags).0 == 8).map(|_| gu.menu_msg(0x259, "")).unwrap_or_default(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::search::{AREA, JOB, LEVEL, LS1, MLEVEL, NAME, RACE, STATUS, condition};

    fn words() -> Option<SearchWords> {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).ok()?;
        crate::dat::tables::search_words(&dll).ok()
    }

    /// search_spec.md 2.2 examples, checked against FUN_10104f00 / FUN_10105640.
    #[test]
    fn grammar() {
        let Some(w) = words() else { return };
        let known = [0xffu8; 0x30];
        let ctx = Ctx { zone: 230, region: w.region_of(230), ls: [7, 8], unity: NO_UNITY, known: Some(&known) };
        let p = |t: &str| parse(t, &w, &ctx);
        assert_eq!(p(""), Cond::zone(230));
        assert_eq!(p("ALL"), Cond::all());
        let c = p("all war 70-75");
        assert_eq!((c.flags, c.job, c.level, c.areas.len(), c.area_given), (AREA | JOB | LEVEL, 1, (70, 75), 0, false));
        assert_eq!(condition(&c), (0, vec![7, 0x00, 0x45, 0x19, 0x2c, 0x68, 0x42, 0x00]));
        let c = p("\"Lower Jeuno\" inv");
        assert_eq!((c.areas.clone(), c.status, c.area_given), (vec![245], 0x8020, true));
        assert_eq!(p("name Lnx").name, "Lnx");
        let c = p("all -level 1-75 elv");
        assert_eq!((c.sorts.clone(), c.level, c.race, c.flags & (LEVEL | RACE)), (vec![0x84], (1, 75), 1, LEVEL | RACE));
        // Level and master level share their fields.
        let c = p("75 30-40");
        assert_eq!((c.flags & (LEVEL | MLEVEL), c.level), (LEVEL | MLEVEL, (30, 40)));
        assert_eq!(p("all unity ayame").status, 0x80_0000);
        assert_eq!(p("all unity").status, NO_UNITY);
        let c = p("all linkshell");
        assert_eq!((c.flags & LS1, c.ls[0]), (LS1, 7));
        // A word nothing takes ends the parse; the rest is not read.
        let c = p("all war xyz bst 10");
        assert_eq!((c.job, c.name.as_str(), c.flags & LEVEL), (1, "xyz", 0));
        assert_eq!(p("region").areas, w.region_of(230).unwrap());
        // Zones not visited (no s2c 0x008 bit) are no area: the word becomes the name.
        let none = Ctx { known: None, ..ctx };
        let c = parse("\"Lower Jeuno\"", &w, &none);
        assert_eq!((c.name.as_str(), c.areas.clone(), c.flags & (NAME | STATUS)), ("Lower Jeuno", vec![230], NAME));
    }
}
