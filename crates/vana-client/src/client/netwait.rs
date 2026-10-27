//! The lobby's "menu    netwait" (the status line) and "menu    netbar" (the progress) while an operation runs
//! (FUN_1023b550 / FUN_1023bb60). Spec `~/ffxi/lab/research/lobby_netwait_spec.md`.
//!
//! The retail step machine runs one step-function call per lobby frame (30 fps); each call may set a group 8 status
//! text and moves the step's sub-step counter, and the bar is (finished steps' counts + sub) / the op's total. Ours
//! replays those calls from tables, never passing a wait for a view reply the worker has not read yet; the worker's
//! Task waits here until the bar has run, then p = 1 for 10 frames, then both windows close.

use super::Task;
use super::game_ui::GameUi;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The lobby operations (FUN_1023b550 param_4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// The lobby entry: connect, the lists.
    Connect,
    /// Log in with a character.
    Login,
    Delete,
    Create,
    NameCheck,
    Rename,
}

/// One step-function call: the status text it sets, the sub-step after it, and whether it waits for a view reply.
#[derive(Clone, Copy)]
struct Call {
    text: Option<u16>,
    sub: u32,
    reply: bool,
}

const fn c(text: Option<u16>, sub: u32, reply: bool) -> Call {
    Call { text, sub, reply }
}

/// FUN_100dbac0: a step's sub-step count, and its calls (FUN_100ed7e0, FUN_100edfb0, FUN_100ee160, FUN_100ee690 /
/// FUN_100ee900 / FUN_100eeb70, FUN_100ef020, FUN_100ee0a0). Step 1 on a live link jumps from sub 5 to 21; step 3 takes
/// the PlayOnline short path (14 -> 15 -> 22) *(inf: no POL under xiloader)*.
fn step(id: u8, fresh: bool) -> (u32, Vec<Call>) {
    match id {
        1 if fresh => {
            let text = |s: u32| match s {
                0 => Some(0),
                3 => Some(1),
                7 => Some(2),
                9 => Some(3),
                11 => Some(4),
                13 => Some(5),
                15 => Some(6),
                17 => Some(7),
                _ => None,
            };
            (21, (0..21).map(|s| c(text(s), s + 1, matches!(s, 14 | 16 | 18))).collect())
        }
        1 => (21, vec![c(Some(0), 3, false), c(Some(1), 4, false), c(None, 5, false), c(None, 21, false)]),
        2 => (4, vec![c(Some(8), 1, false), c(None, 2, true), c(None, 3, false), c(None, 4, false)]),
        3 => (
            22,
            vec![
                c(Some(0xc4), 1, false),
                c(None, 2, true),
                c(Some(0xc5), 4, false),
                c(None, 5, false),
                c(None, 6, false),
                c(None, 7, false),
                c(None, 8, false),
                c(None, 10, false),
                c(Some(0xc), 11, false),
                c(Some(0xd), 12, false),
                c(Some(0xe), 13, false),
                c(Some(0xf), 14, false),
                c(Some(0x10), 15, false),
                c(Some(0x11), 22, false),
            ],
        ),
        4 | 0x14 | 0x16 => {
            let t = match id {
                4 => 0x16,
                0x14 => 0x17,
                _ => 0x18,
            };
            (10, vec![c(Some(t), 1, false), c(None, 2, false), c(None, 3, false), c(None, 4, true), c(None, 5, false), c(None, 10, false)])
        }
        5 => (2, vec![c(Some(0x1c), 1, false), c(None, 2, true)]),
        0x13 => (4, vec![c(Some(9), 1, false), c(None, 2, false), c(None, 3, false), c(None, 4, false)]),
        _ => (1, Vec::new()),
    }
}

/// A frame of the run: the text and bar after it, a reply wait, and the refusal point of steps 4 / 0x14 / 0x16.
#[derive(Clone, Copy)]
struct Frame {
    text: Option<u16>,
    p: Option<f32>,
    reply: bool,
    refusable: bool,
}

pub struct NetWait {
    frames: Vec<Frame>,
    at: usize,
    text: Option<u16>,
    p: f32,
    replies: Arc<AtomicU32>,
    base: u32,
    passed: u32,
    parked: Option<(Task, bool)>,
    /// Frames left: of the p = 1 hold, or of a refusal's subs 6..9.
    hold: Option<u32>,
    acc: f32,
}

impl NetWait {
    /// FUN_1023b550: the op's steps (every caller passes param_3 = 0); `replies` is the link's reply counter.
    pub fn new(op: Op, replies: Arc<AtomicU32>) -> Self {
        let steps: &[u8] = match op {
            Op::Connect => &[1],
            Op::Login => &[1, 3],
            Op::Delete => &[1, 5, 0x13, 2],
            Op::Create => &[1, 4, 0x13, 2],
            Op::NameCheck => &[1, 0x14],
            Op::Rename => &[1, 0x16, 0x13, 2],
        };
        let built: Vec<(u32, Vec<Call>)> = steps.iter().map(|&s| step(s, op == Op::Connect)).collect();
        let total = built.iter().map(|b| b.0).sum::<u32>().max(1) as f32;
        let mut frames = Vec::new();
        let mut done = 0;
        for (k, (n, calls)) in built.into_iter().enumerate() {
            // Each step start: two pump frames with the old text and bar.
            frames.extend([Frame { text: None, p: None, reply: false, refusable: false }; 2]);
            let refusable = matches!(steps[k], 4 | 0x14 | 0x16);
            for call in calls {
                frames.push(Frame { text: call.text, p: Some((done + call.sub) as f32 / total), reply: call.reply, refusable: refusable && call.reply });
            }
            // The call that returns done (sub = N).
            frames.push(Frame { text: None, p: Some((done + n) as f32 / total), reply: false, refusable: false });
            done += n;
        }
        let base = replies.load(Ordering::Relaxed);
        Self { frames, at: 0, text: None, p: 0.0, replies, base, passed: 0, parked: None, hold: None, acc: 0.0 }
    }

    /// The worker's Task waits here; `refused` for a lobby refusal (the step's subs 6..9, then no hold).
    pub fn park(&mut self, task: Task, refused: bool) {
        self.parked = Some((task, refused));
    }

    /// Per frame: the calls at 30 a second; Some(task) once the windows close.
    pub fn update(&mut self, dt: f32) -> Option<Task> {
        // A broken link closes both windows at once.
        if matches!(&self.parked, Some((t, _)) if super::task_failed(t)) {
            return self.parked.take().map(|p| p.0);
        }
        self.acc += dt.min(0.5);
        while self.acc >= 1.0 / 30.0 {
            self.acc -= 1.0 / 30.0;
            if let Some(t) = self.tick() {
                return Some(t);
            }
        }
        None
    }

    fn tick(&mut self) -> Option<Task> {
        if let Some(n) = self.hold {
            if n == 0 {
                return self.parked.take().map(|p| p.0);
            }
            self.hold = Some(n - 1);
            return None;
        }
        let Some(&f) = self.frames.get(self.at) else {
            // The table is done: p = 1 for 10 frames once the Task is in.
            if self.parked.is_some() {
                self.p = 1.0;
                self.hold = Some(10);
            }
            return None;
        };
        // A refusal point waits for the Task itself, which follows its reply at once.
        if f.refusable && self.parked.is_none() {
            return None;
        }
        if f.reply {
            let read = self.replies.load(Ordering::Relaxed).wrapping_sub(self.base);
            // The reply is not in yet (a finished Task with fewer replies than the table passes anyway).
            if read <= self.passed && self.parked.is_none() {
                return None;
            }
            self.passed += 1;
            if f.refusable && self.parked.as_ref().is_some_and(|p| p.1) {
                self.hold = Some(4);
                self.frames.truncate(self.at);
            }
        }
        if let Some(t) = f.text {
            self.text = Some(t);
        }
        if let Some(p) = f.p {
            self.p = p;
        }
        self.at += 1;
        None
    }
}

/// What the wait shows: its text and the bar's progress (the modern layer's panel).
pub fn view(gu: &GameUi, nw: &NetWait) -> (String, f32) {
    (nw.text.map_or(String::new(), |t| gu.lobby_msg(8, t as usize, "")), nw.p)
}

/// Both windows (FUN_102408a0 / FUN_102409a0), centred as the prompts: the status text in netwait's frame, then the
/// netbar track (lobbywin 81, frameless) with its fill, lobbywin 77 from the track's left edge, 256 p wide, while
/// 0.01 < p < 1.1.
pub fn draw(gu: &mut GameUi, nw: &NetWait, w: f32, h: f32) {
    let (text, _) = view(gu, nw);
    gu.netwait_window(&text, w, h);
    gu.netbar_window(nw.p, w, h);
}
