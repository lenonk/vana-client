//! Where FFXiMain.dll keeps the data tables the client reads. Their addresses move with every build, so each is
//! found by content when the DLL is read: a short hint, confirmed by an MD5 over the table (or its stable start;
//! SE appends to some). Tables at a fixed distance from a found one in every known build are placed from it.
//! Signatures from the 2019 base, checked against retail 30261006_0 (lab/current/signatures.py).

use anyhow::{Result, anyhow};
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
}
