//! PlayOnline's profile service, POLPRO (pp%03d.pol.com:51220), as Viewer 1.18.15e speaks it: the member login and the
//! session FFXiMain hands the lobby. Addresses are polcore 73b1864b (1.18.15e); MODLOG "PlayOnline sign-in".

use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

pub const PORT: u16 = 51220;

/// The host for the routing struct's host byte (0x1001e8d0): `pp%03d.pol.com`, index = byte & 0x7f.
pub fn host(host_byte: u8) -> String {
    format!("pp{:03}.pol.com", host_byte & 0x7f)
}

/// polcore's dword sum (0x10063df0): little-endian u32s, then the tail bytes shifted in from the top.
pub fn checksum(d: &[u8]) -> u32 {
    let whole = d.len() & !3;
    let mut s = d[..whole].as_chunks::<4>().0.iter().fold(0u32, |s, w| s.wrapping_add(u32::from_le_bytes(*w)));
    let mut t = 0u32;
    for &b in &d[whole..] {
        t = (t >> 8) | (b as u32) << 24;
    }
    s = s.wrapping_add(t);
    s
}

/// The connect handshake (0x1001f390), sent plain: u16 1, then addr20's port and address (host order) as the chat
/// greeting gave them. Bytes 0x0c..0x17 are not written: the slot's header buffer still holds the previous request
/// header's enciphered bytes there (zeros at first).
pub fn handshake(addr20: &[u8; 20], residue: &[u8; 12]) -> [u8; 0x28] {
    let mut r = [0u8; 0x28];
    r[4..6].copy_from_slice(&1u16.to_le_bytes());
    r[6..12].copy_from_slice(&addr20[2..8]);
    r[12..24].copy_from_slice(residue);
    r
}

/// The request header (0x1001f5e0): 2, category, opcode, the body length (checksum included), and
/// MD5(POL ID's 8 characters ‖ POL password ‖ the handshake's token).
pub fn request_header(
    category: u8,
    opcode: u8,
    body_len: u32,
    pol_id: &str,
    pol_password: &str,
    token: [u8; 4],
) -> [u8; 0x28] {
    let mut h = [0u8; 0x28];
    h[..3].copy_from_slice(&[2, category, opcode]);
    h[4..8].copy_from_slice(&body_len.to_le_bytes());
    let mut m = Md5::new();
    m.update(pol_id.as_bytes());
    m.update(&pol_password.as_bytes()[..pol_password.len().min(15)]);
    m.update(token);
    h[0x18..].copy_from_slice(&m.finalize());
    h
}

/// A body: the payload and its checksum.
pub fn body(payload: &[u8]) -> Vec<u8> {
    let mut b = payload.to_vec();
    b.extend_from_slice(&checksum(payload).to_le_bytes());
    b
}

fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    digest(&SHA1_FOR_LEGACY_USE_ONLY, &parts.concat()).as_ref().try_into().unwrap()
}

/// The Square Enix password as the Viewer keeps it (app 0x1019ab29): SHA1(hex(SHA1(password)) ‖ "playonline"); the
/// low byte of each character.
pub fn secret20(se_password: &str) -> [u8; 20] {
    let low: Vec<u8> = se_password.chars().map(|c| c as u32 as u8).collect();
    sha1(&[hex::encode(sha1(&[&low])).as_bytes(), b"playonline"])
}

/// The member login's 0x3c-byte payload (4/7, 0x1001e760): mode 1 (ID and password) or 2 (with a one-time password),
/// the Square Enix ID (16 at most), the OTP @0x12, and SHA1(hex(secret20) ‖ "%u" of the minute) @0x20. `now` is
/// the chat greeting's clock plus the seconds since.
pub fn member_login(se_id: &str, secret20: &[u8; 20], otp: Option<&str>, now: u32) -> [u8; 0x3c] {
    let mut b = [0u8; 0x3c];
    b[0] = if otp.is_some() { 2 } else { 1 };
    let id = se_id.as_bytes();
    b[1..1 + id.len().min(16)].copy_from_slice(&id[..id.len().min(16)]);
    if let Some(otp) = otp {
        let o = otp.as_bytes();
        b[0x12..0x12 + o.len().min(6)].copy_from_slice(&o[..o.len().min(6)]);
    }
    let minute = (now - now % 60).to_string();
    b[0x20..0x34].copy_from_slice(&sha1(&[hex::encode(secret20).as_bytes(), minute.as_bytes()]));
    b
}

/// The member's status as polcore keeps it: LoadMyStatus's, then each ChangeMyStatus that went through. -1 is
/// "none" for the content and the character (service).
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub content: i32,
    pub service: i32,
    pub flag: i32,
    pub presence: i32,
    pub p5: i32,
    pub activity: i32,
    /// A ChangeMyStatus has gone through (0x100ac548).
    pub changed: bool,
}

/// ChangeMyStatus's 0x24-byte payload (4/5, 0x1001d7e0) and the status it sets. Arguments as polcore takes them,
/// [content, character, flag, presence, p5, activity]: -2 keeps (any negative for p5, outside 0..0x3ff for the
/// activity). `slots(content)` is the handle's 8 slot bytes (`character << 1 | 1`). Bytes 0x20..0x23 are stale buffer
/// bytes in retail; zeros here.
pub fn change_my_status(cur: &Status, a: [i32; 6], slots: impl Fn(i32) -> [u8; 8]) -> ([u8; 0x24], Status) {
    let keep = |v: i32, c: i32| if v == -2 { c } else { v };
    let new = Status {
        content: keep(a[0], cur.content),
        service: keep(a[1], cur.service),
        flag: keep(a[2], cur.flag),
        presence: keep(a[3], cur.presence),
        p5: if a[4] < 0 { cur.p5 } else { a[4] },
        activity: if (0..0x400).contains(&a[5]) { a[5] } else { cur.activity },
        changed: true,
    };
    let kept = a[..4].iter().all(|&v| v == -2) && a[4] < 0 && !(0..0x400).contains(&a[5]);
    let mut b = [0u8; 0x24];
    if new.content >= 0 {
        b[0x10] = new.content as u8;
        b[0x16] = new.presence as u8 + 1;
    }
    if let Some(slot) = slots(b[0x10] as i32).iter().position(|&s| s & 1 != 0 && (s >> 1 & 0x3f) as i32 == new.service)
    {
        b[0x11] = 1;
        b[0x12] = slot as u8;
    }
    b[0x13] = (new.flag == 0) as u8;
    b[0x14..0x16].copy_from_slice(&(new.activity as u16).to_le_bytes());
    b[0x17] = (new.p5 != 0) as u8;
    b[0x18] = (kept && cur.changed) as u8;
    b[0x19] = 1;
    (b, new)
}

/// A reply status as polcore's error code (0x1001f690, 0x1001f400).
pub fn error_code(status: u8) -> i32 {
    match -0x1450 - status as i32 {
        -0x1504 | -0x1544 => -0x104,
        -0x1534 => -0x105,
        -0x14c9 => -0x106,
        -0x1539..=-0x1536 => -0x200,
        c => c,
    }
}

/// What a refusal means, in our words (polcore's labels decoded from polerr.bin).
pub fn status_text(status: u8) -> &'static str {
    match status {
        0x6e => "PlayOnline refused sign-ins from this address.",
        0x6f => "The Square Enix ID, password or one-time password is incorrect.",
        0x70 => "This Square Enix account cannot sign in (closed, suspended or unpaid).",
        0x71 | 0x72 => "This account is bound to an Xbox profile.",
        0x7b | 0x7d => "That handle is not on this PlayOnline account.",
        0x82 | 0x83 => "PlayOnline refused this client's protocol version.",
        0x84 => "The PlayOnline password was changed during this session.",
        0x85 => "This account only accepts sign-ins from a registered address.",
        0xe2 => "PlayOnline could not certify the account.",
        0xea => "This PlayOnline ID is suspended.",
        0xb4 | 0xf4 => "The PlayOnline server timed out.",
        0xef => "PlayOnline rejected a message checksum.",
        _ => "PlayOnline refused the request.",
    }
}

/// The chat session's stream cipher as the profile service uses it (polcore 0x10063ef0): `reset` restarts the
/// keystream from the IV.
pub trait Stream {
    fn apply(&mut self, data: &mut [u8], reset: bool);
}

/// A refused transaction: the reply's status.
#[derive(Debug)]
pub struct Refused(pub u8);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} (POL-{})", status_text(self.0), -error_code(self.0))
    }
}

impl std::error::Error for Refused {}

/// The member as the profile service knows it, from the chat leg.
pub struct Member {
    pub pol_id: String,
    pub pol_password: String,
    /// The chat greeting's {u16 1, u16 port, u32 IPv4} (host order), the rest zero.
    pub addr20: [u8; 20],
    pub host_byte: u8,
    /// A test server in place of the routed host.
    pub fixed: Option<SocketAddr>,
    /// The last request header's enciphered bytes 0x0c..0x17, which the next handshake carries.
    pub residue: [u8; 12],
}

/// How a reply's body comes: none, a fixed size (checksum included), or a counted list of records (an 8-byte
/// preamble with the count in byte 0, then the records). polcore's list readers (0x1001e490, 0x10022260) go from the
/// last record straight to done: whatever follows is never read or checked.
pub enum ReplyBody {
    None,
    Fixed(usize),
    Records(usize),
}

/// One transaction on a separate connection (0x1001f0f0's states): the plain handshake and its token, the header
/// (keystream reset) and body (continued), the reply header (reset) and body (continued). `server` is the address a
/// reply cached, else the host is resolved.
pub fn transact<S: Stream>(
    cipher: &mut S,
    member: &mut Member,
    server: &mut Option<SocketAddr>,
    category: u8,
    opcode: u8,
    payload: Option<&[u8]>,
    reply: ReplyBody,
) -> Result<Vec<u8>> {
    let mut sock = match member.fixed.as_ref().or(server.as_ref()) {
        Some(a) => TcpStream::connect_timeout(a, Duration::from_secs(15)),
        None => TcpStream::connect((host(member.host_byte).as_str(), PORT)),
    }
    .context("connecting to the PlayOnline profile server")?;
    sock.set_read_timeout(Some(Duration::from_secs(30)))?;
    sock.write_all(&handshake(&member.addr20, &member.residue))?;
    let mut hs = [0u8; 0x18];
    sock.read_exact(&mut hs).context("the profile server closed the handshake")?;
    if hs[1] != 0 {
        bail!(Refused(hs[1]));
    }
    let token: [u8; 4] = hs[0x14..0x18].try_into().unwrap();
    let body = payload.map(body);
    let mut head = request_header(
        category,
        opcode,
        body.as_ref().map_or(0, |b| b.len() as u32),
        &member.pol_id,
        &member.pol_password,
        token,
    );
    cipher.apply(&mut head, true);
    member.residue.copy_from_slice(&head[12..24]);
    sock.write_all(&head)?;
    if let Some(mut b) = body {
        cipher.apply(&mut b, false);
        sock.write_all(&b)?;
    }
    let mut rh = [0u8; 0x18];
    sock.read_exact(&mut rh).context("the profile server closed the transaction")?;
    cipher.apply(&mut rh, true);
    log::info!("PlayOnline {category}/{opcode}: reply header {}", hex::encode(rh));
    if rh[1] != 0 {
        *server = None;
        bail!(Refused(rh[1]));
    }
    // The first reply naming a server is where later transactions go (0x1001f690).
    let cached = u32::from_le_bytes(rh[8..12].try_into().unwrap());
    if server.is_none() && cached != 0 && member.fixed.is_none() {
        *server = Some(SocketAddr::from((Ipv4Addr::from(cached), PORT)));
    }
    let mut read = |n: usize| -> Result<Vec<u8>> {
        let mut b = vec![0u8; n];
        sock.read_exact(&mut b).context("the profile server's reply was cut short")?;
        cipher.apply(&mut b, false);
        Ok(b)
    };
    let data = match reply {
        ReplyBody::None => return Ok(Vec::new()),
        ReplyBody::Fixed(n) => read(n)?,
        ReplyBody::Records(size) => {
            let mut d = read(8)?;
            let count = d[0] as usize;
            d.extend(read(count * size)?);
            return Ok(d);
        }
    };
    let (payload, sum) = data.split_at(data.len() - 4);
    if checksum(payload).to_le_bytes() != sum {
        // What else the server sent, to see the reply's real shape.
        let mut more = vec![0u8; 0x4000];
        sock.set_read_timeout(Some(Duration::from_secs(3)))?;
        let n = sock.read(&mut more).unwrap_or(0);
        log::warn!(
            "PlayOnline {category}/{opcode}: checksum {:08x}, sent {}; body {} ({} bytes), {n} more: {}",
            checksum(payload),
            hex::encode(sum),
            hex::encode(&data),
            data.len(),
            hex::encode(&more[..n])
        );
        bail!("a PlayOnline reply failed its checksum (POL-{})", 0x140f);
    }
    Ok(payload.to_vec())
}

/// polcore's *5-chain cipher (schedule 0x10007bc0, blocks 0x10007d10): the authCode's, and polerr.bin's.
struct C5 {
    words: [u64; 32],
    pos: u32,
    ck: u32,
}

impl C5 {
    fn new(key: [u8; 8]) -> Self {
        let k0 = u32::from_le_bytes(key[..4].try_into().unwrap());
        let k1 = u32::from_le_bytes(key[4..].try_into().unwrap());
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&k1.rotate_left(16).to_le_bytes());
        b[4..].copy_from_slice(&k0.rotate_left(8).to_le_bytes());
        b[0] = b[0].wrapping_add(0x45);
        for i in 1..8 {
            let t = b[i].wrapping_add(b[i - 1]).wrapping_sub(0x2c);
            b[i] = (b[i - 1] << 2) ^ t ^ 0x45;
        }
        let mut words = [0u64; 32];
        words[0] = u64::from_le_bytes(b);
        for i in 1..32 {
            words[i] = words[i - 1].wrapping_mul(5);
        }
        Self { words, pos: 0, ck: 0 }
    }

    fn block(&mut self, blk: &[u8; 8], out: &mut Vec<u8>) {
        self.ck = self.ck.wrapping_add(blk[0] as u32 + blk[4] as u32);
        let pos = self.pos;
        let w = self.words[(pos >> 3) as usize & 31];
        let lo = u32::from_le_bytes(blk[..4].try_into().unwrap()) as u64;
        let hi = u32::from_le_bytes(blk[4..].try_into().unwrap()) as u64;
        let mut v = (hi | lo << 32) ^ w;
        let mut c = pos as u64;
        for _ in 0..3 {
            c = c << 10 | pos as u64;
        }
        v ^= c.wrapping_add(0xa165_2347);
        v = v.wrapping_add(w);
        let sub = w.to_le_bytes().iter().fold(0u32, |s, &x| s + x as u32).wrapping_add(8 * 0x88);
        let mut chain = ((pos >> 3) ^ 0x45) as u8;
        for x in v.to_le_bytes() {
            chain ^= (x as u32 + sub) as u8;
            out.push(chain);
        }
        self.pos = pos.wrapping_add(8);
    }

    /// Whole blocks, a zero-padded last one, then the tag {u32 length, u32 checksum}.
    fn encrypt(key: [u8; 8], data: &[u8]) -> Vec<u8> {
        let mut c = Self::new(key);
        let mut out = Vec::with_capacity(data.len() + 16);
        for chunk in data.chunks(8) {
            let mut blk = [0u8; 8];
            blk[..chunk.len()].copy_from_slice(chunk);
            c.block(&blk, &mut out);
        }
        let mut tag = [0u8; 8];
        tag[..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
        tag[4..].copy_from_slice(&c.ck.to_le_bytes());
        c.block(&tag, &mut out);
        out
    }
}

/// The authCode's cipher key (0x10074318).
const AUTH_KEY: [u8; 8] = [0xcb, 0x83, 0x24, 0xb7, 0xba, 0x3e, 0x9e, 0x0a];

/// The 0x34-byte authCode (0x1001a2d0) from ChangeMyStatus's reply, the chat greeting's addr20 and clock, the chat
/// key and the routing host byte. FFXiMain sends it in a 64-byte field; the rest is its stack, zeros here.
pub fn auth_code(reply16: &[u8; 16], addr20: &[u8; 20], clock: u32, chat_key: &[u8; 8], host_byte: u8) -> [u8; 0x34] {
    let mut s = [0u8; 0x28];
    let mut m = Md5::new();
    m.update(reply16);
    m.update(clock.to_le_bytes());
    m.update(chat_key);
    s[0x18..].copy_from_slice(&m.finalize());
    s[4..0x18].copy_from_slice(addr20);
    s[0] = s[0x25] ^ s[0x1b];
    s[1] = host_byte;
    let mut a = [0u8; 0x34];
    a[4..].copy_from_slice(&C5::encrypt(AUTH_KEY, &s));
    for i in 5..0x34 {
        a[i] ^= a[i - 1];
    }
    a[0] = a[0xe] ^ host_byte;
    a[1] = a[0xc] ^ a[6];
    a[2] = a[0xa] ^ a[8];
    a[3] = a[0xd] ^ a[9];
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    // Vectors from polcore 73b1864b run under Wine.

    #[test]
    fn checksum_and_framing() {
        let data = h("808386898c8f9295989b9ea1a4a7aaadb0b3b6b9bcbfc2c5c8cbced1d4d7dadde0e3e6e9");
        assert_eq!(checksum(&data).to_le_bytes().to_vec(), h("30516c87"));
        let addr: [u8; 20] = h("010039300900000a000000000000000000000000").try_into().unwrap();
        assert_eq!(
            handshake(&addr, &[0xaa; 12]).to_vec(),
            h("00000000010039300900000aaaaaaaaaaaaaaaaaaaaaaaaa00000000000000000000000000000000")
        );
        let hd = request_header(4, 5, 0x28, "EFKAOYMJ", "hunter2", [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(hd.to_vec(), h("0204050028000000000000000000000000000000000000004d69ecd7efcce1cc96148c6d7aadd09f"));
    }

    #[test]
    fn member_login_body() {
        assert_eq!(secret20("hunter2").to_vec(), h("505b52b912143468cece9546d5118d9a20689222"));
        let s: [u8; 20] = [0x11; 20];
        let now = 0x6543_210f;
        let b = member_login("TESTMEMBER", &s, None, now);
        assert_eq!(
            b.to_vec(),
            h(
                "01544553544d454d42455200000000000000000000000000000000000000000099175d8367ac6b3a30588ff9f6f716697e7e5e6d0000000000000000"
            )[..0x3c]
        );
        let b = member_login("SIXTEENCHARSXYZ0", &s, Some("123456"), now);
        assert_eq!(
            b.to_vec(),
            h(
                "025349585445454e434841525358595a3000313233343536000000000000000099175d8367ac6b3a30588ff9f6f716697e7e5e6d0000000000000000"
            )[..0x3c]
        );
    }

    #[test]
    fn auth_code_matches_polcore() {
        let c = C5::encrypt(AUTH_KEY, &(0..0x28).collect::<Vec<u8>>());
        assert_eq!(
            c,
            h("41f9fdc5809ab6926b49ee2d4acccd2dcdb0cbfad809d59f1712f0d7d25596d7700405419bf3b2e8b8b15ec730836d17")
        );
        let a = auth_code(
            &h("112233445566778899aabbccddeeff01").try_into().unwrap(),
            &h("202122232425262728292a2b2c2d2e2f30313233").try_into().unwrap(),
            0x5f5e_0100,
            &h("0102030405060708").try_into().unwrap(),
            7,
        );
        assert_eq!(
            a.to_vec(),
            h(
                "3c7d2e21ad948588c390edb7f8b13b2e7044adf8d1994e940051c877c82d5bc8afebe654639dea5eb7a4b246574cb8d54f66a11c"
            )
        );
        let a = auth_code(
            &h("40454a4f54595e63686d72777c81868b").try_into().unwrap(),
            &h("010039300900000a000000000000000000000000").try_into().unwrap(),
            0x6543_210f,
            &h("3141592653589793").try_into().unwrap(),
            7,
        );
        assert_eq!(
            a.to_vec(),
            h(
                "6da82e084cb56f9ee487caa0c78f6a3902b6f569b0fb21e71305f93851955ad8988c94c807cf8f2ead3020b44cbea3275494baee"
            )
        );
    }

    /// polcore's bodies for FFXiMain's (-2,-1,0,-2,-1,1), then app.dll's (3,-2,1,2,0,-2), after a LoadMyStatus of
    /// content 3, service 5, presence 2, p5 1, activity 0x123 (profile run1815e.txt).
    #[test]
    fn change_my_status_bodies() {
        let cur = Status { content: 3, service: 5, flag: 1, presence: 2, p5: 1, activity: 0x123, changed: false };
        let (b, cur) = change_my_status(&cur, [-2, -1, 0, -2, -1, 1], |_| [0; 8]);
        assert_eq!(b[..0x20].to_vec(), h("0000000000000000000000000000000003000001010003010001000000000000"));
        let (b, _) = change_my_status(&cur, [3, -2, 1, 2, 0, -2], |_| [0; 8]);
        assert_eq!(b[..0x20].to_vec(), h("0000000000000000000000000000000003000000010003000001000000000000"));
        // A character in the handle's slot 2: 1 / 2 (0x11 / 0x12).
        let (b, _) = change_my_status(&cur, [3, 9, 0, -2, -1, 1], |_| [0, 0, 9 << 1 | 1, 0, 0, 0, 0, 0]);
        assert_eq!((b[0x11], b[0x12]), (1, 2));
    }

    #[test]
    fn status_codes() {
        assert_eq!(error_code(0x6e), -5310);
        assert_eq!(error_code(0xb4), -0x104);
        assert_eq!(error_code(0xe7), -0x200);
        assert_eq!(host(0x87), "pp007.pol.com");
    }
}
