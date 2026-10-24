//! The treasure pool: s2c 0x0D2 (LAB_100ecb30, an item or gil found) and 0x0D3 (LAB_100ecf80, a lot, a win, a loss)
//! into the slots at G+0x1951c, their log lines, and the windows (spec ~/ffxi/lab/research/loot_spec.md): "menu    loot"
//! (DAT_10578214, vtable 0x10333f28) lists the pool, "menu    lootope" (Cast Lot / Pass) opens beside a row, "menu    lnowin"
//! (Option, Done) passes on the rest. Opened only by the action menu's Treasure row (FUN_1013f060).

use super::World;
use super::game_ui::GameUi;
use super::net::{NetCommand, TrophyList};
use crate::audio::Ui;
use crate::dat::battle::Ctx;

/// Slots in the pool (G+0x1951c, 0x58 bytes each).
pub const SLOTS: usize = 10;
const LOOT: &str = "menu    loot";
const LOOTOPE: &str = "menu    lootope";
const LNOWIN: &str = "menu    lnowin";

/// One pool slot (spec 4.1).
#[derive(Clone, Debug, Default)]
pub struct Slot {
    /// +0x00: 2 in the pool, 3 gone, 0 never used.
    pub state: u8,
    pub item: u16,
    /// +0x08 (rows keep the low byte).
    pub qty: u32,
    /// +0x50: the actor it came from, for the log lines.
    pub from: u32,
    /// +0x54 StartTime: the rows' sort key.
    pub start: u32,
    /// +0x30: 0 none, 1 passed, 2 lotted.
    pub acted: u32,
    /// +0x34: our lot, -1 passed, 0 none.
    pub own_lot: i16,
    /// +0x36 / +0x38 / +0x40: the highest lot and its lotter.
    pub high_lot: u16,
    pub high_id: u32,
    pub high_name: String,
}

/// The open windows: loot's cursor child (1..10), lootope over a row, lnowin focused.
#[derive(Clone, Debug)]
pub struct Loot {
    pub cursor: u16,
    pub ope: Option<Ope>,
    pub done: bool,
}

/// lootope (FUN_1013f340): the row it opened on (placed at that row's right edge) and its cursor (1 Cast Lot, 2 Pass).
#[derive(Clone, Debug)]
pub struct Ope {
    pub slot: u8,
    pub item: u16,
    pub own_lot: i16,
    pub cursor: u16,
    pub row: u16,
}

/// 0x0D2: gil (Gold) prints 7031 0x13; an item takes its slot and, with a source (TargetUniqueNo; 0 for the pool
/// resent at zone-in), prints 0x10 "You find X on Y." or 0xda "... in the Y." (IsContainer); no item ends the slot.
pub(super) fn on_list(w: &mut World, l: TrophyList) {
    if l.gold > 0 {
        let me = w.my_id;
        line(w, 0x13, Some(me), l.target, l.gold as i32, Default::default());
    }
    let Some(s) = w.treasure.get_mut(l.slot as usize) else { return };
    if l.item == 0 {
        *s = Slot { state: 3, ..Default::default() };
        return;
    }
    let high = l.loot_id != 0;
    *s = Slot {
        state: 2,
        item: l.item,
        qty: l.qty,
        from: l.target,
        start: l.start,
        acted: l.entry as u32,
        own_lot: l.own_lot,
        high_lot: if high { l.loot_point } else { 0 },
        high_id: if high { l.loot_id } else { 0 },
        high_name: if high { l.loot_name } else { String::new() },
    };
    if l.target != 0 {
        line(w, if l.container { 0xda } else { 0x10 }, None, l.target, l.item as i32, Default::default());
    }
}

/// 0x0D3 by JudgeFlg: 0 a lot (0x11 "X's lot for the Y: N points."), 1 won (that lot line, then 0x12 "X obtains
/// Y."), 2 won but refused (0x0f with the winner, 0x1f without), 3 lost (no line here). 1..3 end the slot. Our own
/// entry sets our lot and action (EntryFlg ? lotted : passed); the highest lot always follows.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_solution(w: &mut World, slot: u8, judge: u8, loot: u32, loot_point: i16, entry: u32, entry_point: i16, entry_flg: bool, loot_name: String, entry_name: String) {
    let me = w.my_id;
    let Some(s) = w.treasure.get_mut(slot as usize).filter(|s| s.item != 0) else { return };
    let (item, from) = (s.item, s.from);
    if entry == me {
        (s.own_lot, s.acted) = (entry_point, if entry_flg { 2 } else { 1 });
    }
    (s.high_lot, s.high_id, s.high_name) = (loot_point as u16, loot, loot_name.clone());
    // The lot line needs a lot (not -1 / 0) and the lotter's name (sLootName2).
    let lot_line = |w: &mut World| {
        if entry_point > 0 && !entry_name.is_empty() {
            line(w, if entry != 0 { 0x11 } else { 0x82 }, Some(entry), from, item as i32, [entry_point.to_string(), entry_name.clone()]);
        }
    };
    match judge {
        0 => lot_line(w),
        1 => {
            lot_line(w);
            line(w, if loot != 0 { 0x12 } else { 0x83 }, Some(loot), from, item as i32, [loot_name, String::new()]);
        }
        2 if loot != 0 => line(w, 0x0f, Some(loot), from, item as i32, [loot_name, String::new()]),
        2 => line(w, 0x1f, None, from, item as i32, Default::default()),
        _ => {}
    }
    if judge != 0
        && let Some(s) = w.treasure.get_mut(slot as usize)
    {
        *s = Slot { state: 3, ..Default::default() };
    }
}

/// FUN_100a1ab0: a 7031 line with its actor, target, Para0 and string slots 2 / 3, in the message's mode colour.
fn line(w: &mut World, id: u16, actor: Option<u32>, target: u32, para0: i32, strings: [String; 2]) {
    let Some(bt) = w.battle.clone() else { return };
    let ctx = Ctx {
        actor: actor.and_then(|a| super::combat::who(w, a)),
        target: super::combat::who(w, target),
        me: w.name.clone(),
        work: [para0, 0, 0, 0, 0],
        strings,
    };
    match bt.system(id, &ctx) {
        Some(l) => {
            let colour = bt.colour_in(l.mode.unwrap_or(0), &w.game);
            for t in l.lines.into_iter().filter(|t| !t.is_empty()) {
                w.chat.push_colour(t, colour);
            }
        }
        None => log::info!("treasure line {id:#x} has no text"),
    }
}

/// FUN_100ebe60: slots in the pool.
pub fn count(w: &World) -> usize {
    w.treasure.iter().filter(|s| s.state == 2).count()
}

/// FUN_1013ebe0's rows: the pool's slot indexes, oldest StartTime first (equal times in slot order).
fn rows(w: &World) -> Vec<usize> {
    let mut r: Vec<usize> = (0..SLOTS).filter(|&i| w.treasure[i].state == 2).collect();
    r.sort_by_key(|&i| w.treasure[i].start);
    r
}

pub fn active(w: &World) -> bool {
    w.loot.is_some()
}

/// FUN_1013f060: lnowin, then loot (newest, with the focus).
pub fn open(w: &mut World) {
    if count(w) > 0 {
        w.loot = Some(Loot { cursor: 1, ope: None, done: false });
    }
}

/// FUN_1013f090: all three close (engaging, an event start, zoning, an empty pool, Escape).
pub fn close(w: &mut World) {
    w.loot = None;
}

/// Each frame (FUN_1013ebe0 / FUN_1013f430): no rows closes everything; the cursor stays on a row; lootope closes when
/// the cursor row is no longer the slot and item it opened on.
pub fn update(w: &mut World) {
    let Some(mut l) = w.loot.clone() else { return };
    let rows = rows(w);
    if rows.is_empty() {
        w.loot = None;
        return;
    }
    l.cursor = l.cursor.clamp(1, rows.len() as u16);
    let at = rows[l.cursor as usize - 1];
    if l.ope.as_ref().is_some_and(|o| o.slot as usize != at || o.item != w.treasure[at].item) {
        l.ope = None;
    }
    w.loot = Some(l);
}

/// FUN_100eb800: the first empty inventory slot (container 0, 1..size-1).
fn free_slot(w: &World) -> Option<u8> {
    (1..w.inv.sizes[0]).find(|&i| !w.inv.slots.contains_key(&(0, i)))
}

/// FUN_1013f460's Cast Lot: a Rare item (flags 0x8000) held in any container is refused with group 7 0x7e, no free
/// inventory slot with 0x7d; else c2s 0x041 [pool slot, free slot, 0, 0] and the slot is lotted.
fn cast_lot(w: &mut World, slot: u8, item: u16) {
    let rare = w.item_info.as_ref().and_then(|c| c.item(item)).is_some_and(|it| it.flags & 0x8000 != 0);
    if rare && w.inv.slots.iter().any(|(&(c, _), &(id, _))| c < 0x12 && id == item) {
        super::textcmd::refuse(w, 0x7e);
        return;
    }
    let Some(free) = free_slot(w) else {
        super::textcmd::refuse(w, 0x7d);
        return;
    };
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x041, body: vec![slot, free, 0, 0] });
    w.treasure[slot as usize].acted = 2;
}

/// c2s 0x042 [pool slot, 0] (FUN_100ebd10): passed.
fn pass(w: &mut World, slot: u8) {
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x042, body: vec![slot, 0] });
    w.treasure[slot as usize].acted = 1;
}

/// Keys (spec 3): lootope Up / Down / Enter / Escape; lnowin Enter (pass on every slot not acted on), Escape; loot Down /
/// Up one row (no wrap), Right / Left the last / first row, Enter lootope (SE 0004 on a passed row), Escape closes.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut l) = w.loot.clone() else { return };
    let rows = rows(w);
    let n = rows.len() as u16;
    if let Some(o) = l.ope.as_mut() {
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                let to = if key == egui::Key::ArrowUp { 1 } else { 2 };
                if to != o.cursor {
                    o.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            // Cast Lot is disabled once we lotted (child state 5: SE 0004, no handler).
            egui::Key::Enter if o.cursor == 1 && o.own_lot != 0 && o.own_lot != -1 => w.sounds.push(Ui::Error as u32),
            egui::Key::Enter => {
                w.sounds.push(Ui::Select as u32);
                let (slot, item, cursor) = (o.slot, o.item, o.cursor);
                l.ope = None;
                w.loot = Some(l);
                if cursor == 1 {
                    cast_lot(w, slot, item);
                } else {
                    pass(w, slot);
                }
                return;
            }
            egui::Key::Escape => {
                l.ope = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        w.loot = Some(l);
        return;
    }
    if l.done {
        match key {
            egui::Key::Enter => {
                w.sounds.push(Ui::Select as u32);
                for i in 0..SLOTS {
                    if w.treasure[i].state == 2 && w.treasure[i].acted == 0 {
                        pass(w, i as u8);
                    }
                }
                l.done = false;
            }
            egui::Key::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                w.sounds.push(13);
                l.done = false;
            }
            _ => {}
        }
        w.loot = Some(l);
        return;
    }
    let to = match key {
        egui::Key::ArrowDown => (l.cursor + 1).min(n),
        egui::Key::ArrowUp => l.cursor.saturating_sub(1).max(1),
        egui::Key::ArrowRight => n,
        egui::Key::ArrowLeft => 1,
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if let Some(&slot) = rows.get(l.cursor as usize - 1) {
                let s = &w.treasure[slot];
                if s.own_lot == -1 {
                    w.sounds.push(Ui::Error as u32);
                } else {
                    l.ope = Some(Ope { slot: slot as u8, item: s.item, own_lot: s.own_lot, cursor: 1, row: l.cursor });
                }
            }
            l.cursor
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.loot = None;
            return;
        }
        _ => l.cursor,
    };
    if to != l.cursor {
        w.sounds.push(Ui::Cursor as u32);
        l.cursor = to;
    }
    w.loot = Some(l);
}

/// Num+ (list message 8): the focus to lnowin, SE 0013; Num+ on lnowin gives it back.
pub fn num_plus(w: &mut World) {
    if let Some(l) = w.loot.as_mut().filter(|l| l.ope.is_none()) {
        l.done = !l.done;
        w.sounds.push(13);
    }
}

/// The item info window's item (mode 0): the cursor row's.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let l = w.loot.as_ref()?;
    let slot = *rows(w).get(l.cursor as usize - 1)?;
    Some((w.treasure[slot].item, None))
}

/// Help 0xe 300 and title 0xd 73 "Treasure Pool" from the focused window's strings.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(l) = &w.loot else { return };
    let (name, child) = match (&l.ope, l.done) {
        (Some(o), _) => (LOOTOPE, o.cursor),
        (None, true) => (LNOWIN, 1),
        (None, false) => (LOOT, l.cursor),
    };
    let [help, title] = gu.focus_text(name, child);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

/// The three windows (spec 2.5..2.7): loot's rows (a 16 x 16 icon at x + 2, the name at x + 20 white, orange once
/// lotted, grey once passed, "%2d" for stackables at window x + 0x12), lootope at its row's right edge (Cast Lot
/// greyed once lotted), lnowin at its layout spot unless lootope is up.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(l), Some(info)) = (&w.loot, w.item_info.clone()) else { return rects };
    let rows = rows(w);
    let focused = l.ope.is_none() && !l.done && w.confirm.is_none();
    let mut ope_at = None;
    if let Some(win) = gu.lobby_window_off(LOOT, sw, sh, (0.0, 0.0), focused.then_some(l.cursor), focused, &[]) {
        ope_at = l.ope.as_ref().and_then(|o| win.children.iter().find(|(id, _)| *id == o.row)).map(|(_, r)| (r[0] + r[2], r[1]));
        super::items::upload_icons(gu, &info, rows.iter().map(|&i| w.treasure[i].item));
        let wx = win.frame[0];
        for (k, &i) in rows.iter().enumerate() {
            let s = &w.treasure[i];
            let Some(&(_, r)) = win.children.iter().find(|(id, _)| *id as usize == k + 1) else { continue };
            let d = if focused && k + 1 == l.cursor as usize { -1.0 } else { 0.0 };
            let (x, y) = (r[0] + d, r[1] + d);
            let icon = crate::render::invwin::icon_texture(s.item);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, x + 2.0, y, 16.0, 16.0, [1.0; 4]);
            }
            let it = info.item(s.item);
            let tint: u32 = match s.own_lot {
                -1 => 0x8040_4040,
                0 => 0x8080_8080,
                _ => 0x8080_4020,
            };
            if let Some(it) = &it {
                gu.text(&it.name, x + 20.0, y, 0, tint.to_le_bytes());
                if it.stack > 1 {
                    gu.shape_text(&format!("{:2}", s.qty as u8), wx + 18.0, y + 2.0, 0x8080_8080, true);
                }
            }
        }
        rects.push(win.frame);
    }
    if let (Some(o), Some(at)) = (&l.ope, ope_at) {
        let layout = gu.window_rect(LOOTOPE, sw, sh);
        let off = layout.map_or((0.0, 0.0), |r| (at.0 - r[0], at.1 - r[1]));
        let lotted = o.own_lot != 0 && o.own_lot != -1;
        let hide: &[u16] = if lotted { &[1] } else { &[] };
        if let Some(win) = gu.lobby_window_off(LOOTOPE, sw, sh, off, Some(o.cursor), w.confirm.is_none(), hide) {
            gu.disabled_children(LOOTOPE, win.frame[0], win.frame[1], hide, Some(o.cursor));
            rects.push(win.frame);
        }
    } else if l.ope.is_none()
        && let Some(win) = gu.lobby_window_off(LNOWIN, sw, sh, (0.0, 0.0), l.done.then_some(1), l.done, &[]) {
        rects.push(win.frame);
    }
    rects
}

/// FUN_102041d0: while the pool holds items, partywin shows a chest (fontshp 0x7a) on its top edge near the left end.
pub fn draw_chest(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    if count(w) == 0 {
        return;
    }
    let name = gu.party_window_name().to_string();
    if let Some([x, y, ..]) = gu.window_rect(&name, sw, sh) {
        gu.sprite("font    fontshp", 0x7a, x + 8.0, y - 8.0, 0x8080_8080);
    }
}
