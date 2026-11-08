//! Ours (user, 2026-10-27; reworked for issue #75): the next rest tick. The server heals a resting character on its own
//! clock, which no packet tells. A model is learnt per server (config rest_models, by the search address) from every
//! rest seen and kept moving: the first heal after the rest's start, the interval after each heal (the last one stands
//! for every later one, so a server whose ticks quicken to a floor is followed), and the period of the server's looks
//! (LSB heals every 10 s but looks at effects every 3 s, so its ticks land 9, 9, 12 s apart). Each rise in HP or MP
//! re-anchors the rest's ticks, and one the model can't place starts the anchoring over from it. A stat's own ticks
//! (Regen, Refresh: from spells, gear, food or songs, no icon needed) are told apart by their repeats. The party window
//! shows how near the next tick is.

use super::World;
pub use crate::config::RestModel as Model;
use std::time::Instant;

/// A value holds at most this many rests' worth: a server's change shows within a few rests.
const WEIGHT: u32 = 6;
/// Intervals learnt one by one up to here; later ones are the floor (the last).
const MAX_GAPS: usize = 24;

/// Whether intervals are one interval give or take the server's looks (`period`): within the larger of a period and a
/// half and a fifth of their mean.
fn steady(gaps: &[f32], period: f32) -> bool {
    let (lo, hi) = gaps.iter().fold((f32::MAX, f32::MIN), |(a, b), &g| (a.min(g), b.max(g)));
    let mean = gaps.iter().sum::<f32>() / gaps.len().max(1) as f32;
    gaps.is_empty() || hi - lo <= (period + 0.5).max(0.2 * mean)
}

impl Model {
    fn ready(&self) -> bool {
        self.seen.first().is_some_and(|&n| n > 0) && !self.gaps.is_empty()
    }

    /// Every interval as used (whole seconds when near one: servers count in seconds, networks jitter): one interval,
    /// their mean, when they are steady (their spread the looks' doing), else each its own.
    fn resolved(&self) -> Vec<f32> {
        if steady(&self.gaps, self.period) {
            let mean = self.gaps.iter().sum::<f32>() / self.gaps.len().max(1) as f32;
            return vec![whole(if self.gaps.is_empty() { 10.0 } else { mean })];
        }
        self.gaps.iter().map(|&g| whole(g)).collect()
    }

    /// Tick i's interval to the next.
    fn gap(&self, i: usize) -> f32 {
        let r = self.resolved();
        r.get(i).or(r.last()).copied().unwrap_or(10.0)
    }

    fn mean(&mut self, slot: usize, v: f32) {
        while self.seen.len() <= slot {
            self.seen.push(0);
        }
        let n = self.seen[slot].min(WEIGHT - 1) + 1;
        let cur = if slot == 0 { &mut self.first } else { &mut self.gaps[slot - 1] };
        *cur += (v - *cur) / n as f32;
        self.seen[slot] = n;
    }

    /// The first heal seen this rest.
    fn learn_first(&mut self, x: f32) {
        self.mean(0, x);
    }

    /// The interval after tick i seen this rest.
    fn learn_gap(&mut self, i: usize, g: f32) {
        let i = i.min(MAX_GAPS - 1);
        if self.gaps.len() <= i {
            self.gaps.push(g);
            self.seen.resize(i + 2, 0);
        }
        self.mean(i + 1, g);
    }

    /// Whether `g` fits as the interval after tick i (a skipped tick or a stray rise does not).
    fn fits(&self, i: usize, g: f32) -> bool {
        if self.gaps.is_empty() {
            return (INTERVAL.0..INTERVAL.1).contains(&g);
        }
        let e = self.gap(i);
        (g - e).abs() <= (0.35 * e).max(self.period + 0.6)
    }
}

/// The rest being watched: its start, the vitals (HP, MP) last frame, each one's recent rises (when, by how much), the
/// rise waiting to count, the rises seen (seconds from the start), whether its rises still follow on one another (none
/// skipped), a rise seen in this zone (the server's looks fall a period apart from it); the model and whether it
/// changed since saved.
#[derive(Default)]
pub struct RestTick {
    start: Option<Instant>,
    last: [u32; 2],
    recent: [Vec<(Instant, u32)>; 2],
    held: Option<Held>,
    rises: Vec<f32>,
    chain: bool,
    grid: Option<(u16, Instant)>,
    pub model: Model,
    pub dirty: bool,
}

/// A rise waiting to count: when, its rest (start, zone), and the rises (when, by how much; HP, MP) that may still be a
/// rest tick's (a stat may rise twice within it: an own tick, then a rest tick).
struct Held {
    at: Instant,
    start: Instant,
    zone: u16,
    amounts: [Vec<(Instant, u32)>; 2],
}

impl RestTick {
    pub fn new(model: Option<Model>) -> Self {
        Self { model: model.unwrap_or_default(), ..Default::default() }
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
/// A rest tick's interval lies here.
const INTERVAL: (f32, f32) = (2.5, 60.0);
/// A stat's own ticks repeat its amount this far apart (a look or two: LSB and retail apply them at every 3 s look).
/// A rest tick on the same look adds to it; rest heals grow each tick, so two in a row never match.
const OWN_GAP: (f32, f32) = (2.0, 7.0);
/// A rise waits this long before it counts as a tick: an own tick's twin a look later shows what it was.
const HOLD: f32 = 4.5;

fn secs(from: Instant, to: Instant) -> f32 {
    to.saturating_duration_since(from).as_secs_f32()
}

/// Per frame: a rest starts and ends with status 0x21; a rise in HP or MP during it, other than a stat's own tick,
/// is a tick.
pub fn update(w: &mut World) {
    let v = w.vitals.unwrap_or_default();
    let resting = w.my_motion.status == 0x21;
    w.rest.feed(Instant::now(), resting, w.pos_zone, [v.0, v.1], [v.3 >= 100, v.4 >= 100]);
}

impl RestTick {
    /// One frame's vitals (HP, MP) and whether each is full.
    fn feed(&mut self, now: Instant, resting: bool, zone: u16, vitals: [u32; 2], full: [bool; 2]) {
        let mut tick = [None; 2];
        for s in 0..2 {
            let a = vitals[s].saturating_sub(self.last[s]);
            if a > 0 && !self.own_rise(s, now, a, full[s]) {
                tick[s] = Some(a);
            }
        }
        self.last = vitals;
        let Some(start) = self.start.filter(|_| resting) else {
            if resting {
                // A new rest: what the last one held counts now, before its rises go.
                self.count_held();
                (self.rises, self.chain) = (Vec::new(), true);
            }
            self.start = resting.then_some(now);
            // A heal held as the rest ended still waits for its twin.
            self.settle(now);
            return;
        };
        if tick != [None; 2] {
            match self.held.as_mut() {
                // HP and MP come in separate packets.
                Some(h) if secs(h.at, now) <= SAME => {
                    for (held, a) in h.amounts.iter_mut().zip(tick) {
                        held.extend(a.map(|a| (now, a)));
                    }
                }
                _ => {
                    self.count_held();
                    let amounts = tick.map(|a| a.map(|a| (now, a)).into_iter().collect());
                    self.held = Some(Held { at: now, start, zone, amounts });
                }
            }
        }
        self.settle(now);
    }

    /// The held rise counts once its twin had time to show.
    fn settle(&mut self, now: Instant) {
        if self.held.as_ref().is_some_and(|h| secs(h.at, now) > HOLD) {
            self.count_held();
        }
    }

    /// The held rise counts at the first of its amounts left (an own tick's twin may have dropped an earlier one).
    fn count_held(&mut self) {
        if let Some(h) = self.held.take() {
            let at = h.amounts.iter().flatten().map(|x| x.0).min().unwrap_or(h.at);
            self.count(h.start, h.zone, at);
        }
    }

    /// Whether stat `s`'s rise of `a` is its own tick: the amount it rose by a look or two before (or less, filling it).
    /// The held amount this one twins was one too, and no longer counts.
    fn own_rise(&mut self, s: usize, now: Instant, a: u32, full: bool) -> bool {
        let like = |b: u32| a == b || (full && a < b);
        let near = |at: Instant| (OWN_GAP.0..=OWN_GAP.1).contains(&secs(at, now));
        let recent = &mut self.recent[s];
        recent.retain(|&(at, _)| secs(at, now) <= OWN_GAP.1);
        let own = recent.iter().any(|&(at, b)| near(at) && like(b));
        recent.push((now, a));
        if own
            && let Some(h) = self.held.as_mut()
            && near(h.at)
            && let Some(i) =
                h.amounts[s].iter().position(|x| x.1 == a).or_else(|| h.amounts[s].iter().position(|x| like(x.1)))
        {
            h.amounts[s].remove(i);
            if h.amounts.iter().all(Vec::is_empty) {
                self.held = None;
            }
        }
        own
    }

    /// A rest tick at `at`: anchored on and learnt from.
    fn count(&mut self, start: Instant, zone: u16, at: Instant) {
        let t = secs(start, at);
        log::debug!("rest tick seen {t:.2} s into the rest");
        self.rises.push(t);
        self.grid = Some((zone, at));
        let n = self.rises.len();
        if n == 1 {
            if t > SAME && t < INTERVAL.1 {
                self.model.learn_first(t);
                self.dirty = true;
            }
            return;
        }
        let g = t - self.rises[n - 2];
        if self.chain && self.model.fits(n - 2, g) {
            self.model.learn_gap(n - 2, g);
            if let Some(p) = look_period(&self.rises) {
                self.model.period = p;
            }
            self.dirty = true;
            if n == 4 {
                let m = &self.model;
                log::info!(
                    "rest ticks: first heal after {:.1} s, then {:?} s, looked at every {:.1} s",
                    m.first,
                    m.gaps.iter().map(|g| (g * 10.0).round() / 10.0).collect::<Vec<_>>(),
                    m.period
                );
            }
        } else {
            self.chain = false;
        }
    }
}

/// An interval near a whole second is that second (servers' tick delays are whole seconds; the network's jitter is
/// not).
fn whole(t: f32) -> f32 {
    if (t - t.round()).abs() < 0.3 { t.round() } else { t }
}

/// The server's look period from a rest's rises (four or more, their gaps steady: a quickening server's are not the
/// looks' doing): the largest every gap is a multiple of, 1 s and up; None when none fits (the ticks land on time).
fn look_period(rises: &[f32]) -> Option<f32> {
    if rises.len() < 4 {
        return None;
    }
    let gaps: Vec<f32> = rises.windows(2).map(|p| p[1] - p[0]).collect();
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    if gaps.iter().any(|g| (g - mean).abs() > 1.5f32.max(0.2 * mean)) {
        return None;
    }
    let fits = |p: f32| {
        gaps.iter().all(|g| {
            let n = (g / p).round();
            n >= 1.0 && (g - n * p).abs() < 0.4
        })
    };
    let lo = gaps.iter().copied().fold(f32::MAX, f32::min);
    let p = (4..=(lo * 4.0) as u32).rev().map(|q| q as f32 / 4.0).find(|&p| fits(p))?;
    // Gaps that differ by a period are its doing; equal ones say nothing of it.
    let spread = gaps.iter().copied().fold(f32::MIN, f32::max) - lo;
    (spread > 0.5).then_some(p)
}

/// While resting: timing, or the next tick and how far the wait since the last one is.
pub fn shown(w: &World) -> Option<Shown> {
    let r = &w.rest;
    let start = r.start?;
    let now = start.elapsed().as_secs_f32();
    if !r.model.ready() {
        return Some(Shown::Timing(now));
    }
    let grid = r.grid.filter(|g| g.0 == w.pos_zone).map(|(_, g)| {
        if g >= start { g.duration_since(start).as_secs_f32() } else { -start.duration_since(g).as_secs_f32() }
    });
    let (prev, next) = window(&r.model, &r.rises, grid, now);
    Some(Shown::Next(((now - prev) / (next - prev)).clamp(0.0, 1.0), next - now))
}

/// The ticks around `now` (seconds from the rest's start). Tick k's heal is due at s + C(k), C from the model (its
/// first heal less half a look period, then its intervals), and lands on the first look at or after it. The server's
/// start s is bounded by the rises seen, each placed at the tick it is nearest; a rise that fits no bound with the
/// others anchors alone (the model was off). Before the first heal the wait runs from the rest's start.
fn window(m: &Model, rises: &[f32], grid: Option<f32>, now: f32) -> (f32, f32) {
    let p = m.period;
    let c0 = m.first - p / 2.0;
    let gaps = m.resolved();
    let gap = |k: usize| gaps.get(k).or(gaps.last()).copied().unwrap_or(10.0).max(0.5);
    // C(k) for the ticks up to `until` (seconds), cumulative.
    let ticks = |until: f32| {
        let mut c = vec![c0];
        while *c.last().unwrap() < until && c.len() < 100_000 {
            let k = c.len() - 1;
            c.push(c[k] + gap(k));
        }
        c
    };
    const JITTER: f32 = 0.3;
    let (mut lo, mut hi) = (f32::MIN, f32::MAX);
    let mut s = 0.0;
    if let Some(&last) = rises.last() {
        let c = ticks(last + gap(0) * 2.0 + p);
        for &x in rises {
            let k = c
                .iter()
                .enumerate()
                .min_by(|a, b| (x - s - a.1 - p / 2.0).abs().total_cmp(&(x - s - b.1 - p / 2.0).abs()))
                .map_or(0, |(k, _)| k);
            let (a, b) = (x - c[k] - p - JITTER, x - c[k] + JITTER);
            (lo, hi) = if a.max(lo) <= b.min(hi) { (a.max(lo), b.min(hi)) } else { (a, b) };
            s = (lo + hi) / 2.0;
        }
    }
    let at = |ck: f32| match grid {
        Some(g) if p >= 1.0 => g + ((s + ck - g) / p - 0.02).ceil() * p,
        _ => s + ck + p / 2.0,
    };
    let c = ticks(now - s + gap(0) * 2.0 + p);
    let mut k = 0;
    while k + 1 < c.len() && at(c[k]) <= now {
        k += 1;
    }
    (if k > 0 { at(c[k - 1]) } else { 0.0 }, at(c[k]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB's ticks: every 10 s from the effect's start, healing from the second, each on the first 3 s look after.
    fn server(s: f32, phase: f32, k: f32) -> f32 {
        let x = s + 10.0 * k;
        phase + ((x - phase) / 3.0).ceil() * 3.0
    }

    /// A rest's rises as update learns them (Regen off, nothing skipped).
    fn rest(m: &mut Model, rises: &[f32]) {
        let mut r = RestTick { model: std::mem::take(m), chain: true, ..Default::default() };
        for (n, &t) in rises.iter().enumerate() {
            r.rises.push(t);
            if n == 0 {
                r.model.learn_first(t);
            } else if r.chain && r.model.fits(n - 1, t - rises[n - 1]) {
                r.model.learn_gap(n - 1, t - rises[n - 1]);
                if let Some(p) = look_period(&r.rises) {
                    r.model.period = p;
                }
            } else {
                r.chain = false;
            }
        }
        *m = r.model;
    }

    #[test]
    fn learns_the_look_period() {
        let rises: Vec<f32> = (2..7).map(|k| server(-0.1, 0.7, k as f32)).collect();
        let mut m = Model::default();
        rest(&mut m, &rises);
        assert!(m.ready() && (m.period - 3.0).abs() < 0.01, "{m:?}");
        assert!(m.gaps.iter().all(|&g| (whole(g) - 10.0).abs() <= 3.0));
        // Regen's 3 s ticks are not a look period's doing.
        assert_eq!(look_period(&[3.0, 6.0, 9.0, 12.0]), None);
    }

    #[test]
    fn predicts_lsb_looks() {
        for (s, phase) in (0..16).map(|i| (-0.02 - 0.013 * i as f32, 0.19 * i as f32)) {
            let mut m = Model::default();
            // A few rests teach it: each gap 9 or 12 s, the 10 s their mean.
            for r in 0..4 {
                let off = 0.37 * r as f32;
                let rises: Vec<f32> = (2..9).map(|k| server(s + off, phase, k as f32)).collect();
                rest(&mut m, &rises);
            }
            let rises: Vec<f32> = (2..6).map(|k| server(s, phase, k as f32)).collect();
            let grid = Some(rises[3]);
            for k in 6..12 {
                let now = server(s, phase, k as f32 - 1.0) + 0.5;
                let (prev, next) = window(&m, &rises, grid, now);
                assert!((next - server(s, phase, k as f32)).abs() < 0.05, "phase {phase} tick {k}: {next}");
                assert!(prev <= now && now < next);
            }
        }
    }

    /// A server whose ticks quicken to a floor: 20 s to the first heal, then 18, 16, 14, 12 and 10 from then on.
    fn quickening(s: f32, n: usize) -> Vec<f32> {
        let gaps = [18.0, 16.0, 14.0, 12.0];
        let mut t = s + 20.0;
        let mut out = vec![t];
        for i in 0..n - 1 {
            t += gaps.get(i).copied().unwrap_or(10.0);
            out.push(t);
        }
        out
    }

    #[test]
    fn follows_ticks_that_quicken() {
        let mut m = Model::default();
        rest(&mut m, &quickening(0.1, 8));
        assert_eq!(m.gaps.iter().map(|&g| whole(g)).collect::<Vec<_>>(), [18.0, 16.0, 14.0, 12.0, 10.0, 10.0, 10.0]);
        // The next rest: each tick foreseen from the one before it, and past the gaps learnt, the floor.
        let rises = quickening(0.3, 12);
        for k in 1..12 {
            let now = rises[k - 1] + 0.5;
            let (_, next) = window(&m, &rises[..k], None, now);
            assert!((next - rises[k]).abs() < 0.4, "tick {k}: {next} for {}", rises[k]);
        }
    }

    #[test]
    fn recovers_when_off() {
        // Learnt on a server 10 s apart; this rest's server started 4 s late (and its first heal with it).
        let mut m = Model::default();
        rest(&mut m, &[20.0, 30.0, 40.0, 50.0]);
        let rises = [24.0, 34.0];
        let (_, next) = window(&m, &rises, None, 35.0);
        assert!((next - 44.0).abs() < 0.4, "{next}");
        // A model that was learnt wrong (12 s) moves to the server's 10 s within a few rests.
        let mut m = Model { first: 20.0, gaps: vec![12.0], seen: vec![6, 6], period: 0.0 };
        for _ in 0..4 {
            rest(&mut m, &[20.0, 30.0, 40.0, 50.0, 60.0]);
        }
        assert!((m.gap(3) - 10.0).abs() < 0.3, "{m:?}");
    }

    /// LSB with Refresh +1 from gear (no icon) from `own_from` s, HP full: MP +1 on every 3 s look, the rest's heals
    /// (every 10 s from its start at 0, from the second, on the first look after) on top. Only the heals count.
    fn refresh_rest(own_from: f32) -> Vec<f32> {
        let base = Instant::now();
        let mut r = RestTick { chain: true, ..Default::default() };
        let look = |k: i32| 0.7 + 3.0 * k as f32;
        let mut events: Vec<(f32, u32)> = (-4..20).map(look).filter(|&t| t >= own_from).map(|t| (t, 1)).collect();
        for j in 2..6 {
            let at = (0..).map(look).find(|&t| t >= 10.0 * j as f32).unwrap();
            events.push((at, 12 + j as u32 - 2));
        }
        let mut mp = 500;
        let mut prev = -12.0;
        for i in 0..72 * 60 {
            let t = -12.0 + i as f32 / 60.0;
            mp += events.iter().filter(|e| e.0 > prev && e.0 <= t).map(|e| e.1).sum::<u32>();
            prev = t;
            let now = base + std::time::Duration::from_secs_f32(t + 12.0);
            r.feed(now, t >= 0.0, 1, [1000, mp], [true, false]);
        }
        r.rises
    }

    #[test]
    fn own_ticks_are_not_rest_ticks() {
        // Ticking before the rest, and only from just after it starts (a spell cast before sitting).
        for own_from in [-12.0, 0.5] {
            let rises = refresh_rest(own_from);
            let want = [21.7, 30.7, 42.7, 51.7];
            assert_eq!(rises.len(), want.len(), "{own_from}: {rises:?}");
            assert!(rises.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.05), "{own_from}: {rises:?}");
        }
    }

    /// Feeds MP (HP full) at each `(t, mp)` from 0.0, resting until `stand` s, a frame every 1/60 s up to `until`.
    fn feed_mp(events: &[(f32, u32)], stand: f32, until: f32) -> RestTick {
        let base = Instant::now();
        let mut r = RestTick { chain: true, ..Default::default() };
        for i in 0..=(until * 60.0) as u32 {
            let t = i as f32 / 60.0;
            let mp = 500 + events.iter().filter(|e| e.0 <= t).map(|e| e.1).sum::<u32>();
            r.feed(base + std::time::Duration::from_secs_f32(t), t < stand, 1, [1000, mp], [true, false]);
        }
        r
    }

    #[test]
    fn a_heal_just_before_standing_counts() {
        let r = feed_mp(&[(21.7, 12)], 23.0, 30.0);
        assert_eq!(r.rises.len(), 1, "{:?}", r.rises);
        assert!((r.rises[0] - 21.7).abs() < 0.05, "{:?}", r.rises);
        assert!((r.model.first - 21.7).abs() < 0.05, "{:?}", r.model);
    }

    #[test]
    fn a_heal_after_an_own_tick_in_its_hold_counts() {
        // Refresh +1 from 20.5 (first seen there), the rest heal 1.2 s later, Refresh's twin a look after.
        let r = feed_mp(&[(20.5, 1), (21.7, 12), (23.5, 1), (26.5, 1)], 40.0, 30.0);
        assert_eq!(r.rises.len(), 1, "{:?}", r.rises);
        assert!((r.rises[0] - 21.7).abs() < 0.05, "{:?}", r.rises);
    }

    #[test]
    fn a_skipped_tick_teaches_nothing() {
        // Full HP and MP for a tick: the 20 s gap is two ticks, so the chain stops there.
        let mut m = Model::default();
        rest(&mut m, &[20.0, 30.0, 40.0, 60.0, 70.0]);
        assert_eq!(m.gaps.len(), 2, "{m:?}");
        // Without rises the ticks run on by the clock.
        let (prev, next) = window(&m, &[20.0], None, 55.0);
        assert!((prev - 50.0).abs() < 0.4 && (next - 60.0).abs() < 0.4, "{prev} {next}");
    }
}
