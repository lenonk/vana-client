//! Ops 0x26 / 0x27 (CMoPathDriveActorTask, FUN_1005e890 / FUN_1005e9c0 / FUN_1005eef0) and 0xa9 / 0xaa
//! (CMoActorRotationDriveTask, FUN_1005fa20 / FUN_1005fb30): an actor moved along a Vtx path or turned over an op's
//! duration, children of the scheduler task that started them (spec ~/ffxi/lab/research/ship_actors_spec.md 3.4,
//! 3.5, 3.8).

use super::spline::{self, Seg};
use crate::dat::generator::KeyFrame;
use glam::Vec3;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// What a frame's drives set on an actor: its position (+0xd4), its heading target (+0xe4), and the drawn heading
/// snapped to it too (+0x744 / +0x44).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActorPose {
    pub pos: Option<Vec3>,
    pub target: Option<Vec3>,
    pub snap: Option<Vec3>,
}

/// FUN_10055ad0: the path drive's easing of u; 0 and 5 (no KeyFrame) are linear.
pub fn ease(u: f32, mode: u32) -> f32 {
    match mode {
        1 => (u * FRAC_PI_2).sin(),
        2 => 1.0 - (u * FRAC_PI_2).cos(),
        3 if u <= 0.5 => 0.5 * (u * PI).sin(),
        3 => 1.0 - 0.5 * (u * PI).sin(),
        4 => (1.0 - (u * PI).cos()) / 2.0,
        _ => u,
    }
}

fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// Ticks between the samples the facing is taken from: retail's frame at 30 fps *(ours: frame-rate independent)*.
const FACE_STEP: f32 = 2.0;

/// Op 0x26 (actor1) / 0x27 (actor2).
pub struct Drive {
    pub actor: u32,
    /// The scheduler task's uid: the drive dies with it.
    pub owner: u64,
    segs: Vec<Seg>,
    total: f32,
    left: f32,
    /// Flags @0x10: bit 0 absolute, bits 1..2 face (bit 1 of it: no pitch), bit 3 forward.
    abs: bool,
    face: u32,
    fwd: bool,
    yaw0: f32,
    curve: u32,
    key: Option<KeyFrame>,
    started: bool,
    turned: bool,
    /// The last point applied (relative moves), and the facing's sample and ticks since it.
    last: Vec3,
    sample: Vec3,
    since: f32,
    /// Made this frame: its first step places it without running its time.
    pub fresh: bool,
}

impl Drive {
    /// The op's fields: i16 @6 duration (x speed), u32 @0x10 flags, u32 @0x14 curve, f32 @0x18 yaw0; `over` is the
    /// fast-forward's overshoot (DAT_1047bf14), taken off the time left.
    pub fn new(actor: u32, owner: u64, segs: Vec<Seg>, raw: &[u8], speed: f32, over: f32, key: Option<KeyFrame>) -> Self {
        let u32_at = |o: usize| raw.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let dur = raw.get(6..8).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]));
        let flags = u32_at(0x10);
        let total = dur as f32 * speed;
        Self {
            actor,
            owner,
            segs,
            total,
            left: total - over,
            abs: flags & 1 != 0,
            face: flags >> 1 & 3,
            fwd: flags & 8 != 0,
            yaw0: f32::from_bits(u32_at(0x18)),
            curve: u32_at(0x14).min(5),
            key,
            started: false,
            turned: false,
            last: Vec3::ZERO,
            sample: Vec3::ZERO,
            since: 0.0,
            fresh: true,
        }
    }

    fn at(&self, u: f32) -> Vec3 {
        spline::eval(&self.segs, u)
    }

    /// One frame of `dt` ticks, the actor at `pos` turned `rot` now: what it sets, and whether it goes on (the end
    /// applies u = 1.0 when it ever ran, whatever its direction).
    pub fn step(&mut self, dt: f32, pos: Vec3, rot: Vec3) -> (ActorPose, bool) {
        let dt = if std::mem::take(&mut self.fresh) { 0.0 } else { dt };
        self.left -= dt;
        if self.left >= 0.0 {
            let u = if self.total > 0.0 { self.left / self.total } else { 0.0 };
            let u = if self.fwd { 1.0 - u } else { u };
            let f = match (&self.key, self.curve) {
                (_, c) if c < 5 => ease(u, c),
                (Some(k), _) => k.linear(u),
                _ => u,
            };
            let pose = self.apply(self.at(f), dt, pos, rot);
            self.started = true;
            (pose, true)
        } else if self.started {
            (self.apply(self.at(1.0), dt, pos, rot), false)
        } else {
            (ActorPose::default(), false)
        }
    }

    fn apply(&mut self, p: Vec3, dt: f32, pos: Vec3, rot: Vec3) -> ActorPose {
        let mut out = ActorPose::default();
        if self.abs {
            out.pos = Some(p);
        } else if self.started {
            out.pos = Some(pos + (p - self.last));
        }
        self.last = p;
        if self.face == 0 {
            return out;
        }
        if !self.started {
            (self.sample, self.since) = (p, 0.0);
            return out;
        }
        self.since += dt;
        if self.since < FACE_STEP {
            return out;
        }
        let d = p - self.sample;
        (self.sample, self.since) = (p, 0.0);
        let h = (d.x * d.x + d.z * d.z).sqrt();
        let yaw = wrap_pi(self.yaw0 - d.z.atan2(d.x));
        // Face 2 / 3 leave retail's pitch uninitialised (no file uses them): ours keeps it *(inf)*.
        let pitch = if self.face & 2 == 0 { d.y.atan2(h) } else { rot.z };
        let r = Vec3::new(0.0, yaw, pitch);
        if !self.turned {
            self.turned = true;
            out.snap = Some(r);
        } else if h >= 0.1 {
            out.target = Some(r);
        }
        out
    }
}

/// Op 0xa9 (actor1) / 0xaa (actor2): f32 @8 / @0xc / @0x10 the target in degrees, u8 @0x14 the mode, i16 @6 the
/// duration (x speed); the start is the actor's drawn rotation.
pub struct RotDrive {
    pub actor: u32,
    pub owner: u64,
    start: Vec3,
    target: Vec3,
    mode: u8,
    total: f32,
    left: f32,
}

impl RotDrive {
    pub fn new(actor: u32, owner: u64, raw: &[u8], speed: f32, start: Vec3) -> Self {
        let f = |o: usize| raw.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
        let dur = raw.get(6..8).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]));
        let total = dur as f32 * speed;
        Self { actor, owner, start, target: Vec3::new(f(8), f(0xc), f(0x10)) * (PI / 180.0), mode: raw.get(0x14).copied().unwrap_or(0), total, left: total }
    }

    /// Mode 0 holds the target every frame until its scheduler ends; the others turn linearly (no wrap) and end
    /// after the frame they reach it.
    pub fn step(&mut self, dt: f32) -> (ActorPose, bool) {
        if self.mode == 0 {
            return (ActorPose { snap: Some(self.target), ..Default::default() }, true);
        }
        self.left -= dt;
        let k = if self.left >= 0.0 && self.total > 0.0 { 1.0 - self.left / self.total } else { 1.0 };
        let r = self.start + (self.target - self.start) * k;
        (ActorPose { target: Some(r), ..Default::default() }, self.left >= 0.0)
    }
}

/// The Vtx segments of a path, None below two points.
pub fn path(vtx: &[u8]) -> Option<Vec<Seg>> {
    spline::build(&spline::points(vtx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ship_actors_spec 8: f(0.25) for modes 0..4; the ends.
    #[test]
    fn curves() {
        for (m, want) in [(0, 0.25), (1, 0.382_683), (2, 0.076_120), (3, 0.353_553), (4, 0.146_447)] {
            assert!((ease(0.25, m) - want).abs() < 1e-5, "mode {m}: {}", ease(0.25, m));
            assert!(ease(0.0, m).abs() < 1e-6 && (ease(1.0, m) - 1.0).abs() < 1e-6, "mode {m} ends");
        }
    }
}
