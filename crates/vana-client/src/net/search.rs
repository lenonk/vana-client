//! The search server client (LSB xi_search; FFXiMain FUN_100e0d50, FUN_100d8c60, FUN_100e2060): one TCP connection
//! per query, an "IXFF" request Blowfish-keyed by MD5(fixed key, salt), replies keyed by MD5(fixed key, salt, session
//! word), each player a bit-packed entry. LSB sends every page at once, so a query reads until the last-packet flag
//! and closes (retail's "more" requests get only empty auction packets from LSB).

use super::map::blowfish::Blowfish;
use anyhow::{Result, anyhow, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// DAT_104b072c, LSB SearchHandler::key.
const FIXED: &[u8; 16] = b"0s=m<1IZ2zBCc8{~";
/// A slot older than 30 s fails (FUN_100e0d50).
const TIMEOUT: Duration = Duration::from_secs(30);

/// Condition flags (+0x00): bit `1 << search type` for the typed fields.
pub const NAME: u32 = 1;
pub const AREA: u32 = 2;
pub const NATION: u32 = 4;
pub const JOB: u32 = 8;
pub const LEVEL: u32 = 0x10;
pub const RACE: u32 = 0x20;
pub const STATUS: u32 = 0x40;
pub const MLEVEL: u32 = 0x80;
pub const LS1: u32 = 0x800;
pub const FRIEND: u32 = 0x1000;
pub const RANK: u32 = 0x10000;
pub const COMMENT: u32 = 0x20000;
pub const LS2: u32 = 0x80000;

/// What to search for: FUN_10103ec0's condition (0x68 bytes) with the fields set so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cond {
    pub flags: u32,
    /// +0x04: the search types typed with + / - (bit 7 descending), in typing order.
    pub sorts: Vec<u8>,
    /// +0x0c, at most 15 chars.
    pub name: String,
    /// +0x1c / +0x1e: the level, or the master level (the same fields).
    pub level: (u8, u8),
    /// +0x24, newest first; none with the area flag = all areas.
    pub areas: Vec<u16>,
    /// +0x22: the last area set (-1 for ALL).
    pub last_area: i16,
    pub nation: u8,
    pub job: u8,
    pub race: u8,
    /// +0x4c / +0x4d.
    pub rank: (u8, u8),
    /// +0x4e bit 1: an area was given (wire type 3).
    pub area_given: bool,
    /// +0x50 / +0x54.
    pub ls: [u32; 2],
    /// +0x60 (its low half is +0x4a).
    pub status: u32,
    /// +0x5c: the comment type (genre << 4 | sub; FUN_10108040).
    pub comment: u32,
    /// A group list instead (kind 2, FUN_100e2f10; setters FUN_10107640..670): party, alliance, LS1, LS2 ids.
    pub group: Option<[u32; 4]>,
}

impl Cond {
    /// The group list request (wire type 2): the four ids and a zero word (LSB: party / alliance first, else LS1,
    /// else LS2; no reply when all are 0).
    pub fn group(ids: [u32; 4]) -> Self {
        Cond { group: Some(ids), ..Default::default() }
    }

    /// `/sea all`: the area flag with no area, wire type 0.
    #[cfg(test)]
    pub fn all() -> Self {
        Cond { flags: AREA, last_area: -1, ..Default::default() }
    }

    /// One area (FUN_10107b00), wire type 3.
    pub fn zone(z: u16) -> Self {
        Cond { flags: AREA, areas: vec![z], last_area: z as i16, area_given: true, ..Default::default() }
    }

    /// Name only, all areas (FUN_1007c740's lookup), wire type 0.
    pub fn name(n: impl Into<String>) -> Self {
        Cond { flags: NAME, name: n.into(), ..Default::default() }
    }

    /// FUN_10107b40: no area (flag 2 off, +0x4e bit 1 off, +0x22 = -1).
    pub fn clear_area(&mut self) {
        self.flags &= !AREA;
        (self.areas, self.last_area, self.area_given) = (Vec::new(), -1, false);
    }

    /// FUN_10107b00: one area, wire type 3.
    pub fn single_area(&mut self, z: u16) {
        self.flags |= AREA;
        (self.areas, self.last_area, self.area_given) = (vec![z], z as i16, true);
    }

    /// FUN_10107a80: an area in front (no duplicates, the oldest drops past 16); +0x4e bit 1 untouched.
    pub fn add_area(&mut self, z: u16) {
        self.flags |= AREA;
        if !self.areas.contains(&z) {
            self.areas.insert(0, z);
            self.areas.truncate(16);
        }
        self.last_area = z as i16;
    }

    /// FUN_10107ea0.
    pub fn clear_status(&mut self) {
        self.flags &= !STATUS;
        self.status = 0;
    }

    /// FUN_10107e70: the status only while none is set.
    pub fn set_status_if_none(&mut self, v: u32) {
        if self.status == 0 {
            self.flags |= STATUS;
            self.status = v;
        }
    }

    /// FUN_10105fa0: the type is in the sort list.
    pub fn sorted(&self, t: u8) -> bool {
        self.sorts.iter().any(|s| s & 0x1f == t & 0x1f)
    }
}

/// The decoded entry (FUN_101061d0's 0x48-byte record); `present` has bit `1 << type` per field sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub present: u32,
    pub name: String,
    /// Type 8 (rec+0x18): the id 0x06E / 0x11C take.
    pub id: u32,
    pub zone: u16,
    pub nation: u8,
    pub race: u8,
    pub rank: u8,
    pub job: (u8, u8),
    pub level: (u8, u8),
    pub master_level: u8,
    /// Rec+0x30: the high half from type 0x16, the low half from type 6 (flags table in MODLOG "Search").
    pub flags: u32,
    pub comment: u32,
    pub languages: u16,
    /// Type 0xd: the ranks (1 holder, 2 pearlsack, 3 pearl) and ids of the person's three linkshells.
    pub ls_ranks: [u8; 3],
    pub ls_ids: [u32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Reply {
    /// Reply +0x0e: every match, not only those sent (LSB sends 40 at most).
    pub total: u16,
    pub records: Vec<Record>,
}

/// MSB-first bit writer (FUN_10104200; LSB reads the same order).
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    n: usize,
}

impl Bits {
    fn put(&mut self, v: u32, width: usize) {
        for i in (0..width).rev() {
            if self.n % 8 == 0 {
                self.bytes.push(0);
            }
            if v >> i & 1 != 0 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.n % 8);
            }
            self.n += 1;
        }
    }
}

/// FUN_101041e0 -> FUN_10104200: the condition block (u8 byte count, then the bits) and its wire type.
pub fn condition(c: &Cond) -> (u8, Vec<u8>) {
    if let Some(ids) = c.group {
        let mut p: Vec<u8> = ids.iter().flat_map(|v| v.to_le_bytes()).collect();
        p.extend([0; 4]);
        return (2, p);
    }
    let mut b = Bits::default();
    // The typed sorts, then name and each set field's type not listed yet.
    let mut sorts = c.sorts.clone();
    for (flag, t) in [(0, 0), (LEVEL, 4), (MLEVEL, 7), (JOB, 3), (NATION, 2), (RACE, 5), (AREA, 1), (STATUS, 6), (RANK, 0x10)] {
        if (flag == 0 || c.flags & flag != 0) && !sorts.iter().any(|s| s & 0x1f == t) {
            sorts.push(t);
        }
    }
    // Each entry: 5-bit type, descending, present, then the value.
    for s in sorts {
        let (t, desc) = (s & 0x1f, (s >> 7) as u32);
        let head = |b: &mut Bits, present: bool| {
            b.put(t as u32, 5);
            b.put(desc, 1);
            b.put(present as u32, 1);
        };
        let has = |f: u32| c.flags & f != 0;
        match t {
            0 if has(NAME) && !c.name.is_empty() => {
                head(&mut b, true);
                let n: Vec<u8> = c.name.bytes().take(15).collect();
                b.put(0, 1);
                b.put(n.len() as u32, 4);
                for ch in n {
                    b.put(ch as u32 & 0x7f, 7);
                }
            }
            // One entry per area, then an empty one unless all 16 went.
            1 if has(AREA) && !c.areas.is_empty() => {
                for z in c.areas.iter().take(16) {
                    head(&mut b, true);
                    b.put(*z as u32 & 0x3ff, 10);
                }
                if c.areas.len() < 16 {
                    head(&mut b, false);
                }
            }
            2 if has(NATION) => {
                head(&mut b, true);
                b.put(c.nation as u32, 2);
            }
            3 if has(JOB) => {
                head(&mut b, true);
                b.put(c.job as u32, 5);
            }
            4 if has(LEVEL) => {
                head(&mut b, true);
                b.put(c.level.0 as u32, 8);
                b.put(c.level.1 as u32, 8);
            }
            5 if has(RACE) => {
                head(&mut b, true);
                b.put(c.race as u32, 4);
            }
            6 if has(STATUS) && c.status != 0 => {
                head(&mut b, true);
                b.put(c.status & 0xffff, 16);
            }
            7 if has(MLEVEL) => {
                head(&mut b, true);
                b.put(c.level.0 as u32, 8);
                b.put(c.level.1 as u32, 8);
            }
            0x10 if has(RANK) => {
                head(&mut b, true);
                b.put(c.rank.0 as u32, 8);
                b.put(c.rank.1 as u32, 8);
            }
            // Sort only.
            _ => head(&mut b, false),
        }
    }
    // Then without descending / present bits.
    if c.flags & LS1 != 0 {
        b.put(0b01011, 5);
        b.put(c.ls[0], 32);
    }
    if c.flags & LS2 != 0 {
        b.put(0b10011, 5);
        b.put(c.ls[1], 32);
    }
    if c.flags & FRIEND != 0 {
        b.put(0b01100, 5);
    }
    if c.flags & COMMENT != 0 {
        b.put(0b10001, 5);
        b.put(c.comment, 32);
    }
    if c.status != 0 {
        b.put(0b10110, 5);
        b.put(c.status, 32);
    }
    let mut out = vec![b.bytes.len() as u8];
    out.extend(b.bytes);
    (if c.area_given { 3 } else { 0 }, out)
}

/// The request packet (1.2 of the spec): header, payload, session word, MD5, salt; Blowfish over [8, P + 8).
pub fn request(wire: u8, payload: &[u8], session: u32, salt: u32) -> Vec<u8> {
    let dlen = 0x10 + payload.len();
    let p = ((dlen + 0xb) / 0x40 + 1) * 0x40;
    let mut buf = vec![0u8; p + 0xc];
    let total = buf.len() as u32;
    buf[0..4].copy_from_slice(&total.to_le_bytes());
    buf[4..8].copy_from_slice(b"IXFF");
    buf[8..10].copy_from_slice(&(dlen as u16).to_le_bytes());
    (buf[10], buf[11]) = (0x80, wire);
    buf[0x10..dlen].copy_from_slice(payload);
    buf[p - 0xc..p - 8].copy_from_slice(&session.to_le_bytes());
    let md5: [u8; 16] = Md5::digest(&buf[8..p - 8]).into();
    buf[p - 8..p + 8].copy_from_slice(&md5);
    buf[p + 8..p + 12].copy_from_slice(&salt.to_le_bytes());
    let mut key = FIXED.to_vec();
    key.extend(salt.to_le_bytes());
    let key: [u8; 16] = Md5::digest(&key).into();
    Blowfish::new(&key).encrypt(&mut buf[8..p + 8]);
    buf
}

/// FUN_100e2860: decrypts one reply packet in place and checks its size and MD5; (data size, flags, type, total).
fn open(r: &mut [u8], session: u32) -> Result<(usize, u8, u8, u16)> {
    let l = r.len();
    if l < 0x2c {
        bail!("search reply too short ({l})");
    }
    let mut key = FIXED.to_vec();
    key.extend_from_slice(&r[l - 4..]);
    key.extend(session.to_le_bytes());
    let key: [u8; 16] = Md5::digest(&key).into();
    let n = (l - 0xc) & !7;
    Blowfish::new(&key).decrypt(&mut r[8..8 + n]);
    let d = u16::from_le_bytes([r[8], r[9]]) as usize;
    if d >= l || d + 0x14 > l {
        bail!("search reply size {d} of {l}");
    }
    let md5: [u8; 16] = Md5::digest(&r[8..l - 0x14]).into();
    if md5 != r[l - 0x14..l - 4] {
        bail!("search reply MD5 mismatch");
    }
    Ok((d, r[10], r[11], u16::from_le_bytes([r[0x0e], r[0x0f]])))
}

/// FUN_101061d0: one entry's fields, `[5-bit type][value]`, until a repeated type or the bits run out.
pub fn decode(entry: &[u8]) -> Record {
    let total = entry.len() * 8;
    let mut pos = 0;
    let mut get = |w: usize| -> Option<u32> {
        if pos + w > total {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..w {
            v = v << 1 | (entry[pos / 8] >> (7 - pos % 8) & 1) as u32;
            pos += 1;
        }
        Some(v)
    };
    let mut r = Record::default();
    let mut low_flags = None;
    while let Some(t) = get(5) {
        if r.present & 1 << t != 0 {
            break;
        }
        let ok = (|| -> Option<()> {
            match t {
                0 => {
                    let n = get(4)?;
                    r.name = (0..n).map(|_| get(7).map(|c| c as u8 as char)).collect::<Option<String>>()?;
                }
                1 => r.zone = get(10)? as u16,
                2 => r.nation = get(2)? as u8,
                3 => r.job = (get(5)? as u8, get(5)? as u8),
                4 => r.level = (get(8)? as u8, get(8)? as u8),
                5 => r.race = get(4)? as u8,
                6 => low_flags = Some(get(16)?),
                7 => r.master_level = get(8)? as u8,
                8 => r.id = get(20)?,
                0xd => {
                    for k in 0..3 {
                        r.ls_ranks[k] = get(8)? as u8;
                    }
                    for k in 0..3 {
                        r.ls_ids[k] = get(32)?;
                    }
                }
                0xe => {
                    get(32)?;
                }
                0x10 => r.rank = get(8)? as u8,
                0x11 => r.comment = get(32)?,
                0x16 => r.flags = get(32)?,
                0x17 => r.languages = get(16)? as u16,
                _ => return None,
            }
            Some(())
        })();
        if ok.is_none() {
            break;
        }
        r.present |= 1 << t;
    }
    if let Some(f) = low_flags {
        r.flags = r.flags & !0xffff | f;
    }
    r
}

/// One query, blocking: connect, send, read packets until the last-packet flag (reply +0x0a bit 7), close.
pub fn query(addr: (Ipv4Addr, u16), cond: &Cond) -> Result<Reply> {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let (session, salt) = (seed as u32 | 1, (seed >> 32) as u32 ^ 0x5a5a_5a5a);
    let (wire, payload) = condition(cond);
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(addr), Duration::from_secs(10))?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.write_all(&request(wire, &payload, session, salt))?;
    let mut reply = Reply::default();
    loop {
        let mut head = [0u8; 8];
        s.read_exact(&mut head)?;
        let l = u32::from_le_bytes(head[..4].try_into().unwrap()) as usize;
        if !(0x18..0x56d).contains(&l) {
            bail!("search reply length {l}");
        }
        let mut r = head.to_vec();
        r.resize(l, 0);
        s.read_exact(&mut r[8..])?;
        let (d, flags, kind, total) = open(&mut r, session)?;
        if kind & 0x1f != 0 && kind & 0x1f != 2 {
            return Err(anyhow!("search reply type {kind:#x}"));
        }
        reply.total = total;
        let mut o = 0x18;
        while o < d && r[o] != 0 && o + (r[o] as usize) < d {
            let n = r[o] as usize;
            reply.records.push(decode(&r[o + 1..o + 1 + n]));
            o += 1 + n;
        }
        if flags & 0x80 != 0 {
            return Ok(reply);
        }
    }
}

/// [`query`] on its own thread; the result comes through the channel.
pub fn spawn(addr: (Ipv4Addr, u16), cond: Cond) -> mpsc::Receiver<Result<Reply>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = query(addr, &cond);
        if let Err(e) = &r {
            log::warn!("search {cond:?}: {e}");
        }
        let _ = tx.send(r);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn conditions() {
        assert_eq!(condition(&Cond::zone(230)), (3, vec![4, 0x00, 0x14, 0xe6, 0x08]));
        assert_eq!(condition(&Cond::zone(101)), (3, vec![4, 0x00, 0x14, 0x65, 0x08]));
        assert_eq!(condition(&Cond::all()), (0, vec![2, 0x00, 0x10]));
        // [00000 0 1][0][0011][1000001 1100010 1100011]: "Abc".
        let (w, p) = condition(&Cond::name("Abc"));
        assert_eq!((w, p), (0, vec![5, 0x02, 0x38, 0x38, 0xb1, 0x80]));
    }

    /// Type 0xd: three 8-bit ranks, then three 32-bit linkshell ids (the group list's linkshell members).
    #[test]
    fn linkshell_entry() {
        let mut b = Bits::default();
        b.put(0, 5);
        b.put(2, 4);
        b.put(b'A' as u32, 7);
        b.put(b'b' as u32, 7);
        b.put(0xd, 5);
        for v in [2, 0, 3] {
            b.put(v, 8);
        }
        for v in [0x1234_5678, 0, 0x9abc] {
            b.put(v, 32);
        }
        let r = decode(&b.bytes);
        assert_eq!((r.name.as_str(), r.ls_ranks, r.ls_ids), ("Ab", [2, 0, 3], [0x1234_5678, 0, 0x9abc]));
    }

    /// Against the local xi_search: everyone, then the first of them by name (`cargo test -- --ignored live`).
    #[test]
    #[ignore]
    fn live_lookup() {
        let addr = (Ipv4Addr::LOCALHOST, 54002);
        let all = query(addr, &Cond::all()).unwrap();
        println!("all: {} of {}: {:?}", all.records.len(), all.total, all.records.iter().map(|r| (&r.name, r.zone, r.id)).collect::<Vec<_>>());
        let first = &all.records.first().expect("nobody online").name;
        let one = query(addr, &Cond::name(first.clone())).unwrap();
        println!("{first}: {:?}", one.records);
        assert!(one.records.iter().any(|r| &r.name == first));
        let zone = query(addr, &Cond::zone(all.records[0].zone)).unwrap();
        assert!(zone.records.iter().all(|r| r.zone == all.records[0].zone));
    }

    #[test]
    fn spec_vector() {
        let (wire, payload) = condition(&Cond::zone(101));
        let req = request(wire, &payload, 0x1122_3344, 0x5566_7788);
        let want = hex("4c0000004958464669148a59d7cfb8e92a38ab9c9baa18dcb9d7f22348c37029b9d7f22348c37029b9d7f22348c37029daf440daa68666cc9160a6313f2cd3c4769d706a41c5c56b88776655");
        assert_eq!(req, want);
        let mut r = hex("96000000495846464948e905e24b64ca5e6b543e28494878e1ba66f822835a7840cd4907f300e8314bfcb70d85f025e31a27c452db4ec0ac21ad17175e0e812f0b802580f8da0643000c2749c30ac1d2f7cbcc47e1d5dc20476540682ff59bb85302d05f774407d9c22317a1b74479875de459f288462ef76779e08fc7a39c17efa11f8f2c98abe9b54bd4343d6745bfc2d688776655");
        let (d, flags, kind, total) = open(&mut r, 0x1122_3344).unwrap();
        assert_eq!((d, flags, kind, total), (130, 0x80, 0x80, 3));
        let mut names = Vec::new();
        let mut o = 0x18;
        while o < d && r[o] != 0 && o + (r[o] as usize) < d {
            let rec = decode(&r[o + 1..o + 1 + r[o] as usize]);
            names.push((rec.name.clone(), rec.zone, rec.id, rec.flags & 0xffff));
            o += 1 + r[o] as usize;
        }
        assert_eq!(names, [("Korrin".into(), 101, 15, 0x8000), ("Mirelle".into(), 101, 17, 0x8000), ("Rokuro".into(), 101, 19, 0x8000)]);
    }
}
