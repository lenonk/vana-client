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

/// The DLL's prefix table (FUN_10142070: one English table for both prefixes).
fn prefixes() -> &'static [String] {
    static T: OnceLock<Vec<String>> = OnceLock::new();
    T.get_or_init(|| {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).unwrap_or_default();
        crate::dat::tables::mon_prefixes(&dll).map_err(|e| log::warn!("{e:#}")).unwrap_or_default()
    })
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
fn species(w: &World, word: u16) -> Option<Option<String>> {
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
        && let Some(s) = species(w, key.0)
    {
        w.my_mon_name = Some(names(prefixes(), s.as_deref(), key.1));
        w.my_mon_key = key;
    }
    // FUN_10099730: the party records' short names.
    let wanted: Vec<(usize, u16, [u8; 2])> = w.party.records_mut().iter().enumerate().filter(|(_, r)| r.id != 0 && r.mon != 0 && r.mon_short.is_empty()).map(|(i, r)| (i, r.mon, r.mon_ids)).collect();
    for (i, word, ids) in wanted {
        if let Some(s) = species(w, word) {
            w.party.records_mut()[i].mon_short = names(prefixes(), s.as_deref(), ids).1;
        }
    }
    let stale: Vec<(u32, u16, [u8; 2])> = w.entities.iter().filter(|(_, e)| e.monstrosity != 0 && e.mon_key != (e.monstrosity, e.mon_prefix)).map(|(id, e)| (*id, e.monstrosity, e.mon_prefix)).collect();
    for (id, word, ids) in stale {
        if let Some(s) = species(w, word)
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
    use super::names;

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
