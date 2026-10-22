//! The in-game Config menu: configwi (FUN_1018f2a0) and its pages, on FFXiMain's option store (gamecfg).
//! Every change is applied at once through the setter (FUN_1018c7b0 and its callbacks); there is no OK or cancel.
//! Events (FUN_10114f40): Up / Down / Right / Left follow the layout links (element bytes 0x17..0x1a), Enter
//! picks, Escape closes the window on top. Sounds "0001" cursor, "0002" Enter, "0003" Escape, "0004" disabled.

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::gamecfg::Values;

/// Under-the-choice markers (FUN_1018f4a0 / FUN_1018f510 and the pages' own).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mark {
    /// windowps 387 at (x + 37, y + 4): two 88-px buttons.
    Wide,
    /// windowps 412 at (x + 37, y + 4): three 64-px buttons.
    Narrow,
    /// windowps 411 at (x, y + 4): the icon type keys.
    Icon,
    /// windowps 483 at (x, y): the auto-disconnect keys.
    Bar,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ctl {
    /// Enter sets id = v; marked while it holds.
    Pick(u16, u32, Mark),
    /// Enter sets bit mask of id on / off; marked while it holds.
    Bit(u16, u32, bool, Mark),
    /// Enter edits id in lo..=hi by step with Left / Right, live; the fill shows (v - lo) / (hi - lo).
    Slider(u16, i32, i32, i32),
    /// Enter opens another window over this one.
    Open(&'static str),
    /// Page code.
    Act(Act),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    /// conf3win Show / Hide: kept by the page, not a config id (c2s 0x0DC on close in retail).
    Anon(bool),
    /// conf6win Default: the yes/no box, then default key assignments.
    ResetKeys,
    /// confyn Yes / No (mode from the opener).
    Yes(bool),
    /// Opens confyn in a mode (0 text colours, 1 log windows).
    AskReset(u8),
    /// Page code in configui2.
    Custom(u8),
}

use Ctl::*;
use Mark::*;

const fn p(c: u16, id: u16, v: u32) -> (u16, Ctl) {
    (c, Pick(id, v, Wide))
}
const fn p3(c: u16, id: u16, v: u32) -> (u16, Ctl) {
    (c, Pick(id, v, Narrow))
}

/// configwi rows: Gameplay, Chat Filters, Font Colors, Windows, Log, Misc. 1..4, Effects, Mouse/Cam., Global.
const CONFIGWI: &[(u16, Ctl)] = &[
    (1, Open("conf2win")),
    (2, Open("cfilter")),
    (3, Open("conftxtc")),
    (4, Open("conf5m")),
    (10, Open("conf11m")),
    (5, Open("conf3win")),
    (6, Open("conf6win")),
    (11, Open("conf12wi")),
    (12, Open("conf13wi")),
    (7, Open("fxfilter")),
    (8, Open("conf7")),
    (9, Open("conf4")),
];

/// Gameplay (FUN_1018fd20): lock-on 7, volumes 9 / 10, gamma 0x37..0x39, autosort 0x9a, sort type 0xca.
const CONF2WIN: &[(u16, Ctl)] = &[
    p(1, 7, 0),
    p(2, 7, 1),
    (3, Slider(9, 0, 100, 1)),
    (4, Slider(10, 0, 100, 1)),
    (5, Slider(0x37, 0, 100, 1)),
    (6, Slider(0x38, 0, 100, 1)),
    (7, Slider(0x39, 0, 100, 1)),
    p(8, 0x9a, 1),
    p(9, 0x9a, 0),
    p(10, 0xca, 0),
    p(11, 0xca, 1),
];

/// Misc. (FUN_10193010): damage display 0x13, shadows 0x3a (High 2, Normal 0, Off 1), weather 0x3b, characters
/// 0x3c (25..50 in the UI), icon type 0x99 bit 2, PC armour 0xa5.
const CONF3WIN: &[(u16, Ctl)] = &[
    p3(1, 0x13, 0),
    p3(2, 0x13, 1),
    p3(5, 0x13, 2),
    (3, Act(Act::Anon(false))),
    (4, Act(Act::Anon(true))),
    p3(6, 0x3a, 2),
    p3(7, 0x3a, 0),
    p3(8, 0x3a, 1),
    p(9, 0x3b, 1),
    p(10, 0x3b, 0),
    (11, Slider(0x3c, 25, 50, 1)),
    (12, Bit(0x99, 1 << 2, true, Icon)),
    (13, Bit(0x99, 1 << 2, false, Icon)),
    p(14, 0xa5, 0),
    p(15, 0xa5, 1),
];

/// Misc. 2 (FUN_10193680): clipping 0x3e, footsteps 0x3f, animation rate 0x40, keyboard 0x42, aspect 0x43, key
/// assignment, macro palette 0x9d / 0x9e.
const CONF6WIN: &[(u16, Ctl)] = &[
    (1, Slider(0x3e, 0, 25, 1)),
    p(2, 0x3f, 0),
    p(3, 0x3f, 1),
    (4, Slider(0x40, 0, 3, 1)),
    p(5, 0x42, 0),
    p(6, 0x42, 1),
    p(11, 0x42, 2),
    (7, Slider(0x43, 0, 5, 1)),
    (8, Open("k1assign")),
    (9, Open("k2assign")),
    (10, Act(Act::ResetKeys)),
    p(12, 0x9d, 0),
    p(13, 0x9d, 1),
    p(14, 0x9e, 0),
    p(15, 0x9e, 1),
];

/// Mouse/Cam. (table 0x1036dfc0): 0x90 mouse reference, 0x8f edge panning, 0x91 chase cam, 0x8c / 0x8b third-person
/// Y / X invert, 0x8e / 0x8d first-person Y / X invert.
const CONF7: &[(u16, Ctl)] = &[
    p(1, 0x90, 0),
    p(2, 0x90, 1),
    p(3, 0x8f, 0),
    p(4, 0x8f, 1),
    p(5, 0x91, 0),
    p(6, 0x91, 1),
    p(7, 0x8c, 0),
    p(8, 0x8c, 1),
    p(9, 0x8b, 0),
    p(10, 0x8b, 1),
    p(11, 0x8e, 0),
    p(12, 0x8e, 1),
    p(13, 0x8d, 0),
    p(14, 0x8d, 1),
];

/// Global (FUN_10194780): language filter 0x18, auto-disconnect minutes 0x41 (-1 off); both PlayOnline-wide.
const CONF4: &[(u16, Ctl)] = &[
    p(1, 0x18, 0),
    p(2, 0x18, 1),
    (3, Pick(0x41, 10, Bar)),
    (4, Pick(0x41, 20, Bar)),
    (5, Pick(0x41, 30, Bar)),
    (6, Pick(0x41, 40, Bar)),
    (7, Pick(0x41, 50, Bar)),
    (8, Pick(0x41, 60, Bar)),
    (9, Pick(0x41, u32::MAX, Bar)),
];

/// Misc. 3 (FUN_10193fe0): weapon effect 0xbe, style lock 0xbf, area display 0xc0, target expressions 0x99 bit 7,
/// party icons 0xc2, icon timers 0xc3, furniture camera collision 0x99 bit 6.
const CONF12WI: &[(u16, Ctl)] = &[
    p(1, 0xbe, 0),
    p(2, 0xbe, 1),
    p(3, 0xbf, 0),
    p(4, 0xbf, 1),
    p(5, 0xc0, 1),
    p(6, 0xc0, 0),
    (7, Bit(0x99, 1 << 7, false, Wide)),
    (8, Bit(0x99, 1 << 7, true, Wide)),
    p(9, 0xc2, 1),
    p(10, 0xc2, 0),
    p(11, 0xc3, 1),
    p(12, 0xc3, 0),
    (13, Bit(0x99, 1 << 6, false, Wide)),
    (14, Bit(0x99, 1 << 6, true, Wide)),
];

/// Misc. 4 (FUN_10194410): software keyboard 0xc9; Compact only in Japanese.
const CONF13WI: &[(u16, Ctl)] = &[p3(1, 0xc9, 0), p3(2, 0xc9, 1), p3(3, 0xc9, 2)];

/// confyn: Yes / No, the cursor starting on No (FUN_10115b70(2)).
const CONFYN: &[(u16, Ctl)] = &[(1, Act(Act::Yes(true))), (2, Act(Act::Yes(false)))];

/// A page's controls by menu name (without the "menu    " prefix).
pub fn page(name: &str) -> &'static [(u16, Ctl)] {
    match name {
        "configwi" => CONFIGWI,
        "conf2win" => CONF2WIN,
        "conf3win" => CONF3WIN,
        "conf6win" => CONF6WIN,
        "conf7" => CONF7,
        "conf4" => CONF4,
        "conf12wi" => CONF12WI,
        "conf13wi" => CONF13WI,
        "confyn" => CONFYN,
        _ => super::configui2::page(name),
    }
}

/// Children that do not take Enter (state 5, drawn with their state-4 sprite).
fn disabled(name: &str, child: u16, sw: f32) -> bool {
    match name {
        // Compact software keyboard: Japanese only (FUN_10194460).
        "conf13wi" => child == 3,
        // Macro palette size: below 1024 wide the full palette does not fit (FUN_10193e50, simplified).
        "conf6win" => child == 12 && sw < 1024.0,
        _ => false,
    }
}

/// lobycwin's title tracks 7..11 need their expansion installed (FUN_10195ef0 / FUN_1023ae80: bits 0, 2, 3, 4, 11).
fn unowned(name: &str, child: u16, owned: u32) -> bool {
    const BITS: [u32; 5] = [0, 2, 3, 4, 11];
    name == "lobycwin" && (7..=11).contains(&child) && owned & (1 << BITS[child as usize - 7]) == 0
}

/// "(4:3)" .. "(5:4)" for 0x43 (FUN_10002cc0, 0x103443f0).
pub const ASPECTS: [(&str, f32); 6] = [("(4:3)", 4.0 / 3.0), ("(15:9)", 15.0 / 9.0), ("(15:10)", 1.5), ("(16:9)", 16.0 / 9.0), ("(16:10)", 1.6), ("(5:4)", 1.25)];

pub struct Win {
    /// Menu name without the prefix ("conf2win").
    pub name: String,
    pub cursor: u16,
    /// Slider being edited: (child, value).
    pub edit: Option<(u16, i32)>,
    /// Added to the layout position (the lobby centres its windows).
    pub offset: (f32, f32),
}

impl Win {
    fn new(name: &str) -> Self {
        let first = page(name).first().map_or(1, |c| c.0);
        // confyn starts on No.
        Win { name: name.to_string(), cursor: if matches!(name, "confyn" | "yesno") { 2 } else { first }, edit: None, offset: (0.0, 0.0) }
    }

    pub fn full(&self) -> String {
        format!("menu    {}", self.name)
    }
}

/// What a step did.
#[derive(Default)]
pub struct Out {
    pub sounds: Vec<Se>,
    /// Ids whose value changed (their effects run).
    pub changed: Vec<u16>,
    pub closed: bool,
    /// Enter on our Settings button.
    pub open_settings: bool,
}

/// Our Settings button under configwi and lobycwin (not retail): a virtual child after the bottom row.
pub const SETTINGS: u16 = 100;

/// The windows with the button: (bottom row whose Down reaches it, the row its Down goes on to).
fn settings_host(name: &str) -> Option<(u16, u16)> {
    match name {
        "configwi" => Some((9, 1)),
        "lobycwin" => Some((4, 5)),
        _ => None,
    }
}

/// The button's rect: under the window's frame, its left edge, 112 x 24.
pub fn settings_rect(gu: &GameUi, w: &Win, sw: f32, sh: f32) -> Option<[f32; 4]> {
    settings_host(&w.name)?;
    let r = gu.window_rect(&w.full(), sw, sh)?;
    let (dx, dy) = ConfigUi::offset(w, sw, sh);
    Some([r[0] + dx, r[1] + dy + r[3] + 2.0, 112.0, 24.0])
}

/// Our text for the button in the help bar.
pub const SETTINGS_HELP: &str = "Window, background and menu resolution, frame limit and the Config tool's settings.";

/// The open Config windows, bottom first, and page state that is not a config id.
#[derive(Default)]
pub struct ConfigUi {
    pub stack: Vec<Win>,
    /// Character information hidden (conf3win Show / Hide).
    pub anon: bool,
    /// confyn's mode (0 text colours, 1 log windows).
    pub yn_mode: u8,
    /// Page-specific state of the other pages (configui2).
    pub extra: super::configui2::Extra,
    /// Installed-content mask (DAT_10444110 + 0x10, not the server's) for lobycwin's title tracks.
    pub owned: u32,
}

impl ConfigUi {
    pub fn open(name: &str) -> Self {
        let mut c = ConfigUi::default();
        c.push(name);
        c
    }

    pub fn push(&mut self, name: &str) {
        self.stack.push(Win::new(name));
        super::configui2::opened(self, name);
    }

    pub fn top(&self) -> Option<&Win> {
        self.stack.last()
    }

    fn ctl(name: &str, child: u16) -> Option<Ctl> {
        page(name).iter().find(|c| c.0 == child).map(|c| c.1)
    }

    fn set(v: &mut Values, out: &mut Out, id: u16, x: u32) {
        if v.set(id, x) {
            out.changed.push(id);
            super::configui2::linked(v, id, out);
        }
    }

    /// A window's layout position on screen, with its offset (-1, -1 = centred as the lobby's windows).
    pub fn win_xy(gu: &GameUi, w: &Win, sw: f32, sh: f32) -> Option<(f32, f32)> {
        let (x, y) = gu.window_xy(&w.full(), sw, sh)?;
        let (dx, dy) = Self::offset(w, sw, sh);
        Some((x + dx, y + dy))
    }

    pub fn offset(w: &Win, sw: f32, sh: f32) -> (f32, f32) {
        if w.offset == (-1.0, -1.0) { (((sw - 512.0) / 2.0).trunc(), ((sh - 448.0) / 2.0).trunc()) } else { w.offset }
    }

    /// A key on the top window.
    pub fn key(&mut self, gu: &GameUi, v: &mut Values, key: egui::Key, sw: f32) -> Out {
        let mut out = Out::default();
        if super::configui2::key(self, gu, v, key, &mut out) {
            return out;
        }
        let Some(w) = self.stack.last_mut() else { return out };
        // Slider editing takes every key (FUN_1018fee0 and the same code on each page).
        if let Some((child, val)) = w.edit {
            if let Some(Slider(id, lo, hi, step)) = Self::ctl(&w.name, child) {
                let nv = match key {
                    egui::Key::ArrowRight => (val + step).min(hi),
                    egui::Key::ArrowLeft => (val - step).max(lo),
                    _ => val,
                };
                if nv != val {
                    w.edit = Some((child, nv));
                    Self::set(v, &mut out, id, nv as u32);
                }
                if matches!(key, egui::Key::Enter | egui::Key::Escape) {
                    Self::set(v, &mut out, id, nv as u32);
                    w.edit = None;
                    out.sounds.push(if key == egui::Key::Enter { Se::Select } else { Se::Cancel });
                }
            } else {
                w.edit = None;
            }
            return out;
        }
        if let Some((bottom, first)) = settings_host(&w.name) {
            let to = match key {
                egui::Key::ArrowDown if w.cursor == bottom => Some(SETTINGS),
                egui::Key::ArrowDown if w.cursor == SETTINGS => Some(first),
                egui::Key::ArrowUp if w.cursor == SETTINGS => Some(bottom),
                egui::Key::Enter if w.cursor == SETTINGS => {
                    out.sounds.push(Se::Select);
                    out.open_settings = true;
                    return out;
                }
                _ if w.cursor == SETTINGS && key != egui::Key::Escape => return out,
                _ => None,
            };
            if let Some(to) = to {
                w.cursor = to;
                out.sounds.push(Se::Cursor);
                return out;
            }
        }
        let link = match key {
            egui::Key::ArrowUp => Some(2),
            egui::Key::ArrowDown => Some(3),
            egui::Key::ArrowRight => Some(4),
            egui::Key::ArrowLeft => Some(5),
            _ => None,
        };
        if let Some(k) = link {
            let to = gu.menu_links(&w.full(), w.cursor).map_or(w.cursor, |l| l[k] as u16);
            if to != w.cursor && to != 0 && Self::ctl(&w.name, to).is_some() {
                w.cursor = to;
                out.sounds.push(Se::Cursor);
            }
            return out;
        }
        match key {
            egui::Key::Enter => {
                let (name, child) = (w.name.clone(), w.cursor);
                self.enter(v, &name, child, sw, &mut out);
            }
            egui::Key::Escape => {
                out.sounds.push(Se::Cancel);
                self.close_top(&mut out);
            }
            _ => {}
        }
        out
    }

    fn close_top(&mut self, out: &mut Out) {
        if let Some(w) = self.stack.pop() {
            super::configui2::closed(self, &w.name, out);
        }
        out.closed = self.stack.is_empty();
    }

    /// Enter on `child` of the top window `name`.
    fn enter(&mut self, v: &mut Values, name: &str, child: u16, sw: f32, out: &mut Out) {
        if disabled(name, child, sw) || unowned(name, child, self.owned) {
            out.sounds.push(Se::Error);
            return;
        }
        let Some(c) = Self::ctl(name, child) else { return };
        out.sounds.push(Se::Select);
        match c {
            Pick(id, x, _) => Self::set(v, out, id, x),
            Bit(id, mask, on, _) => {
                let cur = v.get(id);
                Self::set(v, out, id, if on { cur | mask } else { cur & !mask });
            }
            Slider(id, lo, hi, _) => {
                if let Some(w) = self.stack.last_mut() {
                    w.edit = Some((child, v.get_i(id).clamp(lo, hi)));
                }
            }
            Open(next) => self.push(next),
            Act(Act::Anon(h)) => self.anon = h,
            Act(Act::ResetKeys) => {
                self.extra.ask_keys = Some(false);
                self.push("yesno");
            }
            Act(Act::Custom(k)) => super::configui2::custom(self, v, name, k, out),
            Act(Act::AskReset(mode)) => {
                self.yn_mode = mode;
                self.push("confyn");
            }
            Act(Act::Yes(yes)) => {
                if yes {
                    // FUN_1018c7f0(mode + 1): FUN_1018ed00 text colours / FUN_1018ed90 log windows.
                    let ids: Vec<u16> = if self.yn_mode == 0 { (0x1b..=0x31).chain([0x9b, 0xb0, 0xb3]).collect() } else { (0xb9..=0xbc).collect() };
                    for id in ids {
                        let d = crate::gamecfg::def(id).map_or(0, |d| d.default);
                        Self::set(v, out, id, d);
                    }
                }
                self.close_top(out);
            }
        }
    }

    /// The pointer: hover moves the top window's cursor, a click is Enter; on a slider the click sets the value
    /// under the pointer (not retail: retail only steps with the keys).
    pub fn pointer(&mut self, gu: &GameUi, v: &mut Values, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32) -> Out {
        let mut out = Out::default();
        if super::configui2::pointer(self, gu, v, p, moved, clicked, sw, sh, &mut out) {
            return out;
        }
        let Some(w) = self.stack.last() else { return out };
        if let Some(r) = settings_rect(gu, w, sw, sh)
            && p.0 >= r[0]
            && p.1 >= r[1]
            && p.0 < r[0] + r[2]
            && p.1 < r[1] + r[3]
        {
            if (moved || clicked) && w.cursor != SETTINGS {
                out.sounds.push(Se::Cursor);
                if let Some(w) = self.stack.last_mut() {
                    w.cursor = SETTINGS;
                }
            }
            if clicked {
                out.sounds.push(Se::Select);
                out.open_settings = true;
            }
            return out;
        }
        let Some(m) = gu.menu_def(&w.full()) else { return out };
        let Some((x, y)) = Self::win_xy(gu, w, sw, sh) else { return out };
        let hit = m.elements.iter().skip(1).find(|c| {
            let r = [x + c.x as f32, y + c.y as f32, c.w as f32, c.h as f32];
            p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3] && Self::ctl(&w.name, c.id).is_some()
        });
        let Some(c) = hit else { return out };
        let (id, cx, cw) = (c.id, x + c.x as f32, c.w as f32);
        let name = w.name.clone();
        let editing = w.edit.is_some();
        if (moved || clicked) && w.cursor != id && !editing {
            out.sounds.push(Se::Cursor);
            if let Some(w) = self.stack.last_mut() {
                w.cursor = id;
            }
        }
        if !clicked {
            return out;
        }
        match Self::ctl(&name, id) {
            Some(Slider(cid, lo, hi, step)) => {
                let f = ((p.0 - cx) / cw).clamp(0.0, 1.0);
                let val = lo + ((f * (hi - lo) as f32 / step as f32).round() as i32) * step;
                Self::set(v, &mut out, cid, val as u32);
                if let Some(w) = self.stack.last_mut() {
                    w.edit = None;
                    w.cursor = id;
                }
            }
            Some(_) => self.enter(v, &name, id, sw, &mut out),
            None => {}
        }
        out
    }

    /// The bars' slot 1 text source: the top window and its cursor child.
    pub fn focus(&self) -> Option<(String, u16)> {
        self.stack.last().map(|w| (w.full(), w.cursor))
    }

    /// Draws every open window, bottom first; returns their frame rects (mouse blockers).
    pub fn draw(&self, gu: &mut GameUi, v: &Values, sw: f32, sh: f32) -> Vec<[f32; 4]> {
        let mut rects = Vec::new();
        let n = self.stack.len();
        for (i, w) in self.stack.iter().enumerate() {
            let top = i + 1 == n;
            // A window under a later one at the same place (k1assign over conf6win, textcol3 over textcol2) is not
            // drawn *(inf)*.
            let at = gu.window_xy(&w.full(), sw, sh);
            if at.is_some() && self.stack[i + 1..].iter().any(|u| gu.window_xy(&u.full(), sw, sh) == at) {
                continue;
            }
            if super::configui2::draw(self, w, top, gu, v, sw, sh, &mut rects) {
                continue;
            }
            draw_page(self, w, top, gu, v, sw, sh);
            if let Some(r) = settings_rect(gu, w, sw, sh) {
                // A frame with a blank config button and the game font (our addition).
                gu.frame(r[0], r[1], r[2], r[3]);
                let on = top && w.cursor == SETTINGS;
                let (bx, by) = (r[0] + 16.0, r[1] + 4.0);
                gu.sprite_cells("menu    windowps", 105, 0..2, bx - on as i32 as f32, by - on as i32 as f32, if on { 0x8080_5020 } else { 0x8080_8080 });
                gu.text("Settings", bx + 44.0, by, 1, [0x80; 4]);
                if on {
                    gu.cursor(bx, by);
                }
                rects.push(r);
            }
            let (dx, dy) = Self::offset(w, sw, sh);
            rects.extend(gu.window_rect(&w.full(), sw, sh).map(|r| [r[0] + dx, r[1] + dy, r[2], r[3]]));
        }
        rects
    }
}

/// A layout page as FUN_101144b0 draws it plus the config handler's markers and slider fills (FUN_1018f4a0,
/// FUN_1018f510, FUN_1018f600).
pub fn draw_page(ui: &ConfigUi, w: &Win, top: bool, gu: &mut GameUi, v: &Values, sw: f32, sh: f32) {
    let name = w.full();
    let (dx, dy) = ConfigUi::offset(w, sw, sh);
    let Some((x, y)) = gu.window_off(&name, sw, sh, dx, dy) else { return };
    let kids: Vec<(u16, f32, f32, f32, Option<(u16, String)>, Option<(u16, String)>)> = gu
        .menu_def(&name)
        .map(|m| {
            m.elements
                .iter()
                .skip(1)
                .map(|c| {
                    let r = |k: u16| c.refs.iter().find(|r| r.kind == k).map(|r| (r.index, r.lib.clone()));
                    (c.id, x + c.x as f32, y + c.y as f32, c.w as f32, r(0), r(4))
                })
                .collect()
        })
        .unwrap_or_default();
    for (id, cx, cy, _, normal, off) in &kids {
        // The cursor child stays highlighted under the window on top (no registry flag 0x80000); the hand only on top.
        let on = *id == w.cursor && w.edit.is_none();
        let dis = disabled(&w.name, *id, sw) || unowned(&w.name, *id, ui.owned);
        let sprite = if dis { off.clone().or(normal.clone()) } else { normal.clone() };
        if let Some((k, lib)) = sprite {
            let d = if on { -1.0 } else { 0.0 };
            gu.sprite(&lib, k as usize, cx + d, cy + d, if on { 0x8080_5020 } else { 0x8080_8080 });
        }
    }
    for (id, cx, cy, cw, _, _) in &kids {
        let Some(c) = ConfigUi::ctl(&w.name, *id) else { continue };
        let marked = |m: Mark, gu: &mut GameUi| match m {
            Wide => gu.sprite("menu    windowps", 387, cx + 37.0, cy + 4.0, 0x8080_8080),
            Narrow => gu.sprite("menu    windowps", 412, cx + 37.0, cy + 4.0, 0x8080_8080),
            Icon => gu.sprite("menu    windowps", 411, *cx, cy + 4.0, 0x8080_8080),
            Bar => gu.sprite("menu    windowps", 483, *cx, *cy, 0x8080_8080),
        };
        match c {
            Pick(cid, x, m) if v.get(cid) == x => {
                marked(m, gu);
            }
            Bit(cid, mask, on, m) if (v.get(cid) & mask != 0) == on => {
                marked(m, gu);
            }
            Act(Act::Anon(h)) if ui.anon == h => {
                marked(Wide, gu);
            }
            Slider(cid, lo, hi, _) => {
                let editing = w.edit.filter(|e| e.0 == *id);
                let val = editing.map_or(v.get_i(cid), |e| e.1);
                // Max lines fill by value / 20 (FUN_10191f20), the others from their low end.
                let base = if matches!(cid, 0x0f | 0xb7) { 0 } else { lo };
                let f = (val - base) as f32 / (hi - base).max(1) as f32;
                if f > 0.0 {
                    gu.sprite_scaled("menu    windowps", 384, *cx, *cy, f.min(1.0), 1.0, if editing.is_some() { 0xa0a0_a0a0 } else { 0x8080_8080 });
                }
                // The aspect ratio's text under its slider (FUN_10002cc0) in fontshp.
                if cid == 0x43 || cid == 0xab {
                    let t = ASPECTS.get(val.clamp(0, 5) as usize).map_or("", |a| a.0);
                    gu.shape_text(t, cx + cw - 20.0, cy + 17.0, 0x8080_8080, false);
                }
            }
            _ => {}
        }
    }
    if top
        && w.edit.is_none()
        && w.cursor != SETTINGS
        && let Some((_, cx, cy, ..)) = kids.iter().find(|k| k.0 == w.cursor)
    {
        gu.cursor(*cx, *cy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_and_sliders_follow_the_store() {
        let mut v = Values::default();
        let mut ui = ConfigUi::open("conf2win");
        let mut out = Out::default();
        // Child 2 = lock-on off (7 = 1).
        ui.enter(&mut v, "conf2win", 2, 1280.0, &mut out);
        assert_eq!(v.get(7), 1);
        assert_eq!(out.changed, vec![7]);
        // Slider: Enter starts editing at the stored value.
        ui.enter(&mut v, "conf2win", 3, 1280.0, &mut out);
        assert_eq!(ui.top().unwrap().edit, Some((3, 100)));
        // Bits keep the other bits.
        v.set(0x99, 1 << 7);
        ui.enter(&mut v, "conf3win", 12, 1280.0, &mut out);
        assert_eq!(v.get(0x99), (1 << 7) | (1 << 2));
        // Disabled children only buzz.
        let mut out = Out::default();
        ui.enter(&mut v, "conf13wi", 3, 1280.0, &mut out);
        assert!(matches!(out.sounds.as_slice(), [Se::Error]));
        assert_eq!(v.get(0xc9), 0);
    }

    #[test]
    fn colour_reset_via_confyn() {
        let mut v = Values::default();
        v.set(0x1c, 0x8011_2233);
        let mut ui = ConfigUi::open("configwi");
        ui.yn_mode = 0;
        ui.push("confyn");
        let mut out = Out::default();
        ui.enter(&mut v, "confyn", 1, 1280.0, &mut out);
        assert_eq!(v.get(0x1c), 0x80a0_5040);
        assert_eq!(ui.stack.len(), 1);
    }
}
