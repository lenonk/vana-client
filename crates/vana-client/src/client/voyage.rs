//! The ship voyage's state (spec ~/ffxi/lab/research/ship_voyage_spec.md 1.1..1.4): DAT_1065ca2c (t),
//! DAT_10376ff0 (rate), DAT_1065ca28 (reverse), DAT_1047bc88 (route), DAT_1047bc8c (hold) and the zone's path.
//! Never reset by a zone change: a zone-in without ship data keeps it, as retail.

use super::net::ShipData;
use crate::fx::voyage::Path;
use glam::Mat4;
use std::time::Instant;

pub struct Voyage {
    pub t: f64,
    pub rate: f64,
    pub reverse: bool,
    pub route: u8,
    pub hold: bool,
    pub path: Option<Path>,
    /// This frame's V (identity off a voyage), and when t last stepped.
    pub matrix: Mat4,
    last: Option<Instant>,
}

impl Default for Voyage {
    /// The .data initials: t 0, a 900 s route, forward, route 0, no hold.
    fn default() -> Self {
        Voyage { t: 0.0, rate: 1.0 / 54000.0, reverse: false, route: 0, hold: false, path: None, matrix: Mat4::IDENTITY, last: None }
    }
}

impl Voyage {
    /// FUN_10177650 / FUN_101776c0 at zone-in: the leg's progress at `now` (the synced clock) and its rate a tick.
    pub fn start(&mut self, s: ShipData, now: u32) {
        // A zero length would divide by zero in retail; LSB never sends one.
        let ticks = (s.end.wrapping_sub(s.start).max(1)) as f64 * 60.0;
        (self.route, self.hold, self.reverse) = (s.route, false, s.reverse);
        self.rate = 1.0 / ticks;
        self.t = (now as f64 - s.start as f64) * 60.0 / ticks;
        self.last = None;
    }

    /// The zone's path once its scene is in: `c00<route>`, else `c000` (by presence only, FUN_10055040).
    pub fn set_path(&mut self, paths: &[Option<Vec<u8>>; 4]) {
        let raw = paths[self.route as usize & 3].as_deref().or(paths[0].as_deref());
        self.path = raw.and_then(Path::parse);
        if let Some(p) = &self.path {
            log::debug!("voyage: route {} ({} keys, chord {:.1}), t {:.4}, rate {:.3e}, reverse {}", self.route, p.keys, p.chord, self.t, self.rate, self.reverse);
        }
    }

    /// FUN_10178b30 for a loaded zone: V from t, then t advances by the real ticks since the last step (retail
    /// counts at least one tick a frame, which only holds at its 30 fps); past 1 it starts over at 0. Held or
    /// without a path: identity, t still.
    pub fn step(&mut self, now: Instant) -> Mat4 {
        let ticks = self.last.map_or(0.0, |l| now.duration_since(l).as_secs_f64() * 60.0);
        self.last = Some(now);
        self.matrix = match self.path.as_ref().filter(|_| !self.hold) {
            Some(p) => {
                let v = p.matrix(self.t.max(0.0) as f32, self.reverse);
                self.t += ticks * self.rate;
                if self.t > 1.0 {
                    self.t = 0.0;
                }
                v
            }
            None => Mat4::IDENTITY,
        };
        self.matrix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ship() -> ShipData {
        ShipData { start: 1_000_000, end: 1_000_944, reverse: false, route: 0 }
    }

    /// Spec 6: a leg 236 s in is a quarter done, 1 / 56640 a tick.
    #[test]
    fn start() {
        let mut v = Voyage::default();
        assert_eq!(v.rate, 1.0 / 54000.0);
        v.start(ship(), 1_000_236);
        assert!((v.t - 0.25).abs() < 1e-9 && (v.rate - 1.0 / 56640.0).abs() < 1e-15);
        // Before the leg: t below 0, posed at 0, still rising.
        v.start(ship(), 999_990);
        assert!(v.t < 0.0);
    }

    /// The step poses at t, then adds the ticks; past 1 it wraps to 0; a hold stops it at identity.
    #[test]
    fn step_wrap_hold() {
        let mut v = Voyage::default();
        v.start(ship(), 1_000_236);
        // A one-key path is no path: identity, t still.
        let at = Instant::now();
        assert_eq!(v.step(at), Mat4::IDENTITY);
        let key = |x: f32, z: f32| -> Vec<u8> { [x, 0.0, z, 350.0, x + 1.0, 0.0, z, 0.0].iter().flat_map(|f| f.to_le_bytes()).chain([0u8; 16]).collect() };
        let mut d = vec![0u8; 0x20];
        d[0x10] = 2;
        d.extend(key(0.0, 0.0));
        d.extend(key(100.0, 0.0));
        v.set_path(&[Some(d), None, None, None]);
        assert!(v.path.is_some());
        v.step(at);
        let m = v.step(at + Duration::from_secs(1));
        // At t 0.25 the ship stands at x 25, heading +x: V takes it to the origin.
        assert!(m.transform_point3(glam::Vec3::new(25.0, 0.0, 0.0)).length() < 1e-3);
        assert!((v.t - (0.25 + 60.0 / 56640.0)).abs() < 1e-9);
        v.t = 0.99999;
        v.step(at + Duration::from_secs(2));
        assert_eq!(v.t, 0.0);
        v.hold = true;
        v.t = 0.5;
        assert_eq!(v.step(at + Duration::from_secs(3)), Mat4::IDENTITY);
        assert_eq!(v.t, 0.5);
    }
}
