//! NPC shops (spec ~/ffxi/lab/research/shop_spec.md): s2c 0x03E opens "menu    shopmain" (Buy / Sell), 0x03C fills the
//! 80-entry stock table; the list "menu    shop" shows the stock (buy) or the inventory (sell), then "menu    itemctrl"
//! for the quantity and "menu    shopbuy" / "menu    shopsell" to confirm; c2s 0x083 buys, 0x084 appraises and 0x084 +
//! 0x085 sell. Closing sends nothing.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::ItemInfo;

const SHOPMAIN: &str = "menu    shopmain";
const SHOP: &str = "menu    shop";
const SHOPBUY: &str = "menu    shopbuy";
const SHOPSELL: &str = "menu    shopsell";
const ITEMCTRL: &str = "menu    itemctrl";
/// Rows of the list (FUN_101f6700(10)), 18 px apart from the window's (3, 4).
const ROWS: usize = 10;
/// The English list: the price column ends at 258 and the window is 18 px wider than its layout (FUN_1020f6c0).
const PRICE_END: f32 = 258.0;
const WIDE: f32 = 18.0;
/// Walking this far (x / z) from where the shop opened closes it (FUN_10210b70: 36.0 squared).
const REACH: f32 = 6.0;

/// A stock entry (table DAT_104dfd98 + 0x464c): price, ItemNo, Skill, GuildInfo.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub price: u32,
    pub item: u16,
    pub skill: u16,
    pub info: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Buy,
    Sell,
}

/// The shop's windows and the buy / sell sources (DAT_10664508..1c).
pub struct Shop {
    stock: Box<[Option<Entry>; 80]>,
    /// ShopListNum (rows listed), a complete list seen (+0x24) and a second one (+0x25), where it opened.
    count: u16,
    complete: bool,
    again: bool,
    origin: (f32, f32),
    main: u16,
    view: Option<View>,
    list: (usize, usize),
    /// Appraised unit prices by item id (the sell source's map, first entry wins; 81 at most).
    prices: Vec<(u16, u32)>,
    /// The row being bought / sold (+0x10 / +0x11): stock index or inventory slot.
    busy: Option<u8>,
    qty: Option<(u32, u32)>,
    confirm: Option<(u32, u16)>,
}

impl Shop {
    fn new(count: u16, origin: (f32, f32)) -> Self {
        Shop { stock: Box::new([None; 80]), count, complete: false, again: false, origin, main: 1, view: None, list: (0, 0), prices: Vec::new(), busy: None, qty: None, confirm: None }
    }
}

pub fn active(w: &World) -> bool {
    w.shop.is_some()
}

/// s2c 0x03E (LAB_10210ca0): refused while a window taking the keys is open (group 7 0x45); else the stock is cleared
/// and shopmain opens (cursor on Buy) where we stand.
pub(super) fn on_open(w: &mut World, count: u16) {
    if w.win.is_some() || w.confirm.is_some() || super::post::active(w) || super::synth::active(w) || w.moogle.is_some() || w.prompt.is_some() {
        super::textcmd::refuse(w, 0x45);
        return;
    }
    w.menu = None;
    w.shop = Some(Shop::new(count, (w.pos.x, w.pos.z)));
}

/// s2c 0x03C (FUN_100f8b00 / LAB_10210c70): entries by ShopIndex (a price of 0 is not listed); Flags bit 0 the last
/// packet. A second complete list without a new 0x03E closes the shop.
pub(super) fn on_list(w: &mut World, last: bool, entries: &[(u32, u16, u8, u16, u16)]) {
    let Some(s) = w.shop.as_mut() else { return };
    for &(price, item, index, skill, info) in entries {
        if let Some(e) = s.stock.get_mut(index as usize) {
            *e = (price != 0).then_some(Entry { price, item, skill, info });
        }
    }
    if last {
        if s.complete {
            s.again = true;
        }
        s.complete = true;
    }
}

/// s2c 0x03F (FUN_10210a30): 7031 0xe6 "You buy N ... from the shop." for 2 or more, else 0xe7.
pub(super) fn on_buy(w: &mut World, index: u16, count: u32) {
    let Some(item) = w.shop.as_ref().and_then(|s| s.stock.get(index as usize).copied().flatten()).map(|e| e.item) else { return };
    let id = if count >= 2 { 0xe6 } else { 0xe7 };
    super::post::system_line(w, id, [item as i32, count as i32, 0, 0, 0], String::new());
}

/// s2c 0x03D (LAB_10210aa0): Type 0 an appraisal (the slot's item takes the price); else a sale (7031 0xe8 / 0xe9).
pub(super) fn on_sell(w: &mut World, price: u32, slot: u8, kind: u8, count: u32) {
    let item = w.inv.slots.get(&(super::items::INVENTORY, slot)).map_or(0, |s| s.0);
    if kind == 0 {
        if let Some(s) = w.shop.as_mut()
            && item != 0
            && s.prices.len() < 81
        {
            s.prices.push((item, price));
        }
        return;
    }
    let id = if count >= 2 { 0xe8 } else { 0xe9 };
    super::post::system_line(w, id, [item as i32, count as i32, 0, 0, 0], String::new());
}

fn buy_rows(s: &Shop) -> Vec<(u8, Entry)> {
    s.stock.iter().enumerate().filter_map(|(i, e)| e.map(|e| (i as u8, e))).take(s.count.max(1) as usize).collect()
}

fn sell_rows(w: &World) -> Vec<crate::inventory::Slot> {
    super::post::picker_rows(w)
}

fn price_of(s: &Shop, item: u16) -> Option<u32> {
    s.prices.iter().find(|p| p.0 == item).map(|p| p.1)
}

/// The item ids we hold in the inventory (the darker price, FUN_101cba50) and wear (the green name, FUN_10131fa0:
/// containers 0, 8, 10..16).
fn held(w: &World, id: u16) -> (bool, bool) {
    let inv = w.inv.slots_of(super::items::INVENTORY).iter().any(|s| s.id == id && s.index != 0);
    let worn = [0u8, 8, 10, 11, 12, 13, 14, 15, 16].iter().any(|&c| w.inv.slots_of(c).iter().any(|s| s.id == id && s.equipped));
    (inv, worn)
}

/// FUN_10132050: the main job can wear it now. 0x119 seen; a race / job / level record (automaton items have no race);
/// the race and main-job bits; level up to the main job's (retail: the unsynced level, s2c 0x067 @0x25, not parsed;
/// 0x061's is its fallback); the superior level's low byte up to 0x061 @0x52; none of its slots encumbered (0x01B @0x60).
fn can_wear(w: &World, it: &crate::dat::items::Item, id: u16) -> bool {
    let (Some(st), Some(jobs)) = (w.stats.as_ref(), w.job_info.as_ref()) else { return false };
    w.recasts_seen
        && it.slots != 0
        && !(0x2000..0x2200).contains(&id)
        && it.races & (1 << w.my_race) != 0
        && it.jobs & (1 << st.main.0) != 0
        && it.level <= st.main.1 as u16
        && (it.superior_level & 0xff) as u8 <= st.su
        && u32::from(it.slots) & jobs.encumbrance == 0
}

/// Buy name tint (FUN_1020f740): gear green when worn, else yellow when the job can wear it now; grey past the gil (and
/// an unmet requirement); white else. Not done: the scroll / dice / attachment colours, the guild rank / job
/// requirements.
fn buy_tint(w: &World, e: &Entry) -> u32 {
    let gear = w.item_info.as_ref().and_then(|c| c.item(e.item)).filter(|it| it.level != 0);
    if e.price > w.inv.gil() as u32 {
        crate::inventory::TINT_DIM
    } else if gear.is_some() && held(w, e.item).1 {
        crate::inventory::TINT_EQUIPPED
    } else if gear.is_some_and(|it| can_wear(w, &it, e.item)) {
        0x8090_9040
    } else {
        crate::inventory::TINT_WHITE
    }
}

/// Sell name tint (FUN_1020ff40): grey for linkshells (0x201..0x203), a bazaar price (lock 0x19), worn, NoSale (0x1000);
/// orange for the row being sold.
fn sell_tint(w: &World, s: &Shop, r: &crate::inventory::Slot) -> u32 {
    if s.busy == Some(r.index) {
        return 0x8080_4020;
    }
    let flags = w.item_info.as_ref().map_or(0, |c| c.flags(r.id));
    let lock = w.inv.locks.get(&(super::items::INVENTORY, r.index)).copied().unwrap_or(0);
    if (0x201..=0x203).contains(&r.id) || lock == 0x19 || r.equipped || flags & 0x1000 != 0 {
        crate::inventory::TINT_DIM
    } else {
        crate::inventory::TINT_WHITE
    }
}

fn packet(w: &World, id: u16, body: Vec<u8>) {
    let _ = w.net.commands.send(NetCommand::Packet { id, body });
}

/// c2s 0x083 (FUN_100f8970): ItemNum, ShopNo 0, ShopItemIndex, PropertyItemIndex 0.
fn send_buy(w: &World, index: u8, qty: u32) {
    let mut b = vec![0u8; 0xc];
    b[0..4].copy_from_slice(&qty.to_le_bytes());
    b[6..8].copy_from_slice(&(index as u16).to_le_bytes());
    packet(w, 0x083, b);
}

/// c2s 0x084 (FUN_100f89d0): ItemNum, the slot's ItemNo, the slot.
fn send_sell_req(w: &World, slot: u8, qty: u32) {
    let item = w.inv.slots.get(&(super::items::INVENTORY, slot)).map_or(0, |s| s.0);
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&qty.to_le_bytes());
    b[4..6].copy_from_slice(&item.to_le_bytes());
    b[6] = slot;
    packet(w, 0x084, b);
}

/// Per frame (FUN_10210b70): a second complete list or walking off closes everything.
pub fn update(w: &mut World) {
    let Some(s) = &w.shop else { return };
    let (dx, dz) = (w.pos.x - s.origin.0, w.pos.z - s.origin.1);
    if s.again || dx * dx + dz * dz > REACH * REACH {
        w.shop = None;
    }
}

fn row_count(w: &World, s: &Shop) -> usize {
    match s.view {
        Some(View::Buy) => buy_rows(s).len(),
        Some(View::Sell) => sell_rows(w).len(),
        None => 0,
    }
}

/// Up / Down a row, scrolling at the ends and stopping at the first / last (FUN_101f6c70: no wrap); plain Right / Left
/// turn a page (+0x47 = 1) as the item lists do.
fn move_list(list: &mut (usize, usize), n: usize, key: egui::Key) -> bool {
    if n == 0 {
        return false;
    }
    if matches!(key, egui::Key::ArrowRight | egui::Key::ArrowLeft) {
        return super::items::page_keys(&mut list.0, &mut list.1, n, key == egui::Key::ArrowRight);
    }
    let (scroll, cursor) = *list;
    let at = (scroll + cursor) as isize;
    let to = match key {
        egui::Key::ArrowDown => at + 1,
        egui::Key::ArrowUp => at - 1,
        _ => at,
    }
    .clamp(0, n as isize - 1) as usize;
    if to == scroll + cursor {
        return false;
    }
    let scroll = if to < scroll { to } else if to >= scroll + ROWS { to + 1 - ROWS } else { scroll };
    *list = (scroll, to - scroll);
    true
}

pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut s) = w.shop.take() else { return };
    let mut close = false;
    if let Some((v, child)) = s.confirm {
        // shopbuy / shopsell (FUN_102110c0): links toggle; Enter answers yes on child 1; Escape no.
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                s.confirm = Some((v, if child == 1 { 2 } else { 1 }));
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Enter | egui::Key::Escape => {
                let yes = key == egui::Key::Enter && child == 1;
                if let Some(at) = s.busy.filter(|_| yes) {
                    match s.view {
                        // LAB_1020fde0: c2s 0x083.
                        Some(View::Buy) => send_buy(w, at, v),
                        // LAB_102102b0: the quantity still held, then 0x084 and 0x085 in the same frame.
                        _ => {
                            let held = w.inv.slots.get(&(super::items::INVENTORY, at)).map_or(0, |x| x.1);
                            if v <= held {
                                send_sell_req(w, at, v);
                                packet(w, 0x085, vec![1, 0, 0, 0]);
                            }
                        }
                    }
                }
                w.sounds.push(if key == egui::Key::Enter { Ui::Select as u32 } else { Ui::Cancel as u32 });
                (s.confirm, s.busy) = (None, None);
            }
            _ => {}
        }
    } else if let Some((v, max)) = s.qty {
        // itemctrl: Up / Down 1, Right 1, Left the most; Enter on to the confirm, Escape back.
        let nv = match key {
            egui::Key::ArrowUp => (v + 1).min(max),
            egui::Key::ArrowDown => v.saturating_sub(1).max(1),
            egui::Key::ArrowRight => 1,
            egui::Key::ArrowLeft => max,
            _ => v,
        };
        match key {
            egui::Key::Enter => {
                (s.qty, s.confirm) = (None, Some((v, 2)));
                w.sounds.push(Ui::Select as u32);
            }
            egui::Key::Escape => {
                (s.qty, s.busy) = (None, None);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ if nv != v => {
                s.qty = Some((nv, max));
                w.sounds.push(Ui::Cursor as u32);
            }
            _ => {}
        }
    } else if let Some(view) = s.view {
        let n = row_count(w, &s);
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                if move_list(&mut s.list, n, key) {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            egui::Key::Enter => {
                let at = s.list.0 + s.list.1;
                let ok = match view {
                    View::Buy => enter_buy(w, &mut s, at),
                    View::Sell => enter_sell(w, &mut s, at),
                };
                w.sounds.push(if ok { Ui::Select as u32 } else { Ui::Error as u32 });
            }
            // FUN_10210840: the list closes; shopmain has the keys again.
            egui::Key::Escape => {
                (s.view, s.list) = (None, (0, 0));
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    } else {
        // shopmain (FUN_10210be0): Buy / Sell; Escape closes everything.
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                s.main = if s.main == 1 { 2 } else { 1 };
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Enter => {
                s.view = Some(if s.main == 1 { View::Buy } else { View::Sell });
                s.list = (0, 0);
                w.sounds.push(Ui::Select as u32);
            }
            egui::Key::Escape => {
                close = true;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    }
    if !close {
        w.shop = Some(s);
    }
}

/// FUN_1020fc80: affordable and not grey, a free inventory slot (else group 7 0x80); the quantity up to the stack and
/// the gil (one skips itemctrl).
fn enter_buy(w: &mut World, s: &mut Shop, at: usize) -> bool {
    let Some(&(index, e)) = buy_rows(s).get(at) else { return false };
    if buy_tint(w, &e) == crate::inventory::TINT_DIM {
        return false;
    }
    let size = w.inv.sizes.first().copied().unwrap_or(0) as usize;
    let used = w.inv.slots_of(super::items::INVENTORY).iter().filter(|x| x.index != 0 && x.id != 0).count();
    if used + 1 >= size.max(1) {
        super::textcmd::refuse(w, 0x80);
        return false;
    }
    let stack = w.item_info.as_ref().map_or(1, |c| c.stack(e.item)).max(1) as u32;
    let max = if e.price > 0 { stack.min(w.inv.gil() as u32 / e.price) } else { stack }.max(1);
    s.busy = Some(index);
    if max <= 1 {
        s.confirm = Some((1, 2));
    } else {
        s.qty = Some((1, max));
    }
    true
}

/// FUN_102101a0: grey rows do nothing; an item not yet appraised is appraised (c2s 0x084 with 1); else the quantity up
/// to the slot's count.
fn enter_sell(w: &mut World, s: &mut Shop, at: usize) -> bool {
    let Some(r) = sell_rows(w).get(at).cloned() else { return false };
    if sell_tint(w, s, &r) == crate::inventory::TINT_DIM {
        return false;
    }
    if price_of(s, r.id).is_none() {
        send_sell_req(w, r.index, 1);
        return true;
    }
    s.busy = Some(r.index);
    if r.count <= 1 {
        s.confirm = Some((1, 2));
    } else {
        s.qty = Some((1, r.count));
    }
    true
}

/// The item info window's item (mode 1 at (130, 240)): the list's cursor row.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let s = w.shop.as_ref()?;
    let at = s.list.0 + s.list.1;
    match s.view? {
        View::Buy => buy_rows(s).get(at).map(|r| (r.1.item, None)),
        View::Sell => sell_rows(w).get(at).map(|r| (r.id, w.inv.ext.get(&(super::items::INVENTORY, r.index)).copied())),
    }
}

/// The bars: the list's title "Shop" (group 0xd 0xc) and help 0xe 0x2f / 0x30; itemctrl's and the confirm's own.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.shop else { return };
    let focus = |name: &str, child: u16, bars: &mut super::hudtop::BarText| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    if let Some((_, child)) = s.confirm {
        focus(if s.view == Some(View::Buy) { SHOPBUY } else { SHOPSELL }, child, bars);
    } else if s.qty.is_some() {
        bars.help[1] = gu.menu_msg(0x10c, "");
        bars.title[1] = gu.title_msg(0x3e).unwrap_or_default();
    } else if let Some(v) = s.view {
        bars.title[1] = gu.title_msg(0xc).unwrap_or_default();
        bars.help[1] = gu.menu_msg(if v == View::Buy { 0x2f } else { 0x30 }, "");
    } else {
        focus(SHOPMAIN, s.main, bars);
    }
}

/// shopmain / shopbuy / shopsell sit on the log like comyn; the list at (16, 48) 274 wide (English), money at
/// (16, 240) over shopmain, itemctrl over money.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32, log_top: f32) -> Vec<[f32; 4]> {
    const WHITE: u32 = 0x8080_8080;
    let mut rects = Vec::new();
    let (Some(s), Some(info)) = (&w.shop, w.item_info.clone()) else { return rects };
    let stacked = |gu: &GameUi, name: &str| gu.window_rect(name, sw, sh).map_or((0.0, 0.0), |r| (0.0, (log_top - r[3] - 2.0).max(16.0) - r[1]));
    let main_focus = s.view.is_none() && s.confirm.is_none();
    if s.view.is_none() || s.confirm.is_some() {
        let off = stacked(gu, SHOPMAIN);
        if s.confirm.is_none()
            && let Some(win) = gu.lobby_window_off(SHOPMAIN, sw, sh, off, Some(s.main), main_focus, &[])
        {
            rects.push(win.frame);
        }
    }
    if let Some(view) = s.view {
        let Some((wx, wy)) = gu.window_xy(SHOP, sw, sh) else { return rects };
        let size = gu.window_rect(SHOP, sw, sh).map_or([wx, wy, 256.0, 190.0], |r| r);
        let (ww, wh) = (size[2] + WIDE, size[3]);
        gu.frame(wx, wy, ww, wh);
        gu.sprite_from("menu    framesus", 82, 1, wx, wy, WHITE);
        // Rows from (3, 4), 18 px apart: the half-size icon, the name at +18 (y + 2), the price right-aligned at the
        // column's end - 2 (y + 4) in the sprite font.
        let rows: Vec<(u16, u32, String, u32)> = match view {
            View::Buy => buy_rows(s)
                .iter()
                .map(|(_, e)| {
                    let price_tint = if held(w, e.item).0 { 0x8060_6060 } else { WHITE };
                    (e.item, buy_tint(w, e), crate::inventory::gil_text(e.price as i64), price_tint)
                })
                .collect(),
            View::Sell => sell_rows(w)
                .iter()
                .map(|r| {
                    let price = price_of(s, r.id).map_or_else(|| "? G".to_string(), |p| crate::inventory::gil_text(p as i64));
                    (r.id, sell_tint(w, s, r), price, WHITE)
                })
                .collect(),
        };
        super::items::upload_icons(gu, &info, rows.iter().skip(s.list.0).take(ROWS).map(|r| r.0));
        let focused = s.qty.is_none() && s.confirm.is_none();
        for (k, (id, tint, price, ptint)) in rows.iter().skip(s.list.0).take(ROWS).enumerate() {
            let (rx, ry) = (wx + 3.0, wy + 4.0 + 18.0 * k as f32);
            let icon = crate::render::invwin::icon_texture(*id);
            if gu.has_image(&icon) {
                gu.image_rect(&icon, rx, ry, 16.0, 16.0, [1.0; 4]);
            }
            let name = info.name(*id).unwrap_or_default();
            gu.text(&name, rx + 18.0, ry + 2.0, 0, tint.to_le_bytes());
            gu.shape_text(price, rx + PRICE_END - 2.0, ry + 4.0, *ptint, true);
            if focused && k == s.list.1 {
                gu.cursor(rx, ry);
            }
        }
        rects.push([wx, wy, ww, wh]);
        // money (FUN_101408c0) with the list.
        let gil = crate::inventory::gil_text(w.inv.gil() as i64);
        let (b, ui, style, _) = gu.parts();
        if let Some(m) = ui.menu("menu    money") {
            crate::render::invwin::push_money(b, ui, style, m, &gil, sw, sh);
        }
        rects.extend(gu.window_rect("menu    money", sw, sh));
    }
    if let Some((v, max)) = s.qty
        && let Some(win) = gu.lobby_window_off(ITEMCTRL, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        let [x, y, ..] = win.frame;
        gu.shape_text(&format!("{v:2}"), x + 52.0, y + 22.0, WHITE, true);
        gu.shape_text(&format!("/{max:2}"), x + 58.0, y + 22.0, WHITE, false);
        rects.push(win.frame);
    }
    if let Some((_, child)) = s.confirm {
        let name = if s.view == Some(View::Buy) { SHOPBUY } else { SHOPSELL };
        let off = stacked(gu, name);
        if let Some(win) = gu.lobby_window_off(name, sw, sh, off, Some(child), true, &[]) {
            rects.push(win.frame);
        }
    }
    rects
}
