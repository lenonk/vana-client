//! The social page's Key Items row (/keyitem, 0x4c; quest_mission_spec section 7): "menu    evitem" (the ten
//! categories, one category's key items in variable-height pages, a description) and "menu    evitem01" ("Examine
//! %s?" on a new one, which marks it seen with c2s 0x064).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const WINDOW: &str = "menu    evitem";
pub const ASK: &str = "menu    evitem01";
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;
/// The categories show 8 of their 10 rows.
const CAT_ROWS: usize = 8;
/// FUN_1022a590's page budget (window bottom - 12 - 12) and the gap between rows.
const BUDGET: f32 = 224.0;
const GAP: f32 = 12.0;
/// The name's wrap width (0x12d - 0x12).
const NAME_W: f32 = 283.0;

/// The seen bits (DAT_104800b8), the last packet's (0x104802bc), DAT_104802b8's low word (tables that merge) and its
/// received flag.
#[derive(Clone, Debug)]
pub struct Seen {
    pub local: [u32; 128],
    last: [u32; 128],
    merge: u16,
    pub received: bool,
}

impl Default for Seen {
    fn default() -> Self {
        Seen { local: [0; 128], last: [0; 128], merge: 0, received: false }
    }
}

impl Seen {
    /// s2c 0x055's seen bits (FUN_10098710, asm 0x10098790..b2): a table marked to merge keeps the marks made here
    /// (local |= ~last & new); the others take the packet. Only the first table ever received is marked.
    pub fn packet(&mut self, table: usize, new: &[u32; 16]) {
        let r = table * 16..table * 16 + 16;
        for (k, i) in r.clone().enumerate() {
            self.local[i] = if self.merge >> table & 1 != 0 { self.local[i] | (!self.last[i] & new[k]) } else { new[k] };
        }
        self.last[r].copy_from_slice(new);
        if !self.received {
            self.merge |= 1 << table;
            self.received = true;
        }
    }

    pub fn seen(&self, id: u16) -> bool {
        self.local.get(id as usize >> 5).is_some_and(|w| w >> (id & 31) & 1 != 0)
    }
}

#[derive(Clone, Debug)]
pub struct KeyItems {
    /// +0x90: 0 waiting for a 0x055, 1 the categories, 2 a category's items, 3 a description.
    pub state: u8,
    /// +0x74: the category cursor; the list's top / cursor (categories: rows; items: entries).
    pub cat: usize,
    pub top: usize,
    pub cursor: usize,
    /// The shown category's owned key items (id, list name) in file order.
    pub items: Vec<(u16, String)>,
    /// evitem01 is up (the cursor: true on Yes).
    pub ask: Option<bool>,
}

/// The ten categories (FUN_1022a360): each '-' header closes the entries before it, titled by its own name.
pub fn categories(list: &[(u32, String)]) -> Vec<(String, Vec<(u16, String)>)> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for (id, name) in list {
        match name.strip_prefix('-') {
            Some(title) => out.push((title.to_string(), std::mem::take(&mut cur))),
            None => cur.push((*id as u16, name.clone())),
        }
    }
    out
}

fn all(w: &World) -> Vec<(String, Vec<(u16, String)>)> {
    w.names.as_ref().map_or(Vec::new(), |n| categories(&n.key_item_list()))
}

/// /keyitem (FUN_1007d220): evitem, waiting until a 0x055 has come.
pub fn open(w: &mut World) {
    let state = if w.ki_seen.received { 1 } else { 0 };
    w.keyitems = Some(KeyItems { state, cat: 0, top: 0, cursor: 0, items: Vec::new(), ask: None });
}

pub fn update(w: &mut World) {
    let received = w.ki_seen.received;
    if let Some(k) = w.keyitems.as_mut()
        && k.state == 0
        && received
    {
        k.state = 1;
    }
}

pub fn busy(w: &World) -> bool {
    w.keyitems.as_ref().is_some_and(|k| k.state == 0)
}

/// FUN_10233380: a row's height: 16 a wrapped line at 283 px, 18 at least.
fn heights(gu: &GameUi, items: &[(u16, String)]) -> Vec<f32> {
    items.iter().map(|(_, n)| (gu.wrap(n, NAME_W).len().max(1) as f32 * 16.0).max(18.0)).collect()
}

/// FUN_1022a590: the rows a page from `start` holds: add a row's height; stop at the budget or 8 rows, else add the
/// gap and count it (7 one-line rows fit).
pub fn fit(h: &[f32], start: usize) -> usize {
    let (mut sum, mut n) = (0.0, 0);
    for &hh in h.iter().skip(start) {
        sum += hh;
        if sum >= BUDGET || n == 8 {
            break;
        }
        sum += GAP;
        n += 1;
    }
    n.max(1).min(h.len().saturating_sub(start))
}

/// FUN_10221a80(n, 1)'s max top: the first start whose page reaches the end.
pub fn max_top(h: &[f32]) -> usize {
    (0..h.len()).find(|&i| i + fit(h, i) >= h.len()).unwrap_or(0)
}

/// FUN_10221e50 / FUN_10221dc0 *(inf: the exact rule)*: a cursor above the page makes it the top; one below it moves
/// the top to the first start whose page holds it.
pub fn follow(h: &[f32], top: usize, cursor: usize) -> usize {
    if cursor < top {
        cursor
    } else if cursor >= top + fit(h, top) {
        (0..=cursor).find(|&i| i + fit(h, i) > cursor).unwrap_or(cursor).min(max_top(h))
    } else {
        top
    }
}

/// 0x19 / 0x18: the next page start (top + its rows), the previous one (the first start whose page reaches the top).
pub fn page(h: &[f32], top: usize, forward: bool) -> usize {
    if forward { (top + fit(h, top)).min(max_top(h)) } else { (0..top).find(|&i| i + fit(h, i) >= top).unwrap_or(0) }
}

fn show_category(w: &mut World, cat: usize) {
    let cats = all(w);
    let items: Vec<(u16, String)> = cats.get(cat).map_or(Vec::new(), |c| c.1.iter().filter(|(id, _)| w.has_key_item(*id)).cloned().collect());
    if let Some(k) = w.keyitems.as_mut() {
        (k.state, k.cat, k.items, k.top, k.cursor) = (2, cat, items, 0, 0);
    }
}

/// c2s 0x064 (FUN_10097f10): our UniqueNo, the table's 16 seen words, our ActIndex, the table.
fn mark_seen(w: &mut World, id: u16) {
    let word = id as usize >> 5;
    if word >= 128 {
        return;
    }
    w.ki_seen.local[word] |= 1 << (id & 31);
    let table = word >> 4;
    let mut b = vec![0u8; 0x48];
    b[0..4].copy_from_slice(&w.my_id.to_le_bytes());
    for k in 0..16 {
        b[4 + 4 * k..8 + 4 * k].copy_from_slice(&w.ki_seen.local[table * 16 + k].to_le_bytes());
    }
    b[0x44..0x46].copy_from_slice(&w.my_index.to_le_bytes());
    b[0x46..0x48].copy_from_slice(&(table as u16).to_le_bytes());
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x064, body: b });
}

/// FUN_1022aaa0 (and FUN_10222710 for evitem01).
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key, page_mod: bool) {
    let Some(mut k) = w.keyitems.clone() else { return };
    if let Some(yes) = k.ask {
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                k.ask = Some(!yes);
                w.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Enter | egui::Key::Escape => {
                w.sounds.push(if key == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
                k.ask = None;
                if key == egui::Key::Enter && yes {
                    if let Some(&(id, _)) = k.items.get(k.cursor) {
                        mark_seen(w, id);
                    }
                    k.state = 3;
                }
            }
            _ => {}
        }
        w.keyitems = Some(k);
        return;
    }
    let h = heights(gu, &k.items);
    match (k.state, key) {
        (0 | 1, egui::Key::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            w.keyitems = None;
            return;
        }
        (1, egui::Key::ArrowUp | egui::Key::ArrowDown) => {
            let n = all(w).len().max(1);
            k.cursor = if key == egui::Key::ArrowDown { (k.cursor + 1) % n } else { (k.cursor + n - 1) % n };
            if k.cursor < k.top {
                k.top = k.cursor;
            } else if k.cursor >= k.top + CAT_ROWS {
                k.top = k.cursor + 1 - CAT_ROWS;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        (1, egui::Key::ArrowLeft | egui::Key::ArrowRight) => {
            let n = all(w).len();
            let max = n.saturating_sub(CAT_ROWS);
            let row = k.cursor - k.top;
            k.top = if key == egui::Key::ArrowRight { (k.top + CAT_ROWS).min(max) } else { k.top.saturating_sub(CAT_ROWS) };
            k.cursor = (k.top + row).min(n.saturating_sub(1));
            w.sounds.push(Ui::Cursor as u32);
        }
        (1, egui::Key::Enter) => {
            w.sounds.push(Ui::Select as u32);
            let cat = k.cursor;
            w.keyitems = Some(k);
            show_category(w, cat);
            return;
        }
        (2, egui::Key::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            (k.state, k.cursor, k.top) = (1, k.cat, k.cat.saturating_sub(CAT_ROWS - 1).min(k.cat));
        }
        (2, egui::Key::ArrowUp | egui::Key::ArrowDown) if !k.items.is_empty() => {
            let n = k.items.len();
            k.cursor = if key == egui::Key::ArrowDown { (k.cursor + 1) % n } else { (k.cursor + n - 1) % n };
            k.top = follow(&h, k.top, k.cursor);
            w.sounds.push(Ui::Cursor as u32);
        }
        (2, egui::Key::ArrowLeft | egui::Key::ArrowRight) if !k.items.is_empty() => {
            let _ = page_mod;
            let t = page(&h, k.top, key == egui::Key::ArrowRight);
            if t != k.top {
                (k.top, k.cursor) = (t, t);
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        (2, egui::Key::Enter) => {
            w.sounds.push(Ui::Select as u32);
            if let Some(&(id, _)) = k.items.get(k.cursor) {
                if w.ki_seen.seen(id) { k.state = 3 } else { k.ask = Some(true) }
            }
        }
        (3, egui::Key::Enter | egui::Key::Escape) => {
            w.sounds.push(if key == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
            k.state = 2;
        }
        _ => {}
    }
    w.keyitems = Some(k);
}

/// Mouse *(inf: the paged lists' hover and click)*: hover moves the cursor to a row, a click on the cursor row is Enter;
/// on evitem01 the same over Yes / No.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(k) = w.keyitems.clone() else { return };
    let inside = |r: [f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    let rows: Vec<(usize, [f32; 4])> = if k.ask.is_some() {
        let Some((x, y)) = gu.window_xy(ASK, sw, sh) else { return };
        gu.layout(ASK).map_or(Vec::new(), |l| l.elements.iter().skip(1).map(|e| (e.id as usize - 1, [x + e.x as f32, y + e.y as f32, e.w as f32, e.h as f32])).collect())
    } else {
        let Some((x, y)) = gu.window_xy(WINDOW, sw, sh) else { return };
        match k.state {
            1 => {
                let n = all(w).len();
                gu.layout(WINDOW).map_or(Vec::new(), |l| l.elements.iter().skip(1).map(|e| (k.top + e.id as usize - 1, [x + e.x as f32, y + e.y as f32, e.w as f32, e.h as f32])).filter(|(r, _)| *r < n).collect())
            }
            2 => {
                let h = heights(gu, &k.items);
                let mut ry = y + 12.0;
                (k.top..k.top + fit(&h, k.top).min(k.items.len() - k.top.min(k.items.len())))
                    .map(|i| {
                        let r = (i, [x + 40.0, ry, NAME_W + 36.0, h[i]]);
                        ry += h[i] + GAP;
                        r
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    };
    let Some(&(row, _)) = rows.iter().find(|(_, r)| inside(*r)) else {
        if clicked && k.state == 3 {
            key(w, gu, egui::Key::Enter, false);
        }
        return;
    };
    gu.hover.set(true);
    let cur = match k.ask {
        Some(yes) => if yes { 0 } else { 1 },
        None => k.cursor,
    };
    if row != cur && (moved || clicked) {
        if let Some(kk) = w.keyitems.as_mut() {
            match kk.ask.as_mut() {
                Some(yes) => *yes = row == 0,
                None => kk.cursor = row,
            }
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter, false);
    }
}

/// The scroll bar: the categories (10 rows, 8 shown) or the items (variable pages: shown = total - max top).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(k) = w.keyitems.as_mut().filter(|k| k.state == 1 && k.ask.is_none()) else { return false };
    let mut row = k.cursor.saturating_sub(k.top);
    let moved = super::scrollbar::paged(m, &mut k.top, &mut row, 10, CAT_ROWS);
    k.cursor = k.top + row;
    moved
}

/// Title "Key Items" (frame string 1); help slot 2 the category name while its items show (FUN_1013a320(name, 2)).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(k) = &w.keyitems else { return };
    (bars.help[1], bars.title[1]) = (String::new(), gu.focus_text(WINDOW, 0)[1].clone().unwrap_or_default());
    if k.state >= 2
        && let Some(c) = all(w).get(k.cat)
    {
        bars.help[2] = c.0.clone();
    }
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// FUN_1022a8b0: the waiting lines, the categories (framesus 35 at child (22, 5), the name at (36, 0)), a category's
/// items (rows from y 12 at their own heights, + 12 between: seen framesus 35 at (22, 5), new keytops3 170 at (18, 1),
/// the name wrapped at 283), the empty line, the description box; evitem01's question and Yes / No over it.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(k) = &w.keyitems else { return rects };
    let cats = all(w);
    let focused = k.ask.is_none() && w.confirm.is_none();
    let rows: Vec<u16> = (1..=8).collect();
    let sel = (k.state == 1 && k.ask.is_none()).then(|| (k.cursor - k.top + 1) as u16);
    let hide: Vec<u16> = if k.state == 1 { (1..=8u16).filter(|&c| k.top + c as usize - 1 >= cats.len()).collect() } else { rows };
    let Some(win) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), sel, focused, &hide) else { return rects };
    let [fx, fy, fw, fh] = win.frame;
    let Some((x, y)) = gu.window_xy(WINDOW, sw, sh) else { return rects };
    rects.push(win.frame);
    let bt = w.battle.clone();
    // evitem01 turns evitem's hand, rows and bar off while it is up (FUN_10221c30 / FUN_10221c50).
    match k.state {
        _ if k.ask.is_some() => {}
        0 => {
            if let Some(bt) = &bt {
                text(gu, &bt.group9(0x61).unwrap_or_default(), x + 20.0, y + 20.0, WHITE);
                text(gu, &bt.group9(0x62).unwrap_or_default(), x + 20.0, y + 40.0, WHITE);
            }
        }
        1 => {
            for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=8).contains(id)) {
                let row = k.top + id as usize - 1;
                let Some(c) = cats.get(row) else { continue };
                gu.sprite("menu    framesus", 35, cx + 22.0, cy + 5.0, WHITE);
                text(gu, &c.0, cx + 36.0, cy, if row == k.cursor { CURSOR } else { WHITE });
            }
            let bx = fx + fw - 8.0;
            gu.scroll_bar(bx, fy, fh, cats.len(), CAT_ROWS, k.top);
            gu.bar_site(super::scrollbar::Owner::Journal, bx, fy, fh, cats.len(), CAT_ROWS, k.top);
        }
        2 if k.items.is_empty() => {
            if let Some(bt) = &bt {
                text(gu, &bt.group9(0x60).unwrap_or_default(), x + 20.0, y + 20.0, WHITE);
            }
        }
        2 => {
            let h = heights(gu, &k.items);
            let n = fit(&h, k.top);
            let mut ry = y + 12.0;
            for i in k.top..k.top + n {
                let (id, name) = &k.items[i];
                let cx = x + 40.0;
                let on = i == k.cursor && k.ask.is_none();
                if w.ki_seen.seen(*id) {
                    gu.sprite("menu    framesus", 35, cx + 22.0, ry + 5.0, WHITE);
                } else {
                    gu.sprite("menu    keytops3", 170, cx + 18.0, ry + 1.0, WHITE);
                }
                super::missions::lines(gu, name, cx + 36.0, ry, NAME_W, if i == k.cursor { CURSOR } else { WHITE });
                if on && focused {
                    gu.cursor(cx, ry);
                }
                ry += h[i] + GAP;
            }
            let (mt, bx) = (max_top(&h), fx + fw - 8.0);
            gu.scroll_bar(bx, fy, fh, k.items.len(), k.items.len() - mt, k.top);
        }
        _ => {
            if let Some((id, name)) = k.items.get(k.cursor) {
                gu.sprite("menu    framesus", 35, x + 36.0, y + 25.0, WHITE);
                text(gu, name, x + 50.0, y + 20.0, WHITE);
                let desc = w.names.as_ref().and_then(|n| n.key_item_description(*id as u32)).unwrap_or_default();
                super::missions::lines(gu, &desc, x + 40.0, y + 50.0, 327.0, WHITE);
            }
        }
    }
    // evitem01 (FUN_10222600): "Examine %s?" (group 9 0x5f) at (28, 30), Yes / No (0xc 0x2b / 0x2c) at its children.
    if let Some(yes) = k.ask
        && let Some(win) = gu.lobby_window_off(ASK, sw, sh, (0.0, 0.0), Some(if yes { 1 } else { 2 }), w.confirm.is_none(), &[])
    {
        rects.push(win.frame);
        if let Some((ax, ay)) = gu.window_xy(ASK, sw, sh) {
            let name = k.items.get(k.cursor).map_or("", |i| i.1.as_str());
            let q = bt.as_ref().and_then(|b| b.group9(0x5f)).unwrap_or_default().replacen("%s", name, 1);
            text(gu, &q, ax + 28.0, ay + 30.0, WHITE);
        }
        for &(id, [cx, cy, ..]) in &win.children {
            let label = gu.lobby_msg(0xc, if id == 1 { 0x2b } else { 0x2c }, "");
            text(gu, &label, cx, cy, if (id == 1) == yes { CURSOR } else { WHITE });
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages() {
        // Seven one-line rows fit (pitch 30): 7 * 18 + 6 * 12 = 198, the eighth would end at 228.
        let one = vec![18.0; 20];
        assert_eq!((fit(&one, 0), max_top(&one), page(&one, 0, true), page(&one, 7, false)), (7, 13, 7, 0));
        let mut mixed = vec![18.0; 10];
        mixed[2] = 48.0;
        assert_eq!(fit(&mixed, 0), 6);
        assert_eq!(follow(&mixed, 0, 6), 1);
        assert_eq!(follow(&mixed, 3, 1), 1);
    }

    #[test]
    fn seen_merge() {
        let mut s = Seen::default();
        s.packet(0, &[0b01; 16]);
        s.local[0] |= 0b10;
        // Table 0 merges: the mark made here stays, new server bits are added.
        s.packet(0, &[0b101; 16]);
        assert_eq!(s.local[0], 0b111);
        // Table 1 was not first: it takes the packet.
        s.packet(1, &[0b1; 16]);
        s.local[16] |= 0b10;
        s.packet(1, &[0b100; 16]);
        assert_eq!(s.local[16], 0b100);
        assert!(s.seen(2) && !s.seen(3));
    }

    /// The English table (55705): ten sections, each closed by its header.
    #[test]
    fn real_sections() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(n) = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En) else { return };
        let c = categories(&n.key_item_list());
        let counts: Vec<usize> = c.iter().map(|s| s.1.len()).collect();
        assert_eq!(c[0].0, "Temporary Key Items");
        assert_eq!(counts, [1632, 389, 412, 117, 102, 40, 127, 134, 89, 192]);
    }

    #[test]
    fn sections() {
        let list = vec![(1, "a".to_string()), (2, "b".to_string()), (0, "-First".to_string()), (3, "c".to_string()), (0, "-Second".to_string())];
        let c = categories(&list);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].0.as_str(), c[0].1.len(), c[1].0.as_str(), c[1].1.len()), ("First", 2, "Second", 1));
    }
}
