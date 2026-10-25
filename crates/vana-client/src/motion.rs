//! Which motion an actor plays and how fast, as FFXiMain decides it: FUN_100c68f0 (movement ratio),
//! FUN_100c6b70 (motion name and rate), FUN_100c6380 (switch, 16-tick blend), motion player
//! 0x10019ab0 (frame advance). Client ticks are 1/60 s (FUN_10014000, at least 1 per frame).

/// Ticks a motion change blends over (FUN_100c6380).
pub const BLEND_TICKS: f32 = 16.0;

/// Frames per second of a motion: frame += ticks * speed * 0.5 * rate.
pub fn frames_per_second(speed: f32, rate: f32) -> f32 {
    60.0 * speed * 0.5 * rate
}

/// Looping frame position: the client wraps at `frames`, and keys are sampled clamped to the last one
/// (the last key repeats the first in the data).
pub fn loop_frame(elapsed_frames: f32, frames: u16) -> f32 {
    let n = frames.max(1) as f32;
    elapsed_frames.rem_euclid(n).min(n - 1.0)
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

#[cfg(test)]
mod tests {
    use super::*;

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
