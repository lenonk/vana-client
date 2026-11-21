//! Game file updates from the PlayOnline patch server, as the Viewer's version-up did (polcore.dll / app.dll).
//! Protocol facts from Kuluu's reverse engineering of viewer 1.18.15e (research notes, not its code; MODLOG
//! "Game file updates"): POLP frames on TCP to pc001.pol.com:53001 for FFXI, no login.
//!
//! Flow: a version check (frame 7 / 8) gives the server's version; a hello (1) answers with the whole manifest
//! (2: status byte, then `.slc`); file requests (3) answer in chunks (4). Each manifest file lists its versions
//! with a signature (length, signed byte sum, MD5 word); a local file matching an older version takes the `.olc`
//! deltas when they are smaller than the `.slc` of the current one.
//!
//! The rules below were read from the Viewer's own patch client (app.dll 1.18.13, the state machine FUN_10018bca;
//! polcore.dll holds an older copy, FUN_1002d22e, without zlib) and checked against the live server (MODLOG
//! "PlayOnline patch client"): the version check carries the version applied here and its reply names the host
//! to download from; with a version applied, only files with a newer line are checked, and only then are deltas
//! used; every new file is staged as `<name>.tmp2` and all of them are renamed into place at the end, so a
//! stopped update leaves the old files whole and is picked up from the staged ones.

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HOST: &str = "pc001.pol.com";
pub const PORT: u16 = 53001;
/// Platform and title tags every client frame carries: Windows, title 1.
const TAGS: [u8; 8] = *b"W20\x000001";

/// A patch service: its server, the platform and title tags every client frame carries, the title (patch.ver's key),
/// and whether its code files are written (the Viewer's: kept, never run) or left (the game's: Windows runs them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Service {
    pub host: &'static str,
    pub port: u16,
    tags: [u8; 8],
    pub title: u32,
    code: bool,
}

/// FFXI's game files, title 1.
pub const GAME: Service =
    Service { host: HOST, port: PORT, tags: TAGS, title: crate::net::pol::patchver::GAME, code: false };
/// The PlayOnline Viewer, title 1000 on port 53000 + 1000 (app.dll FUN_1001795d; the host is env.dat's
/// PATCH_SERVER_DOMAIN); its version check runs before any sign-in.
pub const VIEWER: Service = Service {
    host: "pt008.pol.com",
    port: 54000,
    tags: *b"W20\x001000",
    title: crate::net::pol::patchver::VIEWER,
    code: true,
};
/// The platform tag a manifest path may name (`__W20__`).
const PLATFORM: &str = "W20";
/// The version file a title's folder keeps (net::pol::patchver).
pub const PATCH_VER: &str = "patch.ver";
const MAGIC: &[u8; 4] = b"POLP";
const HEADER: usize = 16;
const CHUNK: u32 = 0x10000;
const VERSION_BLOCK: usize = 0x40;
/// Our record of the last manifest applied (not the Viewer's patch.cfg: code files may stay behind).
pub const STAMP_FILE: &str = "vana-update.cfg";

mod kind {
    pub const HELLO: u32 = 1;
    pub const MANIFEST: u32 = 2;
    pub const FILE_REQUEST: u32 = 3;
    pub const FILE_CHUNK: u32 = 4;
    pub const REJECT: u32 = 5;
    pub const VERSION: u32 = 7;
    pub const VERSION_REPLY: u32 = 8;
}

/// A frame: u32 total length, MD5[0..4] of everything after the checksum, "POLP", u32 kind, body.
fn seal(kind: u32, body: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(HEADER + body.len());
    f.extend_from_slice(&((HEADER + body.len()) as u32).to_le_bytes());
    f.extend_from_slice(&[0; 4]);
    f.extend_from_slice(MAGIC);
    f.extend_from_slice(&kind.to_le_bytes());
    f.extend_from_slice(body);
    let d = Md5::digest(&f[8..]);
    f[4..8].copy_from_slice(&d[..4]);
    f
}

fn open(frame: &[u8]) -> Result<(u32, &[u8])> {
    if frame.len() < HEADER || &frame[8..12] != MAGIC {
        bail!("not a POLP frame");
    }
    if Md5::digest(&frame[8..])[..4] != frame[4..8] {
        bail!("frame checksum mismatch");
    }
    Ok((u32::from_le_bytes(frame[12..16].try_into().unwrap()), &frame[HEADER..]))
}

fn file_request(tags: &[u8; 8], path: &str, offset: u32, len: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(21 + path.len());
    b.extend_from_slice(&offset.to_le_bytes());
    b.extend_from_slice(&len.to_le_bytes());
    b.extend_from_slice(tags);
    b.extend_from_slice(&(path.len() as u32 + 1).to_le_bytes());
    b.extend_from_slice(path.as_bytes());
    b.push(0);
    seal(kind::FILE_REQUEST, &b)
}

/// The version reply: release time, flags, a 0x40-byte field of NUL-separated strings (a label: "empty" for a
/// blank block, "registered" for a version-shaped one, "unknown" for anything else; then the host to download
/// from; then a number only the Viewer's own title reads) and the version string.
#[derive(Debug, Clone, Default)]
pub struct ServerVersion {
    #[allow(dead_code)] // format field, not read
    pub released: u32,
    pub label: String,
    /// Where the manifest and the files come from (retail resolves it and reconnects there; FUN_10018bca state
    /// 0x26b). None when the reply names none.
    pub host: Option<String>,
    pub version: String,
    /// The version the check was asked with: the one applied here, None for a blank block.
    pub asked: Option<String>,
}

/// The label of a reply to a version block the server cannot place. Retail fails the update with -0x48f and
/// deletes patch.ver, so its next run asks with a blank block.
const UNKNOWN: &str = "unknown";

/// A version string as retail orders them (FUN_10017f3a): base 37 with digits 0-9 and A-Z; `_` is skipped and any
/// other character counts as 0. Stops at a space.
pub fn stamp_num(v: &str) -> i64 {
    let mut n = 0i64;
    for c in v.bytes().take_while(|&c| c != b' ') {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'A'..=b'Z' => c - b'A' + 10,
            b'_' => continue,
            _ => 0,
        };
        n = n.wrapping_mul(37).wrapping_add(d as i64);
    }
    n
}

/// Nothing to fetch (state 0x26e): the two strings are as long and the server's is not the newer one.
pub fn up_to_date(server: &str, applied: &str) -> bool {
    server.len() == applied.len() && stamp_num(server) <= stamp_num(applied)
}

fn version_reply(b: &[u8]) -> Result<ServerVersion> {
    let at = 8 + VERSION_BLOCK;
    if b.len() < at + 4 {
        bail!("short version reply");
    }
    let n = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    let text = b.get(at + 4..at + 4 + n).ok_or_else(|| anyhow!("version string truncated"))?;
    let cstr = |s: &[u8]| String::from_utf8_lossy(s.split(|&c| c == 0).next().unwrap_or_default()).into_owned();
    let mut fields = b[8..at].split(|&c| c == 0).map(|f| String::from_utf8_lossy(f).into_owned());
    Ok(ServerVersion {
        released: u32::from_le_bytes(b[..4].try_into().unwrap()),
        label: fields.next().unwrap_or_default(),
        host: fields.next().filter(|h| !h.is_empty()),
        version: cstr(text),
        asked: None,
    })
}

/// A version frame: tags, then the 0x40-byte block (the version applied here; blank when none).
fn version_request(tags: &[u8; 8], applied: Option<&str>) -> Vec<u8> {
    let mut b = tags.to_vec();
    let v = applied.unwrap_or_default().as_bytes();
    let n = v.len().min(VERSION_BLOCK - 1);
    b.extend_from_slice(&v[..n]);
    b.resize(tags.len() + VERSION_BLOCK, 0);
    seal(kind::VERSION, &b)
}

pub struct Session {
    s: TcpStream,
    tags: [u8; 8],
    /// When the last file request went out (retail's +0x7778ec, set at the connect and at every request).
    asked: Instant,
    /// The version frame a keep-alive sends.
    alive: Vec<u8>,
}

/// A download connection idle this long gets a version frame before the next file (FUN_10021f15: 60000 ms).
const KEEP_ALIVE: Duration = Duration::from_secs(60);

impl Session {
    pub fn connect() -> Result<Self> {
        Self::connect_to(None, None)
    }

    /// A connection to `host` (the one a version reply named; the name the client knows when None or when it
    /// does not resolve, as retail keeps its address then). `applied` goes into the keep-alive frames.
    pub fn connect_to(host: Option<&str>, applied: Option<&str>) -> Result<Self> {
        Self::open(GAME, host, applied)
    }

    /// [`Session::connect_to`] for any service.
    pub fn open(service: Service, host: Option<&str>, applied: Option<&str>) -> Result<Self> {
        let port = service.port;
        let named = host.and_then(|h| TcpStream::connect((h, port)).ok());
        let s = match named {
            Some(s) => s,
            None => TcpStream::connect((service.host, port))
                .with_context(|| format!("connecting to {}:{port}", service.host))?,
        };
        s.set_read_timeout(Some(Duration::from_secs(120)))?;
        s.set_write_timeout(Some(Duration::from_secs(120)))?;
        Ok(Self { s, tags: service.tags, asked: Instant::now(), alive: version_request(&service.tags, applied) })
    }

    /// A connection for one quick exchange (a log-in's Viewer check): 15 s to connect, 30 s to answer, as the
    /// profile service's transactions wait (net::pol::polpro::transact).
    pub fn quick(service: Service) -> Result<Self> {
        use std::net::ToSocketAddrs;
        let (host, port) = (service.host, service.port);
        let mut last = None;
        for addr in (host, port).to_socket_addrs().with_context(|| format!("looking up {host}"))? {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(15)) {
                Ok(s) => {
                    s.set_read_timeout(Some(Duration::from_secs(30)))?;
                    s.set_write_timeout(Some(Duration::from_secs(30)))?;
                    let alive = version_request(&service.tags, None);
                    return Ok(Self { s, tags: service.tags, asked: Instant::now(), alive });
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.map_or_else(|| anyhow!("{host} has no address"), anyhow::Error::from))
            .with_context(|| format!("connecting to {host}:{port}"))
    }

    fn exchange(&mut self, req: &[u8], want: u32) -> Result<Vec<u8>> {
        self.s.write_all(req)?;
        let mut head = [0u8; 4];
        self.s.read_exact(&mut head)?;
        let total = u32::from_le_bytes(head) as usize;
        if !(HEADER..=1 << 30).contains(&total) {
            bail!("patch server announced a {total}-byte frame");
        }
        let mut f = vec![0u8; total];
        f[..4].copy_from_slice(&head);
        self.s.read_exact(&mut f[4..])?;
        let (k, _) = open(&f)?;
        match k {
            k if k == want => Ok(f),
            kind::REJECT => bail!("patch server rejected the request"),
            k => bail!("patch server answered with frame {k}, expected {want}"),
        }
    }

    /// The server's version, asked with the version applied here (retail sends patch.ver's; blank without one,
    /// which the server labels "empty").
    pub fn version(&mut self, applied: Option<&str>) -> Result<ServerVersion> {
        let f = self.exchange(&version_request(&self.tags, applied), kind::VERSION_REPLY)?;
        version_reply(&f[HEADER..])
    }

    /// Before a file: a version frame when nothing was asked for a minute (states 900 / 2000 / 0x7d1).
    fn keep_alive(&mut self) -> Result<()> {
        if self.asked.elapsed() > KEEP_ALIVE {
            let req = self.alive.clone();
            self.exchange(&req, kind::VERSION_REPLY)?;
            self.asked = Instant::now();
        }
        Ok(())
    }

    /// The manifest text: status byte 2, then `.slc`.
    pub fn manifest(&mut self) -> Result<Vec<u8>> {
        let f = self.exchange(&seal(kind::HELLO, &self.tags), kind::MANIFEST)?;
        match f.get(HEADER) {
            Some(2) => slc(&f[HEADER + 1..]),
            s => bail!("manifest refused (status {s:?})"),
        }
    }

    /// `len` bytes of a server path in 64 KiB chunks (body: offset, len, extra, [extra bytes], data).
    pub fn fetch(&mut self, path: &str, len: u64) -> Result<Vec<u8>> {
        self.keep_alive()?;
        let mut out = Vec::with_capacity(len as usize);
        while (out.len() as u64) < len {
            let offset = out.len() as u32;
            let want = ((len - out.len() as u64) as u32).min(CHUNK);
            self.asked = Instant::now();
            let f = self.exchange(&file_request(&self.tags, path, offset, want), kind::FILE_CHUNK)?;
            let b = &f[HEADER..];
            let u = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
            let (Some(got), Some(n), Some(extra)) = (u(0), u(4), u(8)) else { bail!("{path}: short chunk") };
            let data = b
                .get(12 + extra as usize..12 + extra as usize + n as usize)
                .ok_or_else(|| anyhow!("{path}: chunk overruns its frame"))?;
            // Retail drops the connection on a chunk longer than its 64 KiB request size (-0x489).
            if n > CHUNK {
                bail!("{path}: a {n}-byte chunk");
            }
            if got != offset || data.is_empty() {
                bail!("{path}: asked for offset {offset}, got {} bytes at {got}", data.len());
            }
            out.extend_from_slice(data);
        }
        Ok(out)
    }
}

/// LSB-first bits after a u32 bit count.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Bits<'a> {
    fn new(coded: &'a [u8]) -> Result<Self> {
        let end =
            u32::from_le_bytes(coded.get(..4).ok_or_else(|| anyhow!("coded stream too short"))?.try_into().unwrap())
                as usize;
        let d = &coded[4..];
        if end > d.len() * 8 {
            bail!("coded stream declares {end} bits, holds {}", d.len() * 8);
        }
        Ok(Self { d, pos: 0, end })
    }

    fn left(&self) -> bool {
        self.pos < self.end
    }

    fn take(&mut self, n: usize) -> Result<u32> {
        if self.pos + n > self.end {
            bail!("coded stream truncated");
        }
        let mut v = 0u32;
        for i in 0..n {
            let p = self.pos + i;
            v |= (((self.d[p >> 3] >> (p & 7)) & 1) as u32) << i;
        }
        self.pos += n;
        Ok(v)
    }
}

fn back(out: &mut Vec<u8>, dist: usize, len: usize) -> Result<()> {
    if dist == 0 || dist > out.len() {
        bail!("back reference {dist} outside {} bytes", out.len());
    }
    let s = out.len() - dist;
    for i in 0..len {
        out.push(out[s + i]);
    }
    Ok(())
}

/// `.slc`: per item a flag bit; 1 = copy (distance 16 bits, length 8) from the output, 0 = literal byte.
pub fn slc(coded: &[u8]) -> Result<Vec<u8>> {
    let mut r = Bits::new(coded)?;
    let mut out = Vec::with_capacity(coded.len() * 2);
    while r.left() {
        if r.take(1)? == 1 {
            let (d, n) = (r.take(16)? as usize, r.take(8)? as usize);
            back(&mut out, d, n)?;
        } else {
            out.push(r.take(8)? as u8);
        }
    }
    Ok(out)
}

/// `.olc` against `base`: 0 = literal; 10 = copy from the output; 11 = copy from base at (output position +
/// signed 16-bit offset), length 8 bits.
pub fn olc(coded: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let mut r = Bits::new(coded)?;
    let mut out = Vec::with_capacity(base.len());
    while r.left() {
        if r.take(1)? == 0 {
            out.push(r.take(8)? as u8);
        } else if r.take(1)? == 0 {
            let (d, n) = (r.take(16)? as usize, r.take(8)? as usize);
            back(&mut out, d, n)?;
        } else {
            let off = r.take(16)? as u16 as i16 as isize;
            let n = r.take(8)? as usize;
            let at = out.len() as isize + off;
            let src = usize::try_from(at)
                .ok()
                .and_then(|a| base.get(a..a + n))
                .ok_or_else(|| anyhow!("base reference {at} outside {} bytes", base.len()))?;
            out.extend_from_slice(src);
        }
    }
    Ok(out)
}

/// A download: flag byte 1 stored, 3 zlib, else `.slc` (state 0x5df).
fn direct(data: &[u8]) -> Result<Vec<u8>> {
    match data.split_first() {
        Some((1, rest)) => Ok(rest.to_vec()),
        Some((3, rest)) => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(rest).read_to_end(&mut out)?;
            Ok(out)
        }
        Some((_, rest)) => slc(rest),
        None => bail!("empty download"),
    }
}

/// What a manifest line says about a file: length, the bytes summed as signed 8-bit values (wrapping), the
/// first MD5 word (little-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub len: u64,
    pub sum: i32,
    pub md5: i32,
}

impl Sig {
    pub fn of(d: &[u8]) -> Self {
        let sum = d.iter().fold(0i32, |s, &b| s.wrapping_add(b as i8 as i32));
        let h = Md5::digest(d);
        Self { len: d.len() as u64, sum, md5: i32::from_le_bytes([h[0], h[1], h[2], h[3]]) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    pub stamp: String,
    pub sig: Sig,
    /// `<stamp>/Direct/<path>.slc` and its length; the delta from the line before, when there is one.
    pub direct: (String, u64),
    pub delta: Option<(String, u64)>,
}

/// The block a manifest file sits in (`reset {`, `reset2 {`, `select {`, `option {` around `file` blocks; the
/// keyword is read in any case). FFXI's manifest has none (49,427 files, 2026-11-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attr {
    #[default]
    None,
    /// Replacing one makes the Viewer restart (result 2).
    Reset,
    /// The same with result 4, the first one listed in patch.rst.
    Reset2,
    /// Counted, nothing more.
    Select,
    /// Not fetched while there is no local file (state 0x4b0).
    Option,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    pub path: String,
    /// Oldest first; the last is current.
    pub versions: Vec<Version>,
    pub attr: Attr,
}

/// A manifest path (it comes over plain TCP) as a plain relative path: no empty, `.` or `..` part, no colon or
/// control character, no part Windows would trim (a trailing dot or space) or open as a device (CON, COM1 ...).
pub fn check_path(p: &str) -> Result<()> {
    let bad = |why: &str| Err(anyhow!("manifest path {p:?} {why}"));
    if p.chars().any(|c| c.is_control() || c == ':') {
        return bad("has a drive, stream or control character");
    }
    for c in p.split(['/', '\\']) {
        if c.is_empty() || c == "." || c == ".." {
            return bad("is not a plain relative path");
        }
        if c.ends_with(['.', ' ']) {
            return bad("has a name ending in a dot or space");
        }
        let stem = c.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
        let numbered = stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit();
        if numbered || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
            return bad("names a Windows device");
        }
    }
    Ok(())
}

/// Where a manifest path lives under `root`, after [`check_path`].
fn local(root: &Path, p: &str) -> Result<PathBuf> {
    check_path(p)?;
    Ok(root.join(p))
}

/// Where a manifest file is written under `root`: [`local`], through no link or junction below `root`.
fn writable(root: &Path, p: &str) -> Result<PathBuf> {
    let out = local(root, p)?;
    // The folders the write really goes through: on Unix a backslash is part of a name, not a separator.
    let mut at = root.to_path_buf();
    for c in Path::new(p).parent().into_iter().flat_map(Path::components) {
        at.push(c);
        let m = match std::fs::symlink_metadata(&at) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e).with_context(|| at.display().to_string()),
        };
        // On Windows a junction counts (a name-surrogate reparse tag); a OneDrive folder does not.
        if m.file_type().is_symlink() {
            bail!("{} is a link; the patcher only writes inside the install folder", at.display());
        }
    }
    Ok(out)
}

/// `file <path> {`, version lines `stamp len sum md5 <direct>.slc len [<delta>.olc len]`, `}`, until `end`. A path
/// with a platform marker (`__W20__`) is kept only for this platform, the marker taken out (FUN_10021a5c /
/// FUN_10021af3).
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>> {
    // Paths may hold spaces: a path token ends at its suffix followed by a space.
    fn path_len<'a>(s: &'a str, suffix: &str) -> Option<(&'a str, u64, &'a str)> {
        let end = s.find(&format!("{suffix} "))? + suffix.len();
        let rest = &s[end + 1..];
        let (n, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        Some((&s[..end], n.parse().ok()?, rest))
    }
    let line_version = |l: &str| -> Option<Version> {
        let mut it = l.splitn(5, ' ');
        let (stamp, len, sum, md5, rest) =
            (it.next()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?);
        let (dp, dl, rest) = path_len(rest, ".slc")?;
        let delta = if rest.is_empty() {
            None
        } else {
            let (p, n, tail) = path_len(rest, ".olc")?;
            if !tail.is_empty() {
                return None;
            }
            Some((p.to_string(), n))
        };
        Some(Version { stamp: stamp.into(), sig: Sig { len, sum, md5 }, direct: (dp.into(), dl), delta })
    };
    let mut out = Vec::new();
    let mut open: Option<Entry> = None;
    let mut attr = Attr::None;
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        // The first word in lower case picks the line's kind (FUN_10027abc); `}` outside a file ends a block.
        let word = l.trim_start().split([' ', '\t', '{']).next().unwrap_or_default().to_ascii_lowercase();
        match open.as_mut() {
            None if word == "end" => break,
            None if word == "}" || l.trim() == "}" => attr = Attr::None,
            None if matches!(word.as_str(), "reset" | "reset2" | "select" | "option") => {
                attr = match word.as_str() {
                    "reset" => Attr::Reset,
                    "reset2" => Attr::Reset2,
                    "select" => Attr::Select,
                    _ => Attr::Option,
                }
            }
            None => {
                let p = l
                    .trim_start()
                    .strip_prefix("file ")
                    .map(|s| s.trim_end_matches(['{', ' ', '\t']))
                    .filter(|_| l.trim_end().ends_with('{'))
                    .ok_or_else(|| anyhow!("manifest line {}: {l:?}", i + 1))?;
                // Another platform's file is read through and dropped (an empty path).
                let p = platform_path(p).unwrap_or_default();
                if !p.is_empty() {
                    check_path(&p).with_context(|| format!("manifest line {}", i + 1))?;
                }
                open = Some(Entry { path: p, versions: Vec::new(), attr });
            }
            Some(_) if l == "}" => {
                let e = open.take().unwrap();
                if !e.versions.is_empty() && !e.path.is_empty() {
                    out.push(e);
                }
            }
            Some(e) => e.versions.push(line_version(l).ok_or_else(|| anyhow!("manifest line {}: {l:?}", i + 1))?),
        }
    }
    if open.is_some() {
        bail!("manifest ends inside a file block");
    }
    Ok(out)
}

/// A manifest path for this platform: unchanged with no marker, the marker taken out when it is `__W20__`, None for
/// another platform's.
fn platform_path(p: &str) -> Option<String> {
    let Some(at) = p.find("__") else { return Some(p.to_string()) };
    if p.get(at + 5..at + 7) != Some("__") {
        return Some(p.to_string());
    }
    (p.get(at + 2..at + 5)? == PLATFORM).then(|| format!("{}{}", &p[..at], &p[at + 7..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Current,
    /// The current version whole.
    Direct,
    /// Deltas from this version index on.
    Deltas(usize),
    /// The staged `.tmp2` already holds the current version: nothing to fetch, it only moves into place.
    Staged,
    /// Deltas from this version index on, built on the staged `.tmp2` (an older version a stopped run left).
    StagedDeltas(usize),
}

/// Newest matching line: current, else its delta chain when every step has one and the sum is smaller than the
/// whole file, else the whole file. Retail takes deltas only with a version applied (patch.ver; state 0x44c):
/// `versioned`.
pub fn decide(e: &Entry, local: Option<Sig>, versioned: bool) -> (Action, u64) {
    let cur = e.versions.last().expect("entries have versions");
    let whole = (Action::Direct, cur.direct.1);
    let Some(i) = local.and_then(|l| e.versions.iter().rposition(|v| v.sig == l)) else { return whole };
    if i + 1 == e.versions.len() {
        return (Action::Current, 0);
    }
    if !versioned {
        return whole;
    }
    let mut bytes = 0;
    for v in &e.versions[i + 1..] {
        match &v.delta {
            Some((_, n)) => bytes += n,
            None => return whole,
        }
    }
    if bytes < cur.direct.1 { (Action::Deltas(i + 1), bytes) } else { whole }
}

/// Executables and libraries: never written into the game folder (retail, Windower and Ashita run them; the Viewer
/// updates them).
pub fn is_code(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".dll") || p.ends_with(".exe")
}

/// The one code file the client needs: it reads its tables out of the current FFXiMain.dll (as data, never run).
fn is_ffximain(path: &str) -> bool {
    path.eq_ignore_ascii_case("FFXiMain.dll")
}

/// The file a manifest entry is checked against (and a delta built on): FFXiMain.dll is the one the client reads.
fn source(root: &Path, p: &str) -> Result<PathBuf> {
    if is_ffximain(p) { Ok(crate::dat::ffximain(root)) } else { local(root, p) }
}

/// Where an entry is written under `root`: FFXiMain.dll to the client's copy (dat::OWN_FFXIMAIN), the rest at its path.
fn target(root: &Path, p: &str) -> Result<PathBuf> {
    writable(root, if is_ffximain(p) { crate::dat::OWN_FFXIMAIN } else { p })
}

/// Retail's staging name beside a file: the new content waits there until every file is fetched.
fn staged(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.tmp2", file.display()))
}

/// Where an entry's staged file would be, for looking only (no link check: nothing is written through it).
fn staged_at(root: &Path, p: &str) -> Result<PathBuf> {
    Ok(staged(&local(root, if is_ffximain(p) { crate::dat::OWN_FFXIMAIN } else { p })?))
}

/// Until 2026-11-04 the updater kept one FFXiMain.dll in the data dir for every install. At start it moves beside
/// `root` when it is the one `root`'s stamped manifest wants; otherwise it stays, unread, for the folder it belongs to.
pub fn adopt_old_ffximain(root: &Path) {
    let old = crate::config::data_dir().join("FFXiMain.dll");
    match adopt(&old, root) {
        Ok(true) => log::info!("updater: {} moved to {}", old.display(), crate::dat::own_ffximain(root).display()),
        Ok(false) => {}
        Err(e) => log::warn!("updater: {}: {e:#}", old.display()),
    }
}

fn adopt(old: &Path, root: &Path) -> Result<bool> {
    let Ok(meta) = std::fs::metadata(old) else { return Ok(false) };
    if crate::dat::own_ffximain(root).is_file() {
        return Ok(false);
    }
    let Ok(text) = std::fs::read_to_string(root.join(STAMP_FILE)) else { return Ok(false) };
    let entries = parse_manifest(&text)?;
    let Some(want) = entries.iter().find(|e| is_ffximain(&e.path)).and_then(|e| e.versions.last()).map(|v| v.sig)
    else {
        return Ok(false);
    };
    if meta.len() != want.len {
        return Ok(false);
    }
    let data = std::fs::read(old)?;
    if Sig::of(&data) != want {
        return Ok(false);
    }
    write_atomic(&target(root, "FFXiMain.dll")?, &data)?;
    std::fs::remove_file(old)?;
    Ok(true)
}

#[derive(Default)]
pub struct Plan {
    pub service: Service,
    pub server: ServerVersion,
    pub manifest: Vec<u8>,
    pub entries: Vec<Entry>,
    /// (entry index, action, bytes) for every file to change.
    pub todo: Vec<(usize, Action, u64)>,
    pub current: usize,
    pub skipped_code: usize,
    /// Staged files that match no version of their file: [`apply`] deletes them (retail's "illegal tmp2 file
    /// rollback", states 0x44d / 0x44e).
    pub stale: Vec<PathBuf>,
}

fn sig_of_path(p: &Path) -> Result<Option<Sig>> {
    if !p.is_file() {
        return Ok(None);
    }
    Ok(Some(Sig::of(&std::fs::read(p).with_context(|| p.display().to_string())?)))
}

/// The game's version here: the newest stamp of the manifest last applied (our stamp file), or the folder's patch.ver
/// (a Viewer patched it) when that is newer.
pub fn applied_version(root: &Path) -> Option<String> {
    let stamped = std::fs::read_to_string(root.join(STAMP_FILE)).ok().and_then(|text| {
        parse_manifest(&text)
            .ok()?
            .iter()
            .flat_map(|e| e.versions.iter().map(|v| v.stamp.clone()))
            .max_by_key(|s| version_key(s))
    });
    [stamped, installed_version(root, GAME)].into_iter().flatten().max_by_key(|s| version_key(s))
}

/// A title's version as its patch.ver holds it: under the no-registry key (ours), else the installer's "0" (a folder
/// the Viewer patched).
pub fn installed_version(root: &Path, service: Service) -> Option<String> {
    let coded = std::fs::read(root.join(PATCH_VER)).ok()?;
    [None, Some("0")].into_iter().find_map(|r| crate::net::pol::patchver::read(&coded, service.title, r))
}

type Stamp = Option<(Option<std::time::SystemTime>, u64)>;
type Seen = (PathBuf, Stamp, Stamp, Option<String>);
static SEEN: std::sync::Mutex<Option<Seen>> = std::sync::Mutex::new(None);

/// [`applied_version`], read again only when the stamp file or patch.ver changes (every Config::load asks for it).
pub fn applied_version_cached(root: &Path) -> Option<String> {
    let path = root.join(STAMP_FILE);
    let (stamp, ver) = (std::fs::metadata(&path).ok(), std::fs::metadata(root.join(PATCH_VER)).ok());
    if stamp.is_none() && ver.is_none() {
        return None;
    }
    // Each file's own time and length: a change to either one reads the version again.
    let mark = |m: Option<std::fs::Metadata>| m.map(|m| (m.modified().ok(), m.len()));
    let key = (path, mark(stamp), mark(ver));
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, n, v)) = seen.as_ref()
        && (p, m, n) == (&key.0, &key.1, &key.2)
    {
        return v.clone();
    }
    let v = applied_version(root);
    *seen = Some((key.0, key.1, key.2, v.clone()));
    v
}

/// A client version as a sortable key: "30261102_0" is (30261102, 0); None for any other shape.
pub fn version_key(v: &str) -> Option<(u64, u64)> {
    let (date, n) = v.trim().split_once('_')?;
    Some((date.parse().ok()?, n.parse().ok()?))
}

/// The version check as retail makes it: asked with the version applied here. A server that does not know that
/// version is asked again with a blank block, which makes the run a full check (retail fails there with -0x48f
/// and deletes patch.ver, so its next run is the blank one).
fn ask(applied: Option<String>) -> Result<ServerVersion> {
    ask_service(GAME, applied)
}

fn ask_service(service: Service, applied: Option<String>) -> Result<ServerVersion> {
    let mut server = Session::open(service, None, None)?.version(applied.as_deref())?;
    server.asked = applied;
    if server.label == UNKNOWN && server.asked.is_some() {
        log::warn!("updater: the patch server does not know version {:?}; checking every file", server.asked);
        server = Session::open(service, None, None)?.version(None)?;
    }
    Ok(server)
}

/// The PlayOnline Viewer's current version, asked as its reconnect check does (a blank block: nothing about this
/// install is sent), on a quick connection.
pub fn viewer_version() -> Result<ServerVersion> {
    Session::quick(VIEWER)?.version(None)
}

/// The Viewer kept here (root: config::viewer_dir) against the patch server's, asked as the Viewer's start-up check
/// (FUN_10018bca state 0x26a: with the local version): the server's version when it is out of date or missing, else
/// None.
pub fn viewer_behind(root: &Path) -> Result<Option<ServerVersion>> {
    let server = ask_service(VIEWER, installed_version(root, VIEWER))?;
    let current = server.asked.as_deref().is_some_and(|a| up_to_date(&server.version, a));
    Ok((!current).then_some(server))
}

/// The files of the Viewer kept under `root` against `server`'s manifest: every file, code too (never run).
pub fn plan_viewer(root: &Path, server: ServerVersion) -> Result<Plan> {
    plan_for(VIEWER, root, true, server)
}

/// The start-up check's quick part (one request): the server's version when it is newer than the one applied
/// last (retail's version check, states 0x26a .. 0x26e) or the client's FFXiMain.dll lags (`code_lags`), else None.
pub fn behind(root: &Path) -> Result<Option<ServerVersion>> {
    let server = ask(applied_version(root))?;
    let current = server.asked.as_deref().is_some_and(|a| up_to_date(&server.version, a));
    Ok((!current || code_lags(root)).then_some(server))
}

/// Not retail (retail keeps no copy of its own): the FFXiMain.dll the client reads lags the version applied. After
/// a Viewer patch past our stamp, our copy must match the game folder's; else the read copy must match the stamp.
fn code_lags(root: &Path) -> bool {
    let Some(entries) = std::fs::read_to_string(root.join(STAMP_FILE)).ok().and_then(|t| parse_manifest(&t).ok())
    else {
        return false;
    };
    let stamped = entries.iter().flat_map(|e| e.versions.iter()).filter_map(|v| version_key(&v.stamp)).max();
    if installed_version(root, GAME).and_then(|v| version_key(&v)) > stamped {
        let own = crate::dat::own_ffximain(root);
        let sig = |p: &Path| sig_of_path(p).ok().flatten();
        return own.is_file() && sig(&own) != sig(&root.join("FFXiMain.dll"));
    }
    !entries.iter().filter(|e| is_ffximain(&e.path)).all(|e| {
        source(root, &e.path)
            .and_then(|p| sig_of_path(&p))
            .ok()
            .flatten()
            .is_some_and(|l| decide(e, Some(l), true).0 == Action::Current)
    })
}

/// The slow part: every listed file checked against `server`'s manifest.
pub fn plan_against(root: &Path, server: ServerVersion) -> Result<Plan> {
    plan_for(GAME, root, true, server)
}

/// Drops the applied manifest: the folder's files are about to be replaced by older ones, so the stamp would
/// wrongly vouch for them (the next update then reads every file).
pub fn forget_applied(root: &Path) -> Result<()> {
    match std::fs::remove_file(root.join(STAMP_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).context("clearing the applied manifest"),
        _ => Ok(()),
    }
}

/// Asks the server for its version and manifest and checks the listed files under `root` (in parallel). Code files
/// are left as they are; `code` fetches the FFXiMain.dll the client reads. `full` asks as an install with no
/// version applied, so every file is read (retail without its patch.ver).
pub fn plan(root: &Path, code: bool, full: bool) -> Result<Plan> {
    plan_for(GAME, root, code, ask(if full { None } else { applied_version(root) })?)
}

/// What one manifest file needs, in retail's order (states 0x385 .. 0x44c): with a version applied, a file with
/// no newer line is taken as current unread; else the staged `.tmp2` is tried, then the file itself. Also the
/// staged file when it matches nothing. `lags`: FFXiMain.dll is read anyway (`code_lags`, not retail).
fn check(root: &Path, e: &Entry, applied: Option<&str>, lags: bool) -> Result<((Action, u64), Option<PathBuf>)> {
    let cur = e.versions.last().expect("entries have versions");
    if let Some(a) = applied
        && !(lags && is_ffximain(&e.path))
        && stamp_num(&cur.stamp) <= stamp_num(a)
    {
        return Ok(((Action::Current, 0), None));
    }
    let tmp2 = staged_at(root, &e.path)?;
    let mut stale = None;
    if let Some(sig) = sig_of_path(&tmp2)? {
        match decide(e, Some(sig), applied.is_some()) {
            (Action::Current, _) => return Ok(((Action::Staged, 0), None)),
            (Action::Deltas(i), n) => return Ok(((Action::StagedDeltas(i), n), None)),
            // Direct for a staged older version writes over it; one that matches nothing goes first.
            d if e.versions.iter().any(|v| v.sig == sig) => return Ok((d, None)),
            // Removed later: only a path no link leads out of the install (as for a write).
            _ => stale = target(root, &e.path).is_ok().then_some(tmp2),
        }
    }
    let have = sig_of_path(&source(root, &e.path)?)?;
    if have.is_none() && e.attr == Attr::Option {
        return Ok(((Action::Current, 0), stale));
    }
    Ok((decide(e, have, applied.is_some()), stale))
}

fn plan_for(service: Service, root: &Path, code: bool, server: ServerVersion) -> Result<Plan> {
    let applied = server.asked.as_deref();
    let manifest = Session::open(service, server.host.as_deref(), applied)?.manifest()?;
    let entries = parse_manifest(&String::from_utf8_lossy(&manifest))?;
    let lags = !service.code && code && code_lags(root);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let found: std::sync::Mutex<Vec<(usize, Action, u64)>> = Default::default();
    let stale: std::sync::Mutex<Vec<PathBuf>> = Default::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    std::thread::scope(|s| -> Result<()> {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| -> Result<()> {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(e) = entries.get(i) else { return Ok(()) };
                        let (d, old) = check(root, e, applied, lags)?;
                        found.lock().unwrap().push((i, d.0, d.1));
                        stale.lock().unwrap().extend(old);
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|w| w.join().map_err(|_| anyhow!("scan thread panicked"))?)
    })?;
    let mut all = found.into_inner().unwrap();
    all.sort_by_key(|x| x.0);
    let current = all.iter().filter(|x| x.1 == Action::Current).count();
    let wanted = |p: &str| service.code || !is_code(p) || (code && is_ffximain(p));
    let skipped_code = all.iter().filter(|x| x.1 != Action::Current && !wanted(&entries[x.0].path)).count();
    let todo = all.into_iter().filter(|x| x.1 != Action::Current && wanted(&entries[x.0].path)).collect();
    let mut stale = stale.into_inner().unwrap();
    stale.sort();
    Ok(Plan { service, server, manifest, entries, todo, current, skipped_code, stale })
}

/// New content for `path`, staged beside it as `.tmp2` (through `.tmp`, as retail writes its download first).
fn stage(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, staged(path))?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = PathBuf::from(format!("{}.vana-tmp", path.display()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The new content of one file, checked against its manifest signature at every step.
fn produce(s: &mut Session, root: &Path, e: &Entry, a: Action) -> Result<Vec<u8>> {
    let cur = e.versions.last().unwrap();
    let data = match a {
        Action::Current | Action::Staged => unreachable!(),
        Action::Direct => direct(&s.fetch(&cur.direct.0, cur.direct.1)?).with_context(|| cur.direct.0.clone())?,
        Action::Deltas(from) | Action::StagedDeltas(from) => {
            // The first delta builds on the file, or on the staged one a stopped run left (state 0x649).
            let base = if a == Action::Deltas(from) { source(root, &e.path)? } else { staged_at(root, &e.path)? };
            let mut d = std::fs::read(base)?;
            for v in &e.versions[from..] {
                let (p, n) = v.delta.as_ref().unwrap();
                d = olc(&s.fetch(p, *n)?, &d).with_context(|| p.clone())?;
                if Sig::of(&d) != v.sig {
                    bail!("{}: delta {p} gave the wrong content", e.path);
                }
            }
            d
        }
    };
    if Sig::of(&data) != cur.sig {
        bail!("{}: download does not match the manifest", e.path);
    }
    Ok(data)
}

/// The current version of one manifest file, written under `dir` at its manifest path (the install untouched).
pub fn fetch_one(path: &str, dir: &Path) -> Result<(PathBuf, String)> {
    let manifest = Session::connect()?.manifest()?;
    let entries = parse_manifest(&String::from_utf8_lossy(&manifest))?;
    let e = entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(path))
        .ok_or_else(|| anyhow!("{path} is not in the manifest"))?;
    let data = produce(&mut Session::connect()?, dir, e, Action::Direct)?;
    let out = writable(dir, &e.path)?;
    write_atomic(&out, &data)?;
    Ok((out, e.versions.last().map(|v| v.stamp.clone()).unwrap_or_default()))
}

/// Fetches every planned file on `connections` sessions into its staged `.tmp2`, then moves them all into place
/// and records the manifest (retail's download pass, then its rename pass: a run stopped before the end leaves
/// the game's files as they were). `progress` gets (files done, bytes done).
pub fn apply(root: &Path, plan: &Plan, connections: usize, progress: &(dyn Fn(usize, u64) + Sync)) -> Result<()> {
    for old in &plan.stale {
        log::info!("updater: {} matches no version, removed", old.display());
        let _ = std::fs::remove_file(old);
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (files, bytes) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicU64::new(0));
    let failed: std::sync::Mutex<Option<anyhow::Error>> = Default::default();
    std::thread::scope(|s| {
        for _ in 0..connections.clamp(1, 6).min(plan.todo.len().max(1)) {
            s.spawn(|| {
                // Connected at the first file to fetch: a run that only moves staged files needs no server.
                let mut sess: Option<Session> = None;
                loop {
                    if failed.lock().unwrap().is_some() {
                        return;
                    }
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(ei, a, n)) = plan.todo.get(i) else { return };
                    let e = &plan.entries[ei];
                    let staged = match a {
                        Action::Staged => target(root, &e.path).map(drop),
                        _ => (|| {
                            if sess.is_none() {
                                let (host, asked) = (plan.server.host.as_deref(), plan.server.asked.as_deref());
                                sess = Some(Session::open(plan.service, host, asked)?);
                            }
                            let d = produce(sess.as_mut().unwrap(), root, e, a)?;
                            stage(&target(root, &e.path)?, &d)
                        })(),
                    };
                    match staged {
                        Ok(()) => {
                            let f = files.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            let b = bytes.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n;
                            progress(f, b);
                        }
                        Err(e) => return *failed.lock().unwrap() = Some(e),
                    }
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e);
    }
    let (mut reset, mut reset2) = (0, 0);
    for &(ei, ..) in &plan.todo {
        let e = &plan.entries[ei];
        let to = target(root, &e.path)?;
        std::fs::rename(staged(&to), &to).with_context(|| to.display().to_string())?;
        reset += (e.attr == Attr::Reset) as usize;
        reset2 += (e.attr == Attr::Reset2) as usize;
    }
    if reset + reset2 > 0 {
        // The Viewer restarts itself here (results 2 and 4); nothing of this client runs from those files.
        log::info!("updater: {reset} reset and {reset2} reset2 files replaced");
    }
    write_atomic(&root.join(STAMP_FILE), &plan.manifest)?;
    // Last, as the Viewer's install does (state 0x6a6): only for a service whose code was written too. The game's
    // patch.ver stays the Viewer's, since its code files were left behind.
    if plan.service.code {
        let v = crate::net::pol::patchver::write(&plan.server.version, plan.service.title);
        write_atomic(&root.join(PATCH_VER), &v)?;
    }
    // A new stamp the same size within the filesystem's timestamp step would look unchanged to the cache.
    *SEEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB-first bit packing with the u32 bit count in front, for building test streams.
    fn pack(items: &[(u32, usize)]) -> Vec<u8> {
        let bits: Vec<bool> = items.iter().flat_map(|&(v, n)| (0..n).map(move |i| (v >> i) & 1 == 1)).collect();
        let mut out = (bits.len() as u32).to_le_bytes().to_vec();
        for chunk in bits.chunks(8) {
            out.push(chunk.iter().enumerate().fold(0u8, |b, (i, &x)| b | (x as u8) << i));
        }
        out
    }

    /// The Viewer's check: 0x58 bytes, title "1000", a blank block; the game's tags stay title 1.
    #[test]
    fn viewer_frames() {
        let f = version_request(&VIEWER.tags, None);
        assert_eq!((f.len(), &f[8..24]), (0x58, &b"POLP\x07\0\0\0W20\x001000"[..]));
        assert!(f[24..].iter().all(|&b| b == 0));
        assert_eq!(&version_request(&GAME.tags, None)[16..24], b"W20\x000001");
        assert!(up_to_date("20261022_0", "20261022_0") && up_to_date("20261022_0", "20261201_0"));
        assert!(!up_to_date("20261201_0", "20261022_0") && !up_to_date("2026102_0", "20261022_0"));
    }

    /// A platform-marked path: kept without the marker for W20, dropped for another platform.
    #[test]
    fn manifest_platform_marks() {
        let v = |p: &str| format!("20261022_0 4 1 2 20261022_0/Direct/{p}.slc 9");
        let text = format!(
            "file x__DWJ__.bin {{\n{}\n}}\nfile y__W20__.bin {{\n{}\n}}\nfile a.txt {{\n{}\n}}\nend\n",
            v("x__DWJ__.bin"),
            v("y__W20__.bin"),
            v("a.txt")
        );
        let paths: Vec<_> = parse_manifest(&text).unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(paths, ["y.bin", "a.txt"]);
    }

    /// A Viewer update against a local patch server: version, manifest, files (code too; an absent option file left
    /// away), then the stamp and patch.ver last; a game plan writes no patch.ver.
    #[test]
    fn viewer_update_against_a_mock() {
        use std::net::TcpListener;
        let files: [(&str, &[u8]); 3] =
            [("viewer/com/app.dll", b"MZ app"), ("data/x.txt", b"hello"), ("EU/env.dat", b"eu")];
        let line = |p: &str, d: &[u8]| {
            let g = Sig::of(d);
            format!(
                "file {p} {{\n20261022_0 {} {} {} 20261022_0/Direct/{p}.slc {}\n}}\n",
                g.len,
                g.sum,
                g.md5,
                d.len() + 1
            )
        };
        let manifest = format!(
            "{}{}option {{\n{}}}\nend\n",
            line(files[0].0, files[0].1),
            line(files[1].0, files[1].1),
            line(files[2].0, files[2].1)
        );
        let literals = |t: &[u8]| pack(&t.iter().flat_map(|&b| [(0, 1), (b as u32, 8)]).collect::<Vec<_>>());
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for c in l.incoming().take(3) {
                let mut c = c.unwrap();
                loop {
                    let mut head = [0u8; 4];
                    if c.read_exact(&mut head).is_err() {
                        break;
                    }
                    let mut f = vec![0u8; u32::from_le_bytes(head) as usize];
                    f[..4].copy_from_slice(&head);
                    c.read_exact(&mut f[4..]).unwrap();
                    let (k, b) = open(&f).unwrap();
                    let reply = match k {
                        kind::VERSION => {
                            assert_eq!(&b[..8], b"W20\x001000");
                            let mut r = vec![0u8; 8 + VERSION_BLOCK];
                            r.extend(10u32.to_le_bytes());
                            r.extend(b"20261022_0");
                            seal(kind::VERSION_REPLY, &r)
                        }
                        kind::HELLO => {
                            let mut r = vec![2u8];
                            r.extend(literals(manifest.as_bytes()));
                            seal(kind::MANIFEST, &r)
                        }
                        kind::FILE_REQUEST => {
                            assert_eq!(&b[8..16], b"W20\x001000");
                            let n = u32::from_le_bytes(b[16..20].try_into().unwrap()) as usize;
                            let path = std::str::from_utf8(&b[20..20 + n - 1]).unwrap().to_string();
                            let name = path.strip_prefix("20261022_0/Direct/").unwrap().strip_suffix(".slc").unwrap();
                            asked.push(name.to_string());
                            let data = files.iter().find(|f| f.0 == name).unwrap().1;
                            let mut r = 0u32.to_le_bytes().to_vec();
                            r.extend((data.len() as u32 + 1).to_le_bytes());
                            r.extend(0u32.to_le_bytes());
                            r.push(1);
                            r.extend(data);
                            seal(kind::FILE_CHUNK, &r)
                        }
                        k => panic!("frame {k}"),
                    };
                    c.write_all(&reply).unwrap();
                }
            }
            asked
        });
        let service = Service { host: "127.0.0.1", port, ..VIEWER };
        let root = std::env::temp_dir().join(format!("vana-patch-viewer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let server_version = Session::open(service, None, None).unwrap().version(None).unwrap();
        assert_eq!(installed_version(&root, VIEWER), None);
        let plan = plan_for(service, &root, true, server_version).unwrap();
        assert_eq!(plan.todo.len(), 2);
        apply(&root, &plan, 1, &|_, _| {}).unwrap();
        let mut asked = server.join().unwrap();
        asked.sort();
        assert_eq!(asked, ["data/x.txt", "viewer/com/app.dll"]);
        assert_eq!(std::fs::read(root.join("viewer/com/app.dll")).unwrap(), b"MZ app");
        assert!(!root.join("EU/env.dat").exists());
        assert_eq!(installed_version(&root, VIEWER).as_deref(), Some("20261022_0"));
        // The same plan as the game's writes no patch.ver (its code files stay behind).
        std::fs::remove_file(root.join(PATCH_VER)).unwrap();
        let game = Plan { service: GAME, todo: Vec::new(), ..plan };
        apply(&root, &game, 1, &|_, _| {}).unwrap();
        assert!(!root.join(PATCH_VER).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn frames_seal_and_open() {
        let f = file_request(&TAGS, "ROM/0/0.DAT", 0x10000, CHUNK);
        let (k, b) = open(&f).unwrap();
        assert_eq!((k, &b[16..20], &b[20..]), (kind::FILE_REQUEST, &12u32.to_le_bytes()[..], &b"ROM/0/0.DAT\0"[..]));
        let mut bad = f.clone();
        bad[20] ^= 1;
        assert!(open(&bad).is_err());
    }

    #[test]
    fn slc_and_olc() {
        let s = pack(&[(0, 1), (b'a' as u32, 8), (0, 1), (b'b' as u32, 8), (1, 1), (2, 16), (5, 8)]);
        assert_eq!(slc(&s).unwrap(), b"abababa");
        // 11 base copy of 3 at +4, literal '-', 10 output copy of 2 from 2 back.
        let o = pack(&[(0b11, 2), (4, 16), (3, 8), (0, 1), (b'-' as u32, 8), (0b01, 2), (2, 16), (2, 8)]);
        assert_eq!(olc(&o, b"0123456789").unwrap(), b"456-6-");
        assert!(slc(&pack(&[(1, 1), (1, 16), (1, 8)])).is_err());
    }

    #[test]
    fn manifest_and_decisions() {
        let text = "file Tools/a b.chm {\n1_0 3 0 0 1_0/Direct/Tools/a b.chm.slc 9\n2_0 4 1 2 2_0/Direct/Tools/a b.chm.slc 10 2_0/Indirect/Tools/a b.chm.olc 3\n}\nfile x.dll {\n1_0 1 1 1 1_0/Direct/x.dll.slc 5\n}\nend\n";
        let m = parse_manifest(text).unwrap();
        assert_eq!(
            (m.len(), m[0].path.as_str(), m[0].versions[1].delta.clone()),
            (2, "Tools/a b.chm", Some(("2_0/Indirect/Tools/a b.chm.olc".into(), 3)))
        );
        let old = m[0].versions[0].sig;
        assert_eq!(decide(&m[0], Some(old), true), (Action::Deltas(1), 3));
        // No version applied: the whole file, as retail without its patch.ver.
        assert_eq!(decide(&m[0], Some(old), false), (Action::Direct, 10));
        assert_eq!(decide(&m[0], Some(m[0].versions[1].sig), false), (Action::Current, 0));
        assert_eq!(decide(&m[0], None, true), (Action::Direct, 10));
        assert!(is_code(&m[1].path));
        let s = Sig::of(b"\xff\x01");
        assert_eq!((s.len, s.sum), (2, 0));
    }

    /// The version check carries the applied version; its reply's field holds the label and the download host.
    #[test]
    fn version_frames() {
        let f = version_request(&TAGS, Some("30261102_0"));
        let (k, b) = open(&f).unwrap();
        assert_eq!((k, f.len(), &b[..8], &b[8..19]), (kind::VERSION, 0x58, &TAGS[..], &b"30261102_0\0"[..]));
        assert!(b[19..].iter().all(|&c| c == 0));
        assert_eq!(open(&version_request(&TAGS, None)).unwrap().1[8..], [0; VERSION_BLOCK]);
        let mut r = vec![0u8; 8 + VERSION_BLOCK];
        r[..4].copy_from_slice(&1_793_600_228u32.to_le_bytes());
        r[8..30].copy_from_slice(b"registered\x00192.0.2.7\x000");
        r.extend_from_slice(&11u32.to_le_bytes());
        r.extend_from_slice(b"30261102_0\0");
        let v = version_reply(&r).unwrap();
        assert_eq!(
            (v.label.as_str(), v.host.as_deref(), v.version.as_str()),
            ("registered", Some("192.0.2.7"), "30261102_0")
        );
        r[8..8 + VERSION_BLOCK].fill(0);
        r[8..13].copy_from_slice(b"empty");
        assert_eq!(version_reply(&r).unwrap().host, None);
    }

    /// FUN_10017f3a: base 37, `_` skipped, other characters 0; state 0x26e's test on top.
    #[test]
    fn versions_order_as_retail() {
        assert_eq!(stamp_num("10_1"), 37 * 37 + 1);
        assert_eq!(stamp_num("1A"), 37 + 10);
        assert_eq!(stamp_num("1a 9"), 37);
        assert!(stamp_num("30261102_0") > stamp_num("30261006_0"));
        assert!(up_to_date("30261102_0", "30261102_0") && up_to_date("30261006_0", "30261102_0"));
        assert!(
            !up_to_date("30261102_0", "30261006_0")
                && !up_to_date("30261102_0", "")
                && !up_to_date("1_0", "30261102_0")
        );
    }

    #[test]
    fn manifest_blocks() {
        let line = "1_0 1 1 1 1_0/Direct/x.slc 5";
        let text = format!(
            "file a {{\n{line}\n}}\nReset {{\nfile b {{\n{line}\n}}\n}}\noption {{\nfile c {{\n{line}\n}}\nfile d {{\n{line}\n}}\n}}\nreset2 {{\nfile e {{\n{line}\n}}\n}}\nfile f {{\n{line}\n}}\nend\n"
        );
        let m = parse_manifest(&text).unwrap();
        let attrs: Vec<_> = m.iter().map(|e| e.attr).collect();
        assert_eq!(attrs, [Attr::None, Attr::Reset, Attr::Option, Attr::Option, Attr::Reset2, Attr::None]);
    }

    /// Retail's order for one file: no newer line = unread; the staged file first; an optional file not fetched.
    #[test]
    fn staged_files_and_the_rename_pass() {
        let root = std::env::temp_dir().join(format!("vana-patch-staged-{}", std::process::id()));
        std::fs::create_dir_all(root.join("ROM/1")).unwrap();
        let (v1, v2) = (b"old content".to_vec(), b"new content!".to_vec());
        let line = |stamp: &str, d: &[u8], delta: &str| {
            let s = Sig::of(d);
            format!("{stamp} {} {} {} {stamp}/Direct/ROM/1/2.DAT.slc 90{delta}\n", s.len, s.sum, s.md5)
        };
        let text = format!(
            "file ROM/1/2.DAT {{\n{}{}}}\noption {{\nfile ROM/1/9.DAT {{\n{}}}\n}}\nend\n",
            line("30261006_0", &v1, ""),
            line("30261102_0", &v2, " 30261102_0/Indirect/ROM/1/2.DAT.olc 7"),
            line("30261102_0", &v2, ""),
        );
        let m = parse_manifest(&text).unwrap();
        let (file, tmp2) = (root.join("ROM/1/2.DAT"), root.join("ROM/1/2.DAT.tmp2"));
        std::fs::write(&file, &v1).unwrap();
        // The optional file is not there: left out. The other one by its lines.
        assert_eq!(check(&root, &m[1], None, false).unwrap().0, (Action::Current, 0));
        assert_eq!(check(&root, &m[0], Some("30261006_0"), false).unwrap().0, (Action::Deltas(1), 7));
        assert_eq!(check(&root, &m[0], None, false).unwrap().0, (Action::Direct, 90));
        // Nothing newer than the applied version: not read at all (the file is not the current one).
        assert_eq!(check(&root, &m[0], Some("30261102_0"), false).unwrap().0, (Action::Current, 0));
        // A staged older version is the base; one that matches nothing is given back to be deleted.
        std::fs::write(&tmp2, &v1).unwrap();
        std::fs::write(&file, b"something else").unwrap();
        assert_eq!(check(&root, &m[0], Some("30261006_0"), false).unwrap(), ((Action::StagedDeltas(1), 7), None));
        std::fs::write(&tmp2, b"half a download").unwrap();
        assert_eq!(check(&root, &m[0], Some("30261006_0"), false).unwrap(), ((Action::Direct, 90), Some(tmp2.clone())));
        // The staged current version only moves into place, with no server.
        std::fs::write(&tmp2, &v2).unwrap();
        let found = check(&root, &m[0], Some("30261006_0"), false).unwrap();
        assert_eq!(found, ((Action::Staged, 0), None));
        let stale = root.join("ROM/1/3.DAT.tmp2");
        std::fs::write(&stale, b"x").unwrap();
        let plan = Plan {
            manifest: text.clone().into_bytes(),
            entries: m,
            todo: vec![(0, Action::Staged, 0)],
            stale: vec![stale.clone()],
            ..Default::default()
        };
        apply(&root, &plan, 2, &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), v2);
        assert!(!tmp2.exists() && !stale.exists());
        assert_eq!(applied_version(&root).as_deref(), Some("30261102_0"));
        // New content waits beside the file until the rename pass.
        stage(&file, &v1).unwrap();
        assert_eq!((std::fs::read(&file).unwrap(), std::fs::read(&tmp2).unwrap()), (v2, v1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// FFXiMain.dll goes to that install's copy, never over the game folder's.
    #[test]
    fn ffximain_to_the_install_copy() {
        let root = Path::new("/games/FINAL FANTASY XI");
        assert_eq!(target(root, "ffximain.DLL").unwrap(), root.join("vana-client/FFXiMain.dll"));
        assert_eq!(target(root, "ffximain.DLL").unwrap(), crate::dat::own_ffximain(root));
        assert_eq!(target(root, "ROM/0/0.DAT").unwrap(), root.join("ROM/0/0.DAT"));
        assert!(is_code("polboot.exe") && !is_ffximain("ToolsUS/polboot.exe") && !is_ffximain("Tools/FFXiMain.dll"));
    }

    /// The old data-dir copy moves to the install whose stamped manifest wants it, and only there.
    #[test]
    fn old_copy_adopted_by_its_install() {
        let base = std::env::temp_dir().join(format!("vana-patch-adopt-{}", std::process::id()));
        let (a, b, old) = (base.join("a"), base.join("b"), base.join("data/FFXiMain.dll"));
        let dll = b"MZ the current one".to_vec();
        let s = Sig::of(&dll);
        let stamp = |sig: Sig| {
            format!(
                "file FFXiMain.dll {{\n1_0 {} {} {} 1_0/Direct/FFXiMain.dll.slc 9\n}}\nend\n",
                sig.len, sig.sum, sig.md5
            )
        };
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(&old, &dll).unwrap();
        std::fs::write(a.join(STAMP_FILE), stamp(Sig::of(b"MZ another"))).unwrap();
        std::fs::write(b.join(STAMP_FILE), stamp(s)).unwrap();
        // Another install, and one with no stamp: left alone.
        assert!(!adopt(&old, &a).unwrap() && !adopt(&old, &base.join("none")).unwrap() && old.is_file());
        assert!(adopt(&old, &b).unwrap());
        assert_eq!(std::fs::read(crate::dat::ffximain(&b)).unwrap(), dll);
        assert_eq!(crate::dat::ffximain(&a), a.join("FFXiMain.dll"));
        assert!(!old.exists() && !adopt(&old, &b).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Our FFXiMain.dll copy lags when it misses the stamp, or when the Viewer patched past the stamp and our copy
    /// differs from the game folder's.
    #[test]
    fn own_ffximain_lags() {
        let root = std::env::temp_dir().join(format!("vana-patch-lags-{}", std::process::id()));
        let (old, new) = (b"MZ 30261006".to_vec(), b"MZ 30261102".to_vec());
        let s = Sig::of(&old);
        std::fs::create_dir_all(root.join("vana-client")).unwrap();
        // No stamp: nothing to compare with.
        assert!(!code_lags(&root));
        let stamp = format!(
            "file FFXiMain.dll {{\n30261006_0 {} {} {} 30261006_0/Direct/FFXiMain.dll.slc 9\n}}\nend\n",
            s.len, s.sum, s.md5
        );
        std::fs::write(root.join(STAMP_FILE), &stamp).unwrap();
        // The folder's 2019 copy behind the stamp (a data-only run), then our copy at the stamp.
        std::fs::write(root.join("FFXiMain.dll"), b"MZ 2019").unwrap();
        assert!(code_lags(&root));
        std::fs::write(crate::dat::own_ffximain(&root), &old).unwrap();
        assert!(!code_lags(&root));
        // The Viewer patched the folder past the stamp: our copy lags until it matches the folder's.
        std::fs::write(root.join("FFXiMain.dll"), &new).unwrap();
        std::fs::write(root.join(PATCH_VER), crate::net::pol::patchver::write("30261102_0", GAME.title)).unwrap();
        assert!(code_lags(&root));
        // The server's manifest has the Viewer's version: a lagging copy is read past the version skip.
        let n = Sig::of(&new);
        let server = stamp.replace(
            "}",
            &format!("30261102_0 {} {} {} 30261102_0/Direct/FFXiMain.dll.slc 9\n}}", n.len, n.sum, n.md5),
        );
        let e = &parse_manifest(&server).unwrap()[0];
        assert_eq!(check(&root, e, Some("30261102_0"), false).unwrap().0, (Action::Current, 0));
        assert_ne!(check(&root, e, Some("30261102_0"), true).unwrap().0, (Action::Current, 0));
        std::fs::write(crate::dat::own_ffximain(&root), &new).unwrap();
        assert!(!code_lags(&root));
        // With no copy of ours, the client reads the Viewer's.
        std::fs::remove_file(crate::dat::own_ffximain(&root)).unwrap();
        assert!(!code_lags(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn version_keys_sort_by_date_then_number() {
        assert_eq!(version_key("30261102_0"), Some((30261102, 0)));
        assert!(version_key("30261102_0") > version_key("30261006_0"));
        assert!(version_key("30261006_10") > version_key("30261006_9"));
        assert_eq!(version_key("custom"), None);
        assert_eq!(version_key(""), None);
        let base = std::env::temp_dir().join(format!("vana-patch-version-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let line = |v: &str| format!("{v} 1 1 1 {v}/Direct/x.slc 5\n");
        let text = format!("file ROM/0/0.DAT {{\n{}{}}}\nend\n", line("30261102_9"), line("30261102_10"));
        std::fs::write(base.join(STAMP_FILE), text).unwrap();
        assert_eq!(applied_version(&base).as_deref(), Some("30261102_10"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn manifest_paths_stay_under_the_root() {
        for ok in ["ROM/0/0.DAT", "ROM9\\12\\34.DAT", "Tools/a b.chm", "FFXiMain.dll", "polboot.exe"] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "\\Windows\\x.dll",
            "\\\\host\\share\\x",
            "C:\\x.exe",
            "C:x.exe",
            "ROM/../../x",
            "..\\x",
            "ROM/./0.DAT",
            "ROM//0.DAT",
            "ROM/",
            "a.DAT:stream",
            "ROM/.. /x",
            "ROM/x.",
            "ROM/NUL",
            "con.txt",
            "ROM/COM1.DAT",
            "a\0b",
        ] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
        let entry = |p: &str| format!("file {p} {{\n1_0 1 1 1 1_0/Direct/x.slc 5\n}}\nend\n");
        assert!(parse_manifest(&entry("ROM/0/0.DAT")).is_ok());
        for p in ["../../.config/autostart/x.desktop", "/tmp/x", "C:\\Users\\x\\Start Menu\\x.exe"] {
            assert!(parse_manifest(&entry(p)).is_err(), "{p}");
        }
        assert!(local(Path::new("/game"), "/etc/passwd").is_err());
        assert_eq!(local(Path::new("/game"), "ROM/0/0.DAT").unwrap(), Path::new("/game/ROM/0/0.DAT"));
    }

    #[cfg(unix)]
    #[test]
    fn writes_do_not_follow_links_out_of_the_root() {
        let base = std::env::temp_dir().join(format!("vana-patch-links-{}", std::process::id()));
        let (root, outside) = (base.join("game"), base.join("outside"));
        std::fs::create_dir_all(root.join("ROM/0")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("ROM2")).unwrap();
        assert_eq!(writable(&root, "ROM/0/0.DAT").unwrap(), root.join("ROM/0/0.DAT"));
        assert_eq!(writable(&root, "ROM3/1/2.DAT").unwrap(), root.join("ROM3/1/2.DAT"));
        assert!(writable(&root, "ROM2/1/2.DAT").is_err());
        // One file name on Unix, so the ROM2 link is not on its way.
        assert!(writable(&root, "ROM2\\1\\2.DAT").is_ok());
        // A linked root itself is fine: only folders below it are checked.
        std::os::unix::fs::symlink(&root, base.join("linked")).unwrap();
        assert!(writable(&base.join("linked"), "ROM/0/0.DAT").is_ok());
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// A staged file that matches no version is queued for removal only when no link leads out of the install.
    #[cfg(unix)]
    #[test]
    fn stale_staged_file_behind_a_link_is_left() {
        let base = std::env::temp_dir().join(format!("vana-patch-stale-{}", std::process::id()));
        let (root, outside) = (base.join("game"), base.join("outside"));
        std::fs::create_dir_all(root.join("ROM")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("ROM2")).unwrap();
        let (a, b) = (Sig::of(b"one"), Sig::of(b"two"));
        let line = |v: &str, s: Sig| format!("{v} {} {} {} {v}/Direct/x.slc 5\n", s.len, s.sum, s.md5);
        let text = |p: &str| format!("file {p} {{\n{}{}}}\nend\n", line("1_0", a), line("2_0", b));
        for (path, stale) in [("ROM/1.DAT", true), ("ROM2/1.DAT", false)] {
            let m = parse_manifest(&text(path)).unwrap();
            std::fs::write(root.join(format!("{path}.tmp2")), b"matches nothing").unwrap();
            let (_, old) = check(&root, &m[0], None, false).unwrap();
            assert_eq!(old.is_some(), stale, "{path}");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn forgetting_the_applied_manifest() {
        let root = std::env::temp_dir().join(format!("vana-patch-forget-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        forget_applied(&root).unwrap();
        let line = "1_0 1 1 1 1_0/Direct/x.slc 5\n";
        std::fs::write(root.join(STAMP_FILE), format!("file ROM/0/0.DAT {{\n{line}}}\nend\n")).unwrap();
        assert_eq!(applied_version(&root).as_deref(), Some("1_0"));
        forget_applied(&root).unwrap();
        assert_eq!(applied_version(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
