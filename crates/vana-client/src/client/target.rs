//! Targeting and lock-on (target object DAT_10565d34, FUN_10151280 / FUN_10150da0), Numpad * (key
//! functions 0x44 / 0x30), the s2c 0x058 assist target, and the locked camera of FUN_1001e030.

use super::World;
use super::partymenu::Pick;
use crate::audio::Ui;
use glam::Vec3;

/// SE "0009" (FUN_10036f50): a target from none; "0010" (FUN_10036fb0): switching targets.
const SE_TARGET: u32 = 9;
const SE_SWITCH: u32 = 10;

/// The forced flash (actor +0x88 bit 0x200) for a frame, as the sub-target cursor and area previews set it,
/// unless /targetcharaeffect turned it off (config 0x99 bit 7).
/// FUN_1021f030: the nearest (horizontal distance) monster with a model, called for help on (+0x127 bit 0, ours
/// marks.yell) and not dead; the compass's untracked pointer and `<ht>`. No range, claim or party test.
pub fn call_for_help(w: &World) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    for (id, e) in &w.entities {
        if !(e.mob
            && e.marks.yell
            && !matches!(e.motion.status, 2 | 3)
            && w.actors.get(id).is_some_and(|a| a.gpu.is_some()))
        {
            continue;
        }
        let d = ((e.pos.x - w.pos.x).powi(2) + (e.pos.z - w.pos.z).powi(2)).sqrt();
        if best.is_none_or(|(_, b)| b == 0.0 || d < b) {
            best = Some((*id, d));
        }
    }
    best.filter(|&(_, d)| d != 0.0).map(|(id, _)| id)
}

pub fn force_flash(w: &mut World, id: u32) {
    if w.game.get(0x99) & 0x80 == 0
        && let Some(a) = w.actors.get_mut(&id)
    {
        a.flash.forced = true;
    }
}

/// What a sub-target prompt's confirm runs (its callback).
#[derive(Clone, Debug)]
pub enum SubThen {
    /// LAB_1022d140: `/magic "<name>" <id>`, then the magic list closes.
    Magic(String),
    /// LAB_10224ef0: `/weaponskill`, `/monsterskill` or `/jobability "<name>" <id>`.
    Ability(&'static str, String),
    /// LAB_10077950: a text command with the tag replaced by the server id (or the quoted name, flag 0x08); a
    /// line from a macro stops the macro when cancelled (FUN_101a1cb0).
    Line { stash: String, quoted: bool, from_macro: bool },
    /// iuse's Use (LAB_101f31e0): FUN_101f0260 on the slot with the pick; the list comes back either way.
    Item { container: u8, index: u8 },
    /// The emote list's line (LAB_10225c50): run while the pick is still the target, then the old one comes back.
    Command(String),
    /// The engaged battle menu's Check (LAB_1012bec0): FUN_101347b0 on the pick.
    Check,
    /// A party menu pick (client/partymenu.rs).
    Party(super::partymenu::Pick),
    /// The main menu's Trade (FUN_1013fe70): FUN_1013fd30 on the pick.
    Trade,
}

/// The party-list cursor of `<stpt>` / `<stal>` (FUN_10202d80, DAT_106643e0): a window slot below `count` (6 / 18)
/// over the used slots; no candidate filter, the world target and lock untouched. The fellow row (-1) is `fellow`.
pub struct PartyCursor {
    pub index: usize,
    pub count: usize,
    pub then: SubThen,
    pub range: Option<f32>,
    /// Rearrange's second stage (DAT_106643fc / +0x3f5 / +0x3f6): the member picked first and its window party's
    /// first slot; the cursor stays inside that party.
    pub moving: Option<(u32, usize)>,
    /// Row -1, the fellow row (batch 1 §4): only for `<stpt>` / `<stal>` (modes 2 / 3), never in a two-step pick.
    pub fellow: bool,
}

/// FUN_10202d80's low bound -1 (modes 2 / 3: the text-command picks, not the party menu's) and a fellow here.
fn fellow_ok(w: &World, then: &SubThen) -> bool {
    !matches!(then, SubThen::Party(_)) && super::petrow::fellow(w).is_some()
}

/// FUN_102031f0(id, 1, 0): a member of our six slots that is not a trust (FUN_10201eb0: ActIndex > 0x6ff).
pub fn in_own_six(w: &World, id: u32) -> bool {
    w.party.slots().slots[..6].iter().flatten().any(|m| m.id == id && m.index < 0x700)
}

/// Ours (modern): the last target while it is still here and targetable, `<lt>`.
pub fn last_target(w: &World) -> Option<(u32, u16)> {
    let id = w.last_target.filter(|&id| Some(id) != w.target)?;
    let e = w.entities.get(&id).filter(|e| e.targetable())?;
    Some((id, e.index))
}

/// FUN_101593c0 on the focus target: someone other than us in our six slots.
pub fn focus_member(w: &World) -> Option<u32> {
    w.focus.map(|f| f.0).filter(|&id| id != w.my_id && in_own_six(w, id))
}

/// "Sub-target up" (T+0x59 || T+0xc4): the 3D prompt or the party-list cursor.
pub fn sub_up(w: &World) -> bool {
    w.prompt.is_some() || w.party_cursor.is_some()
}

/// FUN_10202d80: the cursor opens on the current target's slot when it is below `count`, else on us (slot 0).
pub fn open_party_cursor(w: &mut World, count: usize, then: SubThen, range: Option<f32>) {
    let slots = w.party.slots();
    let row = w
        .target
        .and_then(|t| slots.slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == t)))
        .filter(|&i| i < count);
    // FUN_10203540 failing: -1 when the target is the fellow.
    let fellow = row.is_none() && fellow_ok(w, &then) && super::petrow::fellow(w).map(|f| f.0) == w.target;
    w.party_cursor = Some(PartyCursor { index: row.unwrap_or(0), count, then, range, moving: None, fellow });
}

/// FUN_10203570 / FUN_10203630 (FUN_102035c0 / FUN_10203680 in a stage-2 range): the next / previous used slot in
/// lo..hi, wrapping.
fn party_step(used: &[bool], index: usize, lo: usize, hi: usize, down: bool) -> usize {
    let hi = hi.min(used.len()).max(lo + 1);
    let n = hi - lo;
    let mut i = index.clamp(lo, hi - 1) - lo;
    for _ in 0..n {
        i = if down { (i + 1) % n } else { (i + n - 1) % n };
        if used[lo + i] {
            return lo + i;
        }
    }
    index
}

/// The cursor's keys (FUN_102036f0 through the window events): Down / Up over the used slots (SE 0001), Enter runs
/// the line with the slot's id or quoted roster name (SE 0002), Escape cancels (SE 0003; a macro line stops the
/// macro). Left / Right do nothing. True when the key was taken.
pub fn party_cursor_key(w: &mut World, key: egui::Key) -> bool {
    use egui::Key as K;
    let mut used: Vec<bool> = w.party.slots().slots.iter().map(|m| m.is_some()).collect();
    // Slot 0 is always us.
    used[0] = true;
    // Ours: the modern party panel has no fellow row, so the cursor doesn't stop on one there (a deviation of the
    // modern UI; the classic window keeps retail's fellow row).
    let fellow = w
        .party_cursor
        .as_ref()
        .is_some_and(|pc| !w.modern && pc.moving.is_none() && fellow_ok(w, &pc.then))
        .then(|| super::petrow::fellow(w))
        .flatten();
    let Some(pc) = w.party_cursor.as_mut() else { return false };
    match key {
        K::ArrowDown | K::ArrowUp => {
            let down = key == K::ArrowDown;
            let (lo, hi) = pc.moving.map_or((0, pc.count), |(_, base)| (base, base + 6));
            // FUN_10203570 / FUN_10203630 with the low bound -1: ... last row, fellow, row 0 ...
            let (i, f) = if pc.fellow {
                (if down { 0 } else { party_step(&used, 0, lo, hi, false) }, false)
            } else {
                let i = party_step(&used, pc.index, lo, hi, down);
                let wrapped = if down { i <= pc.index } else { pc.index == lo };
                if fellow.is_some() && wrapped { (pc.index, true) } else { (i, false) }
            };
            if (i, f) != (pc.index, pc.fellow) {
                (pc.index, pc.fellow) = (i, f);
                w.sounds.push(crate::audio::Ui::Cursor as u32);
            }
        }
        K::Enter => {
            let (index, moving, then) = (pc.index, pc.moving, pc.then.clone());
            let row = if pc.fellow { fellow.clone() } else { None };
            let (id, name) = match index {
                // FUN_10202fe0 on -1: the fellow's id and name.
                _ if pc.fellow => match row {
                    Some(f) => f,
                    None => return true,
                },
                0 => (w.my_id, w.name.clone()),
                i => match w.party.slots().slots.get(i).cloned().flatten() {
                    Some(m) => (m.id, m.name),
                    None => return true,
                },
            };
            // FUN_10202fe0: Rearrange (two callbacks) refuses us, Focus (mode 1) us and anyone not in our six slots or
            // a trust (SE 0004, the cursor stays).
            let refused = match then {
                SubThen::Party(Pick::Rearrange) => id == w.my_id,
                SubThen::Party(Pick::Focus) => id == w.my_id || !in_own_six(w, id),
                _ => false,
            };
            if refused {
                w.sounds.push(crate::audio::Ui::Error as u32);
                return true;
            }
            w.sounds.push(crate::audio::Ui::Select as u32);
            // Rearrange: the first pick keeps the cursor inside that member's window party, the second moves the first
            // member to its place in the roster order (LAB_102050c0 -> FUN_100e99b0, no packet), then stage 1 again.
            if let SubThen::Party(Pick::Rearrange) = then {
                let pc = w.party_cursor.as_mut().unwrap();
                match moving {
                    None => pc.moving = Some((id, index / 6 * 6)),
                    Some((first, _)) => {
                        pc.moving = None;
                        w.party.move_member(first, id);
                    }
                }
                return true;
            }
            w.party_cursor = None;
            let line = match &then {
                SubThen::Line { stash, quoted: true, .. } => format!("{stash}\"{name}\""),
                SubThen::Line { stash, .. } => format!("{stash}{id}"),
                SubThen::Party(p) => {
                    super::partymenu::picked(w, *p, Some((index, id, name)));
                    return true;
                }
                _ => return true,
            };
            super::textcmd::run(w, &line, super::textcmd::Mode::SubTarget);
        }
        K::Escape => {
            w.sounds.push(crate::audio::Ui::Cancel as u32);
            // FUN_10202f00: Escape in Rearrange's second stage only goes back to the first.
            if pc.moving.take().is_some() {
                return true;
            }
            let pc = w.party_cursor.take().unwrap();
            if let SubThen::Party(p) = pc.then {
                super::partymenu::picked(w, p, None);
            }
            if let SubThen::Line { from_macro: true, .. } = pc.then
                && let Some(m) = w.macros.as_mut()
            {
                m.stop();
            }
        }
        _ => {}
    }
    true
}

/// The cursor with the mouse over the party windows' rows (the windows' generic events *(inf)*): hovering a used row
/// moves it (SE 0001), a click on the cursor row is Enter, a right click on a party window Escape.
#[allow(clippy::too_many_arguments)]
pub fn party_cursor_pointer(
    w: &mut World,
    gu: &super::game_ui::GameUi,
    sw: f32,
    sh: f32,
    p: (f32, f32),
    moved: bool,
    clicked: bool,
    right: bool,
) {
    let Some(pc) = w.party_cursor.as_ref() else { return };
    let mut used: Vec<bool> = w.party.slots().slots.iter().map(|m| m.is_some()).collect();
    used[0] = true;
    let (lo, hi) = pc.moving.map_or((0, pc.count), |(_, base)| (base, base + 6));
    // The mouse can't pick -1; hovering a row from it moves the cursor there.
    let index = if pc.fellow { usize::MAX } else { pc.index };
    let inside = |r: [f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    let mut hit = None;
    let mut over = false;
    for (name, base) in [(w.party.slots().window(), 0usize), ("menu    raid1".into(), 6), ("menu    raid2".into(), 12)]
    {
        let (Some((x, y)), Some(l)) = (gu.window_xy(&name, sw, sh), gu.layout(&name)) else { continue };
        if !used.iter().skip(base).take(6).any(|&u| u) {
            continue;
        }
        over |= gu.window_rect(&name, sw, sh).is_some_and(inside);
        for e in l.elements.iter().skip(1).filter(|e| (1..=6).contains(&e.id)) {
            if inside([x + e.x as f32, y + e.y as f32, e.w as f32, e.h as f32]) {
                hit = Some(base + e.id as usize - 1);
            }
        }
    }
    if right {
        if over {
            party_cursor_key(w, egui::Key::Escape);
        }
        return;
    }
    let Some(i) = hit.filter(|&i| used.get(i).copied().unwrap_or(false) && (lo..hi).contains(&i)) else { return };
    gu.hover.set(true);
    if (moved || clicked) && i != index {
        if let Some(pc) = w.party_cursor.as_mut() {
            (pc.index, pc.fellow) = (i, false);
        }
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    } else if clicked {
        party_cursor_key(w, egui::Key::Enter);
    }
}

/// FUN_10203c80: a slot that emptied under the cursor steps it back to the previous used one (0 at the least).
pub fn party_cursor_update(w: &mut World) {
    let used: Vec<bool> = w.party.slots().slots.iter().map(|m| m.is_some()).collect();
    let fellow = super::petrow::fellow(w).is_some();
    if let Some(pc) = w.party_cursor.as_mut() {
        // Ours: the fellow leaving under the cursor puts it on row 0.
        if pc.fellow && !fellow {
            (pc.fellow, pc.index) = (false, 0);
        }
        while pc.index > 0 && !used.get(pc.index).copied().unwrap_or(false) {
            pc.index -= 1;
        }
    }
}

/// FUN_101505a0: the cursor hand's tint by the pick's range from us.
fn hand_tint(w: &World, id: Option<u32>, range: Option<f32>) -> u32 {
    match (id, range) {
        (Some(id), Some(r)) => {
            let p = if id == w.my_id { w.pos } else { w.entities.get(&id).map_or(w.pos, |e| e.pos) };
            let raw = (p - w.pos).length();
            let d = Vec3::new(p.x - w.pos.x, 0.0, p.z - w.pos.z).length();
            if d > r + 0.5 || raw >= 25.0 {
                0xe0d0_4040
            } else if d > r - 0.5 {
                0xe0d0_d040
            } else {
                0xa040_40d0
            }
        }
        _ => 0xa040_40d0,
    }
}

/// FUN_10202110 with the cursor at -1: the hand at the fellow row (FUN_102022b0), its tint and the fellow to flash.
pub fn fellow_hand(w: &World) -> Option<(u32, Option<u32>)> {
    let pc = w.party_cursor.as_ref().filter(|pc| pc.fellow)?;
    let id = super::petrow::fellow(w)?.0;
    let tint = hand_tint(w, Some(id), pc.range);
    Some((tint, (tint != 0xe0d0_4040).then_some(id)))
}

/// FUN_10202110: the party window's hand: the cursor's slot, tinted by range from base 0xa04040d0 (red 0xe0d04040,
/// yellow 0xe0d0d040; flashing the pick unless red), or with no cursor the current target's slot in 0x80808080. The
/// slot, its tint and the entity to flash.
pub fn party_hand(w: &World) -> Option<(usize, u32, Option<u32>)> {
    let slots = w.party.slots();
    match &w.party_cursor {
        // Row -1 hides all three windows' hands (each window's vtbl+0x48); see fellow_hand.
        Some(pc) if pc.fellow => None,
        Some(pc) => {
            let id = match pc.index {
                0 => Some(w.my_id),
                // FUN_102033c0: an away member keeps the base tint and flashes nothing.
                i => match slots.here(i, w.pos_zone) {
                    Some(m) => w.entity_at(m.index).map(|(id, _)| id),
                    None => return slots.slots.get(i).cloned().flatten().map(|_| (pc.index, 0xa040_40d0, None)),
                },
            };
            let tint = hand_tint(w, id, pc.range);
            Some((pc.index, tint, id.filter(|_| tint != 0xe0d0_4040)))
        }
        None => {
            let t = w.target?;
            let i = if t == w.my_id {
                0
            } else {
                slots.slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == t))?
            };
            Some((i, 0x8080_8080, None))
        }
    }
}

/// The sub-target prompt (T+0x28 slot B, +0x5c bits 2-3, +0x60 / +0x64, +0x6c range, +0xc4 / +200 callbacks).
#[derive(Clone, Debug)]
pub struct Sub {
    pub inc: u32,
    pub exc: u32,
    pub saved: Option<u32>,
    pub saved_lock: bool,
    pub then: SubThen,
    /// The arrow colour's range from us (spell +0x45 through table 0x103644a8), if any.
    pub range: Option<f32>,
    /// The spell or ability name for titlewin slot 2, and helpwind slot 2's text id (group 0xe).
    pub title: String,
    pub help: u16,
    /// Field +0x58 (FUN_1022b850): the spell's record +0x44 bit 5 (the Geo- spells) lets LT / RT + the d-pad, or Ctrl /
    /// Alt + the arrows, move a ground-position cursor from the pick (key functions 0xb0..0xb3).
    pub ground: bool,
}

impl Sub {
    fn takes(&self, kind: u32) -> bool {
        self.inc & kind != 0 && self.exc & kind == 0
    }
}

/// Table 0x103720c0: a DAT valid-target flag's include / exclude kinds (FUN_10159170).
const SUB_FLAGS: [(u32, u32, u32); 8] = [
    (1, 0x200, 0),
    (2, 0x100, 0),
    (4, 4, 0),
    (8, 8, 0),
    (0x10, 1, 0),
    (0x20, 0x10, 0),
    (0x40, 2, 0),
    (0x80, 2, 0x1800),
];

/// Ours (the action registry): whether DAT target `flags` may take actor `id`, by the prompt's include / exclude
/// kinds (SUB_FLAGS, Sub::takes).
pub(super) fn takes(w: &World, flags: u32, id: u32) -> bool {
    let (inc, exc) = SUB_FLAGS.iter().filter(|f| flags & f.0 != 0).fold((0, 0), |(i, e), f| (i | f.1, e | f.2));
    let kind = sub_kind(w, id);
    inc & kind != 0 && exc & kind == 0
}

/// FUN_1008e290's kind bits: 0x200 self, 1 PC, 4 party, 8 alliance, 0x10 monster, 2 NPC, 0x800 our pet, 0x1000
/// others' pets and trusts (0x700.., inf for +0x12c bit 22). Not done: 0x100 ally NPCs, 0x400 allegiance.
fn sub_kind(w: &World, id: u32) -> u32 {
    if id == w.my_id {
        return 0x200 | 1 | 4;
    }
    let Some(e) = w.entities.get(&id) else { return 0 };
    let party =
        w.party.members().find(|m| m.id == id).map_or(0, |m| if m.flags & 3 == w.party.own_flags & 3 { 4 } else { 8 });
    if id >> 24 == 0 {
        return 1 | party;
    }
    if e.flags & 1 != 0 || e.mob {
        return 0x10;
    }
    let pet = match e.index {
        i if i == w.my_pet && i != 0 => 0x800,
        0x700.. => 0x1000,
        _ => 0,
    };
    2 | pet | party
}

/// FUN_10159440: opens the prompt for DAT target `flags`; the first pick (FUN_101591d0) keeps a fitting target
/// (not an enemy when we may pick ourselves), else us when allowed, else the nearest candidate in front. False when
/// nothing fits (the caller prints its line).
pub fn open_sub(w: &mut World, flags: u32, then: SubThen, range: Option<f32>, title: String, help: u16) -> bool {
    let (inc, exc) = SUB_FLAGS.iter().filter(|f| flags & f.0 != 0).fold((0, 0), |(i, e), f| (i | f.1, e | f.2));
    let sub = Sub { inc, exc, saved: w.target, saved_lock: w.locked, then, range, title, help, ground: false };
    let current = w.target.filter(|&t| t == w.my_id || w.entities.contains_key(&t));
    let first = match current {
        Some(t) if sub.takes(sub_kind(w, t)) && !(inc & 0x200 != 0 && sub_kind(w, t) & 0x10 != 0) => Some(t),
        _ if inc & 0x200 != 0 => Some(w.my_id),
        _ => None,
    };
    w.prompt = Some(sub);
    let first = first.or_else(|| nearest(w, |_, _| true));
    let Some(first) = first else {
        w.prompt = None;
        return false;
    };
    // The lock is only saved (+0x5c bits 2..3); a first pick that moves the target ends it as any change does.
    if w.target != Some(first) {
        if w.locked {
            lock(w, false, "prompt pick");
        }
        w.target = Some(first);
        w.target_since = std::time::Instant::now();
    }
    true
}

/// FUN_1022b850: an open prompt for `spell` takes the ground-position cursor when the spell record's +0x44 has bit 5.
pub fn ground_mode(w: &mut World, spell: u16) {
    let on = w.actions.as_ref().and_then(|a| a.spell(spell)).is_some_and(|r| r.flags & 0x20 != 0);
    if let Some(s) = w.prompt.as_mut() {
        s.ground = on;
    }
}

/// The ground-position cursor (field +0x10..+0x18, live while +0x22 is set): only for the pick it started from, as
/// a new pick resets it (FUN_10159660).
/// FUN_10159cc0: the cursor less target `t`'s position, zero with no cursor (or one started from another pick).
pub fn ground_offset(w: &World, t: u32) -> Vec3 {
    let Some(c) = w.ground_cursor.filter(|c| c.0 == t) else { return Vec3::ZERO };
    let at = if t == w.my_id { w.pos } else { w.entities.get(&t).map_or(c.1, |e| e.pos) };
    c.1 - at
}

/// FUN_10159660: a pick other than the cursor's ends it.
pub fn drop_stale_ground_cursor(w: &mut World) {
    if w.ground_cursor.is_some_and(|c| Some(c.0) != w.target) {
        w.ground_cursor = None;
    }
}

pub fn ground_cursor(w: &World) -> Option<Vec3> {
    w.ground_cursor.filter(|c| Some(c.0) == w.target).map(|c| c.1)
}

/// FUN_10159a20: moves the ground-position cursor by `dir` (x right, z up, as FUN_10159970 builds it for 0xb2 / 0xb3
/// / 0xb0 / 0xb1) times config 0xc4 x the frame's ticks (retail: whole, at least 1; ours: `ticks`) x 0.01, turned to the camera
/// (FUN_10159b10). The first move starts it at the pick's position. Its height follows the ground below
/// (FUN_10177770 -> FUN_10181210; kept when the probe misses).
pub fn move_ground_cursor(w: &mut World, dir: glam::Vec2, ticks: f32, look: Vec3) {
    let Some(t) = w.target.filter(|_| w.prompt.as_ref().is_some_and(|s| s.ground)) else { return };
    let from = match ground_cursor(w) {
        Some(c) => c,
        None if t == w.my_id => w.pos,
        None => match w.entities.get(&t) {
            Some(e) => e.pos,
            None => return,
        },
    };
    let step = w.game.get(0xc4) as f32 * ticks * 0.01;
    let mut p = from + cursor_step(dir, look) * step;
    let above = if w.pos_zone == 0x95 { 0.5 } else { 2.0 };
    if let Some(y) = w.ground.as_ref().and_then(|g| ground_height(g, p, above)) {
        p.y = y;
    }
    w.ground_cursor = Some((t, p));
}

/// FUN_10159b10: `dir` (x right, z up) turned by atan2(look.x, look.z): up is the camera's level forward
/// (sin t, cos t), right (cos t, -sin t), screen right.
fn cursor_step(dir: glam::Vec2, look: Vec3) -> Vec3 {
    let (sin, cos) = look.x.atan2(look.z).sin_cos();
    Vec3::new(dir.x * cos + dir.y * sin, 0.0, -dir.x * sin + dir.y * cos)
}

/// FUN_10181210 at the cursor's x / z: a segment from `above` over the cursor (zone +0x170: 2, 0.5 in zone 0x95, set
/// by FUN_10178df0) to 17 below (y points down); the hit nearest its start, the highest surface, wins (FUN_101693b0).
fn ground_height(g: &super::ground::Ground, p: Vec3, above: f32) -> Option<f32> {
    g.highest_between(p.x, p.z, p.y - above, p.y + 17.0)
}

/// Ours (the suggestions): the open prompt's cursor on `id` when the prompt takes it.
pub fn prefer(w: &mut World, id: u32) {
    if w.prompt.as_ref().is_some_and(|s| s.takes(sub_kind(w, id))) {
        w.target = Some(id);
        w.target_since = std::time::Instant::now();
    }
}

/// FUN_10158db0(0): runs the callback on the pick, SE "0009", the saved target and lock come back.
pub fn confirm_sub(w: &mut World) {
    let (Some(s), Some(t)) = (w.prompt.take(), w.target) else { return };
    w.sounds.push(SE_TARGET);
    if let SubThen::Item { container, index } = s.then {
        restore(w, &s);
        super::items::use_slot(w, container, index, t);
        return;
    }
    if matches!(s.then, SubThen::Check) {
        restore(w, &s);
        super::combat::check(w, Some(t), 0, false);
        return;
    }
    if matches!(s.then, SubThen::Trade) {
        restore(w, &s);
        super::trade::start_on(w, Some(t));
        return;
    }
    if let SubThen::Command(line) = &s.then {
        super::textcmd::run(w, line, super::textcmd::Mode::Internal);
        restore(w, &s);
        return;
    }
    let line = match &s.then {
        SubThen::Item { .. } | SubThen::Command(_) | SubThen::Check | SubThen::Party(_) | SubThen::Trade => {
            String::new()
        }
        SubThen::Magic(name) => format!("/magic \"{name}\" {t}"),
        SubThen::Ability(cmd, name) => format!("{cmd} \"{name}\" {t}"),
        SubThen::Line { stash, quoted: true, .. } => format!("{stash}\"{}\"", super::textcmd::display_name(w, t)),
        SubThen::Line { stash, quoted: false, .. } => format!("{stash}{t}"),
    };
    restore(w, &s);
    if matches!(s.then, SubThen::Magic(_)) {
        w.magic = None;
    }
    if matches!(s.then, SubThen::Ability(..)) {
        w.ability = None;
    }
    let mode = if matches!(s.then, SubThen::Line { .. }) {
        super::textcmd::Mode::SubTarget
    } else {
        super::textcmd::Mode::Internal
    };
    // The /magic it runs reads the ground-position cursor (FUN_10159cc0) before the prompt's state goes.
    w.ground_cursor = w.ground_cursor.filter(|c| c.0 == t && s.ground);
    super::textcmd::run(w, &line, mode);
    w.ground_cursor = None;
}

/// FUN_10158ff0: the cancel callback (the list comes back) and the saved target and lock.
pub fn cancel_sub(w: &mut World) {
    let Some(s) = w.prompt.take() else { return };
    w.ground_cursor = None;
    restore(w, &s);
    if let Some(m) = w.magic.as_mut() {
        m.hidden = false;
    }
    if let Some(m) = w.ability.as_mut() {
        m.hidden = false;
    }
    if matches!(s.then, SubThen::Line { from_macro: true, .. })
        && let Some(m) = w.macros.as_mut()
    {
        m.stop();
    }
}

/// FUN_10159040: the saved target back (a change ends a lock taken meanwhile), then the saved lock state
/// (FUN_10157660), so a lock on the same target never moves the camera.
fn restore(w: &mut World, s: &Sub) {
    let back = s.saved.filter(|&t| t == w.my_id || w.entities.contains_key(&t));
    if w.target != back {
        if w.locked {
            lock(w, false, "prompt closed");
        }
        w.target = back;
        w.target_since = std::time::Instant::now();
    }
    lock(w, s.saved_lock && back.is_some(), "prompt closed");
}

/// The prompt's own picks (FUN_101587f0 with +0xc4 set): F1 / F7 us when allowed, F2..F6 / F11 / F12 party slots
/// when party / alliance members are allowed.
pub fn sub_self(w: &mut World) {
    if w.prompt.as_ref().is_some_and(|s| s.inc & 0x200 != 0) {
        set(w, Some(w.my_id));
    }
}

pub fn sub_slot(w: &mut World, slot: usize) {
    let need = if slot < 6 { 4 } else { 8 };
    if w.prompt.as_ref().is_some_and(|s| s.inc & need != 0)
        && let Some(t) = slot_target(w, slot)
        && w.prompt.as_ref().is_some_and(|s| s.takes(sub_kind(w, t)))
    {
        set(w, Some(t));
    }
}

/// FUN_10150610: the prompt arrow's tint by distance from us against the range (y ignored), less both model radii
/// (+0x208; the spell and ability records set the radii flag): red past range + 0.5 or at 25 and more, yellow past
/// range - 0.5, else blue.
pub fn range_tint(w: &World, t: u32) -> u32 {
    let Some(r) = w.prompt.as_ref().and_then(|s| s.range) else { return 0xe040_40d0 };
    let (p, radius) = if t == w.my_id {
        (w.pos, w.my_plate.hitbox)
    } else {
        w.entities.get(&t).map_or((w.pos, 0.0), |e| (e.pos, e.hitbox))
    };
    let raw = (p - w.pos).length();
    let d = Vec3::new(p.x - w.pos.x, 0.0, p.z - w.pos.z).length() - (w.my_plate.hitbox + radius);
    if d > r + 0.5 || raw >= 25.0 {
        0xe0d0_4040
    } else if d > r - 0.5 {
        0xe0d0_d040
    } else {
        0xe040_40d0
    }
}

/// FUN_10151280: sets the target with its sound; a different target ends the lock (FUN_10153040), a prompt's pick
/// too (the prompt's close locks again).
pub fn set(w: &mut World, t: Option<u32>) {
    if w.target == t {
        return;
    }
    match (w.target, t) {
        (None, Some(_)) => w.sounds.push(SE_TARGET),
        (Some(_), Some(_)) => w.sounds.push(SE_SWITCH),
        _ => {}
    }
    let e = t.and_then(|t| w.entities.get(&t));
    log::info!(
        "target {:#x} {:?} at {:?} radius {:?} (ours {})",
        t.unwrap_or(0),
        e.map(|e| e.name.as_str()),
        e.map(|e| (e.pos.x, e.pos.z)),
        e.map(|e| e.hitbox),
        w.my_plate.hitbox
    );
    // The sub-target prompt's picks are stand-ins, not targets left.
    if w.prompt.is_none()
        && let Some(old) = w.target.filter(|&old| old != w.my_id)
    {
        w.last_target = Some(old);
    }
    w.target = t;
    w.target_since = std::time::Instant::now();
    if w.locked {
        lock(w, false, "target changed");
    }
}

/// FUN_10151f20: the player's own target inputs (Tab, Escape, a mouse pick) do nothing while locked; only the
/// lock toggle and the server move a locked target. A sub-target prompt bypasses the lock (+0xc4 set).
pub fn picks_allowed(w: &World) -> bool {
    !w.locked || sub_up(w)
}

/// FUN_10150da0: lock needs a target, a living player and a target that is not us. `why` goes in the log.
pub fn lock(w: &mut World, on: bool, why: &str) {
    if on == w.locked {
        return;
    }
    if on {
        let ok = w.target.is_some_and(|t| t != w.my_id && w.entities.contains_key(&t)) && !w.dead();
        if !ok {
            return;
        }
        // The locked camera starts from the camera on screen (the same eye and look).
        w.lock_height = w.target.map_or(-1.5, |t| lock_height(w, t));
        w.lock_posed = w.target.is_some_and(|t| posed(w, t));
        w.locked = true;
        log::info!("lock on {:#x} ({why})", w.target.unwrap_or(0));
    } else {
        w.locked = false;
        log::info!("lock off ({why})");
        // Nothing snaps: the same eye and look go on in the free branch, held 20 ticks (_DAT_10444464).
        w.unlock_ticks = 20.0;
    }
}

/// Why the target goes (FUN_10151bb0 -> FUN_10152c70), checked every frame; Some(true) also sends /attackoff
/// (FUN_10151c40, +0x130 bit 7).
fn invalid(w: &World, t: u32) -> Option<bool> {
    // Ourselves: only an event hiding us drops it (distance 0, never attackable).
    if t == w.my_id {
        return super::event::hidden(w).contains(&t).then_some(false);
    }
    let Some(e) = w.entities.get(&t) else { return Some(false) };
    let hidden = e.flags & 2 != 0 || super::event::hidden(w).contains(&t);
    drop_rule(e.flags, e.untargetable, e.motion.status, (e.pos - w.pos).length(), hidden)
}

/// FUN_10152c70 on one target: hidden (+0xb2 bit 1, from the server bits or the event via FUN_10084360);
/// attackable (+0x120 bit 13, 0x00E @0x20 bit 0) and flagged off (Some(true)), despawning (status 2) or dead
/// (3); more than 50 units away (FUN_10152ff0(50.0)).
fn drop_rule(flags: u8, untargetable: bool, status: u8, dist: f32, hidden: bool) -> Option<bool> {
    if hidden {
        return Some(false);
    }
    if flags & 1 != 0 {
        if untargetable {
            return Some(true);
        }
        if matches!(status, 2 | 3) {
            return Some(false);
        }
    }
    (dist > 50.0).then_some(false)
}

/// Kind mask bits (FUN_1008e290): 1 PC, 2 NPC, 0x10 monster, 0x20 door.
/// A door (+0xee 3) is an NPC with 0x20 added.
fn kind_bits(id: u32, e: &super::Entity) -> u32 {
    if id >> 24 == 0 {
        1
    } else {
        (if e.mob { 0x10 } else { 2 }) | if e.door != 0 || e.sub_kind == 2 { 0x20 } else { 0 }
    }
}

/// FUN_10083430: drawn on screen this frame (+0xb2 bit 0 clear: World::visible), not hidden, not ourselves, not a
/// dead or despawning monster, not TargetOffFlag (+0x128 bit 4, FUN_10084580), and in a prompt's include / exclude
/// kinds (T+0x60 / +0x64). No name test. Not done: event ops 0x12..0x15's +0x128 bit 4 and the occlusion part of the
/// cull.
fn candidate(w: &World, hidden: &std::collections::HashSet<u32>, id: u32, e: &super::Entity) -> bool {
    id != w.my_id
        && w.visible.contains(&id)
        && !hidden.contains(&id)
        && !e.target_off
        && !(e.flags & 1 != 0 && matches!(e.motion.status, 2 | 3))
        && w.prompt.as_ref().is_none_or(|s| s.takes(sub_kind(w, id)))
        && !ignored(w, id, e)
}

/// FUN_101cc220: hostile to us. In PvP (both on a team, kind 0x400) another team is, but the same Ballista side; then,
/// for an actor that is neither a player's pet / trust (0x100) nor an allied NPC: a monster, a charmed player or
/// another Monstrosity player.
pub fn hostile(w: &World, id: u32, e: &super::Entity) -> bool {
    let (a, b) = (w.my_team, e.marks.alleg);
    if (2..=99).contains(&a) && (2..=99).contains(&b) {
        let side = |t: u8| match t {
            0x28 | 0x2a => 1,
            0x29 | 0x2b => 2,
            _ => 0,
        };
        if side(a) != 0 && side(a) == side(b) {
            return false;
        }
        if (a == 99 && b == 99) || a != b {
            return true;
        }
    }
    let pet = id >> 24 != 0 && e.marks.charm;
    let allied = id >> 24 != 0 && !e.marks.charm && b == 1;
    !pet && !allied && (e.flags & 1 != 0 || e.marks.charm || (id != w.my_id && e.monstrosity != 0))
}

/// FUN_10083430 steps 3 / 4: /ignorepet (config 0x97) skips other players' pets (charmed, not hostile, not ours; party
/// members' too), /ignoretrust (0xaa) trusts outside our alliance. A target already held is not dropped for it.
fn ignored(w: &World, id: u32, e: &super::Entity) -> bool {
    let k = super::radar::kind_bits(w, id, e);
    let pet = w.game.get(0x97) != 0
        && k & 0x100 != 0
        && e.marks.charm
        && !e.marks.npc_pet
        && !e.marks.b2b1
        && !hostile(w, id, e)
        && !(w.my_pet != 0 && e.index == w.my_pet);
    let trust = w.game.get(0xaa) != 0 && e.marks.trust && k & 0xc == 0;
    pet || trust
}

/// FUN_10083430 as the mouse pick calls it (FUN_10083750): self too, unless a prompt's kinds leave out 0x200.
pub fn mouse_candidate(w: &World, hidden: &std::collections::HashSet<u32>, id: u32) -> bool {
    if id == w.my_id {
        return w.visible.contains(&id) && !hidden.contains(&id) && w.prompt.as_ref().is_none_or(|s| s.takes(0x200));
    }
    w.entities.get(&id).is_some_and(|e| candidate(w, hidden, id, e))
}

/// FUN_101524e0's talk test: an NPC id (high byte 1), not our pet, not a trust (+0x12c bit 22: 0x00E @0x28 bit 0),
/// charmed only when talkable (+0x12c bit 30), not allied with players (FUN_1008ca40: allegiance 1, LSB's trusts).
/// Monsters take the menu *(inferred)*. The Mog House door (0x1000000, which the test itself skips) answers by a route
/// not traced: ours sends it to event::talk, which opens its exit menu.
pub fn talks_to(w: &World, t: u32) -> bool {
    if t == super::myroom::DOOR_ID && w.myroom.is_some() {
        return true;
    }
    let Some(e) = w.entities.get(&t) else { return false };
    let m = &e.marks;
    t >> 24 == 1
        && t != 0x0100_0000
        && !e.mob
        && !m.trust
        && !(w.my_pet != 0 && e.index == w.my_pet)
        && (!m.charm || m.npc_pet)
        && !allied(w, t, e)
}

/// FUN_1008ca40: not a PC, not charmed (+0x1cc 0x100), allegiance (+0x1fb) 1, the players' side: LSB's trusts.
pub fn allied(w: &World, t: u32, e: &super::Entity) -> bool {
    let k = super::radar::kind_bits(w, t, e);
    k & 1 == 0 && k & 0x100 == 0 && e.marks.alleg == 1
}

/// FUN_10158db0(0), the confirm on the target (Enter, a click on the target): an NPC is talked to; anything else
/// (a trust too), or no target, opens the action menu ("menu    playermo").
pub fn confirm_target(w: &mut World) {
    match w.target.filter(|&t| talks_to(w, t)) {
        // FUN_10158db0: SE 0009 once the talk went out (after FUN_1007bf30).
        Some(t) => {
            if super::event::talk(w, t) {
                w.sounds.push(crate::audio::Ui::Target as u32);
            }
        }
        // SE 0011 when it was not already open (FUN_101585f0 / FUN_10158db0).
        None => {
            if w.action_menu.is_none() {
                w.sounds.push(crate::audio::Ui::ActionMenu as u32);
            }
            // Open again (FUN_1015e350 -> FUN_1012ada0): the rows reload and Call for help resets; the cursor
            // stays *(inf: +0x4c is not touched)*.
            let keep = w.action_menu.as_ref().map(|m| m.sel);
            let mut m = super::combat::open_menu(w);
            if let Some(sel) = keep {
                m.sel = sel.min(m.items.len().saturating_sub(1));
            }
            w.action_menu = Some(m);
            w.menu = None;
        }
    }
}

/// FUN_101585f0 for a mouse release (results 0x16 left / 0x17 right). Left: locked (no prompt) confirms without a
/// pick; else the actor under the pointer becomes the target / the prompt's pick, and a click that leaves it as it
/// was confirms (the prompt, the target, or with none the action menu). After a drag only the pick runs. Right:
/// cancels the prompt, clears an unlocked target, or with none stops autorun. True when it confirmed.
/// `double` (ours, the modern UI's double click on the target): instead of the action menu, the safe default runs as
/// Ctrl+C does (talk to an NPC, check a player or a monster), closing the menu the first click opened.
pub fn click(w: &mut World, picked: Option<u32>, left: bool, drag: bool, double: bool) -> bool {
    log::debug!("world click left {left} drag {drag} picked {picked:?} target {:?}", w.target);
    if !left {
        if sub_up(w) {
            cancel_sub(w);
        } else if w.target.is_some() {
            if picks_allowed(w) {
                set(w, None);
            }
        } else if w.autorun {
            w.autorun = false;
        }
        return false;
    }
    if w.locked && !sub_up(w) {
        if !drag {
            confirm_target(w);
        }
        return !drag;
    }
    let old = w.target;
    if let Some(p) = picked.filter(|p| Some(*p) != old) {
        set(w, Some(p));
    }
    if drag || w.target != old {
        return false;
    }
    if sub_up(w) {
        confirm_sub(w);
    } else if double {
        w.action_menu = None;
        super::check_target(w);
    } else {
        confirm_target(w);
    }
    true
}

/// FUN_10083620 / FUN_10083f20: the nearest candidate in front of the character (its own frame, local forward
/// >= 0) within 50 units (distance^2 < 2500), optionally of some kinds.
fn nearest(w: &World, keep: impl Fn(u32, &super::Entity) -> bool) -> Option<u32> {
    let hidden = super::event::hidden(w);
    let forward = Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos());
    w.entities
        .iter()
        .filter(|(id, e)| candidate(w, &hidden, **id, e) && keep(**id, e))
        .map(|(id, e)| (*id, e.pos - w.pos))
        .filter(|(_, d)| d.dot(forward) >= 0.0 && d.length_squared() < 2500.0)
        .min_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))
        .map(|(id, _)| id)
}

/// FUN_10083d00 / FUN_10083b40 order: from the target's screen key, the nearest key to that side (under 3600);
/// none, the far end the other way (Tab wraps to the leftmost, Shift+Tab to the rightmost).
fn screen_step(keys: &[(u32, f32)], from: f32, right: bool, wrap: bool) -> Option<u32> {
    let side = |k: f32| if right { k - from } else { from - k };
    let next =
        keys.iter().filter(|(_, k)| side(*k) > 0.0 && side(*k) < 3600.0).min_by(|a, b| side(a.1).total_cmp(&side(b.1)));
    let wrapped = || {
        keys.iter().filter(|(_, k)| wrap && side(*k) < 0.0).max_by(|a, b| side(a.1).abs().total_cmp(&side(b.1).abs()))
    };
    next.or_else(wrapped).map(|(id, _)| *id)
}

/// Key functions 0x84 / 0x97 (Tab, Num0 / with Shift): FUN_10158ca0. No valid target: the nearest in front;
/// else screen order by x_view / max(|z_view|, 1) x focal among candidates within 50 of the camera (on screen this
/// frame, so nothing behind the camera), else the nearest.
pub fn cycle(w: &mut World, view: glam::Mat4, right: bool) {
    step_screen(w, view, right, true);
}

/// The pad's field d-pad right / left (key functions 0x29 / 0x2a, FUN_10158c40): the next actor that way in Tab's
/// screen order, never wrapping. None that way, or no pick: us when the prompt takes us (+0x60 & 0x200; the field's
/// +0x60 is every bit), else nothing.
pub fn pad_step(w: &mut World, view: glam::Mat4, right: bool) {
    if !picks_allowed(w) {
        return;
    }
    let inc = w.prompt.as_ref().map_or(u32::MAX, |s| s.inc);
    let next = w.target.and_then(|t| screen_next(w, view, t, right, false));
    let pick = next.or_else(|| (inc & 0x200 != 0).then_some(w.my_id));
    if pick.is_some() && pick != w.target {
        set(w, pick);
    }
}

/// The actor after `from` in screen order (FUN_10083d00 / FUN_10083b40), wrapping or not.
fn screen_next(w: &World, view: glam::Mat4, from: u32, right: bool, wrap: bool) -> Option<u32> {
    let at = if from == w.my_id { Some(w.pos) } else { w.entities.get(&from).map(|e| e.pos) }?;
    let focal = super::camera::focal(w);
    let raw = |p: Vec3| {
        let v = view.transform_point3(p);
        (v.x / v.z.abs().max(1.0) * focal, v.length())
    };
    let key = |p: Vec3| Some(raw(p)).filter(|k| k.1 < 50.0).map(|k| k.0);
    let hidden = super::event::hidden(w);
    let keys: Vec<(u32, f32)> = w
        .entities
        .iter()
        .filter(|(id, e)| **id != from && candidate(w, &hidden, **id, e))
        .filter_map(|(id, e)| key(e.pos).map(|k| (*id, k)))
        .collect();
    screen_step(&keys, raw(at).0, right, wrap)
}

fn step_screen(w: &mut World, view: glam::Mat4, right: bool, wrap: bool) {
    if !picks_allowed(w) {
        return;
    }
    let cur = w.target.filter(|t| w.entities.contains_key(t));
    let pick = cur.and_then(|t| screen_next(w, view, t, right, wrap));
    let pick = pick.or_else(|| nearest(w, |_, _| true));
    if pick.is_some() {
        set(w, pick);
    }
}

/// Enter with no target (FUN_101587f0 case 0): the nearest in front, only targeted. False when there is none
/// (the caller opens "menu playermo", the action menu).
pub fn enter_nearest(w: &mut World) -> bool {
    let t = nearest(w, |_, _| true);
    if t.is_some() {
        set(w, t);
    }
    t.is_some()
}

/// F8 / Ctrl+F8 / Alt+F8 / F9 / F10 (FUN_10083ec0 / FUN_10083ee0 / FUN_10083f00): the nearest in front by kind.
/// `prefer` is the kind mask tried first (F8 monsters, F10 / Alt+F8 NPCs or monsters), then any non-PC; Ctrl+F8
/// (`enemies`) only monsters; F9 (`pcs`) only players.
pub fn nearest_kind(w: &mut World, prefer: u32, enemies: bool, pcs: bool) {
    if !picks_allowed(w) {
        return;
    }
    let t = if pcs {
        nearest(w, |id, _| id >> 24 == 0)
    } else if enemies {
        nearest(w, |id, e| nearest_hostile(w, id, e))
    } else {
        nearest(w, |id, e| id >> 24 != 0 && kind_bits(id, e) & prefer != 0)
            .or_else(|| nearest(w, |id, _| id >> 24 != 0))
    };
    if t.is_some() {
        set(w, t);
    }
}

/// FUN_10083f20's kind 2 (Ctrl+F8, FUN_10083f00): both on Belligerency sides (8 / 9) another side (FUN_1008e8d0); else a
/// Monipulator never, the rest by FUN_101cc220. Not modelled: two disguised actors (+0x1d0 bit 10) of one team.
fn nearest_hostile(w: &World, id: u32, e: &super::Entity) -> bool {
    let (a, b) = (w.my_team, e.marks.alleg);
    if (8..=9).contains(&a) && (8..=9).contains(&b) {
        return a != b;
    }
    e.monstrosity == 0 && hostile(w, id, e)
}

/// /targetnpc (FUN_1007a640 -> FUN_10083ee0(-1)): the nearest non-PC in front, any kind.
pub fn nearest_npc(w: &mut World) {
    if !picks_allowed(w) {
        return;
    }
    if let Some(t) = nearest(w, |id, _| id >> 24 != 0) {
        set(w, Some(t));
    }
}

/// A target key's pick (FUN_10157b40 from FUN_101587f0): nothing while locked; picking the target already held
/// plays "0010" again (FUN_10037dd0).
pub fn pick(w: &mut World, t: Option<u32>) {
    if !picks_allowed(w) || super::event::active(w) {
        return;
    }
    if t.is_some() && w.target == t {
        w.sounds.push(SE_SWITCH);
        return;
    }
    set(w, t);
}

/// FUN_102033c0 / FUN_102034a0: the party window slot's member when it is in our zone, not marked elsewhere, and
/// its model is loaded (no distance test).
pub fn slot_target(w: &World, slot: usize) -> Option<u32> {
    let m = w.party.slots().here(slot, w.pos_zone)?.clone();
    if m.id == w.my_id {
        return Some(m.id);
    }
    let id = w.entity_at(m.index)?.0;
    w.actors.get(&id).is_some_and(|a| a.character.is_some()).then_some(id)
}

/// The pad's field d-pad up / down (key functions 0x27 / 0x28, table 0x10371ee0, FUN_10158b60 -> FUN_10203920). None
/// leaves the pick as it is. Field +0x60 is Sub::inc, every bit with no prompt. FUN_10203920 steps DAT_106643dc, which
/// every pick of a party member sets to its slot (FUN_10203520), so the pick's own slot is where it starts; the slot
/// it lands on is not checked against the prompt's exclusions (FUN_10082fa0 reads only the id).
pub fn party_pad_step(w: &World, down: bool) -> Option<u32> {
    let inc = w.prompt.as_ref().map_or(u32::MAX, |s| s.inc);
    let slots: Vec<Option<u32>> = (0..18).map(|s| slot_target(w, s)).collect();
    pad_party_step(&slots, w.target, w.my_id, inc, down)
}

/// FUN_10158b60 over the 18 alliance slots (`slots`, slot 0 us): the step from `pick` under the prompt's `inc`.
fn pad_party_step(slots: &[Option<u32>], pick: Option<u32>, me: u32, inc: u32, down: bool) -> Option<u32> {
    // Without 8 only our six count (FUN_10203500).
    let party_only = inc & 8 == 0;
    let n = slots.len();
    let fits = |s: usize| slots[s].filter(|_| !(party_only && s > 5));
    // FUN_10203920 walks all 18 slots to the next occupied one, wrapping; a pick outside them (or outside our six
    // when party_only) gives nothing.
    let next =
        |at: usize| (1..=n).map(|i| if down { (at + i) % n } else { (at + n - i) % n }).find(|&s| slots[s].is_some());
    // Only with 4 and a pick does it step at all (0x10158b7d..0x10158b83); either missing leaves the result 0, which
    // the 0x200 rule below still turns into us, so with no target and no prompt up / down pick us.
    let at = pick
        .filter(|_| inc & 4 != 0)
        .and_then(|p| slots.iter().position(|s| *s == Some(p)))
        .filter(|&at| fits(at).is_some());
    let mut to = at.and_then(next);
    let mut found = to.and_then(fits);
    // Without 0x200, landing on us steps once more; us again is nothing.
    if inc & 0x200 == 0 && found == Some(me) {
        to = to.and_then(next);
        found = to.and_then(fits).filter(|&id| id != me);
    }
    // With 0x200, nothing becomes us (0x10158c20: ECX 0x1047d600, our actor).
    found.or_else(|| (inc & 0x200 != 0).then_some(me))
}

/// FUN_101514a0: while engaged with no target, the first attackable actor we claimed that is alive.
fn auto_target(w: &World) -> Option<u32> {
    let alive = |e: &super::Entity| {
        e.flags & 1 != 0 && e.claim == w.my_id && !e.untargetable && !e.target_off && !matches!(e.motion.status, 2 | 3)
    };
    w.entities
        .iter()
        .filter(|(id, e)| alive(e) && w.actors.get(id).is_some_and(|a| a.gpu.is_some()))
        .min_by_key(|(_, e)| e.index)
        .map(|(id, _)| *id)
}

/// FUN_10151100 -> FUN_101516d0 / FUN_10151910: Numpad * is /heal with no target or while healing
/// (status 0x21), otherwise the lock-on toggle.
pub fn numpad_star(w: &mut World) {
    if super::event::active(w) {
        return;
    }
    if w.my_motion.status == 0x21 || w.target.is_none() {
        w.sounds.push(Ui::Select as u32);
        super::send_chat(w, "/heal".into());
    } else {
        let on = !w.locked;
        lock(w, on, "Numpad *");
    }
}

/// What LAB_1009e570 does with an s2c 0x058: None = nothing (not about us, no target, or no such actor with a
/// model, +0x120 bit 9), else Some(lock): set the target, and lock when it is a monster (+0x120 bit 13, inferred
/// the 0x00E monster bit) and auto lock-on is on (+0x51, config 7 = 0, the default).
fn assist_rule(player: u32, me: u32, target: u32, modelled: bool, monster: bool, auto_lock: bool) -> Option<bool> {
    (player == me && target != 0 && modelled).then_some(monster && auto_lock)
}

/// s2c 0x058 (LSB sends it on engage, on a target change and on auto-target): our assist target.
pub fn assist(w: &mut World, player: u32, target: u32) {
    let modelled = w.entities.contains_key(&target)
        && w.actors.get(&target).is_some_and(|a| a.gpu.is_some() || a.character.is_some());
    let monster = w.entities.get(&target).is_some_and(|e| e.mob);
    let rule = assist_rule(player, w.my_id, target, modelled, monster, w.auto_lock);
    log::info!("0x058 assist {target:#x}: {rule:?} (monster {monster}, auto lock-on {})", w.auto_lock);
    let Some(locks) = rule else { return };
    set(w, Some(target));
    if locks {
        lock(w, true, "engaged, auto lock-on");
    }
}

/// Per frame (FUN_10151100): own death unlocks (FUN_10150e30); leaving the engaged status (not by dying)
/// unlocks and keeps the target (FUN_10089d50 case 3); an invalid target goes at once with the lock; engaged
/// with no target picks one (FUN_10151910).
pub fn update(w: &mut World) {
    let st = w.my_motion.status;
    if w.locked && (w.dead() || (w.last_status == 1 && st != 1 && st != 3)) {
        lock(w, false, if w.dead() { "dead" } else { "disengaged" });
    }
    w.last_status = st;
    if let Some(attackoff) = w.target.and_then(|t| invalid(w, t)) {
        log::info!("target {:#x} dropped (attackoff {attackoff})", w.target.unwrap_or(0));
        lock(w, false, "target dropped");
        w.target = None;
        super::inspect::drop(w);
        if attackoff {
            super::send_chat(w, "/attackoff".into());
        }
    }
    if st == 1 && w.target.is_none() && !super::event::active(w) {
        let t = auto_target(w);
        if t.is_some() {
            set(w, t);
        }
    }
    if w.locked && (w.target.is_none() || super::event::active(w)) {
        lock(w, false, "no target or an event");
    }
}

/// The camera's eye and look-at (camera object +0x44 / +0x50).
#[derive(Clone, Copy, Debug, Default)]
pub struct LockCam {
    pub eye: Vec3,
    pub look: Vec3,
}

/// An attach point's height above the feet (y down, so negative), from the posed bones when they are posed, else
/// the rest pose; 0.7 of the model height without points (inferred).
fn point_height(w: &World, id: u32, k: usize) -> f32 {
    let Some(a) = w.actors.get(&id) else { return -1.5 };
    let point = a.character.as_ref().and_then(|c| c.skeleton.points.get(k).copied());
    let bones = if a.bones.is_empty() { &a.rest } else { &a.bones };
    point.and_then(|(b, off)| bones.get(b as usize).map(|x| x.at(off).y * a.scale)).unwrap_or(a.top * 0.7)
}

/// FUN_100d6670 (vtable +0x3f8 of the target, from the lock callback): the posed point init's op 0xad set (+0x8a8),
/// else by the model kind: a model-id body (kinds 2 / 7) with leg mode 1 or 2 (+0x878) point 5 x 0.8 (_DAT_10329a34),
/// any other point 7. Taken at lock-on, never re-read while locked, so a bobbing flyer doesn't move the camera.
fn lock_height(w: &World, id: u32) -> f32 {
    let Some(a) = w.actors.get(&id) else { return -1.5 };
    if a.anchors[2] != 0x80 {
        return point_height(w, id, a.anchors[2] as usize);
    }
    let model = matches!(a.source, Some(super::Source::Npc(crate::dat::npc::NpcLook::Model(_))));
    let legs = a.character.as_ref().map_or(-1, |c| c.cib[0]);
    if model && matches!(legs, 1 | 2) { point_height(w, id, 5) * 0.8 } else { point_height(w, id, 7) }
}

/// Whether the actor's bones are this frame's pose (else `point_height` reads the rest pose).
fn posed(w: &World, id: u32) -> bool {
    w.actors.get(&id).is_some_and(|a| !a.bones.is_empty())
}

/// FUN_1001e030, locked branch, run in 1/60 s ticks: look at the player / target midpoint, keep the eye
/// within atan(4 / (near + d)) of the target-to-player line (x3 only in Monstrosity, all.c 23071..23080) and ease its distance into
/// [near + d/2, far + d/2], 25 % of the gap a tick. `turn` is manual orbit input in radians.
/// The camera actor's box (vtable +0x3c8 (1): skeleton box 1) when its model kind is 2 or 7 (FUN_10084400): the wider
/// of x and z, x 1.5 (_DAT_1032a3ac); ours a model-id look.
fn mon_extent(w: &World) -> f32 {
    let Some(a) = w
        .actors
        .get(&w.my_id)
        .filter(|a| matches!(a.source, Some(super::Source::Npc(crate::dat::npc::NpcLook::Model(_)))))
    else {
        return 0.0;
    };
    let Some(b) = a.character.as_ref().and_then(|c| c.skeleton.boxes.get(1).copied()) else { return 0.0 };
    (b[4] - b[5]).max(b[2] - b[3]) * 1.5
}

/// Our bones are posed only once locked: the height snapshot is taken again from the first posed frame (retail reads the
/// posed point at lock-on); the look's easing hides the change.
pub(super) fn refresh_lock_height(w: &mut World) {
    if let Some(t) = w.target.filter(|_| w.locked && !w.lock_posed)
        && posed(w, t)
    {
        (w.lock_height, w.lock_posed) = (lock_height(w, t), true);
    }
}

pub fn step_camera(w: &mut World, ticks: f32, turn: f32, rise: f32) {
    let Some(t) = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.pos))) else { return };
    refresh_lock_height(w);
    let mut c = w.cam;
    let (_, tpos) = t;
    let me = super::drawn_pos(w);
    // The player's attach point 13, posed, every frame; the target's height snapshot.
    let p = me + Vec3::new(0.0, point_height(w, w.my_id, 13), 0.0);
    let q = tpos + Vec3::new(0.0, w.lock_height, 0.0);
    let l = p.lerp(q, 0.5);
    let k = 1.0 - 0.75f32.powf(ticks);
    c.look += (l - c.look) * k;
    let d = (me - tpos).length();
    let e = (me - tpos).normalize_or(Vec3::X);
    let mut dv = c.eye - c.look;
    // Manual orbit, scaled by 6 / |D| when locked (_DAT_1032a3e8 / max(|eye - look|, 0.01)).
    if turn != 0.0 {
        let len = dv.length().max(0.01);
        dv = glam::Quat::from_rotation_y(turn * 6.0 / len) * dv;
    }
    dv.y += rise;
    // near / far by the current focal length (FUN_100145d0, config 0x49). In Monstrosity (FUN_1001ee60, all.c 23040)
    // a single-model body adds its engaged box's wider side x 1.5, and the cone is three times as wide.
    let f = super::camera::focal(w);
    let mon = super::monstrosity::active(w);
    let ext = if mon { mon_extent(w) } else { 0.0 };
    let near = (7.2 + 0.125 * d + ext) * f * 0.001;
    let far = (8.6 + 0.125 * d + ext) * f * 0.001;
    let allowed = (4.0 / (near + d)).atan() * if mon { 3.0 } else { 1.0 };
    let len = dv.length().max(1e-3);
    let dir = dv / len;
    let angle = dir.dot(e).clamp(-1.0, 1.0).acos();
    let dir = if angle > allowed {
        // FUN_10032600: rotate toward E by the excess (about D x E; inferred).
        let axis = dir.cross(e).normalize_or(Vec3::Y);
        glam::Quat::from_axis_angle(axis, angle - allowed) * dir
    } else {
        dir
    };
    let goal = len.clamp(near + d / 2.0, far + d / 2.0);
    let len = len + (goal - len) * k;
    let mut eye = c.look + dir * len;
    // FUN_101815b0: a ray from the player (y - 1) to the eye; on a hit the eye goes 0.2 in front, toward the player.
    // Then the shared tail without its final ray (the lock keeps the unlock hold at 20).
    let from = me + Vec3::new(0.0, -1.0, 0.0);
    if let Some(g) = w.ground.as_ref().filter(|g| !g.is_empty()) {
        if let Some(h) = g.cam_ray(from, eye) {
            eye = h + (h - eye).normalize_or_zero() * 0.2;
        }
        eye = super::camera::tail(g, eye, c.look, c.eye, false, false, ticks);
    }
    c.eye = eye;
    w.cam = c;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ground cursor's d-pad: up away from the camera, right to the screen's right (as walking's
    /// forward.cross(NEG_Y)), whatever the camera's height.
    #[test]
    fn ground_cursor_turns_with_the_camera() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
        let look = Vec3::new(0.0, 0.3, 2.0);
        assert!(near(cursor_step(glam::Vec2::Y, look), Vec3::Z));
        assert!(near(cursor_step(glam::Vec2::X, look), Vec3::X));
        let look = Vec3::new(3.0, -1.0, 0.0);
        let fwd = Vec3::X;
        assert!(near(cursor_step(glam::Vec2::Y, look), fwd));
        assert!(near(cursor_step(glam::Vec2::X, look), fwd.cross(Vec3::NEG_Y)));
        assert!(near(cursor_step(glam::Vec2::NEG_X, look), -fwd.cross(Vec3::NEG_Y)));
    }

    /// Tab steps to the next screen key to the right and wraps to the leftmost; Shift+Tab mirrors it.
    #[test]
    fn screen_order() {
        let keys = [(1, -200.0), (2, -50.0), (3, 40.0), (4, 300.0)];
        assert_eq!(screen_step(&keys, 0.0, true, true), Some(3));
        assert_eq!(screen_step(&keys, 0.0, false, true), Some(2));
        assert_eq!(screen_step(&keys, 500.0, true, true), Some(1));
        assert_eq!(screen_step(&keys, -500.0, false, true), Some(4));
        assert_eq!(screen_step(&[], 0.0, true, true), None);
        // The pad's d-pad stops at the edge.
        assert_eq!(screen_step(&keys, 500.0, true, false), None);
        assert_eq!(screen_step(&keys, -500.0, false, false), None);
    }

    /// Attackable targets go when dead, despawning or flagged off (with /attackoff); NPCs only when hidden or
    /// more than 50 away.
    #[test]
    fn target_drop_rules() {
        assert_eq!(drop_rule(1, false, 0, 10.0, false), None);
        assert_eq!(drop_rule(1, false, 3, 10.0, false), Some(false));
        assert_eq!(drop_rule(1, false, 2, 10.0, false), Some(false));
        assert_eq!(drop_rule(1, true, 0, 10.0, false), Some(true));
        assert_eq!(drop_rule(0, true, 3, 10.0, false), None);
        assert_eq!(drop_rule(0, false, 0, 50.5, false), Some(false));
        assert_eq!(drop_rule(0, false, 0, 1.0, true), Some(false));
    }

    /// Field d-pad up / down (FUN_10158b60): with no target and no prompt they pick us, then step the party from
    /// there, wrapping; a pick outside the party also goes to us; a prompt without 0x200 skips us.
    #[test]
    fn dpad_party_step() {
        let me = 1;
        let mut slots = vec![None; 18];
        (slots[0], slots[1], slots[2]) = (Some(me), Some(2), Some(3));
        let all = u32::MAX;
        assert_eq!(pad_party_step(&slots, None, me, all, true), Some(me));
        assert_eq!(pad_party_step(&slots, None, me, all, false), Some(me));
        assert_eq!(pad_party_step(&slots, Some(me), me, all, true), Some(2));
        assert_eq!(pad_party_step(&slots, Some(me), me, all, false), Some(3));
        assert_eq!(pad_party_step(&slots, Some(3), me, all, true), Some(me));
        assert_eq!(pad_party_step(&slots, Some(0x0106_5013), me, all, true), Some(me));
        // A prompt without 4 steps nothing, but 0x200 still gives us.
        assert_eq!(pad_party_step(&slots, Some(2), me, 0x200, true), Some(me));
        // A party-only prompt that cannot take us: no pick is nothing, landing on us steps past it.
        assert_eq!(pad_party_step(&slots, None, me, 4, true), None);
        assert_eq!(pad_party_step(&slots, Some(3), me, 4, true), Some(2));
        assert_eq!(pad_party_step(&slots, Some(2), me, 4, false), Some(3));
        // Alone in such a prompt: us again is nothing.
        let alone: Vec<Option<u32>> = (0..18).map(|s| (s == 0).then_some(me)).collect();
        assert_eq!(pad_party_step(&alone, Some(me), me, 4, true), None);
    }

    /// Engaging with auto lock-on at its default (config 7 = 0) targets and locks a monster; config 7 = 1
    /// only targets; NPCs are targeted without a lock; another player's 0x058, a zero target or an actor
    /// without a model do nothing.
    #[test]
    fn engage_locks_by_default() {
        let me = 0x0100_0004;
        let auto = crate::gamecfg::Values::default().get(7) == 0;
        assert!(auto, "config 7 defaults to 0 = auto lock-on");
        assert_eq!(assist_rule(me, me, 0x0106_5013, true, true, auto), Some(true));
        assert_eq!(assist_rule(me, me, 0x0106_5013, true, true, false), Some(false));
        assert_eq!(assist_rule(me, me, 0x0106_5239, true, false, auto), Some(false));
        assert_eq!(assist_rule(0x0100_0099, me, 0x0106_5013, true, true, auto), None);
        assert_eq!(assist_rule(me, me, 0, true, true, auto), None);
        assert_eq!(assist_rule(me, me, 0x0106_5013, false, true, auto), None);
    }

    /// Side input keeps the radius (circle strafe); forward closes in, stopping at 0.8.
    #[test]
    fn circle_strafe() {
        let t = Vec3::new(10.0, 0.0, 10.0);
        let p = Vec3::new(10.0, 0.0, 5.0);
        let q = locked_step(p, t, 0.0, 1.0, 0.5);
        assert!(((q - t).length() - 5.0).abs() < 1e-4);
        assert!((q - p).length() > 0.4);
        let r = locked_step(p, t, 1.0, 0.0, 10.0);
        assert!(((r - t).length() - 0.8).abs() < 1e-4);
    }
}

/// FUN_100a6210: locked movement relative to the target. Side input circles it at the same radius,
/// forward / back changes the radius, stopping at the two radii (inferred 0.8 together).
pub fn locked_step(pos: Vec3, target: Vec3, fwd: f32, side: f32, dist: f32) -> Vec3 {
    let r = Vec3::new(pos.x - target.x, 0.0, pos.z - target.z);
    let radius = r.length().max(1e-3);
    let facing = -r / radius;
    let right = facing.cross(Vec3::NEG_Y);
    let moved = r + right * side * dist;
    let new_r = (radius - fwd * dist).max(0.8);
    let p = target + moved.normalize_or(r / radius) * new_r;
    Vec3::new(p.x, pos.y, p.z)
}
