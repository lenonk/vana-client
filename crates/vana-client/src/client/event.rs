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
    /// Op 0x72: the additive flash, RGBA / 255: from, to, start tick, length.
    flash: ([f32; 4], [f32; 4], f32, f32),
    /// Op 0x0e: the feedback blur, RGBA / 255 and the zoom f: from, to, start, length; and its starts (each restarts
    /// the capture, DAT_1045692c = 0).
    blur: ([f32; 5], [f32; 5], f32, f32),
    blur_starts: u32,
}


impl Default for Screen {
    fn default() -> Self {
        Screen { scheds: Vec::new(), files: HashMap::new(), shot: None, tick: 0.0, fade: ([1.0; 3], [1.0; 3], 0.0, 0.0), cross: None, flash: ([0.0; 4], [0.0; 4], 0.0, 0.0), blur: ([0.5, 0.5, 0.5, 0.0, 1.0], [0.5, 0.5, 0.5, 0.0, 1.0], 0.0, 0.0), blur_starts: 0 }
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

    /// An effect scheduler's screen op (fx/tasks.rs): 0x0f the multiply, 0x72 the flash over dur x speed - 1 ticks,
    /// 0x0e the feedback blur over dur x speed. Each starts from the last one's target (its task is killed first).
    pub fn fx_op(&mut self, code: u8, c: [u8; 4], zoom: f32, ticks: f32) {
        let f = |v: u8| v as f32 / 255.0;
        let now = self.tick;
        match code {
            0x0f => self.fade = (self.fade.1, [c[0], c[1], c[2]].map(|v| v as f32 * 2.0 / 255.0), now, (ticks - 1.0).max(0.0)),
            0x72 => self.flash = (self.flash.1, c.map(f), now, (ticks - 1.0).max(0.0)),
            0x0e => {
                self.blur = (self.blur.1, [f(c[0]), f(c[1]), f(c[2]), f(c[3]), zoom], now, ticks.max(0.0));
                self.blur_starts += 1;
            }
            _ => {}
        }
    }

    fn lerp<const N: usize>(&self, (from, to, start, len): ([f32; N], [f32; N], f32, f32)) -> [f32; N] {
        let t = if len <= 0.0 { 1.0 } else { ((self.tick - start) / len).clamp(0.0, 1.0) };
        std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
    }

    /// FUN_100132b0's first block: the flash quad when its RGB is not 0, alpha doubled (SRCALPHA / ONE).
    pub fn flash(&self) -> Option<[f32; 4]> {
        let c = self.lerp(self.flash);
        (c[..3].iter().any(|v| *v * 255.0 >= 0.5)).then(|| [c[0], c[1], c[2], (2.0 * c[3]).min(1.0)])
    }

    /// FUN_100132b0's second block: last frame drawn over the scene while the alpha is not 0.
    pub fn blur(&self) -> Option<crate::render::screenfx::Blur> {
        let b = self.lerp(self.blur);
        (b[3] * 255.0 >= 0.5).then(|| crate::render::screenfx::Blur { rgb: [b[0], b[1], b[2]], a: (2.0 * b[3]).min(1.0), zoom: b[4], start: self.blur_starts })
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
            self.fade = (self.fade.1, to, at, ticks);
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
    /// msg+4 from a 1f code; 0 (no colour of its own, no indent) without one.
    pub mode: Option<u8>,
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
    /// Op 0xd8's rotation x / z (+0x150 / +0x158).
    pub tilt: [f32; 2],
    pub ratio: f32,
    /// +0x12c bit 1: still turning (FUN_1008b680).
    pub turning: bool,
    /// The direction last seen (+0xe4), for the step motions of a turn.
    dir: f32,
}

/// The event input window: kind 1 / 2 digits, 3..5 letters, others printable ASCII; at most `max` bytes.
pub struct Input {
    pub kind: i32,
    pub max: usize,
    pub text: Vec<u8>,
    pub caret: usize,
}

impl Input {
    fn allows(&self, c: u8) -> bool {
        match self.kind {
            1 | 2 => c.is_ascii_digit(),
            3..=5 => c.is_ascii_alphabetic(),
            _ => (0x20..=0x7e).contains(&c),
        }
    }
}

pub struct State {
    pub start: EventStart,
    pub msg: Option<Msg>,
    pub choice: Option<Choice>,
    /// Op 0xb4 6's auto-pick (DAT_104886f4): 1 the open or next choice takes its cursor's option, 2 every choice.
    pub auto_pick: u8,
    /// Op 0x71's input window ("menu    passinpu").
    pub input: Option<Input>,
    /// Op 0xd4 3..5 option entries (select window +0x13c): option position -> item, extdata (subs 4 / 5), flags (byte
    /// 3: 2 for sub 5, no signature).
    pub option_items: HashMap<u32, (u16, Option<[u8; 24]>, u8)>,
    /// The open choice is op 0xd4 sub 2: its cursor option's item shows (LAB_1013e6a0).
    pub item_preview: bool,
    /// Op 0xd4 sub 1 entries: option position -> teleport point (kind, id).
    pub option_points: HashMap<u32, (u16, u16)>,
    /// The open choice is op 0xd4 sub 0: the map follows its cursor option's point (LAB_101f9510).
    pub map_preview: bool,
    /// Actors hidden by the script (op 0x22 / 0x4e, actor +0x120 bit 0x20000; FUN_1008cdc0); seeded at bind
    /// with the server-hidden ones (FUN_100bb300 -> FUN_1008cda0).
    pub hidden: HashSet<u32>,
    /// Actors the script faded (op 0x6c); made opaque again when the event ends (inferred).
    faded: HashSet<u32>,
    pub idle: HashMap<u32, String>,
    /// World ids of the bound actors (+0x120 bit 7) and how each is drawn.
    pub bound: HashSet<u32>,
    pub poses: HashMap<u32, Pose>,
    /// Event status (+0x174) by actor, set by op 0xac sub 1: the status a bound actor poses with (+0x170).
    pub event_status: HashMap<u32, u8>,
    /// Op 0x7e's chocobo or mount by actor (ChocoboIndex, the chocobo look, the mount id).
    pub event_rides: HashMap<u32, super::ride::Model>,
    /// Op 0xab sub 3 (+0x120 bit 2): the event status of these actors is not written (ops 0x7e, 0xac 1).
    status_lock: HashSet<u32>,
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
            auto_pick: 0,
            input: None,
            option_items: HashMap::new(),
            item_preview: false,
            option_points: HashMap::new(),
            map_preview: false,
            hidden: HashSet::new(),
            faded: HashSet::new(),
            idle: HashMap::new(),
            bound: HashSet::new(),
            poses: HashMap::new(),
            event_status: HashMap::new(),
            event_rides: HashMap::new(),
            status_lock: HashSet::new(),
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

    /// Op 0xb6 (FUN_100b1150) on the event actor's look (0x16: the player's): 0 face (a model NPC's model), 1 race (its
    /// race code), 2..0xa slot sub - 2
    /// (face head body hands legs feet main sub range), 0xb race and slots 0..7, 0xc the fixed model val@2 (type 7),
    /// 0x16 race, face and slots 1..8 of the player. 0xd / 0xe (race and slots with flags) and the scales 0xf / 0x17
    /// are not applied yet.
    fn event_look(&mut self, sub: u8, actor: u32, args: &[Arg]) {
        use crate::dat::npc::NpcLook;
        let id = if matches!(sub, 0x16 | 0x17) { self.w.my_id } else { self.wid(actor) };
        let v = |k: usize| Self::arg_int(args, k).unwrap_or(0) as u16;
        // 0xf / 0x17: the event actor's / player's scale +0x204 = val@2 * 0.01.
        if matches!(sub, 0xf | 0x17) {
            if let Some(a) = self.w.actors.get_mut(&id) {
                a.event_scale = Some(Self::arg_int(args, 0).unwrap_or(100) as f32 * 0.01);
            }
            return;
        }
        let cur = self.w.actors.get(&id).and_then(|a| a.source);
        // A model NPC (0x00E SubKind 0): +0xfc is its model and +0xef its race code, so subs 0 / 1 change those; the
        // other subs build a race-and-gear look as for a PC.
        let model_npc = match cur {
            Some(super::Source::Npc(NpcLook::Model(_))) => true,
            None => self.w.entities.get(&id).is_some_and(|e| e.sub_kind == 0 && e.model != 0),
            _ => false,
        };
        if model_npc && sub == 0 {
            let source = super::Source::Npc(NpcLook::Model(v(0)));
            if cur.is_none_or(|c| format!("{c:?}") != format!("{source:?}")) {
                self.w.event_looks.push((id, source));
            }
            return;
        }
        if model_npc && sub == 1 {
            if let Some(e) = self.w.entities.get_mut(&id) {
                e.race = (v(0) != 0).then_some(v(0) as u8);
            }
            return;
        }
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
            // A chocobo: race val@4 + 0x20 (tables 32..36), body / head / feet / hands, legs for 0xe only; face and
            // weapons cleared. +0x120 bit 30 = val@2 in 3, 4, 8 is not used here.
            0xd | 0xe => {
                let mut l = crate::dat::character::Look { race: v(1) as u8 + 0x20, ..Default::default() };
                for (slot, k) in [(2, 2), (1, 3), (5, 4), (3, 5)] {
                    set(&mut l, slot, v(k));
                }
                if sub == 0xe {
                    set(&mut l, 4, v(6));
                }
                super::Source::Pc(l)
            }
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
            if let Some((_, swing, _, _)) = super::combat::start_attack(self.w, a, Some(b), None) {
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
            Effect::Message { id, text, prompt, wait, speaker, op, place, mode, sounds } => {
                self.w.sounds.extend(sounds);
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
                self.st.msg = Some(Msg { id, prompt: prompt && wait.is_none(), wait: wait.unwrap_or(0) as f32, open: 0.0, lines, pages: None, page: 0, logged: 0, mode });
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
            // Op 0x71: the window takes the text engine from the chat line (FUN_101c68a0).
            Effect::Input { kind, max } => {
                self.st.input = Some(Input { kind, max, text: Vec::new(), caret: 0 });
                self.w.typing = false;
            }
            Effect::Camera { flag, offset } => {
                self.st.camera_flag = flag;
                if flag {
                    self.w.screen.shot = None;
                    let dir = self.w.net.me.lock().map(|m| m.dir).unwrap_or(0);
                    let f = super::forward_from_dir(dir);
                    match offset {
                        Some(o) => super::camera::face_at(self.w, f.x.atan2(f.z), Vec3::from(o)),
                        None => super::camera::face(self.w, f.x.atan2(f.z)),
                    }
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
                // FUN_100b5210: 4cc 0 or 'xxxx' only loads the file.
                let name = if name == "xxxx" || name.trim_matches('\0').is_empty() { String::new() } else { name };
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
                Packet::Passwords { head, text } => {
                    let mut body = Vec::with_capacity(24);
                    body.extend_from_slice(&head.target.to_le_bytes());
                    body.extend_from_slice(&index.to_le_bytes());
                    body.extend_from_slice(&[0, 0]);
                    body.extend_from_slice(&text);
                    self.send(NetCommand::Packet { id: 0x060, body });
                }
                Packet::EventPos { head, x, y, z, dir, option } => {
                    self.send(NetCommand::EventPos { pos: [x, y, z], dir: dir_of(dir), target: head.target, index, option, zone: head.zone, event: head.event })
                }
                Packet::Raw { id, body } => self.send(NetCommand::Packet { id, body }),
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
            // FUN_100b6a60 0..3: the actor's turn (+0xdc) / look (+0xe0) percent, read while an event runs.
            Effect::Op { op: 0x59, sub: Some(s @ 0..=3), actor, args } => {
                let id = self.wid(actor);
                if let (Some(a), Some(x)) = (self.w.actors.get_mut(&id), Self::arg_int(&args, 0)) {
                    if s < 2 {
                        a.turn_pct = x;
                    } else {
                        a.look_pct = x;
                    }
                }
            }
            // FUN_100bb180 sub 1: the event actor's event status +0x174 = val@2, which a bound actor poses with (5 / 0x55
            // ride on its own ChocoboIndex / mount id, ride.rs).
            Effect::Op { op: 0xac, sub: Some(1), actor, args } => {
                let id = self.wid(actor);
                if let Some(s) = Self::arg_int(&args, 0).filter(|_| !self.st.status_lock.contains(&id)) {
                    self.st.event_status.insert(id, s as u8);
                }
            }
            // FUN_100bb180 subs 2 / 3: the event actor's entity +0x138 bit 31 (its schedulers mute while its model is
            // hidden); sub 3 clears it and asks for the entity again (c2s 0x016, FUN_100aec70).
            Effect::Op { op: 0xac, sub: Some(s @ (2 | 3)), actor, .. } => {
                let id = self.wid(actor);
                if let Some(e) = self.w.entities.get_mut(&id) {
                    e.ev_reserved = s == 2;
                    if s == 3 {
                        let _ = self.w.net.commands.send(NetCommand::CharReq(e.index));
                    }
                }
            }
            // FUN_100bac90 sub 3 locks the event actor's event status; sub 4 (after its wait) unlocks it.
            Effect::Op { op: 0xab, sub: Some(s @ (3 | 4)), actor, .. } => {
                let id = self.wid(actor);
                if s == 3 {
                    self.st.status_lock.insert(id);
                } else {
                    self.st.status_lock.remove(&id);
                }
            }
            // FUN_100b8a80 (op 0x7c): actor@2's mouth on / off (+0x128 bit 17); silent "..." lines are wrapped in it.
            Effect::Op { op: 0x7c, args, .. } => {
                if let (Some(on), Some(id)) = (Self::arg_int(&args, 0), self.arg_actor(&args, 1)) {
                    if let Some(e) = self.w.entities.get_mut(&id) {
                        e.mouth_off = on == 0;
                    }
                    if on == 0
                        && let Some(a) = self.w.actors.get_mut(&id)
                    {
                        a.mouth = None;
                    }
                }
            }
            // FUN_100b8fb0 (op 0x81): actor@2's blinks on / off (model +0x8b4), set before eyes-closed motions.
            Effect::Op { op: 0x81, args, .. } => {
                if let (Some(on), Some(id)) = (Self::arg_int(&args, 0), self.arg_actor(&args, 1))
                    && let Some(a) = self.w.actors.get_mut(&id)
                {
                    a.blink_off = on == 0;
                }
            }
            // FUN_100bac90 sub 0x11: DAT_1035af3c = val@2; anything but -1 rebuilds the models after the event.
            Effect::Op { op: 0xab, sub: Some(0x11), args, .. } => {
                if Self::arg_int(&args, 0).is_some_and(|v| v != -1) {
                    self.w.rebuild_models = true;
                }
            }
            // FUN_100bac90 subs 1 / 2: the event actor's server status kept (+0x120 bit 1); 5 / 6 its status changes play
            // no transition (+0x120 bit 3); 8 / 7 its footsteps also while idle (+0x128 bit 1).
            Effect::Op { op: 0xab, sub: Some(s @ (1 | 2 | 5 | 6 | 7 | 8)), actor, .. } => {
                let id = self.wid(actor);
                if let Some(e) = self.w.entities.get_mut(&id) {
                    match s {
                        1 | 2 => e.status_held = s == 1,
                        5 | 6 => e.quiet_status = s == 5,
                        _ => e.steps_always = s == 8,
                    }
                }
            }
            // Subs 0x19..0x1c: the area id stops following the floor (+0x13c bit 19), odd sets: 0x1b / 0x1c actor@2,
            // 0x19 / 0x1a the event actor.
            Effect::Op { op: 0xab, sub: Some(s @ (0x19..=0x1c)), actor, args } => {
                let id = if s >= 0x1b { self.arg_actor(&args, 0) } else { Some(self.wid(actor)) };
                if let Some(id) = id {
                    let floor = self.w.entities.get(&id).and_then(|e| self.w.ground.as_ref()?.area(e.pos));
                    let area = (s & 1 != 0).then(|| super::actor_area(self.w.entities.get(&id), floor)).flatten();
                    if let Some(e) = self.w.entities.get_mut(&id) {
                        e.area_frozen = area;
                    }
                }
            }
            // FUN_100bb620 subs 7 / 8: actor@2's area override (+0x20c) = the 4cc u32@6 ('evc1'), 8 none.
            Effect::Op { op: 0xae, sub: Some(s @ (7 | 8)), args, .. } => {
                if let Some(id) = self.arg_actor(&args, 0)
                    && let Some(e) = self.w.entities.get_mut(&id)
                {
                    e.area_override = if s == 7 { Self::arg_int(&args, 1).unwrap_or(0) as u32 } else { 0 };
                }
            }
            // Subs 5 / 6: actor@6's motion clock never runs ahead of actor@2's (model +0x798, FUN_100cb400); 6 ends
            // actor@2's.
            Effect::Op { op: 0xae, sub: Some(s @ (5 | 6)), args, .. } => {
                let (leader, follower) = if s == 5 { (self.arg_actor(&args, 0), self.arg_actor(&args, 1)) } else { (None, self.arg_actor(&args, 0)) };
                if let Some(a) = follower.and_then(|f| self.w.actors.get_mut(&f)) {
                    a.motion_link = leader;
                }
            }
            // FUN_100b34c0 (op 0x20): DAT_10482f18 = u8@1, control locked or free; FUN_100b5620 (op 0x30):
            // DAT_10488100 = 0, the lock outlasts the event (chained events).
            Effect::Op { op: 0x20, args, .. } => {
                let target = self.st.start.target;
                self.w.control_lock = (Self::arg_int(&args, 0).unwrap_or(0) != 0).then(|| (Instant::now(), target));
            }
            Effect::Op { op: 0x30, .. } => self.w.lock_carry = true,
            // FUN_100b74e0 (op 0x61): the event actor's footsteps on / off (+0x128 bit 0 = u8@1 & 1).
            Effect::Op { op: 0x61, args, actor, .. } => {
                let id = self.wid(actor);
                if let (Some(v), Some(e)) = (Self::arg_int(&args, 0), self.w.entities.get_mut(&id)) {
                    e.steps_off = v & 1 == 0;
                }
            }
            // FUN_100bac90 subs 0xd / 0xe: the event actor's generators off / on (+0x130 bit 1, as scheduler op 0x9a),
            // acting at its next model load.
            Effect::Op { op: 0xab, sub: Some(s @ (0x0d | 0x0e)), actor, .. } => {
                let id = self.wid(actor);
                if let Some(e) = self.w.entities.get_mut(&id) {
                    e.gens_off = s == 0x0d;
                }
            }
            // FUN_100b1150 subs 0x12..0x15: TargetOffFlag (+0x128 bit 4), on the event actor (0x13 / 0x12) or actor@2
            // (0x15 / 0x14).
            Effect::Op { op: 0xb6, sub: Some(s @ (0x12..=0x15)), actor, args } => {
                let id = if s >= 0x14 { self.arg_actor(&args, 0) } else { Some(self.wid(actor)) };
                if let Some(e) = id.and_then(|id| self.w.entities.get_mut(&id)) {
                    e.target_off = s & 1 != 0;
                }
            }
            // FUN_100bac90 subs 0x0b / 0x0c: the event actor passable / solid to the player's bump (entity +0x120 bit 6,
            // client/bump.rs).
            Effect::Op { op: 0xab, sub: Some(s @ (0x0b | 0x0c)), actor, .. } => {
                let id = self.wid(actor);
                if let Some(e) = self.w.entities.get_mut(&id) {
                    e.no_bump = s == 0x0b;
                }
            }
            // FUN_100bb620 subs 1 / 2: actor@2's name colour index (+0x1d8) = val@6, or 0x17 (none) for 2.
            Effect::Op { op: 0xae, sub: Some(s @ (1 | 2)), args, .. } => {
                if let Some(id) = self.arg_actor(&args, 0)
                    && let Some(e) = self.w.entities.get_mut(&id)
                {
                    e.name_ncol = if s == 1 { Self::arg_int(&args, 1).unwrap_or(0x17) as u16 } else { 0x17 };
                }
            }
            // FUN_100bb620 subs 3 / 4: actor@2's face motion (entity +0x168) from DAT_1035d000 {mou4, kut4} by val@6 (above
            // 1 left alone); 4 always mou4.
            Effect::Op { op: 0xae, sub: Some(s @ (3 | 4)), args, .. } => {
                if let Some(id) = self.arg_actor(&args, 0) {
                    let v = if s == 4 { 0 } else { Self::arg_int(&args, 1).unwrap_or(-1) };
                    if let (Some(n), Some(a)) = (usize::try_from(v).ok().and_then(|v| ["mou4", "kut4"].get(v)), self.w.actors.get_mut(&id)) {
                        a.mouth_name = n;
                    }
                }
            }
            // FUN_100b9120: the moogle menu as 0x02E opens it (FUN_101d6110(0)).
            Effect::Op { op: 0x85, .. } => super::moogle::open(self.w, false),
            // FUN_100b8b80 subs 0 / 5 / 8: actor@2's +0x174 = 0 (8 also its mount id).
            Effect::Op { op: 0x7e, sub: Some(s @ (0 | 5 | 8)), args, .. } => {
                if let Some(id) = self.arg_actor(&args, 0).filter(|id| !self.st.status_lock.contains(id)) {
                    self.st.event_status.remove(&id);
                    if s == 8 {
                        self.st.event_rides.remove(&id);
                    }
                }
            }
            // Subs 1 / 3 / 6 / 7: actor@2 rides (+0x174 5 / 0x55): 1 the plain chocobo (ChocoboIndex 1), 3 / 6 a chocobo
            // look (race val@6 + 0x20, body val@8, head val@10, feet val@12, hands val@14, legs val@16 for 6, else 0),
            // 7 mount val@6 + 1.
            Effect::Op { op: 0x7e, sub: Some(s @ (1 | 3 | 6 | 7)), args, .. } => {
                if let Some(id) = self.arg_actor(&args, 0).filter(|id| !self.st.status_lock.contains(id)) {
                    let v = |i| Self::arg_int(&args, i).unwrap_or(0);
                    let model = match s {
                        1 => Some(super::ride::Model::Chocobo(39_275)),
                        7 => Some(super::ride::Model::Mount((v(1) + 1) as u8)),
                        _ => {
                            let look = crate::dat::character::Look { race: (v(1) + 0x20) as u8, body: v(2) as u16, head: v(3) as u16, feet: v(4) as u16, hands: v(5) as u16, legs: if s == 6 { v(6) as u16 } else { 0 }, ..Default::default() };
                            (0x20..=0x24).contains(&look.race).then_some(super::ride::Model::Custom(look))
                        }
                    };
                    self.st.event_status.insert(id, if s == 7 { 0x55 } else { 5 });
                    match model {
                        Some(m) => self.st.event_rides.insert(id, m),
                        None => self.st.event_rides.remove(&id),
                    };
                }
            }
            // FUN_100b3540: the event actor hidden as op 0x22 1 (+0x120 bit 17).
            Effect::Op { op: 0x90, actor, .. } => {
                let id = self.wid(actor);
                self.st.hidden.insert(id);
            }
            // FUN_100b10f0 sub 0: the event actor's name = str@2 (24 bytes, +0x7c) unless it has an alternate name.
            Effect::Op { op: 0xb5, sub: Some(0), actor, args, .. } => {
                let id = self.wid(actor);
                if let (Some(Arg::Name(n)), Some(e)) = (args.first(), self.w.entities.get_mut(&id)) {
                    e.name = n.chars().take(20).collect();
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
            // FUN_100b6950 / 6a00 / 69b0 / 6980 / 6a30: the event's door state (+0x174) for the bound actor.
            Effect::Op { op: op @ (0x4c | 0x4d | 0x4f | 0x8e | 0x8f), actor, args, .. } => {
                let v = match op {
                    0x4c => Some(8),
                    0x4d => Some(9),
                    0x4f => Self::arg_int(&args, 0).map(|v| (v + 0x12) as u8),
                    0x8e => Some(0x2d),
                    _ => Some(0x2e),
                };
                if let Some(v) = v {
                    let id = self.wid(actor);
                    self.w.doors.event.insert(id, v);
                }
            }
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
                    // FUN_100a0b60: the high byte is Param (flag 1).
                    let _ = super::actmotion::emote(a, n as u32 & 0xff, (n as u32 >> 8) as u16, 0, 6);
                }
            }
            // FUN_100b9d30: the item info window for val@1 (FUN_1013e520); 0 closes it.
            Effect::Op { op: 0xd4, sub: Some(2), .. } => self.st.item_preview = true,
            // FUN_100b2570(6, zone, -1, 1, 1): the preview of this zone, the sub-map under the player, with the arrow.
            Effect::Op { op: 0xd4, sub: Some(0), .. } => {
                self.st.map_preview = true;
                self.w.map_open = Some(self.w.pos_zone);
                self.w.map_cmds.push(super::map::Cmd::Preview { zone: self.w.pos_zone as i16, key: -1, arrow: true, point_y: 0.0 });
            }
            Effect::Op { op: 0xd4, sub: Some(1), args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                self.st.option_points.insert(int(0) as u32, (int(1) as u16, int(2) as u16));
            }
            Effect::Op { op: 0xd4, sub: Some(s @ (3 | 4 | 5)), args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let ext = (s != 3).then(|| {
                    let mut e = [0u8; 24];
                    for k in 0..3 {
                        e[k * 4..k * 4 + 4].copy_from_slice(&int(2 + k).to_le_bytes());
                    }
                    e
                });
                self.st.option_items.insert(int(0) as u32, (int(1) as u16, ext, if s == 5 { 2 } else { 0 }));
            }
            Effect::Op { op: 0x93, args, .. } => {
                let id = Self::arg_int(&args, 0).unwrap_or(0) as u16;
                self.w.info_win = (id != 0).then_some((id, None, 0));
            }
            // FUN_100b7e10 0x40: the concierge's list (link5 mode 1, link6 title val@2).
            Effect::Op { op: 0x71, sub: Some(0x40), args, .. } => {
                let title = Self::arg_int(&args, 0).unwrap_or(0) as u8;
                super::concierge::open(self.w, title);
            }
            // FUN_100b9d90 0x10: arealist in mode 1 (2 with val@4) from tier val@2.
            Effect::Op { op: 0xcc, sub: Some(0x10), args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                self.w.arealist_req = Some((int(0) as u8, int(1) != 0));
            }
            // FUN_100b9d90 0 / 1 / 3: the same with extdata bytes 0..12 = val@4 / @6 / @8 (the rest kept; sub 3 hides
            // the signature); 2: a linkshell, [6] = r | g << 4, [7] = b, [8] = status, [9..24] the name's 15 bytes.
            Effect::Op { op: 0xcc, sub: Some(s @ 0..=3), args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let id = int(0) as u16;
                if id == 0 {
                    self.w.info_win = None;
                } else {
                    let e = &mut self.w.event_ext;
                    if s == 2 {
                        *e = [0; 24];
                        e[6] = ((int(1) & 0xf) | (int(2) & 0xf) << 4) as u8;
                        e[7] = (int(3) & 0xf) as u8;
                        e[8] = int(4) as u8;
                        let name: Vec<u8> = (5..9).flat_map(|k| int(k).to_le_bytes()).collect();
                        e[9..24].copy_from_slice(&name[..15]);
                    } else {
                        for k in 0..3 {
                            e[k * 4..k * 4 + 4].copy_from_slice(&int(1 + k).to_le_bytes());
                        }
                    }
                    let ext = *e;
                    self.w.info_win = Some((id, Some(ext), if s == 3 { 2 } else { 0 }));
                }
            }
            // FUN_100b08e0(1, zone, map, flag, 0): 0x89 (key -1), 0x8d (key val@3) and 0xc8 (the arrow with val@5) open the
            // map; FUN_100b0dd0 closes it.
            Effect::Op { op: op @ (0x89 | 0x8d | 0xc8), args, .. } => {
                let zone = Self::arg_int(&args, 0).unwrap_or(0) as i16;
                let key = if op == 0x89 { -1 } else { Self::arg_int(&args, 1).unwrap_or(-1) as i16 };
                let show_player = op == 0xc8 && Self::arg_int(&args, 2).unwrap_or(0) != 0;
                self.w.map_open = Some(zone as u16);
                self.w.map_cmds.push(super::map::Cmd::Open { zone, key, show_player });
            }
            Effect::Op { op: 0x8a, .. } => {
                self.w.map_open = None;
                self.w.map_cmds.push(super::map::Cmd::Close);
            }
            // FUN_100b0a00 7 / 0xa: the cast bar in its event mode (4: the time left, no gauge) / the event timer, for
            // val@2 ticks, unless it is up; 8 / 0xb close them.
            Effect::Op { op: 0xb4, sub: Some(7), args, .. } => {
                if self.w.cast_bar.is_none() {
                    let n = Self::arg_int(&args, 0).unwrap_or(0).max(0) as f32;
                    (self.w.cast_bar, self.w.cast_event) = (Some((n, n)), true);
                }
            }
            Effect::Op { op: 0xb4, sub: Some(0xa), args, .. } => {
                if self.w.event_timer.is_none() {
                    let n = Self::arg_int(&args, 0).unwrap_or(0).max(0) as f32;
                    self.w.event_timer = Some((n, n));
                }
            }
            // FUN_100b0a00 6: the auto-pick on (0), off (1) or on for good (2); the open choice follows it.
            Effect::Op { op: 0xb4, sub: Some(6), args, .. } => match Self::arg_int(&args, 0) {
                Some(0) => self.st.auto_pick = 1,
                Some(1) => self.st.auto_pick = 0,
                Some(2) => self.st.auto_pick = 2,
                _ => {}
            },
            Effect::Op { op: 0xb4, sub: Some(8), .. } => self.w.cast_bar = None,
            Effect::Op { op: 0xb4, sub: Some(0xb), .. } => self.w.event_timer = None,
            // FUN_100b0a00 0xd / 0xe: hold the voyage (V identity, t still) / let it go on.
            Effect::Op { op: 0xb4, sub: Some(s @ (0xd | 0xe)), .. } => self.w.voyage.hold = s == 0xd,
            // FUN_100b0a00 0x14 -> FUN_101fb070: the map view to x, y, zoom factor, speed (each x 0.001) and flags.
            Effect::Op { op: 0xb4, sub: Some(0x14), args, .. } => {
                let f = |i| Self::arg_int(&args, i).unwrap_or(0) as f32 * 0.001;
                let flags = Self::arg_int(&args, 4).unwrap_or(0);
                self.w.map_cmds.push(super::map::Cmd::View { x: f(0), y: f(1), zf: f(2), speed: f(3), flags });
            }
            // FUN_100b7600 -> FUN_101efdf0 / FUN_101efe80 / FUN_101efed0: marker slot at x, z with a name ('_' = space).
            Effect::Op { op: 0x8b, args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let name = match args.get(4) {
                    Some(Arg::Name(n)) => n.replace('_', " "),
                    _ => String::new(),
                };
                let m = super::map::Cmd::Mark { zone: int(0) as i16, key: -1, slot: int(1) as usize, x: int(2) as f32 * 0.001, z: int(3) as f32 * 0.001, name };
                self.w.map_cmds.push(m);
            }
            // FUN_100b9600: as 0x8b with the sub-key val@3.
            Effect::Op { op: 0xb8, args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let name = match args.get(5) {
                    Some(Arg::Name(n)) => n.replace('_', " "),
                    _ => String::new(),
                };
                let m = super::map::Cmd::Mark { zone: int(0) as i16, key: int(1) as i16, slot: int(2) as usize, x: int(3) as f32 * 0.001, z: int(4) as f32 * 0.001, name };
                self.w.map_cmds.push(m);
            }
            // FUN_100b9780 / FUN_100b9820: set 1's slot emptied (zone, [key,] slot); sub 0 through the file.
            Effect::Op { op: op @ (0xa8 | 0xb9), sub: Some(s @ (0 | 1)), args, .. } => {
                let int = |i| Self::arg_int(&args, i).unwrap_or(0);
                let (key, slot) = if op == 0xa8 { (-1, int(1)) } else { (int(1) as i16, int(2)) };
                self.w.map_cmds.push(super::map::Cmd::Unmark { zone: int(0) as i16, key, slot: slot as usize, file: s == 0 });
            }
            Effect::Fault { actor, pc, op } => log::warn!("event: op {op:#x} at {pc:#x} on {actor:#x} cannot run"),
            Effect::StopTurn { actor } => {
                let id = self.wid(actor);
                if let Some(a) = self.w.actors.get_mut(&id)
                    && a.play.as_ref().is_some_and(|p| p.name == "@tl0" || p.name == "@tr0")
                {
                    a.play = None;
                }
            }
            _ => {}
        }
    }

    fn busy(&mut self, b: &Busy) -> bool {
        // Op 0xb4 sub 0x15: the map window an event opened is still up.
        if let Busy::Op { op: 0xb4, sub: Some(0x15), .. } = b {
            return self.w.map_open.is_some() || self.w.map.is_some();
        }
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
            // FUN_100bac90 sub 4: while the event status is 0 and the actor still rides or dismounts (+0x1bc).
            Busy::Op { op: 0xab, sub: Some(4), actor } => {
                let id = self.wid(*actor);
                self.st.bound.contains(&id) && self.st.event_status.get(&id).copied().unwrap_or(0) == 0 && self.w.rides.contains_key(&id)
            }
            // FUN_100b8b80 sub 2: until actor@2's chocobo / mount has its model (only while it is getting one).
            Busy::Op { op: 0x7e, sub: Some(2), actor } => {
                let id = self.wid(*actor);
                self.w.rides.contains_key(&id) && !self.w.actors.get(&(super::ride::ANIMAL | id)).is_some_and(|a| a.character.is_some())
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

    /// FUN_100b2550(6) -> FUN_101f9a20: no map image still loading, and the key item table in (FUN_10087650).
    fn map_ready(&self) -> bool {
        self.w.map.as_ref().is_none_or(|m| m.image_ready()) && !self.w.key_items.is_empty()
    }

    /// FUN_10089c70 and +0x120 bit 9: the actor exists (all bound ones do once the event starts).
    fn ready(&self, actor: u32) -> bool {
        let id = self.wid(actor);
        id == self.w.my_id || self.w.entities.contains_key(&id)
    }

    fn name(&self, actor: u32) -> String {
        let id = self.wid(actor);
        if id == self.w.my_id {
            return super::monstrosity::my_name(self.w).to_string();
        }
        self.w.entities.get(&id).map(|e| super::monstrosity::name(e).to_string()).filter(|n| !n.is_empty()).or_else(|| self.w.npc_names.get(&id).cloned()).unwrap_or_default()
    }

    fn player_word(&self) -> u32 {
        self.w.stats.as_ref().map_or(0, |s| s.main.0 as u32 | (s.main.1 as u32) << 8)
    }

    fn race(&self, actor: u32) -> u8 {
        let id = self.wid(actor);
        if id == self.w.my_id {
            return self.w.my_race;
        }
        self.w.actors.get(&id).and_then(|a| a.character.as_ref()?.pc).map_or(0, |pc| pc.race)
    }

    fn server_id(&self, actor: u32) -> u32 {
        self.wid(actor)
    }

    /// Hume F, Elvaan F, Tarutaru F, Mithra.
    fn female(&self) -> bool {
        matches!(self.w.my_race, 2 | 4 | 6 | 7)
    }

    fn text_ctx(&self) -> dialog::Ctx {
        text_ctx(self.w)
    }

    fn concierge_pick(&mut self) -> Option<[u32; 3]> {
        self.w.concierge_ui.is_none().then_some(self.w.concierge_pick)
    }

    fn area_pick(&mut self) -> Option<i32> {
        (!super::arealist::picking(self.w)).then_some(self.w.area_pick)
    }

    fn game_config(&self, id: u16) -> u32 {
        self.w.game.get(id)
    }

    fn bar_left(&self, event_timer: bool) -> i32 {
        let bar = if event_timer { self.w.event_timer } else { self.w.cast_bar };
        bar.map_or(0, |(_, left)| left as i32)
    }

    fn place(&self, actor: u32) -> ([f32; 3], f32) {
        let id = self.wid(actor);
        if id == self.w.my_id {
            let dir = self.w.net.me.lock().map(|m| m.dir).unwrap_or(0);
            return (self.w.pos.to_array(), rad_of(dir));
        }
        self.w.entities.get(&id).map_or(([0.0; 3], 0.0), |e| (e.pos.to_array(), rad_of(e.dir)))
    }

    fn index(&self, actor: u32) -> Option<u16> {
        let id = self.wid(actor);
        if id == self.w.my_id {
            return Some(self.w.my_index);
        }
        self.w.entities.get(&id).map(|e| e.index)
    }

    fn in_region(&self, tag: u32, p: [f32; 3]) -> Option<bool> {
        self.w.sub.regions.iter().find(|r| r.tag() == tag).map(|r| r.contains(p))
    }

    fn weather_day(&mut self, zone: u32, day: u32) -> Option<[u8; 3]> {
        weather_day(self.w.index.as_ref()?, zone, day)
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
    super::inspect::drop(w);
    super::treasure::close(w);
    w.chatctrl = None;
    // Registry state byte 1 (the party and status windows): closed by the 0x800 broadcast, the menu with them.
    (w.partymenu, w.status) = (None, None);
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
    // Event start (FUN_1015f3e0(0x800)) closes the battle set too; it does not come back at the event's end.
    (w.action_menu, w.gauge, w.held_menu) = (None, None, None);
    w.win = None;
    // Menu broadcast 0x800 (event start) closes the item info window, and arealist unless it is the event's picker.
    w.info_win = None;
    if !super::arealist::picking(w) {
        w.arealist = None;
    }
    w.confirm = None;
    w.typing = false;
}

pub(super) fn zone_ready(w: &World) -> bool {
    w.in_scene() && w.renderer.is_some() && !w.unsettled && w.zoning_since.is_none()
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
        st.poses.insert(id, Pose { pos: Vec3::from(b.pos), yaw: b.dir, tilt: b.tilt, ratio: 0.0, turning: false, dir: b.dir });
    }
    // FUN_100ad000 (load finished): DAT_104760f8 = 1, DAT_10473068 = DAT_10475cb0.
    st.loaded = true;
    st.cancellable = st.cb0;
    // FUN_10094190 / FUN_100aed00: control locked, DAT_10488100 = 1 (released at the end).
    (w.control_lock, w.lock_carry) = (Some((Instant::now(), st.start.target)), false);
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
                    w.control_lock = None;
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
    // FUN_1014e3e0: a choice under the auto-pick (a timer ran out) takes its cursor's option, with no sound.
    if live.st.auto_pick != 0
        && live.rt.choice_open()
        && let Some(c) = live.st.choice.take()
    {
        live.rt.choose(c.options.get(c.cursor).map(|o| o.0));
        choice_done(w, &mut live);
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
        let pose = live.st.poses.entry(id).or_insert(Pose { pos: p, yaw: b.dir, tilt: b.tilt, ratio: 0.0, turning: false, dir: b.dir });
        let step = p - pose.pos;
        pose.pos = p;
        pose.tilt = b.tilt;
        let flat = Vec3::new(step.x, 0.0, step.z).length();
        let moving = flat >= 0.01 * ticks.max(1e-3);
        let want = if moving { (-step.z).atan2(step.x) } else { b.dir };
        if snap {
            pose.yaw = b.dir;
        } else {
            let rate = w.actors.get(&id).map_or(0.125, |a| super::turn_rate(a, true));
            pose.yaw += angle_to(pose.yaw, want) * (1.0 - (1.0 - rate).powf(ticks));
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
            let base = crate::motion::choose(&a.motion_names.unwrap_or(ch.motions), 0, ratio).0.to_string();
            if super::actmotion::start(a, &ch, name, &base) {
                super::actmotion::fx_too(&mut w.effects, id, name);
            }
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
    // Menu broadcast 0x1000 (event end): what 0x93 / 0xcc opened closes.
    w.info_win = None;
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
    // FUN_100ae3f0 on the bound entities: the mouth (+0x128 bit 17), footsteps (bit 0) and the blinks (model +0x8b4)
    // back on, the area free again (+0x13c bit 19, +0x20c).
    for id in &live.st.bound {
        if let Some(e) = w.entities.get_mut(id) {
            (e.mouth_off, e.steps_off, e.area_frozen, e.area_override) = (false, false, None, 0);
        }
        if let Some(a) = w.actors.get_mut(id) {
            a.blink_off = false;
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
    // FUN_100ae3f0: the lock goes unless op 0x30 carried it.
    if !w.lock_carry {
        w.control_lock = None;
    }
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

/// FUN_10095900 with DAT_1035af3c set and no event running: every entity's model but ours goes (FUN_10092900: its
/// riding animal with it) and loads again, its load fade from 0. Furniture and auras are not entities and stay.
pub fn rebuild_models(w: &mut World) {
    if !w.rebuild_models || active(w) || w.event_pending.is_some() {
        return;
    }
    w.rebuild_models = false;
    let ids: Vec<u32> = w.actors.keys().copied().filter(|&id| id != w.my_id && w.entities.contains_key(&id)).collect();
    log::info!("event: rebuilding {} models (op 0xab 0x11)", ids.len());
    for id in ids {
        let src = w.actors.get(&id).and_then(|a| a.source);
        for gone in [id, super::ride::ANIMAL | id] {
            if w.actors.remove(&gone).is_some() {
                w.effects.forget_actor(gone);
            }
        }
        if let Some(src) = src {
            w.event_looks.push((id, src));
        }
    }
}

/// s2c 0x052 (100a06f0).
pub fn release(w: &mut World, mode: u8, event: u16) {
    log::info!("event release mode {mode} event {event:#x}");
    match mode {
        // 0x100a20d5: fishing's release only clears the /fish latch.
        4 => w.fish_pending = false,
        // 100a070a: release, and cancel allowed unless (DAT_1046df04 | event Mode) & 0x2000.
        0 => {
            w.control_lock = None;
            if let Some(l) = &mut w.event {
                l.st.released = true;
                if l.st.flags() & 0x2000 == 0 {
                    l.st.cancellable = true;
                }
            }
        }
        _ => match &mut w.event {
            Some(l) => l.rt.release(mode, event),
            // The release after an event's end frees a lock op 0x30 carried *(inf)*.
            None => w.control_lock = None,
        },
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

/// Where the running event draws us (the VM bodies are a draw override; our own position stays).
pub fn own_pose(w: &World) -> Option<Vec3> {
    w.event.as_ref()?.st.poses.get(&w.my_id).map(|p| p.pos)
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

/// The running event binds this actor (+0x120 bit 7).
pub fn bound(w: &World, id: u32) -> bool {
    w.event.as_ref().is_some_and(|l| l.st.bound.contains(&id))
}

/// A bound actor's drawn transform while the event runs.
pub fn pose(w: &World, id: u32) -> Option<Pose> {
    w.event.as_ref()?.st.poses.get(&id).copied()
}

/// A bound actor's event status (+0x174, op 0xac sub 1); 0 when none was set.
/// Op 0x7e's animal for a bound actor.
pub fn ride_model(w: &World, id: u32) -> Option<super::ride::Model> {
    w.event.as_ref()?.st.event_rides.get(&id).copied()
}

/// Bound actors whose event status rides (5 / 0x55).
pub fn riding(w: &World) -> Vec<u32> {
    w.event.as_ref().map_or_else(Vec::new, |l| l.st.event_status.iter().filter(|(_, s)| matches!(s, 5 | 0x55)).map(|(id, _)| *id).collect())
}

pub fn event_status(w: &World, id: u32) -> u8 {
    w.event.as_ref().and_then(|l| l.st.event_status.get(&id).copied()).unwrap_or(0)
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
        // FUN_1008e4d0: /hidetrust (config 0x99 bit 5) takes the model of every trust outside our own party (its plate
        // and targeting go with it; the radar dot stays).
        let trust = w.game.get(0x99) & 0x20 != 0 && e.marks.trust && !w.party.party().any(|m| m.index == e.index);
        let hide = match script {
            Some(h) if takes_part(id) => h.contains(id),
            _ => e.flags & 2 != 0 || (!takes_part(id) && (filtered(*id) || trust)),
        };
        // FUN_100cd040: a listed-only NPC the s2c 0x077 list lacks is not drawn (+0xb2 bit 7), events included.
        if hide || e.unlisted {
            out.insert(*id);
        }
    }
    let me = w.my_id;
    let hide_me = match script {
        Some(h) if takes_part(&me) => h.contains(&me),
        _ => !takes_part(&me) && filtered(me),
    };
    // FUN_101d4d90: laying out the Mog House the player is not drawn (+0xb2 bit 4).
    if hide_me || w.layout.is_some() {
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

pub fn input(w: &World) -> Option<&Input> {
    w.event.as_ref()?.st.input.as_ref()
}

/// The input window takes every key and character (the shared text engine, FUN_101b5d10): the kind's characters
/// up to max (over it SE 0004, others dropped silently), caret keys, Backspace / Delete. Enter (SE 0002 unless
/// empty), Escape (SE 0003) and Up / Down (silent) all close it with the text as typed.
pub fn input_events(w: &mut World, events: &[egui::Event]) {
    let Some(l) = w.event.as_mut() else { return };
    let Some(inp) = l.st.input.as_mut() else { return };
    let mut done = None;
    for ev in events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.bytes() {
                    if !inp.allows(c) {
                        continue;
                    }
                    if inp.text.len() >= inp.max {
                        w.sounds.push(crate::audio::Ui::Error as u32);
                        continue;
                    }
                    inp.text.insert(inp.caret, c);
                    inp.caret += 1;
                }
            }
            egui::Event::Key { key, pressed: true, .. } => match key {
                egui::Key::ArrowLeft => inp.caret = inp.caret.saturating_sub(1),
                egui::Key::ArrowRight => inp.caret = (inp.caret + 1).min(inp.text.len()),
                egui::Key::Home => inp.caret = 0,
                egui::Key::End => inp.caret = inp.text.len(),
                egui::Key::Backspace if inp.caret > 0 => {
                    inp.caret -= 1;
                    inp.text.remove(inp.caret);
                }
                egui::Key::Delete if inp.caret < inp.text.len() => {
                    inp.text.remove(inp.caret);
                }
                egui::Key::Enter => done = Some((!inp.text.is_empty()).then_some(crate::audio::Ui::Select)),
                egui::Key::Escape => done = Some(Some(crate::audio::Ui::Cancel)),
                egui::Key::ArrowUp | egui::Key::ArrowDown => done = Some(None),
                _ => {}
            },
            _ => {}
        }
        if done.is_some() {
            break;
        }
    }
    if let Some(se) = done {
        if let Some(se) = se {
            w.sounds.push(se as u32);
        }
        let text = std::mem::take(&mut inp.text);
        l.st.input = None;
        l.rt.input_done(&text);
    }
}

/// FUN_1014e410: the choice's bar or wheel (3 rows): a row moves the selection (the view follows), a page 3 rows
/// keeping the screen row, the thumb the top with the selection shifted by as much. True when the selection moved.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    use super::scrollbar::Msg;
    let Some(c) = w.event.as_mut().and_then(|l| l.st.choice.as_mut()) else { return false };
    let n = c.options.len();
    if n == 0 {
        return false;
    }
    let (was, max) = (c.cursor, n.saturating_sub(3));
    match m {
        Msg::Up => move_cursor(c, was as isize - 1, false),
        Msg::Down => move_cursor(c, was as isize + 1, false),
        Msg::PageUp => move_cursor(c, was as isize - 3, true),
        Msg::PageDown => move_cursor(c, was as isize + 3, true),
        Msg::Set(t) => {
            let t = t.min(max);
            c.cursor = (c.cursor + t).saturating_sub(c.scroll).min(n - 1);
            c.scroll = t;
            c.scroll = c.scroll.min(c.cursor).max(c.cursor.saturating_sub(2));
        }
    }
    c.cursor != was
}

/// FUN_1014e0f0: the cursor to `to`, clamped to the options (no wrap). A step keeps it inside the 3 rows; a page keeps
/// its screen row, the top at most the last 3.
fn move_cursor(c: &mut Choice, to: isize, page: bool) {
    let n = c.options.len();
    if n == 0 {
        return;
    }
    let row = c.cursor.saturating_sub(c.scroll);
    c.cursor = to.clamp(0, n as isize - 1) as usize;
    c.scroll = if page { c.cursor.saturating_sub(row).min(n.saturating_sub(3)) } else { c.scroll.min(c.cursor).max(c.cursor.saturating_sub(2)) };
}

pub fn choice(w: &World) -> Option<&Choice> {
    w.event.as_ref()?.st.choice.as_ref()
}

/// The mouse's cursor move onto option `k` (the view follows, as a step). True when it moved.
pub(super) fn set_choice(w: &mut World, k: usize) -> bool {
    let Some(c) = w.event.as_mut().and_then(|l| l.st.choice.as_mut()).filter(|c| c.cursor != k && k < c.options.len()) else { return false };
    move_cursor(c, k as isize, false);
    true
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

/// Op 0x72's table: files 0x1b78 / 0x1b79 (zones under 100) or 0x1b7c / 0x1b7d give the zone's row, then 0x870 days
/// of 3 bytes a row (FUN_100b84b0).
pub fn weather_day(index: &DatIndex, zone: u32, day: u32) -> Option<[u8; 3]> {
    let (rows, data, z) = if zone < 100 { (0x1b78, 0x1b79, zone) } else { (0x1b7c, 0x1b7d, zone - 100) };
    let row = *index.read(rows).ok()?.get(z as usize)? as usize;
    let d = index.read(data).ok()?;
    let at = row * 0x1950 + (day % 0x870) as usize * 3;
    d.get(at..at + 3).map(|b| [b[0], b[1], b[2]])
}

/// The target window stays up in an event only with EventPara2 bit 1 (FUN_10085a00(0, 1)).
pub fn keeps_target(w: &World) -> bool {
    w.event.as_ref().is_none_or(|l| l.st.start.para2 & 1 != 0) && w.event_pending.is_none()
}

/// Keys while an event runs: Enter advances / picks, arrows move the choice cursor. Escape (key function
/// 0x0f) cancels only while DAT_10473068 allows it, with SE "0003"; c2s 0x05B then carries 0x40000000.
/// FUN_100b2940 / FUN_100b2aa0 after an answer: an event map closes (FUN_100b2a60), the item info window too,
/// whoever opened it, and the option entries go.
fn choice_done(w: &mut World, l: &mut Live) {
    // FUN_100ae3a0: closing the choice ends the auto-pick unless it is for good.
    if l.st.auto_pick != 2 {
        l.st.auto_pick = 0;
    }
    if w.map_open.take().is_some() {
        w.map_cmds.push(super::map::Cmd::CloseEvent);
    }
    w.info_win = None;
    l.st.option_items.clear();
    l.st.item_preview = false;
    l.st.option_points.clear();
    l.st.map_preview = false;
}

/// The open op 0xd4 sub 0 choice's cursor option's point (FUN_1014e0a0: none past option 0x1f; 0xffff for an
/// option without an entry).
pub fn option_point(w: &World) -> Option<(u16, u16)> {
    let l = w.event.as_ref()?;
    let c = l.st.choice.as_ref().filter(|_| l.st.map_preview)?;
    let pos = c.options.get(c.cursor)?.0;
    (pos < 0x20).then(|| l.st.option_points.get(&pos).copied().unwrap_or((0xffff, 0xffff)))
}

/// The item of the open op 0xd4 sub 2 choice's cursor option (LAB_1013e6a0 -> FUN_1013e6d0): item, extdata, flags.
pub fn option_item(w: &World) -> Option<(u16, Option<[u8; 24]>, u8)> {
    let l = w.event.as_ref()?;
    let c = l.st.choice.as_ref().filter(|_| l.st.item_preview)?;
    let pos = c.options.get(c.cursor)?.0;
    l.st.option_items.get(&pos).copied().filter(|e| e.0 != 0 && e.0 != 0xffff)
}

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
        // FUN_1014e410: Down / Up one option, Right / Left a page of 3, SE 0001 when the cursor moved; Enter 0002 (the
        // generic handler's).
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowUp | egui::Key::ArrowRight | egui::Key::ArrowLeft => {
                let was = c.cursor;
                let (step, page) = match key {
                    egui::Key::ArrowDown => (1, false),
                    egui::Key::ArrowUp => (-1, false),
                    egui::Key::ArrowRight => (3, true),
                    _ => (-3, true),
                };
                move_cursor(c, was as isize + step, page);
                if c.cursor != was {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            egui::Key::Enter if l.rt.choice_open() => {
                w.sounds.push(crate::audio::Ui::Select as u32);
                let pick = c.options.get(c.cursor).map(|o| o.0);
                l.rt.choose(pick);
                l.st.choice = None;
                choice_done(w, l);
            }
            // FUN_10147c20 case 6: result 0xff with SE 0003 when allowed, else SE 0004 and the choice stays.
            _ if cancel && l.rt.choice_open() => {
                if allowed {
                    l.rt.choose(None);
                    l.st.choice = None;
                    choice_done(w, l);
                    w.sounds.push(crate::audio::Ui::Cancel as u32);
                } else {
                    w.sounds.push(crate::audio::Ui::Error as u32);
                }
            }
            _ => {}
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
                w.sounds.push(crate::audio::Ui::Cursor as u32);
            }
        }
    }
    if enter {
        key(w, egui::Key::Enter);
    }
}

/// FUN_1007aac0: Talk (c2s 0x01A action 0) after the client's checks. Returns whether it was sent.
pub fn talk(w: &mut World, target: u32) -> bool {
    if active(w) || w.control_lock.is_some() || w.zone.is_none() {
        return false;
    }
    // FUN_1007bf30: not while mounted (FUN_101cbe50 / FUN_101cbe60: group 7 0x4b).
    if matches!(w.my_motion.status, 5 | 0x55) {
        super::textcmd::refuse(w, 0x4b);
        return false;
    }
    // LAB_101d6040: the Mog House door is the client's own; it opens the exit menu or the prompt.
    if target == super::myroom::DOOR_ID && w.myroom.is_some() {
        super::open_mogdoor(w);
        return true;
    }
    let Some(e) = w.entities.get(&target) else { return false };
    // UniqueNo high byte: NPCs only; a model on the target (+0x120 bit 9); not hidden by the server.
    // A door's XiDoorActor counts as its model.
    if target >> 24 == 0 || e.index == 0 || !(w.actors.contains_key(&target) || e.door != 0) || e.flags & 6 != 0 {
        return false;
    }
    // Not while invisible (own +0x128 bit 6 = 0x037 flags1 bit 15): system message 0xac.
    if w.my_plate.invisible {
        super::textcmd::system_line(w, 0xac);
        return false;
    }
    // LAB_1007c0ec: system message 9 "Target out of range." (FUN_100a1ab0, file 7031).
    if (e.pos - w.pos).length() > TALK_RANGE {
        super::textcmd::system_line(w, 9);
        return false;
    }
    if matches!(e.motion.status, 2 | 3) {
        return false;
    }
    let index = e.index;
    let _ = w.net.commands.send(NetCommand::Act { target, index, action: 0 });
    w.control_lock = Some((Instant::now(), target));
    log::info!("talk to {target:#x} ({index})");
    true
}

/// The tables a line's codes read (dialog::Texts); the point and Monstrosity names load on first use.
struct EventTexts {
    bt: Option<Arc<crate::dat::battle::BattleText>>,
    index: Option<Arc<crate::dat::DatIndex>>,
    files: [Option<u32>; 5],
    merits: std::sync::OnceLock<Option<Arc<crate::dat::merits::MeritTables>>>,
    jp: std::sync::OnceLock<Option<Arc<crate::dat::merits::JobPointTables>>>,
    ae: std::sync::OnceLock<Option<Arc<crate::dat::merits::AeTables>>>,
    mon: std::sync::OnceLock<Option<crate::dat::names::DMsg>>,
}

impl dialog::Texts for EventTexts {
    fn kind(&self, kind: u8, id: u32) -> Option<Vec<u8>> {
        self.bt.as_ref()?.kind_name(kind, id)
    }

    fn point(&self, sub: u8, id: u16) -> Option<Vec<u8>> {
        use crate::dat::merits::{AeTables, JobPointTables, MeritTables};
        let index = self.index.as_ref()?;
        let t = match sub {
            0xb0 => self.merits.get_or_init(|| Some(Arc::new(MeritTables::load(index, self.files[0])))).as_ref()?.name_of_id(id)?,
            0xb1 => self.jp.get_or_init(|| Some(Arc::new(JobPointTables::load(index, self.files[1])))).as_ref()?.name(id),
            _ => self.ae.get_or_init(|| Some(Arc::new(AeTables::load(index, self.files[3])))).as_ref()?.name(id),
        };
        Some(crate::dat::font::encode_sjis(&t)).filter(|t| !t.is_empty())
    }

    fn monstrosity(&self, row: i16) -> Option<Vec<u8>> {
        let mon = self.mon.get_or_init(|| {
            let (index, f) = (self.index.as_ref()?, self.files[4]?);
            crate::dat::names::DMsg::load(index, f).map_err(|e| log::warn!("Monstrosity names {f}: {e:#}")).ok()
        });
        Some(crate::dat::font::encode_sjis(&mon.as_ref()?.text(usize::try_from(row).ok()?)?))
    }
}

/// What message codes take from the client (FUN_100c1650): the date strings, the roster, the other name tables.
pub fn text_ctx(w: &World) -> dialog::Ctx {
    let dates = w.battle.clone().map(|bt| {
        let group10 = Box::new(move |id: u16| bt.group10_raw(id as usize));
        Arc::new(dialog::Dates { group10, date_key: (w.game.get(0x95) & 3) as usize, h12: w.game.get(0x96) & 1 != 0 })
    });
    let texts = EventTexts {
        bt: w.battle.clone(),
        index: w.index.clone(),
        files: w.point_files,
        merits: w.merit_tables.clone().map(Some).map(std::sync::OnceLock::from).unwrap_or_default(),
        jp: w.jp_tables.clone().map(Some).map(std::sync::OnceLock::from).unwrap_or_default(),
        ae: w.ae_tables.clone().map(Some).map(std::sync::OnceLock::from).unwrap_or_default(),
        mon: Default::default(),
    };
    dialog::Ctx {
        dates,
        party: w.party.indexed().map(|(i, m)| (i as u8, m.id, m.number, m.name.clone())).collect(),
        concierge: super::concierge::names(w),
        me: w.my_id,
        texts: Some(Arc::new(texts)),
        ..Default::default()
    }
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
        Some(m) if m.zones.contains_key(&zone) => m.map_near(zone, lsb),
        _ => Some(lsb),
    };
    let Some((_, d)) = &w.zone_text else { return };
    let raw = id.and_then(|i| d.raw(i));
    let Some(raw) = raw else {
        log::info!("zone text {lsb} is not in this install's zone {zone} dialogue");
        return;
    };
    let named = t.mes & 0x8000 == 0;
    let entity = w.entities.get(&t.actor).filter(|e| e.index == t.index).map(|e| super::monstrosity::name(e).to_string());
    // Bit 15 clear: the speaker must be here with that id and index, else the line is dropped (0x02A naming its
    // speaker excepted, inf).
    if named && entity.is_none() && t.name.is_empty() {
        log::info!("zone text {lsb}: speaker {:#x} ({}) not here", t.actor, t.index);
        return;
    }
    let speaker = if t.name.is_empty() { entity.clone().unwrap_or_default() } else { t.name.clone() };
    // msg+0xc10: FUN_100ad5e0 / FUN_100ad660 get the speaker entity's name, else sName; 0x027 none.
    let msg_name = if t.packet == 0x027 { String::new() } else { entity.clone().unwrap_or_else(|| t.name.clone()) };
    // The "%s : " prefix (FUN_100abbb0; MesNum bit 15 none). 0x027: String1's, or with String2 / Flags 1 the NPC's
    // own name (a PC: String2); bit 15 set: String2 when set and Flags 2.
    let prefix = match t.packet {
        0x027 if named && (!t.name2.is_empty() || t.flags & 1 != 0) => if t.actor >> 24 != 0 { entity.unwrap_or_default() } else { t.name2.clone() },
        0x027 if named => t.strings.first().cloned().unwrap_or_default(),
        0x027 if !t.name2.is_empty() && t.flags & 2 != 0 => t.name2.clone(),
        _ if named => speaker.clone(),
        _ => String::new(),
    };
    let g = w.event.as_ref().map_or(w.event_mem.g, |l| l.rt.mem.g).map(|v| v as i32);
    let ctx = dialog::Ctx {
        player: super::monstrosity::my_name(w).to_string(),
        speaker,
        num: t.num,
        g,
        strings: t.strings.clone(),
        female: matches!(w.my_race, 2 | 4 | 6 | 7),
        names: w.names.clone().map(|n| n as Arc<dyn crate::dat::names::NameSource>),
        msg_name,
        ..text_ctx(w)
    };
    let e = dialog::expand(&raw, &ctx);
    w.sounds.extend(&e.sounds);
    // The log mode: Type below 8 by table 0x1035b944, else 0; a 1f code in the text overrides it.
    const MODES: [u8; 8] = [0x8e, 0xa1, 0x90, 0x91, 0x92, 0xa1, 0x94, 0x95];
    let mode = e.mode.unwrap_or(MODES.get(t.kind as usize).copied().unwrap_or(0));
    let colour = w.battle.as_ref().map_or(super::WHITE, |bt| bt.colour_in(mode, &w.game));
    // The log's wrapper (FUN_10128ce0) indents a flagged mode's continuation lines, and 07 starts one too.
    let indent = w.battle.as_ref().is_some_and(|bt| bt.indents(mode));
    for (k, l) in e.lines.iter().enumerate() {
        let text = decode_sjis(l);
        let text = match k {
            0 if !prefix.is_empty() => format!("{prefix} : {text}"),
            0 => text,
            _ if indent => format!("\u{3000}{text}"),
            _ => text,
        };
        if indent {
            w.chat.push_indented(text, colour);
        } else {
            w.chat.push_colour(text, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Op 0x82's regions are RID records by 4cc: Lower Jeuno's e6t0 / e6t1 hold their own centres.
    #[test]
    fn region_ops() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(keys) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).map_err(anyhow::Error::from).and_then(|d| crate::dat::zone::Keys::from_ffximain(&d)) else { return };
        let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(245)).unwrap()).unwrap();
        for t in [*b"e6t0", *b"e6t1"] {
            let r = scene.regions.iter().find(|r| r.tag() == u32::from_le_bytes(t)).expect("region");
            assert!(r.contains(r.pos));
            assert!(!r.contains([r.pos[0] + 500.0, r.pos[1], r.pos[2]]));
        }
    }

    /// Op 0x72's weather table: every zone of both halves has a row and 0x870 days.
    #[test]
    fn weather_table() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let days: Vec<[u8; 3]> = (0..8).filter_map(|d| weather_day(&index, 230, d)).collect();
        eprintln!("230: {days:?}");
        assert_eq!(days.len(), 8);
        assert!(weather_day(&index, 100, 0x86f).is_some() && weather_day(&index, 11, 0x86f).is_some());
    }

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
    fn fx_screen_ops() {
        let mut sc = Screen::default();
        // 0x72 white 0x80 over 11 ticks: half way at 5, the alpha doubled.
        sc.fx_op(0x72, [0xff, 0xff, 0xff, 0x80], 0.0, 11.0);
        sc.tick = 5.0;
        let f = sc.flash().unwrap();
        assert!((f[0] - 0.5).abs() < 1e-3 && (f[3] - 0.502).abs() < 1e-2, "{f:?}");
        // The "off" op starts from the last target.
        sc.fx_op(0x72, [0, 0, 0, 0], 0.0, 1.0);
        sc.tick = 6.0;
        assert!(sc.flash().is_none());
        // 0x0e on, then off: each start counts.
        sc.fx_op(0x0e, [0x80, 0x80, 0x80, 0x1e], 0.99, 0.0);
        let b = sc.blur().unwrap();
        assert_eq!(b.start, 1);
        assert!((b.zoom - 0.99).abs() < 1e-6 && (b.a - 60.0 / 255.0).abs() < 1e-3, "{b:?}");
        sc.fx_op(0x0e, [0x80, 0x80, 0x80, 0], 1.0, 0.0);
        assert!(sc.blur().is_none());
    }

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
