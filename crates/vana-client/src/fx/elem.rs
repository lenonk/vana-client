//! An element's state as the GPU keeps it: the CMoElem fields the update stream touches (generators.md C.2),
//! one u32 array per element, floats as bits. The emission stream fills it on the CPU (fx/emit.rs); the
//! update stream runs on the GPU every frame (render/effects.wgsl).

/// Word offsets; each comment names the CMoElem field.
pub mod w {
    pub const POS: usize = 0; // +0x54..+0x5c
    pub const LIFE: usize = 3; // +0x110
    pub const ROT: usize = 4; // +0xe0..+0xe8
    pub const MAX_LIFE: usize = 7; // +0x114
    pub const SCALE: usize = 8; // +0xec..+0xf4
    pub const FADE: usize = 11; // +0x138
    pub const COLOR: usize = 12; // +0xf8, D3DCOLOR (B, G, R, A bytes)
    pub const FLAGS: usize = 13; // +0x10c
    pub const STREAM: usize = 14; // the update stream's first word in the ops buffer
    pub const PARENT: usize = 15; // matrix slot (generator +0xa4 or the +0x108 snapshot), NONE = world
    pub const UV: usize = 16; // +0x120, +0x124
    pub const BIAS: usize = 18; // +0x128
    pub const T: usize = 19; // life fraction (update op 0x0e)
    pub const FRAME_CD: usize = 20; // +0x118
    pub const FRAME_PERIOD: usize = 21; // +0x11c
    pub const FRAME: usize = 22; // +0x170
    pub const ALPHA: usize = 23; // +0x134
    pub const STATE: usize = 24; // st:: bits
    pub const BLEND: usize = 25; // +0x16c
    pub const OP2_SLOT: usize = 26; // slot of the update stream's op 0x02 (FUN_10048380(2)), for ops 0x30..0x32
    /// Generator +0xdf (zone bits) and +0xde (weather bits): byte 0 / byte 1.
    pub const GEN_BITS: usize = 27;
    /// CPU-written control bits (ctl::), cleared by the GPU after it reads them.
    pub const CTRL: usize = 28;
    /// Spawn serial, echoed in the readback so a reused slot's stale record is ignored.
    pub const SERIAL: usize = 29;
    /// D3a frame count (the draw-time wrap, FUN_1003dad0).
    pub const FRAMES: usize = 30;
    /// Draw kind (kind::) and, for batched D3a, the particle block (d3a_particles.md 4.2): first particle in the
    /// pool, N, elapsed ticks t, gravity, d = drag - 1.
    pub const KIND: usize = 31;
    pub const PART_BASE: usize = 32;
    pub const PART_N: usize = 33;
    pub const PART_T: usize = 34;
    pub const GRAVITY: usize = 35;
    pub const DRAG: usize = 38;
    /// The sprite's first entry in the GPU frame table.
    pub const SFRAMES: usize = 39;
    pub const SLOTS: usize = 40;
    pub const SLOT_COUNT: usize = 64;
    pub const WORDS: usize = SLOTS + SLOT_COUNT;
}

/// STATE bits.
pub mod st {
    pub const ALIVE: u32 = 1;
    /// +0x17b bits 0 / 1: U / V offset in use (the texture matrix is set).
    pub const SCROLL_U: u32 = 2;
    pub const SCROLL_V: u32 = 4;
    /// Generator flag 0x20000000 (batched): op 0x03 is skipped.
    pub const BATCHED: u32 = 16;
    /// The element has a resource (+0x104 != 0): op 0x0d animates frames.
    pub const HAS_RES: u32 = 32;
}

/// KIND values.
pub mod dk {
    pub const NONE: u32 = 0;
    pub const MESH: u32 = 1;
    pub const SPRITE: u32 = 2;
    pub const BATCHED: u32 = 3;
}

/// Particle record in the pool: pos0, dir, vel (9 floats).
pub const PART_WORDS: usize = 9;

/// CTRL bits.
pub mod ctl {
    /// The CPU set LIFE this frame (the expire stream restarted or held it): no decrement.
    pub const LIFE_SET: u32 = 1;
}

pub const NONE: u32 = u32::MAX;

/// Per element out of the update pass, for drawing: element-to-view matrix (0..16), the TFACTOR colour (bytes
/// / 255, alpha per FUN_1003d4f0) (16..20), UV offset (20, 21), frame (22), visible (23), projection depth bias
/// (24), particles drawn (25), particle t (26), d (27), gravity (28..31), particle placement matrix columns
/// (32..44).
pub const OUT_WORDS: usize = 44;
/// Per element back to the CPU (a frame late): sort key +0x12c, status bits, world position, rotation, scale,
/// colour.
pub const BACK_WORDS: usize = 16;
pub mod back {
    pub const KEY: usize = 0;
    pub const STATUS: usize = 1;
    pub const WORLD: usize = 2;
    pub const ROT: usize = 5;
    pub const SCALE: usize = 8;
    pub const COLOR: usize = 11;
    pub const SERIAL: usize = 12;
    /// STATUS: op 0x2e / 0x48 asked to expire (life = 1); drawn this frame.
    pub const KILL: u32 = 1;
    pub const DRAWN: u32 = 2;
}

/// Matrix slot (a generator's attach matrix, or an element's spawn snapshot): the matrix, scale vectors
/// +0x78 (position) and +0x84 (size), and per-generator values the update stream reads.
pub const SLOT_WORDS: usize = 28;
pub mod slot {
    pub const MATRIX: usize = 0;
    pub const SCALE_A: usize = 16;
    pub const SCALE_B: usize = 20;
    /// The owning weather's weight +0x1c (update op 0x50); 1 when not a weather generator.
    pub const WEATHER: usize = 24;
    /// Generator +0x37 low nibble != 0: the local matrix takes the size scale (FUN_10043f00).
    pub const SIZE_SCALED: usize = 25;
    pub const HAS_MATRIX: usize = 26;
}

/// One element's words.
#[derive(Clone)]
pub struct Elem(pub [u32; w::WORDS]);

impl Default for Elem {
    fn default() -> Self {
        let mut e = Elem([0; w::WORDS]);
        e.0[w::PARENT] = NONE;
        e.0[w::OP2_SLOT] = NONE;
        e
    }
}

impl Elem {
    pub fn f(&self, i: usize) -> f32 {
        f32::from_bits(self.0[i])
    }
    pub fn set(&mut self, i: usize, v: f32) {
        self.0[i] = v.to_bits();
    }
    pub fn v3(&self, i: usize) -> glam::Vec3 {
        glam::Vec3::new(self.f(i), self.f(i + 1), self.f(i + 2))
    }
    pub fn set_v3(&mut self, i: usize, v: glam::Vec3) {
        self.set(i, v.x);
        self.set(i + 1, v.y);
        self.set(i + 2, v.z);
    }
    /// Slot `s` (a dword of the state block).
    pub fn s(&self, s: usize) -> u32 {
        self.0.get(w::SLOTS + s).copied().unwrap_or(0)
    }
    pub fn sf(&self, s: usize) -> f32 {
        f32::from_bits(self.s(s))
    }
    pub fn set_s(&mut self, s: usize, v: u32) {
        if s < w::SLOT_COUNT {
            self.0[w::SLOTS + s] = v;
        }
    }
    pub fn set_sf(&mut self, s: usize, v: f32) {
        self.set_s(s, v.to_bits());
    }
    pub fn sv3(&self, s: usize) -> glam::Vec3 {
        glam::Vec3::new(self.sf(s), self.sf(s + 1), self.sf(s + 2))
    }
    pub fn set_sv3(&mut self, s: usize, v: glam::Vec3) {
        self.set_sf(s, v.x);
        self.set_sf(s + 1, v.y);
        self.set_sf(s + 2, v.z);
    }
    /// Colour byte by D3DCOLOR memory offset: 0 B, 1 G, 2 R, 3 A.
    pub fn cb(&self, k: usize) -> u8 {
        self.0[w::COLOR].to_le_bytes()[k]
    }
    pub fn set_cb(&mut self, k: usize, v: u8) {
        let mut b = self.0[w::COLOR].to_le_bytes();
        b[k] = v;
        self.0[w::COLOR] = u32::from_le_bytes(b);
    }
}

/// The WGSL constants for these offsets, prepended to render/effects.wgsl.
pub fn wgsl_consts() -> String {
    let mut s = String::new();
    let mut c = |n: &str, v: usize| s.push_str(&format!("const {n}: u32 = {v}u;\n"));
    for (n, v) in [
        ("E_POS", w::POS),
        ("E_LIFE", w::LIFE),
        ("E_ROT", w::ROT),
        ("E_MAX_LIFE", w::MAX_LIFE),
        ("E_SCALE", w::SCALE),
        ("E_FADE", w::FADE),
        ("E_COLOR", w::COLOR),
        ("E_FLAGS", w::FLAGS),
        ("E_STREAM", w::STREAM),
        ("E_PARENT", w::PARENT),
        ("E_UV", w::UV),
        ("E_BIAS", w::BIAS),
        ("E_T", w::T),
        ("E_FRAME_CD", w::FRAME_CD),
        ("E_FRAME_PERIOD", w::FRAME_PERIOD),
        ("E_FRAME", w::FRAME),
        ("E_ALPHA", w::ALPHA),
        ("E_STATE", w::STATE),
        ("E_BLEND", w::BLEND),
        ("E_OP2_SLOT", w::OP2_SLOT),
        ("E_GEN_BITS", w::GEN_BITS),
        ("E_FRAMES", w::FRAMES),
        ("E_KIND", w::KIND),
        ("E_PART_BASE", w::PART_BASE),
        ("E_PART_N", w::PART_N),
        ("E_PART_T", w::PART_T),
        ("E_GRAVITY", w::GRAVITY),
        ("E_DRAG", w::DRAG),
        ("E_SFRAMES", w::SFRAMES),
        ("DK_MESH", dk::MESH as usize),
        ("DK_SPRITE", dk::SPRITE as usize),
        ("DK_BATCHED", dk::BATCHED as usize),
        ("PART_WORDS", PART_WORDS),
        ("E_SLOTS", w::SLOTS),
        ("E_SLOT_COUNT", w::SLOT_COUNT),
        ("E_WORDS", w::WORDS),
        ("ST_ALIVE", st::ALIVE as usize),
        ("ST_SCROLL_U", st::SCROLL_U as usize),
        ("ST_SCROLL_V", st::SCROLL_V as usize),
        ("E_CTRL", w::CTRL),
        ("E_SERIAL", w::SERIAL),
        ("CTL_LIFE_SET", ctl::LIFE_SET as usize),
        ("ST_BATCHED", st::BATCHED as usize),
        ("ST_HAS_RES", st::HAS_RES as usize),
        ("OUT_WORDS", OUT_WORDS),
        ("BACK_WORDS", BACK_WORDS),
        ("B_KILL", back::KILL as usize),
        ("B_DRAWN", back::DRAWN as usize),
        ("SLOT_WORDS", SLOT_WORDS),
        ("S_SCALE_A", slot::SCALE_A),
        ("S_SCALE_B", slot::SCALE_B),
        ("S_WEATHER", slot::WEATHER),
        ("S_SIZE_SCALED", slot::SIZE_SCALED),
        ("S_HAS_MATRIX", slot::HAS_MATRIX),
    ] {
        c(n, v);
    }
    s.push_str(&format!("const NONE: u32 = {}u;\n", NONE));
    s
}

/// The update stream on the CPU, for checking the GPU pass (render/effects.wgsl `update`) in tests; the same ops
/// and the same order.
#[cfg(test)]
pub fn update_ref(e: &mut Elem, ops: &[u32], key: &dyn Fn(u32, f32, bool) -> f32, slot_words: &[u32], f: &super::Frame) {
    use glam::Vec3;
    use std::f32::consts::PI;
    if e.0[w::STATE] & st::ALIVE == 0 {
        return;
    }
    let dt = f.dt;
    if e.0[w::CTRL] & ctl::LIFE_SET == 0 && e.f(w::MAX_LIFE) != 0.0 {
        e.set(w::LIFE, e.f(w::LIFE) - dt);
    }
    e.0[w::CTRL] = 0;
    e.set(w::FADE, 1.0);
    let parent = e.0[w::PARENT];
    let sf = |k: usize| f32::from_bits(slot_words[parent as usize * SLOT_WORDS + k]);
    let world = |e: &Elem| -> Vec3 {
        if parent == NONE {
            return e.v3(w::POS);
        }
        let a = Vec3::new(sf(slot::SCALE_A), sf(slot::SCALE_A + 1), sf(slot::SCALE_A + 2));
        let wp = e.v3(w::POS) * a;
        if slot_words[parent as usize * SLOT_WORDS + slot::HAS_MATRIX] == 0 {
            return wp;
        }
        let m = glam::Mat4::from_cols_array(&std::array::from_fn(|k| sf(slot::MATRIX + k)));
        m.transform_point3(wp)
    };
    let kt = |e: &Elem, s: usize, t: f32| key(e.s(s), t, e.s(s + 1) & 0xf != 0);
    let kg = |e: &Elem, s: usize| {
        let n = (e.s(s + 1) >> 5) & 0x1ff;
        let x = if n != 0 { (f.dayfrac * n as f32).fract() } else { f.dayfrac };
        key(e.s(s), x, e.s(s + 1) & 0xf != 0)
    };
    let byte = |v: f32| ((v * 255.0) as i32 as u32 & 0xff) as u8;
    let mut ip = e.0[w::STREAM] as usize;
    let mut guard = 0;
    while ip < ops.len() && guard < 256 {
        guard += 1;
        let w0 = ops[ip];
        let (code, len, s) = (w0 & 0xff, ((w0 >> 8) & 0x1f) as usize, ((w0 >> 13) & 0x3f) as usize);
        if code == 0 || len == 0 {
            break;
        }
        let of = |k: usize| f32::from_bits(ops[ip + k]);
        let ov = |k: usize| Vec3::new(of(k), of(k + 1), of(k + 2));
        let (t, life, maxl) = (e.f(w::T), e.f(w::LIFE), e.f(w::MAX_LIFE));
        match code {
            0x01 => e.set_v3(w::POS, e.v3(w::POS) + ov(1) * dt),
            0x02 => e.set_v3(w::POS, e.v3(w::POS) + e.sv3(s) * dt),
            0x04 => e.set_v3(w::ROT, e.v3(w::ROT) + ov(1) * dt),
            0x05 => e.set_v3(w::ROT, e.v3(w::ROT) + e.sv3(s) * dt),
            0x06 | 0x09 => e.set_sv3(s, e.sv3(s) + ov(1) * dt),
            0x07 => e.set_v3(w::SCALE, e.v3(w::SCALE) + ov(1) * dt),
            0x08 => e.set_v3(w::SCALE, e.v3(w::SCALE) + e.sv3(s) * dt),
            0x0e => e.set(w::T, if maxl == 0.0 { 0.0 } else { 1.0 - life / maxl }),
            0x0f..=0x11 => {
                let v = e.sf(s + 2) + kt(e, s, t);
                e.set(w::POS + (code - 0x0f) as usize, v)
            }
            0x15..=0x17 => {
                let v = kt(e, s, t);
                e.set(w::SCALE + (code - 0x15) as usize, v)
            }
            0x18 => e.set_cb(2, byte(kt(e, s, t))),
            0x19 => e.set_cb(1, byte(kt(e, s, t))),
            0x1a => e.set_cb(0, byte(kt(e, s, t))),
            0x1b => e.set_cb(3, byte(kt(e, s, t))),
            0x27 => {
                e.set(w::UV, e.f(w::UV) + of(1) * dt);
                e.0[w::STATE] |= st::SCROLL_U;
            }
            0x28 => {
                e.set(w::UV + 1, e.f(w::UV + 1) + of(1) * dt);
                e.0[w::STATE] |= st::SCROLL_V;
            }
            0x2e => {
                let d = (f.eye - world(e)).length();
                let (near, far) = (of(1), of(2));
                if d > far {
                    e.set(w::FADE, 0.0);
                } else if d >= near {
                    e.set(w::FADE, e.f(w::FADE) * (1.0 - (d - near) / (far - near)));
                }
            }
            0x3c => e.set_cb(2, byte(kg(e, s))),
            0x3d => e.set_cb(1, byte(kg(e, s))),
            0x3e => e.set_cb(0, byte(kg(e, s))),
            0x3f => e.set(w::FADE, e.f(w::FADE) * kg(e, s)),
            0x40..=0x42 => {
                let v = kg(e, s);
                e.set(w::SCALE + (code - 0x40) as usize, v)
            }
            0x50 if parent != NONE => e.set(w::FADE, e.f(w::FADE) * sf(slot::WEATHER)),
            0x56..=0x58 => {
                let k = w::ROT + (code - 0x56) as usize;
                e.set(k, e.f(k) + kt(e, s, t) * PI * dt)
            }
            0x61..=0x63 => {
                let k = w::ROT + (code - 0x61) as usize;
                e.set(k, e.f(k) + kg(e, s) * PI * dt)
            }
            0x66..=0x68 => e.set(w::ROT + (code - 0x66) as usize, kg(e, s) * PI),
            0x6b..=0x6d => {
                let v = e.sf(s + 2) + kg(e, s);
                e.set(w::POS + (code - 0x6b) as usize, v)
            }
            _ => {}
        }
        ip += len;
    }
}
