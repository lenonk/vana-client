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

/// A server-sent character name as a file name: letters and digits as they are, anything else (and a Windows device
/// name) as '_' and the name's bytes in hex, so the file stays in its folder and two names never share one.
fn name_file(name: &str) -> String {
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric()) && !(cfg!(windows) && device_name(name)) {
        return format!("{name}.json");
    }
    format!("_{}.json", name.bytes().map(|b| format!("{b:02x}")).collect::<String>())
}

/// CON, PRN, AUX, NUL, COM0..9 and LPT0..9, which Windows opens as devices whatever the extension.
fn device_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), "con" | "prn" | "aux" | "nul") || (n.len() == 4 && (n.starts_with("com") || n.starts_with("lpt")) && n.as_bytes()[3].is_ascii_digit())
}

/// A character's saved chat lines (the modern chat's history): in the data dir, or beside an FFXI_CONFIG copy.
pub fn chat_history_path(name: &str) -> PathBuf {
    let dir = match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("chat"),
        None => data_dir().join("chat"),
    };
    dir.join(name_file(name))
}

/// The regime folder: in the data dir, or beside an FFXI_CONFIG copy, so a test run never writes the player's.
fn regime_dir() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("regime"),
        None => data_dir().join("regime"),
    }
}

/// A character's training regime state (client/regime.rs): a folder per server ("127.0.0.1-54002", its search
/// address) and a file per character, so a same-named character on another server has its own.
pub fn regime_path(server: &str, name: &str) -> PathBuf {
    regime_dir().join(server).join(name_file(name))
}

/// Where earlier builds kept a character's regime (by name alone), read when the server's own file is not there yet;
/// with FFXI_CONFIG, the data dir's by-name file too.
pub fn regime_legacy_paths(name: &str) -> Vec<PathBuf> {
    let mut v = vec![regime_dir().join(name_file(name))];
    v.extend(regime_moved(std::env::var_os("FFXI_CONFIG").is_some()).map(|d| d.join(name_file(name))));
    v
}

/// With FFXI_CONFIG, the data dir's file for this server and character: builds up to 1.0.5 kept every run's regime
/// there, so it is read before the by-name files while the one beside the config is not there yet.
pub fn regime_moved_path(server: &str, name: &str) -> Option<PathBuf> {
    regime_moved(std::env::var_os("FFXI_CONFIG").is_some()).map(|d| d.join(server).join(name_file(name)))
}

/// The data dir's regime folder when FFXI_CONFIG puts the current one elsewhere.
fn regime_moved(config_copy: bool) -> Option<PathBuf> {
    config_copy.then(|| data_dir().join("regime"))
}

/// The servers' certificate fingerprints (net/tls.rs): in the data dir, or beside an FFXI_CONFIG copy.
pub fn known_servers_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("known_servers.json"),
        None => data_dir().join("known_servers.json"),
    }
}

/// The change notes an update left for the restarted client (client/notes.rs): in the data dir, or beside an
/// FFXI_CONFIG copy.
pub fn whats_new_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("whats-new.json"),
        None => data_dir().join("whats-new.json"),
    }
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

/// A login that worked: the server, the account (the Square Enix ID for PlayOnline), PlayOnline's member ID, and its
/// own Remember password with the password(s) sealed for that server and account (secret.rs).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedLogin {
    pub server: String,
    pub username: String,
    pub pol_id: String,
    pub remember_password: bool,
    pub saved_password: String,
    /// Fields of a newer client, kept as they are.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

/// Server names compare without case or outer spaces ("PlayOnline", "playonline").
fn same_server(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// What a server's rest ticks look like, as learnt (client/resttick.rs keeps it moving).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RestModel {
    /// From the rest's start to the first heal seen (seconds), a mean over rests.
    pub first: f32,
    /// The intervals after each heal, means over rests; the last stands for every later one.
    pub gaps: Vec<f32>,
    /// How many rests each value holds ([first, gaps...]), capped so later rests still move it.
    pub seen: Vec<u32>,
    /// The server's look period (its ticks land on the first look after their time); 0: on time.
    pub period: f32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Holds the "FINAL FANTASY XI" folder, laid out as the Windows installer does.
    pub game_dir: PathBuf,
    pub installed: bool,

    pub server: String,
    /// The login's account; the Square Enix ID for PlayOnline.
    pub username: String,
    /// PlayOnline's member ID (8 characters), for the PlayOnline server.
    pub pol_id: String,
    /// Logins that worked, most recent first: the login screen's Server and Account lists (not retail).
    pub logins: Vec<SavedLogin>,
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
    /// Anti-aliasing of the 3D scene: 0 off (retail), 1 SMAA (was FXAA before 2026-11-04). Not a retail option.
    pub antialias: u32,
    /// Colour grading of the 3D scene: 0 off (retail), 1 Vivid, 2 Warm, 3 Cool, 4 Film (render/scale.rs). Not a
    /// retail option.
    pub grading: u32,
    /// How far scenery, actors and effects draw, in percent of retail's distance (the zone's, scaled by the
    /// Clipping setting); 0 = no limit. Not a retail option.
    pub draw_distance: u32,
    /// Milliseconds between the datagrams sent to a private server's zone server (50..400; 400 is the game's, which a
    /// PlayOnline sign-in always keeps). The server answers each one, so its updates come sooner. Not a retail option.
    pub send_interval: u32,
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
    /// Extra (the distance / target-info add-ons): the target's distance in the target window, and its HP percent in the
    /// classic one (the modern one shows it anyway).
    pub target_distance: bool,
    pub target_hpp: bool,
    /// Not retail (Settings' Lamp Lights): real point lights at the zones' lamps in place of their baked lamplight
    /// (client/lamps.rs).
    pub lamp_lights: bool,
    /// Character shadows: 0 retail's (blob / silhouette), 1 real (a shadow map from the sun, render/sunshadow.rs,
    /// retail's where there is no sun). Not a retail option.
    pub actor_shadows: u32,
    /// The real shadows' map size in texels (1024, 2048 or 4096; render/sunshadow.rs). Not a retail option.
    pub shadow_map: u32,
    /// Ambient occlusion on the scenery (render/ssao.rs). Not a retail option.
    pub ssao: bool,
    /// Extra (the clock add-ons): Earth time, the moon and the next day beside the Vana'diel clock (client/clock.rs).
    pub earth_clock: bool,
    /// The Game Master window (ours, client/gm.rs): its request (c2s 0x1F0) goes out at a zone-in only while this is
    /// on and only to a private server. No Settings row: only a server with its GM extension can use it, so it is set
    /// in config.json by hand.
    pub gm_window: bool,
    /// The version whose change notes were last shown (client/notes.rs): a newer client shows the notes since. A
    /// config from before the notes has none, so its first newer build shows that build's own.
    #[serde(default)]
    pub notes_seen: String,
    /// Extra (the trust chatter filters): trusts' party lines (battle message 711) are not printed.
    pub hide_trust_speech: bool,
    /// Extra (Battlemod's filters): "readies" / "starts casting" lines only for us and monsters.
    pub hide_ready_lines: bool,
    /// Extras (Battlemod's simplify and colours, client/battlemod.rs): battle lines in their short form; names in
    /// battle lines coloured by who they are.
    pub short_battle: bool,
    pub battle_name_colours: bool,
    /// Extra (the text auto-advance add-ons): an event message waiting for a key goes on by itself after this many
    /// seconds; 0 off.
    pub auto_advance: u32,
    /// Extra (the Timers add-on): the timers window, its dock (0 left, 1 right, 2 top), its drawer out, folded
    /// sections (a bit each).
    pub timers: bool,
    pub timers_dock: u32,
    pub timers_out: bool,
    pub timers_folded: u8,
    /// Extra (ours): the training regime window (client/regime.rs) and its drawer out.
    pub regime: bool,
    pub regime_out: bool,
    /// Extra (the skillchain add-ons): in a party, the chains it can make and the open one on our target.
    pub skillchain_helper: bool,
    /// Extra (the minimap add-ons): the minimap, its place (0 top right, 1 top left), size (0..2) and the map pixels
    /// across (the wheel's zoom).
    pub minimap: bool,
    pub minimap_dock: u32,
    pub minimap_size: u32,
    pub minimap_span: f32,
    /// Ours: resting's ticks as learnt per server (by its search address, "ip-port"; client/resttick.rs).
    pub rest_models: std::collections::BTreeMap<String, RestModel>,
    /// The drawn cursor's size (config 0021 off) in percent of retail's pixels on screen; 0 = auto (grows with the
    /// window height from 720). Not retail: it was sized for 640 x 480. The .ani cursors keep their own size.
    pub cursor_size: u32,
    /// A key press hides the mouse cursor until the mouse moves (retail, FUN_10125f70(0)); off keeps it shown (ours).
    pub keys_hide_cursor: bool,
    /// Ours (user, 2026-11-02): Mouse/Cam.'s Camera Keys, the keys' X / Y sense apart from the mouse: 0 Same (the
    /// camera rows, keys and mouse alike, as retail), 1 normal, 2 inverted.
    pub cam_keys_x: u32,
    pub cam_keys_y: u32,
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
    /// How opaque the modern windows' backgrounds are (0.1 ..= 1).
    pub modern_opacity: f32,
    pub modern_places: std::collections::BTreeMap<String, ModernLayout>,
    pub modern_chat_font: f32,
    /// The modern edge windows folded to their tabs, by panel (the minimap, Wide Scan).
    pub modern_folded: std::collections::BTreeMap<String, bool>,
    /// The Wide Scan flyout's kinds shown: 2 NPCs, 4 monsters (players, 1, always: LSB sends none).
    pub modern_scan_kinds: u8,
    /// The modern chat's All tab shows the battle lines too (the Battle tab has them alone).
    pub modern_battle_in_all: bool,
    /// The modern chat merges an action's lines that differ only in their number (BattleMod's condensing).
    pub modern_condense: bool,
    /// The modern chat's tabs hidden by its settings, by name (All always shows).
    pub modern_chat_hidden: Vec<String>,
    /// The list search's recent picks by character, newest first (listsearch.rs).
    pub modern_recent: std::collections::BTreeMap<String, Vec<String>>,
    /// The modern UI's mode presets (client/modern/modes.rs).
    pub modern_modes: ModernModes,
    /// Favourites by character, in the strip's order (client/favs.rs), and whether the strip shows (/favs).
    pub modern_favourites: std::collections::BTreeMap<String, Vec<String>>,
    pub modern_fav_strip: bool,
    /// How fast a mouse drag turns the camera, in % of the old fixed speed (ours; the controller is not touched).
    pub mouse_camera_speed: u32,
    /// Read a game controller (client/gamepad.rs; retail: padmode000's first flag).
    pub gamepad: bool,
    /// Stick travel ignored around the centre as a fraction of full: retail's 42 of 127 (ours round).
    pub gamepad_deadzone: f32,
    /// Read the pad while the window is in the background (padmode000's [4], DISCL_BACKGROUND).
    pub gamepad_background: bool,
    /// The button settings in retail's padsin000 format (FFXiPadConfig's registry value: 32 numbers, XInput button
    /// numbers or axis codes 32 + n); empty = gamepad::DEFAULT_PADSIN.
    pub gamepad_padsin: String,
    /// padexsin000, the map used while slot 34 is held; empty = gamepad::DEFAULT_PADEXSIN.
    pub gamepad_padexsin: String,
    /// LAN voice chat (ours, hidden: Ctrl+Alt+Shift+V).
    pub voice: crate::voice::Settings,
    /// Fields a newer client wrote, kept and saved back as they are: an older client drops nothing it doesn't know.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

/// The modern UI's mode presets: per mode (Exploration, Combat, Management) the HUD panels it hides, the panels shown
/// in every mode, and whether panels fade in and out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModernModes {
    pub hidden: [Vec<String>; 3],
    pub pinned: Vec<String>,
    pub fades: bool,
}

impl Default for ModernModes {
    fn default() -> Self {
        // Combat hides the location panel (user, 2026-11-01) and the training regime; the skillchain helper only in
        // combat.
        let v = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect();
        Self { hidden: [v(&["skillchains"]), v(&["where", "regime"]), v(&["skillchains"])], pinned: Vec::new(), fades: true }
    }
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
    /// An edge window let go away from the edges: it stands at x / y with its whole frame.
    pub float: bool,
    /// An edge window held by the top (1) or the bottom (2) edge, x its left; 0 the left or right edge by x's sign.
    pub edge: u8,
}

/// A panel sitting on another: `gap` points between its bottom and the other's top, `dx` from the other's left, or
/// from its right (`right`) for one placed from the right edge, whose left moves as it folds or grows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModernDock {
    pub to: String,
    pub gap: f32,
    pub dx: f32,
    #[serde(default)]
    pub right: bool,
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
            pol_id: String::new(),
            logins: Vec::new(),
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
            antialias: 0,
            grading: 0,
            draw_distance: 0,
            send_interval: 400,
            party_tp: true,
            map_overlay: false,
            map_overlay_opacity: 0.6,
            synth_amount: false,
            stamp_all_lines: false,
            target_distance: false,
            target_hpp: false,
            earth_clock: false,
            gm_window: false,
            notes_seen: env!("CARGO_PKG_VERSION").into(),
            lamp_lights: true,
            actor_shadows: 0,
            shadow_map: 2048,
            ssao: false,
            hide_trust_speech: false,
            hide_ready_lines: false,
            short_battle: false,
            battle_name_colours: false,
            auto_advance: 0,
            timers: false,
            timers_dock: 0,
            timers_out: true,
            timers_folded: 0,
            regime: false,
            skillchain_helper: false,
            regime_out: true,
            minimap: false,
            minimap_dock: 0,
            minimap_size: 1,
            minimap_span: 160.0,
            rest_models: Default::default(),
            cursor_size: 0,
            keys_hide_cursor: true,
            cam_keys_x: 0,
            cam_keys_y: 0,
            auto_sort: 0,
            mods: Vec::new(),
            modern_ui: false,
            modern_scale: 60,
            modern_opacity: 0.85,
            modern_places: Default::default(),
            modern_chat_font: 15.0,
            modern_folded: Default::default(),
            modern_scan_kinds: 7,
            modern_battle_in_all: true,
            modern_condense: true,
            modern_chat_hidden: Vec::new(),
            modern_recent: Default::default(),
            modern_modes: Default::default(),
            modern_favourites: Default::default(),
            modern_fav_strip: true,
            mouse_camera_speed: 100,
            gamepad: true,
            gamepad_deadzone: 0.33,
            gamepad_background: false,
            gamepad_padsin: String::new(),
            gamepad_padexsin: String::new(),
            voice: Default::default(),
            unknown: Default::default(),
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
        let path = config_path();
        let mut c: Self = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                // Copied aside first, so the save that follows can't lose it.
                let aside = path.with_extension("json.bad");
                log::error!("{}: {e}; copied to {} and the defaults used", path.display(), aside.display());
                let _ = std::fs::copy(&path, &aside);
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        // A config from before the lists: its one login starts them.
        if c.logins.is_empty() && !c.server.trim().is_empty() && !c.username.trim().is_empty() {
            c.remember_login();
        }
        c
    }

    /// The servers logged in to, most recent first.
    pub fn servers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.logins {
            if !out.iter().any(|s| same_server(s, &l.server)) {
                out.push(l.server.clone());
            }
        }
        out
    }

    /// The accounts logged in with on a server, most recent first.
    pub fn accounts(&self, server: &str) -> Vec<String> {
        self.logins.iter().filter(|l| same_server(&l.server, server)).map(|l| l.username.clone()).collect()
    }

    /// The saved login for a server and account; with no account, the server's most recent.
    pub fn saved_login(&self, server: &str, username: Option<&str>) -> Option<&SavedLogin> {
        self.logins.iter().find(|l| same_server(&l.server, server) && username.is_none_or(|u| l.username == u))
    }

    /// Makes a saved login the form's: its server, account, PlayOnline ID, Remember password and sealed passwords.
    pub fn use_login(&mut self, l: &SavedLogin) {
        self.server = l.server.clone();
        self.username = l.username.clone();
        self.pol_id = l.pol_id.clone();
        self.remember_password = l.remember_password;
        self.saved_password = l.saved_password.clone();
    }

    /// The form's login, first in the lists (after a login that worked, or a change to its Remember password).
    pub fn remember_login(&mut self) {
        let (server, username) = (self.server.trim().to_string(), self.username.trim().to_string());
        // A newer client's fields of this login stay with it.
        let unknown = self.logins.iter().find(|o| same_server(&o.server, &server) && o.username == username).map(|o| o.unknown.clone()).unwrap_or_default();
        let l = SavedLogin { server, username, pol_id: self.pol_id.clone(), remember_password: self.remember_password, saved_password: self.saved_password.clone(), unknown };
        self.logins.retain(|o| !(same_server(&o.server, &l.server) && o.username == l.username));
        self.logins.insert(0, l);
    }

    /// Drops a saved login (with its password); with no account, every login of the server.
    pub fn forget_login(&mut self, server: &str, username: Option<&str>) {
        self.logins.retain(|l| !(same_server(&l.server, server) && username.is_none_or(|u| l.username == u)));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The first key or value of `a` that `b` lacks or changes (objects compared key by key, recursively).
    fn lost(a: &serde_json::Value, b: &serde_json::Value, at: &str) -> Option<String> {
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                a.iter().find_map(|(k, v)| b.get(k).map_or_else(|| Some(format!("{at}.{k} dropped")), |w| lost(v, w, &format!("{at}.{k}"))))
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => {
                a.iter().zip(b).enumerate().find_map(|(i, (v, w))| lost(v, w, &format!("{at}[{i}]")))
            }
            _ => (a != b).then(|| format!("{at}: {a} became {b}")),
        }
    }

    /// The config as save writes it, read back (to_value would widen an f32: 0.33 to 0.33000001...).
    fn written(c: &Config) -> serde_json::Value {
        serde_json::from_slice(&serde_json::to_vec_pretty(c).unwrap()).unwrap()
    }

    fn kept(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        lost(a, b, "").inspect(|l| eprintln!("lost: {l}")).is_none()
    }

    /// A newer client's fields, at the top and in a saved login, survive a load and a save.
    #[test]
    fn unknown_fields_kept() {
        let text = r#"{"server": "x", "username": "u", "future": {"a": [1, 2]}, "logins": [{"server": "x", "username": "u", "later": 7}]}"#;
        let mut c: Config = serde_json::from_str(text).unwrap();
        assert_eq!(c.unknown.get("future"), Some(&serde_json::json!({"a": [1, 2]})));
        c.remember_login();
        let out = written(&c);
        assert!(kept(&serde_json::from_str(text).unwrap(), &out), "{out}");
    }

    /// The real config (when there is one) loses nothing through a load and a save (read only).
    #[test]
    fn real_config_round_trips() {
        let Ok(b) = std::fs::read(config_path()) else { return };
        let Ok(before) = serde_json::from_slice::<serde_json::Value>(&b) else { return };
        let c: Config = serde_json::from_slice(&b).expect("the config parses");
        assert!(kept(&before, &written(&c)));
    }

    /// Only a config copy (FFXI_CONFIG) moves the regime folder, so only then is the data dir's read as well.
    #[test]
    fn regime_moved_with_a_config_copy() {
        assert_eq!(super::regime_moved(false), None);
        assert_eq!(super::regime_moved(true), Some(super::data_dir().join("regime")));
    }

    #[test]
    fn name_file_stays_in_its_folder() {
        assert_eq!(name_file("Steven"), "Steven.json");
        assert_eq!(name_file("../x"), "_2e2e2f78.json");
        assert_eq!(name_file("a/b\\c"), "_612f625c63.json");
        assert_eq!(name_file(""), "_.json");
        // Names that differ only in a replaced character keep separate files.
        assert_ne!(name_file("a/b"), name_file("a\\b"));
        assert_eq!(chat_history_path("../x").file_name().unwrap(), "_2e2e2f78.json");
        assert!(device_name("Con") && device_name("nul") && device_name("COM1") && device_name("Lpt9"));
        assert!(!device_name("Conan") && !device_name("Comx") && !device_name("Nine"));
    }
}
