//! The lobby: FFXiMain's view link (TCP 54001), as the game builds it (FUN_10116610), in two dialects.
//! LSB (xi_connect): a session hash in each header, plus the data link (TCP 54230) that xiloader speaks there.
//! PlayOnline (FFXiMain's `-net 3` path): one link, an MD5 of the packet in each header, the session from net/pol.

use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

const MAGIC: &[u8; 4] = b"IXFF";
const NAME_LEN: usize = 16;
const CHAR_INFO_LEN: usize = 140;
/// FFXiMain's largest accepted reply (FUN_1026be74).
const MAX_REPLY: usize = 0xB68;

/// Who vouches for the lobby session.
#[derive(Clone)]
pub enum Session {
    /// xi_connect's login (net/auth.rs): the account id and session hash.
    Lsb { account_id: u32, hash: [u8; 16] },
    /// A PlayOnline sign-in: polcore's authCode (table 0x3A8) and 16-byte value (0x3EB).
    PlayOnline { auth_code: [u8; 64], k16: [u8; 16] },
}

impl Session {
    /// The 16 bytes the key fields and the map key hash: polcore's value; on LSB xi_profile gives polcore MD5(hash).
    fn k16(&self) -> [u8; 16] {
        match self {
            Session::Lsb { hash, .. } => Md5::digest(hash).into(),
            Session::PlayOnline { k16, .. } => *k16,
        }
    }

    fn auth_code(&self) -> [u8; 64] {
        match self {
            Session::Lsb { .. } => [0; 64],
            Session::PlayOnline { auth_code, .. } => *auth_code,
        }
    }
}

/// FUN_100185a0 over FFXiMain.dll (period 0x13): each signed byte shifted left by 4 * (i mod 20), summed. 0x07 sends it.
pub fn ffximain_checksum(dll: &[u8]) -> u32 {
    dll.iter().enumerate().fold(0u32, |s, (i, &b)| s.wrapping_add(((b as i8 as i32) << ((i % 20) * 4 & 31)) as u32))
}

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
    /// The 16-byte value and the key counter at world entry; the map's Blowfish key is their MD5.
    pub key: [u8; 20],
    /// The LSB login's account id and session hash (xi_profile's credentials); none for PlayOnline.
    pub lsb: Option<(u32, [u8; 16])>,
    /// PlayOnline: the chat link, kept open while the game runs.
    pub pol: Option<Arc<super::pol::Link>>,
}

/// The creator's choices for 0x21's make block (the 0x20 record's +0x2C).
pub struct NewCharacter {
    pub race: u8,
    pub face: u8,
    pub size: u8,
    pub job: u8,
    pub nation: u8,
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
fn put(b: &mut [u8], at: usize, v: &[u8]) {
    b[at..at + v.len()].copy_from_slice(v);
}
/// A name field: 15 bytes and a NUL.
fn put_name(b: &mut [u8], at: usize, s: &str) {
    let s = s.as_bytes();
    put(b, at, &s[..s.len().min(NAME_LEN - 1)]);
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

/// Where each request carries its 16-byte key (FUN_10116610); 0x26 has none.
fn key_at(command: u8) -> Option<usize> {
    match command {
        0x1F | 0x24 => Some(0x1C),
        0x21 => Some(0x20),
        0x14 => Some(0x24),
        0x22 => Some(0x30),
        0x07 | 0x28 | 0x2B => Some(0x34),
        _ => None,
    }
}

/// A request of `size` bytes with the header's size and magic; the command, key and header MD5 go in at send.
fn packet(size: usize) -> Vec<u8> {
    let mut p = vec![0u8; size];
    put(&mut p, 0, &(size as u32).to_le_bytes());
    put(&mut p, 4, MAGIC);
    p
}

pub struct Lobby {
    view: TcpStream,
    /// LSB's data link (xiloader's part); none for PlayOnline.
    data: Option<TcpStream>,
    session: Session,
    k16: [u8; 16],
    /// FFXiMain.dll's checksum, for 0x07.
    checksum: u32,
    server_ip: Ipv4Addr,
    /// The game's key counter (data+0x138): each key-carrying request hashes it, then it steps (FUN_10116d30).
    key_counter: u32,
    last_request: u8,
    pub expansions: u32,
    pub features: u32,
    /// View replies read so far: the netbar's waits (client/netwait.rs).
    pub replies: Arc<AtomicU32>,
    /// The world table (0x23), kept for the creator's world step.
    pub worlds: Vec<(u32, String)>,
    /// The last 0x20's records, as sent: requests take their ids from them.
    records: Vec<[u8; CHAR_INFO_LEN]>,
    /// PlayOnline: the signed-in member (net/pol).
    pub pol: Option<super::pol::Pol>,
}

impl Lobby {
    /// `host`: the lobby (LSB's server, or ffxi00.pol.com); `data_port` is LSB's only. `content`: the installed
    /// ROMs (FUN_101c3400, dat::installed).
    #[allow(clippy::too_many_arguments)]
    pub fn connect(host: &str, view_port: u16, data_port: u16, session: Session, ffximain: &[u8], version: &str, region: u32, content: u32, replies: Arc<AtomicU32>) -> Result<Self> {
        let data = match session {
            Session::Lsb { .. } => Some(TcpStream::connect((host, data_port)).with_context(|| format!("connecting to lobby data {host}:{data_port}"))?),
            Session::PlayOnline { .. } => None,
        };
        let view = TcpStream::connect((host, view_port)).with_context(|| format!("connecting to the lobby {host}:{view_port}"))?;
        let server_ip = match view.peer_addr()?.ip() {
            std::net::IpAddr::V4(ip) => ip,
            std::net::IpAddr::V6(ip) => ip.to_ipv4().unwrap_or(Ipv4Addr::LOCALHOST),
        };
        let k16 = session.k16();
        let mut lobby = Self {
            view,
            data,
            session,
            k16,
            checksum: ffximain_checksum(ffximain),
            server_ip,
            key_counter: 0,
            last_request: 0,
            expansions: 0,
            features: 0,
            replies,
            worlds: Vec::new(),
            records: Vec::new(),
            pol: None,
        };

        // Registers the data socket under the session hash.
        if let (Some(d), Session::Lsb { hash, .. }) = (lobby.data.as_mut(), &lobby.session) {
            let mut hello = [0u8; 28];
            hello[0] = 0xFE;
            hello[12..28].copy_from_slice(hash);
            d.write_all(&hello)?;
        }

        // 0x26: the authCode, the region (3 Japanese, else 5), the version, the installed content; 0x05 answers with
        // the counter seed and the expansion bitmasks.
        let mut p = packet(0x98);
        put(&mut p, 0x2C, &region.to_le_bytes());
        put(&mut p, 0x34, &lobby.session.auth_code());
        put_name(&mut p, 0x74, version);
        put(&mut p, 0x84, &content.to_le_bytes());
        lobby.send_view(0x26, p)?;
        let reply = lobby.read_view(0x05, 0x28)?;
        lobby.key_counter = rd_u32(&reply, 0x1C).wrapping_add(1);
        lobby.expansions = rd_u32(&reply, 0x20);
        lobby.features = rd_u32(&reply, 0x24);
        Ok(lobby)
    }

    fn playonline(&self) -> bool {
        matches!(self.session, Session::PlayOnline { .. })
    }

    /// MD5(K16 ‖ counter) (FUN_10116d80), then the counter steps.
    fn next_key(&mut self) -> [u8; 16] {
        let mut h = Md5::new();
        h.update(self.k16);
        h.update(self.key_counter.to_le_bytes());
        self.key_counter = self.key_counter.wrapping_add(1);
        h.finalize().into()
    }

    fn send_view(&mut self, command: u8, mut pkt: Vec<u8>) -> Result<()> {
        put(&mut pkt, 8, &(command as u32).to_le_bytes());
        if let Some(at) = key_at(command) {
            let key = self.next_key();
            put(&mut pkt, at, &key);
        }
        // FFXiMain: the packet's MD5 with these 16 bytes zero (FUN_10116c90); LSB reads its session hash there.
        let tag: [u8; 16] = match &self.session {
            Session::Lsb { hash, .. } => *hash,
            Session::PlayOnline { .. } => Md5::digest(&pkt).into(),
        };
        put(&mut pkt, 12, &tag);
        self.view.write_all(&pkt)?;
        self.last_request = command;
        Ok(())
    }

    /// Reads one view packet and checks it is `expect` and at least `min_len` bytes, the end of the last field its
    /// caller reads (lobby errors become `Err`).
    fn read_view(&mut self, expect: u8, min_len: usize) -> Result<Vec<u8>> {
        let mut size = [0u8; 4];
        self.view.read_exact(&mut size).context("the lobby closed the view connection")?;
        let size = u32::from_le_bytes(size) as usize;
        if !(28..=MAX_REPLY).contains(&size) {
            bail!("bad lobby packet size {size}");
        }
        let mut pkt = vec![0u8; size];
        pkt[..4].copy_from_slice(&(size as u32).to_le_bytes());
        self.view.read_exact(&mut pkt[4..])?;
        if &pkt[4..8] != MAGIC {
            bail!("not a lobby packet");
        }
        // FFXiMain checks every reply's MD5 (FUN_10116540).
        if self.playonline() {
            let mut zeroed = pkt.clone();
            zeroed[12..28].fill(0);
            if Md5::digest(&zeroed)[..] != pkt[12..28] {
                log::warn!("lobby reply failed its checksum: {}", hex::encode(&pkt));
                bail!("lobby reply failed its checksum");
            }
        }
        let command = pkt[8];
        log::info!("lobby: reply 0x{command:02X} ({size} bytes) to 0x{:02X}", self.last_request);
        self.replies.fetch_add(1, Ordering::Relaxed);
        if matches!(command, 0x03 | 0x0B | 0x20 | 0x23)
            || (command == 0x04 && matches!(self.last_request, 0x22 | 0x24 | 0x28 | 0x2B))
        {
            self.key_counter = self.key_counter.wrapping_add(1);
        }
        if command == 0x04 {
            // The code is the u32 @0x20.
            let code = pkt.get(32..36).context("the lobby's refusal is too short to carry its code")?;
            return Err(Refused(u32::from_le_bytes(code.try_into().unwrap()) as u16).into());
        }
        if command != expect {
            bail!("expected lobby reply 0x{expect:02X}, got 0x{command:02X}");
        }
        if size < min_len {
            bail!("lobby reply 0x{command:02X} is {size} bytes, too short (needs {min_len})");
        }
        Ok(pkt)
    }

    /// Reads one server-to-loader message from LSB's data socket.
    fn read_data(&mut self) -> Result<Vec<u8>> {
        let data = self.data.as_mut().context("no lobby data link")?;
        let mut first = [0u8; 1];
        data.read_exact(&mut first).context("the lobby closed the data connection")?;
        let len = match first[0] {
            0x01 | 0x02 => 5,
            0x03 => 0x148,
            other => bail!("unknown lobby data message 0x{other:02X}"),
        };
        let mut msg = vec![first[0]; len];
        data.read_exact(&mut msg[1..])?;
        Ok(msg)
    }

    /// The 0x20 record of a content id.
    fn record(&self, content_id: u32) -> Option<&[u8; CHAR_INFO_LEN]> {
        self.records.iter().find(|r| rd_u32(&r[..], 0) == content_id)
    }

    /// The first free slot (status 1, name " ": FUN_1023df20), whose content id a new character takes.
    fn free_slot(&self) -> u32 {
        self.records.iter().find(|r| rd_u16(&r[..], 8) == 1 && r[12] == b' ' && r[13] == 0).map_or(0, |r| rd_u32(&r[..], 0))
    }

    /// The record's u16 at +4 (ffxi_id_world), which 0x07 / 0x14 / 0x28 carry @0x20.
    fn world_id(&self, content_id: u32) -> u32 {
        self.record(content_id).map_or(content_id & 0xFFFF, |r| rd_u16(&r[..], 4) as u32)
    }

    pub fn characters(&mut self) -> Result<Vec<Character>> {
        self.send_view(0x1F, packet(0x2C))?;
        if let Session::Lsb { account_id, hash } = self.session.clone() {
            let msg = self.read_data()?;
            if msg[0] != 0x01 {
                bail!("lobby asked for 0x{:02X} instead of the account id", msg[0]);
            }
            let mut ident = [0u8; 28];
            ident[0] = 0xA1;
            ident[1..5].copy_from_slice(&account_id.to_le_bytes());
            ident[5..9].copy_from_slice(&self.server_ip.octets());
            ident[12..28].copy_from_slice(&hash);
            self.data.as_mut().unwrap().write_all(&ident)?;
            let list = self.read_data()?;
            if list[0] != 0x03 {
                bail!("expected the character list, got 0x{:02X}", list[0]);
            }
        }
        let pkt = self.read_view(0x20, 32)?;
        let count = (rd_u32(&pkt, 28) as usize).min(16);
        self.records = (0..count).filter_map(|i| pkt.get(32 + i * CHAR_INFO_LEN..32 + (i + 1) * CHAR_INFO_LEN)).map(|r| r.try_into().unwrap()).collect();
        Ok(self.records.iter().map(|r| parse_char(r)).collect())
    }

    pub fn worlds(&mut self) -> Result<Vec<(u32, String)>> {
        self.send_view(0x24, packet(0x2C))?;
        let pkt = self.read_view(0x23, 32)?;
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

    /// View 0x22, the name check after the creator's world step: the free slot @0x1C, the name @0x20, the world's
    /// name @0x40 and the Gold World Pass @0x50 (LSB reads the name and the pass, keeps them for the 0x21 after it).
    pub fn check_name(&mut self, name: &str, world: &str, pass: &str) -> Result<()> {
        let p = check_body(self.free_slot(), name, world, pass);
        self.send_view(0x22, p)?;
        self.read_view(0x03, 28)?;
        Ok(())
    }

    /// View 0x21, after a passed check_name.
    pub fn create_character(&mut self, c: &NewCharacter) -> Result<()> {
        let p = make_body(self.free_slot(), c);
        self.send_view(0x21, p)?;
        self.read_view(0x03, 28)?;
        Ok(())
    }

    pub fn delete_character(&mut self, ch: &Character) -> Result<()> {
        let mut p = packet(0x34);
        put(&mut p, 0x1C, &ch.content_id.to_le_bytes());
        put(&mut p, 0x20, &self.world_id(ch.content_id).to_le_bytes());
        self.send_view(0x14, p)?;
        self.read_view(0x03, 28)?;
        Ok(())
    }

    /// Renames a character flagged with renamef (0x28: content id @0x1C, its world id @0x20, the name @0x24).
    pub fn rename(&mut self, ch: &Character, name: &str) -> Result<()> {
        let p = rename_body(ch.content_id, self.world_id(ch.content_id), name);
        self.send_view(0x28, p)?;
        self.read_view(0x03, 28)?;
        Ok(())
    }

    /// Picks a character; the lobby answers with where its zone runs.
    pub fn select(mut self, ch: &Character) -> Result<MapLogin> {
        let rec = *self.record(ch.content_id).context("that character is not in the lobby's list")?;
        let mut p = packet(0x58);
        put(&mut p, 0x1C, &ch.content_id.to_le_bytes());
        put(&mut p, 0x20, &(rd_u16(&rec, 4) as u32).to_le_bytes());
        put(&mut p, 0x24, &rec[12..12 + NAME_LEN - 1]);
        put(&mut p, 0x44, &3u32.to_le_bytes());
        // @0x48: MD5(authCode ‖ this request's key ‖ 3 ‖ FFXiMain.dll checksum ‖ content id); the key goes in first.
        let key = self.next_key();
        put(&mut p, 0x34, &key);
        let mut h = Md5::new();
        h.update(self.session.auth_code());
        h.update(key);
        h.update(3u32.to_le_bytes());
        h.update(self.checksum.to_le_bytes());
        h.update(ch.content_id.to_le_bytes());
        put(&mut p, 0x48, &h.finalize());
        // The key is in: send without drawing another one.
        put(&mut p, 8, &7u32.to_le_bytes());
        let tag: [u8; 16] = match &self.session {
            Session::Lsb { hash, .. } => *hash,
            Session::PlayOnline { .. } => Md5::digest(&p).into(),
        };
        put(&mut p, 12, &tag);
        self.view.write_all(&p)?;
        self.last_request = 0x07;

        if self.data.is_some() {
            let msg = self.read_data()?;
            if msg[0] != 0x02 {
                bail!("lobby asked for 0x{:02X} instead of the key", msg[0]);
            }
            // xi_connect derives the real key itself; these bytes are xiloader's placeholders.
            let mut key_msg = [0u8; 28];
            key_msg[0] = 0xA2;
            key_msg[17..21].copy_from_slice(&[0x58, 0xE0, 0x5D, 0xAD]);
            self.data.as_mut().unwrap().write_all(&key_msg)?;
        }

        let pkt = self.read_view(0x0B, 0x48)?;
        let ip = |o| Ipv4Addr::from(rd_u32(&pkt, o).to_le_bytes());
        // The lobby closes the link after 0x0B and the game waits for it (FUN_100ee160 sub 2), then makes the character
        // PlayOnline's active one (subs 0xE..0x15); a failure there ends the login.
        if self.playonline() {
            self.view.set_read_timeout(Some(Duration::from_secs(10)))?;
            let mut rest = Vec::new();
            // Bounded: anything after 0x0B is ignored, and a lobby that keeps sending can't grow it without limit.
            let _ = (&mut self.view).take(MAX_REPLY as u64).read_to_end(&mut rest);
            let world_key = rd_u16(&rec, 4) as u32 | (rd_u16(&rec, 6) as u32) << 16 | (rec[0xB] as u32) << 24;
            if let Some(p) = self.pol.as_mut() {
                p.profile.select(ch.content_id, world_key)?;
            }
        }
        // World entry hashes the counter after two more steps (step 6: FUN_101158e0 twice); 0x0B made one.
        let mut key20 = [0u8; 20];
        key20[..16].copy_from_slice(&self.k16);
        key20[16..].copy_from_slice(&self.key_counter.wrapping_add(2).to_le_bytes());
        Ok(MapLogin {
            // The record's (FUN_100ef1a0): +0xB << 16 | u16 +4.
            char_id: (rec[0xB] as u32) << 16 | rd_u16(&rec, 4) as u32,
            name: rd_name(&rec[12..28]),
            map: (ip(0x38), rd_u32(&pkt, 0x3C) as u16),
            search: (ip(0x40), rd_u32(&pkt, 0x44) as u16),
            key: key20,
            lsb: match self.session {
                Session::Lsb { account_id, hash } => Some((account_id, hash)),
                Session::PlayOnline { .. } => None,
            },
            pol: self.pol.take().map(|p| Arc::new(p.into_link())),
        })
    }
}

/// 0x22: the free slot @0x1C, name @0x20, world @0x40, pass @0x50 (15 + NUL each); the key @0x30 at send.
fn check_body(slot: u32, name: &str, world: &str, pass: &str) -> Vec<u8> {
    let mut p = packet(0x60);
    put(&mut p, 0x1C, &slot.to_le_bytes());
    put_name(&mut p, 0x20, name);
    put_name(&mut p, 0x40, world);
    put_name(&mut p, 0x50, pass);
    p
}

/// 0x21: the free slot @0x1C, the make block @0x30: race @0x30, job @0x32, nation @0x36, size @0x39, face @0x3C
/// (LSB login_helpers reads them there).
fn make_body(slot: u32, c: &NewCharacter) -> Vec<u8> {
    let mut p = packet(0x90);
    put(&mut p, 0x1C, &slot.to_le_bytes());
    p[0x30] = c.race;
    p[0x32] = c.job;
    p[0x36] = c.nation;
    p[0x39] = c.size;
    p[0x3C] = c.face;
    p
}

fn rename_body(content_id: u32, world_id: u32, name: &str) -> Vec<u8> {
    let mut p = packet(0x44);
    put(&mut p, 0x1C, &content_id.to_le_bytes());
    put(&mut p, 0x20, &world_id.to_le_bytes());
    put_name(&mut p, 0x24, name);
    p
}

fn parse_char(b: &[u8]) -> Character {
    let info = &b[44..];
    // Model ids 1..7 may carry their slot in the top nibble (0x1000 head .. 0x7000 sub), as the zone's GrapIDTbl
    // does (look_from); LSB's lobby sends them bare.
    let mut looks = [0u16; 8];
    for (i, l) in looks.iter_mut().enumerate() {
        let id = rd_u16(info, 12 + i * 2);
        *l = if i == 0 { id } else { id & 0x0FFF };
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
            let pkt = make_body(7, &NewCharacter { race: 2, face: 5, size, job: 3, nation: 1 });
            assert_eq!((pkt.len(), pkt[57], pkt[48], pkt[50], pkt[54], pkt[60], pkt[0x1C]), (0x90, size, 2, 3, 1, 5, 7));
        }
    }

    /// LSB view 0x22: the name @32 and the Gold World Pass @0x50 (15 each); the world @0x40 it ignores.
    #[test]
    fn name_check_lands_where_lsb_reads_it() {
        let pkt = check_body(9, "Abcde", "Nameless", "1234567890");
        assert_eq!((pkt.len(), &pkt[32..37], &pkt[0x40..0x48], &pkt[0x50..0x5a], pkt[0x1C]), (0x60, &b"Abcde"[..], &b"Nameless"[..], &b"1234567890"[..], 9));
    }

    #[test]
    fn rename_lands_where_lsb_reads_it() {
        // LSB 0x28: charid = ref<uint32>(buf, 28), name from buf + 36 (15 bytes).
        let pkt = rename_body(0x0102_0304, 0x0304, "Newname");
        assert_eq!(u32::from_le_bytes(pkt[28..32].try_into().unwrap()), 0x0102_0304);
        assert_eq!(&pkt[36..44], b"Newname\0");
        assert_eq!(pkt.len(), 0x44);
    }

    /// A lobby as FFXiMain's -net 3 path expects it (MODLOG "PlayOnline sign-in"): every packet's MD5 checked both
    /// ways, the authCode in 0x26, MD5(K16 ‖ counter) in each key field, the 0x07 hash; the map key two steps past
    /// 0x0B.
    #[test]
    fn playonline_dialect_against_a_spec_lobby() {
        use std::net::TcpListener;
        let k16 = [0x42u8; 16];
        let mut auth = [0u8; 64];
        auth[..0x34].fill(0xa5);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let md5_ok = |p: &[u8]| {
                let mut z = p.to_vec();
                z[12..28].fill(0);
                Md5::digest(&z)[..] == p[12..28]
            };
            let read = |s: &mut TcpStream| {
                let mut n = [0u8; 4];
                s.read_exact(&mut n).unwrap();
                let mut p = vec![0u8; u32::from_le_bytes(n) as usize];
                p[..4].copy_from_slice(&n);
                s.read_exact(&mut p[4..]).unwrap();
                p
            };
            let reply = |s: &mut TcpStream, cmd: u8, body: &[u8]| {
                let mut p = packet(28 + body.len());
                p[8] = cmd;
                p[28..].copy_from_slice(body);
                let d = Md5::digest(&p);
                p[12..28].copy_from_slice(&d);
                s.write_all(&p).unwrap();
            };
            let key = |c: u32| -> [u8; 16] {
                let mut m = Md5::new();
                m.update(k16);
                m.update(c.to_le_bytes());
                m.finalize().into()
            };
            let p = read(&mut s);
            assert!(md5_ok(&p) && p[8] == 0x26 && p.len() == 0x98 && p[0x34..0x74] == auth[..] && &p[0x74..0x7e] == b"30261006_0");
            let mut b = [0u8; 12];
            b[..4].copy_from_slice(&99u32.to_le_bytes());
            reply(&mut s, 0x05, &b);
            let mut c = 100;
            let p = read(&mut s);
            assert!(md5_ok(&p) && p[8] == 0x1F && p.len() == 0x2C && p[0x1C..0x2C] == key(c));
            c += 1;
            let mut list = vec![0u8; 4 + CHAR_INFO_LEN];
            list[0] = 1;
            list[4..8].copy_from_slice(&7u32.to_le_bytes());
            list[8..10].copy_from_slice(&0x1234u16.to_le_bytes());
            list[4 + 0xB] = 2;
            list[4 + 12..4 + 16].copy_from_slice(b"Nine");
            reply(&mut s, 0x20, &list);
            c += 1;
            let p = read(&mut s);
            assert!(md5_ok(&p) && p[8] == 0x07 && p[0x34..0x44] == key(c));
            let mut m = Md5::new();
            m.update(auth);
            m.update(key(c));
            m.update(3u32.to_le_bytes());
            m.update(ffximain_checksum(b"dll").to_le_bytes());
            m.update(7u32.to_le_bytes());
            assert!(p[0x48..0x58] == m.finalize()[..] && p[0x20..0x24] == 0x1234u32.to_le_bytes());
            let mut b = vec![0u8; 0x48 - 28];
            b[0x3C - 28..0x3E - 28].copy_from_slice(&54230u16.to_le_bytes());
            reply(&mut s, 0x0B, &b);
            c
        });
        let mut l = Lobby::connect("127.0.0.1", port, 0, Session::PlayOnline { auth_code: auth, k16 }, b"dll", "30261006_0", 5, 0xfff, Default::default()).unwrap();
        let chars = l.characters().unwrap();
        let m = l.select(&chars[0]).unwrap();
        // The 0x07 hashed c; 0x0B makes it c + 2 and world entry c + 4 (MODLOG "PlayOnline sign-in").
        let c = server.join().unwrap();
        assert_eq!((m.char_id, m.map.1, m.key[..16].to_vec(), u32::from_le_bytes(m.key[16..].try_into().unwrap())), (2 << 16 | 0x1234, 54230, k16.to_vec(), c + 4));
    }

    /// A lobby whose replies stop at the header: each one is an error, not a panic (issue #80).
    #[test]
    fn short_replies_are_errors() {
        use std::net::TcpListener;
        // Answers 0x26 with `first` (header and body), then each later request with the next reply.
        let lobby = |replies: Vec<(u8, Vec<u8>)>| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                for (cmd, body) in replies {
                    let mut n = [0u8; 4];
                    if s.read_exact(&mut n).is_err() {
                        return;
                    }
                    let mut p = vec![0u8; u32::from_le_bytes(n) as usize - 4];
                    s.read_exact(&mut p).unwrap();
                    let mut r = packet(28 + body.len());
                    r[8] = cmd;
                    r[28..].copy_from_slice(&body);
                    let d = Md5::digest(&r);
                    r[12..28].copy_from_slice(&d);
                    s.write_all(&r).unwrap();
                }
            });
            let session = Session::PlayOnline { auth_code: [0; 64], k16: [0; 16] };
            (Lobby::connect("127.0.0.1", port, 0, session, b"dll", "30261006_0", 5, 0, Default::default()), server)
        };
        let ok05 = (0x05, vec![0u8; 12]);
        // 0x05 without its counter seed and masks.
        let (l, s) = lobby(vec![(0x05, vec![])]);
        assert!(l.is_err());
        s.join().unwrap();
        // A refusal without its code, to 0x26 and to 0x1F.
        let (l, s) = lobby(vec![(0x04, vec![0; 2])]);
        assert!(l.err().unwrap().downcast_ref::<Refused>().is_none());
        s.join().unwrap();
        let (l, s) = lobby(vec![ok05.clone(), (0x04, vec![])]);
        assert!(l.unwrap().characters().is_err());
        s.join().unwrap();
        // A whole refusal still carries its code.
        let (l, s) = lobby(vec![ok05.clone(), (0x04, 313u32.to_le_bytes().repeat(2))]);
        assert_eq!(refusal(l.unwrap().characters()).unwrap().unwrap_err(), 313);
        s.join().unwrap();
        // 0x20 and 0x23 without their counts.
        let (l, s) = lobby(vec![ok05.clone(), (0x20, vec![])]);
        assert!(l.unwrap().characters().is_err());
        s.join().unwrap();
        let (l, s) = lobby(vec![ok05.clone(), (0x23, vec![])]);
        assert!(l.unwrap().worlds().is_err());
        s.join().unwrap();
        // 0x0B without the search server.
        let mut list = vec![0u8; 4 + CHAR_INFO_LEN];
        list[0] = 1;
        list[4..8].copy_from_slice(&7u32.to_le_bytes());
        list[4 + 12..4 + 16].copy_from_slice(b"Nine");
        let (l, s) = lobby(vec![ok05, (0x20, list), (0x0B, vec![0; 0x40 - 28])]);
        let mut l = l.unwrap();
        let chars = l.characters().unwrap();
        assert!(l.select(&chars[0]).is_err());
        s.join().unwrap();
    }

    /// A lobby record whose model ids carry their slot nibble parses to bare ids every part file resolves from.
    #[test]
    fn looks_lose_their_slot_nibble() {
        let mut rec = [0u8; CHAR_INFO_LEN];
        rec[12..16].copy_from_slice(b"Nine");
        let info = 44;
        rec[info..info + 2].copy_from_slice(&3u16.to_le_bytes());
        for (i, id) in [0u16, 0x1130, 0x2130, 0x3130, 0x4130, 0x5130, 0x6010, 0x7000].iter().enumerate() {
            rec[info + 12 + i * 2..info + 14 + i * 2].copy_from_slice(&id.to_le_bytes());
        }
        let c = parse_char(&rec);
        assert_eq!(c.looks, [0, 0x130, 0x130, 0x130, 0x130, 0x130, 0x010, 0]);
        let l = &c.looks;
        let look = crate::dat::character::Look { race: c.race as u8, face: c.face, head: l[1], body: l[2], hands: l[3], legs: l[4], feet: l[5], main: l[6], sub: l[7], range: 0 };
        assert!(crate::dat::character::file_ids(&look).is_ok());
    }

    #[test]
    fn checksum_matches_the_games() {
        // FUN_100185a0: signed bytes, the shift cycling 0, 4, .. 76 (masked to 31).
        let d: Vec<u8> = (0..45u32).map(|i| (i * 37 + 200) as u8).collect();
        let mut s = 0u32;
        for (i, &b) in d.iter().enumerate() {
            s = s.wrapping_add(((b as i8 as i32) << ((i as u32 % 20) * 4 & 31)) as u32);
        }
        assert_eq!(ffximain_checksum(&d), s);
        // The installed 30261006_0 (its value in MODLOG "PlayOnline sign-in"), when present.
        let dll = crate::config::Config::load().ffxi_dir().join("FFXiMain.dll");
        if let Ok(b) = std::fs::read(dll)
            && b.len() == 2_901_584
        {
            assert_eq!(ffximain_checksum(&b), 0x43FA_0718);
        }
    }
}
