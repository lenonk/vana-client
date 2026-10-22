//! Game file updates from the PlayOnline patch server, as the Viewer's version-up did (polcore.dll / app.dll).
//! Protocol facts from Kuluu's reverse engineering of viewer 1.18.15e (research notes, not its code; MODLOG
//! "Game file updates"): POLP frames on TCP to pc001.pol.com:53001 for FFXI, no login.
//!
//! Flow: a version check (frame 7 / 8) gives the server's version; a hello (1) answers with the whole manifest
//! (2: status byte, then `.slc`); file requests (3) answer in chunks (4). Each manifest file lists its versions
//! with a signature (length, signed byte sum, MD5 word); a local file matching an older version takes the `.olc`
//! deltas when they are smaller than the `.slc` of the current one.

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const HOST: &str = "pc001.pol.com";
pub const PORT: u16 = 53001;
/// Platform and title tags every client frame carries: Windows, title 1.
const TAGS: [u8; 8] = *b"W20\x000001";
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

fn file_request(path: &str, offset: u32, len: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(21 + path.len());
    b.extend_from_slice(&offset.to_le_bytes());
    b.extend_from_slice(&len.to_le_bytes());
    b.extend_from_slice(&TAGS);
    b.extend_from_slice(&(path.len() as u32 + 1).to_le_bytes());
    b.extend_from_slice(path.as_bytes());
    b.push(0);
    seal(kind::FILE_REQUEST, &b)
}

/// The version reply: release time, flags, a label ("empty" for a blank block) and the version string.
#[derive(Debug, Clone)]
pub struct ServerVersion {
    pub released: u32,
    pub label: String,
    pub version: String,
}

fn version_reply(b: &[u8]) -> Result<ServerVersion> {
    let at = 8 + VERSION_BLOCK;
    if b.len() < at + 4 {
        bail!("short version reply");
    }
    let n = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    let text = b.get(at + 4..at + 4 + n).ok_or_else(|| anyhow!("version string truncated"))?;
    let cstr = |s: &[u8]| String::from_utf8_lossy(s.split(|&c| c == 0).next().unwrap_or_default()).into_owned();
    Ok(ServerVersion { released: u32::from_le_bytes(b[..4].try_into().unwrap()), label: cstr(&b[8..at]), version: cstr(text) })
}

pub struct Session(TcpStream);

impl Session {
    pub fn connect() -> Result<Self> {
        let s = TcpStream::connect((HOST, PORT)).with_context(|| format!("connecting to {HOST}:{PORT}"))?;
        s.set_read_timeout(Some(Duration::from_secs(120)))?;
        s.set_write_timeout(Some(Duration::from_secs(120)))?;
        Ok(Self(s))
    }

    fn exchange(&mut self, req: &[u8], want: u32) -> Result<Vec<u8>> {
        self.0.write_all(req)?;
        let mut head = [0u8; 4];
        self.0.read_exact(&mut head)?;
        let total = u32::from_le_bytes(head) as usize;
        if !(HEADER..=1 << 30).contains(&total) {
            bail!("patch server announced a {total}-byte frame");
        }
        let mut f = vec![0u8; total];
        f[..4].copy_from_slice(&head);
        self.0.read_exact(&mut f[4..])?;
        let (k, _) = open(&f)?;
        match k {
            k if k == want => Ok(f),
            kind::REJECT => bail!("patch server rejected the request"),
            k => bail!("patch server answered with frame {k}, expected {want}"),
        }
    }

    /// An all-zero version block asks for the current version (the server labels it "empty").
    pub fn version(&mut self) -> Result<ServerVersion> {
        let mut b = TAGS.to_vec();
        b.extend_from_slice(&[0; VERSION_BLOCK]);
        let f = self.exchange(&seal(kind::VERSION, &b), kind::VERSION_REPLY)?;
        version_reply(&f[HEADER..])
    }

    /// The manifest text: status byte 2, then `.slc`.
    pub fn manifest(&mut self) -> Result<Vec<u8>> {
        let f = self.exchange(&seal(kind::HELLO, &TAGS), kind::MANIFEST)?;
        match f.get(HEADER) {
            Some(2) => slc(&f[HEADER + 1..]),
            s => bail!("manifest refused (status {s:?})"),
        }
    }

    /// `len` bytes of a server path in 64 KiB chunks (body: offset, len, extra, [extra bytes], data).
    pub fn fetch(&mut self, path: &str, len: u64) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len as usize);
        while (out.len() as u64) < len {
            let offset = out.len() as u32;
            let want = ((len - out.len() as u64) as u32).min(CHUNK);
            let f = self.exchange(&file_request(path, offset, want), kind::FILE_CHUNK)?;
            let b = &f[HEADER..];
            let u = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
            let (Some(got), Some(n), Some(extra)) = (u(0), u(4), u(8)) else { bail!("{path}: short chunk") };
            let data = b.get(12 + extra as usize..12 + extra as usize + n as usize).ok_or_else(|| anyhow!("{path}: chunk overruns its frame"))?;
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
        let end = u32::from_le_bytes(coded.get(..4).ok_or_else(|| anyhow!("coded stream too short"))?.try_into().unwrap()) as usize;
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
            let src = usize::try_from(at).ok().and_then(|a| base.get(a..a + n)).ok_or_else(|| anyhow!("base reference {at} outside {} bytes", base.len()))?;
            out.extend_from_slice(src);
        }
    }
    Ok(out)
}

/// A download: flag byte 1 stored, 3 zlib, else `.slc`.
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

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    /// Oldest first; the last is current.
    pub versions: Vec<Version>,
}

/// `file <path> {`, version lines `stamp len sum md5 <direct>.slc len [<delta>.olc len]`, `}`, until `end`.
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
        let (stamp, len, sum, md5, rest) = (it.next()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?);
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
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        match open.as_mut() {
            None if l == "end" => break,
            None => {
                let p = l.strip_prefix("file ").and_then(|s| s.strip_suffix(" {")).ok_or_else(|| anyhow!("manifest line {}: {l:?}", i + 1))?;
                open = Some(Entry { path: p.into(), versions: Vec::new() });
            }
            Some(_) if l == "}" => {
                let e = open.take().unwrap();
                if !e.versions.is_empty() {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Current,
    /// The current version whole.
    Direct,
    /// Deltas from this version index on.
    Deltas(usize),
}

/// Newest matching line: current, else its delta chain when every step has one and the sum is smaller than the
/// whole file, else the whole file.
pub fn decide(e: &Entry, local: Option<Sig>) -> (Action, u64) {
    let cur = e.versions.last().expect("entries have versions");
    let whole = (Action::Direct, cur.direct.1);
    let Some(i) = local.and_then(|l| e.versions.iter().rposition(|v| v.sig == l)) else { return whole };
    if i + 1 == e.versions.len() {
        return (Action::Current, 0);
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

/// Executables and libraries. The client reads its tables out of the current FFXiMain.dll, so they are patched too
/// unless asked not to.
pub fn is_code(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".dll") || p.ends_with(".exe")
}

pub struct Plan {
    pub server: ServerVersion,
    pub manifest: Vec<u8>,
    pub entries: Vec<Entry>,
    /// (entry index, action, bytes) for every file to change.
    pub todo: Vec<(usize, Action, u64)>,
    pub current: usize,
    pub skipped_code: usize,
}

fn sig_of_path(p: &Path) -> Result<Option<Sig>> {
    if !p.is_file() {
        return Ok(None);
    }
    Ok(Some(Sig::of(&std::fs::read(p).with_context(|| p.display().to_string())?)))
}

/// The version of the manifest last applied here (our stamp file).
pub fn applied_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(STAMP_FILE)).ok()?;
    parse_manifest(&text).ok()?.iter().flat_map(|e| e.versions.iter().map(|v| v.stamp.clone())).max()
}

/// The start-up check: None when the server's version is the one applied last and its code files are in place
/// (installs once patched with data files only keep a 2019 FFXiMain.dll), else the plan.
pub fn check(root: &Path) -> Result<Option<Plan>> {
    let server = Session::connect()?.version()?;
    if applied_version(root).as_deref() == Some(server.version.as_str()) && code_current(root) {
        return Ok(None);
    }
    plan_for(root, true, server).map(Some)
}

/// Every code file of the stamped manifest matches its current version.
fn code_current(root: &Path) -> bool {
    let Some(entries) = std::fs::read_to_string(root.join(STAMP_FILE)).ok().and_then(|t| parse_manifest(&t).ok()) else { return false };
    entries.iter().filter(|e| is_code(&e.path)).all(|e| sig_of_path(&root.join(&e.path)).ok().flatten().is_some_and(|l| decide(e, Some(l)).0 == Action::Current))
}

/// Asks the server for its version and manifest and checks every listed file under `root` (in parallel).
pub fn plan(root: &Path, code: bool) -> Result<Plan> {
    plan_for(root, code, Session::connect()?.version()?)
}

fn plan_for(root: &Path, code: bool, server: ServerVersion) -> Result<Plan> {
    let manifest = Session::connect()?.manifest()?;
    let entries = parse_manifest(&String::from_utf8_lossy(&manifest))?;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let found: std::sync::Mutex<Vec<(usize, Action, u64)>> = Default::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    std::thread::scope(|s| -> Result<()> {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| -> Result<()> {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(e) = entries.get(i) else { return Ok(()) };
                        let d = decide(e, sig_of_path(&root.join(&e.path))?);
                        found.lock().unwrap().push((i, d.0, d.1));
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|w| w.join().map_err(|_| anyhow!("scan thread panicked"))?)
    })?;
    let mut all = found.into_inner().unwrap();
    all.sort_by_key(|x| x.0);
    let current = all.iter().filter(|x| x.1 == Action::Current).count();
    let skipped_code = all.iter().filter(|x| x.1 != Action::Current && !code && is_code(&entries[x.0].path)).count();
    let todo = all.into_iter().filter(|x| x.1 != Action::Current && (code || !is_code(&entries[x.0].path))).collect();
    Ok(Plan { server, manifest, entries, todo, current, skipped_code })
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
        Action::Current => unreachable!(),
        Action::Direct => direct(&s.fetch(&cur.direct.0, cur.direct.1)?).with_context(|| cur.direct.0.clone())?,
        Action::Deltas(from) => {
            let mut d = std::fs::read(root.join(&e.path))?;
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
    let e = entries.iter().find(|e| e.path.eq_ignore_ascii_case(path)).ok_or_else(|| anyhow!("{path} is not in the manifest"))?;
    let data = produce(&mut Session::connect()?, dir, e, Action::Direct)?;
    let out = dir.join(&e.path);
    write_atomic(&out, &data)?;
    Ok((out, e.versions.last().map(|v| v.stamp.clone()).unwrap_or_default()))
}

/// Fetches and writes every planned file on `connections` sessions, then records the manifest. `progress` gets
/// (files done, bytes done).
pub fn apply(root: &Path, plan: &Plan, connections: usize, progress: &(dyn Fn(usize, u64) + Sync)) -> Result<()> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (files, bytes) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicU64::new(0));
    let failed: std::sync::Mutex<Option<anyhow::Error>> = Default::default();
    std::thread::scope(|s| {
        for _ in 0..connections.clamp(1, 6).min(plan.todo.len().max(1)) {
            s.spawn(|| {
                let mut sess = match Session::connect() {
                    Ok(x) => x,
                    Err(e) => return *failed.lock().unwrap() = Some(e),
                };
                loop {
                    if failed.lock().unwrap().is_some() {
                        return;
                    }
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(ei, a, n)) = plan.todo.get(i) else { return };
                    let e = &plan.entries[ei];
                    match produce(&mut sess, root, e, a).and_then(|d| write_atomic(&root.join(&e.path), &d)) {
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
    write_atomic(&root.join(STAMP_FILE), &plan.manifest)
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

    #[test]
    fn frames_seal_and_open() {
        let f = file_request("ROM/0/0.DAT", 0x10000, CHUNK);
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
        assert_eq!((m.len(), m[0].path.as_str(), m[0].versions[1].delta.clone()), (2, "Tools/a b.chm", Some(("2_0/Indirect/Tools/a b.chm.olc".into(), 3))));
        let old = m[0].versions[0].sig;
        assert_eq!(decide(&m[0], Some(old)), (Action::Deltas(1), 3));
        assert_eq!(decide(&m[0], Some(m[0].versions[1].sig)), (Action::Current, 0));
        assert_eq!(decide(&m[0], None), (Action::Direct, 10));
        assert!(is_code(&m[1].path));
        let s = Sig::of(b"\xff\x01");
        assert_eq!((s.len, s.sum), (2, 0));
    }
}
