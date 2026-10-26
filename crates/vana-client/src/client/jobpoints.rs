//! statcom2 row 8, Job Points (spec ~/ffxi/lab/research/status_rows_spec.md 2): merit2ca (the jobs) with levmeri2 and
//! bluehelp, jbpcat over it (page 0 the job's categories, page 1 its gifts), raising through merityn.

use super::World;
use super::game_ui::GameUi;
use super::merits::bluehelp;
use crate::audio::Ui;
use crate::dat::merits::{Gifts, JobPointTables};
use std::sync::Arc;

const JOBS: &str = "menu    merit2ca";
const PAGE: &str = "menu    jbpcat";
const LEV: &str = "menu    levmeri2";
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
const BLUE: u32 = 0x8020_60a0;
const CURSOR: u32 = 0x8080_3040;
/// DAT_10371864: the cell columns of merit2ca and jbpcat.
const COLS: [f32; 6] = [0.0, 6.0, 252.0, 253.0, 290.0, 357.0];
const JOB_ROWS: usize = 11;
const PAGE_ROWS: usize = 10;

struct Page {
    job: u8,
    /// +0x58: 0 the job point categories, 1 the gifts.
    page: usize,
    /// Per page: the cursor row and the top (+0x60 / +0x62).
    cursor: [usize; 2],
    top: [usize; 2],
    gifts: Gifts,
}

pub struct JpUi {
    cursor: usize,
    top: usize,
    page: Option<Page>,
    /// DAT_1057844c: frames before another raise may ask.
    hold: f32,
    /// The gift names' file (language slot 0x1f).
    gift_names: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub entry: u16,
}

fn tables(w: &mut World, gu: &GameUi) -> Option<Arc<JobPointTables>> {
    if w.jp_tables.is_none() {
        let index = w.index.clone()?;
        w.jp_tables = Some(Arc::new(JobPointTables::load(&index, gu.point_names(1))));
    }
    w.jp_tables.clone()
}

/// s2c 0x08D: the entries' levels and next costs; session +0x40db5 bit 0.
pub fn on_list(w: &mut World, recs: &[(u16, u8, u16)]) {
    for &(id, level, next) in recs {
        w.jp_list.insert(id, (level, next));
    }
    w.jp_seen = true;
}

/// statcom2 Enter on row 8 (k 7): merit2ca; c2s 0x0C0 while no 0x08D came this session.
pub fn open(w: &mut World, gu: &GameUi) {
    tables(w, gu);
    if !w.jp_seen {
        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0C0, body: Vec::new() });
    }
    w.jp_ui = Some(JpUi { cursor: 0, top: 0, page: None, hold: 0.0, gift_names: gu.point_names(2) });
}

fn job_level(w: &World, job: u8) -> u8 {
    w.job_info.as_ref().and_then(|j| j.levels.get(job as usize).copied()).unwrap_or(0)
}

/// 0x063 type 5: job `job`'s (capacity, current, spent).
fn points(w: &World, job: u8) -> Option<[u16; 3]> {
    w.jp_jobs.and_then(|j| j.get(job as usize).copied())
}

/// jbpcat +0x5e: in the Mog House and not in Monstrosity.
fn can_spend(w: &World) -> bool {
    w.myroom.is_some() && w.my_plate.monstrosity == 0
}

/// A page-0 row's colours (FUN_1014b0d0): name, level (blue when maxed), cost (None = no cost cell).
fn colours(w: &World, t: &JobPointTables, job: u8, e: u16) -> (u32, u32, Option<u32>) {
    let Some(&(level, next)) = w.jp_list.get(&e).filter(|s| can_spend(w) && (s.0 != 0 || s.1 != 0)) else { return (GREY, GREY, None) };
    if level >= t.max.get(e as usize).copied().unwrap_or(0) {
        return (WHITE, BLUE, None);
    }
    let have = points(w, job).map_or(0, |p| p[1]);
    (WHITE, WHITE, Some(if have >= next { WHITE } else { GREY }))
}

fn page_len(p: &Page, t: &JobPointTables) -> usize {
    if p.page == 0 { t.entries(p.job).len() } else { p.gifts.needs.iter().filter(|&&n| n != 0).count() }
}

/// The gifts shown (threshold != 0), in table order.
fn gift_rows(g: &Gifts) -> Vec<usize> {
    (0..g.needs.len()).filter(|&i| g.needs[i] != 0).collect()
}

/// merityn's Yes (LAB_1014b610): c2s 0x0BF {u16 entry}; no new prompt for 30 frames.
pub fn answer(w: &mut World, a: Ask) {
    let mut body = a.entry.to_le_bytes().to_vec();
    body.extend([0, 0]);
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0BF, body });
    if let Some(u) = w.jp_ui.as_mut() {
        u.hold = 30.0;
    }
}

/// Group 10 0x74 "Use <n> job point(s) to raise your "<name>" by one level?".
pub fn ask_text(w: &World, a: Ask) -> String {
    let (Some(bt), Some(t)) = (&w.battle, &w.jp_tables) else { return String::new() };
    let Some(raw) = bt.group10_raw(0x74) else { return String::new() };
    let name = crate::dat::font::encode_sjis(&t.name(a.entry));
    let cost = w.jp_list.get(&a.entry).map_or(0, |s| s.1 as i32);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&name], &[cost], &|_, _, _| None))
}

/// The hold after a raise counts down (frames at 30 a second *(inf: retail counts its frames)*).
pub fn update(w: &mut World, ticks: f32) {
    if let Some(u) = w.jp_ui.as_mut() {
        u.hold = (u.hold - ticks / 2.0).max(0.0);
    }
}

fn scroll(cursor: &mut usize, top: &mut usize, to: usize, rows: usize) {
    *cursor = to;
    *top = (*top).min(to).max((to + 1).saturating_sub(rows));
}

pub fn key(w: &mut World, key: egui::Key, page_mod: bool) {
    use egui::Key as K;
    let Some(t) = w.jp_tables.clone() else { return };
    let Some(u) = w.jp_ui.as_ref() else { return };
    let jobs = t.jobs.len();
    // jbpcat.
    if let Some(p) = &u.page {
        let (job, pg, n) = (p.job, p.page, page_len(p, &t));
        let cursor = p.cursor[pg];
        match key {
            // FUN_1014acc0: the other page, back on its row (FUN_101f7080: top = min(row, the last top)).
            K::ArrowLeft | K::ArrowRight if !page_mod => {
                if let Some(p) = w.jp_ui.as_mut().and_then(|u| u.page.as_mut()) {
                    p.page = 1 - p.page;
                    let (q, n) = (p.page, page_len(p, &t));
                    p.top[q] = p.cursor[q].min(n.saturating_sub(PAGE_ROWS));
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            // With Ctrl / Alt, the row list's page (FUN_101f6c70 0x19 / 0x18): the top by 10, the screen row kept.
            K::ArrowLeft | K::ArrowRight => {
                let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
                if let Some(p) = w.jp_ui.as_mut().and_then(|u| u.page.as_mut())
                    && super::scrollbar::rows_abs(m, &mut p.top[pg], &mut p.cursor[pg], n, PAGE_ROWS)
                {
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::ArrowUp | K::ArrowDown => {
                let to = if key == K::ArrowDown { (cursor + 1).min(n.saturating_sub(1)) } else { cursor.saturating_sub(1) };
                if to != cursor && let Some(p) = w.jp_ui.as_mut().and_then(|u| u.page.as_mut()) {
                    let (c, tp) = (&mut p.cursor[pg], &mut p.top[pg]);
                    scroll(c, tp, to, PAGE_ROWS);
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                if let Some(u) = w.jp_ui.as_mut() {
                    u.page = None;
                }
            }
            K::Enter => {
                if w.myroom.is_none() {
                    super::textcmd::refuse(w, 0xb2);
                    w.sounds.push(Ui::Error as u32);
                    return;
                }
                let e = t.entries(job).get(cursor).copied();
                let ok = pg == 0 && e.is_some_and(|e| colours(w, &t, job, e).2 == Some(WHITE));
                match e.filter(|_| ok) {
                    Some(entry) => {
                        w.sounds.push(Ui::Select as u32);
                        if w.jp_ui.as_ref().is_some_and(|u| u.hold <= 0.0) {
                            w.confirm = Some((super::Confirm::JobPoint(Ask { entry }), false));
                        }
                    }
                    None => w.sounds.push(Ui::Error as u32),
                }
            }
            _ => {}
        }
        return;
    }
    // merit2ca.
    let cursor = u.cursor;
    match key {
        // The row list's page (FUN_101f6c70 3 / 4 with +0x47 set): the top by a page, the screen row kept.
        K::ArrowLeft | K::ArrowRight => {
            let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            if let Some(u) = w.jp_ui.as_mut()
                && super::scrollbar::rows_abs(m, &mut u.top, &mut u.cursor, jobs, JOB_ROWS)
            {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowUp | K::ArrowDown => {
            let to = if key == K::ArrowDown { (cursor + 1).min(jobs.saturating_sub(1)) } else { cursor.saturating_sub(1) };
            if to != cursor && let Some(u) = w.jp_ui.as_mut() {
                scroll(&mut u.cursor, &mut u.top, to, JOB_ROWS);
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.jp_ui = None;
        }
        K::Enter => {
            let Some(&(job, _)) = t.jobs.get(cursor) else { return };
            if job_level(w, job) < 99 {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            w.sounds.push(Ui::Select as u32);
            let Some(index) = w.index.clone() else { return };
            let gifts = Gifts::load(&index, job, u.gift_names);
            if let Some(u) = w.jp_ui.as_mut() {
                u.page = Some(Page { job, page: 0, cursor: [0; 2], top: [0; 2], gifts });
            }
        }
        _ => {}
    }
}

/// The focused list's bar (FUN_101f6c70).
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(t) = w.jp_tables.clone() else { return false };
    let Some(u) = w.jp_ui.as_mut() else { return false };
    match u.page.as_mut() {
        Some(p) => {
            let (pg, n) = (p.page, page_len(p, &t));
            super::scrollbar::rows_abs(m, &mut p.top[pg], &mut p.cursor[pg], n, PAGE_ROWS)
        }
        None => super::scrollbar::rows_abs(m, &mut u.top, &mut u.cursor, t.jobs.len(), JOB_ROWS),
    }
}

/// The mouse on the focused list (the generic window's rows): hover moves the cursor, a click on it is Enter, a right
/// click over the window Escape.
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(t) = w.jp_tables.clone() else { return };
    let Some(u) = w.jp_ui.as_ref() else { return };
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k, false);
    match &u.page {
        Some(pg) => {
            let q = pg.page;
            let (top, cursor) = (pg.top[q], pg.cursor[q]);
            super::winhits::generic(w, gu, PAGE, (cursor - top + 1) as u16, 1..=PAGE_ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(pg) = w.jp_ui.as_mut().and_then(|u| u.page.as_mut()) {
                    pg.cursor[q] = top + id as usize - 1;
                }
            }, keys);
        }
        None if !t.jobs.is_empty() => {
            let (top, cursor) = (u.top, u.cursor);
            super::winhits::generic(w, gu, JOBS, (cursor - top + 1) as u16, 1..=JOB_ROWS as u16, p, moved, clicked, right, |w, id| {
                if let Some(u) = w.jp_ui.as_mut() {
                    u.cursor = top + id as usize - 1;
                }
            }, keys);
        }
        None => {}
    }
}

/// Help slot 1 0xe 0x320 "Select the job for which you wish to expend job points.", title "Categories".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if w.jp_ui.is_none() {
        return;
    }
    bars.help[1] = gu.menu_msg(0x320, "");
    bars.help[2] = String::new();
    bars.title[2] = gu.title_msg(0x82).unwrap_or_default();
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(u) = &w.jp_ui else { return rects };
    let Some(t) = w.jp_tables.clone() else { return rects };
    let free = w.confirm.is_none();
    let right = |gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32| gu.shape_text(s, x, y, tint, true);
    // The job: jbpcat's, else merit2ca's cursor row.
    let job = u.page.as_ref().map(|p| p.job).or_else(|| t.jobs.get(u.cursor).map(|j| j.0)).unwrap_or(0);
    // levmeri2 (FUN_1014a040): capacity / 30000 at (x1 - 8, 0x1c), job points / 500 at (x1 - 8, 0x44).
    if let Some(win) = gu.lobby_window_off(LEV, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, fw, _] = win.frame;
        if let Some([cap, cur, _]) = points(w, job).filter(|_| (1..32).contains(&job)) {
            right(gu, &format!("{cap}/30000"), x + fw - 8.0, y + 28.0, WHITE);
            right(gu, &format!("{cur}/500 P"), x + fw - 8.0, y + 68.0, if cur >= 500 { BLUE } else { WHITE });
        }
        rects.push(win.frame);
    }
    let (name, desc);
    if let Some(p) = &u.page {
        let pg = p.page;
        let n = page_len(p, &t);
        let (top, cursor) = (p.top[pg], p.cursor[pg]);
        let hide: Vec<u16> = (1..=PAGE_ROWS as u16).filter(|&k| top + k as usize > n).collect();
        let sel = (n > 0).then(|| (cursor - top + 1) as u16);
        let Some(win) = gu.lobby_window_off(PAGE, sw, sh, (0.0, 0.0), sel, free, &hide) else { return rects };
        super::winhits::record(w, PAGE, &win);
        let [x0, y0, fw, fh] = win.frame;
        gu.sprite("menu    windowps", 722 + pg, x0, y0, WHITE);
        if pg == 1 {
            let spent = points(w, p.job).map_or(0, |q| q[2]);
            gu.shape_text(&spent.to_string(), x0 + 258.0, y0 + 4.0, WHITE, false);
        }
        let gifts = gift_rows(&p.gifts);
        for i in top..(top + PAGE_ROWS).min(n) {
            let y = y0 + 17.0 + 17.0 * (i - top) as f32;
            let on = i == cursor;
            if pg == 0 {
                let e = t.entries(p.job)[i];
                let (c, lc, cost) = colours(w, &t, p.job, e);
                let (level, next) = w.jp_list.get(&e).copied().unwrap_or((0, 0));
                gu.text(&t.name(e), x0 + COLS[1], y + 2.0, 0, if on { CURSOR } else { c }.to_le_bytes());
                right(gu, &level.to_string(), x0 + COLS[2], y + 4.0, lc);
                gu.shape_text(&format!("/{}", t.max.get(e as usize).copied().unwrap_or(0)), x0 + COLS[3], y + 4.0, lc, false);
                if let Some(cc) = cost {
                    gu.shape_text(&gu.lobby_msg(0xc, 0x5a, "NEXT"), x0 + COLS[4], y + 4.0, cc, false);
                    right(gu, &format!("{next} P"), x0 + COLS[5], y + 4.0, cc);
                }
            } else {
                let g = gifts[i];
                let need = p.gifts.needs[g];
                let have = points(w, p.job).map(|q| q[2]);
                let c = if have.is_some_and(|h| h >= need) { WHITE } else { GREY };
                gu.text(&p.gifts.name(g), x0 + COLS[1], y + 2.0, 0, if on { CURSOR } else { c }.to_le_bytes());
                right(gu, &need.to_string(), x0 + COLS[4] + 52.0, y + 4.0, c);
            }
        }
        if n > PAGE_ROWS {
            gu.scroll_bar(x0 + fw - 8.0, y0, fh, n, PAGE_ROWS, top);
            if free {
                gu.bar_site(super::scrollbar::Owner::Points, x0 + fw - 8.0, y0, fh, n, PAGE_ROWS, top);
            }
        }
        rects.push(win.frame);
        (name, desc) = if pg == 0 {
            t.entries(p.job).get(cursor).map_or((String::new(), String::new()), |&e| (t.name(e), t.desc(e)))
        } else {
            gifts.get(cursor).map_or((String::new(), String::new()), |&g| (p.gifts.name(g), p.gifts.desc(g)))
        };
    } else {
        let hide: Vec<u16> = (1..=JOB_ROWS as u16).filter(|&k| u.top + k as usize > t.jobs.len()).collect();
        let sel = (!t.jobs.is_empty()).then(|| (u.cursor - u.top + 1) as u16);
        let Some(win) = gu.lobby_window_off(JOBS, sw, sh, (0.0, 0.0), sel, free, &hide) else { return rects };
        super::winhits::record(w, JOBS, &win);
        let [x0, y0, fw, fh] = win.frame;
        for (i, &(job, _)) in t.jobs.iter().enumerate().skip(u.top).take(JOB_ROWS) {
            let y = y0 + 4.0 + 16.0 * (i - u.top) as f32;
            let c = if i == u.cursor { CURSOR } else if job_level(w, job) >= 99 { WHITE } else { GREY };
            gu.text(&t.name(job as u16), x0 + COLS[1], y + 2.0, 0, c.to_le_bytes());
        }
        if t.jobs.len() > JOB_ROWS {
            gu.scroll_bar(x0 + fw - 8.0, y0, fh, t.jobs.len(), JOB_ROWS, u.top);
            if free {
                gu.bar_site(super::scrollbar::Owner::Points, x0 + fw - 8.0, y0, fh, t.jobs.len(), JOB_ROWS, u.top);
            }
        }
        rects.push(win.frame);
        (name, desc) = (t.name(job as u16), t.desc(job as u16));
    }
    rects.extend(bluehelp(gu, sw, sh, &name, &desc));
    rects
}
