//! Motion files loaded on top of an actor's model (the model's +0x7e4 list) and the type-0x07
//! schedulers that play them (model vtable +0x298 = FUN_100cce00): event motion banks and the PC
//! battle files. Lookups go newest file first, then the model (FUN_100cb8e0 walks the list from its
//! tail, and a region one file has filled is not taken from a later one).

use super::{Actor, World};
use crate::dat::DatIndex;
use crate::dat::character::{Animation, Character};
use crate::dat::motionfile::MotionFile;
use crate::dat::sched;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// FUN_100d22a0's kind: FUN_100d2340(kind) drops every file of a kind (1 battle, 2 event, 3 emote).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Battle,
    Event,
    Emote,
    /// Weapon-skill files (FUN_100d22a0 kind 6), put on by the effect system.
    Skill,
}

pub type Extra = Vec<(Kind, Arc<MotionFile>)>;

/// Model colour alpha at full opacity.
pub const OPAQUE: f32 = 128.0;

/// An event motion waiting for its files (ops 0x5b / 0x66 yield until FUN_100cf260 reports them loaded).
#[derive(Clone, Debug)]
pub struct Pending {
    pub files: Vec<u32>,
    pub name: String,
    pub kind: Kind,
}

/// Mouth motion of a speaker (FUN_10086710 / FUN_100867a0): the model's face motion (+0x164, default 'mou4',
/// else 'mou6') plays at once, then every 142 ticks, (chars + 20) / 20 times.
#[derive(Clone, Copy, Debug)]
pub struct Mouth {
    pub left: u32,
    pub next: f32,
    /// Ticks into the current playback, None between them.
    pub ticks: Option<f32>,
}

impl Mouth {
    pub fn new(chars: usize) -> Mouth {
        Mouth { left: (chars as u32 + 20) / 20, next: 0.0, ticks: None }
    }
}

/// Head look (actor +0x12c bits 8 / 9 -> model +0x840; FUN_10086820, FUN_100d38c0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LookMode {
    #[default]
    Off,
    /// Mode 1: at an actor's attach point (+0x142 / +0x144).
    Actor(u32, u16),
    /// Mode 2: along the facing, pitch / yaw in 4096ths of a turn (+0x146 / +0x148).
    Facing(i16, i16),
}

/// Weight ramp and direction ease a 1/60 s tick: twice retail's (0.04 a 30 fps frame *(inf)*, 1/32 a tick), the
/// user's call (2026-10-22): retail's turn is too slow.
const LOOK_RATE: f32 = 0.04;
const LOOK_EASE: f32 = 1.0 / 16.0;

/// The look as the skeleton holds it: K+0xb0, the look point in world space (None until placed 20 ahead), and
/// K+0xbc, the weight.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    pub mode: LookMode,
    pub point: Option<glam::Vec3>,
    pub weight: f32,
}

impl Look {
    /// FUN_100d5b10 / FUN_1002ac60 for one step. `frame` is the actor's matrix (x forward), `p3` the world head
    /// point, `target` the world point to look at. The weight fades in, or out with no target, a target behind
    /// (local x < -0.1) or one nearer than 0.3 across; at 0 the point goes back 20 ahead. With a target the look
    /// direction about the head eases toward it, keeping its distance.
    /// `rate` scales the weight's step (op 0x59 +0xe0).
    pub fn step(&mut self, frame: glam::Mat4, p3: glam::Vec3, target: Option<glam::Vec3>, ticks: f32, rate: f32) {
        let ahead = frame.transform_point3(glam::Vec3::new(20.0, 0.0, 0.0));
        let point = *self.point.get_or_insert(ahead);
        let inv = frame.inverse();
        let ok = target.filter(|t| {
            let l = inv.transform_point3(*t);
            l.x >= -0.1 && glam::Vec2::new(l.x, l.z).length() >= 0.3
        });
        match ok {
            Some(t) => {
                let v = point - p3;
                let len = v.length().max(1e-3);
                let want = (t - p3).normalize_or(v / len);
                let dir = (v / len).lerp(want, 1.0 - (1.0 - LOOK_EASE).powf(ticks)).normalize_or(want);
                self.point = Some(p3 + dir * len);
                self.weight = (self.weight + LOOK_RATE * rate * ticks).min(1.0);
            }
            None => {
                self.weight -= LOOK_RATE * rate * ticks;
                if self.weight < 0.0 {
                    self.weight = 0.0;
                    self.point = Some(ahead);
                }
            }
        }
    }
}

/// FUN_100a0b60 / FUN_100a0b90 (op 0x6e, s2c 0x05A): emote `n` with Param loads its files and plays "em0x" (states
/// 0xc, 0xd, 0xb). PC looks only; the standard-model table (0x10352414 by FUN_10082d70) is not traced. Err: /aim
/// failed.
pub fn emote(a: &mut Actor, n: u32, param: u16, status: u8, weapon_class: u8) -> Result<(), ()> {
    let Some(pc) = a.character.as_ref().and_then(|c| c.pc) else {
        log::info!("emote {n}: not a PC look");
        return Ok(());
    };
    let Some(m) = crate::dat::motionfile::emote_index(n, param, status, weapon_class)? else {
        log::info!("emote {n}: no motion");
        return Ok(());
    };
    let (files, name) = crate::dat::motionfile::emote_files(&pc, m);
    log::info!("emote {n}: motion {m}, files {files:?}, {name}");
    a.pending = Some(Pending { files, name: name.clone(), kind: Kind::Emote });
    a.emote = Some(name);
    Ok(())
}

/// Actor +0x1b8 is nonzero: an emote loads or plays (0x6e / 0x99 wait on it).
pub fn emoting(a: &Actor) -> bool {
    a.emote.is_some()
}

/// Battle files of a PC look (FUN_100cf650 on engaging, FUN_100d2340(1) after sheathing).
#[derive(Clone, Debug, Default)]
pub struct Battle {
    /// Files on the list.
    pub on: Vec<u32>,
    /// Sheathing: the "out N" scheduler plays, then the files go.
    pub leaving: bool,
}

/// Loads motion files on worker threads and caches them by file id.
pub struct Loader {
    tx: Sender<(u32, Option<Arc<MotionFile>>)>,
    rx: Receiver<(u32, Option<Arc<MotionFile>>)>,
    /// None while loading; Some(None) when the file is missing or bad.
    files: HashMap<u32, Option<Option<Arc<MotionFile>>>>,
}

impl Default for Loader {
    fn default() -> Self {
        let (tx, rx) = channel();
        Loader { tx, rx, files: HashMap::new() }
    }
}

impl Loader {
    pub fn poll(&mut self) {
        while let Ok((id, f)) = self.rx.try_recv() {
            self.files.insert(id, Some(f));
        }
    }

    /// The file once loaded (inner None = unusable), None while it loads; the first call starts it.
    pub fn get(&mut self, index: &Arc<DatIndex>, id: u32) -> Option<Option<Arc<MotionFile>>> {
        if let Some(f) = self.files.get(&id) {
            return f.clone();
        }
        self.files.insert(id, None);
        let (tx, index) = (self.tx.clone(), index.clone());
        std::thread::spawn(move || {
            let f = MotionFile::load(&index, id).map_err(|e| log::warn!("motion file {id}: {e:#}")).ok().map(|mut f| {
                f.packed = Some(Arc::new(crate::dat::animpack::pack(&f.animations)));
                Arc::new(f)
            });
            let _ = tx.send((id, f));
        });
        None
    }

    /// All of `ids` loaded (missing ones dropped), or None while any still loads.
    pub fn all(&mut self, index: &Arc<DatIndex>, ids: &[u32]) -> Option<Vec<Arc<MotionFile>>> {
        let got: Vec<_> = ids.iter().map(|&id| self.get(index, id)).collect();
        got.into_iter().collect::<Option<Vec<_>>>().map(|v| v.into_iter().flatten().collect())
    }
}

/// Puts files of a kind on the list (as the newest), replacing ones with the same id.
pub fn add(extra: &mut Extra, kind: Kind, files: Vec<Arc<MotionFile>>) {
    for f in files {
        extra.retain(|(_, x)| x.id != f.id);
        extra.push((kind, f));
    }
}

pub fn drop_kind(extra: &mut Extra, kind: Kind) {
    extra.retain(|(k, _)| *k != kind);
}

/// Animation chunks of a motion (FUN_100cb500): chunks matching the name's first 3 letters (mask 0xffffff), each
/// in the body region its 4th character names (0..6), the first one per region in search order (newest file first).
/// So "idl0" plays idl0, idl1 and idl2.
pub fn find<'a>(extra: &'a Extra, ch: &'a Character, name: &str) -> Vec<&'a Animation> {
    let sets = extra.iter().rev().map(|(_, f)| f.animations.as_slice()).chain(std::iter::once(ch.animations.as_slice()));
    region_chunks(sets.flat_map(|s| s.iter()), name)
}

/// The first chunk per region among `chunks` whose name shares `name`'s first 3 letters.
pub fn region_chunks<'a>(chunks: impl Iterator<Item = &'a Animation>, name: &str) -> Vec<&'a Animation> {
    let prefix = name.get(..3).unwrap_or(name);
    let mut regions = HashSet::new();
    chunks
        .filter(|a| {
            let n = a.name.trim_end().as_bytes();
            n.len() == 4 && n.starts_with(prefix.as_bytes()) && (b'0'..=b'6').contains(&n[3]) && regions.insert(n[3])
        })
        .collect()
}

/// `find` results by name for one actor, as (source, index): source 0 the model, k + 1 its k-th file. Kept
/// until the model or the file list changes, so the name lookups are not redone every frame.
#[derive(Default)]
pub struct FindCache {
    sig: Vec<usize>,
    map: HashMap<String, Vec<(usize, usize)>>,
}

/// `find` through the actor's cache.
pub fn find_cached<'a>(cache: &mut FindCache, extra: &'a Extra, ch: &'a Character, name: &str) -> Vec<&'a Animation> {
    let sig: Vec<usize> = std::iter::once(ch as *const Character as usize).chain(extra.iter().map(|(_, f)| Arc::as_ptr(f) as usize)).collect();
    if cache.sig != sig {
        cache.sig = sig;
        cache.map.clear();
    }
    let sources: Vec<&[Animation]> = std::iter::once(ch.animations.as_slice()).chain(extra.iter().map(|(_, f)| f.animations.as_slice())).collect();
    if let Some(hit) = cache.map.get(name) {
        return hit.iter().filter_map(|&(s, i)| sources.get(s)?.get(i)).collect();
    }
    let found = find(extra, ch, name);
    let ids = found
        .iter()
        .filter_map(|a| sources.iter().enumerate().find_map(|(s, list)| list.iter().position(|x| std::ptr::eq(x, *a)).map(|i| (s, i))))
        .collect();
    cache.map.insert(name.to_string(), ids);
    found
}

/// A type-0x07 scheduler by name, newest file first, then the model (FUN_100cc440).
pub fn scheduler<'a>(extra: &'a Extra, ch: &'a Character, name: &str) -> Option<&'a [u8]> {
    let files = extra.iter().rev().flat_map(|(_, f)| f.schedulers.iter());
    files.chain(ch.schedulers.iter()).find(|(n, _)| n == name).map(|(_, d)| d.as_slice())
}

/// Scheduler names newest file first, then the model's.
pub fn scheduler_names<'a>(extra: &'a Extra, ch: &'a Character) -> Vec<&'a str> {
    let files = extra.iter().rev().flat_map(|(_, f)| f.schedulers.iter());
    files.chain(ch.schedulers.iter()).map(|(n, _)| n.as_str()).collect()
}

/// A running scheduler's motions (CMoSchedularTask, op 0x05 only). It is busy for its header length
/// (0x53 waits on it, FUN_10083650 -> FUN_10056060); its last motion keeps playing after that while
/// it is a loop (count 0), since the status motion only comes back when its name changes
/// (FUN_100c6380, +0x79c set by vtable +0x1fc when a counted motion ends).
#[derive(Clone, Debug)]
pub struct Play {
    pub name: String,
    pub motions: Vec<sched::Motion>,
    pub length: f32,
    pub ticks: f32,
    /// The status motion when it started; a change ends a looping hold.
    pub base: String,
    /// Motions started before this tick were cut by a forced default (+0x7d8 cleared, e.g. op 0x5e).
    pub cut: f32,
    /// Scheduler op 0x28: the status motion comes back at this tick.
    pub stance_at: Option<f32>,
}

impl Play {
    pub fn new(name: &str, data: &[u8], base: &str) -> Play {
        let p = sched::plan(data, &[]);
        let mut play = Play { name: name.to_string(), motions: p.motions, length: p.length as f32, ticks: 0.0, base: base.to_string(), cut: -1.0, stance_at: None };
        play.stance_at = p.stance_at.map(|t| t as f32);
        play
    }

    /// The status motion is forced back now (FUN_100c6380 sees +0x7d8 = "    ").
    pub fn cut(&mut self) {
        self.cut = self.ticks;
    }

    /// 0x53: the scheduler task still exists.
    pub fn busy(&self) -> bool {
        self.ticks <= self.length + 1.0
    }

    /// The motion playing now and its start tick.
    pub fn current(&self) -> Option<&sched::Motion> {
        let cut = match self.stance_at {
            Some(s) if s <= self.ticks => self.cut.max(s + 0.5),
            _ => self.cut,
        };
        self.motions.iter().rev().find(|m| m.at as f32 <= self.ticks).filter(|m| m.at as f32 >= cut)
    }

    /// Over once the scheduler ended and its last motion was counted (or none), or the status motion changed.
    pub fn over(&self, base: &str) -> bool {
        if base != self.base {
            return true;
        }
        if self.busy() {
            return false;
        }
        match self.current() {
            Some(m) if m.count == 0 => false,
            Some(m) => self.ticks >= m.at as f32 + (m.ticks.max(1) as f32) * m.count.max(1) as f32,
            None => true,
        }
    }
}

/// The rest of a scheduler whose motions this module plays (FUN_10083650 runs the whole thing): its sounds (the
/// weapon draw / sheathe `sinr` / `sotr`, gesture sounds), generators and holds run in fx with the motions off.
pub(super) fn fx_too(fx: &mut crate::fx::Effects, id: u32, name: &str) {
    if let Ok(n) = <[u8; 4]>::try_from(name.as_bytes()) {
        fx.on_scheduler_quiet(id, u32::from_le_bytes(n));
    }
}

/// Starts scheduler  from the actor's files (model vtable +0x298); a bare motion of that name
/// plays once when there is no scheduler. Returns false when the actor has neither.
pub(super) fn start(a: &mut Actor, ch: &Character, name: &str, base: &str) -> bool {
    let play = match scheduler(&a.extra, ch, name) {
        Some(d) => Play::new(name, d, base),
        None if !find(&a.extra, ch, name).is_empty() => {
            let len = find(&a.extra, ch, name).iter().map(|x| x.frames as f32 / (x.speed * 0.5).max(1e-3)).fold(0.0, f32::max);
            let m = sched::Motion { name: name.to_string(), at: 0, speed: 1.0, ticks: len.ceil() as u16, count: 1, blend: crate::motion::BLEND_TICKS as u16 };
            Play { name: name.to_string(), motions: vec![m], length: len, ticks: 0.0, base: base.to_string(), cut: -1.0, stance_at: None }
        }
        None => return false,
    };
    // Blend from whatever plays now, even into a motion of the same name; the newest motion wins over a one-shot.
    if !a.motion.is_empty() {
        a.prev = Some((std::mem::take(&mut a.motion), a.ticks, a.once));
        a.blend = 0.0;
    }
    a.oneshot = None;
    a.play = Some(play);
    true
}

/// A scheduler's op 0x05 run by the effect system (fx/tasks.rs) on actor `id`, or its op 0x28 (None): the
/// status motion comes back.
/// A motion's length in ticks at `speed` (the longest of its regions).
pub(super) fn motion_len(a: &Actor, ch: &Character, name: &str, speed: f32) -> f32 {
    find(&a.extra, ch, name).iter().map(|x| x.frames as f32 / (x.speed * speed * 0.5).max(1e-3)).fold(0.0, f32::max)
}

pub(super) fn play_op(w: &mut World, id: u32, m: Option<sched::Motion>) {
    let Some(ch) = w.actors.get(&id).and_then(|a| a.character.clone()) else { return };
    let base = status_motion(w, id, &ch);
    let Some(a) = w.actors.get_mut(&id) else { return };
    match m {
        Some(m) if !find(&a.extra, &ch, &m.name).is_empty() => {
            let len = if m.ticks > 0 { m.ticks as f32 } else { find(&a.extra, &ch, &m.name).iter().map(|x| x.frames as f32 / (x.speed * m.speed * 0.5).max(1e-3)).fold(0.0, f32::max) };
            if !a.motion.is_empty() {
                a.prev = Some((std::mem::take(&mut a.motion), a.ticks, a.once));
                a.blend = 0.0;
            }
            a.play = Some(Play { name: m.name.clone(), motions: vec![m], length: len, ticks: 0.0, base, cut: -1.0, stance_at: None });
        }
        Some(m) => log::debug!("actor {id:#x}: scheduler motion {} not in its files", m.name),
        None => {
            if let Some(p) = &mut a.play {
                p.stance_at = Some(p.ticks);
            }
        }
    }
}

/// The status motion an actor would play now (FUN_100c6b70), for ending looping holds.
pub(super) fn status_motion(w: &World, id: u32, ch: &Character) -> String {
    let (status, ratio) = if let Some(p) = super::event::pose(w, id) {
        (0, p.ratio)
    } else if id == w.my_id {
        (w.my_motion.status, if w.moving { 1.0 } else { 0.0 })
    } else {
        w.entities.get(&id).map_or((0, 0.0), |e| (e.motion.status, e.ratio))
    };
    crate::motion::choose(&ch.motions, status, ratio).0.to_string()
}

fn status(w: &World, id: u32) -> u8 {
    if super::event::pose(w, id).is_some() {
        return 0;
    }
    if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) }
}

/// Per frame: loads requested files, swaps battle files on engage / disengage (with the "in N" /
/// "out N" schedulers), and advances schedulers and fades.
pub(super) fn update(w: &mut World, dt: f32) {
    w.motion_files.poll();
    let Some(index) = w.index.clone() else { return };
    let ticks = dt * 60.0;
    let ids: Vec<u32> = w.actors.keys().copied().collect();
    for id in ids {
        let Some(ch) = w.actors.get(&id).and_then(|a| a.character.clone()) else { continue };
        let base = status_motion(w, id, &ch);
        let engaged = status(w, id) == 1;
        let want = match (&ch.pc, engaged) {
            (Some(pc), true) => crate::dat::motionfile::battle_files(pc),
            _ => Vec::new(),
        };
        let a = w.actors.get_mut(&id).unwrap();
        if let Some(p) = a.pending.clone()
            && let Some(files) = w.motion_files.all(&index, &p.files)
        {
            a.pending = None;
            if p.kind == Kind::Skill {
                drop_kind(&mut a.extra, Kind::Skill);
            }
            add(&mut a.extra, p.kind, files);
            if p.name.is_empty() {
                // Files only: the effect system's scheduler plays from them.
            } else if start(a, &ch, &p.name, &base) {
                log::info!("actor {id:#x}: event motion {} (files {:?})", p.name, p.files);
                fx_too(&mut w.effects, id, &p.name);
            } else {
                log::info!("actor {id:#x}: event motion {} not in its files {:?}", p.name, p.files);
            }
        }
        if !want.is_empty() && want != a.battle.on {
            if let Some(files) = w.motion_files.all(&index, &want) {
                drop_kind(&mut a.extra, Kind::Battle);
                add(&mut a.extra, Kind::Battle, files);
                log::info!("actor {id:#x}: engaged, battle files {want:?}");
                // The draw ("in N") plays when the actor becomes engaged, not when its files change mid-fight (a
                // weapon change while engaged).
                let drawing = a.battle.on.is_empty() || a.battle.leaving;
                a.battle = Battle { on: want, leaving: false };
                if drawing && let Some(pc) = &ch.pc {
                    let name = crate::dat::motionfile::draw_scheduler(pc, false);
                    if start(a, &ch, &name, &base) {
                        fx_too(&mut w.effects, id, &name);
                    }
                }
            }
        } else if want.is_empty() && !a.battle.on.is_empty() && !a.battle.leaving {
            a.battle.leaving = true;
            log::info!("actor {id:#x}: disengaged, sheathing");
            let out = ch.pc.as_ref().map(|pc| crate::dat::motionfile::draw_scheduler(pc, true));
            match out {
                Some(n) if start(a, &ch, &n, &base) => fx_too(&mut w.effects, id, &n),
                _ => a.play = None,
            }
        }
        if let Some(p) = &mut a.play {
            p.ticks += ticks;
        }
        if a.play.as_ref().is_some_and(|p| p.over(&base)) {
            a.play = None;
        }
        // State 0xb: the emote's scheduler is over; FUN_100d2340(3) drops its files.
        if a.emote.is_some() && a.pending.is_none() && a.play.as_ref().is_none_or(|p| Some(&p.name) != a.emote.as_ref() || !p.busy()) {
            a.emote = None;
            drop_kind(&mut a.extra, Kind::Emote);
        }
        if let Some(m) = &mut a.mouth {
            match &mut m.ticks {
                Some(t) => *t += ticks,
                None if m.left > 0 && m.next <= 0.0 => {
                    m.ticks = Some(0.0);
                    m.left -= 1;
                    m.next = 142.0;
                }
                None => {}
            }
            m.next -= ticks;
        }
        if a.battle.leaving && a.play.is_none() {
            drop_kind(&mut a.extra, Kind::Battle);
            a.battle = Battle::default();
        }
        if let Some(f) = &mut a.fade {
            let done = f.tick(ticks);
            a.alpha = f.alpha;
            if done {
                a.fade = None;
            }
        }
    }
}

/// A region 5 / 6 overlay (FUN_100cd920 into FUN_1001ad00's queue): `name` at `weight` (the entry's +0x1c), `cycles`
/// times through (+0x2c), in over 1 tick, then faded out over `fade` ticks (+0x20, FUN_1001b590).
#[derive(Clone, Debug)]
pub struct Overlay {
    pub name: String,
    pub ticks: f32,
    pub weight: f32,
    pub cycles: u8,
    pub fade: f32,
    /// Op 0x25's rate (frames advance at speed x rate) and blend-in ticks.
    pub rate: f32,
    pub blend_in: f32,
}

impl Overlay {
    /// The frame and envelope weight `ticks` in, for an animation of `frames` at `speed`; None once faded out.
    pub fn sample(&self, frames: u32, speed: f32) -> Option<(f32, f32)> {
        let per = frames.max(1) as f32;
        let speed = speed * self.rate.max(1e-3);
        let f = self.ticks * speed * 0.5;
        let full = per * self.cycles as f32;
        let frame = if f < full { f % per } else { per - 1.0 };
        let env = if f < full {
            (self.ticks / self.blend_in.max(1.0)).min(1.0)
        } else {
            let over = self.ticks - full / (speed * 0.5).max(1e-3);
            1.0 - over / self.fade.max(1.0)
        };
        (env > 0.0).then_some((frame.min(per - 1.0), self.weight * env))
    }
}

/// Actor alpha (model colour high byte, 0x80 = opaque): op 0x6c (FUN_100b5800) steps it linearly
/// to the target over the frames; DAT 0 "kesu" op 0x29 fades it to 0 over 90 ticks.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    pub alpha: f32,
    pub target: f32,
    pub step: f32,
    pub left: f32,
}

impl Fade {
    pub fn to(from: f32, target: u8, frames: i32) -> Fade {
        let n = frames.max(1) as f32;
        let target = target as f32;
        Fade { alpha: from, target, step: (target - from) / n, left: n }
    }

    /// Returns true when finished.
    pub fn tick(&mut self, ticks: f32) -> bool {
        self.left -= ticks;
        if self.left < 0.0 {
            self.alpha = self.target;
            return true;
        }
        self.alpha += self.step * ticks;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2-frame hit pose at speed 1: in over a tick, full for 2 cycles (8 ticks), out over 24, then gone.
    #[test]
    fn reaction_overlay_envelope() {
        let mut o = Overlay { name: "dfi".into(), ticks: 0.5, weight: 0.5, cycles: 2, fade: 24.0, rate: 1.0, blend_in: 1.0 };
        assert_eq!(o.sample(2, 1.0), Some((0.25, 0.25)));
        o.ticks = 4.0;
        assert_eq!(o.sample(2, 1.0).map(|s| s.1), Some(0.5));
        o.ticks = 20.0;
        let (frame, w) = o.sample(2, 1.0).unwrap();
        assert_eq!(frame, 1.0);
        assert!((w - 0.5 * (1.0 - 12.0 / 24.0)).abs() < 1e-5);
        o.ticks = 33.0;
        assert_eq!(o.sample(2, 1.0), None);
    }

    /// Curilla (model 69) and Rahal (an Elvaan M look) have the mouth motion and the attach points the head
    /// look uses (3, 6, 7).
    #[test]
    fn opening_speakers() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let rahal = crate::dat::npc::parse_look(&hex("0100060300100c200c300c400c5005601b700000")).unwrap();
        for look in [crate::dat::npc::NpcLook::Model(69), rahal] {
            let ch = crate::dat::npc::load_npc(&index, &look).unwrap();
            let mouth = ["mou4", "mou6"].iter().map(|n| find(&Vec::new(), &ch, n).len()).find(|n| *n > 0);
            println!("{look:?}: mouth {mouth:?}, points {:?}", ch.skeleton.points.iter().take(8).map(|p| p.0).collect::<Vec<_>>());
            assert!(mouth.is_some(), "{look:?} has no mou4 / mou6");
            assert!(ch.skeleton.points.len() > 7);
        }
        let (mut l, head) = (Look::default(), glam::Vec3::new(0.0, -1.5, 0.0));
        let want = glam::Vec3::new(3.0, -1.5, 2.0);
        for _ in 0..200 {
            l.step(glam::Mat4::IDENTITY, head, Some(want), 1.0, 1.0);
        }
        let dir = (l.point.unwrap() - head).normalize();
        assert!((l.weight - 1.0).abs() < 1e-6 && dir.dot((want - head).normalize()) > 0.99);
        // Behind the actor: the weight ramps down and the point goes back ahead.
        for _ in 0..60 {
            l.step(glam::Mat4::IDENTITY, head, Some(glam::Vec3::new(-3.0, 0.0, 0.0)), 1.0, 1.0);
        }
        assert_eq!((l.weight, l.point), (0.0, Some(glam::Vec3::new(20.0, 0.0, 0.0))));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Hume M 'tlk0' (file 32712, event 0x66 #0): ta1? once, then the ta2? loop held after the 306 ticks.
    #[test]
    fn talk_scheduler() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let f = MotionFile::load(&index, 32712).unwrap();
        let d = &f.schedulers.iter().find(|(n, _)| n == "tlk0").unwrap().1;
        let mut p = Play::new("tlk0", d, "idl");
        assert_eq!(p.length, 306.0);
        assert_eq!(p.motions.iter().map(|m| (m.name.as_str(), m.at, m.count)).collect::<Vec<_>>(), [("ta1", 0, 1), ("ta2", 53, 0)]);
        p.ticks = 400.0;
        assert!(!p.busy() && !p.over("idl") && p.over("wlk"));
        assert_eq!(p.current().unwrap().name, "ta2");
        p.cut();
        assert!(p.current().is_none() && p.over("idl"));
        let mut fd = Fade::to(128.0, 0, 90);
        for _ in 0..89 {
            assert!(!fd.tick(1.0));
        }
        assert!(fd.tick(2.0) && fd.alpha == 0.0);
    }
}
