//! Scheduler tasks (CMoSchedularTask, interpreter FUN_10057000) as far as effects need them: generators started,
//! stopped and killed on an actor pair, child schedulers looked up in the actors' files or by scope, dolls, record
//! loops and the conditional stack ops. Motions, reactions and battle messages stay with client/combat.rs.
//! Spec: ~/ffxi/lab/research/effects/triggers.md.

use super::Ref;
use crate::net::action::Action;
use std::cell::RefCell;
use std::rc::Rc;

/// An actor slot of a task or generator: an entity, a doll left where an actor stood (ops 0x15 / 0x16), or none.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Who {
    #[default]
    None,
    Actor(u32),
    Doll(usize),
}

impl Who {
    pub fn id(self) -> Option<u32> {
        match self {
            Who::Actor(id) => Some(id),
            _ => None,
        }
    }
}

/// The action a task belongs to (the 0x028 parse; +3 is the current record, shared by the task tree).
pub struct ActionCtx {
    pub action: Action,
    pub cur: usize,
    /// The swing scheduler client/combat.rs picked for op 0x24 (FUN_100d4600).
    pub swing: Option<String>,
    /// Records whose messages printed (op 0x2b marks the first one not yet printed; the rest print at the end).
    pub printed: Vec<bool>,
    /// client/combat.rs picked the melee swing and plays it; without one, op 0x24 picks and the files play it.
    pub client_swing: bool,
    /// The record op 0x2b printed last (+10): op 0x71 prints its proc / react message.
    pub last: usize,
}

impl ActionCtx {
    pub fn records(&self) -> Vec<(u32, &crate::net::action::ActionResult)> {
        self.action.records()
    }

    /// FUN_10061950: scheduler variables (triggers.md 0.6).
    pub fn var(&self, idx: u32, actor1: Option<u32>) -> i64 {
        let recs = self.records();
        let r = recs.get(self.cur.min(recs.len().saturating_sub(1))).map(|r| r.1);
        let side = |s: Option<&crate::net::action::Side>, k: u32| -> i64 {
            s.map_or(0, |s| match k {
                0 => s.kind as i64,
                1 => s.info as i64,
                2 => s.value as i64,
                _ => s.message as i64,
            })
        };
        match idx {
            0x21 => self.action.category as i64,
            0x22 => self.action.arg as i64,
            0x23 => self.action.info as i64,
            0x25 | 0x27 => recs.len() as i64,
            0x28 => r.map_or(0, |r| r.miss as i64),
            0x29 => r.map_or(0, |r| r.kind as i64),
            0x2a => r.map_or(0, |r| r.animation as i64),
            0x2b => r.map_or(0, |r| r.info as i64),
            0x2c => r.map_or(0, |r| r.scale as i64),
            0x2d => r.map_or(0, |r| r.value as i64),
            0x2e => r.map_or(0, |r| r.message as i64),
            0x2f..=0x32 => side(r.and_then(|r| r.proc.as_ref()), idx - 0x2f),
            0x33..=0x36 => side(r.and_then(|r| r.react.as_ref()), idx - 0x33),
            0x38 => (self.action.actor & 0xff00_0000 == 0) as i64,
            0x39 => actor1.is_some_and(|a| a >> 24 == 1) as i64,
            _ => 0,
        }
    }
}

/// One op: code, wait before the next op (i16 @4), duration (i16 @6), and its bytes.
#[derive(Clone, Debug)]
pub struct SOp {
    pub code: u8,
    pub wait: i16,
    pub dur: i16,
    pub raw: Vec<u8>,
}

impl SOp {
    pub fn u32_at(&self, o: usize) -> u32 {
        self.raw.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u16_at(&self, o: usize) -> u16 {
        self.raw.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }
    /// The 4cc at +8.
    pub fn name(&self) -> u32 {
        self.u32_at(8)
    }
}

/// The ops of a scheduler payload (FUN_10055aa0: the stream at data + u32 @0x14 - 0x10; length = byte 1 & 0x1f).
pub fn parse(data: &[u8]) -> Vec<SOp> {
    crate::dat::sched::ops(data)
        .into_iter()
        .map(|o| SOp { code: o.code, wait: o.wait as i16, dur: i16::from_le_bytes([o.raw.get(6).copied().unwrap_or(0), o.raw.get(7).copied().unwrap_or(0)]), raw: o.raw.to_vec() })
        .collect()
}

pub struct Task {
    pub res: Ref,
    pub ops: Rc<Vec<SOp>>,
    pub ip: usize,
    pub countdown: f32,
    /// +0x9c: multiplies every wait and duration.
    pub speed: f32,
    pub a1: Who,
    pub a2: Who,
    pub action: Option<Rc<RefCell<ActionCtx>>>,
    /// Call-and-wait (ops 0x3c / 0x3b / 0x24): the suspended parent, and +0xa0 (the parent resumes this many
    /// ticks before the child's last wait ends).
    pub parent: Option<usize>,
    pub early: f32,
    pub waiting: bool,
    /// Conditional clones die when they reach this op (their block's 0x6a or 0x67).
    pub stop_at: Option<usize>,
    pub stack: Vec<f64>,
    /// The last op 0x31 (record loop start).
    pub loop_at: Option<usize>,
    pub done: bool,
    /// Its op 0x05 / 0x28 motions are played (melee's are client/combat.rs's).
    pub motions: bool,
    /// A damage number's value and message colour 0xRRGGBB (task +0xa8 / +0xa4, FUN_1009e090).
    pub num: Option<(i32, u32)>,
    /// Unique per task (slots are reused): what a task owns ends with it.
    pub uid: u64,
    /// +0x141 (op 0x73's children): back to the first op at the end, until killed.
    pub looping: bool,
    /// Its end stream holds op 0x01: it starts over at its end too (FUN_10057c20).
    pub ends_loop: bool,
    /// The end op was reached on a loop: FUN_10057c20 resets it at the next step (no op runs then).
    pub restart: bool,
    /// Took its first step when an op created it: the frame's own step passes it by once.
    pub stepped: bool,
    /// +0x140 (op 0x52): the countdown runs on the Vana'diel clock (1/60 Vana'diel seconds).
    pub vana: bool,
    /// +0x22: paused while an actor of it is hidden (FUN_10074a30); no op progress, no time.
    pub paused: u32,
}

impl Task {
    pub fn new(res: Ref, ops: Rc<Vec<SOp>>, a1: Who, a2: Who, action: Option<Rc<RefCell<ActionCtx>>>, speed: f32) -> Self {
        // Melee: the attacker's swing is client/combat.rs's plan; the target's side (gurd / pary, a counter's swing)
        // plays its own motions and holds. A ranged finish plays from its files (shlg -> op 0x77 -> ls?? -> gu2).
        let motions = action.as_ref().is_none_or(|a| {
            let a = a.borrow();
            a.action.category != 1 || a1.id().is_some_and(|id| id != a.action.actor) || !a.client_swing
        });
        static UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let uid = UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { res, ops, ip: 0, countdown: 0.0, speed, a1, a2, action, parent: None, early: 0.0, waiting: false, stop_at: None, stack: Vec::new(), loop_at: None, done: false, motions, num: None, uid, looping: false, ends_loop: false, restart: false, stepped: false, vana: false, paused: 0 }
    }
}

/// The op after the 0x6a closing the 0x69 block that starts at `at` (FUN_1003d000).
pub fn block_end(ops: &[SOp], at: usize) -> usize {
    let mut depth = 0i32;
    for (i, o) in ops.iter().enumerate().skip(at) {
        match o.code {
            0x69 => depth += 1,
            0x6a => {
                depth -= 1;
                if depth <= 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    ops.len()
}
