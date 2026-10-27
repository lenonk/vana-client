//! Ours (user, 2026-10-27): the next rest tick. The server heals a resting character on its own clock, which no packet
//! tells. LSB: the healing effect ticks every 10 s from its start and heals from its second tick, but effects are only
//! looked at every 3 s, so a tick lands on the first look after its time (gaps of 9, 9, 12 s). The first clean rest
//! seen is timed (its start to the first HP or MP rise, then rise to rise) and kept in config rest_tick as [first
//! heal, interval, the look period]; from then on the party window shows how near the next tick is, its looks placed
//! by any rise seen in the zone.

use super::World;
use std::time::Instant;

/// The rest being watched: its start, the vitals last frame, the rises seen (seconds from the start); a rise seen in
/// this zone (the server's looks fall a period apart from it); what was learnt and whether it changed since saved.
#[derive(Default)]
pub struct RestTick {
    start: Option<Instant>,
    last: (u32, u32),
    rises: Vec<f32>,
    grid: Option<(u16, Instant)>,
    pub learnt: Option<[f32; 3]>,
    pub dirty: bool,
}

impl RestTick {
    pub fn new(learnt: Option<[f32; 3]>) -> Self {
        Self { learnt, ..Default::default() }
    }
}

/// What the party window shows while resting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown {
    /// Nothing learnt yet: the seconds rested so far.
    Timing(f32),
    /// The share of the wait gone and the seconds to the next tick.
    Next(f32, f32),
}

/// Rises closer than this are one tick (HP and MP come in separate packets).
const SAME: f32 = 2.0;
/// A rest tick's interval lies here (Regen's and Refresh's 3 s ticks do not).
const INTERVAL: (f32, f32) = (5.0, 30.0);

/// Per frame: a rest starts and ends with status 0x21; a rise in HP or MP during it is a tick.
pub fn update(w: &mut World) {
    let now = Instant::now();
    let vitals = w.vitals.map_or((0, 0), |v| (v.0, v.1));
    let (resting, zone) = (w.my_motion.status == 0x21, w.pos_zone);
    let r = &mut w.rest;
    let Some(start) = r.start.filter(|_| resting) else {
        r.start = resting.then_some(now);
        (r.rises, r.last) = (Vec::new(), vitals);
        return;
    };
    let rose = vitals.0 > r.last.0 || vitals.1 > r.last.1;
    r.last = vitals;
    let t = now.duration_since(start).as_secs_f32();
    if rose && r.rises.last().is_none_or(|x| t - x > SAME) {
        log::debug!("rest tick seen {t:.2} s into the rest");
        r.rises.push(t);
        r.grid = Some((zone, now));
        if r.learnt.is_none()
            && let Some(l) = learn(&r.rises)
        {
            log::info!("rest ticks: first heal after {:.1} s, then every {:.1} s, looked at every {:.1} s", l[0], l[1], l[2]);
            (r.learnt, r.dirty) = (Some(l), true);
        }
    }
}

/// An interval near a whole second is that second (LSB's tick delays are whole seconds; the network's jitter is not).
fn whole(t: f32) -> f32 {
    if (t - t.round()).abs() < 0.3 { t.round() } else { t }
}

/// [first heal, interval, look period] from a rest's rises: four or more, the interval their mean gap, the period the
/// largest every gap is a multiple of (none under 1 s: the ticks land on time).
fn learn(rises: &[f32]) -> Option<[f32; 3]> {
    if rises.len() < 4 {
        return None;
    }
    let gaps: Vec<f32> = rises.windows(2).map(|p| p[1] - p[0]).collect();
    let t = whole((rises[rises.len() - 1] - rises[0]) / gaps.len() as f32);
    if !(INTERVAL.0..INTERVAL.1).contains(&t) || rises[0] <= SAME {
        return None;
    }
    let fits = |p: f32| {
        gaps.iter().all(|g| {
            let n = (g / p).round();
            n >= 1.0 && (g - n * p).abs() < 0.4
        })
    };
    let p = (4..=(t * 2.0) as u32).rev().map(|q| q as f32 / 4.0).find(|&p| fits(p)).unwrap_or(0.0);
    gaps.iter().all(|g| (g - t).abs() <= p + 0.5).then_some([rises[0], t, p])
}

/// While resting: timing, or the next tick and how far the wait since the last one is.
pub fn shown(w: &World) -> Option<Shown> {
    let r = &w.rest;
    let start = r.start?;
    let now = start.elapsed().as_secs_f32();
    let Some(model) = r.learnt else { return Some(Shown::Timing(now)) };
    let grid = r.grid.filter(|g| g.0 == w.pos_zone).map(|(_, g)| if g >= start { g.duration_since(start).as_secs_f32() } else { -start.duration_since(g).as_secs_f32() });
    let (prev, next) = window(model, &r.rises, grid, now);
    Some(Shown::Next(((now - prev) / (next - prev)).clamp(0.0, 1.0), next - now))
}

/// The ticks around `now` (seconds from the rest's start): the server's start bounded by the rises seen (each lands
/// within a period after its time), each tick on the first look at or after its time; before the first heal the wait
/// runs from the rest's start.
fn window([first, t, p]: [f32; 3], rises: &[f32], grid: Option<f32>, now: f32) -> (f32, f32) {
    let t = whole(t);
    let k0 = (first / t).round().max(1.0);
    let (mut lo, mut hi) = (f32::MIN, f32::MAX);
    for &x in rises {
        let k = ((x - p / 2.0) / t).round();
        (lo, hi) = (lo.max(x - k * t - p), hi.min(x - k * t));
    }
    // No rise yet: the server started about when we saw it (its looks known), else as the learnt first heal says.
    let s = match (rises.is_empty(), lo <= hi) {
        (true, _) if grid.is_some() => 0.0,
        (true, _) => first - k0 * t - p / 2.0,
        (false, true) => (lo + hi) / 2.0,
        (false, false) => hi,
    };
    let at = |k: f32| match grid {
        Some(g) if p >= 1.0 => g + ((s + k * t - g) / p - 0.02).ceil() * p,
        _ => s + k * t + p / 2.0,
    };
    let mut k = k0;
    while at(k) <= now && k < 100_000.0 {
        k += 1.0;
    }
    (if k > k0 { at(k - 1.0) } else { 0.0 }, at(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB's ticks: every 10 s from the effect's start, healing from the second, each on the first 3 s look after.
    fn server(s: f32, phase: f32, k: f32) -> f32 {
        let x = s + 10.0 * k;
        phase + ((x - phase) / 3.0).ceil() * 3.0
    }

    #[test]
    fn learns_the_look_period() {
        let rises: Vec<f32> = (2..6).map(|k| server(-0.1, 0.7, k as f32)).collect();
        let [first, t, p] = learn(&rises).unwrap();
        assert!(t == 10.0 && (p - 3.0).abs() < 0.01 && (first - rises[0]).abs() < 1e-4);
        assert_eq!(learn(&rises[..3]), None);
        // Regen's 3 s ticks teach nothing.
        assert_eq!(learn(&[3.0, 6.0, 9.0, 12.0]), None);
    }

    #[test]
    fn predicts_the_next_looks() {
        for (s, phase) in (0..16).map(|i| (-0.02 - 0.013 * i as f32, 0.19 * i as f32)) {
            predicts(s, phase);
        }
    }

    fn predicts(s: f32, phase: f32) {
        let rises: Vec<f32> = (2..6).map(|k| server(s, phase, k as f32)).collect();
        let model = learn(&rises).unwrap();
        let grid = Some(rises[3]);
        for k in 6..12 {
            let now = server(s, phase, k as f32 - 1.0) + 0.5;
            let (prev, next) = window(model, &rises, grid, now);
            assert!((next - server(s, phase, k as f32)).abs() < 0.05, "phase {phase} tick {k}: {next}");
            assert!(prev <= now && now < next);
        }
        // A later rest in the zone: its first heal from the looks already known.
        let (s2, at) = (-0.05, 100.0);
        let rest_grid = Some(phase + ((at - phase) / 3.0).floor() * 3.0 - at);
        let (_, next) = window(model, &[], rest_grid, 1.0);
        assert!((next - (server(s2 + at, phase, 2.0) - at)).abs() < 0.05, "phase {phase} later rest: {next}");
    }
}
