//! Monstrosity (main job MON): who is in it. Spec `~/ffxi/lab/research/monstrosity_spec.md`.

use super::World;

/// FUN_10097d50: the 0x00A flag (@0x7E) is set, or the main job (0x061) is MON.
pub fn active(w: &World) -> bool {
    w.mon_login || w.stats.as_ref().is_some_and(|s| s.main.0 == 23)
}
