//! The social page's Quests row (/quest, 0x4d; quest_mission_spec section 6): "menu    quest00" (Current, Completed,
//! the Records of Eminence rows and panel) and "menu    quest01" (one area's quests, over quest00).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const CHOOSER: &str = "menu    quest00";
pub const LIST: &str = "menu    quest01";
const VISIBLE: usize = 8;
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;
const GREY: u32 = 0x8040_4040;
/// FUN_1022f670: the chooser's rows moved to these y (window-relative; child 1 35 high).
const ROW_Y: [f32; 5] = [14.0, 46.0, 110.0, 142.0, 174.0];
/// Titles (group 0xd) by area: current, completed (FUN_10231c60).
const TITLES: [[usize; 11]; 2] = [[0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x88, 0x91, 0x9c, 0xa0, 0xa3], [0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x89, 0x92, 0x9d, 0xa1, 0xa4]];

/// Records of Eminence (s2c 0x110 / 0x111 / 0x112; DAT_104804d0.., flags DAT_104807ec).
#[derive(Clone, Debug)]
pub struct Roe {
    pub sparks: u32,
    pub deeds: u16,
    pub plaudits: u16,
    /// 0x111: 64 records {Id:12, Count:20}.
    pub active: [u32; 64],
    /// 0x112: Data[128] at Offset * 0x80 (0..3).
    pub log: [u8; 512],
    /// Bit 0 a 0x110, 1 a 0x111, 2 the log complete (Offset 3; Offset 0 clears it).
    pub flags: u8,
}

impl Default for Roe {
    fn default() -> Self {
        Roe { sparks: 0, deeds: 0, plaudits: 0, active: [0; 64], log: [0; 512], flags: 0 }
    }
}

impl Roe {
    pub fn log_part(&mut self, offset: u16, data: &[u8; 128]) {
        let Some(o) = (offset < 4).then_some(offset as usize * 0x80) else { return };
        self.log[o..o + 0x80].copy_from_slice(data);
        match offset {
            0 => self.flags &= !4,
            3 => self.flags |= 4,
            _ => {}
        }
    }

    /// Row 2's count (FUN_10230ad0 + records 63 and 42): the objectives taken, over 30.
    fn taken(&self) -> u32 {
        if self.flags & 2 == 0 {
            return 0;
        }
        let n = self.active[..30].iter().filter(|r| *r & 0xfff != 0).count() as u32;
        n + (self.active[63] & 0xfff != 0) as u32 + (self.active[42] & 0xfff != 0) as u32
    }

    /// FUN_10230b00: the set bits of the log's first 0x173 bytes.
    fn completed(&self) -> u32 {
        if self.flags & 4 == 0 { 0 } else { self.log[..0x173].iter().map(|b| b.count_ones()).sum() }
    }

    /// Vana'Bout (child 5) is open while a 0x111 came and one of records 43..62 is set.
    fn vanabout(&self) -> bool {
        self.flags & 2 != 0 && self.active[43..63].iter().any(|r| r & 0xfff != 0)
    }
}

#[derive(Clone, Debug)]
pub struct Quests {
    /// quest00: 0 waiting for the store, 1 the chooser; its cursor row.
    pub state: u8,
    pub sel: usize,
    pub list: Option<List>,
}

#[derive(Clone, Debug)]
pub struct List {
    /// 0 current, 1 completed.
    pub mode: usize,
    pub area: usize,
    /// 0 the list, 1 a description.
    pub state: u8,
    /// Entry indices in file order.
    pub rows: Vec<usize>,
    pub top: usize,
    pub cursor: usize,
}

/// /quest (FUN_1007d240): quest00 in mode 0; its setup asks for fresh stats (c2s 0x061 {0}).
pub fn open(w: &mut World) {
    super::missions::logs(w);
    let _ = w.net.commands.send(super::net::NetCommand::StatusRequest(0));
    let state = if w.journal.complete() { 1 } else { 0 };
    w.quests = Some(Quests { state, sel: 0, list: None });
}

/// Per frame: the chooser waits for the store (FUN_10230280).
pub fn update(w: &mut World) {
    let complete = w.journal.complete();
    if let Some(q) = w.quests.as_mut()
        && q.state == 0
        && complete
    {
        q.state = 1;
    }
}

pub fn busy(w: &World) -> bool {
    w.quests.as_ref().is_some_and(|q| q.state == 0)
}

/// FUN_102333f0: bit `n` of the area's current or completed record; numbers above 1000 are the Mog Garden quests,
/// 1039..1045 and 1047..1049 at bits 39..45 / 47..49 of area 9.
fn has(s: &super::journal::Store, area: usize, n: u32, done: bool) -> bool {
    let (area, n) = match n {
        0..=255 => (area, n as usize),
        1039..=1045 | 1047..=1049 => (9, n as usize - 1000),
        _ => return false,
    };
    let words = if done { &s.completed[area] } else { &s.current[area] };
    words[n / 32] >> (n % 32) & 1 != 0
}

/// FUN_10231ec0 / FUN_10231f50: current (and repeatable '+' or not completed) / completed, in file order.
fn rows(w: &World, area: usize, mode: usize) -> Vec<usize> {
    let Some(f) = w.logs.as_ref().and_then(|l| l.quests.get(area)) else { return Vec::new() };
    let s = &w.journal;
    f.entries
        .iter()
        .enumerate()
        .filter(|(_, e)| match mode {
            0 => has(s, area, e.number, false) && (e.name.starts_with('+') || !has(s, area, e.number, true)),
            _ => has(s, area, e.number, true),
        })
        .map(|(i, _)| i)
        .collect()
}

/// DAT_10386998: areas 5..10 need Zilart, ToAU, WotG, Abyssea (any of 8..10), Adoulin; 0..4 always.
fn area_on(enabled: u32, area: usize) -> bool {
    match area {
        5 => enabled & 2 != 0,
        6 => enabled & 8 != 0,
        7 => enabled & 0x10 != 0,
        8 => enabled & 0x700 != 0,
        9 | 10 => enabled & 0x800 != 0,
        _ => true,
    }
}

fn open_list(w: &mut World, mode: usize) {
    let area = (w.journal.nation as usize).min(2);
    let rows = rows(w, area, mode);
    if let Some(q) = w.quests.as_mut() {
        q.list = Some(List { mode, area, state: 0, rows, top: 0, cursor: 0 });
    }
}

fn show(l: &mut List) {
    if l.cursor < l.top {
        l.top = l.cursor;
    } else if l.cursor >= l.top + VISIBLE {
        l.top = l.cursor + 1 - VISIBLE;
    }
}

pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(l) = w.quests.as_mut().and_then(|q| q.list.as_mut()).filter(|l| l.state == 0) else { return false };
    let mut row = l.cursor.saturating_sub(l.top);
    let moved = super::scrollbar::paged(m, &mut l.top, &mut row, l.rows.len(), VISIBLE);
    l.cursor = l.top + row;
    moved
}

/// quest00 (FUN_102300c0) or quest01 (FUN_102330b0) keys.
pub fn key(w: &mut World, key: egui::Key, page: bool) {
    let Some(q) = w.quests.clone() else { return };
    if let Some(mut l) = q.list {
        list_key(w, &mut l, key, page);
        if let Some(qq) = w.quests.as_mut()
            && qq.list.is_some()
        {
            qq.list = Some(l);
        }
        return;
    }
    let total = if w.roe.vanabout() { 5 } else { 4 };
    match (q.state, key) {
        (_, egui::Key::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            w.quests = None;
        }
        (1, egui::Key::ArrowUp | egui::Key::ArrowDown) => {
            let sel = if key == egui::Key::ArrowDown { (q.sel + 1) % total } else { (q.sel + total - 1) % total };
            if let Some(qq) = w.quests.as_mut() {
                qq.sel = sel;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        (1, egui::Key::Enter) => {
            w.sounds.push(Ui::Select as u32);
            match q.sel {
                0 | 1 => open_list(w, q.sel),
                r => log::info!("quests: row {r} (Records of Eminence lists) is not implemented yet"),
            }
        }
        _ => {}
    }
}

fn list_key(w: &mut World, l: &mut List, key: egui::Key, page: bool) {
    let n = l.rows.len();
    match (l.state, key) {
        (0, egui::Key::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            if let Some(q) = w.quests.as_mut() {
                q.list = None;
            }
        }
        (0, egui::Key::ArrowUp | egui::Key::ArrowDown) if n > 0 => {
            l.cursor = if key == egui::Key::ArrowDown { (l.cursor + 1) % n } else { (l.cursor + n - 1) % n };
            show(l);
            w.sounds.push(Ui::Cursor as u32);
        }
        (0, egui::Key::ArrowLeft | egui::Key::ArrowRight) if page => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            let mut row = l.cursor.saturating_sub(l.top);
            if super::scrollbar::paged(m, &mut l.top, &mut row, n, VISIBLE) {
                l.cursor = l.top + row;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        (0, egui::Key::ArrowLeft | egui::Key::ArrowRight) => {
            let mut a = l.area;
            for _ in 0..11 {
                a = if key == egui::Key::ArrowRight { (a + 1) % 11 } else { (a + 10) % 11 };
                if area_on(w.expansions, a) {
                    break;
                }
            }
            (l.area, l.rows, l.top, l.cursor) = (a, rows(w, a, l.mode), 0, 0);
            w.sounds.push(Ui::Cursor as u32);
        }
        (0, egui::Key::Enter) => {
            w.sounds.push(Ui::Select as u32);
            if n > 0 {
                l.state = 1;
            }
        }
        (1, egui::Key::Enter | egui::Key::Escape) => {
            w.sounds.push(if key == egui::Key::Enter { Ui::Select } else { Ui::Cancel } as u32);
            l.state = 0;
        }
        _ => {}
    }
}

/// Mouse: rows, the cursor row's click as Enter, the arrows (quest01 children 9 / 10) as Left / Right.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(q) = w.quests.clone() else { return };
    let hit = |name: &str| -> Option<u16> {
        let (x, y) = gu.window_xy(name, sw, sh)?;
        let lay = gu.layout(name)?;
        lay.elements.iter().skip(1).find(|e| {
            let (ex, ey) = (x + e.x as f32, y + e.y as f32);
            p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
        }).map(|e| e.id)
    };
    if let Some(l) = &q.list {
        let Some(c) = hit(LIST) else { return };
        gu.hover.set(true);
        match c {
            9 | 10 if clicked && l.state == 0 => key(w, if c == 9 { egui::Key::ArrowLeft } else { egui::Key::ArrowRight }, false),
            1..=8 if l.state == 0 => {
                let row = l.top + c as usize - 1;
                if row >= l.rows.len() {
                    return;
                }
                if row != l.cursor && (moved || clicked) {
                    if let Some(ll) = w.quests.as_mut().and_then(|q| q.list.as_mut()) {
                        ll.cursor = row;
                    }
                    w.sounds.push(Ui::Cursor as u32);
                } else if clicked {
                    key(w, egui::Key::Enter, false);
                }
            }
            _ if clicked && l.state == 1 => key(w, egui::Key::Enter, false),
            _ => {}
        }
        return;
    }
    if q.state != 1 {
        return;
    }
    let Some((x, y)) = gu.window_xy(CHOOSER, sw, sh) else { return };
    let total = if w.roe.vanabout() { 5 } else { 4 };
    let Some(r) = (0..total).find(|&k| {
        let (ex, ey) = (x + 39.0, y + ROW_Y[k]);
        p.0 >= ex && p.0 < ex + 320.0 && p.1 >= ey && p.1 < ey + if k == 0 { 35.0 } else { 18.0 }
    }) else { return };
    gu.hover.set(true);
    if r != q.sel && (moved || clicked) {
        w.quests.as_mut().unwrap().sel = r;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter, false);
    }
}

/// The frame's title "Quests" (group 0xd 0x64); no help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if w.quests.is_some() {
        (bars.help[1], bars.title[1]) = (String::new(), gu.focus_text(CHOOSER, 0)[1].clone().unwrap_or_default());
    }
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// A number right-aligned at x, blue at its 99999 cap.
fn value(gu: &mut GameUi, v: u32, x: f32, y: f32) {
    let s = v.to_string();
    let wd = gu.text_width(&s);
    text(gu, &s, x - wd, y, if v >= 99999 { 0x8040_7090 } else { WHITE });
}

/// quest00 (FUN_102300a0): the rows (framesus 35 at (4, 5), the text at (16, 0)) and the RoE panel (FUN_10233560); with
/// quest01 over it only its frame. quest01 (FUN_10232720): the area's title at (60, 10), the quests, the empty line,
/// the description box, the scroll bar.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(q) = &w.quests else { return rects };
    let all: Vec<u16> = (1..=5).collect();
    let focused = q.list.is_none() && w.confirm.is_none();
    if let Some(win) = gu.lobby_window_off(CHOOSER, sw, sh, (0.0, 0.0), None, false, &all) {
        rects.push(win.frame);
        let Some((x, y)) = gu.window_xy(CHOOSER, sw, sh) else { return rects };
        let bt = w.battle.clone();
        if q.state == 0 {
            if let Some(bt) = &bt {
                text(gu, &bt.group9(0x61).unwrap_or_default(), x + 20.0, y + 20.0, WHITE);
                text(gu, &bt.group9(0x62).unwrap_or_default(), x + 20.0, y + 40.0, WHITE);
            }
        } else if q.list.is_none() {
            let roe = &w.roe;
            let van = roe.vanabout();
            let labels = [
                gu.lobby_msg(0xc, 0x3b, ""),
                gu.lobby_msg(0xc, 0x3c, ""),
                format!("{}          {}/30", gu.lobby_msg(0xc, 0x6d, ""), roe.taken()),
                gu.lobby_msg(0xc, 0x6e, ""),
                gu.lobby_msg(0xc, 0x208, ""),
            ];
            for (k, label) in labels.iter().enumerate() {
                let (cx, cy) = (x + 39.0, y + ROW_Y[k]);
                let on = k == q.sel;
                gu.sprite("menu    quest", 0, cx - on as u8 as f32, cy - on as u8 as f32, if on { 0x8080_5020 } else { WHITE });
                gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
                let tint = if k == 4 && !van { GREY } else if on { CURSOR } else { WHITE };
                text(gu, label, cx + 16.0, cy, tint);
                if on && focused {
                    gu.cursor(cx, cy);
                }
            }
            // FUN_10233560: the Records of Eminence panel.
            let accolades = if w.recasts_seen { w.stats.as_ref().map_or(0, |s| s.unity >> 10 & 0x1ffff) } else { 0 };
            text(gu, &gu.lobby_msg(0xc, 0x6c, ""), x + 20.0, y + 78.0, WHITE);
            for (label, v, lx, vx, ly) in [
                (0x78, if roe.flags & 1 != 0 { roe.sparks & 0xff_ffff } else { 0 }, 20.0, 160.0, 200.0),
                (0x7a, if roe.flags & 1 != 0 { roe.deeds as u32 } else { 0 }, 20.0, 160.0, 220.0),
                (0x79, accolades, 168.0, 350.0, 200.0),
                (0x82, roe.completed(), 168.0, 350.0, 220.0),
                (0x209, roe.plaudits as u32, 180.0, 350.0, 174.0),
            ] {
                text(gu, &gu.lobby_msg(0xc, label, ""), x + lx, y + ly, WHITE);
                value(gu, v, x + vx, y + ly);
            }
        }
    }
    let Some(l) = &q.list else { return rects };
    let Some(f) = w.logs.as_ref().and_then(|g| g.quests.get(l.area)) else { return rects };
    let shown: Vec<u16> = if l.state == 0 { (1..=VISIBLE as u16).filter(|&c| l.top + (c as usize) - 1 < l.rows.len()).collect() } else { Vec::new() };
    let hide: Vec<u16> = (1..=VISIBLE as u16).filter(|c| !shown.contains(c)).collect();
    let sel = (l.state == 0 && !l.rows.is_empty()).then(|| (l.cursor - l.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, w.confirm.is_none(), &hide) else { return rects };
    let [fx, fy, fw, fh] = win.frame;
    let Some((x, y)) = gu.window_xy(LIST, sw, sh) else { return rects };
    rects.push(win.frame);
    if l.state == 0 {
        if let Some(t) = gu.title_msg(TITLES[l.mode.min(1)][l.area]) {
            text(gu, &t, x + 60.0, y + 10.0, WHITE);
        }
        if l.rows.is_empty() {
            if let Some(bt) = &w.battle {
                text(gu, &bt.group9(if l.mode == 0 { 0x63 } else { 0x64 }).unwrap_or_default(), x + 20.0, y + 40.0, WHITE);
            }
        }
        for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=VISIBLE as u16).contains(id)) {
            let row = l.top + id as usize - 1;
            let Some(e) = l.rows.get(row).and_then(|&i| f.entries.get(i)) else { continue };
            gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
            super::missions::lines(gu, e.name.trim_start_matches('+'), cx + 18.0, cy, 301.0, if row == l.cursor { CURSOR } else { WHITE });
        }
        let bx = fx + fw - 8.0;
        gu.scroll_bar(bx, fy, fh, l.rows.len(), VISIBLE, l.top);
        gu.bar_site(super::scrollbar::Owner::Journal, bx, fy, fh, l.rows.len(), VISIBLE, l.top);
    } else if let Some(e) = l.rows.get(l.cursor).and_then(|&i| f.entries.get(i)) {
        gu.sprite("menu    framesus", 35, x + 36.0, y + 25.0, WHITE);
        text(gu, e.name.trim_start_matches('+'), x + 50.0, y + 20.0, WHITE);
        super::missions::lines(gu, &e.desc, x + 40.0, y + 50.0, 327.0, WHITE);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_and_counts() {
        let mut s = super::super::journal::Store::default();
        s.current[4][1] = 1 << 3;
        s.completed[9][1] = 1 << 7;
        assert!(has(&s, 4, 35, false) && !has(&s, 4, 35, true));
        assert!(has(&s, 4, 1039, true) && !has(&s, 4, 1046, true));
        let mut r = Roe::default();
        r.active[0] = 5;
        r.active[63] = 1;
        assert_eq!(r.taken(), 0);
        r.flags = 2;
        assert_eq!((r.taken(), r.vanabout()), (2, false));
        r.active[50] = 9;
        assert!(r.vanabout());
        // Part 3 is past the counted 0x173 bytes but completes the log; part 2 brings 0x100..0x173.
        r.log_part(3, &[0xff; 128]);
        assert_eq!(r.completed(), 0);
        r.log_part(2, &[0xff; 128]);
        assert_eq!(r.completed(), 0x73 * 8);
        r.log_part(0, &[0; 128]);
        assert_eq!(r.completed(), 0);
    }
}
