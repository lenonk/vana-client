//! Item records as FUN_1015b9e0 reads them: 0x1400 bytes per item (0xc00 in the 2019 build, and in the German and
//! French files retail no longer updates), each byte rotated right by 5 (FUN_1015d7f0). The id range picks the file
//! (table DAT_10372150, filled by FUN_1015b720 from language slots) and the field parser (FUN_1015c0a0 and the
//! others). Strings follow FUN_1015be00, the icon is at +0x280 (size) / +0x284 (data). The wide records widen the
//! flags to u32, so fields from +6 on sit 2 later (4 after an alignment gap before the jobs word).

use super::DatIndex;
use super::font::decode_sjis;
use anyhow::{Context, Result};
use std::io::{Read, Seek, SeekFrom};

/// FFXiMain language codes (FUN_10250de0): 0 ja, 1 en, 2 de, 3 fr.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ja = 0,
    En = 1,
    De = 2,
    Fr = 3,
}

impl Lang {
    pub fn parse(s: &str) -> Option<Lang> {
        match s {
            "ja" => Some(Lang::Ja),
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            "fr" => Some(Lang::Fr),
            _ => None,
        }
    }
}

/// Record layout, one per parser function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// FUN_101557b0
    General,
    /// FUN_10155920
    Usable,
    /// FUN_10155a90 (automaton attachments)
    Automaton,
    /// FUN_10155bf0
    Armor,
    /// FUN_10155da0
    Weapon,
    /// FUN_10155f70 (0x7000..)
    K7000,
    /// FUN_101560e0 (0x7400..)
    K7400,
    /// FUN_10156270: Records of Eminence objectives (grammar kinds 0x39-0x41 read id + 0xe000)
    Objective,
    /// FUN_101563d0 (no strings)
    Keff8,
    /// FUN_101564c0: monstrosity species
    Monster,
    /// FUN_10156630: the Records of Eminence categories (dat/roe.rs)
    RoeCategory,
    /// FUN_10156790 (no strings)
    Kf600,
    /// FUN_1015d310 (2019 FUN_10156a20): the Adventuring Primer's categories (guide00 rows, id - 0xf620)
    Guide,
    /// FUN_1015d1c0 (2019 FUN_101568d0): the Primer's topics (guide01 rows; +4 the image number, +5 the image count)
    GuidePage,
    /// FUN_10156b70: gil and other currencies
    Currency,
}

/// (first id, end id, kind, file per language ja/en/de/fr), from DAT_103662e8 and the language tables.
pub const RANGES: [(u32, u32, Kind, [u32; 4]); 18] = [
    (0x0000, 0x1000, Kind::General, [4, 73, 55815, 56235]),
    (0x2200, 0x2800, Kind::General, [55551, 55671, 55791, 56211]),
    (0x1000, 0x2000, Kind::Usable, [5, 74, 55816, 56236]),
    (0x2000, 0x2200, Kind::Automaton, [8, 77, 55819, 56239]),
    (0x2800, 0x4000, Kind::Armor, [7, 76, 55818, 56238]),
    (0x5a00, 0x7000, Kind::Armor, [55548, 55668, 55788, 56208]),
    (0x4000, 0x5a00, Kind::Weapon, [6, 75, 55817, 56237]),
    (0x7000, 0x7400, Kind::K7000, [55547, 55667, 55787, 56207]),
    (0x7400, 0x7800, Kind::K7400, [55550, 55670, 55790, 56210]),
    (0x7800, 0x7c00, Kind::General, [55555, 55675, 55675, 55675]),
    (0xe000, 0xeff8, Kind::Objective, [55552, 55672, 55792, 56212]),
    (0xeff8, 0xf000, Kind::Keff8, [95, 95, 95, 95]),
    (0xf000, 0xf200, Kind::Monster, [55549, 55669, 55789, 56209]),
    (0xf200, 0xf600, Kind::RoeCategory, [55553, 55673, 55793, 56213]),
    (0xf600, 0xf614, Kind::Kf600, [55504, 55504, 55504, 55504]),
    (0xf620, 0xf630, Kind::Guide, [55558, 55678, 55798, 56218]),
    (0xf630, 0xf720, Kind::GuidePage, [55557, 55677, 55797, 56217]),
    (0xffff, 0x1000f, Kind::Currency, [9, 91, 55820, 56240]),
];

/// A string-table field (FUN_1024cde0 / FUN_1024d0f0): text, or a number.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text(String),
    Num(u32),
}

#[derive(Clone, Debug, Default)]
pub struct Item {
    pub id: u32,
    pub kind: Option<Kind>,
    pub flags: u16,
    pub stack: u16,
    pub item_type: u16,
    /// +0xa (icon / resource id).
    pub resource: u16,
    pub targets: u16,
    pub level: u16,
    /// Equipment slot bits.
    pub slots: u16,
    pub races: u16,
    /// Job bits (bit n = job n).
    pub jobs: u32,
    pub superior_level: u16,
    pub shield_size: u16,
    pub damage: u16,
    pub delay: u16,
    pub dps: u16,
    pub skill: u8,
    pub max_charges: u8,
    pub cast_time: u8,
    pub use_delay: u16,
    pub reuse_delay: u32,
    /// Fields the parsers set whose use is not traced: (object offset, value).
    pub extra: Vec<(u16, u32)>,
    pub fields: Vec<Field>,
    pub name: String,
    /// English only (FUN_1014fda0): article type 0 "a", 1 "an", 2 "pair of", 3 "suit of".
    pub article: u32,
    pub singular: String,
    pub plural: String,
    pub description: String,
    icon: Option<Vec<u8>>,
}

fn u16_at(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}

fn u32_at(r: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(r[o..o + 4].try_into().unwrap())
}

/// FUN_1024cdc0 / FUN_1024cde0: u32 count, (offset, type) pairs; type 0 text starts 0x1c bytes in
/// after a u16 marker 1, type 1 is a u32.
pub fn string_table(b: &[u8]) -> Vec<Field> {
    let Some(n) = b.get(0..4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0..n.min(64) {
        let Some(e) = b.get(4 + i * 8..12 + i * 8) else { break };
        let (off, ty) = (u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize, u32::from_le_bytes(e[4..8].try_into().unwrap()));
        if ty == 0 {
            let s = b.get(off + 0x1c..).unwrap_or_default();
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            out.push(Field::Text(decode_sjis(&s[..end])));
        } else {
            out.push(Field::Num(b.get(off..off + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()))));
        }
    }
    out
}

fn text(f: Option<&Field>) -> String {
    match f {
        Some(Field::Text(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A 2019 record offset in this record: wide records (0x1400) put fields from +6 on 2 later, and those from the
/// jobs word on (`jobs`, where the kind has one) 2 more.
fn wide_at(wide: bool, jobs: Option<usize>, old: usize) -> usize {
    if !wide || old < 6 {
        old
    } else {
        old + 2 + if jobs.is_some_and(|j| old >= j) { 2 } else { 0 }
    }
}

/// Decodes one decrypted record (`wide`: a 0x1400-byte record).
pub fn parse(r: &[u8], kind: Kind, lang: Lang, wide: bool) -> Item {
    // Kinds without the shared header keep their layout; the jobs word follows a gap in the wide ones.
    let (shifted, jobs) = match kind {
        Kind::Automaton => (true, Some(0x10)),
        Kind::Armor | Kind::Weapon | Kind::K7400 => (true, Some(0x14)),
        Kind::General | Kind::Usable | Kind::K7000 | Kind::Monster | Kind::Currency => (true, None),
        _ => (false, None),
    };
    let at = |old: usize| wide_at(wide && shifted, jobs, old);
    let r16 = |old: usize| u16_at(r, at(old));
    let r32 = |old: usize| u32_at(r, at(old));
    let r8 = |old: usize| r[at(old)];
    let mut it = Item { id: u32_at(r, 0), kind: Some(kind), ..Default::default() };
    let header = |it: &mut Item| {
        it.flags = u16_at(r, 4);
        it.stack = r16(6);
        it.item_type = r16(8);
        it.resource = r16(0xa);
        it.targets = r16(0xc);
    };
    let x16 = |o: usize, old: usize| (o as u16, r16(old) as u32);
    let x8 = |o: usize, old: usize| (o as u16, r8(old) as u32);
    let x32 = |o: usize, old: usize| (o as u16, r32(old));
    // (string table offset, description count, icon)
    let (strings, descs, icon) = match kind {
        Kind::General => {
            header(&mut it);
            it.extra = vec![x16(0xd10, 0xe), x16(0xd14, 0x10), x16(0xd34, 0x12), x8(0xd54, 0x15), x16(0xd58, 0x16)];
            (Some(if wide { 0x1c } else { 0x18 }), 1, true)
        }
        Kind::Usable => {
            header(&mut it);
            it.extra = vec![x16(0xd08, 0xe), x16(0xd34, 0x10), x8(0xd54, 0x13), x16(0xd58, 0x14), x8(0xd9c, 0x16), x8(0xda0, 0x17), x8(0xda4, 0x18)];
            (Some(0x1c), 1, true)
        }
        Kind::Automaton => {
            header(&mut it);
            it.slots = r16(0xe);
            it.jobs = r32(0x10);
            it.level = r16(0x14);
            it.extra = vec![x16(0xd34, 0x16)];
            (Some(if wide { 0x1c } else { 0x18 }), 1, true)
        }
        Kind::Armor => {
            header(&mut it);
            it.level = r16(0xe);
            it.slots = r16(0x10);
            it.races = r16(0x12);
            it.jobs = r32(0x14);
            it.superior_level = r16(0x18);
            it.shield_size = r16(0x1a);
            it.max_charges = r8(0x1c);
            it.cast_time = r8(0x1d);
            it.use_delay = r16(0x1e);
            it.reuse_delay = r32(0x20);
            it.extra = vec![x16(0xd34, 0x24), x8(0xd50, 0x26), x8(0xd4c, 0x27), x8(0xd84, 0x28), x8(0xd9c, 0x29), x8(0xda0, 0x2a), x8(0xda4, 0x2b)];
            (Some(if wide { 0x30 } else { 0x2c }), 1, true)
        }
        Kind::Weapon => {
            header(&mut it);
            it.level = r16(0xe);
            it.slots = r16(0x10);
            it.races = r16(0x12);
            it.jobs = r32(0x14);
            it.superior_level = r16(0x18);
            it.damage = r16(0x1c);
            it.delay = r16(0x1e);
            it.dps = r16(0x20);
            it.skill = r8(0x22);
            it.max_charges = r8(0x28);
            it.cast_time = r8(0x29);
            it.use_delay = r16(0x2a);
            it.reuse_delay = r32(0x2c);
            it.extra = vec![x16(0xd18, 0x24), x16(0xd34, 0x30), x8(0xd50, 0x32), x8(0xd4c, 0x33), x8(0xd84, 0x34), x8(0xd9c, 0x35), x8(0xda0, 0x36), x8(0xda4, 0x37)];
            (Some(if wide { 0x3c } else { 0x38 }), 1, true)
        }
        Kind::K7000 => {
            header(&mut it);
            // The u32 at 0x14 is aligned in both layouts.
            it.extra = vec![x16(0xd08, 0xe), x16(0xd34, 0x10), (0xd30, u32_at(r, 0x14))];
            (Some(0x54), 1, true)
        }
        Kind::K7400 => {
            header(&mut it);
            it.level = r16(0xe);
            it.slots = r16(0x10);
            it.races = r16(0x12);
            it.jobs = r32(0x14);
            it.max_charges = r8(0x1a);
            it.cast_time = r8(0x1b);
            it.use_delay = r16(0x1c);
            it.reuse_delay = r32(0x20);
            it.extra = vec![x16(0xd2c, 0x18), x16(0xd34, 0x24)];
            (Some(if wide { 0x2c } else { 0x28 }), 1, true)
        }
        Kind::Objective => {
            it.extra = (0..7).map(|i| x32(0xd5c + i * 4, 4 + i * 4)).collect();
            (Some(0x20), 2, false)
        }
        Kind::Keff8 => {
            it.extra = vec![x32(0xd7c, 4), x32(0xd30, 8)];
            (None, 0, false)
        }
        Kind::Monster => {
            it.flags = u16_at(r, 4);
            let n = &r[at(6)..at(0x26)];
            it.name = decode_sjis(&n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())]);
            it.item_type = r16(0x26);
            it.resource = r16(0x2a);
            it.extra = vec![x16(0xd3c, 0x28), x16(0xd40, 0x2c), x16(0xd44, 0x2e), (0xd48, u32_at(r, if wide { 0x34 } else { 0x30 }))];
            // FUN_10155760: one string (the name).
            (Some(if wide { 0x74 } else { 0x70 }), 0, true)
        }
        Kind::RoeCategory => {
            it.extra = vec![x32(0xd78, 4), x32(0xd30, 8)];
            (Some(0x238), 1, false)
        }
        Kind::Kf600 => {
            it.extra = vec![x32(0xd80, 4)];
            (None, 0, false)
        }
        Kind::Guide => {
            it.extra = vec![x16(0xd90, 4), x16(0xd94, 6), x32(0xd98, 8)];
            (Some(0x108), 1, false)
        }
        Kind::GuidePage => {
            it.extra = vec![x8(0xd88, 4), x8(0xd8c, 5)];
            (Some(8), 1, false)
        }
        Kind::Currency => {
            header(&mut it);
            it.extra = vec![x16(0xd34, 0xe)];
            (Some(if wide { 0x14 } else { 0x10 }), 1, true)
        }
    };
    if let Some(at) = strings {
        it.fields = string_table(&r[at..]);
        let f = &it.fields;
        if !f.is_empty() {
            it.name = text(f.first());
        }
        if lang == Lang::En && f.len() >= 4 + descs && kind != Kind::Monster {
            it.article = if let Some(Field::Num(a)) = f.get(1) { *a } else { 0 };
            it.singular = text(f.get(2));
            it.plural = text(f.get(3));
        }
        if descs > 0 && f.len() > descs {
            it.description = f[f.len() - descs..].iter().map(|x| text(Some(x))).collect::<Vec<_>>().join("\n");
        }
    }
    if icon {
        let size = u32_at(r, 0x280) as usize;
        if (0x400..=r.len() - 0x284).contains(&size) {
            it.icon = Some(r[0x284..0x284 + size].to_vec());
        }
    }
    it
}

impl Item {
    /// The 32x32 icon as RGBA: flag byte, 16-byte name, BITMAPINFOHEADER, 256 BGRA colours,
    /// 8-bit pixels bottom-up. Palette alpha is 0..0x80.
    pub fn icon_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let d = self.icon.as_ref()?;
        let h = d.get(0x11..0x11 + 40)?;
        let (w, ht, bpp) = (u32_at(h, 4) as i32, u32_at(h, 8) as i32, u16_at(h, 14));
        if bpp != 8 || !(1..=64).contains(&w) || ht == 0 {
            return None;
        }
        let pal = d.get(0x11 + 40..0x11 + 40 + 1024)?;
        let px = d.get(0x11 + 40 + 1024..)?;
        let (wu, hu) = (w as usize, ht.unsigned_abs() as usize);
        let mut out = vec![0u8; wu * hu * 4];
        for y in 0..hu {
            let sy = if ht > 0 { hu - 1 - y } else { y };
            for x in 0..wu {
                let c = *px.get(sy * wu + x)? as usize * 4;
                let o = (y * wu + x) * 4;
                out[o] = pal[c + 2];
                out[o + 1] = pal[c + 1];
                out[o + 2] = pal[c];
                out[o + 3] = (pal[c + 3] as u32 * 2).min(255) as u8;
            }
        }
        Some((wu as u32, hu as u32, out))
    }

    /// Equipment slot names (bit order of the equipment window).
    pub fn slot_names(&self) -> Vec<&'static str> {
        const S: [&str; 16] = ["main", "sub", "range", "ammo", "head", "body", "hands", "legs", "feet", "neck", "waist", "ear1", "ear2", "ring1", "ring2", "back"];
        (0..16).filter(|i| self.slots & (1 << i) != 0).map(|i| S[i]).collect()
    }
}

/// Item lookup for one language: reads records straight from the files, each file's record size by its length.
pub struct Items {
    lang: Lang,
    paths: Vec<Option<(std::path::PathBuf, usize)>>,
}

impl Items {
    pub fn load(index: &DatIndex, lang: Lang) -> Result<Items> {
        let stride = |p: &std::path::Path, n: u32| match std::fs::metadata(p).map(|m| m.len()) {
            Ok(len) if len == n as u64 * 0x1400 => 0x1400,
            _ => 0xc00,
        };
        let paths: Vec<_> = RANGES.iter().map(|r| index.path(r.3[lang as usize]).map(|p| (stride(&p, r.1 - r.0), p)).map(|(n, p)| (p, n))).collect();
        paths.iter().any(|p| p.is_some()).then_some(()).context("no item files")?;
        Ok(Items { lang, paths })
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// A Primer category's topic ids (FUN_1015d310: u16 count @6, u32 ids from @8).
    pub fn guide_pages(&self, id: u32) -> Vec<u32> {
        let Some((Kind::Guide, r)) = self.record(id) else { return Vec::new() };
        let n = u16::from_le_bytes([r[6], r[7]]) as usize;
        r[8..].chunks_exact(4).take(n.min(64)).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }

    /// Decrypted record bytes of `id`.
    pub fn record(&self, id: u32) -> Option<(Kind, Vec<u8>)> {
        let i = RANGES.iter().position(|r| (r.0..r.1).contains(&id))?;
        let (path, size) = self.paths[i].as_ref()?;
        let mut f = std::fs::File::open(path).ok()?;
        f.seek(SeekFrom::Start((id - RANGES[i].0) as u64 * *size as u64)).ok()?;
        let mut r = vec![0u8; *size];
        f.read_exact(&mut r).ok()?;
        r.iter_mut().for_each(|b| *b = b.rotate_right(5));
        Some((RANGES[i].2, r))
    }

    pub fn item(&self, id: u32) -> Option<Item> {
        let (kind, r) = self.record(id)?;
        let it = parse(&r, kind, self.lang, r.len() == 0x1400);
        (it.id == id).then_some(it)
    }

    pub fn icon(&self, id: u32) -> Option<(u32, u32, Vec<u8>)> {
        self.item(id)?.icon_rgba()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Primer: "Basics" (0xf620) lists 56 topics from "Movement" (0xf631: image number 1, three images).
    #[test]
    fn primer_records() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(items) = Items::load(&index, Lang::En) else { return };
        let pages = items.guide_pages(0xf620);
        assert_eq!((pages.len(), pages.first()), (56, Some(&0xf631)));
        assert_eq!(items.item(0xf620).map(|i| i.name), Some("Basics".to_string()));
        let m = items.item(0xf631).unwrap();
        let extra = |at: u16| m.extra.iter().find(|e| e.0 == at).map(|e| e.1);
        assert_eq!((m.name.as_str(), extra(0xd88), extra(0xd8c)), ("Movement", Some(1), Some(3)));
    }

    #[test]
    fn table_fields() {
        // count 3: text "Ab", number 1, text "c"
        let mut b = vec![0u8; 0x80];
        b[0] = 3;
        let ent = [(0x1c, 0), (0x40, 1), (0x44, 0)];
        for (i, (o, t)) in ent.iter().enumerate() {
            b[4 + i * 8] = *o;
            b[8 + i * 8] = *t;
        }
        b[0x1c] = 1;
        b[0x38..0x3a].copy_from_slice(b"Ab");
        b[0x40] = 1;
        b[0x44] = 1;
        b[0x60] = b'c';
        assert_eq!(string_table(&b), vec![Field::Text("Ab".into()), Field::Num(1), Field::Text("c".into())]);
    }

    #[test]
    fn real_items() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(items) = Items::load(&index, Lang::En) else { return };
        let s = items.item(0x4097).expect("bronze sword");
        assert_eq!((s.name.as_str(), s.level, s.damage, s.delay, s.skill), ("Bronze Sword", 1, 6, 231, 3));
        let b = items.item(1).expect("chocobo bedding");
        assert_eq!((b.singular.as_str(), b.plural.as_str(), b.article), ("pile of chocobo bedding", "piles of chocobo bedding", 0));
        assert_eq!(b.icon_rgba().map(|i| (i.0, i.1)), Some((32, 32)));
    }
}
