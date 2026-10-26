//! The mouse on a list's scroll bar and the wheel (scroll_bar_spec 2): only the focused list's bar takes them.
//! FUN_102347f0 hit-tests the bar, FUN_10234930 sends list messages on the press and the key repeat, FUN_10234a20
//! drags the thumb, FUN_10125360 turns each wheel notch into 0x16 / 0x17 for the focus window.

use super::game_ui::GameUi;
use super::{UiInput, World};
use std::time::Instant;

/// The list messages: 0x16 / 0x17 a row up / down, 0x18 / 0x19 a page, 0x1b the thumb's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Msg {
    Up,
    Down,
    PageUp,
    PageDown,
    Set(usize),
}

/// Which list a bar scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Owner {
    Items,
    EquipPick,
    Bank,
    StoreInv,
    Actions,
    Shop,
    Query,
    FullLog,
    MapList,
    ScanList,
    Search,
    Config,
    /// The inventory list at x 200 of the post, hand-overs, player trade and synthesis.
    Picker,
    /// The mission, quest and key item lists.
    Journal,
    Linkshell,
    /// The Set Spells screen's focused list.
    BlueSet,
    /// The merit, job point and alter ego point lists, Unity's ranking.
    Points,
}

/// A focused list's bar as drawn this frame: x, y, height (8 wide), and its (total, visible, top).
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub owner: Owner,
    pub rect: [f32; 3],
    pub bar: (usize, usize, usize),
}

/// A press held on a bar: the hit zone, the press time and repeats sent; or the thumb's drag (start top, start y).
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    owner: Owner,
    hit: u8,
    since: Instant,
    sent: u32,
    drag: Option<(usize, f32)>,
}

/// FUN_10234680's thumb: (length, offset) in whole pixels.
fn thumb(h: f32, total: usize, visible: usize, top: usize) -> (i64, i64) {
    let (h, t, v) = (h as i64, total as i64, visible as i64);
    let len = ((h - 9) * v + t) / t;
    let off = (h - 8 - len) * (top.min(total - visible) as i64) / (t - v);
    (len, off)
}

/// FUN_102347f0: 1 top cap, 2 bottom cap, 4 above the thumb, 5 below it, 3 the thumb; 0 off the bar.
fn hit(s: &Site, p: (f32, f32)) -> u8 {
    let [x, y, h] = s.rect;
    let (total, visible, top) = s.bar;
    if total <= visible || p.0 < x || p.0 >= x + 8.0 || p.1 < y || p.1 >= y + h {
        return 0;
    }
    let (len, off) = thumb(h, total, visible, top);
    let my = p.1 as i64;
    let (y, h) = (y as i64, h as i64);
    if my <= y + 4 {
        1
    } else if my >= y + h - 4 {
        2
    } else if my < y + 4 + off {
        4
    } else if my > y + 4 + off + len {
        5
    } else {
        3
    }
}

fn zone_msg(hit: u8) -> Option<Msg> {
    match hit {
        1 => Some(Msg::Up),
        2 => Some(Msg::Down),
        4 => Some(Msg::PageUp),
        5 => Some(Msg::PageDown),
        _ => None,
    }
}

/// One frame for last frame's bars: the press, the key repeat (past 16 ticks, then every 2) and the drag, then the
/// wheel. Returns the input with the press taken off when the pointer was on a bar.
pub fn frame(gu: &mut GameUi, w: &mut World, input: &UiInput) -> Option<UiInput> {
    let sites = std::mem::take(&mut gu.bars);
    let held = gu.bar_hold.map(|h| h.owner);
    let mut msgs: Vec<(Owner, Msg)> = Vec::new();
    let on = input.pointer.and_then(|p| sites.iter().find_map(|s| Some((*s, hit(s, p))).filter(|h| h.1 != 0)));
    if input.pressed
        && let Some((s, h)) = on
    {
        let drag = (h == 3).then(|| (s.bar.2, input.pointer.map_or(0.0, |p| p.1)));
        gu.bar_hold = Some(Hold { owner: s.owner, hit: h, since: Instant::now(), sent: 0, drag });
        msgs.extend(zone_msg(h).map(|m| (s.owner, m)));
    } else if let Some(hold) = gu.bar_hold.as_mut()
        && input.down
    {
        let site = sites.iter().find(|s| s.owner == hold.owner);
        match (hold.drag, site, input.pointer) {
            (Some((start, sy)), Some(s), Some(p)) => {
                let (total, visible, top) = s.bar;
                let t = (start as f32 + (p.1 - sy) * total as f32 / (s.rect[2] - 8.0)).trunc().clamp(0.0, total.saturating_sub(visible) as f32) as usize;
                if t != top {
                    msgs.push((s.owner, Msg::Set(t)));
                }
            }
            // The repeat sends only while the pointer stays on the zone it pressed.
            (None, Some(s), Some(p)) if hit(s, p) == hold.hit => {
                let ticks = hold.since.elapsed().as_secs_f32() * 60.0;
                let due = if ticks > 16.0 { ((ticks - 16.0) / 2.0) as u32 + 1 } else { 0 };
                if due > hold.sent {
                    hold.sent = due;
                    msgs.extend(zone_msg(hold.hit).map(|m| (s.owner, m)));
                }
            }
            _ => {}
        }
    }
    if !input.down {
        gu.bar_hold = None;
    }
    if input.wheel != 0
        && let Some(s) = sites.first()
    {
        let m = if input.wheel > 0 { Msg::Up } else { Msg::Down };
        msgs.extend(std::iter::repeat_n((s.owner, m), input.wheel.unsigned_abs() as usize));
    }
    for (owner, m) in msgs {
        let bar = sites.iter().find(|s| s.owner == owner).map_or((0, 0, 0), |s| s.bar);
        if dispatch(w, owner, m, bar) {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    // The bar holds the mouse: the windows under it see neither the press nor the pointer (no hover moving the hand
    // during a drag).
    let grabbed = on.is_some() || gu.bar_hold.is_some() || (input.released && sites.iter().any(|s| Some(s.owner) == held));
    grabbed.then(|| UiInput { pointer: None, moved: false, pressed: false, clicked: false, released: false, events: input.events.clone(), ..*input })
}

/// A list message to its owner, with the bar's (total, visible, top); true when the list moved (SE 0001).
fn dispatch(w: &mut World, owner: Owner, m: Msg, bar: (usize, usize, usize)) -> bool {
    match owner {
        Owner::Items | Owner::EquipPick | Owner::Bank | Owner::StoreInv => super::items::bar(w, owner, m),
        Owner::Actions => super::magic::bar(w, m),
        Owner::Shop => super::shop::bar(w, m),
        Owner::Query => super::event::bar(w, m),
        Owner::FullLog => super::bar_full_log(w, m, bar),
        Owner::MapList | Owner::ScanList => w.map.as_mut().is_some_and(|map| map.bar(owner, m)),
        Owner::Search => super::search::bar(w, m),
        Owner::Picker => super::post::picker_bar(w, m),
        Owner::Journal => super::missions::bar(w, m) || super::quests::bar(w, m) || super::roe::bar(w, m) || super::keyitems::bar(w, m) || super::currencies::bar(w, m),
        Owner::Linkshell => super::linkshell::bar(w, m),
        Owner::BlueSet => super::bluset::bar(w, m),
        Owner::Points => super::merits::bar(w, m) || super::jobpoints::bar(w, m) || super::aepoints::bar(w, m) || super::unity::bar(w, m),
        Owner::Config => match &mut w.config {
            Some(c) => super::configui2::bar(c, &w.game, m),
            None => false,
        },
    }
}

/// FUN_101f6c70 for a list kept as (top, the cursor's screen row): a row moves the top and the hand stays on its row;
/// a page moves the top by `vis`, or at the end moves the hand to the first / last row; 0x1b sets the top. True when
/// something moved.
pub fn rows(m: Msg, top: &mut usize, row: &mut usize, n: usize, vis: usize) -> bool {
    if n == 0 {
        return false;
    }
    let max = n.saturating_sub(vis);
    let before = (*top, *row);
    match m {
        Msg::Up => *top = top.saturating_sub(1),
        Msg::Down => *top = (*top + 1).min(max),
        Msg::PageUp if *top == 0 => *row = 0,
        Msg::PageUp => *top = top.saturating_sub(vis),
        Msg::PageDown if *top >= max => *row = (n - *top).min(vis) - 1,
        Msg::PageDown => *top = (*top + vis).min(max),
        Msg::Set(t) => *top = t.min(max),
    }
    // The shared tail clamps the hand to the count.
    *row = (*row).min(n - 1 - *top);
    (*top, *row) != before
}

/// `rows` for a list kept as (top, absolute cursor).
pub fn rows_abs(m: Msg, top: &mut usize, cursor: &mut usize, n: usize, vis: usize) -> bool {
    let mut row = cursor.saturating_sub(*top);
    let moved = rows(m, top, &mut row, n, vis);
    *cursor = *top + row;
    moved
}

/// FUN_10222150 (the paged lists: magic, ability, emote, mount) as (top, screen row): a row moves the top and the
/// cursor together; 0x18 goes back by min(top, vis) (the cursor to 0 at the top), 0x19 moves the cursor a page on (to
/// the last at most) and the top with it.
pub fn paged(m: Msg, top: &mut usize, row: &mut usize, n: usize, vis: usize) -> bool {
    if n == 0 {
        return false;
    }
    let max = n.saturating_sub(vis);
    let before = (*top, *row);
    let mut at = *top + *row;
    match m {
        Msg::Up if *top > 0 => (*top, at) = (*top - 1, at - 1),
        Msg::Down if *top < max => (*top, at) = (*top + 1, at + 1),
        Msg::Up | Msg::Down => {}
        Msg::PageUp if *top == 0 => at = 0,
        Msg::PageUp => {
            let d = (*top).min(vis);
            (*top, at) = (*top - d, at - d);
        }
        Msg::PageDown if max == 0 => at = n - 1,
        Msg::PageDown => {
            at = (at + vis).min(n - 1);
            *top = (*top + vis).min(max);
        }
        Msg::Set(t) => {
            let t = t.min(max);
            at = (at + t).saturating_sub(*top).min(n - 1);
            *top = t;
        }
    }
    if at < *top {
        *top = at;
    } else if at >= *top + vis {
        *top = at + 1 - vis;
    }
    *row = at - *top;
    (*top, *row) != before
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FUN_101f6c70: rows move the top under the hand, pages stop at the ends with the hand on the first / last row.
    #[test]
    fn row_list() {
        let (mut top, mut row) = (0, 3);
        assert!(!rows(Msg::Up, &mut top, &mut row, 25, 10));
        assert!(rows(Msg::Down, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (1, 3));
        assert!(rows(Msg::PageDown, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (11, 3));
        assert!(rows(Msg::PageDown, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (15, 3));
        assert!(rows(Msg::PageDown, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (15, 9));
        assert!(rows(Msg::Set(4), &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (4, 9));
        assert!(rows(Msg::PageUp, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (0, 9));
        assert!(rows(Msg::PageUp, &mut top, &mut row, 25, 10));
        assert_eq!((top, row), (0, 0));
    }

    /// FUN_10222150: 0x19 moves the cursor a whole page even when the top stops short.
    #[test]
    fn paged_list() {
        let (mut top, mut row) = (10, 5);
        assert!(paged(Msg::PageDown, &mut top, &mut row, 30, 12));
        assert_eq!((top, top + row), (18, 27));
        assert!(paged(Msg::PageUp, &mut top, &mut row, 30, 12));
        assert_eq!((top, top + row), (6, 15));
        let (mut top, mut row) = (0, 4);
        assert!(paged(Msg::PageUp, &mut top, &mut row, 30, 12));
        assert_eq!((top, row), (0, 0));
    }

    /// The thumb and the zones (FUN_102347f0) for a 190 px bar over 25 rows, 10 shown.
    #[test]
    fn hit_zones() {
        let s = Site { owner: Owner::Items, rect: [100.0, 50.0, 190.0], bar: (25, 10, 0) };
        let (len, off) = thumb(190.0, 25, 10, 0);
        assert_eq!((len, off), (73, 0));
        assert_eq!(hit(&s, (103.0, 52.0)), 1);
        assert_eq!(hit(&s, (103.0, 238.0)), 2);
        assert_eq!(hit(&s, (103.0, 100.0)), 3);
        assert_eq!(hit(&s, (103.0, 150.0)), 5);
        assert_eq!(hit(&s, (99.0, 100.0)), 0);
    }
}
