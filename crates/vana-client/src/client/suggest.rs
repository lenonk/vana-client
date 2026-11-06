//! Action suggestions (not retail; docs/modern-ui.md "Recommendations"): a deterministic rule set over the game's
//! state naming a spell, ability or weapon skill worth using now, and why. It never acts, never moves a cursor or a
//! target; the battle menu shows its picks above its commands and takes one only when chosen.

use super::World;
use crate::dat::skillchains::{self as sc, Prop};
use serde::{Deserialize, Serialize};

/// What a suggestion uses: a spell id, or an ability table index (job abilities and weapon skills).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Spell(u16),
    Ability(u16),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub action: Action,
    /// Who it is for (a name), when not ourselves or the target.
    pub target: Option<String>,
    pub reason: String,
    /// A spell's family castable now (its tiers and area forms, strongest single-target first), with the family's
    /// name: the row's submenu. Empty when the family has only the one spell.
    #[serde(default)]
    pub group: Vec<u16>,
    #[serde(default)]
    pub label: Option<String>,
}

impl Suggestion {
    fn new(action: Action, target: Option<String>, reason: String) -> Self {
        Self { action, target, reason, group: Vec::new(), label: None }
    }
}

/// A party member (ourselves first): in our zone, HP now / max, main job, effects.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpellFact {
    pub id: u16,
    pub name: String,
    pub skill: u16,
    pub element: u16,
    pub mp: u16,
    pub targets: u16,
    /// Always hits an area (spell record +0x47).
    pub area: bool,
}

/// A job ability or weapon skill usable now (its table index, name); a weapon skill's properties and power order.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AbilityFact {
    pub index: u16,
    pub name: String,
    pub props: [Prop; 3],
    pub power: u16,
}

/// The monster we fight or target.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetFact {
    pub name: String,
    pub hpp: u8,
    /// Our effects on it (status ids).
    pub ours: Vec<u16>,
    /// Who it last hit (a party member's name; ourselves: `me`).
    pub hitting: Option<String>,
    pub hitting_me: bool,
    /// Its elemental resistance ranks (LSB's data, dat/resists.rs), when known.
    pub ranks: Option<crate::dat::resists::Ranks>,
}

/// The target's resonance (skillchain.rs): properties (or the chain), seconds since and seconds left.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ResonanceFact {
    pub against: Vec<Prop>,
    pub chain: Option<Prop>,
    pub age: f32,
    pub left: f32,
}

/// A party member's skills for opening a chain with: name, TP, properties of each.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Partner {
    pub name: String,
    pub tp: u32,
    pub skills: Vec<(String, [Prop; 3])>,
}

/// The facts the rules read.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
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
    /// The elements today's weather and day strengthen (spell element ids).
    pub weather_element: Option<u16>,
    pub day_element: Option<u16>,
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

/// Each ninjutsu's tool and the universal tool a NIN main may use instead (LSB's spell_list and HasNinjaTool; the
/// client has no such table): Inoshishinofuda elemental, Shikanofuda support, Chonofuda enfeebling.
/// (first spell, last spell, tool, universal tool).
const NINJA_TOOLS: [(u16, u16, u16, u16); 20] = [
    (318, 318, 2553, 2972),
    (319, 319, 2555, 2973),
    (320, 322, 1161, 2971),
    (323, 325, 1164, 2971),
    (326, 328, 1167, 2971),
    (329, 331, 1170, 2971),
    (332, 334, 1173, 2971),
    (335, 337, 1176, 2971),
    (338, 340, 1179, 2972),
    (341, 343, 1182, 2973),
    (344, 346, 1185, 2973),
    (347, 349, 1188, 2973),
    (350, 352, 1191, 2973),
    (353, 354, 1194, 2972),
    (505, 505, 8803, 2972),
    (506, 506, 8804, 2972),
    (507, 507, 2642, 2972),
    (508, 508, 2643, 2973),
    (509, 509, 2644, 2972),
    (510, 510, 2970, 2972),
];

/// A ninjutsu's (tool, universal tool); None for other spells.
fn ninja_tool(spell: u16) -> Option<(u16, u16)> {
    NINJA_TOOLS.iter().find(|t| (t.0..=t.1).contains(&spell)).map(|t| (t.2, t.3))
}

/// A white-magic main (WHM 3, PLD 7) is offered no black magic (spell kind 2) from its support job, a black-magic main
/// (BLM 4, DRK 8) no white (kind 1) (user, 2026-11-04); other mains take both.
fn other_school(main: u8, kind: u16) -> bool {
    match main {
        3 | 7 => kind == 2,
        4 | 8 => kind == 1,
        _ => false,
    }
}

/// The tool is in the inventory, or the universal one with NIN (13) the main job.
fn has_tool(spell: u16, held: impl Fn(u16) -> bool, main: u8) -> bool {
    ninja_tool(spell).is_none_or(|(own, any)| held(own) || (main == 13 && held(any)))
}

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
    (3, &[221, 220, 350], "poisoned"),
];

/// Debuff suggestions at most at once.
const DEBUFFS_AT_ONCE: usize = 2;

/// The elements' names in reasons, and the elemental nuke families' (the element), by element id.
const ELEMENT_WORDS: [&str; 8] = ["fire", "ice", "wind", "earth", "lightning", "water", "light", "darkness"];
const NUKE_FAMILIES: [&str; 8] = ["Fire", "Ice", "Wind", "Earth", "Lightning", "Water", "Light", "Dark"];

/// Ancient magic, Flare .. Flood II: last in a family, and suggested only when nothing else of the element is known.
fn ancient(id: u16) -> bool {
    (204..=215).contains(&id)
}

/// Weather ids (LSB's Weather) to the element they strengthen: hot spell / heat wave fire, rain / squall water, dust /
/// sand storm earth, wind / gales wind, snow / blizzards ice, thunder / thunderstorms lightning, auroras / stellar glare
/// light, gloom / darkness dark.
fn weather_element(weather: u16) -> Option<u16> {
    Some(match weather {
        4 | 5 => 0,
        6 | 7 => 5,
        8 | 9 => 3,
        10 | 11 => 2,
        12 | 13 => 1,
        14 | 15 => 4,
        16 | 17 => 6,
        18 | 19 => 7,
        _ => return None,
    })
}

/// Firesday .. Darksday to their element ids; each element's day by element id.
const DAY_ELEMENTS: [u16; 8] = [0, 3, 5, 2, 1, 4, 6, 7];
const DAY_NAMES: [&str; 8] = ["Firesday", "Iceday", "Windsday", "Earthsday", "Lightningday", "Watersday", "Lightsday", "Darksday"];

/// A spell's family (a suggestion's submenu): an elemental nuke's element (its tiers, -ga / -ja and ancient magic),
/// else the name without its tier (" II" .. " VI", ": Ichi" ..) and its area form (Protectra, Curaga, Diaga ...).
fn family(s: &SpellFact) -> String {
    if s.skill == 36 && nuke(s) {
        return NUKE_FAMILIES.get(s.element as usize).copied().unwrap_or("Nuke").to_string();
    }
    let mut n = s.name.as_str();
    for t in [" VI", " V", " IV", " III", " II", ": Ichi", ": Ni", ": San"] {
        if let Some(base) = n.strip_suffix(t) {
            n = base;
            break;
        }
    }
    const AREA: [(&str, &str); 7] = [("Protectra", "Protect"), ("Shellra", "Shell"), ("Curaga", "Cure"), ("Diaga", "Dia"), ("Poisonga", "Poison"), ("Banishga", "Banish"), ("Sleepga", "Sleep")];
    AREA.iter().find(|a| a.0 == n).map_or(n, |a| a.1).to_string()
}

/// Each spell suggestion's family castable now: single-target, then area, then ancient magic, each strongest (MP)
/// first; and the family's name when it has more than the one spell.
fn add_groups(f: &Facts, out: &mut [Suggestion]) {
    for s in out.iter_mut() {
        let Action::Spell(id) = s.action else { continue };
        if let Some(fam) = f.spell(id).map(family).filter(|n| !n.is_empty()) {
            set_group(f, s, fam);
        }
    }
}

/// Held rows (the cursor on one): each family retaken from what is castable now, so a spell back from its recast
/// returns to the list. The family is the row's name, else its spell's when that is castable now.
fn regroup(f: &Facts, out: &mut [Suggestion]) {
    for s in out.iter_mut() {
        let Action::Spell(id) = s.action else { continue };
        if let Some(fam) = s.label.clone().or_else(|| f.spell(id).map(family)).filter(|n| !n.is_empty()) {
            set_group(f, s, fam);
        }
    }
}

/// A nuke family's suggestion is always its list's top (a tier back from its recast takes it again).
fn set_group(f: &Facts, s: &mut Suggestion, fam: String) {
    let mut group: Vec<&SpellFact> = f.spells.iter().filter(|x| family(x) == fam).collect();
    group.sort_by_key(|x| (ancient(x.id), x.area, std::cmp::Reverse(x.mp), x.id));
    if group.len() > 1 {
        if NUKE_FAMILIES.contains(&fam.as_str()) {
            s.action = Action::Spell(group[0].id);
        }
        s.group = group.iter().map(|x| x.id).collect();
        s.label = Some(fam);
    } else {
        (s.group, s.label) = (Vec::new(), None);
    }
}

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
        push(&mut out, Suggestion::new(Action::Spell(id), who(m), format!("{} KO'd.", named(m))));
    }
    // Shadows.
    if f.fighting()
        && !SHADOWS.iter().any(|&s| f.has(s))
        && let Some(&id) = UTSUSEMI.iter().find(|&&s| f.can(s))
    {
        push(&mut out, Suggestion::new(Action::Spell(id), None, "No shadows up.".into()));
    }
    // Ailments.
    for &(effect, spell, word) in &AILMENTS {
        if let Some(m) = f.party.iter().find(|m| m.here && m.effects.contains(&effect))
            && f.can(spell)
        {
            push(&mut out, Suggestion::new(Action::Spell(spell), who(m), format!("{} {word}.", named(m))));
        }
    }
    // Healing: several low, Curaga; else the lowest; a waltz when no cure is known.
    let low: Vec<&Member> = f.party.iter().filter(|m| m.here && m.hp_max > 0 && m.hp > 0 && m.hp * 4 <= m.hp_max * 3).collect();
    if low.len() >= 3
        && let Some(id) = tier(&CURAGAS, low.iter().map(|m| m.hp_max - m.hp).sum::<u32>() / low.len() as u32, |s| f.can(s))
    {
        push(&mut out, Suggestion::new(Action::Spell(id), None, format!("{} members are below 75% HP.", low.len())));
    } else if let Some(m) = low.iter().min_by_key(|m| m.hp * 100 / m.hp_max) {
        let pct = m.hp * 100 / m.hp_max;
        let reason = format!("{} at {pct}% HP ({} missing).", named(m), m.hp_max - m.hp);
        if let Some(id) = tier(&CURES, m.hp_max - m.hp, |s| f.can(s)) {
            push(&mut out, Suggestion::new(Action::Spell(id), who(m), reason));
        } else if pct < 60
            && let Some(a) = WALTZES.iter().find_map(|n| f.ability(n))
        {
            push(&mut out, Suggestion::new(Action::Ability(a.index), who(m), reason));
        }
    }
    // Close the open chain, or burst the one made.
    if let Some(r) = &f.resonance {
        match r.chain {
            Some(c) if r.left > 1.0 => {
                let els = sc::elements(c);
                // Not ancient magic while anything else bursts it, one target over an area (Holy before Banishga
                // II), then the strongest: the top of its family's list.
                let best = f.spells.iter().filter(|s| nuke(s) && sc::Element::of_spell(s.element as u8).is_some_and(|e| els.contains(&e))).max_by_key(|s| (!ancient(s.id), !s.area, s.mp));
                if let Some(s) = best {
                    push(&mut out, Suggestion::new(Action::Spell(s.id), None, format!("Burst {}: {:.0} s left.", super::skillchain::chain_text(c), r.left)));
                }
            }
            _ => {}
        }
        if f.tp >= 1000 && r.left > 0.5 {
            let close = f.weapon_skills.iter().filter_map(|a| sc::form(&r.against, &a.props).map(|c| (a, c))).max_by_key(|(a, c)| (sc::level(*c), a.power));
            if let Some((a, c)) = close {
                let when = if r.age < super::skillchain::WAIT { format!(" in {:.1} s", super::skillchain::WAIT - r.age) } else { String::new() };
                push(&mut out, Suggestion::new(Action::Ability(a.index), None, format!("Closes the open chain{when}: {}.", super::skillchain::chain_text(c))));
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
        push(&mut out, Suggestion::new(a, None, format!("The {} is attacking {name}.", t.name)));
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
                push(&mut out, Suggestion::new(Action::Ability(a.index), None, sentence(why)));
            }
        }
        // Yonin while the monster is on us, else Innin.
        if !f.has(420) && !f.has(421) {
            let (name, why) = if targeted { ("Yonin", "the monster is on you") } else { ("Innin", "you are not holding the monster") };
            if let Some(a) = f.ability(name) {
                push(&mut out, Suggestion::new(Action::Ability(a.index), None, sentence(why)));
            }
        }
        // A samba while none is up.
        if ![368, 369, 370].iter().any(|&s| f.has(s))
            && let Some(a) = ["Haste Samba", "Drain Samba III", "Drain Samba II", "Drain Samba"].iter().find_map(|n| f.ability(n))
        {
            push(&mut out, Suggestion::new(Action::Ability(a.index), None, "No samba up.".into()));
        }
    }
    // Debuffs on the monster we target, engaged or not (casters fight from range), while it has the HP for them to
    // matter.
    if let Some(t) = f.target.as_ref().filter(|t| t.hpp > 30) {
        let mut given = 0;
        for &(status, spells, word) in &DEBUFFS {
            if t.ours.contains(&status) || given == DEBUFFS_AT_ONCE {
                continue;
            }
            if let Some(&id) = spells.iter().find(|&&s| f.can(s)) {
                let reason = if word.chars().next().is_some_and(char::is_uppercase) { format!("{word} is not on the {}.", t.name) } else { format!("The {} is not {word}.", t.name) };
                push(&mut out, Suggestion::new(Action::Spell(id), None, reason));
                given += 1;
            }
        }
    }
    // A nuke of the element the target resists least (LSB's ranks), then the one today's weather or day strengthens,
    // then the strongest; a single-target spell before an area one, ancient magic last (its family's order).
    if let Some(t) = &f.target {
        let mut best: Option<((i32, u8, bool, bool, u16), &SpellFact)> = None;
        for s in f.spells.iter().filter(|s| s.skill == 36 && nuke(s) && s.element < 8) {
            let e = s.element;
            let rank = t.ranks.map_or(0, |r| r[e as usize]) as i32;
            let boost = 2 * (f.weather_element == Some(e)) as u8 + (f.day_element == Some(e)) as u8;
            let key = (-rank, boost, !ancient(s.id), !s.area, s.mp);
            if best.is_none_or(|b| key > b.0) {
                best = Some((key, s));
            }
        }
        if let Some((_, s)) = best {
            let word = ELEMENT_WORDS[s.element as usize];
            let reason = match t.ranks.map(|r| r[s.element as usize]) {
                Some(r) if r < 0 => format!("The {} is weak to {word}.", t.name),
                Some(0) => format!("The {} does not resist {word}.", t.name),
                Some(_) => format!("The {} resists {word} least.", t.name),
                None if f.weather_element == Some(s.element) => format!("Today's weather strengthens {word}."),
                None if f.day_element == Some(s.element) => format!("{} strengthens {word}.", DAY_NAMES[s.element as usize]),
                None => format!("Your strongest nuke on the {}.", t.name),
            };
            push(&mut out, Suggestion::new(Action::Spell(s.id), None, reason));
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
            push(&mut out, Suggestion::new(Action::Ability(a.index), None, format!("Opens {} with {}'s {sname} (TP {}).", super::skillchain::chain_text(c), p.name, p.tp)));
        }
    }
    // The strongest weapon skill.
    if f.engaged
        && f.tp >= 1000
        && let Some(a) = f.weapon_skills.iter().max_by_key(|a| a.power)
    {
        push(&mut out, Suggestion::new(Action::Ability(a.index), None, format!("Your strongest weapon skill (TP {}).", f.tp)));
    }
    // Renew what wears off soon; between fights, put back what is missing.
    for &(effect, spells, name) in &RENEW {
        if let Some(&(_, Some(left))) = f.own.iter().find(|(e, _)| *e == effect)
            && left < SOON
            && let Some(&id) = spells.iter().find(|&&s| f.can(s))
        {
            push(&mut out, Suggestion::new(Action::Spell(id), None, format!("{name} wears off in {}s.", left.ceil() as u32)));
        }
    }
    if !f.fighting() {
        for &(effect, spells, name) in &BETWEEN {
            if !f.has(effect)
                && let Some(&id) = spells.iter().find(|&&s| f.can(s))
            {
                push(&mut out, Suggestion::new(Action::Spell(id), None, format!("{name} is not on you.")));
            }
        }
    }
    out.truncate(MAX);
    add_groups(f, &mut out);
    out
}

/// The battle menu's suggestions (the modern UI's): retaken every half second while its cursor is on the commands;
/// while it is on one of them the rows hold still and only their families are retaken (the submenu's cursor kept on
/// its spell); none without the menu.
pub fn refresh_root(w: &mut World) {
    if !w.modern || w.action_menu.is_none() || w.prompt.is_some() {
        (w.root_sugg, w.root_sugg_at, w.suggest_sel, w.suggest_sub) = (Vec::new(), None, None, None);
        return;
    }
    if w.suggest_sel.is_none() {
        w.suggest_sub = None;
    }
    let due = w.root_sugg_at.is_none_or(|t| t.elapsed().as_secs_f32() >= 0.5);
    if let Some(k) = w.suggest_sel.filter(|_| !w.root_sugg.is_empty()) {
        if due {
            let on = w.suggest_sub.and_then(|c| w.root_sugg.get(k)?.group.get(c).copied());
            let f = facts(w);
            regroup(&f, &mut w.root_sugg);
            w.root_sugg_at = Some(std::time::Instant::now());
            if let Some(s) = w.root_sugg.get(k).filter(|_| w.suggest_sub.is_some()) {
                w.suggest_sub = (s.group.len() > 1).then(|| on.and_then(|id| s.group.iter().position(|&g| g == id)).unwrap_or_else(|| family_start(s)));
            }
        }
        return;
    }
    if due {
        let f = facts(w);
        let out = suggest(&f);
        if out != w.root_sugg && !out.is_empty() {
            record(&f, &out);
        }
        w.root_sugg = out;
        w.root_sugg_at = Some(std::time::Instant::now());
        w.suggest_sel = None;
    }
}

/// A recorded game state and what the rules made of it: suggest_cases/*.json, replayed by the tests. `rule` names the
/// rule the case is for (RULES), `note` says what was going on.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Case {
    pub rule: String,
    pub note: String,
    pub facts: Facts,
    pub expect: Vec<Suggestion>,
}

/// The rules by the names cases give them, in suggest's order.
pub const RULES: [&str; 17] = [
    "raise", "shadows", "ailment", "curaga", "cure", "waltz", "burst", "close", "provoke", "job_ability", "ninja_stance", "samba", "debuff", "open_chain", "strongest_ws", "renew", "between",
];

/// With VANA_SUGGEST_RECORD set to a folder, each new set of suggestions is written there with its facts as a case
/// (rule and note left for whoever files it under suggest_cases/).
fn record(f: &Facts, out: &[Suggestion]) {
    let Some(dir) = std::env::var_os("VANA_SUGGEST_RECORD") else { return };
    let dir = std::path::PathBuf::from(dir);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let case = Case { rule: String::new(), note: String::new(), facts: f.clone(), expect: out.to_vec() };
    let path = dir.join(format!("suggest-{ms}.json"));
    match serde_json::to_string_pretty(&case).map_err(std::io::Error::other).and_then(|j| std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, j))) {
        Ok(()) => log::info!("suggest: recorded {}", path.display()),
        Err(e) => log::warn!("suggest: can't record to {}: {e}", dir.display()),
    }
}

/// Where a family row's submenu opens: on the suggested spell.
pub fn family_start(s: &Suggestion) -> usize {
    match s.action {
        Action::Spell(id) => s.group.iter().position(|&g| g == id).unwrap_or(0),
        Action::Ability(_) => 0,
    }
}

/// The keys of a suggestion's family submenu: on a family row Right or Enter opens it, on the suggested spell; open,
/// Up / Down move, Enter casts that spell, Left / Escape close it. True when the key was the submenu's.
pub fn family_key(w: &mut World, on: Option<usize>, key: egui::Key) -> bool {
    use crate::audio::Ui as Se;
    let Some(s) = on.and_then(|k| w.root_sugg.get(k)).filter(|s| s.group.len() > 1).cloned() else {
        w.suggest_sub = None;
        return false;
    };
    let n = s.group.len();
    match (w.suggest_sub, key) {
        (None, egui::Key::ArrowRight | egui::Key::Enter) => {
            w.suggest_sub = Some(family_start(&s));
            w.sounds.push(Se::Select as u32);
        }
        (None, _) => return false,
        (Some(c), egui::Key::ArrowUp | egui::Key::ArrowDown) => {
            w.suggest_sub = Some(if key == egui::Key::ArrowDown { (c + 1) % n } else { (c + n - 1) % n });
            w.sounds.push(Se::Cursor as u32);
        }
        (Some(c), egui::Key::Enter) => {
            w.suggest_sub = None;
            super::listsearch::pick_root(w, Action::Spell(s.group[c.min(n - 1)]), s.target.as_deref());
        }
        (Some(_), egui::Key::ArrowLeft | egui::Key::Escape) => {
            w.suggest_sub = None;
            w.sounds.push(Se::Cancel as u32);
        }
        (Some(_), _) => {}
    }
    true
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
    // Ninjutsu only with its tool in the inventory (the server's check; retail's client has none).
    let held = |item: u16| w.inv.slots.iter().any(|(&(bag, _), &(id, n))| bag == super::items::INVENTORY && id == item && n > 0);
    let main = w.stats.as_ref().map_or(0, |s| s.main.0);
    let spells = known
        .into_iter()
        .filter(|&s| magic::magic_check(w, s).is_none() && has_tool(s, held, main))
        .filter_map(|s| {
            let r = w.actions.as_ref()?.spell(s).filter(|r| !other_school(main, r.kind))?;
            Some(SpellFact { id: s, name: magic::spell_name(w, s), skill: r.skill, element: r.element, mp: r.mp, targets: r.targets, area: r.area != 0 })
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
            ranks: crate::dat::resists::of(w.pos_zone, &e.name),
        }
    });
    let resonance = target_id.and_then(|t| w.skillchain.on.get(&t)).filter(|r| r.left() > 0.0).map(|r| ResonanceFact { against: r.against(), chain: r.chain, age: r.age(), left: r.left() });
    let partners = super::skillchain::members(w).into_iter().filter(|m| !m.me).map(|m| Partner { name: m.name.clone(), tp: m.tp, skills: m.skills.iter().map(|&s| (super::skillchain::name(w, s), d.props(s))).collect() }).collect();
    // Firesday .. Darksday: 3456 s each of the 27648 s week (the location panel's day).
    let day_element = w.clock.map(|(t, at)| DAY_ELEMENTS[((t as u64 + at.elapsed().as_secs()) % 27648 / 3456) as usize]);
    let weather_element = weather_element(super::event::weather(w));
    Facts { party, own, tp: w.vitals.map_or(0, |v| v.2), engaged: w.my_motion.status == 1, spells, abilities, weapon_skills, target, resonance, partners, weather_element, day_element }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/client/suggest_cases")
    }

    /// Every case in suggest_cases/ gives exactly its recorded suggestions. VANA_SUGGEST_BLESS=1 rewrites the
    /// expectations from the rules as they are (check the diff before keeping it).
    #[test]
    fn recorded_cases() {
        let bless = std::env::var_os("VANA_SUGGEST_BLESS").is_some();
        let mut paths: Vec<_> = std::fs::read_dir(cases_dir()).expect("suggest_cases").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        paths.sort();
        assert!(!paths.is_empty());
        let mut failed = Vec::new();
        for p in &paths {
            let mut case: Case = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert!(RULES.contains(&case.rule.as_str()), "{}: rule {:?} is not one of RULES", p.display(), case.rule);
            let got = suggest(&case.facts);
            if bless {
                case.expect = got;
                std::fs::write(p, serde_json::to_string_pretty(&case).unwrap() + "\n").unwrap();
            } else if got != case.expect {
                failed.push(format!("{} ({}): expected {:#?}\n got {:#?}", p.display(), case.note, case.expect, got));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }

    /// Each rule has a case.
    #[test]
    fn every_rule_has_a_case() {
        let rules: Vec<String> = std::fs::read_dir(cases_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| serde_json::from_str::<Case>(&std::fs::read_to_string(e.path()).ok()?).ok())
            .map(|c| c.rule)
            .collect();
        let missing: Vec<&str> = RULES.iter().copied().filter(|r| !rules.iter().any(|x| x == r)).collect();
        assert!(missing.is_empty(), "rules without a case: {missing:?}");
    }

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
        assert_eq!(s[0], Suggestion::new(Action::Spell(14), Some("Raven".into()), "Raven is poisoned.".into()));
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
    fn bursts_prefer_one_target() {
        // Fusion (fire, light): Holy over Banishga II though it costs less.
        let nuke = |id, name: &str, mp, area| SpellFact { id, name: name.into(), skill: 32, element: 6, mp, targets: 0x20, area };
        let f = Facts { party: vec![me(1000, 1000)], resonance: Some(ResonanceFact { against: vec![11], chain: Some(11), age: 1.0, left: 8.0 }), spells: vec![nuke(39, "Banishga II", 120, true), nuke(21, "Holy", 100, false)], ..Default::default() };
        assert_eq!(suggest(&f)[0].action, Action::Spell(21));
    }

    /// BLM / WHM gets no white magic and WHM / BLM no black; RDM and the melee jobs keep both.
    #[test]
    fn mains_keep_to_their_school() {
        assert!(other_school(4, 1) && !other_school(4, 2));
        assert!(other_school(3, 2) && !other_school(3, 1));
        assert!(other_school(7, 2) && other_school(8, 1));
        for main in [1, 5, 16, 20, 21] {
            assert!(!other_school(main, 1) && !other_school(main, 2), "job {main}");
        }
        assert!(!other_school(4, 4), "ninjutsu is neither");
    }

    #[test]
    fn ninjutsu_needs_its_tool() {
        let none = |_| false;
        let shihei = |i| i == 1179;
        let shikanofuda = |i| i == 2972;
        assert!(!has_tool(339, none, 13));
        assert!(has_tool(339, shihei, 1));
        // The universal tool only for a NIN main; Katon takes Inoshishinofuda, not Shikanofuda.
        assert!(has_tool(339, shikanofuda, 13));
        assert!(!has_tool(339, shikanofuda, 1));
        assert!(!has_tool(320, shikanofuda, 13));
        assert!(has_tool(510, |i| i == 2970, 1));
        // Not ninjutsu: no tool.
        assert!(has_tool(1, none, 1));
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
        assert_eq!(s[0], Suggestion::new(Action::Spell(23), None, "Dia is not on the Crab.".into()));
        assert_eq!((s[1].action, s[1].reason.as_str()), (Action::Spell(109), "Refresh wears off in 12s."));
    }

    fn named(id: u16, name: &str, skill: u16, element: u16, mp: u16, area: bool) -> SpellFact {
        SpellFact { id, name: name.into(), skill, element, mp, targets: 0x20, area }
    }

    /// A caster who is not engaged still gets the debuffs (two at most) and a nuke of the element the target resists
    /// least, with its family: the tiers and the area form, single-target first.
    #[test]
    fn caster_debuffs_and_the_weakest_element() {
        let f = Facts {
            party: vec![me(1000, 1000)],
            spells: vec![named(23, "Dia", 35, 6, 7, false), named(230, "Bio", 37, 7, 15, false), named(254, "Blind", 35, 7, 5, false), named(144, "Fire", 36, 0, 7, false), named(145, "Fire II", 36, 0, 26, false), named(174, "Firaga", 36, 0, 57, true), named(164, "Thunder", 36, 4, 9, false)],
            // A rabbit's ranks: thunder (-3) below fire (-2).
            target: Some(TargetFact { name: "Wild Rabbit".into(), hpp: 90, ranks: Some([-2, -1, -2, -1, -3, -3, -1, -3]), ..Default::default() }),
            ..Default::default()
        };
        let s = suggest(&f);
        let spells: Vec<Action> = s.iter().map(|x| x.action).collect();
        assert_eq!(spells, [Action::Spell(23), Action::Spell(230), Action::Spell(164)]);
        assert_eq!(s[2].reason, "The Wild Rabbit is weak to lightning.");
        // One Thunder known: no submenu.
        assert!(s[2].group.is_empty() && s[2].label.is_none());
        // Thunder resisted: fire's family, its single-target spells first, strongest first.
        let mut g = f.clone();
        g.target.as_mut().unwrap().ranks = Some([-2, 0, 0, 0, 3, 0, 0, 0]);
        let s = suggest(&g);
        let n = s.iter().find(|x| x.label.as_deref() == Some("Fire")).expect("a fire family");
        assert_eq!((n.action, n.group.as_slice()), (Action::Spell(145), &[145, 144, 174][..]));
    }

    /// A nuke family is named for its element and lists single-target, then area, then ancient magic, strongest first;
    /// ancient magic is not the suggestion while a cheaper spell of the element is known.
    #[test]
    fn nuke_family_order() {
        let f = Facts {
            party: vec![me(1000, 1000)],
            spells: vec![named(206, "Freeze", 36, 1, 315, false), named(150, "Blizzard II", 36, 1, 31, false), named(149, "Blizzard", 36, 1, 8, false), named(180, "Blizzaga II", 36, 1, 175, true), named(179, "Blizzaga", 36, 1, 80, true)],
            target: Some(TargetFact { name: "Bark Spider".into(), hpp: 90, ranks: Some([0, -2, 0, 0, 0, 0, 0, 0]), ..Default::default() }),
            ..Default::default()
        };
        let s = suggest(&f);
        let n = s.iter().find(|x| x.label.as_deref() == Some("Ice")).expect("an ice family");
        assert_eq!(n.reason, "The Bark Spider is weak to ice.");
        assert_eq!((n.action, n.group.as_slice()), (Action::Spell(150), &[150, 149, 180, 179, 206][..]));
        // Only ancient magic known: it is the suggestion.
        let g = Facts { spells: vec![named(206, "Freeze", 36, 1, 315, false)], ..f.clone() };
        assert_eq!(suggest(&g).iter().find(|x| x.reason.contains("ice")).map(|x| x.action), Some(Action::Spell(206)));
    }

    /// Rows held under the cursor: a tier back from its recast returns to its family's list, and the nuke suggestion
    /// moves back to the list's top.
    #[test]
    fn held_rows_regroup() {
        let all = vec![named(150, "Blizzard II", 36, 1, 31, false), named(149, "Blizzard", 36, 1, 8, false), named(180, "Blizzaga II", 36, 1, 175, true), named(179, "Blizzaga", 36, 1, 80, true)];
        let target = Some(TargetFact { name: "Bark Spider".into(), hpp: 90, ranks: Some([0, -2, 0, 0, 0, 0, 0, 0]), ..Default::default() });
        // Built while Blizzard II and Blizzaga II recast.
        let f = Facts { party: vec![me(1000, 1000)], spells: vec![all[1].clone(), all[3].clone()], target, ..Default::default() };
        let mut s = suggest(&f);
        let k = s.iter().position(|x| x.label.as_deref() == Some("Ice")).expect("an ice family");
        assert_eq!((s[k].action, s[k].group.as_slice()), (Action::Spell(149), &[149, 179][..]));
        regroup(&Facts { spells: all, ..f }, &mut s);
        assert_eq!((s[k].action, s[k].group.as_slice()), (Action::Spell(150), &[150, 149, 180, 179][..]));
    }

    /// Protect's family takes Protectra; the weather breaks a tie between elements.
    #[test]
    fn families_and_the_weather() {
        let p = |id, name: &str, mp, area| SpellFact { id, name: name.into(), skill: 34, mp, targets: 1, area, ..Default::default() };
        let f = Facts { spells: vec![p(43, "Protect", 9, false), p(44, "Protect II", 28, false), p(125, "Protectra", 9, true)], ..Default::default() };
        assert_eq!(family(&f.spells[1]), "Protect");
        assert_eq!(family(&f.spells[2]), "Protect");
        let mut s = vec![Suggestion::new(Action::Spell(44), None, String::new())];
        add_groups(&f, &mut s);
        assert_eq!((s[0].label.as_deref(), s[0].group.as_slice()), (Some("Protect"), &[44, 43, 125][..]));
        let f = Facts {
            spells: vec![named(144, "Fire", 36, 0, 7, false), named(149, "Blizzard", 36, 1, 8, false)],
            target: Some(TargetFact { name: "Crab".into(), hpp: 20, ..Default::default() }),
            weather_element: Some(0),
            ..Default::default()
        };
        let s = suggest(&f);
        assert_eq!((s[0].action, s[0].reason.as_str()), (Action::Spell(144), "Today's weather strengthens fire."));
    }
}
