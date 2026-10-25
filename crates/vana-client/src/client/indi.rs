//! The Geomancer Indi aura (spec ~/ffxi/lab/research/fx_indi_aura_spec.md): a second actor of a PC (kind 0x25 +
//! element) whose model file 52211 + element is a self-contained effect; the client creates, places, scales and
//! deletes it (FUN_1008f740's block, FUN_1009ad20, FUN_100c59b0).

use super::{Source, World};

/// The aura actor's id: its owner's with this bit (no entity, target, plate or bump).
pub const AURA: u32 = 0x2000_0000;
/// FUN_100d2f60: kind + 0xcbce, kind = 0x25 + element.
const FIRST_FILE: u32 = 52211;

/// s2c 0x00D u8 @0x42 / 0x037 u8 @0x58: element (+8 enemy-targeted) bits 0..3, size 4..5, on bit 6.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Indi {
    pub element: u8,
    pub size: u8,
    pub on: bool,
}

impl Indi {
    pub fn from_byte(b: u8) -> Self {
        Indi { element: b & 0xf, size: b >> 4 & 3, on: b & 0x40 != 0 }
    }

    pub fn file(self) -> u32 {
        FIRST_FILE + self.element as u32
    }
}

/// Entity +0x130 / +0x134 bits 0 and 17 / +0x202: the wanted value, the shown one, the dying countdown in ticks, and
/// whether the actor's `init` has run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aura {
    pub want: Indi,
    pub shown: Option<Indi>,
    pub dying: Option<f32>,
    pub init_done: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Keep,
    Create(Indi),
    Dead,
    Remove,
}

/// FUN_1009ad20: a new element or size while on drops the shown aura at once (true: remove it); the next frame
/// makes it again.
pub fn set(a: &mut Aura, v: Indi) -> bool {
    let restart = v.on && a.want.on && (v.element, v.size) != (a.want.element, a.want.size) && a.shown.is_some();
    if restart {
        (a.shown, a.dying, a.init_done) = (None, None, false);
    }
    a.want = v;
    restart
}

/// FUN_1008f740's block for one entity: off plays `dead` and removes the actor 90 ticks later; on makes it when the
/// Effects filter allows (`show`), and removes it at once when the filter stops allowing, or when it comes back on
/// while dying.
pub fn step(a: &mut Aura, show: bool, ticks: f32) -> Step {
    if !a.want.on {
        if a.shown.is_none() {
            return Step::Keep;
        }
        return match a.dying {
            None => {
                a.dying = Some(90.0);
                Step::Dead
            }
            Some(t) if t - ticks > 0.0 => {
                a.dying = Some(t - ticks);
                Step::Keep
            }
            Some(_) => {
                (a.shown, a.dying, a.init_done) = (None, None, false);
                Step::Remove
            }
        };
    }
    if a.shown.is_none() {
        if !show {
            return Step::Keep;
        }
        (a.shown, a.dying) = (Some(a.want), None);
        return Step::Create(a.want);
    }
    if a.dying.is_none() && show {
        return Step::Keep;
    }
    (a.shown, a.dying, a.init_done) = (None, None, false);
    Step::Remove
}

/// The aura's scale size for its owner (FUN_10084520: owner entity +0x130 >> 30).
pub fn size(w: &World, aura: u32) -> Option<u8> {
    let owner = aura & !AURA;
    let a = if owner == w.my_id { &w.my_aura } else { &w.entities.get(&owner)?.aura };
    a.shown.map(|i| i.size)
}

/// Drops an aura actor and unlinks its effects (FUN_10092800, the actor's destructor FUN_10082b10).
fn remove(w: &mut World, id: u32) {
    w.actors.remove(&id);
    w.effects.forget_actor(id);
}

/// Per frame, for us and every PC: the filter (caster = target = the owner), the step, and `init` once the model
/// is in (FUN_100d0fc0).
pub fn update(w: &mut World, dt: f32) {
    let (cfg, battle) = w.effects.filter;
    let party: Vec<u32> = w.party.party().map(|m| m.id).collect();
    let mut owners: Vec<u32> = w.entities.iter().filter(|(id, e)| **id != w.my_id && e.kind == super::EntityKind::Player).map(|(id, _)| *id).collect();
    owners.push(w.my_id);
    for owner in owners {
        let me = owner == w.my_id;
        let monster = !me && w.entities.get(&owner).is_some_and(|e| e.flags & 1 != 0);
        let c = crate::fx::filter_class(me, monster, me || party.contains(&owner), false);
        let show = crate::fx::effects_allowed(cfg, battle, c | c << 4);
        let a = if me { &mut w.my_aura } else { &mut w.entities.get_mut(&owner).expect("listed").aura };
        let id = AURA | owner;
        match step(a, show, dt * 60.0) {
            Step::Create(v) => {
                log::info!("indi aura {owner:#x}: element {} size {} (file {})", v.element, v.size, v.file());
                remove(w, id);
                w.event_looks.push((id, Source::File(v.file())));
            }
            Step::Dead => super::actmotion::fx_too(&mut w.effects, id, "dead"),
            Step::Remove => remove(w, id),
            Step::Keep => {}
        }
        let a = if me { &w.my_aura } else { &w.entities[&owner].aura };
        if a.shown.is_some() && !a.init_done && w.actors.get(&id).is_some_and(|x| x.character.is_some()) {
            w.effects.on_scheduler_quiet(id, u32::from_le_bytes(*b"init"));
            if me {
                w.my_aura.init_done = true;
            } else if let Some(e) = w.entities.get_mut(&owner) {
                e.aura.init_done = true;
            }
        }
    }
    // Owners that left (FUN_10092900): their auras go with them.
    let gone: Vec<u32> = w.actors.keys().filter(|&&id| id & AURA != 0 && id & !AURA != w.my_id && !w.entities.contains_key(&(id & !AURA))).copied().collect();
    for id in gone {
        remove(w, id);
    }
}

/// Applies a packet's byte; a restart drops the actor now.
pub fn apply(w: &mut World, owner: u32, b: u8) {
    let v = Indi::from_byte(b);
    let a = if owner == w.my_id {
        &mut w.my_aura
    } else {
        let Some(e) = w.entities.get_mut(&owner) else { return };
        &mut e.aura
    };
    if set(a, v) {
        remove(w, AURA | owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB's indiVisualEffect bytes: Indi-Regen (light, allies), Indi-Poison (water, enemies), none, Widened Compass.
    #[test]
    fn indi_bits() {
        let i = Indi::from_byte(0x56);
        assert_eq!((i, i.file()), (Indi { element: 6, size: 1, on: true }, 52217));
        assert_eq!(Indi::from_byte(0x5d).file(), 52224);
        assert_eq!(Indi::from_byte(0x10), Indi { element: 0, size: 1, on: false });
        assert_eq!(Indi::from_byte(0x60), Indi { element: 0, size: 2, on: true });
        assert_eq!(Indi::from_byte(0xd6 & 0x7f), Indi { element: 6, size: 1, on: true });
        assert_eq!((Indi::from_byte(0x0f).file(), Indi::from_byte(0).file()), (52226, 52211));
    }

    #[test]
    fn indi_steps() {
        let on = Indi::from_byte(0x56);
        let mut a = Aura::default();
        set(&mut a, on);
        assert_eq!(step(&mut a, true, 1.0), Step::Create(on));
        assert_eq!(step(&mut a, true, 1.0), Step::Keep);
        assert!(!set(&mut a, Indi::from_byte(0x10)));
        assert_eq!(step(&mut a, true, 1.0), Step::Dead);
        assert_eq!(step(&mut a, true, 89.0), Step::Keep);
        assert_eq!(step(&mut a, true, 1.0), Step::Remove);
        assert_eq!(step(&mut a, true, 1.0), Step::Keep);
        // Back on while dying: removed at once, then made again.
        set(&mut a, on);
        assert_eq!(step(&mut a, true, 1.0), Step::Create(on));
        set(&mut a, Indi::from_byte(0x10));
        assert_eq!(step(&mut a, true, 1.0), Step::Dead);
        assert!(!set(&mut a, on));
        assert_eq!(step(&mut a, true, 1.0), Step::Remove);
        assert_eq!(step(&mut a, true, 1.0), Step::Create(on));
        // A new element or size while on restarts; the same value does nothing.
        assert!(set(&mut a, Indi::from_byte(0x55)));
        assert_eq!(step(&mut a, true, 1.0), Step::Create(Indi::from_byte(0x55)));
        assert!(set(&mut a, Indi::from_byte(0x65)));
        assert_eq!(step(&mut a, true, 1.0), Step::Create(Indi::from_byte(0x65)));
        assert!(!set(&mut a, Indi::from_byte(0x65)));
        // The filter: nothing made while it says no; turning it off removes without `dead`.
        let mut a = Aura::default();
        set(&mut a, on);
        assert_eq!(step(&mut a, false, 1.0), Step::Keep);
        assert_eq!(step(&mut a, true, 1.0), Step::Create(on));
        assert_eq!(step(&mut a, false, 1.0), Step::Remove);
    }

    /// Aura file 52211 (fire, allies): the size bytes, init -> star, the looping generators.
    #[test]
    fn indi_files() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(bytes) = index.read(52211) else { return };
        let lib = crate::fx::lib::Library::load(52211, bytes.clone()).unwrap();
        let mut loops: Vec<String> = lib.gens.iter().filter(|g| g.flags & 0x1000 != 0).map(|g| String::from_utf8_lossy(&g.name).into_owned()).collect();
        loops.sort();
        assert_eq!(loops, ["gl01", "hk01", "sen8", "sen9", "yr01", "yr02"]);
        assert!(lib.gens.iter().all(|g| g.attach_mode() == 2));
        let ops = |name: &[u8; 4]| {
            let n = lib.find_global(crate::dat::generator::kind::SCHEDULER, u32::from_le_bytes(*name)).unwrap();
            crate::dat::sched::ops(&bytes[lib.tree.nodes[n].data.clone()]).iter().map(|o| (o.code, o.name().unwrap_or("").to_string())).collect::<Vec<_>>()
        };
        assert!(ops(b"init").contains(&(0x57, "star".to_string())));
        assert!(ops(b"star").contains(&(0x0b, "7195".to_string())));
        assert!(ops(b"dead").contains(&(0x57, "stop".to_string())));
    }
}
