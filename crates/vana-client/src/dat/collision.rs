//! Zone collision mesh, stored in the decoded MZB after the placement records (loader FUN_10161130).
//! Table at u32 @8: geometry count / offset, cell-list count / offset, grid, object records / count.
//! A grid of cells lists (object, geometry) pairs; an object is a 0xc0-byte record (world matrix,
//! inverse, normal matrix, flags @0xa4, y range @0xb4); geometry is vertices, normals and triangles
//! of four u16 (v0 & 0x7fff, v1 & 0x3fff, v2 & 0x3fff, normal & 0x7fff; bit 15 of each = surface type).

use anyhow::{Result, bail};
use glam::{Mat3, Mat4, Vec3};
use std::collections::HashMap;

pub struct Collision {
    /// World vertices in retail order (v1 / v2 swapped for mirrored objects).
    pub tris: Vec<[Vec3; 3]>,
    /// Geometry flags per triangle; bit 0: does not block line of sight.
    pub flags: Vec<u16>,
    /// Stored face normal in world space (y < 0 faces up).
    pub normals: Vec<Vec3>,
    /// Surface type: bit 15 of v0 | v1 << 1 | v2 << 2 | normal << 3 (FUN_10162590 +0x34148).
    pub kinds: Vec<u8>,
    /// Index into `objects`.
    pub object: Vec<u32>,
    pub objects: Vec<Object>,
    /// Each distinct (object, geometry) pair: its run of triangles.
    pub entries: Vec<Entry>,
    /// Per grid cell (row-major, 4 units square, centred on the origin): entries in list order.
    pub cells: Vec<Vec<u32>>,
    pub grid: (usize, usize),
}

pub struct Entry {
    pub first: u32,
    pub count: u32,
    pub object: u32,
}

/// The object record a triangle is placed by.
pub struct Object {
    /// +0xa4: bit 0 mirrored, bit 1 resolved after the others in the sweep, bit 2 covered (no weather under it,
    /// FUN_100136a0).
    pub flags: u32,
    /// +0xb4 / +0xb8: y range, checked before any of its triangles.
    pub y_range: (f32, f32),
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}
fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    u32_at(b, o).map(f32::from_bits)
}
fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap()))
}

pub fn parse(p: &[u8]) -> Result<Collision> {
    let mut at = 8;
    let table = loop {
        let o = u32_at(p, at).unwrap_or(0) as usize;
        if o != 0 || at >= 0x20 {
            break o;
        }
        at += 4;
    };
    if table == 0 || table >= p.len() {
        bail!("MZB has no collision table");
    }
    let grid_at = u32_at(p, table + 0x10).unwrap_or(0) as usize;
    if grid_at == 0 || grid_at >= p.len() {
        bail!("MZB collision has no grid");
    }
    // Each cell's entry list starts with its own coordinates, which pins down the grid width.
    let (w, h) = (p[0x0c] as usize * 10, p[0x0d] as usize * 10);
    let mut objects: HashMap<usize, u32> = HashMap::new();
    let mut seen: HashMap<(usize, usize), u32> = HashMap::new();
    let mut out = Collision { tris: Vec::new(), flags: Vec::new(), normals: Vec::new(), kinds: Vec::new(), object: Vec::new(), objects: Vec::new(), entries: Vec::new(), cells: vec![Vec::new(); w * h], grid: (w, h) };
    for y in 0..h {
        for x in 0..w {
            let Some(entry) = u32_at(p, grid_at + (y * w + x) * 4).map(|e| e as usize) else { continue };
            if entry == 0 || entry >= p.len() {
                continue;
            }
            let head = u32_at(p, entry).unwrap_or(0);
            if ((head >> 14) & 0x1FF) as usize != x || ((head >> 23) & 0x1FF) as usize != y {
                bail!("collision cell ({x},{y}) claims to be ({},{})", (head >> 14) & 0x1FF, (head >> 23) & 0x1FF);
            }
            // Low 11 bits: pair count; 0x2000: list switched off (FUN_10162590 skips it).
            if head & 0x2000 != 0 {
                continue;
            }
            for k in 0..(head & 0x7FF) as usize {
                let o = entry + 4 + k * 8;
                let (Some(xf), Some(geo)) = (u32_at(p, o), u32_at(p, o + 4)) else { break };
                let (xf, geo) = (xf as usize, geo as usize);
                if xf == 0 || geo == 0 || xf + 0xc0 > p.len() || geo >= p.len() {
                    continue;
                }
                let id = match seen.get(&(xf, geo)) {
                    Some(&id) => id,
                    None => {
                        let next = objects.len() as u32;
                        let obj = *objects.entry(xf).or_insert_with(|| {
                            out.objects.push(Object { flags: u32_at(p, xf + 0xa4).unwrap_or(0), y_range: (f32_at(p, xf + 0xb4).unwrap_or(f32::MIN), f32_at(p, xf + 0xb8).unwrap_or(f32::MAX)) });
                            next
                        });
                        let first = out.tris.len() as u32;
                        add_geometry(p, xf, geo, obj, &mut out);
                        let id = out.entries.len() as u32;
                        out.entries.push(Entry { first, count: out.tris.len() as u32 - first, object: obj });
                        seen.insert((xf, geo), id);
                        id
                    }
                };
                out.cells[y * w + x].push(id);
            }
        }
    }
    Ok(out)
}

fn add_geometry(p: &[u8], xf: usize, geo: usize, obj: u32, out: &mut Collision) {
    let m: [f32; 41] = std::array::from_fn(|i| f32_at(p, xf + i * 4).unwrap_or(0.0));
    // Row-vector convention (translation in 12..15), so read it as columns.
    let mat = Mat4::from_cols_array(m[..16].try_into().unwrap());
    let nmat = Mat3::from_cols_array(m[32..41].try_into().unwrap());
    let mirrored = out.objects[obj as usize].flags & 1 != 0;
    let (Some(verts), Some(normals), Some(tris), Some(count), Some(flags)) =
        (u32_at(p, geo), u32_at(p, geo + 4), u32_at(p, geo + 8), u16_at(p, geo + 12), u16_at(p, geo + 14))
    else {
        return;
    };
    let vec_at = |o: usize| Some(Vec3::new(f32_at(p, o)?, f32_at(p, o + 4)?, f32_at(p, o + 8)?));
    let nverts = (normals.saturating_sub(verts) / 12) as usize;
    let world: Vec<Vec3> = (0..nverts).filter_map(|i| vec_at(verts as usize + i * 12).map(|v| mat.transform_point3(v))).collect();
    for t in 0..count as usize {
        let o = tris as usize + t * 8;
        let Some(w) = (0..4).map(|k| u16_at(p, o + k * 2)).collect::<Option<Vec<u16>>>() else { break };
        let idx = [(w[0] & 0x7FFF) as usize, (w[1] & 0x3FFF) as usize, (w[2] & 0x3FFF) as usize];
        let (Some(&a), Some(&b), Some(&c)) = (world.get(idx[0]), world.get(idx[1]), world.get(idx[2])) else { continue };
        let Some(n) = vec_at(normals as usize + (w[3] & 0x7FFF) as usize * 12) else { continue };
        // The loader turns -NaN components into 0 and a zero normal into (0, 1e-6, 0).
        let n = Vec3::from_array(n.to_array().map(|v| if v.is_nan() { 0.0 } else { v }));
        let n = if n == Vec3::ZERO { Vec3::new(0.0, 1e-6, 0.0) } else { n };
        // FUN_101671b0: a mirrored object's triangles run v0, v2, v1.
        out.tris.push(if mirrored { [a, c, b] } else { [a, b, c] });
        out.normals.push(nmat * n);
        out.kinds.push(((w[0] >> 15) | (w[1] >> 15) << 1 | (w[2] >> 15) << 2 | (w[3] >> 15) << 3) as u8);
        out.flags.push(flags);
        out.object.push(obj);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(zone: u32) -> Option<Collision> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let keys = crate::dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?).ok()?;
        crate::dat::zone::load_scene(&keys, &index.read(100 + zone).ok()?).ok()?.collision
    }

    /// What the sweep relies on: unit normals, faces wound against them (FUN_101688a0), and cells covering
    /// x 4i - 2w .. 4i - 2w + 4 (FUN_10167b10: cell = (x + half width) / 4).
    #[test]
    fn east_ronfaure_mesh() {
        let Some(c) = load(101) else { return };
        assert!(c.tris.len() > 300_000);
        for (t, n) in c.tris.iter().zip(&c.normals) {
            assert!((n.length() - 1.0).abs() < 1e-3);
            assert!((t[1] - t[0]).cross(t[2] - t[0]).dot(*n) < 0.0);
        }
        let (w, h) = c.grid;
        let bounds: Vec<(Vec3, Vec3)> = c
            .entries
            .iter()
            .map(|e| c.tris[e.first as usize..(e.first + e.count) as usize].iter().flatten().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v))))
            .collect();
        let meets = |flip: bool| {
            let (mut inside, mut total) = (0, 0);
            for (i, list) in c.cells.iter().enumerate() {
                let row = if flip { h - 1 - i / w } else { i / w };
                let (cx, cz) = ((i % w) as f32 * 4.0 - w as f32 * 2.0, row as f32 * 4.0 - h as f32 * 2.0);
                for &e in list {
                    let (lo, hi) = bounds[e as usize];
                    total += 1;
                    if lo.x <= cx + 4.01 && hi.x >= cx - 0.01 && lo.z <= cz + 4.01 && hi.z >= cz - 0.01 {
                        inside += 1;
                    }
                }
            }
            (inside, total)
        };
        let (inside, total) = meets(false);
        assert!(inside as f64 > total as f64 * 0.9 && (meets(true).0 as f64) < total as f64 * 0.1, "{inside} of {total} entries meet their cell");
    }
}
