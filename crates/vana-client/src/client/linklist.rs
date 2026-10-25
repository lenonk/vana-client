//! Linkshell's Link List (spec ~/ffxi/lab/research/menu_rows2_spec.md L4.5): "menu    link12", the scresult class on
//! its own object, listing the members of an equipped linkshell from the search server's group list (kind 2), and the
//! member windows link7..10 (Tell, To Sack / To Pearl, Kick).

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::net::search::{Cond, Record, Reply};

const LIST: &str = "menu    link12";
const WHITE: u32 = 0x8080_8080;
const CURSOR_BAR: usize = 75;
const ROWS: usize = 10;
const PITCH: f32 = 18.0;
/// DAT_1037f30c: the columns.
const COLS: [f32; 5] = [1.0, 18.0, 156.0, 160.0, 270.0];

/// The member windows by (our rank, theirs): link7 a pearl, link8 a pearlsack, link9 / link10.
const PEARL_ACTS: &str = "menu    link7";
const SACK_ACTS: &str = "menu    link8";
const SACK_SELF: &str = "menu    link9";
const TELL_ONLY: &str = "menu    link10";

pub struct LinkList {
    /// The linkshell slot (1 / 2), its group id and our rank there (the item's Flag @8).
    n: u8,
    group: u32,
    rank: u8,
    busy: bool,
    failed: bool,
    /// The members wearing this linkshell, with their rank in it.
    rows: Vec<(Record, u8)>,
    top: usize,
    cursor: usize,
    alt: bool,
    /// The member window open over it and its cursor.
    acts: Option<(&'static str, u16)>,
}

/// FUN_101e5c50(n): the group list of linkshell `n`'s id (the others 0) into link12; nothing while a query is out.
pub fn open(w: &mut World, n: u8, group: u32, rank: u8) {
    if super::search::busy(w) {
        return;
    }
    let ids = if n == 1 { [0, 0, group, 0] } else { [0, 0, 0, group] };
    log::info!("link list: linkshell {n} group {group:#x}");
    let rx = crate::net::search::spawn(w.search_addr, Cond::group(ids));
    w.searches.push((rx, super::search::Then::LinkList));
    w.linklist = Some(LinkList { n, group, rank, busy: true, failed: false, rows: Vec::new(), top: 0, cursor: 0, alt: false, acts: None });
}

/// LAB_1020ac30's rows: only records carrying our group id among their three linkshells (FUN_101e4e50).
pub fn filled(w: &mut World, reply: Option<Reply>) {
    let Some(l) = w.linklist.as_mut() else { return };
    l.busy = false;
    let Some(reply) = reply else {
        l.failed = true;
        return;
    };
    let g = l.group;
    l.rows = reply
        .records
        .into_iter()
        .filter_map(|r| {
            let k = r.ls_ids.iter().position(|&id| id == g)?;
            let rank = r.ls_ranks[k];
            Some((r, rank))
        })
        .collect();
}

/// FUN_101e4d10: the member window by our rank and theirs.
fn acts_for(ours: u8, theirs: u8) -> &'static str {
    match (ours, theirs) {
        (1, 1) => TELL_ONLY,
        (1, 2) => SACK_ACTS,
        (1, _) => PEARL_ACTS,
        (2, _) => SACK_SELF,
        _ => TELL_ONLY,
    }
}

/// The member window's disabled rows (FUN_101e6900 / 69e0 / 6ac0, FUN_101e6780): rank changes and Kick only through
/// linkshell 1; the rank change only for members wearing it as their linkshell 1 *(inf)*; Kick not on a holder, on
/// our own rank, or on ourselves.
fn off(w: &World, l: &LinkList, name: &str, r: &Record, theirs: u8) -> Vec<u16> {
    let mut off = Vec::new();
    let kick = if name == SACK_SELF { 2 } else { 3 };
    if matches!(name, PEARL_ACTS | SACK_ACTS) && (l.n != 1 || r.ls_ids[0] != l.group) {
        off.push(2);
    }
    if matches!(name, PEARL_ACTS | SACK_ACTS | SACK_SELF) && (l.n != 1 || theirs == 1 || theirs == l.rank || r.id == w.my_id) {
        off.push(kick);
    }
    off
}

fn close(w: &mut World) {
    w.linklist = None;
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(l) = w.linklist.as_ref() else { return };
    let n = l.rows.len();
    // A member window.
    if let Some((name, sel)) = l.acts {
        let Some((r, theirs)) = l.rows.get(l.cursor).cloned() else { return };
        let off = off(w, l, name, &r, theirs);
        match key {
            K::ArrowUp | K::ArrowDown => {
                let to = gu.menu_links(name, sel).map_or(sel, |k| k[if key == K::ArrowUp { 2 } else { 3 }] as u16);
                if to != sel && to != 0 && to != 0xff {
                    if let Some(l) = w.linklist.as_mut() {
                        l.acts = Some((name, to));
                    }
                    w.sounds.push(Ui::Cursor as u32);
                }
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                if let Some(l) = w.linklist.as_mut() {
                    l.acts = None;
                }
            }
            K::Enter if off.contains(&sel) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                let kick = if name == SACK_SELF { 2 } else { 3 };
                match sel {
                    1 => {
                        (w.chat_input, w.chat_caret) = (format!("/tell {} ", r.name), None);
                        w.typing = true;
                    }
                    s if s == kick => {
                        w.confirm = Some((super::Confirm::LinkKick(Kick::new(&r.name, r.race)), false));
                        return;
                    }
                    // To Sack (ChangeKind 2) / To Pearl (3): c2s 0x077 {name, kind 1, change}.
                    _ => {
                        let change = if name == PEARL_ACTS { 2 } else { 3 };
                        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x077, body: super::party::change_body(&r.name, 1, change) });
                    }
                }
                // Every action closes link12 (FUN_1015e910), back to link5.
                close(w);
            }
            _ => {}
        }
        return;
    }
    let cursor = l.cursor;
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight if n > 0 => {
            let step = if matches!(key, K::ArrowUp | K::ArrowDown) { 1 } else { ROWS };
            let to = if matches!(key, K::ArrowDown | K::ArrowRight) { (cursor + step).min(n - 1) } else { cursor.saturating_sub(step) };
            if to != cursor && let Some(l) = w.linklist.as_mut() {
                l.cursor = to;
                l.top = l.top.min(to).max((to + 1).saturating_sub(ROWS));
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        K::Enter => match l.rows.get(cursor) {
            Some((_, theirs)) if !l.busy => {
                let name = acts_for(l.rank, *theirs);
                w.sounds.push(Ui::Select as u32);
                if let Some(l) = w.linklist.as_mut() {
                    l.acts = Some((name, 1));
                }
            }
            _ => w.sounds.push(Ui::Error as u32),
        },
        _ => {}
    }
}

/// Num+ (list message 8, FUN_1020a580): the job cells' other view.
pub fn toggle_view(w: &mut World) {
    if let Some(l) = w.linklist.as_mut() {
        l.alt = !l.alt;
    }
}

/// The kick prompt's subject (the name's 15 bytes, kept Copy for the prompt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kick {
    name: [u8; 15],
    race: u8,
}

impl Kick {
    fn new(name: &str, race: u8) -> Kick {
        let mut b = [0u8; 15];
        for (d, s) in b.iter_mut().zip(name.bytes()) {
            *d = s;
        }
        Kick { name: b, race }
    }

    fn name(&self) -> String {
        self.name.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
    }
}

/// Group 10 0x36 "Remove <name> from your linkshell group?" with the name and its pronoun.
pub fn kick_text(w: &World, k: &Kick) -> String {
    let Some(raw) = w.battle.as_ref().and_then(|b| b.group10_raw(0x36)) else { return String::new() };
    let name = crate::dat::font::encode_sjis(&k.name());
    let p = super::partymenu::pronoun(k.race as u16);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&name], &[p], &|_, _, _| None))
}

/// LAB_101e66a0: c2s 0x071 {0, 0, kind 1, name}; link12 closes.
pub fn kick(w: &mut World, k: &Kick) {
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x071, body: super::party::kick_body(1, &k.name()) });
    close(w);
}

/// Help: link12's frame strings, a member window's rows; title "Linkshell" (0xd 0x47).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(l) = &w.linklist else { return };
    bars.title[2] = gu.title_msg(0x47).unwrap_or_default();
    bars.help[2] = String::new();
    bars.help[1] = match l.acts {
        Some((name, sel)) => gu.focus_text(name, sel)[0].clone().unwrap_or_default(),
        None => gu.focus_text(LIST, 1)[0].clone().unwrap_or_default(),
    };
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(l) = &w.linklist else { return rects };
    let focused = l.acts.is_none() && w.confirm.is_none();
    let n = l.rows.len();
    let sel = if focused && n > 0 { (l.cursor - l.top + 1) as u16 } else { 0 };
    let Some((wx, wy)) = gu.menu_sel(LIST, sw, sh, sel) else { return rects };
    rects.extend(gu.window_rect(LIST, sw, sh));
    let bt = w.battle.clone();
    if !l.busy && (l.failed || n == 0) {
        // "Search failed." (7 0x65) or "No applicable entries." (7 0x6b).
        let s = bt.as_ref().and_then(|b| b.group7(if l.failed { 0x65 } else { 0x6b })).unwrap_or_default();
        gu.text(&s, wx + 36.0, wy + 28.0, 0, crate::dat::battle::argb_bytes(0x8080_4040));
    }
    if let Some(info) = &w.item_info {
        super::items::upload_icons(gu, info, [0x201u16, 0x202, 0x203].into_iter());
    }
    for (i, (r, rank)) in l.rows.iter().enumerate().skip(l.top).take(ROWS) {
        let (rx, ry) = (wx + 3.0, wy + 3.0 + PITCH * (i - l.top) as f32);
        // The rank's item icon at half scale: linkshell 0x201, pearlsack 0x202, linkpearl 0x203.
        let item = match rank {
            2 => 0x202,
            3 => 0x203,
            _ => 0x201,
        };
        let icon = crate::render::invwin::icon_texture(item);
        if gu.has_image(&icon) {
            gu.image_rect(&icon, rx + COLS[0], ry, 16.0, 16.0, [1.0; 4]);
        }
        super::search::job_cell(gu, r, rx + COLS[1], ry + 2.0, WHITE, l.alt);
        if r.flags & 0x10 != 0 {
            let k = ((r.comment >> 4) & 0xf) as usize + 0xb;
            if let Some((lib, s)) = super::search::icon(k).filter(|_| k < 0x19) {
                gu.sprite(lib, s, rx + COLS[2], ry + 2.0, WHITE);
            }
        }
        let nc = if r.flags & 8 != 0 {
            gu.ncol(5)
        } else if r.flags & 0x2000 != 0 {
            gu.ncol(1)
        } else {
            WHITE
        };
        gu.shape_fit(&r.name, rx + COLS[3], ry + 2.0, COLS[4] - COLS[3] - 8.0, nc);
        if r.zone != w.pos_zone {
            let s = crate::dat::font::decode_sjis(&gu.area_name(r.zone));
            gu.shape_text(&s, rx + COLS[4], ry + 2.0, WHITE, false);
        }
    }
    if focused && n > 0 {
        gu.sprite("menu    windowps", CURSOR_BAR, wx, wy + 4.0 + PITCH * (l.cursor - l.top) as f32, WHITE);
    }
    if let Some([fx, fy, fw, fh]) = gu.window_rect(LIST, sw, sh) {
        gu.scroll_bar(fx + fw - 8.0, fy, fh, n, ROWS, l.top);
    }
    if let Some((name, sel)) = l.acts
        && let Some((r, theirs)) = l.rows.get(l.cursor)
    {
        let off = off(w, l, name, r, *theirs);
        if let Some(win) = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), Some(sel), w.confirm.is_none() && !off.contains(&sel), &off) {
            gu.disabled_children(name, win.frame[0], win.frame[1], &off, Some(sel));
            rects.push(win.frame);
        }
    }
    rects
}
