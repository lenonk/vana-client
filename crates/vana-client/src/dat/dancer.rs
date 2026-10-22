//! The "dancer" engine (Square's sq* middleware inside FFXiMain) that draws the character creator
//! preview: DMB model banks + DMP geometry, SQLE skeleton/skin/shape objects, motion channels.
//! Layouts follow sqmdDMBResolve FUN_1027d8a0 and the sq*BuildSQO readers named below.

use super::zone::Texture;
use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Quat, Vec3};

const BAD: u32 = 0xEFBE_DDBA; // file bytes "baddbeef": slot filled in at load
const SKEL_MARK: u32 = 0xEDFE_EDFE;

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).with_context(|| format!("read past end at {o:#x}"))
}
fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    Ok(u32_at(d, o)? as i32)
}
fn f32_at(d: &[u8], o: usize) -> Result<f32> {
    Ok(f32::from_bits(u32_at(d, o)?))
}
fn floats(d: &[u8], o: usize, n: usize) -> Result<Vec<f32>> {
    ensure!(o + n * 4 <= d.len(), "float array past end at {o:#x}");
    Ok(d[o..o + n * 4].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}
fn al16(x: usize) -> usize {
    (x + 15) & !15
}

/// Shader record kinds (sqmdDMBResolveShaders FUN_1027e240) and their base texture link.
#[derive(Clone, Debug)]
pub struct Shader {
    pub kind: u32,
    pub words: Vec<u32>,
}

impl Shader {
    /// sqsdGeneric 0xF0001 links textures at words 0x13.., sqsdSampling 0xF0002 at 0x14/0x15, sqsdAniso 0xF0005 at 0x18/0x19.
    pub fn base_texture(&self) -> Option<usize> {
        let w = match self.kind {
            0xF0001 => self.words[0x13],
            0xF0002 => self.words[0x14],
            0xF0005 => self.words[0x18],
            _ => return None,
        };
        (w != BAD).then_some(w as usize)
    }
    /// Inferred, not traced: hair (aniso) and generic shaders with word 0x29 set are cut-out.
    pub fn alpha_test(&self) -> bool {
        self.kind == 0xF0005 || (self.kind == 0xF0001 && self.words.get(0x29) == Some(&1))
    }
}

pub struct Shape {
    pub name: String,
    pub pos: Vec<Vec3>,
    pub normal: Vec<Vec3>,
    pub uv: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    pub shader: usize,
    /// (joint, vertex ids, weights) from the shape's skin updater.
    pub skin: Vec<(usize, Vec<u32>, Vec<f32>)>,
    /// Has a "_sort" updater (FUN_1027e8e0: updaterFlags >> 8 = shape): its listed-triangle runs are
    /// drawn back to front (FUN_10290b40 -> FUN_102a1e30).
    pub sorted: bool,
    /// Listed-triangle runs as (first triangle, count); strips are never sorted.
    pub list_runs: Vec<(usize, usize)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Bone {
    pub parent: i32,
    pub t: Vec3,
    pub q: Quat,
    pub s: Vec3,
    /// Animated channel counts: translation, rotation, scale (0 or 3/4/3).
    pub chans: [u8; 3],
}

pub struct Model {
    pub textures: Vec<Texture>,
    pub shaders: Vec<Shader>,
    /// Material (shader) names by index, from the tag table (type 1).
    pub materials: Vec<String>,
    pub shapes: Vec<Shape>,
    pub bones: Vec<Bone>,
    pub files: Vec<String>,
}

/// Reads a DMB (file `id`) and its DMP (`id + 1`).
pub fn load_model(dmb: &[u8], dmp: &[u8], tex_prefix: &str) -> Result<Model> {
    ensure!(dmb.get(..3) == Some(b"DMB"), "not a DMB file");
    let b = 0x10;
    let n = |o: usize| -> Result<usize> { Ok(i32_at(dmb, b + o)?.max(0) as usize) };
    let (ntex, nshd, nshd2, nupd, nupd2) = (n(0x24)?, n(0x34)?, n(0x38)?, n(0x44)?, n(0x48)?);
    let (nshp, nshp2, naux, naux2, ntag, ntag2) = (n(0x58)?, n(0x5c)?, n(0x68)?, n(0x6c)?, n(0x7c)?, n(0x80)?);
    let (nfile, nfile2, nscene, ntex2) = (n(0x90)?, n(0x94)?, n(0xa4)?, n(0x28)?);
    // Pointer arrays follow the 0xAC-byte model header, each sized by max(count, capacity).
    let mut p = al16(b + 0xac);
    let mut arr = |cnt: usize| -> usize {
        let at = p;
        p += cnt * 4;
        at
    };
    let t = ntex.max(ntex2);
    if t > 0 {
        arr(t);
        arr(t);
    }
    let s = nshd.max(nshd2);
    if s > 0 {
        arr(s);
        arr(s);
    }
    let u = nupd.max(nupd2);
    let (mut upd_flags_at, mut shader_idx_at, mut tag_types_at) = (0, 0, 0);
    if u > 0 {
        arr(u);
        upd_flags_at = arr(u);
    }
    let sh = nshp.max(nshp2);
    if sh > 0 {
        arr(sh);
        arr(sh);
        arr(sh);
    }
    let a = naux.max(naux2);
    if a > 0 {
        arr(a);
        arr(a);
    }
    let tg = ntag.max(ntag2);
    if tg > 0 {
        arr(tg);
        tag_types_at = arr(tg);
        arr(tg);
    }
    let f = nfile.max(nfile2);
    if f > 0 {
        arr(f);
        arr(f);
        arr(f);
    }
    if sh > 0 {
        shader_idx_at = arr(sh);
        arr(sh);
    }
    let sc = nscene.max(nshp2);
    if sc > 0 {
        arr(sc);
    }
    let matrices_at = if sh > 0 { al16(b + 0xac) + 4 * (2 * t + 2 * s + 2 * u) + 4 * sh } else { 0 };

    // Textures (FUN_1027dfc0): 0x24-byte head, optional 0x3C image head, inline data and CLUT.
    p = al16(p);
    let mut raw_tex = Vec::new();
    for _ in 0..ntex {
        let t0 = p;
        p = t0 + 0x24;
        let mut tex = None;
        if i32_at(dmb, t0 + 0x1c)? != -1 {
            let (w, h, bpp, dm) = (u32_at(dmb, t0 + 0x40)?, u32_at(dmb, t0 + 0x44)?, u32_at(dmb, t0 + 0x48)?, u32_at(dmb, t0 + 0x4c)?);
            p = t0 + 0x60;
            let mut data = None;
            if dm == BAD {
                p = al16(p);
                let sz = (w * h * bpp) as usize;
                data = Some(dmb.get(p..p + sz).context("texture data past end")?.to_vec());
                p += sz;
            }
            let (esz, cnt, cm) = (u32_at(dmb, t0 + 0x54)? as usize, u32_at(dmb, t0 + 0x58)? as usize, i32_at(dmb, t0 + 0x5c)?);
            let mut clut = None;
            if cm != -1 {
                p = al16(p);
                clut = Some((esz, dmb.get(p..p + esz * cnt).context("clut past end")?.to_vec()));
                p += esz * cnt;
            }
            tex = data.map(|d| (w, h, bpp, d, clut));
        }
        raw_tex.push(tex);
    }
    // Shaders: sizes in words per kind.
    p = al16(p);
    let mut shaders = Vec::new();
    for _ in 0..s {
        let kind = u32_at(dmb, p)?;
        let size = match kind {
            0xF0001 => 0x60,
            0xF0002 => 0x5b,
            0xF0005 => 0x1b,
            k => bail!("unsupported shader kind {k:#x}"),
        };
        let words = (0..size).map(|i| u32_at(dmb, p + 4 * i)).collect::<Result<Vec<_>>>()?;
        shaders.push(Shader { kind, words });
        p += size * 4;
    }
    // Matrices: one 0x40 slot per placeholder entry.
    p = al16(p);
    for i in 0..sh {
        if u32_at(dmb, matrices_at + 4 * i)? == BAD {
            p += 0x40;
        }
    }
    // Strings (FUN_1027e510): tags then files, NUL-terminated, 4-aligned.
    p = al16(p);
    let mut strs = |cnt: usize| -> Result<Vec<String>> {
        let mut out = Vec::new();
        for _ in 0..cnt {
            let e = dmb[p..].iter().position(|&c| c == 0).context("unterminated string")? + p;
            out.push(String::from_utf8_lossy(&dmb[p..e]).into_owned());
            p = (e + 1 + 3) & !3;
        }
        Ok(out)
    };
    let tags = strs(ntag)?;
    let files = strs(nfile)?;
    let tag_types: Vec<u32> = (0..ntag).map(|i| u32_at(dmb, tag_types_at + 4 * i)).collect::<Result<_>>()?;
    let named = |ty: u32| -> Vec<String> { tags.iter().zip(&tag_types).filter(|(_, t)| **t == ty).map(|(n, _)| n.clone()).collect() };
    let (tex_names, materials, shape_names) = (named(0), named(1), named(3));

    let mut textures = Vec::new();
    for (i, t) in raw_tex.into_iter().enumerate() {
        let name = format!("{tex_prefix}{}", tex_names.get(i).cloned().unwrap_or_else(|| i.to_string()));
        let Some((w, h, bpp, d, clut)) = t else {
            textures.push(Texture::new(name, 0, 1, 1, vec![255, 0, 255, 255]));
            continue;
        };
        // FUN_10276e00 / FUN_102772c0: bytes are R, G, B(, A); 1 byte = CLUT index or luminance.
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for px in d.chunks_exact(bpp as usize) {
            match (bpp, &clut) {
                (4, _) => rgba.extend_from_slice(px),
                (3, _) => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
                (1, Some((esz, c))) => {
                    let e = &c[px[0] as usize * esz..];
                    rgba.extend_from_slice(&[e[0], e[1], e[2], if *esz == 4 { e[3] } else { 255 }]);
                }
                // P8 with the grey palette (i, i, i, i) of FUN_10276e00; L8 (alpha 1) only where P8 is unsupported.
                (1, None) => rgba.extend_from_slice(&[px[0], px[0], px[0], px[0]]),
                _ => bail!("texture {name}: {bpp} bytes per pixel"),
            }
        }
        // format 1 = source has alpha (bpp 4 or a 4-byte CLUT), which is what enables blending (FUN_10286450).
        let alpha = bpp == 4 || bpp == 2 || matches!(&clut, Some((4, _)));
        textures.push(Texture::new(name, alpha as u8, w, h, rgba));
    }

    // DMP: shape blocks then updater blocks, each {u32 flags, u32 size, name..} + 0x40 head.
    let mut q = 0;
    let mut shape_objs = Vec::new();
    for _ in 0..nshp {
        let (flags, size) = (u32_at(dmp, q)?, u32_at(dmp, q + 4)? as usize);
        ensure!(flags & 4 != 0, "DMP block at {q:#x} is not a shape");
        shape_objs.push(&dmp[q + 0x40..q + 0x40 + size]);
        q = al16(q + 0x40 + size);
    }
    let mut bones = Vec::new();
    let mut skins: Vec<Vec<(usize, Vec<u32>, Vec<f32>)>> = (0..nshp).map(|_| Vec::new()).collect();
    let mut sorted = vec![false; nshp];
    for i in 0..nupd {
        let uf = u32_at(dmb, upd_flags_at + 4 * i)?;
        if uf != SKEL_MARK && uf & 0xBC00_0000 == 0 {
            if let Some(x) = sorted.get_mut(((uf >> 8) & 0xFFFF) as usize) {
                *x = true;
            }
            continue; // "_sort" updater built from a shape, no block
        }
        let size = u32_at(dmp, q + 4)? as usize;
        let obj = dmp.get(q + 0x40..q + 0x40 + size).context("updater past end")?;
        if uf == SKEL_MARK {
            ensure!(bones.is_empty(), "two skeletons");
            bones = parse_skeleton(obj)?;
        } else if let Some(sk) = skins.get_mut((uf & 0xFFFF) as usize) {
            *sk = parse_skin(obj)?;
        }
        q = al16(q + 0x40 + size);
    }
    let mut shapes = Vec::new();
    for (i, obj) in shape_objs.into_iter().enumerate() {
        let mut shape = parse_tristrip(obj)?;
        shape.name = shape_names.get(i).cloned().unwrap_or_default();
        shape.shader = u32_at(dmb, shader_idx_at + 4 * i)? as usize;
        shape.skin = std::mem::take(&mut skins[i]);
        shape.sorted = sorted[i];
        shapes.push(shape);
    }
    Ok(Model { textures, shaders, materials, shapes, bones, files })
}

fn sqle(obj: &[u8], kind: u32) -> Result<()> {
    ensure!(obj.get(..4) == Some(b"SQLE"), "not an SQLE object");
    let k = u32_at(obj, 8)?;
    ensure!(k == kind, "SQLE kind {k:#x}, expected {kind:#x}");
    Ok(())
}

/// sqshBuildSQO TriStrip 0xA0009 (FUN_1028f520): comment[0x50], vertex attributes, strip codes.
fn parse_tristrip(b: &[u8]) -> Result<Shape> {
    sqle(b, 0xA0009)?;
    let ver = u32_at(b, 12)?;
    ensure!(ver <= 2, "TriStrip version {ver}");
    let mut p = 0x60;
    let (nv, na) = (u32_at(b, p)? as usize, u32_at(b, p + 4)? as usize);
    p += 8;
    let (mut pos, mut normal, mut uv) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..na {
        let (id, nc) = (u32_at(b, p)?, u32_at(b, p + 4)? as usize);
        let f = floats(b, p + 8, nv * nc)?;
        p += 8 + nv * nc * 4;
        match (id, nc) {
            (0, 3) => pos = f.chunks_exact(3).map(Vec3::from_slice).collect(),
            (1, 3) => normal = f.chunks_exact(3).map(Vec3::from_slice).collect(),
            (2, 2) => uv = f.chunks_exact(2).map(|c| [c[0], c[1]]).collect(),
            _ => {}
        }
    }
    ensure!(pos.len() == nv, "shape without positions");
    normal.resize(nv, Vec3::ZERO);
    uv.resize(nv, [0.0; 2]);
    let nc = u32_at(b, p)? as usize;
    let codes: Vec<i32> = (0..nc).map(|i| i32_at(b, p + 4 + 4 * i)).collect::<Result<_>>()?;
    let (tris, list_runs) = strip_triangles(&codes)?;
    ensure!(tris.iter().flatten().all(|&i| (i as usize) < nv), "strip index out of range");
    Ok(Shape { name: String::new(), pos, normal, uv, tris, shader: 0, skin: Vec::new(), sorted: false, list_runs })
}

/// FUN_102a17b0: codes[0] = run count; run n > 0 is n/3 listed triangles, n <= 0 a strip of -n indices.
fn strip_triangles(codes: &[i32]) -> Result<(Vec<[u32; 3]>, Vec<(usize, usize)>)> {
    let mut out = Vec::new();
    let mut runs = Vec::new();
    let get = |i: usize| codes.get(i).copied().context("strip codes truncated");
    let mut p = 1;
    let (mut v3, mut v4, mut v5, mut v7) = (0, 0, 0, 0);
    for _ in 0..get(0)? {
        let mut n = get(p)?;
        p += 1;
        if n < 1 {
            n = -n;
            let mut flag = 0;
            while n != 0 {
                let step;
                if flag == 0 {
                    v5 = get(p)?;
                    v4 = get(p + 1)?;
                    flag = 1;
                    p += 2;
                    step = -3;
                } else {
                    v4 = if flag == 1 {
                        v5 = v7;
                        v3
                    } else {
                        v7
                    };
                    step = -1;
                    flag = -flag;
                }
                n += step;
                v7 = get(p)?;
                p += 1;
                out.push([v5 as u32, v4 as u32, v7 as u32]);
                v3 = v4;
            }
        } else {
            let first = out.len();
            while n > 0 {
                out.push([get(p)? as u32, get(p + 1)? as u32, get(p + 2)? as u32]);
                p += 3;
                n -= 3;
            }
            runs.push((first, out.len() - first));
        }
    }
    Ok((out, runs))
}

/// sqskBuildSQO 0xB0001 v4 (FUN_10290810): 0x40-byte bones {t, q xyzw, s, channel counts, 2 ints, parent}.
fn parse_skeleton(b: &[u8]) -> Result<Vec<Bone>> {
    sqle(b, 0xB0001)?;
    ensure!(u32_at(b, 12)? & !0x10000 == 4, "skeleton version");
    let n = u32_at(b, 0x60)? as usize;
    (0..n)
        .map(|i| {
            let p = 0x64 + i * 0x40;
            let f = floats(b, p, 10)?;
            let c = |k: usize| -> Result<u8> { Ok(u32_at(b, p + 40 + 4 * k)? as u8) };
            Ok(Bone {
                parent: if i == 0 { -1 } else { i32_at(b, p + 60)? },
                t: Vec3::new(f[0], f[1], f[2]),
                q: Quat::from_xyzw(f[3], f[4], f[5], f[6]),
                s: Vec3::new(f[7], f[8], f[9]),
                chans: [c(0)?, c(1)?, c(2)?],
            })
        })
        .collect()
}

/// sqinBuildSQO 0x150001 v2 (FUN_102904c0): clusters {joint, n, n vertex ids, n weights}.
fn parse_skin(b: &[u8]) -> Result<Vec<(usize, Vec<u32>, Vec<f32>)>> {
    sqle(b, 0x150001)?;
    let ncl = u32_at(b, 0x60)? as usize;
    let mut p = 0x68;
    let mut out = Vec::new();
    for _ in 0..ncl {
        let (j, n) = (u32_at(b, p)? as usize, u32_at(b, p + 4)? as usize);
        let ids = (0..n).map(|i| u32_at(b, p + 8 + 4 * i)).collect::<Result<_>>()?;
        let w = floats(b, p + 8 + 4 * n, n)?;
        p += 8 + 8 * n;
        out.push((j, ids, w));
    }
    Ok(out)
}

/// A motion channel set (sqmoChannelBuildSQO FUN_1027a930), decoded to frame-major floats.
pub struct Channels {
    pub time: f32,
    pub frames: usize,
    pub width: usize,
    /// Per float: 0 scalar, 1 first of a quaternion, 2 scale.
    pub types: Vec<i32>,
    pub data: Vec<f32>,
}

pub fn parse_channels(b: &[u8]) -> Result<Channels> {
    ensure!(b.get(..4) == Some(b"SQLE"), "not an SQLE object");
    let (kind, ver) = (u32_at(b, 8)?, u32_at(b, 12)?);
    let mut p = 0x60;
    match kind {
        // FrameChannel v4 (FUN_1027aab0): time, n, n types, frames, raw frames.
        0x50001 => {
            ensure!(ver == 4, "FrameChannel v{ver}");
            let (time, width) = (f32_at(b, p)?, u32_at(b, p + 4)? as usize);
            p += 8;
            let types = (0..width).map(|i| i32_at(b, p + 4 * i)).collect::<Result<_>>()?;
            p += 4 * width;
            let frames = u32_at(b, p)? as usize;
            let data = floats(b, p + 4, width * frames)?;
            Ok(Channels { time, frames, width, types, data })
        }
        // PlaybackChannel v3 (FUN_1027ace0): time, n, frames, step, packed flag, per-channel delta streams, types.
        0x50003 => {
            ensure!(ver == 3, "PBChannel v{ver}");
            let (time, width, frames, packed) = (f32_at(b, p)?, u32_at(b, p + 4)? as usize, u32_at(b, p + 8)? as usize, u32_at(b, p + 16)?);
            p += 20;
            let mut data = vec![0f32; width * frames];
            if packed == 0 {
                data = floats(b, p, width * frames)?;
                p += 4 * width * frames;
            } else {
                for c in 0..width {
                    let (len, bits, step, init) = (u32_at(b, p)? as usize, u32_at(b, p + 4)?, f32_at(b, p + 8)?, f32_at(b, p + 12)?);
                    let s = b.get(p + 16..p + 16 + len).context("PB stream past end")?;
                    p += 16 + len;
                    // FUN_102886a0: value(n) = init + sum of deltas 0..n-1 (FUN_10288db0 and siblings).
                    let mut v = init;
                    for f in 0..frames {
                        data[f * width + c] = v;
                        if f + 1 < frames {
                            v += delta(s, bits, f) as f32 * step;
                        }
                    }
                }
            }
            let types = (0..width).map(|i| i32_at(b, p + 4 * i)).collect::<Result<_>>()?;
            Ok(Channels { time, frames, width, types, data })
        }
        k => bail!("channel kind {k:#x} not supported"),
    }
}

/// Sign-magnitude deltas: 2 bits MSB-first, 4-bit nibbles high first, 8 bit, 16 bit big-endian.
fn delta(s: &[u8], bits: u32, i: usize) -> i32 {
    let at = |k: usize| s.get(k).copied().unwrap_or(0) as i32;
    let sm = |v: i32, sign: i32, mag: i32| if v & sign != 0 { -(v & mag) } else { v & mag };
    match bits {
        2 => sm(at(i >> 2) >> (6 - 2 * (i & 3)) & 3, 2, 1),
        4 => sm(if i & 1 == 1 { at(i >> 1) & 15 } else { at(i >> 1) >> 4 }, 8, 7),
        8 => sm(at(i), 0x80, 0x7f),
        16 => {
            let (hi, lo) = (at(2 * i), at(2 * i + 1));
            let m = ((hi & 0x7f) << 8) + lo;
            if hi & 0x80 != 0 { -m } else { m }
        }
        _ => 0,
    }
}

impl Channels {
    /// FUN_10272a00 + FUN_10272ac0: t in frames; scalars lerp, quaternions slerp.
    pub fn sample(&self, t: f32) -> Vec<f32> {
        let w = self.width;
        let row = |i: usize| &self.data[i * w..(i + 1) * w];
        if t <= 0.0 || self.frames < 2 {
            return row(0).to_vec();
        }
        let i = t as usize;
        if i >= self.frames - 1 {
            return row(self.frames - 1).to_vec();
        }
        let f = t - i as f32;
        let (a, b) = (row(i), row(i + 1));
        let mut out: Vec<f32> = a.iter().zip(b).map(|(x, y)| x + (y - x) * f).collect();
        let mut k = 0;
        while k < w {
            if self.types[k] == 1 && k + 4 <= w {
                let qa = Quat::from_slice(&a[k..k + 4]);
                let qb = Quat::from_slice(&b[k..k + 4]);
                let q = qa.normalize().slerp(qb.normalize(), f);
                out[k..k + 4].copy_from_slice(&q.to_array());
                k += 4;
            } else {
                k += 1;
            }
        }
        out
    }
}

fn joint_world(bones: &[Bone], local: impl Fn(usize, &Bone) -> Mat4) -> Vec<Mat4> {
    let mut w: Vec<Mat4> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let l = local(i, b);
        let m = if b.parent >= 0 && (b.parent as usize) < i { w[b.parent as usize] * l } else { l };
        w.push(m);
    }
    w
}

/// Joint world matrices for one channel frame: per joint, chans[0] translation, chans[1] quaternion, chans[2] scale floats.
pub fn pose(bones: &[Bone], frame: Option<&[f32]>) -> Vec<Mat4> {
    let mut locals = Vec::with_capacity(bones.len());
    let mut k = 0;
    for b in bones {
        let (mut t, mut q, mut s) = (b.t, b.q, b.s);
        if let Some(f) = frame {
            if b.chans[0] == 3 && k + 3 <= f.len() {
                t = Vec3::from_slice(&f[k..k + 3]);
                k += 3;
            }
            if b.chans[1] == 4 && k + 4 <= f.len() {
                q = Quat::from_slice(&f[k..k + 4]).normalize();
                k += 4;
            }
            if b.chans[2] == 3 && k + 3 <= f.len() {
                s = Vec3::from_slice(&f[k..k + 3]);
                k += 3;
            }
        }
        locals.push(Mat4::from_scale_rotation_translation(s, q, t));
    }
    joint_world(bones, |i, _| locals[i])
}

/// Per-vertex tangent (dP/du) and binormal (dP/dv), FUN_102a06d0: per-corner sums over the
/// triangles, then made orthogonal to the normal (component by component, as the code does) and normalized.
pub fn tangents(s: &Shape) -> (Vec<Vec3>, Vec<Vec3>) {
    let n = s.pos.len();
    let (mut t, mut b) = (vec![Vec3::ZERO; n], vec![Vec3::ZERO; n]);
    let inv = |d: f32| 1.0 / if d < 1e-6 && d > -1e-6 { 1e-6 } else { d };
    for tri in &s.tris {
        let (a, bb, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let uv = |i: usize| glam::Vec2::from(s.uv[i]);
        let (e_ab, e_bc, e_ca) = (s.pos[a] - s.pos[bb], s.pos[bb] - s.pos[c], s.pos[c] - s.pos[a]);
        let (t_ab, t_bc, t_ca) = (uv(a) - uv(bb), uv(bb) - uv(c), uv(c) - uv(a));
        let r = inv(t_ab.y * t_ca.x - t_ca.y * t_ab.x);
        t[a] += e_ab * -(t_ca.y * r) - e_ca * -(t_ab.y * r);
        b[a] += e_ab * (t_ca.x * r) - e_ca * (t_ab.x * r);
        let r = inv(t_bc.y * t_ab.x - t_bc.x * t_ab.y);
        t[bb] += e_bc * -(t_ab.y * r) - e_ab * -(t_bc.y * r);
        b[bb] += e_bc * (t_ab.x * r) - e_ab * (t_bc.x * r);
        let r = inv(t_bc.x * t_ca.y - t_bc.y * t_ca.x);
        t[c] += e_ca * -(t_bc.y * r) - e_bc * -(t_ca.y * r);
        b[c] += e_ca * (t_bc.x * r) - e_bc * (t_ca.x * r);
    }
    let ortho = |v: &mut Vec3, nn: Vec3| {
        v.x -= (nn.x * v.x + v.y * nn.y + v.z * nn.z) * nn.x;
        v.y -= (nn.x * v.x + v.y * nn.y + v.z * nn.z) * nn.y;
        v.z -= (nn.x * v.x + v.y * nn.y + v.z * nn.z) * nn.z;
        *v /= v.length().max(1e-12);
    };
    for i in 0..n {
        ortho(&mut t[i], s.normal[i]);
        ortho(&mut b[i], s.normal[i]);
    }
    (t, b)
}

/// One shape after skinning (model space).
pub struct Skinned {
    pub pos: Vec<Vec3>,
    pub normal: Vec<Vec3>,
    pub tangent: Vec<Vec3>,
    pub binormal: Vec<Vec3>,
}

/// What skinning needs that does not change with the pose: inverse bind matrices, bind-pose tangents.
pub struct SkinPrep {
    pub bind_inv: Vec<Mat4>,
    pub tangents: Vec<(Vec<Vec3>, Vec<Vec3>)>,
}

/// The skinning matrices of a frame: current joint world x inverse bind, per joint.
pub fn joint_mats(m: &Model, prep: &SkinPrep, frame: Option<&[f32]>) -> Vec<Mat4> {
    pose(&m.bones, frame).iter().zip(&prep.bind_inv).map(|(c, b)| *c * *b).collect()
}

/// One shape's skinned positions from `joint_mats` (as skin_shapes_with, positions only).
pub fn skin_positions(s: &Shape, mats: &[Mat4]) -> Vec<Vec3> {
    if s.skin.is_empty() {
        return s.pos.clone();
    }
    let mut p = vec![Vec3::ZERO; s.pos.len()];
    for (j, ids, w) in &s.skin {
        let Some(mat) = mats.get(*j) else { continue };
        for (&v, &wt) in ids.iter().zip(w) {
            if let Some(o) = p.get_mut(v as usize) {
                *o += mat.transform_point3(s.pos[v as usize]) * wt;
            }
        }
    }
    p
}

pub fn skin_prep(m: &Model) -> SkinPrep {
    SkinPrep { bind_inv: pose(&m.bones, None).iter().map(|x| x.inverse()).collect(), tangents: m.shapes.iter().map(tangents).collect() }
}

/// Skins every shape; bind-pose tangents are skinned like normals (inferred).
pub fn skin_shapes(m: &Model, frame: Option<&[f32]>) -> Vec<Skinned> {
    skin_shapes_with(m, &skin_prep(m), frame)
}

pub fn skin_shapes_with(m: &Model, prep: &SkinPrep, frame: Option<&[f32]>) -> Vec<Skinned> {
    let bind = &prep.bind_inv;
    let cur = pose(&m.bones, frame);
    m.shapes
        .iter()
        .zip(&prep.tangents)
        .map(|(s, (t0, b0))| {
            let n = s.pos.len();
            if s.skin.is_empty() {
                return Skinned { pos: s.pos.clone(), normal: s.normal.clone(), tangent: t0.clone(), binormal: b0.clone() };
            }
            let z = || vec![Vec3::ZERO; n];
            let mut o = Skinned { pos: z(), normal: z(), tangent: z(), binormal: z() };
            for (j, ids, w) in &s.skin {
                let Some(mat) = cur.get(*j).map(|c| *c * bind[*j]) else { continue };
                for (&v, &wt) in ids.iter().zip(w) {
                    let v = v as usize;
                    if v < n {
                        o.pos[v] += mat.transform_point3(s.pos[v]) * wt;
                        o.normal[v] += mat.transform_vector3(s.normal[v]) * wt;
                        o.tangent[v] += mat.transform_vector3(t0[v]) * wt;
                        o.binormal[v] += mat.transform_vector3(b0[v]) * wt;
                    }
                }
            }
            for v in o.normal.iter_mut().chain(o.tangent.iter_mut()).chain(o.binormal.iter_mut()) {
                *v = v.normalize_or_zero();
            }
            o
        })
        .collect()
}

/// Skinned triangles grouped by texture name, in the character renderer's batch format.
pub fn skin_model(m: &Model, frame: Option<&[f32]>, out: &mut crate::render::character::PosedMesh) {
    let bind: Vec<Mat4> = pose(&m.bones, None).iter().map(|x| x.inverse()).collect();
    let cur = pose(&m.bones, frame);
    for s in &m.shapes {
        let (mut p, mut n) = (vec![Vec3::ZERO; s.pos.len()], vec![Vec3::ZERO; s.pos.len()]);
        if s.skin.is_empty() {
            p.clone_from(&s.pos);
            n.clone_from(&s.normal);
        }
        for (j, ids, w) in &s.skin {
            let Some(mat) = cur.get(*j).map(|c| *c * bind[*j]) else { continue };
            for (&v, &wt) in ids.iter().zip(w) {
                let v = v as usize;
                if v < p.len() {
                    p[v] += mat.transform_point3(s.pos[v]) * wt;
                    n[v] += mat.transform_vector3(s.normal[v]) * wt;
                }
            }
        }
        let Some(shader) = m.shaders.get(s.shader) else { continue };
        let tex = shader.base_texture().and_then(|t| m.textures.get(t)).map(|t| t.name.clone()).unwrap_or_default();
        let mut tris = Vec::with_capacity(s.tris.len() * 3);
        for t in &s.tris {
            for &i in t {
                let (q, nn, uv) = (p[i as usize], n[i as usize], s.uv[i as usize]);
                out.min = out.min.min(q);
                out.max = out.max.max(q);
                tris.push([q.x, q.y, q.z, nn.x, nn.y, nn.z, uv[0], uv[1]]);
            }
        }
        let key = if shader.alpha_test() { tex } else { format!("{tex}#opaque") };
        out.batches.push((key, tris));
    }
}

/// Creator preview files (FUN_10242a70, FUN_10242d90, FUN_10242e30; tables in FFXiMain .data).
pub mod creator {
    /// 0x1037ce80: model group per creator race (HumeM HumeF ElvM ElvF TaruM TaruF Mithra Galka), 20 files each:
    /// +0/+1 low DMB/DMP (flag-0x20 path), +2/+3 body, +4 + 2*face / +5 + 2*face head (8 faces).
    pub const MODEL_GROUP: [u32; 8] = [31184, 31164, 31124, 31104, 31244, 31224, 31204, 31144];
    /// 0x1037cea0: motion group per race, 58 files: +0 FOV / +1 camera matrix (camera 0), +2/+3 camera 1,
    /// +4..+7 FrameChannel loops, +8 body performance, +9 body idle loop (view mode 2),
    /// +0xa + 6*face .. 4 face FrameChannels, +0xe + 6*face face performance, +0xf + 6*face face idle.
    pub const MOTION_GROUP: [u32; 8] = [31496, 31438, 31322, 31264, 31670, 31612, 31554, 31380];
    /// 0x1037cc80[race * 8 + face]: body model scale (FUN_102460b0 -> FUN_10245500: the body skeleton's root
    /// node is scaled, its channel translation by the inverse, so the body scales about its root). Size S/M/L
    /// does not enter it; the head is not scaled.
    pub const BODY_SCALE: [[f32; 8]; 8] = [
        [1.0, 1.0, 1.0, 0.96, 1.04, 1.04, 1.04, 1.0],
        [0.96, 1.0, 1.0, 0.96, 1.0, 1.0, 1.0, 0.96],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.96, 1.0],
        [0.96, 0.96, 1.0, 0.96, 0.96, 1.0, 1.0, 1.0],
        [1.0; 8],
        [1.0; 8],
        [1.0; 8],
        [1.0; 8],
    ];
    /// 0x1037cec0: BGM per race, restarted when the performance loops (FUN_10245920).
    pub const BGM: [u8; 8] = [0x85, 0xa8, 0x84, 0x83, 0xa7, 0x82, 0xa6, 0xa5];
    /// Directional lights 0..3 (structs 0x1037cc54, nodes 0x1037cc68): node quaternion xyzw and colour
    /// (struct colour x1.5, FUN_10245bc0). Attached to the camera (FUN_10268040, FUN_10266820).
    pub const LIGHTS: [([f32; 4], f32); 4] = [
        ([0.35657, -0.26401, 0.59479, -0.66809], 0.9),
        ([0.51537, 0.23858, -0.69035, -0.44889], 0.45),
        ([-0.18707, 0.15538, -0.66265, -0.70649], 0.15),
        ([-0.00177, 0.00152, 0.85515, 0.51748], 0.45),
    ];
}
