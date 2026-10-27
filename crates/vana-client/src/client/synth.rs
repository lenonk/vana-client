//! Synthesis: "menu    tskill1" with the inventory list as the material picker (FUN_101e0e30), c2s 0x096, the answer
//! s2c 0x06F (a pre-hook that calls the sender's callback, LAB_101e0940), s2c 0x070 (others' results) and the s2c
//! 0x030 animation (FUN_1009f4c0). MODLOG "Synthesis". The history, /lastsynth and "menu    cmbmenu" are not done.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use super::post::{draw_list, draw_rows, draw_slots, link, log_group9, picker_rows, system_line};
use crate::audio::Ui;
use crate::fx::sched::Who;
use std::time::{Duration, Instant};

pub(super) const TSKILL1: &str = "menu    tskill1";
pub(super) const CMBMENU: &str = "menu    cmbmenu";
const CMBHLST: &str = "menu    cmbhlst";
pub(super) const CMBHWIN: &str = "menu    cmbhwin";
const ITEMCTRL: &str = "menu    itemctrl";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Focus {
    Slots,
    List,
    Qty,
    /// The history list or the favourites window has the focus (tskill1 stays open, mode 2).
    Away,
    /// Ours (not retail): how many to make, in itemctrl: (value, most sets held, typed since open).
    Amount(u32, u32, bool),
}

/// Where tskill1 was opened from (window +0x70 mode and the list source +0x4c): Cancel goes back there.
#[derive(Clone, Copy, Debug, PartialEq)]
enum From {
    Items,
    Direct,
    Crystals,
    History,
}

/// "menu    cmbmenu" (FUN_101e2950): its cursor and the crystal list over it (the crystal source DAT_106634c0).
pub struct Menu {
    sel: u16,
    crystals: Option<(usize, usize)>,
    /// cmbhlst's cursor and cmbhwin's when it has the focus.
    history: Option<(u16, Option<u16>)>,
}

/// A history record (FUN_101e1b40): Result, Grade, ItemNo, CrystalNo (bit 15 the favourite), MaterialNo[8].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Record {
    pub result: u8,
    pub grade: i8,
    pub item: u16,
    pub crystal: u16,
    pub materials: [u16; 8],
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
    from: From,
    /// Mode 2 (the refresh struct DAT_106634d4): the crystal found, the materials missing.
    found: bool,
    missing: [bool; 10],
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
    /// Ours (not retail): the recipe being repeated and how many are still to go.
    repeat: Option<(Record, u32)>,
    pub menu: Option<Menu>,
    /// DAT_106634b8: the last ten synthesises, newest first; read once per login.
    pub history: Vec<Record>,
    pub loaded: bool,
    request: Option<(u8, Instant)>,
    /// tskill1 +0x74 (FUN_101e15e0): when the last request went out; kept over zones.
    sent: Option<Instant>,
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
    /// 0x06F CrystalNo @0x22, MaterialNo[8] @0x24.
    pub crystal: u16,
    pub materials: [u16; 8],
}

pub fn active(w: &World) -> bool {
    w.synth.win.is_some() || w.synth.menu.is_some()
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
    let from = if w.synth.menu.as_ref().is_some_and(|m| m.crystals.is_some()) {
        From::Crystals
    } else if matches!(w.win, Some(super::items::Win::Items { direct: true, .. })) {
        From::Direct
    } else {
        From::Items
    };
    w.win = None;
    w.synth.win = Some(Win { crystal: (index, id), slots: [(0, 0, 0); 10], cursor: 1, focus: Focus::Slots, list: (0, 0), qty: None, from, found: true, missing: [false; 10] });
    true
}

/// A zone change drops the windows, the request and the animations; the history stays for the login.
pub fn zone_reset(w: &mut World) {
    let (history, loaded, sent) = (std::mem::take(&mut w.synth.history), w.synth.loaded, w.synth.sent);
    w.synth = State { history, loaded, sent, ..Default::default() };
}

/// Main menu row 10 (FUN_10140400 case 10): "menu    cmbmenu", SE 0002.
pub fn open_menu(w: &mut World) {
    w.synth.menu = Some(Menu { sel: 1, crystals: None, history: None });
    w.menu = None;
    w.sounds.push(Ui::Select as u32);
}

/// FUN_101e15c0: a request went out less than 23 s ago (+0x74, FUN_101e15e0); cmbmenu's rows (FUN_101e28d0) and the
/// main menu's Synthesis (FUN_10140270) are off till then, whether or not it was answered.
pub(super) fn menu_off(w: &World) -> bool {
    w.synth.sent.is_some_and(|t| t.elapsed() < Duration::from_secs(23))
}

/// The crystal source's rows: crystals only (FUN_101e2a40 colours type 8 yellow; the others are not listed, inferred).
/// The focused synthesis picker or crystal list (true), for its bar and the wheel.
pub(super) fn picker(w: &mut World, confirm: bool) -> Option<(&mut (usize, usize), bool)> {
    if let Some(s) = w.synth.win.as_mut().filter(|s| s.focus == Focus::List && !confirm && s.from != From::History) {
        return Some((&mut s.list, false));
    }
    w.synth.menu.as_mut().filter(|m| m.history.is_none()).and_then(|m| m.crystals.as_mut()).map(|l| (l, true))
}

pub(super) fn crystal_count(w: &World) -> usize {
    crystal_rows(w).len()
}

fn crystal_rows(w: &World) -> Vec<crate::inventory::Slot> {
    let info = w.item_info.as_ref();
    picker_rows(w).into_iter().filter(|r| info.and_then(|c| c.item(r.id)).is_some_and(|it| it.item_type == 8)).collect()
}

/// cmbmenu (FUN_101e29a0) and its crystal list (FUN_101e2ac0 -> FUN_101e2a60: gate 0x77, then tskill1 mode 1).
fn menu_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut m) = w.synth.menu.take() else { return };
    if let Some((cur, fav)) = m.history {
        let n = w.synth.history.len() as u16;
        match (fav, key) {
            // cmbhwin (FUN_101e32a0): 1 marks the row a favourite, 2 unmarks it; both disabled with no row.
            (Some(f), K::ArrowUp | K::ArrowDown) => {
                m.history = Some((cur, Some(3 - f)));
                w.sounds.push(Ui::Cursor as u32);
            }
            (Some(_), K::Enter) if n == 0 => w.sounds.push(Ui::Error as u32),
            (Some(f), K::Enter) => {
                if let Some(r) = w.synth.history.get_mut(cur as usize - 1) {
                    r.crystal = if f == 1 { r.crystal | 0x8000 } else { r.crystal & 0x7fff };
                }
                m.history = Some((cur, None));
                w.sounds.push(Ui::Select as u32);
            }
            (Some(_), K::Escape) => {
                m.history = Some((cur, None));
                w.sounds.push(Ui::Close as u32);
            }
            // cmbhlst (FUN_101e2cb0): moves refresh tskill1; Enter focuses it on Synthesize; Escape back to cmbmenu.
            (None, K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft) => {
                let to = link(gu, CMBHLST, cur, key);
                if to != cur && to != 0 && to <= n {
                    m.history = Some((to, None));
                    w.sounds.push(Ui::Cursor as u32);
                    w.synth.menu = Some(m);
                    refresh(w);
                    return;
                }
            }
            (None, K::Enter) if n > 0 => {
                if let Some(s) = w.synth.win.as_mut() {
                    (s.focus, s.cursor) = (Focus::Slots, 9);
                }
                w.sounds.push(Ui::Select as u32);
            }
            (None, K::Escape) => {
                m.history = None;
                w.synth.win = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        w.synth.menu = Some(m);
        return;
    }
    if let Some((mut sc, mut cu)) = m.crystals {
        let rows = crystal_rows(w);
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
                if let Some(r) = rows.get(sc + cu).cloned() {
                    m.crystals = Some((sc, cu));
                    w.synth.menu = Some(m);
                    if super::textcmd::status_gate(w, 0x77) && open(w, r.index, r.id) {
                        w.sounds.push(Ui::Select as u32);
                    } else {
                        w.sounds.push(Ui::Error as u32);
                    }
                    return;
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                m.crystals = None;
                w.synth.menu = Some(m);
                return;
            }
            _ => {}
        }
        m.crystals = Some((sc, cu));
        w.synth.menu = Some(m);
        return;
    }
    match key {
        K::ArrowUp | K::ArrowDown => {
            let to = link(gu, CMBMENU, m.sel, key);
            if to != m.sel && to != 0 {
                m.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter if menu_off(w) => w.sounds.push(Ui::Error as u32),
        K::Enter if m.sel == 1 => {
            w.sounds.push(Ui::Select as u32);
            m.crystals = Some((0, 0));
        }
        // Row 2 (FUN_101e1000): cmbhwin, cmbhlst at (200, 48) focused, tskill1 in mode 2 on the newest record.
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            m.history = Some((1, None));
            w.synth.menu = Some(m);
            w.synth.win = Some(Win { crystal: (0, 0), slots: [(0, 0, 0); 10], cursor: 9, focus: Focus::Away, list: (0, 0), qty: None, from: From::History, found: false, missing: [false; 10] });
            refresh(w);
            return;
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            return;
        }
        _ => {}
    }
    w.synth.menu = Some(m);
}

/// Key function 0x11 (Num+, list message 8): cmbhlst and cmbhwin swap the focus, SE 0013.
pub fn num_plus(w: &mut World) {
    let away = w.synth.win.as_ref().is_some_and(|s| s.focus == Focus::Away);
    if let Some(m) = w.synth.menu.as_mut()
        && let Some((cur, fav)) = m.history
        && away
    {
        m.history = Some((cur, if fav.is_some() { None } else { Some(1) }));
        w.sounds.push(Ui::Close as u32);
    }
}

/// FUN_101e27d0 (FUN_101e2040 / FUN_101e2320): tskill1 takes the cursor record's crystal and materials from the
/// inventory; a material not held keeps its id with no stack (index -1 - i), the same stacks merged.
fn refresh(w: &mut World) {
    let Some(cur) = w.synth.menu.as_ref().and_then(|m| m.history).map(|h| h.0) else { return };
    let Some(r) = w.synth.history.get(cur as usize - 1).copied() else { return };
    let unlocked = |i: u8| w.inv.locks.get(&(0, i)).copied().unwrap_or(0) == 0;
    let crystal_id = r.crystal & 0x7fff;
    let crystal = w.inv.slots.iter().find(|((c, i), (id, n))| *c == 0 && *i != 0 && *id == crystal_id && *n > 0 && unlocked(*i)).map(|(&(_, i), _)| i);
    let mut slots = [(0u8, 0u16, 0u32); 10];
    let mut missing = [false; 10];
    let mut k = 0;
    for m in r.materials.iter().copied().filter(|m| *m != 0) {
        let used = |slots: &[(u8, u16, u32); 10], i: u8| slots.iter().find(|s| s.0 == i).map_or(0, |s| s.2);
        let at = w.inv.slots.iter().find(|((c, i), (id, n))| *c == 0 && *i != 0 && *id == m && unlocked(*i) && Some(*i) != crystal && used(&slots, *i) < *n).map(|(&(_, i), _)| i);
        match at {
            Some(i) if slots.iter().any(|s| s.0 == i) => {
                if let Some(s) = slots.iter_mut().find(|s| s.0 == i) {
                    s.2 += 1;
                }
            }
            Some(i) if k < 10 => {
                slots[k] = (i, m, 1);
                k += 1;
            }
            None if k < 10 => {
                (slots[k], missing[k]) = ((0, m, 0), true);
                k += 1;
            }
            _ => {}
        }
    }
    if let Some(s) = w.synth.win.as_mut() {
        (s.crystal, s.found, s.slots, s.missing) = ((crystal.unwrap_or(0), crystal_id), crystal.is_some(), slots, missing);
    }
}

/// FUN_101e1200: Synthesize is enabled in mode 2 only with the crystal and every material held.
fn complete(s: &Win) -> bool {
    s.from != From::History || (s.found && s.crystal.0 != 0 && !s.missing.iter().any(|m| *m))
}

/// FUN_101e1a70: cmb0.dat (newest) .. cmb9.dat, version 1, 0x20 bytes each; an unreadable or invalid one is skipped,
/// the others appended in order (with the dedupe).
pub fn load_history(w: &mut World, dir: &std::path::Path) {
    w.synth.loaded = true;
    w.synth.history.clear();
    for n in 0..10 {
        let Some(b) = super::macros::read_file(&dir.join(format!("cmb{n}.dat")), 1) else { continue };
        if b.len() < 0x20 || u32::from_le_bytes([b[0x1c], b[0x1d], b[0x1e], b[0x1f]]) == 0 {
            continue;
        }
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let r = Record { result: b[4], grade: b[5] as i8, item: u16_at(6), crystal: u16_at(8), materials: std::array::from_fn(|i| u16_at(0xa + 2 * i)) };
        if !w.synth.history.iter().any(|x| x.crystal & 0x7fff == r.crystal & 0x7fff && x.materials == r.materials) && w.synth.history.len() < 10 {
            w.synth.history.push(r);
        }
    }
}

/// FUN_101e1ae0 (on every s2c 0x00B and at teardown): record i to cmb{i}.dat, state 2, valid 1.
pub fn save_history(w: &World, dir: &std::path::Path) {
    if !w.synth.loaded {
        return;
    }
    for (n, r) in w.synth.history.iter().enumerate() {
        let mut b = vec![0u8; 0x20];
        b[0] = 2;
        (b[4], b[5]) = (r.result, r.grade as u8);
        b[6..8].copy_from_slice(&r.item.to_le_bytes());
        b[8..10].copy_from_slice(&r.crystal.to_le_bytes());
        for (i, m) in r.materials.iter().enumerate() {
            b[0xa + 2 * i..0xc + 2 * i].copy_from_slice(&m.to_le_bytes());
        }
        b[0x1c] = 1;
        super::macros::write_file(&dir.join(format!("cmb{n}.dat")), 1, &b);
    }
}

/// FUN_101e1b40 / FUN_101e1bd0 / FUN_101e1d40: the answer goes to the front; the same crystal and materials replace
/// their record (a failure does not replace a success; the favourite stays); a full list loses its oldest record
/// that is not a favourite.
fn add_history(w: &mut World, a: &Answer) {
    let mut r = Record { result: a.result, grade: a.grade, item: a.item, crystal: a.crystal & 0x7fff, materials: a.materials };
    let h = &mut w.synth.history;
    if let Some(k) = h.iter().position(|x| x.crystal & 0x7fff == r.crystal && x.materials == r.materials) {
        let old = h.remove(k);
        r.crystal |= old.crystal & 0x8000;
        if r.item == 0x73ff && old.item != 0x73ff {
            (r.result, r.grade, r.item) = (old.result, old.grade, old.item);
        }
    } else if h.len() >= 10 {
        match h.iter().rposition(|x| x.crystal & 0x8000 == 0) {
            Some(k) => {
                h.remove(k);
            }
            None => return,
        }
    }
    h.insert(0, r);
}

/// /lastsynth (FUN_1007f990): after SendResRdy; no history: group 7 0xc0. Alone: refused with tskill1 open (0x45),
/// the gate 0x77, engaged nothing; then the newest record's crystal and materials are looked up in the inventory
/// (unlocked, a unit per count) and sent as tskill1 would, or group 9 0xe "Synthesis canceled.". "check" prints the
/// record (file 7031 0x10f item, 0x110 crystal, 0x111 per material with its count).
pub fn lastsynth(w: &mut World, args: &[&str]) -> std::result::Result<(), ()> {
    if !w.res_rdy.sent {
        return Ok(());
    }
    let Some(r) = w.synth.history.first().copied() else {
        super::textcmd::refuse(w, 0xc0);
        return Ok(());
    };
    match args {
        [_] => {
            if w.synth.win.is_some() {
                super::textcmd::refuse(w, 0x45);
                return Ok(());
            }
            if !super::textcmd::status_gate(w, 0x77) || w.my_motion.status == 1 {
                return Ok(());
            }
            match find_units(w, &r) {
                Some((crystal, slots)) => {
                    send(w, crystal, &slots);
                }
                None => log_group9(w, 0xe, 0x79),
            }
            Ok(())
        }
        [_, c] if c.eq_ignore_ascii_case("check") => {
            system_line(w, 0x10f, [r.item as i32, 0, 0, 0, 0], String::new());
            system_line(w, 0x110, [(r.crystal & 0x7fff) as i32, 0, 0, 0, 0], String::new());
            let mut seen: Vec<u16> = Vec::new();
            for m in r.materials.iter().copied().filter(|m| *m != 0) {
                if !seen.contains(&m) {
                    seen.push(m);
                    let n = r.materials.iter().filter(|x| **x == m).count();
                    system_line(w, 0x111, [m as i32, n as i32, 0, 0, 0], String::new());
                }
            }
            Ok(())
        }
        _ => Err(()),
    }
}

/// FUN_101e2040 / FUN_101e2300: each unit from an unlocked stack with uses left, merged per stack.
fn find_units(w: &World, r: &Record) -> Option<((u8, u16), [(u8, u16, u32); 10])> {
    let unlocked = |i: u8| w.inv.locks.get(&(0, i)).copied().unwrap_or(0) == 0;
    let crystal_id = r.crystal & 0x7fff;
    let crystal = w.inv.slots.iter().find(|((c, i), (id, n))| *c == 0 && *i != 0 && *id == crystal_id && *n > 0 && unlocked(*i)).map(|(&(_, i), _)| i)?;
    let mut slots = [(0u8, 0u16, 0u32); 10];
    for m in r.materials.iter().copied().filter(|m| *m != 0) {
        let used = |i: u8| slots.iter().find(|s| s.0 == i).map_or(0, |s| s.2);
        let (&(_, i), _) = w.inv.slots.iter().find(|((c, i), (id, n))| *c == 0 && *i != 0 && *id == m && unlocked(*i) && (*i != crystal || *id != crystal_id) && used(*i) < *n)?;
        match slots.iter_mut().find(|s| s.0 == i) {
            Some(s) => s.2 += 1,
            None => *slots.iter_mut().find(|s| s.0 == 0)? = (i, m, 1),
        }
    }
    Some(((crystal, crystal_id), slots))
}

/// FUN_100eb800: an empty inventory entry 1..size - 1.
pub(super) fn free_slot(w: &World) -> bool {
    let size = w.inv.sizes[0] as usize;
    (1..size).any(|i| w.inv.slots.get(&(0, i as u8)).is_none_or(|s| s.0 == 0))
}

/// FUN_101e0f20(1): Cancel goes back to the list it came from.
fn cancel(w: &mut World) {
    let Some(s) = w.synth.win.take() else { return };
    match s.from {
        From::Direct => super::items::open_direct(w),
        From::Items => super::items::open_items(w),
        From::Crystals | From::History => {}
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
    w.synth.sent = Some(Instant::now());
    true
}

/// tskill1 (FUN_101e1530), the picker (FUN_101e0600) and itemctrl.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut s) = w.synth.win.take() else { return menu_key(w, gu, key) };
    if s.focus == Focus::Away {
        w.synth.win = Some(s);
        return menu_key(w, gu, key);
    }
    match s.focus {
        Focus::Away => {}
        Focus::Slots => match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = link(gu, TSKILL1, s.cursor, key);
                if to != s.cursor && to != 0 {
                    s.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            // FUN_101e08e0: the gate (0x77), the inventory check (silent), the send; then everything closes. With the
            // Synthesis Amount extra it first asks how many (1 ready, so Enter goes on).
            K::Enter if s.cursor == 9 => {
                if !(complete(&s) && super::textcmd::status_gate(w, 0x77) && still_held(w, &s)) {
                    w.sounds.push(Ui::Error as u32);
                } else if w.synth_amount {
                    w.sounds.push(Ui::Select as u32);
                    s.focus = Focus::Amount(1, sets_held(w, &s), false);
                } else if send(w, s.crystal, &s.slots) {
                    w.sounds.push(Ui::Select as u32);
                    // Modes 1 and 2 also close the cmbmenu chain (FUN_1015e910 / cmbhlst).
                    if matches!(s.from, From::Crystals | From::History) {
                        w.synth.menu = None;
                    }
                    return;
                } else {
                    w.sounds.push(Ui::Error as u32);
                }
            }
            // Mode 2's Cancel gives the history list the focus back.
            K::Enter if s.cursor == 10 && s.from == From::History => {
                w.sounds.push(Ui::Select as u32);
                s.focus = Focus::Away;
            }
            K::Enter if s.cursor == 10 => {
                w.sounds.push(Ui::Select as u32);
                w.synth.win = Some(s);
                cancel(w);
                return;
            }
            K::Enter if s.from == From::History => {}
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
        Focus::Amount(mut v, max, mut typed) => {
            let digit = match key {
                K::Num0 => Some(0),
                K::Num1 => Some(1),
                K::Num2 => Some(2),
                K::Num3 => Some(3),
                K::Num4 => Some(4),
                K::Num5 => Some(5),
                K::Num6 => Some(6),
                K::Num7 => Some(7),
                K::Num8 => Some(8),
                K::Num9 => Some(9),
                _ => None,
            };
            match (key, digit) {
                (_, Some(d)) => {
                    v = if typed { v * 10 + d } else { d }.clamp(1, max);
                    typed = true;
                    w.sounds.push(Ui::Cursor as u32);
                }
                (K::ArrowUp, _) => v = (v + 1).min(max),
                (K::ArrowDown, _) => v = v.saturating_sub(1).max(1),
                (K::ArrowRight, _) => v = 1,
                (K::ArrowLeft, _) => v = max,
                (K::Backspace, _) => v = (v / 10).max(1),
                (K::Enter, _) => {
                    if send(w, s.crystal, &s.slots) {
                        w.sounds.push(Ui::Select as u32);
                        if v > 1 {
                            w.synth.repeat = Some((recipe(&s), v - 1));
                        }
                        // Modes 1 and 2 also close the cmbmenu chain (FUN_1015e910 / cmbhlst).
                        if matches!(s.from, From::Crystals | From::History) {
                            w.synth.menu = None;
                        }
                        return;
                    }
                    w.sounds.push(Ui::Error as u32);
                    s.focus = Focus::Slots;
                }
                (K::Escape, _) => {
                    w.sounds.push(Ui::Cancel as u32);
                    s.focus = Focus::Slots;
                }
                _ => {}
            }
            if matches!(key, K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft | K::Backspace) {
                w.sounds.push(Ui::Cursor as u32);
            }
            if matches!(s.focus, Focus::Amount(..)) {
                s.focus = Focus::Amount(v, max, typed);
            }
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

/// The mouse (winhits.rs): cmbmenu, cmbhwin, cmbhlst's rows and tskill1 as the generic window; the crystal list and
/// the picker as the item list (hover moves the row, a click on it is Enter, a right click Escape); itemctrl only the
/// right click.
#[allow(clippy::too_many_arguments)]
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::{generic, over};
    if w.modern {
        return;
    }
    let keys = |w: &mut World, gu: &GameUi, k: egui::Key| key(w, gu, k);
    // A picker row: hover moves the cursor, a click on it is Enter.
    let list = |w: &mut World, (scroll, cursor): (usize, usize), n: usize, set: fn(&mut World, usize)| {
        let row = super::post::picker_row(gu, sw, sh, p);
        if right && row.is_some() {
            key(w, gu, egui::Key::Escape);
            return;
        }
        let Some(r) = row.filter(|r| scroll + r < n) else { return };
        gu.hover.set(true);
        if r != cursor && (moved || clicked) {
            set(w, r);
            w.sounds.push(Ui::Cursor as u32);
        } else if clicked {
            key(w, gu, egui::Key::Enter);
        }
    };
    match w.synth.win.as_ref().map(|s| (s.focus, s.cursor, s.list)) {
        Some((Focus::Slots, cursor, _)) => generic(w, gu, TSKILL1, cursor, 1..=10, p, moved, clicked, right, |w, id| {
            if let Some(s) = w.synth.win.as_mut() {
                s.cursor = id;
            }
        }, keys),
        Some((Focus::List, _, at)) => {
            let n = picker_rows(w).len();
            list(w, at, n, |w, r| {
                if let Some(s) = w.synth.win.as_mut() {
                    s.list.1 = r;
                }
            });
        }
        Some((Focus::Qty | Focus::Amount(..), ..)) => {
            if right && over(w, ITEMCTRL, p) {
                key(w, gu, egui::Key::Escape);
            }
        }
        Some((Focus::Away, ..)) | None => {
            let Some(m) = w.synth.menu.as_ref() else { return };
            let n = w.synth.history.len() as u16;
            match (m.history, m.crystals) {
                (Some((cur, Some(f))), _) => generic(w, gu, CMBHWIN, f, 1..=2, p, moved, clicked, right, |w, id| {
                    if let Some(m) = w.synth.menu.as_mut() {
                        m.history = Some((cur, Some(id)));
                    }
                }, keys),
                (Some((cur, None)), _) => generic(w, gu, CMBHLST, cur, 1..=n.max(1), p, moved, clicked, right, |w, id| {
                    if let Some(m) = w.synth.menu.as_mut() {
                        m.history = Some((id, None));
                    }
                    refresh(w);
                }, keys),
                (None, Some(at)) => {
                    let n = crystal_rows(w).len();
                    list(w, at, n, |w, r| {
                        if let Some(c) = w.synth.menu.as_mut().and_then(|m| m.crystals.as_mut()) {
                            c.1 = r;
                        }
                    });
                }
                (None, None) if w.synth.win.is_none() => {
                    let sel = m.sel;
                    generic(w, gu, CMBMENU, sel, 1..=2, p, moved, clicked, right, |w, id| {
                        if let Some(m) = w.synth.menu.as_mut() {
                            m.sel = id;
                        }
                    }, keys);
                }
                _ => {}
            }
        }
    }
}

/// Ours: how many complete sets (the crystal and every material at its count) the unlocked stacks hold, 1..99.
fn sets_held(w: &World, s: &Win) -> u32 {
    let held = |id: u16| -> u32 { w.inv.slots.iter().filter(|((c, i), (x, _))| *c == 0 && *i != 0 && *x == id && w.inv.locks.get(&(0, *i)).copied().unwrap_or(0) == 0).map(|(_, (_, n))| *n).sum() };
    let mut need: Vec<(u16, u32)> = vec![(s.crystal.1, 1)];
    for &(index, id, count) in s.slots.iter().filter(|x| x.0 != 0 && x.2 != 0) {
        let _ = index;
        match need.iter_mut().find(|n| n.0 == id) {
            Some(n) => n.1 += count,
            None => need.push((id, count)),
        }
    }
    need.iter().map(|&(id, n)| held(id) / n.max(1)).min().unwrap_or(1).clamp(1, 99)
}

/// The request as a record (its crystal and one id per unit) for the repeats.
fn recipe(s: &Win) -> Record {
    let mut materials = [0u16; 8];
    let mut k = 0;
    for &(_, id, count) in s.slots.iter().filter(|x| x.0 != 0) {
        for _ in 0..count {
            if k < 8 {
                materials[k] = id;
                k += 1;
            }
        }
    }
    Record { crystal: s.crystal.1, materials, ..Default::default() }
}

/// Ours: the next of the repeats once the last one is over (its answer in and the 2 s lock gone, our animation done,
/// out of status 0x2c, no synthesis window), from fresh stacks; missing materials stop it with group 9 0xe.
fn repeat_frame(w: &mut World) {
    let Some((r, left)) = w.synth.repeat else { return };
    if left == 0 {
        w.synth.repeat = None;
        return;
    }
    let busy = w.synth.request.is_some() || w.synth.anims.contains_key(&w.my_id) || w.my_motion.status == 0x2c || w.synth.win.is_some();
    if busy {
        return;
    }
    match find_units(w, &r) {
        Some((crystal, slots)) if send(w, crystal, &slots) => w.synth.repeat = Some((r, left - 1)),
        Some(_) => w.synth.repeat = None,
        None => {
            w.synth.repeat = None;
            log_group9(w, 0xe, 0x79);
        }
    }
}

/// s2c 0x06F: the waiting request's callback (LAB_101e0940) by Result, then the request is answered. Ours: anything but
/// a result (success or failure) ends the repeats.
pub fn on_result(w: &mut World, a: Answer) {
    if !matches!(w.synth.request, Some((2, _))) {
        return;
    }
    if !matches!(a.result, 0 | 1 | 0xc) {
        w.synth.repeat = None;
    }
    w.synth.request = Some((4, Instant::now()));
    let line9 = |w: &mut World, id| log_group9(w, id, 0x79);
    match a.result {
        0 | 1 | 0xc => {
            if a.result == 1 {
                line9(w, 0xa);
            }
            add_history(w, &a);
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
    repeat_frame(w);
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
        if let Some(x) = w.actors.get_mut(&id)
            && super::actmotion::start(x, &ch, name, &base)
        {
            super::actmotion::fx_too(&mut w.effects, id, name);
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
    let Some(s) = &w.synth.win else {
        if let Some(m) = &w.synth.menu {
            let [help, title] = gu.focus_text(CMBMENU, m.sel);
            (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
            if m.crystals.is_some() {
                bars.help[1] = gu.menu_msg(3, "");
            }
        }
        return;
    };
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    match s.focus {
        Focus::Slots => focus(TSKILL1, s.cursor, bars),
        Focus::Away => {
            if let Some((cur, fav)) = w.synth.menu.as_ref().and_then(|m| m.history) {
                match fav {
                    Some(f) => {
                        let name = w.synth.history.get(cur as usize - 1).and_then(|r| w.item_info.as_ref()?.item(r.item)).map(|it| it.name.clone()).unwrap_or_default();
                        focus(CMBHWIN, f, bars);
                        bars.help[1] = bars.help[1].replacen("%s", &name, 1);
                    }
                    None => {
                        focus(CMBHLST, cur, bars);
                        bars.help[1] = gu.menu_msg(0x1ed, "");
                    }
                }
            }
        }
        Focus::List => {
            focus("menu    inventor", s.list.1 as u16 + 1, bars);
            bars.help[1] = gu.menu_msg(7, "");
        }
        Focus::Qty | Focus::Amount(..) => {
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
            Focus::Slots | Focus::Amount(..) => s.slots.get((s.cursor as usize).checked_sub(1)?)?.0,
            // FUN_101e2e90: the history row's ItemNo.
            Focus::Away => {
                let (cur, _) = w.synth.menu.as_ref()?.history?;
                return w.synth.history.get(cur as usize - 1).map(|r| (r.item, None));
            }
        };
        let &(id, _) = w.inv.slots.get(&(0, index)).filter(|_| index != 0)?;
        return Some((id, w.inv.ext.get(&(0, index)).copied()));
    }
    w.synth.shown.as_ref().filter(|s| matches!(s.result, 0 | 0xc) && s.item != 0).map(|s| (s.item, None))
}

/// The modern layer's view (this module keeps every rule and key): cmbmenu, the crystal list, the history with
/// cmbhwin, and tskill1 with its picker and the count asked.
pub(super) struct View {
    /// cmbmenu: its cursor, its rows off (the 23 s rule), whether it has the focus.
    pub menu: Option<(u16, bool, bool)>,
    /// The crystal list: its rows and the cursor's row.
    pub crystals: Option<(Vec<crate::inventory::Slot>, usize)>,
    /// The history (newest first), cmbhlst's cursor, cmbhwin's while it has the focus, whether either has it.
    pub history: Option<(Vec<Record>, u16, Option<u16>, bool)>,
    pub win: Option<WinView>,
}

/// tskill1: the crystal and its element, the slots (item, count, missing in mode 2), the cursor and focus.
pub(super) struct WinView {
    pub crystal: u16,
    pub element: Option<u16>,
    pub found: bool,
    pub slots: [(u16, u32, bool); 8],
    pub cursor: u16,
    pub on_slots: bool,
    pub on_list: bool,
    /// The picker (none in mode 2): rows, their colours, the cursor's row.
    pub picker: Option<(Vec<crate::inventory::Slot>, Vec<u32>, usize)>,
    /// The count asked: value, most, whether it is the amount to make (ours) rather than a stack's.
    pub count: Option<(u32, u32, bool)>,
    pub complete: bool,
    pub history: bool,
}

pub(super) fn view(w: &World) -> View {
    let s = &w.synth;
    let free = w.confirm.is_none();
    let m = s.menu.as_ref();
    let menu = m.filter(|m| s.win.is_none() && m.history.is_none()).map(|m| (m.sel, menu_off(w), m.crystals.is_none() && free));
    let crystals = m.filter(|m| s.win.is_none() && m.history.is_none()).and_then(|m| m.crystals).map(|(sc, cu)| (crystal_rows(w), sc + cu));
    let away = s.win.as_ref().is_some_and(|x| x.focus == Focus::Away) && free;
    let history = m.and_then(|m| m.history).map(|(cur, fav)| (s.history.clone(), cur, fav, away));
    let win = s.win.as_ref().map(|x| {
        let picker = (x.from != From::History).then(|| {
            let rows = picker_rows(w);
            let tints = rows.iter().map(|r| tint(w, x, r)).collect();
            (rows, tints, x.list.0 + x.list.1)
        });
        let count = match x.focus {
            Focus::Qty => x.qty.map(|(_, _, v, max)| (v, max, false)),
            Focus::Amount(v, max, _) => Some((v, max, true)),
            _ => None,
        };
        let element = match x.crystal.1 {
            0x1000..=0x1007 => Some(x.crystal.1 - 0x1000),
            0x108e..=0x1095 => Some(x.crystal.1 - 0x108e),
            0x196a..=0x1971 => Some(x.crystal.1 - 0x196a),
            _ => None,
        };
        WinView {
            crystal: x.crystal.1,
            element,
            found: x.found,
            slots: std::array::from_fn(|i| if x.slots[i].0 != 0 || x.missing[i] { (x.slots[i].1, x.slots[i].2, x.missing[i]) } else { (0, 0, false) }),
            cursor: x.cursor,
            on_slots: x.focus == Focus::Slots && free,
            on_list: x.focus == Focus::List && free,
            picker,
            count,
            complete: complete(x),
            history: x.from == From::History,
        }
    });
    View { menu, crystals, history, win }
}

/// The modern layer's mouse: cmbmenu's cursor to `c`; true when it moved.
pub(super) fn set_menu(w: &mut World, c: u16) -> bool {
    let Some(m) = w.synth.menu.as_mut().filter(|m| m.crystals.is_none()) else { return false };
    let moved = m.sel != c;
    m.sel = c;
    moved
}

/// The crystal list's cursor onto row `k`.
pub(super) fn set_crystal(w: &mut World, k: usize) -> bool {
    let Some(c) = w.synth.menu.as_mut().and_then(|m| m.crystals.as_mut()) else { return false };
    let moved = c.0 + c.1 != k;
    *c = super::post::list_at(k);
    moved
}

/// cmbhlst's cursor onto record `c` (tskill1 follows, as the keys do), cmbhwin's while it has the focus.
pub(super) fn set_history(w: &mut World, c: u16) -> bool {
    let Some((cur, fav)) = w.synth.menu.as_ref().and_then(|m| m.history) else { return false };
    if let Some(f) = fav {
        if let Some(m) = w.synth.menu.as_mut() {
            m.history = Some((cur, Some(c)));
        }
        return f != c;
    }
    if cur == c || c == 0 || c as usize > w.synth.history.len() {
        return false;
    }
    if let Some(m) = w.synth.menu.as_mut() {
        m.history = Some((c, None));
    }
    if let Some(s) = w.synth.win.as_mut() {
        s.focus = Focus::Away;
    }
    refresh(w);
    true
}

/// tskill1's cursor onto child `c` (the picker closes; in mode 2 only Synthesize and Cancel take it).
pub(super) fn set_slot(w: &mut World, c: u16) -> bool {
    let Some(s) = w.synth.win.as_mut() else { return false };
    let free = matches!(s.focus, Focus::Slots | Focus::List) || (s.focus == Focus::Away && matches!(c, 9 | 10));
    if !free {
        return false;
    }
    let moved = s.cursor != c || s.focus != Focus::Slots;
    (s.focus, s.cursor) = (Focus::Slots, c);
    moved
}

/// The picker's cursor onto row `k`, the picker taking the focus for the cursor's slot (the first empty one when the
/// cursor is on Synthesize or Cancel).
pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(s) = w.synth.win.as_mut().filter(|s| s.from != From::History && matches!(s.focus, Focus::Slots | Focus::List)) else { return false };
    if !(1..=8).contains(&s.cursor) {
        s.cursor = s.slots[..8].iter().position(|x| x.0 == 0).map_or(1, |i| i as u16 + 1);
    }
    let moved = s.focus != Focus::List || s.list.0 + s.list.1 != k;
    (s.focus, s.list) = (Focus::List, super::post::list_at(k));
    moved
}

/// cmbhlst moved to (200, 48) (FUN_101e2b60): per record newest first the icon at half size at (+3, +6 + 18k), the
/// name in the game font at (+21, +5 + 18k), the favourite (keytops3 101) at (+166, +5 + 18k); the bar keytops3 428 at
/// (0, +6) on the cursor row while focused. cmbhwin at (384, 48), its rows disabled with no record.
#[allow(clippy::too_many_arguments)]
fn draw_history(gu: &mut GameUi, w: &World, info: &super::items::ItemCache, cur: u16, fav: Option<u16>, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let away = w.synth.win.as_ref().is_some_and(|s| s.focus == Focus::Away) && w.confirm.is_none();
    let list_focus = away && fav.is_none();
    if let Some(win) = gu.lobby_window_off(CMBHLST, sw, sh, (184.0, 0.0), list_focus.then_some(cur), list_focus, &[]) {
        let [x, y, fw, fh] = win.frame;
        // The rows are drawn by code: 18 apart from (0, 4).
        let rows = (0..w.synth.history.len()).map(|k| (k as u16 + 1, [x, y + 4.0 + 18.0 * k as f32, fw - 8.0, 18.0])).collect();
        super::winhits::record_rects(w, CMBHLST, win.frame, rows);
        // 11 rows (FUN_101e2b60); placed, so at x1 - 6; ours never scrolls the history.
        gu.scroll_bar(x + fw - 6.0, y, fh, w.synth.history.len(), 11, 0);
        if list_focus {
            gu.sprite("menu    keytops3", 428, x, y + 6.0 + 18.0 * (cur as f32 - 1.0), 0x8080_8080);
        }
        super::items::upload_icons(gu, info, w.synth.history.iter().map(|r| r.item));
        for (k, r) in w.synth.history.iter().enumerate() {
            let ry = y + 18.0 * k as f32;
            let icon = crate::render::invwin::icon_texture(r.item);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, x + 3.0, ry + 6.0, 16.0, 16.0, [1.0; 4]);
            }
            let name = info.item(r.item).map(|it| it.name.clone()).unwrap_or_default();
            gu.text(&name, x + 21.0, ry + 5.0, 0, 0x8080_8080u32.to_le_bytes());
            if r.crystal & 0x8000 != 0 {
                gu.sprite("menu    keytops3", 101, x + 166.0, ry + 5.0, 0x8080_8080);
            }
        }
        rects.push(win.frame);
    }
    let off: &[u16] = if w.synth.history.is_empty() { &[1, 2] } else { &[] };
    let fav_focus = away && fav.is_some();
    if let Some(win) = gu.lobby_window_off(CMBHWIN, sw, sh, (0.0, 0.0), fav.filter(|_| fav_focus), fav_focus, off) {
        super::winhits::record(w, CMBHWIN, &win);
        let [x, y, ..] = win.frame;
        gu.disabled_children(CMBHWIN, x, y, off, fav.filter(|_| fav_focus));
        rects.push(win.frame);
    }
    rects
}

/// tskill1 (FUN_101e12e0): the crystal at half size at (+0x12, +4) and its element (keytops3 0xb9 + element) at
/// (+0x24, +4), the slots' icons and counts; the picker list at (200, 48) and itemctrl.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    // The modern layer draws these and takes their mouse.
    if w.modern {
        return rects;
    }
    let Some(info) = w.item_info.clone() else { return rects };
    if let Some((cur, fav)) = w.synth.menu.as_ref().and_then(|m| m.history) {
        rects.extend(draw_history(gu, w, &info, cur, fav, sw, sh));
    } else if let Some(m) = &w.synth.menu
        && w.synth.win.is_none()
    {
        let off: &[u16] = if menu_off(w) { &[1, 2] } else { &[] };
        let focused = m.crystals.is_none();
        if let Some(win) = gu.lobby_window_off(CMBMENU, sw, sh, (0.0, 0.0), focused.then_some(m.sel), focused, off) {
            super::winhits::record(w, CMBMENU, &win);
            let [x, y, ..] = win.frame;
            gu.disabled_children(CMBMENU, x, y, off, focused.then_some(m.sel));
            rects.push(win.frame);
        }
        if let Some(list) = m.crystals {
            let rows = crystal_rows(w);
            let tints = vec![0x8090_9040; rows.len()];
            rects.extend(draw_rows(gu, &rows, &info, list, true, Some(&tints), sw, sh));
        }
    }
    let Some(s) = &w.synth.win else { return rects };
    let focused = |f: Focus| s.focus == f && w.confirm.is_none();
    // Mode 2 has no picker: cmbhlst sits where the list would be.
    if s.from != From::History {
        let rows = picker_rows(w);
        let tints: Vec<u32> = rows.iter().map(|r| tint(w, s, r)).collect();
        rects.extend(draw_list(gu, w, &info, s.list, focused(Focus::List), Some(&tints), sw, sh));
    }
    if let Some(win) = gu.lobby_window_off(TSKILL1, sw, sh, (0.0, 0.0), focused(Focus::Slots).then_some(s.cursor), focused(Focus::Slots), &[]) {
        super::winhits::record(w, TSKILL1, &win);
        let [x, y, ..] = win.frame;
        let hist = s.from == From::History;
        let element = match s.crystal.1 {
            0x1000..=0x1007 => Some(s.crystal.1 - 0x1000),
            0x108e..=0x1095 => Some(s.crystal.1 - 0x108e),
            0x196a..=0x1971 => Some(s.crystal.1 - 0x196a),
            _ => None,
        };
        if let Some(e) = element {
            // Mode 2 with the crystal not held: the element dims, a red cross (keytops3 515) at 0.3 over it.
            let dim = hist && !s.found;
            gu.sprite("menu    keytops3", 0xb9 + e as usize, x + 36.0, y + 4.0, if dim { 0x8058_5858 } else { 0x8080_8080 });
            super::items::upload_icons(gu, &info, std::iter::once(s.crystal.1));
            let icon = crate::render::invwin::icon_texture(s.crystal.1);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, x + 18.0, y + 4.0, 16.0, 16.0, [1.0; 4]);
            }
        }
        if hist && !s.found && s.crystal.1 != 0 {
            gu.sprite_scaled("menu    keytops3", 515, x + 29.0, y + 10.0, 0.3, 0.3, 0x8080_8080);
        }
        let slots: Vec<(u16, u32, bool)> = (0..8).map(|i| (s.slots[i].1, s.slots[i].2, s.slots[i].0 != 0 || s.missing[i])).collect();
        draw_slots(gu, &info, TSKILL1, win.frame, &slots, None, focused(Focus::Slots).then_some(s.cursor), |_| [1.0; 4]);
        if let Some(l) = gu.layout(TSKILL1).cloned() {
            for e in l.elements.iter().filter(|e| (1..=8).contains(&e.id) && s.missing[e.id as usize - 1]) {
                gu.sprite_scaled("menu    keytops3", 515, x + e.x as f32 + 20.0, y + e.y as f32 + 20.0, 0.32, 0.32, 0x8080_8080);
            }
        }
        if !complete(s) {
            gu.disabled_children(TSKILL1, x, y, &[9], focused(Focus::Slots).then_some(s.cursor));
        }
        rects.push(win.frame);
    }
    let amount = match s.focus {
        Focus::Amount(v, max, _) => Some((0, 0, v, max)),
        _ => None,
    };
    // The amount (ours) hangs from tskill1's bottom edge, as part of that menu; itemctrl for a stack stays at its place.
    let off = match (amount, gu.window_rect(TSKILL1, sw, sh), gu.window_rect(ITEMCTRL, sw, sh)) {
        (Some(_), Some(t), Some(c)) => (t[0] - c[0], t[1] + t[3] - c[1]),
        _ => (0.0, 0.0),
    };
    if let Some((_, _, v, max)) = s.qty.or(amount)
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, off, Some(1), true, &[])
    {
        super::winhits::record(w, ITEMCTRL, &win);
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
