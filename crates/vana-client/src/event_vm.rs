//! Event VM runtime: runs a zone's event scripts (dat/event.rs) as FFXiMain does.
//! Per bound actor one VM (FUN_100bb300) with 16 threads, a pending end-handler slot,
//! an 8-deep call stack and a locals block; each frame the lowest-priority thread of each VM runs
//! until it yields (FUN_100bae70). World work is handed to a [`Host`] as [`Effect`]s and [`Busy`] queries.

use crate::dat::dialog::{self, Ctx, Dialog};
use crate::dat::event::{self, Block, Kind};
use crate::dat::font::decode_sjis;

pub const PLAYER: u32 = 0x7fff_fff0;
const SELF: u32 = 0x7fff_fff8;
const FREE: u16 = 0xff;
/// Position operands are x0.001 (DAT_1031f09c); directions are val * 2pi / 4096.
const POS: f32 = 0.001;
const DIR: f32 = std::f32::consts::TAU / 4096.0;

/// An operand as the handler read it.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i32),
    Actor(u32),
    Name(String),
}

/// Fields every event packet carries: target UniqueNo (+4), zone (EventNum), event id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub target: u32,
    pub zone: u16,
    pub event: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    /// c2s 0x05B mode 1 (FUN_1009ebb0); answered by 0x05C params then 0x052 mode 1.
    EventUpdate { head: Head, option: u32 },
    /// c2s 0x05B mode 0 (FUN_1009ed00); option 0x40000000 when cancelled.
    EventEnd { head: Head, option: u32 },
    /// c2s 0x05C (FUN_1009ec80): x, height, z, dir byte.
    EventPos { head: Head, x: f32, y: f32, z: f32, dir: f32, option: u32 },
    /// Other event packets (0x1b, 0x58, 0x60, 0xeb) by opcode.
    Other { op: u8, sub: u8 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Dialogue line; the window stays open until [`Runtime::close_message`].
    /// `place`: the text's 02 / 03 window position codes.
    Message { op: u8, speaker: Option<u32>, id: u32, text: String, prompt: bool, wait: Option<u32>, place: [Option<i16>; 2] },
    /// Choice menu (op 0x24); answer with [`Runtime::choose`].
    Choice { id: u32, question: String, options: Vec<(u32, String)>, cursor: usize },
    /// Scheduler start (0x45 and family): file id (base + n), tag, actor, target, param.
    Scheduler { file: u32, tag: String, actor: u32, target: u32, param: i32 },
    SchedulerStop { file: u32, tag: String, actor: u32, target: u32 },
    /// 0x46 sub 0 / 1: FUN_10082b20 (flag = DAT_10346088 = 1, camera seeded from the player) /
    /// FUN_10082be0 (flag = 0). Cutscenes run with 0 and set 1 before they end.
    Camera { flag: bool },
    /// Sound volume, 0x69 set / 0x6a fade (FUN_10035200.. / FUN_10034be0): mask 1 world sound effects
    /// (DAT_1034656c), 2 interface sounds (DAT_10346568), 4 music (DAT_10346570), 8 all of them and
    /// the stream at DAT_10450fe8; 0x10 (0x69) sets DAT_10346578. Reaches `level` over `ticks`.
    Volume { mask: i32, level: f32, ticks: f32 },
    /// Actor alpha fade (0x6c).
    ActorFade { actor: u32, alpha: i32, frames: i32 },
    /// Motion start: 0x5b / 0x66 load motion file `file` then play 4cc `name` (model vtable +0x298),
    /// 0x2c plays one the actor has (file 0). 0x53 waits for it ([`Busy::Playing`]).
    Motion { op: u8, actor: u32, target: u32, file: i32, name: String },
    IdleMotion { actor: u32, name: String },
    /// Discrete position / direction set by the script (walking is in [`Runtime::actors`]); 0x36 / 0x37 snap
    /// the drawn transform (FUN_100917e0), 0x39 / 0x4b only turn.
    Position { op: u8, actor: u32, pos: [f32; 3], dir: f32 },
    Face { actor: u32, target: u32 },
    /// A wait started (0x1c, 0x6f): ticks at 60 Hz.
    Wait { actor: u32, ticks: f32 },
    Send(Packet),
    /// Any other opcode, with its operands decoded.
    Op { op: u8, sub: Option<u8>, actor: u32, args: Vec<Arg> },
    /// A form the client cannot run (no length, or b1 sub >= 3): the thread is ended here.
    Fault { actor: u32, pc: u16, op: u8 },
    End { option: u32, cancelled: bool },
}

/// What a waiting op asks the host each frame; true = still busy (the op yields).
#[derive(Clone, Debug, PartialEq)]
pub enum Busy {
    Scheduler { file: u32, tag: String, actor: u32, target: u32 },
    /// Actor turning (0x70, 0x76; actor +0x12c bit 1).
    Turning { actor: u32 },
    /// 0x5b / 0x66: the motion file is still loading (FUN_100cf260); the op then plays and yields once.
    MotionLoad { actor: u32, file: i32 },
    /// 0x53: motion or scheduler `name` of the actor still playing (model vtable +0x2a0).
    Playing { actor: u32, name: String },
    Op { op: u8, sub: Option<u8>, actor: u32 },
}

/// The client side of the VM. Defaults suit a headless run: actors exist, nothing stays busy.
pub trait Host {
    fn effect(&mut self, e: Effect);
    fn busy(&mut self, _b: &Busy) -> bool {
        false
    }
    /// Actor loaded (FUN_10089c70) and with a model (+0x120 bit 9).
    fn ready(&self, _actor: u32) -> bool {
        true
    }
    fn name(&self, actor: u32) -> String {
        if actor == PLAYER { "Player".into() } else { format!("npc{:03x}", actor & 0xfff) }
    }
    fn female(&self) -> bool {
        false
    }
    /// Start position (x, height, z) and direction of a bound actor.
    fn place(&self, _actor: u32) -> ([f32; 3], f32) {
        ([0.0; 3], 0.0)
    }
    /// Ground height under a point (FUN_1017a390 / FUN_1017a400); None = no snap.
    fn ground(&self, _actor: u32, _p: [f32; 3]) -> Option<f32> {
        None
    }
    /// Player bytes read by values 0x7f06 (low) / 0x7f08 (next) (DAT_10472c48).
    fn player_word(&self) -> u32 {
        0
    }
    /// A game config value by id (op 0xda reads 0x43, the aspect).
    fn game_config(&self, _id: u16) -> u32 {
        0
    }

    /// Config word read by op 0xb1 (*(DAT_10444110 + 0x18)).
    fn config_word(&self) -> u32 {
        0
    }
    fn rand(&mut self) -> u32 {
        0
    }
}

/// Event start (0x032 / 0x034 / 0x033).
#[derive(Clone, Default)]
pub struct Start {
    pub zone: u16,
    pub event: u16,
    /// Target UniqueNo (DAT_1046e0ac).
    pub target: u32,
    /// 0x034 num[8] -> Z2..Z9.
    pub params: [u32; 8],
    /// 0x033 strings (DAT_10476300).
    pub strings: Vec<String>,
}

/// Work that outlives one event: 0x1100 memorize and 0x1700 slots.
#[derive(Clone)]
pub struct Memory {
    pub m: [u32; 0x40],
    pub g: [u32; 0x20],
}

impl Default for Memory {
    fn default() -> Self {
        Memory { m: [0; 0x40], g: [0; 0x20] }
    }
}

#[derive(Clone, Copy)]
struct Thread {
    prio: u16,
    pc: u16,
    /// Walk / move target (+4..+0x13).
    target: [f32; 4],
    entry: u8,
    /// 0x28 / 0x29 state (+0x17).
    state: u8,
    /// -1 = idle (+0x18).
    timer: f32,
    caller: u32,
}

impl Thread {
    fn free() -> Thread {
        Thread { prio: FREE, pc: 0, target: [0.0; 4], entry: 0, state: 0, timer: -1.0, caller: 0 }
    }
}

/// The locals block (+0x260): 0x50 words, then the actor transform the event drives.
#[derive(Clone)]
pub struct Body {
    pub locals: [u32; 0x50],
    /// x, height, z (+0x140..+0x148).
    pub pos: [f32; 3],
    /// Direction in radians (+0x154).
    pub dir: f32,
    /// Units per second (+0x168), op 0x32 = val * 0.1.
    pub speed: f32,
    /// +0x16c (5.0 at bind, op 0x91 = val * 10): x 0.8 the model's motion speed reference (FUN_1008cce0).
    pub anim_speed: f32,
    /// Op 0x33: actor +0x120 bit 0x200000, no ground snap for walks and 0x36 / 0x37.
    pub no_ground: bool,
    /// Actor whose talk motion the open message drives (+0x164).
    msg_actor: Option<u32>,
}

struct Vm {
    block: usize,
    /// Own actor (+4 / +0) and the one it acts as (+8 / +2, switched by 0x7a sub 2).
    own: u32,
    actor: u32,
    /// Locals owner (+0x260).
    body: usize,
    threads: [Thread; 17],
    cur: usize,
    pc: u16,
    yielded: bool,
    started: bool,
    stack: Vec<u16>,
    /// Actor +0x120 bit 0x80000 (op 0x2f): the VM does not run.
    paused: bool,
}

#[derive(Default)]
struct ChoiceState {
    requested: bool,
    ready: bool,
    open: bool,
    picked: Option<u16>,
}

pub struct Runtime {
    blocks: Vec<Block>,
    dialog: Option<Dialog>,
    vms: Vec<Vm>,
    bodies: Vec<Body>,
    pub start: Start,
    /// Zone work Z (DAT_10475df0).
    pub z: [u32; 0x60],
    pub mem: Memory,
    /// Table pointer slots (DAT_10476df0 / DAT_10476ce0): (block, code offset).
    slots: [Option<(usize, u16)>; 0x40],
    /// Message window: 0 closed, 1 open, 2 cancelled (DAT_10475800).
    window: u8,
    choice: ChoiceState,
    /// DAT_10472bb0 / DAT_1046d17f: waiting for 0x052 mode 1 / the 0x05C position reply.
    wait_update: bool,
    wait_pos: bool,
    ending: bool,
    end_phase: u8,
    cancelled: bool,
    done: bool,
    camera: bool,
    /// DAT_10470548: actor of the last message.
    last_speaker: Option<u32>,
    names: Option<std::sync::Arc<dyn crate::dat::names::NameSource>>,
    player_word: u32,
    pub dt: f32,
    /// Print each executed op to stderr.
    pub trace: bool,
    pub frame: u64,
}

fn rd16(code: &[u8], at: usize) -> u16 {
    code.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn rd32(code: &[u8], at: usize) -> u32 {
    code.get(at..at + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn four_cc(v: u32) -> String {
    String::from_utf8_lossy(&v.to_le_bytes()).trim_end_matches('\0').to_string()
}

impl Runtime {
    /// Binds every block that has the event id (or the 0xfffe wildcard) and runs FUN_100baf30 on each.
    pub fn start(blocks: Vec<Block>, dialog: Option<Dialog>, start: Start, mem: Memory, host: &mut dyn Host) -> Runtime {
        let mut z = [0u32; 0x60];
        z[2..10].copy_from_slice(&start.params);
        let mut rt = Runtime {
            blocks,
            dialog,
            vms: Vec::new(),
            bodies: Vec::new(),
            start,
            z,
            mem,
            slots: [None; 0x40],
            window: 0,
            choice: ChoiceState::default(),
            wait_update: false,
            wait_pos: false,
            ending: false,
            end_phase: 0,
            cancelled: false,
            done: false,
            camera: false,
            last_speaker: None,
            names: None,
            player_word: host.player_word(),
            dt: 1.0,
            trace: std::env::var_os("FFXI_EVENT_TRACE").is_some(),
            frame: 0,
        };
        let ev = rt.start.event;
        let mut bound: Vec<usize> =
            (0..rt.blocks.len()).filter(|&i| rt.blocks[i].events.iter().any(|(id, _)| *id == ev || *id == 0xfffe)).collect();
        // The client walks actor slots in index order; the player's slot is above the NPCs'.
        bound.sort_by_key(|&i| Self::index_of(rt.blocks[i].actor));
        for b in bound {
            let actor = rt.blocks[b].actor;
            let (pos, dir) = host.place(actor);
            let n = rt.vms.len();
            rt.bodies.push(Body { locals: [0; 0x50], pos, dir, speed: 5.0, anim_speed: 5.0, no_ground: false, msg_actor: None });
            rt.vms.push(Vm {
                block: b,
                own: actor,
                actor,
                body: n,
                threads: [Thread::free(); 17],
                cur: 0,
                pc: 0,
                yielded: false,
                started: false,
                stack: Vec::new(),
                paused: false,
            });
        }
        for v in 0..rt.vms.len() {
            rt.init(v, host);
        }
        rt
    }

    fn index_of(actor: u32) -> u32 {
        if actor == PLAYER { 0x400 } else { actor & 0x3ff }
    }

    /// FUN_100baf30: entry 0 runs at once at priority 0x80, then slot 0 takes the event's entry at 0x10.
    fn init(&mut self, v: usize, host: &mut dyn Host) {
        let (first, ev) = {
            let b = &self.blocks[self.vms[v].block];
            (b.events.first().map_or(0, |e| e.1), self.start.event)
        };
        {
            let vm = &mut self.vms[v];
            vm.stack.clear();
            vm.yielded = false;
            vm.threads[16].prio = FREE;
            vm.threads[0] = Thread { prio: 0x80, pc: first, caller: vm.own, ..Thread::free() };
            vm.cur = 0;
            vm.pc = first;
        }
        self.run(v, host);
        let vm = &mut self.vms[v];
        vm.threads[vm.cur].pc = vm.pc;
        let b = &self.blocks[vm.block];
        let hit = b.events.iter().position(|e| e.0 == ev).or_else(|| b.events.iter().position(|e| e.0 == 0xfffe));
        if let Some(i) = hit {
            vm.threads[0].entry = i as u8;
            vm.threads[0].prio = 0x10;
            vm.threads[0].caller = vm.own;
            vm.threads[0].pc = b.events[i].1;
            vm.pc = vm.threads[vm.cur].pc;
        }
        vm.started = true;
    }

    fn head(&self) -> Head {
        Head { target: self.start.target, zone: self.start.zone, event: self.start.event }
    }

    /// Item / key item / place names for message text.
    pub fn set_names(&mut self, names: std::sync::Arc<dyn crate::dat::names::NameSource>) {
        self.names = Some(names);
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn message_open(&self) -> bool {
        self.window == 1
    }

    /// A choice menu is open and waiting for [`Runtime::choose`].
    pub fn choice_open(&self) -> bool {
        self.choice.open && self.choice.picked.is_none()
    }

    /// Every thread has ended without event_end: the client now waits for the server (0x052).
    pub fn idle(&self) -> bool {
        !self.ending && self.vms.iter().all(|vm| vm.threads[..16].iter().all(|t| t.prio == FREE))
    }

    pub fn camera(&self) -> bool {
        self.camera
    }

    /// The advance key closed the message window.
    pub fn close_message(&mut self) {
        if self.window == 1 {
            self.window = 0;
        }
    }

    /// The message window was cancelled (aborts the event).
    pub fn cancel_message(&mut self) {
        self.window = 2;
    }

    /// Pick option `index` (its position in the message, hidden ones counted); None = cancel.
    pub fn choose(&mut self, index: Option<u32>) {
        if self.choice.open {
            self.choice.picked = Some(index.map_or(0xff, |i| i as u16 + 1));
        }
    }

    /// s2c 0x05C: event parameters into Z2..Z9.
    pub fn params(&mut self, num: [u32; 8]) {
        self.z[2..10].copy_from_slice(&num);
    }

    /// s2c 0x052 (100a06f0): mode 1 ends an update round trip; mode 2 for this event cancels it.
    pub fn release(&mut self, mode: u8, event: u16) {
        match mode {
            1 => self.wait_update = false,
            2 if event == self.start.event => {
                self.ending = true;
                self.end_phase = 1;
                self.cancelled = true;
            }
            _ => {}
        }
    }

    /// s2c 0x065 / 0x05B position modes 1-2 (FUN_1009d500): the 0x05C round trip finished.
    pub fn position_ack(&mut self) {
        self.wait_pos = false;
    }

    /// Transforms of the bound actors (the client draws event actors from these).
    pub fn actors(&self) -> impl Iterator<Item = (u32, &Body)> {
        self.vms.iter().enumerate().map(|(i, v)| (v.own, &self.bodies[i]))
    }

    /// Live threads per VM: (actor, [(slot, priority, pc, opcode)]), for diagnostics.
    pub fn threads(&self) -> Vec<(u32, Vec<(usize, u16, u16, u8)>)> {
        self.vms
            .iter()
            .map(|vm| {
                let code = &self.blocks[vm.block].code;
                let live = (0..16)
                    .filter(|&i| vm.threads[i].prio != FREE)
                    .map(|i| (i, vm.threads[i].prio, vm.threads[i].pc, code.get(vm.threads[i].pc as usize).copied().unwrap_or(0)))
                    .collect();
                (vm.own, live)
            })
            .collect()
    }

    /// One frame: each VM runs its selected thread, then the end phase (FUN_10095xxx, def8).
    pub fn tick(&mut self, host: &mut dyn Host, dt: f32) {
        if self.done {
            return;
        }
        self.dt = dt.max(1.0);
        self.frame += 1;
        for v in 0..self.vms.len() {
            if !self.vms[v].paused {
                self.step_vm(v, host);
            }
        }
        if self.end_phase != 0 {
            let mut busy = false;
            for v in 0..self.vms.len() {
                if self.end_phase == 1 {
                    busy |= self.end_reset(v);
                } else {
                    let vm = &self.vms[v];
                    let t = &vm.threads[vm.cur];
                    busy |= t.prio != FREE && t.entry == vm.threads[16].entry;
                }
            }
            if self.end_phase == 1 {
                self.end_phase = 2;
            }
            if !busy {
                self.end_phase = 0;
            }
        }
        if self.ending && self.end_phase == 0 {
            let option = if self.cancelled { 0x4000_0000 } else { self.z[1] };
            host.effect(Effect::Send(Packet::EventEnd { head: self.head(), option }));
            host.effect(Effect::End { option, cancelled: self.cancelled });
            self.done = true;
        }
    }

    /// FUN_100bb240: free every thread; a registered end handler moves to slot 0.
    fn end_reset(&mut self, v: usize) -> bool {
        let vm = &mut self.vms[v];
        if !vm.started {
            return false;
        }
        vm.pc = 0;
        vm.cur = 0;
        vm.stack.clear();
        for t in vm.threads.iter_mut().take(16) {
            *t = Thread::free();
        }
        if vm.threads[16].prio != FREE {
            vm.threads[0] = vm.threads[16];
            vm.threads[16].prio = FREE;
            return true;
        }
        false
    }

    /// FUN_100bae70: lowest priority value wins, ties to the highest slot.
    fn step_vm(&mut self, v: usize, host: &mut dyn Host) {
        let vm = &mut self.vms[v];
        if !vm.started {
            return;
        }
        let mut best = FREE;
        for i in 0..16 {
            if vm.threads[i].prio <= best {
                vm.cur = i;
                best = vm.threads[i].prio;
            }
        }
        if !((!self.ending && !self.cancelled) || self.end_phase != 0) || best == FREE {
            return;
        }
        vm.pc = vm.threads[vm.cur].pc;
        self.run(v, host);
        let vm = &mut self.vms[v];
        vm.threads[vm.cur].pc = vm.pc;
    }

    /// FUN_100bae30: ops until one yields.
    fn run(&mut self, v: usize, host: &mut dyn Host) {
        self.vms[v].yielded = false;
        let mut guard = 0;
        while !self.vms[v].yielded {
            self.op(v, host);
            guard += 1;
            if guard > 100_000 {
                // A loop that never yields would hang the client; stop it here.
                let (actor, pc) = (self.vms[v].actor, self.vms[v].pc);
                let op = self.code(v).get(pc as usize).copied().unwrap_or(0);
                host.effect(Effect::Fault { actor, pc, op });
                self.end_thread(v);
            }
        }
    }

    fn code(&self, v: usize) -> &[u8] {
        &self.blocks[self.vms[v].block].code
    }

    fn u8_at(&self, v: usize, o: usize) -> u8 {
        self.code(v).get(self.vms[v].pc as usize + o).copied().unwrap_or(0)
    }

    fn u16_at(&self, v: usize, o: usize) -> u16 {
        rd16(self.code(v), self.vms[v].pc as usize + o)
    }

    fn u32_at(&self, v: usize, o: usize) -> u32 {
        rd32(self.code(v), self.vms[v].pc as usize + o)
    }

    /// FUN_100ada80 on block `b`'s constants: operand `raw` + `add`.
    fn get(&self, v: usize, b: usize, raw: u16, add: i32) -> u32 {
        let k = (raw as i32 + add) as u32;
        if k & 0x8000 != 0 {
            return self.blocks[b].consts.get((k & 0x7fff) as usize).copied().unwrap_or(0);
        }
        let body = &self.bodies[self.vms[v].body];
        match k {
            0..0x800 => body.locals.get(k as usize).copied().unwrap_or(0),
            0x800..0x1100 => self.z.get((k as i32 - 0x1000) as usize).copied().unwrap_or(0),
            0x1100..0x1200 => self.mem.m.get(k as usize - 0x1100).copied().unwrap_or(0),
            0x1200..0x1800 => (k as i32 - 0x1700).try_into().ok().and_then(|i: usize| self.mem.g.get(i).copied()).unwrap_or(0),
            0x7f00 => (body.pos[0] / POS) as i32 as u32,
            0x7f01 => (body.pos[1] / POS) as i32 as u32,
            0x7f02 => (body.pos[2] / POS) as i32 as u32,
            0x7f03 => (body.dir.rem_euclid(std::f32::consts::TAU) / DIR) as i32 as u32,
            0x7f06 if self.vms[v].actor == PLAYER => self.player_word & 0xff,
            0x7f08 if self.vms[v].actor == PLAYER => self.player_word >> 8 & 0xff,
            0x7f08 => 1,
            0x7f0a => self.vms[v].actor,
            0x7f80..0x7f84 => {
                let p = self.player_body().map(|b| (b.pos, b.dir)).unwrap_or(([0.0; 3], 0.0));
                match k {
                    0x7f80..0x7f83 => (p.0[(k - 0x7f80) as usize] / POS) as i32 as u32,
                    _ => (p.1.rem_euclid(std::f32::consts::TAU) / DIR) as i32 as u32,
                }
            }
            0x7f86 => self.player_word & 0xff,
            0x7f88 => self.player_word >> 8 & 0xff,
            0x7f8a => PLAYER,
            _ => 0,
        }
    }

    fn player_body(&self) -> Option<&Body> {
        self.vms.iter().position(|v| v.own == PLAYER).map(|i| &self.bodies[i])
    }

    /// Value operand at pc + o.
    fn val(&self, v: usize, o: usize) -> i32 {
        self.get(v, self.vms[v].block, self.u16_at(v, o), 0) as i32
    }

    /// FUN_100ad8f0: scalar store to operand at pc + o (+ add).
    fn set_at(&mut self, v: usize, o: usize, add: i32, value: u32) {
        let raw = self.u16_at(v, o);
        self.set_raw(v, raw, add, value);
    }

    fn set_raw(&mut self, v: usize, raw: u16, add: i32, value: u32) {
        let k = raw as i32 + add;
        if k & 0x8000 != 0 {
            return;
        }
        let body = self.vms[v].body;
        match k {
            0..0x50 => self.bodies[body].locals[k as usize] = value,
            0x1000..0x1060 => self.z[k as usize - 0x1000] = value,
            0x1100..0x1140 => self.mem.m[k as usize - 0x1100] = value,
            0x1700..0x1720 => self.mem.g[k as usize - 0x1700] = value,
            0x7f00..=0x7f02 => self.bodies[body].pos[(k - 0x7f00) as usize] = value as i32 as f32 * POS,
            0x7f03 => self.bodies[body].dir = value as i32 as f32 * DIR,
            _ => {}
        }
    }

    /// FUN_100adf30: 16-byte string operand (constants read as empty).
    fn str_at(&self, v: usize, o: usize) -> [u8; 16] {
        let k = self.u16_at(v, o) as usize;
        let words: Option<&[u32]> = match k {
            _ if k & 0x8000 != 0 => None,
            0..0x50 => self.bodies[self.vms[v].body].locals.get(k..(k + 4).min(0x50)),
            0x1000..0x1060 => self.z.get(k - 0x1000..(k - 0x1000 + 4).min(0x60)),
            0x1100..0x1140 => self.mem.m.get(k - 0x1100..(k - 0x1100 + 4).min(0x40)),
            0x1700..0x1720 => self.mem.g.get(k - 0x1700..(k - 0x1700 + 4).min(0x20)),
            _ => None,
        };
        let mut out = [0u8; 16];
        for (i, w) in words.unwrap_or(&[]).iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out[15] = 0;
        out
    }

    /// FUN_100addc0: 4-word store.
    fn set4(&mut self, v: usize, raw: u16, add: i32, w: [u32; 4]) {
        let k = raw as i32 + add;
        let body = self.vms[v].body;
        let dst: Option<&mut [u32]> = match k {
            _ if k & 0x8000 != 0 => None,
            0..0x40 => Some(&mut self.bodies[body].locals[k as usize..k as usize + 4]),
            0x1000..0x1050 => Some(&mut self.z[k as usize - 0x1000..k as usize - 0x1000 + 4]),
            0x1100..0x1130 => Some(&mut self.mem.m[k as usize - 0x1100..k as usize - 0x1100 + 4]),
            0x1700..0x1710 => Some(&mut self.mem.g[k as usize - 0x1700..k as usize - 0x1700 + 4]),
            _ => None,
        };
        if let Some(d) = dst {
            d.copy_from_slice(&w);
        }
    }

    /// FUN_100ae0c0: special actor ids.
    fn resolve(&self, v: usize, id: u32) -> u32 {
        match id {
            0x7fff_ffc0 | PLAYER | 0x7fff_fff9 => PLAYER,
            SELF => self.vms[v].actor,
            _ if id & 0xff00_0000 == 0 => self.vms[v].actor,
            _ => id,
        }
    }

    fn actor_at(&self, v: usize, o: usize) -> u32 {
        self.resolve(v, self.u32_at(v, o))
    }

    fn vm_of(&self, actor: u32) -> Option<usize> {
        self.vms.iter().position(|vm| vm.own == actor)
    }

    fn adv(&mut self, v: usize, n: u16) {
        self.vms[v].pc = self.vms[v].pc.wrapping_add(n);
    }

    fn jump(&mut self, v: usize, o: usize) {
        self.vms[v].pc = self.u16_at(v, o);
    }

    fn yield_(&mut self, v: usize) {
        self.vms[v].yielded = true;
    }

    /// FUN_100ae240: end the current thread.
    fn end_thread(&mut self, v: usize) {
        let vm = &mut self.vms[v];
        let c = vm.cur;
        vm.threads[c] = Thread::free();
        vm.yielded = true;
    }

    fn thread(&mut self, v: usize) -> &mut Thread {
        let c = self.vms[v].cur;
        &mut self.vms[v].threads[c]
    }

    /// FUN_100b1d10 + FUN_100b1a90: start entry `entry` of `target` at `prio`. 0 already running,
    /// 1 started, 2 no free slot, -1 target not bound.
    fn start_thread(&mut self, v: usize, target: u32, entry: u8, prio: u16) -> i32 {
        let Some(t) = self.vm_of(target) else { return -1 };
        let caller = self.vms[v].actor;
        let tvm = &mut self.vms[t];
        let mut free = None;
        for i in 0..16 {
            if tvm.threads[i].prio == FREE {
                free = Some(i);
            } else if tvm.threads[i].entry == entry {
                return 0;
            }
        }
        let Some(i) = free else { return 2 };
        let pc = self.blocks[tvm.block].events.get(entry as usize).map_or(0, |e| e.1);
        tvm.threads[i] = Thread { prio, pc, entry, caller, ..Thread::free() };
        1
    }

    /// FUN_100b1a40 on `target`: 0 its current thread has the entry, 1 another slot has it, -1 none.
    fn has_entry(&self, target: usize, entry: u8) -> i32 {
        let vm = &self.vms[target];
        if vm.threads[vm.cur].entry == entry {
            return 0;
        }
        if vm.threads[..16].iter().any(|t| t.entry == entry) { 1 } else { -1 }
    }

    /// FUN_100b1a00 on `target`: 1 when no thread has priority <= p.
    fn none_at_or_below(&self, target: usize, p: u16) -> bool {
        let vm = &self.vms[target];
        p < vm.threads[vm.cur].prio && vm.threads[..16].iter().all(|t| t.prio > p)
    }

    fn ctx(&self, speaker: Option<u32>, host: &dyn Host) -> Ctx {
        let mut num = [0i32; 8];
        for (i, n) in num.iter_mut().enumerate() {
            *n = self.z[2 + i] as i32;
        }
        Ctx {
            player: host.name(PLAYER),
            speaker: speaker.map(|s| host.name(s)).unwrap_or_default(),
            num,
            strings: self.start.strings.clone(),
            female: host.female(),
            names: self.names.clone(),
            ..Default::default()
        }
    }

    fn text(&self, id: u32, speaker: Option<u32>, host: &dyn Host) -> (String, bool, Option<dialog::Expanded>) {
        let Some(raw) = self.dialog.as_ref().and_then(|d| d.raw(id)) else {
            return (format!("<message {id}>"), true, None);
        };
        let e = dialog::expand(&raw, &self.ctx(speaker, host));
        let t = e.lines.iter().map(|l| decode_sjis(l)).collect::<Vec<_>>().join("\n");
        (t, e.prompt, Some(e))
    }

    /// `speaker` gets the "Name : " prefix; `about` becomes DAT_10470548, the actor {speaker} codes
    /// name (0x48 keeps the previous one).
    fn message(&mut self, v: usize, op: u8, speaker: Option<u32>, about: Option<u32>, id: u32, host: &mut dyn Host) {
        self.window = 1;
        if about.is_some() {
            self.last_speaker = about;
        }
        let (text, prompt, e) = self.text(id, self.last_speaker, host);
        let (wait, place) = e.map_or((None, [None; 2]), |e| (e.wait, e.place));
        let name = speaker.map(|s| host.name(s));
        let text = match name {
            Some(n) => format!("{n} : {text}"),
            None => text,
        };
        let body = self.vms[v].body;
        self.bodies[body].msg_actor = speaker;
        host.effect(Effect::Message { op, speaker, id, text, prompt, wait, place });
    }

    /// Operands of a generic op from the opcode table.
    fn args(&self, v: usize, op: &event::Op, sub: Option<u8>) -> Vec<Arg> {
        let args = match sub {
            Some(s) if !op.sub_args.is_empty() => op.sub_args.iter().find(|(k, _)| *k == s).map_or(op.args, |(_, a)| *a),
            _ => op.args,
        };
        args.iter()
            .map(|&(k, o)| {
                let o = o as usize;
                match k {
                    Kind::Val | Kind::Msg => Arg::Int(self.val(v, o)),
                    Kind::Set | Kind::Str | Kind::U16 => Arg::Int(self.u16_at(v, o) as i32),
                    Kind::U8 => Arg::Int(self.u8_at(v, o) as i32),
                    Kind::U32 => Arg::Int(self.u32_at(v, o) as i32),
                    Kind::Actor => Arg::Actor(self.actor_at(v, o)),
                    Kind::Motion => Arg::Name(four_cc(self.u32_at(v, o))),
                    Kind::Lit16 => {
                        let pc = self.vms[v].pc as usize + o;
                        let b = self.code(v).get(pc..pc + 16).unwrap_or(&[]);
                        Arg::Name(String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
                    }
                }
            })
            .collect()
    }

    /// Scheduler ops: file id from the op's base (0x77f0 remaps n, FUN_100b26c0).
    fn sched(&self, v: usize, base: u32) -> Option<(u32, String, u32, u32)> {
        let n = self.val(v, 1) as u32;
        let n = if base == 0x77f0 { crate::eventdump::camera_file(n) - 0x77f0 } else { n };
        let (a, t) = (self.actor_at(v, 3), self.actor_at(v, 7));
        Some((base + n, four_cc(self.u32_at(v, 0xb)), a, t))
    }

    fn op(&mut self, v: usize, host: &mut dyn Host) {
        let pc = self.vms[v].pc as usize;
        let Some(&opc) = self.code(v).get(pc) else {
            self.end_thread(v);
            return;
        };
        if self.trace {
            let vm = &self.vms[v];
            eprintln!("f{} {:08x} t{} pc {pc:04x} op {opc:02x}", self.frame, vm.own, vm.cur);
        }
        let Some((op, len, sub)) = event::op_at(self.code(v), pc) else {
            if crate::dat::event_ops::OPS[opc as usize].is_none() {
                // Dispatcher default: end the thread.
                self.end_thread(v);
            } else {
                host.effect(Effect::Fault { actor: self.vms[v].actor, pc: pc as u16, op: opc });
                self.end_thread(v);
            }
            return;
        };
        let len = len as u16;
        let actor = self.vms[v].actor;
        let body = self.vms[v].body;
        match opc {
            0x00 | 0xca | 0xcb => self.end_thread(v),
            0x01 => self.jump(v, 1),
            0x02 => {
                let (a, b) = (self.val(v, 1), self.val(v, 3));
                let t = match self.u8_at(v, 5) & 0xf {
                    0 => a == b,
                    1 | 7 => a != b,
                    2 => a > b,
                    3 => a < b,
                    4 => a >= b,
                    5 => a <= b,
                    6 | 9 => a & b != 0,
                    8 => a != 0 || b != 0,
                    10 => b & !a != 0,
                    _ => false,
                };
                if t { self.adv(v, 8) } else { self.jump(v, 6) }
            }
            0x03 => {
                let x = self.val(v, 3) as u32;
                self.set_at(v, 1, 0, x);
                self.adv(v, 5);
            }
            0x04 => self.adv(v, 3),
            0x05 | 0x06 => {
                self.set_at(v, 1, 0, (opc == 0x05) as u32);
                self.adv(v, 3);
            }
            0x07..=0x11 | 0x14 | 0x15 => {
                let a = self.val(v, 1);
                let b = self.val(v, 3);
                let r = match opc {
                    0x07 => a.wrapping_add(b),
                    0x08 => a.wrapping_sub(b),
                    0x09 => a | 1i32.wrapping_shl(b as u32),
                    0x0a => a & !1i32.wrapping_shl(b as u32),
                    0x0b => a.wrapping_add(1),
                    0x0c => a.wrapping_sub(1),
                    0x0d => a & b,
                    0x0e => a | b,
                    0x0f => a ^ b,
                    0x10 => a.wrapping_shl(b as u32),
                    0x11 => a.wrapping_shr(b as u32),
                    0x14 => a.wrapping_mul(b),
                    _ => if a == 0 || b == 0 { 0 } else { a.wrapping_div(b) },
                };
                self.set_at(v, 1, 0, r as u32);
                self.adv(v, len);
            }
            0x12 => {
                let r = host.rand();
                self.set_at(v, 1, 0, r);
                self.adv(v, 3);
            }
            0x13 => {
                let m = self.val(v, 3).wrapping_add(1);
                let r = host.rand() as i32;
                self.set_at(v, 1, 0, if m == 0 { 0 } else { r.wrapping_rem(m) } as u32);
                self.adv(v, 5);
            }
            0x16 | 0x17 => {
                let (a, m) = (self.val(v, 3) as f32 * DIR, self.val(v, 5) as f32);
                let r = if opc == 0x16 { a.sin() } else { a.cos() } * m;
                self.set_at(v, 1, 0, r as i32 as u32);
                self.adv(v, 7);
            }
            0x18 => {
                let r = (-(self.val(v, 3) as f32)).atan2(self.val(v, 5) as f32) * 4096.0 * 0.318_319_26;
                self.set_at(v, 1, 0, r as i32 as u32);
                self.adv(v, 7);
            }
            0x19 => {
                let (ra, rb) = (self.u16_at(v, 1), self.u16_at(v, 3));
                let (a, b) = (self.val(v, 1) as u32, self.val(v, 3) as u32);
                self.set_raw(v, ra, 0, b);
                self.set_raw(v, rb, 0, a);
                self.adv(v, 5);
            }
            0x1a => {
                let to = self.u16_at(v, 1);
                let ret = self.vms[v].pc.wrapping_add(3);
                self.call(v, to, ret);
            }
            0x1b => {
                if let Some(r) = self.vms[v].stack.pop() {
                    self.vms[v].pc = r;
                } else {
                    self.end_thread(v);
                }
            }
            0x1c | 0x6f => {
                if self.thread(v).timer < 0.0 {
                    let n = if opc == 0x1c { self.val(v, 1) as f32 } else { 16.0 };
                    self.thread(v).timer = n;
                    host.effect(Effect::Wait { actor, ticks: n });
                }
                let dt = self.dt;
                let t = self.thread(v);
                t.timer -= dt;
                let fin = t.timer < 0.0;
                self.yield_(v);
                if fin {
                    self.adv(v, len);
                }
            }
            0x1d => {
                let id = self.val(v, 1) as u32;
                self.message(v, opc, Some(actor), Some(actor), id, host);
                self.adv(v, 3);
            }
            // FUN_100b01f0: nothing shows when the speaker does not resolve (FUN_100ae0c0).
            0x2b | 0x49 => {
                let who = self.actor_at(v, 1);
                let id = self.val(v, 5) as u32;
                if host.ready(who) {
                    self.message(v, opc, (opc == 0x2b).then_some(who), Some(who), id, host);
                }
                self.adv(v, 7);
            }
            // FUN_100b0570 clears DAT_10470548 / DAT_10472bb2.
            0x48 => {
                let id = self.val(v, 1) as u32;
                self.last_speaker = None;
                self.message(v, opc, None, None, id, host);
                self.adv(v, 3);
            }
            0xb0 if sub == Some(0) => {
                let who = self.actor_at(v, 2);
                let id = self.val(v, 10) as u32;
                if host.ready(who) {
                    self.message(v, opc, Some(who), Some(who), id, host);
                }
                self.adv(v, len);
            }
            0x1e => {
                let target = self.actor_at(v, 1);
                self.face(v, actor, target, host);
                self.adv(v, 5);
            }
            0x4a => {
                let (a, t) = (self.actor_at(v, 1), self.actor_at(v, 5));
                self.face(v, a, t, host);
                self.adv(v, 9);
            }
            0x1f | 0x5a => match sub {
                Some(0) => {
                    let (x, z, y) = (self.val(v, 2) as f32 * POS, self.val(v, 4) as f32 * POS, self.val(v, 6) as f32 * POS);
                    self.thread(v).target = [x, y, z, 0.0];
                    self.adv(v, 8);
                }
                _ => self.walk(v, opc == 0x5a, host),
            },
            0x20 => {
                let a = self.args(v, op, sub);
                host.effect(Effect::Op { op: opc, sub, actor, args: a });
                self.adv(v, 2);
            }
            0x21 => {
                self.ending = true;
                if self.end_phase == 0 {
                    self.end_phase = 1;
                }
                self.yield_(v);
            }
            0x23 => match self.window {
                0 => {
                    self.bodies[body].msg_actor = None;
                    self.adv(v, 1);
                }
                1 => self.yield_(v),
                _ => {
                    self.ending = true;
                    self.end_phase = 1;
                    self.cancelled = true;
                    self.yield_(v);
                }
            },
            0x24 => self.choice_op(v, host),
            0x25 => self.wait_choice(v),
            0x26 => self.yield_(v),
            0x27 => {
                let (t, e, p) = (self.actor_at(v, 2), self.u8_at(v, 6), self.u8_at(v, 1) as u16);
                if self.start_thread(v, t, e, p) == 2 {
                    self.yield_(v);
                } else {
                    self.adv(v, 7);
                }
            }
            0x28 | 0x29 => self.thread_call(v, opc),
            0x2a => {
                let (p, t) = (self.u8_at(v, 1) as u16, self.actor_at(v, 2));
                match self.vm_of(t) {
                    Some(tv) if !self.none_at_or_below(tv, p) => self.yield_(v),
                    _ => self.adv(v, 6),
                }
            }
            0x2f => {
                let (on, t) = (self.u8_at(v, 1) != 0, self.actor_at(v, 2));
                if let Some(tv) = self.vm_of(t) {
                    self.vms[tv].paused = on;
                }
                host.effect(Effect::Op { op: opc, sub, actor: t, args: vec![Arg::Int(on as i32)] });
                self.adv(v, 6);
            }
            0x32 => {
                self.bodies[body].speed = self.val(v, 1) as f32 * 0.1;
                self.adv(v, 3);
            }
            // FUN_100b7ce0: +0x16c = val * 10.0.
            0x91 => {
                self.bodies[body].anim_speed = self.val(v, 1) as f32 * 10.0;
                self.adv(v, 3);
            }
            // FUN_100b3da0: actor +0x120 bit 0x200000 = u8@1.
            0x33 => {
                self.bodies[body].no_ground = self.u8_at(v, 1) != 0;
                let a = self.args(v, op, sub);
                host.effect(Effect::Op { op: opc, sub, actor, args: a });
                self.adv(v, 2);
            }
            0x36 | 0x37 => {
                let (x, z, y) = (self.val(v, 1) as f32 * POS, self.val(v, 3) as f32 * POS, self.val(v, 5) as f32 * POS);
                if opc == 0x37 {
                    self.bodies[body].dir = self.val(v, 7) as f32 * DIR;
                }
                let y = if self.bodies[body].no_ground { y } else { host.ground(actor, [x, y, z]).unwrap_or(y) };
                self.bodies[body].pos = [x, y, z];
                let dir = self.bodies[body].dir;
                host.effect(Effect::Position { op: opc, actor, pos: [x, y, z], dir });
                self.adv(v, len);
            }
            // FUN_100b5f70: 0x37 on actor@1's event body (x val@5, z val@7, height val@9, direction val@0xb), drawn
            // there at once; nothing when the actor has no event VM.
            0xba => {
                let t = self.actor_at(v, 1);
                if let Some(tv) = self.vm_of(t) {
                    let tb = self.vms[tv].body;
                    let (x, z, y) = (self.val(v, 5) as f32 * POS, self.val(v, 7) as f32 * POS, self.val(v, 9) as f32 * POS);
                    self.bodies[tb].dir = self.val(v, 11) as f32 * DIR;
                    let y = if self.bodies[tb].no_ground { y } else { host.ground(t, [x, y, z]).unwrap_or(y) };
                    self.bodies[tb].pos = [x, y, z];
                    let dir = self.bodies[tb].dir;
                    host.effect(Effect::Position { op: 0x37, actor: t, pos: [x, y, z], dir });
                }
                self.adv(v, len);
            }
            0x39 => {
                self.bodies[body].dir = self.val(v, 1) as f32 * DIR;
                let (pos, dir) = (self.bodies[body].pos, self.bodies[body].dir);
                host.effect(Effect::Position { op: opc, actor, pos, dir });
                self.adv(v, 3);
            }
            0x4b => {
                let t = self.actor_at(v, 1);
                let d = self.val(v, 5) as f32 * DIR;
                if let Some(tv) = self.vm_of(t) {
                    let b = self.vms[tv].body;
                    self.bodies[b].dir = d;
                }
                let pos = self.vm_of(t).map_or([0.0; 3], |tv| self.bodies[self.vms[tv].body].pos);
                host.effect(Effect::Position { op: opc, actor: t, pos, dir: d });
                self.adv(v, 7);
            }
            0x3a | 0x3b => {
                let t = self.actor_at(v, 1);
                let (pos, dir) = self.vm_of(t).map_or((host.place(t).0, host.place(t).1), |tv| {
                    let b = &self.bodies[self.vms[tv].body];
                    (b.pos, b.dir)
                });
                if opc == 0x3a {
                    self.set_at(v, 5, 0, (dir.rem_euclid(std::f32::consts::TAU) / DIR) as i32 as u32);
                } else {
                    // vars x, z, height like the position operands
                    self.set_at(v, 5, 0, (pos[0] / POS) as i32 as u32);
                    self.set_at(v, 7, 0, (pos[2] / POS) as i32 as u32);
                    self.set_at(v, 9, 0, (pos[1] / POS) as i32 as u32);
                }
                self.adv(v, len);
            }
            0x3c | 0x3d => {
                let bit = self.val(v, 3);
                let words = self.val(v, 5);
                if bit >> 5 < words {
                    let raw = self.u16_at(v, 1);
                    let w = self.get(v, self.vms[v].block, raw, bit >> 5);
                    let m = 1u32 << (bit & 31);
                    self.set_raw(v, raw, bit >> 5, if opc == 0x3c { w | m } else { w & !m });
                }
                self.adv(v, 7);
            }
            0x3e => {
                let bit = self.val(v, 3);
                let w = self.get(v, self.vms[v].block, self.u16_at(v, 1), bit >> 5);
                if w & (1 << (bit & 31)) != 0 { self.adv(v, 7) } else { self.jump(v, 5) }
            }
            0x3f => {
                let (a, b) = (self.val(v, 3), self.val(v, 5));
                self.set_at(v, 1, 0, if b == 0 { 0 } else { a.wrapping_rem(b) } as u32);
                self.adv(v, 7);
            }
            0x40 | 0x41 => {
                let (lo, hi) = (self.val(v, 1), self.val(v, 3));
                let mask = (0..32).filter(|&i| lo <= i && i <= hi).fold(0u32, |m, i| m | 1 << i);
                if opc == 0x40 {
                    let (old, x) = (self.val(v, 5) as u32, self.val(v, 7) as u32);
                    self.set_at(v, 5, 0, (x.wrapping_shl(lo as u32) & mask) | (old & !mask));
                } else {
                    let x = self.val(v, 5) as u32;
                    self.set_at(v, 7, 0, (x & mask).wrapping_shr(lo as u32));
                }
                self.adv(v, 9);
            }
            0x43 => match sub {
                Some(0) => {
                    host.effect(Effect::Send(Packet::EventUpdate { head: self.head(), option: self.z[1] }));
                    self.wait_update = true;
                    self.adv(v, 2);
                }
                _ => {
                    if !self.wait_update {
                        self.adv(v, 2);
                    }
                    self.yield_(v);
                }
            },
            0x44 => {
                let t = self.resolve(v, self.val(v, 1) as u32);
                if host.ready(t) { self.adv(v, 5) } else { self.jump(v, 3) }
            }
            0x45 | 0x62 | 0x9f | 0xbb | 0xc5 | 0xcd | 0xd0 | 0xd5 => {
                if let Some((file, tag, a, t)) = self.sched(v, sched_base(opc)) {
                    if host.ready(a) && host.ready(t) {
                        let param = self.val(v, 0xf);
                        host.effect(Effect::Scheduler { file, tag, actor: a, target: t, param });
                    }
                }
                self.adv(v, 17);
            }
            0x52 | 0xa1 | 0xa3 | 0xbd | 0xc7 | 0xcf | 0xd2 | 0xd7 => {
                if let Some((file, tag, a, t)) = self.sched(v, sched_base(opc)) {
                    host.effect(Effect::SchedulerStop { file, tag, actor: a, target: t });
                }
                self.adv(v, 15);
            }
            0x55 | 0xa0 | 0xa2 | 0xbc | 0xc6 | 0xce | 0xd1 | 0xd6 => {
                let busy = self.sched(v, sched_base(opc)).is_some_and(|(file, tag, a, t)| {
                    host.ready(a) && host.ready(t) && host.busy(&Busy::Scheduler { file, tag, actor: a, target: t })
                });
                if busy { self.yield_(v) } else { self.adv(v, 15) }
            }
            0x46 => {
                match sub {
                    // Sub 3 also sets the point the camera starts from (not modelled; 4 sites in the data).
                    Some(0) | Some(1) | Some(3) => {
                        self.camera = sub != Some(1);
                        host.effect(Effect::Camera { flag: self.camera });
                    }
                    Some(2) => self.set_at(v, 2, 0, self.camera as u32),
                    _ => {}
                }
                self.adv(v, len);
            }
            0x47 => match sub {
                Some(0) => {
                    let (x, z, y) = (self.val(v, 2) as f32 * POS, self.val(v, 4) as f32 * POS, self.val(v, 6) as f32 * POS);
                    let dir = self.val(v, 8) as f32 * DIR;
                    host.effect(Effect::Send(Packet::EventPos { head: self.head(), x, y, z, dir, option: self.z[1] }));
                    self.wait_update = true;
                    self.wait_pos = true;
                    self.adv(v, 10);
                }
                _ => {
                    if !self.wait_update && !self.wait_pos {
                        self.adv(v, 2);
                    }
                    self.yield_(v);
                }
            },
            0x56 | 0x58 => {
                self.adv(v, len);
                self.yield_(v);
            }
            0x57 => {
                let x = self.val(v, 1).wrapping_add(self.dt as i32);
                self.set_at(v, 1, 0, x as u32);
                self.adv(v, 3);
            }
            0x5b | 0x66 => {
                let (a, t, file, name) = (self.actor_at(v, 3), self.actor_at(v, 7), self.val(v, 1), four_cc(self.u32_at(v, 0xb)));
                if self.thread(v).state == 0 {
                    host.effect(Effect::Motion { op: opc, actor: a, target: t, file, name });
                    self.thread(v).state = 1;
                }
                if !host.busy(&Busy::MotionLoad { actor: a, file }) {
                    self.thread(v).state = 0;
                    self.adv(v, len);
                }
                self.yield_(v);
            }
            // FUN_100b5490: subs 3..6 are the 0x5b / 0x66 forms one byte on (5 / 6 add a speed val@0x10).
            0x5f if matches!(sub, Some(3..=6)) => {
                let (a, t, file, name) = (self.actor_at(v, 4), self.actor_at(v, 8), self.val(v, 2), four_cc(self.u32_at(v, 0xc)));
                let bank = if matches!(sub, Some(3) | Some(5)) { 0x5b } else { 0x66 };
                if self.thread(v).state == 0 {
                    host.effect(Effect::Motion { op: bank, actor: a, target: t, file, name });
                    self.thread(v).state = 1;
                }
                if !host.busy(&Busy::MotionLoad { actor: a, file }) {
                    self.thread(v).state = 0;
                    self.adv(v, len);
                }
                self.yield_(v);
            }
            // Sub 7 (FUN_100b3170(0, 1)): waits on model vtable +0x2ac; taken as the 0x53 wait (inferred).
            0x5f if sub == Some(7) => {
                let (a, name) = (self.actor_at(v, 2), four_cc(self.u32_at(v, 10)));
                if host.ready(a) && host.busy(&Busy::Playing { actor: a, name }) {
                    self.yield_(v);
                } else {
                    self.adv(v, len);
                }
            }
            0x2c => {
                let (a, t, name) = (self.actor_at(v, 1), self.actor_at(v, 5), four_cc(self.u32_at(v, 9)));
                host.effect(Effect::Motion { op: opc, actor: a, target: t, file: 0, name });
                self.adv(v, len);
            }
            0x53 => {
                let (a, name) = (self.actor_at(v, 1), four_cc(self.u32_at(v, 9)));
                if host.ready(a) && host.busy(&Busy::Playing { actor: a, name }) {
                    self.yield_(v);
                } else {
                    self.adv(v, len);
                }
            }
            0x5e => {
                host.effect(Effect::IdleMotion { actor, name: four_cc(self.u32_at(v, 1)) });
                self.adv(v, 5);
            }
            0x6b => {
                let a = self.actor_at(v, 5);
                host.effect(Effect::IdleMotion { actor: a, name: four_cc(self.u32_at(v, 1)) });
                self.adv(v, 9);
            }
            0x69 => {
                // u8@1 = 0 restores full volume, else mutes.
                let level = if self.u8_at(v, 1) == 0 { 1.0 } else { 0.0 };
                host.effect(Effect::Volume { mask: self.val(v, 2), level, ticks: 0.0 });
                self.adv(v, 4);
            }
            0x6a => {
                let (level, ticks) = (self.val(v, 1) as f32 * POS, self.val(v, 3) as f32);
                host.effect(Effect::Volume { mask: self.val(v, 5), level, ticks });
                self.adv(v, 7);
            }
            0x6c => {
                let a = self.actor_at(v, 1);
                if self.thread(v).state == 0 {
                    host.effect(Effect::ActorFade { actor: a, alpha: self.val(v, 5), frames: self.val(v, 7) });
                    self.thread(v).state = 1;
                }
                if host.busy(&Busy::Op { op: opc, sub, actor: a }) {
                    self.yield_(v);
                } else {
                    self.thread(v).state = 0;
                    self.adv(v, 9);
                }
            }
            // FUN_100b5db0: waits while the actor's action state (+0x1b8) is busy, then starts the emote and
            // advances; 0x99 (FUN_100b5e50) waits while it plays.
            0x6e => {
                let a = self.actor_at(v, 1);
                if host.ready(a) && host.busy(&Busy::Op { op: 0x99, sub: None, actor: a }) {
                    self.yield_(v);
                } else {
                    let args = self.args(v, op, sub);
                    host.effect(Effect::Op { op: opc, sub, actor, args });
                    self.adv(v, 7);
                }
            }
            0x99 => {
                let a = self.actor_at(v, 1);
                if host.ready(a) && host.busy(&Busy::Op { op: 0x99, sub: None, actor: a }) {
                    self.yield_(v);
                } else {
                    self.adv(v, 5);
                }
            }
            0x70 | 0x76 => {
                let a = if opc == 0x70 { actor } else { self.actor_at(v, 1) };
                if host.ready(a) && host.busy(&Busy::Turning { actor: a }) {
                    self.yield_(v);
                } else {
                    self.adv(v, len);
                }
            }
            0x7a => self.context(v, sub, host),
            0x9d => self.table(v, sub),
            0xb1 => match sub {
                Some(s @ 0..=2) => {
                    let w = host.config_word();
                    let r = match s {
                        0 => (w >> 1) & 3,
                        1 => w & 1,
                        _ => 1,
                    };
                    self.set_at(v, 2, 0, r);
                    self.adv(v, 4);
                }
                _ => {
                    host.effect(Effect::Fault { actor, pc: pc as u16, op: opc });
                    self.end_thread(v);
                }
            },
            0xb2 => match sub {
                Some(0) => {
                    if self.thread(v).timer < 0.0 {
                        self.thread(v).timer = self.val(v, 1) as f32;
                    }
                    let dt = self.dt;
                    self.thread(v).timer -= dt;
                    let fin = self.thread(v).timer < 0.0;
                    self.yield_(v);
                    if fin {
                        self.adv(v, len);
                    }
                }
                _ => {
                    self.adv(v, len);
                }
            },
            0xbe => {
                let c = self.thread(v).caller;
                self.set_at(v, 1, 0, c);
                self.adv(v, 3);
            }
            0xda => {
                let v32 = host.game_config(0x43);
                self.set_at(v, 1, 0, v32);
                self.adv(v, len);
            }
            0x87 | 0x88 | 0x8c | 0xa6 if sub == Some(0) || sub == Some(2) => {
                host.effect(Effect::Send(Packet::Other { op: opc, sub: sub.unwrap_or(0) }));
                self.adv(v, len);
            }
            _ => {
                let a = self.args(v, op, sub);
                let busy = op.waits && host.busy(&Busy::Op { op: opc, sub, actor });
                if self.thread(v).state == 0 || !op.waits {
                    host.effect(Effect::Op { op: opc, sub, actor, args: a });
                }
                if busy {
                    self.thread(v).state = 1;
                    self.yield_(v);
                } else {
                    if op.waits {
                        self.thread(v).state = 0;
                    }
                    self.adv(v, len);
                }
            }
        }
    }

    /// FUN_100aead0: push and jump (the stack is per VM, 8 deep).
    fn call(&mut self, v: usize, to: u16, ret: u16) {
        let vm = &mut self.vms[v];
        if vm.stack.len() == 8 {
            vm.yielded = true;
            return;
        }
        vm.stack.push(ret);
        vm.pc = to;
    }

    /// FUN_100b1090 / FUN_100b4940: turn toward the target (its VM body when bound) and look at it; nothing
    /// when the target does not resolve.
    fn face(&mut self, v: usize, who: u32, target: u32, host: &mut dyn Host) {
        if self.vm_of(target).is_none() && !host.ready(target) {
            return;
        }
        let to = self.vm_of(target).map_or(host.place(target).0, |t| self.bodies[self.vms[t].body].pos);
        let wv = if who == self.vms[v].actor { Some(v) } else { self.vm_of(who) };
        if let Some(w) = wv {
            let b = self.vms[w].body;
            let p = self.bodies[b].pos;
            self.bodies[b].dir = (-(to[2] - p[2])).atan2(to[0] - p[0]);
        }
        host.effect(Effect::Face { actor: who, target });
    }

    /// 1f / 5a sub 1 (FUN_100b1210 / FUN_100b1560): step toward the slot target at speed * ticks / 60.
    fn walk(&mut self, v: usize, three_d: bool, host: &mut dyn Host) {
        let body = self.vms[v].body;
        let actor = self.vms[v].actor;
        let tg = self.thread(v).target;
        let b = &mut self.bodies[body];
        let d = [tg[0] - b.pos[0], tg[1] - b.pos[1], tg[2] - b.pos[2]];
        b.dir = (-d[2]).atan2(d[0]);
        let dist = if three_d { (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() } else { (d[0] * d[0] + d[2] * d[2]).sqrt() };
        let step = self.dt * b.speed / 60.0;
        let arrived = dist <= step;
        if arrived {
            b.pos = [tg[0], tg[1], tg[2]];
        } else {
            let k = step / dist;
            b.pos[0] += d[0] * k;
            b.pos[2] += d[2] * k;
            if three_d {
                b.pos[1] += d[1] * k;
            }
        }
        if !three_d {
            b.pos[1] = tg[1];
            if !b.no_ground
                && let Some(g) = host.ground(actor, b.pos)
            {
                b.pos[1] = g;
            }
        }
        if arrived {
            self.adv(v, 2);
        }
        self.yield_(v);
    }

    /// 0x24 (FUN_100b0960): show the question, then build the menu once its text is ready.
    fn choice_op(&mut self, v: usize, host: &mut dyn Host) {
        if !self.choice.ready {
            if !self.choice.requested {
                let id = self.val(v, 1) as u32;
                let (_, _, e) = self.text(id, None, host);
                let mut options = Vec::new();
                let mut question = String::new();
                let mut cursor = 0;
                if let Some(e) = e {
                    let at = e.choice.unwrap_or(e.lines.len());
                    question = e.lines[..at.min(e.lines.len())].iter().map(|l| decode_sjis(l)).collect::<Vec<_>>().join("\n");
                    let (default, mut mask) = (self.val(v, 3) as u32, self.val(v, 5) as u32);
                    for (i, l) in e.lines.iter().skip(at).enumerate() {
                        if mask & 1 == 0 {
                            if i as u32 == default {
                                cursor = options.len();
                            }
                            options.push((i as u32, decode_sjis(l)));
                        }
                        mask >>= 1;
                    }
                }
                host.effect(Effect::Choice { id, question, options, cursor });
                self.last_speaker = None;
                self.choice.requested = true;
            } else {
                // The message system has the text a frame later (DAT_10476ef0).
                self.choice.ready = true;
            }
            self.yield_(v);
            return;
        }
        self.choice.open = true;
        self.adv(v, 7);
    }

    /// 0x25 (FUN_100b0cb0): Z0 = option - 1; cancel = 0xfe and the event aborts.
    fn wait_choice(&mut self, v: usize) {
        if !self.choice.open {
            self.adv(v, 1);
            self.yield_(v);
            return;
        }
        if let Some(p) = self.choice.picked {
            self.adv(v, 1);
            self.choice = ChoiceState::default();
            if p == 0xff {
                self.z[0] = 0xfe;
                self.ending = true;
                self.end_phase = 1;
                self.cancelled = true;
            } else {
                self.z[0] = p as u32 - 1;
            }
        }
        self.yield_(v);
    }

    /// 0x28 (FUN_100b21c0) / 0x29 (FUN_100b2360): start a thread and wait for it.
    fn thread_call(&mut self, v: usize, opc: u8) {
        let (t, e, p) = (self.actor_at(v, 2), self.u8_at(v, 6), self.u8_at(v, 1) as u16);
        let state = self.thread(v).state;
        let finish = |rt: &mut Self| {
            rt.thread(v).state = 0;
            rt.adv(v, 7);
        };
        if state == 0 {
            match self.start_thread(v, t, e, p) {
                1 => {
                    self.thread(v).state = 1;
                    self.yield_(v);
                }
                2 => self.yield_(v),
                _ => finish(self),
            }
            return;
        }
        let Some(tv) = self.vm_of(t) else { return finish(self) };
        let h = self.has_entry(tv, e);
        match (opc, state) {
            (0x28, _) if h == 1 => self.yield_(v),
            (0x28, _) => finish(self),
            (_, 1) => {
                if h != 0 && h != -1 {
                    self.yield_(v);
                } else {
                    self.thread(v).state = 2;
                    self.yield_(v);
                }
            }
            _ if h != -1 => self.yield_(v),
            _ => finish(self),
        }
    }

    /// 0x7a (FUN_100b1dc0): kill threads, switch the acting actor, end handlers.
    fn context(&mut self, v: usize, sub: Option<u8>, host: &mut dyn Host) {
        match sub {
            Some(0) | Some(1) => {
                let t = self.actor_at(v, 2);
                let e = self.u8_at(v, 6);
                if let Some(tv) = self.vm_of(t) {
                    let vm = &mut self.vms[tv];
                    if sub == Some(0) {
                        vm.stack.clear();
                        vm.cur = 0;
                        for th in vm.threads.iter_mut().take(16) {
                            *th = Thread::free();
                        }
                    } else if let Some(th) = vm.threads.iter_mut().take(16).find(|th| th.entry == e) {
                        *th = Thread::free();
                    }
                }
                self.adv(v, if sub == Some(0) { 6 } else { 7 });
            }
            Some(2) => {
                let t = self.actor_at(v, 2);
                if let Some(tv) = self.vm_of(t) {
                    self.vms[v].actor = t;
                    self.vms[v].body = tv;
                    self.adv(v, 6);
                }
                self.yield_(v);
            }
            Some(3) => {
                self.vms[v].actor = self.vms[v].own;
                self.vms[v].body = v;
                self.adv(v, 2);
                self.yield_(v);
            }
            Some(4) => {
                let t = self.actor_at(v, 3);
                let (e, p) = (self.u8_at(v, 7), self.u8_at(v, 2) as u16);
                let caller = self.vms[v].actor;
                let r = match self.vm_of(t) {
                    Some(tv) if self.vms[tv].threads[16].prio != FREE => 2,
                    Some(tv) => {
                        let pc = self.blocks[self.vms[tv].block].events.get(e as usize).map_or(0, |x| x.1);
                        self.vms[tv].threads[16] = Thread { prio: p, pc, entry: e, caller, ..Thread::free() };
                        1
                    }
                    None => -1,
                };
                if r == 2 { self.yield_(v) } else { self.adv(v, 8) }
            }
            Some(5) => {
                let t = self.actor_at(v, 2);
                if let Some(tv) = self.vm_of(t) {
                    self.vms[tv].threads[16].prio = FREE;
                }
                self.adv(v, 6);
            }
            _ => {
                let (actor, pc) = (self.vms[v].actor, self.vms[v].pc);
                host.effect(Effect::Fault { actor, pc, op: 0x7a });
                self.end_thread(v);
            }
        }
    }

    /// 0x9d (FUN_100b8300): tables in the code, pointer slots, table calls, string compares.
    fn table(&mut self, v: usize, sub: Option<u8>) {
        let s = sub.unwrap_or(0xff);
        let pc = self.vms[v].pc;
        let blk = self.vms[v].block;
        // Bound: 0 means none; an index at or past it reads entry 0.
        let bounded = |idx: i32, bound: i32| if bound != 0 && bound <= idx { 0 } else { idx };
        match s {
            0 | 0xa => {
                let bound = if s == 0xa { self.val(v, 8) as i16 as i32 } else { 0 };
                let idx = bounded(self.val(v, 6) as i16 as i32, bound);
                let tab = self.u16_at(v, 2) as usize;
                let raw = rd16(&self.blocks[blk].code, tab + (idx * 2) as usize);
                let x = self.get(v, blk, raw, 0);
                self.set_at(v, 4, 0, x);
                self.adv(v, if s == 0 { 8 } else { 10 });
            }
            1 | 0xb => {
                let bound = if s == 0xb { self.val(v, 8) as i16 as i32 } else { 0 };
                let idx = bounded(self.val(v, 6) as i16 as i32, bound);
                let at = self.u16_at(v, 2) as usize + (idx << 4) as usize;
                let c = &self.blocks[blk].code;
                let mut w = [rd32(c, at), rd32(c, at + 4), rd32(c, at + 8), rd32(c, at + 12)];
                w[3] &= 0xff_ffff;
                let raw = self.u16_at(v, 4);
                self.set4(v, raw, 0, w);
                self.adv(v, if s == 1 { 8 } else { 10 });
            }
            2 | 0xc => {
                let bound = if s == 0xc { self.val(v, 6) as i16 as i32 } else { 0 };
                let slot = self.val(v, 4) as i16 as i32;
                let slot = if bound != 0 && bound <= slot { 0 } else { slot };
                if (0..0x40).contains(&slot) {
                    self.slots[slot as usize] = Some((blk, self.u16_at(v, 2)));
                }
                self.adv(v, if s == 2 { 6 } else { 8 });
            }
            3 | 0xd | 4 | 0xe => {
                let long = s >= 0xd;
                let bound = if long { self.val(v, 8) as i16 as i32 } else { 0 };
                let slot = self.val(v, 2) as i16 as i32;
                let Some((sb, off)) = (0..0x40).contains(&slot).then(|| self.slots[slot as usize]).flatten() else {
                    self.adv(v, 8);
                    return;
                };
                let idx = bounded(self.val(v, 6) as i16 as i32, bound);
                if s == 3 || s == 0xd {
                    let raw = rd16(&self.blocks[sb].code, off as usize + (idx * 2) as usize);
                    let x = self.get(v, sb, raw, 0);
                    self.set_at(v, 4, 0, x);
                } else {
                    let c = &self.blocks[sb].code;
                    let at = off as usize + (idx << 4) as usize;
                    let mut w = [rd32(c, at), rd32(c, at + 4), rd32(c, at + 8), rd32(c, at + 12)];
                    w[3] &= 0xff_ffff;
                    let raw = self.u16_at(v, 4);
                    self.set4(v, raw, 0, w);
                }
                self.adv(v, if long { 10 } else { 8 });
            }
            5 | 0xf => {
                let bound = if s == 0xf { self.val(v, 8) as i16 as i32 } else { 0 };
                let idx = bounded(self.val(v, 6) as i16 as i32, bound);
                let x = self.val(v, 4) as u32;
                let tab = self.u16_at(v, 2) as usize;
                let raw = rd16(&self.blocks[blk].code, tab + (idx * 2) as usize);
                self.set_raw(v, raw, 0, x);
                self.adv(v, if s == 5 { 8 } else { 10 });
            }
            6 | 0x10 => {
                let bound = if s == 0x10 { self.val(v, 8) as i16 as i32 } else { 0 };
                let st = self.str_at(v, 4);
                let idx = bounded(self.val(v, 6) as i16 as i32, bound);
                let tab = self.u16_at(v, 2) as usize;
                let raw = rd16(&self.blocks[blk].code, tab + (idx << 4) as usize);
                let mut w = [0u32; 4];
                for (i, x) in w.iter_mut().enumerate() {
                    *x = u32::from_le_bytes(st[i * 4..i * 4 + 4].try_into().unwrap());
                }
                w[3] &= 0xff_ffff;
                self.set4(v, raw, 0, w);
                self.adv(v, if s == 6 { 8 } else { 10 });
            }
            7 => {
                let idx = self.val(v, 4) as i16 as i32;
                let tab = self.u16_at(v, 2) as usize;
                let to = rd16(&self.blocks[blk].code, tab + (idx * 2) as usize);
                self.call(v, to, pc.wrapping_add(6));
            }
            8 | 9 => {
                let a = self.str_at(v, 2);
                let (b, mode, fail_at, len) = if s == 8 {
                    let at = pc as usize + 5;
                    let mut b = [0u8; 16];
                    if let Some(x) = self.code(v).get(at..at + 16) {
                        b.copy_from_slice(x);
                    }
                    (b, self.u8_at(v, 4) & 0xf, 0x15, 0x17)
                } else {
                    (self.str_at(v, 4), self.u8_at(v, 6) & 0xf, 7, 9)
                };
                let cstr = |x: &[u8; 16]| x.iter().position(|&c| c == 0).map_or(x.to_vec(), |n| x[..n].to_vec());
                let eq = cstr(&a) == cstr(&b);
                let pass = match mode {
                    0 => eq,
                    1 => !eq,
                    _ => false,
                };
                if pass { self.adv(v, len) } else { self.jump(v, fail_at) }
            }
            _ => self.adv(v, 2),
        }
    }
}

/// Scheduler file bases by opcode (FUN_100b26f0 / FUN_100b2d30 / FUN_100b2e50 callers).
fn sched_base(op: u8) -> u32 {
    match op {
        0x45 | 0x52 | 0x55 => 0x77f0,
        0x62 | 0xa0 | 0xa1 => 0x1394,
        0x9f | 0xa2 | 0xa3 => 0xc7ef,
        0xbb | 0xbc | 0xbd => 0xdd6d,
        0xc5 | 0xc6 | 0xc7 => 0x1071b,
        0xcd | 0xce | 0xcf => 0x11323,
        0xd0 | 0xd1 | 0xd2 => 0x11423,
        _ => 0x19031,
    }
}

/// Headless host: records effects, answers choices from a script, keeps schedulers and motions
/// busy for a fixed number of frames.
pub struct Scripted {
    pub log: Vec<Effect>,
    pub choices: std::collections::VecDeque<Option<u32>>,
    /// Frames a started scheduler / motion stays busy.
    pub busy_frames: u32,
    /// Params the server answers an update (0x05B mode 1) with (0x05C).
    pub update_params: Option<[u32; 8]>,
    /// Cancel the n-th message (0-based) instead of closing it.
    pub cancel: Option<usize>,
    /// Scheduler length in ticks by (file, tag, param); None = unknown or looping.
    pub sched_ticks: Option<Box<dyn FnMut(u32, &str, i32) -> Option<f32>>>,
    running: Vec<(Busy, u32)>,
    seed: u32,
    frame: u32,
}

/// Scheduler length as CMoSchedularTask plays it (FUN_10056e60): each op adds its wait (u16 at +4)
/// times the speed; speed = param / total when the event passes param > 1 (FUN_10056430), and
/// param 1 loops until stopped (None).
pub fn scheduler_ticks(file: &[u8], tag: &str, param: i32) -> Option<f32> {
    if param == 1 {
        return None;
    }
    if param > 1 {
        return Some(param as f32);
    }
    let c = crate::dat::chunks(file).ok()?.into_iter().find(|c| c.kind == 0x07 && c.name() == tag)?;
    let (pay, mut p, mut total) = (c.data, 0x40, 0u32);
    while p + 2 <= pay.len() {
        let (op, len) = (pay[p], pay[p + 1] as usize);
        if len == 0 || p + 4 * len > pay.len() {
            break;
        }
        if len >= 2 {
            total += u16::from_le_bytes([pay[p + 4], pay[p + 5]]) as u32;
        }
        p += 4 * len;
        if op == 0 {
            break;
        }
    }
    Some(total as f32)
}

/// A `sched_ticks` source reading the scheduler files from the install (cached).
pub fn sched_timer(index: crate::dat::DatIndex) -> Box<dyn FnMut(u32, &str, i32) -> Option<f32>> {
    let mut cache: std::collections::HashMap<u32, Option<Vec<u8>>> = Default::default();
    Box::new(move |file, tag, param| {
        let data = cache.entry(file).or_insert_with(|| index.read(file).ok());
        scheduler_ticks(data.as_deref()?, tag, param)
    })
}

impl Scripted {
    pub fn new(choices: &[Option<u32>]) -> Scripted {
        Scripted {
            log: Vec::new(),
            choices: choices.iter().copied().collect(),
            busy_frames: 30,
            update_params: None,
            cancel: None,
            sched_ticks: None,
            running: Vec::new(),
            seed: 1,
            frame: 0,
        }
    }
}

impl Host for Scripted {
    fn effect(&mut self, e: Effect) {
        let until = self.frame + self.busy_frames;
        match &e {
            Effect::Scheduler { file, tag, actor, target, param } => {
                let len = self.sched_ticks.as_mut().map_or(Some(self.busy_frames as f32), |f| f(*file, tag, *param));
                let until = len.map_or(u32::MAX, |t| self.frame + t.ceil() as u32 + 1);
                self.running.push((Busy::Scheduler { file: *file, tag: tag.clone(), actor: *actor, target: *target }, until))
            }
            Effect::SchedulerStop { file, tag, actor, target } => {
                let b = Busy::Scheduler { file: *file, tag: tag.clone(), actor: *actor, target: *target };
                self.running.retain(|(x, _)| *x != b);
            }
            Effect::Motion { actor, file, name, .. } => {
                // Files load in a couple of frames; the motion itself plays busy_frames.
                self.running.push((Busy::MotionLoad { actor: *actor, file: *file }, self.frame + 2));
                self.running.push((Busy::Playing { actor: *actor, name: name.clone() }, until));
            }
            _ => {}
        }
        self.log.push(e);
    }

    fn busy(&mut self, b: &Busy) -> bool {
        self.running.iter().any(|(x, until)| x == b && *until > self.frame)
    }

    /// Deterministic LCG (scripts loop on rand until it differs).
    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.seed >> 16) & 0x7fff
    }
}

/// Runs an event to the end: closes each message after 2 frames, answers choices from the script,
/// acknowledges server round trips. Returns the frames used, or None when it did not finish.
pub fn run_scripted(rt: &mut Runtime, host: &mut Scripted, max_frames: u32) -> Option<u32> {
    let mut open_for = 0;
    let mut shown = 0;
    // Unscripted choices: the last option the first time a menu shows (menus loop until "Never
    // mind"), the first option when it shows again (paged menus).
    let mut last = 0;
    let mut seen = Vec::new();
    for f in 0..max_frames {
        host.frame = f;
        let before = host.log.len();
        rt.tick(host, 1.0);
        if rt.done() {
            return Some(f + 1);
        }
        let mut update = false;
        let mut pos = false;
        for e in &host.log[before..] {
            match e {
                Effect::Send(Packet::EventUpdate { .. }) => update = true,
                Effect::Send(Packet::EventPos { .. }) => pos = true,
                Effect::Choice { id, options, .. } => {
                    let again = seen.contains(id);
                    seen.push(*id);
                    let pick = if again { options.first() } else { options.last() };
                    last = pick.map_or(0, |o| o.0);
                }
                _ => {}
            }
        }
        if update {
            if let Some(p) = host.update_params {
                rt.params(p);
            }
        }
        if update || pos {
            rt.release(1, rt.start.event);
        }
        if pos {
            rt.position_ack();
        }
        if rt.choice_open() {
            let c = host.choices.pop_front().unwrap_or(Some(last));
            rt.choose(c);
        }
        if rt.message_open() {
            open_for += 1;
            if host.cancel == Some(shown) {
                rt.cancel_message();
            } else if open_for >= 2 {
                rt.close_message();
                open_for = 0;
                shown += 1;
            }
        }
    }
    None
}

/// `event-run`: runs one event headless and prints its effects.
pub fn run_cli(zone: u16, event: u16, target: u32, params: [u32; 8], choices: &[Option<u32>]) -> anyhow::Result<()> {
    let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
    let blocks = event::load(&index, zone)?;
    let dialog = Dialog::load(&index, zone).ok();
    let mut host = Scripted::new(choices);
    host.sched_ticks = Some(sched_timer(crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir())?));
    let start = Start { zone, event, target, params, strings: Vec::new() };
    let mut rt = Runtime::start(blocks, dialog, start, Memory::default(), &mut host);
    if let Ok(n) = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En) {
        rt.set_names(std::sync::Arc::new(n));
    }
    let frames = run_scripted(&mut rt, &mut host, 60 * 60 * 20);
    for e in &host.log {
        println!("{}", describe(e));
    }
    for (a, b) in rt.actors() {
        println!("actor {a:08x} at {:.2} {:.2} {:.2} dir {:.2}", b.pos[0], b.pos[1], b.pos[2], b.dir);
    }
    println!("camera flag {}", rt.camera());
    match frames {
        Some(f) => println!("finished in {f} frames ({:.1} s at 60 Hz)", f as f32 / 60.0),
        None => {
            println!("did not finish; live threads:");
            for (a, t) in rt.threads().into_iter().filter(|t| !t.1.is_empty()) {
                println!("  {a:08x}: {:x?}", t);
            }
        }
    }
    Ok(())
}

/// `event-sweep`: runs every event id of each zone headless (first option at every choice) and counts
/// how many finish, fault or hang.
pub fn sweep(zones: &[u16]) -> anyhow::Result<()> {
    let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
    let zones: Vec<u16> = if zones.is_empty() { (0..300).collect() } else { zones.to_vec() };
    let (mut total, mut fin, mut faults, mut hung, mut idle) = (0, 0, 0, 0, 0);
    for z in zones {
        let Ok(blocks) = event::load(&index, z) else { continue };
        let mut ids: Vec<(u16, u32)> = Vec::new();
        for b in &blocks {
            for &(id, _) in &b.events {
                if id < 0xfffe && !ids.iter().any(|x| x.0 == id) {
                    ids.push((id, b.actor));
                }
            }
        }
        let (mut zf, mut zh, mut zi) = (Vec::new(), Vec::new(), Vec::new());
        for &(id, actor) in &ids {
            let mut host = Scripted::new(&[]);
            host.sched_ticks = Some(sched_timer(crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir())?));
            let start = Start { zone: z, event: id, target: actor, ..Default::default() };
            let mut rt = Runtime::start(event::load(&index, z)?, Dialog::load(&index, z).ok(), start, Memory::default(), &mut host);
            let done = run_scripted(&mut rt, &mut host, 60 * 60 * 5).is_some();
            for e in &host.log {
                if let Effect::Fault { actor, pc, op } = e {
                    zf.push(format!("{id:x}:{:03x}@{pc:04x}={op:02x}", actor & 0xfff));
                }
            }
            total += 1;
            if done {
                fin += 1;
            } else if rt.idle() {
                zi.push(id);
            } else {
                zh.push(id);
            }
        }
        faults += zf.len();
        hung += zh.len();
        idle += zi.len();
        println!(
            "zone {z}: {} events, {} finished, idle {:x?}, running {:x?}, faults {:?}",
            ids.len(),
            ids.len() - zh.len() - zi.len(),
            zi,
            zh,
            zf
        );
    }
    println!(
        "total {total} events: {fin} finished, {idle} idle (all threads ended without 0x21; the server ends these), \
         {hung} still running after 5 minutes, {faults} faults"
    );
    Ok(())
}

/// One line per effect.
pub fn describe(e: &Effect) -> String {
    let who = |a: &u32| if *a == PLAYER { "player".to_string() } else { format!("npc{:03x}", a & 0xfff) };
    match e {
        Effect::Message { op, id, text, .. } => format!("message {op:02x} #{id}: {}", text.replace('\n', " / ")),
        Effect::Choice { id, question, options, cursor } => format!(
            "choice #{id} \"{}\" [{}] cursor {cursor}",
            question.replace('\n', " / "),
            options.iter().map(|(i, t)| format!("{i}:{t}")).collect::<Vec<_>>().join(", ")
        ),
        Effect::Scheduler { file, tag, actor, target, param } => {
            format!("scheduler {file} '{tag}' {} -> {} param {param}", who(actor), who(target))
        }
        Effect::SchedulerStop { file, tag, actor, .. } => format!("scheduler stop {file} '{tag}' {}", who(actor)),
        Effect::Motion { op, actor, file, name, .. } => format!("motion {op:02x} {} file {file} '{name}'", who(actor)),
        Effect::IdleMotion { actor, name } => format!("idle {} '{name}'", who(actor)),
        Effect::Position { actor, pos, dir, .. } => {
            format!("position {} {:.2} {:.2} {:.2} dir {:.2}", who(actor), pos[0], pos[1], pos[2], dir)
        }
        Effect::Face { actor, target } => format!("face {} -> {}", who(actor), who(target)),
        Effect::Wait { actor, ticks } => format!("wait {} {ticks}", who(actor)),
        Effect::Op { op, sub, actor, args } => {
            format!("op {op:02x}{} {} {:?}", sub.map_or(String::new(), |s| format!(".{s}")), who(actor), args)
        }
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(z: u16, event: u16, choices: &[Option<u32>]) -> Option<(Vec<Effect>, Option<u32>)> {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else {
            eprintln!("skipped: no FFXI install");
            return None;
        };
        let blocks = event::load(&index, z).ok()?;
        let dialog = Dialog::load(&index, z).ok();
        let mut host = Scripted::new(choices);
        host.sched_ticks = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok().map(sched_timer);
        let start = Start { zone: z, event, target: 0x010e_601b, ..Default::default() };
        let mut rt = Runtime::start(blocks, dialog, start, Memory::default(), &mut host);
        let f = run_scripted(&mut rt, &mut host, 60 * 60 * 20);
        Some((host.log, f))
    }

    fn messages(log: &[Effect]) -> Vec<u32> {
        log.iter().filter_map(|e| if let Effect::Message { id, .. } = e { Some(*id) } else { None }).collect()
    }

    fn end(log: &[Effect]) -> Option<u32> {
        log.iter().find_map(|e| if let Effect::Send(Packet::EventEnd { option, .. }) = e { Some(*option) } else { None })
    }

    /// Southern San d'Oria actor 0x01b, event 0x0249: the Mog House explanation and its choice.
    #[test]
    fn mog_house_choice() {
        let Some((log, f)) = run(230, 0x0249, &[Some(0)]) else { return };
        assert!(f.is_some());
        assert_eq!(messages(&log), vec![0, 1, 3, 4, 5]);
        assert!(log.iter().any(|e| matches!(e, Effect::Choice { id: 2, options, .. } if options.len() == 2)));
        assert_eq!(end(&log), Some(0));

        let Some((log, _)) = run(230, 0x0249, &[Some(1)]) else { return };
        assert_eq!(messages(&log), vec![0, 1, 8]);
        assert_eq!(end(&log), Some(1));

        let Some((log, _)) = run(230, 0x0249, &[None]) else { return };
        assert_eq!(end(&log), Some(0x4000_0000));
    }

    /// Event 0x024a reads the 0x034 param num0 in its text; a cancelled message aborts the event.
    #[test]
    fn params_and_cancel() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let (Ok(blocks), dialog) = (event::load(&index, 230), Dialog::load(&index, 230).ok()) else { return };
        let mut host = Scripted::new(&[Some(1)]);
        let start = Start { zone: 230, event: 0x024a, target: 0x010e_601b, ..Default::default() };
        let mut rt = Runtime::start(event::load(&index, 230).unwrap(), dialog, start, Memory::default(), &mut host);
        rt.params([1, 0, 0, 0, 0, 0, 0, 0]);
        run_scripted(&mut rt, &mut host, 60 * 60).expect("finished");
        let t = host.log.iter().find_map(|e| if let Effect::Message { id: 10, text, .. } = e { Some(text.clone()) } else { None });
        assert!(t.expect("message 10").contains("Bastok"));

        let mut host = Scripted::new(&[]);
        host.cancel = Some(1);
        let start = Start { zone: 230, event: 0x0249, target: 0x010e_601b, ..Default::default() };
        let mut rt = Runtime::start(blocks, Dialog::load(&index, 230).ok(), start, Memory::default(), &mut host);
        run_scripted(&mut rt, &mut host, 600).expect("finished");
        assert_eq!(messages(&host.log), vec![0, 1]);
        assert_eq!(end(&host.log), Some(0x4000_0000));
    }

    /// The opening cutscene: player block event 0x01f7.
    #[test]
    fn sandoria_opening() {
        let Some((log, f)) = run(230, 0x01f7, &[]) else { return };
        assert!(f.is_some(), "did not finish");
        let scheds: Vec<String> = log
            .iter()
            .filter_map(|e| if let Effect::Scheduler { file: 30834, tag, .. } = e { Some(tag.clone()) } else { None })
            .collect();
        assert_eq!(&scheds[..3], &["s043", "s044", "s045"]);
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).unwrap();
        let evte = index.read(30834).unwrap();
        assert_eq!(scheduler_ticks(&evte, "s043", 0), Some(600.0));
        assert_eq!(scheduler_ticks(&evte, "s043", 1), None);
        assert_eq!(messages(&log)[0], 7668);
        assert!(end(&log).is_some());
    }
}
