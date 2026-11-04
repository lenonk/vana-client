//! Misc. 4 chat filter (config 0xce: 1 hide, 2 replace, 0 off). Retail (FFXiMain.dll) reads two files from the
//! game folder, prohibited.txt and allowed.txt (FUN_101bd820), when the setting turns on (and at login, as the
//! callback runs on registration). A missing or oversized prohibited.txt prints an error and turns the setting off
//! (FUN_1019bec0); a missing allowed.txt only prints it.
//!
//! Each file (UTF-8, at most 0x23f0 bytes, 0xc80 UTF-16 units, 100 lines) holds one pattern per line
//! (FUN_101bd880): an optional "(isn)" prefix (i: fold case / width, s: ignore spaces, n: skip the sender's name),
//! up to 30 characters, "\" before one of ()[]^-. for the plain character. A line whose brackets check out
//! ("[abc]", "[a-z]", "[^x]", "[.]" any) is matched as a pattern (FUN_101bd510), others as a substring.
//! A shout, yell or assist line (s2c 0x017 kinds 1, 0x1a, 0x22, 0x23) from someone other than a GM is checked
//! (FUN_100da5e0, FUN_101bcf80): the sender's name and the text (auto-translate as plain text, control codes
//! dropped, 150 bytes). An allowed.txt hit lets it through; else a prohibited.txt hit hides it (1) or replaces the
//! text with group 9 0x159 (2).

use std::path::{Path, PathBuf};

use super::World;

const LINES: usize = 100;
/// Pattern units at entry +4 (u16[31], the last always the terminator).
const COLS: usize = 31;
/// Retail's limits: file bytes (FUN_101bd880) and UTF-16 units.
const MAX_BYTES: usize = 0x23f0;
const MAX_UNITS: usize = 0xc80;

/// One 100-byte entry: flags +0, pattern +4, the pattern flag +0x42, the plain-character marks +0x43.
#[derive(Clone, Copy)]
struct Entry {
    flags: u32,
    pat: [u16; COLS + 1],
    regex: bool,
    esc: [bool; COLS],
}

impl Default for Entry {
    fn default() -> Self {
        Self { flags: 0, pat: [0; COLS + 1], regex: false, esc: [false; COLS] }
    }
}

impl Entry {
    fn pc(&self, i: isize) -> u16 {
        if i < 0 { 0 } else { self.pat.get(i as usize).copied().unwrap_or(0) }
    }
    /// The mark before the first unit is the pattern byte (+0x42), set whenever the matcher runs.
    fn ec(&self, i: isize) -> bool {
        if i < 0 { self.regex } else { self.esc.get(i as usize).copied().unwrap_or(false) }
    }
}

/// Why a file was not taken: missing (group 9 0x15a) or too big / unreadable (0x15b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Missing(String),
    Bad(String),
}

#[derive(Default)]
pub struct SpamFilter {
    dir: PathBuf,
    /// Object +4: some line had a "(...)" prefix, so the folded copies are made.
    any_flags: bool,
    prohibited: Option<Vec<Entry>>,
    allowed: Option<Vec<Entry>>,
    /// Errors waiting for the message tables.
    pub pending: Vec<LoadError>,
}

impl SpamFilter {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, ..Default::default() }
    }

    /// FUN_101bd820: both lists afresh; Err when prohibited.txt is not there (the setting goes off).
    pub fn load(&mut self) -> Result<(), LoadError> {
        let dir = self.dir.clone();
        *self = Self { dir, pending: std::mem::take(&mut self.pending), ..Default::default() };
        self.prohibited = Some(self.read(&self.dir.clone(), "prohibited.txt")?);
        match self.read(&self.dir.clone(), "allowed.txt") {
            Ok(l) => self.allowed = Some(l),
            Err(e) => self.pending.push(e),
        }
        Ok(())
    }

    fn read(&mut self, dir: &Path, name: &str) -> Result<Vec<Entry>, LoadError> {
        let path = dir.join(name);
        // FUN_101c4c80 failing to open is 0x15a, a failed read (FUN_101c47c0) 0x15b.
        let data = std::fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => LoadError::Missing(name.to_string()),
            _ => LoadError::Bad(name.to_string()),
        })?;
        if data.len() > MAX_BYTES {
            return Err(LoadError::Bad(name.to_string()));
        }
        Ok(parse(&data, &mut self.any_flags))
    }

    /// FUN_101bcf80: 2 allowed, 1 prohibited, 0 neither.
    pub fn check(&self, name: &str, text: &str) -> u8 {
        let t = Texts::new(name, text, self.any_flags);
        if self.allowed.as_ref().is_some_and(|l| list_hit(l, &t)) {
            2
        } else if self.prohibited.as_ref().is_some_and(|l| list_hit(l, &t)) {
            1
        } else {
            0
        }
    }
}

/// FUN_1019bec0 for 1 / 2: load the lists; when prohibited.txt cannot be read the setting goes back to 0.
pub(super) fn apply(w: &mut World) {
    if !matches!(w.game.get(0xce), 1 | 2) {
        return;
    }
    if let Err(e) = w.spam.load() {
        w.spam.pending.push(e);
        if w.game.set(0xce, 0) {
            w.game_save = true;
        }
    }
    flush(w);
}

/// The load errors into the log once the message tables are in: group 9 0x15a (file, folder) for a missing file,
/// 0x15b (file) for one too big.
pub(super) fn flush(w: &mut World) {
    let Some(bt) = w.battle.clone() else { return };
    let dir = w.spam.dir.display().to_string();
    for e in std::mem::take(&mut w.spam.pending) {
        let line = match &e {
            LoadError::Missing(n) => bt.group9(0x15a).unwrap_or_else(|| "Could not open %s (%s).".into()).replacen("%s", n, 1).replacen("%s", &dir, 1),
            LoadError::Bad(n) => bt.group9(0x15b).unwrap_or_else(|| "Could not read %s.".into()).replacen("%s", n, 1),
        };
        super::push_chat_mode(w, line, 0x7b, 0);
    }
}

pub(super) enum Verdict {
    Show,
    Hide,
    Replace(String),
}

/// LAB_100e37a0 -> FUN_100da5e0: shouts, yells and assist lines (kinds 1, 0x1a, 0x22, 0x23, table 0x100e3d7c) not
/// from a GM, while config 0xce is 1 or 2.
pub(super) fn screen(w: &World, kind: u8, attr: u8, from: &str, text: &str) -> Verdict {
    let mode = w.game.get(0xce);
    if attr & 1 != 0 || !matches!(kind, 1 | 0x1a | 0x22 | 0x23) || !matches!(mode, 1 | 2) {
        return Verdict::Show;
    }
    let names = w.names.clone();
    let shown = match w.battle.as_ref().and_then(|b| b.phrases.as_ref()) {
        Some(d) => d.expand_plain(text, names.as_deref().map(|n| n as &dyn crate::dat::names::NameSource)),
        None => text.to_string(),
    };
    match (w.spam.check(from, &plain(&shown)), mode) {
        (1, 1) => Verdict::Hide,
        (1, _) => Verdict::Replace(w.battle.as_ref().and_then(|b| b.group9(0x159)).unwrap_or_default()),
        _ => Verdict::Show,
    }
}

/// The checked text (FUN_100da5e0): auto-translate as its plain phrase (the caller), control codes dropped
/// (FUN_1018fd30: the in-line 1e / 1f codes, our U+100400 characters, and any phrase block left unexpanded), at most 150 bytes in the client's Shift-JIS (FUN_101b7260 into 0x97).
pub fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut bytes = 0;
    let code = |c: char| (crate::dat::font::PHRASE_BASE..crate::dat::font::CODE_BASE + 0x200).contains(&(c as u32));
    for c in text.chars().filter(|&c| !(c < ' ' || ('\u{E000}'..='\u{F8FF}').contains(&c) || code(c))) {
        let n = if c.is_ascii() || ('\u{FF61}'..='\u{FF9F}').contains(&c) { 1 } else { 2 };
        if bytes + n > 150 {
            break;
        }
        bytes += n;
        out.push(c);
    }
    out
}

/// The six copies FUN_101bcf80 builds: the name (16 units) as is and folded; the text as is, folded, and both
/// without white space.
struct Texts {
    name: Vec<u16>,
    name_f: Vec<u16>,
    raw: Vec<u16>,
    sp_f: Vec<u16>,
    ns_raw: Vec<u16>,
    ns_f: Vec<u16>,
}

impl Texts {
    fn new(name: &str, text: &str, fold_too: bool) -> Self {
        let name: Vec<u16> = name.encode_utf16().take(16).collect();
        let raw: Vec<u16> = text.encode_utf16().take(0x400).collect();
        let (mut name_f, mut sp_f, mut ns_raw, mut ns_f) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        if fold_too {
            name_f = name.iter().map(|&c| if foldable(c) { fold(c) } else { c }).collect();
            for &c in &raw {
                if is_space(c) {
                    sp_f.push(c);
                } else {
                    let f = if foldable(c) { fold(c) } else { c };
                    sp_f.push(f);
                    ns_raw.push(c);
                    ns_f.push(f);
                }
            }
        }
        Self { name, name_f, raw, sp_f, ns_raw, ns_f }
    }
}

/// FUN_101bd1e0 / FUN_101bd360: an entry hits the name (unless n) or the text copy its flags pick.
fn list_hit(list: &[Entry], t: &Texts) -> bool {
    list.iter().filter(|e| e.pat[0] != 0).any(|e| {
        let hit = |s: &[u16]| if e.regex { pattern_find(s, e) } else { contains(s, e) };
        if e.flags & 8 == 0 && hit(if e.flags & 4 != 0 { &t.name_f } else { &t.name }) {
            return true;
        }
        hit(match (e.flags & 2 != 0, e.flags & 4 != 0) {
            (true, true) => &t.ns_f,
            (true, false) => &t.ns_raw,
            (false, true) => &t.sp_f,
            (false, false) => &t.raw,
        })
    })
}

/// wcsstr.
fn contains(s: &[u16], e: &Entry) -> bool {
    let n = e.pat.iter().position(|&c| c == 0).unwrap_or(COLS);
    s.windows(n).any(|w| w == &e.pat[..n])
}

/// FUN_10312bee (_isctype 8): the C table below 0x100, the system's classes above.
fn is_space(c: u16) -> bool {
    if c < 0x100 { matches!(c, 9..=0xd | 0x20) } else { char::from_u32(c as u32).is_some_and(char::is_whitespace) }
}

/// FUN_101be030: what FUN_101be0b0 changes.
fn foldable(c: u16) -> bool {
    (0x61..0x7b).contains(&c)
        || (0xff41..0xff5b).contains(&c)
        || (0xe0..0xf7).contains(&c)
        || (0xf8..0x100).contains(&c)
        || (0x430..0x450).contains(&c)
        || (0x21..0x7f).contains(&c)
        || (0xff66..0xff9e).contains(&c)
}

/// FUN_101be0b0: lower case to upper (ASCII, full-width, Latin-1, Cyrillic), ASCII to full width, half-width
/// katakana to full width (table 0x1035cbdc).
fn fold(c: u16) -> u16 {
    let mut v = c;
    if (0x61..0x7b).contains(&c) || (0xff41..0xff5b).contains(&c) {
        v = v.wrapping_sub(0x20);
    }
    if (0xe0..0xf7).contains(&c) || (0xf8..0x100).contains(&c) || (0x430..0x450).contains(&c) {
        v = v.wrapping_sub(0x20);
    }
    if (0x21..0x7f).contains(&c) {
        v = v.wrapping_add(0xfee0);
    }
    if (0xff66..0xff9e).contains(&v) {
        v = KANA[(v - 0xff66) as usize];
    }
    v
}

const KANA: [u16; 56] = [
    0x30f2, 0x30a1, 0x30a3, 0x30a5, 0x30a7, 0x30a9, 0x30e3, 0x30e5, 0x30e7, 0x30c3, 0x30fc, 0x30a2, 0x30a4, 0x30a6,
    0x30a8, 0x30aa, 0x30ab, 0x30ad, 0x30af, 0x30b1, 0x30b3, 0x30b5, 0x30b7, 0x30b9, 0x30bb, 0x30bd, 0x30bf, 0x30c1,
    0x30c4, 0x30c6, 0x30c8, 0x30ca, 0x30cb, 0x30cc, 0x30cd, 0x30ce, 0x30cf, 0x30d2, 0x30d5, 0x30d8, 0x30db, 0x30de,
    0x30df, 0x30e0, 0x30e1, 0x30e2, 0x30e4, 0x30e6, 0x30e8, 0x30e9, 0x30ea, 0x30eb, 0x30ec, 0x30ed, 0x30ef, 0x30f3,
];

/// FUN_101be1c0 and its siblings: the character, unless marked plain.
fn is(c: u16, plain: bool, want: u8) -> bool {
    !plain && c == want as u16
}

/// FUN_101be140: any of [ ] - ^ . not marked plain.
fn special(c: u16, plain: bool) -> bool {
    b"[]-^.".iter().any(|&w| is(c, plain, w))
}

/// FUN_101bd880: the lines into entries; `any_flags` set when a line opens with a "(...)" prefix.
fn parse(data: &[u8], any_flags: &mut bool) -> Vec<Entry> {
    let w: Vec<u16> = String::from_utf8_lossy(data).encode_utf16().take(MAX_UNITS).collect();
    let n = w.len();
    if n == 0 {
        return Vec::new();
    }
    let mut ents = vec![Entry::default(); LINES];
    let (mut i, mut col, mut ln) = (0usize, 0usize, 0usize);
    let (mut bad, mut open, mut close) = (false, 0u32, 0u32);
    let nl = |c: u16| c == 0x0d || c == 0x0a;
    let count = 'scan: {
        while i < n {
            let c = w[i];
            if nl(c) {
                ents[ln].pat[col.min(COLS - 1)] = 0;
                col = 0;
                if i + 1 < n && nl(w[i + 1]) {
                    i += 1;
                }
                ln += 1;
                if ln >= LINES {
                    break 'scan LINES;
                }
                i += 1;
                continue;
            }
            if col == 0 {
                bad = false;
                open = 0;
                close = 0;
                ents[ln].regex = false;
                if c == b'(' as u16 {
                    let (mut fl, mut j) = (0u32, i + 1);
                    let mut closed = false;
                    while j < n && !nl(w[j]) {
                        match w[j] {
                            0x69 => fl |= 4,
                            0x73 => fl |= 2,
                            0x6e => fl |= 8,
                            x => {
                                closed = x == b')' as u16;
                                break;
                            }
                        }
                        j += 1;
                    }
                    if closed {
                        i = j + 1;
                        *any_flags = true;
                    } else {
                        fl = 0;
                    }
                    ents[ln].flags = fl;
                    if i >= n {
                        break;
                    }
                } else if c == b'\\' as u16 && i + 1 < n && w[i + 1] == b'(' as u16 {
                    i += 1;
                }
            } else if col >= COLS {
                i += 1;
                continue;
            }
            let c = w[i];
            if c == b'\\' as u16 && i + 1 < n && b"()[]^-.".iter().any(|&x| w[i + 1] == x as u16) {
                if col + 1 < COLS {
                    ents[ln].esc[col] = true;
                }
                i += 1;
                continue;
            }
            // Full-width [ ] ^ - . are plain characters.
            if matches!(c, 0xff3b | 0xff3d | 0xff3e | 0xff0d | 0xff0e) {
                ents[ln].esc[col] = true;
            }
            let e = &mut ents[ln];
            if bad {
                e.regex = false;
            } else {
                let plain = e.esc[col];
                let prev = if i > 0 { w[i - 1] } else { 0 };
                let next = w.get(i + 1).copied().unwrap_or(0);
                let edges = col + 1 < COLS && i > 0 && i + 1 < n;
                let ok = if is(c, plain, b'[') {
                    e.regex = false;
                    open += 1;
                    i + 1 < n && next != b']' as u16
                } else if is(c, plain, b']') {
                    close += 1;
                    if open == close && col + 1 < COLS {
                        e.regex = true;
                    }
                    open >= close
                } else if is(c, plain, b'-') {
                    edges && col > 0 && !special(e.pat[col - 1], e.esc[col - 1]) && prev <= next
                } else if is(c, plain, b'^') {
                    edges && prev == b'[' as u16 && (next == b'\\' as u16 || !special(next, false))
                } else if is(c, plain, b'.') {
                    edges && prev == b'[' as u16 && next == b']' as u16
                } else {
                    true
                };
                if !ok {
                    bad = true;
                    e.regex = false;
                }
            }
            if e.flags & 2 != 0 && is_space(c) {
                e.pat[col] = 0;
            } else {
                e.pat[col] = if e.flags & 4 != 0 && foldable(c) { fold(c) } else { c };
                if col + 1 < COLS {
                    e.pat[col + 1] = 0;
                } else {
                    e.pat[col] = 0;
                }
                col += 1;
            }
            i += 1;
        }
        ln + 1
    };
    ents.truncate(count);
    // Folded patterns: the full-width [ ] ^ - . that folding made go back to ASCII, up to the first plain mark.
    for e in ents.iter_mut().filter(|e| e.regex && e.flags & 4 != 0) {
        for k in 0..COLS {
            if e.pat[k] == 0 || e.esc[k] {
                break;
            }
            e.pat[k] = match e.pat[k] {
                0xff3b => 0x5b,
                0xff3d => 0x5d,
                0xff3e => 0x5e,
                0xff0d => 0x2d,
                0xff0e => 0x2e,
                x => x,
            };
        }
    }
    ents
}

/// FUN_101bd510: the pattern anywhere in `s` (its jumps kept as states).
fn pattern_find(s: &[u16], e: &Entry) -> bool {
    #[derive(Clone, Copy)]
    enum L {
        Start,
        Next,
        Bl,
        C1,
        C8,
        Cf,
        D6,
        Df,
        Fail,
        Skip,
        Done,
    }
    let tc = |i: usize| s.get(i).copied().unwrap_or(0);
    if tc(0) == 0 || e.pat[0] == 0 {
        return false;
    }
    let mut t0 = 0usize;
    loop {
        let mut t = t0;
        let mut p: isize = 0;
        let (mut in_br, mut ok, mut neg, mut bl) = (false, false, false, false);
        let (mut open, mut close) = (0u32, 0u32);
        let mut at = L::Start;
        loop {
            at = match at {
                // 0x101bd565
                L::Start => {
                    (in_br, ok, neg, open, close) = (false, false, false, 0, 0);
                    if e.pc(p) == 0 { L::Done } else { L::Next }
                }
                // 0x101bd588
                L::Next => {
                    bl = false;
                    if !in_br {
                        (open, close, neg) = (0, 0, false);
                    }
                    let (c, pl) = (e.pc(p), e.ec(p));
                    let ch = tc(t);
                    if is(c, pl, b'[') {
                        in_br = true;
                        open += 1;
                        bl = true;
                        L::C1
                    } else if is(c, pl, b']') {
                        close += 1;
                        bl = true;
                        if open != close {
                            L::C1
                        } else {
                            in_br = false;
                            if !ok || neg { L::Done } else { L::C1 }
                        }
                    } else if !in_br {
                        // 0x101bd748
                        if ch != c {
                            L::C1
                        } else {
                            ok = true;
                            L::C8
                        }
                    } else if is(c, pl, b'^') {
                        neg = true;
                        L::Bl
                    } else if is(c, pl, b'-') {
                        let (lo, hi) = (e.pc(p - 1), e.pc(p + 1));
                        if neg {
                            if ch < lo {
                                ok = true;
                                L::Df
                            } else if hi >= ch {
                                L::Fail
                            } else {
                                ok = true;
                                L::Df
                            }
                        } else if lo > ch || ch > hi {
                            L::Bl
                        } else {
                            ok = true;
                            L::Cf
                        }
                    } else if is(e.pc(p + 1), e.ec(p + 1), b'-') || is(e.pc(p - 1), e.ec(p - 1), b'-') {
                        L::Bl
                    } else if neg {
                        if ch == c {
                            L::Fail
                        } else {
                            ok = true;
                            L::Df
                        }
                    } else if is(c, pl, b'.') || ch == c {
                        ok = true;
                        L::Cf
                    } else {
                        L::Bl
                    }
                }
                L::Bl => {
                    bl = true;
                    L::C1
                }
                L::C1 => if ok { L::C8 } else { L::D6 },
                L::C8 => if in_br { L::Cf } else { L::D6 },
                L::Cf => if neg { L::Df } else { L::D6 },
                L::D6 => if bl { L::Df } else { L::Skip },
                L::Df => {
                    p += 1;
                    if e.pc(p) != 0 { L::Next } else { L::Done }
                }
                L::Fail => {
                    ok = false;
                    L::Done
                }
                // 0x101bd769: a hit inside brackets skips to their close.
                L::Skip => {
                    if ok && in_br && e.pc(p) != 0 {
                        loop {
                            p += 1;
                            if is(e.pc(p), e.ec(p), b'[') {
                                open += 1;
                            } else if is(e.pc(p), e.ec(p), b']') {
                                close += 1;
                                if open == close {
                                    break;
                                }
                            }
                            if e.pc(p) == 0 {
                                break;
                            }
                        }
                    }
                    L::Done
                }
                // 0x101bd7c8
                L::Done => {
                    p += 1;
                    if !ok {
                        break;
                    }
                    if e.pc(p) == 0 {
                        return true;
                    }
                    t += 1;
                    if tc(t) == 0 {
                        break;
                    }
                    L::Start
                }
            };
        }
        t0 += 1;
        if tc(t0) == 0 {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(prohibited: &str, allowed: &str) -> SpamFilter {
        let mut f = SpamFilter::default();
        f.prohibited = Some(parse(prohibited.as_bytes(), &mut f.any_flags));
        f.allowed = Some(parse(allowed.as_bytes(), &mut f.any_flags));
        f
    }

    /// Results of retail's FUN_101bd820 / FUN_101bcf80 for the same files and lines, run under an x86 emulator.
    #[test]
    fn matches_retail() {
        type Set<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str, u8)]);
        let sets: &[Set] = &[
            (
                "gil\n(i)buy\n(s)w w w\n(n)Bob",
                "",
                &[("Ann", "cheap gil", 1), ("Ann", "BUY NOW", 1), ("Ann", "b u y", 0), ("Ann", "visit www.x", 1), ("Bob", "hi", 0), ("gilda", "hi", 1), ("Ann", "Gil", 0)],
            ),
            (
                "(is)ゴールド\n[a-c]x\n[^0-9]z9\n(i)[.]q\nr[ab]s\n\\[t\\]",
                "",
                &[
                    ("Ann", "ｺﾞｰﾙﾄﾞ", 0),
                    ("Ann", "ゴ ー ル ド", 1),
                    ("Ann", "bx", 1),
                    ("Ann", "dx", 0),
                    ("Ann", "az9", 1),
                    ("Ann", "5z9", 0),
                    ("Ann", "!Q", 1),
                    ("Ann", "ras", 1),
                    ("Ann", "rcs", 0),
                    ("Ann", "[t]", 1),
                    ("Ann", "t", 0),
                ],
            ),
            ("(i)rmt\nsell", "(i)sell[ ]?\nmog", &[("Ann", "RMT here", 1), ("Ann", "sell items", 1), ("Ann", "sell", 1), ("Ann", "mog sell", 2)]),
            ("[ab\na-]\n[]x\n^y", "", &[("Ann", "[ab", 1), ("Ann", "a-]", 1), ("Ann", "[]x", 1), ("Ann", "^y", 1), ("Ann", "b", 0)]),
        ];
        for (p, a, cases) in sets {
            let f = filter(p, a);
            for (n, m, want) in *cases {
                assert_eq!(f.check(n, m), *want, "{p:?} / {a:?}: {n} {m:?}");
            }
        }
    }

    #[test]
    fn plain_text_limit() {
        assert_eq!(plain("a\u{1}b\u{E008}c"), "abc");
        // A colour code inside a word (decode_sjis keeps 1e xx as one character) does not split it.
        let shout = crate::dat::font::decode_sjis(b"g\x1e\x02il");
        assert_eq!(plain(&shout), "gil");
        assert_eq!(filter("gil", "").check("Ann", &plain(&shout)), 1);
        assert_eq!(plain(&"x".repeat(200)).len(), 150);
        assert_eq!(plain(&"ゴ".repeat(100)).chars().count(), 75);
    }
}
