//! Main menu social row 5 Set Bazaar (spec ~/ffxi/lab/research/social_rows_spec.md part B): "menu    bazaar" (vtable
//! 0x10338e18: open FUN_101dcf80, messages FUN_101dcfa0) with Set Prices (the shop list's price source, shop.rs) and
//! Edit Comment (the comment editor in mode 0, seacom.rs); and the seller's lines (s2c 0x108 / 0x10A).
//!
//! The buyer's side (spec ~/ffxi/lab/research/bazaar_spec.md): the wares store another player's bazaar fills (s2c
//! 0x105), the purchase result (0x106), the bazaar closing under us (0x107) and other customers' purchases (0x109); the
//! list itself is the shop list's bazaar source (shop.rs View::Wares).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const WIN: &str = "menu    bazaar";

pub struct Bazaar {
    pub sel: u16,
    /// menuwind's cursor, back when the window closes (socialme resumes).
    return_menu: Option<usize>,
}

/// The row opens the window over the main menu (SE 0002).
pub fn open(w: &mut World) {
    w.bazaar = Some(Bazaar { sel: 1, return_menu: w.menu });
    w.menu = None;
    w.sounds.push(Ui::Select as u32);
}

fn close(w: &mut World) {
    if let Some(b) = w.bazaar.take() {
        w.menu = b.return_menu;
    }
}

/// The window has the keys unless the price list or the editor is up over it.
pub fn focused(w: &World) -> bool {
    w.bazaar.is_some() && w.shop.is_none() && w.seacom_edit.is_none() && w.confirm.is_none()
}

/// The generic keys (links 1 <-> 2), Enter SE 0002 then: child 1 the price list (FUN_102107d0 on the price source),
/// child 2 the editor once our own message came (s2c 0x0CA MyFlag), else SE 0004 too; Escape SE 0003 and close.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(sel) = w.bazaar.as_ref().map(|b| b.sel) else { return };
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            if let Some(b) = w.bazaar.as_mut() {
                b.sel = 3 - sel;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            if sel == 1 {
                super::shop::open_bazaar(w);
            } else if w.bazaar_msg.own_in {
                super::seacom::open_bazaar(w);
            } else {
                w.sounds.push(Ui::Error as u32);
            }
        }
        _ => {}
    }
}

/// Mouse: hover over a row moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(sel) = w.bazaar.as_ref().map(|b| b.sel) else { return };
    let Some((wx, wy)) = gu.window_xy(WIN, sw, sh) else { return };
    let Some(layout) = gu.layout(WIN) else { return };
    let hit = layout.elements.iter().skip(1).filter(|e| matches!(e.id, 1 | 2)).find(|e| {
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
    });
    let Some(e) = hit else { return };
    gu.hover.set(true);
    if (moved || clicked) && e.id != sel
        && let Some(b) = w.bazaar.as_mut()
    {
        b.sel = e.id;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter);
    }
}

/// The window as the modern page draws it.
pub(super) fn view(w: &World) -> Option<super::modern::MenuWin> {
    let b = w.bazaar.as_ref()?;
    Some(super::modern::MenuWin { name: WIN, sel: b.sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() })
}

/// The window, unfocused while the list or the editor is up (it stays where it is: group 0x81 shares nothing with them).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let b = w.bazaar.as_ref()?;
    gu.lobby_window_off(WIN, sw, sh, (0.0, 0.0), Some(b.sel), focused(w), &[]).map(|win| win.frame)
}

pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(b) = w.bazaar.as_ref().filter(|_| focused(w)) else { return };
    let [help, title] = gu.focus_text(WIN, b.sel);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

/// s2c 0x108 (LAB_101dcd80): State 0 group 10 0x0e "%s begins to browse ...", a customer more; else 0x0f "%s leaves your
/// bazaar.", one fewer (not below 0); printed with HideLevel 0 and the system message filter (0x0B4 word bits 11..12)
/// under 2, in mode 0x79.
pub fn on_shopper(w: &mut World, state: u32, hide: u16, name: String) {
    if state == 0 {
        w.bazaar_customers += 1;
    } else {
        w.bazaar_customers = (w.bazaar_customers - 1).max(0);
    }
    let filter = u32::from_le_bytes(w.conf_sys[0..4].try_into().unwrap()) >> 11 & 3;
    if hide != 0 || filter >= 2 {
        return;
    }
    let Some(bt) = w.battle.clone() else { return };
    let line = bt.group10(if state == 0 { 0x0e } else { 0x0f }).unwrap_or_default().replacen("%s", &name, 1);
    w.chat.push_colour(line, bt.colour_in(0x79, &w.game));
}

/// s2c 0x10A (LAB_100e0500): file 7031 line 0x5c with the item, the count and the buyer's name.
pub fn on_sale(w: &mut World, count: u32, item: u16, name: String) {
    super::post::system_line(w, 0x5c, [item as i32, count as i32, 0, 0, 0], name);
}

/// One record of the bazaar we browse (G+0x3aeb0 + index x 0x30): the item, its index (the seller's slot), the count,
/// the unit price, the extdata and the tax rate.
#[derive(Clone, Copy, Debug)]
pub struct Ware {
    pub item: u16,
    pub index: u8,
    pub count: u32,
    pub price: u32,
    pub attr: [u8; 24],
    pub tax: u16,
}

/// The wares store: 81 records by the seller's slot, whose bazaar it is (UniqueNo, ActIndex: G+0x3bde0 / 0x3bde4) and
/// the last tax rate a row with a count brought (G+0x3bde6, kept across clears).
pub struct Wares {
    recs: Box<[Option<Ware>; 81]>,
    pub from: (u32, u16),
    pub tax: u16,
}

impl Default for Wares {
    fn default() -> Self {
        Wares { recs: Box::new([None; 81]), from: (0, 0), tax: 0 }
    }
}

impl Wares {
    /// The 0x3cc dwords from G+0x3aeb0 (the records and the bazaar's ids).
    pub fn clear(&mut self) {
        (self.recs, self.from) = (Box::new([None; 81]), (0, 0));
    }

    /// FUN_100e0300: a record counts with an item, its own index, a count and a price.
    pub fn get(&self, i: usize) -> Option<&Ware> {
        self.recs.get(i)?.as_ref().filter(|r| r.item != 0 && r.index as usize == i && r.count != 0 && r.price != 0)
    }

    /// The valid records in slot order (1..80), as the list's rows.
    pub fn rows(&self) -> Vec<Ware> {
        (1..81).filter_map(|i| self.get(i).copied()).collect()
    }
}

/// FUN_100fd710 with n = 1: the tax on a unit, at least 1 when there is a rate.
pub fn tax_of(price: u32, rate: u16) -> u32 {
    if rate == 0 { 0 } else { (price as u64 * rate as u64 / 10000).max(1) as u32 }
}

/// FUN_100fd820 with n = 1: a unit's price with the tax.
pub fn unit_of(r: &Ware) -> u32 {
    r.price.saturating_add(tax_of(r.price, r.tax))
}

/// The system message filter (the 0x0B4 word, bits 11..12).
fn sys_filter(w: &World) -> u32 {
    u32::from_le_bytes(w.conf_sys[0..4].try_into().unwrap()) >> 11 & 3
}

/// A group 9 line with the name, in mode 0x79.
fn line9(w: &mut World, id: usize, name: &str) {
    let Some(bt) = w.battle.clone() else { return };
    let line = bt.group9(id).unwrap_or_default().replacen("%s", name, 1);
    w.chat.push_colour(line, bt.colour_in(0x79, &w.game));
}

/// s2c 0x105 (FUN_100e0370): the record at ItemIndex rewritten (an item 0 or count 0 empties it); a row with a count
/// keeps its tax rate.
pub fn on_item(w: &mut World, r: Ware) {
    if let Some(slot) = w.wares.recs.get_mut(r.index as usize) {
        *slot = Some(r);
    }
    if r.count != 0 {
        w.wares.tax = r.tax;
    }
}

/// s2c 0x106 (LAB_101dcc90), with the system message filter under 2: State 0 group 9 0x28 "You made a purchase from %s's
/// bazaar."; else group 7 0x7c.
pub fn on_buy(w: &mut World, state: u32, name: String) {
    if sys_filter(w) >= 2 {
        return;
    }
    if state == 0 {
        line9(w, 0x28, &name);
    } else {
        super::textcmd::refuse(w, 0x7c);
    }
}

/// s2c 0x107 (LAB_100e0400 / LAB_101dcd10): the store cleared, the list closed (c2s 0x104), View Wares disabled, then
/// group 9 0x29 under the filter.
pub fn on_closed(w: &mut World, name: String) {
    w.wares.clear();
    super::shop::close_wares(w);
    w.inspect.bazaar = false;
    if sys_filter(w) < 2 {
        line9(w, 0x29, &name);
    }
}

/// s2c 0x109 (LAB_100e0430): unless it is our own bazaar, file 7031 line 0x5d with the record's item, the count and the
/// buyer; the record's count goes down when it is the bazaar we browse (LSB never fills that index).
pub fn on_sold(w: &mut World, count: u32, owner: u16, name: String, index: u8) {
    let item = w.wares.recs.get(index as usize).and_then(|r| r.as_ref()).map_or(0, |r| r.item);
    if owner != w.my_index {
        super::post::system_line(w, 0x5d, [item as i32, count as i32, 0, 0, 0], name);
    }
    if owner == w.wares.from.1
        && let Some(slot) = w.wares.recs.get_mut(index as usize)
        && let Some(r) = slot.as_mut()
    {
        r.count = r.count.wrapping_sub(count);
        if r.count == 0 {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FUN_100e0300's validity (an item, its own index, a count and a price) and FUN_100fd710's tax (at least 1).
    #[test]
    fn wares_records_and_tax() {
        let mut s = Wares::default();
        let r = |item, index, count, price| Some(Ware { item, index, count, price, attr: [0; 24], tax: 0 });
        s.recs[3] = r(0x1001, 3, 12, 100);
        s.recs[5] = r(0x1002, 6, 1, 100);
        s.recs[7] = r(0x1003, 7, 0, 100);
        s.recs[9] = r(0x1004, 9, 1, 0);
        s.recs[12] = r(0x1005, 12, 2, 50);
        assert_eq!(s.rows().iter().map(|r| r.index).collect::<Vec<_>>(), [3, 12]);
        assert_eq!((tax_of(100, 0), tax_of(100, 500), tax_of(10, 50), tax_of(999, 1000)), (0, 5, 1, 99));
        let mut w = s.rows()[0];
        w.tax = 500;
        assert_eq!(unit_of(&w), 105);
        s.clear();
        assert!(s.rows().is_empty() && s.from == (0, 0));
    }
}
