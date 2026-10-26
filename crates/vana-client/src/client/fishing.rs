//! Fishing, the 2013 mini-game (fishing_spec.md): /fish's checks and request latch, the bite wait, the fight (arrows
//! against the fish's stamina) and its end. The client never decides a catch: Enter sends the stamina left (c2s 0x110
//! mode 3) and LSB answers with the result status.

use super::World;
use super::game_ui::GameUi;
use super::input::{Input, mode};
use super::net::NetCommand;
use crate::fx::sched::Who;

/// NewFishingStart (0x38) .. NewFishingStop (0x3e).
pub fn fishing_status(s: u8) -> bool {
    (0x38..=0x3e).contains(&s)
}

/// s2c 0x115 (LAB_1009fd30), packet offsets 4..0x17.
#[derive(Clone, Copy, Debug)]
pub struct FishParams {
    pub stamina: u16,
    pub delay: u16,
    pub regen: u16,
    pub moves: u16,
    pub damage: u16,
    pub heal: u16,
    pub time: u16,
    pub sense: u8,
    pub special: u32,
}

/// G (DAT_10487ee8) and I (DAT_10487ee4).
struct Game {
    max: i32,
    cur: i32,
    shown: i32,
    delay: i32,
    /// G[4]: the stamina change a tick (regen - 0x80).
    regen: i32,
    /// G[5]: move x 20.
    moves: i32,
    damage: i32,
    heal: i32,
    /// G[8]: ticks left.
    time: i32,
    warned: bool,
    large: bool,
    dir: u8,
    hit_timer: i32,
    special: u32,
    gold: bool,
}

/// Our player's fishing state (entity +0x13c, the DAT_10487e.. globals) while the fishing model is set up.
pub struct Fishing {
    /// +0x13c bits 8..10.
    sub: u8,
    /// DAT_1048315c: Enter or a click during the wait.
    early: bool,
    /// +0x1e8: no early end for 3 s.
    lockout: i32,
    /// DAT_10487ef8: frames since the bite check went.
    frames: u32,
    game: Option<Game>,
    /// DAT_10487ec8: -1 before the first tick, 0 idle, 1 between arrows, 2 an arrow up.
    arrow: i8,
    /// DAT_10487ecc: the next-arrow accumulator, then the arrow's window.
    acc: i32,
    t: i32,
    good: u32,
    bad: u32,
    /// DAT_10487efc: axis 4's edge latch.
    latch: bool,
    /// +0x13c bits 12..17: frames until the arrow effects are cleared again.
    clear: u8,
    rng: u32,
    /// The arrow up was answered (a hit or a wrong key).
    answered: bool,
    /// Retail's frames at 30 a second (u = 2 ticks), and what came in since the last.
    step: f32,
    enter: bool,
    enter_free: bool,
    escape: bool,
    click: bool,
    axis: f32,
}

/// One retail frame at its 30 fps (FUN_10014cf0 = 2.0, FUN_10014d60 = 2).
const U: i32 = 2;

fn name(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

impl Fishing {
    fn rand(&mut self) -> i32 {
        self.rng = self.rng.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.rng >> 16 & 0x7fff) as i32
    }
}

/// c2s 0x110 (FUN_10088380): UniqueNo, para, ActIndex, mode, 0, para2.
fn send(w: &mut World, para: i32, mode: u8, para2: u32) {
    let mut body = w.my_id.to_le_bytes().to_vec();
    body.extend(para.to_le_bytes());
    body.extend(w.my_index.to_le_bytes());
    body.extend([mode, 0]);
    body.extend(para2.to_le_bytes());
    log::info!("fishing: c2s 0x110 mode {mode} para {para} para2 {para2}");
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x110, body });
}

/// The fishing statuses and the request latch keep us in place (FUN_100964f0).
pub fn locked(w: &World) -> bool {
    w.fish_pending || fishing_status(w.my_motion.status) || fishing_status(w.server_status)
}

/// /fish (FUN_1007abf0) after the status gate: "You cannot fish here." (group 7 0x49) without water ahead or in the
/// Cloister of Tides; nothing while the latch is set or already fishing; else c2s 0x01A category 0x0e and the latch.
pub fn command(w: &mut World) {
    let water = w.ground.as_ref().is_some_and(|g| g.water_ahead(w.pos, forward(w)));
    if !water || w.zone == Some(211) {
        super::textcmd::refuse(w, 0x49);
        return;
    }
    if w.fish_pending || fishing_status(w.my_motion.status) {
        return;
    }
    super::textcmd::act(w, (w.my_id, w.my_index), 0x0e, 0);
    w.fish_pending = true;
}

fn forward(w: &World) -> glam::Vec3 {
    glam::Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos())
}

/// s2c 0x115: one fish a game (ignored while one is held); keen sense plays DAT 3705 `main` on us.
pub fn on_fish(w: &mut World, p: FishParams) {
    log::info!("fishing: 0x115 {p:?}");
    let Some(f) = w.fishing.as_mut() else { return };
    if f.game.is_some() {
        return;
    }
    let stamina = p.stamina as i32;
    f.game = Some(Game {
        max: stamina,
        cur: stamina,
        shown: stamina,
        delay: (p.delay as i32).max(1),
        regen: p.regen as i32 - 0x80,
        moves: (p.moves as i32).max(1) * 20,
        damage: p.damage as i32,
        heal: p.heal as i32,
        time: p.time as i32 * 60,
        warned: false,
        large: p.sense & 1 != 0,
        dir: 0,
        hit_timer: 0,
        special: p.special,
        gold: false,
    });
    if p.sense & 2 != 0 {
        let me = Who::Actor(w.my_id);
        w.effects.on_event_scheduler(0xaf0 + 0x389, name(b"main"), me, me);
    }
}

/// A mouse release in the field (key functions 0x8b / 0x8c mode 5).
pub fn click(w: &mut World) {
    if let Some(f) = w.fishing.as_mut() {
        f.click = true;
    }
}

/// Per frame from the key functions (FUN_10089100 for our player): the latch, the fishing model's state, then
/// retail's frames.
pub fn update(w: &mut World, input: &mut Input, dt: f32, window: bool) {
    let status = w.my_motion.status;
    if matches!(status, 1..=4) || fishing_status(status) {
        w.fish_pending = false;
    }
    if !fishing_status(status) {
        if let Some(mut f) = w.fishing.take() {
            clear_arrows(w, &mut f, false);
            w.fish_gauge = None;
            stop_all(w);
        }
        return;
    }
    // FUN_1008f740: the fishing model set up; the bite wait from 0x037, else 900..1500 ticks.
    let mut f = match w.fishing.take() {
        Some(f) => f,
        None => {
            let seed = w.started.elapsed().as_nanos() as u32 ^ w.my_id;
            let mut f = Fishing { sub: 0, early: false, lockout: 180, frames: 0, game: None, arrow: -1, acc: 0, t: 0, good: 0, bad: 0, latch: false, clear: 0, answered: false, rng: seed, step: 0.0, enter: false, enter_free: false, escape: false, click: false, axis: 0.0 };
            if w.hook_wait <= 0 {
                w.hook_wait = f.rand() % 601 + 900;
            }
            f
        }
    };
    // Key function 0x0e (Enter, mode 1) any time, with no window for the wait; 0x0f (Escape) with no window.
    if input.test(0x0e, mode::PRESS) {
        f.enter = true;
        f.enter_free |= !window;
    }
    if !window && input.test(0x0f, mode::PRESS) {
        f.escape = true;
    }
    let a = input.axis(4);
    if a != 0.0 {
        f.axis = a;
    }
    f.step += dt * 30.0;
    while f.step >= 1.0 {
        f.step -= 1.0;
        frame(w, &mut f);
        (f.enter, f.enter_free, f.escape, f.click, f.axis) = (false, false, false, false, 0.0);
    }
    w.fishing = Some(f);
}

fn running(w: &World, s: &[u8; 4]) -> bool {
    w.effects.actor_scheduler_running(w.my_id, name(s))
}

fn stop(w: &mut World, list: &[&[u8; 4]]) {
    for s in list {
        w.effects.stop_actor_scheduler(w.my_id, name(s));
        if let Some(rod) = super::fishrod::rod_of(w, w.my_id) {
            w.effects.stop_actor_scheduler(rod, name(s));
        }
    }
}

/// Starts `s` on the player model and its rod (FUN_10089850's +0xa4 calls).
fn start_both(w: &mut World, s: &[u8; 4]) {
    let me = w.my_id;
    w.effects.on_scheduler(me, me, name(s));
    if let Some(rod) = super::fishrod::rod_of(w, me) {
        w.effects.on_scheduler(rod, rod, name(s));
    }
}

fn stop_all(w: &mut World) {
    stop(w, &[b"fsh0", b"fsh1", b"fsh2", b"fsh3", b"fsh4", b"fsh5", b"fsh6", b"fsh7", b"fsh8", b"fsh9"]);
}

/// FUN_1008a080: DAT 3727's `maiN` on us; the clear counter to 1.
fn arrow_fx(w: &mut World, f: &mut Fishing, n: u32) {
    let digit = b"0123456789ab"[n.min(11) as usize];
    let me = Who::Actor(w.my_id);
    w.effects.on_event_scheduler(0xaf0 + 0x39f, u32::from_le_bytes([b'm', b'a', b'i', digit]), me, me);
    f.clear = 1;
}

/// FUN_10089f80: (gold x 2 - dir + 1) x 3 + k; k 0 show, 1 hit, 2 clear.
fn arrow_index(dir: u8, gold: bool, k: u32) -> u32 {
    ((gold as i32 * 2 - dir as i32 + 1) * 3) as u32 + k
}

/// FUN_10089ff0: the counter runs down; at 0 every arrow's clear plays (with `again`, once more 30 frames on).
fn clear_arrows(w: &mut World, f: &mut Fishing, again: bool) {
    if f.clear == 0 {
        return;
    }
    f.clear -= 1;
    if f.clear == 0 {
        for n in [2, 5, 8, 11] {
            arrow_fx(w, f, n);
        }
        f.clear = if again { 30 } else { 0 };
    }
}

/// FUN_10089e40: the movement axis, non-zero only on a press's first frame.
fn axis_edge(f: &mut Fishing) -> f32 {
    let v = f.axis;
    match (f.latch, v != 0.0) {
        (false, true) => {
            f.latch = true;
            v
        }
        (true, true) => 0.0,
        (_, false) => {
            f.latch = false;
            0.0
        }
    }
}

/// FUN_10177730 in the fight (no map: true).
fn water(w: &World) -> bool {
    w.ground.as_ref().is_none_or(|g| g.water_ahead(w.pos, forward(w)))
}

/// One retail frame of FUN_10089100's sub-states.
fn frame(w: &mut World, f: &mut Fishing) {
    let status = w.my_motion.status;
    match f.sub {
        // Waiting for a bite.
        0 => {
            // A result status: the end (and the rest of this case, as the asm falls through).
            if (0x39..=0x3e).contains(&status) {
                f.sub = 3;
            }
            f.clear = 0;
            if !f.early {
                if w.hook_wait <= 0 {
                    send(w, 0, 2, 0);
                    (f.sub, f.frames) = (1, 0);
                    return;
                }
                w.hook_wait -= U;
            }
            if f.enter_free || f.click {
                f.early = true;
            }
            if f.lockout > 0 {
                f.lockout -= U;
                return;
            }
            if f.escape || f.early {
                send(w, 201, 3, 0);
                f.sub = 3;
            }
        }
        // The bite check sent, then the fight.
        1 => {
            if (0x3a..=0x3e).contains(&status) {
                f.sub = 3;
                return;
            }
            let check = f.frames & 0x3f == 0;
            f.frames = f.frames.wrapping_add(1);
            if check && !water(w) {
                send(w, 200, 3, 0);
                f.sub = 3;
                return;
            }
            stop(w, &[b"fsh0"]);
            if f.game.is_none() {
                return;
            }
            tick(w, f);
            let Some(g) = f.game.as_mut() else { return };
            if f.enter {
                let pct = if g.max == 0 { 0 } else { (g.cur as f64 * 100.0 / g.max as f64) as i32 };
                let special = g.special;
                send(w, pct, 3, special);
            } else if !f.escape {
                g.time -= U;
                if g.time <= 0 {
                    send(w, 300, 3, 0);
                } else {
                    if !g.warned && g.time < 420 {
                        let left = g.time / 60;
                        g.warned = true;
                        send(w, left, 5, 0);
                    }
                    return;
                }
            } else {
                send(w, 200, 3, 0);
            }
            f.sub = 2;
        }
        // The end sent: the result.
        2 => {
            if (0x3a..=0x3e).contains(&status) {
                f.sub = 3;
            } else if status != 0x39 {
                stop(w, &[b"fsh7", b"fsh8", b"fsh9"]);
                f.sub = 3;
            }
        }
        // Ending: the release once the result's motion is over.
        3 => {
            clear_arrows(w, f, true);
            stop(w, &[b"fsh0", b"fsh7", b"fsh8", b"fsh9"]);
            if fishing_status(status) && [b"fsh2", b"fsh3", b"fsh4", b"fsh5", b"fsh6"].iter().any(|s| running(w, s)) {
                return;
            }
            send(w, 0, 4, 0);
            f.sub = 4;
            stop_all(w);
            f.game = None;
            w.fish_gauge = None;
            clear_arrows(w, f, true);
        }
        _ => {}
    }
}

/// FUN_10089850: the fight's frame.
fn tick(w: &mut World, f: &mut Fishing) {
    let Some(mut g) = f.game.take() else { return };
    if g.cur == 0 {
        // An exhausted fish: no arrows, no regen.
        clear_arrows(w, f, true);
    } else {
        if f.arrow == -1 {
            // DAT_10485ff2: music slot 7, 129 small / 136 large.
            w.music.set_slot(7, if g.large { 0x88 } else { 0x81 });
            f.arrow = 0;
        }
        if f.arrow == 0 {
            (f.acc, f.t, f.good, f.bad, f.arrow) = (0, 0, 0, 0, 1);
            // FUN_10089ee0: gold when special >= r1 + r2 (1..50 each).
            let r1 = (f.rand() % 51).max(1);
            let r2 = (f.rand() % 51).max(1);
            g.gold = g.special / (r1 + r2) as u32 != 0;
        }
        if f.arrow == 1 {
            f.t += U;
            // A key before the arrow: the fish gets back half its arrow heal.
            if f.t > 30 && axis_edge(f) != 0.0 {
                g.cur += (U as f32 * g.heal as f32 * 0.5) as i32;
                f.bad = 0;
            }
            f.acc += U * g.moves;
            if f.acc >= 6000 && g.cur != 0 {
                g.hit_timer = 0;
                g.dir = (f.rand() & 1) as u8;
                f.acc = (f.rand() % (2 * g.delay) + 2) * 10;
                stop(w, &[b"fsh7", b"fsh8", b"fsh9"]);
                start_both(w, if g.dir == 0 { b"fsh9" } else { b"fsh8" });
                arrow_fx(w, f, arrow_index(g.dir, g.gold, 0));
                (f.t, f.good, f.bad) = (0, 0, 0);
                if g.gold {
                    f.acc = (f.acc as f32 * 0.7) as i32;
                }
                (f.arrow, f.answered) = (2, false);
                log::debug!("fishing: arrow {} gold {} for {} ticks", if g.dir == 0 { "right" } else { "left" }, g.gold, f.acc);
            }
        }
        if f.arrow == 2 {
            g.hit_timer -= U;
            if g.hit_timer < 0 {
                w.effects.on_scheduler(w.my_id, w.my_id, name(if g.large { b"hitl" } else { b"hits" }));
                g.hit_timer = (f.rand() % 6 + 5) * 60;
            }
            f.acc -= U;
            if f.acc <= 0 {
                f.acc = 0;
                stop(w, &[b"fsh8", b"fsh9"]);
                start_both(w, b"fsh7");
                f.arrow = 0;
                clear_arrows(w, f, true);
                if !f.answered {
                    log::debug!("fishing: arrow missed, stamina {}", g.cur);
                }
            } else {
                let v = axis_edge(f);
                let right = if g.dir == 0 { v > 0.0 } else { v < 0.0 };
                let wrong = if g.dir == 0 { v < 0.0 } else { v > 0.0 };
                f.good += right as u32;
                f.bad += wrong as u32;
                if f.good > f.bad {
                    g.cur -= g.damage * if g.gold { 2 } else { 1 };
                    f.acc = 0;
                    arrow_fx(w, f, arrow_index(g.dir, g.gold, 1));
                    (f.good, f.bad, f.answered) = (0, 0, true);
                    log::debug!("fishing: arrow hit, stamina {}", g.cur.max(0));
                } else if f.bad > f.good {
                    g.cur += (if g.gold { (g.heal as f32 * 0.25) as i32 } else { g.heal }).max(1);
                    f.acc = 0;
                    arrow_fx(w, f, arrow_index(g.dir, g.gold, 2));
                    (f.good, f.bad, f.answered) = (0, 0, true);
                    log::debug!("fishing: wrong key, stamina {}", g.cur);
                }
            }
        }
        g.cur = (g.cur + g.regen * U).clamp(0, g.max);
    }
    // The shown value eases by 0.15 of the gap (all of it within 1); the party window's +0x3a percent.
    if g.max != 0 {
        let d = g.cur - g.shown;
        let step = d as f32 * 0.15;
        g.shown = (g.shown as f32 + if step.abs() <= 1.0 { d as f32 } else { step }) as i32;
        w.fish_gauge = Some((g.shown as i64 * 100 / g.max as i64).clamp(0, 100) as u8);
    }
    f.game = Some(g);
}

/// FUN_10202af0 at the end of the party window's draw: "Fish" and its gauge at (W / 2 - 58, 0.2 H), while fishing
/// and no event holds the UI.
pub fn draw_gauge(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    let Some(pct) = w.fish_gauge else { return };
    if !fishing_status(w.my_motion.status) || super::event::active(w) {
        return;
    }
    let (x, y) = ((sw / 2.0).trunc() - 58.0, (sh / 2.0 * 0.4).trunc());
    // FUN_101cb0d0: windowps 97 at 2 x pct / 100 wide from (4, 1), framesus 98 dimmed at 0.
    if pct > 0 {
        gu.sprite_scaled_blend("menu    windowps", 97, x + 4.0, y + 8.0, 2.0 * pct as f32 / 100.0, 1.0, 0x8080_8080, 8);
    }
    gu.sprite("menu    framesus", 98, x, y + 7.0, if pct == 0 { 0x3050_5050 } else { 0x8080_8080 });
    let tint = gu.ncol(if pct > 0 { 0 } else { 9 });
    gu.shape_fit("Fish", x, y, 102.0, tint);
}
