//! The Mog House 2F Remodel list (DAT_106631e0: FUN_101d6890 .. FUN_101d6b20) in "menu    roomlist": "No changes"
//! and the four 2F styles; a style asks "Make changes?" and Yes sends c2s 0x0CB {5, 0, map} (LSB GP_CLI_COMMAND_MYROOM_IS
//! Remodel), which LSB answers with its line and, on 2F, a rezone.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub const WINDOW: &str = "menu    roomlist";
/// FUN_101d6900: each row's group 0xc text and 2F map (0 closes the list).
const ROWS: [(u16, u16); 5] = [(0x1c2, 0), (0x1be, 0x267), (0x1bf, 0x268), (0x1c0, 0x269), (0x1c1, 0x26a)];
/// FUN_10097e80(0xbeb): the Mog Patio Design Document; without it the patio row is grey and buzzes.
const PATIO_KI: u16 = 0xbeb;
/// DAT_1037ec5c: the list's columns (20, 245); rows from (+3, +4), 16 apart (FUN_101f6a60, as arealist).
const COL: f32 = 20.0;
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;

#[derive(Clone, Copy, Debug)]
pub struct Remodel {
    pub sel: usize,
}

pub fn open(w: &mut World) {
    w.remodel = Some(Remodel { sel: 0 });
}

fn tint(w: &World, row: usize) -> u32 {
    if row == 4 && !w.has_key_item(PATIO_KI) { GREY } else { WHITE }
}

/// FUN_101d6b20 (Enter): "No changes" closes (SE 0003); the patio without its key item buzzes; a style asks group 0xc
/// 0x1c3 "Make changes?" (SE 0002).
fn pick(w: &mut World) {
    let Some(r) = w.remodel else { return };
    match ROWS[r.sel] {
        (_, 0) => {
            w.remodel = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        _ if tint(w, r.sel) == GREY => w.sounds.push(Ui::Error as u32),
        (_, map) => {
            w.confirm = Some((super::Confirm::Remodel(map), false));
            w.sounds.push(Ui::Select as u32);
        }
    }
}

/// LAB_101d6ab0 -> FUN_101d6ad0 on Yes: c2s 0x0CB {5, +0x50 *(inf: 0, never set here)*, map}, SE 0002, the list closes.
pub fn confirmed(w: &mut World, map: u16) {
    let [lo, hi] = map.to_le_bytes();
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0CB, body: vec![5, 0, lo, hi] });
    log::info!("remodel: 0x0CB style {map:#x}");
    w.sounds.push(Ui::Select as u32);
    w.remodel = None;
}

pub fn key(w: &mut World, key: egui::Key) {
    let Some(r) = w.remodel.as_mut() else { return };
    match key {
        egui::Key::ArrowDown if r.sel + 1 < ROWS.len() => {
            r.sel += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::ArrowUp if r.sel > 0 => {
            r.sel -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        egui::Key::Enter => pick(w),
        // FUN_101d6b10: closes with SE 0003.
        egui::Key::Escape => {
            w.remodel = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// The list as the modern layer draws it: each row's text and whether it is grey, the cursor.
pub(super) fn look(w: &World, gu: &GameUi) -> Option<(Vec<(String, bool)>, usize)> {
    let r = w.remodel?;
    let rows = ROWS.iter().enumerate().map(|(k, (text, _))| (gu.lobby_msg(0x0c, *text as usize, ""), tint(w, k) == GREY)).collect();
    Some((rows, r.sel))
}

/// The mouse's cursor move onto row `k`. True when it moved.
pub(super) fn set_sel(w: &mut World, k: usize) -> bool {
    let Some(r) = w.remodel.as_mut().filter(|r| r.sel != k && k < ROWS.len()) else { return false };
    r.sel = k;
    true
}

/// The generic list's mouse: hover moves the cursor, a click on the cursor row is Enter.
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool) {
    // The modern layer draws it and takes its mouse.
    if w.modern {
        return;
    }
    let Some(r) = w.remodel else { return };
    let (Some((x, y)), Some(lay)) = (gu.window_xy(WINDOW, sw, sh), gu.layout(WINDOW)) else { return };
    let Some(e) = lay.elements.iter().skip(1).find(|e| {
        let (ex, ey) = (x + e.x as f32, y + e.y as f32);
        p.0 >= ex && p.0 < ex + e.w as f32 && p.1 >= ey && p.1 < ey + e.h as f32
    }) else { return };
    let row = e.id as usize - 1;
    if row >= ROWS.len() {
        return;
    }
    gu.hover.set(true);
    if row != r.sel && (moved || clicked) {
        if let Some(r) = w.remodel.as_mut() {
            r.sel = row;
        }
        w.sounds.push(Ui::Cursor as u32);
    } else if clicked {
        pick(w);
    }
}

/// Slot 1: the layout header's title and help.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if w.remodel.is_none() {
        return;
    }
    let [help, title] = gu.focus_text(WINDOW, 0);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
}

/// FUN_101f6a60: type-3 cells (game font, the row's tint) at column 0 *(inf: at the cell origin, no + 2)*; the
/// windowps 75 cursor bar while focused.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let Some(r) = w.remodel else { return rects };
    let focused = w.confirm.is_none();
    let Some(win) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), Some(r.sel as u16 + 1), focused, &[]) else { return rects };
    rects.push(win.frame);
    let Some((wx, wy)) = gu.window_xy(WINDOW, sw, sh) else { return rects };
    for (k, (text, _)) in ROWS.iter().enumerate() {
        let line = gu.lobby_msg(0x0c, *text as usize, "");
        gu.text(&line, wx + 3.0 + COL, wy + 4.0 + 16.0 * k as f32, 0, tint(w, k).to_le_bytes());
    }
    if focused {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * r.sel as f32, WHITE);
    }
    rects
}
