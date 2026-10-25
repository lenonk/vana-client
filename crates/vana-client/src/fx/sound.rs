//! Positional effect sound: the sound element CYySoundElem (update FUN_100356c0, manager FUN_10035410) and the
//! scheduler sound ops, with FUN_10035e50's distance volume and pan. Spec:
//! ~/ffxi/lab/research/effects/footsteps_sound.md (sections 2 and 3).

use glam::{Mat4, Vec3};

/// What the client's audio is asked to do this frame (sound thread commands 2, 4, 3, 0 / 5).
#[derive(Clone, Debug, PartialEq)]
pub enum SoundCmd {
    /// `stream`: the scheduler's stream voice (op 0x60), outside the world-effect volume.
    Play { handle: u32, se: u32, vol: i32, pan: u8, stream: bool },
    Level { handle: u32, vol: i32 },
    Pan { handle: u32, pan: u8 },
    /// Stop now (0) or fade over ticks.
    Stop { handle: u32, ticks: i32 },
}

/// +0x1e4: 3 manager passes (FUN_100356c0) at retail's 2-tick frames, in 1/60 s ticks.
pub const GRACE_TICKS: f32 = 6.0;

/// One sound: a generator's element (kind 0x3d), or a scheduler's op 0x0a / 0x0b / 0x53.
#[derive(Clone, Debug)]
pub struct Sound {
    pub se: u32,
    /// SeSep data @0xc bit 31 (FUN_10035950).
    pub loops: bool,
    /// The generator element it belongs to, and whether that generator is persistent (FUN_10052a30).
    pub elem: Option<u32>,
    pub gen_persistent: bool,
    /// Scheduler sounds: the actor's position when the op ran (+0x1a0); elements: emitter space position.
    pub pos: Vec3,
    pub near: f32,
    pub far: f32,
    /// +0x1e8: beyond far, play at volume 1 instead of stopping.
    pub keep: bool,
    /// +0x1e9: 0 height x3, 1 as is, 2 ignored.
    pub ymode: u8,
    /// +0x1be, 0..127.
    pub vol: i32,
    /// +0x1d0: the pan input (64 for elements and scheduler sounds; footsteps 0, so always centred).
    pub pan_in: f32,
    /// Update op 0x43: the volume keyframe (slot keyframe id, its parameter).
    pub vol_key: Option<(u32, u32)>,
    /// Emission op 0x6b: the sphere chain (Sph) the source rides, as capsules.
    pub sph: Option<std::sync::Arc<Vec<(Vec3, f32, Vec3, f32)>>>,
    /// The element's life, the fade when a generator's element goes with its voice (FUN_10035cd0).
    pub fade: f32,
    /// Scheduler op 0x60: the stream (DAT_10450fe8), centred and not attenuated.
    pub stream: bool,
    pub handle: u32,
    pub last_vol: i32,
    pub last_pan: u8,
    /// +0x1e4: counts as playing for 3 manager passes after a start.
    /// Ticks it counts as playing after a start.
    pub grace: f32,
    /// The SeSep resource (library, node) that FUN_10036460's start list counts.
    pub res: (usize, usize),
}

/// FUN_10038330: se ids of which one voice plays at a time (a shared flag, DAT_104638f4).
pub const ONE_AT_A_TIME: [u32; 2] = [2239, 2063];

impl Sound {
    pub fn new(se: u32, loops: bool) -> Self {
        Self { se, loops, elem: None, gen_persistent: false, pos: Vec3::ZERO, near: 0.0, far: 0.0, keep: false, ymode: 0, vol: 127, pan_in: 64.0, vol_key: None, sph: None, fade: 0.0, stream: false, handle: 0, last_vol: 0, last_pan: 64, grace: 0.0, res: (usize::MAX, se as usize) }
    }

    /// vt+0x40 = FUN_10035950.
    pub fn persistent(&self) -> bool {
        (self.elem.is_some() && self.gen_persistent) || self.loops
    }
}

/// SeSep (type 0x3d) data: se id @8 and the loop bit (@0xc bit 31).
pub fn sesep(d: &[u8]) -> Option<(u32, bool)> {
    let id = u32::from_le_bytes(d.get(8..12)?.try_into().ok()?);
    let flags = i32::from_le_bytes(d.get(12..16)?.try_into().ok()?);
    Some((id, flags < 0))
}

/// Sph (type 0x4a) data: points (x, y, z, r) from block u32 @0x10 + 0x10 at 0x20 each, segments (u32 pairs) from
/// block u32 @0x14 + 0x10.
pub fn sph(d: &[u8]) -> Vec<(Vec3, f32, Vec3, f32)> {
    let u = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let f = |o: usize| f32::from_bits(u(o).unwrap_or(0));
    let (Some(pts), Some(segs)) = (u(0x10), u(0x14)) else { return Vec::new() };
    let (pts, segs) = (pts as usize, segs as usize);
    let n = u(pts).unwrap_or(0) as usize;
    let point = |i: usize| {
        let b = pts + 0x10 + i * 0x20;
        (Vec3::new(f(b), f(b + 4), f(b + 8)), f(b + 12))
    };
    let m = u(segs).unwrap_or(0) as usize;
    (0..m.min(4096))
        .filter_map(|k| {
            let (a, b) = (u(segs + 0x10 + 8 * k)? as usize, u(segs + 0x14 + 8 * k)? as usize);
            (a < n && b < n).then(|| {
                let (pa, ra) = point(a);
                let (pb, rb) = point(b);
                (pa, ra, pb, rb)
            })
        })
        .collect()
}

/// FUN_100551b0: the offset from the listener to the nearest point of the capsule chain (radius interpolated
/// along each segment), pulled in by the radius, zero inside *(inf: FUN_10054fb0's exact clamp)*.
pub fn nearest(caps: &[(Vec3, f32, Vec3, f32)], l: Vec3) -> Vec3 {
    let mut best: Option<Vec3> = None;
    for &(a, ra, b, rb) in caps {
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 { ((l - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + ab * t;
        let r = ra + (rb - ra) * t;
        let off = q - l;
        let len = off.length();
        let o = if len <= r { Vec3::ZERO } else { off * ((len - r) / len) };
        if best.is_none_or(|b| o.length_squared() < b.length_squared()) {
            best = Some(o);
        }
    }
    best.unwrap_or(Vec3::ZERO)
}

/// FUN_10035e50: volume and pan of a source at `pos` for the listener at the eye. None = silent (stop).
#[allow(clippy::too_many_arguments)]
pub fn attenuate(pan_in: f32, vol: i32, pos: Vec3, eye: Vec3, view: Mat4, near: f32, far: f32, keep: bool, ymode: u8, focal: f32) -> Option<(i32, u8)> {
    let far = if far == 0.0 { 30.0 } else { far };
    let near = if near == 0.0 { 3.0 } else { near };
    let scale = 512.0;
    let mut d = eye - pos;
    match ymode {
        2 => d.y = 0.0,
        1 => {}
        _ => d.y *= 3.0,
    }
    let dist = d.length();
    let pan_of = |p: f32| p.clamp(0.0, 127.0) as u8;
    if dist < 0.1 {
        return (vol != 0).then_some((vol, pan_of(pan_in)));
    }
    let mut vol = vol;
    if dist > far {
        if !keep {
            return None;
        }
        vol = 1;
    } else if dist > near {
        vol = ((1.0 - (dist - near) / (far - near)) * vol as f32) as i32;
    }
    let v = view.transform_point3(pos);
    let zz = if v.z > 0.1 {
        vol = (vol as f32 * 0.65) as i32;
        v.z
    } else {
        v.z.abs()
    };
    let zz = zz.max(1.0);
    let f = (focal / zz * v.x / scale).clamp(-1.0, 1.0);
    let pan = pan_of(f * pan_in + 64.0);
    if vol != 0 {
        Some((vol, pan))
    } else if keep {
        Some((0, pan))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(se: u32, res: usize) -> Sound {
        let mut s = Sound::new(se, false);
        (s.stream, s.res) = (true, (0, res));
        s
    }

    fn frame(dt: f32) -> crate::fx::Frame {
        crate::fx::Frame { dt, seconds: dt / 60.0, dayfrac: 0.5, view: glam::Mat4::IDENTITY, view_rot: glam::Mat4::IDENTITY, eye: Vec3::ZERO, draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 }
    }

    fn plays(fx: &mut crate::fx::Effects) -> Vec<u32> {
        std::mem::take(&mut fx.sound_cmds).into_iter().filter_map(|c| if let SoundCmd::Play { se, .. } = c { Some(se) } else { None }).collect()
    }

    /// FUN_10036460: 20 SeSep resources a 2-tick pass; once full a repeat waits too.
    #[test]
    fn start_cap_per_pass() {
        let mut fx = crate::fx::Effects::new();
        fx.sounds.extend((0..25).map(|k| stream(1000 + k, k as usize)));
        let n: Vec<usize> = (0..5).map(|_| {
            fx.tick(&frame(0.5), &crate::fx::NoActors);
            plays(&mut fx).len()
        }).collect();
        assert_eq!(n, [20, 0, 0, 0, 5]);
        let mut fx = crate::fx::Effects::new();
        fx.sounds.extend((0..20).map(|k| stream(1000 + k, k as usize)));
        fx.sounds.push(stream(1000, 0));
        fx.tick(&frame(0.5), &crate::fx::NoActors);
        assert_eq!(plays(&mut fx).len(), 20);
    }

    /// FUN_10038330: of 2239 / 2063 one voice at a time; the next starts after it stops.
    #[test]
    fn one_at_a_time() {
        let mut fx = crate::fx::Effects::new();
        fx.sounds.extend([stream(2063, 1), stream(2063, 2), stream(2239, 3)]);
        fx.tick(&frame(2.0), &crate::fx::NoActors);
        assert_eq!(plays(&mut fx), [2063]);
        fx.tick(&frame(2.0), &crate::fx::NoActors);
        assert_eq!(plays(&mut fx), Vec::<u32>::new());
        // Each voice ends (not alive after its grace); the next one takes the flag, never two at once.
        fx.set_alive(Vec::new());
        let mut all = vec![2063];
        for _ in 0..20 {
            fx.tick(&frame(2.0), &crate::fx::NoActors);
            let p = plays(&mut fx);
            assert!(p.len() <= 1, "{p:?}");
            all.extend(p);
        }
        all.sort();
        assert_eq!(all, [2063, 2063, 2239]);
        assert!(fx.solo.is_none());
    }

    /// footsteps_sound.md 2.5: Cure's 0303 (far 60, near 3) on a target 12 units out and 1.5 up, behind the camera:
    /// volume 105, then x 0.65 = 68.
    #[test]
    fn cure_volume() {
        let eye = Vec3::ZERO;
        let pos = Vec3::new(12.0, -1.5, 0.0);
        // A view looking away from the target (it ends up behind: view z > 0).
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::new(-1.0, 0.0, 0.0), Vec3::NEG_Y);
        let (vol, pan) = attenuate(64.0, 127, pos, eye, view, 0.0, 59.99998, false, 0, 350.0).unwrap();
        assert_eq!((vol, pan), (68, 64));
        assert!(attenuate(64.0, 127, Vec3::new(70.0, 0.0, 0.0), eye, view, 0.0, 60.0, false, 0, 350.0).is_none());
    }

    /// footsteps_sound.md 2.5: zone 232 effe/soun/se00 plays se 2009 (persistent) from the nearest point of the
    /// harbour-edge chain 'se-s'; with the eye at (10, -6, 20) the source is 4.44 away: volume 120.
    #[test]
    fn harbour_loop() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let mut fx = crate::fx::Effects::new();
        fx.set_keys(std::sync::Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(crate::fx::lib::Library::load(332, index.read(332).unwrap()).unwrap());
        let eye = Vec3::new(10.0, -6.0, 20.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, -6.0, 20.0), Vec3::NEG_Y);
        let f = crate::fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view, view_rot: view, eye, draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        fx.tick(&f, &crate::fx::NoActors);
        let plays: Vec<(u32, i32)> = fx.sound_cmds.iter().filter_map(|c| match c {
            SoundCmd::Play { se, vol, .. } => Some((*se, *vol)),
            _ => None,
        }).collect();
        let sea = plays.iter().find(|p| p.0 == 2009).copied();
        assert!(sea.is_some_and(|(_, v)| (119..=121).contains(&v)), "{plays:?}");
    }

    /// East Ronfaure (DAT 201): f_ro/effe/sep1 skw1..3 loop along their own Sph chains (op 0x6b names kaw1 / kaw2 /
    /// kaw3): in the gate stream (kaw3) only se 2028 plays, at full volume; on the east river (kaw2) only se 2004.
    #[test]
    fn stream_chains() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let plays = |eye: Vec3| {
            let mut fx = crate::fx::Effects::new();
            fx.set_keys(std::sync::Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
            fx.set_global(crate::fx::lib::Library::load(0, index.read(0).unwrap()).unwrap());
            fx.load_zone(crate::fx::lib::Library::load(201, index.read(201).unwrap()).unwrap());
            let view = glam::camera::rh::view::look_at_mat4(eye, eye + Vec3::X, Vec3::NEG_Y);
            let f = crate::fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view, view_rot: view, eye, draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
            fx.tick(&f, &crate::fx::NoActors);
            fx.sound_cmds
                .iter()
                .filter_map(|c| match c {
                    SoundCmd::Play { se, vol, .. } if matches!(se, 2004 | 2028 | 2058) => Some((*se, *vol)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(plays(Vec3::new(100.0, -61.0, 203.6)), [(2028, 127)]);
        assert_eq!(plays(Vec3::new(440.0, -17.0, -300.0)).iter().map(|p| p.0).collect::<Vec<_>>(), [2004]);
    }

    /// West Ronfaure (zone 100, effects DAT 200): weat/suny 0600 = se 1005, 1800 = se 1007, indo/0000 = se 1056.
    #[test]
    fn zone_ambient() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let mut fx = crate::fx::Effects::new();
        fx.load_zone(crate::fx::lib::Library::load(200, index.read(200).unwrap()).unwrap());
        assert_eq!([fx.ambient(720.0), fx.ambient(1200.0), fx.ambient(180.0)], [Some(1005), Some(1007), Some(1007)]);
        fx.set_covered(true);
        assert_eq!(fx.ambient(720.0), Some(1056));
    }
}
