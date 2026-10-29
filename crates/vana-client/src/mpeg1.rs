//! An MPEG-1 video (ISO 11172-2) elementary-stream decoder, for the opening movie (config 0022): sequence headers and
//! their quantiser matrices, I / P / B pictures, slices, macroblocks with their VLCs, dequantisation, an 8 x 8 inverse DCT
//! and half-pel motion compensation; frames come out in display order as Y / Cb / Cr planes.

use std::io::Read;

/// A decoded picture: planes padded to whole macroblocks (Y w x h, Cb / Cr w/2 x h/2).
#[derive(Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub cb: Vec<u8>,
    pub cr: Vec<u8>,
}

impl Frame {
    fn new(w: usize, h: usize) -> Self {
        Frame { width: w, height: h, y: vec![0; w * h], cb: vec![128; w * h / 4], cr: vec![128; w * h / 4] }
    }
}

/// Bits over a stream, refilled as it is read; `xor` de-obfuscates the bytes by their file offset.
struct Bits {
    src: Box<dyn Read + Send>,
    buf: Vec<u8>,
    /// Bit position in `buf`; the file offset of buf[0].
    pos: usize,
    base: u64,
    eof: bool,
    xor: Option<[u8; 8]>,
}

impl Bits {
    const CHUNK: usize = 1 << 20;

    /// Keeps at least `want` bytes ahead of the read position while the stream lasts.
    fn fill(&mut self, want: usize) {
        let byte = self.pos / 8;
        if self.buf.len() - byte >= want || self.eof {
            return;
        }
        self.buf.drain(..byte);
        self.base += byte as u64;
        self.pos -= byte * 8;
        while self.buf.len() - self.pos / 8 < want.max(Self::CHUNK) && !self.eof {
            let at = self.buf.len();
            self.buf.resize(at + Self::CHUNK, 0);
            let n = self.src.read(&mut self.buf[at..]).unwrap_or(0);
            self.buf.truncate(at + n);
            if let Some(k) = self.xor {
                for (i, b) in self.buf[at..].iter_mut().enumerate() {
                    *b ^= k[((self.base + (at + i) as u64) % 8) as usize];
                }
            }
            if n == 0 {
                self.eof = true;
            }
        }
    }

    fn peek(&mut self, n: u32) -> u32 {
        self.fill(8);
        let mut v = 0u64;
        let byte = self.pos / 8;
        for k in 0..5 {
            v = (v << 8) | *self.buf.get(byte + k).unwrap_or(&0) as u64;
        }
        ((v >> (40 - n as u64 - (self.pos % 8) as u64)) & ((1u64 << n) - 1)) as u32
    }

    fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek(n);
        self.pos += n as usize;
        v
    }

    fn skip(&mut self, n: u32) {
        self.pos += n as usize;
    }

    fn done(&self) -> bool {
        self.eof && self.pos / 8 + 3 >= self.buf.len()
    }

    /// The next start code (00 00 01 xx) from the next byte boundary, read past; None at the end.
    fn next_start(&mut self) -> Option<u8> {
        let c = self.peek_start()?;
        self.pos += 32;
        Some(c)
    }

    /// Moves to the next start code from the next byte boundary and names it, leaving it unread.
    fn peek_start(&mut self) -> Option<u8> {
        self.pos = self.pos.div_ceil(8) * 8;
        loop {
            self.fill(4);
            let byte = self.pos / 8;
            if byte + 3 >= self.buf.len() {
                if self.eof {
                    return None;
                }
                continue;
            }
            let b = &self.buf[byte..];
            if b[0] == 0 && b[1] == 0 && b[2] == 1 {
                return Some(b[3]);
            }
            self.pos += 8;
        }
    }

    /// 23 zero bits next: the slice's macroblocks end (a start code follows).
    fn slice_ends(&mut self) -> bool {
        self.peek(23) == 0
    }
}

/// A VLC table: (code length, code, value).
type Vlc = &'static [(u8, u16, i16)];

fn vlc(b: &mut Bits, t: Vlc) -> Option<i16> {
    let v = b.peek(16);
    for &(len, code, val) in t {
        if (v >> (16 - len as u32)) as u16 == code {
            b.skip(len as u32);
            return Some(val);
        }
    }
    None
}

/// macroblock_address_increment (34 = stuffing, 35 = escape).
const MB_INCREMENT: Vlc = &[
    (1, 0b1, 1), (3, 0b011, 2), (3, 0b010, 3), (4, 0b0011, 4), (4, 0b0010, 5), (5, 0b00011, 6), (5, 0b00010, 7),
    (7, 0b0000111, 8), (7, 0b0000110, 9), (8, 0b00001011, 10), (8, 0b00001010, 11), (8, 0b00001001, 12),
    (8, 0b00001000, 13), (8, 0b00000111, 14), (8, 0b00000110, 15), (10, 0b0000010111, 16), (10, 0b0000010110, 17),
    (10, 0b0000010101, 18), (10, 0b0000010100, 19), (10, 0b0000010011, 20), (10, 0b0000010010, 21),
    (11, 0b00000100011, 22), (11, 0b00000100010, 23), (11, 0b00000100001, 24), (11, 0b00000100000, 25),
    (11, 0b00000011111, 26), (11, 0b00000011110, 27), (11, 0b00000011101, 28), (11, 0b00000011100, 29),
    (11, 0b00000011011, 30), (11, 0b00000011010, 31), (11, 0b00000011001, 32), (11, 0b00000011000, 33),
    (11, 0b00000001111, 34), (11, 0b00000001000, 35),
];

/// macroblock_type flags: 1 quant, 2 forward, 4 backward, 8 pattern, 16 intra.
const MB_TYPE_I: Vlc = &[(1, 0b1, 16), (2, 0b01, 17)];
const MB_TYPE_P: Vlc = &[(1, 0b1, 10), (2, 0b01, 8), (3, 0b001, 2), (5, 0b00011, 16), (5, 0b00010, 11), (5, 0b00001, 9), (6, 0b000001, 17)];
const MB_TYPE_B: Vlc = &[
    (2, 0b10, 6), (2, 0b11, 14), (3, 0b010, 4), (3, 0b011, 12), (4, 0b0010, 2), (4, 0b0011, 10), (5, 0b00011, 16),
    (5, 0b00010, 15), (6, 0b000011, 11), (6, 0b000010, 13), (6, 0b000001, 17),
];

const CBP: Vlc = &[
    (3, 0b111, 60), (4, 0b1101, 4), (4, 0b1100, 8), (4, 0b1011, 16), (4, 0b1010, 32), (5, 0b10011, 12), (5, 0b10010, 48),
    (5, 0b10001, 20), (5, 0b10000, 40), (5, 0b01111, 28), (5, 0b01110, 44), (5, 0b01101, 52), (5, 0b01100, 56),
    (5, 0b01011, 1), (5, 0b01010, 61), (5, 0b01001, 2), (5, 0b01000, 62), (6, 0b001111, 24), (6, 0b001110, 36),
    (6, 0b001101, 3), (6, 0b001100, 63), (7, 0b0010111, 5), (7, 0b0010110, 9), (7, 0b0010101, 17), (7, 0b0010100, 33),
    (7, 0b0010011, 6), (7, 0b0010010, 10), (7, 0b0010001, 18), (7, 0b0010000, 34), (8, 0b00011111, 7),
    (8, 0b00011110, 11), (8, 0b00011101, 19), (8, 0b00011100, 35), (8, 0b00011011, 13), (8, 0b00011010, 49),
    (8, 0b00011001, 21), (8, 0b00011000, 41), (8, 0b00010111, 14), (8, 0b00010110, 50), (8, 0b00010101, 22),
    (8, 0b00010100, 42), (8, 0b00010011, 15), (8, 0b00010010, 51), (8, 0b00010001, 23), (8, 0b00010000, 43),
    (8, 0b00001111, 25), (8, 0b00001110, 37), (8, 0b00001101, 26), (8, 0b00001100, 38), (8, 0b00001011, 29),
    (8, 0b00001010, 45), (8, 0b00001001, 53), (8, 0b00001000, 57), (8, 0b00000111, 30), (8, 0b00000110, 46),
    (8, 0b00000101, 54), (8, 0b00000100, 58), (9, 0b000000111, 31), (9, 0b000000110, 47), (9, 0b000000101, 55),
    (9, 0b000000100, 59), (9, 0b000000011, 27), (9, 0b000000010, 39),
];

/// motion_code magnitudes (the sign bit follows every nonzero one).
const MOTION: Vlc = &[
    (1, 0b1, 0), (2, 0b01, 1), (3, 0b001, 2), (4, 0b0001, 3), (6, 0b000011, 4), (7, 0b0000101, 5), (7, 0b0000100, 6),
    (7, 0b0000011, 7), (9, 0b000001011, 8), (9, 0b000001010, 9), (9, 0b000001001, 10), (10, 0b0000010001, 11),
    (10, 0b0000010000, 12), (10, 0b0000001111, 13), (10, 0b0000001110, 14), (10, 0b0000001101, 15), (10, 0b0000001100, 16),
];

const DC_LUMA: Vlc = &[(3, 0b100, 0), (2, 0b00, 1), (2, 0b01, 2), (3, 0b101, 3), (3, 0b110, 4), (4, 0b1110, 5), (5, 0b11110, 6), (6, 0b111110, 7), (7, 0b1111110, 8)];
const DC_CHROMA: Vlc = &[(2, 0b00, 0), (2, 0b01, 1), (2, 0b10, 2), (3, 0b110, 3), (4, 0b1110, 4), (5, 0b11110, 5), (6, 0b111110, 6), (7, 0b1111110, 7), (8, 0b11111110, 8)];

/// dct_coeff_next as (length without the sign, code, run << 8 | level); 0x7fff = end of block, 0x7ffe = escape.
const DCT: &[(u8, u16, i16)] = &[
    (2, 0b10, 0x7fff), (2, 0b11, 0x001), (3, 0b011, 0x101), (4, 0b0100, 0x002), (4, 0b0101, 0x201), (5, 0b00101, 0x003),
    (5, 0b00111, 0x301), (5, 0b00110, 0x401), (6, 0b000110, 0x102), (6, 0b000111, 0x501), (6, 0b000101, 0x601),
    (6, 0b000100, 0x701), (6, 0b000001, 0x7ffe), (7, 0b0000110, 0x004), (7, 0b0000100, 0x202), (7, 0b0000111, 0x801),
    (7, 0b0000101, 0x901), (8, 0b00100110, 0x005), (8, 0b00100001, 0x006), (8, 0b00100101, 0x103), (8, 0b00100100, 0x302),
    (8, 0b00100111, 0xa01), (8, 0b00100011, 0xb01), (8, 0b00100010, 0xc01), (8, 0b00100000, 0xd01),
    (10, 0b0000001010, 0x007), (10, 0b0000001100, 0x104), (10, 0b0000001011, 0x203), (10, 0b0000001111, 0x402),
    (10, 0b0000001001, 0x502), (10, 0b0000001110, 0xe01), (10, 0b0000001101, 0xf01), (10, 0b0000001000, 0x1001),
    (12, 0b000000011101, 0x008), (12, 0b000000011000, 0x009), (12, 0b000000010011, 0x00a), (12, 0b000000010000, 0x00b),
    (12, 0b000000011011, 0x105), (12, 0b000000010100, 0x204), (12, 0b000000011100, 0x303), (12, 0b000000010010, 0x403),
    (12, 0b000000011110, 0x602), (12, 0b000000010101, 0x702), (12, 0b000000010001, 0x802), (12, 0b000000011111, 0x1101),
    (12, 0b000000011010, 0x1201), (12, 0b000000011001, 0x1301), (12, 0b000000010111, 0x1401), (12, 0b000000010110, 0x1501),
    (13, 0b0000000011010, 0x00c), (13, 0b0000000011001, 0x00d), (13, 0b0000000011000, 0x00e), (13, 0b0000000010111, 0x00f),
    (13, 0b0000000010110, 0x106), (13, 0b0000000010101, 0x107), (13, 0b0000000010100, 0x205), (13, 0b0000000010011, 0x304),
    (13, 0b0000000010010, 0x503), (13, 0b0000000010001, 0x902), (13, 0b0000000010000, 0xa02), (13, 0b0000000011111, 0x1601),
    (13, 0b0000000011110, 0x1701), (13, 0b0000000011101, 0x1801), (13, 0b0000000011100, 0x1901), (13, 0b0000000011011, 0x1a01),
    (14, 0b00000000011111, 0x010), (14, 0b00000000011110, 0x011), (14, 0b00000000011101, 0x012), (14, 0b00000000011100, 0x013),
    (14, 0b00000000011011, 0x014), (14, 0b00000000011010, 0x015), (14, 0b00000000011001, 0x016), (14, 0b00000000011000, 0x017),
    (14, 0b00000000010111, 0x018), (14, 0b00000000010110, 0x019), (14, 0b00000000010101, 0x01a), (14, 0b00000000010100, 0x01b),
    (14, 0b00000000010011, 0x01c), (14, 0b00000000010010, 0x01d), (14, 0b00000000010001, 0x01e), (14, 0b00000000010000, 0x01f),
    (15, 0b000000000011000, 0x020), (15, 0b000000000010111, 0x021), (15, 0b000000000010110, 0x022),
    (15, 0b000000000010101, 0x023), (15, 0b000000000010100, 0x024), (15, 0b000000000010011, 0x025),
    (15, 0b000000000010010, 0x026), (15, 0b000000000010001, 0x027), (15, 0b000000000010000, 0x028),
    (15, 0b000000000011111, 0x108), (15, 0b000000000011110, 0x109), (15, 0b000000000011101, 0x10a),
    (15, 0b000000000011100, 0x10b), (15, 0b000000000011011, 0x10c), (15, 0b000000000011010, 0x10d),
    (15, 0b000000000011001, 0x10e), (16, 0b0000000000010011, 0x10f), (16, 0b0000000000010010, 0x110),
    (16, 0b0000000000010001, 0x111), (16, 0b0000000000010000, 0x112), (16, 0b0000000000010100, 0x603),
    (16, 0b0000000000011010, 0xb02), (16, 0b0000000000011001, 0xc02), (16, 0b0000000000011000, 0xd02),
    (16, 0b0000000000010111, 0xe02), (16, 0b0000000000010110, 0xf02), (16, 0b0000000000010101, 0x1002),
    (16, 0b0000000000011111, 0x1b01), (16, 0b0000000000011110, 0x1c01), (16, 0b0000000000011101, 0x1d01),
    (16, 0b0000000000011100, 0x1e01), (16, 0b0000000000011011, 0x1f01),
];

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50,
    43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const INTRA_Q: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32,
    35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58, 26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

/// The 8 x 8 cosine basis of the inverse DCT: c[x][u] = C(u) / 2 cos((2x + 1) u pi / 16).
fn basis() -> [[f32; 8]; 8] {
    let mut c = [[0f32; 8]; 8];
    for (x, row) in c.iter_mut().enumerate() {
        for (u, v) in row.iter_mut().enumerate() {
            let cu = if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
            *v = 0.5 * cu * (((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI) / 16.0).cos();
        }
    }
    c
}

/// Motion: (recon right, recon down) of the forward / backward vectors.
#[derive(Clone, Copy, Default)]
struct Mv {
    h: i32,
    v: i32,
}

pub struct Decoder {
    bits: Bits,
    pub width: usize,
    pub height: usize,
    /// Frames a second (frame_rate_code), 0 before the first sequence header.
    pub fps: f64,
    mb_w: usize,
    mb_h: usize,
    intra_q: [u8; 64],
    non_intra_q: [u8; 64],
    /// The two reference pictures (older, newer) and whether the newer one still waits to be shown.
    fwd: Frame,
    bwd: Frame,
    held: bool,
    cur: Frame,
    cos: [[f32; 8]; 8],
    // Per picture and slice state.
    ptype: u32,
    fwd_r: u32,
    fwd_full: bool,
    bwd_r: u32,
    bwd_full: bool,
    qscale: i32,
    dc_pred: [i32; 3],
    mv_f: Mv,
    mv_b: Mv,
    last_type: i16,
}

impl Decoder {
    /// A decoder over `src`; `xor` the de-obfuscation key with the file offset of the stream's first byte.
    pub fn new(src: Box<dyn Read + Send>, xor: Option<([u8; 8], u64)>) -> Self {
        let (key, base) = xor.map_or((None, 0), |(k, b)| (Some(k), b));
        let bits = Bits { src, buf: Vec::new(), pos: 0, base, eof: false, xor: key };
        Decoder {
            bits,
            width: 0,
            height: 0,
            fps: 0.0,
            mb_w: 0,
            mb_h: 0,
            intra_q: INTRA_Q,
            non_intra_q: [16; 64],
            fwd: Frame::new(16, 16),
            bwd: Frame::new(16, 16),
            held: false,
            cur: Frame::new(16, 16),
            cos: basis(),
            ptype: 0,
            fwd_r: 0,
            fwd_full: false,
            bwd_r: 0,
            bwd_full: false,
            qscale: 1,
            dc_pred: [1024; 3],
            mv_f: Mv::default(),
            mv_b: Mv::default(),
            last_type: 0,
        }
    }

    /// The file offset reached.
    #[cfg(test)]
    fn offset(&self) -> u64 {
        self.bits.base + (self.bits.pos / 8) as u64
    }

    /// The next picture in display order; None at the end of the stream.
    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let Some(code) = self.bits.next_start() else {
                // The last reference picture is shown at the end.
                if self.held {
                    self.held = false;
                    return Some(self.bwd.clone());
                }
                return None;
            };
            match code {
                0xb3 => self.sequence(),
                0x00 if self.width > 0 => {
                    if let Some(f) = self.picture() {
                        return Some(f);
                    }
                }
                0xb7 => {}
                _ => {}
            }
        }
    }

    fn sequence(&mut self) {
        let b = &mut self.bits;
        let (w, h) = (b.read(12) as usize, b.read(12) as usize);
        b.skip(4);
        let rate = b.read(4);
        b.skip(18 + 1 + 10 + 1);
        if b.read(1) == 1 {
            for &z in &ZIGZAG {
                self.intra_q[z] = b.read(8) as u8;
            }
        } else {
            self.intra_q = INTRA_Q;
        }
        if b.read(1) == 1 {
            for &z in &ZIGZAG {
                self.non_intra_q[z] = b.read(8) as u8;
            }
        } else {
            self.non_intra_q = [16; 64];
        }
        self.fps = [0.0, 24000.0 / 1001.0, 24.0, 25.0, 30000.0 / 1001.0, 30.0, 50.0, 60000.0 / 1001.0, 60.0].get(rate as usize).copied().unwrap_or(30.0);
        if (w, h) != (self.width, self.height) && w > 0 && h > 0 {
            (self.width, self.height) = (w, h);
            (self.mb_w, self.mb_h) = (w.div_ceil(16), h.div_ceil(16));
            let (pw, ph) = (self.mb_w * 16, self.mb_h * 16);
            (self.fwd, self.bwd, self.cur) = (Frame::new(pw, ph), Frame::new(pw, ph), Frame::new(pw, ph));
            self.held = false;
        }
    }

    /// One picture header and its slices; the picture to show now, if any.
    fn picture(&mut self) -> Option<Frame> {
        let b = &mut self.bits;
        b.skip(10);
        self.ptype = b.read(3);
        b.skip(16);
        if self.ptype == 2 || self.ptype == 3 {
            self.fwd_full = b.read(1) == 1;
            self.fwd_r = b.read(3).max(1) - 1;
        }
        if self.ptype == 3 {
            self.bwd_full = b.read(1) == 1;
            self.bwd_r = b.read(3).max(1) - 1;
        }
        if !(1..=3).contains(&self.ptype) {
            return None;
        }
        // A reference picture: the newer reference becomes the older one.
        if self.ptype != 3 {
            std::mem::swap(&mut self.fwd, &mut self.bwd);
        }
        // The slices; any other start code is left for next_frame.
        while let Some(code) = self.bits.peek_start().filter(|c| (0x01..=0xaf).contains(c)) {
            self.bits.skip(32);
            self.slice(code as usize - 1);
        }
        if self.ptype == 3 {
            // B pictures are shown at once.
            return Some(self.cur.clone());
        }
        // The new reference picture waits; the one it replaces is shown now.
        std::mem::swap(&mut self.bwd, &mut self.cur);
        let show = self.held.then(|| self.fwd.clone());
        self.held = true;
        show
    }

    fn slice(&mut self, row: usize) {
        self.qscale = self.bits.read(5) as i32;
        while self.bits.read(1) == 1 {
            self.bits.skip(8);
        }
        self.dc_pred = [1024; 3];
        (self.mv_f, self.mv_b) = (Mv::default(), Mv::default());
        let mut addr: i64 = (row * self.mb_w) as i64 - 1;
        let total = (self.mb_w * self.mb_h) as i64;
        let mut first = true;
        loop {
            // macroblock_address_increment, with stuffing and escapes.
            let mut inc = 0i64;
            loop {
                match vlc(&mut self.bits, MB_INCREMENT) {
                    Some(34) => continue,
                    Some(35) => inc += 33,
                    Some(n) => {
                        inc += n as i64;
                        break;
                    }
                    None => return,
                }
            }
            // Skipped macroblocks (only between a slice's coded ones).
            if !first && inc > 1 {
                for a in addr + 1..addr + inc {
                    if a >= total {
                        return;
                    }
                    self.skipped(a as usize);
                }
            }
            addr += inc;
            first = false;
            if addr >= total {
                return;
            }
            if !self.macroblock(addr as usize) {
                return;
            }
            if self.bits.slice_ends() || self.bits.done() {
                return;
            }
        }
    }

    /// A skipped macroblock: P copies the forward picture in place; B repeats the last prediction.
    fn skipped(&mut self, addr: usize) {
        self.dc_pred = [1024; 3];
        if self.ptype == 2 {
            self.mv_f = Mv::default();
            self.predict(addr, Some(Mv::default()), None);
        } else if self.ptype == 3 {
            let t = self.last_type;
            self.predict(addr, (t & 2 != 0).then_some(self.mv_f), (t & 4 != 0).then_some(self.mv_b));
        }
    }

    fn macroblock(&mut self, addr: usize) -> bool {
        let table = match self.ptype {
            1 => MB_TYPE_I,
            2 => MB_TYPE_P,
            _ => MB_TYPE_B,
        };
        let Some(t) = vlc(&mut self.bits, table) else { return false };
        if t & 1 != 0 {
            self.qscale = self.bits.read(5) as i32;
        }
        let intra = t & 16 != 0;
        if intra {
            (self.mv_f, self.mv_b) = (Mv::default(), Mv::default());
        } else {
            self.dc_pred = [1024; 3];
            if t & 2 != 0 {
                let (r, full) = (self.fwd_r, self.fwd_full);
                self.mv_f = self.motion(self.mv_f, r, full);
            } else if self.ptype == 2 {
                self.mv_f = Mv::default();
            }
            if t & 4 != 0 {
                let (r, full) = (self.bwd_r, self.bwd_full);
                self.mv_b = self.motion(self.mv_b, r, full);
            }
            let (f, bk) = if self.ptype == 2 { (Some(self.mv_f), None) } else { ((t & 2 != 0).then_some(self.mv_f), (t & 4 != 0).then_some(self.mv_b)) };
            self.predict(addr, f, bk);
        }
        self.last_type = t;
        let cbp = if intra {
            63
        } else if t & 8 != 0 {
            match vlc(&mut self.bits, CBP) {
                Some(c) => c as u32,
                None => return false,
            }
        } else {
            0
        };
        for k in 0..6 {
            if cbp & (32 >> k) != 0 && !self.block(addr, k, intra) {
                return false;
            }
        }
        true
    }

    /// One motion vector against its predictor (full-pel ones doubled after the wrap).
    fn motion(&mut self, pred: Mv, r: u32, full: bool) -> Mv {
        let comp = |b: &mut Bits, p: i32| -> i32 {
            let code = vlc(b, MOTION).unwrap_or(0) as i32;
            let code = if code != 0 && b.read(1) == 1 { -code } else { code };
            let f = 1i32 << r;
            let delta = if code == 0 || f == 1 {
                code
            } else {
                let rr = b.read(r) as i32;
                let d = ((code.abs() - 1) << r) + rr + 1;
                if code < 0 { -d } else { d }
            };
            let p = if full { p >> 1 } else { p };
            let mut v = p + delta;
            if v > (f << 4) - 1 {
                v -= f << 5;
            } else if v < -(f << 4) {
                v += f << 5;
            }
            if full { v << 1 } else { v }
        };
        let h = comp(&mut self.bits, pred.h);
        let v = comp(&mut self.bits, pred.v);
        Mv { h, v }
    }

    /// The prediction of a macroblock into `cur` from the forward and / or backward references (averaged).
    fn predict(&mut self, addr: usize, f: Option<Mv>, b: Option<Mv>) {
        let (mx, my) = (addr % self.mb_w, addr / self.mb_w);
        let pw = self.mb_w * 16;
        let ph = self.mb_h * 16;
        for plane in 0..3 {
            let (size, w, h, scale) = if plane == 0 { (16, pw, ph, 1) } else { (8, pw / 2, ph / 2, 2) };
            let mut acc = [0u16; 256];
            let mut n = 0;
            for (mv, src) in [(f, &self.fwd), (b, &self.bwd)] {
                let Some(mv) = mv else { continue };
                let sp = match plane {
                    0 => &src.y,
                    1 => &src.cb,
                    _ => &src.cr,
                };
                let (mh, mvv) = if scale == 1 { (mv.h, mv.v) } else { (mv.h / 2, mv.v / 2) };
                let (oh, ov) = (mh & 1, mvv & 1);
                let (bx, by) = ((mx * size) as i32 + (mh >> 1), (my * size) as i32 + (mvv >> 1));
                let px = |x: i32, y: i32| sp[(y.clamp(0, h as i32 - 1) as usize) * w + x.clamp(0, w as i32 - 1) as usize] as u16;
                for y in 0..size as i32 {
                    for x in 0..size as i32 {
                        let (sx, sy) = (bx + x, by + y);
                        let v = match (oh, ov) {
                            (0, 0) => px(sx, sy),
                            (1, 0) => (px(sx, sy) + px(sx + 1, sy) + 1) >> 1,
                            (0, 1) => (px(sx, sy) + px(sx, sy + 1) + 1) >> 1,
                            _ => (px(sx, sy) + px(sx + 1, sy) + px(sx, sy + 1) + px(sx + 1, sy + 1) + 2) >> 2,
                        };
                        let i = (y * size as i32 + x) as usize;
                        acc[i] = if n == 0 { v } else { (acc[i] + v + 1) >> 1 };
                    }
                }
                n += 1;
            }
            if n == 0 {
                continue;
            }
            let dst = match plane {
                0 => &mut self.cur.y,
                1 => &mut self.cur.cb,
                _ => &mut self.cur.cr,
            };
            for y in 0..size {
                for x in 0..size {
                    dst[(my * size + y) * w + mx * size + x] = acc[y * size + x] as u8;
                }
            }
        }
    }

    /// One 8 x 8 block: its coefficients, dequantised, inverse transformed, written (intra) or added (prediction).
    fn block(&mut self, addr: usize, k: usize, intra: bool) -> bool {
        let mut coef = [0i32; 64];
        let mut i = 0usize;
        if intra {
            let ci = if k < 4 { 0 } else { k - 3 };
            let Some(size) = vlc(&mut self.bits, if k < 4 { DC_LUMA } else { DC_CHROMA }) else { return false };
            let diff = if size == 0 {
                0
            } else {
                let v = self.bits.read(size as u32) as i32;
                if v & (1 << (size - 1)) != 0 { v } else { v - (1 << size) + 1 }
            };
            self.dc_pred[ci] += diff * 8;
            coef[0] = self.dc_pred[ci];
            i = 1;
        } else if self.bits.peek(1) == 1 {
            // The first coefficient of a non-intra block: '1s' is run 0, level 1.
            self.bits.skip(1);
            let level = if self.bits.read(1) == 1 { -1 } else { 1 };
            coef[0] = self.dequant(level, 0, false);
            i = 1;
        }
        loop {
            let v = self.bits.peek(16);
            let Some(&(len, _, rl)) = DCT.iter().find(|&&(len, code, _)| (v >> (16 - len as u32)) as u16 == code) else { return false };
            self.bits.skip(len as u32);
            if rl == 0x7fff {
                break;
            }
            let (run, level) = if rl == 0x7ffe {
                let run = self.bits.read(6) as usize;
                let mut level = self.bits.read(8) as i32;
                if level == 0 {
                    level = self.bits.read(8) as i32;
                } else if level == 128 {
                    level = self.bits.read(8) as i32 - 256;
                } else if level > 128 {
                    level -= 256;
                }
                (run, level)
            } else {
                let l = (rl & 0xff) as i32;
                ((rl >> 8) as usize, if self.bits.read(1) == 1 { -l } else { l })
            };
            i += run;
            if i >= 64 {
                return false;
            }
            let z = ZIGZAG[i];
            coef[z] = self.dequant(level, z, intra);
            i += 1;
        }
        // Inverse DCT (separable; a DC-only block is flat).
        let mut out = [0f32; 64];
        if coef[1..].iter().all(|&c| c == 0) {
            out = [coef[0] as f32 / 8.0; 64];
        } else {
            let mut tmp = [0f32; 64];
            for y in 0..8 {
                for x in 0..8 {
                    let mut s = 0.0;
                    for u in 0..8 {
                        s += self.cos[x][u] * coef[y * 8 + u] as f32;
                    }
                    tmp[y * 8 + x] = s;
                }
            }
            for x in 0..8 {
                for y in 0..8 {
                    let mut s = 0.0;
                    for v in 0..8 {
                        s += self.cos[y][v] * tmp[v * 8 + x];
                    }
                    out[y * 8 + x] = s;
                }
            }
        }
        let (mx, my) = (addr % self.mb_w, addr / self.mb_w);
        let pw = self.mb_w * 16;
        let (dst, w, ox, oy) = match k {
            0..=3 => (&mut self.cur.y, pw, mx * 16 + (k & 1) * 8, my * 16 + (k >> 1) * 8),
            4 => (&mut self.cur.cb, pw / 2, mx * 8, my * 8),
            _ => (&mut self.cur.cr, pw / 2, mx * 8, my * 8),
        };
        for y in 0..8 {
            for x in 0..8 {
                let d = &mut dst[(oy + y) * w + ox + x];
                let v = out[y * 8 + x].round() as i32 + if intra { 0 } else { *d as i32 };
                *d = v.clamp(0, 255) as u8;
            }
        }
        true
    }

    /// ISO 11172-2 2.4.4: intra (level x q x W) / 8, non-intra ((2 level + sign) x q x W) / 16, made odd, clamped.
    fn dequant(&self, level: i32, z: usize, intra: bool) -> i32 {
        let mut c = if intra {
            level * self.qscale * self.intra_q[z] as i32 / 8
        } else {
            (2 * level + level.signum()) * self.qscale * self.non_intra_q[z] as i32 / 16
        };
        if c & 1 == 0 && c != 0 {
            c -= c.signum();
        }
        c.clamp(-2048, 2047)
    }
}

/// The opening movie (mov999.pmv): "PMUS", then the MPEG-1 stream from byte 4 XORed with this key by file offset.
pub const PMV_KEY: [u8; 8] = [0xc2, 0xee, 0x9c, 0xd9, 0xbd, 0x6c, 0x4d, 0x72];

/// A decoder over a .pmv file (None unless it starts with "PMUS").
pub fn open_pmv(path: &std::path::Path) -> Option<Decoder> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).ok()?;
    if &magic != b"PMUS" {
        return None;
    }
    Some(Decoder::new(Box::new(std::io::BufReader::new(f)), Some((PMV_KEY, 4))))
}

#[cfg(test)]
mod tests {
    /// The bit reader alone finds all 12,620 picture start codes of the install's movie.
    #[test]
    fn opening_movie_start_codes() {
        let cfg = crate::config::Config::load();
        let Some(mut d) = super::open_pmv(&cfg.ffxi_dir().join("mov").join("mov999.pmv")) else { return };
        let mut pictures = 0;
        while let Some(c) = d.bits.next_start() {
            if c == 0 {
                pictures += 1;
            }
        }
        assert_eq!((pictures, d.offset()), (12620, 315_815_512));
    }

    /// The install's movie: a 512 x 336 stream at 29.97 fps whose first frames decode.
    #[test]
    fn opening_movie_frames() {
        let cfg = crate::config::Config::load();
        let Some(mut d) = super::open_pmv(&cfg.ffxi_dir().join("mov").join("mov999.pmv")) else { return };
        let f = d.next_frame().expect("a frame");
        assert_eq!((d.width, d.height), (512, 336));
        assert!((d.fps - 29.97).abs() < 0.01);
        assert_eq!(f.y.len(), 512 * 336);
        for _ in 0..30 {
            assert!(d.next_frame().is_some());
        }
    }
}
