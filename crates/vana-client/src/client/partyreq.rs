//! Party join requests (party_request_spec.md): s2c 0x11D into the request history (DAT_10662690, USER pec.dat), the
//! blacklist it is checked against (s2c 0x041), and the history's users: View Request, /recruitlist, the Request
//! toggle's reset prompt.

use super::World;
use super::net::NetCommand;
use std::path::Path;
use std::time::{Duration, Instant};

/// FUN_1018dcd0's 64 records.
const MAX: usize = 64;
/// FUN_1018df70: records older than 30 days are not listed.
const AGED: u64 = 0x278d00;

#[derive(Default)]
pub struct State {
    /// The history, newest first: (name, game clock at the last request).
    pub recs: Vec<(String, u32)>,
    pub loaded: bool,
    /// The blacklist from s2c 0x041 and S+0x1a698 (its last packet seen).
    pub blacklist: Vec<(u32, String)>,
    pub black_loaded: bool,
    /// FUN_100e0aa0: the last c2s 0x03C.
    black_asked: Option<Instant>,
    /// The mode of the c2s 0x03D waiting for its answer (0 add, 1 delete).
    black_pending: Option<u8>,
}

/// comyn group 9 0x140 "Really reset request history?" (default No) from the Request toggle or scoption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// LAB_10204f40: Request on; either answer turns it on.
    Reset,
    /// LAB_1020cc00: Clear History.
    Clear,
}

/// FUN_1018dd40: 1 new, 2 known and last asked >= 180 s ago, 0 known and recent; the name goes to the top.
fn add(s: &mut State, name: &str, now: u32) -> u8 {
    let r = match s.recs.iter().position(|r| r.0 == name) {
        Some(i) => {
            let old = s.recs.remove(i);
            if now.wrapping_sub(old.1) > 0xb3 { 2 } else { 0 }
        }
        None => {
            s.recs.truncate(MAX - 1);
            1
        }
    };
    s.recs.insert(0, (name.to_string(), now));
    r
}

/// FUN_1018de60: the later names move up one, the times stay where they are (the record below takes the deleted
/// one's time); true when it was there.
fn delete(s: &mut State, name: &str) -> bool {
    let Some(i) = s.recs.iter().position(|r| r.0 == name) else { return false };
    for k in i..s.recs.len() - 1 {
        s.recs[k].0 = std::mem::take(&mut s.recs[k + 1].0);
    }
    s.recs.pop();
    true
}

/// FUN_100e3130 over FUN_100da5c0: the names for the search, newest first, none older than 30 days.
pub fn names(w: &World) -> Vec<String> {
    let now = super::vana_now(w);
    w.partyreq.recs.iter().filter(|r| now.saturating_sub(r.1 as u64) <= AGED).map(|r| r.0.clone()).collect()
}

/// FUN_1020c3e0 (FUN_1018df40): the history cleared, group 9 0x142.
pub fn clear(w: &mut World) {
    w.partyreq.recs.clear();
    super::textcmd::log9(w, 0x142);
}

/// FUN_1020c410: delete and group 9 0x141, found or not.
pub fn delete_named(w: &mut World, name: &str) {
    delete(&mut w.partyreq, name);
    super::textcmd::log9(w, 0x141);
}

/// FUN_100e09a0: the leading letters, 16 at most.
fn letters(s: &str) -> &str {
    let n = s.bytes().take(16).take_while(u8::is_ascii_alphabetic).count();
    &s[..n]
}

/// FUN_100e0a50: 1 on the blacklist, 2 not loaded yet (asking for it, c2s 0x03C at most every 60 s), else 0.
pub fn blacklisted(w: &mut World, name: &str) -> u8 {
    let s = &mut w.partyreq;
    if !s.black_loaded {
        if s.black_asked.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
            s.black_asked = Some(Instant::now());
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x03C, body: vec![0; 0x18] });
        }
        return 2;
    }
    s.blacklist.iter().any(|(_, n)| letters(n) == letters(name)) as u8
}

/// FUN_100a1a00: a message from `id` is dropped while its sender (not ourselves; FUN_100a1770's name: party and
/// alliance first, then the actors, else empty) is on the blacklist or the list is not in yet. Kind 0 (s2c 0x009 from
/// a PC) only for messages 0x59 / 0x5a / 0x64..0x67; kinds 1 (0x029 / 0x02D) and 2 (0x05A emotes) always.
pub fn sender_blocked(w: &mut World, id: u32, msg: u16, kind: u8) -> bool {
    if kind == 0 && !matches!(msg, 0x59 | 0x5a | 0x64..=0x67) {
        return false;
    }
    if id == w.my_id {
        return false;
    }
    let name = w.party.members().find(|m| m.id == id).map(|m| m.name.clone()).or_else(|| w.entities.get(&id).map(|e| e.name.clone())).unwrap_or_default();
    if !name.is_empty() && name == w.name {
        return false;
    }
    blacklisted(w, &name) != 0
}

/// FUN_100e0bc0: c2s 0x03D {ID u32, Name[16], Mode i8} for /blacklist add (0) / delete (1).
pub fn black_edit(w: &mut World, id: u32, name: &str, mode: u8) {
    let mut b = vec![0u8; 0x18];
    b[0..4].copy_from_slice(&id.to_le_bytes());
    for (k, c) in name.bytes().take(15).enumerate() {
        b[4 + k] = c;
    }
    b[0x14] = mode;
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x03D, body: b });
    w.partyreq.black_pending = Some(mode);
}

/// s2c 0x042: the list gains or loses the name (9 0x74 "Added to blacklist." / 0x76 "Removed from blacklist."), or the
/// edit failed (0x75 / 0x77 by the edit asked) *(inf: the list update; LSB sends only this, not a new 0x041)*.
pub fn black_result(w: &mut World, id: u32, name: String, mode: u8) {
    let pending = w.partyreq.black_pending.take();
    let s = &mut w.partyreq;
    let line = match mode {
        0 => {
            if !s.blacklist.iter().any(|(_, n)| letters(n) == letters(&name)) {
                s.blacklist.push((id, name));
            }
            0x74
        }
        1 => {
            s.blacklist.retain(|(_, n)| letters(n) != letters(&name));
            0x76
        }
        _ if pending == Some(1) => 0x77,
        _ => 0x75,
    };
    super::textcmd::log9(w, line);
}

/// FUN_100e0620: s2c 0x041; Stat bit 0 starts the list over, bit 1 is its last packet.
pub fn blacklist(w: &mut World, stat: u8, entries: Vec<(u32, String)>) {
    let s = &mut w.partyreq;
    if stat & 1 != 0 {
        s.blacklist.clear();
    }
    s.blacklist.extend(entries);
    if stat & 2 != 0 {
        s.black_loaded = true;
    }
    log::info!("blacklist: {} names (stat {stat:#x})", s.blacklist.len());
}

/// A group 10 line with the name, in mode 0x87 (the 7F name wrapper of 0x76 / 0x77 is not drawn by our log yet).
pub(super) fn line(w: &mut World, id: usize, name: &str) {
    let Some(bt) = w.battle.clone() else { return };
    let Some(raw) = bt.group10_raw(id) else { return };
    let sjis = crate::dat::font::encode_sjis(name);
    let text = crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&sjis], &[], &|_, _, _| None));
    let colour = bt.colour_in(0x87, &w.game);
    w.chat.push_colour(text, colour);
}

/// FUN_100e9590: c2s 0x11C {UniqueNo, ActIndex, Kind (0 request, 1 cancel), 0}; a request prints group 10 0x76.
pub fn send(w: &mut World, id: u32, index: u16, kind: u8, name: &str) {
    let mut body = vec![0u8; 8];
    body[..4].copy_from_slice(&id.to_le_bytes());
    body[4..6].copy_from_slice(&index.to_le_bytes());
    body[6] = kind;
    log::info!("party request: kind {kind} to {name} ({id:#x})");
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x11C, body });
    if kind == 0 {
        line(w, 0x76, name);
    }
}

/// LAB_100a2610: status 0 a request (dropped while blacklisted or the list is not in; a line unless repeated within
/// 180 s), 1 stopped (a line when it was in the history). No sound, no window; an open list is not refreshed.
pub fn received(w: &mut World, status: u8, name: String) {
    log::info!("party request: {name} status {status}");
    match status {
        0 => {
            if blacklisted(w, &name) != 0 {
                return;
            }
            let now = super::vana_now(w) as u32;
            if add(&mut w.partyreq, &name, now) != 0 {
                line(w, 0x77, &name);
            }
        }
        1 => {
            if delete(&mut w.partyreq, &name) {
                line(w, 0x78, &name);
            }
        }
        _ => {}
    }
}

/// LAB_1018dfd0: pec.dat (version 2, 64 records of name[16], u32 time); unreadable: empty. Any name with a character
/// that is not a letter wipes all of it.
pub fn load(w: &mut World, dir: &Path) {
    let s = &mut w.partyreq;
    (s.loaded, s.recs) = (true, Vec::new());
    let Some(b) = super::macros::read_file(&dir.join("pec.dat"), 2).filter(|b| b.len() >= MAX * 0x14) else { return };
    let mut recs = Vec::new();
    for r in b.chunks(0x14).take(MAX) {
        let n = r[..15].iter().position(|&c| c == 0).unwrap_or(15);
        if !r[..n].iter().all(u8::is_ascii_alphabetic) {
            log::warn!("pec.dat: a bad name, the history is cleared");
            return;
        }
        if n == 0 {
            break;
        }
        recs.push((String::from_utf8_lossy(&r[..n]).into_owned(), u32::from_le_bytes([r[0x10], r[0x11], r[0x12], r[0x13]])));
    }
    s.recs = recs;
}

/// FUN_101b9ff0 for pec.dat.
pub fn save(w: &World, dir: &Path) {
    if !w.partyreq.loaded {
        return;
    }
    let mut b = vec![0u8; MAX * 0x14];
    for (r, (name, time)) in b.chunks_mut(0x14).zip(&w.partyreq.recs) {
        for (d, c) in r.iter_mut().zip(name.bytes().take(15)) {
            *d = c;
        }
        r[0x10..].copy_from_slice(&time.to_le_bytes());
    }
    super::macros::write_file(&dir.join("pec.dat"), 2, &b);
}

pub fn ask_text(w: &World) -> String {
    w.battle.as_ref().and_then(|b| b.group9(0x140)).unwrap_or_default()
}

/// LAB_10204f40: Yes clears, then Request turns on either way. LAB_1020cc00: Yes clears and every search window
/// closes (FUN_1020c010); No goes back to scoption.
pub fn answer(w: &mut World, a: Ask, yes: bool) {
    if yes {
        clear(w);
    }
    match a {
        Ask::Reset => super::textcmd::config_flag(w, 0xb, true),
        Ask::Clear if yes => {
            w.search = None;
            w.search_status0 = 0;
        }
        Ask::Clear => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history() {
        let mut s = State::default();
        assert_eq!(add(&mut s, "Ab", 1000), 1);
        assert_eq!(add(&mut s, "Cd", 1010), 1);
        // Within 180 s: silent, but to the top with the new time.
        assert_eq!(add(&mut s, "Ab", 1100), 0);
        assert_eq!(s.recs, vec![("Ab".into(), 1100), ("Cd".into(), 1010)]);
        assert_eq!(add(&mut s, "Cd", 1010 + 0xb4), 2);
        // The delete moves names only: Ab takes the deleted record's time.
        assert!(delete(&mut s, "Cd"));
        assert_eq!(s.recs, vec![("Ab".into(), 1010 + 0xb4)]);
        assert!(!delete(&mut s, "Cd"));
        for k in 0..70 {
            add(&mut s, &format!("N{}", (b'a' + k as u8 % 26) as char).repeat(1 + k / 26), k as u32);
        }
        assert_eq!(s.recs.len(), MAX);
        assert_eq!((letters("Abc1x"), letters("Lnxtest")), ("Abc", "Lnxtest"));
    }
}
