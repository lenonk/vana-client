//! Ours (not retail; Settings' Lamp Lights): real point lights at the zone's lamps in place of their baked lamplight.
//! A lamp is a zone glow mesh textured "light..." (San d'Oria's 'ltla'); one within 4 yalms of a data light (kind 0x47)
//! is lit already. Each other lamp gets a free slot of the light table, filled each frame from the nearest data light
//! (its colour, range and live intensity, so dusk and fades follow the data) at a point 0.7 yalm out from the lamp's
//! wall. The untextured mesh shells beside the glows (the baked lamplight, 'san_') are hidden (Effects::lamp_scan).

use super::ground::Ground;
use crate::fx::Effects;
use glam::Vec3;
use std::time::Instant;

/// A lamp's light: where it is, its table slot, and the data light's slot it copies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub pos: Vec3,
    pub slot: u32,
    pub template: u32,
}

#[derive(Default)]
pub struct Lamps {
    pub list: Vec<Lamp>,
    at: Option<Instant>,
}

/// Glows closer than this are one lamp.
const MERGE: f32 = 1.5;
/// A glow this close to a data light is lit by it.
const LIT: f32 = 4.0;
/// The light stands this far out from the lamp's wall, which is looked for this far away.
const OUT: f32 = 0.7;
const WALL: f32 = 2.0;
/// The last slot an actor's 8-bit light index can name.
const LAST: u32 = 254;
/// How far an actor takes a lamp's light from (the light's range cuts it in the shader).
const REACH: f32 = 16.0;

impl Lamps {
    /// Every second: the lamps from the zone's effects (none while `on` is off); Some when the list changed.
    pub fn update(&mut self, fx: &mut Effects, ground: Option<&Ground>, on: bool) -> Option<Vec<Lamp>> {
        if self.at.is_some_and(|t| t.elapsed().as_secs_f32() < 1.0) {
            return None;
        }
        self.at = Some(Instant::now());
        let scan = fx.lamp_scan(on);
        let first = fx.reg.light_keys.len() as u32;
        let list = if on { build(&scan.glows, &scan.lights, first, |a, b| ground?.raycast(a, b)) } else { Vec::new() };
        if list == self.list {
            return None;
        }
        log::info!("lamps: {} glows, {} data lights, {} shells: {} lamp lights", scan.glows.len(), scan.lights.len(), scan.shells, list.len());
        self.list = list.clone();
        Some(list)
    }
}

/// The lamps of `glows` not already lit by `lights` (position, slot), slots from `first`.
fn build(glows: &[Vec3], lights: &[(Vec3, u32)], first: u32, ray: impl Fn(Vec3, Vec3) -> Option<f32>) -> Vec<Lamp> {
    let mut groups: Vec<(Vec3, f32)> = Vec::new();
    for &g in glows {
        match groups.iter_mut().find(|(s, n)| (*s / *n).distance(g) < MERGE) {
            Some((s, n)) => {
                *s += g;
                *n += 1.0;
            }
            None => groups.push((g, 1.0)),
        }
    }
    let mut lamps: Vec<Vec3> = groups.into_iter().map(|(s, n)| s / n).filter(|c| !lights.iter().any(|(p, _)| p.distance(*c) < LIT)).collect();
    lamps.sort_by(|a, b| (a.x, a.z, a.y).partial_cmp(&(b.x, b.z, b.y)).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    for (k, c) in lamps.into_iter().enumerate() {
        let slot = first + k as u32;
        let Some(&(_, template)) = lights.iter().min_by(|a, b| a.0.distance(c).total_cmp(&b.0.distance(c))) else { break };
        if slot > LAST {
            break;
        }
        out.push(Lamp { pos: off_wall(c, &ray), slot, template });
    }
    out
}

/// The nearest wall within WALL in eight level directions: the point OUT from it, else the glow itself.
fn off_wall(c: Vec3, ray: &impl Fn(Vec3, Vec3) -> Option<f32>) -> Vec3 {
    let wall = (0..8)
        .filter_map(|k| {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            let d = Vec3::new(a.cos(), 0.0, a.sin());
            ray(c, c + d * WALL).map(|t| (t, d))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    wall.map_or(c, |(t, d)| c + d * (t - OUT))
}

/// An actor's floor lights with the nearest lamps in their free places (slot + 1, as the floor's).
pub fn offer(lamps: &[Lamp], pos: Vec3, lights: &mut [u8; 4]) {
    if lamps.is_empty() {
        return;
    }
    let mut near: Vec<(f32, u32)> = lamps.iter().map(|l| (l.pos.distance(pos), l.slot)).filter(|(d, s)| *d < REACH && !lights.contains(&((s + 1) as u8))).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut more = near.into_iter();
    for k in lights.iter_mut().filter(|k| **k == 0) {
        if let Some((_, s)) = more.next() {
            *k = (s + 1) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// San d'Oria's gate tunnel: four glows by the door lights stay theirs; two glows of one lamp merge; a lamp by a
    /// wall at x = 0 stands 0.7 out from it; slots follow the zone's keys.
    #[test]
    fn lamps_from_glows() {
        let lights = [(Vec3::new(88.6, -65.5, 262.7), 3), (Vec3::new(97.4, -65.5, 271.0), 1)];
        let glows = [Vec3::new(86.0, -65.0, 264.0), Vec3::new(74.5, -65.0, 275.5), Vec3::new(74.9, -65.0, 275.5), Vec3::new(0.5, -65.0, 300.0)];
        let ray = |a: Vec3, b: Vec3| {
            let d = b - a;
            (d.x < 0.0 && a.x > 0.0 && a.x + d.x <= 0.0).then(|| a.x / -d.x * d.length())
        };
        let l = build(&glows, &lights, 10, ray);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].slot, l[0].template), (10, 3));
        assert!((l[0].pos - Vec3::new(0.7, -65.0, 300.0)).length() < 1e-3, "{:?}", l[0].pos);
        assert!((l[1].pos.x - 74.7).abs() < 1e-3 && l[1].template == 3);
        let mut f = [2u8, 0, 0, 0];
        offer(&l, Vec3::new(74.0, -65.0, 276.0), &mut f);
        assert_eq!(f, [2, 12, 0, 0]);
    }
}
