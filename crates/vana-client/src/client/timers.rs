//! Timers (an Extra, not retail: the user's Ashita "Timers" add-on built in). Our recasts (0x119 and our own casts),
//! the effects on us (0x037 / 0x063 end times) and the effects we put on others: started by the 0x028 results that
//! say they landed, ended by the server's wear-off, dispel and defeat messages and the party's 0x076 lists; how long
//! they last is LSB's own reckoning (durations.rs). One framed window docked at a screen edge that slides in and out
//! like a drawer; its sections fold.

use super::World;
use super::game_ui::GameUi;
use crate::dat::names::NameSource;
use crate::net::action::Action;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Extras "Timers Position".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dock {
    /// Left edge, under the cast bar and the status icons, down to the compass.
    Left,
    /// Right edge, above the party, target and alliance windows, up to the main menu's foot.
    Right,
    /// Top, beside the status icons, under the help bar.
    Top,
}

impl Dock {
    pub fn of(v: u32) -> Self {
        match v {
            1 => Self::Right,
            2 => Self::Top,
            _ => Self::Left,
        }
    }
}

/// The sections in order (a fold bit each).
pub const SECTIONS: [&str; 3] = ["Recasts", "Buffs", "Debuffs"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    None,
    Status(u16),
    /// A magic / ability list icon (library, sprite).
    List(&'static str, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub icon: Icon,
    pub name: String,
    /// Seconds left; at or below 0 the predicted end has passed and the server's word is awaited.
    pub left: f32,
    /// The whole length (0: unknown, no gauge).
    pub total: f32,
    /// Recasts fill toward ready; effects drain.
    pub fills: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: Option<String>,
    pub rows: Vec<Row>,
}

/// What put an effect on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Spell(u16),
    Ability(u16),
    Ws(u16),
    /// Our avatar's or wyvern's move (category 13: an ability id).
    Pet(u16),
    /// Our jug pet's, luopan's or automaton's move (category 11: a mob skill id).
    MobSkill(u16),
    /// Our weapon's additional effect (the status itself).
    Proc(u16),
}

/// An effect we put on someone.
#[derive(Clone, Debug)]
pub struct Applied {
    pub status: u16,
    pub name: String,
    pub start: Instant,
    /// LSB's length for it (None on ourselves: the server's own times are shown).
    pub secs: Option<f32>,
    /// The server will tell us when it ends (0x029 to its origin); else it ends at `secs`.
    pub told: bool,
    /// Song tiers (copies of one song effect differ by tier).
    pub tier: u8,
}

/// Someone carrying our effects: the name kept from when they landed (for when they are out of range).
#[derive(Clone, Debug, Default)]
pub struct Bearer {
    pub name: String,
    pub mob: bool,
    pub effects: Vec<Applied>,
}

impl Bearer {
    fn left(&self, status: u16, now: Instant) -> Option<f32> {
        self.effects.iter().filter(|e| e.status == status).find_map(|e| e.secs.map(|s| s - now.duration_since(e.start).as_secs_f32()))
    }
}

/// How long a row stays after its predicted end while the server's word is awaited.
const GRACE: f32 = 3.0;
/// A party member's 0x076 list may be older than an effect that just landed.
const LIST_LAG: f32 = 3.0;
/// Our own effects that wore off this long ago still count for the action that used them (spec 1.1).
const LOST_FOR: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Drawer,
    Section(usize),
}

/// The window's hot areas.
pub type Hits = Vec<(Hit, [f32; 4])>;

pub struct Timers {
    pub on: HashMap<u32, Bearer>,
    /// The longest time left seen per recast and per status of ours: the gauge's full length.
    totals: HashMap<(u8, u32), f32>,
    /// The drawer is out; `slide` runs 0 (in) .. 1 (out).
    pub out: bool,
    slide: f32,
    /// Folded sections, a bit each.
    pub folded: u8,
    last: Option<Instant>,
    /// Last frame's window and its hot areas.
    rect: Option<[f32; 4]>,
    hits: Hits,
    /// The drawer or a fold changed: the app keeps it in the config.
    pub dirty: bool,
    /// This frame's rows and place, from `prepare` to `draw`.
    frame: Option<Prepared>,
    /// Our pet's TP (s2c 0x068).
    pub pet_tp: Option<u16>,
    /// Our effects that just wore off (status, when).
    lost: Vec<(u16, Instant)>,
    /// Our TP when our weapon skill started.
    ws_tp: Option<f32>,
    /// Roll totals that landed on us, by roll effect (Naturalist's Roll's power).
    rolls: HashMap<u16, u32>,
    /// Party members' zones as last seen (a change drops their zone-flagged effects).
    zones: HashMap<u32, u16>,
    /// Players we cast Arise on (Reraise follows their acceptance).
    arise: Vec<(u32, Instant)>,
    /// A Geo- spell of ours waiting for its luopan.
    luopan: Option<(u16, Instant)>,
    /// The log window's rect last frame (the right dock stacks on it where it reaches under).
    pub log: Option<[f32; 4]>,
}

struct Prepared {
    secs: [Vec<Group>; 3],
    place: Place,
    pulse: f32,
    small: bool,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            on: HashMap::new(),
            totals: HashMap::new(),
            out: true,
            slide: 1.0,
            folded: 0,
            last: None,
            rect: None,
            hits: Vec::new(),
            dirty: false,
            frame: None,
            pet_tp: None,
            lost: Vec::new(),
            ws_tp: None,
            rolls: HashMap::new(),
            zones: HashMap::new(),
            arise: Vec::new(),
            luopan: None,
            log: None,
        }
    }
}

impl Timers {
    pub fn new(out: bool, folded: u8) -> Self {
        Self { out, slide: out as u8 as f32, folded, ..Default::default() }
    }

    fn total(&mut self, key: (u8, u32), left: f32, seen: &mut HashSet<(u8, u32)>) -> f32 {
        seen.insert(key);
        let t = self.totals.entry(key).or_insert(left);
        if left > *t {
            *t = left;
        }
        *t
    }

    pub fn toggle(&mut self) {
        self.out = !self.out;
        self.dirty = true;
    }

    /// Last frame's window (the training regime window stacks on it).
    pub fn rect(&self) -> Option<[f32; 4]> {
        self.rect
    }

    /// The total of a roll that landed on us.
    pub fn roll_on_me(&self, status: u16) -> Option<u32> {
        self.rolls.get(&status).copied()
    }

    /// The bearer died: effects with the death flag go (all of a monster's).
    pub fn died(&mut self, id: u32) {
        if let Some(b) = self.on.get_mut(&id) {
            log::debug!("timers: {id:#x} died");
            let mob = b.mob;
            b.effects.retain(|e| !mob && !durations::flag(e.status, 'd'));
        }
    }

    fn end(&mut self, target: u32, icon: u16) {
        let Some(b) = self.on.get_mut(&target) else { return };
        // Copies of one effect: the one ending first goes.
        let now = Instant::now();
        let at = |e: &Applied| e.secs.map_or(f32::MAX, |s| s - now.duration_since(e.start).as_secs_f32());
        if let Some(i) = b.effects.iter().enumerate().filter(|(_, e)| durations::icon_matches(e.status, icon)).min_by(|a, b| at(a.1).total_cmp(&at(b.1))).map(|(i, _)| i) {
            log::debug!("timers: {target:#x} {} ended by the server, {:.1} s before the predicted end", b.effects[i].status, at(&b.effects[i]));
            if durations::duplicates(b.effects[i].status) {
                b.effects.remove(i);
            } else {
                b.effects.retain(|e| !durations::icon_matches(e.status, icon));
            }
        }
    }
}

// ---- What the server tells us ----

use super::durations::{self, Kind};

/// 0x028 result messages removing the status in the value from the target (-na, Healing Waltz, Erase / Dispel /
/// Finale, Larceny, steals: spec 80.3).
const REMOVED: &[u16] = &[83, 123, 159, 321, 341, 343, 378, 453];
/// A proc removing the status in its value.
const PROC_REMOVED: u16 = 168;
/// 0x028 messages of anyone's effect landing with its status in the value (replaces ours of the same status).
const OTHERS_LANDED: &[u16] = &[127, 186, 230, 236, 237, 242, 243, 266, 267, 268, 271, 277, 278, 319, 320];
/// 0x029: the effect with the icon in Data wore off (LSB sends only these two: spec 80.4.2).
const WORN: &[u16] = &[204, 206];
/// 0x029: the target was defeated.
const DEFEATED: &[u16] = &[6, 20, 97];
/// Weapon skill messages (category 3 is shared with some abilities: spec 80.2.2).
const WS_MSGS: &[u16] = &[185, 188, 189, 264, 282, 283];

/// Our pet: its server id from the entity list (my_pet is its target index).
fn my_pet(w: &World) -> Option<u32> {
    let i = w.my_pet;
    if i == 0 {
        return None;
    }
    w.entities.iter().find(|(_, e)| e.index == i).map(|(&id, _)| id)
}

fn bearer_name(w: &World, id: u32) -> (String, bool) {
    if id == w.my_id {
        return (w.name.clone(), false);
    }
    if let Some(e) = w.entities.get(&id) {
        return (e.name.clone(), e.mob);
    }
    (w.party.members().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default(), false)
}

fn source_name(w: &World, s: Source) -> String {
    let n = w.names.as_ref();
    match s {
        Source::Spell(id) => n.and_then(|n| n.spell(id as u32)),
        Source::Ability(id) | Source::Pet(id) => n.and_then(|n| n.ability(0x200 + id as u32)),
        Source::Ws(id) => n.and_then(|n| n.ability(id as u32)),
        Source::MobSkill(id) => w.battle.as_ref().map(|b| b.mob_skill(id)),
        Source::Proc(status) => w.battle.as_ref().map(|b| b.status_title(status)),
    }
    .unwrap_or_default()
}

/// What the target looks like to the formulas: ourselves, a party member (level, 0x076 icons) or someone else.
fn target_of(w: &World, id: u32, first: bool, c: &durations::Caster) -> durations::Target {
    let me = id == w.my_id;
    let level = w.party.members().find(|m| m.id == id).map(|m| m.jobs[1]).filter(|&l| l > 0);
    let has = if me { w.buffs.iter().copied().filter(|&b| b != 0xffff).collect() } else { w.party_buffs.get(&id).map_or(HashSet::new(), |l| l.iter().copied().filter(|&b| b != 0xffff).collect()) };
    let mods = if me { c.mods.clone() } else { HashMap::new() };
    durations::Target { me, level, has, first, mods }
}

/// The TP our weapon skill used: the TP at its start, the gear's TP Bonus (not the off-hand's; not the ranged slot's
/// for a melee skill, nor the main hand's for a ranged one), Fencer gear when single-wielding (spec 70.9).
fn ws_tp(w: &World, ws: u16) -> f32 {
    let ranged = (192..=223).contains(&ws);
    let skip: &[usize] = if ranged { &[0, 1] } else { &[1, 2] };
    let mut tp = w.timers.ws_tp.unwrap_or(1000.0);
    for slot in (0..16).filter(|s| !skip.contains(s)) {
        tp += durations::slot_mod(w, slot, 345);
    }
    let skill = |slot: usize| durations::slot_item(w, slot).and_then(|i| w.item_info.as_ref()?.item(i)).map_or(0, |it| it.skill);
    if !ranged && matches!(skill(0), 2 | 3 | 5 | 9 | 11) && skill(1) == 0 {
        tp += (0..16).map(|s| durations::slot_mod(w, s, 903)).sum::<f32>();
    }
    tp.min(3000.0)
}

/// A 0x028 for anyone: removals, defeats and others' landings end ours; our own (and our pet's) landings start them.
pub fn on_action(w: &mut World, a: &Action) {
    use crate::net::action::category as cat;
    for t in &a.targets {
        for r in &t.results {
            if REMOVED.contains(&r.message) {
                w.timers.end(t.id, r.value as u16);
            }
            if let Some(p) = r.proc.as_ref().filter(|p| p.message == PROC_REMOVED) {
                w.timers.end(t.id, p.value as u16);
            }
            // Bust (426 / 427): the roll leaves every target, silently.
            if matches!(r.message, 426 | 427)
                && let Some(row) = durations::tables().rows.get(&(Kind::Ability, a.arg as u16)).and_then(|r| r.first())
                && let Some(b) = w.timers.on.get_mut(&t.id)
            {
                b.effects.retain(|e| e.status != row.effect);
            }
            if r.info & 1 != 0 {
                w.timers.died(t.id);
            }
        }
    }
    let pet = my_pet(w);
    if a.actor != w.my_id && Some(a.actor) != pet {
        others(w, a);
        return;
    }
    let ours = a.actor == w.my_id;
    if ours && a.category == cat::SKILL_START {
        w.timers.ws_tp = w.vitals.map(|v| v.2 as f32);
        return;
    }
    let first_msg = a.targets.first().and_then(|t| t.results.first()).map_or(0, |r| r.message);
    let (kind, source) = match a.category {
        cat::MAGIC_FINISH => (Kind::Spell, Source::Spell(a.arg as u16)),
        cat::ABILITY_FINISH | 14 | 15 if ours => (Kind::Ability, Source::Ability(a.arg as u16)),
        cat::SKILL_FINISH if ours && WS_MSGS.contains(&first_msg) => (Kind::Ws, Source::Ws(a.arg as u16)),
        cat::SKILL_FINISH if ours => (Kind::Ability, Source::Ability(a.arg as u16)),
        cat::SKILL_FINISH | cat::MOB_SKILL_FINISH => (Kind::Pet, Source::MobSkill(a.arg as u16)),
        13 => (Kind::Pet, Source::Pet(a.arg as u16)),
        cat::BASIC_ATTACK | cat::RANGED_FINISH if ours => {
            procs(w, a);
            return;
        }
        _ => return,
    };
    let lost: Vec<u16> = w.timers.lost.iter().map(|l| l.0).collect();
    let tp = (kind == Kind::Ws).then(|| ws_tp(w, a.arg as u16));
    let mut c = durations::Caster::of(w, &lost, tp);
    // A pet casting a spell has none of our inputs (spec 70.6 / 70.8).
    c.pet = !ours && kind == Kind::Spell;
    let Some(rows) = durations::tables().rows.get(&(kind, a.arg as u16)) else { return };
    let white = kind == Kind::Spell && durations::white(w, a.arg as u16);
    let double_up = first_msg == 424;
    let now = Instant::now();
    for (i, t) in a.targets.iter().enumerate() {
        for r in &t.results {
            let hit: Vec<&durations::Row> = rows.iter().filter(|row| durations::lands(row, r.message, r.value) && row.know != 'n' && row.effect != 0).collect();
            // Several effects in one result: the value names the best; it being the first of the action means all
            // landed (Warm-Up, Amplification ...).
            let hit: Vec<&durations::Row> = match hit.iter().position(|row| row.param == 'E') {
                Some(k) if hit[k].effect as u32 != r.value || rows.iter().position(|x| std::ptr::eq(x, hit[k])) != rows.iter().position(|x| x.param == 'E') => hit.into_iter().filter(|row| row.param != 'E' || row.effect as u32 == r.value).collect(),
                _ => hit,
            };
            for row in hit {
                land(w, &c, a, row, source, t.id, i == 0, r.value, white, double_up, now);
            }
        }
    }
    if ours && kind == Kind::Spell && a.arg == 494 {
        for t in &a.targets {
            w.timers.arise.push((t.id, now));
        }
    }
}

/// One row landing on `target`.
#[allow(clippy::too_many_arguments)]
fn land(w: &mut World, c: &durations::Caster, a: &Action, row: &durations::Row, source: Source, target: u32, first: bool, value: u32, white: bool, double_up: bool, now: Instant) {
    let me = w.my_id;
    match row.formula.as_str() {
        // Quick Draw boosts and Double-Up keep the end time; removals were handled.
        "QD_BOOST" | "REMOVE" | "DISPEL" | "COPY" | "STEAL" | "AURA" | "HIDDEN_ELE" | "NONE" => return,
        // The luopan comes as our pet; its life is timed once it is there.
        "AURA_LUOPAN" => {
            w.timers.luopan = Some((a.arg as u16, now));
            return;
        }
        "SOLACE_SS" if !(c.has(417) && !target_of(w, target, first, c).has.contains(&37)) => return,
        _ => {}
    }
    if row.target == "self" && target != a.actor {
        return;
    }
    let status = row.effect;
    let mut name = source_name(w, source);
    if row.formula == "ROLL" {
        name = format!("{name} ({value})");
        if target == me {
            w.timers.rolls.insert(status, value);
        }
        if double_up
            && let Some(e) = w.timers.on.get_mut(&target).and_then(|b| b.effects.iter_mut().find(|e| e.status == status))
        {
            e.name = name;
            return;
        }
    }
    if row.formula == "STEP" {
        name = format!("{name} ({value})");
    }
    let prior = w.timers.on.get(&target).and_then(|b| b.left(status, now));
    let t = target_of(w, target, first, c);
    let secs = durations::predict(c, row, &t, white, prior);
    if secs.is_none() && target != me {
        return;
    }
    // No wear-off reaches us: icon-0 / no-loss effects, our pet's, songs on others, Valiance / Vallation on others
    // (their origin is the bearer), Gambit (the mob's).
    let told = !durations::silent_end(status)
        && !matches!(source, Source::Pet(_) | Source::MobSkill(_))
        && !(row.formula.starts_with("ENH_SONG") && target != me)
        && !(matches!(status, 531 | 535) && target != me)
        && status != 536;
    let (who, mob) = bearer_name(w, target);
    let max_songs = if c.instrument { 2.0 } else { 1.0 } + c.m(453);
    let b = w.timers.on.entry(target).or_default();
    if !who.is_empty() {
        (b.name, b.mob) = (who, mob);
    }
    if durations::duplicates(status) {
        if row.formula.starts_with("ENH_SONG") {
            // Spec 30.3: the same song and tier is replaced (and, as LSB counts it, still fills its slot); at our cap
            // the song of ours that ends first goes.
            let ours = b.effects.iter().filter(|e| e.tier > 0 && (195..=223).contains(&e.status)).count();
            b.effects.retain(|e| !(e.status == status && e.tier == row.tier));
            if ours as f32 >= max_songs {
                let at = |e: &Applied| e.secs.map_or(f32::MAX, |s| s - now.duration_since(e.start).as_secs_f32());
                if let Some(i) = b.effects.iter().enumerate().filter(|(_, e)| e.tier > 0 && (195..=223).contains(&e.status)).min_by(|x, y| at(x.1).total_cmp(&at(y.1))).map(|(i, _)| i) {
                    b.effects.remove(i);
                }
            }
        } else {
            b.effects.retain(|e| !(e.status == status && e.tier == row.tier));
        }
    } else {
        b.effects.retain(|e| e.status != status);
    }
    // Rolls: a COR keeps two of its own on a target (one as a sub job); the one ending first goes.
    if row.formula == "ROLL" {
        let limit = if c.main.0 == 17 { 2 } else { 1 };
        let rolls: Vec<usize> = b.effects.iter().enumerate().filter(|(_, e)| matches!(e.status, 310..=339 | 600)).map(|(i, _)| i).collect();
        if rolls.len() >= limit {
            let at = |e: &Applied| e.secs.map_or(f32::MAX, |s| s - now.duration_since(e.start).as_secs_f32());
            if let Some(&i) = rolls.iter().min_by(|&&x, &&y| at(&b.effects[x]).total_cmp(&at(&b.effects[y]))) {
                b.effects.remove(i);
            }
        }
    }
    let gone = durations::ends(status);
    b.effects.retain(|e| !gone.contains(&e.status));
    log::debug!("timers: {source:?} landed {status} on {target:#x}: {secs:?} s ({}), told {told}", row.formula);
    b.effects.push(Applied { status, name, start: now, secs, told, tier: row.tier });
}

/// Our melee / ranged hit's additional effect (spec 75): the weapon's own length for it.
fn procs(w: &mut World, a: &Action) {
    let ranged = a.category == crate::net::action::category::RANGED_FINISH;
    let now = Instant::now();
    for t in &a.targets {
        for r in &t.results {
            let Some(p) = r.proc.as_ref().filter(|p| matches!(p.message, 160 | 164)) else { continue };
            let status = p.value as u16;
            let slots: &[usize] = if ranged { &[2, 3] } else { &[0, 1] };
            let Some(secs) = slots.iter().map(|&s| durations::slot_mod(w, s, 953)).find(|&v| v > 0.0) else { continue };
            let source = Source::Proc(status);
            let name = source_name(w, source);
            let (who, mob) = bearer_name(w, t.id);
            let b = w.timers.on.entry(t.id).or_default();
            if !who.is_empty() {
                (b.name, b.mob) = (who, mob);
            }
            b.effects.retain(|e| e.status != status);
            b.effects.push(Applied { status, name, start: now, secs: Some(secs), told: !durations::silent_end(status), tier: 0 });
        }
    }
}

/// Someone else's action: their landing of an effect we keep a timer for replaces ours (silently on the server);
/// their step extends the daze we put on; the player we raised with Arise accepting brings Reraise.
fn others(w: &mut World, a: &Action) {
    let now = Instant::now();
    let tiers = |status: u16| durations::tables().rows.get(&(Kind::Spell, a.arg as u16)).and_then(|r| r.iter().find(|r| r.effect == status)).map_or(0, |r| r.tier);
    for t in &a.targets {
        for r in &t.results {
            let Some(b) = w.timers.on.get_mut(&t.id) else { continue };
            if a.category == 14 && matches!(r.message, 519..=521 | 591) {
                let Some(row) = durations::tables().rows.get(&(Kind::Ability, a.arg as u16)).and_then(|r| r.first()) else { continue };
                if let Some(e) = b.effects.iter_mut().find(|e| e.status == row.effect) {
                    let gone = now.duration_since(e.start).as_secs_f32();
                    let left = e.secs.unwrap_or(0.0) - gone;
                    e.secs = Some(gone + (left.floor() + 30.0).min(120.0));
                    e.told = false;
                }
                continue;
            }
            if !OTHERS_LANDED.contains(&r.message) || r.value == 0 {
                continue;
            }
            let status = r.value as u16;
            if durations::duplicates(status) {
                let tier = tiers(status);
                b.effects.retain(|e| !(e.status == status && e.tier == tier));
            } else {
                let gone = durations::ends(status);
                b.effects.retain(|e| e.status != status && !gone.contains(&e.status));
            }
        }
    }
    // Arise accepted: the raised player's own category-4 Arise action (spec 10.5); Reraise for an hour, no origin.
    if a.category == crate::net::action::category::MAGIC_FINISH && a.arg == 494 && w.timers.arise.iter().any(|x| x.0 == a.actor) {
        w.timers.arise.retain(|x| x.0 != a.actor);
        let name = source_name(w, Source::Spell(494));
        let (who, mob) = bearer_name(w, a.actor);
        let b = w.timers.on.entry(a.actor).or_default();
        (b.name, b.mob) = (who, mob);
        b.effects.retain(|e| e.status != 113);
        b.effects.push(Applied { status: 113, name, start: now, secs: Some(3600.0), told: false, tier: 0 });
    }
}

/// A 0x029 / 0x02D message: wear-offs (Data = the icon) and defeats.
pub fn on_message(w: &mut World, target: u32, param: u32, message: u16) {
    let m = message & 0x7fff;
    if WORN.contains(&m) {
        log::debug!("timers: wear-off {m} on {target:#x}, icon {param}");
        w.timers.end(target, param as u16);
        if target == w.my_id {
            w.timers.lost.push((param as u16, Instant::now()));
        }
    } else if DEFEATED.contains(&m) {
        w.timers.died(target);
    }
}

/// s2c 0x076: a member's effects no longer listed have ended (copies of a song: as many of ours as icons left).
pub fn on_party_effects(w: &mut World) {
    let now = Instant::now();
    for (id, b) in w.timers.on.iter_mut() {
        let Some(list) = w.party_buffs.get(id) else { continue };
        let fresh = |e: &Applied| now.duration_since(e.start).as_secs_f32() < LIST_LAG;
        let mut keep: Vec<bool> = b.effects.iter().map(|e| fresh(e) || list.iter().any(|&i| durations::icon_matches(e.status, i))).collect();
        let statuses: HashSet<u16> = b.effects.iter().filter(|e| durations::duplicates(e.status)).map(|e| e.status).collect();
        for s in statuses {
            let icons = list.iter().filter(|&&i| i == s).count();
            let mut ours: Vec<usize> = (0..b.effects.len()).filter(|&i| b.effects[i].status == s && !fresh(&b.effects[i])).collect();
            let at = |e: &Applied| e.secs.map_or(f32::MAX, |x| x - now.duration_since(e.start).as_secs_f32());
            ours.sort_by(|&x, &y| at(&b.effects[x]).total_cmp(&at(&b.effects[y])));
            for &i in ours.iter().take(ours.len().saturating_sub(icons)) {
                keep[i] = false;
            }
        }
        let mut k = keep.into_iter();
        b.effects.retain(|_| k.next().unwrap_or(true));
    }
}

/// Zoning: effects on anyone outside our party stay behind.
pub fn on_zone(w: &mut World) {
    let party: HashSet<u32> = w.party.members().map(|m| m.id).collect();
    let me = w.my_id;
    w.timers.on.retain(|id, _| *id == me || party.contains(id));
    w.timers.luopan = None;
}

/// An entity despawned: a monster's effects go; a player's stay while in our party.
pub fn on_gone(w: &mut World, id: u32, died: bool) {
    if died || !w.party.members().any(|m| m.id == id) {
        if w.timers.on.contains_key(&id) {
            log::debug!("timers: {id:#x} despawned");
        }
        w.timers.on.remove(&id);
    }
}

/// The luopan's life (spec 40.8): 600 s at most, or its HP over the drain each 3 s; Bolster and Blaze of Glory as
/// they were at the cast.
fn luopan_life(c: &durations::Caster) -> f32 {
    let lvl = c.main.1 as f32;
    let bolster = c.has(513);
    let jp = if bolster { c.jp(672) } else { 0.0 };
    let max = (250.0 * lvl / 15.0).floor() * (1.0 + 0.03 * jp);
    let hp = if c.has(569) { max / 2.0 + max * c.jp(675) / 100.0 } else { max };
    let drain = ((lvl / 4.0).floor() - jp).max(1.0);
    (600f32).min((hp / drain).ceil() * 3.0)
}

/// Once a frame: drop what ended unannounced, follow party members' zone changes, time a luopan that came.
fn maintain(w: &mut World) {
    let now = Instant::now();
    let me = w.my_id;
    w.timers.lost.retain(|l| now.duration_since(l.1).as_secs_f32() < LOST_FOR);
    w.timers.arise.retain(|x| now.duration_since(x.1).as_secs() < 600);
    // A party member who changed zones lost their zone-flagged effects.
    let zones: Vec<(u32, u16)> = w.party.members().map(|m| (m.id, m.zone)).collect();
    for (id, z) in zones {
        if w.timers.zones.insert(id, z).is_some_and(|old| old != z)
            && let Some(b) = w.timers.on.get_mut(&id)
        {
            b.effects.retain(|e| !durations::flag(e.status, 'z'));
        }
    }
    let present: HashSet<u16> = w.buffs.iter().copied().filter(|&b| b != 0xffff).collect();
    for (id, b) in w.timers.on.iter_mut() {
        b.effects.retain(|e| {
            let gone = now.duration_since(e.start).as_secs_f32();
            let keep = match e.secs {
                _ if *id == me => present.iter().any(|&i| durations::icon_matches(e.status, i)) || gone < LIST_LAG,
                Some(s) => gone < s + if e.told { GRACE } else { 0.3 },
                None => false,
            };
            if !keep && *id != me {
                log::debug!("timers: {id:#x} {} timed out after {gone:.1} s (told {})", e.status, e.told);
            }
            keep
        });
    }
    w.timers.on.retain(|_, b| !b.effects.is_empty());
    if let Some((spell, at)) = w.timers.luopan {
        if now.duration_since(at).as_secs() > 5 {
            w.timers.luopan = None;
        } else if let Some(pet) = my_pet(w) {
            w.timers.luopan = None;
            let c = durations::Caster::of(w, &[], None);
            let secs = luopan_life(&c);
            let name = source_name(w, Source::Spell(spell));
            let (who, _) = bearer_name(w, pet);
            let b = w.timers.on.entry(pet).or_default();
            (b.name, b.mob) = (who, false);
            b.effects.push(Applied { status: 612, name, start: at, secs: Some(secs), told: false, tier: 0 });
        }
    }
}

// ---- Rows ----

/// The ability on a 0x119 timer: the one we know (job abilities by the 0x0AC bits, pet commands always).
fn ability_for_timer(w: &World, timer: u16) -> Option<&crate::dat::actions::Ability> {
    let acts = w.actions.as_ref()?;
    let known = |id: u16| {
        let bit = id.wrapping_sub(0x200) as usize;
        id >= 0x400 || w.commands_known.get(64 + bit / 8).is_some_and(|b| b & (1 << (bit % 8)) != 0)
    };
    let mut found = None;
    for a in acts.abilities.iter().filter(|a| a.id >= 0x200 && a.timer == timer) {
        // A sub-list's row (Ready, Quick Draw, Stratagems) names the timer over its moves.
        if a.sub != 0 && known(a.id) {
            return Some(a);
        }
        if found.is_none() && known(a.id) {
            found = Some(a);
        }
    }
    found
}

/// FUN_10222c90: the charge abilities by their moves' kind (9 Quick Draw, 0xf Stratagems, 0x12 Ready): the charges
/// at our job's level and the full recast in seconds; none when the job does not give them.
pub fn charge_rule(w: &World, kind: u8) -> Option<(i32, i32)> {
    let s = w.stats.as_ref()?;
    let ((mj, ml), (sj, sl)) = (s.main, s.sub);
    let (n, full) = match kind {
        9 => {
            let lv = if mj == 0x11 { Some(ml) } else if sj == 0x11 { Some(sl) } else { None };
            (lv.map_or(0, |l| if l < 0x28 { 0 } else { 2 }), 0x78)
        }
        0xf => {
            let lv = if mj == 0x14 { Some(ml) } else if sj == 0x14 { Some(sl) } else { None };
            (lv.map_or(0, |l| (l as i32 - 10) / 0x14 + 1), 0xf0)
        }
        // Retail tests the sub job against 0x14 here, not 9 (kept).
        0x12 => {
            let lv = if mj == 9 { Some(ml) } else if sj == 0x14 { Some(sl) } else { None };
            (lv.map_or(0, |l| if l < 0x19 { 0 } else { 3 }), 0x5a)
        }
        _ => return None,
    };
    Some((n, full))
}

/// FUN_10225120 / FUN_10225220 with charges: the full recast adjusted by the slot's Calc2 (added) and Calc1 (sixtieths
/// off), then the seconds to the next charge (`left` mod a charge's share, the share when it divides) and the charges
/// ready.
pub fn charges(left_secs: i32, n: i32, full: i32, calc: (u8, i16)) -> Option<(i32, i32, i32)> {
    if n <= 0 {
        return None;
    }
    let full = full + calc.1 as i32;
    let total = full as f32 - (calc.0 & 0x3f) as f32 * (1.0 / 60.0) * full as f32;
    let per = (total / n as f32) as i32;
    let total = total as i32;
    if per <= 0 || total / n <= 0 {
        return None;
    }
    let mut next = left_secs % per;
    if next == 0 && left_secs != 0 {
        next = per;
    }
    Some((next, (total - left_secs) / (total / n), per))
}

/// The charge kind of the abilities on a timer (the sub-list row's sub, or a move's own kind).
fn charge_kind(w: &World, a: &crate::dat::actions::Ability) -> u8 {
    if a.sub != 0 {
        return a.sub;
    }
    let _ = w;
    a.kind
}

fn recast_rows(w: &World, t: &mut Timers, now: Instant, seen: &mut HashSet<(u8, u32)>) -> Vec<Row> {
    let mut rows = Vec::new();
    let names = w.names.clone();
    let name_of = |id: u32, spell: bool| names.as_ref().and_then(|n| if spell { n.spell(id) } else { n.ability(id) }).unwrap_or_default();
    for (&timer, &end) in &w.ability_recast {
        let left = end.saturating_duration_since(now).as_secs_f32();
        if left <= 0.0 {
            continue;
        }
        let Some(a) = ability_for_timer(w, timer) else { continue };
        let icon = super::magic::list_icon_of(w, false, a.id).map_or(Icon::None, |(l, k)| Icon::List(l, k));
        let mut name = name_of(a.id as u32, false);
        let calc = w.ability_calc.get(&timer).copied().unwrap_or((0, 0));
        let rule = charge_rule(w, charge_kind(w, a)).and_then(|(n, full)| charges(left.ceil() as i32, n, full, calc));
        let (left, total) = match rule {
            Some((next, ready, per)) => {
                name = format!("{name} ({ready})");
                // The share's fraction of a second carries over from the whole.
                (next as f32 - (left.ceil() - left), per as f32)
            }
            None => (left, t.total((0, timer as u32), left, seen)),
        };
        rows.push(Row { icon, name, left, total, fills: true });
    }
    for (&spell, &end) in &w.spell_recast {
        let left = end.saturating_duration_since(now).as_secs_f32();
        if left <= 0.0 {
            continue;
        }
        let icon = super::magic::list_icon_of(w, true, spell).map_or(Icon::None, |(l, k)| Icon::List(l, k));
        let total = t.total((1, spell as u32), left, seen);
        rows.push(Row { icon, name: name_of(spell as u32, true), left, total, fills: true });
    }
    rows.sort_by(|a, b| a.left.total_cmp(&b.left).then_with(|| a.name.cmp(&b.name)));
    rows
}

/// Our own effects with 0x063 end times, in 0x037's order (as the status icons), named by what we cast when it was
/// ours.
fn own_rows(w: &World, gu: &GameUi, t: &mut Timers, seen: &mut HashSet<(u8, u32)>) -> Vec<Row> {
    let small = w.game.get(0x99) & 4 != 0;
    let now60 = w.clock.map_or(0.0, |(c, at)| (c as f64 + at.elapsed().as_secs_f64()) * 60.0) as u64 as u32;
    let names: Vec<(u16, String)> = t.on.get(&w.my_id).map_or(Vec::new(), |b| b.effects.iter().map(|e| (e.status, e.name.clone())).collect());
    let mut rows = Vec::new();
    for i in 0..32 {
        let id = w.buffs[i];
        if id == 0xffff || id == 0 {
            continue;
        }
        let Some(end) = w.buff_timers.as_ref().filter(|b| b.0[i] == id).map(|b| b.1[i]).filter(|&e| e != 0x7fff_ffff) else { continue };
        if gu.status_entry(small, id).is_some_and(|e| e.no_timer) {
            continue;
        }
        let left = end.wrapping_sub(now60) as i32 as f32 / 60.0;
        if left <= 0.0 {
            continue;
        }
        let total = t.total((2, end), left, seen);
        let name = names.iter().rev().find(|e| e.0 == id).map(|e| e.1.clone()).filter(|n| !n.is_empty());
        let name = name.unwrap_or_else(|| w.battle.as_ref().map(|b| b.status_title(id)).unwrap_or_default());
        rows.push(Row { icon: Icon::Status(id), name, left, total, fills: false });
    }
    rows
}

fn applied_rows(b: &Bearer, now: Instant) -> Vec<Row> {
    let mut rows: Vec<Row> = b
        .effects
        .iter()
        .map(|e| {
            let gone = now.duration_since(e.start).as_secs_f32();
            match e.secs {
                Some(s) => Row { icon: Icon::Status(e.status), name: e.name.clone(), left: s - gone, total: s, fills: false },
                None => Row { icon: Icon::Status(e.status), name: e.name.clone(), left: 0.0, total: 0.0, fills: false },
            }
        })
        .collect();
    rows.sort_by(|a, b| a.left.total_cmp(&b.left));
    rows
}

/// The sections' groups: recasts; us, then others by party order, then the rest; monsters, ours targeted first.
pub fn sections(w: &mut World, gu: &GameUi) -> [Vec<Group>; 3] {
    let now = Instant::now();
    maintain(w);
    let me = w.my_id;
    let mut t = std::mem::take(&mut w.timers);
    let mut seen = HashSet::new();
    let recasts = recast_rows(w, &mut t, now, &mut seen);
    let own = own_rows(w, gu, &mut t, &mut seen);
    t.totals.retain(|k, _| seen.contains(k));
    w.timers = t;

    let mut buffs = Vec::new();
    if !own.is_empty() {
        buffs.push(Group { name: Some(w.name.clone()), rows: own });
    }
    let order: Vec<u32> = w.party.members().map(|m| m.id).collect();
    let mut others: Vec<(&u32, &Bearer)> = w.timers.on.iter().filter(|(id, b)| **id != me && !b.mob).collect();
    others.sort_by_key(|(id, b)| (order.iter().position(|o| o == *id).unwrap_or(usize::MAX), b.name.clone()));
    for (_, b) in others {
        buffs.push(Group { name: Some(b.name.clone()), rows: applied_rows(b, now) });
    }
    let mut mobs: Vec<(&u32, &Bearer)> = w.timers.on.iter().filter(|(_, b)| b.mob).collect();
    mobs.sort_by_key(|(id, b)| (Some(**id) != w.target, b.effects.iter().map(|e| e.start).max().map(std::cmp::Reverse), b.name.clone()));
    let debuffs = mobs.into_iter().map(|(_, b)| Group { name: Some(b.name.clone()), rows: applied_rows(b, now) }).collect();
    let recasts = if recasts.is_empty() { Vec::new() } else { vec![Group { name: None, rows: recasts }] };
    [recasts, buffs, debuffs]
}

// ---- The window ----

/// The window's width, its inner margin, a row's pitch and parts.
pub const WIDTH: f32 = 184.0;
const PAD: f32 = 8.0;
const HEAD_H: f32 = 16.0;
const GROUP_H: f32 = 13.0;
const ROW_H: f32 = 24.0;
const GAUGE_X: f32 = 20.0;
/// What stays on screen of a drawer slid in.
const SLIVER: f32 = 22.0;
/// The top dock's drawer slides up into the help bar's foot.
const TOP_EDGE: f32 = 48.0;
/// The slide takes 12 ticks.
const SLIDE_TICKS: f32 = 12.0;
const N: u32 = 0x8080_8080;
const GROUP_TINT: u32 = 0x80a0_a080;
const DIM: u32 = 0x8050_5050;

enum Line<'a> {
    Head(usize, usize),
    Group(&'a str),
    Row(&'a Row),
}

/// The place for the window: x, the top (Left / Top) or the foot (Right), and the most height it may take.
pub struct Place {
    pub dock: Dock,
    pub x: f32,
    pub y: f32,
    pub max_h: f32,
}

/// Where the window goes beside the retail windows (layout pixels): `icons` the status icon rows, `log_top` the log's
/// top, `foot` the top of the party / target / alliance windows on the right, `mini` the minimap's rect.
pub fn place(dock: Dock, sw: f32, sh: f32, icons: usize, log_top: f32, foot: f32, mini: Option<[f32; 4]>) -> Place {
    // The status icons: 26-px rows from (142, 48).
    let icons_foot = if icons == 0 { 0.0 } else { 48.0 + 2.0 + 26.0 * icons.div_ceil(9) as f32 + 4.0 };
    // The minimap's foot where it stands over the column [x0, x1).
    let mini_foot = |x0: f32, x1: f32| mini.filter(|m| m[0] < x1 && m[0] + m[2] > x0).map_or(0.0, |m| m[1] + m[3] + 6.0);
    match dock {
        Dock::Left => {
            // Under the cast bar (16, 94) + 24 and the gauges, past the icons when they reach over; above the compass
            // (its centre 42 above the log, 50 each way); under a minimap there.
            let top = 122f32.max(if 16.0 + WIDTH > 142.0 { icons_foot } else { 0.0 }).max(mini_foot(16.0, 16.0 + WIDTH));
            Place { dock, x: 16.0, y: top, max_h: (log_top - 96.0 - top).max(0.0) }
        }
        Dock::Right => {
            // Above the party / target / alliance windows, below the main menu (W - 128, 48) 112 x 202 and a minimap.
            let (foot, x) = (foot - 4.0, sw - 16.0 - WIDTH);
            Place { dock, x, y: foot, max_h: (foot - 252f32.max(mini_foot(x, x + WIDTH))).max(0.0) }
        }
        Dock::Top => {
            // Centred, clear of the status icons (to x 376), the main menu column (from W - 128) and a minimap at the
            // top right.
            let right = mini.filter(|m| m[1] < 100.0 && m[0] > sw / 2.0).map_or(sw - 132.0, |m| m[0] - 4.0);
            let x = ((sw - WIDTH) / 2.0).max(380.0).min(right - WIDTH);
            let top = 50.0;
            Place { dock, x, y: top, max_h: ((sh * 0.45).min(log_top - 4.0) - top).max(0.0) }
        }
    }
}

fn clock(left: f32) -> String {
    let s = left.max(0.0).ceil() as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The lines that fit: rows dropped from the last section up, each section's heading counting what it hides.
fn lines<'a>(secs: &'a [Vec<Group>; 3], folded: u8, max_h: f32) -> (Vec<Line<'a>>, [usize; 3]) {
    let mut hidden = [0usize; 3];
    let mut out: Vec<Line> = Vec::new();
    for (k, groups) in secs.iter().enumerate() {
        let n: usize = groups.iter().map(|g| g.rows.len()).sum();
        if n == 0 {
            continue;
        }
        out.push(Line::Head(k, n));
        if folded & (1 << k) != 0 {
            hidden[k] = n;
            continue;
        }
        for g in groups.iter().filter(|g| !g.rows.is_empty()) {
            if let Some(name) = &g.name {
                out.push(Line::Group(name));
            }
            out.extend(g.rows.iter().map(Line::Row));
        }
    }
    let height = |l: &[Line]| l.iter().map(line_h).sum::<f32>() + 2.0 * PAD;
    while height(&out) > max_h {
        let Some(i) = out.iter().rposition(|l| matches!(l, Line::Row(_))) else { break };
        let k = out[..i].iter().rev().find_map(|l| if let Line::Head(k, _) = l { Some(*k) } else { None }).unwrap_or(0);
        hidden[k] += 1;
        out.remove(i);
        // A group left without rows loses its name.
        if matches!(out.get(i - 1), Some(Line::Group(_))) && !matches!(out.get(i), Some(Line::Row(_))) {
            out.remove(i - 1);
        }
    }
    (out, hidden)
}

fn line_h(l: &Line) -> f32 {
    match l {
        Line::Head(..) => HEAD_H,
        Line::Group(_) => GROUP_H,
        Line::Row(_) => ROW_H,
    }
}

fn ease(s: f32) -> f32 {
    s * s * (3.0 - 2.0 * s)
}

/// Draws the window at `p` with the drawer at `slide` (0 in .. 1 out); returns its rect and hot areas. `pulse` is the
/// retail status icons' 40-tick triangle (their last 15 s dim with it).
#[allow(clippy::too_many_arguments)]
pub fn draw_panel(gu: &mut GameUi, secs: &[Vec<Group>; 3], folded: u8, p: &Place, slide: f32, sw: f32, pulse: f32, small: bool) -> Option<([f32; 4], Hits)> {
    let (lines, hidden) = lines(secs, folded, p.max_h);
    if lines.is_empty() {
        return None;
    }
    let h = (lines.iter().map(line_h).sum::<f32>() + 2.0 * PAD).max(48.0);
    let (mut x, mut y) = (p.x, if p.dock == Dock::Right { p.y - h } else { p.y });
    let e = 1.0 - ease(slide.clamp(0.0, 1.0));
    match p.dock {
        Dock::Left => x -= (x + WIDTH - SLIVER) * e,
        Dock::Right => x += (sw - SLIVER - x) * e,
        // Into the help bar's foot (y 48): clipped there, so a faded top bar shows none of it.
        Dock::Top => y -= (y + h - (TOP_EDGE + SLIVER)) * e,
    }
    let mut hits = Vec::new();
    if p.dock == Dock::Top {
        gu.clip(Some([0.0, TOP_EDGE, sw, 1.0e5]));
    }
    gu.frame(x, y, WIDTH, h);
    // The drawer's arrow on its outer edge: pointing out of the screen while out, in while in.
    let inward = slide < 0.5;
    let (ax, ay) = match p.dock {
        Dock::Left => (x + WIDTH - 20.0, y + 6.0),
        Dock::Right => (x + 4.0, y + 6.0),
        // In: on the sliver below the help bar; out: the window's top right, clear of the rows.
        Dock::Top if inward => (x + WIDTH - 18.0, y + h - 18.0),
        Dock::Top => (x + WIDTH - 18.0, y + 4.0),
    };
    match p.dock {
        Dock::Left => gu.sprite("menu    frames", if inward { 26 } else { 25 }, ax, ay, N),
        Dock::Right => gu.sprite("menu    frames", if inward { 25 } else { 26 }, ax, ay, N),
        // The left arrow turned a quarter: down while in, up while out.
        Dock::Top => {
            let (m, dx, dy) = if inward { ([0.0, 1.0, -1.0, 0.0], 0.0, 18.0) } else { ([0.0, -1.0, 1.0, 0.0], 9.0, 0.0) };
            gu.sprite_affine("menu    frames", 25, ax + dx, ay + dy, m, N);
            true
        }
    };
    let drawer = match p.dock {
        Dock::Left => [x + WIDTH - SLIVER, y, SLIVER, h.min(48.0)],
        Dock::Right => [x, y, SLIVER, h.min(48.0)],
        Dock::Top if inward => [x, y + h - SLIVER, WIDTH, SLIVER],
        Dock::Top => [x + WIDTH - 24.0, y, 24.0, 24.0],
    };
    hits.push((Hit::Drawer, drawer));
    if slide <= 0.0 {
        gu.clip(None);
        return Some((visible(p.dock, [x, y, WIDTH, h]), hits));
    }
    let inner = WIDTH - 2.0 * PAD;
    let mut ly = y + PAD;
    for (i, l) in lines.iter().enumerate() {
        match l {
            Line::Head(k, n) => {
                let label = if hidden[*k] > 0 { format!("{} ({})", SECTIONS[*k], if hidden[*k] == *n { *n } else { hidden[*k] }) } else { SECTIONS[*k].to_string() };
                // The first heading makes room for the drawer arrow on the right dock.
                let lx = if p.dock == Dock::Right && i == 0 { x + PAD + 16.0 } else { x + PAD };
                gu.label(&label, lx, ly, N, false);
                hits.push((Hit::Section(*k), [x + PAD, ly, inner - 14.0, HEAD_H]));
            }
            Line::Group(name) => {
                gu.shape_fit(name, x + PAD + 2.0, ly + 1.0, inner - 2.0, GROUP_TINT);
            }
            Line::Row(r) => {
                let (rx, right) = (x + PAD, x + WIDTH - PAD);
                let waiting = r.left <= 0.0;
                let c = if !r.fills && r.left > 0.0 && r.left <= 15.0 { (0x40 + (64.0 * pulse) as u32) as f32 / 128.0 } else { 1.0 };
                match r.icon {
                    Icon::Status(id) => {
                        if let Some(t) = gu.status_icon(small, id) {
                            gu.image_rect(&t, rx, ly + 4.0, 16.0, 16.0, [c, c, c, 1.0]);
                        }
                    }
                    Icon::List(lib, k) => {
                        gu.sprite(lib, k, rx, ly + 4.0, N);
                    }
                    Icon::None => {}
                }
                gu.text_fit(&r.name, rx + GAUGE_X, ly - 2.0, inner - GAUGE_X + 16.0, if waiting { 0x8060_6060 } else { N });
                let t = if r.total <= 0.0 { "--:--".to_string() } else { clock(r.left) };
                gu.shape_text(&t, right, ly + 13.0, if waiting { DIM } else { N }, true);
                if r.total > 0.0 {
                    let f = (r.left / r.total).clamp(0.0, 1.0);
                    let f = if r.fills { 1.0 - f } else { f };
                    let (gx, gy) = (rx + GAUGE_X, ly + 14.0);
                    // Frame 38's own fill (97, 64 x 6), coloured by the time left.
                    if f > 0.0 {
                        gu.sprite_coloured("menu    windowps", 97, gx + 4.0, gy + 1.0, f, 1.0, bar_colour(r.left), 8);
                    }
                    gu.sprite("menu    windowps", 38, gx, gy, if f == 0.0 { 0x3050_5050 } else { N });
                }
            }
        }
        ly += line_h(l);
    }
    gu.clip(None);
    Some((visible(p.dock, [x, y, WIDTH, h]), hits))
}

/// The Ashita Timers add-on's bar colours (timer.lua), every bar: green from 30 s left, yellow from 20, orange from 10,
/// else red (vertex colours, 0x80 = 1.0; alpha as fill 97's).
fn bar_colour(left: f32) -> [u8; 4] {
    match left {
        l if l >= 30.0 => [0x00, 0x80, 0x00, 0xa9],
        l if l >= 20.0 => [0x80, 0x80, 0x1a, 0xa9],
        l if l >= 10.0 => [0x80, 0x53, 0x27, 0xa9],
        _ => [0x80, 0x00, 0x00, 0xa9],
    }
}

/// The part of the window on show (the top dock's is clipped at the help bar's foot).
fn visible(dock: Dock, r: [f32; 4]) -> [f32; 4] {
    if dock != Dock::Top || r[1] >= TOP_EDGE {
        return r;
    }
    [r[0], TOP_EDGE, r[2], (r[1] + r[3] - TOP_EDGE).max(0.0)]
}

/// The status icon rows in use (for the left dock's top).
fn icon_count(w: &World) -> usize {
    w.buffs.iter().filter(|&&b| b != 0xffff && b != 0).count()
}

/// Each frame from the HUD, before the party / target windows and the menus (they draw over it). Hidden in events,
/// under the map, and with nothing to show.
pub fn prepare(gu: &GameUi, w: &mut World, sw: f32, sh: f32, foot: f32) {
    let now = Instant::now();
    let dt = w.timers.last.map_or(0.0, |l| now.duration_since(l).as_secs_f32());
    w.timers.last = Some(now);
    let step = dt * 60.0 / SLIDE_TICKS;
    w.timers.slide = if w.timers.out { (w.timers.slide + step).min(1.0) } else { (w.timers.slide - step).max(0.0) };
    w.timers.frame = None;
    if !w.extra_timers || super::event::active(w) || w.map.is_some() || w.cnq.is_some() {
        return;
    }
    let secs = sections(w, gu);
    let place = base_place(gu, w, sw, sh, foot);
    let t = 40.0 - (w.started.elapsed().as_secs_f32() * 60.0) % 40.0;
    let pulse = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
    w.timers.frame = Some(Prepared { secs, place, pulse, small: w.game.get(0x99) & 4 != 0 });
}

/// The dock's place this frame (the training regime window's too).
pub fn base_place(gu: &GameUi, w: &World, sw: f32, sh: f32, foot: f32) -> Place {
    let log_top = gu.log_top.unwrap_or(sh - 16.0 - 120.0);
    // On the right, the log reaching under the window is one more window to stand on.
    let foot = match w.timers.log {
        Some([lx, ly, lw, _]) if lx + lw > sw - 16.0 - WIDTH => foot.min(ly),
        _ => foot,
    };
    place(Dock::of(w.timers_dock), sw, sh, icon_count(w), log_top, foot, w.minimap_rect)
}

/// Draws what `prepare` set up (after the HUD's clear, before the party / target windows and the menus).
pub fn draw(gu: &mut GameUi, t: &mut Timers, sw: f32) {
    t.rect = None;
    t.hits.clear();
    let Some(f) = t.frame.take() else { return };
    if let Some((r, hits)) = draw_panel(gu, &f.secs, t.folded, &f.place, t.slide, sw, f.pulse, f.small) {
        t.rect = Some(r);
        t.hits = hits;
    }
}

/// After every window has drawn: a click on the drawer's arrow or a section heading, unless a window over ours has the
/// spot (`over`). Returns the window's rect for the world-click blockers.
pub fn pointer(w: &mut World, gu: &GameUi, p: Option<(f32, f32)>, clicked: bool, over: &[[f32; 4]]) -> Option<[f32; 4]> {
    let r = w.timers.rect?;
    let Some(p) = p else { return Some(r) };
    let inside = |r: &[f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    if !inside(&r) || over.iter().any(inside) {
        return Some(r);
    }
    let Some(&(hit, _)) = w.timers.hits.iter().find(|(_, h)| inside(h)) else { return Some(r) };
    gu.hover.set(true);
    if clicked {
        match hit {
            Hit::Drawer => w.timers.toggle(),
            Hit::Section(k) => {
                w.timers.folded ^= 1 << k;
                w.timers.dirty = true;
            }
        }
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    Some(r)
}

/// topbar-shot's sample (`--window timers:DOCK[:SLIDE[:FOLDED]]`): spell and ability icons from the action table.
pub fn shot(gu: &mut GameUi, acts: Option<&crate::dat::actions::Actions>, spec: &str, sw: f32, sh: f32, mini: Option<[f32; 4]>) {
    let f: Vec<&str> = spec.split(':').collect();
    let dock = Dock::of(f.first().and_then(|d| d.parse().ok()).unwrap_or(0));
    let slide = f.get(1).and_then(|d| d.parse().ok()).unwrap_or(1.0);
    let folded = f.get(2).and_then(|d| d.parse().ok()).unwrap_or(0);
    let list = |spell: bool, id: u16| -> Icon {
        let Some(a) = acts else { return Icon::None };
        let r = if spell { a.spell(id).map(|r| (r.icon, r.icon_hd)) } else { a.abilities.get(id as usize).map(|r| (r.icon as u16, r.icon_hd)) };
        r.map_or(Icon::None, |(i, hd)| match hd {
            0xffff => Icon::List("menu    magiconw", i as usize),
            1000.. => Icon::List("menu    magico2w", (hd - 1000) as usize),
            _ => Icon::List("menu    magiconw", hd as usize),
        })
    };
    let row = |icon: Icon, name: &str, left: f32, total: f32, fills: bool| Row { icon, name: name.into(), left, total, fills };
    let secs = [
        vec![Group { name: None, rows: vec![
            row(list(false, 0x200 + 35), "Provoke", 18.0, 30.0, true),
            row(list(true, 57), "Haste", 9.5, 20.0, true),
            row(list(false, 0x200 + 0x1f), "Berserk", 252.0, 300.0, true),
            row(list(false, 0x200 + 0xd7), "Stratagems (2)", 22.0, 48.0, true),
        ] }],
        vec![
            Group { name: Some("Lnxcsfix".into()), rows: vec![
                row(Icon::Status(43), "Refresh II", 9.0, 150.0, false),
                row(Icon::Status(33), "Haste II", 134.0, 180.0, false),
                row(Icon::Status(40), "Protect IV", 1561.0, 1800.0, false),
            ] },
            Group { name: Some("Tuxedo".into()), rows: vec![
                row(Icon::Status(116), "Phalanx II", 90.0, 240.0, false),
                row(Icon::Status(33), "Haste II", 170.0, 180.0, false),
            ] },
        ],
        vec![Group { name: Some("Valkurm Emperor".into()), rows: vec![
            row(Icon::Status(4), "Paralyze", -1.0, 120.0, false),
            row(Icon::Status(134), "Dia II", 70.0, 120.0, false),
            row(Icon::Status(13), "Slow II", 160.0, 180.0, false),
        ] }],
    ];
    let log_top = sh - 16.0 - 8.0 * 16.0 - 14.0;
    let foot = sh - 16.0 - 34.0 - 46.0;
    let p = place(dock, sw, sh, 4, log_top, foot, mini);
    draw_panel(gu, &secs, folded, &p, slide, sw, 0.5, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FUN_10225120 / FUN_10225220: Stratagems at SCH 99 (5 charges of 48 s, 240 s whole) with 100 s left: 4 s to
    /// the next, 2 ready; Ready (3 of 30 s) with Calc2 -15 (merits, 5 s each): 25-s shares.
    #[test]
    fn charge_maths() {
        assert_eq!(charges(100, 5, 240, (0, 0)), Some((4, 2, 48)));
        assert_eq!(charges(96, 5, 240, (0, 0)), Some((48, 3, 48)));
        assert_eq!(charges(0, 5, 240, (0, 0)), Some((0, 5, 48)));
        assert_eq!(charges(60, 3, 90, (0, -15)), Some((10, 0, 25)));
        assert_eq!(charges(10, 0, 90, (0, 0)), None);
    }

    #[test]
    fn clocks() {
        assert_eq!([clock(0.2), clock(59.0), clock(61.5), clock(3600.0), clock(3725.0)], ["0:01", "0:59", "1:02", "1:00:00", "1:02:05"]);
    }

    fn row(left: f32) -> Row {
        Row { icon: Icon::None, name: "x".into(), left, total: 60.0, fills: false }
    }

    /// Rows go from the last section up when the height runs out; the headings count what is hidden.
    #[test]
    fn fitting() {
        let secs = [vec![Group { name: None, rows: vec![row(1.0), row(2.0)] }], vec![Group { name: Some("A".into()), rows: vec![row(3.0)] }], vec![Group { name: Some("Mob".into()), rows: vec![row(4.0), row(5.0)] }]];
        let all = HEAD_H * 3.0 + GROUP_H * 2.0 + ROW_H * 5.0 + 2.0 * PAD;
        let (l, hidden) = lines(&secs, 0, all);
        assert_eq!((l.len(), hidden), (10, [0, 0, 0]));
        let (l, hidden) = lines(&secs, 0, all - ROW_H);
        assert_eq!((l.len(), hidden), (9, [0, 0, 1]));
        // Both of the monster's rows gone: its name goes too.
        let (l, hidden) = lines(&secs, 0, all - 2.0 * ROW_H);
        assert_eq!((l.len(), hidden), (7, [0, 0, 2]));
        // Folded: the heading alone.
        let (l, hidden) = lines(&secs, 0b001, all);
        assert_eq!((l.len(), hidden), (8, [2, 0, 0]));
    }

    /// The luopan at level 99: 1650 HP draining 24 a tick (69 ticks, 207 s); with Blaze of Glory half of it.
    #[test]
    fn luopan() {
        let mut c = durations::Caster { main: (21, 99), ..Default::default() };
        assert_eq!(luopan_life(&c), 207.0);
        c.statuses.insert(569);
        assert_eq!(luopan_life(&c), 105.0);
    }

    #[test]
    fn places() {
        let p = place(Dock::Left, 1280.0, 800.0, 3, 650.0, 600.0, None);
        assert_eq!((p.x, p.y, p.max_h), (16.0, 122.0, 650.0 - 96.0 - 122.0));
        // Three icon rows reach past y 122.
        let p = place(Dock::Left, 1280.0, 800.0, 20, 650.0, 600.0, None);
        assert_eq!(p.y, 48.0 + 2.0 + 78.0 + 4.0);
        let p = place(Dock::Right, 1280.0, 800.0, 0, 650.0, 600.0, None);
        assert_eq!((p.x, p.y, p.max_h), (1280.0 - 16.0 - WIDTH, 596.0, 596.0 - 252.0));
        // A minimap at the top left: the left dock starts under it; at the top right the top dock keeps left of it.
        let mini = crate::client::minimap::place(1, 1, 1280.0, 0, None);
        assert_eq!(place(Dock::Left, 1280.0, 800.0, 0, 650.0, 600.0, Some(mini)).y, mini[1] + mini[3] + 6.0);
        let mini = crate::client::minimap::place(0, 2, 1280.0, 0, None);
        let p = place(Dock::Top, 1280.0, 800.0, 0, 650.0, 600.0, Some(mini));
        assert!(p.x + WIDTH <= mini[0] - 4.0, "{}", p.x);
        // A large one reaches below the main menu: the right dock stops above its foot.
        let p = place(Dock::Right, 1280.0, 800.0, 0, 650.0, 600.0, Some(mini));
        assert_eq!(p.max_h, 596.0 - (mini[1] + mini[3] + 6.0));
    }
}
