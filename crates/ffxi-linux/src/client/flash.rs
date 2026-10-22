//! The target flash (FUN_100c94a0, asm 0x100c991f..0x100c9c87): while an actor is drawn as the target, its
//! ambient light is pulled toward grey 220 / 255 and its fog colour scaled by a triangle k over 40-tick pulses;
//! three pulses on acquiring the target, then nothing until it is targeted again. A forced flash (actor +0x88
//! bit 0x200: the sub-target cursor, area previews) runs on while set and stops with its pulse count at 0.

/// Per actor: +0x840 bit 2 latch, bits 25..26 pulses left, +0x844 ticks left in the pulse, +0x88 bit 0x200.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flash {
    latch: bool,
    left: u8,
    t: f32,
    pub forced: bool,
}

/// No flash: (ambient lerp 0, fog scale 1).
pub const NONE: [f32; 2] = [0.0, 1.0];

impl Flash {
    /// One drawn frame of `ticks` (1/60 s); returns (ambient lerp toward 220 / 255, fog colour scale).
    pub fn step(&mut self, subject: bool, ticks: f32) -> [f32; 2] {
        if !(subject || self.forced) {
            self.latch = false;
            return NONE;
        }
        if !self.latch {
            self.left = 3;
            self.t = 40.0;
        }
        self.latch = true;
        if self.left == 0 && !self.forced {
            return NONE;
        }
        let t = self.t;
        let k = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
        let t = t - ticks;
        if t < 0.0 {
            self.t = 40.0;
            // (pulses - 1) & 3: a forced flash wraps 0 -> 3 and ends when a pulse ends at 0.
            self.left = self.left.wrapping_sub(1) & 3;
            if self.left == 0 {
                self.forced = false;
            }
        } else {
            self.t = t;
        }
        [k, k]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_pulses() {
        let mut f = Flash::default();
        // k at t = 40 / 30 / 20 / 10 / 0.
        let ks: Vec<f32> = (0..5).map(|_| f.step(true, 10.0)[0]).collect();
        assert_eq!(ks, vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        // The fifth step went below 0 (t = -10): pulse 2 starts at 40.
        assert_eq!((f.left, f.t), (2, 40.0));
    }

    #[test]
    fn three_pulses_then_rearm() {
        let mut f = Flash::default();
        let mut active = 0;
        for _ in 0..200 {
            if f.step(true, 1.0) != NONE || f.left > 0 {
                active += 1;
            }
        }
        // 3 pulses of 41 frames at one tick a frame, then nothing while still targeted.
        assert_eq!(active, 123);
        assert_eq!(f.step(true, 1.0), NONE);
        // A frame untargeted re-arms it.
        assert_eq!(f.step(false, 1.0), NONE);
        f.step(true, 1.0);
        assert_eq!(f.left, 3);
    }

    #[test]
    fn forced_runs_on_then_clears() {
        let mut f = Flash::default();
        // The setter (the sub-target cursor) sets the bit every frame.
        for _ in 0..41 * 5 {
            f.forced = true;
            f.step(false, 1.0);
        }
        // Still forced: the count wrapped 0 -> 3 instead of stopping.
        assert!(f.forced);
        // The setter stops: the flash ends when a pulse ends at count 0.
        let mut frames = 0;
        while f.forced {
            f.step(false, 1.0);
            frames += 1;
            assert!(frames < 41 * 4);
        }
        assert_eq!(f.step(false, 1.0), NONE);
    }
}
