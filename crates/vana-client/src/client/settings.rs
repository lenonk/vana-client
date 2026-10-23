//! Our Settings and Extras pages (not retail). Settings holds what retail set in the Windows "FINAL FANTASY XI
//! Config" tool before starting (registry names 0000..0043, read by FUN_10016c30) plus our own display options;
//! Extras holds the gameplay additions (the add-ons' features), each off until turned on. Both are drawn as the
//! Config pages are (FUN_101144b0 over windowps 465: page backing, header strip, bullets, fontshp text, the
//! buttons, markers and sliders of conf2win / conf3win), at the pages' place in game and centred in the lobby.

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::config::{Config, FRAME_LIMITS};

/// What the pages edit (merged back into the config by the app).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub tool: std::collections::BTreeMap<String, u32>,
    pub frame_limit: u32,
    pub anisotropy: u16,
    pub party_tp: bool,
    pub map_overlay: bool,
    pub map_opacity: u32,
    pub synth_amount: bool,
    pub stamp_all_lines: bool,
    pub music: u32,
    pub effects: u32,
    pub retail_aspect: bool,
    pub monitor: String,
    pub gpu: String,
    pub cursor_size: u32,
}

impl Settings {
    pub fn of(c: &Config) -> Self {
        let pct = |v: f32| (v.clamp(0.0, 1.0) * 100.0).round() as u32;
        Self {
            tool: c.tool.clone(),
            frame_limit: c.frame_limit,
            anisotropy: c.anisotropy,
            party_tp: c.party_tp,
            map_overlay: c.map_overlay,
            map_opacity: pct(c.map_overlay_opacity),
            synth_amount: c.synth_amount,
            stamp_all_lines: c.stamp_all_lines,
            music: pct(c.music_volume),
            effects: pct(c.effects_volume),
            retail_aspect: c.retail_aspect,
            monitor: c.monitor.clone(),
            gpu: c.gpu.clone(),
            cursor_size: c.cursor_size,
        }
    }

    pub fn store(&self, c: &mut Config) {
        c.monitor = self.monitor.clone();
        c.gpu = self.gpu.clone();
        c.tool = self.tool.clone();
        c.frame_limit = self.frame_limit;
        c.anisotropy = self.anisotropy;
        c.party_tp = self.party_tp;
        c.map_overlay = self.map_overlay;
        c.map_overlay_opacity = self.map_opacity as f32 / 100.0;
        c.synth_amount = self.synth_amount;
        c.stamp_all_lines = self.stamp_all_lines;
        c.music_volume = self.music as f32 / 100.0;
        c.effects_volume = self.effects as f32 / 100.0;
        c.retail_aspect = self.retail_aspect;
        c.cursor_size = self.cursor_size;
    }
}

/// Which page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Settings,
    Extras,
}

/// When a change shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum When {
    Now,
    NextZone,
    Restart,
    /// Kept for the feature we do not have yet.
    Stored,
}

#[derive(Clone, Copy, Debug)]
enum Val {
    Tool(&'static str),
    /// Width / height names; the choices are sizes.
    Size(&'static str, &'static str, Sizes),
    Gamma,
    FrameLimit,
    Aniso,
    PartyTp,
    MapOverlay,
    MapOpacity,
    SynthAmount,
    StampAll,
    Music,
    Effects,
    RetailAspect,
    CursorSize,
    /// The monitor for full screen, from the list the app passes in.
    Monitor,
    /// The graphics adapter ("Default" then the app's list).
    Gpu,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sizes {
    Window,
    Background,
    Ui,
}

struct Row {
    label: &'static str,
    help: &'static str,
    val: Val,
    /// Named choices (value, label); empty for sizes and numbers. Up to three are buttons, more a slider.
    choices: &'static [(u32, &'static str)],
    when: When,
}

const OFF_ON: &[(u32, &str)] = &[(1, "ON"), (0, "OFF")];

const fn row(label: &'static str, help: &'static str, val: Val, choices: &'static [(u32, &'static str)], when: When) -> Row {
    Row { label, help, val, choices, when }
}

const TABS: [&str; 7] = ["General", "Screen", "Textures", "Effects", "Sound", "Misc.", "Linux"];

fn rows(kind: Kind, tab: usize) -> &'static [Row] {
    use Val::*;
    use When::*;
    const GENERAL: &[Row] = &[
        row("Window Mode", "0034: full screen, a window, or borderless at the window size or the desktop's.", Tool("0034"), &[(0, "Full Screen"), (1, "Windowed"), (2, "Borderless"), (3, "Desktop")], Now),
        row("MIP Mapping", "0000: lower-resolution textures on distant objects.", Tool("0000"), OFF_ON, NextZone),
        row("Gamma Base", "0028: base gamma, added to the Gameplay page's R / G / B.", Gamma, &[], Now),
    ];
    const SCREEN: &[Row] = &[
        row("Resolution", "0001 / 0002: the window's size.", Size("0001", "0002", Sizes::Window), &[], Now),
        row("Monitor", "The monitor full screen and borderless use (not retail).", Monitor, &[], Now),
        row("Graphics Card", "The GPU to draw with; Default is the high-performance one (not retail).", Gpu, &[], Restart),
        row("Background", "0003 / 0004: the 3D scene's resolution, scaled to the window.", Size("0003", "0004", Sizes::Background), &[], Now),
        row("Menu Size", "0037 / 0038: the resolution menus and text are laid out at, then stretched.", Size("0037", "0038", Sizes::Ui), &[], Now),
        row("3D Aspect", "Window: the window's shape. Config: retail's ratio from Misc. 2 / the lobby Config.", RetailAspect, &[(0, "Window"), (1, "Config")], Now),
    ];
    const TEXTURES: &[Row] = &[
        row("Textures", "0018: compressed (BC1 / BC2) or uncompressed textures.", Tool("0018"), &[(0, "High"), (1, "Low"), (2, "Full")], NextZone),
        row("2-D Mapping", "0019: on-screen map images.", Tool("0019"), &[(0, "Compressed"), (1, "Full")], Stored),
        row("Fonts", "0036: the font texture quality.", Tool("0036"), &[(0, "Compressed"), (1, "Full"), (2, "High")], Restart),
    ];
    const EFFECTS: &[Row] = &[
        row("Environment Animation", "0011: environment (vegetation) animation.", Tool("0011"), &[(0, "Off"), (1, "Normal"), (2, "Smooth")], Stored),
        row("Bump Mapping", "0017: enhanced shading.", Tool("0017"), OFF_ON, Stored),
        row("Graphics Stabilization", "0040: graphics stabilization for some cards.", Tool("0040"), OFF_ON, Stored),
        row("3D LCD Mode", "0030: interleaved stereo for 3D LCD screens.", Tool("0030"), OFF_ON, Stored),
    ];
    const SOUND: &[Row] = &[
        row("Sound", "0007: sound on or off.", Tool("0007"), OFF_ON, Now),
        row("Sound Effect Channels", "0029: sound effects playing at once.", Tool("0029"), &[(12, "12"), (13, "13"), (14, "14"), (15, "15"), (16, "16"), (17, "17"), (18, "18"), (19, "19"), (20, "20")], Now),
        row("Sound in Background", "0035: keep playing sound while the window is not focused.", Tool("0035"), OFF_ON, Now),
        row("Music Volume", "Our music level, under the Gameplay page's.", Music, &[], Now),
        row("Sound Effects Volume", "Our sound effects level, under the Gameplay page's.", Effects, &[], Now),
    ];
    const MISC: &[Row] = &[
        row("Hardware Mouse", "0021: the game folder's cursor files (.ani) instead of the cursor the game draws.", Tool("0021"), OFF_ON, Now),
        row("Cursor Size", "The drawn cursor's size (Hardware Mouse off); Auto grows it with the window (not retail).", CursorSize, &[(0, "Auto"), (100, "1x"), (150, "1.5x"), (200, "2x"), (300, "3x"), (400, "4x")], Now),
        row("Opening Movie", "0022: play the opening movie on start.", Tool("0022"), OFF_ON, Stored),
        row("Simple Character Creation", "0023: character creation without 3D models.", Tool("0023"), OFF_ON, Stored),
        row("Screenshot Size", "0043: screenshots at the screen size.", Tool("0043"), OFF_ON, Stored),
    ];
    const LINUX: &[Row] = &[
        row("Frame Limit", "Frames drawn a second; game time stays at 60 ticks.", FrameLimit, &[], Now),
        row("Anisotropic Filtering", "Anisotropic texture filtering.", Aniso, &[(1, "Off"), (2, "2x"), (4, "4x"), (8, "8x"), (16, "16x")], NextZone),
        row("Party TP", "Show TP in the party window (retail config 0xae, off there).", PartyTp, OFF_ON, Now),
    ];
    const EXTRAS: &[Row] = &[
        row("Map Overlay", "Ctrl / Alt + O shows the zone map over the screen, as the map add-ons do.", MapOverlay, OFF_ON, Now),
        row("Map Overlay Opacity", "How opaque the map overlay is.", MapOpacity, &[], Now),
        row("Synthesis Amount", "After Synthesize, ask how many to make and make them one after another.", SynthAmount, OFF_ON, Now),
        row("Timestamps on All Lines", "Every log line gets the time, not only chat (the format of /timestamp; seconds while it is off).", StampAll, OFF_ON, Now),
    ];
    match kind {
        Kind::Extras => EXTRAS,
        Kind::Settings => [GENERAL, SCREEN, TEXTURES, EFFECTS, SOUND, MISC, LINUX][tab.min(6)],
    }
}

const RESOLUTIONS: [[u32; 2]; 17] = [
    [640, 480], [800, 600], [1024, 768], [1280, 720], [1280, 800], [1280, 1024], [1366, 768], [1440, 900], [1600, 900], [1680, 1050],
    [1920, 1080], [1920, 1200], [2560, 1080], [2560, 1440], [2560, 1600], [3440, 1440], [3840, 2160],
];

/// The Config pages' size (conf2win and its siblings, 366 x 296 at (16, 48)).
const W: f32 = 366.0;
const H: f32 = 296.0;
/// A row's pitch: its heading, then its buttons or slider under it.
const PITCH: f32 = 34.0;
/// Button columns as conf2win (two 88-px) and conf3win (three 64-px) place them; a slider at the buttons' left.
const WIDE: [f32; 2] = [78.0, 190.0];
const NARROW: [f32; 3] = [78.0, 174.0, 270.0];
const SLIDER_X: f32 = 88.0;
const SLIDER_W: f32 = 192.0;
/// Tabs: two rows of four 88-px buttons.
const TAB_X: f32 = 6.0;
const TAB_PITCH: f32 = 89.0;

pub struct SettingsUi {
    pub kind: Kind,
    pub s: Settings,
    /// Monitors as (name, "name  W x H") and GPU names, set by the app after opening.
    pub monitors: Vec<(String, String)>,
    pub gpus: Vec<String>,
    /// The window's pixels when opened (size lists).
    window: [u32; 2],
    /// Drawn where the in-game Config pages are (else centred, over the lobby).
    pub in_game: bool,
    tab: usize,
    /// 0 = the tab row (Settings), then the rows; the column within the row.
    row: usize,
    col: usize,
    /// A slider being edited (Enter, then Left / Right, as the Config pages' sliders).
    edit: bool,
    pub sounds: Vec<Se>,
    /// Changed since the app last took it.
    pub dirty: bool,
    /// The pointer is over a control (the hit cursor).
    pub hover: bool,
}

/// What a row's controls are.
enum Ctl {
    Buttons(usize),
    Slider,
}

impl SettingsUi {
    pub fn open(c: &Config, window: [u32; 2]) -> Self {
        Self::open_kind(c, window, Kind::Settings)
    }

    pub fn open_kind(c: &Config, window: [u32; 2], kind: Kind) -> Self {
        let mut s = Self { kind, s: Settings::of(c), monitors: Vec::new(), gpus: Vec::new(), window, in_game: false, tab: 0, row: 1, col: 0, edit: false, sounds: Vec::new(), dirty: false, hover: false };
        s.col = s.marked_col();
        s
    }

    fn tabs(&self) -> bool {
        self.kind == Kind::Settings
    }

    fn rows(&self) -> &'static [Row] {
        rows(self.kind, self.tab)
    }

    fn ctl(r: &Row) -> Ctl {
        match r.val {
            Val::Size(..) | Val::Gamma | Val::FrameLimit | Val::Monitor | Val::Gpu | Val::Music | Val::Effects | Val::MapOpacity => Ctl::Slider,
            _ if r.choices.len() <= 3 => Ctl::Buttons(r.choices.len()),
            _ => Ctl::Slider,
        }
    }

    fn sizes(&self, k: Sizes) -> Vec<[u32; 2]> {
        let [w, h] = self.window;
        let scaled = |f: f32| [(w as f32 * f).round() as u32, (h as f32 * f).round() as u32];
        match k {
            Sizes::Window => RESOLUTIONS.to_vec(),
            // 0 = the window's; then scales of it and the tool's squares.
            Sizes::Background => {
                let mut v = vec![[0, 0], scaled(0.5), scaled(0.75), scaled(1.5), scaled(2.0)];
                v.extend([[512, 512], [1024, 1024], [2048, 2048], [4096, 4096]]);
                v
            }
            // The v5 tool: the window, then 95 %, 90 % ... while the height stays 512 or more.
            Sizes::Ui => {
                let mut v = vec![[0, 0]];
                let mut f = 0.95;
                while h as f32 * f >= 512.0 && f > 0.3 {
                    v.push(scaled(f));
                    f -= 0.05;
                }
                v
            }
        }
    }

    fn get(&self, v: Val) -> u32 {
        match v {
            Val::Tool(n) => self.tool_get(n),
            Val::Size(..) | Val::Gamma | Val::Monitor | Val::Gpu => 0,
            Val::FrameLimit => self.s.frame_limit,
            Val::Aniso => self.s.anisotropy as u32,
            Val::PartyTp => self.s.party_tp as u32,
            Val::MapOverlay => self.s.map_overlay as u32,
            Val::MapOpacity => self.s.map_opacity,
            Val::SynthAmount => self.s.synth_amount as u32,
            Val::StampAll => self.s.stamp_all_lines as u32,
            Val::Music => self.s.music,
            Val::Effects => self.s.effects,
            Val::RetailAspect => self.s.retail_aspect as u32,
            Val::CursorSize => self.s.cursor_size,
        }
    }

    fn set(&mut self, v: Val, x: u32) {
        match v {
            Val::Tool(n) => self.tool_set(n, x),
            Val::Aniso => self.s.anisotropy = x as u16,
            Val::PartyTp => self.s.party_tp = x != 0,
            Val::MapOverlay => self.s.map_overlay = x != 0,
            Val::SynthAmount => self.s.synth_amount = x != 0,
            Val::StampAll => self.s.stamp_all_lines = x != 0,
            Val::RetailAspect => self.s.retail_aspect = x != 0,
            Val::CursorSize => self.s.cursor_size = x,
            _ => {}
        }
    }

    fn tool_get(&self, n: &str) -> u32 {
        self.s.tool.get(n).copied().or_else(|| crate::config::TOOL_DEFAULTS.iter().find(|d| d.0 == n).map(|d| d.1)).unwrap_or(0)
    }

    fn tool_set(&mut self, n: &str, x: u32) {
        if crate::config::TOOL_DEFAULTS.iter().any(|d| d.0 == n && d.1 == x) {
            self.s.tool.remove(n);
        } else {
            self.s.tool.insert(n.to_string(), x);
        }
    }

    /// A slider row's stops and the one it is at.
    fn stops(&self, r: &Row) -> (usize, usize) {
        let pos = |list: &[u32], cur: u32| list.iter().position(|&x| x == cur).unwrap_or(0);
        match r.val {
            Val::Size(a, b, k) => {
                let list = self.sizes(k);
                let cur = [self.tool_get(a), self.tool_get(b)];
                (list.len(), list.iter().position(|&x| x == cur).unwrap_or(0))
            }
            Val::Gamma => (21, ((f32::from_bits(self.tool_get("0028")) + 1.0) * 10.0).round().clamp(0.0, 20.0) as usize),
            Val::FrameLimit => (FRAME_LIMITS.len(), pos(&FRAME_LIMITS, self.s.frame_limit)),
            Val::Monitor => (self.monitors.len().max(1), self.monitors.iter().position(|m| m.0 == self.s.monitor).unwrap_or(0)),
            Val::Gpu => (self.gpus.len() + 1, self.gpus.iter().position(|g| *g == self.s.gpu).map_or(0, |i| i + 1)),
            Val::Music => (21, (self.s.music / 5) as usize),
            Val::Effects => (21, (self.s.effects / 5) as usize),
            Val::MapOpacity => (19, (self.s.map_opacity.clamp(10, 100) / 5 - 2) as usize),
            v => {
                let list: Vec<u32> = r.choices.iter().map(|c| c.0).collect();
                (list.len(), pos(&list, self.get(v)))
            }
        }
    }

    /// Moves a slider row to stop `i`.
    fn set_stop(&mut self, r: &Row, i: usize) {
        let before = self.s.clone();
        match r.val {
            Val::Size(a, b, k) => {
                let list = self.sizes(k);
                let [x, y] = list[i.min(list.len() - 1)];
                self.tool_set(a, x);
                self.tool_set(b, y);
            }
            Val::Gamma => {
                let g = ((i.min(20) as f32 - 10.0) / 10.0 * 10.0).round() / 10.0;
                self.tool_set("0028", if g == 0.0 { 0 } else { g.to_bits() });
            }
            Val::FrameLimit => self.s.frame_limit = FRAME_LIMITS[i.min(FRAME_LIMITS.len() - 1)],
            Val::Monitor => {
                if let Some(m) = self.monitors.get(i) {
                    self.s.monitor = m.0.clone();
                }
            }
            Val::Gpu => self.s.gpu = if i == 0 { String::new() } else { self.gpus.get(i - 1).cloned().unwrap_or_default() },
            Val::Music => self.s.music = (i as u32 * 5).min(100),
            Val::Effects => self.s.effects = (i as u32 * 5).min(100),
            Val::MapOpacity => self.s.map_opacity = ((i as u32 + 2) * 5).clamp(10, 100),
            v => {
                if let Some(c) = r.choices.get(i) {
                    self.set(v, c.0);
                }
            }
        }
        if self.s != before {
            self.dirty = true;
        }
    }

    /// The row's value as a slider shows it.
    fn shown(&self, r: &Row) -> String {
        match r.val {
            Val::Size(a, b, _) => match [self.tool_get(a), self.tool_get(b)] {
                [0, _] | [_, 0] => "Window".into(),
                [x, y] => format!("{x}x{y}"),
            },
            Val::Gamma => format!("{:+.1}", f32::from_bits(self.tool_get("0028"))),
            Val::FrameLimit if self.s.frame_limit == 0 => "Unlimited".into(),
            Val::FrameLimit => format!("{}", self.s.frame_limit),
            Val::Music => format!("{}%", self.s.music),
            Val::Effects => format!("{}%", self.s.effects),
            Val::MapOpacity => format!("{}%", self.s.map_opacity),
            Val::Monitor => self.monitors.iter().find(|m| m.0 == self.s.monitor).or(self.monitors.first()).map_or_else(|| "Default".into(), |m| m.0.clone()),
            Val::Gpu if self.s.gpu.is_empty() => "Default".into(),
            Val::Gpu => self.s.gpu.clone(),
            v => {
                let x = self.get(v);
                r.choices.iter().find(|c| c.0 == x).map_or(x.to_string(), |c| c.1.to_string())
            }
        }
    }

    /// The column of a buttons row whose choice holds (the cursor's start on it).
    fn marked_col(&self) -> usize {
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i)) else { return self.tab };
        match Self::ctl(r) {
            Ctl::Buttons(_) => r.choices.iter().position(|c| c.0 == self.get(r.val)).unwrap_or(0),
            Ctl::Slider => 0,
        }
    }

    fn cols(&self, row: usize) -> usize {
        match row.checked_sub(1).and_then(|i| self.rows().get(i)) {
            None => TABS.len(),
            Some(r) => match Self::ctl(r) {
                Ctl::Buttons(n) => n,
                Ctl::Slider => 1,
            },
        }
    }

    fn first_row(&self) -> usize {
        if self.tabs() { 0 } else { 1 }
    }

    /// Enter on the cursor's control.
    fn enter(&mut self) {
        if self.row == 0 {
            if self.tab != self.col {
                self.tab = self.col;
            }
            self.sounds.push(Se::Select);
            return;
        }
        let Some(r) = self.rows().get(self.row - 1) else { return };
        match Self::ctl(r) {
            Ctl::Buttons(_) => {
                let before = self.s.clone();
                if let Some(c) = r.choices.get(self.col) {
                    self.set(r.val, c.0);
                }
                self.dirty |= self.s != before;
                self.sounds.push(Se::Select);
            }
            Ctl::Slider => {
                self.edit = true;
                self.sounds.push(Se::Select);
            }
        }
    }

    /// A key; returns true when the page closes (Escape).
    pub fn key(&mut self, key: egui::Key) -> bool {
        self.sounds.clear();
        if self.edit {
            let r = &self.rows()[self.row - 1];
            let (n, i) = self.stops(r);
            match key {
                egui::Key::ArrowRight if i + 1 < n => self.set_stop(r, i + 1),
                egui::Key::ArrowLeft if i > 0 => self.set_stop(r, i - 1),
                egui::Key::Enter | egui::Key::Escape => {
                    self.edit = false;
                    self.sounds.push(if key == egui::Key::Enter { Se::Select } else { Se::Cancel });
                }
                _ => {}
            }
            return false;
        }
        let n = self.rows().len();
        let first = self.first_row();
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowUp => {
                let span = n + 1 - first;
                let d = if key == egui::Key::ArrowDown { 1 } else { span - 1 };
                self.row = first + (self.row - first + d) % span;
                self.col = if self.row == 0 { self.tab } else { self.marked_col().min(self.cols(self.row) - 1) };
                self.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                let c = self.cols(self.row);
                if c > 1 {
                    self.col = if key == egui::Key::ArrowRight { (self.col + 1) % c } else { (self.col + c - 1) % c };
                    self.sounds.push(Se::Cursor);
                }
            }
            egui::Key::Enter => self.enter(),
            egui::Key::Tab if self.tabs() => {
                self.tab = (self.tab + 1) % TABS.len();
                self.row = self.row.min(self.rows().len());
                self.col = if self.row == 0 { self.tab } else { self.marked_col() };
                self.sounds.push(Se::Cursor);
            }
            egui::Key::Escape => return true,
            _ => {}
        }
        false
    }

    /// The page's top left.
    fn origin(&self, gu: &GameUi, sw: f32, sh: f32) -> (f32, f32) {
        if self.in_game
            && let Some(p) = gu.window_xy("menu    conf2win", sw, sh)
        {
            return p;
        }
        (((sw - W) / 2.0).round(), ((sh - H) / 2.0).round())
    }

    fn top(&self) -> f32 {
        if self.tabs() { 66.0 } else { 28.0 }
    }

    /// Every control's rect relative to the page: (row, col, rect).
    fn controls(&self) -> Vec<(usize, usize, [f32; 4])> {
        let mut out = Vec::new();
        if self.tabs() {
            for t in 0..TABS.len() {
                out.push((0, t, [TAB_X + TAB_PITCH * (t % 4) as f32, 26.0 + 18.0 * (t / 4) as f32, 88.0, 16.0]));
            }
        }
        for (i, r) in self.rows().iter().enumerate() {
            let y = self.top() + PITCH * i as f32 + 14.0;
            match Self::ctl(r) {
                Ctl::Buttons(2) | Ctl::Buttons(1) => {
                    for (c, x) in WIDE.iter().enumerate().take(r.choices.len()) {
                        out.push((i + 1, c, [*x, y, 88.0, 16.0]));
                    }
                }
                Ctl::Buttons(_) => {
                    for (c, x) in NARROW.iter().enumerate().take(r.choices.len()) {
                        out.push((i + 1, c, [*x, y, 64.0, 16.0]));
                    }
                }
                Ctl::Slider => out.push((i + 1, 0, [SLIDER_X, y, SLIDER_W, 16.0])),
            }
        }
        out
    }

    /// The pointer: controls follow it; a click is Enter, on a slider it sets the stop under the pointer.
    pub fn pointer(&mut self, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32) {
        self.sounds.clear();
        self.hover = false;
        let (x, y) = self.origin(gu, sw, sh);
        let hit = self.controls().into_iter().find(|(_, _, r)| p.0 >= x + r[0] && p.1 >= y + r[1] && p.0 < x + r[0] + r[2] && p.1 < y + r[1] + r[3]);
        let Some((row, col, r)) = hit else { return };
        self.hover = true;
        if (moved || clicked) && (row, col) != (self.row, self.col) && !self.edit {
            self.row = row;
            self.col = col;
            self.sounds.push(Se::Cursor);
        }
        if !clicked {
            return;
        }
        if row > 0
            && let Some(rw) = self.rows().get(row - 1)
            && matches!(Self::ctl(rw), Ctl::Slider)
        {
            let (n, _) = self.stops(rw);
            let f = ((p.0 - x - r[0]) / r[2]).clamp(0.0, 1.0);
            self.set_stop(rw, (f * (n - 1) as f32).round() as usize);
            self.edit = false;
            self.sounds.push(Se::Select);
            return;
        }
        self.enter();
    }

    /// Draws the page; returns its rect.
    pub fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) -> [f32; 4] {
        const N: u32 = 0x8080_8080;
        const ON: u32 = 0x8080_5020;
        let (x, y) = self.origin(gu, sw, sh);
        // The frame, then windowps 465's header strip (FUN_101144b0 skips its first cell, the backing).
        gu.frame(x, y, W, H);
        gu.sprite_cells("menu    windowps", 465, 1..2, x, y, N);
        gu.label(self.title(), x + 8.0, y + 6.0, N, false);
        let ctls = self.controls();
        let focus = |row: usize, col: usize| self.row == row && self.col == col;
        let mut hand = None;
        if self.tabs() {
            for (_, t, r) in ctls.iter().filter(|c| c.0 == 0) {
                let on = focus(0, *t);
                let d = if on { -1.0 } else { 0.0 };
                button(gu, [x + r[0] + d, y + r[1] + d, r[2], r[3]], if on { ON } else { N });
                gu.label(TABS[*t], x + r[0] + 5.0 + d, y + r[1] + 1.0 + d, if on { ON } else { N }, true);
                if *t == self.tab {
                    marker(gu, x + r[0], y + r[1], r[2]);
                }
                if on {
                    hand = Some((x + r[0], y + r[1]));
                }
            }
        }
        for (i, r) in self.rows().iter().enumerate() {
            let hy = y + self.top() + PITCH * i as f32;
            // The bullet (465 cell 2, drawn at its own place (20, 27) on the page) moved to the heading.
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 20.0 - b[0], hy + 2.0 - b[1], N);
            }
            gu.label(r.label, x + 32.0, hy, N, false);
            // A slider's value at the heading's end, right-aligned to the slider's (cut to the room left).
            if matches!(Self::ctl(r), Ctl::Slider) {
                let room = SLIDER_X + SLIDER_W - 32.0 - gu.label_width(r.label) - 16.0;
                let mut v = self.shown(r);
                while gu.label_width(&v) > room && v.pop().is_some() {}
                let vw = gu.label_width(&v);
                gu.label(&v, x + SLIDER_X + SLIDER_W - vw, hy, N, false);
            }
            for (_, c, b) in ctls.iter().filter(|c| c.0 == i + 1) {
                let on = focus(i + 1, *c) && !self.edit;
                match Self::ctl(r) {
                    Ctl::Buttons(_) => {
                        let d = if on { -1.0 } else { 0.0 };
                        button(gu, [x + b[0] + d, y + b[1] + d, b[2], b[3]], if on { ON } else { N });
                        gu.label(r.choices[*c].1, x + b[0] + 5.0 + d, y + b[1] + 1.0 + d, if on { ON } else { N }, true);
                        if r.choices[*c].0 == self.get(r.val) {
                            marker(gu, x + b[0], y + b[1], b[2]);
                        }
                    }
                    Ctl::Slider => {
                        // The Config pages' slider: track windowps 383, fill 384 (brighter while edited).
                        let (n, k) = self.stops(r);
                        let f = if n > 1 { k as f32 / (n - 1) as f32 } else { 1.0 };
                        let editing = self.edit && focus(i + 1, 0);
                        // The track is baked into each page's backing (465 cells 8..10 under conf2win child 3 at (88, 80)).
                        gu.sprite_cells("menu    windowps", 465, 8..11, x + b[0] - 88.0, y + b[1] - 80.0, N);
                        if f > 0.0 {
                            gu.sprite_scaled("menu    windowps", 384, x + b[0], y + b[1], f, 1.0, if editing { 0xa0a0_a0a0 } else { N });
                        }
                    }
                }
                if focus(i + 1, *c) && !self.edit {
                    hand = Some((x + b[0], y + b[1]));
                }
            }
        }
        if let Some((hx, hy)) = hand {
            gu.cursor(hx, hy);
        }
        [x, y, W, H]
    }

    /// Puts the cursor on a tab and row (offscreen shots).
    pub fn goto(&mut self, tab: usize, row: usize) {
        if self.tabs() {
            self.tab = tab.min(TABS.len() - 1);
        }
        self.row = row.clamp(self.first_row(), self.rows().len());
        self.col = self.marked_col();
    }

    /// The help bar's text for the cursor's row, with when a change shows.
    pub fn help(&self) -> String {
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i)) else { return "Choose a group of settings.".into() };
        let when = match r.when {
            When::Now => "",
            When::NextZone => " Takes effect at the next zone load.",
            When::Restart => " Takes effect on restart.",
            When::Stored => " Saved; not used by this client yet.",
        };
        format!("{}{when}", r.help)
    }

    pub fn title(&self) -> &'static str {
        if self.kind == Kind::Extras { "Extras" } else { "Settings" }
    }
}

/// A blank Config page button: conf2win's 88-px "ON" (windowps 105) or conf3win's 64-px "Log" (379) without its
/// caption, over the dark copy at (+2, +2) that the windows' own sprites bake in under each button (comwin 12 and
/// windowps 465: buttonto cells coloured (1, 1, 1, 169), alpha blended) and the additive body needs; other widths
/// stretch 105's two buttonto cells.
pub(super) fn button(gu: &mut GameUi, r: [f32; 4], tint: u32) {
    let k = match r[2] as u32 {
        88 => Some(105),
        64 => Some(379),
        _ => None,
    };
    if let Some(k) = k {
        // Tint 1 / 128 takes the cells' colour to about 0; blend word 0x44 = PS2 (Cs - Cd) * As + Cd.
        gu.sprite_base("menu    windowps", k, r[0] + 2.0, r[1] + 2.0, 0x8001_0101, 0x44);
        gu.sprite_base("menu    windowps", k, r[0], r[1], tint, 0);
        return;
    }
    match r[2] as u32 {
        _ => {
            let left = (r[2] * 48.0 / 88.0).round();
            gu.cell_stretch("menu    windowps", 105, 0, r[0], r[1], left, r[3], tint);
            gu.cell_stretch("menu    windowps", 105, 1, r[0] + left, r[1], r[2] - left, r[3], tint);
        }
    }
}

/// The chosen button's red underline: windowps 387 at (x + 37, y + 4) for an 88-px button, 412 for a 64-px one
/// (FUN_1018f4a0 / FUN_1018f510); other widths stretch 387's middle.
fn marker(gu: &mut GameUi, x: f32, y: f32, w: f32) {
    match w as u32 {
        88 => {
            gu.sprite("menu    windowps", 387, x + 37.0, y + 4.0, 0x8080_8080);
        }
        64 => {
            gu.sprite("menu    windowps", 412, x + 37.0, y + 4.0, 0x8080_8080);
        }
        _ => {
            gu.cell_stretch("menu    windowps", 387, 0, x + 6.0, y + 17.0, 9.0, 3.0, 0x8080_8080);
            gu.cell_stretch("menu    windowps", 387, 2, x + 15.0, y + 17.0, w - 30.0, 3.0, 0x8080_8080);
            gu.cell_stretch("menu    windowps", 387, 1, x + w - 15.0, y + 17.0, 8.0, 3.0, 0x8080_8080);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_store() {
        let mut c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        // Window mode (a slider of four): Windowed -> Borderless.
        ui.goto(0, 1);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.tool.get("0034"), Some(&2));
        // Background (under Resolution, Monitor, Graphics Card): Same as window -> 640 x 400.
        ui.goto(1, 4);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Escape);
        assert_eq!((ui.s.tool["0003"], ui.s.tool["0004"]), (640, 400));
        // Frame limit 300 -> 360 -> unlimited.
        ui.goto(6, 1);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.frame_limit, 0);
        assert!(ui.dirty);
        ui.s.store(&mut c);
        assert_eq!((c.window_mode(), c.background([1280, 800]), c.frame_limit), (2, [640, 400], 0));
        let back: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(Settings::of(&back), ui.s);
    }

    /// Extras: no tab row; buttons pick with Left / Right and Enter; Up from the first row wraps to the last.
    #[test]
    fn extras_buttons() {
        let c = Config::default();
        let mut ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
        assert_eq!((ui.row, ui.col), (1, 1));
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.map_overlay && ui.dirty);
        ui.key(egui::Key::ArrowUp);
        assert_eq!(ui.row, 4);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.stamp_all_lines);
    }
}
