//! Social row 8 Help Desk and /helpdesk (spec ~/ffxi/lab/research/social_rows_spec.md part H): "menu    faqsub"
//! (vtable 0x1033bcf8; the state machine FUN_10212760 / FUN_10212aa0) with its menu page (FUN_102133c0 / FUN_102136d0)
//! and Config page (FUN_102157c0 / FUN_10215a50). Q&A runs in PlayOnline's knowledge base, which we do not have: it
//! stays grey and refuses, as retail does when its files fail to load. The GM call: client/faqmain.rs. Not done: the
//! Adventuring Primer (guide00).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const WIN: &str = "menu    faqsub";
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
/// FUN_10216a50(i): group 9 ids.
const TEXT: [usize; 26] = [0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0x12b, 0xd5, 0xd6, 0xd8, 0xd7, 0xd9, 0xda, 0xdb, 0xdc, 0x12c, 0xdd];
/// The menu page's rows: Q&A, Adventuring Primer, Mentor Search, Help Desk, Config, Back.
const MENU: [usize; 6] = [0, 15, 12, 2, 3, 4];
/// Their help (FUN_10216970), none on Back.
const MENU_HELP: [Option<usize>; 6] = [Some(16), Some(24), Some(18), Some(19), Some(20), None];
/// The Config page's entries: Enable / Disable Mentor Status., Cancel New Adventurer Status., the terms (not a row), Back.
const CONFIG: [usize; 5] = [9, 10, 11, 14, 4];
/// Padding inside the window rect (content +0x166 x, +0x16a y, +0x168 right, +0x16c bottom), the rows' indent and
/// the gap between entries (+0x170, +0x174).
const PAD: (f32, f32, f32, f32) = (8.0, 10.0, 28.0, 20.0);
const INDENT: f32 = 20.0;
const GAP: f32 = 10.0;

pub struct HelpDesk {
    config: bool,
    /// +0x128 / +0x154: the menu / Config page cursor (an entry index).
    cursor: usize,
    /// +0x17a: the page's scroll in pixels.
    scroll: f32,
    /// faqmain (the GM call) over it: faqsub is suspended while it is open.
    pub(super) main: Option<super::faqmain::FaqMain>,
}

/// FUN_102174c0 (and /helpdesk when faqsub is not open): faqsub at its menu page; the social page stays beside it.
pub fn open(w: &mut World) {
    if w.helpdesk.is_none() {
        w.helpdesk = Some(HelpDesk { config: false, cursor: 0, scroll: 0.0, main: None });
    }
}

fn text(w: &World, i: usize) -> String {
    w.battle.as_ref().and_then(|b| b.group9(TEXT[i])).unwrap_or_default()
}

/// FUN_100e47c0 on the s2c 0x0B4 bytes @7: 7 mentor unlocked (bit 0), 8 mentor on (bit 1), 9 new adventurer off (bit 2).
fn flag(w: &World, which: u8) -> bool {
    w.conf_sys[3] >> (which - 7) & 1 != 0
}

/// Config rows enabled (FUN_10215a50): Enable with the mentor role unlocked and off, Disable unlocked and on, Cancel New
/// Adventurer while it is not cancelled yet; Back always.
fn config_on(w: &World, row: usize) -> bool {
    match row {
        0 => flag(w, 7) && !flag(w, 8),
        1 => flag(w, 7) && flag(w, 8),
        2 => !flag(w, 9),
        _ => true,
    }
}

/// The selectable entries of the page (the terms are not one).
fn rows(config: bool) -> Vec<usize> {
    if config { vec![0, 1, 2, 4] } else { (0..6).collect() }
}

/// FUN_10216500: a search of every area for mentors (status flag 1), its results over faqsub.
fn mentor_search(w: &mut World) {
    use crate::net::search::{AREA, Cond, STATUS};
    let cond = Cond { flags: AREA | STATUS, last_area: -1, status: 1, ..Default::default() };
    super::search::run(w, cond, super::search::Scope::World);
}

/// FUN_10212aa0: Up / Down over the rows (no wrap, SE 0001); Enter per row; Escape: SE 0003, the Config page back to
/// the menu page, the menu page closes faqsub.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(h) = w.helpdesk.as_ref() else { return };
    if h.main.is_some() {
        super::faqmain::key(w, key);
        return;
    }
    let (config, cursor) = (h.config, h.cursor);
    let r = rows(config);
    let at = r.iter().position(|&c| c == cursor).unwrap_or(0);
    match key {
        K::ArrowUp | K::ArrowDown => {
            let to = if key == K::ArrowDown { (at + 1).min(r.len() - 1) } else { at.saturating_sub(1) };
            if to != at
                && let Some(h) = w.helpdesk.as_mut()
            {
                h.cursor = r[to];
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            back(w, config);
        }
        K::Enter if config => match cursor {
            4 => {
                w.sounds.push(Ui::Cancel as u32);
                back(w, true);
            }
            c if !config_on(w, c) => w.sounds.push(Ui::Error as u32),
            c => {
                w.sounds.push(Ui::Select as u32);
                match c {
                    0 => super::textcmd::config_flag(w, 8, true),
                    1 => super::textcmd::config_flag(w, 8, false),
                    // Group 9 0xd4 with the cursor on No; Yes cancels the status for good.
                    _ => w.confirm = Some((super::Confirm::NewAdventurer, false)),
                }
            }
        },
        K::Enter => match cursor {
            5 => {
                w.sounds.push(Ui::Cancel as u32);
                w.helpdesk = None;
            }
            // Q&A: the knowledge-base files never loaded (+0x1c2 != 1).
            0 => w.sounds.push(Ui::Error as u32),
            2 => {
                w.sounds.push(Ui::Select as u32);
                mentor_search(w);
            }
            4 => {
                w.sounds.push(Ui::Select as u32);
                if let Some(h) = w.helpdesk.as_mut() {
                    (h.config, h.cursor, h.scroll) = (true, 0, 0.0);
                }
            }
            3 => {
                w.sounds.push(Ui::Select as u32);
                let main = super::faqmain::open(w);
                if let Some(h) = w.helpdesk.as_mut() {
                    h.main = main;
                }
            }
            c => {
                w.sounds.push(Ui::Select as u32);
                log::info!("help desk: row {c} (Adventuring Primer) is not implemented yet");
            }
        },
        _ => {}
    }
}

/// The mouse: faqsub's rows as the generic window's children (winhits.rs); faqmain only the right click.
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(h) = w.helpdesk.as_ref() else { return };
    if h.main.is_some() {
        if right && super::winhits::over(w, "menu    faqmain", p) {
            key(w, egui::Key::Escape);
        }
        return;
    }
    super::winhits::generic(w, gu, WIN, h.cursor as u16 + 1, 1..=6, p, moved, clicked, right, |w, id| {
        if let Some(h) = w.helpdesk.as_mut() {
            h.cursor = id as usize - 1;
        }
    }, |w, _, k| key(w, k));
}

/// Escape: the Config page goes back to the menu page (state 1), the menu page closes faqsub.
fn back(w: &mut World, config: bool) {
    match w.helpdesk.as_mut() {
        Some(h) if config => (h.config, h.cursor, h.scroll) = (false, 0, 0.0),
        _ => w.helpdesk = None,
    }
}

/// The comyn answer for Cancel New Adventurer Status (LAB_10215c00): Yes sends c2s 0x0DC which 9 and plays SE 0002.
pub fn new_adventurer_off(w: &mut World) {
    super::textcmd::config_flag(w, 9, true);
    w.sounds.push(Ui::Select as u32);
}

/// faqsub keeps the keys unless a prompt or the search results are up over it.
pub fn focused(w: &World) -> bool {
    w.helpdesk.is_some() && w.confirm.is_none() && w.search.is_none()
}

/// Title slot 2 group 9 0xdd "Help Desk"; help slot 2 the cursor row's (none on Back and on the Config page's terms).
pub fn bars(_gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(h) = &w.helpdesk else { return };
    // faqsub's children carry no help strings: slot 1 stays empty while it has the focus.
    if focused(w) {
        bars.help[1] = String::new();
    }
    bars.title[2] = text(w, 25);
    // faqmain: its title (group 0xd 0x3c), no row help (the script's HELP lines are empty).
    if h.main.is_some() {
        bars.title[1] = _gu.title_msg(0x3c).unwrap_or_default();
        bars.help[2] = String::new();
        return;
    }
    let help = if h.config { [Some(21), Some(22), Some(23), None, None][h.cursor] } else { MENU_HELP[h.cursor] };
    bars.help[2] = help.map_or(String::new(), |i| text(w, i));
}

/// One entry: its text wrapped and where it went.
struct Entry {
    x: f32,
    y: f32,
    lines: Vec<String>,
    tint: u32,
    row: Option<usize>,
}

/// FUN_102136d0 / FUN_10215a50: the entries laid out from the content origin (scroll not applied); the menu page's
/// header has no height, the Config page's "Config" header is drawn; the keyword box under Q&A (its label, then the
/// 14 px field at + 16) with Q&A grey.
fn layout(gu: &GameUi, w: &World, h: &HelpDesk, width: f32) -> Vec<Entry> {
    let mut out = Vec::new();
    let wrap = |s: &str, wd: f32| gu.wrap(s, wd);
    let mut y = 0.0;
    if h.config {
        let t = text(w, 3);
        let lines = wrap(&t, width);
        y += 16.0 * lines.len() as f32;
        out.push(Entry { x: 0.0, y: 0.0, lines, tint: WHITE, row: None });
    }
    let row_w = width - INDENT - GAP;
    let list: Vec<(usize, usize)> = if h.config { CONFIG.iter().copied().enumerate().collect() } else { MENU.iter().copied().enumerate().collect() };
    for (row, i) in list {
        y += GAP;
        let t = text(w, i);
        let lines = wrap(&t, row_w);
        let tint = if h.config { if config_on(w, row) { WHITE } else { GREY } } else if row == 0 { GREY } else { WHITE };
        let selectable = !(h.config && row == 3);
        out.push(Entry { x: INDENT, y, lines: lines.clone(), tint, row: selectable.then_some(row) });
        y += 16.0 * lines.len() as f32;
        if !h.config && row == 0 {
            y += GAP;
            let label = text(w, 1);
            out.push(Entry { x: INDENT * 2.0, y, lines: vec![label], tint: GREY, row: None });
            y += 16.0 + 14.0;
        }
    }
    out
}

/// The window frame, the entries clipped to the content rect, the markers at (x - 16, y): the hand on the cursor row,
/// framesus 35 at y + 4 on the other rows; the scroll keeps the cursor row inside; the bar at x1 - 8.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let h = w.helpdesk.as_ref()?;
    if h.main.is_some() {
        let r = super::faqmain::draw(gu, w, sw, sh);
        if let Some(frame) = r {
            super::winhits::record_rects(w, "menu    faqmain", frame, Vec::new());
        }
        return r;
    }
    let win = gu.lobby_window_off(WIN, sw, sh, (0.0, 0.0), None, false, &(1..=15).collect::<Vec<u16>>())?;
    let [fx, fy, fw, fh] = win.frame;
    let (px, py, pr, pb) = PAD;
    let (cx, cy, cw, ch) = (fx + px, fy + py, fw - pr - px, fh - 5.0 - py - pb);
    let entries = layout(gu, w, h, cw);
    let total = entries.last().map_or(0.0, |e| e.y + 16.0 * e.lines.len() as f32);
    // FUN_10213bd0: the cursor row inside the view.
    let mut scroll = h.scroll;
    if let Some(e) = entries.iter().find(|e| e.row == Some(h.cursor)) {
        let bottom = e.y + 16.0 * e.lines.len() as f32;
        if e.y - scroll < 0.0 {
            scroll = e.y;
        } else if bottom - scroll > ch {
            scroll = bottom - ch;
        }
    }
    gu.clip(Some([cx - 16.0, cy, cw + 16.0, ch]));
    let focused = focused(w);
    // The rows are code-drawn entries: each row's lines from its marker, inside the view.
    let rows = entries
        .iter()
        .filter_map(|e| {
            let (x, y) = (cx + e.x - 16.0, cy + e.y - scroll);
            let (top, bottom) = (y.max(cy), (y + 16.0 * e.lines.len() as f32).min(cy + ch));
            let row = e.row.filter(|_| bottom > top)?;
            Some((row as u16 + 1, [x, top, cx + cw - x, bottom - top]))
        })
        .collect();
    super::winhits::record_rects(w, WIN, win.frame, rows);
    for e in &entries {
        let (x, y) = (cx + e.x, cy + e.y - scroll);
        for (k, l) in e.lines.iter().enumerate() {
            gu.text(l, x, y + 16.0 * k as f32, 0, e.tint.to_le_bytes());
        }
        match e.row {
            Some(r) if r == h.cursor && focused => gu.cursor(x - 16.0, y),
            Some(_) => _ = gu.sprite("menu    framesus", 35, x - 16.0, y + 4.0, WHITE),
            None => {}
        }
    }
    gu.clip(None);
    gu.scroll_bar(fx + fw - 8.0, fy, fh, total as usize, ch as usize, scroll as usize);
    Some(win.frame)
}

/// The scroll the draw settled on is kept so the view does not jump back (FUN_10213bd0 writes +0x17a).
pub fn keep_scroll(w: &mut World, gu: &GameUi, sw: f32, sh: f32) {
    let Some(h) = w.helpdesk.as_ref() else { return };
    if h.main.is_some() {
        super::faqmain::keep_scroll(w, gu, sw, sh);
        return;
    }
    let Some([_, _, fw, fh]) = gu.window_rect(WIN, sw, sh) else { return };
    let (px, py, pr, pb) = PAD;
    let (cw, ch) = (fw - pr - px, fh - 5.0 - py - pb);
    let entries = layout(gu, w, h, cw);
    let Some(e) = entries.iter().find(|e| e.row == Some(h.cursor)) else { return };
    let bottom = e.y + 16.0 * e.lines.len() as f32;
    let mut scroll = h.scroll;
    if e.y - scroll < 0.0 {
        scroll = e.y;
    } else if bottom - scroll > ch {
        scroll = bottom - ch;
    }
    if let Some(h) = w.helpdesk.as_mut() {
        h.scroll = scroll;
    }
}
