//! Our Settings and Extras pages (not retail). Settings holds what retail set in the Windows "FINAL FANTASY XI
//! Config" tool before starting (registry names 0000..0043, read by FUN_10016c30) plus our own display options;
//! Extras holds the gameplay additions (the add-ons' features), each off until turned on. Both are drawn as the
//! Config pages are (FUN_101144b0 over windowps 465: bullets, fontshp text, the buttons, markers and
//! sliders of conf2win / conf3win), at the pages' place in game and centred in the lobby. Both have several pages,
//! turned as miss00 turns its nations: red arrows (menu frames 25 / 26) at the top corners and the page's name beside
//! the left one (FUN_1022de70).

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::config::{Config, FRAME_LIMITS};

/// What the pages edit (merged back into the config by the app).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub tool: std::collections::BTreeMap<String, u32>,
    pub frame_limit: u32,
    pub anisotropy: u16,
    pub draw_distance: u32,
    pub party_tp: bool,
    pub map_overlay: bool,
    pub map_opacity: u32,
    pub synth_amount: bool,
    pub stamp_all_lines: bool,
    pub target_distance: bool,
    pub target_hpp: bool,
    pub auto_advance: u32,
    pub timers: bool,
    pub timers_dock: u32,
    pub regime: bool,
    pub minimap: bool,
    pub minimap_dock: u32,
    pub minimap_size: u32,
    pub music: u32,
    pub effects: u32,
    pub retail_aspect: bool,
    pub monitor: String,
    pub gpu: String,
    pub cursor_size: u32,
    pub modern_ui: bool,
    pub modern_scale: u32,
    pub modern_opacity: u32,
    /// The mod packs in priority order (the Mods page; config `mods`).
    pub mods: Vec<crate::config::ModRef>,
    /// Panel Places' Reset pressed: this character's modern panels go back to where they start (not kept).
    pub reset_places: bool,
}

impl Settings {
    pub fn of(c: &Config) -> Self {
        let pct = |v: f32| (v.clamp(0.0, 1.0) * 100.0).round() as u32;
        Self {
            tool: c.tool.clone(),
            frame_limit: c.frame_limit,
            anisotropy: c.anisotropy,
            draw_distance: c.draw_distance,
            party_tp: c.party_tp,
            map_overlay: c.map_overlay,
            map_opacity: pct(c.map_overlay_opacity),
            synth_amount: c.synth_amount,
            stamp_all_lines: c.stamp_all_lines,
            target_distance: c.target_distance,
            target_hpp: c.target_hpp,
            auto_advance: c.auto_advance,
            timers: c.timers,
            timers_dock: c.timers_dock,
            regime: c.regime,
            minimap: c.minimap,
            minimap_dock: c.minimap_dock,
            minimap_size: c.minimap_size,
            // The game's own volumes (Gameplay page, config 10 / 9; the lobby's 0xa8): set by the opener.
            music: 100,
            effects: 100,
            retail_aspect: c.retail_aspect,
            monitor: c.monitor.clone(),
            gpu: c.gpu.clone(),
            cursor_size: c.cursor_size,
            modern_ui: c.modern_ui,
            modern_scale: c.modern_scale,
            modern_opacity: pct(c.modern_opacity),
            mods: crate::modpack::merged(&c.mods, &crate::modpack::folders()),
            reset_places: false,
        }
    }

    pub fn store(&self, c: &mut Config) {
        c.monitor = self.monitor.clone();
        c.gpu = self.gpu.clone();
        c.tool = self.tool.clone();
        c.frame_limit = self.frame_limit;
        c.anisotropy = self.anisotropy;
        c.draw_distance = self.draw_distance;
        c.party_tp = self.party_tp;
        c.map_overlay = self.map_overlay;
        c.map_overlay_opacity = self.map_opacity as f32 / 100.0;
        c.synth_amount = self.synth_amount;
        c.stamp_all_lines = self.stamp_all_lines;
        c.target_distance = self.target_distance;
        c.target_hpp = self.target_hpp;
        c.auto_advance = self.auto_advance;
        c.timers = self.timers;
        c.timers_dock = self.timers_dock;
        c.regime = self.regime;
        c.minimap = self.minimap;
        c.minimap_dock = self.minimap_dock;
        c.minimap_size = self.minimap_size;
        c.retail_aspect = self.retail_aspect;
        c.cursor_size = self.cursor_size;
        c.modern_ui = self.modern_ui;
        c.modern_scale = self.modern_scale;
        c.modern_opacity = self.modern_opacity as f32 / 100.0;
        c.mods = self.mods.clone();
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
    DrawDistance,
    PartyTp,
    MapOverlay,
    MapOpacity,
    SynthAmount,
    StampAll,
    TargetDistance,
    TargetHpp,
    AutoAdvance,
    Timers,
    TimersDock,
    Regime,
    Minimap,
    MinimapDock,
    MinimapSize,
    Music,
    Effects,
    RetailAspect,
    CursorSize,
    /// The monitor for full screen, from the list the app passes in.
    Monitor,
    /// The graphics adapter ("Default" then the app's list).
    Gpu,
    ModernUi,
    ModernScale,
    ModernOpacity,
    ResetPlaces,
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
const MODERN_SCALES: &[(u32, &str)] = &[(50, "50%"), (55, "55%"), (60, "60%"), (65, "65%"), (70, "70%"), (75, "75%"), (80, "80%"), (85, "85%"), (90, "90%"), (95, "95%"), (100, "100%"), (105, "105%"), (110, "110%"), (115, "115%"), (120, "120%"), (125, "125%"), (130, "130%"), (135, "135%"), (140, "140%"), (145, "145%"), (150, "150%")];

const fn row(label: &'static str, help: &'static str, val: Val, choices: &'static [(u32, &'static str)], when: When) -> Row {
    Row { label, help, val, choices, when }
}

const TABS: [&str; 8] = ["General", "Screen", "Textures", "Effects", "Sound", "Misc.", "Client", "Mods"];
/// The Mods page (client/modsui.rs): its rows come from the mods folder, not a table.
const MODS: usize = 7;
const EXTRA_TABS: [&str; 3] = ["Maps", "Timers", "Misc."];

/// `in_game`: the volumes are the Gameplay page's there, so the Sound page leaves them out. `modern`: the minimap's and
/// the Timers' place and size are set by dragging them there, so those rows are left out.
fn rows(kind: Kind, tab: usize, in_game: bool, modern: bool) -> &'static [Row] {
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
        row("GPU", "The graphics card to draw with; Default is the high-performance one (not retail).", Gpu, &[], Restart),
        row("Background", "0003 / 0004: the 3D scene's resolution, scaled to the window.", Size("0003", "0004", Sizes::Background), &[], Now),
        row("Menu Size", "0037 / 0038: the resolution menus and text are laid out at, then stretched.", Size("0037", "0038", Sizes::Ui), &[], Now),
        row("3D Aspect", "Window: the window's shape. Config: retail's ratio from Misc. 2 / the lobby Config.", RetailAspect, &[(0, "Window"), (1, "Config")], Now),
        row("Draw Distance", "How far scenery, characters and effects draw: Retail is the game's distance, the others farther (not retail).", DrawDistance, &[(100, "Retail"), (150, "1.5x"), (200, "2x"), (300, "3x"), (0, "Unlimited")], Now),
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
        // The title's one volume (lobby config 0xa8); in game the Gameplay page has them (rows left out there).
        row("Music Volume", "The volume before you are in game (the lobby's; in game, the Gameplay page's sliders).", Music, &[], Now),
        row("Sound Effects Volume", "The volume before you are in game (the lobby's; in game, the Gameplay page's sliders).", Effects, &[], Now),
    ];
    const MISC: &[Row] = &[
        row("Hardware Mouse", "0021: the game folder's cursor files (.ani) instead of the cursor the game draws.", Tool("0021"), OFF_ON, Now),
        row("Cursor Size", "The drawn cursor's size (Hardware Mouse off); Auto grows it with the window (not retail).", CursorSize, &[(0, "Auto"), (100, "1x"), (150, "1.5x"), (200, "2x"), (300, "3x"), (400, "4x")], Now),
        row("Opening Movie", "0022: play the opening movie on start.", Tool("0022"), OFF_ON, Stored),
        row("Simple Character Creation", "0023: character creation without 3D models.", Tool("0023"), OFF_ON, Stored),
        row("Screenshot Size", "0043: screenshots at the screen size.", Tool("0043"), OFF_ON, Stored),
    ];
    const CLIENT: &[Row] = &[
        row("Frame Limit", "Frames drawn a second; game time stays at 60 ticks.", FrameLimit, &[], Now),
        row("Anisotropic Filtering", "Anisotropic texture filtering.", Aniso, &[(1, "Off"), (2, "2x"), (4, "4x"), (8, "8x"), (16, "16x")], NextZone),
        row("Party TP", "Show TP in the party window (retail config 0xae, off there).", PartyTp, OFF_ON, Now),
        row("Interface", "Classic: the game's own windows. Modern: the new interface's windows where it has them (not retail).", ModernUi, &[(0, "Classic"), (1, "Modern")], Now),
        row("Interface Size", "The modern interface's size (not retail).", ModernScale, MODERN_SCALES, Now),
        row("Panel Opacity", "How opaque the modern interface's window backgrounds are (not retail).", ModernOpacity, &[], Now),
        row("Panel Places", "Reset puts this character's modern windows back where they start (not retail).", ResetPlaces, &[(0, "As Moved"), (1, "Reset")], Now),
    ];
    const MAPS: &[Row] = &[
        row("Map Overlay", "Ctrl / Alt + O shows the zone map over the screen, as the map add-ons do.", MapOverlay, OFF_ON, Now),
        row("Map Overlay Opacity", "How opaque the map overlay is.", MapOpacity, &[], Now),
        row("Minimap", "The zone map in a corner, centred on you, with what is near; the mouse wheel over it zooms.", Minimap, OFF_ON, Now),
        row("Minimap Position", "Where the minimap stands: the top right (beside the menus while one is open), or the top left under the cast bar.", MinimapDock, &[(0, "Top Right"), (1, "Top Left")], Now),
        row("Minimap Size", "How big the minimap is.", MinimapSize, &[(0, "Small"), (1, "Medium"), (2, "Large")], Now),
    ];
    const TIMERS_ROW: Row = row("Timers", "Recasts, our effects and the ones we put on others and on monsters, in a window that slides in and out (/timers).", Timers, OFF_ON, Now);
    const REGIME_ROW: Row = row("Training Regime", "While a Fields or Grounds of Valor regime is on, its targets and kills in a window beside the timers (from a manual's page, or the kills).", Regime, OFF_ON, Now);
    const TIMERS: &[Row] = &[
        TIMERS_ROW,
        row("Timers Position", "Where the timers window docks: the left edge, the right edge above the party list, or the top.", TimersDock, &[(0, "Left"), (1, "Right"), (2, "Top")], Now),
        REGIME_ROW,
    ];
    const TIMERS_MODERN: &[Row] = &[TIMERS_ROW, REGIME_ROW];
    const EXTRA_MISC: &[Row] = &[
        row("Synthesis Amount", "After Synthesize, ask how many to make and make them one after another.", SynthAmount, OFF_ON, Now),
        row("Timestamps on All Lines", "Every log line gets the time, not only chat (the format of /timestamp; seconds while it is off).", StampAll, OFF_ON, Now),
        row("Target Distance", "How far your target is, centre to centre, in the target window.", TargetDistance, OFF_ON, Now),
        row("Target HP %", "The target's HP percent beside its gauge in the classic target window (the modern one shows it).", TargetHpp, OFF_ON, Now),
        row("Text Auto-Advance", "An event's message waiting for a key goes on by itself after this many seconds.", AutoAdvance, &[(0, "OFF"), (3, "3 s"), (5, "5 s"), (8, "8 s")], Now),
    ];
    match kind {
        Kind::Extras if modern => [&MAPS[..3], TIMERS_MODERN, EXTRA_MISC][tab.min(2)],
        Kind::Extras => [MAPS, TIMERS, EXTRA_MISC][tab.min(2)],
        Kind::Settings if in_game && tab == 4 => &SOUND[..3],
        Kind::Settings if tab == MODS => &[],
        // Panel Places (the last Linux row) is a character's: in game only.
        Kind::Settings if !in_game && tab == 6 => &CLIENT[..CLIENT.len() - 1],
        Kind::Settings => [GENERAL, SCREEN, TEXTURES, EFFECTS, SOUND, MISC, CLIENT][tab.min(6)],
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
/// Where buttons widened for their captions start.
const WIDE_X: f32 = 32.0;
const SLIDER_W: f32 = 192.0;
/// miss00's page arrows (children 9 / 10: menu frames 25 / 26, 18 x 9) and its page name (FUN_1022de70, game font).
const ARROWS: [f32; 2] = [9.0, 343.0];
const ARROW_Y: f32 = 11.0;
const NAME_XY: (f32, f32) = (60.0, 10.0);

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
    /// The page (Settings).
    tab: usize,
    /// The cursor's row: 0 the page name (paged pages), then the rows from 1; the column within it.
    row: usize,
    col: usize,
    /// A slider being edited (Enter, then Left / Right, as the Config pages' sliders).
    edit: bool,
    pub sounds: Vec<Se>,
    /// Changed since the app last took it.
    pub dirty: bool,
    /// The pointer is over a control (the hit cursor).
    pub hover: bool,
    /// The Mods page's archives and moves, opened the first time the page shows.
    pub mods_ui: Option<super::modsui::Mods>,
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
        let mut s = Self { kind, s: Settings::of(c), monitors: Vec::new(), gpus: Vec::new(), window, in_game: false, tab: 0, row: 0, col: 0, edit: false, sounds: Vec::new(), dirty: false, hover: false, mods_ui: None };
        s.col = s.marked_col();
        s
    }

    fn tabs(&self) -> &'static [&'static str] {
        if self.kind == Kind::Extras { &EXTRA_TABS } else { &TABS }
    }

    fn rows(&self) -> &'static [Row] {
        rows(self.kind, self.tab, self.in_game, self.s.modern_ui)
    }

    fn ctl(r: &Row) -> Ctl {
        match r.val {
            Val::Size(..) | Val::Gamma | Val::FrameLimit | Val::Monitor | Val::Gpu | Val::Music | Val::Effects | Val::MapOpacity | Val::ModernOpacity => Ctl::Slider,
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
            Val::DrawDistance => self.s.draw_distance,
            Val::PartyTp => self.s.party_tp as u32,
            Val::ModernUi => self.s.modern_ui as u32,
            Val::ModernScale => self.s.modern_scale,
            Val::ModernOpacity => self.s.modern_opacity,
            Val::ResetPlaces => self.s.reset_places as u32,
            Val::MapOverlay => self.s.map_overlay as u32,
            Val::MapOpacity => self.s.map_opacity,
            Val::SynthAmount => self.s.synth_amount as u32,
            Val::StampAll => self.s.stamp_all_lines as u32,
            Val::TargetDistance => self.s.target_distance as u32,
            Val::TargetHpp => self.s.target_hpp as u32,
            Val::AutoAdvance => self.s.auto_advance,
            Val::Timers => self.s.timers as u32,
            Val::TimersDock => self.s.timers_dock,
            Val::Regime => self.s.regime as u32,
            Val::Minimap => self.s.minimap as u32,
            Val::MinimapDock => self.s.minimap_dock,
            Val::MinimapSize => self.s.minimap_size,
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
            Val::DrawDistance => self.s.draw_distance = x,
            Val::PartyTp => self.s.party_tp = x != 0,
            Val::ModernUi => self.s.modern_ui = x != 0,
            Val::ModernScale => self.s.modern_scale = x,
            Val::ModernOpacity => self.s.modern_opacity = x,
            Val::ResetPlaces => self.s.reset_places = x != 0,
            Val::MapOverlay => self.s.map_overlay = x != 0,
            Val::SynthAmount => self.s.synth_amount = x != 0,
            Val::StampAll => self.s.stamp_all_lines = x != 0,
            Val::TargetDistance => self.s.target_distance = x != 0,
            Val::TargetHpp => self.s.target_hpp = x != 0,
            Val::AutoAdvance => self.s.auto_advance = x,
            Val::Timers => self.s.timers = x != 0,
            Val::TimersDock => self.s.timers_dock = x,
            Val::Regime => self.s.regime = x != 0,
            Val::Minimap => self.s.minimap = x != 0,
            Val::MinimapDock => self.s.minimap_dock = x,
            Val::MinimapSize => self.s.minimap_size = x,
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
            Val::ModernOpacity => (19, (self.s.modern_opacity.clamp(10, 100) / 5 - 2) as usize),
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
            Val::ModernOpacity => self.s.modern_opacity = ((i as u32 + 2) * 5).clamp(10, 100),
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
            Val::ModernOpacity => format!("{}%", self.s.modern_opacity),
            Val::Monitor => self.monitors.iter().find(|m| m.0 == self.s.monitor).or(self.monitors.first()).map_or_else(|| "Default".into(), |m| m.0.clone()),
            Val::Gpu if self.s.gpu.is_empty() => "Default".into(),
            // The name without the driver's part ("AMD Radeon RX 6800 XT (RADV NAVI21)").
            Val::Gpu => self.s.gpu.split(" (").next().unwrap_or_default().to_string(),
            v => {
                let x = self.get(v);
                r.choices.iter().find(|c| c.0 == x).map_or(x.to_string(), |c| c.1.to_string())
            }
        }
    }

    /// The column of a buttons row whose choice holds (the cursor's start on it).
    fn marked_col(&self) -> usize {
        if self.on_mods() {
            return self.mods_marked();
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i)) else { return 0 };
        match Self::ctl(r) {
            Ctl::Buttons(_) => r.choices.iter().position(|c| c.0 == self.get(r.val)).unwrap_or(0),
            Ctl::Slider => 0,
        }
    }

    fn cols(&self, row: usize) -> usize {
        if self.on_mods() {
            return self.mods_cols(row);
        }
        match row.checked_sub(1).and_then(|i| self.rows().get(i)) {
            None => 1,
            Some(r) => match Self::ctl(r) {
                Ctl::Buttons(n) => n,
                Ctl::Slider => 1,
            },
        }
    }

    /// Turns the page by `d`, the cursor kept on its row where the page has it.
    fn turn(&mut self, d: i32) {
        self.tab = (self.tab as i32 + d).rem_euclid(self.tabs().len() as i32) as usize;
        if self.on_mods() {
            self.mods_ui.get_or_insert_with(super::modsui::Mods::open);
        }
        self.row = self.row.min(self.row_count());
        self.col = self.marked_col();
        self.sounds.push(Se::Cursor);
    }

    /// Enter on the cursor's control.
    fn enter(&mut self) {
        if self.on_mods() {
            return self.mods_enter();
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i)) else { return };
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

    /// A key; returns true when the page closes (Escape). Left / Right move between a row's buttons; on the page name
    /// (a stop of ours above the rows) or a slider row, or with Ctrl / Alt (the lists' page modifier), they turn the
    /// page.
    pub fn key(&mut self, key: egui::Key) -> bool {
        self.key_mod(key, false)
    }

    pub fn key_mod(&mut self, key: egui::Key, page_mod: bool) -> bool {
        self.sounds.clear();
        if self.on_mods() {
            return self.mods_key(key, page_mod);
        }
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
        let span = self.rows().len() + 1;
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowUp => {
                let d = if key == egui::Key::ArrowDown { 1 } else { span - 1 };
                self.row = (self.row + d) % span;
                self.col = self.marked_col().min(self.cols(self.row) - 1);
                self.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                let c = self.cols(self.row);
                let d = if key == egui::Key::ArrowRight { 1 } else { -1 };
                if c > 1 && !page_mod {
                    self.col = (self.col as i32 + d).rem_euclid(c as i32) as usize;
                    self.sounds.push(Se::Cursor);
                } else {
                    self.turn(d);
                }
            }
            egui::Key::Enter => self.enter(),
            egui::Key::Tab => self.turn(1),
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

    /// The first heading, under the arrows (miss00 starts its rows at y 40).
    const TOP: f32 = 36.0;

    /// The page arrows' rects relative to the page (a little larger than the sprites, for the pointer).
    fn arrow_rects(&self) -> [[f32; 4]; 2] {
        ARROWS.map(|x| [x - 4.0, ARROW_Y - 4.0, 26.0, 17.0])
    }

    /// Every control's rect relative to the page: (row, col, rect).
    fn controls(&self) -> Vec<(usize, usize, [f32; 4])> {
        if self.on_mods() {
            return self.mods_controls();
        }
        let mut out = Vec::new();
        for (i, r) in self.rows().iter().enumerate() {
            let y = Self::TOP + PITCH * i as f32 + 14.0;
            // conf2win's two 88-px and conf3win's three 64-px columns; a caption that does not fit (12 px of the
            // button are its ends) widens every button of the row, from the bullets' column.
            let need = r.choices.iter().map(|c| super::super::render::screens::label_width(c.1) + 12.0).fold(0.0, f32::max);
            match Self::ctl(r) {
                Ctl::Buttons(n) if n <= 2 && need <= 88.0 => {
                    for (c, x) in WIDE.iter().enumerate().take(n) {
                        out.push((i + 1, c, [*x, y, 88.0, 16.0]));
                    }
                }
                Ctl::Buttons(n) if n == 3 && need <= 64.0 => {
                    for (c, x) in NARROW.iter().enumerate().take(n) {
                        out.push((i + 1, c, [*x, y, 64.0, 16.0]));
                    }
                }
                Ctl::Buttons(n) => {
                    let w = need.ceil();
                    for c in 0..n {
                        out.push((i + 1, c, [WIDE_X + (w + 8.0) * c as f32, y, w, 16.0]));
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
        if let Some(k) = self.arrow_rects().iter().position(|r| p.0 >= x + r[0] && p.1 >= y + r[1] && p.0 < x + r[0] + r[2] && p.1 < y + r[1] + r[3])
        {
            self.hover = true;
            if clicked {
                self.edit = false;
                self.turn(if k == 0 { -1 } else { 1 });
            }
            return;
        }
        if self.on_mods() && self.mods_ui.as_ref().is_some_and(|m| m.held.is_some()) {
            return;
        }
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
        if let Some(rw) = self.rows().get(row - 1)
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
        gu.frame(x, y, W, H);
        // miss00: the arrows and the page's name.
        gu.sprite("menu    frames", 25, x + ARROWS[0], y + ARROW_Y, N);
        gu.sprite("menu    frames", 26, x + ARROWS[1], y + ARROW_Y, N);
        gu.text(self.tabs()[self.tab], x + NAME_XY.0, y + NAME_XY.1, 0, [0x80; 4]);
        if self.row == 0 {
            gu.cursor(x + NAME_XY.0 - 4.0, y + NAME_XY.1);
        }
        if self.on_mods() {
            self.mods_draw(gu, x, y);
            return [x, y, W, H];
        }
        let ctls = self.controls();
        let focus = |row: usize, col: usize| self.row == row && self.col == col;
        let mut hand = None;
        for (i, r) in self.rows().iter().enumerate() {
            let hy = y + Self::TOP + PITCH * i as f32;
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

    /// Puts the cursor on a page and row (offscreen shots; 0 the page name).
    pub fn goto(&mut self, tab: usize, row: usize) {
        self.tab = tab.min(self.tabs().len() - 1);
        if self.on_mods() {
            let m = self.mods_ui.get_or_insert_with(super::modsui::Mods::open);
            m.wait_scan();
            let _ = m.tick(&mut self.s.mods);
        }
        self.row = row.min(self.row_count());
        self.col = self.marked_col();
        if let Some(m) = self.mods_ui.as_mut().filter(|_| self.kind == Kind::Settings && self.tab == MODS) {
            m.follow(self.row);
        }
    }

    /// The help bar's text for the cursor's row, with when a change shows.
    pub fn help(&self) -> String {
        if self.on_mods() {
            return self.mods_help();
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i)) else {
            return format!("Left / Right: another page of {}.", self.title().to_lowercase());
        };
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

/// A row as the modern layer draws it: its heading, its help (with when a change shows), its controls.
pub struct VRow {
    pub label: String,
    pub help: String,
    pub ctl: VCtl,
}

/// Buttons (caption, the choice that holds), or a slider (stops, the one it is at, the value shown).
pub enum VCtl {
    Buttons(Vec<(String, bool)>),
    Slider { n: usize, at: usize, shown: String },
}

/// The page as the modern layer draws it; the cursor's row (0 the page name) and column, a slider being edited.
pub struct View {
    pub title: &'static str,
    pub tabs: &'static [&'static str],
    pub tab: usize,
    pub rows: Vec<VRow>,
    pub row: usize,
    pub col: usize,
    pub edit: bool,
    pub help: String,
}

fn when_text(w: When) -> &'static str {
    match w {
        When::Now => "",
        When::NextZone => " Takes effect at the next zone load.",
        When::Restart => " Takes effect on restart.",
        When::Stored => " Saved; not used by this client yet.",
    }
}

impl SettingsUi {
    pub fn view(&self) -> View {
        let rows = if self.on_mods() {
            let n = self.s.mods.len();
            let m = self.mods_ui.as_ref();
            (0..self.mods_total())
                .filter_map(|i| {
                    if i < n {
                        let on = self.s.mods[i].enabled;
                        let ctl = VCtl::Buttons(vec![("ON".into(), on), ("OFF".into(), !on), ("Move".into(), false)]);
                        Some(VRow { label: format!("{}. {}", i + 1, self.s.mods[i].name), help: format!("Mod pack {} of {n}: a higher one's textures win.", i + 1), ctl })
                    } else {
                        let p = m?.packs.get(i - n)?;
                        let name = p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                        let label = m.and_then(|m| m.progress(&p.slug)).map_or(name.clone(), |pm| format!("{}%  {name}", pm / 10));
                        Some(VRow { label, help: format!("{} game file{} to install.", p.files, if p.files == 1 { "" } else { "s" }), ctl: VCtl::Buttons(vec![("Install".into(), false)]) })
                    }
                })
                .collect()
        } else {
            self.rows()
                .iter()
                .map(|r| {
                    let ctl = match Self::ctl(r) {
                        Ctl::Buttons(_) => VCtl::Buttons(r.choices.iter().map(|c| (c.1.to_string(), c.0 == self.get(r.val))).collect()),
                        Ctl::Slider => {
                            let (n, at) = self.stops(r);
                            VCtl::Slider { n, at, shown: self.shown(r) }
                        }
                    };
                    VRow { label: r.label.to_string(), help: format!("{}{}", r.help, when_text(r.when)), ctl }
                })
                .collect()
        };
        View { title: self.title(), tabs: self.tabs(), tab: self.tab, rows, row: self.row, col: self.col, edit: self.edit, help: self.help() }
    }

    /// The modern layer's mouse: page `t`.
    pub fn m_tab(&mut self, t: usize) {
        if t != self.tab {
            self.edit = false;
            self.turn(t as i32 - self.tab as i32);
        }
    }

    /// The cursor to a control (row from 1); true when it moved.
    pub fn m_point(&mut self, row: usize, col: usize) -> bool {
        if self.edit || self.mods_ui.as_ref().is_some_and(|m| m.held.is_some()) || (row, col) == (self.row, self.col) {
            return false;
        }
        (self.row, self.col) = (row, col);
        let (mods, row) = (self.on_mods(), self.row);
        if let Some(m) = self.mods_ui.as_mut().filter(|_| mods) {
            m.follow(row);
        }
        true
    }

    /// A click on a button: the cursor there and Enter.
    pub fn m_click(&mut self, row: usize, col: usize) {
        self.sounds.clear();
        self.m_point(row, col);
        if self.on_mods() {
            self.mods_enter();
        } else {
            self.enter();
        }
    }

    /// A slider set to stop `i` by the pointer.
    pub fn m_slide(&mut self, row: usize, i: usize) {
        self.sounds.clear();
        let Some(r) = row.checked_sub(1).and_then(|k| self.rows().get(k)) else { return };
        (self.row, self.col, self.edit) = (row, 0, false);
        let (_, at) = self.stops(r);
        if at != i {
            self.set_stop(r, i);
            self.sounds.push(Se::Cursor);
        }
    }
}

/// The Mods page (client/modsui.rs): the installed packs (ON / OFF / Move), then the archives (Install).
impl SettingsUi {
    fn on_mods(&self) -> bool {
        self.kind == Kind::Settings && self.tab == MODS
    }

    /// Rows below the page name.
    fn row_count(&self) -> usize {
        if self.on_mods() { self.mods_total() } else { self.rows().len() }
    }

    fn mods_total(&self) -> usize {
        self.s.mods.len() + self.mods_ui.as_ref().map_or(0, |m| m.packs.len())
    }

    fn mods_cols(&self, row: usize) -> usize {
        match row.checked_sub(1) {
            Some(i) if i < self.s.mods.len() => 3,
            _ => 1,
        }
    }

    fn mods_marked(&self) -> usize {
        match self.row.checked_sub(1).and_then(|i| self.s.mods.get(i)) {
            Some(m) if !m.enabled => 1,
            _ => 0,
        }
    }

    /// Per frame: the archive scan and an install coming in.
    pub fn tick(&mut self) {
        if let Some(m) = self.mods_ui.as_mut()
            && m.tick(&mut self.s.mods)
        {
            self.dirty = true;
        }
    }

    fn mods_key(&mut self, key: egui::Key, page_mod: bool) -> bool {
        use egui::Key as K;
        let n = self.s.mods.len();
        if let Some(before) = self.mods_ui.as_mut().and_then(|m| m.held.take()) {
            // Moving: Up / Down carry the pack, Enter places it, Escape puts the order back.
            let i = self.row - 1;
            let mut keep = Some(before);
            match key {
                K::ArrowUp if i > 0 => {
                    self.s.mods.swap(i, i - 1);
                    self.row -= 1;
                    self.sounds.push(Se::Cursor);
                }
                K::ArrowDown if i + 1 < n => {
                    self.s.mods.swap(i, i + 1);
                    self.row += 1;
                    self.sounds.push(Se::Cursor);
                }
                K::Enter => {
                    self.dirty |= keep.take().is_some_and(|b| b != self.s.mods);
                    self.sounds.push(Se::Select);
                }
                K::Escape => {
                    if let Some(b) = keep.take() {
                        let name = self.s.mods[i].name.clone();
                        self.row = b.iter().position(|m| m.name == name).map_or(self.row, |k| k + 1);
                        self.s.mods = b;
                    }
                    self.sounds.push(Se::Cancel);
                }
                _ => {}
            }
            if let Some(m) = self.mods_ui.as_mut() {
                m.held = keep;
                m.follow(self.row);
            }
            return false;
        }
        match key {
            K::ArrowDown | K::ArrowUp => {
                let span = self.mods_total() + 1;
                let d = if key == K::ArrowDown { 1 } else { span - 1 };
                self.row = (self.row + d) % span;
                self.col = self.mods_marked().min(self.mods_cols(self.row) - 1);
                self.sounds.push(Se::Cursor);
            }
            K::ArrowLeft | K::ArrowRight => {
                let c = self.mods_cols(self.row);
                let d = if key == K::ArrowRight { 1 } else { -1 };
                if self.row > 0 && c > 1 && !page_mod {
                    self.col = (self.col as i32 + d).rem_euclid(c as i32) as usize;
                    self.sounds.push(Se::Cursor);
                } else {
                    self.turn(d);
                }
            }
            K::Enter => self.mods_enter(),
            K::Tab => self.turn(1),
            K::Escape => return true,
            _ => {}
        }
        if let Some(m) = self.mods_ui.as_mut() {
            m.follow(self.row);
        }
        false
    }

    fn mods_enter(&mut self) {
        let Some(i) = self.row.checked_sub(1) else { return };
        let n = self.s.mods.len();
        if i < n {
            if self.col < 2 {
                let on = self.col == 0;
                self.dirty |= self.s.mods[i].enabled != on;
                self.s.mods[i].enabled = on;
            } else if let Some(m) = self.mods_ui.as_mut() {
                m.held = Some(self.s.mods.clone());
            }
            self.sounds.push(Se::Select);
        } else if self.mods_ui.as_mut().is_some_and(|m| m.install(i - n)) {
            self.sounds.push(Se::Select);
        } else {
            self.sounds.push(Se::Error);
        }
    }

    /// The shown rows' buttons, relative to the page: conf3win's three for a pack, conf2win's first for an archive.
    fn mods_controls(&self) -> Vec<(usize, usize, [f32; 4])> {
        let top = self.mods_ui.as_ref().map_or(0, |m| m.top);
        let n = self.s.mods.len();
        let mut out = Vec::new();
        for (vis, i) in (top..self.mods_total()).take(super::modsui::VISIBLE).enumerate() {
            let y = Self::TOP + PITCH * vis as f32 + 14.0;
            if i < n {
                for (c, bx) in NARROW.iter().enumerate() {
                    out.push((i + 1, c, [*bx, y, 64.0, 16.0]));
                }
            } else {
                out.push((i + 1, 0, [WIDE[0], y, 88.0, 16.0]));
            }
        }
        out
    }

    fn mods_draw(&self, gu: &mut GameUi, x: f32, y: f32) {
        const N: u32 = 0x8080_8080;
        const ON: u32 = 0x8080_5020;
        let m = self.mods_ui.as_ref();
        let (top, held) = (m.map_or(0, |m| m.top), m.is_some_and(|m| m.held.is_some()));
        let n = self.s.mods.len();
        let total = self.mods_total();
        for (vis, i) in (top..total).take(super::modsui::VISIBLE).enumerate() {
            let hy = y + Self::TOP + PITCH * vis as f32;
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 20.0 - b[0], hy + 2.0 - b[1], N);
            }
            let (mut label, tint) = match (i < n, m) {
                (true, _) => (format!("{}. {}", i + 1, self.s.mods[i].name), if held && self.row == i + 1 { ON } else { N }),
                (false, Some(m)) => {
                    let p = &m.packs[i - n];
                    let name = p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                    match m.progress(&p.slug) {
                        Some(pm) => (format!("{}%  {name}", pm / 10), ON),
                        None => (name, N),
                    }
                }
                _ => continue,
            };
            while gu.label_width(&label) > W - 52.0 && label.pop().is_some() {}
            gu.label(&label, x + 32.0, hy, tint, false);
        }
        let captions: [&str; 3] = ["ON", "OFF", "Move"];
        let mut hand = None;
        for (row, c, b) in self.mods_controls() {
            let on = self.row == row && self.col == c;
            let d = if on && !held { -1.0 } else { 0.0 };
            let tint = if on { ON } else { N };
            button(gu, [x + b[0] + d, y + b[1] + d, b[2], b[3]], tint);
            let cap = if row - 1 < n { captions[c] } else { "Install" };
            gu.label(cap, x + b[0] + 5.0 + d, y + b[1] + 1.0 + d, tint, true);
            if row - 1 < n && c < 2 && self.s.mods[row - 1].enabled == (c == 0) {
                marker(gu, x + b[0], y + b[1], b[2]);
            }
            if on {
                hand = Some((x + b[0], y + b[1]));
            }
        }
        if let Some((hx, hy)) = hand {
            gu.cursor(hx, hy);
        }
        if total > super::modsui::VISIBLE {
            gu.scroll_bar(x + W - 12.0, y + Self::TOP, PITCH * super::modsui::VISIBLE as f32, total, super::modsui::VISIBLE, top);
        }
    }

    fn mods_help(&self) -> String {
        let n = self.s.mods.len();
        let m = self.mods_ui.as_ref();
        match self.row.checked_sub(1) {
            None => "Left / Right: another page of settings.".into(),
            Some(_) if m.is_some_and(|m| m.held.is_some()) => "Up / Down: move the pack. Enter: place it here. Escape: put it back.".into(),
            Some(i) if i < n => format!("Mod pack {} of {n}: a higher one's textures win. ON / OFF, or Move it. Takes effect on restart.", i + 1),
            Some(i) => match (m.and_then(|m| m.packs.get(i - n)), m.and_then(|m| m.error.clone())) {
                (_, Some(e)) => format!("The last install failed: {e}"),
                (Some(p), None) => format!(
                    "{} game file{} in {}: Install unpacks them into the mods folder, on and first. Takes effect on restart.",
                    p.files,
                    if p.files == 1 { "" } else { "s" },
                    p.path.display()
                ),
                (None, None) => String::new(),
            },
        }
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
    // Other widths: 105's two buttonto cells stretched (the left one to all but the right end's 40 px), base first.
    let left = (r[2] - 40.0).max(8.0);
    for (dx, t, blend) in [(2.0, 0x8001_0101, 0x44), (0.0, tint, 0)] {
        gu.cell_stretch_blend("menu    windowps", 105, 0, r[0] + dx, r[1] + dx, left, r[3], t, blend);
        gu.cell_stretch_blend("menu    windowps", 105, 1, r[0] + dx + left, r[1] + dx, r[2] - left, r[3], t, blend);
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

    /// Extras: pages as Settings (Maps, Timers, Misc.), opened on the page name; buttons pick with Left / Right and
    /// Enter; Up from the page name wraps to the last row.
    #[test]
    fn extras_buttons() {
        let c = Config::default();
        let mut ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
        assert_eq!((ui.tab, ui.row), (0, 0));
        ui.key(egui::Key::ArrowLeft);
        assert_eq!(ui.tab, 2);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowDown);
        assert_eq!((ui.row, ui.col), (1, 1));
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.map_overlay && ui.dirty);
        // The last row, Minimap Size (Small / Medium / Large): Large; then its place, top left; then the minimap on.
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowUp);
        assert_eq!(ui.row, 5);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.minimap_size, 2);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.minimap_dock, 1);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.minimap);
        // Timers: Timers Position (Left / Right / Top): the right dock; then the timers on.
        ui.goto(1, 2);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.timers_dock, 1);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.timers);
        // Misc.: Timestamps on All Lines.
        ui.goto(2, 2);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.stamp_all_lines);
    }

    /// Settings: Left / Right turn the page on a slider row, move between buttons on a buttons row, and always turn
    /// it with the page modifier; the page wraps.
    #[test]
    fn pages_turn() {
        let c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        ui.goto(0, 1);
        ui.key(egui::Key::ArrowLeft);
        assert_eq!(ui.tab, MODS);
        ui.goto(0, 2);
        ui.key(egui::Key::ArrowRight);
        assert_eq!((ui.tab, ui.col), (0, 1));
        ui.key_mod(egui::Key::ArrowRight, true);
        assert_eq!((ui.tab, ui.row), (1, 2));
        // The page name: opened on it, Up from the first row reaches it, Left / Right there turn the page.
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        assert_eq!(ui.row, 0);
        ui.goto(2, 1);
        ui.key(egui::Key::ArrowUp);
        assert_eq!(ui.row, 0);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.tab, 4);
    }

    /// Mods: ON / OFF mark a pack; Move carries it with Up / Down, Enter places it, Escape puts the order back.
    #[test]
    fn mods_page() {
        use crate::config::ModRef;
        let c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        let m = |n: &str, on: bool| ModRef { name: n.into(), enabled: on };
        ui.s.mods = vec![m("a", true), m("b", false), m("c", true)];
        ui.mods_ui = Some(super::super::modsui::Mods::empty());
        (ui.tab, ui.row, ui.col) = (MODS, 1, 0);
        let names = |ui: &SettingsUi| ui.s.mods.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join("");
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert!(!ui.s.mods[0].enabled && ui.dirty);
        ui.dirty = false;
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowDown);
        ui.key(egui::Key::Enter);
        assert_eq!((names(&ui), ui.row, ui.dirty), ("bac".into(), 2, true));
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowDown);
        assert_eq!(names(&ui), "bca");
        ui.key(egui::Key::Escape);
        assert_eq!((names(&ui), ui.row), ("bac".into(), 2));
        // Down past the last pack wraps to the page name; Left / Right there turn the page.
        ui.key(egui::Key::ArrowDown);
        ui.key(egui::Key::ArrowDown);
        assert_eq!(ui.row, 0);
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.tab, 0);
    }
}
