//! Running scheduler tasks, generator attachment to actors (FUN_10052eb0 / FUN_1003afd0), effect files loaded in
//! the background, and the entry points from 0x028 actions (FUN_1009caa0, FUN_100a78c0, FUN_10083d60).

use super::lib::Library;
use super::sched::{ActionCtx, SOp, Task, Who, block_end, parse};
use super::{Actors, Effects, Frame, Owner, Ref};
use crate::dat::generator::kind;
use crate::net::action::Action;
use glam::{Mat4, Vec3};
use std::cell::RefCell;
use std::rc::Rc;

/// What one op asks of the task loop.
enum Flow {
    Next,
    /// The op set `ip` itself.
    Jump,
    /// Call-and-wait: the task is suspended until its child ends; then it goes on after this op.
    Wait,
    /// A file is still loading: run this op again next frame.
    Retry,
    End,
}

/// A start that waits for a file (an effect file, or an actor's files).
pub(super) struct Pending {
    pub file: Option<u32>,
    pub actor: Option<u32>,
    pub name: u32,
    pub a1: Who,
    pub a2: Who,
    pub action: Option<Rc<RefCell<ActionCtx>>>,
    pub speed: f32,
    /// FUN_10083d60(caster, 0) once the file is in (a cast ends).
    pub end_cast: Option<u32>,
    pub age: f32,
}

/// A doll (CXiDollActor, FUN_100aa470): where an actor stood when a scheduler froze it.
pub struct Doll {
    pub of: u32,
    pub matrix: Mat4,
}

const MAIN: u32 = u32::from_le_bytes(*b"main");

/// FUN_100a7380: the effect file base by action category.
fn effect_base(category: u8, anim: u32) -> Option<u32> {
    Some(match category {
        3 | 0xe | 0xf => 0xd48,
        4 | 8 => 0xaf0,
        5 | 9 => {
            if anim > 99 {
                0xdca5
            } else {
                0x1330
            }
        }
        6 | 10 => 0x113c,
        7 | 11 => match anim {
            0x800.. => 0x14b07,
            0x600.. => 0xe739,
            0x200.. => 0xc1ef,
            _ => 0xf3c,
        },
        0xd => 0xc0ef,
        _ => return None,
    })
}

impl Effects {
    pub fn set_index(&mut self, index: std::sync::Arc<crate::dat::DatIndex>) {
        self.index = Some(index);
    }

    /// The library of a DAT, loading it in the background on first use (None until it is in).
    pub(super) fn lib_for(&mut self, file: u32) -> Option<usize> {
        if let Some(l) = self.reg.lib_of(file) {
            return Some(l);
        }
        if self.loading.insert(file)
            && let Some(index) = self.index.clone()
        {
            let tx = self.load_tx.clone();
            std::thread::spawn(move || {
                let lib = index.read(file).and_then(|f| Library::load(file, f)).map_err(|e| log::warn!("effects: DAT {file}: {e:#}")).ok();
                let _ = tx.send((file, lib));
            });
        }
        None
    }

    /// Loads an actor's files ahead of its first scheduler (a model came in).
    pub fn preload(&mut self, files: &[u32]) {
        for &f in files {
            self.lib_for(f);
        }
    }

    pub(super) fn poll_loads(&mut self) {
        while let Ok((file, lib)) = self.load_rx.try_recv() {
            if let Some(l) = lib
                && self.reg.lib_of(file).is_none()
            {
                self.reg.add(l);
            }
        }
    }

    /// A 0x028 action (FUN_1009caa0). `swing` is the swing scheduler combat picked for melee (op 0x24).
    pub fn on_action(&mut self, a: &Action, swing: Option<String>) {
        let caster = a.actor;
        let first = a.targets.first().map_or(Who::None, |t| Who::Actor(t.id));
        let records = a.records().len();
        // FUN_10055cc0: several records outside categories 14 / 15 run at 0.7.
        let speed = if records > 1 && !matches!(a.category, 0xe | 0xf) { 0.7 } else { 1.0 };
        let ctx = Rc::new(RefCell::new(ActionCtx { action: a.clone(), cur: 0, swing }));
        match a.category {
            1 | 2 => self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: None, age: 0.0 }),
            3..=6 | 0xb | 0xd..=0xf => {
                let anim = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.animation as u32);
                let Some(base) = effect_base(a.category, anim) else { return };
                // Skeleton casters of weapon skills load per-race files (FUN_100d0570); those are not loaded yet
                // *(inf: falls back to the generic file)*.
                self.pending.push(Pending { file: Some(base + anim), actor: None, name: MAIN, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: Some(caster), age: 0.0 });
            }
            7..=0xa | 0xc => {
                // Starts: the cmd_arg scheduler on the caster itself; "ca??" goes through FUN_10083d60 first.
                if a.arg & 0xffff == 0x6163 {
                    self.cast_state(caster, a.arg);
                }
                self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0 });
            }
            _ => {}
        }
    }

    /// FUN_10083d60(caster, arg): a stored cast 4cc runs as "st??" (ends the casting effect) and is cleared; a new
    /// one is stored when none was.
    fn cast_state(&mut self, caster: u32, arg: u32) {
        match self.casting.remove(&caster) {
            Some(prev) => {
                let st = (prev & 0xffff_7473) | 0x7473;
                self.pending.push(Pending { file: None, actor: Some(caster), name: st, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0 });
            }
            None if arg != 0 => {
                self.casting.insert(caster, arg);
            }
            None => {}
        }
    }

    /// 0x038: a scheduler by 4cc on an actor (FUN_100cce00 with no action).
    pub fn on_scheduler(&mut self, actor: u32, target: u32, name: u32) {
        self.pending.push(Pending { file: None, actor: Some(actor), name, a1: Who::Actor(actor), a2: Who::Actor(target), action: None, speed: 1.0, end_cast: None, age: 0.0 });
    }

    /// Starts waiting for files, when they are in.
    pub(super) fn run_pending(&mut self, f: &Frame, actors: &dyn Actors) {
        let list = std::mem::take(&mut self.pending);
        for mut p in list {
            p.age += f.dt;
            let found = match (p.file, p.actor) {
                (Some(file), _) => match self.lib_for(file) {
                    Some(l) => Ok(self.reg.libs[l].find_global(kind::SCHEDULER, p.name).map(|n| (l, n))),
                    None => Err(()),
                },
                (None, Some(actor)) => self.find_on_actor(actor, p.name, actors, true, true),
                _ => Ok(None),
            };
            match found {
                Ok(res) => {
                    if let Some(c) = p.end_cast {
                        self.cast_state(c, 0);
                    }
                    match res {
                        Some(r) => {
                            self.spawn_task(r, p.a1, p.a2, p.action, p.speed, None);
                        }
                        None => log::debug!("effects: scheduler {} not found", String::from_utf8_lossy(&p.name.to_le_bytes())),
                    }
                }
                // Give up on a file that does not come within 10 s.
                Err(()) if p.age < 600.0 => self.pending.push(p),
                Err(()) => {}
            }
        }
    }

    /// FUN_100cc970 (equipment part files, `parts`) then FUN_100cc440 (motion files newest first, then the model's)
    /// then DAT 0 (`global`). Err while one of those files is still loading.
    fn find_on_actor(&mut self, actor: u32, name: u32, actors: &dyn Actors, parts: bool, global: bool) -> Result<Option<Ref>, ()> {
        let (part_files, motion_files) = actors.files(actor);
        let order: Vec<u32> = if parts { part_files.into_iter().chain(motion_files).collect() } else { motion_files.into_iter().chain(part_files).collect() };
        let mut waiting = false;
        for file in order {
            match self.lib_for(file) {
                Some(l) => {
                    if let Some(n) = self.reg.libs[l].find_global(kind::SCHEDULER, name) {
                        return Ok(Some((l, n)));
                    }
                }
                None => waiting = true,
            }
        }
        if waiting {
            return Err(());
        }
        if global && let Some(g) = self.reg.global {
            return Ok(self.reg.libs[g].find_global(kind::SCHEDULER, name).map(|n| (g, n)));
        }
        Ok(None)
    }

    pub(super) fn spawn_task(&mut self, r: Ref, a1: Who, a2: Who, action: Option<Rc<RefCell<ActionCtx>>>, speed: f32, parent: Option<usize>) -> usize {
        let ops = self.sched_ops(r);
        let mut t = Task::new(r, ops, a1, a2, action, speed);
        t.parent = parent;
        if let Some(p) = parent.and_then(|p| self.tasks.get(p)?.as_ref()) {
            t.motions = p.motions;
        }
        // Not the slot of the task being run (it is taken out while it runs).
        let busy = self.stepping;
        match self.tasks.iter().enumerate().position(|(i, t)| t.is_none() && Some(i) != busy) {
            Some(i) => {
                self.tasks[i] = Some(t);
                i
            }
            None => {
                self.tasks.push(Some(t));
                self.tasks.len() - 1
            }
        }
    }

    fn sched_ops(&mut self, r: Ref) -> Rc<Vec<SOp>> {
        if let Some(o) = self.sched_cache.get(&r) {
            return o.clone();
        }
        let o = Rc::new(parse(self.reg.libs[r.0].data(r.1)));
        self.sched_cache.insert(r, o.clone());
        o
    }

    /// Every task one frame on (the CMoSchedularTask update): countdown -= ticks; ops run while it is <= 0, each
    /// adding its wait x speed.
    pub(super) fn step_tasks(&mut self, f: &Frame, actors: &dyn Actors) {
        let mut i = 0;
        while i < self.tasks.len() {
            let Some(mut t) = self.tasks[i].take() else {
                i += 1;
                continue;
            };
            self.stepping = Some(i);
            if !t.waiting {
                t.countdown -= f.dt;
            }
            let mut guard = 0;
            while !t.done && !t.waiting && t.countdown <= 0.0 && guard < 512 {
                guard += 1;
                if t.ip >= t.ops.len() || t.stop_at == Some(t.ip) {
                    t.done = true;
                    break;
                }
                let op = t.ops[t.ip].clone();
                t.countdown += op.wait as f32 * t.speed;
                match self.run_op(i, &mut t, &op, actors) {
                    Flow::Next => t.ip += 1,
                    Flow::Jump => {}
                    Flow::Wait => {
                        t.ip += 1;
                        t.waiting = true;
                    }
                    Flow::Retry => {
                        t.countdown = t.countdown.max(0.0) - op.wait as f32 * t.speed + 1e-3;
                        break;
                    }
                    Flow::End => t.done = true,
                }
                // FUN_10061e30: reaching the end op with +0xa0 resumes the parent that much sooner.
                if t.early != 0.0 && t.ops.get(t.ip).is_none_or(|o| o.code == 0) {
                    t.countdown -= t.early;
                    t.early = 0.0;
                }
            }
            if t.done {
                if let Some(p) = t.parent
                    && let Some(Some(pt)) = self.tasks.get_mut(p)
                {
                    pt.waiting = false;
                }
            } else {
                self.tasks[i] = Some(t);
            }
            self.stepping = None;
            i += 1;
        }
    }

    fn run_op(&mut self, me: usize, t: &mut Task, op: &SOp, actors: &dyn Actors) -> Flow {
        let scope = |s: &Self, k: u8| s.reg.resolve(t.res.0, t.res.1, k, op.name());
        match op.code {
            0x00 => Flow::End,
            0x02 => {
                self.op_start_generator(me, t, op, actors);
                Flow::Next
            }
            0x1e | 0x2d => {
                if let Some(g) = scope(self, kind::GENERATOR) {
                    self.stop_generator(g, t.a1, t.a2, op.code == 0x2d);
                }
                Flow::Next
            }
            0x03 | 0x73 => {
                if let Some(r) = scope(self, kind::SCHEDULER) {
                    self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, None);
                }
                Flow::Next
            }
            0x3b => match scope(self, kind::SCHEDULER) {
                Some(r) => {
                    self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, Some(me));
                    Flow::Wait
                }
                None => Flow::Next,
            },
            // On actor2 with the actors swapped / on actor1: its part files, motion and model files, then scope.
            0x09 | 0x57 | 0x85 => {
                let (on, a1, a2) = if op.code == 0x09 { (t.a2, t.a2, t.a1) } else { (t.a1, t.a1, t.a2) };
                let found = match on.id() {
                    Some(id) => match self.find_on_actor(id, op.name(), actors, true, false) {
                        Ok(r) => r,
                        Err(()) => return Flow::Retry,
                    },
                    None => None,
                };
                let r = found.or_else(|| scope(self, kind::SCHEDULER));
                if let Some(r) = r {
                    if op.code == 0x85 {
                        self.kill_tasks(r, Some((t.a1, t.a2)));
                    } else {
                        self.spawn_task(r, a1, a2, t.action.clone(), t.speed, None);
                    }
                }
                Flow::Next
            }
            // The caster's own scheduler, call-and-wait, no global fallback.
            0x3c => {
                let Some(id) = t.a1.id() else { return Flow::Next };
                match self.find_on_actor(id, op.name(), actors, false, false) {
                    Ok(Some(r)) => {
                        self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, Some(me));
                        Flow::Wait
                    }
                    Ok(None) => Flow::Next,
                    Err(()) => Flow::Retry,
                }
            }
            0x5f => {
                if let Some(r) = scope(self, kind::SCHEDULER) {
                    self.kill_tasks(r, None);
                }
                Flow::Next
            }
            // Swing pick (FUN_100d4600, combat chose it) and the counter swing: call-and-wait, parent back 10 sooner.
            0x24 | 0x5d | 0x5c => {
                let name = if op.code == 0x5c { Some("cni0".to_string()) } else { t.action.as_ref().and_then(|a| a.borrow().swing.clone()) };
                let (Some(name), Some(id)) = (name, t.a1.id()) else { return Flow::Next };
                let n = u32::from_le_bytes(crate::dat::generator::fourcc(&name));
                match self.find_on_actor(id, n, actors, false, true) {
                    Ok(Some(r)) => {
                        let c = self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, Some(me));
                        if let Some(Some(ct)) = self.tasks.get_mut(c) {
                            ct.early = 10.0;
                        }
                        Flow::Wait
                    }
                    Ok(None) => Flow::Next,
                    Err(()) => Flow::Retry,
                }
            }
            0x15 | 0x22 => {
                t.a1 = self.doll(t.a1, actors);
                Flow::Next
            }
            0x16 | 0x23 => {
                t.a2 = self.doll(t.a2, actors);
                Flow::Next
            }
            0x17 => {
                t.a1 = self.undoll(t.a1);
                Flow::Next
            }
            0x18 => {
                t.a2 = self.undoll(t.a2);
                Flow::Next
            }
            0x31 => {
                t.loop_at = Some(t.ip);
                Flow::Next
            }
            // FUN_10185df0: the next record; its target becomes actor2 and the loop runs again.
            0x32 => {
                let next = t.action.as_ref().and_then(|a| {
                    let mut a = a.borrow_mut();
                    a.cur += 1;
                    a.records().get(a.cur).map(|r| r.0)
                });
                match next {
                    Some(target) => {
                        t.a2 = Who::Actor(target);
                        t.ip = t.loop_at.unwrap_or(0);
                        Flow::Jump
                    }
                    None => Flow::Next,
                }
            }
            0x30 => {
                if let Some(r) = scope(self, kind::SCHEDULER) {
                    let targets: Vec<u32> = t.action.as_ref().map(|a| a.borrow().records().iter().map(|r| r.0).collect()).unwrap_or_default();
                    for target in targets {
                        self.spawn_task(r, t.a1, Who::Actor(target), t.action.clone(), t.speed, None);
                    }
                }
                Flow::Next
            }
            0x19 => {
                self.pending.push(Pending { file: Some(0xaf0 + op.u32_at(8)), actor: None, name: MAIN, a1: t.a1, a2: t.a2, action: None, speed: t.speed, end_cast: None, age: 0.0 });
                Flow::Next
            }
            // One of the ops up to 0x3e at random (FUN_1006afb0).
            0x3d => {
                let end = (t.ip + 1..t.ops.len()).find(|&k| t.ops[k].code == 0x3e).unwrap_or(t.ops.len());
                let n = end - t.ip - 1;
                if n > 0 {
                    let k = t.ip + 1 + self.rng.un(n as u32) as usize;
                    let chosen = t.ops[k].clone();
                    self.run_op(me, t, &chosen, actors);
                }
                t.ip = end + 1;
                Flow::Jump
            }
            0x6b => {
                self.stack_op(t, op);
                Flow::Next
            }
            // Motions on actor1: name with the region '?' dropped, speed f32 @0x10, length u16 @6, count byte @0x1e,
            // blend u16 @0x18 (as dat::sched::plan reads them).
            0x05 if t.motions => {
                if let Some(id) = t.a1.id() {
                    let name = String::from_utf8_lossy(&op.name().to_le_bytes()).trim_end_matches(['\0', '?']).to_string();
                    let speed = f32::from_bits(op.u32_at(0x10));
                    let count = op.raw.get(0x1e).map_or(1, |b| b & 0x7f);
                    let m = crate::dat::sched::Motion { name, at: 0, speed: if speed == 0.0 { 1.0 } else { speed }, ticks: op.dur.max(0) as u16, count, blend: op.u16_at(0x18) };
                    self.motions.push((id, Some(m)));
                }
                Flow::Next
            }
            0x28 if t.motions => {
                if let Some(id) = t.a1.id() {
                    self.motions.push((id, None));
                }
                Flow::Next
            }
            0x64 | 0x65 => {
                let v = t.stack.pop().unwrap_or(0.0) != 0.0;
                let start = t.ip + 1;
                if t.ops.get(start).is_none_or(|o| o.code != 0x69) {
                    return Flow::Next;
                }
                let end = block_end(&t.ops, start);
                let has_else = t.ops.get(end + 1).is_some_and(|o| o.code == 0x67) && t.ops.get(end + 2).is_some_and(|o| o.code == 0x69);
                let else_end = if has_else { block_end(&t.ops, end + 2) } else { end };
                let branch = if v { Some((start + 1, end)) } else if has_else { Some((end + 3, else_end)) } else { None };
                if let Some((from, stop)) = branch {
                    let mut c = Task::new(t.res, t.ops.clone(), t.a1, t.a2, t.action.clone(), t.speed);
                    c.ip = from;
                    c.stop_at = Some(stop);
                    c.countdown = t.countdown;
                    c.loop_at = t.loop_at;
                    self.tasks.push(Some(c));
                }
                t.ip = else_end + 1;
                Flow::Jump
            }
            0x67 => {
                if t.ops.get(t.ip + 1).is_some_and(|o| o.code == 0x69) {
                    t.ip = block_end(&t.ops, t.ip + 1) + 1;
                    return Flow::Jump;
                }
                Flow::Next
            }
            _ => Flow::Next,
        }
    }

    /// Op 0x6b: the scheduler's stack machine (triggers.md 0.7).
    fn stack_op(&mut self, t: &mut Task, op: &SOp) {
        let sub = op.u16_at(8);
        let pop = |t: &mut Task| t.stack.pop().unwrap_or(0.0);
        match sub {
            0x1c => {
                let v = match op.u16_at(10) {
                    1 => op.u32_at(12) as i32 as f64,
                    2 => f32::from_bits(op.u32_at(12)) as f64,
                    3 => t.action.as_ref().map_or(0, |a| a.borrow().var(op.u32_at(12), t.a1.id())) as f64,
                    _ => 0.0,
                };
                t.stack.push(v);
            }
            5..=0x12 => {
                let (b, a) = (pop(t), pop(t));
                let r = match sub {
                    5 => a + b,
                    6 => a - b,
                    7 => a * b,
                    8 => {
                        if b != 0.0 {
                            a / b
                        } else {
                            0.0
                        }
                    }
                    9 => ((a as i64) << (b as i64 & 63)) as f64,
                    10 => ((a as i64) >> (b as i64 & 63)) as f64,
                    0xb => (a != b) as i32 as f64,
                    0xc => (a == b) as i32 as f64,
                    0xd => (a <= b) as i32 as f64,
                    0xe => (a >= b) as i32 as f64,
                    0xf => (a > b) as i32 as f64,
                    0x10 => (a < b) as i32 as f64,
                    0x11 => (a != 0.0 && b != 0.0) as i32 as f64,
                    _ => (a != 0.0 || b != 0.0) as i32 as f64,
                };
                t.stack.push(r);
            }
            0x14..=0x18 => {
                pop(t);
            }
            _ => {}
        }
    }

    /// Op 0x02 (FUN_10052710 via FUN_10057000 case 2): a clone of the generator on the task's actor pair.
    fn op_start_generator(&mut self, me: usize, t: &Task, op: &SOp, actors: &dyn Actors) {
        let Some(g) = self.reg.resolve(t.res.0, t.res.1, kind::GENERATOR, op.name()) else {
            self.reg.miss(t.res.0, t.res.1, op.name(), 5);
            return;
        };
        let Some(gd) = self.reg.libs[g.0].generator(g.1) else { return };
        let mode = gd.attach_mode();
        let attached = !matches!(mode, 0 | 0xe | 0xf | 0x14);
        let auto = gd.flags & 0x1000 != 0;
        let persistent = gd.flags & 0x400 != 0 || gd.template().is_some_and(|o| o.i16_at(0x22) == 0);
        if attached && [t.a1, t.a2].iter().any(|w| w.id().is_some_and(|id| actors.hidden(id))) {
            return;
        }
        if auto {
            self.stop_generator(g, t.a1, t.a2, true);
        }
        let Some(i) = self.start(g, Owner::Task(me)) else { return };
        if let Some(x) = self.insts[i].as_mut() {
            x.actors = [t.a1, t.a2];
            if !persistent && !auto {
                let d = op.dur as f32 * t.speed;
                x.life = if d == 0.0 { 1.0 } else { d };
            }
        }
        self.attach_now(i, actors);
    }

    /// FUN_10052a50: instances of `g` on (a1, a2): killed now, or soft-stopped (emission ends, elements finish).
    fn stop_generator(&mut self, g: Ref, a1: Who, a2: Who, soft: bool) {
        for i in 0..self.insts.len() {
            let hit = self.insts[i].as_ref().is_some_and(|x| x.src == g && x.actors[0] == a1 && (a2 == Who::None || x.actors[1] == a2));
            if !hit {
                continue;
            }
            if soft {
                if let Some(x) = self.insts[i].as_mut() {
                    x.stopping = true;
                    x.elapsed = x.life;
                }
            } else {
                self.kill(i);
            }
        }
    }

    /// FUN_10055f00 / FUN_10055f40: tasks running scheduler `r` (on one actor pair when given) end now.
    fn kill_tasks(&mut self, r: Ref, pair: Option<(Who, Who)>) {
        for t in self.tasks.iter_mut() {
            if t.as_ref().is_some_and(|t| t.res == r && pair.is_none_or(|(a, b)| t.a1 == a && t.a2 == b)) {
                *t = None;
            }
        }
    }

    /// FUN_100aa470: a doll where the actor stands now; its generator clones move to the doll (FUN_10061da0).
    fn doll(&mut self, w: Who, actors: &dyn Actors) -> Who {
        let Who::Actor(id) = w else { return w };
        let Some(m) = actors.matrix(id) else { return w };
        let d = Who::Doll(self.dolls.len());
        self.dolls.push(Doll { of: id, matrix: m });
        for x in self.insts.iter_mut().flatten() {
            for a in x.actors.iter_mut() {
                if *a == w {
                    *a = d;
                }
            }
        }
        d
    }

    fn undoll(&self, w: Who) -> Who {
        match w {
            Who::Doll(k) => self.dolls.get(k).map_or(w, |d| Who::Actor(d.of)),
            _ => w,
        }
    }

    fn who_matrix(&self, w: Who, actors: &dyn Actors) -> Option<(u32, Mat4)> {
        match w {
            Who::Actor(id) => Some((id, actors.matrix(id)?)),
            Who::Doll(d) => self.dolls.get(d).map(|d| (d.of, d.matrix)),
            Who::None => None,
        }
    }

    /// An actor slot's placement and attach point `k` in the world (FUN_1003abe0): 0x30 the ground below it *(inf:
    /// its position)*, 0x31 / 0x32 the body point 0xd..0x14 nearest the other actor, 0x33 the one nearest the eye;
    /// later special indices use point 0 *(inf)*.
    fn who_point(&self, w: Who, k: u16, other: Who, eye: Option<Vec3>, actors: &dyn Actors) -> Option<(Mat4, Vec3)> {
        let (id, m) = self.who_matrix(w, actors)?;
        let world = |k: u16| actors.point(id, k).map(|p| m.transform_point3(p));
        let nearest = |to: Vec3| (0xd..=0x14u16).filter_map(world).min_by(|a, b| a.distance_squared(to).total_cmp(&b.distance_squared(to)));
        let p = match k {
            0x30 => Some(m.w_axis.truncate()),
            0x31 | 0x32 => self.who_matrix(other, actors).and_then(|(_, o)| nearest(o.w_axis.truncate())),
            0x33 => eye.and_then(nearest),
            0x34.. => world(0),
            _ => world(k),
        };
        Some((m, p.unwrap_or(m.w_axis.truncate())))
    }

    fn who_scale(&self, w: Who, actors: &dyn Actors) -> f32 {
        match w {
            Who::Actor(id) => actors.scale(id),
            Who::Doll(d) => self.dolls.get(d).map_or(1.0, |d| actors.scale(d.of)),
            Who::None => 1.0,
        }
    }

    /// FUN_10052eb0 / FUN_1003afd0 for the instance's actors: its matrix (+0xa4), two-point distance (+0x90) and
    /// the actor-size scale vectors (+0x78 / +0x84).
    pub(super) fn attach_matrix(&self, i: usize, f: Option<&Frame>, actors: &dyn Actors) -> Option<(Option<Mat4>, Vec3, Vec3, f32)> {
        let x = self.insts[i].as_ref()?;
        let g = self.reg.libs[x.src.0].generator(x.src.1)?;
        let w = g.attach;
        let pc = ((((w >> 18) & 1) << 6) | ((w >> 4) & 0x3f)) as u16;
        let pt = ((w >> 10) & 0x3f) as u16;
        let [a1, a2] = x.actors;
        let rot = |m: Mat4| Mat4::from_cols(m.x_axis, m.y_axis, m.z_axis, glam::Vec4::W);
        let at = |m: Mat4, p: Vec3| {
            let mut r = rot(m);
            r.w_axis = p.extend(1.0);
            r
        };
        // FUN_1003ab40: local x from p1 toward p2 (yaw then pitch), at p1.
        let two = |p1: Vec3, p2: Vec3| {
            let d = p2 - p1;
            let yaw = -d.z.atan2(d.x);
            let pitch = d.y.atan2(Vec3::new(d.x, 0.0, d.z).length());
            let mut m = Mat4::from_rotation_y(yaw) * Mat4::from_rotation_z(pitch);
            m.w_axis = p1.extend(1.0);
            (m, d.length())
        };
        let cam = |p: Vec3| f.map(|f| {
            let [_, _, _, c10] = crate::render::effects::view_set(f.view);
            let mut m = rot(c10);
            m.w_axis = p.extend(1.0);
            m
        });
        let eye = f.map(|f| f.eye);
        let p1 = |k: u16| self.who_point(a1, k, a2, eye, actors);
        let p2 = |k: u16| self.who_point(a2, k, a1, eye, actors);
        let mut dist = 0.0;
        let m = match x.mode {
            1 | 9 | 0xb => p1(pc).map(|(m, p)| at(m, p)),
            2 | 0xa | 0xc => p2(pt).map(|(m, p)| at(m, p)),
            3 | 0xd | 0x16 | 0x1a => match (p1(pc), p2(pt)) {
                (Some((_, p1)), Some((_, p2))) => {
                    let (m, d) = two(p1, p2);
                    dist = d;
                    Some(m)
                }
                _ => None,
            },
            6 | 0x12 | 0x19 | 0x1b => match (p2(pt), p1(pc)) {
                (Some((_, p1)), Some((_, p2))) => {
                    let (m, d) = two(p1, p2);
                    dist = d;
                    Some(m)
                }
                _ => None,
            },
            4 | 0x17 => match (p2(pt), p1(0)) {
                (Some((_, p)), Some((m, _))) => Some(at(m, p)),
                _ => None,
            },
            5 | 0x18 => match (p1(pc), p2(0)) {
                (Some((_, p)), Some((m, _))) => Some(at(m, p)),
                _ => None,
            },
            7 => p1(pc).and_then(|(_, p)| cam(p)),
            8 => p2(pt).and_then(|(_, p)| cam(p)),
            0x10 | 0x11 => {
                let (a, o) = if x.mode == 0x10 { (a1, a2) } else { (a2, a1) };
                match (self.who_point(a, pc, o, eye, actors), self.who_point(a, pt, o, eye, actors)) {
                    (Some((_, p1)), Some((_, p2))) => {
                        let (m, d) = two(p1, p2);
                        dist = d;
                        Some(m)
                    }
                    _ => None,
                }
            }
            0x13 => p2(2).map(|(_, p)| Mat4::from_translation(p + Vec3::new(0.0, 0.18, 0.0))),
            _ => return None,
        };
        // Scale A from bits 20..23 (1-4 caster, 5-8 target), scale B from byte 3 low nibble.
        let size = |src: u32, blend: f32| -> Vec3 {
            let who = match src & 0xf {
                1..=4 => a1,
                5..=8 => a2,
                _ => return Vec3::ONE,
            };
            let k = if blend == 0.0 { 1.0 } else { blend };
            let s = (self.who_scale(who, actors) - 1.0) * k + 1.0;
            match (src & 0xf).wrapping_sub(1) & 3 {
                0 => Vec3::splat(s),
                1 => Vec3::new(s, 1.0, s),
                2 => Vec3::new(1.0, s, 1.0),
                _ => Vec3::new(s, s, s),
            }
        };
        let mut a = size(w >> 20, g.blend[0]);
        let b = size(w >> 24, g.blend[1]);
        if dist != 0.0 {
            a.x = dist;
        }
        Some((m, a, b, dist))
    }

    /// At start (FUN_10052220 -> FUN_10052eb0).
    pub(super) fn attach_now(&mut self, i: usize, actors: &dyn Actors) {
        if let Some((m, a, b, d)) = self.attach_matrix(i, None, actors)
            && let Some(x) = self.insts[i].as_mut()
        {
            if m.is_some() {
                x.matrix = m;
            }
            x.scale_a = a;
            x.scale_b = b;
            x.dist = d;
        }
    }

    /// Generator op 0x11 (FUN_10052cd0): the attach matrix again, lerped by k = ((op[1] >> 2) & 0xff) / 255 —
    /// bit 0 the translation, bit 1 the rotation rows.
    pub(super) fn follow(&mut self, i: usize, word: u32, f: &Frame, actors: &dyn Actors) {
        let Some((Some(m), a, b, d)) = self.attach_matrix(i, Some(f), actors) else { return };
        let Some(x) = self.insts[i].as_mut() else { return };
        let k = ((word >> 2) & 0xff) as f32 / 255.0;
        let old = x.matrix.unwrap_or(m);
        let mut out = old;
        if word & 1 != 0 {
            out.w_axis = old.w_axis.lerp(m.w_axis, k);
        }
        if word & 2 != 0 {
            out.x_axis = old.x_axis.lerp(m.x_axis, k);
            out.y_axis = old.y_axis.lerp(m.y_axis, k);
            out.z_axis = old.z_axis.lerp(m.z_axis, k);
        }
        x.matrix = Some(out);
        x.scale_a = a;
        x.scale_b = b;
        x.dist = d;
    }

    /// FUN_10083970: every looping (0x1000) generator of an actor's model files with an actor-attached mode (1, 2,
    /// 4, 5, 7-0xc, 0x10-0x13) runs cloned onto the actor; they go when the actor or its model does.
    pub(super) fn model_generators(&mut self, actors: &dyn Actors) {
        let present = actors.models();
        let gone: Vec<u32> = self.models.keys().filter(|id| !present.iter().any(|(p, files)| p == *id && self.models.get(p) == Some(files))).copied().collect();
        for id in gone {
            self.models.remove(&id);
            for i in 0..self.insts.len() {
                if self.insts[i].as_ref().is_some_and(|x| x.owner == Owner::Actor(id)) {
                    self.kill(i);
                }
            }
        }
        for (id, files) in present {
            if self.models.contains_key(&id) || actors.hidden(id) {
                continue;
            }
            let libs: Vec<Option<usize>> = files.iter().map(|&f| self.lib_for(f)).collect();
            if libs.iter().any(|l| l.is_none()) {
                continue;
            }
            let mut gens = Vec::new();
            for l in libs.into_iter().flatten() {
                for g in &self.reg.libs[l].gens {
                    if g.flags & 0x1000 != 0 && matches!(g.attach_mode(), 1 | 2 | 4 | 5 | 7..=0xc | 0x10..=0x13) {
                        gens.push((l, g.node));
                    }
                }
            }
            for g in gens.into_iter().take(128) {
                if let Some(i) = self.start(g, Owner::Actor(id)) {
                    if let Some(x) = self.insts[i].as_mut() {
                        x.actors = [Who::Actor(id), Who::None];
                    }
                    self.attach_now(i, actors);
                }
            }
            self.models.insert(id, files);
        }
    }

    pub fn task_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::action::{ActionResult, ActionTarget};

    struct Two;
    const HUME: u32 = 0x0001_0001;
    const ORC: u32 = 0x0100_0001;

    impl Actors for Two {
        fn matrix(&self, id: u32) -> Option<Mat4> {
            Some(Mat4::from_translation(if id == HUME { Vec3::ZERO } else { Vec3::new(2.0, 0.0, 0.0) }))
        }
        fn point(&self, _: u32, _: u16) -> Option<Vec3> {
            Some(Vec3::new(0.0, -1.0, 0.0))
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        // Hume M: fists 8392, battle file 9672, skeleton 7072; orc: model file 1300.
        fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
            if id == HUME { (vec![8392], vec![9672, 7072]) } else { (Vec::new(), vec![1300]) }
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
    }

    /// triggers.md 7.1: atk0 -> ati0 -> dada -> dam0 -> damg (orc) -> chit (Hume) -> ef h (fists) -> hit1 (DAT 0):
    /// generators g010..g013 start after the swing's hit.
    #[test]
    fn fist_hit_sparks() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let hit = ActionResult { kind: 1, scale: 1, value: 7, message: 1, ..Default::default() };
        let a = Action { actor: HUME, category: 1, arg: u32::from_le_bytes(*b"atk0"), info: 0, targets: vec![ActionTarget { id: ORC, results: vec![hit] }] };
        fx.on_action(&a, Some("ati0".into()));
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE };
        // Wait for the actors' files first, as a loaded model would have them.
        fx.preload(&[8392, 9672, 7072, 1300]);
        while [8392, 9672, 7072, 1300].iter().any(|f| fx.reg.lib_of(*f).is_none()) {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut started: Vec<(u32, String)> = Vec::new();
        for tick in 0..240 {
            fx.tick(&f, &Two);
            for x in fx.insts.iter().flatten() {
                let n = String::from_utf8_lossy(&x.name).into_owned();
                if !started.iter().any(|s| s.1 == n) {
                    started.push((tick, n));
                }
            }
        }
        // dada at 40 in Hume ati0; damg and its spark 5 ticks later (triggers.md 2.2).
        for g in ["g010", "g011", "g012", "g013"] {
            assert!(started.iter().any(|s| s.1 == g && s.0 == 45), "{g}: {started:?}");
        }
        // The orc hits back: orc ati0 dada at 36, then chit on the orc plays DAT 0 hit2 at the Hume (a doll).
        let a = Action { actor: ORC, category: 1, arg: u32::from_le_bytes(*b"atk0"), info: 0, targets: vec![ActionTarget { id: HUME, results: vec![ActionResult { kind: 1, scale: 1, value: 3, message: 1, ..Default::default() }] }] };
        fx.on_action(&a, Some("ati0".into()));
        let mut started: Vec<(u32, String)> = Vec::new();
        for tick in 0..240 {
            fx.tick(&f, &Two);
            for x in fx.insts.iter().flatten().filter(|x| x.actors[0] == Who::Actor(ORC)) {
                let n = String::from_utf8_lossy(&x.name).into_owned();
                if !started.iter().any(|s| s.1 == n) {
                    started.push((tick, n));
                }
            }
        }
        for g in ["g020", "g021", "g022", "g023"] {
            assert!(started.iter().any(|s| s.1 == g), "{g}: {started:?}");
        }
    }

    /// triggers.md 7.3: Cure. The cast start ("cawh" from the skeleton file) runs ner2's circle (gn2s, gn20, gn21);
    /// the finish loads file 2801, ends the circle (stwh) and plays main: the caster's sparkle (shwh -> eis4) and
    /// tgt0 on the target.
    #[test]
    fn cure_cast() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        fx.preload(&[8392, 9672, 7072, 1300, 2801]);
        while [8392, 9672, 7072, 1300, 2801].iter().any(|f| fx.reg.lib_of(*f).is_none()) {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE };
        let names = |fx: &Effects| -> Vec<String> { fx.insts.iter().flatten().filter(|x| !x.stopping).map(|x| String::from_utf8_lossy(&x.name).into_owned()).collect() };
        let start = Action { actor: HUME, category: 8, arg: u32::from_le_bytes(*b"cawh"), info: 0, targets: vec![ActionTarget { id: HUME, results: vec![ActionResult::default()] }] };
        fx.on_action(&start, None);
        let mut casting: Vec<String> = Vec::new();
        for _ in 0..60 {
            fx.tick(&f, &Two);
            casting.extend(names(&fx).into_iter().filter(|n| !casting.contains(n)).collect::<Vec<_>>());
        }
        for g in ["gn2s", "gn20", "gn21"] {
            assert!(casting.iter().any(|n| n == g), "{g}: {casting:?}");
        }
        // cawh's op 0x05 plays the cast motion mw0? on the caster.
        let moved: Vec<String> = fx.motions.drain(..).filter_map(|(id, m)| (id == HUME).then_some(m).flatten().map(|m| m.name)).collect();
        assert!(moved.iter().any(|n| n == "mw0"), "{moved:?}");
        let finish = Action { actor: HUME, category: 4, arg: 1, info: 0, targets: vec![ActionTarget { id: ORC, results: vec![ActionResult { animation: 1, message: 7, value: 30, ..Default::default() }] }] };
        fx.on_action(&finish, None);
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..200 {
            fx.tick(&f, &Two);
            for n in names(&fx) {
                if !seen.contains(&n) {
                    seen.push(n);
                }
            }
        }
        for g in ["ge4s", "ge40", "kr11", "prn1", "kr10"] {
            assert!(seen.iter().any(|n| n == g), "{g}: {seen:?}");
        }
        // stwh deleted gn2s and stopped gn20 / gn21.
        assert!(!names(&fx).iter().any(|n| n == "gn2s" || n == "gn20" || n == "gn21"), "{:?}", names(&fx));
    }
}
