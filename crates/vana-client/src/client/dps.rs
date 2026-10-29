//! Ours (not retail; the DPS-meter add-ons): `/dps` prints the damage our party dealt since the first hit after a reset
//! (each member's total, share and damage a second over the fight's span), `/dps reset` starts again. The tally takes
//! every battle message that puts a damage number up (main and proc records) by its actor: us, the party (trusts
//! included) and their pets.

use super::World;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
pub struct Dps {
    /// The first and the last counted hit.
    span: Option<(Instant, Instant)>,
    /// Total damage and hits by actor.
    by: HashMap<u32, (u64, u32)>,
}

/// Us, a party member, or the pet of either (its master's ActIndex).
pub(super) fn ours(w: &World, actor: u32) -> bool {
    if actor == w.my_id || w.party.party().any(|m| m.id == actor) {
        return true;
    }
    let Some(master) = w.entities.get(&actor).map(|e| e.master).filter(|&m| m != 0) else { return false };
    master == w.my_index || w.party.party().any(|m| w.entities.get(&m.id).is_some_and(|e| e.index == master))
}

pub fn add(w: &mut World, actor: u32, value: i32) {
    if value <= 0 || !ours(w, actor) {
        return;
    }
    let now = Instant::now();
    w.dps.span = Some((w.dps.span.map_or(now, |s| s.0), now));
    let e = w.dps.by.entry(actor).or_default();
    e.0 += value as u64;
    e.1 += 1;
}

fn name(w: &World, id: u32) -> String {
    if id == w.my_id { w.name.clone() } else { w.entities.get(&id).map_or_else(|| format!("{id:#x}"), |e| e.name.clone()) }
}

/// The lines `/dps` prints: a header with the span, then each actor by damage.
fn report(w: &World) -> Vec<String> {
    let Some((start, last)) = w.dps.span else { return vec!["DPS: no damage yet.".into()] };
    let secs = last.duration_since(start).as_secs_f32().max(1.0);
    let total: u64 = w.dps.by.values().map(|v| v.0).sum();
    let mut rows: Vec<(u32, u64, u32)> = w.dps.by.iter().map(|(id, v)| (*id, v.0, v.1)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = vec![format!("DPS over {}:{:02}: {total} damage", secs as u32 / 60, secs as u32 % 60)];
    out.extend(rows.into_iter().map(|(id, dmg, hits)| format!("  {}: {dmg} ({:.0}%), {:.1}/s, {hits} hits", name(w, id), dmg as f32 * 100.0 / total.max(1) as f32, dmg as f32 / secs)));
    out
}

/// `/dps [reset]`.
pub fn command(w: &mut World, arg: Option<&str>) {
    let colour = w.battle.as_ref().map_or([0x80; 4], |bt| bt.colour_in(0x8b, &w.game));
    if arg.is_some_and(|a| a.eq_ignore_ascii_case("reset")) {
        w.dps = Dps::default();
        w.chat.push_colour("DPS: reset.".into(), colour);
        return;
    }
    for line in report(w) {
        w.chat.push_colour(line, colour);
    }
}
