//! Where FFXiMain.dll keeps the data tables the client reads. Their addresses move with every build, so each is
//! found by content when the DLL is read: a short hint, confirmed by an MD5 over the table (or its stable start;
//! SE appends to some). Tables at a fixed distance from a found one in every known build are placed from it.
//! Signatures from the 2019 base, checked against retail 30261006_0 (lab/current/signatures.py).

use anyhow::{Result, anyhow, bail};
use md5::{Digest, Md5};

struct Sig {
    name: &'static str,
    hint: &'static [u8],
    len: usize,
    md5: &'static str,
}

const KEY1: Sig = Sig { name: "model key 1", hint: &[0xe2, 0xe5, 0x06, 0xa9, 0xed, 0x26, 0xf4, 0x42], len: 0x100, md5: "0bfa75c0e9ff6ff8497b370024b3b148" };
const KEY2: Sig = Sig { name: "model key 2", hint: &[0xb8, 0xc5, 0xf7, 0x84, 0xe4, 0x5a, 0x23, 0x7b], len: 0x100, md5: "44b34e48298e775a58552e7d9a8f931c" };
const WIDTHS: Sig = Sig {
    name: "glyph widths",
    hint: &[0x07, 0x07, 0x07, 0x0a, 0x08, 0x0d, 0x0b, 0x05, 0x08, 0x08, 0x09, 0x09, 0x05, 0x08, 0x05, 0x07],
    len: 0x440,
    md5: "592a3523505144cc5dd1314f41421fca",
};
const PNG_FONTS: Sig = Sig { name: "PNG fonts", hint: &[0x77, 0x9a, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00], len: 0x18, md5: "29ad3dbe3499efcd1c150f200ee8502b" };
/// Language file-id tables: ja, en, fr, de (FUN_10250de0 / retail FUN_1025b510).
const LANGS: [Sig; 4] = [
    Sig { name: "language ja", hint: &[0x6c, 0x9a, 0x00, 0x00, 0x6d, 0x9a, 0x00, 0x00], len: 0x20, md5: "453ac09a342bba80b59b6497c86dafbf" },
    Sig { name: "language en", hint: &[0x76, 0x9a, 0x00, 0x00, 0x75, 0x9a, 0x00, 0x00], len: 0x20, md5: "6546f5a771b7cef7fe0399f37cd02ad8" },
    Sig { name: "language fr", hint: &[0xc2, 0xd9, 0x00, 0x00, 0xc1, 0xd9, 0x00, 0x00], len: 0x20, md5: "3d0a638cd18612bc1cd79118c95da551" },
    Sig { name: "language de", hint: &[0x66, 0xdb, 0x00, 0x00, 0x65, 0xdb, 0x00, 0x00], len: 0x20, md5: "8fff1aa6789f5f74960df9aef2236e95" },
];
/// The lowest of the seven battle mode tables (FUN_1009cda0's second).
const MODES: Sig = Sig { name: "battle modes", hint: &[0xbf, 0x14, 0x14, 0x32, 0x7a, 0x7a, 0x24, 0x17], len: 0x40, md5: "eef9775da232dbd97559849b5dae6517" };
const FILTER_KINDS: Sig = Sig { name: "filter kinds", hint: &[0x00, 0x09, 0x0d, 0x0c, 0x2d, 0x2d, 0x01, 0x12], len: 0x40, md5: "6b4b3991e00bfb97c1174c602e5a737c" };
/// FUN_100ad8f0's on-screen kind per message (0 none, 1 damage, 2 recovery, 3 miss); 1024 bytes, SE appends.
const SCREEN_KINDS: Sig = Sig {
    name: "screen kinds",
    hint: &[0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x03],
    len: 0x40,
    md5: "0ea9b21e9a7f04a68aeb660b0e0c2244",
};
/// DAT_1035bc60 rows 1..7 (row 0 is zeros): knockback speed per tick, decay, constant ticks by level (f32 each).
const KNOCKBACK: Sig = Sig { name: "knockback levels", hint: &[0xab, 0xaa, 0xaa, 0x3d, 0x9a, 0x99, 0x99, 0x3d, 0x00, 0x00, 0xa0, 0x40], len: 84, md5: "098219a788fd1677fe255d3c7e8c95bf" };
/// DAT_1036eba0: the chat call tags, 0x14-byte rows {name[16], u32 SeSep name (byte-swapped), bits 0x800000 /
/// 0x400000 = <ncall> handling} to a row with sound 0.
const CALLS: Sig = Sig { name: "chat calls", hint: b"<call>\0", len: 0x40, md5: "806a8a20b9c977474e5c522233d52ec0" };
const MODE_COLOURS: Sig = Sig { name: "mode colours", hint: &[0x01, 0x00, 0x54, 0x01, 0x02, 0x00, 0x55, 0x01], len: 0x40, md5: "8dcab7880389b2c4c488afb259a5f760" };
const PALETTE: Sig = Sig { name: "palette", hint: &[0x42, 0x00, 0x00, 0x00, 0x80, 0x80, 0x80, 0x60], len: 0x40, md5: "ada7882381d8a7f871792f7f2bc56e63" };
const BINDINGS: Sig = Sig { name: "key bindings", hint: &[0xc8, 0x00, 0x00, 0x00, 0x00, 0x00, 0xd0, 0x00], len: 0x100, md5: "2979f885eeb95c7f5344400b09026857" };
const MASKS: Sig = Sig {
    name: "key masks",
    hint: &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x08, 0x00, 0x08, 0x00, 0x10, 0x00, 0x10, 0x00],
    len: 0x100,
    md5: "d989b989b0e0402124e6af0708690147",
};
const COMPACT: Sig = Sig { name: "compact keys", hint: &[0x11, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00], len: 0x100, md5: "dff8338e5664b470f57bfca986a1ee13" };
const K1: Sig = Sig { name: "k1 keys", hint: &[0x00, 0xff, 0xff, 0xff, 0x01, 0xff, 0xff, 0xff], len: 0x40, md5: "ac0040622246350502b3cdd9e800f821" };
const K2: Sig = Sig { name: "k2 keys", hint: &[0x3b, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], len: 0x80, md5: "1599eaca5b50a3f4185ba10b03444576" };
const STATS: Sig = Sig {
    name: "creator stats",
    hint: &[0x08, 0x07, 0x06, 0x07, 0x05, 0x05, 0x06, 0x07, 0x07, 0x08, 0x05, 0x05, 0x06, 0x06, 0x06, 0x05],
    len: 0x150,
    md5: "6be547df7eeb83be91e4758f55962e46",
};
const CMD_HELP: Sig = Sig {
    name: "command help",
    hint: &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
    len: 0x10,
    md5: "645ded257e5c15cd4d8c973d31128021",
};
const ERRORS: Sig = Sig { name: "error codes", hint: &[0x34, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], len: 0x40, md5: "3e6e09a7959e57630975fc953b865925" };
/// "No. of content IDs purchased: %2d"; the other three formats follow at -0x24 each.
const COUNT_FMT: Sig = Sig { name: "count formats", hint: b"No. of conte", len: 0x18, md5: "2e622d8e177bbaaf8d9aee67475441ae" };
const REGIONS: Sig = Sig { name: "map regions", hint: &[0x64, 0x00, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00], len: 0x380, md5: "8ec88ae948b702b4e5c330f71ad73f32" };

/// Every table's place: file offsets (`*_at`) or VAs.
#[derive(Debug, Clone)]
pub struct Tables {
    pub key1_at: usize,
    pub key2_at: usize,
    pub widths_at: usize,
    pub png_fonts_at: usize,
    pub lang_at: [usize; 4],
    pub mode_tables: [u32; 7],
    pub filter_kinds: u32,
    pub filter_tables: [u32; 11],
    pub screen_kinds: u32,
    /// File offset of the knockback level table's row 1.
    pub knockback_at: usize,
    pub calls_at: usize,
    pub mode_colours: u32,
    pub palette: u32,
    pub bindings: u32,
    pub masks: u32,
    pub compact: u32,
    pub k1: u32,
    pub k2: u32,
    pub stats: u32,
    pub cmd_help: u32,
    pub errors: u32,
    pub count_fmt: [u32; 4],
    pub regions: u32,
    /// The 2019 base's language slot numbering (identity there; SE inserted slots since).
    base_slots: bool,
}

/// File offset of the first place `sig` matches.
fn find(dll: &[u8], sig: &Sig) -> Result<usize> {
    let mut start = 0;
    while let Some(i) = dll.get(start..).and_then(|s| s.windows(sig.hint.len()).position(|w| w == sig.hint)) {
        let at = start + i;
        if dll.get(at..at + sig.len).is_some_and(|t| hex::encode(Md5::digest(t)) == sig.md5) {
            return Ok(at);
        }
        start = at + 1;
    }
    Err(anyhow!("FFXiMain.dll: no {} table (an unknown build?)", sig.name))
}

/// The VA of a file offset (data sections).
pub fn va_of(dll: &[u8], off: usize) -> Option<u32> {
    let u16_at = |o: usize| dll.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| dll.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let nt = u32_at(0x3c)? as usize;
    let (count, opt) = (u16_at(nt + 6)?, u16_at(nt + 20)?);
    let base = u32_at(nt + 24 + 28)?;
    (0..count).find_map(|i| {
        let s = nt + 24 + opt + i * 40;
        let (sva, rsize, raw) = (u32_at(s + 12)?, u32_at(s + 16)? as usize, u32_at(s + 20)? as usize);
        (raw <= off && off < raw + rsize).then(|| base + sva + (off - raw) as u32)
    })
}

/// The tables of this DLL, found once per build (keyed by its length and a few sampled bytes).
pub fn of(dll: &[u8]) -> Result<Tables> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Vec<((usize, u64), Tables)>>> = OnceLock::new();
    let step = (dll.len() / 61).max(1);
    let key = (dll.len(), (0..61).fold(0u64, |h, i| h.wrapping_mul(1_000_003).wrapping_add(*dll.get(i * step).unwrap_or(&0) as u64)));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().unwrap().iter().find(|e| e.0 == key) {
        return Ok(t.1.clone());
    }
    let t = Tables::find(dll)?;
    cache.lock().unwrap().push((key, t.clone()));
    Ok(t)
}

/// FUN_10127990's call tags as (tag, UI sound number): "<call>" 17 .. "<call21>" 38. The <ncall> rows (flag 0x800000,
/// FUN_10127ab0 first) are left out.
pub fn call_sounds(dll: &[u8]) -> Result<Vec<(String, u32)>> {
    let at = of(dll)?.calls_at;
    let mut out = Vec::new();
    for row in dll.get(at..).unwrap_or_default().chunks_exact(0x14) {
        let snd = u32::from_le_bytes(row[16..20].try_into()?);
        if snd == 0 {
            break;
        }
        let name = String::from_utf8_lossy(row[..16].split(|&b| b == 0).next().unwrap_or_default()).into_owned();
        if snd & 0xc0_0000 == 0
            && let Ok(n) = std::str::from_utf8(&snd.to_be_bytes()).unwrap_or("").parse::<u32>()
        {
            out.push((name, n));
        }
    }
    Ok(out)
}

/// FUN_100aa8e0's table: (speed per tick, decay, constant-phase ticks) for knockback levels 0..7.
pub fn knockback(dll: &[u8]) -> Result<[[f32; 3]; 8]> {
    let at = of(dll)?.knockback_at;
    let mut t = [[0.0; 3]; 8];
    for (k, row) in t.iter_mut().enumerate().skip(1) {
        for (j, v) in row.iter_mut().enumerate() {
            let o = at + ((k - 1) * 3 + j) * 4;
            *v = f32::from_le_bytes(dll[o..o + 4].try_into()?);
        }
    }
    Ok(t)
}

/// The file offset of a VA (data sections).
fn off_of(dll: &[u8], va: u32) -> Option<usize> {
    let u16_at = |o: usize| dll.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| dll.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let nt = u32_at(0x3c)? as usize;
    let (count, opt) = (u16_at(nt + 6)?, u16_at(nt + 20)?);
    let base = u32_at(nt + 24 + 28)?;
    (0..count).find_map(|i| {
        let s = nt + 24 + opt + i * 40;
        let (sva, rsize, raw) = (u32_at(s + 12)?, u32_at(s + 16)?, u32_at(s + 20)? as usize);
        let rel = va.checked_sub(base + sva)?;
        (rel < rsize).then_some(raw + rel as usize)
    })
}

fn u32_le(dll: &[u8], at: usize) -> Option<u32> {
    dll.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// The C string at a VA.
fn cstr_at(dll: &[u8], va: u32) -> Option<String> {
    let o = off_of(dll, va)?;
    let end = o + dll.get(o..)?.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&dll[o..end]).into_owned())
}

/// The file offset of the `{i32 value, char *word}` pair for (value, word).
fn find_pair(dll: &[u8], value: i32, word: &str) -> Result<usize> {
    let needle: Vec<u8> = [&[0u8][..], word.as_bytes(), &[0]].concat();
    let mut start = 0;
    while let Some(i) = dll.get(start..).and_then(|s| s.windows(needle.len()).position(|w| w == needle)) {
        let s = start + i + 1;
        if let Some(va) = va_of(dll, s) {
            let pat: Vec<u8> = [value.to_le_bytes(), va.to_le_bytes()].concat();
            if let Some(p) = dll.chunks_exact(4).enumerate().find_map(|(k, _)| (dll.get(k * 4..k * 4 + 8) == Some(&pat[..])).then_some(k * 4)) {
                return Ok(p);
            }
        }
        start = s;
    }
    Err(anyhow!("FFXiMain.dll: no word table with {word} (an unknown build?)"))
}

/// A `{i32 value, char *word}` list from `at` to its NULL word, and the offset after it.
fn word_list(dll: &[u8], mut at: usize) -> (Vec<(i32, String)>, usize) {
    let mut out = Vec::new();
    while let (Some(v), Some(p)) = (u32_le(dll, at), u32_le(dll, at + 4)) {
        at += 8;
        if p == 0 {
            break;
        }
        out.push((v as i32, cstr_at(dll, p).unwrap_or_default()));
    }
    (out, at)
}

/// FUN_10104f00's words ({value, word} lists): the categories (0x10331cf0), the UNITY keyword, areas, nations,
/// races, jobs and status words in a row after them; the unity leaders (0x10364880) elsewhere.
#[derive(Clone, Debug, Default)]
pub struct SearchWords {
    pub categories: Vec<(i32, String)>,
    pub unity_word: Vec<(i32, String)>,
    pub areas: Vec<(i32, String)>,
    pub nations: Vec<(i32, String)>,
    pub races: Vec<(i32, String)>,
    pub jobs: Vec<(i32, String)>,
    pub statuses: Vec<(i32, String)>,
    pub unity: Vec<(i32, String)>,
    /// FUN_1020a3e0's regions (0x32 lists of zone ids at 0x103858a8; entry 30 is filled at run time: empty here).
    pub regions: Vec<Vec<u16>>,
    /// DAT_103669b8: 4 bits per zone below 300 (FUN_101070f0), 2 = searchable.
    pub zone_flags: Vec<u8>,
    /// The mixed-case unity names (0x103648e8, right after the upper-case list; FUN_10108110), keyed by bits.
    pub unity_names: Vec<(i32, String)>,
    /// DAT_1033b430 (FUN_1020e790): (zone, tier, min level, max level) rows, ended by zone 0xffff.
    pub levels: Vec<(u16, u8, u8, u8)>,
}

/// The level table's first four rows (unique in the image).
const LEVELS: [u8; 24] = [5, 0, 2, 0x3a, 0x5a, 0, 0x18, 0, 2, 0x1f, 0, 0, 0x19, 0, 2, 0x1f, 0, 0, 0x21, 0, 2, 0x44, 0, 0];

const ZONE_FLAGS: Sig = Sig {
    name: "zone flags",
    hint: &[0x70, 0x7f, 0xff, 0xf1, 0xf1, 0xf1, 0x1f, 0xf5, 0x1f, 0x1f, 0x1f, 0x1f, 0xff, 0xff, 0xff, 0x1f],
    len: 0x40,
    md5: "7569110eabc4e9d7cd70ab597efacdbf",
};

pub fn search_words(dll: &[u8]) -> Result<SearchWords> {
    let mut w = SearchWords::default();
    let (cats, at) = word_list(dll, find_pair(dll, 0, "NAME")?);
    let lists: [&mut Vec<(i32, String)>; 6] = [&mut w.unity_word, &mut w.areas, &mut w.nations, &mut w.races, &mut w.jobs, &mut w.statuses];
    let mut at = at;
    for l in lists {
        (*l, at) = word_list(dll, at);
    }
    w.categories = cats;
    if w.areas.first().map(|a| a.1.as_str()) != Some("ALL") {
        return Err(anyhow!("FFXiMain.dll: the search word lists moved"));
    }
    let (unity, after) = word_list(dll, find_pair(dll, 0x40_0000, "PIEUJE")? - 8);
    (w.unity, w.unity_names) = (unity, word_list(dll, after).0);
    if let Some(mut at) = dll.windows(LEVELS.len()).position(|x| x == LEVELS) {
        while let Some(r) = dll.get(at..at + 6).filter(|r| r[..2] != [0xff, 0xff]) {
            w.levels.push((u16::from_le_bytes([r[0], r[1]]), r[2], r[3], r[4]));
            at += 6;
        }
    }
    // The region pointer array: its first list is {230..233}.
    let first: Vec<u8> = [230u16, 231, 232, 233, 0].iter().flat_map(|z| z.to_le_bytes()).collect();
    let list_va = dll.windows(first.len()).position(|x| x == first).and_then(|o| va_of(dll, o)).ok_or_else(|| anyhow!("FFXiMain.dll: no region lists"))?;
    let arr = dll.chunks_exact(4).position(|c| c == list_va.to_le_bytes()).ok_or_else(|| anyhow!("FFXiMain.dll: no region array"))? * 4;
    for k in 0..0x32 {
        let mut zones = Vec::new();
        if let Some(mut o) = u32_le(dll, arr + 4 * k).and_then(|p| off_of(dll, p)) {
            while let Some(z) = dll.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).filter(|&z| z != 0) {
                zones.push(z);
                o += 2;
            }
        }
        w.regions.push(zones);
    }
    let flags = find(dll, &ZONE_FLAGS)?;
    w.zone_flags = dll.get(flags..flags + 152).unwrap_or_default().to_vec();
    Ok(w)
}

impl SearchWords {
    /// FUN_101070f0.
    pub fn zone_flags(&self, zone: u16) -> u8 {
        if zone >= 300 {
            return 0;
        }
        let k = (zone as usize >> 3) * 4;
        let word = self.zone_flags.get(k..k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        (word >> ((zone & 7) * 4) & 0xf) as u8
    }

    /// FUN_1020c1c0: the searchable zones (flag 2) of the zone's region, at most 16.
    pub fn region_of(&self, zone: u16) -> Option<Vec<u16>> {
        let r = self.regions.iter().find(|r| r.contains(&zone))?;
        Some(r.iter().copied().filter(|&z| self.zone_flags(z) & 2 != 0).take(16).collect())
    }
}

impl Tables {
    pub fn find(dll: &[u8]) -> Result<Self> {
        let va = |s: &Sig| -> Result<u32> { find(dll, s).and_then(|o| va_of(dll, o).ok_or_else(|| anyhow!("{}: not in a section", s.name))) };
        let key2_at = find(dll, &KEY2)?;
        // Model key 1 sits 0x328 before key 2 in every known build; another copy of it exists further on.
        let at_key1 = |o: usize| dll.get(o..o + KEY1.len).is_some_and(|t| hex::encode(Md5::digest(t)) == KEY1.md5);
        let key1_at = key2_at.checked_sub(0x328).filter(|&o| at_key1(o)).map_or_else(|| find(dll, &KEY1), Ok)?;
        let low = va(&MODES)?;
        // FUN_1009cda0's order: the second table is the lowest, then the first, then the rest, 0x400 apart.
        let mode_tables = [low + 0x400, low, low + 0x800, low + 0xc00, low + 0x1000, low + 0x1400, low + 0x1800];
        let filter_kinds = va(&FILTER_KINDS)?;
        let count = va(&COUNT_FMT)?;
        Ok(Self {
            key1_at,
            key2_at,
            widths_at: find(dll, &WIDTHS)?,
            png_fonts_at: find(dll, &PNG_FONTS)?,
            lang_at: [find(dll, &LANGS[0])?, find(dll, &LANGS[1])?, find(dll, &LANGS[2])?, find(dll, &LANGS[3])?],
            mode_tables,
            filter_kinds,
            // Eleven 49-word filter tables follow the kinds 0x400 on (the pointer array at 0x10351bbc lists them).
            filter_tables: std::array::from_fn(|i| filter_kinds + 0x400 + i as u32 * 200),
            screen_kinds: va(&SCREEN_KINDS)?,
            knockback_at: find(dll, &KNOCKBACK)?,
            calls_at: find(dll, &CALLS)?,
            mode_colours: va(&MODE_COLOURS)?,
            palette: va(&PALETTE)?,
            bindings: va(&BINDINGS)?,
            masks: va(&MASKS)?,
            compact: va(&COMPACT)?,
            k1: va(&K1)?,
            k2: va(&K2)?,
            stats: va(&STATS)?,
            cmd_help: va(&CMD_HELP)?,
            errors: va(&ERRORS)?,
            count_fmt: [count, count - 0x24, count - 0x48, count - 0x6c],
            regions: va(&REGIONS)?,
            base_slots: dll.len() == 2_869_840,
        })
    }

    /// A language slot numbered as in the 2019 base (FUN_10250e50(slot)) for this build. Retail since inserted
    /// slots: from the file ids of both tables, checked against retail FUN_1025b580's callers (0x3b -> 0x3d,
    /// 0x57 -> 0x5a, 0x5a -> 0x5d, 0x5c -> 0x5f).
    pub fn slot(&self, base: usize) -> usize {
        if self.base_slots {
            return base;
        }
        match base {
            0..0x20 => base,
            0x20..0x29 => base + 1,
            0x29..0x55 => base + 2,
            0x55..0x97 => base + 3,
            _ => base + 4,
        }
    }
}

/// A conquest map marker (cnqframe's 0x20-byte entries, menu_rows2_spec R3) with its 8-byte record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CnqMarker {
    /// +0: 1 selected, 4 blink, 8 no label, 0x10 not centred, 0x20 hidden unless selected, 0x40 label by place code,
    /// 0x100 second icon first, 0x200 plain main icon, 0x400 / 0x2000 fading out, 0x800 not pickable, 0x1000 fades out.
    pub flags: u16,
    /// +2 / +3 icons (indices into the icon set), +5 alpha.
    pub icon: u8,
    pub icon2: u8,
    pub alpha: u8,
    /// +6 / +8 map pixel, +0xa / +0xc label offset.
    pub px: i16,
    pub py: i16,
    pub dx: i16,
    pub dy: i16,
    /// +0xf the label place code (not Japanese), +0x10 the label colour index.
    pub place: u8,
    pub colour: i16,
    /// The record: +0 id (region id; a zone's owning region), +2 -1 region / -2 campaign front / zone, +4 name id, +6
    /// mode, +7 (conquest: the region's first zone; campaign: the area index).
    pub id: i16,
    pub par: i16,
    pub name: i16,
    pub b7: u8,
}

/// cnqframe's tables (FUN_101e6fb0 / FUN_101e7980): per mode the markers (top, region and zone records in that order),
/// the counts (top, top + regions) and the default region; the mode 3 (region, zone) pairs at 0x1037f610.
#[derive(Clone, Debug, Default)]
pub struct CnqTables {
    pub modes: [Vec<CnqMarker>; 4],
    pub counts: [(usize, usize, i16); 4],
    pub pairs: Vec<(i16, i16)>,
}

/// Mode 0's first entry (San d'Oria; mode 2 starts with the same bytes 0x1960 further on).
const CNQ_FIRST: [u8; 0x12] = [0x40, 0, 4, 0, 0, 0, 0xb1, 0, 0xa2, 0, 0xd7, 0xff, 0xef, 0xff, 0, 0, 0, 0];
/// Offsets from mode 0's entry table (0x1037fcd0) in 30261006_0: the entry tables, then per mode the top, region and
/// zone record runs (FUN_101e6fb0) and the counts (FUN_101e7980: top, top + regions, total, default region).
const CNQ_ENTRIES: [isize; 4] = [0, 0x1520, 0x1960, 0x1ea0];
const CNQ_RUNS: [[(isize, usize); 3]; 4] = [
    [(-0x874, 4), (-0x800, 19), (-0x6a0, 146)],
    [(-0x854, 1), (-0x768, 5), (-0x210, 28)],
    [(-0x848, 8), (-0x740, 8), (-0x130, 26)],
    [(-0x808, 1), (-0x700, 8), (-0x60, 12)],
];
const CNQ_COUNTS: [(usize, usize, i16); 4] = [(4, 0x17, 4), (1, 6, 0x18), (8, 0x10, 0x21), (1, 9, 1)];
const CNQ_PAIRS: isize = -0x6c0;

/// The four marker tables, checked against their first records (ids 1 / 24 / 30 / 1, all regions).
pub fn conquest(dll: &[u8]) -> Result<CnqTables> {
    let i16_at = |o: usize| dll.get(o..o + 2).map(|b| i16::from_le_bytes([b[0], b[1]]));
    let base = (0..dll.len().saturating_sub(0x1960 + CNQ_FIRST.len()))
        .find(|&o| dll[o..o + CNQ_FIRST.len()] == CNQ_FIRST && dll[o + 0x1960..o + 0x1960 + CNQ_FIRST.len()] == CNQ_FIRST)
        .ok_or_else(|| anyhow!("FFXiMain.dll: no conquest marker tables"))?;
    let at = |d: isize| base.checked_add_signed(d).ok_or_else(|| anyhow!("conquest tables: offset out of range"));
    let mut t = CnqTables { counts: CNQ_COUNTS, ..Default::default() };
    for mode in 0..4 {
        let recs: Vec<usize> = CNQ_RUNS[mode].iter().map(|&(d, n)| at(d).map(|o| (0..n).map(move |k| o + 8 * k))).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect();
        let e0 = at(CNQ_ENTRIES[mode])?;
        for (k, r) in recs.into_iter().enumerate() {
            let e = e0 + 0x20 * k;
            let (Some(rec), Some(ent)) = (dll.get(r..r + 8), dll.get(e..e + 0x12)) else { bail!("conquest tables: short") };
            let s = |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
            t.modes[mode].push(CnqMarker {
                flags: u16::from_le_bytes([ent[0], ent[1]]),
                icon: ent[2],
                icon2: ent[3],
                alpha: ent[5],
                px: s(ent, 6),
                py: s(ent, 8),
                dx: s(ent, 0xa),
                dy: s(ent, 0xc),
                place: ent[0xf],
                colour: s(ent, 0x10),
                id: s(rec, 0),
                par: s(rec, 2),
                name: s(rec, 4),
                b7: rec[7],
            });
        }
    }
    let first: Vec<(i16, i16)> = t.modes.iter().map(|m| (m[0].id, m[0].par)).collect();
    if first != [(1, -1), (24, -1), (30, -1), (1, -1)] {
        bail!("FFXiMain.dll: the conquest tables moved ({first:?})");
    }
    let p = at(CNQ_PAIRS)?;
    t.pairs = (0..8).filter_map(|k| Some((i16_at(p + 4 * k)?, i16_at(p + 4 * k + 2)?))).collect();
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::Tables;

    /// The installed build's tables are found, and model key 1 is the copy next to key 2.
    #[test]
    fn found_in_the_installed_build() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let t = Tables::find(&dll).unwrap();
        assert_eq!(t.key2_at - t.key1_at, 0x328);
        assert_eq!(t.mode_tables[6] - t.mode_tables[1], 0x1800);
    }

    /// The /search word lists, regions and zone flags (search_spec.md 2.2, 3.6).
    #[test]
    fn search_words() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let w = super::search_words(&dll).unwrap();
        assert_eq!(w.categories.len(), 14);
        assert_eq!(w.categories[13], (6, "UNITY".into()));
        assert_eq!(w.unity_word, [(1, "UNITY".into())]);
        assert_eq!(w.areas.len(), 460);
        assert_eq!(w.areas[1], (1, "PHANAUET CHANNEL".into()));
        assert_eq!(w.nations[5], (2, "WIN".into()));
        assert_eq!(w.races.len(), 10);
        assert_eq!(w.jobs.last(), Some(&(22, "RUN".into())));
        assert_eq!(w.statuses[9], (0x2000, "*".into()));
        assert_eq!(w.unity[2], (0x80_0000, "AYAME".into()));
        assert_eq!(w.regions[7], [103, 108, 196, 102, 193, 248]);
        assert!(w.regions[30].is_empty());
        assert_eq!(w.region_of(230).map(|r| r.len()), Some(4));
        assert!(w.unity_names.contains(&(0x40_0000, "Pieuje".into())));
        assert_eq!((w.levels.len(), w.levels[0]), (60, (5, 2, 0x3a, 0x5a)));
        let capped: Vec<u16> = w.levels.iter().filter(|l| l.3 != 0).map(|l| l.0).collect();
        assert_eq!(capped, [5, 112, 118]);
    }

    /// cnqframe's markers (menu_rows2_spec R3, cnqentries.txt): counts, first records, a zone, a label place.
    #[test]
    fn conquest_markers() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let t = super::conquest(&dll).unwrap();
        assert_eq!(t.modes.iter().map(Vec::len).collect::<Vec<_>>(), [0xa9, 0x22, 0x2a, 0x15]);
        let ronfaure = t.modes[0][4];
        assert_eq!((ronfaure.id, ronfaure.par, ronfaure.name, ronfaure.px, ronfaure.py, ronfaure.place), (5, -1, 4, 177, 182, 3));
        assert_eq!((t.modes[0][23].id, t.modes[0][23].par), (1, 230));
        assert_eq!((t.modes[2][8].par, t.modes[2][8].flags), (-2, 0x840));
        assert_eq!(t.pairs[0], (2, 260));
    }
}
