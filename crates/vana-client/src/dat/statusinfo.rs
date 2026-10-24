//! Status info files (language slot 0x3a: 32x32 icons, English file 87; slot 0x3b: 16x16, file 39557): 640
//! entries of 0x1800 bytes by icon id (FUN_102272b0 / FUN_10227140).

use super::items::{Field, string_table};

pub const ENTRY: usize = 0x1800;

/// An entry's head: cancellable (byte @2, Enter sends c2s 0x0F1), never timed (byte @3 0xff, e.g. KO), and the
/// description (string table @4, string 0).
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub cancel: bool,
    pub no_timer: bool,
    pub desc: String,
}

fn raw(file: &[u8], id: u16) -> Option<&[u8]> {
    file.get(id as usize * ENTRY..(id as usize + 1) * ENTRY)
}

/// The first 0x1fc bytes are masked (FUN_1021e870, bytes 2, 0xb and 0xc as stored).
pub fn entry(file: &[u8], id: u16) -> Option<Entry> {
    let mut head = raw(file, id)?.get(..0x1fc)?.to_vec();
    super::actions::unmask(&mut head);
    let desc = string_table(&head[4..]).into_iter().find_map(|f| if let Field::Text(s) = f { Some(s) } else { None }).unwrap_or_default();
    Some(Entry { cancel: head[2] != 0, no_timer: head[3] == 0xff, desc })
}

/// The icon @0x284 as RGBA: flag byte, 16-byte name, BITMAPINFOHEADER (32 bpp), BGRA rows bottom-up. The size field
/// @0x280 is not read (retail ignores it; the xiPrime icons carry a stale one). Alpha 0..0x80 is doubled; a set whose
/// alpha already runs to 0xff (xiPrime) is taken as it is.
pub fn icon(file: &[u8], id: u16) -> Option<(u32, u32, Vec<u8>)> {
    let d = raw(file, id)?.get(0x284..)?;
    let h = d.get(0x11..0x11 + 40)?;
    let rd = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap()) as i32;
    let (w, ht, bpp) = (rd(4), rd(8), u16::from_le_bytes([h[14], h[15]]));
    if bpp != 32 || !(1..=64).contains(&w) || ht == 0 || ht.abs() > 64 {
        return None;
    }
    let (wu, hu) = (w as usize, ht.unsigned_abs() as usize);
    let px = d.get(0x11 + 40..0x11 + 40 + wu * hu * 4)?;
    let doubled = px.chunks(4).all(|p| p[3] <= 0x80);
    let mut out = vec![0u8; wu * hu * 4];
    for y in 0..hu {
        let sy = if ht > 0 { hu - 1 - y } else { y };
        for x in 0..wu {
            let s = (sy * wu + x) * 4;
            let o = (y * wu + x) * 4;
            out[o] = px[s + 2];
            out[o + 1] = px[s + 1];
            out[o + 2] = px[s];
            out[o + 3] = if doubled { (px[s + 3] as u32 * 2).min(255) as u8 } else { px[s + 3] };
        }
    }
    Some((wu as u32, hu as u32, out))
}

#[cfg(test)]
mod tests {
    /// English slots 0x3a / 0x3b are files 87 / 39557; Haste (33) is cancellable with a description, KO (0) is never
    /// timed; both icon sets decode at their sizes.
    #[test]
    fn haste_and_ko() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let files = [0x3a, 0x3b].map(|s| crate::dat::msg::current_slot_file(&dll, 1, s));
        assert_eq!(files, [Some(87), Some(39557)]);
        let big = index.read(87).unwrap();
        let haste = super::entry(&big, 33).unwrap();
        assert!(haste.cancel && !haste.no_timer && !haste.desc.is_empty(), "{haste:?}");
        assert!(super::entry(&big, 0).unwrap().no_timer);
        assert_eq!(super::icon(&big, 33).map(|i| (i.0, i.1)), Some((32, 32)));
        let small = index.read(39557).unwrap();
        assert_eq!(super::icon(&small, 33).map(|i| (i.0, i.1)), Some((16, 16)));
    }
}
