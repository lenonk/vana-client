//! The equipment effect schedulers (MODLOG "Weapon meshes hidden and shown"): FUN_100cef00 looks a name up in the
//! actor's part files then its model's (vtable +0x1ec, FUN_100ce9b0; no DAT 0) and starts it on (actor, actor).
//! Actor +0x840 bits 8..17 say a `wepN` glow is off (`!w02` / `!w12` ran), bit 7 that `!in?` ran.

use super::sched::Who;
use super::{Actors, Effects, Ref, WeaponOp};
use crate::dat::generator::kind;

/// What the client asks of an actor's equipment effects, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EquipCall {
    /// FUN_100d5fe0 (on: `!w01` / `!w11`) / FUN_100d6180 (off: `!w02` / `!w12`) for `wepN`.
    Glow { n: u8, on: bool },
    /// FUN_100cf060: `!in1`, else `!in2`, else `!in3`, once (bit 7); a hidden model (+0xb2) kills instead.
    In { hidden: bool },
    /// FUN_100cf0b0: `!kl1`, `!kl2`, `!kl3` once after an `!in?`.
    Kill,
}

/// Actor +0x840 bits 8..17 (`wepN` glow off) and bit 7 (`!in?` ran).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EquipState {
    pub glow_off: u16,
    pub in_on: bool,
}

/// Op 0x75's timed hide (FUN_10061960): a child of its scheduler; at its end, or the scheduler's, the mesh shows
/// again when `show` (FUN_10061920).
#[derive(Clone, Copy, Debug)]
pub(super) struct WepTimer {
    pub owner: u64,
    pub actor: u32,
    pub n: u8,
    pub left: f32,
    pub show: bool,
}

const fn cc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

impl Effects {
    /// Queues a call; they run in order at the next tick (an actor's wait on its files holds its later calls).
    pub fn equip(&mut self, actor: u32, call: EquipCall) {
        self.equip_calls.push((actor, call));
    }

    /// An actor's calls not yet run (the client's per-frame check waits for them).
    pub fn equip_waiting(&self, actor: u32) -> bool {
        self.equip_calls.iter().any(|c| c.0 == actor)
    }

    /// A new actor on the id (our own at a zone-in).
    pub fn reset_equip(&mut self, actor: u32) {
        self.equip.remove(&actor);
        self.equip_calls.retain(|c| c.0 != actor);
    }

    pub fn equip_state(&self, actor: u32) -> EquipState {
        self.equip.get(&actor).copied().unwrap_or_default()
    }

    pub(super) fn run_equip(&mut self, actors: &dyn Actors) {
        let mut waiting: Vec<u32> = Vec::new();
        for (id, call) in std::mem::take(&mut self.equip_calls) {
            if waiting.contains(&id) || self.equip_call(id, call, actors).is_err() {
                waiting.push(id);
                self.equip_calls.push((id, call));
            }
        }
    }

    fn equip_call(&mut self, id: u32, call: EquipCall, actors: &dyn Actors) -> Result<(), ()> {
        let s = self.equip_state(id);
        match call {
            EquipCall::Glow { n, on } => {
                let (Some(on_name), Some(off_name)) = (glow_names(n, true), glow_names(n, false)) else { return Ok(()) };
                let bit = 1u16 << n;
                if on {
                    // Config 0xbe 1 (Misc. 3 weapon effect Off): the bit clears without the scheduler.
                    if s.glow_off & bit != 0 && (self.weapon_effect_off || self.start_part(id, &[on_name], actors)?) {
                        self.equip.entry(id).or_default().glow_off &= !bit;
                    }
                } else if s.glow_off & bit == 0 && self.start_part(id, &[off_name], actors)? {
                    self.equip.entry(id).or_default().glow_off |= bit;
                }
            }
            EquipCall::In { hidden: true } => self.equip_call(id, EquipCall::Kill, actors)?,
            EquipCall::In { hidden: false } => {
                if !s.in_on {
                    self.start_part(id, &[cc(b"!in1"), cc(b"!in2"), cc(b"!in3")], actors)?;
                    self.equip.entry(id).or_default().in_on = true;
                }
            }
            EquipCall::Kill => {
                if s.in_on {
                    // Each of the three is looked up and started on its own.
                    let found = [cc(b"!kl1"), cc(b"!kl2"), cc(b"!kl3")].map(|name| self.part_scheduler(id, name, actors));
                    if found.iter().any(|f| f.is_err()) {
                        return Err(());
                    }
                    for r in found.into_iter().flatten().flatten() {
                        self.spawn_task(r, Who::Actor(id), Who::Actor(id), None, 1.0, None);
                    }
                    self.equip.entry(id).or_default().in_on = false;
                }
            }
        }
        Ok(())
    }

    /// FUN_100cef00: the first of `names` found starts; Ok(false) when none is there, Err while a file loads.
    fn start_part(&mut self, id: u32, names: &[u32], actors: &dyn Actors) -> Result<bool, ()> {
        for &name in names {
            if let Some(r) = self.part_scheduler(id, name, actors)? {
                log::debug!("effects: {} on {id:#x}", String::from_utf8_lossy(&name.to_le_bytes()));
                self.spawn_task(r, Who::Actor(id), Who::Actor(id), None, 1.0, None);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// FUN_100ce9b0: the part files, then the model's own files.
    fn part_scheduler(&mut self, id: u32, name: u32, actors: &dyn Actors) -> Result<Option<Ref>, ()> {
        let (parts, _) = actors.files(id);
        let mut waiting = false;
        for file in parts.into_iter().chain(actors.model_files(id)) {
            match self.lib_for(file) {
                Some(l) => {
                    if let Some(n) = self.reg.libs[l].find_global(kind::SCHEDULER, name) {
                        return Ok(Some((l, n)));
                    }
                }
                None if self.failed.contains(&file) => {}
                None => waiting = true,
            }
        }
        if waiting { Err(()) } else { Ok(None) }
    }

    /// Timed hides end with their time or their scheduler (FUN_100619d0, FUN_10061920).
    pub(super) fn step_wep_timers(&mut self, dt: f32, live: &[u64]) {
        for mut t in std::mem::take(&mut self.wep_timers) {
            t.left -= dt;
            if t.left >= 0.0 && live.contains(&t.owner) {
                self.wep_timers.push(t);
            } else if t.show {
                self.weapons.push(WeaponOp { actor: t.actor, n: t.n, byte: 0, hide: false });
            }
        }
    }
}

/// `!w01` / `!w11` (on) and `!w02` / `!w12` (off); other N have none.
fn glow_names(n: u8, on: bool) -> Option<u32> {
    let d = match n {
        0 => b'0',
        1 => b'1',
        _ => return None,
    };
    Some(u32::from_le_bytes([b'!', b'w', d, if on { b'1' } else { b'2' }]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::lib::Library;
    use crate::fx::sched::{SOp, Task};
    use crate::fx::{Frame, sound::SoundCmd};
    use glam::{Mat4, Vec3};
    use std::rc::Rc;

    const HUME: u32 = 0x0001_0001;

    /// A Hume M (skeleton 7072, battle file 9672) holding `part`, engaged or not.
    struct Pc {
        part: u32,
        status: u8,
    }

    impl Actors for Pc {
        fn matrix(&self, _: u32) -> Option<Mat4> {
            Some(Mat4::IDENTITY)
        }
        fn point(&self, _: u32, _: u16) -> Option<Vec3> {
            Some(Vec3::new(0.0, -1.0, 0.0))
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
            (vec![self.part], vec![9672, 7072])
        }
        fn model_files(&self, _: u32) -> Vec<u32> {
            vec![7072]
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
        fn status(&self, _: u32) -> u8 {
            self.status
        }
    }

    fn loaded(files: &[u32]) -> Option<Effects> {
        let index = std::sync::Arc::new(crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).ok()?).ok()?);
        fx.preload(files);
        while files.iter().any(|f| fx.reg.lib_of(*f).is_none()) {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Some(fx)
    }

    fn frame() -> Frame {
        Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 }
    }

    fn tasks(fx: &Effects) -> Vec<String> {
        fx.tasks.iter().flatten().map(|t| String::from_utf8_lossy(&fx.reg.libs[t.res.0].tree.nodes[t.res.1].name).into_owned()).collect()
    }

    /// Hume M main 8875: off runs `!w02` and sets the bit; on runs `!w01` (glwc glwa bld1 bld4, SeSep 6191) and
    /// clears it; a second on does nothing.
    #[test]
    fn weapon_glow_cycle() {
        let Some(mut fx) = loaded(&[8875, 9672, 7072]) else { return };
        let pc = Pc { part: 8875, status: 1 };
        fx.equip(HUME, EquipCall::Glow { n: 0, on: false });
        fx.run_equip(&pc);
        assert_eq!((tasks(&fx), fx.equip_state(HUME).glow_off), (vec!["!w02".to_string()], 1));
        fx.tasks.clear();
        fx.equip(HUME, EquipCall::Glow { n: 0, on: true });
        fx.tick(&frame(), &pc);
        assert_eq!(fx.equip_state(HUME).glow_off, 0);
        let names: Vec<String> = fx.insts.iter().flatten().map(|x| String::from_utf8_lossy(&x.name).into_owned()).collect();
        for g in ["glwc", "glwa", "bld1", "bld4"] {
            assert!(names.iter().any(|n| n == g), "{g}: {names:?}");
        }
        assert!(fx.sound_cmds.iter().any(|c| matches!(c, SoundCmd::Play { se: 6191, .. })), "{:?}", fx.sound_cmds);
        fx.tasks.clear();
        fx.equip(HUME, EquipCall::Glow { n: 0, on: true });
        fx.run_equip(&pc);
        assert!(tasks(&fx).is_empty());
    }

    /// A fresh actor's first call is on (already engaged): its bit is clear, so nothing plays, as in retail; config
    /// 0xbe Off clears the bit without `!w01`.
    #[test]
    fn weapon_glow_fresh_and_off() {
        let Some(mut fx) = loaded(&[8875, 9672, 7072]) else { return };
        let pc = Pc { part: 8875, status: 1 };
        fx.equip(HUME, EquipCall::Glow { n: 0, on: true });
        fx.run_equip(&pc);
        assert!(tasks(&fx).is_empty());
        fx.equip(HUME, EquipCall::Glow { n: 0, on: false });
        fx.run_equip(&pc);
        fx.tasks.clear();
        fx.weapon_effect_off = true;
        fx.equip(HUME, EquipCall::Glow { n: 0, on: true });
        fx.run_equip(&pc);
        assert_eq!((tasks(&fx).len(), fx.equip_state(HUME).glow_off), (0, 0));
    }

    /// No `!w02` in the files (fists 8392): the bit stays clear, so a later on plays nothing.
    #[test]
    fn weapon_glow_not_found_keeps_bit() {
        let Some(mut fx) = loaded(&[8392, 9672, 7072]) else { return };
        let pc = Pc { part: 8392, status: 0 };
        fx.equip(HUME, EquipCall::Glow { n: 0, on: false });
        fx.run_equip(&pc);
        assert_eq!((tasks(&fx).len(), fx.equip_state(HUME).glow_off), (0, 0));
    }

    /// 8866: `!in1` once; `!kl1` (with `!kl2` / `!kl3` where they exist) once after it; a hidden model kills.
    #[test]
    fn in_and_kill() {
        let Some(mut fx) = loaded(&[8866, 9672, 7072]) else { return };
        let pc = Pc { part: 8866, status: 0 };
        fx.equip(HUME, EquipCall::In { hidden: true });
        fx.run_equip(&pc);
        assert!(tasks(&fx).is_empty());
        fx.equip(HUME, EquipCall::In { hidden: false });
        fx.equip(HUME, EquipCall::In { hidden: false });
        fx.run_equip(&pc);
        assert_eq!((tasks(&fx), fx.equip_state(HUME).in_on), (vec!["!in1".to_string()], true));
        fx.tasks.clear();
        fx.equip(HUME, EquipCall::In { hidden: true });
        fx.equip(HUME, EquipCall::Kill);
        fx.run_equip(&pc);
        assert_eq!((tasks(&fx), fx.equip_state(HUME).in_on), (vec!["!kl1".to_string()], false));
    }

    fn weapon_task(fx: &mut Effects, raw8: u32, gate: u16) {
        let mut op = vec![0u8; 16];
        op[..8].copy_from_slice(&[0x75, 4, 0, 0, 10, 0, 100, 0]);
        op[8..12].copy_from_slice(&raw8.to_le_bytes());
        op[0xe..0x10].copy_from_slice(&gate.to_le_bytes());
        let ops = vec![SOp { code: 0x75, wait: 10, dur: 100, raw: op }, SOp { code: 0x00, wait: 0, dur: 0, raw: vec![0, 2, 0, 0, 0, 0, 0, 0] }];
        fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(HUME), Who::Actor(HUME), None, 1.0)));
    }

    /// Op 0x75 with a length: hidden now, shown when its scheduler ends after its wait of 10 (not at 100); with @8 set it stays
    /// hidden; the gate (i16 @0xe) skips it out of battle.
    #[test]
    fn weapon_timed_hide_shows_on_task_death() {
        let Some(mut fx) = loaded(&[]) else { return };
        let idle = Pc { part: 8392, status: 0 };
        weapon_task(&mut fx, 0, 0);
        let mut seen = Vec::new();
        for t in 1..=20 {
            fx.tick(&frame(), &idle);
            seen.extend(fx.weapons.drain(..).map(|w| (t, w.hide)));
        }
        assert_eq!(seen, [(1, true), (10, false)]);
        weapon_task(&mut fx, 1, 0);
        seen.clear();
        for t in 1..=120 {
            fx.tick(&frame(), &idle);
            seen.extend(fx.weapons.drain(..).map(|w| (t, w.hide)));
        }
        assert_eq!(seen, [(1, true)]);
        weapon_task(&mut fx, 0, 1);
        fx.tick(&frame(), &idle);
        assert!(fx.weapons.is_empty());
    }
}
