//! Spell and ability records: file 81, chunks 0x49 ("mgc_", 100-byte SpellInfo records, DAT_10667a74) and 0x53
//! ("comm", 0x30-byte AbilityInfo records, DAT_1066714c), each record masked as FUN_1021e870 undoes it.

use super::DatIndex;
use anyhow::{Context, Result};

const FILE: u32 = 81;
const SPELL_CHUNK: u8 = 0x49;
const ABILITY_CHUNK: u8 = 0x53;
const SPELL_SIZE: usize = 100;
const ABILITY_SIZE: usize = 0x30;

/// FUN_1021e870 (POLUtils DecodeDataBlockMask): every byte but 2, 11 and 12 rotated by a count from their bits.
fn unmask(r: &mut [u8]) {
    let pc = |b: u8| b.count_ones() as i32;
    let s = (pc(r[2]) + pc(r[12]) - pc(r[11])).abs() % 5;
    for (i, b) in r.iter_mut().enumerate() {
        if matches!(i, 2 | 11 | 12) {
            continue;
        }
        *b = match s {
            0 => b.rotate_left(1),
            1 => b.rotate_right(1),
            2 => b.rotate_left(2),
            3 => b.rotate_right(2),
            _ => b.rotate_left(3),
        };
    }
}

fn records(index: &DatIndex, kind: u8, size: usize) -> Result<Vec<Vec<u8>>> {
    let file = index.read(FILE)?;
    let c = super::chunks(&file)?.into_iter().find(|c| c.kind == kind).with_context(|| format!("file {FILE}: no chunk {kind:#x}"))?;
    Ok(c.data.chunks_exact(size).map(|r| {
        let mut r = r.to_vec();
        unmask(&mut r);
        r
    }).collect())
}

fn u16_at(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}

#[derive(Clone, Debug, Default)]
pub struct Spell {
    /// +0: the spell id (0 = unused record); names are by record index, which equals it.
    pub id: u16,
    /// +2: 1 white, 2 black, 3 summoning, 4 ninjutsu, 5 song, 6 blue, 7 geomancy, 8 trust.
    pub kind: u16,
    pub element: u16,
    /// +6: valid target bits (1 self, 2 player, 4 party, 8 alliance, 0x10 PC, 0x20 enemy ...).
    pub targets: u16,
    pub skill: u16,
    pub mp: u16,
    /// +0xc / +0xd: cast time and recast in quarter seconds.
    pub cast: u8,
    pub recast: u8,
    /// +0xe: level per job 0..23, -1 when the job can't learn it.
    pub levels: [i16; 24],
    /// +0x5c: per main job, the level is in job points (needs level 99).
    pub jp_mask: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Ability {
    /// +0: id in the name table (weapon skills 0..0x1ff, job abilities 0x200.., pet commands 0x400..).
    pub id: u16,
    pub kind: u8,
    /// +6: Ready charges / cost.
    pub cost: u16,
    /// +0xe: skipped by /ja (inf: hidden).
    pub hidden: bool,
}

pub struct Actions {
    pub spells: Vec<Spell>,
    pub abilities: Vec<Ability>,
}

impl Actions {
    pub fn load(index: &DatIndex) -> Result<Actions> {
        let spells = records(index, SPELL_CHUNK, SPELL_SIZE)?
            .iter()
            .map(|r| Spell {
                id: u16_at(r, 0),
                kind: u16_at(r, 2),
                element: u16_at(r, 4),
                targets: u16_at(r, 6),
                skill: u16_at(r, 8),
                mp: u16_at(r, 0xa),
                cast: r[0xc],
                recast: r[0xd],
                levels: std::array::from_fn(|j| u16_at(r, 0xe + 2 * j) as i16),
                jp_mask: u32::from_le_bytes(r[0x5c..0x60].try_into().unwrap()),
            })
            .collect();
        let abilities = records(index, ABILITY_CHUNK, ABILITY_SIZE)?
            .iter()
            .map(|r| Ability { id: u16_at(r, 0), kind: r[2], cost: u16_at(r, 6), hidden: r[0xe] != 0 })
            .collect();
        Ok(Actions { spells, abilities })
    }

    pub fn spell(&self, id: u16) -> Option<&Spell> {
        self.spells.get(id as usize).filter(|s| s.id != 0)
    }

    /// FUN_1022b7d0: the main or sub job reaches the spell's level; a job-point spell needs main 99 and the points.
    pub fn can_cast(&self, id: u16, main: (u8, u8), sub: (u8, u8), job_points: u16) -> bool {
        let Some(s) = self.spell(id) else { return false };
        let lv = |job: u8| s.levels.get(job as usize).copied().unwrap_or(-1);
        let reach = |(job, level): (u8, u8)| lv(job) >= 0 && level as i16 >= lv(job);
        if s.jp_mask & (1 << main.0) != 0 {
            main.1 >= 99 && lv(main.0) >= 0 && job_points as i16 >= lv(main.0)
        } else {
            reach(main) || reach(sub)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Option<Actions> {
        let index = DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        Actions::load(&index).ok()
    }

    /// Cure is white magic for WHM 1; Shantotto (896) a trust any job can call at 1; ids match the record index.
    #[test]
    fn spells_from_file_81() {
        let Some(a) = real() else { return };
        assert_eq!(a.spells.len(), 1024);
        let cure = a.spell(1).unwrap();
        assert_eq!((cure.kind, cure.mp, cure.levels[3], cure.levels[1]), (1, 8, 1, -1));
        let trust = a.spell(896).unwrap();
        assert_eq!((trust.kind, trust.targets, trust.levels[1]), (8, 1, 1));
        assert!(a.spells.iter().enumerate().all(|(i, s)| s.id == 0 || s.id as usize == i));
        assert!(a.can_cast(1, (3, 1), (0, 0), 0) && !a.can_cast(1, (1, 75), (0, 0), 0) && a.can_cast(1, (1, 75), (3, 37), 0));
    }
}
