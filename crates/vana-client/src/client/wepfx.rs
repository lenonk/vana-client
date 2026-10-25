//! The `wepN` meshes' hide bytes and the equipment effect calls (fx/equip.rs) at retail's points (MODLOG "Weapon
//! meshes hidden and shown"): FUN_100d60e0 / FUN_100d5f10 per mesh, FUN_100cf100 once the model is up, FUN_100973f0
//! every frame for PCs, FUN_100c83c0 at a status motion's start, FUN_100cf160 at the sheathe.

use super::{World, event, ride};
use crate::fx::EquipCall;

/// The applied status (+0x170) is 1.
fn engaged(w: &World, id: u32) -> bool {
    if id == w.my_id { w.my_motion.status == 1 } else { w.entities.get(&id).is_some_and(|e| e.motion.status == 1) }
}

/// InvisFlag (+0x128 bit 6).
fn invisible(w: &World, id: u32) -> bool {
    if id == w.my_id { w.my_plate.invisible } else { w.entities.get(&id).is_some_and(|e| e.invisible) }
}

/// Model +0xb2: the server's hide bits, or a rider waiting hidden for its mount (+0x40).
fn model_hidden(w: &World, id: u32) -> bool {
    w.entities.get(&id).is_some_and(|e| e.flags & 2 != 0) || ride::hidden(w).contains(&id)
}

/// FUN_100d6240: a hide byte of `wepN` is set.
fn wep_hidden(w: &World, id: u32, n: usize) -> bool {
    w.actors.get(&id).is_some_and(|a| a.wep_hide[n].iter().any(|&h| h))
}

/// Only `wep0` / `wep1` have glow schedulers.
fn glow(w: &mut World, id: u32, n: usize, on: bool) {
    if n < 2 {
        w.effects.equip(id, EquipCall::Glow { n: n as u8, on });
    }
}

/// FUN_100d60e0: byte `b` of `wepN` set, its glow off.
pub fn hide(w: &mut World, id: u32, n: usize, b: usize) {
    if let Some(a) = w.actors.get_mut(&id).filter(|_| n < 16) {
        a.wep_hide[n][b] = true;
    }
    glow(w, id, n, false);
}

/// FUN_100d5f10: byte `b` of `wepN` clear; the glow on while engaged and not invisible, else off.
pub fn show(w: &mut World, id: u32, n: usize, b: usize) {
    if let Some(a) = w.actors.get_mut(&id).filter(|_| n < 16) {
        a.wep_hide[n][b] = false;
    }
    let on = engaged(w, id) && !invisible(w, id);
    glow(w, id, n, on);
}

/// FUN_100d60c0: byte 0 of wep0..wep8.
pub fn hide_all(w: &mut World, id: u32) {
    for n in 0..9 {
        hide(w, id, n, 0);
    }
}

/// FUN_100cf100: each glow on while the model shows, engaged, the weapon drawn (+0x83c) and the mesh not hidden.
pub fn refresh(w: &mut World, id: u32) {
    let base = !model_hidden(w, id) && engaged(w, id) && w.actors.get(&id).is_some_and(|a| a.drawn);
    for n in 0..2 {
        let on = base && !wep_hidden(w, id, n);
        glow(w, id, n, on);
    }
}

/// FUN_100cf160: every glow off.
pub fn all_off(w: &mut World, id: u32) {
    for n in 0..2 {
        glow(w, id, n, false);
    }
}

/// FUN_100cf060 after `init` (FUN_1008c480's default status case): `!in?`, or `!kl?` on a hidden model.
pub fn start_in(w: &mut World, id: u32) {
    let hidden = model_hidden(w, id);
    w.effects.equip(id, EquipCall::In { hidden });
}

/// FUN_100973f0, run every frame from the name getter (FUN_100977f0's gate: a model, a name plate, targetable; the
/// own model hide +0x120 bits 14 / 15 not modelled): a PC after its `init`, not bound by an event. Shown: `!in?`
/// and FUN_100cf100; invisible: `!kl?` and every glow off.
pub fn frame(w: &mut World) {
    let mut ids: Vec<u32> = w.entities.iter().filter(|(id, e)| **id >> 24 == 0 && e.init_done && !e.no_plate && !e.untargetable).map(|(id, _)| *id).collect();
    if w.my_init_done {
        ids.push(w.my_id);
    }
    for id in ids {
        if !w.actors.get(&id).is_some_and(|a| a.character.is_some()) || event::bound(w, id) || w.effects.equip_waiting(id) {
            continue;
        }
        if invisible(w, id) {
            w.effects.equip(id, EquipCall::Kill);
            all_off(w, id);
        } else {
            start_in(w, id);
            refresh(w, id);
        }
    }
}

/// FUN_100c83c0, a new status motion on an entity's own actor: mounted (0x55) hides every weapon, a chair (0x3f..0x53)
/// nothing; else wep0 / wep1 shown and wep2 hidden (byte 0) for model kind 0 (a PC, or an equipped look with PetFlag
/// @0x28 bit 6), a monster (+0x120 bit 13: kinds 1 / 2 / 7, Fomors' kind 6), an actor bound by an event, or a trust
/// (+0x12c bit 22).
pub fn status_start(w: &mut World, id: u32, status: u8) {
    if status == 0x55 {
        hide_all(w, id);
        return;
    }
    if (0x3f..=0x53).contains(&status) {
        return;
    }
    let pc_look = w.actors.get(&id).and_then(|a| a.character.as_ref()).is_some_and(|c| c.pc.is_some());
    let e = w.entities.get(&id);
    let kind0 = id == w.my_id || id >> 24 == 0 || (pc_look && e.is_some_and(|e| e.marks.npc_pet));
    let (monster, trust) = (e.is_some_and(|e| e.mob), e.is_some_and(|e| e.marks.trust));
    if kind0 || monster || trust || event::bound(w, id) {
        show(w, id, 0, 0);
        show(w, id, 1, 0);
        hide(w, id, 2, 0);
    }
}

/// Fx's ops 0x75 / 0xa3 (and their timed shows) on our actors.
pub fn apply_ops(w: &mut World) {
    for op in std::mem::take(&mut w.effects.weapons) {
        let (n, b) = (op.n as usize, op.byte as usize);
        if op.hide {
            hide(w, op.actor, n, b);
        } else {
            show(w, op.actor, n, b);
        }
    }
}

/// The meshes not drawn (bit N).
pub fn mask(h: &[[bool; 2]; 16]) -> u16 {
    (0..16).filter(|&n| h[n].iter().any(|&x| x)).fold(0, |m, n| m | 1 << n)
}

/// A new model's `init` hides (dat/npc.rs), until `init` itself runs.
pub fn seed(a: &mut super::Actor, ch: &crate::dat::character::Character) {
    for (h, init) in a.wep_hide.iter_mut().zip(ch.init_hidden) {
        for (x, y) in h.iter_mut().zip(init) {
            *x |= y;
        }
    }
}
