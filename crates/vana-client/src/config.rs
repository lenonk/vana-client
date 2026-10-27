//! Persisted settings and the app's data locations.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `base`/vana-client, moved there from the project's old name (ffxi-linux) the first time.
fn app_dir(base: Option<PathBuf>) -> PathBuf {
    let base = base.unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("vana-client");
    let old = base.join("ffxi-linux");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

pub fn data_dir() -> PathBuf {
    app_dir(dirs::data_dir())
}

/// The client's log: in the data dir, or beside an FFXI_CONFIG copy (test runs keep their own).
pub fn log_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("vana-client.log"),
        None => data_dir().join("vana-client.log"),
    }
}

/// A test run's copy of the config (FFXI_CONFIG): saving it never touches the user's file.
pub fn is_copy() -> bool {
    std::env::var_os("FFXI_CONFIG").is_some()
}

/// FFXI_CONFIG points test runs at a copy, so they never write the user's file.
fn config_path() -> PathBuf {
    std::env::var_os("FFXI_CONFIG").map(PathBuf::from).unwrap_or_else(|| app_dir(dirs::config_dir()).join("config.json"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Holds the "FINAL FANTASY XI" folder, laid out as the Windows installer does.
    pub game_dir: PathBuf,
    pub installed: bool,

    pub server: String,
    pub username: String,
    /// The login screen's Remember password (not retail), and the password sealed for this server and account
    /// (secret.rs; hex, empty when none).
    pub remember_password: bool,
    pub saved_password: String,
    pub auth_port: u16,
    pub data_port: u16,
    pub view_port: u16,
    pub profile_port: u16,
    pub irc_port: u16,
    /// Version string sent to the lobby; LSB rejects clients older than its CLIENT_VER.
    pub client_version: String,
    /// cpal output device id ("pipewire:..."); empty = the system default.
    pub audio_device: String,
    /// Frame limit in frames per second (one of FRAME_LIMITS), 0 = unlimited. Not a retail option;
    /// game ticks stay at 60 Hz and motions at 30 frames a second whatever this is.
    pub frame_limit: u32,
    /// Anisotropic filtering for scenery, models and the creator (1 = off, up to 16). Not a retail option.
    pub anisotropy: u16,
    /// Party and alliance rows show TP (retail config 0xae bit 0, default off; ours on, as the add-ons do).
    pub party_tp: bool,
    /// Extra: the map overlay (Ctrl / Alt + O; the add-ons' map overlay) and its opacity 0..1.
    pub map_overlay: bool,
    pub map_overlay_opacity: f32,
    /// Extra: Synthesize asks how many and repeats.
    pub synth_amount: bool,
    /// Extra (the timestamp add-on): retail's chat timestamps (config 0xb5) on every log line; seconds while 0xb5 is
    /// off.
    pub stamp_all_lines: bool,
    /// Extra (the Timers add-on): the timers window, its dock (0 left, 1 right, 2 top), its drawer out, folded
    /// sections (a bit each).
    pub timers: bool,
    pub timers_dock: u32,
    pub timers_out: bool,
    pub timers_folded: u8,
    /// Extra (the minimap add-ons): the minimap, its place (0 top right, 1 top left), size (0..2) and the map pixels
    /// across (the wheel's zoom).
    pub minimap: bool,
    pub minimap_dock: u32,
    pub minimap_size: u32,
    pub minimap_span: f32,
    /// Ours (the wheel, as modern MMOs): the camera's rest distance as a share of retail's.
    pub cam_dist: f32,
    /// Ours: resting's ticks as timed on the first rest seen, [first heal, interval, the server's look period] in
    /// seconds (client/resttick.rs).
    pub rest_tick: Option<[f32; 3]>,
    /// The drawn cursor's size (config 0021 off) in percent of retail's pixels on screen; 0 = auto (grows with the
    /// window height from 720). Not retail: it was sized for 640 x 480. The .ani cursors keep their own size.
    pub cursor_size: u32,
    /// Ours (user, 2026-10-25): containers whose sort window's Auto stays on, bit = container: a new item there sorts
    /// it again. Retail's Auto sorts once; Manual turns ours off.
    pub auto_sort: u32,
    /// The "FINAL FANTASY XI Config" tool's values by registry name (HKLM\...\FinalFantasyXI\0000..0043,
    /// read at start-up by FUN_10016c30); missing ones take TOOL_DEFAULTS.
    pub tool: std::collections::BTreeMap<String, u32>,
    /// Projection aspect from the config ratio (0x43 / 0xab, FUN_100147a0) instead of the window's. Not retail's
    /// default: retail always used the ratio.
    pub retail_aspect: bool,
    /// The monitor full screen and borderless go on, by its name (empty = the first). Not retail: Wayland can't
    /// tell which monitor is the main one.
    pub monitor: String,
    /// The graphics adapter by name (empty = the high-performance one); read at start-up. Not retail.
    pub gpu: String,
    /// Set once a software adapter (WARP, llvmpipe) was first seen and the background was halved for it. Not retail.
    pub software_gpu_seen: bool,
    /// FFXiMain's own options by config id (the in-game and lobby Config menus).
    pub game: crate::gamecfg::GameConfig,
    /// DAT overlays (not retail; dat::overlay): folders under mods_dir(), highest priority first.
    pub mods: Vec<ModRef>,
    /// Not retail (docs/modern-ui.md): the modern UI layer instead of the retail HUD windows it replaces, its panels'
    /// places by character, then panel, and the chat window's font size in points.
    pub modern_ui: bool,
    /// The modern layer's size in percent of its 1080-line design.
    pub modern_scale: u32,
    pub modern_places: std::collections::BTreeMap<String, ModernLayout>,
    pub modern_chat_font: f32,
    /// The modern edge windows folded to their tabs, by panel (the minimap, Wide Scan).
    pub modern_folded: std::collections::BTreeMap<String, bool>,
    /// The Wide Scan flyout's kinds shown: 1 players, 2 NPCs, 4 monsters.
    pub modern_scan_kinds: u8,
    /// The modern chat's All tab shows the battle lines too (the Battle tab has them alone).
    pub modern_battle_in_all: bool,
}

/// A modern UI panel's place: its left and top (or, for the panels that grow upward, its bottom `b`) as fractions of the
/// screen, so it keeps its place whatever the Interface Size or the resolution; for the chat window its room in points
/// (0: the default); or docked on another panel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModernPlace {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub b: f32,
    pub dock: Option<ModernDock>,
}

/// A panel sitting on another: `gap` points between its bottom and the other's top, `dx` from the other's left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModernDock {
    pub to: String,
    pub gap: f32,
    pub dx: f32,
}

/// A character's panel places by panel id.
pub type ModernLayout = std::collections::BTreeMap<String, ModernPlace>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModRef {
    pub name: String,
    pub enabled: bool,
}

/// Where mod folders live.
pub fn mods_dir() -> PathBuf {
    data_dir().join("mods")
}

/// The enabled mods' folders, highest priority first (none with FFXI_NO_MODS set).
pub fn enabled_mod_dirs() -> Vec<PathBuf> {
    if std::env::var_os("FFXI_NO_MODS").is_some() {
        return Vec::new();
    }
    Config::load().mods.iter().filter(|m| m.enabled).map(|m| mods_dir().join(&m.name)).filter(|d| d.is_dir()).collect()
}

/// Merged overlay DATs (rebuilt when a source changes).
pub fn merged_dir() -> PathBuf {
    app_dir(dirs::cache_dir()).join("merged")
}

/// Config-tool values with our defaults (FFXiMain's own default in the comment where it differs).
pub const TOOL_DEFAULTS: &[(&str, u32)] = &[
    ("0000", 1),    // MIP mapping (FFXiMain 0)
    ("0001", 1280), // window width (FFXiMain 640, the tool 800)
    ("0002", 800),  // window height (480 / 600)
    ("0003", 0),    // background width, 0 = the window's (FFXiMain 512, the tool 800)
    ("0004", 0),    // background height (512 / 600)
    ("0007", 1),    // sound
    ("0011", 2),    // environment animation (FFXiMain 1)
    ("0017", 1),    // bump mapping (FFXiMain 0)
    ("0018", 1),    // texture compression
    ("0019", 1),    // map compression
    ("0021", 0),    // hardware mouse
    ("0022", 1),    // opening movie
    ("0023", 0),    // simplified character creation
    ("0028", 0),    // gamma base 0.0
    ("0029", 12),   // sound effect channels
    ("0030", 0),    // 3D LCD mode
    ("0034", 1),    // window mode (FFXiMain 0 full screen)
    ("0035", 1),    // sound in the background (FFXiMain 0)
    ("0036", 0),    // font quality
    ("0037", 0),    // UI width, 0 = the window's
    ("0038", 0),    // UI height
    ("0040", 0),    // graphics stabilization
    ("0043", 0),    // screenshots at screen size
];

/// Choices for `frame_limit` (0 = unlimited).
pub const FRAME_LIMITS: [u32; 14] = [30, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 300, 360, 0];

impl Default for Config {
    fn default() -> Self {
        Self {
            game_dir: data_dir().join("game"),
            installed: false,
            server: "127.0.0.1".into(),
            username: String::new(),
            remember_password: false,
            saved_password: String::new(),
            auth_port: 54231,
            data_port: 54230,
            view_port: 54001,
            profile_port: 51220,
            irc_port: 51240,
            client_version: "30261006_0".into(),
            audio_device: String::new(),
            frame_limit: 300,
            anisotropy: 16,
            party_tp: true,
            map_overlay: false,
            map_overlay_opacity: 0.6,
            synth_amount: false,
            stamp_all_lines: false,
            timers: false,
            timers_dock: 0,
            timers_out: true,
            timers_folded: 0,
            minimap: false,
            minimap_dock: 0,
            minimap_size: 1,
            minimap_span: 160.0,
            cam_dist: 1.0,
            rest_tick: None,
            cursor_size: 0,
            auto_sort: 0,
            mods: Vec::new(),
            modern_ui: false,
            modern_scale: 60,
            modern_places: Default::default(),
            modern_chat_font: 15.0,
            modern_folded: Default::default(),
            modern_scan_kinds: 7,
            modern_battle_in_all: true,
            tool: Default::default(),
            retail_aspect: false,
            monitor: String::new(),
            gpu: String::new(),
            software_gpu_seen: false,
            game: Default::default(),
        }
    }
}

impl Config {
    /// The cursor's on-screen scale for a window `height` pixels high.
    pub fn cursor_scale(&self, height: u32) -> f32 {
        match self.cursor_size {
            0 => ((height as f32 / 720.0) * 2.0).round().max(2.0) / 2.0,
            n => n as f32 / 100.0,
        }
    }

    pub fn load() -> Self {
        std::fs::read(config_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn ffxi_dir(&self) -> PathBuf {
        self.game_dir.join("FINAL FANTASY XI")
    }

    /// A Config-tool value (registry name "0000".."0043").
    pub fn tool(&self, name: &str) -> u32 {
        self.tool.get(name).copied().or_else(|| TOOL_DEFAULTS.iter().find(|d| d.0 == name).map(|d| d.1)).unwrap_or(0)
    }

    /// Sets a Config-tool value; the default is not stored.
    pub fn set_tool(&mut self, name: &str, v: u32) {
        if TOOL_DEFAULTS.iter().any(|d| d.0 == name && d.1 == v) {
            self.tool.remove(name);
        } else {
            self.tool.insert(name.to_string(), v);
        }
    }

    /// Window size (0001 / 0002).
    pub fn window_size(&self) -> [u32; 2] {
        [self.tool("0001"), self.tool("0002")].map(|v| v.clamp(320, 16384))
    }

    /// Window mode 0034: 0 full screen, 1 windowed, 2 borderless at the window size, 3 borderless at the
    /// desktop size.
    pub fn window_mode(&self) -> u32 {
        self.tool("0034").min(3)
    }

    /// The scene's size for a window of `window` pixels (0003 / 0004; 0 = the window's, not retail).
    pub fn background(&self, window: [u32; 2]) -> [u32; 2] {
        match [self.tool("0003"), self.tool("0004")] {
            [w, h] if w > 0 && h > 0 => [w.min(8192), h.min(8192)],
            _ => window,
        }
    }

    /// The UI layout size (0037 / 0038, 0 = the window's): laid out there and stretched to the window.
    pub fn ui_size(&self, window: [u32; 2]) -> [u32; 2] {
        match [self.tool("0037"), self.tool("0038")] {
            [w, h] if w >= 320 && h >= 240 => [w.min(window[0].max(320) * 4), h.min(window[1].max(240) * 4)],
            _ => window,
        }
    }

    /// Gamma base 0028 (an f32 in the DWORD, -1..1).
    pub fn gamma_base(&self) -> f32 {
        f32::from_bits(self.tool("0028")).clamp(-1.0, 1.0)
    }

    /// The frame limit in effect: FFXI_FRAME_LIMIT overrides the file (test runs), 0 = unlimited.
    pub fn frame_limit(&self) -> u32 {
        std::env::var("FFXI_FRAME_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(self.frame_limit)
    }
}
