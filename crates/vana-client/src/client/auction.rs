//! The auction house (spec ~/ffxi/lab/research/auction_spec.md): s2c 0x04C / c2s 0x04E, the command menus auc1 ..
//! aucitem, auc3 and auc4 at (384, 48), the list "menu    auclist" (bid and sales modes) and the price history
//! "menu    auchisto" at (16, 48), bids, listings and Sales Status. The item lists and histories come from the search
//! server (net/search.rs ah_*).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::inventory::ItemInfo;
use crate::net::search::{AhReply, AhSale};
use glam::Vec3;
use std::sync::mpsc::Receiver;

const WHITE: u32 = 0x8080_8080;
const GREEN: u32 = 0x8050_9050;
const GREY: u32 = 0x8040_4040;
const YELLOW: u32 = 0x8090_9040;
const ORANGE: u32 = 0x8080_4020;
const STACK_NAME: u32 = 0x8090_5050;
const LIST: &str = "menu    auclist";
const HISTORY: &str = "menu    auchisto";
const AUC1: &str = "menu    auc1";
const AUC3: &str = "menu    auc3";
const AUC4: &str = "menu    auc4";
const MONEYCTR: &str = "menu    moneyctr";
const SHOPSELL: &str = "menu    shopsell";
const WEPSORT: &str = "menu    wepsort";
const ARMSORT: &str = "menu    armsort";
const WINDOWPS: &str = "menu    windowps";
/// Categories 0..=0x41, 20 sort bytes each (3.3).
const CATS: usize = 0x42;
const SORTS: usize = 20;
const SORT_FILE: usize = 0x574;
/// The list windows: 10 rows of 18 px from (4, 4).
const ROWS: usize = 10;
const PITCH: f32 = 18.0;

/// One parcel slot (S+0x2cc8c + 0x28 k): a listing or a bid, as the server last sent it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parcel {
    pub stat: u8,
    pub name: String,
    pub item: u16,
    pub qty: u8,
    pub cat: u8,
    pub price: u32,
    pub market: u32,
    pub lot: u32,
    pub time: u32,
}

/// s2c 0x04C (and the c2s 0x04E layout): command, slot, result, status, the parameters and the parcel.
#[derive(Clone, Debug, Default)]
pub struct AucPacket {
    pub cmd: u8,
    pub slot: i8,
    pub result: u8,
    pub status: i8,
    pub p8: u32,
    pub pc: u16,
    pub pe: u16,
    pub p10: u32,
    pub parcel: Parcel,
}

impl AucPacket {
    /// From the packet bytes (offsets with the header).
    pub fn parse(p: &[u8]) -> Self {
        let b = |o: usize| p.get(o).copied().unwrap_or(0);
        let w = |o: usize| u16::from_le_bytes([b(o), b(o + 1)]);
        let d = |o: usize| u32::from_le_bytes([b(o), b(o + 1), b(o + 2), b(o + 3)]);
        let name: String = (0x18..0x28).map(b).take_while(|&c| c != 0).map(|c| c as char).collect();
        AucPacket {
            cmd: b(4),
            slot: b(5) as i8,
            result: b(6),
            status: b(7) as i8,
            p8: d(8),
            pc: w(0xc),
            pe: w(0xe),
            p10: d(0x10),
            parcel: Parcel { stat: b(0x14), name, item: w(0x28), qty: b(0x2a), cat: b(0x2b), price: d(0x2c), market: d(0x30), lot: d(0x34), time: d(0x38) },
        }
    }
}

/// c2s 0x04E (FUN_100df8e0): command, slot, price, the 0xc / 0xe words, stacks.
fn send(w: &World, cmd: u8, slot: i8, p8: u32, pc: u16, pe: u16, p10: u32) {
    let mut b = vec![0u8; 0x38];
    (b[0], b[1]) = (cmd, slot as u8);
    b[4..8].copy_from_slice(&p8.to_le_bytes());
    b[8..10].copy_from_slice(&pc.to_le_bytes());
    b[10..12].copy_from_slice(&pe.to_le_bytes());
    b[12..16].copy_from_slice(&p10.to_le_bytes());
    log::info!("auction: c2s 0x04E cmd {cmd:#x} slot {slot} {p8} {pc} {pe} {p10}");
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x04E, body: b });
}

/// FUN_101d7280: the market of an auction zone (the city zones' table 0x1037ed68): 4, else 0.
pub fn market(zone: Option<u16>) -> u8 {
    const ZONES: [u16; 26] = [230, 231, 232, 234, 235, 236, 238, 239, 240, 241, 243, 244, 245, 246, 247, 250, 252, 26, 48, 50, 53, 256, 257, 80, 87, 94];
    if zone.is_some_and(|z| ZONES.contains(&z)) { 4 } else { 0 }
}

/// A list row: bid mode single (3) / stack (4) {item, single count, stack count}, or a sales mode slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Single { item: u16, single: i32, stack: i32 },
    Stack { item: u16, single: i32, stack: i32 },
    Slot(u8),
}

impl Row {
    fn item(&self, slots: &[Parcel; 8]) -> u16 {
        match *self {
            Row::Single { item, .. } | Row::Stack { item, .. } => item,
            Row::Slot(k) => slots[k as usize].item,
        }
    }

    fn stack(&self) -> bool {
        matches!(self, Row::Stack { .. })
    }
}

/// The list in bid mode (rows from the search server) or sales mode (rows from the slots, rebuilt each frame).
#[derive(Clone, Debug, Default)]
pub struct List {
    pub sales: bool,
    pub rows: Vec<Row>,
    pub scroll: usize,
    pub cursor: usize,
    /// L+0x89a: the reply is in.
    pub ready: bool,
    pub failed: bool,
}

/// The price history (H): its item, form, where it returns (0 the bid list, 1 Sales Status, -1 none) and mode (0 from
/// Sales Status, 1 the bid list, 2 selling).
#[derive(Clone, Debug, Default)]
pub struct History {
    pub item: u16,
    pub stack: bool,
    pub ret: i8,
    pub mode: u8,
    pub head: Option<(u16, u32, u16)>,
    pub sales: Vec<AhSale>,
    pub scroll: usize,
    pub cursor: usize,
    pub ready: bool,
    pub failed: bool,
}

/// What the money counter is for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MoneyFor {
    Bid(Row),
    Sell { inv: u8, item: u16, single: bool },
}

/// The prompts (comyn).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// "Make a bid of N gil for the X?" (10 0x1e).
    Bid { item: u16, price: u32, single: bool },
    /// The fee line (10 0x1c / 0x1d), then "Place X up on auction for N gil?" (10 0x1f).
    Fee { inv: u8, item: u16, price: u32, fee: u32, single: bool },
    Place { inv: u8, item: u16, price: u32, single: bool },
    /// "Remove merchandise from auction?" (9 0x3a).
    Cancel { slot: u8 },
}

/// A layer of the auction windows, newest on top (the focus).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layer {
    Menu(&'static str, u16),
    List,
    History,
    /// The inventory list with the auction source (3.5): scroll, cursor.
    Sell(usize, usize),
}

/// The auction windows while auc1 is open.
pub struct Auction {
    origin: Vec3,
    layers: Vec<Layer>,
    pub list: Option<List>,
    pub history: Option<History>,
    /// The bid list kept while the history is up (returned to without a new query).
    kept: Option<List>,
    query: Option<(bool, Receiver<anyhow::Result<AhReply>>)>,
    cat: u8,
    money: Option<(u32, u32, u32, MoneyFor)>,
    /// shopsell (Sell / Cancel) for a price: its cursor and the sale.
    shopsell: Option<(u16, u8, u16, u32, bool)>,
    /// LotCheck sent for the slot this opening.
    checked: [bool; 8],
    /// The saved sorts (loaded at open, saved at close) and the list's condition (L+0x7ec).
    sorts: Sorts,
    cond: Vec<(u32, u32)>,
}

/// The saved sorts (DAT_10663448, AUCSORT.DAT): per category up to 20 bytes, low nibble the type, high nibble the
/// descending flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Sorts {
    pub bytes: Vec<[u8; SORTS]>,
    pub count: Vec<u8>,
}

impl Default for Sorts {
    fn default() -> Self {
        Sorts { bytes: vec![[0; SORTS]; CATS], count: vec![0; CATS] }
    }
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(b);
    c.sum()
}

impl Sorts {
    /// FUN_101d7820's file: CRC-32 of +8..+0x530 @0, of +0x530..+0x572 @4; a bad size or checksum clears them all.
    /// *(The old 0x550-byte layout's conversion is not done: cleared.)*
    pub fn parse(b: &[u8]) -> Sorts {
        let mut s = Sorts::default();
        if b.len() != SORT_FILE {
            return s;
        }
        let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        if u(0) != crc32(&b[8..0x530]) || u(4) != crc32(&b[0x530..0x572]) {
            return s;
        }
        for c in 0..CATS {
            s.bytes[c].copy_from_slice(&b[8 + SORTS * c..8 + SORTS * (c + 1)]);
            s.count[c] = b[0x530 + c].min(SORTS as u8);
        }
        s
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut b = vec![0u8; SORT_FILE];
        for c in 0..CATS {
            b[8 + SORTS * c..8 + SORTS * (c + 1)].copy_from_slice(&self.bytes[c]);
            b[0x530 + c] = self.count[c];
        }
        let (a, z) = (crc32(&b[8..0x530]), crc32(&b[0x530..0x572]));
        b[0..4].copy_from_slice(&a.to_le_bytes());
        b[4..8].copy_from_slice(&z.to_le_bytes());
        b
    }

    fn of(&self, cat: u8) -> &[u8] {
        let c = (cat as usize).min(CATS - 1);
        &self.bytes[c][..self.count[c] as usize]
    }

    /// FUN_101d7760: the category has a saved sort.
    fn any(&self, cat: u8) -> bool {
        !self.of(cat).is_empty()
    }

    /// FUN_101d76f0: a type saved once, at most 20; Race (3) and Job (4) never.
    fn add(&mut self, cat: u8, ty: u8) {
        let c = (cat as usize).min(CATS - 1);
        if matches!(ty, 3 | 4) || self.of(cat).iter().any(|b| b & 0xf == ty) || self.count[c] as usize >= SORTS {
            return;
        }
        self.bytes[c][self.count[c] as usize] = ty | (descending(ty) as u8) << 4;
        self.count[c] += 1;
    }

    fn clear(&mut self, cat: u8) {
        let c = (cat as usize).min(CATS - 1);
        (self.bytes[c], self.count[c]) = ([0; SORTS], 0);
    }
}

/// The fixed directions (FUN_101d7780): Level, Damage, Defense and Resistance descending; Delay and Name ascending.
fn descending(ty: u8) -> bool {
    matches!(ty, 2 | 5 | 7 | 8)
}

/// A param word (FUN_10104170 / FUN_101040e0): bit 0 a value, bit 1 descending, bits 2.. the value.
fn sort_param(ty: u8) -> (u32, u32) {
    (ty as u32, (descending(ty) as u32) << 1)
}

fn filter_param(ty: u8, value: u32) -> (u32, u32) {
    (ty as u32, 1 | (value & 0x7fff) << 2)
}

/// A param into the condition: a type only once, 20 at most.
fn cond_add(cond: &mut Vec<(u32, u32)>, p: (u32, u32)) {
    if cond.len() < SORTS && !cond.iter().any(|c| c.0 == p.0) {
        cond.push(p);
    }
}

/// `<game>/USER/<content id>/AUCSORT.DAT`.
fn sort_path(w: &World) -> Option<std::path::PathBuf> {
    w.macros.as_ref().map(|m| m.dir().join("AUCSORT.DAT"))
}

/// The bid in flight and its result (DAT_1066344c..60): kept across auc1's close.
#[derive(Clone, Debug, Default)]
pub struct BidState {
    pub pending: bool,
    /// The result waiting to print: kind (1 bought, 2 failed, 3 full), ticks left, item, count, price.
    pub result: Option<(u8, f32, u16, u32, u32)>,
}

/// The command menus: their rows (child ids in screen order) and what each does.
#[derive(Clone, Copy)]
enum Pick {
    Menu(&'static str),
    Cat(u8),
}

fn menu_rows(name: &str) -> &'static [(u16, Pick)] {
    use Pick::*;
    match name {
        "menu    auc2" => &[(1, Menu("menu    aucweapo")), (2, Menu("menu    aucarmor")), (3, Menu("menu    aucmagic")), (4, Cat(0x21)), (5, Cat(0x22)), (6, Menu("menu    aucmater")), (7, Menu("menu    aucfood")), (8, Cat(0x23)), (9, Menu("menu    aucitem"))],
        "menu    aucweapo" => &[(1, Cat(1)), (2, Cat(2)), (3, Cat(3)), (4, Cat(4)), (5, Cat(5)), (6, Cat(6)), (7, Cat(7)), (8, Cat(8)), (9, Cat(9)), (10, Cat(10)), (11, Cat(11)), (12, Cat(12)), (13, Cat(13)), (14, Cat(14)), (15, Menu("menu    aucammo"))],
        "menu    aucammo" => &[(1, Cat(0xf)), (2, Cat(0x2f)), (3, Cat(0x30)), (4, Cat(0x3e))],
        "menu    aucarmor" => &[(1, Cat(0x10)), (2, Cat(0x11)), (3, Cat(0x16)), (4, Cat(0x12)), (5, Cat(0x13)), (6, Cat(0x17)), (7, Cat(0x14)), (8, Cat(0x15)), (9, Cat(0x1a)), (10, Cat(0x18)), (11, Cat(0x19))],
        "menu    aucmagic" => &[(1, Cat(0x1c)), (2, Cat(0x1d)), (3, Cat(0x20)), (4, Cat(0x1f)), (5, Cat(0x1e)), (6, Cat(0x3c)), (7, Cat(0x2d))],
        "menu    aucmater" => &[(1, Cat(0x26)), (2, Cat(0x27)), (3, Cat(0x28)), (4, Cat(0x29)), (5, Cat(0x2a)), (6, Cat(0x2b)), (7, Cat(0x2c)), (8, Cat(0x3f))],
        "menu    aucfood" => &[(1, Menu("menu    aucmeals")), (2, Cat(0x3b)), (3, Cat(0x33))],
        "menu    aucmeals" => &[(1, Cat(0x34)), (2, Cat(0x35)), (3, Cat(0x36)), (4, Cat(0x37)), (5, Cat(0x38)), (6, Cat(0x39)), (7, Cat(0x3a))],
        "menu    aucitem" => &[(1, Cat(0x2e)), (7, Cat(0x40)), (8, Cat(0x41)), (2, Cat(0x32)), (3, Cat(0x24)), (4, Cat(0x31)), (5, Cat(0x25)), (6, Cat(0x3d))],
        _ => &[],
    }
}

/// The plain rows of the other menus (auc1, auc3, auc4).
fn plain_rows(name: &str) -> u16 {
    match name {
        AUC1 | AUC3 => 3,
        AUC4 => 2,
        WEPSORT | ARMSORT => 6,
        _ => menu_rows(name).len() as u16,
    }
}

fn first_row(name: &str) -> u16 {
    menu_rows(name).first().map_or(1, |r| r.0)
}

/// Up / Down along a menu's rows (screen order).
fn step_row(name: &str, sel: u16, down: bool) -> u16 {
    let rows: Vec<u16> = if menu_rows(name).is_empty() { (1..=plain_rows(name)).collect() } else { menu_rows(name).iter().map(|r| r.0).collect() };
    let i = rows.iter().position(|&r| r == sel).unwrap_or(0);
    let n = rows.len();
    rows[if down { (i + 1) % n } else { (i + n - 1) % n }]
}

fn item_name(w: &World, id: u16) -> String {
    w.item_info.as_ref().and_then(|c| c.name(id)).unwrap_or_default()
}

fn refuse(w: &mut World, id: usize) {
    super::textcmd::refuse(w, id);
}

/// A group 9 line in mode 0x79.
fn line9(w: &mut World, id: usize) {
    super::post::log_group9(w, id, 0x79);
}

impl Auction {
    fn top(&self) -> Option<Layer> {
        self.layers.last().copied()
    }

    fn busy(&self) -> bool {
        self.query.is_some()
    }

    /// FUN_101da6a0: a category picked: the condition from its saved sorts, then the list.
    fn ask_list(&mut self, w: &World, cat: u8) {
        self.cat = cat;
        self.rebuild();
        self.requery(w);
    }

    /// FUN_101d7780: the condition is the category's saved sorts, in their fixed directions.
    fn rebuild(&mut self) {
        self.cond.clear();
        for &b in self.sorts.of(self.cat) {
            cond_add(&mut self.cond, sort_param(b & 0xf));
        }
    }

    /// FUN_101d8140(0, 0): the list from the search server with the condition; the menu stays open under it.
    fn requery(&mut self, w: &World) {
        log::info!("auction: list of category {:#x}, condition {:?}", self.cat, self.cond);
        let payload = crate::net::search::ah_list_payload(self.cat as u16, market(w.zone), &self.cond);
        self.query = Some((false, crate::net::search::ah_spawn(w.search_addr, 0x15, payload, 1)));
        self.layers.retain(|l| !matches!(l, Layer::List | Layer::History));
        self.list = Some(List::default());
        self.layers.push(Layer::List);
    }

    /// FUN_101d8800: the history of an item's form; `ret` where Escape goes back to.
    fn ask_history(&mut self, w: &World, item: u16, stack: bool, ret: i8, mode: u8) {
        let payload = crate::net::search::ah_history_payload(item, market(w.zone), stack);
        self.query = Some((true, crate::net::search::ah_spawn(w.search_addr, if stack { 6 } else { 5 }, payload, 0)));
        self.history = Some(History { item, stack, ret, mode, ..Default::default() });
        self.layers.retain(|l| !matches!(l, Layer::History));
        self.layers.push(Layer::History);
    }
}

/// s2c 0x04C Open Result 1 (FUN_101da530): auc1 at (384, 48), the position kept for the 6.0 close; c2s WorkCheck(-1).
fn open(w: &mut World) {
    if w.auction.is_some() || super::world_window_open(w) {
        refuse(w, 0x45);
        return;
    }
    let sorts = sort_path(w).and_then(|p| std::fs::read(p).ok()).map_or_else(Sorts::default, |b| Sorts::parse(&b));
    w.auction = Some(Box::new(Auction { origin: w.pos, layers: vec![Layer::Menu(AUC1, 1)], list: None, history: None, kept: None, query: None, cat: 0, money: None, shopsell: None, checked: [false; 8], sorts, cond: Vec::new() }));
    w.bid.pending = false;
    send(w, 0xa, -1, 0, 0, 0, 0);
}

/// FUN_101da370: every auction window closes; a waiting bid result prints now; the sorts are saved (FUN_101d7820).
pub fn close(w: &mut World) {
    if let Some(a) = w.auction.take() {
        if let Some(p) = sort_path(w)
            && let Err(e) = std::fs::create_dir_all(p.parent().unwrap_or(&p)).and_then(|_| std::fs::write(&p, a.sorts.bytes()))
        {
            log::warn!("auction: {}: {e}", p.display());
        }
        print_result(w, true);
    }
}

/// FUN_100a3ac0: s2c 0x04C.
pub fn on_packet(w: &mut World, p: AucPacket) {
    log::info!("auction: s2c 0x04C cmd {:#x} slot {} result {:#x} item {} price {} p10 {} stat {}", p.cmd, p.slot, p.result, p.pc, p.p8, p.p10, p.parcel.stat);
    if p.result == 0xff {
        line9(w, 0x3e);
        close(w);
    } else if p.result == 0xf5 {
        refuse(w, 0xba);
    }
    match p.cmd {
        2 if p.result == 1 => open(w),
        2 if p.result == 0xf6 => line9(w, 0x2d),
        2 => line9(w, 0x3e),
        3 => close(w),
        4 if p.result == 1 => fee(w, p.pc as u8, p.pe, p.p8, p.p10 == 1),
        4 if p.result == 0xf3 => refuse(w, 0x6d),
        5 if p.result == 1 => open_sales(w),
        5 if p.result == 0xf6 => line9(w, 0x2d),
        0xb if p.result == 1 => line9(w, 0x36),
        0xb if p.result != 2 => line9(w, 0x37),
        0xc if matches!(p.result, 0xe4 | 0xe5) => refuse(w, 0x6f),
        0xd if p.result == 0xf6 => w.sounds.push(Ui::Error as u32),
        0xe => bid_result(w, &p),
        _ => {}
    }
    // FUN_100dfe10: the parcel into its slot.
    if !matches!(p.cmd, 2..=5) && (0..8).contains(&p.slot) {
        w.ah_slots[p.slot as usize] = p.parcel;
    }
}

/// s2c Bid (FUN_101d79e0): the result prints 180..299 ticks later, or at once with auc1 gone.
fn bid_result(w: &mut World, p: &AucPacket) {
    let kind = match p.result {
        1 => 1,
        2 => 0,
        0xe4 | 0xe5 => 3,
        _ => 2,
    };
    w.bid.pending = false;
    let ticks = ((p.pc as u32).wrapping_mul(p.p8) % 120 + 180) as f32;
    w.bid.result = Some((kind, ticks, p.pc, p.p10, p.p8));
    if w.auction.is_none() {
        print_result(w, true);
    }
}

/// FUN_101d7ac0: the bid result's line once its time is up (or `now`).
fn print_result(w: &mut World, now: bool) {
    let Some((kind, ticks, item, count, price)) = w.bid.result else { return };
    if !now && ticks >= 0.0 {
        return;
    }
    w.bid.result = None;
    let work = [item as i32, count as i32, price as i32, 0, 0];
    match kind {
        1 => super::post::system_line(w, if count > 1 { 0xbc } else { 0xb1 }, work, String::new()),
        2 => super::post::system_line(w, if count > 1 { 0xbd } else { 0xb2 }, work, String::new()),
        3 => refuse(w, 0x70),
        _ => {}
    }
}

/// s2c Info Result 1 (FUN_101d8140(1, 0)): the list in sales mode.
fn open_sales(w: &mut World) {
    let Some(a) = w.auction.as_mut() else { return };
    a.checked = [false; 8];
    a.layers.retain(|l| !matches!(l, Layer::List | Layer::History));
    a.list = Some(List { sales: true, ready: true, ..Default::default() });
    a.layers.push(Layer::List);
}

/// The sales rows (FUN_101d7e50): slots with Stat other than 0 / 6 / 7.
fn sales_rows(w: &World) -> Vec<Row> {
    (0..8u8).filter(|&k| !matches!(w.ah_slots[k as usize].stat, 0 | 6 | 7)).map(Row::Slot).collect()
}

/// The parcel stat's tint and help (group 14) in the sales list.
fn stat_look(w: &World, p: &Parcel) -> (u32, Option<usize>) {
    match p.stat {
        3 if p.market == market(w.zone) as u32 => (GREEN, None),
        3 => (GREY, None),
        0xa => (YELLOW, Some(0x3e)),
        0xb => (ORANGE, Some(0x3f)),
        0xc | 0x15 => (YELLOW, Some(0x40)),
        0xd | 0x16 => (ORANGE, Some(0x41)),
        0xe => (GREY, Some(0x3d)),
        0xf => (GREY, Some(0x3c)),
        _ => (GREY, None),
    }
}

/// Per frame: the search replies, the 6.0 close, the bid result's timer, sales rows and their LotChecks.
pub fn update(w: &mut World, dt: f32) {
    if let Some((_, t, ..)) = w.bid.result.as_mut() {
        *t -= dt * 60.0;
    }
    let Some(mut a) = w.auction.take() else { return };
    let d = w.pos - a.origin;
    if d.x * d.x + d.z * d.z > 36.0 || super::event::active(w) {
        w.auction = Some(a);
        close(w);
        return;
    }
    if let Some((hist, rx)) = &a.query {
        let hist = *hist;
        match rx.try_recv() {
            Ok(r) => {
                a.query = None;
                if hist {
                    if let Some(h) = a.history.as_mut() {
                        h.ready = true;
                        match r {
                            Ok(r) => {
                                (h.head, h.sales) = (r.head, r.sales.into_iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect());
                            }
                            Err(_) => h.failed = true,
                        }
                    }
                } else if let Some(l) = a.list.as_mut().filter(|l| !l.sales) {
                    l.ready = true;
                    match r {
                        Ok(r) => {
                            for (item, single, stack) in r.items {
                                if single >= 0 {
                                    l.rows.push(Row::Single { item, single, stack });
                                }
                                if stack >= 0 {
                                    l.rows.push(Row::Stack { item, single, stack });
                                }
                            }
                        }
                        Err(_) => l.failed = true,
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => a.query = None,
            Err(_) => {}
        }
    }
    if let Some(l) = a.list.as_mut().filter(|l| l.sales) {
        l.rows = sales_rows(w);
        l.cursor = l.cursor.min(l.rows.len().saturating_sub(1));
        for r in &l.rows {
            if let Row::Slot(k) = *r
                && matches!(w.ah_slots[k as usize].stat, 3 | 0xc..=0xf)
                && !a.checked[k as usize]
            {
                a.checked[k as usize] = true;
                send(w, 0xd, k as i8, 0, 0, 0, 0);
            }
        }
    }
    w.auction = Some(a);
    print_result(w, false);
}

/// The item the item info window shows: the list's or the history's cursor row, the sell list's item; mode 2 at
/// (16, 48) while the bid price is asked.
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>, u8)> {
    let a = w.auction.as_ref()?;
    if let Some((_, _, _, MoneyFor::Bid(r))) = a.money {
        return Some((r.item(&w.ah_slots), None, 2));
    }
    match a.top()? {
        Layer::History => a.history.as_ref().map(|h| (h.item, None, 0)),
        Layer::List | Layer::Menu(AUC3 | AUC4, _) => {
            let l = a.list.as_ref()?;
            let r = l.rows.get(l.scroll + l.cursor)?;
            Some((r.item(&w.ah_slots), None, 0))
        }
        Layer::Sell(scroll, cursor) => {
            let s = sell_rows(w).get(scroll + cursor).cloned()?;
            Some((s.id, w.inv.ext.get(&(0, s.index)).copied(), 0))
        }
        _ => None,
    }
}

/// The inventory rows offered for sale (the view's order, gil left out).
fn sell_rows(w: &World) -> Vec<crate::inventory::Slot> {
    w.inv.viewed(0).into_iter().filter(|s| s.index != 0).collect()
}

/// FUN_101d9690: a sell row's tint: grey for linkshells, bazaar-priced, equipped, NoAuction (flag 0x40) or augmented
/// rows; else white.
fn sell_tint(w: &World, s: &crate::inventory::Slot) -> u32 {
    let info = w.item_info.as_ref();
    let flags = info.and_then(|c| c.item(s.id)).map_or(0, |it| it.flags);
    let lock = w.inv.locks.get(&(0, s.index)).copied().unwrap_or(0);
    if (0x201..=0x203).contains(&s.id) || lock == 0x19 || s.equipped || flags & 0x40 != 0 {
        GREY
    } else {
        WHITE
    }
}

/// The yes / no prompt's text.
pub fn ask_text(w: &World, a: Ask) -> String {
    let Some(bt) = w.battle.as_ref() else { return String::new() };
    let namer = |id: i32, count: i32, _: bool| -> Option<Vec<u8>> {
        let n = item_name(w, id as u16);
        Some(crate::dat::font::encode_sjis(&if count > 1 { format!("{n}s") } else { n }))
    };
    let fmt = |id: usize, strs: &[&[u8]], ints: &[i32]| bt.group10_raw(id).map(|raw| crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, strs, ints, &namer))).unwrap_or_default();
    match a {
        Ask::Bid { item, price, single } => fmt(0x1e, &[price.to_string().as_bytes()], &[item as i32, if single { 1 } else { 2 }, price as i32]),
        Ask::Fee { fee, single: true, .. } => fmt(0x1c, &[fee.to_string().as_bytes()], &[fee as i32]),
        Ask::Fee { item, fee, .. } => {
            let n = w.item_info.as_ref().and_then(|c| c.item(item)).map_or(1, |it| it.stack.max(1) as i32);
            fmt(0x1d, &[fee.to_string().as_bytes()], &[n, fee as i32])
        }
        Ask::Place { item, price, single, .. } => fmt(0x1f, &[price.to_string().as_bytes()], &[item as i32, if single { 1 } else { 2 }, price as i32]),
        Ask::Cancel { .. } => bt.group9(0x3a).unwrap_or_default(),
    }
}

/// A prompt's answer.
pub fn answered(w: &mut World, ask: Ask, yes: bool) {
    match (ask, yes) {
        // LAB_101d95b0: the first empty slot of 8, c2s Bid, the result pending.
        (Ask::Bid { item, price, single }, true) => match w.ah_slots.iter().position(|s| s.stat == 0) {
            Some(k) => {
                send(w, 0xe, k as i8, price, item, 0, single as u32);
                w.bid.pending = true;
            }
            None => refuse(w, 0x6c),
        },
        (Ask::Fee { inv, item, price, single, .. }, true) => w.confirm = Some((super::Confirm::Auction(Ask::Place { inv, item, price, single }), false)),
        // FUN_101d7510: c2s LotIn into the first empty slot of 7; the sell list closes.
        (Ask::Place { inv, price, single, .. }, true) => match w.ah_slots[..7].iter().position(|s| s.stat == 0) {
            Some(k) => {
                send(w, 0xb, k as i8, price, inv as u16, 0, single as u32);
                if let Some(a) = w.auction.as_mut() {
                    a.layers.retain(|l| !matches!(l, Layer::Sell(..)));
                }
            }
            None => refuse(w, 0x6d),
        },
        (Ask::Cancel { slot }, true) => send(w, 0xc, slot as i8, 0, 0, 0, 0),
        _ => {}
    }
}

/// s2c AskCommit Result 1 (FUN_101d73a0): the fee line in the log, then asked.
fn fee(w: &mut World, inv: u8, item: u16, fee: u32, single: bool) {
    let Some(price) = w.auction.as_ref().and_then(|a| a.shopsell.map(|s| s.3)) else { return };
    if let Some(a) = w.auction.as_mut() {
        a.shopsell = None;
    }
    let ask = Ask::Fee { inv, item, price, fee, single };
    let text = ask_text(w, ask);
    if let Some(bt) = w.battle.clone() {
        w.chat.push_colour(text, bt.colour_in(0x79, &w.game));
    }
    w.confirm = Some((super::Confirm::Auction(ask), false));
}

/// The windows' keys; true when taken.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    let Some(mut a) = w.auction.take() else { return false };
    let took = key_inner(w, &mut a, key);
    if w.auction.is_none() {
        let gone = a.layers.is_empty();
        w.auction = Some(a);
        // Escape out of auc1: FUN_101da370.
        if gone {
            close(w);
        }
    }
    took
}

fn key_inner(w: &mut World, a: &mut Auction, key: egui::Key) -> bool {
    use egui::Key as K;
    // moneyctr (FUN_10140bc0): Down / Up by the step, Right a tenth of it, Left ten times.
    if let Some((mut step, max, mut v, purpose)) = a.money {
        match key {
            K::ArrowDown if v >= step => v -= step,
            K::ArrowUp if v < max => v = (v + step).min(max),
            K::ArrowRight if step > 1 => step /= 10,
            K::ArrowRight => v = 0,
            K::ArrowLeft if max / step.max(1) < 10 => v = max,
            K::ArrowLeft => step *= 10,
            _ => {}
        }
        a.money = Some((step, max, v, purpose));
        match key {
            K::ArrowUp | K::ArrowDown | K::ArrowRight | K::ArrowLeft => w.sounds.push(Ui::Cursor as u32),
            K::Enter | K::Escape => {
                w.sounds.push(if key == K::Enter { Ui::Select } else { Ui::Cancel } as u32);
                a.money = None;
                let price = if key == K::Enter { v } else { 0 };
                match purpose {
                    MoneyFor::Bid(r) if price > 0 => {
                        w.confirm = Some((super::Confirm::Auction(Ask::Bid { item: r.item(&w.ah_slots), price, single: !r.stack() }), false));
                    }
                    MoneyFor::Sell { inv, item, single } if price > 0 => a.shopsell = Some((2, inv, item, price, single)),
                    MoneyFor::Sell { .. } => a.layers.retain(|l| !matches!(l, Layer::History)),
                    _ => {}
                }
            }
            _ => {}
        }
        return true;
    }
    // shopsell: Sell / Cancel (LAB_101d9ac0): Sell sends AskCommit; the fee prompt follows the reply.
    if let Some((sel, inv, item, price, single)) = a.shopsell {
        match key {
            K::ArrowUp | K::ArrowDown => {
                a.shopsell = Some((3 - sel, inv, item, price, single));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter if sel == 1 => {
                w.sounds.push(Ui::Select as u32);
                send(w, 4, 0, price, inv as u16, item, single as u32);
                a.layers.retain(|l| !matches!(l, Layer::History));
            }
            K::Enter | K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                a.shopsell = None;
                a.layers.retain(|l| !matches!(l, Layer::History));
            }
            _ => {}
        }
        return true;
    }
    let Some(top) = a.top() else { return false };
    match top {
        Layer::Menu(name, sel) => menu_key(w, a, name, sel, key),
        Layer::List => list_key(w, a, key),
        Layer::History => history_key(w, a, key),
        Layer::Sell(scroll, cursor) => sell_key(w, a, scroll, cursor, key),
    }
    true
}

fn set_top(a: &mut Auction, l: Layer) {
    if let Some(t) = a.layers.last_mut() {
        *t = l;
    }
}

/// The command menus: Up / Down along their rows, Enter (refused while the search is busy), Escape back (auc1: all).
fn menu_key(w: &mut World, a: &mut Auction, name: &'static str, sel: u16, key: egui::Key) {
    use egui::Key as K;
    match key {
        K::ArrowUp | K::ArrowDown => {
            set_top(a, Layer::Menu(name, step_row(name, sel, key == K::ArrowDown)));
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            a.layers.pop();
            if name == AUC1 {
                a.layers.clear();
            }
        }
        K::Enter if a.busy() => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            match name {
                AUC1 => match sel {
                    1 => a.layers.push(Layer::Menu("menu    auc2", 1)),
                    2 => a.layers.push(Layer::Sell(0, 0)),
                    _ => send(w, 5, 0, 0, 0, 0, 0),
                },
                AUC3 => auc3_pick(w, a, sel),
                AUC4 => auc4_pick(w, a, sel),
                WEPSORT | ARMSORT => sort_pick(w, a, name, sel),
                _ => match menu_rows(name).iter().find(|r| r.0 == sel).map(|r| r.1) {
                    Some(Pick::Menu(m)) => a.layers.push(Layer::Menu(m, first_row(m))),
                    Some(Pick::Cat(c)) => a.ask_list(w, c),
                    None => {}
                },
            }
        }
        _ => {}
    }
}

fn cursor_row(a: &Auction) -> Option<Row> {
    let l = a.list.as_ref()?;
    l.rows.get(l.scroll + l.cursor).copied()
}

/// auc3 (FUN_101d9330): 1 Price History, 2 Bid (money up to our gil), 3 Sort.
fn auc3_pick(w: &mut World, a: &mut Auction, sel: u16) {
    a.layers.pop();
    let Some(r) = cursor_row(a) else { return };
    match sel {
        1 => {
            a.kept = a.list.take();
            a.layers.retain(|l| !matches!(l, Layer::List));
            a.ask_history(w, r.item(&w.ah_slots), r.stack(), 0, 1);
        }
        2 if auc3_dim(w, r) => w.sounds.push(Ui::Error as u32),
        2 => a.money = Some((1, w.inv.gil(), 0, MoneyFor::Bid(r))),
        _ => match sort_kind(a.cat) {
            0 => a.layers.push(Layer::Menu(WEPSORT, 1)),
            1 => a.layers.push(Layer::Menu(ARMSORT, 1)),
            // The name toggle: Name saved when there is no saved sort, else the saved sorts cleared.
            _ => {
                if a.sorts.any(a.cat) {
                    a.sorts.clear(a.cat);
                } else {
                    a.sorts.add(a.cat, 9);
                }
                a.rebuild();
                a.requery(w);
            }
        },
    }
}

/// FUN_101d9200: auc3's Sort is wepsort for the weapon and ammunition categories, armsort for armor, else the name
/// toggle.
fn sort_kind(cat: u8) -> u8 {
    match cat {
        1..=0xf | 0x30 => 0,
        0x10..=0x1a => 1,
        _ => 2,
    }
}

/// wepsort (FUN_101daa10) / armsort (FUN_101dabd0): Reset List, two sorts of their own, By Level, Job, Race; then the
/// list again and the window closed.
fn sort_pick(w: &mut World, a: &mut Auction, name: &str, sel: u16) {
    let arm = name == ARMSORT;
    let ty = match sel {
        2 => Some(if arm { 7 } else { 5 }),
        3 => Some(if arm { 8 } else { 6 }),
        4 => Some(2),
        _ => None,
    };
    match sel {
        1 => {
            a.sorts.clear(a.cat);
            a.rebuild();
        }
        // Job needs s2c 0x119 (DAT_10485824 bit 0); Race our actor's race byte.
        5 => match w.stats.as_ref().map(|s| s.main.0 as u32).filter(|_| w.recasts_seen) {
            Some(job) => cond_add(&mut a.cond, filter_param(4, job)),
            None => return w.sounds.push(Ui::Error as u32),
        },
        6 if w.my_race != 0 => cond_add(&mut a.cond, filter_param(3, w.my_race as u32)),
        6 => return w.sounds.push(Ui::Error as u32),
        _ => {}
    }
    if let Some(ty) = ty {
        a.sorts.add(a.cat, ty);
        cond_add(&mut a.cond, sort_param(ty));
    }
    a.layers.retain(|l| !matches!(l, Layer::Menu(n, _) if *n == name));
    a.requery(w);
}

/// auc3's Bid is disabled for a row with nothing on sale or while a bid result waits.
fn auc3_dim(w: &World, r: Row) -> bool {
    let n = match r {
        Row::Single { single, .. } => single,
        Row::Stack { stack, .. } => stack,
        Row::Slot(_) => 0,
    };
    n <= 0 || w.bid.result.is_some()
}

/// auc4 (FUN_101d9100): 1 Price History, 2 Stop Sale (asks first).
fn auc4_pick(w: &mut World, a: &mut Auction, sel: u16) {
    a.layers.pop();
    let Some(Row::Slot(k)) = cursor_row(a) else { return };
    let p = w.ah_slots[k as usize].clone();
    let here = p.market == market(w.zone) as u32;
    match sel {
        1 if here => {
            a.layers.retain(|l| !matches!(l, Layer::List));
            a.kept = a.list.take();
            a.ask_history(w, p.item, p.qty != 1, 1, 0);
        }
        2 if p.stat == 3 && here => w.confirm = Some((super::Confirm::Auction(Ask::Cancel { slot: k }), false)),
        _ => w.sounds.push(Ui::Error as u32),
    }
}

/// Up / Down a row; Right / Left a page (FUN_101f6c70 messages 3 / 4, as every list window).
fn scroll_keys(scroll: &mut usize, cursor: &mut usize, n: usize, key: egui::Key) -> bool {
    match key {
        egui::Key::ArrowRight | egui::Key::ArrowLeft => super::items::page_keys(scroll, cursor, n, key == egui::Key::ArrowRight),
        _ => super::items::list_keys(scroll, cursor, n, key),
    }
}

/// auclist: rows, Enter (bid: auc3; sales: auc4 or the sold / returned line), Escape closes it.
fn list_key(w: &mut World, a: &mut Auction, key: egui::Key) {
    use egui::Key as K;
    let Some(mut l) = a.list.take() else { return };
    match key {
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            a.layers.retain(|x| !matches!(x, Layer::List));
            a.query = None;
            return;
        }
        K::Enter => {
            let row = l.rows.get(l.scroll + l.cursor).copied();
            if !l.ready || row.is_none() {
                w.sounds.push(Ui::Error as u32);
            } else if !l.sales && w.bid.pending {
                refuse(w, 0x6e);
            } else if !l.sales {
                w.sounds.push(Ui::Select as u32);
                a.layers.push(Layer::Menu(AUC3, 1));
            } else if let Some(Row::Slot(k)) = row {
                let p = w.ah_slots[k as usize].clone();
                let work = [p.item as i32, p.qty as i32, 0, 0, 0];
                match p.stat {
                    3 => {
                        w.sounds.push(Ui::Select as u32);
                        a.layers.push(Layer::Menu(AUC4, 1));
                    }
                    0xa | 0xc | 0x15 => {
                        super::post::system_line(w, if p.qty > 1 { 0xba } else { 0xad }, work, String::new());
                        send(w, 0x10, k as i8, 0, 0, 0, 0);
                    }
                    0xb | 0xd | 0x16 => {
                        super::post::system_line(w, if p.qty > 1 { 0xbb } else { 0xae }, work, String::new());
                        send(w, 0x10, k as i8, 0, 0, 0, 0);
                    }
                    _ => w.sounds.push(Ui::Select as u32),
                }
            }
        }
        _ => {
            if scroll_keys(&mut l.scroll, &mut l.cursor, l.rows.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
    a.list = Some(l);
}

/// auchisto: rows; Enter prints the sale; Escape back to the bid list (no new query) or Sales Status.
fn history_key(w: &mut World, a: &mut Auction, key: egui::Key) {
    use egui::Key as K;
    let Some(mut h) = a.history.take() else { return };
    match key {
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            a.layers.retain(|x| !matches!(x, Layer::History));
            a.query = None;
            if h.ret >= 0
                && let Some(l) = a.kept.take()
            {
                a.list = Some(l);
                a.layers.push(Layer::List);
            }
            return;
        }
        K::Enter => {
            if let Some(s) = h.sales.get(h.scroll + h.cursor).cloned() {
                w.sounds.push(Ui::Select as u32);
                let date = sale_date(w, s.date, 1);
                if let Some(bt) = w.battle.clone() {
                    let arrow = bt.group9(0x3b).unwrap_or_else(|| ">".into());
                    let price = crate::inventory::gil_text(s.price as i64);
                    let line = format!("({date}) {}{arrow}{} [{}G]", s.seller, s.buyer, price.trim_end_matches(" G").trim_end());
                    w.chat.push_colour(line, bt.colour_in(0x79, &w.game));
                }
            } else {
                w.sounds.push(Ui::Error as u32);
            }
        }
        _ => {
            if scroll_keys(&mut h.scroll, &mut h.cursor, h.sales.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
    a.history = Some(h);
}

/// FUN_101c8fd0(date, flags): 2 the date only, 1 the long date and time.
fn sale_date(w: &World, unix: u32, flags: u32) -> String {
    let Some(bt) = w.battle.as_ref() else { return String::new() };
    let g = |id: u16| bt.group10_raw(id as usize);
    let (key, h12) = ((w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::earth_datetime(&g, unix as i64, flags, key, h12))
}

/// The sell list (FUN_101d9850): the checks, then the history in sell mode and the price.
fn sell_key(w: &mut World, a: &mut Auction, mut scroll: usize, mut cursor: usize, key: egui::Key) {
    use egui::Key as K;
    let rows = sell_rows(w);
    match key {
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            a.layers.retain(|l| !matches!(l, Layer::Sell(..)));
        }
        K::Enter if a.busy() => {}
        K::Enter => {
            let Some(s) = rows.get(scroll + cursor).cloned() else { return };
            if sell_tint(w, &s) == GREY {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            let it = w.item_info.as_ref().and_then(|c| c.item(s.id));
            let stack = it.as_ref().map_or(1, |i| i.stack.max(1) as u32);
            let full = stack >= 2 && s.count == stack;
            if stack > 1 && s.count != 1 && !full {
                let text = w.battle.as_ref().and_then(|b| b.group10(0x1b)).map(|f| super::textcmd::printf(&f, &[stack.to_string()]));
                if let (Some(t), Some(bt)) = (text, w.battle.clone()) {
                    w.chat.push_colour(t, bt.colour_in(0x7b, &w.game));
                }
            }
            if !w.ah_slots[..7].iter().any(|p| p.stat == 0) {
                refuse(w, 0x6d);
                w.sounds.push(Ui::Error as u32);
                return;
            }
            w.sounds.push(Ui::Select as u32);
            a.ask_history(w, s.id, full, -1, 2);
            a.money = Some((1, 999_999_999, 0, MoneyFor::Sell { inv: s.index, item: s.id, single: !full }));
        }
        _ => {
            if scroll_keys(&mut scroll, &mut cursor, rows.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
                set_top(a, Layer::Sell(scroll, cursor));
            }
        }
    }
}

/// The focused list's bar (auclist, the sell list; FUN_101f6c70).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let n = sell_rows(w).len();
    let Some(a) = w.auction.as_mut().filter(|a| a.money.is_none() && a.shopsell.is_none()) else { return false };
    match a.top() {
        Some(Layer::List) => a.list.as_mut().is_some_and(|l| super::scrollbar::rows(m, &mut l.scroll, &mut l.cursor, l.rows.len(), ROWS)),
        Some(Layer::Sell(mut scroll, mut cursor)) => {
            let moved = super::scrollbar::rows(m, &mut scroll, &mut cursor, n, crate::inventory::VISIBLE_ROWS);
            set_top(a, Layer::Sell(scroll, cursor));
            moved
        }
        _ => false,
    }
}

/// The mouse on the focused window (winhits.rs): the menus and shopsell as the generic window, auclist / auchisto's
/// rows and the sell list's as lists (hover moves the cursor, a click on it is Enter, a right click Escape); moneyctr
/// only the right click.
#[allow(clippy::too_many_arguments)]
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::{generic, over};
    // The modern layer draws these and takes their mouse.
    if w.modern {
        return;
    }
    let Some(a) = w.auction.as_ref() else { return };
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| {
        key(w, k);
    };
    if a.money.is_some() {
        if right && over(w, MONEYCTR, p) {
            key(w, egui::Key::Escape);
        }
        return;
    }
    if let Some((sel, ..)) = a.shopsell {
        generic(w, gu, SHOPSELL, sel, 1..=2, p, moved, clicked, right, |w, id| {
            if let Some(s) = w.auction.as_mut().and_then(|a| a.shopsell.as_mut()) {
                s.0 = id;
            }
        }, keys);
        return;
    }
    match a.top() {
        Some(Layer::Menu(name, sel)) => {
            let last = menu_rows(name).iter().map(|r| r.0).max().unwrap_or(plain_rows(name));
            generic(w, gu, name, sel, 1..=last, p, moved, clicked, right, |w, id| {
                if let Some(a) = w.auction.as_mut() {
                    set_top(a, Layer::Menu(name, id));
                }
            }, keys);
        }
        Some(Layer::List) => {
            let cursor = a.list.as_ref().map_or(0, |l| l.cursor) as u16 + 1;
            generic(w, gu, LIST, cursor, 1..=ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(l) = w.auction.as_mut().and_then(|a| a.list.as_mut()) {
                    l.cursor = id as usize - 1;
                }
            }, keys);
        }
        Some(Layer::History) => {
            let cursor = a.history.as_ref().map_or(0, |h| h.cursor) as u16 + 1;
            generic(w, gu, HISTORY, cursor, 1..=ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(h) = w.auction.as_mut().and_then(|a| a.history.as_mut()) {
                    h.cursor = id as usize - 1;
                }
            }, keys);
        }
        Some(Layer::Sell(scroll, cursor)) => {
            let n = sell_rows(w).len();
            let Some(r) = gu.window_rect("menu    inventor", sw, sh) else { return };
            let on = p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
            if right && on {
                key(w, egui::Key::Escape);
                return;
            }
            let row = ((p.1 - r[1] - 4.0) / crate::inventory::ROW_PITCH).floor();
            if !on || row < 0.0 || row as usize >= crate::inventory::VISIBLE_ROWS || scroll + row as usize >= n {
                return;
            }
            gu.hover.set(true);
            let row = row as usize;
            if row != cursor && (moved || clicked) {
                if let Some(a) = w.auction.as_mut() {
                    set_top(a, Layer::Sell(scroll, row));
                }
                w.sounds.push(Ui::Cursor as u32);
            } else if clicked {
                key(w, egui::Key::Enter);
            }
        }
        None => {}
    }
}

/// The bars: the focused window's title and help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(a) = &w.auction else { return };
    bars.title[1] = gu.title_msg(0x12).unwrap_or_default();
    match a.top() {
        Some(Layer::Menu(name, sel)) => {
            let [help, title] = gu.focus_text(name, sel);
            bars.help[1] = help.unwrap_or_default();
            if let Some(t) = title.filter(|t| !t.is_empty()) {
                bars.title[1] = t;
            }
        }
        Some(Layer::List) => {
            let l = a.list.as_ref();
            if l.is_some_and(|l| !l.sales) {
                bars.title[1] = gu.title_msg(0x79).unwrap_or_default();
                bars.help[1] = gu.menu_msg(3, "");
            } else if let Some(Row::Slot(k)) = l.and_then(|l| l.rows.get(l.scroll + l.cursor).copied()) {
                bars.help[1] = stat_look(w, &w.ah_slots[k as usize]).1.map_or_else(String::new, |h| gu.menu_msg(h, ""));
            }
        }
        Some(Layer::History) => {
            bars.title[1] = gu.title_msg(0x77).unwrap_or_default();
            bars.help[1] = gu.menu_msg(0x224, "");
        }
        Some(Layer::Sell(..)) => bars.help[1] = gu.menu_msg(if a.money.is_some() { 0x37 } else if a.shopsell.is_some() { 0x38 } else { 0x36 }, ""),
        None => {}
    }
}

/// Draws the windows; returns their rects.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(a) = &w.auction else { return rects };
    let focus = a.money.is_none() && a.shopsell.is_none() && w.confirm.is_none();
    let n = a.layers.len();
    for (i, l) in a.layers.iter().enumerate() {
        let focused = focus && i + 1 == n;
        match *l {
            Layer::Menu(name, sel) => {
                let off = if name == AUC3 && cursor_row(a).is_some_and(|r| auc3_dim(w, r)) { vec![2u16] } else { Vec::new() };
                // FUN_101d9200: the name toggle's label, windowps 151 "Reset List" with a saved sort, else 537 "Sort".
                let swaps: Vec<(u16, Option<&str>, usize)> = if name == AUC3 && sort_kind(a.cat) == 2 { vec![(3, Some(WINDOWPS), if a.sorts.any(a.cat) { 151 } else { 537 })] } else { Vec::new() };
                if let Some(win) = gu.lobby_window_lib_swaps(name, sw, sh, (0.0, 0.0), Some(sel), focused, &off, &swaps) {
                    super::winhits::record(w, name, &win);
                    let [x, y, ..] = win.frame;
                    gu.disabled_children(name, x, y, &off, focused.then_some(sel));
                    rects.push(win.frame);
                }
            }
            Layer::List => rects.extend(draw_list(gu, w, a, focused, sw, sh)),
            Layer::History => rects.extend(draw_history(gu, w, a, focused, sw, sh)),
            Layer::Sell(scroll, cursor) => rects.extend(draw_sell(gu, w, scroll, cursor, focused, sw, sh)),
        }
    }
    if let Some((step, max, v, _)) = a.money
        && let Some(win) = gu.lobby_window_off(MONEYCTR, sw, sh, (0.0, 0.0), Some(1), true, &[])
    {
        super::winhits::record(w, MONEYCTR, &win);
        super::post::draw_moneyctr(gu, win.frame, step, max, v);
        rects.push(win.frame);
    }
    if let Some((sel, ..)) = a.shopsell
        && let Some(win) = gu.lobby_window_off(SHOPSELL, sw, sh, (0.0, 0.0), Some(sel), true, &[])
    {
        super::winhits::record(w, SHOPSELL, &win);
        rects.push(win.frame);
    }
    rects
}

/// The list window's frame and its rows' origin.
fn list_frame(gu: &mut GameUi, name: &str, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let r = gu.window_rect(name, sw, sh)?;
    if !gu.gate_rect(name, r) {
        return None;
    }
    gu.frame(r[0], r[1], r[2], r[3]);
    Some(r)
}

/// auclist (3.2.2 / 3.6): the half-size icon, the name, "[n]" right-aligned at 275 (bid) or the price (sales).
fn draw_list(gu: &mut GameUi, w: &World, a: &Auction, focused: bool, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let l = a.list.as_ref()?;
    let info = w.item_info.clone()?;
    let r = list_frame(gu, LIST, sw, sh)?;
    let (ox, oy) = (r[0] + 4.0, r[1] + 4.0);
    let shown: Vec<Row> = l.rows.iter().skip(l.scroll).take(ROWS).copied().collect();
    super::winhits::record_rects(w, LIST, r, (0..shown.len()).map(|k| (k as u16 + 1, [ox, oy + PITCH * k as f32, r[2] - 12.0, PITCH])).collect());
    super::items::upload_icons(gu, &info, shown.iter().map(|x| x.item(&w.ah_slots)));
    for (k, row) in shown.iter().enumerate() {
        let y = oy + PITCH * k as f32;
        let item = row.item(&w.ah_slots);
        let icon = crate::render::invwin::icon_texture(item);
        if gu.has_image(&icon) {
            gu.image_rect(&icon, ox, y, 16.0, 16.0, [1.0; 4]);
        }
        let name = info.name(item).unwrap_or_default();
        let stack = info.item(item).map_or(1, |it| it.stack.max(1));
        match *row {
            Row::Single { single, .. } => {
                gu.text(&name, ox + 18.0, y + 2.0, 0, super::shop::name_tint(w, item).to_le_bytes());
                gu.shape_text(&format!("[{single}]"), ox + 275.0, y + 4.0, WHITE, true);
            }
            Row::Stack { stack: n, .. } => {
                gu.text(&name, ox + 18.0, y + 2.0, 0, STACK_NAME.to_le_bytes());
                gu.shape_text(&stack.to_string(), ox + 16.0, y + 4.0, WHITE, true);
                gu.shape_text(&format!("[{n}]"), ox + 275.0, y + 4.0, WHITE, true);
            }
            Row::Slot(s) => {
                let p = &w.ah_slots[s as usize];
                gu.text(&name, ox + 18.0, y + 2.0, 0, stat_look(w, p).0.to_le_bytes());
                if p.qty != 1 {
                    gu.shape_text(&stack.to_string(), ox + 16.0, y + 4.0, WHITE, true);
                }
                gu.shape_text(&crate::inventory::gil_text(p.price as i64), ox + 275.0, y + 4.0, WHITE, true);
            }
        }
        if focused && k == l.cursor {
            gu.cursor(ox, y);
        }
    }
    let empty = if !l.ready {
        None
    } else if l.failed {
        Some(0x65)
    } else if l.rows.is_empty() {
        Some(if l.sales { 0x6a } else { 0x6b })
    } else {
        None
    };
    if let Some(t) = empty.and_then(|id| w.battle.as_ref().and_then(|b| b.group7(id))) {
        gu.text(&t, r[0] + 32.0, r[1] + 24.0, 0, WHITE.to_le_bytes());
    }
    gu.scroll_bar(r[0] + r[2] - 6.0, r[1], r[3], l.rows.len(), ROWS, l.scroll);
    if focused {
        gu.bar_site(super::scrollbar::Owner::Auction, r[0] + r[2] - 6.0, r[1], r[3], l.rows.len(), ROWS, l.scroll);
    }
    Some(r)
}

/// auchisto (3.4): the date, seller, "→", buyer, "N G"; in sell mode a head row with the category and the count.
fn draw_history(gu: &mut GameUi, w: &World, a: &Auction, focused: bool, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let h = a.history.as_ref()?;
    let r = list_frame(gu, HISTORY, sw, sh)?;
    let (ox, oy) = (r[0] + 4.0, r[1] + 4.0);
    let arrow = w.battle.as_ref().and_then(|b| b.group9(0x3b)).unwrap_or_else(|| ">".into());
    let n = h.sales.len().saturating_sub(h.scroll).min(ROWS);
    super::winhits::record_rects(w, HISTORY, r, (0..n).map(|k| (k as u16 + 1, [ox, oy + PITCH * k as f32, r[2] - 12.0, PITCH])).collect());
    for (k, s) in h.sales.iter().skip(h.scroll).take(ROWS).enumerate() {
        // The small sprite font (FUN_101c9780) in columns {0, 65, 145, 165, 250, 350}, the price right-aligned at the
        // last column's end - 2; the arrow in the game font (the sprite font has no glyph for it).
        let y = oy + PITCH * k as f32;
        gu.shape_text(&sale_date(w, s.date, 2), ox, y + 4.0, WHITE, false);
        gu.shape_text(&short(&s.seller), ox + 65.0, y + 4.0, WHITE, false);
        gu.text(&arrow, ox + 145.0, y + 2.0, 0, WHITE.to_le_bytes());
        gu.shape_text(&short(&s.buyer), ox + 165.0, y + 4.0, WHITE, false);
        gu.shape_text(&crate::inventory::gil_text(s.price as i64), ox + 348.0, y + 4.0, WHITE, true);
        if focused && k == h.cursor {
            gu.cursor(ox, y);
        }
    }
    let empty = if !h.ready {
        None
    } else if h.failed {
        Some(0x65)
    } else if h.sales.is_empty() {
        Some(0x6b)
    } else {
        None
    };
    if let Some(t) = empty.and_then(|id| w.battle.as_ref().and_then(|b| b.group7(id))) {
        gu.text(&t, r[0] + 32.0, r[1] + 24.0, 0, WHITE.to_le_bytes());
    }
    Some(r)
}

/// FUN_101ca180: a name past 8 characters cut to 7 and an ellipsis.
fn short(s: &str) -> String {
    if s.chars().count() > 8 { format!("{}...", s.chars().take(7).collect::<String>()) } else { s.to_string() }
}

/// The inventory list with the auction source (3.5).
fn draw_sell(gu: &mut GameUi, w: &World, scroll: usize, cursor: usize, focused: bool, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let info = w.item_info.clone()?;
    let rows = sell_rows(w);
    let mut list = crate::inventory::ItemList { rows: crate::inventory::build_rows(&rows, info.as_ref(), false), scroll, cursor };
    for (r, s) in list.rows.iter_mut().zip(&rows) {
        r.tint = sell_tint(w, s);
    }
    super::items::upload_icons(gu, &info, list.visible().iter().map(|r| r.id));
    let open = gu.gate_layout("menu    inventor", sw, sh, None);
    let frame = gu.cursor_frame();
    let (b, ui, style, font) = gu.parts();
    let m = ui.menu("menu    inventor").filter(|_| open)?;
    let [bx, by, bh] = crate::render::invwin::push_inventory(b, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, if focused { frame } else { usize::MAX });
    if focused {
        gu.bar_site(super::scrollbar::Owner::Auction, bx, by, bh, list.rows.len(), crate::inventory::VISIBLE_ROWS, scroll);
    }
    gu.window_rect("menu    inventor", sw, sh)
}

/// A list row as the modern layer shows it: the item, the name's tint, the stack size shown before the name (stack
/// forms, sales of a stack), the cell at the right ("[n]" listed, the price, the count held).
pub(super) struct PaneRow {
    pub item: u16,
    pub tint: u32,
    pub stack: Option<u32>,
    pub right: String,
}

/// A window of the stack as the modern layer shows it.
pub(super) enum Pane {
    /// A command menu: its name, cursor, rows off and windowps label swaps.
    Menu { name: &'static str, sel: u16, off: Vec<u16>, labels: Vec<(u16, usize)> },
    /// auclist (bid or sales mode) and the line shown when it is empty or failed.
    List { sales: bool, rows: Vec<PaneRow>, cursor: usize, note: Option<String> },
    /// auchisto: date, seller, buyer, price per sale.
    History { item: u16, rows: Vec<[String; 4]>, cursor: usize, note: Option<String> },
    /// The inventory offered for sale.
    Sell { rows: Vec<PaneRow>, cursor: usize },
}

/// What the modern layer shows: the stack's windows oldest first (the last has the keys unless the price or shopsell is
/// asked), moneyctr's (step, max, value) and what it is for (the item), shopsell's cursor, item and price.
pub(super) struct Look {
    pub panes: Vec<Pane>,
    pub money: Option<(u32, u32, u32, u16)>,
    pub shopsell: Option<(u16, u16, u32)>,
    pub busy: bool,
}

pub(super) fn look(w: &World) -> Option<Look> {
    let a = w.auction.as_ref()?;
    let info = w.item_info.as_ref()?;
    let empty = |ready: bool, failed: bool, none: bool, sales: bool| {
        let id = if !ready {
            None
        } else if failed {
            Some(0x65)
        } else if none {
            Some(if sales { 0x6a } else { 0x6b })
        } else {
            None
        };
        id.and_then(|id| w.battle.as_ref().and_then(|b| b.group7(id)))
    };
    let mut panes = Vec::new();
    for l in &a.layers {
        panes.push(match *l {
            Layer::Menu(name, sel) => {
                let off = if name == AUC3 && cursor_row(a).is_some_and(|r| auc3_dim(w, r)) { vec![2u16] } else { Vec::new() };
                let labels = if name == AUC3 && sort_kind(a.cat) == 2 { vec![(3, if a.sorts.any(a.cat) { 151 } else { 537 })] } else { Vec::new() };
                Pane::Menu { name, sel, off, labels }
            }
            Layer::List => {
                let Some(list) = a.list.as_ref() else { continue };
                let rows = list
                    .rows
                    .iter()
                    .map(|r| {
                        let item = r.item(&w.ah_slots);
                        let stack = info.item(item).map_or(1, |it| it.stack.max(1) as u32);
                        match *r {
                            Row::Single { single, .. } => PaneRow { item, tint: super::shop::name_tint(w, item), stack: None, right: format!("[{single}]") },
                            Row::Stack { stack: n, .. } => PaneRow { item, tint: STACK_NAME, stack: Some(stack), right: format!("[{n}]") },
                            Row::Slot(k) => {
                                let p = &w.ah_slots[k as usize];
                                PaneRow { item, tint: stat_look(w, p).0, stack: (p.qty != 1).then_some(stack), right: crate::inventory::gil_text(p.price as i64) }
                            }
                        }
                    })
                    .collect();
                Pane::List { sales: list.sales, rows, cursor: list.scroll + list.cursor, note: empty(list.ready, list.failed, list.rows.is_empty(), list.sales) }
            }
            Layer::History => {
                let Some(h) = a.history.as_ref() else { continue };
                let rows = h.sales.iter().map(|x| [sale_date(w, x.date, 2), x.seller.clone(), x.buyer.clone(), crate::inventory::gil_text(x.price as i64)]).collect();
                Pane::History { item: h.item, rows, cursor: h.scroll + h.cursor, note: empty(h.ready, h.failed, h.sales.is_empty(), false) }
            }
            Layer::Sell(scroll, cursor) => {
                let rows = sell_rows(w)
                    .iter()
                    .map(|x| {
                        let stack = info.item(x.id).map_or(1, |it| it.stack.max(1) as u32);
                        PaneRow { item: x.id, tint: sell_tint(w, x), stack: None, right: if stack > 1 { x.count.to_string() } else { String::new() } }
                    })
                    .collect();
                Pane::Sell { rows, cursor: scroll + cursor }
            }
        });
    }
    let money = a.money.map(|(step, max, v, purpose)| {
        let item = match purpose {
            MoneyFor::Bid(r) => r.item(&w.ah_slots),
            MoneyFor::Sell { item, .. } => item,
        };
        (step, max, v, item)
    });
    Some(Look { panes, money, shopsell: a.shopsell.map(|(sel, _, item, price, _)| (sel, item, price)), busy: a.busy() })
}

/// The top menu's cursor onto `sel` while it has the keys. True when it moved.
pub(super) fn set_menu(w: &mut World, sel: u16) -> bool {
    let Some(a) = w.auction.as_mut().filter(|a| a.money.is_none() && a.shopsell.is_none()) else { return false };
    match a.layers.last_mut() {
        Some(Layer::Menu(_, s)) if *s != sel => {
            *s = sel;
            true
        }
        _ => false,
    }
}

/// The top list's (auclist, auchisto, the sell list) cursor onto row `at`, the scroll following. True when it moved.
pub(super) fn set_row(w: &mut World, at: usize) -> bool {
    let n_sell = sell_rows(w).len();
    let Some(a) = w.auction.as_mut().filter(|a| a.money.is_none() && a.shopsell.is_none()) else { return false };
    let place = |scroll: &mut usize, cursor: &mut usize, n: usize, rows: usize| {
        if at >= n || *scroll + *cursor == at {
            return false;
        }
        if at < *scroll || at >= *scroll + rows {
            *scroll = (at + 1).saturating_sub(rows);
        }
        *cursor = at - *scroll;
        true
    };
    match a.layers.last().copied() {
        Some(Layer::List) => a.list.as_mut().is_some_and(|l| place(&mut l.scroll, &mut l.cursor, l.rows.len(), ROWS)),
        Some(Layer::History) => a.history.as_mut().is_some_and(|h| place(&mut h.scroll, &mut h.cursor, h.sales.len(), ROWS)),
        Some(Layer::Sell(mut scroll, mut cursor)) => {
            let moved = place(&mut scroll, &mut cursor, n_sell, crate::inventory::VISIBLE_ROWS);
            set_top(a, Layer::Sell(scroll, cursor));
            moved
        }
        _ => false,
    }
}

/// moneyctr's value outright (All, 0).
pub(super) fn set_money(w: &mut World, v: u32) {
    if let Some(m) = w.auction.as_mut().and_then(|a| a.money.as_mut()) {
        m.2 = v.min(m.1);
    }
}

/// shopsell's cursor (1 Sell, 2 Cancel). True when it moved.
pub(super) fn set_shopsell(w: &mut World, sel: u16) -> bool {
    match w.auction.as_mut().and_then(|a| a.shopsell.as_mut()) {
        Some(s) if s.0 != sel => {
            s.0 = sel;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packet layout (1.1) and the menu rows' order.
    #[test]
    fn packets_and_rows() {
        let mut p = vec![0u8; 0x3c];
        (p[4], p[5], p[6]) = (0xe, 2, 1);
        p[8..12].copy_from_slice(&15u32.to_le_bytes());
        p[0xc..0xe].copy_from_slice(&4096u16.to_le_bytes());
        p[0x10..0x14].copy_from_slice(&12u32.to_le_bytes());
        p[0x14] = 3;
        p[0x18..0x1c].copy_from_slice(b"Nine");
        p[0x28..0x2a].copy_from_slice(&4096u16.to_le_bytes());
        let a = AucPacket::parse(&p);
        assert_eq!((a.cmd, a.slot, a.result, a.p8, a.pc, a.p10, a.parcel.stat, a.parcel.name.as_str(), a.parcel.item), (0xe, 2, 1, 15, 4096, 12, 3, "Nine", 4096));
        assert_eq!(step_row("menu    aucitem", 1, true), 7);
        assert_eq!(step_row("menu    aucitem", 8, true), 2);
        assert_eq!(step_row("menu    aucitem", 1, false), 6);
        assert_eq!(step_row(AUC1, 3, true), 1);
        assert_eq!((market(Some(230)), market(Some(100))), (4, 0));
        assert_eq!(short("Abcdefghij"), "Abcdefg...");
    }

    /// AUCSORT.DAT round trip, the add rules and the param words (3.3, 2.1).
    #[test]
    fn saved_sorts() {
        let mut s = Sorts::default();
        s.add(4, 5);
        s.add(4, 5);
        s.add(4, 4);
        s.add(4, 6);
        assert_eq!(s.of(4), &[0x15, 0x06]);
        let b = s.bytes();
        assert_eq!(b.len(), SORT_FILE);
        assert_eq!(Sorts::parse(&b), s);
        let mut bad = b.clone();
        bad[9] ^= 1;
        assert_eq!(Sorts::parse(&bad), Sorts::default());
        assert_eq!((sort_param(5), sort_param(9), filter_param(4, 6)), ((5, 2), (9, 0), (4, 1 | 6 << 2)));
        assert_eq!((sort_kind(4), sort_kind(0x30), sort_kind(0x12), sort_kind(0x21)), (0, 0, 1, 2));
        let mut c = Vec::new();
        cond_add(&mut c, sort_param(5));
        cond_add(&mut c, sort_param(5));
        assert_eq!(c.len(), 1);
    }
}
