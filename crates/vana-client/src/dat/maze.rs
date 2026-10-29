//! Moblin Maze Mongers' Maze Tabula (mmm_maze_spec.md §2): the rune shapes, their cells on the 5 x 5 grid, and the
//! tabula extdata's bit stream (FUN_101fd290 decodes, FUN_101fd600 encodes).

/// Table 0x103850a8: width and height in cells, and the 4 x 4 masks of rotations 0..3 (bit 15 the top left, rows of 4
/// bits from the top); rotation 1 is a quarter turn clockwise of 0. Shapes: domino, corner, line 3, T, square, L, J, S,
/// Z, line 4, plus.
pub const SHAPES: [(u8, u8, [u16; 4]); 11] = [
    (1, 2, [0x8800, 0xc000, 0x8800, 0xc000]),
    (2, 2, [0x8c00, 0xc800, 0xc400, 0x4c00]),
    (1, 3, [0x8880, 0xe000, 0x8880, 0xe000]),
    (2, 3, [0x8c80, 0xe400, 0x4c40, 0x4e00]),
    (2, 2, [0xcc00, 0xcc00, 0xcc00, 0xcc00]),
    (2, 3, [0x88c0, 0xe800, 0xc440, 0x2e00]),
    (2, 3, [0x44c0, 0x8e00, 0xc880, 0xe200]),
    (2, 3, [0x8c40, 0x6c00, 0x8c40, 0x6c00]),
    (2, 3, [0x4c80, 0xc600, 0x4c80, 0xc600]),
    (1, 4, [0x8888, 0xf000, 0x8888, 0xf000]),
    (3, 3, [0x4e40, 0x4e40, 0x4e40, 0x4e40]),
];

pub const VOUCHER_BASE: u16 = 0x7040;
pub const RUNE_BASE: u16 = 0x7080;
/// Up to 12 runes on a tabula (d1..: 12 indexes of 9 bits).
pub const MAX_RUNES: usize = 12;

/// Width and height in cells at a rotation (swapped for odd ones).
pub fn extent(shape: u8, rot: u8) -> (i8, i8) {
    let (w, h, _) = SHAPES[shape as usize % SHAPES.len()];
    if rot & 1 == 1 { (h as i8, w as i8) } else { (w as i8, h as i8) }
}

pub fn mask(shape: u8, rot: u8) -> u16 {
    SHAPES[shape as usize % SHAPES.len()].2[rot as usize & 3]
}

/// FUN_101fda30: the cells a mask covers with its top left at (col, row), bit (24 - c) for cell c = 5 row + col.
pub fn cells(mask: u16, col: i8, row: i8) -> u32 {
    let base: u32 = (0..4).map(|r| (((mask >> (12 - 4 * r)) & 0xf) as u32) << (21 - 5 * r)).sum();
    base >> (5 * row as i32 + col as i32).clamp(0, 31)
}

/// FUN_101fdb40's clamp: the top left kept so the shape stays on the grid.
pub fn clamp(shape: u8, rot: u8, col: i8, row: i8) -> (i8, i8) {
    let (w, h) = extent(shape, rot);
    (col.clamp(0, 5 - w), row.clamp(0, 5 - h))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub rune: u16,
    pub col: i8,
    pub row: i8,
    pub rot: u8,
}

fn bits(ext: &[u8; 24], at: usize, n: usize) -> u32 {
    (0..n).map(|i| ((ext[(at + i) / 8] >> ((at + i) % 8)) & 1) as u32).enumerate().map(|(i, b)| b << i).sum()
}

fn put(ext: &mut [u8; 24], at: usize, n: usize, v: u32) {
    for i in 0..n {
        let (byte, bit) = ((at + i) / 8, (at + i) % 8);
        ext[byte] = (ext[byte] & !(1 << bit)) | ((((v >> i) & 1) as u8) << bit);
    }
}

/// The voucher in an extdata (0 none).
pub fn voucher(ext: &[u8; 24]) -> u16 {
    match bits(ext, 0, 7) {
        0 => 0,
        v => VOUCHER_BASE - 1 + v as u16,
    }
}

/// The runes on it (iteminfo's "[n]": the anchor bits).
pub fn count(ext: &[u8; 24]) -> u32 {
    bits(ext, 7, 25).count_ones()
}

/// d5 bits 4..10: the number iteminfo shows for an R tabula *(inf: set by the server)*.
pub fn extra(ext: &[u8; 24]) -> u32 {
    bits(ext, 164, 7)
}

/// FUN_101fd290: the voucher and the runes in anchor order (cell c's bit 31 - c), each placed with its first top-row
/// cell on the anchor; a rune that overlaps or whose shape is unknown is left out as retail's refusal skips it.
pub fn decode(ext: &[u8; 24], shape_of: impl Fn(u16) -> Option<u8>, blocked: u32) -> (u16, Vec<Placed>) {
    let v = voucher(ext);
    if v == 0 {
        return (0, Vec::new());
    }
    let anchors: Vec<i8> = (0..25).filter(|&c| bits(ext, 31 - c, 1) == 1).map(|c| c as i8).collect();
    let mut occupied = blocked;
    let mut out = Vec::new();
    for (k, &c) in anchors.iter().enumerate().take(MAX_RUNES) {
        let n = bits(ext, 32 + 9 * k, 9);
        if n == 0 {
            continue;
        }
        let rune = RUNE_BASE - 1 + n as u16;
        let rot = bits(ext, 140 + 2 * k, 2) as u8;
        let Some(shape) = shape_of(rune) else { continue };
        let m = mask(shape, rot);
        let (mut col, row) = (c % 5, c / 5);
        if m & 0x8000 == 0 {
            col -= if m & 0x4000 == 0 { 2 } else { 1 };
        }
        let cl = cells(m, col, row);
        if col < 0 || cl & occupied != 0 {
            continue;
        }
        occupied |= cl;
        out.push(Placed { rune, col, row, rot });
    }
    (v, out)
}

/// FUN_101fd600: the voucher, then per cell 0..24 the first rune not yet written that covers it (its anchor, index and
/// rotation in the next slot); d5's upper bits stay 0.
pub fn encode(voucher: u16, runes: &[Placed], shape_of: impl Fn(u16) -> Option<u8>) -> [u8; 24] {
    let mut ext = [0u8; 24];
    if voucher < VOUCHER_BASE {
        return ext;
    }
    put(&mut ext, 0, 7, (voucher - VOUCHER_BASE + 1) as u32 & 0x7f);
    let covers: Vec<u32> = runes.iter().map(|p| shape_of(p.rune).map_or(0, |s| cells(mask(s, p.rot), p.col, p.row))).collect();
    let mut used = vec![false; runes.len()];
    let mut k = 0;
    for c in 0..25 {
        let bit = 1u32 << (24 - c);
        let Some(i) = (0..runes.len()).find(|&i| !used[i] && covers[i] & bit != 0) else { continue };
        used[i] = true;
        if k >= MAX_RUNES {
            break;
        }
        put(&mut ext, 31 - c, 1, 1);
        put(&mut ext, 32 + 9 * k, 9, (runes[i].rune - RUNE_BASE + 1) as u32);
        put(&mut ext, 140 + 2 * k, 2, runes[i].rot as u32);
        k += 1;
    }
    ext
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runes 021 (Fire, corner), 106 (T), 109 (S): the shapes the item records give them.
    fn shape(r: u16) -> Option<u8> {
        match r - RUNE_BASE + 1 {
            21 => Some(1),
            106 => Some(3),
            109 => Some(7),
            _ => None,
        }
    }

    /// The spec's vector (tabula.py enc 1 106:3:0:0 109:0:3:1 21:0:0:1) both ways.
    #[test]
    fn vector() {
        let ext: [u8; 24] = [0x01, 0x80, 0x00, 0x90, 0x15, 0xd4, 0xb4, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x01, 0, 0, 0, 0, 0];
        let (v, runes) = decode(&ext, shape, 0);
        assert_eq!(v, VOUCHER_BASE);
        let rune = |n: u16, col, row, rot| Placed { rune: RUNE_BASE - 1 + n, col, row, rot };
        assert_eq!(runes, [rune(21, 0, 0, 1), rune(106, 3, 0, 0), rune(109, 0, 3, 1)]);
        assert_eq!(count(&ext), 3);
        assert_eq!(encode(v, &runes, shape), ext);
        // The order runes were placed in does not matter.
        let mut back = runes.clone();
        back.reverse();
        assert_eq!(encode(v, &back, shape), ext);
    }

    /// Cells: the corner's quarter turn at (0, 0) takes cells 0, 1, 5; M01's blocked 6 / 18 refuse a T over them.
    #[test]
    fn cell_masks() {
        assert_eq!(cells(mask(1, 1), 0, 0), 1 << 24 | 1 << 23 | 1 << 19);
        let blocked = 1 << (24 - 6) | 1 << (24 - 18);
        assert_ne!(cells(mask(3, 0), 1, 1) & blocked, 0);
        assert_eq!(cells(mask(3, 0), 3, 0) & blocked, 0);
        assert_eq!(clamp(9, 0, 4, 4), (4, 1));
        assert_eq!(clamp(9, 1, 4, 4), (1, 4));
    }
}
