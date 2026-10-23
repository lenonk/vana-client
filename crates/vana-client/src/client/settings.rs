//! Our settings screen (not retail: retail set these in the Windows "FINAL FANTASY XI Config" tool before
//! starting). The tool's values by tab (registry names 0000..0043, read by FUN_10016c30) plus our own, drawn from
//! the game's window, button, gauge and bullet sprites in the config pages' style.

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::config::{Config, FRAME_LIMITS};

/// What the screen edits (merged back into the config by the app).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub tool: std::collections::BTreeMap<String, u32>,
    pub frame_limit: u32,
    pub anisotropy: u16,
    pub party_tp: bool,
    pub map_opacity: u32,
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
        Self { tool: c.tool.clone(), frame_limit: c.frame_limit, anisotropy: c.anisotropy, party_tp: c.party_tp, map_opacity: pct(c.map_overlay_opacity), music: pct(c.music_volume), effects: pct(c.effects_volume), retail_aspect: c.retail_aspect, monitor: c.monitor.clone(), gpu: c.gpu.clone(), cursor_size: c.cursor_size }
    }

    pub fn store(&self, c: &mut Config) {
        c.monitor = self.monitor.clone();
        c.gpu = self.gpu.clone();
        c.tool = self.tool.clone();
        c.frame_limit = self.frame_limit;
        c.anisotropy = self.anisotropy;
        c.party_tp = self.party_tp;
        c.map_overlay_opacity = self.map_opacity as f32 / 100.0;
        c.music_volume = self.music as f32 / 100.0;
        c.effects_volume = self.effects as f32 / 100.0;
        c.retail_aspect = self.retail_aspect;
        c.cursor_size = self.cursor_size;
    }
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
    MapOpacity,
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
    /// Named choices (value, label); empty for sizes and numbers.
    choices: &'static [(u32, &'static str)],
    when: When,
}

const OFF_ON: &[(u32, &str)] = &[(0, "Off"), (1, "On")];

const fn row(label: &'static str, help: &'static str, val: Val, choices: &'static [(u32, &'static str)], when: When) -> Row {
    Row { label, help, val, choices, when }
}

const TABS: [&str; 7] = ["General", "Screen", "Textures", "Effects", "Sound", "Misc.", "Linux"];

fn rows(tab: usize) -> &'static [Row] {
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
        row("Textures", "0018: compressed (BC1 / BC2) or uncompressed textures.", Tool("0018"), &[(0, "High"), (1, "Low"), (2, "Uncompressed")], NextZone),
        row("2-D Mapping", "0019: on-screen map images.", Tool("0019"), &[(0, "Compressed"), (1, "Uncompressed")], Stored),
        row("Fonts", "0036: the font texture quality.", Tool("0036"), &[(0, "Compressed"), (1, "Uncompressed"), (2, "High Quality")], Restart),
    ];
    const EFFECTS: &[Row] = &[
        row("Environment", "0011: environment (vegetation) animation.", Tool("0011"), &[(0, "Off"), (1, "Normal"), (2, "Smooth")], Stored),
        row("Bump Mapping", "0017: enhanced shading.", Tool("0017"), OFF_ON, Stored),
        row("Stabilization", "0040: graphics stabilization for some cards.", Tool("0040"), OFF_ON, Stored),
        row("3D LCD Mode", "0030: interleaved stereo for 3D LCD screens.", Tool("0030"), OFF_ON, Stored),
    ];
    const SOUND: &[Row] = &[
        row("Sound", "0007: sound on or off.", Tool("0007"), OFF_ON, Now),
        row("Effect Channels", "0029: sound effects playing at once.", Tool("0029"), &[(12, "12"), (13, "13"), (14, "14"), (15, "15"), (16, "16"), (17, "17"), (18, "18"), (19, "19"), (20, "20")], Now),
        row("In Background", "0035: keep playing sound while the window is not focused.", Tool("0035"), OFF_ON, Now),
        row("Music Volume", "Our music level, under the Gameplay page's.", Music, &[], Now),
        row("Effects Volume", "Our sound effects level, under the Gameplay page's.", Effects, &[], Now),
    ];
    const MISC: &[Row] = &[
        row("Hardware Mouse", "0021: the game folder's cursor files (.ani) instead of the cursor the game draws.", Tool("0021"), OFF_ON, Now),
        row("Cursor Size", "The drawn cursor's size (Hardware Mouse off); Auto grows it with the window (not retail).", CursorSize, &[(0, "Auto"), (100, "1x"), (150, "1.5x"), (200, "2x"), (300, "3x"), (400, "4x")], Now),
        row("Opening Movie", "0022: play the opening movie on start.", Tool("0022"), OFF_ON, Stored),
        row("Simple Creator", "0023: character creation without 3D models.", Tool("0023"), OFF_ON, Stored),
        row("Screenshot Size", "0043: screenshots at the screen size.", Tool("0043"), OFF_ON, Stored),
    ];
    const LINUX: &[Row] = &[
        row("Frame Limit", "Frames drawn a second; game time stays at 60 ticks.", FrameLimit, &[], Now),
        row("Anisotropy", "Anisotropic texture filtering.", Aniso, &[(1, "Off"), (2, "2x"), (4, "4x"), (8, "8x"), (16, "16x")], NextZone),
        row("Party TP", "Show TP in the party window (retail config 0xae, off there).", PartyTp, OFF_ON, Now),
        row("Map Overlay", "Ctrl / Alt + O shows the zone map over the screen (not retail); its opacity.", MapOpacity, &[], Now),
    ];
    [GENERAL, SCREEN, TEXTURES, EFFECTS, SOUND, MISC, LINUX][tab.min(6)]
}

const RESOLUTIONS: [[u32; 2]; 17] = [
    [640, 480], [800, 600], [1024, 768], [1280, 720], [1280, 800], [1280, 1024], [1366, 768], [1440, 900], [1600, 900], [1680, 1050],
    [1920, 1080], [1920, 1200], [2560, 1080], [2560, 1440], [2560, 1600], [3440, 1440], [3840, 2160],
];

pub struct SettingsUi {
    pub s: Settings,
    /// Monitors as (name, "name  W x H") and GPU names, set by the app after opening.
    pub monitors: Vec<(String, String)>,
    pub gpus: Vec<String>,
    /// The window's pixels when opened (size lists).
    window: [u32; 2],
    tab: usize,
    /// 0 = the tab row, then the rows.
    row: usize,
    pub sounds: Vec<Se>,
    /// Changed since the app last took it.
    pub dirty: bool,
    /// The pointer is over a tab or a row (the hit cursor).
    pub hover: bool,
}

const W: f32 = 480.0;

impl SettingsUi {
    pub fn open(c: &Config, window: [u32; 2]) -> Self {
        Self { s: Settings::of(c), monitors: Vec::new(), gpus: Vec::new(), window, tab: 0, row: 1, sounds: Vec::new(), dirty: false, hover: false }
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
        let tool = |n: &str| self.s.tool.get(n).copied().or_else(|| crate::config::TOOL_DEFAULTS.iter().find(|d| d.0 == n).map(|d| d.1)).unwrap_or(0);
        match v {
            Val::Tool(n) => tool(n),
            Val::Size(..) | Val::Gamma | Val::Monitor | Val::Gpu => 0,
            Val::FrameLimit => self.s.frame_limit,
            Val::Aniso => self.s.anisotropy as u32,
            Val::PartyTp => self.s.party_tp as u32,
            Val::MapOpacity => self.s.map_opacity,
            Val::Music => self.s.music,
            Val::Effects => self.s.effects,
            Val::RetailAspect => self.s.retail_aspect as u32,
            Val::CursorSize => self.s.cursor_size,
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

    /// One step left (-1) or right (+1) on a row.
    fn step(&mut self, r: &Row, d: i32) {
        let before = self.s.clone();
        let cycle = |list: &[u32], cur: u32| -> u32 {
            let i = list.iter().position(|&x| x == cur).map_or(0, |i| i as i32);
            list[(i + d).clamp(0, list.len() as i32 - 1) as usize]
        };
        match r.val {
            Val::Size(a, b, k) => {
                let list = self.sizes(k);
                let cur = [self.tool_get(a), self.tool_get(b)];
                let i = list.iter().position(|&x| x == cur).map_or(if d > 0 { -1 } else { list.len() as i32 }, |i| i as i32);
                let [x, y] = list[(i + d).clamp(0, list.len() as i32 - 1) as usize];
                self.tool_set(a, x);
                self.tool_set(b, y);
            }
            Val::Gamma => {
                let g = (f32::from_bits(self.tool_get("0028")) + 0.1 * d as f32).clamp(-1.0, 1.0);
                let g = (g * 10.0).round() / 10.0;
                self.tool_set("0028", if g == 0.0 { 0 } else { g.to_bits() });
            }
            Val::FrameLimit => self.s.frame_limit = cycle(&FRAME_LIMITS, self.s.frame_limit),
            Val::Monitor if !self.monitors.is_empty() => {
                let i = self.monitors.iter().position(|m| m.0 == self.s.monitor).map_or(0, |i| i as i32);
                self.s.monitor = self.monitors[(i + d).clamp(0, self.monitors.len() as i32 - 1) as usize].0.clone();
            }
            Val::Monitor => {}
            Val::Gpu => {
                // "" (Default) first, then each adapter.
                let list: Vec<String> = std::iter::once(String::new()).chain(self.gpus.iter().cloned()).collect();
                let i = list.iter().position(|g| *g == self.s.gpu).map_or(0, |i| i as i32);
                self.s.gpu = list[(i + d).clamp(0, list.len() as i32 - 1) as usize].clone();
            }
            Val::Music => self.s.music = (self.s.music as i32 + 5 * d).clamp(0, 100) as u32,
            Val::Effects => self.s.effects = (self.s.effects as i32 + 5 * d).clamp(0, 100) as u32,
            Val::MapOpacity => self.s.map_opacity = (self.s.map_opacity as i32 + 5 * d).clamp(10, 100) as u32,
            v => {
                let list: Vec<u32> = r.choices.iter().map(|c| c.0).collect();
                let x = cycle(&list, self.get(v));
                match v {
                    Val::Tool(n) => self.tool_set(n, x),
                    Val::Aniso => self.s.anisotropy = x as u16,
                    Val::PartyTp => self.s.party_tp = x != 0,
                    Val::RetailAspect => self.s.retail_aspect = x != 0,
                    Val::CursorSize => self.s.cursor_size = x,
                    _ => {}
                }
            }
        }
        if self.s != before {
            self.dirty = true;
            self.sounds.push(Se::Cursor);
        } else {
            self.sounds.push(Se::Error);
        }
    }

    /// The row's value as shown.
    fn shown(&self, r: &Row) -> String {
        match r.val {
            Val::Size(a, b, _) => match [self.tool_get(a), self.tool_get(b)] {
                [0, _] | [_, 0] => "Same as Window".into(),
                [x, y] => format!("{x} x {y}"),
            },
            Val::Gamma => format!("{:+.1}", f32::from_bits(self.tool_get("0028"))),
            Val::FrameLimit if self.s.frame_limit == 0 => "Unlimited".into(),
            Val::FrameLimit => format!("{} fps", self.s.frame_limit),
            Val::Music => format!("{} %", self.s.music),
            Val::Effects => format!("{} %", self.s.effects),
            Val::MapOpacity => format!("{} %", self.s.map_opacity),
            Val::Monitor => self.monitors.iter().find(|m| m.0 == self.s.monitor).or(self.monitors.first()).map_or_else(|| "Default".into(), |m| m.1.clone()),
            Val::Gpu if self.s.gpu.is_empty() => "Default".into(),
            Val::Gpu => self.s.gpu.clone(),
            v => {
                let x = self.get(v);
                r.choices.iter().find(|c| c.0 == x).map_or(x.to_string(), |c| c.1.to_string())
            }
        }
    }

    /// A key; returns true when the screen closes (Escape).
    pub fn key(&mut self, key: egui::Key) -> bool {
        self.sounds.clear();
        let n = rows(self.tab).len();
        match key {
            egui::Key::ArrowDown => {
                self.row = (self.row + 1) % (n + 1);
                self.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowUp => {
                self.row = (self.row + n) % (n + 1);
                self.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter => {
                let d = if key == egui::Key::ArrowLeft { -1 } else { 1 };
                if self.row == 0 {
                    self.tab = (self.tab as i32 + d).rem_euclid(TABS.len() as i32) as usize;
                    self.sounds.push(Se::Cursor);
                } else {
                    let r = &rows(self.tab)[self.row - 1];
                    self.step(r, d);
                }
            }
            egui::Key::Tab => {
                self.tab = (self.tab + 1) % TABS.len();
                self.row = self.row.min(rows(self.tab).len());
                self.sounds.push(Se::Cursor);
            }
            egui::Key::Escape => return true,
            _ => {}
        }
        false
    }

    fn rect(&self, sw: f32, sh: f32) -> [f32; 4] {
        let h = 56.0 + 6.0 * 20.0 + 48.0;
        [((sw - W) / 2.0).round(), ((sh - h) / 2.0).round(), W, h]
    }

    /// The pointer: tabs and rows follow it; a click on a tab picks it, on a value's left / right half steps it.
    pub fn pointer(&mut self, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32) {
        self.sounds.clear();
        self.hover = false;
        let [x, y, _, _] = self.rect(sw, sh);
        let inside = |r: [f32; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3];
        for t in 0..TABS.len() {
            if inside(tab_rect(x, y, t)) {
                self.hover = true;
                if clicked && t != self.tab {
                    self.tab = t;
                    self.row = 0;
                    self.sounds.push(Se::Cursor);
                }
                return;
            }
        }
        for i in 0..rows(self.tab).len() {
            let r = value_rect(x, y, i);
            if inside([x + 12.0, r[1], W - 24.0, 18.0]) {
                self.hover = true;
                if (moved || clicked) && self.row != i + 1 {
                    self.row = i + 1;
                }
                if clicked && inside(r) {
                    let row = &rows(self.tab)[i];
                    self.step(row, if p.0 < r[0] + r[2] / 2.0 { -1 } else { 1 });
                }
                return;
            }
        }
    }

    /// Draws the screen; returns its rect.
    pub fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) -> [f32; 4] {
        let white = [0x80u8; 4];
        let grey = [0x58, 0x58, 0x58, 0x80];
        let [x, y, w, h] = self.rect(sw, sh);
        // Darker than a config page so it reads over the title art (ours).
        gu.rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, [0.0, 0.0, 0.0, 0.55]);
        gu.frame(x, y, w, h);
        // The header bar of the config pages (windowps 465 cell 1: the gauge strip over the width).
        gu.cell_stretch("menu    windowps", 465, 1, x, y + 4.0, w, 16.0, 0x8080_8080);
        gu.text("Settings", x + 8.0, y + 4.0, 0, white);
        for (t, name) in TABS.iter().enumerate() {
            let r = tab_rect(x, y, t);
            let on = t == self.tab;
            let tint = if on { 0x8080_5020 } else { 0x8080_8080 };
            button(gu, r, tint);
            gu.text(name, r[0] + r[2] / 2.0, r[1], 1, if on { white } else { grey });
            if on && self.row == 0 {
                gu.cursor(r[0], r[1]);
            }
        }
        for (i, r) in rows(self.tab).iter().enumerate() {
            let v = value_rect(x, y, i);
            // The bullet of the config pages (windowps 465 cell 2).
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 16.0 - b[0], v[1] + 4.0 - b[1], 0x8080_8080);
            }
            gu.text(r.label, x + 30.0, v[1], 0, white);
            let on = self.row == i + 1;
            match r.val {
                Val::Music | Val::Effects | Val::MapOpacity => {
                    // The config pages' slider: track windowps 383 and fill 384.
                    let f = self.get(r.val) as f32 / 100.0;
                    gu.sprite_scaled("menu    windowps", 383, v[0], v[1], v[2] / 192.0, 1.0, 0x8080_8080);
                    if f > 0.0 {
                        gu.sprite_scaled("menu    windowps", 384, v[0], v[1], f * v[2] / 192.0, 1.0, if on { 0xa0a0_a0a0 } else { 0x8080_8080 });
                    }
                    gu.text(&self.shown(r), v[0] + v[2] + 6.0, v[1], 0, white);
                }
                _ => {
                    button(gu, v, if on { 0x8080_5020 } else { 0x8080_8080 });
                    gu.scroll_text(&self.shown(r), v, white);
                    gu.text("<", v[0] - 10.0, v[1], 0, if on { white } else { grey });
                    gu.text(">", v[0] + v[2] + 3.0, v[1], 0, if on { white } else { grey });
                }
            }
            if on {
                gu.cursor(v[0], v[1]);
            }
        }
        // The highlighted row's help and when it takes effect.
        if let Some(r) = self.row.checked_sub(1).and_then(|i| rows(self.tab).get(i)) {
            let when = match r.when {
                When::Now => "",
                When::NextZone => " Takes effect at the next zone load.",
                When::Restart => " Takes effect on restart.",
                When::Stored => " Saved; not used by this client yet.",
            };
            let lines = gu.wrap(&format!("{}{when}", r.help), w - 24.0);
            for (k, l) in lines.iter().take(2).enumerate() {
                gu.text(l, x + 12.0, y + h - 44.0 + 16.0 * k as f32, 0, grey);
            }
        }
        [x, y, w, h]
    }

    /// Puts the cursor on a tab and row (offscreen shots).
    pub fn goto(&mut self, tab: usize, row: usize) {
        self.tab = tab.min(TABS.len() - 1);
        self.row = row.min(rows(self.tab).len());
    }

    /// The help bar's text for the highlighted row.
    pub fn help(&self) -> String {
        self.row.checked_sub(1).and_then(|i| rows(self.tab).get(i)).map_or_else(|| "Choose a group of settings.".into(), |r| r.help.to_string())
    }
}

fn tab_rect(x: f32, y: f32, t: usize) -> [f32; 4] {
    let tw = (W - 24.0) / TABS.len() as f32;
    [x + 12.0 + tw * t as f32, y + 26.0, tw - 2.0, 16.0]
}

fn value_rect(x: f32, y: f32, i: usize) -> [f32; 4] {
    [x + 240.0, y + 56.0 + 20.0 * i as f32, 200.0, 16.0]
}

/// A blank config-page button (windowps 105's two buttonto cells, without the caption) stretched to `r`.
pub(super) fn button(gu: &mut GameUi, r: [f32; 4], tint: u32) {
    let left = (r[2] * 48.0 / 88.0).round();
    gu.cell_stretch("menu    windowps", 105, 0, r[0], r[1], left, r[3], tint);
    gu.cell_stretch("menu    windowps", 105, 1, r[0] + left, r[1], r[2] - left, r[3], tint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_store() {
        let mut c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        // Window mode: Windowed -> Borderless.
        ui.tab = 0;
        ui.row = 1;
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.s.tool.get("0034"), Some(&2));
        // Background (under Resolution, Monitor, Graphics Card): Same as window -> 640 x 400.
        ui.tab = 1;
        ui.row = 4;
        ui.key(egui::Key::ArrowRight);
        assert_eq!((ui.s.tool["0003"], ui.s.tool["0004"]), (640, 400));
        // Frame limit 300 -> 360 -> unlimited.
        ui.tab = 6;
        ui.row = 1;
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.s.frame_limit, 0);
        assert!(ui.dirty);
        ui.s.store(&mut c);
        assert_eq!((c.window_mode(), c.background([1280, 800]), c.frame_limit), (2, [640, 400], 0));
        let back: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(Settings::of(&back), ui.s);
    }
}
