//! Mog House layout mode (spec ~/ffxi/lab/research/layout_mode_spec.md): the cell grid (FUN_101cecf0 and kin), moving,
//! turning and placing pieces, the selection arrows, the layout camera and the c2s 0x0FA / 0x0FB bodies; the mode
//! itself (FUN_101d3380 mode 1, FUN_101d4d90).

use super::World;
use super::input::{Input, mode};
use super::net::NetCommand;
use crate::audio::Ui;
use crate::dat::furniture::{Furn, FurnTable};
use glam::Vec3;
use std::sync::Arc;

/// The mode's state (the myroom object's fields it uses).
pub struct Layout {
    pub table: Arc<FurnTable>,
    pub floor: u8,
    pub map: u16,
    /// +0x24: 0 top-down, 1 from the door side, 2 from the far side.
    pub view: u8,
    /// +0x7e6: the selected piece (container, slot).
    pub sel: Option<(u8, u8)>,
    /// The cursor's ticks (+0x858).
    pub ticks: f32,
    /// The piece being moved (object state 2).
    pub held: Option<Held>,
    /// An rmlo window (1, 2 or 4) and its cursor row.
    pub win: Option<(u8, u16)>,
    /// bankmenu (Mog Safe / Safe 2) over the rmlo window: its cursor row.
    pub bank: Option<u16>,
    /// The furniture list ("menu    inventor" with layout's source): container, scroll, cursor.
    pub list: Option<(u8, usize, usize)>,
    /// Mode 4: a mannequin being dressed.
    pub dress: Option<super::mannequin::Dress>,
    /// /garden's bankmenu (mode 2): cancelling it leaves (LAB_101d3bf0).
    garden: bool,
    /// The pieces' colours (FUN_101d39a0 for all, FUN_101cea80 for one), ARGB with 0x80 = 1.0.
    color_all: u32,
    colors: std::collections::HashMap<(u8, u8), u32>,
}

/// FUN_101ce9e0 / FUN_101ce900: a piece's pulse, its level (0x80 = 1.0) stepping 8 a tick between 0x20 and 0xff and
/// the pulses left (one counted each time the level rises through 0x80; -1 for ever).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    level: i32,
    step: i32,
    left: i32,
    acc: f32,
}

impl Pulse {
    pub fn new(left: i32) -> Self {
        Pulse { level: 0x80, step: -8, left, acc: 0.0 }
    }

    /// `ticks` more (whole ticks, the rest kept); the level while it runs.
    fn tick(&mut self, ticks: f32) -> Option<i32> {
        if self.left == 0 {
            return None;
        }
        self.acc += ticks;
        while self.acc >= 1.0 && self.left != 0 {
            self.acc -= 1.0;
            self.level += self.step;
            if self.level < 0x20 {
                (self.level, self.step) = (0x40, 8);
            } else if self.level >= 0x100 {
                (self.level, self.step) = (0xff, -8);
            } else if self.level == 0x80 && self.left > 0 {
                self.left -= 1;
            }
        }
        Some(self.level)
    }
}

/// A piece in the room, with its item.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub piece: Piece,
    pub item: u16,
}

/// A held piece (FUN_101d37e0): its cells are free, the pieces on it move with it; `new` until it is installed.
#[derive(Clone, Debug)]
pub struct Held {
    pub piece: Placed,
    pub tops: Vec<Placed>,
    pub new: bool,
}

/// The prompts (FUN_10210e30): "Dispose of what is currently growing?" (0xc 0x35) before a removal, and "Clear away
/// all furniture?" (0xe 0x267).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Dispose { c: u8, slot: u8 },
    ClearAll,
    DisposeAll,
    /// The dressed mannequin's "Remove all equipment from the mannequin?" (9 0xe0).
    Undress,
}

/// The item types layout mode handles: furnishing 10, flowerpot 12, mannequin 14.
const TYPES: [u16; 3] = [10, 12, 14];
const MANNEQUIN: u16 = 14;

/// FUN_101d4d90: the player stands at the room centre while laying out (DAT_1037eba8[floor]).
fn park(floor: u8) -> Vec3 {
    if floor == 0 { Vec3::ZERO } else { Vec3::new(0.0, 0.0, -9.0) }
}

fn item_type(w: &World, id: u16) -> u16 {
    w.item_info.as_ref().and_then(|i| i.item(id)).map_or(0, |it| it.item_type)
}

/// The installed pieces of a floor (exdata[1] bit 6, floor exdata[1] & 3; place exdata[6..9]), containers 1 then 9.
pub fn placed(w: &World, table: &FurnTable, floor: u8) -> Vec<Placed> {
    let mut out = Vec::new();
    for c in [1u8, 9] {
        for slot in 1..0x51u8 {
            let (Some(&(item, _)), Some(ext)) = (w.inv.slots.get(&(c, slot)), w.inv.ext.get(&(c, slot))) else { continue };
            if !TYPES.contains(&item_type(w, item)) || ext[1] & 0x40 == 0 || ext[1] & 3 != floor {
                continue;
            }
            let Some(furn) = table.entry(item) else { continue };
            out.push(Placed { piece: Piece { c, slot, furn, x: ext[6] as i8, level: ext[7] as i8, z: ext[8] as i8, rot: ext[9] & 3 }, item });
        }
    }
    out
}

/// The grid of a floor's pieces: floor pieces first, then those on them.
pub fn grid_of(pieces: &[Placed], floor: u8) -> Grid {
    let mut g = Grid::new(floor);
    for p in pieces.iter().filter(|p| p.piece.level == 0) {
        g.occupy(&p.piece);
    }
    for p in pieces.iter().filter(|p| p.piece.level > 0) {
        g.occupy(&p.piece);
    }
    g
}

impl Layout {
    /// A piece's origin in the room (FUN_101cdd90: its footprint centre at its level, or its wall slot).
    pub fn pos(&self, p: &Piece) -> Option<Vec3> {
        super::furniture::place(&p.furn, self.map, self.floor, p.x, p.level, p.z, p.rot).map(|v| v.0)
    }

    /// The pieces as drawn: the installed ones with the held piece and those on it where they are held.
    fn pieces(&self, w: &World) -> Vec<Placed> {
        let mut v = placed(w, &self.table, self.floor);
        if let Some(h) = &self.held {
            let moved: Vec<Placed> = std::iter::once(h.piece).chain(h.tops.iter().copied()).collect();
            v.retain(|p| !moved.iter().any(|m| (m.piece.c, m.piece.slot) == (p.piece.c, p.piece.slot)));
            v.extend(moved);
        }
        v
    }

    fn selected(&self, pieces: &[Placed]) -> Option<Placed> {
        let s = self.sel?;
        pieces.iter().copied().find(|p| (p.piece.c, p.piece.slot) == s)
    }

    /// The selected piece's origin.
    pub fn sel_pos(&self, w: &World) -> Option<Vec3> {
        self.selected(&self.pieces(w)).and_then(|p| self.pos(&p.piece))
    }

    /// The grid without the held piece and those on it (their cells freed while held).
    fn grid(&self, w: &World) -> Grid {
        let mut v = placed(w, &self.table, self.floor);
        if let Some(h) = &self.held {
            let moved: Vec<(u8, u8)> = std::iter::once(&h.piece).chain(&h.tops).map(|m| (m.piece.c, m.piece.slot)).collect();
            v.retain(|p| !moved.contains(&(p.piece.c, p.piece.slot)));
        }
        grid_of(&v, self.floor)
    }
}

/// The furniture to draw (furniture::changed): exdata[1] / [6..9] of the held piece and those on it replaced by where
/// they are held, a new piece added.
pub fn overrides(w: &World, want: &mut Vec<super::furniture::Key>) {
    let Some(h) = w.layout.as_ref().and_then(|l| l.held.as_ref().map(|h| (l.floor, h))) else { return };
    let (floor, h) = h;
    for p in std::iter::once(&h.piece).chain(&h.tops) {
        let (c, slot) = (p.piece.c, p.piece.slot);
        want.retain(|k| (k.0, k.1) != (c, slot));
        let mut ext = w.inv.ext.get(&(c, slot)).map_or([0u8; 10], |e| e[..10].try_into().unwrap());
        ext[1] = ext[1] & !0x43 | 0x40 | floor;
        (ext[6], ext[7], ext[8], ext[9]) = (p.piece.x as u8, p.piece.level as u8, p.piece.z as u8, p.piece.rot);
        want.push((c, slot, p.item, ext));
    }
    want.sort();
}

/// The record as the server will write it (s2c 0x020): the client moved the piece itself (FUN_101ccf50(5)).
fn record(w: &mut World, p: &Placed, floor: u8, installed: bool) {
    let e = w.inv.ext.entry((p.piece.c, p.piece.slot)).or_insert([0; 24]);
    if installed {
        e[1] = e[1] & !0x43 | 0x40 | floor;
        (e[6], e[7], e[8], e[9]) = (p.piece.x as u8, p.piece.level as u8, p.piece.z as u8, p.piece.rot);
    } else {
        e[1] &= !0x43;
        (e[6], e[7], e[8], e[9]) = (0, 0, 0, 0);
    }
}

/// c2s 0x0FA for a piece where it now stands (FUN_100f0cc0).
fn send_layout(w: &mut World, p: &Placed, floor: u8) {
    let b = layout_body(p.item, p.piece.slot, p.piece.c, floor, p.piece.x, p.piece.level, p.piece.z, p.piece.rot);
    log::info!("layout: 0x0FA {:02x?}", b);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0FA, body: b.to_vec() });
    record(w, p, floor, true);
}

/// c2s 0x0FB: back to the Mog Safe (FUN_100f0d40).
fn send_bankin(w: &mut World, p: &Placed, floor: u8) {
    let b = bankin_body(p.item, p.piece.slot, p.piece.c);
    log::info!("layout: 0x0FB {:02x?}", b);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0FB, body: b.to_vec() });
    record(w, p, floor, false);
}

/// `/layout` passed its gates: FUN_101d3380(1, -1, 1). The action menu, target, main menu and the moogle's menus
/// close; the first piece is selected (none: rmlo1 opens); the player is hidden (event::hidden) and parked at the
/// room centre.
pub fn enter(w: &mut World) {
    let Some(r) = w.myroom else { return };
    let Some(index) = w.index.clone() else { return };
    let table = match FurnTable::load(&index) {
        Ok(t) => Arc::new(t),
        Err(e) => {
            log::warn!("layout: furniture table: {e:#}");
            return;
        }
    };
    (w.menu, w.action_menu, w.held_menu, w.moogle, w.target, w.locked) = (None, None, None, None, None, false);
    let sel = placed(w, &table, r.floor).first().map(|p| (p.piece.c, p.piece.slot));
    log::info!("layout: entered, floor {}, selected {sel:?}", r.floor);
    let win = sel.is_none().then_some((1, 1));
    w.layout = Some(Layout { table, floor: r.floor, map: r.map, view: w.layout_view, sel, ticks: 0.0, held: None, win, bank: None, list: None, dress: None, garden: false, color_all: WHITE, colors: Default::default() });
    w.pos = park(r.floor);
}

/// FUN_101d3380(2, -1, 1) (/garden): bankmenu with LAB_101d3bf0: a pick opens that container's list (mode 3), a
/// cancel leaves (mode 0). Ours enters layout mode first, so the list and the held piece have its camera *(inf)*.
pub fn garden(w: &mut World) {
    enter(w);
    if let Some(l) = w.layout.as_mut() {
        (l.win, l.bank, l.garden) = (None, Some(1), true);
    }
}

/// FUN_101d3380(0, -1, 1): a held piece goes back, the player returns to the room centre, the normal camera behind
/// them, c2s 0x0FA all zero (FUN_100f0c80) and the moogle's menu again.
pub fn leave(w: &mut World) {
    let Some(mut l) = w.layout.take() else { return };
    set_colors(&mut l, WHITE);
    apply_colors(w, &l);
    w.layout_view = l.view;
    w.pos = park(l.floor);
    let yaw = w.yaw;
    super::camera::face(w, yaw);
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0FA, body: vec![0; 12] });
    if w.moogle.is_none() && !super::event::active(w) {
        super::moogle::open(w, false);
    }
    log::info!("layout: left");
}

/// FUN_101d37e0: picks a piece up (state 2); every piece dims to 0x60 and this one (with those on it) pulses for ever;
/// the pieces on it come along.
fn hold(w: &mut World, l: &mut Layout, p: Placed, new: bool) {
    let grid = l.grid(w);
    let pieces = l.pieces(w);
    let tops = if p.piece.surface() { grid.on(p.piece.slot, p.piece.c).iter().filter_map(|&(s, c)| pieces.iter().copied().find(|q| (q.piece.slot, q.piece.c) == (s, c))).collect() } else { Vec::new() };
    l.sel = Some((p.piece.c, p.piece.slot));
    set_colors(l, 0x8060_6060);
    for t in std::iter::once(&p).chain(&tops) {
        w.furniture.pulses.insert((t.piece.c, t.piece.slot), Pulse::new(-1));
    }
    l.held = Some(Held { piece: p, tops, new });
}

/// FUN_101ccf50(5): placed when it fits (0x0FA for it and every piece on it, SE 8060), else SE 0004 and still held.
fn place_held(w: &mut World, l: &mut Layout) {
    let grid = l.grid(w);
    let Some(h) = l.held.take() else { return };
    if grid.fits(&h.piece.piece) {
        send_layout(w, &h.piece, l.floor);
        for t in &h.tops {
            send_layout(w, t, l.floor);
        }
        set_colors(l, WHITE);
        w.sounds.push(8060);
    } else {
        let p = &h.piece.piece;
        log::info!("layout: does not fit at column {} level {} row {} turn {}", p.x, p.level, p.z, p.rot);
        w.sounds.push(Ui::Error as u32);
        l.held = Some(h);
    }
}

/// FUN_101ccf50(6): back to its record's place, or a new piece dropped (SE 0003).
fn cancel_held(w: &mut World, l: &mut Layout) {
    match l.held.take() {
        Some(h) if h.new => l.sel = None,
        Some(_) => set_colors(l, WHITE),
        None => {}
    }
    w.sounds.push(Ui::Cancel as u32);
}

/// FUN_101ce360: the held piece's arrows and turns (context 0x42); Enter / Escape come through `key`.
fn held_keys(w: &mut World, l: &mut Layout, input: &mut Input) {
    let grid = l.grid(w);
    let Some(mut h) = l.held.take() else { return };
    let (eye, look) = (w.cam.eye, w.cam.look);
    let ctrl = w.game.get(0xc1);
    let mut moved = None;
    for (id, dir) in [(0x34, 3), (0x33, 1), (0x31, 0), (0x32, 2)] {
        if input.test(id, mode::REPEAT) {
            moved = Some(step(&grid, &mut h.piece.piece, l.floor, remap(dir, ctrl, eye, look)));
        }
    }
    if let Some(m) = moved {
        if let Some((dx, dz)) = m {
            for t in h.tops.iter_mut() {
                (t.piece.x, t.piece.z) = (t.piece.x + dx as i8, t.piece.z + dz as i8);
            }
            w.sounds.push(8058);
        }
        l.held = Some(h);
        return;
    }
    let by = if input.test(0x8d, mode::PRESS) {
        -1
    } else if input.test(0x8e, mode::PRESS) {
        1
    } else {
        0
    };
    if by != 0 {
        let (dx, dz) = rotate(&grid, &mut h.piece.piece, l.floor, by);
        if !h.piece.piece.furn.wall() {
            for t in h.tops.iter_mut() {
                (t.piece.x, t.piece.z) = (t.piece.x + dx as i8, t.piece.z + dz as i8);
            }
            w.sounds.push(8059);
        }
    }
    l.held = Some(h);
}

/// FUN_101d3c40: a piece and those on it back to the Mog Safe (c2s 0x0FB each); a pot growing something asks first.
fn remove(w: &mut World, l: &mut Layout, c: u8, slot: u8, ask: bool) {
    let pieces = l.pieces(w);
    let grid = l.grid(w);
    let Some(p) = pieces.iter().copied().find(|p| (p.piece.c, p.piece.slot) == (c, slot)) else { return };
    let mut all = vec![p];
    all.extend(grid.on(slot, c).iter().filter_map(|&(s, k)| pieces.iter().copied().find(|q| (q.piece.slot, q.piece.c) == (s, k))));
    if ask && all.iter().any(|q| w.inv.ext.get(&(q.piece.c, q.piece.slot)).is_some_and(|e| e[0] != 0)) {
        w.confirm = Some((super::Confirm::Layout(Ask::Dispose { c, slot }), false));
        return;
    }
    for q in all.iter().rev() {
        send_bankin(w, q, l.floor);
    }
    l.sel = None;
}

/// FUN_101d3cc0: every piece of this floor back to the Mog Safe; a pot growing something asks first.
fn clear_all(w: &mut World, l: &mut Layout, ask_plants: bool) {
    let pieces = l.pieces(w);
    if ask_plants && pieces.iter().any(|q| w.inv.ext.get(&(q.piece.c, q.piece.slot)).is_some_and(|e| e[0] != 0)) {
        w.confirm = Some((super::Confirm::Layout(Ask::DisposeAll), false));
        return;
    }
    for q in &pieces {
        send_bankin(w, q, l.floor);
    }
    l.sel = None;
}

/// A prompt's Yes.
pub fn confirmed(w: &mut World, a: Ask) {
    if a == Ask::Undress {
        super::mannequin::confirmed(w);
        return;
    }
    let Some(mut l) = w.layout.take() else { return };
    match a {
        Ask::Dispose { c, slot } => remove(w, &mut l, c, slot, false),
        Ask::ClearAll => clear_all(w, &mut l, true),
        Ask::DisposeAll => clear_all(w, &mut l, false),
        Ask::Undress => {}
    }
    w.layout = Some(l);
}

pub fn ask_text(gu: &super::game_ui::GameUi, a: Ask) -> String {
    match a {
        Ask::ClearAll => gu.menu_msg(0x267, "Clear away all furniture?"),
        Ask::Undress => super::mannequin::ask_text(gu),
        _ => gu.lobby_msg(0x0c, 0x35, "Dispose of what is currently growing?"),
    }
}

/// FUN_101d4d90 once a frame (context 0x42): select mode's arrows, Num5, Enter and Escape, or the held piece's keys;
/// an event starting leaves at once.
pub fn update(w: &mut World, input: &mut Input, dt: f32) {
    if w.layout.is_none() {
        return;
    }
    if super::event::active(w) || w.myroom.is_none() {
        leave(w);
        return;
    }
    let Some(mut l) = w.layout.take() else { return };
    l.ticks += dt * 60.0;
    frame_colors(w, &mut l, dt * 60.0);
    if let Some(d) = l.dress.as_mut() {
        super::mannequin::update(w, d, dt * 60.0);
        w.layout = Some(l);
        return;
    }
    if l.win.is_some() || w.confirm.is_some() {
        w.layout = Some(l);
        return;
    }
    let pieces = l.pieces(w);
    if l.held.is_none() && l.selected(&pieces).is_none() {
        l.sel = pieces.first().map(|p| (p.piece.c, p.piece.slot));
    }
    if l.sel.is_some() && input.test(0xa7, mode::PRESS) {
        l.view = (l.view + 1) % 3;
    }
    if l.held.is_some() {
        held_keys(w, &mut l, input);
        w.layout = Some(l);
        return;
    }
    let dx = if input.test(0x33, mode::REPEAT) { 1 } else if input.test(0x34, mode::REPEAT) { -1 } else { 0 };
    let dz = if input.test(0x31, mode::REPEAT) { 1 } else if input.test(0x32, mode::REPEAT) { -1 } else { 0 };
    if dx != 0 || dz != 0 {
        w.sounds.push(Ui::Cursor as u32);
        let grid = l.grid(w);
        let cand = |p: &Placed| {
            let pos = l.pos(&p.piece)?;
            Some(Cand { id: (p.piece.c as u32) << 8 | p.piece.slot as u32, pos, level: p.piece.level, wall: p.piece.furn.wall(), rot: p.piece.rot, size: p.piece.furn.footprint(p.piece.rot), has_tops: !grid.on(p.piece.slot, p.piece.c).is_empty() })
        };
        let all: Vec<Cand> = pieces.iter().filter_map(cand).collect();
        if let Some(cur) = l.selected(&pieces).as_ref().and_then(cand)
            && let Some(id) = pick(&cur, &all, dx, dz, l.view)
        {
            l.sel = Some(((id >> 8) as u8, id as u8));
            // A new selection: every piece back to 0x80 and pulsing for ever (only the selected one shows it).
            set_colors(&mut l, WHITE);
            for p in &pieces {
                w.furniture.pulses.insert((p.piece.c, p.piece.slot), Pulse::new(-1));
            }
        }
    }
    if input.test(0x8b, mode::PRESS) {
        open_win(w, &mut l);
    }
    w.layout = Some(l);
}

const WHITE: u32 = 0x8080_8080;

/// FUN_101d39a0: every piece's colour.
pub fn set_colors(l: &mut Layout, c: u32) {
    l.color_all = c;
    l.colors.clear();
}

/// The colour part of a frame: in mode 4 the see-through walk (FUN_101d4310, before the objects update), then the
/// selected piece's pulse (FUN_101ce900: alpha 0x80 for a mannequin or while dressing, else 0x40; the pieces on it
/// 0x40), then every actor piece takes its colour (MMB pieces' colour method is empty).
fn frame_colors(w: &mut World, l: &mut Layout, ticks: f32) {
    if let Some(d) = &l.dress {
        let key = (d.c, d.slot);
        see_through(w, l, key);
    }
    let sel = l.dress.as_ref().map(|d| (d.c, d.slot)).or(l.sel);
    if let Some(k) = sel {
        let mannequin = l.dress.is_some() || w.inv.slots.get(&k).is_some_and(|&(id, _)| item_type(w, id) == MANNEQUIN);
        let alpha = if mannequin { 0x80u32 } else { 0x40 };
        if let Some(level) = w.furniture.pulses.get_mut(&k).and_then(|p| p.tick(ticks)) {
            l.colors.insert(k, alpha << 24 | level as u32 * 0x1_0101);
            let grid = l.grid(w);
            for (s, c) in grid.on(k.1, k.0) {
                if let Some(level) = w.furniture.pulses.get_mut(&(c, s)).and_then(|p| p.tick(ticks)) {
                    l.colors.insert((c, s), 0x40 << 24 | level as u32 * 0x1_0101);
                }
            }
        }
    }
    apply_colors(w, l);
}

/// Each actor piece's model colour from the layout's.
fn apply_colors(w: &mut World, l: &Layout) {
    for (k, id) in w.furniture.actor_pieces() {
        let c = l.colors.get(&k).copied().unwrap_or(l.color_all);
        if let Some(a) = w.actors.get_mut(&id) {
            a.alpha = (c >> 24) as f32;
            a.rgb = [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32);
        }
    }
}

/// FUN_101d4310: the cells from the mannequin's to the room's middle (10, 13), their floor and top pieces 0x20202020;
/// the piece under the mannequin and the mannequin 0x80808080.
fn see_through(w: &World, l: &mut Layout, key: (u8, u8)) {
    let Some(ext) = w.inv.ext.get(&key) else { return };
    let (x0, level, z0) = (ext[6] as i8 as i32, ext[7] as i8 as i32, ext[8] as i8 as i32);
    let grid = l.grid(w);
    for (x, z) in walk(x0, z0) {
        if let Some(c) = grid.cell(x, z) {
            for (s, cn) in [c.floor, c.top] {
                if (s, cn) != (0, 0) {
                    l.colors.insert((cn, s), 0x2020_2020);
                }
            }
        }
    }
    if level != 0
        && let Some(c) = grid.cell(x0, z0)
        && c.floor != (0, 0)
    {
        l.colors.insert((c.floor.1, c.floor.0), WHITE);
    }
    l.colors.insert(key, WHITE);
}

/// FUN_101d4310's cells, Bresenham as read: the error takes the signed run, so a walk towards lower x or z steps the
/// other axis at every cell once the error first passes the run.
pub fn walk(x0: i32, z0: i32) -> Vec<(i32, i32)> {
    let (dx, dz) = (10 - x0, 13 - z0);
    let (sx, sz) = (if dx >= 0 { 1 } else { -1 }, if dz >= 0 { 1 } else { -1 });
    let (ax, az) = (dx.abs(), dz.abs());
    let (mut x, mut z, mut e) = (x0, z0, 0);
    let mut out = Vec::new();
    if az < ax {
        while x != 10 {
            x += sx;
            e += az;
            if ax < e {
                e -= dx;
                z += sz;
            }
            out.push((x, z));
        }
    } else {
        while z != 13 {
            z += sz;
            e += ax;
            if az < e {
                e -= dz;
                x += sx;
            }
            out.push((x, z));
        }
    }
    out
}

/// FUN_101d5ef0: Enter or a click opens rmlo2 on the selection (rmlo4 for a mannequin), rmlo1 with none (SE 0002).
fn open_win(w: &mut World, l: &mut Layout) {
    w.sounds.push(Ui::Select as u32);
    let pieces = l.pieces(w);
    let kind = match l.selected(&pieces) {
        Some(p) if item_type(w, p.item) == MANNEQUIN => 4,
        Some(_) => 2,
        None => 1,
    };
    l.win = Some((kind, 1));
}

/// An rmlo window's rows: (label, what it does).
#[derive(Clone, Copy, PartialEq)]
enum Row {
    Equip,
    Move,
    Remove,
    Place,
    ClearAll,
    Quit,
}

fn rows(kind: u8) -> &'static [Row] {
    match kind {
        1 => &[Row::Place, Row::ClearAll, Row::Quit],
        2 => &[Row::Move, Row::Remove, Row::Place, Row::ClearAll],
        _ => &[Row::Equip, Row::Move, Row::Remove, Row::Place, Row::ClearAll],
    }
}

fn win_name(kind: u8) -> &'static str {
    match kind {
        1 => "menu    rmlo1",
        2 => "menu    rmlo2",
        _ => "menu    rmlo4",
    }
}

/// FUN_101f44f0 / FUN_101f4540 / FUN_101f45d0: the dim rows. Clear All while Storage holds anything; Clear away when
/// Storage would lose more room (the pieces' storage slots, item DAT +0x10 *(inf)*) than it has free.
fn dim_rows(w: &World, l: &Layout, kind: u8) -> Vec<u16> {
    let stored = w.inv.slots.keys().filter(|(c, s)| *c == 2 && *s != 0).count() as i32;
    let size = w.inv.sizes[2] as i32 - 1;
    let slots = |id: u16| w.item_info.as_ref().and_then(|i| i.item(id)).and_then(|it| it.extra.iter().find(|e| e.0 == 0xd14).map(|e| e.1 as i32)).unwrap_or(0);
    let pieces = l.pieces(w);
    let need = match l.selected(&pieces) {
        Some(p) => {
            let grid = l.grid(w);
            slots(p.item) + grid.on(p.piece.slot, p.piece.c).iter().filter_map(|&(s, c)| pieces.iter().find(|q| (q.piece.slot, q.piece.c) == (s, c))).map(|q| slots(q.item)).sum::<i32>()
        }
        None => 1,
    };
    let mut off = Vec::new();
    for (i, r) in rows(kind).iter().enumerate() {
        let n = i as u16 + 1;
        if (*r == Row::ClearAll && stored > 0) || (*r == Row::Remove && size - stored < need) {
            off.push(n);
        }
    }
    off
}

/// Every key while laying out: the rmlo windows' (Up / Down, Enter 5, Escape 6); else Enter (0x8f) and Escape (0x0f /
/// 0x90) of select mode or of the held piece. The arrows and turns are key functions (`update`).
/// No rmlo window, bankmenu or list up: the keys are the field's.
pub fn in_field(w: &World) -> bool {
    w.layout.as_ref().is_some_and(|l| l.win.is_none() && l.bank.is_none() && l.list.is_none() && l.dress.is_none())
}

pub fn key(w: &mut World, key: egui::Key) -> bool {
    let Some(mut l) = w.layout.take() else { return false };
    if l.list.is_some() {
        list_key(w, &mut l, key);
        w.layout = Some(l);
        return true;
    }
    if let Some(row) = l.bank {
        bank_key(w, &mut l, row, key);
        w.layout = Some(l);
        if std::mem::take(&mut w.layout_leave) {
            leave(w);
        }
        return true;
    }
    let Some((kind, sel)) = l.win else {
        match key {
            egui::Key::Enter if l.held.is_some() => place_held(w, &mut l),
            egui::Key::Escape if l.held.is_some() => cancel_held(w, &mut l),
            egui::Key::Enter => open_win(w, &mut l),
            egui::Key::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                w.layout = Some(l);
                leave(w);
                return true;
            }
            _ => {}
        }
        w.layout = Some(l);
        return true;
    };
    let n = rows(kind).len() as u16;
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let s = if key == egui::Key::ArrowUp { if sel <= 1 { n } else { sel - 1 } } else if sel >= n { 1 } else { sel + 1 };
            l.win = Some((kind, s));
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::Escape => {
            l.win = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        egui::Key::Enter => {
            if dim_rows(w, &l, kind).contains(&sel) {
                w.sounds.push(Ui::Error as u32);
            } else {
                w.sounds.push(Ui::Select as u32);
                l.win = None;
                let pieces = l.pieces(w);
                let cur = l.selected(&pieces);
                match rows(kind)[sel as usize - 1] {
                    Row::Move => {
                        if let Some(p) = cur {
                            hold(w, &mut l, p, false);
                        }
                    }
                    Row::Remove => {
                        if let Some(p) = cur {
                            remove(w, &mut l, p.piece.c, p.piece.slot, true);
                        }
                    }
                    Row::ClearAll => w.confirm = Some((super::Confirm::Layout(Ask::ClearAll), false)),
                    Row::Quit => {
                        w.layout = Some(l);
                        leave(w);
                        return true;
                    }
                    // FUN_101d6c90: bankmenu over the window, which stays open under it.
                    // rmlo4's closes (FUN_101f48e0 case 4).
                    Row::Place => {
                        l.win = (kind != 4).then_some((kind, sel));
                        l.bank = Some(1);
                    }
                    // FUN_101f48e0 case 1: rmlo4 closes, the mannequin is dressed (mode 4).
                    Row::Equip => {
                        if let Some(p) = cur {
                            super::mannequin::open(&mut l, p.piece.c, p.piece.slot);
                            // FUN_101d3910: the mannequin pulses 4 times.
                            w.furniture.pulses.insert((p.piece.c, p.piece.slot), Pulse::new(4));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    w.layout = Some(l);
    true
}

const BANKMENU: &str = "menu    bankmenu";
const INVENTOR: &str = "menu    inventor";

/// bankmenu's dim row (FUN_101d6cd0): Safe 2 until the second floor is unlocked.
fn bank_dim(w: &World) -> Vec<u16> {
    if w.mh_floors { Vec::new() } else { vec![2] }
}

/// bankmenu (FUN_101d6d10): Up / Down, Enter opens the list on Mog Safe (1) or Safe 2 (9) and closes the rmlo window,
/// Escape closes bankmenu only.
fn bank_key(w: &mut World, l: &mut Layout, row: u16, key: egui::Key) {
    use egui::Key as K;
    match key {
        K::ArrowUp | K::ArrowDown => {
            l.bank = Some(3 - row);
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            l.bank = None;
            w.sounds.push(Ui::Cancel as u32);
            if l.garden {
                l.garden = false;
                w.layout_leave = true;
            }
        }
        K::Enter if bank_dim(w).contains(&row) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            (l.bank, l.win, l.garden) = (None, None, false);
            l.list = Some((if row == 1 { 1 } else { 9 }, 0, 0));
        }
        _ => {}
    }
}

/// The list's rows (FUN_101f4270): furnishings and mannequins, flowerpots on 1F only; installed on this floor green,
/// on another not listed, not installed white; a locked row (LockFlg not 0 / 5 / 0x1b) grey.
fn list_rows(w: &World, l: &Layout, c: u8) -> Vec<(crate::inventory::Slot, u32)> {
    let mut out = Vec::new();
    for s in w.inv.viewed(c).into_iter().filter(|s| s.index != 0) {
        let ty = item_type(w, s.id);
        if !(ty == 10 || ty == MANNEQUIN || (ty == 12 && l.floor == 0)) {
            continue;
        }
        let ext = w.inv.ext.get(&(c, s.index)).copied().unwrap_or([0; 24]);
        let tint = if ext[1] & 0x40 == 0 {
            0x8080_8080
        } else if ext[1] & 3 == l.floor {
            0x8050_9050
        } else {
            continue;
        };
        let lock = w.inv.locks.get(&(c, s.index)).copied().unwrap_or(0);
        out.push((s, if matches!(lock, 0 | 5 | 0x1b) { tint } else { 0x8040_4040 }));
    }
    out
}

/// Installed pieces of a type in containers 1 and 9, any floor (FUN_101f4190).
fn installed(w: &World, ty: u16) -> usize {
    w.inv.slots.iter().filter(|((c, _), (id, _))| matches!(c, 1 | 9) && item_type(w, *id) == ty).filter(|(k, _)| w.inv.ext.get(k).is_some_and(|e| e[1] & 0x40 != 0)).count()
}

/// The list's keys (FUN_101f42e0): Enter holds the piece (a new one at its start, 3.5); with 10 flowerpots or 8
/// mannequins installed, any row of that type is refused by the moogle (0x62 / 0x63); Escape back to select mode.
fn list_key(w: &mut World, l: &mut Layout, key: egui::Key) {
    use egui::Key as K;
    let Some((c, mut scroll, mut cursor)) = l.list else { return };
    let rows = list_rows(w, l, c);
    match key {
        K::Escape => {
            l.list = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        K::Enter => {
            let Some((s, tint)) = rows.get(scroll + cursor).cloned() else { return };
            let ty = item_type(w, s.id);
            if tint == 0x8040_4040 {
                w.sounds.push(Ui::Error as u32);
            } else if ty == 12 && installed(w, 12) >= 10 {
                super::moogle::line(w, 0x62);
            } else if ty == MANNEQUIN && installed(w, MANNEQUIN) >= 8 {
                super::moogle::line(w, 0x63);
            } else if let Some(f) = l.table.entry(s.id) {
                w.sounds.push(Ui::Select as u32);
                l.list = None;
                let pieces = l.pieces(w);
                match pieces.iter().copied().find(|p| (p.piece.c, p.piece.slot) == (c, s.index)) {
                    Some(p) => hold(w, l, p, false),
                    None => {
                        let grid = l.grid(w);
                        let sel = l.selected(&pieces).map(|p| p.piece);
                        let mut piece = Piece { c, slot: s.index, furn: f, x: 0, level: 0, z: 0, rot: 0 };
                        start(&grid, &mut piece, l.floor, sel.as_ref());
                        hold(w, l, Placed { piece, item: s.id }, true);
                    }
                }
            }
        }
        _ => {
            if super::items::list_keys(&mut scroll, &mut cursor, rows.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
                l.list = Some((c, scroll, cursor));
            }
        }
    }
}

/// The rmlo window at its layout with its dim rows, bankmenu over it, or the furniture list.
pub fn draw(gu: &mut super::game_ui::GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let Some(l) = &w.layout else { return Vec::new() };
    let mut out = super::mannequin::draw(gu, w, sw, sh);
    if w.modern {
        return out;
    }
    if let Some((kind, sel)) = l.win {
        let name = win_name(kind);
        let off = dim_rows(w, l, kind);
        let focus = w.confirm.is_none() && l.bank.is_none();
        if let Some(win) = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), Some(sel), focus, &off) {
            super::winhits::record(w, name, &win);
            let [x, y, ..] = win.frame;
            gu.disabled_children(name, x, y, &off, focus.then_some(sel));
            out.push(win.frame);
        }
    }
    if let Some(row) = l.bank {
        let off = bank_dim(w);
        if let Some(win) = gu.lobby_window_off(BANKMENU, sw, sh, (0.0, 0.0), Some(row), true, &off) {
            super::winhits::record(w, BANKMENU, &win);
            let [x, y, ..] = win.frame;
            gu.disabled_children(BANKMENU, x, y, &off, Some(row));
            out.push(win.frame);
        }
    }
    if let Some((c, scroll, cursor)) = l.list
        && let Some(info) = w.item_info.clone()
    {
        let rows = list_rows(w, l, c);
        let slots: Vec<_> = rows.iter().map(|r| r.0.clone()).collect();
        let mut list = crate::inventory::ItemList { rows: crate::inventory::build_rows(&slots, info.as_ref(), false), scroll, cursor };
        for (r, (_, tint)) in list.rows.iter_mut().zip(&rows) {
            r.tint = *tint;
        }
        super::items::upload_icons(gu, &info, list.visible().iter().map(|r| r.id));
        let open = gu.gate_layout(INVENTOR, sw, sh, None);
        let frame = gu.cursor_frame();
        let (b, ui, style, font) = gu.parts();
        if let Some(m) = ui.menu(INVENTOR).filter(|_| open) {
            let _ = crate::render::invwin::push_inventory(b, ui, style, font, super::game_ui::GameUi::ATLAS, m, &list, sw, sh, if w.confirm.is_none() { frame } else { usize::MAX });
            out.extend(gu.window_rect(INVENTOR, sw, sh));
        }
    }
    out
}

/// Layout mode's windows as the modern layer draws them: the rmlo window (its name, cursor, dim rows), bankmenu over
/// it, the furniture list (rows, tints, the cursor row).
pub(super) struct Look {
    pub win: Option<(&'static str, u16, Vec<u16>)>,
    pub bank: Option<(u16, Vec<u16>)>,
    pub list: Option<(Vec<crate::inventory::Slot>, Vec<u32>, usize)>,
}

pub(super) const BANK: &str = BANKMENU;

pub(super) fn look(w: &World) -> Option<Look> {
    let l = w.layout.as_ref().filter(|l| l.dress.is_none())?;
    let list = l.list.map(|(c, scroll, cursor)| {
        let rows = list_rows(w, l, c);
        (rows.iter().map(|r| r.0.clone()).collect(), rows.iter().map(|r| r.1).collect(), scroll + cursor)
    });
    Some(Look { win: l.win.map(|(kind, sel)| (win_name(kind), sel, dim_rows(w, l, kind))), bank: l.bank.map(|r| (r, bank_dim(w))), list })
}

/// The mouse's cursor moves: the rmlo window, bankmenu, the furniture list's row. True when it moved.
pub(super) fn set_win(w: &mut World, k: u16) -> bool {
    let Some(l) = w.layout.as_mut().filter(|l| l.bank.is_none() && l.list.is_none()) else { return false };
    let Some((_, sel)) = l.win.as_mut().filter(|(kind, sel)| *sel != k && (1..=rows(*kind).len() as u16).contains(&k)) else { return false };
    *sel = k;
    true
}

pub(super) fn set_bank(w: &mut World, k: u16) -> bool {
    let Some(r) = w.layout.as_mut().and_then(|l| l.bank.as_mut()).filter(|r| **r != k && (1..=2).contains(&k)) else { return false };
    *r = k;
    true
}

pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(l) = w.layout.as_ref() else { return false };
    let Some((c, scroll, cursor)) = l.list else { return false };
    if scroll + cursor == k || k >= list_rows(w, l, c).len() {
        return false;
    }
    let top = scroll.min(k).max((k + 1).saturating_sub(crate::inventory::VISIBLE_ROWS));
    if let Some(l) = w.layout.as_mut() {
        l.list = Some((c, top, k - top));
    }
    true
}

/// The mouse (winhits.rs): an rmlo window or bankmenu as the generic window; the furniture list's rows as the item
/// list (hover moves the row, a click on it is Enter, a right click Escape). The dressing windows have their own.
pub fn pointer(w: &mut World, gu: &super::game_ui::GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::generic;
    let Some(l) = &w.layout else { return };
    // The modern layer draws these and takes their mouse.
    if l.dress.is_some() || w.modern {
        return;
    }
    let keys = |w: &mut World, _: &super::game_ui::GameUi, k: egui::Key| {
        key(w, k);
    };
    if let Some((c, scroll, cursor)) = l.list {
        let n = list_rows(w, l, c).len();
        let Some(r) = gu.window_rect(INVENTOR, sw, sh) else { return };
        let row = ((p.1 - r[1] - 4.0) / crate::inventory::ROW_PITCH).floor();
        let on = p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
        if right && on {
            key(w, egui::Key::Escape);
            return;
        }
        if !on || row < 0.0 || row as usize >= crate::inventory::VISIBLE_ROWS || scroll + row as usize >= n {
            return;
        }
        gu.hover.set(true);
        let row = row as usize;
        if row != cursor && (moved || clicked) {
            if let Some(l) = w.layout.as_mut() {
                l.list = Some((c, scroll, row));
            }
        } else if clicked {
            key(w, egui::Key::Enter);
        }
    } else if let Some(row) = l.bank {
        generic(w, gu, BANKMENU, row, 1..=2, p, moved, clicked, right, |w, id| {
            if let Some(l) = w.layout.as_mut() {
                l.bank = Some(id);
            }
        }, keys);
    } else if let Some((kind, sel)) = l.win {
        let n = rows(kind).len() as u16;
        generic(w, gu, win_name(kind), sel, 1..=n, p, moved, clicked, right, |w, id| {
            if let Some(l) = w.layout.as_mut() {
                l.win = Some((kind, id));
            }
        }, keys);
    }
}

/// The list's cursor item, for the item info window.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let l = w.layout.as_ref()?;
    let (c, scroll, cursor) = l.list?;
    let (s, _) = list_rows(w, l, c).get(scroll + cursor).cloned()?;
    Some((s.id, w.inv.ext.get(&(c, s.index)).copied()))
}

/// The context: 0x3e while a window or a prompt is up, else 0x42.
pub fn context(w: &World) -> u16 {
    if w.layout.as_ref().is_some_and(|l| l.win.is_some() || l.bank.is_some() || l.list.is_some() || l.dress.is_some()) || w.confirm.is_some() { 0x3e } else { 0x42 }
}

/// The layout camera each frame (FUN_101d4080, the dressing one in mode 4) into the world camera; true while laying
/// out.
pub fn camera(w: &mut World, ticks: f32) -> bool {
    if super::mannequin::camera(w, ticks) {
        return true;
    }
    let Some(l) = &w.layout else { return false };
    let (view, floor, sel) = (l.view, l.floor, l.sel_pos(w));
    let (mut eye, mut look) = (w.cam.eye, w.cam.look);
    camera_step(&mut eye, &mut look, view, floor, sel, ticks);
    w.cam = super::target::LockCam { eye, look };
    true
}

/// The cursor (FUN_101d3fc0): anc_l at the selected piece's origin, a frame every 3 ticks.
pub fn draw_cursor(gu: &mut super::game_ui::GameUi, w: &World, sw: f32, sh: f32, view_proj: glam::Mat4) {
    // Mode 1 only (FUN_101d5680).
    let Some(l) = w.layout.as_ref().filter(|l| l.dress.is_none()) else { return };
    let Some(p) = l.sel_pos(w) else { return };
    let clip = view_proj * p.extend(1.0);
    let (x, y) = if clip.w > 0.0 { ((clip.x / clip.w + 1.0) / 2.0 * sw, (1.0 - clip.y / clip.w) / 2.0 * sh) } else { (100.0, 100.0) };
    let frame = (l.ticks / 3.0) as usize % gu.lib_len("anc     anc_l").max(1);
    gu.sprite("anc     anc_l", frame, x.trunc().max(0.0), y.trunc().max(0.0), 0x8080_8080);
}

/// The overlay's colours (0xAABBGGRR as FUN_10177b40 takes them).
const BLUE: u32 = 0x80ff_6060;
const GREEN: u32 = 0x8060_ff60;
const RED: u32 = 0x8080_80ff;
const GREY: u32 = 0x8060_6060;

/// PTR_DAT_1037e248[floor * 4 + turn]: a wall slot's outline corners.
#[rustfmt::skip]
const WALL_OUTLINE: [[[f32; 3]; 4]; 8] = [
    [[-1.65, -2.0, -0.2], [-1.65, 2.0, -0.2], [1.65, 2.0, -0.2], [1.65, -2.0, -0.2]],
    [[-0.2, -2.0, -1.65], [-0.2, -2.0, 1.65], [-0.2, 2.0, 1.65], [-0.2, 2.0, -1.65]],
    [[-1.65, -2.0, 0.2], [-1.65, 2.0, 0.2], [1.65, 2.0, 0.2], [1.65, -2.0, 0.2]],
    [[0.2, -2.0, -1.65], [0.2, -2.0, 1.65], [0.2, 2.0, 1.65], [0.2, 2.0, -1.65]],
    [[-0.5, -0.3, -0.2], [-0.5, 0.3, -0.2], [0.5, 0.3, -0.2], [0.5, -0.3, -0.2]],
    [[-0.2, -0.3, -0.5], [-0.2, -0.3, 0.5], [-0.2, 0.3, 0.5], [-0.2, 0.3, -0.5]],
    [[-0.5, -0.3, 0.2], [-0.5, 0.3, 0.2], [0.5, 0.3, 0.2], [0.5, -0.3, 0.2]],
    [[0.2, -0.3, -0.5], [0.2, -0.3, 0.5], [0.2, 0.3, 0.5], [0.2, 0.3, -0.5]],
];

/// A closed outline (D3DPT_LINESTRIP of 5) as line-list pairs.
fn outline(out: &mut Vec<crate::render::LineVertex>, corners: [Vec3; 4], c: u32) {
    let color = [c & 0xff, c >> 8 & 0xff, c >> 16 & 0xff, c >> 24].map(|b| b as f32 / 255.0);
    for k in 0..4 {
        for v in [corners[k], corners[(k + 1) % 4]] {
            out.push(crate::render::LineVertex { pos: v.to_array(), color });
        }
    }
}

/// FUN_101cf350 / FUN_101cf410: a cell's outline over its surface (blue 0.04 up, the others 0.06), inset by `i`.
fn cell_outline(out: &mut Vec<crate::render::LineVertex>, grid: &Grid, floor: u8, x: i32, z: i32, c: u32, i: f32) {
    let o = super::furniture::ORIGIN[(floor != 0) as usize];
    let h = grid.cell(x, z).map_or(0, |c| c.height);
    let y = -(h as f32) * 0.01 - if c == BLUE { 0.04 } else { 0.06 };
    let (x0, z0) = ((x as f32 - o[0]) * 0.6 + i, (z as f32 - o[2]) * 0.6 + i);
    let s = 0.6 - 2.0 * i;
    outline(out, [Vec3::new(x0, y, z0), Vec3::new(x0 + s, y, z0), Vec3::new(x0 + s, y, z0 + s), Vec3::new(x0, y, z0 + s)], c);
}

/// The overlay while a piece is held (FUN_101cf110, drawn with ZENABLE off): a wall piece shows every wall slot blue
/// and its own red-pink; a floor piece the free floor cells blue, for a stackable one the free tops green, and its
/// footprint red-pink where it fits (on the floor, or on a top) or grey.
pub fn overlay(w: &World) -> Vec<crate::render::LineVertex> {
    let mut out = Vec::new();
    let Some((l, h)) = w.layout.as_ref().and_then(|l| Some((l, l.held.as_ref()?))) else { return out };
    let p = h.piece.piece;
    if p.furn.wall() {
        let n = if l.floor == 0 { 5 } else { WALLS as u8 };
        for s in 0..n {
            let Some(at) = super::furniture::wall_pos(l.map, l.floor, s) else { continue };
            let corners = WALL_OUTLINE[(l.floor != 0) as usize * 4 + wall_rot(l.floor, s) as usize];
            let held = s == p.x as u8;
            for scale in if held { &[1.0, 0.9][..] } else { &[1.0][..] } {
                outline(&mut out, corners.map(|c| at + Vec3::from(c) * *scale), if held { RED } else { BLUE });
            }
        }
        return out;
    }
    let grid = l.grid(w);
    let [x0, x1, z0, z1] = BOUNDS[(l.floor != 0) as usize];
    let cells = || (x0..x1).flat_map(move |x| (z0..z1).map(move |z| (x, z)));
    for (x, z) in cells().filter(|&(x, z)| grid.cell(x, z).is_some_and(|c| c.flags & 1 != 0)) {
        cell_outline(&mut out, &grid, l.floor, x, z, BLUE, 0.0);
    }
    if p.stackable() {
        for (x, z) in cells().filter(|&(x, z)| grid.cell(x, z).is_some_and(|c| c.flags & 2 != 0 && c.flags & 1 == 0)) {
            cell_outline(&mut out, &grid, l.floor, x, z, GREEN, 0.0);
        }
    }
    let fits = grid.fits(&Piece { level: 0, ..p }) || (p.stackable() && grid.fits(&Piece { level: 1, ..p }));
    for (x, z) in p.cells() {
        cell_outline(&mut out, &grid, l.floor, x, z, if fits { RED } else { GREY }, 0.0);
        if fits {
            cell_outline(&mut out, &grid, l.floor, x, z, RED, 0.05);
        }
    }
    out
}

/// Title slot 0 "Layout" (0xd 4), help slot 0 "Select furniture to place in your room." (0xe 0xe); an rmlo window's
/// row help in slot 1.
pub fn bars(gu: &super::game_ui::GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(l) = &w.layout else { return };
    bars.title[0] = gu.title_msg(4).unwrap_or_default();
    bars.help[0] = gu.menu_msg(0xe, "");
    if let Some(row) = l.bank {
        let [help, title] = gu.focus_text(BANKMENU, row);
        bars.help[1] = help.unwrap_or_default();
        bars.title[1] = title.unwrap_or_default();
    } else if let Some((kind, sel)) = l.win {
        let [help, _] = gu.focus_text(win_name(kind), sel);
        bars.help[1] = help.unwrap_or_default();
    }
    super::mannequin::bars(gu, w, bars);
}

/// Grid size in cells: 20 columns (x) by 26 rows (z).
pub const COLS: usize = 20;
pub const ROWS: usize = 26;
/// Wall records (2F has 92 slots; 1F uses 0..4).
pub const WALLS: usize = 92;

/// One cell (8 bytes): the floor piece and the piece on top of it, free bits (1 floor, 2 top, 0x80 blocked) and the
/// surface height in 0.01 units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub floor: (u8, u8),
    pub top: (u8, u8),
    pub flags: u8,
    pub height: u16,
}

/// A wall slot's record: the piece and its state (4 free, 0 taken).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub piece: (u8, u8),
    pub state: u8,
}

/// A piece in the grid's terms (object +0x10 container, +5 slot, +0x14 x or wall slot, +0x18 level, +0x1c z,
/// +0x20 rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub c: u8,
    pub slot: u8,
    pub furn: Furn,
    pub x: i8,
    pub level: i8,
    pub z: i8,
    pub rot: u8,
}

impl Piece {
    fn key(&self) -> (u8, u8) {
        (self.slot, self.c)
    }

    fn cells(&self) -> impl Iterator<Item = (i32, i32)> + use<> {
        let (w, d) = self.furn.footprint(self.rot);
        let (x, z) = (self.x as i32, self.z as i32);
        (x..x + w as i32).flat_map(move |cx| (z..z + d as i32).map(move |cz| (cx, cz)))
    }

    /// FUN_101cebe0 flag 2: things can stand on it.
    pub fn surface(&self) -> bool {
        self.furn.flags & 2 != 0
    }

    /// Flag 4: it can stand on a surface.
    pub fn stackable(&self) -> bool {
        self.furn.flags & 4 != 0
    }
}

fn inside(x: i32, z: i32) -> bool {
    (0..COLS as i32).contains(&x) && (0..ROWS as i32).contains(&z)
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub cells: Vec<Cell>,
    pub walls: [Wall; WALLS],
}

impl Grid {
    /// FUN_101cecf0: every cell free (flags 3); on 1F the walkway x 6..13, z 0..17 is blocked (0x80).
    pub fn new(floor: u8) -> Self {
        let mut cells = vec![Cell { floor: (0, 0), top: (0, 0), flags: 3, height: 0 }; COLS * ROWS];
        if floor == 0 {
            for x in 6..=13 {
                for z in 0..=17 {
                    cells[x * ROWS + z].flags = 0x80;
                }
            }
        }
        Grid { cells, walls: [Wall { piece: (0, 0), state: 4 }; WALLS] }
    }

    pub fn cell(&self, x: i32, z: i32) -> Option<&Cell> {
        inside(x, z).then(|| &self.cells[x as usize * ROWS + z as usize])
    }

    fn cell_mut(&mut self, x: i32, z: i32) -> Option<&mut Cell> {
        inside(x, z).then(|| &mut self.cells[x as usize * ROWS + z as usize])
    }

    /// FUN_101cf0a0 / FUN_101cefd0: a wall slot free; on the floor every footprint cell floor-free; on a level every
    /// cell top-free over one and the same surface piece. A cell past the grid does not fit.
    pub fn fits(&self, p: &Piece) -> bool {
        if p.furn.wall() {
            return self.walls.get(p.x as u8 as usize).is_some_and(|w| w.state == 4);
        }
        let mask = if p.level > 0 { 2 } else { 1 };
        let mut under = None;
        for (x, z) in p.cells() {
            let Some(c) = self.cell(x, z) else { return false };
            if c.flags & mask == 0 || (mask == 2 && (c.flags & 1 != 0 || under.is_some_and(|u| u != c.floor))) {
                return false;
            }
            under = Some(c.floor);
        }
        true
    }

    /// FUN_101ced80: takes the piece's cells or wall slot when it fits; returns whether it did (object +0x2f).
    pub fn occupy(&mut self, p: &Piece) -> bool {
        if !self.fits(p) {
            return false;
        }
        if p.furn.wall() {
            if let Some(w) = self.walls.get_mut(p.x as u8 as usize) {
                *w = Wall { piece: p.key(), state: 0 };
            }
            return true;
        }
        let (surface, height) = (p.surface(), p.furn.height);
        for (x, z) in p.cells() {
            let Some(c) = self.cell_mut(x, z) else { continue };
            if p.level == 0 {
                c.floor = p.key();
                if surface {
                    c.height = height;
                    c.flags &= !1;
                } else {
                    c.flags = 0;
                }
            } else {
                c.top = p.key();
                c.flags &= !2;
            }
        }
        true
    }

    /// FUN_101ceef0: gives back what the piece holds.
    pub fn free(&mut self, p: &Piece) {
        if p.furn.wall() {
            for w in self.walls.iter_mut().filter(|w| w.piece == p.key()) {
                *w = Wall { piece: (0, 0), state: 4 };
            }
            return;
        }
        for c in self.cells.iter_mut() {
            if p.level == 0 && c.floor == p.key() {
                (c.floor, c.flags, c.height) = ((0, 0), 3, 0);
            } else if p.level > 0 && c.top == p.key() {
                c.top = (0, 0);
                c.flags = if c.floor != (0, 0) { c.flags | 2 } else { 3 };
            }
        }
    }

    /// FUN_101cec50: the pieces standing on a floor piece (slot, container), each once, at most 0x20.
    pub fn on(&self, slot: u8, c: u8) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        for cell in self.cells.iter().filter(|x| x.floor == (slot, c) && x.top != (0, 0)) {
            if !out.contains(&cell.top) && out.len() < 0x20 {
                out.push(cell.top);
            }
        }
        out
    }
}

/// DAT_1037d7d4[floor]: the move bounds {x0, x1, z0, z1}.
const BOUNDS: [[i32; 4]; 2] = [[0, 20, 0, 24], [0, 20, 0, 26]];

/// FUN_101cd060's flip table and the direction offsets of FUN_101cd0a0 (0 +z, 1 +x, 2 -z, 3 -x).
const FLIP: [u8; 4] = [2, 3, 0, 1];
const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

/// The wall slot moves (PTR_DAT_10337de4) and turns (PTR_DAT_10337dec): 1F, then 2F.
const MOVE_1F: [[u8; 4]; 5] = [[0, 0, 1, 4], [0, 1, 1, 2], [3, 1, 2, 2], [4, 0, 2, 3], [4, 0, 3, 4]];
const ROT_1F: [u8; 5] = [1, 1, 3, 3, 3];
#[rustfmt::skip]
const MOVE_2F: [[u8; 4]; 92] = [
    [4,1,84,0], [5,2,85,0], [6,3,86,1], [7,51,87,2], [8,5,0,4], [9,6,1,4], [10,7,2,5], [11,55,3,6], [12,9,4,8], [13,10,5,8],
    [14,11,6,9], [15,59,7,10], [16,13,8,12], [17,14,9,12], [18,15,10,13], [19,63,11,14], [20,17,12,16], [21,18,13,16],
    [22,19,14,17], [23,67,15,18], [24,21,16,20], [25,22,17,20], [26,23,18,21], [27,71,19,22], [28,25,20,24], [29,26,21,24],
    [30,27,22,25], [31,75,23,26], [32,29,24,28], [33,30,25,28], [34,31,26,29], [35,79,27,30], [36,33,28,32], [37,34,29,32],
    [38,35,30,33], [39,83,31,34], [36,40,37,32], [36,41,38,33], [37,42,39,34], [38,43,87,35], [40,44,41,36], [40,45,42,37],
    [41,46,43,38], [42,47,43,39], [44,80,45,40], [44,81,46,41], [45,82,47,42], [46,83,91,43], [52,48,88,49], [53,48,89,50],
    [54,49,90,51], [55,50,91,3], [56,52,48,53], [57,52,49,54], [58,53,50,55], [59,54,51,7], [60,56,52,57], [61,56,53,58],
    [62,57,54,59], [63,58,55,11], [64,60,56,61], [65,60,57,62], [66,61,58,63], [67,62,59,15], [68,64,60,65], [69,64,61,66],
    [70,65,62,67], [71,66,63,19], [72,68,64,69], [73,68,65,70], [74,69,66,71], [75,70,67,23], [76,72,68,73], [77,72,69,74],
    [78,73,70,75], [79,74,71,27], [80,76,72,77], [81,76,73,78], [82,77,74,79], [83,78,75,31], [44,80,76,81], [45,80,77,82],
    [46,81,78,83], [47,82,79,35], [85,88,84,0], [86,89,84,1], [87,90,85,2], [39,91,86,3], [89,48,88,84], [90,49,88,85],
    [91,50,89,86], [47,51,90,87],
];

/// A wall slot's turn on a floor.
pub fn wall_rot(floor: u8, slot: u8) -> u8 {
    if floor == 0 {
        ROT_1F.get(slot as usize).copied().unwrap_or(0)
    } else {
        match slot {
            0..=35 => 3,
            36..=47 => 0,
            48..=83 => 1,
            _ => 2,
        }
    }
}

/// FUN_101cd060: with /layoutctrl type 1 (config 0xc1 != 1) the arrows flip while the camera looks toward -z.
pub fn remap(dir: u8, ctrl: u32, eye: Vec3, look: Vec3) -> u8 {
    if ctrl != 1 && look.z <= eye.z { FLIP[dir as usize & 3] } else { dir & 3 }
}

/// The level a stackable piece takes over the cells under it (FUN_101cec20: the last raised cell, height / 10).
fn level_over(grid: &Grid, p: &Piece) -> i8 {
    let mut level = 0;
    for (x, z) in p.cells() {
        if let Some(c) = grid.cell(x, z).filter(|c| c.height > 0) {
            level = (c.height / 10) as i8;
        }
    }
    level
}

/// FUN_101cd150: moves a floor piece by (dx, dz) inside the floor's bounds; false when nothing changed. The pieces
/// on a surface are moved by the caller with the same delta (FUN_101cd290).
pub fn shift(grid: &Grid, p: &mut Piece, floor: u8, dx: i32, dz: i32) -> bool {
    let [x0, x1, z0, z1] = BOUNDS[(floor != 0) as usize];
    let (w, d) = p.furn.footprint(p.rot);
    let (mut x, mut z) = ((p.x as i32 + dx).max(x0), (p.z as i32 + dz).max(z0));
    if x + w as i32 >= x1 {
        x = x1 - w as i32;
    }
    if z + d as i32 >= z1 {
        z = z1 - d as i32;
    }
    if (x, z) == (p.x as i32, p.z as i32) {
        return false;
    }
    (p.x, p.z) = (x as i8, z as i8);
    p.level = if p.stackable() { level_over(grid, p) } else { 0 };
    true
}

/// FUN_101cd0a0: one arrow on a held piece: a floor piece by a cell (clamped), a wall piece to the next slot of the
/// table (always "moved"). Returns the delta for the pieces on a surface, or None when nothing moved.
pub fn step(grid: &Grid, p: &mut Piece, floor: u8, dir: u8) -> Option<(i32, i32)> {
    if p.furn.wall() {
        let s = p.x as u8 as usize;
        let next = if floor == 0 { MOVE_1F.get(s).map(|m| m[dir as usize & 3]) } else { MOVE_2F.get(s).map(|m| m[dir as usize & 3]) };
        let s = next.unwrap_or(s as u8);
        (p.x, p.level, p.z, p.rot) = (s as i8, 0, 0, wall_rot(floor, s));
        return Some((0, 0));
    }
    let (ox, oz) = (p.x, p.z);
    let (dx, dz) = DIRS[dir as usize & 3];
    shift(grid, p, floor, dx, dz).then(|| ((p.x - ox) as i32, (p.z - oz) as i32))
}

/// Num- (0x8d, -1) / 0x8e (+1): a floor piece turns about its centre (integer halves) and is clamped; a wall piece's
/// rotation changes but nothing else (retail quirk). Returns the shift for the pieces on it.
pub fn rotate(grid: &Grid, p: &mut Piece, floor: u8, by: i8) -> (i32, i32) {
    let (w0, d0) = p.furn.footprint(p.rot);
    p.rot = (p.rot as i8 + by).rem_euclid(4) as u8;
    if p.furn.wall() {
        return (0, 0);
    }
    let (w1, d1) = p.furn.footprint(p.rot);
    let (ox, oz) = (p.x, p.z);
    shift(grid, p, floor, w0 as i32 / 2 - w1 as i32 / 2, d0 as i32 / 2 - d1 as i32 / 2);
    if !p.stackable() {
        p.level = 0;
    }
    ((p.x - ox) as i32, (p.z - oz) as i32)
}

/// FUN_101cd3b0: a new piece's start: wall slot 0 (its turn); else (9, 12), or the selected floor piece's place,
/// centred on that cell.
pub fn start(grid: &Grid, p: &mut Piece, floor: u8, sel: Option<&Piece>) {
    if p.furn.wall() {
        (p.x, p.level, p.z, p.rot) = (0, 0, 0, wall_rot(floor, 0));
        return;
    }
    (p.x, p.level, p.z, p.rot) = (9, 0, 12, 0);
    if let Some(s) = sel.filter(|s| !s.furn.wall()) {
        (p.x, p.level, p.z) = (s.x, s.level, s.z);
    }
    let (w, d) = p.furn.footprint(0);
    shift(grid, p, floor, -(w as i32 / 2), -(d as i32 / 2));
    if p.stackable() {
        p.level = level_over(grid, p);
    }
}

/// A selection candidate for the arrows: its id, world position, level, and for the current one its wall flag,
/// turn and footprint, and whether pieces stand on it.
#[derive(Clone, Copy, Debug)]
pub struct Cand {
    pub id: u32,
    pub pos: Vec3,
    pub level: i8,
    pub wall: bool,
    pub rot: u8,
    pub size: (u8, u8),
    pub has_tops: bool,
}

/// FUN_101d5710: the next piece in an arrow's direction (dx -1 / +1, dz -1 / +1; view 2 negates both).
pub fn pick(cur: &Cand, others: &[Cand], dx: i32, dz: i32, view: u8) -> Option<u32> {
    if dx == 0 && dz == 0 {
        return None;
    }
    let (dx, dz) = if view == 2 { (-dx, -dz) } else { (dx, dz) };
    let mut p = Vec3::new(cur.pos.x, 0.0, cur.pos.z);
    if cur.wall {
        let d = -cur.pos.y;
        match cur.rot & 3 {
            0 => p.z += d,
            1 => p.x += d,
            2 => p.z -= d,
            _ => p.x -= d,
        }
    }
    let (ex, ez) = if cur.has_tops { (0.0, 0.0) } else { (cur.size.0 as f32 * 0.3, cur.size.1 as f32 * 0.3) };
    let mut best: Option<(f32, u32)> = None;
    let mut floor_best: Option<(f32, u32)> = None;
    for c in others.iter().filter(|c| c.id != cur.id) {
        let (rx, rz) = (c.pos.x - p.x, c.pos.z - p.z);
        if cur.level == 0 && rx.abs() + rz.abs() < 0.1 {
            return Some(c.id);
        }
        let ok_x = match dx {
            1 => rx > ex && rx.abs() >= rz.abs(),
            -1 => rx < -ex && rx.abs() >= rz.abs(),
            _ => true,
        };
        let ok_z = match dz {
            1 => rz > ez && rz.abs() >= rx.abs(),
            -1 => rz < -ez && rz.abs() >= rx.abs(),
            _ => true,
        };
        if !(ok_x && ok_z) {
            continue;
        }
        let d2 = rx * rx + rz * rz;
        if best.is_none_or(|b| d2 < b.0) {
            best = Some((d2, c.id));
        }
        if c.level == 0 && floor_best.is_none_or(|b| d2 < b.0) {
            floor_best = Some((d2, c.id));
        }
    }
    match (best, floor_best) {
        (Some(b), Some(f)) if (f.0 - b.0).abs() < 0.1 => Some(f.1),
        (b, _) => b.map(|b| b.1),
    }
}

/// FUN_101d4080's target eye and look for a view (0 top-down, 1 from the door side, 2 from the far side) and the
/// selected piece's position.
pub fn camera_target(view: u8, floor: u8, sel: Option<Vec3>) -> (Vec3, Vec3) {
    let b = if floor == 0 { [4.0, 7.0, 5.0] } else { [4.0, 9.0, 6.0] };
    let mut l = sel.unwrap_or(Vec3::ZERO);
    l.y = match sel {
        Some(s) if view != 0 => s.y - 0.3,
        _ => -0.3,
    };
    l.x = l.x.clamp(-b[0], b[0]);
    l.z = l.z.clamp(-b[2], b[2]);
    let e = match view {
        0 => Vec3::new(l.x, -5.0, l.z - 0.1),
        1 => Vec3::new(l.x * 0.7, -4.5, (l.z - 4.0).max(-5.2)),
        _ => Vec3::new(l.x * 0.7, -4.5, (l.z + 4.0).min(5.2)),
    };
    (e, l)
}

/// The layout cameras' ease: 0.3 of the way a retail frame *(inf: 2 ticks)*.
pub fn ease(ticks: f32) -> f32 {
    1.0 - 0.7f32.powf(ticks / 2.0)
}

/// One frame of the layout camera: both points eased; view 0 then keeps the eye 7 (2F 9) from the look.
pub fn camera_step(eye: &mut Vec3, look: &mut Vec3, view: u8, floor: u8, sel: Option<Vec3>, ticks: f32) {
    let (e, l) = camera_target(view, floor, sel);
    let k = ease(ticks);
    *look += (l - *look) * k;
    *eye += (e - *eye) * k;
    if view == 0 {
        let dist = if floor == 0 { 7.0 } else { 9.0 };
        *eye = *look + (*eye - *look).normalize_or(Vec3::NEG_Y) * dist;
    }
}

/// c2s 0x0FA's body after the header (FUN_100f0cc0): item, slot, container, floor, x, level, z, rotation, 3 zero.
pub fn layout_body(item: u16, slot: u8, c: u8, floor: u8, x: i8, level: i8, z: i8, rot: u8) -> [u8; 12] {
    let i = item.to_le_bytes();
    [i[0], i[1], slot, c, floor, x as u8, level as u8, z as u8, rot, 0, 0, 0]
}

/// c2s 0x0FB's body (FUN_100f0d40): item, slot, container.
pub fn bankin_body(item: u16, slot: u8, c: u8) -> [u8; 4] {
    let i = item.to_le_bytes();
    [i[0], i[1], slot, c]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(flags: u16, size: [u8; 2], height: u16, slot: u8, x: i8, z: i8) -> Piece {
        Piece { c: 1, slot, furn: Furn { flags, model: 1, size, height }, x, level: 0, z, rot: 0 }
    }

    /// Spec 7.8: the 1F walkway, a desk's surface, a pot on it and across two tables, freeing, a taken wall slot.
    #[test]
    fn grid() {
        let g = Grid::new(0);
        assert_eq!(g.cells.iter().filter(|c| c.flags == 0x80).count(), 144);
        assert_eq!(Grid::new(1).cells.iter().filter(|c| c.flags == 0x80).count(), 0);
        let mut g = Grid::new(1);
        let desk = piece(2, [3, 3], 50, 1, 0, 0);
        assert!(g.occupy(&desk));
        assert_eq!((g.cell(1, 1).unwrap().flags, g.cell(1, 1).unwrap().height), (2, 50));
        let mut pot = piece(0xc, [1, 1], 100, 2, 1, 1);
        pot.level = 5;
        assert!(g.fits(&pot));
        pot.x = 3;
        assert!(!g.fits(&pot));
        let desk2 = piece(2, [3, 3], 50, 3, 3, 0);
        assert!(g.occupy(&desk2));
        let mut wide = piece(0xc, [2, 1], 100, 4, 2, 1);
        wide.level = 5;
        assert!(!g.fits(&wide));
        pot.x = 1;
        assert!(g.occupy(&pot));
        assert_eq!(g.on(1, 1), vec![(2, 1)]);
        g.free(&pot);
        assert_eq!(g.cell(1, 1).unwrap().flags, 2);
        g.free(&desk);
        assert_eq!(g.cell(1, 1).unwrap().flags, 3);
        let flag = piece(1, [1, 1], 60, 5, 3, 0);
        assert!(g.occupy(&flag));
        assert!(!g.fits(&piece(1, [1, 1], 60, 6, 3, 0)));
    }

    /// Spec 7.8: a bed pushed into the right wall, a stackable pot over the desk and off it.
    #[test]
    fn moves() {
        let g = Grid::new(0);
        let mut bed = piece(0, [4, 6], 70, 1, 15, 18);
        assert_eq!(step(&g, &mut bed, 0, 1), Some((1, 0)));
        assert_eq!(bed.x, 16);
        assert_eq!(step(&g, &mut bed, 0, 1), None);
        let mut g = Grid::new(1);
        let mut far = piece(0, [1, 3], 70, 2, 0, 22);
        assert_eq!(step(&g, &mut far, 1, 0), Some((0, 1)));
        assert_eq!(step(&g, &mut far, 1, 0), None);
        assert_eq!(far.z, 23);
        assert!(g.occupy(&piece(2, [3, 3], 50, 3, 0, 0)));
        let mut pot = piece(0xc, [1, 1], 100, 4, 3, 1);
        assert!(step(&g, &mut pot, 1, 3).is_some());
        assert_eq!((pot.x, pot.level), (2, 5));
        assert!(step(&g, &mut pot, 1, 1).is_some());
        assert_eq!((pot.x, pot.level), (3, 0));
    }

    /// Spec 7.8: a bed turned at (2, 2); wall slots on 1F and 2F.
    #[test]
    fn rotate_and_walls() {
        let g = Grid::new(1);
        let mut bed = piece(0, [4, 6], 70, 1, 2, 2);
        rotate(&g, &mut bed, 1, -1);
        assert_eq!((bed.rot, bed.x, bed.z), (3, 1, 3));
        let mut flag = piece(1, [1, 1], 60, 2, 3, 0);
        assert!(step(&g, &mut flag, 0, 1).is_some());
        assert_eq!((flag.x, flag.rot), (0, 1));
        assert!(step(&g, &mut flag, 0, 0).is_some());
        assert_eq!(flag.x, 0);
        flag.x = 39;
        step(&g, &mut flag, 1, 2);
        assert_eq!((flag.x, flag.rot), (87, 2));
    }

    /// The live test room: the bed held, moved right and up and turned, must fit beside the desk, pot, lamp and flag.
    #[test]
    fn live_room() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let t = FurnTable::load(&index).unwrap();
        let mk = |slot, item: u16, x, level, z, rot| Piece { c: 1, slot, furn: t.entry(item).unwrap(), x, level, z, rot };
        let others = [mk(2, 21, 16, 0, 4, 0), mk(3, 216, 17, 5, 5, 0), mk(4, 207, 0, 0, 20, 0), mk(6, 129, 3, 0, 0, 3)];
        let mut g = Grid::new(0);
        for p in others.iter().filter(|p| p.level == 0) {
            assert!(g.occupy(p), "{p:?}");
        }
        for p in others.iter().filter(|p| p.level > 0) {
            assert!(g.occupy(p), "{p:?}");
        }
        let mut bed = mk(1, 2, 0, 0, 0, 0);
        assert!(g.fits(&bed));
        step(&g, &mut bed, 0, 1);
        step(&g, &mut bed, 0, 0);
        rotate(&g, &mut bed, 0, -1);
        assert_eq!((bed.x, bed.z, bed.rot), (0, 2, 3));
        assert!(g.fits(&bed), "{bed:?}");
    }

    #[test]
    fn remap_dirs() {
        let (eye, look) = (Vec3::new(0.0, -4.5, 4.0), Vec3::ZERO);
        assert_eq!((0..4).map(|d| remap(d, 0, eye, look)).collect::<Vec<_>>(), [2, 3, 0, 1]);
        assert_eq!((0..4).map(|d| remap(d, 1, eye, look)).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(remap(0, 0, Vec3::new(0.0, -4.5, -4.0), look), 0);
    }

    /// Spec 7.8: pieces at (0, 0) current, (2, 0.5), (0.5, 2), (-3, 0); a pot at the table's centre; view 2 swaps.
    #[test]
    fn picks() {
        let c = |id, x, z, level| Cand { id, pos: Vec3::new(x, 0.0, z), level, wall: false, rot: 0, size: (1, 1), has_tops: false };
        let cur = c(1, 0.0, 0.0, 0);
        let others = [c(2, 2.0, 0.5, 0), c(3, 0.5, 2.0, 0), c(4, -3.0, 0.0, 0)];
        assert_eq!(pick(&cur, &others, 1, 0, 0), Some(2));
        assert_eq!(pick(&cur, &others, 0, 1, 0), Some(3));
        assert_eq!(pick(&cur, &others, -1, 0, 0), Some(4));
        assert_eq!(pick(&cur, &others, 1, 0, 2), Some(4));
        let table = Cand { size: (3, 3), ..c(5, 0.0, 0.0, 0) };
        assert_eq!(pick(&table, &[c(6, 0.0, 0.0, 5), c(2, 2.0, 0.5, 0)], 1, 0, 0), Some(6));
    }

    #[test]
    fn camera() {
        let sel = Some(Vec3::new(5.0, -0.5, 6.0));
        let (e, l) = camera_target(0, 0, sel);
        assert_eq!((l, e), (Vec3::new(4.0, -0.3, 5.0), Vec3::new(4.0, -5.0, 4.9)));
        let (e, l) = camera_target(1, 0, sel);
        assert_eq!((l, e), (Vec3::new(4.0, -0.8, 5.0), Vec3::new(2.8, -4.5, 1.0)));
        let (e, _) = camera_target(2, 0, sel);
        assert_eq!(e, Vec3::new(2.8, -4.5, 5.2));
    }

    #[test]
    fn packets() {
        assert_eq!(layout_body(0x1234, 5, 1, 0, 7, 5, 9, 3), [0x34, 0x12, 5, 1, 0, 7, 5, 9, 3, 0, 0, 0]);
        assert_eq!(bankin_body(0x1234, 5, 9), [0x34, 0x12, 5, 9]);
    }

    #[test]
    fn pulses() {
        let mut p = Pulse::new(2);
        assert_eq!(p.tick(1.0), Some(0x78));
        // Down to 0x18 (13 ticks), 0x40, up through 0x80 (one counted), 0xff, down, 0x40, up to 0x80: done.
        let mut n = 1;
        while p.tick(1.0).is_some() {
            n += 1;
        }
        assert_eq!((p.level, p.left), (0x80, 0));
        assert_eq!(n, 13 + 8 + 16 + 28 + 8);
        let mut forever = Pulse::new(-1);
        assert!((0..1000).all(|_| forever.tick(1.0).is_some()));
    }

    #[test]
    fn see_through_walk() {
        assert_eq!(walk(1, 18), [(2, 18), (3, 17), (4, 17), (5, 16), (6, 16), (7, 15), (8, 15), (9, 14), (10, 14)]);
        // Towards lower x the error grows: the row steps at every cell once it passes the run.
        assert_eq!(walk(15, 10), [(14, 10), (13, 11), (12, 12), (11, 13), (10, 14)]);
        assert!(walk(10, 13).is_empty());
    }
}
