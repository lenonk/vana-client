//! Ours (the modern UI's action registry, plan D): one place that says, for a spell or an ability, who it may target,
//! how far it reaches, how far the current target is, and why it can't be used now. The checks are retail's
//! (magic::magic_check / ability_check, FUN_1022cf30 / FUN_10224b70, their group 7 lines) plus the effects the
//! modern UI already dims by (magic::blocked). The lists' cost box shows it.

use super::World;
use super::magic;

/// What the registry knows about one action.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// The kinds it may target, in the prompt's order ("Self", "Party", "Enemy" ...).
    pub targets: Vec<&'static str>,
    /// The spell's reach in yalms (record +0x45 through table 0x103644a8); abilities carry none in the DAT.
    pub range: Option<f32>,
    /// The current target's distance (flat, as the prompt's arrow measures it) when the action may take it.
    pub distance: Option<f32>,
    /// Why the action can't be used now: retail's refusal line, or the effect that stops it.
    pub why: Option<String>,
}

impl Info {
    /// The current target is a valid pick but beyond the range (the prompt's red arrow, past r + 0.5).
    pub fn out_of_range(&self) -> bool {
        matches!((self.distance, self.range), (Some(d), Some(r)) if d > r + 0.5)
    }
}

/// DAT valid-target bits (spell +6, ability +0xa) by the kinds target::SUB_FLAGS maps them to.
const TARGETS: [(u16, &str); 8] =
    [(1, "Self"), (4, "Party"), (8, "Alliance"), (0x10, "Player"), (2, "Ally NPC"), (0x40, "NPC"), (0x80, "NPC"), (0x20, "Enemy")];

fn target_names(bits: u16) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (b, name) in TARGETS {
        if bits & b != 0 && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The effect we carry that stops the action, as a reason (the ids magic::blocked tests).
fn effect_reason(w: &World, magic: bool) -> Option<&'static str> {
    w.buffs.iter().find_map(|&b| match b {
        2 | 19 | 193 => Some("Asleep."),
        7 => Some("Petrified."),
        10 => Some("Stunned."),
        14 | 17 => Some("Charmed."),
        28 => Some("Terrorized."),
        6 if magic => Some("Silenced."),
        29 if magic => Some("Muted."),
        16 if !magic => Some("Under amnesia."),
        _ => None,
    })
}

/// A refusal id's group 7 line, or a short line of ours when the DAT has none (a silent refusal).
fn line(w: &World, id: usize) -> String {
    w.battle.as_ref().and_then(|bt| bt.group7(id)).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).unwrap_or_else(|| "Can't be used now.".into())
}

/// The current target's flat distance when `bits` may take it (target::takes).
fn distance(w: &World, bits: u16) -> Option<f32> {
    let t = w.target.filter(|&t| t != w.my_id && super::target::takes(w, bits as u32, t))?;
    let p = w.entities.get(&t)?.pos;
    Some(glam::Vec2::new(p.x - w.pos.x, p.z - w.pos.z).length())
}

pub fn spell(w: &World, s: u16) -> Option<Info> {
    let r = w.actions.as_ref()?.spell(s)?.clone();
    let why = magic::magic_check(w, s).map(|id| line(w, id)).or_else(|| effect_reason(w, true).map(String::from));
    Some(Info {
        targets: target_names(r.targets),
        range: Some(crate::dat::actions::RANGES[(r.range & 0xf) as usize] as f32).filter(|&r| r < 255.0),
        distance: distance(w, r.targets),
        why,
    })
}

pub fn ability(w: &World, i: u16) -> Option<Info> {
    let a = w.actions.as_ref()?.abilities.get(i as usize)?.clone();
    if a.kind == 4 || a.sub != 0 {
        // Traits are always on; a sub-list row only opens its list.
        return Some(Info { targets: Vec::new(), ..Default::default() });
    }
    let why = magic::ability_check(w, i).map(|id| line(w, id)).or_else(|| effect_reason(w, false).map(String::from));
    Some(Info { targets: target_names(a.targets), range: None, distance: distance(w, a.targets), why })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_names_follow_the_prompt_kinds() {
        assert_eq!(target_names(0x20), ["Enemy"]);
        assert_eq!(target_names(1 | 4 | 8), ["Self", "Party", "Alliance"]);
        assert_eq!(target_names(0x40 | 0x80), ["NPC"]);
        assert!(target_names(0).is_empty());
    }

    #[test]
    fn out_of_range_past_half_a_yalm() {
        let i = |d, r| Info { distance: Some(d), range: Some(r), ..Default::default() };
        assert!(!i(20.4, 20.0).out_of_range());
        assert!(i(20.6, 20.0).out_of_range());
        assert!(!Info { distance: Some(50.0), range: None, ..Default::default() }.out_of_range());
    }
}
