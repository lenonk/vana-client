//! Targeting and lock-on (target object DAT_10565d34, FUN_10151280 / FUN_10150da0), Numpad * (key
//! functions 0x44 / 0x30), the s2c 0x058 assist target, and the locked camera of FUN_1001e030.

use super::World;
use crate::audio::Ui;
use crate::render::Camera;
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
    w.target = t;
    w.target_since = std::time::Instant::now();
    if w.locked {
        lock(w, false);
    }
}

/// FUN_10150da0: lock needs a target, a living player and a target that is not us.
pub fn lock(w: &mut World, on: bool) {
    if on == w.locked {
        return;
    }
    if on {
        let ok = w.target.is_some_and(|t| t != w.my_id && w.entities.contains_key(&t)) && !w.dead();
        if !ok {
            return;
        }
        // The locked camera starts from the camera on screen.
        let c = match w.release.take() {
            Some(r) => r,
            None => {
                let c = super::App::free_camera(w);
                LockCam { eye: c.eye, look: c.target }
            }
        };
        w.cam = Some(c);
        w.locked = true;
    } else {
        w.locked = false;
        log::info!("lock off");
        // Nothing snaps: the same eye and look go on in the free branch (FUN_1001e030).
        w.release = w.cam.take();
        w.unlock_ticks = 20.0;
    }
}

/// Why the target goes (FUN_10151bb0 -> FUN_10152c70), checked every frame; Some(true) also sends /attackoff
/// (FUN_10151c40, +0x130 bit 7).
fn invalid(w: &World, t: u32) -> Option<bool> {
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
        lock(w, on);
    }
}

/// s2c 0x058 (LAB_1009e570): our assist target; locks on when the target is a monster and auto
/// lock-on (config 7 = 0, the default) is on.
pub fn assist(w: &mut World, player: u32, target: u32) {
    if player != w.my_id || target == 0 {
        return;
    }
    // +0x120 bit 9: the actor has a model.
    if !w.entities.contains_key(&target) || !w.actors.get(&target).is_some_and(|a| a.gpu.is_some() || a.character.is_some()) {
        return;
    }
    set(w, Some(target));
    // Inferred: +0x120 bit 13 (attackable) is the monster flag.
    if w.auto_lock && w.entities.get(&target).is_some_and(|e| e.mob) {
        lock(w, true);
    }
}

/// Per frame (FUN_10151100): own death unlocks (FUN_10150e30); leaving the engaged status (not by dying)
/// unlocks and keeps the target (FUN_10089d50 case 3); an invalid target goes at once with the lock; engaged
/// with no target picks one (FUN_10151910).
pub fn update(w: &mut World) {
    let st = w.my_motion.status;
    if w.locked && (w.dead() || (w.last_status == 1 && st != 1 && st != 3)) {
        lock(w, false);
    }
    w.last_status = st;
    if let Some(attackoff) = w.target.and_then(|t| invalid(w, t)) {
        log::info!("target {:#x} dropped (attackoff {attackoff})", w.target.unwrap_or(0));
        lock(w, false);
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
        lock(w, false);
    }
}

/// The free branch of FUN_1001e030 after an unlock, in 1/60 s ticks: for 20 ticks the eye holds and the look
/// eases 25 % a tick toward the player's look point; then the eye is pulled in by (look - eye) (d - rest)
/// 0.012 a tick (rest 6 in retail; ours the free camera's distance) and pushed out below 3. Hands over to the
/// yaw / pitch orbit once it has settled or on manual input (inferred: ours has no spring camera).
pub fn step_release(w: &mut World, ticks: f32, head: Vec3, input: bool) {
    let Some(c) = w.release else { return };
    let hold = w.unlock_ticks > 0.0;
    let (c, d) = release_step(c, head, ticks, hold, w.distance);
    w.release = Some(c);
    if !hold && (input || (d - w.distance).abs() < 0.05 || (d <= w.distance && (head - c.look).length() < 0.01)) {
        hand_over(w);
    }
}

/// One step of the release camera; returns it and the eye-to-look distance before the step.
fn release_step(mut c: LockCam, head: Vec3, ticks: f32, hold: bool, rest: f32) -> (LockCam, f32) {
    c.look += (head - c.look) * (1.0 - 0.75f32.powf(ticks));
    let v = c.look - c.eye;
    let d = v.length().max(1e-3);
    if !hold {
        if d > 100.0 {
            c.eye += v / d * (d - 100.0) * (0.5 * ticks).min(1.0);
        } else if d > rest {
            c.eye += v * ((d - rest) * 0.012 * ticks).min(0.5);
        } else if d < 3.0 {
            c.eye -= v * ((3.0 - d) * 0.125 * ticks).min(0.5);
        }
    }
    (c, d)
}

/// Ends the release: yaw, pitch and distance from its eye and look.
pub fn hand_over(w: &mut World) {
    let Some(c) = w.release.take() else { return };
    let v = c.look - c.eye;
    let d = v.length().max(1e-3);
    if v.x.abs() + v.z.abs() > 1e-4 {
        w.yaw = v.x.atan2(v.z);
    }
    w.pitch = ((c.look.y - c.eye.y) / d).clamp(-1.0, 1.0).asin().clamp(-0.4, 1.4);
    w.distance = d.clamp(2.0, 30.0);
}

/// The locked camera's state: eye and look-at (camera object +0x44 / +0x50).
#[derive(Clone, Copy, Debug)]
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
    let (Some(t), Some(mut c)) = (w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.pos))), w.cam) else { return };
    let (tid, tpos) = t;
    let p = w.pos + Vec3::new(0.0, look_height(w, w.my_id, 13, false), 0.0);
    let q = tpos + Vec3::new(0.0, look_height(w, tid, 5, true), 0.0);
    let l = p.lerp(q, 0.5);
    let k = 1.0 - 0.75f32.powf(ticks);
    c.look += (l - c.look) * k;
    let d = (w.pos - tpos).length();
    let e = (w.pos - tpos).normalize_or(Vec3::X);
    let mut dv = c.eye - c.look;
    // Manual orbit, scaled by 6 / |D| when locked.
    if turn != 0.0 {
        let len = dv.length().max(0.1);
        dv = glam::Quat::from_rotation_y(turn * (6.0 / len).min(3.0)) * dv;
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
    let from = w.pos + Vec3::new(0.0, -1.0, 0.0);
    if let Some(hit) = w.ground.as_ref().and_then(|g| g.raycast(from, eye)) {
        eye = from + (eye - from).normalize_or(Vec3::X) * (hit - 0.2).max(0.2);
    }
    c.eye = eye;
    w.cam = Some(c);
}

pub fn camera(w: &World) -> Option<Camera> {
    let c = w.cam.filter(|_| w.locked).or(w.release)?;
    let mut eye = c.eye;
    // The release's wall ray from the look point; the eye 0.5 in front of the hit.
    if !w.locked
        && let Some(t) = w.ground.as_ref().and_then(|g| g.raycast(c.look, c.eye))
    {
        eye = c.look + (c.eye - c.look).normalize_or(Vec3::X) * (t - 0.5).max(0.3);
    }
    Some(Camera { eye, target: c.look, up: Vec3::NEG_Y, fov_y: crate::dat::title::fov_y(super::cfgapply::focal(&w.game)) })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The eye holds while the hold ticks run, then the spring pulls it in to the rest distance, keeping
    /// its direction; the look follows the player.
    #[test]
    fn release_spring() {
        let head = Vec3::new(0.0, -1.5, 0.0);
        let start = LockCam { eye: Vec3::new(0.0, -4.0, -12.0), look: Vec3::new(2.0, -1.5, 3.0) };
        let mut c = start;
        for _ in 0..20 {
            c = release_step(c, head, 1.0, true, 6.0).0;
        }
        assert_eq!(c.eye, start.eye);
        assert!((c.look - head).length() < 0.02);
        let dir = (c.eye - c.look).normalize();
        for _ in 0..600 {
            c = release_step(c, head, 1.0, false, 6.0).0;
        }
        let d = c.eye - c.look;
        assert!((d.length() - 6.0).abs() < 0.1, "{}", d.length());
        assert!(d.normalize().dot(dir) > 0.999);
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
