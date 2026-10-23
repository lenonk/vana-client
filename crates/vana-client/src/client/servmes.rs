//! The server message (/servmes, /smes; asked by itself once per lobby login, on the first zone once no event runs:
//! FUN_100955d0). A transfer (vtable 0x1032d6d4, FUN_100a3040) asks c2s 0x04B for the text in pieces and builds it
//! from s2c 0x04D (LAB_100a2f70 -> FUN_100a2dd0); done, FUN_100a3160 prints it in mode 200.

use super::World;
use super::net::NetCommand;
use std::time::Instant;

/// The 0x400-byte buffer (FUN_100a3040); a longer message fails.
const BUFFER: usize = 0x400;
/// One piece (LSB sends at most 236 bytes).
const PIECE: i32 = 0xec;
/// Pieces asked at once after the first reply (FUN_100a2b20).
const SLOTS: usize = 4;
/// Kind 1 the server message; language 2 English (FUN_100a30e0).
const KIND: u8 = 1;
const LANG: u8 = 2;

#[derive(Clone, Copy, Debug)]
struct Slot {
    offset: i32,
    sent: Instant,
    tries: u8,
}

#[derive(Debug)]
pub struct Transfer {
    timestamp: i32,
    total: i32,
    /// Bytes in order so far and the next offset to ask.
    done: i32,
    asked: i32,
    slots: Vec<Slot>,
    buf: Vec<u8>,
    /// The last reply (or the start); 450 ticks (7.5 s) without one starts over, twice at most.
    heard: Instant,
    starts: u8,
}

impl Transfer {
    fn new() -> Self {
        Transfer { timestamp: 0, total: 0, done: 0, asked: 0, slots: Vec::new(), buf: vec![0; BUFFER], heard: Instant::now(), starts: 0 }
    }
}

/// c2s 0x04B (FUN_100a2aa0): Command (1 the first piece, 2 the rest), Result 0, kind, language, timestamp, size_total,
/// offset, data_size.
fn ask(w: &World, cmd: u8, t: &Transfer, offset: i32) {
    let mut b = vec![cmd, 0, KIND, LANG];
    let len = if t.total > 0 { PIECE.min(t.total - offset) } else { PIECE };
    for v in [t.timestamp, t.total, offset, len] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x04B, body: b });
}

/// FUN_100a3040 (/servmes): starts a transfer unless one is running.
pub(super) fn start(w: &mut World) {
    if w.servmes.is_some() {
        return;
    }
    let mut t = Transfer::new();
    t.starts = 1;
    ask(w, 1, &t, 0);
    t.slots.push(Slot { offset: 0, sent: Instant::now(), tries: 1 });
    w.servmes = Some(t);
}

/// Each frame: the auto request, a piece unanswered over 3 s asked once more, 7.5 s without a reply starting over
/// (twice at most, then it fails silently).
pub(super) fn frame(w: &mut World) {
    // Ours: also once the message tables are in, which print it.
    if w.servmes_armed && w.battle.is_some() && super::event::zone_ready(w) && w.event.is_none() && w.event_pending.is_none() {
        w.servmes_armed = false;
        start(w);
    }
    let Some(mut t) = w.servmes.take() else { return };
    if t.heard.elapsed().as_secs_f32() > 450.0 / 60.0 {
        if t.starts >= 2 {
            log::info!("server message: no reply");
            return;
        }
        let starts = t.starts + 1;
        t = Transfer::new();
        t.starts = starts;
        ask(w, 1, &t, 0);
        t.slots.push(Slot { offset: 0, sent: Instant::now(), tries: 1 });
    }
    for i in 0..t.slots.len() {
        let s = t.slots[i];
        if s.tries < 2 && s.sent.elapsed().as_secs() > 3 {
            ask(w, if s.offset == 0 { 1 } else { 2 }, &t, s.offset);
            t.slots[i] = Slot { sent: Instant::now(), tries: 2, ..s };
        }
    }
    w.servmes = Some(t);
}

/// s2c 0x04D (FUN_100a2dd0): Command @4 (1 / 2), Result @5, kind @6, language @7, timestamp @8, size_total @0xc,
/// offset @0x10, data_size @0x14, data @0x18. Any check failing ends the transfer silently.
pub(super) fn on_fragment(w: &mut World, p: &[u8]) {
    let Some(mut t) = w.servmes.take() else { return };
    let u8_at = |o: usize| p.get(o).copied().unwrap_or(0);
    let i32_at = |o: usize| p.get(o..o + 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
    let (cmd, kind, lang) = (u8_at(4), u8_at(6), u8_at(7));
    let (stamp, total, offset, size) = (i32_at(8), i32_at(0xc), i32_at(0x10), i32_at(0x14));
    if !matches!(cmd, 1 | 2) || kind != KIND || lang != LANG || stamp <= 0 || total <= 0 || total as usize > BUFFER || size < 0 {
        log::info!("server message: piece refused (cmd {cmd}, kind {kind}, total {total})");
        return;
    }
    if t.done > 0 && (stamp != t.timestamp || total != t.total) {
        return;
    }
    let Some(k) = t.slots.iter().position(|s| s.offset == offset) else { return };
    if offset != t.done || (offset + size) as usize > BUFFER {
        return;
    }
    t.slots.remove(k);
    (t.timestamp, t.total) = (stamp, total);
    let data = p.get(0x18..0x18 + size as usize).unwrap_or_default();
    t.buf[offset as usize..offset as usize + data.len()].copy_from_slice(data);
    t.done = offset + data.len() as i32;
    t.asked = t.asked.max(t.done);
    t.heard = Instant::now();
    if t.done >= t.total {
        show(w, &t.buf[..t.total as usize]);
        return;
    }
    while t.slots.len() < SLOTS && t.asked < t.total {
        let o = t.asked;
        ask(w, 2, &t, o);
        t.slots.push(Slot { offset: o, sent: Instant::now(), tries: 1 });
        t.asked += PIECE.min(t.total - o);
    }
    w.servmes = Some(t);
}

/// FUN_100a3160: "<<< Welcome to %s! >>>" (group 0xa 0x22, the world's name), then FUN_100a3240's segments (each
/// ending after a BEL, CR or LF), all in mode 200. Not done: auto-translate phrases (FUN_10127c00 param 5).
fn show(w: &mut World, text: &[u8]) {
    let Some(bt) = w.battle.clone() else { return };
    let colour = bt.colour_in(200, &w.game);
    let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
    w.chat.push_colour(format!("<<< Welcome to {}! >>>", w.world_name), colour);
    for seg in text.split_inclusive(|&b| matches!(b, 7 | b'\r' | b'\n')) {
        let line = crate::dat::font::decode_sjis(seg.strip_suffix(&[7]).or(seg.strip_suffix(b"\r")).or(seg.strip_suffix(b"\n")).unwrap_or(seg));
        if !line.is_empty() {
            w.chat.push_colour(line, colour);
        }
    }
}
