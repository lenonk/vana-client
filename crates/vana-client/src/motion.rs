//! Which motion an actor plays and how fast, as FFXiMain decides it: FUN_100c68f0 (movement ratio),
//! FUN_100c6b70 (motion name and rate), FUN_100c6380 (switch, 16-tick blend), motion player
//! 0x10019ab0 (frame advance). Client ticks are 1/60 s (FUN_10014000, at least 1 per frame).

/// Ticks a motion change blends over (FUN_100c6380).
pub const BLEND_TICKS: f32 = 16.0;

/// Frames per second of a motion: frame += ticks * speed * 0.5 * rate.
pub fn frames_per_second(speed: f32, rate: f32) -> f32 {
    60.0 * speed * 0.5 * rate
}

/// Looping frame position: the client wraps at `frames` (the keys less one), so the last interval runs into the
/// closing key, which repeats the first.
pub fn loop_frame(elapsed_frames: f32, frames: u16) -> f32 {
    if frames == 0 { 0.0 } else { elapsed_frames.rem_euclid(frames as f32) }
}

/// Movement ratio of a remote actor from how far it moved this frame (FUN_100c68f0). `speed` and
/// `speed_base` are the 0x00D/0x00E bytes @0x1C/@0x1D (x0.1 = units per second; the ratio uses
/// SpeedBase). Stops at once below 1e-4 units, else smooths 0.25 new + 0.75 old, capped at 3.
pub fn move_ratio(prev: f32, moved: f32, dt_secs: f32, speed: u8, speed_base: u8, status: u8) -> f32 {
    if moved < 1e-4 {
        return 0.0;
    }
    // Riding (status 5 chocobo, 85 mount) expects 1.6x the distance.
    let k = if status == 5 || status == 85 { 1.6 } else { 1.0 };
    let expected = dt_secs * 60.0 * speed_base as f32 * 0.1 * k / 60.0;
    let r = if expected >= 1e-6 && speed > 0 { moved / expected } else { 0.0 };
    (r * 0.25 + prev * 0.75).min(3.0)
}

/// FUN_100c68f0 against a reference in units per second: bound event actors use 0.8 x their VM body's
/// +0x16c (FUN_1008cce0, _DAT_1031e8a4 = 0.8), 5.0 at bind, so a set_speed 13 walk plays at about 0.98.
pub fn move_ratio_ref(prev: f32, moved: f32, dt_secs: f32, reference: f32) -> f32 {
    if moved < 1e-4 {
        return 0.0;
    }
    let expected = dt_secs * reference;
    let r = if expected >= 1e-6 { moved / expected } else { 0.0 };
    (r * 0.25 + prev * 0.75).min(3.0)
}

/// An actor stand/battle/walk/run motion names; defaults idl/btl/wlk/run (FUN_100c3d20 at 0x100c414d)
/// (constructor 0x100a9b60); scheduler ops 0x8c, 0x79, 0xa5, 0xa4 pick a weapon set 0..3 from the
/// tables at 0x10320684 / 0x10320674 / 0x103206a4 / 0x10320694.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionNames {
    pub idle: &'static str,
    pub battle: &'static str,
    pub walk: &'static str,
    pub run: &'static str,
}

impl Default for MotionNames {
    fn default() -> Self {
        Self { idle: "idl", battle: "btl", walk: "wlk", run: "run" }
    }
}

impl MotionNames {
    /// Applies a scheduler op (0x8c idle, 0x79 battle, 0xa5 walk, 0xa4 run) with its set index.
    pub fn apply(&mut self, op: u8, set: u32) {
        let pick = |t: [&'static str; 4]| t[(set as usize).min(3)];
        match op {
            0x8c => self.idle = pick(["idl", "1dl", "2dl", "3dl"]),
            0x79 => self.battle = pick(["btl", "1tl", "2tl", "3tl"]),
            0xa5 => self.walk = pick(["wlk", "1lk", "2lk", "3lk"]),
            0xa4 => self.run = pick(["run", "1un", "2un", "3un"]),
            _ => {}
        }
    }
}

/// Motion name (3 letters plus the region digit, "idl" -> idl0/idl1/idl2) and playback rate for a
/// status (0x00D/0x00E byte @0x1F, LSB `Animation`) and movement ratio (FUN_100c6b70). The client
/// also falls back from "id1" to "idl" when the first is missing (FUN_100c6380).
/// FUN_100ca9b0: status motions an op 0x2e's end leaves playing (the first three letters).
pub fn keeps_status_motion(n: &str) -> bool {
    matches!(n.get(..3).unwrap_or(n), "idl" | "1dl" | "2dl" | "3dl" | "btl" | "1tl" | "2tl" | "3tl" | "cor" | "rx1")
}

pub fn choose(names: &MotionNames, status: u8, ratio: f32) -> (&'static str, f32) {
    match status {
        2 | 3 => return ("cor", 1.0),
        6 | 38..=43 | 50..=53 | 56..=62 => return ("fh1", 1.0),
        7 | 33 => return ("rx1", 1.0),
        47 => return ("si1", 1.0),
        63..=83 => return ("ci1", 1.0),
        _ => {}
    }
    // Status 1 (engaged) stands in the battle motion.
    let stand = if status == 1 { names.battle } else { names.idle };
    if ratio > 1.0 / 3.0 {
        if ratio * 3.0 <= 1.5 { (names.walk, ratio * 3.0) } else { (names.run, ratio) }
    } else if ratio > 1e-4 {
        (names.walk, ratio * 3.0)
    } else {
        (stand, 1.0)
    }
}

/// First person's tank turn (FUN_100a6240, 0x100a7381): heading (our yaw, forward = (sin, 0, cos)) += 2 x X x ticks x
/// 0.01745, X the stick's normalised and tiered side (FUN_100a78d0), wrapped to +-pi. Stick right turns right.
pub fn tank_turn(heading: f32, x: f32, mag: f32, tier: f32, ticks: f32) -> f32 {
    let x = if mag > 0.0 { x / mag * tier } else { 0.0 };
    let pi = std::f32::consts::PI;
    (heading + 2.0 * x * ticks * 0.017452778 + pi).rem_euclid(std::f32::consts::TAU) - pi
}

/// FUN_100a80d0: the quarter of a step about the heading `f` (both level): 1 forward, 2 right, 3 back, 4 left, the
/// sectors 90 degrees wide and centred on the axes; 0 under 0.001.
pub fn strafe_dir(f: glam::Vec3, step: glam::Vec3) -> u8 {
    if step.length() < 0.001 {
        return 0;
    }
    // Right of the heading, as update_movement's camera right (forward x -Y).
    let r = f.cross(glam::Vec3::NEG_Y);
    let (ahead, side) = (step.dot(f), step.dot(r));
    if ahead.abs() >= side.abs() {
        if ahead >= 0.0 { 1 } else { 3 }
    } else if side > 0.0 {
        2
    } else {
        4
    }
}

/// FUN_100c8bb0's override of the walk / run while the heading is held (actor +0xfb), by the direction above: on foot
/// 'mvr' / 'mvb' / 'mvl', riding (status 5 / 0x55) 'mvb' backing and 'wlk' sideways; forward keeps the walk / run.
/// No fallback: a model without it keeps its last motion (FUN_100c83c0).
pub fn strafe(dir: u8, riding: bool) -> Option<&'static str> {
    match (dir, riding) {
        (3, _) => Some("mvb"),
        (2, false) => Some("mvr"),
        (4, false) => Some("mvl"),
        (2 | 4, true) => Some("wlk"),
        _ => None,
    }
}

/// `choose`, then `strafe` over it when it picked the walk or run (a status motion such as seated stays). `fixed`: a
/// fixed-model actor (FUN_10084400 2 / 7, no mv? motions) takes its walk for 2..4 instead (0x100c8d5e).
pub fn choose_strafed(names: &MotionNames, status: u8, ratio: f32, dir: u8, fixed: bool) -> (&'static str, f32) {
    let (n, r) = choose(names, status, ratio);
    if n != names.walk && n != names.run {
        (n, r)
    } else if fixed && matches!(dir, 2..=4) {
        (names.walk, r)
    } else {
        (strafe(dir, matches!(status, 5 | 0x55)).unwrap_or(n), r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tank_turns_right_two_degrees_a_tick() {
        use glam::Vec3;
        let h = super::tank_turn(0.0, 1.0, 1.0, 1.0, 1.0);
        assert!((h.to_degrees() - 2.0).abs() < 1e-3);
        // Toward the right vector (forward x -Y), as a step of +X goes.
        let (f, nf) = (Vec3::Z, Vec3::new(h.sin(), 0.0, h.cos()));
        assert!(nf.dot(f.cross(Vec3::NEG_Y)) > 0.0);
        // A partial stick (tier a third) turns a third as fast; none, or a dead stick, not at all.
        assert!((super::tank_turn(0.0, 0.5, 0.5, 1.0 / 3.0, 1.0).to_degrees() - 2.0 / 3.0).abs() < 1e-3);
        assert!((super::tank_turn(0.3, 0.0, 0.0, 0.0, 1.0) - 0.3).abs() < 1e-5);
        // Wrapped to +-pi.
        let w = super::tank_turn(std::f32::consts::PI - 0.01, 1.0, 1.0, 1.0, 1.0);
        assert!(w < 0.0 && w > -std::f32::consts::PI);
    }

    #[test]
    fn strafe_quarters() {
        use glam::Vec3;
        let f = Vec3::Z;
        let r = f.cross(Vec3::NEG_Y);
        assert_eq!(strafe_dir(f, f), 1);
        assert_eq!(strafe_dir(f, r), 2);
        assert_eq!(strafe_dir(f, -f), 3);
        assert_eq!(strafe_dir(f, -r), 4);
        // Diagonals forward of 45 degrees stay forward.
        assert_eq!(strafe_dir(f, f + r * 0.9), 1);
        assert_eq!(strafe_dir(f, Vec3::ZERO), 0);
        assert_eq!((strafe(2, false), strafe(3, true), strafe(4, true), strafe(1, false)), (Some("mvr"), Some("mvb"), Some("wlk"), None));
        // Only over the walk / run: seated (47) keeps si1, standing keeps idl.
        let n = MotionNames::default();
        assert_eq!(choose_strafed(&n, 0, 0.5, 4, false).0, "mvl");
        assert_eq!(choose_strafed(&n, 0, 0.9, 4, true).0, "wlk");
        assert_eq!(choose_strafed(&n, 47, 0.5, 4, false).0, "si1");
        assert_eq!(choose_strafed(&n, 0, 0.0, 4, false).0, "idl");
    }

    #[test]
    fn thresholds() {
        let mut n = MotionNames::default();
        assert_eq!(choose(&n, 0, 0.0), ("idl", 1.0));
        n.apply(0x79, 0);
        assert_eq!(choose(&n, 1, 0.0).0, "btl");
        assert_eq!(choose(&n, 0, 0.2).0, "wlk");
        assert_eq!(choose(&n, 0, 0.5), ("wlk", 1.5));
        assert_eq!(choose(&n, 0, 1.0), ("run", 1.0));
        assert_eq!(choose(&n, 3, 1.0).0, "cor");
        // Moving at SpeedBase 40 (4 units/s) for 1/30 s at full speed, from rest.
        let r = move_ratio(0.0, 4.0 / 30.0, 1.0 / 30.0, 40, 40, 0);
        assert!((r - 0.25).abs() < 1e-5);
        // The event walk at 1.3 u/s against 0.8 x 5.0 settles at 0.325: the walk motion at 0.975.
        let mut r = 0.0;
        for _ in 0..200 {
            r = move_ratio_ref(r, 1.3 / 60.0, 1.0 / 60.0, 0.8 * 5.0);
        }
        let (name, rate) = choose(&MotionNames::default(), 0, r);
        assert_eq!(name, "wlk");
        assert!((rate - 0.975).abs() < 1e-3, "{rate}");
    }
}

#[cfg(test)]
mod keep_tests {
    #[test]
    fn keeps_status_motion() {
        for n in ["idl", "3dl", "btl", "2tl", "cor", "rx1", "idl0"] {
            assert!(super::keeps_status_motion(n), "{n}");
        }
        for n in ["wlk", "run", "2un", "chi", "6hi", "fh1", "si1", "ci1"] {
            assert!(!super::keeps_status_motion(n), "{n}");
        }
    }
}

#[cfg(test)]
mod loop_tests {
    /// The run cycle loops without a held frame: every 60 Hz step moves the pose, and the pose just before the wrap
    /// is the one at frame 0 (the closing key repeats the first; the frame count is the keys less one).
    #[test]
    fn run_cycle_has_no_hitch() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(d) = index.read(16600) else { return };
        let f = crate::dat::character::load_file(&d).unwrap();
        let skel = f.skeleton.as_ref().unwrap();
        let run = f.animations.iter().find(|a| a.name == "run0").unwrap();
        let pose = |frame: f32| crate::dat::character::pose(skel, &[(run, frame)]);
        let diff = |a: &[crate::dat::character::BoneXf], b: &[crate::dat::character::BoneXf]| a.iter().zip(b).map(|(x, y)| x.q.angle_between(y.q)).fold(0f32, f32::max);
        assert!(diff(&pose(super::loop_frame(run.frames as f32 - 1e-4, run.frames)), &pose(0.0)) < 1e-2);
        // One tick at rate 1: frames += speed * 0.5.
        let step = run.speed * 0.5;
        let (mut t, mut prev) = (0.0f32, pose(0.0));
        while t < run.frames as f32 * 2.0 {
            t += step;
            let now = pose(super::loop_frame(t, run.frames));
            // A step moves some bone 0.12 rad or more; a held pose reads about 0.0015 (acos noise).
            assert!(diff(&now, &prev) > 1e-2, "held pose at frame {}", super::loop_frame(t, run.frames));
            prev = now;
        }
    }
}
