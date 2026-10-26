//! Dressing a mannequin: layout mode 4 (spec ~/ffxi/lab/research/mannequin_spec.md 5). "menu    inspect" in mannequin
//! mode (FUN_10134670: the 4 x 4 grid, child 17 "Remove all") with the Storage list "menu    inventor" at (200, 48);
//! c2s 0x03B equips, unequips and strips. The mannequin is lifted and turns (FUN_101ce6a0) under its own camera
//! (FUN_101d4760).

use super::World;
use super::game_ui::GameUi;
use super::layout::Layout;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::{SLOT_CHILD, Slot, VISIBLE_ROWS};
use glam::Vec3;

const GRID: &str = super::inspect::WINDOW;
const WINDOWPS: &str = "menu    windowps";
const REMOVE_ALL: u16 = 17;
const STORAGE: u8 = 2;
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const GREEN: u32 = 0x8050_9050;
const ORANGE: u32 = 0x8080_4020;
/// The slots a mannequin wears: main, sub, range, head, body, hands, legs, feet.
const WEARABLE: u16 = 0x1f7;
/// 3° a retail frame (2 ticks *(inf)*).
const SPIN: f32 = 0.052_36 / 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Focus {
    Grid,
    List,
}

/// The inspect window's mannequin mode (+0x14f slot, +0x150 container) and its list source (DAT_105781b4).
pub struct Dress {
    pub c: u8,
    pub slot: u8,
    /// +0x1c: 2 slot first, 1 item first.
    mode: u8,
    cursor: u16,
    /// +0x14a / +0x148: the column last left and the previous child.
    col: u16,
    prev: u16,
    focus: Focus,
    scroll: usize,
    row: usize,
    /// The list view's slot mask.
    mask: u16,
    /// Mode 1: the picked Storage slot and the grid slots it may go to (+0x20).
    picked: Option<u8>,
    allowed: u16,
    /// Object +0x34: the turn, back to 0 at each rebuild.
    spin: f32,
    look: Option<crate::dat::character::Look>,
    /// myroom +0x2d4: the camera's height, kept from earlier frames.
    h: f32,
}

/// FUN_100fe860: an equipment slot's mannequin index (main, sub, range, head .. feet); none for the rest.
fn idx(slot: usize) -> Option<usize> {
    match slot {
        0..=2 => Some(slot),
        4..=8 => Some(slot - 1),
        _ => None,
    }
}

fn slot_of(child: u16) -> Option<usize> {
    SLOT_CHILD.iter().position(|&c| c == child)
}

/// The inspect children of the slots a mannequin cannot wear (FUN_10133560: state 5).
fn off_children() -> Vec<u16> {
    (0..16).filter(|&s| idx(s).is_none()).map(|s| SLOT_CHILD[s]).collect()
}

fn dress(w: &World) -> Option<&Dress> {
    w.layout.as_ref()?.dress.as_ref()
}

fn dress_mut(w: &mut World) -> Option<&mut Dress> {
    w.layout.as_mut()?.dress.as_mut()
}

pub fn active(w: &World) -> bool {
    dress(w).is_some()
}

/// exdata[10..17]: the Storage slots this mannequin wears.
fn worn(w: &World, c: u8, slot: u8) -> [u8; 8] {
    w.inv.ext.get(&(c, slot)).map_or([0; 8], |e| e[10..18].try_into().unwrap())
}

/// FUN_101334c0(slot, -1, c): a mannequin of the Mog Safe or Safe 2 with a look record wears this Storage slot.
pub(super) fn worn_by_any(w: &World, s: u8) -> bool {
    w.mannequin_looks.keys().any(|&(c, k)| matches!(c, 1 | 9) && worn(w, c, k).contains(&s))
}

/// Storage view 2 (FUN_1021c7e0(mask, 2, ..)): pieces with a level, races and a slot in the mask, by (resource, id);
/// tint 0 (only ammo .. waist) drops a row.
fn rows(w: &World, d: &Dress) -> Vec<Slot> {
    let Some(info) = w.item_info.as_ref() else { return Vec::new() };
    let mut out: Vec<Slot> = w
        .inv
        .slots_of(STORAGE)
        .into_iter()
        .filter(|s| s.index != 0 && s.id != 0)
        .filter(|s| info.item(s.id).is_some_and(|it| it.level != 0 && it.races != 0 && it.slots & WEARABLE != 0 && it.slots & d.mask != 0))
        .collect();
    out.sort_by_key(|s| (info.item(s.id).map_or(0, |it| it.resource), s.id));
    out
}

/// FUN_10130d30, first match: the mannequin's race can't wear it grey; picked orange; worn by this one green; by
/// another grey; else white. Then FUN_101f1230: a lock other than 0, 5, 0x1b grey.
fn tint(w: &World, d: &Dress, s: &Slot) -> u32 {
    let race = w.inv.ext.get(&(d.c, d.slot)).map_or(0, |e| e[0x12]);
    let races = w.item_info.as_ref().and_then(|i| i.item(s.id)).map_or(0, |it| it.races);
    let t = if races & (1 << race) == 0 {
        GREY
    } else if d.picked == Some(s.index) {
        ORANGE
    } else if worn(w, d.c, d.slot).contains(&s.index) {
        GREEN
    } else if worn_by_any(w, s.index) {
        GREY
    } else {
        WHITE
    };
    let lock = w.inv.locks.get(&(STORAGE, s.index)).copied().unwrap_or(0);
    if !matches!(lock, 0 | 5 | 0x1b) { GREY } else { t }
}

fn row_at(w: &World, d: &Dress) -> Option<Slot> {
    rows(w, d).get(d.scroll + d.row).cloned()
}

/// c2s 0x03B's body after the header (FUN_100ed450 / FUN_100ed4c0 / FUN_100ed530): kind, the mannequin's container
/// and slot, its index, then the Storage side (2 and the slot to equip, else 0x12 and 0), and 0x12.
pub fn body(kind: u8, c: u8, slot: u8, idx: u8, c2: u8, slot2: u8) -> [u8; 28] {
    let mut b = [0u8; 28];
    (b[0], b[4], b[8], b[9], b[12], b[16], b[20]) = (kind, c, slot, idx, c2, slot2, 0x12);
    b
}

fn send(w: &mut World, b: [u8; 28]) {
    log::info!("mannequin: 0x03B {:02x?}", b);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x03B, body: b.to_vec() });
}

/// rmlo4 row 1 (FUN_10134670 then FUN_10133560 / FUN_101335e0): the windows open on Main, slot first, the list on
/// every piece; mode 4.
pub fn open(l: &mut Layout, c: u8, slot: u8) {
    log::info!("mannequin: dressing ({c}, {slot})");
    l.dress = Some(Dress { c, slot, mode: 2, cursor: 1, col: 0, prev: 1, focus: Focus::Grid, scroll: 0, row: 0, mask: 0xffff, picked: None, allowed: 0, spin: 0.0, look: None, h: 0.0 });
}

/// FUN_10134760 and the close hook FUN_101336d0: back to mode 1 with the mannequin selected.
fn close(w: &mut World) {
    let Some(l) = w.layout.as_mut() else { return };
    if let Some(d) = l.dress.take() {
        l.sel = Some((d.c, d.slot));
        super::layout::set_colors(l, 0x8080_8080);
    }
}

/// FUN_10133410: the grid cursor's slot (none, or one a mannequin can't wear, fails); a piece given and not worn by
/// this mannequin goes on, else that slot comes off. No race or slot check (as read).
fn put(w: &mut World, entry: Option<Slot>) -> bool {
    let Some(d) = dress(w) else { return false };
    let (c, slot) = (d.c, d.slot);
    let Some(i) = slot_of(d.cursor).and_then(idx) else { return false };
    match entry.filter(|e| !worn(w, c, slot).contains(&e.index)) {
        Some(e) => send(w, body(1, c, slot, i as u8, STORAGE, e.index)),
        None => send(w, body(2, c, slot, i as u8, 0x12, 0)),
    }
    true
}

/// The prompt's Yes (LAB_10134530): kind 5.
pub fn confirmed(w: &mut World) {
    let Some(d) = dress(w) else { return };
    let (c, slot) = (d.c, d.slot);
    send(w, body(5, c, slot, 0, 0x12, 0));
}

pub fn ask_text(gu: &GameUi) -> String {
    gu.lobby_msg(9, 0xe0, "Remove all equipment from the mannequin?")
}

/// Num+ (message 8): the grid in mode 2 to mode 1 on the list; the list in mode 1 with nothing picked back.
pub fn num_plus(w: &mut World) {
    let Some(d) = dress_mut(w) else { return };
    match (d.focus, d.mode, d.picked) {
        (Focus::Grid, 2, _) => (d.mode, d.focus) = (1, Focus::List),
        (Focus::List, 1, None) => (d.mode, d.focus, d.mask) = (2, Focus::Grid, 0xffff),
        _ => return,
    }
    w.sounds.push(super::equipwin::SE_FOCUS);
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    match dress(w).map(|d| d.focus) {
        Some(Focus::Grid) => grid_key(w, gu, key),
        Some(Focus::List) => list_key(w, key),
        None => {}
    }
}

/// The generic window's moves then FUN_10134250 (5.6).
fn grid_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(d) = dress(w) else { return };
    let (mode, cursor, allowed, picked, c, slot) = (d.mode, d.cursor, d.allowed, d.picked, d.c, d.slot);
    let any_worn = worn(w, c, slot).iter().any(|&s| s != 0);
    match key {
        // Mode 1: Left / Right walk the picked piece's slots in 0x1036f744 order, wrapping.
        K::ArrowLeft | K::ArrowRight if mode == 1 && allowed != 0 => {
            let order = super::equipwin::ORDER;
            let at = order.iter().position(|&c| c == cursor).unwrap_or(0);
            let next = (1..16).map(|k| if key == K::ArrowRight { order[(at + k) % 16] } else { order[(at + 16 - k) % 16] }).find(|&ch| slot_of(ch).is_some_and(|s| allowed & 1 << s != 0));
            if let (Some(ch), Some(d)) = (next, dress_mut(w)) {
                d.cursor = ch;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            let k = match key {
                K::ArrowUp => 2,
                K::ArrowDown => 3,
                K::ArrowRight => 4,
                _ => 5,
            };
            let Some(mut next) = gu.menu_links(GRID, cursor).map(|l| l[k] as u16).filter(|&n| n != 0) else { return };
            let Some(d) = dress_mut(w) else { return };
            // FUN_10134560: off Remove all, Up / Down land in the remembered column.
            if d.prev == REMOVE_ALL {
                next = (d.col * 4 + next).clamp(1, REMOVE_ALL);
            } else if next != REMOVE_ALL {
                d.col = (next - 1) / 4;
            }
            d.prev = next;
            if next != d.cursor {
                d.cursor = next;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter if cursor == REMOVE_ALL => {
            if any_worn {
                w.sounds.push(Ui::Select as u32);
                w.confirm = Some((super::Confirm::Layout(super::layout::Ask::Undress), false));
            } else {
                w.sounds.push(Ui::Error as u32);
            }
        }
        K::Enter if slot_of(cursor).and_then(idx).is_none() => w.sounds.push(Ui::Error as u32),
        // Mode 2: the list on that slot's pieces, from its top.
        K::Enter if mode == 2 => {
            let s = slot_of(cursor).unwrap_or(0);
            if let Some(d) = dress_mut(w) {
                (d.mask, d.scroll, d.focus) = (1 << s, 0, Focus::List);
            }
            let n = dress(w).map_or(0, |d| rows(w, d).len());
            if let Some(d) = dress_mut(w) {
                d.row = d.row.min(n.clamp(1, VISIBLE_ROWS) - 1);
            }
            w.sounds.push(Ui::Select as u32);
        }
        // Mode 1: the picked piece onto the cursor's slot; then the list again, unpicked.
        K::Enter => {
            let entry = picked.and_then(|i| w.inv.slots.get(&(STORAGE, i)).map(|&(id, count)| Slot { index: i, id, count, equipped: false, equipped_ex: false }));
            let ok = put(w, entry);
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            if let Some(d) = dress_mut(w) {
                (d.allowed, d.picked, d.focus) = (0, None, Focus::List);
            }
        }
        K::Escape if mode == 1 => {
            if let Some(d) = dress_mut(w) {
                (d.allowed, d.picked, d.focus) = (0, None, Focus::List);
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        _ => {}
    }
}

/// The list (FUN_101f6c70, source FUN_10130e40).
fn list_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(d) = dress(w) else { return };
    let rows = rows(w, d);
    let n = rows.len();
    let mode = d.mode;
    let item = rows.get(d.scroll + d.row).cloned();
    let grey = item.as_ref().is_none_or(|s| tint(w, d, s) == GREY);
    match key {
        K::ArrowUp | K::ArrowDown => {
            let Some(d) = dress_mut(w) else { return };
            let at = d.scroll + d.row;
            let to = if key == K::ArrowDown { (at + 1).min(n.saturating_sub(1)) } else { at.saturating_sub(1) };
            if to != at {
                if to < d.scroll {
                    d.scroll = to;
                } else if to >= d.scroll + VISIBLE_ROWS {
                    d.scroll = to + 1 - VISIBLE_ROWS;
                }
                d.row = to - d.scroll;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowLeft | K::ArrowRight => {
            let Some(d) = dress_mut(w) else { return };
            if super::items::page_keys(&mut d.scroll, &mut d.row, n, key == K::ArrowRight) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter if grey => w.sounds.push(Ui::Error as u32),
        // Mode 1: pick; the grid limited to the piece's slots, its cursor on the first of them.
        K::Enter if mode == 1 => {
            let Some(s) = item else { return };
            let bits = w.item_info.as_ref().and_then(|i| i.item(s.id)).map_or(0, |it| it.slots);
            let Some(d) = dress_mut(w) else { return };
            (d.picked, d.allowed, d.focus) = (Some(s.index), bits, Focus::Grid);
            if let Some(&first) = super::equipwin::ORDER.iter().find(|&&ch| slot_of(ch).is_some_and(|k| bits & 1 << k != 0)) {
                d.cursor = first;
            }
            w.sounds.push(Ui::Select as u32);
        }
        // Mode 2: the piece onto the grid's slot (or off it), the grid again.
        K::Enter => {
            if let Some(d) = dress_mut(w) {
                d.mask = 0xffff;
            }
            let ok = put(w, item);
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            if let Some(d) = dress_mut(w) {
                d.focus = Focus::Grid;
            }
        }
        K::Escape if mode == 1 => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Escape => {
            if let Some(d) = dress_mut(w) {
                (d.mask, d.focus) = (0xffff, Focus::Grid);
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// Once a frame (FUN_101ce6a0 state 4): 3° a retail frame; a rebuilt mannequin (a new look) starts again at 0.
pub fn update(w: &World, d: &mut Dress, ticks: f32) {
    let look = w.furniture.mannequins.iter().find(|m| m.key == (d.c, d.slot)).map(|m| m.look);
    if look != d.look {
        (d.look, d.spin) = (look, 0.0);
        return;
    }
    d.spin += SPIN * ticks;
    if d.spin > std::f32::consts::PI {
        d.spin -= std::f32::consts::TAU;
    }
}

/// A mannequin's place: the one being dressed lifted to y -0.3 at most and turned by its spin.
pub fn place(w: &World, id: u32, pos: Vec3, yaw: f32) -> (Vec3, f32) {
    let Some(d) = dress(w) else { return (pos, yaw) };
    if !w.furniture.mannequins.iter().any(|m| m.id == id && m.key == (d.c, d.slot)) {
        return (pos, yaw);
    }
    (Vec3::new(pos.x, pos.y.min(-0.3), pos.z), d.spin)
}

/// FUN_101d4760's target eye and look for the mannequin at `p` (lifted) whose middle is at height `h`: from the room's
/// axis side, 2.5 x (h + 0.3) away.
pub fn camera_target(p: Vec3, h: f32) -> Option<(Vec3, Vec3)> {
    let d = Vec3::new(-p.x, -0.3, -p.z).try_normalize()?;
    let k = (h + 0.3) * -2.5;
    Some((Vec3::new(p.x + k * d.x, h + k * d.y, p.z + k * d.z), Vec3::new(p.x, h, p.z)))
}

/// The dressing camera each frame; false when not dressing. h = halfway up to attach point 12 (posed), kept when the
/// point is missing.
pub fn camera(w: &mut World, ticks: f32) -> bool {
    let Some(d) = dress(w) else { return false };
    let Some(m) = w.furniture.mannequins.iter().find(|m| m.key == (d.c, d.slot)) else { return true };
    let (p, _) = place(w, m.id, m.pos, m.yaw);
    let top = w.actors.get(&m.id).and_then(|a| {
        let (bone, off) = *a.character.as_ref()?.skeleton.points.get(12)?;
        let bones = if a.bones.is_empty() { &a.rest } else { &a.bones };
        bones.get(bone as usize).map(|(q, t)| -(*t + *q * off).y * a.scale)
    });
    let mut h = d.h;
    if let Some(top) = top {
        h = p.y - top * 0.5;
    }
    if let Some(d) = dress_mut(w) {
        d.h = h;
    }
    let Some((e, l)) = camera_target(p, h) else { return true };
    let k = super::layout::ease(ticks);
    w.cam.look += (l - w.cam.look) * k;
    w.cam.eye += (e - w.cam.eye) * k;
    true
}

/// Item info mode 0: the piece the grid cursor's slot wears (FUN_101338f0), or the list's row.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let d = dress(w)?;
    let s = match d.focus {
        Focus::List => row_at(w, d)?.index,
        Focus::Grid => worn(w, d.c, d.slot)[idx(slot_of(d.cursor)?)?],
    };
    let &(id, _) = w.inv.slots.get(&(STORAGE, s))?;
    Some((id, w.inv.ext.get(&(STORAGE, s)).copied()))
}

/// Title slot 2 "Layout" (0xd 4); help slot 2 "Change the equipment on your mannequin." (0xe 0x25c), the list's
/// "Select an item." (0xe 3).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(d) = dress(w) else { return };
    bars.title[2] = gu.title_msg(4).unwrap_or_default();
    bars.help[2] = gu.menu_msg(if d.focus == Focus::List { 3 } else { 0x25c }, "");
}

/// The list's bar or wheel.
pub fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(d) = dress(w).filter(|d| d.focus == Focus::List && w.confirm.is_none()) else { return false };
    let n = rows(w, d).len();
    let Some(d) = dress_mut(w) else { return false };
    super::scrollbar::rows(m, &mut d.scroll, &mut d.row, n, VISIBLE_ROWS)
}

/// The mouse: a grid child under it takes the cursor and a click on the cursor's child is Enter; the list's rows as
/// the item list's; the pointer over the other window moves the focus there (message 8), as the Equipment screen.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(d) = dress(w) else { return };
    let (focus, cursor, scroll, row) = (d.focus, d.cursor, d.scroll, d.row);
    let n = rows(w, d).len();
    let list_row = super::post::picker_row(gu, sw, sh, p);
    let in_grid = super::trade::over(gu, GRID, sw, sh, p);
    if right && (in_grid || list_row.is_some()) {
        key(w, gu, egui::Key::Escape);
        return;
    }
    match focus {
        Focus::Grid => {
            if list_row.is_some() && (moved || clicked) {
                num_plus(w);
                return;
            }
            let Some(id) = super::trade::child_at(gu, GRID, sw, sh, p, 1..=REMOVE_ALL) else { return };
            gu.hover.set(true);
            if id != cursor && (moved || clicked) {
                if let Some(d) = dress_mut(w) {
                    (d.cursor, d.prev) = (id, id);
                    if id != REMOVE_ALL {
                        d.col = (id - 1) / 4;
                    }
                }
                w.sounds.push(Ui::Cursor as u32);
            } else if clicked {
                key(w, gu, egui::Key::Enter);
            }
        }
        Focus::List => {
            if in_grid && (moved || clicked) {
                num_plus(w);
                return;
            }
            let Some(r) = list_row.filter(|r| scroll + r < n) else { return };
            gu.hover.set(true);
            if r != row && (moved || clicked) {
                if let Some(d) = dress_mut(w) {
                    d.row = r;
                }
            } else if clicked {
                key(w, gu, egui::Key::Enter);
            }
        }
    }
}

/// FUN_10134180 / FUN_10133d70: the grid with the worn pieces' icons, child 17 as windowps 545 (546 dim with nothing
/// worn), anc_equi on a slot; the Storage list at (200, 48) with its tints.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(d), Some(info)) = (dress(w), w.item_info.clone()) else { return rects };
    let ext = worn(w, d.c, d.slot);
    let dim = ext.iter().all(|&s| s == 0);
    let focused = w.confirm.is_none();
    let on_grid = focused && d.focus == Focus::Grid;
    let slots_off = off_children();
    let mut hide = slots_off.clone();
    if dim {
        hide.push(REMOVE_ALL);
    }
    let swaps = [(REMOVE_ALL, Some(WINDOWPS), 545usize)];
    if let Some(lw) = gu.lobby_window_lib_swaps(GRID, sw, sh, (0.0, 0.0), on_grid.then_some(d.cursor), on_grid && d.cursor == REMOVE_ALL, &hide, &swaps) {
        let [x0, y0, ..] = lw.frame;
        gu.disabled_children(GRID, x0, y0, &slots_off, None);
        // A dim Remove all: windowps 546 drawn as a kind-4 sprite (FUN_10119c00 keeps the swapped sprite).
        if dim && let Some(e) = gu.layout(GRID).and_then(|l| l.elements.iter().find(|e| e.id == REMOVE_ALL).cloned()) {
            let on = on_grid && d.cursor == REMOVE_ALL;
            let dd = if on { 1.0 } else { 0.0 };
            gu.sprite(WINDOWPS, 546, x0 + e.x as f32 + dd, y0 + e.y as f32 + dd, if on { 0x8080_5020 } else { WHITE });
            if on {
                gu.cursor(x0 + e.x as f32, y0 + e.y as f32);
            }
        }
        let mut by_child = vec![(0u16, 0u32, false); 16];
        for (slot, &child) in SLOT_CHILD.iter().enumerate() {
            if let Some(i) = idx(slot)
                && ext[i] != 0
                && let Some(&(id, _)) = w.inv.slots.get(&(STORAGE, ext[i]))
            {
                by_child[child as usize - 1] = (id, 0, true);
            }
        }
        super::post::draw_slots(gu, &info, GRID, lw.frame, &by_child, None, on_grid.then_some(d.cursor), |_| [1.0; 4]);
        if on_grid
            && d.cursor != REMOVE_ALL
            && let Some(e) = gu.layout(GRID).and_then(|l| l.elements.iter().find(|e| e.id == d.cursor).cloned())
        {
            let n = gu.lib_len("anc     anc_equi").max(1);
            gu.sprite("anc     anc_equi", gu.cursor_frame() % n, x0 + e.x as f32, y0 + e.y as f32, WHITE);
        }
        rects.push(lw.frame);
    }
    let shown = rows(w, d);
    let tints: Vec<u32> = shown.iter().map(|s| tint(w, d, s)).collect();
    rects.extend(super::post::draw_rows(gu, &shown, &info, (d.scroll, d.row), focused && d.focus == Focus::List, Some(&tints), sw, sh));
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        let equip = [1, 0, 0, 0, 1, 0, 0, 0, 0x0a, 3, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0x12, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(body(1, 1, 10, 3, 2, 1), equip);
        let off = [2, 0, 0, 0, 1, 0, 0, 0, 0x0a, 0, 0, 0, 0x12, 0, 0, 0, 0, 0, 0, 0, 0x12, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(body(2, 1, 10, 0, 0x12, 0), off);
        let mut all = off;
        all[0] = 5;
        assert_eq!(body(5, 1, 10, 0, 0x12, 0), all);
    }

    #[test]
    fn indices() {
        let got: Vec<Option<usize>> = (0..16).map(idx).collect();
        assert_eq!(got, [Some(0), Some(1), Some(2), None, Some(3), Some(4), Some(5), Some(6), Some(7), None, None, None, None, None, None, None]);
        assert_eq!(off_children(), [13, 6, 8, 10, 14, 11, 15, 4]);
    }

    #[test]
    fn camera() {
        let (e, l) = camera_target(Vec3::new(-4.5, -0.3, 0.3), -1.175).unwrap();
        assert!((e - Vec3::new(-2.322, -1.320, 0.155)).length() < 0.01, "{e}");
        assert_eq!(l, Vec3::new(-4.5, -1.175, 0.3));
        assert!(camera_target(Vec3::new(0.0, 0.0, 0.0), -1.0).is_some());
    }
}
