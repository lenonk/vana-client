//! statcom2 row 11 Unity (spec ~/ffxi/lab/research/status_rows_spec.md 4): "menu    ut_menu" with "menu    ut_point",
//! quest00 in modes 1 (Unity Information) / 2 (Ranking Information) and quest01 in modes 5 / 6 (the previous / current
//! week's ranking), from s2c 0x063 type 7 and 0x061 @0x58 / @0x5c.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const MENU: &str = "menu    ut_menu";
const POINT: &str = "menu    ut_point";
const CHOOSER: &str = "menu    quest00";
const LIST: &str = "menu    quest01";
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;
const VISIBLE: usize = 8;

/// The two weeks' tables (0x10483164 + week * 0x1000: 32 blocks of 0x80 bytes by dataType) and their completeness
/// (0x10483160[w]: dataType 0x1f came).
pub struct Tables {
    data: Vec<[u8; 0x80]>,
    pub complete: [bool; 2],
}

impl Default for Tables {
    fn default() -> Self {
        Self { data: vec![[0; 0x80]; 64], complete: [false; 2] }
    }
}

impl Tables {
    /// s2c 0x063 type 7: week @8, dataType @9, the 0x80 bytes from @0xc.
    pub fn store(&mut self, week: u8, d: u8, bytes: &[u8; 0x80]) {
        let (w, d) = (week as usize & 1, d as usize & 0x1f);
        self.data[w * 32 + d] = *bytes;
        if d == 0x1f {
            self.complete[w] = true;
        }
    }

    /// FUN_100fddf0: dword `u` of block `k`.
    fn value(&self, week: usize, k: usize, u: usize) -> u32 {
        let b = &self.data[week * 32 + k];
        u32::from_le_bytes(b[4 * u..4 * u + 4].try_into().unwrap())
    }

    /// FUN_100fde80: {unity 0..11, points / members (0 without members, unity 0 never)}, sorted by the average down and
    /// the id up (LAB_100fde10) unless every average is 0 (FUN_100fde50).
    pub fn ranking(&self, week: usize) -> Vec<(u8, u32)> {
        let mut r: Vec<(u8, u32)> = (0..12)
            .map(|u| {
                let members = if u == 0 { 0 } else { self.value(week, 1, u) };
                let avg = if members == 0 { 0 } else { (self.value(week, 2, u) as f64 / members as f64) as u32 };
                (u as u8, avg)
            })
            .collect();
        if r.iter().any(|e| e.1 != 0) {
            r.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        }
        r
    }
}

/// FUN_100fdf20: unity `u`'s rank, ties sharing it; 0 while every average is 0.
fn rank(r: &[(u8, u32)], u: u8) -> u8 {
    if u == 0 || u > 11 || r.iter().all(|e| e.1 == 0) {
        return 0;
    }
    let mut k = 1;
    for (i, e) in r.iter().enumerate() {
        if i > 0 && e.1 != r[i - 1].1 {
            k = i as u8 + 1;
        }
        if e.0 == u {
            return k;
        }
    }
    0
}

/// FUN_100fdf80: unity `u`'s average.
fn average(r: &[(u8, u32)], u: u8) -> u32 {
    r.iter().find(|e| e.0 == u).map_or(0, |e| e.1)
}

pub struct Unity {
    sel: u16,
    info: Option<Info>,
}

/// quest00 in mode 1 / 2 and quest01 over it.
struct Info {
    mode: u8,
    sel: u16,
    /// +0x6e / +0x6d: the Unity chat setting chosen and the one last sent; +0x6f the hold before sending.
    chat: bool,
    sent: bool,
    hold: f32,
    ranks: Option<Ranks>,
}

/// quest01 mode 5 (week 0) / 6 (week 1).
struct Ranks {
    week: usize,
    top: usize,
    cursor: usize,
}

/// FUN_1014c260: c2s 0x116 {u32 week} for each complete week, which is then downloaded again (FUN_100996a0); ut_menu
/// and ut_point open.
pub fn open(w: &mut World) {
    for week in 0..2u32 {
        if w.unity_tables.complete[week as usize] {
            w.unity_tables.complete[week as usize] = false;
            let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x116, body: week.to_le_bytes().to_vec() });
        }
    }
    w.unity = Some(Unity { sel: 1, info: None });
}

/// Stats from s2c 0x119 on (DAT_10485824 bit 0) and both weeks complete.
fn ready(w: &World) -> bool {
    w.recasts_seen && w.stats.is_some() && w.unity_tables.complete == [true, true]
}

fn unity_of(w: &World) -> u8 {
    w.stats.as_ref().map_or(0, |s| (s.unity & 0x1f) as u8)
}

/// Per frame (FUN_102302b0): the chat setting goes out (c2s 0x118 {u8}) once its hold of 30 is over.
pub fn update(w: &mut World, ticks: f32) {
    let Some(i) = w.unity.as_mut().and_then(|u| u.info.as_mut()).filter(|i| i.mode == 1) else { return };
    i.hold -= ticks;
    if i.hold <= 0.0 {
        i.hold = 30.0;
        if i.chat != i.sent {
            i.sent = i.chat;
            let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x118, body: vec![i.chat as u8, 0, 0, 0] });
        }
    }
}

/// quest00's close (FUN_10230370): config 0xb2 takes the chat setting.
fn close_info(w: &mut World) {
    if let Some(i) = w.unity.as_mut().and_then(|u| u.info.take())
        && i.mode == 1
    {
        super::textcmd::cfg(w, 0xb2, i.chat as u32);
    }
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key, page: bool) {
    use egui::Key as K;
    let Some(u) = w.unity.as_ref() else { return };
    let (sel, mode) = (u.sel, u.info.as_ref().map(|i| i.mode));
    let week_ok = w.unity_tables.complete;
    // quest01's rows: unities 1..11 once the data is ready.
    let n = if ready(w) { 11 } else { 0 };
    let Some(u) = w.unity.as_mut() else { return };
    // quest01: Up / Down (wrap), Left / Right the other week when complete (with Ctrl / Alt a page), Escape back.
    if let Some(r) = u.info.as_mut().and_then(|i| i.ranks.as_mut()) {
        match key {
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                u.info.as_mut().unwrap().ranks = None;
            }
            K::ArrowUp | K::ArrowDown if n > 0 => {
                r.cursor = if key == K::ArrowDown { (r.cursor + 1) % n } else { (r.cursor + n - 1) % n };
                r.top = r.top.min(r.cursor).max((r.cursor + 1).saturating_sub(VISIBLE));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::ArrowLeft | K::ArrowRight if page && n > 0 => {
                let mut row = r.cursor - r.top;
                let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
                if super::scrollbar::paged(m, &mut r.top, &mut row, n, VISIBLE) {
                    r.cursor = r.top + row;
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::ArrowLeft | K::ArrowRight if week_ok[1 - r.week] => {
                (r.week, r.top, r.cursor) = (1 - r.week, 0, 0);
                w.sounds.push(Ui::Cursor as u32);
            }
            _ => {}
        }
        return;
    }
    // quest00 mode 1 / 2: two rows; Enter sets the chat setting (mode 1) or opens a week's ranking (mode 2).
    if let Some(i) = u.info.as_mut() {
        match key {
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                close_info(w);
            }
            K::ArrowUp | K::ArrowDown => {
                i.sel = 3 - i.sel;
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                if mode == Some(1) {
                    i.chat = i.sel % 2 == 1;
                } else {
                    i.ranks = Some(Ranks { week: i.sel as usize - 1, top: 0, cursor: 0 });
                }
            }
            _ => {}
        }
        return;
    }
    // ut_menu: the generic window keys.
    match key {
        K::ArrowUp | K::ArrowDown => {
            let k = if key == K::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(MENU, sel).map_or(sel, |l| l[k] as u16);
            if to != sel && (1..=3).contains(&to) {
                u.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.unity = None;
        }
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            match sel {
                1 | 2 => {
                    let chat = w.game.get(0xb2) != 0;
                    let i = Info { mode: sel as u8, sel: 1, chat, sent: chat, hold: 0.0, ranks: None };
                    if let Some(u) = w.unity.as_mut() {
                        u.info = Some(i);
                    }
                }
                // FUN_1020f3a0(1, 1): the area list's Unity mode over a reset condition.
                _ => super::arealist::open(w, gu, super::arealist::Mode::Unity, Default::default()),
            }
        }
        _ => {}
    }
}

/// Help and title: ut_menu's row strings while it has the focus; titlewin slot 2 "Unity Information" (0xc 0x18b) or
/// "Ranking Information" (0xc 0x18c) while quest00 / quest01 are up.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(u) = &w.unity else { return };
    match &u.info {
        None if w.arealist.is_none() => {
            let [help, title] = gu.focus_text(MENU, u.sel);
            (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        }
        None => {}
        // quest00 / quest01 have no help strings: statcom2's row help must not show through.
        Some(i) => {
            (bars.help[1], bars.help[2]) = (String::new(), String::new());
            bars.title[2] = gu.lobby_msg(0xc, if i.mode == 1 { 0x18b } else { 0x18c }, "");
        }
    }
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

fn right(gu: &mut GameUi, s: &str, x: f32, y: f32) {
    let wd = gu.text_width(s);
    text(gu, s, x - wd, y, WHITE);
}

/// The rank's text: "--" (0xc 0x1b2) for 0, "1st" .. "3rd" (0x1b3..0x1b5), else "%d" of 0x1b6.
fn rank_text(gu: &GameUi, r: u8) -> String {
    match r {
        0 => gu.lobby_msg(0xc, 0x1b2, "--"),
        1..=3 => gu.lobby_msg(0xc, 0x1b2 + r as usize, ""),
        _ => super::textcmd::printf(&gu.lobby_msg(0xc, 0x1b6, "%d"), &[r.to_string()]),
    }
}

/// A quest00 row at window (39, y): the child's plate (quest 0, lit and shifted while selected), the hand.
fn plate(gu: &mut GameUi, x: f32, y: f32, on: bool, focused: bool) {
    gu.sprite("menu    quest", 0, x - on as u8 as f32, y - on as u8 as f32, if on { 0x8080_5020 } else { WHITE });
    if on && focused {
        gu.cursor(x, y);
    }
}

/// ut_menu, ut_point ("%d/%d" accolades / 99999 right at (100, 0x14)), and quest00 / quest01 in the Unity modes.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(u) = &w.unity else { return rects };
    let free = w.confirm.is_none() && w.arealist.is_none();
    if let Some(win) = gu.lobby_window_off(MENU, sw, sh, (0.0, 0.0), Some(u.sel), free && u.info.is_none(), &[]) {
        rects.push(win.frame);
    }
    if let Some(win) = gu.lobby_window_off(POINT, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, ..] = win.frame;
        let acc = if w.recasts_seen { w.stats.as_ref().map_or(0, |s| s.unity >> 10 & 0x1ffff) } else { 0 };
        gu.shape_text(&format!("{acc}/99999"), x + 100.0, y + 20.0, WHITE, true);
        rects.push(win.frame);
    }
    let Some(i) = &u.info else { return rects };
    let Some(win) = gu.lobby_window_off(CHOOSER, sw, sh, (0.0, 0.0), None, false, &[1, 2, 3, 4, 5]) else { return rects };
    rects.push(win.frame);
    let Some((x, y)) = gu.window_xy(CHOOSER, sw, sh) else { return rects };
    let bt = w.battle.clone();
    let focused = free && i.ranks.is_none();
    let lines = |gu: &mut GameUi, x: f32, y: f32| {
        if let Some(bt) = &bt {
            text(gu, &bt.group9(0x61).unwrap_or_default(), x + 0x14 as f32, y, WHITE);
            text(gu, &bt.group9(0x62).unwrap_or_default(), x + 0x14 as f32, y + 0x19 as f32, WHITE);
        }
    };
    let rows_y: [f32; 2] = if i.mode == 1 { [195.0, 218.0] } else { [52.0, 82.0] };
    if i.mode == 1 {
        // FUN_1022fb50.
        text(gu, &gu.lobby_msg(0xc, 0x18b, ""), x + 0x50 as f32, y + 0xf as f32, WHITE);
        text(gu, &gu.lobby_msg(0xc, 0x1b7, ""), x + 0x14 as f32, y + 0xa8 as f32, WHITE);
        if ready(w) {
            for (id, ly) in [(0x18d, 0x2b), (0x18e, 0x44), (0x18f, 0x5d), (0x1c4, 0x76), (0x1c5, 0x8f)] {
                text(gu, &gu.lobby_msg(0xc, id, ""), x + 0x14 as f32, y + ly as f32, WHITE);
            }
            let me = unity_of(w);
            text(gu, &gu.lobby_msg(0xc, 0x192 + me as usize, ""), x + 0xb4 as f32, y + 0x2b as f32, WHITE);
            if me == 0 {
                for ly in [0x44, 0x5d] {
                    text(gu, &gu.lobby_msg(0xc, 0x1b2, "--"), x + 0xdc as f32, y + ly as f32, WHITE);
                }
            } else {
                for (week, ly) in [(0usize, 0x44), (1, 0x5d)] {
                    let r = w.unity_tables.ranking(week);
                    text(gu, &rank_text(gu, rank(&r, me)), x + 0xdc as f32, y + ly as f32, WHITE);
                    right(gu, &format!("{}pt", average(&r, me).min(999_999)), x + 0x15e as f32, y + ly as f32);
                }
                let eval = w.stats.as_ref().map_or(0, |s| s.unity_eval);
                right(gu, &format!("{}pt", eval >> 16), x + 0x15e as f32, y + 0x76 as f32);
                right(gu, &format!("{}pt", eval & 0xffff), x + 0x15e as f32, y + 0x8f as f32);
            }
        } else {
            lines(gu, x, y + 0x2b as f32);
        }
        // LAB_1022f9f0: the yellow marker on the current setting, "Active" / "Inactive" (0xc 0x1b8 + r).
        for (r, &ry) in rows_y.iter().enumerate() {
            let (cx, cy) = (x + 39.0, y + ry);
            let on = i.sel as usize == r + 1;
            plate(gu, cx, cy, on, focused);
            if (r == 0) == i.chat {
                gu.sprite("menu    keytops3", 170, cx, cy + 1.0, WHITE);
            } else {
                gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
            }
            text(gu, &gu.lobby_msg(0xc, 0x1b8 + r, ""), cx + 0x18 as f32, cy, if on { CURSOR } else { WHITE });
        }
    } else if i.ranks.is_none() {
        // FUN_10230060: "Ranking Information" and the two weeks' rows (0xc 400 + r).
        text(gu, &gu.lobby_msg(0xc, 0x18c, ""), x + 0x50 as f32, y + 0xf as f32, WHITE);
        for (r, &ry) in rows_y.iter().enumerate() {
            let (cx, cy) = (x + 39.0, y + ry);
            let on = i.sel as usize == r + 1;
            plate(gu, cx, cy, on, focused);
            gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
            text(gu, &gu.lobby_msg(0xc, 400 + r, ""), cx + 0x10 as f32, cy, if on { CURSOR } else { WHITE });
        }
    }
    let Some(rk) = &i.ranks else { return rects };
    // quest01 modes 5 / 6 (FUN_10232720): the unities in ranking order, the week's title, the bar.
    let order: Vec<(u8, u32)> = if ready(w) { w.unity_tables.ranking(rk.week).into_iter().filter(|e| e.0 != 0).collect() } else { Vec::new() };
    let hide: Vec<u16> = (1..=VISIBLE as u16).filter(|&c| rk.top + c as usize > order.len()).collect();
    let sel = (!order.is_empty()).then(|| (rk.cursor - rk.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, free, &hide) else { return rects };
    rects.push(win.frame);
    let Some((lx, ly)) = gu.window_xy(LIST, sw, sh) else { return rects };
    if order.is_empty() {
        lines(gu, lx, ly + 0x2d as f32);
        return rects;
    }
    text(gu, &gu.lobby_msg(0xc, if rk.week == 0 { 400 } else { 0x191 }, ""), lx + 0x3c as f32, ly + 10.0, WHITE);
    let full = w.unity_tables.ranking(rk.week);
    for &(id, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=VISIBLE as u16).contains(id)) {
        let row = rk.top + id as usize - 1;
        let Some(&(un, avg)) = order.get(row) else { continue };
        let tint = if row == rk.cursor { CURSOR } else { WHITE };
        gu.sprite("menu    framesus", 35, cx + 4.0, cy + 5.0, WHITE);
        text(gu, &rank_text(gu, rank(&full, un)), cx + 0x10 as f32, cy, tint);
        text(gu, &gu.lobby_msg(0xc, 0x192 + un as usize, ""), cx + 0x3a as f32, cy, tint);
        text(gu, &format!("{}pt", avg), cx + 0xfa as f32, cy, tint);
    }
    let [fx, fy, fw, fh] = win.frame;
    gu.scroll_bar(fx + fw - 8.0, fy, fh, order.len(), VISIBLE, rk.top);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks() {
        let mut t = Tables::default();
        let mut members = [0u8; 0x80];
        let mut points = [0u8; 0x80];
        // Unity 1: 300 / 3 = 100, unity 2: 200 / 2 = 100, unity 3: 50 / 1 = 50.
        for (u, m, p) in [(1usize, 3u32, 300u32), (2, 2, 200), (3, 1, 50)] {
            members[4 * u..4 * u + 4].copy_from_slice(&m.to_le_bytes());
            points[4 * u..4 * u + 4].copy_from_slice(&p.to_le_bytes());
        }
        t.store(1, 1, &members);
        t.store(1, 2, &points);
        let r = t.ranking(1);
        assert_eq!(&r[..3], &[(1, 100), (2, 100), (3, 50)]);
        assert_eq!([rank(&r, 1), rank(&r, 2), rank(&r, 3), rank(&r, 4)], [1, 1, 3, 4]);
        assert_eq!(rank(&t.ranking(0), 1), 0);
    }
}
