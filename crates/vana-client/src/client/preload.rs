//! Zone preloading (ours, not retail). Near a zone line whose destination the bundled table knows (data/zonelines.txt,
//! from LandSandBoat's zone data: `zonelines_gen.py`), the zone behind it loads on a thread and its renderer streams to
//! the GPU in each frame's spare upload room; the zone-in then takes both instead of loading.

use crate::dat::zone::Region;
use glam::Vec3;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Off for now (user, 2026-11-04): nothing is preloaded while false.
pub const ENABLED: bool = false;

const TABLE: &str = include_str!("../../data/zonelines.txt");
/// Yalms from a zone line's box at which the zone behind it starts loading.
pub const RANGE: f32 = 40.0;
/// A preload is kept while a line to its zone is this near, and dropped past it.
pub const KEEP: f32 = 80.0;

/// The zone and map being loaded, and what the thread brought back (the scene's textures are in the renderer).
pub struct Preload {
    pub zone: u16,
    pub map: u16,
    pub started: std::time::Instant,
    /// The zone-in for it came while the thread ran: its result goes straight to the zone-loaded path.
    pub claimed: bool,
    pub ready: Option<Ready>,
}

pub struct Ready {
    pub data: super::ZoneData,
    /// The renderer streaming the zone's geometry and textures, and the textures' names (the sub-areas want them).
    pub gpu: (crate::render::ZoneRenderer, Vec<String>),
}

/// Each frame: the zone to start loading, if the nearest known zone line within RANGE leads to one not loading yet. A
/// preload goes once no line to its zone is within KEEP, or another zone's line is the nearest in range.
pub fn step(w: &mut super::World) -> Option<(u16, u16)> {
    if !ENABLED {
        return None;
    }
    let zone = w.zone?;
    if !w.in_scene() || w.zoning_since.is_some() || w.loading.is_some() || w.preload.as_ref().is_some_and(|p| p.claimed)
    {
        return None;
    }
    let want = nearest(&w.zone_lines, zone, w.pos, RANGE);
    if let Some(p) = &w.preload {
        let have = (p.zone, p.map);
        match want {
            Some(t) if t == have => return None,
            None if keep(&w.zone_lines, zone, w.pos, have) => return None,
            _ => {
                log::info!("preload: zone {} dropped", p.zone);
                w.preload = None;
            }
        }
    }
    let (to, map) = want?;
    log::info!("preload: zone {to} (map {map}) behind a zone line of {zone}");
    w.preload = Some(Preload { zone: to, map, started: std::time::Instant::now(), claimed: false, ready: None });
    Some((to, map))
}

/// (from zone, the line's 4-byte tag) -> destination zone.
fn table() -> &'static HashMap<(u16, u32), u16> {
    static T: OnceLock<HashMap<(u16, u32), u16>> = OnceLock::new();
    T.get_or_init(|| {
        TABLE
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let from = f.next()?.parse().ok()?;
                let token = f.next()?.as_bytes();
                let to = f.next()?.parse().ok()?;
                (token.len() == 4).then(|| ((from, u32::from_le_bytes(token.try_into().unwrap())), to))
            })
            .collect()
    })
}

/// A zone's scene: LSB's 0x00A carries the zone id as the map number (@0x42), mapped as net::scene_of maps it.
pub fn map_of(zone: u16) -> u16 {
    if zone >= 0x100 { zone + 0x1bc } else { zone }
}

/// Where a zone line leads, when the table knows and it is another zone (a Mog House door leads into the same one).
pub fn destination(zone: u16, tag: u32) -> Option<(u16, u16)> {
    let to = *table().get(&(zone, tag))?;
    (to != zone).then_some((to, map_of(to)))
}

/// Yalms across the ground from `p` to the line's box (0 inside).
fn distance(r: &Region, p: Vec3) -> f32 {
    let d = ((p.x - r.pos[0]).powi(2) + (p.z - r.pos[2]).powi(2)).sqrt();
    (d - r.scale[0].abs().max(r.scale[2].abs()) / 2.0).max(0.0)
}

/// The destination of the nearest known zone line within `range`.
pub fn nearest(lines: &[Region], zone: u16, p: Vec3, range: f32) -> Option<(u16, u16)> {
    lines
        .iter()
        .filter_map(|r| Some((distance(r, p), destination(zone, r.tag())?)))
        .filter(|(d, _)| *d <= range)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, to)| to)
}

/// Whether a line to `to` is still within KEEP.
pub fn keep(lines: &[Region], zone: u16, p: Vec3, to: (u16, u16)) -> bool {
    lines.iter().any(|r| destination(zone, r.tag()) == Some(to) && distance(r, p) <= KEEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(name: &str, pos: [f32; 3]) -> Region {
        Region { pos, scale: [1.0, 8.0, 8.0], name: name.into(), ..Default::default() }
    }

    /// Bastok Markets' lines from LSB's data; its Mog House door leads nowhere new.
    #[test]
    fn destinations_from_the_table() {
        let tag = |s: &str| u32::from_le_bytes(s.as_bytes().try_into().unwrap());
        assert_eq!(destination(235, tag("z6j0")), Some((234, 234)));
        assert_eq!(destination(235, tag("zmra")), None);
        assert_eq!(destination(235, tag("zzzz")), None);
        assert_eq!(map_of(256), 700);
    }

    #[test]
    fn nearest_line_in_range() {
        let lines = [line("z6j0abcd", [0.0, 0.0, 0.0]), line("z6j2abcd", [100.0, 0.0, 0.0])];
        assert_eq!(nearest(&lines, 235, Vec3::new(30.0, 0.0, 0.0), RANGE), Some((234, 234)));
        assert_eq!(nearest(&lines, 235, Vec3::new(70.0, 0.0, 0.0), RANGE), Some((236, 236)));
        assert_eq!(nearest(&lines, 235, Vec3::new(50.0, 0.0, 200.0), RANGE), None);
        assert!(keep(&lines, 235, Vec3::new(70.0, 0.0, 0.0), (234, 234)));
        assert!(!keep(&lines, 235, Vec3::new(200.0, 0.0, 0.0), (234, 234)));
    }
}
