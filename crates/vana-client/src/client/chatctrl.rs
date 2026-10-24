//! "menu    chatctrl" (handler 0x1057818c, vtable 0x10336420; spec ~/ffxi/lab/research/chatctrl_spec.md): the chat mode
//! window the action menu's Right on Chat / Tell opens. Seven rows (Say, Tell, Party, Linkshell, Linkshell 2, Unity,
//! Shout); Enter sets the mode (FUN_101ad670), opens the chat line empty and closes; subwindo shows the range (mode 0).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const WINDOW: &str = "menu    chatctrl";
const SUBWINDO: &str = "menu    subwindo";

/// The open window: the cursor row, the tell selector (FUN_1012d1d0 / FUN_1012d210: -2 the current tell name, -1 the
/// target, i the sent-tell snapshot's name i, -3 none) and that snapshot (frozen while open).
#[derive(Clone, Debug)]
pub struct ChatCtrl {
    pub sel: u16,
    pub tell: i16,
    pub names: Vec<String>,
}

/// FUN_100e9730: linkshell n is equipped (its item still at its slot).
fn ls_on(w: &World, n: usize) -> bool {
    let (index, bag) = w.linkshells[n];
    index != 0 && w.inv.slots.get(&(bag, index)).is_some_and(|s| s.0 != 0)
}

/// FUN_100a3600: a Unity faction (s2c 0x061 @0x58 bits 0..4).
fn unity(w: &World) -> u32 {
    w.stats.as_ref().map_or(0, |s| s.unity & 0x1f)
}

/// FUN_1012ccd0's disabled rows: 4 / 5 without linkshell 1 / 2, 6 without a Unity.
fn disabled(w: &World) -> Vec<u16> {
    let mut off = Vec::new();
    if !ls_on(w, 1) {
        off.push(4);
    }
    if !ls_on(w, 2) {
        off.push(5);
    }
    if unity(w) == 0 {
        off.push(6);
    }
    off
}

fn target_name(w: &World) -> Option<String> {
    let t = w.target?;
    let name = if t == w.my_id { w.name.clone() } else { w.entities.get(&t)?.name.clone() };
    (!name.is_empty()).then_some(name)
}

/// The selector's values in cycling order: the current tell name, the target, the snapshot's names.
fn choices(w: &World, names: &[String]) -> Vec<i16> {
    let mut v = Vec::new();
    if w.tell_name.is_some() {
        v.push(-2);
    }
    if target_name(w).is_some() {
        v.push(-1);
    }
    v.extend((0..names.len() as i16).take(8));
    v
}

/// FUN_1012ccd0: the cursor on the current mode's row (jump table 0x1012cdf8), the first tell choice, the snapshot.
pub fn open(w: &mut World) {
    let sel = match w.chat_mode {
        3 | 9 => 7,
        4 => 2,
        5 => 3,
        6 => 4,
        0xb => 6,
        0xc => 5,
        _ => 1,
    };
    let names = w.tell_hist.clone();
    let tell = choices(w, &names).first().copied().unwrap_or(-3);
    w.chatctrl = Some(ChatCtrl { sel, tell, names });
}

/// FUN_1012d2b0 after the generic part: Up / Down along the links (7 rows, wrapping), Right on Tell cycles the name,
/// Left closes silently, Escape with SE 0003; Enter on a disabled row SE 0004 only, else SE 0002, the mode, the chat
/// line opened empty and the window closed (Tell with no name: SE 0004 too and no mode; the current name: no change).
pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut c) = w.chatctrl.clone() else { return };
    match key {
        egui::Key::ArrowDown | egui::Key::ArrowUp => {
            c.sel = if key == egui::Key::ArrowDown { c.sel % 7 + 1 } else { (c.sel + 5) % 7 + 1 };
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowRight if c.sel == 2 => {
            let v = choices(w, &c.names);
            c.tell = match v.iter().position(|&x| x == c.tell) {
                Some(i) => v[(i + 1) % v.len()],
                None => v.first().copied().unwrap_or(-3),
            };
        }
        egui::Key::ArrowLeft => {
            w.chatctrl = None;
            return;
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.chatctrl = None;
            return;
        }
        egui::Key::Enter if disabled(w).contains(&c.sel) => {
            w.sounds.push(Ui::Error as u32);
        }
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            match c.sel {
                1 => super::textcmd::set_chat_mode(w, 2, None),
                2 => match c.tell {
                    -3 => w.sounds.push(Ui::Error as u32),
                    -2 => {}
                    -1 => {
                        let n = target_name(w);
                        super::textcmd::set_chat_mode(w, 4, n.as_deref());
                    }
                    i => {
                        let n = c.names.get(i as usize).cloned();
                        super::textcmd::set_chat_mode(w, 4, n.as_deref());
                    }
                },
                3 => super::textcmd::set_chat_mode(w, 5, None),
                4 => super::textcmd::set_chat_mode(w, 6, None),
                5 => super::textcmd::set_chat_mode(w, 0xc, None),
                6 => super::textcmd::set_chat_mode(w, 0xb, None),
                _ => super::textcmd::set_chat_mode(w, 3, None),
            }
            w.chat_input.clear();
            w.typing = true;
            w.chatctrl = None;
            return;
        }
        _ => {}
    }
    w.chatctrl = Some(c);
}

/// The window's own header strings: help 0xe 0x253 "Set your default chat mode.", title 0xd 0x81 "Chat".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(c) = &w.chatctrl else { return };
    let [help, title] = gu.focus_text(WINDOW, c.sel);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

/// FUN_1012ce90: the tell name at (+100, +23), the linkshells (icon in the shell's colour at +0x4f, name at +0x5f) or
/// "No Linkshell" on rows 4 / 5, the Unity on row 6, all in fontshp; then subwindo in mode 0 (FUN_1014f960): windowps
/// 283 "Chat Range" and the cursor row's range under it.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(c) = &w.chatctrl else { return rects };
    let off = disabled(w);
    let focused = w.confirm.is_none();
    if let Some(win) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), Some(c.sel), focused, &off) {
        let [x, y, ..] = win.frame;
        gu.disabled_children(WINDOW, x, y, &off, Some(c.sel));
        let tell = match c.tell {
            -2 => w.tell_name.clone().map(|n| (n, 0x8050_9050)),
            -1 => target_name(w).map(|n| (n, 0x8090_9040)),
            i if i >= 0 => c.names.get(i as usize).map(|n| (n.clone(), 0x8080_8080)),
            _ => None,
        };
        if let Some((n, tint)) = tell {
            gu.shape_text(&n, x + 100.0, y + 23.0, tint, false);
        }
        for (n, dy) in [(1usize, 0.0), (2, 16.0)] {
            let (index, bag) = w.linkshells[n];
            let ext = w.inv.ext.get(&(bag, index)).filter(|_| ls_on(w, n));
            match (ext, w.inv.slots.get(&(bag, index))) {
                (Some(e), Some(&(item, _))) => {
                    let colour = u16::from_le_bytes([e[6], e[7]]) as u32;
                    let ch = |s: u32| (((colour >> s) & 0xf) * 13 + 0x38) as f32 / 128.0;
                    if let Some(info) = &w.item_info {
                        super::items::upload_icons(gu, info, std::iter::once(item));
                    }
                    let icon = crate::render::invwin::icon_texture(item);
                    if gu.has_image(&icon) {
                        gu.image_rect(&icon, x + 79.0, y + 52.0 + dy, 16.0, 16.0, [ch(0), ch(4), ch(8), 1.0]);
                    }
                    let mut packed = [0u8; 16];
                    packed[..15].copy_from_slice(&e[9..24]);
                    gu.shape_text(&super::linkshell::ls_name(&packed), x + 95.0, y + 54.0 + dy, 0x8080_6060, false);
                }
                _ => {
                    let none = gu.lobby_msg(0xc, 0x55, "");
                    gu.shape_text(&none, x + 79.0, y + 54.0 + dy, 0x8080_8080, false);
                }
            }
        }
        // FUN_100a3740: group 0xc 0x1a2 + the faction (0 "No Unity", 12 and up as 0); grey outside the Unity chat.
        let f = unity(w);
        let f = if f > 11 { 0 } else { f };
        let tint = match (f, w.stats.as_ref().is_some_and(|s| s.unity_flag & 1 != 0)) {
            (0, _) => 0x8080_3040,
            (_, true) => 0x8080_8080,
            _ => 0x8040_4040,
        };
        gu.shape_text(&gu.lobby_msg(0xc, 0x1a2 + f as usize, ""), x + 80.0, y + 86.0, tint, false);
        rects.push(win.frame);
    }
    // FUN_1012cc20: the range by row (table 0x10371b6c): Say Vicinity, Tell Target only, Party, Linkshell (2), Unity,
    // Shout Wide area.
    let range = match c.sel {
        1 => 284,
        2 | 8 => 285,
        3 => 286,
        4 => 287,
        5 => 709,
        6 => 710,
        _ => 289,
    };
    if gu.gate_layout(SUBWINDO, sw, sh, None)
        && let Some(m) = gu.stacked_menu(SUBWINDO, sw, sh)
    {
        let (b, ui, style, _) = gu.parts();
        let (x, y) = crate::render::screens::push_window(b, ui, style, &m, sw, sh);
        gu.sprite("menu    windowps", 283, x + 5.0, y + 8.0, 0x8080_8080);
        gu.sprite("menu    windowps", range, x + 15.0, y + 26.0, 0x8080_8080);
        rects.extend(gu.window_rect(SUBWINDO, sw, sh));
    }
    rects
}
