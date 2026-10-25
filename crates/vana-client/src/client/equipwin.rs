//! The Equipment screen (spec ~/ffxi/lab/research/equip_screen_spec.md; FUN_10133130): "menu    persona", "money",
//! the item list "menu    inventor" at (314, 48) (source vt 0x103336c0) and the grid "menu    equip", with the list's
//! Storage Options row and "menu    mnstorag". Mode 2 (slot first): Enter on a slot lists its pieces; mode 1 (Num+,
//! item first): a picked item limits the grid to its slots. Equipping sends c2s 0x050 {index, slot, container}.
//! A PUP (main or sub) gets "menu    pupequip" first. Not done: its "Customize automaton" (FUN_10207670).

use super::World;
use super::game_ui::GameUi;
use super::items::{INVENTORY, Win};
use super::net::NetCommand;
use super::scrollbar::Owner;
use crate::audio::Ui;
use crate::inventory::{Equipment, ItemList, SLOT_CHILD, Slot, VISIBLE_ROWS, build_rows, gil_text};
use crate::render::invwin::{INVENTOR_BAR, push_equipment, push_item_list, push_money, push_persona};

const LIST: &str = "menu    inventor";
const GRID: &str = "menu    equip";
const STORE: &str = "menu    mnstorag";
/// The list's place: source vfunc +4 = 2 (FUN_101f1fe0: (314, 48)).
const LIST_X: i16 = 314;
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const GREEN: u32 = 0x8050_9050;
const ORANGE: u32 = 0x8080_4020;
/// SE 0013 (FUN_10037e90).
const SE_FOCUS: u32 = 13;
/// The grid children in a picked item's walk order (0x1036f744).
const ORDER: [u16; 16] = [1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15, 4, 8, 12, 16];
/// mnstorag's rows: Inventory, Wardrobe, Wardrobe 2..8.
const STORE_BAGS: [u8; 9] = [0, 8, 10, 11, 12, 13, 14, 15, 16];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Focus {
    Grid,
    List,
    /// mnstorag with its cursor child.
    Storage(u16),
}

pub struct EquipWin {
    /// The grid cursor child (1 + col * 4 + row).
    grid: u16,
    /// Equip +0x1c: 2 slot first, 1 item first.
    mode: u8,
    focus: Focus,
    /// The list's container (source +0x10) and its view's slot mask (0xffff all gear, 1 << slot after a grid Enter).
    container: u8,
    mask: u16,
    /// The list rows (row 0 the Storage Options strip) and the cursor row on screen.
    scroll: usize,
    cursor: usize,
    /// Mode 1: the picked item's index (source +4 / +5) and the slots it may go to (equip +0x20).
    picked: Option<u8>,
    allowed: u16,
    /// itemxinf: the compared piece (Some(None) for an empty slot), the slot-bit turn and Ctrl last frame.
    compare: Option<Option<(u8, u8)>>,
    alt: u8,
    ctrl: Option<bool>,
    /// The main menu's cursor to resume when the screen closes (FUN_101f2600 resumes menuwind).
    return_menu: Option<usize>,
}

fn slot_of(child: u16) -> usize {
    SLOT_CHILD.iter().position(|&c| c == child).unwrap_or(0)
}

fn win(w: &World) -> Option<&EquipWin> {
    match w.win.as_ref()? {
        Win::Equip(e) => Some(e),
        _ => None,
    }
}

fn win_mut(w: &mut World) -> Option<&mut EquipWin> {
    match w.win.as_mut()? {
        Win::Equip(e) => Some(e),
        _ => None,
    }
}

const PUPEQUIP: &str = "menu    pupequip";

/// "menu    pupequip": its cursor and the main menu row to return to.
pub struct PupMenu {
    sel: u16,
    return_menu: Option<usize>,
}

/// Main menu row 2 (FUN_10140400 case 2): PUP as main or sub job (0x12) opens pupequip, else the Equipment screen.
pub fn menu_open(w: &mut World) {
    if w.stats.as_ref().is_some_and(|s| s.main.0 == 0x12 || s.sub.0 == 0x12) {
        w.pupequip = Some(PupMenu { sel: w.cursor_recall(PUPEQUIP), return_menu: w.menu });
        w.menu = None;
    } else {
        open(w);
    }
}

/// pupequip's keys (generic window; Enter FUN_10140750(5, child)): 1 the Equipment screen, 2 Customize automaton.
pub fn pup_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(p) = w.pupequip.as_ref() else { return };
    let sel = p.sel;
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let to = gu.menu_links(PUPEQUIP, sel).map_or(sel, |l| l[if key == egui::Key::ArrowUp { 2 } else { 3 }] as u16);
            if to != sel && (1..=2).contains(&to) {
                if let Some(p) = w.pupequip.as_mut() {
                    p.sel = to;
                }
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            if let Some(p) = w.pupequip.take() {
                w.menu = p.return_menu;
            }
        }
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if sel == 1 {
                if let Some(p) = w.pupequip.take() {
                    w.menu = p.return_menu;
                }
                open(w);
            } else {
                log::info!("pupequip: Customize automaton (FUN_10207670) is not implemented yet");
            }
        }
        _ => {}
    }
}

/// pupequip's title 0xd 0x14 and its rows' help (0xe 0x284 / 0x285).
pub fn pup_bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(p) = &w.pupequip else { return };
    bars.title[1] = gu.title_msg(0x14).unwrap_or_default();
    bars.help[1] = gu.focus_text(PUPEQUIP, p.sel)[0].clone().unwrap_or_default();
}

pub fn pup_draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let p = w.pupequip.as_ref()?;
    gu.lobby_window_off(PUPEQUIP, sw, sh, (0.0, 0.0), Some(p.sel), w.confirm.is_none(), &[]).map(|win| win.frame)
}

/// FUN_10133130: persona, money, the list on the Inventory with every piece of gear, the grid (focused, mode 2); c2s
/// 0x061 {1}. The main menu is hidden and comes back on close.
pub fn open(w: &mut World) {
    // Refusals first: synthesis (status 0x2c, 7 0x4e), fishing (6, 7 0x4d), the inventory not all in yet (0x01D's
    // +0x198cc bit 0, 7 0xa8).
    let line = match w.my_motion.status {
        0x2c => Some(0x4e),
        6 => Some(0x4d),
        _ if w.inv_loaded & 1 == 0 => Some(0xa8),
        _ => None,
    };
    if let Some(l) = line {
        super::textcmd::refuse(w, l);
        return;
    }
    let _ = w.net.commands.send(NetCommand::StatusRequest(1));
    let return_menu = w.menu;
    w.win = Some(Win::Equip(Box::new(EquipWin { grid: 1, mode: 2, focus: Focus::Grid, container: INVENTORY, mask: 0xffff, scroll: 0, cursor: 0, picked: None, allowed: 0, compare: None, alt: 0, ctrl: None, return_menu })));
    w.menu = None;
    w.action_menu = None;
}

/// FUN_101332d0: everything closes; the main menu resumes where it was.
fn close(w: &mut World) {
    let back = win(w).and_then(|e| e.return_menu);
    w.win = None;
    w.menu = back;
}

/// View 2 of the list's container (FUN_1021c7e0 + FUN_10130540 step 1): slots 1..80 whose record has a level and races
/// and whose equip slots meet the mask, sorted by (resource, item id).
fn rows(w: &World, e: &EquipWin) -> Vec<Slot> {
    let Some(info) = w.item_info.as_ref() else { return Vec::new() };
    let mut out: Vec<Slot> = w
        .inv
        .slots_of(e.container)
        .into_iter()
        .filter(|s| s.index != 0 && s.id != 0)
        .filter(|s| info.item(s.id).is_some_and(|it| it.level != 0 && it.races != 0 && it.slots & e.mask != 0))
        .collect();
    out.sort_by_key(|s| (info.item(s.id).map_or(0, |it| it.resource), s.id));
    out
}

/// Wardrobe 3..8 need their lobby feature bit (PTR_DAT_103601bc + 0xb4 bits 2..7).
fn bag_open(w: &World, c: u8) -> bool {
    !(0xb..=0x10).contains(&c) || w.features & (0x04 << (c - 0xb)) != 0
}

/// FUN_10131f10: an equipment slot holds this container and index.
fn worn(w: &World, c: u8, index: u8) -> bool {
    w.inv.equip.contains(&Some((c, index)))
}

/// FUN_10130540 then FUN_101f1230's status grey: a closed wardrobe grey; the picked row orange; worn green; white
/// when it can be worn now, else grey; any lock status other than 0, 5, 0x1b grey (not over orange).
fn tint(w: &World, e: &EquipWin, s: &Slot) -> u32 {
    let picked = e.picked == Some(s.index);
    let t = if !bag_open(w, e.container) {
        GREY
    } else if picked {
        ORANGE
    } else if worn(w, e.container, s.index) {
        GREEN
    } else if w.item_info.as_ref().and_then(|i| i.item(s.id)).is_some_and(|it| super::shop::can_wear(w, &it, s.id)) {
        WHITE
    } else {
        GREY
    };
    let lock = w.inv.locks.get(&(e.container, s.index)).copied().unwrap_or(0);
    if !picked && !matches!(lock, 0 | 5 | 0x1b) { GREY } else { t }
}

fn n_rows(w: &World, e: &EquipWin) -> usize {
    1 + rows(w, e).len()
}

/// The list row under the cursor: None on the strip.
fn row_at(w: &World, e: &EquipWin) -> Option<Slot> {
    (e.scroll + e.cursor).checked_sub(1).and_then(|k| rows(w, e).get(k).cloned())
}

fn encumbered(w: &World, slot: usize) -> bool {
    w.job_info.as_ref().is_some_and(|j| j.encumbrance & (1 << slot) != 0)
}

/// FUN_10132b90: the grid's slot and the list's container through FUN_101327d0.
fn equip(w: &mut World, item: Option<Slot>) -> bool {
    let Some(e) = win(w) else { return false };
    let (slot, c) = (slot_of(e.grid), e.container);
    let Some(s) = item else { return false };
    try_equip(w, slot, c, s.index, s.id)
}

/// FUN_101327d0: the piece worn there picked again comes off (c2s 0x050 index 0); else the checks, then c2s 0x050
/// {index, slot, container}. False on a refusal (its group 7 line printed) or a silent failure. /equip uses it too.
pub(super) fn try_equip(w: &mut World, slot: usize, c: u8, index: u8, id: u16) -> bool {
    if encumbered(w, slot) {
        return false;
    }
    if w.inv.equip[slot] == Some((c, index)) {
        log::info!("equip: slot {slot} off (0x050 index 0)");
        let _ = w.net.commands.send(NetCommand::Equip { index: 0, slot: slot as u8, container: c });
        return true;
    }
    let Some(info) = w.item_info.clone() else { return false };
    let Some(it) = info.item(id) else { return false };
    // The sub-weapon rules: a weapon that also fits Main.
    if slot == 1 && it.item_type == 4 && it.slots & 1 != 0 {
        let st = w.stats.clone().unwrap_or_default();
        let dual = |j: u8| matches!(j, 6 | 13 | 16 | 19);
        let main = w.inv.equip[0].and_then(|(mc, mi)| w.inv.slots.get(&(mc, mi))).and_then(|&(id, _)| info.item(id));
        let refusal = if !dual(st.main.0) && !dual(st.sub.0) {
            Some(0x8b)
        } else if main.is_none() {
            Some(0x98)
        } else if main.is_some_and(|m| matches!(m.skill, 1 | 4 | 6 | 7 | 8 | 10 | 12)) {
            Some(0x99)
        } else {
            None
        };
        if let Some(id) = refusal {
            super::textcmd::refuse(w, id);
            return false;
        }
    }
    // FUN_10132050 (can wear now) and FUN_10132180 (one of the item's slots); a closed wardrobe fails.
    if !super::shop::can_wear(w, &it, id) || it.slots & (1 << slot) == 0 || !bag_open(w, c) {
        log::info!("equip: {id:#x} not into slot {slot}: wearable {}, slot fits {}, bag {c} open {}", super::shop::can_wear(w, &it, id), it.slots & (1 << slot) != 0, bag_open(w, c));
        return false;
    }
    log::info!("equip: {:?} into slot {slot} from container {c}", (index, id));
    let _ = w.net.commands.send(NetCommand::Equip { index, slot: slot as u8, container: c });
    true
}

/// Num+ (message 8): the grid in mode 2 goes to mode 1 on the list (FUN_101322b0); the list in mode 1 with nothing
/// picked back to mode 2 on the grid (FUN_101306f0); SE 0013.
pub fn num_plus(w: &mut World) {
    let Some(e) = win_mut(w) else { return };
    match (e.focus, e.mode, e.picked) {
        (Focus::Grid, 2, _) => (e.mode, e.focus) = (1, Focus::List),
        (Focus::List, 1, None) => (e.mode, e.focus, e.mask) = (2, Focus::Grid, 0xffff),
        _ => return,
    }
    e.compare = None;
    w.sounds.push(SE_FOCUS);
}

pub fn key(w: &mut World, key: egui::Key) {
    let Some(e) = win(w) else { return };
    let focus = e.focus;
    if let Some(e) = win_mut(w) {
        // FUN_101306f0: the list's messages 1..6 drop the compare.
        e.compare = None;
    }
    match focus {
        Focus::Grid => grid_key(w, key),
        Focus::List => list_key(w, key),
        Focus::Storage(o) => store_key(w, key, o),
    }
}

/// FUN_101322b0 (mode 2) and the mode 1 grid (FUN_10135c00-style walk over the allowed slots).
fn grid_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(e) = win(w) else { return };
    let (mode, grid, allowed, picked) = (e.mode, e.grid, e.allowed, e.picked);
    let free = mode == 2 || allowed == 0;
    match key {
        // FUN_10131840: Down +1 / Up -1 within the column, Right +4 / Left -4 across, wrapping.
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight if free => {
            let (col, row) = ((grid - 1) / 4, (grid - 1) % 4);
            let (col, row) = match key {
                K::ArrowDown => (col, (row + 1) % 4),
                K::ArrowUp => (col, (row + 3) % 4),
                K::ArrowRight => ((col + 1) % 4, row),
                _ => ((col + 3) % 4, row),
            };
            if let Some(e) = win_mut(w) {
                e.grid = 1 + col * 4 + row;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowLeft | K::ArrowRight => {
            let at = ORDER.iter().position(|&c| c == grid).unwrap_or(0);
            let next = (1..16).map(|k| if key == K::ArrowRight { ORDER[(at + k) % 16] } else { ORDER[(at + 16 - k) % 16] }).find(|&c| allowed & 1 << slot_of(c) != 0);
            if let (Some(c), Some(e)) = (next, win_mut(w)) {
                e.grid = c;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // Mode 2: an encumbered slot refuses; else the list on the container of the slot's piece (0 when empty),
        // filtered to the slot, its top row reset, the cursor row kept.
        K::Enter if mode == 2 => {
            let slot = slot_of(grid);
            if encumbered(w, slot) {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            let c = w.inv.equip[slot].map_or(INVENTORY, |(c, _)| c);
            if let Some(e) = win_mut(w) {
                (e.container, e.mask, e.scroll, e.focus) = (c, 1 << slot, 0, Focus::List);
            }
            let n = win(w).map_or(1, |e| n_rows(w, e));
            if let Some(e) = win_mut(w) {
                e.cursor = e.cursor.min(n.min(VISIBLE_ROWS) - 1);
            }
            w.sounds.push(Ui::Select as u32);
        }
        // Mode 1: the picked item into the grid slot (nothing picked fails); then the list again, unpicked.
        K::Enter => {
            let item = picked.and_then(|i| win(w).and_then(|e| rows(w, e).into_iter().find(|s| s.index == i)));
            let ok = equip(w, item);
            if !ok {
                w.sounds.push(Ui::Error as u32);
            }
            if let Some(e) = win_mut(w) {
                (e.allowed, e.picked, e.focus) = (0, None, Focus::List);
            }
        }
        K::Escape if mode == 1 => {
            if let Some(e) = win_mut(w) {
                (e.picked, e.focus) = (None, Focus::List);
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

/// The list (FUN_101f6c70 / FUN_101f2bb0 and the source's FUN_101306f0).
fn list_key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(e) = win(w) else { return };
    let n = n_rows(w, e);
    let mode = e.mode;
    let item = row_at(w, e);
    let grey = item.as_ref().is_some_and(|s| tint(w, e, s) == GREY);
    let on_strip = e.scroll + e.cursor == 0;
    let container = e.container;
    match key {
        // Up / Down one row, no wrap; plain Left / Right a page of 10 (source +0x1c = 1).
        K::ArrowUp | K::ArrowDown => {
            let Some(e) = win_mut(w) else { return };
            let at = e.scroll + e.cursor;
            let to = if key == K::ArrowDown { (at + 1).min(n - 1) } else { at.saturating_sub(1) };
            if to != at {
                if to < e.scroll {
                    e.scroll = to;
                } else if to >= e.scroll + VISIBLE_ROWS {
                    e.scroll = to + 1 - VISIBLE_ROWS;
                }
                e.cursor = to - e.scroll;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowLeft | K::ArrowRight => {
            let Some(e) = win_mut(w) else { return };
            if super::items::page_keys(&mut e.scroll, &mut e.cursor, n, key == K::ArrowRight) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // The strip (FUN_10130d00): mnstorag on the list's container.
        K::Enter if on_strip => {
            let child = STORE_BAGS.iter().position(|&b| b == container).unwrap_or(0) as u16 + 1;
            if let Some(e) = win_mut(w) {
                e.focus = Focus::Storage(child);
            }
            w.sounds.push(Ui::Select as u32);
        }
        // A grey row takes no Enter (SE 0004 from the list).
        K::Enter if grey => w.sounds.push(Ui::Error as u32),
        K::Enter if mode == 1 => {
            let Some(s) = item else { return };
            let bits = w.item_info.as_ref().and_then(|i| i.item(s.id)).map_or(0, |it| it.slots);
            let held = (0..16).find(|&k| w.inv.equip[k] == Some((container, s.index)));
            let Some(e) = win_mut(w) else { return };
            e.picked = Some(s.index);
            e.allowed = held.map_or(bits, |k| 1 << k);
            if let Some(&first) = ORDER.iter().find(|&&ch| e.allowed & 1 << slot_of(ch) != 0) {
                e.grid = first;
            }
            e.focus = Focus::Grid;
            w.sounds.push(Ui::Select as u32);
        }
        // Mode 2: the masks back to all gear, the piece into (or out of) the grid slot, the grid again.
        K::Enter => {
            if let Some(e) = win_mut(w) {
                e.mask = 0xffff;
            }
            let ok = equip(w, item);
            w.sounds.push(if ok { Ui::Select } else { Ui::Error } as u32);
            if let Some(e) = win_mut(w) {
                e.focus = Focus::Grid;
            }
        }
        K::Escape if mode == 1 => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Escape => {
            if let Some(e) = win_mut(w) {
                (e.mask, e.focus) = (0xffff, Focus::Grid);
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// mnstorag (FUN_10134d00): Up / Down wrap 1..9; Enter shows that container (rows 3..9 off without a size), Escape
/// closes; both give the list the focus back.
fn store_key(w: &mut World, key: egui::Key, o: u16) {
    use egui::Key as K;
    let off = store_off(w);
    let Some(e) = win_mut(w) else { return };
    match key {
        K::ArrowUp | K::ArrowDown => {
            e.focus = Focus::Storage(if key == K::ArrowDown { o % 9 + 1 } else if o == 1 { 9 } else { o - 1 });
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Enter if off.contains(&o) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            (e.container, e.scroll, e.cursor, e.focus) = (STORE_BAGS[o as usize - 1], 0, 0, Focus::List);
            w.sounds.push(Ui::Select as u32);
        }
        K::Escape => {
            e.focus = Focus::List;
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// FUN_10134b20: Wardrobe 2..8 rows off while the container has no size (s2c 0x01C ItemNum 0).
fn store_off(w: &World) -> Vec<u16> {
    (3..=9).filter(|&c| w.inv.sizes[STORE_BAGS[c as usize - 1] as usize] == 0).collect()
}

/// FUN_10130a80 per frame: a Ctrl press (key function 0x76, pad 0x89) opens itemxinf, the next closes it. Mode 1 walks
/// the focused item's slot bits in turn (rings, earrings) and moves the grid cursor there; mode 2 compares with the
/// grid slot's piece and moves nothing. The strip row compares nothing.
pub fn compare_key(w: &mut World, held: bool) {
    let info = w.item_info.clone();
    let Some(e) = win(w) else { return };
    let (focus, mode, grid) = (e.focus, e.mode, e.grid);
    let item = row_at(w, e);
    let equip = w.inv.equip;
    let Some(e) = win_mut(w) else { return };
    let rising = held && e.ctrl.is_some_and(|c| !c);
    e.ctrl = Some(held);
    if focus != Focus::List || !rising || e.compare.take().is_some() {
        return;
    }
    let Some(s) = item else { return };
    if mode == 2 {
        e.compare = Some(equip[slot_of(grid)]);
        return;
    }
    let bits = info.as_ref().and_then(|c| c.item(s.id)).map_or(0, |it| it.slots);
    // FUN_101317b0 takes the first or second set bit; two misses clear the turn (FUN_10130be0).
    for _ in 0..2 {
        let nth = (e.alt & 1) as usize;
        e.alt = (e.alt + 1) & 3;
        if let Some(slot) = (0..16).filter(|b| bits & (1 << b) != 0).nth(nth) {
            e.grid = SLOT_CHILD[slot];
            e.compare = Some(equip[slot]);
            return;
        }
    }
    e.alt = 0;
}

/// The compared piece while itemxinf is open: Some(None) for an empty slot; (item, extdata, container).
pub fn compare(w: &World) -> Option<Option<(u16, Option<[u8; 24]>, u8)>> {
    let e = win(w)?;
    let at = (*e.compare.as_ref()?).and_then(|(c, i)| Some((w.inv.slots.get(&(c, i))?.0, w.inv.ext.get(&(c, i)).copied(), c)));
    Some(at)
}

/// The item info window (mode 1): the list's cursor item while the list has the focus (none on the strip), else the
/// piece in the grid cursor's slot (FUN_101321d0); (item, extdata, container, index, equipped, mode).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>, u8, u8, bool, u8)> {
    let e = win(w)?;
    let (c, s) = match e.focus {
        Focus::List => (e.container, row_at(w, e)?),
        Focus::Storage(_) => return None,
        Focus::Grid => {
            let (c, i) = w.inv.equip[slot_of(e.grid)]?;
            let &(id, count) = w.inv.slots.get(&(c, i))?;
            (c, Slot { index: i, id, count, equipped: true, equipped_ex: false })
        }
    };
    Some((s.id, w.inv.ext.get(&(c, s.index)).copied(), c, s.index, worn(w, c, s.index), 1))
}

/// The bars' slot 1 window and child (FUN_10115380).
pub fn focus(w: &World) -> Option<(&'static str, u16)> {
    let e = win(w)?;
    Some(match e.focus {
        Focus::Grid => (GRID, e.grid),
        Focus::List => (LIST, e.cursor as u16 + 1),
        Focus::Storage(o) => (STORE, o),
    })
}

/// C.4: title slot 2 "Equipment" (0xd 0x14) all the time; the list's rows 0xe 0x3c9 in help slot 2, cleared on the
/// strip.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(e) = win(w) else { return };
    bars.title[2] = gu.title_msg(0x14).unwrap_or_default();
    if e.focus == Focus::List && e.scroll + e.cursor > 0 {
        bars.help[2] = gu.menu_msg(0x3c9, "");
    }
}

/// The scroll bar / wheel on the list (FUN_101f6c70).
pub fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(e) = win(w) else { return false };
    let n = n_rows(w, e);
    let Some(e) = win_mut(w) else { return false };
    super::scrollbar::rows(m, &mut e.scroll, &mut e.cursor, n, VISIBLE_ROWS)
}

/// Mouse on the list while it has the focus: hover moves the cursor, a click on the highlighted row acts as Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(e) = win(w) else { return };
    let Some(r) = gu.window_rect(LIST, sw, sh) else { return };
    let x0 = r[0] + LIST_X as f32 - gu.window_rect_x(LIST);
    // C.7 (FUN_10132c30 / FUN_101f27a0): the pointer over the other window sends message 8, as Num+: the grid's goes
    // to the list (mode 1), the list's back to the grid when nothing is picked.
    let in_list = p.0 >= x0 && p.0 < x0 + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    let in_grid = gu.window_rect(GRID, sw, sh).is_some_and(|g| p.0 >= g[0] && p.0 < g[0] + g[2] && p.1 >= g[1] && p.1 < g[1] + g[3]);
    if (moved || clicked) && ((e.focus == Focus::Grid && in_list) || (e.focus == Focus::List && in_grid)) {
        num_plus(w);
        return;
    }
    let Some(e) = win(w) else { return };
    if e.focus != Focus::List {
        return;
    }
    let row = ((p.1 - r[1] - 4.0) / crate::inventory::ROW_PITCH).floor();
    let inside = p.0 >= x0 && p.0 < x0 + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3] && row >= 0.0 && (row as usize) < VISIBLE_ROWS;
    if inside {
        gu.hover.set(true);
    }
    if !inside || !(moved || clicked) {
        return;
    }
    let n = n_rows(w, e);
    let Some(e) = win_mut(w) else { return };
    let row = (row as usize).min(n.saturating_sub(e.scroll + 1));
    let same = e.cursor == row;
    e.cursor = row;
    if clicked && same {
        key(w, egui::Key::Enter);
    }
}

/// The four windows: persona and money, the list at (314, 48) with the strip row and the wardrobe number, the grid with
/// the encumbrance crosses and the wardrobe numbers on wardrobe pieces; mnstorag over the list's place.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(e), Some(info)) = (win(w), w.item_info.clone()) else { return rects };
    let frame = gu.cursor_frame();
    let mut eq = Equipment::default();
    for (k, slot) in w.inv.equip.iter().enumerate() {
        if let Some((c, i)) = slot
            && let Some(&(id, count)) = w.inv.slots.get(&(*c, *i))
        {
            eq.slots[k] = Some(Slot { index: *i, id, count, equipped: true, equipped_ex: false });
        }
    }
    if let Some(st) = &w.stats {
        eq.fields[..8].copy_from_slice(&st.elem);
        eq.fields[8] = st.atk;
        eq.fields[9] = st.def;
    }
    super::items::upload_icons(gu, &info, eq.slots.iter().flatten().map(|s| s.id));
    let persona = super::items::persona(w);
    let gil = gil_text(w.inv.gil() as i64);
    let shown = rows(w, e);
    let mut list_rows = vec![crate::inventory::Row { id: 0, name: String::new(), count: None, tint: WHITE }];
    for (mut r, s) in build_rows(&shown, info.as_ref(), false).into_iter().zip(&shown) {
        r.tint = tint(w, e, s);
        list_rows.push(r);
    }
    let list = ItemList { rows: list_rows, scroll: e.scroll, cursor: e.cursor };
    super::items::upload_icons(gu, &info, list.visible().iter().map(|r| r.id).filter(|&id| id != 0));
    let open = [("menu    persona", None), ("menu    money", None), (LIST, Some(LIST_X)), (GRID, None)].map(|(n, x)| gu.gate_layout(n, sw, sh, x));
    let wardrobe = |c: u8| match c {
        8 => 1,
        10..=16 => c - 8,
        _ => 0,
    };
    let mut list_bar = None;
    {
        let (b, ui, style, font) = gu.parts();
        if let Some(m) = ui.menu("menu    persona").filter(|_| open[0]) {
            push_persona(b, ui, style, m, &persona, sw, sh);
        }
        if let Some(m) = ui.menu("menu    money").filter(|_| open[1]) {
            push_money(b, ui, style, m, &gil, sw, sh);
        }
        if let Some(m) = ui.menu(LIST).filter(|_| open[2]) {
            let mut m = m.clone();
            if let Some(el) = m.elements.first_mut() {
                el.x = LIST_X;
            }
            list_bar = Some(push_item_list(b, ui, style, font, GameUi::ATLAS, &m, &list, sw, sh, frame, 3.0, e.focus == Focus::List && w.confirm.is_none(), INVENTOR_BAR));
            // Row 0: framesus 185 ("Storage Options", its text baked in) at the row origin + (0, 2); the wardrobe number
            // right-aligned at (row x + 13, y + 4) (B.4; its badge sprites have alpha 0).
            if e.scroll == 0 {
                let (wx, wy) = crate::render::screens::window_pos(&m, sw, sh, 1.0, 1.0);
                if let Some(lib) = ui.lib("menu    framesus") {
                    b.push_sprite(ui, lib, 185, wx + 3.0, wy + 4.0 + 2.0, 1.0, 1.0, WHITE, 0);
                }
                let n = wardrobe(e.container);
                if n > 0 {
                    crate::render::screens::push_shape_text_right(b, ui, n.to_string().as_bytes(), wx + 3.0 + 13.0, wy + 4.0 + 4.0, WHITE);
                }
            }
        }
        if let Some(m) = ui.menu(GRID).filter(|_| open[3]) {
            let cursor = (e.focus == Focus::Grid && w.confirm.is_none()).then_some(e.grid as usize);
            push_equipment(b, ui, style, m, &eq, info.as_ref(), sw, sh, cursor, frame);
            // C.8: an encumbered slot's red cross (keytops3 429 at (+1, 0)); a wardrobe piece's number right-aligned at
            // (+30, +2).
            let (gx, gy) = crate::render::screens::window_pos(m, sw, sh, 1.0, 1.0);
            for slot in 0..16 {
                let Some(el) = m.elements.get(SLOT_CHILD[slot] as usize) else { continue };
                let (cx, cy) = (gx + el.x as f32, gy + el.y as f32);
                if encumbered(w, slot)
                    && let Some(lib) = ui.lib("menu    keytops3")
                {
                    b.push_sprite(ui, lib, 429, cx + 1.0, cy, 1.0, 1.0, WHITE, 0);
                }
                let n = w.inv.equip[slot].map_or(0, |(c, _)| wardrobe(c));
                if n > 0 {
                    crate::render::screens::push_shape_text_right(b, ui, n.to_string().as_bytes(), cx + 30.0, cy + 2.0, WHITE);
                }
            }
        }
    }
    if let Some([x, y, h]) = list_bar {
        gu.bar_site(Owner::EquipPick, x, y, h, list.rows.len(), VISIBLE_ROWS, list.scroll);
    }
    for n in ["menu    persona", "menu    money", GRID] {
        rects.extend(gu.window_rect(n, sw, sh));
    }
    if let Some(mut r) = gu.window_rect(LIST, sw, sh) {
        r[0] += LIST_X as f32 - gu.window_rect_x(LIST);
        rects.push(r);
    }
    if let Focus::Storage(o) = e.focus {
        let off = store_off(w);
        if let Some(win) = gu.lobby_window_off(STORE, sw, sh, (0.0, 0.0), Some(o), w.confirm.is_none(), &off) {
            gu.disabled_children(STORE, win.frame[0], win.frame[1], &off, Some(o));
            rects.push(win.frame);
        }
    }
    rects
}
