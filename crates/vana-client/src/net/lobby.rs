//! The lobby: xi_connect's view (TCP 54001) and data (TCP 54230) sessions.
//! On Windows FFXiMain speaks view and polcore/xiloader speak data; this client does both.

use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::net::{Ipv4Addr, TcpStream};

const MAGIC: &[u8; 4] = b"IXFF";
const NAME_LEN: usize = 16;
const CHAR_INFO_LEN: usize = 140;

#[derive(Clone, Debug)]
pub struct Character {
    pub content_id: u32,
    pub name: String,
    pub world: String,
    /// 0 hidden, 1 available, 2 disabled.
    pub status: u16,
    pub rename_required: bool,
    pub race: u16,
    pub main_job: u8,
    pub main_level: u8,
    pub sub_job: u8,
    pub face: u16,
    pub nation: u8,
    pub size: u8,
    pub zone: u16,
    /// face, head, body, hands, legs, feet, main, sub
    pub looks: [u16; 8],
}

impl Character {
    pub fn is_empty_slot(&self) -> bool {
        self.name.trim().is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct MapLogin {
    pub char_id: u32,
    pub name: String,
    pub map: (Ipv4Addr, u16),
    pub search: (Ipv4Addr, u16),
    /// The 20-byte session key the map server stored for this login.
    pub key: [u8; 20],
    /// The game login's account id and session hash (xi_profile's credentials).
    pub account: u32,
    pub hash: [u8; 16],
}

/// 0x21's make block; the name went with the 0x22 before it.
pub struct NewCharacter {
    pub race: u8,
    pub face: u8,
    pub size: u8,
    pub job: u8,
    pub nation: u8,
}

/// 0x21 body after the 28-byte header: race @48, job @50, nation @54, size @57, face @60 (LSB login_helpers).
fn make_body(c: &NewCharacter) -> Vec<u8> {
    let mut make = vec![0u8; 0x98 - 28];
    make[48 - 28] = c.race;
    make[50 - 28] = c.job;
    make[54 - 28] = c.nation;
    make[57 - 28] = c.size;
    make[60 - 28] = c.face;
    make
}

fn rename_body(content_id: u32, name: &str) -> Vec<u8> {
    let mut body = vec![0u8; 0x58 - 28];
    body[..4].copy_from_slice(&content_id.to_le_bytes());
    let n = name.as_bytes();
    body[8..8 + n.len().min(NAME_LEN - 1)].copy_from_slice(&n[..n.len().min(NAME_LEN - 1)]);
    body
}

fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd_name(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

pub fn error_text(code: u16) -> &'static str {
    match code {
        201 => "That character is already logged in. Wait a few minutes before reconnecting.",
        208 => "The world server is congested.",
        305 => "Unable to connect to the world server.",
        313 => "That character name is unavailable.",
        314 => "Failed to register with the name server.",
        316 => "Incorrect World Pass.",
        317 => "World Pass expired.",
        321 => "The character's parameters are incorrect.",
        331 => "The game data has been updated (client version rejected).",
        332 => "Could not connect to the lobby server.",
        339 => "A Gold World Pass is valid for your first character only.",
        _ => "Lobby error.",
    }
}

/// A lobby refusal (view 0x04): retail shows it as FFXI-3000 + code.
#[derive(Debug)]
pub struct Refused(pub u16);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} (FFXI-{})", error_text(self.0), 3000 + self.0 as u32)
    }
}

impl std::error::Error for Refused {}

/// A lobby result with the server's refusal split out: Err only when the link itself failed.
pub fn refusal<T>(r: Result<T>) -> Result<std::result::Result<T, u16>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match e.downcast_ref::<Refused>() {
            Some(c) => Ok(Err(c.0)),
            None => Err(e),
        },
    }
}

pub struct Lobby {
    view: TcpStream,
    data: TcpStream,
    hash: [u8; 16],
    account_id: u32,
    server_ip: Ipv4Addr,
    /// Mirrors the client's lobby key counter, which the server folds into the map key.
    key_counter: u32,
    last_request: u8,
    pub expansions: u32,
    pub features: u32,
    /// View replies read so far: the netbar's waits (client/netwait.rs).
    pub replies: Arc<AtomicU32>,
    /// The world table (0x23), kept for the creator's world step.
    pub worlds: Vec<(u32, String)>,
}

impl Lobby {
    pub fn connect(host: &str, view_port: u16, data_port: u16, account_id: u32, hash: [u8; 16], version: &str, replies: Arc<AtomicU32>) -> Result<Self> {
        let data = TcpStream::connect((host, data_port)).with_context(|| format!("connecting to lobby data {host}:{data_port}"))?;
        let view = TcpStream::connect((host, view_port)).with_context(|| format!("connecting to lobby view {host}:{view_port}"))?;
        let server_ip = match view.peer_addr()?.ip() {
            std::net::IpAddr::V4(ip) => ip,
            std::net::IpAddr::V6(ip) => ip.to_ipv4().unwrap_or(Ipv4Addr::LOCALHOST),
        };
        let mut lobby = Self { view, data, hash, account_id, server_ip, key_counter: 0, last_request: 0, expansions: 0, features: 0, replies, worlds: Vec::new() };

        // Registers the data socket under the session hash.
        let mut hello = [0u8; 28];
        hello[0] = 0xFE;
        hello[12..28].copy_from_slice(&hash);
        lobby.data.write_all(&hello)?;

        // 0x26 carries the client version string; 0x05 answers with the key and expansion bitmasks.
        let mut body = vec![0u8; 0x98 - 28];
        let ver = version.as_bytes();
        body[0x74 - 28..0x74 - 28 + ver.len().min(10)].copy_from_slice(&ver[..ver.len().min(10)]);
        lobby.send_view(0x26, &body)?;
        let reply = lobby.read_view(0x05)?;
        lobby.key_counter = rd_u32(&reply, 0x1C).wrapping_add(1);
        lobby.expansions = rd_u32(&reply, 0x20);
        lobby.features = rd_u32(&reply, 0x24);
        Ok(lobby)
    }

    fn send_view(&mut self, command: u8, body: &[u8]) -> Result<()> {
        let mut pkt = Vec::with_capacity(28 + body.len());
        pkt.extend_from_slice(&((28 + body.len()) as u32).to_le_bytes());
        pkt.extend_from_slice(MAGIC);
        pkt.extend_from_slice(&(command as u32).to_le_bytes());
        pkt.extend_from_slice(&self.hash);
        pkt.extend_from_slice(body);
        self.view.write_all(&pkt)?;
        self.last_request = command;
        if matches!(command, 0x07 | 0x14 | 0x1F | 0x21 | 0x22 | 0x24 | 0x28 | 0x2B) {
            self.key_counter = self.key_counter.wrapping_add(1);
        }
        Ok(())
    }

    /// Reads one view packet and checks it is `expect` (lobby errors become `Err`).
    fn read_view(&mut self, expect: u8) -> Result<Vec<u8>> {
        let mut size = [0u8; 4];
        self.view.read_exact(&mut size).context("the lobby closed the view connection")?;
        let size = u32::from_le_bytes(size) as usize;
        if !(28..=0x4000).contains(&size) {
            bail!("bad lobby packet size {size}");
        }
        let mut pkt = vec![0u8; size];
        pkt[..4].copy_from_slice(&(size as u32).to_le_bytes());
        self.view.read_exact(&mut pkt[4..])?;
        let command = pkt[8];
        self.replies.fetch_add(1, Ordering::Relaxed);
        if matches!(command, 0x03 | 0x0B | 0x20 | 0x23)
            || (command == 0x04 && matches!(self.last_request, 0x22 | 0x24 | 0x28 | 0x2B))
        {
            self.key_counter = self.key_counter.wrapping_add(1);
        }
        if command == 0x04 {
            return Err(Refused(rd_u16(&pkt, 32)).into());
        }
        if command != expect {
            bail!("expected lobby reply 0x{expect:02X}, got 0x{command:02X}");
        }
        Ok(pkt)
    }

    /// Reads one server-to-loader message from the data socket.
    fn read_data(&mut self) -> Result<Vec<u8>> {
        let mut first = [0u8; 1];
        self.data.read_exact(&mut first).context("the lobby closed the data connection")?;
        let len = match first[0] {
            0x01 | 0x02 => 5,
            0x03 => 0x148,
            other => bail!("unknown lobby data message 0x{other:02X}"),
        };
        let mut msg = vec![first[0]; len];
        self.data.read_exact(&mut msg[1..])?;
        Ok(msg)
    }

    pub fn characters(&mut self) -> Result<Vec<Character>> {
        self.send_view(0x1F, &[0u8; 0x24])?;
        let msg = self.read_data()?;
        if msg[0] != 0x01 {
            bail!("lobby asked for 0x{:02X} instead of the account id", msg[0]);
        }
        let mut ident = [0u8; 28];
        ident[0] = 0xA1;
        ident[1..5].copy_from_slice(&self.account_id.to_le_bytes());
        ident[5..9].copy_from_slice(&self.server_ip.octets());
        ident[12..28].copy_from_slice(&self.hash);
        self.data.write_all(&ident)?;

        let list = self.read_data()?;
        if list[0] != 0x03 {
            bail!("expected the character list, got 0x{:02X}", list[0]);
        }
        let pkt = self.read_view(0x20)?;
        let count = (rd_u32(&pkt, 28) as usize).min(16);
        Ok((0..count).map(|i| parse_char(&pkt[32 + i * CHAR_INFO_LEN..32 + (i + 1) * CHAR_INFO_LEN])).collect())
    }

    pub fn worlds(&mut self) -> Result<Vec<(u32, String)>> {
        self.send_view(0x24, &[0u8; 0x24])?;
        let pkt = self.read_view(0x23)?;
        let count = rd_u32(&pkt, 28) as usize;
        let worlds: Vec<(u32, String)> = (0..count)
            .filter_map(|i| {
                let o = 32 + i * 20;
                (o + 20 <= pkt.len()).then(|| (rd_u32(&pkt, o), rd_name(&pkt[o + 4..o + 20])))
            })
            .collect();
        self.worlds = worlds.clone();
        Ok(worlds)
    }

    /// View 0x22, the name check after the creator's world step (FUN_10116610 case 0x22): the name @0x20, the world's
    /// name @0x40 and the Gold World Pass @0x50 (LSB reads the name and the pass, keeps them for the 0x21 after it).
    pub fn check_name(&mut self, name: &str, world: &str, pass: &str) -> Result<()> {
        self.send_view(0x22, &check_body(name, world, pass))?;
        self.read_view(0x03)?;
        Ok(())
    }

    /// View 0x21, after a passed check_name.
    pub fn create_character(&mut self, c: &NewCharacter) -> Result<()> {
        self.send_view(0x21, &make_body(c))?;
        self.read_view(0x03)?;
        Ok(())
    }

    pub fn delete_character(&mut self, ch: &Character) -> Result<()> {
        let mut body = vec![0u8; 0x34 - 28];
        body[..4].copy_from_slice(&ch.content_id.to_le_bytes());
        body[4..8].copy_from_slice(&(ch.content_id & 0xFFFF).to_le_bytes());
        self.send_view(0x14, &body)?;
        self.read_view(0x03)?;
        Ok(())
    }

    /// Renames a character flagged with renamef (LSB view 0x28: character id @28, name @36; reply 0x03).
    pub fn rename(&mut self, ch: &Character, name: &str) -> Result<()> {
        self.send_view(0x28, &rename_body(ch.content_id, name))?;
        self.read_view(0x03)?;
        Ok(())
    }

    /// Picks a character; the lobby answers with where its zone runs.
    pub fn select(mut self, ch: &Character) -> Result<MapLogin> {
        let mut body = vec![0u8; 0x58 - 28];
        body[..4].copy_from_slice(&ch.content_id.to_le_bytes());
        let name = ch.name.as_bytes();
        body[8..8 + name.len().min(NAME_LEN - 1)].copy_from_slice(&name[..name.len().min(NAME_LEN - 1)]);
        self.send_view(0x07, &body)?;

        let msg = self.read_data()?;
        if msg[0] != 0x02 {
            bail!("lobby asked for 0x{:02X} instead of the key", msg[0]);
        }
        // xi_connect derives the real key itself; these bytes are xiloader's placeholders.
        let mut key_msg = [0u8; 28];
        key_msg[0] = 0xA2;
        key_msg[17..21].copy_from_slice(&[0x58, 0xE0, 0x5D, 0xAD]);
        self.data.write_all(&key_msg)?;

        // The server steps the counter three more times: once for 0x0B, twice at zone connect.
        let mut key = [0u8; 20];
        key[..16].copy_from_slice(&Md5::digest(self.hash));
        key[16..].copy_from_slice(&self.key_counter.wrapping_add(3).to_le_bytes());

        let pkt = self.read_view(0x0B)?;
        let ip = |o| Ipv4Addr::from(rd_u32(&pkt, o).to_le_bytes());
        Ok(MapLogin {
            char_id: rd_u32(&pkt, 0x1C),
            name: rd_name(&pkt[0x24..0x34]),
            map: (ip(0x38), rd_u32(&pkt, 0x3C) as u16),
            search: (ip(0x40), rd_u32(&pkt, 0x44) as u16),
            key,
            account: self.account_id,
            hash: self.hash,
        })
    }
}

/// 0x22's body (offsets from the packet start less the 28-byte header): name @0x20, world @0x40, pass @0x50, 15 + NUL.
fn check_body(name: &str, world: &str, pass: &str) -> Vec<u8> {
    let mut b = vec![0u8; 0x60 - 28];
    for (at, s) in [(0x20, name), (0x40, world), (0x50, pass)] {
        let s = s.as_bytes();
        let n = s.len().min(NAME_LEN - 1);
        b[at - 28..at - 28 + n].copy_from_slice(&s[..n]);
    }
    b
}

fn parse_char(b: &[u8]) -> Character {
    let info = &b[44..];
    let mut looks = [0u16; 8];
    for (i, l) in looks.iter_mut().enumerate() {
        *l = rd_u16(info, 12 + i * 2);
    }
    Character {
        content_id: rd_u32(b, 0),
        status: rd_u16(b, 8),
        rename_required: b[10] & 1 != 0,
        name: rd_name(&b[12..28]),
        world: rd_name(&b[28..44]),
        race: rd_u16(info, 0),
        main_job: info[2],
        sub_job: info[3],
        face: rd_u16(info, 4),
        nation: info[6],
        size: info[9],
        zone: info[28] as u16 | ((info[35] as u16 & 1) << 8),
        main_level: info[29],
        looks,
    }
}

#[cfg(test)]
mod create_tests {
    use super::*;

    #[test]
    fn size_lands_where_lsb_reads_it() {
        // LSB: m_look.size = ref<uint8>(buf, 57), buf from the packet start.
        for size in 0..3u8 {
            let mut pkt = vec![0u8; 28];
            pkt.extend(make_body(&NewCharacter { race: 2, face: 5, size, job: 3, nation: 1 }));
            assert_eq!((pkt[57], pkt[48], pkt[50], pkt[54], pkt[60]), (size, 2, 3, 1, 5));
        }
    }

    /// LSB view 0x22: the name @32 and the Gold World Pass @0x50 (15 each); the world @0x40 it ignores.
    #[test]
    fn name_check_lands_where_lsb_reads_it() {
        let mut pkt = vec![0u8; 28];
        pkt.extend(check_body("Abcde", "Nameless", "1234567890"));
        assert_eq!((pkt.len(), &pkt[32..37], &pkt[0x40..0x48], &pkt[0x50..0x5a]), (0x60, &b"Abcde"[..], &b"Nameless"[..], &b"1234567890"[..]));
    }

    #[test]
    fn rename_lands_where_lsb_reads_it() {
        // LSB 0x28: charid = ref<uint32>(buf, 28), name from buf + 36 (15 bytes).
        let mut pkt = vec![0u8; 28];
        pkt.extend(rename_body(0x0102_0304, "Newname"));
        assert_eq!(u32::from_le_bytes(pkt[28..32].try_into().unwrap()), 0x0102_0304);
        assert_eq!(&pkt[36..44], b"Newname\0");
    }
}
