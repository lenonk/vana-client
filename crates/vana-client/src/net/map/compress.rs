//! FFXI's packet compression: a fixed per-byte prefix code, bits packed LSB-first,
//! behind a leading 0x01 byte. The table holds 512 little-endian u32s indexed by the
//! byte as a signed char: codes at `b + 0x80`, code lengths at `b + 0x180`.

use anyhow::{Result, bail};

#[derive(Clone)]
pub struct Codec {
    codes: [u32; 256],
    lens: [u8; 256],
    /// Binary trie; a child >= 0 is a node index, < 0 is `-(symbol + 1)`.
    tree: Vec<[i32; 2]>,
}

fn signed_index(b: u8, base: i32) -> usize {
    (b as i8 as i32 + base) as usize
}

/// FFXiMain.dll keeps the code as 256 packed records `{u8 sym, u32 code, u32 bits}`,
/// ordered by sym as a signed char (0x80, 0x81, .. 0x7F).
const RECORDS: usize = 256 * 9;
const KNOWN_OFFSET: usize = 0x37EA8;

fn records_at(dll: &[u8], at: usize) -> bool {
    dll.len() >= at + RECORDS && (0..256).all(|i| dll[at + i * 9] == (i as u8).wrapping_sub(0x80))
}

impl Codec {
    /// Reads the code table out of the game's FFXiMain.dll.
    pub fn from_ffximain(dll: &[u8]) -> Result<Self> {
        let at = if records_at(dll, KNOWN_OFFSET) {
            KNOWN_OFFSET
        } else {
            (0..dll.len().saturating_sub(RECORDS)).find(|&o| records_at(dll, o)).ok_or_else(|| anyhow::anyhow!("no compression table in FFXiMain.dll"))?
        };
        let field = |i: usize, o: usize| u32::from_le_bytes(dll[at + i * 9 + o..at + i * 9 + o + 4].try_into().unwrap());
        let mut table = vec![0u8; 2048];
        for i in 0..256 {
            table[i * 4..i * 4 + 4].copy_from_slice(&field(i, 1).to_le_bytes());
            table[(0x100 + i) * 4..(0x100 + i) * 4 + 4].copy_from_slice(&field(i, 5).to_le_bytes());
        }
        Self::from_table(&table)
    }

    /// From the server's 512-u32 layout (LandSandBoat's res/compress.dat).
    pub fn from_table(table: &[u8]) -> Result<Self> {
        if table.len() != 2048 {
            bail!("compression table must be 2048 bytes, got {}", table.len());
        }
        let word = |i: usize| u32::from_le_bytes(table[i * 4..i * 4 + 4].try_into().unwrap());
        let mut codec = Codec { codes: [0; 256], lens: [0; 256], tree: vec![[0, 0]] };
        for b in 0..=255u8 {
            let len = word(signed_index(b, 0x180));
            if !(1..=32).contains(&len) {
                bail!("bad code length {len} for byte {b:#04x}");
            }
            codec.codes[b as usize] = word(signed_index(b, 0x80));
            codec.lens[b as usize] = len as u8;
        }
        for b in 0..=255u8 {
            let (code, len) = (codec.codes[b as usize], codec.lens[b as usize] as u32);
            let mut node = 0usize;
            for k in 0..len {
                let bit = ((code >> k) & 1) as usize;
                if k + 1 == len {
                    if codec.tree[node][bit] != 0 {
                        bail!("compression table is not a prefix code (byte {b:#04x})");
                    }
                    codec.tree[node][bit] = -(b as i32 + 1);
                } else {
                    match codec.tree[node][bit] {
                        0 => {
                            codec.tree.push([0, 0]);
                            let next = codec.tree.len() - 1;
                            codec.tree[node][bit] = next as i32;
                            node = next;
                        }
                        n if n > 0 => node = n as usize,
                        _ => bail!("compression table is not a prefix code (byte {b:#04x})"),
                    }
                }
            }
        }
        Ok(codec)
    }

    /// Returns the compressed bytes and the size field the protocol carries (payload bits + 8).
    pub fn compress(&self, input: &[u8]) -> (Vec<u8>, u32) {
        let mut out = vec![1u8];
        let (mut acc, mut acc_bits, mut bits) = (0u64, 0u32, 0u32);
        for &b in input {
            let len = self.lens[b as usize] as u32;
            let code = if len >= 32 { self.codes[b as usize] } else { self.codes[b as usize] & ((1 << len) - 1) };
            acc |= (code as u64) << acc_bits;
            acc_bits += len;
            bits += len;
            while acc_bits >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                acc_bits -= 8;
            }
        }
        if acc_bits > 0 {
            out.push(acc as u8);
        }
        (out, bits + 8)
    }

    pub fn decompress(&self, data: &[u8], size_field: u32) -> Result<Vec<u8>> {
        if data.first() != Some(&1) {
            bail!("compressed data does not start with 0x01");
        }
        let payload = &data[1..];
        let bits = (size_field.saturating_sub(8) as usize).min(payload.len() * 8);
        let mut out = Vec::with_capacity(bits / 4);
        let (mut pos, mut node) = (0usize, 0usize);
        while pos < bits {
            let bit = ((payload[pos >> 3] >> (pos & 7)) & 1) as usize;
            pos += 1;
            match self.tree[node][bit] {
                0 => bail!("invalid code in compressed data"),
                n if n < 0 => {
                    out.push((-n - 1) as u8);
                    node = 0;
                }
                n => node = n as usize,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set FFXI_COMPRESS_TABLE (and FFXI_DECOMPRESS_TABLE for the tree check) to run.
    fn codec() -> Option<Codec> {
        let path = std::env::var("FFXI_COMPRESS_TABLE").ok()?;
        Some(Codec::from_table(&std::fs::read(path).unwrap()).unwrap())
    }

    /// Set FFXI_MAIN_DLL to the game's FFXiMain.dll: its table must equal the server's.
    #[test]
    fn game_table_matches_server_table() {
        let (Some(server), Ok(dll)) = (codec(), std::env::var("FFXI_MAIN_DLL")) else { return };
        let game = Codec::from_ffximain(&std::fs::read(dll).unwrap()).unwrap();
        assert_eq!(game.codes, server.codes);
        assert_eq!(game.lens, server.lens);
    }

    #[test]
    fn round_trip() {
        let Some(c) = codec() else { return };
        let input: Vec<u8> = (0..4000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let (packed, size) = c.compress(&input);
        assert_eq!(c.decompress(&packed, size).unwrap(), input);
    }

    /// Our trie must decode every code exactly as the decompress.dat tree used by LSB does.
    #[test]
    fn matches_server_tree() {
        let (Some(c), Ok(path)) = (codec(), std::env::var("FFXI_DECOMPRESS_TABLE")) else { return };
        let raw = std::fs::read(path).unwrap();
        let dec: Vec<u32> = raw.chunks(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        let base = dec[0] - 4;
        for b in 0..=255u8 {
            let (code, len) = (c.codes[b as usize], c.lens[b as usize]);
            let mut node = ((dec[0] - base) / 4) as usize;
            for k in 0..len {
                let next = dec[node + ((code >> k) & 1) as usize];
                assert!(next > 0xff, "byte {b:#04x} leaves the tree early");
                node = ((next - base) / 4) as usize;
            }
            assert!(dec[node] == 0 && dec[node + 1] == 0, "byte {b:#04x} does not end on a leaf");
            assert_eq!(dec[node + 3], b as u32, "byte {b:#04x} decodes differently");
        }
    }
}
