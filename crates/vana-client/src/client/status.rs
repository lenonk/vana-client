//! The main menu's Status row (FUN_10140400 case 5 / child 1; spec ~/ffxi/lab/research/status_window_spec.md): persona
//! at (16, 16), level under it, statcom2 over the main menu, and the page statcom2 shows at (130, 48): profile,
//! joblevel, masterle, btlskill, mgcskill or trdskill.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;

pub const STATCOM: &str = "menu    statcom2";
const PERSONA: &str = "menu    persona";
const LEVEL: &str = "menu    level";
const WHITE: u32 = 0x8080_8080;
const BLUE: u32 = 0x8020_60a0;

/// FUN_10148d00's display pages by statcom2 child (table 0x103716b8, and masterle for child 12).
fn page(child: u16) -> Option<&'static str> {
    Some(match child {
        1 => "menu    profile",
        2 => "menu    joblevel",
        3 => "menu    btlskill",
        4 => "menu    mgcskill",
        5 => "menu    trdskill",
        12 => "menu    masterle",
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct StatusUi {
    pub sel: u16,
    /// +0x18: the child whose page is shown.
    pub shown: Option<u16>,
    /// Disabled children; 3 / 4 / 5 are enabled once when the stats are first ready (+0x14), 12 / 13 when their test
    /// first passes (+0x15 / +0x16).
    off: Vec<u16>,
    once: bool,
    /// c2s 0x061 {0} out (helpwind busy bit 0x40) until the next 0x061.
    pub busy: bool,
    return_menu: Option<usize>,
}

/// FUN_10140400 case 5: persona in Status mode (FUN_10146ec0: at (16, 16), its group 0x30 so titlewin is suspended),
/// level, statcom2; persona's first frame asks for fresh stats (FUN_10147030). Children 3, 4, 5, 12, 13 start disabled;
/// 7 unless 0x063 type 2 has canUseMeritMode (bit 13); 8 unless 0x063 type 5's access and the main level is 99+.
pub fn open(w: &mut World) {
    let mut off = vec![3, 4, 5, 12, 13];
    if !w.merits.is_some_and(|m| m.1 & 0x2000 != 0) {
        off.push(7);
    }
    let level = w.stats.as_ref().map_or(0, |s| s.main.1);
    if !(w.jp_access.is_some_and(|a| a & 1 != 0) && level >= 99) {
        off.push(8);
    }
    let _ = w.net.commands.send(NetCommand::StatusRequest(0));
    w.status = Some(StatusUi { sel: w.cursor_recall(STATCOM), shown: None, off, once: false, busy: true, return_menu: w.menu });
    w.menu = None;
}

/// s2c 0x061 (LAB_10147010): the busy strip ends.
pub fn on_stats(w: &mut World) {
    if let Some(s) = w.status.as_mut() {
        s.busy = false;
    }
}

/// A skill_base entry exists: bit 15 clear or a value (0x8000 = none).
fn exists(v: u16) -> bool {
    v & 0x8000 == 0 || v & 0x7fff != 0
}

/// Per frame (FUN_10148be0): once stats are ready, 3 / 4 / 5 by the skills there are (once); 12 while the main level is
/// 99+ with a master level unlocked (0x061 @0x66 bit 0), 13 with the level and key item 0xd21 (each for good).
pub fn update(w: &mut World) {
    let ready = w.recasts_seen && w.stats.is_some();
    let level = w.stats.as_ref().map_or(0, |s| s.main.1);
    let ml = w.stats.as_ref().is_some_and(|s| s.ml.1 & 1 != 0);
    let ae = w.has_key_item(0xd21);
    let skills = w.skill_base;
    let Some(s) = w.status.as_mut() else { return };
    if !ready {
        return;
    }
    if !s.once {
        s.once = true;
        if let Some(b) = skills {
            let on = [b[1..32].iter().any(|&v| exists(v)), b[32..48].iter().any(|&v| exists(v)), b[48..63].iter().any(|&v| v != 0xffff)];
            for (c, on) in [3u16, 4, 5].into_iter().zip(on) {
                if on {
                    s.off.retain(|&o| o != c);
                }
            }
        }
    }
    if level > 98 && ml {
        s.off.retain(|&o| o != 12);
    }
    if level > 98 && ae {
        s.off.retain(|&o| o != 13);
    }
}

fn close(w: &mut World) {
    if let Some(s) = w.status.take() {
        w.menu = s.return_menu;
    }
}

/// FUN_10148d00 after the generic part: Up / Down along the links (SE 0001); Enter on a disabled row SE 0004, else SE
/// 0002 and the row (the shown row again: nothing; else the page closes first); Escape (SE 0003) closes the page, or
/// statcom2 with persona and level when none is shown.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(s) = w.status.clone() else { return };
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let k = if key == egui::Key::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(STATCOM, s.sel).map_or(s.sel, |l| l[k] as u16);
            if to != s.sel && to != 0 && to != 0xff
                && let Some(st) = w.status.as_mut()
            {
                st.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            match w.status.as_mut() {
                Some(st) if st.shown.is_some() => st.shown = None,
                _ => close(w),
            }
        }
        egui::Key::Enter if s.off.contains(&s.sel) => w.sounds.push(Ui::Error as u32),
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            if s.shown == Some(s.sel) {
                return;
            }
            let shown = page(s.sel).map(|_| s.sel);
            if let Some(st) = w.status.as_mut() {
                st.shown = shown;
            }
            match s.sel {
                9 => super::textcmd::run(w, "/playtime", super::textcmd::Mode::Internal),
                // FUN_1022a620(1 / 2): evitem with the currencies.
                6 => super::currencies::open(w, 1),
                10 => super::currencies::open(w, 2),
                11 => super::unity::open(w),
                c if page(c).is_none() => log::info!("status: statcom2 row {c} is not implemented yet"),
                _ => {}
            }
        }
        _ => {}
    }
}

/// Mouse: hover over a row moves the cursor, a click on the cursor row is Enter, a right click Escape *(inf: the
/// generic window events)*.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(s) = w.status.clone() else { return };
    let Some((wx, wy)) = gu.window_xy(STATCOM, sw, sh) else { return };
    let Some(layout) = gu.layout(STATCOM) else { return };
    let hit = layout.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).find(|e| {
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
    });
    if right && gu.window_rect(STATCOM, sw, sh).is_some_and(|r| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]) {
        key(w, gu, egui::Key::Escape);
        return;
    }
    let Some(e) = hit else { return };
    gu.hover.set(true);
    if (moved || clicked) && e.id != s.sel
        && let Some(st) = w.status.as_mut()
    {
        st.sel = e.id;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// Help slot 1 from the cursor row; with the profile up, slot 2 "Title: <name>" (group 0xe 0x4e + the title,
/// FUN_10147ac0); the busy strip while the stats request is out.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.status else { return };
    let [help, _] = gu.focus_text(STATCOM, s.sel);
    bars.help[1] = help.unwrap_or_default();
    if s.shown == Some(1)
        && let (Some(st), Some(names)) = (&w.stats, &w.names)
    {
        use crate::dat::names::NameSource;
        bars.help[2] = format!("{}{}", gu.menu_msg(0x4e, ""), names.title(st.title as u32).unwrap_or_default());
    }
}

/// FUN_101c9440(v, 0, 1): 0..999,999,999 in groups of three with group 0xc 0x5b between.
fn grouped(v: i64, sep: &str) -> String {
    let v = v.clamp(0, 999_999_999);
    match v {
        0..1000 => format!("{v}"),
        1000..1_000_000 => format!("{}{sep}{:03}", v / 1000, v % 1000),
        _ => format!("{}{sep}{:03}{sep}{:03}", v / 1_000_000, v / 1000 % 1000, v % 1000),
    }
}

fn gauge(gu: &mut GameUi, lib: &str, x: f32, y: f32, frame: usize, fill: usize, f: f32) {
    let (b, ui, _, _) = gu.parts();
    if let Some(l) = ui.lib(lib) {
        crate::render::screens::push_gauge(b, ui, l, x, y, frame, fill, f);
    }
}

/// FUN_101476b0, with stats ready: "Level N" (0x067 @0x25, else the main level), EXP and NEXTLevel with the gauge; or
/// the master level, exemplar points and the next with theirs once one is unlocked (0x061 @0x66 bit 0). Blue / grey
/// while EXP is capped (0x063 type 2 bit 14; for the master level @0x66 bit 1 with the points at the next - 1).
fn draw_level(gu: &mut GameUi, w: &World, x: f32, y: f32) {
    let Some(st) = w.stats.as_ref().filter(|_| w.recasts_seen) else { return };
    let sep = gu.lobby_msg(0xc, 0x5b, ",");
    let ml = st.ml.1 & 1 != 0;
    let capped = if ml { st.ml.1 & 2 != 0 && st.ml_exp.0 >= st.ml_exp.1.wrapping_sub(1) } else { w.merits.is_some_and(|m| m.1 & 0x4000 != 0) };
    let (c1, c2) = if capped { (BLUE, 0x8040_4040) } else { (WHITE, WHITE) };
    let (head, level, l1, l2, now, next) = if ml {
        (gu.lobby_msg(0xc, 0x203, ""), st.ml.0, gu.lobby_msg(0xc, 0x204, ""), gu.lobby_msg(0xc, 0x205, ""), st.ml_exp.0 as i64, st.ml_exp.1 as i64)
    } else {
        (gu.lobby_msg(0xc, 0x56, ""), w.real_level.unwrap_or(st.main.1), gu.lobby_msg(0xc, 0x57, ""), gu.lobby_msg(0xc, 0x58, ""), st.exp.0 as i64, st.exp.1 as i64)
    };
    gu.shape_text(&format!("{head} {level}"), x + 8.0, y + 10.0, c1, false);
    gu.shape_text(&l1, x + if ml { 7.0 } else { 15.0 }, y + 0x16 as f32, c2, false);
    gu.shape_text(&grouped(now, &sep), x + 0x5a as f32, y + 0x22 as f32, c2, true);
    gu.shape_text(&l2, x + 15.0, y + 0x2e as f32, c2, false);
    gu.shape_text(&grouped(next, &sep), x + 0x5a as f32, y + 0x3a as f32, c2, true);
    let f = if next > 0 { (now as f32 / next as f32).min(1.0) } else { 0.0 };
    let (frame, fill) = crate::render::screens::GAUGE_HP;
    gauge(gu, "menu    windowps", x + 0x16 as f32, y + 0x48 as f32, frame, fill, f);
}

/// FUN_10107320's table 0x10364cb0 (English).
const RACES: [&str; 9] = ["", "Hume", "Hume", "Elvaan", "Elvaan", "Tarutaru", "Tarutaru", "Mithra", "Galka"];

/// FUN_10147ac0: the nation crest (keytops3 74 + nation), the race, the home point (group 4), the rank and its gauge
/// (framesus 103 / 98, points / 4096); with the Monstrosity data (0x063 types 3 and 4) the gladiator rank, prestige,
/// mastery rank and mentor.
fn draw_profile(gu: &mut GameUi, w: &World, x: f32, y: f32) {
    let Some(st) = w.stats.as_ref().filter(|_| w.recasts_seen) else { return };
    if st.nation < 3 {
        gu.sprite("menu    keytops3", 74 + st.nation as usize, x + 0x37 as f32, y + 0x12 as f32, WHITE);
    }
    let home = crate::dat::font::decode_sjis(&gu.area_name(st.home));
    gu.shape_text(&home, x + 0x5e as f32, y + 0x34 as f32, WHITE, false);
    if let Some(r) = RACES.get(w.my_race as usize).filter(|r| !r.is_empty()) {
        gu.shape_text(r, x + 0x32 as f32, y + 0x24 as f32, WHITE, false);
    }
    gu.shape_text(&st.rank.to_string(), x + 0x40 as f32, y + 0x44 as f32, WHITE, true);
    gauge(gu, "menu    framesus", x + 0x5c as f32, y + 0x54 as f32, 98, 103, st.rank_points as f32 / 4096.0);
    if let Some((_, rank, prestige)) = w.monstrosity.filter(|m| m.0 & 0x18 == 0x18) {
        if let Some(g) = ["Gla", "CG", "HCG"].get((rank >> 2 & 3) as usize) {
            gu.shape_text(g, x + 0x8c as f32, y + 0x91 as f32, WHITE, false);
        }
        gu.shape_text(&prestige.to_string(), x + 0x5a as f32, y + 0xa1 as f32, WHITE, false);
        let (mentor, mastery) = w.job_info.map_or((0, 0), |j| (j.mentor, j.mastery));
        gu.shape_text(&mastery.to_string(), x + 0x6e as f32, y + 0x6a as f32, WHITE, false);
        if mentor > 0 {
            gu.sprite("menu    keytops3", 560 + mentor as usize, x + 0xda as f32, y + 0x68 as f32, WHITE);
        } else {
            gu.shape_text("--", x + 0xda as f32, y + 0x68 as f32, WHITE, false);
        }
    }
}

/// joblevel (FUN_10148030) / masterle (FUN_1014c540): the jobs 1..22 there are, alternating left and right from y 0x13,
/// 14 px a row: the name (windowps 212 + job) at x 0x0a / 0x82, gold for the main job, blue for the support job; the
/// level right-aligned at 0x6e / 0xe6.
fn job_rows(w: &World, master: bool) -> Vec<(u8, u8)> {
    let (Some(j), Some(st)) = (w.job_info, w.stats.as_ref()) else { return Vec::new() };
    let flags = if master { j.ml_jobs } else { j.flags & !1 };
    (1..=22u8)
        .filter(|&n| flags >> n & 1 != 0)
        .map(|n| {
            let lv = match (master, n == st.main.0) {
                (false, true) => st.main.1,
                (true, true) => st.ml.0,
                (false, false) => j.levels[n as usize],
                (true, false) => j.ml_levels[n as usize],
            };
            (n, lv)
        })
        .collect()
}

fn draw_jobs(gu: &mut GameUi, w: &World, x: f32, y: f32, rows: &[(u8, u8)]) {
    let Some(st) = w.stats.as_ref().filter(|_| w.recasts_seen) else { return };
    for (k, &(job, lv)) in rows.iter().enumerate() {
        let (col, ry) = (k & 1, y + 0x13 as f32 + (k / 2) as f32 * 14.0);
        let tint = if job == st.main.0 {
            0x80e0_9620
        } else if job == st.sub.0 {
            0x8040_70a0
        } else {
            WHITE
        };
        gu.sprite("menu    windowps", 212 + job as usize, x + [0x0a, 0x82][col] as f32, ry, tint);
        gu.shape_text(&lv.to_string(), x + [0x6e, 0xe6][col] as f32, ry, WHITE, true);
    }
}

/// The skills of btlskill (1..31) / mgcskill (32..47) that exist (FUN_101484b0 / FUN_101486b0).
fn skill_rows(w: &World, magic: bool) -> Vec<(usize, u16)> {
    let Some(b) = w.skill_base else { return Vec::new() };
    let r = if magic { 32..48 } else { 1..32 };
    r.filter(|&i| exists(b[i])).map(|i| (i, b[i])).collect()
}

/// Two a row like the jobs: the name keytops3 208 + id at 0x0a / 0x82, the value right-aligned (combat 0x78 / 0xf0,
/// magic 0x6e / 0xe6), blue when capped (bit 15).
fn draw_skills(gu: &mut GameUi, x: f32, y: f32, rows: &[(usize, u16)], magic: bool) {
    let vx = if magic { [0x6e, 0xe6] } else { [0x78, 0xf0] };
    for (k, &(id, v)) in rows.iter().enumerate() {
        let (col, ry) = (k & 1, y + 0x13 as f32 + (k / 2) as f32 * 14.0);
        gu.sprite("menu    keytops3", 208 + id, x + [0x0a, 0x82][col] as f32, ry, WHITE);
        gu.shape_text(&(v & 0x7fff).to_string(), x + vx[col] as f32, ry, if v & 0x8000 != 0 { BLUE } else { WHITE }, true);
    }
}

/// trdskill (FUN_101488a0): a row per craft 48..62 there is: the name keytops3 208 + id at 0x0a, the rank (keytops3 193 +
/// rank, below 16) at 0x6e in blue when capped, "(%d)" of the level right-aligned at 0xf5.
fn craft_rows(w: &World) -> Vec<(usize, u16)> {
    let Some(b) = w.skill_base else { return Vec::new() };
    (48..63).filter(|&i| b[i] != 0xffff).map(|i| (i, b[i])).collect()
}

fn draw_crafts(gu: &mut GameUi, x: f32, y: f32, rows: &[(usize, u16)]) {
    for (k, &(id, v)) in rows.iter().enumerate() {
        let ry = y + 0x13 as f32 + k as f32 * 14.0;
        gu.sprite("menu    keytops3", 208 + id, x + 0x0a as f32, ry, WHITE);
        if v & 0x1f < 0x10 {
            gu.sprite("menu    keytops3", 193 + (v & 0x1f) as usize, x + 0x6e as f32, ry, if v & 0x8000 != 0 { BLUE } else { WHITE });
        }
        gu.shape_text(&format!("({})", (v >> 5) & 0x3ff), x + 0xf5 as f32, ry, WHITE, true);
    }
}

/// persona (16, 16), level, statcom2 with its disabled rows, and the shown page at its computed height (FUN_1011a5a0:
/// ((n + 1) / 2) * 14 + 0x16 for two a row, n * 14 + 0x16 for the crafts).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    gu.set_group(PERSONA, w.status.is_some().then_some(0x30));
    let Some(s) = &w.status else { return rects };
    if gu.gate_layout_at(PERSONA, sw, sh, None, Some(16))
        && let Some(mut m) = gu.layout(PERSONA).cloned()
    {
        m.elements[0].y = 16;
        let p = super::items::persona(w);
        let (b, ui, style, _) = gu.parts();
        crate::render::invwin::push_persona(b, ui, style, &m, &p, sw, sh);
    }
    rects.extend(gu.window_rect(PERSONA, sw, sh).map(|mut r| {
        r[1] -= 32.0;
        r
    }));
    if let Some((x, y)) = gu.window(LEVEL, sw, sh) {
        draw_level(gu, w, x, y);
    }
    rects.extend(gu.window_rect(LEVEL, sw, sh));
    if let Some(name) = s.shown.and_then(page) {
        let layout_h = gu.layout(name).map_or(0.0, |m| m.elements[0].h as f32);
        let two = |n: usize| ((n + 1) / 2) as f32 * 14.0 + 22.0;
        let h = match s.shown {
            Some(2) => Some(two(job_rows(w, false).len())),
            Some(12) => Some(two(job_rows(w, true).len())),
            Some(3) => Some(two(skill_rows(w, false).len())),
            Some(4) => Some(two(skill_rows(w, true).len())),
            Some(5) => Some(craft_rows(w).len() as f32 * 14.0 + 22.0),
            _ => None,
        };
        let grow = h.map_or(0.0, |h| h - layout_h);
        if let Some((x, y)) = gu.window_off_grow(name, sw, sh, 0.0, 0.0, grow) {
            match s.shown {
                Some(1) => draw_profile(gu, w, x, y),
                Some(2) => draw_jobs(gu, w, x, y, &job_rows(w, false)),
                Some(12) => draw_jobs(gu, w, x, y, &job_rows(w, true)),
                Some(3) => draw_skills(gu, x, y, &skill_rows(w, false), false),
                Some(4) => draw_skills(gu, x, y, &skill_rows(w, true), true),
                Some(5) => draw_crafts(gu, x, y, &craft_rows(w)),
                _ => {}
            }
        }
        rects.extend(gu.window_rect(name, sw, sh).map(|mut r| {
            r[3] += grow;
            r
        }));
    }
    let focused = w.confirm.is_none() && w.currencies.is_none() && w.unity.is_none();
    if let Some(win) = gu.lobby_window_off(STATCOM, sw, sh, (0.0, 0.0), Some(s.sel), focused, &s.off) {
        if let Some((wx, wy)) = gu.window_xy(STATCOM, sw, sh) {
            gu.disabled_children(STATCOM, wx, wy, &s.off, Some(s.sel).filter(|_| focused));
        }
        rects.push(win.frame);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!([grouped(0, ","), grouped(999, ","), grouped(1234, ","), grouped(12_345_678, ",")], ["0", "999", "1,234", "12,345,678"]);
        assert!(exists(0) && exists(0x8005) && !exists(0x8000));
    }
}
