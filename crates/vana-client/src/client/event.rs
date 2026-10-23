//! Event playback in the live client: event_vm's Runtime run on the world (FUN_100ad2c0 start, FUN_100ad000
//! actor wait, per-frame tick in FUN_10094ea0). Messages go to the event message window with the kaipage
//! prompt (and the log), choices to "menu    query"; scheduler cameras and fades, script-driven actors
//! drawn from their VM bodies, and the 0x05B / 0x05C / 0x016 sends.

use super::net::{EventStart, NetCommand, ZoneText};
use super::World;
use crate::dat::DatIndex;
use crate::dat::dialog::{self, Dialog};
use crate::dat::event::Block;
use crate::dat::font::decode_sjis;
use crate::dat::title::{Key, Title, focal_of, fov_y, sample_from};
use crate::event_vm::{Arg, Busy, Effect, Host, Packet, PLAYER, Runtime, Start};
use crate::render::Camera;
use glam::Vec3;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

const TAU: f32 = std::f32::consts::TAU;
/// Talk range (FUN_1007aac0, _DAT_10321d80).
pub const TALK_RANGE: f32 = 6.0;
/// FUN_100ad2a0: unanswered 0x016 requests go again after 120 ticks.
const ASK_AGAIN: f32 = 120.0;
/// FUN_1008b680: an actor is turning while its drawn rotation is more than 16/4096 of a turn off.
const TURN_GAP: f32 = TAU * 16.0 / 4096.0;

pub fn rad_of(dir: u8) -> f32 {
    dir as f32 / 256.0 * TAU
}

pub fn dir_of(rad: f32) -> u8 {
    ((rad / TAU * 256.0).round().rem_euclid(256.0)) as u8
}

/// Signed angle from `a` to `b` in (-pi, pi].
fn angle_to(a: f32, b: f32) -> f32 {
    (b - a + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

/// FUN_100ad2c0: the zone whose event and dialogue files the event reads.
pub fn file_zone(s: &EventStart, current: u16) -> u16 {
    if (1000..=0x513).contains(&current) {
        current + 1000
    } else if s.zone == s.zone2 || s.zone2 == 0 {
        s.zone
    } else {
        s.zone2 + 1000
    }
}

/// A scheduler file (0x77f0 + n, ...): raw bytes for its ops and the camera paths.
struct SchedFile {
    bytes: Vec<u8>,
    title: Option<Title>,
}

/// One scheduler op: u8 op, u8 words, u16 wait @4, u16 duration @6, then its data.
struct SOp {
    op: u8,
    wait: u16,
    ticks: u16,
    data: Vec<u8>,
}

fn sched_ops(file: &[u8], tag: &str) -> Option<Vec<SOp>> {
    let c = crate::dat::chunks(file).ok()?.into_iter().find(|c| c.kind == 0x07 && c.name() == tag)?;
    let (pay, mut p, mut out) = (c.data, 0x40, Vec::new());
    while p + 2 <= pay.len() {
        let (op, len) = (pay[p], pay[p + 1] as usize);
        if len == 0 || p + 4 * len > pay.len() {
            break;
        }
        let b = &pay[p..p + 4 * len];
        let (wait, ticks) = if len >= 2 { (u16::from_le_bytes([b[4], b[5]]), u16::from_le_bytes([b[6], b[7]])) } else { (0, 0) };
        out.push(SOp { op, wait, ticks, data: b.to_vec() });
        p += 4 * len;
        if op == 0 {
            break;
        }
    }
    Some(out)
}

/// A running CMoSchedularTask (FUN_10056e60): ops run when the previous op's wait is over.
struct Sched {
    file: u32,
    tag: String,
    actor: u32,
    target: u32,
    ops: Vec<SOp>,
    cursor: usize,
    next_at: f32,
    start: f32,
    /// Ticks until it is done (None = loops until stopped).
    len: Option<f32>,
    speed: f32,
}

struct Shot {
    file: Arc<SchedFile>,
    /// The scheduler that started it.
    tag: String,
    path: usize,
    start: f32,
    ticks: f32,
    /// The camera when it started (flag bit 19) and the player's follow camera (bit 20), FUN_1003be40.
    from: Option<Key>,
    follow: Option<Key>,
}

/// Op 0x10 (0x10057d12, FUN_1005f020): the frame before the cut fades out over `len` ticks.
#[derive(Clone, Copy)]
pub struct Cross {
    pub start: f32,
    pub len: f32,
    /// The camera of the captured frame; `captured` once the renderer has drawn it offscreen.
    pub camera: Option<Camera>,
    pub captured: bool,
}

/// Scheduler tasks, the cutscene camera and the screen fades. They are not part of the event (CMoSchedularTask,
/// nothing in FUN_100acf20 / FUN_100bb750 kills them), so the fade-in 0x1f7 starts with its 0x21 finishes.
pub struct Screen {
    scheds: Vec<Sched>,
    files: HashMap<u32, Option<Arc<SchedFile>>>,
    shot: Option<Shot>,
    tick: f32,
    /// Op 0x0f: the scene colour multiply per channel, 2 * RGB / 255 (MODULATE2X, 0x80 = 1.0; A unused):
    /// from, to, start tick, length.
    fade: ([f32; 3], [f32; 3], f32, f32),
    pub cross: Option<Cross>,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { scheds: Vec::new(), files: HashMap::new(), shot: None, tick: 0.0, fade: ([1.0; 3], [1.0; 3], 0.0, 0.0), cross: None }
    }
}

impl Screen {
    fn file(&mut self, index: &DatIndex, id: u32) -> Option<Arc<SchedFile>> {
        self.files
            .entry(id)
            .or_insert_with(|| {
                let bytes = index.read(id).map_err(|e| log::warn!("event scheduler file {id}: {e:#}")).ok()?;
                let title = Title::parse(&bytes).ok();
                Some(Arc::new(SchedFile { bytes, title }))
            })
            .clone()
    }

    /// The scene multiply now (1.0 = unchanged).
    pub fn multiply(&self) -> [f32; 3] {
        let (from, to, start, len) = self.fade;
        let t = if len <= 0.0 { 1.0 } else { ((self.tick - start) / len).clamp(0.0, 1.0) };
        std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
    }

    /// Alpha of the crossfade image (FUN_1005f020: remaining / total), None when there is none.
    pub fn cross_alpha(&self) -> Option<f32> {
        let c = self.cross?;
        let a = 1.0 - (self.tick - c.start) / c.len.max(1.0);
        (a > 0.0).then_some(a.min(1.0))
    }

    /// Runs the scheduler ops due by now: 0x04 camera path, 0x0f fade, 0x10 crossfade. `current` is the camera
    /// drawn last frame, `follow` the player's follow camera.
    fn advance(&mut self, current: Option<Camera>, follow: Option<Key>) {
        let now = self.tick;
        let mut shot = None;
        let mut fade = None;
        let mut cross = None;
        for s in &mut self.scheds {
            while s.cursor < s.ops.len() && now >= s.next_at {
                let op = &s.ops[s.cursor];
                s.cursor += 1;
                let at = s.next_at;
                match op.op {
                    0x04 if op.data.len() >= 12 => {
                        let name = String::from_utf8_lossy(&op.data[8..12]).trim_end_matches('\0').to_string();
                        shot = Some((s.file, s.tag.clone(), name, at, op.ticks.max(1) as f32 * s.speed));
                    }
                    // 0x10057ee6 / FUN_1005f630: D3DCOLOR R G B A @8; lasts dur * speed - 1 ticks.
                    0x0f if op.data.len() >= 12 => {
                        let c = &op.data[8..12];
                        let m = [c[0], c[1], c[2]].map(|v| v as f32 * 2.0 / 255.0);
                        fade = Some((m, at, (op.ticks as f32 * s.speed - 1.0).max(0.0)));
                    }
                    0x10 => cross = Some(Cross { start: at, len: (op.ticks as f32 * s.speed).max(1.0), camera: current, captured: false }),
                    _ => {}
                }
                s.next_at = at + op.wait as f32 * s.speed;
            }
        }
        if let Some((file, tag, name, at, ticks)) = shot
            && let Some(Some(f)) = self.files.get(&file).cloned()
            && let Some(path) = f.title.as_ref().and_then(|t| t.paths.iter().position(|p| p.name == name))
        {
            if let Some(p) = f.title.as_ref().and_then(|t| t.paths.get(path)) {
                log::info!("event camera {tag}: path {} flags {:#x}, {} keys, {ticks} ticks{}", p.name, p.flags, p.keys.len(), if p.flags & crate::dat::title::FROM_CURRENT != 0 { ", from the current camera" } else { "" });
            }
            self.shot = Some(Shot { file: f, tag, path, start: at, ticks, from: current.map(key_of), follow });
        }
        if let Some((to, at, ticks)) = fade {
            self.fade = (self.multiply(), to, at, ticks);
        }
        if cross.is_some() {
            self.cross = cross;
        }
        if self.cross_alpha().is_none() && self.cross.is_some_and(|c| self.tick > c.start) {
            self.cross = None;
        }
        let tick = self.tick;
        self.scheds.retain(|s| s.len.is_none_or(|l| tick - s.start <= l + 1.0));
    }
}

fn key_of(c: Camera) -> Key {
    Key { eye: c.eye, focal: focal_of(c.fov_y), target: c.target, roll: 0.0 }
}

/// FUN_10020450 (bit-20 end point): 3 units behind the player's attach point 2, looking at its body
/// (position lifted by 0.6 x its height, 1.2 without one), focal 350 *(inf: the matrix and height inputs)*.
fn follow_key(w: &World) -> Option<Key> {
    let dir = w.net.me.lock().map(|m| m.dir).ok()?;
    let f = super::forward_from_dir(dir);
    let h = w.actors.get(&w.my_id).map_or(2.0, |a| -a.top);
    let lift = if h > 0.1 { h * 0.6 } else { 1.2 };
    Some(Key { eye: w.pos + Vec3::new(0.0, -h, 0.0) - f * 3.0, focal: 350.0, target: w.pos - Vec3::new(0.0, lift, 0.0), roll: 0.0 })
}

/// The open message (event message window, FUN_10131e30): its text, paged 4 lines at a time once wrapped.
pub struct Msg {
    pub id: u32,
    /// Ends in 7f 31: the last page waits for a key.
    pub prompt: bool,
    wait: f32,
    open: f32,
    pub lines: Vec<String>,
    /// Pages of wrapped lines and the width they were wrapped at; the page shown.
    pub pages: Option<(f32, Vec<Vec<String>>)>,
    pub page: usize,
    /// Pages already written to the log (FUN_10131e30 writes each as it opens).
    pub logged: usize,
}

impl Msg {
    /// A page that is not the last waits for a key too.
    pub fn waits_key(&self) -> bool {
        self.prompt || self.pages.as_ref().is_some_and(|(_, p)| self.page + 1 < p.len())
    }
}

/// The open choice menu (op 0x24): options are (index in the message, text).
pub struct Choice {
    pub question: String,
    pub options: Vec<(u32, String)>,
    pub cursor: usize,
    pub scroll: usize,
}

/// A bound actor as drawn while the event runs: the VM body (FUN_1008cce0: actor +0x260 +0x140 / +0x150),
/// the rotation eased toward it (FUN_100c4390) and the motion ratio (FUN_100c68f0).
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub pos: Vec3,
    pub yaw: f32,
    pub ratio: f32,
    /// +0x12c bit 1: still turning (FUN_1008b680).
    pub turning: bool,
    /// The direction last seen (+0xe4), for the step motions of a turn.
    dir: f32,
}

/// A map marker (op 0x8b, FUN_101efdf0): zone, slot, x, z, name.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub zone: u16,
    pub slot: u16,
    pub x: f32,
    pub z: f32,
    pub name: String,
}

pub struct State {
    pub start: EventStart,
    pub msg: Option<Msg>,
    pub choice: Option<Choice>,
    /// Actors hidden by the script (op 0x22 / 0x4e, actor +0x120 bit 0x20000; FUN_1008cdc0); seeded at bind
    /// with the server-hidden ones (FUN_100bb300 -> FUN_1008cda0).
    pub hidden: HashSet<u32>,
    /// Actors the script faded (op 0x6c); made opaque again when the event ends (inferred).
    faded: HashSet<u32>,
    pub idle: HashMap<u32, String>,
    /// World ids of the bound actors (+0x120 bit 7) and how each is drawn.
    pub bound: HashSet<u32>,
    pub poses: HashMap<u32, Pose>,
    /// Snapped by 0x36 / 0x37 this frame (FUN_100917e0).
    snap: HashSet<u32>,
    /// Despawned while bound (0x1009a04d keeps an event participant); removed at the end.
    despawned: HashSet<u32>,
    /// 0x46: true = normal camera (FUN_10082b20).
    camera_flag: bool,
    /// s2c 0x052 mode 0 arrived.
    released: bool,
    /// Cancel by Escape: DAT_10475cb0 (ops 0x2e / 0x42, default on), DAT_10473068 (in effect once the
    /// event has loaded, DAT_104760f8) and DAT_1046df04 (op 0x38; bit 0x2000 keeps 0x052 mode 0 out).
    cb0: bool,
    pub cancellable: bool,
    loaded: bool,
    flags_df04: u32,
    idle_since: Option<Instant>,
    rng: u32,
    /// Op 0x77: hour and weather while the event runs (None = 0xff: that one stays); 0x78 drops it.
    pub time: Option<(Option<u32>, Option<u16>)>,
    /// FUN_10132030 on the event message windows: +0x48 on, +0x4a x, +0x4c y (ops 0x67 / 0x68), and the text
    /// codes' x / y (DAT_10476f98 / 9a).
    place_on: bool,
    place: (i16, i16),
    code_xy: (i16, i16),
    /// Ticks waiting for the next VM step.
    vm_ticks: f32,
}

pub struct Live {
    pub rt: Runtime,
    pub st: State,
}

/// An event waiting for its actors (FUN_100ad000 -> FUN_100ad460).
pub struct Waiting {
    pub start: EventStart,
    blocks: Option<Vec<Block>>,
    /// DAT_104755d8: indexes already asked for with 0x016, and the re-send timer DAT_10475ce8.
    asked: HashSet<u16>,
    timer: f32,
    /// Actors that took part already (+0x120 bit 8), exempt from the hide flags.
    pub joined: HashSet<u32>,
    since: Instant,
}

impl State {
    fn new(start: EventStart) -> Self {
        State {
            start,
            msg: None,
            choice: None,
            hidden: HashSet::new(),
            faded: HashSet::new(),
            idle: HashMap::new(),
            bound: HashSet::new(),
            poses: HashMap::new(),
            snap: HashSet::new(),
            despawned: HashSet::new(),
            camera_flag: false,
            released: false,
            cb0: true,
            cancellable: false,
            loaded: false,
            flags_df04: 0,
            idle_since: None,
            rng: 1,
            time: None,
            place_on: false,
            place: (0, 0),
            code_xy: (0, 0),
            vm_ticks: 0.0,
        }
    }

    /// Ops 0x2e / 0x42 (FUN_100b3930 / FUN_100b3960): cancelling allowed / not (DAT_10475cb0, and DAT_10473068
    /// once loaded); 0x38 (FUN_100b44e0): DAT_1046df04 = val@1 | 0x2000.
    fn cancel_op(&mut self, op: u8, args: &[Arg]) {
        match op {
            0x2e | 0x42 => {
                self.cb0 = op == 0x2e;
                if self.loaded {
                    self.cancellable = self.cb0;
                }
            }
            0x38 => {
                let v = match args.first() {
                    Some(Arg::Int(v)) => *v as u32,
                    _ => 0,
                };
                self.flags_df04 = v | 0x2000;
            }
            _ => {}
        }
    }

    /// DAT_1046df04 | the event Mode (DAT_1046d210).
    fn flags(&self) -> u32 {
        self.flags_df04 | self.start.mode as u32
    }
}

/// The world side of the VM.
struct LiveHost<'a> {
    w: &'a mut World,
    st: &'a mut State,
    index: &'a DatIndex,
}

impl LiveHost<'_> {
    /// Event actor id -> world id.
    fn wid(&self, a: u32) -> u32 {
        if a == PLAYER { self.w.my_id } else { a }
    }

    /// Op 0xb6 (FUN_100b1150) on the event actor's look (0x16: the player's): 0 face, 1 race, 2..0xa slot sub - 2
    /// (face head body hands legs feet main sub range), 0xb race and slots 0..7, 0xc the fixed model val@2 (type 7),
    /// 0x16 race, face and slots 1..8 of the player. 0xd / 0xe (race and slots with flags) and the scales 0xf / 0x17
    /// are not applied yet.
    fn event_look(&mut self, sub: u8, actor: u32, args: &[Arg]) {
        use crate::dat::npc::NpcLook;
        let id = if sub == 0x16 { self.w.my_id } else { self.wid(actor) };
        let v = |k: usize| Self::arg_int(args, k).unwrap_or(0) as u16;
        let cur = self.w.actors.get(&id).and_then(|a| a.source);
        let mut look = match cur {
            Some(super::Source::Pc(l)) | Some(super::Source::Npc(NpcLook::Equipped(l))) => l,
            _ => crate::dat::character::Look::default(),
        };
        let set = |l: &mut crate::dat::character::Look, slot: usize, val: u16| {
            let val = val & 0x0fff;
            match slot {
                0 => l.face = val,
                1 => l.head = val,
                2 => l.body = val,
                3 => l.hands = val,
                4 => l.legs = val,
                5 => l.feet = val,
                6 => l.main = val,
                7 => l.sub = val,
                _ => {}
            }
        };
        let source = match sub {
            0 => {
                set(&mut look, 0, v(0));
                super::Source::Pc(look)
            }
            1 => {
                look.race = v(0) as u8;
                super::Source::Pc(look)
            }
            2..=0xa => {
                set(&mut look, sub as usize - 2, v(0));
                super::Source::Pc(look)
            }
            0xb => {
                look.race = v(0) as u8;
                (0..8).for_each(|k| set(&mut look, k, v(k + 1)));
                super::Source::Pc(look)
            }
            0xc => super::Source::Npc(NpcLook::Model(v(0))),
            0x16 => {
                look.race = v(0) as u8;
                (0..9).for_each(|k| set(&mut look, k, v(k + 1)));
                super::Source::Pc(look)
            }
            _ => return,
        };
        if cur.is_none_or(|c| format!("{c:?}") != format!("{source:?}")) {
            self.w.event_looks.push((id, source));
        }
    }

    fn send(&self, c: NetCommand) {
        let _ = self.w.net.commands.send(c);
    }

    /// Where an actor is drawn now: its VM body when bound, else its own position.
    fn drawn_at(&self, id: u32) -> Option<Vec3> {
        if let Some(p) = self.st.poses.get(&id) {
            return Some(p.pos);
        }
        if id == self.w.my_id { Some(self.w.pos) } else { self.w.entities.get(&id).map(|e| e.pos) }
    }

    fn arg_actor(&self, args: &[Arg], i: usize) -> Option<u32> {
        match args.get(i) {
            Some(Arg::Actor(a)) => Some(self.wid(*a)),
            _ => None,
        }
    }

    fn action_file(&mut self, kind: u8, id: u32, a: u32, b: u32) {
        use crate::fx::sched::Who;
        use crate::net::action::{Action, ActionResult, ActionTarget};
        if let Some(file) = crate::fx::event_action_file(kind, id) {
            self.w.effects.on_event_scheduler(file, u32::from_le_bytes(*b"main"), Who::Actor(a), Who::Actor(b));
            return;
        }
        // Vtable +0x3e4 builds an action of one zeroed record (DAT_1032cfac) whose animation is `id`.
        let category = match kind {
            0x10 => 3,
            0x11 => 1,
            0x12 => 0xb,
            _ => return,
        };
        let result = ActionResult { animation: id as u16, ..Default::default() };
        let act = Action { actor: a, category, targets: vec![ActionTarget { id: b, results: vec![result] }], ..Default::default() };
        if category == 1 {
            // FUN_100d67b0 (characters only): a swing unless one is already playing (+0x2a4) *(inf: not checked)*.
            if let Some((_, swing)) = super::combat::start_attack(self.w, a, Some(b)) {
                self.w.effects.on_action(&act, Some(swing));
            }
        } else {
            self.w.effects.on_action(&act, None);
        }
    }

    fn arg_int(args: &[Arg], i: usize) -> Option<i32> {
        match args.get(i) {
            Some(Arg::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// FUN_100b6980: `who` looks at `target`'s attach point `point` (look mode 1).
    fn look_at(&mut self, who: u32, target: u32, point: u16) {
        if !self.w.actors.get(&target).is_some_and(|a| a.character.is_some()) {
            return;
        }
        if let Some(a) = self.w.actors.get_mut(&who).filter(|a| a.character.is_some()) {
            a.look.mode = super::actmotion::LookMode::Actor(target, point);
        }
    }
}

impl Host for LiveHost<'_> {
    fn effect(&mut self, e: Effect) {
        log::debug!("event: {}", crate::event_vm::describe(&e));
        let index = self.st.start.index;
        match e {
            Effect::Message { id, text, prompt, wait, speaker, op, place } => {
                // FUN_100bf790: 02 / 03 set DAT_10476f98 / 9a and place the window there (FUN_10132030(1, x, y)).
                if place.iter().any(Option::is_some) {
                    self.st.code_xy = (place[0].unwrap_or(self.st.code_xy.0), place[1].unwrap_or(self.st.code_xy.1));
                    self.st.place_on = true;
                    self.st.place = self.st.code_xy;
                }
                // FUN_100c14d0 -> FUN_10086710: the speaker's mouth moves (chars + 20) / 20 times.
                if let Some(s) = speaker.filter(|_| op != 0x48) {
                    let id = self.wid(s);
                    if let Some(a) = self.w.actors.get_mut(&id) {
                        a.mouth = Some(super::actmotion::Mouth::new(text.chars().count()));
                    }
                }
                let lines = text.split('\n').map(str::to_string).collect();
                // Inferred: a 7f 34 / 7f 38 wait closes the line by itself, even before a 7f 31.
                self.st.msg = Some(Msg { id, prompt: prompt && wait.is_none(), wait: wait.unwrap_or(0) as f32, open: 0.0, lines, pages: None, page: 0, logged: 0 });
            }
            Effect::Choice { question, options, cursor, .. } => {
                self.st.choice = Some(Choice { question, options, cursor, scroll: cursor.saturating_sub(2) });
            }
            Effect::Scheduler { file, tag, actor, target, param } => {
                // The same CMoSchedularTask runs the effects side (generators, sounds, motions): the set of a
                // cutscene staged off the map is built from it.
                let who = |a: u32| if a == 0 { crate::fx::sched::Who::None } else { crate::fx::sched::Who::Actor(self.wid(a)) };
                self.w.effects.on_event_scheduler(file, u32::from_le_bytes(crate::dat::generator::fourcc(&tag)), who(actor), who(target));
                let Some(f) = self.w.screen.file(self.index, file) else { return };
                let Some(ops) = sched_ops(&f.bytes, &tag) else {
                    log::warn!("event: no scheduler {tag} in {file}");
                    return;
                };
                let total: f32 = ops.iter().map(|o| o.wait as f32).sum();
                let len = crate::event_vm::scheduler_ticks(&f.bytes, &tag, param);
                let speed = match (param, len) {
                    (p, Some(l)) if p > 1 && total > 0.0 => l / total,
                    _ => 1.0,
                };
                let sc = &mut self.w.screen;
                let now = sc.tick;
                sc.scheds.retain(|x| !(x.file == file && x.tag == tag && x.actor == actor && x.target == target));
                sc.scheds.push(Sched { file, tag, actor, target, ops, cursor: 0, next_at: now, start: now, len, speed });
                let (cam, follow) = (self.w.last_camera, follow_key(self.w));
                self.w.screen.advance(cam, follow);
            }
            Effect::SchedulerStop { file, tag, actor, target } => {
                let who = |a: u32| if a == 0 { crate::fx::sched::Who::None } else { crate::fx::sched::Who::Actor(self.wid(a)) };
                self.w.effects.stop_event_scheduler(file, u32::from_le_bytes(crate::dat::generator::fourcc(&tag)), who(actor), who(target));
                self.w.screen.scheds.retain(|x| !(x.file == file && x.tag == tag && x.actor == actor && x.target == target));
            }
            // 0x46 sub 0 kills the camera tasks (FUN_1003c780) and puts the camera behind the player.
            Effect::Camera { flag } => {
                self.st.camera_flag = flag;
                if flag {
                    self.w.screen.shot = None;
                    let dir = self.w.net.me.lock().map(|m| m.dir).unwrap_or(0);
                    let f = super::forward_from_dir(dir);
                    super::camera::face(self.w, f.x.atan2(f.z));
                }
            }
            Effect::Volume { mask, level, ticks } => self.w.audio_cmds.push((mask, level, ticks)),
            // FUN_100b5800: from the model's alpha to u8 val@5 over val@7 frames (0 -> 1).
            Effect::ActorFade { actor, alpha, frames } => {
                let id = self.wid(actor);
                if let Some(a) = self.w.actors.get_mut(&id) {
                    a.fade = Some(super::actmotion::Fade::to(a.alpha, alpha.clamp(0, 0xff) as u8, frames));
                    self.st.faded.insert(id);
                }
            }
            Effect::Motion { op, actor, file, name, .. } => {
                let id = self.wid(actor);
                if let Some(a) = self.w.actors.get_mut(&id) {
                    let files = motion_files(op, file, id, a);
                    a.pending = Some(super::actmotion::Pending { files, name, kind: super::actmotion::Kind::Event });
                }
            }
            // FUN_100b3870 (0x5f sub 2): drop the event motion files (FUN_100d2340(2)).
            Effect::Op { op: 0x5f, sub: Some(2), actor, .. } => {
                let id = self.wid(actor);
                if let Some(a) = self.w.actors.get_mut(&id) {
                    super::actmotion::drop_kind(&mut a.extra, super::actmotion::Kind::Event);
                    a.play = None;
                }
            }
            // FUN_100b53b0 also clears the model's default name (+0x7d8), so the default comes back now.
            Effect::IdleMotion { actor, name } => {
                let id = self.wid(actor);
                self.st.idle.insert(id, name);
                if let Some(p) = self.w.actors.get_mut(&id).and_then(|a| a.play.as_mut()) {
                    p.cut();
                }
            }
            Effect::Send(p) => match p {
                Packet::EventUpdate { head, option } => self.send(NetCommand::EventEnd { target: head.target, index, option, zone: head.zone, event: head.event, mode: 1 }),
                Packet::EventEnd { head, option } => self.send(NetCommand::EventEnd { target: head.target, index, option, zone: head.zone, event: head.event, mode: 0 }),
                Packet::EventPos { head, x, y, z, dir, option } => {
                    self.send(NetCommand::EventPos { pos: [x, y, z], dir: dir_of(dir), target: head.target, index, option, zone: head.zone, event: head.event })
                }
                Packet::Other { op, sub } => log::info!("event: packet op {op:#x} sub {sub} not sent"),
            },
            Effect::Position { op: 0x36 | 0x37, actor, .. } => {
                let id = self.wid(actor);
                self.st.snap.insert(id);
            }
            // 0x4b on an unbound actor turns the actor itself (+0x18).
            Effect::Position { op: 0x4b, actor, dir, .. } => {
                let id = self.wid(actor);
                if !self.st.bound.contains(&id)
                    && let Some(e) = self.w.entities.get_mut(&id)
                {
                    e.dir = dir_of(dir);
                }
            }
            // FUN_100b1090 / FUN_100b4940 -> FUN_100b6980(who, target, 6); 0x4a also turns unbound actors (+0x18).
            Effect::Face { actor, target } => {
                let (who, to) = (self.wid(actor), self.wid(target));
                self.look_at(who, to, 6);
                if !self.st.bound.contains(&who)
                    && let Some(tp) = self.drawn_at(to)
                    && let Some(e) = self.w.entities.get_mut(&who)
                {
                    let d = tp - e.pos;
                    e.dir = dir_of((-d.z).atan2(d.x));
                }
            }
            Effect::Op { op: 0x22, actor, args, .. } => {
                let id = self.wid(actor);
                self.hide(id, &args, 0);
            }
            Effect::Op { op: 0x4e, args, .. } => {
                if let Some(Arg::Actor(a)) = args.get(1) {
                    let id = self.wid(*a);
                    self.hide(id, &args, 0);
                }
            }
            // FUN_100b5070: sub 0..7 music slot = val@2; 0x80..0x87 the same with a volume.
            Effect::Op { op: 0x5c, sub: Some(sub), args, .. } if sub < 8 || (0x80..0x88).contains(&sub) => {
                if let Some(Arg::Int(id)) = args.get(1) {
                    let vol = if sub >= 0x80 { Self::arg_int(&args, 2).unwrap_or(0x7f) } else { 0x7f };
                    self.w.music.set_slot_vol((sub & 7) as u16, *id as u16, vol.clamp(0, 0xff) as u16);
                }
            }
            Effect::Op { op: 0x5c, sub: Some(0xa0 | 0xa1), args, .. } => {
                if let (Some(v), Some(t)) = (Self::arg_int(&args, 1), Self::arg_int(&args, 2)) {
                    self.w.music_fades.push((v, t));
                }
            }
            Effect::Op { op: 0x5d, args, .. } => {
                if let (Some(v), Some(t)) = (Self::arg_int(&args, 0), Self::arg_int(&args, 1)) {
                    self.w.music_fades.push((v, t));
                }
            }
            // FUN_100b6880: the weather applies even with hour 0xff (the clock then stays); FUN_100b6920 restores.
            Effect::Op { op: 0x77, args, .. } => {
                if let (Some(h), Some(wt)) = (Self::arg_int(&args, 0), Self::arg_int(&args, 1)) {
                    self.st.time = Some(((h != 0xff).then_some(h as u32), (wt != 0xff).then_some(wt as u16)));
                }
            }
            Effect::Op { op: 0x78, .. } => self.st.time = None,
            // FUN_10178900(val, 1): the sub-area loads (no 0x0F2); sub 2 tells the server (0x0F2 state 2).
            Effect::Op { op: 0x75, sub: Some(0), args, .. } => {
                if let Some(id) = Self::arg_int(&args, 0) {
                    self.w.sub.asked = Some((id as u32, true));
                }
            }
            Effect::Op { op: 0x75, sub: Some(2), args, .. } => {
                if let Some(id) = Self::arg_int(&args, 0) {
                    let _ = self.w.net.commands.send(super::net::NetCommand::SubMap { state: 2, id: id as u16 });
                }
            }
            // FUN_100b4590 / FUN_100bb340: actor A runs an action file's "main" on actor B (model vtable +0x2c4..
            // +0x2ec); 0xc4 0 / 2 play a weapon skill / mob skill as a one-record action, 1 a melee swing.
            Effect::Op { op: op @ (0x73 | 0xad | 0xc4), sub, args, .. } => {
                let (kind, rest) = match op {
                    0x73 => (1, &args[..]),
                    0xad => (Self::arg_int(&args, 0).filter(|k| k & 0xf0 == 0).unwrap_or(-1) as u8, args.get(1..).unwrap_or_default()),
                    _ => (sub.map_or(0xff, |s| 0x10 + s), &args[..]),
                };
                if let (Some(id), Some(Arg::Actor(a)), Some(Arg::Actor(b))) = (Self::arg_int(rest, 0), rest.get(1), rest.get(2))
                    && *a != 0
                    && *b != 0
                {
                    self.action_file(kind, id as u32, self.wid(*a), self.wid(*b));
                }
            }
            // FUN_100b8b20: the player runs file 0x13f8 + val's "main" on itself.
            Effect::Op { op: 0x7d, args, .. } => {
                if let Some(id) = Self::arg_int(&args, 0) {
                    let me = crate::fx::sched::Who::Actor(self.w.my_id);
                    self.w.effects.on_event_scheduler(0x13f8 + id as u32, u32::from_le_bytes(*b"main"), me, me);
                }
            }
            Effect::Op { op: 0xb6, sub: Some(sub), actor, args } => self.event_look(sub, actor, &args),
            // Zone actions (XiZone, DAT_106175ac vtable +0x18 / +0x1c): schedulers of the zone's effects by 4cc.
            Effect::Op { op: op @ (0x2d | 0x51), args, .. } => {
                let who = |a: Option<&Arg>| match a {
                    Some(Arg::Actor(0)) | None => crate::fx::sched::Who::None,
                    Some(Arg::Actor(a)) => crate::fx::sched::Who::Actor(self.wid(*a)),
                    _ => crate::fx::sched::Who::None,
                };
                let (a, b) = (who(args.first()), who(args.get(1)));
                if let (Some(Arg::Name(n)), Some(file)) = (args.get(2), self.w.effects.zone_file()) {
                    let name = u32::from_le_bytes(crate::dat::generator::fourcc(n));
                    if op == 0x2d { self.w.effects.on_event_scheduler(file, name, a, b) } else { self.w.effects.stop_event_scheduler(file, name, a, b) }
                }
            }
            Effect::Op { op: 0x60, sub: Some(2), args, .. } => {
                if let (Some(Arg::Int(n)), Some(file)) = (args.first(), self.w.effects.zone_file()) {
                    self.w.effects.on_event_scheduler(file, *n as u32, crate::fx::sched::Who::None, crate::fx::sched::Who::None);
                }
            }
            Effect::Op { op: 0xd9, args, .. } => self.w.event_voices = Self::arg_int(&args, 0) == Some(0),
            // FUN_10184380 with the area: the App loads it (or brings the zone's scene back) and ends the wait.
            Effect::Op { op: 0x34 | 0x35, args, .. } => match args.first() {
                Some(Arg::Int(v)) if (0..0x10000).contains(v) => self.w.area.want = Some(*v as u16),
                _ => self.w.area.done = true,
            },
            Effect::Op { op: op @ (0x2e | 0x42 | 0x38), args, .. } => self.st.cancel_op(op, &args),
            // FUN_100b5b90 / FUN_100b5bf0: FUN_10132030(1, x, y) / (0, 0, 0) on both event message windows.
            Effect::Op { op: 0x67, args, .. } => {
                self.st.place_on = true;
                self.st.place = (Self::arg_int(&args, 0).unwrap_or(0) as i16, Self::arg_int(&args, 1).unwrap_or(0) as i16);
            }
            Effect::Op { op: 0x68, .. } => {
                self.st.place_on = false;
                self.st.place = (0, 0);
            }
            // FUN_100b6a50: sub 0 / 1 look at an actor's point (6 or val@10), sub 2 along its facing.
            Effect::Op { op: 0x79, sub, args, actor } => match sub {
                Some(0) | Some(1) => {
                    if let (Some(who), Some(to)) = (self.arg_actor(&args, 0), self.arg_actor(&args, 1)) {
                        let point = if sub == Some(1) { Self::arg_int(&args, 2).unwrap_or(6) as u16 } else { 6 };
                        self.look_at(who, to, point);
                    }
                }
                Some(2) => {
                    let who = self.arg_actor(&args, 0).unwrap_or(self.wid(actor));
                    let (pitch, yaw) = (Self::arg_int(&args, 1).unwrap_or(0) as i16, Self::arg_int(&args, 2).unwrap_or(0) as i16);
                    if let Some(a) = self.w.actors.get_mut(&who).filter(|a| a.character.is_some()) {
                        a.look.mode = super::actmotion::LookMode::Facing(pitch, yaw);
                    }
                }
                _ => {}
            },
            // FUN_100b6b70: stop looking; the head comes back as the weight ramps down.
            Effect::Op { op: 0x7b, args, actor, .. } => {
                let who = self.arg_actor(&args, 0).unwrap_or(self.wid(actor));
                if let Some(a) = self.w.actors.get_mut(&who) {
                    a.look.mode = super::actmotion::LookMode::Off;
                }
            }
            // FUN_100b5db0 -> FUN_1009f200: emote val@5 through table 0x103504a8.
            Effect::Op { op: 0x6e, args, .. } => {
                if let (Some(who), Some(n)) = (self.arg_actor(&args, 0), Self::arg_int(&args, 1))
                    && let Some(a) = self.w.actors.get_mut(&who)
                {
                    super::actmotion::emote(a, n as u32 & 0xff);
                }
            }
            // FUN_100b08e0(1, zone, map, flag, 0) opens the map; FUN_100b0dd0 closes it.
            Effect::Op { op: 0xc8, args, .. } => {
                let zone = Self::arg_int(&args, 0).unwrap_or(0) as u16;
                self.w.map_open = Some(zone);
                log::info!("event: map window, zone {zone}");
            }
            Effect::Op { op: 0x8a, .. } => self.w.map_open = None,
            // FUN_100b7600 -> FUN_101efdf0 / FUN_101efe80 / FUN_101efed0: marker slot at x, z with a name ('_' = space).
            Effect::Op { op: 0x8b, args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let name = match args.get(4) {
                    Some(Arg::Name(n)) => n.replace('_', " "),
                    _ => String::new(),
                };
                let m = Marker { zone: int(0) as u16, slot: int(1) as u16, x: int(2) as f32 * 0.001, z: int(3) as f32 * 0.001, name };
                log::info!("event: map marker {m:?}");
                self.w.map_markers.retain(|x| !(x.zone == m.zone && x.slot == m.slot));
                self.w.map_markers.push(m);
            }
            Effect::Fault { actor, pc, op } => log::warn!("event: op {op:#x} at {pc:#x} on {actor:#x} cannot run"),
            _ => {}
        }
    }

    fn busy(&mut self, b: &Busy) -> bool {
        match b {
            Busy::Scheduler { file, tag, actor, target } => self.w.screen.scheds.iter().any(|s| s.file == *file && &s.tag == tag && s.actor == *actor && s.target == *target),
            // Done turning: FUN_10089810 stops the step schedulers.
            Busy::Turning { actor } => {
                let id = self.wid(*actor);
                let turning = self.st.poses.get(&id).is_some_and(|p| p.turning);
                if !turning
                    && let Some(a) = self.w.actors.get_mut(&id)
                    && a.play.as_ref().is_some_and(|p| p.name == "@tl0" || p.name == "@tr0")
                {
                    a.play = None;
                }
                turning
            }
            Busy::MotionLoad { actor, .. } => {
                let id = self.wid(*actor);
                self.w.actors.get(&id).is_some_and(|a| a.pending.is_some())
            }
            // FUN_10083650: the scheduler task of that name still exists (or its files still load).
            Busy::Playing { actor, name } => {
                let id = self.wid(*actor);
                let Some(a) = self.w.actors.get(&id) else { return false };
                let n = name.trim_end_matches('?');
                a.pending.as_ref().is_some_and(|p| p.name == *name)
                    || a.play.as_ref().is_some_and(|p| p.name == *name && p.busy())
                    || a.oneshot.as_ref().is_some_and(|o| o.name.starts_with(n) && o.ticks < o.len)
            }
            // Asked before the op's effect runs, so the first frame waits too (retail spends three frames on it).
            Busy::Op { op: 0x34 | 0x35, .. } => !std::mem::take(&mut self.w.area.done),
            Busy::Op { op: 0x6c, actor, .. } => {
                let id = self.wid(*actor);
                self.w.actors.get(&id).is_some_and(|a| a.fade.is_some())
            }
            // Actor +0x1b8 (action state) is nonzero while an emote loads and plays.
            Busy::ZoneAction { name, a, b } => {
                let who = |x: &u32| if *x == 0 { crate::fx::sched::Who::None } else { crate::fx::sched::Who::Actor(self.wid(*x)) };
                let n = u32::from_le_bytes(crate::dat::generator::fourcc(name));
                self.w.effects.zone_file().is_some_and(|f| self.w.effects.event_scheduler_running(f, n, who(a), who(b)))
            }
            // FUN_100b8e90: an actor of the event waits for its model and resources; ours waits while that load or its
            // motion files are in flight (an actor that never gets a model does not hold the event).
            Busy::Op { op: 0x80, actor, .. } => {
                let id = self.wid(*actor);
                self.w.actors.get(&id).is_some_and(|a| (a.character.is_none() && self.w.models_loading.contains(&a.key)) || a.pending.is_some())
            }
            // 0x9a: until the music change has loaded.
            Busy::Op { op: 0x9a, .. } => self.w.music_changing,
            // FUN_10178780: a sub-area load is running.
            Busy::Op { op: 0x75, .. } => self.w.sub.loading.is_some() || self.w.sub.asked.is_some(),
            // Op 0xb6 sub 0x10: until the player's model is up.
            Busy::Op { op: 0xb6, sub: Some(0x10), .. } => self.w.actors.get(&self.w.my_id).is_none_or(|a| a.character.is_none()),
            Busy::Op { op: 0x99, actor, .. } | Busy::Op { op: 0x59, sub: Some(6), actor } => {
                let id = self.wid(*actor);
                self.w.actors.get(&id).is_some_and(super::actmotion::emoting)
            }
            _ => false,
        }
    }

    /// FUN_10089c70 and +0x120 bit 9: the actor exists (all bound ones do once the event starts).
    fn ready(&self, actor: u32) -> bool {
        let id = self.wid(actor);
        id == self.w.my_id || self.w.entities.contains_key(&id)
    }

    fn name(&self, actor: u32) -> String {
        let id = self.wid(actor);
        if id == self.w.my_id {
            return self.w.name.clone();
        }
        self.w.entities.get(&id).map(|e| e.name.clone()).filter(|n| !n.is_empty()).or_else(|| self.w.npc_names.get(&id).cloned()).unwrap_or_default()
    }

    /// Hume F, Elvaan F, Tarutaru F, Mithra.
    fn female(&self) -> bool {
        matches!(self.w.my_race, 2 | 4 | 6 | 7)
    }

    fn game_config(&self, id: u16) -> u32 {
        self.w.game.get(id)
    }

    fn place(&self, actor: u32) -> ([f32; 3], f32) {
        let id = self.wid(actor);
        if id == self.w.my_id {
            let dir = self.w.net.me.lock().map(|m| m.dir).unwrap_or(0);
            return (self.w.pos.to_array(), rad_of(dir));
        }
        self.w.entities.get(&id).map_or(([0.0; 3], 0.0), |e| (e.pos.to_array(), rad_of(e.dir)))
    }

    fn ground(&self, _actor: u32, p: [f32; 3]) -> Option<f32> {
        self.w.ground.as_ref().and_then(|g| g.settle(p[0], p[2], p[1]))
    }

    fn rand(&mut self) -> u32 {
        self.st.rng = self.st.rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.st.rng >> 16) & 0x7fff
    }
}

impl LiveHost<'_> {
    /// Op 0x22 / 0x4e: u8@1 = 1 hides the actor while the event runs.
    fn hide(&mut self, id: u32, args: &[Arg], at: usize) {
        if let Some(Arg::Int(v)) = args.get(at) {
            if *v != 0 {
                self.st.hidden.insert(id);
            } else {
                self.st.hidden.remove(&id);
            }
        }
    }
}

/// An event start from the server, or the login event (started once the zone is up).
pub fn queue(w: &mut World, s: EventStart) {
    if w.event.is_some() {
        // 1009e8a0: an event is already running (group 9 message 0x81).
        log::warn!("event {:#x} ignored: event {:#x} is running", s.event, w.event.as_ref().map_or(0, |l| l.st.start.event));
        return;
    }
    log::info!("event start: zone {} event {:#x} target {:#x} index {} mode {:#x} params {:?}", s.zone, s.event, s.target, s.index, s.mode, s.params);
    // FUN_100ad2c0 clears the post-event re-request (+0x1f8 / +0x1fa) of every actor.
    w.self_lock = None;
    w.event_pending = Some(Box::new(Waiting { start: s, blocks: None, asked: HashSet::new(), timer: ASK_AGAIN, joined: HashSet::new(), since: Instant::now() }));
    w.menu = None;
    w.action_menu = None;
    w.win = None;
    w.confirm = None;
    w.typing = false;
}

fn zone_ready(w: &World) -> bool {
    w.zone == Some(w.pos_zone) && w.renderer.is_some() && !w.unsettled && w.zoning_since.is_none()
}

/// FUN_100bb290: the block runs in this event (its id, or the 0xfffe wildcard).
fn bound_block(b: &Block, event: u16) -> bool {
    b.events.iter().any(|(id, _)| *id == event || *id == 0xfffe)
}

/// FUN_100ad740: (world id, ActIndex) of a block's actor, `me` being ours; party and alliance slots are not
/// modelled.
fn resolve(me: (u32, u16), actor: u32) -> Option<(u32, u16)> {
    match actor {
        0x7fff_ffc0 | PLAYER => Some(me),
        0x7fff_ffc1..=0x7fff_fff5 => None,
        _ => Some((actor, (actor & 0x3ff) as u16)),
    }
}

/// FUN_100ad460's walk in block order: the first bound actor that is missing (`true`) or still without its
/// model (`false`); None once all are ready. Every actor passed takes part (`joined`, +0x120 bit 8).
fn first_waiting(blocks: &[Block], event: u16, me: (u32, u16), present: &dyn Fn(u32) -> bool, model: &dyn Fn(u32) -> bool, joined: &mut HashSet<u32>) -> Option<(u32, u16, bool)> {
    for b in blocks.iter().filter(|b| bound_block(b, event)) {
        let Some((id, index)) = resolve(me, b.actor) else { continue };
        if !present(id) {
            return Some((id, index, true));
        }
        joined.insert(id);
        if !model(id) {
            return Some((id, index, false));
        }
    }
    None
}

/// +0x120 bit 9: the model is up. Entities that carry no look have no model to wait for, and a model that
/// failed to load counts as loaded (inferred: retail has every model).
fn has_model(w: &World, id: u32) -> bool {
    w.actors.get(&id).is_none_or(|a| a.character.is_some() || a.failed)
}

/// FUN_100ad460 / FUN_100ad280: every bound actor must exist with a full update (+0x141) and a model. The
/// first one missing is asked for with 0x016 (once until the 120-tick timer clears the list); it returns at
/// that actor, so the requests go one at a time.
fn actors_ready(w: &World, wt: &mut Waiting, ticks: f32) -> bool {
    let ev = wt.start.event;
    let present = |id: u32| id == w.my_id || w.entities.contains_key(&id);
    let model = |id: u32| has_model(w, id);
    let blocks = wt.blocks.as_deref().unwrap_or_default();
    let waiting = first_waiting(blocks, ev, (w.my_id, w.my_index), &present, &model, &mut wt.joined);
    // Not retail, which asks for the first missing actor and waits for it: every missing one is asked for at
    // once. With LSB answering only the client's 400 ms datagrams, the San d'Oria opening (29 actors across the
    // city) otherwise froze the player for 12 s before the event began.
    let mut asked = 0;
    for b in blocks.iter().filter(|b| bound_block(b, ev)) {
        if let Some((id, index)) = resolve((w.my_id, w.my_index), b.actor)
            && !present(id)
            && wt.asked.insert(index)
        {
            let _ = w.net.commands.send(NetCommand::CharReq(index));
            asked += 1;
        }
    }
    if asked > 0 {
        wt.timer = ASK_AGAIN;
        log::info!("event {ev:#x}: asking for {asked} actors after {:.2} s", wt.since.elapsed().as_secs_f32());
    }
    let ready = waiting.is_none();
    if !ready {
        wt.timer -= ticks;
        if wt.timer < 0.0 {
            wt.asked.clear();
            wt.timer = ASK_AGAIN;
        }
    }
    ready
}

fn begin(w: &mut World, mut wt: Box<Waiting>, index: &DatIndex) {
    let s = wt.start.clone();
    let fz = file_zone(&s, w.zone.unwrap_or(s.zone));
    let blocks = wt.blocks.take().unwrap_or_default();
    log::info!("event {:#x}: actors ready after {:.2} s", s.event, wt.since.elapsed().as_secs_f32());
    let dialog = Dialog::load(index, fz).map_err(|e| log::warn!("dialogue of zone {fz}: {e:#}")).ok();
    let start = Start { zone: s.zone, event: s.event, target: s.target, params: s.params, strings: s.strings.clone() };
    let mut st = State::new(s);
    // FUN_100bb300 -> FUN_1008cda0: a server-hidden actor (+0x120 bits 14 / 15) starts the event hidden; the
    // model colour goes back to 0x80808080 unless Mode & 0x200 (FUN_10095c00).
    for b in blocks.iter().filter(|b| bound_block(b, st.start.event)) {
        if let Some((id, _)) = resolve((w.my_id, w.my_index), b.actor) {
            st.bound.insert(id);
            if w.entities.get(&id).is_some_and(|e| e.flags & 2 != 0) {
                st.hidden.insert(id);
            }
            if st.start.mode & 0x200 == 0
                && let Some(a) = w.actors.get_mut(&id)
            {
                a.alpha = super::actmotion::OPAQUE;
                a.fade = None;
            }
        }
    }
    log::info!("event {:#x}: {} actors bound, {} start hidden", st.start.event, st.bound.len(), st.hidden.len());
    let mem = w.event_mem.clone();
    let mut rt = {
        let mut host = LiveHost { w: &mut *w, st: &mut st, index };
        Runtime::start(blocks, dialog, start, mem, &mut host)
    };
    if let Some(n) = &w.names {
        rt.set_names(n.clone() as Arc<dyn crate::dat::names::NameSource>);
    }
    let me = w.my_id;
    for (a, b) in rt.actors() {
        let id = if a == PLAYER { me } else { a };
        st.poses.insert(id, Pose { pos: Vec3::from(b.pos), yaw: b.dir, ratio: 0.0, turning: false, dir: b.dir });
    }
    // FUN_100ad000 (load finished): DAT_104760f8 = 1, DAT_10473068 = DAT_10475cb0.
    st.loaded = true;
    st.cancellable = st.cb0;
    w.event = Some(Box::new(Live { rt, st }));
}

/// Per frame: the screen's tasks, a queued event waiting for its actors, the VM, and the actors it drives.
pub fn update(w: &mut World, dt: f32) {
    let Some(index) = w.index.clone() else { return };
    let ticks = dt * 60.0;
    w.screen.tick += ticks;
    let (cam, follow) = (w.last_camera, follow_key(w));
    w.screen.advance(cam, follow);
    self_request(w, ticks);
    if w.event.is_none()
        && let Some(mut wt) = w.event_pending.take()
    {
        if !zone_ready(w) {
            w.event_pending = Some(wt);
        } else {
            if wt.blocks.is_none() {
                let fz = file_zone(&wt.start, w.zone.unwrap_or(wt.start.zone));
                let blocks = crate::dat::event::load(&index, fz).map_err(|e| log::warn!("event file of zone {fz}: {e:#}")).unwrap_or_default();
                if !blocks.iter().any(|b| bound_block(b, wt.start.event)) {
                    // Nothing would ever end it: answer at once so the server lets go.
                    let s = &wt.start;
                    log::warn!("event {:#x}: no script in zone {fz}; ending it", s.event);
                    let _ = w.net.commands.send(NetCommand::EventEnd { target: s.target, index: s.index, option: 0, zone: s.zone, event: s.event, mode: 0 });
                    w.talk_lock = None;
                    return;
                }
                wt.blocks = Some(blocks);
            }
            if actors_ready(w, &mut wt, ticks) {
                begin(w, wt, &index);
            } else {
                w.event_pending = Some(wt);
            }
        }
    }
    let Some(mut live) = w.event.take() else { return };
    if let Some(m) = &mut live.st.msg {
        m.open += ticks;
        // A message without 7f 31 closes by itself (after its 7f 34 / 7f 38 wait).
        if !m.waits_key() && m.open >= m.wait.max(1.0) {
            live.rt.close_message();
            close_msg(&mut live.st, w);
        }
    }
    // The VM runs in 60 Hz ticks as at retail's frame rate (FUN_10014000 counts at least one tick a frame), so
    // a faster frame limit does not speed the script up; a slow frame runs up to 4 ticks.
    live.st.vm_ticks = (live.st.vm_ticks + ticks).min(4.0);
    let mut ran = 0;
    while live.st.vm_ticks >= 1.0 && !live.rt.done() {
        live.st.vm_ticks -= 1.0;
        ran += 1;
        let mut host = LiveHost { w: &mut *w, st: &mut live.st, index: &index };
        live.rt.tick(&mut host, 1.0);
    }
    // The bodies only move when the VM ticks: their motion ratios count VM ticks, not frames (at 300 fps a
    // frame-based ratio flipped between 0 and a capped run, and the legs blurred).
    if ran > 0 {
        sync(&mut live, w, ran as f32, ran as f32 / 60.0);
    }
    if !live.rt.done() && live.rt.idle() && live.st.msg.is_none() && live.st.choice.is_none() {
        // Every thread ended without 0x21: the server ends these (0x052); then the client cancels.
        let since = *live.st.idle_since.get_or_insert_with(Instant::now);
        if live.st.released || since.elapsed().as_secs_f32() > 10.0 {
            let s = &live.st.start;
            let _ = w.net.commands.send(NetCommand::EventEnd { target: s.target, index: s.index, option: 0x4000_0000, zone: s.zone, event: s.event, mode: 0 });
            log::info!("event {:#x}: idle, cancelled", s.event);
            finish(w, live);
            return;
        }
    }
    if live.rt.done() {
        log::info!("event {:#x} done", live.st.start.event);
        finish(w, live);
        return;
    }
    w.event = Some(live);
}

/// The message window closed: its speaker's mouth stops (FUN_100b1140 on 0x23).
fn close_msg(st: &mut State, w: &mut World) {
    st.msg = None;
    for a in w.actors.values_mut() {
        a.mouth = None;
    }
}

/// The post-event request of our own character (FUN_100ac9c0 sets +0x1f8 = 300 / +0x1fa; FUN_1008d280 sends
/// 0x016 when it runs out, then every 600 ticks) until a full 0x00D about us arrives.
fn self_request(w: &mut World, ticks: f32) {
    let Some(left) = &mut w.self_lock else { return };
    *left -= ticks;
    if *left < 0.0 {
        *left = 600.0;
        log::info!("event: asking for our own character (index {})", w.my_index);
        let _ = w.net.commands.send(NetCommand::CharReq(w.my_index));
    }
}

/// The bodies the script moved, as drawn: positions straight from the VM, the rotation eased 12.5% a tick
/// toward the direction (of travel while moving; snapped by 0x36 / 0x37), the walk / run ratio against
/// 0.8 x the body's +0x16c (FUN_1008cce0).
fn sync(live: &mut Live, w: &mut World, ticks: f32, dt: f32) {
    let me = w.my_id;
    let mut steps = Vec::new();
    for (a, b) in live.rt.actors() {
        let id = if a == PLAYER { me } else { a };
        let p = Vec3::from(b.pos);
        let snap = live.st.snap.remove(&id);
        let pose = live.st.poses.entry(id).or_insert(Pose { pos: p, yaw: b.dir, ratio: 0.0, turning: false, dir: b.dir });
        let step = p - pose.pos;
        pose.pos = p;
        let flat = Vec3::new(step.x, 0.0, step.z).length();
        let moving = flat >= 0.01 * ticks.max(1e-3);
        let want = if moving { (-step.z).atan2(step.x) } else { b.dir };
        if snap {
            pose.yaw = b.dir;
        } else {
            pose.yaw += angle_to(pose.yaw, want) * (1.0 - 0.875f32.powf(ticks));
        }
        pose.turning = angle_to(pose.yaw, b.dir).abs() > TURN_GAP;
        // A placement (snap, or a jump of over 2 a tick) is not walking *(inf)*: it set the ratio near its cap and the
        // body sprinted in place for a few ticks.
        let walked = if snap || flat > 2.0 * ticks { 0.0 } else { flat };
        pose.ratio = crate::motion::move_ratio_ref(pose.ratio, walked, dt, 0.8 * b.anim_speed);
        // FFXI_TRACE_MOVE=1: event bodies' speed and walk / run ratio (test runs).
        if flat > 1e-4 && std::env::var_os("FFXI_TRACE_MOVE").is_some() {
            log::info!("evmove {id:#x} {:.2} u/s ratio {:.2} anim_speed {:.2}", flat / dt, pose.ratio, b.anim_speed);
        }
        // FUN_1008b680 -> FUN_10089850: a new direction more than 1/8 turn away, not while walking, plays the
        // step scheduler '@tl0' (old - new in (0x200, 0x800) of 4096) or '@tr0'.
        if b.dir != pose.dir {
            let q = |r: f32| ((r / TAU * 4096.0).round() as i32).rem_euclid(4096);
            let (l, r) = (q(pose.dir - b.dir), q(b.dir - pose.dir));
            if !moving && !snap {
                if (0x201..0x800).contains(&l) {
                    steps.push((id, "@tl0", pose.ratio));
                } else if r > 0x200 {
                    steps.push((id, "@tr0", pose.ratio));
                }
            }
            pose.dir = b.dir;
        }
    }
    for (id, name, ratio) in steps {
        if let Some(a) = w.actors.get_mut(&id)
            && let Some(ch) = a.character.clone()
            && super::actmotion::scheduler(&a.extra, &ch, name).is_some()
        {
            let base = crate::motion::choose(&ch.motions, 0, ratio).0.to_string();
            super::actmotion::start(a, &ch, name, &base);
        }
    }
}

/// Ops 0x5b / 0x5f sub 3, 5 (bank 0, FUN_100d00a0) and 0x66 / 0x5f sub 4, 6 (bank 1, FUN_100d01b0, PC
/// looks with their skirt file); 0x2c plays from the files the actor has. Bank 0 is refused by model kind
/// 0 (FUN_10082d10), taken to be real players (inferred).
fn motion_files(op: u8, file: i32, id: u32, a: &super::Actor) -> Vec<u32> {
    use crate::dat::motionfile::{bank0_file, bank1_files};
    let pc = a.character.as_ref().and_then(|c| c.pc);
    match op {
        0x5b if id >> 24 != 0 => vec![bank0_file(file)],
        0x66 => pc.map_or(Vec::new(), |p| bank1_files(file, p.skirt)),
        _ => Vec::new(),
    }
}

/// FUN_100ac9c0 / FUN_100bb750 / FUN_1008d710: the bound actors go back to their own transforms and status
/// bits, event motions and looks end, deferred despawns happen.
fn finish(w: &mut World, live: Box<Live>) {
    for a in w.actors.values_mut() {
        if a.extra.iter().any(|(k, _)| *k == super::actmotion::Kind::Event) || a.pending.is_some() {
            super::actmotion::drop_kind(&mut a.extra, super::actmotion::Kind::Event);
            a.pending = None;
            a.play = None;
        }
        a.mouth = None;
        a.look.mode = super::actmotion::LookMode::Off;
    }
    for id in &live.st.faded {
        if let Some(a) = w.actors.get_mut(id) {
            a.fade = None;
            a.alpha = super::actmotion::OPAQUE;
        }
    }
    for id in &live.st.despawned {
        w.entities.remove(id);
        w.actors.remove(id);
    }
    w.event_mem = live.rt.mem.clone();
    // Masks 1 (effects) and 4 (music) back to 1.0 over 30 ticks (taken with DAT_1046dacb == 0).
    w.audio_cmds.push((1, 1.0, 30.0));
    w.audio_cmds.push((4, 1.0, 30.0));
    w.talk_lock = None;
    w.event_end_at = Some(Instant::now());
    w.screen.shot = None;
    w.map_open = None;
    // The player's post-event request (+0x1f8 = 300, +0x1fa = 1).
    if live.st.bound.contains(&w.my_id) {
        w.self_lock = Some(300.0);
    }
    // (DAT_1046df04 | Mode) & 1: the camera goes back behind the player (FUN_1001d950).
    if live.st.flags() & 1 != 0 {
        let dir = w.net.me.lock().map(|m| m.dir).unwrap_or(0);
        let f = super::forward_from_dir(dir);
        super::camera::face(w, f.x.atan2(f.z));
    }
}

/// s2c 0x052 (100a06f0).
pub fn release(w: &mut World, mode: u8, event: u16) {
    log::info!("event release mode {mode} event {event:#x}");
    match mode {
        // 100a070a: release, and cancel allowed unless (DAT_1046df04 | event Mode) & 0x2000.
        0 => {
            w.talk_lock = None;
            if let Some(l) = &mut w.event {
                l.st.released = true;
                if l.st.flags() & 0x2000 == 0 {
                    l.st.cancellable = true;
                }
            }
        }
        _ => {
            if let Some(l) = &mut w.event {
                l.rt.release(mode, event);
            }
        }
    }
}

pub fn params(w: &mut World, num: [u32; 8]) {
    if let Some(l) = &mut w.event {
        l.rt.params(num);
    } else if let Some(p) = &mut w.event_pending {
        p.start.params = num;
    }
}

/// s2c 0x05B / 0x065 position modes 1 and 2 end a 0x05C round trip (FUN_1009d500).
pub fn position_ack(w: &mut World, mode: u8) {
    if matches!(mode, 1 | 2)
        && let Some(l) = &mut w.event
    {
        l.rt.position_ack();
    }
}

pub fn active(w: &World) -> bool {
    w.event.is_some() || w.event_pending.is_some()
}

/// An event holds this actor (bound, or taking part while it waits): a despawn waits for the end.
pub fn holds(w: &mut World, id: u32) -> bool {
    if let Some(l) = &mut w.event
        && l.st.bound.contains(&id)
    {
        l.st.despawned.insert(id);
        return true;
    }
    false
}

/// A bound actor's drawn transform while the event runs.
pub fn pose(w: &World, id: u32) -> Option<Pose> {
    w.event.as_ref()?.st.poses.get(&id).copied()
}

/// The cutscene camera: the scheduler's path while the 0x46 flag is 0.
pub fn camera(w: &World) -> Option<Camera> {
    let l = w.event.as_ref()?;
    if l.st.camera_flag {
        return None;
    }
    let s = w.screen.shot.as_ref()?;
    let path = s.file.title.as_ref()?.paths.get(s.path).filter(|p| !p.keys.is_empty())?;
    let k = sample_from(path, (w.screen.tick - s.start) / s.ticks, s.from, s.follow);
    Some(Camera { eye: k.eye, target: k.target, up: Vec3::NEG_Y, fov_y: fov_y(k.focal) })
}

/// The running shot's path name (test drives wait for it).
pub fn shot_name(w: &World) -> Option<String> {
    w.event.as_ref()?;
    Some(w.screen.shot.as_ref()?.tag.clone())
}

/// FUN_1008cdc0, per actor: a bound actor while the event runs is hidden only by the script (+0x120 bit
/// 0x20000); otherwise by the server bits (+0x120 bits 14 / 15) or, unless it takes part, by (DAT_1046df04 |
/// Mode) & 0x02 for player characters and & 0x10 for NPCs and monsters (+0x120 bit 0x800000).
pub fn hidden(w: &World) -> HashSet<u32> {
    let mut out = HashSet::new();
    let (flags, script, part): (u32, Option<&HashSet<u32>>, Option<&HashSet<u32>>) = match (&w.event, &w.event_pending) {
        (Some(l), _) => (l.st.flags(), Some(&l.st.hidden), Some(&l.st.bound)),
        (None, Some(p)) => (p.start.mode as u32, None, Some(&p.joined)),
        _ => (0, None, None),
    };
    let takes_part = |id: &u32| part.is_some_and(|p| p.contains(id));
    let filtered = |id: u32| if id >> 24 == 0 { flags & 0x02 != 0 } else { flags & 0x10 != 0 };
    for (id, e) in &w.entities {
        let hide = match script {
            Some(h) if takes_part(id) => h.contains(id),
            _ => e.flags & 2 != 0 || (!takes_part(id) && filtered(*id)),
        };
        if hide {
            out.insert(*id);
        }
    }
    let me = w.my_id;
    let hide_me = match script {
        Some(h) if takes_part(&me) => h.contains(&me),
        _ => !takes_part(&me) && filtered(me),
    };
    if hide_me {
        out.insert(me);
    }
    out
}

pub fn idles(w: &World) -> HashMap<u32, String> {
    w.event.as_ref().map(|l| l.st.idle.clone()).unwrap_or_default()
}

/// The open message waits for a key (the kaipage arrow shows).
pub fn prompt(w: &World) -> bool {
    w.event.as_ref().is_some_and(|l| l.st.msg.as_ref().is_some_and(Msg::waits_key))
}

pub fn choice(w: &World) -> Option<&Choice> {
    w.event.as_ref()?.st.choice.as_ref()
}

/// The open message, for the event message window.
pub fn message(w: &mut World) -> Option<&mut Msg> {
    w.event.as_mut()?.st.msg.as_mut()
}

pub fn message_ref(w: &World) -> Option<&Msg> {
    w.event.as_ref()?.st.msg.as_ref()
}

/// The id of the open message (test drives wait for one).
pub fn message_id(w: &World) -> Option<u32> {
    w.event.as_ref()?.st.msg.as_ref().map(|m| m.id)
}

/// FUN_10132030: the event message window's placement (+0x48 on) in pixels; negative values are percentages
/// of the UI width / height less 176 / 32.
pub fn placement(w: &World, sw: f32, sh: f32) -> Option<(f32, f32)> {
    let l = w.event.as_ref()?;
    if !l.st.place_on {
        return None;
    }
    let (x, y) = l.st.place;
    let px = if x < 0 { -176.0 - sw * x as f32 / 100.0 } else { x as f32 };
    let py = if y < 0 { -32.0 - sh * y as f32 / 100.0 } else { y as f32 };
    Some((px, py))
}

/// The target window stays up in an event only with EventPara2 bit 1 (FUN_10085a00(0, 1)).
pub fn keeps_target(w: &World) -> bool {
    w.event.as_ref().is_none_or(|l| l.st.start.para2 & 1 != 0) && w.event_pending.is_none()
}

/// Keys while an event runs: Enter advances / picks, arrows move the choice cursor. Escape (key function
/// 0x0f) cancels only while DAT_10473068 allows it, with SE "0003"; c2s 0x05B then carries 0x40000000.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut l) = w.event.take() else { return };
    key_live(w, &mut l, key);
    w.event = Some(l);
}

fn key_live(w: &mut World, l: &mut Live, key: egui::Key) {
    let cancel = key == egui::Key::Escape;
    let allowed = l.st.cancellable;
    if cancel {
        log::info!("event {:#x}: Escape, cancellable {allowed}", l.st.start.event);
    }
    if let Some(c) = &mut l.st.choice {
        let n = c.options.len().max(1);
        match key {
            egui::Key::ArrowDown => c.cursor = (c.cursor + 1) % n,
            egui::Key::ArrowUp => c.cursor = (c.cursor + n - 1) % n,
            egui::Key::Enter if l.rt.choice_open() => {
                let pick = c.options.get(c.cursor).map(|o| o.0);
                l.rt.choose(pick);
                l.st.choice = None;
            }
            // FUN_10147c20 case 6: result 0xff with SE 0003 when allowed, else SE 0004 and the choice stays.
            _ if cancel && l.rt.choice_open() => {
                if allowed {
                    l.rt.choose(None);
                    l.st.choice = None;
                    w.sounds.push(crate::audio::Ui::Cancel as u32);
                } else {
                    w.sounds.push(crate::audio::Ui::Error as u32);
                }
            }
            _ => {}
        }
        if let Some(c) = &mut l.st.choice {
            c.scroll = c.scroll.min(c.cursor).max(c.cursor.saturating_sub(2));
        }
        return;
    }
    if let Some(m) = l.st.msg.as_mut().filter(|m| m.waits_key()) {
        match key {
            egui::Key::Enter => {
                // A full page (4 lines) waits for a key before the next one.
                let more = m.pages.as_ref().is_some_and(|(_, p)| m.page + 1 < p.len());
                if more {
                    m.page += 1;
                    m.open = 0.0;
                } else {
                    l.rt.close_message();
                    close_msg(&mut l.st, w);
                }
            }
            // FUN_101323f0 event 6: only while cancellable; otherwise nothing.
            _ if cancel && allowed => {
                l.rt.cancel_message();
                close_msg(&mut l.st, w);
                w.sounds.push(crate::audio::Ui::Cancel as u32);
            }
            _ => {}
        }
        return;
    }
    // FUN_10094ea0 tail: no message window and cancellable: abort (daca = def8 = 1, DAT_1047462c = 1).
    if cancel && allowed && l.st.msg.is_none() {
        let event = l.st.start.event;
        l.rt.release(2, event);
        w.sounds.push(crate::audio::Ui::Cancel as u32);
    }
}

/// A click: advances a prompt, or picks choice row `row` (None = not on a row).
pub fn click(w: &mut World, row: Option<usize>) {
    let mut enter = true;
    if let Some(c) = w.event.as_mut().and_then(|l| l.st.choice.as_mut()) {
        enter = false;
        if let Some(k) = row.map(|r| c.scroll + r).filter(|&k| k < c.options.len()) {
            if c.cursor == k {
                enter = true;
            } else {
                c.cursor = k;
            }
        }
    }
    if enter {
        key(w, egui::Key::Enter);
    }
}

/// FUN_1007aac0: Talk (c2s 0x01A action 0) after the client's checks. Returns whether it was sent.
pub fn talk(w: &mut World, target: u32) -> bool {
    if active(w) || w.talk_lock.is_some() || w.zone.is_none() {
        return false;
    }
    let Some(e) = w.entities.get(&target) else { return false };
    // UniqueNo high byte: NPCs only; a model on the target (+0x120 bit 9); not hidden by the server.
    if target >> 24 == 0 || e.index == 0 || !w.actors.contains_key(&target) || e.flags & 6 != 0 {
        return false;
    }
    if (e.pos - w.pos).length() > TALK_RANGE {
        super::combat::system_message(w, 9);
        return false;
    }
    if matches!(e.motion.status, 2 | 3) {
        return false;
    }
    let index = e.index;
    let _ = w.net.commands.send(NetCommand::Act { target, index, action: 0 });
    w.talk_lock = Some(Instant::now());
    log::info!("talk to {target:#x} ({index})");
    true
}

/// Inferred: an id in a gap just before a segment (within 8) takes that segment's delta. The gaps are
/// where the generator found no unique anchor (e.g. 230: LSB 6439 "Obtained: <item>." is DAT 6432, -7).
fn gap_map(m: &crate::dat::msgshift::MsgShift, zone: u16, lsb: u32) -> Option<u32> {
    let next = m.zones.get(&zone)?.segs.iter().filter(|s| s.from > lsb).min_by_key(|s| s.from)?;
    (next.from - lsb <= 8).then(|| lsb.checked_add_signed(next.delta)).flatten()
}

/// s2c 0x036 / 0x02A: zone dialogue by server id (mapped to the install with MsgShift), into the log.
pub fn zone_text(w: &mut World, t: &ZoneText) {
    let Some(zone) = w.zone else { return };
    let Some(index) = w.index.clone() else { return };
    if w.zone_text.as_ref().is_none_or(|(z, _)| *z != zone) {
        w.zone_text = Dialog::load(&index, zone).ok().map(|d| (zone, Arc::new(d)));
    }
    let lsb = (t.mes & 0x7fff) as u32;
    let id = match &w.msgshift {
        Some(m) if m.zones.contains_key(&zone) => m.map(zone, lsb).or_else(|| gap_map(m, zone, lsb)),
        _ => Some(lsb),
    };
    let Some((_, d)) = &w.zone_text else { return };
    let raw = id.and_then(|i| d.raw(i));
    let Some(raw) = raw else {
        log::info!("zone text {lsb} is not in this install's zone {zone} dialogue");
        return;
    };
    let speaker = if t.name.is_empty() { w.entities.get(&t.actor).map(|e| e.name.clone()).unwrap_or_default() } else { t.name.clone() };
    let mut num = [0i32; 8];
    num[..4].copy_from_slice(&t.num);
    let ctx = dialog::Ctx { player: w.name.clone(), speaker: speaker.clone(), num, female: matches!(w.my_race, 2 | 4 | 6 | 7), names: w.names.clone().map(|n| n as Arc<dyn crate::dat::names::NameSource>), ..Default::default() };
    let e = dialog::expand(&raw, &ctx);
    for (k, l) in e.lines.iter().enumerate() {
        let text = decode_sjis(l);
        // FUN_100abbb0 prefixes "%s : "; MesNum bit 15 (FUN_100abc30) has no name.
        if k == 0 && t.mes & 0x8000 == 0 && !speaker.is_empty() {
            w.chat.push(format!("{speaker} : {text}"));
        } else {
            w.chat.push(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB 6439 "Obtained: <item>." sat in a gap of the 2019 zone 230 map (at 6432 there); retail 30261006_0's
    /// dialogue matches LSB's ids.
    #[test]
    fn text_gap() {
        let m = crate::dat::msgshift::MsgShift::bundled();
        assert_eq!(m.map(230, 6439), Some(6439));
    }

    /// The San d'Oria opening 0x1f7 turns cancelling off (0x42) and keeps 0x052 mode 0 from turning it on (0x38).
    #[test]
    fn opening_not_cancellable() {
        use crate::event_vm::{Memory, Scripted, run_scripted};
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(blocks) = crate::dat::event::load(&index, 230) else { return };
        let mut host = Scripted::new(&[]);
        host.sched_ticks = Some(crate::event_vm::sched_timer(crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).unwrap()));
        let start = Start { zone: 230, event: 0x01f7, target: 0x010e_601b, ..Default::default() };
        let mut rt = Runtime::start(blocks, Dialog::load(&index, 230).ok(), start, Memory::default(), &mut host);
        run_scripted(&mut rt, &mut host, 60 * 60 * 10).expect("finished");
        // Loaded with the default (on); the script's first frame turns it off and nothing turns it back on.
        let mut st = State::new(EventStart { zone: 230, event: 0x01f7, mode: 0x83, ..Default::default() });
        st.loaded = true;
        st.cancellable = st.cb0;
        let ops: Vec<u8> = host.log.iter().filter_map(|e| if let Effect::Op { op: op @ (0x2e | 0x42 | 0x38), .. } = e { Some(*op) } else { None }).collect();
        assert_eq!(ops.first(), Some(&0x42));
        assert!(!ops.contains(&0x2e));
        for e in &host.log {
            if let Effect::Op { op, args, .. } = e {
                st.cancel_op(*op, args);
            }
        }
        assert!(!st.cancellable);
        assert!(st.flags_df04 & 0x2000 != 0, "op 0x38 keeps 0x052 mode 0 from enabling cancel");
    }

    #[test]
    fn file_zones() {
        let s = EventStart { zone: 230, zone2: 230, ..Default::default() };
        assert_eq!(file_zone(&s, 230), 230);
        let s = EventStart { zone: 230, zone2: 12, ..Default::default() };
        assert_eq!(file_zone(&s, 230), 1012);
        assert_eq!(dir_of(rad_of(200)), 200);
    }

    /// LSB's "cutscene only" NPCs bound by 0x1f7 (npcs.yaml status 6), per the validation report.
    const CUTSCENE_ONLY: [u32; 23] = [0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0x67, 0x4d, 0x78, 0x79, 0x7a, 0x7c, 0x7d];

    /// 0x1f7 waits for every bound actor, asking for one at a time in block order, then shows the cutscene-only
    /// ones when the script does: the gate knights for the s006 narration, the expedition before "Wait, the
    /// expedition has returned.", Curilla after her first line (spoken off screen), Rahal before his.
    #[test]
    fn opening_actors() {
        use crate::event_vm::{Memory, Scripted, run_scripted};
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(blocks) = crate::dat::event::load(&index, 230) else { return };
        let me = (0x0001_0001, 0x401);
        // Nothing but us at first; the server answers each 0x016 with the actor and its model.
        let mut have: HashSet<u32> = HashSet::from([me.0]);
        let mut asked = Vec::new();
        let mut joined = HashSet::new();
        while let Some((id, idx, missing)) = first_waiting(&blocks, 0x1f7, me, &|id| have.contains(&id), &|_| true, &mut joined) {
            assert!(missing && !asked.contains(&idx), "{id:#x} asked twice");
            asked.push(idx);
            have.insert(id);
        }
        assert_eq!(asked.len(), 35, "{asked:x?}");
        for k in [0xd5u16, 0xd6, 0xd7, 0x68, 0x6f, 0x01] {
            assert!(asked.contains(&k), "{k:#x} not asked");
        }
        // With the model of Curilla still loading the walk stops at her.
        let mut j = HashSet::new();
        let w = first_waiting(&blocks, 0x1f7, me, &|_| true, &|id| id != 0x010e_60d5, &mut j);
        assert_eq!(w, Some((0x010e_60d5, 0xd5, false)));

        let mut host = Scripted::new(&[]);
        host.sched_ticks = Some(crate::event_vm::sched_timer(crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).unwrap()));
        let start = Start { zone: 230, event: 0x01f7, target: me.0, ..Default::default() };
        let mut rt = Runtime::start(blocks, Dialog::load(&index, 230).ok(), start, Memory::default(), &mut host);
        run_scripted(&mut rt, &mut host, 60 * 60 * 20).expect("finished");
        let mut hidden: HashSet<u32> = CUTSCENE_ONLY.iter().map(|i| 0x010e_6000 | i).collect();
        let mut seen: HashMap<u32, HashSet<u32>> = HashMap::new();
        for e in &host.log {
            match e {
                Effect::Op { op: 0x22, actor, args, .. } if *actor != PLAYER => {
                    if let Some(Arg::Int(v)) = args.first() {
                        if *v != 0 { hidden.insert(*actor) } else { hidden.remove(actor) };
                    }
                }
                Effect::Op { op: 0x4e, args, .. } => {
                    if let (Some(Arg::Int(v)), Some(Arg::Actor(a))) = (args.first(), args.get(1)) {
                        if *v != 0 { hidden.insert(*a) } else { hidden.remove(a) };
                    }
                }
                Effect::Message { id, .. } => {
                    seen.entry(*id).or_insert_with(|| hidden.clone());
                }
                _ => {}
            }
        }
        let shown = |msg: u32, npc: u32| !seen[&msg].contains(&(0x010e_6000 | npc));
        assert!((0x68..=0x6f).all(|k| shown(7671, k)), "gate knights at the s006 line");
        assert!(!shown(7670, 0x68));
        assert!((0xd7..=0xdc).all(|k| shown(7680, k)) && !shown(7679, 0xd7), "expedition");
        assert!(!shown(7687, 0xd5) && shown(7689, 0xd5), "Curilla");
        assert!(!shown(7695, 0xd6) && shown(7696, 0xd6), "Rahal");
    }

    /// fdo1 (0, 0, 0, 0x80) then fdi1 (0x80, 0x80, 0x80, 0) over 60 ticks: a multiply, halfway scene x 0.5.
    #[test]
    fn fade_is_a_multiply() {
        let mut sc = Screen::default();
        assert_eq!(sc.multiply(), [1.0; 3]);
        let op = |rgba: [u8; 4]| {
            let mut data = vec![0x0f, 3, 0, 0, 60, 0, 60, 0];
            data.extend_from_slice(&rgba);
            SOp { op: 0x0f, wait: 60, ticks: 60, data }
        };
        sc.scheds.push(Sched { file: 30904, tag: "fdo1".into(), actor: 0, target: 0, ops: vec![op([0, 0, 0, 0x80])], cursor: 0, next_at: 0.0, start: 0.0, len: Some(60.0), speed: 1.0 });
        sc.advance(None, None);
        sc.tick = 59.0;
        assert!(sc.multiply()[0] < 1e-6);
        sc.scheds.push(Sched { file: 30904, tag: "fdi1".into(), actor: 0, target: 0, ops: vec![op([0x80, 0x80, 0x80, 0])], cursor: 0, next_at: 59.0, start: 59.0, len: Some(60.0), speed: 1.0 });
        sc.advance(None, None);
        sc.tick = 59.0 + 29.5;
        let m = sc.multiply()[0];
        assert!((m - 0.5 * 256.0 / 255.0).abs() < 1e-3, "{m}");
    }
}
