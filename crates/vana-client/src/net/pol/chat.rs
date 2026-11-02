//! PlayOnline's chat service (ci000.pol.com:51240, an IRC dialect): the RSA key agreement that gives the session its
//! cipher, the PlayOnline ID and password proof, and the profile host. Addresses polcore 73b1864b;
//! MODLOG "PlayOnline sign-in".

use super::cipher::PolBlowfish;
use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

pub const HOST: &str = "ci000.pol.com";
pub const PORT: u16 = 51240;

const B64: &[u8; 64] = b"TSG8IncW3HFKokOg79qzeCmZs2yBYEQVAUxR5rbwi4P@jMDLtpvad0f_J1hlN6uX";
const B32: &[u8; 32] = b"N43OVHBJ1Y2C0WSXED5QFILRZMUTAPGK";
const ID_ALPHABET: &[u8; 36] = b"EFKAOYMJVNGTDSWBQLPCIRHZXU6328401795";
/// The Viewer's area code (env.dat POL_UCS_AREA_KBN "01", the US Viewer).
const AREA: u8 = 1;

/// polcore's base64 (0x10007a60): MSB-first 6-bit groups; a short last group sends only the characters with data.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for g in data.chunks(3) {
        let mut v = [0u8; 4];
        v[1..1 + g.len()].copy_from_slice(g);
        let v = u32::from_be_bytes(v);
        for k in 0..=g.len() {
            out.push(B64[(v >> (18 - 6 * k) & 63) as usize] as char);
        }
    }
    out
}

/// 0x100078a0: `n` characters, a short last group filled with the zero character; other bytes decode as zero.
pub fn b64_decode(text: &[u8], n: usize) -> Vec<u8> {
    let mut t = text[..n.min(text.len())].to_vec();
    while t.len() % 4 != 0 {
        t.push(b'T');
    }
    let idx = |c: u8| B64.iter().position(|&x| x == c).unwrap_or(0) as u32;
    t.chunks(4).flat_map(|g| (g.iter().fold(0, |v, &c| v << 6 | idx(c)) << 8).to_be_bytes()[..3].to_vec()).collect()
}

/// 0x10007590: 5 bytes to 8 characters, a short last group zero-padded.
pub fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for g in data.chunks(5) {
        let mut v = [0u8; 8];
        v[3..3 + g.len()].copy_from_slice(g);
        let v = u64::from_be_bytes(v);
        for k in 0..8 {
            out.push(B32[(v >> (35 - 5 * k) & 31) as usize] as char);
        }
    }
    out
}

/// 0x10007210: 8 characters to 5 bytes; only bytes covered by real characters mean anything.
pub fn b32_decode(text: &[u8], n: usize) -> Vec<u8> {
    let mut t = text[..n.min(text.len())].to_vec();
    while t.len() % 8 != 0 {
        t.push(b'N');
    }
    let idx = |c: u8| B32.iter().position(|&x| x == c).unwrap_or(0) as u64;
    t.chunks(8).flat_map(|g| g.iter().fold(0, |v, &c| v << 5 | idx(c)).to_be_bytes()[3..].to_vec()).collect()
}

/// The 4-character line trailer: the profile checksum's bits 8..31, six at a time from the top, plus 0x3f.
pub fn trailer(body: &[u8]) -> [u8; 4] {
    let s = super::polpro::checksum(body);
    [26, 20, 14, 8].map(|k| ((s >> k) & 0x3f) as u8 + 0x3f)
}

/// The PlayOnline ID's value (0x1001a020): base 36 in polcore's digit order, most significant first.
pub fn pol_id_value(id: &str) -> Option<u64> {
    id.bytes().try_fold(0u64, |v, c| Some(v * 36 + ID_ALPHABET.iter().position(|&x| x == c)? as u64))
}

/// The NICK handle (0x1001a390): 'U' and 8 digits of the value with each byte xored by the one above.
pub fn nick_handle(id: &str) -> Option<String> {
    let mut w = (pol_id_value(id)? & 0x1ff_ffff_ffff) | 0xc000_0000_0000;
    for k in (0..5).rev() {
        w ^= (0xff << (8 * k)) & (w >> 8);
    }
    w &= !0xc000_0000_0000;
    let mut d = [0u8; 8];
    for c in d.iter_mut().rev() {
        *c = ID_ALPHABET[(w % 36) as usize];
        w /= 36;
    }
    Some(format!("U{}", String::from_utf8_lossy(&d)))
}

/// The identity pad (0x10019f20) the process derives from the ID's characters.
fn identity_pad(id: &[u8; 8]) -> [u8; 8] {
    let mut b = *id;
    for i in 1..8 {
        b[i] ^= b[i - 1];
        if b[i] & 2 != 0 {
            b[i] |= 0x80;
        }
    }
    let (mut acc, mut mixed) = (1u64, 0x7048_860d_df79u64);
    for i in 1..8 {
        acc += b[i] as u64;
        mixed = mixed.wrapping_mul(b[i - 1] as u64 * b[i] as u64);
    }
    let mut lcg = 0u32;
    for &c in &b {
        for _ in 0..c.saturating_sub(0x30) {
            lcg = lcg.wrapping_mul(0x425f_0cbd).wrapping_add(0x7f4f);
        }
    }
    let m = mixed.wrapping_add(acc);
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&((m as u32) ^ lcg).to_le_bytes());
    out[4..].copy_from_slice(&((m >> 32) as u32).to_le_bytes());
    out
}

/// The NICK's third field (0x100198f0): {00 00 00 02 01, the area byte, 0, 0, session digest 0, MD5(host record)[..8]};
/// the host record is a 0x40-byte name (none), the identity pad, and zeros. Viewer 1.18.15e's app.dll sets only the
/// area (table 0x51d, 0x1001a1fb) from env.dat's POL_UCS_AREA_KBN, "01" in the US Viewer; nobody sets the others.
pub fn nick_token(id: &str) -> String {
    let mut host = [0u8; 0x52];
    host[0x40..0x48].copy_from_slice(&identity_pad(id.as_bytes().try_into().unwrap_or(&[0; 8])));
    let mut rec = vec![0, 0, 0, 2, 1, AREA, 0, 0];
    rec.extend_from_slice(&[0; 8]);
    rec.extend_from_slice(&Md5::digest(host)[..8]);
    b64_encode(&rec)
}

/// The NICK line's body: the handle, MD5(challenge ‖ password) in lower-case hex, the token.
pub fn nick_line(id: &str, challenge: &[u8], password: &str) -> Option<String> {
    let mut m = Md5::new();
    m.update(challenge);
    m.update(password.as_bytes());
    Some(format!("NICK {}:{}:{}", nick_handle(id)?, hex::encode(m.finalize()), nick_token(id)))
}

/// Unsigned integers as little-endian u32 limbs, for the 256-bit RSA.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Big(Vec<u32>);

impl Big {
    fn from_le(b: &[u8]) -> Self {
        let mut v: Vec<u32> = b.chunks(4).map(|c| c.iter().rev().fold(0, |a, &x| a << 8 | x as u32)).collect();
        while v.last() == Some(&0) {
            v.pop();
        }
        Big(v)
    }

    fn small(x: u32) -> Self {
        Big::from_le(&x.to_le_bytes())
    }

    fn to_le(&self, n: usize) -> Vec<u8> {
        let mut b: Vec<u8> = self.0.iter().flat_map(|w| w.to_le_bytes()).collect();
        b.resize(n, 0);
        b
    }

    fn bits(&self) -> usize {
        self.0.last().map_or(0, |w| self.0.len() * 32 - w.leading_zeros() as usize)
    }

    fn bit(&self, i: usize) -> bool {
        self.0.get(i / 32).is_some_and(|w| w >> (i % 32) & 1 != 0)
    }

    fn cmp(&self, o: &Big) -> std::cmp::Ordering {
        self.0.len().cmp(&o.0.len()).then_with(|| self.0.iter().rev().cmp(o.0.iter().rev()))
    }

    fn sub(&self, o: &Big) -> Big {
        let mut r = self.0.clone();
        let mut borrow = 0i64;
        for (i, w) in r.iter_mut().enumerate() {
            let d = *w as i64 - *o.0.get(i).unwrap_or(&0) as i64 - borrow;
            borrow = (d < 0) as i64;
            *w = d.rem_euclid(1 << 32) as u32;
        }
        let mut b = Big(r);
        while b.0.last() == Some(&0) {
            b.0.pop();
        }
        b
    }

    fn mul(&self, o: &Big) -> Big {
        let mut r = vec![0u64; self.0.len() + o.0.len() + 1];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.0.iter().enumerate() {
                let t = r[i + j] + a as u64 * b as u64 + carry;
                r[i + j] = t & 0xffff_ffff;
                carry = t >> 32;
            }
            r[i + o.0.len()] += carry;
        }
        let mut b = Big(r.into_iter().map(|w| w as u32).collect());
        while b.0.last() == Some(&0) {
            b.0.pop();
        }
        b
    }

    /// Bit-at-a-time long division's remainder.
    fn rem(&self, m: &Big) -> Big {
        let mut r = Big(Vec::new());
        for i in (0..self.bits()).rev() {
            r = r.mul(&Big::small(2));
            if self.bit(i) {
                r = r.add_small(1);
            }
            if r.cmp(m) != std::cmp::Ordering::Less {
                r = r.sub(m);
            }
        }
        r
    }

    fn add_small(&self, x: u32) -> Big {
        let mut r = self.0.clone();
        let mut carry = x as u64;
        for w in r.iter_mut() {
            let t = *w as u64 + carry;
            *w = t as u32;
            carry = t >> 32;
        }
        if carry > 0 {
            r.push(carry as u32);
        }
        Big(r)
    }

    fn rem_small(&self, m: u32) -> u32 {
        self.0.iter().rev().fold(0u64, |r, &w| ((r << 32) | w as u64) % m as u64) as u32
    }

    fn div_small(&self, m: u32) -> Big {
        let mut r = 0u64;
        let mut q = vec![0u32; self.0.len()];
        for i in (0..self.0.len()).rev() {
            let cur = (r << 32) | self.0[i] as u64;
            q[i] = (cur / m as u64) as u32;
            r = cur % m as u64;
        }
        let mut b = Big(q);
        while b.0.last() == Some(&0) {
            b.0.pop();
        }
        b
    }

    fn pow_mod(&self, e: &Big, m: &Big) -> Big {
        let mut r = Big::small(1);
        let base = self.rem(m);
        for i in (0..e.bits()).rev() {
            r = r.mul(&r).rem(m);
            if e.bit(i) {
                r = r.mul(&base).rem(m);
            }
        }
        r
    }
}

const E: u32 = 0xffff;

/// Miller-Rabin with a few random bases (polcore runs one; more only rejects composites it would take).
fn is_prime(n: &Big) -> Result<bool> {
    for p in [3u32, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47] {
        if n.rem_small(p) == 0 {
            return Ok(false);
        }
    }
    let n1 = n.sub(&Big::small(1));
    let s = (0..).find(|&i| n1.bit(i)).unwrap_or(0);
    let mut d = n1.clone();
    for _ in 0..s {
        d = d.div_small(2);
    }
    'bases: for _ in 0..8 {
        let mut a = [0u8; 16];
        getrandom::fill(&mut a).map_err(|e| anyhow!("{e}"))?;
        let a = Big::from_le(&a).rem(&n1.sub(&Big::small(1))).add_small(2);
        let mut x = a.pow_mod(&d, n);
        if x == Big::small(1) || x == n1 {
            continue;
        }
        for _ in 1..s {
            x = x.mul(&x).rem(n);
            if x == n1 {
                continue 'bases;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

/// A 128-bit prime as polcore draws them (0x1002c820, 0x1002b800): bit 0 and bit 127 set, stepping up by 2 until
/// gcd(c - 1, 0xffff) = 1 and c is prime.
fn prime() -> Result<Big> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| anyhow!("{e}"))?;
    b[0] |= 1;
    b[15] = (b[15] & 0x0f) | 0x80;
    let mut c = Big::from_le(&b);
    loop {
        let r = c.sub(&Big::small(1)).rem_small(E);
        if gcd(r, E) == 1 && is_prime(&c)? {
            return Ok(c);
        }
        c = c.add_small(2);
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The client's RSA key (0x1002bb40): n of two 128-bit primes, e = 0xffff (never sent), d its inverse mod φ.
pub struct RsaKey {
    n: Big,
    d: Big,
}

impl RsaKey {
    pub fn generate() -> Result<Self> {
        loop {
            let (p, q) = (prime()?, prime()?);
            if p == q {
                continue;
            }
            let phi = p.sub(&Big::small(1)).mul(&q.sub(&Big::small(1)));
            // d = (1 + k·φ) / e with k = -φ⁻¹ mod e.
            let r = phi.rem_small(E) as i64;
            let Some(inv) = (1..E as i64).find(|x| r * x % E as i64 == 1) else { continue };
            let k = (E as i64 - inv) as u32;
            let d = phi.mul(&Big::small(k)).add_small(1).div_small(E);
            return Ok(Self { n: p.mul(&q), d });
        }
    }

    /// n as the 32 little-endian bytes USER's realname carries (and whose first 8 are the cipher's IV).
    pub fn modulus_le(&self) -> [u8; 32] {
        self.n.to_le(32).try_into().unwrap()
    }

    /// 0x1002bde0: m = c^d mod n on the big-endian block, then the type-2 unpad on its little-endian bytes (the type
    /// byte unchecked); the message in natural order. None when the block is shorter than 32 bytes.
    pub fn decrypt(&self, c_be: &[u8]) -> Option<Vec<u8>> {
        let c: Vec<u8> = c_be.get(..32)?.iter().rev().copied().collect();
        let mut m = Big::from_le(&c).pow_mod(&self.d, &self.n).to_le(32);
        m[0x1e] = 0;
        let mut i = 0x1d;
        let mut count = 3;
        while i > 0 && m[i] != 0 {
            m[i] = 0;
            count += 1;
            i -= 1;
        }
        let len = 32usize.saturating_sub(count);
        Some(m[..len].iter().rev().copied().collect())
    }
}

/// The rest of a line after its `:prefix` and `skip` more words (polcore's numeric and greeting parse).
fn rest(line: &[u8], skip: usize) -> &[u8] {
    let mut s = line;
    if s.first() == Some(&b':') {
        s = s.splitn(2, |&c| c == b' ').nth(1).unwrap_or(&[]);
    }
    for _ in 0..skip {
        s = s.splitn(2, |&c| c == b' ').nth(1).unwrap_or(&[]);
    }
    s
}

/// The command word after the prefix.
fn command(line: &[u8]) -> &[u8] {
    let s = if line.first() == Some(&b':') { rest(line, 0) } else { line };
    s.split(|&c| c == b' ').next().unwrap_or(&[])
}

/// What a refusal during sign-in means.
fn refusal(body: &[u8]) -> Option<String> {
    let cmd = command(body);
    if cmd == b"ERROR" {
        let text = String::from_utf8_lossy(body);
        if text.contains("Kicked by same NICK") {
            return Some("This PlayOnline ID signed in somewhere else.".into());
        }
        if let Some(i) = text.find("(POL ") {
            let code = b32_decode(&body[i + 5..], 16).get(6).copied().unwrap_or(0);
            return Some(format!("PlayOnline refused the sign-in (POL error {code:#x})."));
        }
        return Some(format!("PlayOnline closed the connection: {text}"));
    }
    let n: u32 = std::str::from_utf8(cmd).ok()?.parse().ok()?;
    match n {
        433 => Some("This PlayOnline ID is already signed in.".into()),
        464 => Some("The PlayOnline ID or PlayOnline password is incorrect.".into()),
        // 422 (no MOTD) is the registration's end, not a refusal.
        422 => None,
        400.. => Some(format!("PlayOnline refused the sign-in ({n}).")),
        _ => None,
    }
}

/// A signed-in chat connection and what the profile service takes from it.
pub struct Chat {
    sock: TcpStream,
    buf: Vec<u8>,
    pub cipher: PolBlowfish,
    pub key: [u8; 8],
    /// The greeting's server clock and when it came: the session's time base (0x1004cca0).
    pub clock: u32,
    clock_at: Instant,
    /// {u16 1, u16 port, u32 IPv4} host order, then zeros: the client's address as the greeting gave it.
    pub addr20: [u8; 20],
    /// The routing struct's region and profile host byte.
    pub region: u16,
    pub host_byte: u8,
    /// The profile-available NOTICE's +0x21: the member login (4/7) is skipped (1.18.15e's POLCON 0x1b).
    pub skip_member_login: bool,
}

impl Chat {
    /// The session's time: the greeting's clock plus whole seconds since.
    pub fn now(&self) -> u32 {
        self.clock.wrapping_add(self.clock_at.elapsed().as_secs() as u32)
    }

    fn read_raw(sock: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Vec<u8>> {
        loop {
            if let Some(i) = buf.iter().position(|&c| c == b'\n') {
                return Ok(buf.drain(..=i).collect());
            }
            let mut tmp = [0u8; 4096];
            let n = sock.read(&mut tmp).context("reading from PlayOnline")?;
            if n == 0 {
                bail!("PlayOnline closed the connection");
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// A plaintext line as polcore takes it before the cipher: the byte before LF dropped, the trailer kept.
    fn read_plain(sock: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Vec<u8>> {
        let mut l = Self::read_raw(sock, buf)?;
        l.pop();
        l.pop();
        Ok(l)
    }

    fn frame(body: &str) -> Vec<u8> {
        let mut l = body.as_bytes().to_vec();
        l.extend_from_slice(&trailer(body.as_bytes()));
        l.extend_from_slice(b"\r\n");
        l
    }

    /// An enciphered line's body: deciphered from the IV, its trailer checked (bad lines are dropped, as polcore).
    pub fn read_line(&mut self) -> Result<Option<Vec<u8>>> {
        let mut l = Self::read_raw(&mut self.sock, &mut self.buf)?;
        self.cipher.stream(&mut l, true);
        while matches!(l.last(), Some(b'\n' | b'\r')) {
            l.pop();
        }
        if l.len() < 5 {
            return Ok(None);
        }
        let (body, tr) = l.split_at(l.len() - 4);
        Ok((trailer(body) == tr).then(|| body.to_vec()))
    }

    pub fn send_line(&mut self, body: &str) -> Result<()> {
        let mut l = Self::frame(body);
        self.cipher.stream(&mut l, true);
        self.sock.write_all(&l)?;
        Ok(())
    }

    /// Signs the PlayOnline ID in: greeting, USER with the client modulus, the 300 key, NICK, the routing 300 and 422,
    /// then the profile-available NOTICE.
    pub fn sign_in(host: &str, port: u16, pol_id: &str, pol_password: &str) -> Result<Self> {
        let key = RsaKey::generate()?;
        let mut target = (host.to_string(), port);
        let mut redirected = false;
        let (mut sock, mut buf, greeting) = loop {
            let sock = TcpStream::connect((target.0.as_str(), target.1)).with_context(|| format!("connecting to PlayOnline ({}:{})", target.0, target.1))?;
            sock.set_read_timeout(Some(Duration::from_secs(30)))?;
            let mut s = sock;
            let mut buf = Vec::new();
            let g = Self::read_plain(&mut s, &mut buf)?;
            log::info!("PlayOnline chat: greeting {}", String::from_utf8_lossy(&g));
            let challenge = rest(&g, 2);
            let challenge = challenge[..challenge.len().saturating_sub(4)].to_vec();
            let blob = b32_decode(&challenge, challenge.len());
            if blob.len() < 0x18 {
                bail!("PlayOnline's greeting was not understood");
            }
            // A redirect, once (chat state 3).
            let to = Ipv4Addr::new(blob[8], blob[9], blob[10], blob[11]);
            if !to.is_unspecified() && !redirected {
                let p = u16::from_be_bytes([blob[12], blob[13]]);
                target = (to.to_string(), if p == 0 { PORT } else { p });
                redirected = true;
                continue;
            }
            break (s, buf, (challenge, blob));
        };
        let (challenge, blob) = greeting;
        let clock = u32::from_be_bytes(blob[..4].try_into().unwrap());
        let mut addr20 = [0u8; 20];
        addr20[..2].copy_from_slice(&1u16.to_le_bytes());
        addr20[2..4].copy_from_slice(&u16::from_be_bytes([blob[0x14], blob[0x15]]).to_le_bytes());
        addr20[4..8].copy_from_slice(&u32::from_be_bytes(blob[4..8].try_into().unwrap()).to_le_bytes());
        let clock_at = Instant::now();

        let modulus = key.modulus_le();
        sock.write_all(&Self::frame(&format!("USER x 8 * :{}", b64_encode(&modulus))))?;
        // Numeric 300: the session key, RSA-encrypted to our modulus.
        let session = loop {
            let l = Self::read_plain(&mut sock, &mut buf)?;
            log::info!("PlayOnline chat: {} ({} bytes, plain)", String::from_utf8_lossy(command(&l)), l.len());
            if let Some(why) = refusal(&l[..l.len().saturating_sub(4)]) {
                bail!(why);
            }
            if command(&l) == b"300" {
                let block = b64_decode(rest(&l, 2), 0x2e);
                // A short or malformed reply is refused here rather than panicking the chat thread.
                let Some(msg) = key.decrypt(&block).filter(|m| m.len() >= 8) else {
                    bail!("PlayOnline's session key was not understood");
                };
                // The key is the message's first 8 bytes, reversed.
                let mut k: [u8; 8] = msg[..8].try_into().unwrap();
                k.reverse();
                break k;
            }
        };
        let mut chat = Self {
            sock,
            buf,
            cipher: PolBlowfish::new(&session, modulus[..8].try_into().unwrap()),
            key: session,
            clock,
            clock_at,
            addr20,
            region: 0,
            host_byte: 0,
            skip_member_login: false,
        };
        let nick = nick_line(pol_id, &challenge, pol_password).context("a PlayOnline ID is four letters and four digits")?;
        chat.send_line(&nick)?;
        let mut registered = false;
        loop {
            let Some(l) = chat.read_line()? else {
                log::info!("PlayOnline chat: a line failed its trailer");
                continue;
            };
            log::info!("PlayOnline chat: {} ({} bytes)", String::from_utf8_lossy(command(&l)), l.len());
            if let Some(why) = refusal(&l) {
                bail!(why);
            }
            match command(&l) {
                b"PING" => chat.send_line("PONG a")?,
                // The routing struct counts only before 422.
                b"300" if !registered => {
                    let r = b32_decode(rest(&l, 2), 0x2e);
                    log::info!("PlayOnline chat: routing {}", hex::encode(&r[..r.len().min(0x18)]));
                    // A short routing reply is logged and skipped, not indexed out of range.
                    let [a, b, host, ..] = r[..] else { continue };
                    chat.region = u16::from_be_bytes([a, b]);
                    chat.host_byte = host;
                }
                b"422" => registered = true,
                b"NOTICE" if registered && !rest(&l, 1).starts_with(b"#") => {
                    let text = rest(&l, 2);
                    let text = text.strip_prefix(b":").unwrap_or(text);
                    let h = b64_decode(text, 0x60);
                    // Read up to 0x42 below; a shorter notice is not the one we wait for.
                    if h.len() <= 0x42 {
                        continue;
                    }
                    // The profile service's "available" notice (0x1001b6f0).
                    let ty = u16::from_le_bytes([h[0x3e], h[0x3f]]) >> 7 & 0x1f;
                    if h[0x42] & 1 != 0 && ty == 31 && h[0x1b] != 0 && h[0x18] == 0xff && h[0x1c] == 0xff && h[0x10] == 1 {
                        chat.skip_member_login = h[0x21] != 0;
                        chat.sock.set_read_timeout(None)?;
                        return Ok(chat);
                    }
                }
                _ => {}
            }
        }
    }

    /// Another handle on the connection, to close it from outside.
    pub fn socket(&self) -> Option<TcpStream> {
        self.sock.try_clone().ok()
    }

    /// Keeps the session up for the game: PING answered with PONG, the rest read and dropped. Ends when the server
    /// closes it.
    pub fn keep_alive(mut self) {
        std::thread::spawn(move || {
            while let Ok(l) = self.read_line() {
                if l.as_deref().is_some_and(|l| command(l) == b"PING") && self.send_line("PONG a").is_err() {
                    break;
                }
            }
            log::info!("PlayOnline: the chat connection closed");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    // Vectors from polcore 73b1864b run under Wine (lab: scratch/pol/chat/vectors.json).

    #[test]
    fn codecs() {
        assert_eq!(b64_encode(&h("00")), "TT");
        assert_eq!(b64_encode(&h("00010203")), "TTIGTt");
        assert_eq!(b64_encode(&(0..32).collect::<Vec<u8>>()), "TTIGTt7nSAY3G7iK8TdO8pT9IUoen9sZcS5ycptEWUN");
        assert_eq!(b64_decode(b"TTIGTt7nSAY3G7iK8TdO8pT9IUoen9sZcS5ycptEWUN", 43)[..32].to_vec(), (0..32).collect::<Vec<u8>>());
        assert_eq!(b32_encode(&h("0001020304")), "NNNEVNZV");
        assert_eq!(b32_encode(&(0..25).collect::<Vec<u8>>()), "NNNEVNZVNFONS3NY41HEZO1S4A143VEQ3E2D0HZZ");
        assert_eq!(b32_decode(b"NNNEVNZVNFONS3NY41HEZO1S4A143VEQ3E2D0HZZ", 40), (0..25).collect::<Vec<u8>>());
        assert_eq!(trailer(b"USER x 8 * :TSG8").len(), 4);
    }

    #[test]
    fn identity_and_nick() {
        assert_eq!(pol_id_value("ABCD1234"), Some(0x3e_9c91_c1aa));
        assert_eq!(pol_id_value("EFKAOYMJ"), Some(0x8944_2e2f));
        assert_eq!(nick_handle("ABCD1234").as_deref(), Some("US77G53VV"));
        assert_eq!(nick_handle("EFKAOYMJ").as_deref(), Some("UGCWJTDPO"));
        assert_eq!(identity_pad(b"ABCD1234").to_vec(), h("20fbe79048d7a031"));
        assert_eq!(identity_pad(b"EFKAOYMJ").to_vec(), h("6cc447f2f5608141"));
        // As 1.18.15e sends it after app.dll's area "01" (chat runs/15e/full).
        assert_eq!(nick_token("ABCD1234"), "TTTTTAISTTTTTTTTTTTTTHBy5pjMnc0G");
        assert_eq!(nick_line("ABCD1234", b"CHALLENGE", "testpass").as_deref(), Some("NICK US77G53VV:82c40f69877fc2359531c90cc3f090dc:TTTTTAISTTTTTTTTTTTTTHBy5pjMnc0G"));
    }

    #[test]
    fn rsa_matches_polcore() {
        let k = RsaKey {
            n: Big::from_le(&h("a9117fcd9fc86ba6ff0444ab91083b0f1b440d15457364a6e1cfc47d0c4d094a")),
            d: Big::from_le(&h("af83a34b7d9da940222e3fa56a724d79fd396e56595f026a272f96431bb43c40")),
        };
        assert_eq!(k.decrypt(&h("0e4cc04625bcb376824893ff1c49e6e4777d7bdb03968d85c47a100193d45984")), Some(h("1122334455667788")));
        assert_eq!(b64_encode(&h("97400b4f21d135a21c85ba38b12b7ed1e31e309b078f7f68c73dab4b0971d444")), "rdTKzvW9ky3YUBiJjqMudQoQoHjWR_6ipaf@qtrp0I7");
        // A fresh pair decrypts what the server encrypts with e = 0xffff.
        let k = RsaKey::generate().unwrap();
        assert_eq!(k.n.bits(), 255);
        let mut em = vec![0u8, 2];
        em.extend((0..21).map(|i| 0x11 + i as u8));
        em.push(0);
        em.extend_from_slice(&h("0807060504030201"));
        let m = Big::from_le(&em.iter().rev().copied().collect::<Vec<u8>>());
        let c: Vec<u8> = m.pow_mod(&Big::small(E), &k.n).to_le(32).into_iter().rev().collect();
        assert_eq!(k.decrypt(&c), Some(h("0807060504030201")));
        // A block short of 32 bytes is refused, not sliced out of range.
        assert_eq!(k.decrypt(&c[..31]), None);
    }
}
