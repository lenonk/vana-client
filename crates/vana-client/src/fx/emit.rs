//! The CPU side of a generator: its per-frame update stream (FUN_10048740, generators.md section 4) and the
//! emission stream that builds each new element (FUN_1004d920, section B). The element's per-frame stream
//! runs on the GPU.

use super::elem::{Elem, NONE, dk, st, w};
use super::math::{Rng, euler, then, wrap};
use super::{Draw, ElemCpu, Frame, Instance, Ref, Registry};
use crate::dat::generator::{Op, kind};
use glam::{Mat4, Vec3};
use std::f32::consts::PI;

/// What an emission asks of the effect manager after the element exists.
pub enum Child {
    /// Ops 0x44 / 0x53 / 0x6a: clone generator `src`, linked to the new element (+0x164); `orient` = 0x53 / 0x6a
    /// (the clone's matrix follows the element's rotation and scale), `billboard` = 0x6a.
    Clone { src: Ref, op: u8 },
    /// Op 0x3c: run generator `src` once now with the new element as parent.
    Burst { src: Ref },
}

/// The parent element of a child emission (FUN_1004d920's `parent`).
#[derive(Clone, Copy, Debug)]
pub struct Parent {
    pub pos: Vec3,
    pub world: Vec3,
    pub rot: Vec3,
    pub scale: Vec3,
    pub color: u32,
    pub life: f32,
    pub max_life: f32,
    pub uv: [f32; 2],
    pub bias: f32,
}

pub struct Spawn {
    pub elem: Elem,
    pub cpu: ElemCpu,
    pub children: Vec<Child>,
    /// Batched D3a (CMoVuAnmElem): the particle records (pos0, dir, vel) of FUN_10063060.
    pub parts: Vec<[f32; 9]>,
    /// A sound element (kind 0x3d, CYySoundElem).
    pub sound: Option<super::sound::Sound>,
}

/// Resource kind after FUN_1004d920's remap (table 0x10051748): Vum / Om1 -> D3m, Anm -> D3a, Mmd -> D3b.
fn remap(k: u8) -> u8 {
    match k {
        0x0b | 0x0c => kind::D3M,
        0x0e => kind::D3A,
        0x1d => 0x25,
        k => k,
    }
}

/// Kinds that name no resource (FUN_100485c0: handle = 1).
fn needs_no_resource(k: u8) -> bool {
    matches!(k, 0x57 | 0x47 | 0x22 | 0x24 | 0x5c)
}

/// FUN_1004d920: builds one element from the instance's emission stream, or None when op 1's resource does not
/// resolve (retail returns without emitting).
pub fn emit(reg: &mut Registry, inst: &mut Instance, parent: Option<&Parent>, f: &Frame, rng: &mut Rng) -> Option<Spawn> {
    let lib = inst.src.0;
    let from = inst.src.1;
    let stream = inst.streams[1].clone();
    let op1 = stream.first().filter(|o| o.code == 0x01)?;
    let mut e = Elem::default();
    let mut children = Vec::new();
    let flags = op1.u32(1);
    let res_kind = op1.byte(0x21);
    let name = op1.u32(3);
    // Link step FUN_10047270 / FUN_100485c0: D3m kinds fall back to an MMB of the same name.
    let draw = if needs_no_resource(res_kind) {
        Draw::None
    } else {
        match remap(res_kind) {
            kind::D3M => match reg.resolve(lib, from, kind::D3M, name).or_else(|| reg.resolve(lib, from, kind::MMB, name)) {
                Some(r) => reg.mesh(r).map(Draw::Mesh).unwrap_or(Draw::None),
                None => {
                    reg.miss(lib, from, name, res_kind);
                    return None;
                }
            },
            kind::D3A | 0x39 => match reg.resolve(lib, from, kind::D3A, name) {
                Some(r) => reg.sprite(r).map(Draw::Sprite).unwrap_or(Draw::None),
                None => {
                    reg.miss(lib, from, name, res_kind);
                    return None;
                }
            },
            0x25 => match reg.resolve(lib, from, 0x25, name) {
                Some(r) => reg.morph(r).map(Draw::Morph).unwrap_or(Draw::None),
                None => {
                    reg.miss(lib, from, name, res_kind);
                    return None;
                }
            },
            _ => Draw::None,
        }
    };
    // Kind 0x3d: a SeSep; no resource means no element (FUN_1004d920 returns at op 1).
    let mut sound = None;
    if res_kind == 0x3d {
        let r = reg.resolve(lib, from, 0x3d, name)?;
        let (se, loops) = super::sound::sesep(reg.libs[r.0].data(r.1))?;
        let mut s = super::sound::Sound::new(se, loops);
        // Op 0x4c on a world generator sets height mode 1; without 0x4c the defaults stand.
        if inst.mode == 0 {
            s.ymode = 0;
        }
        sound = Some(s);
    }
    e.0[w::FLAGS] = flags;
    let mut pos = Vec3::new(op1.f32(5), op1.f32(6), op1.f32(7));
    // Step 4: flag 0x800000 snapshots the emitter frame at spawn.
    let snapshot = flags & 0x80_0000 != 0 && (inst.matrix.is_some() || parent.is_some());
    // Step 8: flag 0x4000000 = position in camera space (z negated, through the inverse view).
    if flags & 0x400_0000 != 0 {
        pos.z = -pos.z;
        pos = f.view.inverse().transform_point3(pos);
    }
    let mut pos0 = pos;
    let mut life = op1.i16_at(0x22) as f32;
    let extra = op1.i16_at(0x24);
    if extra != 0 {
        life += rng.un(extra as u16 as u32) as f32;
    }
    e.set(w::LIFE, life);
    e.set(w::MAX_LIFE, life);
    e.0[w::BLEND] = 0x48;
    e.set(w::ALPHA, 1.0);
    e.set(w::FADE, 1.0);
    e.set_v3(w::SCALE, Vec3::ZERO);
    e.0[w::STATE] = st::ALIVE | if inst.flags & 0x2000_0000 != 0 { st::BATCHED } else { 0 } | if matches!(draw, Draw::None) { 0 } else { st::HAS_RES };
    inst.emitted += 1;
    let batched = inst.flags & 0x2000_0000 != 0;
    e.0[w::GEN_BITS] = (inst.de as u32) << 8;
    e.0[w::KIND] = match draw {
        Draw::None => dk::NONE,
        Draw::Mesh(_) => dk::MESH,
        Draw::Morph(id) => {
            e.0[w::FRAMES] = reg.morph_data_of(id).map_or(0, |m| m.k);
            // Byte 2 bit 7: the alpha test, read by vs_morph.
            e.0[w::GLARE] = reg.morph_data_of(id).is_some_and(|m| m.alpha_test) as u32;
            e.0[w::SFRAMES] = reg.morph_at[id as usize];
            dk::MORPH
        }
        Draw::Sprite(id) => {
            e.0[w::FRAMES] = reg.sprite_frames(id);
            e.0[w::SFRAMES] = reg.sprite_at[id as usize].0;
            if reg.sprite_is_flare(id) {
                dk::FLARE
            } else if batched {
                dk::BATCHED
            } else {
                dk::SPRITE
            }
        }
    };
    let mut parts = Vec::new();
    let count = inst.emitted;
    let gen_dist = inst.dist;
    let key = |reg: &mut Registry, op: &Op| reg.key_ref(lib, from, op.u32(2));
    for op in stream.iter().skip(1) {
        let s = op.slot as usize;
        let v3 = |k: usize| Vec3::new(op.f32(k), op.f32(k + 1), op.f32(k + 2));
        match op.code {
            // Creation / lifetime.
            0x1d => {
                if let Draw::Sprite(id) = draw {
                    let n = reg.sprite_frames(id).max(1) as f32;
                    e.set(w::FRAME_PERIOD, life / n);
                    e.set(w::FRAME_CD, life / n);
                }
            }
            0x1e => e.0[w::BLEND] = op.u32(1) & 0xffff,
            0x30 => e.set(w::BIAS, op.f32(1)),
            // 0x72 scales by the target's model size; actors are not linked yet *(inf: size 1)*.
            0x72 => e.set(w::BIAS, op.f32(1)),
            // Position.
            0x06 if !batched => {
                let r = rng.uf(op.f32(1)) + op.f32(2);
                let mut m = euler(Vec3::new(rng.us(PI), rng.us(PI), 0.0));
                if gen_dist != 0.0 {
                    m.x_axis.x /= gen_dist;
                }
                pos += m.transform_point3(Vec3::new(r, 0.0, 0.0));
            }
            0x07 => {
                let r = rng.uf(op.f32(1)) + op.f32(2);
                let mr = euler(Vec3::new(rng.us(PI), rng.us(PI), 0.0));
                let ms = Mat4::from_scale(v3(3));
                let mf = euler(Vec3::new(0.0, op.f32(7), op.f32(6)));
                let mut m = then(mr, then(ms, mf));
                if gen_dist != 0.0 {
                    m.x_axis.x /= gen_dist;
                }
                pos += m.transform_point3(Vec3::new(r, 0.0, 0.0));
            }
            0x1f => {
                let a = wrap(op.f32(8) + rng.us(op.f32(9)));
                let n = op.i16_at(0x2c);
                let b = if n != 0 { ((count % (n as u32 + 1)) as f32 * PI / (n as f32 + 1.0)) * 2.0 - PI } else { rng.us(PI) };
                let mr = euler(Vec3::new(0.0, b, a));
                let ms = Mat4::from_scale(v3(3));
                let mf = euler(Vec3::new(0.0, op.f32(7), op.f32(6)));
                let mut m = then(then(mr, ms), mf);
                if op.byte(0x28) & 1 != 0
                    && let Some(att) = inst.matrix
                {
                    let mut a = att;
                    a.w_axis = glam::Vec4::W;
                    a = then(a, f.view_rot);
                    m = then(m, a.inverse());
                }
                if gen_dist != 0.0 {
                    m.x_axis.x /= gen_dist;
                }
                let r = rng.uf(op.f32(1)) + op.f32(2);
                pos += m.transform_point3(Vec3::new(r, 0.0, 0.0));
            }
            0x45 | 0x86 | 0x9b => {
                if let Some(p) = parent {
                    pos += p.pos;
                    pos0 = pos;
                }
            }
            // Point list (Vtx, kind 0x3e): u32 count, points from +0x10 (vec4); random (0x4e) or by emission count
            // (0x4f) *(the attach-rotation option, byte op+0xc bit 0, is not applied)*.
            0x4e | 0x4f if !batched => {
                if let Some(r) = reg.resolve(lib, from, kind::VTX, op.u32(2)) {
                    let d = reg.libs[r.0].data(r.1);
                    let n = d.get(0..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
                    if n > 0 {
                        let i = if op.code == 0x4e { rng.un(n) } else { count % n } as usize;
                        let f = |o: usize| d.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
                        pos += Vec3::new(f(0x10 + i * 16), f(0x14 + i * 16), f(0x18 + i * 16));
                    }
                }
            }
            0x87 => {
                e.set_sv3(s, pos);
                e.set_sv3(s + 3, Vec3::ZERO);
            }
            0x67 => {
                let v = e.sv3(s);
                pos += v * life;
                e.set_sv3(s, -v);
            }
            // Rotation / scale.
            0x09 => e.set_v3(w::ROT, v3(1)),
            0x0a => {
                let r = e.v3(w::ROT);
                e.set_v3(w::ROT, Vec3::new(wrap(r.x + rng.us(op.f32(1))), wrap(r.y + rng.us(op.f32(2))), wrap(r.z + rng.us(op.f32(3)))));
            }
            0x3b => {
                let r = e.v3(w::ROT);
                let c = count as f32;
                let spin = |k: usize, v: f32| (c * op.f32(k) + v) % (2.0 * PI) - PI;
                e.set_v3(w::ROT, Vec3::new(spin(1, r.x), spin(2, r.y), spin(3, r.z)));
            }
            0x3d => {
                let d = pos - pos0;
                e.set_sf(s + 3, -d.z.atan2(d.x));
            }
            0x0f => e.set_v3(w::SCALE, v3(1)),
            // On the source actor: local matrix times its ground frame, position = its foot contact (+0x63c).
            0x8e => {
                if let Some((p, m)) = inst.ground {
                    pos = p;
                    for (k, c) in m.iter().enumerate() {
                        e.set_v3(w::GROUND + 3 * k, *c);
                    }
                    e.0[w::STATE] |= st::GROUND;
                }
            }
            0x10 => {
                let sc = e.v3(w::SCALE);
                e.set_v3(w::SCALE, sc + Vec3::new(rng.uf(op.f32(1)), rng.uf(op.f32(2)), rng.uf(op.f32(3))));
            }
            0x11 => {
                let u = rng.uf(op.f32(1));
                e.set_v3(w::SCALE, e.v3(w::SCALE) + Vec3::splat(u));
            }
            0x47 => {
                if let Some(p) = parent {
                    e.set_v3(w::ROT, p.rot)
                }
            }
            0x79 => {
                if let Some(p) = parent {
                    e.set_v3(w::ROT, e.v3(w::ROT) + p.rot)
                }
            }
            0x49 => {
                if let Some(p) = parent {
                    e.set_v3(w::SCALE, p.scale)
                }
            }
            0x7a => {
                if let Some(p) = parent {
                    e.set_v3(w::SCALE, e.v3(w::SCALE) + p.scale)
                }
            }
            // Colour: file bytes R, G, B, A -> +0xfa, +0xf9, +0xf8, +0xfb (FUN_1003d440).
            0x16 => {
                let [r, g, b, a] = op.u32(1).to_le_bytes();
                e.0[w::COLOR] = u32::from_le_bytes([b, g, r, a]);
            }
            0x17 => {
                for (k, o) in [(2usize, 4usize), (1, 5), (0, 6)] {
                    let add = rng.un(op.byte(o) as u32) as u8;
                    e.set_cb(k, e.cb(k).wrapping_add(add));
                }
            }
            0x18 => {
                let add = rng.un(op.byte(4) as u32) as u8;
                for k in 0..3 {
                    e.set_cb(k, e.cb(k).wrapping_add(add));
                }
            }
            0x48 => {
                if let Some(p) = parent {
                    e.0[w::COLOR] = p.color
                }
            }
            0x91 => e.set_s(s, e.0[w::COLOR]),
            0x8f | 0x90 => {
                // Tint by the area light: the brighter of (sun + ambient) and (moon + ambient).
                let c = f.area_light;
                for (k, ch) in [(2usize, c.x), (1, c.y), (0, c.z)] {
                    e.set_cb(k, (e.cb(k) as f32 * ch).max(0.0) as u32 as u8);
                }
            }
            // Slot initialisers.
            0x02 => e.set_sv3(s, if batched { Vec3::ZERO } else { v3(1) }),
            0x03 | 0x0c | 0x13 => {
                for c in 0..3 {
                    e.set_sf(s + c, e.sf(s + c) + rng.us(op.f32(1 + c)));
                }
            }
            0x0b | 0x12 => e.set_sv3(s, v3(1)),
            0x31 => e.set_sv3(s, Vec3::splat(rng.us(op.f32(1)))),
            0x19 => {
                e.set_s(s, op.u32(1));
                e.set_s(s + 1, op.u32(2));
            }
            0x1a => {
                for (c, o) in [(0usize, 4usize), (1, 6), (2, 8)] {
                    let word = e.s(s + c / 2);
                    let half = (c % 2) * 16;
                    let cur = (word >> half) as u16 as i16;
                    let add = rng.sn(op.i16_at(o) as i32) as i16;
                    let nw = (word & !(0xffff << half)) | (((cur.wrapping_add(add)) as u16 as u32) << half);
                    e.set_s(s + c / 2, nw);
                }
            }
            0x3e..=0x40 => {
                let c = (op.code - 0x3e) as usize;
                e.set_sf(s + c, op.f32(1) + rng.us(op.f32(2)));
            }
            0x41 => {
                let d = (pos - pos0).normalize_or_zero();
                let u = rng.us(op.f32(1));
                e.set_sv3(s, e.sv3(s) + d * u);
            }
            0x08 if !batched => {
                let d = (pos - pos0).normalize_or_zero();
                e.set_sv3(s, e.sv3(s) + d * op.f32(1));
            }
            0x50..=0x52 => {
                e.set_s(s, op.u32(1));
                e.set_s(s + 1, op.u32(3));
                if let Some(k) = reg.update_op_slot(inst.src, 0x02) {
                    let src = match op.code {
                        0x50 => k,
                        0x51 => k + 1,
                        _ => k + 2,
                    };
                    e.set_s(k + 2, e.s(src));
                }
            }
            // Keyframe bindings: S[s] = keyframe, S[s+1] = its parameter, S[s+2] = the base value.
            0x21..=0x2f | 0x33..=0x39 | 0x59..=0x66 | 0x68 | 0x69 | 0x6c..=0x70 | 0x74..=0x78 | 0x7c | 0x7d | 0x80..=0x85 | 0x8b..=0x8d | 0x93 | 0x95..=0x97 => {
                e.set_s(s, key(reg, op));
                e.set_s(s + 1, op.u32(3));
                let r = e.v3(w::ROT);
                let base: Option<u32> = match op.code {
                    0x21 | 0x95 => Some(pos.x.to_bits()),
                    0x22 | 0x96 => Some(pos.y.to_bits()),
                    0x23 | 0x97 => Some(pos.z.to_bits()),
                    0x24 => Some((r.x / PI).to_bits()),
                    0x25 => Some((r.y / PI).to_bits()),
                    0x26 => Some((r.z / PI).to_bits()),
                    0x27..=0x29 => Some(e.f(w::SCALE + (op.code - 0x27) as usize).to_bits()),
                    0x2a => Some((e.cb(2) as f32 / 255.0).to_bits()),
                    0x2b => Some((e.cb(1) as f32 / 255.0).to_bits()),
                    0x2c => Some((e.cb(0) as f32 / 255.0).to_bits()),
                    0x2d => Some((e.cb(3) as f32 / 255.0).to_bits()),
                    0x2e => {
                        e.0[w::STATE] |= st::SCROLL_U;
                        Some(e.0[w::UV])
                    }
                    0x2f => {
                        e.0[w::STATE] |= st::SCROLL_V;
                        Some(e.0[w::UV + 1])
                    }
                    0x33..=0x37 => Some(0.0f32.to_bits()),
                    0x38 => Some((1.0f32 / 120.0).to_bits()),
                    _ => None,
                };
                if let Some(b) = base {
                    e.set_s(s + 2, b);
                }
            }
            // Child generators.
            0x3c => {
                if let Some(g) = reg.resolve(lib, from, kind::GENERATOR, op.u32(2)) {
                    children.push(Child::Burst { src: g });
                }
            }
            0x44 | 0x53 | 0x6a => {
                if let Some(g) = reg.resolve(lib, from, kind::GENERATOR, op.u32(2)) {
                    children.push(Child::Clone { src: g, op: op.code });
                }
            }
            // Copy from parent.
            0x4a => {
                if let Some(p) = parent {
                    e.set(w::UV, p.uv[0]);
                    e.set(w::UV + 1, p.uv[1]);
                }
            }
            0x9d => {
                if let Some(p) = parent {
                    e.set(w::BIAS, p.bias)
                }
            }
            0x98..=0x9a => {
                if let Some(p) = parent.filter(|p| p.max_life != 0.0) {
                    e.set(w::LIFE, p.life);
                    e.set(w::MAX_LIFE, p.max_life);
                }
            }
            // Op 0x4b (CMoLfdElem): the glare colour (the two floats have no reader).
            0x4b if e.0[w::KIND] == dk::FLARE => {
                let [r, g, b, a] = op.u32(3).to_le_bytes();
                e.0[w::GLARE] = u32::from_le_bytes([b, g, r, a]);
            }
            // Sound ops (CYySoundElem only).
            0x4c if sound.is_some() => {
                let s = sound.as_mut().unwrap();
                if s.loops || inst.flags & 0x400 != 0 || life == 0.0 {
                    inst.flags &= !0x800;
                }
                if inst.mode == 0 {
                    s.ymode = 1;
                }
                s.near = op.f32(2);
                s.far = op.f32(1);
                s.vol = 127;
            }
            0x6b if sound.is_some() => {
                if let Some(r) = reg.resolve(lib, from, kind::SPH, op.u32(2)) {
                    sound.as_mut().unwrap().sph = Some(std::sync::Arc::new(super::sound::sph(reg.libs[r.0].data(r.1))));
                }
            }
            0x8a if sound.is_some() => sound.as_mut().unwrap().keep = true,
            0x92 if sound.is_some() => sound.as_mut().unwrap().ymode = 1,
            0x9e if sound.is_some() => sound.as_mut().unwrap().ymode = 2,
            // Op 0x56: the particle block of a batched D3a (CMoVuAnmElem), from the other ops of this generator.
            0x56 if e.0[w::KIND] == dk::BATCHED => {
                let find = |st: usize, code: u8| -> Option<Op> { inst.streams[st].iter().find(|o| o.code == code).cloned() };
                let speed = find(1, 0x08).map_or(0.0, |o| o.f32(1));
                let o7 = find(1, 0x07);
                let r = o7.as_ref().map(|o| o.f32(1)).or_else(|| find(1, 0x06).map(|o| o.f32(1))).unwrap_or(0.0);
                let bscale = o7.as_ref().map_or(Vec3::ONE, |o| Vec3::new(o.f32(3), o.f32(4), o.f32(5)));
                let base = find(1, 0x02).map_or(Vec3::ZERO, |o| Vec3::new(o.f32(1), o.f32(2), o.f32(3)));
                let gravity = find(2, 0x03).map_or(Vec3::ZERO, |o| Vec3::new(o.f32(1), o.f32(2), o.f32(3)));
                let drag = find(2, 0x2c).map_or(1.0, |o| o.f32(1)) - 1.0;
                let n = (inst.flags & 0x1ff) as usize + 1;
                for _ in 0..n {
                    let (r1, r2, r3) = (rng.us(1.0), rng.us(1.0), rng.us(1.0));
                    let u = Vec3::new(r3, r2, r1);
                    let p0 = u * r * bscale;
                    let dir = u.normalize_or_zero();
                    let vel = dir * speed + base;
                    parts.push([p0.x, p0.y, p0.z, dir.x, dir.y, dir.z, vel.x, vel.y, vel.z]);
                }
                e.0[w::PART_N] = n as u32;
                e.set_v3(w::GRAVITY, gravity);
                e.set(w::DRAG, drag);
            }
            // Ground snap, point lists, paths, sound, class-specific: not handled yet.
            _ => reg.unhandled(1, op.code),
        }
    }
    e.set_v3(w::POS, pos);
    let life = e.f(w::LIFE);
    let max_life = e.f(w::MAX_LIFE);
    let (stream_at, op2) = reg.stream(inst.src);
    e.0[w::STREAM] = stream_at;
    e.0[w::OP2_SLOT] = op2.map_or(NONE, |s| s as u32);
    e.0[w::PARENT] = if snapshot { NONE } else { inst.mslot };
    let cpu = ElemCpu {
        inst: usize::MAX,
        serial: 0,
        life,
        max_life,
        flags,
        blend: e.0[w::BLEND] as u16,
        draw,
        key: e.f(w::BIAS),
        drawn: false,
        back: None,
        child: None,
        snapshot: snapshot.then(|| inst.matrix.unwrap_or(Mat4::IDENTITY)),
        mslot: NONE,
        parts: None,
    };
    Some(Spawn { elem: e, cpu, children, parts, sound })
}

/// FUN_10048740 steps 2-3: the generator's own stream. Returns false when this frame's emission is skipped
/// (distance or view gate).
pub fn generator_stream(reg: &mut Registry, inst: &mut Instance, f: &Frame) -> bool {
    let x = if inst.life > 0.0 { (1.0 - (inst.life - inst.elapsed) / inst.life).min(1.0) } else { 0.0 };
    let mut emit = true;
    let ops = std::mem::take(&mut inst.streams[0]);
    let lib = inst.src.0;
    let from = inst.src.1;
    let sample = |reg: &Registry, op: &Op, x: f32| -> Option<f32> {
        let r = reg.resolve(lib, from, kind::KEYFRAME, op.u32(2))?;
        Some(reg.keyframe(r)?.sample(x, op.u32(3) & 0xf != 0))
    };
    let set_emission = |inst: &mut Instance, code: u8, word: usize, v: f32| {
        if let Some(o) = inst.streams[1].iter_mut().find(|o| o.code == code)
            && let Some(slot) = o.words.get_mut(word)
        {
            *slot = v.to_bits();
        }
    };
    let half_turns = |v: f32| wrap((v % 2.0) * 3.1415);
    for op in &ops {
        match op.code {
            0x04 | 0x17 => {
                let t = if op.code == 0x17 { f.dayfrac } else { x };
                if op.code == 0x04 && x == 0.0 {
                    continue;
                }
                match sample(reg, op, t) {
                    Some(v) if v > 0.0 => {
                        let interval = ((60.0 / v) as i32).min(0x7fff) as i16;
                        inst.interval = interval;
                        inst.flags = (inst.flags & !0x1ff) | ((v / 60.0) as u32 & 0x1ff);
                        inst.countdown = inst.countdown.min(interval as f32);
                    }
                    Some(_) => inst.countdown = 32767.0,
                    None => {
                        inst.countdown = 32767.0;
                    }
                }
            }
            0x05 => {
                if let Some(v) = sample(reg, op, x) {
                    set_emission(inst, 0x08, 1, v)
                }
            }
            0x06 | 0x07 => {
                if let Some(v) = sample(reg, op, x) {
                    set_emission(inst, 0x1f, if op.code == 0x06 { 1 } else { 2 }, v)
                }
            }
            0x08 | 0x09 => {
                if let Some(v) = sample(reg, op, x) {
                    set_emission(inst, 0x1f, if op.code == 0x08 { 6 } else { 7 }, half_turns(v))
                }
            }
            0x0b..=0x0d => {
                if let Some(v) = sample(reg, op, x) {
                    set_emission(inst, 0x01, 5 + (op.code - 0x0b) as usize, v)
                }
            }
            0x12..=0x14 => {
                if let Some(v) = sample(reg, op, x) {
                    set_emission(inst, 0x02, 1 + (op.code - 0x12) as usize, v)
                }
            }
            0x0a => {
                let p = inst.streams[1].first().map(|o| Vec3::new(o.f32(5), o.f32(6), o.f32(7))).unwrap_or_default();
                let wp = inst.matrix.map_or(p, |m| m.transform_point3(p));
                let d = (f.eye - wp).length();
                let far = if op.f32(1) == 0.0 { f.draw_distance } else { op.f32(1) };
                if d > far || d < op.f32(2) {
                    emit = false;
                    if op.u32(3) & 1 != 0 {
                        inst.stopping = true;
                        inst.flags &= !0x800;
                    }
                    break;
                }
            }
            // 0x11 follow is the attachment (fx/mod.rs recomputes the matrix every frame); 0x0e..0x10 spin and
            // 0x15 view-box gate are not handled yet.
            0x11 => inst.flags |= 0x200,
            _ => reg.unhandled(0, op.code),
        }
    }
    inst.streams[0] = ops;
    emit
}
