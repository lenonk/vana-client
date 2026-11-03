//! One mod's Lua VM (docs/lua-mods.md 3.1): its own Lua 5.4 state with a fixed library set, a memory limit, a time
//! budget per call, and every handler run as a coroutine that may sleep.

use super::manifest::Manifest;

use mlua::thread::ThreadStatus;
use mlua::{Function, HookTriggers, IntoLuaMulti, Lua, LuaOptions, MultiValue, RegistryKey, StdLib, Thread, Value, VmState};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Memory a mod may hold.
pub(super) const MEMORY: usize = 64 << 20;
/// A single call past this is stopped; repeated, the mod is suspended.
const HARD_CAP: Duration = Duration::from_millis(100);
/// A call past this is reported once (BG3SE's client default).
const SOFT_CAP: Duration = Duration::from_millis(2);
/// Errors within `ERROR_WINDOW` that suspend a mod.
const ERROR_LIMIT: usize = 20;
const ERROR_WINDOW: Duration = Duration::from_secs(60);

/// The prelude: sleeping yields our token, so the scheduler can tell its own yields from a mod's.
const PRELUDE: &str = r#"
local token = ...
function sleep(seconds) return coroutine.yield(token, "sleep", seconds or 0) end
function sleep_frames(n) return coroutine.yield(token, "frames", n or 1) end
function wait_until(test, timeout) return coroutine.yield(token, "until", test, timeout) end
"#;

/// Where a mod's files come from: its folder, or the binary (a bundled mod, assets/mods).
#[derive(Clone)]
pub(super) enum Source {
    Dir(PathBuf),
    Bundled(&'static [(&'static str, &'static str)]),
}

impl Source {
    pub fn read(&self, rel: &str) -> std::io::Result<String> {
        match self {
            Source::Dir(d) => std::fs::read_to_string(d.join(rel)),
            Source::Bundled(files) => files.iter().find(|f| f.0 == rel).map(|f| f.1.to_string()).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
        }
    }

    pub fn exists(&self, rel: &str) -> bool {
        match self {
            Source::Dir(d) => d.join(rel).is_file(),
            Source::Bundled(files) => files.iter().any(|f| f.0 == rel),
        }
    }
}

pub(super) struct Vm {
    pub manifest: Manifest,
    /// The folder (a bundled mod's is "bundled/<id>"), and where its files are read.
    pub dir: PathBuf,
    pub src: Source,
    pub lua: Lua,
    pub st: Rc<RefCell<ModState>>,
    /// Why the mod stopped (errors, memory, budget): handlers off until reloaded.
    pub suspended: Option<String>,
    token: RegistryKey,
}

#[derive(Default)]
pub(super) struct ModState {
    /// Frames this mod has run (sleepers wake in a later frame than they slept).
    pub frame_no: u64,
    pub handlers: Vec<Handler>,
    pub commands: Vec<(String, RegistryKey)>,
    pub timers: Vec<Timer>,
    pub sleepers: Vec<Sleeper>,
    pub ops: Vec<super::Op>,
    pub next_id: u64,
    pub deadline: Option<Instant>,
    pub stats: Stats,
    pub errors: VecDeque<Instant>,
    pub reported: HashSet<String>,
    /// Lines for the chat log about this mod (errors, budget), printed by the host.
    pub notices: Vec<String>,
    pub save: bool,
    /// Filters (docs/lua-mods.md 3.2): chat lines before they are kept, typed lines before the parser.
    pub chat_filters: Vec<(u64, RegistryKey)>,
    pub typed_filters: Vec<(u64, RegistryKey)>,
    pub filters_dirty: bool,
    /// The user's setting values by key (account and character scopes merged); declared defaults fill the rest.
    pub settings: serde_json::Map<String, serde_json::Value>,
    /// The mod's windows (docs/lua-mods.md 4.10) and each one's handler.
    pub windows: Vec<super::window::Window>,
    pub window_handlers: Vec<(String, RegistryKey)>,
    /// A window opened this phase takes the focus (the host moves it there).
    pub focus_request: Option<String>,
    /// Packet hooks (docs/lua-mods.md 4.7): (handle, packet id or none for all, handler), each direction; packets the
    /// mod injected (whole sub-packets) for the host to pass on.
    pub hooks_in: Vec<(u64, Option<u16>, RegistryKey)>,
    pub hooks_out: Vec<(u64, Option<u16>, RegistryKey)>,
    pub inject_in: Vec<Vec<u8>>,
    pub inject_out: Vec<Vec<u8>>,
    /// The permissions the user granted this mod.
    pub granted: Vec<String>,
    /// The running coroutine began with the player's input (a typed command, a window event): `actions` suffices.
    pub input: bool,
    /// Key and mouse filters (docs/lua-mods.md 4.5), and key binds.
    pub key_filters: Vec<(u64, RegistryKey)>,
    pub mouse_filters: Vec<(u64, RegistryKey)>,
    pub binds: Vec<super::input::Bind>,
    /// Functions other mods may call (mods.provide).
    pub provided: Vec<(String, RegistryKey)>,
    /// The mod was suspended (Vm::suspended): other mods can't run its code through mods.call either.
    pub suspended: bool,
    /// HUD elements (docs/lua-mods.md 4.10) and their click handlers.
    pub hud: Vec<super::hud::Elem>,
    pub hud_handlers: Vec<(String, RegistryKey)>,
    /// World shapes (draw.*).
    pub shapes: Vec<super::draw::Shape>,
    /// HTTP requests in flight (net.request): their callbacks, and the channel their answers come back on.
    pub net_pending: Vec<(u64, RegistryKey)>,
    pub net_rx: Option<std::sync::mpsc::Receiver<super::net::Answer>>,
    pub net_tx: Option<std::sync::mpsc::Sender<super::net::Answer>>,
    /// client.paste's handlers, waiting for the clipboard.
    pub paste_handlers: Vec<RegistryKey>,
    /// Open sockets and their threads' reports.
    pub socks: Vec<super::sock::Sock>,
    pub sock_rx: Option<std::sync::mpsc::Receiver<super::sock::Msg>>,
    pub sock_tx: Option<std::sync::mpsc::Sender<super::sock::Msg>>,
}

impl ModState {
    /// An error: reported once per distinct message, counted toward suspension.
    pub fn record(&mut self, name: &str, e: &mlua::Error) {
        let text = clean(e);
        let now = Instant::now();
        self.errors.push_back(now);
        while self.errors.front().is_some_and(|t| now.duration_since(*t) > ERROR_WINDOW) {
            self.errors.pop_front();
        }
        if self.reported.insert(text.clone()) {
            self.notices.push(format!("{name}: {text}"));
        }
    }
}

/// A filter's budget per call (it runs inside the game's own paths).
pub(super) const FILTER_CAP: Duration = Duration::from_millis(20);

pub(super) struct Handler {
    pub id: u64,
    pub event: String,
    pub func: RegistryKey,
    pub priority: i32,
}

pub(super) struct Timer {
    pub id: u64,
    pub due: Instant,
    pub every: Option<Duration>,
    pub func: RegistryKey,
}

pub(super) struct Sleeper {
    pub thread: RegistryKey,
    pub wake: Wake,
    /// The frame it went to sleep in: it wakes in a later one, after that frame's actions applied.
    pub born: u64,
    /// Begun with the player's input (ModState::input), kept across sleeps.
    pub input: bool,
}

pub(super) enum Wake {
    At(Instant),
    Frames(u32),
    /// A test function, and when to give up.
    Until(RegistryKey, Option<Instant>),
}

/// Time spent in this mod: this frame, a running average and the peak (ms), calls made.
#[derive(Default)]
pub(super) struct Stats {
    pub frame: Duration,
    pub avg: f32,
    pub peak: f32,
    pub calls: u64,
    pub slow: u64,
}

impl Vm {
    pub fn new(manifest: Manifest, dir: PathBuf, src: Source) -> mlua::Result<Self> {
        // The base library always loads; no io, os, package or debug.
        let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE | StdLib::OS, LuaOptions::default())?;
        lua.set_memory_limit(MEMORY)?;
        let g = lua.globals();
        // os: the clock and the date only.
        let os: mlua::Table = g.get("os")?;
        let keep = lua.create_table()?;
        for name in ["time", "date", "clock", "difftime"] {
            keep.raw_set(name, os.get::<Value>(name)?)?;
        }
        g.raw_set("os", keep)?;
        for name in ["dofile", "loadfile"] {
            g.raw_set(name, Value::Nil)?;
        }
        // Text chunks only: no bytecode in, none out.
        let load: Function = g.get("load")?;
        // An env given as nil must stay given (it sets _ENV), so the arguments pass through as they came.
        let text_only = lua.create_function(move |lua, mut args: MultiValue| {
            while args.len() < 3 {
                args.push_back(Value::Nil);
            }
            args[2] = Value::String(lua.create_string("t")?);
            load.call::<MultiValue>(args)
        })?;
        g.raw_set("load", text_only)?;
        let string: mlua::Table = g.get("string")?;
        string.raw_set("dump", Value::Nil)?;
        // Patterns run in C, out of reach of the budget hook: ours count their steps against the deadline.
        super::pattern::install(&lua, &string)?;
        let gc = lua.create_function(|lua, opt: Option<String>| match opt.as_deref().unwrap_or("collect") {
            "count" => Ok(lua.used_memory() as f64 / 1024.0),
            _ => Ok(0.0),
        })?;
        g.raw_set("collectgarbage", gc)?;
        let st = Rc::new(RefCell::new(ModState { next_id: 1, ..Default::default() }));
        lua.set_app_data(st.clone());
        let token_table = lua.create_table()?;
        lua.load(PRELUDE).set_name("=prelude").call::<()>(token_table.clone())?;
        let token = lua.create_registry_value(token_table)?;
        let vm = Self { manifest, dir, src, lua, st, suspended: None, token };
        vm.set_hook_on_main()?;
        Ok(vm)
    }

    fn hook(&self) -> impl Fn(&Lua, &mlua::debug::Debug) -> mlua::Result<VmState> + 'static {
        let st = self.st.clone();
        move |_, _| match st.borrow().deadline {
            Some(d) if Instant::now() > d => Err(mlua::Error::runtime("ran past its time budget")),
            _ => Ok(VmState::Continue),
        }
    }

    fn set_hook_on_main(&self) -> mlua::Result<()> {
        self.lua.set_hook(HookTriggers::new().every_nth_instruction(10_000), self.hook())
    }

    /// Runs the entry file.
    pub fn run_entry(&self) {
        let entry = self.manifest.entry.clone();
        let r = self.src.read(&entry).map_err(|e| mlua::Error::runtime(format!("{entry}: {e}"))).and_then(|src| {
            let f = self.lua.load(src).set_name(format!("@{}", self.manifest.entry)).set_mode(mlua::chunk::ChunkMode::Text).into_function()?;
            Ok(f)
        });
        match r {
            Ok(f) => self.call(f, ()),
            Err(e) => self.error(&e),
        }
    }

    /// Runs `func(args)` as a new coroutine.
    pub fn call(&self, func: Function, args: impl IntoLuaMulti) {
        self.call_as(func, args, false);
    }

    /// As call, for a handler of the player's input (a command typed, a window event): `actions` allows game actions.
    pub fn call_input(&self, func: Function, args: impl IntoLuaMulti) {
        self.call_as(func, args, true);
    }

    fn call_as(&self, func: Function, args: impl IntoLuaMulti, input: bool) {
        if self.suspended.is_some() {
            return;
        }
        match self.lua.create_thread(func) {
            Ok(t) => {
                self.st.borrow_mut().input = input;
                self.resume(t, args);
                self.st.borrow_mut().input = false;
            }
            Err(e) => self.error(&e),
        }
    }

    /// Runs `func(args)` as a coroutine inside a game path (FILTER_CAP): what it returned, when it ended in this slice. One
    /// that sleeps goes on later and its result is lost.
    pub fn call_filter(&self, func: Function, args: impl IntoLuaMulti, input: bool) -> Option<MultiValue> {
        if self.suspended.is_some() {
            return None;
        }
        let t = match self.lua.create_thread(func) {
            Ok(t) => t,
            Err(e) => {
                self.error(&e);
                return None;
            }
        };
        self.st.borrow_mut().input = input;
        let r = self.resume_for(t, args, FILTER_CAP);
        self.st.borrow_mut().input = false;
        r
    }

    /// Resumes `thread`; a yield of ours puts it to sleep, any other waits a frame.
    pub fn resume(&self, thread: Thread, args: impl IntoLuaMulti) {
        self.resume_for(thread, args, HARD_CAP);
    }

    /// As resume, with a time budget; what the thread returned when it ended.
    fn resume_for(&self, thread: Thread, args: impl IntoLuaMulti, cap: Duration) -> Option<MultiValue> {
        if let Err(e) = thread.set_hook(HookTriggers::new().every_nth_instruction(10_000), self.hook()) {
            self.error(&e);
            return None;
        }
        self.st.borrow_mut().deadline = Some(Instant::now() + cap);
        let t0 = Instant::now();
        let r = thread.resume::<MultiValue>(args);
        let dt = t0.elapsed();
        {
            let mut st = self.st.borrow_mut();
            st.deadline = None;
            st.stats.frame += dt;
            st.stats.calls += 1;
            if dt > SOFT_CAP {
                st.stats.slow += 1;
            }
        }
        match r {
            Err(e) => {
                self.error(&e);
                None
            }
            Ok(vals) if thread.status() == ThreadStatus::Resumable => {
                self.park(thread, vals);
                None
            }
            Ok(vals) => Some(vals),
        }
    }

    fn park(&self, thread: Thread, vals: MultiValue) {
        let token: Value = self.lua.registry_value(&self.token).unwrap_or(Value::Nil);
        let mut it = vals.into_iter();
        let ours = it.next().is_some_and(|v| v.to_pointer() == token.to_pointer());
        let wake = if ours {
            let kind = it.next().and_then(|v| v.as_string().map(|s| s.to_string_lossy()));
            match kind.as_deref() {
                Some("sleep") => {
                    let s = it.next().and_then(|v| v.as_number().or(v.as_integer().map(|i| i as f64))).unwrap_or(0.0).max(0.0);
                    Wake::At(Instant::now() + Duration::from_secs_f64(s))
                }
                Some("until") => {
                    let Some(Value::Function(test)) = it.next() else {
                        self.error(&mlua::Error::runtime("wait_until needs a function"));
                        return;
                    };
                    let limit = it.next().and_then(|v| v.as_number().or(v.as_integer().map(|i| i as f64))).map(|s| Instant::now() + Duration::from_secs_f64(s.max(0.0)));
                    match self.lua.create_registry_value(test) {
                        Ok(k) => Wake::Until(k, limit),
                        Err(e) => {
                            self.error(&e);
                            return;
                        }
                    }
                }
                _ => Wake::Frames(it.next().and_then(|v| v.as_integer()).unwrap_or(1).max(1) as u32),
            }
        } else {
            Wake::Frames(1)
        };
        let input = self.st.borrow().input;
        match self.lua.create_registry_value(thread) {
            Ok(k) => {
                let born = self.st.borrow().frame_no;
                self.st.borrow_mut().sleepers.push(Sleeper { thread: k, wake, input, born });
            }
            Err(e) => self.error(&e),
        }
    }

    /// Wakes the sleepers whose time has come.
    pub fn wake(&self) {
        let now = Instant::now();
        let frame = self.st.borrow().frame_no;
        let sleepers = std::mem::take(&mut self.st.borrow_mut().sleepers);
        let mut keep = Vec::new();
        let mut due = Vec::new();
        for mut s in sleepers {
            if s.born == frame {
                keep.push(s);
                continue;
            }
            let ready = match &mut s.wake {
                Wake::At(t) => now >= *t,
                Wake::Frames(n) => {
                    *n = n.saturating_sub(1);
                    *n == 0
                }
                Wake::Until(test, limit) => {
                    let f: Option<Function> = self.lua.registry_value(test).ok();
                    // The test runs on the game thread too: under a budget, so one that loops can't freeze the frame.
                    // An error (the budget's included) is recorded, and the wait ends as before.
                    let passed = f.is_none_or(|f| {
                        let prev = self.st.borrow_mut().deadline.replace(Instant::now() + FILTER_CAP);
                        let r = f.call::<bool>(());
                        self.st.borrow_mut().deadline = prev;
                        r.unwrap_or_else(|e| {
                            self.error(&e);
                            true
                        })
                    });
                    passed || limit.is_some_and(|l| now >= l)
                }
            };
            if ready { due.push(s) } else { keep.push(s) }
        }
        self.st.borrow_mut().sleepers.extend(keep);
        for s in due {
            if let Ok(t) = self.lua.registry_value::<Thread>(&s.thread) {
                self.st.borrow_mut().input = s.input;
                self.resume(t, ());
                self.st.borrow_mut().input = false;
            }
            let _ = self.lua.remove_registry_value(s.thread);
        }
    }

    /// Fires the timers whose time has come.
    pub fn timers(&self) {
        let now = Instant::now();
        let mut fire = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            for t in st.timers.iter_mut() {
                if now >= t.due {
                    fire.push(t.id);
                    if let Some(e) = t.every {
                        t.due = now + e;
                    }
                }
            }
        }
        for id in fire {
            let f = {
                let st = self.st.borrow();
                st.timers.iter().find(|t| t.id == id).and_then(|t| self.lua.registry_value::<Function>(&t.func).ok())
            };
            if let Some(f) = f {
                self.call(f, ());
            }
            let mut st = self.st.borrow_mut();
            if let Some(k) = st.timers.iter().position(|t| t.id == id && t.every.is_none()) {
                let t = st.timers.remove(k);
                drop(st);
                let _ = self.lua.remove_registry_value(t.func);
            }
        }
    }

    /// Calls every handler of `event` with `args` (by priority, then registration order).
    pub fn emit(&self, event: &str, args: impl IntoLuaMulti + Clone) {
        let funcs: Vec<Function> = {
            let st = self.st.borrow();
            let mut hs: Vec<&Handler> = st.handlers.iter().filter(|h| h.event == event).collect();
            hs.sort_by_key(|h| -h.priority);
            hs.iter().filter_map(|h| self.lua.registry_value::<Function>(&h.func).ok()).collect()
        };
        for f in funcs {
            self.call(f, args.clone());
        }
    }

    pub fn has_handler(&self, event: &str) -> bool {
        self.st.borrow().handlers.iter().any(|h| h.event == event)
    }

    /// Reports an error once per distinct message; too many suspend the mod.
    pub fn error(&self, e: &mlua::Error) {
        log::warn!("lua mod {}: {e}", self.manifest.id);
        self.st.borrow_mut().record(&self.manifest.name, e);
    }

    /// Whether the mod should be suspended now (too many errors).
    pub fn failing(&self) -> Option<String> {
        let st = self.st.borrow();
        (st.errors.len() >= ERROR_LIMIT).then(|| format!("{} errors in {} s", st.errors.len(), ERROR_WINDOW.as_secs()))
    }

    /// Ends the frame's accounting: the running average and peak (ms).
    pub fn end_frame(&self) {
        let mut st = self.st.borrow_mut();
        st.frame_no += 1;
        let ms = st.stats.frame.as_secs_f32() * 1000.0;
        st.stats.avg = st.stats.avg * 0.95 + ms * 0.05;
        st.stats.peak = st.stats.peak.max(ms);
        st.stats.frame = Duration::ZERO;
    }
}

/// An error's message without mlua's wrapping ("runtime error: ", the callback's traceback).
fn clean(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(s) | mlua::Error::SyntaxError { message: s, .. } => s.lines().next().unwrap_or("").to_string(),
        mlua::Error::CallbackError { cause, .. } => clean(cause),
        mlua::Error::MemoryError(s) => format!("out of memory ({s})"),
        e => e.to_string().lines().next().unwrap_or("").to_string(),
    }
}
