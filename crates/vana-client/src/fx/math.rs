//! Retail's math interface (DAT_1046a64c vtable) in glam terms. Retail uses D3DX row vectors (v' = v·M, "a then
//! b" = a·b); glam uses column vectors, so a retail product a·b is `b * a` here and TransformCoord(v, M) is
//! `M.transform_point3(v)`.

use glam::{Mat4, Vec3};

/// Vtable +0x58 MatrixFromEuler(rx, ry, rz) = FUN_1006cd40: Rz·Ry·Rx in row vectors (Z applied first, X last).
pub fn euler(r: Vec3) -> Mat4 {
    Mat4::from_rotation_x(r.x) * Mat4::from_rotation_y(r.y) * Mat4::from_rotation_z(r.z)
}

/// Retail a·b (apply a, then b).
pub fn then(a: Mat4, b: Mat4) -> Mat4 {
    b * a
}

/// Wraps an angle into [-pi, pi] the way ops 0x0a / 0x3b do (one step of 2pi).
pub fn wrap(a: f32) -> f32 {
    use std::f32::consts::PI;
    if a > PI {
        a - 2.0 * PI
    } else if a < -PI {
        a + 2.0 * PI
    } else {
        a
    }
}

/// The MT19937 helpers' value mappings (FUN_1006afd0 / FUN_1006b020 / FUN_1006afb0 / FUN_1006af90) over our own
/// generator *(inf: retail's sequence is not reproduced; only the mappings are)*.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u32
    }
    /// Uniform [0, x).
    pub fn uf(&mut self, x: f32) -> f32 {
        if x == 0.0 { 0.0 } else { (self.next() % 100_000) as f32 * 1e-5 * x }
    }
    /// Uniform [-x, x).
    pub fn us(&mut self, x: f32) -> f32 {
        if x == 0.0 { 0.0 } else { ((self.next() % 200_000) as f32 * 1e-5 - 1.0) * x }
    }
    /// 0..n-1 (unsigned).
    pub fn un(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
    /// -(n-1)..n-1 (signed: retail's IDIV of a 32-bit value).
    pub fn sn(&mut self, n: i32) -> i32 {
        if n == 0 { 0 } else { (self.next() as i32) % n }
    }
}
