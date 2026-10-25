//! Mog House furniture data (spec ~/ffxi/lab/research/furniture_spec.md 2): file 32922's 'furn' table (chunk 0x59,
//! 1024 entries of 8 bytes by item id, FUN_101d31e0) and 'mode' plant table (chunk 0x5b, FUN_101d5ec0), and the model
//! file formula (FUN_101cce00).

use super::{DatIndex, chunks};
use anyhow::{Context, Result};

/// FUN_101d3120 loads this file once.
pub const TABLE_FILE: u32 = 0x809a;

/// A 'furn' entry: {u16 flags, u16 model, u8 size x, u8 size z, u16 height in 0.01 units}.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Furn {
    pub flags: u16,
    pub model: u16,
    pub size: [u8; 2],
    pub height: u16,
}

impl Furn {
    /// FUN_101cebe0: placed in a wall slot.
    pub fn wall(&self) -> bool {
        self.flags & 1 != 0
    }

    /// FUN_101cec10: a flowerpot (grows a plant).
    pub fn pot(&self) -> bool {
        self.flags & 8 != 0
    }

    /// FUN_101ceaf0 / FUN_101ceb40: the footprint in cells (width, depth), turned for odd rotations.
    pub fn footprint(&self, rot: u8) -> (u8, u8) {
        if rot % 2 == 0 { (self.size[0], self.size[1]) } else { (self.size[1], self.size[0]) }
    }
}

pub struct FurnTable {
    entries: Vec<Furn>,
    /// Plant models by kind 1..8, 12 growth stages each (999 none).
    plants: Vec<[u32; 12]>,
}

impl FurnTable {
    pub fn load(index: &DatIndex) -> Result<Self> {
        Self::parse(&index.read(TABLE_FILE)?)
    }

    pub fn parse(file: &[u8]) -> Result<Self> {
        let list = chunks(file)?;
        let furn = list.iter().find(|c| c.kind == 0x59).context("no 'furn' chunk")?;
        let u16_at = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let entries = furn.data.chunks_exact(8).map(|e| Furn { flags: u16_at(e, 0), model: u16_at(e, 2), size: [e[4], e[5]], height: u16_at(e, 6) }).collect();
        let u32_at = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        let mut plants = Vec::new();
        if let Some(mode) = list.iter().find(|c| c.kind == 0x5b) {
            let d = mode.data;
            let n = u32_at(d, 0).unwrap_or(0) as usize;
            for k in 0..n.min(16) {
                let Some(at) = u32_at(d, 4 + 4 * k).map(|o| o as usize) else { break };
                let mut row = [999u32; 12];
                let count = u32_at(d, at).unwrap_or(0) as usize;
                for (s, m) in row.iter_mut().enumerate().take(count.min(12)) {
                    *m = u32_at(d, at + 4 + 4 * s).unwrap_or(999);
                }
                plants.push(row);
            }
        }
        Ok(FurnTable { entries, plants })
    }

    /// FUN_101d31e0: the entry by item id, id - 0xc00 from 0xe00 on.
    pub fn entry(&self, item: u16) -> Option<Furn> {
        let i = if item > 0xdff { item - 0xc00 } else { item };
        self.entries.get(i as usize).copied()
    }

    /// FUN_101d5ec0: a plant's model by kind 1..8 and stage 0..11 (999 none).
    pub fn plant_model(&self, kind: u8, stage: u8) -> Option<u16> {
        let m = *self.plants.get((kind as usize).checked_sub(1)?)?.get(stage as usize)?;
        (m != 999).then_some(m as u16)
    }
}

/// FUN_101cce00: a furniture model's file.
pub fn model_file(model: u16) -> Option<u32> {
    let m = model as u32;
    match m {
        0..0x100 => Some(m + 0x809f),
        0x100..0x200 => Some(m + 0xc7ef),
        0x200..0xe00 => None,
        0xe00..0x1000 => Some(m + 0xe75b),
        _ => Some(0x809f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's checked entries (furniture_spec.md 2).
    #[test]
    fn table_entries() {
        assert_eq!((model_file(2), model_file(0x108), model_file(0xe9f), model_file(0x200)), (Some(32929), Some(51447), Some(62970), None));
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let t = FurnTable::load(&index).unwrap();
        assert_eq!(t.entry(2), Some(Furn { flags: 0, model: 2, size: [4, 6], height: 70 }));
        let desk = t.entry(21).unwrap();
        assert_eq!((desk.flags, desk.size, desk.height), (2, [3, 3], 50));
        assert_eq!(t.entry(0xe9f).map(|f| f.model), Some(0xe9f));
        assert_eq!((t.plant_model(1, 2), t.plant_model(1, 0), t.plant_model(8, 11)), (Some(231), None, Some(160)));
    }
}
