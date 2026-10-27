//! The Adventuring Primer (primer_spec.md): "menu    guide00" lists the categories (item records 0xf620.., at most
//! 8), "menu    guide01" one category's topics (records 0xf630..0xf71f), and a topic's pages are illustrations the
//! map window shows in mode 7 (map.rs `Mode::Primer`): file (number + 0x83b1) x 3 + page. No packets.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const LIST: &str = "menu    guide00";
pub const PAGES: &str = "menu    guide01";
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;
const ROWS: usize = 8;

struct Topic {
    name: String,
    /// +4: the image number; +5: how many images (1..3).
    num: u8,
    images: u8,
}

struct Category {
    name: String,
    topics: Vec<Topic>,
}

/// guide01 over guide00: the category, top, cursor; `page` while a page image is up (no hand, no bar).
struct List {
    cat: usize,
    top: usize,
    cursor: usize,
    page: bool,
}

pub struct Primer {
    cats: Vec<Category>,
    /// +0x8c: retail frames since open; the rows are built at the 10th (FUN_10233cf0).
    frames: f32,
    cursor: usize,
    /// +0x64 / +0x65: the main menu closed at open (its row and page), reopened at close.
    back: Option<(usize, u8)>,
    list: Option<List>,
    /// Title slot 2 holds group 9 0xdd "Help Desk": opened from the Help Desk or back from a page (LAB_101f7b40).
    desk_title: bool,
}

/// FUN_10234010: the contiguous run of categories from 0xf620 with topics, at most 8.
fn load(w: &World) -> Vec<Category> {
    let Some(info) = w.item_info.clone() else { return Vec::new() };
    let mut cats = Vec::new();
    for k in 0..ROWS as u16 {
        let Some((name, ids)) = info.guide(0xf620 + k).filter(|g| !g.1.is_empty()) else { break };
        let topics = ids
            .iter()
            .filter_map(|&id| {
                let it = info.item(id)?;
                let extra = |at: u16| it.extra.iter().find(|e| e.0 == at).map_or(0, |e| e.1);
                Some(Topic { name: it.name.clone(), num: extra(0xd88) as u8, images: extra(0xd8c) as u8 })
            })
            .collect();
        cats.push(Category { name, topics });
    }
    cats
}

/// FUN_10233b80 (/guide, /primer, Help Desk row 1): guide00; its open hook (FUN_10233c50) closes the main menu or the
/// social page beside it, to reopen them at close. Nothing while it is open.
pub fn open(w: &mut World, from_desk: bool) {
    if w.primer.is_some() {
        return;
    }
    let cats = load(w);
    log::info!("primer: {} categories", cats.len());
    let back = w.menu.take().map(|m| (m, w.menu_page));
    w.primer = Some(Primer { cats, frames: 0.0, cursor: 0, back, list: None, desk_title: from_desk });
}

/// FUN_10234080 / FUN_10233e90: guide00 and guide01 close, the menu closed at open comes back.
fn close(w: &mut World) {
    if let Some(p) = w.primer.take()
        && let Some((m, page)) = p.back
    {
        (w.menu, w.menu_page) = (Some(m), page);
    }
}

/// Per frame: the rows' wait; busy (FUN_10087070(0): an event, frozen, charmed, fishing) closes the windows and the
/// page with no callback (FUN_10233cf0, FUN_10234140, FUN_101fa900).
pub fn update(w: &mut World, ticks: f32) {
    if w.primer.is_none() {
        return;
    }
    if super::textcmd::busy(w) {
        if let Some(m) = w.map.as_mut().filter(|m| m.primer_page().is_some()) {
            m.primer_next = None;
            w.game.set(0x8a, m.zoom_cfg());
            m.begin_close();
        }
        close(w);
        return;
    }
    if let Some(p) = w.primer.as_mut() {
        p.frames += ticks / 2.0;
    }
}

fn ready(p: &Primer) -> bool {
    p.frames >= 10.0
}

/// The page's close (LAB_101f7b40): guide01's hand and bar back, title slot 2 "Help Desk".
pub fn map_back(w: &mut World) {
    if let Some(p) = w.primer.as_mut()
        && let Some(l) = p.list.as_mut()
    {
        l.page = false;
        p.desk_title = true;
    }
}

/// guide00 (FUN_10233e30) and guide01 (FUN_10234370). `page_mod`: Ctrl / Alt held (key function 0xa8).
pub fn key(w: &mut World, key: egui::Key, page_mod: bool) {
    use egui::Key as K;
    let Some(p) = w.primer.as_mut() else { return };
    if let Some(l) = p.list.as_mut() {
        if l.page {
            return;
        }
        let n = p.cats[l.cat].topics.len();
        match key {
            K::ArrowUp | K::ArrowDown if n > 0 => {
                l.cursor = if key == K::ArrowDown { (l.cursor + 1) % n } else { (l.cursor + n - 1) % n };
                l.top = l.top.min(l.cursor).max((l.cursor + 1).saturating_sub(ROWS));
                w.sounds.push(Ui::Cursor as u32);
            }
            // With 0xa8 the list's page (0x19 / 0x18); plain, the next / previous category with SE 0001 always
            // (FUN_10234280: FUN_10233f90 / FUN_10233fd0, the cursor and top back to 0).
            K::ArrowLeft | K::ArrowRight if page_mod => {
                let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
                let mut row = l.cursor - l.top;
                if super::scrollbar::paged(m, &mut l.top, &mut row, n, ROWS) {
                    l.cursor = l.top + row;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::ArrowLeft | K::ArrowRight => {
                let c = p.cats.len();
                l.cat = if key == K::ArrowRight { (l.cat + 1) % c } else { (l.cat + c - 1) % c };
                (l.top, l.cursor) = (0, 0);
                w.sounds.push(Ui::Cursor as u32);
            }
            // FUN_102342c0: the page in the map window (FUN_101f7ad0(number, images, 0)); title slot 2 cleared.
            K::Enter => {
                if let Some(t) = p.cats[l.cat].topics.get(l.cursor).filter(|t| t.images > 0) {
                    w.sounds.push(Ui::Select as u32);
                    l.page = true;
                    p.desk_title = false;
                    w.map_cmds.push(super::map::Cmd::Primer { first: (t.num as u32 + 0x83b1) * 3, count: t.images, at: 0 });
                }
            }
            // FUN_10234230(1): guide00's hand and bar back, its cursor kept.
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                p.list = None;
            }
            _ => {}
        }
        return;
    }
    let n = p.cats.len();
    match key {
        K::ArrowUp | K::ArrowDown if n > 0 => {
            p.cursor = if key == K::ArrowDown { (p.cursor + 1) % n } else { (p.cursor + n - 1) % n };
            w.sounds.push(Ui::Cursor as u32);
        }
        // 3 / 4 as 0x19 / 0x18 with the whole list visible: the last / first row.
        K::ArrowLeft | K::ArrowRight if n > 0 => {
            let to = if key == K::ArrowRight { n - 1 } else { 0 };
            if to != p.cursor {
                p.cursor = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // FUN_10233ba0: guide01 on that category (FUN_10234540).
        K::Enter if n > 0 => {
            w.sounds.push(Ui::Select as u32);
            p.list = Some(List { cat: p.cursor, top: 0, cursor: 0, page: false });
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        _ => {}
    }
}

/// guide01's bar and the wheel (the paged base, as quest01).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(p) = w.primer.as_mut() else { return false };
    let Some(l) = p.list.as_mut().filter(|l| !l.page) else { return false };
    let n = p.cats[l.cat].topics.len();
    let mut row = l.cursor - l.top;
    let moved = super::scrollbar::paged(m, &mut l.top, &mut row, n, ROWS);
    l.cursor = l.top + row;
    moved
}

/// The Primer as the modern layer draws it: the categories and guide00's cursor, guide01's category name, topics and
/// cursor; `page` while a page image is up.
pub(super) struct Look {
    pub cats: Vec<String>,
    pub cursor: usize,
    pub list: Option<(String, Vec<String>, usize)>,
    pub page: bool,
}

pub(super) fn look(w: &World) -> Option<Look> {
    let p = w.primer.as_ref()?;
    let cats = if ready(p) { p.cats.iter().map(|c| c.name.clone()).collect() } else { Vec::new() };
    let list = p.list.as_ref().and_then(|l| p.cats.get(l.cat).map(|c| (c.name.clone(), c.topics.iter().map(|t| t.name.clone()).collect(), l.cursor)));
    Some(Look { cats, cursor: p.cursor, list, page: p.list.as_ref().is_some_and(|l| l.page) })
}

/// The mouse's cursor moves: guide00's row (while guide01 is closed), guide01's row. True when it moved.
pub(super) fn set_cat(w: &mut World, k: usize) -> bool {
    let Some(p) = w.primer.as_mut().filter(|p| p.list.is_none() && p.cursor != k && k < p.cats.len()) else { return false };
    p.cursor = k;
    true
}

pub(super) fn set_topic(w: &mut World, k: usize) -> bool {
    let Some(p) = w.primer.as_mut() else { return false };
    let Some(l) = p.list.as_mut().filter(|l| !l.page && l.cursor != k && k < p.cats[l.cat].topics.len()) else { return false };
    l.cursor = k;
    l.top = l.top.min(k).max((k + 1).saturating_sub(ROWS));
    true
}

/// The mouse (winhits.rs): guide00 / guide01 rows as the generic window, guide01's arrows (children 9 / 10) as Left /
/// Right; with a page up a click is its Enter (FUN_101fa900: every left release) and a right click its Escape.
pub fn pointer(w: &mut World, gu: &GameUi, pt: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::{child_under, generic};
    let Some(p) = w.primer.as_ref() else { return };
    // The modern layer draws the lists and takes their mouse; a page stays the map window's.
    if w.modern && !p.list.as_ref().is_some_and(|l| l.page) {
        return;
    }
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k, false);
    match &p.list {
        Some(l) if l.page => {
            let k = if clicked { egui::Key::Enter } else if right { egui::Key::Escape } else { return };
            if let Some(m) = w.map.as_mut().filter(|m| m.primer_page().is_some())
                && !m.key(k)
            {
                w.game.set(0x8a, m.zoom_cfg());
                m.begin_close();
            }
        }
        Some(l) => {
            if clicked && let (Some(a), _) = child_under(w, PAGES, pt, 9..=10) {
                gu.hover.set(true);
                key(w, if a == 9 { egui::Key::ArrowLeft } else { egui::Key::ArrowRight }, false);
                return;
            }
            let top = l.top;
            generic(w, gu, PAGES, (l.cursor - top + 1) as u16, 1..=ROWS as u16, pt, moved, clicked, right, |w, id| {
                if let Some(l) = w.primer.as_mut().and_then(|p| p.list.as_mut()) {
                    l.cursor = top + id as usize - 1;
                }
            }, keys);
        }
        None => {
            generic(w, gu, LIST, p.cursor as u16 + 1, 1..=ROWS as u16, pt, moved, clicked, right, |w, id| {
                if let Some(p) = w.primer.as_mut() {
                    p.cursor = id as usize - 1;
                }
            }, keys);
        }
    }
}

/// Title slot 1 group 9 0xc5 "Help Desk" (the frame hooks), slot 2 its 0xdd from the Help Desk or after a page; no help
/// (the layouts carry no strings).
pub fn bars(_gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(p) = &w.primer else { return };
    let g9 = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
    if !p.list.as_ref().is_some_and(|l| l.page) {
        bars.title[1] = g9(0xc5);
    }
    bars.title[2] = if p.desk_title { g9(0xdd) } else { String::new() };
    (bars.help[1], bars.help[2]) = (String::new(), String::new());
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// guide00 (FUN_10221f50 with LAB_10233dc0's rows: framesus 35 at (4, 5), the name at (16, 0)), suspended while a page
/// is up; guide01 over it (FUN_10234270: the rows, the category's name at (60, 10), the bar at x1 - 8).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(p) = &w.primer else { return rects };
    let free = w.confirm.is_none();
    let page = p.list.as_ref().is_some_and(|l| l.page);
    if !page {
        // Before the 10th frame the list has no rows *(inf: retail shows the row plates empty for that third of a second)*.
        let n = if ready(p) { p.cats.len() } else { 0 };
        let hide: Vec<u16> = (1..=ROWS as u16).filter(|&k| k as usize > n).collect();
        let focused = free && p.list.is_none();
        let sel = (n > 0 && p.list.is_none()).then_some(p.cursor as u16 + 1);
        if let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, focused, &hide) {
            super::winhits::record(w, LIST, &win);
            for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=ROWS as u16).contains(id)) {
                let k = id as usize - 1;
                let Some(c) = p.cats.get(k).filter(|_| k < n) else { continue };
                gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
                text(gu, &c.name, cx + 16.0, cy, if k == p.cursor { CURSOR } else { WHITE });
            }
            rects.push(win.frame);
        }
    }
    let Some(l) = &p.list else { return rects };
    let Some(cat) = p.cats.get(l.cat) else { return rects };
    let n = cat.topics.len();
    let hide: Vec<u16> = (1..=ROWS as u16).filter(|&k| l.top + k as usize > n).collect();
    let sel = (!page && n > 0).then(|| (l.cursor - l.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(PAGES, sw, sh, (0.0, 0.0), sel, free && !page, &hide) else { return rects };
    super::winhits::record(w, PAGES, &win);
    let [fx, fy, fw, fh] = win.frame;
    if let Some((x, y)) = gu.window_xy(PAGES, sw, sh) {
        text(gu, &cat.name, x + 60.0, y + 10.0, WHITE);
    }
    for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=ROWS as u16).contains(id)) {
        let k = l.top + id as usize - 1;
        let Some(t) = cat.topics.get(k) else { continue };
        gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
        text(gu, &t.name, cx + 16.0, cy, if k == l.cursor { CURSOR } else { WHITE });
    }
    if !page {
        gu.scroll_bar(fx + fw - 8.0, fy, fh, n, ROWS, l.top);
        if free {
            gu.bar_site(super::scrollbar::Owner::Primer, fx + fw - 8.0, fy, fh, n, ROWS, l.top);
        }
    }
    rects.push(win.frame);
    rects
}
