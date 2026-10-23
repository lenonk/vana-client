//! Item augments for the item info window: FUN_100ff140 (the extdata forms into a record of (message id, value)
//! pairs), FUN_100ff9c0 / FUN_100ff2f0 (the classic form folded into stats and packed into lines) and FUN_1013b280
//! (the block of +0x1b8 and the short "<...>" line). Texts are file slot 0x1b (55692 in English), formats slot 0x1c
//! (55688), server names group 0x16. Everything works on Shift-JIS bytes.

use crate::dat::msg::Messages;
use md5::{Digest, Md5};

/// The DLL's data tables (all plain data in the packed install): attribute / pet / element masks, the prefixed-entry
/// table, the evolith table, the mezzotint index and RP tables, and the weapons that never get the Aug mark.
pub struct AugTables {
    attr: Vec<[u8; 2]>,
    pet: Vec<[u8; 2]>,
    elem: Vec<[u8; 2]>,
    prefix: Vec<[u8; 4]>,
    evolith: Vec<[u8; 20]>,
    mezz: Vec<u16>,
    mezz_rp: Vec<u16>,
    no_mark: Vec<u16>,
}

/// A table found by its first bytes, confirmed by the MD5 over `len` (the first matching occurrence).
fn table<'a>(dll: &'a [u8], hint: &str, len: usize, md5: &str) -> Option<&'a [u8]> {
    let h = hex::decode(hint).ok()?;
    let mut from = 0;
    while let Some(p) = dll.get(from..)?.windows(h.len()).position(|w| w == h.as_slice()) {
        let at = from + p;
        if let Some(t) = dll.get(at..at + len).filter(|t| hex::encode(Md5::digest(t)) == md5) {
            return Some(t);
        }
        from = at + 1;
    }
    None
}

fn pairs<const N: usize>(t: &[u8]) -> Vec<[u8; N]> {
    t.chunks_exact(N).map(|c| c.try_into().unwrap()).collect()
}

fn words(t: &[u8]) -> Vec<u16> {
    t.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

impl AugTables {
    /// Hints and digests from the 30261006_0 build (va 0x10362738, 0x10362838, 0x10362878, 0x103628f8, 0x10362af8,
    /// 0x10331aa8, 0x10331ca8, 0x1037d378).
    pub fn find(dll: &[u8]) -> Option<Self> {
        let t = |hint, len, md5| {
            let r = table(dll, hint, len, md5);
            if r.is_none() {
                log::warn!("augments: table {hint} not found");
            }
            r
        };
        Some(Self {
            attr: pairs(t("010002000400080010002000400000010002000400080010002000400106010a", 0x100, "7e5ad611e599ce744e22e895bf6ae247")?),
            pet: pairs(t("0100020004000800100020004000000100020004000800100020004007000000", 0x40, "537ff5ec0c0d3e2e6f3e976563167777")?),
            elem: pairs(t("01000200040008001000200040008000", 0x80, "45426bec9f3c632be49a4720c48177d3")?),
            prefix: pairs(t("0400f0020601f002", 0x200, "6923d18738a382f2d7419fc491acde4d")?),
            evolith: pairs(t("fe03000000000000", 0x1978, "3124bafdc39bcc04b03dd21cf9d5b89d")?),
            mezz: words(t("0000ff0309000900", 0x200, "6bd34070083775d9d714477e8046a8d6")?),
            mezz_rp: words(t("1e00320050007800", 0x3e, "066af22f9cd4980ab895ef6a7e935ec3")?),
            no_mark: words(t("df405f419941fc41", 0x66, "eddd707b4645d3070c4bd5b824492db6")?).into_iter().take_while(|&w| w != 0).collect(),
        })
    }
}

/// The "mnc2" chunk of file 81 (the bundle RP curves, bundles and records), its data after the 16-byte header.
pub fn mnc2(file: &[u8]) -> Option<Vec<u8>> {
    (0..file.len().saturating_sub(16)).step_by(16).find(|&o| &file[o..o + 4] == b"mnc2").map(|o| file[o + 16..].to_vec())
}

/// What decoding needs besides the extdata.
pub struct AugCtx<'a> {
    pub tables: &'a AugTables,
    pub mnc2: Option<&'a [u8]>,
    pub text: &'a Messages,
    pub fmt88: Option<&'a Messages>,
    pub servers: Option<&'a Messages>,
}

/// Evolith slot: shape, level, element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    shape: u8,
    level: u8,
    element: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Plain,
    Evolith(Vec<Slot>),
    Serial { server: u8, no: u16 },
    Mezz { t: u8, rank: u8, rp: i32, groups: [usize; 3] },
    Craft { stage: u8, craft: u16 },
    Bundle { t: u8, rank: u8, max: u8, rp: i32, flags: u32, groups: [usize; 4] },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub pairs: Vec<(u16, i32)>,
    pub kind: Kind,
}

const NO_AUG_IDS: [u16; 8] = [0xffff, 0, 0x200, 0x201, 0x202, 0x203, 0x108c, 0x108d];

/// FUN_100fedc0 and byte 0 (FUN_100fee70 / FUN_100feef0): weapons and armour, not the listed ids, kind 2 or 3.
pub fn applies(id: u16, item_type: u16, ext: &[u8; 24]) -> bool {
    !NO_AUG_IDS.contains(&id) && !matches!(id, 0x1525 | 0x1526) && matches!(item_type, 4 | 5) && matches!(ext[0], 2 | 3)
}

/// FUN_101cb8a0 / FUN_101cb990: Some(ext[1]) when the Aug mark shows (kind 2 not for the no-mark weapons nor a
/// serialized item; kind 3 always).
pub fn marked(id: u16, item_type: u16, ext: Option<&[u8; 24]>, t: &AugTables) -> Option<u8> {
    let e = ext.filter(|e| applies(id, item_type, e))?;
    match e[0] {
        2 if t.no_mark.contains(&id) || e[1] & 0x10 != 0 => None,
        _ => Some(e[1]),
    }
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// FUN_100ff9c0's work record (0x310 bytes): s16 fields by byte offset, s8 fields by byte offset, the lists.
struct Work {
    item: u16,
    trial: u16,
    w: [i16; 0x40],
    b: [i8; 0x100],
    list: Vec<(u16, i32)>,
    pre: Vec<(u16, u16, i32)>,
    x1c4: (u16, u16),
    pet: u8,
    pets: Vec<(u16, i32)>,
    slot: u16,
}

impl Work {
    fn addw(&mut self, o: usize, v: i32) {
        self.w[o / 2] = self.w[o / 2].wrapping_add(v as i16);
    }

    fn addb(&mut self, o: usize, v: i32) {
        if let Some(b) = self.b.get_mut(o) {
            *b = b.wrapping_add(v as i8);
        }
    }

    /// FUN_10100850: the pet list, a stable ascending insert.
    fn pet(&mut self, kind: u8, id: u16, v: i32) {
        self.pet = kind;
        let at = self.pets.iter().position(|e| e.0 > id).unwrap_or(self.pets.len());
        self.pets.insert(at, (id, v));
    }
}

/// Handler table 0x10362538, ids 0..0x7f (`v` = value + 1).
fn handler(r: &mut Work, i: u16, v: i32) {
    let i32_ = i as i32;
    match i {
        1..=4 => r.addw(0x14, (i32_ - 1) * 32 + v),
        5..=8 => r.addw(0x14, (5 - i32_) * 32 - v),
        9..=0xc => r.addw(0x16, (i32_ - 9) * 32 + v),
        0xd..=0x10 => r.addw(0x16, (0xd - i32_) * 32 - v),
        0x11 | 0x12 => {
            let d = (i32_ - 0x11) * 32 + v;
            r.addw(0x14, d);
            r.addw(0x16, d);
        }
        0x13 | 0x14 => {
            r.addw(0x14, (i32_ - 0x13) * 32 + v);
            r.addw(0x16, (0x13 - i32_) * 32 - v);
        }
        0x15 | 0x16 => {
            r.addw(0x14, (0x15 - i32_) * 32 - v);
            r.addw(0x16, (i32_ - 0x15) * 32 + v);
        }
        0x17 | 0x18 | 0x3e => {
            let d = match i {
                0x17 => v,
                0x18 => -v,
                _ => v + 32,
            };
            r.addb(0x28, d);
            r.addb(0x29, d);
        }
        0x19 | 0x1a | 0x41 => {
            let d = match i {
                0x19 => v,
                0x1a => -v,
                _ => v + 32,
            };
            r.addb(0x2b, d);
            r.addb(0x2c, d);
        }
        0x1b => r.addb(0x2a, v),
        0x3f => r.addb(0x2a, v + 32),
        0x1c => r.addb(0x2a, -v),
        0x1d => r.addb(0x2d, v),
        0x42 => r.addb(0x2d, v + 32),
        0x1e => r.addb(0x2d, -v),
        0x1f => r.addw(0x2e, v),
        0x20 => r.addw(0x2e, -v),
        0x21 => r.addw(0x30, v),
        0x22 => r.addw(0x30, -v),
        0x23 => r.addb(0x35, v),
        0x40 => r.addb(0x35, v + 32),
        0x24 => r.addb(0x35, -v),
        0x25 => r.addw(0x32, v),
        0x26 => r.addw(0x32, -v),
        0x2c => {
            r.list.push((0x8e, v));
            r.list.push((0xc3, v));
        }
        0x2d => r.addw(0xa, v),
        0x4c => r.addw(0xa, v + 32),
        0x2e => r.addw(0xa, -v),
        0x2f => r.addw(0x12, v),
        0x30 => r.addw(0x12, -v),
        0x4d => r.addw(0x12, -32 - v),
        0x31 => r.addb(0x34, v),
        0x3a => r.addb(0x3e, -v),
        0x44 => [0x28, 0x29, 0x2b, 0x2c].into_iter().for_each(|o| r.addb(o, v)),
        0x45 => {
            r.addb(0x2a, v);
            r.addb(0x2d, v);
        }
        0x46 => {
            r.addb(0x35, v);
            r.addb(0x3d, v);
        }
        0x48 | 0x49 => r.addb(0x36, (i32_ << 5) + v),
        0x4a | 0x4b => r.addb(0x37, ((i32_ - 2) << 5) + v),
        0x4e => r.addw(0x14, 2 * v),
        0x4f => r.addw(0x14, 3 * v),
        0x52 => r.addw(0x16, 2 * v),
        0x53 => r.addw(0x16, 3 * v),
        _ => {
            // FUN_101003e0 thunks: (display id, signed value); the list keeps augment order.
            let list = match i {
                0x27 => Some((0x1d, v)),
                0x28 => Some((0x1d, -v)),
                0x29 => Some((0x1a, v)),
                0x2a => Some((0x1b, -v)),
                0x2b => Some((0x1c, v)),
                0x32..=0x35 => Some((0x21 + i - 0x32, v)),
                0x36..=0x38 => Some((0x25 + i - 0x36, -v)),
                0x39 => Some((0x28, v)),
                0x3b => Some((0x30, 10 * v)),
                0x3c => Some((0x31, v)),
                0x3d => Some((0x32, v)),
                0x43 => Some((0x33, v)),
                0x47 => Some((0x29, -v)),
                0x50 => Some((0x62, v)),
                0x51 => Some((0x63, v)),
                _ => None,
            };
            if let Some(e) = list {
                r.list.push(e);
                return;
            }
            // FUN_10100850 thunks: (kind, display id, value).
            let pet = match i {
                0x60 | 0x6a => Some((1, 0x81, v)),
                0x61 | 0x6b => Some((1, 0x82, v)),
                0x62..=0x64 => Some((1, 0x16 + i - 0x62, v)),
                0x65 => Some((1, 0x85, v)),
                0x66 => Some((1, 0x1a, v)),
                0x67 => Some((1, 0x1b, -v)),
                0x68 => Some((1, 0x1d, v)),
                0x69 => Some((1, 0x1d, -v)),
                0x6c => Some((1, 0x83, v)),
                0x6d => Some((1, 0x84, v)),
                0x6e => Some((1, 0x89, v)),
                0x6f => Some((1, 0x20, v)),
                0x70 => Some((1, 0x29, -v)),
                0x71 => Some((1, 0x14, v)),
                0x72 => Some((1, 0x15, v)),
                0x73 => Some((1, 0x8e, v)),
                0x74 => Some((1, 0xc3, v)),
                0x75 => Some((1, 0x19, v)),
                0x76 => Some((1, 0x25, -v)),
                0x77 => Some((1, 0x86, v)),
                0x78 => Some((2, 0x85, v)),
                0x79 => Some((1, 0x34, v)),
                0x7a => Some((1, 0x35, 20 * v)),
                0x7b => Some((1, 0x8f, v)),
                0x7c => Some((1, 0x61, v)),
                0x7d => Some((1, 0x62, v)),
                0x7e => Some((1, 0x16a, v)),
                0x7f => Some((1, 0x26, -v)),
                _ => None,
            };
            if let Some((k, d, x)) = pet {
                r.pet(k, d, x);
            }
        }
    }
}

/// The ids 0x480..0x4df remapped to handlers (table 0x10331828: {handler, multiplier}; the rest have none).
fn remap(i: u16) -> Option<(u16, i32)> {
    Some(match i {
        0x480 => (0x21, 10),
        0x481 => (0x1f, 3),
        0x482 => (0x25, 3),
        0x483 => (0x36, 2),
        0x484 => (0x37, 2),
        0x485 => (0x35, 2),
        0x486 => (0x3d, 2),
        0x4de => (0x76, 2),
        0x4df => (0x7f, 2),
        _ => return None,
    })
}

/// FUN_100ff9c0: the classic augments (4 with a trial word) folded into the work record.
fn classic(r: &mut Work, ext: &[u8; 24], t: &AugTables) {
    let n = if r.trial != 0 { 4 } else { 5 };
    for k in 0..n {
        let w = u16_at(ext, 2 + 2 * k);
        let (i, val) = (w & 0x7ff, (w >> 11) as i32);
        let v = val + 1;
        match i {
            0..0x80 => handler(r, i, v),
            0x80..0x100 => r.addb(i as usize - 0x48, v),
            0x100..0x140 => r.list.push((i - 1, v)),
            0x140..0x180 => {
                let m = match i {
                    0x153 => 5,
                    0x156 | 0x168 => 10,
                    0x161 => 50,
                    _ => 1,
                };
                r.list.push((i, v * m));
            }
            0x200..0x280 | 0x700..0x720 => {
                let pet = i >= 0x700;
                let j = (i & 0xff) as usize;
                let [add, sub] = if pet { t.pet.get(j) } else { t.attr.get(j) }.copied().unwrap_or([0, 0]);
                let halve = if pet { (0xe..=0x1f).contains(&j) } else { (0xe..=0x25).contains(&j) };
                let vs = if halve { ((v + 1) / 2).max(1) } else { v };
                let base = if pet { 0xb8 } else { 0x18 };
                if pet {
                    r.pet = 1;
                }
                for b in 0..7 {
                    if add >> b & 1 != 0 {
                        r.addb(base + b, v);
                    }
                    if sub >> b & 1 != 0 {
                        r.addb(base + b, -vs);
                    }
                }
            }
            // Table 0x103319a8: only 0x280 is set (display 0x91 += 2 v).
            0x280 => r.addb(0x91 - 0x80 + 0x38, 2 * v),
            0x2e4..0x300 => {
                let (lo, o, pos) = match i {
                    0x2e4..0x2e8 => (0x2e4, 0xa, true),
                    0x2e8..0x2ea => (0x2e8, 0xa, false),
                    0x2ea..0x2ee => (0x2ea, 0xc, true),
                    0x2ee..0x2f0 => (0x2ee, 0xc, false),
                    0x2f0..0x2f4 => (0x2f0, 0xe, true),
                    0x2f4..0x2f8 => (0x2f4, 0xe, false),
                    0x2f8..0x2fc => (0x2f8, 0x10, true),
                    _ => (0x2fc, 0x10, false),
                };
                let d = (i as i32 - lo) * 32;
                r.addw(o, if pos { d + v } else { -d - v });
            }
            0x300..0x340 => {
                let [add, sub] = t.elem.get((i - 0x300) as usize).copied().unwrap_or([0, 0]);
                for b in 0..8 {
                    if add >> b & 1 != 0 {
                        r.addb(0x20 + b, v);
                    }
                    if sub >> b & 1 != 0 {
                        r.addb(0x20 + b, -v);
                    }
                }
            }
            0x340..0x348 if r.item >= 0x4000 => r.x1c4 = (i - 0x17f, (val + 5) as u16),
            0x348..0x35a if r.item >= 0x4000 => r.x1c4 = (i - 0x17f, v as u16),
            0x380..0x390 if r.item >= 0x4000 => r.list.push((i - 0x180, v)),
            0x390..0x393 => r.list.push(((i - 0x36e) * 16 + val as u16, 1)),
            0x3a0..0x3b8 => r.list.push((i - 0x150, val)),
            0x3b8..0x3c0 => r.list.push((i - 0x150, v)),
            0x3c0..0x3d0 => {
                r.list.push((i - 0x168, val));
                r.list.push((i - 0x150, val * 2 + 2));
            }
            0x3d0..0x3d8 => r.list.push((i - 0xd0, v)),
            0x3d8..0x3e8 => r.list.push((i - 0x168, val * 2 + 2)),
            0x3e8..0x3f0 => {
                r.list.push((i - 0x190, val));
                r.list.push((i - 0x170, (val * 3 + 3) * 2));
            }
            0x3f0..0x3f8 => {
                r.list.push((i - 0x190, val));
                r.list.push((i - 0x180, (val * 3 + 3) * 2));
            }
            0x400..0x480 | 0x600..0x680 => {
                let j = (i & 0x7f) as usize;
                if r.pre.len() < 5
                    && let Some(e) = t.prefix.get(j)
                {
                    r.pre.push((e[1] as u16, u16::from_le_bytes([e[2], e[3]]) >> 4, v * 5));
                }
            }
            0x480..0x4e0 => {
                if let Some((h, m)) = remap(i) {
                    handler(r, h, (m * v) as i16 as i32);
                }
            }
            0x4e0..0x500 => r.list.push((i, v)),
            0x500..0x518 | 0x530..0x5c0 => r.list.push((i - 0x1e0, v)),
            0x5c0 => r.list.push((0x36, v)),
            0x680..=0x690 => r.slot = i - 0x67f,
            0x7ff => r.list.push((0x7fe, 1)),
            _ => {}
        }
    }
}

/// FUN_100ff2f0's packer: five units a line, a pair skipped when its value is 0; (0, 0) is a line break.
struct Packer {
    out: Vec<(u16, i32)>,
    budget: i32,
}

impl Packer {
    fn add(&mut self, id: u16, v: i32, w: i32) {
        if v == 0 {
            return;
        }
        if self.budget < w {
            self.out.push((0, 0));
            self.budget = 5;
        }
        self.out.push((id, v));
        self.budget -= w;
    }

    fn brk(&mut self) {
        if self.budget < 5 {
            self.out.push((0, 0));
            self.budget = 5;
        }
    }
}

/// FUN_100ff2f0: the work record in its fixed order.
fn emit(r: &Work) -> Vec<(u16, i32)> {
    let mut p = Packer { out: Vec::new(), budget: 5 };
    let w = |o: usize| r.w[o / 2] as i32;
    let b = |o: usize| r.b[o] as i32;
    if (1..0x11).contains(&r.slot) {
        p.add(r.slot + 0x67e, 1, 1);
    }
    p.add(0x1e, w(0xa), 1);
    p.add(0x1e, w(0xc), 1);
    p.add(0x2e, w(0xe), 1);
    if r.trial == 0 {
        p.add(0x1f, w(0x12), 1);
    }
    p.add(0x2e, w(0x10), 1);
    p.add(9, w(0x14), 1);
    p.add(10, w(0x16), 1);
    for k in 0..7 {
        p.add(0xb + k as u16, b(0x18 + k), 1);
    }
    for k in 0..8 {
        p.add(1 + k as u16, b(0x20 + k), 1);
    }
    p.add(0x12, b(0x28), 1);
    p.add(0x13, b(0x2b), 1);
    p.add(0x14, b(0x2a), 3);
    p.add(0x15, b(0x2d), 3);
    p.add(0x16, w(0x2e), 1);
    p.add(0x17, w(0x30), 1);
    p.add(0x18, b(0x35), 3);
    p.add(0x19, w(0x32), 1);
    p.add(0x20, b(0x34), 1);
    for i in 0x80..0x100usize {
        p.add(i as u16, b(i - 0x48), 5);
    }
    for &(i, v) in &r.list {
        p.add(i, v, 5);
    }
    for &(pre, i, v) in &r.pre {
        if i != 0 {
            p.brk();
            p.add(pre + 0x280, 1, 2);
            p.add(i, v, 2);
        }
    }
    p.add(0x500, b(0x36), 2);
    p.add(0x501, b(0x37), 3);
    p.add(r.x1c4.0, r.x1c4.1 as i32, 5);
    if r.pet != 0 {
        p.brk();
        p.add(if r.pet == 1 { 0x80 } else { 0x87 }, 1, 1);
        for k in 0..7 {
            p.add(0xb + k as u16, b(0xb8 + k), 1);
        }
        for &(i, v) in &r.pets {
            match i {
                0x81 => [(0x12, 2), (0x14, 2)].into_iter().for_each(|(d, w)| p.add(d, v, w)),
                0x82 => [(0x13, 2), (0x15, 2)].into_iter().for_each(|(d, w)| p.add(d, v, w)),
                0x83 => [(0x18, 4), (0x85, 4)].into_iter().for_each(|(d, w)| p.add(d, v, w)),
                0x84 => [(0x8f, 2), (0x1a, 2)].into_iter().for_each(|(d, w)| p.add(d, v, w)),
                0xac => p.add(0x34, v, 2),
                0xad => p.add(0x35, v, 2),
                _ => p.add(i, v, 5),
            }
        }
    }
    if r.trial > 0 && r.trial < 0x8000 {
        p.add(0x7ff, r.trial as i32, 5);
    }
    p.out
}

/// FUN_10100ed0: per slot with a shape, (prefix, 0), (suffix, value[rank]), or the empty-slot pair.
fn evolith(slots: &[(u16, u8, u8, u8, u8)], t: &AugTables) -> Record {
    let mut pairs = Vec::new();
    let mut shown = Vec::new();
    for &(aug, rank, shape, element, level) in slots {
        if shape == 0 {
            continue;
        }
        match t.evolith.get(aug as usize).filter(|_| aug != 0) {
            Some(e) => {
                pairs.push((u16::from_le_bytes([e[0], e[1]]), 0));
                pairs.push((u16::from_le_bytes([e[2], e[3]]), e[4 + rank as usize % 16] as i8 as i32));
            }
            None => pairs.extend([(0x3ff, 0), (0x3fe, 0)]),
        }
        shown.push(Slot { shape, level, element });
    }
    Record { pairs, kind: Kind::Evolith(shown) }
}

/// FUN_100ff140: the form by item id and the extdata flags.
pub fn decode(ext: &[u8; 24], item: u16, ctx: &AugCtx) -> Record {
    let t = ctx.tables;
    let plain = |pairs| Record { pairs, kind: Kind::Plain };
    if item < 0x2800 {
        let d = u32_at(ext, 0);
        let lvl = (d >> 17 & 0xf) as u8;
        return evolith(&[((d & 0x3ff) as u16, lvl, (d >> 10 & 0xf) as u8, (d >> 14 & 7) as u8, lvl)], t);
    }
    match (ext[0], ext[1]) {
        (2, f) if f & 0x80 != 0 => {
            let w: [u16; 3] = std::array::from_fn(|k| u16_at(ext, 2 + 2 * k));
            let w10 = u16_at(ext, 10);
            let slots = [
                (w[0] & 0x7ff, (w[0] >> 11 & 0xf) as u8, ext[8] & 0xf, ext[11] & 7, ext[9] >> 4),
                (w[1] & 0x7ff, (w[1] >> 11 & 0xf) as u8, ext[8] >> 4, (w10 >> 11 & 7) as u8, ext[10] & 0xf),
                (w[2] & 0x7ff, (w[2] >> 11 & 0xf) as u8, ext[9] & 0xf, ((w10 >> 14) + (w[2] >> 15) * 4) as u8, ext[10] >> 4),
            ];
            evolith(&slots, t)
        }
        (2, f) if f & 0x10 != 0 => Record { pairs: Vec::new(), kind: Kind::Serial { server: ext[4].max(1), no: u16_at(ext, 6) } },
        (2, f) if f & 0x20 != 0 => mezzotint(ext, t),
        (2, f) if f & 0x08 != 0 => escutcheon(ext),
        (2, f) => {
            let trial = if f & 0x40 != 0 { u16_at(ext, 10) } else { 0 };
            let mut r = Work { item, trial, w: [0; 0x40], b: [0; 0x100], list: Vec::new(), pre: Vec::new(), x1c4: (0, 0), pet: 0, pets: Vec::new(), slot: 0 };
            classic(&mut r, ext, t);
            plain(emit(&r))
        }
        (3, f) if f & 0x80 != 0 => ctx.mnc2.map_or_else(|| plain(Vec::new()), |m| bundle(ext, m)),
        _ => plain(Vec::new()),
    }
}

/// FUN_101011c0: type, rank and RP; three slots through the index table (masks over STR..CHR, pet / automaton
/// prefixes, the odd-index +256 twins, negation).
fn mezzotint(ext: &[u8; 24], t: &AugTables) -> Record {
    let (ty, rank) = (ext[2] & 3, ext[2] >> 2 & 0x1f);
    let rp = if ext[2] & 0x7c == 0x3c { 0 } else { (t.mezz_rp.get(rank as usize).copied().unwrap_or(0) as i32 - u16_at(ext, 4) as i32).max(0) };
    let mut pairs = Vec::new();
    let mut groups = [0; 3];
    for (s, g) in groups.iter_mut().enumerate() {
        let (idx, val) = (ext[6 + 2 * s] as usize, ext[7 + 2 * s] as i32);
        let e = t.mezz.get(idx).copied().unwrap_or(0);
        let mask = e & 0x800 != 0;
        let x = if mask { e & 0x7f } else { e & 0x7ff };
        if x == 0 {
            continue;
        }
        match e >> 14 {
            1 => pairs.push((0x7fd, 0)),
            2 => pairs.push((0x80, 0)),
            _ => {}
        }
        let mut v = val + 1 + if (2..=0x5f).contains(&idx) && idx & 1 == 1 { 0x100 } else { 0 };
        if e & 0x1000 != 0 {
            v = -v;
        }
        if mask {
            (0..7).filter(|b| x >> b & 1 != 0).for_each(|b| pairs.push((0xb + b, v)));
        } else {
            pairs.push((x, v));
        }
        *g = pairs.len();
    }
    Record { pairs, kind: Kind::Mezz { t: ty, rank, rp, groups } }
}

/// FUN_10101aa0 / FUN_10101a00: the success-rate penalty by stage and the craftsmanship objective.
fn escutcheon(ext: &[u8; 24]) -> Record {
    let mut p = Packer { out: Vec::new(), budget: 5 };
    if ext[4] == 0 || ext[8] & 0xf == 0 {
        return Record { pairs: Vec::new(), kind: Kind::Plain };
    }
    let (stage, craft) = (ext[8] & 0xf, u16_at(ext, 6));
    let pen = match stage {
        1 => 30 - craft as i32 / 100,
        2 => (craft as i32 / 300).min(15),
        3 => {
            let x = (u32_at(ext, 8) >> 4 & 0xffff) as i32 - 0x80;
            if x >= 0 { 0 } else { (-x / 3).min(30) }
        }
        4 => 0,
        _ => 100,
    };
    if pen > 0 {
        p.add(0x7ce, -pen, 1);
    }
    if ext[5] != 0 {
        p.brk();
        p.add(0x79d, 1, 1);
        p.brk();
        p.add(0x79d + ext[5] as u16, 1, 1);
    }
    Record { pairs: p.out, kind: Kind::Craft { stage, craft } }
}

/// FUN_10101360 / FUN_10101470: the bundle's type, rank, max rank and RP to the next rank, then up to four records.
fn bundle(ext: &[u8; 24], m: &[u8]) -> Record {
    let d4 = u32_at(ext, 4);
    let (ty, rank) = ((d4 & 3) as u8, (d4 >> 18 & 0x1f) as u8);
    let max = ((((d4 >> 23 & 3) + 3) * 5).min(30)) as u8;
    let curve = |r: u8| m.get(((d4 >> 26 & 3) as usize * 32 + r as usize) * 2..).map_or(0, |b| u16_at(b, 0) as i32);
    let rp = if rank == max { 0 } else { curve(rank) - (d4 >> 2 & 0xffff) as i32 };
    let n = i16::from_le_bytes([ext[8], ext[9]]) as i32;
    let flags = if n >= 0 { m.get(0x88 + n as usize * 12..).map_or(0, |b| u32_at(b, 0)) } else { 0 };
    let mut pairs = Vec::new();
    let mut groups = [0; 4];
    if n > 0 {
        for (s, g) in groups.iter_mut().enumerate() {
            let ri = m.get(0x80 + (s + n as usize * 6) * 2..).map_or(0, |b| u16_at(b, 0)) as usize;
            if ri == 0 {
                break;
            }
            if let Some(rec) = m.get(0x3080 + ri * 0x4c..0x3080 + ri * 0x4c + 0x4c) {
                bundle_rec(&mut pairs, rec, rank, None);
            }
            *g = pairs.len();
        }
    }
    Record { pairs, kind: Kind::Bundle { t: ty, rank, max, rp, flags, groups } }
}

/// FUN_10101950: one bundle record at a rank (masks, attribute lists, prefixes, weapon-skill DMG).
fn bundle_rec(out: &mut Vec<(u16, i32)>, rec: &[u8], rank: u8, typ: Option<i16>) {
    let s16 = |o: usize| i16::from_le_bytes([rec[o], rec[o + 1]]);
    let t = typ.unwrap_or_else(|| s16(8));
    let p = s16(10);
    let v = s16(12 + 2 * rank as usize % 62) as i32;
    if v == 0 {
        out.push((0x3ff, 0));
        return;
    }
    let mut expand = |ids: &[u16], sep: bool| {
        let mut first = true;
        for (k, &i) in ids.iter().enumerate() {
            if p >> k & 1 != 0 {
                if sep && !first {
                    out.push((0x7cf, 1));
                }
                out.push((i, v));
                first = false;
            }
        }
    };
    match t as u16 {
        0x801 => expand(&[9, 10, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11], false),
        0x803 => expand(&[1, 2, 3, 4, 5, 6, 7, 8], false),
        0x804 => expand(&[0x12, 0x14, 0x18], false),
        0x805 => expand(&[0x13, 0x15, 0x85, 0x16a], false),
        0x806 => expand(&[0x17, 0x86, 0x16, 0x19], false),
        0x807 => expand(&[0x12, 0x14, 0x13, 0x85, 0x17], false),
        0x808 => expand(&[0x18, 0x85, 0x16a, 0x86, 0x19], false),
        0x802 if p != 0x7f => {
            expand(&[0x7d2, 0x7d3, 0x7d4, 0x7d5, 0x7d6, 0x7d7, 0x7d8], true);
            out.push((0x7d0, v));
        }
        0x802 | 0x1000 => out.extend([(0x7d9, v), (0x7d0, v)]),
        0x161 => out.push((0x161, v * 10)),
        0x81c => out.extend([(p as u16, 1), (0x2f, v)]),
        x @ 0x810..=0x81b => {
            const PREF: [u16; 12] = [0x80, 0x7fc, 0x87, 0x7fd, 0x7fb, 0x7fa, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
            out.push((PREF[(x - 0x810) as usize], 1));
            bundle_rec(out, rec, rank, Some(p));
        }
        x if x < 0x800 => out.push((x, v)),
        _ => {}
    }
}

/// MSVC sprintf of one 55692 entry with `v` as every argument (%d, %+d, %%, widths and the 0 flag).
fn cfmt(f: &[u8], v: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        if c >= 0x80 {
            out.extend_from_slice(&f[i..(i + 2).min(f.len())]);
            i += 2;
            continue;
        }
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if f.get(j) == Some(&b'%') {
            out.push(b'%');
            i = j + 1;
            continue;
        }
        let mut flags = Vec::new();
        while let Some(&ch) = f.get(j).filter(|ch| b"-+ 0#".contains(ch)) {
            flags.push(ch);
            j += 1;
        }
        let ws = j;
        while f.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        let width: usize = std::str::from_utf8(&f[ws..j]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        let conv = f.get(j).copied().unwrap_or(b'd');
        if matches!(conv, b'd' | b'i') {
            let body = v.unsigned_abs().to_string();
            let sign = if v < 0 { "-" } else if flags.contains(&b'+') { "+" } else { "" };
            let mut t = format!("{sign}{body}");
            if t.len() < width {
                t = if flags.contains(&b'0') { format!("{sign}{}{body}", "0".repeat(width - t.len())) } else { format!("{t:>width$}") };
            }
            out.extend_from_slice(t.as_bytes());
        }
        i = j + 1;
    }
    out
}

/// FUN_1013b9c0: "{FIR}".."{DKN}" become the element glyphs EF 1F..EF 26.
fn tags(b: &[u8]) -> Vec<u8> {
    const T: [&[u8]; 8] = [b"{FIR}", b"{ICE}", b"{WND}", b"{ERT}", b"{LTG}", b"{WTR}", b"{LIT}", b"{DKN}"];
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(k) = T.iter().position(|t| b[i..].starts_with(t)) {
            out.extend([0xef, 0x1f + k as u8]);
            i += 5;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// The shape glyphs (pointer table 0x1036fcf0): none, ▼ ◆ ★ △ □ ○ ▽ ◇ ☆ ▲ ■ ● ◎.
const SHAPES: [[u8; 2]; 14] = [
    [0, 0],
    [0x81, 0xa5],
    [0x81, 0x9f],
    [0x81, 0x9a],
    [0x81, 0xa2],
    [0x81, 0xa0],
    [0x81, 0x9b],
    [0x81, 0xa4],
    [0x81, 0x9e],
    [0x81, 0x99],
    [0x81, 0xa3],
    [0x81, 0xa1],
    [0x81, 0x9c],
    [0x81, 0x9d],
];

fn shape(k: u8) -> Vec<u8> {
    SHAPES.get(k as usize).filter(|s| s[0] != 0).map_or(Vec::new(), |s| s.to_vec())
}

/// FUN_1013b280 mode 0 (2 for a stone): the block, groups prefixed "[n]" for bundles, mezzotints and evolith slots;
/// a text that would pass the 0xbf-byte room is left out but still counted.
pub fn block(r: &Record, stone: bool, ctx: &AugCtx) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut room = 0xbf_i32;
    let mut put = |buf: &mut Vec<u8>, room: &mut i32, (i, v): (u16, i32)| {
        let t = if i == 0 { b"\n".to_vec() } else { ctx.text.raw(i as usize).map(|f| cfmt(&f, v)).unwrap_or_default() };
        if (t.len() as i32) < *room {
            buf.extend_from_slice(&t);
        }
        *room -= t.len() as i32;
    };
    if stone {
        for &p in r.pairs.iter().take(2) {
            put(&mut buf, &mut room, p);
        }
        return tags(&buf);
    }
    let groups = |buf: &mut Vec<u8>, room: &mut i32, counts: &[usize], head: Option<u16>, put: &mut dyn FnMut(&mut Vec<u8>, &mut i32, (u16, i32))| {
        if let Some(h) = head {
            put(buf, room, (h, 0));
            buf.push(b'\n');
        }
        let mut j = 0;
        for (g, &c) in counts.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let lab = format!("[{}]", g + 1);
            if *room < lab.len() as i32 + 1 {
                break;
            }
            buf.extend_from_slice(lab.as_bytes());
            *room -= lab.len() as i32 + 1;
            while j < c {
                put(buf, room, r.pairs[j]);
                j += 1;
            }
            buf.push(b'\n');
        }
    };
    match &r.kind {
        Kind::Bundle { flags, groups: g, .. } if g.iter().any(|&c| c != 0) => groups(&mut buf, &mut room, g, (flags & 1 != 0).then_some(0x520), &mut put),
        Kind::Mezz { groups: g, .. } if g.iter().any(|&c| c != 0) => groups(&mut buf, &mut room, g, None, &mut put),
        Kind::Evolith(slots) if !slots.is_empty() => {
            for k in 0..slots.len() {
                let lab = format!("[{}]", k + 1);
                if room < lab.len() as i32 + 1 {
                    break;
                }
                buf.extend_from_slice(lab.as_bytes());
                room -= lab.len() as i32 + 1;
                for p in r.pairs.iter().skip(2 * k).take(2) {
                    put(&mut buf, &mut room, *p);
                }
                buf.push(b'\n');
            }
        }
        _ => {
            for &p in &r.pairs {
                put(&mut buf, &mut room, p);
            }
        }
    }
    tags(&buf)
}

/// Replaces %c / %d / %02d / %% in order (the 55688 short formats).
fn fill(f: &[u8], args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = args.iter();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' {
            let rest = &f[i + 1..];
            let (len, a) = if rest.starts_with(b"%") {
                out.push(b'%');
                (2, None)
            } else if rest.starts_with(b"02d") {
                (4, Some(true))
            } else if rest.first().is_some_and(|c| matches!(c, b'd' | b'c' | b's')) {
                (2, Some(false))
            } else {
                out.push(b'%');
                (1, None)
            };
            if let Some(zero) = a
                && let Some(arg) = it.next()
            {
                match arg {
                    Arg::N(n) if zero => out.extend(format!("{n:02}").bytes()),
                    Arg::N(n) => out.extend(n.to_string().bytes()),
                    Arg::C(c) => out.push(*c),
                    Arg::S(s) => out.extend_from_slice(s),
                }
            }
            i += len;
        } else {
            out.push(f[i]);
            i += 1;
        }
    }
    out
}

enum Arg {
    N(i32),
    C(u8),
    S(Vec<u8>),
}

/// FUN_1013b280 mode 1 (3 for a stone): the short line and its colour (0x80407090 at the top rank).
pub fn short(r: &Record, stone: bool, ctx: &AugCtx) -> Option<(Vec<u8>, Option<u32>)> {
    let f88 = |k: usize, fallback: &str| ctx.fmt88.and_then(|m| m.raw(k)).unwrap_or_else(|| fallback.as_bytes().to_vec());
    let elem = |e: u8| vec![0xef, 0x1f + (e & 7)];
    match &r.kind {
        Kind::Evolith(slots) if stone => slots.first().map(|s| ([elem(s.element), shape(s.shape), (s.level as i32 + 1).to_string().into_bytes()].concat(), None)),
        Kind::Evolith(slots) if !slots.is_empty() => {
            let mut s: Vec<u8> = slots.iter().enumerate().flat_map(|(k, s)| [format!("[{}]", k + 1).into_bytes(), elem(s.element), shape(s.shape), format!("{} ", s.level as i32 + 1).into_bytes()].concat()).collect();
            s.pop();
            Some((s, None))
        }
        Kind::Serial { server, no } => {
            let name = ctx.servers.and_then(|m| m.raw(*server as usize)).unwrap_or_default();
            Some(([name, format!(" No. {no:05}").into_bytes()].concat(), None))
        }
        Kind::Mezz { t, rank, rp, .. } => Some((fill(&f88(0x3f, "%c/%d/%d"), &[Arg::C(b'A' + t), Arg::N(*rank as i32), Arg::N(*rp)]), (*rank >= 15).then_some(0x8040_7090))),
        Kind::Bundle { t, rank, max, rp, .. } => Some((fill(&f88(0x3b, "%c/%d(%d)/%d"), &[Arg::C(b'A' + t), Arg::N(*rank as i32), Arg::N(*max as i32), Arg::N(*rp)]), (max <= rank).then_some(0x8040_7090))),
        Kind::Craft { stage, craft } => {
            let cap = match stage {
                1 => 3000,
                2 => 5000,
                3 => 10000,
                4 => 30000,
                _ => 9999,
            };
            let q = (*craft as i32 * 10000 / cap).min(10000);
            Some((fill(&f88(0x3d, "%d.%d"), &[Arg::N(q / 100), Arg::N(q % 100)]), None))
        }
        _ => None,
    }
}

/// FUN_101ca520 / FUN_101ca570: lines no wider than `max`, broken after the last of " \t,/" (kept on the line, no
/// trimming) or before the overflowing character; a '\n' ends its line and belongs to it.
pub fn wrap(s: &str, max: f32, width: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    let mut last_break = None;
    for c in s.chars() {
        if c == '\n' {
            line.push(c);
            out.push(std::mem::take(&mut line));
            last_break = None;
            continue;
        }
        line.push(c);
        if width(line.trim_end_matches('\n')) > max {
            let cut = match last_break {
                Some(b) => b,
                None => line.len() - c.len_utf8(),
            };
            let rest = line.split_off(cut);
            out.push(std::mem::take(&mut line));
            line = rest;
            last_break = line.char_indices().filter(|(_, ch)| matches!(ch, ' ' | '\t' | ',' | '/')).last().map(|(i, ch)| i + ch.len_utf8());
        } else if matches!(c, ' ' | '\t' | ',' | '/') {
            last_break = Some(line.len());
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        tables: AugTables,
        mnc2: Option<Vec<u8>>,
        text: Messages,
        fmt88: Option<Messages>,
        servers: Option<Messages>,
    }

    fn env() -> Option<Env> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?;
        let msgs = |f: Option<u32>| f.and_then(|f| Messages::load(&index, f).ok());
        Some(Env {
            tables: AugTables::find(&dll)?,
            mnc2: index.read(81).ok().and_then(|f| mnc2(&f)),
            text: msgs(crate::dat::msg::slot_file(&dll, 1, 0x1b))?,
            fmt88: msgs(crate::dat::msg::slot_file(&dll, 1, 0x1c)),
            servers: msgs(crate::dat::msg::group_file(&dll, 1, 0x16)),
        })
    }

    fn std_ext(words: &[u16], flags: u8, trial: Option<u16>) -> [u8; 24] {
        let mut e = [0u8; 24];
        (e[0], e[1]) = (2, flags);
        for (k, w) in words.iter().enumerate() {
            e[2 + 2 * k..4 + 2 * k].copy_from_slice(&w.to_le_bytes());
        }
        if let Some(t) = trial {
            e[10..12].copy_from_slice(&t.to_le_bytes());
        }
        e
    }

    fn w(i: u16, v: u16) -> u16 {
        i | (v - 1) << 11
    }

    fn text(env: &Env, ext: &[u8; 24], item: u16) -> (String, Option<String>) {
        let ctx = AugCtx { tables: &env.tables, mnc2: env.mnc2.as_deref(), text: &env.text, fmt88: env.fmt88.as_ref(), servers: env.servers.as_ref() };
        let r = decode(ext, item, &ctx);
        let b = crate::dat::font::decode_sjis(&block(&r, false, &ctx));
        let s = short(&r, false, &ctx).map(|s| crate::dat::font::decode_sjis(&s.0));
        (b, s)
    }

    #[test]
    fn classic_vectors() {
        let Some(e) = env() else { return };
        assert_eq!(text(&e, &std_ext(&[w(1, 10), w(9, 5), w(23, 3), w(25, 3)], 3, None), 0x3000).0, "HP+10 MP+5 Accuracy+3 Attack+3 ");
        assert_eq!(text(&e, &std_ext(&[w(23, 5), w(25, 5), w(27, 5), w(29, 5), w(31, 5)], 3, None), 0x3000).0, "Accuracy+5 Attack+5 Rng. Acc.+5 \nRng. Atk.+5 Evasion+5 ");
        assert_eq!(text(&e, &std_ext(&[w(44, 3), w(45, 4), w(48, 3)], 3, None), 0x4000).0, "DMG:+4 Delay:-3% \n\"Store TP\"+3 \n\"Subtle Blow\"+3 ");
        assert_eq!(text(&e, &std_ext(&[w(0x400, 2), w(0x480, 2)], 3, None), 0x4000).0, "DEF+20 \nBackhand Blow: DMG:+10% ");
        assert_eq!(text(&e, &std_ext(&[w(1792, 4), w(99, 3)], 3, None), 0x3000).0, "Pet: STR+4 \nDEF+3 ");
        assert_eq!(text(&e, &std_ext(&[w(1, 5)], 0x43, Some(1234)), 0x3000).0, "HP+5 \n[Trial 1234]");
        assert_eq!(text(&e, &std_ext(&[w(1, 5)], 0x43, Some(0x8000 | 77)), 0x3000).0, "HP+5 ");
        assert_eq!(text(&e, &std_ext(&[0x07ff], 3, None), 0x3000).0, "???");
        assert_eq!(text(&e, &std_ext(&[w(18, 1), w(4, 1)], 3, None), 0x3000).0, "HP+130 MP+33 ");
    }

    #[test]
    fn other_forms() {
        let Some(e) = env() else { return };
        let mut m = [0u8; 24];
        (m[0], m[1], m[2]) = (2, 0x23, (5 << 2) | 1);
        m[4..6].copy_from_slice(&100u16.to_le_bytes());
        m[6..12].copy_from_slice(&[0x0c, 9, 0x1c, 4, 0xbf, 2]);
        assert_eq!(text(&e, &m, 0x3000), ("[1]Accuracy+10 \n[2]DMG:+5 \n[3]STR+3 \n".into(), Some("Type:B/Rank:5/RP:120".into())));
        let mut s = [0u8; 24];
        (s[0], s[1], s[4]) = (2, 0x13, 0x64);
        s[6..8].copy_from_slice(&42u16.to_le_bytes());
        assert_eq!(text(&e, &s, 0x4000), (String::new(), Some("Bahamut No. 00042".into())));
        let mut c = [0u8; 24];
        (c[0], c[1], c[4], c[5]) = (2, 0x0b, 3, 0x0a);
        c[6..8].copy_from_slice(&1500u16.to_le_bytes());
        c[8] = 1;
        assert_eq!(text(&e, &c, 0x3000), ("Synthesis success rate -15\nFor craftsmanship upgrade:\nCraft w/fire crystals".into(), Some("Craftsmanship: 50.00%".into())));
        let mut b = [0u8; 24];
        (b[0], b[1]) = (3, 0x80);
        b[4..8].copy_from_slice(&(1u32 | (500 << 2) | (10 << 18)).to_le_bytes());
        b[8] = 7;
        assert_eq!(text(&e, &b, 0x4000), ("[1]Chance of double damage +20% \n[2]\"Store TP\"+10 \n[3]DMG:+4 \n".into(), Some("Type:B/Rank:10[15]/NextRP:60".into())));
    }

    #[test]
    fn wraps_like_retail() {
        let width = |s: &str| s.chars().count() as f32;
        assert_eq!(wrap("WAR/MNK/WHM", 6.0, &width), vec!["WAR/".to_string(), "MNK/".into(), "WHM".into()]);
        assert_eq!(wrap("ab\ncd", 9.0, &width), vec!["ab\n".to_string(), "cd".into()]);
    }
}
