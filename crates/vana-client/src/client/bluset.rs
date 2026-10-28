//! Blue Mage set spells (blue_magic_spec.md): s2c 0x044's blocks, the cast rules they feed (client/magic.rs), and the
//! Set Spells screen opened from blusortw row 3 (FUN_101df020): persona, bluepoin, bluequip (the slots), bluinven (the
//! known spells), bluehelp and bluesibo (Refine by Type), sending c2s 0x102.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::dat::blumagic::{self, BluData};
use crate::dat::names::NameSource;
use std::time::{Duration, Instant};

const EQUIP: &str = "menu    bluequip";
const INVEN: &str = "menu    bluinven";
const POINTS: &str = "menu    bluepoin";
const HELP: &str = "menu    bluehelp";
const SIBO: &str = "menu    bluesibo";
const PERSONA: &str = "menu    persona";
const VISIBLE: usize = 10;
const WHITE: u32 = 0x8080_8080;
const PICKED: u32 = 0x8080_4020;
const SET: u32 = 0x8050_9050;
const GREY: u32 = 0x8040_4040;
/// FUN_100fc3c0: one c2s 0x102 per second, extras dropped.
pub(super) const THROTTLE: Duration = Duration::from_millis(1000);
/// DAT_1037f1dc / DAT_1037f1e8: the slot numbers' colours by the level a set needs.
const BAND_LEVELS: [u8; 9] = [20, 30, 40, 50, 60, 70, 80, 90, 99];
const BAND_COLOURS: [u32; 9] = [0x8080_8080, 0x8090_9050, 0x8050_8080, 0x8080_6090, 0x8050_9050, 0x8090_6050, 0x8050_6090, 0x8060_6050, 0x8060_4010];
/// bluesibo's rows by family (rows 15..18 are families 16..19).
const SIBO_ROWS: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Equip,
    Inven,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// framesus 130 "Type ▶": bluesibo.
    Type,
    /// group 9 0xe3 "(Remove All)".
    RemoveAll,
    Spell(u16),
}

#[derive(Clone, Debug)]
pub struct Screen {
    /// bluequip +0x5c: 0 main BLU, 1 sub BLU; +0x5f / +0x5e and the level they come from (FUN_101df020).
    side: usize,
    slots: usize,
    total: u32,
    level: u8,
    focus: Focus,
    equip: (usize, usize),
    inven: (usize, usize),
    /// bluequip +0x50: a slot picked first (then a spell).
    slot_pick: Option<usize>,
    /// a8+0x10: a spell picked first (then a slot).
    spell_pick: Option<u16>,
    /// a8+0x14: the family shown (0 all).
    filter: u8,
    /// bluesibo's cursor row while it is open.
    sibo: Option<u16>,
}

/// FUN_100fcb70: a block is kept only when its job is the one s2c 0x01B gave that side. The stamp (FUN_1022b290):
/// a new non-zero one starts the 60 s reset lockout (recast slot 0x400).
pub fn ext_job(w: &mut World, b: &[u8]) {
    let (job, side) = (b[0], b[1] as usize);
    let want = w.job_info.map(|j| if side == 0 { j.main } else { j.sub });
    if side > 1 || want != Some(job) {
        return;
    }
    w.ext_job[side].copy_from_slice(&b[..0x9c]);
    if job == 0x10 {
        log::info!("blue magic: side {side} set {:?}", &b[4..0x18]);
    }
    let stamp = w.ext_job.iter().find(|b| b[0] == 0x10).map_or(0, |b| u32::from_le_bytes([b[0x18], b[0x19], b[0x1a], b[0x1b]]));
    if stamp != 0 && stamp != w.blu_stamp {
        w.blu_reset = Some(Instant::now() + Duration::from_secs(60));
    }
    w.blu_stamp = stamp;
}

/// The side the blue magic rules read: main BLU 0, else sub BLU 1.
pub fn side(w: &World) -> Option<usize> {
    let st = w.stats.as_ref()?;
    if st.main.0 == 16 {
        Some(0)
    } else if st.sub.0 == 16 {
        Some(1)
    } else {
        None
    }
}

/// FUN_100fcbc0(side, 0x1000, i): slot i's spell (id - 0x200), 0 empty.
fn slot(w: &World, side: usize, i: usize) -> u8 {
    let b = &w.ext_job[side];
    if b[0] == 0x10 && i < 20 { b[4 + i] } else { 0 }
}

/// A blue spell in the side's 20 slots.
pub fn is_set(w: &World, s: u16) -> bool {
    let Some(side) = side(w) else { return false };
    let idx = s.wrapping_sub(0x200);
    idx != 0 && idx < 0x100 && (0..20).any(|i| slot(w, side, i) as u16 == idx)
}

/// Recast slot 0x400 running: "Your blue magic spells have recently been reset."
pub fn reset_active(w: &World) -> bool {
    w.blu_reset.is_some_and(|t| t > Instant::now())
}

fn data(w: &World) -> Option<&BluData> {
    w.blu_data.as_deref()
}

/// The spell's BLU level (record +0x2e).
fn spell_level(w: &World, s: u16) -> i16 {
    w.actions.as_ref().and_then(|a| a.spell(s)).map_or(-1, |r| r.levels[16])
}

/// FUN_101df130: the points of a level plus the s2c 0x063 type 2 bonus (u16 @0xa bits 7..12).
fn total_points(w: &World, level: u8) -> u32 {
    blumagic::points(level) + w.merits.map_or(0, |m| (m.1 as u32 >> 7) & 0x3f)
}

fn cost(w: &World, idx: u8) -> u32 {
    if idx == 0 {
        return 0;
    }
    data(w).and_then(|d| d.entry(0x200 + idx as u16)).map_or(0, |e| e.points as u32)
}

/// The points the side's slots use.
fn used(w: &World, s: &Screen) -> u32 {
    (0..20).map(|i| cost(w, slot(w, s.side, i))).sum()
}

/// blusortw row 3 (FUN_101df020): 0x119 seen and BLU main or sub, else group 7 0x57.
pub fn open(w: &mut World) {
    let Some(side) = side(w).filter(|_| w.recasts_seen) else {
        super::textcmd::refuse(w, 0x57);
        return;
    };
    let st = w.stats.as_ref().unwrap();
    let level = if side == 0 { st.main.1 } else { st.sub.1 };
    let (slots, total) = (blumagic::slots(level), total_points(w, level));
    w.bluset = Some(Screen { side, slots, total, level, focus: Focus::Equip, equip: (0, 0), inven: (0, 0), slot_pick: None, spell_pick: None, filter: 0, sibo: None });
}

/// FUN_101dfe30: Type, Remove All, then the known blue spells in the mix.dat order that have points and the family.
fn inven_rows(w: &World, s: &Screen) -> Vec<Row> {
    let Some(d) = data(w) else { return vec![Row::Type, Row::RemoveAll] };
    let spells = super::magic::blue_known(w).into_iter().filter(|&id| d.entry(id).is_some_and(|e| e.points != 0 && e.points != 0xff && (s.filter == 0 || e.family == s.filter)));
    [Row::Type, Row::RemoveAll].into_iter().chain(spells.map(Row::Spell)).collect()
}

/// FUN_101df1a0: what is left, the picked slot's spell given back.
fn remaining(w: &World, s: &Screen) -> u32 {
    let back = s.slot_pick.map_or(0, |i| cost(w, slot(w, s.side, i)));
    (s.total + back).saturating_sub(used(w, s))
}

/// bluinven's name colour: picked, set, too dear or too high, else white.
fn spell_tint(w: &World, s: &Screen, id: u16) -> u32 {
    let idx = id - 0x200;
    if s.spell_pick == Some(id) {
        PICKED
    } else if (0..20).any(|i| slot(w, s.side, i) as u16 == idx) {
        SET
    } else if refused(w, s, id) {
        GREY
    } else {
        WHITE
    }
}

fn refused(w: &World, s: &Screen, id: u16) -> bool {
    cost(w, (id - 0x200) as u8) > remaining(w, s) || (s.level as i16) < spell_level(w, id)
}

/// The slot numbers' colours (FUN_101df730): the band whose points and slots hold the sets so far.
fn band_colours(w: &World, s: &Screen) -> Vec<u32> {
    let pts: Vec<u32> = BAND_LEVELS.iter().map(|&l| total_points(w, l)).collect();
    let n: Vec<usize> = BAND_LEVELS.iter().map(|&l| blumagic::slots(l)).collect();
    let (mut used, mut count, mut k) = (0u32, 0usize, 0usize);
    (0..s.slots)
        .map(|i| {
            let idx = slot(w, s.side, i);
            if idx != 0 {
                used += cost(w, idx);
                while k < 8 && pts[k] < used {
                    k += 1;
                }
                count += 1;
                if k < 8 && n[k] < count {
                    k += 1;
                }
            }
            BAND_COLOURS[k]
        })
        .collect()
}

/// FUN_100fcf80 / FUN_100fd100: c2s 0x102 {u8 spell @4, the block's first dword @8, slot bytes @0xc..}: a set puts
/// the spell in its slot, a removal (spell 0) lists the slots to empty. Not sent when nothing changes or within a
/// second of the last.
fn send(w: &mut World, s: &mut Screen, spell: u8, slots: &[(usize, u8)]) {
    let b = w.ext_job[s.side];
    if b[0] != 0x10 || slots.is_empty() || w.ext_job_sent.is_some_and(|t| t.elapsed() < THROTTLE) {
        return;
    }
    let mut body = vec![0u8; 0xa0];
    body[0] = spell;
    body[4..8].copy_from_slice(&b[0..4]);
    for &(i, v) in slots {
        body[8 + i] = v;
    }
    log::info!("blue magic: c2s 0x102 spell {spell} slots {slots:?}");
    w.ext_job_sent = Some(Instant::now());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x102, body });
}

/// Slot `i` gets spell `idx`, or loses it when it already holds it.
fn set_or_remove(w: &mut World, s: &mut Screen, i: usize, idx: u8) {
    if i > 19 {
        return;
    }
    let cur = slot(w, s.side, i);
    if cur == idx {
        send(w, s, 0, &[(i, cur)]);
    } else {
        send(w, s, idx, &[(i, idx)]);
    }
}

/// The Remove All prompt's Yes (LAB_101e0560).
pub fn remove_all(w: &mut World) {
    let Some(mut s) = w.bluset.take() else { return };
    let all: Vec<(usize, u8)> = (0..20).map(|i| (i, slot(w, s.side, i))).filter(|v| v.1 != 0).collect();
    send(w, &mut s, 0, &all);
    w.bluset = Some(s);
}

/// The generic list: Up / Down stop at the ends, Left / Right a page of 10. True when it moved.
fn step(c: &mut (usize, usize), n: usize, key: egui::Key) -> bool {
    use egui::Key as K;
    let old = c.0;
    match key {
        K::ArrowDown if c.0 + 1 < n => c.0 += 1,
        K::ArrowUp if c.0 > 0 => c.0 -= 1,
        K::ArrowRight => c.0 = (c.0 + VISIBLE).min(n.saturating_sub(1)),
        K::ArrowLeft => c.0 = c.0.saturating_sub(VISIBLE),
        _ => {}
    }
    c.1 = c.1.min(c.0).max((c.0 + 1).saturating_sub(VISIBLE));
    c.0 != old
}

/// bluequip (FUN_101dfa50) and bluinven (FUN_101e0340) keys, bluesibo's over them.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut s) = w.bluset.take() else { return };
    let keep = keys(w, &mut s, key);
    if keep && w.bluset.is_none() {
        w.bluset = Some(s);
    }
}

fn keys(w: &mut World, s: &mut Screen, key: egui::Key) -> bool {
    use egui::Key as K;
    if let Some(r) = s.sibo {
        match key {
            K::ArrowDown | K::ArrowUp => {
                s.sibo = Some(if key == K::ArrowDown { r % SIBO_ROWS + 1 } else if r <= 1 { SIBO_ROWS } else { r - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            // FUN_101dfc40: families 1..14 by row, then 16..19.
            K::Enter => {
                s.filter = if r > 14 { r as u8 + 1 } else { r as u8 };
                s.sibo = None;
                let n = inven_rows(w, s).len();
                s.inven.0 = s.inven.0.min(n - 1);
                s.inven.1 = s.inven.1.min(s.inven.0);
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                s.sibo = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return true;
    }
    let rows = inven_rows(w, s);
    match (s.focus, key) {
        (_, K::Escape) => match (s.focus, s.spell_pick, s.slot_pick) {
            (Focus::Equip, Some(_), _) => {
                (s.spell_pick, s.focus) = (None, Focus::Inven);
                w.sounds.push(Ui::Cancel as u32);
            }
            (Focus::Inven, _, Some(_)) => {
                (s.slot_pick, s.focus) = (None, Focus::Equip);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {
                w.sounds.push(Ui::Cancel as u32);
                return false;
            }
        },
        (Focus::Equip, K::Enter) => {
            let i = s.equip.0;
            if let Some(id) = s.spell_pick.take() {
                set_or_remove(w, s, i, (id - 0x200) as u8);
                s.focus = Focus::Inven;
            } else {
                // FUN_101e01b0: bluinven's cursor on the slot's spell.
                s.slot_pick = Some(i);
                let idx = slot(w, s.side, i);
                if let Some(r) = rows.iter().position(|r| *r == Row::Spell(0x200 + idx as u16)).filter(|_| idx != 0) {
                    s.inven.0 = r;
                    s.inven.1 = s.inven.1.min(r).max((r + 1).saturating_sub(VISIBLE));
                }
                s.focus = Focus::Inven;
            }
            w.sounds.push(Ui::Select as u32);
        }
        (Focus::Inven, K::Enter) => match rows.get(s.inven.0) {
            Some(Row::Type) => {
                (s.filter, s.sibo) = (0, Some(1));
                w.sounds.push(Ui::Select as u32);
            }
            Some(Row::RemoveAll) => {
                w.confirm = Some((super::Confirm::BlueRemoveAll, false));
                w.sounds.push(Ui::Select as u32);
            }
            // Grey rows refuse (SE 0004); set (green) ones are taken.
            Some(&Row::Spell(id)) if spell_tint(w, s, id) != GREY => {
                if let Some(i) = s.slot_pick.take() {
                    set_or_remove(w, s, i, (id - 0x200) as u8);
                    s.focus = Focus::Equip;
                } else {
                    s.spell_pick = Some(id);
                    // bluequip +0x54: its cursor on the slot holding the spell.
                    if let Some(i) = (0..s.slots).find(|&i| slot(w, s.side, i) as u16 == id - 0x200) {
                        s.equip.0 = i;
                        s.equip.1 = s.equip.1.min(i).max((i + 1).saturating_sub(VISIBLE));
                    }
                    s.focus = Focus::Equip;
                }
                w.sounds.push(Ui::Select as u32);
            }
            _ => w.sounds.push(Ui::Error as u32),
        },
        (Focus::Equip, _) => {
            if step(&mut s.equip, s.slots, key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        (Focus::Inven, _) => {
            if step(&mut s.inven, rows.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
    true
}

/// Num+ (message 8): with nothing picked the other list takes the focus (SE 0013); bluequip with a pick refuses.
pub fn num_plus(w: &mut World) {
    let Some(s) = w.bluset.as_mut().filter(|s| s.sibo.is_none()) else { return };
    match (s.focus, s.spell_pick.is_some() || s.slot_pick.is_some()) {
        (Focus::Equip, true) => w.sounds.push(Ui::Error as u32),
        (_, true) => {}
        (f, false) => {
            s.focus = if f == Focus::Equip { Focus::Inven } else { Focus::Equip };
            w.sounds.push(Ui::Close as u32);
        }
    }
}

/// A list row under the pointer: (window, row index from the top).
fn hit(gu: &GameUi, sw: f32, sh: f32, p: (f32, f32)) -> Option<(Focus, usize)> {
    for (name, f) in [(EQUIP, Focus::Equip), (INVEN, Focus::Inven)] {
        let Some((x, y)) = gu.window_xy(name, sw, sh) else { continue };
        for k in 0..VISIBLE {
            let (rx, ry) = (x + 3.0, y + 6.0 + 18.0 * k as f32);
            if p.0 >= rx && p.0 < rx + 132.0 && p.1 >= ry && p.1 < ry + 16.0 {
                return Some((f, k));
            }
        }
    }
    None
}

/// Mouse *(inf)*: over the other list the focus moves there (message 8) while nothing is picked; rows move the
/// cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    // The modern layer draws it and takes its mouse.
    if w.modern {
        return;
    }
    let Some(s) = w.bluset.clone().filter(|s| s.sibo.is_none()) else { return };
    let Some((f, k)) = hit(gu, sw, sh, p) else { return };
    gu.hover.set(true);
    if f != s.focus {
        if moved && s.spell_pick.is_none() && s.slot_pick.is_none() {
            num_plus(w);
        }
        return;
    }
    let n = if f == Focus::Equip { s.slots } else { inven_rows(w, &s).len() };
    let c = if f == Focus::Equip { s.equip } else { s.inven };
    let row = c.1 + k;
    if row >= n {
        return;
    }
    if row != c.0 && (moved || clicked) {
        if let Some(b) = w.bluset.as_mut() {
            if f == Focus::Equip { b.equip.0 = row } else { b.inven.0 = row }
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter);
    }
}

/// The scroll bar or wheel on the focused list.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(s) = w.bluset.as_ref() else { return false };
    let n = if s.focus == Focus::Equip { s.slots } else { inven_rows(w, s).len() };
    let s = w.bluset.as_mut().unwrap();
    let c = if s.focus == Focus::Equip { &mut s.equip } else { &mut s.inven };
    let mut row = c.0.saturating_sub(c.1);
    let moved = super::scrollbar::paged(m, &mut c.1, &mut row, n, VISIBLE);
    c.0 = c.1 + row;
    moved
}

/// Help slot 1 the focused row's help, title "Blue Magic" (0xd 0x8d).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(s) = &w.bluset else { return };
    if s.sibo.is_some() {
        let [help, title] = gu.focus_text(SIBO, 0);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        return;
    }
    bars.title[1] = gu.title_msg(0x8d).unwrap_or_default();
    bars.help[1] = match s.focus {
        Focus::Equip => gu.menu_msg(0x28b, ""),
        Focus::Inven => match inven_rows(w, s).get(s.inven.0) {
            Some(Row::Type) => gu.menu_msg(0x28a, ""),
            Some(Row::RemoveAll) => gu.lobby_msg(0xc, 0x54, ""),
            _ => String::new(),
        },
    };
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// The spell's standard icon (record +0x40, never the HD one).
fn icon(gu: &mut GameUi, w: &World, id: u16, x: f32, y: f32) {
    if let Some(r) = w.actions.as_ref().and_then(|a| a.spell(id)) {
        gu.sprite("menu    magiconw", r.icon as usize, x, y, WHITE);
    }
}

fn spell_name(w: &World, id: u16) -> String {
    w.names.as_ref().and_then(|n| n.spell(id as u32)).unwrap_or_default()
}

/// A row list window: its frame and the hand on the cursor row while focused; rows' own sprites are not drawn.
fn list_window(gu: &mut GameUi, name: &str, sw: f32, sh: f32, c: (usize, usize), focused: bool) -> Option<([f32; 4], (f32, f32))> {
    let rows: Vec<u16> = (1..=VISIBLE as u16).collect();
    let win = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), None, false, &rows)?;
    let (x, y) = gu.window_xy(name, sw, sh)?;
    if focused {
        gu.cursor(x + 3.0, y + 6.0 + 18.0 * (c.0 - c.1) as f32);
    }
    Some((win.frame, (x + 3.0, y + 4.0)))
}

/// bluehelp's texts: the name, "(family - type) " (group 10 0x54), the description, "Lv.%d  MP cost %d  Blue Magic
/// Points %d" (0x55), both through sprintf.
fn help_text(gu: &GameUi, w: &World, id: u16) -> Option<[String; 4]> {
    let d = data(w)?;
    let e = d.entry(id)?;
    let fmt = |id: usize, args: &[String]| super::textcmd::printf(&w.battle.as_ref().and_then(|b| b.group10(id)).unwrap_or_default(), args);
    let head = fmt(0x54, &[gu.lobby_msg(0xc, 0x3e + e.family as usize, ""), gu.lobby_msg(0xc, 0x52 + e.physical as usize, "")]);
    let mp = w.actions.as_ref().and_then(|a| a.spell(id)).map_or(0, |r| r.mp as i32);
    let foot = fmt(0x55, &[spell_level(w, id).to_string(), mp.to_string(), e.points.to_string()]);
    Some([spell_name(w, id), head, d.description(e), foot])
}

/// bluehelp (FUN_101ec500 with LAB_101dfc80 / LAB_101dfd60): the name and "(family - type) " on one line, the
/// description from (16, 20), the level, MP cost and points below; at least 56 high, growing down.
fn help(gu: &mut GameUi, w: &World, sw: f32, sh: f32, id: u16) -> Option<[f32; 4]> {
    let [name, head, desc, foot] = help_text(gu, w, id)?;
    let [x, y, fw, fh] = gu.window_rect(HELP, sw, sh)?;
    let lines: Vec<String> = desc.split('\n').flat_map(|l| gu.wrap(l, fw - 20.0)).collect();
    let h = (16.0 * lines.len() as f32 + 40.0).max(fh);
    if !gu.window_sized(HELP, x, y, fw, h) {
        return None;
    }
    text(gu, &name, x + 4.0, y + 4.0, WHITE);
    let hw = gu.text_width(&head);
    text(gu, &head, x + fw - 4.0 - hw, y + 4.0, WHITE);
    for (i, l) in lines.iter().enumerate() {
        text(gu, l, x + 16.0, y + 20.0 + 16.0 * i as f32, WHITE);
    }
    text(gu, &foot, x + 16.0, y + 20.0 + 16.0 * lines.len() as f32, WHITE);
    Some([x, y, fw, h])
}

/// A list row as the modern layer draws it: the slot number in its band's colour (bluequip), the spell's icon, the
/// text in its tint.
pub(super) struct RowLook {
    pub number: Option<(usize, u32)>,
    pub icon: Option<usize>,
    pub text: String,
    pub tint: u32,
}

/// The Set Spells screen as the modern layer draws it.
pub(super) struct Look {
    pub used: u32,
    pub total: u32,
    pub equip: (Vec<RowLook>, usize),
    pub inven: (Vec<RowLook>, usize),
    /// bluequip has the keys (else bluinven); a slot or a spell is picked.
    pub on_equip: bool,
    pub picking: bool,
    pub help: Option<[String; 4]>,
    /// bluesibo's families and cursor row (1..18) while it is open.
    pub sibo: Option<(Vec<String>, u16)>,
}

fn spell_icon(w: &World, id: u16) -> Option<usize> {
    w.actions.as_ref().and_then(|a| a.spell(id)).map(|r| r.icon as usize)
}

pub(super) fn look(w: &World, gu: &GameUi) -> Option<Look> {
    let s = w.bluset.as_ref()?;
    let bands = band_colours(w, s);
    let equip = (0..s.slots)
        .map(|i| {
            let idx = slot(w, s.side, i);
            let id = (idx != 0).then_some(0x200 + idx as u16);
            let tint = if s.slot_pick == Some(i) { PICKED } else { WHITE };
            RowLook { number: Some((i + 1, bands[i])), icon: id.and_then(|id| spell_icon(w, id)), text: id.map(|id| spell_name(w, id)).unwrap_or_default(), tint }
        })
        .collect();
    let rows = inven_rows(w, s);
    let inven = rows
        .iter()
        .map(|r| match *r {
            Row::Type => RowLook { number: None, icon: None, text: gu.sprite_caption("menu    framesus", 130).unwrap_or_else(|| "Type".into()), tint: WHITE },
            Row::RemoveAll => RowLook { number: None, icon: None, text: w.battle.as_ref().and_then(|b| b.group9(0xe3)).unwrap_or_default(), tint: 0x8090_9040 },
            Row::Spell(id) => RowLook { number: None, icon: spell_icon(w, id), text: spell_name(w, id), tint: spell_tint(w, s, id) },
        })
        .collect();
    let shown = match s.focus {
        Focus::Equip => Some(slot(w, s.side, s.equip.0)).filter(|&i| i != 0).map(|i| 0x200 + i as u16),
        Focus::Inven => match rows.get(s.inven.0) {
            Some(&Row::Spell(id)) => Some(id),
            _ => None,
        },
    };
    // bluesibo's rows as its layout draws them (their sprites' captions).
    let sibo = s.sibo.map(|r| ((1..=SIBO_ROWS).map(|k| gu.child_caption(SIBO, k).unwrap_or_default()).collect(), r));
    Some(Look {
        used: used(w, s),
        total: s.total,
        equip: (equip, s.equip.0),
        inven: (inven, s.inven.0),
        on_equip: s.focus == Focus::Equip,
        picking: s.spell_pick.is_some() || s.slot_pick.is_some(),
        help: shown.and_then(|id| help_text(gu, w, id)),
        sibo,
    })
}

/// The mouse's cursor moves: a slot (bluequip), a row of bluinven, bluesibo's row; with nothing picked a row of the
/// other list takes the focus first (as Num+). True when the cursor moved.
pub(super) fn set_row(w: &mut World, equip: bool, k: usize) -> bool {
    let Some(s) = w.bluset.as_ref().filter(|s| s.sibo.is_none()) else { return false };
    if (s.focus == Focus::Equip) != equip {
        if s.spell_pick.is_some() || s.slot_pick.is_some() {
            return false;
        }
        num_plus(w);
    }
    let n = match w.bluset.as_ref() {
        Some(s) if equip => s.slots,
        Some(s) => inven_rows(w, s).len(),
        None => 0,
    };
    let Some(s) = w.bluset.as_mut() else { return false };
    let c = if equip { &mut s.equip } else { &mut s.inven };
    if c.0 == k || k >= n {
        return false;
    }
    c.0 = k;
    c.1 = c.1.min(k).max((k + 1).saturating_sub(VISIBLE));
    true
}

pub(super) fn set_sibo(w: &mut World, r: u16) -> bool {
    let Some(cur) = w.bluset.as_mut().and_then(|s| s.sibo.as_mut()).filter(|c| **c != r && (1..=SIBO_ROWS).contains(&r)) else { return false };
    *cur = r;
    true
}

/// The screen: persona, bluepoin, bluequip, bluinven, bluehelp, bluesibo (drawn after the magic list so the window
/// walk suspends it, blusortw and mgcmenu).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(s) = &w.bluset else { return rects };
    let free = w.confirm.is_none() && s.sibo.is_none();
    // persona as on the Equipment screen.
    let persona = super::items::persona(w);
    if gu.gate_layout(PERSONA, sw, sh, None) {
        let (b, ui, style, _) = gu.parts();
        if let Some(m) = ui.menu(PERSONA) {
            crate::render::invwin::push_persona(b, ui, style, m, &persona, sw, sh);
        }
    }
    rects.extend(gu.window_rect(PERSONA, sw, sh));
    // bluepoin (FUN_101dfbb0): used / total right-aligned at (x1 - 8, 0x1f), blue once full.
    let used = used(w, s);
    if let Some(win) = gu.lobby_window_off(POINTS, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, fw, _] = win.frame;
        gu.shape_text(&format!("{used}/{}", s.total), x + fw - 8.0, y + 31.0, if used < s.total { WHITE } else { 0x8020_60a0 }, true);
        rects.push(win.frame);
    }
    // bluequip: the slot number (keytops3 400 + i) in its band's colour, the icon, the name.
    let bands = band_colours(w, s);
    if let Some((frame, (ox, oy))) = list_window(gu, EQUIP, sw, sh, s.equip, free && s.focus == Focus::Equip) {
        for k in 0..VISIBLE.min(s.slots.saturating_sub(s.equip.1)) {
            let i = s.equip.1 + k;
            let y = oy + 18.0 * k as f32;
            gu.sprite("menu    keytops3", 400 + i, ox, y + 3.0, bands[i]);
            let idx = slot(w, s.side, i);
            if idx != 0 {
                let id = 0x200 + idx as u16;
                icon(gu, w, id, ox + 18.0, y);
                text(gu, &spell_name(w, id), ox + 36.0, y + 2.0, if s.slot_pick == Some(i) { PICKED } else { WHITE });
            }
        }
        gu.scroll_bar(frame[0] + frame[2] - 8.0, frame[1], frame[3], s.slots, VISIBLE, s.equip.1);
        if s.focus == Focus::Equip {
            gu.bar_site(super::scrollbar::Owner::BlueSet, frame[0] + frame[2] - 8.0, frame[1], frame[3], s.slots, VISIBLE, s.equip.1);
        }
        rects.push(frame);
    }
    // bluinven: Type, Remove All, the spells.
    let rows = inven_rows(w, s);
    if let Some((frame, (ox, oy))) = list_window(gu, INVEN, sw, sh, s.inven, free && s.focus == Focus::Inven) {
        for (k, r) in rows.iter().skip(s.inven.1).take(VISIBLE).enumerate() {
            let y = oy + 18.0 * k as f32;
            match *r {
                Row::Type => {
                    gu.sprite("menu    framesus", 130, ox, y, WHITE);
                }
                Row::RemoveAll => text(gu, &w.battle.as_ref().and_then(|b| b.group9(0xe3)).unwrap_or_default(), ox + 18.0, y + 2.0, 0x8090_9040),
                Row::Spell(id) => {
                    icon(gu, w, id, ox, y);
                    text(gu, &spell_name(w, id), ox + 18.0, y + 2.0, spell_tint(w, s, id));
                }
            }
        }
        gu.scroll_bar(frame[0] + frame[2] - 8.0, frame[1], frame[3], rows.len(), VISIBLE, s.inven.1);
        if s.focus == Focus::Inven {
            gu.bar_site(super::scrollbar::Owner::BlueSet, frame[0] + frame[2] - 8.0, frame[1], frame[3], rows.len(), VISIBLE, s.inven.1);
        }
        rects.push(frame);
    }
    // bluehelp for the focused list's cursor row when it holds a spell.
    let shown = match s.focus {
        Focus::Equip => Some(slot(w, s.side, s.equip.0)).filter(|&i| i != 0).map(|i| 0x200 + i as u16),
        Focus::Inven => match rows.get(s.inven.0) {
            Some(&Row::Spell(id)) => Some(id),
            _ => None,
        },
    };
    match shown.and_then(|id| help(gu, w, sw, sh, id)) {
        Some(r) => rects.push(r),
        None => {
            if let Some(win) = gu.lobby_window_off(HELP, sw, sh, (0.0, 0.0), None, false, &[]) {
                rects.push(win.frame);
            }
        }
    }
    if let Some(r) = s.sibo
        && let Some(win) = gu.lobby_window_off(SIBO, sw, sh, (0.0, 0.0), Some(r), w.confirm.is_none(), &[19])
    {
        rects.push(win.frame);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_steps() {
        let mut c = (0, 0);
        assert!(!step(&mut c, 3, egui::Key::ArrowUp));
        assert!(step(&mut c, 3, egui::Key::ArrowRight));
        assert_eq!(c, (2, 0));
        let mut c = (9, 0);
        assert!(step(&mut c, 25, egui::Key::ArrowDown));
        assert_eq!(c, (10, 1));
        assert!(step(&mut c, 25, egui::Key::ArrowRight));
        assert_eq!(c, (20, 11));
    }
}
