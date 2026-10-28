//! Puppetmaster automaton customization (pup_automaton_spec.md): pupequip's "Automaton" row (FUN_10207670) opens
//! statpup, automato (head, frame, 12 attachments), BLU's bluinven / bluehelp fed by their own rows, and pupsibor
//! (Refine by Type), sending c2s 0x102.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use std::time::Instant;

pub(super) const AUTO: &str = "menu    automato";
const STAT: &str = "menu    statpup";
const SIBO: &str = "menu    pupsibor";
const INVEN: &str = "menu    bluinven";
const HELP: &str = "menu    bluehelp";
const VISIBLE: usize = 10;
const WHITE: u32 = 0x8080_8080;
const PICKED: u32 = 0x8080_4020;
const SET: u32 = 0x8050_9050;
const GREY: u32 = 0x8040_4040;
const RED: u32 = 0x8080_4040;
const GREEN: u32 = 0x8040_8040;
const REMOVE_ALL: u32 = 0x8090_9040;
/// DAT_10385530: automato's child of slot 0 (head), 1 (frame), 2..13 (the attachments, row by row).
const SLOT_CHILD: [u16; 14] = [1, 5, 2, 6, 9, 12, 3, 7, 10, 13, 4, 8, 11, 14];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Grid,
    List,
}

/// The list's mode (source +0x1e): what automato's Enter on a slot asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    All,
    Heads,
    Frames,
    Attachments,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// framesus 129 "Element ▶": pupsibor.
    Element,
    /// group 9 0xe3 "(Remove All)".
    RemoveAll,
    Item(u16),
}

#[derive(Clone, Debug)]
pub struct Screen {
    /// automato +0x1c: 0 main PUP, 1 sub PUP.
    side: usize,
    focus: Focus,
    /// automato's cursor child.
    grid: u16,
    list: (usize, usize),
    /// automato +0x14: a slot picked first (then an item).
    slot_pick: Option<usize>,
    /// source +0x10 / +0x14: an item picked first (then a slot), and its kind.
    pick: Option<(u16, u16)>,
    mode: Mode,
    /// source +0x1c: the attachment kind shown (0 all, 2..9 by element).
    filter: u16,
    /// pupsibor's cursor row while it is open.
    sibo: Option<u16>,
}

/// FUN_100fcbc0(side, 0x1200, i): slot i's byte (0 head, 1 frame, 2..13 attachments), 0 without a PUP block.
fn slot(w: &World, side: usize, i: usize) -> u8 {
    let b = &w.ext_job[side];
    if b[0] == 0x12 && i < 0x10 { b[4 + i] } else { 0 }
}

/// FUN_100fcc50: bit n of field 1 (heads / frames, +0x14) or 2 (attachments, +0x34).
fn owned(w: &World, side: usize, field: usize, n: u16) -> bool {
    let b = &w.ext_job[side];
    let at = if field == 1 { 0x14 } else { 0x34 };
    b[0] == 0x12 && n < 0x100 && b[at + (n >> 3) as usize] >> (n & 7) & 1 != 0
}

/// FUN_100fcdd0: the first PUP block (main, then sub).
fn status_block(w: &World) -> Option<&[u8; 0x9c]> {
    w.ext_job.iter().find(|b| b[0] == 0x12)
}

/// The block's automaton name (+0x54, NUL-terminated).
fn auto_name(b: &[u8]) -> String {
    let n = &b[0x54..0x64];
    crate::dat::font::decode_sjis(&n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())])
}

/// An automaton item's kind (0 head, 1 frame, 2..9 attachment element + 2) and element nibbles (record +0xc /
/// +0x14; ours `resource` / `jobs`).
fn rec(w: &World, id: u16) -> Option<(u16, u32)> {
    let it = w.item_info.as_ref()?.item(id)?;
    Some((it.resource, it.jobs))
}

fn nibble(n: u32, e: usize) -> i32 {
    (n >> (4 * e) & 0xf) as i32
}

/// The item a slot's byte stands for.
fn slot_item(i: usize, b: u8) -> Option<u16> {
    (b != 0).then(|| if i < 2 { 0x2000 + b as u16 } else { 0x2100 + b as u16 })
}

/// automato's per-frame sums (FUN_10207a40): used[e] over the attachments, capacity[e] from the head, the frame and
/// the block's bonus (& 0xf).
fn capacity(w: &World, side: usize) -> ([i32; 8], [i32; 8]) {
    let (mut used, mut cap) = ([0; 8], [0; 8]);
    let bonus = (w.ext_job[side][0x98] & 0xf) as i32;
    for i in 0..14 {
        let Some((_, n)) = slot_item(i, slot(w, side, i)).and_then(|id| rec(w, id)) else { continue };
        let to = if i < 2 { &mut cap } else { &mut used };
        for (e, v) in to.iter_mut().enumerate() {
            *v += nibble(n, e);
        }
    }
    cap.iter_mut().for_each(|c| *c += bonus);
    (used, cap)
}

fn slot_of(child: u16) -> Option<usize> {
    SLOT_CHILD.iter().position(|&c| c == child)
}

/// 7.2: an attachment fits what is free, the cursor slot's attachment given back while a slot is picked.
fn fits(w: &World, s: &Screen, nibbles: u32) -> bool {
    let (used, cap) = capacity(w, s.side);
    let mut free: Vec<i32> = cap.iter().zip(used).map(|(c, u)| c - u).collect();
    if s.slot_pick.is_some()
        && let Some(i) = slot_of(s.grid).filter(|&i| i >= 2)
        && let Some((_, back)) = slot_item(i, slot(w, s.side, i)).and_then(|id| rec(w, id))
    {
        free.iter_mut().enumerate().for_each(|(e, f)| *f += nibble(back, e));
    }
    nibbles != 0 && (0..8).all(|e| nibble(nibbles, e) == 0 || nibble(nibbles, e) <= free[e])
}

/// FUN_10208b60: the rows by mode; heads and frames owned (0x1201), Element and Remove All, the owned attachments of
/// the filter, each in id order.
fn rows(w: &World, s: &Screen) -> Vec<Row> {
    let mut out = Vec::new();
    for kind in [0u16, 1] {
        if matches!((s.mode, kind), (Mode::All, _) | (Mode::Heads, 0) | (Mode::Frames, 1)) {
            out.extend((1..0x100).filter(|&n| owned(w, s.side, 1, n) && rec(w, 0x2000 + n).is_some_and(|r| r.0 == kind)).map(|n| Row::Item(0x2000 + n)));
        }
    }
    if matches!(s.mode, Mode::All | Mode::Attachments) {
        out.extend([Row::Element, Row::RemoveAll]);
        out.extend((1..0x100).filter(|&n| owned(w, s.side, 2, n) && rec(w, 0x2100 + n).is_some_and(|r| r.0 >= 2 && (s.filter == 0 || r.0 == s.filter))).map(|n| Row::Item(0x2100 + n)));
    }
    out
}

/// A row's name colour (FUN_102086d0 / FUN_102088b0).
fn tint(w: &World, s: &Screen, id: u16) -> u32 {
    let picked = s.pick.is_some_and(|p| p.0 == id);
    if id < 0x2100 {
        let kind = rec(w, id).map_or(0, |r| r.0) as usize;
        let cur = slot(w, s.side, kind.min(1));
        return if cur as u16 + 0x2000 == id {
            SET
        } else if picked {
            PICKED
        } else {
            WHITE
        };
    }
    let n = (id - 0x2100) as u8;
    if picked {
        PICKED
    } else if (2..14).any(|i| slot(w, s.side, i) == n) {
        SET
    } else if !rec(w, id).is_some_and(|r| fits(w, s, r.1)) {
        GREY
    } else {
        WHITE
    }
}

/// FUN_101cc400: our pet is the automaton (its shown name is the block's).
fn activated(w: &World, side: usize) -> bool {
    let name = auto_name(&w.ext_job[side]);
    w.my_pet != 0 && w.entities.values().any(|e| e.index == w.my_pet && !name.is_empty() && super::monstrosity::name(e) == name)
}

/// pupequip row 2 (FUN_10207670): 0x119 seen and PUP main or sub, else group 7 0x57. automato opens last with the
/// cursor it last had.
pub fn open(w: &mut World) {
    let side = w.stats.as_ref().and_then(|st| if st.main.0 == 0x12 { Some(0) } else if st.sub.0 == 0x12 { Some(1) } else { None });
    let Some(side) = side.filter(|_| w.recasts_seen) else {
        super::textcmd::refuse(w, 0x57);
        return;
    };
    let grid = Some(w.cursor_recall(AUTO)).filter(|c| (1..=14).contains(c)).unwrap_or(1);
    w.pupset = Some(Screen { side, focus: Focus::Grid, grid, list: (0, 0), slot_pick: None, pick: None, mode: Mode::All, filter: 0, sibo: None });
}

/// The cursors kept by the windows (FUN_10118830): automato's, pupsibor's while open.
pub(super) fn kept(s: &Screen) -> Vec<(&'static str, u16)> {
    let mut v = vec![(AUTO, s.grid)];
    v.extend(s.sibo.map(|r| (SIBO, r)));
    v
}

/// FUN_100fcf80: c2s 0x102 {u8 value @4, the block's first dword @8, the slot's byte @0xc + slot}: a set puts the
/// value in the slot, a removal (0) names the slot's current byte. Nothing when the byte is already the value, nor
/// within a second of the last 0x102 (FUN_100fc3c0).
fn send(w: &mut World, side: usize, i: usize, value: u8) {
    let cur = slot(w, side, i);
    if w.ext_job[side][0] != 0x12 || i > 0xf || cur == value {
        return;
    }
    let mut body = vec![0u8; 0xa0];
    body[0] = value;
    body[4..8].copy_from_slice(&w.ext_job[side][0..4]);
    body[8 + i] = if value == 0 { cur } else { value };
    send_body(w, body);
}

fn send_body(w: &mut World, body: Vec<u8>) {
    if w.ext_job_sent.is_some_and(|t| t.elapsed() < super::bluset::THROTTLE) {
        return;
    }
    log::info!("automaton: c2s 0x102 value {} slots {:?}", body[0], &body[8..0x16]);
    w.ext_job_sent = Some(Instant::now());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x102, body });
}

/// The Remove All prompt's Yes (FUN_100fd100): value 0, the 12 attachments (and the two bytes after) copied, head and
/// frame 0; sent even when empty.
pub fn remove_all(w: &mut World) {
    let Some(side) = w.pupset.as_ref().map(|s| s.side) else { return };
    let b = w.ext_job[side];
    if b[0] != 0x12 {
        return;
    }
    let mut body = vec![0u8; 0xa0];
    body[4..8].copy_from_slice(&b[0..4]);
    body[0xa..0x18].copy_from_slice(&b[6..0x14]);
    send_body(w, body);
}

/// The generic list: Up / Down stop at the ends, Left / Right a page of 10. True when it moved.
fn step(c: &mut (usize, usize), n: usize, key: egui::Key) -> bool {
    use egui::Key as K;
    let old = c.0;
    match key {
        K::ArrowDown if c.0 + 1 < n => c.0 += 1,
        K::ArrowUp if c.0 > 0 => c.0 -= 1,
        K::ArrowRight => c.0 = (c.0 + VISIBLE).min(n.saturating_sub(1)),
        K::ArrowLeft => c.0 = c.0.saturating_sub(VISIBLE),
        _ => {}
    }
    c.1 = c.1.min(c.0).max((c.0 + 1).saturating_sub(VISIBLE));
    c.0 != old
}

fn list_to(s: &mut Screen, r: usize) {
    s.list.0 = r;
    s.list.1 = s.list.1.min(r).max((r + 1).saturating_sub(VISIBLE));
}

/// The list's cursor kept on its rows after they change.
fn clamp_list(w: &World, s: &mut Screen) {
    let n = rows(w, s).len();
    s.list.0 = s.list.0.min(n.saturating_sub(1));
    s.list.1 = s.list.1.min(s.list.0);
}

/// 4.1: a picked head or frame holds the cursor on its cell; a picked attachment keeps it off them.
fn hold_grid(s: &mut Screen) {
    match s.pick.map(|p| p.1) {
        Some(0) => s.grid = 1,
        Some(1) => s.grid = 5,
        Some(_) if matches!(s.grid, 1 | 5) => s.grid = 2,
        _ => {}
    }
}

/// automato (FUN_10207e80), the list (FUN_10209000) and pupsibor (FUN_102084c0).
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut s) = w.pupset.take() else { return };
    let keep = keys(w, gu, &mut s, key);
    if keep && w.pupset.is_none() {
        w.pupset = Some(s);
    }
}

fn refuse_activated(w: &mut World, s: &Screen) -> bool {
    if activated(w, s.side) {
        super::textcmd::refuse(w, 0xb8);
        w.sounds.push(Ui::Error as u32);
        return true;
    }
    false
}

fn keys(w: &mut World, gu: &GameUi, s: &mut Screen, key: egui::Key) -> bool {
    use egui::Key as K;
    if let Some(r) = s.sibo {
        match key {
            K::ArrowDown | K::ArrowUp => {
                s.sibo = Some(if key == K::ArrowDown { r % 8 + 1 } else if r <= 1 { 8 } else { r - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                (s.filter, s.sibo) = (r + 1, None);
                clamp_list(w, s);
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                s.sibo = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return true;
    }
    match (s.focus, key) {
        (Focus::Grid, K::Escape) => match (s.pick, s.slot_pick) {
            (Some(_), _) => {
                (s.pick, s.mode, s.focus) = (None, Mode::All, Focus::List);
                clamp_list(w, s);
                w.sounds.push(Ui::Cancel as u32);
            }
            (None, Some(_)) => {}
            (None, None) => {
                w.sounds.push(Ui::Cancel as u32);
                return false;
            }
        },
        (Focus::List, K::Escape) => match (s.slot_pick, s.pick) {
            (Some(_), _) => {
                (s.slot_pick, s.mode, s.focus) = (None, Mode::All, Focus::Grid);
                clamp_list(w, s);
                w.sounds.push(Ui::Cancel as u32);
            }
            (None, Some(_)) => {}
            (None, None) => {
                w.sounds.push(Ui::Cancel as u32);
                return false;
            }
        },
        (Focus::Grid, K::Enter) => {
            let Some(i) = slot_of(s.grid) else { return true };
            let cur = slot(w, s.side, i);
            if let Some((item, kind)) = s.pick.take() {
                // Flow B's second step: the head / frame unless it is already on; an attachment set, or removed when
                // the slot holds it.
                if kind < 2 {
                    if cur as u16 + 0x2000 == item {
                        w.sounds.push(Ui::Error as u32);
                    } else {
                        w.sounds.push(Ui::Select as u32);
                        send(w, s.side, i, (item - 0x2000) as u8);
                    }
                } else {
                    let n = (item - 0x2100) as u8;
                    w.sounds.push(Ui::Select as u32);
                    send(w, s.side, i, if cur == n { 0 } else { n });
                }
                (s.mode, s.focus) = (Mode::All, Focus::List);
                clamp_list(w, s);
                w.sounds.push(Ui::Select as u32);
            } else {
                // Flow A: the slot first; the list shows what fits it, on the slot's item.
                if refuse_activated(w, s) {
                    return true;
                }
                s.slot_pick = Some(i);
                s.mode = match i {
                    0 => Mode::Heads,
                    1 => Mode::Frames,
                    _ => Mode::Attachments,
                };
                clamp_list(w, s);
                if let Some(id) = slot_item(i, cur)
                    && let Some(r) = rows(w, s).iter().position(|r| *r == Row::Item(id))
                {
                    list_to(s, r);
                }
                s.focus = Focus::List;
                w.sounds.push(Ui::Select as u32);
            }
        }
        (Focus::List, K::Enter) => {
            let rows = rows(w, s);
            match rows.get(s.list.0).copied() {
                Some(Row::Element) => {
                    (s.filter, s.sibo) = (0, Some(w.cursor_recall(SIBO).clamp(1, 8)));
                    w.sounds.push(Ui::Select as u32);
                }
                Some(Row::RemoveAll) => {
                    w.confirm = Some((super::Confirm::PupRemoveAll, false));
                    w.sounds.push(Ui::Select as u32);
                }
                Some(Row::Item(id)) if tint(w, s, id) != GREY => {
                    let kind = rec(w, id).map_or(0, |r| r.0);
                    if let Some(i) = s.slot_pick.take() {
                        // Flow A's second step.
                        let cur = slot(w, s.side, i);
                        if kind < 2 {
                            if cur as u16 + 0x2000 == id {
                                w.sounds.push(Ui::Error as u32);
                            } else {
                                w.sounds.push(Ui::Select as u32);
                                send(w, s.side, i, (id - 0x2000) as u8);
                            }
                        } else {
                            let n = (id - 0x2100) as u8;
                            w.sounds.push(Ui::Select as u32);
                            send(w, s.side, i, if cur == n { 0 } else { n });
                        }
                        (s.mode, s.focus) = (Mode::All, Focus::Grid);
                        clamp_list(w, s);
                    } else {
                        // Flow B: the item first; automato's cursor on the slot holding the attachment.
                        if refuse_activated(w, s) {
                            return true;
                        }
                        s.pick = Some((id, kind));
                        if id >= 0x2100
                            && let Some(i) = (2..14).find(|&i| slot(w, s.side, i) as u16 + 0x2100 == id)
                        {
                            s.grid = SLOT_CHILD[i];
                        }
                        hold_grid(s);
                        s.focus = Focus::Grid;
                        w.sounds.push(Ui::Select as u32);
                    }
                }
                _ => w.sounds.push(Ui::Error as u32),
            }
        }
        (Focus::Grid, K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft) => {
            // The links (up, down, right, left); frozen on a picked head / frame, past head and frame for an attachment.
            if matches!(s.pick.map(|p| p.1), Some(0 | 1)) {
                return true;
            }
            let k = match key {
                K::ArrowUp => 2,
                K::ArrowDown => 3,
                K::ArrowRight => 4,
                _ => 5,
            };
            let link = |c: u16| gu.menu_links(AUTO, c).map_or(c, |l| l[k] as u16);
            let mut to = link(s.grid);
            if s.pick.is_some() && matches!(to, 1 | 5) {
                to = link(to);
            }
            if to != s.grid && (1..=14).contains(&to) {
                s.grid = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        (Focus::List, _) => {
            let n = rows(w, s).len();
            if step(&mut s.list, n, key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        _ => {}
    }
    true
}

/// Num+ (message 8): with nothing picked the other window takes the focus (SE 0013); automato with a pick refuses.
pub fn num_plus(w: &mut World) {
    let Some(s) = w.pupset.as_mut().filter(|s| s.sibo.is_none()) else { return };
    match (s.focus, s.pick.is_some() || s.slot_pick.is_some()) {
        (Focus::Grid, true) => w.sounds.push(Ui::Error as u32),
        (_, true) => {}
        (f, false) => {
            s.focus = if f == Focus::Grid { Focus::List } else { Focus::Grid };
            w.sounds.push(Ui::Close as u32);
        }
    }
}

/// The scroll bar or wheel on the list.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(s) = w.pupset.as_ref() else { return false };
    let n = rows(w, s).len();
    let s = w.pupset.as_mut().unwrap();
    let mut row = s.list.0.saturating_sub(s.list.1);
    let moved = super::scrollbar::paged(m, &mut s.list.1, &mut row, n, VISIBLE);
    s.list.0 = s.list.1 + row;
    moved
}

/// What the pointer is over: a grid child or a list row (from the top).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Grid(u16),
    List(usize),
}

fn hit(gu: &GameUi, sw: f32, sh: f32, p: (f32, f32)) -> Option<Hit> {
    let inside = |r: [f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    if let Some((x, y)) = gu.window_xy(AUTO, sw, sh)
        && let Some(m) = gu.layout(AUTO)
        && let Some(e) = m.elements.iter().skip(1).find(|e| inside([x + e.x as f32, y + e.y as f32, e.w as f32, e.h as f32]))
    {
        return Some(Hit::Grid(e.id));
    }
    let (x, y) = gu.window_xy(INVEN, sw, sh)?;
    (0..VISIBLE).find(|&k| inside([x + 3.0, y + 6.0 + 18.0 * k as f32, 132.0, 16.0])).map(Hit::List)
}

/// Mouse *(inf, as BLU)*: over the other window the focus moves there (message 8) while nothing is picked; a cell or
/// row moves the cursor, a click on the cursor's is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    if w.modern {
        return;
    }
    let Some(s) = w.pupset.clone().filter(|s| s.sibo.is_none()) else { return };
    let Some(h) = hit(gu, sw, sh, p) else { return };
    gu.hover.set(true);
    let on_grid = matches!(h, Hit::Grid(_));
    if on_grid != (s.focus == Focus::Grid) {
        if moved {
            num_plus(w);
        }
        return;
    }
    set_cursor(w, h, moved, clicked, gu);
}

fn set_cursor(w: &mut World, h: Hit, moved: bool, clicked: bool, gu: &GameUi) {
    let Some(s) = w.pupset.as_ref() else { return };
    let n = rows(w, s).len();
    let same = match h {
        Hit::Grid(c) => c == s.grid,
        Hit::List(k) => s.list.1 + k == s.list.0,
    };
    if same {
        if clicked {
            key(w, gu, egui::Key::Enter);
        }
        return;
    }
    if !(moved || clicked) {
        return;
    }
    let Some(s) = w.pupset.as_mut() else { return };
    match h {
        // Not off a held head / frame, nor onto them with an attachment picked.
        Hit::Grid(c) if !matches!(s.pick.map(|p| p.1), Some(0 | 1)) && !(s.pick.is_some() && matches!(c, 1 | 5)) => s.grid = c,
        Hit::List(k) if s.list.1 + k < n => s.list.0 = s.list.1 + k,
        _ => return,
    }
    w.sounds.push(Ui::Cursor as u32);
}

/// Title "Equipment" (0xd 0x14); help the cursor cell's (0xe 0x286..0x288), the row's, or pupsibor's.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.pupset else { return };
    if let Some(r) = s.sibo {
        let [help, title] = gu.focus_text(SIBO, r);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        return;
    }
    bars.title[1] = gu.title_msg(0x14).unwrap_or_default();
    bars.help[1] = match s.focus {
        Focus::Grid => gu.focus_text(AUTO, s.grid)[0].clone().unwrap_or_default(),
        Focus::List => match rows(w, s).get(s.list.0) {
            Some(Row::Element) => gu.menu_msg(0x289, ""),
            Some(Row::RemoveAll) => gu.lobby_msg(0xc, 0x54, ""),
            Some(&Row::Item(id)) => gu.menu_msg(0x286 + rec(w, id).map_or(0, |r| r.0).min(2) as usize, ""),
            None => String::new(),
        },
    };
}

/// The item the help window shows: the focused window's cursor item.
fn shown(w: &World, s: &Screen) -> Option<u16> {
    match s.focus {
        Focus::Grid => slot_of(s.grid).and_then(|i| slot_item(i, slot(w, s.side, i))),
        Focus::List => match rows(w, s).get(s.list.0) {
            Some(&Row::Item(id)) => Some(id),
            _ => None,
        },
    }
}

/// bluehelp's texts (LAB_10208d90): the singular name with its first letter upper-cased, an attachment's element
/// costs (that many gaiji EF 1F + e each), the description's lines.
fn help_text(w: &World, id: u16) -> Option<(String, Vec<usize>, String)> {
    let it = w.item_info.as_ref()?.item(id)?;
    let mut name: Vec<char> = it.singular.chars().collect();
    if let Some(c) = name.first_mut() {
        *c = c.to_ascii_uppercase();
    }
    let costs = if it.resource >= 2 { (0..8).flat_map(|e| std::iter::repeat_n(e, nibble(it.jobs, e) as usize)).collect() } else { Vec::new() };
    Some((name.into_iter().collect(), costs, it.description.clone()))
}

fn gaiji(e: usize) -> String {
    crate::dat::font::decode_sjis(&[0xef, 0x1f + e as u8])
}

/// bluehelp (FUN_101ec500 with LAB_10208d90): the header at (4, 4), the costs from x1 - 100, 16 apart; the
/// description from (16, 20); (1 + lines) * 16 + 8 high, at least the layout's, growing down.
fn help(gu: &mut GameUi, w: &World, sw: f32, sh: f32, id: u16) -> Option<[f32; 4]> {
    let (name, costs, desc) = help_text(w, id)?;
    let [x, y, fw, fh] = gu.window_rect(HELP, sw, sh)?;
    let lines: Vec<String> = desc.split('\n').flat_map(|l| gu.wrap(l, 316.0)).collect();
    let h = ((1 + lines.len()) as f32 * 16.0 + 8.0).max(fh);
    if !gu.window_sized(HELP, x, y, fw, h) {
        return None;
    }
    let white = WHITE.to_le_bytes();
    gu.text(&name, x + 4.0, y + 4.0, 0, white);
    for (k, &e) in costs.iter().enumerate() {
        gu.text(&gaiji(e), x + fw - 100.0 + 16.0 * k as f32, y + 4.0, 0, white);
    }
    for (i, l) in lines.iter().enumerate() {
        gu.text(l, x + 16.0, y + 20.0 + 16.0 * i as f32, 0, white);
    }
    Some([x, y, fw, h])
}

fn item_name(w: &World, id: u16) -> String {
    w.item_info.as_ref().and_then(|i| i.item(id)).map(|it| it.name.clone()).unwrap_or_default()
}

/// statpup (FUN_10208120): the automaton's name, HP, MP, skills and stats once a PUP block with a name exists.
fn draw_status(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let win = gu.lobby_window_off(STAT, sw, sh, (0.0, 0.0), None, false, &[])?;
    let [x, y, fw, _] = win.frame;
    let Some(b) = status_block(w).filter(|b| b[0x54] != 0) else { return Some(win.frame) };
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    gu.shape_fit(&auto_name(b), x + 4.0, y + 7.0, fw - 6.0, WHITE);
    for (o, dy) in [(0x64, 0x14), (0x68, 0x21)] {
        gu.shape_text(&u16_at(o).to_string(), x + 0x45 as f32, y + dy as f32, WHITE, true);
        gu.shape_text(&format!("/{}", u16_at(o + 2)), x + 0x47 as f32, y + dy as f32, WHITE, false);
    }
    for (i, dy) in [0x37, 0x44, 0x51].into_iter().enumerate() {
        gu.shape_text(&u16_at(0x6c + 4 * i).to_string(), x + 0x4d as f32, y + dy as f32, WHITE, true);
        gu.shape_text(&format!("/{}", u16_at(0x6e + 4 * i)), x + 0x4f as f32, y + dy as f32, WHITE, false);
    }
    for i in 0..7 {
        let ry = y + (0x60 + 13 * i) as f32;
        let end = gu.shape_text(&format!("{:4}", u16_at(0x7c + 4 * i)), x + 0x32 as f32, ry, WHITE, false);
        let bonus = u16_at(0x7e + 4 * i) as i16;
        if bonus < 0 {
            gu.shape_text(&format!(" {bonus}"), end, ry, RED, false);
        } else if bonus > 0 {
            gu.shape_text(&format!(" +{bonus}"), end, ry, GREEN, false);
        }
    }
    Some(win.frame)
}

/// A row list window: its frame and the hand on the cursor row while focused.
fn list_window(gu: &mut GameUi, sw: f32, sh: f32, c: (usize, usize), focused: bool) -> Option<([f32; 4], (f32, f32))> {
    let rows: Vec<u16> = (1..=VISIBLE as u16).collect();
    let win = gu.lobby_window_off(INVEN, sw, sh, (0.0, 0.0), None, false, &rows)?;
    let (x, y) = gu.window_xy(INVEN, sw, sh)?;
    if focused {
        gu.cursor(x + 3.0, y + 6.0 + 18.0 * (c.0 - c.1) as f32);
    }
    Some((win.frame, (x + 3.0, y + 4.0)))
}

/// The screen: statpup, automato, the list, bluehelp, pupsibor (drawn after pupequip so the window walk suspends it).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let (Some(s), Some(info)) = (&w.pupset, w.item_info.clone()) else { return rects };
    let free = w.confirm.is_none() && s.sibo.is_none();
    rects.extend(draw_status(gu, w, sw, sh));
    let items: Vec<(usize, u16)> = (0..14).filter_map(|i| slot_item(i, slot(w, s.side, i)).map(|id| (i, id))).collect();
    let list = rows(w, s);
    super::items::upload_icons(gu, &info, items.iter().map(|v| v.1).chain(list.iter().skip(s.list.1).take(VISIBLE).filter_map(|r| if let Row::Item(id) = r { Some(*id) } else { None })));
    // automato: the cells, the items' icons (the cursor's (-1, -1) while focused), the element counts beside the
    // frame's orbs (red over capacity).
    // The cursor box (anc_equi, as the Equipment grid's) while focused, and on the picked slot *(inf: the unfocused
    // cursor's look)*.
    let grid_on = free && s.focus == Focus::Grid;
    let lit = grid_on || s.slot_pick.is_some();
    if let Some(win) = gu.lobby_window_off(AUTO, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, _, _] = win.frame;
        for &(i, id) in &items {
            let Some(&(_, r)) = win.children.iter().find(|c| c.0 == SLOT_CHILD[i]) else { continue };
            let d = if lit && SLOT_CHILD[i] == s.grid { -1.0 } else { 0.0 };
            let icon = crate::render::invwin::icon_texture(id);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, r[0] + d, r[1] + d, 32.0, 32.0, [1.0; 4]);
            }
        }
        let (used, cap) = capacity(w, s.side);
        for e in 0..8 {
            let (ex, ey) = (x + 39.0 + 60.0 * (e / 3) as f32, y + 148.0 + 14.0 * (e % 3) as f32);
            let c = if cap[e] < used[e] { RED } else { WHITE };
            gu.shape_text(&format!("{}/", used[e]), ex, ey, c, true);
            gu.shape_text(&cap[e].to_string(), ex, ey, c, false);
        }
        if lit
            && let Some((wx, wy)) = gu.window_xy(AUTO, sw, sh)
        {
            let frame = gu.cursor_frame();
            let (b, ui, _, _) = gu.parts();
            if let Some(m) = ui.menu(AUTO) {
                crate::render::invwin::push_cursor(b, ui, m, wx, wy, s.grid as usize, frame);
            }
        }
        rects.push(win.frame);
    }
    // The list: Element, Remove All, the items (the half-size icon, the name in its colour).
    if let Some((frame, (ox, oy))) = list_window(gu, sw, sh, s.list, free && s.focus == Focus::List) {
        for (k, r) in list.iter().skip(s.list.1).take(VISIBLE).enumerate() {
            let y = oy + 18.0 * k as f32;
            match *r {
                Row::Element => {
                    gu.sprite("menu    framesus", 129, ox, y + 2.0, WHITE);
                }
                Row::RemoveAll => {
                    gu.text(&w.battle.as_ref().and_then(|b| b.group9(0xe3)).unwrap_or_default(), ox + 18.0, y + 2.0, 0, REMOVE_ALL.to_le_bytes());
                }
                Row::Item(id) => {
                    let icon = crate::render::invwin::icon_texture(id);
                    if gu.has_image(&icon) {
                        gu.image_rect(&icon, ox, y, 16.0, 16.0, [1.0; 4]);
                    }
                    gu.text(&item_name(w, id), ox + 18.0, y + 2.0, 0, tint(w, s, id).to_le_bytes());
                }
            }
        }
        gu.scroll_bar(frame[0] + frame[2] - 8.0, frame[1], frame[3], list.len(), VISIBLE, s.list.1);
        if s.focus == Focus::List {
            gu.bar_site(super::scrollbar::Owner::PupSet, frame[0] + frame[2] - 8.0, frame[1], frame[3], list.len(), VISIBLE, s.list.1);
        }
        rects.push(frame);
    }
    match shown(w, s).and_then(|id| help(gu, w, sw, sh, id)) {
        Some(r) => rects.push(r),
        None => {
            if let Some(win) = gu.lobby_window_off(HELP, sw, sh, (0.0, 0.0), None, false, &[]) {
                rects.push(win.frame);
            }
        }
    }
    if let Some(r) = s.sibo
        && let Some(win) = gu.lobby_window_off(SIBO, sw, sh, (0.0, 0.0), Some(r), w.confirm.is_none(), &[])
    {
        rects.push(win.frame);
    }
    rects
}

/// The screen as the modern layer draws it.
pub(super) struct Look {
    pub name: String,
    /// Head, frame and the 12 attachments: (label, item, tint).
    pub slots: Vec<(String, Option<u16>, u32)>,
    pub slot_cursor: usize,
    pub capacity: [(i32, i32); 8],
    pub list: Vec<(String, Option<u16>, u32)>,
    pub list_cursor: usize,
    pub on_grid: bool,
    pub picking: bool,
    pub help: Option<(String, Vec<usize>, String)>,
    pub status: Vec<(String, String)>,
    /// pupsibor's rows and cursor (1..8) while it is open.
    pub sibo: Option<(Vec<String>, u16)>,
}

pub(super) fn look(w: &World, gu: &GameUi) -> Option<Look> {
    let s = w.pupset.as_ref()?;
    // The cells' captions (framesus 44 "Head", 126 "Frame"), the attachments numbered.
    let slot_label = |i: usize| match i {
        0 => gu.sprite_caption("menu    framesus", 44).unwrap_or_else(|| "Head".into()),
        1 => gu.sprite_caption("menu    framesus", 126).unwrap_or_else(|| "Frame".into()),
        _ => format!("{}", i - 1),
    };
    let slots = (0..14)
        .map(|i| {
            let id = slot_item(i, slot(w, s.side, i));
            let tint = if s.slot_pick == Some(i) { PICKED } else { WHITE };
            (format!("{}  {}", slot_label(i), id.map(|id| item_name(w, id)).unwrap_or_default()), id, tint)
        })
        .collect();
    let (used, cap) = capacity(w, s.side);
    let list = rows(w, s)
        .iter()
        .map(|r| match *r {
            Row::Element => (gu.sprite_caption("menu    framesus", 129).unwrap_or_else(|| "Element".into()), None, WHITE),
            Row::RemoveAll => (w.battle.as_ref().and_then(|b| b.group9(0xe3)).unwrap_or_default(), None, REMOVE_ALL),
            Row::Item(id) => (item_name(w, id), Some(id), tint(w, s, id)),
        })
        .collect();
    let mut status = Vec::new();
    let mut name = String::new();
    if let Some(b) = status_block(w).filter(|b| b[0x54] != 0) {
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        name = auto_name(b);
        status.push(("HP".into(), format!("{}/{}", u16_at(0x64), u16_at(0x66))));
        status.push(("MP".into(), format!("{}/{}", u16_at(0x68), u16_at(0x6a))));
        for (i, l) in ["Melee", "Ranged", "Magic"].into_iter().enumerate() {
            status.push((l.into(), format!("{}/{}", u16_at(0x6c + 4 * i), u16_at(0x6e + 4 * i))));
        }
        for (i, l) in ["STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR"].into_iter().enumerate() {
            let bonus = u16_at(0x7e + 4 * i) as i16;
            let extra = match bonus {
                0 => String::new(),
                b if b > 0 => format!(" +{b}"),
                b => format!(" {b}"),
            };
            status.push((l.into(), format!("{}{extra}", u16_at(0x7c + 4 * i))));
        }
    }
    let sibo = s.sibo.map(|r| ((1..=8).map(|k| gu.child_caption(SIBO, k).unwrap_or_default()).collect(), r));
    Some(Look {
        name,
        slots,
        slot_cursor: slot_of(s.grid).unwrap_or(0),
        capacity: std::array::from_fn(|e| (used[e], cap[e])),
        list,
        list_cursor: s.list.0,
        on_grid: s.focus == Focus::Grid,
        picking: s.pick.is_some() || s.slot_pick.is_some(),
        help: shown(w, s).and_then(|id| help_text(w, id)),
        status,
        sibo,
    })
}

/// The mouse's moves in the modern layer: a slot (grid) or a list row; with nothing picked the other side takes the
/// focus first (as Num+). True when the cursor moved.
pub(super) fn set_row(w: &mut World, grid: bool, k: usize) -> bool {
    let Some(s) = w.pupset.as_ref().filter(|s| s.sibo.is_none()) else { return false };
    if (s.focus == Focus::Grid) != grid {
        if s.pick.is_some() || s.slot_pick.is_some() {
            return false;
        }
        num_plus(w);
    }
    let n = w.pupset.as_ref().map_or(0, |s| rows(w, s).len());
    let Some(s) = w.pupset.as_mut() else { return false };
    if grid {
        let Some(&c) = SLOT_CHILD.get(k) else { return false };
        if c == s.grid || matches!(s.pick.map(|p| p.1), Some(0 | 1)) || (s.pick.is_some() && matches!(c, 1 | 5)) {
            return false;
        }
        s.grid = c;
    } else {
        if k == s.list.0 || k >= n {
            return false;
        }
        list_to(s, k);
    }
    true
}

pub(super) fn set_sibo(w: &mut World, r: u16) -> bool {
    let Some(cur) = w.pupset.as_mut().and_then(|s| s.sibo.as_mut()).filter(|c| **c != r && (1..=8).contains(&r)) else { return false };
    *cur = r;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_and_nibbles() {
        assert_eq!(SLOT_CHILD.iter().filter(|&&c| (1..=14).contains(&c)).count(), 14);
        assert_eq!(slot_of(5), Some(1));
        assert_eq!(slot_of(14), Some(13));
        assert_eq!(slot_item(0, 2), Some(0x2002));
        assert_eq!(slot_item(5, 0x42), Some(0x2142));
        assert_eq!(slot_item(5, 0), None);
        // Harlequin Head 0x00222222: two of each but Light and Dark.
        assert_eq!((0..8).map(|e| nibble(0x0022_2222, e)).collect::<Vec<_>>(), [2, 2, 2, 2, 2, 2, 0, 0]);
    }
}
