//! BGMStream (.bgw music) and SeWave (.spw effects): header, ADPCM / PCM / ATRAC3 decoding and looping
//! (FFXiMain FUN_102408b0, FUN_10240090, FUN_1023c620, FUN_1023cb20, FUN_1023c3a0).

use super::atrac3::{self, Atrac3};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// ADPCM predictor pairs (prev1, prev2) / 64, FFXiMain .data 0x1037c538.
const COEF: [[i32; 2]; 5] = [[0, 0], [60, 0], [115, -52], [98, -55], [122, -60]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// 4-bit ADPCM: per channel a header byte (filter << 4 | shift) and block / 2 bytes.
    Adpcm { block: u8 },
    Pcm16 { big_endian: bool },
    /// XOR-scrambled ATRAC3 (mode byte: 1 = 0x130-byte frames, 2 = 0xc0 joint stereo, else 0x180).
    Atrac3 { mode: u8, key: u32 },
}

#[derive(Clone, Debug)]
pub struct Header {
    pub music: bool,
    pub codec: Codec,
    pub id: u32,
    pub channels: u8,
    pub rate: u32,
    /// Length in units: ADPCM blocks, otherwise sample frames.
    pub length: u32,
    pub loop_start: Option<u32>,
    pub data: usize,
    /// bgw 0x2c: the track's own volume level; spw 0x28 (used when flags bit 7 is set).
    pub volume: u8,
    /// spw 0x29 (0x80 own volume, 0x20 fade in, 0x08 fixed volume); 0 for music.
    pub flags: u8,
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Self> {
        if b.len() < 0x30 {
            bail!("sound file shorter than its header");
        }
        let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        if &b[..4] == b"RIFF" && &b[8..12] == b"WAVE" {
            return Self::parse_wav(b);
        }
        let music = &b[..10] == b"BGMStream\0";
        if !music && &b[..7] != b"SeWave\0" {
            bail!("not a BGMStream or SeWave file");
        }
        // SeWave fields sit 4 bytes before BGMStream's from the id on.
        let s = if music { 0 } else { 4 };
        let (ch, bits, mode) = if music { (b[0x2e], b[0x2d], b[0x2f]) } else { (b[0x2a], b[0x2b], b[0x2c]) };
        let rate = u(0x20 - s).wrapping_add(u(0x24 - s)) & 0x7fff_ffff;
        let codec = match u(0x0c) {
            0 => Codec::Adpcm { block: mode },
            1 => Codec::Pcm16 { big_endian: false },
            2 => Codec::Pcm16 { big_endian: true },
            3 => Codec::Atrac3 { mode, key: u(0x24 - s) },
            f => bail!("unknown sound format {f}"),
        };
        if ch == 0 || ch > 2 || rate == 0 || (matches!(codec, Codec::Pcm16 { .. }) && bits != 16) {
            bail!("unsupported layout: {ch} channels, {bits} bits, {rate} Hz");
        }
        if let Codec::Adpcm { block } = codec
            && (block < 2 || block % 2 != 0)
        {
            bail!("bad ADPCM block size {block}");
        }
        let loop_raw = u(0x1c - s) as i32;
        Ok(Self {
            music,
            codec,
            id: u(0x14 - s),
            channels: ch,
            rate,
            length: u(0x18 - s),
            loop_start: (loop_raw >= 0).then_some(loop_raw as u32),
            data: u(0x28 - s) as usize,
            volume: if music { b[0x2c] } else { b[0x28] },
            flags: if music { 0 } else { b[0x29] },
        })
    }

    /// A plain RIFF WAVE played as music (mov/music999.bgw; FUN_1023f3c0 / FUN_1023e4d0): PCM16, id 0x400,
    /// volume 0x7f. Its 'cue ' loop is not read.
    fn parse_wav(b: &[u8]) -> Result<Self> {
        let u = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0);
        let (mut at, mut fmt, mut data) = (12, None, None);
        while at + 8 <= b.len() {
            let size = u(at + 4) as usize;
            match &b[at..at + 4] {
                b"fmt " if at + 24 <= b.len() => fmt = Some((u16::from_le_bytes([b[at + 10], b[at + 11]]), u(at + 12), u16::from_le_bytes([b[at + 22], b[at + 23]]))),
                b"data" => data = Some((at + 8, size.min(b.len() - at - 8))),
                _ => {}
            }
            at += 8 + size + (size & 1);
        }
        let (Some((ch, rate, bits)), Some((data, len))) = (fmt, data) else { bail!("RIFF without fmt/data") };
        if bits != 16 || !(1..=2).contains(&ch) {
            bail!("unsupported WAV: {ch} channels, {bits} bits");
        }
        Ok(Self { music: true, codec: Codec::Pcm16 { big_endian: false }, id: 0x400, channels: ch as u8, rate, length: (len / (2 * ch as usize)) as u32, loop_start: None, data, volume: 0x7f, flags: 0 })
    }

    /// Sample frames per length unit.
    pub fn unit(&self) -> u32 {
        match self.codec {
            Codec::Adpcm { block } => block as u32,
            _ => 1,
        }
    }

    pub fn frames(&self) -> u64 {
        self.length as u64 * self.unit() as u64
    }

    pub fn loop_frame(&self) -> Option<u64> {
        self.loop_start.map(|l| l as u64 * self.unit() as u64)
    }

    /// ATRAC3 frame size in bytes (FUN_1023d230 / FUN_1023d9b0; mono effects use half).
    pub fn atrac_frame_bytes(&self) -> usize {
        let Codec::Atrac3 { mode, .. } = self.codec else { return 0 };
        let full = match mode {
            1 => 0x130,
            2 => 0xc0,
            _ => 0x180,
        };
        if !self.music && self.channels != 2 { full / 2 } else { full }
    }
}

/// Streams one file's samples, looping as the client does (ADPCM history and the ATRAC3 state carry
/// across the loop; ATRAC3 re-primes with the frame before the loop frame).
pub struct Decoder {
    pub header: Header,
    bytes: Arc<[u8]>,
    /// Next unit (ADPCM block / PCM frame) or, for ATRAC3, the next sample frame.
    unit: u64,
    hist: [[i32; 2]; 2],
    at3: Option<(Box<Atrac3>, Box<[u8; 400]>)>,
    /// ATRAC3 frame the decoder consumes next.
    at3_next: i64,
    pending: Vec<i16>,
    pending_at: usize,
    ended: bool,
    pub loops: bool,
}

impl Decoder {
    pub fn new(bytes: Arc<[u8]>) -> Result<Self> {
        let header = Header::parse(&bytes)?;
        let at3 = match header.codec {
            Codec::Atrac3 { mode, key } => Some((Box::new(Atrac3::new(header.channels, mode)), Box::new(atrac3::key_stream(key)))),
            _ => None,
        };
        let loops = header.loop_start.is_some();
        Ok(Self { header, bytes, unit: 0, hist: [[0; 2]; 2], at3, at3_next: 0, pending: Vec::new(), pending_at: 0, ended: false, loops })
    }

    pub fn channels(&self) -> usize {
        self.header.channels as usize
    }

    /// Starts from a saved position (in units, as the client's resume table keeps it).
    pub fn seek_unit(&mut self, unit: u64) {
        if unit < self.header.length as u64 {
            self.unit = unit;
            self.pending.clear();
            self.pending_at = 0;
        }
    }

    pub fn unit_position(&self) -> u64 {
        self.unit
    }

    /// Fills interleaved samples; returns frames written (fewer only at the end of a one-shot sound).
    pub fn read(&mut self, out: &mut [i16]) -> usize {
        let ch = self.channels();
        let want = out.len() / ch * ch;
        let mut n = 0;
        while n < want {
            if self.pending_at >= self.pending.len() {
                self.pending.clear();
                self.pending_at = 0;
                if self.ended || !self.refill() {
                    self.ended = true;
                    break;
                }
            }
            let take = (self.pending.len() - self.pending_at).min(want - n);
            out[n..n + take].copy_from_slice(&self.pending[self.pending_at..self.pending_at + take]);
            self.pending_at += take;
            n += take;
        }
        n / ch
    }

    pub fn ended(&self) -> bool {
        self.ended && self.pending_at >= self.pending.len()
    }

    /// Decodes the next block or frame into `pending`; false at the end.
    fn refill(&mut self) -> bool {
        if self.at3.is_some() {
            return self.refill_atrac();
        }
        let h = &self.header;
        let len = h.length as u64;
        if self.unit >= len {
            match h.loop_start {
                Some(l) if self.loops && (l as u64) < len => self.unit = l as u64,
                _ => return false,
            }
        }
        let ch = h.channels as usize;
        match h.codec {
            Codec::Adpcm { block } => {
                let bs = block as usize;
                let per = bs / 2 + 1;
                let at = h.data + self.unit as usize * per * ch;
                let Some(src) = self.bytes.get(at..at + per * ch) else { return false };
                self.pending.resize(bs * ch, 0);
                for c in 0..ch {
                    decode_block(&src[c * per..(c + 1) * per], &mut self.hist[c], &mut self.pending, c, ch);
                }
            }
            Codec::Pcm16 { big_endian } => {
                let at = h.data + self.unit as usize * 2 * ch;
                let Some(src) = self.bytes.get(at..at + 2 * ch) else { return false };
                for s in src.chunks_exact(2) {
                    let b = [s[0], s[1]];
                    self.pending.push(if big_endian { i16::from_be_bytes(b) } else { i16::from_le_bytes(b) });
                }
            }
            Codec::Atrac3 { .. } => unreachable!(),
        }
        self.unit += 1;
        true
    }

    fn refill_atrac(&mut self) -> bool {
        let h = &self.header;
        let len = h.length as u64;
        if self.unit >= len {
            match h.loop_start {
                Some(l) if self.loops && (l as u64) < len => self.unit = l as u64,
                _ => return false,
            }
        }
        let fb = h.atrac_frame_bytes();
        let frame = (self.unit >> 10) as i64;
        let (dec, key) = self.at3.as_mut().unwrap();
        let mut feed = |k: i64, out: &mut Vec<i16>| -> bool {
            let at = h.data + k as usize * fb;
            let Some(src) = self.bytes.get(at..at + fb) else { return false };
            let mut buf = [0u8; 0x180];
            for (i, b) in src.iter().enumerate() {
                buf[i] = b ^ key[i];
            }
            dec.decode_frame(&buf[..fb], out);
            true
        };
        if frame != self.at3_next {
            // A jump (loop or resume): prime with the previous frame, as FUN_1023c620 does.
            let mut scratch = Vec::new();
            if frame > 0 && !feed(frame - 1, &mut scratch) {
                return false;
            }
        }
        let mut out = std::mem::take(&mut self.pending);
        out.clear();
        if !feed(frame, &mut out) {
            return false;
        }
        self.at3_next = frame + 1;
        let ch = h.channels as usize;
        let skip = (self.unit & 0x3ff) as usize;
        // Never play past the track length inside the last frame.
        let frames_here = (1024 - skip).min((len - self.unit) as usize);
        self.pending = out;
        self.pending_at = skip * ch;
        self.pending.truncate((skip + frames_here) * ch);
        self.unit += frames_here as u64;
        true
    }
}

/// One channel's ADPCM block into interleaved `out` (FUN_1023c3a0: low nibble first, no clamping; the
/// history keeps the unclamped values).
fn decode_block(src: &[u8], hist: &mut [i32; 2], out: &mut [i16], c: usize, ch: usize) {
    let shift = (src[0] & 0xf) as u32;
    let [k0, k1] = COEF.get((src[0] >> 4) as usize).copied().unwrap_or([0, 0]);
    let mut i = 0;
    for &b in &src[1..] {
        for nib in [(b as u16) << 12, ((b & 0xf0) as u16) << 8] {
            let v = ((k1 * hist[1] + k0 * hist[0]) >> 6) + ((nib as i16 as i32) >> shift);
            hist[1] = hist[0];
            hist[0] = v;
            out[i * ch + c] = v as i16;
            i += 1;
        }
    }
}

/// The client's sound roots in search order: `sound`, `sound2` .. `sound9` (FFXiMain 0x10346608), each enabled mod's
/// before the game's (not retail).
pub fn sound_dirs(ffxi_dir: &Path) -> Vec<PathBuf> {
    let mut roots = crate::config::enabled_mod_dirs();
    roots.push(ffxi_dir.to_path_buf());
    sound_dirs_in(&roots)
}

fn sound_dirs_in(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().flat_map(|r| (1..=9).map(move |n| r.join(if n == 1 { "sound".to_string() } else { format!("sound{n}") }).join("win"))).collect()
}

/// `music%03u.bgw` from the first root that has it (FUN_1023bed0; id & 0x3ff); id 0x400 is the opening movie's
/// `mov/music999.bgw` (FUN_1023bce0 plays it by path).
pub fn music_path(ffxi_dir: &Path, id: u32) -> Option<PathBuf> {
    if id == 0x400 {
        return Some(ffxi_dir.join("mov").join("music999.bgw")).filter(|p| p.exists());
    }
    sound_dirs(ffxi_dir).into_iter().map(|d| d.join("music/data").join(format!("music{:03}.bgw", id & 0x3ff))).find(|p| p.exists())
}

/// `se%03u/se%06u.spw` (FUN_1023f6f0).
pub fn se_path(ffxi_dir: &Path, id: u32) -> Option<PathBuf> {
    sound_dirs(ffxi_dir).into_iter().map(|d| d.join("se").join(format!("se{:03}", id / 1000)).join(format!("se{id:06}.spw"))).find(|p| p.exists())
}

/// 16-bit PCM WAV with a `smpl` loop (start, end) when the sound loops.
pub fn write_wav(path: &Path, samples: &[i16], channels: u16, rate: u32, looped: Option<(u64, u64)>) -> Result<()> {
    let data_len = samples.len() * 2;
    let smpl_len = if looped.is_some() { 36 + 24 } else { 0 };
    let mut w = Vec::with_capacity(44 + data_len + smpl_len + 8);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + data_len + if smpl_len > 0 { smpl_len + 8 } else { 0 }) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    if let Some((start, end)) = looped {
        w.extend_from_slice(b"smpl");
        w.extend_from_slice(&(smpl_len as u32).to_le_bytes());
        let period = (1_000_000_000u64 / rate as u64) as u32;
        for v in [0u32, 0, period, 60, 0, 0, 0, 1, 0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0u32, 0, start as u32, end.saturating_sub(1) as u32, 0, 0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, w)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mod's sound folders come before the game's, each in sound, sound2 .. sound9 order.
    #[test]
    fn mod_sound_first() {
        let d = sound_dirs_in(&[PathBuf::from("/m"), PathBuf::from("/g")]);
        assert_eq!(d.len(), 18);
        assert_eq!(d[0], PathBuf::from("/m/sound/win"));
        assert_eq!(d[1], PathBuf::from("/m/sound2/win"));
        assert_eq!(d[9], PathBuf::from("/g/sound/win"));
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "bgw" || x == "spw") {
                out.push(p);
            }
        }
    }

    /// Every .bgw / .spw in the install parses and its start decodes (needs the game files).
    #[test]
    #[ignore]
    fn every_installed_sound_parses() {
        let mut files = Vec::new();
        walk(&crate::config::Config::load().ffxi_dir(), &mut files);
        assert!(files.len() > 10000);
        let mut bad = Vec::new();
        for p in &files {
            let bytes: Arc<[u8]> = std::fs::read(p).unwrap().into();
            match Decoder::new(bytes) {
                Ok(mut d) => {
                    let mut buf = vec![0i16; 4096 * d.channels()];
                    if d.read(&mut buf) == 0 {
                        bad.push(format!("{}: no samples", p.display()));
                    }
                }
                Err(e) => bad.push(format!("{}: {e}", p.display())),
            }
        }
        assert!(bad.is_empty(), "{} of {} failed: {:?}", bad.len(), files.len(), &bad[..bad.len().min(10)]);
    }

    #[test]
    fn adpcm_block_matches_hand_decode() {
        // Filter 1 (60/64), shift 12: nibble 1 -> 1, then 1*60>>6 + 1 = 1 (wraps nothing).
        let src = [0x1c, 0x11, 0x00];
        let mut hist = [0; 2];
        let mut out = [0i16; 4];
        decode_block(&src, &mut hist, &mut out, 0, 1);
        assert_eq!(out, [1, 1, 0, 0]);
        // Shift 0 makes 0x7 << 12 = 28672 per step; the history is not clamped.
        let src = [0x10, 0x77];
        let mut hist = [0; 2];
        let mut out = [0i16; 2];
        decode_block(&src, &mut hist, &mut out, 0, 1);
        assert_eq!(hist[0], 28672 * 60 / 64 + 28672);
        assert_eq!(out[1], (28672 * 60 / 64 + 28672) as i16);
    }
}
