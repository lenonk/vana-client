//! One mod's Lua VM (docs/lua-mods.md 3.1): its Lua 5.4 state with a fixed library set, a memory limit, a time
//! budget per call, and every handler run as a coroutine that may sleep.

use super::manifest::Manifest;

use mlua::thread::ThreadStatus;
use mlua::{
    Function, HookTriggers, IntoLuaMulti, Lua, LuaOptions, MultiValue, RegistryKey, StdLib, Thread, Value, VmState,
};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Memory a mod may hold.
pub(super) const MEMORY: usize = 64 << 20;
/// A single call past this is stopped; repeated, the mod is suspended.
const HARD_CAP: Duration = Duration::from_millis(100);
/// Distinct error texts a mod's state remembers as reported (ModState::record).
const REPORTED_KEPT: usize = 256;
/// A call past this is reported once (BG3SE's client default).
const SOFT_CAP: Duration = Duration::from_millis(2);
/// Errors within `ERROR_WINDOW` that suspend a mod.
const ERROR_LIMIT: usize = 20;
const ERROR_WINDOW: Duration = Duration::from_secs(60);

/// The prelude: sleeping yields our token, so the scheduler can tell its yields from a mod's. A `__gc`
/// finalizer runs with hooks off (lgc.c GCTM), out of the budget's reach, so setmetatable refuses one.
const PRELUDE: &str = r#"
local token = ...
function sleep(seconds) return coroutine.yield(token, "sleep", seconds or 0) end
function sleep_frames(n) return coroutine.yield(token, "frames", n or 1) end
function wait_until(test, timeout) return coroutine.yield(token, "until", test, timeout) end
local setmt, rawget, type, error = setmetatable, rawget, type, error
function setmetatable(t, mt)
  if type(mt) == "table" and rawget(mt, "__gc") ~= nil then
    error("setmetatable: __gc finalizers are not allowed (they would run outside the time budget)", 2)
  end
  return setmt(t, mt)
end
"#;

/// Where a mod's files come from: its folder, or the binary (a bundled mod, assets/mods).
#[derive(Clone)]
pub(super) enum Source {
    Dir(PathBuf),
    #[cfg_attr(not(test), allow(dead_code))]
    Bundled(&'static [(&'static str, &'static str)]),
}

/// A path a mod gave that names something outside its folder: absolute (a root or a drive), or with a ".." part. A
/// name with two dots inside one part ("v1..2.wav") stays inside, so it passes.
pub(super) fn outside(rel: &str) -> bool {
    rel.starts_with(['/', '\\']) || rel.contains(':') || rel.split(['/', '\\']).any(|p| p == "..")
}

/// A file under the mod's folder `d`: a relative path, and no link below `d` (one could lead outside it).
pub(super) fn inside(d: &std::path::Path, rel: &str) -> std::io::Result<PathBuf> {
    let refuse = |why: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("{rel:?}: {why}"));
    let mut p = d.to_path_buf();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return Err(refuse("outside the mod's folder")),
            s if s.contains(':') => return Err(refuse("not a relative path")),
            s => p.push(s),
        }
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(refuse("a link, which could lead outside the mod's folder"));
        }
    }
    Ok(p)
}

impl Source {
    pub fn read(&self, rel: &str) -> std::io::Result<String> {
        match self {
            Source::Dir(d) => std::fs::read_to_string(inside(d, rel)?),
            Source::Bundled(files) => files
                .iter()
                .find(|f| f.0 == rel)
                .map(|f| f.1.to_string())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
        }
    }

    /// A file's bytes, text or not (fs.read).
    pub fn read_bytes(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        match self {
            Source::Dir(d) => std::fs::read(inside(d, rel)?),
            Source::Bundled(files) => files
                .iter()
                .find(|f| f.0 == rel)
                .map(|f| f.1.as_bytes().to_vec())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
        }
    }

    pub fn exists(&self, rel: &str) -> bool {
        match self {
            Source::Dir(d) => inside(d, rel).is_ok_and(|p| p.is_file()),
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
    /// Suspended for its errors (the Vm's flag, seen by other mods' calls).
    pub suspended: bool,
    /// The character storage file it loaded (saved there even when the client closes without a world).
    pub char_file: Option<String>,
    /// Keys it holds down for the game (input.send), let go when it unloads.
    pub keys_down: Vec<winit::keyboard::KeyCode>,
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
    /// `reported` in the order its texts came, so the oldest goes first once it holds REPORTED_KEPT.
    reported_order: VecDeque<String>,
    /// Lines for the chat log about this mod (errors, budget), printed by the host.
    pub notices: Vec<String>,
    pub save: bool,
    /// Filters (docs/lua-mods.md 3.2): chat lines before they are kept, typed lines before the parser.
    pub chat_filters: Vec<(u64, RegistryKey)>,
    pub typed_filters: Vec<(u64, RegistryKey)>,
    /// Battle messages with their data, before they print (filters.battle).
    pub battle_filters: Vec<(u64, RegistryKey)>,
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
    pub pad_filters: Vec<(u64, RegistryKey)>,
    pub binds: Vec<super::input::Bind>,
    /// Functions other mods may call (mods.provide).
    pub provided: Vec<(String, RegistryKey)>,
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
        // Errors that carry values are each new: the oldest texts go past a few hundred rather than the set grow
        // (emptying it all at once would report a steady error again after every few hundred others).
        if self.reported.insert(text.clone()) {
            self.reported_order.push_back(text.clone());
            if self.reported_order.len() > REPORTED_KEPT
                && let Some(old) = self.reported_order.pop_front()
            {
                self.reported.remove(&old);
            }
            self.notices.push(format!("{name}: {text}"));
        }
    }
}

/// A filter's budget per call (it runs inside the game's paths).
pub(super) const FILTER_CAP: Duration = Duration::from_millis(20);

/// The client's start, for os.clock.
fn process_start() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

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

/// A wait longer than an Instant holds (`sleep(math.huge)`, `timer.after(1/0, f)`): a century, which is never.
pub(super) const NEVER: Duration = Duration::from_secs(100 * 365 * 24 * 3600);

/// A mod's number of seconds as a Duration, at most NEVER (infinite and huge values too; negative and NaN: none).
pub(super) fn secs(s: f64) -> Duration {
    Duration::try_from_secs_f64(s.max(0.0)).map_or(NEVER, |d| d.min(NEVER))
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
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE | StdLib::OS,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(MEMORY)?;
        let g = lua.globals();
        // os: the clock and the date only.
        let os: mlua::Table = g.get("os")?;
        let keep = lua.create_table()?;
        for name in ["time", "date", "difftime"] {
            keep.raw_set(name, os.get::<Value>(name)?)?;
        }
        // os.clock as wall time (seconds on a monotonic clock since the client started): C's clock() is the process's
        // CPU time, which runs at its pace in a many-threaded client and broke mods' countdowns.
        keep.raw_set("clock", lua.create_function(|_, ()| Ok(process_start().elapsed().as_secs_f64()))?)?;
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
        vm.set_hook()?;
        Ok(vm)
    }

    /// The time budget's check (a state borrowed elsewhere just lets the code run on to the next check).
    fn hook(&self) -> impl Fn(&Lua, &mlua::debug::Debug) -> mlua::Result<VmState> + 'static {
        let st = self.st.clone();
        move |_, _| match st.try_borrow().ok().and_then(|s| s.deadline) {
            Some(d) if Instant::now() > d => Err(mlua::Error::runtime("ran past its time budget")),
            _ => Ok(VmState::Continue),
        }
    }

    /// One hook for every thread: ours and the coroutines a mod makes itself, which a per-thread hook missed.
    fn set_hook(&self) -> mlua::Result<()> {
        self.lua.set_global_hook(HookTriggers::new().every_nth_instruction(10_000), self.hook())
    }

    /// Runs the entry file.
    pub fn run_entry(&self) {
        let entry = self.manifest.entry.clone();
        let r = self.src.read(&entry).map_err(|e| mlua::Error::runtime(format!("{entry}: {e}"))).and_then(|src| {
            let f = self
                .lua
                .load(src)
                .set_name(format!("@{}", self.manifest.entry))
                .set_mode(mlua::chunk::ChunkMode::Text)
                .into_function()?;
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
                    let s = it
                        .next()
                        .and_then(|v| v.as_number().or(v.as_integer().map(|i| i as f64)))
                        .unwrap_or(0.0)
                        .max(0.0);
                    Wake::At(Instant::now() + secs(s))
                }
                Some("until") => {
                    let Some(Value::Function(test)) = it.next() else {
                        self.error(&mlua::Error::runtime("wait_until needs a function"));
                        return;
                    };
                    let limit = it
                        .next()
                        .and_then(|v| v.as_number().or(v.as_integer().map(|i| i as f64)))
                        .map(|s| Instant::now() + secs(s));
                    match self.lua.create_registry_value(test) {
                        Ok(k) => Wake::Until(k, limit),
                        Err(e) => {
                            self.error(&e);
                            return;
                        }
                    }
                }
                // A whole float (2.0) counts as its integer.
                _ => Wake::Frames(
                    it.next()
                        .and_then(|v| v.as_integer().or(v.as_number().filter(|n| n.fract() == 0.0).map(|n| n as i64)))
                        .unwrap_or(1)
                        .clamp(1, u32::MAX as i64) as u32,
                ),
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
        let mut ended = Vec::new();
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
                    // The test runs on the game thread: under the filter budget, counted as the mod's time.
                    let passed = match self.lua.registry_value::<Function>(test) {
                        Ok(f) => {
                            let prev = self.st.borrow_mut().deadline.replace(Instant::now() + FILTER_CAP);
                            let t0 = Instant::now();
                            let r = f.call::<bool>(());
                            let mut st = self.st.borrow_mut();
                            st.deadline = prev;
                            st.stats.frame += t0.elapsed();
                            r
                        }
                        Err(_) => Ok(true),
                    };
                    match passed {
                        Ok(p) => p || limit.is_some_and(|l| now >= l),
                        // A test that fails (its budget included) ends the handler.
                        Err(e) => {
                            self.error(&e);
                            ended.push(s);
                            continue;
                        }
                    }
                }
            };
            if ready { due.push(s) } else { keep.push(s) }
        }
        self.st.borrow_mut().sleepers.extend(keep);
        for s in ended {
            let _ = self.lua.remove_registry_value(s.thread);
        }
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
            // Highest first (a negation would wrap for i32::MIN); stable for equal priorities.
            hs.sort_by_key(|h| std::cmp::Reverse(h.priority));
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
        mlua::Error::RuntimeError(s) | mlua::Error::SyntaxError { message: s, .. } => {
            s.lines().next().unwrap_or("").to_string()
        }
        mlua::Error::CallbackError { cause, .. } => clean(cause),
        mlua::Error::MemoryError(s) => format!("out of memory ({s})"),
        e => e.to_string().lines().next().unwrap_or("").to_string(),
    }
}

#[cfg(test)]
mod tests {
    /// Two dots inside a name stay in the mod's folder; a ".." part, a root or a drive do not.
    #[test]
    fn outside_paths() {
        for p in ["sounds/v1..2.wav", "a..b.png", "./x/y.png", "x/.../y"] {
            assert!(!super::outside(p), "{p}");
        }
        for p in ["../x.wav", "a/../../b", "a\\..\\b", "/etc/x", "\\x", "C:x", "C:\\x"] {
            assert!(super::outside(p), "{p}");
        }
    }
}
