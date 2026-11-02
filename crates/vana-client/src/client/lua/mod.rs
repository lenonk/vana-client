//! Lua mods (docs/lua-mods.md): a VM per enabled mod with a mod.toml in its folder under mods/, created when the
//! character enters the world and dropped at logout. Once a frame the mod phase delivers events, commands, timers and
//! wake-ups; what the mods asked for is applied after it.

mod actions;
mod api;
mod docs;
pub(super) mod input;
mod manifest;
mod runtime;
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
}

/// The mods of this session.
#[derive(Default)]
pub struct Host {
    vms: Vec<Vm>,
    started: bool,
    /// Events raised by the client since the last mod phase.
    events: Vec<(&'static str, Vec<serde_json::Value>)>,
    /// Lines for mods (a registered command, or /lua), run in the next mod phase, with where they came from.
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
    /// What the UI knows that mods read (the map grid).
    info: std::rc::Rc<std::cell::RefCell<state::FrameInfo>>,
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

    fn char_file(w: &World) -> String {
        let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
        format!("character-{}-{}.json", clean(&w.world_name), clean(&w.name))
    }

    /// Creates the VM of the mod in `dir`, runs its entry file and sends it `load`.
    fn load(&mut self, w: &World, dir: PathBuf) -> Result<(), String> {
        let m = manifest::read(&dir).map_err(|e| format!("{e:#}"))?;
        self.load_with(w, m, dir.clone(), vm::Source::Dir(dir))
    }

    /// A bundled mod (assets/mods) by id.
    fn load_bundled(&mut self, w: &World, id: &str) -> Result<(), String> {
        let (_, text, files) = BUNDLED.iter().find(|b| b.0 == id).ok_or_else(|| format!("no bundled mod {id}"))?;
        let m = manifest::parse(text, id).map_err(|e| format!("{e:#}"))?;
        self.load_with(w, m, PathBuf::from("bundled").join(id), vm::Source::Bundled(files))
    }

    fn load_with(&mut self, w: &World, m: manifest::Manifest, dir: PathBuf, src: vm::Source) -> Result<(), String> {
        if self.vms.iter().any(|v| v.manifest.id == m.id) {
            return Err(format!("{} is already loaded", m.id));
        }
        let data = data_dir().join(&m.id);
        let storage = (read_json(&data.join("account.json")), read_json(&data.join(Self::char_file(w))));
        let settings = load_settings(&m, &data, &Self::char_file(w));
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
        api::install(&vm.lua, &vm, storage).map_err(|e| format!("{e}"))?;
        self.dir.borrow_mut().mods.push(runtime::Entry { id: vm.manifest.id.clone(), name: vm.manifest.name.clone(), version: vm.manifest.version.clone(), lua: vm.lua.weak(), st: vm.st.clone() });
        log::info!("lua mod {} {} loaded from {}", vm.manifest.id, vm.manifest.version, vm.dir.display());
        lend(&vm, w, |vm| {
            vm.run_entry();
            vm.emit("load", ());
        });
        self.vms.push(vm);
        self.filters_dirty = true;
        Ok(())
    }

    /// Sends `unload`, saves its storage and drops the VM.
    fn unload(&mut self, w: &World, k: usize) {
        let vm = self.vms.remove(k);
        self.dir.borrow_mut().mods.retain(|m| m.id != vm.manifest.id);
        self.filters_dirty = true;
        lend(&vm, w, |vm| vm.emit("unload", ()));
        save(&vm, w);
        log::info!("lua mod {} unloaded", vm.manifest.id);
    }

    /// The enabled mods (config `mods`) whose folders have a mod.toml.
    fn start(&mut self, w: &mut World) {
        self.started = true;
        let cfg = crate::config::Config::load();
        let root = mods_dir();
        install_stubs(root.clone());
        // Bundled mods (our own features on the mod API) run unless turned off in `mods` or replaced by a folder.
        for (id, ..) in BUNDLED {
            let off = cfg.mods.iter().any(|r| r.name == *id && !r.enabled);
            let folder = cfg.mods.iter().any(|r| r.name == *id && r.enabled) && root.join(id).join("mod.toml").is_file();
            if off || folder {
                continue;
            }
            if let Err(e) = self.load_bundled(w, id) {
                log::warn!("lua mod {id}: {e}");
                w.chat.push_colour(format!("Mod {id}: {e}"), ERROR);
            }
        }
        for r in cfg.mods.iter().filter(|r| r.enabled) {
            let dir = root.join(&r.name);
            if !dir.join("mod.toml").is_file() {
                continue;
            }
            if let Err(e) = self.load(w, dir) {
                log::warn!("lua mod {}: {e}", r.name);
                w.chat.push_colour(format!("Mod {}: {e}", r.name), ERROR);
            }
        }
        for id in watched() {
            self.set_watch(&id, true);
        }
        let name = w.name.clone();
        self.events.push(("login", vec![name.into()]));
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
    fn load_named(&mut self, w: &World, id: &str) -> Result<(), String> {
        let dir = mods_dir().join(id);
        if !dir.join("mod.toml").is_file() && BUNDLED.iter().any(|b| b.0 == id) { self.load_bundled(w, id) } else { self.load(w, dir) }
    }

    /// Reloads the mod at `k` from its files, in the same place in the load order; its id.
    fn reload(&mut self, w: &World, k: usize) -> Result<String, String> {
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
    fn watch_upkeep(&mut self, w: &World, out: &mut Vec<(String, [u8; 4])>) {
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

    /// Logout: `logout` and `unload` to every mod, storage saved.
    pub fn stop(&mut self, w: &World) {
        for vm in &self.vms {
            lend(vm, w, |vm| vm.emit("logout", ()));
        }
        while !self.vms.is_empty() {
            self.unload(w, 0);
        }
        self.started = false;
        self.prev = None;
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
            "load" | "l" => match args.get(1) {
                Some(id) => match self.load_named(w, id) {
                    Ok(()) => out.push((format!("Loaded {id}."), super::WHITE)),
                    Err(e) => out.push((e, ERROR)),
                },
                None => out.push(("/lua load <mod>".into(), ERROR)),
            },
            "unload" | "u" => match args.get(1).and_then(|id| find(&self.vms, id)) {
                Some(k) => {
                    let id = self.vms[k].manifest.id.clone();
                    self.unload(w, k);
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
                            match self.load_named(w, id) {
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
                    match self.reload(w, k) {
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
    vm.lua.set_app_data(api::WorldRef(w as *const World));
    f(vm);
    vm.lua.remove_app_data::<api::WorldRef>();
}

/// Writes a mod's storage tables.
fn save(vm: &Vm, w: &World) {
    let dir = data_dir().join(&vm.manifest.id);
    let Ok(storage) = vm.lua.globals().get::<mlua::Table>("storage") else { return };
    for (key, file) in [("account", "account.json".to_string()), ("character", Host::char_file(w))] {
        let Ok(t) = storage.get::<mlua::Value>(key) else { continue };
        use mlua::LuaSerdeExt;
        match vm.lua.from_value::<serde_json::Value>(t) {
            Ok(v) => {
                let _ = std::fs::create_dir_all(&dir);
                if let Err(e) = std::fs::write(dir.join(&file), serde_json::to_vec_pretty(&v).unwrap_or_default()) {
                    log::warn!("lua mod {}: saving {file}: {e}", vm.manifest.id);
                }
            }
            Err(e) => log::warn!("lua mod {}: storage.{key} is not plain data: {e}", vm.manifest.id),
        }
    }
}

/// The mod phase (docs/lua-mods.md 3.2): once a frame in the world.
pub(super) fn frame(w: &mut World, gu: Option<&super::game_ui::GameUi>) {
    let mut host = std::mem::take(&mut w.lua);
    if !host.started {
        if w.zone.is_none() || w.name.is_empty() {
            w.lua = host;
            return;
        }
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
    }
    let now = std::time::Instant::now();
    let dt = host.last_frame.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
    host.last_frame = Some(now);
    let mut out: Vec<(String, [u8; 4])> = Vec::new();
    let bind_lines;
    {
        let w: &World = w;
        host.watch_upkeep(w, &mut out);
        bind_lines = input::run_binds(&mut host, w, &mut out);
        // Lines typed for mods.
        for (line, from) in std::mem::take(&mut host.typed) {
            let body = line.trim_start_matches('/');
            let mut words = body.split_whitespace();
            let name = words.next().unwrap_or("").to_ascii_lowercase();
            let args: Vec<&str> = words.collect();
            if name == "lua" {
                // /lua grants, runs code in and unloads mods: only from the player, never from a mod's line or bind.
                if from == From::Player {
                    host.lua_command(w, &args, &mut out);
                } else {
                    out.push(("/lua runs only when typed by the player, not from a mod.".into(), ERROR));
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
        // Events mods sent each other (mods.emit), to every other mod listening.
        let emitted = std::mem::take(&mut host.dir.borrow_mut().emitted);
        for (from, name, args) in &emitted {
            for vm in host.vms.iter().filter(|v| v.suspended.is_none() && v.manifest.id != *from && v.has_handler(name)) {
                lend(vm, w, |vm| {
                    use mlua::LuaSerdeExt;
                    let vals: Vec<mlua::Value> = args.iter().map(|a| vm.lua.to_value(a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                });
            }
        }
        for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
            lend(vm, w, |vm| {
                use mlua::LuaSerdeExt;
                for (name, args) in &events {
                    if !vm.has_handler(name) {
                        continue;
                    }
                    let vals: Vec<mlua::Value> = args.iter().map(|a| vm.lua.to_value(a).unwrap_or(mlua::Value::Nil)).collect();
                    vm.emit(name, mlua::MultiValue::from_iter(vals));
                }
                let mine: Vec<(String, window::Event)> = ui_events.iter().filter(|e| e.0 == vm.manifest.id).map(|e| (e.1.clone(), e.2.clone())).collect();
                for (win, ev) in mine {
                    let f = vm.st.borrow().window_handlers.iter().find(|h| h.0 == win).and_then(|h| vm.lua.registry_value::<mlua::Function>(&h.1).ok());
                    if let Some(f) = f {
                        let t = vm.lua.to_value(&ev.json()).unwrap_or(mlua::Value::Nil);
                        vm.call_input(f, (win, t));
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
        }
        let (notices, vm_ops, save_now) = {
            let mut st = vm.st.borrow_mut();
            (std::mem::take(&mut st.notices), std::mem::take(&mut st.ops), std::mem::take(&mut st.save))
        };
        out.extend(notices.into_iter().map(|n| (n, ERROR)));
        ops.extend(vm_ops);
        if save_now {
            save(vm, w);
        }
    }
    // A window opened this phase takes the focus from every other.
    let wanted: Option<(usize, String)> = host.vms.iter().enumerate().find_map(|(k, v)| v.st.borrow_mut().focus_request.take().map(|id| (k, id)));
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
    /// A mod's key bind the player pressed: the mod's command runs as the player's input, but /lua does not run.
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

fn apply(w: &mut World, gu: Option<&super::game_ui::GameUi>, op: Op) {
    match op {
        Op::Print(text) => w.chat.push_colour(text, DIM),
        Op::Chat { text, mode, colour } => match (mode, colour) {
            (Some(m), _) => {
                let (colour, indent) = w.battle.as_ref().map_or((super::WHITE, false), |b| (b.colour_in(m, &w.game), b.indents(m)));
                w.chat.push_event(text, colour, indent, m);
            }
            (None, Some(c)) => {
                let colour = w.battle.as_ref().map_or(super::WHITE, |b| b.colour_in(c, &w.game));
                w.chat.push_colour(text, colour);
            }
            (None, None) => w.chat.push(text),
        },
        Op::Run(line) => run_line(w, &line, super::textcmd::Mode::Typed, From::Mod),
        Op::Act(a) => actions::apply(w, gu, a),
        Op::Key(k) => w.lua.synth.push(k),
        Op::Copy(text) => w.copied = Some(text),
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
        let p = fit(vec![0x15, 0x02, 0, 0, 1, 2, 3]).unwrap();
        assert_eq!(p.len(), 8);
        assert_eq!(u16::from_le_bytes([p[0], p[1]]), 0x015 | (2 << 9));
        assert_eq!(fit(vec![0; 508]).unwrap().len(), 508);
        assert!(fit(vec![0; 509]).is_err());
        assert!(fit(vec![0; 3]).is_err());
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

    /// A mod can't reach /lua through commands.run (issue #68): it would grant itself permissions.
    #[test]
    fn commands_run_refuses_lua() {
        let vm = Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        for line in ["//lua grant x automation", "/lua run other print(1)", "  //LUA unload other", "lua list"] {
            let r = vm.lua.load(format!("commands.run({line:?})")).exec();
            assert!(r.is_err(), "{line:?} was accepted");
        }
        assert!(vm.st.borrow().ops.is_empty());
        // Other // lines still queue (they need no permission).
        vm.lua.load("commands.run('//mycommand a b')").exec().unwrap();
        assert_eq!(vm.st.borrow().ops.len(), 1);
    }

    /// Every documented function is bound, and every function in a documented namespace is documented.
    #[test]
    fn docs_match_bindings() {
        let vm = Vm::new(manifest::Manifest::default(), PathBuf::from("."), vm::Source::Dir(PathBuf::from("."))).unwrap();
        api::install(&vm.lua, &vm, (serde_json::Value::Null, serde_json::Value::Null)).unwrap();
        let g = vm.lua.globals();
        for f in docs::FUNCS {
            let v: Value = if f.ns.is_empty() { g.get(f.name).unwrap() } else { g.get::<Table>(f.ns).unwrap().get(f.name).unwrap() };
            assert!(matches!(v, Value::Function(_)), "{}.{} is documented but not bound", f.ns, f.name);
        }
        // Every global table but Lua's own is a namespace of ours, documented or not.
        const STD: [&str; 7] = ["_G", "table", "string", "math", "utf8", "coroutine", "os"];
        let namespaces: Vec<String> = g.pairs::<String, Value>().filter_map(|p| p.ok()).filter(|(k, v)| matches!(v, Value::Table(_)) && !STD.contains(&k.as_str())).map(|p| p.0).collect();
        for ns in namespaces {
            let ns = ns.as_str();
            let t: Table = g.get(ns).unwrap();
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
fn load_settings(m: &manifest::Manifest, data: &std::path::Path, char_file: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    let account = read_json(&data.join("settings.json"));
    let character = read_json(&data.join(format!("settings-{char_file}")));
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
        let file = if scope == "character" { dir.join(format!("settings-{}", Self::char_file(w))) } else { dir.join("settings.json") };
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
            use mlua::LuaSerdeExt;
            let v = vm.lua.to_value(&value).unwrap_or(mlua::Value::Nil);
            vm.emit("settings_changed", (key.to_string(), v));
        });
    }
}

impl Host {
    /// Whether a mod window has the keys.
    pub(super) fn focused(&self) -> bool {
        self.vms.iter().any(|v| v.suspended.is_none() && v.st.borrow().windows.iter().any(|w| w.open && w.focus))
    }
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
            out.push(win.draw_classic(gu, x, y));
        }
    }
    out
}

impl Host {
    /// The open windows (mod id, a copy of the window), for the modern UI's panels.
    pub(super) fn open_windows(&self) -> Vec<(String, window::Window)> {
        let mut out = Vec::new();
        for vm in self.vms.iter().filter(|v| v.suspended.is_none()) {
            for win in vm.st.borrow().windows.iter().filter(|w| w.open) {
                out.push((vm.manifest.id.clone(), win.clone()));
            }
        }
        out
    }
}

/// The pointer on a mod window (modern UI): it takes the focus, then the click acts as the window model says.
pub(super) fn window_click(w: &mut World, mod_id: &str, win_id: &str, click: super::modern::ModClick) {
    use super::modern::ModClick as C;
    let host = &mut w.lua;
    let mut out = None;
    for vm in host.vms.iter() {
        let mut st = vm.st.borrow_mut();
        let mine = vm.manifest.id == mod_id;
        let hud = mine && st.windows.iter().any(|w| w.id == win_id && w.hud);
        for win in st.windows.iter_mut() {
            let this = mine && win.id == win_id && win.open;
            if !hud {
                win.focus = this;
            }
            if this {
                out = Some(match click {
                    C::Button(i, c) => win.click_button(i, c),
                    C::Slide(i, s) => win.slide(i, s),
                    C::Row(i, r) => win.pick(i, r),
                });
            }
        }
    }
    if let Some((ev, se)) = out {
        host.ui_events.extend(ev.into_iter().map(|e| (mod_id.to_string(), win_id.to_string(), e)));
        w.sounds.extend(se.into_iter().map(|s| s as u32));
    }
}

/// The classic places of the open windows, in draw order (cascaded from the Config pages' place).
fn classic_places(w: &World) -> Vec<(usize, String, f32, f32)> {
    let mut out = Vec::new();
    let mut k = 0.0;
    for (v, vm) in w.lua.vms.iter().enumerate().filter(|(_, v)| v.suspended.is_none()) {
        for win in vm.st.borrow().windows.iter().filter(|w| w.open) {
            match win.place {
                Some((x, y)) => out.push((v, win.id.clone(), x, y)),
                None => {
                    out.push((v, win.id.clone(), 16.0 + 24.0 * k, 48.0 + 24.0 * k));
                    k += 1.0;
                }
            }
        }
    }
    out
}

/// The pointer on the classic mod windows (the topmost under it); whether it is over a control.
pub(super) fn pointer_classic(w: &mut World, gu: &super::game_ui::GameUi, p: (f32, f32), moved: bool, clicked: bool) -> bool {
    for (v, id, x, y) in classic_places(w).into_iter().rev() {
        let (hover, ev, se, mod_id) = {
            let vm = &w.lua.vms[v];
            let mut st = vm.st.borrow_mut();
            let Some(win) = st.windows.iter_mut().find(|x| x.id == id) else { continue };
            let (wx, wy) = win.classic_size();
            if p.0 < x || p.1 < y || p.0 >= x + wx || p.1 >= y + wy {
                continue;
            }
            let (hover, ev, se) = win.pointer_classic(gu, x, y, p, moved, clicked);
            (hover, ev, se, vm.manifest.id.clone())
        };
        // A click on a window gives it the focus (not a HUD's).
        let hud = w.lua.vms[v].st.borrow().windows.iter().any(|x| x.id == id && x.hud);
        if clicked && !hud {
            for (j, vm) in w.lua.vms.iter().enumerate() {
                for win in vm.st.borrow_mut().windows.iter_mut() {
                    win.focus = j == v && win.id == id && win.open;
                }
            }
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
    /// The stubs in assets/mods/library are what `lua-stubs` writes now (regenerate them after an API change:
    /// `vana-client lua-stubs --out crates/vana-client/assets/mods/library/vana.lua`).
    #[test]
    fn committed_stubs_current() {
        assert_eq!(include_str!("../../../assets/mods/library/vana.lua"), super::stubs(), "assets/mods/library/vana.lua is stale");
    }
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
        "multibox" => ("Allow multibox", "Talks to your other game windows on this computer."),
        "ui.native" => ("Allow game windows", "Opens, hides and drives the game's own windows."),
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

/// The header's size field (bits 9.., in 4-byte words) made to fit `data` (padded to a whole word). An error when it
/// can't: the field is 7 bits, so a sub-packet is at most 508 bytes (issue #69).
pub(super) fn fit(mut data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.len() < 4 {
        return Err(format!("a packet is at least 4 bytes (its header), not {}", data.len()));
    }
    let len = data.len().div_ceil(4) * 4;
    if len > crate::net::map::session::MAX_SUB_LEN {
        return Err(format!("a packet is at most {} bytes (header included), not {len}", crate::net::map::session::MAX_SUB_LEN));
    }
    data.resize(len, 0);
    let id = packet_id(&data);
    data[..2].copy_from_slice(&(id | ((len / 4) as u16) << 9).to_le_bytes());
    Ok(data)
}

/// Every mod's hooks of one direction over one packet, in load order and priority (Windower's chain): each sees the
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
                    Ok(mlua::Value::String(s)) => match fit(s.as_bytes().to_vec()) {
                        Ok(b) => cur = b,
                        Err(e) => vm.error(&mlua::Error::runtime(format!("a packet hook's result was not used: {e}"))),
                    },
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
