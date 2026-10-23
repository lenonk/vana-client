//! Sub-areas: building interiors loaded over the zone while the player is near them (zone renderer
//! DAT_10631f24, FUN_10178a20 / FUN_101787c0). Triggers are the zone RID's 'm' / 'M' records; the interior's DAT
//! is the sub-area id's zone file, its placements already in the zone's world space.

use super::ground::Ground;
use crate::dat::collision::Collision;
use crate::dat::weather::ZoneWeather;
use crate::dat::zone::{self, Region};
use std::sync::Arc;

#[derive(Default)]
pub struct SubArea {
    /// The zone's RID records.
    pub regions: Vec<Region>,
    /// +0x17c: the loaded sub-area.
    pub current: Option<u32>,
    /// +0x48: the one loading, and whether an event asked (FUN_10178900 from an event sends no 0x0F2).
    pub loading: Option<(u32, bool)>,
    /// Asked for by 0x00A SubMapNumber, event op 0x75 sub 0, or 0 by a mode-7 position.
    pub asked: Option<(u32, bool)>,
    /// The zone's collision and point-light keys, which a sub-area's merge into.
    pub zone_collision: Option<Arc<Collision>>,
    pub zone_keys: Vec<u32>,
    /// The zone's own ground while the merged one is in use.
    pub home: Option<Ground>,
    /// The sub-area's area environments (its 'ev10').
    pub weather: Option<ZoneWeather>,
}

/// What FUN_10178a20 decides.
pub enum Step {
    Load(u32, bool),
    Unload,
}

impl SubArea {
    pub fn new(regions: Vec<Region>, collision: Option<Collision>, keys: Vec<u32>) -> Self {
        SubArea { regions, zone_collision: collision.map(Arc::new), zone_keys: keys, ..Default::default() }
    }

    /// FUN_10178a20 (slot 1): nothing while a load runs or for the loaded id; 0 unloads; another id loads when a
    /// trigger declares it.
    pub fn request(&self, id: u32, from_event: bool) -> Option<Step> {
        if self.loading.is_some() || self.current == Some(id) {
            return None;
        }
        if id == 0 {
            return self.current.is_some().then_some(Step::Unload);
        }
        zone::declares_sub_area(&self.regions, id).then_some(Step::Load(id, from_event))
    }

    /// This frame's request: one asked for, else the trigger under the player (FUN_10014390 -> FUN_10181540).
    pub fn next(&mut self, feet: [f32; 3]) -> Option<Step> {
        if let Some((id, ev)) = self.asked.take() {
            return self.request(id, ev);
        }
        zone::sub_area_at(&self.regions, feet).and_then(|id| self.request(id, false))
    }
}

/// A sub-area off the loader thread.
pub struct Loaded {
    pub geometry: crate::render::ZoneGeometry,
    pub textures: Vec<zone::Texture>,
    pub ground: Ground,
    /// The zone's light keys with the sub-area's after them.
    pub keys: Vec<u32>,
    pub fx: Option<crate::fx::lib::Library>,
    pub weather: Option<ZoneWeather>,
}

/// FUN_10178d90 and the chunk handlers: the file of `id` (the zone file id mapping, asm 1017894d), its collision
/// merged into the zone's (FUN_10167be0) with the stand-ins for `id` dropped, its light refs remapped into the
/// zone's table (FUN_1017a5e0).
pub fn load(index: &crate::dat::DatIndex, keys: &zone::Keys, id: u32, zone_collision: Option<Arc<Collision>>, zone_keys: &[u32]) -> anyhow::Result<Loaded> {
    let file_id = zone::file_id(id);
    let file = index.read(file_id)?;
    let mut scene = zone::load_scene(keys, &file)?;
    let mut all = zone_keys.to_vec();
    for &k in &scene.light_keys {
        if k != 0 && !all.contains(&k) {
            all.push(k);
        }
    }
    let ground = match (zone_collision.as_deref(), scene.collision.as_ref()) {
        (Some(z), Some(s)) => Ground::from_collision(&z.merged(s, id), &all),
        (Some(z), None) => Ground::from_collision(&z.merged(&Collision::empty(z.grid), id), &all),
        (None, Some(s)) => Ground::from_collision(s, &all),
        (None, None) => Ground::build(&scene),
    };
    let geometry = crate::render::ZoneGeometry::build_keyed(&scene, &all);
    let weather = ZoneWeather::parse(&file).map_err(|e| log::warn!("sub-area {id} weather: {e:#}")).ok();
    let fx = crate::fx::lib::Library::load(file_id, file).map_err(|e| log::warn!("effects: sub-area {id}: {e:#}")).ok().map(|mut l| {
        l.prewarm(keys);
        l
    });
    Ok(Loaded { geometry, textures: std::mem::take(&mut scene.textures), ground, keys: all, fx, weather })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Southern San d'Oria: trigger m6eb outside door _6e1 loads 335 (file 435), whose stand-in is house_05_in.
    #[test]
    fn southern_sandoria_house() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(keys) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).map_err(anyhow::Error::from).and_then(|d| zone::Keys::from_ffximain(&d)) else { return };
        let scene = zone::load_scene(&keys, &index.read(zone::file_id(230)).unwrap()).unwrap();
        let m = scene.regions.iter().find(|r| r.name.starts_with("m6eb")).unwrap();
        assert_eq!((m.class, m.param), (0, 335));
        assert_eq!(zone::sub_area_at(&scene.regions, [-270.95, -6.5, 97.98]), Some(335));
        assert_eq!(zone::sub_area_at(&scene.regions, [-250.0, -6.5, 97.98]), None);
        assert!(scene.placements.iter().any(|p| p.name == "house_05_in" && p.link == 335));
        let zc = scene.collision.as_ref().unwrap();
        let stand_ins = zc.object.iter().filter(|&&o| zc.objects[o as usize].link == 335).count();
        assert!(stand_ins > 0);
        let l = load(&index, &keys, 335, Some(Arc::new(Collision::empty(zc.grid))), &scene.light_keys).unwrap();
        assert!(l.textures.len() > 10);
        let sub = zone::load_scene(&keys, &index.read(zone::file_id(335)).unwrap()).unwrap();
        let merged = zc.merged(sub.collision.as_ref().unwrap(), 335);
        assert_eq!(merged.tris.len(), zc.tris.len() - stand_ins + sub.collision.as_ref().unwrap().tris.len());
    }
}
