//! Footsteps (FUN_100d2970; ~/ffxi/lab/research/effects/footsteps_sound.md section 1): the posed foot points 9 and
//! 8 (and 0xb / 0xa for four-legged models) relative to the actor root, read back from the GPU; a point that comes
//! down to root level (height from < 0 to >= 0) on a moving actor near the camera is a step, which plays the
//! surface's footstep sound.

use super::{Place, World};
use glam::Vec3;
use std::collections::HashMap;

/// Front feet first, then the hind feet (actor+0x878 == 2).
pub const POINTS: [usize; 4] = [9, 8, 0xb, 0xa];

pub fn key(id: u32, slot: usize) -> u64 {
    (id as u64) << 8 | slot as u64
}

/// The gates before the points are read: a footstep kind (Cib byte 1 >= 1), a status FUN_100c4390 calls it for
/// (0, 1, 4, 0x1c, 0x1f, 5, 0x55) and within 30 units of the camera eye.
pub fn wanted(ch: &crate::dat::character::Character, status: u8, pos: Vec3, eye: Vec3) -> bool {
    ch.cib[1] >= 1 && matches!(status, 0 | 1 | 4 | 0x1c | 0x1f | 5 | 0x55) && pos.distance_squared(eye) < 900.0
}

/// Heights that came back: a crossing on a moving actor plays its step.
pub(super) fn steps(w: &mut World, places: &HashMap<u32, Place>, points: Vec<(u64, Vec3)>) {
    // The last point to touch down (tested 9, 8, 0xb, 0xa), relative to the root.
    let mut contact: HashMap<u32, Option<Vec3>> = HashMap::new();
    let actors = &w.actors;
    w.feet.retain(|id, _| actors.contains_key(id));
    w.contacts.retain(|id, _| actors.contains_key(id));
    for (k, p) in points {
        let (id, slot) = ((k >> 8) as u32, (k & 0xff) as usize);
        let prev = w.feet.entry(id).or_insert([0.0; 4]);
        let c = prev[slot.min(3)] < 0.0 && p.y >= 0.0;
        prev[slot.min(3)] = p.y;
        let e = contact.entry(id).or_default();
        if c {
            *e = Some(p);
        }
    }
    for (id, hit) in contact {
        let (Some(pl), Some(ch)) = (places.get(&id), w.actors.get(&id).and_then(|a| a.character.clone())) else { continue };
        let Some(foot) = hit else { continue };
        // The motion gate (+0x7d8 != the idle 'idl?', or leg mode 3): moving *(inf: from the movement ratio)*.
        if pl.ratio <= 1e-4 && ch.cib[0] != 3 {
            continue;
        }
        let Some(g) = &w.ground else { continue };
        let (surface, covered) = g.surface(pl.pos);
        let kind = ch.cib[1] as i32;
        if surface == 0 || kind < 1 {
            continue;
        }
        let c = if kind < 10 { b'0' + kind as u8 } else { b'a' + (kind - 10) as u8 };
        let name = [b'0', b'0' + surface, c, b'1' + ch.cib[2].max(0) as u8];
        let rain = matches!(crate::dat::weather::weather_tag(w.weather), "rain" | "squl" | "bolt") && !covered;
        // actor+0x824 < 0.5: walking, level 101.6 -> 101; else 127.
        let level = if pl.ratio < 0.5 { 101 } else { 127 };
        w.contacts.insert(id, (pl.pos + foot, g.floor_normal(pl.pos).unwrap_or(Vec3::Y)));
        // Dust "w100" walking / "0100" running, unless leg mode 3 (footstep-effects option DAT_10444158, default on).
        let dust = (ch.cib[0] != 3).then_some([if level == 101 { b'w' } else { b'0' }, b'0' + surface, b'0', b'0']);
        log::debug!("footstep {id:#x} {} level {level}", String::from_utf8_lossy(&name));
        // Footprints on surfaces 3, 4 and 8, unless leg mode 3.
        let print = ch.cib[0] != 3 && matches!(surface, 3 | 4 | 8);
        w.effects.footstep(id, pl.pos, name, dust, print, rain, level);
    }
}

#[cfg(test)]
mod tests {
    use crate::fx::sound::SoundCmd;
    use glam::Vec3;

    /// footsteps_sound.md 1.2 survey and 1.5: Hume M feet 8136 is kind 1 shoe 1, the orc 1300 (0, 9, 0); in zone 232
    /// running on surface 1 plays fses/0112 = se 101001 at 95 from 8 units out and 1.8 up, walking at 76.
    #[test]
    fn port_sandoria_step() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let info = |id| crate::dat::character::load_file(&index.read(id).unwrap()).unwrap().info.unwrap();
        assert_eq!(info(8136)[..3], [0xff, 1, 1]);
        assert_eq!(info(1300)[..3], [0, 9, 0]);
        let mut fx = crate::fx::Effects::new();
        fx.set_keys(std::sync::Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(crate::fx::lib::Library::load(332, index.read(332).unwrap()).unwrap());
        let eye = Vec3::new(0.0, -1.8, 0.0);
        let pos = Vec3::new(8.0, 0.0, 0.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, pos, Vec3::NEG_Y);
        let f = crate::fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view, view_rot: view, eye, draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let mut plays = Vec::new();
        for level in [127, 101] {
            fx.footstep(1, pos, *b"0112", None, false, false, level);
            fx.tick(&f, &crate::fx::NoActors);
            plays.extend(fx.sound_cmds.drain(..).filter_map(|c| match c {
                SoundCmd::Play { se, vol, pan, .. } if se != 2009 => Some((se, vol, pan)),
                _ => None,
            }));
        }
        assert_eq!(plays, [(101001, 95, 64), (101001, 76, 64)]);
    }

}
