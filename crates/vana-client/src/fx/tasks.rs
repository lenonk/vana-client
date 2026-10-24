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
    /// A skill category and animation whose file depends on the caster (FUN_100d0570), not chosen yet.
    pub skill: Option<(u8, u16)>,
    /// Wait until the actor lists this motion file (the client adds it).
    pub wait_motion: Option<(u32, u32)>,
    /// A damage number for the task (Task::num).
    pub num: Option<(i32, u32)>,
    /// Its motion ops (0x05 / 0x28) and holds are the client's (a death: combat::die plays ded? / cor?).
    pub quiet: bool,
    /// Looked up as vtable +0x298 (FUN_100cce00): the motion and model files, then DAT 0; no part files.
    pub own: bool,
}

/// A doll (CXiDollActor, FUN_100aa470): where an actor stood when a scheduler froze it.
pub struct Doll {
    pub of: u32,
    pub matrix: Mat4,
}

const MAIN: u32 = u32::from_le_bytes(*b"main");

/// Where an actor's equipment part files go in a scheduler lookup.
#[derive(Clone, Copy)]
enum Parts {
    First,
    Last,
    Skip,
}

/// DAT_1035bc3c: the skillchain burst's animation by proc kind (file 0xaf0 + it; LSB ActionProcSkillChain order).
const SKILLCHAIN: [u16; 18] = [0, 479, 480, 475, 476, 477, 478, 467, 469, 474, 471, 472, 473, 470, 468, 1013, 1014, 0];

/// FUN_10055cc0: several records outside categories 14 / 15 run at 0.7.
fn action_speed(a: &Action) -> f32 {
    if a.records().len() > 1 && !matches!(a.category, 0xe | 0xf) { 0.7 } else { 1.0 }
}

/// FUN_100d0570's files for a skill on a PC skeleton (tables 0x10353be4..0x10353cb8, indexed by race code 0..8):
/// the motion and effect file, and the second file when body info byte 9 > 0 (0x...c30 table when it is 1).
fn skill_files(category: u8, anim: u16, race: u8, skirt: i8) -> Option<(u32, Option<u32>)> {
    const WS: [u32; 9] = [33227, 33227, 33995, 34763, 35531, 36299, 36299, 37067, 37835];
    const WS2: [[u32; 2]; 9] = [[33483, 33739], [33483, 33739], [34251, 34507], [35019, 35275], [35787, 36043], [36555, 36811], [36555, 36811], [37323, 37579], [38091, 38347]];
    const WS_HI: [u32; 9] = [61451, 61451, 61499, 61547, 61595, 61643, 61643, 61691, 61739];
    const DANCE: [u32; 9] = [58041, 58041, 58425, 58809, 59193, 59577, 59577, 59961, 60345];
    const RUNE: [u32; 9] = [88035, 88035, 89571, 91107, 92643, 94179, 94179, 95715, 97251];
    let r = race as usize;
    if r >= 9 {
        return None;
    }
    let a = anim as u32;
    let (file, second) = match category {
        0xe => (DANCE[r] + a, None),
        0xf => (RUNE[r] + a, None),
        _ if a < 0x100 => (WS[r] + a, Some(WS2[r][(skirt != 1) as usize] + a)),
        // The 0x10353bf8 skirt table is not read *(inf: its values)*.
        _ => (WS_HI[r] + a - 0x100, None),
    };
    Some((file, second.filter(|_| skirt > 0)))
}

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

/// The action file a model's vtable +0x2c4..+0x2ec loads for `id` (event ops 0x73 / 0xad / 0xc4, FUN_100a9700..
/// FUN_100a98b0): 0 item, 1 magic, 2 ability, then fixed bases.
pub fn event_action_file(kind: u8, id: u32) -> Option<u32> {
    Some(id + match kind {
        0 if id < 100 => 0x1330,
        0 => 0xdca5,
        1 => 0xaf0,
        2 => 0x113c,
        3 => 0x1394,
        4 => 0xc7ef,
        5 => 0xdd6d,
        6 => 0x1071b,
        7 => 0x11323,
        8 => 0x11423,
        9 => 0x19031,
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
        // No index yet (a model before the first zone): not marked, so a later call loads it.
        if let Some(index) = self.index.clone()
            && self.loading.insert(file)
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
            match lib {
                Some(l) if self.reg.lib_of(file).is_none() => {
                    self.reg.add(l);
                }
                Some(_) => {}
                None => {
                    self.failed.insert(file);
                }
            }
        }
    }

    /// A 0x028 action (FUN_1009caa0). `swing` is the swing scheduler combat picked for melee (op 0x24).
    pub fn on_action(&mut self, a: &Action, swing: Option<String>) {
        let caster = a.actor;
        let first = a.targets.first().map_or(Who::None, |t| Who::Actor(t.id));
        let records = a.records().len();
        let speed = action_speed(a);
        log::debug!("effects: action category {} arg {:#x} from {caster:#x}, swing {swing:?}", a.category, a.arg);
        // Melee and ranged main lines are client/combat.rs's (at the hit); their proc / react lines come from op 0x71.
        let client_swing = swing.is_some();
        let ctx = Rc::new(RefCell::new(ActionCtx { action: a.clone(), cur: 0, swing, printed: vec![matches!(a.category, 1 | 2); records], last: 0, client_swing }));
        match a.category {
            1 | 2 => self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: false }),
            3..=6 | 0xb | 0xd..=0xf => {
                let anim = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.animation as u32);
                let Some(base) = effect_base(a.category, anim) else {
                    let ctx = Some(ctx);
                    self.release(&ctx);
                    self.end_action(&ctx);
                    return;
                };
                // Skeleton casters of weapon skills load per-race files (FUN_100d0570); those are not loaded yet
                // *(inf: falls back to the generic file)*.
                self.pending.push(Pending { file: Some(base + anim), actor: None, name: MAIN, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: Some(caster), age: 0.0, skill: matches!(a.category, 3 | 0xe | 0xf).then_some((a.category, anim as u16)), wait_motion: None, num: None, quiet: false, own: false });
            }
            7..=0xa | 0xc => {
                // Starts: the cmd_arg scheduler on the caster itself; "ca??" goes through FUN_10083d60 first.
                if a.arg & 0xffff == 0x6163 {
                    self.cast_state(caster, a.arg);
                }
                self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: true });
            }
            _ => {}
        }
    }

    /// FUN_10083d60(caster, arg): a stored cast 4cc runs as "st??" (ends the casting effect); +0xa0 = arg either way
    /// (asm 0x10083d83; the decompile's 0 is a stack misread), 0 clearing it.
    fn cast_state(&mut self, caster: u32, arg: u32) {
        if let Some(prev) = self.casting.remove(&caster) {
            let st = (prev & 0xffff_7473) | 0x7473;
            self.pending.push(Pending { file: None, actor: Some(caster), name: st, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: true });
        }
        if arg != 0 {
            self.casting.insert(caster, arg);
        }
    }

    /// Event opcodes 0x45 and family: the scheduler `name` of DAT `file` as a task on (a1, a2), the same task the
    /// event's camera runner follows (its camera / fade ops are the runner's; the rest run here).
    pub fn on_event_scheduler(&mut self, file: u32, name: u32, a1: Who, a2: Who) {
        log::debug!("effects: event scheduler {} of DAT {file}", String::from_utf8_lossy(&name.to_le_bytes()));
        self.pending.push(Pending { file: Some(file), actor: None, name, a1, a2, action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: false });
    }

    /// Event opcodes 0x52 and family: that scheduler's tasks on (a1, a2) end.
    pub fn stop_event_scheduler(&mut self, file: u32, name: u32, a1: Who, a2: Who) {
        self.pending.retain(|p| !(p.file == Some(file) && p.name == name && p.a1 == a1 && p.a2 == a2));
        if let Some(l) = self.reg.lib_of(file)
            && let Some(n) = self.reg.libs[l].find_global(kind::SCHEDULER, name)
        {
            self.kill_tasks((l, n), Some((a1, a2)));
        }
    }

    /// A scheduler of `file` on (a1, a2) still waiting for files or running (event op 0x54).
    pub fn event_scheduler_running(&self, file: u32, name: u32, a1: Who, a2: Who) -> bool {
        if self.pending.iter().any(|p| p.file == Some(file) && p.name == name && p.a1 == a1 && p.a2 == a2) {
            return true;
        }
        let Some(r) = self.reg.lib_of(file).and_then(|l| self.reg.libs[l].find_global(kind::SCHEDULER, name).map(|n| (l, n))) else { return false };
        self.tasks.iter().flatten().any(|t| t.res == r && t.a1 == a1 && t.a2 == a2)
    }

    /// The zone library's file id (its schedulers are the zone's actions, event ops 0x2d / 0x51 / 0x54).
    pub fn zone_file(&self) -> Option<u32> {
        self.zone_lib.map(|l| self.reg.libs[l].file_id)
    }

    /// FUN_1009e090's on-screen number: DAT 27's `damv` / `curv` / `misv` (kind 1 / 2 / 3) on (a1, a2) with the value
    /// and the message colour 0xRRGGBB.
    pub fn on_damage_number(&mut self, kind: u8, a1: u32, a2: u32, value: i32, rgb: u32) {
        let name = match kind {
            1 => *b"damv",
            2 => *b"curv",
            3 => *b"misv",
            _ => return,
        };
        self.pending.push(Pending { file: Some(27), actor: None, name: u32::from_le_bytes(name), a1: Who::Actor(a1), a2: Who::Actor(a2), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: Some((value, rgb)), quiet: false, own: false });
    }

    /// A status scheduler on the actor with its motions left to the client (FUN_1008c6c0's `dead`).
    pub fn on_scheduler_quiet(&mut self, actor: u32, name: u32) {
        self.pending.push(Pending { file: None, actor: Some(actor), name, a1: Who::Actor(actor), a2: Who::Actor(actor), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: true, own: false });
    }

    /// FUN_10083650: a scheduler `name` waiting or running on the actor (as actor1).
    pub fn actor_scheduler_running(&self, actor: u32, name: u32) -> bool {
        let who = Who::Actor(actor);
        self.pending.iter().any(|p| p.actor == Some(actor) && p.name == name)
            || self.tasks.iter().flatten().any(|t| t.a1 == who && u32::from_le_bytes(self.reg.libs[t.res.0].tree.nodes[t.res.1].name) == name)
    }

    /// Model vtable +0x29c: the actor's scheduler `name` stops (waiting or running).
    pub fn stop_actor_scheduler(&mut self, actor: u32, name: u32) {
        let who = Who::Actor(actor);
        self.pending.retain(|p| !(p.actor == Some(actor) && p.name == name));
        for i in 0..self.tasks.len() {
            let hit = self.tasks[i].as_ref().is_some_and(|t| t.a1 == who && u32::from_le_bytes(self.reg.libs[t.res.0].tree.nodes[t.res.1].name) == name);
            if hit {
                self.tasks[i] = None;
            }
        }
    }

    /// 0x038: a scheduler by 4cc on an actor (FUN_100cce00 with no action).
    pub fn on_scheduler(&mut self, actor: u32, target: u32, name: u32) {
        self.pending.push(Pending { file: None, actor: Some(actor), name, a1: Who::Actor(actor), a2: Who::Actor(target), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: true });
    }

    /// Starts waiting for files, when they are in.
    pub(super) fn run_pending(&mut self, f: &Frame, actors: &dyn Actors) {
        let list = std::mem::take(&mut self.pending);
        for mut p in list {
            p.age += f.dt;
            // FUN_100d0570: PC skeletons take weapon skills from per-race files (and a skirt variant), loaded as
            // motion files on the actor; others keep the generic file.
            if let (Some((cat, anim)), Some(caster)) = (p.skill.take(), p.a1.id())
                && let Some((race, skirt)) = actors.pc(caster)
                && let Some((file, second)) = skill_files(cat, anim, race, skirt)
            {
                p.file = Some(file);
                p.wait_motion = Some((caster, file));
                self.motion_files.push((caster, std::iter::once(file).chain(second).collect()));
            }
            if let Some((actor, file)) = p.wait_motion
                && !actors.files(actor).1.contains(&file)
            {
                if p.age < 600.0 {
                    self.pending.push(p);
                }
                continue;
            }
            let found = match (p.file, p.actor) {
                (Some(file), _) => match self.lib_for(file) {
                    Some(l) => Ok(self.reg.libs[l].find_global(kind::SCHEDULER, p.name).map(|n| (l, n))),
                    None if self.failed.contains(&file) => Ok(None),
                    None => Err(()),
                },
                (None, Some(actor)) => self.find_on_actor(actor, p.name, actors, if p.own { Parts::Skip } else { Parts::First }, true),
                _ => Ok(None),
            };
            match found {
                Ok(res) => {
                    if let Some(c) = p.end_cast {
                        self.cast_state(c, 0);
                    }
                    if p.name == MAIN {
                        self.release(&p.action);
                    }
                    match res {
                        Some(r) => {
                            log::debug!("effects: scheduler {} started", String::from_utf8_lossy(&p.name.to_le_bytes()));
                            let ti = self.spawn_task(r, p.a1, p.a2, p.action, p.speed, None);
                            if let Some(Some(t)) = self.tasks.get_mut(ti) {
                                t.num = p.num;
                                t.motions &= !p.quiet;
                            }
                        }
                        None => {
                            log::debug!("effects: scheduler {} not found", String::from_utf8_lossy(&p.name.to_le_bytes()));
                            self.end_action(&p.action);
                        }
                    }
                }
                // Give up on a file that does not come within 10 s (asm 0x100a912e: everything prints).
                Err(()) if p.age < 600.0 => self.pending.push(p),
                Err(()) => {
                    if p.name == MAIN {
                        self.release(&p.action);
                    }
                    self.end_action(&p.action);
                }
            }
        }
    }

    /// FUN_100a9610: the caster's and the targets' load holds end (the skill action's "main" started or failed).
    fn release(&mut self, action: &Option<Rc<RefCell<ActionCtx>>>) {
        let Some(a) = action else { return };
        let a = a.borrow();
        if matches!(a.action.category, 1 | 2) {
            return;
        }
        self.releases.push(a.action.actor);
        self.releases.extend(a.records().iter().map(|r| r.0));
    }

    /// FUN_1018c620: an action ending prints every record not printed yet.
    fn end_action(&mut self, action: &Option<Rc<RefCell<ActionCtx>>>) {
        let Some(a) = action else { return };
        let mut a = a.borrow_mut();
        if matches!(a.action.category, 1 | 2) {
            return;
        }
        for k in 0..a.printed.len() {
            if !a.printed[k] {
                a.printed[k] = true;
                self.action_prints.push((a.action.clone(), k, 0));
            }
        }
    }

    /// FUN_100cc970 (equipment part files, `parts`) and FUN_100cc440 (motion files newest first, then the model's),
    /// then DAT 0 (`global`). Err while one of those files is still loading; a failed file is passed by.
    fn find_on_actor(&mut self, actor: u32, name: u32, actors: &dyn Actors, parts: Parts, global: bool) -> Result<Option<Ref>, ()> {
        let (part_files, motion_files) = actors.files(actor);
        let order: Vec<u32> = match parts {
            Parts::First => part_files.into_iter().chain(motion_files).collect(),
            Parts::Last => motion_files.into_iter().chain(part_files).collect(),
            Parts::Skip => motion_files,
        };
        let mut waiting = false;
        for file in order {
            match self.lib_for(file) {
                Some(l) => {
                    if let Some(n) = self.reg.libs[l].find_global(kind::SCHEDULER, name) {
                        return Ok(Some((l, n)));
                    }
                }
                None if self.failed.contains(&file) => {}
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
        log::debug!("effects: task {} (DAT {}) on {a1:?} -> {a2:?}", String::from_utf8_lossy(&self.reg.libs[r.0].tree.nodes[r.1].name), self.reg.libs[r.0].file_id);
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
        let mut o = parse(self.reg.libs[r.0].data(r.1));
        // FUN_1006b2b0: at start-up the global `proc` (op 01 on disk) gets 0x80 written over its first op.
        if Some(r.0) == self.reg.global
            && self.reg.libs[r.0].tree.nodes[r.1].name == *b"proc"
            && let Some(first) = o.first_mut()
        {
            first.code = 0x80;
        }
        let o = Rc::new(o);
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
                if t.action.as_ref().is_some_and(|a| Rc::strong_count(a) == 1) {
                    self.end_action(&t.action);
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
        log::trace!("effects: task {:?} op {:#04x} {}", t.res, op.code, String::from_utf8_lossy(&op.name().to_le_bytes()));
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
                    Some(id) => match self.find_on_actor(id, op.name(), actors, Parts::First, false) {
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
                match self.find_on_actor(id, op.name(), actors, Parts::Last, false) {
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
            // Ops 0x76 / 0x77 (asm 0x1005adb5): `lc` / `ls` + actor1's ranged class (+0x886) in two digits, a plain
            // child on actor1's own files; mode @8 1: above 2 sings (0), 2: 10 unless 11. Nothing while moving.
            0x76 | 0x77 => {
                let (Some(a1), Some(a2)) = (self.doll_of(t.a1).id(), self.doll_of(t.a2).id()) else { return Flow::Next };
                if actors.moving(a1) {
                    return Flow::Next;
                }
                let Some(k) = actors.ranged_class(a1).map(i32::from) else { return Flow::Next };
                let k = match op.u32_at(8) {
                    1 if k > 2 => 0,
                    2 if k != 11 => 10,
                    _ => k,
                };
                if k < 0 {
                    return Flow::Next;
                }
                let name = u32::from_le_bytes([b'l', if op.code == 0x77 { b's' } else { b'c' }, b'0' + (k / 10) as u8, b'0' + (k % 10) as u8]);
                match self.find_on_actor(a1, name, actors, Parts::Last, false) {
                    Ok(Some(r)) => {
                        self.spawn_task(r, Who::Actor(a1), Who::Actor(a2), t.action.clone(), t.speed, None);
                        Flow::Next
                    }
                    Ok(None) => Flow::Next,
                    Err(()) => Flow::Retry,
                }
            }
            // Swing pick (FUN_100d4600, combat chose it) and the counter swing: call-and-wait, parent back 10 sooner.
            // Op 0x5c: the counter swing, `cni0` standing, `cnf0` moving (ours moves forward only), and no wait.
            0x5c => {
                let Some(id) = t.a1.id() else { return Flow::Next };
                let name = if actors.moving(id) { *b"cnf0" } else { *b"cni0" };
                match self.find_on_actor(id, u32::from_le_bytes(name), actors, Parts::Last, true) {
                    Ok(Some(r)) => {
                        self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, None);
                        Flow::Next
                    }
                    Ok(None) => Flow::Next,
                    Err(()) => Flow::Retry,
                }
            }
            0x24 | 0x5d => {
                let Some(id) = t.a1.id() else { return Flow::Next };
                let mut name = t.action.as_ref().and_then(|a| a.borrow().swing.clone());
                // FUN_100d4600 picks here once the attacker's files are in (ours picks at the packet when it can).
                if name.is_none() && op.code == 0x24 && t.action.as_ref().is_some_and(|a| a.borrow().action.category == 1) {
                    let moving = actors.moving(id);
                    match actors.swings(id, moving) {
                        None => return Flow::Retry,
                        Some(pool) if pool.is_empty() => return Flow::Next,
                        Some(pool) => {
                            let pick = if moving { pool[0].clone() } else { pool[self.rng.un(pool.len() as u32) as usize].clone() };
                            log::debug!("effects: swing {pick} picked in op 0x24 for {id:#x}");
                            if let Some(a) = &t.action {
                                a.borrow_mut().swing = Some(pick.clone());
                            }
                            name = Some(pick);
                        }
                    }
                }
                let Some(name) = name else { return Flow::Next };
                let n = u32::from_le_bytes(crate::dat::generator::fourcc(&name));
                match self.find_on_actor(id, n, actors, Parts::Last, true) {
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
                t.a1 = self.doll(t.a1, actors, me);
                Flow::Next
            }
            0x16 | 0x23 => {
                t.a2 = self.doll(t.a2, actors, me);
                Flow::Next
            }
            0x17 => {
                t.a1 = self.undoll(t.a1, me);
                Flow::Next
            }
            0x18 => {
                t.a2 = self.undoll(t.a2, me);
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
            // FUN_100439c0 / FUN_10043bb0: a door part turns (0x0d) or moves (0x0c) to vec @8 over i16 @6 x speed,
            // part u32 @0x14; actor1 must be a door (the client checks).
            0x0c | 0x0d => {
                if let Some(actor) = t.a1.id() {
                    let to = [8, 0xc, 0x10].map(|o| f32::from_bits(op.u32_at(o)));
                    self.door_moves.push(super::DoorMove { actor, rotate: op.code == 0x0d, part: op.u32_at(0x14), to, ticks: op.dur as f32 * t.speed });
                }
                Flow::Next
            }
            // Sounds at a snapshot of the actor's position: 0x0a actor1, 0x0b actor2, 0x53 the action target nearest
            // the player *(inf: actor2)*; far @0x14, near @0x18, flag 0x100 = height as is.
            0x0a | 0x0b | 0x53 => {
                let mut who = if op.code == 0x0a { t.a1 } else { t.a2 };
                // Op 0x53 (all.c 67094): the action target nearest our own actor (actor1 with no player).
                if op.code == 0x53
                    && let Some(a) = &t.action
                {
                    let me = actors.player().and_then(|p| actors.matrix(p)).map(|m| m.w_axis.truncate());
                    let ids: Vec<u32> = a.borrow().records().iter().map(|r| r.0).collect();
                    who = match me {
                        Some(mp) => ids.into_iter().filter_map(|id| Some((id, (actors.matrix(id)?.w_axis.truncate() - mp).length()))).min_by(|x, y| x.1.total_cmp(&y.1)).map_or(t.a2, |(id, _)| Who::Actor(id)),
                        None => t.a1,
                    };
                }
                // `shit` / `skaz` take the actor's sub-animation as the last digit when that SeSep exists.
                let sub = self.doll_of(who).id().map_or(0, |id| actors.sub_anim(id) & 3);
                let r = match op.name().to_le_bytes() {
                    [b's', b'h', b'i', b't'] | [b's', b'k', b'a', b'z'] if sub != 0 => {
                        let mut n = op.name().to_le_bytes();
                        n[3] = b'0' + sub;
                        self.reg.resolve(t.res.0, t.res.1, 0x3d, u32::from_le_bytes(n)).or_else(|| scope(self, 0x3d))
                    }
                    _ => scope(self, 0x3d),
                };
                if let (Some(r), Some((_, m))) = (r, self.who_matrix(who, actors))
                    && let Some((se, loops)) = super::sound::sesep(self.reg.libs[r.0].data(r.1))
                {
                    log::debug!("effects: sound {se} op {:#x}", op.code);
                    let mut s = super::sound::Sound::new(se, loops);
                    s.pos = m.w_axis.truncate();
                    s.far = f32::from_bits(op.u32_at(0x14));
                    s.near = f32::from_bits(op.u32_at(0x18));
                    if op.u32_at(0x10) & 0x100 != 0 {
                        s.ymode = 1;
                    }
                    self.sounds.push(s);
                }
                Flow::Next
            }
            // FUN_10057000 case 0x60: the named SeSep played as the stream (FUN_100360e0, DAT_10450fe8), which the
            // event volume op leaves alone unless its mask has 8 (the opening narration's music).
            0x60 => {
                if let Some(r) = scope(self, 0x3d)
                    && let Some((se, loops)) = super::sound::sesep(self.reg.libs[r.0].data(r.1))
                {
                    log::debug!("effects: stream {se}");
                    let mut s = super::sound::Sound::new(se, loops);
                    s.stream = true;
                    self.sounds.push(s);
                }
                Flow::Next
            }
            0x19 => {
                self.pending.push(Pending { file: Some(0xaf0 + op.u32_at(8)), actor: None, name: MAIN, a1: t.a1, a2: t.a2, action: None, speed: t.speed, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: false });
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
                self.stack_op(t, op, actors);
                Flow::Next
            }
            // Entity +0x130 bit 1 (FUN_100832f0): set, the model file's 0x1000 generators don't start (vtable +0x124).
            0x9a => {
                if let Some(id) = t.a1.id() {
                    self.gens_off.push((id, op.u32_at(8) & 1 != 0));
                }
                Flow::Next
            }
            // Motions on actor1: name with the region '?' dropped, speed f32 @0x10, length u16 @6, count byte @0x1e,
            // blend u16 @0x18 (as dat::sched::plan reads them).
            0x05 if t.motions => {
                if let Some(id) = t.a1.id() {
                    let name = String::from_utf8_lossy(&op.name().to_le_bytes()).trim_end_matches(['\0', '?']).to_string();
                    let speed = f32::from_bits(op.u32_at(0x10));
                    let count = op.raw.get(0x1e).map_or(1, |b| b & 0x7f);
                    let m = crate::dat::sched::Motion { name, at: 0, speed: if speed == 0.0 { 1.0 } else { speed }, ticks: (op.dur.max(0) as f32 * t.speed).round() as u16, count, blend: op.u16_at(0x18) };
                    self.motions.push((id, Some(m)));
                }
                Flow::Next
            }
            // FUN_1005f6c0: the real actor's model colour toward u32 @8 over dur * speed ticks (alpha = high byte).
            0x29 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.fades.push((id, (op.u32_at(8) >> 24) as u8, op.dur.max(0) as f32 * t.speed));
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
            // FUN_100a4140 for op dur x speed: 0x20 holds actor2 (its status changes wait), 0x59 actor1, no reactions.
            0x20 | 0x59 if t.motions => {
                let (who, noreact) = if op.code == 0x20 { (t.a2, false) } else { (t.a1, true) };
                if let Some(id) = self.doll_of(who).id() {
                    self.holds.push((id, op.dur.max(0) as f32 * t.speed, noreact));
                }
                Flow::Next
            }
            // FUN_10057fb0 cases 0x25 / 0x21: the hit reaction of actor2 / actor1 at this frame (melee's damg too).
            0x25 | 0x21 => {
                let (reactor, other) = if op.code == 0x25 { (t.a2, t.a1) } else { (t.a1, t.a2) };
                // FUN_1005e350: the current record's hit distortion (scale & 3); no action 1.
                let level = t.action.as_ref().map_or(1, |a| {
                    let a = a.borrow();
                    let recs = a.records();
                    recs.get(a.cur.min(recs.len().saturating_sub(1))).map_or(1, |r| r.1.scale & 3)
                });
                if let (Some(reactor), Some(other)) = (self.doll_of(reactor).id(), self.doll_of(other).id()) {
                    let f = |o: usize| f32::from_bits(op.u32_at(o));
                    self.reacts.push(super::React { reactor, other, amount: f(8), rate: f(0xc), count: op.u32_at(0x10), blend_in: f(0x14), fade: f(0x18), side: op.u32_at(0x1c), level });
                }
                Flow::Next
            }
            // Ops 0x5e / 0xbf (+ delay u16 @6): actor2 pushed from actor1; mode u16 @8 0 = the record's level (scale >> 2,
            // no action 1, 0 = nothing) from DAT_1035bc60, else speed f32 @0xc a second, decay @0x10, ticks @0x14; pull
            // u16 @0xa toward. Motion bf0? with the pusher in front of the reactor's facing, bb0? behind (swapped for a pull).
            0x5e | 0xbf => {
                let (Some(reactor), Some(pusher)) = (self.doll_of(t.a2).id(), self.doll_of(t.a1).id()) else { return Flow::Next };
                let (Some(rm), Some(pm)) = (actors.matrix(reactor), actors.matrix(pusher)) else { return Flow::Next };
                let (rp, pp) = (rm.w_axis.truncate(), pm.w_axis.truncate());
                let front = rm.x_axis.truncate().dot(pp - rp) >= 0.0;
                let pull = op.u16_at(0xa) != 0;
                let f = |o: usize| f32::from_bits(op.u32_at(o));
                let (speed, decay, ticks) = if op.u16_at(8) == 0 {
                    let level = t.action.as_ref().map_or(1, |a| {
                        let a = a.borrow();
                        let recs = a.records();
                        recs.get(a.cur.min(recs.len().saturating_sub(1))).map_or(1, |r| r.1.scale >> 2)
                    });
                    if level == 0 {
                        return Flow::Next;
                    }
                    let row = self.knock_table[level.min(7) as usize];
                    (row[0], row[1], row[2])
                } else {
                    (f(0xc) / 60.0, f(0x10), f(0x14))
                };
                let k = super::Knock { reactor, from: pp, speed: if pull { -speed } else { speed }, decay, ticks, motion: if front != pull { "bf0" } else { "bb0" }.to_string(), count: (f(0x14) as i32).clamp(0, 255) as u8 };
                if op.code == 0x5e {
                    self.knocks.push(k);
                } else {
                    self.knocks_later.push((op.u16_at(6) as f32 * t.speed, k));
                }
                Flow::Next
            }
            // Op 0x78: the body hitting the floor: `0a` + actor1's footstep kind + '1' + shoe in the folder of its last
            // footstep (model +0x800), at a snapshot of it, near f32 @0xc, far @8; nothing before a first step.
            0x78 => {
                let Some(id) = self.doll_of(t.a1).id() else { return Flow::Next };
                let (Some([_, k, s]), Some(&rain), Some(l), Some(m)) = (actors.cib(id), self.foot_folder.get(&id), self.zone_lib, actors.matrix(id)) else { return Flow::Next };
                if k < 0 || s < 0 {
                    return Flow::Next;
                }
                let c = if k < 10 { b'0' + k as u8 } else { b'a' + (k - 10) as u8 };
                let name = [b'0', b'a', c, b'1' + s as u8];
                let lib = &self.reg.libs[l];
                let folder = crate::dat::generator::fourcc(if rain { "fser" } else { "fses" });
                let found = lib.tree.nodes.iter().position(|n| n.kind == kind::FOLDER && n.name == folder).and_then(|f| lib.find_in(f, 0x3d, name));
                if let Some(n) = found
                    && let Some((se, loops)) = super::sound::sesep(lib.data(n))
                {
                    log::debug!("effects: sound {se} op 0x78 ({})", String::from_utf8_lossy(&name));
                    let mut snd = super::sound::Sound::new(se, loops);
                    snd.pos = m.w_axis.truncate();
                    snd.far = f32::from_bits(op.u32_at(8));
                    snd.near = f32::from_bits(op.u32_at(0xc));
                    self.sounds.push(snd);
                }
                Flow::Next
            }
            // Ops 0x5a / 0x5b: actor1 / actor2 guards (@0xe 0) or parries.
            0x5a | 0x5b => {
                let who = if op.code == 0x5a { t.a1 } else { t.a2 };
                if let Some(reactor) = self.doll_of(who).id() {
                    let f = |o: usize| f32::from_bits(op.u32_at(o));
                    self.guards.push(super::Guard { reactor, parry: op.u16_at(0xe) != 0, rate: f(8), count: op.u16_at(0xc) as i16, blend_in: f(0x10), fade: f(0x14) });
                }
                Flow::Next
            }
            // FUN_1018c810: the first record not printed yet prints its main message.
            0x2b if t.motions => {
                if let Some(a) = &t.action {
                    let mut a = a.borrow_mut();
                    if let Some(k) = a.printed.iter().position(|p| !p) {
                        a.printed[k] = true;
                        a.last = k;
                        self.action_prints.push((a.action.clone(), k, 0));
                    }
                }
                Flow::Next
            }
            // FUN_1018c870: message n (1 proc, 2 react) of the record op 0x2b printed last (`mprc`, `wapr`, `sb0?`,
            // `dcnt`).
            0x71 => {
                if let Some(a) = &t.action {
                    let a = a.borrow();
                    self.action_prints.push((a.action.clone(), a.last, op.u32_at(8) as u8));
                }
                Flow::Next
            }
            // FUN_100a8e30: record 0's skillchain plays its burst file on (actor1, record 0's target), with a copy of
            // the action whose records are all printed (only the burst's `mprc` prints its proc message).
            0x80 if t.motions => {
                let Some(ctx) = &t.action else { return Flow::Next };
                let a = ctx.borrow().action.clone();
                let recs = a.records();
                let Some((target, r)) = recs.first().map(|(id, r)| (*id, *r)) else { return Flow::Next };
                let kind = r.proc.as_ref().map_or(0, |p| p.kind as usize);
                let anim = SKILLCHAIN.get(kind).copied().unwrap_or(0);
                let Some(caster) = self.doll_of(t.a1).id() else { return Flow::Next };
                if anim == 0 {
                    return Flow::Next;
                }
                log::debug!("effects: skillchain kind {kind}: burst DAT {} on {caster:#x} -> {target:#x}", 0xaf0 + anim as u32);
                let n = recs.len();
                self.loads.push(caster);
                self.loads.extend(recs.iter().map(|r| r.0));
                let speed = action_speed(&a);
                let clone = Rc::new(RefCell::new(ActionCtx { action: a, cur: 0, swing: None, printed: vec![true; n], last: 0, client_swing: false }));
                self.pending.push(Pending { file: Some(0xaf0 + anim as u32), actor: None, name: MAIN, a1: Who::Actor(caster), a2: Who::Actor(target), action: Some(clone), speed, end_cast: None, age: 0.0, skill: None, wait_motion: None, num: None, quiet: false, own: false });
                Flow::Next
            }
            _ => Flow::Next,
        }
    }

    /// Op 0x6b: the scheduler's stack machine (triggers.md 0.7).
    fn stack_op(&mut self, t: &mut Task, op: &SOp, actors: &dyn Actors) {
        let sub = op.u16_at(8);
        let pop = |t: &mut Task| t.stack.pop().unwrap_or(0.0);
        match sub {
            0x1c => {
                let v = match op.u16_at(10) {
                    1 => op.u32_at(12) as i32 as f64,
                    2 => f32::from_bits(op.u32_at(12)) as f64,
                    // 0x3a: actor1's sub-animation (vtable +0xa4, +0x8a5 & 3), with or without an action (FUN_10061950).
                    3 => match op.u32_at(12) {
                        0x3a => t.a1.id().map_or(0, |id| actors.sub_anim(id) & 3) as f64,
                        k => t.action.as_ref().map_or(0, |a| a.borrow().var(k, t.a1.id())) as f64,
                    },
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
        // The Effects filter, for action tasks off the unattached-persistent path *(the sound-type ok test is not
        // made)*; dolls count as their owners.
        if t.action.is_some() && (attached || !persistent) {
            let class = |w: Who| self.doll_of(w).id().map_or(8, |id| actors.fx_class(id));
            if !effects_allowed(self.filter.0, self.filter.1, class(t.a1) | class(t.a2) << 4) {
                return;
            }
        }
        log::debug!("effects: generator {} (DAT {}) on {:?} -> {:?}", String::from_utf8_lossy(&self.reg.libs[g.0].tree.nodes[g.1].name), self.reg.libs[g.0].file_id, t.a1, t.a2);
        let Some(i) = self.start(g, Owner::Task(me)) else { return };
        // FUN_10041f40: "%d" of the task's number, ":" (the "Miss!" art) under `misv`; fixed at emission.
        let misv = self.reg.libs[t.res.0].tree.nodes[t.res.1].name == *b"misv";
        let dam = t.num.map(|(v, rgb)| super::DamValue { glyphs: if misv { vec![10] } else { v.to_string().bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').take(16).collect() }, rgb });
        if let Some(x) = self.insts[i].as_mut() {
            x.actors = [t.a1, t.a2];
            x.dam = dam;
            if !persistent && !auto {
                let d = op.dur as f32 * t.speed;
                x.life = if d == 0.0 { 1.0 } else { d };
            }
        }
        self.attach_now(i, actors);
    }

    /// FUN_10052a50: instances of `g` on (a1, a2): killed now, or soft-stopped (emission ends, elements finish). No a1:
    /// every instance; a clone with no actor2 matches any a2 (asm 0x10052b38).
    fn stop_generator(&mut self, g: Ref, a1: Who, a2: Who, soft: bool) {
        for i in 0..self.insts.len() {
            let hit = self.insts[i].as_ref().is_some_and(|x| x.src == g && (a1 == Who::None || (x.actors[0] == a1 && (x.actors[1] == Who::None || x.actors[1] == a2))));
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

    /// FUN_100abea0: a doll where the actor stands now; FUN_10062d50 moves the clones this task started (clone
    /// +0xf0 == the task) to it, others stay on the actor.
    fn doll(&mut self, w: Who, actors: &dyn Actors, me: usize) -> Who {
        let Who::Actor(id) = w else { return w };
        let Some(m) = actors.matrix(id) else { return w };
        let d = Who::Doll(self.dolls.len());
        self.dolls.push(Doll { of: id, matrix: m });
        self.move_clones(w, d, me);
        d
    }

    /// Ops 0x17 / 0x18: back to the actor, with the clones of this task.
    fn undoll(&mut self, w: Who, me: usize) -> Who {
        let back = self.doll_of(w);
        if back != w {
            self.move_clones(w, back, me);
        }
        back
    }

    /// The actor a doll stands for.
    fn doll_of(&self, w: Who) -> Who {
        match w {
            Who::Doll(k) => self.dolls.get(k).map_or(w, |d| Who::Actor(d.of)),
            _ => w,
        }
    }

    fn move_clones(&mut self, from: Who, to: Who, me: usize) {
        for x in self.insts.iter_mut().flatten().filter(|x| x.owner == Owner::Task(me)) {
            for a in x.actors.iter_mut() {
                if *a == from {
                    *a = to;
                }
            }
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
            0x34 => actors.foot(id),
            // 0x36..0x3c name skeleton points (FUN_1003abe0).
            0x36 => world(0x7e),
            0x37 => world(0x7f),
            0x38..=0x3c => world(k - 0x38 + 0x66),
            0x35.. => world(0),
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
        // FUN_10052cd0 runs once per pass at retail's 2 ticks a frame: the same pull per 2 ticks at any frame rate.
        let k = 1.0 - (1.0 - ((word >> 2) & 0xff) as f32 / 255.0).powf(f.dt / 2.0);
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
            if self.models.contains_key(&id) || actors.hidden(id) || actors.model_gens_off(id) {
                continue;
            }
            let libs: Vec<Option<usize>> = files.iter().map(|&f| self.lib_for(f)).collect();
            if files.iter().zip(&libs).any(|(f, l)| l.is_none() && !self.failed.contains(f)) {
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
            if !gens.is_empty() {
                let names: Vec<String> = gens.iter().map(|g| String::from_utf8_lossy(&self.reg.libs[g.0].tree.nodes[g.1].name).into_owned()).collect();
                log::debug!("effects: model generators on {id:#x} {:?} at {:?}: {names:?}", files, actors.matrix(id).map(|m| m.w_axis.truncate()));
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

/// FUN_100ac4b0 (effect_filter.md): bit 0 limits the filter to the player's status 1 / 3; rows 1..16 hide when
/// their config bit is set and the caster class (low nibble) and target class (high) match (table 0x10324fb4:
/// caster mask 1 << ((b - 1) / 4), target mask 0x10 << ((b - 1) % 4)); bit 17 (screen shaking) has no row.
pub fn effects_allowed(cfg: u32, battle: bool, class: u8) -> bool {
    if cfg == 0 || (cfg & 1 != 0 && !battle) {
        return true;
    }
    !(1..=16u32).any(|b| cfg & (1 << b) != 0 && class & (1 << ((b - 1) / 4)) != 0 && class & (0x10 << ((b - 1) % 4)) != 0)
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

    /// A Hume with a motion file that cannot load (no such DAT).
    struct Broken;
    const BAD: u32 = 0x00ff_ffff;

    impl Actors for Broken {
        fn matrix(&self, _: u32) -> Option<Mat4> {
            Some(Mat4::IDENTITY)
        }
        fn point(&self, _: u32, _: u16) -> Option<Vec3> {
            None
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
            (vec![8392], vec![BAD])
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
    }

    /// cast_fx_spec F5: a failed file is passed by, so a +0x298 lookup reaches DAT 0's stwh instead of waiting on it
    /// for ever; part files are not looked in.
    #[test]
    fn failed_file_passed_by() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let stwh = u32::from_le_bytes(*b"stwh");
        let t = std::time::Instant::now();
        loop {
            fx.poll_loads();
            match fx.find_on_actor(HUME, stwh, &Broken, Parts::Skip, true) {
                Ok(r) => {
                    assert_eq!(r.map(|r| r.0), fx.reg.global);
                    break;
                }
                Err(()) => assert!(t.elapsed().as_secs() < 10, "still waiting on a failed file"),
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(fx.failed.contains(&BAD));
        assert!(!fx.loading.contains(&8392));
    }

    struct Walker;
    const FOOT: Vec3 = Vec3::new(3.0, 0.5, -2.0);

    impl Actors for Walker {
        fn matrix(&self, _: u32) -> Option<Mat4> {
            Some(Mat4::from_translation(Vec3::new(3.0, 0.0, -2.0)))
        }
        fn point(&self, _: u32, _: u16) -> Option<Vec3> {
            None
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
            (Vec::new(), Vec::new())
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
        fn foot(&self, _: u32) -> Option<Vec3> {
            Some(FOOT)
        }
    }

    /// footsteps_sound.md 1.3 step 7: a walking step in zone 232 bursts fses/fefs 'w100' once (attach mode 1,
    /// point 0x34 = the foot contact), and the clone goes when its elements have.
    #[test]
    fn walk_dust() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let mut fx = Effects::new();
        fx.set_keys(std::sync::Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(Library::load(332, index.read(332).unwrap()).unwrap());
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        fx.footstep(7, Vec3::new(3.0, 0.0, -2.0), *b"0112", Some(*b"w100"), false, false, 101);
        fx.tick(&f, &Walker);
        let dust: Vec<&crate::fx::Instance> = fx.insts.iter().flatten().filter(|x| &x.name == b"w100").collect();
        assert_eq!(dust.len(), 1);
        let x = dust[0];
        assert!(x.finished && !x.live.is_empty(), "emitted {} live {}", x.emitted, x.live.len());
        assert_eq!(x.matrix.map(|m| m.w_axis.truncate()), Some(FOOT));
        for _ in 0..600 {
            fx.tick(&f, &Walker);
        }
        assert!(!fx.insts.iter().flatten().any(|x| &x.name == b"w100"));
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
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
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
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
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
        // 0303 = se 3037 at the target (op 0x53 / 0x0b, far 60).
        assert!(fx.sound_cmds.iter().any(|c| matches!(c, super::super::sound::SoundCmd::Play { se: 3037, .. })), "{:?}", fx.sound_cmds);
        // stwh deleted gn2s and stopped gn20 / gn21.
        assert!(!names(&fx).iter().any(|n| n == "gn2s" || n == "gn20" || n == "gn21"), "{:?}", names(&fx));
    }

    /// A Hume M PC skeleton (race 1) whose list already holds the weapon-skill file.
    struct HumeWs;

    impl Actors for HumeWs {
        fn matrix(&self, id: u32) -> Option<Mat4> {
            Two.matrix(id)
        }
        fn point(&self, id: u32, k: u16) -> Option<Vec3> {
            Two.point(id, k)
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
            if id == HUME { (vec![8392], vec![33243, 9672, 7072]) } else { Two.files(id) }
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
        fn pc(&self, id: u32) -> Option<(u8, i8)> {
            (id == HUME).then_some((1, 0))
        }
    }

    /// triggers.md 3.1: Combo (weapon skill animation 16) from a Hume M is file 33227 + 16 = 33243, put on the
    /// actor's motion list; its "main" plays a20? and starts g006 / g000.
    #[test]
    fn weapon_skill_files() {
        assert_eq!(skill_files(3, 16, 1, 0), Some((33243, None)));
        assert_eq!(skill_files(3, 16, 2, 2), Some((33995 + 16, Some(34507 + 16))));
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        fx.preload(&[8392, 9672, 7072, 1300, 33243]);
        while [8392, 9672, 7072, 1300, 33243].iter().any(|f| fx.reg.lib_of(*f).is_none()) {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let a = Action { actor: HUME, category: 3, arg: 1, info: 0, targets: vec![ActionTarget { id: ORC, results: vec![ActionResult { animation: 16, message: 185, value: 40, ..Default::default() }] }] };
        fx.on_action(&a, None);
        let mut gens: Vec<String> = Vec::new();
        let mut moves: Vec<String> = Vec::new();
        for _ in 0..120 {
            fx.tick(&f, &HumeWs);
            for x in fx.insts.iter().flatten() {
                let n = String::from_utf8_lossy(&x.name).into_owned();
                if !gens.contains(&n) {
                    gens.push(n);
                }
            }
            moves.extend(fx.motions.drain(..).filter_map(|(_, m)| m.map(|m| m.name)));
        }
        assert_eq!(fx.motion_files.first(), Some(&(HUME, vec![33243])));
        assert!(moves.iter().any(|m| m == "a20"), "{moves:?}");
        for g in ["g006", "g000"] {
            assert!(gens.iter().any(|n| n == g), "{g}: {gens:?}");
        }
    }
}

#[cfg(test)]
mod skillchain_tests {
    use super::*;
    use crate::net::action::{ActionResult, ActionTarget, Side};

    pub(super) struct Pair;

    impl Actors for Pair {
        fn matrix(&self, _: u32) -> Option<Mat4> {
            Some(Mat4::IDENTITY)
        }
        fn point(&self, _: u32, _: u16) -> Option<Vec3> {
            Some(Vec3::ZERO)
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
            (Vec::new(), Vec::new())
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
        fn pc(&self, _: u32) -> Option<(u8, i8)> {
            None
        }
    }

    /// skillchain_spec.md 5: DAT 0 `proc` (op 0x80) with record 0's proc kind 14 plays Impaction (3268) on the
    /// target: hold 64, the reaction at t39, the proc message at `mprc` (t65); nothing for kind 0.
    #[test]
    fn impaction_burst() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let mut fx = Effects::new();
        let index = std::sync::Arc::new(index);
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let g = fx.reg.global.unwrap();
        let proc_ = fx.reg.libs[g].find_global(kind::SCHEDULER, u32::from_le_bytes(*b"proc")).unwrap();
        assert_eq!(fx.sched_ops((g, proc_)).first().map(|o| o.code), Some(0x80));
        for (kind, burst) in [(0u8, false), (14, true)] {
            let r = ActionResult { animation: 16, message: 185, value: 40, proc: (kind != 0).then_some(Side { kind, info: 0, value: 57, message: 301 }), ..Default::default() };
            let a = Action { actor: 1, category: 3, arg: 1, info: 0, targets: vec![ActionTarget { id: 2, results: vec![r] }] };
            let ctx = Rc::new(RefCell::new(ActionCtx { action: a, cur: 0, swing: None, printed: vec![true], last: 0, client_swing: false }));
            fx.spawn_task((g, proc_), Who::Actor(1), Who::Actor(2), Some(ctx), 1.0, None);
            let (mut prints, mut reacts, mut holds) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..600 {
                fx.tick(&f, &Pair);
                prints.extend(fx.action_prints.drain(..).map(|p| (p.1, p.2)));
                reacts.extend(fx.reacts.drain(..).map(|r| r.reactor));
                holds.extend(fx.holds.drain(..).map(|h| (h.0, h.1)));
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if burst {
                assert_eq!(std::mem::take(&mut fx.loads), vec![1, 2]);
                assert_eq!(std::mem::take(&mut fx.releases), vec![1, 2]);
                assert_eq!(prints, vec![(0, 1)]);
                assert_eq!(reacts, vec![2]);
                assert_eq!(holds, vec![(2, 64.0)]);
            } else {
                assert!(prints.is_empty() && reacts.is_empty() && holds.is_empty() && fx.loads.is_empty());
            }
        }
    }
}

#[cfg(test)]
mod damage_number_tests {
    use super::*;

    /// damage_display_spec.md 3.1: `damv` with 1234 starts dam0 at t0 and the glow dam1 at t2, both kind 0x5c
    /// elements carrying the glyphs and the colour; `misv` carries ':'.
    #[test]
    fn damv_elements() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let mut fx = Effects::new();
        let index = std::sync::Arc::new(index);
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        for (kind, glyphs) in [(1u8, vec![1u8, 2, 3, 4]), (3, vec![10])] {
            fx.on_damage_number(kind, 1, 2, 1234, 0x80_40_20);
            let mut seen = Vec::new();
            for _ in 0..300 {
                fx.tick(&f, &super::skillchain_tests::Pair);
                // The task's own instances carry the number; mis0's op 0x3c burst of mis1 runs without one.
                for x in fx.insts.iter().flatten() {
                    let n = String::from_utf8_lossy(&x.name).into_owned();
                    if x.parent_elem.is_none() && !seen.contains(&n) {
                        seen.push(n);
                        assert_eq!(x.dam.as_ref().map(|d| (d.glyphs.clone(), d.rgb)), Some((glyphs.clone(), 0x80_40_20)));
                    } else if x.parent_elem.is_some() {
                        assert!(x.dam.is_none());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
                if seen.len() >= 2 && fx.elems.iter().flatten().any(|c| c.draw == crate::fx::Draw::DamValue) {
                    break;
                }
            }
            assert!(fx.elems.iter().flatten().any(|c| c.draw == crate::fx::Draw::DamValue), "{seen:?}");
            let want: Vec<String> = if kind == 1 { vec!["dam0".into(), "dam1".into()] } else { vec!["mis0".into(), "mis1".into()] };
            for g in want {
                assert!(seen.contains(&g), "{g}: {seen:?}");
            }
            for _ in 0..120 {
                fx.tick(&f, &super::skillchain_tests::Pair);
            }
        }
    }
}

#[cfg(test)]
mod damage_number_rate_tests {
    use super::*;

    /// A `damv` number's elements live the same number of ticks at 30 fps (2 ticks a frame) and 300 fps (0.2).
    #[test]
    fn damv_life_by_frame_rate() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut lives = Vec::new();
        for dt in [2.0f32, 0.2] {
            let mut fx = Effects::new();
            fx.set_index(index.clone());
            fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
            let f = Frame { dt, seconds: dt / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
            while fx.reg.lib_of(27).is_none() {
                fx.poll_loads();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            fx.on_damage_number(1, 1, 2, 57, 0x808080);
            let (mut t, mut first, mut last) = (0.0f32, None, 0.0f32);
            while t < 200.0 {
                fx.tick(&f, &super::skillchain_tests::Pair);
                t += dt;
                if fx.elems.iter().flatten().any(|c| c.draw == crate::fx::Draw::DamValue) {
                    first.get_or_insert(t);
                    last = t;
                }
            }
            lives.push((first.unwrap_or(0.0), last));
        }
        eprintln!("damv visible ticks (first, last) at 30 / 300 fps: {lives:?}");
        assert!((lives[0].1 - lives[1].1).abs() <= 2.0 && (lives[0].0 - lives[1].0).abs() <= 2.0, "{lives:?}");
    }
}

#[cfg(test)]
mod load_tests {
    /// A model's files asked for before the first zone set the index are loaded on the next ask.
    #[test]
    fn preload_before_index() {
        let mut fx = super::Effects::new();
        fx.preload(&[9672]);
        assert!(!fx.loading.contains(&9672));
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        fx.set_index(std::sync::Arc::new(index));
        let t = std::time::Instant::now();
        while fx.lib_for(9672).is_none() && t.elapsed().as_secs() < 10 {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(fx.reg.lib_of(9672).is_some());
    }
}

#[cfg(test)]
mod filter_tests {
    use super::effects_allowed;

    /// effect_filter.md: bit 2 = you -> monster, bit 5 = monster -> you, bit 0 limits it to battle.
    #[test]
    fn filter_rows() {
        let you_hit_mob = 1 | 2 << 4;
        let mob_hits_you = 2 | 1 << 4;
        assert!(effects_allowed(0, true, you_hit_mob));
        assert!(!effects_allowed(1 << 2, false, you_hit_mob));
        assert!(effects_allowed(1 << 2, false, mob_hits_you));
        assert!(!effects_allowed(1 << 5, false, mob_hits_you));
        assert!(effects_allowed(1 << 2 | 1, false, you_hit_mob));
        assert!(!effects_allowed(1 << 2 | 1, true, you_hit_mob));
        // Others hitting others: bit 16 (non-party PC -> non-party PC).
        assert!(!effects_allowed(1 << 16, false, 8 | 8 << 4));
        assert!(effects_allowed(1 << 17, false, 8 | 8 << 4));
    }
}
