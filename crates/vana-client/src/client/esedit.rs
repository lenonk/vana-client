//! The equipment set slot editor (spec ~/ffxi/lab/research/social_rows_spec.md M2.6 / M2.7): "menu    mcresed" (the
//! 16-slot grid, an equipment window subclass), its item list ("menu    inventor" at (200, 48), three rows above the
//! items) and "menu    mcrmogc" (Storage Options). An item asks the server (c2s 0x052), whose s2c 0x116 replaces the
//! working copy; Remove / Keep change it here. The working copy goes back into the set when the editor closes.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::inventory::{ItemList, Row, SLOT_CHILD};
use std::time::{Duration, Instant};

const GRID: &str = "menu    mcresed";
const LIST: &str = "menu    inventor";
const MOGC: &str = "menu    mcrmogc";
/// The list's place: source vfunc +4 = 1 (FUN_101f1fe0: (200, 48)).
const LIST_X: i16 = 200;
const WHITE: u32 = 0x8080_8080;
/// SE 0013 (FUN_10037e90).
const SE_FOCUS: u32 = 13;
/// The grid children in a picked item's walk order (0x1036f744).
const ORDER: [u16; 16] = [1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15, 4, 8, 12, 16];
/// Rows above the items: the storage strip, "Remove piece.", "Keep same piece.".
const EXTRA: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Picked {
    Remove,
    Keep,
    Item(u8),
}

pub struct Editor {
    row: usize,
    /// +0x78: the working copy of the set's slots.
    work: [[u8; 4]; 16],
    /// +0x1c: 2 slot first (opens on the grid), 1 item first.
    mode: u8,
    grid: u16,
    on_list: bool,
    /// The list's container and its slot filter (FUN_10134f00), set by Enter on a grid slot in mode 2.
    container: u8,
    filter: Option<u16>,
    scroll: usize,
    cursor: usize,
    /// Mode 1's pick (source +4 / +5) and the slots it may go to (+0x20).
    picked: Option<Picked>,
    mask: u16,
    /// +0xbc: the last change (700 ms apart).
    last: Option<Instant>,
    mogc: Option<u16>,
}

fn ed_mut(w: &mut World) -> Option<&mut Editor> {
    w.macroedit.as_mut()?.sets.as_mut()?.ed.as_mut()
}

fn ed(w: &World) -> Option<&Editor> {
    w.macroedit.as_ref()?.sets.as_ref()?.ed.as_ref()
}

fn slot_of(child: u16) -> usize {
    SLOT_CHILD.iter().position(|&c| c == child).unwrap_or(0)
}

/// mcresmn's Edit (FUN_10134d90 / FUN_10135de0): the list on the inventory, the grid focused in mode 2.
pub fn open(w: &mut World, row: usize) -> Option<Editor> {
    let work = w.equipsets.as_ref()?.sets.get(row)?.slots;
    Some(Editor { row, work, mode: 2, grid: 1, on_list: false, container: 0, filter: None, scroll: 0, cursor: 0, picked: None, mask: 0, last: None, mogc: None })
}

/// FUN_10134e70 and the close hook (FUN_101358e0): the list and the grid close, the working copy goes into the set.
fn close(w: &mut World) {
    let Some(e) = w.macroedit.as_mut().and_then(|m| m.sets.as_mut()).and_then(|s| s.ed.take()) else { return };
    if let Some(s) = w.equipsets.as_mut().and_then(|es| es.sets.get_mut(e.row)) {
        s.slots = e.work;
    }
}

/// The list's items: view 2 of the container (FUN_10134f00 = FUN_1021c7e0, sorted by (resource, item id)) through the
/// slot filter, rows without a level or races dropped (FUN_10135120: record +0x14d0 / +0x14d8).
fn items(w: &World, e: &Editor) -> Vec<crate::inventory::Slot> {
    let Some(info) = w.item_info.as_ref() else { return Vec::new() };
    let mut out: Vec<crate::inventory::Slot> = w
        .inv
        .slots_of(e.container)
        .into_iter()
        .filter(|s| s.index != 0 && s.id != 0)
        .filter(|s| info.item(s.id).is_some_and(|it| it.level != 0 && it.races != 0 && e.filter.is_none_or(|m| it.slots & m != 0)))
        .collect();
    out.sort_by_key(|s| (info.item(s.id).map_or(0, |it| it.resource), s.id));
    out
}

/// FUN_10131f10: a list item is in the working set (container and index of any slot).
fn in_set(e: &Editor, c: u8, index: u8) -> bool {
    e.work.iter().any(|s| s[0] >> 2 == c && s[1] == index)
}

/// FUN_10135f00(item, kind) on the grid cursor's slot, refused within 700 ms of the last change: kind 0 keep, 2 remove
/// (here); kind 1 an item: the slot already holding it is cleared, else it leaves any other slot and c2s 0x052 asks
/// for it {slot @4, the entry @8, the 16 working entries @0xc}.
fn assign(w: &mut World, kind: u8, index: Option<u8>) -> bool {
    let inv_id = |w: &World, c: u8, i: u8| w.inv.slots.get(&(c, i)).map(|v| v.0);
    let Some(e) = ed(w) else { return false };
    if e.last.is_some_and(|t| t.elapsed() <= Duration::from_millis(700)) {
        return false;
    }
    let (slot, c) = (slot_of(e.grid), e.container);
    let item = index.and_then(|i| Some((i, inv_id(w, c, i)?)));
    let known = item.is_some_and(|(_, id)| w.item_info.as_ref().is_some_and(|info| info.item(id).is_some()));
    let Some(e) = ed_mut(w) else { return false };
    e.last = Some(Instant::now());
    match kind {
        0 => e.work[slot] = [0; 4],
        2 => e.work[slot] = [2, 0, 0, 0],
        _ => {
            let Some((i, id)) = item else { return false };
            let same = |s: &[u8; 4]| s[0] >> 2 == c && s[1] == i && u16::from_le_bytes([s[2], s[3]]) == id;
            if same(&e.work[slot]) {
                e.work[slot] = [0; 4];
                return true;
            }
            if let Some(k) = (0..16).find(|&k| k != slot && same(&e.work[k])) {
                e.work[k] = [0; 4];
            }
            if !known {
                return false;
            }
            let id = id.to_le_bytes();
            let mut b = vec![0u8; 0x48];
            b[0] = slot as u8;
            b[4..8].copy_from_slice(&[1 | c << 2, i, id[0], id[1]]);
            for (k, s) in e.work.iter().enumerate() {
                b[8 + 4 * k..12 + 4 * k].copy_from_slice(s);
            }
            log::info!("equip set editor: c2s 0x052 slot {slot} item {:?}", (c, i));
            let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x052, body: b });
        }
    }
    true
}

/// s2c 0x116 (FUN_10135e50): the server's slots replace the working copy.
pub fn on_valid(w: &mut World, slots: [[u8; 4]; 16]) {
    log::info!("s2c 0x116 {slots:?}");
    if let Some(e) = ed_mut(w) {
        e.work = slots;
    }
}

/// Num+ (message 8): the grid in mode 2 goes to mode 1 on the list (FUN_10135bd0); the list in mode 1 with nothing
/// picked back to mode 2 on the grid (FUN_10135270); SE 0013.
pub fn num_plus(w: &mut World) {
    let Some(e) = ed_mut(w) else { return };
    match (e.on_list, e.mode, e.picked) {
        (false, 2, _) => (e.mode, e.on_list) = (1, true),
        (true, 1, None) => (e.mode, e.on_list, e.filter) = (2, false, None),
        _ => return,
    }
    w.sounds.push(SE_FOCUS);
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(e) = ed(w) else { return };
    if let Some(o) = e.mogc {
        mogc_key(w, gu, key, o);
    } else if e.on_list {
        list_key(w, key);
    } else {
        grid_key(w, key);
    }
}

/// FUN_10135c00 / FUN_10135c40 / FUN_10135ab0 / FUN_10135ba0.
fn grid_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(e) = ed(w) else { return };
    let (mode, mask, grid, picked) = (e.mode, e.mask, e.grid, e.picked);
    let free = mode == 2 || mask == 0;
    match key {
        // FUN_10131840 as the equipment grid: within the column, across the columns, wrapping.
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight if free => {
            let (col, row) = ((grid - 1) / 4, (grid - 1) % 4);
            let (col, row) = match key {
                K::ArrowDown => (col, (row + 1) % 4),
                K::ArrowUp => (col, (row + 3) % 4),
                K::ArrowRight => ((col + 1) % 4, row),
                _ => ((col + 3) % 4, row),
            };
            if let Some(e) = ed_mut(w) {
                e.grid = 1 + col * 4 + row;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        // A picked item: Left / Right walk the allowed slots in ORDER (wrapping, not onto itself); Up / Down nothing.
        K::ArrowLeft | K::ArrowRight => {
            let at = ORDER.iter().position(|&c| c == grid).unwrap_or(0);
            let next = (1..16).map(|k| if key == K::ArrowRight { ORDER[(at + k) % 16] } else { ORDER[(at + 16 - k) % 16] }).find(|&c| mask & 1 << slot_of(c) != 0);
            if let (Some(c), Some(e)) = (next, ed_mut(w)) {
                e.grid = c;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // Mode 2: the list on the slot's container, filtered to the slot (its cursor on the top row).
        K::Enter if mode == 2 => {
            let slot = slot_of(grid);
            if let Some(e) = ed_mut(w) {
                (e.container, e.filter, e.scroll, e.cursor, e.on_list) = (e.work[slot][0] >> 2, Some(1 << slot), 0, 0, true);
            }
            w.sounds.push(Ui::Select as u32);
        }
        // Mode 1: the pick goes in (nothing picked fails), then the list again.
        K::Enter => {
            let ok = match picked {
                Some(Picked::Remove) => assign(w, 2, None),
                Some(Picked::Keep) => assign(w, 0, None),
                Some(Picked::Item(i)) => assign(w, 1, Some(i)),
                None => false,
            };
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            if let Some(e) = ed_mut(w) {
                (e.mask, e.picked, e.on_list) = (0, None, true);
            }
        }
        K::Escape if mode == 1 => {
            if let Some(e) = ed_mut(w) {
                (e.mask, e.picked, e.on_list) = (0, None, true);
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

/// The list source (FUN_101352c0 items, FUN_10135650 the rows above them).
fn list_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(e) = ed(w) else { return };
    let shown = items(w, e);
    let n = EXTRA + shown.len();
    let (mode, at) = (e.mode, e.scroll + e.cursor);
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            let Some(e) = ed_mut(w) else { return };
            let moved = match key {
                K::ArrowUp | K::ArrowDown => super::items::list_keys(&mut e.scroll, &mut e.cursor, n, key),
                _ => super::items::page_keys(&mut e.scroll, &mut e.cursor, n, key == K::ArrowRight),
            };
            if moved {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // The storage strip (FUN_10135520): mcrmogc on the list's container.
        K::Enter if at == 0 => {
            if let Some(e) = ed_mut(w) {
                e.mogc = Some(match e.container {
                    0 => 1,
                    1 => 2,
                    2 => 3,
                    c => c as u16,
                });
            }
            w.sounds.push(Ui::Select as u32);
        }
        // Remove / Keep (FUN_10135550 / FUN_101355d0): mode 1 picks them (no slot limit); mode 2 sets the grid slot.
        K::Enter if at < EXTRA => {
            let p = if at == 1 { Picked::Remove } else { Picked::Keep };
            let ok = mode == 1 || assign(w, if at == 1 { 2 } else { 0 }, None);
            if let Some(e) = ed_mut(w) {
                if mode == 1 {
                    (e.picked, e.mask) = (Some(p), 0);
                }
                (e.filter, e.on_list) = (if mode == 1 { e.filter } else { None }, false);
            }
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
        }
        // An item (FUN_10135190): mode 1 picks it, the grid limited to its slots (or the slot already holding it) with the
        // cursor on the first of them; mode 2 puts it in the grid slot.
        K::Enter => {
            let Some(s) = shown.get(at - EXTRA).cloned() else { return };
            let c = e.container;
            if mode == 1 {
                let bits = w.item_info.as_ref().and_then(|i| i.item(s.id)).map_or(0, |it| it.slots);
                let Some(e) = ed_mut(w) else { return };
                let held = (0..16).find(|&k| e.work[k][0] >> 2 == c && e.work[k][1] == s.index && u16::from_le_bytes([e.work[k][2], e.work[k][3]]) == s.id);
                e.mask = held.map_or(bits, |k| 1 << k);
                e.picked = Some(Picked::Item(s.index));
                if let Some(&first) = ORDER.iter().find(|&&ch| e.mask & 1 << slot_of(ch) != 0) {
                    e.grid = first;
                }
                e.on_list = false;
                w.sounds.push(Ui::Select as u32);
            } else {
                if let Some(e) = ed_mut(w) {
                    e.filter = None;
                }
                let ok = assign(w, 1, Some(s.index));
                if let Some(e) = ed_mut(w) {
                    e.on_list = false;
                }
                w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            }
        }
        // FUN_10135230: mode 1 closes everything; mode 2 drops the filter and goes back to the grid.
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            if mode == 1 {
                close(w);
            } else if let Some(e) = ed_mut(w) {
                (e.filter, e.on_list) = (None, false);
            }
        }
        _ => {}
    }
}

/// mcrmogc (FUN_10136300): child 1 -> container 0, 2 -> 1, 3 -> 2, the others their own number; Enter shows it.
fn mogc_key(w: &mut World, gu: &GameUi, key: egui::Key, o: u16) {
    use egui::Key as K;
    match key {
        K::ArrowUp | K::ArrowDown => {
            let to = gu.menu_links(MOGC, o).map_or(o, |l| l[if key == K::ArrowUp { 2 } else { 3 }] as u16);
            if (1..=16).contains(&to) && to != o {
                if let Some(e) = ed_mut(w) {
                    e.mogc = Some(to);
                }
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter => {
            if let Some(e) = ed_mut(w) {
                e.container = match o {
                    1 => 0,
                    2 => 1,
                    3 => 2,
                    c => c as u8,
                };
                (e.scroll, e.cursor, e.mogc) = (0, 0, None);
            }
            w.sounds.push(Ui::Select as u32);
        }
        K::Escape => {
            if let Some(e) = ed_mut(w) {
                e.mogc = None;
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// The mouse: mcrmogc's or the grid's children as the generic window; the list's rows as the item list (hover moves
/// the cursor, a click on the cursor row is Enter); the pointer over the other of grid and list moves the focus
/// (message 8), as the Equipment screen.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::macroedit::{child_under, generic};
    if w.modern {
        return;
    }
    let Some(e) = ed(w) else { return };
    if let Some(o) = e.mogc {
        generic(w, gu, MOGC, o, 1..=16, p, moved, clicked, right, |w, id| {
            if let Some(e) = ed_mut(w) {
                e.mogc = Some(id);
            }
        });
        return;
    }
    let (on_list, grid, mode, picked, scroll, cursor) = (e.on_list, e.grid, e.mode, e.picked, e.scroll, e.cursor);
    let n = EXTRA + items(w, e).len();
    let row = super::post::picker_row(gu, sw, sh, p);
    let (_, in_grid) = child_under(w, GRID, p, 1..=16);
    if (moved || clicked) && ((!on_list && row.is_some() && mode == 2) || (on_list && in_grid && mode == 1 && picked.is_none())) {
        num_plus(w);
        return;
    }
    if !on_list {
        let free = mode == 2 || ed(w).is_some_and(|e| e.mask == 0);
        generic(w, gu, GRID, grid, 1..=16, p, moved, clicked, right, |w, id| {
            if let Some(e) = ed_mut(w)
                && (free || e.mask & 1 << slot_of(id) != 0)
            {
                e.grid = id;
            }
        });
        return;
    }
    if right && row.is_some() {
        key(w, gu, egui::Key::Escape);
        return;
    }
    let Some(r) = row.filter(|r| scroll + r < n) else { return };
    gu.hover.set(true);
    if r != cursor && (moved || clicked) {
        if let Some(e) = ed_mut(w) {
            e.cursor = r;
        }
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The modern layer's view (this module keeps every rule and key): per grid child (1..16) the slot's state (0 keep, 1 an
/// item, 2 remove) and item; the cursor and focus; the list's container and its rows after the storage strip
/// (Remove, Keep, the items: item, name, count, colour) with the cursor's.
pub(super) struct View {
    pub slots: [(u8, u16); 16],
    pub grid: u16,
    pub on_list: bool,
    /// Children the cursor may take (mode 1 with an item picked: its slots).
    pub allowed: [bool; 16],
    pub container: u8,
    pub rows: Vec<(u16, String, u32, u32)>,
    pub cur: Option<usize>,
}

pub(super) fn view(w: &World) -> Option<View> {
    let e = ed(w)?;
    let info = w.item_info.as_ref()?;
    let group9 = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
    let mut rows: Vec<(u16, String, u32, u32)> = [(0x113, Picked::Remove), (0x114, Picked::Keep)].into_iter().map(|(k, p)| (0, group9(k), 0, if e.picked == Some(p) { 0x8080_4020 } else { 0x8090_9040 })).collect();
    let shown = items(w, e);
    for s in &shown {
        let tint = if e.picked == Some(Picked::Item(s.index)) {
            0x8080_4020
        } else if in_set(e, e.container, s.index) {
            0x8050_9050
        } else {
            WHITE
        };
        rows.push((s.id, info.item(s.id).map_or(String::new(), |it| it.name.clone()), s.count, tint));
    }
    Some(View {
        slots: std::array::from_fn(|k| {
            let x = e.work[slot_of(k as u16 + 1)];
            (x[0] & 3, u16::from_le_bytes([x[2], x[3]]))
        }),
        grid: e.grid,
        on_list: e.on_list && e.mogc.is_none(),
        allowed: std::array::from_fn(|k| e.mode == 2 || e.mask == 0 || e.mask & 1 << slot_of(k as u16 + 1) != 0),
        container: e.container,
        rows,
        cur: (e.scroll + e.cursor).checked_sub(1),
    })
}

/// The modern layer's mouse: the grid's cursor onto child `c` (in mode 1 only onto the picked item's slots), the focus
/// back on the grid; true when it moved.
pub(super) fn set_grid(w: &mut World, c: u16) -> bool {
    let Some(e) = ed_mut(w).filter(|e| e.mogc.is_none()) else { return false };
    if e.mode == 1 && e.mask != 0 && e.mask & 1 << slot_of(c) == 0 {
        return false;
    }
    let moved = e.grid != c || e.on_list;
    (e.grid, e.on_list) = (c, false);
    if moved && e.mode == 2 {
        e.filter = None;
    }
    moved
}

/// The list's cursor onto row `k` of the view's rows (the storage strip above them), the focus on the list.
pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(e) = ed_mut(w).filter(|e| e.mogc.is_none()) else { return false };
    let (scroll, cursor) = super::post::list_at(k + 1);
    let moved = e.scroll + e.cursor != k + 1 || !e.on_list;
    (e.scroll, e.cursor, e.on_list) = (scroll, cursor, true);
    moved
}

/// mcrmogc's pick without its window: the list on container `c` (child 1 -> 0, 2 -> 1, 3 -> 2, the others their own).
pub(super) fn set_container(w: &mut World, c: u8) {
    if let Some(e) = ed_mut(w).filter(|e| e.container != c) {
        (e.container, e.scroll, e.cursor, e.mogc) = (c, 0, 0, None);
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// The item info window's item (FUN_101360e0 on the grid: the slot's id whatever its state; the list's cursor item),
/// mode 0.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let e = ed(w)?;
    if e.mogc.is_some() {
        return None;
    }
    if !e.on_list {
        let s = e.work[slot_of(e.grid)];
        let id = u16::from_le_bytes([s[2], s[3]]);
        return (id != 0).then_some((id, None));
    }
    let at = (e.scroll + e.cursor).checked_sub(EXTRA)?;
    let s = items(w, e).get(at)?.clone();
    Some((s.id, w.inv.ext.get(&(e.container, s.index)).copied()))
}

/// The grid's help (group 0xe 0xf3..0x102 by child) and title "Equipment Sets", also in title slot 2 (FUN_10135920);
/// the list's: group 0xe 3 on items, 0xc 0x54 on Remove / Keep, none on the strip, the container's title.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(e) = ed(w) else { return };
    let focus = |bars: &mut super::hudtop::BarText, name: &str, child: u16| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    bars.title[2] = gu.title_msg(0xac).unwrap_or_default();
    match (e.mogc, e.on_list) {
        (Some(o), _) => focus(bars, MOGC, o),
        (None, false) => focus(bars, GRID, e.grid),
        (None, true) => {
            let at = e.scroll + e.cursor;
            bars.help[1] = match at {
                0 => String::new(),
                1 | 2 => gu.lobby_msg(0x0c, 0x54, ""),
                _ => gu.menu_msg(3, ""),
            };
            bars.title[1] = gu.title_msg(super::items::title_id(e.container)).unwrap_or_default();
        }
    }
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32, free: bool) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(e), Some(info)) = (ed(w), w.item_info.clone()) else { return rects };
    // The grid (FUN_101359c0): "remove" keytops3 429 at (+1, 0), an item its icon, "keep" the slot's label.
    super::items::upload_icons(gu, &info, e.work.iter().filter(|s| s[0] & 3 == 1).map(|s| u16::from_le_bytes([s[2], s[3]])));
    if let Some(win) = gu.lobby_window_off(GRID, sw, sh, (0.0, 0.0), Some(e.grid), free && !e.on_list && e.mogc.is_none(), &[]) {
        super::macroedit::hit(w, GRID, &win);
        for &(child, r) in win.children.iter().filter(|(c, _)| (1..=16).contains(c)) {
            let s = e.work[slot_of(child)];
            match s[0] & 3 {
                2 => _ = gu.sprite("menu    keytops3", 429, r[0] + 1.0, r[1], WHITE),
                1 => {
                    let name = crate::render::invwin::icon_texture(u16::from_le_bytes([s[2], s[3]]));
                    if gu.has_image(&name) {
                        gu.image_rect(&name, r[0], r[1], 32.0, 32.0, [1.0; 4]);
                    }
                }
                _ => {}
            }
        }
        rects.push(win.frame);
    }
    // The list: picked orange, in the set green, else white; Remove / Keep yellow (orange when picked).
    let shown = items(w, e);
    let mut rows = vec![Row { id: 0, name: String::new(), count: None, tint: WHITE }];
    let group9 = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
    for (k, p) in [(0x113, Picked::Remove), (0x114, Picked::Keep)] {
        rows.push(Row { id: 0, name: group9(k), count: None, tint: if e.picked == Some(p) { 0x8080_4020 } else { 0x8090_9040 } });
    }
    for (mut r, s) in crate::inventory::build_rows(&shown, info.as_ref(), false).into_iter().zip(&shown) {
        r.tint = if e.picked == Some(Picked::Item(s.index)) {
            0x8080_4020
        } else if in_set(e, e.container, s.index) {
            0x8050_9050
        } else {
            WHITE
        };
        rows.push(r);
    }
    let list = ItemList { rows, scroll: e.scroll, cursor: e.cursor };
    super::items::upload_icons(gu, &info, list.visible().iter().map(|r| r.id).filter(|&id| id != 0));
    let open = gu.gate_layout(LIST, sw, sh, Some(LIST_X));
    let frame = gu.cursor_frame();
    let (b, ui, style, font) = gu.parts();
    if let Some(m) = ui.menu(LIST).filter(|_| open) {
        let mut m = m.clone();
        if let Some(el) = m.elements.first_mut() {
            el.x = LIST_X;
        }
        crate::render::invwin::push_item_list(b, ui, style, font, GameUi::ATLAS, &m, &list, sw, sh, frame, 3.0, free && e.on_list && e.mogc.is_none(), crate::render::invwin::INVENTOR_BAR);
        // The storage strip (cell 1 kind 0x41: framesus 185, its text baked in) at the row origin + (0, 2); cell 0 kind
        // 0x40 the wardrobe number right-aligned at (row x + 13, y + 4) (its badge sprites have alpha 0).
        if e.scroll == 0 {
            let (wx, wy) = crate::render::screens::window_pos(&m, sw, sh, 1.0, 1.0);
            if let Some(lib) = ui.lib("menu    framesus") {
                b.push_sprite(ui, lib, 185, wx + 3.0, wy + 4.0 + 2.0, 1.0, 1.0, WHITE, 0);
            }
            let n = match e.container {
                8 => 1,
                c @ 10..=16 => c - 8,
                _ => 0,
            };
            if n > 0 {
                crate::render::screens::push_shape_text_right(b, ui, n.to_string().as_bytes(), wx + 3.0 + 13.0, wy + 4.0 + 4.0, WHITE);
            }
        }
    }
    if let Some(mut r) = gu.window_rect(LIST, sw, sh) {
        r[0] += LIST_X as f32 - gu.window_rect_x(LIST);
        rects.push(r);
    }
    if let Some(o) = e.mogc
        && let Some(win) = gu.lobby_window_off(MOGC, sw, sh, (0.0, 0.0), Some(o), free, &[])
    {
        super::macroedit::hit(w, MOGC, &win);
        rects.push(win.frame);
    }
    rects
}
