//! Trading (spec ~/ffxi/lab/research/trade_spec.md). The NPC hand-over "menu    handover" (FUN_10139840): up to
//! eight inventory items and gil picked from the inventory list, OK sends c2s 0x036 at once and closes; the server's
//! onTrade answers with an event, a line or nothing. Player trade (FUN_10151040): c2s 0x032 asks, the other side
//! accepts by choosing Trade (0x033 Start), "menu    trade" (ours) and "menu    gift" (theirs) open on s2c 0x022
//! Start; offers go as 0x034 and come back as 0x025 / 0x023; both OKs (0x033 Make) end it with 0x022 End.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::ItemInfo;
use std::time::{Duration, Instant};

const HANDOVER: &str = "menu    handover";
const TRADE: &str = "menu    trade";
const GIFT: &str = "menu    gift";
const GIL: u16 = 0xffff;
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
    w.handover.is_some() || w.p2p.ui.is_some()
}

/// FUN_1013fd30 for the current target: a monster refuses (7031 0x1c), an NPC opens the hand-over after the status
/// gate (0x43) and the range check (7031 9). False when nothing opened.
pub(super) fn start(w: &mut World) -> bool {
    // A pending request from someone wins: Trade accepts it.
    if w.p2p.incoming.is_some() {
        accept(w);
        return true;
    }
    let Some((t, index, mob, pos)) = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.index, e.mob, e.pos))) else {
        super::post::system_line(w, 0x1d, [0; 5], String::new());
        return false;
    };
    if mob {
        super::post::system_line(w, 0x1c, [0; 5], String::new());
        return false;
    }
    if t < 0x0100_0000 {
        return request(w, t, index, pos);
    }
    if !super::textcmd::status_gate(w, 0x43) {
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
fn handover_update(w: &mut World) {
    let Some(h) = &w.handover else { return };
    let far = w.entities.get(&h.npc.0).is_none_or(|e| e.pos.distance_squared(w.pos) > REACH * REACH);
    if far {
        w.handover = None;
        super::post::system_line(w, 9, [0; 5], String::new());
    } else if super::event::active(w) {
        w.handover = None;
    }
}

fn handover_key(w: &mut World, gu: &GameUi, key: egui::Key) {
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
fn handover_info(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let h = w.handover.as_ref()?;
    let inv = match h.focus {
        Focus::Picker | Focus::Qty => super::post::picker_rows(w).get(h.list.0 + h.list.1)?.index,
        _ => h.slots.get((h.cursor as usize).checked_sub(1)?)?.as_ref()?.1,
    };
    let id = w.inv.slots.get(&(super::items::INVENTORY, inv))?.0;
    Some((id, w.inv.ext.get(&(super::items::INVENTORY, inv)).copied()))
}

/// The bars: title slot 2 group 0xd 10 "Trade"; the focused window's help.
fn handover_bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
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
fn handover_draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
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

// --- Dispatch: the hand-over or the player trade ---

pub fn update(w: &mut World) {
    handover_update(w);
    p2p_update(w);
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    if w.handover.is_some() {
        handover_key(w, gu, key);
    } else {
        p2p_key(w, gu, key);
    }
}

pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    handover_info(w).or_else(|| p2p_info(w))
}

pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    handover_bars(gu, w, bars);
    p2p_bars(gu, w, bars);
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut r = handover_draw(gu, w, sw, sh);
    r.extend(p2p_draw(gu, w, sw, sh));
    r
}

// --- Player trade ---

#[derive(Clone, Copy, Debug, PartialEq)]
enum PFocus {
    Trade,
    Gift,
    Picker,
    Qty,
    Money,
}

/// The trade and gift windows' cursors and the picker / itemctrl / moneyctr state.
struct TradeUi {
    focus: PFocus,
    cursor: u16,
    gift: u16,
    list: (usize, usize),
    qty: Option<(u8, u32, u32)>,
    money: Option<(u32, u32, u32)>,
}

/// DAT_10578484: our state (+0xc0: 0 idle, 1 asked or asked by someone, 3 trading, 4 our OK), the partner's OK
/// (+0xc4 == 4), the partner (+0x6c / +0x70) and the pending requester (+0x74 / +0x78), the times (+0x20 asked,
/// +0x28 cancelled), the TradeCounter of the last 0x023, our list (s2c 0x025: count, item, inventory slot; entry 0
/// gil) and theirs (0x023: count, item, extdata).
#[derive(Default)]
pub struct P2p {
    state: u8,
    partner_ok: bool,
    partner: Option<(u32, u16)>,
    incoming: Option<(u32, u16)>,
    asked_at: Option<Instant>,
    cancelled_at: Option<Instant>,
    counter: u16,
    mine: [(u32, u16, u8); 9],
    theirs: [(u32, u16, [u8; 24]); 9],
    ui: Option<TradeUi>,
}

impl P2p {
    fn reset(&mut self) {
        let (incoming, cancelled_at) = (self.incoming, self.cancelled_at);
        *self = P2p { incoming, cancelled_at, ..Default::default() };
    }
}

fn packet(w: &World, id: u16, body: Vec<u8>) {
    let _ = w.net.commands.send(NetCommand::Packet { id, body });
}

/// c2s 0x033 (FUN_100eb9c0..ebab0): Kind (0 Start, 1 Cancel, 2 Make, 3 MakeCancell), the last TradeCounter.
fn respond(w: &World, kind: u32) {
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&kind.to_le_bytes());
    b[4..6].copy_from_slice(&w.p2p.counter.to_le_bytes());
    packet(w, 0x033, b);
}

/// c2s 0x034 (FUN_100ebb00): ItemNum (0 clears), ItemNo, inventory slot, trade slot (0 = gil); after our OK a
/// MakeCancell goes first (FUN_101511a0).
fn offer(w: &mut World, n: u32, item: u16, inv: u8, slot: u8) {
    if w.p2p.state != 3 {
        respond(w, 3);
        w.p2p.state = 3;
    }
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&n.to_le_bytes());
    b[4..6].copy_from_slice(&item.to_le_bytes());
    b[6] = inv;
    b[7] = slot;
    packet(w, 0x034, b);
}

/// A 7031 line with the partner's name (combat::on_message_std's actor).
fn line(w: &mut World, id: u16, actor: u32) {
    super::combat::on_message_std(w, id, actor, [0; 4], Default::default());
}

fn in_reach(w: &World, id: u32) -> bool {
    w.entities.get(&id).is_some_and(|e| e.pos.distance_squared(w.pos) <= REACH * REACH)
}

fn close(w: &mut World) {
    w.p2p.reset();
}

/// FUN_10151e40 on a player: someone already asked gets the cancel prompt (group 0xc 0x2d, within 60 s and in reach;
/// else a silent cancel); out of reach 7031 9; within 3 s of a cancel nothing; else c2s 0x032 and group 10 0x14 "You
/// offer to trade with %s.".
fn request(w: &mut World, t: u32, index: u16, pos: glam::Vec3) -> bool {
    let now = Instant::now();
    let since = |at: Option<Instant>, s: u64| at.is_none_or(|a| now.duration_since(a) > Duration::from_secs(s));
    if let Some((p, _)) = w.p2p.partner {
        if in_reach(w, p) && since(w.p2p.asked_at, 1) && since(w.p2p.cancelled_at, 5) {
            if !since(w.p2p.asked_at, 60) {
                w.confirm = Some((super::Confirm::CancelTrade, false));
            } else {
                cancel_request(w);
            }
        }
        return false;
    }
    if pos.distance_squared(w.pos) > REACH * REACH {
        super::post::system_line(w, 9, [0; 5], String::new());
        return false;
    }
    if !since(w.p2p.cancelled_at, 3) {
        return false;
    }
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&t.to_le_bytes());
    b[4..6].copy_from_slice(&index.to_le_bytes());
    packet(w, 0x032, b);
    (w.p2p.partner, w.p2p.asked_at, w.p2p.state) = (Some((t, index)), Some(now), 1);
    let name = w.entities.get(&t).map(|e| e.name.clone()).unwrap_or_default();
    if let Some(bt) = w.battle.clone()
        && let Some(f) = bt.group10(0x14)
    {
        let colour = bt.colour_in(0x79, &w.game);
        w.chat.push_colour(f.replacen("%s", &name, 1), colour);
    }
    true
}

/// The cancel prompt's Yes (FUN_10151340): c2s 0x033 Cancel, no partner.
pub(super) fn cancel_request(w: &mut World) {
    respond(w, 1);
    w.p2p.reset();
    w.p2p.cancelled_at = Some(Instant::now());
}

/// Trade with a request pending: the requester still here and in reach gets c2s 0x033 Start; else it is dropped.
fn accept(w: &mut World) {
    let Some((id, _)) = w.p2p.incoming else { return };
    if in_reach(w, id) {
        respond(w, 0);
    } else {
        w.p2p.incoming = None;
        super::post::system_line(w, 9, [0; 5], String::new());
    }
}

/// s2c 0x021 (FUN_10152150): only while idle or asked; 7031 0x18 "{name} wishes to trade with you.". No prompt: the
/// player accepts by choosing Trade.
pub(super) fn on_request(w: &mut World, id: u32, index: u16) {
    if !matches!(w.p2p.state, 0 | 1) {
        return;
    }
    (w.p2p.incoming, w.p2p.state) = (Some((id, index)), 1);
    line(w, 0x18, id);
}

/// The windows that keep a trade from opening (registry flag 0x20000).
fn window_open(w: &World) -> bool {
    w.shop.is_some() || w.win.is_some() || w.handover.is_some() || super::post::active(w) || super::synth::active(w) || w.moogle.is_some()
}

/// s2c 0x022 (FUN_101521c0 / FUN_10152240): 0 Start opens both windows (unless out of reach or a window is in the
/// way: then c2s 0x033 Cancel); 1 Cancell and the errors 4 / 5 / 7 / 8 print 7031 0x1a and close; 2 the partner's OK;
/// 6 closes quietly; 9 End "Trade complete.".
pub(super) fn on_response(w: &mut World, id: u32, kind: u32, index: u16) {
    let partner = w.p2p.partner.map(|p| p.0);
    let incoming = w.p2p.incoming.map(|p| p.0);
    let trading = w.p2p.ui.is_some();
    match kind {
        0 => {
            if incoming == Some(id) {
                (w.p2p.partner, w.p2p.incoming) = (Some((id, index)), None);
            }
            if w.p2p.partner.map(|p| p.0) != Some(id) {
                return;
            }
            if matches!(w.confirm, Some((super::Confirm::CancelTrade, _))) {
                w.confirm = None;
            }
            if !in_reach(w, id) || window_open(w) {
                respond(w, 1);
                w.p2p.reset();
                return;
            }
            w.menu = None;
            w.p2p.ui = Some(TradeUi { focus: PFocus::Trade, cursor: 1, gift: 1, list: (0, 0), qty: None, money: None });
            (w.p2p.state, w.p2p.partner_ok, w.p2p.mine, w.p2p.theirs) = (3, false, Default::default(), Default::default());
        }
        2 => w.p2p.partner_ok = true,
        3 => {}
        9 => {
            line(w, 0x1e, id);
            close(w);
        }
        6 if trading => close(w),
        6 => {}
        1 | 4 | 5 | 7 | 8 => {
            if Some(id) == partner || Some(id) == incoming {
                line(w, 0x1a, id);
            }
            if Some(id) == incoming {
                w.p2p.incoming = None;
            }
            if !trading && kind == 7 {
                (w.p2p.partner, w.p2p.state) = (None, 0);
            } else if Some(id) == partner || trading {
                close(w);
            }
        }
        _ => {}
    }
}

/// s2c 0x023 (LAB_100ec9f0 / FUN_10152450): their slot; any change voids both OKs.
pub(super) fn on_their_slot(w: &mut World, n: u32, counter: u16, item: u16, slot: u8, attr: [u8; 24]) {
    w.p2p.counter = counter;
    if let Some(e) = w.p2p.theirs.get_mut(slot as usize) {
        *e = (n, item, attr);
    }
    if w.p2p.ui.is_some() {
        (w.p2p.state, w.p2p.partner_ok) = (3, false);
    }
}

/// s2c 0x025 (LAB_100ec9a0): our slot as the server took it (the window draws from this).
pub(super) fn on_my_slot(w: &mut World, n: u32, item: u16, slot: u8, inv: u8) {
    if let Some(e) = w.p2p.mine.get_mut(slot as usize) {
        *e = (n, item, inv);
    }
}

/// An offered slot: count, item, inventory slot (0 empty, gil, or no item).
fn offered(e: &(u32, u16, u8)) -> bool {
    e.2 != 0 && e.0 > 0 && e.1 != 0 && e.1 != GIL
}

/// FUN_10150e60: grey for Ex (item flag 0x4000) and worn items; orange when offered.
fn p2p_tint(w: &World, r: &crate::inventory::Slot) -> u32 {
    let flags = w.item_info.as_ref().map_or(0, |c| c.flags(r.id));
    if flags & 0x4000 != 0 || r.equipped {
        crate::inventory::TINT_DIM
    } else if w.p2p.mine.iter().any(|e| offered(e) && e.2 == r.index) {
        ORANGE
    } else {
        crate::inventory::TINT_WHITE
    }
}

/// FUN_101511a0: an inventory slot already offered is taken back (a toggle); else the cursor slot takes it.
fn place_offer(w: &mut World, cursor: u16, inv: u8, item: u16, n: u32) {
    if let Some(k) = w.p2p.mine.iter().position(|e| offered(e) && e.2 == inv) {
        offer(w, 0, item, inv, k as u8);
    } else if (1..=8).contains(&cursor) {
        offer(w, n, item, inv, cursor as u8);
    }
}

/// Per frame (FUN_101518b0): the partner out of reach cancels (7031 9); an event cancels (7031 0x1a, the 0x838 hook).
fn p2p_update(w: &mut World) {
    let Some((p, _)) = w.p2p.partner.filter(|_| w.p2p.ui.is_some()) else { return };
    if !in_reach(w, p) {
        super::post::system_line(w, 9, [0; 5], String::new());
        respond(w, 1);
        close(w);
    } else if super::event::active(w) {
        line(w, 0x1a, p);
        respond(w, 1);
        close(w);
    }
}

fn p2p_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(mut u) = w.p2p.ui.take() else { return };
    let locked = w.p2p.state == 4;
    let mut quit = false;
    match u.focus {
        // FUN_101515e0: slots pick from the inventory (a filled one is cleared), OK sends Make, Cancel cancels, the gil
        // child asks the amount; after our OK only OK / Cancel take keys. Down from the bottom row moves to the gift.
        PFocus::Trade => match key {
            K::ArrowDown if !locked && (5..=8).contains(&u.cursor) => {
                (u.focus, u.gift) = (PFocus::Gift, u.cursor - 4);
                w.sounds.push(Ui::Cursor as u32);
            }
            K::ArrowDown | K::ArrowUp if locked => {
                u.cursor = if key == K::ArrowDown { 10 } else { 9 };
                w.sounds.push(Ui::Cursor as u32);
            }
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft if !locked => {
                let to = super::post::link(gu, TRADE, u.cursor, key);
                if to != u.cursor && to != 0 {
                    u.cursor = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Enter => match (u.cursor, locked) {
                (10, _) => {
                    respond(w, 1);
                    quit = true;
                    w.sounds.push(Ui::Select as u32);
                }
                (9, false) => {
                    respond(w, 2);
                    w.p2p.state = 4;
                    w.sounds.push(Ui::Select as u32);
                }
                (9, true) => {}
                (_, true) => u.cursor = 9,
                (c @ 1..=8, false) => {
                    let e = w.p2p.mine[c as usize];
                    if offered(&e) {
                        offer(w, 0, e.1, e.2, c as u8);
                    } else {
                        u.focus = PFocus::Picker;
                    }
                    w.sounds.push(Ui::Select as u32);
                }
                (11, false) => {
                    let current = if w.p2p.mine[0].1 == GIL { w.p2p.mine[0].0 } else { 0 };
                    u.money = Some((1, w.inv.gil(), current));
                    u.focus = PFocus::Money;
                    w.sounds.push(Ui::Select as u32);
                }
                _ => {}
            },
            K::Escape => {
                u.cursor = 10;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        },
        PFocus::Gift => match key {
            K::ArrowUp if (1..=4).contains(&u.gift) => {
                (u.focus, u.cursor) = (PFocus::Trade, u.gift + 4);
                w.sounds.push(Ui::Cursor as u32);
            }
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => {
                let to = super::post::link(gu, GIFT, u.gift, key);
                if to != u.gift && (1..=8).contains(&to) {
                    u.gift = to;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Escape => {
                (u.focus, u.cursor) = (PFocus::Trade, 10);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        },
        PFocus::Picker => {
            let rows = super::post::picker_rows(w);
            let (mut sc, mut cu) = u.list;
            match key {
                K::ArrowUp | K::ArrowDown => super::items::list_keys(&mut sc, &mut cu, rows.len(), key),
                K::ArrowRight | K::ArrowLeft => {
                    if super::items::page_keys(&mut sc, &mut cu, rows.len(), key == K::ArrowRight) {
                        w.sounds.push(Ui::Cursor as u32);
                    }
                }
                K::Enter => {
                    if let Some(r) = rows.get(sc + cu).cloned() {
                        if p2p_tint(w, &r) == crate::inventory::TINT_DIM {
                            w.sounds.push(Ui::Error as u32);
                        } else {
                            w.sounds.push(Ui::Select as u32);
                            if r.count > 1 {
                                (u.qty, u.focus) = (Some((r.index, 1, r.count)), PFocus::Qty);
                            } else {
                                place_offer(w, u.cursor, r.index, r.id, 1);
                                u.focus = PFocus::Trade;
                            }
                        }
                    }
                }
                K::Escape => {
                    u.focus = PFocus::Trade;
                    w.sounds.push(Ui::Cancel as u32);
                }
                _ => {}
            }
            u.list = (sc, cu);
        }
        PFocus::Qty => {
            let (inv, mut v, max) = u.qty.unwrap_or((0, 1, 1));
            match key {
                K::ArrowUp => v = (v + 1).min(max),
                K::ArrowDown => v = v.saturating_sub(1).max(1),
                K::ArrowRight => v = 1,
                K::ArrowLeft => v = max,
                _ => {}
            }
            u.qty = Some((inv, v, max));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter => {
                    w.sounds.push(Ui::Select as u32);
                    let item = w.inv.slots.get(&(super::items::INVENTORY, inv)).map_or(0, |x| x.0);
                    place_offer(w, u.cursor, inv, item, v);
                    (u.qty, u.focus) = (None, PFocus::Trade);
                }
                K::Escape => {
                    w.sounds.push(Ui::Cancel as u32);
                    (u.qty, u.focus) = (None, PFocus::Picker);
                }
                _ => {}
            }
        }
        PFocus::Money => {
            let (mut step, max, mut v) = u.money.unwrap_or((1, 0, 0));
            match key {
                K::ArrowDown if v >= step => v -= step,
                K::ArrowUp if v < max => v = (v + step).min(max),
                K::ArrowRight if step > 1 => step /= 10,
                K::ArrowRight => v = 0,
                K::ArrowLeft if max / step.max(1) < 10 => v = max,
                K::ArrowLeft => step *= 10,
                _ => {}
            }
            u.money = Some((step, max, v));
            match key {
                K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
                K::Enter | K::Escape => {
                    w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
                    if key == K::Enter {
                        offer(w, v, GIL, 0, 0);
                    }
                    (u.money, u.focus) = (None, PFocus::Trade);
                }
                _ => {}
            }
        }
    }
    if quit {
        close(w);
    } else if w.p2p.partner.is_some() {
        // After our OK the cursor is held on OK (or Cancel).
        if w.p2p.state == 4 && u.focus == PFocus::Trade && !matches!(u.cursor, 9 | 10) {
            u.cursor = 9;
        }
        w.p2p.ui = Some(u);
    }
}

fn p2p_info(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let u = w.p2p.ui.as_ref()?;
    let inv = match u.focus {
        PFocus::Picker | PFocus::Qty => super::post::picker_rows(w).get(u.list.0 + u.list.1)?.index,
        PFocus::Gift => {
            let e = w.p2p.theirs.get(u.gift as usize)?;
            return (e.0 > 0 && e.1 != 0 && e.1 != GIL).then_some((e.1, Some(e.2)));
        }
        _ => w.p2p.mine.get(u.cursor as usize).filter(|e| offered(e))?.2,
    };
    let id = w.inv.slots.get(&(super::items::INVENTORY, inv))?.0;
    Some((id, w.inv.ext.get(&(super::items::INVENTORY, inv)).copied()))
}

/// Title slot 2 group 0xd 2 (FUN_10151380); the focused window's help.
fn p2p_bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(u) = &w.p2p.ui else { return };
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    match u.focus {
        PFocus::Trade => focus(TRADE, u.cursor, bars),
        PFocus::Gift => focus(GIFT, u.gift, bars),
        PFocus::Picker => {
            focus("menu    inventor", u.list.1 as u16 + 1, bars);
            bars.help[1] = gu.menu_msg(3, "");
        }
        PFocus::Qty => {
            bars.help[1] = gu.menu_msg(0x10c, "");
            bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
        }
        PFocus::Money => focus(MONEYCTR, 1, bars),
    }
    if bars.title[1].is_empty() {
        bars.title[1] = gu.title_msg(2).unwrap_or_default();
    }
}

/// The picker, money, "menu    trade" (our offer from the 0x025 list, our name orange after our OK) and "menu    gift"
/// (theirs from 0x023, their name orange after their OK, their gil at the top right).
fn p2p_draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const WHITE: u32 = 0x8080_8080;
    let mut rects = Vec::new();
    let (Some(u), Some(info)) = (&w.p2p.ui, w.item_info.clone()) else { return rects };
    let rows = super::post::picker_rows(w);
    let tints: Vec<u32> = rows.iter().map(|r| p2p_tint(w, r)).collect();
    rects.extend(super::post::draw_list(gu, w, &info, u.list, u.focus == PFocus::Picker, Some(&tints), sw, sh));
    rects.extend(super::post::draw_money(gu, w, sw, sh));
    let name_of = |id: Option<u32>| id.and_then(|i| w.entities.get(&i)).map(|e| e.name.clone()).unwrap_or_default();
    let focused = u.focus == PFocus::Trade;
    if let Some(win) = gu.lobby_window_off(TRADE, sw, sh, (0.0, 0.0), focused.then_some(u.cursor), focused, &[]) {
        let [x, y, ..] = win.frame;
        let slots: Vec<(u16, u32, bool)> = w.p2p.mine[1..].iter().map(|e| if offered(e) { (e.1, e.0, true) } else { (0, 0, false) }).collect();
        super::post::draw_slots(gu, &info, TRADE, win.frame, &slots, None, focused.then_some(u.cursor), |_| [1.0; 4]);
        let gil = if w.p2p.mine[0].1 == GIL && w.p2p.mine[0].2 == 0 { w.p2p.mine[0].0 } else { 0 };
        let gil_child = gu.layout(TRADE).and_then(|l| l.elements.iter().find(|e| e.id == 11).cloned());
        if let Some(e) = &gil_child {
            gu.shape_text(&crate::inventory::gil_text(gil as i64), x + e.x as f32 + e.w as f32 + 10.0, y + e.y as f32, WHITE, true);
        }
        let tint = if w.p2p.state == 4 { ORANGE } else { WHITE };
        let room = gil_child.map_or(80.0, |e| e.x as f32 - 8.0);
        clip_text(gu, &w.name, x + 4.0, y + 5.0, room, tint);
        rects.push(win.frame);
    }
    let gift_focus = u.focus == PFocus::Gift;
    if let Some(win) = gu.lobby_window_off(GIFT, sw, sh, (0.0, 0.0), gift_focus.then_some(u.gift), gift_focus, &[]) {
        let [x, y, fw, _] = win.frame;
        let slots: Vec<(u16, u32, bool)> = w.p2p.theirs[1..].iter().map(|e| if e.0 > 0 && e.1 != 0 && e.1 != GIL { (e.1, e.0, true) } else { (0, 0, false) }).collect();
        super::post::draw_slots(gu, &info, GIFT, win.frame, &slots, None, gift_focus.then_some(u.gift), |_| [1.0; 4]);
        // Their gil at the top right (inf: x + w - 14, y + 8).
        let gil = w.p2p.theirs[0].0;
        let end = gu.shape_text(&crate::inventory::gil_text(gil as i64), x + fw - 14.0, y + 8.0, WHITE, true);
        let tint = if w.p2p.partner_ok { ORANGE } else { WHITE };
        clip_text(gu, &name_of(w.p2p.partner.map(|p| p.0)), x + 4.0, y + 5.0, (end - x - 12.0).max(40.0), tint);
        rects.push(win.frame);
    }
    if let Some((_, v, max)) = u.qty
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        let [x, y, ..] = win.frame;
        gu.shape_text(&format!("{v:2}"), x + 52.0, y + 22.0, WHITE, true);
        gu.shape_text(&format!("/{max:2}"), x + 58.0, y + 22.0, WHITE, false);
        rects.push(win.frame);
    }
    if let Some((step, max, v)) = u.money
        && let Some(win) = gu.lobby_window_off(MONEYCTR, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        super::post::draw_moneyctr(gu, win.frame, step, max, v);
        rects.push(win.frame);
    }
    rects
}

/// Game-font text cut to `max` px (FUN_101ca250).
fn clip_text(gu: &mut GameUi, s: &str, x: f32, y: f32, max: f32, tint: u32) {
    let mut cut = s.len();
    while cut > 0 && gu.text_width(&s[..cut]) > max {
        cut -= 1;
    }
    gu.text(&s[..cut], x, y, 0, tint.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Handover {
        Handover { npc: (0x0100_4011, 0x11), slots: [None; 8], gil: 0, cursor: 1, focus: Focus::Window, list: (0, 0), qty: None, money: None }
    }

    /// A slot counts as offered with an inventory slot, a count and a real item (LSB's empty echo is 0xffff / 0).
    #[test]
    fn offered_slots() {
        assert!(offered(&(3, 0x1234, 5)));
        assert!(!offered(&(0, 0xffff, 0)));
        assert!(!offered(&(500, GIL, 0)));
        assert!(!offered(&(0, 0x1234, 5)));
    }

    /// Closing a trade keeps a request someone else made meanwhile and the cancel time.
    #[test]
    fn reset_keeps_incoming() {
        let mut p = P2p { state: 4, partner_ok: true, partner: Some((1, 2)), incoming: Some((3, 4)), ..Default::default() };
        p.mine[1] = (1, 0x1234, 5);
        p.reset();
        assert_eq!((p.state, p.partner_ok, p.partner, p.incoming, p.mine[1]), (0, false, None, Some((3, 4)), (0, 0, 0)));
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
