//! Windows as drawn this frame (name, frame, children), for the mouse on windows drawn at a runtime place (stacked on
//! the log, beside a row): the generic window handler's hit tests on what was really drawn.

use super::World;
use super::game_ui::{GameUi, LobbyWin};
use crate::audio::Ui;

/// Forgets last frame's windows (before the windows draw).
pub fn clear(w: &World) {
    w.win_hits.borrow_mut().clear();
}

pub fn record(w: &World, name: &str, win: &LobbyWin) {
    record_rects(w, name, win.frame, win.children.clone());
}

pub fn record_rects(w: &World, name: &str, frame: [f32; 4], children: Vec<(u16, [f32; 4])>) {
    w.win_hits.borrow_mut().push((name.to_string(), frame, children));
}

fn inside(r: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]
}

/// The child of a window drawn this frame under the pointer (`ids` only), and whether the pointer is on the window.
pub fn child_under(w: &World, name: &str, p: (f32, f32), ids: std::ops::RangeInclusive<u16>) -> (Option<u16>, bool) {
    let hits = w.win_hits.borrow();
    let Some((_, frame, children)) = hits.iter().find(|h| h.0 == name) else { return (None, false) };
    (children.iter().find(|(id, r)| ids.contains(id) && inside(*r, p)).map(|c| c.0), inside(*frame, p))
}

/// The pointer on a recorded window.
pub fn over(w: &World, name: &str, p: (f32, f32)) -> bool {
    child_under(w, name, p, 0..=0).1
}

/// The generic handler on the focused window: a child under the pointer takes the cursor (SE 0001), a click on the
/// cursor's child is Enter, a right click on the window Escape (ours only over it; retail's message 6 takes any right
/// click, FUN_1015def0). `key` is the window's key handler.
#[allow(clippy::too_many_arguments)]
pub fn generic(w: &mut World, gu: &GameUi, name: &str, cursor: u16, ids: std::ops::RangeInclusive<u16>, p: (f32, f32), moved: bool, clicked: bool, right: bool, set: impl FnOnce(&mut World, u16), key: impl FnOnce(&mut World, &GameUi, egui::Key)) {
    let (child, on) = child_under(w, name, p, ids);
    if right && on {
        key(w, gu, egui::Key::Escape);
        return;
    }
    let Some(id) = child else { return };
    gu.hover.set(true);
    if id != cursor && (moved || clicked) {
        set(w, id);
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// What a click on an itemctrl / moneyctr child re-sends (message 5, FUN_1013a6c0 / FUN_10140bc0's tables): 1 the
/// confirm (Enter), 2 message 4 (Left: all, or ten times the step), 3 message 3 (Right: 1, or a tenth of the step),
/// 4 message 1 (Down: one step less), 5 message 2 (Up: one step more). The windows' own keys do the rest.
pub fn ctr_key(child: u16) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match child {
        1 => K::Enter,
        2 => K::ArrowLeft,
        3 => K::ArrowRight,
        4 => K::ArrowDown,
        5 => K::ArrowUp,
        _ => return None,
    })
}

/// A held - / + repeats (retail's message 0x1d re-sends 1 / 2 for children 4 / 5; how often it comes is not traced,
/// ours: after 400 ms, every 60 ms).
const HOLD: std::time::Duration = std::time::Duration::from_millis(400);
const REPEAT: std::time::Duration = std::time::Duration::from_millis(60);

/// The mouse on itemctrl / moneyctr (`name`, as drawn this frame): a click on a child is its message (ctr_key), - / +
/// step on the press and repeat while held, a right click on the window is Escape. `key` is the window's key handler.
pub fn ctr(w: &mut World, gu: &GameUi, name: &str, p: (f32, f32), clicked: bool, right: bool, mut key: impl FnMut(&mut World, &GameUi, egui::Key)) {
    let (pressed, down) = w.ptr_button;
    let (child, on) = child_under(w, name, p, 1..=5);
    if !down || w.ctr_hold.is_some_and(|(c, _)| Some(c) != child) {
        w.ctr_hold = None;
    }
    if right && on {
        key(w, gu, egui::Key::Escape);
        return;
    }
    let Some(id) = child else { return };
    gu.hover.set(true);
    let Some(k) = ctr_key(id) else { return };
    if matches!(id, 4 | 5) {
        let now = std::time::Instant::now();
        if pressed {
            w.ctr_hold = Some((id, now + HOLD));
            key(w, gu, k);
        } else if w.ctr_hold.is_some_and(|(_, next)| now >= next) {
            w.ctr_hold = Some((id, now + REPEAT));
            key(w, gu, k);
        }
    } else if clicked {
        key(w, gu, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key as K;

    #[test]
    fn ctr_children_send_retails_messages() {
        let keys: Vec<_> = (0..=6).map(ctr_key).collect();
        assert_eq!(keys, [None, Some(K::Enter), Some(K::ArrowLeft), Some(K::ArrowRight), Some(K::ArrowDown), Some(K::ArrowUp), None]);
    }
}
