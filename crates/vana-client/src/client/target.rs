//! Targeting and lock-on (target object DAT_10565d34, FUN_10151280 / FUN_10150da0), Numpad * (key
//! functions 0x44 / 0x30), the s2c 0x058 assist target, and the locked camera of FUN_1001e030.

use super::World;
use crate::audio::Ui;
use glam::Vec3;

/// SE "0009" (FUN_10036f50): a target from none; "0010" (FUN_10036fb0): switching targets.
const SE_TARGET: u32 = 9;
const SE_SWITCH: u32 = 10;
/// Camera focal length, config 0x49 default 350 (range 242..900).
pub const FOCAL: f32 = 350.0;

/// The forced flash (actor +0x88 bit 0x200) for a frame, as the sub-target cursor and area previews set it,
/// unless /targetcharaeffect turned it off (config 0x99 bit 7).
/// FUN_1021f030: the nearest (horizontal distance) monster with a model, called for help on (+0x127 bit 0, ours
/// marks.yell) and not dead; the compass's untracked pointer and `<ht>`. No range, claim or party test.
pub fn call_for_help(w: &World) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    for (id, e) in &w.entities {
        if !(e.mob && e.marks.yell && !matches!(e.motion.status, 2 | 3) && w.actors.get(id).is_some_and(|a| a.gpu.is_some())) {
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
}

/// The party-list cursor of `<stpt>` / `<stal>` (FUN_10202d80, DAT_106643e0): a window slot below `count` (6 / 18)
/// over the used slots; no candidate filter, the world target and lock untouched. The fellow row (-1) is not modelled.
pub struct PartyCursor {
    pub index: usize,
    pub count: usize,
    pub then: SubThen,
    pub range: Option<f32>,
}

/// "Sub-target up" (T+0x59 || T+0xc4): the 3D prompt or the party-list cursor.
pub fn sub_up(w: &World) -> bool {
    w.prompt.is_some() || w.party_cursor.is_some()
}

/// FUN_10202d80: the cursor opens on the current target's slot when it is below `count`, else on us (slot 0).
pub fn open_party_cursor(w: &mut World, count: usize, then: SubThen, range: Option<f32>) {
    let slots = w.party.slots();
    let index = w.target.and_then(|t| slots.slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == t))).filter(|&i| i < count).unwrap_or(0);
    w.party_cursor = Some(PartyCursor { index, count, then, range });
}

/// FUN_10203570 / FUN_10203630: the next / previous used slot below the count, wrapping.
fn party_step(used: &[bool], index: usize, count: usize, down: bool) -> usize {
    let n = count.min(used.len()).max(1);
    let mut i = index;
    for _ in 0..n {
        i = if down { (i + 1) % n } else { (i + n - 1) % n };
        if used[i] {
            return i;
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
    let Some(pc) = w.party_cursor.as_mut() else { return false };
    match key {
        K::ArrowDown | K::ArrowUp => {
            let i = party_step(&used, pc.index, pc.count, key == K::ArrowDown);
            if i != pc.index {
                pc.index = i;
                w.sounds.push(crate::audio::Ui::Cursor as u32);
            }
        }
        K::Enter => {
            let pc = w.party_cursor.take().unwrap();
            w.sounds.push(crate::audio::Ui::Select as u32);
            let (id, name) = match pc.index {
                0 => (w.my_id, w.name.clone()),
                i => match w.party.slots().slots.get(i).cloned().flatten() {
                    Some(m) => (m.id, m.name),
                    None => return true,
                },
            };
            let line = match &pc.then {
                SubThen::Line { stash, quoted: true, .. } => format!("{stash}\"{name}\""),
                SubThen::Line { stash, .. } => format!("{stash}{id}"),
                _ => return true,
            };
            super::textcmd::run(w, &line, super::textcmd::Mode::SubTarget);
        }
        K::Escape => {
            let pc = w.party_cursor.take().unwrap();
            w.sounds.push(crate::audio::Ui::Cancel as u32);
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

/// FUN_10203c80: a slot that emptied under the cursor steps it back to the previous used one (0 at the least).
pub fn party_cursor_update(w: &mut World) {
    let used: Vec<bool> = w.party.slots().slots.iter().map(|m| m.is_some()).collect();
    if let Some(pc) = w.party_cursor.as_mut() {
        while pc.index > 0 && !used.get(pc.index).copied().unwrap_or(false) {
            pc.index -= 1;
        }
    }
}

/// FUN_10202110: the party window's hand: the cursor's slot, tinted by range from base 0xa04040d0 (red 0xe0d04040,
/// yellow 0xe0d0d040; flashing the pick unless red), or with no cursor the current target's slot in 0x80808080. The
/// slot, its tint and the entity to flash.
pub fn party_hand(w: &World) -> Option<(usize, u32, Option<u32>)> {
    let slots = w.party.slots();
    match &w.party_cursor {
        Some(pc) => {
            let id = match pc.index {
                0 => Some(w.my_id),
                i => {
                    let m = slots.slots.get(i).cloned().flatten()?;
                    w.entities.iter().find(|(_, e)| e.index == m.index).map(|(id, _)| *id)
                }
            };
            let tint = match (id, pc.range) {
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
            };
            Some((pc.index, tint, id.filter(|_| tint != 0xe0d0_4040)))
        }
        None => {
            let t = w.target?;
            let i = if t == w.my_id { 0 } else { slots.slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == t))? };
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
}

impl Sub {
    fn takes(&self, kind: u32) -> bool {
        self.inc & kind != 0 && self.exc & kind == 0
    }
}

/// Table 0x103720c0: a DAT valid-target flag's include / exclude kinds (FUN_10159170).
const SUB_FLAGS: [(u32, u32, u32); 8] = [(1, 0x200, 0), (2, 0x100, 0), (4, 4, 0), (8, 8, 0), (0x10, 1, 0), (0x20, 0x10, 0), (0x40, 2, 0), (0x80, 2, 0x1800)];

/// FUN_1008e290's kind bits: 0x200 self, 1 PC, 4 party, 8 alliance, 0x10 monster, 2 NPC, 0x800 our pet, 0x1000
/// others' pets and trusts (0x700.., inf for +0x12c bit 22). Not done: 0x100 ally NPCs, 0x400 allegiance.
fn sub_kind(w: &World, id: u32) -> u32 {
    if id == w.my_id {
        return 0x200 | 1 | 4;
    }
    let Some(e) = w.entities.get(&id) else { return 0 };
    let party = w.party.members().find(|m| m.id == id).map_or(0, |m| if m.flags & 3 == w.party.own_flags & 3 { 4 } else { 8 });
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
    let sub = Sub { inc, exc, saved: w.target, saved_lock: w.locked, then, range, title, help };
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
    w.locked = false;
    w.target = Some(first);
    w.target_since = std::time::Instant::now();
    true
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
    if let SubThen::Command(line) = &s.then {
        super::textcmd::run(w, line, super::textcmd::Mode::Internal);
        restore(w, &s);
        return;
    }
    let line = match &s.then {
        SubThen::Item { .. } | SubThen::Command(_) => String::new(),
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
    let mode = if matches!(s.then, SubThen::Line { .. }) { super::textcmd::Mode::SubTarget } else { super::textcmd::Mode::Internal };
    super::textcmd::run(w, &line, mode);
}

/// FUN_10158ff0: the cancel callback (the list comes back) and the saved target and lock.
pub fn cancel_sub(w: &mut World) {
    let Some(s) = w.prompt.take() else { return };
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

/// FUN_10159040.
fn restore(w: &mut World, s: &Sub) {
    w.target = s.saved.filter(|&t| t == w.my_id || w.entities.contains_key(&t));
    w.target_since = std::time::Instant::now();
    w.locked = s.saved_lock && w.target.is_some();
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

/// FUN_10150610: the prompt arrow's tint by distance from us against the range (y ignored): red past range + 0.5 or
/// at 25 and more, yellow past range - 0.5, else blue. Not done: the model radii (+0x208) the spell record adds.
pub fn range_tint(w: &World, t: u32) -> u32 {
    let Some(r) = w.prompt.as_ref().and_then(|s| s.range) else { return 0xe040_40d0 };
    let p = if t == w.my_id { w.pos } else { w.entities.get(&t).map_or(w.pos, |e| e.pos) };
    let raw = (p - w.pos).length();
    let d = Vec3::new(p.x - w.pos.x, 0.0, p.z - w.pos.z).length();
    if d > r + 0.5 || raw >= 25.0 {
        0xe0d0_4040
    } else if d > r - 0.5 {
        0xe0d0_d040
    } else {
        0xe040_40d0
    }
}

/// FUN_10151280: sets the target with its sound; a different target ends the lock (FUN_10153040).
pub fn set(w: &mut World, t: Option<u32>) {
    if w.target == t {
        return;
    }
    match (w.target, t) {
        (None, Some(_)) => w.sounds.push(SE_TARGET),
        (Some(_), Some(_)) => w.sounds.push(SE_SWITCH),
        _ => {}
    }
    log::info!("target {:#x} {:?}", t.unwrap_or(0), t.and_then(|t| w.entities.get(&t)).map(|e| e.name.as_str()));
    w.target = t;
    w.target_since = std::time::Instant::now();
    if w.locked && w.prompt.is_none() {
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
fn kind_bits(id: u32, e: &super::Entity) -> u32 {
    if id >> 24 == 0 {
        1
    } else if e.door != 0 {
        0x20
    } else if e.mob {
        0x10
    } else {
        2
    }
}

/// FUN_10083430: drawn on screen this frame (+0xb2 bit 0 clear: World::visible), not hidden, not ourselves, not a
/// dead or despawning monster, and in a prompt's include / exclude kinds (T+0x60 / +0x64). No name test. Not done:
/// the event untargetable bit (ops 0x12..0x15, +0x128 bit 4) and the occlusion part of the cull.
fn candidate(w: &World, hidden: &std::collections::HashSet<u32>, id: u32, e: &super::Entity) -> bool {
    id != w.my_id && w.visible.contains(&id) && !hidden.contains(&id) && !(e.flags & 1 != 0 && matches!(e.motion.status, 2 | 3)) && w.prompt.as_ref().is_none_or(|s| s.takes(sub_kind(w, id)))
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
fn screen_step(keys: &[(u32, f32)], from: f32, right: bool) -> Option<u32> {
    let side = |k: f32| if right { k - from } else { from - k };
    let next = keys.iter().filter(|(_, k)| side(*k) > 0.0 && side(*k) < 3600.0).min_by(|a, b| side(a.1).total_cmp(&side(b.1)));
    let wrap = || keys.iter().filter(|(_, k)| side(*k) < 0.0).max_by(|a, b| side(a.1).abs().total_cmp(&side(b.1).abs()));
    next.or_else(wrap).map(|(id, _)| *id)
}

/// Key functions 0x84 / 0x97 (Tab, Num0 / with Shift): FUN_10158ca0. No valid target: the nearest in front;
/// else screen order by x_view / max(|z_view|, 1) x focal among candidates within 50 of the camera (on screen this
/// frame, so nothing behind the camera), else the nearest.
pub fn cycle(w: &mut World, view: glam::Mat4, right: bool) {
    if !picks_allowed(w) {
        return;
    }
    let focal = super::camera::focal(w);
    let raw = |p: Vec3| {
        let v = view.transform_point3(p);
        (v.x / v.z.abs().max(1.0) * focal, v.length())
    };
    let cur = w.target.filter(|t| w.entities.contains_key(t));
    let pick = cur.and_then(|t| {
        let from = raw(w.entities[&t].pos).0;
        let key = |p: Vec3| Some(raw(p)).filter(|k| k.1 < 50.0).map(|k| k.0);
        let hidden = super::event::hidden(w);
        let keys: Vec<(u32, f32)> = w.entities.iter().filter(|(id, e)| **id != t && candidate(w, &hidden, **id, e)).filter_map(|(id, e)| key(e.pos).map(|k| (*id, k))).collect();
        screen_step(&keys, from, right)
    });
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
        nearest(w, |id, e| id >> 24 != 0 && e.mob)
    } else {
        nearest(w, |id, e| id >> 24 != 0 && kind_bits(id, e) & prefer != 0).or_else(|| nearest(w, |id, _| id >> 24 != 0))
    };
    if t.is_some() {
        set(w, t);
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
    let id = w.entities.iter().find(|(_, e)| e.index == m.index).map(|(id, _)| *id)?;
    w.actors.get(&id).is_some_and(|a| a.character.is_some()).then_some(id)
}

/// FUN_101514a0: while engaged with no target, the first attackable actor we claimed that is alive.
fn auto_target(w: &World) -> Option<u32> {
    let alive = |e: &super::Entity| e.flags & 1 != 0 && e.claim == w.my_id && !e.untargetable && !matches!(e.motion.status, 2 | 3);
    w.entities.iter().filter(|(id, e)| alive(e) && w.actors.get(id).is_some_and(|a| a.gpu.is_some())).min_by_key(|(_, e)| e.index).map(|(id, _)| *id)
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
    let modelled = w.entities.contains_key(&target) && w.actors.get(&target).is_some_and(|a| a.gpu.is_some() || a.character.is_some());
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
    point.and_then(|(b, off)| bones.get(b as usize).map(|(q, t)| (*t + *q * off).y * a.scale)).unwrap_or(a.top * 0.7)
}

/// FUN_1001ee60's target height: a snapshot at lock-on of the actor's look point (+0x8a8: point 7 for PCs and
/// monsters without an init op 0xad), x0.8 when we are a Tarutaru. Never re-read while locked, so a bobbing flyer
/// doesn't move the camera.
fn lock_height(w: &World, id: u32) -> f32 {
    let y = point_height(w, id, 7);
    if matches!(w.my_race, 5 | 6) { y * 0.8 } else { y }
}

/// FUN_1001e030, locked branch, run in 1/60 s ticks: look at the player / target midpoint, keep the eye
/// within 3 atan(4 / (near + d)) of the target-to-player line and ease its distance into
/// [near + d/2, far + d/2], 25 % of the gap a tick. `turn` is manual orbit input in radians.
pub fn step_camera(w: &mut World, ticks: f32, turn: f32, rise: f32) {
    let Some(t) = w.target.and_then(|t| w.entities.get(&t).map(|e| (t, e.pos))) else { return };
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
    let near = (7.2 + 0.125 * d) * FOCAL * 0.001;
    let far = (8.6 + 0.125 * d) * FOCAL * 0.001;
    let allowed = (4.0 / (near + d)).atan() * 3.0;
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
    // FUN_1017a6b0: a ray from the player (y - 1) to the eye; on a hit the eye goes 0.2 in front.
    let from = me + Vec3::new(0.0, -1.0, 0.0);
    if let Some(hit) = w.ground.as_ref().and_then(|g| g.raycast(from, eye)) {
        eye = from + (eye - from).normalize_or(Vec3::X) * (hit - 0.2).max(0.2);
    }
    // FUN_1001ee60's shared tail, after both branches: 0.17 clear of the floor below (within 10), and the eye never
    // passes through a wall: a sweep from where it was stops 0.2 short (as camera.rs does for the free camera).
    if let Some(g) = w.ground.as_ref().filter(|g| !g.is_empty()) {
        if let Some(f) = g.floor(eye.x, eye.z, eye.y, 0.0).filter(|f| f - eye.y <= 10.0)
            && eye.y > f - 0.17
        {
            eye.y = f - 0.17;
        }
        if g.raycast(from, c.eye).is_none()
            && let Some(t) = g.raycast(c.eye, eye)
        {
            eye = c.eye + (eye - c.eye).normalize_or(Vec3::X) * (t - 0.2).max(0.0);
        }
    }
    c.eye = eye;
    w.cam = c;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tab steps to the next screen key to the right and wraps to the leftmost; Shift+Tab mirrors it.
    #[test]
    fn screen_order() {
        let keys = [(1, -200.0), (2, -50.0), (3, 40.0), (4, 300.0)];
        assert_eq!(screen_step(&keys, 0.0, true), Some(3));
        assert_eq!(screen_step(&keys, 0.0, false), Some(2));
        assert_eq!(screen_step(&keys, 500.0, true), Some(1));
        assert_eq!(screen_step(&keys, -500.0, false), Some(4));
        assert_eq!(screen_step(&[], 0.0, true), None);
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
