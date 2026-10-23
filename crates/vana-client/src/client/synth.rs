//! Synthesis: "menu    tskill1" with the inventory list as the material picker (FUN_101e0e30), c2s 0x096, the answer
//! s2c 0x06F (a pre-hook that calls the sender's callback, LAB_101e0940), s2c 0x070 (others' results) and the s2c
//! 0x030 animation (FUN_1009f4c0). MODLOG "Synthesis". The history, /lastsynth and "menu    cmbmenu" are not done.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use super::post::{draw_list, draw_slots, link, log_group9, picker_rows, system_line};
use crate::audio::Ui;
use crate::fx::sched::Who;
use std::time::{Duration, Instant};

const TSKILL1: &str = "menu    tskill1";
const ITEMCTRL: &str = "menu    itemctrl";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Focus {
    Slots,
    List,
    Qty,
}

/// tskill1 (DAT_106634ac): +0x14 the request {crystal index / id, ten slots of index, id, count}, its cursor, the
/// picker's scroll / cursor, itemctrl, and the list it came from (+0x4c: the direct-use list or the Items list).
pub struct Win {
    crystal: (u8, u16),
    slots: [(u8, u16, u32); 10],
    cursor: u16,
    focus: Focus,
    list: (usize, usize),
    qty: Option<(u8, u16, u32, u32)>,
    direct: bool,
}

/// trddummy (DAT_106634b4): the answer's lines over 300 ticks, one a frame, and the result item in iteminfo.
struct Shown {
    result: u8,
    grade: i8,
    count: u8,
    item: u16,
    breaks: [u16; 8],
    timer: f32,
    line: usize,
}

/// FUN_1009f4c0's actor state (+0x15a state, +0x158 effect file, +0x15b type, +0x152 timer).
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    file: u32,
    kind: u8,
    timer: f32,
    state: u8,
}

/// The session's request state (+0x19dd4: 0 idle, 2 waiting for 0x06F, 4 answered) since when.
#[derive(Default)]
pub struct State {
    pub win: Option<Win>,
    request: Option<(u8, Instant)>,
    shown: Option<Shown>,
    anims: std::collections::HashMap<u32, Anim>,
    last: Option<Instant>,
}

/// s2c 0x06F / 0x070: Result @4, Grade @5, Count @6, ItemNo @8, BreakNo[8] @0xa; 0x070 the name @0x1e.
#[derive(Clone, Debug)]
pub struct Answer {
    pub result: u8,
    pub grade: i8,
    pub count: u8,
    pub item: u16,
    pub breaks: [u16; 8],
    pub name: String,
}

pub fn active(w: &World) -> bool {
    w.synth.win.is_some()
}

/// FUN_101e0e30: refused silently while the cast bar is up, with group 7 0x8c with no free inventory slot; else the
/// picker (the inventory list at (200, 48)) and tskill1 open, the cursor on child 1.
pub fn open(w: &mut World, index: u8, id: u16) -> bool {
    if w.cast_bar.is_some() {
        return false;
    }
    if !free_slot(w) {
        super::textcmd::refuse(w, 0x8c);
        return false;
    }
    let direct = matches!(w.win, Some(super::items::Win::Items { direct: true, .. }));
    w.win = None;
    w.synth.win = Some(Win { crystal: (index, id), slots: [(0, 0, 0); 10], cursor: 1, focus: Focus::Slots, list: (0, 0), qty: None, direct });
    true
}

/// FUN_100eb800: an empty inventory entry 1..size - 1.
fn free_slot(w: &World) -> bool {
    let size = w.inv.sizes[0] as usize;
    (1..size).any(|i| w.inv.slots.get(&(0, i as u8)).is_none_or(|s| s.0 == 0))
}

/// FUN_101e0f20(1): Cancel goes back to the list it came from.
fn cancel(w: &mut World) {
    let Some(s) = w.synth.win.take() else { return };
    if s.direct {
        super::items::open_direct(w);
    } else {
        super::items::open_items(w);
    }
}

/// FUN_101e0580: linkshells, equipped items and the crystal grey; a stack already in a slot orange.
fn tint(w: &World, s: &Win, row: &crate::inventory::Slot) -> u32 {
    use crate::inventory::{TINT_DIM, TINT_WHITE};
    if (0x201..=0x203).contains(&row.id) || row.equipped || row.index == s.crystal.0 {
        TINT_DIM
    } else if s.slots.iter().any(|x| x.0 == row.index && x.0 != 0) {
        0x8080_4020
    } else {
        let _ = w;
        TINT_WHITE
    }
}

/// FUN_101e0d30: the cursor slot takes the stack; the same stack elsewhere moves here; the same stack on this slot
/// is taken out.
fn place(s: &mut Win, index: u8, id: u16, count: u32) {
    let slot = (s.cursor as usize).saturating_sub(1).min(9);
    if let Some(k) = s.slots.iter().position(|x| x.0 == index && x.1 == id && x.0 != 0) {
        s.slots[k] = (0, 0, 0);
        if k == slot {
            return;
        }
    }
    s.slots[slot] = (index, id, count);
}

/// FUN_101e1f40: the crystal and the slots up to the first empty one are still in the inventory, unlocked, with the
/// counts.
fn still_held(w: &World, s: &Win) -> bool {
    let held = |index: u8, id: u16, count: u32| {
        w.inv.slots.get(&(0, index)).is_some_and(|e| (id == 0 || e.0 == id) && e.1 != 0 && e.1 >= count) && w.inv.locks.get(&(0, index)).copied().unwrap_or(0) == 0
    };
    if s.crystal.0 == 0 || !held(s.crystal.0, s.crystal.1, 1) {
        return false;
    }
    for &(index, id, count) in &s.slots {
        if index == 0 {
            break;
        }
        if id == 0 || !held(index, id, count) {
            return false;
        }
    }
    true
}

/// FUN_100e4240's HashNo: the crystal (cluster ids folded), the unit count and the first (lowest) item.
fn hash(crystal: u16, items: u8, first: u16) -> u8 {
    let c = match crystal {
        0x196a..=0x1971 => crystal - 0x96a,
        0x108e..=0x1095 => crystal - 0x8e,
        c => c,
    } as u64;
    ((items as u64 + 5) * (first as u64 + 7) * (c + 3) % 0x7f) as u8
}

/// The c2s 0x096 body (0x20 bytes): HashNo, crystal id / index, the units sorted ascending by item id with their
/// inventory indexes.
fn body(crystal: (u8, u16), units: &mut [(u16, u8)]) -> Vec<u8> {
    units.sort_by_key(|u| u.0);
    let mut b = vec![0u8; 0x20];
    b[0] = hash(crystal.1, units.len() as u8, units.first().map_or(0, |u| u.0));
    b[2..4].copy_from_slice(&crystal.1.to_le_bytes());
    (b[4], b[5]) = (crystal.0, units.len() as u8);
    for (i, u) in units.iter().enumerate() {
        b[6 + 2 * i..8 + 2 * i].copy_from_slice(&u.0.to_le_bytes());
        b[0x16 + i] = u.1;
    }
    b
}

/// FUN_101e0770: the units (more than 8: group 9 0xf; none: group 7 0x63), a free slot (0x8c), not healing (0x51), no
/// request in flight (0x64); then c2s 0x096 and the request waits.
fn send(w: &mut World, crystal: (u8, u16), slots: &[(u8, u16, u32); 10]) -> bool {
    let mut units: Vec<(u16, u8)> = Vec::new();
    for &(index, id, count) in slots.iter().filter(|s| s.0 != 0 && s.2 != 0) {
        for _ in 0..count {
            units.push((id, index));
        }
    }
    if units.len() > 8 {
        log_group9(w, 0xf, 0x79);
        return false;
    }
    let refusal = if units.is_empty() {
        Some(0x63)
    } else if !free_slot(w) {
        Some(0x8c)
    } else if matches!(w.my_motion.status, 0x21 | 7) {
        Some(0x51)
    } else if w.synth.request.is_some() {
        Some(0x64)
    } else {
        None
    };
    if let Some(id) = refusal {
        super::textcmd::refuse(w, id);
        return false;
    }
    let b = body(crystal, &mut units);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x096, body: b });
    w.synth.request = Some((2, Instant::now()));
    true
}

/// tskill1 (FUN_101e1530), the picker (FUN_101e0600) and itemctrl.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut s) = w.synth.win.take() else { return };
    match s.focus {
        Focus::Slots => match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = link(gu, TSKILL1, s.cursor, key);
                if to != s.cursor && to != 0 {
                    s.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            // FUN_101e08e0: the gate (0x77), the inventory check (silent), the send; then everything closes.
            K::Enter if s.cursor == 9 => {
                let ok = super::textcmd::status_gate(w, 0x77) && still_held(w, &s) && send(w, s.crystal, &s.slots);
                if ok {
                    w.sounds.push(Ui::Select as u32);
                    return;
                }
                w.sounds.push(Ui::Error as u32);
            }
            K::Enter if s.cursor == 10 => {
                w.sounds.push(Ui::Select as u32);
                w.synth.win = Some(s);
                cancel(w);
                return;
            }
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                s.focus = Focus::List;
            }
            // Escape only moves the cursor to Cancel (FUN_10118db0(10, 1)).
            K::Escape => {
                s.cursor = 10;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        },
        Focus::List => {
            let rows = picker_rows(w);
            let (mut sc, mut cu) = s.list;
            match key {
                K::ArrowUp | K::ArrowDown => super::items::list_keys(&mut sc, &mut cu, rows.len(), key),
                K::ArrowRight | K::ArrowLeft => {
                    if super::items::page_keys(&mut sc, &mut cu, rows.len(), key == K::ArrowRight) {
                        w.sounds.push(Ui::Cursor as u32);
                    }
                }
                K::Enter => {
                    if let Some(r) = rows.get(sc + cu) {
                        let locked = w.inv.locks.get(&(0, r.index)).copied().unwrap_or(0) == 0x19;
                        if locked || tint(w, &s, r) == crate::inventory::TINT_DIM {
                            w.sounds.push(Ui::Error as u32);
                        } else {
                            w.sounds.push(Ui::Select as u32);
                            if r.count > 1 {
                                (s.qty, s.focus) = (Some((r.index, r.id, 1, r.count)), Focus::Qty);
                            } else {
                                place(&mut s, r.index, r.id, 1);
                                s.focus = Focus::Slots;
                            }
                        }
                    }
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    s.focus = Focus::Slots;
                }
                _ => {}
            }
            s.list = (sc, cu);
        }
        Focus::Qty => {
            let (index, id, mut v, max) = s.qty.unwrap_or((0, 0, 1, 1));
            match key {
                K::ArrowUp => v = (v + 1).min(max),
                K::ArrowDown => v = v.saturating_sub(1).max(1),
                K::ArrowRight => v = 1,
                K::ArrowLeft => v = max,
                _ => {}
            }
            s.qty = Some((index, id, v, max));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter => {
                    w.sounds.push(Ui::Select as u32);
                    place(&mut s, index, id, v);
                    (s.qty, s.focus) = (None, Focus::Slots);
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    (s.qty, s.focus) = (None, Focus::List);
                }
                _ => {}
            }
        }
    }
    w.synth.win = Some(s);
}

/// s2c 0x06F: the waiting request's callback (LAB_101e0940) by Result, then the request is answered.
pub fn on_result(w: &mut World, a: Answer) {
    if !matches!(w.synth.request, Some((2, _))) {
        return;
    }
    w.synth.request = Some((4, Instant::now()));
    let line9 = |w: &mut World, id| log_group9(w, id, 0x79);
    match a.result {
        0 | 1 | 0xc => {
            if a.result == 1 {
                line9(w, 0xa);
            }
            w.synth.shown = Some(Shown { result: a.result, grade: a.grade, count: a.count, item: a.item, breaks: a.breaks, timer: 300.0, line: 0 });
        }
        2 | 0xe => line9(w, 0xc),
        3 => line9(w, 9),
        4 => line9(w, 0xe),
        6 => line9(w, 0xd),
        7 => line9(w, 0x10),
        0xd => super::textcmd::refuse(w, 0x64),
        _ => line9(w, 0xb),
    }
}

/// s2c 0x070 (LAB_100e3f70): file 7031 0xc8 + Grade / 0xcc per lost item / 0xcd, with the name in string slot 2.
pub fn on_others(w: &mut World, a: Answer) {
    match a.result {
        0 => system_line(w, 0xc8 + a.grade.clamp(0, 3) as u16, [a.item as i32, a.count as i32, 0, 0, 0], a.name),
        1 => {
            for b in a.breaks.into_iter().filter(|b| *b != 0) {
                system_line(w, 0xcc, [b as i32, 0, 0, 0, 0], a.name.clone());
            }
        }
        0xc => system_line(w, 0xcd, [a.item as i32, a.count as i32, 0, 0, 0], a.name),
        _ => {}
    }
}

/// s2c 0x030 (FUN_1009f990 / FUN_1009f430): the actor (id checked) starts the animation; its status takes Status.
pub fn on_effect(w: &mut World, id: u32, index: u16, effect: i16, kind: i8, status: i8, timer: u16) {
    let known = if id == w.my_id { index == w.my_index } else { w.entities.get(&id).is_some_and(|e| e.index == index) };
    if !known {
        return;
    }
    let file = if effect < 100 { (0x1330 + effect as i32) as u32 } else { (effect as i32 - 0x235b) as u32 };
    w.synth.anims.insert(id, Anim { file, kind: kind.clamp(0, 2) as u8, timer: timer as f32, state: 0 });
    if id == w.my_id {
        w.my_motion.status = status as u8;
    } else if let Some(e) = w.entities.get_mut(&id) {
        e.motion.status = status as u8;
    }
}

const ENDS: [&[u8; 4]; 3] = [b"ednn", b"edmm", b"edxx"];

/// Per frame: the request state (2 waits 30 s, 4 clears after 2 s), trddummy's lines and the animations.
pub fn update(w: &mut World) {
    let now = Instant::now();
    let ticks = w.synth.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32() * 60.0);
    w.synth.last = Some(now);
    w.synth.request = match w.synth.request {
        Some((2, t)) if t.elapsed() > Duration::from_secs(30) => None,
        Some((4, t)) if t.elapsed() > Duration::from_secs(2) => None,
        r => r,
    };
    shown_frame(w, ticks);
    let ids: Vec<u32> = w.synth.anims.keys().copied().collect();
    for id in ids {
        anim_frame(w, id, ticks);
    }
}

/// FUN_101e1850: Result 0 one line 0xa0 + Grade, 0xc 0xa9, 1 one 0xa4 a frame per lost item; closed when the timer
/// runs out and the lines are done.
fn shown_frame(w: &mut World, ticks: f32) {
    let Some(mut s) = w.synth.shown.take() else { return };
    s.timer -= ticks;
    let done = match s.result {
        1 => s.line >= 8,
        _ => s.line >= 1,
    };
    if !done {
        match s.result {
            0 => system_line(w, 0xa0 + s.grade.clamp(0, 3) as u16, [s.item as i32, s.count as i32, 0, 0, 0], String::new()),
            0xc => system_line(w, 0xa9, [s.item as i32, s.count as i32, 0, 0, 0], String::new()),
            _ => {
                if s.breaks[s.line] != 0 {
                    system_line(w, 0xa4, [s.breaks[s.line] as i32, 0, 0, 0, 0], String::new());
                }
            }
        }
        s.line += 1;
    }
    if !(s.timer < 0.0 && done) {
        w.synth.shown = Some(s);
    }
}

/// FUN_1009f4c0: res0, then the effect file's frst, its end (ednn / edmm / edxx by type), res2, and for us c2s 0x059
/// {0}; our status leaving 0x2c during frst plays stcr and res2 and sends {1}.
fn anim_frame(w: &mut World, id: u32, ticks: f32) {
    let Some(mut a) = w.synth.anims.get(&id).copied() else { return };
    let Some(ch) = w.actors.get(&id).and_then(|x| x.character.clone()) else { return };
    let me = Who::Actor(id);
    let fourcc = |s: &[u8; 4]| u32::from_le_bytes(*s);
    let frst = fourcc(b"frst");
    let end = fourcc(ENDS[a.kind as usize]);
    let status = if id == w.my_id { w.my_motion.status } else { w.entities.get(&id).map_or(0, |e| e.motion.status) };
    let motion = |w: &mut World, name: &str| {
        let base = super::actmotion::status_motion(w, id, &ch);
        if let Some(x) = w.actors.get_mut(&id) {
            super::actmotion::start(x, &ch, name, &base);
        }
    };
    let finish = |w: &mut World, para: u32| {
        if id == w.my_id {
            let mut b = vec![0u8; 12];
            b[0..4].copy_from_slice(&para.to_le_bytes());
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x059, body: b });
        }
        w.synth.anims.remove(&id);
    };
    match a.state {
        0 => {
            motion(w, "res0");
            a.state = 10;
        }
        10 => {
            let busy = w.actors.get(&id).and_then(|x| x.play.as_ref()).is_some_and(|p| p.name == "res0" && p.busy());
            if !busy {
                a.state = 0xb;
            }
        }
        0xb => {
            w.effects.on_event_scheduler(a.file, frst, me, me);
            a.state = 1;
        }
        1 => {
            let running = w.effects.event_scheduler_running(a.file, frst, me, me);
            if !running {
                w.effects.on_event_scheduler(a.file, end, me, me);
                (a.timer, a.state) = (0.0, 2);
            } else if status != 0x2c {
                w.effects.stop_event_scheduler(a.file, frst, me, me);
                w.effects.on_event_scheduler(a.file, fourcc(b"stcr"), me, me);
                motion(w, "res2");
                a.state = 4;
            } else if a.timer > 0.0 {
                a.timer -= ticks;
                if a.timer <= 0.0 {
                    w.effects.stop_event_scheduler(a.file, frst, me, me);
                    w.effects.on_event_scheduler(a.file, end, me, me);
                    (a.timer, a.state) = (0.0, 2);
                }
            }
        }
        2 => {
            if !w.effects.event_scheduler_running(a.file, end, me, me) {
                motion(w, "res2");
                a.state = 3;
            }
        }
        3 => return finish(w, 0),
        4 => return finish(w, 1),
        _ => {}
    }
    w.synth.anims.insert(id, a);
}

/// The bars: tskill1's child strings, the picker's help (0xe 7), itemctrl's.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.synth.win else { return };
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    match s.focus {
        Focus::Slots => focus(TSKILL1, s.cursor, bars),
        Focus::List => {
            focus("menu    inventor", s.list.1 as u16 + 1, bars);
            bars.help[1] = gu.menu_msg(7, "");
        }
        Focus::Qty => {
            bars.help[1] = gu.menu_msg(0x10c, "");
            bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
        }
    }
}

/// The item info window's item: the cursor slot's stack, the picker's row, or the result.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    if let Some(s) = &w.synth.win {
        let index = match s.focus {
            Focus::List | Focus::Qty => picker_rows(w).get(s.list.0 + s.list.1)?.index,
            Focus::Slots => s.slots.get((s.cursor as usize).checked_sub(1)?)?.0,
        };
        let &(id, _) = w.inv.slots.get(&(0, index)).filter(|_| index != 0)?;
        return Some((id, w.inv.ext.get(&(0, index)).copied()));
    }
    w.synth.shown.as_ref().filter(|s| matches!(s.result, 0 | 0xc) && s.item != 0).map(|s| (s.item, None))
}

/// tskill1 (FUN_101e12e0): the crystal at half size at (+0x12, +4) and its element (keytops3 0xb9 + element) at
/// (+0x24, +4), the slots' icons and counts; the picker list at (200, 48) and itemctrl.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(s), Some(info)) = (&w.synth.win, w.item_info.clone()) else { return rects };
    let focused = |f: Focus| s.focus == f && w.confirm.is_none();
    let rows = picker_rows(w);
    let tints: Vec<u32> = rows.iter().map(|r| tint(w, s, r)).collect();
    rects.extend(draw_list(gu, w, &info, s.list, focused(Focus::List), Some(&tints), sw, sh));
    if let Some(win) = gu.lobby_window_off(TSKILL1, sw, sh, (0.0, 0.0), focused(Focus::Slots).then_some(s.cursor), focused(Focus::Slots), &[]) {
        let [x, y, ..] = win.frame;
        let element = match s.crystal.1 {
            0x1000..=0x1007 => Some(s.crystal.1 - 0x1000),
            0x108e..=0x1095 => Some(s.crystal.1 - 0x108e),
            0x196a..=0x1971 => Some(s.crystal.1 - 0x196a),
            _ => None,
        };
        if let Some(e) = element {
            gu.sprite("menu    keytops3", 0xb9 + e as usize, x + 36.0, y + 4.0, 0x8080_8080);
            super::items::upload_icons(gu, &info, std::iter::once(s.crystal.1));
            let icon = crate::render::invwin::icon_texture(s.crystal.1);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, x + 18.0, y + 4.0, 16.0, 16.0, [1.0; 4]);
            }
        }
        let slots: Vec<(u16, u32, bool)> = s.slots[..8].iter().map(|x| (x.1, x.2, x.0 != 0)).collect();
        draw_slots(gu, &info, TSKILL1, win.frame, &slots, None, focused(Focus::Slots).then_some(s.cursor), |_| [1.0; 4]);
        rects.push(win.frame);
    }
    if let Some((_, _, v, max)) = s.qty
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        let [x, y, ..] = win.frame;
        gu.shape_text(&format!("{v:2}"), x + 52.0, y + 22.0, 0x8080_8080, true);
        gu.shape_text(&format!("/{max:2}"), x + 58.0, y + 22.0, 0x8080_8080, false);
        rects.push(win.frame);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The units sort ascending by id (indexes with them); HashNo from the lowest id.
    #[test]
    fn request_body() {
        let mut units = vec![(0x0284, 7), (0x0284, 7), (0x0001, 3)];
        let b = body((2, 0x1000), &mut units);
        assert_eq!(&b[2..6], &[0x00, 0x10, 2, 3]);
        assert_eq!(&b[6..12], &[0x01, 0x00, 0x84, 0x02, 0x84, 0x02]);
        assert_eq!(&b[0x16..0x19], &[3, 7, 7]);
        assert_eq!(b[0], ((3u64 + 5) * (1 + 7) * (0x1000 + 3) % 0x7f) as u8);
    }
}
