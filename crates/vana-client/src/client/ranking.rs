//! The Fishing Ranking (op 0xb3, FUN_100bbaf0; Selbina and Chenon's contest NPCs). A request (DAT_10480aa8) starts
//! a kind 2 transfer (FUN_100a3310: the c2s 0x04B / s2c 0x04D transfer of servmes.rs, one at a time): command 1
//! fetches the ranking table in 0xb4-byte pieces (five 36-byte entries) after a header and our own entry, 10..13 send
//! one bodiless request whose result byte is the answer. The script reads the result and our entry, then opens
//! "menu    ranking" (DAT_10664444, FUN_10209b90) on the table.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use std::time::Instant;

pub const WINDOW: &str = "menu    ranking";
/// The request's buffer (DAT_10487038): 100 entries.
const BUFFER: usize = 0xe10;
const ENTRY: usize = 36;
/// The kind 2 piece (FUN_100a28e0) and the most a slot asks (FUN_100a2b70).
const PIECE: i32 = 0xb4;
const MAX_PIECE: i32 = 0xec;
const SLOTS: usize = 4;
const KIND: u8 = 2;
/// FUN_100a2790: 450 ticks without a reply starts over (+0x3c), a sent piece unanswered for 3 s is asked again
/// (+0x44).
const TIMEOUT: f32 = 450.0;
const RESEND_SECS: u64 = 3;
/// FUN_101f6700(0xf): 15 rows 16 apart.
const ROWS: usize = 15;
const PITCH: f32 = 16.0;
const WHITE: u32 = 0x8080_8080;

/// The request (DAT_10480aa8, FUN_100a2fd0): command, result (0 pending, 1 done, negative failed or the server's
/// refusal), the value (the ranking's type, 1..7), the table's timestamp, size and bytes in order, the header word
/// (+0x18, the entry count) and our own entry (+0x1c).
#[derive(Clone, Debug)]
pub struct Request {
    cmd: u8,
    result: i8,
    val: u8,
    total: i32,
    done: i32,
    header: u32,
    own: [u8; ENTRY],
    buf: Vec<u8>,
}

impl Default for Request {
    fn default() -> Self {
        Request { cmd: 0, result: 0, val: 0, total: 0, done: 0, header: 0, own: [0; ENTRY], buf: vec![0; BUFFER] }
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    cmd: u8,
    offset: i32,
    size: i32,
    stamp: i32,
    total: i32,
    tries: u8,
    sent: Option<Instant>,
    answered: bool,
}

/// The transfer (vtable 0x1032d6d4): state (+0x58: the command, 2 for the later pieces), the reply's timestamp and
/// size (+0x5c / +0x60), bytes in order (+0x68) and asked (+0x6c), four slots (+0x70).
#[derive(Debug)]
pub struct Transfer {
    state: u8,
    stamp: i32,
    total: i32,
    piece: i32,
    done: i32,
    asked: i32,
    slots: [Option<Slot>; SLOTS],
    heard: Instant,
    /// Ticks added when a piece ran out of tries (+0x34 += +0x3c).
    late: f32,
    starts: u8,
}

/// The window (FUN_1020a2a0): its type (1..7) and the list's cursor.
#[derive(Clone, Debug)]
pub struct Win {
    kind: u8,
    top: usize,
    cursor: usize,
}

#[derive(Debug, Default)]
pub struct State {
    req: Request,
    xfer: Option<Transfer>,
    win: Option<Win>,
}

/// A transfer is running (DAT_10487f08): the server message waits for it.
pub fn busy(w: &World) -> bool {
    w.ranking.xfer.is_some()
}

impl Transfer {
    fn new() -> Self {
        Transfer { state: 0, stamp: 0, total: 0, piece: 0, done: 0, asked: 0, slots: [None; SLOTS], heard: Instant::now(), late: 0.0, starts: 2 }
    }

    /// FUN_100a2b70: a slot for the next bytes; commands 1 and 2 get two tries, the others one.
    fn slot(&self, size: i32) -> Slot {
        let tries = if matches!(self.state, 1 | 2) { 2 } else { 1 };
        Slot { cmd: self.state, offset: self.asked, size: size.min(MAX_PIECE), stamp: self.stamp, total: self.total, tries, sent: None, answered: false }
    }

    /// FUN_100a28e0: command 1 asks the first piece (one start used), 10..13 one piece of 0 (no restart).
    fn init(&mut self, req: &Request) {
        let starts = self.starts;
        *self = Transfer::new();
        self.starts = starts;
        match req.cmd {
            1 => {
                self.state = 1;
                self.piece = PIECE;
                let s = self.slot(self.piece);
                self.asked += s.size;
                self.slots[0] = Some(s);
                self.starts = self.starts.saturating_sub(1);
            }
            10..=13 => {
                self.state = req.cmd;
                self.slots[0] = Some(self.slot(0));
                self.starts = 0;
            }
            _ => {}
        }
    }
}

/// FUN_100a3310: a value of 0 or over 7 is 0; only commands 1 and 10..13, and only while no transfer runs.
pub fn start(w: &mut World, cmd: u8, val: u8) {
    if w.servmes.is_none() {
        w.ranking.start(cmd, val);
    }
}

impl State {
    fn start(&mut self, cmd: u8, val: u8) {
        let val = if val == 0 || val > 7 { 0 } else { val };
        if !(cmd == 1 || (10..=13).contains(&cmd)) || self.xfer.is_some() {
            return;
        }
        self.req = Request { cmd, val, ..Request::default() };
        let mut t = Transfer::new();
        t.init(&self.req);
        self.xfer = Some(t);
    }
}

/// FUN_100a2aa0: c2s 0x04B {slot command, 0, kind 2, value, timestamp, size, offset, piece}; one try used.
fn send(out: &mut Vec<Vec<u8>>, val: u8, s: &mut Slot) {
    let mut b = vec![s.cmd, 0, KIND, val];
    for v in [s.stamp, s.total, s.offset, s.size] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    out.push(b);
    s.tries = s.tries.saturating_sub(1);
    s.sent = Some(Instant::now());
    s.answered = false;
}

/// FUN_100a29f0 then the transfer's end (FUN_100a2810): a result still 0 becomes -1.
fn finish(st: &mut State) {
    st.xfer = None;
    if st.req.result == 0 {
        st.req.result = -1;
    }
}

/// FUN_100a2840 each frame: over 450 ticks starts over while starts are left (else fails); state 2 fills the free
/// slots with the next pieces (FUN_100a2b20); then FUN_100a2a20 sends new slots (none left to try: dropped) and asks
/// a piece unanswered over 3 s again (none left: the 450 ticks run out).
pub(super) fn frame(w: &mut World) {
    for body in w.ranking.tick() {
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x04B, body });
    }
}

impl State {
    /// The c2s 0x04B bodies to send.
    fn tick(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let Some(mut t) = self.xfer.take() else { return out };
        if t.heard.elapsed().as_secs_f32() * 60.0 + t.late > TIMEOUT {
            if t.starts >= 1 {
                t.init(&self.req);
                self.xfer = Some(t);
            } else {
                log::info!("fishing ranking: no reply");
                finish(self);
            }
            return out;
        }
        if t.state == 2 {
            for k in 0..SLOTS {
                let left = t.total - t.asked;
                if left <= 0 {
                    break;
                }
                if t.slots[k].is_none() {
                    let s = t.slot(left.min(t.piece));
                    t.asked += s.size;
                    t.slots[k] = Some(s);
                }
            }
        }
        if matches!(t.state, 1 | 2 | 10..=13) {
            let val = self.req.val;
            for k in 0..SLOTS {
                let Some(mut s) = t.slots[k] else { continue };
                match s.sent {
                    None if s.tries < 1 => {
                        t.slots[k] = None;
                        continue;
                    }
                    None => send(&mut out, val, &mut s),
                    Some(at) if !s.answered && at.elapsed().as_secs() > RESEND_SECS => {
                        if s.tries < 1 {
                            t.late += TIMEOUT;
                        } else {
                            send(&mut out, val, &mut s);
                        }
                    }
                    _ => {}
                }
                t.slots[k] = Some(s);
            }
        }
        self.xfer = Some(t);
        out
    }
}

/// s2c 0x04D while a ranking transfer runs (LAB_100a2f70): commands 1 / 2 a piece of the table, 10..13 the answer.
pub(super) fn on_fragment(w: &mut World, p: &[u8]) {
    w.ranking.reply(p);
}

impl State {
    fn reply(&mut self, p: &[u8]) {
        let st = self;
        let Some(mut t) = st.xfer.take() else { return };
        let u8_at = |o: usize| p.get(o).copied().unwrap_or(0);
        let i32_at = |o: usize| p.get(o..o + 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
        let (cmd, result, kind, val) = (u8_at(4), u8_at(5) as i8, u8_at(6), u8_at(7));
        let (stamp, total, offset, size) = (i32_at(8), i32_at(0xc), i32_at(0x10), i32_at(0x14));
        let own =
            |p: &[u8]| -> Option<(u32, [u8; ENTRY])> { Some((u32::from_le_bytes(p.get(0x18..0x1c)?.try_into().ok()?), p.get(0x1c..0x40)?.try_into().ok()?)) };
        t.heard = Instant::now();
        t.late = 0.0;
        // FUN_100a2c30: a timestamp, a size, the state's command, kind and value.
        let head = stamp > 0 && total >= 0 && cmd == t.state && kind == KIND && val == st.req.val;
        // FUN_100a2be0: the slot of (command, offset) is answered and takes the reply's size.
        let answer = |t: &mut Transfer, offset: i32, size: i32| {
            let s = t.slots.iter_mut().flatten().find(|s| s.cmd == cmd && s.offset == offset)?;
            s.answered = true;
            s.size = size;
            Some(())
        };
        match cmd {
            // FUN_100a2d20: the result is the answer; with -3 / -2 / 1 the header and our entry come along.
            10..=13 => {
                st.req.result = result;
                if kind == KIND
                    && matches!(result, -3 | -2 | 1)
                    && let Some((h, e)) = own(p)
                {
                    (st.req.header, st.req.own) = (h, e);
                }
                if result >= 0 && head && answer(&mut t, 0, 0).is_some() {
                    st.xfer = None;
                    if st.req.result == 0 {
                        st.req.result = -1;
                    }
                    return;
                }
                finish(st);
            }
            // FUN_100a2dd0: a piece within what was asked and the buffer, after the bytes in order; command 1 sets the
            // timestamp and size and brings the header and our entry, 2 must match them. Data from +0x40.
            1 | 2 => {
                let fits = offset >= 0
                    && size >= 0
                    && t.done <= offset
                    && offset <= t.asked
                    && total <= BUFFER as i32
                    && offset < BUFFER as i32
                    && size + offset <= BUFFER as i32;
                if !head || !fits || answer(&mut t, offset, size).is_none() {
                    finish(st);
                    return;
                }
                if cmd == 1 {
                    (t.stamp, t.total) = (stamp, total);
                    (st.req.total, st.req.done) = (total, 0);
                    if let Some((h, e)) = own(p) {
                        (st.req.header, st.req.own) = (h, e);
                    }
                } else if stamp != t.stamp || total != t.total {
                    finish(st);
                    return;
                }
                if size > 0 {
                    let data = p.get(0x40..).unwrap_or_default();
                    let n = data.len().min(size as usize);
                    st.req.buf[offset as usize..offset as usize + n].copy_from_slice(&data[..n]);
                }
                // FUN_100a2f00: answered slots at the bytes in order move them on.
                while let Some(k) = t.slots.iter().position(|s| s.is_some_and(|s| s.sent.is_some() && s.answered && s.offset == t.done)) {
                    t.done += t.slots[k].map_or(0, |s| s.size);
                    t.slots[k] = None;
                }
                st.req.done = t.done;
                if t.total <= t.done {
                    // FUN_100a29b0.
                    st.req.result = 1;
                    return;
                }
                t.state = 2;
                st.xfer = Some(t);
            }
            _ => st.xfer = Some(t),
        }
    }
}

/// Op 0xb3 1 / 5 (FUN_100a34d0 / FUN_100a34e0 and the getters after them).
pub fn values(w: &World, full: bool) -> Option<[i32; 8]> {
    let r = &w.ranking.req;
    if r.result == 0 || full && r.cmd == 0 {
        return None;
    }
    let e = &r.own;
    // FUN_100a3410: the main job only for values 1..4, 6 and 7.
    let mjob = if matches!(r.val, 1..=4 | 6 | 7) { e[0x10] } else { 0 };
    let u32_at = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap()) as i32;
    Some([r.result as i32, r.header as u16 as i16 as i32, e[0x20] as i32, mjob as i32, e[0x23] as i32, e[0x22] as i32, u32_at(0x18), u32_at(0x1c)])
}

/// Op 0xb3 2 (FUN_1020a2a0 with FUN_100a3490): a kind 2 request with a value of 1..7 opens the window of that type;
/// else nothing opens.
pub fn open(w: &mut World) {
    let r = &w.ranking.req;
    if (1..=7).contains(&r.val) {
        w.ranking.win = Some(Win { kind: r.val, top: 0, cursor: 0 });
    }
}

/// Op 0xb3 8 (FUN_1020a330).
pub fn close(w: &mut World) {
    w.ranking.win = None;
}

/// Op 0xb3 9.
pub fn is_open(w: &World) -> bool {
    w.ranking.win.is_some()
}

/// One row of the table: the entry's fields the window shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rank: u8,
    pub name: String,
    pub job: (u8, u8),
    pub level: (u8, u8),
    pub race: u8,
    pub allegiance: u8,
    pub fish_rank: u8,
    /// +0x14 as a short (type 5's "[%d-%d-%d]" with the fishing rank and +0x16).
    pub word: i16,
    pub score: u32,
    pub time: u32,
}

/// FUN_100a3500 over the 100 entries: received in order, within the table and the buffer, with a contest rank.
pub fn rows(w: &World) -> Vec<Row> {
    w.ranking.req.rows()
}

impl Request {
    fn rows(&self) -> Vec<Row> {
        let r = self;
        (0..BUFFER / ENTRY)
            .map(|i| i * ENTRY)
            .filter(|&o| (o as i32) < r.done && (o as i32) < r.total && r.buf[o + 0x20] != 0)
            .map(|o| {
                let e = &r.buf[o..o + ENTRY];
                let name = &e[..16];
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(16)];
                let u32_at = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
                Row {
                    rank: e[0x20],
                    name: crate::dat::font::decode_sjis(name),
                    job: (e[0x10], e[0x11]),
                    level: (e[0x12], e[0x13]),
                    race: e[0x14],
                    allegiance: e[0x16],
                    fish_rank: e[0x17],
                    word: i16::from_le_bytes([e[0x14], e[0x15]]),
                    score: u32_at(0x18),
                    time: u32_at(0x1c),
                }
            })
            .collect()
    }
}

/// The window's type, cursor and rows (for the modern panel).
pub fn look(w: &World) -> Option<(u8, usize, Vec<Row>)> {
    let win = w.ranking.win.as_ref()?;
    Some((win.kind, win.cursor, rows(w)))
}

/// The modern panel's click.
pub fn set_cursor(w: &mut World, k: usize) {
    if let Some(win) = w.ranking.win.as_mut() {
        win.cursor = k;
        win.top = win.top.min(k).max((k + 1).saturating_sub(ROWS));
    }
}

/// The list base (FUN_101f6c70): Up / Down stop at the ends, Right / Left page by 15. Escape closes *(inf: the
/// generic window handler)*, which ends the script's op 0xb3 9 loop.
pub fn key(w: &mut World, key: egui::Key) {
    let n = rows(w).len();
    let Some(mut a) = w.ranking.win.clone() else { return };
    match key {
        egui::Key::ArrowDown if a.cursor + 1 < n => {
            a.cursor += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowUp if a.cursor > 0 => {
            a.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowRight | egui::Key::ArrowLeft => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if super::scrollbar::rows_abs(m, &mut a.top, &mut a.cursor, n, ROWS) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.ranking.win = None;
            return;
        }
        _ => {}
    }
    a.cursor = a.cursor.min(n.saturating_sub(1));
    a.top = a.top.min(a.cursor).max((a.cursor + 1).saturating_sub(ROWS));
    w.ranking.win = Some(a);
}

/// A cell (FUN_101f4bb0 by type).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cell {
    /// 7: the contest rank "%d" right-aligned (ranks over 10 dimmer).
    Rank,
    /// 0x11: the job / level cell (FUN_101ca6c0 over the entry's jobs, levels, race, allegiance and fishing rank).
    Job,
    /// 0x12: the fishing rank's icon, keytops3 0xc0 + rank + 1.
    Icon,
    /// 0x25: the name fitted to the column - 8.
    Name,
    /// 9 "[%d]", 8 "%d", 0x47 "%d P": the score right-aligned.
    Score(&'static str),
    /// 0x18: "[%d-%d-%d]" of +0x14 (a short), the fishing rank and +0x16.
    Triple,
    /// 6: the submit time as a Vana'diel date (FUN_101c92b0(t, 2, 1)).
    VanaDate,
    /// 5 / 0x48: the submit time moved to Earth time (t - the game clock + time()) as a long date (FUN_101c8fd0(t,
    /// 7)); 0x48 draws nothing before 0x3ca72470 (2002-04-01).
    EarthDate { from_2002: bool },
}

/// FUN_1020a2a0's columns and FUN_10209c40 (1..3), FUN_10209f80 (4), FUN_10209dd0 (5), FUN_1020a100 (6 / 7): the
/// cells by column.
pub fn layout(kind: u8) -> ([f32; 6], [Option<Cell>; 5]) {
    use Cell::*;
    match kind {
        1..=3 => ([0.0, 18.0, 160.0, 243.0, 275.0, 305.0], [Some(Rank), Some(Job), Some(Name), Some(Score("[%d]")), Some(VanaDate)]),
        4 => ([0.0, 18.0, 24.0, 120.0, 243.0, 305.0], [Some(Rank), None, Some(Icon), Some(Name), Some(Score("[%d]"))]),
        5 => ([0.0, 18.0, 185.0, 200.0, 265.0, 305.0], [Some(Rank), Some(Name), Some(Score("%d")), Some(Triple), Some(EarthDate { from_2002: false })]),
        _ => ([0.0, 24.0, 160.0, 250.0, 265.0, 305.0], [Some(Rank), Some(Name), Some(Score("%d P")), None, Some(EarthDate { from_2002: true })]),
    }
}

/// A text cell's string (None for the drawn cells and an Earth date before 2002).
pub fn text(w: &World, c: Cell, r: &Row) -> Option<String> {
    let dates = || {
        let bt = w.battle.as_ref()?;
        Some((bt, (w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0))
    };
    match c {
        Cell::Rank => Some(r.rank.to_string()),
        Cell::Name => Some(r.name.clone()),
        Cell::Score(f) => Some(f.replace("%d", &(r.score as i32).to_string())),
        Cell::Triple => Some(format!("[{}-{}-{}]", r.word, r.fish_rank, r.allegiance)),
        Cell::VanaDate => {
            let (bt, key, _) = dates()?;
            let g = |id: u16| bt.group10_raw(id as usize);
            Some(crate::dat::font::decode_sjis(&crate::dat::msgfmt::vana_datetime(&g, r.time as u64, 2, key)))
        }
        Cell::EarthDate { from_2002 } => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            let t = (r.time as i32).wrapping_sub(super::vana_now(w) as i32).wrapping_add(now as i32);
            if from_2002 && t < 0x3ca7_2470 {
                return None;
            }
            let (bt, key, h12) = dates()?;
            let g = |id: u16| bt.group10_raw(id as usize);
            Some(crate::dat::font::decode_sjis(&crate::dat::msgfmt::earth_datetime(&g, t as i64, 7, key, h12)))
        }
        Cell::Job | Cell::Icon => None,
    }
}

/// FUN_101f6a60 with the type's cells: rows from (+0, +4) 16 apart; the rank, scores and "[a-b-c]" right-aligned or
/// in the shape font at y + 4, the name fitted in the game font at y + 2, the icon and job cell at y + 2, dates in the
/// shape font at y + 4; the windowps 75 cursor bar while focused. No help or title text.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(a) = &w.ranking.win else { return rects };
    let rows = rows(w);
    let focused = w.confirm.is_none();
    let sel = (!rows.is_empty()).then(|| (a.cursor - a.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), sel, focused, &[]) else { return rects };
    rects.push(win.frame);
    let Some((wx, wy)) = gu.window_xy(WINDOW, sw, sh) else { return rects };
    let (cols, cells) = layout(a.kind);
    for (k, r) in rows.iter().enumerate().skip(a.top).take(ROWS) {
        let (rx, ry) = (wx, wy + 4.0 + PITCH * (k - a.top) as f32);
        for (i, c) in cells.iter().enumerate() {
            let Some(c) = *c else { continue };
            let (x, end) = (rx + cols[i], rx + cols[i + 1]);
            match c {
                Cell::Rank => {
                    let tint = if r.rank > 10 { 0x8070_7070 } else { WHITE };
                    gu.shape_text(&r.rank.to_string(), end - 2.0, ry + 4.0, tint, true);
                }
                Cell::Icon => {
                    gu.sprite("menu    keytops3", 0xc0 + r.fish_rank as usize + 1, x, ry + 2.0, WHITE);
                }
                Cell::Job => {
                    let rec = crate::net::search::Record {
                        present: 1 << 6,
                        job: r.job,
                        level: r.level,
                        race: r.race,
                        nation: r.allegiance,
                        rank: r.fish_rank,
                        ..Default::default()
                    };
                    super::search::job_cell(gu, &rec, x, ry + 2.0, WHITE, false);
                }
                Cell::Name => gu.text_fit(&r.name, x, ry + 2.0, end - x - 8.0, WHITE),
                Cell::Score(_) => {
                    let s = text(w, c, r).unwrap_or_default();
                    gu.shape_text(&s, end - 2.0, ry + 4.0, WHITE, true);
                }
                Cell::Triple | Cell::VanaDate | Cell::EarthDate { .. } => {
                    if let Some(s) = text(w, c, r) {
                        gu.shape_text(&s, x, ry + 4.0, WHITE, false);
                    }
                }
            }
        }
    }
    if focused && !rows.is_empty() {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + PITCH * (a.cursor - a.top) as f32, WHITE);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, rank: u8, fish: u8, score: u32) -> [u8; ENTRY] {
        let mut e = [0u8; ENTRY];
        e[..name.len()].copy_from_slice(name.as_bytes());
        e[0x10] = 3;
        e[0x17] = fish;
        e[0x18..0x1c].copy_from_slice(&score.to_le_bytes());
        e[0x20] = rank;
        e
    }

    /// LSB's 0x04D FISHRANKING: {cmd, result 1, kind 2, value, timestamp, size, offset, piece, entry count, our
    /// entry, the piece's entries}.
    fn reply(cmd: u8, result: i8, val: u8, total: i32, offset: i32, entries: &[[u8; ENTRY]]) -> Vec<u8> {
        let mut p = vec![0x4d, 0, 0, 0, cmd, result as u8, KIND, val];
        let size = (entries.len() * ENTRY) as i32;
        for v in [77, total, offset, size] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        p.extend_from_slice(&((total as usize / ENTRY) as u32).to_le_bytes());
        p.extend_from_slice(&entry("Me", 2, 5, 1234));
        for e in entries {
            p.extend_from_slice(e);
        }
        p
    }

    /// Command 1 over seven entries: the first piece (five) asked at once, then the rest as command 2; rows in
    /// order with a contest rank; our entry's values for op 0xb3 1.
    #[test]
    fn table_in_pieces() {
        let es: Vec<_> = (0..7).map(|i| entry(&format!("Fisher{i}"), if i == 6 { 0 } else { i as u8 + 1 }, 3, 900 - i)).collect();
        let total = (7 * ENTRY) as i32;
        let mut st = State::default();
        st.start(1, 4);
        let sent = st.tick();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0][..4], [1, 0, KIND, 4]);
        assert_eq!(sent[0][12..20], [[0u8; 4], PIECE.to_le_bytes()].concat()[..]);
        assert!(st.tick().is_empty(), "asked once until 3 s pass");
        st.reply(&reply(1, 1, 4, total, 0, &es[..5]));
        assert_eq!((st.xfer.as_ref().map(|t| t.state), st.req.done, st.req.result), (Some(2), PIECE, 0));
        let sent = st.tick();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0][0], 2);
        assert_eq!(sent[0][4..20], [77i32, total, PIECE, total - PIECE].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()[..]);
        st.reply(&reply(2, 1, 4, total, PIECE, &es[5..]));
        assert!(st.xfer.is_none());
        assert_eq!(st.req.result, 1);
        let rows = st.req.rows();
        assert_eq!(rows.len(), 6, "the rankless entry is left out");
        assert_eq!((rows[0].name.as_str(), rows[0].rank, rows[5].score), ("Fisher0", 1, 895));
        let w = st.req.clone();
        assert_eq!((w.header, w.own[0x20]), (7, 2));
    }

    /// A piece that does not follow the request fails the whole transfer (result -1); a second request waits for
    /// the first.
    #[test]
    fn bad_piece_fails() {
        let mut st = State::default();
        st.start(1, 4);
        st.tick();
        st.start(12, 4);
        assert_eq!(st.req.cmd, 1);
        st.reply(&reply(1, 1, 5, 36, 0, &[entry("A", 1, 0, 1)]));
        assert!(st.xfer.is_none());
        assert_eq!(st.req.result, -1);
    }

    /// The answer to 10..13 is the result byte; 0 ends as -1, a refusal keeps its value and our entry.
    #[test]
    fn answer_result() {
        for (res, want) in [(1i8, 1i8), (0, -1), (-2, -2), (-1, -1)] {
            let mut st = State::default();
            st.start(12, 4);
            let sent = st.tick();
            assert_eq!(sent[0][..4], [12, 0, KIND, 4]);
            st.reply(&reply(12, res, 4, 0, 0, &[]));
            assert!(st.xfer.is_none());
            assert_eq!(st.req.result, want);
            assert_eq!(st.req.own[0x20], if matches!(res, 1 | -2) { 2 } else { 0 });
        }
    }
}
