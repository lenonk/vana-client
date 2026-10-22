//! The game camera (FUN_1001e030): one eye and look point (camera object +0x44 / +0x50) for the free, locked and
//! first-person views. The free branch runs in whole 1/60 s ticks and is drawn between the last two tick states.
//! Spec with addresses: MODLOG "Free camera".

use super::target::LockCam;
use super::World;
use crate::render::Camera;
use glam::Vec3;

/// Camera X (rad, about the look point) and Y (eye height) per tick at full input (_DAT_1031f25c, _DAT_1031f254).
const YAW_RATE: f32 = 0.0279244;
const RISE_RATE: f32 = 0.1066667;
/// The eye rests 3..6 from the look; beyond 6 it is reeled in by D (d - 6) 0.012 a tick.
const REST_NEAR: f32 = 3.0;
const REST_FAR: f32 = 6.0;
/// Ticks of manual hold after camera input (DAT_1044445c).
const MANUAL_HOLD: i32 = 10;
/// Home: ticks of easing (8 retail frames of 2 ticks) toward 3 behind the player.
const HOME_TICKS: i32 = 16;
/// Focal length: config 0x49, 242..900, 6 a tick; both zoom keys ease back to 350. First person uses 280.
const FOCAL_MIN: f32 = 242.0;
const FOCAL_MAX: f32 = 900.0;
const FOCAL_RATE: f32 = 6.0;
const FOCAL_DEFAULT: f32 = 350.0;
const FIRST_FOCAL: f32 = 280.0;
/// A wheel notch zooms as one retail frame of the keys (2 ticks) *(inf: FUN_10253af0)*.
const WHEEL_FOCAL: f32 = FOCAL_RATE * 2.0;

/// The free camera's state beyond the eye and look in `World::cam`.
#[derive(Default)]
pub struct State {
    /// The eye and look one tick ago, and ticks not yet stepped (the view is drawn between).
    prev: LockCam,
    acc: f32,
    hold: i32,
    home: i32,
    /// Chase cam: ticks to wait after manual input, and where the player was then.
    chase_hold: i32,
    chase_from: Vec3,
    /// Last |eye.y - look.y| (_DAT_10444460), for the height memory.
    last_dy: f32,
    zoom_reset: bool,
    zoomed: bool,
    /// Face-camera (keyboard type 2, Shift): the eye offset stored when Shift went down.
    face: Option<Vec3>,
    /// First person (DAT_10475578) and its eased look offsets (pitch, yaw).
    pub first: bool,
    fp: (f32, f32),
    /// Our own model's fade (actor +0x167): out in first person, 0.0625 a tick.
    pub self_alpha: f32,
    started: bool,
}

impl State {
    pub fn new() -> Self {
        Self { self_alpha: 1.0, ..Default::default() }
    }
}

/// This frame's camera input.
#[derive(Default)]
pub struct Inputs {
    /// Axes 6 / 7 (config 0x8b / 0x8c invert, 0x8d / 0x8e in first person).
    pub v6: f32,
    pub v7: f32,
    /// The player's turn this frame from Q / E (radians, our yaw): the camera follows it.
    pub turn: f32,
    pub face_cam: bool,
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub home: bool,
    /// Wheel notches (+ in) and screen-edge panning (radians); mouse drag goes through drag().
    pub wheel: f32,
    pub edge: f32,
}

/// The look point the free camera eases toward: 0.6 of the model height above the feet, at most 2.5.
fn look_target(w: &World) -> Vec3 {
    let p = super::drawn_pos(w);
    match w.actors.get(&w.my_id).map(|a| -a.top).filter(|h| *h > 0.1) {
        Some(h) => p + Vec3::new(0.0, -h.min(2.5) * 0.6, 0.0),
        None if w.camst.started => Vec3::new(p.x, w.cam.look.y, p.z),
        None => p + Vec3::new(0.0, -1.0, 0.0),
    }
}

/// Turns the eye about the look point by `a` of our yaw (FUN_1001dd80; its angle runs the other way).
fn orbit(c: &mut LockCam, a: f32) {
    let v = c.eye - c.look;
    let (s, k) = (-a).sin_cos();
    c.eye = c.look + Vec3::new(v.x * k - v.z * s, v.y, v.x * s + v.z * k);
}

/// Our yaw of a view (look - eye), if it has a horizontal part.
fn view_yaw(c: &LockCam) -> Option<f32> {
    let v = c.look - c.eye;
    (v.x.abs() + v.z.abs() > 1e-4).then(|| v.x.atan2(v.z))
}

/// Puts the camera behind the player facing `yaw` (zone-in, after events, test drives) *(inf: 5 back, 1 up)*.
pub fn face(w: &mut World, yaw: f32) {
    let look = look_target(w);
    let f = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let mut eye = look - f * 5.0 + Vec3::new(0.0, -1.0, 0.0);
    // Not inside the hill behind: in front of what the look -> eye ray hits.
    if let Some(t) = w.ground.as_ref().and_then(|g| g.raycast(look, eye)) {
        eye = look + (eye - look).normalize_or(Vec3::Z) * (t - 0.5).max(0.3);
    }
    w.cam = LockCam { eye, look };
    w.camst.prev = w.cam;
    w.camst.acc = 0.0;
    w.camst.started = true;
    w.yaw = yaw;
}

/// Mouse drag: turns and raises the eye at once (not a rate), with the manual hold, as retail's drag axes do.
pub fn drag(w: &mut World, yaw: f32, rise: f32) {
    for c in [&mut w.cam, &mut w.camst.prev] {
        orbit(c, yaw);
        c.eye.y -= rise;
    }
    w.camst.hold = MANUAL_HOLD;
}

/// Toggles first person (key function 0x10, FUN_100a4580 -> FUN_100a7350): SE 0014 *(caller)*, focal 280 in it.
pub fn toggle_first(w: &mut World) {
    w.camst.first = !w.camst.first;
    w.camst.fp = (0.0, 0.0);
    w.camst.face = None;
    if !w.camst.first {
        w.unlock_ticks = w.unlock_ticks.max(8.0);
    }
}

/// The focal length in use.
pub fn focal(w: &World) -> f32 {
    if w.camst.first { FIRST_FOCAL } else { super::cfgapply::focal(&w.game) }
}

/// One frame of the camera, after the player has moved.
pub fn step(w: &mut World, dt: f32, inp: &Inputs) {
    let ticks = dt * 60.0;
    if !w.camst.started {
        let yaw = w.yaw;
        face(w, yaw);
    }
    let fade = if w.camst.first { -0.0625 } else { 0.0625 };
    w.camst.self_alpha = (w.camst.self_alpha + fade * ticks).clamp(0.0, 1.0);
    if w.locked {
        // The locked branch (target::step_camera) moves w.cam itself, per frame.
        w.camst.prev = w.cam;
        w.camst.acc = 0.0;
        return;
    }
    if w.camst.first {
        first_person(w, ticks, inp);
        return;
    }
    // Direct turns: mouse drag (applied by drag()), screen edge and the player's Q / E turn.
    let turn = inp.edge + inp.turn;
    if turn != 0.0 {
        orbit(&mut w.cam, turn);
        orbit(&mut w.camst.prev, turn);
    }
    w.camst.face = match (inp.face_cam, w.camst.face) {
        (true, None) => Some(w.cam.eye - w.cam.look),
        (true, f) => f,
        (false, _) => None,
    };
    zoom(w, ticks, inp);
    if inp.home && w.camst.home < 1 {
        w.camst.home = HOME_TICKS;
    }
    let target = look_target(w);
    let player = super::drawn_pos(w);
    let facing = w.net.me.lock().map_or(Vec3::Z, |m| super::forward_from_dir(m.dir));
    let chase = w.game.get(0x91) == 1;
    w.camst.acc += ticks;
    let mut n = 0;
    while w.camst.acc >= 1.0 {
        w.camst.acc -= 1.0;
        n += 1;
        if n > 30 {
            w.camst.acc = 0.0;
            break;
        }
        w.camst.prev = w.cam;
        tick(w, target, player, facing, chase, inp);
    }
    if let Some(y) = view_yaw(&w.cam) {
        w.yaw = y;
    }
}

/// One tick of the free branch: look easing, the axes, the spring, chase, height memory, Home and the eye's
/// collision (section 6 without path mode and the side rays).
fn tick(w: &mut World, target: Vec3, player: Vec3, facing: Vec3, chase: bool, inp: &Inputs) {
    let st = &mut w.camst;
    let c = &mut w.cam;
    let start = c.eye;
    c.look += (target - c.look) * 0.25;
    if let Some(off) = st.face {
        c.eye = c.look + off;
    } else {
        if w.unlock_ticks <= 0.0 {
            if inp.v6 != 0.0 {
                orbit(c, -inp.v6 * YAW_RATE);
                st.hold = MANUAL_HOLD;
            }
            if inp.v7 != 0.0 {
                c.eye.y += inp.v7 * RISE_RATE;
                st.hold = MANUAL_HOLD;
            }
        }
        w.unlock_ticks = (w.unlock_ticks - 1.0).max(0.0);
        if w.unlock_ticks <= 0.0 {
            let d_vec = c.look - c.eye;
            let d = d_vec.length();
            // Chase cam (0x91): waits after manual input until the player has moved, then drifts 2.5 % a tick
            // toward d behind the player while 8..120 degrees off.
            if chase {
                if inp.v6.abs() >= 0.01 || inp.turn != 0.0 {
                    st.chase_from = player;
                    st.chase_hold = 4;
                }
                let moved = Vec3::new(player.x - st.chase_from.x, 0.0, player.z - st.chase_from.z).length() > 0.5;
                if st.chase_hold > 0 && moved {
                    st.chase_hold -= 1;
                }
                if st.chase_hold == 0 {
                    let behind = Vec3::new(-facing.x, 0.0, -facing.z).normalize_or_zero() * d;
                    let off = Vec3::new(c.eye.x - c.look.x, 0.0, c.eye.z - c.look.z).normalize_or_zero();
                    let k = behind.normalize_or_zero().dot(off);
                    if k > -0.5 && k < 0.99 {
                        let to = c.look + behind - c.eye;
                        c.eye += Vec3::new(to.x, 0.0, to.z) * 0.025;
                    }
                }
            }
            if spring(c, d_vec) {
                st.hold = MANUAL_HOLD;
            }
        }
        st.hold = (st.hold - 1).max(0);
        // Height memory: walking uphill the eye keeps its height above the look.
        if st.hold == 0 && w.unlock_ticks <= 0.0 {
            let dy = (c.eye.y - c.look.y).abs();
            if dy < st.last_dy && c.eye.y < c.look.y {
                c.eye.y -= st.last_dy - dy;
            }
        }
    }
    // Home (0x51): eases 1/8 a tick toward 3 behind the player at its look height, until the view is behind.
    if st.home > 0 {
        let behind = player - Vec3::new(facing.x, 0.0, facing.z).normalize_or_zero() * 3.0 + Vec3::new(0.0, target.y - player.y, 0.0);
        let off = view_yaw(c).zip(Some(facing.x.atan2(facing.z))).map_or(0.0, |(a, b)| (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI);
        if off.abs() >= 0.05 {
            c.eye += (behind - c.eye) * 0.125;
            st.home -= 1;
        } else {
            st.home = 0;
        }
    }
    if let Some(g) = w.ground.as_ref().filter(|g| !g.is_empty()) {
        // The look -> eye ray puts the eye 0.5 in front of a wall.
        if let Some(t) = g.raycast(c.look, c.eye) {
            c.eye = c.look + (c.eye - c.look).normalize_or(Vec3::Z) * (t - 0.5).max(0.0);
        }
        // 0.17 clear of the floor below (within 10).
        if let Some(f) = g.floor(c.eye.x, c.eye.z, c.eye.y, 0.0).filter(|f| f - c.eye.y <= 10.0)
            && c.eye.y > f - 0.17
        {
            c.eye.y = f - 0.17;
        }
        // The eye never passes through a wall: a sweep from where it was stops 0.2 short. Not from an eye the look
        // could not see (inside geometry), or it would stay there *(inf)*.
        if g.raycast(c.look, start).is_none()
            && let Some(t) = g.raycast(start, c.eye)
        {
            c.eye = start + (c.eye - start).normalize_or(Vec3::Z) * (t - 0.2).max(0.0);
        }
    }
    st.last_dy = (c.eye.y - c.look.y).abs();
}

/// The distance spring, one tick, `d_vec` = look - eye before the chase: reeled in beyond 6 (past 100 by half the
/// excess), pushed out below 3 and to 3 horizontally. True when it pushed (that holds like manual input).
fn spring(c: &mut LockCam, d_vec: Vec3) -> bool {
    let d = d_vec.length();
    if d > 100.0 {
        c.eye += d_vec / d * (d - 100.0) * 0.5;
    } else if d >= REST_FAR {
        c.eye += d_vec * (d - REST_FAR) * 0.012;
    }
    let mut pushed = false;
    if d < REST_NEAR {
        c.eye -= d_vec * (REST_NEAR - d) * 0.125;
        pushed = true;
    }
    let h = Vec3::new(c.eye.x - c.look.x, 0.0, c.eye.z - c.look.z);
    if h.length() < REST_NEAR {
        c.eye += h.normalize_or(Vec3::Z) * (REST_NEAR - h.length()) * 0.125;
        pushed = true;
    }
    pushed
}

/// Section 3: the zoom keys and wheel change the focal length (config 0x49), both keys ease it back to 350.
fn zoom(w: &mut World, ticks: f32, inp: &Inputs) {
    let st = &mut w.camst;
    let mut f = super::cfgapply::focal(&w.game);
    let before = f;
    if inp.zoom_in && inp.zoom_out {
        st.zoom_reset = true;
    }
    if st.zoom_reset {
        // 25 % a retail frame *(inf: per 2 ticks)*.
        let s = (FOCAL_DEFAULT - f) * (1.0 - 0.75f32.powf(ticks / 2.0));
        if (FOCAL_DEFAULT - f).abs() < 1.0 {
            f = FOCAL_DEFAULT;
            st.zoom_reset = false;
        } else {
            f += s;
        }
    } else if inp.zoom_in {
        f += ticks * FOCAL_RATE;
    } else if inp.zoom_out {
        f -= ticks * FOCAL_RATE;
    } else {
        f += inp.wheel * WHEEL_FOCAL;
    }
    let f = f.clamp(FOCAL_MIN, FOCAL_MAX);
    if f != before {
        w.game.set(0x49, f.round() as u32);
        st.zoomed = true;
    } else if std::mem::take(&mut st.zoomed) {
        w.game_save = true;
    }
}

/// Section 7: the eye behind the head, the view where the stick points (eased), springing back when released.
fn first_person(w: &mut World, ticks: f32, inp: &Inputs) {
    let p = super::drawn_pos(w);
    let dir = w.net.me.lock().map_or(0, |m| m.dir);
    let f = super::forward_from_dir(dir);
    let status = w.my_motion.status;
    let joint = if matches!(status, 7 | 0x21 | 0x2c | 0x2f | 0x30 | 0x3f..=0x53) { 6 } else { 12 };
    let h = w.actors.get(&w.my_id).and_then(|a| a.bones.get(joint).map(|b| -b.1.y * a.scale)).unwrap_or(1.5);
    let h = if joint == 12 { (h * 0.91).min(2.0) } else { h };
    let eye = Vec3::new(p.x - f.x * 0.5, p.y - h, p.z - f.z * 0.5);
    // 0.25 of the gap a retail frame *(inf: per 2 ticks)*.
    let k = 1.0 - 0.75f32.powf(ticks / 2.0);
    let st = &mut w.camst;
    st.fp.0 += (inp.v7 * 1.413675 - st.fp.0) * k;
    st.fp.1 += (inp.v6 * 1.8849 - st.fp.1) * k;
    let yaw = f.x.atan2(f.z) + st.fp.1;
    let (pitch, fwd) = (st.fp.0, Vec3::new(yaw.sin(), 0.0, yaw.cos()));
    let look = eye + fwd * 5.0 * pitch.cos() + Vec3::new(0.0, 5.0 * pitch.sin() + 0.25, 0.0);
    w.unlock_ticks = 8.0;
    st.chase_hold = 0;
    // The camera eases 0.125 a tick toward the head.
    let e = 1.0 - 0.875f32.powf(ticks);
    for (c, t) in [(&mut w.cam.eye, eye), (&mut w.cam.look, look)] {
        if (t - *c).length() > 0.01 {
            *c += (t - *c) * e;
        }
    }
    st.prev = w.cam;
    st.acc = 0.0;
}

/// The view to draw: the locked camera as stepped, else between the last two free ticks.
pub fn view(w: &World) -> Camera {
    let (s, c) = (&w.camst, &w.cam);
    let t = s.acc.clamp(0.0, 1.0);
    let (eye, look) = (s.prev.eye.lerp(c.eye, t), s.prev.look.lerp(c.look, t));
    Camera { eye, target: look, up: Vec3::NEG_Y, fov_y: crate::dat::title::fov_y(focal(w)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's rotation turns the view the other way from our yaw, and keeps the eye's distance and height.
    #[test]
    fn orbit_keeps_radius_and_height() {
        let mut c = LockCam { eye: Vec3::new(0.0, -2.0, -5.0), look: Vec3::ZERO };
        let before = view_yaw(&c).unwrap();
        orbit(&mut c, 0.3);
        assert!((view_yaw(&c).unwrap() - before - 0.3).abs() < 1e-5);
        assert!((Vec3::new(c.eye.x, 0.0, c.eye.z).length() - 5.0).abs() < 1e-5 && c.eye.y == -2.0);
    }

    /// Far eyes are reeled in to 6 along their line, near ones pushed out to 3; between, the eye stays.
    #[test]
    fn spring_rests_between_3_and_6() {
        for (start, rest) in [(12.0f32, 6.0f32), (1.5, 3.0), (4.5, 4.5)] {
            let mut c = LockCam { eye: Vec3::new(0.0, 0.0, -start), look: Vec3::ZERO };
            for _ in 0..2000 {
                let d = c.look - c.eye;
                spring(&mut c, d);
            }
            let d = (c.eye - c.look).length();
            assert!((d - rest).abs() < 0.02, "{start} -> {d}");
            assert!(c.eye.x.abs() < 1e-4 && c.eye.z < 0.0);
        }
    }
}
