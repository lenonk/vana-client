//! Targeting and lock-on (target object DAT_10565d34, FUN_10151280 / FUN_10150da0), Numpad * (key
//! functions 0x44 / 0x30), the s2c 0x058 assist target, and the locked camera of FUN_1001e030.

use super::World;
use crate::audio::Ui;
use glam::Vec3;

/// SE "0009" (FUN_10036f50): a target from none; "0010" (FUN_10036fb0): switching targets.
const SE_TARGET: u32 = 9;
const SE_SWITCH: u32 = 10;
/// Camera focal length, config 0x49 default 350 (range 242..900).
pub const FOCAL: f32 = 350.0;

/// The forced flash (actor +0x88 bit 0x200) for a frame, as the sub-target cursor and area previews set it,
/// unless /targetcharaeffect turned it off (config 0x99 bit 7). Nothing sets it yet: there is no `<st>`.
#[allow(dead_code)]
pub fn force_flash(w: &mut World, id: u32) {
    if w.game.get(0x99) & 0x80 == 0
        && let Some(a) = w.actors.get_mut(&id)
    {
        a.flash.forced = true;
    }
}

/// FUN_10151280: sets the target with its sound; a different target ends the lock (FUN_10153040).
pub fn set(w: &mut World, t: Option<u32>) {
    if w.target == t {
        return;
    }
    match (w.target, t) {
        (None, Some(_)) => w.sounds.push(SE_TARGET),
        (Some(_), Some(_)) => w.sounds.push(SE_SWITCH),
        _ => {}
    }
    log::info!("target {:#x} {:?}", t.unwrap_or(0), t.and_then(|t| w.entities.get(&t)).map(|e| e.name.as_str()));
    w.target = t;
    w.target_since = std::time::Instant::now();
    if w.locked {
        lock(w, false, "target changed");
    }
}

/// FUN_10151f20: the player's own target inputs (Tab, Escape, a mouse pick) do nothing while locked; only the
/// lock toggle and the server move a locked target.
pub fn picks_allowed(w: &World) -> bool {
    !w.locked
}

/// FUN_10150da0: lock needs a target, a living player and a target that is not us. `why` goes in the log.
pub fn lock(w: &mut World, on: bool, why: &str) {
    if on == w.locked {
        return;
    }
    if on {
        let ok = w.target.is_some_and(|t| t != w.my_id && w.entities.contains_key(&t)) && !w.dead();
        if !ok {
            return;
        }
        // The locked camera starts from the camera on screen (the same eye and look).
        w.locked = true;
        log::info!("lock on {:#x} ({why})", w.target.unwrap_or(0));
    } else {
        w.locked = false;
        log::info!("lock off ({why})");
        // Nothing snaps: the same eye and look go on in the free branch, held 20 ticks (_DAT_10444464).
        w.unlock_ticks = 20.0;
    }
}

/// Why the target goes (FUN_10151bb0 -> FUN_10152c70), checked every frame; Some(true) also sends /attackoff
/// (FUN_10151c40, +0x130 bit 7).
fn invalid(w: &World, t: u32) -> Option<bool> {
    // Ourselves: only an event hiding us drops it (distance 0, never attackable).
    if t == w.my_id {
        return super::event::hidden(w).contains(&t).then_some(false);
    }
    let Some(e) = w.entities.get(&t) else { return Some(false) };
    let hidden = e.flags & 2 != 0 || super::event::hidden(w).contains(&t);
    drop_rule(e.flags, e.untargetable, e.motion.status, (e.pos - w.pos).length(), hidden)
}

/// FUN_10152c70 on one target: hidden (+0xb2 bit 1, from the server bits or the event via FUN_10084360);
/// attackable (+0x120 bit 13, 0x00E @0x20 bit 0) and flagged off (Some(true)), despawning (status 2) or dead
/// (3); more than 50 units away (FUN_10152ff0(50.0)).
fn drop_rule(flags: u8, untargetable: bool, status: u8, dist: f32, hidden: bool) -> Option<bool> {
    if hidden {
        return Some(false);
    }
    if flags & 1 != 0 {
        if untargetable {
            return Some(true);
        }
        if matches!(status, 2 | 3) {
            return Some(false);
        }
    }
    (dist > 50.0).then_some(false)
}

/// Kind mask bits (FUN_1008e290): 1 PC, 2 NPC, 0x10 monster, 0x20 door.
fn kind_bits(id: u32, e: &super::Entity) -> u32 {
    if id >> 24 == 0 {
        1
    } else if e.door != 0 {
        0x20
    } else if e.mob {
        0x10
    } else {
        2
    }
}

/// FUN_10083430 with no sub-target prompt: drawn on screen this frame (+0xb2 bit 0 clear: World::visible), not
/// hidden, not ourselves, and not a dead or despawning monster. No name test. Not done: the event untargetable bit
/// (ops 0x12..0x15, +0x128 bit 4) and the occlusion part of the cull.
fn candidate(w: &World, hidden: &std::collections::HashSet<u32>, id: u32, e: &super::Entity) -> bool {
    id != w.my_id && w.visible.contains(&id) && !hidden.contains(&id) && !(e.flags & 1 != 0 && matches!(e.motion.status, 2 | 3))
}

/// FUN_10083620 / FUN_10083f20: the nearest candidate in front of the character (its own frame, local forward
/// >= 0) within 50 units (distance^2 < 2500), optionally of some kinds.
fn nearest(w: &World, keep: impl Fn(u32, &super::Entity) -> bool) -> Option<u32> {
    let hidden = super::event::hidden(w);
    let forward = Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos());
    w.entities
        .iter()
        .filter(|(id, e)| candidate(w, &hidden, **id, e) && keep(**id, e))
        .map(|(id, e)| (*id, e.pos - w.pos))
        .filter(|(_, d)| d.dot(forward) >= 0.0 && d.length_squared() < 2500.0)
        .min_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))
        .map(|(id, _)| id)
}

/// FUN_10083d00 / FUN_10083b40 order: from the target's screen key, the nearest key to that side (under 3600);
/// none, the far end the other way (Tab wraps to the leftmost, Shift+Tab to the rightmost).
fn screen_step(keys: &[(u32, f32)], from: f32, right: bool) -> Option<u32> {
    let side = |k: f32| if right { k - from } else { from - k };
    let next = keys.iter().filter(|(_, k)| side(*k) > 0.0 && side(*k) < 3600.0).min_by(|a, b| side(a.1).total_cmp(&side(b.1)));
    let wrap = || keys.iter().filter(|(_, k)| side(*k) < 0.0).max_by(|a, b| side(a.1).abs().total_cmp(&side(b.1).abs()));
    next.or_else(wrap).map(|(id, _)| *id)
}

/// Key functions 0x84 / 0x97 (Tab, Num0 / with Shift): FUN_10158ca0. No valid target: the nearest in front;
/// else screen order by x_view / max(|z_view|, 1) x focal among candidates within 50 of the camera (on screen this
/// frame, so nothing behind the camera), else the nearest.
pub fn cycle(w: &mut World, view: glam::Mat4, right: bool) {
    if !picks_allowed(w) {
        return;
    }
    let focal = super::camera::focal(w);
    let raw = |p: Vec3| {
        let v = view.transform_point3(p);
        (v.x / v.z.abs().max(1.0) * focal, v.length())
    };
    let cur = w.target.filter(|t| w.entities.contains_key(t));
    let pick = cur.and_then(|t| {
        let from = raw(w.entities[&t].pos).0;
        let key = |p: Vec3| Some(raw(p)).filter(|k| k.1 < 50.0).map(|k| k.0);
        let hidden = super::event::hidden(w);
        let keys: Vec<(u32, f32)> = w.entities.iter().filter(|(id, e)| **id != t && candidate(w, &hidden, **id, e)).filter_map(|(id, e)| key(e.pos).map(|k| (*id, k))).collect();
        screen_step(&keys, from, right)
    });
    let pick = pick.or_else(|| nearest(w, |_, _| true));
    if pick.is_some() {
        set(w, pick);
    }
}

/// Enter with no target (FUN_101587f0 case 0): the nearest in front, only targeted. False when there is none
/// (the caller opens "menu playermo", the action menu).
pub fn enter_nearest(w: &mut World) -> bool {
    let t = nearest(w, |_, _| true);
    if t.is_some() {
        set(w, t);
    }
    t.is_some()
}

/// F8 / Ctrl+F8 / Alt+F8 / F9 / F10 (FUN_10083ec0 / FUN_10083ee0 / FUN_10083f00): the nearest in front by kind.
/// `prefer` is the kind mask tried first (F8 monsters, F10 / Alt+F8 NPCs or monsters), then any non-PC; Ctrl+F8
/// (`enemies`) only monsters; F9 (`pcs`) only players.
pub fn nearest_kind(w: &mut World, prefer: u32, enemies: bool, pcs: bool) {
    if !picks_allowed(w) {
        return;
    }
    let t = if pcs {
        nearest(w, |id, _| id >> 24 == 0)
    } else if enemies {
        nearest(w, |id, e| id >> 24 != 0 && e.mob)
    } else {
        nearest(w, |id, e| id >> 24 != 0 && kind_bits(id, e) & prefer != 0).or_else(|| nearest(w, |id, _| id >> 24 != 0))
    };
    if t.is_some() {
        set(w, t);
    }
}

/// A target key's pick (FUN_10157b40 from FUN_101587f0): nothing while locked; picking the target already held
/// plays "0010" again (FUN_10037dd0).
pub fn pick(w: &mut World, t: Option<u32>) {
    if !picks_allowed(w) || super::event::active(w) {
        return;
    }
    if t.is_some() && w.target == t {
        w.sounds.push(SE_SWITCH);
        return;
    }
    set(w, t);
}

/// FUN_102033c0 / FUN_102034a0: the party window slot's member when it is in our zone, not marked elsewhere, and
/// its model is loaded (no distance test).
pub fn slot_target(w: &World, slot: usize) -> Option<u32> {
    let m = w.party.slots().here(slot, w.pos_zone)?.clone();
    if m.id == w.my_id {
        return Some(m.id);
    }
    let id = w.entities.iter().find(|(_, e)| e.index == m.index).map(|(id, _)| *id)?;
    w.actors.get(&id).is_some_and(|a| a.character.is_some()).then_some(id)
}

/// FUN_101514a0: while engaged with no target, the first attackable actor we claimed that is alive.
fn auto_target(w: &World) -> Option<u32> {
    let alive = |e: &super::Entity| e.flags & 1 != 0 && e.claim == w.my_id && !e.untargetable && !matches!(e.motion.status, 2 | 3);
    w.entities.iter().filter(|(id, e)| alive(e) && w.actors.get(id).is_some_and(|a| a.gpu.is_some())).min_by_key(|(_, e)| e.index).map(|(id, _)| *id)
}

/// FUN_10151100 -> FUN_101516d0 / FUN_10151910: Numpad * is /heal with no target or while healing
/// (status 0x21), otherwise the lock-on toggle.
pub fn numpad_star(w: &mut World) {
    if super::event::active(w) {
        return;
    }
    if w.my_motion.status == 0x21 || w.target.is_none() {
        w.sounds.push(Ui::Select as u32);
        super::send_chat(w, "/heal".into());
    } else {
        let on = !w.locked;
        lock(w, on, "Numpad *");
    }
}

/// What LAB_1009e570 does with an s2c 0x058: None = nothing (not about us, no target, or no such actor with a
/// model, +0x120 bit 9), else Some(lock): set the target, and lock when it is a monster (+0x120 bit 13, inferred
/// the 0x00E monster bit) and auto lock-on is on (+0x51, config 7 = 0, the default).
fn assist_rule(player: u32, me: u32, target: u32, modelled: bool, monster: bool, auto_lock: bool) -> Option<bool> {
    (player == me && target != 0 && modelled).then_some(monster && auto_lock)
}

/// s2c 0x058 (LSB sends it on engage, on a target change and on auto-target): our assist target.
pub fn assist(w: &mut World, player: u32, target: u32) {
    let modelled = w.entities.contains_key(&target) && w.actors.get(&target).is_some_and(|a| a.gpu.is_some() || a.character.is_some());
    let monster = w.entities.get(&target).is_some_and(|e| e.mob);
    let rule = assist_rule(player, w.my_id, target, modelled, monster, w.auto_lock);
    log::info!("0x058 assist {target:#x}: {rule:?} (monster {monster}, auto lock-on {})", w.auto_lock);
    let Some(locks) = rule else { return };
    set(w, Some(target));
    if locks {
        lock(w, true, "engaged, auto lock-on");
    }
}

/// Per frame (FUN_10151100): own death unlocks (FUN_10150e30); leaving the engaged status (not by dying)
/// unlocks and keeps the target (FUN_10089d50 case 3); an invalid target goes at once with the lock; engaged
/// with no target picks one (FUN_10151910).
pub fn update(w: &mut World) {
    let st = w.my_motion.status;
    if w.locked && (w.dead() || (w.last_status == 1 && st != 1 && st != 3)) {
        lock(w, false, if w.dead() { "dead" } else { "disengaged" });
    }
    w.last_status = st;
    if let Some(attackoff) = w.target.and_then(|t| invalid(w, t)) {
        log::info!("target {:#x} dropped (attackoff {attackoff})", w.target.unwrap_or(0));
        lock(w, false, "target dropped");
        w.target = None;
        if attackoff {
            super::send_chat(w, "/attackoff".into());
        }
    }
    if st == 1 && w.target.is_none() && !super::event::active(w) {
        let t = auto_target(w);
        if t.is_some() {
            set(w, t);
        }
    }
    if w.locked && (w.target.is_none() || super::event::active(w)) {
        lock(w, false, "no target or an event");
    }
}

/// The camera's eye and look-at (camera object +0x44 / +0x50).
#[derive(Clone, Copy, Debug, Default)]
pub struct LockCam {
    pub eye: Vec3,
    pub look: Vec3,
}

/// Height of the look point above the feet (y down, so negative): player bone 13, target bone 5
/// (FUN_100a9500), x0.8 when we are a Tarutaru. Falls back to 0.7 of the model height (inferred).
fn look_height(w: &World, id: u32, bone: usize, scale_taru: bool) -> f32 {
    let a = w.actors.get(&id);
    let y = a.and_then(|a| a.bones.get(bone).map(|b| b.1.y * a.scale)).unwrap_or_else(|| a.map_or(-1.5, |a| a.top * 0.7));
    if scale_taru && matches!(w.my_race, 5 | 6) { y * 0.8 } else { y }
}

/// FUN_1001e030, locked branch, run in 1/60 s ticks: look at the player / target midpoint, keep the eye
/// within 3 atan(4 / (near + d)) of the target-to-player line and ease its distance into
/// [near + d/2, far + d/2], 25 % of the gap a tick. `turn` is manual orbit input in radians.
pub fn step_camera(w: &mut World, ticks: f32, turn: f32, rise: f32) {
    let Some(t) = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.pos))) else { return };
    let mut c = w.cam;
    let (tid, tpos) = t;
    let me = super::drawn_pos(w);
    let p = me + Vec3::new(0.0, look_height(w, w.my_id, 13, false), 0.0);
    let q = tpos + Vec3::new(0.0, look_height(w, tid, 5, true), 0.0);
    let l = p.lerp(q, 0.5);
    let k = 1.0 - 0.75f32.powf(ticks);
    c.look += (l - c.look) * k;
    let d = (me - tpos).length();
    let e = (me - tpos).normalize_or(Vec3::X);
    let mut dv = c.eye - c.look;
    // Manual orbit, scaled by 6 / |D| when locked (_DAT_1032a3e8 / max(|eye - look|, 0.01)).
    if turn != 0.0 {
        let len = dv.length().max(0.01);
        dv = glam::Quat::from_rotation_y(turn * 6.0 / len) * dv;
    }
    dv.y += rise;
    let near = (7.2 + 0.125 * d) * FOCAL * 0.001;
    let far = (8.6 + 0.125 * d) * FOCAL * 0.001;
    let allowed = (4.0 / (near + d)).atan() * 3.0;
    let len = dv.length().max(1e-3);
    let dir = dv / len;
    let angle = dir.dot(e).clamp(-1.0, 1.0).acos();
    let dir = if angle > allowed {
        // FUN_10032600: rotate toward E by the excess (about D x E; inferred).
        let axis = dir.cross(e).normalize_or(Vec3::Y);
        glam::Quat::from_axis_angle(axis, angle - allowed) * dir
    } else {
        dir
    };
    let goal = len.clamp(near + d / 2.0, far + d / 2.0);
    let len = len + (goal - len) * k;
    let mut eye = c.look + dir * len;
    // FUN_1017a6b0: a ray from the player (y - 1) to the eye; on a hit the eye goes 0.2 in front.
    let from = me + Vec3::new(0.0, -1.0, 0.0);
    if let Some(hit) = w.ground.as_ref().and_then(|g| g.raycast(from, eye)) {
        eye = from + (eye - from).normalize_or(Vec3::X) * (hit - 0.2).max(0.2);
    }
    // FUN_1001ee60's shared tail, after both branches: 0.17 clear of the floor below (within 10), and the eye never
    // passes through a wall: a sweep from where it was stops 0.2 short (as camera.rs does for the free camera).
    if let Some(g) = w.ground.as_ref().filter(|g| !g.is_empty()) {
        if let Some(f) = g.floor(eye.x, eye.z, eye.y, 0.0).filter(|f| f - eye.y <= 10.0)
            && eye.y > f - 0.17
        {
            eye.y = f - 0.17;
        }
        if g.raycast(from, c.eye).is_none()
            && let Some(t) = g.raycast(c.eye, eye)
        {
            eye = c.eye + (eye - c.eye).normalize_or(Vec3::X) * (t - 0.2).max(0.0);
        }
    }
    c.eye = eye;
    w.cam = c;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tab steps to the next screen key to the right and wraps to the leftmost; Shift+Tab mirrors it.
    #[test]
    fn screen_order() {
        let keys = [(1, -200.0), (2, -50.0), (3, 40.0), (4, 300.0)];
        assert_eq!(screen_step(&keys, 0.0, true), Some(3));
        assert_eq!(screen_step(&keys, 0.0, false), Some(2));
        assert_eq!(screen_step(&keys, 500.0, true), Some(1));
        assert_eq!(screen_step(&keys, -500.0, false), Some(4));
        assert_eq!(screen_step(&[], 0.0, true), None);
    }

    /// Attackable targets go when dead, despawning or flagged off (with /attackoff); NPCs only when hidden or
    /// more than 50 away.
    #[test]
    fn target_drop_rules() {
        assert_eq!(drop_rule(1, false, 0, 10.0, false), None);
        assert_eq!(drop_rule(1, false, 3, 10.0, false), Some(false));
        assert_eq!(drop_rule(1, false, 2, 10.0, false), Some(false));
        assert_eq!(drop_rule(1, true, 0, 10.0, false), Some(true));
        assert_eq!(drop_rule(0, true, 3, 10.0, false), None);
        assert_eq!(drop_rule(0, false, 0, 50.5, false), Some(false));
        assert_eq!(drop_rule(0, false, 0, 1.0, true), Some(false));
    }

    /// Engaging with auto lock-on at its default (config 7 = 0) targets and locks a monster; config 7 = 1
    /// only targets; NPCs are targeted without a lock; another player's 0x058, a zero target or an actor
    /// without a model do nothing.
    #[test]
    fn engage_locks_by_default() {
        let me = 0x0100_0004;
        let auto = crate::gamecfg::Values::default().get(7) == 0;
        assert!(auto, "config 7 defaults to 0 = auto lock-on");
        assert_eq!(assist_rule(me, me, 0x0106_5013, true, true, auto), Some(true));
        assert_eq!(assist_rule(me, me, 0x0106_5013, true, true, false), Some(false));
        assert_eq!(assist_rule(me, me, 0x0106_5239, true, false, auto), Some(false));
        assert_eq!(assist_rule(0x0100_0099, me, 0x0106_5013, true, true, auto), None);
        assert_eq!(assist_rule(me, me, 0, true, true, auto), None);
        assert_eq!(assist_rule(me, me, 0x0106_5013, false, true, auto), None);
    }

    /// Side input keeps the radius (circle strafe); forward closes in, stopping at 0.8.
    #[test]
    fn circle_strafe() {
        let t = Vec3::new(10.0, 0.0, 10.0);
        let p = Vec3::new(10.0, 0.0, 5.0);
        let q = locked_step(p, t, 0.0, 1.0, 0.5);
        assert!(((q - t).length() - 5.0).abs() < 1e-4);
        assert!((q - p).length() > 0.4);
        let r = locked_step(p, t, 1.0, 0.0, 10.0);
        assert!(((r - t).length() - 0.8).abs() < 1e-4);
    }
}

/// FUN_100a6210: locked movement relative to the target. Side input circles it at the same radius,
/// forward / back changes the radius, stopping at the two radii (inferred 0.8 together).
pub fn locked_step(pos: Vec3, target: Vec3, fwd: f32, side: f32, dist: f32) -> Vec3 {
    let r = Vec3::new(pos.x - target.x, 0.0, pos.z - target.z);
    let radius = r.length().max(1e-3);
    let facing = -r / radius;
    let right = facing.cross(Vec3::NEG_Y);
    let moved = r + right * side * dist;
    let new_r = (radius - fwd * dist).max(0.8);
    let p = target + moved.normalize_or(r / radius) * new_r;
    Vec3::new(p.x, pos.y, p.z)
}
