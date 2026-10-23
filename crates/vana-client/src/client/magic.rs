//! The magic and ability lists ("menu    magic" / "menu    ability", FUN_1022b220 / FUN_10222b90), the category
//! windows that open them (mgcmenu, magselec, abimenu, abiselec), the cost box (subwindo) and the client checks run
//! before the target prompt (FUN_1022cf30 / FUN_10224b70). MODLOG "Magic and ability menus".

use super::World;
use super::game_ui::GameUi;
use super::target::{self, SubThen};
use crate::audio::Ui;
use crate::dat::actions::RANGES;
use crate::dat::names::NameSource;
use crate::render::invwin::{ActionRow, push_action_list, push_category, push_cost_box};
use std::time::Instant;

/// Rows shown at once (children 1..12).
pub const VISIBLE: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Magic,
    Ability,
}

/// An open list.
#[derive(Clone, Debug)]
pub struct ListWin {
    pub kind: Kind,
    /// Spell type (record +2) or ability type; 0 = every one.
    pub ty: u16,
    /// Magic: spell types dropped by bit (0x100 drops trusts).
    pub exclude: u32,
    /// Magic: list mode (every known spell of the type) or cast mode (castable ones only).
    pub list_mode: bool,
    pub rows: Vec<u16>,
    pub scroll: usize,
    pub cursor: usize,
    /// While the target prompt is up (FUN_1015e6e0 hides, the cancel callback shows it again).
    pub hidden: bool,
}

impl ListWin {
    pub fn menu(&self) -> &'static str {
        match self.kind {
            Kind::Magic => "menu    magic",
            Kind::Ability => "menu    ability",
        }
    }

    fn at(&self) -> Option<u16> {
        self.rows.get(self.scroll + self.cursor).copied()
    }
}

/// What a category row opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cat {
    Magic(u16, bool),
    Ability(u16),
    Ranged,
    Mount,
}

/// An open category window and its cursor (index into its rows, top to bottom).
#[derive(Clone, Copy, Debug)]
pub struct Cats {
    pub menu: &'static str,
    pub sel: usize,
}

/// Rows (child id, opens) top to bottom: mgcmenu FUN_1013f750 (list mode), magselec FUN_1013f680 (cast mode, no
/// Trust), abimenu FUN_1013fa70, abiselec FUN_1013f7f0 (Mount sits above Pet in both).
fn cat_rows(menu: &str) -> &'static [(u16, Cat)] {
    match menu {
        "menu    mgcmenu" => &[(1, Cat::Magic(1, true)), (2, Cat::Magic(2, true)), (3, Cat::Magic(5, true)), (4, Cat::Magic(4, true)), (5, Cat::Magic(3, true)), (6, Cat::Magic(6, true)), (7, Cat::Magic(7, true)), (8, Cat::Magic(8, true))],
        "menu    magselec" => &[(1, Cat::Magic(1, false)), (2, Cat::Magic(2, false)), (3, Cat::Magic(5, false)), (4, Cat::Magic(4, false)), (5, Cat::Magic(3, false)), (6, Cat::Magic(6, false)), (7, Cat::Magic(7, false))],
        "menu    abimenu" => &[(1, Cat::Ability(4)), (2, Cat::Ability(1)), (3, Cat::Ability(3)), (4, Cat::Ranged), (6, Cat::Mount), (5, Cat::Ability(2))],
        "menu    abiselec" => &[(1, Cat::Ability(1)), (2, Cat::Ability(3)), (3, Cat::Ranged), (5, Cat::Mount), (4, Cat::Ability(2))],
        _ => &[],
    }
}

/// Table 0x10386159 by ability type: (the type Esc goes back to, the kind: subwindo mode and cost check).
const ABILITY_TYPES: [(u8, u8); 24] = [(0, 2), (0, 2), (0, 5), (0, 4), (0, 0), (0, 0), (2, 6), (0, 0), (1, 2), (1, 8), (2, 6), (1, 3), (1, 3), (1, 3), (1, 2), (1, 8), (1, 3), (1, 2), (2, 8), (1, 2), (0x14, 9), (1, 2), (1, 2), (1, 2)];

fn ability_kind(ty: u8) -> u8 {
    ABILITY_TYPES.get(ty as usize).map_or(0, |t| t.1)
}

fn bit(bits: &[u8], n: usize) -> bool {
    bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

/// Seconds left on a spell's recast (DAT_106671e0).
pub fn spell_recast(w: &World, s: u16) -> f32 {
    w.spell_recast.get(&s).map_or(0.0, |t| t.saturating_duration_since(Instant::now()).as_secs_f32())
}

/// Seconds left on an ability's 0x119 timer (FUN_10225120; timer 900 none).
fn ability_recast(w: &World, timer: u16) -> f32 {
    w.ability_recast.get(&timer).map_or(0.0, |t| t.saturating_duration_since(Instant::now()).as_secs_f32())
}

/// FUN_1022b330: our cast finished (0x028 category 4): the recast from the header's info, else the DAT's * 15 ticks.
pub fn set_recast(w: &mut World, spell: u16, info: u32) {
    let Some(r) = w.actions.as_ref().and_then(|a| a.spell(spell)) else { return };
    let secs = if info == 0 { r.recast as f32 * 15.0 / 60.0 } else { info as f32 };
    w.spell_recast.insert(spell, Instant::now() + std::time::Duration::from_secs_f32(secs));
}

/// FUN_1022c3a0's rows: known spells (0x0AA) in id order (ours has no mix.dat order yet: retail's order with no
/// file), by type and exclusion; cast mode needs 0x119 seen, the job / level rule, SCH / GEO main for flags 8 /
/// 0x20, and leaves out blue magic (needs the set spells, s2c 0x044, not read yet).
fn magic_rows(w: &World, ty: u16, list_mode: bool, exclude: u32) -> Vec<u16> {
    let (Some(acts), Some(st)) = (w.actions.as_ref(), w.stats.as_ref()) else { return Vec::new() };
    (1..1024u16)
        .filter(|&s| bit(&w.spells_known, s as usize))
        .filter(|&s| {
            let Some(r) = acts.spell(s) else { return false };
            let shown = exclude & (1 << (r.kind & 0x1f)) == 0 && (ty == 0 || r.kind == ty);
            let castable = w.recasts_seen
                && acts.can_cast(s, st.main, st.sub, 0)
                && !(0x200..0x2e0).contains(&s)
                && (r.flags & 8 == 0 || st.main.0 == 20)
                && (r.flags & 0x20 == 0 || st.main.0 == 21);
            shown && (list_mode || castable)
        })
        .collect()
}

/// FUN_10223ec0's rows: known abilities (0x0AC bits by table index) of the type; pet commands only with a pet.
fn ability_rows(w: &World, ty: u16) -> Vec<u16> {
    let Some(acts) = w.actions.as_ref() else { return Vec::new() };
    if ty == 2 && w.my_pet == 0 {
        return Vec::new();
    }
    (0..0x700u16).filter(|&i| bit(&w.commands_known, i as usize)).filter(|&i| acts.abilities.get(i as usize).is_some_and(|a| a.id != 0 && (ty == 0 || a.kind as u16 == ty))).collect()
}

/// FUN_1022b220: the magic list, built and opened.
pub fn open_magic(w: &mut World, ty: u16, list_mode: bool, exclude: u32) {
    let rows = magic_rows(w, ty, list_mode, exclude);
    w.magic = Some(ListWin { kind: Kind::Magic, ty, exclude, list_mode, rows, scroll: 0, cursor: 0, hidden: false });
    w.menu = None;
    w.action_menu = None;
    w.cats = None;
}

/// FUN_10222b90: the ability list of a type.
pub fn open_ability(w: &mut World, ty: u16) {
    let rows = ability_rows(w, ty);
    w.ability = Some(ListWin { kind: Kind::Ability, ty, exclude: 0, list_mode: false, rows, scroll: 0, cursor: 0, hidden: false });
    w.menu = None;
    w.action_menu = None;
    w.cats = None;
}

pub fn open_cats(w: &mut World, menu: &'static str) {
    w.cats = Some(Cats { menu, sel: 0 });
    w.menu = None;
    w.action_menu = None;
}

/// A status check shared by both lists: fishing, mounted, healing, sitting, a place without actions.
fn status_check(w: &World) -> Option<usize> {
    let s = w.my_motion.status;
    match s {
        6 => Some(0x4d),
        5 | 0x55 => Some(0x4b),
        0x21 | 7 => Some(0x51),
        0x2f | 0x3f..=0x53 => Some(0xa0),
        _ if w.room_flag => Some(0x44),
        _ => None,
    }
}

/// FUN_1022cf30: the group 7 line refusing a spell, or None.
fn magic_check(w: &World, s: u16) -> Option<usize> {
    if let Some(m) = status_check(w) {
        return Some(m);
    }
    if spell_recast(w, s) > 0.0 {
        return Some(if (0x380..0x400).contains(&s) { 0xbf } else { 0x59 });
    }
    let (Some(acts), Some(st)) = (w.actions.as_ref(), w.stats.as_ref()) else { return Some(0x57) };
    if !w.recasts_seen || !acts.can_cast(s, st.main, st.sub, 0) {
        return Some(0x57);
    }
    let mp = w.vitals.map_or(0, |v| v.1);
    if acts.spell(s).is_some_and(|r| mp < r.mp as u32) {
        return Some(0x58);
    }
    None
}

/// FUN_10224b70: the line refusing an ability, or None (traits: "Job traits are always active.").
fn ability_check(w: &World, i: u16) -> Option<usize> {
    let a = w.actions.as_ref()?.abilities.get(i as usize)?.clone();
    if a.kind == 4 {
        return Some(0x5e);
    }
    if let Some(m) = status_check(w) {
        return Some(m);
    }
    let tp = w.vitals.map_or(0, |v| v.2);
    match ability_kind(a.kind) {
        2 | 3 | 5 if a.timer != 900 && ability_recast(w, a.timer) > 0.0 => Some(0x5c),
        3 if a.tp != 255 && tp < a.tp as u32 => Some(0x5f),
        4 if w.my_motion.status != 1 => Some(0x52),
        4 if tp < 1000 => Some(0x5f),
        6 if w.vitals.is_some_and(|v| v.1 < a.cost as u32) => Some(0x58),
        _ => None,
    }
}

/// FUN_10220d40: the HD icon unless config 0x99 bit 2 (standard icons, conf3win 12 / 13) or it is 0xffff;
/// kind 6 ("menu    magico2w") from 1000, else kind 5 ("menu    magiconw").
fn list_icon(icon: u16, hd: u16, standard: bool) -> (&'static str, usize) {
    match hd {
        0xffff => ("menu    magiconw", icon as usize),
        _ if standard => ("menu    magiconw", icon as usize),
        1000.. => ("menu    magico2w", (hd - 1000) as usize),
        _ => ("menu    magiconw", hd as usize),
    }
}

fn row_icon(w: &World, kind: Kind, id: u16) -> Option<(&'static str, usize)> {
    let standard = w.game.get(0x99) & 4 != 0;
    let acts = w.actions.as_ref()?;
    Some(match kind {
        Kind::Magic => acts.spell(id).map(|r| list_icon(r.icon, r.icon_hd, standard))?,
        Kind::Ability => acts.abilities.get(id as usize).map(|a| list_icon(a.icon as u16, a.icon_hd, standard))?,
    })
}

fn spell_name(w: &World, s: u16) -> String {
    w.names.as_ref().and_then(|n| n.spell(s as u32)).unwrap_or_default()
}

fn ability_name(w: &World, i: u16) -> String {
    w.names.as_ref().and_then(|n| n.ability(i as u32)).unwrap_or_default()
}

/// A group 7 line (mode 0x7a, the magic / ability ones) and the error SE.
fn refuse(w: &mut World, id: usize) {
    if let Some(bt) = w.battle.clone()
        && let Some(line) = bt.group7(id).filter(|l| !l.is_empty())
    {
        let colour = bt.colour_in(0x7a, &w.game);
        w.chat.push_colour(line, colour);
    }
    w.sounds.push(Ui::Error as u32);
}

/// FUN_1022c7a0 / FUN_102241a0: Enter on a row: the checks, then the target prompt for the spell's or ability's
/// targets; a row naming a sub-list opens that list instead.
fn enter(w: &mut World, kind: Kind) {
    let Some(win) = (match kind {
        Kind::Magic => w.magic.as_ref(),
        Kind::Ability => w.ability.as_ref(),
    }) else {
        return;
    };
    let Some(id) = win.at() else { return };
    match kind {
        Kind::Magic => {
            if let Some(m) = magic_check(w, id) {
                refuse(w, m);
                return;
            }
            let Some(r) = w.actions.as_ref().and_then(|a| a.spell(id)).cloned() else { return };
            let name = spell_name(w, id);
            let range = Some(RANGES[(r.range & 0xf) as usize] as f32);
            let help = 0x4f;
            if !target::open_sub(w, r.targets as u32, SubThen::Magic(name.clone()), range, name, help) {
                refuse(w, 0x16);
                return;
            }
            if let Some(m) = w.magic.as_mut() {
                m.hidden = true;
            }
        }
        Kind::Ability => {
            let Some(a) = w.actions.as_ref().and_then(|x| x.abilities.get(id as usize)).cloned() else { return };
            if a.sub != 0 {
                open_ability(w, a.sub as u16);
                return;
            }
            if let Some(m) = ability_check(w, id) {
                refuse(w, m);
                return;
            }
            let name = ability_name(w, id);
            let cmd = if a.kind == 3 { "/weaponskill" } else if a.kind == 0x14 { "/monsterskill" } else { "/jobability" };
            if !target::open_sub(w, a.targets as u32, SubThen::Ability(cmd, name.clone()), None, name, 0x51) {
                refuse(w, 0x1b);
                return;
            }
            if let Some(m) = w.ability.as_mut() {
                m.hidden = true;
            }
        }
    }
    w.sounds.push(Ui::Select as u32);
}

/// FUN_10222150: Down / Up step and wrap, Right / Left page by 12.
fn step(win: &mut ListWin, key: egui::Key) -> bool {
    let n = win.rows.len();
    if n == 0 {
        return false;
    }
    let mut at = win.scroll + win.cursor;
    match key {
        egui::Key::ArrowDown => at = (at + 1) % n,
        egui::Key::ArrowUp => at = (at + n - 1) % n,
        egui::Key::ArrowRight => at = (at + VISIBLE).min(n - 1),
        egui::Key::ArrowLeft => at = at.saturating_sub(VISIBLE),
        _ => return false,
    }
    if at < win.scroll {
        win.scroll = at;
    } else if at >= win.scroll + VISIBLE {
        win.scroll = at + 1 - VISIBLE;
    }
    win.cursor = at - win.scroll;
    true
}

/// Keys while a list or category window is open (the prompt takes them first). True when used.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    if w.prompt.is_some() {
        return false;
    }
    if let Some(c) = w.cats {
        let rows = cat_rows(c.menu);
        let n = rows.len().max(1);
        match key {
            egui::Key::ArrowDown => w.cats = Some(Cats { sel: (c.sel + 1) % n, ..c }),
            egui::Key::ArrowUp => w.cats = Some(Cats { sel: (c.sel + n - 1) % n, ..c }),
            egui::Key::Escape => {
                w.cats = None;
                w.sounds.push(Ui::Cancel as u32);
                return true;
            }
            egui::Key::Enter => {
                match rows.get(c.sel).map(|r| r.1) {
                    Some(Cat::Magic(ty, list)) => open_magic(w, ty, list, 0),
                    Some(Cat::Ability(ty)) => open_ability(w, ty),
                    // FUN_1013f880: Ranged Attack through the prompt for enemies, as `/ra`.
                    Some(Cat::Ranged) => {
                        w.cats = None;
                        if !target::open_sub(w, 0x20, SubThen::Line { stash: "/range ".into(), quoted: false, from_macro: false }, None, String::new(), 0) {
                            refuse(w, 0x1b);
                        }
                    }
                    Some(Cat::Mount) => log::info!("the mount list is not implemented yet"),
                    None => {}
                }
                w.sounds.push(Ui::Select as u32);
                return true;
            }
            _ => return true,
        }
        w.sounds.push(Ui::Cursor as u32);
        return true;
    }
    let kind = if w.ability.is_some() { Kind::Ability } else if w.magic.is_some() { Kind::Magic } else { return false };
    match key {
        egui::Key::Enter => enter(w, kind),
        egui::Key::Escape => {
            // Esc on an ability sub-list goes back to its parent type (not Monstrosity's).
            let parent = w.ability.as_ref().filter(|_| kind == Kind::Ability).map(|a| ABILITY_TYPES.get(a.ty as usize).map_or(0, |t| t.0)).filter(|&p| p != 0 && p != 0x14);
            match kind {
                Kind::Magic => w.magic = None,
                Kind::Ability => w.ability = None,
            }
            if let Some(p) = parent {
                open_ability(w, p as u16);
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        k => {
            let win = match kind {
                Kind::Magic => w.magic.as_mut(),
                Kind::Ability => w.ability.as_mut(),
            };
            if win.is_some_and(|win| step(win, k)) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
    true
}

/// FUN_1022b3d0 -> FUN_1012c1d0(0, record +0xc * 20): the bar runs for the cast byte times 20 ticks (Cure's 8: 2.67 s,
/// where LSB casts it in 2 s; retail's bar can end late). Not done: none under Chainspell (status 48).
pub fn open_cast_bar(w: &mut World, spell: u16) {
    let Some(r) = w.actions.as_ref().and_then(|a| a.spell(spell)) else { return };
    let total = r.cast as f32 * 20.0;
    if total > 0.0 {
        w.cast_bar = Some((total, total));
    }
}

/// FUN_1012bf60 mode 0: casttime at (16, 47) (at its layout's (16, 94) only under gaugewin, not modelled), the HP
/// gauge pair (windowps 38 / 97) at (+8, +9) by the fraction done, "%d" of it right-aligned at +0x6a and '%' at
/// +0x70, y + 7, in the sprite font.
pub fn draw_cast_bar(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    let Some((total, left)) = w.cast_bar else { return };
    let f = ((total - left) / total).clamp(0.0, 1.0);
    let (b, ui, style, _) = gu.parts();
    let (Some(m), Some(wps)) = (ui.menu("menu    casttime"), ui.lib("menu    windowps")) else { return };
    let (x, y) = crate::render::screens::push_window_at(b, ui, style, m, sw, sh, 47.0 - m.elements[0].y as f32);
    crate::render::screens::push_gauge(b, ui, wps, x + 8.0, y + 9.0, 38, 97, f);
    let pct = ((f * 100.0) as u32).to_string();
    crate::render::screens::push_shape_text_right(b, ui, pct.as_bytes(), x + 106.0, y + 7.0, 0x8080_8080);
    crate::render::screens::push_shape_text(b, ui, b"%", x + 112.0, y + 7.0, 0x8080_8080);
}

/// "%d:%02d" (">= 1 h: %d:%02d:%02d", FUN_101c93c0) of the seconds left, rounded up.
fn clock(secs: f32) -> String {
    let s = secs.ceil() as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The open category window, list and cost box. Returns the window rects that take clicks.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let frame = gu.cursor_frame();
    if let Some(c) = w.cats {
        let child = cat_rows(c.menu).get(c.sel).map_or(1, |r| r.0 as usize);
        let (b, ui, style, _) = gu.parts();
        if let Some(m) = ui.menu(c.menu) {
            push_category(b, ui, style, m, child, sw, sh, frame);
        }
        rects.extend(gu.window_rect(c.menu, sw, sh));
    }
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|x| !x.hidden) else { return rects };
    // Colours (table 0x103860fc): 0x80808080 normal, 0x80404040 can't use now, 0x80803040 the cursor row.
    let rows: Vec<ActionRow> = win
        .rows
        .iter()
        .skip(win.scroll)
        .take(VISIBLE)
        .enumerate()
        .map(|(k, &id)| {
            let (name, refused) = match win.kind {
                Kind::Magic => (spell_name(w, id), magic_check(w, id).is_some()),
                Kind::Ability => (ability_name(w, id), w.actions.as_ref().and_then(|a| a.abilities.get(id as usize)).is_some_and(|a| a.kind != 4) && ability_check(w, id).is_some()),
            };
            let tint = if k == win.cursor { 0x8080_3040 } else if refused { 0x8040_4040 } else { 0x8080_8080 };
            ActionRow { icon: row_icon(w, win.kind, id), name, tint }
        })
        .collect();
    // Empty: group 9 0x4a "No spells learned." (list mode) / 0x4b "No spells available.".
    let empty = match (win.kind, win.list_mode) {
        (Kind::Magic, true) => gu.lobby_msg(9, 0x4a, ""),
        _ => gu.lobby_msg(9, 0x4b, ""),
    };
    let menu = win.menu();
    let (b, ui, style, font) = gu.parts();
    if let Some(m) = ui.menu(menu) {
        push_action_list(b, ui, style, font, GameUi::ATLAS, m, &rows, win.cursor, &empty, sw, sh, frame);
    }
    rects.extend(gu.window_rect(menu, sw, sh));
    // subwindo for the cursor row: magic "MP Cost" (9 0xe6) and "Next" (9 0xe7); abilities by their kind.
    let lines: Vec<(String, String)> = match (win.kind, win.at()) {
        (Kind::Magic, Some(s)) => {
            let mp = w.actions.as_ref().and_then(|a| a.spell(s)).map_or(0, |r| r.mp);
            vec![(gu.lobby_msg(9, 0xe6, "MP Cost"), mp.to_string()), (gu.lobby_msg(9, 0xe7, "Next"), clock(spell_recast(w, s)))]
        }
        (Kind::Ability, Some(i)) => match w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)) {
            Some(a) => {
                let next = (gu.lobby_msg(9, 0xe7, "Next"), clock(if a.timer == 900 { 0.0 } else { ability_recast(w, a.timer) }));
                match ability_kind(a.kind) {
                    2 | 5 | 8 | 10 => vec![next],
                    3 => vec![(gu.lobby_msg(0x0b, 0xbd, "TP Cost"), a.tp.to_string()), next],
                    4 => vec![("TP".into(), w.vitals.map_or(0, |v| v.2).to_string())],
                    6 => vec![(gu.lobby_msg(9, 0xe6, "MP Cost"), a.cost.to_string())],
                    _ => Vec::new(),
                }
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    if !lines.is_empty() {
        let (b, ui, style, font) = gu.parts();
        if let Some(m) = ui.menu("menu    subwindo") {
            push_cost_box(b, ui, style, font, GameUi::ATLAS, m, &lines, sw, sh);
        }
    }
    rects
}

/// The help and title bars: the list's title (group 0xd 76 "Magic List" / 68 "Abilities") and the cursor row's
/// description; while the prompt is up "Select target of spell." / "...ability." and the name in slot 2.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if let Some(p) = w.prompt.as_ref().filter(|p| p.help != 0) {
        bars.help[2] = gu.menu_msg(p.help as usize, "");
        bars.title[2] = p.title.clone();
        return;
    }
    if let Some(c) = w.cats {
        let child = cat_rows(c.menu).get(c.sel).map_or(1, |r| r.0);
        let [help, title] = gu.focus_text(c.menu, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|x| !x.hidden) else { return };
    bars.title[1] = gu.title_msg(if win.kind == Kind::Magic { 76 } else { 68 }).unwrap_or_default();
    bars.help[1] = match (win.kind, win.at(), w.names.as_ref()) {
        (Kind::Magic, Some(s), Some(n)) => n.spell_description(s as u32).unwrap_or_default(),
        (Kind::Ability, Some(i), Some(n)) => n.ability_description(i as u32).unwrap_or_default(),
        _ => String::new(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recast_clock() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(47.2), "0:48");
        assert_eq!(clock(3725.0), "1:02:05");
    }

    #[test]
    fn list_paging() {
        let mut win = ListWin { kind: Kind::Magic, ty: 0, exclude: 0, list_mode: true, rows: (1..=30).collect(), scroll: 0, cursor: 0, hidden: false };
        step(&mut win, egui::Key::ArrowUp);
        assert_eq!((win.scroll, win.cursor, win.at()), (18, 11, Some(30)));
        step(&mut win, egui::Key::ArrowDown);
        assert_eq!(win.at(), Some(1));
        step(&mut win, egui::Key::ArrowRight);
        assert_eq!(win.at(), Some(13));
        step(&mut win, egui::Key::ArrowLeft);
        assert_eq!(win.at(), Some(1));
    }
}
