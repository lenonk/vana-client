//! The auto-translate dictionary (language slot 0x18; English file 55665) and the phrase blocks in text
//! (`FD kind lang hi lo FD`, kept by font::decode_sjis as four private-use chars). MODLOG "Auto-translate: the
//! dictionary" (loader FUN_101bb7a0, resolver FUN_101bc100, expander FUN_101ac990).

use super::DatIndex;
use super::font::{KIND_VARIABLE, PHRASE_MARK, decode_sjis, encode_sjis, is_phrase, raw_byte};
use super::items::Lang;
use super::msg::Messages;
use super::names::NameSource;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// One phrase group: its title (phrase id 0, "【Greetings】") and its phrases by id.
pub struct Group {
    pub id: u8,
    pub title: String,
    /// (phrase id, text, completion key) in file order (each entry's first word); `@X<hex>` texts stay raw when no
    /// name resolved them.
    pub phrases: Vec<(u8, String, String)>,
    /// The category's words (FUN_101bc090): its title words first, then the entries' words, and the order of the
    /// entries' words by key (the category's sorted array).
    words: Vec<Word>,
    sorted: Vec<usize>,
}

pub struct Dictionary {
    pub groups: Vec<Group>,
    by_id: HashMap<u8, usize>,
    /// Group 0x12 (job names, `@J`), group 0x13 (`@M`), group 0x14 or 0x15 (places, `@A`), group 0xb 0x91 .. 0x94
    /// (the item and key item groups' titles and keys).
    jobs: Option<Messages>,
    m_names: Option<Messages>,
    places: Option<Messages>,
    titles: [(String, String); 2],
    /// The groups built at run time (FUN_101bcd10), in slot order: Key Items (D+0x218), then Items (D+0x214). Empty
    /// until the root category list first opens.
    built: std::sync::RwLock<Vec<Built>>,
    /// Every category's words but the empty ones and a lone block, sorted by key (FUN_101bdaf0, D+0xc): what a typed
    /// word is looked up in. Made again with the built groups.
    merged: std::sync::RwLock<Vec<Word>>,
    /// The files' language at load: its compares (retail reads the global each time; ours does not change it).
    lang: Lang,
    /// Its language byte (the files' language + 1), which the built groups carry.
    lang_byte: u8,
}

/// A group built at run time: its header kind (7 items, 0x13 key items), title, completion key and words (block id,
/// text; a word's key is its text).
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub kind: u8,
    pub title: String,
    pub key: String,
    pub words: Vec<([u8; 4], String)>,
    list: Vec<Word>,
    titles: usize,
    sorted: Vec<usize>,
}

/// A word (FUN_101bdd10, 0x18 bytes): its text, the key completion compares, the four id bytes. `raw` / `key` hold the
/// bytes from the text's / key's start to the end of the string they lie in, so a compare that reads past `tlen` /
/// `klen` sees what retail's would (a French second title's key is the title from its third byte, `klen` four
/// short of the title).
#[derive(Clone, Debug, PartialEq)]
struct Word {
    text: String,
    raw: Vec<u8>,
    tlen: usize,
    key: Vec<u8>,
    klen: usize,
    id: [u8; 4],
}

impl Word {
    /// FUN_101bdd80: the key is the text.
    fn plain(raw: &[u8], tlen: usize, id: [u8; 4]) -> Word {
        Word { text: text_of(raw, tlen), raw: raw.to_vec(), tlen, key: raw.to_vec(), klen: tlen, id }
    }

    /// FUN_101bddc0: a key of its own.
    fn keyed(raw: &[u8], tlen: usize, key: &[u8], klen: usize, id: [u8; 4]) -> Word {
        Word { text: text_of(raw, tlen), raw: raw.to_vec(), tlen, key: key.to_vec(), klen, id }
    }

    fn key_text(&self) -> String {
        text_of(&self.key, self.klen)
    }
}

/// The text of `len` bytes (to a NUL within them).
fn text_of(raw: &[u8], len: usize) -> String {
    let b = &raw[..len.min(raw.len())];
    decode_sjis(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
}

/// Header kind 2 a phrase group.
const GROUP_KIND: u8 = 2;
/// The built groups' header kinds.
pub const KIND_ITEMS: u8 = 7;
pub const KIND_KEY_ITEMS: u8 = 0x13;
/// FUN_101b84b0's room for candidates (+0xec28 = 0x3fc): FUN_101bd430 lists one fewer, the typed word takes the last.
const ROOM: usize = 0x3fc;

/// The dictionary's language byte: the files' language + 1 (FUN_101b67e0 -> FUN_101bc590), which the groups it loads
/// carry and the built groups and their blocks are given (1 Japanese, 2 English, 3 German, 4 French).
pub fn lang_byte() -> u8 {
    super::lang::files() as u8 + 1
}

/// FUN_101c9e70's tables (FUN_101c9d70: letter - 0x40, 0x85 0x9f..0xbe - 0x7f, 0x85 0xbf..0xde - 0x9f, 0x85 0x5c ->
/// 0x40, 0x85 0x4c -> 0x41; the char is the entry + 0x40). DAT_1037e358 orders the accented letters among the plain
/// ones (the French title test); DAT_1037e3a0 takes the accents off (German and French word compares).
const FOLD_ORDER: [u8; 0x42] = *b"\x00\x08\x09\x0b\x0c\x11\x12\x13\x14\x19\x1a\x1b\x1c\x1d\x1f\x25\x28\x29\x2a\x2c\x2d\x32\x33\x34\x35\x37\x38\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x0a\x0d\x0e\x0f\x10\x15\x16\x17\x18\x00\x1e\x20\x21\x22\x23\x24\x00\x00\x2e\x2f\x30\x31\x36\x00\x2b\x26\x27";
const FOLD_ACCENTS: [u8; 0x42] = *b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f\x01\x01\x01\x01\x01\x01\x01\x03\x05\x05\x05\x05\x09\x09\x09\x09\x04\x0e\x0f\x0f\x0f\x0f\x0f\x1b\x0f\x15\x15\x15\x15\x13\x13\x13\x0f\x0f";

fn byte(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// FUN_101c9d70: the folded char at `i` and its byte count.
fn fold(t: &[u8; 0x42], s: &[u8], i: usize) -> (u8, usize) {
    let b = byte(s, i);
    match b {
        0x41..=0x5a => (t[(b - 0x40) as usize] + 0x40, 1),
        0x61..=0x7a => (t[(b - 0x60) as usize] + 0x40, 1),
        0x85 => match byte(s, i + 1) {
            c @ 0x9f..=0xbe => (t[(c - 0x7f) as usize] + 0x40, 2),
            c @ 0xbf..=0xde => (t[(c - 0x9f) as usize] + 0x40, 2),
            0x5c => (t[0x40] + 0x40, 2),
            0x4c => (t[0x41] + 0x40, 2),
            _ => (b, 1),
        },
        _ => (b, 1),
    }
}

/// FUN_101c9e70: `n` bytes of `a` against `b` char by char through the table; the signed difference of the first chars
/// that differ (each string steps by its own char's bytes).
fn fold_cmp(a: &[u8], b: &[u8], n: usize, t: &[u8; 0x42]) -> i32 {
    let (mut i, mut j) = (0, 0);
    while i < n {
        let (x, k) = fold(t, a, i);
        let (y, l) = fold(t, b, j);
        (i, j) = (i + k, j + l);
        if x != y {
            return x as i8 as i32 - y as i8 as i32;
        }
    }
    0
}

/// _strncmp.
fn strncmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let (x, y) = (byte(a, i), byte(b, i));
        if x != y || x == 0 {
            return x as i32 - y as i32;
        }
    }
    0
}

/// FUN_10327340 (_strnicmp in the C locale): A..Z as a..z.
fn strnicmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let (x, y) = (byte(a, i).to_ascii_lowercase(), byte(b, i).to_ascii_lowercase());
        if x != y || x == 0 {
            return x as i32 - y as i32;
        }
    }
    0
}

/// The compare of the files' language (FUN_101bde00 for keys, FUN_101bded0 for texts): Japanese _strncmp, English
/// _strnicmp, German and French without accents (FUN_101c9e70, DAT_1037e3a0).
fn lang_cmp(lang: Lang, a: &[u8], b: &[u8], n: usize) -> i32 {
    match lang {
        Lang::Ja => strncmp(a, b, n),
        Lang::En => strnicmp(a, b, n),
        Lang::De | Lang::Fr => fold_cmp(a, b, n, &FOLD_ACCENTS),
    }
}

/// The words' order (vtable slot 0, FUN_101bdeb0 -> FUN_101bde80 -> FUN_101bde00): `w` before `m` when its key is less
/// than m's over m's key length.
fn before(lang: Lang, w: &Word, m: &Word) -> bool {
    lang_cmp(lang, &w.key, &m.key, m.klen) < 0
}

/// The selection sort of FUN_101bc090 / FUN_101bdaf0 (not stable: equal keys end up as its swaps leave them).
fn select_sort<T>(v: &mut [T], less: impl Fn(&T, &T) -> bool) {
    for i in 0..v.len() {
        let mut m = i;
        for j in i + 1..v.len() {
            if less(&v[j], &v[m]) {
                m = j;
            }
        }
        if m != i {
            v.swap(i, m);
        }
    }
}

/// FUN_10191360 on the typed word: hiragana (82 9f..f1) as katakana (83 40..96), the kana readings' script.
fn kana(b: &mut [u8]) {
    let mut i = 0;
    while i < b.len() {
        let n = super::font::char_len(b[i]).max(1);
        if n == 2 && b[i] == 0x82 && i + 1 < b.len() {
            match b[i + 1] {
                c @ 0x9f..=0xdd => (b[i], b[i + 1]) = (0x83, c - 0x5f),
                c @ 0xde..=0xf1 => (b[i], b[i + 1]) = (0x83, c - 0x5e),
                _ => {}
            }
        }
        i += n;
    }
}

/// FUN_101bd230: the run of `words` whose keys start with the typed bytes (the compare over the typed length), by
/// retail's binary search and its walk out from the hit: (first, count), count 0 for none.
fn search(lang: Lang, words: &[&Word], typed: &[u8]) -> (usize, usize) {
    let n = words.len() as i64;
    if n == 0 {
        return (0, 0);
    }
    let cmp = |i: i64| lang_cmp(lang, typed, &words[i as usize].key, typed.len());
    let (mut lo, mut hi) = (0i64, n - 1);
    let mut mid = hi / 2;
    let mut r = n as i32;
    if mid > 0 {
        loop {
            if hi <= mid {
                break;
            }
            r = cmp(mid);
            if r < 0 {
                hi = mid - 1;
            } else {
                if r < 1 {
                    break;
                }
                lo = mid + 1;
            }
            mid = (hi - lo) / 2 + lo;
            if lo >= mid {
                break;
            }
        }
    }
    if r != 0 {
        r = cmp(mid);
        if r != 0 {
            if mid == lo && lo != hi {
                mid += 1;
                r = cmp(mid);
            }
            if r != 0 {
                return (0, 0);
            }
        }
    }
    let mut i = mid - 1;
    let mut r = 0;
    while i >= 0 && r == 0 {
        r = cmp(i);
        i -= 1;
    }
    let first = if r != 0 { i + 2 } else { i + 1 };
    let mut j = mid + 1;
    let mut r = 0;
    while j < n && r == 0 {
        r = cmp(j);
        j += 1;
    }
    let count = if r != 0 { j - first - 1 } else { j - first };
    (first as usize, count as usize)
}

/// FUN_101b8150's root list and FUN_101bd430's listing of words: no text starting with a space, a text only once (the
/// texts' compare, same length), at most ROOM - 1 looked at.
fn listed<'a>(lang: Lang, words: impl Iterator<Item = &'a Word>) -> Vec<(String, [u8; 4])> {
    let mut out: Vec<&Word> = Vec::new();
    for w in words.take(ROOM - 1) {
        if byte(&w.raw, 0) == b' ' {
            continue;
        }
        if out.iter().any(|o| o.tlen == w.tlen && lang_cmp(lang, &o.raw, &w.raw, w.tlen) == 0) {
            continue;
        }
        out.push(w);
    }
    out.into_iter().map(|w| (w.text.clone(), w.id)).collect()
}

/// FUN_101ae530: block kinds 7..0x12 are items (7), 0x13.. key items (0x13); the rest stay.
fn norm(kind: u8) -> u8 {
    match kind {
        0..=6 => kind,
        7..=0x12 => KIND_ITEMS,
        _ => KIND_KEY_ITEMS,
    }
}

/// An item or key item block's id (the inner switches of FUN_101bd6d0): kinds 7 / 0xb / 0xf / 0x13 carry both bytes,
/// 9 / 0xd / 0x11 / 0x15 the low byte, 0xa / 0xe / 0x12 / 0x16 the high byte; any other kind is id 0.
fn block_id(kind: u8, hi: u8, lo: u8) -> u32 {
    match kind {
        7 | 0xb | 0xf | 0x13 => (hi as u32) << 8 | lo as u32,
        9 | 0xd | 0x11 | 0x15 => lo as u32,
        0xa | 0xe | 0x12 | 0x16 => (hi as u32) << 8,
        _ => 0,
    }
}

/// What a block names: None a phrase (its group and id are the block's last two bytes), else (key item, id) of an
/// item or key item block, id 0 its group's title.
pub fn block_item(b: [u8; 4]) -> Option<(bool, u32)> {
    (b[0] >= 7).then(|| (norm(b[0]) == KIND_KEY_ITEMS, block_id(b[0], b[2], b[3])))
}

/// An item or key item id as a block (FUN_101bc770 / FUN_101bca80), with the dictionary's language byte and no 00
/// byte: `base` 7 or 0x13, + 1 for id 0, + 2 when only the high byte is 0, + 3 when only the low byte is 0, the zero
/// bytes written FF.
pub fn id_block(base: u8, id: u16) -> [u8; 4] {
    id_block_in(lang_byte(), base, id)
}

fn id_block_in(l: u8, base: u8, id: u16) -> [u8; 4] {
    let [hi, lo] = id.to_be_bytes();
    match (hi, lo) {
        (0, 0) => [base + 1, l, 0xff, 0xff],
        (0, _) => [base + 2, l, 0xff, lo],
        (_, 0) => [base + 3, l, hi, 0xff],
        _ => [base, l, hi, lo],
    }
}

/// A phrase of a group as a block: the group's kind, the dictionary's language byte (the file's entries carry it).
pub fn phrase_block(group: u8, id: u8) -> [u8; 4] {
    [GROUP_KIND, lang_byte(), group, id]
}

/// A category's title words (FUN_101bc090 with FUN_101bbd10's count): `head` = the 0x4c-byte header (kind, language,
/// group, 0, title[32], completion[32], ..). The title keyed by the completion; a French group (language 4) whose
/// title from its third byte differs from the completion over the completion's length (FUN_101c9e70, DAT_1037e358)
/// has a second, keyed by the title inside its brackets.
fn title_words(head: &[u8], second: bool) -> Vec<Word> {
    let id = [head[0], head[1], head[2], head[3]];
    let title = &head[4..];
    let tlen = title.iter().take(0x20).position(|&c| c == 0).unwrap_or(0x20);
    let comp = &head[0x24..];
    let clen = comp.iter().take(0x20).position(|&c| c == 0).unwrap_or(0x20);
    let mut w = vec![Word::keyed(title, tlen, comp, clen, id)];
    if second && head[1] == 4 && fold_cmp(&head[6..], comp, clen, &FOLD_ORDER) != 0 && tlen >= 4 {
        w.push(Word::keyed(title, tlen, &title[2..], tlen - 4, id));
    }
    w
}

/// An entry's words (FUN_101bbfa0 for English and German, FUN_101bbe00 for Japanese (1) and French (4)): an item kind
/// (7..0x12) is one word of its whole length (its NUL in); any other kind's text goes through `at` (FUN_101bbba0) and,
/// in Japanese and French, a second string follows: longer than its NUL it is the word's key, and French keeps a
/// second word keyed by the text. (bytes used, words).
fn entry_words(body: &[u8], p: usize, lang: u8, at: &dyn Fn(&[u8]) -> Option<Vec<u8>>) -> Option<(usize, Vec<Word>)> {
    let e = body.get(p..p + 5)?;
    let (id, len) = ([e[0], e[1], e[2], e[3]], e[4] as usize);
    let raw = body.get(p + 5..p + 5 + len)?;
    let mut next = p + 5 + len;
    if (7..=0x12).contains(&id[0]) {
        return Some((next, vec![Word::plain(raw, len, id)]));
    }
    let (text, tlen) = match at(raw) {
        Some(mut name) => {
            let l = name.len();
            name.push(0);
            (name, l)
        }
        None => (raw.to_vec(), len.saturating_sub(1)),
    };
    let mut words = Vec::new();
    if matches!(lang, 1 | 4) {
        let l2 = *body.get(next)? as usize;
        let key = body.get(next + 1..next + 1 + l2)?;
        next += 1 + l2;
        if l2 >= 2 {
            words.push(Word::keyed(&text, tlen, key, l2 - 1, id));
            if lang == 1 {
                return Some((next, words));
            }
        }
    }
    words.push(Word::plain(&text, tlen, id));
    Some((next, words))
}

/// FUN_101bd430's text for a category FUN_101bc640 does not find (DAT_1037da7c, "エラー").
const ERROR_TEXT: [u8; 6] = [0x83, 0x47, 0x83, 0x89, 0x81, 0x5b];

/// The name tables FUN_101bbba0 reads for an `@` text, in the files' language.
struct AtNames {
    jobs: Option<Messages>,
    m_names: Option<Messages>,
    places: Option<Messages>,
    spells: Option<Messages>,
    abilities: Option<Messages>,
}

impl AtNames {
    /// FUN_101bbba0: `@A` place, `@C` spell, `@J` job, `@M` group 0x13, `@Y` ability by hex id (strtoul base 16 from
    /// the third byte); None for other texts and names the tables lack.
    fn get(&self, raw: &[u8]) -> Option<Vec<u8>> {
        if raw.first() != Some(&b'@') {
            return None;
        }
        let hex: String = raw.iter().skip(2).take_while(|c| c.is_ascii_hexdigit()).map(|&c| c as char).collect();
        let id = usize::from_str_radix(&hex, 16).unwrap_or(0);
        let table = match raw.get(1)? {
            b'A' => &self.places,
            b'C' => &self.spells,
            b'J' => &self.jobs,
            b'M' => &self.m_names,
            b'Y' => &self.abilities,
            _ => return None,
        };
        table.as_ref()?.get(id).filter(|n| !n.is_empty()).map(|n| encode_sjis(&n))
    }
}

impl Dictionary {
    pub fn load(index: &DatIndex, ffximain: &[u8]) -> Result<Self> {
        Self::load_lang(index, ffximain, super::lang::files())
    }

    /// The dictionary of a language's files (FUN_101b67e0: slot 0x18, language byte + 1).
    pub fn load_lang(index: &DatIndex, ffximain: &[u8], lang: Lang) -> Result<Self> {
        let lang = lang as u8;
        let file = super::msg::slot_file(ffximain, lang, 0x18).context("auto-translate slot")?;
        let data = index.read(file)?;
        let msgs = |g: u8| super::msg::group_file(ffximain, lang, g).and_then(|f| Messages::load(index, f).ok());
        let slot = |s: usize| super::msg::slot_file(ffximain, lang, s).and_then(|f| Messages::load(index, f).ok());
        let b = msgs(0xb);
        let title = |id: usize| b.as_ref().and_then(|m| m.get(id)).unwrap_or_default();
        let names = AtNames {
            jobs: msgs(0x12),
            m_names: msgs(0x13),
            // FUN_101bbba0 'A': group 0x14 in Japanese and English, 0x15 in German and French.
            places: msgs(if lang < 2 { 0x14 } else { 0x15 }),
            spells: slot(super::battle::SLOT_SPELLS),
            abilities: slot(super::battle::SLOT_ABILITIES),
        };
        let mut d = Self::parse_with(&data, lang + 1, &|raw| names.get(raw))?;
        let AtNames { jobs, m_names, places, .. } = names;
        (d.jobs, d.m_names, d.places) = (jobs, m_names, places);
        d.titles = [(title(0x91), title(0x92)), (title(0x93), title(0x94))];
        Ok(d)
    }

    /// The groups built at run time as they stand (held read: `rebuild` waits).
    pub fn built(&self) -> std::sync::RwLockReadGuard<'_, Vec<Built>> {
        self.built.read().unwrap_or_else(|e| e.into_inner())
    }

    /// FUN_101bcd10 on the root category list: Key Items, then Items, built again from the record cache's slots
    /// (`recent`) and the key item table as (id, name, owned); a group with no entry is not there. Then the merged
    /// words again (FUN_101bdaf0).
    pub fn rebuild(&self, recent: &[(u32, String)], key_items: &[(u32, String, bool)]) {
        let lang = self.lang_byte;
        let head = |kind: u8, (title, key): &(String, String)| {
            let mut h = vec![kind, lang, 0, 0];
            for t in [title, key] {
                let mut f = encode_sjis(t);
                f.resize(0x20, 0);
                h.extend(f);
            }
            h
        };
        let mut built = Vec::new();
        let mut keys = Vec::new();
        for (id, name, _) in key_items.iter().filter(|k| k.2) {
            if (0x181..=0x183).contains(id) || keys.len() >= 0xfe {
                break;
            }
            keys.push((id_block_in(lang, KIND_KEY_ITEMS, *id as u16), name.clone()));
        }
        // A title that did not load (group 0xb) leaves its group out: no blank category, no empty brackets.
        if !keys.is_empty() && !self.titles[1].0.is_empty() {
            let (title, key) = self.titles[1].clone();
            // FUN_101bbcb0 (the pairs path): one title, a word per key item keyed by its name, in table order (the
            // category is not sorted).
            let mut list = title_words(&head(KIND_KEY_ITEMS, &self.titles[1]), false);
            let titles = list.len();
            for (b, name) in &keys {
                let mut raw = encode_sjis(name);
                let l = raw.len();
                raw.push(0);
                list.push(Word::plain(&raw, l, *b));
            }
            let sorted = (titles..list.len()).collect();
            built.push(Built { kind: KIND_KEY_ITEMS, title, key, words: keys, list, titles, sorted });
        }
        let items: Vec<_> = recent
            .iter()
            .filter(|(id, name)| *id != 0 && !name.starts_with('.') && !(0x7400..=0xfffe).contains(id))
            .map(|(id, name)| (id_block_in(lang, KIND_ITEMS, *id as u16), name.clone()))
            .collect();
        if !items.is_empty() && !self.titles[0].0.is_empty() {
            let (title, key) = self.titles[0].clone();
            // FUN_101bc770: an item's length byte counts its NUL, and an item kind keeps it as the word's length.
            let mut list = title_words(&head(KIND_ITEMS, &self.titles[0]), true);
            let titles = list.len();
            for (b, name) in &items {
                let mut raw = encode_sjis(name);
                raw.push(0);
                list.push(Word::plain(&raw, raw.len(), *b));
            }
            let mut sorted: Vec<usize> = (titles..list.len()).collect();
            select_sort(&mut sorted, |&a, &b| before(self.lang, &list[a], &list[b]));
            built.push(Built { kind: KIND_ITEMS, title, key, words: items, list, titles, sorted });
        }
        let merged = self.merge(&built);
        if let Ok(mut b) = self.built.write() {
            *b = built;
        }
        if let Ok(mut m) = self.merged.write() {
            *m = merged;
        }
    }

    /// FUN_101bdaf0: every category's words (titles in) but a NULL or empty text and a text that is one block (FD,
    /// six bytes), selection-sorted by key.
    fn merge(&self, built: &[Built]) -> Vec<Word> {
        let mut all: Vec<Word> = self
            .groups
            .iter()
            .flat_map(|g| g.words.iter())
            .chain(built.iter().flat_map(|b| b.list.iter()))
            .filter(|w| w.tlen != 0 && !(byte(&w.raw, 0) == PHRASE_MARK && w.tlen == 6))
            .cloned()
            .collect();
        select_sort(&mut all, |a, b| before(self.lang, a, b));
        all
    }

    /// FUN_101b84b0's lookups (FUN_101bd060 / FUN_101bd230 / FUN_101bd430): with `filter` the category's words by key
    /// (the typed word, when there is one, picks among them), else the words whose key starts with `word` (the files'
    /// compare over its length; hiragana typed as katakana). None found (or an empty category): the categories'
    /// titles. (text, block id) and whether words were found; an unknown category gives retail's "エラー".
    pub fn query(&self, word: &str, filter: Option<[u8; 4]>) -> (Vec<(String, [u8; 4])>, bool) {
        let built = self.built();
        let titles = || {
            self.groups
                .iter()
                .filter_map(|g| g.words.first())
                .chain(built.iter().filter_map(|b| b.list.first()))
                .map(|w| (w.text.clone(), w.id))
                .collect::<Vec<_>>()
        };
        let mut typed = encode_sjis(word);
        kana(&mut typed);
        let merged = self.merged.read().unwrap_or_else(|e| e.into_inner());
        let words: Vec<&Word> = match filter {
            // FUN_101bc640: a file group by its group byte, else a built one by its kind byte.
            Some(f) => match self.by_id.get(&f[2]) {
                Some(&i) => self.groups[i].sorted.iter().map(|&k| &self.groups[i].words[k]).collect(),
                None => match built.iter().find(|b| b.kind == f[0]) {
                    Some(b) => b.sorted.iter().map(|&k| &b.list[k]).collect(),
                    None => return (vec![(decode_sjis(&ERROR_TEXT), [0; 4])], false),
                },
            },
            None if typed.is_empty() => return (titles(), false),
            None => merged.iter().collect(),
        };
        let (first, count) = if typed.is_empty() { (0, words.len()) } else { search(self.lang, &words, &typed) };
        if count == 0 {
            return (titles(), false);
        }
        (listed(self.lang, words[first..first + count].iter().copied()), true)
    }

    /// FUN_101ac990 without braces (a text command's argument, the line being typed): blocks as their plain text.
    pub fn expand_plain(&self, s: &str, names: Option<&dyn NameSource>) -> String {
        self.expand(s, names).chars().filter(|&c| c != '\u{E008}' && c != '\u{E009}').collect()
    }

    /// Groups (0x4c-byte header: kind, language, group id, 0, title[32], completion[32], u32 count, u32 bytes) then
    /// entries (4 id bytes, u8 length, text; languages 1 and 4 add a second string). Only English groups are kept.
    #[cfg(test)]
    pub fn parse(data: &[u8]) -> Result<Self> {
        Self::parse_lang(data, 2)
    }

    /// The groups of one language byte (1 Japanese, 2 English, 3 German, 4 French: the client's language + 1), or of
    /// kind 1 in any language (FUN_101bcd70; the files have none), with their words (`entry_words`): the second string
    /// of a Japanese entry holds the kana reading, of a French one the text without its accents. `@` texts stay.
    #[cfg(test)]
    pub fn parse_lang(data: &[u8], language: u8) -> Result<Self> {
        Self::parse_with(data, language, &|_| None)
    }

    fn parse_with(data: &[u8], language: u8, at: &dyn Fn(&[u8]) -> Option<Vec<u8>>) -> Result<Self> {
        let lang = match language {
            1 => Lang::Ja,
            3 => Lang::De,
            4 => Lang::Fr,
            _ => Lang::En,
        };
        let mut groups = Vec::new();
        let mut off = 0;
        while off + 0x4c <= data.len() {
            let h = &data[off..off + 0x4c];
            let bytes = u32::from_le_bytes(h[0x48..0x4c].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(h[0x44..0x48].try_into().unwrap()) as usize;
            let body = data.get(off + 0x4c..off + 0x4c + bytes).context("group runs past the file")?;
            off += 0x4c + bytes;
            if h[1] != language && h[0] != 1 {
                continue;
            }
            let mut words = title_words(h, true);
            let titles = words.len();
            // Each entry takes at least 5 bytes; a bad count must not reserve more than the group holds.
            let mut phrases = Vec::with_capacity(count.min(body.len() / 5));
            let mut p = 0;
            while phrases.len() < count && p + 5 <= body.len() {
                let (next, ws) = entry_words(body, p, h[1], at).context("entry runs past its group")?;
                p = next;
                phrases.push((ws[0].id[3], ws[0].text.clone(), ws[0].key_text()));
                words.extend(ws);
            }
            let mut sorted: Vec<usize> = (titles..words.len()).collect();
            select_sort(&mut sorted, |&a, &b| before(lang, &words[a], &words[b]));
            groups.push(Group { id: h[2], title: words[0].text.clone(), phrases, words, sorted });
        }
        let by_id = groups.iter().enumerate().map(|(i, g)| (g.id, i)).collect();
        let d = Dictionary {
            groups,
            by_id,
            jobs: None,
            m_names: None,
            places: None,
            titles: Default::default(),
            built: Default::default(),
            merged: Default::default(),
            lang,
            lang_byte: language,
        };
        let merged = d.merge(&[]);
        *d.merged.write().unwrap_or_else(|e| e.into_inner()) = merged;
        Ok(d)
    }

    /// FUN_101bd6d0: a block's text, the language byte ignored. Kinds 7.. are items / key items (`block_id`, id 0 the
    /// built group's title), any other kind (6 too) a phrase. None: unknown (the line ends there).
    pub fn resolve(&self, b: [u8; 4], names: Option<&dyn NameSource>) -> Option<String> {
        let [kind, _, hi, lo] = b;
        if kind >= 7 {
            let id = block_id(kind, hi, lo);
            let key = norm(kind) == KIND_KEY_ITEMS;
            if id == 0 {
                let title = self.built.read().ok()?.iter().find(|g| g.kind == norm(kind)).map(|g| g.title.clone());
                if title.is_some() || !key {
                    return title;
                }
            }
            let n = names?;
            return if key { n.key_item(id).map(|x| x.bare) } else { n.item_name(id) };
        }
        let g = &self.groups[*self.by_id.get(&hi)?];
        if lo == 0 {
            return Some(g.title.clone());
        }
        let (_, text, _) = g.phrases.iter().find(|p| p.0 == lo)?;
        Some(text.clone()).filter(|t| !t.starts_with('@'))
    }

    /// FUN_101ac990 with braces (the log): each block becomes its text between the green and red auto-translate
    /// brackets (EF 27 / EF 28, U+E008 / U+E009); an unknown block cuts the line there. A variable block (kind 6)
    /// has no brackets (`variable`).
    pub fn expand(&self, s: &str, names: Option<&dyn NameSource>) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut at = 0;
        while at < chars.len() {
            if is_variable(&chars, at) {
                match self.variable(&chars, &mut at, names) {
                    Some(t) => out.push_str(&t),
                    None => break,
                }
            } else if let Some(block) = block_at(&chars, at) {
                at += 4;
                // A 6-byte block of kind 6 is a variable block whose length byte is FD (negative): the line ends.
                match Some(block).filter(|b| b[0] != KIND_VARIABLE).and_then(|b| self.resolve(b, names)) {
                    Some(t) => {
                        out.push('\u{E008}');
                        out.push_str(&t);
                        out.push('\u{E009}');
                    }
                    None => break,
                }
            } else {
                // A lone phrase char (a cut block) shows nothing, as before.
                if !is_phrase(chars[at]) {
                    out.push(chars[at]);
                }
                at += 1;
            }
        }
        out
    }

    /// FUN_101adbb0: `FD 06 x x x nn <template> x`; each FD in the template is a place, filled by the next block in
    /// the line, else by the place's id bytes. None (the line ends): nn over 0x7f, or a following block that fails.
    fn variable(&self, chars: &[char], at: &mut usize, names: Option<&dyn NameSource>) -> Option<String> {
        let byte = |i: usize| chars.get(i).and_then(|&c| raw_byte(c));
        let nn = byte(*at + 5).filter(|&n| n < 0x80)? as usize;
        let body: Vec<u8> = (0..nn).map(|k| byte(*at + 6 + k)).collect::<Option<_>>()?;
        byte(*at + 6 + nn)?;
        *at += nn + 7;
        let mut out = String::new();
        let mut run = Vec::new();
        let mut i = 0;
        while i < nn {
            if body[i] != PHRASE_MARK {
                run.push(body[i]);
                i += 1;
                continue;
            }
            out.push_str(&decode_sjis(&std::mem::take(&mut run)));
            if is_variable(chars, *at) {
                out.push_str(&self.variable(chars, at, names)?);
            } else if let Some(block) = block_at(chars, *at) {
                // Kind 6 there is a variable block with a negative length: FUN_101adbb0 fails on it.
                if block[0] == KIND_VARIABLE {
                    return None;
                }
                *at += 4;
                out.push_str(&self.resolve(block, names)?);
            } else {
                // A failed id adds the resolver's -1 to this call's length (its buffer, not the caller's).
                let id = [1, 2, 3, 4].map(|k| body.get(i + k).copied().unwrap_or(0));
                match self.resolve(id, names) {
                    Some(t) => out.push_str(&t),
                    None => {
                        out.pop();
                    }
                }
            }
            i += 6;
        }
        out.push_str(&decode_sjis(&run));
        Some(out)
    }
}

/// The four id bytes of the block starting at `at` (four phrase chars, positions 0..3).
fn block_at(chars: &[char], at: usize) -> Option<[u8; 4]> {
    let mut b = [0u8; 4];
    for (k, x) in b.iter_mut().enumerate() {
        let cp = *chars.get(at + k)? as u32;
        if !is_phrase(chars[at + k]) || (cp - super::font::PHRASE_BASE) / 0x100 != k as u32 {
            return None;
        }
        *x = cp as u8;
    }
    Some(b)
}

/// A variable block starts at `at`: the kept bytes FD 06 (font::decode_sjis).
fn is_variable(chars: &[char], at: usize) -> bool {
    let byte = |i: usize| chars.get(i).and_then(|&c| raw_byte(c));
    byte(at) == Some(PHRASE_MARK) && byte(at + 1) == Some(KIND_VARIABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The other languages' files (slot 0x18: ja 55545, de 55785, fr 56205): the same 42 groups; Japanese and French
    /// entries carry the second key.
    #[test]
    fn second_keys() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        for (file, lang) in [(55545, 1), (55785, 3), (56205, 4)] {
            let Ok(data) = index.read(file) else { return };
            let d = Dictionary::parse_lang(&data, lang).unwrap();
            let all: Vec<&(u8, String, String)> = d.groups.iter().flat_map(|g| &g.phrases).collect();
            let key = |text: &str| all.iter().find(|p| p.1 == text).map(|p| p.2.as_str());
            assert_eq!((d.groups.len(), all.len()), (42, 2685));
            match lang {
                // The kana reading; a phrase written in kana already has none.
                1 => assert_eq!(key("初めまして。"), Some("ハジメマシテ。")),
                3 => assert!(all.iter().all(|p| p.1 == p.2)),
                // The text without accents (and here without its full stop).
                _ => assert_eq!(key("Pardon."), Some("Pardon")),
            }
        }
    }

    /// FUN_101c9e70's two tables are the DLL's (DAT_1037e358, DAT_1037e3a0).
    #[test]
    fn fold_tables() {
        let cfg = crate::config::Config::load();
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let mut both = FOLD_ORDER.to_vec();
        both.extend([0; 6]);
        both.extend(FOLD_ACCENTS);
        assert!(dll.windows(both.len()).any(|w| w == both.as_slice()));
    }

    /// The compares: the order table keeps the accented letters apart, the accent table folds them; _strnicmp folds
    /// case only; a 0x85 pair against a plain letter steps each side by its own bytes.
    #[test]
    fn compares() {
        let (accented, plain) = (b"R\x85\xc8ponses", b"Reponses");
        assert_ne!(fold_cmp(accented, plain, 8, &FOLD_ORDER), 0);
        assert_eq!(fold_cmp(accented, plain, 9, &FOLD_ACCENTS), 0);
        assert_eq!(fold_cmp(b"reponses", accented, 8, &FOLD_ACCENTS), 0);
        assert!(strnicmp(b"Abc", b"abd", 3) < 0);
        assert_eq!(strnicmp(b"PROT", b"protect", 4), 0);
        assert!(strncmp(b"Prot", b"prot", 4) < 0);
        // A byte outside the tables compares signed: 0x82.. is below 'A'.
        assert!(fold_cmp(b"\x82\xa0", b"A", 1, &FOLD_ACCENTS) < 0);
        // Hiragana typed as katakana.
        let mut t = vec![0x82, 0xcd, 0x82, 0xb6, b'a'];
        kana(&mut t);
        assert_eq!(t, [0x83, 0x6e, 0x83, 0x57, b'a']);
    }

    /// FUN_101bd230's search gives the run a scan of the sorted words finds, for every prefix of every key.
    #[test]
    fn search_finds_the_run() {
        let raw: Vec<&[u8]> = vec![b"Apple", b"apricot", b"Banana", b"band", b"Bandit", b"can", b"Can", b"cane", b"x"];
        let mut words: Vec<Word> = raw.iter().map(|r| Word::plain(r, r.len(), [2, 2, 1, 1])).collect();
        select_sort(&mut words, |a, b| before(Lang::En, a, b));
        let refs: Vec<&Word> = words.iter().collect();
        for r in &raw {
            for n in 1..=r.len() {
                let t = &r[..n];
                let want: Vec<usize> = (0..refs.len()).filter(|&i| strnicmp(t, &refs[i].key, n) == 0).collect();
                let (first, count) = search(Lang::En, &refs, t);
                assert_eq!((first..first + count).collect::<Vec<_>>(), want, "{}", String::from_utf8_lossy(t));
            }
        }
        assert_eq!(search(Lang::En, &refs, b"zz"), (0, 0));
    }

    /// French (file 56205): an entry with a second string is two words, nine titles differ from their completion by
    /// their accents and have a second word; a typed word finds the text without its accents, once.
    #[test]
    fn french_words() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(data) = index.read(56205) else { return };
        let d = Dictionary::parse_lang(&data, 4).unwrap();
        let entries: usize = d.groups.iter().map(|g| g.phrases.len()).sum();
        let titles = d.groups.iter().map(|g| g.words.len() - g.sorted.len()).collect::<Vec<_>>();
        assert_eq!(titles.iter().filter(|&&t| t == 2).count(), 9);
        let words: usize = d.groups.iter().map(|g| g.sorted.len()).sum();
        assert_eq!((entries, words), (2685, 2685 + 550));
        let (hits, matched) = d.query("pardon", None);
        assert!(matched);
        assert_eq!(hits.iter().filter(|h| h.0 == "Pardon.").count(), 1);
        // The title "【Réponses】" by its key without accents and by itself; a block names the entry it came from.
        let rep = d.groups.iter().find(|g| g.words.len() - g.sorted.len() == 2).unwrap();
        let typed = text_of(&rep.words[1].key, rep.words[1].klen);
        let (hits, matched) = d.query(&typed.replace('\u{f85c8}', "e"), None);
        assert!(matched && hits.iter().filter(|h| h.0 == rep.title).count() == 1, "{typed} {hits:?}");
        assert_eq!(d.resolve([2, 4, rep.id, 0], None).as_deref(), Some(rep.title.as_str()));
        let id = rep.phrases[0].0;
        assert_eq!(d.resolve([2, 4, rep.id, id], None), Some(rep.phrases[0].1.clone()));
    }

    /// Japanese (file 55545): a word keyed by its kana reading, found by the reading typed in hiragana.
    #[test]
    fn japanese_words() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(data) = index.read(55545) else { return };
        let d = Dictionary::parse_lang(&data, 1).unwrap();
        let words: usize = d.groups.iter().map(|g| g.sorted.len()).sum();
        assert_eq!(words, 2685);
        let (hits, matched) = d.query("はじめまして", None);
        assert!(matched && hits.iter().any(|h| h.0 == "初めまして。"), "{hits:?}");
    }

    /// German: `@A` places come from group 0x15 (FUN_101bbba0), the blocks carry language byte 3.
    #[test]
    fn german_places() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let d = Dictionary::load_lang(&index, &dll, Lang::De).unwrap();
        let places = super::super::msg::group_file(&dll, 2, 0x15).and_then(|f| Messages::load(&index, f).ok()).unwrap();
        let g = d.groups.iter().find(|g| g.words.iter().skip(1).any(|w| w.id[1] == 3)).unwrap();
        assert!(g.words.iter().all(|w| w.id[1] == 3));
        // Some phrase of the places group is one of 0x15's names.
        let names: std::collections::HashSet<String> = (0..0x400).filter_map(|i| places.get(i)).collect();
        assert!(d.groups.iter().flat_map(|g| &g.phrases).filter(|p| names.contains(&p.1)).count() > 100);
        assert!(d.groups.iter().flat_map(|g| &g.phrases).all(|p| !p.1.starts_with("@A")));
    }

    /// File 55665: 42 English groups; Greetings 1 is "Nice to meet you."; @J / @C / @Y name references resolve; a
    /// block in a chat line expands between the brackets.
    #[test]
    fn english_dictionary() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let d = Dictionary::load(&index, &dll).unwrap();
        assert_eq!(d.groups.len(), 42);
        let names = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En).unwrap();
        let n: Option<&dyn NameSource> = Some(&names);
        assert_eq!(d.resolve([2, 2, 1, 1], n).as_deref(), Some("Nice to meet you."));
        assert_eq!(d.resolve([2, 2, 0x10, 1], n).as_deref(), Some("Thief"));
        assert_eq!(d.resolve([2, 2, 0x1b, 1], n).as_deref(), Some("Protect"));
        assert_eq!(d.resolve([2, 2, 0x2a, 0x27], n).as_deref(), Some("Soul Enslavement"));
        let line = decode_sjis(b"Hi \xFD\x02\x02\x01\x01\xFD");
        assert_eq!(d.expand(&line, n), "Hi \u{E008}Nice to meet you.\u{E009}");
        // Completion: "prot" finds Protect (and its kin) before the categories; no word gives the 42 titles.
        let (hits, matched) = d.query("prot", None);
        assert!(matched && hits.iter().any(|c| c.0 == "Protect"));
        assert_eq!(d.query("", None).0.len(), 42);
        assert_eq!(d.query("", Some([2, 2, 1, 0])).0.first().map(|c| c.1[2]), Some(1));
        // An item phrase is the record's name, not its log noun (record cache +0x1b4), and joins the Items group.
        assert_eq!(d.resolve([7, 2, 0x40, 0x97], n).as_deref(), Some("Bronze Sword"));
        assert_eq!(d.resolve([8, 2, 0xff, 0xff], n), None);
        d.rebuild(&names.items.recent(), &[]);
        let b = d.built().clone();
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].kind, b[0].words.as_slice()), (7, &[([7, 2, 0x40, 0x97], "Bronze Sword".to_string())][..]));
        assert_eq!(d.query("", None).0.len(), 43);
        assert_eq!(d.resolve([8, 2, 0xff, 0xff], n).as_deref(), Some(b[0].title.as_str()));
        assert!(d.query("bronze s", None).0.iter().any(|c| c.1 == [7, 2, 0x40, 0x97]));
    }

    /// One group (id 1) with phrases 1 "Hello!" and 2 "Thanks.".
    fn small() -> Dictionary {
        let mut data = vec![2, 2, 1, 0];
        let field = |t: &str| {
            let mut f = t.as_bytes().to_vec();
            f.resize(32, 0);
            f
        };
        data.extend(field("[Greetings]"));
        data.extend(field("greetings"));
        let mut body = Vec::new();
        for (id, t) in [(1u8, "Hello!"), (2, "Thanks.")] {
            body.extend([2, 2, 1, id, t.len() as u8 + 1]);
            body.extend(t.as_bytes());
            body.push(0);
        }
        data.extend(2u32.to_le_bytes());
        data.extend((body.len() as u32).to_le_bytes());
        data.extend(body);
        let mut d = Dictionary::parse(&data).unwrap();
        d.titles = [("(Items)".into(), "items".into()), ("(Key Items)".into(), "key items".into())];
        d
    }

    /// FUN_101bcd10: Key Items takes the first free slot, Items the next; the skips, the 00-free ids and the scan's
    /// end at an owned 0x181..0x183.
    #[test]
    fn built_groups() {
        let d = small();
        assert_eq!(d.query("", None).0.len(), 1);
        // No group yet: id 0 of an item is nothing, of a key item key item 0's name (none here).
        assert_eq!(d.resolve([8, 2, 0xff, 0xff], None), None);
        assert_eq!(d.resolve([0x14, 2, 0xff, 0xff], None), None);
        let s = |t: &str| t.to_string();
        let recent = [
            (0x1010, s("Potion")),
            (0x0050, s("Chair")),
            (0x1000, s("Ether")),
            (0x7400, s("Objective")),
            (0xfffe, s("Last")),
            (0xffff, s("Gil")),
            (0x0011, s(".hidden")),
        ];
        let keys = [
            (0, s("-header"), false),
            (0x45, s("Airship pass"), true),
            (0x46, s("Not owned"), false),
            (0x200, s("Map"), true),
            (0x182, s("Stop"), true),
            (0x300, s("After"), true),
        ];
        d.rebuild(&recent, &keys);
        let b = d.built().clone();
        assert_eq!(b.iter().map(|g| g.kind).collect::<Vec<_>>(), vec![0x13, 7]);
        assert_eq!(b[0].words, vec![([0x15, 2, 0xff, 0x45], s("Airship pass")), ([0x16, 2, 2, 0xff], s("Map"))]);
        assert_eq!(
            b[1].words,
            vec![
                ([7, 2, 0x10, 0x10], s("Potion")),
                ([9, 2, 0xff, 0x50], s("Chair")),
                ([10, 2, 0x10, 0xff], s("Ether")),
                ([7, 2, 0xff, 0xff], s("Gil")),
            ]
        );
        // The categories: the file's groups, Key Items, Items; a category's words by key.
        let cats = d.query("", None).0;
        assert_eq!(
            cats.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["[Greetings]", "(Key Items)", "(Items)"]
        );
        assert_eq!(cats[2].1, [7, 2, 0, 0]);
        let items = d.query("", Some(cats[2].1)).0;
        assert_eq!(items.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["Chair", "Ether", "Gil", "Potion"]);
        assert_eq!(d.query("", Some(cats[1].1)).0.len(), 2);
        // A typed word finds them among the phrases; the titles answer by their keys.
        assert_eq!(d.query("po", None).0, vec![(s("Potion"), [7, 2, 0x10, 0x10])]);
        assert_eq!(d.query("key", None).0, vec![(s("(Key Items)"), [0x13, 2, 0, 0])]);
        // Id 0 is the group's title while it stands; kinds past 0x16 are id 0 too.
        assert_eq!(d.resolve([8, 2, 0xff, 0xff], None).as_deref(), Some("(Items)"));
        assert_eq!(d.resolve([0x17, 2, 1, 2], None).as_deref(), Some("(Key Items)"));
        assert_eq!((block_id(0xd, 0xff, 0x50), block_id(0x12, 0x10, 0xff), block_id(0xf, 1, 2)), (0x50, 0x1000, 0x102));
        // What a block names: every built word gives its id back; a category block is id 0; a phrase is none.
        assert_eq!(
            b[1].words.iter().map(|w| block_item(w.0)).collect::<Vec<_>>(),
            [0x1010, 0x50, 0x1000, 0xffff].map(|i| Some((false, i)))
        );
        assert_eq!(
            b[0].words.iter().map(|w| block_item(w.0)).collect::<Vec<_>>(),
            [0x45, 0x200].map(|i| Some((true, i)))
        );
        assert_eq!(
            (block_item(cats[2].1), block_item(cats[1].1), block_item(cats[0].1)),
            (Some((false, 0)), Some((true, 0)), None)
        );
        // Nothing held and an empty cache: both groups go.
        d.rebuild(&[], &keys[..3].iter().map(|k| (k.0, k.1.clone(), false)).collect::<Vec<_>>());
        assert!(d.built().is_empty());
    }

    /// A title that did not load leaves its group out: no blank category, and id 0 still ends the line.
    #[test]
    fn untitled_groups() {
        let mut d = small();
        d.titles = Default::default();
        d.rebuild(&[(0x1010, "Potion".into())], &[(0x45, "Airship pass".into(), true)]);
        assert!(d.built().is_empty());
        assert_eq!(d.query("", None).0.len(), 1);
        assert_eq!(d.resolve([8, 2, 0xff, 0xff], None), None);
    }

    /// FUN_101adbb0's variable block: the template's places take the blocks that follow it, else their id bytes; no
    /// brackets; a length byte over 0x7f ends the line.
    #[test]
    fn variable_block() {
        let d = small();
        // "<" place ">" then the block that fills the place.
        let line = decode_sjis(b"a \xFD\x06\x02\x01\x01\x08<\xFD\x02\x02\x01\x02\xFD>\xFD\xFD\x02\x02\x01\x01\xFD b");
        assert_eq!(d.expand(&line, None), "a <Hello!> b");
        // No block follows: the place's id bytes; the next plain block keeps its brackets.
        let line = decode_sjis(b"\xFD\x06\x00\x00\x00\x06\xFD\x02\x02\x01\x02\xFD\xFD!");
        assert_eq!(d.expand(&line, None), "Thanks.!");
        let line = decode_sjis(b"\xFD\x06\x00\x00\x00\x00\xFD\xFD\x02\x02\x01\x01\xFD");
        assert_eq!(d.expand(&line, None), "\u{E008}Hello!\u{E009}");
        // A variable block fills a place of another; an unknown place id takes one byte off.
        let line = decode_sjis(
            b"\xFD\x06\x00\x00\x00\x07[\xFD\x02\x02\x01\x02\xFD\xFD\xFD\x06\x00\x00\x00\x08xy\xFD\x02\x02\x09\x09\xFD\xFD.",
        );
        assert_eq!(d.expand(&line, None), "[x.");
        // A following block that resolves to nothing, and a negative length, end the line.
        let line = decode_sjis(b"ok\xFD\x06\x00\x00\x00\x06\xFD\x02\x02\x01\x02\xFD\xFD\xFD\x02\x02\x09\x09\xFDtail");
        assert_eq!(d.expand(&line, None), "ok");
        let line = decode_sjis(b"ok\xFD\x06\x00\x00\x00\xFDtail");
        assert_eq!(d.expand(&line, None), "ok");
        // The template's text before a failing place goes with it: the call builds its text apart and returns -1.
        let line =
            decode_sjis(b"ok\xFD\x06\x00\x00\x00\x09Hi \xFD\x02\x02\x01\x02\xFD\xFD\xFD\x02\x02\x09\x09\xFDtail");
        assert_eq!(d.expand(&line, None), "ok");
        // A following kind-6 block (its length byte FD) ends the line; a place's id bytes of kind 6 are a phrase.
        let line = decode_sjis(b"ok\xFD\x06\x00\x00\x00\x06\xFD\x02\x02\x01\x02\xFD\xFD\xFD\x06\x02\x01\x01\xFDtail");
        assert_eq!(d.expand(&line, None), "ok");
        let line = decode_sjis(b"\xFD\x06\x00\x00\x00\x06\xFD\x06\x02\x01\x01\xFD\xFD!");
        assert_eq!(d.expand(&line, None), "Hello!!");
        // The bytes survive a round trip.
        let raw = b"\xFD\x06\x02\x01\x01\x08<\xFD\x02\x02\x01\x02\xFD>\xFD";
        assert_eq!(crate::dat::font::encode_sjis(&decode_sjis(raw)), raw);
    }
}
