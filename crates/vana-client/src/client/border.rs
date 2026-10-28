//! The objective border (s2c 0x075's centre and radius, FUN_10087990): 8 parts of kinds 0x35..0x3c, each an actor of
//! the border file (52741 red, 52751 blue) placed on the circle near the player (FUN_100c9c10). The walls are the
//! file's effects: `efon` loops `lp00` (generators bou3 / bou2), `efof` stops them, `init` runs `efkl`.

use super::{App, Source, World, drawn_pos};
use glam::Vec3;
use std::f32::consts::PI;

/// The parts' actor ids: this bit and the part (0..7).
pub const BORDER: u32 = 0x0800_0000;
const PARTS: usize = 8;

/// The player's parts (entity +0x134 bits 9..16, actors +0xac..): their file, `init` run, +0x8ac bit 2 (`efon`
/// played), and this frame's place and yaw.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    file: Option<u32>,
    init: [bool; PARTS],
    near: [bool; PARTS],
    place: [Option<(Vec3, f32)>; PARTS],
}

impl Parts {
    pub fn place_of(&self, id: u32) -> Option<(Vec3, f32)> {
        let k = (id & !BORDER) as usize;
        (id & BORDER != 0 && k < PARTS).then(|| self.place[k]).flatten()
    }
}

/// FUN_100d43e0: table 0x10330f34 by colour (0 red, 1 blue) + 0xca53.
fn file(blue: bool) -> u32 {
    if blue { 0x3bc + 0xca53 } else { 0x3b2 + 0xca53 }
}

/// FUN_10092780 for each part. The looping `lp00` goes with it *(inf: retail frees the part's file with it)*.
pub fn remove(w: &mut World) {
    for k in 0..PARTS as u32 {
        let id = BORDER | k;
        w.effects.stop_actor_scheduler(id, u32::from_le_bytes(*b"lp00"));
        w.effects.forget_actor(id);
        w.actors.remove(&id);
    }
    w.border = Parts::default();
}

/// The parts' arc (FUN_100c9c10): the point `render` from the camera toward the player, its bearing from the centre
/// in whole degrees (ftol of -atan2(x, z) in degrees), start = (-4 - deg / 15) x 15; part k at start + 15 k on the
/// circle, at the player's height, facing along it.
fn arc(c: (f32, f32), r: f32, render: f32, eye: Vec3, me: Vec3) -> [(Vec3, f32); PARTS] {
    let d = Vec3::new(me.x - eye.x, 0.0, me.z - eye.z).normalize_or_zero();
    let q = eye + d * render;
    let q = Vec3::new(q.x - c.0, 0.0, q.z - c.1).normalize_or_zero();
    let deg = (q.x.atan2(q.z) * -57.295_78) as i32;
    let start = (-4 - deg / 15) * 15;
    std::array::from_fn(|k| {
        let mut a = (start + 15 * k as i32) as f32 * 0.017_453_3;
        if a > PI {
            a -= 2.0 * PI;
        }
        if a < -PI {
            a += 2.0 * PI;
        }
        (Vec3::new(c.0 + r * a.sin(), me.y, c.1 + r * a.cos()), a)
    })
}

/// Per frame: the parts made while a radius is set (FUN_1008f740's block), `init` once each is in, then the arc and
/// `efon` / `efof` as the player comes within `render` of the line or leaves it (all parts ready first).
pub fn update(w: &mut World) {
    let b = w.objective.border;
    if b.radius <= 0.0 {
        if w.border.file.is_some() {
            remove(w);
        }
        return;
    }
    if w.border.file.is_none() {
        let f = file(b.blue);
        log::info!("border: radius {} at ({}, {}), file {f}", b.radius, b.x, b.z);
        for k in 0..PARTS as u32 {
            w.event_looks.push((BORDER | k, Source::File(f)));
        }
        w.border.file = Some(f);
    }
    let ready: Vec<bool> = (0..PARTS as u32).map(|k| w.actors.get(&(BORDER | k)).is_some_and(|a| a.character.is_some())).collect();
    for (k, &r) in ready.iter().enumerate() {
        if r && !w.border.init[k] {
            w.effects.on_scheduler_quiet(BORDER | k as u32, u32::from_le_bytes(*b"init"));
            w.border.init[k] = true;
        }
    }
    if !ready.iter().all(|&r| r) {
        return;
    }
    let me = drawn_pos(w);
    let eye = App::camera(w).eye;
    let places = arc((b.x, b.z), b.radius, b.render, eye, me);
    let d = Vec3::new(me.x - b.x, 0.0, me.z - b.z).length();
    let near = (b.radius - d).abs() < b.render;
    if near != w.border.near[0] {
        log::debug!("border: {} at {me:?}, parts {:?} .. {:?}", if near { "efon" } else { "efof" }, places[0], places[7]);
    }
    for (k, p) in places.into_iter().enumerate() {
        w.border.place[k] = Some(p);
        if near != w.border.near[k] {
            w.border.near[k] = near;
            let name = if near { *b"efon" } else { *b"efof" };
            w.effects.on_scheduler_quiet(BORDER | k as u32, u32::from_le_bytes(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The camera south of the player, both north of the centre: the arc centres on the north (+z) side.
    #[test]
    fn arc_faces_the_view() {
        let p = arc((0.0, 0.0), 25.0, 25.0, Vec3::new(0.0, -5.0, 10.0), Vec3::new(0.0, 0.0, 20.0));
        // q = (0, 45) -> bearing 0, start -60: parts at -60..45 degrees.
        assert!((p[0].1 + 60.0f32.to_radians()).abs() < 1e-3 && (p[7].1 - 45.0f32.to_radians()).abs() < 1e-3);
        assert!((p[4].0 - Vec3::new(0.0, 0.0, 25.0)).length() < 1e-3);
        assert_eq!(p[4].0.y, 0.0);
    }

    #[test]
    fn files() {
        assert_eq!((file(false), file(true)), (52741, 52751));
    }
}
