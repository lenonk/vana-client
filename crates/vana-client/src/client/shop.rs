//! NPC shops (spec ~/ffxi/lab/research/shop_spec.md): s2c 0x03E opens "menu    shopmain" (Buy / Sell), 0x03C fills the
//! 80-entry stock table; the list "menu    shop" shows the stock (buy) or the inventory (sell), then "menu    itemctrl"
//! for the quantity and "menu    shopbuy" / "menu    shopsell" to confirm; c2s 0x083 buys, 0x084 appraises and 0x084 +
//! 0x085 sell. Closing sends nothing.
//!
//! Guild shops (~/ffxi/lab/research/guild_shop_spec.md) use the same windows: s2c 0x086 opens "menu    guildsho", Buy
//! asks for the list (c2s 0x0AB, s2c 0x083), Sell for the guild's wanted list (0x0AD, 0x085), whose rows lead to the
//! inventory; c2s 0x0AA buys, 0x0AC sells, s2c 0x082 / 0x084 answer.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::ItemInfo;
use std::time::{Duration, Instant};

const SHOPMAIN: &str = "menu    shopmain";
const GUILDSHO: &str = "menu    guildsho";
const SHOP: &str = "menu    shop";
const SHOPBUY: &str = "menu    shopbuy";
const SHOPSELL: &str = "menu    shopsell";
const ITEMCTRL: &str = "menu    itemctrl";
/// Rows of the list (FUN_101f6700(10)), 18 px apart from the window's (3, 4).
const ROWS: usize = 10;
/// The English list: the price column ends at 258 and the window is 18 px wider than its layout (FUN_1020f6c0).
const PRICE_END: f32 = 258.0;
const WIDE: f32 = 18.0;
/// The requirement column: 0x69 wide, its cells ending at window + 0x16e.
const REQ_W: f32 = 105.0;
const REQ_END: f32 = 366.0;
/// Walking this far (x / z) from where the shop opened closes it (FUN_10210b70 / FUN_101ed6d0: 36.0 squared).
const REACH: f32 = 6.0;
const WHITE: u32 = 0x8080_8080;
const ORANGE: u32 = 0x8080_4020;
const YELLOW: u32 = 0x8090_9040;

/// A stock entry (table DAT_104dfd98 + 0x464c): price, ItemNo, Skill, GuildInfo.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub price: u32,
    pub item: u16,
    pub skill: u16,
    pub info: u16,
}

/// A guild list entry (GP_GUILD_ITEM): the guild's stock now, its ceiling, the unit price (bit 31: hidden).
#[derive(Clone, Copy, Debug)]
pub struct GuildItem {
    pub item: u16,
    pub count: u8,
    pub max: u8,
    pub price: u32,
}

/// A guild list table (DAT_104dfd98 + 0x19de4, k 0 buy / 1 sell): kept across windows, cleared by 0x086 only.
#[derive(Default)]
pub struct GuildTable {
    /// 0 never, 3 loading, 1 complete.
    state: u8,
    items: Vec<GuildItem>,
    /// The last packet with 0x40 (the 5 s request rule).
    stamp: Option<Instant>,
}

impl GuildTable {
    /// FUN_100eb1b0: the count once complete.
    fn count(&self) -> Option<usize> {
        (self.state == 1).then_some(self.items.len())
    }

    fn find(&self, id: u16) -> Option<&GuildItem> {
        self.items.iter().find(|g| g.item == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Buy,
    Sell,
    /// The guild's wanted list (its sell table, row 0 "Select from your inventory.").
    Wanted,
    /// The inventory for the guild, only the item with this id (0 all).
    Inv(u16),
}

/// The shop's windows and the buy / sell sources (DAT_10664508..1c; the guild's DAT_106636ac..b8).
pub struct Shop {
    guild: bool,
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
    /// The row being bought / sold (+0x10 / +0x11): stock index, guild row or inventory slot.
    busy: Option<u8>,
    /// The wanted row the inventory view was opened from (the cursor goes back there).
    wanted_at: usize,
    qty: Option<(u32, u32)>,
    confirm: Option<(u32, u16)>,
}

impl Shop {
    fn new(guild: bool, count: u16, origin: (f32, f32)) -> Self {
        Shop {
            guild,
            stock: Box::new([None; 80]),
            count,
            complete: false,
            again: false,
            origin,
            main: 1,
            view: None,
            list: (0, 0),
            prices: Vec::new(),
            busy: None,
            wanted_at: 0,
            qty: None,
            confirm: None,
        }
    }

    fn main_window(&self) -> &'static str {
        if self.guild { GUILDSHO } else { SHOPMAIN }
    }
}

/// A list row as drawn: icon and name by id (or the given text), the name's tint, the price cell, the requirement cell
/// (NPC buy rows only; drawn in the name's tint).
struct Row {
    id: u16,
    text: Option<String>,
    tint: u32,
    price: Option<String>,
    price_tint: u32,
    req: Option<Req>,
}

/// A stock entry's requirement (FUN_1020f8a0): a craft rank, a job level, or a line of group 0xc.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Req {
    /// Skill 0x30..0x3a and GuildInfo / 100 (the rank + 1), only above 1.
    Rank { skill: u16, need: u16 },
    /// Skill 0x40 + job and the level (GuildInfo low byte), only for jobs above WAR.
    Job { job: u8, level: u8 },
    /// Skill 0x80: group 0xc 0x11d + GuildInfo ("New wares!").
    Text(u16),
}

pub fn active(w: &World) -> bool {
    w.shop.is_some()
}

/// The windows taking the keys that refuse a new shop (registry flag 0x20000, the shops included).
fn window_open(w: &World) -> bool {
    w.shop.is_some() || w.win.is_some() || w.confirm.is_some() || super::post::active(w) || super::synth::active(w) || w.moogle.is_some() || w.prompt.is_some()
}

/// s2c 0x03E (LAB_10210ca0): refused while a window taking the keys is open (group 7 0x45); else the stock is cleared
/// and shopmain opens (cursor on Buy) where we stand.
pub(super) fn on_open(w: &mut World, count: u16) {
    if window_open(w) {
        super::textcmd::refuse(w, 0x45);
        return;
    }
    w.menu = None;
    w.shop = Some(Shop::new(false, count, (w.pos.x, w.pos.z)));
}

/// s2c 0x03C (FUN_100f8b00 / LAB_10210c70): entries by ShopIndex (a price of 0 is not listed); Flags bit 0 the last
/// packet. A second complete list without a new 0x03E closes the shop.
pub(super) fn on_list(w: &mut World, last: bool, entries: &[(u32, u16, u8, u16, u16)]) {
    let Some(s) = w.shop.as_mut().filter(|s| !s.guild) else { return };
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

/// A line in chat mode 0x79 (FUN_10129e80).
fn line79(w: &mut World, text: String) {
    let Some(bt) = w.battle.clone() else { return };
    let colour = bt.colour_in(0x79, &w.game);
    w.chat.push_colour(text, colour);
}

/// s2c 0x086 (store LAB_100eae80, UI LAB_101ed810): both lists' counts go to 0 first. Stat 0 opens guildsho (refused as
/// 0x03E); 1 closed: group 10 0x19 with the hours (the lowest open bit, the first closed bit after it), 2 holiday:
/// group 10 0x1a with the weekday; both silent when bit 31 is set and no guild window is open, then everything closes.
pub(super) fn on_guild_open(w: &mut World, stat: u8, time: u32) {
    for t in w.guild.iter_mut() {
        t.items.clear();
    }
    let open = w.shop.as_ref().is_some_and(|s| s.guild);
    if stat == 0 {
        if window_open(w) {
            super::textcmd::refuse(w, 0x45);
            return;
        }
        w.menu = None;
        w.shop = Some(Shop::new(true, 0, (w.pos.x, w.pos.z)));
        return;
    }
    if !open && time & 0x8000_0000 != 0 {
        return;
    }
    let bt = w.battle.clone();
    let line = match stat {
        1 => (0..24).find(|h| time & (1 << h) != 0).and_then(|from| {
            let to = (from..24).find(|h| time & (1 << h) == 0).unwrap_or(24);
            Some(bt.as_ref()?.group10(0x19)?.replacen("%d", &from.to_string(), 1).replacen("%d", &to.to_string(), 1))
        }),
        2 if time & 0x7fff_ffff < 8 => bt.as_ref().and_then(|b| Some(b.group10(0x1a)?.replacen("%s", &b.day_name((time & 7) as usize)?, 1))),
        _ => None,
    };
    if let Some(l) = line {
        line79(w, l);
    }
    if open {
        w.shop = None;
    }
}

/// s2c 0x083 / 0x085 (FUN_100eaf00): Stat 0x40 starts the table (and the 5 s timer), entries are appended unless the id
/// is already there, 0x80 completes it. An open list fills in as they come.
pub(super) fn on_guild_list(w: &mut World, sell: bool, stat: u8, entries: &[(u16, u8, u8, u32)]) {
    let t = &mut w.guild[sell as usize];
    if stat & 0x40 != 0 {
        t.items.clear();
        t.stamp = Some(Instant::now());
        t.state = 3;
    }
    for &(item, count, max, price) in entries {
        if t.find(item).is_none() {
            t.items.push(GuildItem { item, count, max, price });
        }
    }
    if stat & 0x80 != 0 {
        t.state = 1;
    }
}

/// s2c 0x082 / 0x084 (FUN_100eafc0 / FUN_100eb060; UI LAB_101ec660 / LAB_101ec750): the entry's stock takes Count; then
/// the result line: refusals in group 7, trades in 7031 (0xa5 / 0xa6 bought, 0xa7 / 0xa8 sold) and a partial one's
/// group 7 0x83 / 0x84 after.
pub(super) fn on_guild_result(w: &mut World, sell: bool, item: u16, count: u8, trade: i8) {
    if let Some(g) = w.guild[sell as usize].items.iter_mut().find(|g| g.item == item) {
        g.count = count;
    }
    let refuse = |w: &mut World, id: usize| super::textcmd::refuse(w, id);
    if item == 0 {
        let id = match (sell, trade as u8) {
            (false, 0xfb) => 0xa3,
            (false, 0xfd) => 0x9a,
            _ => 0x9b,
        };
        refuse(w, id);
        return;
    }
    if trade == 0 {
        refuse(w, if sell { 0x82 } else { 0x81 });
        return;
    }
    let n = trade.unsigned_abs() as i32;
    let id = match (sell, n >= 2) {
        (false, true) => 0xa5,
        (false, false) => 0xa6,
        (true, true) => 0xa7,
        (true, false) => 0xa8,
    };
    super::post::system_line(w, id, [item as i32, n, 0, 0, 0], String::new());
    if trade < 0 {
        refuse(w, if sell { 0x84 } else { 0x83 });
    }
}

fn buy_rows(s: &Shop) -> Vec<(u8, Entry)> {
    s.stock.iter().enumerate().filter_map(|(i, e)| e.map(|e| (i as u8, e))).take(s.count.max(1) as usize).collect()
}

/// The inventory rows (FUN_1020ffd0 / FUN_101ed150): none until the inventory has settled (flags 0x800 or 0x200000).
fn sell_rows(w: &World) -> Vec<crate::inventory::Slot> {
    if w.inv_flags & 0x0020_0800 == 0 {
        return Vec::new();
    }
    super::post::picker_rows(w)
}

fn price_of(s: &Shop, item: u16) -> Option<u32> {
    s.prices.iter().find(|p| p.0 == item).map(|p| p.1)
}

/// The guild's wanted entries: the sell table without the hidden ones (price bit 31).
fn wanted(w: &World) -> Vec<GuildItem> {
    w.guild[1].items.iter().filter(|g| g.price & 0x8000_0000 == 0).copied().collect()
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

/// A scroll's spell (FUN_1022b680 / b570): the first spell with its name that is not blue magic (kind 6).
fn scroll_spell(w: &World, arg: u16) -> Option<u16> {
    let (names, acts) = (w.names.as_ref()?, w.actions.as_ref()?);
    if acts.spell(arg).is_some_and(|s| s.kind != 6) {
        return Some(arg);
    }
    let name = crate::dat::names::NameSource::spell(&**names, arg as u32)?;
    acts.spells.iter().filter(|s| s.id != 0 && s.kind != 6).find(|s| crate::dat::names::NameSource::spell(&**names, s.id as u32).is_some_and(|n| n == name)).map(|s| s.id)
}

/// The buy name colour (FUN_1020f740 / FUN_101ec800): gear green when worn, else yellow when the job can wear it now;
/// a scroll (type 7, teach kind 1 at record +0x1554, spell at +0x1558) green when learned (s2c 0x0AA), else yellow when
/// the main or sub job can learn it (FUN_1022b7d0; job points not parsed); white else. Not done: dice (kind 2, green
/// only with Monstrosity data, which LSB never sends) and automaton attachments (kind 3, s2c 0x044).
fn name_tint(w: &World, id: u16) -> u32 {
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return WHITE };
    let extra = |at: u16| it.extra.iter().find(|e| e.0 == at).map(|e| e.1);
    if it.level != 0 {
        if held(w, id).1 {
            return crate::inventory::TINT_EQUIPPED;
        }
        if can_wear(w, &it, id) {
            return YELLOW;
        }
    } else if it.item_type == 7
        && extra(0xd54) == Some(1)
        && let Some(spell) = extra(0xd58).and_then(|a| scroll_spell(w, a as u16))
    {
        if w.spells_known.get(spell as usize / 8).is_some_and(|b| b & (1 << (spell % 8)) != 0) {
            return crate::inventory::TINT_EQUIPPED;
        }
        if let (Some(acts), Some(st)) = (w.actions.as_ref(), w.stats.as_ref())
            && acts.can_cast(spell, st.main, st.sub, 0)
        {
            return YELLOW;
        }
    }
    WHITE
}

fn requirement(e: &Entry) -> Option<Req> {
    match e.skill {
        0x30..=0x3a if e.info / 100 > 1 => Some(Req::Rank { skill: e.skill, need: e.info / 100 }),
        0x40..=0x57 if e.skill - 0x40 > 1 => Some(Req::Job { job: (e.skill - 0x40) as u8, level: (e.info & 0xff) as u8 }),
        0x80 => Some(Req::Text(e.info)),
        _ => None,
    }
}

/// FUN_1020f8a0's test: the craft rank (s2c 0x062 skill_base & 0x1f, after 0x119; 0x8000 none) at least the needed
/// rank - 1; the job's level (s2c 0x01B) at least the needed one.
fn req_met(w: &World, r: Req) -> bool {
    match r {
        Req::Rank { skill, need } => {
            let word = w.skill_base.and_then(|b| b.get(skill as usize).copied()).filter(|&v| w.recasts_seen && v != 0x8000);
            word.is_some_and(|v| v & 0x1f >= need - 1)
        }
        Req::Job { job, level } => w.job_info.as_ref().and_then(|j| j.levels.get(job as usize)).is_some_and(|&l| l >= level),
        Req::Text(_) => true,
    }
}

/// NPC buy rows: the name colour, grey past the gil or with an unmet requirement.
fn buy_tint(w: &World, e: &Entry) -> u32 {
    if e.price > w.inv.gil() as u32 || requirement(e).is_some_and(|r| !req_met(w, r)) {
        crate::inventory::TINT_DIM
    } else {
        name_tint(w, e.item)
    }
}

/// Guild buy rows (FUN_101ec920): the name colour, orange on the busy row, grey past the gil or out of stock.
fn guild_buy_tint(w: &World, s: &Shop, row: usize, g: &GuildItem) -> u32 {
    if g.price & 0x7fff_ffff > w.inv.gil() as u32 || g.count == 0 {
        crate::inventory::TINT_DIM
    } else if s.busy == Some(row as u8) {
        ORANGE
    } else {
        name_tint(w, g.item)
    }
}

/// Inventory rows that can never be sold: linkshells (0x201..0x203), a bazaar price (lock 0x19), worn, NoSale (0x1000).
fn unsellable(w: &World, r: &crate::inventory::Slot) -> bool {
    let flags = w.item_info.as_ref().map_or(0, |c| c.flags(r.id));
    let lock = w.inv.locks.get(&(super::items::INVENTORY, r.index)).copied().unwrap_or(0);
    (0x201..=0x203).contains(&r.id) || lock == 0x19 || r.equipped || flags & 0x1000 != 0
}

/// Sell name tint (FUN_1020ff40 / guild FUN_101ed090): orange for the row being sold; grey when unsellable (or, for the
/// guild, not on its list at all).
fn sell_tint(w: &World, s: &Shop, r: &crate::inventory::Slot) -> u32 {
    if s.busy == Some(r.index) {
        ORANGE
    } else if unsellable(w, r) || (s.guild && w.guild[1].find(r.id).is_none()) {
        crate::inventory::TINT_DIM
    } else {
        WHITE
    }
}

/// The guild's inventory rows: with a filter only that item (FUN_101ed090's 0 leaves the others out).
fn inv_rows(w: &World, filter: u16) -> Vec<crate::inventory::Slot> {
    sell_rows(w).into_iter().filter(|r| filter == 0 || r.id == filter).collect()
}

/// The rows the list shows now (rebuilt every frame, as retail's sources are).
fn rows(w: &World, s: &Shop) -> Vec<Row> {
    let gil = |p: u32| Some(crate::inventory::gil_text(p as i64));
    let price_tint = |id: u16| if held(w, id).0 { 0x8060_6060 } else { WHITE };
    match s.view {
        None => Vec::new(),
        Some(View::Buy) if s.guild => match w.guild[0].count() {
            Some(_) => w.guild[0]
                .items
                .iter()
                .enumerate()
                .map(|(k, g)| Row { id: g.item, text: None, tint: guild_buy_tint(w, s, k, g), price: gil(g.price & 0x7fff_ffff), price_tint: price_tint(g.item), req: None })
                .collect(),
            None => Vec::new(),
        },
        Some(View::Buy) => buy_rows(s).iter().map(|(_, e)| Row { id: e.item, text: None, tint: buy_tint(w, e), price: gil(e.price), price_tint: price_tint(e.item), req: requirement(e) }).collect(),
        Some(View::Sell) => sell_rows(w)
            .iter()
            .map(|r| Row { id: r.id, text: None, tint: sell_tint(w, s, r), price: Some(price_of(s, r.id).map_or_else(|| "? G".to_string(), |p| crate::inventory::gil_text(p as i64))), price_tint: WHITE, req: None })
            .collect(),
        // FUN_101ecd10: nothing while loading; row 0 group 9 0x2c, then the entries, grey unless held and wanted.
        Some(View::Wanted) => {
            if w.guild[1].count().is_none() {
                return Vec::new();
            }
            let first = w.battle.as_ref().and_then(|b| b.group9(0x2c)).unwrap_or_default();
            std::iter::once(Row { id: 0, text: Some(first), tint: WHITE, price: None, price_tint: WHITE, req: None })
                .chain(wanted(w).iter().map(|g| {
                    let ok = held(w, g.item).0 && g.count < g.max;
                    Row { id: g.item, text: None, tint: if ok { WHITE } else { crate::inventory::TINT_DIM }, price: gil(g.price), price_tint: WHITE, req: None }
                }))
                .collect()
        }
        // FUN_101ed150: the price only for items on the guild's list (hidden ones too).
        Some(View::Inv(filter)) => inv_rows(w, filter)
            .iter()
            .map(|r| Row { id: r.id, text: None, tint: sell_tint(w, s, r), price: w.guild[1].find(r.id).and_then(|g| gil(g.price & 0x7fff_ffff)), price_tint: WHITE, req: None })
            .collect(),
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

/// c2s 0x0AA / 0x0AC (FUN_100eb230 / FUN_100eb270): ItemNo, slot (0 to buy), quantity; only once the table is
/// complete (FUN_100eb1e0).
fn send_guild(w: &World, sell: bool, item: u16, slot: u8, qty: u32) {
    if w.guild[sell as usize].state != 1 {
        return;
    }
    let mut b = vec![0u8; 4];
    b[0..2].copy_from_slice(&item.to_le_bytes());
    b[2] = slot;
    b[3] = qty.min(255) as u8;
    packet(w, if sell { 0x0AC } else { 0x0AA }, b);
}

/// FUN_100eb0b0: asks for list k (c2s 0x0AB buy / 0x0AD sell, no body) unless the last one began within 5 s.
fn request_guild(w: &World, sell: bool) -> bool {
    if w.guild[sell as usize].stamp.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
        return false;
    }
    packet(w, if sell { 0x0AD } else { 0x0AB }, Vec::new());
    true
}

/// Per frame (FUN_10210b70 / FUN_101ed6d0): walking off (or an NPC shop's second complete list) closes everything.
pub fn update(w: &mut World) {
    let Some(s) = &w.shop else { return };
    let (dx, dz) = (w.pos.x - s.origin.0, w.pos.z - s.origin.1);
    if s.again || dx * dx + dz * dz > REACH * REACH {
        w.shop = None;
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

/// The cursor on row `at` (FUN_101f7080).
fn list_at(at: usize) -> (usize, usize) {
    let scroll = (at + 1).saturating_sub(ROWS);
    (scroll, at - scroll)
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
                let held = |w: &World, at: u8| w.inv.slots.get(&(super::items::INVENTORY, at)).map_or((0, 0), |x| (x.0, x.1));
                if let Some(at) = s.busy.filter(|_| yes) {
                    match s.view {
                        // LAB_101eccc0: c2s 0x0AA.
                        Some(View::Buy) if s.guild => {
                            if let Some(g) = w.guild[0].items.get(at as usize).copied() {
                                send_guild(w, false, g.item, 0, v);
                            }
                        }
                        // LAB_1020fde0: c2s 0x083.
                        Some(View::Buy) => send_buy(w, at, v),
                        // LAB_101ed3e0: the quantity still held, then c2s 0x0AC; the list stays on the inventory.
                        Some(View::Inv(_)) => {
                            let (id, n) = held(w, at);
                            if v <= n {
                                send_guild(w, true, id, at, v);
                            }
                        }
                        // LAB_102102b0: the quantity still held, then 0x084 and 0x085 in the same frame.
                        _ => {
                            if v <= held(w, at).1 {
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
        let n = rows(w, &s).len();
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                if move_list(&mut s.list, n, key) {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            egui::Key::Enter => {
                let at = s.list.0 + s.list.1;
                let ok = match view {
                    View::Buy if s.guild => enter_guild_buy(w, &mut s, at),
                    View::Buy => enter_buy(w, &mut s, at),
                    View::Sell => enter_sell(w, &mut s, at),
                    View::Wanted => enter_wanted(w, &mut s, at),
                    View::Inv(filter) => enter_inv(w, &mut s, filter, at),
                };
                w.sounds.push(if ok { Ui::Select as u32 } else { Ui::Error as u32 });
            }
            // The inventory goes back to the wanted list on its row (FUN_101ed430 / FUN_101ed590); the others close
            // (FUN_10210840), the main window has the keys again.
            egui::Key::Escape => {
                if matches!(view, View::Inv(_)) {
                    (s.view, s.list) = (Some(View::Wanted), list_at(s.wanted_at));
                } else {
                    (s.view, s.list, s.wanted_at) = (None, (0, 0), 0);
                }
                s.busy = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    } else {
        // shopmain (FUN_10210be0) / guildsho (FUN_101ed730): Buy / Sell; Escape closes everything.
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                s.main = if s.main == 1 { 2 } else { 1 };
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Enter if s.guild => {
                // A list is fetched once per 0x086: an empty one asks again, refused within 5 s of the last (SE 0004).
                let sell = s.main == 2;
                let ok = w.guild[sell as usize].count().is_some_and(|n| n >= 1) || request_guild(w, sell);
                if ok {
                    s.view = Some(if sell { View::Wanted } else { View::Buy });
                    s.list = (0, 0);
                }
                w.sounds.push(if ok { Ui::Select as u32 } else { Ui::Error as u32 });
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

/// The quantity window up to `max` (one skips it, straight to the confirm).
fn ask_qty(s: &mut Shop, max: u32) {
    if max <= 1 {
        s.confirm = Some((1, 2));
    } else {
        s.qty = Some((1, max));
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
    ask_qty(s, max);
    true
}

/// FUN_101ecba0: affordable, in stock and a known item (no free-slot check); the quantity up to the stack, the gil and
/// the guild's stock.
fn enter_guild_buy(w: &mut World, s: &mut Shop, at: usize) -> bool {
    let Some(g) = w.guild[0].items.get(at).copied() else { return false };
    let price = g.price & 0x7fff_ffff;
    let Some(stack) = w.item_info.as_ref().and_then(|c| c.item(g.item).map(|_| c.stack(g.item).max(1) as u32)) else { return false };
    if price > w.inv.gil() as u32 || g.count == 0 {
        return false;
    }
    let max = if price > 0 { stack.min(w.inv.gil() as u32 / price) } else { stack }.min(g.count as u32).max(1);
    s.busy = Some(at as u8);
    ask_qty(s, max);
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
    ask_qty(s, r.count);
    true
}

/// FUN_101ecf60: row 0 opens the whole inventory; an item row only when held and the guild has room, the inventory
/// showing just that item.
fn enter_wanted(w: &mut World, s: &mut Shop, at: usize) -> bool {
    let filter = match at.checked_sub(1) {
        None => 0,
        Some(k) => match wanted(w).get(k) {
            Some(g) if held(w, g.item).0 && g.count < g.max => g.item,
            _ => return false,
        },
    };
    s.wanted_at = at;
    (s.view, s.list) = (Some(View::Inv(filter)), (0, 0));
    true
}

/// FUN_101ed300: not grey; the quantity up to the slot's count only.
fn enter_inv(w: &mut World, s: &mut Shop, filter: u16, at: usize) -> bool {
    let Some(r) = inv_rows(w, filter).get(at).cloned() else { return false };
    if sell_tint(w, s, &r) == crate::inventory::TINT_DIM {
        return false;
    }
    s.busy = Some(r.index);
    ask_qty(s, r.count);
    true
}

/// The item info window's item (mode 1 at (130, 240)): the list's cursor row (none on the wanted list's first row).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let s = w.shop.as_ref()?;
    let at = s.list.0 + s.list.1;
    let slot = |r: &crate::inventory::Slot| (r.id, w.inv.ext.get(&(super::items::INVENTORY, r.index)).copied());
    match s.view? {
        View::Buy if s.guild => w.guild[0].items.get(at).map(|g| (g.item, None)),
        View::Buy => buy_rows(s).get(at).map(|r| (r.1.item, None)),
        View::Sell => sell_rows(w).get(at).map(slot),
        View::Wanted => wanted(w).get(at.checked_sub(1)?).map(|g| (g.item, None)),
        View::Inv(filter) => inv_rows(w, filter).get(at).map(slot),
    }
}

/// The bars: the list's title "Shop" / "Guild Shop" (group 0xd 0xc / 0xd), help 0xe 0x2f buy, 0x30 sell, 0x33 the
/// wanted list; itemctrl's and the confirm's own.
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
        bars.title[1] = gu.title_msg(if s.guild { 0xd } else { 0xc }).unwrap_or_default();
        let help = match v {
            View::Buy => 0x2f,
            View::Wanted => 0x33,
            View::Sell | View::Inv(_) => 0x30,
        };
        bars.help[1] = gu.menu_msg(help, "");
    } else {
        focus(s.main_window(), s.main, bars);
    }
}

/// A requirement cell ending at `end` (FUN_101f4bb0): kind 0x13 the rank's word (keytops3 192 + rank + 1), right edge
/// on the end, y + 2; kind 0x3b the level in the sprite font right-aligned at the end (y + 4) and the job's name
/// (windowps 212 + job) ending 24 px before it; kind 2 the line in the game font ending at end - 2.
fn draw_req(gu: &mut GameUi, req: Req, end: f32, y: f32, tint: u32) {
    const KEYTOPS3: &str = "menu    keytops3";
    const WINDOWPS: &str = "menu    windowps";
    match req {
        Req::Rank { need, .. } => {
            let k = 192 + need as usize;
            gu.sprite(KEYTOPS3, k, end - gu.sprite_right(KEYTOPS3, k), y + 2.0, tint);
        }
        Req::Job { job, level } => {
            gu.shape_text(&level.to_string(), end, y + 4.0, tint, true);
            let k = 212 + job as usize;
            gu.sprite(WINDOWPS, k, end - 24.0 - gu.sprite_right(WINDOWPS, k), y + 2.0, tint);
        }
        Req::Text(info) => {
            let t = gu.item_msg(0xc, 0x11d + info as usize).unwrap_or_default();
            let x = end - 2.0 - gu.text_width(&t);
            gu.text(&t, x, y + 2.0, 0, tint.to_le_bytes());
        }
    }
}

/// shopmain / guildsho / shopbuy / shopsell sit on the log like comyn; the list at (16, 48) 274 wide (English), money
/// at (16, 240) over shopmain, itemctrl over money.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32, log_top: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let (Some(s), Some(info)) = (&w.shop, w.item_info.clone()) else { return rects };
    let stacked = |gu: &GameUi, name: &str| gu.window_rect(name, sw, sh).map_or((0.0, 0.0), |r| (0.0, (log_top - r[3] - 2.0).max(16.0) - r[1]));
    let main_focus = s.view.is_none() && s.confirm.is_none();
    if s.view.is_none() || s.confirm.is_some() {
        let main = s.main_window();
        let off = stacked(gu, main);
        if s.confirm.is_none()
            && let Some(win) = gu.lobby_window_off(main, sw, sh, off, Some(s.main), main_focus, &[])
        {
            rects.push(win.frame);
        }
    }
    if s.view.is_some() {
        let Some((wx, wy)) = gu.window_xy(SHOP, sw, sh) else { return rects };
        let size = gu.window_rect(SHOP, sw, sh).map_or([wx, wy, 256.0, 190.0], |r| r);
        let rows = rows(w, s);
        // A requirement on any row adds its column (FUN_10210880: the window 0x69 wider, cells up to + 0x16e).
        let column = rows.iter().any(|r| r.req.is_some());
        let (ww, wh) = (size[2] + WIDE + if column { REQ_W } else { 0.0 }, size[3]);
        gu.frame(wx, wy, ww, wh);
        gu.sprite_from("menu    framesus", 82, 1, wx, wy, WHITE);
        // Rows from (3, 4), 18 px apart: the half-size icon, the name at +18 (y + 2), the price right-aligned at the
        // column's end - 2 (y + 4) in the sprite font.
        super::items::upload_icons(gu, &info, rows.iter().skip(s.list.0).take(ROWS).filter(|r| r.id != 0).map(|r| r.id));
        let focused = s.qty.is_none() && s.confirm.is_none();
        for (k, r) in rows.iter().skip(s.list.0).take(ROWS).enumerate() {
            let (rx, ry) = (wx + 3.0, wy + 4.0 + 18.0 * k as f32);
            let icon = crate::render::invwin::icon_texture(r.id);
            if r.id != 0 && gu.has_image(&icon) {
                gu.image_rect(&icon, rx, ry, 16.0, 16.0, [1.0; 4]);
            }
            let name = r.text.clone().unwrap_or_else(|| info.name(r.id).unwrap_or_default());
            gu.text(&name, rx + 18.0, ry + 2.0, 0, r.tint.to_le_bytes());
            if let Some(p) = &r.price {
                gu.shape_text(p, rx + PRICE_END - 2.0, ry + 4.0, r.price_tint, true);
            }
            if let Some(req) = r.req {
                draw_req(gu, req, wx + REQ_END, ry, r.tint);
            }
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
