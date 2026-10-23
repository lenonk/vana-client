//! Map marker files (FUN_101f80b0 load, FUN_101f82b0 save): USER/<id>/M%03d%02d.MRK (zone, sub-key), "KMIX", the
//! CRC-32 of the data, then sets 0 (yours) and 1 (event markers), 10 records of 0x16 bytes each: x / y s16 map pixels,
//! name[16], valid u8, pad.

use std::path::{Path, PathBuf};

const MAGIC: u32 = 0x5849_4d4b;
const REC: usize = 0x16;
const DATA: usize = 2 * 10 * REC;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub px: (i16, i16),
    pub name: String,
    pub valid: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkFile {
    pub sets: [[Slot; 10]; 2],
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(b);
    c.sum()
}

impl MarkFile {
    pub fn path(dir: &Path, zone: u16, key: u8) -> PathBuf {
        dir.join(format!("M{zone:03}{key:02}.MRK"))
    }

    /// The new name, else the old "M%03d%02d_%08d.MRK" (0x1c8 bytes: magic @0, CRC @8, data @0x10); a bad magic or
    /// CRC gives empty sets.
    pub fn load(dir: &Path, zone: u16, key: u8, char_id: u32) -> MarkFile {
        if let Ok(b) = std::fs::read(Self::path(dir, zone, key)) {
            return Self::parse(&b, 0, 4, 8).unwrap_or_default();
        }
        std::fs::read(dir.join(format!("M{zone:03}{key:02}_{char_id:08}.MRK"))).ok().and_then(|b| Self::parse(&b, 0, 8, 0x10)).unwrap_or_default()
    }

    fn parse(b: &[u8], magic: usize, crc: usize, data: usize) -> Option<MarkFile> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        let d = b.get(data..data + DATA)?;
        if u32_at(magic)? != MAGIC || u32_at(crc)? != crc32(d) {
            return None;
        }
        let mut f = MarkFile::default();
        for (k, r) in d.chunks(REC).enumerate() {
            let mut name = r[4..0x14].to_vec();
            name[15] = 0;
            let end = name.iter().position(|&c| c == 0).unwrap_or(15);
            f.sets[k / 10][k % 10] = Slot {
                px: (i16::from_le_bytes([r[0], r[1]]), i16::from_le_bytes([r[2], r[3]])),
                name: crate::dat::font::decode_sjis(&name[..end]),
                valid: r[0x14] != 0,
            };
        }
        Some(f)
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(DATA);
        for s in self.sets.iter().flatten() {
            d.extend_from_slice(&s.px.0.to_le_bytes());
            d.extend_from_slice(&s.px.1.to_le_bytes());
            let mut name = crate::dat::font::encode_sjis(&s.name);
            name.resize(16, 0);
            name[15] = 0;
            d.extend_from_slice(&name);
            d.extend_from_slice(&[s.valid as u8, 0]);
        }
        let mut out = Vec::with_capacity(8 + DATA);
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&crc32(&d).to_le_bytes());
        out.extend_from_slice(&d);
        out
    }

    /// Writes the new layout and removes the old name (inf: the removal, FUN_101c49e0).
    pub fn save(&self, dir: &Path, zone: u16, key: u8, char_id: u32) {
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(Self::path(dir, zone, key), self.bytes())) {
            log::warn!("map markers: {e}");
        }
        let _ = std::fs::remove_file(dir.join(format!("M{zone:03}{key:02}_{char_id:08}.MRK")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut f = MarkFile::default();
        f.sets[0][3] = Slot { px: (-120, 77), name: "Gate".into(), valid: true };
        f.sets[1][0] = Slot { px: (5, -6), name: "Quest NPC".into(), valid: true };
        let b = f.bytes();
        assert_eq!((b.len(), &b[..4]), (0x1c0, &b"KMIX"[..]));
        assert_eq!(MarkFile::parse(&b, 0, 4, 8), Some(f));
        let mut bad = b.clone();
        bad[20] ^= 1;
        assert_eq!(MarkFile::parse(&bad, 0, 4, 8), None);
    }
}
