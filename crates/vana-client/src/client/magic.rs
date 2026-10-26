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
    /// The mount list ("menu    mount", FUN_10226490), kept in `w.ability`.
    Mount,
    /// The emote list ("menu    emote", FUN_10225460), kept in `w.ability`; `ty` = 0x8000 | the open folder.
    Emote,
}

/// A file 96 entry (0x20 bytes): {u16 id, u16 name string, u16 help string, u16 parent (0xffff none), cmd[0x10],
/// arg[8]}; a folder has no command.
#[derive(Clone, Debug)]
pub struct EmoteEntry {
    pub id: u16,
    pub name: u16,
    pub help: u16,
    pub parent: u16,
    pub cmd: String,
    pub arg: String,
}

fn parse_emotes(d: &[u8]) -> Vec<EmoteEntry> {
    let n = d.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize;
    let s = |b: &[u8]| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned();
    (0..n.min(0x200))
        .filter_map(|i| {
            let e = d.get(4 + i * 0x20..4 + i * 0x20 + 0x20)?;
            let u = |o: usize| u16::from_le_bytes([e[o], e[o + 1]]);
            Some(EmoteEntry { id: u(0), name: u(2), help: u(4), parent: u(6), cmd: s(&e[8..0x18]), arg: s(&e[0x18..0x20]) })
        })
        .collect()
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
    /// The sort window beside the list (list +0xc0 / +0xd1): "menu    mgcsortw", "menu    blusortw" or
    /// "menu    abisortw"; `sort_focus` its cursor child while it has the focus, `ask` the Auto-sort sortyn.
    pub sort: Option<&'static str>,
    pub sort_focus: Option<u16>,
    pub ask: Option<bool>,
    /// Manual Sort (+0x6c / +0x68): 0 off, 1 pick, 2 picked (`picked` = its row).
    pub manual: u8,
    pub picked: usize,
}

impl ListWin {
    pub fn menu(&self) -> &'static str {
        match self.kind {
            Kind::Magic => "menu    magic",
            Kind::Ability => "menu    ability",
            Kind::Mount => "menu    mount",
            Kind::Emote => "menu    emote",
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
    Friends,
    Emote,
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
        // FUN_101ebdc0: 1 the Friend List (flistmai, not done), 2 the emote list.
        "menu    commenu" => &[(1, Cat::Friends), (2, Cat::Emote)],
        // Monstrosity: mp_abisc from mp_pmode (Monster Skills, Monster Abilities), mp_mnabi from the main menu (Traits
        // first); FUN_10222b90(type, 0, 1), no abisortw.
        "menu    mp_abisc" => &[(1, Cat::Ability(0x14)), (2, Cat::Ability(1))],
        "menu    mp_mnabi" => &[(1, Cat::Ability(4)), (2, Cat::Ability(0x14)), (3, Cat::Ability(1))],
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

/// The list order (FUN_1022c030 / FUN_10223c90): mix.dat / aix.dat as read at field entry (Macros), merged in place
/// with the known bits (FUN_1021e550; with no file: id order).
fn order(w: &World, aix: bool, known: &[u8]) -> Vec<u16> {
    let n = if aix { 0xb00 } else { 0x400 };
    let mut o = w.macros.as_ref().map(|m| if aix { m.aix.clone() } else { m.mix.clone() }).unwrap_or_else(|| super::macros::Order { bits: vec![0; n / 8], list: vec![0; n], count: 0 });
    o.merge(known);
    o.ids().to_vec()
}

/// The merged order itself, for a sort: merged in place in Macros first.
fn order_mut<'a>(w: &'a mut World, aix: bool) -> Option<&'a mut super::macros::Order> {
    let known = if aix { super::monstrosity::ability_bits(w).unwrap_or_default() } else { w.spells_known.clone() };
    let m = w.macros.as_mut()?;
    let o = if aix { &mut m.aix } else { &mut m.mix };
    o.merge(&known);
    Some(o)
}

/// FUN_1022c3a0's rows: known spells (0x0AA) in the mix.dat order, by type and exclusion; cast mode needs 0x119 seen,
/// the job / level rule, SCH / GEO main for flags 8 / 0x20, and for blue magic 0x200..0x2df a set spell (s2c 0x044).
fn magic_rows(w: &World, ty: u16, list_mode: bool, exclude: u32) -> Vec<u16> {
    let (Some(acts), Some(st)) = (w.actions.as_ref(), w.stats.as_ref()) else { return Vec::new() };
    order(w, false, &w.spells_known)
        .into_iter()
        .filter(|&s| {
            let Some(r) = acts.spell(s) else { return false };
            let shown = exclude & (1 << (r.kind & 0x1f)) == 0 && (ty == 0 || r.kind == ty);
            let castable = w.recasts_seen
                && acts.can_cast(s, st.main, st.sub, 0)
                && (!(0x200..0x2e0).contains(&s) || super::bluset::is_set(w, s))
                && (r.flags & 8 == 0 || st.main.0 == 20)
                && (r.flags & 0x20 == 0 || st.main.0 == 21);
            shown && (list_mode || castable)
        })
        .collect()
}

/// The known blue magic (type 6) in the mix.dat order, for the Set Spells screen's bluinven.
pub(super) fn blue_known(w: &World) -> Vec<u16> {
    let Some(acts) = w.actions.as_ref() else { return Vec::new() };
    order(w, false, &w.spells_known).into_iter().filter(|&s| acts.spell(s).is_some_and(|r| r.kind == 6)).collect()
}

/// FUN_10223ec0's rows: known abilities (0x0AC bits by table index) in the aix.dat order, of the type; pet commands
/// only with a pet.
pub(super) fn ability_rows(w: &World, ty: u16) -> Vec<u16> {
    let Some(acts) = w.actions.as_ref() else { return Vec::new() };
    if ty == 2 && w.my_pet == 0 {
        return Vec::new();
    }
    // FUN_10223c90: no list until FUN_100e6020 has both 0x0AC and the moves. Type 0x14: a move of our form, at our level.
    let Some(known) = super::monstrosity::ability_bits(w) else { return Vec::new() };
    let level = w.stats.as_ref().map_or(0, |s| s.main.1);
    order(w, true, &known)
        .into_iter()
        .filter(|&i| {
            acts.abilities.get(i as usize).is_some_and(|a| {
                a.id != 0 && (ty == 0 || a.kind as u16 == ty) && (ty != 0x14 || (w.mon_in && super::monstrosity::is_move(w, i) && a.level <= level))
            })
        })
        .collect()
}

/// FUN_10221b90: the cursor on the row of the id remembered for this list (scrolled into view, inferred). The mount
/// list remembers a row (+0x68); ours its mount, the same while the set does not change.
fn recall(w: &World, win: &mut ListWin) {
    let key = (win.kind != Kind::Magic, win.list_mode && win.kind == Kind::Magic, win.ty);
    if let Some(at) = w.list_memory.get(&key).and_then(|id| win.rows.iter().position(|r| r == id)) {
        win.scroll = at.saturating_sub(VISIBLE - 1);
        win.cursor = at - win.scroll;
    }
}

/// FUN_1022c990 / FUN_102243d0 store the cursor row's id at close; ours keeps it as the cursor moves.
fn remember(w: &mut World) {
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()) else { return };
    let key = (win.kind != Kind::Magic, win.list_mode && win.kind == Kind::Magic, win.ty);
    if let Some(id) = win.at() {
        w.list_memory.insert(key, id);
    }
}

/// FUN_1022b220: the magic list, built and opened.
pub fn open_magic(w: &mut World, ty: u16, list_mode: bool, exclude: u32) {
    let rows = magic_rows(w, ty, list_mode, exclude);
    let mut win = ListWin { kind: Kind::Magic, ty, exclude, list_mode, rows, scroll: 0, cursor: 0, hidden: false, sort: None, sort_focus: None, ask: None, manual: 0, picked: 0 };
    recall(w, &mut win);
    w.magic = Some(win);
    w.menu = None;
    // An action menu still up is suspended under the list (shared group bit) and comes back when it is done.
    if let Some(m) = w.action_menu.take() {
        w.held_menu = Some(m);
    }
    w.cats = None;
}

/// FUN_10222b90: the ability list of a type; `sortable` attaches abisortw (abimenu's Job Abilities / Weapon Skills,
/// passed on to sub-lists and the Escape back to the parent).
pub fn open_ability(w: &mut World, ty: u16, sortable: bool) {
    let rows = ability_rows(w, ty);
    let sort = sortable.then_some("menu    abisortw");
    let mut win = ListWin { kind: Kind::Ability, ty, exclude: 0, list_mode: false, rows, scroll: 0, cursor: 0, hidden: false, sort, sort_focus: None, ask: None, manual: 0, picked: 0 };
    recall(w, &mut win);
    w.ability = Some(win);
    w.menu = None;
    // An action menu still up is suspended under the list (shared group bit) and comes back when it is done.
    if let Some(m) = w.action_menu.take() {
        w.held_menu = Some(m);
    }
    w.cats = None;
}

/// FUN_10226650: the owned mounts (s2c 0x0AE bits) in moix.dat order; none before the first 0x0AE.
fn mount_rows(w: &World) -> Vec<u16> {
    let Some(bits) = w.mount_bits else { return Vec::new() };
    let order: Vec<u16> = w.macros.as_ref().map_or_else(|| (0..0x40).collect(), |m| m.moix.clone());
    order.into_iter().filter(|&id| id < 64 && bits & (1 << id) != 0).collect()
}

/// FUN_10226490(1, sortable): the mount list (abimenu row 6 attaches abisortw; abiselec row 5 does not), with subwindo
/// in mode 10. Kept in `w.ability` (type 0xfffe for the cursor memory).
pub fn open_mount(w: &mut World, sortable: bool) {
    let rows = mount_rows(w);
    let sort = sortable.then_some("menu    abisortw");
    let mut win = ListWin { kind: Kind::Mount, ty: 0xfffe, exclude: 0, list_mode: false, rows, scroll: 0, cursor: 0, hidden: false, sort, sort_focus: None, ask: None, manual: 0, picked: 0 };
    recall(w, &mut win);
    w.ability = Some(win);
    w.menu = None;
    // An action menu still up is suspended under the list (shared group bit) and comes back when it is done.
    if let Some(m) = w.action_menu.take() {
        w.held_menu = Some(m);
    }
    w.cats = None;
}

/// The emote tree (file 96, loaded once here; retail reads it at every open).
fn emote_tree(w: &mut World) -> Option<std::sync::Arc<Vec<EmoteEntry>>> {
    if w.emote_tree.is_none() {
        let d = w.index.as_ref()?.read(96).ok()?;
        w.emote_tree = Some(std::sync::Arc::new(parse_emotes(&d)));
    }
    w.emote_tree.clone()
}

/// FUN_102256f0: the open folder's entries in eix.dat order.
fn emote_rows(w: &World, folder: u16) -> Vec<u16> {
    let Some(tree) = w.emote_tree.as_ref() else { return Vec::new() };
    let order: Vec<u16> = w.macros.as_ref().map_or_else(|| (0..0x80).collect(), |m| m.eix.clone());
    order
        .into_iter()
        .filter(|&id| id != 0 && tree.iter().find(|e| e.id == id).is_some_and(|e| e.parent != 0xffff && e.parent == folder))
        .collect()
}

/// FUN_10225460: the emote list on the open folder (path), with abisortw; each folder keeps its cursor row.
pub fn open_emote(w: &mut World) {
    if emote_tree(w).is_none() {
        return;
    }
    let folder = w.emote_path.last().copied().unwrap_or(0);
    let rows = emote_rows(w, folder);
    let mut win = ListWin { kind: Kind::Emote, ty: 0x8000 | folder, exclude: 0, list_mode: false, rows, scroll: 0, cursor: 0, hidden: false, sort: Some("menu    abisortw"), sort_focus: None, ask: None, manual: 0, picked: 0 };
    recall(w, &mut win);
    w.ability = Some(win);
    w.menu = None;
    // An action menu still up is suspended under the list (shared group bit) and comes back when it is done.
    if let Some(m) = w.action_menu.take() {
        w.held_menu = Some(m);
    }
    w.cats = None;
}

fn emote_entry(w: &World, id: u16) -> Option<EmoteEntry> {
    w.emote_tree.as_ref()?.iter().find(|e| e.id == id).cloned()
}

/// FUN_102253e0 / FUN_10225430: a job emote (0x4c..0x61) or chair (0x63..0x6e) not unlocked (s2c 0x11A).
fn emote_locked(w: &World, id: u16) -> bool {
    match id {
        0x4c..=0x61 => w.emote_jobs & (1 << (id - 0x4c)) == 0,
        0x63..=0x6e => w.emote_chairs & (1 << (id - 0x63)) == 0,
        _ => false,
    }
}

/// FUN_10226d80: the mount refusals: the status checks, then the shared recast (group 7 0x4c).
fn mount_check(w: &World) -> Option<usize> {
    status_check(w).or_else(|| w.mount_recast.is_some_and(|t| t > Instant::now()).then_some(0x4c))
}

fn mount_name(w: &World, id: u16) -> String {
    w.names.as_ref().and_then(|n| n.mount(id as u32)).map_or_else(String::new, |m| m.0)
}

/// The cursor row's child id (the cursor memory is by child).
pub(super) fn cat_child(c: &Cats) -> u16 {
    cat_rows(c.menu).get(c.sel).map_or(1, |r| r.0)
}

pub fn open_cats(w: &mut World, menu: &'static str) {
    let child = w.cursor_recall(menu);
    let sel = cat_rows(menu).iter().position(|r| r.0 == child).unwrap_or(0);
    w.cats = Some(Cats { menu, sel });
    w.menu = None;
    // An action menu still up is suspended under the list (shared group bit) and comes back when it is done.
    if let Some(m) = w.action_menu.take() {
        w.held_menu = Some(m);
    }
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
    // Blue magic: the reset lockout (0xb7), an unset spell (0xb5); Unbridled spells need Unbridled Learning (485) or
    // Wisdom (505).
    match s {
        0x200..0x2e0 if super::bluset::reset_active(w) => Some(0xb7),
        0x200..0x2e0 if !super::bluset::is_set(w, s) => Some(0xb5),
        0x2e0..0x300 if !w.buffs.iter().any(|&b| b == 485 || b == 505) => Some(0x57),
        _ => None,
    }
}

/// A refusal with no line and no sound.
const SILENT: usize = usize::MAX;

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
        // Kind 9 (Monstrosity): TP below the u16 cost, no 0x119 yet, then a level too low: code 0xd, past table
        // 0x10386124, so nothing is said (FUN_10129d90 drops it).
        9 if tp < a.tp16 as u32 => Some(0x5f),
        9 if !w.recasts_seen => Some(0x5b),
        9 if w.stats.as_ref().map_or(0, |s| s.main.1) < a.level => Some(SILENT),
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
        // FUN_10220fb0: magico2w 63 on every emote row.
        Kind::Emote => ("menu    magico2w", 63),
        // FUN_10220fc0: the 55681 entry's icon field, kind 6 (magico2w 86 / 87), never greyed.
        Kind::Mount => ("menu    magico2w", w.names.as_ref()?.mount(id as u32)?.1 as usize),
    })
}

/// A spell's (or an ability record's) list icon, as the magic and ability lists draw it.
pub fn list_icon_of(w: &World, spell: bool, id: u16) -> Option<(&'static str, usize)> {
    row_icon(w, if spell { Kind::Magic } else { Kind::Ability }, id)
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
        let colour = bt.colour_in(super::textcmd::refusal_mode(id), &w.game);
        w.chat.push_colour(line, colour);
    }
    w.sounds.push(Ui::Error as u32);
}

/// FUN_1022c7a0 / FUN_102241a0: Enter on a row: the checks, then the target prompt for the spell's or ability's
/// targets; a row naming a sub-list opens that list instead.
fn enter(w: &mut World, kind: Kind) {
    let Some(win) = (match kind {
        Kind::Magic => w.magic.as_ref(),
        Kind::Ability | Kind::Mount | Kind::Emote => w.ability.as_ref(),
    }) else {
        return;
    };
    let Some(id) = win.at() else { return };
    match kind {
        // FUN_10225c70: a folder opens; a locked job emote still runs (SE 0004); a locked chair: group 7 0xc2 and
        // SE 0004; else the target prompt (flags 0x7f) and the line run on the pick (LAB_10225c50), SE 0002.
        Kind::Emote => {
            let Some(e) = emote_entry(w, id) else { return };
            let line = format!("/{} {}", e.cmd.trim_start_matches('/'), e.arg);
            if e.cmd.is_empty() {
                w.emote_path.push(id);
                open_emote(w);
            } else if (0x4c..=0x61).contains(&id) && emote_locked(w, id) {
                w.sounds.push(Ui::Error as u32);
                super::textcmd::run(w, &line, super::textcmd::Mode::Internal);
            } else if emote_locked(w, id) {
                refuse(w, 0xc2);
            } else {
                w.sounds.push(Ui::Select as u32);
                target::open_sub(w, 0x7f, SubThen::Command(line), None, String::new(), 0);
            }
            return;
        }
        // FUN_10226950: refused -> the line and SE 0004, the list stays; else `/mount "<name>"`, SE 0002, closed.
        Kind::Mount => {
            if let Some(m) = mount_check(w) {
                refuse(w, m);
                return;
            }
            let line = format!("/mount \"{}\"", mount_name(w, id));
            w.ability = None;
            w.sounds.push(Ui::Select as u32);
            super::textcmd::run(w, &line, super::textcmd::Mode::Internal);
            return;
        }
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
                refuse(w, 0x5a);
                return;
            }
            if let Some(m) = w.magic.as_mut() {
                m.hidden = true;
            }
        }
        Kind::Ability => {
            let Some(a) = w.actions.as_ref().and_then(|x| x.abilities.get(id as usize)).cloned() else { return };
            if a.sub != 0 {
                let sortable = win.sort.is_some();
                open_ability(w, a.sub as u16, sortable);
                return;
            }
            if let Some(m) = ability_check(w, id) {
                if m != SILENT {
                    refuse(w, m);
                }
                return;
            }
            let name = ability_name(w, id);
            let cmd = if a.kind == 3 { "/weaponskill" } else if a.kind == 0x14 { "/monsterskill" } else { "/jobability" };
            if !target::open_sub(w, a.targets as u32, SubThen::Ability(cmd, name.clone()), None, name, 0x51) {
                refuse(w, 0x5d);
                return;
            }
            if let Some(m) = w.ability.as_mut() {
                m.hidden = true;
            }
        }
    }
    w.sounds.push(Ui::Select as u32);
}

/// A scroll bar or wheel message to the open list (FUN_10222150). True when it moved.
pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(win) = w.ability.as_mut().or(w.magic.as_mut()).filter(|x| !x.hidden) else { return false };
    let n = win.rows.len();
    super::scrollbar::paged(m, &mut win.scroll, &mut win.cursor, n, VISIBLE)
}

/// FUN_10222150: Down / Up step and wrap; Right / Left are re-sent as 0x19 / 0x18, a page of the view with the screen
/// row kept.
fn step(win: &mut ListWin, key: egui::Key) -> bool {
    let n = win.rows.len();
    if n == 0 {
        return false;
    }
    let mut at = win.scroll + win.cursor;
    match key {
        egui::Key::ArrowDown => at = (at + 1) % n,
        egui::Key::ArrowUp => at = (at + n - 1) % n,
        egui::Key::ArrowRight | egui::Key::ArrowLeft => {
            let m = if key == egui::Key::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            return super::scrollbar::paged(m, &mut win.scroll, &mut win.cursor, n, VISIBLE);
        }
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
                    // FUN_1022c3a0's tail: list mode (mgcmenu) attaches mgcsortw, blusortw for blue magic.
                    Some(Cat::Magic(ty, list)) => {
                        // FUN_1013f750 in Monstrosity: mgcmenu's rows open cast mode, no mgcsortw.
                        let list = list && !super::monstrosity::active(w);
                        open_magic(w, ty, list, 0);
                        if let Some(m) = w.magic.as_mut().filter(|_| list) {
                            m.sort = Some(if ty == 6 { "menu    blusortw" } else { "menu    mgcsortw" });
                        }
                    }
                    Some(Cat::Ability(ty)) => open_ability(w, ty, c.menu == "menu    abimenu" && matches!(ty, 1 | 3)),
                    // FUN_1013f880: Ranged Attack through the prompt for enemies, as `/ra`.
                    Some(Cat::Ranged) => {
                        w.cats = None;
                        if !target::open_sub(w, 0x20, SubThen::Line { stash: "/range ".into(), quoted: false, from_macro: false }, None, String::new(), 0) {
                            refuse(w, 0x5d);
                        }
                    }
                    Some(Cat::Mount) => open_mount(w, c.menu == "menu    abimenu"),
                    Some(Cat::Emote) => {
                        w.emote_path.clear();
                        open_emote(w);
                    }
                    Some(Cat::Friends) => log::info!("the Friend List (flistmai) is not implemented"),
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
    let kind = match (&w.ability, &w.magic) {
        (Some(a), _) => a.kind,
        (None, Some(_)) => Kind::Magic,
        _ => return false,
    };
    if sort_key(w, kind, key) {
        return true;
    }
    match key {
        egui::Key::Enter => enter(w, kind),
        // FUN_10225b00 / FUN_10225b80: up one folder, or close at the root.
        egui::Key::Escape if kind == Kind::Emote && !w.emote_path.is_empty() => {
            w.emote_path.pop();
            open_emote(w);
        }
        egui::Key::Escape => {
            // Esc on an ability sub-list goes back to its parent type (not Monstrosity's).
            let parent = w.ability.as_ref().filter(|_| kind == Kind::Ability).map(|a| ABILITY_TYPES.get(a.ty as usize).map_or(0, |t| t.0)).filter(|&p| p != 0 && p != 0x14);
            let sortable = w.ability.as_ref().is_some_and(|a| a.sort.is_some());
            match kind {
                Kind::Magic => w.magic = None,
                Kind::Ability | Kind::Mount | Kind::Emote => w.ability = None,
            }
            if let Some(p) = parent {
                open_ability(w, p as u16, sortable);
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        k => {
            let win = match kind {
                Kind::Magic => w.magic.as_mut(),
                Kind::Ability | Kind::Mount | Kind::Emote => w.ability.as_mut(),
            };
            if win.is_some_and(|win| step(win, k)) {
                w.sounds.push(Ui::Cursor as u32);
                remember(w);
            }
        }
    }
    true
}

fn list_mut(w: &mut World, kind: Kind) -> Option<&mut ListWin> {
    match kind {
        Kind::Magic => w.magic.as_mut(),
        Kind::Ability | Kind::Mount | Kind::Emote => w.ability.as_mut(),
    }
}

/// Key function 0x11 (Num+, message 8): from the list (rows, a sort window attached, not in Manual) to the sort
/// window and back (FUN_1022ca30 / FUN_1022c290, abilities FUN_10224460 / FUN_10223cf0), SE 0013.
pub fn num_plus(w: &mut World) {
    if w.prompt.is_some() || w.cats.is_some() {
        return;
    }
    let kind = w.ability.as_ref().map_or(Kind::Magic, |a| a.kind);
    let Some(win) = list_mut(w, kind).filter(|x| !x.hidden && x.sort.is_some() && x.ask.is_none() && x.manual == 0) else { return };
    if win.sort_focus.is_some() {
        win.sort_focus = None;
    } else if !win.rows.is_empty() {
        // The sort window's own cursor starts on child 1 (inferred).
        win.sort_focus = Some(1);
    } else {
        return;
    }
    w.sounds.push(Ui::Close as u32);
}

/// FUN_1022c070: the current list (record valid, type, exclusion) sorted by record +0x3e; the other spells keep
/// their relative order and fill the holes after it; the count is kept.
fn auto_sort(o: &mut super::macros::Order, known: &[u8], in_list: impl Fn(u16) -> bool, key: impl Fn(u16) -> Option<u16>) {
    let n = o.list.len();
    let mut local = o.list.clone();
    for x in local.iter_mut().filter(|x| **x != 0 && in_list(**x)) {
        *x = 0;
    }
    let mut by_key = vec![0u16; n];
    for id in 1..n as u16 {
        if bit(known, id as usize)
            && let Some(k) = key(id).filter(|&k| (k as usize) < n)
        {
            by_key[k as usize] = id;
        }
    }
    let mut out: Vec<u16> = by_key[1..].iter().copied().filter(|&x| x != 0).map(|x| if in_list(x) { x } else { 0 }).collect();
    out.resize(n, 0);
    let mut at = 0;
    for &x in local.iter().filter(|&&x| x != 0) {
        while at < n && out[at] != 0 {
            at += 1;
        }
        if at < n {
            out[at] = x;
            at += 1;
        }
    }
    o.list = out;
    o.bits = known.iter().copied().chain(std::iter::repeat(0)).take(n / 8).collect();
}

/// The sort window, its sortyn and Manual Sort on the list. True when the key was taken.
fn sort_key(w: &mut World, kind: Kind, key: egui::Key) -> bool {
    use egui::Key as K;
    let win = match kind {
        Kind::Magic => w.magic.as_mut(),
        Kind::Ability | Kind::Mount | Kind::Emote => w.ability.as_mut(),
    };
    let Some(win) = win else { return false };
    let Some(menu) = win.sort else { return false };
    // Auto-sort's sortyn (default No): Yes sorts the type by record +0x3e and rebuilds the list, focused.
    if let Some(yes) = win.ask {
        match key {
            K::ArrowUp | K::ArrowDown => {
                win.ask = Some(!yes);
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter | K::Escape => {
                let yes = yes && key == K::Enter;
                win.ask = None;
                w.sounds.push(if yes { Ui::Select } else { Ui::Cancel } as u32);
                if yes {
                    win.sort_focus = None;
                    let (ty, exclude) = (win.ty, win.exclude);
                    let known = w.spells_known.clone();
                    let acts = w.actions.clone();
                    if let (Some(o), Some(acts)) = (order_mut(w, false), acts) {
                        let in_list = |id: u16| acts.spell(id).is_some_and(|r| exclude & (1 << (r.kind & 0x1f)) == 0 && (ty == 0 || r.kind == ty));
                        auto_sort(o, &known, in_list, |id| acts.spell(id).map(|r| r.sort));
                    }
                    rebuild(w, kind);
                }
            }
            _ => {}
        }
        return true;
    }
    if let Some(c) = win.sort_focus {
        let n = match menu {
            "menu    blusortw" => 3,
            "menu    mgcsortw" => 2,
            _ => 1,
        };
        match key {
            K::ArrowDown => win.sort_focus = Some(c % n + 1),
            K::ArrowUp => win.sort_focus = Some(if c <= 1 { n } else { c - 1 }),
            K::Escape => {
                win.sort_focus = None;
                w.sounds.push(Ui::Close as u32);
                return true;
            }
            // Row 1 of mgcsortw / blusortw: Auto-sort; their row 2 and abisortw's row 1: Manual Sort on the list.
            K::Enter if c == 1 && n > 1 => {
                win.ask = Some(false);
                w.sounds.push(Ui::Select as u32);
                return true;
            }
            K::Enter if c <= 2 => {
                (win.manual, win.sort_focus) = (1, None);
                w.sounds.push(Ui::Select as u32);
                return true;
            }
            // blusortw row 3 (FUN_101df020): the Set Spells screen (0x119 seen and BLU main or sub, else group 7 0x57).
            K::Enter => {
                super::bluset::open(w);
                if w.bluset.is_some() {
                    w.sounds.push(Ui::Select as u32);
                }
                return true;
            }
            _ => return true,
        }
        w.sounds.push(Ui::Cursor as u32);
        return true;
    }
    if win.manual == 0 {
        return false;
    }
    match key {
        K::Enter if win.manual == 1 => {
            (win.manual, win.picked) = (2, win.scroll + win.cursor);
            w.sounds.push(Ui::Select as u32);
        }
        K::Enter => {
            let (from, to) = (win.rows.get(win.picked).copied(), win.rows.get(win.scroll + win.cursor).copied());
            win.manual = 1;
            w.sounds.push(Ui::Select as u32);
            // FUN_10226d00 / FUN_10226100 for mounts / emotes: the same insert on moix's 64 / eix's 128 entries.
            if matches!(kind, Kind::Mount | Kind::Emote) {
                if let (Some(a), Some(b), Some(m)) = (from, to, w.macros.as_mut()) {
                    let order = if kind == Kind::Mount { &mut m.moix } else { &mut m.eix };
                    if let (Some(p), Some(t)) = (order.iter().position(|&x| x == a), order.iter().position(|&x| x == b))
                        && p != t
                    {
                        let id = order.remove(p);
                        order.insert(t, id);
                    }
                }
            } else if let (Some(a), Some(b)) = (from, to)
                && let Some(o) = order_mut(w, kind == Kind::Ability)
                && let (Some(p), Some(t)) = (o.ids().iter().position(|&x| x == a), o.ids().iter().position(|&x| x == b))
            {
                o.move_to(p, t);
            }
            let keep = list_mut(w, kind).map(|x| (x.scroll, x.cursor));
            rebuild(w, kind);
            if let (Some(x), Some((s, c))) = (list_mut(w, kind), keep) {
                (x.scroll, x.cursor, x.manual) = (s, c, 1);
            }
        }
        K::Escape if win.manual == 2 => win.manual = 1,
        K::Escape => {
            // Back to the sort window, on its Manual Sort row (inferred).
            (win.manual, win.sort_focus) = (0, Some(if menu == "menu    abisortw" { 1 } else { 2 }));
            w.sounds.push(Ui::Close as u32);
        }
        k => {
            if step(win, k) {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
    }
    true
}

/// The rows again after a sort, the window state kept but Manual and the sort focus.
fn rebuild(w: &mut World, kind: Kind) {
    let Some(win) = list_mut(w, kind) else { return };
    let (ty, list_mode, exclude) = (win.ty, win.list_mode, win.exclude);
    let rows = match kind {
        Kind::Magic => magic_rows(w, ty, list_mode, exclude),
        Kind::Ability => ability_rows(w, ty),
        Kind::Mount => mount_rows(w),
        Kind::Emote => emote_rows(w, ty & 0x7fff),
    };
    if let Some(win) = list_mut(w, kind) {
        let n = rows.len();
        win.rows = rows;
        win.manual = 0;
        win.sort_focus = None;
        if win.scroll + win.cursor >= n {
            (win.scroll, win.cursor) = (0, 0);
        }
    }
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
    // y 47, or 94 under the gauge window (FUN_1012c0f0).
    let top = if w.gauge.is_some() { 94.0 } else { 47.0 };
    let (x, y) = crate::render::screens::push_window_at(b, ui, style, m, sw, sh, top - m.elements[0].y as f32);
    crate::render::screens::push_gauge(b, ui, wps, x + 8.0, y + 9.0, 38, 97, f);
    let pct = ((f * 100.0) as u32).to_string();
    crate::render::screens::push_shape_text_right(b, ui, pct.as_bytes(), x + 106.0, y + 7.0, 0x8080_8080);
    crate::render::screens::push_shape_text(b, ui, b"%", x + 112.0, y + 7.0, 0x8080_8080);
}

/// Mouse (window messages 9 hover / 5 confirm): hovering a row or category moves the cursor there (SE 0001), a click
/// on the highlighted one is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    if w.prompt.is_some() || !(moved || clicked) {
        return;
    }
    let hit = |name: &str, child: u16| -> bool {
        let (Some(r), Some(m)) = (gu.window_rect(name, sw, sh), gu.layout(name)) else { return false };
        m.elements.iter().find(|e| e.id == child).is_some_and(|e| {
            let (x, y) = (r[0] + e.x as f32, r[1] + e.y as f32);
            p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
        })
    };
    if let Some(c) = w.cats {
        let Some(k) = cat_rows(c.menu).iter().position(|r| hit(c.menu, r.0)) else { return };
        gu.hover.set(true);
        if clicked && k == c.sel {
            key(w, egui::Key::Enter);
        } else if k != c.sel {
            w.cats = Some(Cats { sel: k, ..c });
            w.sounds.push(Ui::Cursor as u32);
        }
        return;
    }
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|x| !x.hidden) else { return };
    let menu = win.menu();
    let rows = win.rows.len().saturating_sub(win.scroll).min(VISIBLE);
    let Some(k) = (0..rows).find(|&k| hit(menu, k as u16 + 1)) else { return };
    gu.hover.set(true);
    let same = k == win.cursor;
    if clicked && same {
        key(w, egui::Key::Enter);
    } else if !same {
        let win = if w.ability.is_some() { w.ability.as_mut() } else { w.magic.as_mut() };
        if let Some(win) = win {
            win.cursor = k;
        }
        w.sounds.push(Ui::Cursor as u32);
    }
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
        let m = gu.stacked_menu(c.menu, sw, sh).filter(|_| gu.gate_layout(c.menu, sw, sh, None));
        let (b, ui, style, _) = gu.parts();
        if let Some(m) = &m {
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
                Kind::Mount => (mount_name(w, id), mount_check(w).is_some()),
                Kind::Emote => (emote_entry(w, id).and_then(|e| w.names.as_ref()?.emote_text(e.name as u32)).unwrap_or_default(), emote_locked(w, id)),
                Kind::Magic => (spell_name(w, id), magic_check(w, id).is_some()),
                Kind::Ability => (ability_name(w, id), w.actions.as_ref().and_then(|a| a.abilities.get(id as usize)).is_some_and(|a| a.kind != 4) && ability_check(w, id).is_some()),
            };
            // Manual's picked row (colour 3) when not under the cursor.
            let picked = win.manual == 2 && win.scroll + k == win.picked;
            let tint = if k == win.cursor { 0x8080_3040 } else if picked { 0x8080_4020 } else if refused { 0x8040_4040 } else { 0x8080_8080 };
            ActionRow { icon: row_icon(w, win.kind, id), name, tint }
        })
        .collect();
    // Empty: group 9 0x4a "No spells learned." (list mode) / 0x4b "No spells available."; mounts 0x4f / 0x50 on two
    // lines (FUN_10226430).
    let empty = match (win.kind, win.list_mode) {
        (Kind::Mount, _) => format!("{}\n{}", gu.lobby_msg(9, 0x4f, ""), gu.lobby_msg(9, 0x50, "")),
        (Kind::Magic, true) => gu.lobby_msg(9, 0x4a, ""),
        _ => gu.lobby_msg(9, 0x4b, ""),
    };
    let menu = win.menu();
    gu.replay_on_change(menu, (win.kind as u64) << 32 | (win.ty as u64) << 16 | w.emote_path.len() as u64);
    let m = gu.stacked_menu(menu, sw, sh).filter(|_| gu.gate_layout(menu, sw, sh, None));
    let (b, ui, style, font) = gu.parts();
    let bar = m.as_ref().map(|m| push_action_list(b, ui, style, font, GameUi::ATLAS, m, &rows, win.cursor, &empty, sw, sh, frame, (win.rows.len(), win.scroll)));
    // The list has the focus unless its sort window or a prompt over it does.
    if let Some([x, y, h]) = bar.filter(|_| win.sort_focus.is_none() && win.ask.is_none()) {
        gu.bar_site(super::scrollbar::Owner::Actions, x, y, h, win.rows.len(), VISIBLE, win.scroll);
    }
    rects.extend(gu.window_rect(menu, sw, sh));
    // The sort window at its layout (384, 48), highlighted while focused; blusortw's row 3 disabled (no disabled
    // sprite: keytops3 89 at 0x30808080, under the cursor 0x40402810 at (+1, +1)) without 0x119 and BLU.
    if let Some(sort) = win.sort {
        let off = sort == "menu    blusortw" && !(w.recasts_seen && w.stats.as_ref().is_some_and(|s| s.main.0 == 16 || s.sub.0 == 16));
        let hide: &[u16] = if off { &[3] } else { &[] };
        if let Some(sw_) = gu.lobby_window_off(sort, sw, sh, (0.0, 0.0), win.sort_focus, win.sort_focus.is_some(), hide) {
            rects.push(sw_.frame);
            if off && let Some(&(_, r)) = sw_.children.iter().find(|ch| ch.0 == 2) {
                let on = win.sort_focus == Some(3);
                let d = if on { 1.0 } else { 0.0 };
                gu.sprite("menu    keytops3", 89, r[0] + d, r[1] + 16.0 + d, if on { 0x4040_2810 } else { 0x3080_8080 });
            }
        }
        // sortyn 16 px lower under blusortw (FUN_102227d0, inferred from its child count).
        if let Some(yes) = win.ask {
            let dy = if sort == "menu    blusortw" { 16.0 } else { 0.0 };
            rects.extend(gu.lobby_window_off("menu    sortyn", sw, sh, (0.0, dy), Some(if yes { 1 } else { 2 }), true, &[]).map(|x| x.frame));
        }
    }
    // subwindo for the cursor row: magic "MP Cost" (9 0xe6) and "Next" (9 0xe7); abilities by their kind.
    let lines: Vec<(String, String)> = match (win.kind, win.at()) {
        (Kind::Magic, Some(s)) => {
            let mp = w.actions.as_ref().and_then(|a| a.spell(s)).map_or(0, |r| r.mp);
            vec![(gu.lobby_msg(9, 0xe6, "MP Cost"), mp.to_string()), (gu.lobby_msg(9, 0xe7, "Next"), clock(spell_recast(w, s)))]
        }
        // subwindo mode 10: the shared mount recast, "Next" on the second line.
        (Kind::Mount, _) => {
            let secs = w.mount_recast.map_or(0.0, |t| t.saturating_duration_since(Instant::now()).as_secs_f32());
            vec![(String::new(), String::new()), (gu.lobby_msg(9, 0xe7, "Next"), clock(secs))]
        }
        (Kind::Ability, Some(i)) => match w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)) {
            Some(a) => {
                let next = (gu.lobby_msg(9, 0xe7, "Next"), clock(if a.timer == 900 { 0.0 } else { ability_recast(w, a.timer) }));
                match ability_kind(a.kind) {
                    // subwindo modes 2 / 10 (FUN_1014fc20): Next on the second line (+0x1a).
                    2 | 10 => vec![(String::new(), String::new()), next],
                    5 | 8 => vec![next],
                    3 => vec![(gu.lobby_msg(0x0b, 0xbd, "TP Cost"), a.tp.to_string()), next],
                    4 => vec![("TP".into(), w.vitals.map_or(0, |v| v.2).to_string())],
                    9 => {
                        if gu.gate_layout("menu    subwindo", sw, sh, None)
                            && let Some(m) = gu.stacked_menu("menu    subwindo", sw, sh)
                        {
                            let (b, ui, style, _) = gu.parts();
                            crate::render::invwin::push_tp_cost_box(b, ui, style, &m, w.vitals.map_or(0, |v| v.2), a.tp16, sw, sh);
                        }
                        Vec::new()
                    }
                    6 => vec![(gu.lobby_msg(9, 0xe6, "MP Cost"), a.cost.to_string())],
                    _ => Vec::new(),
                }
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    if !lines.is_empty() && gu.gate_layout("menu    subwindo", sw, sh, None) {
        let stacked = gu.stacked_menu("menu    subwindo", sw, sh);
        let (b, ui, style, _) = gu.parts();
        if let Some(m) = stacked {
            push_cost_box(b, ui, style, &m, &lines, sw, sh);
        }
    }
    rects
}

/// The help and title bars: the list's title (group 0xd 76 "Magic List" / 68 "Abilities") and the cursor row's
/// description; while the prompt is up "Select target of spell." / "...ability." and the name in slot 2.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if let Some(p) = w.prompt.as_ref().filter(|p| p.help != 0) {
        bars.help[2] = gu.menu_msg(p.help as usize, "");
        bars.title[2] = if matches!(p.then, SubThen::Item { .. }) { gu.title_msg(2).unwrap_or_default() } else { p.title.clone() };
        return;
    }
    if let Some(c) = w.cats {
        let child = cat_rows(c.menu).get(c.sel).map_or(1, |r| r.0);
        let [help, title] = gu.focus_text(c.menu, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|x| !x.hidden) else { return };
    // sortyn: 0xe 0x50 "Sort list?", no title; the sort window: its rows' strings.
    if win.ask.is_some() {
        (bars.help[1], bars.title[1]) = (gu.menu_msg(0x50, ""), String::new());
        return;
    }
    if let (Some(menu), Some(c)) = (win.sort, win.sort_focus) {
        let [help, title] = gu.focus_text(menu, c);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        // FUN_10223e10: abisortw's help for the mount / emote list.
        match win.kind {
            Kind::Mount => bars.help[1] = gu.menu_msg(0x3e5, ""),
            Kind::Emote => bars.help[1] = gu.menu_msg(0x39f, ""),
            _ => {}
        }
        return;
    }
    bars.title[1] = gu.title_msg(match win.kind {
        Kind::Magic => 76,
        Kind::Emote => 0xb4,
        _ => 68,
    })
    .unwrap_or_default();
    // Manual Sort: magic 0xe 8 / 9, abilities 0xa / 0xb (FUN_1022cb90 / FUN_10224640), mounts 0x3e6 / 0x3e7.
    if win.manual != 0 {
        let base = match win.kind {
            Kind::Magic => 8,
            Kind::Ability => 0xa,
            Kind::Mount => 0x3e6,
            Kind::Emote => 0x3a3,
        };
        bars.help[1] = gu.menu_msg(base + (win.manual as usize - 1), "");
        return;
    }
    bars.help[1] = match (win.kind, win.at(), w.names.as_ref()) {
        (Kind::Magic, Some(s), Some(n)) => n.spell_description(s as u32).unwrap_or_default(),
        (Kind::Ability, Some(i), Some(n)) => n.ability_description(i as u32).unwrap_or_default(),
        (Kind::Mount, Some(i), Some(n)) => n.mount_description(i as u32).unwrap_or_default(),
        (Kind::Emote, Some(i), Some(n)) => emote_entry(w, i).and_then(|e| n.emote_text(e.help as u32)).unwrap_or_default(),
        _ => String::new(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Auto-sort (FUN_1022c070 steps 1..7): the type's spells by key, the others keep their order in the holes.
    #[test]
    fn auto_sort_keeps_others() {
        let mut o = crate::client::macros::Order { bits: vec![0; 2], list: vec![0; 16], count: 5 };
        o.list[..5].copy_from_slice(&[5, 1, 9, 3, 7]);
        let mut known = vec![0u8; 2];
        for id in [1u16, 3, 5, 7, 9] {
            known[(id / 8) as usize] |= 1 << (id % 8);
        }
        // Odd ids below 6 are the type; keys reverse them.
        let in_list = |id: u16| id < 6;
        auto_sort(&mut o, &known, in_list, |id| Some(10 - id));
        // All known ids by key: 9 7 5 3 1; the others (9, 7) zeroed, then refilled in their old order.
        assert_eq!(&o.list[..5], &[9, 7, 5, 3, 1]);
        assert_eq!(o.count, 5);
    }

    #[test]
    fn recast_clock() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(47.2), "0:48");
        assert_eq!(clock(3725.0), "1:02:05");
    }

    #[test]
    fn list_paging() {
        let mut win = ListWin { kind: Kind::Magic, ty: 0, exclude: 0, list_mode: true, rows: (1..=30).collect(), scroll: 0, cursor: 0, hidden: false, sort: None, sort_focus: None, ask: None, manual: 0, picked: 0 };
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
