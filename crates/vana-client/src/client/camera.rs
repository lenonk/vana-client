//! The game camera (FUN_1001ee60 [FUN_1001e030]): one eye and look point (camera object +0x44 / +0x50) for the free,
//! locked and first-person views. The free branch runs in whole 1/60 s ticks and is drawn between the last two tick
//! states. Specs: MODLOG "Free camera", ~/ffxi/lab/research/camera_collision_spec.md (the collision).

use super::ground::Ground;
use super::target::LockCam;
use super::World;
use crate::render::Camera;
use glam::Vec3;

/// Camera X (rad, about the look point) and Y (eye height) per tick at full input (_DAT_1031f25c, _DAT_1031f254).
pub const YAW_RATE: f32 = 0.0279244;
pub const RISE_RATE: f32 = 0.1066667;
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
    /// Effect op 0x5f's view shake.
    pub shake: Shake,
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
    /// Path mode (DAT_10456d70): the look's last four points (+0xbc, newest first), how many were recorded (+0xf0, a
    /// signed char), and the eye retracing them.
    path: [Vec3; 4],
    path_n: i8,
    path_mode: bool,
    /// DAT_10456d80: ticks after a zone load that run the floor / ceiling / bar tests with the eye at rest.
    zone_in: i32,
    /// Ours (the wheel, as modern MMOs): the rest distances' scale (0 = 1, retail's 3..6); and in first person the
    /// mouse's look (pitch, absolute yaw) once dragged.
    pub dist: f32,
    fp_mouse: Option<(f32, f32)>,
}

/// The wheel's distance range: retail's rest 3..6 at 1.
pub const DIST: (f32, f32) = (0.5, 3.0);

fn scale(st: &State) -> f32 {
    if st.dist > 0.0 { st.dist } else { 1.0 }
}

/// A wheel's notches (+ in): the eye moves in or out about the look at once, 15 % a notch; fully in, a notch more
/// goes to first person, and out of it the first notch back. Locked, nothing. True when it changed something.
pub fn zoom_distance(w: &mut World, notches: f32) -> bool {
    if w.locked || notches == 0.0 {
        return false;
    }
    if w.camst.first {
        if notches < 0.0 {
            toggle_first(w);
            return true;
        }
        return false;
    }
    let k = scale(&w.camst);
    if notches > 0.0 && k <= DIST.0 + 1e-3 {
        toggle_first(w);
        return true;
    }
    let to = (k * 0.85f32.powf(notches)).clamp(DIST.0, DIST.1);
    for c in [&mut w.cam, &mut w.camst.prev] {
        c.eye = c.look + (c.eye - c.look) * (to / k);
    }
    w.camst.dist = to;
    w.camst.hold = MANUAL_HOLD;
    true
}

/// First person: the mouse turns the look (radians; pitch kept within 80 degrees).
pub fn first_look(w: &mut World, dyaw: f32, dpitch: f32) {
    let st = &mut w.camst;
    let (p, y) = st.fp_mouse.unwrap_or_else(|| {
        let f = super::forward_from_dir(w.net.me.lock().map_or(0, |m| m.dir));
        (0.0, f.x.atan2(f.z))
    });
    st.fp_mouse = Some(((p + dpitch).clamp(-1.4, 1.4), y + dyaw));
}

/// The view shake (camera +0x7c offset, +0x80 speed, +0x84 scale; effect op 0x5f's kicks), stepped in retail frames
/// of 2 ticks, held between them.
#[derive(Default)]
pub struct Shake {
    x: f32,
    speed: f32,
    scale: f32,
    acc: f32,
    seed: u32,
}

impl Shake {
    /// FUN_10021ba0: x and the scale only when at rest; the speed when a kick outruns it.
    fn kick(&mut self, a: f32) {
        if self.speed == 0.0 {
            (self.scale, self.x) = (1.0, a);
        }
        if 2.0 * self.speed.abs() < a {
            self.speed = a * 0.125;
        }
    }

    /// FUN_10021900 over `n` ticks: x runs toward 0; crossing it, 0.86 of the overshoot bounces back at an eighth of
    /// it a tick with a new scale in [0, 2) (FUN_1006bfd0); it stops once both are under 1e-4.
    fn step(&mut self, n: f32) {
        if self.x == 0.0 && self.speed == 0.0 {
            return;
        }
        let s = n * self.speed;
        if self.x > 0.0 {
            self.x -= s;
            if self.x >= 0.0 {
                return;
            }
        } else {
            self.x += s;
            if self.x <= 0.0 {
                return;
            }
        }
        if self.x.abs() < 1e-4 && self.speed.abs() < 1e-4 {
            (self.x, self.speed) = (0.0, 0.0);
            return;
        }
        self.x *= 0.86;
        self.speed = (self.x * 0.125).abs();
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.scale = ((self.seed >> 8) % 200_000) as f32 * 1e-5;
    }

    /// FUN_10021c10: the unit look direction's y nudged by scale x, at the view build only.
    pub fn apply(&self, c: &mut Camera) {
        if self.x == 0.0 {
            return;
        }
        let v = c.target - c.eye;
        let d = (v.normalize_or_zero() + Vec3::Y * (self.scale * self.x)).normalize_or_zero();
        c.target = c.eye + d * v.length();
    }
}

/// Every frame, every view: this frame's op 0x5f kicks, then the integrator, in 2-tick steps (stock retail's n = 2).
pub fn shake_step(w: &mut World, dt: f32) {
    let sh = &mut w.camst.shake;
    sh.acc += dt * 60.0;
    while sh.acc >= 2.0 {
        sh.acc -= 2.0;
        for &a in &w.effects.quakes {
            sh.kick(a);
        }
        sh.step(2.0);
        if !w.effects.quakes.is_empty() {
            log::debug!("camera shake: kicks {:?} x {:.4} speed {:.4} scale {:.2}", w.effects.quakes, sh.x, sh.speed, sh.scale);
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self { self_alpha: 1.0, ..Default::default() }
    }

    /// With the wheel's distance kept from the last session.
    pub fn with_dist(dist: f32) -> Self {
        Self { dist: dist.clamp(DIST.0, DIST.1), ..Self::new() }
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
    // FUN_1001ee60 riding: the riding box's height plus the mount's camera add, x 0.6, no cap.
    if let Some(h) = super::ride::look_height(w, w.my_id, w.my_motion.status) {
        return p + Vec3::new(0.0, -h * 0.6, 0.0);
    }
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

/// Puts the camera behind the player facing `yaw` (zone-in, after events, test drives): FUN_10021680's (-4, -2, 0) in
/// the player's frame, (-3, -1.5, 0) in the Mog House (FUN_10096640).
pub fn face(w: &mut World, yaw: f32) {
    let o = if w.myroom.is_some() { Vec3::new(-3.0, -1.5, 0.0) } else { Vec3::new(-4.0, -2.0, 0.0) };
    face_at(w, yaw, o);
}

/// FUN_10021680: the eye at `o` in the player's frame (x forward, y down, z to the side *(inf: its sense)*).
pub fn face_at(w: &mut World, yaw: f32, o: Vec3) {
    // Without a model yet (a login's zone-in) the look is 1 above the feet, not the old look's height.
    let look = if w.actors.get(&w.my_id).is_some_and(|a| -a.top > 0.1) { look_target(w) } else { super::drawn_pos(w) + Vec3::new(0.0, -1.0, 0.0) };
    let f = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let side = Vec3::new(f.z, 0.0, -f.x);
    let mut eye = super::drawn_pos(w) + f * o.x + side * o.z + Vec3::new(0.0, o.y, 0.0);
    // Not inside the hill behind: in front of what the look -> eye ray hits.
    if let Some(h) = w.ground.as_ref().and_then(|g| g.cam_ray(look, eye)) {
        eye = look + (eye - look).normalize_or(Vec3::Z) * ((h - look).length() - 0.5).max(0.3);
    }
    w.cam = LockCam { eye, look };
    w.camst.prev = w.cam;
    w.camst.acc = 0.0;
    w.camst.started = true;
    // FUN_10184380 -> FUN_1001d480: 60 frames *(inf: also after events and drives, which face() serves too)*. The
    // height memory starts from this view *(inf)*.
    (w.camst.zone_in, w.camst.path_n, w.camst.path_mode) = (120, 0, false);
    w.camst.last_dy = (eye.y - look.y).abs();
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

/// On a lift (FUN_1001ee60, all.c 22907): the eye rides with the player's height change, and a change holds the
/// automatic correction for 10 ticks.
pub fn ride(w: &mut World, dy: f32) {
    if dy != 0.0 {
        w.cam.eye.y += dy;
        w.camst.prev.eye.y += dy;
        w.camst.hold = MANUAL_HOLD;
    }
}

/// Toggles first person (key function 0x10, FUN_100a4580 -> FUN_100a7350): SE 0014 *(caller)*, focal 280 in it.
pub fn toggle_first(w: &mut World) {
    w.camst.first = !w.camst.first;
    w.camst.fp = (0.0, 0.0);
    w.camst.face = None;
    w.camst.fp_mouse = None;
    if !w.camst.first {
        w.unlock_ticks = w.unlock_ticks.max(8.0);
    }
}

/// The focal length in use.
pub fn focal(w: &World) -> f32 {
    if let Some(l) = &w.layout {
        // DAT_1037eb94[1] / [4]: layout mode's focal, 300 while dressing a mannequin.
        if l.dress.is_some() { 300.0 } else { 250.0 }
    } else if w.camst.first {
        FIRST_FOCAL
    } else {
        super::cfgapply::focal(&w.game)
    }
}

/// One frame of the camera, after the player has moved.
pub fn step(w: &mut World, dt: f32, inp: &Inputs) {
    if super::layout::camera(w, dt * 60.0) {
        w.camst.prev = w.cam;
        return;
    }
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
    // The frame's start eye (slot +0xd4) is taken before any of its turns; the collision tail's gate measures from it.
    let mut frame_start = Some(w.cam.eye);
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
        tick(w, target, player, facing, chase, inp, frame_start.take());
    }
    if let Some(y) = view_yaw(&w.cam) {
        w.yaw = y;
    }
}

/// One tick of the free branch: look easing, the axes, the spring (or path mode), chase, height memory, the look ->
/// eye ray (A), Home and the shared collision tail. `frame_start`: the eye before this frame's direct turns (first tick).
fn tick(w: &mut World, target: Vec3, player: Vec3, facing: Vec3, chase: bool, inp: &Inputs, frame_start: Option<Vec3>) {
    // _DAT_10456d7c: (A) and the final ray wait for the unlock hold (20 after the lock, 8 after first person).
    let unlocked = w.unlock_ticks <= 0.0;
    let ground = w.ground.as_ref().filter(|g| !g.is_empty());
    let st = &mut w.camst;
    let k = scale(st);
    let c = &mut w.cam;
    let start = frame_start.unwrap_or(c.eye);
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
        if st.path_mode {
            path_tick(st, c);
        } else if w.unlock_ticks <= 0.0 {
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
            // 2019 0x1001eb4a: with chase cam on and the eye 1..3 out horizontally, a wall within 3.1 out from the look
            // stops the spring's push this tick.
            let h = Vec3::new(c.eye.x - c.look.x, 0.0, c.eye.z - c.look.z);
            let push = !(chase && h.length() > 1.0 && h.length() < REST_NEAR * k && ground.is_some_and(|g| g.cam_ray(c.look, c.look + h.normalize() * 3.1 * k).is_some()));
            if spring(c, d_vec, push, k) {
                st.hold = MANUAL_HOLD;
            }
        }
        st.hold = (st.hold - 1).max(0);
        // Height memory: walking uphill the eye keeps its height above the look.
        if !st.path_mode && st.hold == 0 && w.unlock_ticks <= 0.0 {
            let dy = (c.eye.y - c.look.y).abs();
            if dy < st.last_dy && c.eye.y < c.look.y {
                c.eye.y -= st.last_dy - dy;
            }
        }
        // (A) 2019 0x1001f0cc: the eye 0.5 in front of what the look -> eye ray hits; with the view blocked after the
        // look has moved on (4 points recorded) and no camera input, path mode instead. An unblocked view records the
        // look's trail.
        if !st.path_mode && unlocked && let Some(g) = ground {
            match g.cam_ray(c.look, c.eye) {
                Some(_) if st.hold == 0 && st.path_n >= 4 => (st.path_mode, st.hold) = (true, 0),
                Some(h) => c.eye = h + (h - c.eye).normalize_or_zero() * 0.5,
                // FUN_1001e320: a point 0.1 or more from the newest.
                None if (c.look - st.path[0]).length() >= 0.1 => {
                    st.path = [c.look, st.path[0], st.path[1], st.path[2]];
                    st.path_n = st.path_n.wrapping_add(1);
                }
                None => {}
            }
        }
    }
    // Home (0x51): eases 1/8 a tick toward 3 behind the player at its look height, until the view is behind.
    if st.home > 0 {
        let behind = player - Vec3::new(facing.x, 0.0, facing.z).normalize_or_zero() * 3.0 * k + Vec3::new(0.0, target.y - player.y, 0.0);
        let off = view_yaw(c).zip(Some(facing.x.atan2(facing.z))).map_or(0.0, |(a, b)| (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI);
        if off.abs() >= 0.05 {
            c.eye += (behind - c.eye) * 0.125;
            st.home -= 1;
        } else {
            st.home = 0;
        }
    }
    if let Some(g) = ground {
        c.eye = tail(g, c.eye, c.look, start, st.zone_in > 0, unlocked);
    }
    st.zone_in = (st.zone_in - 1).max(0);
    st.last_dy = (c.eye.y - c.look.y).abs();
}

/// Path mode (2019 0x1001e7e9), in place of chase, spring, height memory and (A): camera input ends it; otherwise the
/// eye eases 5 % a tick toward the oldest trail point and stops within 0.05 of it, or when the look is 10 away.
fn path_tick(st: &mut State, c: &mut LockCam) {
    if st.hold != 0 {
        st.path_n = 0;
    }
    if st.path_n < 4 {
        (st.path_n, st.path_mode) = (0, false);
        return;
    }
    let p = st.path[3];
    if (p - c.eye).length() <= 0.05 {
        (st.path_n, st.path_mode) = (0, false);
    } else {
        c.eye += (p - c.eye) * 0.05;
    }
    if (p - c.look).length() > 10.0 {
        (st.path_n, st.path_mode) = (0, false);
    }
}

/// FUN_1001ee60's tail for the free and locked branches: when the eye moved this frame (or just after a zone load),
/// (B) 0.17 clear of the floor below or else of the ceiling above (each within 10) and (C) the side bar; then (D) on
/// the free branch outside the unlock hold, the eye 0.2 in front of what the look -> eye ray hits; (E) a move under
/// 0.0001 snaps back. Nothing refuses a move: the eye always lands where the look can see it.
pub(super) fn tail(g: &Ground, mut eye: Vec3, look: Vec3, start: Vec3, zone_in: bool, final_ray: bool) -> Vec3 {
    // 2019 0x1001f958: |eye - start| > 0.01 a frame *(inf: 0.005 a tick)*.
    if (eye - start).length() > 0.005 || zone_in {
        let floor = g.floor(eye.x, eye.z, eye.y, 0.0).filter(|f| f - eye.y <= 10.0);
        match floor {
            Some(f) if eye.y > f - 0.17 => eye.y = f - 0.17,
            _ => {
                let up = Vec3::new(eye.x + 1e-6, eye.y, eye.z + 1e-6);
                if let Some(h) = g.cam_ray(up, up - Vec3::new(0.0, 10.0, 0.0))
                    && eye.y < h.y + 0.17
                {
                    eye.y = h.y + 0.17;
                }
            }
        }
        eye = side_bar(g, eye, look);
    }
    if final_ray && let Some(h) = g.cam_ray(look, eye) {
        eye = h + (h - eye).normalize_or_zero() * 0.2;
    }
    if (eye - start).length() < 0.0001 { start } else { eye }
}

/// (C) 2019 0x1001fa16: a bar of +-0.17 along the camera's x through the eye; crossing two faces (a pole, a corner, a
/// thin wall) the eye moves sideways until the bar's +x end touches the face nearest its -x end. One face crossing it
/// (the same point from both ends) changes nothing *(inf: within 1e-4, retail compares exactly in camera space)*.
fn side_bar(g: &Ground, eye: Vec3, look: Vec3) -> Vec3 {
    let x = Vec3::NEG_Y.cross(eye - look).normalize_or_zero();
    if x == Vec3::ZERO {
        return eye;
    }
    let (l, r) = (eye - x * 0.17, eye + x * 0.17);
    let (h1, h2) = (g.cam_ray(r, l), g.cam_ray(l, r));
    if let (Some(a), Some(b)) = (h1, h2)
        && (a - b).length() < 1e-4
    {
        return eye;
    }
    let mut out = eye;
    if let Some(h) = h1 {
        out = eye + x * ((h - eye).dot(x) + 0.17);
    }
    if let Some(h) = h2 {
        out = eye + x * ((h - eye).dot(x) - 0.17);
    }
    out
}

/// The distance spring, one tick, `d_vec` = look - eye before the chase: reeled in beyond 6 (past 100 by half the
/// excess), pushed out below 3 and to 3 horizontally unless `push` is off; both rests scaled by `k` (the wheel's
/// distance, ours). True when it pushed (that holds like manual input).
fn spring(c: &mut LockCam, d_vec: Vec3, push: bool, k: f32) -> bool {
    let (near, far) = (REST_NEAR * k, REST_FAR * k);
    let d = d_vec.length();
    if d > 100.0 {
        c.eye += d_vec / d * (d - 100.0) * 0.5;
    } else if d >= far {
        c.eye += d_vec * (d - far) * 0.012;
    }
    let mut pushed = false;
    if !push {
        return pushed;
    }
    if d < near {
        c.eye -= d_vec * (near - d) * 0.125;
        pushed = true;
    }
    let h = Vec3::new(c.eye.x - c.look.x, 0.0, c.eye.z - c.look.z);
    if h.length() < near {
        c.eye += h.normalize_or(Vec3::Z) * (near - h.length()) * 0.125;
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
    // FUN_10020ba0: attach point 6 resting or seated, else point 12 (vt+0x1c4, the posed point), 0.91 x its height
    // at most 2.
    let k = if matches!(status, 7 | 0x21 | 0x2c | 0x2f | 0x30 | 0x3f..=0x53) { 6 } else { 12 };
    let point = |a: &super::Actor, k: usize| {
        let (bone, off) = *a.character.as_ref()?.skeleton.points.get(k)?;
        let bones = if a.bones.is_empty() { &a.rest } else { &a.bones };
        bones.get(bone as usize).map(|(q, t)| -(*t + *q * off).y * a.scale)
    };
    let me = w.actors.get(&w.my_id);
    let h = if matches!(super::ride::applied(w, w.my_id, status), 5 | 0x55) {
        // Riding: 0.91 x point 5 over point 1, plus 1.6 x the animal's size, at most 2.
        let hs = me.and_then(|a| Some((point(a, 5)?, point(a, 1)?)));
        hs.map_or(1.5, |(p5, p1)| (0.91 * (p5 - p1) + 1.6 * super::ride::animal_scale(w, w.my_id)).min(2.0))
    } else {
        let h = me.and_then(|a| point(a, k)).unwrap_or(1.5);
        if k == 12 { (h * 0.91).min(2.0) } else { h }
    };
    let eye = Vec3::new(p.x - f.x * 0.5, p.y - h, p.z - f.z * 0.5);
    // 0.25 of the gap a retail frame *(inf: per 2 ticks)*.
    let k = 1.0 - 0.75f32.powf(ticks / 2.0);
    let st = &mut w.camst;
    st.fp.0 += (inp.v7 * 1.413675 - st.fp.0) * k;
    st.fp.1 += (inp.v6 * 1.8849 - st.fp.1) * k;
    // The mouse's look, once dragged, stands for the facing (and the keys still add theirs); moves go that way.
    let (yaw, pitch) = match st.fp_mouse {
        Some((mp, my)) => (my + st.fp.1, (mp + st.fp.0).clamp(-1.4, 1.4)),
        None => (f.x.atan2(f.z) + st.fp.1, st.fp.0),
    };
    if st.fp_mouse.is_some() {
        w.yaw = yaw;
    }
    let st = &mut w.camst;
    let fwd = Vec3::new(yaw.sin(), 0.0, yaw.cos());
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
    // Not glam's lerp (a(1 - t) + bt): a camera at rest must not wobble in the last bit with t.
    let (eye, look) = (s.prev.eye + (c.eye - s.prev.eye) * t, s.prev.look + (c.look - s.prev.look) * t);
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
    /// File 3445 gob_/vib1 (factor 0.0003, inside near): the first frames of fx_shake_spec.md 2.8, x and speed after
    /// each 2-tick step.
    #[test]
    fn shake_follows_retail_frames() {
        let mut sh = Shake::default();
        let want = [(239, 0.0538, 0.0090), (239, 0.0359, 0.0090), (239, 0.0179, 0.0090), (224, 0.0011, 0.0084), (197, -0.0117, 0.0015)];
        for (v, x, speed) in want {
            sh.kick(v as f32 * 0.0003);
            sh.step(2.0);
            assert!((sh.x - x).abs() < 2e-4 && (sh.speed - speed).abs() < 2e-4, "{} {} want {x} {speed}", sh.x, sh.speed);
        }
    }

    #[test]
    fn spring_rests_between_3_and_6() {
        for (start, rest) in [(12.0f32, 6.0f32), (1.5, 3.0), (4.5, 4.5)] {
            let mut c = LockCam { eye: Vec3::new(0.0, 0.0, -start), look: Vec3::ZERO };
            for _ in 0..2000 {
                let d = c.look - c.eye;
                spring(&mut c, d, true, 1.0);
            }
            let d = (c.eye - c.look).length();
            assert!((d - rest).abs() < 0.02, "{start} -> {d}");
            assert!(c.eye.x.abs() < 1e-4 && c.eye.z < 0.0);
        }
    }
}
