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

/// A mod window's canvas (element `i`, `id`) as last drawn, in window pixels: its origin, the part of it shown (min x,
/// min y, max x, max y), the pixels per point (a canvas's units are points), whether it was on top under the
/// pointer, and when.
struct CanvasAt {
    m: String,
    win: String,
    i: usize,
    id: String,
    origin: [f32; 2],
    shown: [f32; 4],
    ppp: f32,
    top: bool,
    at: std::time::Instant,
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

/// The mods of this session.
#[derive(Default)]
pub struct Host {
    vms: Vec<Vm>,
    started: bool,
    /// `/lua hide`: every mod's HUD, windows and world drawing hidden (they keep running); shared with a parked stand-in.
    all_hidden: std::rc::Rc<std::cell::Cell<bool>>,
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
    /// `/lua mods` (and `/lua settings` in the modern UI): the Mods window, on this mod's Settings when one is named.
    open_mods: Option<Option<String>>,
    /// The lines the mods printed lately, numbered (the Mods window shows a mod's since its Run), and the next number.
    printed: std::collections::VecDeque<(String, String, u64)>,
    print_seq: u64,
    /// Window events for the mods' handlers: (mod, window, event).
    ui_events: Vec<(String, String, window::Event)>,
    /// Events for one mod (its settings changed), delivered in the mod phase: (mod, name, arguments).
    mod_events: Vec<(String, &'static str, Vec<serde_json::Value>)>,
    /// Mods turned on outside the mod phase (the Mods window, a character or job switch): their folders, loaded in the
    /// next mod phase, where their entry file and `load` can read the game UI's data as at the start.
    pending_loads: Vec<PathBuf>,
    /// Where the mods' canvases were last drawn (the modern UI), for a HUD drag let go over one.
    canvases: Vec<CanvasAt>,
    /// The canvas a HUD drag is over now: (mod, window, canvas, the dragged element).
    drag_over: Option<(String, String, String, String)>,
    /// A stand-in left in `World::lua` while the host is out running the mods (`Host::park`): the host's mod states, read
    /// live by the World's queries (focus, typing, modal) during the call.
    parked: Vec<std::rc::Rc<std::cell::RefCell<vm::ModState>>>,
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
    /// A mod pressed any key this frame: the client's own question (fs.open_folder) takes no keys then.
    pub(in crate::client) synth_keys: bool,
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
    /// A mod's recording asked for: (the mod, its file and frames a second, or None to stop its recording).
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

/// The built-in edge panels a mod's edge window may stand in for (ui.window's `panel`).
pub(in crate::client) const EDGE_PANELS: [&str; 6] =
    ["regime", "timers_recasts", "timers_buffs", "timers_debuffs", "skillchains", "minimap"];

/// The built-in modern panels a mod's ordinary window may stand in for (ui.window's `panel`): its place and docking,
/// /uimodes and hiding; not drawn in the classic UI.
pub(in crate::client) const STAND_IN_PANELS: [&str; 2] = ["focus", "pending"];

/// The timers' sections' panels, in section order.
pub(in crate::client) const TIMERS_PANELS: [&str; 3] = ["timers_recasts", "timers_buffs", "timers_debuffs"];

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

/// The mods' folders; a test run's (`mods` beside its FFXI_CONFIG copy) when it has one, so test mods never sit
/// among the user's.
fn mods_dir() -> PathBuf {
    crate::config::mods_dir()
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

    /// The running mods' text commands by name, each with its mod's name and its `[commands]` usage and help (none
    /// declared: one bare form), then //mods and /lua; a name two mods register is the first loaded's.
    pub fn command_list(&self) -> Vec<CmdInfo> {
        let mut out: Vec<CmdInfo> = Vec::new();
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for (name, _) in &vm.st.borrow().commands {
                if out.iter().any(|c| c.name == *name) {
                    continue;
                }
                let decl = vm.manifest.commands.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|d| d.1);
                out.push(CmdInfo {
                    name: name.clone(),
                    id: vm.manifest.id.clone(),
                    owner: vm.manifest.name.clone(),
                    help: decl.map_or(String::new(), |d| d.help.clone()),
                    forms: decl.map_or_else(|| vec![(String::new(), String::new())], |d| d.forms()),
                });
            }
        }
        out.push(CmdInfo {
            name: "mods".into(),
            id: String::new(),
            owner: "Mods".into(),
            help: "The mods that have commands (as /lua help).".into(),
            forms: help_forms(),
        });
        out.push(CmdInfo {
            name: "lua".into(),
            id: String::new(),
            owner: "Mods".into(),
            help: "Loads, unloads and lists mods, grants their permissions, and shows their commands.".into(),
            forms: [
                ("help [mod or command]", "The mods' commands: a mod's, or one command's forms."),
                ("list", "The mods, loaded or not."),
                ("load <id>", "Loads a mod."),
                ("unload <id>", "Unloads a mod."),
                ("reload [id]", "Loads a mod (or every mod) again."),
                ("grant <id> <permission>", "Lets a mod do what it asks."),
                ("settings <id>", "Opens a mod's settings."),
            ]
            .map(|(a, h)| (a.to_string(), h.to_string()))
            .to_vec(),
        });
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// An installed mod that is not running and declares the command `name`: its name.
    fn off_mod_with(&self, name: &str) -> Option<String> {
        let running = |id: &str| self.vms.iter().any(|v| v.suspended.is_none() && v.manifest.id == id);
        std::fs::read_dir(mods_dir())
            .ok()?
            .flatten()
            .filter_map(|e| manifest::read(&e.path()).ok())
            .find(|m| !running(&m.id) && m.commands.keys().any(|n| n.eq_ignore_ascii_case(name)))
            .map(|m| m.name)
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

    /// The number the next printed line gets (the Mods window's Run notes it).
    pub(in crate::client) fn print_seq(&self) -> u64 {
        self.print_seq
    }

    /// The lines mod `id` printed from number `from` on, oldest first.
    pub(in crate::client) fn printed_since(&self, id: &str, from: u64) -> Vec<&str> {
        self.printed.iter().filter(|p| p.2 >= from && p.0 == id).map(|p| p.1.as_str()).collect()
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

    fn load_with(
        &mut self,
        w: Option<&World>,
        m: manifest::Manifest,
        dir: PathBuf,
        src: vm::Source,
    ) -> Result<(), String> {
        if let Some(n) = m.commands.keys().find(|n| super::textcmd::is_game_command(&n.to_ascii_lowercase())) {
            return Err(format!("{}: [commands.{n}]: /{n} is a game command; give yours another name", m.id));
        }
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
        let granted: Vec<String> = m.permissions.iter().filter(|p| given.contains(p)).cloned().collect();
        let missing: Vec<String> = m.permissions.iter().filter(|p| !granted.contains(p)).cloned().collect();
        let vm = Vm::new(m, dir, src).map_err(|e| format!("{e:#}"))?;
        vm.st.borrow_mut().char_file = Self::char_file(w);
        vm.st.borrow_mut().settings = settings;
        vm.st.borrow_mut().granted = granted;
        if !missing.is_empty() {
            let id = vm.manifest.id.clone();
            vm.st.borrow_mut().notices.push(format!(
                "{} asks for {}: allow it in its settings (/lua settings {id}) or with /lua grant {id} {}.",
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
        // A parked stand-in may still hold its state: its windows are gone with it.
        vm.st.borrow_mut().gone = true;
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
        // The list as saved: the app's copy (follow_scope's) differs only between a change and its save, and a scope
        // or switch change is saved at once.
        let cfg = crate::config::Config::load();
        let root = mods_dir();
        install_stubs(root.clone());
        let (character, job) = (super::uiconf::character(&w.world_name, &w.name), super::uiconf::job_of(w));
        self.dir.borrow_mut().at = Some((character.clone(), job.map(str::to_string)));
        for r in cfg.mods.iter().filter(|r| r.active(Some(&character), job) == Some(true)) {
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
        self.unload(w, k);
        self.load(w, dir).map_err(|e| format!("{id}: {e}"))?;
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
        self.pending_loads.clear();
    }

    /// `/lua ...`: list, load, unload, reload, run, set.
    fn lua_command(&mut self, w: &World, args: &[&str], out: &mut Vec<(String, [u8; 4])>) {
        let find = |vms: &[Vm], id: &str| vms.iter().position(|v| v.manifest.id.eq_ignore_ascii_case(id));
        match args.first().copied().unwrap_or("list") {
            "help" => help_lines(&self.command_list(), "/lua help", &args[1..], out),
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
                self.all_hidden.set(args[0] == "hide");
                out.push((
                    if self.all_hidden.get() { "Mods hidden (/lua show brings them back)." } else { "Mods shown." }
                        .into(),
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
            // The modern UI shows a mod's settings in the Mods window.
            "settings" if w.modern => {
                self.open_mods =
                    Some(args.get(1).and_then(|id| find(&self.vms, id)).map(|k| self.vms[k].manifest.id.clone()))
            }
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
            "mods" if w.modern => self.open_mods = Some(None),
            "mods" => out.push(("The Mods window is the modern UI's: Config > Settings > Mods here.".into(), ERROR)),
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
                format!(
                    "/lua {other}: list, load, unload, reload, watch, run, set, settings, mods, grant, revoke, help"
                ),
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
/// The events queued for one mod (Host.mod_events: settings_changed), run in the mod phase.
fn emit_mod_events(vm: &Vm, events: &[(String, &'static str, Vec<serde_json::Value>)]) {
    for (_, name, args) in events.iter().filter(|e| e.0 == vm.manifest.id) {
        let vals: Vec<mlua::Value> = args.iter().map(|a| api::to_lua(&vm.lua, a).unwrap_or(mlua::Value::Nil)).collect();
        vm.emit(name, mlua::MultiValue::from_iter(vals));
    }
}

pub(super) fn frame(w: &mut World, gu: Option<&super::game_ui::GameUi>) {
    if !w.lua.opt_owners.is_empty() {
        release_options(w);
    }
    let mut host = Host::park(&mut w.lua);
    host.gu = gu.map(|g| g as *const _);
    // Who binds what, for binds.users: every running mod's binds.
    let all: Vec<(String, input::Combo)> = host
        .vms
        .iter()
        .filter(|v| v.suspended.is_none())
        .flat_map(|v| v.st.borrow().binds.iter().map(|b| (v.manifest.id.clone(), b.combo)).collect::<Vec<_>>())
        .collect();
    host.info.borrow_mut().all_binds = all;
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
        // The mods turned on since the last mod phase.
        host.load_pending(Some(w), &mut out);
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
                None if name == "mods" => help_lines(&host.command_list(), "//mods", &args, &mut out),
                None => {
                    let hint = match (host.off_mod_with(&name), nearest(&host.command_list(), &name)) {
                        (Some(m), _) => format!(" {m} has it, but it is not running (Settings > Mods)."),
                        (None, Some(n)) => format!(" Did you mean //{n}? (/lua help lists them.)"),
                        (None, None) => " /lua help lists them.".to_string(),
                    };
                    out.push((format!("No mod command {name}.{hint}"), ERROR));
                }
            }
        }
        let events = std::mem::take(&mut host.events);
        let mod_events = std::mem::take(&mut host.mod_events);
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
                emit_mod_events(vm, &mod_events);
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
        for text in notices.iter().chain(vm_ops.iter().filter_map(|o| match o {
            Op::Print(t) | Op::Chat { text: t, .. } => Some(t),
            _ => None,
        })) {
            if host.printed.len() >= 64 {
                host.printed.pop_front();
            }
            host.printed.push_back((vm.manifest.id.clone(), text.clone(), host.print_seq));
            host.print_seq += 1;
        }
        out.extend(notices.into_iter().map(|n| (n, ERROR)));
        ops.extend(vm_ops);
        if save_now {
            save(vm, Some(w));
        }
    }
    host.return_focus();
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
    host.packet_upkeep(w);
    // `/lua settings` (classic): Extras on the mod's page, at once when its state is up, else once the app has made it
    // (it takes open_page then). The modern UI's is the Mods window.
    if host.open_page.is_some() {
        let extras = w.settings.as_ref().is_some_and(|s| s.kind == super::settings::Kind::Extras);
        if let Some(s) = w.settings.as_mut().filter(|_| extras) {
            if let Some(id) = host.open_page.take() {
                s.goto_mod(&id);
            }
        } else if w.settings.is_none() {
            w.want_settings = Some(super::settings::Kind::Extras);
        }
    }
    if let Some(id) = host.open_mods.take() {
        super::modern::open_mods(w, id);
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
    /// A mod's line (commands.run, actions): runs as the mod's, and as the player's input only when the mod ran
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
    if w.lua.started && !w.lua.all_hidden.get() { draw::vertices(w) } else { [Vec::new(), Vec::new()] }
}

/// The mods' HUD elements, under the windows (world_hud).
/// The HUD elements drawn over the windows (`top`), after the classic UI's windows.
pub(super) fn draw_hud_top(gu: &mut super::game_ui::GameUi, w: &World) {
    if !w.modern && w.lua.started && !w.lua.all_hidden.get() {
        hud::draw(gu, w, true);
    }
}

pub(super) fn draw_hud(gu: &mut super::game_ui::GameUi, w: &World) {
    if !w.modern {
        w.lua.set_modern_scale(None);
    }
    if w.lua.started && !w.lua.all_hidden.get() {
        draw::draw_overlay(gu, w);
        hud::draw(gu, w, false);
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

    /// The timers' sections: per section, the groups of the timers element in its panel's open window (the newest
    /// mod's) and when they were given; None while the panel is hidden or has no such window.
    pub(in crate::client) fn timers_sections(&self) -> [Option<(Vec<window::TimerGroup>, std::time::Instant)>; 3] {
        TIMERS_PANELS.map(|p| {
            if self.all_hidden.get() || self.hides(p) {
                return None;
            }
            // Read in place (edge_panel copies the window).
            self.vms.iter().rev().filter(|v| v.suspended.is_none()).find_map(|v| {
                let st = v.st.borrow();
                let win = st.windows.iter().find(|w| w.open && w.edge && w.panel.as_deref() == Some(p))?;
                win.elems.iter().find_map(|e| match e {
                    window::Elem::Timers { groups, at, .. } => Some((groups.clone(), *at)),
                    _ => None,
                })
            })
        })
    }

    /// The wheel over the map element of the open window standing in for `panel`: its `wheel` event.
    pub(in crate::client) fn map_wheel(&mut self, panel: &str, notches: f32) {
        let hit = self.vms.iter().rev().filter(|v| v.suspended.is_none()).find_map(|v| {
            let st = v.st.borrow();
            let win = st.windows.iter().find(|w| w.open && w.edge && w.panel.as_deref() == Some(panel))?;
            let id = win.elems.iter().find_map(|e| match e {
                window::Elem::Map { id, .. } => Some(id.clone()),
                _ => None,
            })?;
            Some((v.manifest.id.clone(), win.id.clone(), id))
        });
        if let Some((m, win, id)) = hit {
            self.ui_events.push((m, win, window::Event::Wheel { id, notches }));
        }
    }

    /// Whether any timers panel's window is open and shown.
    pub(in crate::client) fn has_timers(&self) -> bool {
        TIMERS_PANELS.iter().any(|p| !self.hides(p) && self.has_edge_panel(p))
    }

    /// Whether an open edge window stands in for a built-in panel (edge_panel, without copying the window).
    pub(in crate::client) fn has_edge_panel(&self, panel: &str) -> bool {
        !self.all_hidden.get()
            && self.running().any(|s| s.windows.iter().any(|w| w.open && w.edge && w.panel.as_deref() == Some(panel)))
    }

    /// The open edge window standing in for a built-in panel (the newest mod's).
    pub(in crate::client) fn edge_panel(&self, panel: &str) -> Option<window::Window> {
        if self.all_hidden.get() {
            return None;
        }
        self.vms.iter().rev().filter(|v| v.suspended.is_none()).find_map(|v| {
            v.st.borrow().windows.iter().find(|w| w.open && w.edge && w.panel.as_deref() == Some(panel)).cloned()
        })
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
        self.started && !self.all_hidden.get()
    }

    /// The actors the mods' HUD portraits show (hud.portrait), first by layer, each once (as its first element asks).
    pub(in crate::client) fn portraits_wanted(&self) -> Vec<hud::Wanted> {
        if !self.hud_shown() {
            return Vec::new();
        }
        // Read in place (every element was copied for this every frame), in the draw order elements() gives.
        let mut all: Vec<(i32, hud::Wanted)> = Vec::new();
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for e in vm.st.borrow().hud.iter().filter(|e| e.visible) {
                if let hud::Kind::Portrait { entity, w, h, look, .. } = e.kind {
                    all.push((e.layer, hud::Wanted { entity, size: [w, h], look }));
                }
            }
        }
        all.sort_by_key(|p| p.0);
        let mut out: Vec<hud::Wanted> = Vec::new();
        for (_, p) in all {
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
    pub(in crate::client) fn hud_tip(&self) -> Option<(String, String, Option<window::Icon>, f32)> {
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

/// A running mod's text command as the chat line's hint and /lua help show it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::client) struct CmdInfo {
    pub name: String,
    /// The mod's id and name ("" and "Mods" for the client's).
    pub id: String,
    pub owner: String,
    pub help: String,
    /// Its forms: the words after the name ("" for none) and a line on each.
    pub forms: Vec<(String, String)>,
}

/// //mods' forms.
fn help_forms() -> Vec<(String, String)> {
    [("", "The mods that have commands."), ("<mod or command>", "A mod's commands, or one command's forms.")]
        .map(|(a, h)| (a.to_string(), h.to_string()))
        .to_vec()
}

/// //mods (and /lua help, `cmd` being the one typed): one line naming the mods with commands by id; with a mod (its id
/// or name), its commands' forms; with a command, its forms. Each form a line: `//name words - help`.
fn help_lines(list: &[CmdInfo], cmd: &str, args: &[&str], out: &mut Vec<(String, [u8; 4])>) {
    const INFO: [u8; 4] = [0x80; 4];
    let forms = |c: &CmdInfo, out: &mut Vec<(String, [u8; 4])>| {
        for (a, h) in &c.forms {
            let line = if a.is_empty() { format!("//{}", c.name) } else { format!("//{} {a}", c.name) };
            let line = if h.is_empty() { format!("  {line}") } else { format!("  {line} - {h}") };
            out.push((line, INFO));
        }
    };
    let want = args.join(" ").trim_start_matches('/').to_ascii_lowercase();
    if want.is_empty() {
        let mut ids: Vec<&str> = list.iter().filter(|c| !c.id.is_empty()).map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        let names = if ids.is_empty() { "none".to_string() } else { ids.join(", ") };
        out.push((format!("{cmd} <mod or command>: its commands. Mods: {names}."), INFO));
        return;
    }
    // A mod (by id or name) first: a mod with a command of the same name lists them all.
    let of: Vec<&CmdInfo> =
        list.iter().filter(|c| !c.id.is_empty() && (c.owner.to_ascii_lowercase() == want || c.id == want)).collect();
    if of.is_empty()
        && let Some(c) = list.iter().find(|c| c.name == want)
    {
        let help = if c.help.is_empty() { String::new() } else { format!(": {}", c.help) };
        out.push((format!("//{} ({}){help}", c.name, c.owner), INFO));
        forms(c, out);
        return;
    }
    if of.is_empty() {
        let hint = nearest(list, &want).map_or(String::new(), |n| format!(" Did you mean {n}?"));
        out.push((format!("No mod command or mod {want}.{hint}"), ERROR));
        return;
    }
    out.push((format!("{}:", of[0].owner), INFO));
    for c in of {
        forms(c, out);
    }
}

/// The command a mistyped name most likely meant: one starting with it, else one a couple of edits away.
fn nearest(list: &[CmdInfo], name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if let Some(c) = list.iter().find(|c| c.name.starts_with(name)) {
        return Some(c.name.clone());
    }
    let dist = |a: &str, b: &str| {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut prev = row[0];
            row[0] = i;
            for j in 1..=b.len() {
                let cur = row[j];
                row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
                prev = cur;
            }
        }
        row[b.len()]
    };
    list.iter()
        .map(|c| (dist(name, &c.name), &c.name))
        .filter(|(d, _)| *d <= 2.min(name.len().saturating_sub(1)).max(1))
        .min()
        .map(|(_, n)| n.clone())
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

    /// The repository's mods (assets/mods), each a folder.
    fn repo_mods() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/mods");
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("mod.toml").is_file())
            .collect();
        dirs.sort();
        dirs
    }

    /// A repository mod's Lua files, its tests left out: (path in its folder, text).
    fn repo_mod_lua(dir: &Path) -> Vec<(String, String)> {
        fn walk(root: &Path, d: &Path, out: &mut Vec<(String, String)>) {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.is_dir() {
                    if !(d == root && p.file_name().is_some_and(|n| n == "tests")) {
                        walk(root, &p, out);
                    }
                } else if p.extension().is_some_and(|x| x == "lua") {
                    let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    out.push((rel, std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out
    }

    /// A mod of the repository loaded from its folder into a fresh VM, the API installed, as the client loads it.
    fn repo_mod_vm(dir: &Path) -> Vm {
        let manifest = manifest::read(dir).unwrap_or_else(|e| panic!("{}: {e:#}", dir.display()));
        let host = Host::default();
        let vm = Vm::new(manifest, dir.to_path_buf(), vm::Source::Dir(dir.to_path_buf())).unwrap();
        vm.lua.set_app_data(host.info.clone());
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        install_test_api(&vm.lua, dir).unwrap();
        vm
    }

    /// What a mod's tests get beyond the mod API: the TEST table. Nothing in it names a mod.
    fn install_test_api(lua: &mlua::Lua, dir: &Path) -> mlua::Result<()> {
        let t = lua.create_table()?;
        // source(path): a file of the mod's folder as text, nil when it has none.
        let d = dir.to_path_buf();
        t.set(
            "source",
            lua.create_function(move |_, rel: String| {
                Ok(vm::inside(&d, &rel).ok().and_then(|p| std::fs::read_to_string(p).ok()))
            })?,
        )?;
        // list(path): the file names in a folder of the mod's, sorted.
        let d = dir.to_path_buf();
        t.set(
            "list",
            lua.create_function(move |_, rel: String| {
                let p = vm::inside(&d, &rel).map_err(mlua::Error::external)?;
                let mut names: Vec<String> = std::fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                Ok(names)
            })?,
        )?;
        // grant(permission): as the player allowing it.
        t.set(
            "grant",
            lua.create_function(|lua, p: String| {
                api::state(lua).borrow_mut().granted.push(p);
                Ok(())
            })?,
        )?;
        // window(id): one of the mod's windows as the client holds it ({open, hud, focus, rows: its first list's
        // cells}), nil when it has none.
        t.set(
            "window",
            lua.create_function(|lua, id: String| {
                let st = api::state(lua);
                let st = st.borrow();
                let Some(w) = st.windows.iter().find(|w| w.id == id) else { return Ok(None) };
                let out = lua.create_table()?;
                out.set("open", w.open)?;
                out.set("hud", w.hud)?;
                out.set("focus", w.focus)?;
                let rows = lua.create_table()?;
                if let Some(list) = w.elems.iter().find_map(|e| match e {
                    window::Elem::List { rows, .. } => Some(rows),
                    _ => None,
                }) {
                    for r in list {
                        rows.push(r.cells.clone())?;
                    }
                }
                out.set("rows", rows)?;
                Ok(Some(out))
            })?,
        )?;
        // zone_text(zone): a zone's dialogue entries by id (raw bytes) and their count; nil without the game's files.
        t.set(
            "zone_text",
            lua.create_function(|lua, zone: u16| {
                let cfg = crate::config::Config::load();
                let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return Ok((None, None)) };
                let Ok(d) = crate::dat::dialog::Dialog::load(&index, zone) else { return Ok((None, None)) };
                let out = lua.create_table()?;
                for id in 0..d.len() as u32 {
                    if let Some(b) = d.raw(id) {
                        out.set(id, lua.create_string(&b)?)?;
                    }
                }
                Ok((Some(out), Some(d.len())))
            })?,
        )?;
        // game_items(): the game's armour and weapon records ({id, name, slots, description}); nil without its files.
        t.set(
            "game_items",
            lua.create_function(|lua, ()| {
                let cfg = crate::config::Config::load();
                let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return Ok(mlua::Value::Nil) };
                let Ok(items) = crate::dat::items::Items::load(&index, crate::dat::items::Lang::En) else {
                    return Ok(mlua::Value::Nil);
                };
                let out = lua.create_table()?;
                for id in 0x2800u32..0x7000 {
                    let Some(it) = items.peek(id).filter(|it| it.slots != 0 && !it.name.is_empty()) else { continue };
                    let rec = lua.create_table()?;
                    rec.set("id", id)?;
                    rec.set("name", it.name.as_str())?;
                    rec.set("slots", it.slots)?;
                    rec.set("description", it.description.as_str())?;
                    out.push(rec)?;
                }
                Ok(mlua::Value::Table(out))
            })?,
        )?;
        lua.globals().set("TEST", t)
    }

    /// A host with one mod "t" holding `wins`, on a 1000 x 800 screen.
    fn classic_host(wins: Vec<window::Window>) -> Host {
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        vm.st.borrow_mut().windows = wins;
        let mut host = Host::default();
        {
            let mut f = host.info.borrow_mut();
            (f.window, f.k) = ([1000.0, 800.0], (1.0, 1.0));
        }
        host.vms.push(vm);
        host
    }

    fn classic_win(id: &str, hud: bool, place: Option<(f32, f32)>) -> window::Window {
        let mut win = window::Window::new(id);
        (win.open, win.hud, win.place) = (true, hud, place);
        win
    }

    /// The classic UI draws and routes the HUD layer's windows first, then the other windows, modal and anchored
    /// ones last, whatever order the mod opened them in.
    #[test]
    fn classic_places_hud_layer_first() {
        let mut modal = classic_win("modal", false, None);
        modal.modal = true;
        let mut popup = classic_win("popup", false, None);
        popup.anchor = Some((10.0, 10.0));
        let mut hidden = classic_win("closed", true, Some((0.0, 0.0)));
        hidden.open = false;
        let host = classic_host(vec![
            modal,
            classic_win("manager", false, Some((50.0, 50.0))),
            popup,
            classic_win("edge", true, Some((0.0, 0.0))),
            hidden,
            classic_win("cascade", false, None),
        ]);
        let ids: Vec<_> = classic_places(&host).into_iter().map(|(_, id, _, _)| id).collect();
        assert_eq!(ids, ["edge", "manager", "cascade", "modal", "popup"]);
        host.all_hidden.set(true);
        assert!(classic_places(&host).is_empty());
    }

    /// Under a rect of World::over_mod_hud a HUD-layer window takes no pointer, unless a press or a drag on it
    /// already holds it; the other windows are drawn over the game's and keep it.
    #[test]
    fn pointer_places_under_game_windows() {
        let host = classic_host(vec![
            classic_win("edge", true, Some((0.0, 0.0))),
            classic_win("manager", false, Some((0.0, 0.0))),
        ]);
        let ids = |over: &[[f32; 4]], p| -> Vec<String> {
            pointer_places(&host, over, p).into_iter().map(|(_, id, _, _)| id).collect()
        };
        let over = [[0.0, 0.0, 100.0, 100.0]];
        assert_eq!(ids(&over, (50.0, 50.0)), ["manager"]);
        assert_eq!(ids(&over, (150.0, 150.0)), ["edge", "manager"]);
        assert_eq!(ids(&[], (50.0, 50.0)), ["edge", "manager"]);
        host.vms[0].st.borrow_mut().windows[0].press = Some((0, 1.0, 1.0));
        assert_eq!(ids(&over, (50.0, 50.0)), ["edge", "manager"]);
        host.vms[0].st.borrow_mut().windows[0].press = None;
        host.vms[0].st.borrow_mut().windows[0].drag = Some((0, 1.0, 1.0));
        assert_eq!(ids(&over, (50.0, 50.0)), ["edge", "manager"]);
    }

    /// A press or a drag holds the pointer only on a window pointer_classic reaches and so can end: not a suspended
    /// mod's, nor a panel's (the Timers' dock), nor any while the mods' windows are hidden.
    #[test]
    fn holds_pointer_only_routed_windows() {
        let mut win = classic_win("w", false, None);
        win.drag = Some((0, 1.0, 1.0));
        let mut host = classic_host(vec![win]);
        assert!(holds_pointer(&host));
        host.all_hidden.set(true);
        assert!(!holds_pointer(&host));
        host.all_hidden.set(false);
        host.vms[0].st.borrow_mut().windows[0].panel = Some("p".into());
        assert!(!holds_pointer(&host));
        host.vms[0].st.borrow_mut().windows[0].panel = None;
        host.vms[0].suspended = Some("errors".into());
        assert!(!holds_pointer(&host));
        host.vms[0].suspended = None;
        host.vms[0].st.borrow_mut().windows[0].open = false;
        assert!(!holds_pointer(&host));
    }

    /// The stand-in parked in `World::lua` while the host runs the mods answers the World's queries from the mods'
    /// states as they are now (the macro gate read by game.info().macros from a pad filter). A throwaway mod: the
    /// host saves its storage when it drops.
    #[test]
    fn parked_host_reads_windows_live() {
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        let mut win = window::Window::new("w");
        (win.open, win.focus) = (true, true);
        vm.st.borrow_mut().windows.push(win);
        let mut slot = Host::default();
        slot.vms.push(vm);
        assert!(slot.focused());
        let host = Host::park(&mut slot);
        assert!(slot.vms.is_empty() && slot.focused() && host.focused());
        // Changes made during the call show at once.
        host.vms[0].st.borrow_mut().windows[0].focus = false;
        assert!(!slot.focused());
        host.vms[0].st.borrow_mut().windows[0].focus = true;
        host.all_hidden.set(true);
        assert!(!slot.focused() && !slot.typing());
        host.all_hidden.set(false);
        host.vms[0].st.borrow_mut().windows[0].modal = true;
        assert_eq!(slot.modal(), Some((0, "w".to_string())));
        // A state the mod's own code holds is passed over, not a panic.
        {
            let _held = host.vms[0].st.borrow_mut();
            assert!(!slot.focused() && slot.modal().is_none());
        }
        // An unloaded mod's windows go with it.
        let mut host = host;
        let st = host.vms[0].st.clone();
        host.unload(None, 0);
        assert!(st.borrow().gone && !slot.focused() && slot.modal().is_none());
        slot = host;
        assert!(!slot.focused() && slot.parked.is_empty());
    }

    /// Every mod's tests: each assets/mods/<id>/tests/*.lua in a fresh VM of its mod. VANA_MOD_TEST=<id> or
    /// <id>/<file stem> runs only those; every failure is listed.
    #[test]
    fn mod_tests() {
        let only = std::env::var("VANA_MOD_TEST").ok();
        let (mut ran, mut failed) = (0, Vec::new());
        for dir in repo_mods() {
            let id = dir.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(rd) = std::fs::read_dir(dir.join("tests")) else { continue };
            let mut files: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "lua")).collect();
            files.sort();
            for file in files {
                let name = format!("{id}/{}", file.file_stem().unwrap().to_string_lossy());
                if only.as_deref().is_some_and(|o| o != id && o != name) {
                    continue;
                }
                ran += 1;
                let code = std::fs::read_to_string(&file).unwrap();
                let vm = repo_mod_vm(&dir);
                if let Err(e) = vm.lua.load(code).set_name(format!("@{name}.lua")).exec() {
                    failed.push(format!("{name}: {e}"));
                }
            }
        }
        assert!(failed.is_empty(), "{} of {ran} mod tests failed:\n{}", failed.len(), failed.join("\n\n"));
        match &only {
            Some(o) => assert!(ran > 0, "VANA_MOD_TEST={o} matches no mod test"),
            None => assert!(ran > 0, "no mod tests found"),
        }
    }
    use mlua::{Table, Value};

    /// Every repository mod declares (`[commands.<name>]`, with help and usage) exactly the commands its code
    /// registers, so the chat line's hint, //mods and the Mods page show them all.
    #[test]
    fn repo_commands_declared() {
        for dir in repo_mods() {
            let m = manifest::read(&dir).unwrap();
            let id = &m.id;
            let mut registered: Vec<String> = Vec::new();
            for (_, src) in repo_mod_lua(&dir) {
                for part in src.split("commands.register(").skip(1) {
                    let part = part.trim_start();
                    let q = part.chars().next().filter(|c| *c == '"' || *c == '\'');
                    let name = q.and_then(|q| part[1..].split(q).next()).expect("a literal command name");
                    registered.push(name.trim_start_matches('/').to_ascii_lowercase());
                }
            }
            registered.sort();
            registered.dedup();
            let mut declared: Vec<String> = m.commands.keys().map(|k| k.to_ascii_lowercase()).collect();
            declared.sort();
            assert_eq!(registered, declared, "{id}: commands registered vs declared in mod.toml");
            for (name, c) in &m.commands {
                assert!(!c.help.is_empty(), "{id}: //{name} needs help");
                assert!(c.forms.iter().all(|f| !f.help.is_empty()), "{id}: each form of //{name} needs help");
            }
        }
    }

    /// //mods: by mod, one command's forms, a mod's commands; a near miss names the command.
    #[test]
    fn help_and_near_misses() {
        let info = |name: &str, id: &str, owner: &str, forms: &[(&str, &str)]| CmdInfo {
            name: name.into(),
            id: id.into(),
            owner: owner.into(),
            help: format!("{name} help"),
            forms: forms.iter().map(|(a, h)| (a.to_string(), h.to_string())).collect(),
        };
        let list = vec![
            info("tdbuy", "trade-desk", "Trade Desk", &[("<item> <price> [stack]", "Bids.")]),
            info("tdsell", "trade-desk", "Trade Desk", &[("<item> <price> [stack]", "Sells.")]),
            info("sb", "sample", "Samples", &[("", "Opens the window."), ("show", "Shows the bars.")]),
            info("lua", "", "Mods", &[("list", "The mods.")]),
        ];
        let text = |args: &[&str]| {
            let mut out = Vec::new();
            help_lines(&list, "//mods", args, &mut out);
            out.into_iter().map(|l| l.0).collect::<Vec<_>>()
        };
        assert_eq!(text(&[]), ["//mods <mod or command>: its commands. Mods: sample, trade-desk."]);
        assert_eq!(
            text(&["sb"]),
            ["//sb (Samples): sb help", "  //sb - Opens the window.", "  //sb show - Shows the bars."]
        );
        assert_eq!(text(&["trade", "desk"]).len(), 3);
        assert_eq!(text(&["sample"]).len(), 3);
        // A mod with a command of the same name: the mod's commands, all of them.
        let same = vec![
            info("sample", "sample", "Sample", &[("", "Shows the file.")]),
            info("sa", "sample", "Sample", &[("<command>", "Runs one.")]),
        ];
        let mut out = Vec::new();
        help_lines(&same, "//mods", &["sample"], &mut out);
        assert_eq!(
            out.into_iter().map(|l| l.0).collect::<Vec<_>>(),
            ["Sample:", "  //sample - Shows the file.", "  //sa <command> - Runs one."]
        );
        assert_eq!(nearest(&list, "tdbyu").as_deref(), Some("tdbuy"));
        assert_eq!(nearest(&list, "tds").as_deref(), Some("tdsell"));
        assert_eq!(nearest(&list, "zzzz"), None);
        // A declaration's forms: the main one first (when given, or when alone), a form's help else the command's.
        let c =
            |t: &str| manifest::parse(&format!("[commands.x]\nhelp = \"X.\"\n{t}"), "m").unwrap().commands["x"].forms();
        assert_eq!(c(""), [(String::new(), "X.".to_string())]);
        assert_eq!(
            c("args = \"\"\nforms = [{ args = \"a\", help = \"A.\" }, { args = \"b\" }]"),
            [("".into(), "X.".into()), ("a".into(), "A.".into()), ("b".into(), "X.".into())]
        );
        assert_eq!(c("forms = [{ args = \"a\", help = \"A.\" }]"), [("a".to_string(), "A.".to_string())]);
        assert!(manifest::parse("[commands.lua]\nhelp = \"x\"", "m").is_err());
        assert!(manifest::parse("[commands.mods]\nhelp = \"x\"", "m").is_err());
        // Typed params: a known type, an enum with its options.
        let p = |t: &str| manifest::parse(&format!("[commands.x]\nhelp = \"X.\"\nargs = \"<a>\"\nparams = [{t}]"), "m");
        let ok = p("{ name = \"a\", type = \"item\", also = [\"none\"] }").unwrap();
        assert_eq!(ok.commands["x"].params[0].also, ["none"]);
        assert!(p("{ name = \"a\", type = \"enum\", options = [\"b\"] }").is_ok());
        assert!(p("{ name = \"a\", type = \"enum\" }").is_err());
        assert!(p("{ name = \"a\", type = \"thing\" }").is_err());
    }

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

    /// A coroutine the mod makes itself is under the time budget too (a per-thread hook missed it: the frame hung).
    #[test]
    fn own_coroutine_has_a_budget() {
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let f: mlua::Function =
            vm.lua.load("return function() coroutine.wrap(function() while true do end end)() end").eval().unwrap();
        let t0 = std::time::Instant::now();
        vm.resume(vm.lua.create_thread(f).unwrap(), ());
        assert!(t0.elapsed() < std::time::Duration::from_secs(2), "ran {:?}", t0.elapsed());
        assert_eq!(vm.st.borrow().errors.len(), 1);
    }

    /// Endless or huge waits are a century, not a crash; handlers by priority, the lowest i32 last.
    #[test]
    fn endless_waits_and_extreme_priorities() {
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua.load("timer.after(1/0, print); timer.every(1e300, print); timer.after(0/0, print)").exec().unwrap();
        assert_eq!(vm.st.borrow().timers.len(), 3);
        let f: mlua::Function = vm.lua.load("return function() sleep(math.huge) end").eval().unwrap();
        vm.resume(vm.lua.create_thread(f).unwrap(), ());
        assert_eq!(vm.st.borrow().sleepers.len(), 1);
        assert!(vm.st.borrow().errors.is_empty());
        assert_eq!(vm::secs(f64::INFINITY), vm::NEVER);
        assert_eq!(vm::secs(-1.0), std::time::Duration::ZERO);
        vm.lua
            .load(
                r#"order = {}
                events.on("e", function() order[#order + 1] = "min" end, {priority = -2147483648})
                events.on("e", function() order[#order + 1] = "max" end, {priority = 2147483647})
                events.on("e", function() order[#order + 1] = "zero" end)"#,
            )
            .exec()
            .unwrap();
        vm.emit("e", ());
        let order: Vec<String> = vm.lua.load("return order").eval().unwrap();
        assert_eq!(order, ["max", "zero", "min"]);
    }

    /// A spec whose metamethod reaches the mod's state while it is read: no panic (the fields are copied first).
    #[test]
    fn spec_metamethods_reach_no_borrowed_state() {
        let vm =
            Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua
            .load(
                r#"local base = {title = "From the class"}
                local spec = setmetatable({elements = {{type = "text", id = "t", text = "x"}}}, {__index = base})
                setmetatable(base, {__index = function() hud.clear(); timer.after(1, print) end})
                ui.window("w", spec)
                ui.update("w", "t", setmetatable({}, {__index = function() hud.clear() end}))"#,
            )
            .exec()
            .unwrap();
        // A table named twice (two elements the same table): both references take the copy, not the original.
        vm.lua
            .load(
                r#"local e = setmetatable({type = "text", id = "s", text = "y"}, {__index = function() hud.clear() end})
                ui.window("w2", {title = "Shared", elements = {e, e}})"#,
            )
            .exec()
            .unwrap();
        let st = vm.st.borrow();
        assert_eq!(st.windows[0].title, "From the class");
        assert_eq!(st.windows[1].title, "Shared");
    }

    /// hud.tri: its corners and colour from the spec (zero and white by default), one corner moved by hud.set, its
    /// hit box round the corners.
    #[test]
    fn hud_triangles() {
        // The hit box: the corners' bounds from the element's place, negative corners included.
        assert_eq!(hud::tri_bounds(100.0, 40.0, &[[-4.0, 0.0], [4.0, 0.0], [0.0, 5.0]]), [96.0, 40.0, 8.0, 5.0]);
        assert_eq!(hud::tri_bounds(10.0, 10.0, &[[0.0, 0.0]; 3]), [10.0, 10.0, 0.0, 0.0]);
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
                hud.tri("m", {x = 100, y = 40, x1 = -4, y1 = 0, x2 = 4, y2 = 0, x3 = 0, y3 = 5, colour = {215, 190, 120}})
                hud.tri("z", {})
            "#,
            )
            .exec()
            .unwrap();
        let tri = |vm: &Vm, id: &str| {
            vm.st.borrow().hud.iter().find_map(|e| match e.kind {
                hud::Kind::Tri { pts, colour } if e.id == id => Some((pts, colour)),
                _ => None,
            })
        };
        assert_eq!(tri(&vm, "z"), Some(([[0.0; 2]; 3], [255; 4])));
        assert_eq!(tri(&vm, "m"), Some(([[-4.0, 0.0], [4.0, 0.0], [0.0, 5.0]], [215, 190, 120, 255])));
        // hud.set moves one corner; the others and the colour stay.
        vm.lua.load(r#"hud.set("m", {x2 = 10, y3 = 8})"#).exec().unwrap();
        assert_eq!(tri(&vm, "m"), Some(([[-4.0, 0.0], [10.0, 0.0], [0.0, 8.0]], [215, 190, 120, 255])));
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

    /// A setting changed (the Mods window, outside the mod phase) is queued for the mod phase, for that mod only; its
    /// handler does not run at once.
    #[test]
    fn settings_changed_waits_for_the_mod_phase() {
        let mut host = Host::default();
        let vm = Vm::new(
            manifest::Manifest { id: "t".into(), ..Default::default() },
            PathBuf::from("."),
            vm::Source::Dir(PathBuf::from(".")),
        )
        .unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        vm.lua
            .load(r#"RAN = nil; events.on("settings_changed", function(key, value) RAN = key .. "=" .. value end)"#)
            .exec()
            .unwrap();
        host.vms.push(vm);
        host.settings_changed("t", "opacity", serde_json::json!(0.55));
        host.settings_changed("other", "size", serde_json::json!(3));
        assert!(host.vms[0].lua.globals().get::<Option<String>>("RAN").unwrap().is_none());
        assert_eq!(host.mod_events[0], ("t".to_string(), "settings_changed", vec!["opacity".into(), 0.55.into()]));
        // The frame's delivery: only this mod's event reaches it.
        let events = std::mem::take(&mut host.mod_events);
        emit_mod_events(&host.vms[0], &events);
        assert_eq!(host.vms[0].lua.globals().get::<String>("RAN").unwrap(), "opacity=0.55");
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
        host.all_hidden.set(true);
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

    /// A menu opened from a window gives that window the focus back when it closes (picked or Escaped), so Escape
    /// closes the window next; a list row's `colours` colour single cells by column.
    #[test]
    fn menu_gives_the_focus_back() {
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
                ui.window("w", {title = "W", elements = {{type = "list", id = "l",
                  rows = {{"a", "100%", colours = {[2] = {120, 220, 120}}}, "b"}}}})
                ui.open("w")
            "#,
            )
            .exec()
            .unwrap();
        host.vms.push(vm);
        let win = |host: &Host, id: &str| {
            let st = host.vms[0].st.borrow();
            st.windows.iter().find(|w| w.id == id).map(|w| (w.open, w.focus))
        };
        let Some(window::Elem::List { rows, .. }) = host.vms[0].st.borrow().windows[0].elems.first().cloned() else {
            panic!("no list")
        };
        assert_eq!(rows[0].cell_colours, vec![None, Some([120, 220, 120, 255])]);
        assert!(rows[1].cell_colours.is_empty());
        host.vms[0].st.borrow_mut().windows[0].focus = true;
        host.vms[0].lua.load(r#"ui.menu({"x", "y"}, function() end)"#).exec().unwrap();
        for w in host.vms[0].st.borrow_mut().windows.iter_mut() {
            w.focus = w.id == "__menu";
        }
        // Escape closes the menu: the window has the keys again.
        let st = host.vms[0].st.clone();
        st.borrow_mut().windows.iter_mut().find(|w| w.id == "__menu").unwrap().key(egui::Key::Escape);
        host.return_focus();
        assert_eq!((win(&host, "w"), win(&host, "__menu")), (Some((true, true)), Some((false, false))));
        // Once only: a later frame with the window unfocused leaves it so.
        host.vms[0].st.borrow_mut().windows[0].focus = false;
        host.return_focus();
        assert_eq!(win(&host, "w"), Some((true, false)));
    }

    /// A HUD element dragged and let go over one of its mod's canvases (as the modern UI last drew it): the canvas's
    /// window gets a drop at the point in the canvas, in its points; none once the window is closed.
    #[test]
    fn hud_drag_onto_canvas() {
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
                hud.rect("a", {x = 0, y = 0, w = 10, h = 10})
                hud.on("a", function() end)
                hud.rect("b", {x = 100, y = 0, w = 200, h = 100})
                hud.on("b", function() end)
                ui.window("w", {title = "W", elements = {{type = "canvas", id = "c", w = 100, h = 50}}})
                ui.open("w")
            "#,
            )
            .exec()
            .unwrap();
        host.vms.push(vm);
        // "b", with a handler, is under the window's canvas.
        host.info.borrow_mut().hud_rects = vec![
            ("t".into(), "a".into(), [0.0, 0.0, 10.0, 10.0]),
            ("t".into(), "b".into(), [100.0, 0.0, 200.0, 100.0]),
        ];
        // Drawn at (100, 0) in window pixels, 2 pixels to the point: 200 by 100 pixels.
        host.canvas_at("t", "w", 0, [100.0, 0.0], [100.0, 0.0, 300.0, 100.0], 2.0, true);
        let drag = |host: &mut Host, path: &[f64]| {
            host.input.borrow_mut().cursor = (5.0, 5.0);
            assert!(input::hud_press(host, Left));
            for &x in path {
                host.input.borrow_mut().cursor = (x, 5.0);
                input::hud_drag(host, x, 5.0);
            }
            let window = host.hud_events.iter().rev().find(|e| e.2["kind"] == "drag").map(|e| e.2["window"].clone());
            input::hud_release(host, Left);
            let hud = std::mem::take(&mut host.hud_events)
                .into_iter()
                .filter(|e| e.2["kind"] != "drag" && e.2["kind"] != "drag_start")
                .map(|e| (e.1, e.2["kind"].as_str().unwrap().to_string(), e.2["over"].clone()))
                .collect::<Vec<_>>();
            let ev = std::mem::take(&mut host.ui_events);
            assert!(ev.iter().all(|e| e.0 == "t" && e.1 == "w"));
            (ev.into_iter().map(|e| e.2.json()).collect::<Vec<_>>(), window, hud)
        };
        // The canvas is over "b": it takes the drop, and drag_end names no HUD element.
        let (ev, window, hud) = drag(&mut host, &[20.0, 150.0]);
        assert_eq!(
            ev,
            [
                serde_json::json!({"kind": "drag_over", "id": "c", "x": 25, "y": 3, "from": "a"}),
                serde_json::json!({"kind": "drop", "id": "c", "x": 25, "y": 3, "from": "a"})
            ]
        );
        assert_eq!(window, Some(serde_json::json!("w")));
        assert_eq!(hud, [("a".to_string(), "drag_end".to_string(), serde_json::Value::Null)]);
        // Over it and off again: drag_out and no drop; the last drag names no window.
        let (ev, window, _) = drag(&mut host, &[150.0, 320.0]);
        assert_eq!(ev.iter().map(|e| e["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["drag_over", "drag_out"]);
        assert_eq!(window, Some(serde_json::Value::Null));
        // Something over the canvas under the pointer: no drop there, "b" under it takes it.
        host.canvas_at("t", "w", 0, [100.0, 0.0], [100.0, 0.0, 300.0, 100.0], 2.0, false);
        let (ev, _, hud) = drag(&mut host, &[20.0, 150.0]);
        assert!(ev.is_empty());
        assert_eq!(
            hud,
            [
                ("a".to_string(), "drag_end".to_string(), serde_json::json!("b")),
                ("b".to_string(), "drop".to_string(), serde_json::Value::Null)
            ]
        );
        host.canvas_at("t", "w", 0, [100.0, 0.0], [100.0, 0.0, 300.0, 100.0], 2.0, true);
        host.vms[0].lua.load(r#"ui.close("w")"#).exec().unwrap();
        assert!(drag(&mut host, &[20.0, 150.0]).0.is_empty());
    }

    /// A drag on a mod's canvas over another of its windows' canvases: the drag events say where on screen and which
    /// window is under the pointer; that canvas gets drag_over, then the drop on the release (not its canvas).
    #[test]
    fn canvas_drag_onto_canvas() {
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
                ui.window("a", {title = "A", elements = {{type = "canvas", id = "s", w = 100, h = 50}}})
                ui.window("b", {title = "B", elements = {{type = "canvas", id = "c", w = 100, h = 50}}})
                ui.open("a"); ui.open("b")
            "#,
            )
            .exec()
            .unwrap();
        host.vms.push(vm);
        host.canvas_at("t", "a", 0, [0.0, 0.0], [0.0, 0.0, 100.0, 50.0], 1.0, true);
        host.canvas_at("t", "b", 0, [200.0, 0.0], [200.0, 0.0, 300.0, 50.0], 1.0, true);
        let drag = |phase: u8, x: f32| window::Event::Drag {
            id: "s".into(),
            phase,
            x,
            y: 10.0,
            dx: 0.0,
            dy: 0.0,
            screen: None,
            over: None,
            covered: None,
        };
        // A drag says whether a window or panel is under the pointer; a client that cannot tell leaves it out.
        let mut marked = vec![drag(2, 5.0)];
        assert!(marked[0].json().get("covered").is_none());
        window::Event::set_covered(&mut marked, true);
        assert_eq!(marked[0].json()["covered"].as_bool(), Some(true));
        // Within its canvas: nothing crosses.
        let mut ev = vec![drag(1, 50.0)];
        assert!(host.canvas_drags("t", "a", &mut ev).is_empty() && host.ui_events.is_empty());
        assert_eq!(ev[0].json()["window"], serde_json::Value::Null);
        let mut ev = vec![drag(1, 250.0)];
        assert!(host.canvas_drags("t", "a", &mut ev).is_empty());
        assert_eq!((ev[0].json()["sx"].as_i64(), ev[0].json()["window"].as_str()), (Some(250), Some("b")));
        let over = std::mem::take(&mut host.ui_events);
        assert_eq!(
            (over[0].1.as_str(), over[0].2.json()),
            ("b", serde_json::json!({"kind": "drag_over", "id": "c", "x": 50, "y": 10, "from": "s"}))
        );
        let mut ev = vec![drag(2, 260.0)];
        let dropped = host.canvas_drags("t", "a", &mut ev);
        assert_eq!(
            (dropped[0].1.as_str(), dropped[0].2.json()),
            ("b", serde_json::json!({"kind": "drop", "id": "c", "x": 60, "y": 10, "from": "s"}))
        );
    }

    /// No two repository mods register the same // command (the first loaded would take it).
    #[test]
    fn repo_commands_unique() {
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for dir in repo_mods() {
            let id = manifest::read(&dir).unwrap().id;
            for (_, src) in repo_mod_lua(&dir) {
                for part in src.split("commands.register(\"").skip(1) {
                    let name = part.split('"').next().unwrap().to_string();
                    if let Some(other) = seen.insert(name.clone(), id.clone()) {
                        assert_eq!(other, id, "//{name} is registered by both {other} and {id}");
                    }
                }
            }
        }
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

    /// Every repository mod's manifest parses and its Lua compiles; settings on our Extras tabs name one and label
    /// their values.
    #[test]
    fn repo_mods_load() {
        for dir in repo_mods() {
            let m = manifest::read(&dir).unwrap_or_else(|e| panic!("{}: {e:#}", dir.display()));
            let id = &m.id;
            for (name, src) in repo_mod_lua(&dir) {
                let lua = mlua::Lua::new();
                lua.load(&src).set_name(&name).into_function().unwrap_or_else(|e| panic!("{id}/{name}: {e}"));
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

    /// Per-window opacity: a mod's for its windows (ui.window, ui.opacity, hud.opacity), cleared by it alone; the
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

    /// require and the mod's files stay in its folder: no leading dot, no link below the folder.
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

    /// A mod turned on outside the mod phase waits for it: queued once, loaded there (a failure reported), skipped if it
    /// is running by then.
    #[test]
    fn mods_turned_on_load_in_the_mod_phase() {
        let mut host = Host::default();
        let (running, missing) = (PathBuf::from("running-mod"), PathBuf::from("no-such-mod-folder"));
        let m = manifest::Manifest { id: "running".into(), ..Default::default() };
        host.vms.push(Vm::new(m, running.clone(), vm::Source::Dir(running.clone())).unwrap());
        host.load_later(running.clone());
        host.load_later(missing.clone());
        host.load_later(missing.clone());
        assert_eq!(host.pending_loads, vec![running, missing]);
        let mut out = Vec::new();
        host.load_pending(None, &mut out);
        assert!(host.pending_loads.is_empty());
        assert_eq!(host.vms.len(), 1);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].0.starts_with("Mod no-such-mod-folder: "), "{out:?}");
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
        // Every global table but Lua's is a namespace of ours, documented or not.
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
            // Sub-tables (ui.native) are separate namespaces.
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
    let host = Host::park(&mut w.lua);
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
    let host = Host::park(&mut w.lua);
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
            old.sort_by_key(|o| std::cmp::Reverse(o.0));
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
/// A mod's page: `scope` (per character, per job) for one in the mods folder, its first two rows.
fn mod_page(
    m: &manifest::Manifest,
    settings: &serde_json::Map<String, serde_json::Value>,
    granted: &[String],
    scope: Option<(bool, bool)>,
) -> Option<super::settings::ModPage> {
    use super::settings::{Edit, ModPage, ModRow};
    let mut rows = Vec::new();
    for (on, key, label, help) in scope.into_iter().flat_map(|(c, j)| {
        [
            (
                c,
                SCOPE_CHARACTER,
                "Per Character",
                "On: the Mods page's switch for this mod is each character's. A \
             character never switched follows it as it was (not retail).",
            ),
            (
                j,
                SCOPE_JOB,
                "Per Job",
                "On: the Mods page's switch for this mod is each job's (with Per Character, \
             each character's job's). A job never switched follows it as it was (not retail).",
            ),
        ]
    }) {
        rows.push(ModRow {
            key: key.into(),
            label: label.into(),
            help: help.into(),
            choices: vec![("ON".into(), true.into()), ("OFF".into(), false.into())],
            at: if on { 0 } else { 1 },
            tab: String::new(),
            place: None,
            place_modern: None,
            preview: None,
            edit: Edit::Choice,
        });
    }
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
            // Edited in the Mods window: the one choice holds the value and what shows of it.
            k @ ("string" | "colour" | "key") => {
                let edit = match k {
                    "colour" => Edit::Colour,
                    "key" => Edit::Key,
                    _ => Edit::Text,
                };
                rows.push(ModRow {
                    key: key.clone(),
                    label: if d.label.is_empty() { key.clone() } else { d.label.clone() },
                    help: d.help.clone(),
                    choices: vec![(edit_shown(edit, &current), current)],
                    at: 0,
                    tab: String::new(),
                    place: None,
                    place_modern: None,
                    preview: None,
                    edit,
                });
                continue;
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
            edit: Edit::Choice,
        });
    }
    // The permissions it asks for (docs/lua-mods.md 3.5).
    for p in &m.permissions {
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
            edit: Edit::Choice,
        });
    }
    (!rows.is_empty()).then(|| ModPage { id: m.id.clone(), name: m.name.clone(), rows })
}

/// An edited setting's value as the Mods window shows it: the text, the colour as #RRGGBB, the key as players read it.
pub(in crate::client) fn edit_shown(edit: super::settings::Edit, v: &serde_json::Value) -> String {
    use super::settings::Edit;
    match edit {
        Edit::Colour => colour_rgba(v).map_or(String::new(), |[r, g, b, _]| format!("#{r:02X}{g:02X}{b:02X}")),
        Edit::Key => v.as_str().filter(|k| !k.is_empty()).map_or_else(|| "none".into(), input::combo_label),
        _ => v.as_str().map_or_else(|| if v.is_null() { String::new() } else { v.to_string() }, str::to_string),
    }
}

/// A colour setting's value ([r, g, b] or [r, g, b, a], 0..255) as RGBA.
pub(in crate::client) fn colour_rgba(v: &serde_json::Value) -> Option<[u8; 4]> {
    let a = v.as_array().filter(|a| a.len() == 3 || a.len() == 4)?;
    let mut out = [255u8; 4];
    for (o, x) in out.iter_mut().zip(a) {
        *o = u8::try_from(x.as_u64()?).ok()?;
    }
    Some(out)
}

/// A colour typed as #RRGGBB (or RRGGBB), with the alpha of `old` (opaque when it has none): the setting's value.
pub(in crate::client) fn colour_typed(text: &str, old: &serde_json::Value) -> Option<serde_json::Value> {
    let h = text.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(colour_value([n(0)?, n(2)?, n(4)?], old))
}

/// An RGB with the alpha of `old` (opaque when it has none), as a colour setting's value.
pub(in crate::client) fn colour_value(rgb: [u8; 3], old: &serde_json::Value) -> serde_json::Value {
    let a = colour_rgba(old).map_or(255, |c| c[3]);
    serde_json::json!([rgb[0], rgb[1], rgb[2], a])
}

/// Removes every saved permission for `id` (a server catalogue install replaces an older copy).
pub(in crate::client) fn clear_grants_for_mod(id: &str) {
    let mut g = grants();
    if g.remove(id).is_some() {
        save_grants(&g);
    }
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
/// A mod page's Per Character and Per Job rows as setting changes' keys: the app keeps them in the config's entry.
pub(in crate::client) const SCOPE_CHARACTER: &str = "@scope:character";
pub(in crate::client) const SCOPE_JOB: &str = "@scope:job";

/// The mods folder's folder of mod `id` (a page names a mod by its manifest id, the config by its folder).
pub(in crate::client) fn folder_name(id: &str) -> String {
    folder_of(&mods_dir(), id).file_name().map_or_else(|| id.to_string(), |f| f.to_string_lossy().into_owned())
}

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
    /// `mods`: the config's list, for the Per Character and Per Job rows of the mods in the mods folder.
    pub(super) fn settings_pages(&self, mods: &[crate::config::ModRef]) -> Vec<super::settings::ModPage> {
        let scope_of = |folder: Option<&std::ffi::OsStr>| {
            let r = mods.iter().find(|r| folder.is_some_and(|f| f == r.name.as_str()));
            r.map(|r| (r.per_character, r.per_job))
        };
        let mut pages = Vec::new();
        for vm in &self.vms {
            let st = vm.st.borrow();
            pages.extend(mod_page(&vm.manifest, &st.settings, &st.granted, scope_of(vm.dir.file_name())));
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
            pages.extend(mod_page(&m, &settings, &given, scope_of(e.path().file_name())));
        }
        pages
    }

    /// The mods whose switch is kept per character or job: each loaded or unloaded as the character and job in play
    /// (the job not known yet: left as they are), and those in `changed` (a scope just turned off: back to `enabled`).
    /// After the start: it loads what it finds on then.
    pub(super) fn follow_scope(&mut self, w: &World, mods: &[crate::config::ModRef], changed: &[String]) {
        if !self.started {
            return;
        }
        let (character, job) = (super::uiconf::character(&w.world_name, &w.name), super::uiconf::job_of(w));
        self.dir.borrow_mut().at = Some((character.clone(), job.map(str::to_string)));
        for r in mods.iter().filter(|r| r.per_character || r.per_job || changed.contains(&r.name)) {
            let dir = mods_dir().join(&r.name);
            let Some(want) = r.active(Some(&character), job).filter(|_| dir.join("mod.toml").is_file()) else {
                continue;
            };
            match (want, self.named(&r.name)) {
                (true, None) => {
                    log::info!("lua mod {}: on for {character} {}", r.name, job.unwrap_or("?"));
                    self.load_later(dir);
                }
                (false, Some(k)) => {
                    log::info!("lua mod {}: off for {character} {}", r.name, job.unwrap_or("?"));
                    self.unload(Some(w), k);
                }
                (false, None) => self.pending_loads.retain(|d| *d != dir),
                _ => {}
            }
        }
    }

    /// Queues the mod in `dir` for the next mod phase (`pending_loads`).
    fn load_later(&mut self, dir: PathBuf) {
        if !self.pending_loads.contains(&dir) {
            self.pending_loads.push(dir);
        }
    }

    /// Loads the mods queued by `load_later` (in the mod phase, with the game UI's data lent); a failure goes to the chat
    /// log, as at the start.
    fn load_pending(&mut self, w: Option<&World>, out: &mut Vec<(String, [u8; 4])>) {
        for dir in std::mem::take(&mut self.pending_loads) {
            if self.vms.iter().any(|v| v.dir == dir) {
                continue;
            }
            if let Err(e) = self.load(w, dir.clone()) {
                let name = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                log::warn!("lua mod {name}: {e}");
                out.push((format!("Mod {name}: {e}"), ERROR));
            }
        }
    }

    /// The loaded mod `id` names: the Mods page names a mod by its folder, the rest by its manifest id, and the two
    /// may differ (a manifest picks its id); the folder first.
    fn named(&self, id: &str) -> Option<usize> {
        let by_folder = self.vms.iter().position(|v| v.dir.file_name().is_some_and(|f| f == id));
        by_folder.or_else(|| self.vms.iter().position(|v| v.manifest.id == id))
    }

    /// A setting changed on its Extras page: kept, written to its scope's file, and `settings_changed` sent.
    pub(super) fn apply_setting(&mut self, w: &World, id: &str, key: &str, value: serde_json::Value) {
        if key == ENABLED {
            let k = self.named(id);
            match (value.as_bool() == Some(true), k) {
                (true, None) => self.load_later(folder_of(&mods_dir(), id)),
                (false, Some(k)) => self.unload(Some(w), k),
                (false, None) => {
                    let dir = folder_of(&mods_dir(), id);
                    self.pending_loads.retain(|d| *d != dir);
                }
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
        self.settings_changed(id, key, value);
    }

    /// A mod's setting changed: told to it in the mod phase, where its handler can read all the game state (the Mods
    /// window changes settings outside it).
    fn settings_changed(&mut self, id: &str, key: &str, value: serde_json::Value) {
        self.mod_events.push((id.to_string(), "settings_changed", vec![key.into(), value]));
    }
}

impl Host {
    /// Moves the host out of `slot` to run the mods (the free `lend` goes the other way: the World to a mod). The
    /// stand-in left behind shares the mods' states, `/lua hide` and the frame info, so what the World asks it during
    /// the call (the macro gate, game.info().menu_open, a mod line being typed) is the mods' state now.
    pub(in crate::client) fn park(slot: &mut Host) -> Host {
        let host = std::mem::take(slot);
        slot.parked = host.vms.iter().map(|v| v.st.clone()).collect();
        slot.all_hidden = host.all_hidden.clone();
        slot.info = host.info.clone();
        host
    }

    /// The running mods' states, in load order: the host's, or a parked stand-in's (live). A state borrowed by the
    /// mod's own code at this moment is skipped.
    fn running(&self) -> impl Iterator<Item = std::cell::Ref<'_, vm::ModState>> {
        let own = self.vms.iter().filter(|v| v.suspended.is_none()).filter_map(|v| v.st.try_borrow().ok());
        let parked = self.parked.iter().filter_map(|s| s.try_borrow().ok()).filter(|s| !s.suspended && !s.gone);
        own.chain(parked)
    }

    /// Whether a mod window has the keys.
    pub(super) fn focused(&self) -> bool {
        !self.all_hidden.get() && self.running().any(|s| s.windows.iter().any(|w| w.open && w.focus))
    }

    /// The loaded mods' ids.
    pub(super) fn mod_ids(&self) -> std::collections::HashSet<String> {
        self.vms.iter().map(|v| v.manifest.id.clone()).collect()
    }

    /// A popup that closed gives the focus back to the window of its mod it was opened from, while that is still open
    /// and nothing else took the focus (a pick in a slot's menu leaves the window it came from on the keys).
    fn return_focus(&mut self) {
        let focused = self.vms.iter().any(|v| v.st.borrow().windows.iter().any(|w| w.focus && w.open));
        for vm in &self.vms {
            let mut st = vm.st.borrow_mut();
            let backs: Vec<String> =
                st.windows.iter_mut().filter(|w| !w.open && w.back.is_some()).filter_map(|w| w.back.take()).collect();
            if focused {
                continue;
            }
            if let Some(win) = backs.iter().find_map(|b| st.windows.iter().position(|w| w.id == *b && w.open)) {
                st.windows[win].focus = true;
            }
        }
    }

    /// The open modal window (vm, id), the last opened when there are several.
    pub(super) fn modal(&self) -> Option<(usize, String)> {
        // The index is the mod's place in `vms` (a parked stand-in's states are in the same order).
        let all = self.vms.iter().map(|v| (v.suspended.is_none(), &v.st));
        let parked = self.parked.iter().map(|s| (true, s));
        all.chain(parked).enumerate().filter(|(_, (on, _))| *on).find_map(|(k, (_, st))| {
            let s = st.try_borrow().ok().filter(|s| !s.suspended && !s.gone)?;
            s.windows.iter().rev().find(|w| w.open && w.modal).map(|w| (k, w.id.clone()))
        })
    }

    /// A mod window's canvas drawn this frame, at its element `i` (modwin's CanvasAt): kept for drops. Every frame, for
    /// every canvas: an entry already kept is updated in place.
    fn canvas_at(&mut self, m: &str, win: &str, i: usize, origin: [f32; 2], shown: [f32; 4], ppp: f32, top: bool) {
        let now = std::time::Instant::now();
        let Some(vm) = self.vms.iter().find(|v| v.manifest.id == m) else { return };
        let st = vm.st.borrow();
        let Some(Some(window::Elem::Canvas { id, .. })) =
            st.windows.iter().find(|w| w.id == win).map(|w| w.elems.get(i))
        else {
            return;
        };
        let id = id.clone();
        drop(st);
        // ui.bounds reads them from the frame info.
        self.info.borrow_mut().canvas_rects.insert((m.into(), win.into(), id.clone()), [origin[0], origin[1], ppp]);
        if let Some(c) = self.canvases.iter_mut().find(|c| c.i == i && c.m == m && c.win == win) {
            if c.id != id {
                c.id = id;
            }
            (c.origin, c.shown, c.ppp, c.top, c.at) = (origin, shown, ppp, top, now);
            return;
        }
        self.canvases.retain(|c| now.duration_since(c.at) < std::time::Duration::from_secs(1));
        self.canvases.push(CanvasAt { m: m.into(), win: win.into(), i, id, origin, shown, ppp, top, at: now });
        let live: std::collections::HashSet<(String, String, String)> =
            self.canvases.iter().map(|c| (c.m.clone(), c.win.clone(), c.id.clone())).collect();
        self.info.borrow_mut().canvas_rects.retain(|k, _| live.contains(k));
    }

    /// Where mod `m`'s canvas `id` of window `win`, at (x, y) in its units, is on screen (window pixels).
    fn canvas_to_screen(&self, m: &str, win: &str, id: &str, x: f32, y: f32) -> Option<(f32, f32)> {
        let c = self.canvases.iter().rev().find(|c| c.m == m && c.win == win && c.id == id)?;
        Some((c.origin[0] + x * c.ppp, c.origin[1] + y * c.ppp))
    }

    /// A drag of mod `m`'s (`from`: a HUD element, or a canvas, which is `skip` (window, canvas)) at (x, y) in window
    /// pixels: the canvas of the mod's under it gets drag_over, the one it left drag_out. The window under it, if any.
    fn drag_cross(&mut self, m: &str, from: &str, x: f64, y: f64, skip: Option<(&str, &str)>) -> Option<String> {
        let over = self.canvas_drop(m, x, y, from, skip).and_then(|(win, ev)| match ev {
            window::Event::Drop { id: c, x, y, from } => {
                Some((win, c.clone(), window::Event::DragOver { id: c, x, y, from }))
            }
            _ => None,
        });
        let now = over.as_ref().map(|(win, c, _)| (m.to_string(), win.clone(), c.clone(), from.to_string()));
        if let Some((pm, pwin, pc, pfrom)) = self.drag_over.take()
            && now.as_ref().is_none_or(|n| n.1 != pwin || n.2 != pc)
        {
            self.ui_events.push((pm, pwin, window::Event::DragOver { id: pc, x: -1.0, y: -1.0, from: pfrom }));
        }
        self.drag_over = now;
        let (win, _, ev) = over?;
        match self.ui_events.last_mut() {
            Some(last)
                if last.0 == m && last.1 == win && matches!(last.2, window::Event::DragOver { x, .. } if x >= 0.0) =>
            {
                last.2 = ev
            }
            _ => self.ui_events.push((m.to_string(), win.clone(), ev)),
        }
        Some(win)
    }

    /// A drag of mod `m`'s released at (x, y) in window pixels (as drag_cross): the drop on its canvas there, if any
    /// (the window, its event); the canvas it was over before gets drag_out unless it is the one dropped on.
    fn drag_release(
        &mut self,
        m: &str,
        from: &str,
        x: f64,
        y: f64,
        skip: Option<(&str, &str)>,
    ) -> Option<(String, window::Event)> {
        let drop = self.canvas_drop(m, x, y, from, skip);
        if let Some((pm, pwin, pc, pfrom)) = self.drag_over.take()
            && drop
                .as_ref()
                .is_none_or(|(w, e)| *w != pwin || !matches!(e, window::Event::Drop { id, .. } if *id == pc))
        {
            self.ui_events.push((pm, pwin, window::Event::DragOver { id: pc, x: -1.0, y: -1.0, from: pfrom }));
        }
        drop
    }

    /// The drop on mod `m`'s canvas under (x, y) in window pixels, drawn in the last moments in an open window, on top
    /// there (no panel or window over it under the pointer), not `skip` (the (window, canvas) the drag started on):
    /// the window and its event, at the point in the canvas.
    fn canvas_drop(
        &self,
        m: &str,
        x: f64,
        y: f64,
        from: &str,
        skip: Option<(&str, &str)>,
    ) -> Option<(String, window::Event)> {
        let (x, y) = (x as f32, y as f32);
        let now = std::time::Instant::now();
        let c = self.canvases.iter().rev().find(|c| {
            c.m == m
                && c.top
                && skip.is_none_or(|(w, id)| c.win != w || c.id != id)
                && now.duration_since(c.at) < std::time::Duration::from_millis(250)
                && x >= c.shown[0]
                && y >= c.shown[1]
                && x < c.shown[2]
                && y < c.shown[3]
        })?;
        let open = self
            .vms
            .iter()
            .find(|v| v.manifest.id == m && v.suspended.is_none())
            .is_some_and(|v| v.st.borrow().windows.iter().any(|w| w.id == c.win && w.open));
        let ppp = c.ppp.max(0.01);
        open.then(|| {
            let ev = window::Event::Drop {
                id: c.id.clone(),
                x: (x - c.origin[0]) / ppp,
                y: (y - c.origin[1]) / ppp,
                from: from.into(),
            };
            (c.win.clone(), ev)
        })
    }

    /// Whether a mod window's line is being typed (it takes the text and the keys).
    pub(super) fn typing(&self) -> bool {
        !self.all_hidden.get() && self.running().any(|s| s.windows.iter().any(|w| w.open && w.focus && w.takes_text()))
    }

    /// The focused window's title and the help under its cursor, for the classic help bar.
    pub(super) fn focused_help(&self) -> Option<(String, String)> {
        self.running().find_map(|s| {
            s.windows.iter().find(|w| w.open && w.focus).map(|w| (w.title.clone(), w.help_now().to_string()))
        })
    }
}

impl Host {
    /// A key press as a key setting's value ("ctrl+numpad1"), as a mod window's key element takes it.
    pub(in crate::client) fn key_combo(&self, e: &egui::Event) -> Option<String> {
        input::combo_of(e, self.input.borrow().last_down)
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
            // A key a mod pressed never answers the client's question for the player (a real key the same frame
            // goes too).
            if win.id == api::CLIENT_ASK && host.synth_keys {
                log::info!("{}: a pressed key ignored by the client's question", vm.manifest.id);
                return true;
            }
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

/// A classic window of the HUD layer, drawn with the Timers' dock under the HUD and every game window: a HUD or edge
/// window that is not modal and not opened at an anchor. The others (a mod's manager, a popup) stay over them.
fn hud_layer(win: &window::Window) -> bool {
    win.hud && !win.modal && win.anchor.is_none()
}

/// The open mod windows in the classic UI of one layer (`hud`: hud_layer's), cascaded from the Config pages' place;
/// their rects.
pub(super) fn draw_classic(gu: &mut super::game_ui::GameUi, w: &World, hud: bool) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for (v, id, x, y) in classic_places(&w.lua) {
        if let Some(win) = w.lua.vms[v].st.borrow().windows.iter().find(|x| x.id == id && hud_layer(x) == hud) {
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
        self.windows_where(|w| w.open && keep(w))
    }

    /// The windows `keep` takes, open or not (a closed animated window still folds away).
    pub(super) fn windows_where(&self, keep: impl Fn(&window::Window) -> bool) -> Vec<(String, window::Window)> {
        let mut out = Vec::new();
        if self.all_hidden.get() {
            return out;
        }
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for win in vm.st.borrow().windows.iter().filter(|w| keep(w)) {
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
    if let super::modern::ModClick::CanvasAt(i, origin, shown, ppp, top) = click {
        host.canvas_at(mod_id, win_id, i, origin, shown, ppp, top);
        return;
    }
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
    if let Some((mut ev, se)) = out {
        let dropped = host.canvas_drags(mod_id, win_id, &mut ev);
        host.ui_events.extend(ev.into_iter().map(|e| (mod_id.to_string(), win_id.to_string(), e)));
        host.ui_events.extend(dropped);
        w.sounds.extend(se.into_iter().map(|s| s as u32));
    }
}

impl Host {
    /// A window's events: a canvas drag's get where on screen it is, and it crosses the mod's other canvases (they
    /// get drag_over and drag_out, and the drop on the release: returned, to follow the window's events).
    fn canvas_drags(
        &mut self,
        mod_id: &str,
        win_id: &str,
        ev: &mut [window::Event],
    ) -> Vec<(String, String, window::Event)> {
        let host = self;
        let mut dropped = Vec::new();
        for e in ev.iter_mut() {
            let window::Event::Drag { id, phase, x, y, screen, over, .. } = e else { continue };
            let Some((sx, sy)) = host.canvas_to_screen(mod_id, win_id, id, *x, *y) else { continue };
            *screen = Some((sx, sy));
            let skip = Some((win_id, id.as_str()));
            match phase {
                1 => *over = host.drag_cross(mod_id, id, sx as f64, sy as f64, skip),
                2 => {
                    if let Some((w, d)) = host.drag_release(mod_id, id, sx as f64, sy as f64, skip) {
                        dropped.push((mod_id.to_string(), w, d));
                    }
                }
                _ => {}
            }
        }
        dropped
    }
}

/// The classic places of the open windows, in draw order (cascaded from the Config pages' place; the HUD layer's
/// first, modal ones last, centred, or at their anchor kept on screen).
fn classic_places(h: &Host) -> Vec<(usize, String, f32, f32)> {
    let (mut hud, mut out, mut modal) = (Vec::new(), Vec::new(), Vec::new());
    if h.all_hidden.get() {
        return out;
    }
    let (screen, uk) = {
        let f = h.info.borrow();
        (f.window, f.k)
    };
    let (sw, sh) = (screen[0] * uk.0, screen[1] * uk.1);
    let mut k = 0.0;
    for (v, vm) in h.vms.iter().enumerate().filter(|(_, v)| v.suspended.is_none()) {
        // A panel's edge window is the Timers' dock's (regime.rs).
        for win in vm.st.borrow().windows.iter().filter(|w| w.open && w.panel.is_none()) {
            // The room below its top: a list shows fewer rows rather than run past the screen's bottom.
            let top = match (win.anchor, win.place) {
                (None, Some((_, y))) => y,
                (None, None) if !win.modal => 48.0 + 24.0 * k,
                _ => 0.0,
            };
            win.room.set((sh - top - 8.0).max(0.0));
            let (ww, wh) = win.classic_size();
            match (win.anchor, win.place) {
                (Some((ax, ay)), _) => modal.push((
                    v,
                    win.id.clone(),
                    (ax * uk.0).min(sw - ww).max(0.0),
                    (ay * uk.1 - if win.above { wh } else { 0.0 }).min(sh - wh).max(0.0),
                )),
                (None, Some((x, y))) if win.modal => modal.push((v, win.id.clone(), x, y)),
                (None, None) if win.modal => {
                    modal.push((v, win.id.clone(), ((sw - ww) / 2.0).max(0.0), ((sh - wh) / 2.0).max(0.0)))
                }
                (None, Some((x, y))) => {
                    if hud_layer(win) { &mut hud } else { &mut out }.push((v, win.id.clone(), x, y))
                }
                (None, None) => {
                    let at = (v, win.id.clone(), 16.0 + 24.0 * k, 48.0 + 24.0 * k);
                    if hud_layer(win) { &mut hud } else { &mut out }.push(at);
                    k += 1.0;
                }
            }
        }
    }
    hud.extend(out);
    hud.extend(modal);
    hud
}

/// The classic places the pointer at `p` can reach: the HUD layer's windows not where a game window or the HUD is over
/// them (`over_mod_hud`: World::over_mod_hud), unless a press or a drag on one already holds the pointer.
fn pointer_places(h: &Host, over_mod_hud: &[[f32; 4]], p: (f32, f32)) -> Vec<(usize, String, f32, f32)> {
    let mut places = classic_places(h);
    let r = |r: &[f32; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3];
    if over_mod_hud.iter().any(r) {
        places.retain(|(v, id, _, _)| {
            h.vms[*v]
                .st
                .borrow()
                .windows
                .iter()
                .find(|x| x.id == *id)
                .is_none_or(|win| !hud_layer(win) || win.drag.is_some() || win.press.is_some())
        });
    }
    places
}

/// A classic mod window holds the pointer: a press or a canvas drag on it goes on until the button comes up, wherever
/// the pointer is. Only the windows classic_places routes (pointer_classic clears the press or drag): not while the
/// mods' windows are hidden (/lua hide), nor a suspended mod's or a panel's window, whose press or drag nothing ends.
pub(super) fn holds_pointer(h: &Host) -> bool {
    !h.all_hidden.get()
        && h.vms.iter().filter(|vm| vm.suspended.is_none()).any(|vm| {
            vm.st
                .borrow()
                .windows
                .iter()
                .any(|win| win.open && win.panel.is_none() && (win.drag.is_some() || win.press.is_some()))
        })
}

/// The item of the classic mod list cell under the pointer, for the item window: (id, extdata).
pub(super) fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    if w.modern {
        return None;
    }
    w.lua.vms.iter().find_map(|vm| {
        let st = vm.st.borrow();
        st.windows.iter().filter(|x| x.open).find_map(|x| x.pointed).map(|p| (p.id, p.ext(w)))
    })
}

/// Forgets the classic list cell under the pointer: found again while the pointer is over the window.
pub(super) fn clear_pointed(w: &World) {
    for vm in &w.lua.vms {
        for win in vm.st.borrow_mut().windows.iter_mut() {
            win.pointed = None;
        }
    }
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
    let all = classic_places(&w.lua);
    let mut places = pointer_places(&w.lua, &w.over_mod_hud, p);
    // A canvas the pointer has left (its window not under it now, or a window over it there): its 'leave'.
    let mut left = Vec::new();
    for (v, id, x, y) in &all {
        let vm = &w.lua.vms[*v];
        let reached = places.iter().any(|q| q.0 == *v && q.1 == *id);
        let mut st = vm.st.borrow_mut();
        // A click on a window over it closes its open dropdown, as a click off it does.
        if !reached
            && (clicked || right)
            && let Some(win) = st.windows.iter_mut().find(|x| x.id == *id)
            && let Some(d) = win.drop_open()
        {
            win.drop_close(d);
        }
        if let Some(win) = st.windows.iter_mut().find(|x| x.id == *id && x.hovered.is_some()) {
            // With an open dropdown's list hanging below it.
            let (ww, wh) = win.classic_reach();
            if !reached || p.0 < *x || p.1 < *y || p.0 >= x + ww || p.1 >= y + wh {
                left.extend(win.hover_off().into_iter().map(|e| (vm.manifest.id.clone(), id.clone(), e)));
            }
        }
    }
    w.lua.ui_events.extend(left);
    // An open modal window is the only one the pointer reaches; a click outside a popup closes it.
    if let Some((k, mid)) = w.lua.modal() {
        places.retain(|(v, id, _, _)| *v == k && *id == mid);
        let inside = places.first().is_some_and(|(v, id, x, y)| {
            let (ww, wh) = w.lua.vms[*v]
                .st
                .borrow()
                .windows
                .iter()
                .find(|x| x.id == *id)
                .map_or((0.0, 0.0), |x| x.classic_reach());
            p.0 >= *x && p.1 >= *y && p.0 < x + ww && p.1 < y + wh
        });
        if (clicked || right) && !inside {
            close_popup(w, k, &mid);
            return true;
        }
    }
    // A mod window (a HUD-layer one a game window covers too) or a recorded game window under the pointer, over the HUD
    // there (canvas drags report it).
    let covered = all.iter().any(|(v, id, x, y)| {
        w.lua.vms[*v].st.borrow().windows.iter().find(|win| win.id == *id).is_some_and(|win| {
            let (ww, wh) = win.classic_reach();
            p.0 >= *x && p.1 >= *y && p.0 < x + ww && p.1 < y + wh
        })
    }) || w.win_hits.borrow().iter().any(|h| {
        let r = h.1;
        p.0 >= r[0] && p.1 >= r[1] && p.0 <= r[0] + r[2] && p.1 <= r[1] + r[3]
    });
    for (v, id, x, y) in places.into_iter().rev() {
        let (hover, mut ev, se, mod_id) = {
            let vm = &w.lua.vms[v];
            let mut st = vm.st.borrow_mut();
            let Some(win) = st.windows.iter_mut().find(|x| x.id == id) else { continue };
            let (wx, wy) = win.classic_reach();
            if win.drag.is_none() && win.press.is_none() && (p.0 < x || p.1 < y || p.0 >= x + wx || p.1 >= y + wy) {
                // A click off the window closes its open dropdown.
                if (clicked || right)
                    && let Some(d) = win.drop_open()
                {
                    win.drop_close(d);
                }
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
        window::Event::set_covered(&mut ev, covered);
        w.lua.ui_events.extend(ev.into_iter().map(|e| (mod_id.clone(), id.clone(), e)));
        w.sounds.extend(se.into_iter().map(|s| s as u32));
        return hover;
    }
    false
}

impl Host {
    /// The game's key bindings as the client's Input table holds them, for binds.users.
    pub(in crate::client) fn set_game_keys(&self, keys: &[[u16; 3]]) {
        let mut f = self.info.borrow_mut();
        if f.game_keys != keys {
            f.game_keys = keys.to_vec();
        }
    }

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

/// Every mod installed (the mods folder): (id, name, version, enabled in the config, bundled: always false now). With
/// `at` (the character and job in play) a mod kept per character or job is enabled by its switch there.
pub(super) fn installed(
    at: Option<&(String, Option<String>)>,
) -> Vec<(String, String, String, bool, bool, Vec<String>)> {
    let cfg = crate::config::Config::load();
    let (c, j) = at.map_or((None, None), |(c, j)| (Some(c.as_str()), j.as_deref()));
    let enabled = |id: &str, bundled: bool| {
        cfg.mods.iter().find(|r| r.name == id).map_or(bundled, |r| r.active(c, j).unwrap_or(r.enabled))
    };
    let mut out = Vec::new();
    for e in std::fs::read_dir(mods_dir()).into_iter().flatten().flatten() {
        let dir = e.path();
        if let Ok(m) = manifest::read(&dir) {
            let id = m.id.clone();
            let commands = m.commands.keys().cloned().collect();
            out.push((id.clone(), m.name, m.version, enabled(&id, false), false, commands));
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
pub(in crate::client) const PERM: &str = "@permission:";

/// A permission's row label and help line.
fn permission_text(p: &str) -> (&'static str, &'static str) {
    match p {
        "actions" => {
            ("Allow game actions", "Casting, items, equipment and the like when you use its commands or windows.")
        }
        "automation" => {
            ("Allow automation", "Game actions by itself: casting, targeting, moving, trading, answering NPCs.")
        }
        "packets.read" => ("Allow reading packets", "Sees every packet the server sends and the client sends."),
        "packets.write" => ("Allow changing packets", "Changes, blocks and sends packets."),
        "input.capture" => {
            ("Allow taking input", "Takes keys, the mouse and the controller from the game and presses keys.")
        }
        "files.shared" => ("Allow shared files", "Reads and writes files outside its folder, in the shared mod data."),
        "files.outside" => (
            "Allow reading your folders",
            "Reads folders on this computer that you pick or allow, such as a game install.",
        ),
        "net" => ("Allow network", "Connects to the internet (web requests, sockets)."),
        "exec" => ("Allow running programs", "Starts programs on this computer."),
        "clipboard" => ("Allow reading the clipboard", "Reads the text you copied."),
        "multibox" => ("Allow multibox", "Talks to your other game windows on this computer."),
        "ui.native" => (
            "Allow game windows",
            "Opens, hides, moves, scales and reskins the game's windows and panels, and drives them.",
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
    let host = Host::park(&mut w.lua);
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
    let host = Host::park(&mut w.lua);
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
        // Each way, the ids hooked (a hook on no id takes them all).
        let mut ids = [[0u64; 8]; 2];
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            let st = vm.st.borrow();
            fin |= !st.hooks_in.is_empty();
            fout |= !st.hooks_out.is_empty();
            for (way, hooks) in [&st.hooks_in, &st.hooks_out].into_iter().enumerate() {
                for h in hooks {
                    match h.1 {
                        Some(id) => ids[way][(id & 0x1ff) as usize / 64] |= 1 << (id % 64),
                        None => ids[way] = [u64::MAX; 8],
                    }
                }
            }
        }
        w.net.stats.set_hooked(false, &ids[0]);
        w.net.stats.set_hooked(true, &ids[1]);
        let last =
            self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().granted.iter().any(|g| g == "packets.read"));
        let flags = if fin { super::net::HOOK_IN } else { 0 }
            | if fout { super::net::HOOK_OUT } else { 0 }
            | if last { super::net::HOOK_LAST } else { 0 };
        w.net.hooks.store(flags, std::sync::atomic::Ordering::Relaxed);
    }
}
