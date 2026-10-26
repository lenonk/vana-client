//! The delivery box (s2c 0x04B / c2s 0x04D): the session's slot store (FUN_100a38b0), the receive UI ("menu    post1"
//! / "menu    post2", FUN_1014cb70) and the send UI ("menu    delivery" / "menu    stringdl", FUN_1012e880). Each UI is a
//! per-frame state machine that sends one request and waits for its answer; the state numbers are retail's. MODLOG
//! "Delivery Box".

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use std::time::{Duration, Instant};

const POST1: &str = "menu    post1";
const POST2: &str = "menu    post2";
const DELIVERY: &str = "menu    delivery";
const STRINGDL: &str = "menu    stringdl";
const MONEYCTR: &str = "menu    moneyctr";
const ITEMCTRL: &str = "menu    itemctrl";
/// 300 ticks *(inf: 60 a second)*.
const TIMEOUT: Duration = Duration::from_secs(5);
const GIL: u16 = 0xffff;

mod cmd {
    pub const WORK: u8 = 1;
    pub const SET: u8 = 2;
    pub const SEND: u8 = 3;
    pub const CANCEL: u8 = 4;
    pub const CHECK: u8 = 5;
    pub const RECV: u8 = 6;
    pub const CONFIRM: u8 = 7;
    pub const ACCEPT: u8 = 8;
    pub const REJECT: u8 = 9;
    pub const GET: u8 = 0xa;
    pub const CLEAR: u8 = 0xb;
    pub const QUERY: u8 = 0xc;
    pub const DELI_OPEN: u8 = 0xd;
    pub const POST_OPEN: u8 = 0xe;
    pub const POST_CLOSE: u8 = 0xf;
}

/// A box slot (FUN_100f80f0's 0x48-byte record): Stat, To / From, OpponentPBoxNo (AH gil: item id and sold count),
/// ItemNo (0xffff gil), Stack, extdata.
#[derive(Clone, Debug, Default)]
pub struct Slot {
    pub stat: u32,
    pub name: String,
    pub work_no: u32,
    pub item: u16,
    pub stack: u32,
    pub ext: [u8; 24],
}

/// s2c 0x04B: Command @4, BoxNo @5, PostWorkNo @6, Result @0xc, ResParam1..3 @0xd, the slot from @0x10.
#[derive(Clone, Debug)]
pub struct Pbx {
    pub cmd: u8,
    pub box_no: i8,
    pub post: i8,
    pub result: i8,
    pub res: [i8; 3],
    pub slot: Slot,
}

/// The session part (S+0x2c7c8 ..) and the two UIs.
#[derive(Default)]
pub struct Session {
    recv: [Slot; 8],
    send: [Slot; 8],
    /// The latest stored answer (Command, Result) with flag 0x4000000; the UIs take it once per frame.
    last: Option<(u8, i8)>,
    /// Check's counts (flags 0x2000000 / 0x1000000).
    recv_count: Option<i32>,
    send_count: Option<i32>,
    recv_ui: Option<Recv>,
    send_ui: Option<Send>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RFocus {
    Post1,
    Post2,
}

/// DAT_10578450: +0x14 state, +0x18 request slot, +0x1c timeout, +0x20 / +0x24 the cursor slot and its Stat last
/// frame; post1's cursor, post2's cursor / result (+0x5c) / child 3 state, busy (post1 +0x5d).
struct Recv {
    state: u8,
    slot: usize,
    timeout: Option<Instant>,
    count: i32,
    cursor: u16,
    last_cursor: u16,
    last_stat: u32,
    focus: RFocus,
    post2: u16,
    result: u8,
    no_return: bool,
    busy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SFocus {
    Delivery,
    Stringdl,
    List,
    Qty,
    Money,
}

/// "menu    stringdl" (DAT_10578470): +0x14 mode (1 entry, 2 cancel mode), +0x15c the result, the text input (15
/// characters), +0x58 the accepted name, +0x118 the label (group 9), +0x18 the name shown instead of the field.
struct Stringdl {
    mode: u8,
    result: u8,
    cursor: u16,
    editing: bool,
    text: String,
    accepted: String,
    label: usize,
    shown: Option<String>,
    caret: Instant,
}

/// A placed item not yet Set (the local record's pending flag): inventory index, item, count.
#[derive(Clone, Copy, Debug)]
struct Pending {
    index: u8,
    item: u16,
    count: u32,
}

/// DAT_10578198: +0x1b4 state, +0x1b6 the state after the close sequence, +0x1b8 after a Get, +0x1bc the request slot,
/// +0x1c0 the timeout, +0x1c6 the recipient, +0x1f2 the recipient on our account; the delivery cursor, busy (+0x5d).
struct Send {
    state: u8,
    next: u8,
    after_get: u8,
    slot: usize,
    timeout: Option<Instant>,
    count: i32,
    name: String,
    on_account: bool,
    pending: [Option<Pending>; 8],
    cursor: u16,
    focus: SFocus,
    busy: bool,
    dl: Stringdl,
    list: (usize, usize),
    qty: Option<(u8, u32, u32)>,
    money: Option<(u32, u32, u32)>,
}

// --- c2s 0x04D (FUN_100f7bc0: 0x20 bytes; Result / ResParam 0) ---

fn pbx(w: &World, cmd: u8, box_no: i8, post: i8, item: i8, stacks: i32, name: &str) {
    let mut b = vec![0u8; 0x1c];
    (b[0], b[1], b[2], b[3]) = (cmd, box_no as u8, post as u8, item as u8);
    b[4..8].copy_from_slice(&stacks.to_le_bytes());
    for (i, c) in name.bytes().take(15).enumerate() {
        b[0xc + i] = if i == 0 { c.to_ascii_uppercase() } else { c };
    }
    log::debug!("c2s 0x04D cmd {cmd:#x} box {box_no} post {post} item {item} stacks {stacks} {name:?}");
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x04D, body: b });
}

fn simple(w: &World, cmd: u8, box_no: i8, post: i8) {
    pbx(w, cmd, box_no, post, -1, -1, "");
}

/// FUN_100f7fa0: DeliOpen 0x0d, PostOpen 0x0e, PostClose 0x0f.
pub fn open_cmd(w: &World, c: u8) {
    simple(w, c, -1, -1);
}

pub fn send_open(w: &World) -> bool {
    w.post.send_ui.is_some()
}

pub fn recv_open(w: &World) -> bool {
    w.post.recv_ui.is_some()
}

pub fn active(w: &World) -> bool {
    send_open(w) || recv_open(w)
}

// --- s2c 0x04B ---

/// FUN_100a38b0: Result -6 / -7 lines first, then the store by Command (only the Results each store takes), the log
/// lines of Get / Clear / Reject, Query's answer, and the opens / closes.
pub fn on_pbx(w: &mut World, p: Pbx) {
    log::debug!("s2c 0x04B cmd {:#x} box {} post {} result {} res {:?} stat {} item {:#x} x{} {:?}", p.cmd, p.box_no, p.post, p.result, p.res, p.slot.stat, p.slot.item, p.slot.stack, p.slot.name);
    match p.result {
        -6 => log_group9(w, 0x2d, 0x79),
        -7 => super::textcmd::refuse(w, 0xb9),
        _ => {}
    }
    let r = p.result;
    let to_recv = |b: i8| b == 1;
    match p.cmd {
        cmd::WORK if r == 1 => store(w, to_recv(p.box_no), &p),
        cmd::SET if r == 1 => store(w, false, &p),
        cmd::SEND | cmd::CANCEL if matches!(r, 1 | 2) => store(w, false, &p),
        cmd::CHECK if r == 1 => {
            let [_, rc, sc] = p.res;
            if rc != -1 && sc == -1 {
                w.post.recv_count = Some(rc as i32);
            } else if rc == -1 && sc != -1 {
                w.post.send_count = Some(sc as i32);
            }
        }
        cmd::RECV | cmd::ACCEPT if matches!(r, 1 | 2) => store(w, true, &p),
        cmd::CONFIRM if r == 1 => store(w, false, &p),
        cmd::REJECT | cmd::GET | cmd::CLEAR if matches!((p.cmd, r), (cmd::REJECT, 1 | 2) | (_, 1)) => {
            if (p.box_no == 1 && recv_open(w)) || (p.box_no == 2 && p.cmd == cmd::GET && send_open(w)) {
                item_line(w, &p);
            }
            store(w, p.cmd == cmd::REJECT || to_recv(p.box_no), &p);
        }
        // FUN_1012f880: ResParam1 1 = the recipient is on our account (CanSendAccount items).
        cmd::QUERY => {
            if let Some(s) = w.post.send_ui.as_mut() {
                s.on_account = r == 1 && p.res[0] == 1;
            }
        }
        cmd::DELI_OPEN if r == 1 => open_send(w),
        cmd::POST_OPEN if r == 1 => open_recv(w),
        cmd::POST_CLOSE if r == 1 => {
            close_recv(w);
            close_send(w);
        }
        _ => {}
    }
    if (cmd::WORK..=cmd::QUERY).contains(&p.cmd) {
        w.post.last = Some((p.cmd, r));
    }
}

/// FUN_100f8270 / FUN_100f80f0: the slot record; Stat 0 empties the item fields.
fn store(w: &mut World, recv: bool, p: &Pbx) {
    let Ok(i) = usize::try_from(p.post) else { return };
    if i >= 8 {
        return;
    }
    let mut s = p.slot.clone();
    if s.stat == 0 {
        (s.item, s.stack, s.work_no, s.ext) = (0, 0, 0, [0; 24]);
    }
    if recv {
        w.post.recv[i] = s;
    } else {
        w.post.send[i] = s;
    }
}

pub(super) fn log_group9(w: &mut World, id: usize, mode: u8) {
    let Some(bt) = w.battle.clone() else { return };
    let colour = bt.colour_in(mode, &w.game);
    if let Some(l) = bt.group9(id) {
        w.chat.push_colour(l, colour);
    }
}

/// A file 7031 line with work {slot + 1 or slot, ...} and the string parameter (FUN_100a1ab0).
pub(super) fn system_line(w: &mut World, id: u16, work: [i32; 5], text: String) {
    let Some(bt) = w.battle.clone() else { return };
    let ctx = crate::dat::battle::Ctx { me: w.name.clone(), work, strings: [text, String::new()], ..Default::default() };
    if let Some(line) = bt.system(id, &ctx) {
        let colour = bt.colour_in(line.mode.unwrap_or(0), &w.game);
        for l in line.lines.into_iter().filter(|l| !l.is_empty()) {
            w.chat.push_colour(l, colour);
        }
    }
}

/// FUN_1014d0b0 (receive box, BoxNo 1) / FUN_1012f2a0 (send box Get, BoxNo 2): Result 1, a slot, an item and a count;
/// table 0x10371a64 {gil, one, several} per Command, string group 9 0xc1 / 0xc0 "delivery".
fn item_line(w: &mut World, p: &Pbx) {
    let s = &p.slot;
    let Ok(slot) = usize::try_from(p.post) else { return };
    if p.result != 1 || slot >= 8 || s.item == 0 || s.stack == 0 || (s.item == GIL && s.stack >= 1_000_000_000) {
        return;
    }
    let recv = p.box_no == 1;
    let ids: [u16; 3] = match p.cmd {
        cmd::GET => [0xbe, 0xc1, 0xc2],
        cmd::CLEAR => [0xbf, 0xc3, 0xc4],
        _ => [0xc0, 0xc5, 0xc6],
    };
    let id = if s.item == GIL { ids[0] } else if s.stack == 1 { ids[1] } else { ids[2] };
    let text = w.battle.as_ref().and_then(|b| b.group9(if recv { 0xc1 } else { 0xc0 })).unwrap_or_default();
    system_line(w, id, [slot as i32 + 1, s.item as i32, s.stack as i32, 0, 0], text);
}

/// FUN_1014ca70: the gil line for the cursor slot (param 0 its child, 1..8): 0xaa from a player, 0xab / 0xc7 for an auction sale ("AH-" sender:
/// item id and sold count from OpponentPBoxNo).
fn gil_line(w: &mut World, slot: usize) {
    let s = w.post.recv[slot].clone();
    if s.item != GIL || s.stack == 0 || s.stack >= 1_000_000_000 {
        return;
    }
    if !s.name.starts_with("AH-") {
        let name: String = s.name.chars().take(15).collect();
        system_line(w, 0xaa, [slot as i32 + 1, s.stack as i32, 0, 0, 0], name);
        return;
    }
    let (item, sold) = (s.work_no & 0xffff, s.work_no >> 16 & 0xff);
    if (1..0xffff).contains(&item) {
        system_line(w, if sold <= 1 { 0xab } else { 0xc7 }, [slot as i32 + 1, s.stack as i32, item as i32, sold as i32, 0], String::new());
    }
}

/// The open refusals of FUN_1012f100 / the receive side: a blocking window (group 7 0x45), healing (0x51); already open:
/// silent. True when it may open.
fn may_open(w: &mut World) -> bool {
    if active(w) {
        return false;
    }
    if w.win.is_some() || w.map.is_some() || w.config.is_some() || w.mogdoor.is_some() || w.magic.is_some() || w.ability.is_some() {
        super::textcmd::refuse(w, 0x45);
        return false;
    }
    if matches!(w.my_motion.status, 0x21 | 7) {
        super::textcmd::refuse(w, 0x51);
        return false;
    }
    true
}

// --- The receive UI ---

/// FUN_1014ce80: post1 (focused), post2, money and the look-only inventory list; Work (1, -1), then state 1.
fn open_recv(w: &mut World) {
    if !may_open(w) {
        return;
    }
    (w.post.last, w.post.recv_count) = (None, None);
    w.post.recv_ui = Some(Recv { state: 1, slot: 0, timeout: None, count: 0, cursor: 1, last_cursor: 0, last_stat: 0, focus: RFocus::Post1, post2: 1, result: 5, no_return: false, busy: false });
    simple(w, cmd::WORK, 1, -1);
}

/// FUN_1014d000: the server's close moves to state 0x12 (closed the next frame).
fn close_recv(w: &mut World) {
    if let Some(r) = w.post.recv_ui.as_mut()
        && r.state != 0x12
    {
        r.state = 0x12;
    }
}

/// FUN_1014cb70.
fn recv_frame(w: &mut World) {
    let Some(mut r) = w.post.recv_ui.take() else { return };
    let now = Instant::now();
    let expired = r.timeout.is_some_and(|t| now >= t);
    // The cursor slot's gil line: on a move, or when it turns from 6 to 7 under the cursor.
    let slot = (r.cursor as usize).wrapping_sub(1);
    if slot < 8 {
        let stat = w.post.recv[slot].stat;
        if r.cursor != r.last_cursor || (r.last_stat == 6 && stat == 7) {
            gil_line(w, slot);
        }
        (r.last_cursor, r.last_stat) = (r.cursor, stat);
    }
    let last = w.post.last;
    let answer = |c: u8| last.filter(|l| l.0 == c && l.1 != 2).map(|l| l.1);
    let cur = (r.cursor as usize).saturating_sub(1).min(7);
    let timeout = |r: &mut Recv| r.timeout = Some(now + TIMEOUT);
    match r.state {
        1 => {
            w.post.recv_count = None;
            simple(w, cmd::CHECK, 1, -1);
            timeout(&mut r);
            (r.busy, r.state) = (true, 2);
        }
        2 => {
            if let Some(n) = w.post.recv_count.take() {
                r.count = n;
                if n > 0 {
                    r.state = 3;
                } else {
                    (r.busy, r.state) = (false, 5);
                }
            } else if expired {
                r.state = 0x13;
            }
        }
        3 => match w.post.recv.iter().position(|s| s.stat == 0).filter(|_| r.count > 0) {
            Some(i) => {
                pbx(w, cmd::RECV, 1, i as i8, 1, -1, "");
                (r.busy, r.slot, r.state) = (true, i, 4);
                timeout(&mut r);
            }
            None => (r.busy, r.state) = (false, 5),
        },
        4 => match answer(cmd::RECV) {
            Some(1) => {
                r.count -= 1;
                (r.busy, r.state) = (false, 3);
            }
            Some(_) => {
                simple(w, cmd::WORK, 1, r.slot as i8);
                (r.busy, r.state) = (false, 1);
            }
            None if expired => r.state = 0x13,
            None => {}
        },
        5 => {
            (r.result, r.no_return, r.focus, r.state) = (5, false, RFocus::Post1, 6);
        }
        // FUN_1014d400: post2's answer by the cursor slot's Stat.
        7 => {
            let stat = w.post.recv[cur].stat;
            let next = match (r.result, stat) {
                (5, _) => None,
                (1, 7) => Some(8),
                (1, 0xb) => Some(0xa),
                (2, 7) => Some(0xe),
                (2, 0xb) => Some(0x10),
                (3, 7 | 0xb) => Some(0xc),
                (4, _) => Some(5),
                _ => {
                    w.sounds.push(Ui::Error as u32);
                    Some(5)
                }
            };
            if let Some(n) = next {
                r.busy = n != 5;
                (r.slot, r.state) = (cur, n);
            }
        }
        8 | 0xe => {
            simple(w, cmd::ACCEPT, 1, r.slot as i8);
            timeout(&mut r);
            (r.busy, r.state) = (true, r.state + 1);
        }
        9 | 0xf => match answer(cmd::ACCEPT) {
            Some(1) => r.state = if r.state == 9 { 0xa } else { 0x10 },
            Some(_) => {
                w.sounds.push(Ui::Error as u32);
                simple(w, cmd::WORK, 1, r.slot as i8);
                (r.busy, r.state) = (false, 5);
            }
            None if expired => r.state = 0x13,
            None => {}
        },
        0xa => {
            pbx(w, cmd::GET, 1, r.slot as i8, -1, -1, "");
            timeout(&mut r);
            (r.busy, r.state) = (true, 0xb);
        }
        // FUN_1014d5e0 / FUN_1014d930: a failed Get prints 0x88 / 0x89 by its result.
        0xb => match answer(cmd::GET) {
            Some(1) => (r.busy, r.state) = (false, 5),
            Some(e) => {
                w.sounds.push(Ui::Error as u32);
                match e {
                    -0x47 => super::textcmd::refuse(w, 0x88),
                    -0x46 => super::textcmd::refuse(w, 0x89),
                    _ => {}
                }
                simple(w, cmd::WORK, 1, r.slot as i8);
                (r.busy, r.state) = (false, 5);
            }
            None if expired => r.state = 0x13,
            None => {}
        },
        0xc => {
            simple(w, cmd::REJECT, 1, r.slot as i8);
            timeout(&mut r);
            (r.busy, r.state) = (true, 0xd);
        }
        0xd => match answer(cmd::REJECT) {
            Some(_) => (r.busy, r.state) = (false, 5),
            None if expired => r.state = 0x13,
            None => {}
        },
        0x10 => {
            simple(w, cmd::CLEAR, 1, r.slot as i8);
            timeout(&mut r);
            (r.busy, r.state) = (true, 0x11);
        }
        0x11 => match answer(cmd::CLEAR) {
            Some(1) => (r.busy, r.state) = (false, 5),
            Some(_) => {
                w.sounds.push(Ui::Error as u32);
                simple(w, cmd::WORK, 1, r.slot as i8);
                (r.busy, r.state) = (false, 5);
            }
            None if expired => r.state = 0x13,
            None => {}
        },
        0x12 => return,
        0x13 => {
            super::textcmd::refuse(w, 0x85);
            (r.busy, r.state) = (false, 0x12);
        }
        _ => {}
    }
    w.post.recv_ui = Some(r);
}

/// post1 (FUN_1014cda0) and post2 (FUN_1014db80) keys.
fn recv_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut r) = w.post.recv_ui.take() else { return };
    let cur = (r.cursor as usize).saturating_sub(1).min(7);
    match r.focus {
        _ if r.busy || r.state == 0x12 => {}
        RFocus::Post1 => match key {
            // Child 9 (gil) is skipped (state 0x10; the per-frame puts the cursor back).
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = link(gu, POST1, r.cursor, key);
                if to != r.cursor && (1..=8).contains(&to) {
                    r.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter => {
                let s = w.post.recv[cur].clone();
                if matches!(s.stat, 7 | 0xb | 0xd) {
                    r.no_return = matches!(s.stat, 7 | 0xb) && s.name.starts_with("AH-");
                    (r.post2, r.focus, r.result, r.state) = (1, RFocus::Post2, 5, 7);
                }
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                open_cmd(w, cmd::POST_CLOSE);
            }
            _ => {}
        },
        RFocus::Post2 => match key {
            K::ArrowRight | K::ArrowLeft | K::ArrowUp | K::ArrowDown => {
                let to = link(gu, POST2, r.post2, key);
                if to != r.post2 && (1..=3).contains(&to) {
                    r.post2 = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter if r.post2 == 3 && r.no_return => w.sounds.push(Ui::Error as u32),
            K::Enter if r.post2 == 1 => {
                (r.result, r.focus) = (1, RFocus::Post1);
                w.sounds.push(Ui::Select as u32);
            }
            // FUN_10210e30: comyn "Dispose of this item?" / "Return this item to sender?", default No.
            K::Enter => {
                let ctx = r.post2 as u8;
                w.sounds.push(Ui::Select as u32);
                w.confirm = Some((super::Confirm::Post(ctx), false));
            }
            K::Escape => {
                (r.result, r.focus) = (4, RFocus::Post1);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        },
    }
    w.post.recv_ui = Some(r);
}

/// LAB_1014dc20: Yes on comyn: post2's result = 2 / 3 and post1 takes the focus.
pub fn confirmed(w: &mut World, ctx: u8) {
    if let Some(r) = w.post.recv_ui.as_mut() {
        (r.result, r.focus) = (ctx, RFocus::Post1);
    }
}

pub fn confirm_text(gu: &GameUi, ctx: u8) -> String {
    gu.item_msg(9, if ctx == 2 { 0x34 } else { 0x35 }).unwrap_or_default()
}

// --- The send UI ---

/// FUN_1012eff0: the picker list, money, delivery and stringdl (focused); Work (2, -1); label "Recipient"; state 1.
fn open_send(w: &mut World) {
    if !may_open(w) {
        return;
    }
    (w.post.last, w.post.send_count) = (None, None);
    let dl = Stringdl { mode: 1, result: 0, cursor: 1, editing: false, text: String::new(), accepted: String::new(), label: 0x2e, shown: None, caret: Instant::now() };
    w.post.send_ui = Some(Send {
        state: 1,
        next: 0x18,
        after_get: 0x18,
        slot: 0,
        timeout: None,
        count: 0,
        name: String::new(),
        on_account: false,
        pending: [None; 8],
        cursor: 1,
        focus: SFocus::Stringdl,
        busy: false,
        dl,
        list: (0, 0),
        qty: None,
        money: None,
    });
    simple(w, cmd::WORK, 2, -1);
}

/// FUN_1012f1b0: in state 0x19 everything closes; in the close sequence nothing; otherwise the close sequence runs
/// first (unsent items come back).
fn close_send(w: &mut World) {
    let Some(s) = w.post.send_ui.as_mut() else { return };
    match s.state {
        0x19 => w.post.send_ui = None,
        0x15..=0x17 => {}
        _ => (s.next, s.state) = (0x18, 0x15),
    }
}

/// The picker's rows: the occupied inventory slots but gil, in the view order.
pub(super) fn picker_rows(w: &World) -> Vec<crate::inventory::Slot> {
    w.inv.viewed(super::items::INVENTORY).into_iter().filter(|s| s.index != 0 && s.id != 0).collect()
}

/// FUN_1012e1a0: NoDelivery (0x2000) without CanSendAccount (0x10) grey; CanSendAccount only to our own account;
/// equipped grey. The augment rule (Ex by augment) and the equipped linkshell are not checked here.
fn picker_tint(w: &World, s: &crate::inventory::Slot, on_account: bool) -> u32 {
    use crate::inventory::{TINT_DIM, TINT_WHITE};
    let flags = w.item_info.as_ref().and_then(|c| c.item(s.id)).map_or(0, |it| it.flags);
    let grey = if flags & 0x10 == 0 { flags & 0x2000 != 0 } else { !on_account };
    if grey || s.equipped { TINT_DIM } else { TINT_WHITE }
}

/// FUN_1012f380: the delivery cursor slot takes the item (pending until Set goes out next frame).
fn place(s: &mut Send, index: u8, item: u16, count: u32) -> bool {
    let n = s.cursor as usize;
    if !(1..=8).contains(&n) || index >= 0x51 || item == 0 {
        return false;
    }
    s.pending[n - 1] = Some(Pending { index, item, count });
    s.state = 0xb;
    true
}

/// FUN_1012e880.
fn send_frame(w: &mut World) {
    let Some(mut s) = w.post.send_ui.take() else { return };
    let now = Instant::now();
    let expired = s.timeout.is_some_and(|t| now >= t);
    let last = w.post.last;
    let answer = |c: u8| last.filter(|l| l.0 == c && l.1 != 2).map(|l| l.1);
    let arm = |s: &mut Send| s.timeout = Some(now + TIMEOUT);
    // FUN_10130150: a wait that runs out goes to 0x1a.
    if expired && matches!(s.state, 2 | 4 | 8 | 0xc | 0xe | 0x12 | 0x14) {
        s.state = 0x1a;
    }
    match s.state {
        1 => {
            w.post.send_count = None;
            simple(w, cmd::CHECK, 2, -1);
            arm(&mut s);
            s.state = 2;
        }
        2 => {
            if let Some(n) = w.post.send_count.take() {
                s.count = n;
                s.state = if n > 0 { 3 } else { 5 };
            }
        }
        3 => {
            if s.count < 1 {
                s.state = 5;
            } else {
                simple(w, cmd::CONFIRM, -1, -1);
                arm(&mut s);
                s.state = 4;
            }
        }
        4 => {
            if last.is_some_and(|l| matches!(l.0, cmd::CONFIRM | cmd::WORK) && l.1 != 2) {
                s.count -= 1;
                s.state = 3;
            }
        }
        // FUN_1012f570: name entry again (the typed text stays in the field).
        5 => {
            s.dl.accepted.clear();
            (s.dl.mode, s.dl.result, s.dl.label, s.dl.shown, s.dl.cursor, s.dl.editing) = (1, 0, 0x2e, None, 1, false);
            s.focus = SFocus::Stringdl;
            simple(w, cmd::WORK, 2, -1);
            s.state = 6;
        }
        // FUN_1012f620: stringdl's answer.
        6 if s.dl.result != 0 => {
            let res = std::mem::take(&mut s.dl.result);
            s.focus = SFocus::Delivery;
            match res {
                1 if super::textcmd::name_case(&s.dl.accepted).is_some() => {
                    let name = super::textcmd::name_case(&s.dl.accepted).unwrap_or_default();
                    (s.name, s.on_account, s.cursor, s.busy, s.state) = (name, false, 1, true, 7);
                }
                2 => (s.next, s.state) = (0x18, 0x15),
                5 => {
                    log_group9(w, 0x32, 0x79);
                    (s.cursor, s.state) = (5, 0xf);
                }
                _ => {
                    w.sounds.push(Ui::Error as u32);
                    (s.dl.cursor, s.focus) = (1, SFocus::Stringdl);
                }
            }
        }
        7 => {
            pbx(w, cmd::QUERY, -1, -1, -1, -1, &s.name);
            s.on_account = false;
            arm(&mut s);
            s.state = 8;
        }
        8 => match answer(cmd::QUERY) {
            Some(1) => (s.focus, s.busy, s.timeout, s.state) = (SFocus::Delivery, false, None, 9),
            Some(_) => {
                w.sounds.push(Ui::Error as u32);
                (s.busy, s.state) = (false, 5);
            }
            None => {}
        },
        9 | 0x10 => {
            stringdl_label(w, &mut s);
            if s.state == 0x10 && s.dl.result != 0 {
                cancel_answer(w, &mut s);
            }
        }
        0xb => match s.pending.iter().position(Option::is_some) {
            Some(i) => {
                let p = s.pending[i].take().unwrap();
                pbx(w, cmd::SET, 2, i as i8, p.index as i8, p.count as i32, &s.name);
                (s.busy, s.slot, s.state) = (true, i, 0xc);
                arm(&mut s);
            }
            None => (s.busy, s.state) = (false, 9),
        },
        0xc => match answer(cmd::SET) {
            Some(1) => s.state = 0xb,
            Some(_) => {
                simple(w, cmd::WORK, 2, s.slot as i8);
                s.state = 0xb;
            }
            None => {}
        },
        0xd => match w.post.send.iter().position(|x| x.stat == 1) {
            Some(i) => {
                simple(w, cmd::SEND, 2, i as i8);
                (s.busy, s.slot, s.state) = (true, i, 0xe);
                arm(&mut s);
            }
            None => (s.busy, s.state) = (false, 5),
        },
        0xe => match answer(cmd::SEND) {
            Some(1) => s.state = 0xd,
            Some(_) => {
                simple(w, cmd::WORK, 2, s.slot as i8);
                (s.busy, s.state) = (false, 5);
            }
            None => {}
        },
        0xf => (s.focus, s.state) = (SFocus::Delivery, 0x10),
        0x11 => {
            s.focus = SFocus::Delivery;
            simple(w, cmd::CANCEL, 2, s.slot as i8);
            arm(&mut s);
            s.state = 0x12;
        }
        0x12 => {
            if let Some(l) = last.filter(|l| l.0 == cmd::CANCEL && l.1 != 2) {
                if l.1 != 1 {
                    simple(w, cmd::WORK, 2, s.slot as i8);
                }
                (s.busy, s.state) = (false, 0x10);
            }
        }
        0x13 => {
            s.focus = SFocus::Delivery;
            simple(w, cmd::GET, 2, s.slot as i8);
            arm(&mut s);
            s.state = 0x14;
        }
        0x14 => {
            if last.is_some_and(|l| l.0 == cmd::GET) {
                (s.busy, s.state, s.after_get) = (false, s.after_get, 0x18);
            }
        }
        // FUN_1012fec0 / FUN_1012ff30: unsent items (Stat 1 / 0x10) come back, then the next state.
        0x15 => {
            s.pending = [None; 8];
            (s.dl.label, s.dl.result, s.dl.mode, s.dl.shown) = (0x2e, 0, 1, None);
            s.state = 0x16;
        }
        0x16 => {
            s.busy = true;
            let back: Vec<usize> = (0..8).filter(|&i| matches!(w.post.send[i].stat, 1 | 0x10)).collect();
            for &i in &back {
                simple(w, cmd::GET, 2, i as i8);
            }
            if back.is_empty() {
                (s.state, s.next) = (s.next, 0x18);
            } else {
                arm(&mut s);
                s.state = 0x17;
            }
        }
        0x17 => {
            if last.is_some_and(|l| l.0 == cmd::GET) || expired {
                if expired {
                    super::textcmd::refuse(w, 0x85);
                }
                (s.state, s.next) = (s.next, 0x18);
            }
        }
        0x18 => {
            open_cmd(w, cmd::POST_CLOSE);
            s.state = 0x19;
        }
        0x1a => {
            super::textcmd::refuse(w, 0x85);
            (s.busy, s.next, s.state) = (false, 0x18, 0x15);
        }
        _ => {}
    }
    // FUN_10130290: a negative result's line, then SE 0004.
    if let Some((_, e)) = last.filter(|l| l.1 < 0) {
        let id = match e {
            -0x47 => Some(0x88),
            -0x46 => Some(0x89),
            -7 => Some(0xb9),
            -2 => Some(0x87),
            -1 => Some(0x86),
            _ => None,
        };
        if let Some(id) = id {
            super::textcmd::refuse(w, id);
        }
        w.sounds.push(Ui::Error as u32);
    }
    w.post.send_ui = Some(s);
}

/// FUN_10130170: stringdl's label and name follow the delivery cursor's slot.
fn stringdl_label(w: &World, s: &mut Send) {
    let c = s.cursor as usize;
    let slot = (1..=8).contains(&c).then(|| &w.post.send[c - 1]);
    let (label, shown) = match slot.map(|x| x.stat) {
        Some(1) => (0x2f, slot.map(|x| x.name.clone())),
        Some(3) => (0x30, slot.map(|x| x.name.clone())),
        Some(5 | 0x10) => (0x31, slot.map(|x| x.name.clone())),
        Some(2 | 4 | 0xf) => (0x2e, slot.map(|x| x.name.clone())),
        _ if s.state == 0x10 => (0x2e, Some(String::new())),
        _ if (1..=11).contains(&c) => (0x2e, None),
        _ => (0x2e, Some(String::new())),
    };
    (s.dl.label, s.dl.shown) = (label, shown);
}

/// FUN_1012fb90: stringdl's answer in cancel mode: OK takes a placed / canceled item back or cancels a sent one.
fn cancel_answer(w: &World, s: &mut Send) {
    let res = std::mem::take(&mut s.dl.result);
    let slot = (s.cursor as usize).saturating_sub(1).min(7);
    if res == 3 {
        match w.post.send[slot].stat {
            1 | 5 | 9 | 0x10 => (s.busy, s.after_get, s.slot, s.state) = (true, 0x10, slot, 0x13),
            3 => (s.busy, s.slot, s.state) = (true, slot, 0x11),
            _ => {}
        }
    }
    (s.dl.mode, s.focus) = (1, SFocus::Delivery);
}

/// The send UI's keys: delivery (FUN_1012ece0 / FUN_1012ef60), stringdl (FUN_1014f520 / FUN_1014f700), the picker
/// (FUN_1012e310), itemctrl (FUN_1013a6c0) and moneyctr (FUN_10140bc0).
fn send_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut s) = w.post.send_ui.take() else { return };
    let empty = |w: &World, s: &Send, n: usize| w.post.send[n].stat == 0 && s.pending[n].is_none();
    match s.focus {
        _ if s.busy && s.focus != SFocus::Stringdl => {}
        SFocus::Delivery => match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = link(gu, DELIVERY, s.cursor, key);
                if to != s.cursor && to != 0 && !(s.state == 0x10 && to == 11) {
                    s.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter => {
                let c = s.cursor as usize;
                match (c, s.state) {
                    (1..=8, 9) if empty(w, &s, c - 1) => {
                        w.sounds.push(Ui::Select as u32);
                        s.focus = SFocus::List;
                    }
                    (1..=8, 9) => {
                        w.sounds.push(Ui::Select as u32);
                        (s.busy, s.slot, s.after_get, s.state) = (true, c - 1, 9, 0x13);
                    }
                    (1..=8, 0x10) if empty(w, &s, c - 1) => w.sounds.push(Ui::Error as u32),
                    (1..=8, 0x10) => {
                        w.sounds.push(Ui::Select as u32);
                        (s.dl.result, s.dl.mode, s.dl.cursor, s.focus) = (0, 2, 2, SFocus::Stringdl);
                    }
                    (9, 9) => {
                        w.sounds.push(Ui::Select as u32);
                        s.state = 0xd;
                    }
                    (10, _) => {
                        w.sounds.push(Ui::Select as u32);
                        (s.next, s.state) = (0x18, 0x15);
                    }
                    (11, 9) => match (0..8).find(|&n| empty(w, &s, n)) {
                        Some(n) => {
                            w.sounds.push(Ui::Select as u32);
                            s.cursor = n as u16 + 1;
                            s.money = Some((1, w.inv.gil(), 0));
                            (s.focus, s.state) = (SFocus::Money, 0xa);
                        }
                        None => w.sounds.push(Ui::Error as u32),
                    },
                    (9 | 11, _) => w.sounds.push(Ui::Error as u32),
                    _ => {}
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                match s.state {
                    9 | 0x10 => (s.next, s.state) = (5, 0x15),
                    0x15..=0x17 => (s.state, s.next) = (s.next, 0x18),
                    0x19 => s.state = 0x18,
                    0x1a => {}
                    _ => (s.next, s.state) = (0x18, 0x15),
                }
            }
            _ => {}
        },
        SFocus::Stringdl => stringdl_key(w, gu, &mut s, key),
        SFocus::List => {
            let rows = picker_rows(w);
            let (mut sc, mut cu) = s.list;
            match key {
                K::ArrowUp | K::ArrowDown => {
                if super::items::list_keys(&mut sc, &mut cu, rows.len(), key) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
                K::ArrowRight | K::ArrowLeft => {
                    if super::items::page_keys(&mut sc, &mut cu, rows.len(), key == K::ArrowRight) {
                        w.sounds.push(Ui::Cursor as u32);
                    }
                }
                K::Enter => {
                    if let Some(r) = rows.get(sc + cu) {
                        let locked = w.inv.locks.get(&(0, r.index)).copied().unwrap_or(0) == 0x19;
                        if locked || picker_tint(w, r, s.on_account) == crate::inventory::TINT_DIM {
                            w.sounds.push(Ui::Error as u32);
                        } else {
                            w.sounds.push(Ui::Select as u32);
                            if r.count > 1 {
                                s.qty = Some((r.index, 1, r.count));
                                s.focus = SFocus::Qty;
                            } else {
                                place(&mut s, r.index, r.id, 1);
                                s.focus = SFocus::Delivery;
                            }
                        }
                    }
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    s.focus = SFocus::Delivery;
                }
                _ => {}
            }
            s.list = (sc, cu);
        }
        SFocus::Qty => {
            let (index, mut v, max) = s.qty.unwrap_or((0, 1, 1));
            match key {
                K::ArrowUp => v = (v + 1).min(max),
                K::ArrowDown => v = v.saturating_sub(1).max(1),
                K::ArrowRight => v = 1,
                K::ArrowLeft => v = max,
                _ => {}
            }
            s.qty = Some((index, v, max));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter => {
                    w.sounds.push(Ui::Select as u32);
                    let item = w.inv.slots.get(&(0, index)).map_or(0, |x| x.0);
                    place(&mut s, index, item, v);
                    (s.qty, s.focus) = (None, SFocus::Delivery);
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    (s.qty, s.focus) = (None, SFocus::List);
                }
                _ => {}
            }
        }
        // moneyctr (FUN_10140bc0): Down / Up by the step, Right a tenth of the step (at 1: 0), Left ten times (past
        // the max: the max); Enter places the gil (inventory slot 0), Escape -1.
        SFocus::Money => {
            let (mut step, max, mut v) = s.money.unwrap_or((1, 0, 0));
            match key {
                K::ArrowDown if v >= step => v -= step,
                K::ArrowUp if v < max => v = (v + step).min(max),
                K::ArrowRight if step > 1 => step /= 10,
                K::ArrowRight => v = 0,
                K::ArrowLeft if max / step.max(1) < 10 => v = max,
                K::ArrowLeft => step *= 10,
                _ => {}
            }
            s.money = Some((step, max, v));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter | K::Escape => {
                    w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
                    let placed = key == K::Enter && v > 0 && place(&mut s, 0, GIL, v);
                    if !placed {
                        s.state = 9;
                    }
                    (s.money, s.focus) = (None, SFocus::Delivery);
                }
                _ => {}
            }
        }
    }
    w.post.send_ui = Some(s);
}

/// stringdl: mode 1 (entry) and 2 (cancel mode); a non-zero result gives the delivery window the focus.
fn stringdl_key(w: &mut World, gu: &GameUi, s: &mut Send, key: egui::Key) {
    use egui::Key as K;
    let d = &mut s.dl;
    if d.editing {
        if matches!(key, K::Enter | K::Escape) {
            d.editing = false;
        }
        return;
    }
    match key {
        // Down from the buttons leaves the window: cancel mode (result 5).
        K::ArrowDown if d.mode == 1 && d.cursor != 1 => d.result = 5,
        K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
            let to = link(gu, STRINGDL, d.cursor, key);
            if to != d.cursor && (1..=3).contains(&to) {
                d.cursor = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter => match (d.mode, d.cursor) {
            (1, 1) => {
                (d.editing, d.caret) = (true, Instant::now());
                w.sounds.push(Ui::Select as u32);
            }
            (1, 2) => {
                d.accepted = d.text.clone();
                d.result = 1;
                w.sounds.push(Ui::Select as u32);
            }
            (1, _) => {
                d.result = 2;
                w.sounds.push(Ui::Select as u32);
            }
            (_, 1) => w.sounds.push(Ui::Error as u32),
            (_, 2) => {
                d.result = 3;
                w.sounds.push(Ui::Select as u32);
            }
            _ => {
                d.result = 4;
                w.sounds.push(Ui::Select as u32);
            }
        },
        K::Escape => {
            d.result = if d.mode == 1 { 2 } else { 4 };
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
    if d.result != 0 {
        s.focus = SFocus::Delivery;
    }
}

/// The name field takes the typed text (15 characters) while editing.
pub fn typing(w: &World) -> bool {
    w.post.send_ui.as_ref().is_some_and(|s| s.focus == SFocus::Stringdl && s.dl.editing)
}

pub fn text_events(w: &mut World, events: &[egui::Event]) {
    let Some(s) = w.post.send_ui.as_mut() else { return };
    let d = &mut s.dl;
    for ev in events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.chars().filter(|c| (' '..='~').contains(c)) {
                    if d.text.chars().count() < 15 {
                        d.text.push(c);
                    }
                }
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                d.text.pop();
            }
            egui::Event::Key { key: egui::Key::Enter | egui::Key::Escape, pressed: true, .. } => d.editing = false,
            _ => {}
        }
    }
}

pub(super) fn link(gu: &GameUi, name: &str, from: u16, key: egui::Key) -> u16 {
    let k = match key {
        egui::Key::ArrowUp => 2,
        egui::Key::ArrowDown => 3,
        egui::Key::ArrowRight => 4,
        _ => 5,
    };
    gu.menu_links(name, from).map_or(from, |l| l[k] as u16)
}

/// Per frame: both state machines, then the latest answer is used up.
pub fn update(w: &mut World) {
    recv_frame(w);
    if w.post.recv_ui.as_ref().is_some_and(|r| r.state == 0x12) {
        w.post.recv_ui = None;
    }
    send_frame(w);
    w.post.last = None;
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    if recv_open(w) {
        recv_key(w, gu, key);
    } else {
        send_key(w, gu, key);
    }
}

/// The logout / shutdown refusal while a UI is up (FUN_1007cf80): group 7 0x54 / 0x55.
pub fn logout_refusal(w: &World) -> Option<usize> {
    if send_open(w) {
        Some(0x54)
    } else if recv_open(w) {
        Some(0x55)
    } else {
        None
    }
}

/// The item info window's item: the cursor slot (post1 / delivery) or the picker's row.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    if let Some(r) = &w.post.recv_ui {
        let s = w.post.recv.get((r.cursor as usize).checked_sub(1)?)?;
        return (s.item != 0 && s.item != GIL).then_some((s.item, Some(s.ext)));
    }
    let s = w.post.send_ui.as_ref()?;
    match s.focus {
        SFocus::List | SFocus::Qty => {
            let r = picker_rows(w).get(s.list.0 + s.list.1)?.clone();
            Some((r.id, w.inv.ext.get(&(0, r.index)).copied()))
        }
        _ => {
            let n = (s.cursor as usize).checked_sub(1).filter(|&n| n < 8)?;
            let x = &w.post.send[n];
            if x.item != 0 && x.item != GIL {
                return Some((x.item, Some(x.ext)));
            }
            let p = s.pending[n]?;
            (p.item != GIL).then(|| (p.item, w.inv.ext.get(&(0, p.index)).copied()))
        }
    }
}

/// The bars: the focused window's title and help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    if let Some(r) = &w.post.recv_ui {
        match r.focus {
            RFocus::Post1 => focus(POST1, r.cursor, bars),
            RFocus::Post2 => focus(POST2, r.post2, bars),
        }
        return;
    }
    let Some(s) = &w.post.send_ui else { return };
    let cancel = matches!(s.state, 0xf | 0x10) || s.dl.mode == 2;
    match s.focus {
        SFocus::Stringdl if !cancel => {
            focus(STRINGDL, s.dl.cursor, bars);
            bars.title[1] = gu.title_msg(0x10).unwrap_or_default();
        }
        SFocus::Delivery | SFocus::Stringdl => {
            bars.title[1] = gu.title_msg(if cancel { 0x11 } else { 0x10 }).unwrap_or_default();
            bars.help[1] = gu.menu_msg(if cancel { 0x35 } else { 0x34 }, "");
        }
        SFocus::List => {
            focus("menu    inventor", s.list.1 as u16 + 1, bars);
            bars.help[1] = gu.menu_msg(3, "");
        }
        SFocus::Qty => {
            bars.help[1] = gu.menu_msg(0x10c, "");
            bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
        }
        SFocus::Money => focus(MONEYCTR, 1, bars),
    }
}

/// The open windows: post1 / post2 or delivery / stringdl, money, the inventory list at (200, 48), itemctrl /
/// moneyctr.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const WHITE: u32 = 0x8080_8080;
    const NAME: u32 = 0x8060_6010;
    let mut rects = Vec::new();
    let Some(info) = w.item_info.clone() else { return rects };
    if let Some(r) = &w.post.recv_ui {
        let focused = |f: RFocus| !r.busy && r.focus == f && w.confirm.is_none();
        rects.extend(draw_list(gu, w, &info, (0, 0), false, None, sw, sh));
        rects.extend(draw_money(gu, w, sw, sh));
        let off: &[u16] = if r.no_return { &[3] } else { &[] };
        if let Some(win) = gu.lobby_window_off(POST2, sw, sh, (0.0, 0.0), focused(RFocus::Post2).then_some(r.post2), focused(RFocus::Post2), off) {
            let [x, y, ..] = win.frame;
            gu.disabled_children(POST2, x, y, off, focused(RFocus::Post2).then_some(r.post2));
            // FUN_1014dab0 / FUN_1014dc80: "Sender:" and the cursor slot's name (FUN_101ca250, cut at 128 px).
            let s = w.post.recv.get((r.cursor as usize).wrapping_sub(1)).filter(|s| matches!(s.stat, 6 | 7 | 0xa..=0xe));
            if let Some(s) = s {
                let label = gu.item_msg(9, 0x33).unwrap_or_default();
                gu.text(&label, x + 10.0, y + 6.0, 0, WHITE.to_le_bytes());
                clipped(gu, &s.name, x + 20.0, y + 30.0, 128.0, NAME);
            }
            rects.push(win.frame);
        }
        if let Some(win) = gu.lobby_window_off(POST1, sw, sh, (0.0, 0.0), focused(RFocus::Post1).then_some(r.cursor), focused(RFocus::Post1), &[]) {
            let slots: Vec<(u16, u32, bool)> = w.post.recv.iter().map(|s| (s.item, s.stack, matches!(s.stat, 6 | 7 | 0xa..=0xe))).collect();
            let cursor = focused(RFocus::Post1).then_some(r.cursor);
            draw_slots(gu, &info, POST1, win.frame, &slots, Some(9), cursor, |_| [1.0, 1.0, 1.0, 0.75]);
            rects.push(win.frame);
        }
        return rects;
    }
    let Some(s) = &w.post.send_ui else { return rects };
    let focused = |f: SFocus| !s.busy && s.focus == f && w.confirm.is_none();
    let rows = picker_rows(w);
    let tints: Vec<u32> = rows.iter().map(|r| picker_tint(w, r, s.on_account)).collect();
    rects.extend(draw_list(gu, w, &info, s.list, focused(SFocus::List), Some(&tints), sw, sh));
    rects.extend(draw_money(gu, w, sw, sh));
    if let Some(win) = gu.lobby_window_off(DELIVERY, sw, sh, (0.0, 0.0), focused(SFocus::Delivery).then_some(s.cursor), focused(SFocus::Delivery), &[]) {
        // FUN_1012e6d0: Stat 1 / 5 / 9 / 0x10 bright, 2 / 3 / 4 / 8 / 0xe / 0xf dim (0x80404040), others not drawn.
        let slots: Vec<(u16, u32, bool)> = (0..8)
            .map(|i| {
                let x = &w.post.send[i];
                (x.item, x.stack, matches!(x.stat, 1..=5 | 8 | 9 | 0xe..=0x10))
            })
            .collect();
        let stats: Vec<u32> = w.post.send.iter().map(|x| x.stat).collect();
        let cursor = focused(SFocus::Delivery).then_some(s.cursor);
        draw_slots(gu, &info, DELIVERY, win.frame, &slots, Some(11), cursor, |i| if matches!(stats[i], 2..=4 | 8 | 0xe | 0xf) { [0.5, 0.5, 0.5, 1.0] } else { [1.0; 4] });
        rects.push(win.frame);
    }
    // stringdl at (16, 144) (FUN_1014ef20); its buttons are drawn by code (FUN_1014f080): mode 1 keytops3 53 / 59,
    // mode 2 56 / 59, the cursor one 0x80805020.
    let dl_focus = focused(SFocus::Stringdl);
    if let Some(win) = gu.lobby_window_off(STRINGDL, sw, sh, (16.0, 144.0), dl_focus.then_some(s.dl.cursor), dl_focus, &[2, 3]) {
        let [x, y, ..] = win.frame;
        let label = gu.item_msg(9, s.dl.label).unwrap_or_default();
        gu.text(&label, x + 10.0, y + 6.0, 0, WHITE.to_le_bytes());
        if let Some(l) = gu.layout(STRINGDL).cloned() {
            for (child, sprite) in [(2u16, if s.dl.mode == 2 { 56 } else { 53 }), (3, 59)] {
                if let Some(e) = l.elements.iter().find(|e| e.id == child) {
                    let tint = if dl_focus && s.dl.cursor == child { 0x8080_5020 } else { WHITE };
                    gu.sprite("menu    keytops3", sprite, x + e.x as f32, y + e.y as f32, tint);
                }
            }
        }
        let (fx, fy) = (x + 20.0, y + 30.0);
        match &s.dl.shown {
            Some(n) => clipped(gu, n, fx, fy, 130.0, NAME),
            // FUN_101c60e0: the live input in white with '|' at the caret, on 19 of every 30 ticks (inf: 1/60 s).
            None if s.dl.editing => {
                let t = (s.dl.caret.elapsed().as_secs_f32() * 60.0) as u32 % 30;
                let caret = if (1..20).contains(&t) { "|" } else { "" };
                gu.text(&format!("{}{caret}", s.dl.text), fx, fy, 0, WHITE.to_le_bytes());
            }
            None => clipped(gu, &s.dl.text, fx, fy, 130.0, NAME),
        }
        rects.push(win.frame);
    }
    if let Some((_, v, max)) = s.qty
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        let [x, y, ..] = win.frame;
        gu.shape_text(&format!("{v:2}"), x + 52.0, y + 22.0, WHITE, true);
        gu.shape_text(&format!("/{max:2}"), x + 58.0, y + 22.0, WHITE, false);
        rects.push(win.frame);
    }
    if let Some((step, max, v)) = s.money
        && let Some(win) = gu.lobby_window_off(MONEYCTR, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        draw_moneyctr(gu, win.frame, step, max, v);
        rects.push(win.frame);
    }
    rects
}

/// FUN_101409f0: the value with "  G" right-aligned at (+0x7f, +0x16), the step's digit in 0x80a06040 (below the step:
/// leading zeros up to it), then "/" and the max at (+0x80 / +0x88, +0x24).
pub(super) fn draw_moneyctr(gu: &mut GameUi, [x, y, ..]: [f32; 4], step: u32, max: u32, v: u32) {
    const WHITE: u32 = 0x8080_8080;
    let mut text = crate::inventory::gil_text(if v < step { (step + v) as i64 } else { v as i64 }).into_bytes();
    if v < step && !text.is_empty() {
        text[0] = b'0';
    }
    let k = step.to_string().len() + (step > 999) as usize + (step > 999_999) as usize;
    let text = String::from_utf8_lossy(&text).into_owned();
    let Some(at) = text.len().checked_sub(3 + k) else { return };
    let (head, rest) = text.split_at(at);
    let (digit, tail) = rest.split_at(1);
    let end = gu.shape_text(tail, x + 127.0, y + 22.0, WHITE, true);
    let end = gu.shape_text(digit, end, y + 22.0, 0x80a0_6040, true);
    gu.shape_text(head, end, y + 22.0, WHITE, true);
    gu.shape_text("/", x + 128.0, y + 36.0, WHITE, false);
    gu.shape_text(&crate::inventory::gil_text(max as i64), x + 136.0, y + 36.0, WHITE, false);
}

/// Game-font text cut to fit `max` px with "..." after it (FUN_101ca250 / FUN_101906a0).
fn clipped(gu: &mut GameUi, s: &str, x: f32, y: f32, max: f32, tint: u32) {
    let mut cut = s.len();
    while cut > 0 && gu.text_width(&s[..cut]) > max {
        cut -= 1;
    }
    gu.text(&s[..cut], x, y, 0, tint.to_le_bytes());
    if cut < s.len() {
        let wd = gu.text_width(&s[..cut]);
        gu.text("...", x + wd, y, 0, tint.to_le_bytes());
    }
}

/// The eight slots (FUN_1015bd70: the 32x32 icon, "%2d" up to 99 right-aligned at (+30, +22) in white for stackable
/// items but gil; the cursor cell shifted by (-1, -1)) and the gil total (FUN_10140910: "N  G" right-aligned at the gil
/// child's right edge + 10, always drawn).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_slots(gu: &mut GameUi, info: &super::items::ItemCache, name: &str, frame: [f32; 4], slots: &[(u16, u32, bool)], gil_child: Option<u16>, cursor: Option<u16>, tint: impl Fn(usize) -> [f32; 4]) {
    let Some(l) = gu.layout(name).cloned() else { return };
    let [x, y, ..] = frame;
    super::items::upload_icons(gu, info, slots.iter().filter(|s| s.2 && s.0 != 0).map(|s| s.0));
    let mut gil = 0u64;
    for (i, &(item, stack, drawn)) in slots.iter().enumerate() {
        if item == 0 || !drawn {
            continue;
        }
        if item == GIL {
            gil += stack as u64;
        }
        let child = i as u16 + 1;
        let Some(e) = l.elements.iter().find(|e| e.id == child) else { continue };
        let d = if cursor == Some(child) { -1.0 } else { 0.0 };
        let (cx, cy) = (x + e.x as f32 + d, y + e.y as f32 + d);
        let icon = crate::render::invwin::icon_texture(item);
        if gu.has_image(&icon) {
            gu.image_rect(&icon, cx, cy, 32.0, 32.0, tint(i));
        }
        let stackable = info.item(item).is_some_and(|it| it.stack > 1);
        if stack > 0 && item != GIL && stackable {
            gu.shape_text(&format!("{:2}", stack.min(99)), cx + 30.0, cy + 22.0, 0x8080_8080, true);
        }
    }
    if let Some(e) = gil_child.and_then(|g| l.elements.iter().find(|e| e.id == g)) {
        let g = crate::inventory::gil_text(gil.min(999_999_999) as i64);
        gu.shape_text(&g, x + e.x as f32 + e.w as f32 + 10.0, y + e.y as f32, 0x8080_8080, true);
    }
}

/// "menu    money": the gil we hold (FUN_101408c0).
pub(super) fn draw_money(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let gil = crate::inventory::gil_text(w.inv.gil() as i64);
    let open = gu.gate_layout("menu    money", sw, sh, None);
    let (b, ui, style, _) = gu.parts();
    if let Some(m) = ui.menu("menu    money").filter(|_| open) {
        crate::render::invwin::push_money(b, ui, style, m, &gil, sw, sh);
    }
    gu.window_rect("menu    money", sw, sh)
}

/// The inventory list moved to x 200 (the source's +4): the picker with its colours, or look-only.
#[allow(clippy::too_many_arguments)]
/// The picker's visible row under the pointer (its list drawn at x 200, draw_rows).
pub(super) fn picker_row(gu: &GameUi, sw: f32, sh: f32, p: (f32, f32)) -> Option<usize> {
    let mut r = gu.window_rect("menu    inventor", sw, sh)?;
    r[0] += 200.0 - gu.window_rect_x("menu    inventor");
    let row = ((p.1 - r[1] - 4.0) / crate::inventory::ROW_PITCH).floor();
    (p.0 >= r[0] && p.0 < r[0] + r[2] && row >= 0.0 && (row as usize) < crate::inventory::VISIBLE_ROWS).then_some(row as usize)
}

pub(super) fn draw_list(gu: &mut GameUi, w: &World, info: &super::items::ItemCache, list: (usize, usize), focused: bool, tints: Option<&[u32]>, sw: f32, sh: f32) -> Option<[f32; 4]> {
    draw_rows(gu, &picker_rows(w), info, list, focused, tints, sw, sh)
}

/// The pickers' bar or wheel (FUN_101f6c70): the post's, the hand-over's, player trade's, synthesis' and its crystals'.
/// True when it moved.
pub(super) fn picker_bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    use super::scrollbar::rows;
    use crate::inventory::VISIBLE_ROWS;
    let (n, crystals, confirm) = (picker_rows(w).len(), super::synth::crystal_count(w), w.confirm.is_some());
    if !confirm && let Some(s) = w.post.send_ui.as_mut().filter(|s| !s.busy && s.focus == SFocus::List) {
        return rows(m, &mut s.list.0, &mut s.list.1, n, VISIBLE_ROWS);
    }
    if let Some(l) = super::trade::picker(w) {
        return rows(m, &mut l.0, &mut l.1, n, VISIBLE_ROWS);
    }
    if let Some((l, cr)) = super::synth::picker(w, confirm) {
        return rows(m, &mut l.0, &mut l.1, if cr { crystals } else { n }, VISIBLE_ROWS);
    }
    false
}

/// The inventory list at x 200 over some rows.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_rows(gu: &mut GameUi, rows: &[crate::inventory::Slot], info: &super::items::ItemCache, (scroll, cursor): (usize, usize), focused: bool, tints: Option<&[u32]>, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let mut built = crate::inventory::build_rows(rows, info, false);
    for (k, (r, s)) in built.iter_mut().zip(rows).enumerate() {
        r.tint = match tints {
            Some(t) => t[k],
            // FUN_1014c690: NoDelivery or equipped grey.
            None => {
                let flags = info.item(s.id).map_or(0, |it| it.flags);
                if flags & 0x2000 != 0 || s.equipped { crate::inventory::TINT_DIM } else { crate::inventory::TINT_WHITE }
            }
        };
    }
    let list = crate::inventory::ItemList { rows: built, scroll, cursor };
    super::items::upload_icons(gu, info, list.visible().iter().map(|r| r.id));
    let frame = gu.cursor_frame();
    let open = gu.gate_layout("menu    inventor", sw, sh, Some(200));
    let (b, ui, style, font) = gu.parts();
    if let Some(m) = ui.menu("menu    inventor").filter(|_| open) {
        let mut m = m.clone();
        if let Some(e) = m.elements.first_mut() {
            e.x = 200;
        }
        let [x, y, h] = crate::render::invwin::push_item_list(b, ui, style, font, GameUi::ATLAS, &m, &list, sw, sh, frame, 3.0, focused, crate::render::invwin::INVENTOR_BAR);
        if focused {
            gu.bar_site(super::scrollbar::Owner::Picker, x, y, h, list.rows.len(), crate::inventory::VISIBLE_ROWS, list.scroll);
        }
    }
    let mut r = gu.window_rect("menu    inventor", sw, sh)?;
    r[0] += 200.0 - gu.window_rect_x("menu    inventor");
    Some(r)
}
