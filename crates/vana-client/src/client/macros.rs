//! Macros (data object DAT_10662798, controller DAT_1066279c): the USER/<content id>/ files, the current set's 20
//! macros (0..9 Ctrl+1..0, 10..19 Alt+1..0), the runner that sends one line a frame (FUN_101a1cc0) and its waits
//! (FUN_101a2480). Lines and titles stay raw Shift-JIS bytes so the files round-trip.

use md5::{Digest, Md5};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One record of a set file (0x17c bytes): u32 (unused), 6 lines of 61 bytes, a 10-byte title.
#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    pub lines: [[u8; 61]; 6],
    pub title: [u8; 10],
}

impl Default for Macro {
    fn default() -> Self {
        Macro { lines: [[0; 61]; 6], title: [0; 10] }
    }
}

const RECORD: usize = 0x17c;
const SET_PAYLOAD: usize = 20 * RECORD;
const BOOKS: usize = 40;

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

impl Macro {
    fn parse(r: &[u8]) -> Self {
        let mut m = Macro::default();
        for (k, l) in m.lines.iter_mut().enumerate() {
            l.copy_from_slice(&r[4 + 61 * k..4 + 61 * (k + 1)]);
        }
        m.title.copy_from_slice(&r[0x172..0x17c]);
        // FUN_101a1840: a title without a NUL in 9 bytes or a line without one clears the record.
        if !m.title[..9].contains(&0) || m.lines.iter().any(|l| !l.contains(&0)) { Macro::default() } else { m }
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[0; 4]);
        for l in &self.lines {
            out.extend_from_slice(l);
        }
        out.extend_from_slice(&self.title);
    }

    pub fn title(&self) -> String {
        crate::dat::font::decode_sjis(cstr(&self.title))
    }

    pub fn line(&self, k: usize) -> &[u8] {
        cstr(&self.lines[k])
    }

    fn set_line(&mut self, k: usize, text: &[u8]) {
        let n = text.len().min(60);
        self.lines[k] = [0; 61];
        self.lines[k][..n].copy_from_slice(&text[..n]);
    }
}

/// The user-file container (FUN_101b9a10 / FUN_101b9bc0): u32 version (as registered: 1 for the macro files, 2
/// mix.dat, 4 aix.dat), u32 (0), MD5 of the payload, payload.
pub(super) fn read_file(path: &Path, version: u32) -> Option<Vec<u8>> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 0x18 || u32::from_le_bytes(b[0..4].try_into().unwrap()) != version {
        return None;
    }
    let payload = b[0x18..].to_vec();
    (Md5::digest(&payload).as_slice() == &b[8..0x18]).then_some(payload)
}

pub(super) fn write_file(path: &Path, version: u32, payload: &[u8]) {
    let mut out = Vec::with_capacity(0x18 + payload.len());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(Md5::digest(payload).as_slice());
    out.extend_from_slice(payload);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, out) {
        log::warn!("macro file {}: {e}", path.display());
    }
}

/// mcr.dat's defaults (FUN_101a1fe0): titles from FFXiMain, lines from group 9 (read until an empty one).
const DEFAULTS: [(&str, &[usize]); 20] = [
    ("Find", &[0x87]), ("Pos", &[0x89]), ("Help", &[0x8b]), ("Link", &[0x8d]), ("", &[]), ("NameSW", &[0x8f]), ("InviteSW", &[0x91]),
    ("AnonSW", &[0x93]), ("NoMP", &[0x95]), ("TP", &[0x97]), ("Yes", &[0x99]), ("No", &[0x9b]), ("Wave", &[0x9d]), ("PartyAdd", &[0x9f]),
    ("JoinPT", &[0xa1, 0xa2, 0xa3, 0xa4, 0xa5]), ("SoloPlay", &[0xa7, 0xa8, 0xa9, 0xaa]), ("Nominate", &[0xac]), ("LeftAns", &[0xae]),
    ("RightAns", &[0xb0]), ("Entice", &[0xb2, 0xb3, 0xb4, 0xb5]),
];

#[derive(Clone, Debug)]
struct Running {
    m: Macro,
    line: usize,
    wait: Option<(Instant, Duration)>,
}

/// A list order file: bits[n / 8], u16 order[n], u32 count (mix.dat n 0x400, aix.dat n 0xb00). Retail keeps it in
/// memory from field entry to leaving the field and merges it in place with the known bits on every list build.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub bits: Vec<u8>,
    pub list: Vec<u16>,
    pub count: usize,
}

impl Order {
    fn empty(n: usize) -> Self {
        Order { bits: vec![0; n / 8], list: vec![0; n], count: 0 }
    }

    /// LAB_1022c000 / LAB_10223c00: a bad file, or a count of `max` or more, reads as all zero.
    fn load(path: &Path, version: u32, n: usize, max: usize) -> Self {
        let nb = n / 8;
        let Some(p) = read_file(path, version).filter(|p| p.len() == nb + 2 * n + 4) else { return Self::empty(n) };
        let count = u32::from_le_bytes(p[nb + 2 * n..].try_into().unwrap()) as usize;
        if count >= max {
            return Self::empty(n);
        }
        Order { bits: p[..nb].to_vec(), list: (0..n).map(|i| u16::from_le_bytes([p[nb + 2 * i], p[nb + 2 * i + 1]])).collect(), count }
    }

    fn payload(&self) -> Vec<u8> {
        let mut p = self.bits.clone();
        p.extend(self.list.iter().flat_map(|v| v.to_le_bytes()));
        p.extend_from_slice(&(self.count as u32).to_le_bytes());
        p
    }

    /// FUN_1021e550: newly known ids appended in id order, forgotten ones taken out (the tail shifts down).
    pub fn merge(&mut self, known: &[u8]) {
        let n = self.list.len();
        let bit = |b: &[u8], i: usize| b.get(i / 8).is_some_and(|x| x & (1 << (i % 8)) != 0);
        for id in 1..n {
            match (bit(known, id), bit(&self.bits, id)) {
                (true, false) => {
                    self.bits[id / 8] |= 1 << (id % 8);
                    if self.count < n {
                        self.list[self.count] = id as u16;
                        self.count += 1;
                    }
                }
                (false, true) => {
                    self.bits[id / 8] &= !(1 << (id % 8));
                    if let Some(p) = self.list[..self.count].iter().position(|&x| x as usize == id) {
                        self.list.copy_within(p + 1.., p);
                        self.list[n - 1] = 0;
                        self.count -= 1;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn ids(&self) -> &[u16] {
        &self.list[..self.count]
    }

    /// FUN_1022c8e0 / FUN_10224320: the id at `from` taken out and put in at `to`, the ones between shifting (not a
    /// swap).
    pub fn move_to(&mut self, from: usize, to: usize) {
        if from >= self.count || to >= self.count {
            return;
        }
        let id = self.list.remove(from);
        self.list.insert(to, id);
    }
}

pub struct Macros {
    dir: PathBuf,
    /// mix.dat (version 2) and aix.dat (version 4): the magic and ability list orders.
    pub mix: Order,
    pub aix: Order,
    /// +0x1dc0 book 0..39, +0x1db8 / 2 set 0..9.
    pub book: usize,
    pub set: usize,
    pub recs: Vec<Macro>,
    pub titles: Vec<[u8; 16]>,
    run: Option<Running>,
    /// Retail stepped once a frame at 30 fps: one step per 2 ticks here.
    acc: f32,
    /// Lines of group 9 for the defaults, by id.
    group9: Box<dyn Fn(usize) -> Option<String>>,
}

impl Macros {
    /// FUN_101c3d00: `<game>/USER/<content id, lowercase hex>`; everything is read at field entry.
    pub fn load(game: &Path, content_id: u32, group9: Box<dyn Fn(usize) -> Option<String>>) -> Self {
        let dir = game.join("USER").join(format!("{content_id:x}"));
        let (mix, aix) = (Order::load(&dir.join("mix.dat"), 2, 0x400, 0x400), Order::load(&dir.join("aix.dat"), 4, 0xb00, 0x700));
        let mut m = Macros { dir, mix, aix, book: 0, set: 0, recs: vec![Macro::default(); 20], titles: vec![[0; 16]; BOOKS], run: None, acc: 0.0, group9 };
        for (k, name) in ["mcr.ttl", "mcr_2.ttl"].iter().enumerate() {
            if let Some(p) = read_file(&m.dir.join(name), 1).filter(|p| p.len() == 0x140) {
                for i in 0..20 {
                    m.titles[k * 20 + i].copy_from_slice(&p[16 * i..16 * i + 16]);
                }
            }
        }
        // mcr.sys: book * 20 + 2 * set.
        if let Some(p) = read_file(&m.dir.join("mcr.sys"), 1).filter(|p| p.len() == 4) {
            let c = u32::from_le_bytes(p[..4].try_into().unwrap()) as usize;
            if c < 800 {
                (m.book, m.set) = (c / 20, (c % 20) / 2);
            }
        }
        m.load_set();
        m
    }

    /// The character's USER folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn set_path(&self) -> PathBuf {
        let n = self.book * 10 + self.set;
        self.dir.join(if n == 0 { "mcr.dat".to_string() } else { format!("mcr{n}.dat") })
    }

    /// FUN_101a26c0 / FUN_101a1e20: the set file; missing or bad: mcr.dat takes the defaults, others are cleared
    /// and written back.
    fn load_set(&mut self) {
        match read_file(&self.set_path(), 1) {
            Some(p) if p.len() == SET_PAYLOAD => self.recs = p.chunks(RECORD).map(Macro::parse).collect(),
            Some(_) => self.recs = vec![Macro::default(); 20],
            None if self.book == 0 && self.set == 0 => self.recs = self.defaults(),
            None => {
                self.recs = vec![Macro::default(); 20];
                self.save_set();
            }
        }
    }

    fn defaults(&self) -> Vec<Macro> {
        DEFAULTS
            .iter()
            .map(|(title, ids)| {
                let mut m = Macro::default();
                m.title[..title.len()].copy_from_slice(title.as_bytes());
                for (k, line) in ids.iter().map_while(|&id| (self.group9)(id).filter(|l| !l.is_empty())).take(6).enumerate() {
                    m.set_line(k, &crate::dat::font::encode_sjis(&line));
                }
                m
            })
            .collect()
    }

    fn save_set(&self) {
        let mut p = Vec::with_capacity(SET_PAYLOAD);
        for m in &self.recs {
            m.write(&mut p);
        }
        write_file(&self.set_path(), 1, &p);
    }

    /// FUN_101ab7d0: leaving the field writes mcr.sys, both title files, the current set and the list orders (mix.dat
    /// is skipped in Monstrosity, which we do not have).
    pub fn save(&self) {
        write_file(&self.dir.join("mix.dat"), 2, &self.mix.payload());
        write_file(&self.dir.join("aix.dat"), 4, &self.aix.payload());
        write_file(&self.dir.join("mcr.sys"), 1, &((self.book * 20 + 2 * self.set) as u32).to_le_bytes());
        for (k, name) in ["mcr.ttl", "mcr_2.ttl"].iter().enumerate() {
            let p: Vec<u8> = self.titles[k * 20..k * 20 + 20].iter().flatten().copied().collect();
            write_file(&self.dir.join(name), 1, &p);
        }
        self.save_set();
    }

    /// FUN_101a2890 / FUN_101a2920: switching writes the old set first; out of range wraps once, else the first.
    pub fn switch(&mut self, book: usize, set: usize) {
        self.save_set();
        let wrap = |v: usize, n: usize| if v < n { v } else if v - n < n { v - n } else { 0 };
        (self.book, self.set) = (wrap(book, BOOKS), wrap(set, 10));
        self.load_set();
    }

    /// The book's title, "Book%02d" when empty (group 10 0x6c).
    pub fn book_title(&self, book: usize) -> String {
        let t = crate::dat::font::decode_sjis(cstr(&self.titles[book]));
        if t.is_empty() { format!("Book{:02}", book + 1) } else { t }
    }

    /// FUN_101a1c60: runs macro `n` from its first line, replacing a running one.
    pub fn start(&mut self, n: usize) {
        self.run = self.recs.get(n).map(|m| Running { m: m.clone(), line: 0, wait: None });
    }

    pub fn stop(&mut self) {
        self.run = None;
    }

    /// FUN_101a1cc0, once per 2-tick frame: the next line to send, if any. The first empty line ends the macro, "//"
    /// lines are skipped (a frame each), "/wait n" (1..60 s) waits without sending, "<wait n>" (0..60) sends the line
    /// cut at the tag and then waits.
    pub fn step(&mut self, ticks: f32) -> Option<Vec<u8>> {
        self.acc += ticks;
        if self.acc < 2.0 {
            return None;
        }
        self.acc = self.acc.min(4.0) - 2.0;
        let r = self.run.as_mut()?;
        if let Some((at, d)) = r.wait {
            if at.elapsed() >= d {
                r.wait = None;
            }
            return None;
        }
        let text = r.m.lines.get(r.line).map(|l| cstr(l).to_vec()).unwrap_or_default();
        if text.is_empty() {
            self.run = None;
            return None;
        }
        r.line += 1;
        if text.starts_with(b"//") {
            return None;
        }
        if let Some(i) = text.windows(5).position(|w| w == b"<wait")
            && text[i..].contains(&b'>')
        {
            let tail = &text[i + 5..];
            let n: String = tail.iter().take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
            let secs = if n.is_empty() { Some(1) } else { n.parse::<u64>().ok().filter(|v| *v <= 60) };
            if let Some(s) = secs {
                r.wait = Some((Instant::now(), Duration::from_secs(s)));
            }
            let cut = text[..i].to_vec();
            return (!cut.iter().all(|c| *c == b' ')).then_some(cut);
        }
        let s = String::from_utf8_lossy(&text);
        let mut words = s.split_whitespace();
        if words.next() == Some("/wait") {
            let n = words.next().map_or(Some(1), |w| w.parse::<i64>().ok());
            if let Some(n) = n.filter(|n| (1..=60).contains(n)) {
                r.wait = Some((Instant::now(), Duration::from_secs(n as u64)));
                return None;
            }
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("vana-macros-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn files_round_trip_and_defaults() {
        let game = scratch();
        let g9 = |id: usize| match id {
            0x99 => Some("/yes".to_string()),
            0xa1 => Some("/join".to_string()),
            0xa2 => Some("/wait 3".to_string()),
            0xa3 => Some(String::new()),
            _ => None,
        };
        let mut m = Macros::load(&game, 0x2a, Box::new(g9));
        assert_eq!(m.recs[10].title(), "Yes");
        assert_eq!(m.recs[10].line(0), b"/yes");
        assert_eq!((m.recs[14].line(0), m.recs[14].line(1), m.recs[14].line(2)), (&b"/join"[..], &b"/wait 3"[..], &b""[..]));
        m.recs[0].set_line(0, b"/echo hi");
        m.switch(0, 1);
        assert!(game.join("USER/2a/mcr.dat").exists() && game.join("USER/2a/mcr1.dat").exists());
        assert_eq!(std::fs::metadata(game.join("USER/2a/mcr1.dat")).unwrap().len(), 7624);
        m.switch(0, 0);
        assert_eq!(m.recs[0].line(0), b"/echo hi");
        m.save();
        let again = Macros::load(&game, 0x2a, Box::new(|_| None));
        assert_eq!((again.book, again.set, again.recs[0].line(0)), (0, 0, &b"/echo hi"[..]));
        assert_eq!(again.book_title(3), "Book04");
        // Wraps: set 11 -> 1 (index 0), book 81 -> 1.
        let mut w = again;
        w.switch(80, 10);
        assert_eq!((w.book, w.set), (0, 0));
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn runner_lines_and_waits() {
        let game = scratch();
        let mut m = Macros::load(&game, 1, Box::new(|_| None));
        let mut mac = Macro::default();
        for (k, l) in [&b"// note"[..], b"/say a", b"/wait 1", b"/say b<wait 0>junk", b"/say c", b"", ].iter().enumerate() {
            mac.set_line(k, l);
        }
        m.recs[3] = mac;
        m.start(3);
        let mut sent = Vec::new();
        for _ in 0..300 {
            if let Some(l) = m.step(2.0) {
                sent.push(String::from_utf8(l).unwrap());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sent, vec!["/say a", "/say b", "/say c"]);
        let _ = std::fs::remove_dir_all(&game);
    }
}

#[cfg(test)]
mod order_tests {
    use super::Order;

    fn known(ids: &[u16]) -> Vec<u8> {
        let mut k = vec![0u8; 128];
        for id in ids {
            k[(*id / 8) as usize] |= 1 << (id % 8);
        }
        k
    }

    /// Saved ids still known keep their places, a forgotten one goes, newly known ones follow in id order.
    #[test]
    fn merge_in_place() {
        let mut o = Order::empty(1024);
        for (i, id) in [7u16, 3, 5].iter().enumerate() {
            o.bits[(*id / 8) as usize] |= 1 << (id % 8);
            o.list[i] = *id;
        }
        o.count = 3;
        o.merge(&known(&[3, 7, 9, 2]));
        assert_eq!(o.ids(), [7, 3, 2, 9]);
        let mut e = Order::empty(1024);
        e.merge(&known(&[3, 7, 9, 2]));
        assert_eq!(e.ids(), [2, 3, 7, 9]);
    }

    #[test]
    fn move_inserts() {
        let mut o = Order::empty(16);
        o.list[..5].copy_from_slice(&[1, 2, 3, 4, 5]);
        o.count = 5;
        o.move_to(0, 3);
        assert_eq!(o.ids(), [2, 3, 4, 1, 5]);
        o.move_to(4, 1);
        assert_eq!(o.ids(), [2, 5, 3, 4, 1]);
    }

    #[test]
    fn version_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("vana-order-test-{}", std::process::id()));
        let path = dir.join("mix.dat");
        let mut o = Order::empty(0x400);
        o.merge(&known(&[4, 9]));
        super::write_file(&path, 2, &o.payload());
        assert_eq!(Order::load(&path, 2, 0x400, 0x400), o);
        assert_eq!(Order::load(&path, 1, 0x400, 0x400), Order::empty(0x400));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
