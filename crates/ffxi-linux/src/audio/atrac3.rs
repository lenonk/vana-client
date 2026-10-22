//! ATRAC3 decoder ported from FFXiMain's own: setup FUN_102bf950 / FUN_102c2c20, frame FUN_102bfb30 ->
//! FUN_102c44e0, sound unit FUN_102c4640 (gain, tonal FUN_102c4bc0, spectrum FUN_102c4fc0, mantissas
//! FUN_102c4da0), IMDCT + gain FUN_102c2f40, joint stereo FUN_102c4970, QMF FUN_102c3960. Tables are
//! copied from the DLL (addresses on each).

/// Descrambling stream (FUN_1023c310); every frame is XORed from its start.
pub fn key_stream(key: u32) -> [u8; 400] {
    let mut k = key.rotate_left(11);
    let mut out = [0u8; 400];
    for c in out.chunks_exact_mut(4) {
        c.copy_from_slice(&k.to_le_bytes());
        k = k.wrapping_add(k.rotate_left(3));
    }
    out
}

const PAD: usize = 8;
/// QMF buffer: 0x2e history, 23 x 4 input history, 256 x 4 new band samples.
const BUF: usize = 0x48a;
const NEW: usize = 0x8a;

type Gains = [[(u32, u32); 8]; 4];
const NO_GAIN: Gains = [[(0xff, 4); 8]; 4];

#[inline]
fn f(b: u32) -> f32 {
    f32::from_bits(b)
}

struct Channel {
    prev_bands: usize,
    buf: Vec<f32>,
    overlap: [[f32; 4]; 128],
    /// Previous frame's gain points {loc * 8, level}, terminated by (0xff, 4).
    gains: Gains,
}

impl Channel {
    fn new() -> Self {
        Self { prev_bands: 0, buf: vec![0.0; BUF], overlap: [[0.0; 4]; 128], gains: NO_GAIN }
    }
}

/// Joint-stereo side info (state +0x00..+0x2c): matrix codes, weight index, unit-2 band count.
#[derive(Clone, Copy)]
struct Joint {
    m: [u8; 4],
    w: u8,
    nb: usize,
}

const JOINT_INIT: Joint = Joint { m: [3; 4], w: 0, nb: 4 };

pub struct Atrac3 {
    out_channels: usize,
    dec_channels: usize,
    joint: bool,
    /// Bytes per sound unit (dual stereo / mono), or the whole frame (joint).
    unit: usize,
    frame: usize,
    ch: [Channel; 2],
    js: Joint,
    js_prev: Joint,
    frames: u64,
    errors: u64,
    prime: bool,
    data: Vec<u8>,
    spec: Vec<f32>,
}

struct Bits<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    /// FUN_102c4b50: 16 - k bits at `pos` from a big-endian 16-bit window.
    fn peek16(&self, pos: usize, k: u32) -> u32 {
        let i = pos >> 3;
        let w = ((self.b[i] as u32) << 8) | self.b[i + 1] as u32;
        let sh = (k as i32 - (pos & 7) as i32) as u32 & 0x1f;
        (0xffff >> k) & (w >> sh)
    }

    /// FUN_102c4b90.
    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek16(self.pos, 16 - n);
        self.pos += n as usize;
        v
    }

    /// FUN_102c4d70.
    fn bit(&mut self) -> u32 {
        let v = (self.b[self.pos >> 3] >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        v as u32
    }
}

type Err = ();

fn vlc(set: u32, sel: usize) -> (&'static [i8], &'static [i8], u32) {
    match (set, sel) {
        (0, 0) => (&LUT0_0, &LEN0_0, BITS0[0] as u32),
        (0, 1) => (&LUT0_1, &LEN0_1, BITS0[1] as u32),
        (0, 2) => (&LUT0_2, &LEN0_2, BITS0[2] as u32),
        (0, 3) => (&LUT0_3, &LEN0_3, BITS0[3] as u32),
        (0, 4) => (&LUT0_4, &LEN0_4, BITS0[4] as u32),
        (0, 5) => (&LUT0_5, &LEN0_5, BITS0[5] as u32),
        (0, 6) => (&LUT0_6, &LEN0_6, BITS0[6] as u32),
        (_, 0) => (&LUT1_0, &LEN1_0, BITS1[0] as u32),
        (_, 1) => (&LUT1_1, &LEN1_1, BITS1[1] as u32),
        (_, 2) => (&LUT1_2, &LEN1_2, BITS1[2] as u32),
        (_, 3) => (&LUT1_3, &LEN1_3, BITS1[3] as u32),
        (_, 4) => (&LUT1_4, &LEN1_4, BITS1[4] as u32),
        (_, 5) => (&LUT1_5, &LEN1_5, BITS1[5] as u32),
        _ => (&LUT1_6, &LEN1_6, BITS1[6] as u32),
    }
}

fn values(sel: usize) -> &'static [u32] {
    match sel {
        1 => &VAL1,
        2 => &VAL2,
        3 => &VAL3,
        4 => &VAL4,
        5 => &VAL5,
        _ => &VAL6,
    }
}

/// FUN_102c4da0: adds sf * value for coefficients start..end (pairs for selector 0, 8 per step).
fn mantissas(bits: &mut Bits, set: u32, sel: usize, sf: f32, spec: &mut [f32], start: usize, end: usize) -> Result<(), Err> {
    let (lut, len, k) = vlc(set, sel);
    let mut i = start;
    if sel == 0 {
        let mask = u32::MAX >> k;
        let mut p = bits.pos;
        loop {
            let j = p >> 3;
            let w = u32::from_be_bytes([bits.b[j], bits.b[j + 1], bits.b[j + 2], bits.b[j + 3]]);
            let mut sh = k as i32 - (p & 7) as i32;
            for _ in 0..4 {
                let idx = lut[(w.wrapping_shr(sh as u32) & mask) as usize];
                if idx < 0 {
                    return Err(());
                }
                let idx = idx as usize;
                spec[i] += sf * f(PAIRS[idx]);
                spec[i + 1] += sf * f(PAIRS[idx + 1]);
                i += 2;
                sh -= len[idx] as i32;
            }
            p = ((p & !7) as i64 + k as i64 - sh as i64) as usize;
            if i >= end {
                bits.pos = p;
                return Ok(());
            }
        }
    }
    let vals = values(sel);
    loop {
        let idx = lut[bits.peek16(bits.pos, k) as usize];
        if idx < 0 {
            return Err(());
        }
        let idx = idx as usize;
        bits.pos += len[idx] as usize;
        spec[i] += sf * f(vals[idx]);
        i += 1;
        if i >= end {
            return Ok(());
        }
    }
}

/// FUN_102c4bc0: tonal components; returns the highest coefficient end seen.
fn tonal(bits: &mut Bits, count: u32, nb: usize, mut max_end: i32, spec: &mut [f32]) -> Result<i32, Err> {
    let sel = bits.read(2);
    if sel == 2 {
        return Err(());
    }
    let mut total = 0;
    for _ in 0..count {
        let p0 = bits.pos;
        let v = bits.peek16(p0, 9);
        let flags = ((-0x40i32 >> nb) as u32) & v;
        let rest = (v - flags) as i32;
        bits.pos = p0 + 4 + nb;
        let step = bits.read(3);
        if step < 2 {
            return Err(());
        }
        let mode = if sel > 1 { bits.bit() } else { sel };
        let mut blk: i32 = 0;
        let mut fl = flags;
        loop {
            if blk & 0xff == 0 {
                fl = fl.wrapping_mul(2);
                if (fl as u8 as i8) >= 0 {
                    if fl as u8 != 0 {
                        blk += 0x100;
                        continue;
                    }
                    break;
                }
            }
            let n = bits.read(3);
            for _ in 0..n {
                let sfi = bits.read(6) as usize;
                let p = bits.read(6) as i32;
                let end = (p + blk + (rest >> (3 - nb)) + 1).min(0x400);
                max_end = max_end.max(end);
                if total == 64 {
                    return Err(());
                }
                mantissas(bits, mode, step as usize - 1, f(SF[sfi]), spec, (p + blk) as usize, end as usize)?;
                total += 1;
            }
            blk += 0x40;
        }
    }
    Ok(max_end)
}

/// FUN_102c4fc0: subband spectrum; returns the end of the last subband.
fn spectrum(bits: &mut Bits, nsub: usize, spec: &mut [f32]) -> Result<i32, Err> {
    let mode = bits.bit();
    let mut sel = [0i32; 33];
    let mut nz = 0;
    for s in sel.iter_mut().take(nsub) {
        *s = bits.read(3) as i32 - 1;
        if *s >= 0 {
            nz += 6;
        }
    }
    let mut sfp = bits.pos;
    bits.pos += nz;
    for i in 0..nsub {
        if sel[i] >= 0 {
            let sfi = bits.peek16(sfp, 10) as usize;
            sfp += 6;
            let (a, b) = (SUBBAND[i] as usize, SUBBAND[i + 1] as usize);
            mantissas(bits, mode, sel[i] as usize, f(SF[sfi]), spec, a, b)?;
        }
    }
    Ok(SUBBAND[nsub])
}

/// FUN_102c2f40 (SSE path): 4-band IMDCT in lanes, window/overlap, then gain compensation.
fn imdct(ch: &mut Channel, nbands: usize, gc: [f32; 4], spec: &[f32]) {
    let mut a = [[0f32; 4]; 256];
    for i in 0..256 {
        a[i][0] = spec[i];
        a[255 - i][1] = spec[256 + i];
        a[i][2] = spec[512 + i];
        a[255 - i][3] = spec[768 + i];
    }
    let mut c = 128;
    while c >= 2 {
        let mut u = c / 2;
        let mut s = 0;
        while u < 128 {
            for m in 0..c {
                for l in 0..4 {
                    a[s + m][l] -= a[s + 2 * c - 1 - m][l];
                }
            }
            let t = f(TW[127 - u]);
            for m in 0..c {
                for l in 0..4 {
                    let x = a[s + m][l];
                    let y = a[s + c + m][l] * t;
                    a[s + c + m][l] = x - y;
                    a[s + m][l] = x + y;
                }
            }
            s += 2 * c;
            u += c;
        }
        c /= 2;
    }
    let out = &mut ch.buf[NEW..];
    for n in 0..128 {
        let idx = PERM[n] as usize;
        let r = &POST[idx * 4..idx * 4 + 4];
        let (f0, f4, f8, fc) = (f(r[0]), f(r[1]), f(r[2]), f(r[3]));
        for l in 0..4 {
            let (p, q) = (a[2 * n][l], a[2 * n + 1][l]);
            let d = p - q;
            let qq = q * fc;
            let y = (d - qq) * gc[l] * f8;
            let cc = ch.overlap[idx][l];
            out[(127 - idx) * 4 + l] = cc - f0 * y;
            out[(128 + idx) * 4 + l] = cc * f0 + y;
            ch.overlap[idx][l] = (d + qq) * f4;
        }
    }
    for l in 0..nbands {
        gain(out, l, &ch.gains[l]);
    }
}

/// Gain compensation of one lane (second half of FUN_102c2f40).
fn gain(out: &mut [f32], l: usize, pts: &[(u32, u32); 8]) {
    let at = |s: usize| s * 4 + l;
    let mut s = 0usize;
    let mut k = 0;
    loop {
        let (loc, lev) = pts[k];
        let stop = loc as usize + 1;
        if lev == 4 {
            if stop >= 256 {
                return;
            }
            s = stop;
        } else {
            let g = f(GAIN_LEV[lev as usize]);
            out[at(s)] *= g;
            s += 1;
            while s < stop {
                for j in 0..8 {
                    out[at(s + j)] *= g;
                }
                s += 8;
            }
        }
        let next = pts[(k + 1).min(7)].1 as i32;
        let diff = next - lev as i32;
        let mut u = (diff + lev as i32 * 8) as u32;
        for j in 0..7 {
            out[at(s + j)] *= f(GAIN_LEV[(u >> 3) as usize]) * f(GAIN_FRAC[(u & 7) as usize]);
            u = u.wrapping_add(diff as u32);
        }
        s += 7;
        k += 1;
        if s >= 256 || k >= 8 {
            return;
        }
    }
}

/// FUN_102c4b10: clears an unused band's lane and overlap.
fn zero_band(ch: &mut Channel, b: usize) {
    for o in ch.overlap.iter_mut() {
        o[b] = 0.0;
    }
    for t in 0..256 {
        ch.buf[NEW + t * 4 + b] = 0.0;
    }
}

/// FUN_102c4970: joint-stereo reconstruction of the new band samples of both channels.
fn joint(c0: &mut [f32], c1: &mut [f32], cur: Joint, prev: Joint) {
    let wt = |w: u8| (f(WEIGHTS[w as usize * 2]), f(WEIGHTS[w as usize * 2 + 1]));
    for l in 0..4 {
        let (w0p, w1p) = if prev.nb < l { wt(prev.w) } else { (2.0, 2.0) };
        let (w0, w1) = if cur.nb < l { wt(cur.w) } else { (2.0, 2.0) };
        let mc = (f(MATRIX[cur.m[l] as usize]), f(MATRIX[cur.m[l] as usize + 1]));
        let mp = (f(MATRIX[prev.m[l] as usize]), f(MATRIX[prev.m[l] as usize + 1]));
        let mut start = 0;
        if w0p != w0 || cur.m[l] != prev.m[l] {
            let d0 = (mc.0 - mp.0) * 0.125;
            let d1 = (mc.1 - mp.1) * 0.125;
            let dw0 = (w0 - w0p) * 0.125;
            let dw1 = (w1 - w1p) * 0.125;
            for t in -8i32..0 {
                let i = NEW + (t + 8) as usize * 4 + l;
                let ft = t as f32;
                let (x0, x1) = (c0[i], c1[i]);
                let mix = (ft * d0 + mc.0) * x0 + (ft * d1 + mc.1) * x1;
                c1[i] = (x0 - mix) * (ft * dw1 + w1);
                c0[i] = (ft * dw0 + w0) * mix;
            }
            start = 8;
        }
        for t in start..256 {
            let i = NEW + t * 4 + l;
            let (x0, x1) = (c0[i], c1[i]);
            let mix = x0 * mc.0 + x1 * mc.1;
            c0[i] = mix * w0;
            c1[i] = (x0 - mix) * w1;
        }
    }
}

/// FUN_102c3960 (SSE path): 4-band QMF synthesis in place; 1024 samples land in buf[0..0x400].
fn qmf(buf: &mut [f32]) {
    let q = |i: usize| f(QMF[i]);
    let mix = |i: usize| f(QMF_MIX[i]);
    for it in 0..256 {
        let p = it * 4;
        let mut a = [0f32; 4];
        let mut b = [0f32; 4];
        for k in 0..24 {
            for l in 0..4 {
                let x = buf[p + 0x2e + 4 * k + l];
                a[l] += q(192 + 4 * k + l) * x;
                b[l] += q(96 + 4 * k + l) * x;
            }
        }
        for l in 0..4 {
            let s = if l < 2 { a } else { b };
            buf[p + 0x2e + l] = (s[0] + s[1] * mix(l)) + (s[2] * mix(4 + l) + s[3] * mix(8 + l));
        }
        for l in 0..4 {
            let mut s = 0f32;
            for k in 0..24 {
                s += q(4 * k + l) * buf[p + 2 * k + l];
            }
            buf[p + l] = s;
        }
    }
}

impl Atrac3 {
    /// channels 1 or 2; mode = the header byte (bgw 0x2f, spw 0x2c): 1 -> 0x130-byte frames, 2 -> 0xc0 (joint
    /// stereo), else 0x180. Mono spw frames are half size.
    pub fn new(channels: u8, mode: u8) -> Self {
        // FUN_1023d230 picks (bytes, kbps, mode); FUN_102bf950 table 0x10338ad8.. gives joint and unit size.
        let (frame2, joint) = match mode {
            1 => (0x130, false),
            2 => (0xc0, true),
            _ => (0x180, false),
        };
        let out_channels = if channels == 2 { 2 } else { 1 };
        let frame = if out_channels == 2 { frame2 } else { frame2 / 2 };
        let unit = if joint { frame2 } else { frame2 / 2 };
        Self {
            out_channels,
            dec_channels: if joint || out_channels == 2 { 2 } else { 1 },
            joint,
            unit,
            frame,
            ch: [Channel::new(), Channel::new()],
            js: JOINT_INIT,
            js_prev: JOINT_INIT,
            frames: 0,
            errors: 0,
            prime: true,
            data: vec![0; frame2 + PAD],
            spec: vec![0.0; 1024 + PAD],
        }
    }

    pub fn frame_bytes(&self) -> usize {
        self.frame
    }

    /// Frames that failed to decode (output as silence).
    pub fn errors(&self) -> u64 {
        self.errors
    }

    pub fn reset(&mut self) {
        self.ch = [Channel::new(), Channel::new()];
        self.js = JOINT_INIT;
        self.js_prev = JOINT_INIT;
        self.frames = 0;
    }

    /// One already-descrambled frame -> 1024 samples per channel, interleaved i16, appended to out.
    /// The first frame after new/reset is decoded twice: FUN_102bfb30 yields nothing on its first call and
    /// the callers retry with the same input.
    pub fn decode_frame(&mut self, frame: &[u8], out: &mut Vec<i16>) {
        if self.frames == 0 && self.prime {
            let _ = self.decode_raw(frame);
        }
        self.frames += 1;
        let ok = self.decode_raw(frame).is_ok();
        if !ok {
            self.errors += 1;
        }
        let base = out.len();
        out.resize(base + 1024 * self.out_channels, 0);
        if !ok {
            return;
        }
        for c in 0..self.out_channels {
            for (t, &x) in self.ch[c].buf[..1024].iter().enumerate() {
                // Clamp, then the 1.5 * 2^23 rounding trick of FUN_102bfb30.
                let v = (x.clamp(-32767.0, 32767.0) + 12582912.0).to_bits() as u16 as i16;
                out[base + t * self.out_channels + c] = v;
            }
        }
    }

    /// FUN_102c44e0: decodes all units, then QMF per channel.
    fn decode_raw(&mut self, frame: &[u8]) -> Result<(), Err> {
        let n = frame.len().min(self.frame);
        self.data.fill(0);
        self.data[..n].copy_from_slice(&frame[..n]);
        if self.out_channels == 1 {
            // Mono effects are decoded from the frame repeated into both halves (FUN_1023cb20).
            let (a, b) = self.data.split_at_mut(n);
            b[..n].copy_from_slice(&a[..n]);
        }
        let mut ok = true;
        let mut limit = self.unit;
        let mut off = 0;
        for c in 0..self.dec_channels {
            let second = self.joint && c == 1;
            if second {
                let size = self.unit;
                let mut last = size;
                loop {
                    last -= 1;
                    if self.data[last] != 0xf8 || last == 0 {
                        break;
                    }
                }
                self.data[..=last].reverse();
                limit = (last as i64 + 1 - off as i64) as usize;
            }
            let r = self.unit_decode(c, off, second);
            match r {
                Ok(pos) => {
                    if limit * 8 + 0x10 < pos {
                        ok = false;
                    }
                    if !self.joint {
                        off += self.unit;
                        limit += self.unit;
                    } else {
                        off = pos.div_ceil(8);
                    }
                }
                Err(()) => {
                    ok = false;
                    if !self.joint {
                        off += self.unit;
                        limit += self.unit;
                    }
                }
            }
        }
        for c in 0..self.dec_channels {
            qmf(&mut self.ch[c].buf);
        }
        if ok { Ok(()) } else { Err(()) }
    }

    /// FUN_102c4640: one sound unit; returns the bit position reached.
    fn unit_decode(&mut self, c: usize, off: usize, second: bool) -> Result<usize, Err> {
        let mut bits = Bits { b: &self.data, pos: 0 };
        let hdr;
        if second {
            hdr = self.data[1];
            bits.pos = 0x10;
            if hdr & 0xc != 0xc {
                return Err(());
            }
        } else {
            hdr = self.data[off];
            bits.pos = off * 8 + 8;
            if hdr & 0xfc != 0xa0 {
                return Err(());
            }
        }
        let nb = (hdr & 3) as usize;
        let mut gains = NO_GAIN;
        let mut min_end = 0i32;
        for band in 0..=nb {
            let n = bits.read(3) as usize;
            for i in 0..n {
                let lev = bits.peek16(bits.pos, 0xc);
                let loc = bits.peek16(bits.pos + 4, 8) & 0xf8;
                bits.pos += 9;
                if i > 0 && loc <= gains[band][i - 1].0 {
                    return Err(());
                }
                gains[band][i] = (loc, lev);
            }
            gains[band][n] = (0xff, 4);
            if n != 0 {
                min_end = 0x100 * (band as i32 + 1);
            }
        }
        let spec = &mut self.spec;
        spec.fill(0.0);
        let ntonal = bits.read(5);
        if ntonal != 0 {
            min_end = tonal(&mut bits, ntonal, nb, min_end, spec)?;
        }
        let nsub = bits.read(5) as usize + 1;
        let end = spectrum(&mut bits, nsub, spec)?.max(min_end);
        let pos = bits.pos;
        let ch = &mut self.ch[c];
        let bands = ((end + 0xff) >> 8) as usize;
        let nproc = bands.max(ch.prev_bands);
        ch.prev_bands = bands;
        ch.buf.copy_within(0x400..BUF, 0);
        let mut gc = [1f32; 4];
        for (b, g) in gc.iter_mut().enumerate().take(nproc) {
            *g = f(GAIN_LEV[gains[b][0].1 as usize]);
        }
        imdct(ch, nproc, gc, &self.spec);
        for b in nproc..4 {
            zero_band(&mut self.ch[c], b);
        }
        if second {
            let [c0, c1] = &mut self.ch;
            joint(&mut c0.buf, &mut c1.buf, self.js, self.js_prev);
            self.js_prev = self.js;
            let (b0, b1) = (self.data[0] as u32, self.data[1] as u32);
            let mut js = Joint { m: [0; 4], w: (b0 >> 4) as u8, nb };
            let mut v = (b1 >> 4) | (b0 << 8 >> 4);
            for i in (0..4).rev() {
                let code = (v & 3) as u8;
                if code == 2 {
                    return Err(());
                }
                js.m[i] = code;
                v >>= 2;
            }
            self.js = js;
        }
        self.ch[c].gains = gains;
        Ok(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_stream_vector() {
        let k = key_stream(0x46a65af4);
        assert_eq!(&k[..8], &[53, 162, 215, 50, 222, 179, 148, 201]);
        assert_eq!(&k[396..], &[117, 215, 13, 111]);
    }

    /// A unit with no gain, no tonal, one uncoded subband: header 0xa0, then 3 + 5 + 5 + 1 + 3 zero bits.
    fn silent_frame(bytes: usize) -> Vec<u8> {
        let mut f = vec![0u8; bytes];
        for u in (0..bytes).step_by(bytes / 2) {
            f[u] = 0xa0;
        }
        f
    }

    #[test]
    fn silent_units_decode_to_zero() {
        for mode in [0u8, 1] {
            let mut d = Atrac3::new(2, mode);
            let mut out = Vec::new();
            let fr = silent_frame(d.frame_bytes());
            for _ in 0..4 {
                d.decode_frame(&fr, &mut out);
            }
            assert_eq!(out.len(), 4 * 2048);
            assert!(out.iter().all(|&s| s == 0));
            assert_eq!(d.errors(), 0);
        }
        let mut d = Atrac3::new(1, 0);
        assert_eq!(d.frame_bytes(), 0xc0);
        let mut out = Vec::new();
        let mut fr = vec![0u8; 0xc0];
        fr[0] = 0xa0;
        d.decode_frame(&fr, &mut out);
        assert_eq!(out.len(), 1024);
        assert_eq!(d.errors(), 0);
    }

    /// Joint stereo (mode 2): unit 2 is stored byte-reversed at the frame end, behind its side info.
    #[test]
    fn silent_joint_frame() {
        let mut d = Atrac3::new(2, 2);
        assert_eq!(d.frame_bytes(), 0xc0);
        let mut fr = vec![0u8; 0xc0];
        fr[0] = 0xa0;
        fr[0xbf] = 0x7f; // weight index 7, matrix codes 3 3
        fr[0xbe] = 0xfc; // matrix codes 3 3, sync 11, no gain bands
        let mut out = Vec::new();
        for _ in 0..3 {
            d.decode_frame(&fr, &mut out);
        }
        assert_eq!(d.errors(), 0);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn bad_sync_is_an_error() {
        let mut d = Atrac3::new(2, 0);
        let mut out = Vec::new();
        d.decode_frame(&[0u8; 0x180], &mut out);
        assert_eq!(d.errors(), 1);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn tables_match_known_values() {
        assert_eq!(f(SF[15]), 1.0);
        assert_eq!(f(GAIN_LEV[4]), 1.0);
        assert_eq!(SUBBAND[32], 1024);
        // Quantised values are mantissa * 4882.5 / max quant (1.5, 2.5, ..., 31.5).
        assert_eq!(f(PAIRS[4]), 3255.0);
        assert_eq!(f(VAL1[1]), 1953.0);
        assert_eq!(f(VAL6[1]), 155.0);
    }

    const SCRATCH: &str = "/tmp/claude-1000/-home-lenon-Projects/54d8551b-f3bb-497f-b001-407002933c40/scratchpad/at3/";
    const GAME: &str = "/home/lenon/ffxi/game/FINAL FANTASY XI/sound/win/";

    /// Decodes a game file without priming (as ffmpeg) and returns (max, mean) abs error vs ffmpeg s16le.
    /// The client output is the negation of ffmpeg (its QMF/IMDCT tables carry the opposite sign).
    fn compare(file: &str, raw: &str, frames: usize) -> (i32, f64, u64) {
        let d = std::fs::read(format!("{GAME}{file}")).unwrap();
        let u32at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let se = &d[..6] == b"SeWave";
        let (key, ch, mode, off) =
            if se { (u32at(0x20), d[0x2a], d[0x2c], u32at(0x24)) } else { (u32at(0x24), d[0x2e], d[0x2f], u32at(0x28)) };
        let ks = key_stream(key);
        let mut dec = Atrac3::new(ch, mode);
        dec.prime = false;
        let fb = dec.frame_bytes();
        let mut ours = Vec::new();
        let n = ((d.len() - off as usize) / fb).min(frames);
        for i in 0..n {
            let at = off as usize + i * fb;
            let fr: Vec<u8> = d[at..at + fb].iter().zip(ks.iter()).map(|(a, b)| a ^ b).collect();
            dec.decode_frame(&fr, &mut ours);
        }
        let dump: Vec<u8> = ours.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(format!("{SCRATCH}{raw}.ours"), dump).unwrap();
        let r = std::fs::read(format!("{SCRATCH}{raw}")).unwrap();
        let theirs: Vec<i16> = r.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        let m = ours.len().min(theirs.len());
        let mut max = 0;
        let mut sum = 0f64;
        for i in 0..m {
            let e = (ours[i] as i32 + theirs[i] as i32).abs();
            max = max.max(e);
            sum += e as f64;
        }
        (max, sum / m as f64, dec.errors())
    }

    #[test]
    #[ignore]
    fn matches_ffmpeg() {
        for (file, raw, frames) in [
            ("music/data/music040.bgw", "m040.ffmpeg.raw", 200),
            ("se/se000/se000040.spw", "s040.ffmpeg.raw", 1000),
            ("se/se032/se032140.spw", "s32140.ffmpeg.raw", 1000),
        ] {
            let (max, mean, errs) = compare(file, raw, frames);
            eprintln!("{file}: max {max} mean {mean:.4} errors {errs}");
            assert_eq!(errs, 0);
            assert!(max <= 2, "{file}: max error {max}");
        }
    }

    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "bgw" || x == "spw") {
                out.push(p);
            }
        }
    }

    /// Every format-3 file in the install decodes; only the final three frames of music069, 071 and 900 fail
    /// (they are not ATRAC3 after descrambling).
    #[test]
    #[ignore]
    fn all_game_files_decode() {
        let mut files = Vec::new();
        walk(std::path::Path::new("/home/lenon/ffxi/game/FINAL FANTASY XI"), &mut files);
        let (mut n, mut frames, mut bad, mut bad_mid) = (0, 0u64, 0u64, 0u64);
        let t = std::time::Instant::now();
        for p in files {
            let d = std::fs::read(&p).unwrap();
            if d.len() < 0x30 || u32::from_le_bytes(d[0xc..0x10].try_into().unwrap()) != 3 {
                continue;
            }
            let u32at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
            let se = &d[..6] == b"SeWave";
            let (key, ch, mode, off) =
                if se { (u32at(0x20), d[0x2a], d[0x2c], u32at(0x24)) } else { (u32at(0x24), d[0x2e], d[0x2f], u32at(0x28)) };
            let ks = key_stream(key);
            let mut dec = Atrac3::new(ch, mode);
            let fb = dec.frame_bytes();
            let mut out = Vec::new();
            for (fi, fr) in d[off as usize..].chunks_exact(fb).enumerate() {
                let x: Vec<u8> = fr.iter().zip(ks.iter()).map(|(a, b)| a ^ b).collect();
                out.clear();
                let e = dec.errors();
                dec.decode_frame(&x, &mut out);
                if dec.errors() != e {
                    let total = (d.len() - off as usize) / fb;
                    eprintln!("bad frame {} of {} ({:02x?})", fi, total, &x[..8]);
                    if fi + 3 < total {
                        bad_mid += 1;
                    }
                }
                frames += 1;
            }
            if dec.errors() != 0 {
                eprintln!("{}: {} bad frames", p.display(), dec.errors());
            }
            bad += dec.errors();
            n += 1;
        }
        let s = t.elapsed().as_secs_f64();
        eprintln!("{n} files, {frames} frames, {bad} bad, {:.1} us/frame", s * 1e6 / frames as f64);
        assert_eq!(bad_mid, 0);
    }
}

// Tables copied from FFXiMain_unpacked.dll (generated by gen.py from the listed addresses).

/// IMDCT twiddles: tw(u) = TW[127 - u], u = 1..=127 (0x103bf7f0).
const TW: [u32; 127] = [
    0x3f80e7e4, 0x3f7c5d27, 0x3fdd2d53, 0x3f839c3d, 0x3f027dc0, 0x3fdebe05, 0x3ff78bc5, 0x3f715aea,
    0x3f9b02c6, 0x3f088e93, 0x3fcbbbf8, 0x3fdb941a, 0x3e89cf86, 0x3ff6ba07, 0x3ffdabcc, 0x3f8e39da,
    0x3f60924f, 0x3f9d7fd1, 0x3fe60f88, 0x3ef8cfcc, 0x3f268f12, 0x3fc9d112, 0x3ff21352, 0x3fe1c598,
    0x3fa99415, 0x3e7ab273, 0x3fbfc767, 0x3ff853f8, 0x3dfb2b74, 0x3ffe1324, 0x3fff84ab, 0x3f43ef15,
    0x3f864b82, 0x3f663375, 0x3fd9f26a, 0x3f987fc0, 0x3eec9a7f, 0x3fe4aa59, 0x3ff91298, 0x3f14a031,
    0x3f95f6d9, 0x3f209ae5, 0x3fcf7a1f, 0x3fcd9f02, 0x3ea2abb6, 0x3ff31447, 0x3ffcbfc9, 0x3fd4db31,
    0x3f553641, 0x3fa73656, 0x3fe8bf3c, 0x3e964083, 0x3f3263ef, 0x3fc1d870, 0x3feff573, 0x3ff4fa0b,
    0x3fae3bde, 0x3e16a905, 0x3fbb8f3b, 0x3ffd3aac, 0x3d96c32c, 0x3fff4e6d, 0x3fffd397, 0x3fb504f3,
    0x3f76e0cb, 0x3f88f59b, 0x3fe04621, 0x3f5ae880, 0x3f0e9a22, 0x3fd84853, 0x3ff5dec6, 0x3fa26799,
    0x3f9ff6cb, 0x3ee05c13, 0x3fc7de65, 0x3fe76bd8, 0x3e61bc2e, 0x3ff9c79d, 0x3ffe70b0, 0x3ec7c5c2,
    0x3f6bcbbb, 0x3f93682a, 0x3fe33c5a, 0x3f2c7cd4, 0x3f1aa086, 0x3fd14d3d, 0x3ff40bdd, 0x3fc5e403,
    0x3fa4d225, 0x3eaf10a2, 0x3fc3e200, 0x3ff10908, 0x3e2fb680, 0x3ffc3b28, 0x3fff0e58, 0x3fec835e,
    0x3f8b9a6b, 0x3f4f7bca, 0x3fd695e5, 0x3fabeb4a, 0x3ed41501, 0x3fea09a7, 0x3ffa7302, 0x3e48bd36,
    0x3f90d3cd, 0x3f38442a, 0x3fd31849, 0x3fbdaef9, 0x3ebb6ecf, 0x3feed89e, 0x3ffbaccd, 0x3ffb14be,
    0x3f49b953, 0x3fb085bb, 0x3feb4b0c, 0x3dc8fb30, 0x3f3e1d4a, 0x3fb96842, 0x3fedb293, 0x3ffec46d,
    0x3fb2c8c9, 0x3d490ab0, 0x3fb73a23, 0x3fffb10f, 0x3cc90e90, 0x3fffec43, 0x3ffffb11,
];
/// IMDCT post-twiddle/window records {f0, f4, f8, fc} per index (0x103beff0).
const POST: [u32; 512] = [
    0x3f7ce0a9, 0xaa465548, 0xa61bc55e, 0x3ffffec4, 0x3f76bf00, 0xaa48af1b, 0xa6ec6e94, 0x3ffff4e6,
    0x3f70c30e, 0xaa4af6cc, 0xa747466e, 0x3fffe129, 0x3f6aebb4, 0xaa4d2bb8, 0xa78d0514, 0x3fffc38f,
    0x3f6537e0, 0xaa4f4d46, 0xa7b7367f, 0x3fff9c18, 0x3f5fa687, 0xaa515aee, 0xa7e22c5a, 0x3fff6ac7,
    0x3f5a36a9, 0xaa535430, 0xa806ed98, 0x3fff2f9d, 0x3f54e750, 0xaa55389b, 0xa81d1ba1, 0x3ffeea9d,
    0x3f4fb78f, 0xaa5707cd, 0xa8339a4a, 0x3ffe9bc9, 0x3f4aa681, 0xaa58c16f, 0xa84a637d, 0x3ffe4323,
    0x3f45b34b, 0xaa5a6538, 0xa8617110, 0x3ffde0b1, 0x3f40dd17, 0xaa5bf2f0, 0xa878bccf, 0x3ffd7474,
    0x3f3c2319, 0xaa5d6a69, 0xa8882040, 0x3ffcfe73, 0x3f37848c, 0xaa5ecb84, 0xa893faf7, 0x3ffc7eb0,
    0x3f3300b0, 0xaa60162f, 0xa89feb71, 0x3ffbf531, 0x3f2e96cf, 0xaa614a67, 0xa8abee9e, 0x3ffb61fc,
    0x3f2a4636, 0xaa626834, 0xa8b80173, 0x3ffac516, 0x3f260e3c, 0xaa636fab, 0xa8c420f4, 0x3ffa1e84,
    0x3f21ee39, 0xaa6460ec, 0xa8d04a32, 0x3ff96e4e, 0x3f1de58f, 0xaa653c25, 0xa8dc7a4d, 0x3ff8b47b,
    0x3f19f3a3, 0xaa66018c, 0xa8e8ae7c, 0x3ff7f110, 0x3f1617e0, 0xaa66b164, 0xa8f4e407, 0x3ff72417,
    0x3f1251b7, 0xaa674bf8, 0xa9008c27, 0x3ff64d97, 0x3f0ea09b, 0xaa67d19d, 0xa906a464, 0x3ff56d97,
    0x3f0b0407, 0xaa6842b1, 0xa90cb985, 0x3ff48422, 0x3f077b79, 0xaa689f98, 0xa912ca5f, 0x3ff3913f,
    0x3f040672, 0xaa68e8c1, 0xa918d5d6, 0x3ff294f8, 0x3f00a478, 0xaa691e9c, 0xa91edade, 0x3ff18f57,
    0x3efaaa2d, 0xaa6941a4, 0xa924d877, 0x3ff08066, 0x3ef42fb6, 0xaa695257, 0xa92acdb0, 0x3fef6830,
    0x3eedd8ae, 0xaa695137, 0xa930b9aa, 0x3fee46be, 0x3ee7a440, 0xaa693eca, 0xa9369b91, 0x3fed1c1d,
    0x3ee1919d, 0xaa691b9b, 0xa93c72a3, 0x3febe858, 0x3edb9ffd, 0xaa68e836, 0xa9423e2c, 0x3feaab7b,
    0x3ed5ce9f, 0xaa68a52a, 0xa947fd86, 0x3fe96591, 0x3ed01cc5, 0xaa685306, 0xa94db01b, 0x3fe816a8,
    0x3eca89bb, 0xaa67f25c, 0xa9535562, 0x3fe6becc, 0x3ec514d0, 0xaa6783bd, 0xa958ece1, 0x3fe55e0b,
    0x3ebfbd59, 0xaa6707bc, 0xa95e762b, 0x3fe3f473, 0x3eba82b1, 0xaa667ee9, 0xa963f0e0, 0x3fe28210,
    0x3eb56438, 0xaa65e9d4, 0xa9695cae, 0x3fe106f2, 0x3eb06151, 0xaa65490e, 0xa96eb94f, 0x3fdf8327,
    0x3eab7967, 0xaa649d23, 0xa9740688, 0x3fddf6be, 0x3ea6abe7, 0xaa63e6a0, 0xa979442a, 0x3fdc61c7,
    0x3ea1f843, 0xaa63260e, 0xa97e7211, 0x3fdac450, 0x3e9d5df1, 0xaa625bf5, 0xa981c811, 0x3fd91e6a,
    0x3e98dc6d, 0xaa6188da, 0xa9844f28, 0x3fd77026, 0x3e947335, 0xaa60ad3e, 0xa986ce4a, 0x3fd5b993,
    0x3e9021cb, 0xaa5fc9a0, 0xa9894577, 0x3fd3fac3, 0x3e8be7b6, 0xaa5ede7c, 0xa98bb4b6, 0x3fd233c6,
    0x3e87c47f, 0xaa5dec4b, 0xa98e1c0f, 0x3fd064af, 0x3e83b7b4, 0xaa5cf382, 0xa9907b8f, 0x3fce8d90,
    0x3e7f81cb, 0xaa5bf492, 0xa992d347, 0x3fccae79, 0x3e77bf50, 0xaa5aefe9, 0xa995234b, 0x3fcac77f,
    0x3e702724, 0xaa59e5f2, 0xa9976bb0, 0x3fc8d8b3, 0x3e68b87b, 0xaa58d714, 0xa999ac90, 0x3fc6e22a,
    0x3e617292, 0xaa57c3b1, 0xa99be607, 0x3fc4e3f5, 0x3e5a54a7, 0xaa56ac2b, 0xa99e1833, 0x3fc2de29,
    0x3e535e00, 0xaa5590dc, 0xa9a04333, 0x3fc0d0da, 0x3e4c8dea, 0xaa54721d, 0xa9a2672a, 0x3fbebc1b,
    0x3e45e3b3, 0xaa535042, 0xa9a4843c, 0x3fbca003, 0x3e3f5eb3, 0xaa522b9e, 0xa9a69a8d, 0x3fba7ca4,
    0x3e38fe43, 0xaa51047e, 0xa9a8aa45, 0x3fb85216, 0x3e32c1c4, 0xaa4fdb2b, 0xa9aab38b, 0x3fb6206c,
    0x3e2ca898, 0xaa4eafee, 0xa9acb688, 0x3fb3e7bc, 0x3e26b229, 0xaa4d830a, 0xa9aeb366, 0x3fb1a81d,
    0x3e20dde4, 0xaa4c54bf, 0xa9b0aa50, 0x3faf61a5, 0x3e1b2b3a, 0xaa4b254b, 0xa9b29b71, 0x3fad1469,
    0x3e1599a0, 0xaa49f4e8, 0xa9b486f4, 0x3faac082, 0x3e102891, 0xaa48c3ce, 0xa9b66d08, 0x3fa86605,
    0x3e0ad789, 0xaa479230, 0xa9b84dd7, 0x3fa6050a, 0x3e05a60b, 0xaa466041, 0xa9ba2990, 0x3fa39da9,
    0x3e00939a, 0xaa452e2f, 0xa9bc005f, 0x3fa12ff9, 0x3df73f7e, 0xaa43fc26, 0xa9bdd272, 0x3f9ebc12,
    0x3ded940f, 0xaa42ca51, 0xa9bf9ff6, 0x3f9c420c, 0x3de42404, 0xaa4198d7, 0xa9c16919, 0x3f99c200,
    0x3ddaee84, 0xaa4067dd, 0xa9c32e07, 0x3f973c07, 0x3dd1f2bb, 0xaa3f3785, 0xa9c4eeef, 0x3f94b039,
    0x3dc92fdc, 0xaa3e07ef, 0xa9c6abfc, 0x3f921eb0, 0x3dc0a51f, 0xaa3cd93b, 0xa9c8655b, 0x3f8f8784,
    0x3db851c4, 0xaa3bab84, 0xa9ca1b3a, 0x3f8cead0, 0x3db03511, 0xaa3a7ee5, 0xa9cbcdc3, 0x3f8a48ad,
    0x3da84e4f, 0xaa395376, 0xa9cd7d23, 0x3f87a136, 0x3da09cce, 0xaa38294e, 0xa9cf2985, 0x3f84f484,
    0x3d991fe6, 0xaa370083, 0xa9d0d314, 0x3f8242b1, 0x3d91d6f0, 0xaa35d927, 0xa9d279fa, 0x3f7f17b2,
    0x3d8ac14e, 0xaa34b34c, 0xa9d41e62, 0x3f79a02d, 0x3d83de65, 0xaa338f03, 0xa9d5c075, 0x3f741f07,
    0x3d7a5b3f, 0xaa326c5a, 0xa9d7605c, 0x3f6e9479, 0x3d6d5cda, 0xaa314b5f, 0xa9d8fe40, 0x3f6900b7,
    0x3d60c084, 0xaa302c1f, 0xa9da9a49, 0x3f6363fa, 0x3d54852f, 0xaa2f0ea4, 0xa9dc349e, 0x3f5dbe79,
    0x3d48a9d5, 0xaa2df2f8, 0xa9ddcd67, 0x3f58106b, 0x3d3d2d7c, 0xaa2cd923, 0xa9df64ca, 0x3f525a09,
    0x3d320f30, 0xaa2bc12e, 0xa9e0faed, 0x3f4c9b8b, 0x3d274e06, 0xaa2aab1e, 0xa9e28ff6, 0x3f46d529,
    0x3d1ce91f, 0xaa2996fa, 0xa9e42409, 0x3f41071e, 0x3d12df9f, 0xaa2884c5, 0xa9e5b74c, 0x3f3b31a0,
    0x3d0930b5, 0xaa277484, 0xa9e749e1, 0x3f3554ec, 0x3cffb734, 0xaa266639, 0xa9e8dbed, 0x3f2f713a,
    0x3cedbf16, 0xaa2559e7, 0xa9ea6d92, 0x3f2986c4, 0x3cdc779e, 0xaa244f8f, 0xa9ebfef2, 0x3f2395c5,
    0x3ccbdf69, 0xaa234731, 0xa9ed9030, 0x3f1d9e78, 0x3cbbf522, 0xaa2240ce, 0xa9ef216d, 0x3f17a117,
    0x3cacb785, 0xaa213c64, 0xa9f0b2ca, 0x3f119ddd, 0x3c9e255a, 0xaa2039f2, 0xa9f24466, 0x3f0b9507,
    0x3c903d79, 0xaa1f3976, 0xa9f3d662, 0x3f0586ce, 0x3c82fec7, 0xaa1e3aed, 0xa9f568dd, 0x3efee6e1,
    0x3c6cd074, 0xaa1d3e55, 0xa9f6fbf5, 0x3ef2b651, 0x3c54f1a4, 0xaa1c43a9, 0xa9f88fca, 0x3ee67c66,
    0x3c3e5f3f, 0xaa1b4ae6, 0xa9fa247a, 0x3eda3997, 0x3c29177e, 0xaa1a5407, 0xa9fbba20, 0x3ecdee60,
    0x3c1518b7, 0xaa195f06, 0xa9fd50db, 0x3ec19b37, 0x3c026159, 0xaa186bdf, 0xa9fee8c7, 0x3eb54098,
    0x3be1dfdd, 0xaa177a8b, 0xaa004100, 0x3ea8defc, 0x3bc18634, 0xaa168b04, 0xaa010e50, 0x3e9c76de,
    0x3ba3b334, 0xaa159d44, 0xaa01dc63, 0x3e9008b7, 0x3b88648c, 0xaa14b143, 0xaa02ab44, 0x3e839502,
    0x3b5f3038, 0xaa13c6fa, 0xaa037b02, 0x3e6e3876, 0x3b3297f0, 0xaa12de61, 0xaa044ba9, 0x3e553db9,
    0x3b0afccd, 0xaa11f770, 0xaa051d45, 0x3e3c3ac3, 0x3ad0b779, 0xaa11121f, 0xaa05efe4, 0x3e23308c,
    0x3a956424, 0xaa102e64, 0xaa06c390, 0x3e0a200a, 0x3a47f60c, 0xaa0f4c38, 0xaa079858, 0x3de21469,
    0x39f1e132, 0xaa0e6b91, 0xaa086e45, 0x3dafe007, 0x3976c761, 0xaa0d8c65, 0xaa094563, 0x3d7b49ba,
    0x38b1a9b5, 0xaa0caeab, 0xaa0a1dbf, 0x3d16c9b6, 0x371dea27, 0xaa0bd258, 0xaa0af762, 0x3c490f88,
];
/// IMDCT output index order (0x103bfa28).
const PERM: [i32; 128] = [
    0, 127, 63, 64, 31, 96, 32, 95, 15, 112, 48, 79, 16, 111, 47, 80,
    7, 120, 56, 71, 24, 103, 39, 88, 8, 119, 55, 72, 23, 104, 40, 87,
    3, 124, 60, 67, 28, 99, 35, 92, 12, 115, 51, 76, 19, 108, 44, 83,
    4, 123, 59, 68, 27, 100, 36, 91, 11, 116, 52, 75, 20, 107, 43, 84,
    1, 126, 62, 65, 30, 97, 33, 94, 14, 113, 49, 78, 17, 110, 46, 81,
    6, 121, 57, 70, 25, 102, 38, 89, 9, 118, 54, 73, 22, 105, 41, 86,
    2, 125, 61, 66, 29, 98, 34, 93, 13, 114, 50, 77, 18, 109, 45, 82,
    5, 122, 58, 69, 26, 101, 37, 90, 10, 117, 53, 74, 21, 106, 42, 85,
];
/// QMF taps: [0..96) output stage, [96..192) and [192..288) band stages (0x103397e0).
const QMF: [u32; 288] = [
    0x40c98030, 0x3f800000, 0x40c98030, 0x3f800000, 0xc1a4cf95, 0x4075d95b, 0xc1a4cf95, 0x4075d95b,
    0x42696089, 0xc184914f, 0x42696089, 0xc184914f, 0xc30b226f, 0x420e6eb3, 0xc30b226f, 0x420e6eb3,
    0x43902c9a, 0xc256555c, 0x43902c9a, 0xc256555c, 0xc4061394, 0x424ee4e5, 0xc4061394, 0x424ee4e5,
    0x4465dd4f, 0x4085e56b, 0x4465dd4f, 0x4085e56b, 0xc4b9da9a, 0xc32874f2, 0xc4b9da9a, 0xc32874f2,
    0x4511be69, 0x44056a7f, 0x4511be69, 0x44056a7f, 0xc56841b1, 0xc4a0d480, 0xc56841b1, 0xc4a0d480,
    0x45d47227, 0x453a6288, 0x45d47227, 0x453a6288, 0xc6f817c8, 0xc60d2ada, 0xc6f817c8, 0xc60d2ada,
    0xc60d2ada, 0xc6f817c8, 0xc60d2ada, 0xc6f817c8, 0x453a6288, 0x45d47227, 0x453a6288, 0x45d47227,
    0xc4a0d480, 0xc56841b1, 0xc4a0d480, 0xc56841b1, 0x44056a7f, 0x4511be69, 0x44056a7f, 0x4511be69,
    0xc32874f2, 0xc4b9da9a, 0xc32874f2, 0xc4b9da9a, 0x4085e56b, 0x4465dd4f, 0x4085e56b, 0x4465dd4f,
    0x424ee4e5, 0xc4061394, 0x424ee4e5, 0xc4061394, 0xc256555c, 0x43902c9a, 0xc256555c, 0x43902c9a,
    0x420e6eb3, 0xc30b226f, 0x420e6eb3, 0xc30b226f, 0xc184914f, 0x42696089, 0xc184914f, 0x42696089,
    0x4075d95b, 0xc1a4cf95, 0x4075d95b, 0xc1a4cf95, 0x3f800000, 0x40c98030, 0x3f800000, 0x40c98030,
    0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x4075d95b, 0x4075d95b, 0x4075d95b, 0x4075d95b,
    0xc184914f, 0xc184914f, 0xc184914f, 0xc184914f, 0x420e6eb3, 0x420e6eb3, 0x420e6eb3, 0x420e6eb3,
    0xc256555c, 0xc256555c, 0xc256555c, 0xc256555c, 0x424ee4e5, 0x424ee4e5, 0x424ee4e5, 0x424ee4e5,
    0x4085e56b, 0x4085e56b, 0x4085e56b, 0x4085e56b, 0xc32874f2, 0xc32874f2, 0xc32874f2, 0xc32874f2,
    0x44056a7f, 0x44056a7f, 0x44056a7f, 0x44056a7f, 0xc4a0d480, 0xc4a0d480, 0xc4a0d480, 0xc4a0d480,
    0x453a6288, 0x453a6288, 0x453a6288, 0x453a6288, 0xc60d2ada, 0xc60d2ada, 0xc60d2ada, 0xc60d2ada,
    0xc6f817c8, 0xc6f817c8, 0xc6f817c8, 0xc6f817c8, 0x45d47227, 0x45d47227, 0x45d47227, 0x45d47227,
    0xc56841b1, 0xc56841b1, 0xc56841b1, 0xc56841b1, 0x4511be69, 0x4511be69, 0x4511be69, 0x4511be69,
    0xc4b9da9a, 0xc4b9da9a, 0xc4b9da9a, 0xc4b9da9a, 0x4465dd4f, 0x4465dd4f, 0x4465dd4f, 0x4465dd4f,
    0xc4061394, 0xc4061394, 0xc4061394, 0xc4061394, 0x43902c9a, 0x43902c9a, 0x43902c9a, 0x43902c9a,
    0xc30b226f, 0xc30b226f, 0xc30b226f, 0xc30b226f, 0x42696089, 0x42696089, 0x42696089, 0x42696089,
    0xc1a4cf95, 0xc1a4cf95, 0xc1a4cf95, 0xc1a4cf95, 0x40c98030, 0x40c98030, 0x40c98030, 0x40c98030,
    0x40c98030, 0x40c98030, 0x40c98030, 0x40c98030, 0xc1a4cf95, 0xc1a4cf95, 0xc1a4cf95, 0xc1a4cf95,
    0x42696089, 0x42696089, 0x42696089, 0x42696089, 0xc30b226f, 0xc30b226f, 0xc30b226f, 0xc30b226f,
    0x43902c9a, 0x43902c9a, 0x43902c9a, 0x43902c9a, 0xc4061394, 0xc4061394, 0xc4061394, 0xc4061394,
    0x4465dd4f, 0x4465dd4f, 0x4465dd4f, 0x4465dd4f, 0xc4b9da9a, 0xc4b9da9a, 0xc4b9da9a, 0xc4b9da9a,
    0x4511be69, 0x4511be69, 0x4511be69, 0x4511be69, 0xc56841b1, 0xc56841b1, 0xc56841b1, 0xc56841b1,
    0x45d47227, 0x45d47227, 0x45d47227, 0x45d47227, 0xc6f817c8, 0xc6f817c8, 0xc6f817c8, 0xc6f817c8,
    0xc60d2ada, 0xc60d2ada, 0xc60d2ada, 0xc60d2ada, 0x453a6288, 0x453a6288, 0x453a6288, 0x453a6288,
    0xc4a0d480, 0xc4a0d480, 0xc4a0d480, 0xc4a0d480, 0x44056a7f, 0x44056a7f, 0x44056a7f, 0x44056a7f,
    0xc32874f2, 0xc32874f2, 0xc32874f2, 0xc32874f2, 0x4085e56b, 0x4085e56b, 0x4085e56b, 0x4085e56b,
    0x424ee4e5, 0x424ee4e5, 0x424ee4e5, 0x424ee4e5, 0xc256555c, 0xc256555c, 0xc256555c, 0xc256555c,
    0x420e6eb3, 0x420e6eb3, 0x420e6eb3, 0x420e6eb3, 0xc184914f, 0xc184914f, 0xc184914f, 0xc184914f,
    0x4075d95b, 0x4075d95b, 0x4075d95b, 0x4075d95b, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000,
];
/// QMF 4-band mixing signs (0x103befc0).
const QMF_MIX: [u32; 12] = [
    0xbf800000, 0xbf800000, 0x3f800000, 0x3f800000, 0x3f800000, 0xbf800000, 0xbf800000, 0x3f800000,
    0xbf800000, 0x3f800000, 0xbf800000, 0x3f800000,
];
/// scale factors (0x103bfc28).
const SF: [u32; 64] = [
    0x3d000000, 0x3d214518, 0x3d4b2ff5, 0x3d800000, 0x3da14518, 0x3dcb2ff5, 0x3e000000, 0x3e214518,
    0x3e4b2ff5, 0x3e800000, 0x3ea14518, 0x3ecb2ff5, 0x3f000000, 0x3f214518, 0x3f4b2ff5, 0x3f800000,
    0x3fa14518, 0x3fcb2ff5, 0x40000000, 0x40214518, 0x404b2ff5, 0x40800000, 0x40a14518, 0x40cb2ff5,
    0x41000000, 0x41214518, 0x414b2ff5, 0x41800000, 0x41a14518, 0x41cb2ff5, 0x42000000, 0x42214518,
    0x424b2ff5, 0x42800000, 0x42a14518, 0x42cb2ff5, 0x43000000, 0x43214518, 0x434b2ff5, 0x43800000,
    0x43a14518, 0x43cb2ff5, 0x44000000, 0x44214518, 0x444b2ff5, 0x44800000, 0x44a14518, 0x44cb2ff5,
    0x45000000, 0x45214518, 0x454b2ff5, 0x45800000, 0x45a14518, 0x45cb2ff5, 0x46000000, 0x46214518,
    0x464b2ff5, 0x46800000, 0x46a14518, 0x46cb2ff5, 0x47000000, 0x47214518, 0x474b2ff5, 0x47800000,
];
/// gain levels (0x10339718).
const GAIN_LEV: [u32; 16] = [
    0x41800000, 0x41000000, 0x40800000, 0x40000000, 0x3f800000, 0x3f000000, 0x3e800000, 0x3e000000,
    0x3d800000, 0x3d000000, 0x3c800000, 0x3c000000, 0x3b800000, 0x3b000000, 0x3a800000, 0x3a000000,
];
/// gain interpolation steps (0x10339758).
const GAIN_FRAC: [u32; 8] = [
    0x3f800000, 0x3f6ac0c7, 0x3f5744fd, 0x3f45672a, 0x3f3504f3, 0x3f25fed7, 0x3f1837f0, 0x3f0b95c2,
];
/// subband edges (0x10339cc0).
const SUBBAND: [i32; 33] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176,
    192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 896,
    1024,
];
/// joint-stereo matrix pairs, code c -> (MATRIX[c], MATRIX[c + 1]) (0x10339680).
const MATRIX: [u32; 5] = [
    0x00000000, 0x3f800000, 0x3f800000, 0x3f000000, 0x3f000000,
];
/// joint-stereo channel weight pairs (0x10339698).
const WEIGHTS: [u32; 32] = [
    0x00000000, 0x403504f3, 0x3e924925, 0x403417e9, 0x3f124925, 0x4031495d, 0x3f5b6db7, 0x402c81d0,
    0x3f924925, 0x4025958d, 0x3fb6db6e, 0x401c3bb4, 0x3fdb6db7, 0x400ffb5b, 0x40000000, 0x40000000,
    0x403504f3, 0x00000000, 0x403417e9, 0x3e924925, 0x4031495d, 0x3f124925, 0x402c81d0, 0x3f5b6db7,
    0x4025958d, 0x3f924925, 0x401c3bb4, 0x3fb6db6e, 0x400ffb5b, 0x3fdb6db7, 0x40000000, 0x40000000,
];
/// selector 0 values, index i -> (PAIRS[i], PAIRS[i + 1]) (0x103393b0).
const PAIRS: [u32; 10] = [
    0xc54b7000, 0xc54b7000, 0x00000000, 0xc54b7000, 0x454b7000, 0x00000000, 0x00000000, 0x454b7000,
    0x454b7000, 0xc54b7000,
];
/// selector 1 values (0x103393d8).
const VAL1: [u32; 5] = [
    0x00000000, 0x44f42000, 0x45742000, 0xc5742000, 0xc4f42000,
];
/// selector 2 values (0x103393ec).
const VAL2: [u32; 7] = [
    0x4582c800, 0x44ae6000, 0x452e6000, 0x00000000, 0xc582c800, 0xc52e6000, 0xc4ae6000,
];
/// selector 3 values (0x10339408).
const VAL3: [u32; 9] = [
    0x4587a000, 0x4487a000, 0x4507a000, 0x454b7000, 0x00000000, 0xc587a000, 0xc54b7000, 0xc507a000,
    0xc487a000,
];
/// selector 4 values (0x1033942c).
const VAL4: [u32; 15] = [
    0x00000000, 0x4422c000, 0x44a2c000, 0x44f42000, 0x4522c000, 0x454b7000, 0x45742000, 0x458e6800,
    0xc58e6800, 0xc5742000, 0xc54b7000, 0xc522c000, 0xc4f42000, 0xc4a2c000, 0xc422c000,
];
/// selector 5 values (0x10339468).
const VAL5: [u32; 32] = [
    0x00000000, 0x439d8000, 0x441d8000, 0x446c4000, 0x449d8000, 0x44c4e000, 0x44ec4000, 0x4509d000,
    0x451d8000, 0x45313000, 0x4544e000, 0x45589000, 0x456c4000, 0x457ff000, 0x4589d000, 0x4593a800,
    0xc593a800, 0xc589d000, 0xc57ff000, 0xc56c4000, 0xc5589000, 0xc544e000, 0xc5313000, 0xc51d8000,
    0xc509d000, 0xc4ec4000, 0xc4c4e000, 0xc49d8000, 0xc46c4000, 0xc41d8000, 0xc39d8000, 0x00000000,
];
/// selector 6 values (0x103394e8).
const VAL6: [u32; 63] = [
    0x4441c000, 0x431b0000, 0x439b0000, 0x43e88000, 0x441b0000, 0x00000000, 0x44688000, 0x4487a000,
    0x449b0000, 0x44ae6000, 0x44c1c000, 0x44d52000, 0x44e88000, 0x44fbe000, 0x4507a000, 0x45115000,
    0x451b0000, 0x4524b000, 0x452e6000, 0x45381000, 0x4541c000, 0x454b7000, 0x45552000, 0x455ed000,
    0x45688000, 0x45723000, 0x457be000, 0x4582c800, 0x4587a000, 0x458c7800, 0x45915000, 0x45962800,
    0xc5962800, 0xc5915000, 0xc58c7800, 0xc587a000, 0xc582c800, 0xc57be000, 0xc5723000, 0xc5688000,
    0xc55ed000, 0xc5552000, 0xc54b7000, 0xc541c000, 0xc5381000, 0xc52e6000, 0xc524b000, 0xc51b0000,
    0xc5115000, 0xc507a000, 0xc4fbe000, 0xc4e88000, 0xc4d52000, 0xc4c1c000, 0xc4ae6000, 0xc49b0000,
    0xc487a000, 0xc4688000, 0xc441c000, 0xc41b0000, 0xc3e88000, 0xc39b0000, 0xc31b0000,
];
// set 0 (0x10339600): peek shift bytes [27, 13, 12, 11, 10, 9, 8]
const LUT0_0: [i8; 32] = [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 2, 2, 2, 2, 4, 4, 1, 1, 7, 8, 3, 0];
const LEN0_0: [i8; 9] = [5, 4, 3, 5, 4, 1, 3, 5, 5];
const LUT0_1: [i8; 8] = [0, 0, 0, 0, 1, 4, 2, 3];
const LEN0_1: [i8; 5] = [1, 3, 3, 3, 3];
const LUT0_2: [i8; 16] = [3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 6, 6, 2, 5, 0, 4];
const LEN0_2: [i8; 7] = [4, 3, 4, 1, 4, 4, 3];
const LUT0_3: [i8; 32] = [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 1, 1, 1, 8, 8, 8, 8, 2, 2, 7, 7, 3, 6, 0, 5];
const LEN0_3: [i8; 9] = [5, 3, 4, 5, 1, 5, 5, 4, 3];
const LUT0_4: [i8; 64] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 14, 14, 14, 14, 14, 14, 14, 14, 2, 2, 2, 2, 13, 13, 13, 13, 3, 3, 3, 3, 12, 12, 12, 12, 7, 7, 7, 7, 8, 8, 8, 8, 4, 4, 11, 11, 5, 10, 6, 9];
const LEN0_4: [i8; 15] = [2, 3, 4, 4, 5, 6, 6, 4, 4, 6, 6, 5, 4, 4, 3];
const LUT0_5: [i8; 128] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 30, 30, 30, 30, 30, 30, 30, 30, 2, 2, 2, 2, 2, 2, 2, 2, 29, 29, 29, 29, 29, 29, 29, 29, 3, 3, 3, 3, 3, 3, 3, 3, 28, 28, 28, 28, 28, 28, 28, 28, 15, 15, 15, 15, 15, 15, 15, 15, 16, 16, 16, 16, 16, 16, 16, 16, 4, 4, 4, 4, 27, 27, 27, 27, 5, 5, 5, 5, 26, 26, 26, 26, 6, 6, 6, 6, 25, 25, 25, 25, 7, 7, 24, 24, 8, 8, 23, 23, 9, 9, 22, 22, 10, 10, 21, 21, 11, 20, 12, 19, 13, 18, 14, 17];
const LEN0_5: [i8; 31] = [3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 4, 4, 7, 7, 7, 7, 6, 6, 6, 6, 5, 5, 5, 4, 4, 4];
const LUT0_6: [i8; 256] = [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 1, 1, 1, 1, 1, 1, 1, 1, 62, 62, 62, 62, 62, 62, 62, 62, 2, 2, 2, 2, 2, 2, 2, 2, 61, 61, 61, 61, 61, 61, 61, 61, 3, 3, 3, 3, 3, 3, 3, 3, 60, 60, 60, 60, 60, 60, 60, 60, 4, 4, 4, 4, 4, 4, 4, 4, 59, 59, 59, 59, 59, 59, 59, 59, 0, 0, 0, 0, 0, 0, 0, 0, 58, 58, 58, 58, 58, 58, 58, 58, 6, 6, 6, 6, 57, 57, 57, 57, 7, 7, 7, 7, 56, 56, 56, 56, 8, 8, 8, 8, 55, 55, 55, 55, 9, 9, 9, 9, 54, 54, 54, 54, 10, 10, 10, 10, 53, 53, 53, 53, 11, 11, 11, 11, 52, 52, 52, 52, 12, 12, 12, 12, 51, 51, 51, 51, 13, 13, 13, 13, 50, 50, 50, 50, 14, 14, 49, 49, 15, 15, 48, 48, 16, 16, 47, 47, 17, 17, 46, 46, 18, 18, 45, 45, 19, 19, 44, 44, 20, 20, 43, 43, 21, 42, 22, 41, 23, 40, 24, 39, 25, 38, 26, 37, 27, 36, 28, 35, 29, 34, 30, 33];
const LEN0_6: [i8; 63] = [5, 5, 5, 5, 5, 3, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5];
const BITS0: [u8; 7] = [27, 13, 12, 11, 10, 9, 8];
// set 1 (0x10339640): peek shift bytes [28, 13, 13, 12, 12, 11, 10]
const LUT1_0: [i8; 16] = [5, 6, -1, 2, 4, 7, -1, 8, -1, -1, -1, -1, 1, 3, -1, 0];
const LEN1_0: [i8; 9] = [4, 4, 4, 4, 4, 4, 4, 4, 4];
const LUT1_1: [i8; 8] = [0, 1, 2, -1, -1, -1, 3, 4];
const LEN1_1: [i8; 5] = [3, 3, 3, 3, 3];
const LUT1_2: [i8; 8] = [3, 1, 2, 0, -1, 4, 5, 6];
const LEN1_2: [i8; 7] = [3, 3, 3, 3, 3, 3, 3];
const LUT1_3: [i8; 16] = [4, 1, 2, 3, 0, -1, -1, -1, -1, -1, -1, -1, 5, 6, 7, 8];
const LEN1_3: [i8; 9] = [4, 4, 4, 4, 4, 4, 4, 4, 4];
const LUT1_4: [i8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, -1, 8, 9, 10, 11, 12, 13, 14];
const LEN1_4: [i8; 15] = [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4];
const LUT1_5: [i8; 32] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, -1, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30];
const LEN1_5: [i8; 31] = [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5];
const LUT1_6: [i8; 64] = [5, 1, 2, 3, 4, 0, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, -1, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62];
const LEN1_6: [i8; 63] = [6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6];
const BITS1: [u8; 7] = [28, 13, 13, 12, 12, 11, 10];
