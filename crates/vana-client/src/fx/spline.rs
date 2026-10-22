//! Effect paths (splines.md): a Vtx point list (chunk 0x3e) as the CMoSpline FUN_10062600 builds over it, a
//! cubic Hermite curve with Bessel tangents and chord-length knots, open, N >= 2.

use glam::Vec3;

/// One segment: start point and tangent, end point and tangent (in segment t units), knot u_k and c_k / C.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub p0: Vec3,
    pub m0: Vec3,
    pub p1: Vec3,
    pub m1: Vec3,
    pub u0: f32,
    pub len: f32,
}

/// FUN_10062bd0 / FUN_10062600 over the points; None below two points (N = 1 reads garbage in retail).
pub fn build(q: &[Vec3]) -> Option<Vec<Seg>> {
    let n = q.len();
    if n < 2 {
        return None;
    }
    // Chords c_k = max(|Q_k+1 - Q_k|, 0.01), with c_-1 = c_0 and c_N-1 = c_N-2; reflected phantom ends.
    let c: Vec<f32> = (0..n - 1).map(|k| (q[k + 1] - q[k]).length().max(0.01)).collect();
    let total: f32 = c.iter().sum();
    let cq = |k: isize| c[k.clamp(0, n as isize - 2) as usize];
    let pq = |k: isize| match k {
        -1 => 2.0 * q[0] - q[1],
        k if k == n as isize => 2.0 * q[n - 1] - q[n - 2],
        k => q[k as usize],
    };
    let mut u = 0.0;
    let mut segs = Vec::with_capacity(n - 1);
    for k in 0..n - 1 {
        let ki = k as isize;
        let a = cq(ki - 1) / (cq(ki - 1) + c[k]);
        let b = c[k] / (c[k] + cq(ki + 1));
        let d0 = pq(ki) - pq(ki - 1);
        let d1 = pq(ki + 1) - pq(ki);
        let d2 = pq(ki + 2) - pq(ki + 1);
        segs.push(Seg {
            p0: q[k],
            m0: (1.0 - a) * (1.0 - a) / a * d0 + a * d1,
            p1: q[k + 1],
            m1: (1.0 - b) * d1 + b * b / (1.0 - b) * d2,
            u0: u / total,
            len: c[k] / total,
        });
        u += c[k];
    }
    Some(segs)
}

/// FUN_100628e0 per axis: the first segment with u <= its accumulated end, else the last at t = 1.
pub fn eval(segs: &[Seg], u: f32) -> Vec3 {
    let mut acc = 0.0;
    let mut pick = None;
    for (j, s) in segs.iter().enumerate() {
        acc += s.len;
        if u <= acc {
            pick = Some((j, (u - (acc - s.len)) / s.len));
            break;
        }
    }
    let (j, t) = pick.unwrap_or((segs.len() - 1, 1.0));
    let s = &segs[j];
    let (t2, t3) = (t * t, t * t * t);
    s.p0 * (2.0 * t3 - 3.0 * t2 + 1.0) + s.m0 * (t3 - 2.0 * t2 + t) + s.p1 * (-2.0 * t3 + 3.0 * t2) + s.m1 * (t3 - t2)
}

/// The GPU form: a header (segment count) then four vec4s a segment (p0, u0), (m0, len), (p1, 0), (m1, 0).
pub fn gpu_words(segs: &[Seg]) -> Vec<[f32; 4]> {
    let mut out = vec![[f32::from_bits(segs.len() as u32), 0.0, 0.0, 0.0]];
    for s in segs {
        out.push([s.p0.x, s.p0.y, s.p0.z, s.u0]);
        out.push([s.m0.x, s.m0.y, s.m0.z, s.len]);
        out.push([s.p1.x, s.p1.y, s.p1.z, 0.0]);
        out.push([s.m1.x, s.m1.y, s.m1.z, 0.0]);
    }
    out
}

/// Vtx data: u32 count, then vec4 points from +0x10 (w = 1 in every file).
pub fn points(d: &[u8]) -> Vec<Vec3> {
    let n = d.get(0..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize;
    let f = |o: usize| d.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
    (0..n).take_while(|i| 0x10 + i * 16 + 12 <= d.len()).map(|i| Vec3::new(f(0x10 + i * 16), f(0x14 + i * 16), f(0x18 + i * 16))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 2e-3
    }

    fn vtx(file: u32, name: &[u8; 4]) -> Option<Vec<Vec3>> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let lib = crate::fx::lib::Library::load(file, index.read(file).ok()?).ok()?;
        let n = lib.find_global(crate::dat::generator::kind::VTX, u32::from_le_bytes(*name))?;
        Some(points(lib.data(n)))
    }

    /// splines.md 6.1 / 6.2: DAT 202 'lin2' (12 points) and DAT 265 'a7  ' (3 points).
    #[test]
    fn worked_paths() {
        let Some(q) = vtx(202, b"lin2") else { return };
        let s = build(&q).unwrap();
        for (u, p) in [(0.0, Vec3::new(122.9643, -1.5996, 446.3557)), (0.25, Vec3::new(137.8889, -6.2966, 313.4352)), (0.5, Vec3::new(70.5023, -4.3057, 178.0302)), (0.75, Vec3::new(-27.9632, -9.8760, 111.9538)), (1.0, Vec3::new(-127.1780, -18.9483, 26.7836))] {
            assert!(close(eval(&s, u), p), "lin2 u {u}: {}", eval(&s, u));
        }
        let Some(q) = vtx(265, b"a7  ") else { return };
        let s = build(&q).unwrap();
        for (u, p) in [(0.25, Vec3::new(-492.9381, -170.8691, -233.2339)), (0.5, Vec3::new(-494.5517, -170.8691, -233.7639)), (1.0, Vec3::new(-497.9810, -170.8691, -233.8575))] {
            assert!(close(eval(&s, u), p), "a7 u {u}: {}", eval(&s, u));
        }
    }
}

