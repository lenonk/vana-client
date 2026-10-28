//! PlayOnline messages (research friend_list_spec.md 1.8, 2.6): msgline the composer, msglist the unread messages,
//! mes1rcv / mes2frnd the actions on a read one. The messages and the jobs are client/friends.rs's.

use super::World;
use super::friends::{self, Op};
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::net::profile;

pub(super) const LIST: &str = "menu    msglist";
pub(super) const RCV: &str = "menu    mes1rcv";
pub(super) const FRND: &str = "menu    mes2frnd";
const LINE: &str = "menu    msgline";
const ROWS: usize = 15;
const WHITE: u32 = 0x8080_8080;
/// msglist's columns (table 0x10385174).
const COL: [f32; 6] = [0.0, 3.0, 78.0, 158.0, 215.0, 300.0];

/// What the composer is for (msgline's modes).
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// 0: a plain message to a searched character's account.
    Plain { to: u64 },
    /// 1: a plain message to a friend.
    Friend { account: u64 },
    /// 2: a friend request: the box is our list name for them (15), prefilled with their character's.
    Request { to: u64, char_name: String },
    /// 5: a reply.
    Reply { to: u64, name: String },
    /// 7: an accept: the list name for the requester (15).
    Accept { to: u64, name: String },
}

impl Mode {
    /// Modes 2 and 7: box kind 7, 15 characters, "Friend's Name:" before it; the others 300.
    fn naming(&self) -> bool {
        matches!(self, Mode::Request { .. } | Mode::Accept { .. })
    }
}

#[derive(Clone, Debug)]
pub struct Line {
    mode: Mode,
    text: String,
    /// The message the line answers (marked read once sent: Accept).
    about: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PolMail {
    pub line: Option<Line>,
    /// msglist: its rows (message names; row 0 the header), top, cursor, and the sender it is limited to.
    pub list: Option<(Vec<String>, usize, usize, Option<u64>)>,
    /// The read message's name, mes2frnd (a friend request) or mes1rcv, and the cursor.
    pub read: Option<(String, bool, u16)>,
    /// The new-message count last seen (netstat's SE 0039).
    seen_new: u32,
}

/// Any opener (FUN_101ffb60 ...): one PlayOnline operation at a time, else 7 0x7a.
pub fn compose(w: &mut World, mode: Mode, prefill: String) -> bool {
    if w.friends.busy || w.polmail.line.is_some() {
        super::textcmd::refuse(w, 0x7a);
        return false;
    }
    let text = if mode.naming() { prefill.chars().take(15).collect() } else { prefill };
    w.polmail.line = Some(Line { mode, text, about: None });
    true
}

pub fn composing(w: &World) -> bool {
    w.polmail.line.is_some()
}

/// msgline's keys: text (15 or 300), Backspace; Enter commits (empty: just closes), Escape cancels.
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    let Some(l) = w.polmail.line.as_mut() else { return };
    let max = if l.mode.naming() { 15 } else { 300 };
    let mut done = None;
    for ev in events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.chars().filter(|c| !c.is_control()) {
                    if l.text.chars().count() < max {
                        l.text.push(c);
                    }
                }
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                l.text.pop();
            }
            egui::Event::Key { key: k @ (egui::Key::Enter | egui::Key::Escape), pressed: true, .. } => done = Some(*k == egui::Key::Enter),
            _ => {}
        }
    }
    let Some(commit) = done else { return };
    let Some(l) = w.polmail.line.take() else { return };
    w.sounds.push(if commit { Ui::Select } else { Ui::Cancel } as u32);
    if !commit || l.text.is_empty() {
        return;
    }
    let text = crate::dat::font::encode_sjis(&l.text);
    let op = match l.mode {
        Mode::Plain { to } | Mode::Friend { account: to } => Op::Plain { to },
        Mode::Request { to, char_name } => Op::Request { to, list_name: l.text.clone(), char_name },
        Mode::Reply { to, .. } => Op::Reply { to },
        Mode::Accept { to, .. } => Op::Accept { to, list_name: l.text.clone() },
    };
    friends::send(w, op, text);
    if let Some(name) = l.about {
        friends::mark_read(w, &name);
        refresh(w);
    }
}

/// FUN_10200be0: msglist on everyone's unread messages, or one sender's; busy -> false (the caller's 7 0x7a).
pub fn open_list(w: &mut World, from: Option<u64>) -> bool {
    if w.friends.busy {
        super::textcmd::refuse(w, 0x7a);
        return false;
    }
    w.friends.mail_new = 0;
    w.polmail.list = Some((Vec::new(), 0, 0, from));
    refresh(w);
    true
}

pub fn list_up(w: &World) -> bool {
    w.polmail.list.is_some()
}

/// The rows: the header, then the unread messages oldest first (by Date).
fn refresh(w: &mut World) {
    let Some((_, top, cursor, from)) = w.polmail.list.clone() else { return };
    let mut rows = vec![String::new()];
    rows.extend(w.friends.mail.iter().filter(|m| m.unread && from.is_none_or(|f| m.header.from == f)).map(|m| m.name.clone()));
    let n = rows.len();
    w.polmail.list = Some((rows, top.min(n.saturating_sub(ROWS)), cursor.min(n - 1), from));
}

/// Each frame: new arrivals rebuild an open list (FUN_10200d30); a grown count plays SE 0039 (config 0x48).
pub fn update(w: &mut World) {
    let n = w.friends.mail_new;
    if n > w.polmail.seen_new && w.game.get(0x48) != 0 {
        w.sounds.push(39);
    }
    if n != w.polmail.seen_new {
        w.polmail.seen_new = n;
        refresh(w);
    }
}

/// The modern layer's mouse: msglist's cursor onto row `k` (the header 0), or the read window's onto child `k`.
pub(super) fn set_cursor(w: &mut World, k: usize) -> bool {
    if let Some(r) = w.polmail.read.as_mut() {
        let old = std::mem::replace(&mut r.2, k as u16);
        return old != k as u16;
    }
    let Some(l) = w.polmail.list.as_mut().filter(|l| k < l.0.len()) else { return false };
    let old = std::mem::replace(&mut l.2, k);
    l.1 = l.1.min(k).max((k + 1).saturating_sub(ROWS));
    old != k
}

/// msgline's text and whether it is a name box ("Friend's Name:").
pub(super) fn line_view(w: &World) -> Option<(&str, bool)> {
    w.polmail.line.as_ref().map(|l| (l.text.as_str(), l.mode.naming()))
}

pub(super) fn mail<'a>(w: &'a World, name: &str) -> Option<&'a friends::Mail> {
    w.friends.mail.iter().find(|m| m.name == name)
}

/// The type label (FUN_10200aa0, table 0x10385180).
pub(super) fn type_label(kind: u8) -> &'static str {
    match kind {
        0 => "[NRM]",
        1 => "[FWT]",
        3 => "[KNK]",
        9 => "[FOK]",
        10 => "[FNO]",
        0xe..=0x13 => "[GRP]",
        0x1e => "[SYS]",
        _ => "[OTR]",
    }
}

/// From: the name we list them under, else the header's sender.
pub(super) fn from_name(w: &World, m: &friends::Mail) -> String {
    w.friends.list.iter().find(|f| f.account == m.header.from).map_or_else(|| m.header.sender.clone(), |f| f.name.clone())
}

/// Read (LAB_10200de0): [OTR] -> 7 0x60; else From / Title / Message to the log in mode 0x12 (the built-in title
/// alone for the friend kinds), then mes2frnd for a friend request, mes1rcv for the rest.
fn read(w: &mut World, name: &str) {
    let Some(m) = mail(w, name).cloned() else { return };
    if type_label(m.header.kind) == "[OTR]" {
        super::textcmd::refuse(w, 0x60);
        return;
    }
    let (title, text) = profile::decode_body(&m.data);
    let dec = crate::dat::font::decode_sjis;
    let mut lines = vec![format!("From: {}", from_name(w, &m))];
    match m.header.kind {
        0 | 3 | 0x1e | 0x11 => {
            lines.push(format!("Title: {}", dec(&title)));
            lines.push(format!("Message: {}", dec(&text)));
        }
        _ => lines.push(format!("Title: {}", dec(&title))),
    }
    let colour = w.battle.as_ref().map_or(super::WHITE, |b| b.colour_in(0x12, &w.game));
    for l in lines {
        w.chat.push_event(l, colour, false, 0x12);
    }
    w.polmail.read = Some((name.to_string(), m.header.kind == profile::FRIEND_REQUEST, 1));
}

/// mes1rcv's (FUN_10200370) or mes2frnd's (FUN_10200550) disabled rows.
pub fn read_off(w: &World) -> Vec<u16> {
    let Some((name, frnd, _)) = &w.polmail.read else { return Vec::new() };
    let Some(m) = mail(w, name) else { return vec![1, 2, 3, 4] };
    let ignore_off = friends::room(w, true, m.header.from) < 0;
    if *frnd {
        return if ignore_off { vec![3] } else { Vec::new() };
    }
    let mut off = Vec::new();
    match m.header.kind {
        9 | 10 => off.push(1),
        3 | 0x1e => off.extend([1, 2]),
        k if k != 0 => off.extend([1, 2]),
        _ => {}
    }
    if m.header.no_reply {
        off.extend([1, 2]);
    }
    if ignore_off {
        off.push(2);
    }
    off
}

pub fn focused(w: &World) -> bool {
    (w.polmail.list.is_some() || w.polmail.read.is_some()) && w.confirm.is_none() && w.polmail.line.is_none()
}

pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    if let Some((name, frnd, c)) = w.polmail.read.clone() {
        match key {
            K::ArrowUp | K::ArrowDown => {
                let c = if key == K::ArrowDown { c % 4 + 1 } else { (c + 2) % 4 + 1 };
                w.polmail.read = Some((name, frnd, c));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                w.polmail.read = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter if read_off(w).contains(&c) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                read_pick(w, &name, frnd, c);
            }
            _ => {}
        }
        return;
    }
    let Some((rows, top, cursor, from)) = w.polmail.list.clone() else { return };
    let n = rows.len();
    let (last, max_top) = (n.saturating_sub(1), n.saturating_sub(ROWS));
    let moved = match key {
        K::ArrowDown if cursor < last => Some((top.max((cursor + 2).saturating_sub(ROWS)), cursor + 1)),
        K::ArrowUp if cursor > 0 => Some((top.min(cursor - 1), cursor - 1)),
        K::ArrowRight if top < max_top => {
            let t = (top + ROWS).min(max_top);
            Some((t, cursor + t - top))
        }
        K::ArrowLeft if top > 0 => {
            let t = top.saturating_sub(ROWS);
            Some((t, cursor - (top - t)))
        }
        // The header row does nothing; a message is read (busy: 7 0x7a).
        K::Enter if cursor > 0 => {
            if w.friends.busy {
                super::textcmd::refuse(w, 0x7a);
            } else {
                w.sounds.push(Ui::Select as u32);
                read(w, &rows[cursor]);
            }
            None
        }
        // FUN_10200c90: mes1rcv / mes2frnd close with it.
        K::Escape => {
            (w.polmail.list, w.polmail.read) = (None, None);
            w.sounds.push(Ui::Cancel as u32);
            None
        }
        _ => None,
    };
    if let Some((t, c)) = moved {
        w.polmail.list = Some((rows, t, c, from));
        w.sounds.push(Ui::Cursor as u32);
    }
}

fn read_pick(w: &mut World, name: &str, frnd: bool, c: u16) {
    let Some(m) = mail(w, name).cloned() else { return };
    let sender = from_name(w, &m);
    let close = |w: &mut World| w.polmail.read = None;
    match (frnd, c) {
        // mes1rcv: Reply, Ignore Sender, Leave Unread, Exit (marked read, 9 0x19).
        (false, 1) => {
            if compose(w, Mode::Reply { to: m.header.from, name: sender }, String::new()) {
                close(w);
            }
        }
        (false, 2) | (true, 3) => w.confirm = Some((super::Confirm::PolIgnore, false)),
        (false, 3) | (true, 4) => close(w),
        (false, 4) => {
            friends::mark_read(w, name);
            super::textcmd::log9(w, 0x19);
            refresh(w);
            close(w);
        }
        // mes2frnd: Accept (the "Friend's Name:" box with their name), Decline (sent at once).
        (true, 1) => {
            if compose(w, Mode::Accept { to: m.header.from, name: sender.clone() }, m.header.sender.clone()) {
                if let Some(l) = w.polmail.line.as_mut() {
                    l.about = Some(name.to_string());
                }
                close(w);
            }
        }
        (true, 2) => {
            friends::send(w, Op::Decline { to: m.header.from }, Vec::new());
            friends::mark_read(w, name);
            refresh(w);
            close(w);
        }
        _ => {}
    }
}

/// Ignore Sender's prompt 9 0x1a (default No), Yes (FUN_10200280): the sender on the block list (9 0x18); then mes2frnd declines, mes1rcv marks the
/// message read (9 0x19).
pub fn ignore_yes(w: &mut World) {
    let Some((name, frnd, _)) = w.polmail.read.take() else { return };
    let Some(m) = mail(w, &name).cloned() else { return };
    let sender = m.header.sender.clone();
    friends::ignore(w, m.header.from, &sender, move |w: &mut World| {
        if frnd {
            friends::send(w, Op::Decline { to: m.header.from }, Vec::new());
            friends::mark_read(w, &name);
        } else {
            friends::mark_read(w, &name);
            super::textcmd::log9(w, 0x19);
        }
        refresh(w);
    });
}

pub(super) fn date(w: &World, unix: u32) -> String {
    let Some(bt) = w.battle.as_ref() else { return String::new() };
    let g = |id: u16| bt.group10_raw(id as usize);
    let (key, h12) = ((w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::earth_datetime(&g, unix as i64, 0, key, h12))
}

/// msglist's rows (the header band windowps 275, then From / To / Type / Date), mes1rcv / mes2frnd from their layouts,
/// msgline at the chat line's width with "Friend's Name:" before a name box.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32, chat_w: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let free = w.confirm.is_none() && w.polmail.line.is_none();
    if let Some((rows, top, cursor, _)) = &w.polmail.list {
        let focused = free && w.polmail.read.is_none();
        let sel = if focused { (cursor - top + 1) as u16 } else { 0 };
        if let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) {
            rects.extend(gu.window_rect(LIST, sw, sh));
            let me = w.friends.handle.as_ref().map(|h| h.1.clone()).unwrap_or_default();
            for (k, name) in rows.iter().enumerate().skip(*top).take(ROWS) {
                let (x, y) = (wx + 3.0, wy + 4.0 + 16.0 * (k - top) as f32);
                if k == 0 {
                    gu.sprite("menu    windowps", 275, wx, y, WHITE);
                    continue;
                }
                let Some(m) = mail(w, name) else { continue };
                gu.shape_fit(&from_name(w, m), x + COL[1], y + 2.0, 67.0, WHITE);
                gu.shape_fit(&me, x + COL[2], y + 2.0, 72.0, WHITE);
                gu.shape_text(type_label(m.header.kind), x + COL[3], y, WHITE, false);
                gu.shape_text(&date(w, m.header.date), x + COL[4], y, WHITE, false);
            }
            if focused {
                gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (cursor - top) as f32, WHITE);
            }
            if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
                gu.scroll_bar(fx + fw - 8.0, fy, fh, rows.len(), ROWS, *top);
            }
        }
    }
    if let Some((_, frnd, c)) = &w.polmail.read {
        let name = if *frnd { FRND } else { RCV };
        let off = read_off(w);
        if let Some(win) = gu.lobby_window(name, sw, sh, false, Some(*c), free && !off.contains(c), &off) {
            super::winhits::record(w, name, &win);
            gu.disabled_children(name, win.frame[0], win.frame[1], &off, Some(*c));
            rects.push(win.frame);
        }
    }
    if let Some(l) = &w.polmail.line
        && let Some(m) = gu.layout(LINE)
    {
        let (x, y) = (16.0, sh - 448.0 + m.elements[0].y as f32);
        let width = chat_w + 14.0;
        gu.window_sized(LINE, x, y, width, 22.0);
        let mut tx = x + 2.0 + 4.0;
        if l.mode.naming() {
            let label = w.battle.as_ref().and_then(|b| b.group9(0x22)).unwrap_or_default();
            gu.text(&label, tx, y + 3.0, 0, crate::dat::battle::argb_bytes(0x8040_60a0));
            tx += gu.text_width(&label) + 6.0;
        }
        let caret = if (w.started.elapsed().as_secs_f32() * 60.0) as u32 % 30 < 20 { "|" } else { "" };
        gu.text(&format!("{}{caret}", l.text), tx, y + 3.0, 0, [0x80; 4]);
        rects.push([x, y, width, 22.0]);
    }
    rects
}

/// The mouse: mes1rcv / mes2frnd as the generic window, msglist's rows.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k);
    if let Some((_, frnd, c)) = w.polmail.read.clone() {
        let name = if frnd { FRND } else { RCV };
        super::winhits::generic(w, gu, name, c, 1..=4, p, moved, clicked, right, |w, id| {
            if let Some(r) = w.polmail.read.as_mut() {
                r.2 = id;
            }
        }, keys);
        return;
    }
    let Some((rows, top, cursor, _)) = w.polmail.list.clone() else { return };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(LIST, sw, sh), gu.layout(LIST)) else { return };
    if gu.window_rect(LIST, sw, sh).is_some_and(|[fx, fy, fw, fh]| p.0 >= fx && p.0 < fx + fw && p.1 >= fy && p.1 < fy + fh) {
        gu.hover.set(true);
        if right {
            key(w, egui::Key::Escape);
            return;
        }
    }
    let inside = |e: &crate::dat::ui::MenuElement| p.0 >= x + e.x as f32 && p.0 < x + e.x as f32 + e.w as f32 && p.1 >= y + e.y as f32 && p.1 < y + e.y as f32 + e.h as f32;
    let Some(e) = lay.elements.iter().skip(1).find(|e| inside(e)) else { return };
    let row = top + e.id as usize - 1;
    if row >= rows.len() {
        return;
    }
    if row != cursor && (moved || clicked) {
        if let Some(l) = w.polmail.list.as_mut() {
            l.2 = row;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        key(w, egui::Key::Enter);
    }
}

/// The bars: the read window's rows' own strings.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if let Some((_, frnd, c)) = &w.polmail.read {
        let [help, title] = gu.focus_text(if *frnd { FRND } else { RCV }, *c);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
}
