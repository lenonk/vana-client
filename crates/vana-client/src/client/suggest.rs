//! Action suggestions (not retail; docs/modern-ui.md "Recommendations"): a deterministic rule set over the game's
//! state naming a spell or ability worth using now, and why. It never acts, never moves a cursor or a target; the
//! windows show its picks apart from their lists and take them only when chosen.

use super::World;

/// What a suggestion uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Spell(u16),
    Ability(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub action: Action,
    /// Who it is for (a name), when not ourselves.
    pub target: Option<String>,
    pub reason: String,
}

/// A party member (ourselves first): in our zone, HP now / max, effects.
#[derive(Clone, Debug, Default)]
pub struct Member {
    pub name: String,
    pub me: bool,
    pub here: bool,
    pub hp: u32,
    pub hp_max: u32,
    pub effects: Vec<u16>,
}

/// The facts the rules read.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub party: Vec<Member>,
    /// Our effects and their seconds left (untimed: none).
    pub own: Vec<(u16, Option<f32>)>,
    pub tp: u32,
    pub engaged: bool,
    /// Spells castable now (known, MP, recast, status) and weapon skills ready now.
    pub spells: Vec<u16>,
    pub weapon_skills: Vec<u16>,
}

/// Suggestions at most.
pub const MAX: usize = 3;

/// Ailments and the spell that lifts each: poison, paralysis, blindness, silence, petrification, disease, curse, plague,
/// doom (status ids; spells Poisona .. Cursna).
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

/// Seconds left under which an effect is worth renewing.
const SOON: f32 = 30.0;

fn tier(table: &[(u16, u32)], missing: u32, can: &[u16]) -> Option<u16> {
    let have: Vec<(u16, u32)> = table.iter().copied().filter(|(id, _)| can.contains(id)).collect();
    have.iter().rev().find(|(_, heal)| *heal <= missing).or(have.first()).map(|(id, _)| *id)
}

/// The rules in their order: lift an ailment, heal the lowest (several low: Curaga), renew our expiring effects,
/// weapon skill with the TP for it while engaged.
pub fn suggest(f: &Facts) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let who = |m: &Member| (!m.me).then(|| m.name.clone());
    let named = |m: &Member| if m.me { "You are".to_string() } else { format!("{} is", m.name) };
    for &(effect, spell, word) in &AILMENTS {
        if let Some(m) = f.party.iter().find(|m| m.here && m.effects.contains(&effect))
            && f.spells.contains(&spell)
            && !out.iter().any(|s: &Suggestion| s.action == Action::Spell(spell))
        {
            out.push(Suggestion { action: Action::Spell(spell), target: who(m), reason: format!("{} {word}.", named(m)) });
        }
    }
    let low: Vec<&Member> = f.party.iter().filter(|m| m.here && m.hp_max > 0 && m.hp > 0 && m.hp * 4 <= m.hp_max * 3).collect();
    if low.len() >= 3
        && let Some(id) = tier(&CURAGAS, low.iter().map(|m| m.hp_max - m.hp).sum::<u32>() / low.len() as u32, &f.spells)
    {
        out.push(Suggestion { action: Action::Spell(id), target: None, reason: format!("{} members are below 75% HP.", low.len()) });
    } else if let Some(m) = low.iter().min_by_key(|m| m.hp * 100 / m.hp_max)
        && let Some(id) = tier(&CURES, m.hp_max - m.hp, &f.spells)
    {
        let pct = m.hp * 100 / m.hp_max;
        out.push(Suggestion { action: Action::Spell(id), target: who(m), reason: format!("{} at {pct}% HP ({} missing).", named(m), m.hp_max - m.hp) });
    }
    for &(effect, spells, name) in &RENEW {
        if let Some(&(_, Some(left))) = f.own.iter().find(|(e, _)| *e == effect)
            && left < SOON
            && let Some(&id) = spells.iter().find(|s| f.spells.contains(s))
        {
            out.push(Suggestion { action: Action::Spell(id), target: None, reason: format!("{name} wears off in {}s.", left.ceil() as u32) });
        }
    }
    if f.engaged
        && f.tp >= 1000
        && let Some(&ws) = f.weapon_skills.first()
    {
        out.push(Suggestion { action: Action::Ability(ws), target: None, reason: format!("TP {}: a weapon skill is ready.", f.tp) });
    }
    out.truncate(MAX);
    out
}

/// The facts from the game: the party (ours from our own vitals), our effects' times, TP, engaged, what can be used.
pub fn facts(w: &World) -> Facts {
    let slots = w.party.slots();
    let now = (super::vana_now(w) as u32).wrapping_mul(60);
    let own_ids: Vec<u16> = super::buffs::icons(w).iter().map(|e| e.0).collect();
    let mut party = Vec::new();
    if let (Some((hp, ..)), Some(st)) = (w.vitals, w.stats.as_ref()) {
        party.push(Member { name: w.name.clone(), me: true, here: true, hp, hp_max: st.hp_max, effects: own_ids.clone() });
    }
    for m in slots.slots[1..6].iter().flatten() {
        let max = w.party_hp_max.get(&m.id).copied().filter(|&x| m.hp <= x).unwrap_or_else(|| if m.hpp > 0 { m.hp * 100 / m.hpp as u32 } else { 0 });
        let effects = w.party_buffs.get(&m.id).map_or(Vec::new(), |row| row.iter().copied().filter(|&b| b != 0 && b != 0xffff).collect());
        party.push(Member { name: m.name.clone(), me: false, here: m.away == 0 && m.zone == w.pos_zone, hp: m.hp, hp_max: max, effects });
    }
    let own = super::buffs::icons(w).into_iter().map(|(id, end)| (id, (end != 0x7fff_ffff).then(|| (end.wrapping_sub(now) as i32) as f32 / 60.0))).collect();
    let known: Vec<u16> = (0..1024u16).filter(|&s| w.spells_known.get(s as usize / 8).is_some_and(|b| b & (1 << (s % 8)) != 0)).collect();
    let spells = known.into_iter().filter(|&s| super::magic::magic_check(w, s).is_none()).collect();
    let weapon_skills = super::magic::ability_rows(w, 3).into_iter().filter(|&a| super::magic::ability_check(w, a).is_none()).collect();
    Facts { party, own, tp: w.vitals.map_or(0, |v| v.2), engaged: w.my_motion.status == 1, spells, weapon_skills }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(hp: u32, max: u32) -> Member {
        Member { name: "Lenon".into(), me: true, here: true, hp, hp_max: max, effects: vec![] }
    }

    fn ally(name: &str, hp: u32, max: u32, effects: &[u16]) -> Member {
        Member { name: name.into(), me: false, here: true, hp, hp_max: max, effects: effects.to_vec() }
    }

    #[test]
    fn ailments_first_then_the_lowest_cure() {
        let f = Facts { party: vec![me(900, 1000), ally("Raven", 300, 1000, &[3]), ally("Koru", 800, 1000, &[])], spells: vec![1, 2, 3, 4, 14], ..Default::default() };
        let s = suggest(&f);
        assert_eq!(s[0], Suggestion { action: Action::Spell(14), target: Some("Raven".into()), reason: "Raven is poisoned.".into() });
        // 700 missing: Cure IV (380) is the highest known not past it.
        assert_eq!(s[1].action, Action::Spell(4));
        assert_eq!(s[1].target.as_deref(), Some("Raven"));
    }

    #[test]
    fn several_low_curaga_and_no_spell_no_suggestion() {
        let f = Facts { party: vec![me(500, 1000), ally("A", 600, 1000, &[]), ally("B", 400, 1000, &[])], spells: vec![1, 7, 8], ..Default::default() };
        assert_eq!(suggest(&f)[0].action, Action::Spell(8));
        let none = Facts { party: vec![me(100, 1000)], spells: vec![], ..Default::default() };
        assert!(suggest(&none).is_empty());
    }

    #[test]
    fn renew_and_weapon_skills() {
        let f = Facts { party: vec![me(1000, 1000)], own: vec![(43, Some(12.0)), (40, Some(600.0)), (33, None)], tp: 1200, engaged: true, spells: vec![109, 43], weapon_skills: vec![0x2a], ..Default::default() };
        let s = suggest(&f);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].action, s[0].reason.as_str()), (Action::Spell(109), "Refresh wears off in 12s."));
        assert_eq!(s[1].action, Action::Ability(0x2a));
        // Not engaged: no weapon skill.
        assert!(suggest(&Facts { engaged: false, ..f }).iter().all(|s| !matches!(s.action, Action::Ability(_))));
    }
}
