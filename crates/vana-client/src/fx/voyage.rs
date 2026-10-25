//! A ship voyage's path (spec ~/ffxi/lab/research/ship_voyage_spec.md 1.5): the zone's type-6 chunk `c00N`, two
//! CMoSplines over its keys' points A and B (FUN_10055780), and the voyage matrix V, the rigid inverse of the ship's
//! pose on it (FUN_10055160, FUN_10183370): A(u) carried to the origin, the heading A -> B onto +Z.

use super::spline::{self, Seg};
use glam::{Mat4, Vec3};

pub struct Path {
    a: Vec<Seg>,
    b: Vec<Seg>,
    pub keys: usize,
    /// Spline A's chord total (FUN_10055130 runs no voyage on a zero one).
    pub chord: f32,
}

fn f32_at(d: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

impl Path {
    /// A `c00N` payload (after the chunk header): u32 @0x10 the key count in its low byte (bits 19 / 20 would add
    /// camera keys, clear in every voyage path), keys of 0x30 bytes from @0x20 (A @+0, B @+0x10). None below two keys.
    pub fn parse(d: &[u8]) -> Option<Self> {
        let n = (u32::from_le_bytes(d.get(0x10..0x14)?.try_into().ok()?) & 0xff) as usize;
        let point = |o: usize| Some(Vec3::new(f32_at(d, o)?, f32_at(d, o + 4)?, f32_at(d, o + 8)?));
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        for k in 0..n {
            let o = 0x20 + 0x30 * k;
            a.push(point(o)?);
            b.push(point(o + 0x10)?);
        }
        let chord = a.windows(2).map(|w| (w[1] - w[0]).length().max(0.01)).sum();
        Some(Path { a: spline::build(&a)?, b: spline::build(&b)?, keys: n, chord })
    }

    /// The ship's place on the path at `t` (reversed: 1 - t).
    #[cfg(test)]
    pub fn point(&self, t: f32, reverse: bool) -> Vec3 {
        spline::eval(&self.a, if reverse { 1.0 - t } else { t })
    }

    /// V at `t`: the pose RotY(pi/2 - phi) at A(u), phi the heading of B - A (A - B reversed), inverted.
    pub fn matrix(&self, t: f32, reverse: bool) -> Mat4 {
        let u = if reverse { 1.0 - t } else { t };
        let (a, b) = (spline::eval(&self.a, u), spline::eval(&self.b, u));
        let d = if reverse { a - b } else { b - a };
        let mut m = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2 - d.z.atan2(d.x));
        m.w_axis = a.extend(1.0);
        m.inverse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(file: u32, name: &[u8; 4]) -> Option<Path> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let d = index.read(file).ok()?;
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?).ok()?;
        let scene = crate::dat::zone::load_scene(&keys, &d).ok()?;
        let k = (name[3] - b'0') as usize;
        Path::parse(scene.paths[k].as_deref()?)
    }

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).abs().max_element() < eps
    }

    fn scene(file: u32) -> Option<crate::dat::zone::ZoneScene> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?).ok()?;
        crate::dat::zone::load_scene(&keys, &index.read(file).ok()?).ok()
    }

    /// Spec 6: the world MZB's placements carry the flag, the ship's do not; the grid is the ship's; a town has none.
    #[test]
    fn world_slots() {
        for (file, world, ship) in [(320, 1670, 203), (101, 2970, 82), (326, 2423, 108)] {
            let Some(s) = scene(file) else { return };
            let w = s.placements.iter().filter(|p| p.world).count();
            assert_eq!((w, s.placements.len() - w), (world, ship), "file {file}");
            assert!(s.collision.is_some(), "file {file}");
        }
        let Some(s) = scene(330) else { return };
        assert!(s.placements.iter().all(|p| !p.world) && s.paths.iter().all(|p| p.is_none()));
    }

    /// The route picks `c00N`, else `c000` (zone 220 has only that).
    #[test]
    fn route_fallback() {
        let Some(s) = scene(320) else { return };
        assert!(s.paths[0].is_some() && s.paths[3].is_none());
        let mut v = crate::client::voyage_for_tests();
        v.route = 3;
        v.set_path(&s.paths);
        assert_eq!(v.path.as_ref().map(|p| p.keys), Some(18));
    }

    /// Spec 6 sun_voyage: day fraction 0.25, eye (0, -2, -10), V of 320 c000 forward at t 0.5.
    #[test]
    fn sun_turns() {
        let Some(p) = path(320, b"c000") else { return };
        let eye = Vec3::new(0.0, -2.0, -10.0);
        let at = super::super::sky_point(true, 0.25, eye, p.matrix(0.5, false));
        assert!(close(at, Vec3::new(27.224, -2.0, -999.679), 0.01), "{at}");
        assert!(close(super::super::sky_point(true, 0.25, eye, Mat4::IDENTITY), Vec3::new(1000.0, -2.0, -10.0), 0.01));
    }

    /// Spec section 3: zone 1's four paths (keys, chord totals).
    #[test]
    fn paths_101() {
        let Some(p0) = path(101, b"c000") else { return };
        let want = [(57, 2530.348), (60, 2136.954), (41, 2202.018), (28, 1665.571)];
        let p = [Some(p0), path(101, b"c001"), path(101, b"c002"), path(101, b"c003")];
        for (k, (n, chord)) in want.into_iter().enumerate() {
            let p = p[k].as_ref().unwrap();
            assert_eq!(p.keys, n);
            assert!((p.chord - chord).abs() < 0.05, "c00{k} chord {}", p.chord);
        }
    }

    /// Spec section 3's poses of the Selbina ship (file 320 c000) and others.
    #[test]
    fn poses() {
        let Some(p) = path(320, b"c000") else { return };
        let check = |m: Mat4, x: Vec3, z: Vec3, w: Vec3| {
            assert!(close(m.x_axis.truncate(), x, 1e-4) && close(m.z_axis.truncate(), z, 1e-4), "{m}");
            assert!(close(m.w_axis.truncate(), w, 0.01), "{}", m.w_axis);
        };
        check(p.matrix(0.0, false), Vec3::new(-0.998915, 0.0, 0.04657), Vec3::new(-0.04657, 0.0, -0.998915), Vec3::new(3588.467, 0.0, 1092.05));
        check(p.matrix(0.25, false), Vec3::new(-0.406212, 0.0, -0.913779), Vec3::new(0.913779, 0.0, -0.406212), Vec3::new(1049.094, 0.0, 1802.976));
        check(p.matrix(0.5, false), Vec3::new(0.037217, 0.0, -0.999307), Vec3::new(0.999307, 0.0, 0.037217), Vec3::new(826.869, 0.0, -261.705));
        check(p.matrix(1.0, false), Vec3::new(0.868821, 0.0, 0.495127), Vec3::new(-0.495127, 0.0, 0.868821), Vec3::new(4054.643, 0.0, 296.392));
        check(p.matrix(0.5, true), Vec3::new(-0.037217, 0.0, 0.999307), Vec3::new(-0.999307, 0.0, -0.037217), Vec3::new(-826.869, 0.0, 261.705));
        // V carries the ship's place to the origin and its heading onto +Z.
        for t in [0.0, 0.13, 0.5, 0.77, 1.0] {
            for rev in [false, true] {
                let v = p.matrix(t, rev);
                assert!(close(v.transform_point3(p.point(t, rev)), Vec3::ZERO, 0.01));
            }
        }
        if let Some(p) = path(101, b"c002") {
            check(p.matrix(0.5, false), Vec3::new(0.87232, 0.0, -0.488935), Vec3::new(0.488935, 0.0, 0.87232), Vec3::new(-286.906, 0.0, 192.504));
        }
        if let Some(p) = path(147, b"c000") {
            check(p.matrix(0.5, true), Vec3::new(0.107841, 0.0, -0.994168), Vec3::new(0.994168, 0.0, 0.107841), Vec3::new(1179.334, 0.0, 475.422));
        }
        if let Some(p) = path(326, b"c000") {
            check(p.matrix(0.0, false), Vec3::new(-0.374498, 0.0, 0.927228), Vec3::new(-0.927228, 0.0, -0.374498), Vec3::new(1238.856, 60.0, 2637.019));
            check(p.matrix(0.5, false), Vec3::new(-0.651872, 0.0, 0.758329), Vec3::new(-0.758329, 0.0, -0.651872), Vec3::new(790.793, 75.305, 288.221));
        }
    }
}
