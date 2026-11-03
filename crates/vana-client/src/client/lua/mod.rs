//! Lua mods (docs/lua-mods.md): a VM per enabled mod with a mod.toml in its folder under mods/, created when the
//! character enters the world and dropped at logout. Once a frame the mod phase delivers events, commands, timers and
//! wake-ups; what the mods asked for is applied after it.

mod actions;
mod api;
mod devices;
pub(super) mod audio;
mod docs;
mod draw;
mod ipc;
pub(super) mod lobby;
mod process;
mod net;
mod hud;
pub(super) mod input;
mod manifest;
mod native;
mod pattern;
pub(super) mod render;
mod runtime;
mod sock;
mod state;
mod vm;
pub(super) mod window;

use super::World;
use std::path::PathBuf;
use vm::Vm;

/// What a mod asked the client to do, applied after the mod phase.
pub(super) enum Op {
    /// A line with a mode (its colour, filters and indent), or only a mode's colour.
    Chat { text: String, mode: Option<u8>, colour: Option<u8> },
    Print(String),
    Run(String),
    Act(actions::Action),
    /// A key the mod pressed (input.press / input.send).
    Key(input::Synth),
    /// The chat line opened with this text, or closed.
    Line { text: String, open: bool },
    /// Text for the clipboard.
    Copy(String),
    /// The whole interface shown or hidden (the UI-hide key's state).
    Interface(bool),
    /// A game config value set, as the Config window does.
    Config(u16, u32),
    /// The camera's distance (the zoom's scale).
    CameraDistance(f32),
    /// The camera's pan speed scale, farthest wheel distance, distance while engaged (Some(None) clears it).
    Camera(Option<f32>, Option<f32>, Option<Option<f32>>),
    Sound(audio::Sound),
    /// A message to the same mod in the other clients: (mod, the character it is for, text).
    Ipc(String, Option<String>, String),
    /// A mod takes the music (a track or silence) or gives it back (None).
    Music(String, Option<audio::Music>),
    /// The game window moved, sized or given the focus.
    Window(WinReq),
    /// A screenshot under the screenshots folder.
    Shot(String),
    /// The clipboard read for client.paste.
    Paste,
    /// A behaviour switch set (client.set_option): (mod, name, value).
    Option(String, String, serde_json::Value),
    /// A timer in the Timers window added or replaced (mod, name, seconds, section, group, icon kind and id, total,
    /// fills), or removed (seconds None), or all the mod's removed (name None).
    Timer { mod_id: String, name: Option<String>, secs: Option<f32>, section: usize, group: Option<String>, icon: Option<(String, u16)>, total: f32, fills: bool },
    /// A lobby action (sign in, play a character), for the app before login.
    Lobby(lobby::Act),
    /// A recording started (mod, file, frames a second) or stopped (client.record).
    Record(Option<(String, std::path::PathBuf, u32)>),
}

/// What a mod asked of the game window.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::client) struct WinReq {
    pub pos: Option<(i32, i32)>,
    pub size: Option<(u32, u32)>,
    pub focus: bool,
    /// Always on top, minimised, borderless full screen, the frame, kept out of screen captures.
    pub top: Option<bool>,
    pub minimised: Option<bool>,
    pub fullscreen: Option<bool>,
    pub border: Option<bool>,
    pub protected: Option<bool>,
}

/// The behaviour switches (client.set_option) by name; false for an unknown name or value.
pub(super) const OPTIONS: [&str; 19] = [
    "macro_palette_delay", "ah_result_delay", "equipset_delay", "log_slide", "cutscene_speed", "animation_rate", "clear_colour", "ambient", "minimap_span",
    "move_lock", "all_maps", "dead_commands", "compass_monsters", "player_collision", "occlusion", "menu_reach", "send_interval", "wireframe", "ttf_text",
];

/// The switches retail has off.
const OFF: [&str; 5] = ["all_maps", "dead_commands", "compass_monsters", "wireframe", "ttf_text"];

/// An {r, g, b} (0..255) as gamma-space floats; false / nil: none.
fn rgb_of(v: &serde_json::Value) -> Option<Option<[f32; 3]>> {
    if v.is_null() || v == &serde_json::Value::Bool(false) {
        return Some(None);
    }
    let get = |k: &str, i: usize| v.get(k).or_else(|| v.get(i)).and_then(|x| x.as_f64());
    Some(Some([get("r", 0)?, get("g", 1)?, get("b", 2)?].map(|c| (c / 255.0).clamp(0.0, 1.0) as f32)))
}

fn rgb_json(c: Option<[f32; 3]>) -> serde_json::Value {
    c.map_or(serde_json::Value::Bool(false), |c| serde_json::json!({"r": (c[0] * 255.0).round(), "g": (c[1] * 255.0).round(), "b": (c[2] * 255.0).round()}))
}

fn set_option(w: &mut World, name: &str, v: &serde_json::Value) -> bool {
    let b = v.as_bool();
    match (name, b) {
        ("macro_palette_delay", Some(b)) => w.opts.macro_delay = b,
        ("ah_result_delay", Some(b)) => w.opts.ah_delay = b,
        ("equipset_delay", Some(b)) => w.opts.equipset_delay = b,
        ("move_lock", Some(b)) => w.opts.move_lock = b,
        ("all_maps", Some(b)) => w.opts.all_maps = b,
        ("dead_commands", Some(b)) => w.opts.dead_commands = b,
        ("compass_monsters", Some(b)) => w.opts.radar_mobs = b,
        ("player_collision", Some(b)) => w.opts.player_bump = b,
        ("occlusion", Some(b)) => w.opts.occlusion = b,
        ("menu_reach", Some(b)) => w.opts.menu_reach = b,
        ("wireframe", Some(b)) => w.opts.wireframe = b,
        ("ttf_text", Some(b)) => w.opts.ttf_text = b,
        ("log_slide", Some(b)) => (w.log.anim.instant, w.log2.anim.instant, w.modern_log.anim.instant) = (!b, !b, !b),
        ("cutscene_speed", _) => match v.as_f64() {
            Some(k) => w.opts.cutscene_speed = (k as f32).clamp(1.0, 4.0),
            None => return false,
        },
        ("animation_rate", _) => match v.as_f64() {
            Some(k) => w.opts.anim_rate = (k as f32).clamp(0.25, 4.0),
            None => return false,
        },
        ("clear_colour", _) => match rgb_of(v) {
            Some(c) => w.opts.clear = c,
            None => return false,
        },
        ("ambient", _) => match rgb_of(v) {
            Some(c) => w.opts.ambient = c,
            None => return false,
        },
        ("send_interval", _) => match v.as_f64() {
            Some(ms) => w.net.stats.send_ms.store((ms as u32).clamp(50, 1000), std::sync::atomic::Ordering::Relaxed),
            None => return false,
        },
        ("minimap_span", _) => match v.as_f64() {
            Some(k) => (w.minimap_span, w.minimap_dirty) = ((k as f32).clamp(super::minimap::SPAN.0, super::minimap::SPAN.1), true),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// A switch's value now.
pub(super) fn option(w: &World, name: &str) -> Option<serde_json::Value> {
    use serde_json::json;
    Some(match name {
        "macro_palette_delay" => json!(w.opts.macro_delay),
        "ah_result_delay" => json!(w.opts.ah_delay),
        "equipset_delay" => json!(w.opts.equipset_delay),
        "move_lock" => json!(w.opts.move_lock),
        "all_maps" => json!(w.opts.all_maps),
        "dead_commands" => json!(w.opts.dead_commands),
        "compass_monsters" => json!(w.opts.radar_mobs),
        "player_collision" => json!(w.opts.player_bump),
        "occlusion" => json!(w.opts.occlusion),
        "menu_reach" => json!(w.opts.menu_reach),
        "wireframe" => json!(w.opts.wireframe),
        "ttf_text" => json!(w.opts.ttf_text),
        "log_slide" => json!(!w.log.anim.instant),
        "cutscene_speed" => json!(w.opts.cutscene_speed),
        "animation_rate" => json!(w.opts.anim_rate),
        "clear_colour" => rgb_json(w.opts.clear),
        "ambient" => rgb_json(w.opts.ambient),
        "minimap_span" => json!(w.minimap_span),
        "send_interval" => json!(match w.net.stats.send_ms.load(std::sync::atomic::Ordering::Relaxed) {
            0 => 400,
            n => n,
        }),
        _ => return None,
    })
}

/// Switches whose mod is gone go back to retail's.
fn release_options(w: &mut World) {
    let loaded = w.lua.mod_ids();
    let gone: Vec<String> = w.lua.opt_owners.iter().filter(|(_, m)| !loaded.contains(*m)).map(|(n, _)| n.clone()).collect();
    for n in gone {
        w.lua.opt_owners.remove(&n);
        let default = match n.as_str() {
            "cutscene_speed" | "animation_rate" => serde_json::json!(1.0),
            "send_interval" => serde_json::json!(400),
            "clear_colour" | "ambient" => serde_json::Value::Null,
            n if OFF.contains(&n) => serde_json::json!(false),
            _ => serde_json::json!(true),
        };
        set_option(w, &n, &default);
    }
}

/// The mods of this session.
#[derive(Default)]
pub struct Host {
    vms: Vec<Vm>,
    started: bool,
    /// `/lua hide`: every mod's HUD, windows and world drawing hidden (they keep running).
    all_hidden: bool,
    /// The map the mods' HUD maps draw (opened and followed as the minimap's).
    map: std::cell::RefCell<Option<super::map::Overlay>>,
    /// Events raised by the client since the last mod phase.
    events: Vec<(&'static str, Vec<serde_json::Value>)>,
    /// Lines for mods (a registered command, or /lua), run in the next mod phase, and who ran them.
    typed: Vec<(String, From)>,
    last_frame: Option<std::time::Instant>,
    /// Last frame's state, for the derived change events.
    prev: Option<Snap>,
    /// The chat filter must be rebuilt (a mod loaded, unloaded or changed its filters).
    filters_dirty: bool,
    /// `/lua settings`: open Extras on this mod's page (taken by the app).
    pub(super) open_page: Option<String>,
    /// Window events for the mods' handlers: (mod, window, event).
    ui_events: Vec<(String, String, window::Event)>,
    /// The last packet of each id each way, shared with every mod (packets.last).
    pub(super) last: std::rc::Rc<std::cell::RefCell<LastPackets>>,
    /// `/lua watch`: mods reloaded when their files are saved.
    watch: Option<Watch>,
    /// The keys and mouse, shared with every mod (input.held, input.mouse).
    input: std::rc::Rc<std::cell::RefCell<input::State>>,
    /// Binds pressed since the last mod phase: (mod, bind as written, on release).
    fired: Vec<(String, String, bool)>,
    /// Keys the mods pressed, for the app to hand the game this frame.
    pub(in crate::client) synth: Vec<input::Synth>,
    /// Every loaded mod, for mods.list / call / emit.
    dir: runtime::Shared,
    /// Sounds the mods asked for, for the app's mixer.
    pub(in crate::client) sounds: Vec<audio::Sound>,
    /// The bus to the other clients, open while a mod holds multibox.
    ipc: Option<ipc::Bus>,
    /// The behaviour switches mods set, by name: the mod that set it (put back when it unloads).
    opt_owners: std::collections::BTreeMap<String, String>,
    /// Requests for the app after the frame: the window, a screenshot, a clipboard read; and the text read.
    pub(in crate::client) win_reqs: Vec<WinReq>,
    pub(in crate::client) shot: Option<String>,
    pub(in crate::client) paste_wanted: bool,
    pub(in crate::client) pasted: Option<String>,
    /// The music a mod holds (the mod, what), and whether the app must act on a change.
    pub(in crate::client) music: Option<(String, audio::Music)>,
    pub(in crate::client) music_dirty: bool,
    /// What the UI knows that mods read (the map grid).
    info: std::rc::Rc<std::cell::RefCell<state::FrameInfo>>,
    /// HUD elements clicked since the last mod phase: (mod, element, button).
    hud_clicks: Vec<(String, String, &'static str)>,
    /// Since when the world's mods wait for our character's state.
    start_wait: Option<std::time::Instant>,
    /// The game UI's data for this mod phase, lent to mods loaded during it too (their entry file and `load`).
    gu: Option<*const super::game_ui::GameUi>,
    /// The lobby mods are loaded; the screen they last saw (for `lobby_screen`); what the lobby shows.
    lobby_started: bool,
    lobby_screen: Option<&'static str>,
    lobby: lobby::Shared,
    /// A recording asked for (taken by the app), and the file being recorded, as the app sets it.
    pub(in crate::client) record_req: Option<Option<(String, std::path::PathBuf, u32)>>,
    pub(in crate::client) recording: std::rc::Rc<std::cell::RefCell<Option<std::path::PathBuf>>>,
}

impl Drop for Host {
    /// The app closing: `unload` to the mods left (the lobby mods), their account storage saved.
    fn drop(&mut self) {
        while !self.vms.is_empty() {
            self.unload(None, 0);
        }
    }
}

/// Mod folders watched for saved files (`/lua watch`): a thread polls their .lua / .toml files' times and names a mod
/// once its files have been still for a moment. It ends with the host.
struct Watch {
    dirs: std::sync::Arc<std::sync::Mutex<Vec<(String, PathBuf)>>>,
    rx: std::sync::mpsc::Receiver<String>,
}

impl Watch {
    fn new() -> Self {
        let dirs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, PathBuf)>::new()));
        let weak = std::sync::Arc::downgrade(&dirs);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("lua-watch".into())
            .spawn(move || {
                use std::time::{Duration, Instant, SystemTime};
                let mut seen: std::collections::HashMap<String, (SystemTime, Option<Instant>)> = Default::default();
                while let Some(dirs) = weak.upgrade() {
                    let list = dirs.lock().map(|d| d.clone()).unwrap_or_default();
                    drop(dirs);
                    for (id, dir) in list {
                        let t = newest(&dir);
                        match seen.get_mut(&id) {
                            None => {
                                seen.insert(id, (t, None));
                            }
                            Some(e) if e.0 != t => *e = (t, Some(Instant::now())),
                            // Sent once the files have been still for 300 ms (editors save in steps).
                            Some(e) if e.1.is_some_and(|c| c.elapsed() >= Duration::from_millis(300)) => {
                                e.1 = None;
                                if tx.send(id).is_err() {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            })
            .ok();
        Self { dirs, rx }
    }
}

/// The newest modification time of a mod folder's .lua and .toml files (DAT trees skipped).
fn newest(dir: &std::path::Path) -> std::time::SystemTime {
    let mut t = std::time::SystemTime::UNIX_EPOCH;
    let Ok(rd) = std::fs::read_dir(dir) else { return t };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_dir() {
            if !name.starts_with("rom") && !name.starts_with('.') {
                t = t.max(newest(&path));
            }
        } else if (name.ends_with(".lua") || name.ends_with(".toml")) && let Ok(m) = e.metadata().and_then(|m| m.modified()) {
            t = t.max(m);
        }
    }
    t
}

/// The watched mods (mod-data/watch.json).
fn watched() -> Vec<String> {
    serde_json::from_value(read_json(&data_dir().join("watch.json"))).unwrap_or_default()
}

/// What the derived events watch (Windower's change events).
#[derive(Clone, Default, PartialEq)]
struct Snap {
    vitals: Option<(u32, u32, u32, u8, u8)>,
    status: u8,
    jobs: Option<(u8, u8, u8, u8)>,
    buffs: Vec<u16>,
    target: Option<u32>,
    minute: Option<i32>,
    weekday: Option<usize>,
    moon: Option<usize>,
    weather: u16,
    party: Vec<u32>,
    items: Option<std::collections::BTreeMap<(u8, u8), (u16, u32)>>,
}

impl Snap {
    fn of(w: &World, items: bool) -> Self {
        let clock = w.clock.map(|_| crate::dat::msgfmt::VanaParts::of(super::vana_now(w)));
        let mut buffs: Vec<u16> = w.buffs.iter().copied().filter(|&b| b != 0xffff).collect();
        buffs.sort_unstable();
        Self {
            vitals: w.vitals,
            status: w.my_motion.status,
            jobs: w.stats.as_ref().map(|s| (s.main.0, s.main.1, s.sub.0, s.sub.1)),
            buffs,
            target: w.target,
            minute: clock.as_ref().map(|p| p.hour * 60 + p.minute),
            weekday: clock.as_ref().map(|p| p.weekday),
            moon: clock.as_ref().map(|p| p.phase),
            weather: w.weather,
            party: w.party.slots().slots.iter().flatten().map(|m| m.id).collect(),
            items: items.then(|| w.inv.slots.clone()),
        }
    }
}

/// The change events between two snapshots: (name, arguments).
fn changes(a: &Snap, b: &Snap) -> Vec<(&'static str, Vec<serde_json::Value>)> {
    use serde_json::Value as J;
    let mut out = Vec::new();
    if let (Some(o), Some(n)) = (a.vitals, b.vitals) {
        for (name, nv, ov) in [("hp_change", n.0, o.0), ("mp_change", n.1, o.1), ("tp_change", n.2, o.2), ("hpp_change", n.3 as u32, o.3 as u32), ("mpp_change", n.4 as u32, o.4 as u32)] {
            if nv != ov {
                out.push((name, vec![nv.into(), ov.into()]));
            }
        }
    }
    if a.status != b.status {
        out.push(("status_change", vec![b.status.into(), a.status.into()]));
    }
    if let (Some(o), Some(n)) = (a.jobs, b.jobs) {
        if o.0 != n.0 || o.2 != n.2 {
            out.push(("job_change", vec![n.0.into(), n.1.into(), n.2.into(), n.3.into()]));
        } else if n.1 > o.1 {
            out.push(("level_up", vec![n.1.into()]));
        } else if n.1 < o.1 {
            out.push(("level_down", vec![n.1.into()]));
        }
    }
    // Buffs as multisets: each id gained or lost once per copy.
    let mut lost = a.buffs.clone();
    for id in &b.buffs {
        match lost.iter().position(|x| x == id) {
            Some(k) => {
                lost.remove(k);
            }
            None => out.push(("buff_gain", vec![(*id).into()])),
        }
    }
    for id in lost {
        out.push(("buff_lose", vec![id.into()]));
    }
    if a.target != b.target {
        out.push(("target_change", vec![b.target.map_or(J::Null, J::from), a.target.map_or(J::Null, J::from)]));
    }
    if let (Some(o), Some(n)) = (a.minute, b.minute)
        && o != n
    {
        out.push(("time_change", vec![n.into(), o.into()]));
    }
    if let (Some(o), Some(n)) = (a.weekday, b.weekday)
        && o != n
    {
        out.push(("day_change", vec![n.into(), o.into()]));
    }
    if let (Some(o), Some(n)) = (a.moon, b.moon)
        && o != n
    {
        out.push(("moon_change", vec![n.into(), o.into()]));
    }
    if a.weather != b.weather {
        out.push(("weather_change", vec![b.weather.into(), a.weather.into()]));
    }
    if a.party != b.party {
        out.push(("party_change", vec![]));
    }
    if let (Some(o), Some(n)) = (&a.items, &b.items) {
        for (&(bag, slot), &(id, count)) in n {
            match o.get(&(bag, slot)) {
                Some(&(oid, ocount)) if oid == id && ocount == count => {}
                Some(&(oid, ocount)) if oid == id => out.push(("item_change", vec![bag.into(), slot.into(), id.into(), count.into(), ocount.into()])),
                Some(&(oid, ocount)) => {
                    out.push(("item_remove", vec![bag.into(), slot.into(), oid.into(), ocount.into()]));
                    out.push(("item_add", vec![bag.into(), slot.into(), id.into(), count.into()]));
                }
                None => out.push(("item_add", vec![bag.into(), slot.into(), id.into(), count.into()])),
            }
        }
        for (&(bag, slot), &(id, count)) in o {
            if !n.contains_key(&(bag, slot)) {
                out.push(("item_remove", vec![bag.into(), slot.into(), id.into(), count.into()]));
            }
        }
    }
    out
}

/// Mods built into the client (assets/mods): our own features on the mod API (docs/lua-mods.md 7.4). Each is (id,
/// mod.toml, files).
const BUNDLED: &[(&str, &str, &[(&str, &str)])] = &[("dps", include_str!("../../../assets/mods/dps/mod.toml"), &[("init.lua", include_str!("../../../assets/mods/dps/init.lua"))])];

/// Where mods keep their settings and storage: beside an FFXI_CONFIG copy for test runs.
fn data_dir() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("mod-data"),
        None => crate::config::data_dir().join("mod-data"),
    }
}

/// The mods' folders; a test run's own (`mods` beside its FFXI_CONFIG copy) when it has one, so test mods never sit
/// among the user's.
fn mods_dir() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG").map(|p| PathBuf::from(p).with_file_name("mods")) {
        Some(d) if d.is_dir() => d,
        _ => crate::config::mods_dir(),
    }
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(serde_json::Value::Null)
}

impl Host {
    /// A client event for the mods (delivered in the next mod phase).
    pub fn raise(&mut self, name: &'static str, args: Vec<serde_json::Value>) {
        if self.started {
            self.events.push((name, args));
        }
    }

    /// Whether a typed line is for the mods: //anything, /lua, or a mod's registered /command.
    pub fn takes(&self, line: &str) -> bool {
        if line.starts_with("//") {
            return true;
        }
        let first = line.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        first == "lua" || self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().commands.iter().any(|(n, _)| *n == first))
    }

    /// A line the player typed (the chat line or a macro): the only source that may run /lua.
    pub fn type_line(&mut self, line: &str) {
        self.typed.push((line.to_string(), From::Player));
    }

    /// The character's storage file; none in the lobby.
    fn char_file(w: Option<&World>) -> Option<String> {
        let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
        w.map(|w| format!("character-{}-{}.json", clean(&w.world_name), clean(&w.name)))
    }

    /// Creates the VM of the mod in `dir`, runs its entry file and sends it `load`.
    fn load(&mut self, w: Option<&World>, dir: PathBuf) -> Result<(), String> {
        let m = manifest::read(&dir).map_err(|e| format!("{e:#}"))?;
        self.load_with(w, m, dir.clone(), vm::Source::Dir(dir))
    }

    /// A bundled mod (assets/mods) by id.
    fn load_bundled(&mut self, w: Option<&World>, id: &str) -> Result<(), String> {
        let (_, text, files) = BUNDLED.iter().find(|b| b.0 == id).ok_or_else(|| format!("no bundled mod {id}"))?;
        let m = manifest::parse(text, id).map_err(|e| format!("{e:#}"))?;
        self.load_with(w, m, PathBuf::from("bundled").join(id), vm::Source::Bundled(files))
    }

    fn load_with(&mut self, w: Option<&World>, m: manifest::Manifest, dir: PathBuf, src: vm::Source) -> Result<(), String> {
        if self.vms.iter().any(|v| v.manifest.id == m.id) {
            return Err(format!("{} is already loaded", m.id));
        }
        let data = data_dir().join(&m.id);
        let char_file = Self::char_file(w);
        let storage = (read_json(&data.join("account.json")), char_file.as_ref().map_or(serde_json::Value::Null, |f| read_json(&data.join(f))));
        let settings = load_settings(&m, &data, char_file.as_deref());
        let given: Vec<String> = grants().get(&m.id).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let granted: Vec<String> = m.permissions.iter().filter(|p| given.contains(p)).cloned().collect();
        let missing: Vec<String> = m.permissions.iter().filter(|p| !granted.contains(p)).cloned().collect();
        let vm = Vm::new(m, dir, src).map_err(|e| format!("{e:#}"))?;
        vm.st.borrow_mut().settings = settings;
        vm.st.borrow_mut().granted = granted;
        if !missing.is_empty() {
            let id = vm.manifest.id.clone();
            vm.st.borrow_mut().notices.push(format!("{} asks for {}: allow it on its Extras page (/lua settings {id}) or with /lua grant {id} {}.", vm.manifest.name, missing.join(", "), missing.join(" ")));
        }
        vm.lua.set_app_data(self.last.clone());
        vm.lua.set_app_data(self.input.clone());
        vm.lua.set_app_data(self.dir.clone());
        vm.lua.set_app_data(self.info.clone());
        vm.lua.set_app_data(self.lobby.clone());
        vm.lua.set_app_data(Recording(self.recording.clone()));
        if let Some(p) = self.gu {
            vm.lua.set_app_data(api::GuRef(p));
        }
        api::install(&vm.lua, &vm, storage).map_err(|e| format!("{e}"))?;
        self.dir.borrow_mut().mods.push(runtime::Entry { id: vm.manifest.id.clone(), name: vm.manifest.name.clone(), version: vm.manifest.version.clone(), lua: vm.lua.weak(), st: vm.st.clone() });
        log::info!("lua mod {} {} loaded from {}", vm.manifest.id, vm.manifest.version, vm.dir.display());
        lend_opt(&vm, w, |vm| {
            vm.run_entry();
            vm.emit("load", ());
        });
        self.vms.push(vm);
        self.filters_dirty = true;
        Ok(())
    }

    /// Sends `unload`, saves its storage and drops the VM.
    fn unload(&mut self, w: Option<&World>, k: usize) {
        let vm = self.vms.remove(k);
        self.dir.borrow_mut().mods.retain(|m| m.id != vm.manifest.id);
        if self.music.as_ref().is_some_and(|m| m.0 == vm.manifest.id) {
            (self.music, self.music_dirty) = (None, true);
        }
        let mut info = self.info.borrow_mut();
        for mods in info.hidden.values_mut() {
            mods.remove(&vm.manifest.id);
        }
        info.looks.retain(|_, l| l.0 != vm.manifest.id);
        info.item_tints.retain(|_, t| t.0 != vm.manifest.id);
        info.shaders.retain(|s| s.0 != vm.manifest.id);
        drop(info);
        self.filters_dirty = true;
        lend_opt(&vm, w, |vm| vm.emit("unload", ()));
        save(&vm, w);
        log::info!("lua mod {} unloaded", vm.manifest.id);
    }

    /// The enabled mods (config `mods`) whose folders have a mod.toml; the lobby mods are running already and get
    /// the character's storage and settings.
    fn start(&mut self, w: &mut World) {
        self.started = true;
        let cfg = crate::config::Config::load();
        let root = mods_dir();
        install_stubs(root.clone());
        let loaded = |h: &Self, id: &str| h.vms.iter().any(|v| v.manifest.id == id);
        // Bundled mods (our own features on the mod API) run unless turned off in `mods` or replaced by a folder.
        for (id, ..) in BUNDLED {
            let off = cfg.mods.iter().any(|r| r.name == *id && !r.enabled);
            let folder = cfg.mods.iter().any(|r| r.name == *id && r.enabled) && root.join(id).join("mod.toml").is_file();
            if off || folder || loaded(self, id) {
                continue;
            }
            if let Err(e) = self.load_bundled(Some(w), id) {
                log::warn!("lua mod {id}: {e}");
                w.chat.push_colour(format!("Mod {id}: {e}"), ERROR);
            }
        }
        for r in cfg.mods.iter().filter(|r| r.enabled) {
            let dir = root.join(&r.name);
            if !dir.join("mod.toml").is_file() || self.vms.iter().any(|v| v.dir == dir) {
                continue;
            }
            if let Err(e) = self.load(Some(w), dir) {
                log::warn!("lua mod {}: {e}", r.name);
                w.chat.push_colour(format!("Mod {}: {e}", r.name), ERROR);
            }
        }
        for vm in self.vms.iter().filter(|v| v.manifest.lobby) {
            let data = data_dir().join(&vm.manifest.id);
            let file = Self::char_file(Some(w));
            vm.st.borrow_mut().settings = load_settings(&vm.manifest, &data, file.as_deref());
            let stored = file.map_or(serde_json::Value::Null, |f| read_json(&data.join(f)));
            if let Ok(t) = vm.lua.globals().get::<mlua::Table>("storage")
                && let Ok(c) = api::json_table(&vm.lua, &stored)
            {
                let _ = t.set("character", c);
            }
        }
        if self.watch.is_none() {
            for id in watched() {
                self.set_watch(&id, true);
            }
        }
        let name = w.name.clone();
        self.events.push(("login", vec![name.into()]));
        self.lobby.borrow_mut().screen = "world";
        let old = self.lobby_screen.replace("world");
        self.events.push(("lobby_screen", vec!["world".into(), old.into()]));
    }

    /// Before login: the enabled mods whose manifest asks for the lobby (`lobby = true`).
    fn start_lobby(&mut self) {
        self.lobby_started = true;
        let cfg = crate::config::Config::load();
        let root = mods_dir();
        for r in cfg.mods.iter().filter(|r| r.enabled) {
            let dir = root.join(&r.name);
            let Ok(m) = manifest::read(&dir) else { continue };
            if !m.lobby || self.vms.iter().any(|v| v.manifest.id == m.id) {
                continue;
            }
            if let Err(e) = self.load_with(None, m, dir.clone(), vm::Source::Dir(dir)) {
                log::warn!("lua mod {}: {e}", r.name);
            }
        }
        for id in watched() {
            self.set_watch(&id, true);
        }
    }

    /// Starts or stops watching mod `id`'s folder.
    fn set_watch(&mut self, id: &str, on: bool) {
        let watch = self.watch.get_or_insert_with(Watch::new);
        if let Ok(mut d) = watch.dirs.lock() {
            d.retain(|(i, _)| i != id);
            if on {
                d.push((id.to_string(), mods_dir().join(id)));
            }
        }
    }

    /// A mod by id: its folder, else the bundled one.
    fn load_named(&mut self, w: Option<&World>, id: &str) -> Result<(), String> {
        let dir = mods_dir().join(id);
        if !dir.join("mod.toml").is_file() && BUNDLED.iter().any(|b| b.0 == id) { self.load_bundled(w, id) } else { self.load(w, dir) }
    }

    /// Reloads the mod at `k` from its files, in the same place in the load order; its id.
    fn reload(&mut self, w: Option<&World>, k: usize) -> Result<String, String> {
        let dir = self.vms[k].dir.clone();
        let id = self.vms[k].manifest.id.clone();
        let bundled = matches!(self.vms[k].src, vm::Source::Bundled(_));
        self.unload(w, k);
        if bundled { self.load_bundled(w, &id) } else { self.load(w, dir) }.map_err(|e| format!("{id}: {e}"))?;
        let vm = self.vms.pop().expect("just loaded");
        self.vms.insert(k.min(self.vms.len()), vm);
        Ok(id)
    }

    /// Saved files of watched mods: each reloaded (or loaded, when it failed before).
    fn watch_upkeep(&mut self, w: Option<&World>, out: &mut Vec<(String, [u8; 4])>) {
        let ids: Vec<String> = self.watch.as_ref().map(|wa| wa.rx.try_iter().collect()).unwrap_or_default();
        for id in ids {
            let r = match self.vms.iter().position(|v| v.manifest.id.eq_ignore_ascii_case(&id)) {
                Some(k) => self.reload(w, k),
                None => self.load_named(w, &id).map(|()| id.clone()).map_err(|e| format!("{id}: {e}")),
            };
            match r {
                Ok(id) => out.push((format!("Reloaded {id} (files saved)."), super::WHITE)),
                Err(e) => out.push((e, ERROR)),
            }
        }
    }

    /// The world's mods are loaded (the first frame in a zone).
    pub fn started(&self) -> bool {
        self.started
    }

    /// Logout: `logout` to every mod, `unload` to all but the lobby mods, storage saved; the lobby mods keep
    /// running without the character's storage.
    pub fn stop(&mut self, w: &World) {
        for vm in &self.vms {
            lend(vm, w, |vm| vm.emit("logout", ()));
        }
        while let Some(k) = self.vms.iter().position(|v| !v.manifest.lobby) {
            self.unload(Some(w), k);
        }
        for vm in &self.vms {
            save(vm, Some(w));
            if let Ok(t) = vm.lua.globals().get::<mlua::Table>("storage")
                && let Ok(c) = vm.lua.create_table()
            {
                let _ = t.set("character", c);
            }
        }
        self.started = false;
        self.prev = None;
        self.events.clear();
    }

    /// `/lua ...`: list, load, unload, reload, run, set.
    fn lua_command(&mut self, w: &World, args: &[&str], out: &mut Vec<(String, [u8; 4])>) {
        let find = |vms: &[Vm], id: &str| vms.iter().position(|v| v.manifest.id.eq_ignore_ascii_case(id));
        match args.first().copied().unwrap_or("list") {
            "list" => {
                if self.vms.is_empty() {
                    out.push(("No Lua mods loaded.".into(), super::WHITE));
                }
                for v in &self.vms {
                    let st = v.st.borrow();
                    let state = v.suspended.as_deref().map_or("running".to_string(), |r| format!("suspended: {r}"));
                    out.push((format!("{} {} ({state}): {:.2} ms avg, {:.1} ms peak, {} KB", v.manifest.id, v.manifest.version, st.stats.avg, st.stats.peak, v.lua.used_memory() / 1024), super::WHITE));
                }
            }
            // Every mod's drawing hidden or shown again (hideui); the mods keep running.
            "hide" | "show" => {
                self.all_hidden = args[0] == "hide";
                out.push((if self.all_hidden { "Mods hidden (/lua show brings them back)." } else { "Mods shown." }.into(), super::WHITE));
            }
            "load" | "l" => match args.get(1) {
                Some(id) => match self.load_named(Some(w), id) {
                    Ok(()) => out.push((format!("Loaded {id}."), super::WHITE)),
                    Err(e) => out.push((e, ERROR)),
                },
                None => out.push(("/lua load <mod>".into(), ERROR)),
            },
            "unload" | "u" => match args.get(1).and_then(|id| find(&self.vms, id)) {
                Some(k) => {
                    let id = self.vms[k].manifest.id.clone();
                    self.unload(Some(w), k);
                    out.push((format!("Unloaded {id}."), super::WHITE));
                }
                None => out.push(("No such mod loaded.".into(), ERROR)),
            },
            // One mod (loaded if it is not, e.g. after an error), or every loaded one.
            "reload" | "r" => {
                let ids: Vec<String> = match args.get(1) {
                    Some(id) => match find(&self.vms, id) {
                        Some(k) => vec![self.vms[k].manifest.id.clone()],
                        None => {
                            match self.load_named(Some(w), id) {
                                Ok(()) => out.push((format!("Loaded {id}."), super::WHITE)),
                                Err(e) => out.push((e, ERROR)),
                            }
                            Vec::new()
                        }
                    },
                    None => self.vms.iter().map(|v| v.manifest.id.clone()).collect(),
                };
                // By id: a failed reload drops its mod and moves the others up.
                for id in &ids {
                    let Some(k) = find(&self.vms, id) else { continue };
                    match self.reload(Some(w), k) {
                        Ok(id) => out.push((format!("Reloaded {id}."), super::WHITE)),
                        Err(e) => out.push((e, ERROR)),
                    }
                }
            }
            // Reload a mod by itself whenever its files are saved; on or off per mod, kept across sessions.
            "watch" => match args.get(1) {
                Some(id) => {
                    let id = self.vms.iter().find(|v| v.manifest.id.eq_ignore_ascii_case(id)).map_or(id.to_string(), |v| v.manifest.id.clone());
                    if !mods_dir().join(&id).join("mod.toml").is_file() {
                        out.push((format!("{id}: no mods/{id}/mod.toml to watch."), ERROR));
                    } else {
                        let mut list = watched();
                        let on = !list.contains(&id);
                        list.retain(|i| *i != id);
                        if on {
                            list.push(id.clone());
                        }
                        let _ = std::fs::create_dir_all(data_dir());
                        let _ = std::fs::write(data_dir().join("watch.json"), serde_json::to_vec(&list).unwrap_or_default());
                        self.set_watch(&id, on);
                        out.push((if on { format!("Watching {id}: saving its files reloads it.") } else { format!("No longer watching {id}.") }, super::WHITE));
                    }
                }
                None => {
                    let list = watched();
                    out.push((if list.is_empty() { "/lua watch <mod>: reload it whenever its files are saved.".into() } else { format!("Watching: {}.", list.join(", ")) }, super::WHITE));
                }
            },
            "run" => match args.get(1).and_then(|id| find(&self.vms, id)) {
                Some(k) => {
                    let code = args[2..].join(" ");
                    let vm = &self.vms[k];
                    lend(vm, w, |vm| match vm.lua.load(code).set_name("=/lua run").into_function() {
                        Ok(f) => vm.call_input(f, ()),
                        Err(e) => vm.error(&e),
                    });
                }
                None => out.push(("/lua run <mod> <code>".into(), ERROR)),
            },
            "set" => match (args.get(1), args.get(2), args.get(3)) {
                (Some(id), Some(key), Some(_)) => {
                    let raw = args[3..].join(" ");
                    let value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw));
                    match self.vms.iter().find(|v| v.manifest.id.eq_ignore_ascii_case(id)).map(|v| v.manifest.id.clone()) {
                        Some(id) => {
                            self.apply_setting(w, &id, key, value);
                            out.push((format!("{id}: {key} set."), super::WHITE));
                        }
                        None => out.push(("No such mod loaded.".into(), ERROR)),
                    }
                }
                _ => out.push(("/lua set <mod> <key> <value>".into(), ERROR)),
            },
            "settings" => {
                let id = args.get(1).and_then(|id| find(&self.vms, id)).or((!self.vms.is_empty()).then_some(0)).map(|k| self.vms[k].manifest.id.clone());
                match id {
                    Some(id) => self.open_page = Some(id),
                    None => out.push(("No Lua mods loaded.".into(), ERROR)),
                }
            }
            "grant" | "revoke" => match args.get(1).and_then(|id| find(&self.vms, id)) {
                Some(k) => {
                    for p in &args[2..] {
                        self.set_grant(k, p, args[0] == "grant");
                    }
                    let id = self.vms[k].manifest.id.clone();
                    let list = self.vms[k].st.borrow().granted.clone();
                    out.push((format!("{id}: granted {}.", if list.is_empty() { "nothing".into() } else { list.join(", ") }), super::WHITE));
                }
                None => out.push(("/lua grant <mod> <permission...> | /lua revoke <mod> <permission...>".into(), ERROR)),
            },
            other => out.push((format!("/lua {other}: list, load, unload, reload, watch, run, set, settings, grant, revoke"), ERROR)),
        }
    }
}

/// An error line's colour (BGRA, 0x80 = 1.0: the log's red).
const ERROR: [u8; 4] = [0x30, 0x30, 0x80, 0x80];
/// A mod's print line (dim).
const DIM: [u8; 4] = [0x60, 0x60, 0x60, 0x80];

/// Lends the world to `vm` for `f`.
fn lend(vm: &Vm, w: &World, f: impl FnOnce(&Vm)) {
    lend_opt(vm, Some(w), f);
}

/// As `lend`; in the lobby there is no world to lend.
fn lend_opt(vm: &Vm, w: Option<&World>, f: impl FnOnce(&Vm)) {
    if let Some(w) = w {
        vm.lua.set_app_data(api::WorldRef(w as *const World));
    }
    f(vm);
    vm.lua.remove_app_data::<api::WorldRef>();
}

/// Writes a mod's storage tables.
fn save(vm: &Vm, w: Option<&World>) {
    let dir = data_dir().join(&vm.manifest.id);
    let Ok(storage) = vm.lua.globals().get::<mlua::Table>("storage") else { return };
    for (key, file) in [("account", Some("account.json".to_string())), ("character", Host::char_file(w))] {
        let Some(file) = file else { continue };
        let Ok(t) = storage.get::<mlua::Value>(key) else { continue };
        let mut skipped = Vec::new();
        match api::plain(&t, &format!("storage.{key}"), &mut skipped) {
            Ok(v) => {
                let _ = std::fs::create_dir_all(&dir);
                if let Err(e) = std::fs::write(dir.join(&file), serde_json::to_vec_pretty(&v).unwrap_or_default()) {
                    log::warn!("lua mod {}: saving {file}: {e}", vm.manifest.id);
                    vm.st.borrow_mut().notices.push(format!("{}: storage.{key} not saved: {e}", vm.manifest.name));
                }
            }
            Err(e) => {
                log::warn!("lua mod {}: storage.{key} not saved: {e}", vm.manifest.id);
                vm.st.borrow_mut().notices.push(format!("{}: storage.{key} not saved: {e}", vm.manifest.name));
            }
        }
        if !skipped.is_empty() {
            log::warn!("lua mod {}: left out of the save: {}", vm.manifest.id, skipped.join(", "));
            vm.st.borrow_mut().notices.push(format!("{}: left out of the save (not plain data): {}", vm.manifest.name, skipped.join(", ")));
        }
    }
}

/// The mod phase (docs/lua-mods.md 3.2): once a frame in the world.
pub(super) fn frame(w: &mut World, gu: Option<&super::game_ui::GameUi>) {
    if !w.lua.opt_owners.is_empty() {
        release_options(w);
    }
    let mut host = std::mem::take(&mut w.lua);
    host.gu = gu.map(|g| g as *const _);
    if !host.started {
        // Mods start with our character's state in: in a zone, our vitals known (or 5 s on, whatever came).
        let waited = *host.start_wait.get_or_insert_with(std::time::Instant::now);
        if w.zone.is_none() || w.name.is_empty() || (w.vitals.is_none() && waited.elapsed().as_secs() < 5) {
            if w.zone.is_none() {
                host.start_wait = None;
            }
            host.gu = None;
            w.lua = host;
            return;
        }
        host.start_wait = None;
        host.start(w);
    }
    // The derived change events (items only while someone listens: the bags are big).
    let items = host.vms.iter().any(|v| ["item_add", "item_remove", "item_change"].iter().any(|e| v.has_handler(e)));
    let snap = Snap::of(w, items);
    if let Some(prev) = &host.prev {
        let found = changes(prev, &snap);
        host.events.extend(found);
    }
    host.prev = Some(snap);
    if let Some(gu) = gu {
        host.info.borrow_mut().update(w, gu);
        for vm in &host.vms {
            vm.lua.set_app_data(api::GuRef(gu as *const _));
        }
    }
    let now = std::time::Instant::now();
    let dt = host.last_frame.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
    host.last_frame = Some(now);
    let mut out: Vec<(String, [u8; 4])> = Vec::new();
    let bind_lines;
    {
        let w: &World = w;
        host.watch_upkeep(Some(w), &mut out);
        bind_lines = input::run_binds(&mut host, w, &mut out);
        // Lines typed for mods.
        for (line, from) in std::mem::take(&mut host.typed) {
            let body = line.trim_start_matches('/');
            let mut words = body.split_whitespace();
            let name = words.next().unwrap_or("").to_ascii_lowercase();
            let args: Vec<&str> = words.collect();
            if name == "lua" {
                // Permissions, other mods' code and settings are the player's alone, never a mod's or a bind's.
                let player_only = matches!(args.first().map(|a| a.to_ascii_lowercase()).as_deref(), Some("grant" | "revoke" | "run" | "set" | "watch"));
                if player_only && from != From::Player {
                    out.push(("/lua grant, revoke, run, set and watch only work when you type them.".into(), ERROR));
                } else {
                    host.lua_command(w, &args, &mut out);
                }
                continue;
            }
            let input = from != From::Mod;
            let owner = host.vms.iter().find(|v| v.suspended.is_none() && v.st.borrow().commands.iter().any(|(n, _)| *n == name));
            match owner {
                Some(vm) => lend(vm, w, |vm| {
                    let f = vm.st.borrow().commands.iter().find(|(n, _)| *n == name).and_then(|(_, k)| vm.lua.registry_value::<mlua::Function>(k).ok());
                    if let Some(f) = f {
                        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                        if input { vm.call_input(f, mlua::Variadic::from_iter(args)) } else { vm.call(f, mlua::Variadic::from_iter(args)) }
                    }
                }),
                None => out.push((format!("No mod command {name}."), ERROR)),
            }
        }
        let events = std::mem::take(&mut host.events);
        let ui_events = std::mem::take(&mut host.ui_events);
        let pasted = host.pasted.take();
        // The bus to the other clients, and what they sent (to the same mod, when it holds multibox).
        if host.ipc.is_none() && host.vms.iter().any(|v| v.st.borrow().granted.iter().any(|g| g == "multibox")) {
            host.ipc = ipc::Bus::open();
        }
        let msgs: Vec<ipc::Msg> = host.ipc.as_ref().map(|b| b.rx.try_iter().collect()).unwrap_or_default();
        for (m, from, to, text) in msgs {
            if !to.is_empty() && !to.eq_ignore_ascii_case(&w.name) {
                continue;
            }
            let Some(vm) = host.vms.iter().find(|v| v.manifest.id == m && v.suspended.is_none() && v.st.borrow().granted.iter().any(|g| g == "multibox")) else { continue };
            lend(vm, w, |vm| vm.emit("ipc_message", (text.clone(), from.clone())));
        }
        for (m, id, button) in std::mem::take(&mut host.hud_clicks) {
            let Some(vm) = host.vms.iter().find(|v| v.manifest.id == m && v.suspended.is_none()) else { continue };
            let f = vm.st.borrow().hud_handlers.iter().find(|h| h.0 == id).and_then(|h| vm.lua.registry_value::<mlua::Function>(&h.1).ok());
            if let Some(f) = f {
                lend(vm, w, |vm| {
                    let e = api::to_lua(&vm.lua, &serde_json::json!({"kind": "click", "id": id, "button": button})).unwrap_or(mlua::Value::Nil);
                    vm.call_input(f, (id.clone(), e));
                });
            }
        }
        // Events mods sent each other (mods.emit), to every other mod listening.
        let emitted = std::mem::take(&mut host.dir.borrow_mut().emitted);
        for (from, name, args) in &emitted {
            for vm in host.vms.iter().filter(|v| v.suspended.is_none() && v.manifest.id != *from && v.has_handler(name)) {
                lend(vm, w, |vm| {
                    let vals: Vec<mlua::Value> = args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                });
            }
        }
        for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
            lend(vm, w, |vm| {
                for (name, args) in &events {
                    if !vm.has_handler(name) {
                        continue;
                    }
                    let vals: Vec<mlua::Value> = args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
                let mine: Vec<(String, window::Event)> = ui_events.iter().filter(|e| e.0 == vm.manifest.id).map(|e| (e.1.clone(), e.2.clone())).collect();
                for (win, ev) in mine {
                    let f = vm.st.borrow().window_handlers.iter().find(|h| h.0 == win).and_then(|h| vm.lua.registry_value::<mlua::Function>(&h.1).ok());
                    if let Some(f) = f {
                        let t = api::to_lua(&vm.lua, &ev.json()).unwrap_or(mlua::Value::Nil);
                        vm.call_input(f, (win, t));
                    }
                }
                net::poll(vm);
                sock::poll(vm);
                if let Some(text) = &pasted {
                    let keys = std::mem::take(&mut vm.st.borrow_mut().paste_handlers);
                    for k in keys {
                        if let Ok(f) = vm.lua.registry_value::<mlua::Function>(&k) {
                            vm.call(f, text.as_str());
                        }
                        let _ = vm.lua.remove_registry_value(k);
                    }
                }
                vm.timers();
                vm.wake();
                if vm.has_handler("frame") {
                    vm.emit("frame", dt);
                }
            });
        }
    }
    for vm in &host.vms {
        vm.lua.remove_app_data::<api::GuRef>();
    }
    host.gu = None;
    // Accounting, failures, saves and the mods' ops.
    let mut ops = Vec::new();
    for vm in host.vms.iter_mut() {
        vm.end_frame();
        if vm.suspended.is_none()
            && let Some(why) = vm.failing()
        {
            out.push((format!("Mod {} suspended: {why}. /lua reload {} to start it again.", vm.manifest.name, vm.manifest.id), ERROR));
            vm.suspended = Some(why);
            vm.st.borrow_mut().suspended = true;
            // Its chat filters stop with it.
            vm.st.borrow_mut().filters_dirty = true;
        }
        let (notices, vm_ops, save_now) = {
            let mut st = vm.st.borrow_mut();
            (std::mem::take(&mut st.notices), std::mem::take(&mut st.ops), std::mem::take(&mut st.save))
        };
        out.extend(notices.into_iter().map(|n| (n, ERROR)));
        ops.extend(vm_ops);
        if save_now {
            save(vm, Some(w));
        }
    }
    // A window opened this phase takes the focus from every other (not from an open modal one, unless modal itself).
    let wanted: Option<(usize, String)> = host.vms.iter().enumerate().find_map(|(k, v)| v.st.borrow_mut().focus_request.take().map(|id| (k, id)));
    let held = host.modal().is_some();
    let wanted = wanted.filter(|(k, id)| !held || host.vms[*k].st.borrow().windows.iter().any(|w| w.id == *id && w.modal));
    if let Some((k, id)) = wanted {
        for (j, vm) in host.vms.iter().enumerate() {
            for win in vm.st.borrow_mut().windows.iter_mut() {
                win.focus = j == k && win.id == id && win.open;
            }
        }
    }
    let dirty = std::mem::take(&mut host.filters_dirty) | host.vms.iter().fold(false, |d, v| d | std::mem::take(&mut v.st.borrow_mut().filters_dirty));
    if dirty {
        w.chat.filter = host.chat_filter();
        w.chat.mode_colours = w.battle.as_ref().map(|b| std::rc::Rc::new(b.code_tables(&w.game).1));
    }
    // `/lua settings`: the modern UI shows Extras inside the Config window (its Extras entry, page open); the classic
    // page stands alone.
    host.packet_upkeep(w);
    if host.open_page.is_some() && w.settings.is_none() {
        if w.modern {
            let mut c = super::configui::ConfigUi::open("configwi");
            (c.m_entry, c.m_open, c.m_row) = (super::modern::extras_entry(), true, 0);
            w.config = Some(c);
            w.menu = None;
        }
        w.want_settings = Some(super::settings::Kind::Extras);
    }
    w.lua = host;
    for (line, colour) in out {
        w.chat.push_colour(line, colour);
    }
    for op in ops {
        apply(w, gu, op);
    }
    for line in bind_lines {
        run_line(w, &line, super::textcmd::Mode::Typed, From::Bind);
    }
    // Injected packets go through the hooks now, not with the next batch (which may be seconds away).
    if w.lua.vms.iter().any(|v| !v.st.borrow().inject_in.is_empty()) {
        let out = packets_in(w, Vec::new());
        let _ = w.net.commands.send(super::net::NetCommand::Decode(0, out));
    }
    if w.lua.vms.iter().any(|v| !v.st.borrow().inject_out.is_empty()) {
        let out = packets_out(w, Vec::new());
        let _ = w.net.commands.send(super::net::NetCommand::SendBatch(0, out));
    }
}

/// Where a line for the mods came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum From {
    /// Typed by the player (the chat line, a macro).
    Player,
    /// A mod's key bind the player pressed: the mod's command runs as the player's input, but /lua grant, revoke,
    /// run, set and watch do not.
    Bind,
    /// A mod's own line (commands.run, actions): runs as the mod's own, not the player's input.
    Mod,
}

/// A line from a mod or a mod's bind, run as typed. Lines for the mods are queued here with their source, so that
/// textcmd::run never takes them as the player's (it trims the line the same way, so the check matches).
pub(super) fn run_line(w: &mut World, line: &str, mode: super::textcmd::Mode, from: From) {
    let line = line.trim();
    if line.starts_with('/') && w.lua.takes(line) {
        w.lua.typed.push((line.to_string(), from));
    } else {
        super::textcmd::run(w, line, mode);
    }
}

/// The file the app is recording, shared with the mods (client.recording).
pub(super) struct Recording(pub std::rc::Rc<std::cell::RefCell<Option<std::path::PathBuf>>>);

/// What the lobby mods asked of the app this frame.
#[derive(Default)]
pub(in crate::client) struct LobbyOut {
    pub keys: Vec<input::Synth>,
    pub acts: Vec<lobby::Act>,
}

/// The mod phase before login, for the lobby mods: events, timers, sockets and `frame`; no world is lent, so game
/// state errors. Lines a mod prints go to the log file.
pub(in crate::client) fn lobby_frame(host: &mut Host, view: lobby::View) -> LobbyOut {
    let screen = view.screen;
    *host.lobby.borrow_mut() = view;
    if !host.lobby_started {
        host.start_lobby();
    }
    if host.lobby_screen != Some(screen) {
        let old = host.lobby_screen.replace(screen);
        host.events.push(("lobby_screen", vec![screen.into(), old.into()]));
    }
    let now = std::time::Instant::now();
    let dt = host.last_frame.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
    host.last_frame = Some(now);
    let mut lines: Vec<(String, [u8; 4])> = Vec::new();
    host.watch_upkeep(None, &mut lines);
    let events = std::mem::take(&mut host.events);
    let emitted = std::mem::take(&mut host.dir.borrow_mut().emitted);
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        lend_opt(vm, None, |vm| {
            for (_, name, args) in emitted.iter().filter(|e| e.0 != vm.manifest.id) {
                if vm.has_handler(name) {
                    let vals: Vec<mlua::Value> = args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
            }
            for (name, args) in &events {
                if vm.has_handler(name) {
                    let vals: Vec<mlua::Value> = args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
            }
            net::poll(vm);
            sock::poll(vm);
            vm.timers();
            vm.wake();
            if vm.has_handler("frame") {
                vm.emit("frame", dt);
            }
        });
    }
    let mut out = LobbyOut::default();
    for vm in host.vms.iter_mut() {
        vm.end_frame();
        if vm.suspended.is_none()
            && let Some(why) = vm.failing()
        {
            log::warn!("lua mod {} suspended: {why}", vm.manifest.id);
            vm.suspended = Some(why);
            vm.st.borrow_mut().suspended = true;
        }
        let (notices, ops, save_now) = {
            let mut st = vm.st.borrow_mut();
            (std::mem::take(&mut st.notices), std::mem::take(&mut st.ops), std::mem::take(&mut st.save))
        };
        for n in notices {
            log::warn!("lua mod {}: {n}", vm.manifest.id);
        }
        for op in ops {
            match op {
                Op::Key(k) => out.keys.push(k),
                Op::Lobby(a) => out.acts.push(a),
                Op::Print(text) | Op::Chat { text, .. } => log::info!("lua mod {}: {text}", vm.manifest.id),
                _ => log::debug!("lua mod {}: not in the lobby", vm.manifest.id),
            }
        }
        if save_now {
            save(vm, None);
        }
    }
    for (line, _) in lines {
        log::info!("lua: {line}");
    }
    out
}

fn apply(w: &mut World, gu: Option<&super::game_ui::GameUi>, op: Op) {
    match op {
        Op::Print(text) => w.chat.push_colour(text, DIM),
        Op::Chat { text, mode, colour } => match (mode, colour, super::expand_phrases(w, &text)) {
            (Some(m), _, text) => {
                let (colour, indent) = w.battle.as_ref().map_or((super::WHITE, false), |b| (b.colour_in(m, &w.game), b.indents(m)));
                w.chat.push_event(text, colour, indent, m);
            }
            (None, Some(c), text) => {
                let colour = w.battle.as_ref().map_or(super::WHITE, |b| b.colour_in(c, &w.game));
                w.chat.push_colour(text, colour);
            }
            (None, None, text) => w.chat.push(text),
        },
        Op::Run(line) => run_line(w, &line, super::textcmd::Mode::Typed, From::Mod),
        Op::Act(a) => actions::apply(w, gu, a),
        Op::Key(k) => w.lua.synth.push(k),
        Op::Copy(text) => w.copied = Some(text),
        Op::Window(r) => w.lua.win_reqs.push(r),
        Op::Shot(name) => w.lua.shot = Some(name),
        Op::Paste => w.lua.paste_wanted = true,
        Op::Option(mod_id, name, v) => {
            // The minimap's zoom stays as the wheel's does; the rest go back when the mod unloads.
            if set_option(w, &name, &v) && name != "minimap_span" {
                w.lua.opt_owners.insert(name, mod_id);
            }
        }
        Op::Timer { mod_id, name, secs, section, group, icon, total, fills } => {
            let Some(name) = name else {
                w.timers.mods.retain(|t| t.mod_id != mod_id);
                return;
            };
            w.timers.mods.retain(|t| !(t.mod_id == mod_id && t.name == name));
            if let Some(s) = secs {
                use super::timers::Icon;
                let icon = match icon {
                    Some((k, id)) if k == "status" => Icon::Status(id),
                    Some((k, id)) => super::magic::list_icon_of(w, k == "spell", id).map_or(Icon::None, |(l, i)| Icon::List(l, i)),
                    None => Icon::None,
                };
                let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(s.max(0.0));
                w.timers.mods.push(super::timers::ModTimer { mod_id, section, group, icon, name, end, total: if total > 0.0 { total } else { s.max(0.0) }, fills });
            }
        }
        Op::Interface(show) => w.ui_hidden = !show,
        // In the world there is no lobby to act on.
        Op::Lobby(_) => {}
        Op::Record(r) => w.lua.record_req = Some(r),
        Op::CameraDistance(k) => super::camera::set_distance(w, k),
        Op::Camera(speed, max, battle) => super::camera::set_mod(w, speed, max, battle),
        Op::Sound(s) => w.lua.sounds.push(s),
        Op::Music(mod_id, m) => {
            let now = m.map(|m| (mod_id.clone(), m));
            // Giving it back only counts from the mod that holds it.
            if now.is_some() || w.lua.music.as_ref().is_some_and(|h| h.0 == mod_id) {
                (w.lua.music, w.lua.music_dirty) = (now, true);
            }
        }
        Op::Ipc(mod_id, to, text) => {
            if let Some(b) = &w.lua.ipc {
                b.send(&mod_id, &w.name, to.as_deref(), &text);
            }
        }
        Op::Config(id, v) => {
            if w.game.set(id, v) {
                super::cfgapply::apply(w, id);
                w.game_save = true;
            }
        }
        Op::Line { text, open: true } => {
            (w.chat_input, w.chat_caret, w.chat_anchor) = (text, None, None);
            w.typing = true;
        }
        Op::Line { open: false, .. } => {
            w.typing = false;
            (w.chat_input, w.chat_caret, w.chat_anchor) = (String::new(), None, None);
        }
    }
}

/// The mods' world shapes as vertices for the renderer: [line pairs, triangles].
pub(super) fn world_shapes(w: &World) -> [Vec<crate::render::LineVertex>; 2] {
    if w.lua.started && !w.lua.all_hidden { draw::vertices(w) } else { [Vec::new(), Vec::new()] }
}

/// The mods' HUD elements, under the windows (world_hud).
pub(super) fn draw_hud(gu: &mut super::game_ui::GameUi, w: &World) {
    if w.lua.started && !w.lua.all_hidden {
        draw::draw_overlay(gu, w);
        hud::draw(gu, w);
    } else {
        w.lua.info.borrow_mut().hud_rects.clear();
    }
}

impl Host {
    /// This frame's view-projection (world_hud), for draw.to_screen.
    pub(in crate::client) fn set_view_proj(&self, m: glam::Mat4) {
        self.info.borrow_mut().view_proj = m;
    }

    /// Whether a mod hid a native HUD piece (ui.native.hide).
    pub(in crate::client) fn hides(&self, piece: &str) -> bool {
        self.info.borrow().hidden.get(piece).is_some_and(|m| !m.is_empty())
    }

    /// The entity looks mods set (render.entity), by server id.
    /// The mods' post-process passes this frame.
    pub(in crate::client) fn shaders(&self) -> Vec<crate::render::postfx::Want> {
        self.info.borrow().shaders.clone()
    }

    /// A pass's WGSL error, for its mod's log line.
    pub(in crate::client) fn shader_error(&self, mod_id: &str, id: &str, e: &str) {
        if let Some(vm) = self.vms.iter().find(|v| v.manifest.id == mod_id) {
            vm.st.borrow_mut().notices.push(format!("render.shader {id}: {e}"));
        }
    }

    /// A mod's colour for an item's name in the game's lists (shops, the auction house).
    pub(in crate::client) fn item_tint(&self, id: u16) -> Option<u32> {
        self.info.borrow().item_tints.get(&id).map(|t| t.1)
    }

    pub(in crate::client) fn looks(&self) -> std::collections::HashMap<u32, render::Look> {
        self.info.borrow().looks.iter().map(|(id, l)| (*id, l.1.clone())).collect()
    }

    /// The modern panels of the hidden pieces.
    pub(in crate::client) fn hidden_panels(&self) -> Vec<&'static str> {
        native::PIECES.iter().filter(|p| self.hides(p.0)).flat_map(|p| p.1.iter().copied()).collect()
    }

    /// The window's place on the desktop and its focus; a change of focus raises focus_change.
    pub(in crate::client) fn set_window_state(&mut self, pos: Option<(i32, i32)>, focused: bool) {
        let was = {
            let mut f = self.info.borrow_mut();
            f.win_pos = pos;
            std::mem::replace(&mut f.focused, focused)
        };
        if was != focused && self.started {
            self.raise("focus_change", vec![serde_json::json!(focused)]);
        }
    }

    /// The window's size in pixels and the UI's units per pixel, for the HUD's places.
    pub(in crate::client) fn set_view(&self, window: [u32; 2], k: (f32, f32)) {
        let mut f = self.info.borrow_mut();
        f.window = [window[0] as f32, window[1] as f32];
        f.k = k;
    }
}

/// LuaLS stubs for the whole mod API (`vana-client lua-stubs`).
pub fn stubs() -> String {
    docs::stubs()
}

/// The stubs for editors in the mods folder, off the UI thread: `.library/vana.lua` and a `.luarc.json` naming it
/// (hidden, so the Mods page does not list them). Written only when they differ.
fn install_stubs(root: PathBuf) {
    std::thread::spawn(move || {
        if !root.is_dir() {
            return;
        }
        let luarc = "{\n  \"runtime.version\": \"Lua 5.4\",\n  \"workspace.library\": [\".library\"],\n  \"workspace.checkThirdParty\": false\n}\n";
        for (path, text) in [(root.join(".library").join("vana.lua"), stubs()), (root.join(".luarc.json"), luarc.to_string())] {
            if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
                continue;
            }
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&root));
            if let Err(e) = std::fs::write(&path, text) {
                log::warn!("lua mods: writing {}: {e}", path.display());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Table, Value};

    /// A wait_until test that never returns runs out of its budget instead of freezing the frame.
    #[test]
    fn wait_until_test_has_a_budget() {
        let vm = Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let f: mlua::Function = vm.lua.load("return function() wait_until(function() while true do end end) end").eval().unwrap();
        vm.resume(vm.lua.create_thread(f).unwrap(), ());
        assert_eq!(vm.st.borrow().sleepers.len(), 1);
        // A wait resumes in a later frame, never the one it began in.
        vm.st.borrow_mut().frame_no += 1;
        let t0 = std::time::Instant::now();
        vm.wake();
        assert!(t0.elapsed() < std::time::Duration::from_secs(2), "the test ran {:?}", t0.elapsed());
        assert!(vm.st.borrow().sleepers.is_empty());
        assert_eq!(vm.st.borrow().errors.len(), 1);
    }

    /// A suspended mod's provided functions don't run for other mods either.
    #[test]
    fn mods_call_skips_a_suspended_mod() {
        let dir: runtime::Shared = Default::default();
        let new = |id: &str| {
            let m = manifest::Manifest { id: id.into(), ..Default::default() };
            let vm = Vm::new(m, PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
            vm.lua.set_app_data(dir.clone());
            api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
            dir.borrow_mut().mods.push(runtime::Entry { id: id.into(), name: id.into(), version: String::new(), lua: vm.lua.weak(), st: vm.st.clone() });
            vm
        };
        let (caller, provider) = (new("caller"), new("provider"));
        provider.lua.load("mods.provide('twice', function(n) return n * 2 end)").exec().unwrap();
        assert_eq!(caller.lua.load("return mods.call('provider', 'twice', 21)").eval::<i64>().unwrap(), 42);
        provider.st.borrow_mut().suspended = true;
        let e = caller.lua.load("return mods.call('provider', 'twice', 21)").eval::<i64>().unwrap_err();
        assert!(e.to_string().contains("suspended"), "{e}");
    }

    /// Injected and hook-built packets carry a size field that matches their bytes, and none overflows it (issue #69).
    #[test]
    fn packet_sizes_fit_the_header() {
        // A whole sub-packet with a wrong size field gets the right one; the data is padded to a word.
        let p = fit(vec![0x15, 0x02, 0, 0, 1, 2, 3]);
        assert_eq!(p.len(), 8);
        assert_eq!(u16::from_le_bytes([p[0], p[1]]), 0x015 | (2 << 9));
        assert_eq!(fit(vec![0; 508]).len(), 508);
        let vm = Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.st.borrow_mut().granted.push("packets.write".into());
        vm.lua.load("assert(#packets.build(0x15, string.rep('x', 504)) == 508)").exec().unwrap();
        for code in [
            "packets.build(0x15, string.rep('x', 505))",
            "packets.build(0x200, '')",
            "packets.inject_out(0x15, string.rep('x', 505))",
            "packets.inject_in(string.rep('x', 512))",
            "packets.inject_in('abc')",
        ] {
            assert!(vm.lua.load(code).exec().is_err(), "{code} was accepted");
        }
        vm.lua.load("packets.inject_in(string.char(0x15, 0x02, 0, 0, 1))").exec().unwrap();
        let st = vm.st.borrow();
        assert_eq!(st.inject_in, vec![vec![0x15, 0x04, 0, 0, 1, 0, 0, 0]]);
        assert!(st.inject_out.is_empty());
    }

    /// Every documented function is bound, and every function in a documented namespace is documented.
    #[test]
    fn docs_match_bindings() {
        let vm = Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let g = vm.lua.globals();
        for f in docs::FUNCS {
            let v: Value = if f.ns.is_empty() {
                g.get(f.name).unwrap()
            } else {
                let mut t: Table = g.clone();
                for part in f.ns.split('.') {
                    t = t.get::<Table>(part).unwrap();
                }
                t.get(f.name).unwrap()
            };
            assert!(matches!(v, Value::Function(_)), "{}.{} is documented but not bound", f.ns, f.name);
        }
        // Every global table but Lua's own is a namespace of ours, documented or not.
        const STD: [&str; 7] = ["_G", "table", "string", "math", "utf8", "coroutine", "os"];
        let namespaces: Vec<String> = g.pairs::<String, Value>().filter_map(|p| p.ok()).filter(|(k, v)| matches!(v, Value::Table(_)) && !STD.contains(&k.as_str())).map(|p| p.0).collect();
        for ns in namespaces {
            let ns = ns.as_str();
            let t: Table = g.get(ns).unwrap();
            // Sub-tables (ui.native) are namespaces of their own.
            for pair in t.pairs::<String, Value>() {
                let (name, v) = pair.unwrap();
                if let Value::Table(sub) = v {
                    let full = format!("{ns}.{name}");
                    for p in sub.pairs::<String, Value>() {
                        let (fname, fv) = p.unwrap();
                        if matches!(fv, Value::Function(_)) {
                            assert!(docs::FUNCS.iter().any(|f| f.ns == full && f.name == fname), "{full}.{fname} is bound but not documented");
                        }
                    }
                }
            }
            for pair in t.pairs::<String, Value>() {
                let (name, v) = pair.unwrap();
                if matches!(v, Value::Function(_)) {
                    assert!(docs::FUNCS.iter().any(|f| f.ns == ns && f.name == name), "{ns}.{name} is bound but not documented");
                }
            }
        }
    }
}

/// A decoded s2c 0x028 for the `action` event (Windower's action table, our field names).
pub(super) fn action_json(a: &crate::net::action::Action) -> serde_json::Value {
    use serde_json::json;
    let side = |s: &Option<crate::net::action::Side>| s.as_ref().map(|s| json!({"kind": s.kind, "info": s.info, "value": s.value, "message": s.message}));
    let targets: Vec<serde_json::Value> = a
        .targets
        .iter()
        .map(|t| {
            let results: Vec<serde_json::Value> = t
                .results
                .iter()
                .map(|r| json!({"miss": r.miss, "kind": r.kind, "animation": r.animation, "info": r.info, "scale": r.scale, "value": r.value, "message": r.message, "bit": r.bit, "proc": side(&r.proc), "react": side(&r.react)}))
                .collect();
            json!({"id": t.id, "results": results})
        })
        .collect();
    json!({"actor": a.actor, "category": a.category, "param": a.arg, "info": a.info, "targets": targets})
}

impl Host {
    /// The chat filter over every running mod's `filters.chat` functions, in load order; none when there are none.
    fn chat_filter(&self) -> Option<std::rc::Rc<dyn Fn(&str, u16) -> super::combat::Filtered>> {
        type Entry = (String, mlua::Lua, mlua::Function, std::rc::Rc<std::cell::RefCell<vm::ModState>>);
        let mut list: Vec<Entry> = Vec::new();
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for (_, k) in &vm.st.borrow().chat_filters {
                if let Ok(f) = vm.lua.registry_value::<mlua::Function>(k) {
                    list.push((vm.manifest.name.clone(), vm.lua.clone(), f, vm.st.clone()));
                }
            }
        }
        if list.is_empty() {
            return None;
        }
        Some(std::rc::Rc::new(move |text: &str, mode: u16| {
            use super::combat::{Filtered, NO_MODE, is_battle};
            let (mut text, mut mode, mut changed) = (text.to_string(), mode, false);
            for (name, lua, f, st) in &list {
                let game_mode = (mode != NO_MODE && mode != super::combat::BATTLE_MODE).then_some(mode as u8);
                st.borrow_mut().deadline = Some(std::time::Instant::now() + vm::FILTER_CAP);
                let r = f.call::<mlua::MultiValue>((text.as_str(), game_mode, is_battle(mode)));
                st.borrow_mut().deadline = None;
                let _ = lua;
                match r {
                    Err(e) => {
                        log::warn!("lua mod {name}: chat filter: {e}");
                        st.borrow_mut().record(name, &e);
                    }
                    Ok(v) => match v.front() {
                        Some(mlua::Value::Boolean(false)) => return Filtered::Block,
                        Some(mlua::Value::String(t)) => {
                            text = t.to_string_lossy();
                            if let Some(m) = v.get(1).and_then(|m| m.as_integer()) {
                                mode = if is_battle(mode) { super::combat::BATTLE | (m as u16 & 0xff) } else { m as u16 & 0xff };
                            }
                            changed = true;
                        }
                        _ => {}
                    },
                }
            }
            if changed { Filtered::Replace(text, mode) } else { Filtered::Keep }
        }))
    }
}

/// The mods' typed-line filters (docs/lua-mods.md 4.3), before our command parser: the line as they left it, or none
/// when one blocked it.
pub(super) fn filter_typed(w: &mut World, line: &str) -> Option<String> {
    if w.lua.vms.iter().all(|v| v.st.borrow().typed_filters.is_empty()) {
        return Some(line.to_string());
    }
    let host = std::mem::take(&mut w.lua);
    let mut line = line.to_string();
    let mut blocked = false;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let funcs: Vec<mlua::Function> = vm.st.borrow().typed_filters.iter().filter_map(|(_, k)| vm.lua.registry_value(k).ok()).collect();
        lend(vm, w, |vm| {
            for f in funcs {
                if blocked {
                    return;
                }
                vm.st.borrow_mut().deadline = Some(std::time::Instant::now() + vm::FILTER_CAP);
                let r = f.call::<mlua::Value>(line.as_str());
                vm.st.borrow_mut().deadline = None;
                match r {
                    Err(e) => vm.error(&e),
                    Ok(mlua::Value::Boolean(false)) => blocked = true,
                    Ok(mlua::Value::String(t)) => line = t.to_string_lossy(),
                    Ok(_) => {}
                }
            }
        });
        if blocked {
            break;
        }
    }
    w.lua = host;
    (!blocked).then_some(line)
}

/// A mod's settings: the account file's keys, then the character file's for character-scoped ones.
fn load_settings(m: &manifest::Manifest, data: &std::path::Path, char_file: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    let account = read_json(&data.join("settings.json"));
    let character = char_file.map_or(serde_json::Value::Null, |c| read_json(&data.join(format!("settings-{c}"))));
    for (key, decl) in &m.settings {
        let src = if decl.scope == "character" { &character } else { &account };
        if let Some(v) = src.get(key) {
            out.insert(key.clone(), v.clone());
        }
    }
    // Keys set by /lua set before a declaration existed stay readable.
    if let serde_json::Value::Object(o) = account {
        for (k, v) in o {
            out.entry(k).or_insert(v);
        }
    }
    out
}

impl Host {
    /// The Extras pages of the running mods: their declared settings that buttons and sliders can show (bool, enum,
    /// int and number).
    pub(super) fn settings_pages(&self) -> Vec<super::settings::ModPage> {
        use super::settings::{ModPage, ModRow};
        let mut pages = Vec::new();
        for vm in &self.vms {
            let st = vm.st.borrow();
            let mut rows = Vec::new();
            for (key, d) in &vm.manifest.settings {
                let current = st.settings.get(key).cloned().or_else(|| d.default.as_ref().and_then(|v| serde_json::to_value(v).ok())).unwrap_or(serde_json::Value::Null);
                let choices: Vec<(String, serde_json::Value)> = match d.kind.as_str() {
                    "bool" => vec![("ON".into(), true.into()), ("OFF".into(), false.into())],
                    "enum" => d.options.iter().map(|o| (o.clone(), o.as_str().into())).collect(),
                    "int" | "number" => {
                        let (min, max) = (d.min.unwrap_or(0.0), d.max.unwrap_or(100.0));
                        let step = d.step.unwrap_or(if d.kind == "int" { 1.0 } else { (max - min) / 20.0 }).max(f64::EPSILON);
                        let n = (((max - min) / step).floor() as usize).min(200);
                        (0..=n)
                            .map(|i| {
                                let v = min + step * i as f64;
                                if d.kind == "int" { (format!("{}", v.round() as i64), (v.round() as i64).into()) } else { (format!("{v}"), v.into()) }
                            })
                            .collect()
                    }
                    _ => continue,
                };
                let at = choices.iter().position(|c| c.1 == current || c.1.as_f64().zip(current.as_f64()).is_some_and(|(a, b)| (a - b).abs() < 1e-9)).unwrap_or(0);
                let label = if d.label.is_empty() { key.clone() } else { d.label.clone() };
                rows.push(ModRow { key: key.clone(), label, help: d.help.clone(), choices, at });
            }
            // The permissions it asks for, each allowed or not (docs/lua-mods.md 3.5).
            for p in &vm.manifest.permissions {
                let (label, help) = permission_text(p);
                let on = st.granted.contains(p);
                let choices = vec![("ON".into(), true.into()), ("OFF".into(), false.into())];
                rows.push(ModRow { key: format!("{PERM}{p}"), label: label.into(), help: help.into(), choices, at: if on { 0 } else { 1 } });
            }
            if !rows.is_empty() {
                pages.push(ModPage { id: vm.manifest.id.clone(), name: vm.manifest.name.clone(), rows });
            }
        }
        pages
    }

    /// A setting changed on its Extras page: kept, written to its scope's file, and `settings_changed` sent.
    pub(super) fn apply_setting(&mut self, w: &World, id: &str, key: &str, value: serde_json::Value) {
        if let Some(p) = key.strip_prefix(PERM) {
            if let Some(k) = self.vms.iter().position(|v| v.manifest.id == id) {
                self.set_grant(k, p, value.as_bool() == Some(true));
            }
            return;
        }
        let Some(vm) = self.vms.iter().find(|v| v.manifest.id == id) else { return };
        vm.st.borrow_mut().settings.insert(key.to_string(), value.clone());
        let scope = vm.manifest.settings.get(key).map_or("account", |d| d.scope.as_str());
        let dir = data_dir().join(id);
        let file = match Self::char_file(Some(w)) {
            Some(c) if scope == "character" => dir.join(format!("settings-{c}")),
            _ => dir.join("settings.json"),
        };
        let mut v = read_json(&file);
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v[key] = value.clone();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::fs::write(&file, serde_json::to_vec_pretty(&v).unwrap_or_default()) {
            log::warn!("lua mod {id}: saving {}: {e}", file.display());
        }
        lend(vm, w, |vm| {
            let v = api::to_lua(&vm.lua, &value).unwrap_or(mlua::Value::Nil);
            vm.emit("settings_changed", (key.to_string(), v));
        });
    }
}

impl Host {
    /// Whether a mod window has the keys.
    pub(super) fn focused(&self) -> bool {
        !self.all_hidden && self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().windows.iter().any(|w| w.open && w.focus))
    }

    /// The loaded mods' ids.
    pub(super) fn mod_ids(&self) -> std::collections::HashSet<String> {
        self.vms.iter().map(|v| v.manifest.id.clone()).collect()
    }

    /// The open modal window (vm, id), the last opened when there are several.
    pub(super) fn modal(&self) -> Option<(usize, String)> {
        self.vms.iter().enumerate().filter(|(_, v)| v.suspended.is_none()).find_map(|(k, v)| v.st.borrow().windows.iter().rev().find(|w| w.open && w.modal).map(|w| (k, w.id.clone())))
    }

    /// Whether a mod window's line is being typed (it takes the text and the keys).
    pub(super) fn typing(&self) -> bool {
        !self.all_hidden && self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().windows.iter().any(|w| w.open && w.focus && w.typing()))
    }

    /// The focused window's title and the help under its cursor, for the classic help bar.
    pub(super) fn focused_help(&self) -> Option<(String, String)> {
        self.vms.iter().filter(|v| v.suspended.is_none()).find_map(|v| v.st.borrow().windows.iter().find(|w| w.open && w.focus).map(|w| (w.title.clone(), w.help_now().to_string())))
    }
}

/// Typed text for the focused window's line.
pub(super) fn window_text(w: &mut World, events: &[egui::Event]) {
    let host = &mut w.lua;
    let mut hit = None;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let mut st = vm.st.borrow_mut();
        if let Some(win) = st.windows.iter_mut().find(|w| w.open && w.focus && w.typing()) {
            let (ev, se) = win.text(events);
            hit = Some((vm.manifest.id.clone(), win.id.clone(), ev, se));
            break;
        }
    }
    let Some((id, win, ev, se)) = hit else { return };
    host.ui_events.extend(ev.into_iter().map(|e| (id.clone(), win.clone(), e)));
    w.sounds.extend(se.into_iter().map(|s| s as u32));
}

/// A key for the focused mod window, if any (its events go to the mod's handler next phase).
pub(super) fn window_key(w: &mut World, key: egui::Key) -> bool {
    let host = &mut w.lua;
    let mut hit = None;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let mut st = vm.st.borrow_mut();
        if let Some(win) = st.windows.iter_mut().find(|w| w.open && w.focus) {
            let (ev, se) = win.key(key);
            hit = Some((vm.manifest.id.clone(), win.id.clone(), ev, se));
            break;
        }
    }
    let Some((id, win, ev, se)) = hit else { return false };
    host.ui_events.extend(ev.into_iter().map(|e| (id.clone(), win.clone(), e)));
    w.sounds.extend(se.into_iter().map(|s| s as u32));
    true
}

/// The open mod windows in the classic UI, cascaded from the Config pages' place; their rects.
pub(super) fn draw_classic(gu: &mut super::game_ui::GameUi, w: &World) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for (v, id, x, y) in classic_places(w) {
        if let Some(win) = w.lua.vms[v].st.borrow().windows.iter().find(|x| x.id == id) {
            out.push(win.draw_classic(gu, w.item_info.as_deref(), x, y));
        }
    }
    out
}

impl Host {
    /// The open windows (mod id, a copy of the window), for the modern UI's panels.
    pub(super) fn open_windows(&self) -> Vec<(String, window::Window)> {
        let mut out = Vec::new();
        if self.all_hidden {
            return out;
        }
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for win in vm.st.borrow().windows.iter().filter(|w| w.open) {
                out.push((vm.manifest.id.clone(), win.clone()));
            }
        }
        out
    }
}

/// close_popup by mod and window ids (the modern UI).
pub(super) fn close_popup_by(w: &mut World, mod_id: &str, id: &str) {
    if let Some(k) = w.lua.vms.iter().position(|v| v.manifest.id == mod_id) {
        close_popup(w, k, id);
    }
}

/// A popup closed by a click outside it (a modal prompt stays).
pub(super) fn close_popup(w: &mut World, k: usize, id: &str) {
    let vm = &w.lua.vms[k];
    let mut st = vm.st.borrow_mut();
    if let Some(win) = st.windows.iter_mut().find(|x| x.id == id && x.popup && x.open) {
        (win.open, win.focus, win.edit) = (false, false, false);
        w.lua.ui_events.push((vm.manifest.id.clone(), id.to_string(), window::Event::Close));
        w.sounds.push(crate::audio::Ui::Cancel as u32);
    }
}

/// The pointer on a mod window (modern UI): it takes the focus, then the click acts as the window model says.
pub(super) fn window_click(w: &mut World, mod_id: &str, win_id: &str, click: super::modern::ModClick) {
    use super::modern::ModClick as C;
    let host = &mut w.lua;
    // An open modal window takes no clicks meant for another.
    if let Some((k, id)) = host.modal()
        && (host.vms[k].manifest.id != mod_id || id != win_id)
    {
        return;
    }
    let mut out = None;
    let mut ended = Vec::new();
    for vm in host.vms.iter() {
        let mut st = vm.st.borrow_mut();
        let mine = vm.manifest.id == mod_id;
        let hud = mine && st.windows.iter().any(|w| w.id == win_id && w.hud);
        for win in st.windows.iter_mut() {
            let this = mine && win.id == win_id && win.open;
            if !hud {
                // A window losing the focus ends its edit.
                if win.focus && !this {
                    ended.extend(win.end_edit().into_iter().map(|e| (vm.manifest.id.clone(), win.id.clone(), e)));
                }
                win.focus = this;
            }
            if this {
                out = Some(match click {
                    C::Button(i, c) => win.click_button(i, c),
                    C::Slide(i, s) => win.slide(i, s),
                    C::Row(i, r) => win.pick(i, r),
                    C::Turn(i, d) => win.turn_to(i, d, None),
                    C::TurnTo(i, k) => win.turn_to(i, 0, Some(k)),
                    C::Colour(i, rgb) => win.set_colour(i, Some(rgb), None),
                    C::Alpha(i, s) => win.set_colour(i, None, Some(s)),
                    C::Sort(i, c) => win.sort(i, c),
                    C::Canvas(i, x, y) => win.canvas_click(i, x, y),
                    C::CanvasDrag(i, phase, x, y) => win.canvas_drag(i, phase, x, y),
                });
            }
        }
    }
    host.ui_events.extend(ended);
    if let Some((ev, se)) = out {
        host.ui_events.extend(ev.into_iter().map(|e| (mod_id.to_string(), win_id.to_string(), e)));
        w.sounds.extend(se.into_iter().map(|s| s as u32));
    }
}

/// The classic places of the open windows, in draw order (cascaded from the Config pages' place; modal ones last,
/// centred, or at their anchor kept on screen).
fn classic_places(w: &World) -> Vec<(usize, String, f32, f32)> {
    let (mut out, mut modal) = (Vec::new(), Vec::new());
    if w.lua.all_hidden {
        return out;
    }
    let (screen, uk) = {
        let f = w.lua.info.borrow();
        (f.window, f.k)
    };
    let (sw, sh) = (screen[0] * uk.0, screen[1] * uk.1);
    let mut k = 0.0;
    for (v, vm) in w.lua.vms.iter().enumerate().filter(|(_, v)| v.suspended.is_none()) {
        for win in vm.st.borrow().windows.iter().filter(|w| w.open) {
            let (ww, wh) = win.classic_size();
            match (win.anchor, win.place) {
                (Some((ax, ay)), _) => modal.push((v, win.id.clone(), (ax * uk.0).min(sw - ww).max(0.0), (ay * uk.1).min(sh - wh).max(0.0))),
                (None, Some((x, y))) if win.modal => modal.push((v, win.id.clone(), x, y)),
                (None, None) if win.modal => modal.push((v, win.id.clone(), ((sw - ww) / 2.0).max(0.0), ((sh - wh) / 2.0).max(0.0))),
                (None, Some((x, y))) => out.push((v, win.id.clone(), x, y)),
                (None, None) => {
                    out.push((v, win.id.clone(), 16.0 + 24.0 * k, 48.0 + 24.0 * k));
                    k += 1.0;
                }
            }
        }
    }
    out.extend(modal);
    out
}

/// The pointer on the classic mod windows (the topmost under it); whether it is over a control.
pub(super) fn pointer_classic(w: &mut World, gu: &super::game_ui::GameUi, p: (f32, f32), moved: bool, clicked: bool, held: (bool, bool, bool)) -> bool {
    let mut places = classic_places(w);
    // An open modal window is the only one the pointer reaches; a click outside a popup closes it.
    if let Some((k, mid)) = w.lua.modal() {
        places.retain(|(v, id, _, _)| *v == k && *id == mid);
        let inside = places.first().is_some_and(|(v, id, x, y)| {
            let (ww, wh) = w.lua.vms[*v].st.borrow().windows.iter().find(|x| x.id == *id).map_or((0.0, 0.0), |x| x.classic_size());
            p.0 >= *x && p.1 >= *y && p.0 < x + ww && p.1 < y + wh
        });
        if clicked && !inside {
            close_popup(w, k, &mid);
            return true;
        }
    }
    for (v, id, x, y) in places.into_iter().rev() {
        let (hover, ev, se, mod_id) = {
            let vm = &w.lua.vms[v];
            let mut st = vm.st.borrow_mut();
            let Some(win) = st.windows.iter_mut().find(|x| x.id == id) else { continue };
            let (wx, wy) = win.classic_size();
            if win.drag.is_none() && win.press.is_none() && (p.0 < x || p.1 < y || p.0 >= x + wx || p.1 >= y + wy) {
                continue;
            }
            let (hover, ev, se) = win.pointer_classic(gu, x, y, p, moved, clicked, held);
            (hover, ev, se, vm.manifest.id.clone())
        };
        // A click on a window gives it the focus (not a HUD's).
        let hud = w.lua.vms[v].st.borrow().windows.iter().any(|x| x.id == id && x.hud);
        if clicked && !hud {
            let mut ended = Vec::new();
            for (j, vm) in w.lua.vms.iter().enumerate() {
                for win in vm.st.borrow_mut().windows.iter_mut() {
                    let this = j == v && win.id == id && win.open;
                    if win.focus && !this {
                        ended.extend(win.end_edit().into_iter().map(|e| (vm.manifest.id.clone(), win.id.clone(), e)));
                    }
                    win.focus = this;
                }
            }
            w.lua.ui_events.extend(ended);
        }
        w.lua.ui_events.extend(ev.into_iter().map(|e| (mod_id.clone(), id.clone(), e)));
        w.sounds.extend(se.into_iter().map(|s| s as u32));
        return hover;
    }
    false
}

impl Host {
    /// F12's lines: each mod's time a frame (average and peak, ms), calls over the soft cap, memory, errors.
    pub(super) fn debug_lines(&self) -> Vec<String> {
        self.vms
            .iter()
            .map(|v| {
                let st = v.st.borrow();
                let state = v.suspended.as_deref().map_or(String::new(), |r| format!("  SUSPENDED ({r})"));
                format!("lua {}  {:.2} ms avg  {:.1} peak  {} slow  {} KB  {} errors{state}", v.manifest.id, st.stats.avg, st.stats.peak, st.stats.slow, v.lua.used_memory() / 1024, st.errors.len())
            })
            .collect()
    }
}

#[cfg(test)]
mod stub_tests {
    /// Every function and event in docs.rs has its section in docs/modding/reference.
    #[test]
    fn reference_complete() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/modding/reference");
        let mut text = String::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            text += &std::fs::read_to_string(e.path()).unwrap();
        }
        for f in super::docs::FUNCS {
            let full = if f.ns.is_empty() { f.name.to_string() } else { format!("{}.{}", f.ns, f.name) };
            assert!(text.contains(&format!("\n## `{full}`\n")), "docs/modding/reference has no section for {full}");
        }
        for e in super::docs::EVENTS {
            assert!(text.contains(&format!("\n## `{}`\n", e.name)), "docs/modding/reference/events.md has no section for {}", e.name);
        }
    }

    /// The stubs in assets/mods/library are what `lua-stubs` writes now (regenerate them after an API change:
    /// `vana-client lua-stubs --out crates/vana-client/assets/mods/library/vana.lua`).
    #[test]
    fn committed_stubs_current() {
        assert_eq!(include_str!("../../../assets/mods/library/vana.lua"), super::stubs(), "assets/mods/library/vana.lua is stale");
    }
}

/// A bundled mod's manifest.
fn bundled_manifest(id: &str) -> Option<manifest::Manifest> {
    BUNDLED.iter().find(|b| b.0 == id).and_then(|b| manifest::parse(b.1, id).ok())
}

/// Every mod on disk and bundled: (id, name, version, enabled in the config, bundled).
pub(super) fn installed() -> Vec<(String, String, String, bool, bool)> {
    let cfg = crate::config::Config::load();
    let enabled = |id: &str, bundled: bool| cfg.mods.iter().find(|r| r.name == id).map_or(bundled, |r| r.enabled);
    let mut out = Vec::new();
    for e in std::fs::read_dir(mods_dir()).into_iter().flatten().flatten() {
        let dir = e.path();
        if let Ok(m) = manifest::read(&dir) {
            let id = m.id.clone();
            out.push((id.clone(), m.name, m.version, enabled(&id, false), false));
        }
    }
    for (id, ..) in BUNDLED {
        if !out.iter().any(|o| o.0 == *id) {
            let m = bundled_manifest(id);
            out.push((id.to_string(), m.as_ref().map_or_else(|| id.to_string(), |m| m.name.clone()), m.map_or_else(String::new, |m| m.version), enabled(id, true), true));
        }
    }
    out.sort();
    out
}

/// The user's grants by mod (mod-data/grants.json): the permissions each may use (docs/lua-mods.md 3.5).
fn grants() -> serde_json::Map<String, serde_json::Value> {
    match read_json(&data_dir().join("grants.json")) {
        serde_json::Value::Object(o) => o,
        _ => serde_json::Map::new(),
    }
}

/// A setting row's key prefix for a permission (the mod's Extras page).
const PERM: &str = "@permission:";

/// A permission's row label and help line.
fn permission_text(p: &str) -> (&'static str, &'static str) {
    match p {
        "actions" => ("Allow game actions", "Casting, items, equipment and the like when you use its commands or windows."),
        "automation" => ("Allow automation", "Game actions on its own: casting, targeting, moving, trading, answering NPCs."),
        "packets.read" => ("Allow reading packets", "Sees every packet the server sends and the client sends."),
        "packets.write" => ("Allow changing packets", "Changes, blocks and sends packets of its own."),
        "input.capture" => ("Allow taking input", "Takes keys and the mouse from the game and presses keys."),
        "files.shared" => ("Allow shared files", "Reads and writes files outside its own folder, in the shared mod data."),
        "net" => ("Allow network", "Connects to the internet (web requests, sockets)."),
        "exec" => ("Allow running programs", "Starts programs on this computer."),
        "clipboard" => ("Allow reading the clipboard", "Reads the text you copied."),
        "multibox" => ("Allow multibox", "Talks to your other game windows on this computer."),
        "ui.native" => ("Allow game windows", "Opens, hides and drives the game's own windows."),
        "process" => ("Allow process settings", "Sets which CPU cores the game runs on, and its priority."),
        "devices" => ("Allow devices", "Talks to USB devices directly (G-keys, Stream Decks and the like)."),
        _ => ("Allow", ""),
    }
}

impl Host {
    /// Allows or withdraws permission `p` (one the mod asks for) for the mod at `k`; kept in grants.json.
    fn set_grant(&mut self, k: usize, p: &str, on: bool) {
        let id = self.vms[k].manifest.id.clone();
        let mut g = grants();
        let mut list: Vec<String> = g.get(&id).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        list.retain(|x| x != p);
        if on && manifest::PERMISSIONS.contains(&p) {
            list.push(p.to_string());
        }
        g.insert(id, list.clone().into());
        save_grants(&g);
        let asked = self.vms[k].manifest.permissions.clone();
        self.vms[k].st.borrow_mut().granted = asked.into_iter().filter(|p| list.contains(p)).collect();
    }
}

fn save_grants(g: &serde_json::Map<String, serde_json::Value>) {
    let _ = std::fs::create_dir_all(data_dir());
    if let Err(e) = std::fs::write(data_dir().join("grants.json"), serde_json::to_vec_pretty(g).unwrap_or_default()) {
        log::warn!("lua mods: saving grants: {e}");
    }
}

/// The last packet of each id seen each way (packets.last), shared by every mod.
#[derive(Default)]
pub(super) struct LastPackets {
    pub incoming: std::collections::HashMap<u16, Vec<u8>>,
    pub outgoing: std::collections::HashMap<u16, Vec<u8>>,
}

/// A sub-packet's id from its header (9 bits).
fn packet_id(data: &[u8]) -> u16 {
    if data.len() < 2 { 0 } else { u16::from_le_bytes([data[0], data[1]]) & 0x1ff }
}

/// The header's size field (bits 9.., in 4-byte words) made to fit `data` (padded to a whole word).
pub(super) fn fit(mut data: Vec<u8>) -> Vec<u8> {
    let len = data.len().max(4).div_ceil(4) * 4;
    data.resize(len, 0);
    let id = packet_id(&data);
    data[..2].copy_from_slice(&(id | ((len / 4) as u16) << 9).to_le_bytes());
    data
}

/// Every mod's hooks of one direction over one packet, in load order, then the order each mod added them: each sees the
/// original and the current bytes, whether it was blocked by an earlier hook, and whether it was injected. The
/// packet's bytes after them, or none when blocked.
fn chain(host: &Host, w: &World, data: Vec<u8>, injected: bool, outgoing: bool) -> Option<Vec<u8>> {
    let id = packet_id(&data);
    let original = data.clone();
    let (mut cur, mut blocked) = (data, false);
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let funcs: Vec<mlua::Function> = {
            let st = vm.st.borrow();
            let hooks = if outgoing { &st.hooks_out } else { &st.hooks_in };
            hooks.iter().filter(|h| h.1.is_none_or(|i| i == id)).filter_map(|h| vm.lua.registry_value(&h.2).ok()).collect()
        };
        if funcs.is_empty() {
            continue;
        }
        let write = vm.st.borrow().granted.iter().any(|g| g == "packets.write");
        lend(vm, w, |vm| {
            for f in funcs {
                let p = (|| -> mlua::Result<mlua::Table> {
                    let t = vm.lua.create_table()?;
                    t.set("id", id)?;
                    t.set("data", vm.lua.create_string(&cur)?)?;
                    t.set("original", vm.lua.create_string(&original)?)?;
                    t.set("injected", injected)?;
                    t.set("blocked", blocked)?;
                    t.set("outgoing", outgoing)?;
                    Ok(t)
                })();
                let Ok(p) = p else { continue };
                vm.st.borrow_mut().deadline = Some(std::time::Instant::now() + vm::FILTER_CAP);
                let r = f.call::<mlua::Value>(p);
                vm.st.borrow_mut().deadline = None;
                match r {
                    Err(e) => vm.error(&e),
                    Ok(mlua::Value::Boolean(false)) | Ok(mlua::Value::String(_)) if !write => {
                        vm.error(&mlua::Error::runtime("changing or blocking packets needs packets.write (/lua grant)"));
                    }
                    Ok(mlua::Value::Boolean(false)) => blocked = true,
                    Ok(mlua::Value::String(s)) => {
                        let b = s.as_bytes().to_vec();
                        if b.len() > 508 {
                            vm.error(&mlua::Error::runtime(format!("a packet is at most 508 bytes ({} returned): kept as it was", b.len())));
                        } else if b.len() >= 4 {
                            cur = fit(b);
                        }
                    }
                    Ok(_) => {}
                }
            }
        });
    }
    (!blocked).then_some(cur)
}

/// A received batch through the mods' incoming hooks, then what they injected; what to decode.
pub(super) fn packets_in(w: &mut World, packets: Vec<crate::net::map::session::SubPacket>) -> Vec<crate::net::map::session::SubPacket> {
    let host = std::mem::take(&mut w.lua);
    let mut out = Vec::new();
    let injected: Vec<Vec<u8>> = host.vms.iter().flat_map(|v| std::mem::take(&mut v.st.borrow_mut().inject_in)).collect();
    let all = packets.into_iter().map(|p| (p.sync, p.data, false)).chain(injected.into_iter().map(|d| (0, d, true)));
    for (sync, data, inj) in all {
        let id = packet_id(&data);
        host.last.borrow_mut().incoming.insert(id, data.clone());
        if let Some(d) = chain(&host, w, data, inj, false) {
            out.push(crate::net::map::session::SubPacket { id: packet_id(&d), sync, data: d });
        }
    }
    w.lua = host;
    out
}

/// A batch about to be sent through the mods' outgoing hooks, then what they injected; what to send.
pub(super) fn packets_out(w: &mut World, packets: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let host = std::mem::take(&mut w.lua);
    let injected: Vec<Vec<u8>> = host.vms.iter().flat_map(|v| std::mem::take(&mut v.st.borrow_mut().inject_out)).collect();
    let mut out = Vec::new();
    for (data, inj) in packets.into_iter().map(|d| (d, false)).chain(injected.into_iter().map(|d| (d, true))) {
        let id = packet_id(&data);
        host.last.borrow_mut().outgoing.insert(id, data.clone());
        if let Some(d) = chain(&host, w, data, inj, true) {
            out.push(d);
        }
    }
    w.lua = host;
    out
}

impl Host {
    /// After the mod phase: the hook flags for the net thread.
    fn packet_upkeep(&self, w: &World) {
        let (mut fin, mut fout) = (false, false);
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            let st = vm.st.borrow();
            fin |= !st.hooks_in.is_empty();
            fout |= !st.hooks_out.is_empty();
        }
        let flags = if fin { super::net::HOOK_IN } else { 0 } | if fout { super::net::HOOK_OUT } else { 0 };
        w.net.hooks.store(flags, std::sync::atomic::Ordering::Relaxed);
    }
}
