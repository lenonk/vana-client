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
    /// c2s 0x060 PASSWARDS (op 0x71 sub 1): the input window's 16 bytes for the event's target.
    Passwords { head: Head, text: [u8; 16] },
    /// Any other c2s packet: id and body after the 4-byte header (0x01B, 0x058, 0x0EB).
    Raw { id: u16, body: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Dialogue line; the window stays open until [`Runtime::close_message`].
    /// `place`: the text's 02 / 03 window position codes.
    /// `mode`: a 1f code's (msg+4); `sounds`: the 0d / 0e SeSep ids played as the text was built.
    Message { op: u8, speaker: Option<u32>, id: u32, text: String, prompt: bool, wait: Option<u32>, place: [Option<i16>; 2], mode: Option<u8>, sounds: Vec<u32> },
    /// Choice menu (op 0x24); answer with [`Runtime::choose`].
    Choice { id: u32, question: String, options: Vec<(u32, String)>, cursor: usize },
    /// Scheduler start (0x45 and family): file id (base + n), tag, actor, target, param.
    Scheduler { file: u32, tag: String, actor: u32, target: u32, param: i32 },
    SchedulerStop { file: u32, tag: String, actor: u32, target: u32 },
    /// 0x46 sub 0 / 1: FUN_10082b20 (flag = DAT_10346088 = 1, camera seeded from the player) /
    /// FUN_10082be0 (flag = 0). Cutscenes run with 0 and set 1 before they end. Sub 3 seeds it at `offset` in the
    /// player's frame (x forward, y down) instead of FUN_10021680's (-4, -2, 0).
    Camera { flag: bool, offset: Option<[f32; 3]> },
    /// Op 0x71: open "menu    passinpu" (kind: title and allowed characters; max bytes, at most 16); answer with
    /// [`Runtime::input_done`].
    Input { kind: i32, max: usize },
    /// Sound volume, 0x69 set / 0x6a fade (FUN_10035200.. / FUN_10034be0): mask 1 world sound effects
    /// (DAT_1034656c), 2 interface sounds (DAT_10346568), 4 music (DAT_10346570), 8 all of them and
    /// the stream at DAT_10450fe8; 0x10 (0x69) sets DAT_10346578. Reaches `level` over `ticks`.
    Volume { mask: i32, level: f32, ticks: f32 },
    /// Actor alpha fade (0x6c).
    ActorFade { actor: u32, alpha: i32, frames: i32 },
    /// FUN_1008af20: the actor's turning steps ('@tl0' / '@tr0') stop.
    StopTurn { actor: u32 },
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
    /// 0x54: the zone action (a scheduler of the zone's effects) `name` on (a, b) still running (FUN_100b50f0).
    ZoneAction { name: String, a: u32, b: u32 },
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
    /// The client's side of the text codes (date strings, the roster, other name tables).
    fn text_ctx(&self) -> Ctx {
        Ctx::default()
    }
    /// Op 0xcc 0x11: None while arealist's event pick is up (+0x6d), then its pick (+0x6e: a zone, -1 cancelled).
    fn area_pick(&mut self) -> Option<i32> {
        Some(-1)
    }
    /// Start position (x, height, z) and direction of a bound actor.
    fn place(&self, _actor: u32) -> ([f32; 3], f32) {
        ([0.0; 3], 0.0)
    }
    /// Ground height under a point (FUN_1017a390 / FUN_1017a400); None = no snap.
    fn ground(&self, _actor: u32, _p: [f32; 3]) -> Option<f32> {
        None
    }
    /// Player bytes read by values 0x7f06 (low) / 0x7f08 (next): main job and its level (DAT_10485640 / 41, s2c 0x061
    /// @0x0C / @0x0D).
    fn player_word(&self) -> u32 {
        0
    }
    /// Values 0x7f07 / 0x7f87: an actor's race byte (+0xef); 0 for non-PC looks.
    fn race(&self, _actor: u32) -> u8 {
        0
    }
    /// Values 0x7f0a / 0x7f8a: an actor's server id (+0x78).
    fn server_id(&self, actor: u32) -> u32 {
        actor
    }
    /// A game config value by id (op 0xda reads 0x43, the aspect).
    fn game_config(&self, _id: u16) -> u32 {
        0
    }

    /// Config word read by op 0xb1 (*(DAT_10444110 + 0x18)).
    fn config_word(&self) -> u32 {
        0
    }
    /// FUN_100b2550(6): op 0xd4 sub 0 waits while the map's image loads or before the key item table came.
    fn map_ready(&self) -> bool {
        true
    }
    fn rand(&mut self) -> u32 {
        0
    }
    /// The client language (DAT_10456a28 + 8): 0 Japanese, 1 English, 2 / 3 the European ones.
    fn language(&self) -> u32 {
        1
    }
    /// An actor's ActIndex (FUN_100afb00's second result); None when it is not in the zone.
    fn index(&self, actor: u32) -> Option<u16> {
        Some(if actor == PLAYER { 0 } else { (actor & 0xfff) as u16 })
    }
    /// Op 0x72: the three weather bytes of `zone` on day `day` (files 7032 / 7033, or 7036 / 7037 from zone 100);
    /// None when the files cannot be read.
    fn weather_day(&mut self, _zone: u32, _day: u32) -> Option<[u8; 3]> {
        None
    }
    /// Op 0x82: the zone RID region named by 4cc `tag` holds `p` (FUN_1017ad70 / FUN_1017ad80); None when there is no
    /// such region.
    fn in_region(&self, _tag: u32, _p: [f32; 3]) -> Option<bool> {
        Some(true)
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
    /// Op 0x31's frame limit (the same +0x18 word, as a short).
    frames: i16,
    caller: u32,
}

impl Thread {
    fn free() -> Thread {
        Thread { prio: FREE, pc: 0, target: [0.0; 4], entry: 0, state: 0, timer: -1.0, frames: 0, caller: 0 }
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
    /// Rotation x / z in radians (+0x150 / +0x158), op 0xd8.
    pub tilt: [f32; 2],
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
    /// Op 0xb7 sub 4's actor indices (DAT_10480a94, cleared to 0xffff).
    actor_slots: [u16; 9],
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
    /// Op 0x46 sub 3's offset, taken once a run (DAT_104897ea bit 0).
    cam_seed: Option<[f32; 3]>,
    /// DAT_10470548: actor of the last message.
    last_speaker: Option<u32>,
    names: Option<std::sync::Arc<dyn crate::dat::names::NameSource>>,
    player_word: u32,
    /// Op 0xa7 (body +0x180 bit 0 / +0x184): a registration answer is awaited, and the answer (s2c 0x0BF).
    reg_armed: bool,
    reg_value: u32,
    /// Answers awaited for ops 0x8c (DAT_10480a90), 0x87 / 0x88 (DAT_1048557a), 0xa6 (DAT_104804c0); 0xa6's map number.
    recipe_wait: bool,
    pass_wait: bool,
    /// Op 0x71: the input window is open (DAT_10489930), the server's 0x052 mode 3 is awaited (DAT_1047fab0), the
    /// last text entered (DAT_104886e0).
    input_open: bool,
    input_wait: bool,
    input_buf: [u8; 16],
    submap_wait: bool,
    submap: u32,
    /// Race and server id per VM actor (values 0x7f07 / 0x7f0a), from the host each tick.
    actor_info: std::collections::HashMap<u32, (u8, u32)>,
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
            actor_slots: [0xffff; 9],
            window: 0,
            choice: ChoiceState::default(),
            wait_update: false,
            wait_pos: false,
            ending: false,
            end_phase: 0,
            cancelled: false,
            done: false,
            camera: false,
            cam_seed: None,
            last_speaker: None,
            names: None,
            player_word: host.player_word(),
            actor_info: std::collections::HashMap::new(),
            reg_armed: false,
            reg_value: 0,
            recipe_wait: false,
            pass_wait: false,
            input_open: false,
            input_wait: false,
            input_buf: [0; 16],
            submap_wait: false,
            submap: 0,
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
            rt.bodies.push(Body { locals: [0; 0x50], pos, dir, tilt: [0.0; 2], speed: 5.0, anim_speed: 5.0, no_ground: false, msg_actor: None });
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

    /// The input window closed (callback LAB_100b83d0): every way out returns the text as typed.
    pub fn input_done(&mut self, text: &[u8]) {
        self.input_buf = [0; 16];
        let n = text.len().min(16);
        self.input_buf[..n].copy_from_slice(&text[..n]);
        self.input_open = false;
    }

    /// s2c 0x0BF (FUN_100a2160 -> FUN_100bab20): the registration answer op 0xa7 waits for.
    pub fn registration(&mut self, value: u32) {
        if self.reg_armed {
            (self.reg_value, self.reg_armed) = (value, false);
        }
    }

    /// s2c 0x031 (FUN_1009f7c0), 0x059 (FUN_1009fa00), 0x10E (100a2100): the answers ops 0x8c / 0x87 0x88 / 0xa6 wait
    /// for. `p` is the whole packet, header included.
    pub fn answer(&mut self, id: u16, p: &[u8]) {
        let b = |o: usize| p.get(o).copied().unwrap_or(0);
        let h = |o: usize| u16::from_le_bytes([b(o), b(o + 1)]) as u32;
        let w = |o: usize| u32::from_le_bytes([b(o), b(o + 1), b(o + 2), b(o + 3)]);
        match id {
            // Type @0x30: 1 / 3 / 4 a recipe (product, skills, crystal, key item into Z40..Z44, Z55; ingredients and
            // counts into Z45.. / Z4D.., type 4 one slot per unit), 2 a list (Z2..Z9 and 0x1700..0x1707, next 0x1708).
            0x031 => {
                match h(0x30) {
                    t @ (1 | 3 | 4) => {
                        for i in 0..5 {
                            self.z[0x40 + i] = h(4 + 2 * i);
                        }
                        self.z[0x55] = h(0xe);
                        if t == 4 {
                            let mut n = 0;
                            for i in 0..8 {
                                let (item, count) = (h(0x10 + 2 * i), h(0x20 + 2 * i));
                                if item == 0 || count == 0 {
                                    break;
                                }
                                for _ in 0..count.min(8 - n as u32) {
                                    (self.z[0x45 + n], self.z[0x4d + n]) = (item, 1);
                                    n += 1;
                                }
                                if n == 8 {
                                    break;
                                }
                            }
                            for k in n..8 {
                                (self.z[0x45 + k], self.z[0x4d + k]) = (0, 0);
                            }
                        } else {
                            for i in 0..8 {
                                (self.z[0x45 + i], self.z[0x4d + i]) = (h(0x10 + 2 * i), h(0x20 + 2 * i));
                            }
                        }
                    }
                    2 => {
                        for i in 0..8 {
                            (self.z[2 + i], self.mem.g[i]) = (h(0x10 + 2 * i), h(0x20 + 2 * i));
                        }
                        self.mem.g[8] = h(0x32);
                    }
                    _ => {}
                }
                self.recipe_wait = false;
            }
            // Type into Z2, three counts into Z3..Z5, Z6 = byte @0x21, the 16-byte pass code into string slot 0.
            0x059 => {
                self.z[2] = b(0x20) as i8 as u32;
                for i in 0..3 {
                    self.z[3 + i] = w(4 + 4 * i);
                }
                self.z[6] = b(0x21) as i8 as u32;
                let code = p.get(0x10..0x20).unwrap_or(&[]);
                let code = String::from_utf8_lossy(&code[..code.iter().position(|&c| c == 0).unwrap_or(code.len())]).into_owned();
                if self.start.strings.is_empty() {
                    self.start.strings.push(String::new());
                }
                self.start.strings[0] = code;
                self.pass_wait = false;
            }
            0x10E if self.submap_wait => (self.submap, self.submap_wait) = (w(4), false),
            _ => {}
        }
    }

    /// s2c 0x052 (100a06f0): mode 1 ends an update round trip; mode 2 for this event cancels it.
    pub fn release(&mut self, mode: u8, event: u16) {
        match mode {
            1 => self.wait_update = false,
            // 0x100a2050: CancelInput, the answer to op 0x71's c2s 0x060.
            3 => self.input_wait = false,
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
        for a in self.vms.iter().map(|v| v.actor).chain([PLAYER]).collect::<Vec<_>>() {
            self.actor_info.insert(a, (host.race(a), host.server_id(a)));
        }
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
            // FUN_100af4c0: the race byte (+0xef) and the server id (+0x78).
            0x7f07 => self.actor_info.get(&self.vms[v].actor).map_or(0, |i| i.0 as u32),
            0x7f0a => self.actor_info.get(&self.vms[v].actor).map_or(self.vms[v].actor, |i| i.1),
            0x7f87 => self.actor_info.get(&PLAYER).map_or(0, |i| i.0 as u32),
            0x7f80..0x7f84 => {
                let p = self.player_body().map(|b| (b.pos, b.dir)).unwrap_or(([0.0; 3], 0.0));
                match k {
                    0x7f80..0x7f83 => (p.0[(k - 0x7f80) as usize] / POS) as i32 as u32,
                    _ => (p.1.rem_euclid(std::f32::consts::TAU) / DIR) as i32 as u32,
                }
            }
            0x7f86 => self.player_word & 0xff,
            0x7f88 => self.player_word >> 8 & 0xff,
            0x7f8a => self.actor_info.get(&PLAYER).map_or(PLAYER, |i| i.1),
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
            g: self.mem.g.map(|v| v as i32),
            strings: self.start.strings.clone(),
            female: host.female(),
            names: self.names.clone(),
            ..host.text_ctx()
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
        let (wait, place, mode, sounds) = e.map_or((None, [None; 2], None, Vec::new()), |e| (e.wait, e.place, e.mode, e.sounds));
        let name = speaker.map(|s| host.name(s));
        let text = match name {
            Some(n) => format!("{n} : {text}"),
            None => text,
        };
        let body = self.vms[v].body;
        self.bodies[body].msg_actor = speaker;
        host.effect(Effect::Message { op, speaker, id, text, prompt, wait, place, mode, sounds });
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
                // 0x16 is -(m sin a): FCHS in both builds (XiEvents agrees); 0x17 is m cos a.
                let r = if opc == 0x16 { -a.sin() } else { a.cos() } * m;
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
            // FUN_100b1fa0: the "Name : " prefix is actor@2's; the talking mouth (body +0x164) is actor@6's.
            0xb0 if sub == Some(0) => {
                let who = self.actor_at(v, 2);
                let id = self.val(v, 10) as u32;
                if host.ready(who) {
                    self.message(v, opc, Some(who), Some(who), id, host);
                    let body = self.vms[v].body;
                    self.bodies[body].msg_actor = Some(self.actor_at(v, 6));
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
            // FUN_100b5630: sub 0 stops the turning steps and keeps target x val@2, z val@4, height val@6 and a frame
            // limit val@8; sub 1 walks there in 3D at the body speed until it arrives or the limit runs out. Both
            // then put the body on the ground unless it is off-ground.
            0x31 => {
                if sub == Some(0) {
                    host.effect(Effect::StopTurn { actor });
                    let (x, z, y) = (self.val(v, 2) as f32 * POS, self.val(v, 4) as f32 * POS, self.val(v, 6) as f32 * POS);
                    let frames = self.val(v, 8) as i16;
                    let t = self.thread(v);
                    t.target = [x, y, z, 0.0];
                    t.frames = frames;
                    self.adv(v, 10);
                } else if sub == Some(1) {
                    self.move_3d(v);
                } else {
                    return;
                }
                let (body, actor) = (self.vms[v].body, self.vms[v].actor);
                let b = &mut self.bodies[body];
                if !b.no_ground
                    && let Some(g) = host.ground(actor, b.pos)
                {
                    b.pos[1] = g;
                }
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
            0x24 => self.choice_op(v, host, 0, 7),
            0x25 => self.wait_choice(v, true),
            // FUN_100b2280 subs 0 / 2: the 0x24 menu one byte on. Sub 2 is the choice whose cursor option shows its item
            // (LAB_1013e6a0): the host is told each frame. Sub 0 waits for the map, and once the menu is up opens the
            // map preview (FUN_100b2570 mode 6): the host is told then.
            0xd4 if matches!(sub, Some(0) | Some(2)) => {
                if sub == Some(2) {
                    host.effect(Effect::Op { op: opc, sub, actor, args: Vec::new() });
                }
                if sub == Some(0) && !host.map_ready() {
                    self.yield_(v);
                } else {
                    let was = self.choice.open;
                    self.choice_op(v, host, 1, 8);
                    if sub == Some(0) && !was && self.choice.open {
                        host.effect(Effect::Op { op: opc, sub, actor, args: Vec::new() });
                    }
                }
            }
            // FUN_100b2280 1 / 3 / 4 / 5: option val@2's 32-byte entry (FUN_1014e070): val@4 (an item for 3..5, a point
            // kind for 1), sub 1 also val@6 (the point id), subs 4 / 5 the extdata words val@6 / @8 / @10.
            0xd4 if matches!(sub, Some(1 | 3 | 4 | 5)) => {
                let offs: &[usize] = match sub {
                    Some(1) => &[2, 4, 6],
                    Some(3) => &[2, 4],
                    _ => &[2, 4, 6, 8, 10],
                };
                let a = offs.iter().map(|&o| Arg::Int(self.val(v, o))).collect();
                host.effect(Effect::Op { op: opc, sub, actor, args: a });
                self.adv(v, len);
            }
            // FUN_100b2aa0: as 0x25, but a cancel stores 0xff and the event goes on.
            0x7f => self.wait_choice(v, false),
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
            // FUN_100b50f0: wait while the zone action 4cc@9 on (u32@1, u32@5) runs.
            0x54 => {
                let (a, b, name) = (self.actor_at(v, 1), self.actor_at(v, 5), four_cc(self.u32_at(v, 9)));
                if host.busy(&Busy::ZoneAction { name, a, b }) {
                    self.yield_(v);
                    return;
                }
                self.adv(v, len);
            }
            // FUN_100b6a60: sub 4 actor@2's walk speed (+0x168 = val@6 x 0.1), 5 its no-ground bit (u8@6), 6 waits
            // while its motion plays; 0 / 2 the event actor's turn / look percent (+0xdc / +0xe0 = val@2), 1 / 3
            // actor@2's (val@6); 7 and 8 go to the host. Always advances.
            0x59 => {
                let t = self.actor_at(v, 2);
                match sub {
                    Some(4) | Some(5) => {
                        if let Some(tv) = self.vm_of(t) {
                            let b = self.vms[tv].body;
                            if sub == Some(4) {
                                self.bodies[b].speed = self.val(v, 6) as f32 * 0.1;
                            } else {
                                self.bodies[b].no_ground = self.u8_at(v, 6) != 0;
                            }
                        }
                    }
                    Some(6) => {
                        if host.busy(&Busy::Op { op: 0x59, sub, actor: t }) {
                            self.yield_(v);
                            return;
                        }
                    }
                    Some(s @ 0..=3) => {
                        let (who, x) = if s & 1 == 0 { (actor, self.val(v, 2)) } else { (t, self.val(v, 6)) };
                        host.effect(Effect::Op { op: opc, sub, actor: who, args: vec![Arg::Int(x)] });
                    }
                    _ => {
                        let a = self.args(v, op, sub);
                        host.effect(Effect::Op { op: opc, sub, actor, args: a });
                    }
                }
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
                    Some(0) | Some(1) => {
                        self.camera = sub == Some(0);
                        host.effect(Effect::Camera { flag: self.camera, offset: None });
                    }
                    // FUN_100b6440 sub 3: as sub 0 from (val@2, val@4, val@6) x 0.001, read the first time a run
                    // gets here (4 sites: "check your character's appearance", (4, -1.2, 0): in front).
                    Some(3) => {
                        self.camera = true;
                        let read = [2, 4, 6].map(|k| self.val(v, k) as f32 * 0.001);
                        let o = *self.cam_seed.get_or_insert(read);
                        host.effect(Effect::Camera { flag: true, offset: Some(o) });
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
            // FUN_100b8b80 sub 2: stays on the op until actor@2's chocobo / mount has its model; then the frame ends.
            // FUN_100bac90 sub 4: stays on the op while the event actor's event status is 0 and its riding transition
            // (+0x1bc) runs, then its status lock (+0x120 bit 2) clears.
            0xab if sub == Some(4) => {
                if host.busy(&Busy::Op { op: 0xab, sub, actor }) {
                    self.yield_(v);
                } else {
                    host.effect(Effect::Op { op: opc, sub, actor, args: Vec::new() });
                    self.adv(v, 2);
                }
            }
            0x7e if sub == Some(2) => {
                let a = self.actor_at(v, 2);
                if host.ready(a) && host.busy(&Busy::Op { op: 0x7e, sub, actor: a }) {
                    self.yield_(v);
                } else {
                    self.adv(v, 6);
                    self.yield_(v);
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
            // FUN_100b3e20: the caller's real server id.
            0xbe => {
                let c = self.thread(v).caller;
                let c = self.actor_info.get(&c).map_or(c, |i| i.1);
                self.set_at(v, 1, 0, c);
                self.adv(v, 3);
            }
            // FUN_100b67d0 sub 0: waits out a running load, then loads sub-area val@2 (FUN_10178900, synchronous in
            // retail; ours waits for it).
            0x75 if sub == Some(0) => {
                let busy = Busy::Op { op: 0x75, sub, actor };
                if self.thread(v).state == 0 {
                    if host.busy(&busy) {
                        self.yield_(v);
                        return;
                    }
                    let args = self.args(v, op, sub);
                    host.effect(Effect::Op { op: 0x75, sub, actor, args });
                    self.thread(v).state = 1;
                }
                if host.busy(&busy) {
                    self.yield_(v);
                } else {
                    self.thread(v).state = 0;
                    self.adv(v, len);
                }
            }
            // FUN_100b9040: on when the event actor's model stands in region 4cc u32@1, else (or with no such region)
            // jump to u16@5.
            0x82 => {
                let p = self.bodies[body].pos;
                if host.in_region(self.u32_at(v, 1), p) == Some(true) {
                    self.adv(v, 7);
                } else {
                    self.jump(v, 5);
                }
            }
            // FUN_100ba100: the language as 1 Japanese / 2 English (the European ones count as English).
            0x9c => {
                let n = match host.language() {
                    1..=3 => 2,
                    _ => 1,
                };
                self.set_at(v, 1, 0, n);
                self.adv(v, 3);
            }
            // FUN_100b75c0: set@1 = sqrt((v7 - v3)^2 + (v9 - v5)^2), integer squares, truncated.
            0x64 => {
                let (dx, dz) = (self.val(v, 7).wrapping_sub(self.val(v, 3)), self.val(v, 9).wrapping_sub(self.val(v, 5)));
                let d = ((dz.wrapping_mul(dz) as f64) + (dx.wrapping_mul(dx) as f64)).sqrt();
                self.set_at(v, 1, 0, d as i32 as u32);
                self.adv(v, 0xb);
            }
            // FUN_100b7860: set@1 = the 3D distance between actor@3 and actor@7 x 1000 (event bodies where bound), 0 when
            // either is missing.
            0x65 => {
                let at = |rt: &Self, host: &dyn Host, a: u32| match rt.vm_of(a) {
                    Some(i) => host.index(a).map(|_| rt.bodies[i].pos),
                    None => host.index(a).map(|_| host.place(a).0),
                };
                let (a, b) = (self.actor_at(v, 3), self.actor_at(v, 7));
                let d = match (at(self, host, a), at(self, host, b)) {
                    (Some(p), Some(q)) => ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt() as f64 * 1000.0,
                    _ => 0.0,
                };
                self.set_at(v, 1, 0, d as i32 as u32);
                self.adv(v, 0xb);
            }
            // FUN_100bc060: actor@2's event body rotation, val x 2 pi / 4096: sub 1 x, 2 y (the direction), 3 z, 4 all
            // three (val@6 / @8 / @10); sub 0 copies the actor's. No body: advances 6 whatever the sub.
            0xd8 => {
                let t = self.actor_at(v, 2);
                let rad = |x: i32| x as f32 * std::f32::consts::TAU / 4096.0;
                match self.vm_of(t).map(|tv| self.vms[tv].body) {
                    Some(b) => {
                        match sub {
                            Some(0) => (self.bodies[b].tilt, self.bodies[b].dir) = ([0.0, 0.0], host.place(t).1),
                            Some(1) => self.bodies[b].tilt[0] = rad(self.val(v, 6)),
                            Some(2) => self.bodies[b].dir = rad(self.val(v, 6)),
                            Some(3) => self.bodies[b].tilt[1] = rad(self.val(v, 6)),
                            Some(4) => {
                                let (x, y, z) = (rad(self.val(v, 6)), rad(self.val(v, 8)), rad(self.val(v, 10)));
                                (self.bodies[b].tilt, self.bodies[b].dir) = ([x, z], y);
                            }
                            _ => {}
                        }
                        self.adv(v, len);
                    }
                    None => self.adv(v, 6),
                }
            }
            // FUN_100babe0: Vana'diel time val@1 (earth seconds; hour 0x90, day 0xd80) as year + 886, month + 1,
            // day + 1, weekday, hour, minute and moon step.
            0xaa => {
                let t = self.val(v, 1) as u32;
                let parts = [t / 0x12fc00 + 0x376, t % 0x12fc00 / 0x19500 + 1, t % 0x19500 / 0xd80 + 1, t % 0x6c00 / 0xd80, t % 0xd80 / 0x90, t % 0x90 * 60 / 0x90, t / 0x5e80 % 12];
                for (k, p) in parts.into_iter().enumerate() {
                    self.set_at(v, 3 + 2 * k, 0, p);
                }
                self.adv(v, 0x11);
            }
            // FUN_100b84b0: sub 0 loads the weather table (file 0x1b79, 0x1b7d from zone 100; a failed load skips
            // the sub 1 after it, len 10); sub 1 copies zone val@2's three bytes for day val@4 % 0x870 into zone work
            // 0x1002..0x1004. Both end the frame.
            0x72 if sub == Some(0) => {
                let ok = host.weather_day(self.val(v, 2) as u32, 0).is_some();
                self.adv(v, if ok { 4 } else { 10 });
                self.yield_(v);
            }
            0x72 if sub == Some(1) => {
                let (zone, day) = (self.val(v, 2) as u32, (self.val(v, 4).rem_euclid(0x870)) as u32);
                if let Some(b) = host.weather_day(zone, day) {
                    for k in 0..3 {
                        self.z[2 + k] = b[k] as u32;
                    }
                }
                self.adv(v, 6);
                self.yield_(v);
            }
            // FUN_100afee0: sub 0 stores val@8 into actor@2's VM, at the operand @6 of *its* current op; 1 actor@4's
            // name into str@2; 2 its server id (0 when missing); 3 its ActIndex (0xffff when missing); 4 its ActIndex
            // into slot val@2 of a nine-entry table (DAT_10480a94; nothing reads it).
            0xb7 => {
                match sub {
                    Some(0) => {
                        let a = self.actor_at(v, 2);
                        let x = self.val(v, 8) as u32;
                        if let Some(t) = self.vm_of(a).filter(|_| host.index(a).is_some()) {
                            self.set_at(t, 6, 0, x);
                        }
                    }
                    Some(1) => {
                        let a = self.actor_at(v, 4);
                        let name = if host.index(a).is_some() { host.name(a) } else { String::new() };
                        let mut b = [0u8; 16];
                        for (i, c) in name.bytes().take(15).enumerate() {
                            b[i] = c;
                        }
                        let w = std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()));
                        let raw = self.u16_at(v, 2);
                        self.set4(v, raw, 0, w);
                    }
                    Some(2) => {
                        let a = self.actor_at(v, 4);
                        let id = if host.index(a).is_some() { a } else { 0 };
                        self.set_at(v, 2, 0, id);
                    }
                    Some(3) => {
                        let a = self.actor_at(v, 4);
                        self.set_at(v, 2, 0, host.index(a).unwrap_or(0xffff) as u32);
                    }
                    Some(4) => {
                        let a = self.actor_at(v, 4);
                        let k = self.val(v, 2);
                        if (0..9).contains(&k) {
                            self.actor_slots[k as usize] = host.index(a).unwrap_or(0xffff);
                        }
                    }
                    _ => {}
                }
                self.adv(v, len);
            }
            // FUN_100baa60: sub 0 arms the wait (+0x180 bit 0) and yields, then sends the update (c2s 0x05B mode 1,
            // FUN_100a0510) and zeroes the answer; sub 1 waits until s2c 0x0BF clears it, then set@2 = the answer. Any
            // other sub stalls. Every call ends the frame.
            0xa7 => {
                match sub {
                    Some(0) if !self.reg_armed => self.reg_armed = true,
                    Some(0) => {
                        host.effect(Effect::Send(Packet::EventUpdate { head: self.head(), option: self.z[1] }));
                        self.reg_value = 0;
                        self.adv(v, 2);
                    }
                    Some(1) if !self.reg_armed => {
                        let x = self.reg_value;
                        self.set_at(v, 2, 0, x);
                        self.adv(v, 4);
                    }
                    _ => {}
                }
                self.yield_(v);
            }
            // FUN_100b0a00: string slots and literals (feeding the 0x9d compares and message string codes), timers and
            // windows. Every sub ends the frame (+0x25a). 0 / 0x13: str@2 = the 16 literal bytes at pc + 4 (byte 15
            // zeroed; 0x13 turns '@' into spaces); 1: str@2 = event string slot val@4 (0..3, else 0); 4: that slot =
            // str@2; 9 / 0xc: the cast bar's / event timer's progress (windows not modelled: 0); 0x15: waits while the
            // map window is open. The other subs (window and timer opens / closes, the map view) go to the host.
            0xb4 => {
                let s = sub.unwrap_or(0xff);
                match s {
                    0 | 0x13 => {
                        let pc = self.vms[v].pc as usize + 4;
                        let mut b = [0u8; 16];
                        b.copy_from_slice(self.code(v).get(pc..pc + 16).unwrap_or(&[0; 16]));
                        b[15] = 0;
                        if s == 0x13 {
                            b[..15].iter_mut().filter(|c| **c == b'@').for_each(|c| *c = b' ');
                        }
                        let raw = self.u16_at(v, 2);
                        self.set4(v, raw, 0, std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())));
                    }
                    1 => {
                        let slot = (self.val(v, 4) as usize).min(4) % 4;
                        let mut b = [0u8; 16];
                        if let Some(t) = self.start.strings.get(slot) {
                            let n = t.len().min(15);
                            b[..n].copy_from_slice(&t.as_bytes()[..n]);
                        }
                        let raw = self.u16_at(v, 2);
                        self.set4(v, raw, 0, std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())));
                    }
                    4 => {
                        let slot = (self.val(v, 4) as usize).min(4) % 4;
                        let b = self.str_at(v, 2);
                        let text = String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(15)]).into_owned();
                        if self.start.strings.len() <= slot {
                            self.start.strings.resize(slot + 1, String::new());
                        }
                        self.start.strings[slot] = text;
                    }
                    9 | 0xc => self.set_at(v, 2, 0, 0),
                    // The map view: x, y, zoom factor, speed, flags (each a value) for the host.
                    0x14 => {
                        let a = [2, 4, 6, 8, 10].map(|o| Arg::Int(self.val(v, o)));
                        host.effect(Effect::Op { op: opc, sub, actor, args: a.to_vec() });
                    }
                    0x15 if host.busy(&Busy::Op { op: opc, sub, actor }) => {
                        self.yield_(v);
                        return;
                    }
                    0x15 => {}
                    _ => {
                        let a = self.args(v, op, sub);
                        host.effect(Effect::Op { op: opc, sub, actor, args: a });
                    }
                }
                self.adv(v, len);
                self.yield_(v);
            }
            0xda => {
                let v32 = host.game_config(0x43);
                self.set_at(v, 1, 0, v32);
                self.adv(v, len);
            }
            // FUN_100b7e10: the input window. 0 / 0x10 / 0x12 / 0x30 / 0x32 open it (kind 0 / val@2 + 1 / val@2 + 3,
            // max 16 / 8 / val@4), 1 sends the last text (c2s 0x060) once it is closed, 2 waits for 0x052 mode 3, 3 the
            // vulgar-name check (no dictionary here: 0), 0x11 / 0x13 atoi into set@2, 0x31 the 16 bytes into str@2.
            // The rest go to the host; unknown subs stay. Every call ends the frame.
            0x71 => {
                let s = sub.unwrap_or(0xff);
                let open = match s {
                    0 => Some((0, 16, 2)),
                    0x10 => Some((self.val(v, 2) + 1, 8, 4)),
                    0x12 => Some((self.val(v, 2) + 1, self.val(v, 4), 6)),
                    0x30 => Some((self.val(v, 2) + 3, 16, 4)),
                    0x32 => Some((self.val(v, 2) + 3, self.val(v, 4), 6)),
                    _ => None,
                };
                if let Some((kind, max, n)) = open {
                    // FUN_10206540 ignores an open while one is up.
                    if !self.input_open {
                        host.effect(Effect::Input { kind, max: max.clamp(0, 16) as usize });
                    }
                    (self.input_open, self.input_wait) = (true, true);
                    self.adv(v, n);
                } else {
                    match s {
                        1 if !self.input_open => {
                            host.effect(Effect::Send(Packet::Passwords { head: self.head(), text: self.input_buf }));
                            self.adv(v, 2);
                        }
                        2 if !self.input_wait => self.adv(v, 2),
                        3 => {
                            self.set_at(v, 2, 0, 0);
                            self.adv(v, 4);
                        }
                        0x11 | 0x13 if !self.input_open => {
                            let x = Self::atoi(&self.input_buf) as u32;
                            self.set_at(v, 2, 0, x);
                            self.adv(v, 4);
                        }
                        0x31 if !self.input_open => {
                            let b = self.input_buf;
                            let raw = self.u16_at(v, 2);
                            self.set4(v, raw, 0, std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())));
                            self.adv(v, 4);
                        }
                        0x20 | 0x21 | 0x40 | 0x41 | 0x50..=0x55 => {
                            let a = self.args(v, op, sub);
                            host.effect(Effect::Op { op: opc, sub, actor, args: a });
                            self.adv(v, len);
                        }
                        _ => {}
                    }
                }
                self.yield_(v);
            }
            // FUN_100b9d90 0x11: wait while arealist picks, then set@2 = the pick (FUN_100af310(2, +0x6e)).
            0xcc if sub == Some(0x11) => match host.area_pick() {
                Some(z) => {
                    self.set_at(v, 2, 0, z as u32);
                    self.adv(v, len);
                }
                None => self.yield_(v),
            },
            // FUN_100b9d90 0..3 (the item info window): item and values; sub 2 adds the string operand's bytes as words.
            0xcc if matches!(sub, Some(0..=3)) => {
                let offs: &[usize] = if sub == Some(2) { &[2, 4, 6, 8, 10] } else { &[2, 4, 6, 8] };
                let mut a: Vec<Arg> = offs.iter().map(|&o| Arg::Int(self.val(v, o))).collect();
                if sub == Some(2) {
                    let b = self.str_at(v, 0xc);
                    a.extend((0..4).map(|k| Arg::Int(i32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap()))));
                }
                host.effect(Effect::Op { op: opc, sub, actor, args: a });
                self.adv(v, len);
            }
            // FUN_100b98e0: c2s 0x058 RECIPE {skill +4, level +6, +8, mode +0xa = sub (1 for sub 0), +0xc..+0x12} from
            // the operands; sub 1 waits for s2c 0x031. Sub 5 arms no wait. Every call ends the frame.
            0x8c => {
                let s = sub.unwrap_or(0xff);
                let w = |o: usize| self.val(v, o) as u16;
                // Words at c2s +4, +6, ... +0x12, and the op's length.
                let mut b = [0u16; 8];
                let n = match s {
                    0 => {
                        b[..3].copy_from_slice(&[w(2), w(4), w(6)]);
                        8
                    }
                    2 => {
                        (b[0], b[1], b[7], b[4], b[5]) = (w(2), w(4), w(6), w(8), w(10));
                        12
                    }
                    3 | 4 => {
                        (b[0], b[1], b[7], b[6]) = (w(2), w(4), w(6), w(8));
                        10
                    }
                    5 => {
                        (b[4], b[0], b[1], b[7], b[6], b[2]) = (w(2), w(4), w(6), w(8), w(10), w(12));
                        14
                    }
                    _ => 0,
                };
                if n > 0 {
                    b[3] = if s == 0 { 1 } else { s as u16 };
                    let body = b.iter().flat_map(|x| x.to_le_bytes()).collect();
                    host.effect(Effect::Send(Packet::Raw { id: 0x058, body }));
                    self.recipe_wait = s != 5;
                    self.adv(v, n);
                } else if s == 1 && !self.recipe_wait {
                    self.adv(v, 2);
                }
                self.yield_(v);
            }
            // FUN_100b9140 / FUN_100b9220: c2s 0x01B FRIENDPASS Para = sub (0 / 2) for 0x87, sub + 1 for 0x88; sub 1
            // waits for s2c 0x059. Every call ends the frame.
            0x87 | 0x88 => {
                match sub {
                    Some(s @ (0 | 2)) => {
                        let para = s as u16 + (opc == 0x88) as u16;
                        let mut body = vec![0u8; 0x18];
                        body[..2].copy_from_slice(&para.to_le_bytes());
                        host.effect(Effect::Send(Packet::Raw { id: 0x01B, body }));
                        self.pass_wait = true;
                        self.adv(v, 2);
                    }
                    Some(1) if !self.pass_wait => self.adv(v, 2),
                    _ => {}
                }
                self.yield_(v);
            }
            // FUN_100ba9c0: sub 0 sends c2s 0x0EB REQSUBMAPNUM, sub 1 waits for s2c 0x10E, sub 2 set@2 = its MapNum.
            0xa6 => {
                match sub {
                    Some(0) => {
                        host.effect(Effect::Send(Packet::Raw { id: 0x0EB, body: Vec::new() }));
                        self.submap_wait = true;
                        self.adv(v, 2);
                    }
                    Some(1) if !self.submap_wait => self.adv(v, 2),
                    Some(2) => {
                        let m = self.submap;
                        self.set_at(v, 2, 0, m);
                        self.adv(v, 4);
                    }
                    _ => {}
                }
                self.yield_(v);
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
                    // Retail ends the frame after these (+0x25a = 1 in every sub).
                    if matches!(opc, 0xb6 | 0xb5 | 0xc1 | 0x7e | 0x71 | 0xa7 | 0xb3) {
                        self.yield_(v);
                    }
                }
            }
        }
    }

    /// C atoi over at most 16 bytes: leading whitespace, a sign, digits; wrapping.
fn atoi(b: &[u8]) -> i32 {
    let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
    let mut i = b.iter().take_while(|c| c.is_ascii_whitespace()).count();
    let neg = b.get(i) == Some(&b'-');
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let n = b[i.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).fold(0i32, |n, c| n.wrapping_mul(10).wrapping_add((c - b'0') as i32));
    if neg { n.wrapping_neg() } else { n }
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

    /// 0x31 sub 1: a step toward the thread's target at speed * ticks / 60 (snapping when within it); done when there
    /// or when the frame limit is below 1. The limit drops by the frame's ticks; always ends the frame.
    fn move_3d(&mut self, v: usize) {
        let body = self.vms[v].body;
        let (tg, frames) = (self.thread(v).target, self.thread(v).frames);
        let dt = self.dt;
        let b = &mut self.bodies[body];
        let d = [tg[0] - b.pos[0], tg[1] - b.pos[1], tg[2] - b.pos[2]];
        b.dir = (-d[2]).atan2(d[0]);
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let step = dt * b.speed / 60.0;
        let done = if frames < 1 {
            true
        } else if dist <= step {
            b.pos = [tg[0], tg[1], tg[2]];
            true
        } else {
            for k in 0..3 {
                b.pos[k] += d[k] / dist * step;
            }
            false
        };
        if done {
            self.adv(v, 2);
        }
        self.thread(v).frames = (frames as f32 - dt) as i16;
        self.yield_(v);
    }

    /// 0x24 (FUN_100b0960): show the question, then build the menu once its text is ready.
    /// 0x24 (FUN_100b0960) and 0xd4 subs 0 / 2 (operands `off` bytes later, `len` bytes long).
    fn choice_op(&mut self, v: usize, host: &mut dyn Host, off: usize, len: u16) {
        if !self.choice.ready {
            if !self.choice.requested {
                let id = self.val(v, 1 + off) as u32;
                let (_, _, e) = self.text(id, None, host);
                let mut options = Vec::new();
                let mut question = String::new();
                let mut cursor = 0;
                if let Some(e) = e {
                    let at = e.choice.unwrap_or(e.lines.len());
                    question = e.lines[..at.min(e.lines.len())].iter().map(|l| decode_sjis(l)).collect::<Vec<_>>().join("\n");
                    let (default, mut mask) = (self.val(v, 3 + off) as u32, self.val(v, 5 + off) as u32);
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
        self.adv(v, len);
    }

    /// 0x25 (FUN_100b0cb0): Z0 = option - 1; cancel = 0xfe and the event aborts (`abort`), or 0xff for 0x7f.
    fn wait_choice(&mut self, v: usize, abort: bool) {
        if !self.choice.open {
            self.adv(v, 1);
            self.yield_(v);
            return;
        }
        if let Some(p) = self.choice.picked {
            self.adv(v, 1);
            self.choice = ChoiceState::default();
            if p == 0xff && !abort {
                self.z[0] = 0xff;
            } else if p == 0xff {
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
    /// The player's race byte (value 0x7f87).
    pub race: u8,
    /// Texts typed into op 0x71's input window, in order (empty once used up).
    pub inputs: std::collections::VecDeque<Vec<u8>>,
    /// The map is not ready (op 0xd4 sub 0 waits).
    pub map_busy: bool,
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
            race: 0,
            inputs: Default::default(),
            map_busy: false,
            running: Vec::new(),
            seed: 1,
            frame: 0,
        }
    }
}

impl Host for Scripted {
    fn race(&self, actor: u32) -> u8 {
        if actor == PLAYER { self.race } else { 0 }
    }

    fn map_ready(&self) -> bool {
        !self.map_busy
    }

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
            rt.registration(0);
        }
        if host.log[before..].iter().any(|e| matches!(e, Effect::Input { .. })) {
            let t = host.inputs.pop_front().unwrap_or_default();
            rt.input_done(&t);
        }
        // LSB answers c2s 0x060 with 0x052 mode 1, then mode 3.
        if host.log[before..].iter().any(|e| matches!(e, Effect::Send(Packet::Passwords { .. }))) {
            rt.release(1, rt.start.event);
            rt.release(3, rt.start.event);
        }
        if host.log[before..].iter().any(|e| matches!(e, Effect::Send(Packet::Raw { .. }))) {
            for id in [0x031, 0x059, 0x10E] {
                rt.answer(id, &[]);
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
pub fn run_cli(zone: u16, event: u16, target: u32, params: [u32; 8], choices: &[Option<u32>], race: u8) -> anyhow::Result<()> {
    let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
    let blocks = event::load(&index, zone)?;
    let dialog = Dialog::load(&index, zone).ok();
    let mut host = Scripted::new(choices);
    host.race = race;
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

    /// Zone 37 event 0x040a: a 0xd4 sub 2 menu (the 0x24 menu one byte on) and the 0x7f wait after it.
    #[test]
    fn select_prompt_d4() {
        let Some((log, f)) = run(37, 0x040a, &[Some(1), Some(1)]) else { return };
        assert!(f.is_some());
        assert!(log.iter().any(|e| matches!(e, Effect::Choice { id: 7284, options, cursor: 2, .. } if options.len() == 3)), "{log:?}");
        assert!(log.iter().any(|e| matches!(e, Effect::Choice { id: 7279, .. })));
    }

    /// Southern San d'Oria Home Point #1 (event 0x21fc, LSB's params with every Home Point registered): the zone
    /// list's op 0xd4 sub 1 entries name the Home Point options, the sub 0 menu then opens the map preview; while the
    /// map is not ready the VM stays on the op.
    #[test]
    fn home_point_preview() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        if event::load(&index, 230).is_err() {
            return;
        }
        let run = |busy: bool| {
            let mut host = Scripted::new(&[Some(0), Some(2), Some(1), Some(0), None]);
            host.map_busy = busy;
            let start = Start { zone: 230, event: 0x21fc, target: 0x010e_6087, ..Default::default() };
            let mut rt = Runtime::start(event::load(&index, 230).unwrap(), Dialog::load(&index, 230).ok(), start, Memory::default(), &mut host);
            rt.params([1, u32::MAX, u32::MAX, u32::MAX, u32::MAX, 100_000, 4095, 0]);
            if let Ok(n) = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En) {
                rt.set_names(std::sync::Arc::new(n));
            }
            run_scripted(&mut rt, &mut host, 60 * 60);
            host.log
        };
        let log = run(false);
        let at = |log: &[Effect], f: &dyn Fn(&Effect) -> bool| log.iter().position(f);
        let entry = |o: i32, id: i32| move |e: &Effect| matches!(e, Effect::Op { op: 0xd4, sub: Some(1), args, .. } if args[..] == [Arg::Int(o), Arg::Int(0), Arg::Int(id)]);
        let (e2, e4) = (at(&log, &entry(2, 1)).unwrap(), at(&log, &entry(4, 97)).unwrap());
        let menu = at(&log, &|e| matches!(e, Effect::Choice { id: 30, .. })).unwrap();
        let open = at(&log, &|e| matches!(e, Effect::Op { op: 0xd4, sub: Some(0), .. })).unwrap();
        assert!(e2 < e4 && e4 < menu && menu < open, "{e2} {e4} {menu} {open}");
        // The title's place name is num[33]: G25 (message slots 8.. are the G registers).
        assert!(matches!(&log[menu], Effect::Choice { question, .. } if question.contains("Southern San d'Oria")), "{:?}", log[menu]);

        let log = run(true);
        assert!(at(&log, &entry(4, 97)).is_some());
        assert!(at(&log, &|e| matches!(e, Effect::Choice { id: 30, .. } | Effect::Op { op: 0xd4, sub: Some(0), .. })).is_none());
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

    /// Op 0x71 over hand-made code: an "Enter number:" input read with atoi, then a name stored as words.
    #[test]
    fn input_window() {
        assert_eq!((Runtime::atoi(b" -42x"), Runtime::atoi(b"+7"), Runtime::atoi(b"")), (-42, 7, 0));
        assert_eq!(Runtime::atoi(b"1234567890123456"), 1234567890123456u64 as i32);
    }

    /// Lower Jeuno 0xc7 (compatibility test): the Password: input (kind 0), its text sent with c2s 0x060, then the wait
    /// for 0x052 mode 3.
    #[test]
    fn input_password_lower_jeuno() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(blocks) = event::load(&index, 245) else { return };
        let mut host = Scripted::new(&[Some(0)]);
        host.inputs.push_back(b"Tuxedo".to_vec());
        let start = Start { zone: 245, event: 0xc7, target: 0x010f_5023, ..Default::default() };
        let mut rt = Runtime::start(blocks, Dialog::load(&index, 245).ok(), start, Memory::default(), &mut host);
        run_scripted(&mut rt, &mut host, 60 * 60).expect("finished");
        assert!(host.log.iter().any(|e| matches!(e, Effect::Input { kind: 0, max: 16 })));
        let sent = host.log.iter().find_map(|e| if let Effect::Send(Packet::Passwords { text, .. }) = e { Some(*text) } else { None });
        assert_eq!(&sent.expect("0x060")[..7], b"Tuxedo\0");
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
