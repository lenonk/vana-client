//! Teleport points (FUN_101f9420): item records 0xf600..0xf613 (file 55504), 32 points of 20 bytes from record + 4.
//! The map marks them; op 0xd4 sub 1 names one per menu option.

use super::items::Items;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Global id (op 0xd4 sub 1's val@6).
    pub id: u16,
    pub kind: u16,
    pub zone: u16,
    /// The sub-map key (map +0x102).
    pub map: u8,
    /// Its bit in the kind's mask (s2c 0x063 type 6).
    pub bit: u8,
    /// World position (thousandths in the file); `y` is the height.
    pub x: f32,
    pub z: f32,
    pub y: f32,
}

/// Point kinds (table 0x10382190): name, marker icon (map sprite list index), the mask's byte offset in s2c 0x063
/// type 6 (FUN_101cbc70 and siblings; none: always registered). Kinds past the table read row 0.
pub const KINDS: [(&str, u8, Option<usize>); 12] = [
    ("Home Point", 0x0c, Some(0x00)),
    ("Telepoint", 0x0c, Some(0x30)),
    ("Survival Guide", 0x0d, Some(0x10)),
    ("Waypoint", 0x13, Some(0x20)),
    ("Beginner", 0x10, None),
    ("ZoneChange", 0x0e, None),
    ("Atmos", 0x0f, None),
    ("Up", 0x11, None),
    ("Down", 0x12, None),
    ("Atmos(S)", 0x0f, Some(0x34)),
    ("Eschan Portal", 0x13, Some(0x38)),
    ("Ethereal Ingress", 0x13, None),
];

/// FUN_101f97a0's row: kinds 12 and up count as 0.
pub fn kind_row(kind: u16) -> u16 {
    if (kind as usize) < KINDS.len() { kind } else { 0 }
}

/// The point is registered: its bit in the kind's mask is set (bit / 32 words on from the offset).
pub fn registered(masks: &[u8; 64], kind: u16, bit: u8) -> bool {
    let Some(off) = KINDS[kind_row(kind) as usize].2 else { return true };
    let at = off + (bit as usize >> 5) * 4;
    masks.get(at..at + 4).is_some_and(|b| u32::from_le_bytes(b.try_into().unwrap()) >> (bit & 31) & 1 != 0)
}

/// One decrypted record's points (empty slots, zone 0, left out).
pub fn parse(r: &[u8]) -> Vec<Point> {
    let u16_at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
    let f = |o: usize| i32::from_le_bytes(r[o..o + 4].try_into().unwrap()) as f32 * 0.001;
    (0..32)
        .map(|k| 4 + 20 * k)
        .take_while(|&o| o + 20 <= r.len())
        .filter(|&o| u16_at(o + 4) != 0)
        .map(|o| Point { id: u16_at(o), kind: u16_at(o + 2), zone: u16_at(o + 4), map: r[o + 6], bit: r[o + 7], x: f(o + 8), z: f(o + 12), y: f(o + 16) })
        .collect()
}

/// The whole table, in record order.
pub fn load(items: &Items) -> Vec<Point> {
    (0xf600..0xf614).filter_map(|id| items.record(id)).flat_map(|(_, r)| parse(&r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::DatIndex;
    use crate::dat::items::Lang;

    #[test]
    fn table() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(items) = Items::load(&index, Lang::En) else { return };
        let p = load(&items);
        assert_eq!(p.len(), 576);
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let hp1 = p[0];
        assert_eq!((hp1.id, hp1.kind, hp1.zone, hp1.map, hp1.bit), (0, 0, 230, 0, 0));
        assert!(near(hp1.x, -85.468) && near(hp1.z, -66.454) && near(hp1.y, 1.0));
        let hp4 = p.iter().find(|p| p.id == 97).unwrap();
        assert!(hp4.kind == 0 && hp4.zone == 230 && near(hp4.x, -165.0) && near(hp4.z, 11.0));
        let sg = p.iter().find(|p| p.id == 152).unwrap();
        assert_eq!((sg.kind, sg.zone, sg.bit), (2, 231, 24));
        let ing = p.iter().find(|p| p.id == 631).unwrap();
        assert_eq!((ing.kind, ing.zone, ing.map), (11, 291, 1));
    }

    #[test]
    fn masks() {
        let mut m = [0u8; 64];
        m[12] = 2;
        assert!(registered(&m, 0, 97) && !registered(&m, 0, 96) && !registered(&m, 2, 97));
        // Beginner points have no mask; a kind past the table reads the Home Point mask.
        assert!(registered(&m, 4, 0) && registered(&m, 12, 97));
        m[0x30] = 1;
        assert!(registered(&m, 1, 0) && !registered(&m, 1, 1));
    }
}
