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
    /// FUN_10057fb0 returned 2: stay on this op, its countdown set by the op.
    Pause,
    End,
}

/// Hold kinds (`Effects::holds`): the status change waits (FUN_100a4140, +0x11c), no hit reactions (+0x80).
pub const HOLD_STATUS: u8 = 1;
pub const HOLD_REACT: u8 = 2;

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
    /// The caster whose skill files this `main` keeps (task +0xac = 0x100a8db0).
    pub skill_caster: Option<u32>,
    /// Wait until the actor lists this motion file (the client adds it).
    pub wait_motion: Option<(u32, u32)>,
    /// A damage number for the task (Task::num).
    pub num: Option<(i32, u32)>,
    /// Its motion ops (0x05 / 0x28) and holds are the client's (a death: combat::die plays ded? / cor?).
    pub quiet: bool,
    /// Looked up as vtable +0x298 (FUN_100cce00): the motion and model files, then DAT 0; no part files.
    pub own: bool,
}

/// A doll (CXiDollActor, FUN_100abea0): the actor's placement and its 48 points when a scheduler froze it. Ops
/// 0x22 / 0x23 make one that follows the actor's position each frame (FUN_100ac0f0), rotation and pose kept.
pub struct Doll {
    pub of: u32,
    pub matrix: Mat4,
    pub follow: bool,
    pub points: Vec<Option<Vec3>>,
}

const MAIN: u32 = u32::from_le_bytes(*b"main");
/// The points a doll copies (vtable +0x1c4, 0..0x30).
const DOLL_POINTS: u16 = 0x30;

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

/// FUN_100d25e0's files for a skill on a PC skeleton (tables 0x1035f484..0x1035f550, indexed by race code 0..8):
/// the motion and effect file, and the skirt file when body info byte 9 > 0 (the pair's first when it is 1).
fn skill_files(category: u8, anim: u16, race: u8, skirt: i8) -> Option<(u32, Option<u32>)> {
    const WS: [u32; 9] = [33227, 33227, 33995, 34763, 35531, 36299, 36299, 37067, 37835];
    const WS2: [[u32; 2]; 9] = [[33483, 33739], [33483, 33739], [34251, 34507], [35019, 35275], [35787, 36043], [36555, 36811], [36555, 36811], [37323, 37579], [38091, 38347]];
    const WS_HI: [u32; 9] = [61451, 61451, 61499, 61547, 61595, 61643, 61643, 61691, 61739];
    const WS_HI2: [[u32; 2]; 9] = [[61467, 61483], [61467, 61483], [61515, 61531], [61563, 61579], [61611, 61627], [61659, 61675], [61659, 61675], [61707, 61723], [61755, 61771]];
    const DANCE: [u32; 9] = [58041, 58041, 58425, 58809, 59193, 59577, 59577, 59961, 60345];
    const DANCE2: [[u32; 2]; 9] = [[58169, 58297], [58169, 58297], [58553, 58681], [58937, 59065], [59321, 59449], [59705, 59833], [59705, 59833], [60089, 60217], [60473, 60601]];
    const RUNE: [u32; 9] = [88035, 88035, 89571, 91107, 92643, 94179, 94179, 95715, 97251];
    const RUNE2: [[u32; 2]; 9] = [[88547, 89059], [88547, 89059], [90083, 90595], [91619, 92131], [93155, 93667], [94691, 95203], [94691, 95203], [96227, 96739], [97763, 98275]];
    let r = race as usize;
    if r >= 9 {
        return None;
    }
    let (a, k) = (anim as u32, (skirt != 1) as usize);
    let (file, second) = match category {
        0xe => (DANCE[r] + a, DANCE2[r][k] + a),
        0xf => (RUNE[r] + a, RUNE2[r][k] + a),
        _ if a < 0x100 => (WS[r] + a, WS2[r][k] + a),
        _ => (WS_HI[r] + a - 0x100, WS_HI2[r][k] + a - 0x100),
    };
    Some((file, Some(second).filter(|_| skirt > 0)))
}

/// 0x4f1a00: a Vana'diel day in 1/60 seconds.
pub const VANA_DAY: i32 = 0x4f1a00;

/// FUN_10057fb0 case 0x52 for the time of day `now`: (a copy runs now, countdown in 1/60 Vana'diel seconds).
pub fn vana_window(now: i32, start: u32, end: u32, period: i32, tol: i32) -> (bool, i32) {
    let (s, mut e) = (start as i32, end as i32);
    if s == e {
        e = s + VANA_DAY;
    }
    let (mut n, mut len) = (now - s, e - s);
    if len < 0 {
        len += VANA_DAY;
    }
    if n < 0 {
        n += VANA_DAY;
    }
    if n >= len || period <= 0 {
        return (false, VANA_DAY - n);
    }
    let p = n % period;
    (p < tol || (n + tol) % period < tol, period - p)
}

/// FUN_10053a00 / FUN_1003b700: no actor1 takes every clone; else actor1's clones whose actor2 is none or actor2.
fn pair_match(actors: [Who; 2], a1: Who, a2: Who) -> bool {
    a1 == Who::None || (actors[0] == a1 && (actors[1] == Who::None || actors[1] == a2))
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

    /// Whether every file is in (or failed); the others start loading.
    pub fn libs_ready(&mut self, files: &[u32]) -> bool {
        files.iter().fold(true, |ok, &f| (self.lib_for(f).is_some() || self.failed.contains(&f)) && ok)
    }

    pub fn poll_loads(&mut self) {
        while let Ok((file, lib)) = self.load_rx.try_recv() {
            match lib {
                Some(l) if self.reg.lib_of(file).is_none() => {
                    let l = self.reg.add(l);
                    // FUN_10073430's link pass: its self-starting schedulers start (event files: on no actors).
                    self.start_auto(l, None);
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
            1 | 2 => self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: false }),
            3..=6 | 0xb | 0xd..=0xf => {
                let anim = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.animation as u32);
                let Some(base) = effect_base(a.category, anim) else {
                    let ctx = Some(ctx);
                    self.release(&ctx);
                    self.end_action(&ctx);
                    return;
                };
                // The generic file; run_pending swaps in the per-race files for a skeleton caster.
                self.pending.push(Pending { file: Some(base + anim), actor: None, name: MAIN, a1: Who::Actor(caster), a2: first, action: Some(ctx), speed, end_cast: Some(caster), age: 0.0, skill: matches!(a.category, 3 | 0xe | 0xf).then_some((a.category, anim as u16)), skill_caster: None, wait_motion: None, num: None, quiet: false, own: false });
            }
            7..=0xa | 0xc => {
                // Starts: the cmd_arg scheduler on the caster itself; "ca??" goes through FUN_10083d60 first.
                if a.arg & 0xffff == 0x6163 {
                    self.cast_state(caster, a.arg);
                }
                self.pending.push(Pending { file: None, actor: Some(caster), name: a.arg, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: true });
            }
            _ => {}
        }
    }

    /// FUN_10083d60(caster, arg): a stored cast 4cc runs as "st??" (ends the casting effect); +0xa0 = arg either way
    /// (asm 0x10083d83; the decompile's 0 is a stack misread), 0 clearing it.
    fn cast_state(&mut self, caster: u32, arg: u32) {
        if let Some(prev) = self.casting.remove(&caster) {
            let st = (prev & 0xffff_7473) | 0x7473;
            self.pending.push(Pending { file: None, actor: Some(caster), name: st, a1: Who::Actor(caster), a2: Who::Actor(caster), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: true });
        }
        if arg != 0 {
            self.casting.insert(caster, arg);
        }
    }

    /// Event opcodes 0x45 and family: the scheduler `name` of DAT `file` as a task on (a1, a2), the same task the
    /// event's camera runner follows (its camera / fade ops are the runner's; the rest run here).
    pub fn on_event_scheduler(&mut self, file: u32, name: u32, a1: Who, a2: Who) {
        log::debug!("effects: event scheduler {} of DAT {file}", String::from_utf8_lossy(&name.to_le_bytes()));
        self.pending.push(Pending { file: Some(file), actor: None, name, a1, a2, action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: false });
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
        self.pending.push(Pending { file: Some(27), actor: None, name: u32::from_le_bytes(name), a1: Who::Actor(a1), a2: Who::Actor(a2), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: Some((value, rgb)), quiet: false, own: false });
    }

    /// A status scheduler on the actor with its motions left to the client (FUN_1008c6c0's `dead`).
    pub fn on_scheduler_quiet(&mut self, actor: u32, name: u32) {
        self.pending.push(Pending { file: None, actor: Some(actor), name, a1: Who::Actor(actor), a2: Who::Actor(actor), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: true, own: false });
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

    /// The actor's destructor (FUN_10082b10): every effect that has it as caster or target forgets it and runs on
    /// without (the attach modes then give the identity, 0x1003c5f0); starts waiting on its own files go.
    pub fn forget_actor(&mut self, id: u32) {
        let who = Who::Actor(id);
        let clear = |w: &mut Who| {
            if *w == who {
                *w = Who::None;
            }
        };
        self.pending.retain(|p| p.actor != Some(id));
        self.equip.remove(&id);
        self.equip_calls.retain(|c| c.0 != id);
        for p in &mut self.pending {
            clear(&mut p.a1);
            clear(&mut p.a2);
        }
        for t in self.tasks.iter_mut().flatten() {
            clear(&mut t.a1);
            clear(&mut t.a2);
        }
        for x in self.insts.iter_mut().flatten() {
            x.actors.iter_mut().for_each(|w| clear(w));
        }
    }

    /// 0x038: a scheduler by 4cc on an actor (FUN_100cce00 with no action).
    pub fn on_scheduler(&mut self, actor: u32, target: u32, name: u32) {
        self.pending.push(Pending { file: None, actor: Some(actor), name, a1: Who::Actor(actor), a2: Who::Actor(target), action: None, speed: 1.0, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: true });
    }

    /// Starts waiting for files, when they are in.
    pub(super) fn run_pending(&mut self, f: &Frame, actors: &dyn Actors) {
        let list = std::mem::take(&mut self.pending);
        for mut p in list {
            p.age += f.dt;
            // FUN_100a9230 -> FUN_100d25e0: a skeleton caster (any actor with a model) takes weapon skills from its race
            // code's files (and a skirt variant), loaded as motion files on the actor; only a caster without one keeps
            // the generic file.
            if let (Some((cat, anim)), Some(caster)) = (p.skill.take(), p.a1.id())
                && let Some((race, skirt)) = actors.pc(caster)
                && let Some((file, second)) = skill_files(cat, anim, race, skirt)
            {
                p.file = Some(file);
                p.wait_motion = Some((caster, file));
                p.skill_caster = Some(caster);
                self.motion_files.push((caster, std::iter::once(file).chain(second).collect()));
            }
            if let Some((actor, file)) = p.wait_motion
                && !actors.files(actor).1.contains(&file)
            {
                if p.age < 600.0 {
                    self.pending.push(p);
                } else {
                    self.release(&p.action);
                    self.end_action(&p.action);
                }
                continue;
            }
            // FUN_100a9500 (the 0x028 skill, spell and ability starts): the caster's model and files first; without
            // the caster's actor the wait ends with no `main`.
            if p.end_cast.is_some()
                && let Some(caster) = p.a1.id()
            {
                match actors.ready(caster) {
                    None => {
                        log::debug!("effects: {caster:#x} has no actor, no main");
                        self.release(&p.action);
                        self.end_action(&p.action);
                        continue;
                    }
                    Some(false) if p.age < 600.0 => {
                        self.pending.push(p);
                        continue;
                    }
                    _ => {}
                }
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
                    // FUN_100a9500 then checks every target once: one without an actor or still loading and `main`
                    // does not start.
                    let blocked = p.end_cast.is_some() && p.action.as_ref().is_some_and(|a| a.borrow().action.targets.iter().any(|t| actors.ready(t.id) != Some(true)));
                    if blocked && res.is_some() {
                        log::debug!("effects: main not started, a target's model is not in");
                        self.end_action(&p.action);
                        continue;
                    }
                    match res {
                        Some(r) => {
                            log::debug!("effects: scheduler {} started", String::from_utf8_lossy(&p.name.to_le_bytes()));
                            let ti = self.spawn_task(r, p.a1, p.a2, p.action, p.speed, None);
                            if let Some(Some(t)) = self.tasks.get_mut(ti) {
                                t.num = p.num;
                                t.motions &= !p.quiet;
                                if let Some(c) = p.skill_caster {
                                    self.skill_mains.push((t.uid, c));
                                }
                            }
                        }
                        None => {
                            if let Some(a) = self.reg.audit.as_mut() {
                                *a.no_sched.entry(p.name).or_default() += 1;
                            }
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

    /// FUN_100ce480 / vtable +0x1e8 for an Afb: the motion files in order, matched on 3 letters, then the first Afb of
    /// any name in the part files. Err while one of them is still loading.
    fn find_afb(&mut self, actor: u32, name: u32, actors: &dyn Actors) -> Result<Option<super::trail::Afb>, ()> {
        let (parts, motions) = actors.files(actor);
        let want = name.to_le_bytes();
        let mut waiting = false;
        for (files, any) in [(motions, false), (parts, true)] {
            for file in files {
                match self.lib_for(file) {
                    Some(l) => {
                        let lib = &self.reg.libs[l];
                        if let Some(n) = lib.tree.nodes.iter().position(|x| x.kind == kind::AFB && (any || x.name[..3] == want[..3])) {
                            return Ok(super::trail::Afb::parse(lib.data(n)));
                        }
                    }
                    None if self.failed.contains(&file) => {}
                    None => waiting = true,
                }
            }
            if waiting {
                return Err(());
            }
        }
        Ok(None)
    }

    pub(super) fn spawn_task(&mut self, r: Ref, a1: Who, a2: Who, action: Option<Rc<RefCell<ActionCtx>>>, speed: f32, parent: Option<usize>) -> usize {
        let ops = self.sched_ops(r);
        log::debug!("effects: task {} (DAT {}) on {a1:?} -> {a2:?}", String::from_utf8_lossy(&self.reg.libs[r.0].tree.nodes[r.1].name), self.reg.libs[r.0].file_id);
        let mut t = Task::new(r, ops, a1, a2, action, speed);
        t.ends_loop = crate::dat::sched::loops(self.reg.libs[r.0].data(r.1));
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
            if t.paused > 0 {
                self.tasks[i] = Some(t);
                i += 1;
                continue;
            }
            // FUN_10057c20 a frame after the end op, then FUN_100573e0(res, 0, 0): speed 1.0, the action wrapper and the
            // parent link dropped (the action never prints its rest; a calling parent waits for ever); no op runs.
            if std::mem::take(&mut t.restart) {
                (t.ip, t.speed, t.countdown, t.early, t.vana, t.parent) = (0, 1.0, 0.0, 0.0, false, None);
                if let Some(a) = t.action.take() {
                    self.leaked.push(a);
                }
                self.tasks[i] = Some(t);
                i += 1;
                continue;
            }
            self.stepping = Some(i);
            if !t.waiting {
                t.countdown -= if t.vana { self.vana_dt } else { f.dt };
            }
            let mut guard = 0;
            while !t.done && !t.waiting && t.countdown <= 0.0 && guard < 512 {
                guard += 1;
                if t.ip >= t.ops.len() || t.stop_at == Some(t.ip) {
                    // FUN_10057c20: op 0x73's children and end-stream loops start over (the step after).
                    if (t.looping || t.ends_loop) && t.stop_at.is_none() {
                        t.restart = true;
                        break;
                    }
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
                    Flow::Pause => break,
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
        let live: Vec<u64> = self.tasks.iter().flatten().map(|t| t.uid).collect();
        self.step_shine(f.dt, &live);
        self.step_wep_timers(f.dt, &live);
        // Op 0xb5's holds end with their time or their task (FUN_100ce0c0).
        self.no_blink.clear();
        for (owner, id, left) in std::mem::take(&mut self.blink_holds) {
            let left = left - f.dt;
            if left >= 0.0 && live.contains(&owner) {
                self.no_blink.insert(id);
                self.blink_holds.push((owner, id, left));
            }
        }
        // Op 0x2e's roots end below 0 (FUN_10062380) or with their task; each end restarts actor1's status motion
        // (FUN_100622c0 -> FUN_100ca9b0).
        self.rooted.clear();
        for (owner, id, left) in std::mem::take(&mut self.roots) {
            let left = left - f.dt;
            if left < 0.0 || !live.contains(&owner) {
                self.status_restarts.push(id);
            } else {
                self.rooted.insert(id);
                self.roots.push((owner, id, left));
            }
        }
        // Op 0x7a's pins end with their time or their task (FUN_10061f50).
        self.pinned.clear();
        let eye = Some(f.eye);
        for (owner, a, b, k, off, left) in std::mem::take(&mut self.pins) {
            let left = left - f.dt;
            if left < 0.0 || !live.contains(&owner) {
                continue;
            }
            if let Some((m, p)) = self.who_point(Who::Actor(b), k, Who::Actor(a), eye, Vec3::ZERO, actors) {
                let yaw = (k < 0x30).then(|| (-m.x_axis.z).atan2(m.x_axis.x) + std::f32::consts::PI);
                self.pinned.insert(a, (p + m.transform_vector3(off), yaw));
            }
            self.pins.push((owner, a, b, k, off, left));
        }
        // FUN_1005f820: a drive ends below 0; its scheduler's death deletes it early, frozen (0xa0: on its target).
        for mut d in std::mem::take(&mut self.color_drives) {
            d.left -= f.dt;
            if !live.contains(&d.owner) {
                if d.left >= 0.0 {
                    self.fades.push(if d.snap { crate::fx::ColorOp::Snap(d.actor, d.target) } else { crate::fx::ColorOp::Freeze(d.actor) });
                }
            } else if d.left >= 0.0 {
                self.color_drives.push(d);
            }
        }
        for (owner, id, mut left) in std::mem::take(&mut self.color_guards) {
            left -= f.dt;
            if !live.contains(&owner) {
                if left >= 0.0 {
                    self.fades.push(crate::fx::ColorOp::Snap(id, 0x8080_8080));
                }
            } else if left >= 0.0 {
                self.color_guards.push((owner, id, left));
            }
        }
        // 0x100a8db0: a skill `main`'s end drops its caster's skill files (FUN_100d44b0(6)) unless another skill `main`
        // of that caster still runs.
        if !self.skill_mains.is_empty() {
            let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.skill_mains).into_iter().partition(|(o, _)| live.contains(o));
            for (_, c) in gone {
                if !keep.iter().any(|k| k.1 == c) && !self.skill_files_done.contains(&c) {
                    self.skill_files_done.push(c);
                }
            }
            self.skill_mains = keep;
        }
        // Effect motion files whose every owning task has ended come off their actors.
        if !self.owned_motions.is_empty() {
            let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.owned_motions).into_iter().partition(|(o, ..)| live.contains(o));
            for (_, a, f) in gone {
                if !keep.iter().any(|&(_, ka, kf)| ka == a && kf == f) && !self.effect_motions_done.contains(&(a, f)) {
                    self.effect_motions_done.push((a, f));
                }
            }
            self.owned_motions = keep;
        }
    }

    fn run_op(&mut self, me: usize, t: &mut Task, op: &SOp, actors: &dyn Actors) -> Flow {
        let scope = |s: &Self, k: u8| s.reg.resolve(t.res.0, t.res.1, k, op.name());
        log::trace!("effects: task {:?} op {:#04x} {}", t.res, op.code, String::from_utf8_lossy(&op.name().to_le_bytes()));
        if let Some(a) = self.reg.audit.as_mut() {
            *a.ops.entry(op.code).or_default() += 1;
        }
        match op.code {
            0x00 => Flow::End,
            // FUN_10057fb0 case 0x52: a Vana'diel time window. The task counts on the Vana'diel clock and stays on the
            // op; inside the window at a period boundary (or within the tolerance of one) a copy runs the rest on the
            // tick clock, its countdown the op's wait x speed, stepped this frame (FUN_10074920 / FUN_100750b0).
            0x52 => {
                t.vana = true;
                let (spawn, wait) = vana_window(self.vana_now, op.u32_at(8), op.u32_at(0xc), op.u32_at(0x10) as i32, op.u32_at(0x14) as i32);
                if spawn {
                    let mut c = Task::new(t.res, t.ops.clone(), t.a1, t.a2, t.action.clone(), t.speed);
                    (c.ip, c.countdown, c.parent, c.motions, c.num, c.ends_loop) = (t.ip + 1, op.wait as f32 * t.speed, t.parent, t.motions, t.num, t.ends_loop);
                    log::debug!("effects: {} window opens", String::from_utf8_lossy(&self.reg.libs[t.res.0].tree.nodes[t.res.1].name));
                    self.tasks.push(Some(c));
                }
                t.countdown = wait as f32;
                Flow::Pause
            }
            // No case (0x1005ac96): it only keeps the scheduler from starting by itself.
            0xa2 => Flow::Next,
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
            // 0x73 (FUN_10055cc0 with 1): the child loops (+0x141) until killed.
            0x03 | 0x73 => {
                if let Some(r) = scope(self, kind::SCHEDULER) {
                    let i = self.spawn_task(r, t.a1, t.a2, t.action.clone(), t.speed, None);
                    if let Some(c) = self.tasks[i].as_mut() {
                        c.looping = op.code == 0x73;
                    }
                }
                Flow::Next
            }
            // No case in FUN_10057fb0's switch: 0x01 opens most schedulers, 0x69 opens a block (0x64 / 0x65 take it);
            // 0x50, 0x74; 0x7f and 0x86 write fields nothing reads.
            0x01 | 0x50 | 0x74 | 0x69 | 0x7f | 0x86 => Flow::Next,
            // Reached in the main flow (no 0x64 took the block): the task is killed (vtable +0x18).
            0x6a | 0x67 => Flow::End,
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
                t.a1 = self.doll(t.a1, op.code == 0x22, actors, me);
                Flow::Next
            }
            0x16 | 0x23 => {
                t.a2 = self.doll(t.a2, op.code == 0x23, actors, me);
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
            // FUN_10057fb0 case 0x1d: the lift move; its own duration @6 is unused (the packet's end time sets it).
            0x1d => {
                if let Some(actor) = t.a1.id() {
                    let name = self.reg.libs[t.res.0].tree.nodes[t.res.1].name;
                    self.lift_moves.push(super::LiftMove { actor, floor: name[3].wrapping_sub(b'0') });
                }
                Flow::Next
            }
            // Sounds at a snapshot of the actor's position: 0x0a actor1, 0x0b actor2, 0x53 the action target nearest
            // the player *(inf: actor2)*, 0x4a actor2 only when it is the player (vtable +0x304, FUN_100a4640: the
            // level-up jingle); far @0x14, near @0x18, flag 0x100 = height as is.
            0x4a if t.a2.id().is_none() || t.a2.id() != actors.player() => Flow::Next,
            // FUN_10057fb0 case 0xb3: with /bellsw off the op ends; on, it is case 0x0a.
            0xb3 if !actors.bells() => Flow::Next,
            0x0a | 0x0b | 0x53 | 0x4a | 0xb3 => {
                let mut who = if matches!(op.code, 0x0a | 0xb3) { t.a1 } else { t.a2 };
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
                // asm 0x1005b760: an actor reserved by its event (+0x138 bit 31) whose model is hidden is silent.
                if self.doll_of(who).id().is_some_and(|id| actors.sound_muted(id)) {
                    return Flow::Next;
                }
                if let (Some(r), Some((_, m))) = (r, self.who_matrix(who, actors))
                    && let Some((se, loops)) = super::sound::sesep(self.reg.libs[r.0].data(r.1))
                {
                    log::debug!("effects: sound {se} op {:#x}", op.code);
                    let mut s = super::sound::Sound::new(se, loops);
                    s.res = r;
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
                    s.res = r;
                    s.stream = true;
                    self.sounds.push(s);
                }
                Flow::Next
            }
            0x19 => {
                self.pending.push(Pending { file: Some(0xaf0 + op.u32_at(8)), actor: None, name: MAIN, a1: t.a1, a2: t.a2, action: None, speed: t.speed, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: false });
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
                    // A motion in the scheduler's own file plays from there: that file goes on the actor first.
                    let (lib, raw) = (t.res.0, op.name().to_le_bytes());
                    let has = |n: u32| self.reg.libs[lib].find(t.res.1, kind::MOTION, n).is_some();
                    let in_file = if raw[3] == b'?' { (b'0'..=b'6').any(|d| has(u32::from_le_bytes([raw[0], raw[1], raw[2], d]))) } else { has(op.name()) };
                    let file = self.reg.libs[lib].file_id;
                    let owned = self.owned_motions.iter().any(|&(o, a, f)| o == t.uid && a == id && f == file);
                    // A file the actor already has (its model, a skill file) plays as it is.
                    if in_file && (owned || !actors.files(id).1.contains(&file)) {
                        if !owned {
                            log::debug!("effects: motion {} on {id:#x} from DAT {file}", String::from_utf8_lossy(&raw));
                            self.owned_motions.push((t.uid, id, file));
                            self.effect_motions.push((id, file));
                        }
                        if !actors.files(id).1.contains(&file) {
                            return Flow::Retry;
                        }
                    }
                    let name = String::from_utf8_lossy(&op.name().to_le_bytes()).trim_end_matches(['\0', '?']).to_string();
                    let speed = f32::from_bits(op.u32_at(0x10));
                    let count = op.raw.get(0x1e).map_or(1, |b| b & 0x7f);
                    let m = crate::dat::sched::Motion { name, at: 0, speed: if speed == 0.0 { 1.0 } else { speed }, ticks: (op.dur.max(0) as f32 * t.speed).round() as u16, count, blend: op.u16_at(0x18) };
                    self.motions.push((id, Some(m)));
                }
                Flow::Next
            }
            // FUN_100aad40: actor1 moves itself along its facing for u16 @6 ticks at f32 @8 / 60 units a tick (a dismounted
            // animal running off).
            0x6e => {
                if let Some(id) = t.a1.id() {
                    self.self_moves.push((id, op.dur.max(0) as f32, f32::from_bits(op.u32_at(8)) / 60.0));
                }
                Flow::Next
            }
            // FUN_1005f6c0 channel 0: the real actor's model colour toward ARGB u32 @8 over dur x speed ticks (0x2a
            // actor2; 0xa0 actor1, landing on its target even when cut short).
            0x29 | 0x2a | 0xa0 => {
                if let Some(id) = self.doll_of(if op.code == 0x2a { t.a2 } else { t.a1 }).id() {
                    self.color_drive(id, t.uid, op.u32_at(8), op.dur.max(0) as f32 * t.speed, op.code == 0xa0);
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
            // FUN_100a4140 for op dur x speed: 0x20 holds actor2, 0x1f actor1 (their status changes wait); 0x59
            // actor1 with no reactions; 0x07 / 0x08 actor1 / actor2 no reactions only (+0x80, FUN_100607e0). Quiet
            // tasks too: the draw's 0x07 keeps the run's end from cutting its `ina` short.
            0x20 | 0x1f | 0x59 | 0x07 | 0x08 => {
                let (who, kind) = match op.code {
                    0x20 => (t.a2, HOLD_STATUS),
                    0x1f => (t.a1, HOLD_STATUS),
                    0x59 => (t.a1, HOLD_STATUS | HOLD_REACT),
                    0x07 => (t.a1, HOLD_REACT),
                    _ => (t.a2, HOLD_REACT),
                };
                if let Some(id) = self.doll_of(who).id() {
                    self.holds.push((id, op.dur.max(0) as f32 * t.speed, kind));
                }
                Flow::Next
            }
            // Every record's target (FUN_100601f0 each); without an action, op 0x20.
            0x56 if t.motions => {
                let ticks = op.dur.max(0) as f32 * t.speed;
                match &t.action {
                    Some(a) => {
                        let ids: Vec<u32> = a.borrow().action.targets.iter().map(|x| x.id).collect();
                        self.holds.extend(ids.into_iter().map(|id| (id, ticks, HOLD_STATUS)));
                    }
                    None => {
                        if let Some(id) = self.doll_of(t.a2).id() {
                            self.holds.push((id, ticks, HOLD_STATUS));
                        }
                    }
                }
                Flow::Next
            }
            // The shine overlay (FUN_100617b0, FUN_100615b0, FUN_1005f6c0 channel 1), on actor1 (0x40 / 0x54 / 0x42 /
            // 0x43 / 0x46) or actor2 (0x41 / 0x55 / 0x44 / 0x45 / 0x47) for dur x speed: on with the TexInfo of 4cc
            // u32 @8 (the loader makes one beside each IMG, same 4cc); U (0x42 / 0x44) or V (0x43 / 0x45) linearly to
            // f32 @8, kept mod 2; the colour to ARGB u32 @8 (a new colour drive replaces the running one).
            0x40 | 0x41 | 0x54 | 0x55 | 0x42..=0x47 => {
                let who = if matches!(op.code, 0x40 | 0x54 | 0x42 | 0x43 | 0x46) { t.a1 } else { t.a2 };
                let Some(id) = self.doll_of(who).id() else { return Flow::Next };
                let left = (op.dur.max(0) as f32 * t.speed).trunc();
                let kind = match op.code {
                    0x40 | 0x41 | 0x54 | 0x55 => {
                        let Some(r) = scope(self, kind::IMG) else { return Flow::Next };
                        self.shine.entry(id).or_default().tex = Some((r, self.next_drive + 1));
                        log::debug!("effects: shine on {id:#x}: {} of DAT {} for {left} ticks", self.reg.libs[r.0].tree.nodes[r.1].name(), self.reg.libs[r.0].file_id);
                        crate::fx::ShineKind::Overlay
                    }
                    0x46 | 0x47 => {
                        self.shine_drives.retain(|d| !(d.actor == id && matches!(d.kind, crate::fx::ShineKind::Tf { .. })));
                        // Another scheduler's op 0x88 on the actor: the drive never writes (the running one is gone).
                        if self.color_locked(id, t.uid) {
                            return Flow::Next;
                        }
                        crate::fx::ShineKind::Tf { start: self.shine.entry(id).or_default().tf, target: op.u32_at(8), total: left }
                    }
                    c => {
                        let v = matches!(c, 0x43 | 0x45);
                        let s = self.shine.entry(id).or_default();
                        crate::fx::ShineKind::Uv { v, start: if v { s.v } else { s.u }, target: f32::from_bits(op.u32_at(8)), total: left }
                    }
                };
                self.next_drive += 1;
                self.shine_drives.push(crate::fx::ShineDrive { id: self.next_drive, owner: t.uid, actor: id, left, kind });
                Flow::Next
            }
            // FUN_10061240 (0x48 actor1, 0x49 actor2): the refraction pass on at once (+0x734 |= 2) and its level from
            // where it is to f32 @8, linearly over trunc(dur x speed); the end (or the scheduler's death) snaps to the
            // target, and a target of 0 turns the pass off. Drives do not replace each other.
            0x48 | 0x49 => {
                let Some(id) = self.doll_of(if op.code == 0x48 { t.a1 } else { t.a2 }).id() else { return Flow::Next };
                let left = (op.dur.max(0) as f32 * t.speed).trunc();
                let s = self.shine.entry(id).or_default();
                s.refract = true;
                let kind = crate::fx::ShineKind::Refract { start: s.level, target: f32::from_bits(op.u32_at(8)), total: left };
                self.next_drive += 1;
                self.shine_drives.push(crate::fx::ShineDrive { id: self.next_drive, owner: t.uid, actor: id, left, kind });
                Flow::Next
            }
            // FUN_100618a0: a dead (status 2 / 3) actor2 (0xb2) or actor1 (0xb1): model colour 0x80808080 at once.
            0xb1 | 0xb2 => {
                if let Some(id) = self.doll_of(if op.code == 0xb1 { t.a1 } else { t.a2 }).id()
                    && matches!(actors.status(id), 2 | 3)
                {
                    self.fades.push(crate::fx::ColorOp::Snap(id, 0x8080_8080));
                }
                Flow::Next
            }
            // FUN_1005f320 (CMoLockColorDriveTask): for dur x speed other schedulers' colour drives on actor1 do not
            // write; when its scheduler task ends before that, actor1's model colour goes back to 0x80808080.
            0x88 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.color_guards.push((t.uid, id, op.dur.max(0) as f32 * t.speed));
                }
                Flow::Next
            }
            // FUN_1005a432: actor1's Afb by the 4cc's first three letters (motion files newest first, then the model's;
            // failing those the first Afb in its part files) starts a top-level after-image; none: nothing.
            0x2c => {
                let Some(id) = self.doll_of(t.a1).id() else { return Flow::Next };
                match self.find_afb(id, op.u32_at(8), actors) {
                    Err(()) => return Flow::Retry,
                    Ok(Some(afb)) => {
                        log::debug!("effects: after-image {} on {id:#x} ({} points)", String::from_utf8_lossy(&op.raw[8..11]), afb.a.len());
                        let tr = super::trail::Trail::new(id, afb, &op.raw);
                        match self.trails.iter().position(|t| t.is_none()) {
                            Some(i) => self.trails[i] = Some(tr),
                            None => self.trails.push(Some(tr)),
                        }
                        self.trails_dirty = true;
                    }
                    Ok(None) => {}
                }
                Flow::Next
            }
            // FUN_10061af0: actor1 does not blink for dur x speed (actor +0x8aa += 1, FUN_100ce0b0).
            0xb5 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.blink_holds.push((t.uid, id, op.dur.max(0) as f32 * t.speed));
                }
                Flow::Next
            }
            // FUN_10057fb0 case 0xac -> vtable +0x130 (FUN_10084ad0): actor1's death scheduler pick, entity +0x130
            // bits 5..6 = u32 @8 & 3, read when it dies (FUN_1008c6c0); instant.
            0xac => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.death_picks.push((id, (op.u32_at(8) & 3) as u8));
                }
                Flow::Next
            }
            // Case 0xb7 -> vtable +0xd4 (FUN_10084740): actor1's entity race +0xef = u32 @8, kept over 0x00E's while
            // nonzero (+0x12c bit 6). A trust's model init names its skeleton's race (Valaineral, model 3014: 3).
            0xb7 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.races.push((id, op.u32_at(8) as u8));
                }
                Flow::Next
            }
            // Case 0x99 (FUN_10084980): actor1's entity scale +0x204 = f32 @8.
            0x99 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.entity_scales.push((id, f32::from_bits(op.u32_at(8))));
                }
                Flow::Next
            }
            // Case 0x1005c18f (FUN_1005e720): actor1's entity +0x130 bit 4 = u32 @8 & 1.
            0xab => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.base_resets.push((id, op.u32_at(8) & 1 != 0));
                }
                Flow::Next
            }
            // FUN_1005e770 / 740 / 760 / 750: actor1's battle / idle / run / walk names from set u32 @8.
            0x79 | 0x8c | 0xa4 | 0xa5 => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    self.motion_sets.push((id, op.code, op.u32_at(8)));
                }
                Flow::Next
            }
            // FUN_10057000 cases 0x0f / 0x72 / 0x0e: the scene multiply, the additive screen flash and the feedback
            // blur, a global task each (a new one ends the last at its target).
            0x0f | 0x72 | 0x0e => {
                let c = op.u32_at(8).to_le_bytes();
                let zoom = if op.code == 0x0e { f32::from_bits(op.u32_at(0xc)) } else { 0.0 };
                self.screen_ops.push((op.code, c, zoom, op.dur.max(0) as f32 * t.speed));
                Flow::Next
            }
            // FUN_10061fd0: for dur x speed, actor1 stands at actor2's point (u32 @0x20) + offset (vec3 @0x10) turned
            // by actor2's yaw, facing actor2's yaw + pi for ordinary points (FUN_100c63d0).
            0x7a => {
                if let (Some(a), Some(b)) = (self.doll_of(t.a1).id(), self.doll_of(t.a2).id())
                    && a != b
                {
                    let off = Vec3::new(f32::from_bits(op.u32_at(0x10)), f32::from_bits(op.u32_at(0x14)), f32::from_bits(op.u32_at(0x18)));
                    self.pins.retain(|p| p.1 != a);
                    self.pins.push((t.uid, a, b, op.u32_at(0x20) as u16, off, op.dur.max(0) as f32 * t.speed));
                }
                Flow::Next
            }
            // FUN_10053a00 / FUN_10052a50: every instance of generator A (4cc @8) on the pair ends, each live element of
            // it bursting generator B (4cc @0x10) once where it is (FUN_10052930). A static A morphing in place and a
            // lone B emission when no A runs are not done.
            0x3f => {
                let (Some(ga), Some(gb)) = (scope(self, kind::GENERATOR), self.reg.resolve(t.res.0, t.res.1, kind::GENERATOR, op.u32_at(0x10))) else { return Flow::Next };
                let f = self.last_frame;
                // An unattached static A morphs as its template (every instance); else FUN_10053a00's actor match. With
                // no A running, retail's lone B emission is deleted in the same op: nothing.
                let any = self.template_wide(ga);
                for i in 0..self.insts.len() {
                    if !self.insts[i].as_ref().is_some_and(|x| x.src == ga && x.driven.is_none() && (any || pair_match(x.actors, t.a1, t.a2))) {
                        continue;
                    }
                    if let Some(f) = f.as_ref() {
                        let live = self.insts[i].as_ref().map(|x| x.live.clone()).unwrap_or_default();
                        for e in live {
                            self.burst(gb, e, f, actors);
                        }
                    }
                    self.kill(i);
                }
                Flow::Next
            }
            // Actor1's wepN meshes, N = i16 @0xc: 0x75 on hide byte 0 (hide u32 @8 != 0, show 0; i16 @0xe: only while
            // actor1 is engaged; dur x speed != 0.0: hidden now and, when @8 is 0, shown at the end of that or of the
            // scheduler, FUN_10061960), 0xa3 on byte 1, untimed and ungated.
            0x75 | 0xa3 => {
                let Some(id) = self.doll_of(t.a1).id() else { return Flow::Next };
                let (n, show) = (op.u16_at(0xc) as u8, op.u32_at(8) == 0);
                if op.code == 0xa3 {
                    self.weapons.push(crate::fx::WeaponOp { actor: id, n, byte: 1, hide: !show });
                    return Flow::Next;
                }
                if op.u16_at(0xe) != 0 && actors.status(id) != 1 {
                    return Flow::Next;
                }
                let len = op.dur as f32 * t.speed;
                self.weapons.push(crate::fx::WeaponOp { actor: id, n, byte: 0, hide: !show || len != 0.0 });
                if len != 0.0 {
                    self.wep_timers.push(super::equip::WepTimer { owner: t.uid, actor: id, n, left: len.trunc(), show });
                }
                Flow::Next
            }
            // Actor1's move input (FUN_10062310, +0xfc: the player's walk is ignored; a child of the scheduler) and
            // facing (FUN_100624b0, +0x838: no heading ease, network headings left alone) for dur x speed.
            0x2e | 0x2f => {
                if let Some(id) = self.doll_of(t.a1).id() {
                    let len = op.dur.max(0) as f32 * t.speed;
                    if op.code == 0x2e {
                        self.roots.push((t.uid, id, len));
                    } else {
                        self.locks.push((id, op.code, len));
                    }
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
                    snd.res = (l, n);
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
                self.pending.push(Pending { file: Some(0xaf0 + anim as u32), actor: None, name: MAIN, a1: Who::Actor(caster), a2: Who::Actor(target), action: Some(clone), speed, end_cast: None, age: 0.0, skill: None, skill_caster: None, wait_motion: None, num: None, quiet: false, own: false });
                Flow::Next
            }
            _ => {
                // The motion-gated arms (client/combat.rs plays those) are not gaps.
                if !matches!(op.code, 0x05 | 0x28 | 0x20 | 0x59 | 0x2b | 0x80 | 0x4a)
                    && let Some(a) = self.reg.audit.as_mut()
                {
                    *a.sched.entry(op.code).or_default() += 1;
                }
                Flow::Next
            }
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
        // An unattached static generator is stopped as its template, whatever the actors (generators.md D.3).
        let any = self.template_wide(g);
        for i in 0..self.insts.len() {
            let hit = self.insts[i].as_ref().is_some_and(|x| x.src == g && (any || pair_match(x.actors, a1, a2)));
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

    /// FUN_1005e0b0 / FUN_100539e0: unattached (mode 0 / 0xe / 0xf / 0x14) and static (flags 0x400, or op 1's life
    /// i16 @0x22 is 0): ops act on the template, i.e. every instance.
    fn template_wide(&self, g: Ref) -> bool {
        self.reg.libs[g.0].gens.iter().find(|x| x.node == g.1).is_some_and(|x| matches!(x.attach_mode(), 0 | 0xe | 0xf | 0x14) && (x.flags & 0x400 != 0 || x.template().is_some_and(|o| o.i16_at(0x22) == 0)))
    }

    /// FUN_10056a50 / FUN_1018a7a0: every self-starting scheduler of library `l` (under folder `under` when given) starts
    /// on no actors, speed 1.0 (retail steps it at once inside the load; ours at the next tick).
    pub(super) fn start_auto(&mut self, l: usize, under: Option<usize>) {
        let tree = &self.reg.libs[l].tree;
        let inside = |mut i: usize, f: usize| loop {
            match tree.nodes[i].parent {
                Some(p) if p == f => return true,
                Some(p) => i = p,
                None => return false,
            }
        };
        let nodes: Vec<usize> = (0..tree.nodes.len())
            .filter(|&n| tree.nodes[n].kind == kind::SCHEDULER && under.is_none_or(|f| inside(n, f)) && crate::dat::sched::auto_start(self.reg.libs[l].data(n)))
            .collect();
        if !nodes.is_empty() {
            log::debug!("effects: DAT {}: {} schedulers start by themselves", self.reg.libs[l].file_id, nodes.len());
        }
        for n in nodes {
            self.spawn_task((l, n), Who::None, Who::None, None, 1.0, None);
        }
    }

    /// FUN_10055f00 / FUN_10055f40: tasks running scheduler `r` (on one actor pair when given) end now.
    pub(super) fn kill_tasks(&mut self, r: Ref, pair: Option<(Who, Who)>) {
        for t in self.tasks.iter_mut() {
            if t.as_ref().is_some_and(|t| t.res == r && pair.is_none_or(|(a, b)| t.a1 == a && t.a2 == b)) {
                *t = None;
            }
        }
    }

    /// FUN_100abea0: a doll where the actor stands now; FUN_10062d50 moves the clones this task started (clone
    /// +0xf0 == the task) to it, others stay on the actor.
    fn doll(&mut self, w: Who, follow: bool, actors: &dyn Actors, me: usize) -> Who {
        let Who::Actor(id) = w else { return w };
        let Some(m) = actors.matrix(id) else { return w };
        let d = Who::Doll(self.dolls.len());
        let points = (0..DOLL_POINTS).map(|k| actors.point(id, k)).collect();
        self.dolls.push(Doll { of: id, matrix: m, follow, points });
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
    pub(super) fn doll_of(&self, w: Who) -> Who {
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
            Who::Doll(d) => self.dolls.get(d).map(|d| {
                let mut m = d.matrix;
                if d.follow
                    && let Some(a) = actors.matrix(d.of)
                {
                    m.w_axis = a.w_axis;
                }
                (d.of, m)
            }),
            Who::None => None,
        }
    }

    /// An actor slot's placement and attach point `k` in the world (FUN_1003ba20): 0x30 the ground under point 0,
    /// 0x31 / 0x32 the body point 0xd..0x14 nearest the other actor, 0x33 the one nearest the eye, 0x34 the last foot
    /// contact, 0x35 the position, 0x36..0x3c points 0x7e / 0x7f / 0x66..0x6a, others point k.
    fn who_point(&self, w: Who, k: u16, other: Who, eye: Option<Vec3>, off: Vec3, actors: &dyn Actors) -> Option<(Mat4, Vec3)> {
        let (id, m) = self.who_matrix(w, actors)?;
        let local = |k: u16| match w {
            Who::Doll(d) if k < DOLL_POINTS => self.dolls.get(d).and_then(|d| d.points[k as usize]),
            _ => actors.point(id, k),
        };
        // Op 0x7b's offset turns with the actor (TransformCoord by its yaw).
        let off = m.transform_vector3(off);
        let world = |k: u16| local(k).map(|p| m.transform_point3(p) + off);
        let nearest = |to: Vec3| (0xd..=0x14u16).filter_map(world).min_by(|a, b| a.distance_squared(to).total_cmp(&b.distance_squared(to)));
        let p = match k {
            // From 1 above to 6 below; retail's miss is (0, 0, 0), ours keeps the position (our collision is not
            // retail's everywhere).
            0x30 => world(0).and_then(|p| actors.ground_hit(p - Vec3::Y, p + Vec3::Y * 6.0)),
            0x31 | 0x32 => self.who_matrix(other, actors).and_then(|(_, o)| nearest(o.w_axis.truncate())),
            0x33 => eye.and_then(nearest),
            0x34 => actors.foot(id),
            // 0x36..0x3c name skeleton points (FUN_1003abe0).
            0x36 => world(0x7e),
            0x37 => world(0x7f),
            0x38..=0x3c => world(k - 0x38 + 0x66),
            // FUN_100d4bc0's +0x64c, raw: no op-0x7b offset, no drop.
            0x35 => actors.wade(id),
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
        let w = x.attach;
        let pc = ((((w >> 18) & 1) << 6) | ((w >> 4) & 0x3f)) as u16;
        let pt = ((w >> 10) & 0x3f) as u16;
        let [a1, a2] = x.actors;
        // 0x1003c5f0: the mode's actor missing (never linked, or deleted) gives the identity.
        let missing = match x.mode {
            1 | 7 | 9 | 0xb | 0x10 => a1 == Who::None,
            2 | 8 | 0xa | 0xc | 0x11 | 0x13 => a2 == Who::None,
            3..=6 | 0xd | 0x12 | 0x16..=0x1b => a1 == Who::None && a2 == Who::None,
            _ => false,
        };
        if missing {
            return Some((Some(Mat4::IDENTITY), Vec3::ONE, Vec3::ONE, 0.0));
        }
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
        // FUN_10053e60: with gen +0x98 set, emission op 0x7b's offsets A (caster point) / B (target point) reach the
        // two-point modes only.
        let two_point = matches!(x.mode, 3 | 6 | 0xd | 0x10 | 0x11 | 0x12 | 0x16..=0x1b);
        let (off_a, off_b) = match x.streams[1].iter().find(|o| o.code == 0x7b) {
            Some(o) if g.offsets && two_point => (Vec3::new(o.f32(1), o.f32(2), o.f32(3)), Vec3::new(o.f32(4), o.f32(5), o.f32(6))),
            _ => (Vec3::ZERO, Vec3::ZERO),
        };
        let p1 = |k: u16| self.who_point(a1, k, a2, eye, off_a, actors);
        let p2 = |k: u16| self.who_point(a2, k, a1, eye, off_b, actors);
        let mut dist = 0.0;
        // FUN_1003bd90 remaps points 0x36..0x3c, then vtable +0x1d0: a skeleton's point frame Rx Ry Rz T(offset) on
        // its drawn bone, whose rows carry the actor size (FUN_100d49d0 -> FUN_1002a900); a doll's (FUN_10082180) is
        // the generator's last matrix moved by the doll's position: no rotation, the point ignored, so a rebuild adds
        // the position again (as decompiled).
        let remap = |k: u16| match k {
            0x36 => 0x7e,
            0x37 => 0x7f,
            0x38..=0x3c => k - 0x38 + 0x66,
            _ => k,
        };
        let bone_frame = |w: Who, k: u16| match w {
            Who::Actor(id) => {
                let f = actors.matrix(id)? * actors.point_frame(id, remap(k))?;
                let s = actors.scale(id);
                Some(Mat4::from_cols(f.x_axis * s, f.y_axis * s, f.z_axis * s, f.w_axis))
            }
            Who::Doll(_) => {
                let (_, d) = self.who_matrix(w, actors)?;
                let mut o = x.matrix.unwrap_or(Mat4::IDENTITY);
                o.w_axis += d.w_axis.truncate().extend(0.0);
                Some(o)
            }
            _ => None,
        };
        // 0xb / 0xc: RotY(-atan2(B._33, B._31)) of that frame, at the point FUN_1003ba20 resolves (vt+0x1c4, the
        // specials included). A doll's B is the math pool's scratch *(inf: taken as identity)*.
        let heading = |w: Who, k: u16, p: Option<(Mat4, Vec3)>| {
            let b = match w {
                Who::Doll(_) => Some(Mat4::IDENTITY),
                _ => bone_frame(w, k),
            }?;
            let mut r = Mat4::from_rotation_y(-b.z_axis.z.atan2(b.z_axis.x));
            r.w_axis = p.map_or(b.w_axis, |(_, p)| p.extend(1.0));
            Some(r)
        };
        let m = match x.mode {
            9 | 0xa => bone_frame(if x.mode == 9 { a1 } else { a2 }, if x.mode == 9 { pc } else { pt }).or_else(|| if x.mode == 9 { p1(pc) } else { p2(pt) }.map(|(m, p)| at(m, p))),
            0xb => heading(a1, pc, p1(pc)).or_else(|| p1(pc).map(|(m, p)| at(m, p))),
            0xc => heading(a2, pt, p2(pt)).or_else(|| p2(pt).map(|(m, p)| at(m, p))),
            // FUN_1003be10 0x14 / 0x15: the voyage matrix DAT_10631ed0 (identity off a voyage), whole / its rotation.
            0x14 => Some(self.voyage),
            0x15 => Some(rot(self.voyage)),
            1 => p1(pc).map(|(m, p)| at(m, p)),
            2 => p2(pt).map(|(m, p)| at(m, p)),
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
                match (self.who_point(a, pc, o, eye, off_a, actors), self.who_point(a, pt, o, eye, off_b, actors)) {
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
            // FUN_10053e60 on nibble & 3: 0 uniform, bit 0 x and z, bit 1 y.
            match src & 3 {
                0 => Vec3::splat(s),
                b => {
                    let xz = if b & 1 != 0 { s } else { 1.0 };
                    Vec3::new(xz, if b & 2 != 0 { s } else { 1.0 }, xz)
                }
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
        if let (Some(au), Some(x)) = (self.reg.audit.as_mut(), self.insts[i].as_ref()) {
            let w = x.attach;
            let (pc, pt) = (((((w >> 18) & 1) << 6) | ((w >> 4) & 0x3f)) as u16, ((w >> 10) & 0x3f) as u16);
            *au.attach.entry((x.mode, pc, pt)).or_default() += 1;
        }
        let last = self.last_frame;
        let placed = self.attach_matrix(i, last.as_ref(), actors);
        if placed.as_ref().is_none_or(|p| p.0.is_none())
            && let (Some(au), Some(x)) = (self.reg.audit.as_mut(), self.insts[i].as_ref())
            && x.mode != 0
        {
            *au.no_attach.entry(x.mode).or_default() += 1;
        }
        if let Some((m, a, b, d)) = placed
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

    /// Generator ops 0x0e / 0x0f / 0x10 (FUN_100496f0): the attachment rebuilt (FUN_10053e60), then turned about X / Y
    /// / Z at its origin by keyframe 4cc u32 @8 at t over the element plus generator life (op 1's i16 @0x22),
    /// half-turns wrapped to +-pi.
    pub(super) fn spin(&mut self, i: usize, op: &crate::dat::generator::Op, f: &Frame, actors: &dyn Actors) {
        let Some((Some(m), a, b, d)) = self.attach_matrix(i, Some(f), actors) else { return };
        let Some(x) = self.insts[i].as_ref() else { return };
        let l = x.streams[1].first().map_or(0.0, |o| o.i16_at(0x22) as f32);
        let t = if l + x.life > 0.0 { (1.0 - (l + x.life - x.elapsed) / (l + x.life)).min(1.0) } else { 0.0 };
        let Some(v) = self.reg.resolve(x.src.0, x.src.1, kind::KEYFRAME, op.u32(2)).and_then(|r| self.reg.keyframe(r)).map(|k| k.sample(t, op.u32(3) & 0xf != 0)) else { return };
        let mut ang = (v % 2.0) * 3.141592;
        if ang > 3.1415 {
            ang -= 2.0 * 3.1415;
        } else if ang < -3.1415 {
            ang += 2.0 * 3.1415;
        }
        let r = match op.code {
            0x0e => Mat4::from_rotation_x(ang),
            0x0f => Mat4::from_rotation_y(ang),
            _ => Mat4::from_rotation_z(ang),
        };
        // D3D's A x Rot with the translation set aside: the turn follows the attach rotation.
        let mut rot = m;
        rot.w_axis = glam::Vec4::W;
        let mut out = r * rot;
        out.w_axis = m.w_axis;
        let Some(x) = self.insts[i].as_mut() else { return };
        x.matrix = Some(out);
        x.scale_a = a;
        x.scale_b = b;
        x.dist = d;
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
    /// FUN_100539e0: flags 0x400, or op 1's life i16 @0x22 is 0.
    fn static_gen(&self, g: Ref) -> bool {
        self.reg.libs[g.0].gens.iter().find(|x| x.node == g.1).is_some_and(|x| x.flags & 0x400 != 0 || x.template().is_some_and(|o| o.i16_at(0x22) == 0))
    }

    /// FUN_10085a50 (hide, once): the actor's non-static clones go, its static ones stop (none live: held here) or
    /// hide their first live element, its tasks pause. FUN_10085a10 (show): undone, and the model's generators start
    /// again (FUN_10085060 replaces their clones).
    fn model_hide(&mut self, actors: &dyn Actors) {
        let mut ids: Vec<u32> = actors.models().into_iter().map(|m| m.0).collect();
        ids.extend(self.hidden_actors.iter().filter(|id| !ids.contains(id)).copied().collect::<Vec<_>>());
        for id in ids {
            let hide = actors.hidden(id);
            if hide == self.hidden_actors.contains(&id) {
                continue;
            }
            let who = Who::Actor(id);
            log::debug!("effects: model of {id:#x} {}", if hide { "hidden" } else { "shown" });
            for i in 0..self.insts.len() {
                let Some(x) = self.insts[i].as_ref().filter(|x| x.actors.contains(&who) && x.driven.is_none()) else { continue };
                let (src, first) = (x.src, x.live.first().copied());
                if hide && !self.static_gen(src) {
                    self.kill(i);
                    continue;
                }
                match first {
                    Some(e) => {
                        if let Some(Some(c)) = self.elems.get_mut(e as usize) {
                            c.hidden = hide;
                        }
                    }
                    None => {
                        if let Some(x) = self.insts[i].as_mut() {
                            x.held = hide;
                            if !hide {
                                (x.elapsed, x.countdown, x.emitted, x.stopping, x.finished) = (0.0, 0.0, 0, false, false);
                            }
                        }
                    }
                }
            }
            for t in self.tasks.iter_mut().flatten().filter(|t| t.a1 == who || t.a2 == who) {
                t.paused = if hide { t.paused + 1 } else { t.paused.saturating_sub(1) };
            }
            if hide {
                self.hidden_actors.insert(id);
            } else {
                self.hidden_actors.remove(&id);
                self.models.remove(&id);
                for i in 0..self.insts.len() {
                    if self.insts[i].as_ref().is_some_and(|x| x.owner == Owner::Actor(id)) {
                        self.kill(i);
                    }
                }
            }
        }
    }

    pub(super) fn model_generators(&mut self, actors: &dyn Actors) {
        self.model_hide(actors);
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

    /// Actors whose attach points the live effects read: the client poses their bones (FUN_100d4560 reads the
    /// drawn pose).
    pub fn posed_actors(&self) -> std::collections::HashSet<u32> {
        let insts = self.insts.iter().flatten().filter(|x| x.mode != 0).flat_map(|x| x.actors);
        let tasks = self.tasks.iter().flatten().flat_map(|t| [t.a1, t.a2]);
        let pending = self.pending.iter().flat_map(|p| [p.a1, p.a2]);
        let trails = self.trails.iter().flatten().filter(|t| t.placing()).map(|t| t.actor);
        insts.chain(tasks).chain(pending).filter_map(|w| self.doll_of(w).id()).chain(self.dolls.iter().map(|d| d.of)).chain(trails).collect()
    }

    pub fn task_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_some()).count()
    }

    /// Starts still waiting for their files (fx-sweep).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Files being read in the background (fx-sweep waits for them in real time, not in ticks).
    pub fn loading_count(&self) -> usize {
        self.loading.iter().filter(|f| self.reg.lib_of(**f).is_none() && !self.failed.contains(f)).count()
    }

    /// fx-sweep between runs: every task, pending start and generator gone.
    /// The shine drives' frame (FUN_10060680, FUN_1005e870): a drive whose task is gone ends there (U / V snap to the
    /// target, the overlay goes if still its own, the colour stays where it was).
    fn step_shine(&mut self, dt: f32, live: &[u64]) {
        use crate::fx::ShineKind;
        for mut d in std::mem::take(&mut self.shine_drives) {
            d.left -= dt;
            let dead = !live.contains(&d.owner);
            let done = d.left < 0.0 || dead;
            let s = self.shine.entry(d.actor).or_default();
            match d.kind {
                ShineKind::Overlay => {
                    if done && s.tex.is_some_and(|(_, o)| o == d.id) {
                        s.tex = None;
                    }
                }
                ShineKind::Uv { v, start, target, total } => {
                    let x = if done || total <= 0.0 { target % 2.0 } else { (start + (target - start) * (1.0 - d.left / total)) % 2.0 };
                    if v {
                        s.v = x;
                    } else {
                        s.u = x;
                    }
                }
                ShineKind::Tf { start, target, total } if !dead => {
                    let k = if d.left < 0.0 || total <= 0.0 { 1.0 } else { 1.0 - d.left / total };
                    let (a, b) = (start.to_le_bytes(), target.to_le_bytes());
                    s.tf = u32::from_le_bytes(std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * k) as u8));
                }
                ShineKind::Tf { .. } => {}
                // FUN_100612b0 / FUN_100611e0.
                ShineKind::Refract { start, target, total } => {
                    if done || total <= 0.0 {
                        s.level = target;
                        if target == 0.0 {
                            s.refract = false;
                        }
                    } else {
                        s.level = start + (target - start) * (1.0 - d.left / total);
                    }
                }
            }
            if !done {
                self.shine_drives.push(d);
            }
        }
    }

    /// FUN_1005f6c0's walk: a CMoLockColorDriveTask of another scheduler on the actor.
    fn color_locked(&self, id: u32, owner: u64) -> bool {
        self.color_guards.iter().any(|g| g.1 == id && g.0 != owner)
    }

    /// FUN_1005f6c0 channel 0: the actor's running drive is deleted first (its destructor: 0xa0's lands on its
    /// target, others stay); a locked one never writes (0xa0's destructor still lands).
    fn color_drive(&mut self, id: u32, owner: u64, target: u32, ticks: f32, snap: bool) {
        use crate::fx::ColorOp;
        if let Some(i) = self.color_drives.iter().position(|d| d.actor == id) {
            let old = self.color_drives.remove(i);
            self.fades.push(if old.snap { ColorOp::Snap(id, old.target) } else { ColorOp::Freeze(id) });
        }
        if self.color_locked(id, owner) {
            if snap {
                self.fades.push(ColorOp::Snap(id, target));
            }
            return;
        }
        self.fades.push(ColorOp::Drive { actor: id, argb: target, ticks });
        self.color_drives.push(crate::fx::ColorDrive { actor: id, owner, left: ticks, target, snap });
    }

    /// Actors with the refraction pass on, and its level.
    pub fn refractions(&self) -> Vec<(u32, f32)> {
        self.shine.iter().filter(|(_, s)| s.refract).map(|(id, s)| (*id, s.level)).collect()
    }

    /// Actors with the shine overlay on: (actor, its image, U, V, TFACTOR as ARGB).
    pub fn shines(&self) -> Vec<(u32, Ref, f32, f32, u32)> {
        self.shine.iter().filter_map(|(id, s)| s.tex.map(|(r, _)| (*id, r, s.u, s.v, s.tf))).collect()
    }

    pub fn reset_runs(&mut self) {
        self.trails.clear();
        self.shine.clear();
        self.shine_drives.clear();
        self.pending.clear();
        self.tasks.iter_mut().for_each(|t| *t = None);
        for i in 0..self.insts.len() {
            if self.insts[i].is_some() {
                self.kill(i);
            }
        }
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

/// FUN_100ac560's class of one side (the target's is shifted 4 up): 1 the player or its pet, 2 a monster (+0x120 bit
/// 13); else 4 in our party, or a dynamic entity whose master is, and 8 when not itself listed (so such a pet or
/// trust is 0xc).
pub fn filter_class(mine: bool, monster: bool, listed: bool, master_listed: bool) -> u8 {
    let c = mine as u8 | (monster as u8) << 1;
    if c != 0 {
        return c;
    }
    (if listed || master_listed { 4 } else { 0 }) | if listed { 0 } else { 8 }
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

    /// Op 0x7a (86913's layout): the orc stands at the Hume's point + (2, 0, 0), facing back, for dur ticks.
    #[test]
    fn pin_holds_actor1() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let mut pin = vec![0u8; 0x30];
        pin[..8].copy_from_slice(&[0x7a, 0x0c, 0, 0, 0, 0, 30, 0]);
        pin[0x10..0x14].copy_from_slice(&2.0f32.to_le_bytes());
        pin[0x1c..0x20].copy_from_slice(&1.0f32.to_le_bytes());
        let ops = vec![SOp { code: 0x7a, wait: 0, dur: 30, raw: pin }, SOp { code: 0x01, wait: 60, dur: 0, raw: vec![1, 2, 0, 0, 60, 0, 0, 0] }];
        fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(ORC), Who::Actor(HUME), None, 1.0)));
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        fx.tick(&f, &Two);
        let (p, yaw) = fx.pinned[&ORC];
        assert!(p.distance(Vec3::new(2.0, -1.0, 0.0)) < 1e-5, "{p}");
        assert!((yaw.unwrap() - std::f32::consts::PI).abs() < 1e-5, "{yaw:?}");
        for _ in 0..31 {
            fx.tick(&f, &Two);
        }
        assert!(!fx.pinned.contains_key(&ORC));
    }

    /// Op 0x2e (dur 30): the root holds while its counter is not below 0, so the restart comes on the 31st frame;
    /// a scheduler killed at frame 10 ends it there.
    #[test]
    fn root_end_restarts_status_motion() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let run = |kill_at: Option<usize>| -> Vec<(usize, u32, bool)> {
            let mut fx = Effects::new();
            fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
            let mut root = vec![0u8; 16];
            root[..8].copy_from_slice(&[0x2e, 4, 0, 0, 0, 0, 30, 0]);
            let ops = vec![SOp { code: 0x2e, wait: 0, dur: 30, raw: root }, SOp { code: 0x01, wait: 200, dur: 0, raw: vec![1, 2, 0, 0, 200, 0, 0, 0] }];
            fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(HUME), Who::Actor(ORC), None, 1.0)));
            let mut out = Vec::new();
            for frame in 1..=40 {
                if kill_at == Some(frame) {
                    fx.tasks.clear();
                }
                fx.tick(&f, &Two);
                out.extend(fx.status_restarts.drain(..).map(|id| (frame, id, fx.rooted.contains(&HUME))));
                if frame == 5 {
                    assert!(fx.rooted.contains(&HUME));
                }
            }
            out
        };
        assert_eq!(run(None), [(31, HUME, false)]);
        assert_eq!(run(Some(10)), [(10, HUME, false)]);
    }

    fn color_fx() -> Option<Effects> {
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        let mut fx = Effects::new();
        fx.set_global(Library::load(0, index.read(0).ok()?).ok()?);
        Some(fx)
    }

    /// One op: code, wait, dur, u32 @8.
    fn sop(code: u8, wait: i16, dur: i16, v: u32) -> SOp {
        let mut raw = vec![0u8; 12];
        raw[0] = code;
        raw[1] = 3;
        raw[4..6].copy_from_slice(&wait.to_le_bytes());
        raw[6..8].copy_from_slice(&dur.to_le_bytes());
        raw[8..12].copy_from_slice(&v.to_le_bytes());
        SOp { code, wait, dur, raw }
    }

    fn color_task(fx: &mut Effects, ops: Vec<SOp>) {
        fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(ORC), Who::Actor(HUME), None, 1.0)));
    }

    fn color_tick(fx: &mut Effects) -> Vec<crate::fx::ColorOp> {
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        fx.tick(&f, &Two);
        std::mem::take(&mut fx.fades)
    }

    /// FUN_1005f6c0: another scheduler's op 0x88 on the actor keeps a drive from writing; the lock's own passes.
    #[test]
    fn color_lock() {
        use crate::fx::ColorOp::*;
        let Some(mut fx) = color_fx() else { return };
        color_task(&mut fx, vec![sop(0x88, 0, 100, 0), sop(0x01, 200, 0, 0), sop(0, 0, 0, 0)]);
        color_tick(&mut fx);
        color_task(&mut fx, vec![sop(0x29, 0, 30, 0x8000_0000), sop(0x01, 100, 0, 0), sop(0, 0, 0, 0)]);
        assert_eq!(color_tick(&mut fx), []);
        let Some(mut fx) = color_fx() else { return };
        color_task(&mut fx, vec![sop(0x88, 0, 100, 0), sop(0x29, 0, 30, 0x8000_0000), sop(0x01, 100, 0, 0), sop(0, 0, 0, 0)]);
        assert_eq!(color_tick(&mut fx), [Drive { actor: ORC, argb: 0x8000_0000, ticks: 30.0 }]);
    }

    /// A drive whose scheduler ends first stops where it is; 0xa0 lands on its target, at once when locked out.
    #[test]
    fn color_drive_ends() {
        use crate::fx::ColorOp::*;
        let Some(mut fx) = color_fx() else { return };
        color_task(&mut fx, vec![sop(0x29, 10, 60, 0x8000_0000), sop(0, 0, 0, 0)]);
        let ops: Vec<_> = (0..20).flat_map(|_| color_tick(&mut fx)).collect();
        assert_eq!(ops, [Drive { actor: ORC, argb: 0x8000_0000, ticks: 60.0 }, Freeze(ORC)]);
        color_task(&mut fx, vec![sop(0xa0, 10, 60, 0x8000_0000), sop(0, 0, 0, 0)]);
        let ops: Vec<_> = (0..20).flat_map(|_| color_tick(&mut fx)).collect();
        assert_eq!(ops, [Drive { actor: ORC, argb: 0x8000_0000, ticks: 60.0 }, Snap(ORC, 0x8000_0000)]);
        color_task(&mut fx, vec![sop(0x88, 0, 100, 0), sop(0x01, 200, 0, 0), sop(0, 0, 0, 0)]);
        color_tick(&mut fx);
        color_task(&mut fx, vec![sop(0xa0, 10, 60, 0x4000_0000), sop(0, 0, 0, 0)]);
        assert_eq!(color_tick(&mut fx), [Snap(ORC, 0x4000_0000)]);
    }

    /// A locked-out drive still deletes the running one, which stops where it is.
    #[test]
    fn blocked_drive_stops_running_fade() {
        use crate::fx::ColorOp::*;
        let Some(mut fx) = color_fx() else { return };
        color_task(&mut fx, vec![sop(0x29, 0, 60, 0x8000_0000), sop(0x01, 200, 0, 0), sop(0, 0, 0, 0)]);
        assert_eq!(color_tick(&mut fx), [Drive { actor: ORC, argb: 0x8000_0000, ticks: 60.0 }]);
        color_task(&mut fx, vec![sop(0x88, 0, 100, 0), sop(0x01, 200, 0, 0), sop(0, 0, 0, 0)]);
        color_tick(&mut fx);
        color_task(&mut fx, vec![sop(0x29, 0, 30, 0x0080_8080), sop(0x01, 100, 0, 0), sop(0, 0, 0, 0)]);
        assert_eq!(color_tick(&mut fx), [Freeze(ORC)]);
    }

    /// FUN_10053a00's match: no actor1 takes every clone; else actor1's clones with actor2 none or the task's.
    #[test]
    fn morph_pair_match() {
        let (h, o) = (Who::Actor(HUME), Who::Actor(ORC));
        assert!(pair_match([o, Who::None], o, h) && pair_match([o, h], o, h) && pair_match([h, o], Who::None, h));
        assert!(!pair_match([o, o], o, h) && !pair_match([h, Who::None], o, h));
    }

    /// File 110 `pks1`'s op 0x3f: the zone's static `yaa0` (unattached, one element) morphs into `idy0` whatever the
    /// task's actors; with no `yaa0` running nothing is made.
    #[test]
    fn morph_static_world_in_place() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let (Ok(zone), Ok(dat0)) = (index.read(110), index.read(0)) else { return };
        let index = std::sync::Arc::new(index);
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let morph = |fx: &mut Effects, zl: usize| {
            let node = fx.reg.libs[zl].find_global(kind::SCHEDULER, u32::from_le_bytes(*b"pks1")).unwrap();
            let mut raw = vec![0x3f, 7, 0, 0, 0, 0, 0, 0];
            raw.extend_from_slice(b"yaa0\0\0\0\0idy0\0\0\0\0\0\0\0\0");
            let ops = vec![SOp { code: 0x3f, wait: 0, dur: 0, raw }, SOp { code: 0x01, wait: 100, dur: 0, raw: vec![1, 2, 0, 0, 100, 0, 0, 0] }];
            fx.tasks.push(Some(Task::new((zl, node), Rc::new(ops), Who::Actor(HUME), Who::Actor(ORC), None, 1.0)));
        };
        let live = |fx: &Effects, n: &[u8; 4]| -> Vec<(usize, Vec<u32>, Option<u32>)> {
            fx.insts.iter().enumerate().filter_map(|(i, x)| x.as_ref().filter(|x| x.name == *n).map(|x| (i, x.live.clone(), x.parent_elem))).collect()
        };
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, dat0.clone()).unwrap());
        let zl = fx.load_zone(Library::load(110, zone.clone()).unwrap());
        let node = fx.reg.libs[zl].find_global(kind::SCHEDULER, u32::from_le_bytes(*b"pks1")).unwrap();
        let ga = fx.reg.resolve(zl, node, kind::GENERATOR, u32::from_le_bytes(*b"yaa0")).unwrap();
        let of_a = |fx: &Effects| fx.insts.iter().flatten().filter(|x| x.src == ga).flat_map(|x| x.live.clone()).collect::<Vec<u32>>();
        // The camera on yaa0's emission point (its op 0x0a / 0x15 gates).
        let p = fx.insts.iter().flatten().find(|x| x.src == ga).and_then(|x| x.streams[1].first().map(|o| Vec3::new(o.f32(5), o.f32(6), o.f32(7)))).unwrap();
        let eye = p + Vec3::new(0.0, -2.0, 10.0);
        let f = Frame { eye, view: glam::camera::rh::view::look_at_mat4(eye, p, Vec3::NEG_Y), ..f };
        let mut elem = None;
        for _ in 0..600 {
            fx.tick(&f, &Two);
            elem = of_a(&fx).first().copied();
            if elem.is_some() {
                break;
            }
        }
        let Some(e) = elem else { panic!("pks1's yaa0 never emits") };
        morph(&mut fx, zl);
        fx.tick(&f, &Two);
        assert!(of_a(&fx).is_empty(), "{:?}", of_a(&fx));
        assert!(live(&fx, b"idy0").iter().any(|x| x.2 == Some(e)), "{:?}", live(&fx, b"idy0"));
        // No yaa0 running: no idy0.
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, dat0).unwrap());
        let zl = fx.load_zone(Library::load(110, zone).unwrap());
        for i in 0..fx.insts.len() {
            fx.kill(i);
        }
        morph(&mut fx, zl);
        fx.tick(&f, &Two);
        assert!(live(&fx, b"idy0").is_empty());
    }

    /// An actor with these model files whose model can be hidden.
    struct Hideable {
        files: Vec<u32>,
        hidden: std::cell::Cell<bool>,
    }

    impl Actors for Hideable {
        fn matrix(&self, id: u32) -> Option<Mat4> {
            Two.matrix(id)
        }
        fn point(&self, id: u32, k: u16) -> Option<Vec3> {
            Two.point(id, k)
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, _: u32) -> (Vec<u32>, Vec<u32>) {
            (Vec::new(), self.files.clone())
        }
        fn hidden(&self, id: u32) -> bool {
            id == ORC && self.hidden.get()
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            vec![(ORC, self.files.clone())]
        }
    }

    fn gen_insts(fx: &Effects) -> Vec<(String, f32, bool, [Who; 2])> {
        fx.insts.iter().flatten().filter(|x| x.owner == Owner::Actor(ORC)).map(|x| (String::from_utf8_lossy(&x.name).into_owned(), x.elapsed, x.held, x.actors)).collect()
    }

    /// FUN_10085a50 / FUN_10085a10: model 1526's looping `g000` goes while the model is hidden and starts fresh when
    /// it shows; the actor's tasks pause meanwhile.
    #[test]
    fn model_gens_unhide() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        fx.preload(&[1526]);
        while fx.reg.lib_of(1526).is_none() {
            fx.poll_loads();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let a = Hideable { files: vec![1526], hidden: std::cell::Cell::new(false) };
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        for _ in 0..30 {
            fx.tick(&f, &a);
        }
        let before = gen_insts(&fx);
        assert!(before.iter().any(|x| x.0 == "g000"), "{before:?}");
        let ops = vec![SOp { code: 0x01, wait: 500, dur: 0, raw: vec![1, 2, 0, 0, 0xf4, 1, 0, 0] }];
        fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(ORC), Who::Actor(HUME), None, 1.0)));
        a.hidden.set(true);
        fx.tick(&f, &a);
        let hidden = gen_insts(&fx);
        assert!(hidden.iter().all(|x| x.0 != "g000" || x.2), "{hidden:?}");
        assert!(fx.tasks.iter().flatten().any(|t| t.paused == 1));
        for _ in 0..10 {
            fx.tick(&f, &a);
        }
        a.hidden.set(false);
        fx.tick(&f, &a);
        let shown = gen_insts(&fx);
        let g = shown.iter().filter(|x| x.0 == "g000").collect::<Vec<_>>();
        assert!(g.len() == 1 && g[0].1 <= 1.0 && !g[0].2, "{shown:?}");
        assert!(fx.tasks.iter().flatten().all(|t| t.paused == 0));
    }

    /// FUN_100d62c0: a part file's looping generators start on the actor too (Hume M main 8700: `gm01..gm06`).
    #[test]
    fn part_file_gens() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let a = Hideable { files: vec![7072, 8700], hidden: std::cell::Cell::new(false) };
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        for _ in 0..200 {
            fx.poll_loads();
            fx.tick(&f, &a);
            if fx.reg.lib_of(8700).is_some() && !gen_insts(&fx).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut names: Vec<String> = gen_insts(&fx).into_iter().map(|x| x.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names, ["gm01", "gm02", "gm03", "gm04", "gm05", "gm06"]);
        assert!(gen_insts(&fx).iter().all(|x| x.3 == [Who::Actor(ORC), Who::None]));
    }

    /// A posed stand-in: point k's frame RotY(0.5) at height k / 100 above its bone, the actor at x 1, size 2.
    struct Skel;

    impl Actors for Skel {
        fn matrix(&self, _: u32) -> Option<Mat4> {
            Some(Mat4::from_translation(Vec3::X))
        }
        fn point(&self, _: u32, k: u16) -> Option<Vec3> {
            Some(Vec3::new(0.0, -(k as f32) / 100.0, 0.0))
        }
        fn scale(&self, _: u32) -> f32 {
            2.0
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
        fn point_frame(&self, _: u32, k: u16) -> Option<Mat4> {
            Some(Mat4::from_translation(Vec3::new(0.0, -(k as f32) / 100.0, 0.0)) * Mat4::from_rotation_y(0.5))
        }
    }

    /// The first generator of a file with this attach mode (and caster point), started on HUME / ORC and attached.
    fn attached(file: u32, mode: u8, pc: Option<u16>, actors: &dyn Actors) -> Option<(Mat4, Vec3)> {
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()?;
        let mut fx = Effects::new();
        let lib = Library::load(file, index.read(file).ok()?).ok()?;
        let node = lib.gens.iter().find(|g| g.attach_mode() == mode && pc.is_none_or(|pc| ((((g.attach >> 18) & 1) << 6) | ((g.attach >> 4) & 0x3f)) as u16 == pc))?.node;
        let l = fx.reg.add(lib);
        let i = fx.start((l, node), super::super::Owner::Zone)?;
        fx.insts[i].as_mut()?.actors = [Who::Actor(HUME), Who::Actor(ORC)];
        fx.attach_now(i, actors);
        let x = fx.insts[i].as_ref()?;
        Some((x.matrix?, x.scale_a))
    }

    /// Mode 9 with point 0x36 (Bisection, 87053): FUN_1003bd90 takes point 0x7e, and the frame's rows keep the size 2.
    #[test]
    fn attach_bone_frame() {
        let Some((m, _)) = attached(87053, 9, Some(0x36), &Skel) else { return };
        let want = Mat4::from_translation(Vec3::new(1.0, -1.26, 0.0)) * Mat4::from_rotation_y(0.5);
        let want = Mat4::from_cols(want.x_axis * 2.0, want.y_axis * 2.0, want.z_axis * 2.0, want.w_axis);
        assert!(m.abs_diff_eq(want, 1e-5), "{m} != {want}");
    }

    /// Mode 0x14 (56775 alex): the voyage matrix, identity off a ship's voyage.
    #[test]
    fn attach_voyage_identity() {
        let Some((m, a)) = attached(56775, 0x14, None, &Two) else { return };
        assert_eq!((m, a), (Mat4::IDENTITY, Vec3::ONE));
    }

    /// Ops 0x48 / 0x49's drive: linear from the start, the target at the end; to 0 (or the owner gone) turns it off.
    #[test]
    fn refract_drive() {
        use crate::fx::{ShineDrive, ShineKind};
        assert_eq!(crate::fx::Shine::default().level, 0.1);
        let mut fx = Effects::new();
        let drive = |fx: &mut Effects, start: f32, target: f32, total: f32| {
            let s = fx.shine.entry(7).or_default();
            s.refract = true;
            s.level = start;
            fx.shine_drives.push(ShineDrive { id: 1, owner: 1, actor: 7, left: total, kind: ShineKind::Refract { start, target, total } });
        };
        drive(&mut fx, 0.1, 0.12, 28.0);
        fx.step_shine(14.0, &[1]);
        assert!((fx.shine[&7].level - 0.11).abs() < 1e-6 && fx.shine[&7].refract);
        fx.step_shine(15.0, &[1]);
        assert!(fx.shine[&7].level == 0.12 && fx.shine[&7].refract && fx.shine_drives.is_empty());
        drive(&mut fx, 0.12, 0.0, 25.0);
        fx.step_shine(10.0, &[]);
        assert!(fx.shine[&7].level == 0.0 && !fx.shine[&7].refract);
        drive(&mut fx, 0.3, 0.0, 0.0);
        fx.step_shine(1.0, &[1]);
        assert!(fx.shine[&7].level == 0.0 && !fx.shine[&7].refract);
        assert_eq!(fx.refractions(), vec![]);
    }

    /// Op 0xac: actor1's death pick, u32 @8 & 3, at once.
    #[test]
    fn death_pick_op() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        for (v, want) in [(1u8, 1u8), (6, 2)] {
            let ops = vec![SOp { code: 0xac, wait: 0, dur: 0, raw: vec![0xac, 3, 0, 0, 0, 0, 0, 0, v, 0, 0, 0] }];
            fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(ORC), Who::Actor(HUME), None, 1.0)));
            fx.tick(&f, &Two);
            assert_eq!(std::mem::take(&mut fx.death_picks), vec![(ORC, want)]);
        }
        // Op 0xb7 (Valaineral's init): the race code, at once.
        let ops = vec![SOp { code: 0xb7, wait: 0, dur: 0, raw: vec![0xb7, 3, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0] }];
        fx.tasks.push(Some(Task::new((fx.reg.global.unwrap(), 0), Rc::new(ops), Who::Actor(ORC), Who::Actor(HUME), None, 1.0)));
        fx.tick(&f, &Two);
        assert_eq!(std::mem::take(&mut fx.races), vec![(ORC, 3)]);
    }

    /// DAT 50713 (Core Meltdown) main picks 1 at tick 0; the Ghrah model 2461's `1ead` puts it back to 0 at tick 262.
    #[test]
    fn death_pick_data() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let find = |file: u32, name: &[u8; 4]| -> Option<(u32, u32)> {
            let bytes = index.read(file).ok()?;
            let lib = Library::load(file, bytes.clone()).ok()?;
            let n = lib.find_global(crate::dat::generator::kind::SCHEDULER, u32::from_le_bytes(*name))?;
            let ops = crate::dat::sched::ops(&bytes[lib.tree.nodes[n].data.clone()]);
            let mut tick = 0u32;
            // An op's wait runs after it.
            for o in ops {
                if o.code == 0xac {
                    return Some((tick, u32::from_le_bytes(o.raw[8..12].try_into().ok()?)));
                }
                tick += o.wait as u32;
            }
            None
        };
        assert_eq!(find(50713, b"main"), Some((0, 1)));
        assert_eq!(find(2461, b"1ead"), Some((262, 0)));
        assert_eq!(find(2461, b"dead"), None);
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

    /// Walker wading: its wade point (3, -0.5, -2).
    struct Wader;

    impl Actors for Wader {
        fn matrix(&self, id: u32) -> Option<Mat4> {
            Walker.matrix(id)
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
        fn wade(&self, _: u32) -> Option<Vec3> {
            Some(Vec3::new(3.0, -0.5, -2.0))
        }
    }

    /// Point 0x35 is the wade point as stored, without op 0x7b's offset; none stored: the position.
    #[test]
    fn wade_point() {
        let fx = Effects::new();
        let (_, p) = fx.who_point(Who::Actor(7), 0x35, Who::None, None, Vec3::new(0.1, 0.0, 0.0), &Wader).unwrap();
        assert_eq!(p, Vec3::new(3.0, -0.5, -2.0));
        let (_, p) = fx.who_point(Who::Actor(7), 0x35, Who::None, None, Vec3::ZERO, &Walker).unwrap();
        assert_eq!(p, Vec3::new(3.0, 0.0, -2.0));
    }

    /// FUN_100d64d0 / FUN_100d6550 in East Ronfaure (DAT 201): with the footstep folder known, wading starts one
    /// endless `i900` on the actor; leaving the water ends it.
    #[test]
    fn wading_ripple() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let mut fx = Effects::new();
        fx.set_keys(std::sync::Arc::new(crate::dat::zone::Keys::from_ffximain(&dll).unwrap()));
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        fx.load_zone(Library::load(201, index.read(201).unwrap()).unwrap());
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let ripples = |fx: &Effects| fx.insts.iter().flatten().filter(|x| &x.name == b"i900").count();
        fx.wade(7, false, true, Some(false));
        fx.wade(7, true, true, Some(false));
        fx.wade(7, true, true, Some(false));
        assert_eq!(ripples(&fx), 1);
        for _ in 0..600 {
            fx.tick(&f, &Wader);
        }
        assert_eq!(ripples(&fx), 1);
        fx.wade(7, false, true, Some(false));
        fx.tick(&f, &Wader);
        assert_eq!(ripples(&fx), 0);
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
        assert_eq!(skill_files(3, 0x105, 1, 1), Some((61456, Some(61472))));
        assert_eq!(skill_files(0xe, 3, 2, 2), Some((58428, Some(58684))));
        assert_eq!(skill_files(0xf, 1, 8, 1), Some((97252, Some(97764))));
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

    /// HumeWs with the orc's model still loading.
    struct OrcLoading;

    impl Actors for OrcLoading {
        fn matrix(&self, id: u32) -> Option<Mat4> {
            HumeWs.matrix(id)
        }
        fn point(&self, id: u32, k: u16) -> Option<Vec3> {
            HumeWs.point(id, k)
        }
        fn scale(&self, _: u32) -> f32 {
            1.0
        }
        fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
            HumeWs.files(id)
        }
        fn hidden(&self, _: u32) -> bool {
            false
        }
        fn models(&self) -> Vec<(u32, Vec<u32>)> {
            Vec::new()
        }
        fn pc(&self, id: u32) -> Option<(u8, i8)> {
            HumeWs.pc(id)
        }
        fn ready(&self, id: u32) -> Option<bool> {
            Some(id != ORC)
        }
    }

    /// FUN_100a9500: a target whose model is not in stops the skill's `main` (no task), and its lines print.
    #[test]
    fn main_needs_ready_targets() {
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
        for _ in 0..5 {
            fx.tick(&f, &OrcLoading);
        }
        assert!(fx.tasks.iter().flatten().all(|t| fx.reg.libs[t.res.0].tree.nodes[t.res.1].name != *b"main"));
        assert_eq!(fx.action_prints.len(), 1);
    }

    /// 0x100a8db0: Combo's skill file goes when its `main` (waits summing to 140 ticks) ends, once; a second Combo at
    /// frame 60 keeps it until that one's `main` ends too.
    #[test]
    fn skill_files_dropped_at_main_end() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let index = std::sync::Arc::new(index);
        let f = Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
        let combo = Action { actor: HUME, category: 3, arg: 1, info: 0, targets: vec![ActionTarget { id: ORC, results: vec![ActionResult { animation: 16, message: 185, value: 40, ..Default::default() }] }] };
        let run = |second: Option<usize>| -> Vec<usize> {
            let mut fx = Effects::new();
            fx.set_index(index.clone());
            fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
            fx.preload(&[8392, 9672, 7072, 1300, 33243]);
            while [8392, 9672, 7072, 1300, 33243].iter().any(|f| fx.reg.lib_of(*f).is_none()) {
                fx.poll_loads();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            fx.on_action(&combo, None);
            let mut done = Vec::new();
            for frame in 1..=400 {
                if second == Some(frame) {
                    fx.on_action(&combo, None);
                }
                fx.tick(&f, &HumeWs);
                for c in fx.skill_files_done.drain(..) {
                    assert_eq!(c, HUME);
                    done.push(frame);
                }
            }
            done
        };
        let once = run(None);
        assert!(once.len() == 1 && (139..=142).contains(&once[0]), "{once:?}");
        let twice = run(Some(60));
        assert!(twice.len() == 1 && (199..=202).contains(&twice[0]), "{twice:?}");
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

    /// FUN_100ac560: a party member is 4, a stranger 8, our trust (master in the party) 0xc; the player and monsters
    /// stop there.
    #[test]
    fn filter_classes() {
        use super::filter_class;
        assert_eq!(filter_class(true, false, true, false), 1);
        assert_eq!(filter_class(false, true, false, true), 2);
        assert_eq!(filter_class(false, false, true, false), 4);
        assert_eq!(filter_class(false, false, false, false), 8);
        assert_eq!(filter_class(false, false, false, true), 0xc);
        // Hiding "party -> monster" (bit 10: caster row 2, target column 1) hides our trust's hits too.
        assert!(!effects_allowed(1 << 10, false, 0xc | 2 << 4));
        assert!(effects_allowed(1 << 10, false, 8 | 2 << 4));
    }
}

#[cfg(test)]
mod auto_tests {
    use super::*;
    use crate::dat::generator::fourcc;

    fn index() -> Option<std::sync::Arc<crate::dat::DatIndex>> {
        crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok().map(std::sync::Arc::new)
    }

    /// The scheduler named `name` whose folder path ends with `path`.
    fn sched(lib: &Library, path: &[&str], name: &str) -> usize {
        let want: Vec<[u8; 4]> = path.iter().map(|p| fourcc(p)).collect();
        lib.tree.of_kind(kind::SCHEDULER).find(|&n| lib.tree.nodes[n].name == fourcc(name) && lib.tree.path(n).ends_with(&want)).unwrap()
    }

    fn frame(dayfrac: f32) -> Frame {
        Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac, view: Mat4::IDENTITY, view_rot: Mat4::IDENTITY, eye: Vec3::new(0.0, -2.0, -10.0), draw_distance: 2000.0, area_light: Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 }
    }

    /// FUN_10056e10 / e80 / ae0 on the data: the end loops, the windows, op 0xa2's exception.
    #[test]
    fn end_stream_and_auto() {
        let Some(index) = index() else { return };
        let (Ok(f103), Ok(f202), Ok(f62870)) = (index.read(103), index.read(202), index.read(62870)) else { return };
        let (l103, l202, l62870) = (Library::load(103, f103).unwrap(), Library::load(202, f202).unwrap(), Library::load(62870, f62870).unwrap());
        let d = |l: &Library, n: usize| l.data(n).to_vec();
        let lop0 = d(&l103, sched(&l103, &["s_pa", "effe", "nami"], "lop0"));
        assert!(crate::dat::sched::loops(&lop0) && crate::dat::sched::auto_start(&lop0));
        assert_eq!(crate::dat::sched::span(&lop0), 90);
        let s002 = d(&l202, sched(&l202, &["f_la", "mode", "hato"], "s002"));
        assert!(!crate::dat::sched::loops(&s002) && crate::dat::sched::auto_start(&s002));
        assert_eq!(crate::dat::sched::span(&s002), 9380);
        assert_eq!(crate::dat::sched::span(&d(&l202, sched(&l202, &["f_la", "mode", "hato"], "s001"))), 9000);
        assert!(!crate::dat::sched::auto_start(&d(&l62870, sched(&l62870, &["m_ev", "bels"], "s000"))));
        let count = |l: &Library| l.tree.of_kind(kind::SCHEDULER).filter(|&n| crate::dat::sched::auto_start(l.data(n))).count();
        assert_eq!((count(&l202), count(&l103)), (5, 10));
    }

    #[test]
    fn vana_windows() {
        let hato = |now| vana_window(now, 2376000, 2592000, 216000, 10800);
        assert_eq!([hato(2376000), hato(2383200), hato(2484000), hato(2592000), hato(2368800)], [(true, 216000), (true, 208800), (false, 108000), (false, 4968000), (false, 7200)]);
        assert_eq!(vana_window(432000, 4536000, 1080000, 108000, 10800), (true, 108000));
        assert_eq!(vana_window(18000, 0, 5162400, 43200, 21600), (true, 25200));
        assert_eq!(vana_window(28800, 0, 5162400, 43200, 21600), (true, 14400));
        assert_eq!(vana_window(500, 1000, 1000, 3600, 100), (false, 500));
    }

    /// La Theine (DAT 202) from 10:59:58: `f_la/mode/hato/s001` waits 120, 95 .. 20 units, and at 11:00 a copy starts
    /// the `hato` flight in the same tick.
    #[test]
    fn hato_at_eleven() {
        let Some(index) = index() else { return };
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let l = fx.load_zone(Library::load(202, index.read(202).unwrap()).unwrap());
        assert_eq!(fx.tasks.iter().flatten().count(), 5);
        let s001 = (l, sched(&fx.reg.libs[l], &["f_la", "mode", "hato"], "s001"));
        let hatos = |fx: &Effects| fx.insts.iter().flatten().filter(|x| x.name == *b"hato").count();
        let mut seen = Vec::new();
        for i in 0..6 {
            fx.tick(&frame((2375880 + 25 * i) as f32 / VANA_DAY as f32), &crate::fx::NoActors);
            let cd = fx.tasks.iter().flatten().find(|t| t.res == s001 && t.vana).map(|t| t.countdown);
            seen.push((cd, hatos(&fx)));
        }
        let cds: Vec<i32> = seen.iter().map(|x| x.0.unwrap().round() as i32).collect();
        assert_eq!(&cds[..5], &[120, 95, 70, 45, 20]);
        assert!(seen[..5].iter().all(|x| x.1 == 0) && seen[5].1 == 1, "{seen:?}");
        assert_eq!(fx.tasks.iter().flatten().count(), 6);
    }

    /// DAT 103 `s_pa/effe/nami/lop0` (waits 90, an end loop): it starts over every 91 ticks, one frame after its end.
    #[test]
    fn end_loop_period() {
        let Some(index) = index() else { return };
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let l = fx.load_zone(Library::load(103, index.read(103).unwrap()).unwrap());
        let lop0 = (l, sched(&fx.reg.libs[l], &["s_pa", "effe", "nami"], "lop0"));
        let mut starts = Vec::new();
        let mut prev = 0;
        for tick in 1..=200 {
            fx.tick(&frame(0.5), &crate::fx::NoActors);
            let ip = fx.tasks.iter().flatten().find(|t| t.res == lop0).map_or(0, |t| t.ip);
            if prev == 0 && ip > 0 {
                starts.push((tick, ip));
            }
            prev = ip;
        }
        let ticks: Vec<i32> = starts.iter().map(|s| s.0).collect();
        assert_eq!(ticks, [1, 92, 183], "{starts:?}");
    }

    /// FUN_1018a7a0 on DAT 103: the tag folder's schedulers replace the `weat` ones; no scheduler in `clod`; a
    /// missing tag takes `suny`; the same tag does nothing.
    #[test]
    fn weather_switches_schedulers() {
        let Some(index) = index() else { return };
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let l = fx.load_zone(Library::load(103, index.read(103).unwrap()).unwrap());
        let count = |fx: &Effects, tag: &str| {
            let want = [fourcc("weat"), fourcc(tag), fourcc("kamo")];
            fx.tasks.iter().flatten().filter(|t| t.res.0 == l && fx.reg.libs[l].tree.path(t.res.1).ends_with(&want)).count()
        };
        fx.set_weather("fine", 0.0);
        assert_eq!((count(&fx, "fine"), count(&fx, "suny"), fx.tasks.iter().flatten().count()), (3, 0, 7));
        fx.set_weather("suny", 0.0);
        assert_eq!((count(&fx, "fine"), count(&fx, "suny")), (0, 3));
        fx.set_weather("suny", 0.0);
        assert_eq!((count(&fx, "fine"), count(&fx, "suny")), (0, 3));
        fx.set_weather("clod", 0.0);
        assert_eq!((count(&fx, "fine"), count(&fx, "suny")), (0, 0));
        fx.set_weather("rain", 0.0);
        assert_eq!(count(&fx, "suny"), 3);
    }

    /// DAT 62870 `m_ev/bels/s002` at 10:01:00 (inside 10:01..11:00, period 5 min): one copy at once with op 0x52's
    /// wait (80) as its countdown, stepped in the same tick; the source sleeps 18000 units (to 10:06).
    #[test]
    fn clone_inherits_wait() {
        let Some(index) = index() else { return };
        let mut fx = Effects::new();
        fx.set_index(index.clone());
        fx.set_global(Library::load(0, index.read(0).unwrap()).unwrap());
        let Ok(bytes) = index.read(62870) else { return };
        let l = fx.reg.add(Library::load(62870, bytes).unwrap());
        let s002 = (l, sched(&fx.reg.libs[l], &["m_ev", "bels"], "s002"));
        fx.spawn_task(s002, Who::None, Who::None, None, 1.0, None);
        fx.tick(&frame(2163600.0 / VANA_DAY as f32), &crate::fx::NoActors);
        let tasks: Vec<&Task> = fx.tasks.iter().flatten().filter(|t| t.res == s002).collect();
        assert_eq!(tasks.len(), 2);
        let (src, copy) = if tasks[0].vana { (tasks[0], tasks[1]) } else { (tasks[1], tasks[0]) };
        assert_eq!(src.countdown, 18000.0);
        assert!(!copy.vana && (79.0..=80.0).contains(&copy.countdown), "{}", copy.countdown);
    }
}
