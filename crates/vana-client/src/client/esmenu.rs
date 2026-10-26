//! MacroPalette's Edit Equip. Set (spec ~/ffxi/lab/research/social_rows_spec.md M2.4 / M2.5): the set list
//! "menu    mcres20" (ten pages of 20, mcralpha names, the in-row name field) and its menu "menu    mcresmn"
//! (Edit, Name, Copy, Paste, Delete, Overwrite, Equip, Set appearance). The store and packets: client/equipsets.rs; the
//! slot editor: client/esedit.rs.

use super::World;
use super::game_ui::GameUi;
use super::macroedit::{Ask, alpha, caret};
use crate::audio::Ui;

const LIST: &str = "menu    mcres20";
const MENU: &str = "menu    mcresmn";
const WHITE: u32 = 0x8080_8080;

/// mcres20: the cursor child (1..20 rows, 21 / 22 the page arrows), the name field's text, the picked row (+0x20)
/// and mcresmn's cursor while it is open; the slot editor.
pub struct SetList {
    sel: u16,
    rename: Option<Vec<u8>>,
    row: usize,
    opt: Option<u16>,
    pub(super) ed: Option<super::esedit::Editor>,
}

fn list_mut(w: &mut World) -> Option<&mut SetList> {
    w.macroedit.as_mut()?.sets.as_mut()
}

/// mcrmenu child 2 (FUN_101a6c90): the page kept for the session read (or its defaults), the cursor on row 1.
pub fn open(w: &mut World) {
    let Some(es) = w.equipsets.as_mut() else { return };
    es.load_page(es.page.unwrap_or(0));
    if let Some(m) = w.macroedit.as_mut() {
        m.sets = Some(SetList { sel: 1, rename: None, row: 0, opt: None, ed: None });
    }
}

pub fn typing(w: &World) -> bool {
    w.macroedit.as_ref().and_then(|m| m.sets.as_ref()).is_some_and(|s| s.rename.is_some())
}

/// FUN_101a7170(1): the field committed (Enter and Escape both; empty -> "Set%03d").
fn commit(w: &mut World) {
    let Some(s) = list_mut(w) else { return };
    let (row, raw) = (s.row, s.rename.take().unwrap_or_default());
    if let Some(es) = w.equipsets.as_mut() {
        es.rename(row, &raw);
    }
}

/// The name field (FUN_101c5be0 mode 4, 15 bytes): printable ASCII, Backspace, Enter / Escape commit.
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    use egui::Key as K;
    for ev in events {
        let Some(name) = list_mut(w).and_then(|s| s.rename.as_mut()) else { return };
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.bytes().filter(|c| (b' '..=b'~').contains(c)) {
                    if name.len() < 15 {
                        name.push(c);
                    }
                }
            }
            egui::Event::Key { key: K::Backspace, pressed: true, .. } => _ = name.pop(),
            egui::Event::Key { key: K::Enter | K::Escape, pressed: true, .. } => {
                commit(w);
                w.sounds.push(Ui::Select as u32);
            }
            _ => {}
        }
    }
}

/// FUN_101a71e0 / FUN_101a7230: the page written, the next / previous (wrapping 0..9) read.
fn flip(w: &mut World, next: bool) {
    let Some(es) = w.equipsets.as_mut() else { return };
    es.save_page();
    let page = es.page.unwrap_or(0);
    es.load_page(if next { (page + 1) % 10 } else { (page + 9) % 10 });
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(s) = w.macroedit.as_ref().and_then(|m| m.sets.as_ref()) else { return };
    if s.ed.is_some() {
        super::esedit::key(w, gu, key);
        return;
    }
    if s.rename.is_some() {
        return;
    }
    let (sel, row) = (s.sel, s.row);
    if let Some(o) = s.opt {
        menu_key(w, gu, key, o, row);
        return;
    }
    // FUN_101a6eb0, after the generic link move: Left / Right turn the page, Enter on a row opens mcresmn, on an
    // arrow turns the page; Escape writes the page and closes.
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            let k = match key {
                K::ArrowLeft => 5,
                K::ArrowRight => 4,
                K::ArrowUp => 2,
                _ => 3,
            };
            let to = gu.menu_links(LIST, sel).map_or(sel, |l| l[k] as u16);
            if (1..=22).contains(&to) && to != sel {
                if let Some(s) = list_mut(w) {
                    s.sel = to;
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            if matches!(key, K::ArrowLeft | K::ArrowRight) {
                flip(w, key == K::ArrowRight);
            }
        }
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            match sel {
                21 => flip(w, false),
                22 => flip(w, true),
                _ => {
                    if let Some(s) = list_mut(w) {
                        (s.row, s.opt) = (sel as usize - 1, Some(1));
                    }
                }
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        _ => {}
    }
}

/// The mouse on mcres20 (rows and page arrows) or mcresmn; the slot editor's own.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::macroedit::generic;
    let Some(s) = w.macroedit.as_ref().and_then(|m| m.sets.as_ref()) else { return };
    if s.ed.is_some() {
        super::esedit::pointer(w, gu, sw, sh, p, moved, clicked, right);
        return;
    }
    if s.rename.is_some() {
        return;
    }
    match s.opt {
        Some(o) => generic(w, gu, MENU, o, 1..=8, p, moved, clicked, right, |w, id| {
            if let Some(s) = list_mut(w) {
                s.opt = Some(id);
            }
        }),
        None => {
            let sel = s.sel;
            generic(w, gu, LIST, sel, 1..=22, p, moved, clicked, right, |w, id| {
                if let Some(s) = list_mut(w) {
                    s.sel = id;
                }
            });
        }
    }
}

/// FUN_101a6d00 after the page is written: the clipboard freed, an open name field dropped.
fn close(w: &mut World) {
    if let Some(es) = w.equipsets.as_mut() {
        es.save_page();
        es.clip = None;
    }
    if let Some(m) = w.macroedit.as_mut() {
        m.sets = None;
    }
}

/// mcresmn's dimmed rows: Paste without a copy (4), Set appearance during the lockstyle wait (8).
fn menu_off(w: &World) -> Vec<u16> {
    let Some(es) = w.equipsets.as_ref() else { return Vec::new() };
    let mut off = Vec::new();
    if es.clip.is_none() {
        off.push(4);
    }
    // FUN_101a72e0: Set appearance only once the lockstyle wait is over, and never in Monstrosity.
    if es.lock_waiting() || super::monstrosity::active(w) {
        off.push(8);
    }
    off
}

/// mcresmn (FUN_101a8190): every row closes it afterwards; Escape closes it.
fn menu_key(w: &mut World, gu: &GameUi, key: egui::Key, o: u16, row: usize) {
    use egui::Key as K;
    match key {
        K::ArrowUp | K::ArrowDown => {
            let to = gu.menu_links(MENU, o).map_or(o, |l| l[if key == K::ArrowUp { 2 } else { 3 }] as u16);
            if (1..=8).contains(&to) && to != o {
                if let Some(s) = list_mut(w) {
                    s.opt = Some(to);
                }
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            if let Some(s) = list_mut(w) {
                s.opt = None;
            }
            w.sounds.push(Ui::Cancel as u32);
        }
        K::Enter if menu_off(w).contains(&o) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            if let Some(s) = list_mut(w) {
                s.opt = None;
            }
            let Some(es) = w.equipsets.as_mut() else { return };
            let id = es.page.unwrap_or(0) * 20 + row;
            match o {
                1 => {
                    let ed = super::esedit::open(w, row);
                    if let Some(s) = list_mut(w) {
                        s.ed = ed;
                    }
                }
                2 => {
                    let name = es.sets.get(row).map_or(Vec::new(), |s| s.raw_name().to_vec());
                    if let Some(s) = list_mut(w) {
                        s.rename = Some(name);
                    }
                }
                3 => es.clip = es.sets.get(row).copied(),
                4 => w.confirm = Some((super::Confirm::Macro(Ask::EsPaste(row as u8)), false)),
                5 => w.confirm = Some((super::Confirm::Macro(Ask::EsDelete(row as u8)), false)),
                6 => w.confirm = Some((super::Confirm::Macro(Ask::EsOverwrite(row as u8)), false)),
                7 => _ = super::equipsets::equip(w, id, true),
                _ => _ = super::equipsets::lock_set(w, id, true),
            }
        }
        _ => {}
    }
}

/// comyn's yes (LAB_101a7650 / 7690 / 7670): the copy over the set, the defaults, the worn gear.
pub fn answer(w: &mut World, a: Ask) {
    let worn = super::equipsets::from_equipped(w);
    let Some(es) = w.equipsets.as_mut() else { return };
    match a {
        Ask::EsPaste(r) => {
            if let (Some(c), Some(s)) = (es.clip, es.sets.get_mut(r as usize)) {
                *s = c;
            }
        }
        Ask::EsDelete(r) => es.clear(r as usize),
        Ask::EsOverwrite(r) => {
            if let Some(s) = es.sets.get_mut(r as usize) {
                s.slots = worn;
            }
        }
        _ => {}
    }
}

/// The set's name for the title bar's slot 2.
fn name(w: &World, row: usize) -> String {
    w.equipsets.as_ref().and_then(|e| e.sets.get(row)).map_or(String::new(), |s| crate::dat::font::decode_sjis(s.raw_name()))
}

/// FUN_101a6d60: on a row, title slot 2 = the set's name and the help "Equipment Set (#%d)" (group 10 0x75); on an
/// arrow both cleared. mcresmn shows its own row help under the "Equipment Sets" title.
pub fn bars(gu: &GameUi, w: &World, s: &SetList, bars: &mut super::hudtop::BarText) {
    let focus = |bars: &mut super::hudtop::BarText, name: &str, child: u16| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    if s.ed.is_some() {
        super::esedit::bars(gu, w, bars);
        return;
    }
    if let Some(o) = s.opt {
        focus(bars, MENU, o);
        bars.title[2] = name(w, s.row);
        return;
    }
    focus(bars, LIST, s.sel);
    if (1..=20).contains(&s.sel) {
        let page = w.equipsets.as_ref().and_then(|e| e.page).unwrap_or(0);
        let n = page * 20 + s.sel as usize;
        bars.help[1] = gu.lobby_msg(0x0a, 0x75, "Equipment Set (#%d)").replacen("%d", &n.to_string(), 1);
        bars.title[2] = name(w, s.sel as usize - 1);
    }
}

/// FUN_101a6e00: each row's name in mcralpha at (8, 2) cut before 75 px, or the name field; mcresmn left of the
/// picked row (x = row x - its width, y = min(row y, mcres20 bottom - its height)).
pub fn draw(gu: &mut GameUi, w: &World, s: &SetList, sw: f32, sh: f32, free: bool) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(es) = w.equipsets.as_ref() else { return rects };
    if let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), Some(s.sel), free && s.opt.is_none() && s.rename.is_none() && s.ed.is_none(), &[]) {
        super::macroedit::hit(w, LIST, &win);
        for &(id, r) in win.children.iter().filter(|(id, _)| (1..=20).contains(id)) {
            let row = id as usize - 1;
            match &s.rename {
                Some(t) if row == s.row => gu.text(&format!("{}{}", String::from_utf8_lossy(t), caret(w)), r[0] + 8.0, r[1] + 2.0, 0, WHITE.to_le_bytes()),
                _ => alpha(gu, es.sets.get(row).map_or(&[][..], |s| s.raw_name()), r[0] + 8.0, r[1] + 2.0, 75.0),
            }
        }
        rects.push(win.frame);
        if let (Some(o), Some(&(_, r)), Some([lx, ly, lw, lh])) = (s.opt, win.children.iter().find(|(id, _)| *id as usize == s.row + 1), gu.window_rect(MENU, sw, sh)) {
            let [_, fy, _, fh] = win.frame;
            let (x, y) = (r[0] - lw, r[1].min(fy + fh - lh));
            let off = menu_off(w);
            if let Some(win) = gu.lobby_window_off(MENU, sw, sh, (x - lx, y - ly), Some(o), free, &off) {
                super::macroedit::hit(w, MENU, &win);
                gu.disabled_children(MENU, win.frame[0], win.frame[1], &off, free.then_some(o));
                rects.push(win.frame);
            }
        }
    }
    // The slot editor over the list (mcrmogc sits where mcres20 does).
    if s.ed.is_some() {
        rects.extend(super::esedit::draw(gu, w, sw, sh, free));
    }
    rects
}
