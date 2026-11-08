//! Lua mods (docs/lua-mods.md): a VM per enabled mod with a mod.toml in its folder under mods/, created when the
//! character enters the world and dropped at logout. Once a frame the mod phase delivers events, commands, timers and
//! wake-ups; what the mods asked for is applied after it.

mod actions;
mod api;
pub(super) mod audio;
mod devices;
mod docs;
mod draw;
pub(super) mod hud;
pub(super) mod input;
mod ipc;
pub(super) mod lobby;
mod manifest;
mod native;
mod net;
mod pattern;
mod process;
pub(super) mod render;
mod runtime;
pub(super) mod skin;
mod sock;
mod state;
mod vm;
pub(super) mod window;

pub(in crate::client) use input::combo_label;
pub(in crate::client) use state::Placed;

use super::World;
use crate::dat::battle::Who;
use std::path::{Path, PathBuf};
use vm::Vm;

/// What a mod asked the client to do, applied after the mod phase.
pub(super) enum Op {
    /// A line with a mode (its colour, filters and indent), or only a mode's colour.
    Chat {
        text: String,
        mode: Option<u8>,
        colour: Option<u8>,
    },
    Print(String),
    /// commands.run: the line, and whether the mod holds the mods permission.
    /// A line run as typed, whether the mod may control mods, and whether it ran it on the player's input.
    Run(String, bool, bool),
    Act(actions::Action),
    /// A key the mod pressed (input.press / input.send).
    Key(input::Synth),
    /// The chat line opened with this text, or closed.
    Line {
        text: String,
        open: bool,
    },
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
    Timer {
        mod_id: String,
        name: Option<String>,
        secs: Option<f32>,
        section: usize,
        group: Option<String>,
        icon: Option<(String, u16)>,
        total: f32,
        fills: bool,
    },
    /// A lobby action (sign in, play a character), for the app before login.
    Lobby(lobby::Act),
    /// A mod's recording started (file, frames a second) or stopped (client.record).
    Record(String, Option<(std::path::PathBuf, u32)>),
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
pub(super) const OPTIONS: [&str; 22] = [
    "macro_palette_delay",
    "ah_result_delay",
    "equipset_delay",
    "log_slide",
    "cutscene_speed",
    "animation_rate",
    "clear_colour",
    "ambient",
    "minimap_span",
    "map_opacity",
    "move_lock",
    "all_maps",
    "dead_commands",
    "compass_monsters",
    "player_collision",
    "occlusion",
    "menu_reach",
    "send_interval",
    "wireframe",
    "ttf_text",
    "stamp_all_lines",
    "synth_amount",
];

/// The switches retail has off.
const OFF: [&str; 7] =
    ["all_maps", "dead_commands", "compass_monsters", "wireframe", "ttf_text", "stamp_all_lines", "synth_amount"];

/// An {r, g, b} (0..255) as gamma-space floats; false / nil: none.
fn rgb_of(v: &serde_json::Value) -> Option<Option<[f32; 3]>> {
    if v.is_null() || v == &serde_json::Value::Bool(false) {
        return Some(None);
    }
    let get = |k: &str, i: usize| v.get(k).or_else(|| v.get(i)).and_then(|x| x.as_f64());
    Some(Some([get("r", 0)?, get("g", 1)?, get("b", 2)?].map(|c| (c / 255.0).clamp(0.0, 1.0) as f32)))
}

fn rgb_json(c: Option<[f32; 3]>) -> serde_json::Value {
    c.map_or(
        serde_json::Value::Bool(false),
        |c| serde_json::json!({"r": (c[0] * 255.0).round(), "g": (c[1] * 255.0).round(), "b": (c[2] * 255.0).round()}),
    )
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
        ("stamp_all_lines", Some(b)) => w.stamp_all_lines = b,
        ("synth_amount", Some(b)) => w.opts.synth_amount = b,
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
            Some(k) => {
                (w.minimap_span, w.minimap_dirty) =
                    ((k as f32).clamp(super::minimap::SPAN.0, super::minimap::SPAN.1), true)
            }
            None => return false,
        },
        // Saved to config when it changes, so it holds while the Map Overlay mod is off.
        ("map_opacity", _) => match v.as_f64() {
            Some(k) => {
                let k = (k as f32).clamp(0.0, 1.0);
                w.map_opacity_dirty |= k != w.map_opacity;
                w.map_opacity = k;
            }
            None => return false,
        },
        _ => return false,
    }
    true
}

/// Whether a value suits a switch: a boolean, a number, or a colour ({r, g, b} or false).
pub(super) fn option_value_ok(name: &str, v: &serde_json::Value) -> Result<(), &'static str> {
    match name {
        "cutscene_speed" | "animation_rate" | "send_interval" | "minimap_span" | "map_opacity" => {
            v.as_f64().map(|_| ()).ok_or("a number")
        }
        "clear_colour" | "ambient" => rgb_of(v).map(|_| ()).ok_or("a colour {r, g, b} (0..255), or false"),
        _ => v.as_bool().map(|_| ()).ok_or("true or false"),
    }
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
        "stamp_all_lines" => json!(w.stamp_all_lines),
        "synth_amount" => json!(w.opts.synth_amount),
        "log_slide" => json!(!w.log.anim.instant),
        "cutscene_speed" => json!(w.opts.cutscene_speed),
        "animation_rate" => json!(w.opts.anim_rate),
        "clear_colour" => rgb_json(w.opts.clear),
        "ambient" => rgb_json(w.opts.ambient),
        "minimap_span" => json!(w.minimap_span),
        "map_opacity" => json!(w.map_opacity),
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
    let gone: Vec<String> =
        w.lua.opt_owners.iter().filter(|(_, m)| !loaded.contains(*m)).map(|(n, _)| n.clone()).collect();
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

/// The mods' render.entity looks, borrowed from the frame info (Host::looks).
pub(in crate::client) struct Looks<'a>(std::cell::Ref<'a, std::collections::HashMap<u32, (String, render::Look)>>);

impl Looks<'_> {
    pub fn get(&self, id: &u32) -> Option<&render::Look> {
        self.0.get(id).map(|l| &l.1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &render::Look)> {
        self.0.iter().map(|(id, l)| (id, &l.1))
    }
}

/// Window `w` is open on its edge in place of built-in panel `panel` (edge_panel, has_edge_panel, timers_panels).
fn stands_in(w: &window::Window, panel: &str) -> bool {
    w.open && w.edge && w.panel.as_deref() == Some(panel)
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
    /// `/lua watch`: mods reloaded when their files are saved.
    watch: Option<Watch>,
    /// The keys and mouse, shared with every mod (input.held, input.mouse).
    input: std::rc::Rc<std::cell::RefCell<input::State>>,
    /// Binds pressed since the last mod phase: (mod, bind as written, on release).
    fired: Vec<(String, String, bool)>,
    /// Keys the mods pressed, for the app to hand the game this frame.
    pub(in crate::client) synth: Vec<input::Synth>,
    /// A mod pressed Enter this frame: a chat line it submits is the mod's, not the player's.
    pub(in crate::client) synth_enter: bool,
    /// actions.choose's pick, held until the event's menu takes input (5 s at most).
    pub(in crate::client) choose: Option<(usize, std::time::Instant)>,
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
    /// HUD element events since the last mod phase (clicks, drags, drops, the pointer in and out): (mod, element,
    /// event).
    hud_events: Vec<(String, String, serde_json::Value)>,
    /// Since when the world's mods wait for our character's state.
    start_wait: Option<std::time::Instant>,
    /// The game UI's data for this mod phase, lent to mods loaded during it too (their entry file and `load`).
    gu: Option<*const super::game_ui::GameUi>,
    /// The lobby mods are loaded; the screen they last saw (for `lobby_screen`); what the lobby shows.
    lobby_started: bool,
    /// Watched world mods saved before login: reloaded (or loaded) once there is a world.
    watch_deferred: Vec<String>,
    lobby_screen: Option<&'static str>,
    lobby: lobby::Shared,
    /// A recording asked for (taken by the app), and the file being recorded, as the app sets it.
    /// A mod's recording asked for: (the mod, its file and frames a second, or None to stop its own).
    pub(in crate::client) record_req: Option<(String, Option<(std::path::PathBuf, u32)>)>,
    /// The recording running: its mod and file.
    pub(in crate::client) recording: std::rc::Rc<std::cell::RefCell<Option<(String, std::path::PathBuf)>>>,
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
        } else if (name.ends_with(".lua") || name.ends_with(".toml"))
            && let Ok(m) = e.metadata().and_then(|m| m.modified())
        {
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
        for (name, nv, ov) in [
            ("hp_change", n.0, o.0),
            ("mp_change", n.1, o.1),
            ("tp_change", n.2, o.2),
            ("hpp_change", n.3 as u32, o.3 as u32),
            ("mpp_change", n.4 as u32, o.4 as u32),
        ] {
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
                Some(&(oid, ocount)) if oid == id => {
                    out.push(("item_change", vec![bag.into(), slot.into(), id.into(), count.into(), ocount.into()]))
                }
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
/// Our own features on the mod API (docs/extras-as-mods.md): id, mod.toml, files.
#[cfg(test)]
macro_rules! bundled {
    ($($id:literal $([$($file:literal),*])?),* $(,)?) => {
        &[$(($id, include_str!(concat!("../../../assets/mods/", $id, "/mod.toml")), &[
            ("init.lua", include_str!(concat!("../../../assets/mods/", $id, "/init.lua")))
            $($(, ($file, include_str!(concat!("../../../assets/mods/", $id, "/", $file))))*)?
        ])),*]
    };
}
/// The built-in edge panels a mod's edge window may stand in for (ui.window's `panel`).
pub(in crate::client) const EDGE_PANELS: [&str; 6] =
    ["regime", "timers_recasts", "timers_buffs", "timers_debuffs", "skillchains", "minimap"];

/// The built-in modern panels a mod's ordinary window may stand in for (ui.window's `panel`): its place and docking,
/// /uimodes and hiding; not drawn in the classic UI.
pub(in crate::client) const STAND_IN_PANELS: [&str; 2] = ["focus", "pending"];

/// The timers' sections' panels, in section order.
pub(in crate::client) const TIMERS_PANELS: [&str; 3] = ["timers_recasts", "timers_buffs", "timers_debuffs"];

/// The mods kept in this repository (assets/mods), for the tests: the client loads only installed mods (the mods
/// folder); these reach players from GitHub (Settings > Mods, Download Standard Mods).
#[cfg(test)]
const BUNDLED: &[(&str, &str, &[(&str, &str)])] = bundled!["dps", "auto-advance", "trust-speech", "find", "map-overlay", "clock", "target-info", "battle-text", "timestamps", "synth-amount", "regime", "timers", "skillchain", "minimap", "suggestions", "xp-tracker", "cutscene-speed", "loot-rules", "client-tweaks", "equipment-grid", "inventory-hud", "chat-alerts", "player-bars", "effect-remover", "macro-books", "mount-picker", "log-files", "tell-inbox", "link-catcher", "trust-lineups", "sell-hand-in", "web-search", "camera-range", "chat-filter", "target-bar", "quick-commands", "countdowns", "party-tools", "key-commands", "event-rules", "music-picker", "typing-helpers", "zone-banner", "use-all", "hide-rules", "screen-notes", "calculator", "attendance", "screen-capture", "quick-play", "target-marker", "cast-grid", "target-debuffs", "skill-log", "swing-timer", "weather-watch", "targeting", "entity-styles", "visual-tweaks", "window-placement", "range-rings", "tracking", "pet-status", "job-gauges", "roll-helper", "automaton-sets", "loot-tracker", "monster-alerts", "gathering-log", "casket-solver",
    "blue-sets" ["src/learn.lua"],
    "warp-helper" ["src/places.lua"],
    "dialogue-bubbles",
    "look-changer" ["src/models.lua"],
    "content-trackers" ["src/omen.lua", "src/delve.lua", "src/odyssey.lua", "src/sortie.lua", "src/reive.lua", "src/pops.lua", "src/empy.lua", "src/znm.lua"],
    "auction-desk",
    "npc-errands",
    "completion-log",
    "monster-notes",
    "chat-windows",
    "multibox",
    "gear-stats",
    "item-details" ["src/trials.lua"],
    "objective-sets",
    "focus-target",
    "pending-actions",
    "action-panel",
    "map-notes",
    "exit-markers",
    "list-trimmer",
    "bag-manager",
    "gearshift" ["src/core.lua", "src/compat.lua", "src/names.lua", "src/gs.lua", "src/lac.lua", "src/windower.lua", "src/xml.lua", "src/acxml.lua", "src/modes.lua", "src/texts.lua", "src/regex.lua", "src/spellcast.lua"],
    "hotbar" ["src/actions.lua", "src/bar.lua", "src/data.lua", "src/jobs.lua", "src/keys.lua", "src/manager.lua", "src/runner.lua",
        "src/stock.lua"],
    "pad-bars" ["src/actions.lua", "src/runner.lua"],
    // After gearshift: its hooks see an action first.
    "steady-cast" ["src/hold.lua"],
];

/// Where mods keep their settings and storage: beside an FFXI_CONFIG copy for test runs.
fn data_dir() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("mod-data"),
        None => crate::config::data_dir().join("mod-data"),
    }
}

/// The folder in `mods` of the mod `id` names: the folder of that name, else the one whose manifest has that id.
fn folder_of(mods: &Path, id: &str) -> PathBuf {
    let named = mods.join(id);
    if named.join("mod.toml").is_file() {
        return named;
    }
    std::fs::read_dir(mods)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| manifest::read(p).is_ok_and(|m| m.id == id))
        .unwrap_or(named)
}

/// The mods' folders; a test run's own (`mods` beside its FFXI_CONFIG copy) when it has one, so test mods never sit
/// among the user's.
fn mods_dir() -> PathBuf {
    if std::env::var_os("VANA_MODS_DIR").is_some() {
        return crate::config::mods_dir();
    }
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

    /// Whether a running mod has a handler for an event.
    pub(in crate::client) fn listens(&self, event: &str) -> bool {
        self.vms.iter().any(|v| v.suspended.is_none() && v.has_handler(event))
    }

    /// Whether a typed line is for the mods: //anything, /lua, or a mod's registered /command.
    pub fn takes(&self, line: &str) -> bool {
        if line.starts_with("//") {
            return true;
        }
        let first = line.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        first == "lua"
            || self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().commands.iter().any(|(n, _)| *n == first))
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

    /// A bundled mod (assets/mods) by id (tests).
    #[cfg(test)]
    fn load_bundled(&mut self, w: Option<&World>, id: &str) -> Result<(), String> {
        let (_, text, files) = BUNDLED.iter().find(|b| b.0 == id).ok_or_else(|| format!("no bundled mod {id}"))?;
        let m = manifest::parse(text, id).map_err(|e| format!("{e:#}"))?;
        self.load_with(w, m, PathBuf::from("bundled").join(id), vm::Source::Bundled(files))
    }

    fn load_with(
        &mut self,
        w: Option<&World>,
        m: manifest::Manifest,
        dir: PathBuf,
        src: vm::Source,
    ) -> Result<(), String> {
        if self.vms.iter().any(|v| v.manifest.id == m.id) {
            return Err(format!("{} is already loaded", m.id));
        }
        let data = data_dir().join(&m.id);
        let char_file = Self::char_file(w);
        if m.id == "regime"
            && let (Some(w), Some(f)) = (w, char_file.as_deref())
        {
            migrate_regime(w, &data, f);
        }
        let storage = (
            read_json(&data.join("account.json")),
            char_file.as_ref().map_or(serde_json::Value::Null, |f| read_json(&data.join(f))),
        );
        migrate_settings(&m, &data, char_file.as_deref());
        let mut settings = load_settings(&m, &data, char_file.as_deref());
        force_on(&m, &mut settings);
        let given: Vec<String> = grants()
            .get(&m.id)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        // A bundled mod is the client's own code: it has what it asks for.
        let bundled = matches!(src, vm::Source::Bundled(_));
        let granted: Vec<String> = m.permissions.iter().filter(|p| bundled || given.contains(p)).cloned().collect();
        let missing: Vec<String> = m.permissions.iter().filter(|p| !granted.contains(p)).cloned().collect();
        let vm = Vm::new(m, dir, src).map_err(|e| format!("{e:#}"))?;
        vm.st.borrow_mut().char_file = Self::char_file(w);
        vm.st.borrow_mut().settings = settings;
        vm.st.borrow_mut().granted = granted;
        if !missing.is_empty() {
            let id = vm.manifest.id.clone();
            vm.st.borrow_mut().notices.push(format!(
                "{} asks for {}: allow it on its Extras page (/lua settings {id}) or with /lua grant {id} {}.",
                vm.manifest.name,
                missing.join(", "),
                missing.join(" ")
            ));
        }
        vm.lua.set_app_data(self.input.clone());
        vm.lua.set_app_data(self.dir.clone());
        vm.lua.set_app_data(self.info.clone());
        vm.lua.set_app_data(self.lobby.clone());
        vm.lua.set_app_data(Recording(self.recording.clone()));
        if let Some(p) = self.gu {
            vm.lua.set_app_data(api::GuRef(p));
        }
        api::install(&vm.lua, &vm, storage).map_err(|e| format!("{e}"))?;
        self.dir.borrow_mut().mods.push(runtime::Entry {
            id: vm.manifest.id.clone(),
            name: vm.manifest.name.clone(),
            version: vm.manifest.version.clone(),
            lua: vm.lua.weak(),
            st: vm.st.clone(),
        });
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
        info.placed.retain(|_, p| p.0 != vm.manifest.id);
        info.opacity.retain(|_, p| p.0 != vm.manifest.id);
        info.slots.retain(|_, s| s.0 != vm.manifest.id);
        info.skin.forget(&vm.manifest.id);
        info.looks.retain(|_, l| l.0 != vm.manifest.id);
        let n = info.models.len();
        info.models.retain(|_, m| m.0 != vm.manifest.id);
        if info.models.len() != n {
            info.models_ver += 1;
        }
        info.item_tints.retain(|_, t| t.0 != vm.manifest.id);
        info.trimmed.retain(|k, _| k.0 != vm.manifest.id);
        info.shaders.retain(|s| s.0 != vm.manifest.id);
        drop(info);
        self.filters_dirty = true;
        lend_opt(&vm, w, |vm| vm.emit("unload", ()));
        save(&vm, w);
        // The keys it held for the game come up.
        let held = std::mem::take(&mut vm.st.borrow_mut().keys_down);
        self.synth.extend(held.into_iter().map(|key| input::Synth {
            key,
            down: Some(false),
            ctrl: false,
            alt: false,
            shift: false,
        }));
        log::info!("lua mod {} unloaded", vm.manifest.id);
    }

    /// The enabled mods (config `mods`) whose folders have a mod.toml; the lobby mods are running already and get
    /// the character's storage and settings.
    fn start(&mut self, w: &mut World) {
        self.started = true;
        // In the world from here on: the mods loading now may queue world ops.
        self.lobby.borrow_mut().screen = "world";
        let cfg = crate::config::Config::load();
        let root = mods_dir();
        install_stubs(root.clone());
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
            vm.st.borrow_mut().char_file = file.clone();
            // Its character-scoped settings declared `from` the config waited for a character (migrate_settings).
            migrate_settings(&vm.manifest, &data, file.as_deref());
            vm.st.borrow_mut().settings = load_settings(&vm.manifest, &data, file.as_deref());
            force_on(&vm.manifest, &mut vm.st.borrow_mut().settings);
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

    /// A mod by id: its folder in the mods folder.
    fn load_named(&mut self, w: Option<&World>, id: &str) -> Result<(), String> {
        self.load(w, mods_dir().join(id))
    }

    /// Reloads the mod at `k` from its files, in the same place in the load order; its id.
    fn reload(&mut self, w: Option<&World>, k: usize) -> Result<String, String> {
        let dir = self.vms[k].dir.clone();
        let id = self.vms[k].manifest.id.clone();
        #[cfg(test)]
        let bundled = matches!(self.vms[k].src, vm::Source::Bundled(_));
        self.unload(w, k);
        #[cfg(test)]
        let r = if bundled { self.load_bundled(w, &id) } else { self.load(w, dir) };
        #[cfg(not(test))]
        let r = self.load(w, dir);
        r.map_err(|e| format!("{id}: {e}"))?;
        let vm = self.vms.pop().expect("just loaded");
        self.vms.insert(k.min(self.vms.len()), vm);
        Ok(id)
    }

    /// Saved files of watched mods: each reloaded (or loaded, when it failed before). Before login (`w` None) only
    /// the lobby mods, by the manifest as saved: a world mod loaded there would run its load without a world, and
    /// `start` would then skip it as already loaded. A world mod saved then waits for the world (one that was a lobby
    /// mod and no longer is unloads now); a mod `start` loads from its saved files anyway is not loaded twice.
    fn watch_upkeep(&mut self, w: Option<&World>, out: &mut Vec<(String, [u8; 4])>) {
        let mut ids: Vec<String> = self.watch.as_ref().map(|wa| wa.rx.try_iter().collect()).unwrap_or_default();
        if w.is_some() && !self.watch_deferred.is_empty() {
            let loaded = |h: &Self, id: &str| h.vms.iter().any(|v| v.manifest.id.eq_ignore_ascii_case(id));
            let deferred: Vec<String> =
                std::mem::take(&mut self.watch_deferred).into_iter().filter(|id| !loaded(self, id)).collect();
            ids.splice(0..0, deferred);
        }
        for id in ids {
            if w.is_none() {
                let at = self.vms.iter().position(|v| v.manifest.id.eq_ignore_ascii_case(&id));
                let lobby = match manifest::read(&mods_dir().join(&id)) {
                    Ok(m) => m.lobby,
                    Err(_) => at.is_some_and(|k| self.vms[k].manifest.lobby),
                };
                if !lobby {
                    if let Some(k) = at {
                        self.unload(None, k);
                        out.push((
                            format!("Unloaded {id}: no longer a lobby mod, it loads with the world."),
                            super::WHITE,
                        ));
                    }
                    if !self.watch_deferred.iter().any(|d| d.eq_ignore_ascii_case(&id)) {
                        self.watch_deferred.push(id);
                    }
                    continue;
                }
            }
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
            // The character's settings go with it: the lobby sees the account's and the defaults, as before login.
            vm.st.borrow_mut().settings = load_settings(&vm.manifest, &data_dir().join(&vm.manifest.id), None);
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
                    out.push((
                        format!(
                            "{} {} ({state}): {:.2} ms avg, {:.1} ms peak, {} KB",
                            v.manifest.id,
                            v.manifest.version,
                            st.stats.avg,
                            st.stats.peak,
                            v.lua.used_memory() / 1024
                        ),
                        super::WHITE,
                    ));
                }
            }
            // Every mod's drawing hidden or shown again (hideui); the mods keep running.
            "hide" | "show" => {
                self.all_hidden = args[0] == "hide";
                out.push((
                    if self.all_hidden { "Mods hidden (/lua show brings them back)." } else { "Mods shown." }.into(),
                    super::WHITE,
                ));
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
                    let id = self
                        .vms
                        .iter()
                        .find(|v| v.manifest.id.eq_ignore_ascii_case(id))
                        .map_or(id.to_string(), |v| v.manifest.id.clone());
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
                        let _ = std::fs::write(
                            data_dir().join("watch.json"),
                            serde_json::to_vec(&list).unwrap_or_default(),
                        );
                        self.set_watch(&id, on);
                        out.push((
                            if on {
                                format!("Watching {id}: saving its files reloads it.")
                            } else {
                                format!("No longer watching {id}.")
                            },
                            super::WHITE,
                        ));
                    }
                }
                None => {
                    let list = watched();
                    out.push((
                        if list.is_empty() {
                            "/lua watch <mod>: reload it whenever its files are saved.".into()
                        } else {
                            format!("Watching: {}.", list.join(", "))
                        },
                        super::WHITE,
                    ));
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
                    match self
                        .vms
                        .iter()
                        .find(|v| v.manifest.id.eq_ignore_ascii_case(id))
                        .map(|v| v.manifest.id.clone())
                    {
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
                let id = args
                    .get(1)
                    .and_then(|id| find(&self.vms, id))
                    .or((!self.vms.is_empty()).then_some(0))
                    .map(|k| self.vms[k].manifest.id.clone());
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
                    out.push((
                        format!("{id}: granted {}.", if list.is_empty() { "nothing".into() } else { list.join(", ") }),
                        super::WHITE,
                    ));
                }
                None => {
                    out.push(("/lua grant <mod> <permission...> | /lua revoke <mod> <permission...>".into(), ERROR))
                }
            },
            other => out.push((
                format!("/lua {other}: list, load, unload, reload, watch, run, set, settings, grant, revoke"),
                ERROR,
            )),
        }
    }
}

/// An error line's colour (BGRA, 0x80 = 1.0: the log's red).
pub(in crate::client) const ERROR: [u8; 4] = [0x30, 0x30, 0x80, 0x80];
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
    let char_file = Host::char_file(w).or_else(|| vm.st.borrow().char_file.clone());
    for (key, file) in [("account", Some("account.json".to_string())), ("character", char_file)] {
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
            vm.st.borrow_mut().notices.push(format!(
                "{}: left out of the save (not plain data): {}",
                vm.manifest.name,
                skipped.join(", ")
            ));
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
                let sub = args.first().map(|a| a.to_ascii_lowercase());
                let player_only = matches!(sub.as_deref(), Some("grant" | "revoke" | "run" | "set" | "watch"));
                if player_only && from != From::Player {
                    out.push(("/lua grant, revoke, run, set and watch only work when you type them.".into(), ERROR));
                } else if sub.as_deref().is_some_and(|s| MOD_CONTROL.contains(&s)) && !from.controls_mods() {
                    out.push((
                        "/lua load, unload, reload, hide and show from a mod need its mods permission.".into(),
                        ERROR,
                    ));
                } else {
                    host.lua_command(w, &args, &mut out);
                }
                continue;
            }
            let input = !matches!(from, From::Mod { input: false, .. });
            let owner = host
                .vms
                .iter()
                .find(|v| v.suspended.is_none() && v.st.borrow().commands.iter().any(|(n, _)| *n == name));
            match owner {
                Some(vm) => lend(vm, w, |vm| {
                    let f = vm
                        .st
                        .borrow()
                        .commands
                        .iter()
                        .find(|(n, _)| *n == name)
                        .and_then(|(_, k)| vm.lua.registry_value::<mlua::Function>(k).ok());
                    if let Some(f) = f {
                        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                        if input {
                            vm.call_input(f, mlua::Variadic::from_iter(args))
                        } else {
                            vm.call(f, mlua::Variadic::from_iter(args))
                        }
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
            let Some(vm) = host.vms.iter().find(|v| {
                v.manifest.id == m && v.suspended.is_none() && v.st.borrow().granted.iter().any(|g| g == "multibox")
            }) else {
                continue;
            };
            lend(vm, w, |vm| vm.emit("ipc_message", (text.clone(), from.clone())));
        }
        hud::hover(&mut host);
        for (m, id, mut e) in std::mem::take(&mut host.hud_events) {
            let Some(vm) = host.vms.iter().find(|v| v.manifest.id == m && v.suspended.is_none()) else { continue };
            let f = vm
                .st
                .borrow()
                .hud_handlers
                .iter()
                .find(|h| h.0 == id)
                .and_then(|h| vm.lua.registry_value::<mlua::Function>(&h.1).ok());
            if let Some(f) = f {
                e["id"] = id.clone().into();
                // The pointer coming and going is not the player acting.
                let input = !matches!(e["kind"].as_str(), Some("mouse_enter" | "mouse_leave"));
                lend(vm, w, |vm| {
                    let t = api::to_lua(&vm.lua, &e).unwrap_or(mlua::Value::Nil);
                    if input { vm.call_input(f, (id.clone(), t)) } else { vm.call(f, (id.clone(), t)) }
                });
            }
        }
        // Events mods sent each other (mods.emit), to every other mod listening.
        let emitted = std::mem::take(&mut host.dir.borrow_mut().emitted);
        for (from, name, args) in &emitted {
            for vm in host.vms.iter().filter(|v| v.suspended.is_none() && v.manifest.id != *from && v.has_handler(name))
            {
                lend(vm, w, |vm| {
                    let vals: Vec<mlua::Value> =
                        args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
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
                    let vals: Vec<mlua::Value> =
                        args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
                let mine: Vec<(String, window::Event)> =
                    ui_events.iter().filter(|e| e.0 == vm.manifest.id).map(|e| (e.1.clone(), e.2.clone())).collect();
                for (win, ev) in mine {
                    let f = vm
                        .st
                        .borrow()
                        .window_handlers
                        .iter()
                        .find(|h| h.0 == win)
                        .and_then(|h| vm.lua.registry_value::<mlua::Function>(&h.1).ok());
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
            out.push((
                format!("Mod {} suspended: {why}. /lua reload {} to start it again.", vm.manifest.name, vm.manifest.id),
                ERROR,
            ));
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
    let wanted: Option<(usize, String)> =
        host.vms.iter().enumerate().find_map(|(k, v)| v.st.borrow_mut().focus_request.take().map(|id| (k, id)));
    let held = host.modal().is_some();
    let wanted =
        wanted.filter(|(k, id)| !held || host.vms[*k].st.borrow().windows.iter().any(|w| w.id == *id && w.modal));
    if let Some((k, id)) = wanted {
        for (j, vm) in host.vms.iter().enumerate() {
            for win in vm.st.borrow_mut().windows.iter_mut() {
                win.focus = j == k && win.id == id && win.open;
            }
        }
    }
    let dirty = std::mem::take(&mut host.filters_dirty)
        | host.vms.iter().fold(false, |d, v| d | std::mem::take(&mut v.st.borrow_mut().filters_dirty));
    if dirty {
        w.chat.filter = host.chat_filter();
        w.chat.mode_colours = w.battle.as_ref().map(|b| std::rc::Rc::new(b.code_tables(&w.game).1));
    }
    // `/lua settings`: the modern UI shows the mods' pages in the Settings window (Mods, Mod Settings); the classic
    // page stands alone.
    host.packet_upkeep(w);
    // The modern Settings window keeps our state up on every page, so an open window is turned to the mod's page too:
    // at once when its Extras state is up, else once the app has made it (it takes open_page then).
    if host.open_page.is_some() {
        let extras = w.settings.as_ref().is_some_and(|s| s.kind == super::settings::Kind::Extras);
        if w.modern {
            match w.config.as_mut() {
                Some(c) => super::modern::open_mod_settings(c),
                None => {
                    let mut c = super::configui::ConfigUi::open("configwi");
                    super::modern::open_mod_settings(&mut c);
                    w.config = Some(c);
                    w.menu = None;
                }
            }
        }
        if let Some(s) = w.settings.as_mut().filter(|_| extras) {
            if let Some(id) = host.open_page.take() {
                s.goto_mod(&id);
            }
        } else if w.settings.is_none() || w.modern {
            w.want_settings = Some(super::settings::Kind::Extras);
        }
    }
    w.lua = host;
    for (line, colour) in out {
        w.chat.push_colour(line, colour);
    }
    for op in ops {
        apply(w, gu, op);
    }
    actions::choose_pending(w);
    for line in bind_lines {
        run_line(w, &line, super::textcmd::Mode::Typed, From::Bind { mods: false });
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

/// Where a line for the mods came from. `mods`: the mod that sent it holds the mods permission (false where no one
/// mod can be named: a macro a mod started, a chat line it submitted).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum From {
    /// Typed by the player (the chat line, a macro).
    Player,
    /// A mod's key bind the player pressed: the mod's command runs as the player's input, but /lua grant, revoke,
    /// run, set and watch do not.
    Bind { mods: bool },
    /// A mod's own line (commands.run, actions): runs as the mod's own, and as the player's input only when the mod ran
    /// it on the player's input (a hotbar slot clicked runs another mod's command as the player would).
    Mod { mods: bool, input: bool },
}

impl From {
    fn controls_mods(self) -> bool {
        matches!(self, From::Player | From::Bind { mods: true } | From::Mod { mods: true, .. })
    }
}

/// The /lua subcommands that control other mods: a mod's lines run them only with the mods permission.
const MOD_CONTROL: [&str; 8] = ["load", "l", "unload", "u", "reload", "r", "hide", "show"];

/// A /lua line, as the host reads one (any number of leading slashes, any case).
pub(super) fn lua_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('/')
        && line.trim_start_matches('/').split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case("lua"))
}

/// A /lua line that loads, unloads, reloads, hides or shows mods (split as the host splits it).
pub(super) fn controls_mods(line: &str) -> bool {
    let line = line.trim();
    let mut words = line.trim_start_matches('/').split_whitespace();
    line.starts_with('/')
        && words.next().is_some_and(|n| n.eq_ignore_ascii_case("lua"))
        && words.next().is_some_and(|s| MOD_CONTROL.contains(&s.to_ascii_lowercase().as_str()))
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
pub(super) struct Recording(pub std::rc::Rc<std::cell::RefCell<Option<(String, std::path::PathBuf)>>>);

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
    // The bus works in the lobby too; a message for a named character waits for nobody here.
    let multibox = |v: &Vm| v.suspended.is_none() && v.st.borrow().granted.iter().any(|g| g == "multibox");
    if host.ipc.is_none() && host.vms.iter().any(multibox) {
        host.ipc = ipc::Bus::open();
    }
    let msgs: Vec<ipc::Msg> = host.ipc.as_ref().map(|b| b.rx.try_iter().collect()).unwrap_or_default();
    for (m, from, _, text) in msgs.into_iter().filter(|m| m.2.is_empty()) {
        if let Some(vm) = host.vms.iter().find(|v| v.manifest.id == m && multibox(v)) {
            lend_opt(vm, None, |vm| vm.emit("ipc_message", (text.clone(), from.clone())));
        }
    }
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        lend_opt(vm, None, |vm| {
            for (_, name, args) in emitted.iter().filter(|e| e.0 != vm.manifest.id) {
                if vm.has_handler(name) {
                    let vals: Vec<mlua::Value> =
                        args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
            }
            for (name, args) in &events {
                if vm.has_handler(name) {
                    let vals: Vec<mlua::Value> =
                        args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
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
                Op::Ipc(mod_id, to, text) => {
                    if let Some(b) = &host.ipc {
                        b.send(&mod_id, "", to.as_deref(), &text);
                    }
                }
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
                let (colour, indent) =
                    w.battle.as_ref().map_or((super::WHITE, false), |b| (b.colour_in(m, &w.game), b.indents(m)));
                w.chat.push_event(text, colour, indent, m);
            }
            (None, Some(c), text) => {
                let colour = w.battle.as_ref().map_or(super::WHITE, |b| b.colour_in(c, &w.game));
                w.chat.push_colour(text, colour);
            }
            (None, None, text) => w.chat.push(text),
        },
        Op::Run(line, mods, input) => run_line(w, &line, super::textcmd::Mode::Typed, From::Mod { mods, input }),
        Op::Act(a) => actions::apply(w, gu, a),
        Op::Key(k) => w.lua.synth.push(k),
        Op::Copy(text) => w.copied = Some(text),
        Op::Window(r) => w.lua.win_reqs.push(r),
        Op::Shot(name) => w.lua.shot = Some(name),
        Op::Paste => w.lua.paste_wanted = true,
        Op::Option(mod_id, name, v) => {
            // The minimap's zoom stays as the wheel's does, the maps' opacity as the player set it; the rest go back
            // when the mod unloads.
            if set_option(w, &name, &v) && !["minimap_span", "map_opacity"].contains(&name.as_str()) {
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
                    Some((k, id)) => super::timers::list_icon(w, k == "spell", id),
                    None => Icon::None,
                };
                let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(s.max(0.0));
                w.timers.mods.push(super::timers::ModTimer {
                    mod_id,
                    section,
                    group,
                    icon,
                    name,
                    end,
                    total: if total > 0.0 { total } else { s.max(0.0) },
                    fills,
                });
            }
        }
        Op::Interface(show) => w.ui_hidden = !show,
        // In the world there is no lobby to act on.
        Op::Lobby(_) => {}
        Op::Record(m, r) => w.lua.record_req = Some((m, r)),
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
    if !w.modern {
        w.lua.set_modern_scale(None);
    }
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

    /// A window's opacity (per-window opacity): the player's (config `window_opacity`), else its mod's, else 1.
    pub(in crate::client) fn window_opacity(&self, player: &std::collections::BTreeMap<String, f32>, key: &str) -> f32 {
        player
            .get(key)
            .copied()
            .or_else(|| self.info.borrow().opacity.get(key).map(|o| o.1))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0)
    }

    /// Every window's opacity that is not 1 this frame: the player's and the mods'.
    pub(in crate::client) fn window_opacities(
        &self,
        player: &std::collections::BTreeMap<String, f32>,
    ) -> std::collections::HashMap<String, f32> {
        let mut out: std::collections::HashMap<String, f32> =
            self.info.borrow().opacity.iter().map(|(k, v)| (k.clone(), v.1)).collect();
        out.extend(player.iter().map(|(k, v)| (k.clone(), *v)));
        out
    }

    /// A built-in panel's slot a mod fills (ui.native.slot).
    pub(in crate::client) fn slot(&self, name: &str) -> Option<serde_json::Value> {
        self.info.borrow().slots.get(name).map(|s| s.1.clone())
    }

    /// The timers' sections shown: per section, whether its panel's window is open, and with the client's own rows.
    pub(in crate::client) fn timers_panels(&self) -> [Option<bool>; 3] {
        TIMERS_PANELS.map(|p| {
            if self.all_hidden || self.hides(p) {
                return None;
            }
            // Read in place (edge_panel copies the window).
            self.vms.iter().rev().filter(|v| v.suspended.is_none()).find_map(|v| {
                let st = v.st.borrow();
                let win = st.windows.iter().find(|w| stands_in(w, p))?;
                Some(win.elems.iter().any(|e| matches!(e, window::Elem::Timers { builtin: true, .. })))
            })
        })
    }

    /// Whether an open edge window stands in for a built-in panel (edge_panel, without copying the window).
    pub(in crate::client) fn has_edge_panel(&self, panel: &str) -> bool {
        !self.all_hidden
            && self
                .vms
                .iter()
                .filter(|v| v.suspended.is_none())
                .any(|v| v.st.borrow().windows.iter().any(|w| stands_in(w, panel)))
    }

    /// The open edge window standing in for a built-in panel (the newest mod's).
    pub(in crate::client) fn edge_panel(&self, panel: &str) -> Option<window::Window> {
        if self.all_hidden {
            return None;
        }
        self.vms
            .iter()
            .rev()
            .filter(|v| v.suspended.is_none())
            .find_map(|v| v.st.borrow().windows.iter().find(|w| stands_in(w, panel)).cloned())
    }

    /// A number slot (target_distance, target_hpp).
    pub(in crate::client) fn slot_num(&self, name: &str) -> Option<f64> {
        self.info.borrow().slots.get(name).and_then(|s| s.1.as_f64())
    }

    /// The clock slot as the panels draw it.
    pub(in crate::client) fn clock_slot(&self) -> Option<super::clock::Parts> {
        let v = self.slot("clock")?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let n = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        Some(super::clock::Parts {
            earth: s("earth"),
            moon: (s("moon_name"), n("moon") as i32),
            next: (s("next_day"), n("next_minutes").max(0) as u64),
        })
    }

    /// Whether the mods' HUD draws: they run and `/lua hide` is off.
    pub(in crate::client) fn hud_shown(&self) -> bool {
        self.started && !self.all_hidden
    }

    /// The actors the mods' HUD portraits show (hud.portrait), first by layer, each once (as its first element asks).
    pub(in crate::client) fn portraits_wanted(&self) -> Vec<hud::Wanted> {
        if !self.hud_shown() {
            return Vec::new();
        }
        // Read in place (every element was copied for this every frame), in the draw order elements() gives.
        let all = hud::visible(self, |_, e| match e.kind {
            hud::Kind::Portrait { entity, w, h, look, .. } => Some(hud::Wanted { entity, size: [w, h], look }),
            _ => None,
        });
        let mut out: Vec<hud::Wanted> = Vec::new();
        for p in all {
            if !out.iter().any(|o| o.entity == p.entity) {
                out.push(p);
            }
        }
        out
    }

    /// The portraits drawn this frame: entity -> (game UI image, modern UI texture).
    pub(in crate::client) fn set_portraits(&self, drawn: std::collections::HashMap<u32, (String, egui::TextureId)>) {
        self.info.borrow_mut().portraits = drawn;
    }

    /// A portrait drawn this frame (hud.portrait's actor): its game UI image and modern UI texture.
    pub(in crate::client) fn portrait(&self, entity: u32) -> Option<(String, egui::TextureId)> {
        self.info.borrow().portraits.get(&entity).cloned()
    }

    /// The mods' visible HUD elements by layer, then load order: (mod, element).
    pub(in crate::client) fn hud_elements(&self) -> Vec<(String, hud::Elem)> {
        hud::elements(self)
    }

    /// What HUD places count from: the panels as last drawn and the window, in pixels.
    pub(in crate::client) fn hud_frame(&self) -> (std::collections::BTreeMap<String, ([f32; 4], bool)>, [f32; 2]) {
        let f = self.info.borrow();
        (f.native_rects.clone(), f.window)
    }

    /// HUD elements drawn this frame by the modern layer, after the game UI's: (mod, element, window pixel rect).
    pub(in crate::client) fn add_hud_rects(&self, rects: Vec<(String, String, [f32; 4])>) {
        self.info.borrow_mut().hud_rects.extend(rects);
    }

    /// The modern UI's pixels per point while it draws (hud.screen's ui_scale there), else none.
    pub(in crate::client) fn set_modern_scale(&self, ppp: Option<f32>) {
        self.info.borrow_mut().modern_ppp = ppp;
    }

    /// The map HUD maps show (the one the client shows here), as the modern layer draws it.
    pub(in crate::client) fn hud_map_image(
        &self,
        w: &World,
        regions: &[super::map::Region],
    ) -> Option<super::map::MapImage> {
        let mut m = self.map.borrow_mut();
        match m.as_mut() {
            Some(o) if o.zone == w.pos_zone => o.follow(w, regions),
            _ => *m = Some(super::map::Overlay::open_hud(w, regions)),
        }
        m.as_mut()?.modern_image()
    }

    /// The tooltip of the mod HUD element under the pointer: (title, text, seconds there).
    pub(in crate::client) fn hud_tip(&self) -> Option<(String, String, f32)> {
        hud::tip(self)
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

    /// Whether a mod leaves this row out of a game list (0 magic, 1 abilities, 2 mounts).
    pub(in crate::client) fn trimmed(&self, list: u8, id: u16) -> bool {
        self.info.borrow().trimmed.iter().any(|(k, ids)| k.1 == list && ids.contains(&id))
    }

    /// A mod's colour for an item's name in the game's lists (shops, the auction house).
    pub(in crate::client) fn item_tint(&self, id: u16) -> Option<u32> {
        self.info.borrow().item_tints.get(&id).map(|t| t.1)
    }

    /// The player looks mods set (render.look) and their version.
    /// The mods' render.look models' version: it moves when one changes.
    pub(in crate::client) fn models_ver(&self) -> u64 {
        self.info.borrow().models_ver
    }

    /// Entity `id`'s model as a mod set it (render.look).
    pub(in crate::client) fn model(&self, id: u32) -> Option<render::Model> {
        self.info.borrow().models.get(&id).map(|m| m.1)
    }

    /// Entity `id`'s size as a mod set it (render.entity scale; 1 for none).
    pub(in crate::client) fn look_scale(&self, id: u32) -> f32 {
        self.info.borrow().looks.get(&id).and_then(|l| l.1.scale).unwrap_or(1.0)
    }

    /// Entity `id`'s alpha and tint as a mod set them (render.entity), when one did.
    pub(in crate::client) fn look_shade(&self, id: u32) -> Option<(Option<f32>, Option<[f32; 3]>)> {
        self.info.borrow().looks.get(&id).map(|l| (l.1.alpha, l.1.tint))
    }

    /// A mod hides entity `id` (render.entity hidden).
    pub(in crate::client) fn look_hidden(&self, id: u32) -> bool {
        self.info.borrow().looks.get(&id).is_some_and(|l| l.1.hidden)
    }

    /// The looks mods set (render.entity) by entity, borrowed (a copy every frame was a map of strings): hold it only
    /// while no mod runs.
    pub(in crate::client) fn looks(&self) -> Looks<'_> {
        Looks(std::cell::Ref::map(self.info.borrow(), |f| &f.looks))
    }

    /// The modern panels of the hidden pieces, and the panels hidden by name.
    pub(in crate::client) fn hidden_panels(&self) -> Vec<&'static str> {
        let pieces = native::PIECES.iter().filter(|p| self.hides(p.0)).flat_map(|p| p.1.iter().copied());
        pieces.chain(native::PANELS.iter().filter(|p| p.hide && self.hides(p.name)).map(|p| p.name)).collect()
    }

    /// The modern panels mods placed, scaled or faded (ui.native.place).
    pub(in crate::client) fn placed(&self) -> std::collections::HashMap<String, Placed> {
        self.info.borrow().placed.iter().map(|(k, v)| (k.clone(), v.1)).collect()
    }

    /// The modern panels drawn this frame, in window pixels; those not drawn keep their last rect, as not shown, but
    /// those a mod hid take where they would stand now (`hidden`: what they sit on moves).
    pub(in crate::client) fn set_native_rects(&self, drawn: Vec<(String, [f32; 4])>, hidden: Vec<(String, [f32; 4])>) {
        let mut f = self.info.borrow_mut();
        for r in f.native_rects.values_mut() {
            r.1 = false;
        }
        for (id, r) in drawn {
            f.native_rects.insert(id, (r, true));
        }
        for (id, r) in hidden {
            f.native_rects.insert(id, (r, false));
        }
    }

    /// The classic log region (ui.native.rect's "chat" there), in layout units: as drawn, and at its full height.
    pub(in crate::client) fn set_classic_chat(&self, drawn: [f32; 4], shown: bool, room: [f32; 4]) {
        let mut f = self.info.borrow_mut();
        let (kx, ky) = f.k;
        if kx <= 0.0 || ky <= 0.0 {
            return;
        }
        let px = |r: [f32; 4]| [r[0] / kx, r[1] / ky, r[2] / kx, r[3] / ky];
        f.native_rects.insert("chat".into(), (px(drawn), shown));
        f.native_rooms.insert("chat".into(), px(room));
    }

    /// The whole room of the modern panels drawn folded or growing this frame; others keep their last.
    pub(in crate::client) fn set_native_rooms(&self, rooms: Vec<(String, [f32; 4])>) {
        self.info.borrow_mut().native_rooms.extend(rooms);
    }

    /// The mods' look for the modern UI, when it changed since version `seen`.
    pub(in crate::client) fn skin_since(&self, seen: u64) -> Option<skin::Skin> {
        let f = self.info.borrow();
        (f.skin.ver != seen).then(|| f.skin.clone())
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

    /// Whether the pointer is over a modern panel (egui's), which the HUD under it then leaves alone.
    pub(in crate::client) fn set_over_ui(&self, over: bool) {
        self.input.borrow_mut().over_ui = over;
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
        for (path, text) in
            [(root.join(".library").join("vana.lua"), stubs()), (root.join(".luarc.json"), luarc.to_string())]
        {
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
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let f: mlua::Function =
            vm.lua.load("return function() wait_until(function() while true do end end) end").eval().unwrap();
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

    /// hud.bubble: the tail leaves the side facing its point and stops at its length; none for a point inside; the
    /// outline and feathering go round it.
    #[test]
    fn hud_bubbles() {
        let r = [100.0, 100.0, 200.0, 80.0];
        let tail = |x: f32, y: f32| Some(hud::Tail { at: [x, y], w: 20.0, len: 25.0 });
        let pts = |t: &[[([f32; 2], [f32; 4]); 3]]| t.iter().flat_map(|t| t.iter().map(|v| v.0)).collect::<Vec<_>>();
        let fill = [10, 20, 30, 255];
        // A point far below: the tip is 25 under the bottom edge's base (its mitred feather a little more).
        let below = hud::bubble_tris(r, 12.0, tail(200.0, 400.0), fill, None);
        let lowest = pts(&below).iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(lowest > 205.0 && lowest < 209.0, "{lowest}");
        assert!(pts(&below).iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        // Off to the left: from the left side.
        let left = hud::bubble_tris(r, 12.0, tail(0.0, 140.0), fill, None);
        assert!(pts(&left).iter().map(|p| p[0]).fold(f32::MAX, f32::min) < 100.0 - 20.0);
        // Inside: no tail, the plain rounded rect.
        let none = hud::bubble_tris(r, 12.0, tail(150.0, 120.0), fill, None);
        let plain = hud::bubble_tris(r, 12.0, None, fill, None);
        assert_eq!(none.len(), plain.len());
        // An outline adds a ring of quads; its colour is on them.
        let edged = hud::bubble_tris(r, 12.0, tail(200.0, 400.0), fill, Some(([200, 160, 90, 255], 2.0)));
        assert!(edged.len() > below.len());
        assert!(edged.iter().any(|t| t[0].1[0] > 0.7));
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua
            .load(
                r#"
                hud.bubble("b", {x = 10, y = 10, w = 300, h = 90, round = 8, edge = {200, 160, 90}, tail = {x = 100, y = 300}, tail_w = 30})
                assert(not pcall(hud.bubble, "c", {tail = 5}))
            "#,
            )
            .exec()
            .unwrap();
        let tail_of = |vm: &Vm| {
            vm.st.borrow().hud.iter().find_map(|e| match e.kind {
                hud::Kind::Bubble { tail, w, .. } if e.id == "b" => Some((tail, w)),
                _ => None,
            })
        };
        assert_eq!(tail_of(&vm), Some((Some(hud::Tail { at: [100.0, 300.0], w: 30.0, len: 22.0 }), 300.0)));
        vm.lua.load(r#"hud.set("b", {tail = false})"#).exec().unwrap();
        assert_eq!(tail_of(&vm), Some((None, 300.0)));
    }

    /// hud.portrait: its actor is wanted once however many show it, a later entity moves it, none is an error.
    #[test]
    fn hud_portraits() {
        let mut host = Host::default();
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua
            .load(
                r#"
                hud.portrait("a", {entity = 7, size = 64, w = 50, bg = {0, 0, 0, 200}, shape = "square", frame = {200, 160, 90}})
                hud.portrait("b", {entity = 7, layer = 3})
                hud.portrait("c", {entity = 9})
                hud.set("c", {entity = 11, frame_w = 5})
                assert(not pcall(hud.portrait, "d", {size = 10}))
                assert(not pcall(hud.portrait, "e", {entity = 1, shape = "star"}))
            "#,
            )
            .exec()
            .unwrap();
        host.vms.push(vm);
        host.started = true;
        let want = host.portraits_wanted();
        assert_eq!(want.iter().map(|p| p.entity).collect::<Vec<_>>(), vec![7, 11]);
        assert_eq!(
            (want[0].size, want[0].look),
            (
                [50.0, 64.0],
                hud::Look { bg: Some([0, 0, 0, 200]), oval: false, frame: Some([200, 160, 90, 255]), frame_w: 3.0 }
            )
        );
        assert_eq!((want[1].size, want[1].look.oval, want[1].look.frame_w), ([96.0, 96.0], true, 5.0));
        host.all_hidden = true;
        assert!(host.portraits_wanted().is_empty());
    }

    /// HUD elements: a press released in place is a click; one that moves is a drag, ending over another element that
    /// gets a drop; the pointer coming and going; tips and spell icons from their specs.
    #[test]
    fn hud_clicks_drags_and_hover() {
        use winit::event::MouseButton::Left;
        let mut host = Host::default();
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua
            .load(
                r#"
                hud.rect("a", {x = 0, y = 0, w = 10, h = 10, tip = "A"})
                hud.rect("b", {x = 20, y = 0, w = 10, h = 10, tip = {title = "B", text = "b"}})
                hud.icon("c", {kind = "spell", id = 1})
                hud.on("a", function() end)
                hud.on("b", function() end)
            "#,
            )
            .exec()
            .unwrap();
        assert!(
            vm.st
                .borrow()
                .hud
                .iter()
                .any(|e| e.id == "c" && matches!(e.kind, hud::Kind::Icon { icon: window::Icon::Spell(1), .. }))
        );
        host.vms.push(vm);
        host.info.borrow_mut().hud_rects =
            vec![("t".into(), "a".into(), [0.0, 0.0, 10.0, 10.0]), ("t".into(), "b".into(), [20.0, 0.0, 10.0, 10.0])];
        let at = |host: &mut Host, x: f64| {
            host.input.borrow_mut().cursor = (x, 5.0);
            input::hud_drag(host, x, 5.0);
        };
        let take = |host: &mut Host| {
            std::mem::take(&mut host.hud_events).into_iter().map(|(_, id, e)| (id, e)).collect::<Vec<_>>()
        };
        let kinds = |ev: &[(String, serde_json::Value)]| {
            ev.iter().map(|(id, e)| format!("{id} {}", e["kind"].as_str().unwrap())).collect::<Vec<_>>()
        };
        at(&mut host, 5.0);
        assert!(input::hud_press(&mut host, Left));
        at(&mut host, 7.0);
        input::hud_release(&mut host, Left);
        assert_eq!(kinds(&take(&mut host)), ["a click"]);
        assert!(input::hud_press(&mut host, Left));
        for x in [12.0, 18.0, 25.0] {
            at(&mut host, x);
        }
        input::hud_release(&mut host, Left);
        let ev = take(&mut host);
        assert_eq!(kinds(&ev), ["a drag_start", "a drag", "a drag_end", "b drop"]);
        // Pressed at x 7, released at 25.
        assert_eq!(
            (ev[1].1["dx"].as_i64(), ev[2].1["over"].as_str(), ev[3].1["from"].as_str()),
            (Some(18), Some("b"), Some("a"))
        );
        // Nothing under the pointer is pressed there.
        at(&mut host, 15.0);
        assert!(!input::hud_press(&mut host, Left));
        at(&mut host, 25.0);
        hud::hover(&mut host);
        at(&mut host, 5.0);
        hud::hover(&mut host);
        assert_eq!(kinds(&take(&mut host)), ["b mouse_enter", "b mouse_leave", "a mouse_enter"]);
        assert_eq!(hud::tip(&host).map(|t| (t.0, t.1)), Some((String::new(), "A".into())));
        // Under a modern panel the element is neither pressed nor hovered.
        host.set_over_ui(true);
        assert!(!input::hud_press(&mut host, Left));
        hud::hover(&mut host);
        assert_eq!(kinds(&take(&mut host)), ["a mouse_leave"]);
        host.set_over_ui(false);
        // Nor while a menu is open.
        host.vms.last().unwrap().lua.load(r#"ui.menu({"x", "y"}, function() end)"#).exec().unwrap();
        assert!(!input::hud_press(&mut host, Left));
    }

    /// The bundled Battle Text's short lines: the common forms; an unknown one stays as printed; names in their colours.
    #[test]
    fn battle_text_short_lines() {
        let host = Host::default();
        let vm =
            Vm::new(bundled_manifest("battle-text").unwrap(), PathBuf::from("."), vm::Source::Dir(PathBuf::from(".")))
                .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let src = BUNDLED
            .iter()
            .find(|b| b.0 == "battle-text")
            .and_then(|b| b.2.iter().find(|f| f.0 == "init.lua"))
            .unwrap()
            .1;
        let module: mlua::Table = vm.lua.load(src).eval().unwrap();
        let shorten: mlua::Function = module.get("shorten").unwrap();
        let who = |name: &str, pc: bool, colour: u8| {
            let t = vm.lua.create_table().unwrap();
            t.set("name", name).unwrap();
            t.set("pc", pc).unwrap();
            t.set("colour", colour).unwrap();
            t
        };
        let (me, rabbit) = (who("Lnxcsfix", true, 0), who("Wild Rabbit", false, 0));
        let s = |l: &[&str], a: &mlua::Table, t: &mlua::Table| {
            shorten.call::<Option<String>>((l.to_vec(), a.clone(), t.clone())).unwrap()
        };
        assert_eq!(
            s(&["Lnxcsfix uses Fast Blade.", "The Wild Rabbit takes 123 points of damage."], &me, &rabbit).as_deref(),
            Some("Lnxcsfix [Fast Blade] → Wild Rabbit: 123")
        );
        assert_eq!(
            s(&["Lnxcsfix hits the Wild Rabbit for 18 points of damage."], &me, &rabbit).as_deref(),
            Some("Lnxcsfix → Wild Rabbit: 18")
        );
        assert_eq!(
            s(&["Lnxcsfix scores a critical hit!", "The Wild Rabbit takes 31 points of damage."], &me, &rabbit)
                .as_deref(),
            Some("Lnxcsfix → Wild Rabbit: 31!")
        );
        assert_eq!(
            s(&["Lnxcsfix misses the Wild Rabbit."], &me, &rabbit).as_deref(),
            Some("Lnxcsfix → Wild Rabbit: miss")
        );
        assert_eq!(
            s(&["Lnxcsfix casts Dia.", "The Wild Rabbit gains the effect of dia."], &me, &rabbit).as_deref(),
            Some("Lnxcsfix [Dia] → Wild Rabbit: +Dia")
        );
        assert_eq!(
            s(&["Lnxcsfix casts Fire.", "Magic Burst! The Wild Rabbit takes 80 points of damage."], &me, &rabbit)
                .as_deref(),
            Some("Lnxcsfix [Fire] → Wild Rabbit: 80 (Magic Burst)")
        );
        assert_eq!(
            s(&["The Wild Rabbit takes 48 points of damage."], &me, &rabbit).as_deref(),
            Some("Wild Rabbit: 48")
        );
        assert_eq!(s(&["Lnxcsfix defeats the Wild Rabbit."], &me, &rabbit), None);
        assert_eq!(
            s(&["Lnxcsfix uses Provoke on the Wild Rabbit."], &me, &rabbit).as_deref(),
            Some("Lnxcsfix [Provoke] → Wild Rabbit")
        );
        assert_eq!(
            s(&["Lnxcsfix casts Cure.", "Lnxcsfix recovers 45 HP."], &me, &me).as_deref(),
            Some("Lnxcsfix [Cure] → Lnxcsfix: +45 HP")
        );
        let red = who("Wild Rabbit", false, 8);
        let c = s(&["Lnxcsfix misses the Wild Rabbit."], &me, &red).unwrap();
        assert_eq!(crate::dat::font::strip_codes(&c), "Lnxcsfix → Wild Rabbit: miss");
        assert!(c.contains(crate::dat::font::colour_code(8)));
    }

    /// The bundled Training Regime's page labels, credit attribution, and its block on the real zone 100 dialogue.
    #[test]
    fn regime_labels_and_credits() {
        let host = Host::default();
        let vm = Vm::new(bundled_manifest("regime").unwrap(), PathBuf::from("."), vm::Source::Dir(PathBuf::from(".")))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.st.borrow_mut().granted.push("packets.read".into());
        let src =
            BUNDLED.iter().find(|b| b.0 == "regime").and_then(|b| b.2.iter().find(|f| f.0 == "init.lua")).unwrap().1;
        let module: mlua::Table = vm.lua.load(src).eval().unwrap();
        let lua = &vm.lua;
        let label = |b: &[u8]| {
            module.get::<mlua::Function>("label").unwrap().call::<String>(lua.create_string(b).unwrap()).unwrap()
        };
        assert_eq!(label(b"\x0a\x00 \x7f\x92\x00[Siege Bat/Siege Bats]."), "Siege Bat");
        assert_eq!(label(b"\x0a\x01 member\x7f\x92\x01[/s] of the worm family."), "Worm family");
        assert_eq!(label(b"\x0a\x02 undead-type creature\x7f\x92\x02[/s]."), "Undead-type");
        // Credits: by counts, then names; a family slot takes the monster no name fits; nothing fits a full slot.
        let attribute = |slots: &[(&str, u8, u8, bool)], k: u8, n: u8, name: Option<&str>| {
            let t = lua.create_table().unwrap();
            for (i, (l, needed, killed, known)) in slots.iter().enumerate() {
                let s = lua.create_table().unwrap();
                (
                    s.set("label", *l).unwrap(),
                    s.set("needed", *needed).unwrap(),
                    s.set("killed", *killed).unwrap(),
                    s.set("known", *known).unwrap(),
                );
                t.set(i + 1, s).unwrap();
            }
            module.get::<mlua::Function>("attribute").unwrap().call::<Option<usize>>((t, k, n, name)).unwrap()
        };
        let s = [("Siege Bat", 4, 2, true), ("Borer Beetle", 2, 0, true)];
        assert_eq!((attribute(&s, 3, 4, None), attribute(&s, 1, 2, None)), (Some(1), Some(2)));
        assert_eq!(
            attribute(&[("Goblin family", 4, 1, true), ("Gigas family", 4, 1, true)], 2, 4, Some("Goblin Mugger")),
            Some(1)
        );
        let s = [("Crab family", 6, 2, true), ("Hill Lizard", 6, 2, true)];
        assert_eq!(
            (attribute(&s, 3, 6, Some("Snipper")), attribute(&s, 3, 6, Some("Hill Lizard"))),
            (Some(1), Some(2))
        );
        // Counts unknown (a book's page, no kills yet): the name decides.
        assert_eq!(
            attribute(&[("Siege Bat", 0, 0, false), ("Borer Beetle", 0, 0, false)], 3, 4, Some("Borer Beetle")),
            Some(2)
        );
        let matches = |a: &str, b: &str| module.get::<mlua::Function>("matches").unwrap().call::<bool>((a, b)).unwrap();
        assert!(
            matches("Bat family", "Seeker Bats")
                && matches("Siege Bat", "siege bat")
                && !matches("Siege Bat", "Siege Bats")
        );
        // The real block of zone 100, through stand-ins for res.zone_text / zone_text_find.
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(d) = crate::dat::dialog::Dialog::load(&index, 100) else { return };
        let d = std::sync::Arc::new(d);
        let res: mlua::Table = lua.globals().get("res").unwrap();
        let d1 = d.clone();
        res.set(
            "zone_text",
            lua.create_function(move |lua, (_z, id): (u16, u32)| d1.raw(id).map(|b| lua.create_string(&b)).transpose())
                .unwrap(),
        )
        .unwrap();
        let d2 = d.clone();
        res.set(
            "zone_text_find",
            lua.create_function(move |_, (_z, p): (u16, mlua::LuaString)| {
                Ok((0..d2.len() as u32).find(|&i| d2.raw(i).is_some_and(|r| r.starts_with(&p.as_bytes()))))
            })
            .unwrap(),
        )
        .unwrap();
        let r: u32 = res
            .get::<mlua::Function>("zone_text_find")
            .unwrap()
            .call((100, "New training regime registered!"))
            .unwrap();
        let page = |id: u32| {
            module.get::<mlua::Function>("page").unwrap().call::<(Vec<String>, String)>((100, r, id)).unwrap()
        };
        let (l, area) = page(2);
        assert_eq!(l, ["Forest Hare"]);
        assert!(area.contains("Ronfaure"), "{area}");
        assert_eq!(
            page(704),
            (vec!["Siege Bat".to_string(), "Borer Beetle".to_string()], "Garlaige Citadel".to_string())
        );
    }

    /// Gearshift with a fake game: a GearSwap file and a LuAshitacast profile each wear their precast gear before a
    /// Cure's action packet and their midcast gear after it, in one batch; the client's own packet is dropped.
    #[test]
    fn gearshift_swaps_around_a_cast() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "gearshift").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("gearshift").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let script = r#"
            local sent, said = {}, {}
            local ITEMS = {
              [1001] = { name = "Fast Hat", log_name = "Fast Hat", slots = 1 << 4, jobs = 1 << 3, level = 1 },
              [1002] = { name = "Cast Coat", log_name = "Cast Coat", slots = 1 << 5, jobs = 1 << 3, level = 1 },
              [1003] = { name = "Heal Gloves", log_name = "Healing Gloves", slots = 1 << 6, jobs = 1 << 3, level = 1 },
              [1004] = { name = "Idle Hat", log_name = "Idle Hat", slots = 1 << 4, jobs = 1 << 3, level = 1 },
            }
            local FILES = {}
            game = {
              player = function() return { name = "Tester", id = 100, index = 1, main_job = 3, main_level = 75, sub_job = 4,
                sub_level = 37, hp = 900, mp = 500, tp = 0, hpp = 100, mpp = 100, hp_max = 900, mp_max = 500, status = 0, zone = 230 } end,
              equipment = function() return {} end,
              items = function(bag) if bag ~= 0 then return {} end
                return { { bag = 0, slot = 1, id = 1001, count = 1, locked = 0 }, { bag = 0, slot = 2, id = 1002, count = 1, locked = 0 },
                  { bag = 0, slot = 3, id = 1003, count = 1, locked = 0 }, { bag = 0, slot = 4, id = 1004, count = 1, locked = 0 } } end,
              buffs = function() return {} end,
              target = function() return nil end,
              entity = function(id) return { id = id, index = 5, name = "Tester", kind = "player", me = true, hpp = 100, distance = 0, status = 0 } end,
              entity_by_tag = function() return nil end,
              time = function() return { weekday = 0, moon_phase = 0, moon = 50, hour = 12, minute = 0, day = 1 } end,
              weather = function() return 0 end,
              info = function() return { mog_house = false, zone = 230 } end,
              party = function() return {} end,
              find_items = function() return {} end,
              entities = function() return {} end,
            }
            res = setmetatable({
              item = function(id) return ITEMS[id] end,
              spell = function(id) if id == 1 then return { name = "Cure", kind = 1, skill = 33, element = 6, mp = 8, cast_time = 2, recast = 5, targets = 63 } end end,
              ability = function() return nil end,
              job = function(id) return ({ "WAR", "MNK", "WHM", "BLM" })[id] end,
              status = function() return nil end,
              zone = function() return "Somewhere" end,
              day = function() return "Firesday" end, moon = function() return "Full Moon" end, weather = function() return "Clear" end,
            }, { __index = function() return function() return nil end end })
            packets.inject_out = function(d) sent[#sent + 1] = d end
            fs = { list = function(dir) local t = {} for k in pairs(FILES) do local n = k:match("^" .. dir:gsub("%p", "%%%0") .. "([^/]+)$") if n then t[#t + 1] = n end end return t end,
              read = function(p) return FILES[p] end, exists = function(p) return FILES[p] ~= nil end }
            chat = { add = function(t) said[#said + 1] = t end }
            local function cast_packet()
              return packets.build(0x01A, string.pack("<I4I2I2I4fff", 100, 1, 3, 1, 0, 0, 0))
            end
            local function ids()
              local t = {}
              for _, d in ipairs(sent) do t[#t + 1] = string.format("%03X", string.unpack("<I2", d) & 0x1ff) end
              return table.concat(t, " ")
            end
            local core = require("src.core")

            FILES["gearswap/Tester_WHM.lua"] = [[
              function get_sets()
                sets.precast = { FC = { head = "Fast Hat", body = "Cast Coat" } }
                sets.midcast = { Cure = { hands = "Healing Gloves" } }
              end
              function precast(spell) if spell.action_type == "Magic" then equip(sets.precast.FC) end end
              function midcast(spell) if spell.english == "Cure" then equip(sets.midcast.Cure) end end
            ]]
            local gs = require("src.gs")
            assert(gs.find() == "gearswap/Tester_WHM.lua", tostring(gs.find()))
            assert(gs.load(gs.find()), table.concat(said, " | "))
            gs.typed('/ma "Cure" <me>')
            local act = core.parse_action({ id = 0x01A, data = cast_packet() })
            assert(gs.action(act) == false)
            local gs_order = ids()
            assert(sent[#sent]:sub(5, 8) == string.pack("BBBB", 3, 6, 0, 0), "the gloves after the action")
            gs.unload()

            sent = {}
            core.sent = {}
            FILES["ashitacast/Tester_100/WHM.lua"] = [[
              local profile = {}
              profile.Sets = { Precast = { Head = "Fast Hat", Body = "Cast Coat" }, Cure = { Hands = "Heal Gloves" } }
              profile.HandlePrecast = function() gFunc.EquipSet("precast") end
              profile.HandleMidcast = function() if gData.GetAction().Name == "Cure" then gFunc.EquipSet(profile.Sets.Cure) end end
              return profile
            ]]
            local lac = require("src.lac")
            assert(lac.find() == "ashitacast/Tester_100/WHM.lua", tostring(lac.find()))
            assert(lac.load(lac.find()), table.concat(said, " | "))
            act = core.parse_action({ id = 0x01A, data = cast_packet() })
            assert(lac.action(act) == false)
            local lac_order = ids()
            lac.unload()

            sent = {}
            core.sent = {}
            FILES["ashitacast/Tester_WHM.xml"] = [[<?xml version="1.0"?>
              <ashitacast>
                <sets>
                  <set name="Base"><head>Idle Hat</head></set>
                  <set name="FC" baseset="Base"><head>Fast Hat</head><body>Cast Coat</body></set>
                </sets>
                <precast><equip set="FC"/></precast>
                <midcast>
                  <if ad_name="Cure*" p_mainjob="WHM"><equip><hands>Heal Gloves</hands></equip></if>
                  <else><echo>not a cure</echo></else>
                </midcast>
              </ashitacast>]]
            local ac = require("src.acxml")
            assert(ac.find() == "ashitacast/Tester_WHM.xml", tostring(ac.find()))
            assert(ac.load(ac.find()), table.concat(said, " | "))
            act = core.parse_action({ id = 0x01A, data = cast_packet() })
            assert(ac.action(act) == false)
            return gs_order, lac_order, ids()
        "#;
        let (gs, lac, xml): (String, String, String) = vm.lua.load(script).eval().unwrap_or_else(|e| panic!("{e}"));
        // GearSwap: two pieces go as single equips, then the action, then the midcast piece.
        assert_eq!(gs, "050 050 01A 050");
        // LuAshitacast: its precast is one equipset, its midcast single equips.
        assert_eq!(lac, "051 01A 050");
        // Ashitacast XML: precast as single equips, the action, midcast as one equipset.
        assert_eq!(xml, "050 050 01A 051");
    }

    /// Gearshift's XML front ends. Ashitacast: substitution, basic and advanced rules with the original's quirks,
    /// setvar arithmetic, augment paths; a profile's commands, default and sets; a v3 file through the import rules.
    /// Spellcast: wildcards, regex, variables, Advanced; a file's command lines, phases and timers, a trigger, an
    /// include and the idle autoset. The regex engine alone.
    #[test]
    fn gearshift_ashitacast_rules() {
        let files = BUNDLED.iter().find(|b| b.0 == "gearshift").unwrap().2;
        for (script, want) in [
            (ACXML_EVAL, "acxml eval ok"),
            (ACXML_FLOW, "acxml flow ok"),
            (SPELLCAST, "spellcast ok"),
            (REGEX, "regex ok"),
        ] {
            let host = Host::default();
            let vm = Vm::new(bundled_manifest("gearshift").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
                .unwrap();
            vm.lua.set_app_data(host.info.clone());
            api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
            let got: String = vm.lua.load(script).eval().unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(got, want);
        }
    }

    /// Gearshift's built-in GearSwap libraries: M() modes, and texts boxes drawn as HUD texts (fields filled, colour
    /// runs placed one after another by their drawn widths).
    #[test]
    fn gearshift_gearswap_libs() {
        let files = BUNDLED.iter().find(|b| b.0 == "gearshift").unwrap().2;
        let host = Host::default();
        let vm =
            Vm::new(bundled_manifest("gearshift").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let script = r#"
            local M = require("src.modes")
            local idle = M('refresh', 'dt', 'mdt')
            assert(idle.value == 'refresh' and idle.current == 'refresh' and idle[1] == 'refresh' and idle.index == 1)
            assert(idle:cycle() == 'dt' and idle:cycleback() == 'refresh' and idle:cycleback() == 'mdt')
            idle:set('DT')
            assert(idle.value == 'dt' and idle:contains('mdt') and not idle:contains('x') and tostring(idle) == 'dt')
            idle:reset()
            assert(idle.value == 'refresh')
            local burst = M(false, 'Magic Burst')
            assert(burst.value == false and burst.description == 'Magic Burst')
            burst:toggle()
            assert(burst.value == true)
            burst:set('off')
            assert(burst.value == false)
            local s = M{ ['description'] = 'Weapon', ['string'] = 'Naegling' }
            assert(s.value == 'Naegling' and s.description == 'Weapon')
            local d = M{ ['description'] = 'Nuke', 'normal', 'acc' }
            assert(d.description == 'Nuke' and d:length() == 2)

            local drawn = {}
            hud = { text = function(id, spec) drawn[id] = spec end, rect = function(id, spec) drawn[id] = spec end,
              remove = function(id) drawn[id] = nil end, screen = function() return { width = 1000, height = 800, ui_scale = 1 } end,
              bounds = function(id) local d = drawn[id] return d and d.text and { x = d.x, y = d.y, w = #d.text * 10, h = 18 * d.size } end }
            local texts = require("src.texts")
            local t = texts.new('Idle: \\cs(0,0,255)${idle|none}\\cr!', { pos = { x = 100, y = 50 }, padding = 5 })
            t:show()
            texts.frame()
            texts.frame()
            local a, b, c = drawn['gstext1_1_1'], drawn['gstext1_1_2'], drawn['gstext1_1_3']
            assert(a.text == 'Idle: ' and b.text == 'none' and c.text == '!', tostring(b and b.text))
            assert(b.colour[3] == 255 and a.x == 105 and b.x == 105 + 60 and c.x == 105 + 100, b.x .. ' ' .. c.x)
            t.idle = 'dt'
            texts.frame()
            texts.frame()
            assert(drawn['gstext1_1_2'].text == 'dt' and drawn['gstext1_1_3'].x == 105 + 80)
            t:hide()
            assert(next(drawn) == nil)
            return "ok"
        "#;
        let got: String = vm.lua.load(script).eval().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, "ok");
    }

    const SPELLCAST: &str = r#"local said, ran, sent, timers = {}, {}, {}, {}
local FILES = {}
local BUFFS = {}
local ITEMS = {
  [11] = { name = "Idle Hat", log_name = "idle hat", slots = 1 << 4, jobs = 1 << 3, level = 1 },
  [12] = { name = "Fast Hat", log_name = "fast hat", slots = 1 << 4, jobs = 1 << 3, level = 1 },
  [13] = { name = "Cast Coat", log_name = "cast coat", slots = 1 << 5, jobs = 1 << 3, level = 1 },
  [14] = { name = "Heal Gloves", log_name = "healing gloves", slots = 1 << 6, jobs = 1 << 3, level = 1 },
}
local PLAYER = { name = "Tester", id = 100, index = 1, main_job = 3, main_level = 75, sub_job = 4, sub_level = 37,
  hp = 900, hp_max = 1000, mp = 400, mp_max = 500, tp = 0, hpp = 90, mpp = 80, status = 0, zone = 230 }
local SPELLS = { [1] = { name = "Cure", kind = 1, skill = 33, element = 6, mp = 8, cast_time = 2, recast = 5, targets = 63 },
  [3] = { name = "Cure III", kind = 1, skill = 33, element = 6, mp = 46, cast_time = 2.5, recast = 6, targets = 63 },
  [144] = { name = "Fire", kind = 2, skill = 36, element = 0, mp = 7, cast_time = 1, recast = 2, targets = 32 },
  [338] = { name = "Utsusemi: Ichi", kind = 4, skill = 39, element = 2, mp = 0, cast_time = 4, recast = 30, targets = 1 } }
game = {
  player = function() return PLAYER end,
  buffs = function() return BUFFS end,
  party = function() return {} end,
  target = function() return { id = 200, index = 50, name = "Goblin", kind = "mob", hpp = 100, distance = 5, status = 1 } end,
  entity = function(id) if id == 100 then return { id = 100, index = 1, name = "Tester", kind = "player", me = true, hpp = 90 } end end,
  entities = function() return {} end,
  entity_by_tag = function() return nil end,
  time = function() return { earth = 0, weekday = 5, hour = 7, minute = 5, moon = 50, moon_phase = 0 } end,
  weather = function() return 4 end,
  equipment = function() return {} end,
  items = function(bag)
    if bag ~= 0 then return {} end
    return { { bag = 0, slot = 1, id = 11 }, { bag = 0, slot = 2, id = 12 }, { bag = 0, slot = 3, id = 13 }, { bag = 0, slot = 4, id = 14 } }
  end,
}
res = {
  job = function(id) return ({ "WAR", "MNK", "WHM", "BLM" })[id] end,
  status = function(id) return ({ [33] = "Haste", [5] = "Blindness" })[id] end,
  spell = function(id) return SPELLS[id] end,
  ability = function(id) if id == 0x200 + 10 then return { name = "Divine Seal", recast_id = 26, targets = 1 } end end,
  item = function(id) return ITEMS[id] end,
  zone = function() return "Southern San d'Oria" end,
  moon = function() return "Full Moon" end,
}
chat = { add = function(t) said[#said + 1] = t end }
clock = function() return 1000 end
timer = { after = function(secs, f) timers[#timers + 1] = { secs, f } end }
commands = { run = function(l) ran[#ran + 1] = l end }
packets = { inject_out = function(d) sent[#sent + 1] = d end, build = function(id, b) return string.pack("<I2I2", id, 0) .. b end }
fs = {
  list = function(dir)
    local t = {}
    for k in pairs(FILES) do
      local n = k:match("^" .. dir:gsub("%p", "%%%0") .. "([^/]+)$")
      if n then t[#t + 1] = n end
    end
    return t
  end,
  read = function(p) return FILES[p] end,
}
local function equips()
  local t = {}
  for _, d in ipairs(sent) do
    local id = string.unpack("<I2", d)
    if id == 0x050 then
      local index, slot = string.unpack("BB", d, 5)
      t[#t + 1] = slot .. ":" .. ITEMS[({ 11, 12, 13, 14 })[index]].name
    elseif id == 0x051 then
      local n = string.unpack("B", d, 5)
      for k = 0, n - 1 do
        local index, slot = string.unpack("BB", d, 9 + k * 4)
        t[#t + 1] = slot .. ":" .. ITEMS[({ 11, 12, 13, 14 })[index]].name
      end
    end
  end
  sent = {}
  table.sort(t)
  return table.concat(t, " ")
end
local function fire_timers()
  local list = timers
  timers = {}
  table.sort(list, function(a, b) return a[1] < b[1] end)
  local order = {}
  for _, x in ipairs(list) do
    x[2]()
    order[#order + 1] = string.format("%.1f", x[1])
  end
  return table.concat(order, " ")
end

local core = require("src.core")
local sc = require("src.spellcast")

-- Matching, variables, Advanced.
assert(sc._match("m*d*", "mad") and not sc._match("m*d*", "master") and sc._match("m???e?", "monkey"))
assert(sc._match("Cure*|Fire", "fire") and sc._match("(regex)^cure (ii|iii)$(/regex)", "Cure III"))
assert(not sc._match("(regex-case)^cure$(/regex)", "Cure"))
assert(sc._key("Cure III") == "cure3" and sc._key("Utsusemi: Ichi") == "utsusemiichi")
sc._uvars({ instra1 = { name = "instra1", value = "A" }, fireobi = { name = "FireObi", value = "Karin Obi" },
  use = { name = "use", value = "$" }, b = { name = "B", value = "bee" } })
assert(sc._expand("$instra11") == "A1", sc._expand("$instra11"))
assert(sc._expand("$%SpellElementObi", { spell = { element = "Fire" } }) == "Karin Obi")
assert(sc._expand("%MoonPCT %Moon %Time %Weather %MainJob/%SubJob") == "50 Full Moon 07.05 Fire WHM/BLM", sc._expand("%MoonPCT %Moon %Time %Weather %MainJob/%SubJob"))
assert(sc._expand("\\$B and $B") == "$B and bee")
local A = sc._advanced
assert(A('"%MainJob"="WHM" AND "%SubJob"="BLM"') and not A('"%MainJob"="WHM" AND %MPP < 50'))
assert(A("(float)%MP/%MPMax*100 > 79") and A("%MP/%MPMax*100 = 0") and A("!(%HPP < 50)"))
assert(A('(bool)strmatch("Southern*","%Area")') and A('(bool)isArea("*d\'Oria")') and A('(bool)regex("%HPP","8[5-9]|9.")'))
assert(not A('(bool)buffactive("Haste")') and A("(regex)%Weather=^fire$(/regex)") and not A("broken ((("))

FILES["spellcast/WHM.xml"] = [==[<?xml version="1.0" ?>
<spellcast xmlns:xi="http://www.w3.org/2001/XInclude">
  <config ShowGearSwaps="false" RequiredVersion="2.30"/>
  <variables clear="true">
    <var name="IdleSet">Standard</var>
  </variables>
  <sets>
    <group name="WHM|WhiteMage" default="yes">
      <set name="Standard"><head>Idle Hat</head></set>
      <set name="FastCast" baseset="Standard"><head>Fast Hat</head><body>Cast Coat</body></set>
      <set name="Cure"><hands>Healing Gloves</hands></set>
    </group>
  </sets>
  <rules>
    <xi:include href="inc.xml" xpointer="//include[@name='Auto']/*"/>
    <if Type="WhiteMagic|BlackMagic">
      <equip when="precast" set="FastCast"/>
    </if>
    <if Spell="Cure*">
      <action type="equip" when="midcast" set="Cure"/>
      <midcastdelay delay="0.4"/>
    </if>
    <elseif Spell="Fire*">
      <addtochat>fire chain</addtochat>
    </elseif>
    <if Spell="Trigger1">
      <var cmd="set IdleSet FastCast"/>
      <addtochat when="precast">idle $IdleSet</addtochat>
    </if>
    <if NotSpellTargetType="SELF" Spell="Fire">
      <cmd when="aftercast">input /p burned %SpellTargetName</cmd>
    </if>
    <equip when="aftercast" set="$IdleSet"/>
  </rules>
</spellcast>]==]
FILES["spellcast/inc.xml"] = [==[<includes><include name="Auto">
  <if spell="autoset"><equip when="idle" set="$IdleSet"/><return/></if>
</include></includes>]==]

assert(sc.find() == "spellcast/WHM.xml", tostring(sc.find()))
assert(sc.load(sc.find()), table.concat(said, " | "))

-- Cure III typed loosely: the line goes out canonical, the precast set (FastCast over its base) at once.
local out = sc.typed("/ma cure3 me")
assert(out == '/ma "Cure III" <me>', tostring(out))
assert(equips() == "4:Fast Hat 5:Cast Coat", "precast")
local when = fire_timers()
assert(when == "0.4 4.0", when)
local got = equips()
assert(got == "4:Idle Hat 6:Heal Gloves", got)

-- Fire at the target: aftercast command.
core.sent = {}
assert(sc.typed('/ma "Fire" <t>') == '/ma "Fire" <t>')
equips()
assert(said[#said] == "fire chain", "the elseif runs when the if before it failed")
fire_timers()
equips()
assert(ran[#ran] == "/p burned Goblin", tostring(ran[#ran]))

-- A trigger: never sent; its var and chat run.
assert(sc.typed("/ma Trigger1") == false)
assert(said[#said] == "idle FastCast", said[#said])

-- The idle autoset uses the variable now.
core.sent = {}
sc.status_change(0)
local idle = equips()
assert(idle == "4:Fast Hat 5:Cast Coat", idle .. " | " .. table.concat(said, " | "))

-- /sc takes over; other lines get their variables.
assert(sc.typed("/sc var set IdleSet Standard") == false and sc._uvars().idleset.value == "Standard")
assert(sc.typed("/p job %MainJob, %HPP%") == "/p job WHM, 90%")
assert(sc.typed("/echo plain") == nil)
return "spellcast ok"
"#;

    const REGEX: &str = r#"local R = require("src.regex").test
local cases = {
  { "^.* x2$", "Fire x2", true }, { "^.* x2$", "Fire", false }, { "8[5-9]|9.", "87", true }, { "8[5-9]|9.", "84", false },
  { "8[5-9]|9.", "93", true }, { "^(Cure|Curaga)( I+V?)?$", "cure iii", true }, { "^(Cure|Curaga)( I+V?)?$", "Cura", false },
  { "a{2,3}b", "caaab", true }, { "^a{2}b", "aaab", false }, { "\\d+\\.\\d", "v2.45", true }, { "\\bfire\\b", "firestorm", false },
  { "\\bfire\\b", "the fire burns", true }, { "[^aeiou]x", "ax", false }, { "[^aeiou]x", "bx", true }, { "a.*?b", "aXbYb", true },
  { "Dynamis.*", "Dynamis - Bastok", true }, { "^$", "", true }, { "x?", "", true }, { "(ab)+c", "ababc", true },
}
for _, c in ipairs(cases) do
  local got = R(c[1], c[2])
  assert(got == c[3], c[1] .. " ~ " .. c[2] .. " gave " .. tostring(got))
end
assert(R("ABC", "abc", true) == false and R("ABC", "abc") == true)
assert(not pcall(R, "(ab", "ab"))
return "regex ok"
"#;

    const ACXML_EVAL: &str = r#"local said, ran, sent = {}, {}, {}
local BUFFS = { { id = 5 }, { id = 33 } }
local PLAYER = { name = "Tester", id = 100, index = 1, main_job = 1, main_level = 75, sub_job = 13, sub_level = 37,
  hp = 300, hp_max = 1000, mp = 50, mp_max = 100, tp = 1000, hpp = 75, mpp = 50, status = 1, zone = 230, pos = { x = 0, y = 0, z = 0 } }
game = {
  player = function() return PLAYER end,
  buffs = function() return BUFFS end,
  party = function() return {} end,
  target = function() return nil end,
  entity = function() return nil end,
  entities = function() return {} end,
  entity_by_tag = function() return nil end,
  stats = function() return { attack = 300, defense = 250, resist = { 1, 2, 3, 4, 5, 6, 7, 8 } } end,
  time = function() return { earth = 0, weekday = 0, hour = 7, minute = 5, moon = 50, moon_phase = 0 } end,
  weather = function() return 4 end,
  equipment = function() return {} end,
  items = function() return {} end,
}
res = {
  job = function(id) return ({ [0] = nil, "WAR", "MNK", "WHM", [13] = "NIN" })[id] end,
  status = function(id) return ({ [5] = "Blindness", [33] = "Haste", [580] = "Haste" })[id] end,
  spell = function(id) if id == 1 then return { name = "Cure", mp = 8, cast_time = 2, element = 6, skill = 33, kind = 1 } end end,
  ability = function() return nil end,
  item = function() return nil end,
  zone = function() return "Somewhere" end,
  mob_skill = function() return nil end,
}
chat = { add = function(t) said[#said + 1] = t end }
clock = function() return 1000 end
timer = { after = function(_, f) f() end }
commands = { run = function(l) ran[#ran + 1] = l end }
packets = { inject_out = function(d) sent[#sent + 1] = d end, build = function(id, b) return string.pack("<I2", id) .. b end }
fs = { list = function() return {} end, read = function() return nil end }

local ac = require("src.acxml")
local R, B, A, S = ac._resolve, ac._basic, ac._advanced, ac._setvar

-- Substitution: longest names, text after a name kept, unknown names lose their $, right to left.
assert(R("$p_hpp") == "75" and R("$p_hp") == "300" and R("$p_hpmax") == "1000", R("$p_hpp"))
assert(R("$p_hppx") == "75x", R("$p_hppx"))
assert(R("$U_x") == "U_x" and R("$nope") == "nope")
ac._uvars({ x = "p" })
assert(R("$p_hp$u_x") == "75", R("$p_hp$u_x"))
assert(R("$p_job") == "WAR/NIN" and R("$e_time") == "07.05" and R("$e_weather") == "fire")
-- Buffs by id and by name (the longest name; several ids under one name summed).
assert(R("$b_33") == "1" and R("$b_haste") == "1" and R("$b_5 ") == "1 ")
-- The single-digit quirk: $b_5=1 reads "5=" as the id.
assert(R("$b_5=1") == "11")
assert(B("b_5", "1") == false)
-- Arguments only inside a command match.
assert(R("$arg1") == "arg1")
ac._args({ [0] = "/acc", "x", "y", text = "x y" })
assert(R("$args|$arg0|$arg1|$arg2|$arg9") == "x y|/acc|x|y|arg9", R("$args|$arg0|$arg1|$arg2|$arg9"))
assert(R("$arg1st") == "x")
ac._args(nil)

-- Basic rules: | before &, operators anywhere, wildcards, the stray character of two-character operators.
assert(B("p_hpp", "<50") == false and B("p_hpp", ">=75") == true and B("p_hpp", "75") == true)
assert(B("p_mainjob", "WAR|NIN") == true and B("p_mainjob", "NIN|THF") == false)
assert(B("p_mainjob", "W*") == true and B("p_mainjob", "w*R") == true and B("p_mainjob", "WA**") == true and B("p_mainjob", "WAR**") == false)
assert(B("p_mainjob", "!WAR") == false and B("p_mainjob", "!=WAR") == true)
assert(B("p_mainjob", "THF|") == true, "an empty piece compares as numbers")
assert(B("p_hpp", ">50&<80") == true and B("p_hpp", ">50&<70") == false)

-- Advanced rules: precedence, left association, signed values (no hang), no crash on division by zero.
assert(A("$p_hpp > 50 & $p_mp >= 10") == true)
assert(A("($p_hp - 500) > 0") == false and A("$p_hp-500<0") == true)
assert(A("2^3^2=64.000000") == true and A("8-2-1=5") == true and A("7/2=3") == true and A("-7/2=-3") == true)
assert(A("5/0=0") == true and A("$p_job=WAR/NIN") == true)
-- "*" multiplies here: WAR=W* is WAR=0, true for any name (as in the original).
assert(A("$p_mainjob=war") == true and A("$p_mainjob=W*") == true and A("$p_mainjob=NIN") == false)
assert(A("($p_mainjob=WAR)|($p_mainjob=NIN)") == true and A("!true") == false)

-- setvar arithmetic: * / + - only, names with - become numbers.
ac._uvars({ n = "3" })
assert(S("$u_n + 1") == "4" and S("$u_n - 5") == "-2" and S("Mid-Acc") == "0" and S("Plain") == "Plain")

-- Augment path / rank / trial from extdata.
local ext = string.char(2, 0x20, 0x07, 0) .. string.rep("\0", 20)
local p, r = ac._aug_meta(ext)
assert(p == 4 and r == 1, p .. " " .. r)

return "acxml eval ok"
"#;

    const ACXML_FLOW: &str = r#"local said, ran, sent = {}, {}, {}
local FILES = {}
local BUFFS = {}
local ITEMS = {
  [11] = { name = "Idle Hat", slots = 1 << 4, jobs = 1 << 1, level = 1 },
  [12] = { name = "Acc Hat", slots = 1 << 4, jobs = 1 << 1, level = 1 },
  [13] = { name = "Ear A", slots = (1 << 11) | (1 << 12), jobs = 1 << 1, level = 1 },
  [14] = { name = "Body B", slots = 1 << 5, jobs = 1 << 1, level = 1 },
}
local PLAYER = { name = "Tester", id = 100, index = 1, main_job = 1, main_level = 75, sub_job = 13, sub_level = 37,
  hp = 900, hp_max = 1000, mp = 50, mp_max = 100, tp = 0, hpp = 90, mpp = 50, status = 0, zone = 230, pos = { x = 0, y = 0, z = 0 } }
game = {
  player = function() return PLAYER end,
  buffs = function() return BUFFS end,
  party = function() return {} end,
  target = function() return nil end,
  entity = function() return nil end,
  entities = function() return {} end,
  entity_by_tag = function() return nil end,
  time = function() return { earth = 0, weekday = 0, hour = 7, minute = 5 } end,
  weather = function() return 0 end,
  equipment = function() return {} end,
  items = function(bag)
    if bag ~= 0 then return {} end
    return { { bag = 0, slot = 1, id = 11 }, { bag = 0, slot = 2, id = 12 }, { bag = 0, slot = 3, id = 13 }, { bag = 0, slot = 4, id = 14 } }
  end,
}
res = {
  job = function(id) return ({ "WAR", "MNK", "WHM", [13] = "NIN" })[id] end,
  status = function(id) return ({ [33] = "Haste" })[id] end,
  spell = function() return nil end,
  ability = function() return nil end,
  item = function(id) return ITEMS[id] end,
  zone = function() return "Somewhere" end,
}
chat = { add = function(t) said[#said + 1] = t end }
clock = function() return 1000 end
timer = { after = function(_, f) f() end }
commands = { run = function(l) ran[#ran + 1] = l end }
packets = { inject_out = function(d) sent[#sent + 1] = d end, build = function(id, b) return string.pack("<I2", id) .. b end }
fs = {
  list = function(dir)
    local t = {}
    for k in pairs(FILES) do
      local n = k:match("^" .. dir:gsub("%p", "%%%0") .. "([^/]+)$")
      if n then t[#t + 1] = n end
    end
    return t
  end,
  read = function(p) return FILES[p] end,
}
local function ids()
  local t = {}
  for _, d in ipairs(sent) do
    local id = string.unpack("<I2", d)
    if id == 0x050 then
      local index, slot = string.unpack("BB", d, 3)
      t[#t + 1] = string.format("%d:%d", slot, index)
    else
      t[#t + 1] = string.format("%03X", id)
    end
  end
  sent = {}
  return table.concat(t, " ")
end

local core = require("src.core")
local ac = require("src.acxml")

FILES["ashitacast/Tester_WAR.xml"] = [==[<?xml version="1.0"?>
<!-- a comment with <tags> in it -->
<ashitacast>
  <sets>
    <set name="Idle"><head>Idle Hat</head></set>
    <set name="Acc" baseset="Idle"><head>Acc Hat</head><body>Body B</body></set>
  </sets>
  <load><setvar name="mode" value="Idle"/><echo>loaded $p_name</echo></load>
  <commands>
    <command input="/mode"><setvar name="mode" value="$arg1"/><echo>mode $u_mode</echo></command>
    <command input="/modex"><echo>never: the shorter input wins</echo></command>
  </commands>
  <default>
    <if u_mode="Acc"><equip set="Acc"/></if>
    <else><equip set="Idle"/></else>
  </default>
</ashitacast>]==]

assert(ac.find() == "ashitacast/Tester_WAR.xml", tostring(ac.find()))
assert(ac.load(ac.find()), table.concat(said, " | "))
assert(said[#said] == "loaded Tester", said[#said])
ac.tick()
assert(ids() == "4:1", "idle hat")
-- /modex is taken by /mode (a prefix, the shorter input); the line is consumed.
assert(ac.typed("/modex Acc") == false and said[#said] == "mode Acc", said[#said])
assert(ac.typed("/other") == nil)
core.sent = {}
ac.tick()
assert(ids() == "4:2 5:4", "acc set over its baseset")

-- A v3 file through the import rules: idlegear, buffactive, lear, variables.
ac.unload()
core.sent = {}
FILES["ashitacast/Tester_WAR.xml"] = [==[<ashitacast>
  <variables><var name="x">7</var></variables>
  <sets><set name="Ears" lock="true"><lear>Ear A</lear></set></sets>
  <idlegear>
    <if buffactive="Haste"><equip set="Ears"/></if>
    <elseif advanced="%p_hpp &lt; 50"><equip><head>Acc Hat</head></equip></elseif>
    <doidlegear/>
  </idlegear>
</ashitacast>]==]
assert(ac.load(ac.find()), table.concat(said, " | "))
assert(ac._uvars().x == "7")
ac.tick()
assert(ids() == "", "no haste: nothing")
BUFFS = { { id = 33 } }
ac.tick()
assert(ids() == "11:3", "ear1 from lear under haste")
return "acxml flow ok"
"#;

    /// Steady Cast: a spell sent while moving waits with what follows it, and all of it goes in order once we stand
    /// still; a spell sent standing goes at once; a hold ends after three seconds even while moving.
    #[test]
    fn steady_cast_holds_while_moving() {
        let files = BUNDLED.iter().find(|b| b.0 == "steady-cast").unwrap().2;
        let host = Host::default();
        let vm =
            Vm::new(bundled_manifest("steady-cast").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let script = r#"
            local hold = require("src.hold")
            local function act(cat) return string.pack("<I2I2I4I2I2I4", 0x01A, 0, 200, 50, cat, 1) end
            local equip = string.pack("<I2I2BBBB", 0x050, 0, 1, 4, 0, 0)
            local sent = {}
            local send = function(d) sent[#sent + 1] = d end
            -- Moving: the cast and the midcast gear behind it wait.
            hold.frame({ x = 0, z = 0 }, 1.00, send)
            hold.frame({ x = 1, z = 0 }, 1.05, send)
            assert(hold.out(0x01A, act(3), 1.06) == false and hold.out(0x050, equip, 1.06) == false and hold.held() == 2)
            hold.frame({ x = 1, z = 0 }, 1.10, send)
            assert(#sent == 0, "still too short")
            hold.frame({ x = 1, z = 0 }, 1.25, send)
            assert(#sent == 2 and sent[1] == act(3) and sent[2] == equip)
            -- What we send again passes once.
            assert(hold.out(0x01A, sent[1], 1.25) == nil and hold.out(0x050, sent[2], 1.25) == nil)
            -- Standing: at once; a weapon skill never waits.
            assert(hold.out(0x01A, act(3), 2.0) == nil)
            hold.frame({ x = 2, z = 0 }, 2.1, send)
            assert(hold.out(0x01A, act(7), 2.1) == nil)
            -- Moving all along: three seconds, then it goes anyway.
            sent = {}
            assert(hold.out(0x01A, act(3), 2.1) == false)
            for t = 2.2, 5.3, 0.1 do hold.frame({ x = t, z = 0 }, t, send) end
            assert(#sent == 1)
            return "ok"
        "#;
        let got: String = vm.lua.load(script).eval().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, "ok");
    }

    /// Chat Alerts: a tell, our name and a chosen word call; our own lines, other people's names and a second call
    /// within a second do not.
    #[test]
    fn chat_alerts_calls() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "chat-alerts").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("chat-alerts").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local S = { on = true, sound = 2, tells = true, mentions = true, party = false, linkshell = false, invites = true,
              examined = false, emotes = false, away = false, words = "rare drop, nm" }
            local H, calls, clock = {}, {}, 0
            settings = { get = function(k) return S[k] end }
            events = { on = function(n, f) H[n] = f end }
            audio = { play_ui = function(id) calls[#calls + 1] = id end }
            game = { player = function() return { name = "Tester", id = 7 } end }
            text = { plain = function(t) return t end }
            os.clock = function() return clock end
            load(INIT)()
            local function heard(line, sender, kind)
              local n = #calls
              clock = clock + 2
              H.chat_message(line, sender, kind, false)
              return #calls > n
            end
            assert(heard("hi", "Friend", 3))
            assert(not heard("hi", "Tester", 3))
            assert(heard("Tester, over here", "Friend", 0))
            assert(not heard("Testers unite", "Friend", 0))
            assert(heard("an NM popped", "Friend", 1))
            assert(not heard("nothing", "Friend", 4))
            assert(calls[1] == 19)
            -- Two tells in one moment: one call.
            clock = clock + 2
            H.chat_message("hi", "Friend", 3, false)
            H.chat_message("hi again", "Friend", 3, false)
            assert(#calls == 4, #calls)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Blue Magic Sets: a monster readying a move that teaches a blue spell we lack gives a line and a call (only as
    /// a Blue Mage, never for one we know); the codex lists what is missing and what teaches it.
    #[test]
    fn blue_sets_learnable_moves() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "blue-sets").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("blue-sets").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local S = { on = true, alert = true, alert_sound = true }
            local H, C, said, calls, wins = {}, {}, {}, {}, {}
            local job, learned = 16, { [517] = true }
            settings = { get = function(k) return S[k] end }
            events = { on = function(n, f) H[n] = f end }
            commands = { register = function(n, f) C[n] = f end }
            timer = { every = function() end }
            chat = { add = function(t) said[#said + 1] = t end }
            audio = { play_ui = function(id) calls[#calls + 1] = id end }
            ui = { window = function(id, w) wins[id] = w end, open = function() end, close = function() end,
              is_open = function() return false end, on = function() end, update = function() end }
            game = {
              player = function() return { name = "Tester", main_job = job, main_level = 75 } end,
              entity = function(id) return { id = id, name = "Bat", kind = id == 9 and "player" or "mob" } end,
              spells = function() local t = {} for id in pairs(learned) do t[#t + 1] = id end return t end,
            }
            res = { spell = function(id) return ({ [515] = { name = "Maelstrom", levels = { [16] = 61 } },
                [517] = { name = "Metallic Body", levels = { [16] = 8 } } })[id] or { name = "Spell " .. id, levels = {} } end,
              mob_skill = function(id) return ({ [462] = "Maelstrom", [448] = "Metallic Body" })[id] end }
            load(INIT)()
            local function readies(actor, move, cat)
              H.action({ actor = actor, category = cat or 7, param = 0, targets = { { id = 1, results = { { value = move, message = 43 } } } } })
            end
            readies(5, 462)
            assert(said[#said] == "Blue Magic: Bat readies Maelstrom, which teaches Maelstrom.", said[#said])
            assert(#calls == 1 and calls[1] == 18)
            readies(5, 448)
            readies(9, 462)
            readies(5, 462, 8)
            assert(#said == 1, "a learned spell, a player, a spell cast: none")
            job = 1
            readies(5, 462)
            assert(#said == 1, "not a Blue Mage")
            C.blu("codex")
            local w = wins.blucodex
            assert(w and w.elements[1].text:match("^1 of %d+ learned$"), w and w.elements[1].text)
            local rows = w.elements[3].rows
            local found
            for _, r in ipairs(rows) do
              if r[1] == "Maelstrom" then found = r end
              assert(r[1] ~= "Metallic Body", "the Missing tab leaves learned spells out")
            end
            assert(found and found[2] == "61" and found[4] == "Maelstrom")
            return "ok"
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        let got: String = vm.lua.load(script).eval().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, "ok");
    }

    /// Tell Inbox: tells while /away are kept and each sender gets one away reply; none kept while online; back
    /// online, the count is told; //r answers the last sender.
    #[test]
    fn tell_inbox_keeps_and_replies() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "tell-inbox").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("tell-inbox").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local S = { on = true, reply = "Back soon." }
            local H, C, T, ran, said, away = {}, {}, {}, {}, {}, true
            settings = { get = function(k) return S[k] end }
            events = { on = function(n, f) H[n] = f end }
            commands = { register = function(n, f) C[n] = f end, run = function(l) ran[#ran + 1] = l end }
            timer = { every = function(_, f) T[#T + 1] = f end }
            game = { player = function() return { name = "Tester", away = away } end }
            text = { plain = function(t) return t end, strip = function(t) return t end }
            chat = { add = function(t) said[#said + 1] = t end }
            ui = { window = function() end, open = function() end, on = function() end, close = function() end }
            storage = { character = {}, save = function() end }
            load(INIT)()
            T[1]()
            H.chat_message("are you there?", "Friend", 3, false)
            H.chat_message("hello?", "Friend", 3, false)
            assert(#storage.character.inbox == 2)
            assert(#ran == 1 and ran[1] == "/tell Friend Back soon.", ran[1])
            away = false
            T[1]()
            assert(said[#said]:find("2 tells"), said[#said])
            H.chat_message("ok", "Other", 3, false)
            assert(#storage.character.inbox == 2)
            C.r("see", "you")
            assert(ran[#ran] == "/tell Other see you", ran[#ran])
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Target Debuffs: an effect landing on the target shows its icon; it goes when it wears off, and all go when the
    /// target is defeated.
    #[test]
    fn target_debuffs_track() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "target-debuffs").unwrap().2;
        let vm = Vm::new(bundled_manifest("target-debuffs").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local H, icons = {}, {}
            settings = { get = function() return true end }
            events = { on = function(n, f) H[n] = f end }
            game = { target = function() return { id = 77 } end }
            res = { status = function(id) return "Effect " .. id end }
            ui = { native = { rect = function() return nil end } }
            hud = { screen = function() return { width = 1920, height = 1080, ui_scale = 1 } end,
              clear = function() icons = {} end, icon = function(k, s) icons[k] = s.id end }
            load(INIT)()
            local function count() local n = 0 for _ in pairs(icons) do n = n + 1 end return n end
            H.action({ actor = 5, category = 4, targets = { { id = 77, results = { { message = 236, value = 4 } } } } })
            H.action({ actor = 5, category = 4, targets = { { id = 77, results = { { message = 2, value = 30 } } } } })
            H.frame()
            assert(icons.d4 == 4 and count() == 1)
            H.action({ actor = 5, category = 4, targets = { { id = 77, results = { { message = 237, value = 13 } } } } })
            H.frame()
            assert(count() == 2)
            H.battle_message(5, 77, 206, 4, 0)
            H.frame()
            assert(icons.d4 == nil and icons.d13 == 13)
            H.battle_message(5, 77, 6, 0, 0)
            H.frame()
            assert(count() == 0)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Pending Actions: a melee round or another spell's finish while a spell waits is not its answer, nor a message
    /// that is not one of the spell's refusals (akreao/vana-client#243); the spell's own start, finish and
    /// refusals are, and each kind matches its own ids.
    #[test]
    fn pending_actions_match_the_ask() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "pending-actions").unwrap().2;
        let vm = Vm::new(bundled_manifest("pending-actions").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local H, OUT, chip = {}, {}, nil
            local T = 100
            os.clock = function() return T end
            settings = { get = function() return true end }
            events = { on = function(n, f) H[n] = f end }
            packets = { on_out = function(id, f) OUT[id] = f end }
            game = { player = function() return { id = 5 } end, entity = function() return nil end,
              items = function() return { { slot = 3, id = 4112 } } end }
            res = { spell = function() return { name = "Cure" } end, ability = function() return { name = "Provoke" } end,
              item = function() return { name = "Potion" } end,
              message = function() return { text = "No." } end }
            text = { strip = function(s) return s end }
            ui = { window = function(_, spec) chip = spec.elements[1] end, update = function(_, _, v) chip = v end,
              open = function() end, close = function() chip = nil end }
            load(INIT)()
            local function ask(cat, param) OUT[0x01A]({ data = string.pack("<I4I4I2I2I4", 0, 0, 0, cat, param) }) end
            local function item() OUT[0x037]({ data = string.pack("<I4I4I4BBI4I4", 0, 0, 0, 0, 0, 0, 0)
              :sub(1, 0x0e) .. string.pack("<BBI4I4", 3, 0, 0, 0) }) end
            local function state() T = T + 0.2 H.frame() return chip and chip.tone end
            local function act(cat, param, value)
              local targets = value and { { id = 9, results = { { value = value } } } } or {}
              H.action({ actor = 5, category = cat, param = param, targets = targets })
            end
            -- A spell (Cure, 1): a melee round, another spell's finish, an interrupt and another spell's start leave
            -- it waiting.
            ask(3, 1)
            act(1, 0x746b6174) act(4, 2) act(8, 0x6f737073, 1) act(8, 0x6f736163, 2)
            assert(state() == "waiting")
            -- Melee-side chatter of ours (71 with no spell id, experience) is no refusal; its own start answers.
            H.battle_message(5, 9, 71, 0, 0) H.battle_message(5, 5, 8, 300, 0)
            assert(state() == "waiting")
            act(8, 0x6f736163, 1)
            assert(state() == nil)
            -- Its finish answers too; its refusal (47 with its id) refuses.
            ask(3, 1) act(4, 1) assert(state() == nil)
            ask(3, 1) H.battle_message(5, 9, 47, 2, 0) assert(state() == "waiting")
            H.battle_message(5, 9, 47, 1, 0) assert(state() == "refused")
            T = T + 10 state()
            -- A job ability (Provoke, 0x01A param 0x23): its finish's param, not another's.
            ask(9, 0x23) act(6, 0x24) assert(state() == "waiting")
            act(6, 0x23) assert(state() == nil)
            ask(9, 0x23) H.battle_message(5, 9, 88, 0x24, 0) assert(state() == "waiting")
            H.battle_message(5, 9, 88, 0, 0) assert(state() == "refused")
            T = T + 10 state()
            -- A weapon skill: its start's record value; a monster skill finish answers whatever its id.
            ask(7, 32) act(7, 0x65746163, 33) assert(state() == "waiting")
            act(7, 0x65746163, 32) assert(state() == nil)
            ask(7, 32) act(11, 999) assert(state() == nil)
            ask(7, 32) H.battle_message(5, 9, 192, 0, 0) assert(state() == "refused")
            T = T + 10 state()
            -- An item: its category alone; 62 only with no id or its own.
            item() act(1, 0) assert(state() == "waiting")
            act(9, 0) assert(state() == nil)
            item() H.battle_message(5, 9, 62, 4113, 0) assert(state() == "waiting")
            H.battle_message(5, 9, 62, 4112, 0) assert(state() == "refused")
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Job Gauges: SCH 99 with 100 s of the stratagem recast left has 2 of 5 charges, the next in 4 s; a NIN sub sees
    /// its shadows and tools.
    #[test]
    fn job_gauges_counts() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "job-gauges").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("job-gauges").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local T, shown = nil, nil
            settings = { get = function() return true end }
            timer = { every = function(_, f) T = f end }
            game = {
              player = function() return { main_job = 20, main_level = 99, sub_job = 13, sub_level = 49 } end,
              recasts = function() return { abilities = { [231] = 100 }, spells = {}, totals = { abilities = {}, spells = {} } } end,
              buffs = function() return { { id = 445 } } end,
              find_items = function(n) return n == "Shihei" and { { count = 30 } } or {} end,
            }
            ui = { window = function(_, spec) shown = spec.elements end, open = function() end, close = function() end }
            load(INIT)()
            T()
            local by = {}
            for _, e in ipairs(shown) do by[e.label] = e.text end
            assert(by["Stratagems"] == "2 / 5  +1 in 4s", by["Stratagems"])
            assert(by["Shadows"] == "3", by["Shadows"])
            assert(by["Shihei"] == "30")
            assert(by["Ready"] == nil)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Roll Helper: a Chaos Roll of 4 is lucky (+25), a Double-Up typed then is held back; at 9 it goes through; a
    /// bust ends the roll.
    #[test]
    fn roll_helper_lucky() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "roll-helper").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("roll-helper").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local H, F, said = {}, nil, {}
            settings = { get = function() return true end }
            events = { on = function(n, f) H[n] = f end }
            filters = { typed = function(f) F = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            game = { player = function() return { id = 5 } end }
            res = { ability = function(id) return ({ [0x200 + 98] = { name = "Chaos Roll" }, [0x200 + 123] = { name = "Double-Up" } })[id] end,
              status = function() return nil end }
            load(INIT)()
            local function act(param, msg, value)
              H.action({ actor = 5, category = 6, param = param, targets = { { id = 5, results = { { message = msg, value = value } } } } })
            end
            act(98, 420, 4)
            assert(said[#said] == "Chaos Roll 4 (Lucky!): Attack % +25  (lucky 4, unlucky 8)", said[#said])
            assert(F('/ja "Double-Up" <me>') == false)
            act(123, 424, 9)
            assert(said[#said]:find("^Chaos Roll 9: Attack %% %+17"), said[#said])
            assert(F('/ja "Double-Up" <me>') == nil)
            act(123, 426, 0)
            assert(said[#said] == "Chaos Roll: Bust!")
            assert(F('/ja "Double-Up" <me>') == nil)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Automaton Sets: a saved set changes only the slots that differ: a different attachment off, then the head,
    /// the frame and the set's attachments on.
    #[test]
    fn automaton_sets_round_trip() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "automaton-sets").unwrap().2;
        let vm = Vm::new(bundled_manifest("automaton-sets").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, ops, said = {}, {}, {}
            local job = { job = 18, head = 8193, frame = 8226, attachments = { 8449, 8450 }, attachment_slots = { 8449, 8450, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 } }
            settings = { get = function() return true end }
            commands = { register = function(n, f) C[n] = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            storage = { account = {}, save = function() end }
            sleep = function() end
            res = { item = function(id) return { name = "item" .. id } end }
            actions = { attach = function(slot, id) ops[#ops + 1] = "a" .. slot .. ":" .. id end,
              detach = function(slot) ops[#ops + 1] = "d" .. slot end }
            game = { job_data = function() return { main = job } end, entity_by_tag = function() return nil end }
            load(INIT)()
            C.automaton("save", "Tank")
            assert(storage.account.sets.tank.head == 8193)
            job = { job = 18, head = 8194, frame = 8227, attachments = { 8451 }, attachment_slots = { 0, 0, 8451, 0, 0, 0, 0, 0, 0, 0, 0, 0 } }
            C.automaton("tank")
            local s = table.concat(ops, " ")
            assert(s == "d5 a1:8193 a2:8226 a3:8449 a4:8450", s)
            assert(said[#said]:find("but not"), said[#said])
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Loot Tracker: a TH rise (proc 603) shows on the target; a new pool item counts as its dropper's drop, with the
    /// pool's time left.
    #[test]
    fn loot_tracker_th_and_pool() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "loot-tracker").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("loot-tracker").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local H, T, shown = {}, nil, {}
            settings = { get = function() return true end }
            events = { on = function(n, f) H[n] = f end }
            timer = { every = function(_, f) T = f end }
            commands = { register = function() end }
            local mob = { id = 77, name = "Forest Hare", kind = "mob" }
            game = { target = function() return mob end, entity = function(id) return id == 77 and mob or nil end,
              treasure = function() return { { slot = 0, item = 4358, count = 1, dropper = 77 } } end }
            res = { item = function() return { name = "Hare Meat" } end }
            ui = { window = function(_, s) shown = s.elements end, open = function() end, close = function() end }
            os.clock = function() return 100 end
            load(INIT)()
            H.action({ targets = { { id = 77, results = { { proc = { message = 603, value = 3 } } } } } })
            H.battle_message(5, 77, 6, 0, 0)
            T()
            assert(shown[1].label == "Treasure Hunter: Forest Hare" and shown[1].text == "3", shown[1].text)
            assert(shown[2].label == "Hare Meat" and shown[2].text == "5:00", shown[2].text)
            assert(shown[3].text == "Forest Hare: 1 killed \u{2014} Hare Meat x1", shown[3].text)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Monster Alerts: a monster's readied move and started spell make lines in the box; a watched move calls; a
    /// player's action makes none.
    #[test]
    fn monster_alerts_lines() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "monster-alerts").unwrap().2;
        let vm = Vm::new(bundled_manifest("monster-alerts").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local H, texts, calls = {}, {}, 0
            local S = { on = true, moves = "Foot Kick", monsters = "" }
            settings = { get = function(k) return S[k] end }
            events = { on = function(n, f) H[n] = f end }
            timer = { every = function() end }
            local E = { [9] = { id = 9, name = "Forest Hare", kind = "mob" }, [5] = { id = 5, name = "Lnxcsfix", kind = "player" } }
            game = { entity = function(id) return E[id] end, entities = function() return {} end }
            res = { mob_skill = function(id) return id == 257 and "Foot Kick" or nil end, ability = function() return nil end,
              spell = function(id) return id == 144 and { name = "Fire" } or nil end }
            audio = { play_ui = function() calls = calls + 1 end }
            hud = { screen = function() return { width = 1920, height = 1080, ui_scale = 1 } end, clear = function() texts = {} end,
              text = function(k, s) texts[k] = s.text end }
            load(INIT)()
            local function act(actor, cat, value)
              H.action({ actor = actor, category = cat, param = 24931, targets = { { id = 5, results = { { message = 43, value = value } } } } })
            end
            act(9, 7, 257)
            act(9, 8, 144)
            act(5, 7, 257)
            H.frame()
            assert(texts.l1 == "Forest Hare readies Foot Kick.", texts.l1)
            assert(texts.l2 == "Forest Hare starts casting Fire on Lnxcsfix.", texts.l2)
            assert(texts.l3 == nil and calls == 1)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Casket Solver: hint lines narrow 10..99; the answer comes when one is left.
    #[test]
    fn casket_solver_narrows() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "casket-solver").unwrap().2;
        let vm = Vm::new(bundled_manifest("casket-solver").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local F, T, said = nil, nil, {}
            settings = { get = function() return true end }
            filters = { chat = function(f) F = f end }
            timer = { every = function(_, f) T = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            text = { strip = function(t) return t end }
            load(INIT)()
            F("The monster was concealing a treasure chest!")
            F("You have a hunch that the combination is greater than 40 and less than 60.")
            T()
            assert(said[#said] == "Casket Solver: 19 left. Try 50.", said[#said])
            F("You have a hunch that the first digit is odd.")
            F("You have a hunch that the second digit is 2, 7, or 9.")
            T()
            assert(said[#said] == "Casket Solver: 3 left: 52, 57, 59. Try 57.", said[#said])
            F("You have a hunch that the lock's combination is greater than 57.")
            T()
            assert(said[#said] == "Casket Solver: the combination is 59.", said[#said])
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Warp Helper: the Home Point and Survival Guide walks pick the rows the menus showed live, through a second page.
    #[test]
    fn warp_helper_walks() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "warp-helper").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("warp-helper").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, said, picks, menus, at = {}, {}, {}, {}, 0
            settings = { get = function(k) return k == "fare" and "Gil" or true end }
            commands = { register = function(n, f) C[n] = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            events = { on = function() end }
            sleep = function() end
            game = {
              event = function()
                if at == 0 or at > #menus then return nil end
                return { menu = { question = menus[at][1], options = menus[at][2], cursor = 1 } }
              end,
              entities = function() return { { id = 7, name = "Home Point #1", distance = 3, pos = { x = 0, z = 0 } },
                { id = 8, name = "Survival Guide", distance = 40, pos = { x = 0, z = 0 } } } end,
            }
            actions = {
              talk = function() at = 1 end,
              choose = function(i) picks[#picks + 1] = i; at = at + 1 end,
            }
            load(INIT)()
            menus = {
              { "What will you do?", { "Travel to another home point.", "Set this as your home point.", "Other settings.", "On second thought, never mind." } },
              { "Teleport to which region?", { "On second thought, none.", "Select from favorites.", "Current region.", "San d'Oria.", "Bastok.", "Windurst.", "Jeuno.", "Next page." } },
              { "Teleport where?", { "Never mind.", "Ru'Lude Gardens.", "Upper Jeuno.", "Lower Jeuno.", "Port Jeuno." } },
              { "Teleport where?(Lower Jeuno)", { "Nowhere.", "Home Point #1 (E).", "Home Point #2 (M)." } },
              { "Teleport? (Current gil: 97531)", { "Add to favorites.", "Yes, please.", "No, thank you." } },
            }
            C.warp("hp", "lower", "jeuno", "2")
            assert(table.concat(picks, " ") == "1 7 4 3 2", table.concat(picks, " "))
            C.warp("jeuno")
            assert(said[#said] == "Warp Helper: which one: Upper Jeuno, Lower Jeuno, Port Jeuno?", said[#said])
            picks, at = {}, 0
            game.entities = function() return { { id = 8, name = "Survival Guide", distance = 2, pos = { x = 0, z = 0 } } } end
            menus = {
              { "What chapter will you read?", { "None.", "\u{201C}Teleportation Assistance.\u{201D}", "\u{201C}Other mysteries.\u{201D}" } },
              { "Travel to which region?", { "On second thought, none.", "Select from amongst favorites.", "Bastok.", "Ronfaure.", "Next page." } },
              { "Travel to which region?", { "On second thought, none.", "Select from amongst favorites.", "Movalpolos.", "Mamool Ja Savage.", "Previous page." } },
              { "Where will you travel?", { "Nowhere.", "Wajaom Woodlands.", "Mamook.", "Aydeewa Subterrane." } },
              { "Travel to Wajaom Woodlands?", { "Add to favorites.", "Travel using 1000 gil. (97531 gil)", "Travel using 50 tabs. (0 total)", "Not just yet." } },
            }
            C.warp("wajaom")
            assert(table.concat(picks, " ") == "2 5 4 2 2", table.concat(picks, " "))
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Content Trackers: each content's lines become its window's rows; a new zone starts afresh; Odyssey only from
    /// Rabao; the pop windows.
    #[test]
    fn content_trackers_read() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "content-trackers").unwrap().2;
        let vm = Vm::new(bundled_manifest("content-trackers").unwrap(), PathBuf::from("."), vm::Source::Bundled(files))
            .unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r##"
            local F, T, W, Z = nil, nil, nil, 1
            local NAMES = { [1] = "Dynamis - San d'Oria", [2] = "Abyssea - Konschtat", [3] = "Zhayolm Remnants", [4] = "Nyzul Isle",
              [292] = "Reisenjima Henge", [264] = "Yorcia Weald [U]", [261] = "Ceizak Battlegrounds", [262] = "Foret de Hennetiel",
              [247] = "Rabao", [298] = "Walk of Echoes [P1]", [279] = "Walk of Echoes [P2]", [275] = "Outer Ra'Kaznar [U1]" }
            local WINS, ON, CMDS, SAID, UPD = {}, {}, {}, {}, {}
            settings = { get = function() return true end }
            filters = { chat = function(f) F = f end }
            timer = { every = function(_, f) T = f end }
            text = { strip = function(t) return t end }
            commands = { register = function(n, f) CMDS[n] = f end }
            chat = { add = function(t) SAID[#SAID + 1] = t end }
            storage = { character = {} }
            ui = { window = function(id, spec) WINS[id] = spec if id == "content" then W = spec end end, open = function() end,
              close = function(id) WINS[id] = nil if id == "content" then W = nil end end, on = function(id, f) ON[id] = f end,
              is_open = function(id) return WINS[id] ~= nil end, update = function(id, el, f) UPD[#UPD + 1] = { id, el, f } end }
            res = { zone = function(id) return NAMES[id] end, item = function(id) return { name = "item" .. id } end,
              key_item = function(id) return { name = "ki" .. id } end }
            local bag = { { id = 1452, count = 30 }, { id = 1453, count = 2 } }
            local KEYS, BUFFS, POOL, COUNT = { 1546, 9 }, {}, {}, nil
            game = { player = function() return { zone = Z, name = "Lnxcsfix" } end, items = function(b) return b == 0 and bag or {} end,
              key_items = function() return KEYS end, buffs = function() return BUFFS end, treasure = function() return POOL end,
              countdown = function() return COUNT end }
            load(INIT)()
            local function texts()
              local out = {}
              for _, e in ipairs(W.elements) do out[#out + 1] = e.text end
              return table.concat(out, "|")
            end
            local function el(t)
              for _, e in ipairs(W.elements) do if e.text == t then return e end end
            end
            T()
            F("The sands of the hourglass have begun to fall.\nYou have 60 minutes (Earth time) remaining in Dynamis.")
            F("Your stay in Dynamis has been extended by 10 minutes.")
            T()
            assert(W.title == "Dynamis")
            assert(texts() == "69:59|Bronzepiece: 30|Silverpiece: 2|Currency worth 230 singles|Granules: Azure"
              or texts() == "70:00|Bronzepiece: 30|Silverpiece: 2|Currency worth 230 singles|Granules: Azure", texts())
            Z = 2
            T()
            F("Visitant Light Intensity")
            F("Pearlescent: 12 / Ebon: 3")
            F("Golden: 40 / Silvery: 5")
            F("Azure: 100 / Ruby: 0 / Amber: 255")
            F("Lnxcsfix's body emits a strong golden light!")
            F("Obtained 120 cruor. (Total: 4410)")
            F("Exiting in 5 minutes.")
            T()
            assert(W.elements[1].text == "4:59" or W.elements[1].text == "5:00", W.elements[1].text)
            F("Your visitant status will wear off in 30 minutes.")
            T()
            assert(texts():find("Pearlescent: 12|Ebon: 3|Golden: 40 %+|Silvery: 5|Azure: 100|Ruby: 0|Amber: 255|Cruor: 4410"), texts())
            Z = 3
            bag = { { id = 5365, count = 1 }, { id = 5376, count = 2 } }
            T()
            F("Main Weapon/Sub-Weapon restriction removed.")
            F("STR Down removed.")
            T()
            local t = texts()
            assert(not t:find("Main Weapon") and not t:find("STR Down") and t:find("Head/Neck|Body"), t)
            Z = 4
            T()
            F("Transfer complete. Welcome to Floor 21.")
            F("Objective: Eliminate all enemies.")
            F("You obtain 160 tokens!")
            F("Floor 21 objective complete. Rune of Transfer activated.")
            T()
            assert(texts() == "Floor 21 \u{2014} cleared|Eliminate all enemies|Tokens this run: 160", texts())

            -- Omen: the floor objective, omens, the bonus objectives; a cleared floor's next objective drops them.
            Z = 292
            T()
            F("Vanquish 5 sweetwater foes.")
            F("There are 12 omens from your foes!")
            F("Follow the light and carry out your charge before time runs out.\nYou have 120 seconds remaining.")
            F("1: Deal 3 critical hits to your foes.")
            F("2: Reduce your foe's HP by at least 2000 using a single weapon skill.")
            F("3: Restore at least 500 HP 4 times.")
            F("1: You have dealt 3 critical hits to your foes.")
            F("3: You have restored at least 500 HP 2 times.")
            F("2: You have failed to reduce your foe's HP by at least 2000 using a single weapon skill.")
            T()
            assert(W.title == "Omen")
            t = texts()
            assert(t:find("Floor 1  \u{00b7}  Omens: 12|Vanquish 5 sweetwater foes|", 1, true), t)
            assert(el("1  Critical hits  3/3").colour[2] == 220 and el("2  Weapon skill damage  at least 2000").colour[1] == 235, t)
            assert(el("3  Cures  2/4") and el("3  Cures  2/4").colour == nil, t)
            F("A spectral light flares up.")
            F("Your stay has been extended by 5 minutes.")
            F("Vanquish all monsters.")
            T()
            t = texts()
            assert(t == "Floor 2  \u{00b7}  Omens: 12|Vanquish all monsters", t)

            -- Delve: a fracture's plasm, kills, stage and airlixirs; in a field zone the plasm opens the window.
            Z = 264
            T()
            F("You receive 150 corpuscles of mweya plasm.")
            F("You receive 500 corpuscles, making for a total of 12500.")
            F("Your ears pick up a discordant howl.")
            F("You find an airlixir +1 on the Yorcia Bloodfly.")
            T()
            assert(W.title == "Delve")
            assert(texts() == "Plasm: 650 (total 12500)|Kills (estimate): 4|3 of 6|Airlixirs: 0  \u{00b7}  +1: 1  \u{00b7}  +2: 0", texts())
            Z = 261
            T()
            assert(W == nil)
            F("You receive 100 corpuscles of mweya plasm.")
            T()
            assert(W.title == "Delve" and texts():find("Plasm: 100", 1, true), texts())

            -- A reive: ours joined, momentum, bayld (a quest's is not counted).
            Z = 262
            T()
            F("Lnxcsfix has joined the Colonization Reive!")
            F("Reive momentum score: Physical attack.")
            F("Momentum bonus: HP recovery!")
            F("Momentum Bonus: Fast Cast!")
            F("Lnxcsfix obtained 300 bayld!")
            F("You have obtained 50 bayld!")
            T()
            assert(W.title == "Reive", tostring(W and W.title))
            t = texts()
            assert(t:find("Colonization Reive|", 1, true) and t:find("Bayld: 300", 1, true) and t:find("1  Physical attack", 1, true)
              and t:find("HP recovery \u{00d7}1", 1, true) and t:find("Fast Cast \u{00d7}1", 1, true), t)

            -- Odyssey: armed in Rabao, the run, the last run back in Rabao; the moglophone's wait.
            KEYS = { 3212 }
            Z = 247
            T()
            assert(W.title == "Odyssey" and texts() == "Moglophone: in hand", texts())
            F("Entering Sheol B: Difficult.")
            T()
            KEYS = {}
            Z = 298
            T()
            assert(W.title == "Odyssey")
            F("You receive 20 moogle segments for a total of 1020.")
            F("You receive 5 izzat, for a total of 35.")
            F("Moogle Mastery is now level IV.")
            T()
            assert(texts() == "Sheol B (Difficult)|Segments: 20 (total 1020)|Izzat: 5 (total 35)|Moogle Mastery: IV", texts())
            Z = 247
            T()
            t = texts()
            assert(t:find("Last run: Sheol B (Difficult)|Segments: 20 (total 1020)|Another moglophone in about ", 1, true), t)
            Z = 279
            T()
            assert(W == nil, "not entered from Rabao: not Odyssey")

            -- Sortie: our gallimaufry, coffer reports, temporary items; the game's countdown for the time.
            Z = 275
            COUNT = { left = 600, length = 1800 }
            T()
            F("Lnxcsfix received 100 gallimaufry for a total of 900.")
            F("Someone received 50 gallimaufry for a total of 60.")
            F("#B treasure coffer status report: 3/3 #?: 1/1.")
            F("You obtain the temporary item: Ra'Kaznarian Ether!")
            T()
            assert(W.title == "Sortie")
            t = texts()
            assert(t:find("^10:00|Gallimaufry: 100 %(total 900%)|#A  no report yet|#B  3/3   #%?: 1/1|"), t)
            assert(el("#B  3/3   #?: 1/1").colour[2] == 220 and t:find("Temporary items: Ra'Kaznarian Ether", 1, true), t)

            -- Pop lists: an NM's tree with what we hold; the NM list; the Zeni NMs and a ???'s place.
            KEYS = { 1530 }
            bag = { { id = 3262, count = 1 }, { id = 3291, count = 12 } }
            CMDS.pops("alf")
            local P = WINS.pops
            assert(P.title == "Alfard", P.title)
            local tree = P.elements[2].rows
            assert(#tree == 5 and tree[1].colour[2] == 220 and tree[2].colour[2] == 220 and tree[3].colour[1] == 235, #tree)
            assert(tree[1].cells[1] == "ki1530" and tree[4].cells[1] == "        item3267" and tree[1].cells[2] == "Ningishzida  Forced (I-7/I-8)",
              tree[4].cells[1])
            assert(P.elements[3].text == "Every pop held" and P.elements[4].text == "item3291: 12 / 75", P.elements[4].text)
            T()
            assert(#UPD > 0 and UPD[1][1] == "pops")
            CMDS.pops("")
            assert(WINS.pops.title == "Pop lists" and #WINS.pops.elements[1].rows == 28)
            ON.pops("pops", { kind = "select", id = "nms", index = 2 })
            assert(WINS.pops.title == "Amphitrite")
            CMDS.pops("nobody")
            assert(SAID[#SAID]:find("No pop list", 1, true))
            CMDS.znm()
            assert(WINS.znm.elements[1].type == "tabs" and #WINS.znm.elements[1].items == 4)
            ON.znm("znm", { kind = "select", id = "t1_1", index = 1 })
            assert(SAID[#SAID] == "Vulpangue: its ??? is at (-697, -7, -123) in zone 51.", SAID[#SAID])
        "##;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// No two bundled mods register the same // command (the first loaded would take it).
    #[test]
    fn bundled_commands_unique() {
        let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for (id, _, files) in BUNDLED {
            for (_, src) in files.iter() {
                for part in src.split("commands.register(\"").skip(1) {
                    let name = part.split('"').next().unwrap().to_string();
                    if let Some(other) = seen.insert(name.clone(), id) {
                        assert_eq!(other, *id, "//{name} is registered by both {other} and {id}");
                    }
                }
            }
        }
    }

    /// Multibox Control: //send, //sendtarget and //stna go out as ipc lines; a window that gets them acts.
    #[test]
    fn multibox_routes() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "multibox").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("multibox").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, E, sent, ran, cast, said = {}, {}, {}, {}, {}, {}
            settings = { get = function() return true end }
            commands = { register = function(n, f) C[n] = f end, run = function(l) ran[#ran + 1] = l end }
            events = { on = function(n, f) E[n] = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            ipc = { send = function(t, to) sent[#sent + 1] = (to or "*") .. "|" .. t end }
            client = { focus = function() ran[#ran + 1] = "focus" end }
            actions = { cast = function(s, who) cast[#cast + 1] = s .. ">" .. who end, target = function(id) ran[#ran + 1] = "target " .. id end }
            game = {
              player = function() return { name = "Mage" } end,
              target = function() return { id = 1234 } end,
              spells = function() return { 14, 15, 20, 143 } end,
              recasts = function() return { spells = {} } end,
              party = function() return {
                { slot = 0, name = "Mage", in_zone = true, hp = 500, buffs = { 3 } },
                { slot = 1, name = "Tank", in_zone = true, hp = 900, buffs = { 4, 13 } },
              } end,
            }
            load(INIT)()
            C.send("Tank", "/ja", "Provoke", "<t>")
            C.sendtarget()
            C.stna("Mage")
            assert(sent[1] == "Tank|run\t/ja Provoke <t>", sent[1])
            assert(sent[2] == "*|target\t1234", sent[2])
            assert(cast[1] == "15>Tank", tostring(cast[1]))
            E.ipc_message("run\t/heal", "Mage")
            E.ipc_message("target\t77", "Mage")
            E.ipc_message("focus\t", "Mage")
            assert(table.concat(ran, ",") == "/heal,target 77,focus", table.concat(ran, ","))
            E.chat_message("hello", "Friend", 3, false)
            assert(sent[3] == "*|tell\t[Mage] Friend>> hello", sent[3])
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Action Panel: //actions shows its window (a HUD one: the game keeps its keys), listing the castable spells by
    /// school and the abilities by tab with the costs the game's lists show; a row runs its macro line; the list
    /// follows a pet called while it is open.
    #[test]
    fn action_panel_opens() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "action-panel").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("action-panel").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            C, E, ran, said, tick = {}, {}, {}, {}, nil
            on, pet = true, nil
            settings = { get = function() return on end }
            commands = { register = function(n, f) C[n] = f end, run = function(l) ran[#ran + 1] = l end }
            events = { on = function(n, f) E[n] = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            timer = { every = function(_, f) tick = f end }
            local spells = {
              [1] = { name = "Cure", kind = 1, levels = { [3] = 1 }, mp = 8, targets = 0x1F },
              [100] = { name = "Protect", kind = 1, levels = { [3] = 7 }, mp = 9, targets = 1 },
              [144] = { name = "Fire", kind = 2, levels = { [4] = 13 }, mp = 7, targets = 0x20 },
              [145] = { name = "Fire II", kind = 2, levels = { [4] = 38 }, mp = 26, targets = 0x20 },
              [146] = { name = nil, kind = 2, levels = { [4] = 1 }, mp = 1, targets = 0x20 },
            }
            local function ab(name, kind, t) t.name, t.kind = name, kind; t.tp = t.tp or 255; t.cost = t.cost or 0
              t.targets = t.targets or 1; t.recast_id = t.recast_id or 900; t.sub = t.sub or 0; return t end
            local abilities = {
              [0x20] = ab("Fast Blade", 3, { targets = 0x20 }),
              [0x210] = ab("Divine Seal", 1, { recast_id = 26 }),
              [0x220] = ab("Curing Waltz", 11, { tp = 200, targets = 0x1F }),
              -- Not listed: a trait, and a row that only opens a sub-list (its rolls are listed).
              [0x230] = ab("Resist Sleep", 4, {}),
              [0x240] = ab("Phantom Roll", 1, { recast_id = 193, sub = 8 }),
              [0x250] = ab("Fighter's Roll", 8, { recast_id = 193 }),
              -- Pet commands, only with a pet: a blood pact costs MP, a Ready move charges.
              [0x400] = ab("Fight", 2, { targets = 0x20 }),
              [0x410] = ab("Foot Kick", 0x12, { cost = 1, targets = 0x20 }),
              [0x420] = ab("Searing Light", 6, { cost = 107, targets = 0x20 }),
            }
            game = {
              player = function() return { main_job = 3, main_level = 75, sub_job = 4, sub_level = 37 } end,
              info = function() return { pet_index = pet } end,
              job_data = function() return {} end,
              spells = function() return { 1, 100, 144, 145, 146 } end,
              abilities = function() return { 0x20, 0x210, 0x220, 0x230, 0x240, 0x250, 0x400, 0x410, 0x420 } end,
              recasts = function() return { spells = { [1] = 3.2 }, abilities = { [26] = 300 }, totals = {} } end,
            }
            res = { spell = function(id) return spells[id] end, ability = function(id) return abilities[id] end }
            local on_window = ui.on
            ui.on = function(id, f) H = f; on_window(id, f) end
            load(INIT)()
            C.actions()
            assert(ui.is_open("actions"), "//actions opened no window")
            tick()
            -- White magic (the first school): Cure and Protect by name; a pick runs its line.
            H("actions", { kind = "select", id = "list", index = 1 })
            H("actions", { kind = "select", id = "list", index = 2 })
            -- Black magic: the nameless spell (names still loading) is left out.
            H("actions", { kind = "set", id = "school", index = 2 })
            H("actions", { kind = "select", id = "list", index = 1 })
            -- Weapon skills.
            H("actions", { kind = "set", id = "tabs", index = 3 })
            H("actions", { kind = "select", id = "list", index = 1 })
            assert(table.concat(ran, "|") == '/ma "Cure" <st>|/ma "Protect" <me>|/ma "Fire" <st>|/ws "Fast Blade" <st>',
              table.concat(ran, "|"))
            C.actions()
            assert(not ui.is_open("actions"), "a second //actions closes it")
            on = false
            C.actions()
            assert(not ui.is_open("actions"), "off: //actions does nothing")
            on = true
            C.actions()
            on = false
            E.settings_changed("on", false)
            assert(not ui.is_open("actions"), "switched off: the window closes")
            on = true
            C.actions()
            H("actions", { kind = "press", id = "close", index = 1 })
            assert(not ui.is_open("actions"), "Close closes it")
            C.actions()
            H("actions", { kind = "set", id = "tabs", index = 2 })
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        let run = |code: &str| vm.lua.load(code).exec().unwrap_or_else(|e| panic!("{e}"));
        let rows = || -> Vec<Vec<String>> {
            let st = vm.st.borrow();
            let win = st.windows.iter().find(|w| w.id == "actions").unwrap();
            assert!(win.open && win.hud && !win.focus, "a HUD window, open, without the keys");
            win.elems
                .iter()
                .find_map(|e| match e {
                    window::Elem::List { rows, .. } => Some(rows.iter().map(|r| r.cells.clone()).collect()),
                    _ => None,
                })
                .unwrap()
        };
        run(script);
        // The Abilities page: the waltz's TP, Divine Seal's recast, Fighter's Roll.
        assert_eq!(rows(), [["Curing Waltz", "200 TP", ""], ["Divine Seal", "", "5:00"], ["Fighter's Roll", "", ""]]);
        // The Pet page: nothing without a pet; the commands once one is called.
        run(r#"H("actions", { kind = "set", id = "tabs", index = 4 })"#);
        assert!(rows().is_empty());
        run("pet = 1234; tick()");
        assert_eq!(rows(), [["Fight", "", ""], ["Foot Kick", "1 charge", ""], ["Searing Light", "107 MP", ""]]);
    }

    /// Gear Stats: descriptions and augments summed; conditional lines and DMG / Delay left out.
    #[test]
    fn gear_stats_sum() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "gear-stats").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("gear-stats").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, W = {}, nil
            settings = { get = function() return true end }
            commands = { register = function(n, f) C[n] = f end }
            timer = { every = function() end }
            local open = false
            ui = { window = function(_, spec) W = spec end, open = function() open = true end, close = function() end,
              is_open = function() return open end }
            local desc = {
              [1] = "DMG:20 Delay:240\nSTR+2 Accuracy+5",
              [2] = "DEF:30 HP+20 MP-10\n\"Store TP\"+3 Haste+3%\nPet: Attack+10",
              [3] = "DEF:5 Accuracy+2\nSet: Increases Haste",
            }
            res = {
              item = function(id) return { description = desc[id] } end,
              augments = function(id) return { lines = id == 3 and { "STR+1", "Magic Atk. Bonus+4" } or {} } end,
            }
            game = {
              equipment = function() return { [0] = { id = 1, bag = 0, slot = 1 }, [5] = { id = 2, bag = 0, slot = 2 },
                [15] = { id = 3, bag = 0, slot = 3 } } end,
              items = function() return { { slot = 3, extdata = "x" } } end,
            }
            load(INIT)()
            C.gearstats()
            local got = {}
            for _, r in ipairs(W.elements[1].rows) do got[#got + 1] = r[1] .. "=" .. r[2] end
            local s = table.concat(got, " ")
            assert(s == '"Store TP"=+3 Accuracy=+7 DEF=35 HP=+20 Haste=+3% MP=-10 Magic Atk. Bonus=+4 STR=+3', s)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Calculator: precedence, brackets, powers, k / m, commas in the answer; bad input is an error line, never run.
    #[test]
    fn calculator_answers() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "calculator").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("calculator").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, said = {}, {}
            settings = { get = function() return true end }
            commands = { register = function(n, f) C[n] = f end }
            chat = { add = function(t) said[#said + 1] = t end }
            ui = { window = function() end, open = function() end, on = function() end, update = function() end }
            load(INIT)()
            local function calc(s) C.calc(s) return said[#said] end
            assert(calc("12 * 99 + 450") == "12 * 99 + 450 = 1,638", said[#said])
            assert(calc("(2+3)*4") == "(2+3)*4 = 20")
            assert(calc("2^10") == "2^10 = 1,024")
            assert(calc("1.5k / 4") == "1.5k / 4 = 375")
            assert(calc("2m - 1") == "2m - 1 = 1,999,999")
            assert(calc("10 / 4") == "10 / 4 = 2.5")
            assert(calc("os.exit()"):find("^Calculator: "))
            assert(calc("1/0"):find("no answer"))
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Chat Filter: a listed player, a listed word, a hidden kind and a repeat are dropped; others pass.
    #[test]
    fn chat_filter_drops() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "chat-filter").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("chat-filter").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local S = { on = true, players = "Spammer, Bot", words = "gil selling", shouts = false, yells = true, repeats = true }
            local F, clock = nil, 0
            settings = { get = function(k) return S[k] end }
            filters = { chat = function(f) F = f end }
            timer = { every = function() end }
            text = { strip = function(t) return t end }
            os.clock = function() return clock end
            load(INIT)()
            assert(F("Spammer : hi", 9) == false)
            assert(F("[1]<Bot> hi", 14) == false)
            assert(F("(Friend) hi", 13) == nil)
            assert(F("Friend : cheap GIL SELLING here", 10) == false)
            assert(F("Friend : party?", 11) == false)
            assert(F("Friend : need a healer", 10) == nil)
            clock = 30
            assert(F("Friend : need a healer", 10) == false)
            clock = 100
            assert(F("Friend : need a healer", 10) == nil)
            assert(F("The Forest Hare is paralyzed.", 36) == nil)
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Web Search: the words given, else the item under a list's cursor, else the target, go to the site, encoded.
    #[test]
    fn web_search_urls() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "web-search").unwrap().2;
        let vm =
            Vm::new(bundled_manifest("web-search").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let init = files.iter().find(|f| f.0 == "init.lua").unwrap().1;
        let script = r#"
            local C, opened, high, target = {}, {}, nil, nil
            settings = { get = function() return true end }
            commands = { register = function(n, f) C[n] = f end }
            net = { open_url = function(u) opened[#opened + 1] = u end }
            chat = { add = function() end }
            ui = { native = { highlighted = function() return high end } }
            game = { target = function() return target end }
            res = { item = function(id) return id == 4096 and { name = "Fire Crystal" } or nil end }
            load(INIT)()
            C.wiki("Kupipi's", "Trust")
            assert(opened[1] == "https://www.bg-wiki.com/index.php?search=Kupipi%27s%20Trust", opened[1])
            high = { kind = "item", id = 4096 }
            C.ah()
            assert(opened[2] == "https://www.ffxiah.com/search/item?q=Fire%20Crystal", opened[2])
            high, target = nil, { name = "Forest Hare" }
            C.wiki()
            assert(opened[3] == "https://www.bg-wiki.com/index.php?search=Forest%20Hare", opened[3])
        "#;
        vm.lua.globals().set("INIT", init).unwrap();
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// Hotbars: a macro's first action names its icon and recast; a drag swaps two slots, in the sub job's set when
    /// either slot comes from it.
    #[test]
    fn hotbar_actions_and_swap() {
        let host = Host::default();
        let files = BUNDLED.iter().find(|b| b.0 == "hotbar").unwrap().2;
        let vm = Vm::new(bundled_manifest("hotbar").unwrap(), PathBuf::from("."), vm::Source::Bundled(files)).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let script = r#"
            res = setmetatable({
              spell = function(id) if id == 1 then return { name = "Cure" } end end,
              ability = function(id)
                if id == 0x20 then return { name = "Raging Axe" } end
                if id == 0x210 then return { name = "Provoke", recast_id = 5 } end
              end,
            }, { __index = function() return function() return nil end end })
            game.find_items = function(name) if name == "potion" then return { { id = 4112 } } end return {} end
            local a = require("src.actions")
            local p = a.of({ lines = { "/echo go", '/ja "Provoke" <t>' } })
            assert(p.kind == "ability" and p.id == 0x210 and p.recast_id == 5)
            assert(a.of({ lines = { "/ma Cure <me>" } }).id == 1)
            assert(a.of({ lines = { '/ws "Raging Axe" <t>' } }).id == 0x20)
            assert(a.of({ lines = { '/item "Potion" <me>' } }).id == 4112)
            assert(a.of({ lines = { "/echo nothing" } }) == false)
            local r = { spells = {}, abilities = { [5] = 15 }, totals = { spells = {}, abilities = { [5] = 30 } } }
            local left, frac = a.recast(p, r)
            assert(left == 15 and frac == 0.5)

            local saved
            local data = require("src.data")
            data.init({ read = function() return nil end, write = function(_, t) saved = t end, decode = json.decode,
              encode = json.encode })
            local m = function(n) return { name = n, lines = { "/echo " .. n } } end
            data.set_macro("WAR", nil, 1, 1, m("one"))
            data.set_macro("WAR", nil, 1, 2, m("two"))
            data.set_macro("WAR", "NIN", 1, 3, m("sub"))
            data.swap("WAR", nil, 1, 1, 1, 2)
            assert(data.resolve("WAR", nil, 1, 1).name == "two" and data.resolve("WAR", nil, 1, 2).name == "one")
            -- Into the sub set: the main set keeps its macro under the sub set's.
            data.swap("WAR", "NIN", 1, 3, 1, 4)
            local four, from = data.resolve("WAR", "NIN", 1, 4)
            assert(four.name == "sub" and from == "sub")
            assert(data.resolve("WAR", "NIN", 1, 3) == nil)
            assert(saved ~= nil)
        "#;
        vm.lua.load(script).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    /// ui.native.place / rect / panels and ui.skin: names checked, the permission asked, each change its mod's (another
    /// mod's release leaves it), a skin change counted once; HUD anchors name panels.
    #[test]
    fn native_panels_and_skin() {
        let host = Host::default();
        let new = |id: &str, granted: bool| {
            let vm = Vm::new(
                manifest::Manifest { id: id.into(), ..Default::default() },
                PathBuf::from("."),
                vm::Source::Dir(PathBuf::from(".")),
            )
            .unwrap();
            vm.lua.set_app_data(host.info.clone());
            api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
            if granted {
                vm.st.borrow_mut().granted.push("ui.native".into());
            }
            vm
        };
        let (a, b, c) = (new("a", true), new("b", true), new("c", false));
        let run = |vm: &Vm, code: &str| vm.lua.load(code).exec();
        run(&a, r#"ui.native.place("party", {x = 10, y = 20, scale = 2})"#).unwrap();
        run(&b, r#"ui.native.place("party", nil)"#).unwrap();
        assert_eq!(
            host.info.borrow().placed.get("party").map(|p| (p.0.as_str(), p.1.scale, p.1.x)),
            Some(("a", 2.0, Some(10.0)))
        );
        for bad in [
            r#"ui.native.place("timers", {})"#,
            r#"ui.native.place("nope", {})"#,
            r#"ui.native.hide("command")"#,
            r#"ui.skin.sprite("nope", false)"#,
            r#"ui.skin.colour("nope", false)"#,
            r#"hud.rect("r", {anchor = "nope"})"#,
        ] {
            assert!(run(&a, bad).is_err(), "{bad}");
        }
        assert!(run(&c, r#"ui.native.place("party", {})"#).unwrap_err().to_string().contains("ui.native"));
        run(
            &a,
            r#"ui.native.hide("focus"); ui.skin.colour("gold", {255, 0, 0}); ui.skin.sprite("gauges/hp_full", false)"#,
        )
        .unwrap();
        run(&b, r#"ui.skin.colour("gold", false)"#).unwrap();
        assert!(host.hidden_panels().contains(&"focus"));
        {
            let f = host.info.borrow();
            assert_eq!(
                (f.skin.ver, f.skin.colours.get("gold").and_then(|v| v.last()).map(|e| e.1)),
                (1, Some([255, 0, 0, 255]))
            );
        }
        host.info.borrow_mut().skin.forget("a");
        assert_eq!((host.info.borrow().skin.ver, host.info.borrow().skin.colours.len()), (2, 0));
        host.set_native_rects(vec![("party".into(), [1.0, 2.0, 3.0, 4.0])], Vec::new());
        assert_eq!(
            a.lua
                .load(r#"local r = ui.native.rect("party"); return r.x, r.h, r.shown"#)
                .eval::<(i64, i64, bool)>()
                .unwrap(),
            (1, 4, true)
        );
        // Closed: the last rect stays. Hidden by a mod: where it would stand now.
        host.set_native_rects(Vec::new(), Vec::new());
        assert_eq!(
            a.lua.load(r#"local r = ui.native.rect("party"); return r.x, r.shown"#).eval::<(i64, bool)>().unwrap(),
            (1, false)
        );
        host.set_native_rects(Vec::new(), vec![("party".into(), [9.0; 4]), ("where".into(), [5.0, 6.0, 7.0, 0.0])]);
        assert_eq!(
            a.lua.load(r#"local r = ui.native.rect("party"); return r.x, r.shown"#).eval::<(i64, bool)>().unwrap(),
            (9, false)
        );
        assert_eq!(
            a.lua.load(r#"local r = ui.native.rect("where"); return r.x, r.shown"#).eval::<(i64, bool)>().unwrap(),
            (5, false)
        );
        assert!(run(&a, r#"ui.native.place("target", {scale = 2, origin = "middle"})"#).is_err());
        run(&a, r#"ui.native.place("target", {scale = 2, origin = "bottom_right"})"#).unwrap();
        assert_eq!(host.info.borrow().placed.get("target").map(|p| p.1.origin), Some((1.0, 1.0)));
        assert!(a.lua.load(r#"return ui.native.rect("focus")"#).eval::<Option<mlua::Table>>().unwrap().is_none());
        run(&a, r#"hud.rect("r", {anchor = "party", corner = "top_right", x = -10, y = -24})"#).unwrap();
        assert!(run(&a, r#"hud.rect("r", {anchor = "party", corner = "middle"})"#).is_err());
        let n: i64 = a
            .lua
            .load(r#"local n = 0 for _, p in ipairs(ui.native.panels()) do n = n + 1 end return n"#)
            .eval()
            .unwrap();
        assert_eq!(n as usize, native::PANELS.len());
    }

    /// Every bundled mod's manifest parses and its Lua compiles; settings on our Extras tabs name one and label their
    /// values.
    #[test]
    fn bundled_mods_load() {
        for (id, text, files) in BUNDLED {
            let m = manifest::parse(text, id).unwrap_or_else(|e| panic!("{id}: {e:#}"));
            for (name, src) in files.iter() {
                let lua = mlua::Lua::new();
                lua.load(*src).set_name(*name).into_function().unwrap_or_else(|e| panic!("{id}/{name}: {e}"));
            }
            for (key, d) in &m.settings {
                assert!(d.tab.is_empty() || !d.label.is_empty(), "{id}.{key}: a tabbed setting needs its row's label");
            }
        }
    }

    /// A setting `from` a config field takes its value once, when not saved yet; others are left alone.
    #[test]
    fn settings_carry_over_from_the_config() {
        let m = manifest::parse(
            r#"
            [settings.a]
            type = "int"
            from = "x"
            [settings.b]
            type = "bool"
            from = "y"
            scope = "character"
            [settings.c]
            type = "bool"
            from = "missing"
            [settings.d]
            type = "bool"
            "#,
            "t",
        )
        .unwrap();
        let config = serde_json::json!({"x": 3, "y": true});
        assert_eq!(
            migrations(&m, &config, |_, _| false),
            [("a".to_string(), false, 3.into()), ("b".to_string(), true, true.into())]
        );
        assert_eq!(migrations(&m, &config, |k, character| k == "b" && character), [("a".to_string(), false, 3.into())]);
        // Labelled values need a label each; a tab must be one of ours.
        assert!(manifest::parse("[settings.e]\ntype = \"int\"\noptions = [\"OFF\"]\nvalues = [0, 3]", "t").is_err());
        assert!(manifest::parse("[settings.e]\ntype = \"int\"\ntab = \"Nope\"", "t").is_err());
    }

    /// Per-window opacity: a mod's for its own windows (ui.window, ui.opacity, hud.opacity), cleared by it alone; the
    /// player's wins.
    #[test]
    fn window_opacity() {
        let host = Host::default();
        let new = |id: &str| {
            let vm = Vm::new(
                manifest::Manifest { id: id.into(), ..Default::default() },
                PathBuf::from("."),
                vm::Source::Dir(PathBuf::from(".")),
            )
            .unwrap();
            vm.lua.set_app_data(host.info.clone());
            api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
            vm
        };
        let (a, b) = (new("a"), new("b"));
        a.lua.load(r#"ui.window("w", {title = "W", opacity = 0.5}); hud.opacity("bars", 0.25); hud.rect("r", {window = "bars"})"#).exec().unwrap();
        let player = std::collections::BTreeMap::new();
        assert_eq!(
            (
                host.window_opacity(&player, "mod:a:w"),
                host.window_opacity(&player, "hud:a:bars"),
                host.window_opacity(&player, "hud:a:hud")
            ),
            (0.5, 0.25, 1.0)
        );
        assert!(a.st.borrow().hud.iter().any(|e| e.id == "r" && e.window == "bars"));
        b.lua.load(r#"ui.opacity("w", nil)"#).exec().unwrap();
        assert_eq!(host.window_opacity(&player, "mod:a:w"), 0.5);
        let player = std::collections::BTreeMap::from([("mod:a:w".to_string(), 0.75)]);
        assert_eq!(host.window_opacity(&player, "mod:a:w"), 0.75);
        a.lua.load(r#"ui.opacity("w", nil)"#).exec().unwrap();
        assert_eq!(host.window_opacity(&std::collections::BTreeMap::new(), "mod:a:w"), 1.0);
    }

    /// ui.skin: the newest mod's change wins, and a clear or unload lets the change before it show again.
    #[test]
    fn skin_override_precedence() {
        let host = Host::default();
        let new = |id: &str| {
            let vm = Vm::new(
                manifest::Manifest { id: id.into(), ..Default::default() },
                PathBuf::from("."),
                vm::Source::Dir(PathBuf::from(".")),
            )
            .unwrap();
            vm.lua.set_app_data(host.info.clone());
            api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
            vm.st.borrow_mut().granted.push("ui.native".into());
            vm
        };
        let (a, b) = (new("a"), new("b"));
        let run = |vm: &Vm, code: &str| vm.lua.load(code).exec();
        let gold = || host.info.borrow().skin.colours.get("gold").and_then(|v| v.last()).map(|e| e.1);
        run(&a, r#"ui.skin.colour("gold", {255, 0, 0})"#).unwrap();
        run(&b, r#"ui.skin.colour("gold", {0, 0, 255})"#).unwrap();
        assert_eq!(gold(), Some([0, 0, 255, 255])); // b, the newest, wins.
        run(&b, r#"ui.skin.colour("gold", false)"#).unwrap();
        assert_eq!(gold(), Some([255, 0, 0, 255])); // b gives it back: a's shows again.
        run(&b, r#"ui.skin.colour("gold", {0, 255, 0})"#).unwrap();
        host.info.borrow_mut().skin.forget("b"); // b unloads: a's still shows.
        assert_eq!(gold(), Some([255, 0, 0, 255]));
        run(&a, r#"ui.skin.colour("gold", false)"#).unwrap();
        assert_eq!(gold(), None);
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
            dir.borrow_mut().mods.push(runtime::Entry {
                id: id.into(),
                name: id.into(),
                version: String::new(),
                lua: vm.lua.weak(),
                st: vm.st.clone(),
            });
            vm
        };
        let (caller, provider) = (new("caller"), new("provider"));
        provider.lua.load("mods.provide('twice', function(n) return n * 2 end)").exec().unwrap();
        assert_eq!(caller.lua.load("return mods.call('provider', 'twice', 21)").eval::<i64>().unwrap(), 42);
        provider.st.borrow_mut().suspended = true;
        let e = caller.lua.load("return mods.call('provider', 'twice', 21)").eval::<i64>().unwrap_err();
        assert!(e.to_string().contains("suspended"), "{e}");
    }

    /// A `__gc` finalizer would run with the budget hook off: setmetatable refuses one.
    #[test]
    fn setmetatable_refuses_gc() {
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        let e = vm.lua.load("setmetatable({}, {__gc = function() while true do end end})").exec().unwrap_err();
        assert!(e.to_string().contains("__gc"), "{e}");
        vm.lua.load("assert(getmetatable(setmetatable({}, {__index = {}})))").exec().unwrap();
    }

    /// require and the mod's own files stay in its folder: no leading dot, no link below the folder.
    #[cfg(unix)]
    #[test]
    fn mod_files_stay_in_the_folder() {
        let base = std::env::temp_dir().join(format!("vana-lua-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, out) = (base.join("mod"), base.join("out"));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(dir.join("lib/ok.lua"), "return 1").unwrap();
        std::fs::write(out.join("evil.lua"), "return 2").unwrap();
        std::os::unix::fs::symlink(&out, dir.join("linked")).unwrap();
        let vm = Vm::new(manifest::Manifest::default(), dir.clone(), vm::Source::Dir(dir.clone())).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        assert_eq!(vm.lua.load("return require('lib.ok')").eval::<i64>().unwrap(), 1);
        for code in ["require('.tmp.evil')", "require('linked.evil')", "require('lib\\..\\x')"] {
            assert!(vm.lua.load(code).exec().is_err(), "{code} was allowed");
        }
        assert!(vm.src.read_bytes("linked/evil.lua").is_err());
        assert!(vm.src.read_bytes("lib/../../out/evil.lua").is_err());
        assert!(!vm.src.exists("linked/evil.lua"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Mods page's on/off names a mod by its folder, which need not be its manifest id: either finds it.
    #[test]
    fn mods_named_by_folder_or_id() {
        let base = std::env::temp_dir().join(format!("vana-lua-named-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("my-folder")).unwrap();
        std::fs::write(base.join("my-folder/mod.toml"), "id = \"the-id\"\n").unwrap();
        assert_eq!(folder_of(&base, "my-folder"), base.join("my-folder"));
        assert_eq!(folder_of(&base, "the-id"), base.join("my-folder"));
        let mut host = Host::default();
        let dir = base.join("my-folder");
        let m = manifest::Manifest { id: "the-id".into(), ..Default::default() };
        host.vms.push(Vm::new(m, dir.clone(), vm::Source::Dir(dir)).unwrap());
        assert_eq!(host.named("my-folder"), Some(0));
        assert_eq!(host.named("the-id"), Some(0));
        assert_eq!(host.named("other"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A /lua line as the host reads one, whatever its spaces, slashes or case.
    #[test]
    fn lua_lines() {
        for l in ["/lua grant x exec", " /lua grant", "//LUA run x", "\t/Lua"] {
            assert!(lua_line(l), "{l:?}");
        }
        for l in ["/luax", "/echo /lua", "lua grant", ""] {
            assert!(!lua_line(l), "{l:?}");
        }
    }

    /// Loading, unloading, reloading, hiding and showing mods from a mod's commands.run needs mods; the op says so.
    #[test]
    fn mod_control_needs_mods() {
        for l in ["//lua unload other", "/ lua UNLOAD other", "/lua l x", "///lua reload", "/lua hide"] {
            assert!(controls_mods(l), "{l:?}");
        }
        for l in ["//lua list", "/lua settings x", "lua unload x", "//luaunload x", "/echo lua unload"] {
            assert!(!controls_mods(l), "{l:?}");
        }
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let e = vm.lua.load("commands.run('//lua unload other')").exec().unwrap_err();
        assert!(e.to_string().contains("mods permission"), "{e}");
        vm.lua.load("commands.run('//lua list')").exec().unwrap();
        vm.st.borrow_mut().granted.push("mods".into());
        vm.lua.load("commands.run('//lua unload other')").exec().unwrap();
        let ops: Vec<bool> =
            vm.st.borrow().ops.iter().filter_map(|o| if let Op::Run(_, m, _) = o { Some(*m) } else { None }).collect();
        assert_eq!(ops, [false, true]);
        assert!(
            !From::Mod { mods: false, input: false }.controls_mods() && !From::Bind { mods: false }.controls_mods()
        );
        assert!(From::Player.controls_mods() && From::Mod { mods: true, input: false }.controls_mods());
    }

    /// Injected and hook-built packets carry a size field that matches their bytes, and none overflows it.
    #[test]
    fn packet_sizes_fit_the_header() {
        // A whole sub-packet with a wrong size field gets the right one; the data is padded to a word.
        let p = fit(vec![0x15, 0x02, 0, 0, 1, 2, 3]);
        assert_eq!(p.len(), 8);
        assert_eq!(u16::from_le_bytes([p[0], p[1]]), 0x015 | (2 << 9));
        assert_eq!(fit(vec![0; 508]).len(), 508);
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
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
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
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
        let namespaces: Vec<String> = g
            .pairs::<String, Value>()
            .filter_map(|p| p.ok())
            .filter(|(k, v)| matches!(v, Value::Table(_)) && !STD.contains(&k.as_str()))
            .map(|p| p.0)
            .collect();
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
                            assert!(
                                docs::FUNCS.iter().any(|f| f.ns == full && f.name == fname),
                                "{full}.{fname} is bound but not documented"
                            );
                        }
                    }
                }
            }
            for pair in t.pairs::<String, Value>() {
                let (name, v) = pair.unwrap();
                if matches!(v, Value::Function(_)) {
                    assert!(
                        docs::FUNCS.iter().any(|f| f.ns == ns && f.name == name),
                        "{ns}.{name} is bound but not documented"
                    );
                }
            }
        }
    }
}

/// A decoded s2c 0x028 for the `action` event (Windower's action table, our field names).
pub(super) fn action_json(a: &crate::net::action::Action) -> serde_json::Value {
    use serde_json::json;
    let side = |s: &Option<crate::net::action::Side>| {
        s.as_ref().map(|s| json!({"kind": s.kind, "info": s.info, "value": s.value, "message": s.message}))
    };
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
                                mode = if is_battle(mode) {
                                    super::combat::BATTLE | (m as u16 & 0xff)
                                } else {
                                    m as u16 & 0xff
                                };
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

/// A battle message as the mods' battle filters see it (filters.battle).
pub(in crate::client) struct BattleMsg {
    pub id: u16,
    /// The 0x028 action's category; none for a 0x029 / 0x02D message.
    pub category: Option<u8>,
    pub actor: u32,
    pub target: u32,
    pub value: i32,
    pub mode: u8,
}

/// What the battle filters made of a message.
pub(in crate::client) enum Verdict {
    Keep,
    Drop,
    Replace(Vec<String>),
}

impl Host {
    pub(in crate::client) fn has_battle_filters(&self) -> bool {
        self.vms.iter().any(|v| v.suspended.is_none() && !v.st.borrow().battle_filters.is_empty())
    }
}

/// The mods' battle filters in load order, with game state: each sees the lines so far and may drop the message,
/// replace its lines, or set its names' colour codes (written back to `actor` / `target`).
pub(in crate::client) fn filter_battle(
    w: &mut World,
    m: &BattleMsg,
    actor: &mut Option<Who>,
    target: &mut Option<Who>,
    mut lines: Vec<String>,
) -> Verdict {
    let host = std::mem::take(&mut w.lua);
    let mut verdict = Verdict::Keep;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let funcs: Vec<mlua::Function> =
            vm.st.borrow().battle_filters.iter().filter_map(|(_, k)| vm.lua.registry_value(k).ok()).collect();
        if funcs.is_empty() {
            continue;
        }
        lend(vm, w, |vm| {
            for f in funcs {
                let r = (|| -> mlua::Result<()> {
                    let lua = &vm.lua;
                    let t = lua.create_table()?;
                    t.set("id", m.id)?;
                    t.set("category", m.category)?;
                    t.set("value", m.value)?;
                    t.set("mode", m.mode)?;
                    t.set("lines", lua.create_sequence_from(lines.iter().map(String::as_str))?)?;
                    // Each side by id; its name, flags and colour where it is known.
                    for (key, id, x) in [("actor", m.actor, &*actor), ("target", m.target, &*target)] {
                        if id == 0 {
                            continue;
                        }
                        let p = lua.create_table()?;
                        p.set("id", id)?;
                        if let Some(x) = x {
                            p.set("name", x.name.as_str())?;
                            p.set("pc", x.pc)?;
                            p.set("named", x.named)?;
                            p.set("colour", x.colour)?;
                        }
                        t.set(key, p)?;
                    }
                    vm.st.borrow_mut().deadline = Some(std::time::Instant::now() + vm::FILTER_CAP);
                    let r = f.call::<mlua::Value>(&t);
                    vm.st.borrow_mut().deadline = None;
                    let r = r?;
                    for (key, x) in [("actor", &mut *actor), ("target", &mut *target)] {
                        if let (Some(x), Some(p)) = (x.as_mut(), t.get::<Option<mlua::Table>>(key)?) {
                            x.colour = p.get::<Option<u8>>("colour")?.unwrap_or(0);
                        }
                    }
                    match r {
                        mlua::Value::Boolean(false) => verdict = Verdict::Drop,
                        mlua::Value::String(s) => lines = vec![s.to_string_lossy()],
                        mlua::Value::Table(l) => lines = l.sequence_values::<String>().collect::<mlua::Result<_>>()?,
                        _ => return Ok(()),
                    }
                    if !matches!(verdict, Verdict::Drop) {
                        verdict = Verdict::Replace(lines.clone());
                    }
                    Ok(())
                })();
                if let Err(e) = r {
                    vm.error(&e);
                }
                if matches!(verdict, Verdict::Drop) {
                    return;
                }
            }
        });
        if matches!(verdict, Verdict::Drop) {
            break;
        }
    }
    w.lua = host;
    verdict
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
        let funcs: Vec<mlua::Function> =
            vm.st.borrow().typed_filters.iter().filter_map(|(_, k)| vm.lua.registry_value(k).ok()).collect();
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

/// The training regime the client kept before the bundled Training Regime mod (its file per server and character)
/// goes into the mod's character storage, once ("migrated" marks it).
fn migrate_regime(w: &World, data: &std::path::Path, char_file: &str) {
    let file = data.join(char_file);
    let mut v = read_json(&file);
    if v.get("regime").is_some() || v.get("migrated").is_some() || w.name.is_empty() {
        return;
    }
    let server = format!("{}-{}", w.search_addr.0, w.search_addr.1);
    let read =
        |p: &std::path::Path| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let kept = read(&crate::config::regime_path(&server, &w.name))
        .or_else(|| crate::config::regime_moved_path(&server, &w.name).and_then(|p| read(&p)))
        .or_else(|| {
            let mut old: Vec<_> = crate::config::regime_legacy_paths(&w.name)
                .into_iter()
                .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
                .collect();
            old.sort_by(|a, b| b.0.cmp(&a.0));
            old.into_iter().find_map(|(_, p)| read(&p))
        });
    if !v.is_object() {
        v = serde_json::json!({});
    }
    if let Some(k) = kept.filter(|k| !k.is_null()) {
        log::info!("lua mod regime: the client's kept regime for {}: {k}", w.name);
        v["regime"] = k;
    }
    v["migrated"] = true.into();
    let _ = std::fs::create_dir_all(data);
    if let Err(e) = std::fs::write(&file, serde_json::to_vec_pretty(&v).unwrap_or_default()) {
        log::warn!("lua mod regime: {}: {e}", file.display());
    }
}

/// Settings declared `from` a client config field (a bundled mod's, replacing an extra that kept its value there) and
/// not set yet take that field's value, written to their scope's file so the config may drop it. A character-scoped
/// one waits for a character (it would go to the account file, which every character falls back to).
fn migrate_settings(m: &manifest::Manifest, data: &std::path::Path, char_file: Option<&str>) {
    if m.settings.values().all(|d| d.from.is_empty()) {
        return;
    }
    let config = read_json(&crate::config::config_path());
    let file = |character: bool| match char_file {
        Some(c) if character => data.join(format!("settings-{c}")),
        _ => data.join("settings.json"),
    };
    for (key, character, old) in migrations(m, &config, |key, character| read_json(&file(character)).get(key).is_some())
        .into_iter()
        .filter(|(_, character, _)| !character || char_file.is_some())
    {
        let file = file(character);
        let mut v = read_json(&file);
        if !v.is_object() {
            v = serde_json::json!({});
        }
        log::info!("lua mod {}: setting {key} from the config ({old})", m.id);
        v[key.as_str()] = old;
        let _ = std::fs::create_dir_all(data);
        if let Err(e) = std::fs::write(&file, serde_json::to_vec_pretty(&v).unwrap_or_default()) {
            log::warn!("lua mod {}: saving {}: {e}", m.id, file.display());
        }
    }
}

/// The settings declared `from` a config field `config` holds that are not saved yet (`saved(key, character scope)`):
/// (key, character scope, the field's value).
fn migrations(
    m: &manifest::Manifest,
    config: &serde_json::Value,
    saved: impl Fn(&str, bool) -> bool,
) -> Vec<(String, bool, serde_json::Value)> {
    m.settings
        .iter()
        .filter(|(_, d)| !d.from.is_empty())
        .filter_map(|(key, d)| {
            let character = d.scope == "character";
            let old = config.get(&d.from)?;
            (!saved(key, character)).then(|| (key.clone(), character, old.clone()))
        })
        .collect()
}

/// A mod's settings: the account file's keys, then the character file's for character-scoped ones.
/// A mod's settings page: its settings as rows (the "on" switch left out: it is the Mods page's toggle) and its
/// permissions, each allowed or not; none when it has neither.
fn mod_page(
    m: &manifest::Manifest,
    settings: &serde_json::Map<String, serde_json::Value>,
    granted: &[String],
    bundled: bool,
) -> Option<super::settings::ModPage> {
    use super::settings::{ModPage, ModRow};
    let mut rows = Vec::new();
    for (key, d) in &m.settings {
        if key == "on" && d.kind == "bool" {
            continue;
        }
        let current = settings
            .get(key)
            .cloned()
            .or_else(|| d.default.as_ref().and_then(|v| serde_json::to_value(v).ok()))
            .unwrap_or(serde_json::Value::Null);
        let choices: Vec<(String, serde_json::Value)> = match d.kind.as_str() {
            // Labelled values (any type).
            _ if !d.values.is_empty() => d
                .options
                .iter()
                .cloned()
                .zip(d.values.iter().map(|v| serde_json::to_value(v).unwrap_or_default()))
                .collect(),
            "bool" => vec![("ON".into(), true.into()), ("OFF".into(), false.into())],
            "enum" => d.options.iter().map(|o| (o.clone(), o.as_str().into())).collect(),
            "int" | "number" => {
                let (min, max) = (d.min.unwrap_or(0.0), d.max.unwrap_or(100.0));
                let step = d.step.unwrap_or(if d.kind == "int" { 1.0 } else { (max - min) / 20.0 }).max(f64::EPSILON);
                let n = (((max - min) / step).floor() as usize).min(200);
                (0..=n)
                    .map(|i| {
                        let v = min + step * i as f64;
                        if d.kind == "int" {
                            (format!("{}", v.round() as i64), (v.round() as i64).into())
                        } else {
                            (format!("{v}"), v.into())
                        }
                    })
                    .collect()
            }
            _ => continue,
        };
        let at = choices
            .iter()
            .position(|c| {
                c.1 == current || c.1.as_f64().zip(current.as_f64()).is_some_and(|(a, b)| (a - b).abs() < 1e-6)
            })
            .unwrap_or(0);
        let label = if d.label.is_empty() { key.clone() } else { d.label.clone() };
        rows.push(ModRow {
            key: key.clone(),
            label,
            help: d.help.clone(),
            choices,
            at,
            tab: String::new(),
            place: None,
            place_modern: None,
            preview: d.preview_sound,
        });
    }
    // The permissions it asks for (docs/lua-mods.md 3.5); a bundled mod's are given.
    for p in m.permissions.iter().filter(|_| !bundled) {
        let (label, help) = permission_text(p);
        let on = granted.contains(p);
        let choices = vec![("ON".into(), true.into()), ("OFF".into(), false.into())];
        rows.push(ModRow {
            key: format!("{PERM}{p}"),
            label: label.into(),
            help: help.into(),
            choices,
            at: if on { 0 } else { 1 },
            tab: String::new(),
            place: None,
            place_modern: None,
            preview: None,
        });
    }
    (!rows.is_empty()).then(|| ModPage { id: m.id.clone(), name: m.name.clone(), rows })
}

/// Gives an installed mod every permission its manifest asks for (a standard mod installed from our list).
pub(in crate::client) fn grant_manifest(dir: &std::path::Path) {
    let Ok(m) = manifest::read(dir) else { return };
    if m.permissions.is_empty() {
        return;
    }
    let mut g = grants();
    let list: Vec<serde_json::Value> = m
        .permissions
        .iter()
        .filter(|p| manifest::PERMISSIONS.contains(&p.as_str()))
        .map(|p| p.as_str().into())
        .collect();
    g.insert(m.id.clone(), list.into());
    save_grants(&g);
}

/// The Mods page's toggle as a setting change's key: its value turns the mod on (loads it) or off (unloads it).
pub(in crate::client) const ENABLED: &str = "@enabled";

/// A mod's "on" switch is its enable toggle on the Mods page: a loaded mod is on.
fn force_on(m: &manifest::Manifest, settings: &mut serde_json::Map<String, serde_json::Value>) {
    if m.settings.get("on").is_some_and(|d| d.kind == "bool") {
        settings.insert("on".into(), true.into());
    }
}

fn load_settings(
    m: &manifest::Manifest,
    data: &std::path::Path,
    char_file: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
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
    /// Every installed mod's settings page (Settings > Mods, its Settings button): its settings but the "on" switch (the
    /// page's toggle), and the permissions it asks for. A mod that is off reads its settings from its files.
    pub(super) fn settings_pages(&self) -> Vec<super::settings::ModPage> {
        let mut pages = Vec::new();
        for vm in &self.vms {
            let st = vm.st.borrow();
            let bundled = matches!(vm.src, vm::Source::Bundled(_));
            pages.extend(mod_page(&vm.manifest, &st.settings, &st.granted, bundled));
        }
        let char_file = self.vms.first().and_then(|v| v.st.borrow().char_file.clone());
        let g = grants();
        for e in std::fs::read_dir(mods_dir()).into_iter().flatten().flatten() {
            let Ok(m) = manifest::read(&e.path()) else { continue };
            if self.vms.iter().any(|v| v.manifest.id == m.id) {
                continue;
            }
            // A setting declared `from` the config shows the player's value before the mod first loads too.
            migrate_settings(&m, &data_dir().join(&m.id), char_file.as_deref());
            let settings = load_settings(&m, &data_dir().join(&m.id), char_file.as_deref());
            let given: Vec<String> = g
                .get(&m.id)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            pages.extend(mod_page(&m, &settings, &given, false));
        }
        pages
    }

    /// The loaded mod `id` names: the Mods page names a mod by its folder, the rest by its manifest id, and the two
    /// may differ (a manifest's id is its own); the folder first.
    fn named(&self, id: &str) -> Option<usize> {
        let by_folder = self.vms.iter().position(|v| v.dir.file_name().is_some_and(|f| f == id));
        by_folder.or_else(|| self.vms.iter().position(|v| v.manifest.id == id))
    }

    /// A setting changed on its Extras page: kept, written to its scope's file, and `settings_changed` sent.
    pub(super) fn apply_setting(&mut self, w: &World, id: &str, key: &str, value: serde_json::Value) {
        if key == ENABLED {
            let k = self.named(id);
            match (value.as_bool() == Some(true), k) {
                (true, None) => {
                    if let Err(e) = self.load(Some(w), folder_of(&mods_dir(), id)) {
                        log::warn!("lua mod {id}: {e}");
                    }
                }
                (false, Some(k)) => self.unload(Some(w), k),
                _ => {}
            }
            return;
        }
        if let Some(p) = key.strip_prefix(PERM) {
            match self.vms.iter().position(|v| v.manifest.id == id) {
                Some(k) => self.set_grant(k, p, value.as_bool() == Some(true)),
                None => {
                    grant_one(id, p, value.as_bool() == Some(true));
                }
            }
            return;
        }
        let Some(vm) = self.vms.iter().find(|v| v.manifest.id == id) else {
            // A mod that is off: its settings file only.
            if let Ok(m) = manifest::read(&mods_dir().join(id)) {
                let scope = m.settings.get(key).map_or("account", |d| d.scope.as_str());
                let dir = data_dir().join(id);
                let file = match Self::char_file(Some(w)) {
                    Some(c) if scope == "character" => dir.join(format!("settings-{c}")),
                    _ => dir.join("settings.json"),
                };
                let mut v = read_json(&file);
                if !v.is_object() {
                    v = serde_json::json!({});
                }
                v[key] = value;
                let _ = std::fs::create_dir_all(&dir);
                if let Err(e) = std::fs::write(&file, serde_json::to_vec_pretty(&v).unwrap_or_default()) {
                    log::warn!("lua mod {id}: saving {}: {e}", file.display());
                }
            }
            return;
        };
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
        !self.all_hidden
            && self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().windows.iter().any(|w| w.open && w.focus))
    }

    /// The loaded mods' ids.
    pub(super) fn mod_ids(&self) -> std::collections::HashSet<String> {
        self.vms.iter().map(|v| v.manifest.id.clone()).collect()
    }

    /// The open modal window (vm, id), the last opened when there are several.
    pub(super) fn modal(&self) -> Option<(usize, String)> {
        self.vms.iter().enumerate().filter(|(_, v)| v.suspended.is_none()).find_map(|(k, v)| {
            v.st.borrow().windows.iter().rev().find(|w| w.open && w.modal).map(|w| (k, w.id.clone()))
        })
    }

    /// Whether a mod window's line is being typed (it takes the text and the keys).
    pub(super) fn typing(&self) -> bool {
        !self.all_hidden
            && self.vms.iter().any(|v| {
                v.suspended.is_none() && v.st.borrow().windows.iter().any(|w| w.open && w.focus && w.takes_text())
            })
    }

    /// The focused window's title and the help under its cursor, for the classic help bar.
    pub(super) fn focused_help(&self) -> Option<(String, String)> {
        self.vms.iter().filter(|v| v.suspended.is_none()).find_map(|v| {
            v.st.borrow()
                .windows
                .iter()
                .find(|w| w.open && w.focus)
                .map(|w| (w.title.clone(), w.help_now().to_string()))
        })
    }
}

/// Typed text for the focused window's line.
pub(super) fn window_text(w: &mut World, events: &[egui::Event]) {
    let host = &mut w.lua;
    let last = host.input.borrow().last_down;
    let mut hit = None;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let mut st = vm.st.borrow_mut();
        if let Some(win) = st.windows.iter_mut().find(|w| w.open && w.focus && w.takes_text()) {
            let (ev, se) = if win.listening() {
                win.listen(events, |e| input::combo_of(e, last))
            } else if win.typing() {
                win.text(events)
            } else {
                win.filter_text(events)
            };
            hit = Some((vm.manifest.id.clone(), win.id.clone(), ev, se));
            break;
        }
    }
    let Some((id, win, ev, se)) = hit else { return };
    host.ui_events.extend(ev.into_iter().map(|e| (id.clone(), win.clone(), e)));
    w.sounds.extend(se.into_iter().map(|s| s as u32));
}

/// A key for the focused mod window, if any (its events go to the mod's handler next phase).
pub(super) fn window_key(w: &mut World, key: egui::Key, shift: bool) -> bool {
    let host = &mut w.lua;
    let mut hit = None;
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let mut st = vm.st.borrow_mut();
        if let Some(win) = st.windows.iter_mut().find(|w| w.open && w.focus) {
            let (ev, se) = win.key_mod(key, shift);
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
            // Its opacity (per-window opacity), over the whole window.
            let o = w.lua.window_opacity(&w.window_opacity, &format!("mod:{}:{id}", w.lua.vms[v].manifest.id));
            gu.opacity((o < 1.0).then_some(o));
            out.push(win.draw_classic(gu, Some(w), x, y));
            gu.opacity(None);
        }
    }
    out
}

impl Host {
    /// The open windows (mod id, a copy of the window), for the modern UI's panels.
    /// What /uimodes lists of the mods: each running mod's HUD, edge and panel windows (not those the player opens) by
    /// panel key, and each of its HUD windows as hud:<mod>:<window>, with a label.
    pub(in crate::client) fn mode_windows(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            let st = vm.st.borrow();
            let name = &vm.manifest.name;
            // Only what stands on the screen as the HUD does (HUD and edge windows, built-in panels' stand-ins): windows
            // the player opens (a mod's manager) are never hidden by a mode.
            for win in st.windows.iter().filter(|w| !w.modal && (w.hud || w.edge || w.panel.is_some())) {
                let key = win.panel.clone().unwrap_or_else(|| format!("mod:{}:{}", vm.manifest.id, win.id));
                let what = [&win.title, &win.heading]
                    .into_iter()
                    .find(|t| !t.is_empty())
                    .cloned()
                    .unwrap_or_else(|| win.id.clone());
                let label = if what.eq_ignore_ascii_case(name) || win.panel.is_some() {
                    name.clone()
                } else {
                    format!("{name}: {what}")
                };
                if !out.iter().any(|o| o.0 == key) {
                    out.push((key, label));
                }
            }
            let mut groups: Vec<&str> = st.hud.iter().map(|e| e.window.as_str()).collect();
            groups.sort_unstable();
            groups.dedup();
            for g in groups {
                let label = if g == "hud" { format!("{name} (on screen)") } else { format!("{name}: {g}") };
                out.push((format!("hud:{}:{g}", vm.manifest.id), label));
            }
        }
        out
    }

    /// The open windows `keep` takes (copied: the caller draws them while it changes the world).
    pub(super) fn open_windows_where(&self, keep: impl Fn(&window::Window) -> bool) -> Vec<(String, window::Window)> {
        let mut out = Vec::new();
        if self.all_hidden {
            return out;
        }
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for win in vm.st.borrow().windows.iter().filter(|w| w.open && keep(w)) {
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
    // A `targets` row picks its entity, as the target list does.
    if let super::modern::ModClick::Pick(id) = click {
        super::target::pick(w, Some(id));
        return;
    }
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
                out = Some(click.apply(win));
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
        // A panel's edge window is the Timers' dock's (regime.rs).
        for win in vm.st.borrow().windows.iter().filter(|w| w.open && w.panel.is_none()) {
            let (ww, wh) = win.classic_size();
            match (win.anchor, win.place) {
                (Some((ax, ay)), _) => modal.push((
                    v,
                    win.id.clone(),
                    (ax * uk.0).min(sw - ww).max(0.0),
                    (ay * uk.1).min(sh - wh).max(0.0),
                )),
                (None, Some((x, y))) if win.modal => modal.push((v, win.id.clone(), x, y)),
                (None, None) if win.modal => {
                    modal.push((v, win.id.clone(), ((sw - ww) / 2.0).max(0.0), ((sh - wh) / 2.0).max(0.0)))
                }
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
pub(super) fn pointer_classic(
    w: &mut World,
    gu: &super::game_ui::GameUi,
    p: (f32, f32),
    moved: bool,
    clicked: bool,
    right: bool,
    held: (bool, bool, bool),
) -> bool {
    let mut places = classic_places(w);
    // A canvas the pointer has left (its window not under it now): its 'leave'.
    let mut left = Vec::new();
    for (v, id, x, y) in &places {
        let vm = &w.lua.vms[*v];
        let mut st = vm.st.borrow_mut();
        if let Some(win) = st.windows.iter_mut().find(|x| x.id == *id && x.hovered.is_some()) {
            let (ww, wh) = win.classic_size();
            if p.0 < *x || p.1 < *y || p.0 >= x + ww || p.1 >= y + wh {
                left.extend(win.hover_off().into_iter().map(|e| (vm.manifest.id.clone(), id.clone(), e)));
            }
        }
    }
    w.lua.ui_events.extend(left);
    // An open modal window is the only one the pointer reaches; a click outside a popup closes it.
    if let Some((k, mid)) = w.lua.modal() {
        places.retain(|(v, id, _, _)| *v == k && *id == mid);
        let inside = places.first().is_some_and(|(v, id, x, y)| {
            let (ww, wh) =
                w.lua.vms[*v].st.borrow().windows.iter().find(|x| x.id == *id).map_or((0.0, 0.0), |x| x.classic_size());
            p.0 >= *x && p.1 >= *y && p.0 < x + ww && p.1 < y + wh
        });
        if (clicked || right) && !inside {
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
            let (hover, ev, se) = win.pointer_classic(gu, x, y, p, moved, clicked, right, held);
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
                format!(
                    "lua {}  {:.2} ms avg  {:.1} peak  {} slow  {} KB  {} errors{state}",
                    v.manifest.id,
                    st.stats.avg,
                    st.stats.peak,
                    st.stats.slow,
                    v.lua.used_memory() / 1024,
                    st.errors.len()
                )
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
            // A Windows checkout (core.autocrlf) has CRLF lines.
            text += &std::fs::read_to_string(e.path()).unwrap().replace("\r\n", "\n");
        }
        for f in super::docs::FUNCS {
            let full = if f.ns.is_empty() { f.name.to_string() } else { format!("{}.{}", f.ns, f.name) };
            assert!(text.contains(&format!("\n## `{full}`\n")), "docs/modding/reference has no section for {full}");
        }
        for e in super::docs::EVENTS {
            assert!(
                text.contains(&format!("\n## `{}`\n", e.name)),
                "docs/modding/reference/events.md has no section for {}",
                e.name
            );
        }
    }

    /// The stubs in assets/mods/library are what `lua-stubs` writes now (regenerate them after an API change:
    /// `vana-client lua-stubs --out crates/vana-client/assets/mods/library/vana.lua`).
    #[test]
    fn committed_stubs_current() {
        assert_eq!(
            include_str!("../../../assets/mods/library/vana.lua"),
            super::stubs(),
            "assets/mods/library/vana.lua is stale"
        );
    }
}

/// The bundled mods' settings on our Extras tabs: (label, tab, place).
#[cfg(test)]
/// A bundled mod's manifest (tests).
#[cfg(test)]
fn bundled_manifest(id: &str) -> Option<manifest::Manifest> {
    BUNDLED.iter().find(|b| b.0 == id).and_then(|b| manifest::parse(b.1, id).ok())
}

/// Every mod installed (the mods folder): (id, name, version, enabled in the config, bundled: always false now).
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
        "actions" => {
            ("Allow game actions", "Casting, items, equipment and the like when you use its commands or windows.")
        }
        "automation" => {
            ("Allow automation", "Game actions on its own: casting, targeting, moving, trading, answering NPCs.")
        }
        "packets.read" => ("Allow reading packets", "Sees every packet the server sends and the client sends."),
        "packets.write" => ("Allow changing packets", "Changes, blocks and sends packets of its own."),
        "input.capture" => ("Allow taking input", "Takes keys and the mouse from the game and presses keys."),
        "files.shared" => {
            ("Allow shared files", "Reads and writes files outside its own folder, in the shared mod data.")
        }
        "net" => ("Allow network", "Connects to the internet (web requests, sockets)."),
        "exec" => ("Allow running programs", "Starts programs on this computer."),
        "clipboard" => ("Allow reading the clipboard", "Reads the text you copied."),
        "multibox" => ("Allow multibox", "Talks to your other game windows on this computer."),
        "ui.native" => (
            "Allow game windows",
            "Opens, hides, moves, scales and reskins the game's own windows and panels, and drives them.",
        ),
        "process" => ("Allow process settings", "Sets which CPU cores the game runs on, and its priority."),
        "devices" => ("Allow devices", "Talks to USB devices directly (G-keys, Stream Decks and the like)."),
        "mods" => ("Allow controlling mods", "Loads, unloads, reloads, hides and shows your other mods."),
        _ => ("Allow", ""),
    }
}

impl Host {
    /// Allows or withdraws permission `p` (one the mod asks for) for the mod at `k`; kept in grants.json.
    fn set_grant(&mut self, k: usize, p: &str, on: bool) {
        let id = self.vms[k].manifest.id.clone();
        let list = grant_one(&id, p, on);
        let asked = self.vms[k].manifest.permissions.clone();
        self.vms[k].st.borrow_mut().granted = asked.into_iter().filter(|p| list.contains(p)).collect();
    }
}

/// Mod `id`'s permission `p` allowed or not in grants.json; its list of grants after.
fn grant_one(id: &str, p: &str, on: bool) -> Vec<String> {
    let mut g = grants();
    let mut list: Vec<String> = g
        .get(id)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    list.retain(|x| x != p);
    if on && manifest::PERMISSIONS.contains(&p) {
        list.push(p.to_string());
    }
    g.insert(id.to_string(), list.clone().into());
    save_grants(&g);
    list
}

fn save_grants(g: &serde_json::Map<String, serde_json::Value>) {
    let _ = std::fs::create_dir_all(data_dir());
    if let Err(e) = std::fs::write(data_dir().join("grants.json"), serde_json::to_vec_pretty(g).unwrap_or_default()) {
        log::warn!("lua mods: saving grants: {e}");
    }
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
    let original = data.clone();
    // A hook may change the id: the later hooks are matched against the packet as it is then.
    let mut id = packet_id(&data);
    let (mut cur, mut blocked) = (data, false);
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        let funcs: Vec<(Option<u16>, mlua::Function)> = {
            let st = vm.st.borrow();
            let hooks = if outgoing { &st.hooks_out } else { &st.hooks_in };
            hooks.iter().filter_map(|h| vm.lua.registry_value(&h.2).ok().map(|f| (h.1, f))).collect()
        };
        if funcs.is_empty() {
            continue;
        }
        let write = vm.st.borrow().granted.iter().any(|g| g == "packets.write");
        lend(vm, w, |vm| {
            for (want, f) in funcs {
                if want.is_some_and(|i| i != id) {
                    continue;
                }
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
                        vm.error(&mlua::Error::runtime(
                            "changing or blocking packets needs packets.write (/lua grant)",
                        ));
                    }
                    Ok(mlua::Value::Boolean(false)) => blocked = true,
                    Ok(mlua::Value::String(s)) => {
                        let b = s.as_bytes().to_vec();
                        if b.len() > 508 {
                            vm.error(&mlua::Error::runtime(format!(
                                "a packet is at most 508 bytes ({} returned): kept as it was",
                                b.len()
                            )));
                        } else if b.len() >= 4 {
                            cur = fit(b);
                            id = packet_id(&cur);
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
pub(super) fn packets_in(
    w: &mut World,
    packets: Vec<crate::net::map::session::SubPacket>,
) -> Vec<crate::net::map::session::SubPacket> {
    let host = std::mem::take(&mut w.lua);
    let mut out = Vec::new();
    let injected: Vec<Vec<u8>> =
        host.vms.iter().flat_map(|v| std::mem::take(&mut v.st.borrow_mut().inject_in)).collect();
    let all = packets.into_iter().map(|p| (p.sync, p.data, false)).chain(injected.into_iter().map(|d| (0, d, true)));
    for (sync, data, inj) in all {
        w.net.stats.keep_last(false, &data);
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
    let injected: Vec<Vec<u8>> =
        host.vms.iter().flat_map(|v| std::mem::take(&mut v.st.borrow_mut().inject_out)).collect();
    let mut out = Vec::new();
    for (data, inj) in packets.into_iter().map(|d| (d, false)).chain(injected.into_iter().map(|d| (d, true))) {
        w.net.stats.keep_last(true, &data);
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
        let last =
            self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().granted.iter().any(|g| g == "packets.read"));
        let flags = if fin { super::net::HOOK_IN } else { 0 }
            | if fout { super::net::HOOK_OUT } else { 0 }
            | if last { super::net::HOOK_LAST } else { 0 };
        w.net.hooks.store(flags, std::sync::atomic::Ordering::Relaxed);
    }
}
