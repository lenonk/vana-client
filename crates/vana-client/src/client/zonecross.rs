//! Ours (retail cuts to black): a zone change holds the old zone's last frame over the load, then dissolves it into
//! the new zone.

use std::time::Instant;

/// How long the dissolve takes.
const SECS: f32 = 1.0;

#[derive(Default, Clone, Copy, Debug)]
pub struct ZoneCross {
    /// The frame is held: the last one drawn, copied from the scene texture before the next frame draws.
    pub captured: bool,
    /// When the new zone first stood ready.
    shown: Option<Instant>,
}

impl ZoneCross {
    /// The held frame's alpha at `at`; None once it has dissolved.
    pub fn alpha(&self, at: Instant) -> Option<f32> {
        let Some(t) = self.shown else { return Some(1.0) };
        let a = 1.0 - at.saturating_duration_since(t).as_secs_f32() / SECS;
        (a > 0.0).then_some(a)
    }
}

/// A zone change starts (a zone line touched, or the server's 0x00B): hold the frame once a world frame was drawn, again
/// over a dissolve.
pub fn arm(c: &mut Option<ZoneCross>, drawn: bool) {
    if drawn && c.is_none_or(|c| c.shown.is_some()) {
        log::info!("zone crossfade: holding the frame");
        *c = Some(ZoneCross::default());
    }
}

/// Each frame after the fade: dissolves once the new zone stands ready on the GPU, ends when done.
pub fn step(c: &mut Option<ZoneCross>, zoning: bool, drawn: bool, ready: bool, now: Instant) {
    if zoning {
        arm(c, drawn);
    }
    if let Some(x) = c {
        if ready && x.captured && !zoning && x.shown.is_none() {
            log::info!("zone crossfade: dissolving");
            x.shown = Some(now);
        }
        if x.alpha(now).is_none() {
            *c = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn holds_then_dissolves() {
        let t0 = Instant::now();
        let at = |s: f32| t0 + Duration::from_secs_f32(s);
        let mut c = None;
        // Not zoning, or no world drawn yet (the login): nothing.
        step(&mut c, false, true, true, t0);
        step(&mut c, true, false, false, t0);
        assert!(c.is_none());
        step(&mut c, true, true, false, t0);
        assert_eq!(c.unwrap().alpha(t0), Some(1.0));
        // Ready before the frame is held: still held.
        step(&mut c, false, true, true, at(0.1));
        assert_eq!(c.unwrap().alpha(at(5.0)), Some(1.0));
        c.as_mut().unwrap().captured = true;
        step(&mut c, false, true, false, at(2.0));
        assert_eq!(c.unwrap().alpha(at(3.0)), Some(1.0));
        step(&mut c, false, true, true, at(3.0));
        assert!((c.unwrap().alpha(at(3.5)).unwrap() - 0.5).abs() < 1e-3);
        step(&mut c, false, true, true, at(3.5));
        assert!(c.is_some());
        step(&mut c, false, true, true, at(4.0));
        assert!(c.is_none());
    }

    /// A zone change during the dissolve holds a new frame.
    #[test]
    fn rearms_over_a_dissolve() {
        let t0 = Instant::now();
        let mut c = Some(ZoneCross { captured: true, shown: Some(t0) });
        step(&mut c, true, true, false, t0 + Duration::from_millis(300));
        let x = c.unwrap();
        assert!(!x.captured && x.shown.is_none());
    }
}
