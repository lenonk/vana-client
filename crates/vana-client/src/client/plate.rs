//! Name plate icons (nameplate_icons_spec.md): fontshp glyph bytes 0x8e..0xb1 put in front of the name (FUN_10097840),
//! the job master stars on a second line (FUN_100979b0), and the target window's glyph (FUN_101501a0).

use super::net::{Marks, PlateExtra};

/// What the glyph chain reads off an actor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plate {
    /// A PC (id & 0xff000000 == 0); an NPC (id >> 24 == 1).
    pub pc: bool,
    pub npc: bool,
    pub marks: Marks,
    pub extra: PlateExtra,
    /// +0x210 the Monstrosity word, 0x067 @0x10 (bit 1 Campaign, bit 2 Level Sync), the NPC MonStat (+0x124 bits 1..3).
    pub monstrosity: u16,
    pub sync: u8,
    pub mon_stat: u8,
}

/// Table 0x1035b61c: the GM badge by level.
const GM: [u8; 8] = [0, 0x93, 0x93, 0x95, 0x96, 0x97, 0x98, 0x99];

/// FUN_10097500 slot 0: the first match wins; `default` is the linkshell pearl or 0.
fn main_glyph(p: &Plate, default: u8) -> u8 {
    let m = &p.marks;
    if m.pol {
        return 0x8e;
    }
    if m.link_dead {
        return 0x8f;
    }
    if p.monstrosity != 0 {
        return 0xab;
    }
    if let Some(g) = team(p) {
        return g.unwrap_or(default);
    }
    if m.away {
        if !p.npc {
            return 0x90;
        }
        return match p.mon_stat {
            5 => 0xae,
            6 => 0xaf,
            7 => 0xb0,
            _ => default,
        };
    }
    if m.gm != 0 && !m.gm_icon_off {
        return GM[m.gm as usize & 7];
    }
    if p.sync & 2 != 0 {
        return 0x94;
    }
    if m.lfg_master {
        return 0xac;
    }
    if m.auto {
        return 0x9d;
    }
    if m.lfg {
        return 0x91;
    }
    if p.sync & 4 != 0 {
        return 0xa8;
    }
    if p.extra.trial {
        return 0x93;
    }
    if m.mentor {
        return if p.pc { 0xa2 } else { 0xb1 };
    }
    if m.new_char {
        return 0xa1;
    }
    if m.bazaar {
        return 0x9c;
    }
    default
}

fn on_team(p: &Plate) -> bool {
    let a = p.marks.alleg;
    (2..100).contains(&a) && !(0x28..=0x2b).contains(&a)
}

/// Rule 4: a Campaign / Ballista side (allegiance 2..99 but 0x28..0x2b): Some(glyph or None = the default).
/// *(inf: the Petra word +0x29c is not read; LSB has no Ballista)*
fn team(p: &Plate) -> Option<Option<u8>> {
    if !on_team(p) {
        return None;
    }
    let a = p.marks.alleg;
    let i = if p.extra.ballista != 0 { p.extra.ballista } else if a < 7 { a - 1 } else { 0 };
    let g = match a {
        0x20 | 0x22 | 0x24 | 0x26 => Some(0xa6),
        0x21 | 0x23 | 0x25 | 0x27 => Some(0xa7),
        _ => match i {
            1 => Some(0x9e),
            2 => Some(0x9f),
            3 => Some(0xa0),
            4 | 6 => Some(0xa3),
            5 | 7 => Some(0xa4),
            _ => None,
        },
    };
    Some(g)
}

/// FUN_10097730: the badge after the main glyph, for a team member or in Campaign.
fn badge(p: &Plate) -> u8 {
    if !on_team(p) && p.sync & 2 == 0 {
        return 0;
    }
    let m = &p.marks;
    match () {
        _ if m.lfg_master => 0xac,
        _ if m.auto => 0x9d,
        _ if m.lfg => 0x91,
        _ if p.sync & 4 != 0 => 0xa8,
        _ => 0,
    }
}

/// FUN_100977a0: the Belligerency rank glyph (allegiance 8 or 9), 0xc8..0xcd.
fn rank(p: &Plate) -> u8 {
    if !matches!(p.marks.alleg, 8 | 9) {
        return 0;
    }
    if p.monstrosity != 0 {
        [0xc8, 0xc9, 0xca, 0][(p.extra.ranks >> 2 & 3) as usize]
    } else {
        [0xcb, 0xcc, 0xcd, 0][(p.extra.ranks & 3) as usize]
    }
}

/// FUN_10097840: the bytes before the name, and the icon colour (the linkshell colour for the pearl, else 0x80808080).
pub fn prefix(p: &Plate) -> (Vec<u8>, u32) {
    let default = if p.pc && p.marks.ls { 0x92 } else { 0 };
    let mut a = main_glyph(p, default);
    let x = badge(p);
    let c = rank(p);
    let mut out = Vec::new();
    if c != 0 {
        if a != 0xab {
            a = 0;
        }
        out.push(c);
    }
    if a != 0 {
        out.push(a);
        if x != 0 {
            out.push(x);
        }
        out.push(b' ');
    }
    let [r, g, b] = p.marks.ls_colour;
    let colour = if a == 0x92 { 0x8000_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32 } else { 0x8080_8080 };
    (out, colour)
}

/// FUN_100979b0: line 2, the job master stars.
pub fn stars(p: &Plate) -> bool {
    p.pc && p.extra.job_master
}

/// The target window's glyph (FUN_101501a0): 0xa9 for a Belligerency rank, else the main glyph without the pearl.
pub fn target_glyph(p: &Plate) -> Option<u8> {
    if rank(p) != 0 {
        return Some(0xa9);
    }
    Some(main_glyph(p, 0)).filter(|&g| g != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(m: Marks) -> Plate {
        Plate { pc: true, marks: Marks { alleg: 1, ..m }, ..Plate::default() }
    }

    #[test]
    fn gm_badges_by_level() {
        let got: Vec<Vec<u8>> = (0..8).map(|g| prefix(&pc(Marks { gm: g, ..Marks::default() })).0).collect();
        assert_eq!(got[0], Vec::<u8>::new());
        assert_eq!(got[1], [0x93, b' ']);
        assert_eq!(got[3], [0x95, b' ']);
        assert_eq!(got[6], [0x98, b' ']);
        assert_eq!(got[7], [0x99, b' ']);
        // GmIconFlag drops the badge only.
        assert!(prefix(&pc(Marks { gm: 5, gm_icon_off: true, ..Marks::default() })).0.is_empty());
    }

    #[test]
    fn priority_and_pearl() {
        // Away beats GM; the pearl only when nothing else matched, in the shell's colour.
        assert_eq!(prefix(&pc(Marks { gm: 4, away: true, ..Marks::default() })).0, [0x90, b' ']);
        let (b, c) = prefix(&pc(Marks { ls: true, ls_colour: [0x10, 0x20, 0x30], ..Marks::default() }));
        assert_eq!((b, c), (vec![0x92, b' '], 0x8010_2030));
        let (b, c) = prefix(&pc(Marks { ls: true, bazaar: true, ..Marks::default() }));
        assert_eq!((b, c), (vec![0x9c, b' '], 0x8080_8080));
        assert_eq!(prefix(&pc(Marks { mentor: true, new_char: true, ..Marks::default() })).0, [0xa2, b' ']);
        assert_eq!(prefix(&pc(Marks { new_char: true, bazaar: true, ..Marks::default() })).0, [0xa1, b' ']);
        assert_eq!(prefix(&pc(Marks { lfg_master: true, lfg: true, ..Marks::default() })).0, [0xac, b' ']);
        assert_eq!(target_glyph(&pc(Marks { ls: true, ..Marks::default() })), None);
    }

    #[test]
    fn campaign_and_ranks() {
        // A San d'Oria side (allegiance 2) with a seeking-party badge; Campaign alone gives the sword.
        let p = Plate { pc: true, marks: Marks { alleg: 2, lfg: true, ..Marks::default() }, ..Plate::default() };
        assert_eq!(prefix(&p).0, [0x9e, 0x91, b' ']);
        let p = Plate { pc: true, marks: Marks { alleg: 1, ..Marks::default() }, sync: 2 | 4, ..Plate::default() };
        assert_eq!(prefix(&p).0, [0x94, 0xa8, b' ']);
        // Belligerency gladiator rank 2: the rank glyph, no main glyph, no space.
        let p = Plate { pc: true, marks: Marks { alleg: 8, ..Marks::default() }, extra: PlateExtra { ranks: 2, ..PlateExtra::default() }, ..Plate::default() };
        assert_eq!(prefix(&p).0, [0xcd]);
        assert_eq!(target_glyph(&p), Some(0xa9));
    }
}
