//! The compass's radar dots (FUN_102203b0; spec ~/ffxi/lab/research/radar_dots_spec.md): every entity within range,
//! coloured by its kind bits (FUN_1008e290), faded with distance and height, on the compass's rotated disk.

use super::{Entity, World};

/// The compass's job-dependent fields, sampled when it opens (FUN_1021ef60 -> FUN_1021ee70): +0x30 scale, +0x34
/// range, +0x38 the fade start, +0x2f monsters shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub scale: f32,
    pub range: f32,
    pub fade: f32,
    pub mobs: bool,
}

/// FUN_10215200's placeholders until the first open.
impl Default for Sample {
    fn default() -> Self {
        Sample { scale: 1.0, range: 32.0, fade: 16.0, mobs: false }
    }
}

/// From s2c 0x061's jobs (main, sub, sub level): NIN main or sub widens it (36 yalms, scale 1), THF / BST / RNG / NIN /
/// SMN / BLU main, or sub with a level, show monsters.
pub fn sample(main: u8, sub: u8, sub_level: u8) -> Sample {
    let nin = main == 13 || sub == 13;
    let scale = if nin { 1.0 } else { 0.8 };
    let range = scale * 36.0;
    let seer = |j: u8| matches!(j, 6 | 9 | 11 | 13 | 15 | 16);
    Sample { scale, range, fade: range * 0.5, mobs: seer(main) || (sub_level != 0 && seer(sub)) }
}

/// FUN_1008e290: the entity's kind word (+0x1d0), rebuilt every frame.
pub fn kind_bits(w: &World, id: u32, e: &Entity) -> u32 {
    let pc = id >> 24 == 0;
    let mut b = if pc {
        1
    } else if e.mob {
        0x10
    } else {
        2
    };
    if w.party.party().any(|m| m.id == id) {
        b |= 4;
    }
    if w.party.members().any(|m| m.id == id) {
        b |= 8;
    }
    b |= match e.sub_kind {
        2 => 0x20,
        3 => 0x40,
        4 => 0x80,
        _ => 0,
    };
    if !pc && e.marks.charm {
        b |= 0x100;
    }
    if id == w.my_id {
        b |= 0x200;
    }
    if (2..=99).contains(&e.marks.alleg) {
        b |= 0x400;
    }
    if !pc && !e.marks.trust && w.my_fellow != 0 && e.index == w.my_fellow {
        b |= 0x800;
    }
    if !pc && e.marks.trust {
        b |= 0x1000;
    }
    b
}

/// FUN_10098210(0) as far as we have its inputs: no door / lift / ship, not hidden or asleep (@0x20 bits 1 / 2), not
/// invisible, not untargetable or radar-off, not an object model. Not modelled: +0x120 bit 15, the s2c 0x077 hide.
pub fn shown(e: &Entity) -> bool {
    !matches!(e.sub_kind, 2..=4) && e.flags & 6 == 0 && !e.invisible && !e.untargetable && !e.radar_off && !e.object
}

/// FUN_102203b0's alpha (asm 0x10216a40..0x10216ad7, x87 with truncation) from the compass alpha `a` (0..0x80):
/// the distance fade past `fade`, then the height fade past 2.
pub fn alpha(a: u32, dist: f64, h: f64, s: &Sample) -> u32 {
    let (range, fade) = (s.range as f64, s.fade as f64);
    let mut a = a as f64;
    if dist - fade > 0.0 {
        let t = (dist - fade) / (range - fade);
        a = ((1.0 - t) * a).trunc();
    }
    if h - 2.0 > 0.0 {
        let u = (h - 2.0) * 0.5;
        a = ((1.0 - u) * a).trunc();
    }
    a.max(0.0) as u32
}

/// The dot's RGB (asm 0x10216add..0x10216b3e), or none.
pub fn colour(bits: u32, monstrosity: u16, status: u8, mobs: bool) -> Option<u32> {
    if bits & 1 != 0 && monstrosity == 0 {
        Some(if bits & 0xc != 0 { 0x30_60e0 } else { 0x00_40f0 })
    } else if bits & 0x100 != 0 {
        Some(0x30_d000)
    } else if bits & 2 != 0 {
        (status != 0x31).then_some(0x00_b040)
    } else if mobs {
        Some(if matches!(status, 2 | 3) { 0x20_6060 } else { 0x80_8080 })
    } else {
        None
    }
}

/// The dots as (local x, local y, ARGB) in slot order: local (P.x - E.x, E.z - P.z), kept within the range and 4 high.
pub fn dots(w: &World, a: u32, s: &Sample) -> Vec<(f64, f64, u32)> {
    let mut ents: Vec<(&u32, &Entity)> = w.entities.iter().filter(|(id, _)| **id != w.my_id).collect();
    ents.sort_by_key(|(_, e)| e.index);
    let p = w.pos;
    ents.into_iter()
        .filter(|(_, e)| shown(e))
        .filter_map(|(&id, e)| {
            let (lx, ly) = (p.x as f64 - e.pos.x as f64, e.pos.z as f64 - p.z as f64);
            let (dist, h) = ((lx * lx + ly * ly).sqrt(), (p.y as f64 - e.pos.y as f64).abs());
            if dist > s.range as f64 || h > 4.0 {
                return None;
            }
            let rgb = colour(kind_bits(w, id, e), e.monstrosity, e.motion.status, s.mobs)?;
            Some((lx, ly, alpha(a, dist, h, s) << 24 | rgb))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's vectors (radar_ref.py): WAR, A = 128.
    #[test]
    fn fades() {
        let war = sample(1, 0, 0);
        assert!((war.range - 28.8).abs() < 1e-4 && (war.fade - 14.4).abs() < 1e-4 && war.scale == 0.8 && !war.mobs);
        assert_eq!([10.0, 14.4, 20.0, 28.7].map(|d| alpha(128, d, 0.0, &war)), [128, 128, 78, 0]);
        assert_eq!([(10.0, 3.0), (20.0, 3.0), (10.0, 4.0)].map(|(d, h)| alpha(128, d, h, &war)), [64, 39, 0]);
        let nin = sample(13, 0, 0);
        assert_eq!((nin.range, alpha(128, 27.0, 0.0, &nin), nin.mobs), (36.0, 64, true));
        assert!(sample(1, 6, 10).mobs && !sample(1, 6, 0).mobs);
    }

    #[test]
    fn colours() {
        assert_eq!(colour(1, 0, 0, false), Some(0x00_40f0));
        assert_eq!(colour(1 | 4 | 8, 0, 0, false), Some(0x30_60e0));
        assert_eq!(colour(2 | 0x100 | 0x1000, 0, 0, false), Some(0x30_d000));
        assert_eq!(colour(2, 0, 0, false), Some(0x00_b040));
        assert_eq!(colour(2, 0, 0x31, true), None);
        assert_eq!(colour(0x10, 0, 0, false), None);
        assert_eq!(colour(0x10, 0, 0, true), Some(0x80_8080));
        assert_eq!(colour(0x10, 0, 3, true), Some(0x20_6060));
        assert_eq!(colour(1, 0x8005, 0, true), Some(0x80_8080));
    }
}
