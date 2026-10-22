//! Client message files, as `thunk_FUN_1014c660(group, id)` reads them: "XISTRING" (groups 8 and 10,
//! FUN_1014c800) and XOR-0xFF "d_msg" (the rest, FUN_1014d630 / FUN_1024d1a0).

use super::DatIndex;
use super::font::decode_sjis;
use anyhow::{Context, Result, bail};

/// Language tables of file ids (FUN_10250de0), FFXiMain file offsets: ja, en, fr, de.
const LANG_TABLE_AT: [usize; 4] = [0x60828, 0x60280, 0x600d0, 0x60438];

/// Group -> language-table slot: the group table at 0x10365f50 names a message object, and
/// FUN_1014d350 loads each object from FUN_10250e50(slot). Groups 8 and 10 read their slot directly.
const GROUP_SLOT: [(u8, usize); 22] = [
    (0x01, 0x03), (0x03, 0x04), (0x06, 0x02), (0x07, 0x07), (0x08, 0x08), (0x09, 0x09), (0x0a, 0x0a),
    (0x0b, 0x0b), (0x0c, 0x0e), (0x0d, 0x0d), (0x0e, 0x0c), (0x0f, 0x16), (0x10, 0x15), (0x11, 0x17),
    (0x12, 0x13), (0x13, 0x0f), (0x14, 0x10), (0x15, 0x11), (0x16, 0x12), (0x17, 0x19), (0x1a, 0x3b),
    (0x1b, 0x06),
];

/// File id of a message group for a language (0 ja, 1 en, 2 fr, 3 de).
pub fn group_file(ffximain: &[u8], lang: u8, group: u8) -> Option<u32> {
    slot_file(ffximain, lang, GROUP_SLOT.iter().find(|(g, _)| *g == group)?.1)
}

/// File id in a language's slot table (FUN_10250e50(slot)).
pub fn slot_file(ffximain: &[u8], lang: u8, slot: usize) -> Option<u32> {
    let at = LANG_TABLE_AT.get(lang as usize)? + slot * 4;
    Some(u32::from_le_bytes(ffximain.get(at..at + 4)?.try_into().ok()?))
}

pub struct Messages {
    entries: Vec<Vec<u8>>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl Messages {
    pub fn load(index: &DatIndex, file: u32) -> Result<Self> {
        Self::parse(&index.read(file)?).with_context(|| format!("message file {file}"))
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut entries = Vec::new();
        if data.starts_with(b"XISTRING") {
            let count = u32_at(data, 0x24).context("header")? as usize;
            let base = 0x38 + count * 12;
            for i in 0..count {
                // The length's high half carries flags (0x10000 on the "%"-code strings of file 48; inferred).
                let (off, len) = (u32_at(data, 0x38 + i * 12).context("table")? as usize, (u32_at(data, 0x3c + i * 12).context("table")? & 0xffff) as usize);
                entries.push(data.get(base + off..base + off + len).context("string out of range")?.to_vec());
            }
        } else if data.starts_with(b"d_msg") {
            let (hsize, tsize) = (u32_at(data, 0x18).context("header")? as usize, u32_at(data, 0x1c).context("header")? as usize);
            // XOR flag u16 @0x0a: set in 55651 / 55725, clear in the plain spell and ability files.
            let count = u32_at(data, 0x28).context("header")? as usize;
            let xor = if data.get(0x0a) == Some(&1) { 0xFF } else { 0 };
            let table: Vec<u8> = data.get(hsize..hsize + tsize).context("table")?.iter().map(|b| b ^ xor).collect();
            let body: Vec<u8> = data.get(hsize + tsize..).context("body")?.iter().map(|b| b ^ xor).collect();
            if tsize == 0 {
                // No table: fixed-size records of u32 @0x20 bytes (55701 abilities, 55702 spells).
                let size = u32_at(data, 0x20).context("header")? as usize;
                entries.extend((0..count).map(|i| body.get(i * size..(i + 1) * size).unwrap_or_default().to_vec()));
            }
            for i in 0..count.min(tsize / 8) {
                let (off, len) = (u32_at(&table, i * 8).unwrap() as usize, u32_at(&table, i * 8 + 4).unwrap() as usize);
                entries.push(body.get(off..off + len).unwrap_or_default().to_vec());
            }
        } else {
            bail!("not a message file");
        }
        Ok(Self { entries })
    }

    /// The text of entry `id`: a d_msg entry is a field count, (offset, kind) pairs, then fields whose
    /// text starts 0x1c bytes in; an XISTRING entry is the text itself.
    pub fn get(&self, id: usize) -> Option<String> {
        self.raw(id).map(|t| decode_sjis(&t))
    }

    /// Field 0's bytes up to the first NUL (still Shift-JIS).
    pub fn raw(&self, id: usize) -> Option<Vec<u8>> {
        let e = self.entries.get(id)?;
        let text = match u32_at(e, 0) {
            Some(n) if (1..16).contains(&n) => e.get(u32_at(e, 4)? as usize + 0x1c..)?,
            _ => &e[..],
        };
        let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        Some(text[..end].to_vec())
    }
}
