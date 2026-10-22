//! Zone lighting: type 0x2f environment chunks (0xac bytes) in a zone DAT's `weat/<weather>` folder,
//! named by Vana'diel time `HHMM` (FUN_100711d0), interpolated between neighbouring keys (FUN_10183320).

use anyhow::Result;
use glam::Vec3;
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

    /// FUN_10182470: prev + (next - prev) * t for every field.
    pub fn lerp(prev: &Env, next: &Env, t: f32) -> Env {
        let lb = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
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
        out
    }
}

/// Weather tag -> keys (minute of day, env), from the zone DAT's `weat` folder.
pub struct ZoneWeather {
    pub keys: BTreeMap<String, Vec<(u32, Env)>>,
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
        let mut keys: BTreeMap<String, Vec<(u32, Env)>> = BTreeMap::new();
        let mut stack: Vec<String> = Vec::new();
        for ch in super::chunks(file)? {
            match ch.kind {
                0x01 => stack.push(ch.name()),
                0x00 => {
                    stack.pop();
                }
                0x2f => {
                    // Only direct children of weat/<tag>, not sub-folders such as "indo".
                    let n = stack.len();
                    if n >= 2 && stack[n - 2] == "weat" {
                        if let (Some(m), Some(e)) = (key_minutes(&ch.name()), Env::parse(ch.data)) {
                            keys.entry(stack[n - 1].clone()).or_default().push((m, e));
                        }
                    }
                }
                _ => {}
            }
        }
        for v in keys.values_mut() {
            v.sort_by_key(|k| k.0);
        }
        Ok(Self { keys })
    }

    /// Env at `minute` of the day for `weather`, falling back to "suny" (FUN_10183780).
    pub fn at(&self, weather: &str, minute: f32) -> Option<Env> {
        let keys = self.keys.get(weather).or_else(|| self.keys.get("suny"))?;
        if keys.is_empty() {
            return None;
        }
        let day = 24.0 * 60.0;
        let now = minute.rem_euclid(day);
        // Previous key: latest at or before now (wrapping); next: the following one.
        let i = keys.iter().rposition(|k| k.0 as f32 <= now).unwrap_or(keys.len() - 1);
        let j = (i + 1) % keys.len();
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

impl Lighting {
    pub fn from_env(env: &Env, minute: f32) -> Self {
        // FUN_10180700: a = time / (216000 * 24) * 2pi; moon (sin a, cos a, 0), sun at a + pi.
        let a = minute / (24.0 * 60.0) * std::f32::consts::TAU;
        let moon = Vec3::new(a.sin(), a.cos(), 0.0);
        let sun = -moon;
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
    }
}
