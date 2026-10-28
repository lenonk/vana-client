//! "menu    charlnk": the full log's menu on a player's line (FUN_10138230 case 5 opens it, FUN_101389c0 its rows,
//! FUN_10138b50 the picks, FUN_10138e60 the name at its top), and the Assist Channel's c2s 0x0B7 (FUN_100e3650) that
//! its Warning / Mute / Thumbs Up and acoption's Remove send.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::client::net::NetCommand;

pub(super) const CHARLNK: &str = "menu    charlnk";

/// The rows top to bottom (the layout's links, wrapping): Tell, Add / Remove Request, Copy, Search, Target, Warning,
/// Thumbs Up, Mute.
pub const ORDER: [u16; 9] = [1, 4, 5, 9, 2, 3, 6, 8, 7];

/// c2s 0x0B7 kinds.
pub const THUMBS_UP: u8 = 0x24;
pub const WARNING: u8 = 0x25;
pub const MUTE: u8 = 0x26;
pub const UNMUTE: u8 = 0x27;

#[derive(Clone, Debug, PartialEq)]
pub struct CharLnk {
    /// The cursor's child.
    pub cursor: u16,
    /// The line's record: its name (+0x18), mode (+0) and flags byte 1 bit 0x20 (the assist rank icon).
    pub name: String,
    pub mode: u8,
    pub ranked: bool,
    /// fulllog +0x2c (our own line: rows 1..8 off, our name at the top) and +0x2d (a chat mode: Copy on).
    pub own: bool,
    pub chat: bool,
    /// +0x56: the text from the cursor's row to the line's end (Copy).
    pub text: String,
    /// The log line it was opened on.
    pub line: usize,
}

/// FUN_10137c70: +0x2c / +0x2d for a player's line in `mode` naming `name`.
fn sides(mode: u8, name: &str, me: &str) -> (bool, bool) {
    match mode {
        1..=7 | 0xd3 | 0xd5 | 0xdb | 0xdd => (true, true),
        9..=0xf | 0xd4 | 0xd6 | 0xdc | 0xde => (mode == 0xf && name == me, true),
        _ => (false, false),
    }
}

/// Enter in the full log (FUN_10138230 case 5): on a player's line charlnk opens over it on its first row; on any
/// other row the buzzer (FUN_10037c50).
pub fn open(w: &mut World, line: Option<(usize, String)>) {
    let rec = line.and_then(|(k, text)| {
        let (name, flags) = w.chat.who(k)?;
        let mode = super::combat::log_mode(w.chat.modes()[k])?;
        Some((k, name.to_string(), flags, mode, text))
    });
    let Some((k, name, flags, mode, text)) = rec else {
        w.sounds.push(Ui::Error as u32);
        return;
    };
    let (own, chat) = sides(mode, &name, &w.name);
    w.charlnk = Some(CharLnk { cursor: 1, name, mode, ranked: flags & 0x20 != 0, own, chat, text, line: k });
    w.sounds.push(Ui::Select as u32);
}

/// The name drawn at its top (+0x458): ours on our own line.
pub fn shown_name<'a>(w: &'a World, c: &'a CharLnk) -> &'a str {
    if c.own { &w.name } else { &c.name }
}

/// Whether we are a mentor (MentorFlg, s2c 0x0B4 byte 3 bit 1).
fn mentor(w: &World) -> bool {
    w.conf_sys[3] & 2 != 0
}

/// acoption's Remove and charlnk's Mute (FUN_1020cc90, FUN_101389c0): a mentor, a GM of rank 3 or more (0x037 flags
/// bits 29..31), or mastery rank 6 or more (s2c 0x01B @0x66).
pub fn can_mute(w: &World) -> bool {
    mentor(w) || w.self_flags >> 29 >= 3 || w.job_info.is_some_and(|j| j.mastery >= 6)
}

/// FUN_101389c0's disabled rows.
pub fn off(w: &World, c: &CharLnk) -> Vec<u16> {
    if c.own {
        return (1..=8).collect();
    }
    let mut off = Vec::new();
    if !c.chat {
        off.push(9);
    }
    if c.name.is_empty() || !matches!(c.mode, 0xdc | 0xde) {
        off.extend([6, 8, 7]);
        return off;
    }
    if !mentor(w) {
        off.push(6);
        if !can_mute(w) {
            off.push(7);
        }
    }
    // Thumbs Up and Warning only for a ranked speaker, and then only for a mentor or with 0x01B @0x64 at 1..3.
    let thumbs = w.job_info.is_some_and(|j| (1..=3).contains(&j.thumbs));
    if !(c.ranked && (mentor(w) || thumbs)) {
        off.extend([8, 6]);
    }
    off
}

/// The generic window's keys: Up / Down along the links, Enter picks (the buzzer on a disabled row), Escape closes.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(c) = w.charlnk.clone() else { return };
    let at = ORDER.iter().position(|&o| o == c.cursor).unwrap_or(0);
    let n = ORDER.len();
    match key {
        egui::Key::ArrowDown | egui::Key::ArrowUp => {
            let next = if key == egui::Key::ArrowDown { (at + 1) % n } else { (at + n - 1) % n };
            if let Some(s) = w.charlnk.as_mut() {
                s.cursor = ORDER[next];
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::Escape => {
            w.charlnk = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        egui::Key::Enter if off(w, &c).contains(&c.cursor) => w.sounds.push(Ui::Error as u32),
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            pick(w, &c);
        }
        _ => {}
    }
}

/// FUN_10138b50 (5, row): the rows act only with a name, but Copy.
fn pick(w: &mut World, c: &CharLnk) {
    let name = c.name.clone();
    if c.cursor != 9 && name.is_empty() {
        return;
    }
    let run = |w: &mut World, line: String| super::textcmd::run(w, &line, super::textcmd::Mode::Internal);
    match c.cursor {
        // "%s %s" with "/tell" into the chat line.
        1 => {
            (w.chat_input, w.chat_caret, w.chat_anchor) = (format!("/tell {name} "), None, None);
            w.typing = true;
        }
        // A name search in every area; the full log and charlnk close.
        2 => {
            w.full_log = None;
            w.charlnk = None;
            if !super::search::busy(w) {
                let cond = crate::net::search::Cond { last_area: -1, ..crate::net::search::Cond::name(name) };
                super::search::run(w, cond, super::search::Scope::World);
            }
        }
        3 => run(w, format!("/target {name}")),
        4 => run(w, format!("/partyrequestcmd add {name}")),
        5 => run(w, format!("/partyrequestcmd remove {name}")),
        // comyn (FUN_10210e30, default No): 0xe 0x417 / 0x419 / 0x418.
        6 | 7 | 8 => {
            w.assist_name = name;
            let kind = match c.cursor {
                6 => WARNING,
                7 => MUTE,
                _ => THUMBS_UP,
            };
            w.confirm = Some((super::Confirm::Assist(kind), false));
        }
        // FUN_1011dee0 -> FUN_10001d20: the text to the clipboard.
        9 => {
            let text = crate::dat::font::strip_codes(&c.text);
            log::info!("charlnk: copied {text:?}");
            w.copied = Some(text);
        }
        _ => {}
    }
}

/// The prompts' question.
pub fn ask_text(gu: &GameUi, kind: u8) -> String {
    let (id, fallback) = match kind {
        WARNING => (0x417, "Issue a warning?"),
        MUTE => (0x419, "Add to the Mute list?"),
        THUMBS_UP => (0x418, "Give a thumbs up?"),
        _ => (0x41a, "Remove from the Mute list?"),
    };
    gu.menu_msg(id, fallback)
}

/// A Yes (LAB_10138f50 / 70 / 90, LAB_1020cd60): c2s 0x0B7 {Kind, 0, the name (15), " "}; acoption's then closes the
/// search windows (scresult vtable +0x74 = FUN_1020c010).
pub fn answer(w: &mut World, kind: u8) {
    let name = std::mem::take(&mut w.assist_name);
    if !send(w, kind, &name) {
        return;
    }
    if kind == UNMUTE {
        w.search = None;
    }
}

/// FUN_100e3650: nothing for an empty name; a lowercase first letter goes up (English); the name cut below 16.
fn send(w: &World, kind: u8, name: &str) -> bool {
    let mut n: Vec<u8> = name.bytes().collect();
    if n.is_empty() || n.len() >= 0x10 {
        return false;
    }
    n[0] = n[0].to_ascii_uppercase();
    let mut body = vec![kind, 0];
    let mut field = [0u8; 15];
    field[..n.len().min(15)].copy_from_slice(&n[..n.len().min(15)]);
    body.extend_from_slice(&field);
    body.extend_from_slice(b" \0");
    log::info!("assist: c2s 0x0B7 kind {kind:#x} {name}");
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0B7, body });
    true
}

/// FUN_10138e60 and the window: the name fitted to 100 px at (+4, +6), its disabled rows dimmed.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let c = w.charlnk.as_ref()?;
    let off = off(w, c);
    let focused = w.confirm.is_none();
    let win = gu.lobby_window(CHARLNK, sw, sh, false, Some(c.cursor), focused && !off.contains(&c.cursor), &[])?;
    super::winhits::record(w, CHARLNK, &win);
    gu.disabled_children(CHARLNK, win.frame[0], win.frame[1], &off, Some(c.cursor));
    gu.shape_fit(shown_name(w, c), win.frame[0] + 4.0, win.frame[1] + 6.0, 100.0, 0x8080_8080);
    Some(win.frame)
}

/// The mouse (winhits.rs): the generic window.
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let Some(c) = w.charlnk.as_ref() else { return };
    super::winhits::generic(w, gu, CHARLNK, c.cursor, 1..=9, p, moved, clicked, right, |w, id| {
        if let Some(c) = w.charlnk.as_mut() {
            c.cursor = id;
        }
    }, |w, _, k| key(w, k));
}

/// Help slot 1 and the title: the row's own.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(c) = &w.charlnk else { return };
    let [help, title] = gu.focus_text(CHARLNK, c.cursor);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_sides() {
        assert_eq!(sides(1, "Me", "Me"), (true, true));
        assert_eq!(sides(0xdb, "Me", "Me"), (true, true));
        assert_eq!(sides(9, "Rabbit", "Me"), (false, true));
        assert_eq!(sides(0xf, "Me", "Me"), (true, true));
        assert_eq!(sides(0xc, "Me", "Me"), (false, true));
        assert_eq!(sides(0x79, "", "Me"), (false, false));
    }
}
