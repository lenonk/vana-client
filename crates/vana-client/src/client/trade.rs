//! Trading (spec ~/ffxi/lab/research/trade_spec.md). The NPC hand-over "menu    handover" (FUN_10139840): up to
//! eight inventory items and gil picked from the inventory list, OK sends c2s 0x036 at once and closes; the server's
//! onTrade answers with an event, a line or nothing.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;

const HANDOVER: &str = "menu    handover";
const MONEYCTR: &str = "menu    moneyctr";
const ITEMCTRL: &str = "menu    itemctrl";
/// FUN_10152490: 6.0 between the two actors (squared 36).
const REACH: f32 = 6.0;
const ORANGE: u32 = 0x8080_4020;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Focus {
    Window,
    Picker,
    Qty,
    Money,
}

/// DAT_105781f0: the NPC (+0x48 / +0x44), the eight entries (+0x14: item, inventory slot, count), gil (+0x3c).
pub struct Handover {
    npc: (u32, u16),
    slots: [Option<(u16, u8, u32)>; 8],
    gil: u32,
    cursor: u16,
    focus: Focus,
    list: (usize, usize),
    qty: Option<(u8, u32, u32)>,
    money: Option<(u32, u32, u32)>,
}

pub fn active(w: &World) -> bool {
    w.handover.is_some()
}

/// FUN_1013fd30 for the current target: a monster refuses (7031 0x1c), an NPC opens the hand-over after the status
/// gate (0x43) and the range check (7031 9). False when nothing opened.
pub(super) fn start(w: &mut World) -> bool {
    let Some((t, index, mob, pos)) = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.index, e.mob, e.pos))) else {
        super::post::system_line(w, 0x1d, [0; 5], String::new());
        return false;
    };
    if mob {
        super::post::system_line(w, 0x1c, [0; 5], String::new());
        return false;
    }
    // Players: the trade request (not done yet).
    if t < 0x0100_0000 || !super::textcmd::status_gate(w, 0x43) {
        return false;
    }
    if pos.distance_squared(w.pos) > REACH * REACH {
        super::post::system_line(w, 9, [0; 5], String::new());
        return false;
    }
    w.menu = None;
    w.handover = Some(Handover { npc: (t, index), slots: [None; 8], gil: 0, cursor: 1, focus: Focus::Window, list: (0, 0), qty: None, money: None });
    true
}

/// The picker's grey (FUN_10139750): linkshell items (0x201..0x203) and worn ones; orange when already placed.
fn picker_tint(h: &Handover, r: &crate::inventory::Slot) -> u32 {
    if (0x201..=0x203).contains(&r.id) || r.equipped {
        crate::inventory::TINT_DIM
    } else if h.slots.iter().flatten().any(|s| s.1 == r.index) {
        ORANGE
    } else {
        crate::inventory::TINT_WHITE
    }
}

/// FUN_10139e30: an inventory slot already placed is taken out (and when it sat under the cursor, that is all);
/// then the cursor slot takes it.
fn place(h: &mut Handover, inv: u8, item: u16, n: u32) {
    let c = (h.cursor as usize).wrapping_sub(1);
    if let Some(k) = h.slots.iter().position(|s| s.is_some_and(|s| s.1 == inv && s.0 == item)) {
        h.slots[k] = None;
        if k == c {
            return;
        }
    }
    if let Some(s) = h.slots.get_mut(c) {
        *s = Some((item, inv, n));
    }
}

/// c2s 0x036 (FUN_10139cd0): UniqueNo, counts, inventory slots, ActIndex, the number of entries; gil first (slot 0).
fn send(w: &World, h: &Handover) -> bool {
    let entries: Vec<(u32, u8)> = (h.gil > 0).then_some((h.gil, 0)).into_iter().chain(h.slots.iter().flatten().map(|s| (s.2, s.1))).collect();
    if entries.is_empty() {
        return false;
    }
    let mut b = vec![0u8; 0x3c];
    b[0..4].copy_from_slice(&h.npc.0.to_le_bytes());
    for (k, (n, inv)) in entries.iter().enumerate() {
        b[4 + 4 * k..8 + 4 * k].copy_from_slice(&n.to_le_bytes());
        b[0x2c + k] = *inv;
    }
    b[0x36..0x38].copy_from_slice(&h.npc.1.to_le_bytes());
    b[0x38] = entries.len() as u8;
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x036, body: b });
    true
}

/// Per frame (FUN_10139b00): the NPC out of reach (or gone) closes it with "Target out of range."; an event closes it
/// (the 0x838 hook).
pub fn update(w: &mut World) {
    let Some(h) = &w.handover else { return };
    let far = w.entities.get(&h.npc.0).is_none_or(|e| e.pos.distance_squared(w.pos) > REACH * REACH);
    if far {
        w.handover = None;
        super::post::system_line(w, 9, [0; 5], String::new());
    } else if super::event::active(w) {
        w.handover = None;
    }
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut h) = w.handover.take() else { return };
    let mut close = false;
    match h.focus {
        // FUN_10139a40: Enter on a slot picks into it, OK sends and closes, Cancel closes, the gil child asks the
        // amount; Escape puts the cursor on Cancel first.
        Focus::Window => match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = super::post::link(gu, HANDOVER, h.cursor, key);
                if to != h.cursor && to != 0 {
                    h.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter => match h.cursor {
                1..=8 => {
                    h.focus = Focus::Picker;
                    w.sounds.push(Ui::Select as u32);
                }
                9 if send(w, &h) => {
                    close = true;
                    w.sounds.push(Ui::Select as u32);
                }
                9 => w.sounds.push(Ui::Error as u32),
                10 => {
                    close = true;
                    w.sounds.push(Ui::Select as u32);
                }
                11 => {
                    h.money = Some((1, w.inv.gil(), 0));
                    h.focus = Focus::Money;
                    w.sounds.push(Ui::Select as u32);
                }
                _ => {}
            },
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                if h.cursor == 10 { close = true } else { h.cursor = 10 }
            }
            _ => {}
        },
        // The inventory (FUN_101397f0): grey rows refuse; a stack asks how many (itemctrl), one goes straight in.
        Focus::Picker => {
            let rows = super::post::picker_rows(w);
            let (mut sc, mut cu) = h.list;
            match key {
                K::ArrowUp | K::ArrowDown => super::items::list_keys(&mut sc, &mut cu, rows.len(), key),
                K::ArrowRight | K::ArrowLeft => {
                    if super::items::page_keys(&mut sc, &mut cu, rows.len(), key == K::ArrowRight) {
                        w.sounds.push(Ui::Cursor as u32);
                    }
                }
                K::Enter => {
                    if let Some(r) = rows.get(sc + cu) {
                        if picker_tint(&h, r) == crate::inventory::TINT_DIM {
                            w.sounds.push(Ui::Error as u32);
                        } else {
                            w.sounds.push(Ui::Select as u32);
                            if r.count > 1 {
                                h.qty = Some((r.index, 1, r.count));
                                h.focus = Focus::Qty;
                            } else {
                                place(&mut h, r.index, r.id, 1);
                                h.focus = Focus::Window;
                            }
                        }
                    }
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    h.focus = Focus::Window;
                }
                _ => {}
            }
            h.list = (sc, cu);
        }
        Focus::Qty => {
            let (inv, mut v, max) = h.qty.unwrap_or((0, 1, 1));
            match key {
                K::ArrowUp => v = (v + 1).min(max),
                K::ArrowDown => v = v.saturating_sub(1).max(1),
                K::ArrowRight => v = 1,
                K::ArrowLeft => v = max,
                _ => {}
            }
            h.qty = Some((inv, v, max));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter => {
                    w.sounds.push(Ui::Select as u32);
                    let item = w.inv.slots.get(&(super::items::INVENTORY, inv)).map_or(0, |x| x.0);
                    place(&mut h, inv, item, v);
                    (h.qty, h.focus) = (None, Focus::Window);
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    (h.qty, h.focus) = (None, Focus::Picker);
                }
                _ => {}
            }
        }
        // moneyctr (FUN_10140bc0), as the delivery box's: Enter sets the gil, Escape keeps it.
        Focus::Money => {
            let (mut step, max, mut v) = h.money.unwrap_or((1, 0, 0));
            match key {
                K::ArrowDown if v >= step => v -= step,
                K::ArrowUp if v < max => v = (v + step).min(max),
                K::ArrowRight if step > 1 => step /= 10,
                K::ArrowRight => v = 0,
                K::ArrowLeft if max / step.max(1) < 10 => v = max,
                K::ArrowLeft => step *= 10,
                _ => {}
            }
            h.money = Some((step, max, v));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter | K::Escape => {
                    w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
                    if key == K::Enter {
                        h.gil = v;
                    }
                    (h.money, h.focus) = (None, Focus::Window);
                }
                _ => {}
            }
        }
    }
    if !close {
        w.handover = Some(h);
    }
}

/// The item info window: the picker's row, or the cursor slot's inventory item (none for gil or an empty slot).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let h = w.handover.as_ref()?;
    let inv = match h.focus {
        Focus::Picker | Focus::Qty => super::post::picker_rows(w).get(h.list.0 + h.list.1)?.index,
        _ => h.slots.get((h.cursor as usize).checked_sub(1)?)?.as_ref()?.1,
    };
    let id = w.inv.slots.get(&(super::items::INVENTORY, inv))?.0;
    Some((id, w.inv.ext.get(&(super::items::INVENTORY, inv)).copied()))
}

/// The bars: title slot 2 group 0xd 10 "Trade"; the focused window's help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(h) = &w.handover else { return };
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    match h.focus {
        Focus::Window => focus(HANDOVER, h.cursor, bars),
        Focus::Picker => {
            focus("menu    inventor", h.list.1 as u16 + 1, bars);
            bars.help[1] = gu.menu_msg(3, "");
        }
        Focus::Qty => {
            bars.help[1] = gu.menu_msg(0x10c, "");
            bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
        }
        Focus::Money => focus(MONEYCTR, 1, bars),
    }
    if bars.title[1].is_empty() {
        bars.title[1] = gu.title_msg(10).unwrap_or_default();
    }
}

/// The inventory list at x 200, money, the hand-over (16, 48) with its slots and gil, itemctrl / moneyctr.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const WHITE: u32 = 0x8080_8080;
    let mut rects = Vec::new();
    let (Some(h), Some(info)) = (&w.handover, w.item_info.clone()) else { return rects };
    let rows = super::post::picker_rows(w);
    let tints: Vec<u32> = rows.iter().map(|r| picker_tint(h, r)).collect();
    rects.extend(super::post::draw_list(gu, w, &info, h.list, h.focus == Focus::Picker, Some(&tints), sw, sh));
    rects.extend(super::post::draw_money(gu, w, sw, sh));
    let focused = h.focus == Focus::Window;
    if let Some(win) = gu.lobby_window_off(HANDOVER, sw, sh, (0.0, 0.0), focused.then_some(h.cursor), focused, &[]) {
        let slots: Vec<(u16, u32, bool)> = h.slots.iter().map(|s| s.map_or((0, 0, false), |s| (s.0, s.2, true))).collect();
        super::post::draw_slots(gu, &info, HANDOVER, win.frame, &slots, None, focused.then_some(h.cursor), |_| [1.0; 4]);
        // FUN_10140910: the gil "N  G" right-aligned at child 11's right edge + 10.
        if let Some(e) = gu.layout(HANDOVER).and_then(|l| l.elements.iter().find(|e| e.id == 11).cloned()) {
            let g = crate::inventory::gil_text(h.gil as i64);
            gu.shape_text(&g, win.frame[0] + e.x as f32 + e.w as f32 + 10.0, win.frame[1] + e.y as f32, WHITE, true);
        }
        rects.push(win.frame);
    }
    if let Some((_, v, max)) = h.qty
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        let [x, y, ..] = win.frame;
        gu.shape_text(&format!("{v:2}"), x + 52.0, y + 22.0, WHITE, true);
        gu.shape_text(&format!("/{max:2}"), x + 58.0, y + 22.0, WHITE, false);
        rects.push(win.frame);
    }
    if let Some((step, max, v)) = h.money
        && let Some(win) = gu.lobby_window_off(MONEYCTR, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        super::post::draw_moneyctr(gu, win.frame, step, max, v);
        rects.push(win.frame);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Handover {
        Handover { npc: (0x0100_4011, 0x11), slots: [None; 8], gil: 0, cursor: 1, focus: Focus::Window, list: (0, 0), qty: None, money: None }
    }

    /// FUN_10139e30: placing again under the cursor takes it out; elsewhere it moves.
    #[test]
    fn place_toggles_and_moves() {
        let mut x = h();
        place(&mut x, 5, 0x1234, 3);
        assert_eq!(x.slots[0], Some((0x1234, 5, 3)));
        place(&mut x, 5, 0x1234, 3);
        assert_eq!(x.slots[0], None);
        place(&mut x, 5, 0x1234, 3);
        x.cursor = 2;
        place(&mut x, 5, 0x1234, 2);
        assert_eq!((x.slots[0], x.slots[1]), (None, Some((0x1234, 5, 2))));
    }
}
