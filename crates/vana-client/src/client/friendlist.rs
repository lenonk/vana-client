//! The Friend List's windows (research friend_list_spec.md 1): flistmai (To List / Messages / Online Stat.) from
//! commenu, the list "menu    friend" grouped under its category bars, flmes on a friend, olstat, and titlehan with our
//! handle. The data is client/friends.rs's.

use super::World;
use super::friends::Friend;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub(super) const MAIN: &str = "menu    flistmai";
pub(super) const LIST: &str = "menu    friend";
pub(super) const MES: &str = "menu    flmes";
pub(super) const STAT: &str = "menu    olstat";
pub(super) const TITLE: &str = "menu    titlehan";
const ROWS: usize = 15;
const WHITE: u32 = 0x8080_8080;
/// The category bars (windowps 0x10381fcc): Online Friends, Offline Friends, Ignore List, Pending, the block list.
pub(super) const BARS: [usize; 5] = [324, 325, 326, 327, 343];
/// The column table 0x10381f98.
const COL: [f32; 7] = [0.0, 18.0, 138.0, 171.0, 175.0, 265.0, 360.0];
/// olstat's children's values (table 0x10381fc7): Online, Away, Invisible.
pub(super) const STATS: [u8; 3] = [0, 1, 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A category's bar.
    Header(u8),
    /// An entry: its index in `friends.list`.
    Entry(usize),
}

#[derive(Clone, Debug, Default)]
pub struct FriendUi {
    /// commenu, suspended under flistmai (none when /friendlist opened the list).
    back: Option<super::magic::Cats>,
    /// flistmai's cursor while it is up.
    pub main: Option<u16>,
    /// The list's rows, top and cursor while it is up.
    pub list: Option<(Vec<Row>, usize, usize)>,
    /// flmes's cursor over the list.
    pub mes: Option<u16>,
    /// olstat's cursor.
    pub stat: Option<u16>,
}

/// FUN_101eb0d0's category: 4 the block list, 3 pending, 2 status 4 (the Ignore List), 0 status 1..3, else 1.
pub fn category(f: &Friend) -> u8 {
    match () {
        _ if f.black => 4,
        _ if f.pending => 3,
        _ if f.status == 4 => 2,
        _ if (1..=3).contains(&f.status) => 0,
        _ => 1,
    }
}

/// commenu child 1 (FUN_101ebdc0): flistmai, commenu suspended under it.
pub fn open_main(w: &mut World) {
    let back = w.cats.take();
    w.friendui = Some(FriendUi { back, main: Some(1), ..Default::default() });
}

/// FUN_101eb660 (flistmai's To List, /friendlist): the rows built afresh (a snapshot), the view at the top.
pub fn open_list(w: &mut World) {
    let rows = rows(w);
    let ui = w.friendui.get_or_insert_with(Default::default);
    (ui.list, ui.mes) = (Some((rows, 0, 0)), None);
}

/// FUN_101eb0d0: every entry sorted by category then name (byte order); a bar before each category, a hidden one's
/// entries left out.
fn rows(w: &World) -> Vec<Row> {
    let list = &w.friends.list;
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by(|&a, &b| (category(&list[a]), list[a].name.as_bytes()).cmp(&(category(&list[b]), list[b].name.as_bytes())));
    let mut out = Vec::new();
    let mut last = None;
    for k in order {
        let c = category(&list[k]);
        if last != Some(c) {
            out.push(Row::Header(c));
            last = Some(c);
        }
        if !w.friends.hidden[c as usize] {
            out.push(Row::Entry(k));
        }
    }
    out
}

fn cursor_row(w: &World) -> Option<Row> {
    let (rows, _, c) = w.friendui.as_ref()?.list.as_ref()?;
    rows.get(*c).copied()
}

/// The friend flmes is on.
fn mes_friend(w: &World) -> Option<&Friend> {
    match cursor_row(w)? {
        Row::Entry(k) => w.friends.list.get(k),
        Row::Header(_) => None,
    }
}

/// FUN_101eb880's disabled rows: 1 Tell unless they play FFXI with a record (kind 5), 2 Send Message when pending or on
/// the Ignore List, 3 View Messages with no unread message from them.
pub fn mes_off(w: &World) -> Vec<u16> {
    let Some(f) = mes_friend(w) else { return vec![1, 2, 3] };
    let mut off = Vec::new();
    if !(category(f) == 0 && f.fresh && f.record.is_some()) {
        off.push(1);
    }
    if f.pending || category(f) == 2 {
        off.push(2);
    }
    if !f.mail {
        off.push(3);
    }
    off
}

/// Which window has the keys.
pub fn focused(w: &World) -> bool {
    w.friendui.is_some() && w.confirm.is_none()
}

pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(ui) = w.friendui.clone() else { return };
    let wrap = |c: u16, n: u16, down: bool| if down { c % n + 1 } else { (c + n - 2) % n + 1 };
    // olstat (FUN_101ebd10): Left / Right along its three, Enter sets the status (and the away flag), it stays open.
    if let Some(c) = ui.stat {
        match key {
            K::ArrowLeft | K::ArrowRight => {
                set(w, |u| u.stat = Some(wrap(c, 3, key == K::ArrowRight)));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                let v = STATS[c as usize - 1];
                super::friends::set_status(w, v);
                super::textcmd::config_flag(w, 1, v == 1);
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                set(w, |u| u.stat = None);
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return;
    }
    // flmes (FUN_101eb9d0): Up / Down wrap, the generic Enter (SE 0004 on a disabled row).
    if let Some(c) = ui.mes {
        match key {
            K::ArrowUp | K::ArrowDown => {
                set(w, |u| u.mes = Some(wrap(c, 3, key == K::ArrowDown)));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                set(w, |u| u.mes = None);
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter if mes_off(w).contains(&c) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                mes_pick(w, c);
            }
            _ => {}
        }
        return;
    }
    if let Some((rows, top, cursor)) = ui.list.clone() {
        let n = rows.len();
        let last = n.saturating_sub(1);
        let max_top = n.saturating_sub(ROWS);
        let moved = match key {
            K::ArrowDown if cursor < last => Some((top.max((cursor + 2).saturating_sub(ROWS)), cursor + 1)),
            K::ArrowUp if cursor > 0 => Some((top.min(cursor - 1), cursor - 1)),
            K::ArrowRight if n > 0 && top < max_top => {
                let t = (top + ROWS).min(max_top);
                Some((t, cursor + t - top))
            }
            K::ArrowRight if n > 0 => Some((top, last)),
            K::ArrowLeft if top > 0 => {
                let t = top.saturating_sub(ROWS);
                Some((t, cursor - (top - t)))
            }
            K::ArrowLeft => Some((top, 0)),
            K::Enter => {
                match rows.get(cursor) {
                    // A bar: its category hidden or shown, the rows built again from the top.
                    Some(&Row::Header(c)) => {
                        let h = &mut w.friends.hidden[c as usize];
                        *h = !*h;
                        let rows = self::rows(w);
                        let at = rows.iter().position(|r| *r == Row::Header(c)).unwrap_or(0);
                        set(w, |u| u.list = Some((rows, 0, at)));
                        w.sounds.push(Ui::Select as u32);
                    }
                    Some(Row::Entry(_)) => {
                        set(w, |u| u.mes = Some(1));
                        w.sounds.push(Ui::Select as u32);
                    }
                    None => {}
                }
                None
            }
            // FUN_101eb600: titlehan and flmes close with it; flistmai has the keys again.
            K::Escape => {
                set(w, |u| (u.list, u.mes) = (None, None));
                close_empty(w);
                w.sounds.push(Ui::Cancel as u32);
                None
            }
            _ => None,
        };
        if let Some((t, c)) = moved
            && (t, c) != (top, cursor)
        {
            set(w, |u| u.list = Some((rows, t, c)));
            w.sounds.push(Ui::Cursor as u32);
        }
        return;
    }
    // flistmai (FUN_101ebc80): Up / Down wrap, Enter acts with its own SE 0002, Escape back to commenu.
    if let Some(c) = ui.main {
        match key {
            K::ArrowUp | K::ArrowDown => {
                set(w, |u| u.main = Some(wrap(c, 3, key == K::ArrowDown)));
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                match c {
                    1 => open_list(w),
                    2 => {
                        super::polmail::open_list(w, None);
                    }
                    _ => {
                        let now = 1 + STATS.iter().position(|&s| s == w.friends.my_status).unwrap_or(0) as u16;
                        set(w, |u| u.stat = Some(now));
                    }
                }
                w.sounds.push(Ui::Select as u32);
            }
            K::Escape => {
                let back = w.friendui.take().and_then(|u| u.back);
                w.cats = back;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    }
}

/// The modern layer's mouse: the cursor onto a row of the top window (list rows from 0). True when it moved.
pub(super) fn set_cursor(w: &mut World, k: usize) -> bool {
    let Some(u) = w.friendui.as_mut() else { return false };
    let id = k as u16;
    if let Some(c) = u.stat {
        u.stat = Some(id);
        return c != id;
    }
    if let Some(c) = u.mes {
        u.mes = Some(id);
        return c != id;
    }
    if let Some(l) = u.list.as_mut() {
        if k >= l.0.len() {
            return false;
        }
        let old = std::mem::replace(&mut l.2, k);
        l.1 = l.1.min(k).max((k + 1).saturating_sub(ROWS));
        return old != k;
    }
    if let Some(c) = u.main {
        u.main = Some(id);
        return c != id;
    }
    false
}

/// What the modern layer draws: the rows as (category or entry), the cursor; flmes's / olstat's / flistmai's.
pub(super) fn rows_view(w: &World) -> Option<(&[Row], usize)> {
    let (rows, _, c) = w.friendui.as_ref()?.list.as_ref()?;
    Some((rows, *c))
}

fn set(w: &mut World, f: impl FnOnce(&mut FriendUi)) {
    if let Some(u) = w.friendui.as_mut() {
        f(u);
    }
}

/// The list closed with no flistmai under it (/friendlist): nothing is left.
fn close_empty(w: &mut World) {
    if w.friendui.as_ref().is_some_and(|u| u.main.is_none() && u.list.is_none() && u.stat.is_none()) {
        let back = w.friendui.take().and_then(|u| u.back);
        w.cats = back;
    }
}

/// flmes's rows: 1 the chat line on "/tell <character name> ", 2 the composer (a message to them), 3 their messages.
fn mes_pick(w: &mut World, c: u16) {
    let Some(f) = mes_friend(w).cloned() else { return };
    match c {
        1 => {
            let name = f.record.as_ref().map(|r| r.name.clone()).unwrap_or_default();
            (w.chat_input, w.chat_caret, w.chat_anchor) = (format!("/tell {name} "), None, None);
            w.typing = true;
            set(w, |u| u.mes = None);
        }
        2 => {
            if super::polmail::compose(w, super::polmail::Mode::Friend { account: f.account }, String::new()) {
                set(w, |u| u.mes = None);
            }
        }
        _ => {
            if super::polmail::open_list(w, Some(f.account)) {
                set(w, |u| u.mes = None);
            }
        }
    }
}

/// The mouse: the generic windows (olstat, flmes, flistmai) and the list's rows (hover moves, a click on the cursor row
/// is Enter, a right click Escape).
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::generic;
    let Some(ui) = w.friendui.clone() else { return };
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k);
    if let Some(c) = ui.stat {
        generic(w, gu, STAT, c, 1..=3, p, moved, clicked, right, |w, id| set(w, |u| u.stat = Some(id)), keys);
        return;
    }
    if let Some(c) = ui.mes {
        generic(w, gu, MES, c, 1..=3, p, moved, clicked, right, |w, id| set(w, |u| u.mes = Some(id)), keys);
        return;
    }
    if let Some((rows, top, cursor)) = ui.list {
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
            set(w, |u| {
                if let Some(l) = u.list.as_mut() {
                    l.2 = row;
                }
            });
            w.sounds.push(Ui::Cursor as u32);
        } else if clicked {
            key(w, egui::Key::Enter);
        }
        return;
    }
    if let Some(c) = ui.main {
        generic(w, gu, MAIN, c, 1..=3, p, moved, clicked, right, |w, id| set(w, |u| u.main = Some(id)), keys);
    }
}

/// "(" + the name fitted to `max` + ")" in the shape font.
fn paren(gu: &mut GameUi, s: &str, x: f32, y: f32, max: f32) {
    let x = gu.shape_text("(", x, y, WHITE, false);
    let x = gu.shape_fit(s, x, y, max, WHITE);
    gu.shape_text(")", x, y, WHITE, false);
}

/// The status icon of a friend in the Online category (FUN_101eae00): keytops3 64 / 65 / 66 / 177.
pub(super) fn status_icon(s: u8) -> Option<usize> {
    match s {
        1 => Some(64),
        2 => Some(65),
        3 => Some(66),
        4 => Some(177),
        _ => None,
    }
}

/// The list base's cells (1.4.4) at (wx + 4 + col, wy + 3 + 16 k): bars, the status icon (or the bubble for unread
/// mail), "(Handle)", the game icon, the character name and their zone when not ours; the band while focused; the
/// scroll bar; "No friends registered." on an empty list. flistmai, flmes, olstat and titlehan from their layouts.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(ui) = &w.friendui else { return rects };
    let free = w.confirm.is_none() && !super::polmail::composing(w);
    if let Some(c) = ui.main
        && ui.list.is_none()
        && ui.stat.is_none()
        && let Some(win) = gu.lobby_window(MAIN, sw, sh, false, Some(c), free, &[])
    {
        super::winhits::record(w, MAIN, &win);
        rects.push(win.frame);
    }
    if let Some((rows, top, cursor)) = &ui.list {
        let focused = free && ui.mes.is_none();
        let n = rows.len();
        let sel = if focused && n > 0 { (cursor - top + 1) as u16 } else { 0 };
        if let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) {
            rects.extend(gu.window_rect(LIST, sw, sh));
            if n == 0 {
                let s = w.battle.as_ref().and_then(|bt| bt.group7(0x66)).unwrap_or_default();
                gu.text(&s, wx + 36.0, wy + 27.0, 0, crate::dat::battle::argb_bytes(0x8080_4040));
            }
            for (k, row) in rows.iter().enumerate().skip(*top).take(ROWS) {
                let y = wy + 3.0 + 16.0 * (k - top) as f32;
                let cell = |c: usize| wx + 4.0 + COL[c];
                match *row {
                    Row::Header(c) => {
                        gu.sprite("menu    windowps", BARS[c as usize], wx + 1.0, y, WHITE);
                        if w.friends.hidden[c as usize] {
                            gu.shape_text("[Hidden]", cell(5), y, WHITE, false);
                        }
                    }
                    Row::Entry(i) => {
                        let Some(f) = w.friends.list.get(i) else { continue };
                        let cat = category(f);
                        let icon = if f.mail { Some(158) } else if cat == 0 { status_icon(f.status) } else { None };
                        if let Some(s) = icon {
                            gu.sprite("menu    keytops3", s, cell(0), y + 2.0, WHITE);
                        }
                        paren(gu, &f.name, cell(1), y + 4.0, 106.0);
                        if cat == 0 {
                            // The game they are in (contents class 1, FFXI: "menu    contents" sprite 0).
                            gu.sprite("menu    contents", 0, cell(2), y + 2.0, WHITE);
                            if let Some(r) = f.record.as_ref().filter(|_| f.fresh) {
                                if !r.name.is_empty() {
                                    gu.shape_fit(&r.name, cell(4), y + 2.0, 82.0, WHITE);
                                }
                                if r.zone != w.pos_zone {
                                    let z = crate::dat::font::decode_sjis(&gu.area_name(r.zone));
                                    gu.shape_text(&z, cell(5), y + 2.0, WHITE, false);
                                }
                            }
                        }
                    }
                }
            }
            if focused && n > 0 {
                gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (cursor - top) as f32, WHITE);
            }
            if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
                gu.scroll_bar(fx + fw - 8.0, fy, fh, n, ROWS, *top);
            }
        }
    }
    if let Some(c) = ui.mes {
        let off = mes_off(w);
        if let Some(win) = gu.lobby_window(MES, sw, sh, false, Some(c), free && !off.contains(&c), &off) {
            super::winhits::record(w, MES, &win);
            gu.disabled_children(MES, win.frame[0], win.frame[1], &off, Some(c));
            // FUN_101eb980: the handle name fitted to 108 and the handle id text above the buttons.
            if let Some(f) = mes_friend(w) {
                gu.shape_fit(&f.name, win.frame[0] + 2.0, win.frame[1] + 6.0, 108.0, WHITE);
                gu.shape_text(&handle_id_text(f.account), win.frame[0] + 2.0, win.frame[1] + 18.0, WHITE, false);
            }
            rects.push(win.frame);
        }
    }
    if let Some(c) = ui.stat
        && let Some(win) = gu.lobby_window(STAT, sw, sh, false, Some(c), false, &[])
    {
        super::winhits::record(w, STAT, &win);
        // FUN_101ebc20: the marker (keytops3 101) under the status in force; the half-size hand.
        let now = 1 + STATS.iter().position(|&s| s == w.friends.my_status).unwrap_or(0) as u16;
        if let Some(r) = win.children.iter().find(|c| c.0 == now).map(|c| c.1) {
            gu.sprite("menu    keytops3", 101, r[0] + 8.0, r[1] + 24.0, WHITE);
        }
        if free && let Some(r) = win.children.iter().find(|ch| ch.0 == c).map(|ch| ch.1) {
            gu.cursor_scaled("anc     anc2s", r[0], r[1], 1.0);
        }
        rects.push(win.frame);
    }
    // titlehan (FUN_10150da0) while the list (or the message list) is up: our handle and its id.
    if (ui.list.is_some() || super::polmail::list_up(w))
        && let Some((id, name)) = w.friends.handle.clone()
        && let Some(win) = gu.lobby_window(TITLE, sw, sh, false, None, false, &[])
    {
        gu.shape_fit(&name, win.frame[0] + 4.0, win.frame[1] + 5.0, 106.0, WHITE);
        gu.shape_text(&handle_id_text(id), win.frame[0] + 20.0, win.frame[1] + 17.0, WHITE, false);
    }
    rects
}

/// Slot 0x1db's handle id text *(inf: polcore's 8 base-36 digits, as the IRC nick spells the id)*.
pub fn handle_id_text(id: u64) -> String {
    crate::net::profile::nick(id)[1..].to_string()
}

/// The bars from the focused window's strings; the list's bar rows say "Toggle display of this category of friends.".
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(ui) = &w.friendui else { return };
    // msglist over flistmai has the focus: its rows have no strings.
    if super::polmail::list_up(w) {
        (bars.help[1], bars.title[1]) = (String::new(), String::new());
        return;
    }
    let [help, title] = if let Some(c) = ui.stat {
        gu.focus_text(STAT, c)
    } else if let Some(c) = ui.mes {
        gu.focus_text(MES, c)
    } else if ui.list.is_some() {
        let help = matches!(cursor_row(w), Some(Row::Header(_))).then(|| gu.menu_msg(0x2c, ""));
        [help, None]
    } else if let Some(c) = ui.main {
        gu.focus_text(MAIN, c)
    } else {
        [None, None]
    };
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}
