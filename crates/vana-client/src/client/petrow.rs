//! The pet and fellow rows by the party window (FUN_102041d0 -> FUN_10202960 / FUN_10202a40, notes
//! docs/re/notes/2026-11-03-modlog-gaps-batch5-build.md §5): no frame, floating left of the party window or above it,
//! 16 px apart, the fellow above the pet (or in its place). Each row: the name (fontshp, cut at 102 px), and with a
//! model the HP gauge (windowps 38 / 97, never eased); the pet's TP with /showtp.

use super::World;
use super::game_ui::GameUi;

/// One row's data.
struct Row {
    name: String,
    hpp: u8,
    model: bool,
    tp: Option<u16>,
}

/// Our pet (player +0x1fa, s2c 0x037 @0x34 >> 3) or fellow (player +0x2a0, 0x037 @0x48 / 0x067 type 2) by ActIndex.
fn row(w: &World, index: u16, pet: bool) -> Option<Row> {
    if index == 0 {
        return None;
    }
    let (id, e) = w.entities.iter().find(|(_, e)| e.index == index)?;
    let model = w.actors.get(id).is_some_and(|a| a.character.is_some());
    // FUN_10098320: no TP for the luopans (models 0xb22..0xb31); FUN_10087610: 0x068's TP only for this pet.
    let tp = (pet && w.party_tp && !(0xb22..=0xb31).contains(&e.model)).then(|| w.pet_sync.as_ref().filter(|p| p.index == index).map_or(0, |p| p.tp));
    Some(Row { name: super::monstrosity::name(e).to_string(), hpp: e.hpp, model, tp })
}

/// The fellow's server id and name, for the party-list cursor's row -1.
pub(super) fn fellow(w: &World) -> Option<(u32, String)> {
    if w.my_fellow == 0 {
        return None;
    }
    w.entities.iter().find(|(_, e)| e.index == w.my_fellow).map(|(id, e)| (*id, super::monstrosity::name(e).to_string()))
}

/// FUN_102041d0 (0x10204462..0x10204552): the rows' x and the first row's y from the party window (x, y, w, h), log
/// window 1's frame and log window 2's (when open), with config 0xb4 (`second_mode`).
pub(super) fn origin(party: [f32; 4], log: [f32; 4], log2: Option<[f32; 4]>, second_mode: u32) -> (f32, f32) {
    let (l, r, b) = (party[0], party[0] + party[2], party[1] + party[3]);
    // +0x3c: the stack column of the log (window 2's top when it is open and 0xb4 != 2).
    let col0 = match log2 {
        Some(l2) if second_mode != 2 => l2[1],
        _ => log[1],
    };
    let mut lr = log[0] + log[2];
    let (mut y0, mut extra) = (col0, 0.0);
    if let Some(l2) = log2 {
        y0 = l2[1];
        if second_mode == 2 {
            extra = l2[2];
        } else {
            lr = lr.max(l2[0] + l2[2]);
        }
    }
    let (e, a, wd) = (lr + extra, l - 8.0, (r - l) - 8.0);
    let (x, yb) = if a - e > wd {
        (a - wd, b)
    } else if a - lr > wd {
        (a.min(e) - wd, y0)
    } else {
        ((if a > lr { a.min(e) } else { lr }) - wd, col0.min(y0))
    };
    (x, yb - 0x17 as f32)
}

/// FUN_1015ed10(8, 0, 0) and FUN_10087070(3): an open window of registry group 8 (the lists, the config windows, the
/// map, the full log, the macro editor, ...) or a running event skip both rows.
fn skipped(w: &World) -> bool {
    w.config.is_some()
        // The item info window is group 8; the item windows (ours: w.win) are treated the same *(inf)*.
        || w.win.is_some()
        || super::item_info_source(w).is_some()
        || w.full_log.is_some()
        || w.macroedit.is_some()
        || w.map.is_some()
        || w.cnq.is_some()
        || w.magic.is_some()
        || w.ability.is_some()
        || w.quests.is_some()
        || w.missions.is_some()
        || super::event::active(w)
        || w.party_cursor.as_ref().is_some_and(|c| c.moving.is_some())
}

/// The rows at (x, y); the fellow row's place when it was drawn (the party-list hand goes there at row -1).
pub(super) fn draw(gu: &mut GameUi, w: &World, x: f32, y: f32) -> Option<(f32, f32)> {
    if skipped(w) {
        return None;
    }
    let mut y = y;
    if let Some(r) = row(w, w.my_pet, true) {
        piece(gu, &r, x, y, gu.ncol(1));
        y -= 16.0;
    }
    let r = row(w, w.my_fellow, false)?;
    piece(gu, &r, x, y, gu.ncol(4));
    Some((x, y))
}

fn piece(gu: &mut GameUi, r: &Row, x: f32, y: f32, tint: u32) {
    gu.shape_limited(&r.name, x, y, 102.0, tint);
    if !r.model {
        return;
    }
    // FUN_101cb090: the frame, then the fill scaled by HP% (stored as current first, so no ease trail).
    gu.sprite("menu    windowps", 38, x + 26.0, y + 7.0, if r.hpp == 0 { 0x3050_5050 } else { 0x8080_8080 });
    let f = r.hpp.min(100) as f32 * 0.01;
    if (f * 100.0 + 0.5) as i32 != 0 {
        gu.sprite_scaled("menu    windowps", 97, x + 30.0, y + 8.0, f, 1.0, 0x8080_8080);
    }
    // FUN_10202900: "%d" right-aligned, its right edge at x + 48, top y + 10; blue from 1000.
    if let Some(tp) = r.tp {
        gu.shape_text(&tp.to_string(), x + 48.0, y + 10.0, if tp >= 1000 { 0x8020_60d0 } else { 0x8060_6060 }, true);
    }
}

#[cfg(test)]
mod tests {
    use super::origin;

    /// Party window at the bottom right, log at the bottom left: the rows end 8 px left of the party window, from its
    /// bottom less 0x17.
    #[test]
    fn rows_left_of_the_party_window() {
        let (x, y) = origin([500.0, 400.0, 120.0, 100.0], [16.0, 380.0, 300.0, 120.0], None, 0);
        assert_eq!((x, y), (492.0 - 112.0, 500.0 - 23.0));
    }

    /// No room left of the party window: the rows go above the log.
    #[test]
    fn rows_above_the_log() {
        let (x, y) = origin([200.0, 400.0, 120.0, 100.0], [16.0, 380.0, 300.0, 120.0], None, 0);
        assert_eq!((x, y), (316.0 - 112.0, 380.0 - 23.0));
    }
}
