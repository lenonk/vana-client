//! Ground tilt (spec ~/ffxi/lab/research/ground_tilt_spec.md): models of leg mode 1 / 2, and every dead actor, draw
//! along the ground under them (FUN_100d54c0) instead of the plain Euler rotation.

use glam::{Mat3, Quat, Vec3};

const UP: Vec3 = Vec3::NEG_Y;

/// Actor +0x160 / +0x828 / +0x82c: the ground normal (y < 0 up), the blend weight, the eased normal.
#[derive(Clone, Copy, Debug)]
pub struct GroundTilt {
    pub n: Vec3,
    pub w: f32,
    pub e: Vec3,
}

impl Default for GroundTilt {
    fn default() -> Self {
        GroundTilt { n: UP, w: 0.0, e: UP }
    }
}

/// FUN_100cbde0's gate: leg mode (MI[0], signed) 1 or 2, or a dead status (2 / 3).
pub fn eligible(leg_mode: i8, status: u8) -> bool {
    matches!(leg_mode, 1 | 2) || matches!(status, 2 | 3)
}

/// FUN_100aa820: a face normal replaces the ground normal only when |y| > 0.7.
pub fn set_normal(t: &mut GroundTilt, n: Vec3) {
    if n.y.abs() > 0.7 {
        t.n = n;
    }
}

/// FUN_100a4d10 (info[0] == info[2] == 2): the normal from the snapped front and rear feet midpoints, in the vertical
/// plane through them (pitch only), facing up; None when they share x and z.
pub fn feet_normal(front: Vec3, rear: Vec3) -> Option<Vec3> {
    if front.x == rear.x && front.z == rear.z {
        return None;
    }
    let d = rear - front;
    let n = d.cross(d.cross(UP)).normalize_or_zero();
    (n != Vec3::ZERO).then_some(if n.y > 0.0 { -n } else { n })
}

/// The eased fraction for `ticks` (FUN_100d54c0 eases 0.1875 a drawn 30 fps frame).
pub fn rate(ticks: f32) -> f32 {
    1.0 - 0.8125f32.powf(ticks / 2.0)
}

impl GroundTilt {
    /// One FUN_100d54c0 call: the ground normal clamped to 30 degrees from up, blended with up by last call's weight,
    /// eased into the stored normal; the frame keeps the Euler model Z axis `zf`; then the next weight: 1 for a
    /// square-ish status box, else the fall line along the body's long axis (with the scale). Returns the rotation
    /// columns (model X, Y = down, Z).
    pub fn step(&mut self, zf: Vec3, scale: Vec3, bx: [f32; 6], t: f32) -> Mat3 {
        let n = self.n;
        let nc = if UP.dot(n) >= 0.866_025_4 {
            n
        } else {
            let ax = UP.cross(n).normalize_or_zero();
            if ax == Vec3::ZERO { UP } else { (Quat::from_axis_angle(ax, std::f32::consts::FRAC_PI_6) * UP).normalize() }
        };
        let b = (UP * (1.0 - self.w) + nc * self.w).normalize_or_zero();
        self.e = (b * t + self.e * (1.0 - t)).normalize_or_zero();
        let dn = -self.e;
        let sx = dn.cross(zf.normalize_or_zero()).normalize_or_zero();
        let sz = sx.cross(dn).normalize_or_zero();
        let g = nc.cross(UP.cross(nc).normalize_or_zero()).normalize_or_zero();
        let r = (bx[2] - bx[3]) / (bx[4] - bx[5]);
        self.w = if r > 0.6 && r < 1.4 {
            1.0
        } else if r >= 1.0 || r.is_nan() {
            g.dot(sz * scale.z).abs()
        } else {
            g.dot(sx * scale.x).abs()
        };
        Mat3::from_cols(sx, dn, sz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slope(deg: f32) -> Vec3 {
        // Rising toward +x: the up-facing normal leans to -x.
        let a = deg.to_radians();
        Vec3::new(-a.sin(), -a.cos(), 0.0)
    }

    fn angle(e: Vec3) -> f32 {
        e.angle_between(UP).to_degrees()
    }

    const SQUARE: [f32; 6] = [-2.0, 0.0, 1.0, -1.0, 1.0, -1.0];

    /// ground_tilt_spec.md 5.2 test vectors: flat ground keeps the plain frame; 20 degrees eases 0, 3.711, 6.744,
    /// 9.218 ...; 60 degrees clamps at 30; the tiger's long box across a slope stays upright, facing uphill pitches.
    #[test]
    fn steps() {
        for yaw in [0.0f32, 0.7, 2.5] {
            let zf = Vec3::new(yaw.sin(), 0.0, yaw.cos());
            let mut t = GroundTilt::default();
            let m = t.step(zf, Vec3::ONE, SQUARE, 0.1875);
            let plain = Mat3::from_rotation_y(yaw);
            assert!((m.x_axis - plain.x_axis).length() < 1e-5 && (m.z_axis - plain.z_axis).length() < 1e-5 && (m.y_axis - Vec3::Y).length() < 1e-5);
        }
        let mut t = GroundTilt { n: slope(20.0), ..Default::default() };
        let got: Vec<f32> = (0..4).map(|_| {
            t.step(Vec3::Z, Vec3::ONE, SQUARE, 0.1875);
            angle(t.e)
        }).collect();
        for (g, want) in got.iter().zip([0.0, 3.711, 6.744, 9.218]) {
            assert!((g - want).abs() < 0.01, "{got:?}");
        }
        let mut t = GroundTilt { n: slope(60.0), ..Default::default() };
        for _ in 0..200 {
            t.step(Vec3::Z, Vec3::ONE, SQUARE, 0.1875);
        }
        assert!((angle(t.e) - 30.0).abs() < 1e-3);
        // The tiger's box 0 (ratio 0.31: long along X). Facing up the slope (+x) it pitches; across (facing z) it
        // stays upright.
        let tiger = [-2.52, 0.0, 0.94, -0.81, 2.51, -3.07];
        let mut up = GroundTilt { n: slope(20.0), ..Default::default() };
        let mut across = GroundTilt { n: slope(20.0), ..Default::default() };
        for _ in 0..200 {
            up.step(Vec3::NEG_Z, Vec3::ONE, tiger, 0.1875);
            across.step(Vec3::X, Vec3::ONE, tiger, 0.1875);
        }
        assert!((angle(up.e) - 20.0).abs() < 0.01 && angle(across.e) < 0.01, "{} {}", angle(up.e), angle(across.e));
        assert!(eligible(0, 2) && !eligible(0, 0) && eligible(2, 0) && !eligible(3, 0) && !eligible(-1, 0));
        let f = feet_normal(Vec3::new(1.0, -0.5, 0.0), Vec3::new(-1.0, 0.0, 0.0)).unwrap();
        assert!((f - Vec3::new(-0.2425, -0.9701, 0.0)).length() < 1e-4, "{f:?}");
        assert_eq!(feet_normal(Vec3::new(1.0, 0.0, 2.0), Vec3::new(1.0, -1.0, 2.0)), None);
    }
}
