//! "menu    gaugewin" (DAT_105781ec; spec ~/ffxi/lab/research/combat_mode_spec.md section 4): our HP / MP / TP at
//! (16, 48) while engaged (opened by the battle reset, closed when we leave status 1). Draw FUN_101393a0, update
//! FUN_10139660.

use super::World;
use super::game_ui::GameUi;

pub const WINDOW: &str = "menu    gaugewin";
const WINDOWPS: &str = "menu    windowps";
const FRAMESUS: &str = "menu    framesus";
/// TP tiers' colours (>= 1000, 2000, 3000).
const TIER: [u32; 3] = [0x8000_60b0, 0x8000_b060, 0x8020_60d0];

/// +0x2c the previous TP, +0x24 the pulse, +0x2e its direction; ticks toward the next 2-tick step.
pub struct Gauge {
    prev_tp: u32,
    pulse: f32,
    up: bool,
    acc: f32,
    last: std::time::Instant,
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge { prev_tp: 0, pulse: 1.0, up: true, acc: 0.0, last: std::time::Instant::now() }
    }
}

impl Gauge {
    /// Once a frame: the ticks since the last call.
    pub fn tick(&mut self, tp: u32) {
        let now = std::time::Instant::now();
        let ticks = now.duration_since(self.last).as_secs_f32() * 60.0;
        self.last = now;
        self.update(tp, ticks);
    }

    /// FUN_10139660 once per retail frame (2 ticks): crossing 1000 starts the pulse at 1.0 rising; while it stays at
    /// 1000 or more it grows by 1.25 to past 2.0, then shrinks by 0.75 to below 1.0, and again.
    pub fn update(&mut self, tp: u32, ticks: f32) {
        self.acc += ticks;
        while self.acc >= 2.0 {
            self.acc -= 2.0;
            if self.prev_tp < 1000 {
                if tp >= 1000 {
                    (self.pulse, self.up) = (1.0, true);
                }
            } else if tp >= 1000 {
                if self.up {
                    self.pulse *= 1.25;
                    self.up = self.pulse <= 2.0;
                } else {
                    self.pulse *= 0.75;
                    self.up = self.pulse < 1.0;
                }
            }
            self.prev_tp = tp;
        }
    }

    /// The full bar's colour: the tier's base with alpha (min(trunc(p 127), 255) - 127) (wrapping below 1.0).
    fn pulse_colour(&self, tp: u32) -> u32 {
        let base = match tp {
            3000.. => TIER[2],
            2000.. => TIER[1],
            1000.. => TIER[0],
            _ => 0,
        };
        let a = ((self.pulse * 127.0) as i32).min(255) - 127;
        base & 0x00ff_ffff | ((a as u32) & 0xff) << 24
    }
}

/// The window, its labels and troughs (framesus 8), the HP / MP fills (windowps 97 scaled by the percent, blend 8),
/// the TP tiers, and the numbers ("%4d" right-aligned at x + 123, 0xc0808080).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let g = w.gauge.as_ref()?;
    let (hp, mp, tp, hpp, mpp) = w.vitals?;
    let (x, y) = gu.window(WINDOW, sw, sh)?;
    gu.sprite_scaled_blend(WINDOWPS, 97, x + 25.0, y + 8.0, hpp as f32 * 0.01, 1.0, 0x8080_8080, 8);
    gu.sprite_scaled_blend(WINDOWPS, 97, x + 25.0, y + 18.0, mpp as f32 * 0.01, 1.0, 0x8080_e080, 8);
    gu.sprite(FRAMESUS, 8, x + 4.0, y + 8.0, 0x8080_8080);
    let tp = tp.min(3000);
    let (tx, ty) = (x + 25.0, y + 28.0);
    if tp < 1000 {
        gu.sprite_scaled_blend(WINDOWPS, 97, tx, ty, tp as f32 * 0.001, 1.0, TIER[0], 0);
    } else {
        gu.sprite_scaled_blend(WINDOWPS, 97, tx, ty, 1.0, 1.0, g.pulse_colour(tp), 0);
        // Retail's `% 1001` (0x3e9) for the part above the tier.
        if tp < 3000 {
            let tint = if tp < 2000 { TIER[1] } else { TIER[2] };
            gu.sprite_scaled_blend(WINDOWPS, 97, tx, ty, (tp % 1001) as f32 * 0.001, 1.0, tint, 0);
        }
    }
    for (k, v) in [hp, mp, tp].into_iter().enumerate() {
        gu.shape_text(&format!("{v:4}"), x + 123.0, y + 7.0 + 10.0 * k as f32, 0xc080_8080, true);
    }
    gu.window_rect(WINDOW, sw, sh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pulse from crossing 1000: 1.0, then x1.25 up to past 2.0, then x0.75 down to below 1.0.
    #[test]
    fn tp_pulse() {
        let mut g = Gauge::default();
        g.update(900, 2.0);
        g.update(1000, 2.0);
        assert_eq!((g.pulse, g.up), (1.0, true));
        let mut seen = Vec::new();
        for _ in 0..6 {
            g.update(1200, 2.0);
            seen.push((g.pulse * 1000.0).round() as i32);
        }
        assert_eq!(seen, [1250, 1563, 1953, 2441, 1831, 1373]);
    }
}
