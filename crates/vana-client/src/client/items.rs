//! Items and Equipment in the live client: containers from s2c 0x01C / 0x01E / 0x01F / 0x020 / 0x050,
//! the "inventor", "equip", "persona" and "money" windows (inventory.rs models, render/invwin.rs
//! drawing); equipping sends c2s 0x050 (FUN_100e25d0: index, slot, container; index 0 removes).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::dat::items::Item;
use crate::inventory::{Equipment, ItemInfo, ItemList, Persona, ROW_PITCH, SLOT_CHILD, Slot, Stats, VISIBLE_ROWS, build_rows, gil_text};
use crate::render::invwin::{icon_texture, push_equipment, push_inventory, push_money, push_persona};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Main inventory (LSB LOC_INVENTORY).
pub const INVENTORY: u8 = 0;
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
}

impl Inventory {
    pub fn set(&mut self, container: u8, index: u8, id: u16, count: u32) {
        if id == 0 {
            self.slots.remove(&(container, index));
        } else {
            self.slots.insert((container, index), (id, count));
        }
    }

    pub fn count(&mut self, container: u8, index: u8, count: u32) {
        if count == 0 {
            self.slots.remove(&(container, index));
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
    /// Main menu / action menu Items: the inventory list.
    Items { scroll: usize, cursor: usize },
    /// Equipment: grid cursor (row, column), and the list of candidates once a slot is picked.
    Equip { row: usize, col: usize, pick: Option<Pick> },
}

pub struct Pick {
    /// Container slot indexes of the candidates.
    pub indexes: Vec<u8>,
    pub scroll: usize,
    pub cursor: usize,
}

pub fn open_items(w: &mut World) {
    w.win = Some(Win::Items { scroll: 0, cursor: 0 });
    w.menu = None;
    w.action_menu = None;
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
    let rows = w.inv.slots_of(INVENTORY).iter().filter(|s| s.index != 0).count();
    let mut send = None;
    let mut close = false;
    match &mut w.win {
        None => return false,
        Some(Win::Items { scroll, cursor }) => match key {
            egui::Key::Escape => close = true,
            egui::Key::Enter => log::info!("using items is not implemented yet"),
            k => list_keys(scroll, cursor, rows, k),
        },
        Some(Win::Equip { row, col, pick }) => {
            let slot = slot_at(*row, *col);
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
                        *pick = Some(Pick { indexes, scroll: 0, cursor: 0 });
                    }
                    _ => {}
                },
            }
        }
    }
    if close {
        w.win = None;
    }
    if let Some(c) = send {
        log::info!("equip: {c:?}");
        let _ = w.net.commands.send(c);
    }
    true
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

fn upload_icons(gu: &mut GameUi, info: &ItemCache, ids: impl Iterator<Item = u16>) {
    for id in ids {
        let name = icon_texture(id);
        if !gu.has_image(&name)
            && let Some(rgba) = info.icon(id)
        {
            gu.image(&name, 32, 32, rgba);
        }
    }
}

/// Draws the open window. Returns the rects that take clicks.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(win), Some(info)) = (&w.win, w.item_info.as_ref()) else { return rects };
    let frame = gu.cursor_frame();
    let slots = w.inv.slots_of(INVENTORY);
    match win {
        Win::Items { scroll, cursor } => {
            let list = ItemList { rows: build_rows(&slots, info.as_ref(), false), scroll: *scroll, cursor: *cursor };
            upload_icons(gu, info, list.visible().iter().map(|r| r.id));
            let (b, ui, style, font) = gu.parts();
            if let Some(m) = ui.menu("menu    inventor") {
                push_inventory(b, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, frame);
            }
            rects.extend(gu.window_rect("menu    inventor", sw, sh));
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

/// Mouse on the item list: hover moves the cursor, a click on the highlighted row acts as Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
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
