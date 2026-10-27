//! Monstrosity Customization (main menu row 2 as a monster; monstrosity_custom_spec.md): mp_stat mode 1 and level
//! mode 2, mp_cstm (the instinct slots), mp_mncst (Species, Faculties, Naming), the generic list mp_invnt over three
//! sources (species, instincts, names), mp_mnnm, mp_mntp2 (Refine by Type), mp_kncst (Refine by Cost), the cost panel
//! in level's place; the pending buffer committed as c2s 0x102 at most once a second, only in the Feretory.

use super::World;
use super::game_ui::GameUi;
use super::monstrosity;
use super::net::NetCommand;
use crate::audio::Ui;
use std::time::{Duration, Instant};

const CSTM: &str = "menu    mp_cstm";
const MNCST: &str = "menu    mp_mncst";
const INVNT: &str = "menu    mp_invnt";
const MNNM: &str = "menu    mp_mnnm";
const MNTP2: &str = "menu    mp_mntp2";
const KNCST: &str = "menu    mp_kncst";
const HELP: &str = "menu    bluehelp";
const LEVEL: &str = "menu    level";
const VISIBLE: usize = 11;
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const GREEN: u32 = 0x8050_9050;
const YELLOW: u32 = 0x8090_9040;
/// mp_kncst's field being edited (FUN_10144070).
const EDITING: u32 = 0x80a0_6040;
/// FUN_101420a0: a commit at most every 30 frames (retail's 30 a second, inf).
const COMMIT: Duration = Duration::from_millis(1000);
/// The 'star' a species change plays on us (file 0x113bd, scheduler 'star').
const STAR_FILE: u32 = 0x113bd;
/// mp_cstm's children by slot (+0x168): slots 0..3 the top row, 4..7 the middle, 8..11 the bottom.
const SLOT_CHILD: [u16; 12] = [1, 4, 7, 10, 2, 5, 8, 11, 3, 6, 9, 12];
/// mp_mntp2's rows left enabled (FUN_10143f00 disables 3..5, 7..11, 16..18).
const TYPE_ROWS: [u16; 9] = [1, 2, 6, 12, 13, 14, 15, 19, 20];
/// The cost filter's default (FUN_10144330).
const COST_ALL: (u8, u8) = (1, 50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Species,
    Instincts,
    Names,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Mncst,
    List,
    Cstm,
    Mnnm,
    Mntp2,
    Kncst,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    /// framesus 130 "Refine by Type".
    Type,
    /// framesus 186 "Refine by Cost".
    Cost,
    /// group 9 0xe3 "(Remove All)".
    RemoveAll,
    Species(u16),
    /// The instinct's item (0x7400 + n) and the catalogue's cost.
    Instinct(u16, u16),
    /// A prefix id (0 the empty one).
    Name(u8),
}

/// mp_kncst: the pair, the pair saved at its open, the cursor child, the field being edited.
#[derive(Clone, Copy, Debug)]
struct Kncst {
    min: u8,
    max: u8,
    saved: (u8, u8),
    child: u16,
    editing: u8,
}

/// The open Customization session.
#[derive(Clone, Debug)]
pub struct Screen {
    /// The pending buffer (0x1057825c): flags (1 species, 4 instincts, 8 / 0x10 descriptors), species, rank, the 12
    /// slots (0 unchanged, 0xffff remove, n equip), the descriptors; dirty and the last commit.
    flags: u16,
    species: u16,
    rank: u8,
    slots: [u16; 12],
    desc: [u8; 2],
    dirty: bool,
    committed: Option<Instant>,
    /// DAT_105782ab: a species commit played 'star' (mp_mncst greyed until it ends).
    star: bool,
    /// DAT_10371480: the current species' base, -1 until known.
    base: i32,
    focus: Focus,
    mncst: u16,
    /// The list's source while it is open, its cursor (row, top) per source, the species list placed once.
    list: Option<Source>,
    cursors: [(usize, usize); 3],
    placed: bool,
    /// Type filters per list (-1 none; mp_mntp2 sets both), the cost filter.
    types: [i8; 2],
    cost: (u8, u8),
    /// mp_cstm's cursor child, the slot picked first (DAT_1037148c), the instinct picked first and the picked slot's
    /// cost (DAT_105782ac).
    cstm: u16,
    slot_pick: Option<usize>,
    inst_pick: u16,
    pick_cost: u16,
    faculties: bool,
    mnnm: Option<u16>,
    mntp2: Option<u16>,
    kncst: Option<Kncst>,
    /// mp_mntp2 opened from the instinct list (focus returns there).
    mntp2_from: Source,
}

fn source_index(s: Source) -> usize {
    match s {
        Source::Species => 0,
        Source::Instincts => 1,
        Source::Names => 2,
    }
}

/// FUN_10141b20: the buffer from the 0x044 block at every open.
fn fresh(w: &World) -> Screen {
    Screen {
        flags: 0,
        species: monstrosity::current_species(w),
        rank: monstrosity::rank(w),
        slots: [0; 12],
        desc: [monstrosity::descriptor(w, 0), monstrosity::descriptor(w, 1)],
        dirty: false,
        committed: None,
        star: false,
        base: -1,
        focus: Focus::Mncst,
        mncst: w.cursor_mem.get(MNCST).copied().unwrap_or(1),
        list: None,
        cursors: [(0, 0); 3],
        placed: false,
        types: [-1, -1],
        cost: COST_ALL,
        cstm: w.cursor_mem.get(CSTM).copied().unwrap_or(1),
        slot_pick: None,
        inst_pick: 0,
        pick_cost: 0,
        faculties: false,
        mnnm: None,
        mntp2: None,
        kncst: None,
        mntp2_from: Source::Species,
    }
}

/// Main menu row 2 in Monstrosity (FUN_10141ba0): the buffer, mp_stat mode 1, mp_cstm, mp_mncst on top.
pub fn open(w: &mut World) {
    w.moncustom = Some(fresh(w));
}

/// mp_mncst's Escape (FUN_10141bf0): every Customization window closes; the main menu resumes.
fn close(w: &mut World, s: &Screen) {
    w.cursor_mem.insert(MNCST.into(), s.mncst);
    w.cursor_mem.insert(CSTM.into(), s.cstm);
    w.moncustom = None;
}

fn main_level(w: &World) -> u8 {
    w.stats.as_ref().map_or(1, |st| st.main.1)
}

fn instinct_cost(w: &World, n: u16) -> u32 {
    if n == 0 || n == 0xffff {
        return 0;
    }
    w.item_info.as_ref().map_or(0xff, |c| c.instinct_cost(n) as u32)
}

/// FUN_101444b0: what the 0x044 slots cost.
fn used(w: &World) -> u32 {
    (0..12).map(|i| instinct_cost(w, monstrosity::slot(w, i))).sum()
}

fn cap(w: &World) -> u32 {
    monstrosity::cap(main_level(w), monstrosity::rank(w))
}

/// FUN_10144620: what is left, the picked slot's instinct given back.
fn left(w: &World, s: &Screen) -> i64 {
    cap(w) as i64 + s.pick_cost as i64 - used(w) as i64
}

/// FUN_10141cc0's rule over the 0x044 slots `cur` and the pending ones: the same instinct on its slot is a removal, one
/// already set or pending elsewhere refuses, and the effective set must stay within `cap`. True when it changed.
fn equip_rule(cur: &[u16; 12], pending: &mut [u16; 12], slot: usize, n: u16, cost: impl Fn(u16) -> u32, cap: u32) -> bool {
    if slot > 11 {
        return false;
    }
    let saved = *pending;
    if cur[slot] == n {
        pending[slot] = 0xffff;
    } else if cur.contains(&n) || pending.contains(&n) {
        return false;
    } else {
        pending[slot] = n;
    }
    let total: u32 = (0..12).map(|i| if pending[i] != 0 { pending[i] } else { cur[i] }).map(|v| if v == 0 || v == 0xffff { 0 } else { cost(v) }).sum();
    if total > cap {
        *pending = saved;
        return false;
    }
    true
}

fn current_slots(w: &World) -> [u16; 12] {
    std::array::from_fn(|i| monstrosity::slot(w, i))
}

/// Equip FUN_10141cc0 (slot, item); flags |= 4 when it took.
fn equip(w: &World, s: &mut Screen, slot: usize, item: u16) -> bool {
    if item < 0x7400 {
        return false;
    }
    let cur = current_slots(w);
    let ok = equip_rule(&cur, &mut s.slots, slot, item - 0x7400, |n| instinct_cost(w, n), cap(w));
    if ok {
        s.flags |= 4;
        s.dirty = true;
    }
    ok
}

/// FUN_100fd0b0: c2s 0x102's body (packet @(4 + k) at k).
fn packet(head: [u8; 2], flags: u16, species: u16, rank: u8, slots: &[u16; 12], desc: [u8; 2]) -> Vec<u8> {
    let mut b = vec![0u8; 0xa0];
    b[4..6].copy_from_slice(&head);
    b[6..8].copy_from_slice(&flags.to_le_bytes());
    b[8..10].copy_from_slice(&species.to_le_bytes());
    b[10] = rank;
    for (i, v) in slots.iter().enumerate() {
        b[12 + 2 * i..14 + 2 * i].copy_from_slice(&v.to_le_bytes());
    }
    b[36..38].copy_from_slice(&desc);
    b
}

/// mp_cstm's per frame (FUN_10143950 / FUN_101420a0 / FUN_101420d0): a dirty buffer commits at most once a second;
/// outside the Feretory the change is dropped. A species change plays 'star' on us.
pub fn tick(w: &mut World) {
    let Some(mut s) = w.moncustom.take() else { return };
    // Out of Monstrosity (a zone, the bout ended): the screen is gone.
    if !monstrosity::active(w) {
        return;
    }
    // The star started by a species commit; mp_mncst greys until it ends.
    if s.star && !w.effects.event_scheduler_running(STAR_FILE, u32::from_le_bytes(*b"star"), crate::fx::sched::Who::Actor(w.my_id), crate::fx::sched::Who::Actor(w.my_id)) {
        s.star = false;
    }
    if s.dirty && s.committed.is_none_or(|t| t.elapsed() >= COMMIT) {
        if s.flags != 0 {
            if monstrosity::in_feretory(w)
                && let Some(head) = monstrosity::block_head(w)
            {
                log::info!("monstrosity: c2s 0x102 flags {:#x} species {:#x} slots {:?} desc {:?}", s.flags, s.species, s.slots, s.desc);
                let _ = w.net.commands.send(NetCommand::Packet { id: 0x102, body: packet(head, s.flags, s.species, s.rank, &s.slots, s.desc) });
                if s.flags & 1 != 0 && w.actors.contains_key(&w.my_id) {
                    w.effects.on_event_scheduler(STAR_FILE, u32::from_le_bytes(*b"star"), crate::fx::sched::Who::Actor(w.my_id), crate::fx::sched::Who::Actor(w.my_id));
                    s.star = true;
                }
            }
            s.flags = 0;
            s.slots = [0; 12];
        }
        (s.dirty, s.committed) = (false, Some(Instant::now()));
    }
    if s.base < 0 {
        s.base = species_base(w, monstrosity::current_species(w)).map_or(-1, |b| b as i32);
    }
    w.moncustom = Some(s);
}

/// A species' base (record +0x2a, FUN_10142770).
fn species_base(w: &World, v: u16) -> Option<u16> {
    let it = w.item_info.as_ref()?.item(0xf000 + v)?;
    it.extra.iter().find(|e| e.0 == 0xd3c).map(|e| e.1 as u16)
}

/// The species list (FUN_10142940): Refine by Type, then the owned `mon_` codes of the type.
fn species_rows(w: &World, gu: &GameUi, s: &Screen) -> Vec<Row> {
    let mut rows = vec![Row::Type];
    let f = s.types[0];
    rows.extend(gu.mon_order().iter().copied().filter(|&v| monstrosity::species_owned(w, v) && (f < 0 || monstrosity::species_type(v) as i8 == f)).map(Row::Species));
    rows
}

/// The instinct list (FUN_10142e80): the two filters, Remove All, then the catalogues' owned instincts by type and cost.
fn instinct_rows(w: &World, s: &Screen) -> Vec<Row> {
    let Some(cache) = w.item_info.as_ref() else { return Vec::new() };
    let cat = cache.catalogues();
    if cat.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![Row::Type, Row::Cost, Row::RemoveAll];
    let f = s.types[1];
    rows.extend(cat.iter().filter_map(|&(item, cost)| {
        let n = item.wrapping_sub(0x7400);
        let ty = if n < 0x300 { monstrosity::species_type(n / 3) } else { 19 };
        let c = (cost & 0xff) as u8;
        ((f < 0 || ty as i8 == f) && s.cost.0 <= c && c <= s.cost.1 && monstrosity::instinct_owned(w, n)).then_some(Row::Instinct(item, cost))
    }));
    rows
}

/// The names list (FUN_101434a0): every prefix, the empty one first.
fn name_rows() -> Vec<Row> {
    (0..249u16).map(|i| Row::Name(i as u8)).collect()
}

fn rows(w: &World, gu: &GameUi, s: &Screen, src: Source) -> Vec<Row> {
    match src {
        Source::Species => species_rows(w, gu, s),
        Source::Instincts => instinct_rows(w, s),
        Source::Names => name_rows(),
    }
}

/// FUN_101f7080: the cursor on `row`, scrolled into view.
fn place(c: &mut (usize, usize), row: usize, n: usize) {
    c.0 = row.min(n.saturating_sub(1));
    c.1 = c.1.min(c.0).max((c.0 + 1).saturating_sub(VISIBLE)).min(n.saturating_sub(VISIBLE));
}

/// The generic list's keys: Up / Down a row (silent at the ends), Left / Right a page (FUN_101f6c70, +0x47).
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

/// mp_cstm's grid: arrows wrap in rows and columns (the layout's links).
fn grid_step(child: u16, key: egui::Key) -> u16 {
    use egui::Key as K;
    let (col, row) = ((child - 1) / 3, (child - 1) % 3);
    let (col, row) = match key {
        K::ArrowDown => (col, (row + 1) % 3),
        K::ArrowUp => (col, (row + 2) % 3),
        K::ArrowRight => ((col + 1) % 4, row),
        K::ArrowLeft => ((col + 3) % 4, row),
        _ => (col, row),
    };
    col * 3 + row + 1
}

fn child_slot(child: u16) -> usize {
    SLOT_CHILD.iter().position(|&c| c == child).unwrap_or(0)
}

/// The keys of whichever Customization window has the focus.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(mut s) = w.moncustom.take() else { return };
    let keep = keys(w, gu, &mut s, key);
    if keep && w.moncustom.is_none() {
        w.moncustom = Some(s);
    }
}

fn keys(w: &mut World, gu: &GameUi, s: &mut Screen, key: egui::Key) -> bool {
    use egui::Key as K;
    let se = |w: &mut World, u: Ui| w.sounds.push(u as u32);
    match s.focus {
        Focus::Mntp2 => {
            let r = s.mntp2.unwrap_or(1);
            match key {
                K::ArrowDown => {
                    s.mntp2 = Some(r % 20 + 1);
                    se(w, Ui::Cursor);
                }
                K::ArrowUp => {
                    s.mntp2 = Some(if r <= 1 { 20 } else { r - 1 });
                    se(w, Ui::Cursor);
                }
                // FUN_10143fc0: a type sets both lists' filter, row 20 clears it.
                K::Enter if TYPE_ROWS.contains(&r) => {
                    let f = if r == 20 { -1 } else { r as i8 };
                    s.types = [f, f];
                    se(w, Ui::Select);
                    (s.mntp2, s.focus) = (None, Focus::List);
                    let src = s.mntp2_from;
                    let n = rows(w, gu, s, src).len();
                    let c = &mut s.cursors[source_index(src)];
                    place(c, c.0, n);
                }
                K::Enter => se(w, Ui::Error),
                K::Escape => {
                    se(w, Ui::Cancel);
                    (s.mntp2, s.focus) = (None, Focus::List);
                }
                _ => {}
            }
        }
        Focus::Kncst => kncst_key(w, s, key),
        Focus::Mnnm => {
            let r = s.mnnm.unwrap_or(1);
            match key {
                K::ArrowDown | K::ArrowUp => {
                    s.mnnm = Some(if r == 1 { 2 } else { 1 });
                    se(w, Ui::Cursor);
                }
                K::Enter => {
                    se(w, Ui::Select);
                    s.focus = Focus::List;
                }
                K::Escape => {
                    se(w, Ui::Cancel);
                    (s.mnnm, s.list, s.focus) = (None, None, Focus::Mncst);
                }
                _ => {}
            }
        }
        Focus::Cstm => cstm_key(w, gu, s, key),
        Focus::List => list_key(w, gu, s, key),
        Focus::Mncst => match key {
            K::ArrowDown => {
                s.mncst = s.mncst % 3 + 1;
                se(w, Ui::Cursor);
            }
            K::ArrowUp => {
                s.mncst = if s.mncst <= 1 { 3 } else { s.mncst - 1 };
                se(w, Ui::Cursor);
            }
            // FUN_101423a0: greyed while the species change's star plays.
            K::Enter if s.star => se(w, Ui::Error),
            K::Enter => {
                se(w, Ui::Select);
                match s.mncst {
                    1 => {
                        s.base = species_base(w, monstrosity::current_species(w)).map_or(-1, |b| b as i32);
                        s.types[0] = -1;
                        (s.list, s.focus) = (Some(Source::Species), Focus::List);
                        // The first build after the open: the cursor on the current species.
                        if !s.placed {
                            let rws = species_rows(w, gu, s);
                            let cur = monstrosity::current_species(w);
                            let at = rws.iter().position(|r| *r == Row::Species(cur)).unwrap_or(0);
                            s.cursors[0] = (0, 0);
                            place(&mut s.cursors[0], at, rws.len());
                            s.placed = true;
                        }
                    }
                    2 => {
                        s.types[1] = -1;
                        s.cost = COST_ALL;
                        (s.list, s.faculties, s.focus) = (Some(Source::Instincts), true, Focus::Cstm);
                    }
                    _ => {
                        (s.list, s.mnnm, s.focus) = (Some(Source::Names), Some(1), Focus::Mnnm);
                    }
                }
            }
            K::Escape => {
                se(w, Ui::Cancel);
                close(w, s);
                return false;
            }
            _ => {}
        },
    }
    true
}

/// The generic list's keys over its source (vt+0x28).
fn list_key(w: &mut World, gu: &GameUi, s: &mut Screen, key: egui::Key) {
    use egui::Key as K;
    let Some(src) = s.list else { return };
    let rws = rows(w, gu, s, src);
    let i = source_index(src);
    match key {
        K::Enter => match (src, rws.get(s.cursors[i].0).copied()) {
            (Source::Species | Source::Instincts, Some(Row::Type)) => {
                w.sounds.push(Ui::Select as u32);
                (s.mntp2, s.mntp2_from, s.focus) = (Some(1), src, Focus::Mntp2);
            }
            (_, Some(Row::Cost)) => {
                w.sounds.push(Ui::Select as u32);
                s.kncst = Some(Kncst { min: s.cost.0, max: s.cost.1, saved: s.cost, child: 1, editing: 0 });
                s.focus = Focus::Kncst;
            }
            (_, Some(Row::RemoveAll)) => {
                w.sounds.push(Ui::Select as u32);
                w.confirm = Some((super::Confirm::MonRemoveAll, false));
            }
            (_, Some(Row::Species(v))) if monstrosity::in_feretory(w) => {
                let base = species_base(w, v).map(|b| b as i32);
                let cur = monstrosity::current_species(w);
                match base {
                    _ if v == cur || s.base < 0 => w.sounds.push(Ui::Error as u32),
                    // Another family resets the experience: FUN_10142b50's prompt.
                    Some(b) if b != s.base => {
                        w.sounds.push(Ui::Select as u32);
                        w.confirm = Some((super::Confirm::MonSpecies(v), false));
                    }
                    _ => {
                        w.sounds.push(Ui::Select as u32);
                        set_species(w, s, v);
                    }
                }
            }
            (_, Some(Row::Instinct(item, _))) if monstrosity::in_feretory(w) => {
                match s.slot_pick.take() {
                    None => {
                        w.sounds.push(Ui::Select as u32);
                        s.inst_pick = item;
                        // FUN_10143e90: mp_cstm's cursor to the slot holding it.
                        if let Some(k) = (0..12).find(|&k| monstrosity::slot(w, k) == item - 0x7400) {
                            s.cstm = SLOT_CHILD[k];
                        }
                    }
                    Some(slot) => {
                        let ok = equip(w, s, slot, item);
                        w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
                        (s.inst_pick, s.pick_cost) = (0, 0);
                    }
                }
                s.focus = Focus::Cstm;
            }
            (Source::Names, Some(Row::Name(id))) => {
                w.sounds.push(Ui::Select as u32);
                let slot = s.mnnm.unwrap_or(1) as usize - 1;
                s.desc[slot] = id;
                s.flags |= if slot == 0 { 8 } else { 0x10 };
                s.dirty = true;
                s.focus = Focus::Mnnm;
            }
            _ => w.sounds.push(Ui::Error as u32),
        },
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            match src {
                Source::Species => (s.list, s.focus) = (None, Focus::Mncst),
                Source::Instincts => {
                    (s.slot_pick, s.inst_pick, s.pick_cost) = (None, 0, 0);
                    s.focus = Focus::Cstm;
                }
                Source::Names => s.focus = Focus::Mnnm,
            }
        }
        _ => {
            if step(&mut s.cursors[i], rws.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
}

/// FUN_10141e40 and the species list's close: the pending species, the base remembered.
fn set_species(w: &World, s: &mut Screen, v: u16) {
    s.species = v;
    s.flags |= 1;
    s.dirty = true;
    s.base = species_base(w, v).map_or(-1, |b| b as i32);
    s.placed = false;
    (s.list, s.focus) = (None, Focus::Mncst);
}

/// The exp prompt's Yes (LAB_10142cc0).
pub fn species_yes(w: &mut World, v: u16) {
    let Some(mut s) = w.moncustom.take() else { return };
    set_species(w, &mut s, v);
    w.moncustom = Some(s);
}

/// Remove All's Yes (LAB_10143380, FUN_10141e00): every set slot to 0xffff.
pub fn remove_all(w: &mut World) {
    let Some(mut s) = w.moncustom.take() else { return };
    for i in 0..12 {
        s.slots[i] = if monstrosity::slot(w, i) != 0 { 0xffff } else { 0 };
    }
    s.flags |= 4;
    s.dirty = true;
    (s.slot_pick, s.inst_pick, s.pick_cost, s.focus) = (None, 0, 0, Focus::Cstm);
    w.moncustom = Some(s);
}

/// mp_cstm (FUN_10143cd0): the slot grid, a slot then an instinct (or the other way) equips.
fn cstm_key(w: &mut World, gu: &GameUi, s: &mut Screen, key: egui::Key) {
    use egui::Key as K;
    match key {
        K::Enter if !monstrosity::in_feretory(w) => {
            (s.slot_pick, s.inst_pick, s.pick_cost) = (None, 0, 0);
            w.sounds.push(Ui::Error as u32);
        }
        K::Enter => {
            let slot = child_slot(s.cstm);
            if s.inst_pick != 0 {
                let ok = equip(w, s, slot, s.inst_pick);
                w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
                (s.slot_pick, s.inst_pick, s.pick_cost) = (None, 0, 0);
                return;
            }
            // FUN_10143bf0: the list's cursor to the slot's instinct, else its last row; the list takes the focus.
            let rws = instinct_rows(w, s);
            if rws.is_empty() {
                s.slot_pick = Some(slot);
                w.sounds.push(Ui::Error as u32);
                return;
            }
            let n = monstrosity::slot(w, slot);
            let at = rws.iter().position(|r| matches!(r, Row::Instinct(item, _) if n != 0 && *item == 0x7400 + n)).unwrap_or(s.cursors[1].0);
            place(&mut s.cursors[1], at, rws.len());
            s.slot_pick = Some(slot);
            let c = instinct_cost(w, n);
            s.pick_cost = if c == 0xff { 0 } else { c as u16 };
            s.focus = Focus::List;
            w.sounds.push(Ui::Select as u32);
        }
        // FUN_10142210: Faculties ends; level back, the list and iteminfo closed.
        K::Escape => {
            (s.slot_pick, s.inst_pick, s.pick_cost) = (None, 0, 0);
            (s.list, s.faculties, s.focus) = (None, false, Focus::Mncst);
            w.sounds.push(Ui::Cancel as u32);
        }
        K::ArrowDown | K::ArrowUp | K::ArrowLeft | K::ArrowRight => {
            s.cstm = grid_step(s.cstm, key);
            w.sounds.push(Ui::Cursor as u32);
        }
        _ => {}
    }
    let _ = gu;
}

/// mp_kncst (FUN_10144190): the min and max boxes edited by the arrows, Keep, Cancel, Reset.
fn kncst_key(w: &mut World, s: &mut Screen, key: egui::Key) {
    use egui::Key as K;
    let Some(mut k) = s.kncst else { return };
    if k.editing != 0 {
        let d: i32 = match key {
            K::ArrowDown => -1,
            K::ArrowUp => 1,
            K::ArrowRight => -10,
            K::ArrowLeft => 10,
            _ => 0,
        };
        if d != 0 {
            if k.editing == 1 {
                k.min = (k.min as i32 + d).clamp(1, 50) as u8;
                k.max = k.max.max(k.min);
            } else {
                k.max = (k.max as i32 + d).clamp(1, 50) as u8;
                if k.max <= k.min {
                    k.min = k.max.saturating_sub(1).max(1).min(k.max);
                }
            }
            s.cost = (k.min, k.max);
            s.kncst = Some(k);
            return;
        }
        if matches!(key, K::Enter | K::Escape) {
            k.editing = 0;
            w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
        }
        s.kncst = Some(k);
        return;
    }
    // Links: 1 <-> 2 across, down 3, 4, 5 and back to 1.
    let next = |c: u16, key: K| match (c, key) {
        (1, K::ArrowRight | K::ArrowLeft) => 2,
        (2, K::ArrowRight | K::ArrowLeft) => 1,
        (1 | 2, K::ArrowDown) => 3,
        (1 | 2, K::ArrowUp) => 5,
        (3, K::ArrowUp) => 1,
        (c, K::ArrowDown) => c % 5 + 1,
        (c, K::ArrowUp) => c - 1,
        (c, _) => c,
    };
    match key {
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            match k.child {
                1 | 2 => k.editing = k.child as u8,
                3 => {
                    s.cost = (k.min, k.max);
                    (s.kncst, s.focus) = (None, Focus::List);
                    return;
                }
                4 => {
                    s.cost = k.saved;
                    (s.kncst, s.focus) = (None, Focus::List);
                    return;
                }
                _ => {
                    s.cost = COST_ALL;
                    (s.kncst, s.focus) = (None, Focus::List);
                    return;
                }
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            s.cost = k.saved;
            (s.kncst, s.focus) = (None, Focus::List);
            return;
        }
        K::ArrowDown | K::ArrowUp | K::ArrowLeft | K::ArrowRight => {
            let c = next(k.child, key);
            if c != k.child {
                k.child = c;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        _ => {}
    }
    s.kncst = Some(k);
}

/// Num+ (message 8): between mp_cstm and the instinct list in Faculties (SE 0013), picks reset from the list.
pub fn num_plus(w: &mut World) {
    let Some(s) = w.moncustom.as_mut() else { return };
    match s.focus {
        Focus::Cstm if s.faculties => {
            s.focus = Focus::List;
            w.sounds.push(Ui::Close as u32);
        }
        Focus::List if s.list == Some(Source::Instincts) => {
            (s.slot_pick, s.inst_pick, s.pick_cost, s.focus) = (None, 0, 0, Focus::Cstm);
            w.sounds.push(Ui::Close as u32);
        }
        _ => {}
    }
}

/// The item info window in Faculties (mode 3 at (130, 260)): the focused list's instinct, or mp_cstm's slot's.
pub fn info_source(w: &World) -> Option<u16> {
    let s = w.moncustom.as_ref().filter(|s| s.faculties)?;
    match s.focus {
        Focus::Cstm => Some(monstrosity::slot(w, child_slot(s.cstm))).filter(|&n| n != 0).map(|n| 0x7400 + n),
        Focus::List if s.list == Some(Source::Instincts) => match instinct_rows(w, s).get(s.cursors[1].0) {
            Some(Row::Instinct(item, _)) => Some(*item),
            _ => None,
        },
        _ => None,
    }
}

/// The help and title bars (section 1): each focused window's help line in slot 1, "Customization" or "Refine by Type".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.moncustom else { return };
    let help = |id: usize| gu.menu_msg(id, "");
    let (h, t) = match s.focus {
        Focus::Mncst => (help(0x2bc + s.mncst as usize), 0xa5),
        Focus::Cstm => (help(0x2be), 0xa5),
        Focus::Mnnm => (help(0x2bf), 0xa5),
        Focus::Mntp2 => (help(0x2c3), 0x69),
        Focus::Kncst => (help(if s.kncst.is_some_and(|k| k.child == 4) { 0x17b } else { 0x328 }), 0x69),
        Focus::List => {
            let line = match s.list {
                Some(Source::Species) => match species_rows(w, gu, s).get(s.cursors[0].0) {
                    Some(Row::Species(v)) => w.item_info.as_ref().and_then(|c| c.item(0xf000 + v)).map(|it| it.description.clone()).unwrap_or_default(),
                    _ => help(0x2c3),
                },
                Some(Source::Instincts) => match s.cursors[1].0 {
                    0 => help(0x2c3),
                    1 => help(0x328),
                    _ => help(0x2be),
                },
                _ => help(0x2bf),
            };
            (line, 0xa5)
        }
    };
    bars.help[1] = h;
    bars.title[1] = gu.title_msg(t).unwrap_or_default();
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// An item's icon at half scale (FUN_1015bd70, 16x16).
fn item_icon(gu: &mut GameUi, w: &World, id: u16, x: f32, y: f32) {
    let Some(cache) = w.item_info.as_ref() else { return };
    super::items::upload_icons(gu, cache, std::iter::once(id));
    let icon = crate::render::invwin::icon_texture(id);
    if gu.has_image(&icon) {
        gu.image_rect(&icon, x, y, 16.0, 16.0, [1.0; 4]);
    }
}

/// A row's name colour: green when it is the 0x044 one, else white in the Feretory and grey outside.
fn owned_tint(w: &World, set: bool) -> u32 {
    if set {
        GREEN
    } else if monstrosity::in_feretory(w) {
        WHITE
    } else {
        GREY
    }
}

/// mp_invnt over its source: 11 rows at (3, 6 + 18 k), the band (keytops3 428) and the hand while focused, the bar.
fn draw_list(gu: &mut GameUi, w: &World, s: &Screen, src: Source, sw: f32, sh: f32, focused: bool, rects: &mut Vec<[f32; 4]>) {
    let rws = rows(w, gu, s, src);
    let c = s.cursors[source_index(src)];
    let hide: Vec<u16> = (1..=VISIBLE as u16).collect();
    let Some(win) = gu.lobby_window_off(INVNT, sw, sh, (0.0, 0.0), None, false, &hide) else { return };
    super::winhits::record(w, INVNT, &win);
    let [x, y, fw, fh] = win.frame;
    if focused {
        gu.sprite("menu    keytops3", 428, x, y + 6.0 + 18.0 * (c.0 - c.1) as f32, WHITE);
        gu.cursor(x + 3.0, y + 6.0 + 18.0 * (c.0 - c.1) as f32);
    }
    let names_col = if src == Source::Names { 5.0 } else { 0.0 };
    let left = left(w, s);
    for (k, r) in rws.iter().skip(c.1).take(VISIBLE).enumerate() {
        let ry = y + 3.0 + 18.0 * k as f32;
        match *r {
            Row::Type => {
                let f = s.types[if src == Source::Species { 0 } else { 1 }];
                gu.sprite("menu    framesus", 130, x, ry + 2.0, if f >= 0 { GREEN } else { WHITE });
            }
            Row::Cost => {
                gu.sprite("menu    framesus", 186, x, ry + 2.0, if s.cost == COST_ALL { WHITE } else { GREEN });
            }
            Row::RemoveAll => text(gu, &w.battle.as_ref().and_then(|b| b.group9(0xe3)).unwrap_or_default(), x + 18.0, ry + 2.0, YELLOW),
            Row::Species(v) => {
                item_icon(gu, w, 0xf000 + v, x, ry);
                let name = w.item_info.as_ref().and_then(|c| c.item(0xf000 + v)).map(|it| it.name.clone()).unwrap_or_default();
                text(gu, &name, x + 18.0, ry + 2.0, owned_tint(w, v == monstrosity::current_species(w)));
                if monstrosity::data_in(w) {
                    let lv = species_base(w, v).map_or(0, |b| monstrosity::level(w, b));
                    gu.shape_text("Lv", x + 170.0 - 32.0, ry + 4.0, WHITE, false);
                    gu.shape_text(&lv.to_string(), x + 170.0, ry + 4.0, WHITE, true);
                }
            }
            Row::Instinct(item, _) => {
                item_icon(gu, w, item, x, ry);
                let n = item - 0x7400;
                let set = (0..12).any(|i| monstrosity::slot(w, i) == n);
                let mut tint = owned_tint(w, set);
                if tint == WHITE && left < instinct_cost(w, n) as i64 {
                    tint = GREY;
                }
                let name = w.item_info.as_ref().and_then(|c| c.item(item)).map(|it| it.name.clone()).unwrap_or_default();
                text(gu, &name, x + 18.0, ry + 2.0, tint);
            }
            Row::Name(id) => {
                let slot = s.mnnm.unwrap_or(1) as usize - 1;
                text(gu, monstrosity::prefix_text(id), x + names_col, ry + 2.0, owned_tint(w, monstrosity::descriptor(w, slot) == id));
            }
        }
    }
    gu.scroll_bar(x + fw - 8.0, y, fh, rws.len(), VISIBLE, c.1);
    if focused {
        gu.bar_site(super::scrollbar::Owner::MonCustom, x + fw - 8.0, y, fh, rws.len(), VISIBLE, c.1);
    }
    rects.push(win.frame);
}

/// mp_cstm (FUN_10143af0): the 0x044 instincts' icons in their slots, the species icon, the ten number fields.
fn draw_cstm(gu: &mut GameUi, w: &World, s: &Screen, sw: f32, sh: f32, focused: bool, rects: &mut Vec<[f32; 4]>) {
    // +0x63 = 0: the cursor child shows only while focused; a slot picked first stays marked meanwhile.
    let sel = if focused { Some(s.cstm) } else { s.slot_pick.map(|k| SLOT_CHILD[k]) };
    let Some(win) = gu.lobby_window_off(CSTM, sw, sh, (0.0, 0.0), sel, focused, &[]) else { return };
    super::winhits::record(w, CSTM, &win);
    let [x, y, ..] = win.frame;
    let child = |c: u16| win.children.iter().find(|e| e.0 == c).map(|e| e.1);
    for (i, &c) in SLOT_CHILD.iter().enumerate() {
        let n = monstrosity::slot(w, i);
        if n != 0
            && let Some(r) = child(c)
        {
            item_icon_big(gu, w, 0x7400 + n, r[0], r[1]);
        }
    }
    let sp = monstrosity::current_species(w);
    if sp != 0
        && let Some(r) = child(13)
    {
        item_icon_big(gu, w, 0xf000 + sp, r[0], r[1]);
    }
    // The ten fields: elements 0..7, Attack, Defense (0x061), at the equipment window's places + 4 in y.
    if let Some(st) = w.stats.as_ref() {
        for (k, f) in crate::inventory::EQUIP_FIELDS.iter().enumerate() {
            let v = crate::inventory::field_text(match k {
                0..8 => st.elem[k],
                8 => st.atk,
                _ => st.def,
            });
            let (fx, fy) = (x + f[0] as f32, y + f[1] as f32 + 4.0);
            if k >= 8 {
                gu.shape_text(&v, fx + f[2] as f32, fy, WHITE, true);
            } else {
                gu.shape_text(&v, fx, fy, WHITE, false);
            }
        }
    }
    rects.push(win.frame);
}

/// An item's icon at full size (32x32) in a slot.
fn item_icon_big(gu: &mut GameUi, w: &World, id: u16, x: f32, y: f32) {
    let Some(cache) = w.item_info.as_ref() else { return };
    super::items::upload_icons(gu, cache, std::iter::once(id));
    let icon = crate::render::invwin::icon_texture(id);
    if gu.has_image(&icon) {
        gu.image_rect(&icon, x, y, 32.0, 32.0, [1.0; 4]);
    }
}

/// The cost panel in level's place (FUN_10144530): "Cost", used / cap.
fn draw_cost(gu: &mut GameUi, w: &World, sw: f32, sh: f32, rects: &mut Vec<[f32; 4]>) {
    let Some([lx, ly, ..]) = gu.window_rect(LEVEL, sw, sh) else { return };
    let (x, y, fw, fh) = (lx, ly + 52.0, 112.0, 88.0);
    if !gu.window_sized(HELP, x, y, fw, fh) {
        return;
    }
    let label = w.battle.as_ref().and_then(|b| b.group9(0x102)).unwrap_or_default();
    gu.shape_text(&label, x + 7.0, y + 7.0, WHITE, false);
    gu.shape_text(&used(w).to_string(), x + 58.0, y + 24.0, WHITE, true);
    gu.shape_text("/", x + 59.0, y + 24.0, WHITE, false);
    gu.shape_text(&cap(w).to_string(), x + 68.0, y + 24.0, WHITE, false);
    rects.push([x, y, fw, fh]);
}

/// mp_kncst: the boxes' numbers right-aligned, the edited one coloured.
fn draw_kncst(gu: &mut GameUi, w: &World, k: &Kncst, sw: f32, sh: f32, rects: &mut Vec<[f32; 4]>) {
    let Some(win) = gu.lobby_window_off(KNCST, sw, sh, (0.0, 0.0), Some(k.child), k.editing == 0, &[]) else { return };
    super::winhits::record(w, KNCST, &win);
    for (c, v) in [(1u16, k.min), (2, k.max)] {
        if let Some(r) = win.children.iter().find(|e| e.0 == c).map(|e| e.1) {
            gu.shape_text(&v.to_string(), r[0] + r[2] - 2.0, r[1] + 2.0, if k.editing == c as u8 { EDITING } else { WHITE }, true);
        }
    }
    rects.push(win.frame);
}

/// Customization's windows in their order: mp_stat mode 1 and level mode 2 (or the cost panel), mp_cstm, mp_mncst,
/// the list and mp_mnnm / mp_mntp2 / mp_kncst over it.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(s) = &w.moncustom else { return rects };
    let free = w.confirm.is_none();
    rects.extend(super::status::draw_mon_custom(gu, w, sw, sh, !s.faculties));
    if s.faculties {
        draw_cost(gu, w, sw, sh, &mut rects);
    }
    draw_cstm(gu, w, s, sw, sh, free && s.focus == Focus::Cstm, &mut rects);
    // The group walk hides mp_mncst while the list (its group 0x81) is up.
    if s.list.is_none() || s.list == Some(Source::Instincts) && s.focus == Focus::Cstm && false {
        let off: Vec<u16> = if s.star { vec![1, 2, 3] } else { Vec::new() };
        if let Some(win) = gu.lobby_window_off(MNCST, sw, sh, (0.0, 0.0), Some(s.mncst), free && s.focus == Focus::Mncst, &[]) {
            super::winhits::record(w, MNCST, &win);
            if !off.is_empty() {
                gu.disabled_children(MNCST, win.frame[0], win.frame[1], &off, Some(s.mncst));
            }
            rects.push(win.frame);
        }
    }
    if let Some(src) = s.list {
        draw_list(gu, w, s, src, sw, sh, free && s.focus == Focus::List, &mut rects);
    }
    if let Some(r) = s.mnnm
        && let Some(win) = gu.lobby_window_off(MNNM, sw, sh, (0.0, 0.0), Some(r), free && s.focus == Focus::Mnnm, &[])
    {
        super::winhits::record(w, MNNM, &win);
        rects.push(win.frame);
    }
    if let Some(r) = s.mntp2 {
        let off: Vec<u16> = (1..=20).filter(|c| !TYPE_ROWS.contains(c)).collect();
        if let Some(win) = gu.lobby_window_off(MNTP2, sw, sh, (0.0, 0.0), Some(r), free, &[]) {
            super::winhits::record(w, MNTP2, &win);
            gu.disabled_children(MNTP2, win.frame[0], win.frame[1], &off, Some(r));
            rects.push(win.frame);
        }
    }
    if let Some(k) = &s.kncst {
        draw_kncst(gu, w, k, sw, sh, &mut rects);
    }
    rects
}

/// The mouse: the generic handler on the focused window, list rows as the list's own (a row moves the cursor, a click
/// on it is Enter).
#[allow(clippy::too_many_arguments)]
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(s) = w.moncustom.clone() else { return };
    let enter = |w: &mut World, gu: &GameUi, k: egui::Key| key(w, gu, k);
    match s.focus {
        Focus::Mncst => super::winhits::generic(w, gu, MNCST, s.mncst, 1..=3, p, moved, clicked, right, |w, id| set(w, |s| s.mncst = id), enter),
        Focus::Cstm => super::winhits::generic(w, gu, CSTM, s.cstm, 1..=12, p, moved, clicked, right, |w, id| set(w, |s| s.cstm = id), enter),
        Focus::Mnnm => super::winhits::generic(w, gu, MNNM, s.mnnm.unwrap_or(1), 1..=2, p, moved, clicked, right, |w, id| set(w, |s| s.mnnm = Some(id)), enter),
        Focus::Mntp2 => super::winhits::generic(w, gu, MNTP2, s.mntp2.unwrap_or(1), 1..=20, p, moved, clicked, right, |w, id| set(w, |s| s.mntp2 = Some(id)), enter),
        Focus::Kncst if s.kncst.is_some_and(|k| k.editing == 0) => {
            super::winhits::generic(w, gu, KNCST, s.kncst.map_or(1, |k| k.child), 1..=5, p, moved, clicked, right, |w, id| set(w, |s| if let Some(k) = s.kncst.as_mut() { k.child = id }), enter)
        }
        Focus::List => {
            let Some(src) = s.list else { return };
            let c = s.cursors[source_index(src)];
            let n = rows(w, gu, &s, src).len();
            super::winhits::generic(w, gu, INVNT, (c.0 - c.1 + 1) as u16, 1..=VISIBLE as u16, p, moved, clicked, right, |w, id| {
                set(w, |s| {
                    let row = s.cursors[source_index(src)].1 + id as usize - 1;
                    if row < n {
                        s.cursors[source_index(src)].0 = row;
                    }
                })
            }, enter);
        }
        Focus::Kncst => {}
    }
}

fn set(w: &mut World, f: impl FnOnce(&mut Screen)) {
    if let Some(s) = w.moncustom.as_mut() {
        f(s);
    }
}

/// The scroll bar or wheel on the focused list.
pub(super) fn bar(w: &mut World, n: usize, m: super::scrollbar::Msg) -> bool {
    let Some(s) = w.moncustom.as_mut() else { return false };
    let Some(src) = s.list else { return false };
    let c = &mut s.cursors[source_index(src)];
    let mut row = c.0.saturating_sub(c.1);
    let moved = super::scrollbar::paged(m, &mut c.1, &mut row, n, VISIBLE);
    c.0 = c.1 + row;
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_bytes() {
        let b = packet([0x17, 0], 5, 0x101, 1, &[3, 0xffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x30], [4, 9]);
        assert_eq!(b.len(), 0xa0);
        assert_eq!(&b[4..12], &[0x17, 0, 5, 0, 0x01, 0x01, 1, 0]);
        assert_eq!(&b[12..16], &[3, 0, 0xff, 0xff]);
        assert_eq!(&b[34..36], &[0x30, 0]);
        assert_eq!(&b[36..38], &[4, 9]);
    }

    #[test]
    fn equip_rules() {
        let cost = |n: u16| n as u32;
        let cur = [3u16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        // A new one in an empty slot.
        let mut p = [0u16; 12];
        assert!(equip_rule(&cur, &mut p, 1, 5, cost, 20));
        assert_eq!(p[1], 5);
        // The same instinct on its own slot removes it.
        let mut p = [0u16; 12];
        assert!(equip_rule(&cur, &mut p, 0, 3, cost, 20));
        assert_eq!(p[0], 0xffff);
        // Set elsewhere or pending elsewhere: refused.
        let mut p = [0u16; 12];
        assert!(!equip_rule(&cur, &mut p, 2, 3, cost, 20));
        let mut p = [0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(!equip_rule(&cur, &mut p, 2, 7, cost, 20));
        // Over the cap: refused and the pending slots kept.
        let mut p = [0u16; 12];
        assert!(!equip_rule(&cur, &mut p, 1, 18, cost, 20));
        assert_eq!(p, [0; 12]);
        // A swap on a set slot: the old one's cost leaves.
        let mut p = [0u16; 12];
        assert!(equip_rule(&cur, &mut p, 0, 19, cost, 20));
    }

    #[test]
    fn grid_wraps() {
        assert_eq!(grid_step(1, egui::Key::ArrowUp), 3);
        assert_eq!(grid_step(1, egui::Key::ArrowLeft), 10);
        assert_eq!(grid_step(10, egui::Key::ArrowRight), 1);
        assert_eq!(child_slot(4), 1);
        assert_eq!(child_slot(2), 4);
    }
}
