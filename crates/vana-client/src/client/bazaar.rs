//! Main menu social row 5 Set Bazaar (spec ~/ffxi/lab/research/social_rows_spec.md part B): "menu    bazaar" (vtable
//! 0x10338e18: open FUN_101dcf80, messages FUN_101dcfa0) with Set Prices (the shop list's price source, shop.rs) and
//! Edit Comment (the comment editor in mode 0, seacom.rs); and the seller's lines (s2c 0x108 / 0x10A).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

const WIN: &str = "menu    bazaar";

pub struct Bazaar {
    pub sel: u16,
    /// menuwind's cursor, back when the window closes (socialme resumes).
    return_menu: Option<usize>,
}

/// The row opens the window over the main menu (SE 0002).
pub fn open(w: &mut World) {
    w.bazaar = Some(Bazaar { sel: 1, return_menu: w.menu });
    w.menu = None;
    w.sounds.push(Ui::Select as u32);
}

fn close(w: &mut World) {
    if let Some(b) = w.bazaar.take() {
        w.menu = b.return_menu;
    }
}

/// The window has the keys unless the price list or the editor is up over it.
pub fn focused(w: &World) -> bool {
    w.bazaar.is_some() && w.shop.is_none() && w.seacom_edit.is_none() && w.confirm.is_none()
}

/// The generic keys (links 1 <-> 2), Enter SE 0002 then: child 1 the price list (FUN_102107d0 on the price source),
/// child 2 the editor once our own message came (s2c 0x0CA MyFlag), else SE 0004 too; Escape SE 0003 and close.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(sel) = w.bazaar.as_ref().map(|b| b.sel) else { return };
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            if let Some(b) = w.bazaar.as_mut() {
                b.sel = 3 - sel;
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            if sel == 1 {
                super::shop::open_bazaar(w);
            } else if w.bazaar_msg.own_in {
                super::seacom::open_bazaar(w);
            } else {
                w.sounds.push(Ui::Error as u32);
            }
        }
        _ => {}
    }
}

/// Mouse: hover over a row moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    let Some(sel) = w.bazaar.as_ref().map(|b| b.sel) else { return };
    let Some((wx, wy)) = gu.window_xy(WIN, sw, sh) else { return };
    let Some(layout) = gu.layout(WIN) else { return };
    let hit = layout.elements.iter().skip(1).filter(|e| matches!(e.id, 1 | 2)).find(|e| {
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        p.0 >= x && p.0 < x + e.w as f32 && p.1 >= y && p.1 < y + e.h as f32
    });
    let Some(e) = hit else { return };
    gu.hover.set(true);
    if (moved || clicked) && e.id != sel
        && let Some(b) = w.bazaar.as_mut()
    {
        b.sel = e.id;
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter);
    }
}

/// The window as the modern page draws it.
pub(super) fn view(w: &World) -> Option<super::modern::MenuWin> {
    let b = w.bazaar.as_ref()?;
    Some(super::modern::MenuWin { name: WIN, sel: b.sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() })
}

/// The window, unfocused while the list or the editor is up (it stays where it is: group 0x81 shares nothing with them).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let b = w.bazaar.as_ref()?;
    gu.lobby_window_off(WIN, sw, sh, (0.0, 0.0), Some(b.sel), focused(w), &[]).map(|win| win.frame)
}

pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(b) = w.bazaar.as_ref().filter(|_| focused(w)) else { return };
    let [help, title] = gu.focus_text(WIN, b.sel);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

/// s2c 0x108 (LAB_101dcd80): State 0 group 10 0x0e "%s begins to browse ...", a customer more; else 0x0f "%s leaves your
/// bazaar.", one fewer (not below 0); printed with HideLevel 0 and the system message filter (0x0B4 word bits 11..12)
/// under 2, in mode 0x79.
pub fn on_shopper(w: &mut World, state: u32, hide: u16, name: String) {
    if state == 0 {
        w.bazaar_customers += 1;
    } else {
        w.bazaar_customers = (w.bazaar_customers - 1).max(0);
    }
    let filter = u32::from_le_bytes(w.conf_sys[0..4].try_into().unwrap()) >> 11 & 3;
    if hide != 0 || filter >= 2 {
        return;
    }
    let Some(bt) = w.battle.clone() else { return };
    let line = bt.group10(if state == 0 { 0x0e } else { 0x0f }).unwrap_or_default().replacen("%s", &name, 1);
    w.chat.push_colour(line, bt.colour_in(0x79, &w.game));
}

/// s2c 0x10A (LAB_100e0500): file 7031 line 0x5c with the item, the count and the buyer's name.
pub fn on_sale(w: &mut World, count: u32, item: u16, name: String) {
    super::post::system_line(w, 0x5c, [item as i32, count as i32, 0, 0, 0], name);
}
