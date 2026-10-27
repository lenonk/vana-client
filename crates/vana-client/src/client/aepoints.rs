//! statcom2 row 13, Alter Ego Points (spec ~/ffxi/lab/research/status_rows_spec.md 5): fp_cat (the categories) with
//! fp_info and bluehelp, fp_cat2 over it (the category's attributes), raising through merityn.

use super::World;
use super::game_ui::GameUi;
use super::merits::{PointList, PointRow, bluehelp};
use crate::audio::Ui;
use crate::dat::merits::AeTables;
use std::sync::Arc;

const CATS: &str = "menu    fp_cat";
const ATTRS: &str = "menu    fp_cat2";
const INFO: &str = "menu    fp_info";
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const BLUE: u32 = 0x8020_60a0;
const CURSOR: u32 = 0x8080_3040;
/// DAT_10371870: the cell columns of fp_cat and fp_cat2.
const COLS: [f32; 6] = [0.0, 6.0, 252.0, 253.0, 290.0, 357.0];
const ROWS: usize = 10;

/// s2c 0x08E (FUN_100e5140): the points, the upgrades and the next costs by attribute id.
#[derive(Clone, Copy, Debug)]
pub struct AeData {
    pub points: u16,
    pub upgrades: [u8; 32],
    pub costs: [u16; 32],
}

pub struct AeUi {
    cursor: usize,
    top: usize,
    /// fp_cat2: the category and its cursor and top.
    attrs: Option<(u16, usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub entry: u16,
}

pub(super) fn tables(w: &mut World) -> Option<Arc<AeTables>> {
    if w.ae_tables.is_none() {
        let index = w.index.clone()?;
        w.ae_tables = Some(Arc::new(AeTables::load(&index, w.point_files[3])));
    }
    w.ae_tables.clone()
}

/// s2c 0x08E: session +0x2d614 / +0x2d638 and +0x40db5 bit 4.
pub fn on_data(w: &mut World, d: AeData) {
    w.ae_data = Some(d);
}

/// statcom2 Enter on row 13 (k 12): fp_cat and fp_info; c2s 0x0C2 while no 0x08E came.
pub fn open(w: &mut World) {
    tables(w);
    if w.ae_data.is_none() {
        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0C2, body: Vec::new() });
    }
    w.ae_ui = Some(AeUi { cursor: 0, top: 0, attrs: None });
}

/// fp_cat2 +0x5b: in the Mog House and not in Monstrosity.
fn can_spend(w: &World) -> bool {
    w.myroom.is_some() && !super::monstrosity::active(w)
}

/// An attribute row's colours (FUN_1014bdb0): name, level (blue when maxed), cost (None = no cost cell).
fn colours(w: &World, t: &AeTables, e: u16) -> (u32, u32, Option<u32>) {
    let Some(d) = w.ae_data.filter(|_| can_spend(w)) else { return (GREY, GREY, None) };
    let i = e as usize % 32;
    if d.upgrades[i] >= t.max.get(e as usize).copied().unwrap_or(0) {
        return (WHITE, BLUE, None);
    }
    (WHITE, WHITE, Some(if d.costs[i] > d.points { GREY } else { WHITE }))
}

/// merityn's Yes (LAB_1014c180): c2s 0x0C1 {u16 entry}.
pub fn answer(w: &mut World, a: Ask) {
    let mut body = a.entry.to_le_bytes().to_vec();
    body.extend([0, 0]);
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0C1, body });
}

/// Group 10 0x7b "Use <n> alter ego point(s) to raise your "<name>" by one level?" with costs[e].
pub fn ask_text(w: &World, a: Ask) -> String {
    let (Some(bt), Some(t)) = (&w.battle, &w.ae_tables) else { return String::new() };
    let Some(raw) = bt.group10_raw(0x7b) else { return String::new() };
    let name = crate::dat::font::encode_sjis(&t.name(a.entry));
    let cost = w.ae_data.map_or(0, |d| d.costs[a.entry as usize % 32] as i32);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&name], &[cost], &|_, _, _| None))
}

fn scroll(cursor: &mut usize, top: &mut usize, to: usize) {
    *cursor = to;
    *top = (*top).min(to).max((to + 1).saturating_sub(ROWS));
}

/// Up / Down a row, Left / Right a page with Ctrl or Alt (+0x47 = 0); Enter opens a category / raises; Escape goes
/// back.
pub fn key(w: &mut World, key: egui::Key, page_mod: bool) {
    use egui::Key as K;
    let Some(t) = w.ae_tables.clone() else { return };
    let Some(u) = w.ae_ui.as_ref() else { return };
    let (n, cursor) = match u.attrs {
        Some((k, c, _)) => (t.entries(k).len(), c),
        None => (t.cats.len(), u.cursor),
    };
    match key {
        K::ArrowUp | K::ArrowDown => {}
        K::ArrowLeft | K::ArrowRight if page_mod => {}
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            if let Some(u) = w.ae_ui.as_mut()
                && u.attrs.take().is_none()
            {
                w.ae_ui = None;
            }
            return;
        }
        K::Enter => {
            match u.attrs {
                None => {
                    let Some(&(k, _)) = t.cats.get(cursor) else { return };
                    w.sounds.push(Ui::Select as u32);
                    if let Some(u) = w.ae_ui.as_mut() {
                        u.attrs = Some((k, 0, 0));
                    }
                }
                Some((k, c, _)) => {
                    if w.myroom.is_none() {
                        super::textcmd::refuse(w, 0xb2);
                        w.sounds.push(Ui::Error as u32);
                        return;
                    }
                    match t.entries(k).get(c).copied().filter(|&e| colours(w, &t, e).2 == Some(WHITE)) {
                        Some(entry) => {
                            w.sounds.push(Ui::Select as u32);
                            w.confirm = Some((super::Confirm::AlterEgo(Ask { entry }), false));
                        }
                        None => w.sounds.push(Ui::Error as u32),
                    }
                }
            }
            return;
        }
        _ => return,
    }
    // The row list's page (FUN_101f6c70 0x19 / 0x18): the top by a page, the screen row kept.
    if matches!(key, K::ArrowLeft | K::ArrowRight) {
        let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
        let moved = w.ae_ui.as_mut().is_some_and(|u| match u.attrs.as_mut() {
            Some((_, c, top)) => super::scrollbar::rows_abs(m, top, c, n, ROWS),
            None => super::scrollbar::rows_abs(m, &mut u.top, &mut u.cursor, n, ROWS),
        });
        if moved {
            w.sounds.push(Ui::Cursor as u32);
        }
        return;
    }
    let to = if key == K::ArrowDown { (cursor + 1).min(n.saturating_sub(1)) } else { cursor.saturating_sub(1) };
    if to == cursor {
        return;
    }
    if let Some(u) = w.ae_ui.as_mut() {
        match u.attrs.as_mut() {
            Some((_, c, top)) => scroll(c, top, to),
            None => scroll(&mut u.cursor, &mut u.top, to),
        }
    }
    w.sounds.push(Ui::Cursor as u32);
}

/// The focused list's bar (FUN_101f6c70).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(t) = w.ae_tables.clone() else { return false };
    let Some(u) = w.ae_ui.as_mut() else { return false };
    match u.attrs.as_mut() {
        Some((k, c, top)) => super::scrollbar::rows_abs(m, top, c, t.entries(*k).len(), ROWS),
        None => super::scrollbar::rows_abs(m, &mut u.top, &mut u.cursor, t.cats.len(), ROWS),
    }
}

/// The Alter Ego Points windows as the modern layer draws them: the points, fp_cat, fp_cat2 while open.
pub(super) struct Look {
    pub points: Option<u16>,
    pub cats: PointList,
    pub attrs: Option<PointList>,
}

pub(super) fn look(w: &World) -> Option<Look> {
    let u = w.ae_ui.as_ref()?;
    let t = w.ae_tables.clone()?;
    let free = w.confirm.is_none();
    let rows = t.cats.iter().map(|&(k, _)| PointRow { name: t.name(k), tint: WHITE, ..Default::default() }).collect();
    let help = t.cats.get(u.cursor).map_or((String::new(), String::new()), |&(k, _)| (t.name(k), t.desc(k)));
    let cats = PointList { rows, cursor: u.cursor, focused: free && u.attrs.is_none(), help };
    let attrs = u.attrs.map(|(k, cursor, _)| {
        let es = t.entries(k);
        let rows = es
            .iter()
            .map(|&e| {
                let (c, lc, cost) = colours(w, &t, e);
                let i = e as usize % 32;
                let level = w.ae_data.map_or(0, |d| d.upgrades[i]);
                let max = t.max.get(e as usize).copied().unwrap_or(0);
                let next = w.ae_data.map_or(0, |d| d.costs[i]);
                PointRow { name: t.name(e), tint: c, level: Some((format!("{level}/{max}"), lc)), cost: cost.map(|cc| (format!("{next} P"), cc)), ..Default::default() }
            })
            .collect();
        let help = es.get(cursor).map_or((String::new(), String::new()), |&e| (t.name(e), t.desc(e)));
        PointList { rows, cursor, focused: free, help }
    });
    Some(Look { points: w.ae_data.map(|d| d.points), cats, attrs })
}

/// The mouse's cursor moves: a category (fp_cat), an attribute (fp_cat2). True when it moved.
pub(super) fn set_cat(w: &mut World, k: usize) -> bool {
    let n = w.ae_tables.as_ref().map_or(0, |t| t.cats.len());
    let Some(u) = w.ae_ui.as_mut().filter(|u| u.attrs.is_none() && u.cursor != k && k < n) else { return false };
    scroll(&mut u.cursor, &mut u.top, k);
    true
}

pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(t) = w.ae_tables.clone() else { return false };
    let Some((cat, c, top)) = w.ae_ui.as_mut().and_then(|u| u.attrs.as_mut()) else { return false };
    if *c == k || k >= t.entries(*cat).len() {
        return false;
    }
    scroll(c, top, k);
    true
}

/// The mouse on the focused list (the generic window's rows).
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    // The modern layer draws them and takes their mouse.
    if w.modern {
        return;
    }
    let Some(t) = w.ae_tables.clone() else { return };
    let Some(u) = w.ae_ui.as_ref() else { return };
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k, false);
    match u.attrs {
        Some((k, cursor, top)) => {
            super::winhits::generic(w, gu, ATTRS, (cursor - top + 1) as u16, 1..=ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(u) = w.ae_ui.as_mut() {
                    u.attrs = Some((k, top + id as usize - 1, top));
                }
            }, keys);
        }
        None if !t.cats.is_empty() => {
            let (top, cursor) = (u.top, u.cursor);
            super::winhits::generic(w, gu, CATS, (cursor - top + 1) as u16, 1..=ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(u) = w.ae_ui.as_mut() {
                    u.cursor = top + id as usize - 1;
                }
            }, keys);
        }
        None => {}
    }
}

/// Help 0xe 1097 (fp_cat) / 1098 (fp_cat2), title 0xd 130.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(u) = &w.ae_ui else { return };
    bars.help[1] = gu.menu_msg(if u.attrs.is_some() { 1098 } else { 1097 }, "");
    bars.help[2] = String::new();
    bars.title[2] = gu.title_msg(130).unwrap_or_default();
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(u) = &w.ae_ui else { return rects };
    let Some(t) = w.ae_tables.clone() else { return rects };
    let free = w.confirm.is_none();
    let right = |gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32| gu.shape_text(s, x, y, tint, true);
    // fp_info (FUN_1014c1b0): points / 1000 at (x1 - 8, y0 + 0x14), blue above 999.
    if let Some(win) = gu.lobby_window_off(INFO, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, fw, _] = win.frame;
        let p = w.ae_data.map_or(0, |d| d.points);
        right(gu, &format!("{p}/1000"), x + fw - 8.0, y + 20.0, if p > 999 { BLUE } else { WHITE });
        rects.push(win.frame);
    }
    // fp_cat: the categories, white, rows 17 apart from (0, 4); suspended (not drawn) while fp_cat2, of its group, is up.
    let k = t.cats.get(u.cursor).map_or(0, |c| c.0);
    let (cat_name, cat_desc) = (t.name(k), t.desc(k));
    let n = t.cats.len();
    let hide: Vec<u16> = (1..=ROWS as u16).filter(|&k| u.top + k as usize > n).collect();
    let sel = (n > 0 && u.attrs.is_none()).then(|| (u.cursor - u.top + 1) as u16);
    if let Some(win) = gu.lobby_window_off(CATS, sw, sh, (0.0, 0.0), sel, free && u.attrs.is_none(), &hide) {
        super::winhits::record(w, CATS, &win);
        let [x0, y0, fw, fh] = win.frame;
        for (i, &(k, _)) in t.cats.iter().enumerate().skip(u.top).take(ROWS) {
            let y = y0 + 4.0 + 17.0 * (i - u.top) as f32;
            let c = if i == u.cursor && u.attrs.is_none() { CURSOR } else { WHITE };
            gu.text(&t.name(k), x0 + COLS[1], y + 2.0, 0, c.to_le_bytes());
        }
        if n > ROWS {
            gu.scroll_bar(x0 + fw - 8.0, y0, fh, n, ROWS, u.top);
            if free && u.attrs.is_none() {
                gu.bar_site(super::scrollbar::Owner::Points, x0 + fw - 8.0, y0, fh, n, ROWS, u.top);
            }
        }
        rects.push(win.frame);
    }
    let (name, desc) = match u.attrs {
        Some((k, cursor, top)) => {
            let es = t.entries(k);
            let n = es.len();
            let hide: Vec<u16> = (1..=ROWS as u16).filter(|&r| top + r as usize > n).collect();
            let sel = (n > 0).then(|| (cursor - top + 1) as u16);
            let Some(win) = gu.lobby_window_off(ATTRS, sw, sh, (0.0, 0.0), sel, free, &hide) else { return rects };
            super::winhits::record(w, ATTRS, &win);
            let [x0, y0, fw, fh] = win.frame;
            for (i, &e) in es.iter().enumerate().skip(top).take(ROWS) {
                let y = y0 + 17.0 + 17.0 * (i - top) as f32;
                let (c, lc, cost) = colours(w, &t, e);
                let level = w.ae_data.map_or(0, |d| d.upgrades[e as usize % 32]);
                gu.text(&t.name(e), x0 + COLS[1], y + 2.0, 0, if i == cursor { CURSOR } else { c }.to_le_bytes());
                right(gu, &level.to_string(), x0 + COLS[2], y + 4.0, lc);
                gu.shape_text(&format!("/{}", t.max.get(e as usize).copied().unwrap_or(0)), x0 + COLS[3], y + 4.0, lc, false);
                if let Some(cc) = cost {
                    let next = w.ae_data.map_or(0, |d| d.costs[e as usize % 32]);
                    gu.shape_text(&gu.lobby_msg(0xc, 0x5a, "NEXT"), x0 + COLS[4], y + 4.0, cc, false);
                    right(gu, &format!("{next} P"), x0 + COLS[5], y + 4.0, cc);
                }
            }
            if n > ROWS {
                gu.scroll_bar(x0 + fw - 8.0, y0, fh, n, ROWS, top);
                if free {
                    gu.bar_site(super::scrollbar::Owner::Points, x0 + fw - 8.0, y0, fh, n, ROWS, top);
                }
            }
            rects.push(win.frame);
            es.get(cursor).map_or((String::new(), String::new()), |&e| (t.name(e), t.desc(e)))
        }
        None => (cat_name, cat_desc),
    };
    rects.extend(bluehelp(gu, sw, sh, &name, &desc));
    rects
}
