//! Zone lighting: type 0x2f environment chunks (0xac bytes) in a zone DAT's `weat/<weather>` folder,
//! named by Vana'diel time `HHMM` (FUN_100711d0), interpolated between neighbouring keys (FUN_10183320).

use anyhow::Result;
use glam::{Quat, Vec3};
use std::collections::BTreeMap;

/// One light group (actor group at 0x0c, background group at 0x2c; FUN_10181170).
#[derive(Clone, Copy, Debug, Default)]
pub struct Group {
    /// Sun diffuse (or the light colour when `flags & 1`).
    pub sun: [u8; 4],
    /// Moon diffuse; with `flags & 1` these bytes are a signed light direction instead.
    pub moon: [u8; 4],
    pub ambient: [u8; 4],
    pub fog: [u8; 4],
    pub fog_end: f32,
    pub fog_start: f32,
    /// Scales the directional light colour (FUN_10181c80 via +0x24 / +0x44).
    pub light_scale: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Env {
    pub flags: u8,
    pub actor: Group,
    pub bg: Group,
    /// Back buffer clear colour (FUN_1003cd20 -> cam+0x308).
    pub clear: [u8; 4],
    pub unk50: f32,
    pub unk54: f32,
    /// Scenery draw distance before multipliers (FUN_101848d0).
    pub draw_dist: f32,
    pub bytes5c: [u8; 3],
    pub unk68: f32,
    /// Sky dome ring colours, horizon to zenith, and their elevations as fractions of 90 degrees.
    pub sky: [[u8; 4]; 8],
    pub stops: [f32; 8],
}

fn f(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn c(b: &[u8], o: usize) -> [u8; 4] {
    b[o..o + 4].try_into().unwrap()
}

impl Env {
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < 0xac {
            return None;
        }
        let g = |o: usize| Group { sun: c(b, o), moon: c(b, o + 4), ambient: c(b, o + 8), fog: c(b, o + 12), fog_end: f(b, o + 16), fog_start: f(b, o + 20), light_scale: f(b, o + 24) };
        let mut sky = [[0u8; 4]; 8];
        let mut stops = [0f32; 8];
        for i in 0..8 {
            sky[i] = c(b, 0x6c + 4 * i);
            stops[i] = f(b, 0x8c + 4 * i);
        }
        Some(Self {
            flags: b[0],
            actor: g(0x0c),
            bg: g(0x2c),
            clear: c(b, 0x4c),
            unk50: f(b, 0x50),
            unk54: f(b, 0x54),
            draw_dist: f(b, 0x58),
            bytes5c: [b[0x5c], b[0x5d], b[0x5e]],
            unk68: f(b, 0x68),
            sky,
            stops,
        })
    }

    /// Sky dome is drawn when byte 0x5e (ring segments) > 2 (FUN_1003cd20).
    pub fn has_dome(&self) -> bool {
        self.bytes5c[2] > 2
    }

    /// FUN_10189490: prev at t = 0, next at t = 1; bytes prev + ftol((next - prev) x t) (chopped toward 0), floats
    /// linear; with flags bit 0 the signed light directions turn along the great circle (FUN_10033360).
    pub fn lerp(prev: &Env, next: &Env, t: f32) -> Env {
        let lb = |a: u8, b: u8| (a as i32 + ((b as i32 - a as i32) as f32 * t) as i32) as u8;
        let lc = |a: [u8; 4], b: [u8; 4]| [lb(a[0], b[0]), lb(a[1], b[1]), lb(a[2], b[2]), lb(a[3], b[3])];
        let lf = |a: f32, b: f32| a + (b - a) * t;
        let lg = |a: &Group, b: &Group| Group {
            sun: lc(a.sun, b.sun),
            moon: lc(a.moon, b.moon),
            ambient: lc(a.ambient, b.ambient),
            fog: lc(a.fog, b.fog),
            fog_end: lf(a.fog_end, b.fog_end),
            fog_start: lf(a.fog_start, b.fog_start),
            light_scale: lf(a.light_scale, b.light_scale),
        };
        let mut out = *prev;
        out.flags = prev.flags | next.flags;
        out.actor = lg(&prev.actor, &next.actor);
        out.bg = lg(&prev.bg, &next.bg);
        out.clear = lc(prev.clear, next.clear);
        out.unk50 = lf(prev.unk50, next.unk50);
        out.unk54 = lf(prev.unk54, next.unk54);
        out.draw_dist = lf(prev.draw_dist, next.draw_dist);
        out.bytes5c = [lb(prev.bytes5c[0], next.bytes5c[0]), lb(prev.bytes5c[1], next.bytes5c[1]), lb(prev.bytes5c[2], next.bytes5c[2])];
        out.unk68 = lf(prev.unk68, next.unk68);
        for i in 0..8 {
            out.sky[i] = lc(prev.sky[i], next.sky[i]);
            out.stops[i] = lf(prev.stops[i], next.stops[i]);
        }
        if out.flags & 1 != 0 {
            out.bg.moon = turn_dir(prev.bg.moon, next.bg.moon, out.bg.moon[3], t);
            out.actor.moon = turn_dir(prev.actor.moon, next.actor.moon, out.actor.moon[3], t);
        }
        out
    }
}

/// Flags bit 0: the signed-byte light direction turned from prev toward next by t of the angle between them
/// (FUN_10033360 / FUN_10032fe0 / FUN_10028200), x 127, chopped; byte 3 keeps the byte lerp. Parallel or opposite
/// directions do not turn *(inf: the sense toward next)*.
fn turn_dir(a: [u8; 4], b: [u8; 4], w: u8, t: f32) -> [u8; 4] {
    let v = |c: [u8; 4]| Vec3::new(c[0] as i8 as f32, c[1] as i8 as f32, c[2] as i8 as f32).normalize_or_zero();
    let (p, n) = (v(a), v(b));
    let axis = p.cross(n).normalize_or_zero();
    let r = if axis == Vec3::ZERO { p } else { Quat::from_axis_angle(axis, p.dot(n).clamp(-1.0, 1.0).acos() * t) * p };
    let q = |x: f32| (x * 127.0) as i32 as i8 as u8;
    [q(r.x), q(r.y), q(r.z), w]
}

type Keys = BTreeMap<String, Vec<(u32, Env)>>;

/// Weather tag -> keys (minute of day, env), from the zone DAT's `weat` folder; the area environments' own keys
/// by area 4cc (`<zone>/<id>/<tag>` or `<zone>/<id>/weat/<tag>`, FUN_10180380 / FUN_10183780).
pub struct ZoneWeather {
    pub keys: Keys,
    pub areas: std::collections::HashMap<u32, ZoneWeather>,
}

fn key_minutes(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if b.len() != 4 || !b.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let d = |i: usize| (b[i] - b'0') as u32;
    Some((d(0) * 10 + d(1)) * 60 + d(2) * 10 + d(3))
}

impl ZoneWeather {
    pub fn parse(file: &[u8]) -> Result<Self> {
        let mut keys: Keys = BTreeMap::new();
        let mut areas: std::collections::HashMap<u32, Keys> = std::collections::HashMap::new();
        let mut stack: Vec<(String, u32)> = Vec::new();
        for ch in super::chunks(file)? {
            match ch.kind {
                0x01 => stack.push((ch.name(), u32::from_le_bytes(ch.name))),
                0x00 => {
                    stack.pop();
                }
                0x2f => {
                    let n = stack.len();
                    let (Some(m), Some(e)) = (key_minutes(&ch.name()), Env::parse(ch.data)) else { continue };
                    let tag = || stack[n - 1].0.clone();
                    if n == 4 && stack[2].0 == "weat" && stack[1].0 != "weat" {
                        areas.entry(stack[1].1).or_default().entry(tag()).or_default().push((m, e));
                    } else if n >= 2 && stack[n - 2].0 == "weat" {
                        // Only direct children of weat/<tag>, not sub-folders such as "indo".
                        keys.entry(tag()).or_default().push((m, e));
                    } else if n == 3 && WEATHER_TAGS.contains(&stack[2].0.as_str()) {
                        areas.entry(stack[1].1).or_default().entry(tag()).or_default().push((m, e));
                    }
                }
                _ => {}
            }
        }
        let sorted = |mut k: Keys| {
            for v in k.values_mut() {
                v.sort_by_key(|k| k.0);
            }
            k
        };
        let areas = areas.into_iter().map(|(id, k)| (id, Self { keys: sorted(k), areas: Default::default() })).collect();
        Ok(Self { keys: sorted(keys), areas })
    }

    /// Env at `minute` of the day for `weather`, falling back to "suny" (FUN_10183780).
    pub fn at(&self, weather: &str, minute: f32) -> Option<Env> {
        let keys = self.keys.get(weather).or_else(|| self.keys.get("suny"))?;
        if keys.is_empty() {
            return None;
        }
        let day = 24.0 * 60.0;
        let now = minute.rem_euclid(day);
        // FUN_10189e30: the latest key at or before now, the first in file order on equal times (keys are stably
        // sorted); FUN_1018a0f0: the earliest key strictly after now, else the day's first.
        let i = keys.iter().rposition(|k| k.0 as f32 <= now).unwrap_or(keys.len() - 1);
        let i = keys.iter().position(|k| k.0 == keys[i].0).unwrap_or(i);
        let j = keys.iter().position(|k| k.0 as f32 > now).unwrap_or(0);
        let (p, n) = (keys[i].0 as f32, keys[j].0 as f32);
        let span = (n - p).rem_euclid(day);
        let into = (now - p).rem_euclid(day);
        let t = if span == 0.0 { 0.0 } else { into / span };
        Some(Env::lerp(&keys[i].1, &keys[j].1, t))
    }
}

impl Env {
    /// FUN_1017fee0 / FUN_1017fef0 + FUN_1017ff00 (area_light.md): c = actor sun + ambient, c2 = actor moon +
    /// ambient, bytes / 255; c2 when flags bit 0 is clear (FUN_10181af0) and it is the brighter (a tie keeps c).
    pub fn area_light(&self) -> Vec3 {
        let c = rgb(self.actor.sun) + rgb(self.actor.ambient);
        let c2 = rgb(self.actor.moon) + rgb(self.actor.ambient);
        if self.flags & 1 == 0 && c2.dot(c2) > c.dot(c) { c2 } else { c }
    }
}

fn rgb(c: [u8; 4]) -> Vec3 {
    Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) / 255.0
}

/// The D3D8 fixed-function state the client derives from an env (FUN_10180700, FUN_10181c80,
/// FUN_10181de0, FUN_10182100). Directions are the way the light travels (world y is down).
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub dir: Vec3,
    pub diffuse: Vec3,
    pub ambient: Vec3,
    pub actor_dir: Vec3,
    pub actor_diffuse: Vec3,
    pub actor_ambient: Vec3,
    pub fog: Vec3,
    pub fog_start: f32,
    pub fog_end: f32,
    pub clear: Vec3,
    pub sky: Option<([Vec3; 8], [f32; 8])>,
    /// The effect tint (emission ops 0x8f / 0x90, update op 0x69): Env::area_light.
    pub area: Vec3,
}

/// FUN_10187720's stored 2 pi and pi (0x10329d2c, 0x10329d30): the switch falls at 05:59:59.4 and 18:00:00.6.
const TWO_PI: f32 = 6.283;
const PI: f32 = 3.1415;

/// FUN_10187720: the sun and moon light directions (the way the light travels, +y down) at `minute`.
pub fn sun_moon(minute: f32) -> (Vec3, Vec3) {
    let a = minute / 1440.0 * TWO_PI;
    let moon = Vec3::new(a.sin(), a.cos(), 0.0);
    let mut b = a + PI;
    if b > PI {
        b -= TWO_PI;
    }
    if b < -PI {
        b += TWO_PI;
    }
    (Vec3::new(b.sin(), b.cos(), 0.0), moon)
}

/// One eased light (E+0x14c / +0x16c, E+0x17c / +0x19c): direction and colour (bytes / 255, before the scale).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EasedLight {
    pub dir: Vec3,
    pub color: Vec3,
}

impl EasedLight {
    /// One retail frame (0x10187ecc): the direction 0.1 of the way, renormalised; while the remaining turn is large
    /// (|dot(target - old, new)| >= 0.5) the colour fades x 0.7, else it eases 0.1 toward the target.
    pub fn step(&mut self, dir: Vec3, color: Vec3) {
        let delta = dir - self.dir;
        let d = self.dir + delta * 0.1;
        let len = d.length();
        self.dir = if len != 0.0 { d / len } else { d * 1e7 };
        if delta.dot(self.dir).abs() >= 0.5 {
            self.color *= 0.7;
        } else {
            self.color += (color - self.color) * 0.1;
        }
    }
}

/// E+0x148..0x1a8 of one environment object (the zone, or an area): the eased background and actor lights.
#[derive(Clone, Copy, Debug, Default)]
pub struct LightEase {
    init: bool,
    pub bg: EasedLight,
    pub actor: EasedLight,
}

impl LightEase {
    /// FUN_10187030 at a zone load: the next step copies the targets.
    pub fn reset(&mut self) {
        self.init = false;
    }

    /// FUN_10187720 once a retail frame in game: the sun by day, the moon by night (sun.y < 0); flags-bit-0
    /// environments keep their data directions (no ease).
    pub fn step(&mut self, env: &Env, minute: f32) {
        if env.flags & 1 != 0 {
            return;
        }
        let (sun, moon) = sun_moon(minute);
        let (d, bg, actor) = if sun.y < 0.0 { (moon, rgb(env.bg.moon), rgb(env.actor.moon)) } else { (sun, rgb(env.bg.sun), rgb(env.actor.sun)) };
        if !self.init {
            self.init = true;
            self.bg = EasedLight { dir: d, color: bg };
            self.actor = EasedLight { dir: d, color: actor };
            return;
        }
        self.bg.step(d, bg);
        self.actor.step(d, actor);
    }
}

impl Lighting {
    /// In game (FUN_10188ca0): the eased direction and colour times the environment's light scale (not eased);
    /// ambient and fog as the keys give them.
    pub fn from_env_eased(env: &Env, minute: f32, ease: &LightEase) -> Self {
        let mut l = Self::from_env(env, minute);
        if env.flags & 1 == 0 && ease.init {
            l.dir = ease.bg.dir;
            l.diffuse = ease.bg.color * env.bg.light_scale;
            l.actor_dir = ease.actor.dir;
            l.actor_diffuse = ease.actor.color * env.actor.light_scale;
        }
        l
    }

    pub fn from_env(env: &Env, minute: f32) -> Self {
        let (sun, moon) = sun_moon(minute);
        let night = sun.y < 0.0;
        let (dir, diffuse, actor_dir, actor_diffuse) = if env.flags & 1 != 0 {
            let d = |c: [u8; 4]| Vec3::new(c[0] as i8 as f32, c[1] as i8 as f32, c[2] as i8 as f32).normalize_or(Vec3::Y);
            (d(env.bg.moon), rgb(env.bg.sun), d(env.actor.moon), rgb(env.actor.sun))
        } else if night {
            (moon, rgb(env.bg.moon), moon, rgb(env.actor.moon))
        } else {
            (sun, rgb(env.bg.sun), sun, rgb(env.actor.sun))
        };
        let mut end = env.bg.fog_end;
        let start = env.bg.fog_start;
        if end <= start {
            end = start + 1.0;
        }
        let sky = env.has_dome().then(|| (env.sky.map(rgb), env.stops));
        Self {
            dir,
            diffuse: diffuse * env.bg.light_scale,
            ambient: rgb(env.bg.ambient),
            actor_dir,
            actor_diffuse: actor_diffuse * env.actor.light_scale,
            actor_ambient: rgb(env.actor.ambient),
            fog: rgb(env.bg.fog),
            fog_start: start,
            fog_end: end,
            clear: rgb(env.clear),
            sky,
            area: env.area_light(),
        }
    }
}

/// Weather tags by the server's weather number (FFXiMain table 0x1036bab8, FUN_10184ac0); the
/// order is LSB's xi::Weather.
pub const WEATHER_TAGS: [&str; 20] =
    ["fine", "suny", "clod", "mist", "dryw", "heat", "rain", "squl", "dust", "sand", "wind", "stom", "snow", "bliz", "thdr", "bolt", "aura", "ligt", "fogd", "dark"];

pub fn weather_tag(number: u16) -> &'static str {
    WEATHER_TAGS.get(number as usize).copied().unwrap_or("suny")
}

/// Minute of the Vana'diel day from 0x00A's GameTime (Earth seconds since the Vana'diel epoch) plus
/// Earth seconds since: one Vana'diel minute is 2.4 Earth seconds (LSB vanadiel_clock).
pub fn vana_minute(game_time: u32, since_secs: f32) -> f32 {
    ((game_time as f64 + since_secs as f64) / 2.4).rem_euclid(24.0 * 60.0) as f32
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{EasedLight, LightEase, ZoneWeather, sun_moon};

    /// FUN_10187720's step: 0.1 of the way; a large remaining turn fades the colour x 0.7.
    #[test]
    fn eased_light_steps() {
        let mut l = EasedLight { dir: Vec3::X, color: Vec3::ZERO };
        l.step(Vec3::X, Vec3::ONE);
        assert!((l.color.x - 0.1).abs() < 1e-6);
        for _ in 0..9 {
            l.step(Vec3::X, Vec3::ONE);
        }
        assert!((l.color.x - 0.65132).abs() < 1e-4, "{}", l.color);
        let mut l = EasedLight { dir: Vec3::new(1.0, 0.01, 0.0).normalize(), color: Vec3::splat(0.8) };
        l.step(Vec3::new(-1.0, 0.01, 0.0).normalize(), Vec3::ZERO);
        assert!((l.color.x - 0.56).abs() < 1e-5 && (l.dir - Vec3::new(0.99992, 0.0125, 0.0)).length() < 1e-4, "{l:?}");
    }

    /// The stored constants put the switch just before 06:00 and just after 18:00.
    #[test]
    fn sun_moon_switch() {
        assert!(sun_moon(359.98).0.y < 0.0 && sun_moon(360.0).0.y >= 0.0);
        assert!(sun_moon(1080.0).0.y >= 0.0 && sun_moon(1080.02).0.y < 0.0);
    }

    fn weather(file: u32) -> Option<ZoneWeather> {
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        ZoneWeather::parse(&index.read(file).ok()?).ok()
    }

    /// West Ronfaure (DAT 200) suny at 30 Hz (1/72 Vana minute a step): no one-frame light at dawn or dusk.
    #[test]
    fn dawn_dusk_ease() {
        let Some(w) = weather(200) else { return };
        let run = |start: f32, n: usize| -> Vec<(Vec3, Vec3)> {
            let mut e = LightEase::default();
            (0..n)
                .map(|i| {
                    let m = start + i as f32 / 72.0;
                    e.step(&w.at("suny", m).unwrap(), m);
                    (e.bg.dir, e.bg.color)
                })
                .collect()
        };
        let dawn = run(359.0, 241);
        assert!(sun_moon(359.0 + 71.0 / 72.0).0.y < 0.0 && sun_moon(360.0).0.y >= 0.0);
        assert!(dawn[73..=135].iter().all(|x| x.1.max_element() < 0.005));
        let near = |a: Vec3, b: Vec3, e: f32| (a - b).abs().max_element() <= e;
        assert!(near(dawn[180].1, Vec3::new(0.6655, 0.6346, 0.4024), 0.01), "{}", dawn[180].1);
        assert!(near(dawn[240].1, Vec3::new(0.6745, 0.6431, 0.4078), 0.003), "{}", dawn[240].1);
        assert!(near(dawn[240].0, Vec3::new(-1.0, 0.0097, 0.0), 0.002), "{}", dawn[240].0);
        let wall = |x: &(Vec3, Vec3)| x.1.max_element() * 1.5 * Vec3::X.dot(-x.0).max(0.0);
        let jump = dawn.windows(2).map(|p| (wall(&p[1]) - wall(&p[0])).abs()).fold(0.0, f32::max);
        assert!(jump < 0.1, "{jump}");
        let dusk = run(1079.0, 91);
        assert!((dusk[72].1.max_element() - 0.8).abs() < 0.005, "{}", dusk[72].1);
        assert!((dusk[73].1.max_element() - 0.5603).abs() < 0.001, "{}", dusk[73].1);
        assert!((dusk[75].1.max_element() - 0.2745).abs() < 0.002, "{}", dusk[75].1);
        assert!(dusk[90].1.max_element() < 0.005);
    }

    /// FUN_10189490: bytes chopped; flags-bit-0 directions turned (Tavnazian Safehold, DAT 126), not lerped as u8.
    #[test]
    fn env_lerp_retail() {
        if let Some(w) = weather(200) {
            let e = w.at("suny", 330.0).unwrap();
            assert_eq!((&e.bg.sun[..3], &e.bg.moon[..3], &e.bg.fog[..3]), (&[113, 109, 77][..], &[53, 53, 53][..], &[78, 71, 59][..]));
            assert!((e.bg.fog_end - 371.25).abs() < 0.01, "{}", e.bg.fog_end);
        }
        if let Some(w) = weather(126) {
            let d = |m: f32| w.at("suny", m).unwrap().bg.moon.map(|b| b as i8);
            for m in [535.0, 537.0] {
                let v = d(m);
                assert!((v[0] + 16).abs() <= 1 && (v[1] - 125).abs() <= 1 && v[2].abs() <= 1, "{m}: {v:?}");
            }
            assert_eq!(&d(720.0)[..3], &[0, 126, 0]);
        }
    }

    /// FUN_10189e30 keeps the first of two equal key times (Qufim Island, DAT 226, `ligt` 1200 x 2).
    #[test]
    fn duplicate_keys() {
        let Some(w) = weather(226) else { return };
        let e = w.at("ligt", 780.0).unwrap();
        assert_eq!(&e.bg.ambient[..3], &[163, 162, 165]);
        assert!(e.bg.fog_start == 0.0 && (e.bg.fog_end - 450.5).abs() < 0.1, "{} {}", e.bg.fog_start, e.bg.fog_end);
        let (a, b) = (w.at("ligt", 719.99).unwrap().bg.ambient, w.at("ligt", 720.0).unwrap().bg.ambient);
        assert!(a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 1), "{a:?} {b:?}");
    }

    /// area_light.md worked numbers: zone 232 street, fine: 12:00 sun + ambient, 00:00 moon + ambient.
    #[test]
    fn port_sandoria_area_light() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let w = super::ZoneWeather::parse(&index.read(332).unwrap()).unwrap();
        let close = |a: Vec3, b: Vec3| (a - b).abs().max_element() < 1e-3;
        let noon = w.at("fine", 720.0).unwrap().area_light();
        assert!(close(noon, Vec3::new(1.6196, 1.6235, 1.5451)), "{noon}");
        let night = w.at("fine", 0.0).unwrap().area_light();
        assert!(close(night, Vec3::new(0.9255, 0.9333, 1.1412)), "{night}");
        // The ev01 area (shops, auction house): flags bit 0, so always sun + ambient.
        let ev01 = &w.areas[&u32::from_le_bytes(*b"ev01")];
        assert!(close(ev01.at("fine", 720.0).unwrap().area_light(), Vec3::new(0.9843, 0.9529, 0.8588)));
        assert!(close(ev01.at("fine", 0.0).unwrap().area_light(), Vec3::new(0.9020, 0.8706, 0.7765)));
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &index.read(332).unwrap()).unwrap();
        let objs = &scene.collision.unwrap().objects;
        // 159 ev01 records in table [5]; 76 of them are reached from the collision grid.
        assert_eq!(objs.iter().filter(|o| o.area == u32::from_le_bytes(*b"ev01")).count(), 76);
        // point_lights.md 1.3: collision objects name lamps by MZB list ref, e.g. (pl18, pl19).
        let pl18 = u32::from_le_bytes(*b"pl18");
        assert!(objs.iter().any(|o| o.lights[..2] == [pl18, u32::from_le_bytes(*b"pl19")]));
    }
}
