//! Blue magic data (blue_magic_spec.md 2, 3): file 90's 6-byte entries {idx, set points, description + 1, family,
//! level, magical / physical} for spells 0x200 + idx, the descriptions (d_msg in language slot 0x1a, entry idx - 1),
//! and the points and slots by level.

use super::DatIndex;
use super::names::DMsg;

/// FUN_101df4a0: the file of the record cache DAT_106631e8.
const FILE: u32 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The set points (0: never set, the Unbridled spells; 0xff placeholders).
    pub points: u8,
    /// The description entry + 1.
    pub desc: u8,
    /// 1..19 (group 0xc 0x3e + family).
    pub family: u8,
    pub level: u8,
    pub physical: bool,
}

pub struct BluData {
    /// By idx (spell - 0x200).
    entries: Vec<Option<Entry>>,
    descs: Option<DMsg>,
}

impl BluData {
    pub fn load(index: &DatIndex, desc_file: Option<u32>) -> BluData {
        let mut entries = vec![None; 256];
        match index.read(FILE) {
            Ok(d) => {
                for e in d.chunks_exact(6) {
                    entries[e[0] as usize] = Some(Entry { points: e[1], desc: e[2], family: e[3], level: e[4], physical: e[5] != 0 });
                }
            }
            Err(e) => log::warn!("blue magic file {FILE}: {e:#}"),
        }
        let descs = desc_file.and_then(|f| DMsg::load(index, f).map_err(|e| log::warn!("blue magic descriptions {f}: {e:#}")).ok());
        BluData { entries, descs }
    }

    /// FUN_101d6d70: the entry of spell 0x200 + idx.
    pub fn entry(&self, spell: u16) -> Option<Entry> {
        spell.checked_sub(0x200).filter(|&i| i < 0x100).and_then(|i| self.entries[i as usize])
    }

    /// The description (entry desc - 1), lines split at '\n'.
    pub fn description(&self, e: Entry) -> String {
        self.descs.as_ref().and_then(|d| d.text(e.desc as usize - 1)).unwrap_or_default()
    }
}

/// FUN_10101bf0: the level band, ceil(level / 10), at most 10.
fn band(level: u8) -> usize {
    (level as usize).div_ceil(10).min(10)
}

/// FUN_10101cb0 (table 0x10364470): the set points of a level, before the 0x063 bonus.
pub fn points(level: u8) -> u32 {
    [0, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55][band(level)]
}

/// FUN_10101bc0 (table 0x1036447c): the slots of a level.
pub fn slots(level: u8) -> usize {
    [0, 6, 8, 10, 12, 14, 16, 18, 20, 20, 20][band(level)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        assert_eq!((points(1), points(10), points(11), points(75), points(99)), (10, 10, 15, 45, 55));
        assert_eq!((slots(0), slots(20), slots(21), slots(75)), (0, 8, 10, 20));
    }

    #[test]
    fn real_file() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok();
        let file = dll.as_deref().and_then(|d| crate::dat::msg::current_slot_file(d, 1, 0x1a));
        let b = BluData::load(&index, file);
        // Venom Shell 513: 3 points, Aquans, Lv 42, magical; Screwdriver 519: physical, Lv 26.
        assert_eq!(b.entry(513), Some(Entry { points: 3, desc: 1, family: 1, level: 42, physical: false }));
        assert_eq!(b.entry(519).map(|e| (e.level, e.physical)), Some((26, true)));
        assert!(b.description(b.entry(513).unwrap()).starts_with("Poisons enemies within range"));
        assert_eq!(b.entry(0x2e0).map(|e| e.points), Some(0));
    }
}
