//! LSB's xi_profile, which stands in for PlayOnline's profile and IRC services (research friend_list_spec.md 3): TLS
//! 1.3, every connection opening with {u32 account id, session hash}. The profile port answers one request per
//! connection; the IRC port is one long session for presence and new-mail notices.

use super::tls;
use anyhow::{Context, Result, bail};
use std::io::{BufRead, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The account's credentials from the game login and where xi_profile listens.
#[derive(Clone, Debug)]
pub struct Cred {
    pub host: String,
    pub port: u16,
    pub irc_port: u16,
    pub account: u32,
    pub hash: [u8; 16],
}

/// XORed into both POL ids of a message header or notice.
pub const POL_MASK: u64 = 0x1C27_3E45_6789_1133;

/// polcore's sqCalcChecksum: the little-endian u32 words summed, a 1..3-byte tail folded in from the top.
pub fn checksum(b: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunks = b.chunks_exact(4);
    for c in &mut chunks {
        sum = sum.wrapping_add(u32::from_le_bytes([c[0], c[1], c[2], c[3]]));
    }
    let tail = chunks.remainder().iter().fold(0u32, |t, &x| t >> 8 | (x as u32) << 24);
    sum.wrapping_add(tail)
}

/// An IRC line's four checksum characters.
pub fn line_checksum(s: &str) -> String {
    let sum = checksum(s.as_bytes());
    [26, 20, 14, 8].iter().map(|&k| (((sum >> k) & 0x3f) as u8 + 0x3f) as char).collect()
}

const B64: &[u8; 64] = b"TSG8IncW3HFKokOg79qzeCmZs2yBYEQVAUxR5rbwi4P@jMDLtpvad0f_J1hlN6uX";

/// polcore's base64: its own alphabet, standard bit order, a short group padded with the zero digit `T` (or not at all).
pub fn b64(bytes: &[u8], padded: bool) -> String {
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        let n = c.len() + 1;
        for k in 0..4 {
            if k < n {
                out.push(B64[(v >> (18 - 6 * k) & 0x3f) as usize] as char);
            } else if padded {
                out.push('T');
            }
        }
    }
    out
}

/// Decodes whole 4-character groups (padding reads as zero bits).
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for g in s.as_bytes().chunks(4) {
        let mut v = 0u32;
        for (k, &ch) in g.iter().enumerate() {
            v |= (B64.iter().position(|&a| a == ch)? as u32) << (18 - 6 * k);
        }
        let n = g.len().saturating_sub(1);
        out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8][..n]);
    }
    Some(out)
}

/// The IRC nick for a POL id (codec scrambleNick): "U" and 8 base-36 digits of polcore's alphabet.
pub fn nick(pol: u64) -> String {
    const DIGITS: &[u8] = b"EFKAOYMJVNGTDSWBQLPCIRHZXU6328401795";
    let mut v = (pol & 0x1ff_ffff_ffff) | 0xc000_0000_0000;
    for byte in [4, 3, 2, 1, 0] {
        v ^= (v >> 8) & (0xff << (8 * byte));
    }
    v &= !0xc000_0000_0000;
    let mut d = [b'E'; 8];
    for x in d.iter_mut().rev() {
        *x = DIGITS[(v % 36) as usize];
        v /= 36;
    }
    format!("U{}", String::from_utf8_lossy(&d))
}

/// FFXI's character key (characterKey): the id's low 16 bits, bits 16..23 moved to the top byte.
pub fn character_key(id: u32) -> u32 {
    (id & 0xffff) | ((id >> 16) & 0xff) << 24
}

fn connect(c: &Cred, port: u16) -> Result<tls::TlsStream> {
    let mut s = tls::connect(&c.host, port).with_context(|| format!("connecting to xi_profile {}:{port}", c.host))?;
    let mut hello = [0u8; 20];
    hello[..4].copy_from_slice(&c.account.to_le_bytes());
    hello[4..].copy_from_slice(&c.hash);
    s.write_all(&hello)?;
    Ok(s)
}

/// One profile request (3.2): ProfOpenData, ProfOpenAns 0x81, ProfSendReqData {kind 2, command, sub, size}, the body;
/// the answer's error (0 ok, 1 refused, 0x7e not found) and body.
pub fn request(c: &Cred, cmd: u8, sub: u8, body: &[u8]) -> Result<(u8, Vec<u8>)> {
    let mut s = connect(c, c.port)?;
    s.sock.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut open = [0u8; 0x28];
    open[4] = 1;
    s.write_all(&open)?;
    let mut ans = [0u8; 0x18];
    s.read_exact(&mut ans).context("xi_profile closed the connection (refused the credentials?)")?;
    if ans[0] != 0x81 || ans[1] != 0 {
        bail!("xi_profile open answer {:#x} error {}", ans[0], ans[1]);
    }
    let mut head = [0u8; 0x28];
    (head[0], head[1], head[2]) = (2, cmd, sub);
    head[4..8].copy_from_slice(&(body.len() as u32).to_le_bytes());
    s.write_all(&head)?;
    s.write_all(body)?;
    s.flush()?;
    let mut recv = [0u8; 0x18];
    s.read_exact(&mut recv).with_context(|| format!("no answer to request {cmd:#x}/{sub:#x}"))?;
    if recv[0] != 0x83 {
        bail!("xi_profile answer code {:#x}", recv[0]);
    }
    let size = u32::from_le_bytes(recv[4..8].try_into().unwrap()) as usize;
    let mut out = vec![0u8; size];
    s.read_exact(&mut out)?;
    s.conn.send_close_notify();
    let _ = s.flush();
    Ok((recv[1], out))
}

/// The body sealed with its checksum (the last u32).
fn sealed(mut b: Vec<u8>) -> Vec<u8> {
    let sum = checksum(&b);
    b.extend_from_slice(&sum.to_le_bytes());
    b
}

fn ok(r: (u8, Vec<u8>), what: &str) -> Result<Vec<u8>> {
    match r.0 {
        0 => Ok(r.1),
        e => bail!("{what}: xi_profile error {e:#x}"),
    }
}

fn count(b: &[u8]) -> usize {
    b.get(..4).map_or(0, |c| u32::from_le_bytes(c.try_into().unwrap()) as usize)
}

fn name16(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// FriendInfo (3.3): one friend or block-list entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FriendInfo {
    /// 1 set, 2 delete.
    pub op: u8,
    pub black: bool,
    pub handle_id: u64,
    pub pending: bool,
    /// The list index (0..199 friends, 0..99 block list) and display order.
    pub num: u8,
    pub order: u8,
    /// A store's answer: 0 applied, 1 refused.
    pub result: u8,
    pub pol: u64,
    pub name: String,
}

pub const FRIEND_INFO: usize = 0xa8;

impl FriendInfo {
    pub fn to_bytes(&self) -> [u8; FRIEND_INFO] {
        let mut b = [0u8; FRIEND_INFO];
        let bits = (self.op as u64 & 0xf) | (self.black as u64) << 4 | 1 << 5 | (self.handle_id & 0xfff_ffff_ffff) << 13 | 0xe << 57 | (self.pending as u64) << 62;
        b[..8].copy_from_slice(&bits.to_le_bytes());
        (b[8], b[9], b[10]) = (self.num, self.order, self.result);
        b[0x10..0x18].copy_from_slice(&self.pol.to_le_bytes());
        let n = self.name.as_bytes();
        b[0x18..0x18 + n.len().min(15)].copy_from_slice(&n[..n.len().min(15)]);
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let b = b.get(..FRIEND_INFO)?;
        let bits = u64::from_le_bytes(b[..8].try_into().unwrap());
        Some(Self {
            op: (bits & 0xf) as u8,
            black: bits >> 4 & 1 != 0,
            handle_id: bits >> 13 & 0xfff_ffff_ffff,
            pending: bits >> 62 & 1 != 0,
            num: b[8],
            order: b[9],
            result: b[10],
            pol: u64::from_le_bytes(b[0x10..0x18].try_into().unwrap()),
            name: name16(&b[0x18..0x28]),
        })
    }
}

/// LoadFriendList (0x02 / 0x03): friends, then the block list.
pub fn load_friend_list(c: &Cred) -> Result<Vec<FriendInfo>> {
    let b = ok(request(c, 2, 3, &[])?, "LoadFriendList")?;
    Ok((0..count(&b)).filter_map(|k| FriendInfo::from_bytes(b.get(8 + k * FRIEND_INFO..)?)).collect())
}

/// StoreFriendList (0x02 / 0x06): every list index's display order and the changed entries; the entries answered.
pub fn store_friend_list(c: &Cred, friend_order: &[u8; 200], black_order: &[u8; 100], entries: &[FriendInfo]) -> Result<Vec<FriendInfo>> {
    let mut b = Vec::with_capacity(0x130 + entries.len() * FRIEND_INFO + 8);
    b.extend_from_slice(friend_order);
    b.extend_from_slice(black_order);
    b.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    for e in entries {
        b.extend_from_slice(&e.to_bytes());
    }
    b.extend_from_slice(&[0; 4]);
    let a = ok(request(c, 2, 6, &sealed(b))?, "StoreFriendList")?;
    Ok((0..count(&a)).filter_map(|k| FriendInfo::from_bytes(a.get(8 + k * FRIEND_INFO..)?)).collect())
}

/// LoadHandleNameList (0x00 / 0x09): our handle's id and name (LSB: the account's first character).
pub fn load_handle(c: &Cred) -> Result<(u64, String)> {
    let b = ok(request(c, 0, 9, &[])?, "LoadHandleNameList")?;
    let h = b.get(8..8 + 0x88).context("no handle")?;
    Ok((u64::from_le_bytes(h[8..16].try_into().unwrap()) & 0xfff_ffff_ffff, name16(&h[0x10..0x20])))
}

/// LoadCharacterList (0x01 / 0x03): (slot, character id, name) in the account's order.
pub fn load_characters(c: &Cred) -> Result<Vec<(u8, u32, String)>> {
    let b = ok(request(c, 1, 3, &[])?, "LoadCharacterList")?;
    Ok((0..count(&b))
        .filter_map(|k| {
            let e = b.get(8 + k * 0x68..8 + (k + 1) * 0x68)?;
            Some((e[0], u64::from_le_bytes(e[0x10..0x18].try_into().unwrap()) as u32, name16(&e[0x18..0x28])))
        })
        .collect())
}

/// SearchPolId (0x01 / 0x0B) for a character in game: its owner's POL id and the character's name.
pub fn search_pol_id(c: &Cred, char_id: u32) -> Result<Option<(u64, String)>> {
    let mut b = vec![0u8; 0x14];
    b[8..12].copy_from_slice(&character_key(char_id).to_le_bytes());
    b[0xc..0xe].copy_from_slice(&1u16.to_le_bytes());
    let a = ok(request(c, 1, 0xb, &sealed(b))?, "SearchPolId")?;
    if a.len() < 0x30 || a[0x20] == 0 {
        return Ok(None);
    }
    Ok(Some((u64::from_le_bytes(a[..8].try_into().unwrap()), name16(&a[0x10..0x20]))))
}

/// LoadMyStatus (0x04 / 0x06): our open status (0 Online, 1 Away, 4 Invisible).
pub fn load_my_status(c: &Cred) -> Result<u8> {
    let b = ok(request(c, 4, 6, &[])?, "LoadMyStatus")?;
    Ok(b.get(6).map_or(0, |&s| s.saturating_sub(1)))
}

/// ChangeMyStatus (0x04 / 0x05): in game with character `slot` (None: not in game), `open` the open status to set
/// (None keeps it).
pub fn change_my_status(c: &Cred, slot: Option<u8>, open: Option<u8>) -> Result<()> {
    let mut b = vec![0u8; 0x24];
    b[0x11] = slot.is_some() as u8;
    b[0x12] = slot.unwrap_or(0);
    b[0x13] = 1;
    b[0x14..0x16].copy_from_slice(&1u16.to_le_bytes());
    b[0x16] = open.map_or(0, |o| o + 1);
    ok(request(c, 4, 5, &sealed(b))?, "ChangeMyStatus").map(|_| ())
}

/// ProfFileHead (0x198): target (0 our own files), path, offset, length.
fn file_head(target: u64, path: &str, offset: u32, length: u32) -> Vec<u8> {
    let mut b = vec![0u8; 0x198];
    b[8..16].copy_from_slice(&target.to_le_bytes());
    let p = path.as_bytes();
    b[0x10..0x10 + p.len().min(0x17f)].copy_from_slice(&p[..p.len().min(0x17f)]);
    b[0x190..0x194].copy_from_slice(&offset.to_le_bytes());
    b[0x194..0x198].copy_from_slice(&length.to_le_bytes());
    b
}

/// The mailbox.
pub const MAILBOX: &str = "O/m/";

/// GetFileList (0x03 / 0x03) of the mailbox: (time, size, name) oldest first.
pub fn mail_list(c: &Cred) -> Result<Vec<(u32, u32, String)>> {
    let mut b = file_head(0, MAILBOX, 0, 0);
    b.extend_from_slice(&[0; 4]);
    let a = ok(request(c, 3, 3, &sealed(b))?, "GetFileList")?;
    Ok((0..count(&a))
        .filter_map(|k| {
            let e = a.get(8 + k * 0x108..8 + (k + 1) * 0x108)?;
            Some((u32::from_le_bytes(e[..4].try_into().unwrap()), u32::from_le_bytes(e[4..8].try_into().unwrap()), name16(&e[8..])))
        })
        .collect())
}

/// ReadFile (0x03 / 0x00): a whole file of ours (the answer's trailing checksum dropped).
pub fn read_file(c: &Cred, path: &str) -> Result<Vec<u8>> {
    let mut b = file_head(0, path, 0, u32::MAX);
    b.extend_from_slice(&[0; 4]);
    let mut a = ok(request(c, 3, 0, &sealed(b))?, "ReadFile")?;
    a.truncate(a.len().saturating_sub(4));
    Ok(a)
}

/// WriteFile (0x03 / 0x01) to another account: posts a message into its mailbox.
pub fn write_file(c: &Cred, target: u64, path: &str, data: &[u8]) -> Result<()> {
    let mut b = file_head(target, path, 0, data.len() as u32);
    b.extend_from_slice(data);
    ok(request(c, 3, 1, &sealed(b))?, "WriteFile").map(|_| ())
}

/// RemoveFile (0x03 / 0x02): the head's length field carries the checksum.
pub fn remove_file(c: &Cred, path: &str) -> Result<()> {
    let b = sealed(file_head(0, path, 0, 0)[..0x194].to_vec());
    ok(request(c, 3, 2, &b)?, "RemoveFile").map(|_| ())
}

/// What the IRC session hears.
#[derive(Clone, Debug)]
pub enum Irc {
    /// The profile-available notice: signed on.
    SignedOn,
    /// A friend's status (Status - 1: 0 offline, 1 online, 2 away), where we list them, the character they play, and
    /// the notice's age (only newer ones apply).
    Status { account: u64, list_index: u8, status: u8, char_id: Option<u32>, age: u64 },
    /// A new message: its 0x48-byte header.
    Mail(Vec<u8>),
    Closed(String),
}

/// The IRC session (3.4) on its own thread; dropping the sender ends it with QUIT.
pub struct IrcLink {
    pub events: mpsc::Receiver<Irc>,
    pub quit: mpsc::Sender<()>,
}

pub fn irc(c: Cred) -> IrcLink {
    let (tx, events) = mpsc::channel();
    let (quit, stop) = mpsc::channel();
    std::thread::spawn(move || {
        let why = match irc_run(&c, &tx, &stop) {
            Ok(()) => "quit".to_string(),
            Err(e) => format!("{e:#}"),
        };
        let _ = tx.send(Irc::Closed(why));
    });
    IrcLink { events, quit }
}

fn send_line(s: &mut tls::TlsStream, text: &str) -> Result<()> {
    s.write_all(format!("{text}{}\r\n", line_checksum(text)).as_bytes())?;
    s.flush()?;
    Ok(())
}

fn irc_run(c: &Cred, tx: &mpsc::Sender<Irc>, stop: &mpsc::Receiver<()>) -> Result<()> {
    let mut s = connect(c, c.irc_port)?;
    s.sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    let me = nick(c.account as u64);
    send_line(&mut s, "USER vana")?;
    send_line(&mut s, &format!("NICK {me}"))?;
    let mut r = std::io::BufReader::new(s);
    let mut line = String::new();
    let mut last = Instant::now();
    loop {
        match stop.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                let _ = send_line(r.get_mut(), "QUIT");
                return Ok(());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        // The server pings after 60 s of silence; ours comes first.
        if last.elapsed() > Duration::from_secs(50) {
            send_line(r.get_mut(), "PING :x")?;
            last = Instant::now();
        }
        match r.read_line(&mut line) {
            Ok(0) => bail!("closed by the server"),
            Ok(_) if line.ends_with('\n') => {
                let l = line.trim_end_matches(['\r', '\n']);
                let text = &l[..l.len().saturating_sub(4)];
                irc_line(r.get_mut(), text, tx)?;
                line.clear();
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn irc_line(s: &mut tls::TlsStream, text: &str, tx: &mpsc::Sender<Irc>) -> Result<()> {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).skip(text.starts_with(':') as usize).collect();
    match words.first().copied() {
        Some("PING") => send_line(s, "PONG a")?,
        Some("NOTICE") => {
            let payload = text.split_once(" :").map_or("", |p| p.1);
            if let Some(ev) = notice(payload) {
                let _ = tx.send(ev);
            }
        }
        _ => {}
    }
    Ok(())
}

/// A notice's payload (3.4.3): the 0x48-byte header in 96 characters, then a status notice's extended block.
fn notice(payload: &str) -> Option<Irc> {
    let h = b64_decode(payload.get(..96)?)?;
    if h.len() < 0x48 {
        return None;
    }
    let ty = u16::from_le_bytes([h[0x3e], h[0x3f]]) >> 7 & 0x1f;
    let polpro = h[0x42] & 1 != 0;
    if ty != 31 {
        return polpro.then(|| Irc::Mail(h[..0x48].to_vec()));
    }
    if h[0x10] == 1 {
        return Some(Irc::SignedOn);
    }
    let account = u64::from_le_bytes(h[..8].try_into().unwrap()) ^ POL_MASK;
    let ext = b64_decode(&payload[96..]).unwrap_or_default();
    let char_id = (ext.first().is_some_and(|f| f & 8 != 0) && ext.len() >= 0x18).then(|| u64::from_le_bytes(ext[0x10..0x18].try_into().unwrap()) as u32);
    Some(Irc::Status { account, list_index: h[0x1c], status: h[0x11].saturating_sub(1), char_id, age: u64::from_le_bytes(h[0x30..0x38].try_into().unwrap()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's vectors (friend_list_spec.md 4.1, derived from LSB's codec).
    #[test]
    fn codec() {
        assert_eq!(checksum(&[1, 2, 3, 4, 5]), 0x0903_0201);
        assert_eq!(checksum(b"USER x"), 0xca65_5355);
        assert_eq!(line_checksum("USER x"), "qeTR");
        assert_eq!(line_checksum("NICK UABCDEFGH"), "F@Ba");
        assert_eq!(line_checksum("PONG :x"), "nw`n");
        assert_eq!(b64(&[0, 1, 2, 3, 4, 5], true), "TTIGTt7n");
        assert_eq!(b64_decode("TTIGTt7n").unwrap(), vec![0, 1, 2, 3, 4, 5]);
        let f = FriendInfo { op: 1, handle_id: 1003, ..Default::default() };
        assert_eq!(&f.to_bytes()[..8], &[0x21, 0x60, 0x7d, 0, 0, 0, 0, 0x1c]);
        assert_eq!(FriendInfo::from_bytes(&f.to_bytes()).unwrap().handle_id, 1003);
        assert_eq!(character_key(0x12_3456), 0x1200_3456);
    }

    /// The local xi_profile with lnxtest01 (by hand: `cargo test --release profile_live -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn profile_live() {
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read("/home/lenon/ffxi/lab/lnxtest01.json").unwrap()).unwrap();
        let req = super::super::auth::Request { username: j["username"].as_str().unwrap(), password: j["password"].as_str().unwrap(), ..Default::default() };
        let super::super::auth::Reply::LoggedIn { account_id, session_hash, .. } = super::super::auth::send("127.0.0.1", 54231, super::super::auth::Command::Login, &req).unwrap() else { panic!() };
        let c = Cred { host: "127.0.0.1".into(), port: 51220, irc_port: 51240, account: account_id, hash: session_hash };
        let link = irc(c.clone());
        println!("irc: {:?}", link.events.recv_timeout(Duration::from_secs(10)));
        println!("status {:?}", load_my_status(&c));
        println!("friends {:?}", load_friend_list(&c));
        println!("handle {:?}", load_handle(&c));
        println!("chars {:?}", load_characters(&c));
        println!("mail {:?}", mail_list(&c));
        let _ = link.quit.send(());
        println!("irc: {:?}", link.events.recv_timeout(Duration::from_secs(5)));
    }

    #[test]
    fn padding() {
        let h = vec![7u8; 0x48];
        assert_eq!(b64(&h, true).len(), 96);
        assert_eq!(b64_decode(&b64(&h, true)).unwrap(), h);
        assert_eq!(b64(&[1, 2], false).len(), 3);
    }
}
