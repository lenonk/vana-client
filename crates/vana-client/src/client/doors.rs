//! Doors (XiDoorActor, FUN_100acba0 / FUN_1008c6c0): a door NPC (0x00E SubKind 2, its id at @0x34) plays its folder's
//! schedulers when its state changes, and its RID box stops blocking while it is open.

use super::World;
use std::collections::HashMap;

/// A door's state in force (+0x170) by entity.
#[derive(Default)]
pub struct Doors {
    pub state: HashMap<u32, u8>,
    /// +0x174: states event ops 0x4c / 0x4d / 0x4f / 0x8e / 0x8f set on bound doors; they hold while an event runs.
    pub event: HashMap<u32, u8>,
}

fn opens(s: u8) -> bool {
    matches!(s, 8 | 0x1a | 0x1d | 0x2d)
}

fn four(s: &str) -> u32 {
    u32::from_le_bytes(crate::dat::generator::fourcc(s))
}

/// Per frame: each door entity's wanted state (+0x16c, or +0x174 while an event runs), first sight and changes.
pub fn update(w: &mut World) {
    let in_event = super::event::active(w);
    if !in_event {
        w.doors.event.clear();
    }
    let doors: Vec<(u32, u32, u8)> = w.entities.iter().filter(|(_, e)| e.door != 0).map(|(id, e)| (*id, e.door, w.doors.event.get(id).copied().filter(|_| in_event).unwrap_or(e.motion.status))).collect();
    w.doors.state.retain(|id, _| w.entities.contains_key(id));
    for (id, door, want) in doors {
        // FUN_100acba0: doors with a group play open / clos, the others (effects only) evst / eved.
        let grouped = w.renderer.as_ref().is_some_and(|r| r.door_parts.contains_key(&door));
        let name = |grouped_name: &str, plain: &str| four(if grouped { grouped_name } else { plain });
        match w.doors.state.get(&id).copied() {
            None => {
                // FUN_1008f740 at the first model init: the instant state, or the animated open when it is missing.
                w.doors.state.insert(id, want);
                if opens(want) {
                    if !w.effects.door_scheduler(id, door, name("into", "ints")) {
                        w.effects.door_scheduler(id, door, name("open", "evst"));
                    }
                    if want != 0x2d {
                        set_solid(w, door, false);
                    }
                } else {
                    w.effects.door_scheduler(id, door, name("intc", "inte"));
                    set_solid(w, door, true);
                }
            }
            Some(s) if s != want => {
                w.doors.state.insert(id, want);
                // FUN_1008c6c0: 8 / 0x1a / 0x1d open and let through, 0x2d opens but stays solid, 9 / 0x1b / 0x1e /
                // 0x2e close; the box changes as the swing starts.
                match want {
                    8 | 0x1a | 0x1d => {
                        w.effects.door_scheduler(id, door, name("open", "evst"));
                        set_solid(w, door, false);
                    }
                    0x2d => {
                        w.effects.door_scheduler(id, door, name("open", "evst"));
                    }
                    9 | 0x1b | 0x1e | 0x2e => {
                        w.effects.door_scheduler(id, door, name("clos", "eved"));
                        set_solid(w, door, true);
                    }
                    _ => {}
                }
            }
            Some(_) => {}
        }
    }
}

/// FUN_10176dd0(1): a new ground (a sub-area came or went) takes the doors' states; open doors stop blocking.
pub fn reapply(w: &World) {
    for (id, s) in &w.doors.state {
        if let Some(e) = w.entities.get(id)
            && e.door != 0
            && matches!(s, 8 | 0x1a | 0x1d)
        {
            set_solid(w, e.door, false);
        }
    }
}

/// FUN_1017ac40 on the ground in use and, while a sub-area is in, the zone's own kept ground.
fn set_solid(w: &World, door: u32, solid: bool) {
    for g in w.ground.iter().chain(w.sub.home.iter()) {
        g.set_door(door, solid);
    }
}

/// The frame's part moves (scheduler ops 0x0c / 0x0d) onto the door's parts.
pub fn moves(w: &mut World, gpu: &crate::render::Gpu) {
    let moves = std::mem::take(&mut w.effects.door_moves);
    let Some(r) = w.renderer.as_mut() else { return };
    for m in moves {
        let Some(door) = w.entities.get(&m.actor).map(|e| e.door).filter(|d| *d != 0) else { continue };
        if let Some(&slot) = r.door_parts.get(&door).and_then(|p| p.get(m.part as usize)) {
            r.move_part(gpu, slot, m.rotate, m.to, m.ticks);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Southern San d'Oria's `_6e1` (Count's Manor): two leaves, a 12-triangle box, open / clos in t_sa/door/_6e1.
    #[test]
    fn count_manor_door() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(keys) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).map_err(anyhow::Error::from).and_then(|d| crate::dat::zone::Keys::from_ffximain(&d)) else { return };
        let file = index.read(crate::dat::zone::file_id(230)).unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &file).unwrap();
        let id = super::four("_6e1");
        let geo = crate::render::ZoneGeometry::build(&scene);
        assert_eq!(geo.door_parts.get(&id).map(Vec::len), Some(2));
        // The target arrow's anchor, part 0's origin: in the doorway (RID box at -275.7, -7.9, 98.0), at mid height.
        let o = geo.door_bases[&id][0].w_axis.truncate();
        assert!((o - glam::Vec3::new(-275.7, -7.9, 98.0)).length() < 2.5, "{o}");
        let c = scene.collision.as_ref().unwrap();
        let boxes = c.objects.iter().filter(|o| o.door == id).count();
        assert_eq!(boxes, 1);
        let mut fx = crate::fx::Effects::new();
        fx.load_zone(crate::fx::lib::Library::load(crate::dat::zone::file_id(230), file).unwrap());
        assert!(fx.door_scheduler(1, id, super::four("open")));
        assert!(fx.door_scheduler(1, id, super::four("clos")));
        assert!(!fx.door_scheduler(1, id, super::four("into")));
    }
}
