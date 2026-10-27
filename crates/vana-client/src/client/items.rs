//! Items and Equipment in the live client: containers from s2c 0x01C / 0x01E / 0x01F / 0x020 / 0x050,
//! the "inventor", "equip", "persona" and "money" windows (inventory.rs models, render/invwin.rs
//! drawing); equipping sends c2s 0x050 (FUN_100e25d0: index, slot, container; index 0 removes).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use super::scrollbar::{Msg, Owner, rows};
use crate::audio::Ui;
use crate::dat::items::Item;
use crate::inventory::{ItemInfo, ItemList, Persona, ROW_PITCH, Slot, Stats, VISIBLE_ROWS, build_rows};
use crate::render::invwin::{icon_texture, push_inventory, push_item_list};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Main inventory (LSB LOC_INVENTORY).
pub const INVENTORY: u8 = 0;
/// The recycle bin's container.
const BIN: u8 = 17;
/// The inventory list's x beside the bin (the partner source's position kind 1, FUN_101f1fe0).
const BIN_LIST_X: i16 = 200;

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
    /// u32 @0x58 (faction bits 0..4, rank, points; FUN_100994a0) and @0x60 (bit 0: in the Unity chat, FUN_100a3680).
    pub unity: u32,
    pub unity_flag: u32,
    /// u32 @0x5c: the Unity evaluation, this week (low u16) and the previous one (high u16).
    pub unity_eval: u32,
    /// EXP / to the next level (u16 @0x10 / @0x12), title @0x44, rank @0x46, rank points @0x48, home point zone @0x4a,
    /// nation @0x50; master level @0x65, its flags @0x66 (bit 0 unlocked), exemplar points @0x68 / next @0x6c.
    pub exp: (u16, u16),
    pub title: u16,
    pub rank: u16,
    pub rank_points: u16,
    pub home: u16,
    pub nation: u8,
    pub ml: (u8, u8),
    pub ml_exp: (u32, u32),
}

#[derive(Default)]
pub struct Inventory {
    /// 0x01C ItemNum: 1 + size per container.
    pub sizes: [u8; 18],
    /// 0x01C ItemNum2 (u16 @0x24): the Locker's is 0 without a lease (FUN_101cc470).
    pub sizes2: [u16; 18],
    /// (container, index) -> (item id, count).
    pub slots: BTreeMap<(u8, u8), (u16, u32)>,
    /// Equipment slot -> (container, index).
    pub equip: [Option<(u8, u8)>; 16],
    /// (container, index) -> LockFlg (s2c 0x01F / 0x020).
    pub locks: BTreeMap<(u8, u8), u8>,
    /// (container, index) -> the item's 24-byte extdata (s2c 0x020 Attr).
    pub ext: BTreeMap<(u8, u8), [u8; 24]>,
    /// (container, index) -> s2c 0x020 Price (our bazaar price with LockFlg 0x19).
    pub prices: BTreeMap<(u8, u8), u32>,
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
    objectives: RefCell<HashMap<u16, Option<Arc<crate::dat::roe::Objective>>>>,
    categories: RefCell<HashMap<u16, Option<Arc<crate::dat::roe::Category>>>>,
    points: std::cell::OnceCell<Arc<Vec<crate::dat::tpoints::Point>>>,
    catalogues: std::cell::OnceCell<Arc<Vec<(u16, u16)>>>,
}

impl ItemCache {
    pub fn new(names: Arc<crate::dat::names::Names>) -> Self {
        ItemCache { names, cache: RefCell::new(HashMap::new()), objectives: RefCell::default(), categories: RefCell::default(), points: Default::default(), catalogues: Default::default() }
    }

    pub fn item(&self, id: u16) -> Option<Arc<Item>> {
        self.cache.borrow_mut().entry(id).or_insert_with(|| self.names.items.item(id as u32).map(Arc::new)).clone()
    }

    /// A Records of Eminence objective record (0xe000 + n).
    pub fn objective(&self, id: u16) -> Option<Arc<crate::dat::roe::Objective>> {
        let items = &self.names.items;
        self.objectives.borrow_mut().entry(id).or_insert_with(|| crate::dat::roe::objective(items, id).map(Arc::new)).clone()
    }

    /// A Records of Eminence category record (0xf200..).
    pub fn category(&self, id: u16) -> Option<Arc<crate::dat::roe::Category>> {
        let items = &self.names.items;
        self.categories.borrow_mut().entry(id).or_insert_with(|| crate::dat::roe::category(items, id).map(Arc::new)).clone()
    }

    /// A Primer category (0xf620 + k): its name and topic ids.
    pub fn guide(&self, id: u16) -> Option<(String, Vec<u16>)> {
        let name = self.item(id)?.name.clone();
        Some((name, self.names.items.guide_pages(id as u32).into_iter().map(|p| p as u16).collect()))
    }

    /// The Monstrosity instinct catalogues 0xeff8..0xeffe in order: {item 0x7400 + n, cost}, read once.
    pub fn catalogues(&self) -> Arc<Vec<(u16, u16)>> {
        self.catalogues.get_or_init(|| Arc::new((0xeff8..0xefff).flat_map(|id| self.names.items.catalogue(id)).collect())).clone()
    }

    /// FUN_10141ed0: an instinct's cost (record +0x1c), 0xff without its record.
    pub fn instinct_cost(&self, n: u16) -> u16 {
        self.item(0x7400 + n).and_then(|i| i.extra.iter().find(|e| e.0 == 0xd2c).map(|e| e.1 as u16)).unwrap_or(0xff)
    }

    /// The teleport point table (records 0xf600..0xf613), read once.
    pub fn points(&self) -> Arc<Vec<crate::dat::tpoints::Point>> {
        self.points.get_or_init(|| Arc::new(crate::dat::tpoints::load(&self.names.items))).clone()
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
    Items { scroll: usize, cursor: usize, sub: Option<Sub>, direct: bool, sort: Option<SortWin>, manual: Option<Option<u8>>, store: Option<Storage> },
    /// The Equipment screen (client/equipwin.rs).
    Equip(Box<super::equipwin::EquipWin>),
}

/// "menu    itmsort2" with the focus (FUN_101f36b0 / FUN_101f3740): 1 Auto, 2 Manual, 3 Recycle Bin; `ask` is the
/// Auto prompt (sortyn "Sort items?", Yes / No).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SortWin {
    pub cursor: u16,
    pub ask: Option<bool>,
}

/// The storage view (FUN_101dc880 / FUN_101dc700): "menu    bank" on a container at (16, 48) beside the inventory list
/// at (200, 48), itmsortw at (384, 48) but for the recycle bin; or, `read_only`, the container alone in the item list
/// with every row grey (FUN_101dae10 away from the moogle). The inventory side keeps the list's scroll / cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Storage {
    /// The bank side's container (17 = the recycle bin) and the inventory side's (0 until switched).
    pub bank: u8,
    pub side: u8,
    pub scroll: usize,
    pub cursor: usize,
    /// Focus on the bank side (else the inventory side).
    pub on_bank: bool,
    /// "menu    itemctrl" for a stack.
    pub qty: Option<Qty>,
    /// The bin opened from itmsort2 (list +0x90): Escape returns there, else the view closes.
    pub from_menu: bool,
    /// Bank +0x70: at a moogle (Mog Safe, Storage, Locker and Safe 2 rows are grey without it); +0x71: a nomad moogle
    /// (no Storage).
    pub at_moogle: bool,
    pub nomad: bool,
    pub read_only: bool,
    /// itmsortw focused: its cursor, the side it serves, Auto's sortyn.
    pub sortw: Option<SortW>,
    /// Manual Sort (FUN_101dbed0) on a side (true: the bank), with the held slot.
    pub manual: Option<(bool, Option<u8>)>,
    /// "menu    itmstora" (Switch) with its cursor row, for the side itmsortw serves.
    pub stora: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SortW {
    pub cursor: u16,
    pub bank_side: bool,
    pub ask: Option<bool>,
}

/// The quantity window's pick: the slot, the value 1..max, the move's containers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qty {
    pub index: u8,
    pub value: u32,
    pub max: u32,
    pub from: u8,
    pub to: u8,
}

/// The windows over the list: "menu    iuse" (FUN_101f2bb0: 1 Use, 2 Drop) for the slot it opened on, and
/// "menu    sortyn" for Drop (FUN_10222770: Yes / No, default No; iuse stays under it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sub {
    Use { child: u16, index: u8, id: u16 },
    Drop { index: u8, id: u16, yes: bool },
}

pub fn open_items(w: &mut World) {
    w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: false, sort: None, manual: None, store: None });
    w.menu = None;
    w.action_menu = None;
}

/// The action menu's Items: only usable items, no iuse or sort window.
pub fn open_direct(w: &mut World) {
    w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: true, sort: None, manual: None, store: None });
    w.menu = None;
    w.action_menu = None;
}

/// The modern layer draws this window itself: the item list, the storage view, the Equipment screen.
pub fn modern_list(w: &World) -> bool {
    matches!(w.win, Some(Win::Items { .. } | Win::Equip(_)))
}

/// The list's rows: the occupied inventory slots but gil; for the direct list only those FUN_101f0810 colours
/// (crystals, maze tabulae, usable non-equipment, equipped charged gear).
pub(super) fn listed(w: &World, direct: bool) -> Vec<Slot> {
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
    super::equipwin::open(w);
}

/// The main menu's Equipment row: pupequip first for a PUP.
pub fn menu_equipment(w: &mut World) {
    super::equipwin::menu_open(w);
}

/// The focused window and its cursor child, for the bars' slot 1 (FUN_10115380). The equipment's candidate
/// list is taken as an inventor list (inferred).
pub fn focus(w: &World) -> Option<(&'static str, u16)> {
    match w.win.as_ref()? {
        Win::Items { sub: Some(Sub::Use { child, .. }), .. } => Some(("menu    iuse", *child)),
        Win::Items { sub: Some(Sub::Drop { yes, .. }), .. } => Some(("menu    sortyn", if *yes { 1 } else { 2 })),
        Win::Items { sort: Some(SortWin { ask: Some(yes), .. }), .. } => Some(("menu    sortyn", if *yes { 1 } else { 2 })),
        Win::Items { sort: Some(s), .. } => Some(("menu    itmsort2", s.cursor)),
        Win::Items { store: Some(b), cursor, .. } => Some(match (b.qty, b.sortw, b.on_bank) {
            (Some(_), ..) => ("menu    itemctrl", 1),
            _ if b.stora.is_some() => ("menu    itmstora", b.stora.unwrap_or(1)),
            (None, Some(sw), _) if sw.ask.is_some() => ("menu    sortyn", if sw.ask == Some(true) { 1 } else { 2 }),
            (None, Some(sw), _) => ("menu    itmsortw", sw.cursor),
            _ if b.read_only => ("menu    inventor", b.cursor as u16 + 1),
            (None, None, true) => ("menu    bank", b.cursor as u16 + 1),
            (None, None, false) => ("menu    inventor", *cursor as u16 + 1),
        }),
        Win::Items { cursor, .. } => Some(("menu    inventor", *cursor as u16 + 1)),
        Win::Equip(_) => super::equipwin::focus(w),
    }
}

/// Up / Down a row (wrapping); true when the cursor moved (the generic handler's SE 0001).
pub(super) fn list_keys(scroll: &mut usize, cursor: &mut usize, n: usize, key: egui::Key) -> bool {
    if n == 0 {
        return false;
    }
    let was = *scroll + *cursor;
    let mut at = was;
    match key {
        egui::Key::ArrowDown => at = (at + 1) % n,
        egui::Key::ArrowUp => at = (at + n - 1) % n,
        _ => return false,
    }
    if at < *scroll {
        *scroll = at;
    } else if at >= *scroll + VISIBLE_ROWS {
        *scroll = at + 1 - VISIBLE_ROWS;
    }
    *cursor = at - *scroll;
    at != was
}

/// FUN_101f6c70 messages 3 / 4: Right / Left turn a page of 10 keeping the cursor row; on the last / first page the
/// cursor goes to the last / first row instead. True when something moved (SE 0001).
pub(super) fn page_keys(scroll: &mut usize, cursor: &mut usize, n: usize, right: bool) -> bool {
    if n == 0 {
        return false;
    }
    let max = n.saturating_sub(VISIBLE_ROWS);
    let before = (*scroll, *cursor);
    match right {
        true if *scroll == max => *cursor = (n - *scroll).min(VISIBLE_ROWS) - 1,
        true => *scroll = (*scroll + VISIBLE_ROWS).min(max),
        false if *scroll == 0 => *cursor = 0,
        false => *scroll = scroll.saturating_sub(VISIBLE_ROWS),
    }
    *cursor = (*cursor).min(n - 1 - *scroll);
    (*scroll, *cursor) != before
}

/// A scroll bar or wheel message (FUN_101f6c70) to the focused item list. True when it moved.
pub(super) fn bar(w: &mut World, owner: Owner, m: Msg) -> bool {
    if owner == Owner::EquipPick {
        return super::equipwin::bar(w, m);
    }
    let direct = matches!(w.win, Some(Win::Items { direct: true, .. }));
    let n = match (&w.win, owner) {
        (Some(Win::Items { store: None, .. }), Owner::Items) => listed(w, direct).len(),
        (Some(Win::Items { store: Some(b), .. }), Owner::Bank) => store_rows(w, b.bank).len(),
        (Some(Win::Items { store: Some(b), .. }), Owner::StoreInv) => store_rows(w, b.side).len(),
        _ => return false,
    };
    match (&mut w.win, owner) {
        (Some(Win::Items { scroll, cursor, store: None, .. }), Owner::Items) | (Some(Win::Items { scroll, cursor, store: Some(_), .. }), Owner::StoreInv) => rows(m, scroll, cursor, n, VISIBLE_ROWS),
        (Some(Win::Items { store: Some(b), .. }), Owner::Bank) => rows(m, &mut b.scroll, &mut b.cursor, n, VISIBLE_ROWS),
        _ => false,
    }
}

/// Keys while a window is open; `page` is key function 0xa8 (Ctrl / Alt held). Returns true when the key was used.
pub fn key(w: &mut World, key: egui::Key, page: bool, alt: bool) -> bool {
    if matches!(w.win, Some(Win::Equip(_))) {
        super::equipwin::key(w, key);
        return true;
    }
    let Some(info) = w.item_info.clone() else {
        if key == egui::Key::Escape {
            w.win = None;
        }
        return w.win.is_some() || key == egui::Key::Escape;
    };
    let direct = matches!(w.win, Some(Win::Items { direct: true, .. }));
    let rows = listed(w, direct).len();
    let shown = listed(w, direct);
    if matches!(w.win, Some(Win::Items { store: Some(_), .. })) {
        // The modern storage view has its own keys.
        if !super::modern::storage_key(w, key) {
            store_key(w, &info, key, page, alt);
        }
        return true;
    }
    // Modern: the sort strip stands above the list as a row, so Up from the first item goes to it, Left / Right move
    // along it (and between Yes / No), Down comes back to the list.
    if w.modern
        && let Some(Win::Items { scroll, cursor, sub: None, sort, manual: None, store: None, direct: false }) = w.win.as_mut()
    {
        use crate::audio::Ui;
        use egui::Key as K;
        match (sort.as_mut(), key) {
            (None, K::ArrowUp) if *scroll + *cursor == 0 => {
                *sort = Some(SortWin { cursor: 1, ask: None });
                w.sounds.push(Ui::Cursor as u32);
                return true;
            }
            (Some(SortWin { ask: Some(yes), .. }), K::ArrowLeft | K::ArrowRight) => {
                *yes = !*yes;
                w.sounds.push(Ui::Cursor as u32);
                return true;
            }
            (Some(s @ SortWin { ask: None, .. }), K::ArrowLeft | K::ArrowRight) => {
                s.cursor = if key == K::ArrowRight { s.cursor % 3 + 1 } else if s.cursor <= 1 { 3 } else { s.cursor - 1 };
                w.sounds.push(Ui::Cursor as u32);
                return true;
            }
            (Some(SortWin { ask: None, .. }), K::ArrowDown) => {
                *sort = None;
                w.sounds.push(Ui::Cursor as u32);
                return true;
            }
            (Some(SortWin { ask: None, .. }), K::ArrowUp) => return true,
            _ => {}
        }
    }
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
                        keep_sorted(w, INVENTORY, true);
                        return true;
                    }
                }
                (None, egui::Key::ArrowUp) => {
                    s.cursor = if s.cursor <= 1 { 3 } else { s.cursor - 1 };
                    w.sounds.push(Ui::Cursor as u32);
                }
                (None, egui::Key::ArrowDown) => {
                    s.cursor = if s.cursor >= 3 { 1 } else { s.cursor + 1 };
                    w.sounds.push(Ui::Cursor as u32);
                }
                (None, egui::Key::Enter) if s.cursor == 1 => {
                    s.ask = Some(false);
                    w.sounds.push(Ui::Select as u32);
                }
                (None, egui::Key::Enter) if s.cursor == 2 => {
                    // keep_sorted off, by field (w.win is borrowed).
                    if w.auto_sort & 1 << INVENTORY != 0 {
                        (w.auto_sort, w.auto_sort_dirty) = (w.auto_sort & !(1 << INVENTORY), true);
                    }
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
                egui::Key::ArrowRight | egui::Key::ArrowLeft => {
                    if page_keys(scroll, cursor, rows, key == egui::Key::ArrowRight) {
                        w.sounds.push(Ui::Cursor as u32);
                    }
                }
                k => {
                if list_keys(scroll, cursor, rows, k) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            }
        }
        Some(Win::Items { scroll, cursor, sub, direct, .. }) => match key {
            egui::Key::Escape => {
                close = true;
                w.sounds.push(Ui::Cancel as u32);
            }
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
            // The main source's +0x1c (FUN_1012e470) is 1, so plain Right / Left page.
            egui::Key::ArrowRight | egui::Key::ArrowLeft => {
                if page_keys(scroll, cursor, rows, key == egui::Key::ArrowRight) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            k => {
                if list_keys(scroll, cursor, rows, k) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
        },
        Some(Win::Equip(_)) => {}
    }
    if close {
        w.win = None;
    }
    if open_recycle {
        open_bin(w, true);
    }
    true
}

pub fn compare_key(w: &mut World, held: bool) {
    super::equipwin::compare_key(w, held);
}

/// The compared piece while itemxinf is open (client/equipwin.rs).
pub fn compare(w: &World) -> Option<Option<(u16, Option<[u8; 24]>, u8)>> {
    super::equipwin::compare(w)
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
        Some(Win::Items { store: Some(b), .. }) => store_bars(gu, b, bars),
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
        Some(Win::Equip(_)) => super::equipwin::bars(gu, w, bars),
        _ => {}
    }
}

/// The item info window's item from the open list (FUN_101f27a0 / FUN_101321d0): the slot under the cursor, mode 0
/// for the inventory, mode 1 for the equipment grid or its candidates; (item, extdata, container, index, equipped,
/// mode).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>, u8, u8, bool, u8)> {
    let (slot, container, mode) = match w.win.as_ref()? {
        // FUN_101f2740: the list and iteminfo are hidden while the item's target prompt is up.
        Win::Items { .. } if item_prompt(w) => return None,
        // FUN_101dca80: the bin side shows its cursor row; mode 1 (130, 240) clears itemctrl.
        Win::Items { store: Some(b), .. } if b.on_bank || b.read_only => (store_rows(w, b.bank).get(b.scroll + b.cursor)?.clone(), b.bank, if b.qty.is_some() { 1 } else { 0 }),
        Win::Items { store: Some(b), scroll, cursor, .. } => (store_rows(w, b.side).get(scroll + cursor)?.clone(), b.side, if b.qty.is_some() { 1 } else { 0 }),
        Win::Items { scroll, cursor, direct, .. } => (listed(w, *direct).get(scroll + cursor)?.clone(), INVENTORY, 0),
        Win::Equip(_) => return super::equipwin::info_source(w),
    };
    Some((slot.id, w.inv.ext.get(&(container, slot.index)).copied(), container, slot.index, slot.equipped, mode))
}

/// The item's target prompt is up (SubThen::Item).
pub(super) fn item_prompt(w: &World) -> bool {
    matches!(w.prompt.as_ref().map(|p| &p.then), Some(super::target::SubThen::Item { .. }))
}

/// The list's rows in their colours; FUN_101f0810: the direct list's, yellow for usable items, equipped charged gear
/// blue when ready (extdata kind 1, u16 @2 bit 0x4000), else grey.
pub(super) fn list_rows(w: &World, info: &ItemCache, shown: &[Slot], direct: bool) -> Vec<crate::inventory::Row> {
    let mut rows = build_rows(shown, info, false);
    if direct {
        for (r, s) in rows.iter_mut().zip(shown) {
            let gear = info.item(s.id).is_some_and(|it| matches!(it.item_type, 4 | 5));
            let ready = w.inv.ext.get(&(INVENTORY, s.index)).is_some_and(|e| e[0] == 1 && u16::from_le_bytes([e[2], e[3]]) & 0x4000 != 0);
            r.tint = match (gear, ready) {
                (false, _) => crate::inventory::TINT_FLAGGED,
                (true, true) => 0x8040_7090,
                (true, false) => crate::inventory::TINT_DIM,
            };
        }
    }
    rows
}

/// FUN_101f2de0 / FUN_101f2ec0: [Use, Drop] enabled. Use: crystals and maze tabulae, non-equipment with record flag
/// 0x400, equipped charged gear whose extdata (kind 1) has u16 @2 bit 0x4000; never as a monster (main job 0x17).
/// Drop: the slot's lock byte is 0.
pub(super) fn iuse_states(w: &World, index: u8, id: u16) -> [bool; 2] {
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return [false, false] };
    let ext = w.inv.ext.get(&(INVENTORY, index));
    let equipped = w.inv.equip.contains(&Some((INVENTORY, index)));
    let usable = matches!(it.item_type, 8 | 0x16 | 0x17)
        || (!matches!(it.item_type, 4 | 5) && it.flags & 0x400 != 0)
        || (matches!(it.item_type, 4 | 5) && it.max_charges != 0 && equipped && ext.is_some_and(|e| e[0] == 1 && u16::from_le_bytes([e[2], e[3]]) & 0x4000 != 0));
    // FUN_101f2de0 / FUN_101f2ec0: Use is off in Monstrosity.
    let monster = super::monstrosity::active(w);
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
        gu.disabled_children("menu    iuse", x, y, &off, child);
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

/// iuse Use (FUN_101f33a0 case 1): tabulae open a window we do not have yet, crystals the synthesis; anything else passes the
/// status gate 0x7f, or, for an item with record flag 0x0008 in the Mog Garden, gate 0x77 quietly (the Mog House bit
/// left out), and opens the target prompt with the item's targets (0x7f with no record), title "Items", help 0xe 4.
fn use_entry(w: &mut World, index: u8, id: u16) {
    let it = w.item_info.as_ref().and_then(|c| c.item(id));
    match it.as_ref().map(|i| i.item_type) {
        Some(0x16 | 0x17) => log::info!("iuse: the maze tabula window is not implemented"),
        // A crystal (type 8): the status gate 0x77, then the synthesis window (FUN_101e0e30); a refusal plays SE 0004.
        Some(8) => {
            if !super::textcmd::status_gate(w, 0x77) || !super::synth::open(w, index, id) {
                w.sounds.push(crate::audio::Ui::Error as u32);
            }
        }
        _ => {
            use super::textcmd::{gate_line, status_gate};
            let garden = it.as_ref().is_some_and(|i| i.flags & 0x0008 != 0) && gate_line(w, 0x80).is_some() && gate_line(w, 0x77).is_none();
            if garden || status_gate(w, 0x7f) {
                let targets = it.map_or(0x7f, |i| i.targets as u32);
                super::target::open_sub(w, targets, super::target::SubThen::Item { container: INVENTORY, index }, None, String::new(), 4);
            }
        }
    }
}

/// Seconds after the last new item before a container kept sorted sorts (a pickup's updates come in a burst).
pub(super) const SORT_DELAY: f32 = 0.5;

/// Ours: the containers kept sorted (config auto_sort) sort again once their new items are in.
pub(super) fn auto_sort_tick(w: &mut World, dt: f32) {
    if w.sort_due.0 == 0 {
        return;
    }
    w.sort_due.1 -= dt;
    if w.sort_due.1 > 0.0 {
        return;
    }
    let due = std::mem::take(&mut w.sort_due.0);
    for c in (0..32u8).filter(|c| due >> c & 1 != 0) {
        log::info!("items: container {c} kept sorted, sorting");
        auto_sort_c(w, c);
    }
}

/// Auto keeps the container sorted (ours), Manual stops it.
fn keep_sorted(w: &mut World, c: u8, on: bool) {
    let v = if on { w.auto_sort | 1 << c } else { w.auto_sort & !(1 << c) };
    if v != w.auto_sort {
        (w.auto_sort, w.auto_sort_dirty) = (v, true);
    }
}

/// FUN_1021c7e0 (sort type config 0xca): 0 sorts the view by the record's key (+0xc, our `resource`) then id, 1 pulls
/// identical ids next to their first; then c2s 0x03A {container} merges the stacks.
fn auto_sort(w: &mut World) {
    auto_sort_c(w, INVENTORY);
}

/// FUN_1021c7e0 on a container; 0x03A {c} is sent again by the storage view's itmsortw (LAB_101f3780: two in all).
fn auto_sort_c(w: &mut World, c: u8) {
    let grouped = w.game.get(0xca) == 1;
    let info = w.item_info.clone();
    let key = |s: &Slot| (info.as_ref().and_then(|c| c.item(s.id)).map_or(0xffff, |it| it.resource), s.id);
    let mut v = w.inv.viewed(c).into_iter().filter(|s| s.index != 0).collect::<Vec<_>>();
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
    let gil = w.inv.order.get(&c).is_some_and(|o| o.contains(&0));
    let order: Vec<u8> = gil.then_some(0).into_iter().chain(v.iter().map(|s| s.index)).collect();
    w.inv.order.insert(c, order);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x03A, body: (c as u32).to_le_bytes().to_vec() });
}

/// FUN_101f0b30's second pick: the same row splits one off (to index 0x52, inf: the server's first free slot) when
/// the stack holds more than one and stacks; another row of the same item takes the whole first stack; then the view
/// entry moves (FUN_1021cac0, client only).
fn manual_move(w: &mut World, from: u8, to: &Slot) {
    manual_move_c(w, INVENTORY, from, to);
}

/// FUN_101dbed0: the same in a storage container.
fn manual_move_c(w: &mut World, c: u8, from: u8, to: &Slot) {
    let Some(&(id, count)) = w.inv.slots.get(&(c, from)) else { return };
    let stack = w.item_info.as_ref().and_then(|c| c.item(id)).map_or(1, |it| it.stack);
    let mv = |w: &World, n: u32, i1: u8, i2: u8| {
        let mut b = vec![0u8; 8];
        b[0..4].copy_from_slice(&n.to_le_bytes());
        (b[4], b[5], b[6], b[7]) = (c, c, i1, i2);
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
    if let Some(o) = w.inv.order.get_mut(&c)
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
    match win {
        Win::Items { .. } if item_prompt(w) => {}
        Win::Items { scroll, cursor, store: Some(b), .. } => rects.extend(draw_storage(gu, w, info, *scroll, *cursor, b, sw, sh, frame)),
        Win::Items { scroll, cursor, sub, direct, sort, manual, .. } => {
            let shown = listed(w, *direct);
            let rows = list_rows(w, info, &shown, *direct);
            let list = ItemList { rows, scroll: *scroll, cursor: *cursor };
            if !*direct && sub.is_none() && sort.is_none() {
                draw_count(gu, shown.len(), w.inv.sizes[INVENTORY as usize], sw, sh);
            }
            upload_icons(gu, info, list.visible().iter().map(|r| r.id));
            let open = gu.gate_layout("menu    inventor", sw, sh, None);
            let (b, ui, style, font) = gu.parts();
            let bar = ui.menu("menu    inventor").filter(|_| open).map(|m| push_inventory(b, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, frame));
            if let Some([x, y, h]) = bar.filter(|_| sub.is_none() && sort.is_none()) {
                gu.bar_site(Owner::Items, x, y, h, list.rows.len(), VISIBLE_ROWS, list.scroll);
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
        Win::Equip(_) => rects.extend(super::equipwin::draw(gu, w, sw, sh)),
    }
    rects
}

/// Bank-side titles by container (FUN_1011ae30's table, also the inventory side's): group 0xd.
pub(super) fn title_id(c: u8) -> usize {
    match c {
        1 => 3,
        2 => 0x25,
        4 => 0x8b,
        5 => 0x97,
        6 => 0x9e,
        7 => 0xa6,
        8 => 0xa8,
        9 => 0xb5,
        10..=12 => 0xb9 + (c as usize - 10),
        13..=16 => 0xbf + (c as usize - 13),
        17 => 0xc5,
        _ => 2,
    }
}

/// FUN_101db0a0: a row's help names where it goes (group 0xe), 0 for none.
fn dest_help(c: u8) -> usize {
    match c {
        0 => 0x42d,
        1 => 0x42e,
        9 => 0x42f,
        2 => 0x430,
        4 => 0x431,
        5 => 0x432,
        6 => 0x433,
        7 => 0x434,
        8 => 0x435,
        10..=16 => 0x436 + (c as usize - 10),
        _ => 0,
    }
}

/// FUN_10097b50: the residential zones where the moogle is at hand (table 0x1035b680).
pub(super) fn moogle_zone(zone: u16) -> bool {
    matches!(zone, 230..=232 | 234..=236 | 238..=241 | 243..=246 | 48 | 50 | 80 | 87 | 94 | 256 | 257 | 280)
}

fn wardrobe(c: u8) -> bool {
    matches!(c, 8 | 10..=16)
}

/// A container's rows in view order (the inventory as the item list shows it).
pub(super) fn store_rows(w: &World, c: u8) -> Vec<Slot> {
    if c == INVENTORY {
        return listed(w, false);
    }
    w.inv.viewed(c).into_iter().filter(|s| s.index != 0 && s.id != 0).collect()
}

/// FUN_1021cc60: an empty slot in 1..size.
pub(super) fn has_free(w: &World, c: u8) -> bool {
    (w.inv.sizes[c as usize] as usize).saturating_sub(1) > w.inv.slots_of(c).iter().filter(|x| x.index != 0).count()
}

/// The two-list view on `bank` (FUN_101dc880): focused on the bank side.
fn open_view(w: &mut World, bank: u8, at_moogle: bool, read_only: bool, from_menu: bool) {
    open_view_n(w, bank, at_moogle, false, read_only, from_menu);
}

fn open_view_n(w: &mut World, bank: u8, at_moogle: bool, nomad: bool, read_only: bool, from_menu: bool) {
    let st = Storage { bank, side: INVENTORY, scroll: 0, cursor: 0, on_bank: true, qty: None, from_menu, at_moogle, nomad, read_only, sortw: None, manual: None, stora: None };
    match &mut w.win {
        Some(Win::Items { store, sort, direct: false, .. }) if from_menu => {
            *store = Some(st);
            *sort = None;
        }
        _ => {
            w.win = Some(Win::Items { scroll: 0, cursor: 0, sub: None, direct: false, sort: None, manual: None, store: Some(st) });
            w.menu = None;
            w.action_menu = None;
        }
    }
}

/// FUN_101daf70(0x11): the recycle bin, from itmsort2 (`from_menu`) or `/recycle`.
pub fn open_bin(w: &mut World, from_menu: bool) -> bool {
    if !super::textcmd::status_gate(w, 0x44) {
        return false;
    }
    open_view(w, BIN, false, false, from_menu);
    true
}

/// FUN_101daf70(c, need_satchel, nomad): Satchel, Sack, Case, the wardrobes. Gate 0x44 (the costume bit and the job
/// change cooldown are not tracked); a missing Satchel (ItemNum < 30) fails silently. At the moogle in our own Mog
/// House of a residential zone (with the moogle's line 0x2e), or by a nomad moogle in a MogZoneFlag zone; else the
/// Safe / Storage / Locker rows stay grey. Opens anywhere.
pub fn open_daf70(w: &mut World, c: u8, need_satchel: bool, nomad: bool) -> bool {
    if c == BIN {
        return open_bin(w, false);
    }
    if !super::textcmd::status_gate(w, 0x44) || super::moogle::job_cooling(w) || (need_satchel && w.inv.sizes[5] < 0x1e) {
        return false;
    }
    if w.myroom.is_some() && w.zone.is_some_and(moogle_zone) && w.mh_owner == 0 {
        super::moogle::line(w, 0x2e);
        open_view_n(w, c, true, nomad, false, false);
    } else if nomad && w.mog_zone {
        open_view_n(w, c, true, true, false, false);
    } else {
        open_view(w, c, false, false, false);
    }
    true
}

/// FUN_101dae10(src, c, fn, 0): Mog Safe 1, Storage 2, Locker 4, Safe 2 9. Outside the Mog House the container alone
/// in a read-only list; the Locker without a lease (ItemNum2[4] == 0) read-only with group 7 0xb6; the moogle away
/// read-only with 0x48; visiting another's Mog House (s2c 0x067 @0x20) group 7 0xbc; else the view at the moogle.
/// With `nomad` (a nomad moogle's storage2) in a MogZoneFlag zone the view opens at once, at the moogle and nomad.
pub fn open_dae10(w: &mut World, c: u8, nomad: bool) -> bool {
    if !super::textcmd::status_gate(w, 0x44) || super::moogle::job_cooling(w) {
        return false;
    }
    let here = w.myroom.is_some() || (nomad && w.mog_zone);
    let refuse = if !here {
        None
    } else if c == 4 && w.inv.sizes2[4] == 0 {
        Some(0xb6)
    } else if nomad {
        open_view_n(w, c, true, true, false, false);
        return true;
    } else if !w.zone.is_some_and(moogle_zone) {
        Some(0x48)
    } else if w.mh_owner != 0 {
        super::textcmd::refuse(w, 0xbc);
        return true;
    } else {
        super::moogle::line(w, 0x2e);
        open_view(w, c, true, false, false);
        return true;
    };
    open_view(w, c, false, true, false);
    if let Some(id) = refuse {
        super::textcmd::refuse(w, id);
    }
    true
}

/// FUN_101db220 for a row of the bank side (`bank_side`) or the inventory side; the first rule that applies wins.
/// Not done: the equipped linkshell (7, s2c 0x0E0).
pub(super) fn tint(w: &World, info: &ItemCache, st: &Storage, bank_side: bool, s: &Slot) -> u32 {
    use crate::inventory::{TINT_DIM, TINT_WHITE};
    let (own, dest) = if bank_side { (st.bank, st.side) } else { (st.side, st.bank) };
    let it = info.item(s.id);
    let furniture = it.as_ref().is_some_and(|it| matches!(it.item_type, 10 | 12 | 14));
    if (!bank_side && st.bank == BIN) || !has_free(w, dest) {
        return TINT_DIM;
    }
    if own == BIN {
        let rare = it.as_ref().is_some_and(|it| it.flags & 0x8000 != 0) && w.inv.slots.iter().any(|(&(c, _), &(id, _))| id == s.id && c != 3 && c != BIN);
        if s.id == 0 || rare {
            return TINT_DIM;
        }
    }
    // Rule 4: Wardrobe 3..8 (0xb..0x10) need lobby feature bit 0x04 << (c - 0xb); the lower of own / dest decides.
    if let Some(c) = [own, dest].into_iter().filter(|c| (0xb..=0x10).contains(c)).min()
        && w.features & (0x04 << (c - 0xb)) == 0
    {
        return TINT_DIM;
    }
    // Installed furniture in the Safe: grey on this floor, dark red on another.
    if matches!(own, 1 | 9)
        && furniture
        && let Some(e) = w.inv.ext.get(&(own, s.index)).filter(|e| e[1] & 0x40 != 0)
    {
        if w.myroom.as_ref().is_some_and(|r| r.floor == e[1] & 3) {
            return TINT_DIM;
        }
        return 0x8060_2020;
    }
    // A Storage piece a mannequin wears (FUN_101334c0) stays.
    if own == 2 && super::mannequin::worn_by_any(w, s.index) {
        return TINT_DIM;
    }
    let equipped = (own == INVENTORY || wardrobe(own)) && w.inv.equip.contains(&Some((own, s.index)));
    let gear = it.as_ref().is_some_and(|it| it.level != 0 && it.races != 0);
    let moogle_only = |c: u8| matches!(c, 1 | 2 | 4 | 9);
    if equipped || (wardrobe(dest) && !gear) || (dest == 2 && furniture) || (!st.at_moogle && (moogle_only(own) || moogle_only(dest))) {
        return TINT_DIM;
    }
    if st.qty.is_some_and(|q| q.index == s.index && q.from == own) {
        return 0x8080_4020;
    }
    TINT_WHITE
}

/// FUN_101dbdb0, Manual's colours: the held row orange, equipped gear green (still movable), a Storage piece a
/// mannequin wears grey, installed furniture grey on this floor and dark red on another, else white.
fn manual_tint(w: &World, c: u8, held: Option<u8>, s: &Slot) -> u32 {
    let furniture = w.item_info.as_ref().and_then(|i| i.item(s.id)).is_some_and(|it| matches!(it.item_type, 10 | 12 | 14));
    let installed = w.inv.ext.get(&(c, s.index)).filter(|e| matches!(c, 1 | 9) && furniture && e[1] & 0x40 != 0);
    if held == Some(s.index) {
        0x8080_4020
    } else if (c == INVENTORY || wardrobe(c)) && w.inv.equip.contains(&Some((c, s.index))) {
        0x8050_9050
    } else if c == 2 && super::mannequin::worn_by_any(w, s.index) {
        crate::inventory::TINT_DIM
    } else if let Some(e) = installed {
        if w.myroom.as_ref().is_some_and(|r| r.floor == e[1] & 3) { crate::inventory::TINT_DIM } else { 0x8060_2020 }
    } else {
        crate::inventory::TINT_WHITE
    }
}

/// FUN_100eb710: c2s 0x029 {n, from, to, slot, 0x52} (0x52: the destination's first free slot).
pub(super) fn send_move(w: &World, from: u8, to: u8, index: u8, n: u32) {
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&n.to_le_bytes());
    (b[4], b[5], b[6], b[7]) = (from, to, index, 0x52);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x029, body: b });
}

/// Num+ in the storage view (FUN_101db690 case 8): itmsortw for the focused side, SE 0013; nothing for the bin or the
/// read-only list.
pub fn store_num_plus(w: &mut World) {
    let Some(Win::Items { store: Some(st), .. }) = &mut w.win else { return };
    if st.bank == BIN || st.read_only || st.qty.is_some() || st.manual.is_some() {
        return;
    }
    if st.stora.is_some() {
        st.stora = None;
        w.sounds.push(crate::audio::Ui::Close as u32);
        return;
    }
    st.sortw = match st.sortw {
        Some(_) => None,
        None => Some(SortW { cursor: 1, bank_side: st.on_bank, ask: None }),
    };
    w.sounds.push(crate::audio::Ui::Close as u32);
}

/// The storage view's keys: FUN_101db690 (both sides), FUN_101dcb80 / FUN_101f2bb0 (Enter), itemctrl FUN_1013a6c0,
/// itmsortw FUN_101f3a50, Manual FUN_101dbed0.
fn store_key(w: &mut World, info: &ItemCache, key: egui::Key, page: bool, alt: bool) {
    use crate::audio::Ui;
    use egui::Key as K;
    let Some(Win::Items { store: Some(b), scroll, cursor, .. }) = &w.win else { return };
    let (mut b, mut scroll, mut cursor) = (*b, *scroll, *cursor);
    let banks = store_rows(w, b.bank);
    let inv = store_rows(w, b.side);
    let (mut close, mut back) = (false, false);
    if let Some(mut q) = b.qty {
        // itemctrl: Up / Down +-1 (the step of 10 after a long hold is not done), Right 1, Left the count.
        match key {
            K::ArrowUp => q.value = (q.value + 1).min(q.max),
            K::ArrowDown => q.value = q.value.saturating_sub(1).max(1),
            K::ArrowRight => q.value = 1,
            K::ArrowLeft => q.value = q.max,
            _ => {}
        }
        b.qty = Some(q);
        match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
            K::Enter => {
                send_move(w, q.from, q.to, q.index, q.value);
                b.qty = None;
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                b.qty = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    } else if let (Some(row), Some(sw)) = (b.stora, b.sortw) {
        // itmstora (FUN_101f3e60): Up / Down skip disabled rows (inferred); Enter switches the side itmsortw serves
        // and focuses that list; Escape / Num+ back to itmsortw (SE 0013).
        let off = stora_off(w, &b);
        let step = |r: u16, up: bool| -> u16 {
            let mut r = r;
            for _ in 0..16 {
                r = if up { if r <= 1 { 16 } else { r - 1 } } else if r >= 16 { 1 } else { r + 1 };
                if !off.contains(&r) {
                    break;
                }
            }
            r
        };
        match key {
            K::ArrowUp | K::ArrowDown => {
                b.stora = Some(step(row, key == K::ArrowUp));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter if off.contains(&row) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                let c = stora_container(row);
                if sw.bank_side {
                    (b.bank, b.scroll, b.cursor) = (c, 0, 0);
                } else {
                    (b.side, scroll, cursor) = (c, 0, 0);
                }
                (b.stora, b.sortw, b.on_bank) = (None, None, sw.bank_side);
            }
            K::Escape => {
                b.stora = None;
                w.sounds.push(Ui::Close as u32);
            }
            _ => {}
        }
    } else if let Some(mut sw) = b.sortw {
        let c = if sw.bank_side { b.bank } else { b.side };
        if let Some(yes) = sw.ask {
            match key {
                K::ArrowUp | K::ArrowDown => {
                    sw.ask = Some(!yes);
                    w.sounds.push(Ui::Cursor as u32);
                }
                // LAB_101f3780: Yes sorts the side's container (0x03A {c} here and again in the sort), focus back.
                K::Enter | K::Escape => {
                    let yes = yes && key == K::Enter;
                    sw.ask = None;
                    w.sounds.push(if yes { Ui::Select } else { Ui::Cancel } as u32);
                    if yes {
                        if c != INVENTORY {
                            let _ = w.net.commands.send(NetCommand::Packet { id: 0x03A, body: (c as u32).to_le_bytes().to_vec() });
                        }
                        auto_sort_c(w, c);
                        keep_sorted(w, c, true);
                        b.sortw = None;
                        b.on_bank = sw.bank_side;
                        write_store(w, b, scroll, cursor);
                        return;
                    }
                }
                _ => {}
            }
            b.sortw = Some(sw);
        } else {
            match key {
                K::ArrowUp => sw.cursor = if sw.cursor <= 1 { 3 } else { sw.cursor - 1 },
                K::ArrowDown => sw.cursor = if sw.cursor >= 3 { 1 } else { sw.cursor + 1 },
                K::Enter if sw.cursor == 1 => {
                    sw.ask = Some(false);
                    w.sounds.push(Ui::Select as u32);
                }
                K::Enter if sw.cursor == 2 => {
                    keep_sorted(w, c, false);
                    b.manual = Some((sw.bank_side, None));
                    b.on_bank = sw.bank_side;
                    w.sounds.push(Ui::Select as u32);
                    b.sortw = None;
                    write_store(w, b, scroll, cursor);
                    return;
                }
                // Row 3 Switch: itmstora over itmsortw, on the first enabled row (inferred).
                K::Enter => {
                    let off = stora_off(w, &b);
                    b.stora = (1..=16).find(|r| !off.contains(r));
                    w.sounds.push(Ui::Select as u32);
                }
                K::Escape => {
                    b.sortw = None;
                    b.on_bank = sw.bank_side;
                    w.sounds.push(Ui::Close as u32);
                    write_store(w, b, scroll, cursor);
                    return;
                }
                _ => {}
            }
            if matches!(key, K::ArrowUp | K::ArrowDown) {
                w.sounds.push(Ui::Cursor as u32);
            }
            b.sortw = Some(sw);
        }
    } else if let Some((bank_side, held)) = b.manual {
        let c = if bank_side { b.bank } else { b.side };
        let rows = if bank_side { &banks } else { &inv };
        let (sc, cu) = if bank_side { (&mut b.scroll, &mut b.cursor) } else { (&mut scroll, &mut cursor) };
        match key {
            K::Enter => {
                // A grey row takes no Enter (FUN_101f6c70: SE 0004).
                let row = rows.get(*sc + *cu).cloned();
                if row.as_ref().is_some_and(|s| manual_tint(w, c, held, s) == crate::inventory::TINT_DIM) {
                    w.sounds.push(Ui::Error as u32);
                } else if let Some(s) = row {
                    w.sounds.push(Ui::Select as u32);
                    match held {
                        None => b.manual = Some((bank_side, Some(s.index))),
                        Some(from) => {
                            b.manual = Some((bank_side, None));
                            manual_move_c(w, c, from, &s);
                        }
                    }
                }
            }
            K::Escape if held.is_some() => b.manual = Some((bank_side, None)),
            K::Escape => {
                b.manual = None;
                b.sortw = Some(SortW { cursor: 2, bank_side, ask: None });
            }
            k => {
                if list_keys(sc, cu, rows.len(), k) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
        }
    } else if b.read_only {
        match key {
            K::ArrowUp | K::ArrowDown => {
                if list_keys(&mut b.scroll, &mut b.cursor, banks.len(), key) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            K::ArrowRight | K::ArrowLeft => {
                if page_keys(&mut b.scroll, &mut b.cursor, banks.len(), key == K::ArrowRight) {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter => w.sounds.push(Ui::Error as u32),
            K::Escape => {
                close = true;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    } else {
        match key {
            K::ArrowUp | K::ArrowDown if b.on_bank => {
                if list_keys(&mut b.scroll, &mut b.cursor, banks.len(), key) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            K::ArrowUp | K::ArrowDown => {
                if list_keys(&mut scroll, &mut cursor, inv.len(), key) {
                    w.sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            // Key functions 0xb5 / 0xb6 (Alt+Right / Alt+Left): the focused side's container cycles through table
            // 0x1037ef98 (FUN_101dbb20 / FUN_101dbba0), skipping what FUN_101dbc20 rejects; not for the bin.
            K::ArrowRight | K::ArrowLeft if alt => {
                if b.bank != BIN
                    && let Some(c) = cycle(w, &b, b.on_bank, key == K::ArrowRight)
                {
                    if b.on_bank {
                        (b.bank, b.scroll, b.cursor) = (c, 0, 0);
                    } else {
                        (b.side, scroll, cursor) = (c, 0, 0);
                    }
                }
            }
            // Both sources' +0x1c is 0: Right / Left page only with 0xa8 held.
            K::ArrowRight | K::ArrowLeft if page => {
                let right = key == K::ArrowRight;
                let moved = if b.on_bank { page_keys(&mut b.scroll, &mut b.cursor, banks.len(), right) } else { page_keys(&mut scroll, &mut cursor, inv.len(), right) };
                if moved {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            // FUN_101f7080(-1, child): the focus moves with the visible row (clamped), SE 0013.
            K::ArrowRight if b.on_bank => {
                b.on_bank = false;
                cursor = b.cursor.min(inv.len().saturating_sub(scroll + 1)).min(VISIBLE_ROWS - 1);
                w.sounds.push(Ui::Close as u32);
            }
            K::ArrowLeft if !b.on_bank => {
                b.on_bank = true;
                b.cursor = cursor.min(banks.len().saturating_sub(b.scroll + 1)).min(VISIBLE_ROWS - 1);
                w.sounds.push(Ui::Close as u32);
            }
            K::Enter => {
                let (s, bank_side) = if b.on_bank { (banks.get(b.scroll + b.cursor), true) } else { (inv.get(scroll + cursor), false) };
                if let Some(s) = s.cloned() {
                    let t = tint(w, info, &b, bank_side, &s);
                    let locked = !bank_side && w.inv.locks.get(&(b.side, s.index)).copied().unwrap_or(0) == 0x19;
                    if locked || t == crate::inventory::TINT_DIM || t == 0x8060_2020 {
                        w.sounds.push(Ui::Error as u32);
                    } else {
                        w.sounds.push(Ui::Select as u32);
                        let (from, to) = if bank_side { (b.bank, b.side) } else { (b.side, b.bank) };
                        if s.count > 1 {
                            b.qty = Some(Qty { index: s.index, value: 1, max: s.count, from, to });
                        } else {
                            send_move(w, from, to, s.index, 1);
                        }
                    }
                }
            }
            // FUN_101dca30, then FUN_101dc830: the bin back to itmsort2 (FUN_101f2640) when it came from there; the
            // view closes otherwise (FUN_101f2600; no SE in that code: the cancel sound is inferred).
            K::Escape => {
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
    if back {
        if let Some(Win::Items { store, scroll: s, cursor: c, sort, .. }) = &mut w.win {
            (*s, *c) = (scroll, cursor);
            *store = None;
            *sort = Some(SortWin { cursor: 3, ask: None });
        }
        return;
    }
    write_store(w, b, scroll, cursor);
}

/// FUN_101f3f80: itmstora's rows to containers (1 Inventory, 2 Mog Safe, 3 Safe 2, 4 Storage, 5 Locker, 6 Satchel,
/// 7 Sack, 8 Case, 9 Wardrobe, 10..16 Wardrobe 2..8).
fn stora_container(row: u16) -> u8 {
    match row {
        1 => 0,
        2 => 1,
        3 => 9,
        4 => 2,
        5..=9 => (row - 1) as u8,
        r => r as u8,
    }
}

/// FUN_101dbc20 for a container: not on either side, not 1 / 2 / 4 / 9 away from the moogle, the Locker leased, the
/// Satchel there (ItemNum >= 30), Safe 2 owned (s2c 0x067 @0x27), Sack and wardrobes 2..8 with a size.
pub(super) fn usable(w: &World, b: &Storage, c: u8) -> bool {
    let n = |c: u8| w.inv.sizes[c as usize];
    !(c == b.bank
        || c == b.side
        || (!b.at_moogle && matches!(c, 1 | 2 | 4 | 9))
        || (b.at_moogle && b.nomad && c == 2)
        || (c == 4 && w.inv.sizes2[4] == 0)
        || (c == 5 && n(5) < 0x1e)
        || (c == 9 && !w.mh_floors)
        || (matches!(c, 6 | 10..=16) && n(c) == 0))
}

/// FUN_101f3c60: itmstora's disabled rows.
fn stora_off(w: &World, b: &Storage) -> Vec<u16> {
    (1..=16).filter(|&r| !usable(w, b, stora_container(r))).collect()
}

/// The next usable container after the focused side's in table 0x1037ef98, forward or back.
fn cycle(w: &World, b: &Storage, bank_side: bool, forward: bool) -> Option<u8> {
    const ORDER: [u8; 16] = [0, 1, 9, 2, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16];
    let cur = if bank_side { b.bank } else { b.side };
    let at = ORDER.iter().position(|&c| c == cur).unwrap_or(0);
    (1..16).map(|k| ORDER[if forward { (at + k) % 16 } else { (at + 16 - k) % 16 }]).find(|&c| usable(w, b, c))
}

fn write_store(w: &mut World, b: Storage, scroll: usize, cursor: usize) {
    if let Some(Win::Items { store, scroll: s, cursor: c, .. }) = &mut w.win {
        (*s, *c) = (scroll, cursor);
        *store = Some(b);
    }
}

/// The bars in the storage view: itemctrl (0xe 0x10c / 0xd 0x3e), itmsortw's rows and sortyn (0xc 0x3d), Manual
/// (0xe 5 / 6), else the focused list's container title and the row help naming the destination; the read-only list
/// puts its title in slot 2.
fn store_bars(gu: &GameUi, b: &Storage, bars: &mut super::hudtop::BarText) {
    if b.qty.is_some() {
        bars.help[1] = gu.menu_msg(0x10c, "");
        bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
        return;
    }
    if let Some(row) = b.stora {
        let [help, title] = gu.focus_text("menu    itmstora", row);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        return;
    }
    if let Some(sw) = b.sortw {
        if sw.ask.is_some() {
            bars.help[1] = gu.lobby_msg(0x0c, 0x3d, "");
            bars.title[1] = gu.title_msg(2).unwrap_or_default();
        } else {
            let [help, title] = gu.focus_text("menu    itmsortw", sw.cursor);
            (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        }
        return;
    }
    if b.read_only {
        bars.title[2] = gu.title_msg(title_id(b.bank)).unwrap_or_default();
        return;
    }
    let (own, dest) = if b.on_bank { (b.bank, b.side) } else { (b.side, b.bank) };
    bars.title[1] = gu.title_msg(title_id(own)).unwrap_or_default();
    bars.help[1] = match b.manual {
        Some((_, held)) => gu.menu_msg(if held.is_some() { 6 } else { 5 }, ""),
        None => match dest_help(dest) {
            0 => String::new(),
            id => gu.menu_msg(id, ""),
        },
    };
}

/// The storage view's windows: "menu    bank" at its layout (16, 48), the inventory list moved to (200, 48), the
/// cursor on the focused one; itmsortw at (384, 48) (not for the bin) with sortyn; itemctrl over them for a stack
/// ("%2d" right-aligned at (+0x34, +0x16), "/%2d" at (+0x3a, +0x16), FUN_1013a520). Read-only: the item list alone.
#[allow(clippy::too_many_arguments)]
fn draw_storage(gu: &mut GameUi, w: &World, info: &ItemCache, scroll: usize, cursor: usize, b: &Storage, sw: f32, sh: f32, frame: usize) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let banks = store_rows(w, b.bank);
    if b.read_only {
        let list = ItemList { rows: build_rows(&banks, info, true), scroll: b.scroll, cursor: b.cursor };
        upload_icons(gu, info, list.visible().iter().map(|r| r.id));
        let (bt, ui, style, font) = gu.parts();
        if let Some(m) = ui.menu("menu    inventor") {
            let [x, y, h] = push_item_list(bt, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, frame, 3.0, true, crate::render::invwin::INVENTOR_BAR);
            gu.bar_site(Owner::Bank, x, y, h, list.rows.len(), VISIBLE_ROWS, list.scroll);
        }
        rects.extend(gu.window_rect("menu    inventor", sw, sh));
        return rects;
    }
    let inv = store_rows(w, b.side);
    let manual = |bank_side: bool| b.manual.filter(|m| m.0 == bank_side).map(|m| m.1);
    let tinted = |rows: &[Slot], bank_side: bool| {
        let mut out = build_rows(rows, info, false);
        let c = if bank_side { b.bank } else { b.side };
        for (r, s) in out.iter_mut().zip(rows) {
            r.tint = match manual(bank_side) {
                Some(held) => manual_tint(w, c, held, s),
                None => tint(w, info, b, bank_side, s),
            };
        }
        out
    };
    let blist = ItemList { rows: tinted(&banks, true), scroll: b.scroll, cursor: b.cursor };
    let ilist = ItemList { rows: tinted(&inv, false), scroll, cursor };
    upload_icons(gu, info, blist.visible().iter().chain(ilist.visible()).map(|r| r.id));
    let free = b.qty.is_none() && b.sortw.is_none() && b.stora.is_none();
    // FUN_101dcb40 for the bank side; the inventory side's own list draws it the same way (inferred).
    if free {
        let (n, c) = if b.on_bank { (banks.len(), b.bank) } else { (inv.len(), b.side) };
        draw_count(gu, n, w.inv.sizes[c as usize], sw, sh);
    }
    let (bt, ui, style, font) = gu.parts();
    let mut bars = Vec::new();
    if let Some(m) = ui.menu("menu    bank") {
        let r = push_item_list(bt, ui, style, font, GameUi::ATLAS, m, &blist, sw, sh, frame, 4.0, free && b.on_bank, crate::render::invwin::LIST_BAR);
        bars.push((Owner::Bank, r, &blist, b.on_bank));
    }
    if let Some(m) = ui.menu("menu    inventor") {
        let mut m = m.clone();
        if let Some(e) = m.elements.first_mut() {
            e.x = BIN_LIST_X;
        }
        let r = push_item_list(bt, ui, style, font, GameUi::ATLAS, &m, &ilist, sw, sh, frame, 3.0, free && !b.on_bank, crate::render::invwin::INVENTOR_BAR);
        bars.push((Owner::StoreInv, r, &ilist, !b.on_bank));
    }
    for (owner, [x, y, h], l, on) in bars {
        if free && on {
            gu.bar_site(owner, x, y, h, l.rows.len(), VISIBLE_ROWS, l.scroll);
        }
    }
    rects.extend(gu.window_rect("menu    bank", sw, sh));
    if let Some(mut r) = gu.window_rect("menu    inventor", sw, sh) {
        r[0] += BIN_LIST_X as f32 - gu.window_rect_x("menu    inventor");
        rects.push(r);
    }
    if b.bank != BIN {
        // itmsortw hides while itmstora is up (FUN_1015e6e0); itmstora's disabled rows take their dim sprites (kind 4).
        if let Some(row) = b.stora {
            let off = stora_off(w, b);
            if let Some(win) = gu.lobby_window_off("menu    itmstora", sw, sh, (0.0, 0.0), Some(row), true, &off) {
                let [x, y, ..] = win.frame;
                gu.disabled_children("menu    itmstora", x, y, &off, Some(row));
                rects.push(win.frame);
            }
        } else {
            let sel = b.sortw.map(|s| s.cursor);
            rects.extend(gu.lobby_window_off("menu    itmsortw", sw, sh, (0.0, 0.0), sel, sel.is_some(), &[]).map(|x| x.frame));
            if let Some(yes) = b.sortw.and_then(|s| s.ask) {
                rects.extend(gu.lobby_window_off("menu    sortyn", sw, sh, (0.0, 0.0), Some(if yes { 1 } else { 2 }), true, &[]).map(|x| x.frame));
            }
        }
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

/// FUN_101cb590: "items/size" at titlewin's right edge while an item list is focused: "/" at (x1 - 0x16, y0 + 8), the
/// count right-aligned at (x1 - 0x13, y0 + 4), the size right-aligned at (x1 - 1, y1 - 0x10), all 0x80808080.
/// Retail takes temporary items and a few table counts off (FUN_101f2740 tail, not traced); ours counts the rows.
fn draw_count(gu: &mut GameUi, n: usize, size: u8, sw: f32, sh: f32) {
    let Some([x0, y0, w, h]) = gu.window_rect("menu    titlewin", sw, sh) else { return };
    let (x1, y1) = (x0 + w, y0 + h);
    let white = 0x8080_8080;
    gu.shape_text("/", x1 - 22.0, y0 + 8.0, white, false);
    gu.shape_text(&n.to_string(), x1 - 19.0, y0 + 4.0, white, true);
    gu.shape_text(&(size.saturating_sub(1)).to_string(), x1 - 1.0, y1 - 16.0, white, true);
}

/// Mouse on the item list: hover moves the cursor, a click on the highlighted row acts as Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    if matches!(w.win, Some(Win::Equip(_))) {
        super::equipwin::pointer(w, gu, sw, sh, p, moved, clicked);
        return;
    }
    // The bin view moves the list; its mouse handling (hover messages 3 / 4) is not done.
    if matches!(w.win, Some(Win::Items { store: Some(_), .. })) {
        return;
    }
    let Some(r) = gu.window_rect("menu    inventor", sw, sh) else { return };
    let row = ((p.1 - r[1] - 4.0) / ROW_PITCH).floor();
    let inside = p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3] && row >= 0.0 && (row as usize) < VISIBLE_ROWS;
    if inside && matches!(w.win, Some(Win::Items { .. })) {
        gu.hover.set(true);
    }
    if !inside || !(moved || clicked) {
        return;
    }
    let row = row as usize;
    let (cur, enter) = match &mut w.win {
        Some(Win::Items { cursor, .. }) => (cursor, true),
        _ => return,
    };
    let same = *cur == row;
    *cur = row;
    if clicked && same && enter {
        key(w, egui::Key::Enter, false, false);
    }
}

/// The status panel from 0x061 and 0x0DF.
pub(super) fn persona(w: &World) -> Persona {
    let st = w.stats.clone().unwrap_or_default();
    let (hp, mp, tp) = w.vitals.map_or((0, 0, 0), |v| (v.0, v.1, v.2));
    let job = |(j, lv): (u8, u8)| (j != 0).then_some((lv, j));
    Persona::build(&Stats {
        name: w.name.clone(),
        main: job(st.main),
        sub: job(st.sub),
        main_blue: w.merits.is_some_and(|m| m.1 & 0x4000 != 0),
        enc: w.job_info.map_or(0, |j| j.encumbrance),
        ready: w.recasts_seen && w.stats.is_some(),
        // FUN_101cc5f0: 0x061 @0x55 + 99, "---/" for 0.
        ilvl: (st.ilvl != 0).then_some(st.ilvl as u16 + 99),
        su: st.su,
        hp: (hp, st.hp_max),
        mp: (mp, st.mp_max),
        tp,
        base: st.base,
        bonus: st.adj,
    })
}
