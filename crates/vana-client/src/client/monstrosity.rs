//! Monstrosity (main job MON): who is in it and the names Monipulators show. Spec
//! `~/ffxi/lab/research/monstrosity_spec.md`.

use super::{Entity, World};
use std::sync::OnceLock;

/// FUN_10097d50: the 0x00A flag (@0x7E) is set, or the main job (0x061) is MON.
pub fn active(w: &World) -> bool {
    w.mon_login || w.stats.as_ref().is_some_and(|s| s.main.0 == 23)
}

/// 0x063 types 3 / 4 (FUN_10099270): the blocks into G1 / G2; a type 4 asks for the moves (DAT_104804ce).
pub fn on_data(w: &mut World, kind: u16, data: Vec<u8>) {
    let m = w.monstrosity.get_or_insert((0, 0, 0));
    m.0 |= 1 << kind;
    if kind == 3 {
        let u = |o: usize| data.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        // G1 +4 the ranks (Monstrosity bits 0..1, gladiator 2..3), +0xc prestige.
        (m.1, m.2) = (u(4), u(0xc) as u16);
        w.mon_g1 = data;
    } else {
        w.mon_g2 = data;
        w.mon_due = true;
    }
}

/// FUN_10093b30 in the zone-in reset: the blocks, the moves and the data flag go (they come again after GAMEOK).
pub fn zone_reset(w: &mut World) {
    (w.mon_g1, w.mon_g2, w.mon_due, w.mon_in, w.mon_moves) = (Vec::new(), Vec::new(), false, false, [(0, 0, 0); 16]);
    w.monstrosity = None;
}

/// FUN_100997b0 each frame: after a type 4, the species record's 16 moves (id + 0x600, 0 kept) and their bits
/// (FUN_100e6080 clears 0x700..0xaff first); waits for the record.
pub fn moves(w: &mut World) {
    if !w.mon_due || !w.actors.contains_key(&w.my_id) {
        return;
    }
    // The cleared moves go in first (G+0x2cf4a set), then the record's when it reads.
    (w.mon_moves, w.move_bits) = ([(0, 0, 0); 16], Some(vec![0; 0x80]));
    let species = w.mon_g1.get(0..2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) & 0x1ff;
    let mut moves = [(0u16, 0u8, 0u8); 16];
    if species != 0 {
        let Some(it) = w.item_info.as_ref().and_then(|c| c.item(species.wrapping_sub(0x1000))) else { return };
        for (i, m) in moves.iter_mut().enumerate() {
            let v = it.extra.iter().find(|e| e.0 == 0xd48 + 4 * i as u16).map_or(0, |e| e.1);
            let id = v as u16;
            *m = (if id == 0 { 0 } else { id + 0x600 }, (v >> 16) as u8, (v >> 24) as u8);
        }
    }
    let mut bits = vec![0u8; 0x80];
    for &(id, ..) in &moves {
        if id >= 0x700 && ((id >> 3) as usize) < 0x160 {
            bits[(id >> 3) as usize - 0xe0] |= 1 << (id & 7);
        }
    }
    (w.mon_moves, w.move_bits, w.mon_due, w.mon_in) = (moves, Some(bits), false, true);
}

/// FUN_100e6020: the ability bitfield (0x0AC's 0xe0 bytes, then the move bits to 0xaff), once both are in.
pub fn ability_bits(w: &World) -> Option<Vec<u8>> {
    let moves = w.move_bits.as_ref()?;
    (w.commands_known.len() >= 0xe0).then(|| [&w.commands_known[..0xe0], moves].concat())
}

/// FUN_102231f0: one of the 16 moves, its form bits holding our form.
pub fn is_move(w: &World, id: u16) -> bool {
    w.mon_in && w.mon_moves.iter().any(|m| m.0 == id && m.2 & (1 << (w.my_form & 7)) != 0)
}

/// The DLL's prefix table (FUN_10142070: one English table for both prefixes) and the species types before it.
fn dll_tables() -> &'static (Vec<String>, Vec<u8>) {
    static T: OnceLock<(Vec<String>, Vec<u8>)> = OnceLock::new();
    T.get_or_init(|| {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).unwrap_or_default();
        let prefixes = crate::dat::tables::mon_prefixes(&dll).map_err(|e| log::warn!("{e:#}")).unwrap_or_default();
        (prefixes, crate::dat::tables::mon_types(&dll).unwrap_or_default())
    })
}

fn prefixes() -> &'static [String] {
    &dll_tables().0
}

/// Prefix `id`'s text (the names list; id 0 empty).
pub fn prefix_text(id: u8) -> &'static str {
    prefixes().get(id as usize).map_or("", String::as_str)
}

/// A species code's type for Refine by Type (0x10370280).
pub fn species_type(code: u16) -> u8 {
    dll_tables().1.get(code as usize).copied().unwrap_or(0)
}

/// File 81's `mon_` chunk (DAT_1057829c, FUN_101671d0): 0x200 u16 species codes from +0x10, zeros skipped.
pub fn order(file: &[u8]) -> Vec<u16> {
    let Some(o) = (0..file.len().saturating_sub(16)).step_by(16).find(|&o| &file[o..o + 4] == b"mon_") else { return Vec::new() };
    file.get(o + 16..o + 16 + 0x400).unwrap_or_default().chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).filter(|&v| v != 0).collect()
}

/// The Feretory (FUN_10141b80: zone 285), where Customization may change anything.
pub fn in_feretory(w: &World) -> bool {
    w.zone == Some(285)
}

/// The 0x044 MON block (side 0, job 0x17): packet @(4 + k) at k.
fn block(w: &World) -> Option<&[u8; 0x9c]> {
    Some(&w.ext_job[0]).filter(|b| b[0] == 0x17)
}

fn block16(w: &World, k: usize) -> u16 {
    block(w).map_or(0, |b| u16::from_le_bytes([b[k], b[k + 1]]))
}

/// FUN_100fd480: the current species (0x044 @8).
pub fn current_species(w: &World) -> u16 {
    block16(w, 4)
}

/// FUN_100fd4a0: 0x044 @0xA, the instinct cap tier (the Monstrosity rank).
pub fn rank(w: &World) -> u8 {
    block(w).map_or(0, |b| b[6])
}

/// FUN_100fd4c0: instinct slot i (0x044 @0xC + 2i; n = item 0x7400 + n, 0 empty).
pub fn slot(w: &World, i: usize) -> u16 {
    if i < 12 { block16(w, 8 + 2 * i) } else { 0 }
}

/// FUN_100fd4f0: name descriptor i (0x044 @0x24 + i).
pub fn descriptor(w: &World, i: usize) -> u8 {
    block(w).map_or(0, |b| b[0x20 + i.min(1)])
}

/// The 0x044 block's first bytes (job 0x17, side) for c2s 0x102.
pub fn block_head(w: &World) -> Option<[u8; 2]> {
    block(w).map(|b| [b[0], b[1]])
}

/// FUN_10087b20: the Monstrosity data is in (a 0x063 type 4 came).
pub fn data_in(w: &World) -> bool {
    !w.mon_g2.is_empty()
}

/// FUN_10087b30: a base species' level, 0 when not owned (codes < 0x80 in G1 +0x54, 0x80..0xff in G2 +0).
pub fn level(w: &World, code: u16) -> u8 {
    let at = match code {
        0..0x80 => w.mon_g1.get(0x54 + code as usize),
        0x80..0x100 => w.mon_g2.get(code as usize - 0x80),
        _ => None,
    };
    at.copied().filter(|_| data_in(w)).unwrap_or(0)
}

/// FUN_10087b70: variant code `v` (0x100..0x1ff) owned (G2 +0x8c bits).
pub fn variant(w: &World, v: u16) -> bool {
    let i = v.wrapping_sub(0x100) as usize;
    data_in(w) && i < 0x100 && w.mon_g2.get(0x8c + i / 8).is_some_and(|b| b >> (i % 8) & 1 != 0)
}

/// Species code `v` in the species list: a base with a level, a variant with its bit.
pub fn species_owned(w: &World, v: u16) -> bool {
    if v < 0x100 { level(w, v) != 0 } else { variant(w, v) }
}

/// FUN_100fda90 over G1 +0x14: a family's 2-bit instinct count.
fn instinct_count(g1: &[u8], s: u16) -> u8 {
    let d = g1.get(0x14 + (s as usize >> 4) * 4..0x18 + (s as usize >> 4) * 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    if s < 0x100 { (d >> ((s & 15) * 2) & 3) as u8 } else { 0 }
}

/// FUN_100fdad0: instinct n owned: n < 0x300 when its family's count passes n % 3, 0x300..0x35f by the bought bit (G2 +0x80).
pub fn instinct_owned(w: &World, n: u16) -> bool {
    match n {
        0..0x300 => instinct_count(&w.mon_g1, n / 3) > (n % 3) as u8,
        0x300..0x360 => w.mon_g2.get(0x80 + (n as usize - 0x300) / 8).is_some_and(|b| b >> ((n - 0x300) % 8) & 1 != 0),
        _ => false,
    }
}

/// FUN_100fda60: the instinct cost cap, main level + 10 / 30 / 50 by the rank byte.
pub fn cap(level: u8, rank: u8) -> u32 {
    level as u32 + [10, 30, 50].get(rank as usize).copied().unwrap_or(0)
}

fn prefix(table: &[String], id: u8) -> Option<&str> {
    (id != 0).then(|| table.get(id as usize).map_or("", String::as_str))
}

/// FUN_10093460 (full, 31 bytes) and FUN_100935c0 (short, 23): "P1 P2 Species" and "P1[0].P2[0]. Species", each part
/// optional.
pub fn names(table: &[String], species: Option<&str>, ids: [u8; 2]) -> (String, String) {
    let (p1, p2) = (prefix(table, ids[0]), prefix(table, ids[1]));
    let mut full: Vec<&str> = [p1, p2].into_iter().flatten().collect();
    // An empty prefix's initial is the C string's end.
    let mut short: Vec<char> = [p1, p2].into_iter().flatten().flat_map(|p| [p.chars().next().unwrap_or('\0'), '.']).collect();
    if let Some(s) = species {
        full.push(s);
        if p1.is_some() || p2.is_some() {
            short.push(' ');
        }
        short.extend(s.chars());
    }
    (full.join(" ").chars().take(31).collect(), short.into_iter().take_while(|&c| c != '\0').take(23).collect())
}

/// The species name of a word (+0x210): item 0x7000 + word (0x8000 | s -> 0xf000 + s); only species records have one.
fn species_name(w: &World, word: u16) -> Option<Option<String>> {
    let cache = w.item_info.as_ref()?;
    let it = cache.item(word.wrapping_add(0x7000));
    Some(it.filter(|i| i.kind == Some(crate::dat::items::Kind::Monster)).map(|i| i.name.clone()))
}

/// FUN_1008e870 each frame: a Monipulator's names are rebuilt when its word or prefixes change, once its record reads.
pub fn refresh(w: &mut World) {
    let key = (w.my_plate.monstrosity, w.my_plate.mon_prefix);
    if key.0 == 0 {
        w.my_mon_name = None;
    } else if w.my_mon_key != key
        && let Some(s) = species_name(w, key.0)
    {
        w.my_mon_name = Some(names(prefixes(), s.as_deref(), key.1));
        w.my_mon_key = key;
    }
    // FUN_10099730: the party records' short names.
    let wanted: Vec<(usize, u16, [u8; 2])> = w.party.records_mut().iter().enumerate().filter(|(_, r)| r.id != 0 && r.mon != 0 && r.mon_short.is_empty()).map(|(i, r)| (i, r.mon, r.mon_ids)).collect();
    for (i, word, ids) in wanted {
        if let Some(s) = species_name(w, word) {
            w.party.records_mut()[i].mon_short = names(prefixes(), s.as_deref(), ids).1;
        }
    }
    let stale: Vec<(u32, u16, [u8; 2])> = w.entities.iter().filter(|(_, e)| e.monstrosity != 0 && e.mon_key != (e.monstrosity, e.mon_prefix)).map(|(id, e)| (*id, e.monstrosity, e.mon_prefix)).collect();
    for (id, word, ids) in stale {
        if let Some(s) = species_name(w, word)
            && let Some(e) = w.entities.get_mut(&id)
        {
            e.mon_name = Some(names(prefixes(), s.as_deref(), ids));
            e.mon_key = (word, ids);
        }
    }
}

/// The name an actor shows (`+0x210 ? +0x214 : +0x7c`): plates, the target window, battle lines, event text.
pub fn name(e: &Entity) -> &str {
    match &e.mon_name {
        Some((full, _)) if e.monstrosity != 0 => full,
        _ => &e.name,
    }
}

/// Ours.
pub fn my_name(w: &World) -> &str {
    match &w.my_mon_name {
        Some((full, _)) if w.my_plate.monstrosity != 0 => full,
        _ => &w.name,
    }
}

/// A main party row's name in our zone (FUN_10202530): the short name while the record has a word, else the character's.
pub fn row_name(m: &super::party::Member) -> &str {
    if m.mon != 0 && !m.mon_short.is_empty() { &m.mon_short } else { &m.name }
}

/// An id's shown name.
pub fn name_of(w: &World, id: u32) -> Option<&str> {
    if id == w.my_id { Some(my_name(w)) } else { w.entities.get(&id).map(name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instinct_counts_and_cap() {
        // Family 1 count 2 (bits 2..3 of dword 0), family 0x11 count 3 (dword 1 bits 2..3).
        let mut g1 = vec![0u8; 0xd4];
        g1[0x14] = 0b1000;
        g1[0x18] = 0b1100;
        assert_eq!((instinct_count(&g1, 1), instinct_count(&g1, 0x11), instinct_count(&g1, 0)), (2, 3, 0));
        assert_eq!((cap(50, 0), cap(50, 1), cap(50, 2), cap(50, 3)), (60, 80, 100, 50));
    }

    /// The type table and the mon_ order from the real files: Rabbit first, 160 codes; Rabbit 13, Slime 19.
    #[test]
    fn tables() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(f) = index.read(81) else { return };
        let o = order(&f);
        assert_eq!((o.len(), o.first(), o.get(1), o.get(3)), (160, Some(&1), Some(&0x100), Some(&0x102)));
        if !dll_tables().1.is_empty() {
            assert_eq!((species_type(1), species_type(0xfe), species_type(0x100), species_type(0x1fc)), (13, 19, 13, 19));
        }
    }

    #[test]
    fn spec_examples() {
        let mut t = vec![String::new(); 256];
        (t[1], t[4], t[0x72]) = ("Abashed".into(), "Ace".into(), "King".into());
        assert_eq!(names(&t, Some("Rabbit"), [1, 4]), ("Abashed Ace Rabbit".into(), "A.A. Rabbit".into()));
        assert_eq!(names(&t, Some("Tiger"), [0, 0x72]), ("King Tiger".into(), "K. Tiger".into()));
        assert_eq!(names(&t, Some("Rabbit"), [0, 0]), ("Rabbit".into(), "Rabbit".into()));
        assert_eq!(names(&t, None, [1, 4]), ("Abashed Ace".into(), "A.A.".into()));
    }
}
