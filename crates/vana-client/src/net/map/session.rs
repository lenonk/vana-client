//! UDP session with a zone (map) server.
//!
//! Datagram: 28-byte header (u16 our sequence, u16 last server sequence seen, u32 time at 8),
//! then sub-packets. Except for the first 0x00A, sub-packets are compressed, followed by the
//! compressed size in bits (u32) and an MD5 of both, and everything after the header is
//! Blowfish-encrypted in whole 8-byte blocks.

use super::blowfish::Blowfish;
use super::compress::Codec;
use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HEADER: usize = 28;

#[derive(Debug, Clone)]
pub struct SubPacket {
    pub id: u16,
    pub sync: u16,
    /// Whole sub-packet, including its 4-byte header.
    pub data: Vec<u8>,
}

impl SubPacket {
    pub fn u8(&self, o: usize) -> u8 {
        self.data.get(o).copied().unwrap_or(0)
    }
    pub fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.u8(o), self.u8(o + 1)])
    }
    pub fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.u8(o), self.u8(o + 1), self.u8(o + 2), self.u8(o + 3)])
    }
    pub fn f32(&self, o: usize) -> f32 {
        f32::from_bits(self.u32(o))
    }
    pub fn str(&self, o: usize, len: usize) -> String {
        let end = (o + len).min(self.data.len());
        let bytes = &self.data[o.min(end)..end];
        let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        // The game's text is Shift-JIS (with its own glyph pairs and auto-translate blocks).
        crate::dat::font::decode_sjis(bytes)
    }
}

/// The largest sub-packet: its header's size field is 7 bits of 4-byte words (127 * 4).
pub const MAX_SUB_LEN: usize = 0x7f * 4;

/// Builds one sub-packet; the body is padded to the 4-byte unit the size field counts in.
pub fn build(id: u16, body: &[u8]) -> Vec<u8> {
    let len = (4 + body.len()).div_ceil(4) * 4;
    let mut p = vec![0u8; len];
    p[..2].copy_from_slice(&(id & 0x1FF | ((len / 4) as u16) << 9).to_le_bytes());
    p[4..4 + body.len()].copy_from_slice(body);
    p
}

fn now() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

pub struct MapSession {
    sock: UdpSocket,
    cipher: Blowfish,
    /// The key the cipher was made from, for 0x00A's ticket.
    key: [u8; 16],
    codec: Codec,
    seq: u16,
    server_seq: u16,
    /// Datagram bytes sent / received on this session, for the Network window.
    pub bytes_out: u64,
    pub bytes_in: u64,
    /// Sub-packets not yet acked, stamped with the sequence of their first send (FUN_100fac80 resends them in
    /// every datagram until the server's header u16 @2 reaches it), and that ack.
    unacked: Vec<(u16, Vec<u8>)>,
    server_ack: u16,
    /// Header sequence of the last datagram handled; none before the first.
    last_in: Option<u16>,
    /// Datagrams logged so far each way (the first few of a session are logged).
    logged: (u32, u32),
    /// From the zone-in (s2c 0x00A u32 @0x94 / @0x98): every later datagram's header carries it (FUN_100fa9f0).
    cert: Option<[u32; 2]>,
    /// The header clock (DAT_10492e10): seconds and ms at an instant, set by the zone-in and every 30th datagram.
    clock: Option<(u32, u32, std::time::Instant)>,
}

const LOG_DATAGRAMS: u32 = 40;

/// The sub-packets' ids and syncs, for the log.
fn sub_ids(plain: &[u8]) -> String {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= plain.len() {
        let word = u16::from_le_bytes([plain[at], plain[at + 1]]);
        let len = ((word >> 9) as usize) * 4;
        if len == 0 {
            break;
        }
        out.push(format!("{:03X}/{}", word & 0x1ff, u16::from_le_bytes([plain[at + 2], plain[at + 3]])));
        at += len;
    }
    out.join(" ")
}

/// FUN_100fac80's raw payload limit.
const MAX_PAYLOAD: usize = 4000;

impl MapSession {
    pub fn connect(server: SocketAddr, key: &[u8; 20], codec: Codec) -> Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.connect(server)?;
        Ok(Self { sock, cipher: Blowfish::for_session_key(key), key: super::blowfish::session_key(key), codec, seq: 0, server_seq: 0, bytes_out: 0, bytes_in: 0, unacked: Vec::new(), server_ack: 0, last_in: None, logged: (0, 0), cert: None, clock: None })
    }

    /// @0 our sequence, @2 the last server datagram handled, @4 / @8 the clock's ms and seconds; after the zone-in
    /// @0x0C = 1 and the zone-in's two words @0x10 / @0x14 (FUN_100fa9f0, FUN_100fdbf0).
    fn header(&mut self) -> Vec<u8> {
        self.seq = self.seq.wrapping_add(1);
        let mut h = vec![0u8; HEADER];
        h[..2].copy_from_slice(&self.seq.to_le_bytes());
        h[2..4].copy_from_slice(&self.server_seq.to_le_bytes());
        let (secs, ms) = match self.clock {
            Some((s, m, at)) => {
                let t = m as u64 + at.elapsed().as_millis() as u64;
                (s.wrapping_add((t / 1000) as u32), (t % 1000) as u32)
            }
            None => (now(), 0),
        };
        h[4..8].copy_from_slice(&ms.to_le_bytes());
        h[8..12].copy_from_slice(&secs.to_le_bytes());
        h
    }

    /// The zone-in (s2c 0x00A, LAB_100facf0): its two words for every later header, its clock (@0x38 seconds, @0x34
    /// ms, FUN_100dc150).
    pub fn zoned_in(&mut self, p: &SubPacket) {
        self.cert = Some([p.u32(0x94), p.u32(0x98)]);
        self.clock = Some((p.u32(0x38), p.u32(0x34), std::time::Instant::now()));
        log::info!("map: zone-in, header words {:08x} {:08x}", p.u32(0x94), p.u32(0x98));
    }

    fn stamp(&self, packets: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for p in packets {
            let mut p = p.clone();
            p[2..4].copy_from_slice(&self.seq.to_le_bytes());
            body.extend_from_slice(&p);
        }
        body
    }

    /// The opening 0x00A goes out unencrypted and uncompressed; `attempt` @6 is DAT_104df920: 0 at first, then 2, or 3
    /// once a datagram was read (FUN_100fa940).
    pub fn send_login(&mut self, char_id: u32, char_name: &str, account: &str, attempt: u16) -> Result<()> {
        let mut body = vec![0u8; 88];
        body[2..4].copy_from_slice(&attempt.to_le_bytes());
        body[8..12].copy_from_slice(&char_id.to_le_bytes());
        let put = |b: &mut [u8], at: usize, s: &str| {
            let s = &s.as_bytes()[..s.len().min(14)];
            b[at..at + s.len()].copy_from_slice(s);
        };
        put(&mut body, 30, char_name);
        put(&mut body, 45, account);
        // The ticket @0x40: MD5(the account's 15 bytes ‖ the Blowfish key) (FUN_100fba40 -> FUN_100dd800).
        let mut h = Md5::new();
        h.update(&body[45..60]);
        h.update(self.key);
        body[60..76].copy_from_slice(&h.finalize());
        body[80..84].copy_from_slice(b"WIN\0");
        // The language (FFXi.dll's map of polcore 0x1A5: 0 ja, 1 en, 2 de, 3 fr), then SYS\info02.bin's
        // {01 01} after a normal start (FUN_102532c0).
        body[84..86].copy_from_slice(&1u16.to_le_bytes());
        body[86..88].copy_from_slice(&0x0101u16.to_le_bytes());
        // LoginPacketCheck: byte sum of everything from MyIP (packet offset 8) on.
        body[0] = body[4..].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let packet = build(0x00A, &body);

        let mut dgram = self.header();
        dgram.extend_from_slice(&self.stamp(&[packet]));
        log::info!("map out (login): header {} [{}]", hex::encode(&dgram[..HEADER]), sub_ids(&dgram[HEADER..]));
        let digest = Md5::digest(&dgram[HEADER..]);
        dgram.extend_from_slice(&digest);
        self.sock.send(&dgram)?;
        Ok(())
    }

    /// One datagram: the new sub-packets plus every earlier one the server has not acked; a new 0x015 replaces
    /// a queued one (retail rewrites its single 0x015 slot).
    pub fn send(&mut self, packets: &[Vec<u8>]) -> Result<()> {
        let mut dgram = self.header();
        let (seq, ack) = (self.seq, self.server_ack);
        self.unacked.retain(|(s, _)| ack.wrapping_sub(*s) >= 0x8000);
        let id = |p: &[u8]| u16::from_le_bytes([p[0], p[1]]) & 0x1ff;
        if packets.iter().any(|p| id(p) == 0x015) {
            self.unacked.retain(|(_, p)| id(p) != 0x015);
        }
        for p in packets {
            let mut p = p.clone();
            p[2..4].copy_from_slice(&seq.to_le_bytes());
            self.unacked.push((seq, p));
        }
        let mut raw = Vec::new();
        for (_, p) in &self.unacked {
            if raw.len() + p.len() > MAX_PAYLOAD {
                break;
            }
            raw.extend_from_slice(p);
        }
        if let Some([a, b]) = self.cert {
            dgram[0x0C] = 1;
            dgram[0x10..0x14].copy_from_slice(&a.to_le_bytes());
            dgram[0x14..0x18].copy_from_slice(&b.to_le_bytes());
        }
        if self.logged.1 < LOG_DATAGRAMS {
            self.logged.1 += 1;
            log::info!("map out: header {} [{}]", hex::encode(&dgram[..HEADER]), sub_ids(&raw));
        }
        let (mut body, bits) = self.codec.compress(&raw);
        body.extend_from_slice(&bits.to_le_bytes());
        let digest = Md5::digest(&body);
        body.extend_from_slice(&digest);
        self.cipher.encrypt(&mut body);
        dgram.extend_from_slice(&body);
        self.sock.send(&dgram)?;
        self.bytes_out += dgram.len() as u64;
        Ok(())
    }

    /// Waits for one datagram and unpacks its sub-packets.
    pub fn recv(&mut self, timeout: Duration) -> Result<Option<Vec<SubPacket>>> {
        self.sock.set_read_timeout(Some(timeout))?;
        let mut buf = vec![0u8; 4096];
        let n = match self.sock.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        // LSB sends an empty datagram when nothing is queued (send_parse sets the size to 0). It carries nothing, so
        // it counts as no answer: dropped, and the login keeps retrying *(inf: retail drops it)*.
        if n < HEADER + 20 {
            log::debug!("dropped a {n}-byte datagram");
            return Ok(None);
        }
        self.bytes_in += n as u64;
        let mut dgram = buf[..n].to_vec();
        self.cipher.decrypt(&mut dgram[HEADER..]);
        let (data, digest) = dgram[HEADER..].split_at(n - HEADER - 16);
        if Md5::digest(data).as_slice() != digest {
            bail!("datagram failed its checksum (wrong session key?)");
        }
        let bits = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap());
        let plain = self.codec.decompress(&data[..data.len() - 4], bits).context("decompressing datagram")?;
        // FUN_100f76b0: a datagram not newer than the last one handled is dropped (counted), and so is a sub-packet
        // whose sync is not newer; only an accepted datagram moves the stored sequence and ack.
        let seq = u16::from_le_bytes([dgram[0], dgram[1]]);
        if self.logged.0 < LOG_DATAGRAMS {
            self.logged.0 += 1;
            log::info!("map in: header {} [{}]", hex::encode(&dgram[..HEADER]), sub_ids(&plain));
        }
        let newer = |s: u16| self.last_in.is_none_or(|last| last.wrapping_sub(s) >= 0x8000);
        if !newer(seq) {
            // LSB resends its last datagram when our ack lags (map_networking.cpp parse).
            log::debug!("dropped resent datagram {seq}");
            return Ok(Some(Vec::new()));
        }

        let mut out = Vec::new();
        let mut at = 0;
        while at + 4 <= plain.len() {
            let word = u16::from_le_bytes([plain[at], plain[at + 1]]);
            let len = ((word >> 9) as usize) * 4;
            if len == 0 || at + len > plain.len() {
                break;
            }
            let sync = u16::from_le_bytes([plain[at + 2], plain[at + 3]]);
            if newer(sync) {
                out.push(SubPacket { id: word & 0x1FF, sync, data: plain[at..at + len].to_vec() });
            }
            at += len;
        }
        self.server_seq = seq;
        self.server_ack = u16::from_le_bytes([dgram[2], dgram[3]]);
        self.last_in = Some(seq);
        // Every 30th datagram re-syncs the clock to the server's header (FUN_100fbd10; ours without the drift limit).
        if seq % 30 == 0 && self.clock.is_some() {
            let at = |o: usize| u32::from_le_bytes(dgram[o..o + 4].try_into().unwrap());
            self.clock = Some((at(8), at(4), std::time::Instant::now()));
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set FFXI_MAIN_DLL to run: a resent datagram (same header sequence) and an older one carry nothing.
    #[test]
    fn resent_datagram_dropped() {
        let Ok(dll) = std::env::var("FFXI_MAIN_DLL") else { return };
        let codec = Codec::from_ffximain(&std::fs::read(dll).unwrap()).unwrap();
        let key = [7u8; 20];
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut s = MapSession::connect(server.local_addr().unwrap(), &key, codec.clone()).unwrap();
        let cipher = Blowfish::for_session_key(&key);
        let dgram = |seq: u16| {
            let mut sub = build(0x029, &[0u8; 24]);
            sub[2..4].copy_from_slice(&seq.to_le_bytes());
            let (mut body, bits) = codec.compress(&sub);
            body.extend_from_slice(&bits.to_le_bytes());
            let digest = Md5::digest(&body);
            body.extend_from_slice(&digest);
            cipher.encrypt(&mut body);
            let mut d = vec![0u8; HEADER];
            d[..2].copy_from_slice(&seq.to_le_bytes());
            d.extend_from_slice(&body);
            d
        };
        let to = s.sock.local_addr().unwrap();
        for seq in [5, 5, 4, 6] {
            server.send_to(&dgram(seq), to).unwrap();
        }
        let got: Vec<usize> = (0..4).map(|_| s.recv(Duration::from_secs(1)).unwrap().unwrap().len()).collect();
        assert_eq!(got, [1, 0, 0, 1]);
        assert_eq!(s.server_seq, 6);
    }
}
