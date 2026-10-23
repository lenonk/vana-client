//! Items and Equipment in the live client: containers from s2c 0x01C / 0x01E / 0x01F / 0x020 / 0x050,
//! the "inventor", "equip", "persona" and "money" windows (inventory.rs models, render/invwin.rs
//! drawing); equipping sends c2s 0x050 (FUN_100e25d0: index, slot, container; index 0 removes).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::dat::items::Item;
use crate::inventory::{Equipment, ItemInfo, ItemList, Persona, ROW_PITCH, SLOT_CHILD, Slot, Stats, VISIBLE_ROWS, build_rows, gil_text};
use crate::render::invwin::{icon_texture, push_equipment, push_inventory, push_item_list, push_money, push_persona};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Main inventory (LSB LOC_INVENTORY).
pub const INVENTORY: u8 = 0;
/// The recycle bin's container.
const BIN: u8 = 17;
/// The inventory list's x beside the bin (the partner source's position kind 1, FUN_101f1fe0).
const BIN_LIST_X: i16 = 200;
/// Item list x while equipping.
const LIST_X: i16 = 314;

pub(super) const JOBS: [&str; 23] = ["", "WAR", "MNK", "WHM", "BLM", "RDM", "THF", "PLD", "DRK", "BST", "BRD", "RNG", "SAM", "NIN", "DRG", "SMN", "BLU", "COR", "PUP", "DNC", "SCH", "GEO", "RUN"];

/// s2c 0x061 (GP_SERV_COMMAND_CLISTATUS) fields the status panel shows.
#[derive(Clone, Debug, Default)]
pub struct CharStats {
    pub hp_max: u32,
    pub mp_max: u32,
    pub main: (u8, u8),
    pub sub: (u8, u8),
    pub base: [i32; 7],
    pub adj: [i32; 7],
    pub atk: i32,
    pub def: i32,
    pub elem: [i32; 8],
    pub su: u8,
    pub ilvl: u8,
}

#[derive(Default)]
pub struct Inventory {
    /// 0x01C ItemNum: 1 + size per container.
    pub sizes: [u8; 18],
    /// (container, index) -> (item id, count).
    pub slots: BTreeMap<(u8, u8), (u16, u32)>,
    /// Equipment slot -> (container, index).
    pub equip: [Option<(u8, u8)>; 16],
    /// (container, index) -> LockFlg (s2c 0x01F / 0x020).
    pub locks: BTreeMap<(u8, u8), u8>,
    /// (container, index) -> the item's 24-byte extdata (s2c 0x020 Attr).
    pub ext: BTreeMap<(u8, u8), [u8; 24]>,
    /// The list's view per container (FUN_1021c210 / FUN_1021c5b0 append a new slot, FUN_1021c680 drops an emptied
    /// one; sorting reorders it): slot indexes.
    pub order: BTreeMap<u8, Vec<u8>>,
}

impl Inventory {
    pub fn set(&mut self, container: u8, index: u8, id: u16, count: u32) {
        if id == 0 {
            self.slots.remove(&(container, index));
            self.ext.remove(&(container, index));
            self.locks.remove(&(container, index));
            self.unlist(container, index);
        } else {
            self.slots.insert((container, index), (id, count));
            let o = self.order.entry(container).or_default();
            if !o.contains(&index) {
                o.push(index);
            }
        }
    }

    fn unlist(&mut self, container: u8, index: u8) {
        if let Some(o) = self.order.get_mut(&container) {
            o.retain(|&i| i != index);
        }
    }

    /// The occupied slots in the view's order.
    pub fn viewed(&self, container: u8) -> Vec<Slot> {
        let slots = self.slots_of(container);
        let o = self.order.get(&container).map(Vec::as_slice).unwrap_or_default();
        o.iter().filter_map(|i| slots.iter().find(|s| s.index == *i).cloned()).collect()
    }

    pub fn count(&mut self, container: u8, index: u8, count: u32) {
        if count == 0 {
            self.slots.remove(&(container, index));
            self.unlist(container, index);
        } else if let Some(s) = self.slots.get_mut(&(container, index)) {
            s.1 = count;
        }
    }

    pub fn set_equip(&mut self, slot: u8, index: u8, container: u8) {
        if let Some(e) = self.equip.get_mut(slot as usize) {
            *e = (index != 0).then_some((container, index));
        }
    }

    /// The occupied slots of a container, in order, with their equipped flag.
    pub fn slots_of(&self, container: u8) -> Vec<Slot> {
        self.slots
            .range((container, 0)..=(container, 255))
            .map(|(&(c, i), &(id, count))| Slot { index: i, id, count, equipped: self.equip.contains(&Some((c, i))), equipped_ex: false })
            .collect()
    }

    pub fn gil(&self) -> u32 {
        self.slots.get(&(INVENTORY, 0)).map_or(0, |s| s.1)
    }
}

/// Item records by id, read once (the files are read on demand).
pub struct ItemCache {
    names: Arc<crate::dat::names::Names>,
    cache: RefCell<HashMap<u16, Option<Arc<Item>>>>,
}

impl ItemCache {
    pub fn new(names: Arc<crate::dat::names::Names>) -> Self {
        ItemCache { names, cache: RefCell::new(HashMap::new()) }
    }

    pub fn item(&self, id: u16) -> Option<Arc<Item>> {
        self.cache.borrow_mut().entry(id).or_insert_with(|| self.names.items.item(id as u32).map(Arc::new)).clone()
    }
}

impl ItemInfo for ItemCache {
    fn name(&self, id: u16) -> Option<String> {
        self.item(id).map(|i| i.name.clone())
    }
    fn stack(&self, id: u16) -> u16 {
        self.item(id).map_or(1, |i| i.stack)
    }
    fn kind(&self, id: u16) -> u16 {
        self.item(id).map_or(0, |i| i.item_type)
    }
    fn flags(&self, id: u16) -> u16 {
        self.item(id).map_or(0, |i| i.flags)
    }
    fn icon(&self, id: u16) -> Option<Vec<u8>> {
        let (w, h, rgba) = self.names.items.icon(id as u32)?;
        (w == 32 && h == 32).then_some(rgba)
    }
}

/// An open window and its cursor.
pub enum Win {
    /// Main menu Items: the inventory list, and iuse / sortyn over it. `direct`: the action menu's list of usable items,
    /// where Enter uses at once (FUN_1012b880 case 8, FUN_101f0a10).
    Items { scroll: usize, cursor: usize, sub: Option<Sub>, direct: bool, sort: Option<SortWin>, manual: Option<Option<u8>>, bin: Option<Bin> },
    /// Equipment: grid cursor (row, column), and the list of candidates once a slot is picked.
    Equip { row: usize, col: usize, pick: Option<Pick> },
}

/// "menu    itmsort2" with the focus (FUN_101f36b0 / FUN_101f3740): 1 Auto, 2 Manual, 3 Recycle Bin; `ask` is the
/// Auto prompt (sortyn "Sort items?", Yes / No).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SortWin {
    pub cursor: u16,
    pub ask: Option<bool>,
}

/// The recycle bin view (FUN_101daf70(0x11) -> FUN_101dc880 / FUN_101dc700): "menu    bank" on container 17 at (16, 48)
/// beside the inventory list at (200, 48), its rows all grey; the list's scroll / cursor stay the inventory side's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bin {
    pub scroll: usize,
    pub cursor: usize,
    /// Focus on the bin (else the inventory side).
    pub on_bin: bool,
    /// "menu    itemctrl" for a stack.
    pub qty: Option<Qty>,
    /// Opened from itmsort2 (list +0x90): Escape returns there, else the view closes.
    pub from_menu: bool,
}

/// The quantity window's pick: the bin slot, the value 1..max.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qty {
    pub index: u8,
    pub value: u32,
    pub max: u32,
}

/// The windows over the list: "menu    iuse" (FUN_101f2bb0: 1 Use, 2 Drop) for the slot it opened on, and
/// "menu    sortyn" for Drop (FUN_10222770: Yes / No, default No; iuse stays under it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sub {
    Use { child: u16, index: u8, id: u16 },
    Drop { index: u8, id: u16, yes: bool },
}

pub struct Pick {
    /// Container slot indexes of the candidates.
    pub indexes: Vec<u8>,
    pub scroll: usize,
    pub cursor: usize,
    /// itemxinf: the worn piece compared (Some(None) for an empty slot), the slot-bit turn and Ctrl last frame.
    pub compare: Option<Option<(u8, u8)>>,
    pub alt: u8,
    pub ctrl: Option<bool>,
}

pub fn open_items(w: &mut World) {
    w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: false, sort: None, manual: None, bin: None });
    w.menu = None;
    w.action_menu = None;
}

/// The action menu's Items: only usable items, no iuse or sort window.
pub fn open_direct(w: &mut World) {
    w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: true, sort: None, manual: None, bin: None });
    w.menu = None;
    w.action_menu = None;
}

/// The list's rows: the occupied inventory slots but gil; for the direct list only those FUN_101f0810 colours
/// (crystals, maze tabulae, usable non-equipment, equipped charged gear).
fn listed(w: &World, direct: bool) -> Vec<Slot> {
    let info = w.item_info.as_ref();
    w.inv
        .viewed(INVENTORY)
        .into_iter()
        .filter(|s| s.index != 0 && s.id != 0)
        .filter(|s| {
            !direct
                || info.and_then(|c| c.item(s.id)).is_some_and(|it| {
                    matches!(it.item_type, 8 | 0x16 | 0x17) || (!matches!(it.item_type, 4 | 5) && it.flags & 0x400 != 0) || (matches!(it.item_type, 4 | 5) && it.max_charges != 0 && s.equipped)
                })
        })
        .collect()
}

pub fn open_equipment(w: &mut World) {
    // FUN_1012ec60 asks for fresh stats (FUN_100967e0(0, 1)).
    let _ = w.net.commands.send(NetCommand::StatusRequest(1));
    w.win = Some(Win::Equip { row: 0, col: 0, pick: None });
    w.menu = None;
    w.action_menu = None;
}

/// The focused window and its cursor child, for the bars' slot 1 (FUN_10115380). The equipment's candidate
/// list is taken as an inventor list (inferred).
pub fn focus(w: &World) -> Option<(&'static str, u16)> {
    match w.win.as_ref()? {
        Win::Items { sub: Some(Sub::Use { child, .. }), .. } => Some(("menu    iuse", *child)),
        Win::Items { sub: Some(Sub::Drop { yes, .. }), .. } => Some(("menu    sortyn", if *yes { 1 } else { 2 })),
        Win::Items { sort: Some(SortWin { ask: Some(yes), .. }), .. } => Some(("menu    sortyn", if *yes { 1 } else { 2 })),
        Win::Items { sort: Some(s), .. } => Some(("menu    itmsort2", s.cursor)),
        Win::Items { bin: Some(b), cursor, .. } => Some(match (b.qty, b.on_bin) {
            (Some(_), _) => ("menu    itemctrl", 1),
            (None, true) => ("menu    bank", b.cursor as u16 + 1),
            (None, false) => ("menu    inventor", *cursor as u16 + 1),
        }),
        Win::Items { cursor, .. } => Some(("menu    inventor", *cursor as u16 + 1)),
        Win::Equip { pick: Some(p), .. } => Some(("menu    inventor", p.cursor as u16 + 1)),
        Win::Equip { row, col, pick: None } => Some(("menu    equip", (1 + col * 4 + row) as u16)),
    }
}

/// Equipment slot of grid cell (row, col): children run 1 + col * 4 + row (table 0x103639d4).
fn slot_at(row: usize, col: usize) -> usize {
    let child = (1 + col * 4 + row) as u16;
    SLOT_CHILD.iter().position(|&c| c == child).unwrap_or(0)
}

fn list_keys(scroll: &mut usize, cursor: &mut usize, n: usize, key: egui::Key) {
    if n == 0 {
        return;
    }
    let mut at = *scroll + *cursor;
    match key {
        egui::Key::ArrowDown => at = (at + 1) % n,
        egui::Key::ArrowUp => at = (at + n - 1) % n,
        _ => return,
    }
    if at < *scroll {
        *scroll = at;
    } else if at >= *scroll + VISIBLE_ROWS {
        *scroll = at + 1 - VISIBLE_ROWS;
    }
    *cursor = at - *scroll;
}

/// Keys while a window is open. Returns true when the key was used.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    let Some(info) = w.item_info.clone() else {
        if key == egui::Key::Escape {
            w.win = None;
        }
        return w.win.is_some() || key == egui::Key::Escape;
    };
    let direct = matches!(w.win, Some(Win::Items { direct: true, .. }));
    let rows = listed(w, direct).len();
    let shown = listed(w, direct);
    if matches!(w.win, Some(Win::Items { bin: Some(_), .. })) {
        bin_key(w, &info, key);
        return true;
    }
    let mut send = None;
    let mut close = false;
    let mut open_recycle = false;
    match &mut w.win {
        None => return false,
        // iuse (FUN_101f33a0): Up / Down between Use and Drop, Enter on a disabled child SE 0004, Escape back to the
        // list. sortyn (FUN_102228b0): Yes / No, Enter or Escape answers.
        Some(Win::Items { sub: sub @ Some(_), .. }) => {
            use crate::audio::Ui;
            match sub.unwrap() {
                Sub::Use { child, index, id } => match key {
                    egui::Key::ArrowUp | egui::Key::ArrowDown => {
                        *sub = Some(Sub::Use { child: 3 - child, index, id });
                        w.sounds.push(Ui::Cursor as u32);
                    }
                    egui::Key::Escape => {
                        *sub = None;
                        w.sounds.push(Ui::Cancel as u32);
                    }
                    egui::Key::Enter => {
                        let ok = iuse_states(w, index, id)[child as usize - 1];
                        if !ok {
                            w.sounds.push(Ui::Error as u32);
                        } else if child == 2 {
                            if let Some(Win::Items { sub, .. }) = &mut w.win {
                                *sub = Some(Sub::Drop { index, id, yes: false });
                            }
                            w.sounds.push(Ui::Select as u32);
                        } else {
                            if let Some(Win::Items { sub, .. }) = &mut w.win {
                                *sub = None;
                            }
                            w.sounds.push(Ui::Select as u32);
                            use_entry(w, index, id);
                        }
                    }
                    _ => {}
                },
                Sub::Drop { index, id, yes } => match key {
                    egui::Key::ArrowUp | egui::Key::ArrowDown => {
                        *sub = Some(Sub::Drop { index, id, yes: !yes });
                        w.sounds.push(Ui::Cursor as u32);
                    }
                    egui::Key::Enter | egui::Key::Escape => {
                        let yes = yes && key == egui::Key::Enter;
                        *sub = Some(Sub::Use { child: 2, index, id });
                        w.sounds.push(if yes { Ui::Select } else { Ui::Cancel } as u32);
                        if yes {
                            drop_entry(w, index, id);
                        }
                    }
                    _ => {}
                },
            }
        }
        // itmsort2 focused (FUN_101f36d0 / its messages): Up / Down over the three, Enter Auto (sortyn) / Manual / the
        // Recycle Bin (not done), Escape back to the list (SE 0013).
        Some(Win::Items { sort: sort @ Some(_), manual, .. }) => {
            use crate::audio::Ui;
            let mut s = sort.unwrap();
            match (s.ask, key) {
                (Some(yes), egui::Key::ArrowUp | egui::Key::ArrowDown) => {
                    s.ask = Some(!yes);
                    w.sounds.push(Ui::Cursor as u32);
                }
                (Some(yes), egui::Key::Enter | egui::Key::Escape) => {
                    let yes = yes && key == egui::Key::Enter;
                    s.ask = None;
                    w.sounds.push(if yes { Ui::Select } else { Ui::Cancel } as u32);
                    if yes {
                        *sort = None;
                        auto_sort(w);
                        return true;
                    }
                }
                (None, egui::Key::ArrowUp) => s.cursor = if s.cursor <= 1 { 3 } else { s.cursor - 1 },
                (None, egui::Key::ArrowDown) => s.cursor = if s.cursor >= 3 { 1 } else { s.cursor + 1 },
                (None, egui::Key::Enter) if s.cursor == 1 => {
                    s.ask = Some(false);
                    w.sounds.push(Ui::Select as u32);
                }
                (None, egui::Key::Enter) if s.cursor == 2 => {
                    *manual = Some(None);
                    *sort = None;
                    w.sounds.push(Ui::Select as u32);
                    return true;
                }
                // FUN_101f3a50 row 3: FUN_101daf70(0x11), itmsort2 hidden meanwhile.
                (None, egui::Key::Enter) => open_recycle = true,
                (None, egui::Key::Escape) => {
                    *sort = None;
                    w.sounds.push(Ui::Close as u32);
                    return true;
                }
                _ => {}
            }
            *sort = Some(s);
        }
        // Manual (FUN_101f0b30): Enter picks a row, Enter on a second one moves it there (the same row splits one off a
        // stack, a row of the same item takes the whole first stack: c2s 0x029); Escape drops the pick, else leaves
        // to itmsort2.
        Some(Win::Items { scroll, cursor, manual: manual @ Some(_), sort, .. }) => {
            use crate::audio::Ui;
            let held = manual.unwrap();
            match key {
                egui::Key::Enter => {
                    let at = *scroll + *cursor;
                    match (held, shown.get(at)) {
                        (None, Some(s)) => {
                            *manual = Some(Some(s.index));
                            w.sounds.push(Ui::Select as u32);
                        }
                        (Some(from), Some(to)) => {
                            let to = to.clone();
                            *manual = Some(None);
                            w.sounds.push(Ui::Select as u32);
                            manual_move(w, from, &to);
                            return true;
                        }
                        _ => {}
                    }
                }
                egui::Key::Escape if held.is_some() => *manual = Some(None),
                egui::Key::Escape => {
                    *manual = None;
                    *sort = Some(SortWin { cursor: 2, ask: None });
                }
                k => list_keys(scroll, cursor, rows, k),
            }
        }
        Some(Win::Items { scroll, cursor, sub, direct, .. }) => match key {
            egui::Key::Escape => close = true,
            // FUN_101f0a10: the direct list uses at once, SE 0002.
            egui::Key::Enter if *direct => {
                if let Some(s) = shown.get(*scroll + *cursor).cloned() {
                    w.sounds.push(crate::audio::Ui::Select as u32);
                    use_entry(w, s.index, s.id);
                }
            }
            // FUN_101f2bb0: iuse for the row (not for lock 0x19), SE 0002.
            egui::Key::Enter => {
                if let Some(s) = shown.get(*scroll + *cursor)
                    && w.inv.locks.get(&(INVENTORY, s.index)).copied().unwrap_or(0) != 0x19
                {
                    *sub = Some(Sub::Use { child: 1, index: s.index, id: s.id });
                    w.sounds.push(crate::audio::Ui::Select as u32);
                }
            }
            k => list_keys(scroll, cursor, rows, k),
        },
        Some(Win::Equip { row, col, pick }) => {
            let slot = slot_at(*row, *col);
            // FUN_101306f0: the list's messages 1..6 drop the compare.
            if let Some(p) = pick {
                p.compare = None;
            }
            match pick {
                Some(p) => match key {
                    egui::Key::Escape => *pick = None,
                    egui::Key::Enter => {
                        if let Some(&index) = p.indexes.get(p.scroll + p.cursor) {
                            send = Some(NetCommand::Equip { index, slot: slot as u8, container: INVENTORY });
                        }
                        *pick = None;
                    }
                    k => list_keys(&mut p.scroll, &mut p.cursor, p.indexes.len(), k),
                },
                None => match key {
                    egui::Key::Escape => close = true,
                    egui::Key::ArrowDown => *row = (*row + 1) % 4,
                    egui::Key::ArrowUp => *row = (*row + 3) % 4,
                    egui::Key::ArrowRight => *col = (*col + 1) % 4,
                    egui::Key::ArrowLeft => *col = (*col + 3) % 4,
                    // Inferred binding: Backspace / Delete removes the piece in the slot (0x050 index 0).
                    egui::Key::Backspace | egui::Key::Delete => {
                        if let Some((c, _)) = w.inv.equip[slot] {
                            send = Some(NetCommand::Equip { index: 0, slot: slot as u8, container: c });
                        }
                    }
                    egui::Key::Enter => {
                        let indexes = candidates_world(&w.inv, w.stats.as_ref(), w.my_race, &info, slot);
                        *pick = Some(Pick { indexes, scroll: 0, cursor: 0, compare: None, alt: 0, ctrl: None });
                    }
                    _ => {}
                },
            }
        }
    }
    if close {
        w.win = None;
    }
    if open_recycle {
        open_bin(w, true);
    }
    if let Some(c) = send {
        log::info!("equip: {c:?}");
        let _ = w.net.commands.send(c);
    }
    true
}

/// FUN_10130a80 / FUN_1013ac50: in the candidate list a Ctrl press (key function 0x76, pad 0x89) opens itemxinf with
/// the piece worn in a slot the focused item fits, taking its slot bits in turn (rings and earrings alternate) and
/// moving the grid cursor there; the next press closes it.
pub fn compare_key(w: &mut World, held: bool) {
    let info = w.item_info.clone();
    let Some(Win::Equip { row, col, pick: Some(p) }) = &mut w.win else { return };
    let rising = held && p.ctrl.is_some_and(|c| !c);
    p.ctrl = Some(held);
    if !rising || p.compare.take().is_some() {
        return;
    }
    let Some(&index) = p.indexes.get(p.scroll + p.cursor) else { return };
    let Some(&(id, _)) = w.inv.slots.get(&(INVENTORY, index)) else { return };
    let bits = info.as_ref().and_then(|c| c.item(id)).map_or(0, |it| it.slots);
    // FUN_101317b0 takes the first or second set bit; two misses clear the turn (FUN_10130be0).
    for _ in 0..2 {
        let nth = (p.alt & 1) as usize;
        p.alt = (p.alt + 1) & 3;
        if let Some(slot) = (0..16).filter(|b| bits & (1 << b) != 0).nth(nth) {
            let child = SLOT_CHILD[slot] as usize - 1;
            (*col, *row) = (child / 4, child % 4);
            p.compare = Some(w.inv.equip[slot]);
            return;
        }
    }
    p.alt = 0;
}

/// The compared piece while itemxinf is open: Some(None) for an empty slot; (item, extdata, container).
pub fn compare(w: &World) -> Option<Option<(u16, Option<[u8; 24]>, u8)>> {
    let Some(Win::Equip { pick: Some(p), .. }) = &w.win else { return None };
    let at = (*p.compare.as_ref()?).and_then(|(c, i)| Some((w.inv.slots.get(&(c, i))?.0, w.inv.ext.get(&(c, i)).copied(), c)));
    Some(at)
}

/// Inventory items that fit `slot` (item +0x10 slot bits), and the character's level, job and race when known.
fn candidates_world(inv: &Inventory, st: Option<&CharStats>, race: u8, info: &ItemCache, slot: usize) -> Vec<u8> {
    inv.slots_of(INVENTORY)
        .into_iter()
        .filter(|s| s.index != 0)
        .filter(|s| {
            let Some(it) = info.item(s.id) else { return false };
            let fits = it.slots & (1 << slot) != 0;
            let level = st.is_none_or(|st| it.level <= st.main.1 as u16);
            let job = st.is_none_or(|st| it.jobs & (1 << st.main.0) != 0);
            let ok_race = race == 0 || it.races & (1 << race) != 0;
            fits && level && job && ok_race
        })
        .map(|s| s.index)
        .collect()
}

pub(super) fn upload_icons(gu: &mut GameUi, info: &ItemCache, ids: impl Iterator<Item = u16>) {
    for id in ids {
        let name = icon_texture(id);
        if !gu.has_image(&name)
            && let Some(rgba) = info.icon(id)
        {
            gu.image(&name, 32, 32, rgba);
        }
    }
}

/// sortyn (FUN_10221740 / its open hook): help slot 1 group 0xc 0x3e "Dispose of item?", title slot 2 "Items".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    match &w.win {
        // The bin (group 0xd 0xc5 "Recycling Bin", rows 0xe 0x42d), the inventory side ("Items", rows without help),
        // itemctrl (0xe 0x10c, 0xd 0x3e).
        Some(Win::Items { bin: Some(b), .. }) => {
            let (help, title) = match (b.qty, b.on_bin) {
                (Some(_), _) => (gu.menu_msg(0x10c, ""), 0x3e),
                (None, true) => (gu.menu_msg(0x42d, ""), 0xc5),
                (None, false) => (String::new(), 2),
            };
            bars.help[1] = help;
            bars.title[1] = gu.title_msg(title).unwrap_or_default();
        }
        Some(Win::Items { sub: Some(Sub::Drop { .. }), .. }) => {
            bars.help[1] = gu.lobby_msg(0x0c, 0x3e, "");
            bars.title[2] = gu.title_msg(2).unwrap_or_default();
        }
        // Auto's sortyn: group 0xc 0x3d "Sort items?".
        Some(Win::Items { sort: Some(SortWin { ask: Some(_), .. }), .. }) => {
            bars.help[1] = gu.lobby_msg(0x0c, 0x3d, "");
            bars.title[2] = gu.title_msg(2).unwrap_or_default();
        }
        // Manual (FUN_101f0690 / FUN_101f0b30): group 0xe 5 "Select item to move.", 6 with one held.
        Some(Win::Items { manual: Some(held), sort: None, .. }) => {
            bars.help[2] = gu.menu_msg(if held.is_some() { 6 } else { 5 }, "");
        }
        _ => {}
    }
}

/// The item info window's item from the open list (FUN_101f27a0 / FUN_101321d0): the slot under the cursor, mode 0
/// for the inventory, mode 1 for the equipment grid or its candidates; (item, extdata, container, index, equipped,
/// mode).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>, u8, u8, bool, u8)> {
    let slots = w.inv.slots_of(INVENTORY);
    let (slot, container, mode) = match w.win.as_ref()? {
        // FUN_101f2740: the list and iteminfo are hidden while the item's target prompt is up.
        Win::Items { .. } if item_prompt(w) => return None,
        // FUN_101dca80: the bin side shows its cursor row; mode 1 (130, 240) clears itemctrl.
        Win::Items { bin: Some(b), .. } if b.on_bin => (bin_rows(w).get(b.scroll + b.cursor)?.clone(), BIN, if b.qty.is_some() { 1 } else { 0 }),
        Win::Items { scroll, cursor, direct, .. } => (listed(w, *direct).get(scroll + cursor)?.clone(), INVENTORY, 0),
        Win::Equip { pick: Some(p), .. } => {
            let i = *p.indexes.get(p.scroll + p.cursor)?;
            (slots.iter().find(|s| s.index == i)?.clone(), INVENTORY, 1)
        }
        Win::Equip { row, col, pick: None } => {
            let (c, i) = w.inv.equip[slot_at(*row, *col)]?;
            let &(id, count) = w.inv.slots.get(&(c, i))?;
            (Slot { index: i, id, count, equipped: true, equipped_ex: false }, c, 1)
        }
    };
    Some((slot.id, w.inv.ext.get(&(container, slot.index)).copied(), container, slot.index, slot.equipped, mode))
}

/// The item's target prompt is up (SubThen::Item).
fn item_prompt(w: &World) -> bool {
    matches!(w.prompt.as_ref().map(|p| &p.then), Some(super::target::SubThen::Item { .. }))
}

/// FUN_101f2de0 / FUN_101f2ec0: [Use, Drop] enabled. Use: crystals and maze tabulae, non-equipment with record flag
/// 0x400, equipped charged gear whose extdata (kind 1) has u16 @2 bit 0x4000; never as a monster (main job 0x17).
/// Drop: the slot's lock byte is 0.
fn iuse_states(w: &World, index: u8, id: u16) -> [bool; 2] {
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return [false, false] };
    let ext = w.inv.ext.get(&(INVENTORY, index));
    let equipped = w.inv.equip.contains(&Some((INVENTORY, index)));
    let usable = matches!(it.item_type, 8 | 0x16 | 0x17)
        || (!matches!(it.item_type, 4 | 5) && it.flags & 0x400 != 0)
        || (matches!(it.item_type, 4 | 5) && it.max_charges != 0 && equipped && ext.is_some_and(|e| e[0] == 1 && u16::from_le_bytes([e[2], e[3]]) & 0x4000 != 0));
    let monster = w.stats.as_ref().is_some_and(|s| s.main.0 == 0x17);
    [usable && !monster, w.inv.locks.get(&(INVENTORY, index)).copied().unwrap_or(0) == 0 && id != 0xffff]
}

/// iuse and sortyn: the frame, children, caption; disabled children with their dim (kind 4) sprite.
fn draw_iuse(gu: &mut GameUi, w: &World, s: Sub, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let (child, index, id, ask) = match s {
        Sub::Use { child, index, id } => (Some(child), index, id, None),
        Sub::Drop { index, id, yes } => (None, index, id, Some(yes)),
    };
    let on = iuse_states(w, index, id);
    let off: Vec<u16> = (1..=2).filter(|&c| !on[c as usize - 1]).collect();
    let mut rects = Vec::new();
    if let Some(win) = gu.lobby_window_off("menu    iuse", sw, sh, (0.0, 0.0), child, child.is_some(), &off) {
        let [x, y, ..] = win.frame;
        if let Some(m) = gu.layout("menu    iuse").cloned() {
            for e in m.elements.iter().filter(|e| off.contains(&e.id)) {
                if let Some(r) = e.refs.iter().find(|r| r.kind == 4) {
                    gu.sprite(&r.lib, r.index as usize, x + e.x as f32, y + e.y as f32, 0x8080_8080);
                }
            }
        }
        rects.push(win.frame);
    }
    if let Some(yes) = ask
        && let Some(win) = gu.lobby_window_off("menu    sortyn", sw, sh, (0.0, 0.0), Some(if yes { 1 } else { 2 }), true, &[])
    {
        rects.push(win.frame);
    }
    rects
}

/// FUN_101f2ec0: iuse closes when the list cursor no longer rests on the slot it opened for.
pub fn check_iuse(w: &mut World) {
    let Some(Win::Items { scroll, cursor, sub: Some(s), direct, .. }) = &w.win else { return };
    let (index, id) = match *s {
        Sub::Use { index, id, .. } | Sub::Drop { index, id, .. } => (index, id),
    };
    let here = listed(w, *direct).get(scroll + cursor).cloned();
    if here.is_none_or(|s| s.index != index || s.id != id)
        && let Some(Win::Items { sub, .. }) = &mut w.win
    {
        *sub = None;
    }
}

/// iuse Use (FUN_101f33a0 case 1): tabulae and crystals open windows we do not have yet; anything else passes the
/// status gate (0x7f; the Mog Garden's 0x77 / flag 0x08 rule not done) and opens the target prompt with the item's
/// targets (0x7f with no record), title "Items", help 0xe 4.
fn use_entry(w: &mut World, index: u8, id: u16) {
    let it = w.item_info.as_ref().and_then(|c| c.item(id));
    match it.as_ref().map(|i| i.item_type) {
        Some(0x16 | 0x17) => log::info!("iuse: the maze tabula window is not implemented"),
        Some(8) => {
            if super::textcmd::status_gate(w, 0x77) {
                log::info!("iuse: synthesis (tskill1) is not implemented");
            }
        }
        _ => {
            if super::textcmd::status_gate(w, 0x7f) {
                let targets = it.map_or(0x7f, |i| i.targets as u32);
                super::target::open_sub(w, targets, super::target::SubThen::Item { container: INVENTORY, index }, None, String::new(), 4);
            }
        }
    }
}

/// FUN_1021c7e0 (sort type config 0xca): 0 sorts the view by the record's key (+0xc, our `resource`) then id, 1 pulls
/// identical ids next to their first; then c2s 0x03A {container} merges the stacks.
fn auto_sort(w: &mut World) {
    let grouped = w.game.get(0xca) == 1;
    let info = w.item_info.clone();
    let key = |s: &Slot| (info.as_ref().and_then(|c| c.item(s.id)).map_or(0xffff, |it| it.resource), s.id);
    let mut v = w.inv.viewed(INVENTORY).into_iter().filter(|s| s.index != 0).collect::<Vec<_>>();
    if grouped {
        let mut out: Vec<Slot> = Vec::new();
        for s in v {
            match out.iter().rposition(|o| o.id == s.id) {
                Some(p) => out.insert(p + 1, s),
                None => out.push(s),
            }
        }
        v = out;
    } else {
        v.sort_by_key(key);
    }
    let gil = w.inv.order.get(&INVENTORY).is_some_and(|o| o.contains(&0));
    let order: Vec<u8> = gil.then_some(0).into_iter().chain(v.iter().map(|s| s.index)).collect();
    w.inv.order.insert(INVENTORY, order);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x03A, body: 0u32.to_le_bytes().to_vec() });
}

/// FUN_101f0b30's second pick: the same row splits one off (to index 0x52, inf: the server's first free slot) when
/// the stack holds more than one and stacks; another row of the same item takes the whole first stack; then the view
/// entry moves (FUN_1021cac0, client only).
fn manual_move(w: &mut World, from: u8, to: &Slot) {
    let Some(&(id, count)) = w.inv.slots.get(&(INVENTORY, from)) else { return };
    let stack = w.item_info.as_ref().and_then(|c| c.item(id)).map_or(1, |it| it.stack);
    let mv = |w: &World, n: u32, i1: u8, i2: u8| {
        let mut b = vec![0u8; 8];
        b[0..4].copy_from_slice(&n.to_le_bytes());
        (b[4], b[5], b[6], b[7]) = (INVENTORY, INVENTORY, i1, i2);
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x029, body: b });
    };
    if to.index == from {
        if count > 1 && stack > 1 {
            mv(w, 1, from, 0x52);
        }
        return;
    }
    if to.id == id {
        mv(w, count, from, to.index);
    }
    if let Some(o) = w.inv.order.get_mut(&INVENTORY)
        && let (Some(a), Some(b)) = (o.iter().position(|&i| i == from), o.iter().position(|&i| i == to.index))
    {
        let x = o.remove(a);
        o.insert(b, x);
    }
}

/// FUN_101f0260: the use sender (iuse and /item). No charges and a non-monster NPC: a trade (c2s 0x036, inventory
/// only, within 6 yalms else message std 9). Charged gear must be equipped and ready. Then the cast bar (use time
/// x 20 ticks) and c2s 0x037.
pub fn use_slot(w: &mut World, container: u8, index: u8, target: u32) {
    let Some(&(id, _)) = w.inv.slots.get(&(container, index)) else { return };
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return };
    let tindex = if target == w.my_id { w.my_index } else { w.entities.get(&target).map_or(0, |e| e.index) };
    let send = |w: &World, id: u16, body: Vec<u8>| {
        let _ = w.net.commands.send(NetCommand::Packet { id, body });
    };
    if it.max_charges == 0 {
        if let Some(e) = w.entities.get(&target).filter(|e| target >> 24 == 1 && !(e.mob || e.flags & 1 != 0)) {
            if (e.pos - w.pos).length_squared() > 36.0 {
                super::textcmd::system_line(w, 9);
                return;
            }
            if container != 0 {
                return;
            }
            let mut b = vec![0u8; 0x3c];
            b[0..4].copy_from_slice(&target.to_le_bytes());
            b[4..8].copy_from_slice(&1u32.to_le_bytes());
            b[0x2c] = index;
            b[0x36..0x38].copy_from_slice(&tindex.to_le_bytes());
            b[0x38] = 1;
            send(w, 0x036, b);
            return;
        }
    } else {
        let ext = w.inv.ext.get(&(container, index));
        let ready = w.inv.equip.contains(&Some((container, index))) && ext.is_some_and(|e| e[0] == 1 && u16::from_le_bytes([e[2], e[3]]) & 0x4000 != 0);
        if !ready {
            return;
        }
    }
    let use_time = if matches!(it.item_type, 4 | 5) { it.cast_time as u32 } else { it.extra.iter().find(|e| e.0 == 0xd08).map_or(0, |e| e.1) };
    if use_time > 0 {
        let total = use_time as f32 * 20.0;
        w.cast_bar = Some((total, total));
    }
    let mut b = vec![0u8; 16];
    b[0..4].copy_from_slice(&target.to_le_bytes());
    b[8..10].copy_from_slice(&tindex.to_le_bytes());
    b[10] = index;
    b[12..16].copy_from_slice(&(container as u32).to_le_bytes());
    send(w, 0x037, b);
}

/// LAB_101f30c0: Ex / Rare (flags 0xc000), augmented or the Linkshell ask once more (comyn); others go at once.
fn drop_entry(w: &mut World, index: u8, id: u16) {
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return };
    let aug = w.inv.ext.get(&(INVENTORY, index)).is_some_and(|e| matches!(e[0], 2 | 3));
    if it.flags & 0xc000 != 0 || aug || id == 0x201 {
        w.confirm = Some((super::Confirm::Drop { index, id, stage: 0 }, false));
    } else {
        drop_now(w, index);
    }
}

/// FUN_101f2ff0: c2s 0x028 {count = the whole stack, container, index}; iuse closes.
pub fn drop_now(w: &mut World, index: u8) {
    let Some(&(_, count)) = w.inv.slots.get(&(INVENTORY, index)) else { return };
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&count.to_le_bytes());
    (b[4], b[5]) = (INVENTORY, index);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x028, body: b });
    if let Some(Win::Items { sub, .. }) = &mut w.win {
        *sub = None;
    }
}

/// Draws the open window. Returns the rects that take clicks.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(win), Some(info)) = (&w.win, w.item_info.as_ref()) else { return rects };
    let frame = gu.cursor_frame();
    let slots = w.inv.slots_of(INVENTORY);
    match win {
        Win::Items { .. } if item_prompt(w) => {}
        Win::Items { scroll, cursor, bin: Some(b), .. } => rects.extend(draw_bin(gu, w, info, *scroll, *cursor, b, sw, sh, frame)),
        Win::Items { scroll, cursor, sub, direct, sort, manual, .. } => {
            let shown = listed(w, *direct);
            let mut rows = build_rows(&shown, info.as_ref(), false);
            // FUN_101f0810: the direct list's colours, yellow for usable items, equipped charged gear blue when ready
            // (extdata kind 1, u16 @2 bit 0x4000), else grey.
            if *direct {
                for (r, s) in rows.iter_mut().zip(&shown) {
                    let gear = info.item(s.id).is_some_and(|it| matches!(it.item_type, 4 | 5));
                    let ready = w.inv.ext.get(&(INVENTORY, s.index)).is_some_and(|e| e[0] == 1 && u16::from_le_bytes([e[2], e[3]]) & 0x4000 != 0);
                    r.tint = match (gear, ready) {
                        (false, _) => crate::inventory::TINT_FLAGGED,
                        (true, true) => 0x8040_7090,
                        (true, false) => crate::inventory::TINT_DIM,
                    };
                }
            }
            let list = ItemList { rows, scroll: *scroll, cursor: *cursor };
            upload_icons(gu, info, list.visible().iter().map(|r| r.id));
            let (b, ui, style, font) = gu.parts();
            if let Some(m) = ui.menu("menu    inventor") {
                push_inventory(b, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, frame);
            }
            rects.extend(gu.window_rect("menu    inventor", sw, sh));
            // itmsort2 beside the list (FUN_101f0600), focused or not; iuse sits on top of it.
            if !*direct && sub.is_none() {
                let sel = sort.map(|s| s.cursor);
                rects.extend(gu.lobby_window_off("menu    itmsort2", sw, sh, (0.0, 0.0), sel, sel.is_some(), &[]).map(|w| w.frame));
                if let Some(yes) = sort.and_then(|s| s.ask) {
                    rects.extend(gu.lobby_window_off("menu    sortyn", sw, sh, (0.0, 0.0), Some(if yes { 1 } else { 2 }), true, &[]).map(|w| w.frame));
                }
            }
            let _ = manual;
            if let Some(s) = sub {
                rects.extend(draw_iuse(gu, w, *s, sw, sh));
            }
        }
        Win::Equip { row, col, pick } => {
            let slot = slot_at(*row, *col);
            let mut eq = Equipment::default();
            for (k, e) in w.inv.equip.iter().enumerate() {
                if let Some((c, i)) = e
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
            upload_icons(gu, info, eq.slots.iter().flatten().map(|s| s.id));
            let persona = persona(w);
            let gil = gil_text(w.inv.gil() as i64);
            let rows: Vec<Slot> = match pick {
                Some(p) => p.indexes.iter().filter_map(|i| slots.iter().find(|s| s.index == *i).cloned()).collect(),
                None => Vec::new(),
            };
            let list = pick.as_ref().map(|p| ItemList { rows: build_rows(&rows, info.as_ref(), false), scroll: p.scroll, cursor: p.cursor });
            if let Some(l) = &list {
                upload_icons(gu, info, l.visible().iter().map(|r| r.id));
            }
            let (b, ui, style, font) = gu.parts();
            if let Some(m) = ui.menu("menu    persona") {
                push_persona(b, ui, style, m, &persona, sw, sh);
            }
            if let Some(m) = ui.menu("menu    money") {
                push_money(b, ui, style, m, &gil, sw, sh);
            }
            if let (Some(m), Some(l)) = (ui.menu("menu    inventor"), &list) {
                // Inferred: the equippable source puts the list at (314, 48), right of equip (vfunc +4).
                let mut m = m.clone();
                if let Some(e) = m.elements.first_mut() {
                    e.x = LIST_X;
                }
                push_inventory(b, ui, style, font, GameUi::ATLAS, &m, l, sw, sh, frame);
            }
            if let Some(m) = ui.menu("menu    equip") {
                let cursor = pick.is_none().then_some(SLOT_CHILD[slot] as usize);
                push_equipment(b, ui, style, m, &eq, info.as_ref(), sw, sh, cursor, frame);
            }
            for n in ["menu    persona", "menu    money", "menu    equip"] {
                rects.extend(gu.window_rect(n, sw, sh));
            }
            if list.is_some()
                && let Some(mut r) = gu.window_rect("menu    inventor", sw, sh)
            {
                r[0] += LIST_X as f32 - gu.window_rect_x("menu    inventor");
                rects.push(r);
            }
        }
    }
    rects
}

/// FUN_101daf70(0x11): status gate 0x44 (synthesis; the costume bit is not tracked here), then the bin beside the
/// inventory, from itmsort2 (`from_menu`) or `/recycle`. No zone or Mog House rule.
pub fn open_bin(w: &mut World, from_menu: bool) -> bool {
    if !super::textcmd::status_gate(w, 0x44) {
        return false;
    }
    let b = Bin { scroll: 0, cursor: 0, on_bin: true, qty: None, from_menu };
    match &mut w.win {
        Some(Win::Items { bin, sort, direct: false, .. }) if from_menu => {
            *bin = Some(b);
            *sort = None;
        }
        _ => {
            w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: false, sort: None, manual: None, bin: Some(b) });
            w.menu = None;
            w.action_menu = None;
        }
    }
    true
}

/// The bin's rows: container 17 in view order.
fn bin_rows(w: &World) -> Vec<Slot> {
    w.inv.viewed(BIN).into_iter().filter(|s| s.index != 0 && s.id != 0).collect()
}

/// FUN_101db220 for a bin row: grey with no free inventory slot, or a Rare item already held (but in containers 3 /
/// 17); orange for the row whose quantity is being set; else white.
fn bin_tint(w: &World, info: &ItemCache, s: &Slot, picked: Option<u8>) -> u32 {
    let free = (w.inv.sizes[0] as usize).saturating_sub(1) > w.inv.slots_of(INVENTORY).iter().filter(|x| x.index != 0).count();
    let rare = info.item(s.id).is_some_and(|it| it.flags & 0x8000 != 0) && w.inv.slots.iter().any(|(&(c, _), &(id, _))| id == s.id && c != 3 && c != BIN);
    if !free || rare {
        crate::inventory::TINT_DIM
    } else if picked == Some(s.index) {
        0x8080_4020
    } else {
        crate::inventory::TINT_WHITE
    }
}

/// FUN_100eb710 from the bin: c2s 0x029 {n, 17, 0, slot, 0x52} (the inventory's first free slot).
fn send_bin(w: &World, index: u8, n: u32) {
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&n.to_le_bytes());
    (b[4], b[5], b[6], b[7]) = (BIN, INVENTORY, index, 0x52);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x029, body: b });
}

/// The bin view's keys (FUN_101db690, FUN_101dcb80, itemctrl FUN_1013a6c0): Right on the bin / Left on the inventory
/// moves the focus (SE 0013); Enter on a bin row sends one item at once or opens itemctrl for a stack (SE 0002), on a
/// grey row or the inventory side SE 0004; Escape leaves (to itmsort2 when it came from there). itemctrl: Up / Down
/// +-1 (the step of 10 after a long hold is not done), Right 1, Left the count, Enter sends, Escape drops it.
fn bin_key(w: &mut World, info: &ItemCache, key: egui::Key) {
    use crate::audio::Ui;
    let Some(Win::Items { bin: Some(b), scroll, cursor, .. }) = &w.win else { return };
    let (mut b, mut scroll, mut cursor) = (*b, *scroll, *cursor);
    let bins = bin_rows(w);
    let inv = listed(w, false).len();
    let (mut close, mut back) = (false, false);
    if let Some(mut q) = b.qty {
        match key {
            egui::Key::ArrowUp => q.value = (q.value + 1).min(q.max),
            egui::Key::ArrowDown => q.value = q.value.saturating_sub(1).max(1),
            egui::Key::ArrowRight => q.value = 1,
            egui::Key::ArrowLeft => q.value = q.max,
            _ => {}
        }
        b.qty = Some(q);
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowRight | egui::Key::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
            egui::Key::Enter => {
                send_bin(w, q.index, q.value);
                b.qty = None;
                w.sounds.push(Ui::Select as u32);
            }
            egui::Key::Escape => {
                b.qty = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    } else {
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown if b.on_bin => list_keys(&mut b.scroll, &mut b.cursor, bins.len(), key),
            egui::Key::ArrowUp | egui::Key::ArrowDown => list_keys(&mut scroll, &mut cursor, inv, key),
            egui::Key::ArrowRight if b.on_bin => {
                b.on_bin = false;
                w.sounds.push(Ui::Close as u32);
            }
            egui::Key::ArrowLeft if !b.on_bin => {
                b.on_bin = true;
                w.sounds.push(Ui::Close as u32);
            }
            egui::Key::Enter if b.on_bin => {
                if let Some(s) = bins.get(b.scroll + b.cursor) {
                    if bin_tint(w, info, s, None) == crate::inventory::TINT_DIM {
                        w.sounds.push(Ui::Error as u32);
                    } else {
                        w.sounds.push(Ui::Select as u32);
                        if s.count > 1 {
                            b.qty = Some(Qty { index: s.index, value: 1, max: s.count });
                        } else {
                            send_bin(w, s.index, 1);
                        }
                    }
                }
            }
            egui::Key::Enter => w.sounds.push(Ui::Error as u32),
            // FUN_101dca30 / FUN_101f2640 (no SE in that code: the cancel sound is inferred).
            egui::Key::Escape => {
                (close, back) = (!b.from_menu, b.from_menu);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    }
    if close {
        w.win = None;
        return;
    }
    if let Some(Win::Items { bin, scroll: s, cursor: c, sort, .. }) = &mut w.win {
        (*s, *c) = (scroll, cursor);
        if back {
            *bin = None;
            *sort = Some(SortWin { cursor: 3, ask: None });
        } else {
            *bin = Some(b);
        }
    }
}

/// The bin view: "menu    bank" with the bin rows at its layout (16, 48), the inventory list moved to (200, 48) with
/// every row grey, the cursor on the focused one; "menu    itemctrl" over them for a stack ("%2d" right-aligned at
/// (+0x34, +0x16), "/%2d" at (+0x3a, +0x16), FUN_1013a520).
#[allow(clippy::too_many_arguments)]
fn draw_bin(gu: &mut GameUi, w: &World, info: &ItemCache, scroll: usize, cursor: usize, b: &Bin, sw: f32, sh: f32, frame: usize) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let bins = bin_rows(w);
    let mut rows = build_rows(&bins, info, false);
    for (r, s) in rows.iter_mut().zip(&bins) {
        r.tint = bin_tint(w, info, s, b.qty.map(|q| q.index));
    }
    let blist = ItemList { rows, scroll: b.scroll, cursor: b.cursor };
    let ilist = ItemList { rows: build_rows(&listed(w, false), info, true), scroll, cursor };
    upload_icons(gu, info, blist.visible().iter().chain(ilist.visible()).map(|r| r.id));
    let (bt, ui, style, font) = gu.parts();
    if let Some(m) = ui.menu("menu    bank") {
        push_item_list(bt, ui, style, font, GameUi::ATLAS, m, &blist, sw, sh, frame, 4.0, b.on_bin && b.qty.is_none());
    }
    if let Some(m) = ui.menu("menu    inventor") {
        let mut m = m.clone();
        if let Some(e) = m.elements.first_mut() {
            e.x = BIN_LIST_X;
        }
        push_item_list(bt, ui, style, font, GameUi::ATLAS, &m, &ilist, sw, sh, frame, 3.0, !b.on_bin);
    }
    rects.extend(gu.window_rect("menu    bank", sw, sh));
    if let Some(mut r) = gu.window_rect("menu    inventor", sw, sh) {
        r[0] += BIN_LIST_X as f32 - gu.window_rect_x("menu    inventor");
        rects.push(r);
    }
    if let Some(q) = b.qty
        && let Some(win) = gu.lobby_window_off("menu    itemctrl", sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        rects.push(win.frame);
        let (bt, ui, _, _) = gu.parts();
        if let Some(m) = ui.menu("menu    itemctrl") {
            let (x, y) = crate::render::screens::window_pos(m, sw, sh, 1.0, 1.0);
            crate::render::screens::push_shape_text_right(bt, ui, format!("{:2}", q.value).as_bytes(), x + 52.0, y + 22.0, 0x8080_8080);
            crate::render::screens::push_shape_text(bt, ui, format!("/{:2}", q.max).as_bytes(), x + 58.0, y + 22.0, 0x8080_8080);
        }
    }
    rects
}

/// Mouse on the item list: hover moves the cursor, a click on the highlighted row acts as Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    // The bin view moves the list; its mouse handling (hover messages 3 / 4) is not done.
    if matches!(w.win, Some(Win::Items { bin: Some(_), .. })) {
        return;
    }
    let Some(r) = gu.window_rect("menu    inventor", sw, sh) else { return };
    let row = ((p.1 - r[1] - 4.0) / ROW_PITCH).floor();
    let inside = p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3] && row >= 0.0 && (row as usize) < VISIBLE_ROWS;
    if inside && matches!(w.win, Some(Win::Items { .. }) | Some(Win::Equip { pick: Some(_), .. })) {
        gu.hover.set(true);
    }
    if !inside || !(moved || clicked) {
        return;
    }
    let row = row as usize;
    let (cur, enter) = match &mut w.win {
        Some(Win::Items { cursor, .. }) => (cursor, true),
        Some(Win::Equip { pick: Some(p), .. }) => (&mut p.cursor, true),
        _ => return,
    };
    let same = *cur == row;
    *cur = row;
    if clicked && same && enter {
        key(w, egui::Key::Enter);
    }
}

/// The status panel from 0x061 and 0x0DF.
fn persona(w: &World) -> Persona {
    let st = w.stats.clone().unwrap_or_default();
    let (hp, mp, tp) = w.vitals.map_or((0, 0, 0), |v| (v.0, v.1, v.2));
    let job = |(j, lv): (u8, u8)| (j != 0).then(|| (lv, JOBS.get(j as usize).copied().unwrap_or("").to_string()));
    Persona::build(&Stats {
        name: w.name.clone(),
        main: job(st.main),
        sub: job(st.sub),
        ilvl: (st.ilvl != 0).then_some(st.ilvl as u16),
        su: st.su,
        hp: (hp, st.hp_max),
        mp: (mp, st.mp_max),
        tp,
        base: st.base,
        bonus: st.adj,
    })
}
