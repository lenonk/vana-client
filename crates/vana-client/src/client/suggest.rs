//! Action suggestions (not retail; docs/modern-ui.md "Recommendations"): a deterministic rule set over the game's
//! state naming a spell, ability or weapon skill worth using now, and why. It never acts, never moves a cursor or a
//! target; the battle menu shows its picks above its commands and takes one only when chosen.

use super::World;
use crate::dat::skillchains::{self as sc, Prop};

/// What a suggestion uses: a spell id, or an ability table index (job abilities and weapon skills).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Spell(u16),
    Ability(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub action: Action,
    /// Who it is for (a name), when not ourselves or the target.
    pub target: Option<String>,
    pub reason: String,
}

/// A party member (ourselves first): in our zone, HP now / max, main job, effects.
#[derive(Clone, Debug, Default)]
pub struct Member {
    pub name: String,
    pub me: bool,
    pub here: bool,
    pub hp: u32,
    pub hp_max: u32,
    pub job: u8,
    pub effects: Vec<u16>,
}

/// A spell castable now.
#[derive(Clone, Debug, Default)]
pub struct SpellFact {
    pub id: u16,
    pub name: String,
    pub skill: u16,
    pub element: u16,
    pub mp: u16,
    pub targets: u16,
}

/// A job ability or weapon skill usable now (its table index, name); a weapon skill's properties and power order.
#[derive(Clone, Debug, Default)]
pub struct AbilityFact {
    pub index: u16,
    pub name: String,
    pub props: [Prop; 3],
    pub power: u16,
}

/// The monster we fight or target.
#[derive(Clone, Debug, Default)]
pub struct TargetFact {
    pub name: String,
    pub hpp: u8,
    /// Our effects on it (status ids).
    pub ours: Vec<u16>,
    /// Who it last hit (a party member's name; ourselves: `me`).
    pub hitting: Option<String>,
    pub hitting_me: bool,
}

/// The target's resonance (skillchain.rs): properties (or the chain), seconds since and seconds left.
#[derive(Clone, Debug, Default)]
pub struct ResonanceFact {
    pub against: Vec<Prop>,
    pub chain: Option<Prop>,
    pub age: f32,
    pub left: f32,
}

/// A party member's skills for opening a chain with: name, TP, properties of each.
#[derive(Clone, Debug, Default)]
pub struct Partner {
    pub name: String,
    pub tp: u32,
    pub skills: Vec<(String, [Prop; 3])>,
}

/// The facts the rules read.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub party: Vec<Member>,
    /// Our effects and their seconds left (untimed: none).
    pub own: Vec<(u16, Option<f32>)>,
    pub tp: u32,
    pub engaged: bool,
    pub spells: Vec<SpellFact>,
    pub abilities: Vec<AbilityFact>,
    pub weapon_skills: Vec<AbilityFact>,
    pub target: Option<TargetFact>,
    pub resonance: Option<ResonanceFact>,
    pub partners: Vec<Partner>,
}

impl Facts {
    fn spell(&self, id: u16) -> Option<&SpellFact> {
        self.spells.iter().find(|s| s.id == id)
    }

    fn can(&self, id: u16) -> bool {
        self.spell(id).is_some()
    }

    fn ability(&self, name: &str) -> Option<&AbilityFact> {
        self.abilities.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    fn has(&self, status: u16) -> bool {
        self.own.iter().any(|(e, _)| *e == status)
    }

    /// Fighting: engaged, or a monster is on us.
    fn fighting(&self) -> bool {
        self.engaged || self.target.as_ref().is_some_and(|t| t.hitting_me)
    }
}

/// Suggestions at most.
pub const MAX: usize = 4;

/// Ailments and the spell that lifts each: petrification, doom, silence, paralysis, curse, poison, blindness, disease,
/// plague (status ids; spells Stona .. Cursna).
const AILMENTS: [(u16, u16, &str); 9] = [
    (7, 18, "petrified"),
    (15, 20, "doomed"),
    (6, 17, "silenced"),
    (4, 15, "paralyzed"),
    (9, 20, "cursed"),
    (3, 14, "poisoned"),
    (5, 16, "blinded"),
    (8, 19, "diseased"),
    (31, 19, "plagued"),
];

/// Cure I .. VI and Curaga I .. V with a typical heal each (rough, for picking a tier, not a promise).
const CURES: [(u16, u32); 6] = [(1, 40), (2, 90), (3, 190), (4, 380), (5, 590), (6, 700)];
const CURAGAS: [(u16, u32); 5] = [(7, 60), (8, 130), (9, 270), (10, 450), (11, 600)];
/// Raise I .. III, Arise, strongest first.
const RAISES: [u16; 4] = [494, 140, 13, 12];
/// Utsusemi: San, Ni, Ichi; the Copy Image statuses (1, 2, 3, 4+ shadows).
const UTSUSEMI: [u16; 3] = [340, 339, 338];
const SHADOWS: [u16; 4] = [66, 444, 445, 446];
/// Curing Waltz V .. I (the job ability names).
const WALTZES: [&str; 5] = ["Curing Waltz V", "Curing Waltz IV", "Curing Waltz III", "Curing Waltz II", "Curing Waltz"];

/// Our effects worth renewing before they wear, and their spells: Refresh, Haste, Protect I..V, Shell I..V, Regen
/// I..V, Phalanx, Stoneskin, Blink, Aquaveil, Reraise I..IV.
const RENEW: [(u16, &[u16], &str); 10] = [
    (43, &[109, 473, 894], "Refresh"),
    (33, &[57, 511], "Haste"),
    (40, &[47, 46, 45, 44, 43], "Protect"),
    (41, &[52, 51, 50, 49, 48], "Shell"),
    (42, &[504, 477, 111, 110, 108], "Regen"),
    (116, &[106], "Phalanx"),
    (37, &[54], "Stoneskin"),
    (36, &[53], "Blink"),
    (39, &[55], "Aquaveil"),
    (113, &[848, 142, 141, 135], "Reraise"),
];

/// Effects worth having up between fights when missing, and their spells (strongest first).
const BETWEEN: [(u16, &[u16], &str); 3] = [(40, &[47, 46, 45, 44, 43], "Protect"), (41, &[52, 51, 50, 49, 48], "Shell"), (113, &[848, 142, 141, 135], "Reraise")];

/// Seconds left under which an effect is worth renewing.
const SOON: f32 = 30.0;

/// Debuffs to keep on what we fight, in order: the status and the spells that give it (strongest first). Dia / Bio,
/// Slow, Paralyze, Blind, Gravity, and the ninjutsu Hojo / Jubaku / Kurayami / Dokumori, the songs Elegy / Requiem.
const DEBUFFS: [(u16, &[u16], &str); 9] = [
    (134, &[25, 24, 23], "Dia"),
    (135, &[232, 231, 230], "Bio"),
    (13, &[79, 56, 345, 344], "slowed"),
    (4, &[80, 58, 342, 341], "paralyzed"),
    (5, &[276, 254, 348, 347], "blinded"),
    (12, &[217, 216], "weighed down"),
    (194, &[422, 421], "Elegy"),
    (192, &[374, 373, 372, 371, 370, 369, 368], "Requiem"),
    (3, &[350], "poisoned"),
];

/// Job abilities worth using while fighting when their effect is not up: the name, its status (0: none to check), and
/// when. Yonin / Innin and the sambas have rules of their own.
#[derive(Clone, Copy, PartialEq)]
enum When {
    Fighting,
    TpUnder,
    TpReady,
    HpOver,
    HpUnder,
    Targeted,
}

const BUFFS: [(&str, u16, When, &str); 14] = [
    ("Berserk", 56, When::Fighting, "attack up"),
    ("Warcry", 68, When::Fighting, "the party's attack up"),
    ("Last Resort", 64, When::Fighting, "attack up"),
    ("Souleater", 63, When::HpOver, "HP into damage while it is high"),
    ("Hasso", 353, When::Fighting, "faster two-handed swings"),
    ("Meditate", 0, When::TpUnder, "TP now"),
    ("Third Eye", 67, When::Targeted, "the monster is on you"),
    ("Sneak Attack", 65, When::TpReady, "before the weapon skill"),
    ("Focus", 59, When::Fighting, "accuracy up"),
    ("Chakra", 0, When::HpUnder, "HP low"),
    ("Sentinel", 62, When::HpUnder, "HP low while the monster is on you"),
    ("Jump", 0, When::Fighting, "damage and TP"),
    ("High Jump", 0, When::Fighting, "damage and TP"),
    ("Aggressor", 58, When::Fighting, "accuracy up"),
];

/// "the monster is on you" as a sentence.
fn sentence(why: &str) -> String {
    let mut c = why.chars();
    c.next().map_or_else(String::new, |f| format!("{}{}.", f.to_uppercase(), c.as_str()))
}

fn tier(table: &[(u16, u32)], missing: u32, can: impl Fn(u16) -> bool) -> Option<u16> {
    let have: Vec<(u16, u32)> = table.iter().copied().filter(|(id, _)| can(*id)).collect();
    have.iter().rev().find(|(_, heal)| *heal <= missing).or(have.first()).map(|(id, _)| *id)
}

/// A spell that does magic damage of `element` (a burst candidate): elemental and divine magic, Drain / Aspir, the
/// elemental ninjutsu; not the damage-over-time spells.
fn nuke(s: &SpellFact) -> bool {
    let dot = ["Burn", "Frost", "Choke", "Rasp", "Shock", "Drown", "Bio", "Dia"].iter().any(|p| s.name.starts_with(p));
    s.targets & 0x20 != 0 && !dot && (matches!(s.skill, 32 | 36) || s.name.starts_with("Drain") || s.name.starts_with("Aspir") || (s.skill == 39 && s.name.contains("ton:")))
}

/// The rules in their order: raise, shadows, ailments, heal, close the open chain, burst it, pull the monster off
/// someone, job abilities, debuffs, open a chain with a partner, the strongest weapon skill, renew what wears off.
pub fn suggest(f: &Facts) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let push = |out: &mut Vec<Suggestion>, s: Suggestion| {
        if !out.iter().any(|o| o.action == s.action) {
            out.push(s);
        }
    };
    let who = |m: &Member| (!m.me).then(|| m.name.clone());
    let named = |m: &Member| if m.me { "You are".to_string() } else { format!("{} is", m.name) };
    let me = f.party.first();

    // Raise the fallen.
    if let Some(m) = f.party.iter().find(|m| !m.me && m.here && m.hp_max > 0 && m.hp == 0)
        && let Some(&id) = RAISES.iter().find(|&&s| f.can(s))
    {
        push(&mut out, Suggestion { action: Action::Spell(id), target: who(m), reason: format!("{} KO'd.", named(m)) });
    }
    // Shadows.
    if f.fighting()
        && !SHADOWS.iter().any(|&s| f.has(s))
        && let Some(&id) = UTSUSEMI.iter().find(|&&s| f.can(s))
    {
        push(&mut out, Suggestion { action: Action::Spell(id), target: None, reason: "No shadows up.".into() });
    }
    // Ailments.
    for &(effect, spell, word) in &AILMENTS {
        if let Some(m) = f.party.iter().find(|m| m.here && m.effects.contains(&effect))
            && f.can(spell)
        {
            push(&mut out, Suggestion { action: Action::Spell(spell), target: who(m), reason: format!("{} {word}.", named(m)) });
        }
    }
    // Healing: several low, Curaga; else the lowest; a waltz when no cure is known.
    let low: Vec<&Member> = f.party.iter().filter(|m| m.here && m.hp_max > 0 && m.hp > 0 && m.hp * 4 <= m.hp_max * 3).collect();
    if low.len() >= 3
        && let Some(id) = tier(&CURAGAS, low.iter().map(|m| m.hp_max - m.hp).sum::<u32>() / low.len() as u32, |s| f.can(s))
    {
        push(&mut out, Suggestion { action: Action::Spell(id), target: None, reason: format!("{} members are below 75% HP.", low.len()) });
    } else if let Some(m) = low.iter().min_by_key(|m| m.hp * 100 / m.hp_max) {
        let pct = m.hp * 100 / m.hp_max;
        let reason = format!("{} at {pct}% HP ({} missing).", named(m), m.hp_max - m.hp);
        if let Some(id) = tier(&CURES, m.hp_max - m.hp, |s| f.can(s)) {
            push(&mut out, Suggestion { action: Action::Spell(id), target: who(m), reason });
        } else if pct < 60
            && let Some(a) = WALTZES.iter().find_map(|n| f.ability(n))
        {
            push(&mut out, Suggestion { action: Action::Ability(a.index), target: who(m), reason });
        }
    }
    // Close the open chain, or burst the one made.
    if let Some(r) = &f.resonance {
        match r.chain {
            Some(c) if r.left > 1.0 => {
                let els = sc::elements(c);
                let best = f.spells.iter().filter(|s| nuke(s) && sc::Element::of_spell(s.element as u8).is_some_and(|e| els.contains(&e))).max_by_key(|s| s.mp);
                if let Some(s) = best {
                    push(&mut out, Suggestion { action: Action::Spell(s.id), target: None, reason: format!("Burst {}: {:.0} s left.", super::skillchain::chain_text(c), r.left) });
                }
            }
            _ => {}
        }
        if f.tp >= 1000 && r.left > 0.5 {
            let close = f.weapon_skills.iter().filter_map(|a| sc::form(&r.against, &a.props).map(|c| (a, c))).max_by_key(|(a, c)| (sc::level(*c), a.power));
            if let Some((a, c)) = close {
                let when = if r.age < super::skillchain::WAIT { format!(" in {:.1} s", super::skillchain::WAIT - r.age) } else { String::new() };
                push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: format!("Closes the open chain{when}: {}.", super::skillchain::chain_text(c)) });
            }
        }
    }
    // The monster on someone who should not hold it.
    if let Some(t) = &f.target
        && let Some(name) = &t.hitting
        && !t.hitting_me
        && let Some(m) = f.party.iter().find(|m| &m.name == name)
        && (!matches!(m.job, 7 | 13 | 22) || m.hp * 2 < m.hp_max)
        && let Some(a) = f.ability("Provoke").map(|a| Action::Ability(a.index)).or_else(|| f.can(112).then_some(Action::Spell(112)))
    {
        push(&mut out, Suggestion { action: a, target: None, reason: format!("The {} is attacking {name}.", t.name) });
    }
    // Job abilities.
    if f.fighting() {
        let hp_pct = me.map_or(100, |m| if m.hp_max > 0 { m.hp * 100 / m.hp_max } else { 100 });
        let targeted = f.target.as_ref().is_some_and(|t| t.hitting_me);
        for &(name, status, when, why) in &BUFFS {
            let Some(a) = f.ability(name) else { continue };
            let ok = match when {
                When::Fighting => true,
                When::TpUnder => f.tp < 1000,
                When::TpReady => f.tp >= 1000 && !f.weapon_skills.is_empty(),
                When::HpOver => hp_pct > 75,
                When::HpUnder => hp_pct < 50 && (name != "Sentinel" || targeted),
                When::Targeted => targeted,
            };
            if ok && (status == 0 || !f.has(status)) && !(name == "Hasso" && f.has(354)) {
                push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: sentence(why) });
            }
        }
        // Yonin while the monster is on us, else Innin.
        if !f.has(420) && !f.has(421) {
            let (name, why) = if targeted { ("Yonin", "the monster is on you") } else { ("Innin", "you are not holding the monster") };
            if let Some(a) = f.ability(name) {
                push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: sentence(why) });
            }
        }
        // A samba while none is up.
        if ![368, 369, 370].iter().any(|&s| f.has(s))
            && let Some(a) = ["Haste Samba", "Drain Samba III", "Drain Samba II", "Drain Samba"].iter().find_map(|n| f.ability(n))
        {
            push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: "No samba up.".into() });
        }
    }
    // Debuffs on what we fight, while it has the HP for them to matter.
    if f.engaged
        && let Some(t) = f.target.as_ref().filter(|t| t.hpp > 30)
    {
        for &(status, spells, word) in &DEBUFFS {
            if t.ours.contains(&status) {
                continue;
            }
            if let Some(&id) = spells.iter().find(|&&s| f.can(s)) {
                let reason = if word.chars().next().is_some_and(char::is_uppercase) { format!("{word} is not on the {}.", t.name) } else { format!("The {} is not {word}.", t.name) };
                push(&mut out, Suggestion { action: Action::Spell(id), target: None, reason });
                break;
            }
        }
    }
    // Open a chain someone with TP can close.
    if f.engaged && f.tp >= 1000 && f.resonance.as_ref().is_none_or(|r| r.left <= 0.5) {
        let mut best: Option<(&AbilityFact, Prop, &Partner, &str)> = None;
        for a in &f.weapon_skills {
            for p in f.partners.iter().filter(|p| p.tp >= 1000) {
                for (sname, props) in &p.skills {
                    if let Some(c) = sc::form(&a.props, props)
                        && best.is_none_or(|b| (sc::level(c), a.power) > (sc::level(b.1), b.0.power))
                    {
                        best = Some((a, c, p, sname));
                    }
                }
            }
        }
        if let Some((a, c, p, sname)) = best {
            push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: format!("Opens {} with {}'s {sname} (TP {}).", super::skillchain::chain_text(c), p.name, p.tp) });
        }
    }
    // The strongest weapon skill.
    if f.engaged
        && f.tp >= 1000
        && let Some(a) = f.weapon_skills.iter().max_by_key(|a| a.power)
    {
        push(&mut out, Suggestion { action: Action::Ability(a.index), target: None, reason: format!("Your strongest weapon skill (TP {}).", f.tp) });
    }
    // Renew what wears off soon; between fights, put back what is missing.
    for &(effect, spells, name) in &RENEW {
        if let Some(&(_, Some(left))) = f.own.iter().find(|(e, _)| *e == effect)
            && left < SOON
            && let Some(&id) = spells.iter().find(|&&s| f.can(s))
        {
            push(&mut out, Suggestion { action: Action::Spell(id), target: None, reason: format!("{name} wears off in {}s.", left.ceil() as u32) });
        }
    }
    if !f.fighting() {
        for &(effect, spells, name) in &BETWEEN {
            if !f.has(effect)
                && let Some(&id) = spells.iter().find(|&&s| f.can(s))
            {
                push(&mut out, Suggestion { action: Action::Spell(id), target: None, reason: format!("{name} is not on you.") });
            }
        }
    }
    out.truncate(MAX);
    out
}

/// The battle menu's suggestions (the modern UI's): retaken every half second while its cursor is on the commands, held
/// while it is on one of them; none without the menu.
pub fn refresh_root(w: &mut World) {
    if !w.modern || w.action_menu.is_none() || w.prompt.is_some() {
        (w.root_sugg, w.root_sugg_at, w.suggest_sel) = (Vec::new(), None, None);
        return;
    }
    if w.suggest_sel.is_some() && !w.root_sugg.is_empty() {
        return;
    }
    if w.root_sugg_at.is_none_or(|t| t.elapsed().as_secs_f32() >= 0.5) {
        w.root_sugg = suggest(&facts(w));
        w.root_sugg_at = Some(std::time::Instant::now());
        w.suggest_sel = None;
    }
}

/// The facts from the game.
pub fn facts(w: &World) -> Facts {
    use super::magic;
    let slots = w.party.slots();
    let now = (super::vana_now(w) as u32).wrapping_mul(60);
    let own_ids: Vec<u16> = super::buffs::icons(w).iter().map(|e| e.0).collect();
    let mut party = Vec::new();
    if let (Some((hp, ..)), Some(st)) = (w.vitals, w.stats.as_ref()) {
        party.push(Member { name: w.name.clone(), me: true, here: true, hp, hp_max: st.hp_max, job: st.main.0, effects: own_ids.clone() });
    }
    for m in slots.slots[1..6].iter().flatten() {
        let max = w.party_hp_max.get(&m.id).copied().filter(|&x| m.hp <= x).unwrap_or_else(|| if m.hpp > 0 { m.hp * 100 / m.hpp as u32 } else { 0 });
        let effects = w.party_buffs.get(&m.id).map_or(Vec::new(), |row| row.iter().copied().filter(|&b| b != 0 && b != 0xffff).collect());
        party.push(Member { name: m.name.clone(), me: false, here: m.away == 0 && m.zone == w.pos_zone, hp: m.hp, hp_max: max, job: m.jobs[0], effects });
    }
    let own = super::buffs::icons(w).into_iter().map(|(id, end)| (id, (end != 0x7fff_ffff).then(|| (end.wrapping_sub(now) as i32) as f32 / 60.0))).collect();
    let known: Vec<u16> = (0..1024u16).filter(|&s| w.spells_known.get(s as usize / 8).is_some_and(|b| b & (1 << (s % 8)) != 0)).collect();
    let spells = known
        .into_iter()
        .filter(|&s| magic::magic_check(w, s).is_none())
        .filter_map(|s| {
            let r = w.actions.as_ref()?.spell(s)?;
            Some(SpellFact { id: s, name: magic::spell_name(w, s), skill: r.skill, element: r.element, mp: r.mp, targets: r.targets })
        })
        .collect();
    let d = sc::data();
    let ready = |ty: u16| magic::ability_rows(w, ty).into_iter().filter(|&a| magic::ability_check(w, a).is_none());
    let abilities = ready(1).map(|i| AbilityFact { index: i, name: magic::ability_name(w, i), ..Default::default() }).collect();
    let weapon_skills = ready(3)
        .map(|i| {
            let ws = w.actions.as_ref().and_then(|a| a.abilities.get(i as usize)).map_or(i, |a| a.id);
            let e = d.weapon.get(&ws);
            AbilityFact { index: i, name: magic::ability_name(w, i), props: e.map_or([0; 3], |e| e.props), power: e.map_or(0, |e| e.skill) }
        })
        .collect();
    let target_id = super::skillchain::focus(w);
    let target = target_id.and_then(|t| w.entities.get(&t).map(|e| (t, e))).map(|(t, e)| {
        let hit = super::skillchain::hitting(w, t);
        TargetFact {
            name: super::monstrosity::name(e).to_string(),
            hpp: e.hpp,
            ours: w.timers.on.get(&t).map_or(Vec::new(), |b| b.effects.iter().map(|x| x.status).collect()),
            hitting_me: hit == Some(w.my_id),
            hitting: hit.filter(|&h| h != w.my_id).and_then(|h| w.party.party().find(|m| m.id == h).map(|m| m.name.clone())),
        }
    });
    let resonance = target_id.and_then(|t| w.skillchain.on.get(&t)).filter(|r| r.left() > 0.0).map(|r| ResonanceFact { against: r.against(), chain: r.chain, age: r.age(), left: r.left() });
    let partners = super::skillchain::members(w).into_iter().filter(|m| !m.me).map(|m| Partner { name: m.name.clone(), tp: m.tp, skills: m.skills.iter().map(|&s| (super::skillchain::name(w, s), d.props(s))).collect() }).collect();
    Facts { party, own, tp: w.vitals.map_or(0, |v| v.2), engaged: w.my_motion.status == 1, spells, abilities, weapon_skills, target, resonance, partners }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(hp: u32, max: u32) -> Member {
        Member { name: "Lenon".into(), me: true, here: true, hp, hp_max: max, ..Default::default() }
    }

    fn ally(name: &str, hp: u32, max: u32, effects: &[u16]) -> Member {
        Member { name: name.into(), me: false, here: true, hp, hp_max: max, effects: effects.to_vec(), ..Default::default() }
    }

    fn spells(ids: &[u16]) -> Vec<SpellFact> {
        ids.iter().map(|&id| SpellFact { id, ..Default::default() }).collect()
    }

    fn ws(index: u16, name: &str, props: [Prop; 3], power: u16) -> AbilityFact {
        AbilityFact { index, name: name.into(), props, power }
    }

    #[test]
    fn ailments_first_then_the_lowest_cure() {
        let f = Facts { party: vec![me(900, 1000), ally("Raven", 300, 1000, &[3]), ally("Koru", 800, 1000, &[])], spells: spells(&[1, 2, 3, 4, 14]), ..Default::default() };
        let s = suggest(&f);
        assert_eq!(s[0], Suggestion { action: Action::Spell(14), target: Some("Raven".into()), reason: "Raven is poisoned.".into() });
        // 700 missing: Cure IV (380) is the highest known not past it.
        assert_eq!(s[1].action, Action::Spell(4));
        assert_eq!(s[1].target.as_deref(), Some("Raven"));
    }

    #[test]
    fn several_low_curaga_and_no_spell_no_suggestion() {
        let f = Facts { party: vec![me(500, 1000), ally("A", 600, 1000, &[]), ally("B", 400, 1000, &[])], spells: spells(&[1, 7, 8]), ..Default::default() };
        assert_eq!(suggest(&f)[0].action, Action::Spell(8));
        let none = Facts { party: vec![me(100, 1000)], ..Default::default() };
        assert!(suggest(&none).is_empty());
    }

    #[test]
    fn ninja_shadows_stance_and_strongest_weapon_skill() {
        // A NIN 55 engaged, no shadows: Utsusemi: Ni, Innin (not held), then the strongest of its weapon skills.
        let f = Facts {
            party: vec![me(800, 1000)],
            tp: 1200,
            engaged: true,
            spells: spells(&[338, 339]),
            abilities: vec![AbilityFact { index: 700, name: "Innin".into(), ..Default::default() }],
            weapon_skills: vec![ws(128, "Blade: Rin", [1, 0, 0], 5), ws(131, "Blade: Teki", [10, 0, 0], 75), ws(133, "Blade: Ten", [9, 0, 0], 225)],
            target: Some(TargetFact { name: "Bat".into(), hpp: 90, ..Default::default() }),
            ..Default::default()
        };
        let s = suggest(&f);
        assert_eq!(s[0].action, Action::Spell(339));
        assert_eq!(s[1].action, Action::Ability(700));
        assert_eq!(s[2].action, Action::Ability(133));
    }

    #[test]
    fn close_and_burst() {
        // Liquefaction open on the target 4 s ago: an Impaction weapon skill closes it into Fusion.
        let mut f = Facts {
            party: vec![me(1000, 1000)],
            tp: 1100,
            engaged: true,
            weapon_skills: vec![ws(1, "Shoulder Tackle", [5, 8, 0], 40), ws(2, "Combo", [8, 0, 0], 5)],
            resonance: Some(ResonanceFact { against: vec![3], chain: None, age: 4.0, left: 6.0 }),
            ..Default::default()
        };
        let s = suggest(&f);
        assert_eq!(s[0].action, Action::Ability(1));
        assert!(s[0].reason.contains("Fusion"), "{}", s[0].reason);
        // Fusion made: a Fire nuke bursts it.
        f.resonance = Some(ResonanceFact { against: vec![11], chain: Some(11), age: 1.0, left: 8.0 });
        f.tp = 0;
        f.spells = vec![SpellFact { id: 144, name: "Fire".into(), skill: 36, element: 0, mp: 9, targets: 0x20, ..Default::default() }, SpellFact { id: 149, name: "Blizzard".into(), skill: 36, element: 1, mp: 8, targets: 0x20, ..Default::default() }];
        assert_eq!(suggest(&f)[0].action, Action::Spell(144));
    }

    #[test]
    fn renew_and_debuff() {
        let f = Facts {
            party: vec![me(1000, 1000)],
            own: vec![(43, Some(12.0)), (40, Some(600.0)), (33, None)],
            engaged: true,
            spells: spells(&[109, 43, 23]),
            target: Some(TargetFact { name: "Crab".into(), hpp: 80, ..Default::default() }),
            ..Default::default()
        };
        let s = suggest(&f);
        assert_eq!(s[0], Suggestion { action: Action::Spell(23), target: None, reason: "Dia is not on the Crab.".into() });
        assert_eq!((s[1].action, s[1].reason.as_str()), (Action::Spell(109), "Refresh wears off in 12s."));
    }
}
