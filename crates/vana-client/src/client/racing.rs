//! Chocobo racing (spec ~/ffxi/lab/research/chocobo_racing_spec.md): the toteboard "menu    chocobor" and the race
//! card "menu    racecard" (event op 0xb4 0xf..0x12), their c2s 0x09B requests and s2c 0x073 / 0x074 answers, and
//! the race store of s2c 0x069 that event op 0xbf reads.

use super::World;
use super::game_ui::GameUi;
use std::time::Instant;

const BOARD: &str = "menu    chocobor";
const CARD: &str = "menu    racecard";
const WHITE: u32 = 0x8080_8080;

fn u32_at(p: &[u8], o: usize) -> u32 {
    p.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Copies `src` into `dst` from `at`, clamped (retail copies without bounds).
fn copy_at(dst: &mut [u8], at: usize, src: &[u8]) {
    if at < dst.len() {
        let n = src.len().min(dst.len() - at);
        dst[at..at + n].copy_from_slice(&src[..n]);
    }
}

/// S+0x3bde8.. (s2c 0x069, LAB_100f9ba0): the download flag, race params, 8 chocobos and 32 sections of 12 bytes,
/// the result.
#[derive(Clone, Debug)]
pub struct RaceStore {
    pub ready: bool,
    pub race: [u32; 2],
    chocobos: [u8; 96],
    sections: [u8; 384],
    result: [u8; 4],
}

impl Default for RaceStore {
    fn default() -> Self {
        Self { ready: false, race: [0; 2], chocobos: [0; 96], sections: [0; 384], result: [0; 4] }
    }
}

impl RaceStore {
    /// Mode @4, record index @5, size in bytes @6, data from @8; modes 1..4 clear the flag, any other sets it.
    pub fn apply(&mut self, p: &[u8]) {
        let (mode, index, size) = (p.get(4).copied().unwrap_or(0), p.get(5).copied().unwrap_or(0) as usize, p.get(6).copied().unwrap_or(0) as usize);
        let data = p.get(8..).unwrap_or_default();
        let data = &data[..size.min(data.len())];
        match mode {
            1 => self.race = [u32_at(p, 8), u32_at(p, 0xc)],
            2 => copy_at(&mut self.chocobos, 12 * index, data),
            3 => copy_at(&mut self.sections, 12 * index, data),
            4 => copy_at(&mut self.result, 0, data),
            _ => {}
        }
        self.ready = !(1..=4).contains(&mode);
    }

    /// Op 0xbf (FUN_100bbd80): 0 the race param [a], 0x20 chocobo [a] dword [b], 0x40 section [a] dword [b], 0x60
    /// the result; 0 until the store is complete or out of range.
    pub fn value(&self, sub: u8, a: i32, b: i32) -> u32 {
        if !self.ready {
            return 0;
        }
        let rec = |d: &[u8], n: i32| if (0..n).contains(&a) && (0..3).contains(&b) { u32_at(d, 12 * a as usize + 4 * b as usize) } else { 0 };
        match sub {
            0 if (0..2).contains(&a) => self.race[a as usize],
            0x20 => rec(&self.chocobos, 8),
            0x40 => rec(&self.sections, 32),
            0x60 if a == 0 => u32::from_le_bytes(self.result),
            _ => 0,
        }
    }
}

/// S+0x3c01c (s2c 0x074, LAB_100ec190): 8 chocobo params of 12 bytes and 8 {u32 flags, name[16]}; `filled` is the
/// mode-4 callback that hands them to the race card.
#[derive(Clone, Debug)]
pub struct RaceList {
    params: [u8; 96],
    names: [u8; 160],
    filled: bool,
}

impl Default for RaceList {
    fn default() -> Self {
        Self { params: [0; 96], names: [0; 160], filled: false }
    }
}

impl RaceList {
    /// Mode byte @0x10, start index byte @0x11; 1 zeroes, 2 copies 0x60 bytes of params, 3 0xa0 bytes of names (both
    /// from @0x14, whatever their size field), 4 calls back.
    pub fn apply(&mut self, p: &[u8]) {
        let (mode, index) = (p.get(0x10).copied().unwrap_or(0), p.get(0x11).copied().unwrap_or(0) as usize);
        let data = p.get(0x14..).unwrap_or_default();
        match mode {
            1 => *self = Self::default(),
            2 => copy_at(&mut self.params, 12 * index, &data[..data.len().min(0x60)]),
            3 => copy_at(&mut self.names, 20 * index, &data[..data.len().min(0xa0)]),
            4 => self.filled = true,
            _ => {}
        }
    }
}

/// The toteboard handler (DAT_1066443c): the attendant colour, the betting countdown and the refresh countdown in
/// ticks, the race name once loaded.
#[derive(Clone, Debug)]
pub struct Board {
    colour: i8,
    left: f32,
    refresh: f32,
    name: Option<String>,
}

/// The state retail keeps in the two handlers and the session block.
#[derive(Clone, Debug, Default)]
pub struct Racing {
    pub store: RaceStore,
    list: RaceList,
    /// 0x073's race number and grade (-1 unknown), the 28 odds and their colour tiers; kept across opens.
    race_no: u32,
    grade: i32,
    odds: [u16; 28],
    tiers: [u8; 28],
    board: Option<Board>,
    /// The race card's colour, and whether its pointers are set (the mode-4 callback after this open).
    card: Option<(i8, bool)>,
    /// FUN_100fc3c0's throttle for packet 0x9b: 1000 ms.
    sent: Option<Instant>,
}

/// c2s 0x09B {i32 Param = the colour sign-extended, u32 Kind (1 odds, 2 the field)}; refused within 1 s of the last.
fn request(w: &mut World, colour: i8, kind: u32) {
    if w.racing.sent.is_some_and(|t| t.elapsed().as_millis() < 1000) {
        return;
    }
    w.racing.sent = Some(Instant::now());
    let mut body = (colour as i32).to_le_bytes().to_vec();
    body.extend(kind.to_le_bytes());
    log::info!("racing: c2s 0x09B colour {colour} kind {kind}");
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x09B, body });
}

/// s2c 0x069 / 0x073 / 0x074.
pub fn on_packet(w: &mut World, id: u16, p: &[u8]) {
    let r = &mut w.racing;
    match id {
        0x069 => r.store.apply(p),
        0x073 => {
            // FUN_10209530: odds u16[28] @0x10, the tier by {200, 1000, 3000, 6000} (0x1033b2d4), Ident @8.
            for k in 0..28 {
                let o = p.get(0x10 + 2 * k..0x12 + 2 * k).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                r.odds[k] = o;
                r.tiers[k] = [200u16, 1000, 3000, 6000].iter().position(|&t| o < t).unwrap_or(3).min(3) as u8;
            }
            let ident = u32_at(p, 8);
            (r.race_no, r.grade) = (ident & 0x3ffff, (ident >> 18) as i32);
            log::info!("racing: odds for race {} grade {}", r.race_no, r.grade);
        }
        0x074 => {
            r.list.apply(p);
            if p.get(0x10) == Some(&4)
                && let Some(c) = &mut r.card
            {
                c.1 = true;
            }
        }
        _ => {}
    }
}

/// Op 0xb4 0xf (FUN_10209430): the toteboard for the colour, `seconds` of betting left; the grade and name start
/// unknown, the odds stay until the next 0x073.
pub fn open_board(w: &mut World, colour: i8, seconds: i32) {
    w.racing.grade = -1;
    w.racing.board = Some(Board { colour, left: seconds as f32 * 60.0, refresh: 0.0, name: None });
}

/// Op 0xb4 0x10.
pub fn close_board(w: &mut World) {
    w.racing.board = None;
}

/// Op 0xb4 0x11 (FUN_10209890, its init FUN_102098f0): nothing draws until this open's mode 4; asks once.
pub fn open_card(w: &mut World, colour: i8) {
    w.racing.card = Some((colour, false));
    request(w, colour, 2);
}

/// Op 0xb4 0x12.
pub fn close_card(w: &mut World) {
    w.racing.card = None;
}

/// FUN_102095b0 each frame: both countdowns run; when the refresh one goes below 0 it re-arms by the time left (30 s,
/// 15 s under 3 min, 10 s under 1 min, at once after the close: i16 0x8700) and asks for the odds.
pub fn step(w: &mut World, ticks: f32) {
    let Some(b) = &mut w.racing.board else { return };
    b.left -= ticks;
    b.refresh -= ticks;
    let colour = b.colour;
    if b.refresh < 0.0 {
        let s = (b.left / 60.0).trunc() as i32;
        b.refresh = match s {
            ..0 => -30976.0,
            0..60 => 600.0,
            60..180 => 900.0,
            _ => 1800.0,
        };
        request(w, colour, 1);
    }
    let grade = w.racing.grade;
    if let Some(b) = &mut w.racing.board
        && b.name.is_none()
        && grade >= 0
    {
        b.name = w.battle.as_ref().and_then(|t| t.race_name(grade as usize));
    }
}

/// FUN_101c93c0: "%d:%02d", "%d:%02d:%02d" from an hour.
fn clock(secs: i32) -> String {
    let s = secs.max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The pairs' column origins (0x10385584) and counts (0x1033b2ec), rows 16 apart.
const COLUMNS: [(f32, f32, usize); 7] = [(102.0, 83.0, 7), (182.0, 83.0, 6), (262.0, 83.0, 5), (342.0, 83.0, 4), (342.0, 179.0, 3), (262.0, 195.0, 2), (182.0, 211.0, 1)];
/// The tiers' colours (0x1033b2dc).
const TIER: [u32; 4] = [0x8050_9050, 0x8080_8080, 0x8090_9040, 0x8080_4020];

fn right(gu: &mut GameUi, s: &str, x: f32, y: f32, colour: u32) {
    let w = gu.text_width(s);
    gu.text(s, x - w, y, 0, colour.to_le_bytes());
}

/// FUN_10209660: the title "%s (Race No. %d)" (10 0x66) at (+0x10, +0x18); the countdown right-aligned at (x1 - 0x10,
/// +0x2c); the 28 odds right-aligned in the sprite font by tier; while betting is open "Toteboard updated in (%s)" (10
/// 0x64) right-aligned at (x1 - 4, y1 - 0x14) in 0x80404040.
fn draw_board(gu: &mut GameUi, w: &World, b: &Board, sw: f32, sh: f32) {
    let Some(win) = gu.lobby_window_off(BOARD, sw, sh, (0.0, 0.0), None, false, &[]) else { return };
    let [x0, y0, fw, fh] = win.frame;
    let (x1, y1) = (x0 + fw, y0 + fh);
    let r = &w.racing;
    let title = gu.lobby_msg(0x0a, 0x66, "%s (Race No. %d)").replacen("%s", b.name.as_deref().unwrap_or(""), 1).replacen("%d", &r.race_no.to_string(), 1);
    gu.text(&title, x0 + 16.0, y0 + 24.0, 0, WHITE.to_le_bytes());
    let line = if b.left <= 0.0 {
        gu.lobby_msg(0x0b, 0xbb, "Bet placement closed")
    } else if b.left < 60.0 {
        gu.lobby_msg(0x0b, 0xba, "Bet placement ending momentarily")
    } else {
        gu.lobby_msg(0x0a, 0x65, "Betting ends in (%s)").replacen("%s", &clock((b.left / 60.0) as i32), 1)
    };
    right(gu, &line, x1 - 16.0, y0 + 44.0, WHITE);
    let mut k = 0;
    for &(cx, cy, n) in &COLUMNS {
        for row in 0..n {
            let (bt, ui, _, _) = gu.parts();
            crate::render::screens::push_shape_text_right(bt, ui, r.odds[k].to_string().as_bytes(), x0 + cx, y0 + cy + 16.0 * row as f32, TIER[r.tiers[k] as usize & 3]);
            k += 1;
        }
    }
    if b.left > 0.0 {
        let s = gu.lobby_msg(0x0a, 0x64, "Toteboard updated in (%s)").replacen("%s", &clock((b.refresh / 60.0) as i32), 1);
        right(gu, &s, x1 - 4.0, y1 - 20.0, 0x8040_4040);
    }
}

/// FUN_10209920 for a row at y (30 px apart): the name in the sprite font at (+0x20, +5); the four rank letters
/// (keytops3 455 + byte >> 5) on the stat row; the crest (434 + flags & 3, when set); the colour ball (439 + DNA bits
/// 25..27) under the gender (444 + bit 24); the preferred weather (462 + bits 20..23, 1..8); abilities 1 / 2 (448 +
/// bits 9..12 / 13..16, when set); the orders (446 + byte 0 & 0xf).
fn draw_row(gu: &mut GameUi, x: f32, y: f32, name: &[u8], param: &[u8]) {
    let flags = u32_at(name, 0);
    let text = &name[4..20];
    let text = &text[..text.iter().position(|&c| c == 0).unwrap_or(16)];
    {
        let (bt, ui, _, _) = gu.parts();
        crate::render::screens::push_shape_text(bt, ui, text, x + 32.0, y + 5.0, WHITE);
    }
    let k3 = |gu: &mut GameUi, k: u32, sx: f32, sy: f32| {
        gu.sprite("menu    keytops3", k as usize, x + sx, y + sy, WHITE);
    };
    for (i, sx) in [0x3a, 0x68, 0x99, 0xcb].into_iter().enumerate() {
        k3(gu, 455 + (param[4 + i] >> 5) as u32, sx as f32, 18.0);
    }
    if flags & 3 != 0 {
        k3(gu, 434 + (flags & 3), 224.0, 8.0);
    }
    let dna = u32_at(param, 8);
    k3(gu, 439 + (dna >> 25 & 7), 240.0, 8.0);
    k3(gu, 444 + (dna >> 24 & 1), 240.0, 8.0);
    let weather = dna >> 20 & 0xf;
    if (1..=8).contains(&weather) {
        k3(gu, 462 + weather, 256.0, 8.0);
    }
    for (bits, sy) in [(dna >> 9 & 0xf, 7.0), (dna >> 13 & 0xf, 17.0)] {
        if bits != 0 {
            k3(gu, 448 + bits, 276.0, sy);
        }
    }
    k3(gu, 446 + (u32_at(param, 0) & 0xf), 333.0, 8.0);
}

/// FUN_10209b40: once its pointers are set, 8 rows whatever the entrants.
fn draw_card(gu: &mut GameUi, w: &World, ready: bool, sw: f32, sh: f32) {
    let Some(win) = gu.lobby_window_off(CARD, sw, sh, (0.0, 0.0), None, false, &[]) else { return };
    if !ready {
        return;
    }
    let [x0, y0, ..] = win.frame;
    let l = &w.racing.list;
    for i in 0..8 {
        draw_row(gu, x0, y0 + 30.0 * i as f32, &l.names[20 * i..20 * i + 20], &l.params[12 * i..12 * i + 12]);
    }
}

/// Both windows (no input: their vtable input slots are the base no-ops).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    if let Some(b) = &w.racing.board {
        draw_board(gu, w, b, sw, sh);
    }
    if let Some((_, ready)) = w.racing.card {
        draw_card(gu, w, ready, sw, sh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_reads() {
        let mut s = RaceStore::default();
        let mut p = vec![0u8; 8];
        p[4] = 1;
        p.extend(0x0000_0128u32.to_le_bytes());
        p.extend(0x8000_0002u32.to_le_bytes());
        s.apply(&p);
        assert!(!s.ready);
        assert_eq!(s.value(0, 0, 0), 0);
        // Mode 2: 8 chocobos from record 0; mode 4: the result; mode 5 completes.
        let mut c = vec![0, 0, 0, 0, 2, 0, 96, 0];
        c.extend((0..24u32).flat_map(|v| v.to_le_bytes()));
        s.apply(&c);
        s.apply(&[0, 0, 0, 0, 4, 0, 4, 0, 0x10, 0x32, 0x54, 0x76]);
        s.apply(&[0, 0, 0, 0, 5, 0, 0, 0]);
        assert!(s.ready);
        assert_eq!((s.value(0, 0, 0), s.value(0, 1, 0), s.value(0, 2, 0)), (0x128, 0x8000_0002, 0));
        assert_eq!((s.value(0x20, 1, 2), s.value(0x20, 8, 0), s.value(0x60, 0, 0)), (5, 0, 0x7654_3210));
    }

    #[test]
    fn list_modes() {
        let mut l = RaceList::default();
        let mut p = vec![0u8; 0x14];
        p[0x10] = 3;
        p.extend(1u32.to_le_bytes());
        p.extend(b"Bluechoco\0\0\0\0\0\0\0");
        l.apply(&p);
        assert_eq!(&l.names[4..13], b"Bluechoco");
        p[0x10] = 4;
        l.apply(&p);
        assert!(l.filled);
        p[0x10] = 1;
        l.apply(&p);
        assert!(!l.filled && l.names[4] == 0);
    }

    #[test]
    fn clocks() {
        assert_eq!((clock(65), clock(3661), clock(-3)), ("1:05".to_string(), "1:01:01".to_string(), "0:00".to_string()));
    }
}
