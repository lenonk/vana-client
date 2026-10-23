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
    pub auth_port: u16,
    pub data_port: u16,
    pub view_port: u16,
    pub profile_port: u16,
    pub irc_port: u16,
    /// Version string sent to the lobby; LSB rejects clients older than its CLIENT_VER.
    pub client_version: String,
    /// Music and effect volume factors 0..1 (the client's 0..127 levels are scaled by these).
    pub music_volume: f32,
    pub effects_volume: f32,
    /// cpal output device id ("pipewire:..."); empty = the system default.
    pub audio_device: String,
    /// Frame limit in frames per second (one of FRAME_LIMITS), 0 = unlimited. Not a retail option;
    /// game ticks stay at 60 Hz and motions at 30 frames a second whatever this is.
    pub frame_limit: u32,
    /// Anisotropic filtering for scenery, models and the creator (1 = off, up to 16). Not a retail option.
    pub anisotropy: u16,
    /// Party and alliance rows show TP (retail config 0xae bit 0, default off; ours on, as the add-ons do).
    pub party_tp: bool,
    /// The map overlay's opacity 0..1 (Ctrl / Alt + O; not retail, the add-ons' map overlay).
    pub map_overlay_opacity: f32,
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
    /// FFXiMain's own options by config id (the in-game and lobby Config menus).
    pub game: crate::gamecfg::GameConfig,
    /// DAT overlays (not retail; dat::overlay): folders under mods_dir(), highest priority first.
    pub mods: Vec<ModRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModRef {
    pub name: String,
    pub enabled: bool,
}

/// Where mod folders live.
pub fn mods_dir() -> PathBuf {
    data_dir().join("mods")
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
            auth_port: 54231,
            data_port: 54230,
            view_port: 54001,
            profile_port: 51220,
            irc_port: 51240,
            client_version: "30261006_0".into(),
            music_volume: 1.0,
            effects_volume: 1.0,
            audio_device: String::new(),
            frame_limit: 300,
            anisotropy: 16,
            party_tp: true,
            map_overlay_opacity: 0.6,
            mods: Vec::new(),
            tool: Default::default(),
            retail_aspect: false,
            monitor: String::new(),
            gpu: String::new(),
            game: Default::default(),
        }
    }
}

impl Config {
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
