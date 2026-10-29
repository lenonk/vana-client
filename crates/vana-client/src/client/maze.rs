//! Moblin Maze Mongers' Maze Tabula editor (mmm_maze_spec.md): using a tabula (or event op 0x71 0x20) opens prvdungp,
//! the 5 x 5 grid, with BLU's bluinven fed by the maze list (vouchers, runes, placed runes), bluehelp and prvdunsi (the
//! shape filter). s2c 0x0AD carries the learned vouchers and runes; an edited tabula sends c2s 0x0D8 as it closes.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::dat::maze::{self, Placed, RUNE_BASE, VOUCHER_BASE};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const GRID: &str = "menu    prvdungp";
pub(super) const SIBO: &str = "menu    prvdunsi";
const INVEN: &str = "menu    bluinven";
const HELP: &str = "menu    bluehelp";
const LIB: &str = "menu    prvdunge";
const VISIBLE: usize = 10;
const WHITE: u32 = 0x8080_8080;
pub(super) const GREEN: u32 = 0x8050_9050;
pub(super) const GREY: u32 = 0x8040_4040;
pub(super) const HELD: u32 = 0x8080_4020;
/// FUN_100fc3c0(0xd8, 0, 0): a 0x0D8 within a second of the last is dropped, and the edit with it.
const THROTTLE: Duration = Duration::from_millis(1000);
/// The open hooks' places for BLU's windows: bluinven (FUN_101ebf10) and bluehelp (FUN_101ec220).
const INVEN_AT: (i16, i16) = (16, 48);
const HELP_AT: (i16, i16) = (16, 240);
/// The language slot of the rune texts (FUN_1025b580(0x26)): entry rune index x 64 + voucher index.
pub(super) const TEXT_SLOT: usize = 0x26;

#[derive(Default)]
pub struct Maze {
    /// s2c 0x0AD's 0x80 bytes: learned vouchers (8), runes (64), 56 unread.
    store: Option<Box<[u8; 0x80]>>,
    pub(super) screen: Option<Screen>,
    /// An edited screen the event start closed; the event end saves it (FUN_100ae3f0 -> FUN_101fcfb0).
    pending: Option<Screen>,
    sent: Option<Instant>,
    texts: Option<Arc<crate::dat::names::DMsg>>,
}

/// The list's mode (source +0x10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Vouchers,
    Runes,
    Placed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    List,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    /// framesus 159 "Type ▶": prvdunsi.
    Type,
    Item(u16),
}

#[derive(Clone, Debug)]
pub struct Screen {
    /// +0x14: 0 edit (an M tabula), 1 view (an R tabula, the event preview).
    edit: bool,
    /// +0x17: the inventory slot (None from the event op).
    slot: Option<u8>,
    tabula: u16,
    /// +0x3c: the extdata it opened with.
    ext: [u8; 24],
    /// +0x30: 0 none.
    voucher: u16,
    /// +0x58: the 12 entries, None empty.
    placed: [Option<Placed>; 12],
    mode: Mode,
    /// Source +0x11: the shape shown, None all.
    filter: Option<u8>,
    list: (usize, usize),
    /// Source +0x12: the cursor goes to the current voucher's row (mode 0), else the first, on the next rows.
    reset: bool,
    focus: Focus,
    /// +0x32 / +0x1e: the held rune and whether it is carried (else the hover preview).
    held: Option<(u16, bool)>,
    /// +0x34 / +0x35 / +0x36: where the held rune sits, kept after it goes.
    at: (i8, i8),
    rot: u8,
    /// prvdunsi's cursor child (1..11) while it is open.
    sibo: Option<u16>,
    /// +0x1c / +0x1f the pulse alpha and its step, +0x1a the glint; stepped at 30 a second.
    pulse: (i16, i16),
    glint: i16,
    clock: f32,
}

fn bit(b: &[u8], n: usize) -> bool {
    b.get(n >> 3).is_some_and(|v| v >> (n & 7) & 1 != 0)
}

/// s2c 0x0AD (LAB_100e6150): the store replaced whole.
pub fn on_store(w: &mut World, data: &[u8]) {
    let mut b = Box::new([0u8; 0x80]);
    let n = data.len().min(0x80);
    b[..n].copy_from_slice(&data[..n]);
    log::debug!("maze: 0x0AD vouchers {:02x?} runes {} learned", &b[..8], (0..512).filter(|&n| bit(&b[8..0x48], n)).count());
    w.maze.store = Some(b);
}

/// The zone's session block is zeroed (FUN_100f9940) and its windows torn down.
pub fn zone_reset(w: &mut World) {
    let sent = w.maze.sent;
    let texts = w.maze.texts.take();
    w.maze = Maze { sent, texts, ..Default::default() };
}

/// The K7000 block of a record (record +0x14..+0x54).
fn block(w: &World, id: u16) -> Option<[u8; 64]> {
    w.item_info.as_ref()?.item(id)?.maze
}

/// A rune's element (0..7, 8 none) and shape (0..10): record bytes 0 and 1 (FUN_101fdfe0's cache).
pub(super) fn rune(w: &World, id: u16) -> Option<(u8, u8)> {
    let b = block(w, id)?;
    let e = if (b[0] as i8) < 0 { 8 } else { b[0] & 0xf };
    (id >= RUNE_BASE && (b[1] as usize) < maze::SHAPES.len()).then_some((e.min(8), b[1]))
}

fn shape(w: &World, id: u16) -> Option<u8> {
    rune(w, id).map(|r| r.1)
}

/// A tabula's 25 cells (5 r + c): -1 empty, < -1 blocked, 0..7 an element.
pub(super) fn grid(w: &World, tabula: u16) -> [i8; 25] {
    block(w, tabula).map_or([-1; 25], |b| std::array::from_fn(|c| b[c] as i8))
}

fn blocked(g: &[i8; 25]) -> u32 {
    (0..25).filter(|&c| g[c] < -1).fold(0, |m, c| m | 1 << (24 - c))
}

/// FUN_101fe4d0: the voucher's record allows the rune (bit rune index).
fn allows(w: &World, voucher: u16, rune: u16) -> bool {
    rune.checked_sub(RUNE_BASE).zip(block(w, voucher)).is_some_and(|(n, b)| bit(&b, n as usize))
}

/// The cells a rune covers where it is placed.
pub(super) fn cells_at(w: &World, rune: u16, col: i8, row: i8, rot: u8) -> u32 {
    shape(w, rune).map_or(0, |s| maze::cells(maze::mask(s, rot), col, row))
}

/// +0x54: the blocked cells and every placed rune's.
fn occupied(w: &World, s: &Screen) -> u32 {
    s.placed.iter().flatten().fold(blocked(&grid(w, s.tabula)), |m, p| m | cells_at(w, p.rune, p.col, p.row, p.rot))
}

/// FUN_101fe650 / FUN_101fe760 / FUN_101fe890: the learned vouchers; the Type row and the learned runes of the shape
/// filter; the placed runes in entry order. Without 0x0AD the first has no rows and the second only Type.
pub(super) fn rows(w: &World, s: &Screen) -> Vec<Row> {
    let store = w.maze.store.as_deref();
    match s.mode {
        Mode::Vouchers => store.map_or_else(Vec::new, |b| (0..64u16).filter(|&n| bit(&b[..8], n as usize)).map(|n| Row::Item(VOUCHER_BASE + n)).collect()),
        Mode::Runes => {
            let mut v = vec![Row::Type];
            if let Some(b) = store {
                // FUN_101fe540: under a filter only runes whose shape is known and matches.
                v.extend((0..512u16).filter(|&n| bit(&b[8..0x48], n as usize) && s.filter.is_none_or(|f| shape(w, RUNE_BASE + n) == Some(f))).map(|n| Row::Item(RUNE_BASE + n)));
            }
            v
        }
        Mode::Placed => s.placed.iter().flatten().map(|p| Row::Item(p.rune)).collect(),
    }
}

/// A row's name colour (FUN_101fe580's callers).
pub(super) fn tint(w: &World, s: &Screen, row: Row) -> u32 {
    let Row::Item(id) = row else { return WHITE };
    match s.mode {
        Mode::Vouchers if id == s.voucher => GREEN,
        Mode::Vouchers => WHITE,
        Mode::Runes if !allows(w, s.voucher, id) => GREY,
        Mode::Runes if s.placed.iter().flatten().any(|p| p.rune == id) => GREEN,
        Mode::Runes if s.held.is_some_and(|h| h.0 == id) => HELD,
        Mode::Runes => WHITE,
        Mode::Placed => GREEN,
    }
}

/// The help line of the list's cursor row (its +0x4c).
fn row_help(gu: &GameUi, w: &World, s: &Screen) -> String {
    match (s.mode, rows(w, s).get(s.list.0).copied()) {
        (_, None) => String::new(),
        (_, Some(Row::Type)) => gu.menu_msg(0x2a4, ""),
        (Mode::Vouchers, _) => gu.menu_msg(0x2a5, ""),
        (Mode::Runes, Some(r)) if tint(w, s, r) == GREY => gu.menu_msg(0x2a7, ""),
        (Mode::Runes, _) => gu.menu_msg(0x2a6, ""),
        (Mode::Placed, _) => gu.lobby_msg(0xc, 0x54, ""),
    }
}

/// FUN_101fcf60 / FUN_101fcec0: the editor on a tabula's extdata, decoded at once (every record is at hand); edit
/// mode lists the vouchers, view mode the placed runes. The list has the focus.
pub fn open(w: &mut World, edit: bool, slot: Option<u8>, tabula: u16, ext: [u8; 24]) {
    close(w);
    w.maze.pending = None;
    let g = grid(w, tabula);
    let (voucher, runes) = maze::decode(&ext, |r| shape(w, r), blocked(&g));
    let mut placed = [None; 12];
    for (e, p) in placed.iter_mut().zip(runes) {
        *e = Some(p);
    }
    log::info!("maze: tabula {tabula:#x} slot {slot:?} {} voucher {voucher:#x}, {} runes", if edit { "edit" } else { "view" }, placed.iter().flatten().count());
    w.maze.screen = Some(Screen {
        edit,
        slot,
        tabula,
        ext,
        voucher,
        placed,
        mode: if edit { Mode::Vouchers } else { Mode::Placed },
        filter: None,
        list: (0, 0),
        reset: true,
        focus: Focus::List,
        held: None,
        at: (0, 0),
        rot: 0,
        sibo: None,
        pulse: (0x60, 8),
        glint: 0x60,
        clock: 0.0,
    });
}

/// iuse Use / the direct list on an inventory tabula: type 0x16 edits, 0x17 views (FUN_101fcf60(0 / 1, slot)).
pub fn use_tabula(w: &mut World, index: u8, id: u16, edit: bool) {
    let ext = w.inv.ext.get(&(0, index)).copied().unwrap_or([0; 24]);
    open(w, edit, Some(index), id, ext);
}

/// FUN_101fcfb0: an edited tabula whose encoding changed goes out as c2s 0x0D8; then the windows close.
pub fn close(w: &mut World) {
    if let Some(s) = w.maze.screen.take() {
        save(w, &s);
    }
}

/// FUN_100ec3f0's c2s 0x0D8 {ActIndex, tabula, slot, UniqueNo, the new extdata}.
fn save(w: &mut World, s: &Screen) {
    let Some(slot) = s.slot.filter(|_| s.edit && s.voucher != 0) else { return };
    let placed: Vec<Placed> = s.placed.iter().flatten().copied().collect();
    let ext = maze::encode(s.voucher, &placed, |r| shape(w, r));
    if ext == s.ext {
        return;
    }
    if w.maze.sent.is_some_and(|t| t.elapsed() < THROTTLE) {
        log::info!("maze: c2s 0x0D8 dropped (within a second of the last)");
        return;
    }
    let mut body = vec![0u8; 0x24];
    body[0..2].copy_from_slice(&w.my_index.to_le_bytes());
    body[2..4].copy_from_slice(&s.tabula.to_le_bytes());
    body[4] = slot;
    body[8..12].copy_from_slice(&w.my_id.to_le_bytes());
    body[12..36].copy_from_slice(&ext);
    log::info!("maze: c2s 0x0D8 tabula {:#x} slot {slot} ext {:02x?}", s.tabula, ext);
    w.maze.sent = Some(Instant::now());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0xd8, body });
}

/// The event start's broadcast closes the windows; the handler keeps its state for the event's end.
pub fn event_start(w: &mut World) {
    if let Some(s) = w.maze.screen.take() {
        w.maze.pending = Some(s);
    }
}

/// FUN_100ae3f0 -> FUN_101fcfb0 at every event end: an edit the start closed is saved now (retail's late 0x0D8), and
/// an open screen closes.
pub fn event_end(w: &mut World) {
    if let Some(s) = w.maze.pending.take() {
        save(w, &s);
    }
    close(w);
}

/// FUN_101fd220: hold `rune` (None: nothing), carried or as the hover preview; a placed rune lends its place, and
/// leaves the grid when carried; another starts unturned where the last one was. False when its shape is unknown.
fn hold(w: &mut World, s: &mut Screen, rune: Option<u16>, carry: bool) -> bool {
    let Some(r) = rune else {
        s.held = None;
        return true;
    };
    if shape(w, r).is_none() {
        return false;
    }
    s.held = Some((r, carry));
    s.pulse = (0x60, 8);
    match s.placed.iter().position(|p| p.is_some_and(|p| p.rune == r)) {
        Some(i) => {
            let p = s.placed[i].unwrap();
            (s.at, s.rot) = ((p.col, p.row), p.rot);
            if carry {
                s.placed[i] = None;
            }
        }
        None => s.rot = 0,
    }
    nudge(w, s, 0, 0);
    true
}

/// FUN_101fdb40: the held rune's top left moved to `to`, kept on the grid; SE 0001 when it moved.
fn move_to(w: &mut World, s: &mut Screen, to: (i8, i8)) {
    let Some(sh) = s.held.and_then(|h| shape(w, h.0)) else { return };
    let to = maze::clamp(sh, s.rot, to.0, to.1);
    if to != s.at {
        s.at = to;
        w.sounds.push(Ui::Cursor as u32);
    }
}

fn nudge(w: &mut World, s: &mut Screen, dx: i8, dy: i8) {
    let to = (s.at.0.saturating_add(dx), s.at.1.saturating_add(dy));
    move_to(w, s, to);
}

/// FUN_101fda30: the held rune into the first empty entry where it is, unless a cell is taken.
fn place(w: &World, s: &mut Screen) -> bool {
    let Some((r, _)) = s.held else { return false };
    let cells = cells_at(w, r, s.at.0, s.at.1, s.rot);
    if cells == 0 || cells & occupied(w, s) != 0 {
        return false;
    }
    let Some(e) = s.placed.iter_mut().find(|p| p.is_none()) else { return false };
    *e = Some(Placed { rune: r, col: s.at.0, row: s.at.1, rot: s.rot });
    true
}

/// FUN_101fd1f0: the voucher set; `clear` sets the grid up again (every rune off).
fn set_voucher(s: &mut Screen, v: u16, clear: bool) {
    s.voucher = v;
    if clear {
        s.placed = [None; 12];
    }
}

fn list_to(s: &mut Screen, r: usize) {
    s.list.0 = r;
    s.list.1 = s.list.1.min(r).max((r + 1).saturating_sub(VISIBLE));
}

/// FUN_101fe930's tail: the pending cursor reset (the current voucher's row in mode 0, else the first), and the cursor
/// kept on the rows.
fn fix(w: &World, s: &mut Screen) {
    let n = rows(w, s).len();
    if s.reset {
        let r = if s.mode == Mode::Vouchers { rows(w, s).iter().position(|r| *r == Row::Item(s.voucher)) } else { None };
        s.list = (0, 0);
        list_to(s, r.unwrap_or(0));
        s.reset = false;
    }
    s.list.0 = s.list.0.min(n.saturating_sub(1));
    s.list.1 = s.list.1.min(s.list.0);
}

enum Then {
    Keep,
    Close,
    /// View mode's Enter: the event's message (rem4line) takes it.
    Forward,
}

/// The keys: prvdunsi (FUN_101fe3f0 over the generic window), prvdungp while a rune is carried (FUN_101fe290), else
/// the list (FUN_101febf0 over BLU's). True when taken.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    let Some(mut s) = w.maze.screen.take() else { return false };
    match keys(w, &mut s, key) {
        Then::Close => save(w, &s),
        then => {
            if w.maze.screen.is_none() {
                fix(w, &mut s);
                w.maze.screen = Some(s);
            }
            if matches!(then, Then::Forward) && super::event::active(w) {
                super::event::key(w, egui::Key::Enter);
            }
        }
    }
    true
}

fn keys(w: &mut World, s: &mut Screen, key: egui::Key) -> Then {
    use egui::Key as K;
    if let Some(r) = s.sibo {
        match key {
            K::ArrowDown | K::ArrowUp => {
                s.sibo = Some(if key == K::ArrowDown { r % 11 + 1 } else if r <= 1 { 11 } else { r - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                (s.filter, s.sibo) = (Some(r as u8 - 1), None);
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                s.sibo = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return Then::Keep;
    }
    if s.focus == Focus::Grid {
        match key {
            K::ArrowDown => nudge(w, s, 0, 1),
            K::ArrowUp => nudge(w, s, 0, -1),
            K::ArrowRight => nudge(w, s, 1, 0),
            K::ArrowLeft => nudge(w, s, -1, 0),
            K::Enter if place(w, s) => {
                (s.focus, s.held) = (Focus::List, None);
                w.sounds.push(Ui::Select as u32);
            }
            K::Enter => w.sounds.push(Ui::Error as u32),
            // A rune picked up off the grid is gone with it.
            K::Escape => {
                (s.focus, s.held) = (Focus::List, None);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return Then::Keep;
    }
    let rows = rows(w, s);
    match key {
        // Silent both ways.
        K::Escape if s.mode == Mode::Runes => {
            (s.mode, s.reset) = (Mode::Vouchers, true);
        }
        K::Escape => return Then::Close,
        K::Enter => match (s.mode, rows.get(s.list.0).copied()) {
            (Mode::Placed, _) => return Then::Forward,
            (_, None) => {}
            (_, Some(Row::Type)) => {
                (s.filter, s.sibo) = (None, Some(w.cursor_recall(SIBO).clamp(1, 11)));
                w.sounds.push(Ui::Select as u32);
            }
            (_, Some(r)) if tint(w, s, r) == GREY => w.sounds.push(Ui::Error as u32),
            (Mode::Vouchers, Some(Row::Item(v))) => {
                if s.voucher == 0 || s.voucher == v {
                    set_voucher(s, v, s.voucher == 0);
                    (s.mode, s.reset) = (Mode::Runes, true);
                } else {
                    // Another voucher: comyn "All Maze Runes will be removed. Do you wish to proceed?", default No.
                    w.confirm = Some((super::Confirm::MazeVoucher(v), false));
                }
                w.sounds.push(Ui::Select as u32);
            }
            (Mode::Runes, Some(Row::Item(r))) => {
                if hold(w, s, Some(r), true) {
                    s.focus = Focus::Grid;
                    w.sounds.push(Ui::Select as u32);
                } else {
                    w.sounds.push(Ui::Error as u32);
                }
            }
        },
        K::ArrowDown | K::ArrowUp | K::ArrowRight | K::ArrowLeft => {
            let old = s.list.0;
            let n = rows.len();
            match key {
                K::ArrowDown if s.list.0 + 1 < n => s.list.0 += 1,
                K::ArrowUp if s.list.0 > 0 => s.list.0 -= 1,
                K::ArrowRight => s.list.0 = (s.list.0 + VISIBLE).min(n.saturating_sub(1)),
                K::ArrowLeft => s.list.0 = s.list.0.saturating_sub(VISIBLE),
                _ => {}
            }
            let r = s.list.0;
            list_to(s, r);
            if s.list.0 != old {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        _ => {}
    }
    Then::Keep
}

/// The voucher prompt's Yes (LAB_101febb0): the new voucher, the grid emptied, the rune list unfiltered.
pub fn voucher_yes(w: &mut World, v: u16) {
    let Some(mut s) = w.maze.screen.take() else { return };
    set_voucher(&mut s, v, true);
    (s.mode, s.reset, s.filter) = (Mode::Runes, true, None);
    fix(w, &mut s);
    w.maze.screen = Some(s);
    w.sounds.push(Ui::Select as u32);
}

/// Message 7 (key function 0x18, Num-) on the grid: the carried rune turns a quarter (FUN_101fdbf0), SE 0015.
pub fn rotate(w: &mut World) {
    let Some(mut s) = w.maze.screen.take() else { return };
    if s.sibo.is_none() && s.focus == Focus::Grid && s.held.is_some_and(|h| h.1) {
        s.rot = (s.rot + 1) & 3;
        w.sounds.push(Ui::Page as u32);
        nudge(w, &mut s, 0, 0);
    }
    w.maze.screen = Some(s);
}

/// Per frame (FUN_101feab0 and the draw's counters): the rune texts loaded once, the pulse and glint at 30 a second,
/// and while the list has the focus a placed rune under its cursor pulses on the grid.
pub fn tick(w: &mut World, dt: f32) {
    if w.maze.screen.is_none() {
        return;
    }
    if w.maze.texts.is_none()
        && let (Some(index), Some(file)) = (w.index.clone(), w.point_files[5])
    {
        w.maze.texts = crate::dat::names::DMsg::load(&index, file).map_err(|e| log::warn!("maze rune texts {file}: {e:#}")).ok().map(Arc::new);
    }
    let Some(mut s) = w.maze.screen.take() else { return };
    s.clock += dt;
    while s.clock >= 1.0 / 30.0 {
        s.clock -= 1.0 / 30.0;
        s.pulse.0 += s.pulse.1;
        if s.pulse.0 > 0xa0 {
            s.pulse.1 = -8;
        }
        if s.pulse.0 < 0x60 {
            s.pulse.1 = 8;
        }
        s.glint += 2;
        if s.glint > 0x7f {
            s.glint = 0x30;
        }
    }
    fix(w, &mut s);
    if s.focus == Focus::List && s.sibo.is_none() && w.confirm.is_none() {
        let v = match rows(w, &s).get(s.list.0) {
            Some(&Row::Item(v)) => v,
            _ => 0,
        };
        if s.held.map(|h| h.0) != Some(v) {
            let placed = v != 0 && s.placed.iter().flatten().any(|p| p.rune == v);
            hold(w, &mut s, placed.then_some(v), false);
        }
    }
    if w.maze.screen.is_none() {
        w.maze.screen = Some(s);
    }
}

/// bluehelp's text (FUN_101feab0): the voucher's description; the rune's text for the current voucher (FUN_101ec330
/// of the slot 0x26 entry); nothing for the Type row or no row. True for a description (pre-split as iteminfo).
pub(super) fn help_text(w: &World, s: &Screen) -> Option<(String, bool)> {
    let Some(&Row::Item(v)) = rows(w, s).get(s.list.0) else { return None };
    if v < RUNE_BASE || s.mode == Mode::Vouchers {
        return w.item_info.as_ref()?.item(v).map(|it| (it.description.clone(), true));
    }
    let entry = (v - RUNE_BASE) as usize * 64 + s.voucher.checked_sub(VOUCHER_BASE)? as usize;
    w.maze.texts.as_ref()?.text(entry).map(|t| (t, false))
}

/// The voucher a tabula's extdata names, by name (iteminfo's extra line).
pub(super) fn tabula_voucher(w: &World, it: &crate::dat::items::Item, ext: Option<&[u8; 24]>) -> Option<String> {
    let v = ext.filter(|_| matches!(it.item_type, 0x16 | 0x17)).map(maze::voucher).filter(|&v| v != 0)?;
    w.item_info.as_ref()?.item(v).map(|i| i.name.clone())
}

fn item_name(w: &World, id: u16) -> String {
    w.item_info.as_ref().and_then(|i| i.item(id)).map(|it| it.name.clone()).unwrap_or_default()
}

pub(super) fn gaiji(e: u8) -> String {
    crate::dat::font::decode_sjis(&[0xef, 0x1f + e])
}

/// The runes as FUN_101fdcc0 draws them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Mode 0.
    Plain,
    /// Mode 1: lifted 2, at the pulse's alpha.
    Hover,
    /// Mode 2: a shadow at (3, 3), the rune lifted 4 at the pulse's alpha.
    Carried,
    /// Mode 3: a third of full alpha.
    Faded,
    /// Mode 4: on a cell of its element, a fading copy rising off it.
    Glint,
}

fn alpha(a: f32) -> u32 {
    ((a.clamp(0.0, 255.0) as u32) << 24) | 0x80_8080
}

/// FUN_101fdcc0: sprite prvdunge 3 + 11 element + shape, turned by `rot` quarters about its origin and moved so it
/// stays on its cells (rot 1 (h, 0), 2 (w, h), 3 (0, w)), at cell (x, y).
#[allow(clippy::too_many_arguments)]
fn rune_sprite(gu: &mut GameUi, element: u8, shape: u8, x: f32, y: f32, rot: u8, look: Look, pulse: i16, glint: i16) {
    let k = 3 + 11 * element.min(8) as usize + shape as usize;
    let (sw, sh) = (gu.sprite_width(LIB, k), gu.sprite_height(LIB, k));
    if sw <= 0.0 {
        return;
    }
    let (m, d) = match rot & 3 {
        0 => ([1.0, 0.0, 0.0, 1.0], (0.0, 0.0)),
        1 => ([0.0, -1.0, 1.0, 0.0], (sh, 0.0)),
        2 => ([-1.0, 0.0, 0.0, -1.0], (sw, sh)),
        _ => ([0.0, 1.0, -1.0, 0.0], (0.0, sw)),
    };
    let (px, py) = (x + d.0, y + d.1);
    let mut draw = |off: f32, tint: u32| gu.sprite_affine_blend(LIB, k, px + off, py + off, m, tint, 0x44);
    match look {
        Look::Plain => draw(0.0, WHITE),
        Look::Hover => draw(-2.0, alpha(pulse as f32)),
        Look::Carried => {
            draw(3.0, 0x4030_3030);
            draw(-4.0, alpha(pulse as f32));
        }
        Look::Glint if glint >= 0x60 => {
            let t = (glint - 0x60) as f32 * 0.03125;
            draw(-(5.0 * t + 0.5).trunc(), alpha((80.0 * (1.0 - t)).trunc()));
            draw(0.0, alpha((80.0 * t).trunc()));
        }
        Look::Faded | Look::Glint => draw(0.0, 0x5080_8080),
    }
}

/// Whether a placed rune covers a cell of its own element (FUN_101fe0b0's mode 4).
pub(super) fn bonus(w: &World, g: &[i8; 25], p: &Placed) -> bool {
    let (Some((e, _)), cells) = (rune(w, p.rune), cells_at(w, p.rune, p.col, p.row, p.rot)) else { return false };
    (0..25).any(|c| cells >> (24 - c) & 1 != 0 && g[c] == e as i8)
}

/// prvdungp's contents (FUN_101fe0b0): the voucher's icon and name, the placed runes, the tabula's blocked cells and
/// element marks over them, the placed runes again (faded, glinting on their element), the held rune.
fn draw_grid(gu: &mut GameUi, w: &World, s: &Screen, x0: f32, y0: f32) {
    if s.voucher != 0 {
        let icon = crate::render::invwin::icon_texture(s.voucher);
        if gu.has_image(&icon) {
            gu.image_rect(&icon, x0 + 8.0, y0 + 16.0, 32.0, 32.0, [1.0; 4]);
        }
        gu.text(&item_name(w, s.voucher), x0 + 44.0, y0 + 24.0, 0, GREEN.to_le_bytes());
    }
    let g = grid(w, s.tabula);
    let at = |c: i8, r: i8| (x0 + 40.0 + 24.0 * c as f32, y0 + 56.0 + 24.0 * r as f32);
    let placed: Vec<(Placed, u8, u8)> = s.placed.iter().flatten().filter_map(|p| rune(w, p.rune).map(|(e, sh)| (*p, e, sh))).collect();
    for &(p, e, sh) in &placed {
        let (x, y) = at(p.col, p.row);
        rune_sprite(gu, e, sh, x, y, p.rot, Look::Plain, s.pulse.0, s.glint);
    }
    for c in 0..25usize {
        let (x, y) = at((c % 5) as i8, (c / 5) as i8);
        match g[c] {
            v if v < -1 => _ = gu.sprite(LIB, 1, x, y, WHITE),
            e @ 0..=7 => gu.text(&gaiji(e as u8), x + 5.0, y + 5.0, 0, 0x2080_8080u32.to_le_bytes()),
            _ => {}
        }
    }
    for &(p, e, sh) in &placed {
        let (x, y) = at(p.col, p.row);
        rune_sprite(gu, e, sh, x, y, p.rot, if bonus(w, &g, &p) { Look::Glint } else { Look::Faded }, s.pulse.0, s.glint);
    }
    if let Some((r, carry)) = s.held
        && let Some((e, sh)) = rune(w, r)
    {
        let (x, y) = at(s.at.0, s.at.1);
        rune_sprite(gu, e, sh, x, y, s.rot, if carry { Look::Carried } else { Look::Hover }, s.pulse.0, s.glint);
    }
}

/// bluehelp (FUN_101ec500): a description's lines at (4, 4 + 16 k), or the rune text wrapped to the width - 8 from
/// (4, 4); lines x 16 + 8 high, at least the layout's, growing down; empty without a text.
fn draw_help(gu: &mut GameUi, w: &World, s: &Screen, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let off = gu.moved_off(HELP, sw, sh, HELP_AT.0, HELP_AT.1);
    let [x, y, fw, fh] = gu.window_rect(HELP, sw, sh)?;
    let (x, y) = (x + off.0, y + off.1);
    let lines: Vec<String> = match help_text(w, s) {
        Some((t, true)) => t.split('\n').flat_map(|l| gu.wrap(l, 316.0)).collect(),
        Some((t, false)) => t.split('\n').flat_map(|l| gu.wrap(l, fw - 8.0)).collect(),
        None => Vec::new(),
    };
    let h = (lines.len() as f32 * 16.0 + 8.0).max(fh);
    if !gu.window_sized(HELP, x, y, fw, h) {
        return None;
    }
    for (k, l) in lines.iter().enumerate() {
        gu.text(l, x + 4.0, y + 4.0 + 16.0 * k as f32, 0, WHITE.to_le_bytes());
    }
    Some([x, y, fw, h])
}

/// The screen (classic): prvdungp, bluinven moved to (16, 48), bluehelp to (16, 240), prvdunsi while open. Drawn after
/// the item windows so the walk suspends them; the windows take groups 0x10 and 8 while it is up.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(s), false) = (w.maze.screen.as_ref(), w.modern) else {
        gu.set_group(INVEN, None);
        gu.set_group(HELP, None);
        return rects;
    };
    gu.set_group(INVEN, Some(0x10));
    gu.set_group(HELP, Some(8));
    let Some(info) = w.item_info.clone() else { return rects };
    let list = rows(w, s);
    let shown = list.iter().skip(s.list.1).take(VISIBLE).filter_map(|r| if let Row::Item(id) = r { Some(*id) } else { None });
    super::items::upload_icons(gu, &info, std::iter::once(s.voucher).filter(|&v| v != 0).chain(shown));
    if let Some(win) = gu.lobby_window_off(GRID, sw, sh, (0.0, 0.0), None, false, &[]) {
        draw_grid(gu, w, s, win.frame[0], win.frame[1]);
        rects.push(win.frame);
    }
    let off = gu.moved_off(INVEN, sw, sh, INVEN_AT.0, INVEN_AT.1);
    let hide: Vec<u16> = (1..=VISIBLE as u16).collect();
    if let Some(win) = gu.lobby_window_off(INVEN, sw, sh, off, None, false, &hide)
        && let Some((x, y)) = gu.window_xy(INVEN, sw, sh)
    {
        let (x, y) = (x + off.0, y + off.1);
        if w.confirm.is_none() && s.sibo.is_none() && s.focus == Focus::List {
            gu.cursor(x + 3.0, y + 6.0 + 18.0 * (s.list.0 - s.list.1) as f32);
        }
        let (ox, oy) = (x + 3.0, y + 4.0);
        for (k, r) in list.iter().skip(s.list.1).take(VISIBLE).enumerate() {
            let ry = oy + 18.0 * k as f32;
            match *r {
                Row::Type => _ = gu.sprite("menu    framesus", 159, ox, ry + 2.0, WHITE),
                Row::Item(id) => {
                    let icon = crate::render::invwin::icon_texture(id);
                    if gu.has_image(&icon) {
                        gu.image_rect(&icon, ox, ry, 16.0, 16.0, [1.0; 4]);
                    }
                    gu.text(&item_name(w, id), ox + 18.0, ry + 2.0, 0, tint(w, s, *r).to_le_bytes());
                }
            }
        }
        let f = win.frame;
        gu.scroll_bar(f[0] + f[2] - 8.0, f[1], f[3], list.len(), VISIBLE, s.list.1);
        if s.focus == Focus::List {
            gu.bar_site(super::scrollbar::Owner::Maze, f[0] + f[2] - 8.0, f[1], f[3], list.len(), VISIBLE, s.list.1);
        }
        rects.push(f);
    }
    rects.extend(draw_help(gu, w, s, sw, sh));
    if let Some(r) = s.sibo
        && let Some(win) = gu.lobby_window_off(SIBO, sw, sh, (0.0, 0.0), Some(r), w.confirm.is_none(), &[])
    {
        rects.push(win.frame);
    }
    rects
}

/// Title slot 2 "Tabula" (0xd 0x96) while the list source is open; help the list row's, prvdunsi's override, nothing
/// for the grid.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.maze.screen else { return };
    bars.title[1] = gu.title_msg(0x96).unwrap_or_default();
    bars.help[1] = match (s.sibo, s.focus) {
        (Some(_), _) => gu.menu_msg(0x2a4, ""),
        (None, Focus::List) => row_help(gu, w, s),
        (None, Focus::Grid) => String::new(),
    };
}

/// The scroll bar or wheel on the list.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(mut s) = w.maze.screen.take() else { return false };
    let n = rows(w, &s).len();
    let mut row = s.list.0.saturating_sub(s.list.1);
    let moved = super::scrollbar::paged(m, &mut s.list.1, &mut row, n, VISIBLE);
    s.list.0 = s.list.1 + row;
    w.maze.screen = Some(s);
    moved
}

/// prvdunsi's cursor children kept (FUN_10118830).
pub(super) fn kept(w: &World) -> Option<(&'static str, u16)> {
    w.maze.screen.as_ref()?.sibo.map(|r| (SIBO, r))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Sibo(u16),
    Cell(i8, i8),
    Row(usize),
}

fn hit(gu: &GameUi, sw: f32, sh: f32, p: (f32, f32)) -> Option<Hit> {
    let inside = |r: [f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    let child = |name: &str, off: (f32, f32)| -> Option<u16> {
        let (x, y) = gu.window_xy(name, sw, sh)?;
        let m = gu.layout(name)?;
        m.elements.iter().skip(1).find(|e| inside([x + off.0 + e.x as f32, y + off.1 + e.y as f32, e.w as f32, e.h as f32])).map(|e| e.id)
    };
    if let Some(c) = child(SIBO, (0.0, 0.0)) {
        return Some(Hit::Sibo(c));
    }
    if let Some(c) = child(GRID, (0.0, 0.0)).filter(|c| (1..=25).contains(c)) {
        return Some(Hit::Cell(((c - 1) % 5) as i8, ((c - 1) / 5) as i8));
    }
    let off = gu.moved_off(INVEN, sw, sh, INVEN_AT.0, INVEN_AT.1);
    let (x, y) = gu.window_xy(INVEN, sw, sh)?;
    (0..VISIBLE).find(|&k| inside([x + off.0 + 3.0, y + off.1 + 6.0 + 18.0 * k as f32, 132.0, 16.0])).map(Hit::Row)
}

/// Mouse *(inf: the generic handlers act on the focused window)*: prvdunsi's buttons while it is open; while a rune is
/// carried a cell moves it there (message 9), a click places it, a right click drops it, a middle click turns it; the
/// list's rows move the cursor, a click on the cursor's row is Enter, a right click Escape.
#[allow(clippy::too_many_arguments)]
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool, middle: bool) {
    let Some(s) = w.maze.screen.as_ref().filter(|_| !w.modern) else { return };
    let (sibo, focus, list) = (s.sibo, s.focus, s.list);
    let Some(h) = hit(gu, sw, sh, p) else { return };
    gu.hover.set(true);
    match (sibo, focus, h) {
        (Some(r), _, Hit::Sibo(c)) => {
            if c != r && (moved || clicked) && (1..=11).contains(&c) {
                set_sibo(w, c);
                w.sounds.push(Ui::Cursor as u32);
            }
            if clicked {
                key(w, egui::Key::Enter);
            }
        }
        (None, Focus::Grid, Hit::Cell(c, r)) => {
            if moved || clicked {
                set_cell(w, c, r);
            }
            if clicked {
                key(w, egui::Key::Enter);
            } else if right {
                key(w, egui::Key::Escape);
            } else if middle {
                rotate(w);
            }
        }
        (None, Focus::List, Hit::Row(k)) => {
            let n = w.maze.screen.as_ref().map_or(0, |s| rows(w, s).len());
            if list.1 + k == list.0 {
                if clicked {
                    key(w, egui::Key::Enter);
                }
            } else if (moved || clicked) && list.1 + k < n {
                set_row(w, list.1 + k);
                w.sounds.push(Ui::Cursor as u32);
            }
            if right {
                key(w, egui::Key::Escape);
            }
        }
        _ => {}
    }
}

/// The screen as the modern layer draws it.
pub(super) struct View {
    pub edit: bool,
    pub tabula: String,
    pub voucher: Option<(u16, String)>,
    pub mode: Mode,
    pub grid: [i8; 25],
    /// Placed runes: (rune, element, cells, on its element).
    pub placed: Vec<(u16, u8, u32, bool)>,
    /// The held rune: (rune, element, cells, carried, fits).
    pub held: Option<(u16, u8, u32, bool, bool)>,
    pub list: Vec<(Row, String, u32)>,
    pub cursor: usize,
    pub on_grid: bool,
    pub help: Option<String>,
    pub help_line: String,
    pub sibo: Option<u16>,
    pub filter: Option<u8>,
    pub pulse: f32,
}

pub(super) fn view(gu: &GameUi, w: &World) -> Option<View> {
    let s = w.maze.screen.as_ref()?;
    let g = grid(w, s.tabula);
    let placed = s.placed.iter().flatten().filter_map(|p| rune(w, p.rune).map(|(e, _)| (p.rune, e, cells_at(w, p.rune, p.col, p.row, p.rot), bonus(w, &g, p)))).collect();
    let held = s.held.and_then(|(r, carry)| {
        let (e, _) = rune(w, r)?;
        let cells = cells_at(w, r, s.at.0, s.at.1, s.rot);
        Some((r, e, cells, carry, !carry || cells & occupied(w, s) == 0))
    });
    let list = rows(w, s)
        .into_iter()
        .map(|r| {
            let text = match r {
                Row::Type => gu.sprite_caption("menu    framesus", 159).unwrap_or_else(|| "Type".into()),
                Row::Item(id) => item_name(w, id),
            };
            (r, text, tint(w, s, r))
        })
        .collect();
    Some(View {
        edit: s.edit,
        tabula: item_name(w, s.tabula),
        voucher: (s.voucher != 0).then(|| (s.voucher, item_name(w, s.voucher))),
        mode: s.mode,
        grid: g,
        placed,
        held,
        list,
        cursor: s.list.0,
        on_grid: s.focus == Focus::Grid,
        help: help_text(w, s).map(|t| t.0),
        help_line: if s.focus == Focus::List { row_help(gu, w, s) } else { String::new() },
        sibo: s.sibo,
        filter: s.filter,
        pulse: s.pulse.0 as f32 / 128.0,
    })
}

/// The list's cursor onto row `k` while it has the keys. True when it moved.
pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(mut s) = w.maze.screen.take() else { return false };
    let n = rows(w, &s).len();
    let moved = s.focus == Focus::List && s.sibo.is_none() && k < n && k != s.list.0;
    if moved {
        list_to(&mut s, k);
    }
    w.maze.screen = Some(s);
    moved
}

/// A carried rune's top left onto cell (col, row) (message 9).
pub(super) fn set_cell(w: &mut World, col: i8, row: i8) {
    let Some(mut s) = w.maze.screen.take() else { return };
    if s.focus == Focus::Grid && s.sibo.is_none() {
        move_to(w, &mut s, (col, row));
    }
    w.maze.screen = Some(s);
}

/// prvdunsi's cursor onto child `c` (1..11). True when it moved.
pub(super) fn set_sibo(w: &mut World, c: u16) -> bool {
    let Some(cur) = w.maze.screen.as_mut().and_then(|s| s.sibo.as_mut()).filter(|r| **r != c && (1..=11).contains(&c)) else { return false };
    *cur = c;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_cells() {
        let mut g = [-1i8; 25];
        (g[6], g[18]) = (-2, -2);
        assert_eq!(blocked(&g), 1 << 18 | 1 << 6);
        assert!(bit(&[0x02], 1) && !bit(&[0x02], 0) && !bit(&[], 3));
    }
}
