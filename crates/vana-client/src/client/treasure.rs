//! The treasure pool's log lines: s2c 0x0D2 (LAB_100ecb30, an item or gil found) and 0x0D3 (LAB_100ecf80, a lot,
//! a win, a loss). The pool window itself is not done; the pool keeps each slot's item and the actor it came from,
//! as G+0x19520 / +0x1956c, for 0x0D3's lines.

use super::World;
use crate::dat::battle::Ctx;

/// Slots in the pool (G+0x1951c, 0x58 bytes each).
pub const SLOTS: usize = 10;

/// 0x0D2: gil (Gold) prints 7031 0x13; an item takes its slot and, with a source (TargetUniqueNo; 0 for the pool
/// resent at zone-in), prints 0x10 "You find X on Y." or 0xda "... in the Y." (IsContainer); no item clears the slot.
pub(super) fn on_list(w: &mut World, slot: u8, item: u16, target: u32, gold: u16, container: bool) {
    if gold > 0 {
        let me = w.my_id;
        line(w, 0x13, Some(me), target, gold as i32, Default::default());
    }
    let Some(s) = w.treasure.get_mut(slot as usize) else { return };
    if item == 0 {
        *s = (0, 0);
        return;
    }
    *s = (item, target);
    if target != 0 {
        line(w, if container { 0xda } else { 0x10 }, None, target, item as i32, Default::default());
    }
}

/// 0x0D3 by JudgeFlg: 0 a lot (0x11 "X's lot for the Y: N points."), 1 won (that lot line, then 0x12 "X obtains
/// Y."), 2 won but refused (0x0f with the winner, 0x1f without), 3 lost (no line here). 1..3 free the slot.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_solution(w: &mut World, slot: u8, judge: u8, loot: u32, entry: u32, entry_point: i16, loot_name: String, entry_name: String) {
    let Some(&(item, from)) = w.treasure.get(slot as usize) else { return };
    // The lot line needs a lot (not -1 / 0) and the lotter's name (sLootName2).
    let lot_line = |w: &mut World| {
        if entry_point > 0 && !entry_name.is_empty() {
            line(w, if entry != 0 { 0x11 } else { 0x82 }, Some(entry), from, item as i32, [entry_point.to_string(), entry_name.clone()]);
        }
    };
    match judge {
        0 if item != 0 => lot_line(w),
        1 => {
            lot_line(w);
            if item != 0 {
                line(w, if loot != 0 { 0x12 } else { 0x83 }, Some(loot), from, item as i32, [loot_name, String::new()]);
            }
        }
        2 if item != 0 && loot != 0 => line(w, 0x0f, Some(loot), from, item as i32, [loot_name, String::new()]),
        2 if item != 0 => line(w, 0x1f, None, from, item as i32, Default::default()),
        _ => {}
    }
    if judge != 0
        && let Some(s) = w.treasure.get_mut(slot as usize)
    {
        *s = (0, 0);
    }
}

/// FUN_100a1ab0: a 7031 line with its actor, target, Para0 and string slots 2 / 3, in the message's mode colour.
fn line(w: &mut World, id: u16, actor: Option<u32>, target: u32, para0: i32, strings: [String; 2]) {
    let Some(bt) = w.battle.clone() else { return };
    let ctx = Ctx {
        actor: actor.and_then(|a| super::combat::who(w, a)),
        target: super::combat::who(w, target),
        me: w.name.clone(),
        work: [para0, 0, 0, 0, 0],
        strings,
    };
    match bt.system(id, &ctx) {
        Some(l) => {
            let colour = bt.colour_in(l.mode.unwrap_or(0), &w.game);
            for t in l.lines.into_iter().filter(|t| !t.is_empty()) {
                w.chat.push_colour(t, colour);
            }
        }
        None => log::info!("treasure line {id:#x} has no text"),
    }
}
