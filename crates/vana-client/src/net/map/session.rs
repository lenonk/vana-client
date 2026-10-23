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
}

/// FUN_100fac80's raw payload limit.
const MAX_PAYLOAD: usize = 4000;

impl MapSession {
    pub fn connect(server: SocketAddr, key: &[u8; 20], codec: Codec) -> Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.connect(server)?;
        Ok(Self { sock, cipher: Blowfish::for_session_key(key), codec, seq: 0, server_seq: 0, bytes_out: 0, bytes_in: 0, unacked: Vec::new(), server_ack: 0 })
    }

    fn header(&mut self) -> Vec<u8> {
        self.seq = self.seq.wrapping_add(1);
        let mut h = vec![0u8; HEADER];
        h[..2].copy_from_slice(&self.seq.to_le_bytes());
        h[2..4].copy_from_slice(&self.server_seq.to_le_bytes());
        h[8..12].copy_from_slice(&now().to_le_bytes());
        h
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

    /// The opening 0x00A goes out unencrypted and uncompressed; `attempt` counts retries.
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
        body[80..84].copy_from_slice(b"WIN\0");
        body[84..86].copy_from_slice(&2u16.to_le_bytes()); // English
        // LoginPacketCheck: byte sum of everything from MyIP (packet offset 8) on.
        body[0] = body[4..].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let packet = build(0x00A, &body);

        let mut dgram = self.header();
        dgram.extend_from_slice(&self.stamp(&[packet]));
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
        self.server_seq = u16::from_le_bytes([dgram[0], dgram[1]]);
        self.server_ack = u16::from_le_bytes([dgram[2], dgram[3]]);

        let mut out = Vec::new();
        let mut at = 0;
        while at + 4 <= plain.len() {
            let word = u16::from_le_bytes([plain[at], plain[at + 1]]);
            let len = ((word >> 9) as usize) * 4;
            if len == 0 || at + len > plain.len() {
                break;
            }
            out.push(SubPacket {
                id: word & 0x1FF,
                sync: u16::from_le_bytes([plain[at + 2], plain[at + 3]]),
                data: plain[at..at + len].to_vec(),
            });
            at += len;
        }
        Ok(Some(out))
    }
}
