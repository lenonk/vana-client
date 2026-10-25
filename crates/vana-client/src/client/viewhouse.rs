//! Main menu social row 4 View House (FUN_1014e7b0 case 4; spec ~/ffxi/lab/research/social_rows_spec.md part V):
//! "menu    mogcont" (vtable 0x10336588, open FUN_1014eaf0, messages FUN_1014ec40), whose rows open the storage views
//! through FUN_101dae10 / FUN_101daf70 with nomad 0, anywhere.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const WIN: &str = "menu    mogcont";

pub struct ViewHouse {
    pub sel: u16,
    off: Vec<u16>,
    /// menuwind's cursor, back when mogcont closes (socialme resumes).
    return_menu: Option<usize>,
}

/// The container of a child: 1 Mog Safe, 8 Safe 2, 2 Storage, 3 Locker, 4 Satchel, 5 Sack, 6 Case, 7 Wardrobe,
/// 9..15 Wardrobe 2..8.
fn container(child: u16) -> Option<u8> {
    Some(match child {
        1 => 1,
        8 => 9,
        2 => 2,
        3 => 4,
        4 => 5,
        5 => 6,
        6 => 7,
        7 => 8,
        9..=15 => child as u8 + 1,
        _ => return None,
    })
}

/// FUN_1014eaf0: the Satchel under 30, the Sack with no size, Safe 2 without the 2F flag, Wardrobes 2..8 with no
/// size; the Mog Safe is never disabled here.
fn disabled(w: &World) -> Vec<u16> {
    let n = |c: usize| w.inv.sizes[c];
    let mut off = Vec::new();
    if n(5) < 0x1e {
        off.push(4);
    }
    if n(6) == 0 {
        off.push(5);
    }
    if !w.mh_floors {
        off.push(8);
    }
    off.extend((10..=16).filter(|&c| n(c) == 0).map(|c| c as u16 - 1));
    off
}

/// FUN_100f9680: in the nations' residential zones, whether our s2c 0x067 kind 2 has come (true elsewhere).
fn owner_known(w: &World) -> bool {
    !w.zone.is_some_and(|z| matches!(z, 230 | 231 | 232 | 234 | 235 | 236 | 238 | 239 | 240 | 241)) || w.mh_seen
}

/// The row: in a room, before the owner info SE 0004 + 0002 only, visiting another's Mog House group 7 0xbc; else
/// mogcont opens over the main menu, SE 0002.
pub fn open(w: &mut World) {
    if w.room_flag {
        if !owner_known(w) {
            w.sounds.extend([Ui::Error as u32, Ui::Select as u32]);
            return;
        }
        if w.mh_owner != 0 {
            super::textcmd::refuse(w, 0xbc);
            w.sounds.extend([Ui::Error as u32, Ui::Select as u32]);
            return;
        }
    }
    w.viewhouse = Some(ViewHouse { sel: 1, off: disabled(w), return_menu: w.menu });
    w.menu = None;
    w.sounds.push(Ui::Select as u32);
}

fn close(w: &mut World) {
    if let Some(v) = w.viewhouse.take() {
        w.menu = v.return_menu;
    }
}

/// The generic window keys (FUN_10118260): Up / Down along the links (they wrap), Enter on a disabled row SE 0004,
/// else SE 0002 and the row's opener (its result not looked at); Escape SE 0003 and socialme comes back.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(v) = w.viewhouse.as_ref() else { return };
    let sel = v.sel;
    match key {
        K::ArrowUp | K::ArrowDown => {
            let k = if key == K::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(WIN, sel).map_or(sel, |l| l[k] as u16);
            if to != sel && to != 0 && to != 0xff
                && let Some(v) = w.viewhouse.as_mut()
            {
                v.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Enter if v.off.contains(&sel) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            let Some(c) = container(sel) else { return };
            w.sounds.push(Ui::Select as u32);
            if matches!(c, 1 | 2 | 4 | 9) {
                super::items::open_dae10(w, c, false);
            } else {
                super::items::open_daf70(w, c, c == 5, false);
            }
        }
        _ => {}
    }
}

/// mogcont keeps the keys unless a storage view is up over it.
pub fn focused(w: &World) -> bool {
    w.viewhouse.is_some() && w.win.is_none() && w.confirm.is_none()
}

/// A storage view up over mogcont: the two-list view ("menu    bank" with itmsortw, group 0x01) suspends it; the
/// read-only list ("menu    inventor", group 0) leaves it drawn, unfocused *(inf: the registry groups)*.
fn suspended(w: &World) -> bool {
    match &w.win {
        Some(super::items::Win::Items { store: Some(st), .. }) => !st.read_only,
        Some(_) => true,
        None => false,
    }
}

/// Mouse: hover over a row moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(sel) = w.viewhouse.as_ref().map(|v| v.sel) else { return };
    let Some((wx, wy)) = gu.window_xy(WIN, sw, sh) else { return };
    let Some(layout) = gu.layout(WIN) else { return };
    let hit = layout.elements.iter().skip(1).find(|e| {
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
    });
    let Some(e) = hit else { return };
    gu.hover.set(true);
    if (moved || clicked) && e.id != sel
        && let Some(v) = w.viewhouse.as_mut()
    {
        v.sel = e.id;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, gu, egui::Key::Enter);
    }
}

/// The window with its disabled rows' dim sprites (ref 4).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let v = w.viewhouse.as_ref().filter(|_| !suspended(w))?;
    let focused = focused(w);
    let win = gu.lobby_window_off(WIN, sw, sh, (0.0, 0.0), Some(v.sel), focused, &v.off)?;
    gu.disabled_children(WIN, win.frame[0], win.frame[1], &v.off, focused.then_some(v.sel));
    Some(win.frame)
}

/// Help slot 1 the row's (group 0xe), title 0xd 0x6b "View House" (window strings) while focused.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(v) = w.viewhouse.as_ref().filter(|_| focused(w)) else { return };
    let [help, title] = gu.focus_text(WIN, v.sel);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers() {
        let got: Vec<u8> = [1, 8, 2, 3, 4, 5, 6, 7, 9, 15].iter().filter_map(|&c| container(c)).collect();
        assert_eq!(got, [1, 9, 2, 4, 5, 6, 7, 8, 10, 16]);
    }
}
