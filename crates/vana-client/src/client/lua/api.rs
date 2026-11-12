//! The globals a mod sees (docs/lua-mods.md 4). Reads go through the world the host lends for the mod phase; writes
//! are queued as ops and applied after it, through the paths keys and text commands take.

use super::vm::{Handler, ModState, Timer};
use super::{Op, World, ipc};
use mlua::{FromLua, Function, Lua, LuaSerdeExt, Table, Value, Variadic};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// The `timers` namespace: calls to the Timers mod's `add`, `remove` and `clear` with the calling mod's id; quiet while
/// that mod is not running.
const TIMERS_SHIM: &str = r#"
local function send(name, ...)
  local ok, err = pcall(mods.call, "timers", name, mod.id, ...)
  if ok then return end
  local e = tostring(err)
  if e:find("no mod timers loaded", 1, true) or e:find("is suspended", 1, true) or e:find("provides no", 1, true) then
    return
  end
  error(e, 3)
end
return {
  add = function(name, seconds, options) send("add", name, seconds, options) end,
  remove = function(name) send("remove", name) end,
  clear = function() send("clear") end,
}
"#;

/// The world, lent while the host runs a mod's Lua (null otherwise).
pub(super) struct WorldRef(pub *const World);

pub(super) fn world(lua: &Lua) -> mlua::Result<&World> {
    let p = lua.app_data_ref::<WorldRef>().map_or(std::ptr::null(), |r| r.0);
    if p.is_null() {
        return Err(mlua::Error::runtime("game state is not available here (in the lobby, or outside the mod phase)"));
    }
    // The host sets the pointer from a live &World for the length of the call and clears it after.
    Ok(unsafe { &*p })
}

/// The game UI's data (augment tables and texts...), lent for the mod phase (null otherwise).
pub(super) struct GuRef(pub *const super::super::game_ui::GameUi);

pub(super) fn gu(lua: &Lua) -> mlua::Result<&super::super::game_ui::GameUi> {
    let p = lua.app_data_ref::<GuRef>().map_or(std::ptr::null(), |r| r.0);
    if p.is_null() {
        return Err(mlua::Error::runtime("the game's UI data is not available here (only in the mod phase)"));
    }
    // The host sets the pointer from a live &GameUi for the mod phase and clears it after.
    Ok(unsafe { &*p })
}

/// A language's name tables: the world's, else loaded once on a thread (none until they are in).
fn names_in(w: &World, lang: crate::dat::items::Lang) -> Option<std::sync::Arc<crate::dat::names::Names>> {
    use crate::dat::names::Names;
    use std::sync::{Arc, Mutex};
    type Slot = Option<Option<Arc<Names>>>;
    static LOADED: Mutex<Vec<(crate::dat::items::Lang, Arc<Mutex<Slot>>)>> = Mutex::new(Vec::new());
    if let Some(n) = w.names.as_ref().filter(|n| n.lang == lang) {
        return Some(n.clone());
    }
    let mut all = LOADED.lock().ok()?;
    if let Some((_, s)) = all.iter().find(|e| e.0 == lang) {
        return s.lock().ok()?.clone().flatten();
    }
    let index = w.index.clone()?;
    let slot: Arc<Mutex<Slot>> = Arc::default();
    all.push((lang, slot.clone()));
    std::thread::spawn(move || {
        let n = Names::load(&index, lang).map_err(|e| log::warn!("lua: the {lang:?} names: {e:#}")).ok().map(Arc::new);
        if let Ok(mut s) = slot.lock() {
            *s = Some(n);
        }
    });
    None
}

/// A Lua value as plain JSON data (storage, json.encode, values between mods): a table with keys 1..n is an array,
/// any other an object whose number and boolean keys become text; functions and other non-data values are left out
/// and named in `skipped`.
pub(super) fn plain(v: &Value, path: &str, skipped: &mut Vec<String>) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    fn go(v: &Value, path: &str, depth: usize, skipped: &mut Vec<String>) -> Result<J, String> {
        if depth > 64 {
            return Err(format!("{path}: tables more than 64 deep (a table inside itself?)"));
        }
        Ok(match v {
            Value::Nil => J::Null,
            Value::LightUserData(p) if p.0.is_null() => J::Null,
            Value::Boolean(b) => J::Bool(*b),
            Value::Integer(i) => J::from(*i),
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(J::Number)
                .ok_or_else(|| format!("{path}: {n} is not a finite number"))?,
            Value::String(s) => {
                J::String(s.to_str().map_err(|_| format!("{path}: a string that is not text (UTF-8)"))?.to_string())
            }
            Value::Table(t) => {
                let pairs: Vec<(Value, Value)> = t.pairs::<Value, Value>().filter_map(|p| p.ok()).collect();
                let n = pairs.len();
                let seq =
                    n > 0 && pairs.iter().all(|(k, _)| matches!(k, Value::Integer(i) if *i >= 1 && (*i as usize) <= n));
                if seq {
                    let mut out = vec![J::Null; n];
                    for (k, val) in &pairs {
                        let Value::Integer(i) = k else { continue };
                        out[*i as usize - 1] = go(val, &format!("{path}[{i}]"), depth + 1, skipped)?;
                    }
                    J::Array(out)
                } else {
                    let mut out = serde_json::Map::new();
                    for (k, val) in &pairs {
                        let key = match k {
                            Value::String(s) => {
                                s.to_str().map_err(|_| format!("{path}: a key that is not text"))?.to_string()
                            }
                            Value::Integer(i) => i.to_string(),
                            Value::Number(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", *x as i64),
                            Value::Number(x) => x.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => {
                                skipped.push(format!("{path}: a key that is not text or a number"));
                                continue;
                            }
                        };
                        let sub = format!("{path}.{key}");
                        if matches!(
                            val,
                            Value::Function(_) | Value::Thread(_) | Value::UserData(_) | Value::LightUserData(_)
                        ) && !matches!(val, Value::LightUserData(p) if p.0.is_null())
                        {
                            skipped.push(sub);
                            continue;
                        }
                        out.insert(key, go(val, &sub, depth + 1, skipped)?);
                    }
                    J::Object(out)
                }
            }
            _ => {
                skipped.push(path.to_string());
                J::Null
            }
        })
    }
    go(v, path, 0, skipped)
}

/// A serde value (JSON) as Lua, null as nil.
pub(super) fn to_lua(lua: &Lua, v: &(impl serde::Serialize + ?Sized)) -> mlua::Result<Value> {
    lua.to_value_with(
        v,
        mlua::serde::SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false),
    )
}

/// A mod's table to read while a state is borrowed: itself when neither it nor a table in it has a metatable, else a
/// copy with the fields its `__index` tables give folded in (an `__index` function's cannot be listed: logged once a
/// mod). A metamethod run during the read could reach the borrowed state again: a panic. Every table reached is copied
/// once and every reference to it takes that copy (a table named twice, or a cycle, must not keep the original).
pub(super) fn unhooked(lua: &Lua, t: &Table) -> mlua::Result<Table> {
    use std::collections::{HashMap, HashSet};
    fn meta(t: &Table, seen: &mut HashSet<usize>) -> bool {
        if !seen.insert(t.to_pointer() as usize) {
            return false;
        }
        t.metatable().is_some()
            || t.pairs::<Value, Value>().flatten().any(|(_, v)| matches!(&v, Value::Table(n) if meta(n, seen)))
    }
    fn copy(lua: &Lua, t: &Table, copies: &mut HashMap<usize, Table>, lost: &mut bool) -> mlua::Result<Table> {
        if let Some(c) = copies.get(&(t.to_pointer() as usize)) {
            return Ok(c.clone());
        }
        let out = lua.create_table()?;
        copies.insert(t.to_pointer() as usize, out.clone());
        let mut src = Some(t.clone());
        // The table, then its __index tables in turn (a few: a class chain); its fields first.
        for _ in 0..8 {
            let Some(s) = src.take() else { break };
            for pair in s.pairs::<Value, Value>() {
                let (k, v) = pair?;
                if out.raw_get::<Value>(k.clone())?.is_nil() {
                    let v = match v {
                        Value::Table(n) => Value::Table(copy(lua, &n, copies, lost)?),
                        v => v,
                    };
                    out.raw_set(k, v)?;
                }
            }
            let index = s.metatable().map_or(Value::Nil, |m| m.raw_get::<Value>("__index").unwrap_or(Value::Nil));
            *lost |= matches!(index, Value::Function(_));
            src = match index {
                Value::Table(n) => Some(n),
                _ => None,
            };
        }
        Ok(out)
    }
    if !meta(t, &mut HashSet::new()) {
        return Ok(t.clone());
    }
    let mut lost = false;
    let out = copy(lua, t, &mut HashMap::new(), &mut lost)?;
    // Fields an __index function would give are gone from the copy: said once, so a mod's author sees why.
    if lost && lua.app_data_ref::<WarnedIndexFn>().is_none() {
        let _ = lua.set_app_data(WarnedIndexFn);
        log::warn!("lua mod: a table given to the UI has an __index function; the fields it gives are not read");
    }
    Ok(out)
}

/// `unhooked` has warned this mod about an `__index` function.
struct WarnedIndexFn;

pub(super) fn state(lua: &Lua) -> Rc<RefCell<ModState>> {
    lua.app_data_ref::<Rc<RefCell<ModState>>>().expect("vm state").clone()
}

/// Queues an op for after the mod phase; in the lobby only lines, keys and lobby actions are carried out.
pub(super) fn push_op(lua: &Lua, op: Op) -> mlua::Result<()> {
    if !matches!(op, Op::Print(_) | Op::Chat { .. } | Op::Key(_) | Op::Lobby(_) | Op::Ipc(..))
        && super::lobby::in_lobby(lua)
    {
        return Err(mlua::Error::runtime("not available in the lobby (before login)"));
    }
    state(lua).borrow_mut().ops.push(op);
    Ok(())
}

pub(super) fn next_id(st: &Rc<RefCell<ModState>>) -> u64 {
    let mut s = st.borrow_mut();
    s.next_id += 1;
    s.next_id
}

/// Every global, for the mod `vm`.
pub(super) fn install(
    lua: &Lua,
    vm: &super::vm::Vm,
    storage: (serde_json::Value, serde_json::Value),
) -> mlua::Result<()> {
    let g = lua.globals();
    let m = &vm.manifest;

    let me = lua.create_table()?;
    me.set("id", m.id.as_str())?;
    me.set("name", m.name.as_str())?;
    me.set("version", m.version.as_str())?;
    me.set("api", super::manifest::API)?;
    me.set("path", vm.dir.to_string_lossy().as_ref())?;
    g.set("mod", me)?;

    // print and log: the chat log (dim) and vana-client.log.
    let id = m.id.clone();
    g.set(
        "print",
        lua.create_function(move |lua, args: Variadic<Value>| {
            let text =
                args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect::<Vec<_>>().join("\t");
            log::info!("lua {id}: {text}");
            push_op(lua, Op::Print(text))?;
            Ok(())
        })?,
    )?;
    let log = lua.create_table()?;
    for (name, level) in [
        ("info", log::Level::Info),
        ("warn", log::Level::Warn),
        ("error", log::Level::Error),
        ("debug", log::Level::Debug),
    ] {
        let id = m.id.clone();
        log.set(
            name,
            lua.create_function(move |_, text: String| {
                log::log!(level, "lua {id}: {text}");
                Ok(())
            })?,
        )?;
    }
    g.set("log", log)?;

    // require: the mod's files (a.b -> a/b.lua or a/b/init.lua), loaded once.
    let src = vm.src.clone();
    let loaded = lua.create_table()?;
    g.set(
        "require",
        lua.create_function(move |lua, name: String| {
            if let Ok(v) = loaded.raw_get::<Value>(name.as_str())
                && !v.is_nil()
            {
                return Ok(v);
            }
            // No way out of the folder: no "..", no root or leading dot, no drive or backslash path.
            if name.contains("..") || name.starts_with(['/', '.']) || name.contains(['\\', ':']) {
                return Err(mlua::Error::runtime(format!("require {name:?}: outside the mod's folder")));
            }
            let rel = name.replace('.', "/");
            let file = [format!("{rel}.lua"), format!("{rel}/init.lua")]
                .into_iter()
                .find(|p| src.exists(p))
                .ok_or_else(|| mlua::Error::runtime(format!("module {name:?} not found in the mod's folder")))?;
            let text = src.read(&file).map_err(|e| mlua::Error::runtime(format!("{file}: {e}")))?;
            let f =
                lua.load(text).set_name(format!("@{file}")).set_mode(mlua::chunk::ChunkMode::Text).into_function()?;
            let v: Value = f.call(name.as_str())?;
            let v = if v.is_nil() { Value::Boolean(true) } else { v };
            loaded.raw_set(name.as_str(), v.clone())?;
            Ok(v)
        })?,
    )?;

    // events.on(name, fn[, {priority}]) -> id; events.off(id).
    let events = lua.create_table()?;
    events.set(
        "on",
        lua.create_function(|lua, (event, func, opts): (String, Function, Option<Table>)| {
            let st = state(lua);
            let id = next_id(&st);
            let priority = opts.and_then(|o| o.get::<i32>("priority").ok()).unwrap_or(0);
            let func = lua.create_registry_value(func)?;
            st.borrow_mut().handlers.push(Handler { id, event, func, priority });
            Ok(id)
        })?,
    )?;
    events.set(
        "off",
        lua.create_function(|lua, id: u64| {
            let st = state(lua);
            let removed: Vec<Handler> = {
                let mut s = st.borrow_mut();
                let (gone, keep): (Vec<_>, Vec<_>) =
                    std::mem::take(&mut s.handlers).into_iter().partition(|h| h.id == id);
                s.handlers = keep;
                gone
            };
            for h in removed {
                lua.remove_registry_value(h.func)?;
            }
            Ok(())
        })?,
    )?;
    g.set("events", events)?;

    // commands.register(name, fn): //name and /name; commands.run(line): as if typed.
    let commands = lua.create_table()?;
    commands.set(
        "register",
        lua.create_function(|lua, (name, func): (String, Function)| {
            let name = name.trim_start_matches('/').to_ascii_lowercase();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(mlua::Error::runtime(format!("command name {name:?}")));
            }
            // A mod command never hides a game command or /lua (the game's lines and actions would reach the mod).
            if name == "lua" || super::super::textcmd::is_game_command(&name) {
                return Err(mlua::Error::runtime(format!("/{name} is a game command: give yours another name")));
            }
            // //mods is the client's list of the mods' commands.
            if name == "mods" {
                return Err(mlua::Error::runtime("//mods is the client's: give yours another name"));
            }
            let st = state(lua);
            let key = lua.create_registry_value(func)?;
            let mut s = st.borrow_mut();
            s.commands.retain(|(n, _)| *n != name);
            s.commands.push((name, key));
            Ok(())
        })?,
    )?;
    commands.set(
        "run",
        lua.create_function(|lua, line: String| {
            if !line.starts_with("//") {
                need_action(lua)?;
            }
            if super::controls_mods(&line) {
                need(lua, "mods")?;
            }
            let (mods, input) = {
                let s = state(lua);
                let s = s.borrow();
                (s.granted.iter().any(|g| g == "mods"), s.input)
            };
            push_op(lua, Op::Run(line, mods, input))?;
            Ok(())
        })?,
    )?;
    // commands.usage(name): the command's declared forms (mod.toml [commands]) as lines to print.
    let declared = m.commands.clone();
    commands.set(
        "usage",
        lua.create_function(move |lua, name: String| {
            let name = name.trim_start_matches('/').to_ascii_lowercase();
            let t = lua.create_table()?;
            if let Some((_, c)) = declared.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                for (a, _) in c.forms() {
                    t.push(if a.is_empty() { format!("//{name}") } else { format!("//{name} {a}") })?;
                }
            }
            Ok(t)
        })?,
    )?;
    commands.set(
        "is_game",
        lua.create_function(|_, name: String| {
            Ok(super::super::textcmd::is_game_command(&name.trim_start_matches('/').to_ascii_lowercase()))
        })?,
    )?;
    g.set("commands", commands)?;

    // filters.chat / typed / battle(fn) -> id; filters.off(id). Run inline: no sleeping; chat filters have no game state.
    let filters = lua.create_table()?;
    for (name, kind) in [("chat", 0), ("typed", 1), ("battle", 2)] {
        filters.set(
            name,
            lua.create_function(move |lua, func: Function| {
                let st = state(lua);
                let id = next_id(&st);
                let key = lua.create_registry_value(func)?;
                let mut s = st.borrow_mut();
                match kind {
                    0 => s.chat_filters.push((id, key)),
                    1 => s.typed_filters.push((id, key)),
                    _ => s.battle_filters.push((id, key)),
                }
                s.filters_dirty = true;
                Ok(id)
            })?,
        )?;
    }
    filters.set(
        "off",
        lua.create_function(|lua, id: u64| {
            let st = state(lua);
            let gone: Vec<mlua::RegistryKey> = {
                let mut guard = st.borrow_mut();
                let s: &mut ModState = &mut guard;
                let mut gone = Vec::new();
                for list in [&mut s.chat_filters, &mut s.typed_filters, &mut s.battle_filters] {
                    let (g, keep): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(|(i, _)| *i == id);
                    *list = keep;
                    gone.extend(g.into_iter().map(|(_, k)| k));
                }
                s.filters_dirty = true;
                gone
            };
            for k in gone {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    g.set("filters", filters)?;

    // ui: the mod's windows (docs/lua-mods.md 4.10), drawn by the active UI.
    let ui = lua.create_table()?;
    ui.set(
        "window",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            let spec = unhooked(lua, &spec)?;
            let st = state(lua);
            let mut s = st.borrow_mut();
            let k = match s.windows.iter().position(|w| w.id == id) {
                Some(k) => k,
                None => {
                    s.windows.push(super::window::Window::new(&id));
                    s.windows.len() - 1
                }
            };
            let w = &mut s.windows[k];
            w.title = spec.get::<Option<String>>("title")?.unwrap_or_default();
            w.width = spec.get::<Option<f32>>("width")?.unwrap_or(0.0);
            w.help = spec.get::<Option<String>>("help")?.unwrap_or_default();
            w.hud = spec.get::<Option<bool>>("hud")?.unwrap_or(false);
            w.framed = spec.get::<Option<bool>>("frame")?.unwrap_or(true);
            w.place = match (spec.get::<Option<f32>>("x")?, spec.get::<Option<f32>>("y")?) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            };
            // An edge window is a HUD one: it never takes the keys.
            w.edge = spec.get::<Option<bool>>("edge")?.unwrap_or(false);
            w.hud |= w.edge;
            w.heading = spec.get::<Option<String>>("heading")?.unwrap_or_default();
            w.resize = spec.get::<Option<bool>>("resize")?.unwrap_or(false);
            w.grow_up = spec.get::<Option<String>>("grow")?.as_deref() == Some("up");
            w.fit = spec.get::<Option<bool>>("fit")?.unwrap_or(false);
            w.panel = spec.get::<Option<String>>("panel")?;
            if let Some(p) = w.panel.as_deref() {
                let (list, kind) = if w.edge {
                    (&super::EDGE_PANELS[..], "edge panel")
                } else {
                    (&super::STAND_IN_PANELS[..], "panel")
                };
                if !list.contains(&p) {
                    return Err(mlua::Error::runtime(format!("no {kind} {p:?} ({})", list.join(", "))));
                }
            }
            w.popup = spec.get::<Option<bool>>("popup")?.unwrap_or(false);
            w.modal = w.popup || spec.get::<Option<bool>>("modal")?.unwrap_or(false);
            // A popup opens at the pointer.
            w.anchor = w.popup.then(|| super::input::cursor(lua));
            w.animate = spec.get::<Option<bool>>("animate")?.unwrap_or(false);
            // Its opacity (per-window opacity), kept with the window's key.
            let opacity = spec.get::<Option<f32>>("opacity")?;
            drop(s);
            set_opacity(lua, &format!("mod:{}:{id}", mod_id(lua)?), opacity);
            let mut s = st.borrow_mut();
            let w = &mut s.windows[k];
            let elems: Option<Table> = spec.get("elements")?;
            let parsed: Vec<(super::window::Elem, super::window::Meta)> = match elems {
                Some(t) => {
                    t.sequence_values::<Table>().map(|e| e.and_then(|e| elem(&e))).collect::<mlua::Result<_>>()?
                }
                None => Vec::new(),
            };
            let (elems, meta) = parsed.into_iter().unzip();
            w.set_elems(elems, meta);
            Ok(())
        })?,
    )?;
    ui.set(
        "update",
        lua.create_function(|lua, (id, eid, fields): (String, String, Table)| {
            let fields = unhooked(lua, &fields)?;
            let st = state(lua);
            let mut s = st.borrow_mut();
            let w = s
                .windows
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
            let k = w
                .elems
                .iter()
                .position(|e| e.id() == Some(eid.as_str()))
                .ok_or_else(|| mlua::Error::runtime(format!("no element {eid:?} in {id:?}")))?;
            update(&mut w.elems[k], &fields)?;
            w.meta.resize(w.elems.len(), Default::default());
            // A key element starts listening (the cursor goes there).
            if fields.get::<Option<bool>>("listen")? == Some(true)
                && matches!(w.elems[k], super::window::Elem::Input { key: true, .. })
            {
                (w.at, w.edit) = (k, true);
            }
            if let Some(v) = fields.get::<Option<String>>("tip")? {
                w.meta[k].tip = v;
            }
            if fields.contains_key("page")? {
                w.meta[k].page = fields.get::<Option<i64>>("page")?.map(|p| (p.max(1) - 1) as usize);
            }
            w.settle();
            Ok(())
        })?,
    )?;
    ui.set(
        "get",
        lua.create_function(|lua, (id, eid): (String, String)| {
            use super::window::Elem;
            let st = state(lua);
            let s = st.borrow();
            let w = s
                .windows
                .iter()
                .find(|w| w.id == id)
                .ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
            let e = w
                .elems
                .iter()
                .find(|e| e.id() == Some(eid.as_str()))
                .ok_or_else(|| mlua::Error::runtime(format!("no element {eid:?} in {id:?}")))?;
            let t = lua.create_table()?;
            match e {
                Elem::Timers { groups, .. } => t.set("count", groups.iter().map(|g| g.rows.len()).sum::<usize>())?,
                Elem::Targets { rows, .. } => t.set("count", rows.len())?,
                Elem::Lines { lines, .. } => t.set("count", lines.len())?,
                Elem::Map { dots, .. } => t.set("count", dots.len())?,
                Elem::Chip { text, .. } => t.set("text", text.as_str())?,
                Elem::Text { text, .. } | Elem::Input { text, .. } => t.set("text", text.as_str())?,
                Elem::Buttons { marked, .. } => t.set("at", marked.map(|m| m + 1))?,
                Elem::Slider { at, .. } | Elem::List { at, .. } | Elem::Tabs { at, .. } | Elem::Choice { at, .. } => {
                    t.set("at", at + 1)?
                }
                Elem::Gauge { value, .. } => t.set("value", *value)?,
                Elem::Colour { value: [r, g, b, a], .. } => {
                    let c = lua.create_table()?;
                    c.set("r", *r)?;
                    c.set("g", *g)?;
                    c.set("b", *b)?;
                    c.set("a", *a)?;
                    t.set("value", c)?;
                }
                Elem::Section { open, .. } => t.set("open", *open)?,
                Elem::Separator | Elem::Canvas { .. } => {}
            }
            Ok(t)
        })?,
    )?;
    ui.set(
        "bounds",
        lua.create_function(|lua, (win, id): (String, String)| {
            let info = lua
                .app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>()
                .map(|f| f.clone())
                .unwrap_or_default();
            let Some([x, y, k]) = info.borrow().canvas_rects.get(&(mod_id(lua)?, win.clone(), id.clone())).copied()
            else {
                return Ok(None);
            };
            let st = state(lua);
            let s = st.borrow();
            // A closed window's last place is stale: none.
            let size = s.windows.iter().find(|w| w.id == win && w.open).and_then(|w| {
                w.elems.iter().find_map(|e| match e {
                    super::window::Elem::Canvas { id: c, w, h, .. } if *c == id => Some((*w, *h)),
                    _ => None,
                })
            });
            let Some((cw, ch)) = size else { return Ok(None) };
            let t = lua.create_table()?;
            for (key, v) in [("x", x), ("y", y), ("w", cw * k), ("h", ch * k), ("scale", k)] {
                t.set(key, v)?;
            }
            Ok(Some(t))
        })?,
    )?;
    ui.set(
        "open",
        lua.create_function(|lua, (id, at): (String, Option<Table>)| {
            let pointer = super::input::cursor(lua);
            // Or at {x, y} (window pixels), its bottom there with above = true.
            let place = match &at {
                Some(t) => match (t.get::<Option<f32>>("x")?, t.get::<Option<f32>>("y")?) {
                    (Some(x), Some(y)) => Some(((x, y), t.get::<Option<bool>>("above")?.unwrap_or(false))),
                    _ => None,
                },
                None => None,
            };
            let st = state(lua);
            let mut s = st.borrow_mut();
            let w = s
                .windows
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
            // A popup opens at the pointer as it is now, or where it is told.
            if w.popup && !w.open {
                (w.anchor, w.above) = match place {
                    Some((p, above)) => (Some(p), above),
                    None => (Some(pointer), false),
                };
            }
            w.open = true;
            w.settle();
            if !w.hud {
                s.focus_request = Some(id);
            }
            Ok(())
        })?,
    )?;
    ui.set(
        "show",
        lua.create_function(|lua, id: String| {
            let st = state(lua);
            let mut s = st.borrow_mut();
            let w = s
                .windows
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
            w.open = true;
            w.settle();
            Ok(())
        })?,
    )?;
    ui.set(
        "close",
        lua.create_function(|lua, id: String| {
            let st = state(lua);
            if let Some(w) = st.borrow_mut().windows.iter_mut().find(|w| w.id == id) {
                (w.open, w.focus, w.edit) = (false, false, false);
            }
            Ok(())
        })?,
    )?;
    ui.set(
        "prompt",
        lua.create_function(|lua, (text, a, b): (String, Value, Value)| {
            use super::window::{Elem, Meta};
            // prompt(text, handler) or prompt(text, buttons, handler).
            let (items, handler) = match (a, b) {
                (Value::Function(f), _) => (None, f),
                (items, Value::Function(f)) => (lua.unpack::<Option<Vec<String>>>(items)?, f),
                _ => return Err(mlua::Error::runtime("ui.prompt(text, [buttons], handler)")),
            };
            let items = items.unwrap_or_else(|| vec!["Yes".into(), "No".into()]);
            let elems = vec![
                Elem::Text { id: String::new(), text, dim: false, colour: None },
                Elem::Buttons { id: "answer".into(), label: String::new(), items, marked: None },
            ];
            popup(lua, "__prompt", elems.into_iter().map(|e| (e, Meta::default())).collect(), false, handler)
        })?,
    )?;
    ui.set(
        "menu",
        lua.create_function(|lua, (items, handler): (Vec<String>, Function)| {
            use super::window::{Elem, Meta, Row};
            let height = items.len().clamp(1, 12);
            let rows = items.iter().map(|s| Row::text(s)).collect();
            let elems = vec![(
                Elem::List { id: "item".into(), rows, columns: Vec::new(), at: 0, top: 0, height },
                Meta::default(),
            )];
            popup(lua, "__menu", elems, true, handler)
        })?,
    )?;
    ui.set(
        "is_open",
        lua.create_function(|lua, id: String| Ok(state(lua).borrow().windows.iter().any(|w| w.id == id && w.open)))?,
    )?;
    ui.set(
        "opacity",
        lua.create_function(|lua, (id, opacity): (String, Option<f32>)| {
            set_opacity(lua, &format!("mod:{}:{id}", mod_id(lua)?), opacity);
            Ok(())
        })?,
    )?;
    ui.set(
        "on",
        lua.create_function(|lua, (id, func): (String, Function)| {
            let st = state(lua);
            let key = lua.create_registry_value(func)?;
            let mut s = st.borrow_mut();
            s.window_handlers.retain(|(w, _)| *w != id);
            s.window_handlers.push((id, key));
            Ok(())
        })?,
    )?;
    super::native::install(lua, &ui, &m.id)?;
    super::skin::install(lua, &ui, vm)?;
    g.set("ui", ui)?;

    // timers: rows in the Timers mod's windows, through the service it offers (nothing while it is not running).
    let timers: Table = lua.load(TIMERS_SHIM).set_name("=timers").set_environment(g.clone()).eval()?;
    g.set("timers", timers)?;

    // packets (docs/lua-mods.md 4.7): hooks both ways with Windower's chain, injection, the last of each id.
    let packets = lua.create_table()?;
    for (name, out) in [("on_in", false), ("on_out", true)] {
        packets.set(
            name,
            lua.create_function(move |lua, (which, func): (Value, Function)| {
                need(lua, "packets.read")?;
                // nil hooks every packet; anything else must be an id.
                let id = match which {
                    Value::Nil => None,
                    Value::Integer(i) if (0..=0x1ff).contains(&i) => Some(i as u16),
                    Value::Number(n) if n.fract() == 0.0 && (0.0..=511.0).contains(&n) => Some(n as u16),
                    v => {
                        return Err(mlua::Error::runtime(format!(
                            "packet ids are 0..0x1FF, or nil for every packet ({} given)",
                            v.type_name()
                        )));
                    }
                };
                let st = state(lua);
                let h = next_id(&st);
                let key = lua.create_registry_value(func)?;
                let mut s = st.borrow_mut();
                if out {
                    s.hooks_out.push((h, id, key))
                } else {
                    s.hooks_in.push((h, id, key))
                }
                Ok(h)
            })?,
        )?;
    }
    packets.set(
        "off",
        lua.create_function(|lua, h: u64| {
            let st = state(lua);
            let gone: Vec<mlua::RegistryKey> = {
                let mut guard = st.borrow_mut();
                let s: &mut ModState = &mut guard;
                let mut gone = Vec::new();
                for list in [&mut s.hooks_in, &mut s.hooks_out] {
                    let (g, keep): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(|x| x.0 == h);
                    *list = keep;
                    gone.extend(g.into_iter().map(|x| x.2));
                }
                gone
            };
            for k in gone {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    for (name, out) in [("inject_in", false), ("inject_out", true)] {
        packets.set(
            name,
            lua.create_function(move |lua, (a, b): (Value, Option<mlua::LuaString>)| {
                need(lua, "packets.write")?;
                if super::lobby::in_lobby(lua) {
                    return Err(mlua::Error::runtime("not available in the lobby (before login)"));
                }
                let data = packet_bytes(a, b)?;
                let st = state(lua);
                let mut s = st.borrow_mut();
                if out {
                    s.inject_out.push(data)
                } else {
                    s.inject_in.push(data)
                }
                Ok(())
            })?,
        )?;
    }
    packets.set(
        "build",
        lua.create_function(|lua, (id, body): (i64, mlua::LuaString)| {
            lua.create_string(build_packet(id, &body.as_bytes())?)
        })?,
    )?;
    packets.set(
        "last",
        lua.create_function(|lua, (dir, id): (String, u16)| {
            need(lua, "packets.read")?;
            let stats = &world(lua)?.net.stats;
            let m = if dir == "out" { &stats.last_out } else { &stats.last_in };
            let d = m.lock().ok().and_then(|m| m.get(&id).cloned());
            d.map(|d| lua.create_string(d)).transpose()
        })?,
    )?;
    g.set("packets", packets)?;

    // chat.add(text[, mode]): a log line in the mode's colour, like a game line.
    let chat = lua.create_table()?;
    chat.set(
        "add",
        lua.create_function(|lua, (text, how): (String, Value)| {
            let (mode, colour) = match how {
                Value::Nil => (None, None),
                Value::Table(t) => (t.get::<Option<u8>>("mode")?, t.get::<Option<u8>>("colour")?),
                // A number is checked as the table's fields are: 0..255.
                v => (
                    Some(
                        u8::from_lua(v, lua)
                            .map_err(|_| mlua::Error::runtime("a chat mode is 0..255, or {mode = , colour = }"))?,
                    ),
                    None,
                ),
            };
            push_op(lua, Op::Chat { text, mode, colour })?;
            Ok(())
        })?,
    )?;
    g.set("chat", chat)?;

    // timer.after(s, fn) / timer.every(s, fn) -> id; timer.cancel(id). clock(): monotonic seconds.
    let timer = lua.create_table()?;
    for (name, repeat) in [("after", false), ("every", true)] {
        timer.set(
            name,
            lua.create_function(move |lua, (secs, func): (f64, Function)| {
                let st = state(lua);
                let id = next_id(&st);
                let d = super::vm::secs(secs.max(if repeat { 0.01 } else { 0.0 }));
                let func = lua.create_registry_value(func)?;
                st.borrow_mut().timers.push(Timer { id, due: Instant::now() + d, every: repeat.then_some(d), func });
                Ok(id)
            })?,
        )?;
    }
    timer.set(
        "cancel",
        lua.create_function(|lua, id: u64| {
            let st = state(lua);
            let gone = {
                let mut s = st.borrow_mut();
                s.timers.iter().position(|t| t.id == id).map(|k| s.timers.remove(k))
            };
            if let Some(t) = gone {
                lua.remove_registry_value(t.func)?;
            }
            Ok(())
        })?,
    )?;
    g.set("timer", timer)?;
    super::actions::install(lua, &g)?;
    super::input::install(lua, &g)?;
    super::runtime::install(lua, &g, vm)?;
    super::hud::install(lua, &g, vm)?;
    super::render::install(lua, &g, &m.id)?;
    super::audio::install(lua, &g, vm)?;
    super::draw::install(lua, &g, vm)?;
    super::net::install(lua, &g)?;
    super::devices::install(lua, &g)?;
    let me = m.id.clone();
    let ipc = lua.create_table()?;
    ipc.set(
        "send",
        lua.create_function(move |lua, (text, to): (String, Option<String>)| {
            need(lua, "multibox")?;
            // One UDP datagram each: the encoded message (with room for the sender's name) must fit.
            let size = serde_json::json!({"mod": me, "from": "", "to": to, "text": text}).to_string().len() + 32;
            if size > ipc::MAX {
                return Err(mlua::Error::runtime(format!(
                    "an ipc message is at most {} bytes encoded ({size} given)",
                    ipc::MAX
                )));
            }
            push_op(lua, Op::Ipc(me.clone(), to, text))?;
            Ok(())
        })?,
    )?;
    g.set("ipc", ipc)?;
    let start = Instant::now();
    g.set("clock", lua.create_function(move |_, ()| Ok(start.elapsed().as_secs_f64()))?)?;

    // game: the façade's first areas (docs/lua-mods.md 4.6).
    let game = lua.create_table()?;
    game.set("player", lua.create_function(|lua, ()| player(lua))?)?;
    game.set("zone", lua.create_function(|lua, ()| Ok(world(lua)?.zone))?)?;
    game.set(
        "entity",
        lua.create_function(|lua, key: u32| {
            let w = world(lua)?;
            if key == w.my_id || key == u32::from(w.my_index) {
                return self_entity(lua, w).map(Some);
            }
            let e =
                w.entities.get(&key).map(|e| (key, e)).or_else(|| u16::try_from(key).ok().and_then(|i| w.entity_at(i)));
            e.map(|(id, e)| entity(lua, w, id, e)).transpose()
        })?,
    )?;
    game.set(
        "entities",
        lua.create_function(|lua, opts: Option<Table>| {
            let w = world(lua)?;
            // A filter field of the wrong type is an error, not silently no filter.
            let max = opts
                .as_ref()
                .map(|o| o.get::<Option<f32>>("distance"))
                .transpose()
                .map_err(|_| mlua::Error::runtime("distance is a number"))?
                .flatten();
            let kinds = opts
                .as_ref()
                .map(|o| o.get::<Option<Vec<String>>>("kinds"))
                .transpose()
                .map_err(|_| mlua::Error::runtime("kinds is a list of \"player\", \"npc\", \"mob\""))?
                .flatten();
            if let Some(k) =
                kinds.as_ref().and_then(|k| k.iter().find(|k| !["player", "npc", "mob"].contains(&k.as_str())))
            {
                return Err(mlua::Error::runtime(format!("no entity kind {k:?} (player, npc, mob)")));
            }
            let out = lua.create_table()?;
            if kinds.as_ref().is_none_or(|k| k.iter().any(|k| k == "player")) {
                out.push(self_entity(lua, w)?)?;
            }
            let mut list: Vec<(&u32, &super::super::Entity)> = w.entities.iter().collect();
            list.sort_by_key(|(_, e)| e.index);
            let slots = w.party.slots();
            for (id, e) in list {
                if max.is_some_and(|m| e.pos.distance(w.pos) > m) {
                    continue;
                }
                if kinds.as_ref().is_some_and(|k| !k.iter().any(|k| k == kind_name(e))) {
                    continue;
                }
                out.push(entity_in(lua, w, *id, e, &slots)?)?;
            }
            Ok(out)
        })?,
    )?;
    game.set(
        "target",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            match w.target {
                Some(id) if id == w.my_id => self_entity(lua, w).map(Some),
                t => t
                    .and_then(|id| w.entities.get(&id).map(|e| (id, e)))
                    .map(|(id, e)| entity(lua, w, id, e))
                    .transpose(),
            }
        })?,
    )?;
    game.set("party", lua.create_function(|lua, ()| party(lua))?)?;
    game.set(
        "items",
        lua.create_function(|lua, bag: u8| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            // Slot 0 of every bag is the gil (game.gil), not an item.
            for s in w.inv.viewed(bag).into_iter().filter(|s| s.index != 0) {
                let (id, count) = w.inv.slots.get(&(bag, s.index)).copied().unwrap_or((0, 0));
                let t = lua.create_table()?;
                t.set("bag", bag)?;
                t.set("slot", s.index)?;
                t.set("id", id)?;
                t.set("count", count)?;
                if let Some(x) = w.inv.ext.get(&(bag, s.index)) {
                    t.set("extdata", lua.create_string(x)?)?;
                }
                t.set("locked", w.inv.locks.get(&(bag, s.index)).copied().unwrap_or(0))?;
                if let Some(p) = w.inv.prices.get(&(bag, s.index)) {
                    t.set("price", *p)?;
                }
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    // Held items by name (the short name or the long log name, any case), in the bags given or all, bag by bag.
    game.set(
        "find_items",
        lua.create_function(|lua, (name, bags): (String, Option<Vec<u8>>)| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            let Some(info) = w.item_info.as_ref() else { return Ok(out) };
            let named = |id: u16| {
                info.item(id)
                    .is_some_and(|it| it.name.eq_ignore_ascii_case(&name) || it.singular.eq_ignore_ascii_case(&name))
            };
            for (&(bag, slot), &(id, count)) in
                w.inv.slots.iter().filter(|((b, s), _)| *s != 0 && bags.as_ref().is_none_or(|bs| bs.contains(b)))
            {
                if !named(id) {
                    continue;
                }
                let t = lua.create_table()?;
                t.set("bag", bag)?;
                t.set("slot", slot)?;
                t.set("id", id)?;
                t.set("count", count)?;
                if let Some(x) = w.inv.ext.get(&(bag, slot)) {
                    t.set("extdata", lua.create_string(x)?)?;
                }
                t.set("locked", w.inv.locks.get(&(bag, slot)).copied().unwrap_or(0))?;
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    // Every quest area's quests in the journal's order, with ours current or completed (s2c 0x056).
    game.set(
        "quests",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            let Some(logs) = super::super::quests::names(w) else { return Ok(out) };
            for (area, file) in logs.quests.iter().enumerate().take(11) {
                let list = lua.create_table()?;
                for e in &file.entries {
                    let t = lua.create_table()?;
                    let repeatable = e.name.starts_with('+');
                    t.set("number", e.number)?;
                    t.set("name", crate::dat::font::plain(e.name.trim_start_matches('+')))?;
                    t.set("repeatable", repeatable)?;
                    t.set("current", super::super::quests::has(&w.journal, area, e.number, false))?;
                    t.set("completed", super::super::quests::has(&w.journal, area, e.number, true))?;
                    list.push(t)?;
                }
                out.set(area, list)?;
            }
            Ok(out)
        })?,
    )?;
    // The zone's zone lines (its 'z' boxes with 8-character names): centre, size, yaw and the four corners of the
    // floor they stand on (y down: the box's bottom is its centre's y plus half its height).
    game.set(
        "zone_lines",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for r in &w.zone_lines {
                let t = lua.create_table()?;
                t.set("name", r.name.as_str())?;
                let v = |x: f32, y: f32, z: f32| -> mlua::Result<Table> {
                    let p = lua.create_table()?;
                    p.set("x", x)?;
                    p.set("y", y)?;
                    p.set("z", z)?;
                    Ok(p)
                };
                let [sx, sy, sz] = r.scale.map(f32::abs);
                t.set("centre", v(r.pos[0], r.pos[1], r.pos[2])?)?;
                t.set("size", v(sx, sy, sz)?)?;
                t.set("yaw", r.rot[1])?;
                let floor = r.pos[1] + sy / 2.0;
                let corners = lua.create_table()?;
                for c in r.corners() {
                    corners.push(v(c[0], floor, c[1])?)?;
                }
                t.set("corners", corners)?;
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    // Records of Eminence: the sparks and the objectives set, with their progress (s2c 0x110 / 0x111).
    game.set(
        "roe",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let t = lua.create_table()?;
            t.set("sparks", w.roe.sparks)?;
            let list = lua.create_table()?;
            for (slot, n, progress) in super::super::roe::mod_active(w) {
                let r = lua.create_table()?;
                r.set("slot", slot)?;
                r.set("id", n)?;
                r.set("progress", progress)?;
                list.push(r)?;
            }
            t.set("active", list)?;
            Ok(t)
        })?,
    )?;
    // Our auction slots while the auction house is open.
    game.set(
        "auction",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(slots) = super::super::auction::mod_slots(w) else { return Ok(None) };
            let t = lua.create_table()?;
            let list = lua.create_table()?;
            for (slot, item, count, price, state) in slots {
                let r = lua.create_table()?;
                r.set("slot", slot)?;
                r.set("item", item)?;
                r.set("count", count)?;
                r.set("price", price)?;
                r.set("state", state)?;
                list.push(r)?;
            }
            t.set("slots", list)?;
            Ok(Some(t))
        })?,
    )?;
    // The client's wear rule (the shops' yellow): race, main job and level, superior level, encumbrance.
    game.set(
        "can_wear",
        lua.create_function(|lua, id: u16| {
            let w = world(lua)?;
            Ok(w.item_info.as_ref().and_then(|c| c.item(id)).is_some_and(|it| super::super::shop::can_wear(w, &it, id)))
        })?,
    )?;
    game.set(
        "bag_size",
        lua.create_function(|lua, bag: u8| {
            let w = world(lua)?;
            Ok(w.inv.sizes.get(bag as usize).map(|s| s.saturating_sub(1)))
        })?,
    )?;
    game.set(
        "equipment",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (slot, at) in w.inv.equip.iter().enumerate() {
                if let Some((bag, index)) = at {
                    let t = lua.create_table()?;
                    t.set("bag", *bag)?;
                    t.set("slot", *index)?;
                    t.set("id", w.inv.item(*bag, *index))?;
                    out.set(slot, t)?;
                }
            }
            Ok(out)
        })?,
    )?;
    game.set(
        "buffs",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let now = (super::super::vana_now(w) as u32).wrapping_mul(60);
            let out = lua.create_table()?;
            for (id, end) in super::super::buffs::icons(w) {
                let t = lua.create_table()?;
                t.set("id", id)?;
                if end != 0x7fff_ffff {
                    t.set("remaining", (end.wrapping_sub(now) as i32).max(0) as f64 / 60.0)?;
                }
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    game.set(
        "recasts",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let now = Instant::now();
            let left = |at: &Instant| at.saturating_duration_since(now).as_secs_f64();
            let spells = lua.create_table()?;
            for (id, at) in &w.spell_recast {
                spells.set(*id, left(at))?;
            }
            let abilities = lua.create_table()?;
            for (id, at) in &w.ability_recast {
                abilities.set(*id, left(at))?;
            }
            // Each running recast's whole length.
            let (st, at) = (lua.create_table()?, lua.create_table()?);
            for id in w.spell_recast.keys() {
                st.set(*id, w.spell_total.get(id).copied().unwrap_or(0.0))?;
            }
            for (id, end) in &w.ability_recast {
                at.set(*id, w.ability_total.get(id).copied().unwrap_or_else(|| left(end) as f32))?;
            }
            let totals = lua.create_table()?;
            totals.set("spells", st)?;
            totals.set("abilities", at)?;
            // The 0x119 entries' adjustments: calc1 (sixtieths off the whole, low 6 bits), calc2 (seconds added).
            let calc = lua.create_table()?;
            for (id, &(c1, c2)) in &w.ability_calc {
                let c = lua.create_table()?;
                c.set("calc1", c1)?;
                c.set("calc2", c2)?;
                calc.set(*id, c)?;
            }
            let t = lua.create_table()?;
            t.set("spells", spells)?;
            t.set("abilities", abilities)?;
            t.set("totals", totals)?;
            t.set("calc", calc)?;
            Ok(t)
        })?,
    )?;
    game.set(
        "time",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let earth = super::super::vana_now(w);
            let p = crate::dat::msgfmt::VanaParts::of(earth);
            let t = lua.create_table()?;
            t.set("earth", earth)?;
            t.set("year", p.year)?;
            t.set("month", p.month)?;
            t.set("day", p.day)?;
            t.set("hour", p.hour)?;
            t.set("minute", p.minute)?;
            t.set("weekday", p.weekday)?;
            t.set("moon_phase", p.phase)?;
            t.set("moon", p.moon)?;
            Ok(t)
        })?,
    )?;
    game.set("weather", lua.create_function(|lua, ()| Ok(world(lua)?.weather))?)?;
    // Whether a spell or ability can be used now, as the game's lists decide (status, recast, job and level, MP, TP,
    // blue magic set): true, or false and the game's refusal (a group 7 message id; none for a silent one).
    game.set(
        "usable",
        lua.create_function(|lua, (kind, id): (String, u16)| {
            let w = world(lua)?;
            let refusal = match kind.as_str() {
                "spell" => super::super::magic::magic_check(w, id),
                "ability" => {
                    let index = w.actions.as_ref().and_then(|a| {
                        if a.abilities.get(id as usize).is_some_and(|x| x.id == id) {
                            Some(id)
                        } else {
                            a.abilities.iter().position(|x| x.id == id).map(|p| p as u16)
                        }
                    });
                    match index {
                        Some(i) => super::super::magic::ability_check(w, i),
                        None => Some(super::super::magic::SILENT),
                    }
                }
                other => return Err(mlua::Error::runtime(format!("usable: kind {other:?} (spell or ability)"))),
            };
            Ok((refusal.is_none(), refusal.filter(|&r| r != super::super::magic::SILENT).map(|r| r as u32)))
        })?,
    )?;
    // The held key items' ids, from the server's bit table (one bit per id, 32 to a word).
    game.set(
        "key_items",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            Ok(w.key_items
                .iter()
                .enumerate()
                .flat_map(|(k, &word)| (0..32u32).filter(move |b| word & (1 << b) != 0).map(move |b| k as u32 * 32 + b))
                .collect::<Vec<u32>>())
        })?,
    )?;
    // The treasure pool: each item in it by slot (0..9, as actions.lot / pass take them).
    game.set(
        "treasure",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (k, s) in w.treasure.iter().enumerate().filter(|(_, s)| s.state == 2) {
                let t = lua.create_table()?;
                t.set("slot", k)?;
                t.set("item", s.item)?;
                t.set("count", s.qty)?;
                t.set("dropper", s.from)?;
                t.set("lotted", s.acted == 2)?;
                t.set("passed", s.acted == 1)?;
                t.set("lot", (s.own_lot > 0).then_some(s.own_lot))?;
                t.set("high_lot", s.high_lot)?;
                t.set("high_name", s.high_name.as_str())?;
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    // The content countdown the client shows (s2c 0x075's cnttime): seconds left and its whole length; nil when none.
    game.set(
        "countdown",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let o = &w.objective;
            if !o.counting() {
                return Ok(None);
            }
            let t = lua.create_table()?;
            t.set("left", o.left(super::super::vana_now(w) as u32))?;
            t.set("length", o.duration)?;
            Ok(Some(t))
        })?,
    )?;
    // An event (cutscene or NPC talk) under way: nil when none; its open menu (question, options, cursor from 1).
    game.set(
        "event",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            if w.event.is_none() {
                return Ok(None);
            }
            let t = lua.create_table()?;
            if let Some(c) = super::super::event::choice(w) {
                let m = lua.create_table()?;
                // As the window reads: the game's quotes and full-width forms as characters, in-line codes dropped.
                m.set("question", crate::dat::font::plain(&c.question))?;
                m.set("options", c.options.iter().map(|o| crate::dat::font::plain(&o.1)).collect::<Vec<_>>())?;
                m.set("cursor", c.cursor + 1)?;
                t.set("menu", m)?;
            }
            if let Some((waiting, open)) = super::super::event::message_wait(w) {
                let m = lua.create_table()?;
                m.set("waiting", waiting)?;
                m.set("open", open)?;
                if let Some((page, actor)) = super::super::event::message_page(w) {
                    m.set("lines", page.iter().map(|l| crate::dat::font::plain(l)).collect::<Vec<_>>())?;
                    m.set("actor", actor)?;
                }
                t.set("text", m)?;
            }
            Ok(Some(t))
        })?,
    )?;
    super::state::install(lua, &game)?;
    g.set("game", game)?;

    // res: the game's static data from its DATs (docs/lua-mods.md 4.9).
    let res = lua.create_table()?;
    // A zone's dialogue entry as the file holds it (Shift-JIS with its codes), and the first entry starting with bytes.
    res.set(
        "zone_text",
        lua.create_function(|lua, (zone, id): (u16, u32)| {
            let Some(d) = zone_dialog(world(lua)?, zone) else { return Ok(None) };
            d.raw(id).map(|b| lua.create_string(&b)).transpose()
        })?,
    )?;
    res.set(
        "zone_text_find",
        lua.create_function(|lua, (zone, prefix): (u16, mlua::LuaString)| {
            let Some(d) = zone_dialog(world(lua)?, zone) else { return Ok(None) };
            let p = prefix.as_bytes();
            Ok((0..d.len() as u32).find(|&i| d.raw(i).is_some_and(|r| r.starts_with(&p))))
        })?,
    )?;
    res.set(
        "item",
        lua.create_function(|lua, id: u32| {
            let w = world(lua)?;
            // The currency file's range runs past 0xffff (only gil has a record now).
            let it = match u16::try_from(id) {
                Ok(small) => w.item_info.as_ref().and_then(|c| c.item(small)),
                Err(_) => w.names.as_ref().and_then(|n| n.items.item(id)).map(std::sync::Arc::new),
            };
            let Some(it) = it else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", it.name.as_str())?;
            // The long name the log uses ("Hachiman Domaru +1" for "Hachi. Domaru +1").
            t.set("log_name", it.singular.as_str())?;
            t.set("description", it.description.as_str())?;
            t.set("kind", it.kind.map(item_kind))?;
            t.set("flags", it.flags)?;
            t.set("stack", it.stack)?;
            t.set("type", it.item_type)?;
            t.set("targets", it.targets)?;
            t.set("level", it.level)?;
            t.set("slots", it.slots)?;
            t.set("races", it.races)?;
            t.set("jobs", it.jobs)?;
            t.set("superior_level", it.superior_level)?;
            t.set("damage", it.damage)?;
            t.set("delay", it.delay)?;
            t.set("skill", it.skill)?;
            t.set("max_charges", it.max_charges)?;
            t.set("cast_time", it.cast_time)?;
            t.set("use_delay", it.use_delay)?;
            t.set("reuse_delay", it.reuse_delay)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "spell",
        lua.create_function(|lua, id: u16| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            let Some(s) = w.actions.as_ref().and_then(|a| a.spell(id)) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", w.names.as_ref().and_then(|n| n.spell(id as u32)))?;
            t.set("description", w.names.as_ref().and_then(|n| n.spell_description(id as u32)))?;
            t.set("kind", s.kind)?;
            t.set("element", s.element)?;
            t.set("targets", s.targets)?;
            t.set("skill", s.skill)?;
            t.set("mp", s.mp)?;
            t.set("cast_time", f32::from(s.cast) / 4.0)?;
            t.set("recast", f32::from(s.recast) / 4.0)?;
            t.set("range", s.range)?;
            // It always hits an area (record +0x47).
            t.set("area", s.area != 0)?;
            let levels = lua.create_table()?;
            for (job, l) in s.levels.iter().enumerate() {
                if *l >= 0 {
                    levels.set(job, *l)?;
                }
            }
            t.set("levels", levels)?;
            // Only as the main job: SCH's (flag 8) and GEO's (flag 0x20), as the magic list keeps them.
            t.set(
                "main_only",
                if s.flags & 8 != 0 {
                    Some(20u8)
                } else if s.flags & 0x20 != 0 {
                    Some(21)
                } else {
                    None
                },
            )?;
            // Jobs whose level is the job points it takes at 99 (a gift).
            let jp = lua.create_table()?;
            for job in 0..32u32 {
                if s.jp_mask & (1 << job) != 0 {
                    jp.set(job, true)?;
                }
            }
            t.set("job_points", jp)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "ability",
        lua.create_function(|lua, id: u16| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            let Some(a) = w.actions.as_ref().and_then(|a| a.abilities.iter().find(|x| x.id == id)) else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", w.names.as_ref().and_then(|n| n.ability(id as u32)))?;
            t.set("description", w.names.as_ref().and_then(|n| n.ability_description(id as u32)))?;
            t.set("kind", a.kind)?;
            t.set("cost", a.cost)?;
            t.set("recast_id", a.timer)?;
            t.set("recast", crate::dat::recasts::ability(id))?;
            t.set("targets", a.targets)?;
            t.set("tp", a.tp)?;
            t.set("sub", a.sub)?;
            t.set("level", a.level)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "status",
        lua.create_function(|lua, id: u16| {
            let w = world(lua)?;
            Ok(w.battle.as_ref().map(|b| b.status_title(id)).filter(|n| !n.is_empty()))
        })?,
    )?;
    // The game's status info for an effect (the status icons' file): whether it can be cancelled, whether its icon
    // never shows a time, its description.
    res.set(
        "status_info",
        lua.create_function(|lua, id: u16| {
            let w = world(lua)?;
            let gu = gu(lua)?;
            let small = w.game.get(0x99) & 4 != 0;
            let Some(e) = gu.status_entry(small, id) else { return Ok(None) };
            let (cancel, untimed) = gu.status_flags(small, id).unwrap_or((false, false));
            let t = lua.create_table()?;
            t.set("cancellable", cancel)?;
            t.set("untimed", untimed)?;
            t.set("description", e.desc)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "zone",
        lua.create_function(|lua, id: u32| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            Ok(w.names.as_ref().and_then(|n| n.place(id)))
        })?,
    )?;
    res.set(
        "title",
        lua.create_function(|lua, id: u32| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            Ok(w.names.as_ref().and_then(|n| n.title(id)))
        })?,
    )?;
    res.set(
        "key_item",
        lua.create_function(|lua, id: u32| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            let Some(n) = w.names.as_ref() else { return Ok(None) };
            let Some(noun) = n.key_item(id) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", noun.bare.as_str())?;
            t.set("description", n.key_item_description(id))?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "augments",
        lua.create_function(|lua, (item, ext): (u16, mlua::LuaString)| {
            let w = world(lua)?;
            let ctx = gu(lua)?.aug_ctx().ok_or_else(|| mlua::Error::runtime("augment texts not loaded yet"))?;
            let bytes = ext.as_bytes();
            let e: [u8; 24] = bytes
                .get(..24)
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| mlua::Error::runtime("extdata is 24 bytes"))?;
            let t = w.item_info.as_ref().and_then(|c| c.item(item)).map_or(0, |it| it.item_type);
            let out = lua.create_table()?;
            if !((matches!(t, 4 | 5) && super::super::augment::applies(item, t, &e)) || t == 0x1a) {
                return Ok(out);
            }
            let r = super::super::augment::decode(&e, item, &ctx);
            let stone = t == 0x1a;
            let block = crate::dat::font::decode_sjis(&super::super::augment::block(&r, stone, &ctx));
            out.set("lines", block.lines().filter(|l| !l.is_empty()).map(str::to_string).collect::<Vec<_>>())?;
            if let Some((s, _)) = super::super::augment::short(&r, stone, &ctx) {
                out.set("short", crate::dat::font::decode_sjis(&s))?;
            }
            let pairs = lua.create_table()?;
            for (id, v) in &r.pairs {
                let p = lua.create_table()?;
                p.set("id", *id)?;
                p.set("value", *v)?;
                pairs.push(p)?;
            }
            out.set("pairs", pairs)?;
            Ok(out)
        })?,
    )?;
    res.set(
        "message",
        lua.create_function(|lua, (id, opts): (u16, Option<Table>)| {
            let w = world(lua)?;
            let Some(bt) = w.battle.as_ref() else { return Ok(None) };
            let who = |v: Value| -> Option<crate::dat::battle::Who> {
                match v {
                    Value::Integer(i) => {
                        let id = i as u32;
                        if id == w.my_id {
                            return Some(crate::dat::battle::Who {
                                name: w.name.clone(),
                                pc: true,
                                ..Default::default()
                            });
                        }
                        w.entities.get(&id).map(|e| crate::dat::battle::Who {
                            name: e.name.clone(),
                            pc: id < 0x0100_0000,
                            named: e.named,
                            plural: e.plural,
                            female: e.female,
                            colour: 0,
                        })
                    }
                    Value::String(s) => {
                        Some(crate::dat::battle::Who { name: s.to_string_lossy(), pc: true, ..Default::default() })
                    }
                    _ => None,
                }
            };
            let (mut actor, mut target, mut work) = (None, None, [0i32; 5]);
            if let Some(o) = &opts {
                actor = who(o.get("actor")?);
                target = who(o.get("target")?);
                if let Some(v) = o.get::<Option<Vec<i64>>>("values")? {
                    for (k, x) in v.iter().take(5).enumerate() {
                        work[k] = *x as i32;
                    }
                }
            }
            let ctx = crate::dat::battle::Ctx { actor, target, me: w.name.clone(), work, ..Default::default() };
            let Some(line) = bt.expand(id, &ctx) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("text", line.lines.join("\n"))?;
            t.set("mode", line.mode)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "autotranslate",
        lua.create_function(|lua, (word, group): (Option<String>, Option<u8>)| {
            let w = world(lua)?;
            let Some(d) = w.battle.as_ref().and_then(|b| b.phrases.as_ref()) else { return Ok(None) };
            let word = word.unwrap_or_default();
            let (hits, matched) = d.query(&word, group);
            let out = lua.create_table()?;
            // A word that matches nothing gives nothing (the game's candidate list shows the groups instead).
            if !word.trim().is_empty() && group.is_none() && !matched {
                return Ok(Some(out));
            }
            for (text, id) in hits {
                let t = lua.create_table()?;
                t.set("text", text)?;
                t.set("group", id[2])?;
                t.set("id", id[3])?;
                t.set("phrase", super::runtime::phrase(id))?;
                out.push(t)?;
            }
            Ok(Some(out))
        })?,
    )?;
    res.set(
        "autotranslate_groups",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(d) = w.battle.as_ref().and_then(|b| b.phrases.as_ref()) else { return Ok(None) };
            let out = lua.create_table()?;
            for g in &d.groups {
                let t = lua.create_table()?;
                t.set("group", g.id)?;
                t.set("title", g.title.as_str())?;
                t.set("count", g.phrases.len())?;
                out.push(t)?;
            }
            Ok(Some(out))
        })?,
    )?;
    res.set(
        "name",
        lua.create_function(|lua, (kind, id, lang): (String, u32, Option<String>)| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            if !["item", "key_item", "spell", "ability", "zone", "title"].contains(&kind.as_str()) {
                return Err(mlua::Error::runtime("kind: item, key_item, spell, ability, zone or title"));
            }
            let lang = crate::dat::items::Lang::parse(lang.as_deref().unwrap_or("en"))
                .ok_or_else(|| mlua::Error::runtime("lang: ja, en, de or fr"))?;
            let Some(n) = names_in(w, lang) else { return Ok(None) };
            Ok(match kind.as_str() {
                "item" => n.item(id).map(|x| x.bare),
                "key_item" => n.key_item(id).map(|x| x.bare),
                "spell" => n.spell(id),
                "ability" => n.ability(id),
                "zone" => n.place(id),
                "title" => n.title(id),
                _ => return Err(mlua::Error::runtime("kind: item, key_item, spell, ability, zone or title")),
            })
        })?,
    )?;
    res.set(
        "dat",
        lua.create_function(|lua, id: u32| {
            let w = world(lua)?;
            match w.index.as_ref().map(|i| i.read(id)) {
                Some(Ok(b)) => lua.create_string(b).map(Some),
                _ => Ok(None),
            }
        })?,
    )?;
    res.set(
        "dat_path",
        lua.create_function(|lua, id: u32| {
            Ok(world(lua)?.index.as_ref().and_then(|i| i.path(id)).map(|p| p.display().to_string()))
        })?,
    )?;
    res.set(
        "model_files",
        lua.create_function(|lua, v: Value| {
            let out = lua.create_table()?;
            match v {
                Value::Integer(m) => out.set("file", crate::dat::npc::model_file(m as u16))?,
                Value::Table(t) => {
                    let g = |k: &str| -> mlua::Result<u16> { Ok(t.get::<Option<u16>>(k)?.unwrap_or(0)) };
                    let look = crate::dat::character::Look {
                        race: t.get::<Option<u8>>("race")?.unwrap_or(0),
                        face: g("face")?,
                        head: g("head")?,
                        body: g("body")?,
                        hands: g("hands")?,
                        legs: g("legs")?,
                        feet: g("feet")?,
                        main: g("main")?,
                        sub: g("sub")?,
                        range: g("range")?,
                    };
                    let (skel, parts) =
                        crate::dat::character::file_ids(&look).map_err(|e| mlua::Error::runtime(format!("{e:#}")))?;
                    out.set("skeleton", skel)?;
                    let list = lua.create_table()?;
                    for (slot, model, file) in parts {
                        let p = lua.create_table()?;
                        p.set("slot", slot)?;
                        p.set("model", model)?;
                        p.set("file", file)?;
                        list.push(p)?;
                    }
                    out.set("parts", list)?;
                }
                _ => {
                    return Err(mlua::Error::runtime(
                        "res.model_files takes an NPC model id or a look {race, face, head, ...}",
                    ));
                }
            }
            Ok(out)
        })?,
    )?;
    res.set("weather", lua.create_function(|_, id: usize| Ok(WEATHER.get(id).map(|w| w.0)))?)?;
    res.set("weather_element", lua.create_function(|_, id: usize| Ok(WEATHER.get(id).and_then(|w| w.1)))?)?;
    res.set("element", lua.create_function(|_, id: usize| Ok(ELEMENTS.get(id).copied()))?)?;
    res.set(
        "mount",
        lua.create_function(|lua, id: u16| {
            Ok(Some(super::super::magic::mount_name(world(lua)?, id)).filter(|s| !s.is_empty()))
        })?,
    )?;
    res.set(
        "mob_skill",
        lua.create_function(|lua, id: u16| {
            Ok(world(lua)?.battle.as_ref().map(|b| b.mob_skill(id)).filter(|s| !s.is_empty() && !s.starts_with('#')))
        })?,
    )?;
    res.set(
        "skill",
        lua.create_function(|lua, id: u32| {
            Ok(world(lua)?
                .battle
                .as_ref()
                .and_then(|b| b.kind_name(2, id))
                .map(|s| crate::dat::font::decode_sjis(&s))
                .filter(|s| !s.is_empty()))
        })?,
    )?;
    res.set("day", lua.create_function(|lua, d: usize| Ok(world(lua)?.battle.as_ref().and_then(|b| b.day_name(d))))?)?;
    res.set(
        "moon",
        lua.create_function(|lua, p: usize| Ok(world(lua)?.battle.as_ref().and_then(|b| b.moon_name(p))))?,
    )?;
    res.set(
        "job",
        lua.create_function(|_, id: usize| {
            Ok(super::super::items::JOBS.get(id).filter(|s| !s.is_empty()).map(|s| s.to_string()))
        })?,
    )?;
    g.set("res", res)?;

    // storage.account / storage.character: plain data, saved at logout, unload and storage.save().
    let st_table = lua.create_table()?;
    st_table.set("account", json_table(lua, &storage.0)?)?;
    st_table.set("character", json_table(lua, &storage.1)?)?;
    st_table.set(
        "save",
        lua.create_function(|lua, ()| {
            state(lua).borrow_mut().save = true;
            Ok(())
        })?,
    )?;
    g.set("storage", st_table)?;
    super::lobby::install(lua, &g, &m.id)?;

    // settings.get(key): the user's value, else the declared default.
    let decl = m.settings.clone();
    let settings_table = lua.create_table()?;
    settings_table.set(
        "get",
        lua.create_function(move |lua, key: String| {
            if let Some(v) = state(lua).borrow().settings.get(&key) {
                return to_lua(lua, v);
            }
            // Declared without a default: nil until the player sets it.
            match decl.get(&key) {
                Some(d) => d.default.as_ref().map_or(Ok(Value::Nil), |v| to_lua(lua, v)),
                None => Err(mlua::Error::runtime(format!("no setting {key:?} in mod.toml"))),
            }
        })?,
    )?;
    g.set("settings", settings_table)?;
    Ok(())
}

/// A stored JSON value as a Lua table (an empty table for nothing stored).
pub(super) fn json_table(lua: &Lua, v: &serde_json::Value) -> mlua::Result<Value> {
    match v {
        serde_json::Value::Object(o) if !o.is_empty() => to_lua(lua, v),
        serde_json::Value::Array(a) if !a.is_empty() => to_lua(lua, v),
        _ => Ok(Value::Table(lua.create_table()?)),
    }
}

/// An item record's layout by name, as mods read it (res.item's kind).
fn item_kind(k: crate::dat::items::Kind) -> &'static str {
    use crate::dat::items::Kind as K;
    match k {
        K::General => "general",
        K::Usable => "usable",
        K::Automaton => "automaton",
        K::Armor => "armor",
        K::Weapon => "weapon",
        K::K7000 => "maze",
        K::K7400 => "instinct",
        K::Objective => "objective",
        K::Keff8 => "instinct_catalogue",
        K::Monster => "monster",
        K::RoeCategory => "roe_category",
        K::Kf600 => "teleport_point",
        K::Guide => "primer_category",
        K::GuidePage => "primer_topic",
        K::Currency => "currency",
    }
}

fn kind_name(e: &super::super::Entity) -> &'static str {
    match (e.kind, e.mob) {
        (super::super::net::EntityKind::Player, _) => "player",
        (_, true) => "mob",
        _ => "npc",
    }
}

/// Weather names by id (LSB's and the game's order) and the element each brings (0 Fire .. 7 Dark).
const WEATHER: [(&str, Option<u8>); 20] = [
    ("Clear", None),
    ("Sunshine", None),
    ("Clouds", None),
    ("Fog", None),
    ("Hot Spell", Some(0)),
    ("Heat Wave", Some(0)),
    ("Rain", Some(5)),
    ("Squall", Some(5)),
    ("Dust Storm", Some(3)),
    ("Sand Storm", Some(3)),
    ("Wind", Some(2)),
    ("Gales", Some(2)),
    ("Snow", Some(1)),
    ("Blizzards", Some(1)),
    ("Thunder", Some(4)),
    ("Thunderstorms", Some(4)),
    ("Auroras", Some(6)),
    ("Stellar Glare", Some(6)),
    ("Gloom", Some(7)),
    ("Darkness", Some(7)),
];

/// A zone's dialogue: the one loaded for this zone, or read for another.
fn zone_dialog(w: &World, zone: u16) -> Option<std::sync::Arc<crate::dat::dialog::Dialog>> {
    match &w.zone_text {
        Some((z, d)) if *z == zone => Some(d.clone()),
        _ => crate::dat::dialog::Dialog::load(w.index.as_ref()?, zone).ok().map(std::sync::Arc::new),
    }
}

const ELEMENTS: [&str; 8] = ["Fire", "Ice", "Wind", "Earth", "Lightning", "Water", "Light", "Dark"];

/// An entity as a table: who, where, how it stands.
pub(super) fn entity(lua: &Lua, w: &World, id: u32, e: &super::super::Entity) -> mlua::Result<Table> {
    entity_in(lua, w, id, e, &w.party.slots())
}

/// `entity` with the party's slots worked out once (game.entities: not again for every entity).
fn entity_in(
    lua: &Lua,
    w: &World,
    id: u32,
    e: &super::super::Entity,
    slots: &super::super::party::Slots,
) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", id)?;
    t.set("index", e.index)?;
    t.set("name", e.name.as_str())?;
    t.set("kind", kind_name(e))?;
    let pos = lua.create_table()?;
    pos.set("x", e.pos.x)?;
    pos.set("y", e.pos.y)?;
    pos.set("z", e.pos.z)?;
    t.set("pos", pos)?;
    t.set("heading", e.yaw)?;
    t.set("distance", e.pos.distance(w.pos))?;
    t.set("hpp", e.hpp)?;
    // It has an HP gauge (the target window shows one).
    t.set("gauge", !e.no_gauge)?;
    t.set("status", e.motion.status)?;
    t.set("speed", f32::from(e.motion.speed) / 10.0)?;
    t.set("claim", e.claim)?;
    t.set("targetable", e.flags & 6 == 0 && !e.untargetable && !e.target_off)?;
    t.set("master", e.master)?;
    // A trust (the server marks it so).
    t.set("trust", e.marks.trust)?;
    // The compass radar's colour for it (0xRRGGBB, monsters included), while the radar shows it.
    if id != w.my_id && super::super::radar::shown(e) {
        let bits = super::super::radar::kind_bits(w, id, e);
        t.set("radar", super::super::radar::colour(bits, e.monstrosity, e.motion.status, true))?;
    }
    t.set("hidden", e.flags & 2 != 0)?;
    t.set("race", e.race.or(e.appearance.map(|l| l.race)))?;
    t.set("model", e.model)?;
    t.set("height", model_height(w, id))?;
    // The model's skeleton bones: a handful for an object (a casket's box and lid), dozens for a creature; 0 for none.
    t.set("bones", w.actors.get(&id).and_then(|a| a.character.as_ref()).map_or(0, |c| c.skeleton.bones.len()))?;
    t.set("walks", w.actors.get(&id).and_then(|a| a.character.as_ref()).is_some_and(|c| walks(c)))?;
    if let Some(l) = e.appearance {
        let look = lua.create_table()?;
        for (k, v) in [
            ("face", l.face),
            ("head", l.head),
            ("body", l.body),
            ("hands", l.hands),
            ("legs", l.legs),
            ("feet", l.feet),
            ("main", l.main),
            ("sub", l.sub),
            ("range", l.range),
        ] {
            look.set(k, v)?;
        }
        t.set("look", look)?;
    }
    let slot = slots.slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == id));
    t.set("in_party", slot.is_some_and(|k| k < 6))?;
    t.set("in_alliance", slot.is_some())?;
    t.set("hitbox", e.hitbox)?;
    // Its target: the face target (0x00D / 0x00E facetarget), and that entity's server id when known.
    if e.face != 0 {
        t.set("target_index", e.face)?;
        let tid = if e.face == w.my_index { Some(w.my_id) } else { w.entity_at(e.face).map(|(k, _)| k) };
        t.set("target_id", tid)?;
    }
    // Our pet: its MP% and TP (s2c 0x068).
    if let Some(p) = w.pet_sync.filter(|p| p.index == e.index) {
        t.set("mpp", p.mpp)?;
        t.set("tp", p.tp)?;
        t.set("pet", true)?;
        if p.target != 0 {
            t.set("pet_target", p.target)?;
        }
    }
    Ok(t)
}

/// An actor's model height in yalms (its top at rest, scaled), once its model is in.
/// Whether a model can walk: a person's look, or a walk or run motion (a moogle, a monster); an object (a book, a
/// casket, a crystal) has neither.
fn walks(c: &crate::dat::character::Character) -> bool {
    c.pc.is_some() || c.animations.iter().any(|a| a.name.starts_with("wlk") || a.name.starts_with("run"))
}

fn model_height(w: &World, id: u32) -> Option<f32> {
    w.actors.get(&id).filter(|a| a.character.is_some()).map(|a| -a.top)
}

/// Our character as an entity (the entity map holds the others only).
/// Our facing (what 0x015 sends and the model shows), as entity headings are: radians, 0 east, pi / 2 south.
fn my_heading(w: &World) -> f32 {
    super::super::event::rad_of(w.net.me.lock().map_or(0, |m| m.dir))
}

pub(super) fn self_entity(lua: &Lua, w: &World) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", w.my_id)?;
    t.set("index", w.my_index)?;
    t.set("name", w.name.as_str())?;
    t.set("kind", "player")?;
    let pos = lua.create_table()?;
    pos.set("x", w.pos.x)?;
    pos.set("y", w.pos.y)?;
    pos.set("z", w.pos.z)?;
    t.set("pos", pos)?;
    t.set("heading", my_heading(w))?;
    t.set("distance", 0.0)?;
    t.set("height", model_height(w, w.my_id))?;
    t.set("bones", w.actors.get(&w.my_id).and_then(|a| a.character.as_ref()).map_or(0, |c| c.skeleton.bones.len()))?;
    t.set("walks", true)?;
    t.set("hpp", w.vitals.map_or(100, |v| v.3))?;
    t.set("gauge", true)?;
    t.set("status", w.my_motion.status)?;
    t.set("speed", f32::from(w.my_motion.speed) / 10.0)?;
    t.set("claim", 0)?;
    t.set("targetable", true)?;
    t.set("hidden", false)?;
    t.set("master", 0)?;
    t.set("in_party", true)?;
    t.set("in_alliance", true)?;
    t.set("race", w.my_race)?;
    t.set("me", true)?;
    // Our look (0x00A / 0x051): the last one our model was built from, before the mods' (render.look).
    t.set("model", 0)?;
    if let Some(l) = w.look_base.get(&w.my_id) {
        let look = lua.create_table()?;
        for (k, v) in [
            ("face", l.face),
            ("head", l.head),
            ("body", l.body),
            ("hands", l.hands),
            ("legs", l.legs),
            ("feet", l.feet),
            ("main", l.main),
            ("sub", l.sub),
            ("range", l.range),
        ] {
            look.set(k, v)?;
        }
        t.set("look", look)?;
    }
    // Ours from s2c 0x037 @0x59 (actor +0x208).
    t.set("hitbox", w.my_plate.hitbox)?;
    if let Some((tid, e)) = w.target.and_then(|id| w.entities.get(&id).map(|e| (id, e))) {
        t.set("target_index", e.index)?;
        t.set("target_id", tid)?;
    }
    Ok(t)
}

/// game.party(): every member of the party and alliance, by slot (0..5 our party, 6..17 the others).
fn party(lua: &Lua) -> mlua::Result<Table> {
    let w = world(lua)?;
    let s = w.party.slots();
    let out = lua.create_table()?;
    for (slot, m) in s.slots.iter().enumerate() {
        let Some(m) = m else { continue };
        let t = lua.create_table()?;
        t.set("slot", slot)?;
        t.set("party", slot / 6)?;
        t.set("id", m.id)?;
        t.set("index", m.index)?;
        t.set("name", m.name.as_str())?;
        t.set("hp", m.hp)?;
        t.set("mp", m.mp)?;
        t.set("tp", m.tp)?;
        t.set("hpp", m.hpp)?;
        t.set("mpp", m.mpp)?;
        t.set("zone", m.zone)?;
        t.set("in_zone", m.away == 0)?;
        t.set("main_job", m.jobs[0])?;
        t.set("main_level", m.jobs[1])?;
        t.set("sub_job", m.jobs[2])?;
        t.set("sub_level", m.jobs[3])?;
        t.set("leader", s.leaders.get(slot / 6).is_some_and(|l| *l == m.id))?;
        t.set("alliance_leader", s.alliance_leader == m.id)?;
        let buffs: Vec<u16> = w
            .party_buffs
            .get(&m.id)
            .map(|b| b.iter().copied().filter(|&b| b != 0xffff && b != 0).collect())
            .unwrap_or_default();
        t.set("buffs", buffs)?;
        // The max HP the party window goes by: ours from our stats, a member's as last seen at 100 % (HP / HP %
        // when that no longer fits).
        let max = if m.id == w.my_id {
            w.stats.as_ref().map(|st| match w.vitals {
                Some((hp, _, _, hpp, _)) => {
                    super::super::party::hp_max(Some(w.own_hp_max.unwrap_or(st.hp_max)), hp, hpp).unwrap_or(st.hp_max)
                }
                None => st.hp_max,
            })
        } else {
            super::super::party::hp_max(w.party_hp_max.get(&m.id).copied(), m.hp, m.hpp)
        };
        t.set("hp_max", max)?;
        out.push(t)?;
    }
    Ok(out)
}

/// game.player(): who and where we are, vitals and jobs.
fn player(lua: &Lua) -> mlua::Result<Table> {
    let w = world(lua)?;
    let t = lua.create_table()?;
    t.set("name", w.name.as_str())?;
    t.set("id", w.my_id)?;
    t.set("index", w.my_index)?;
    t.set("zone", w.zone)?;
    let pos = lua.create_table()?;
    pos.set("x", w.pos.x)?;
    pos.set("y", w.pos.y)?;
    pos.set("z", w.pos.z)?;
    t.set("pos", pos)?;
    t.set("heading", my_heading(w))?;
    t.set("status", w.my_motion.status)?;
    // Our 0x037 flags word: bit 4 seeking a party, 5 anonymous, 7 away.
    t.set("seeking", w.self_flags >> 4 & 1 != 0)?;
    t.set("anonymous", w.self_flags >> 5 & 1 != 0)?;
    t.set("away", w.self_flags >> 7 & 1 != 0)?;
    if let Some((hp, mp, tp, hpp, mpp)) = w.vitals {
        t.set("hp", hp)?;
        t.set("mp", mp)?;
        t.set("tp", tp)?;
        t.set("hpp", hpp)?;
        t.set("mpp", mpp)?;
    }
    if let Some(s) = &w.stats {
        t.set("hp_max", s.hp_max)?;
        t.set("mp_max", s.mp_max)?;
        t.set("main_job", s.main.0)?;
        t.set("main_level", s.main.1)?;
        t.set("sub_job", s.sub.0)?;
        t.set("sub_level", s.sub.1)?;
    }
    t.set("target", w.target)?;
    // The focus target (/focustarget, one of our six) and the target before the current one, while here.
    t.set("focus_target", super::super::target::focus_member(w))?;
    t.set("last_target", super::super::target::last_target(w).map(|t| t.0))?;
    // Our pet as our status names it (s2c 0x037: avatars, wyverns, jug pets, automatons, luopans).
    t.set("pet", (w.my_pet != 0).then(|| w.entity_at(w.my_pet).map(|(id, _)| id)).flatten())?;
    Ok(t)
}

/// ui.prompt / ui.menu: a modal window whose answer (1-based, or nil when it is closed) goes to `handler`.
fn popup(
    lua: &Lua,
    id: &str,
    elems: Vec<(super::window::Elem, super::window::Meta)>,
    at_pointer: bool,
    handler: Function,
) -> mlua::Result<()> {
    let close = lua.create_function(|lua, id: String| {
        if let Some(w) = state(lua).borrow_mut().windows.iter_mut().find(|w| w.id == id) {
            (w.open, w.focus, w.edit) = (false, false, false);
        }
        Ok(())
    })?;
    let wrap: Function = lua
        .load(r#"local h, close, id = ... return function(_, e) if e.kind == "press" or e.kind == "select" then close(id); h(e.index) elseif e.kind == "close" then h(nil) end end"#)
        .set_name("=ui.popup")
        .call((handler, close, id))?;
    let cursor = super::input::cursor(lua);
    let st = state(lua);
    let mut s = st.borrow_mut();
    s.windows.retain(|w| w.id != id);
    let mut w = super::window::Window::new(id);
    (w.elems, w.meta) = elems.into_iter().unzip();
    (w.modal, w.popup, w.open) = (true, at_pointer, true);
    w.anchor = at_pointer.then_some(cursor);
    w.settle();
    s.windows.push(w);
    s.focus_request = Some(id.to_string());
    let key = lua.create_registry_value(wrap)?;
    s.window_handlers.retain(|(w, _)| w != id);
    s.window_handlers.push((id.to_string(), key));
    Ok(())
}

/// A canvas's commands: {type = "line" | "rect" | "circle" | "tri" | "text" | "icon", ...}.
fn cmds(v: Option<Table>) -> mlua::Result<Vec<super::window::Cmd>> {
    use super::window::Cmd;
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Table>()
        .map(|c| {
            let c = c?;
            let n = |k: &str| -> mlua::Result<f32> { Ok(c.get::<Option<f32>>(k)?.unwrap_or(0.0)) };
            let colour = super::hud::colour(c.get("colour")?)?.unwrap_or([255; 4]);
            let kind: String = c.get("type")?;
            Ok(match kind.as_str() {
                "line" => Cmd::Line {
                    a: [n("x1")?, n("y1")?],
                    b: [n("x2")?, n("y2")?],
                    width: c.get::<Option<f32>>("width")?.unwrap_or(1.0),
                    colour,
                },
                "rect" => Cmd::Rect {
                    r: [n("x")?, n("y")?, n("w")?, n("h")?],
                    colour,
                    to: super::hud::colour(c.get("to")?)?,
                    line: c.get("line")?,
                    round: n("round")?.max(0.0),
                },
                "circle" => Cmd::Circle { c: [n("x")?, n("y")?], radius: n("r")?, colour, line: c.get("line")? },
                "tri" => Cmd::Tri { pts: [[n("x1")?, n("y1")?], [n("x2")?, n("y2")?], [n("x3")?, n("y3")?]], colour },
                "text" => Cmd::Text {
                    at: [n("x")?, n("y")?],
                    text: c.get::<Option<String>>("text")?.unwrap_or_default(),
                    colour,
                    size: c.get::<Option<f32>>("size")?.unwrap_or(1.0).clamp(0.25, 8.0),
                    align: match c.get::<Option<String>>("align")?.as_deref() {
                        Some("center" | "centre") => 1,
                        Some("right") => 2,
                        _ => 0,
                    },
                    outline: super::hud::colour(c.get("outline")?)?,
                    bold: c.get::<Option<String>>("font")?.as_deref() == Some("bold"),
                },
                "tip" => Cmd::Tip {
                    r: [n("x")?, n("y")?, n("w")?, n("h")?],
                    title: c.get::<Option<String>>("title")?.unwrap_or_default(),
                    text: c.get::<Option<String>>("text")?.unwrap_or_default(),
                },
                "icon" => Cmd::Icon {
                    at: [n("x")?, n("y")?],
                    size: c.get::<Option<f32>>("size")?.unwrap_or(16.0),
                    icon: super::window::Icon::of(c.get::<Option<String>>("kind")?.as_deref(), c.get("id")?),
                    tint: colour,
                },
                other => return Err(mlua::Error::runtime(format!("unknown draw command {other:?}"))),
            })
        })
        .collect()
}

/// A list's rows: strings, or {cells..., icon = {kind = "item" | "status" | "spell" | "ability", id}, colour}.
fn rows(v: Option<Table>) -> mlua::Result<Vec<super::window::Row>> {
    use super::window::Row;
    let cell = |v: Value| -> String {
        match v {
            Value::String(s) => s.to_string_lossy(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => String::new(),
        }
    };
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Value>()
        .map(|r| {
            Ok(match r? {
                Value::Table(t) => {
                    let cells = match t.get::<Option<Table>>("cells")? {
                        Some(c) => c.sequence_values::<Value>().map(|v| v.map(cell)).collect::<mlua::Result<_>>()?,
                        None => t.sequence_values::<Value>().map(|v| v.map(cell)).collect::<mlua::Result<_>>()?,
                    };
                    let icon = match t.get::<Option<Table>>("icon")? {
                        Some(i) => Some(super::window::Icon::of(
                            i.get::<Option<String>>("kind")?.as_deref(),
                            i.get::<u16>("id")?,
                        )),
                        None => None,
                    };
                    Row {
                        cells,
                        icon,
                        colour: super::hud::colour(t.get("colour")?)?,
                        tip: t.get::<Option<String>>("tip")?.unwrap_or_default(),
                    }
                }
                other => Row::text(&cell(other)),
            })
        })
        .collect()
}

/// A timers element's groups: `{name?, rows = {{name, left, total?, fills?, icon = {kind, id}}...}}...` (an icon's
/// kind defaults to "status").
fn timer_groups(v: Option<Table>) -> mlua::Result<Vec<super::window::TimerGroup>> {
    use super::window::{Icon, TimerGroup, TimerRow};
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Table>()
        .map(|g| {
            let g = g?;
            let rows = match g.get::<Option<Table>>("rows")? {
                Some(r) => r
                    .sequence_values::<Table>()
                    .map(|r| {
                        let r = r?;
                        let icon = match r.get::<Option<Table>>("icon")? {
                            Some(i) => Some(Icon::of(
                                Some(i.get::<Option<String>>("kind")?.as_deref().unwrap_or("status")),
                                i.get::<u16>("id")?,
                            )),
                            None => None,
                        };
                        Ok(TimerRow {
                            icon,
                            name: r.get::<Option<String>>("name")?.unwrap_or_default(),
                            left: r.get::<Option<f32>>("left")?.unwrap_or(0.0),
                            total: r.get::<Option<f32>>("total")?.unwrap_or(0.0).max(0.0),
                            fills: r.get::<Option<bool>>("fills")?.unwrap_or(false),
                        })
                    })
                    .collect::<mlua::Result<_>>()?,
                None => Vec::new(),
            };
            Ok(TimerGroup { name: g.get::<Option<String>>("name")?.filter(|n| !n.is_empty()), rows })
        })
        .collect()
}

/// A map element's dots: {x, z, colour = 0xRRGGBB or {r, g, b}, target, tip}.
fn map_dots(v: Option<Table>) -> mlua::Result<Vec<super::window::MapDot>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Table>()
        .map(|d| {
            let d = d?;
            let colour = match d.get::<Value>("colour")? {
                Value::Integer(c) => [(c >> 16) as u8, (c >> 8) as u8, c as u8],
                Value::Number(c) => {
                    let c = c as i64;
                    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
                }
                v => super::hud::colour(v)?.map_or([255; 3], |c| [c[0], c[1], c[2]]),
            };
            Ok(super::window::MapDot {
                x: d.get("x")?,
                z: d.get("z")?,
                colour,
                target: d.get::<Option<bool>>("target")?.unwrap_or(false),
                tip: d.get::<Option<String>>("tip")?.unwrap_or_default(),
            })
        })
        .collect()
}

/// A tone: "text", "muted", "gold", "bronze" or {r, g, b, a}.
fn line_tone(v: Value) -> mlua::Result<super::window::Tone> {
    use super::window::Tone;
    Ok(match v {
        Value::Nil => Tone::Text,
        Value::String(s) => {
            let s = s.to_string_lossy();
            Tone::of(&s)
                .ok_or_else(|| mlua::Error::runtime(format!("tone {s:?}: text, muted, gold, bronze or a colour")))?
        }
        v => super::hud::colour(v)?.map_or(Tone::Text, Tone::Rgba),
    })
}

/// A span: {text, tone, strong, elements = {element ids}}, or a string.
fn span(v: Value) -> mlua::Result<super::window::Span> {
    Ok(match v {
        Value::Table(t) => super::window::Span {
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            tone: line_tone(t.get("tone")?)?,
            strong: t.get::<Option<bool>>("strong")?.unwrap_or(false),
            elements: t.get::<Option<Vec<u8>>>("elements")?.unwrap_or_default(),
        },
        Value::String(s) => super::window::Span { text: s.to_string_lossy(), ..Default::default() },
        _ => return Err(mlua::Error::runtime("a span is {text, tone, strong, elements} or a string")),
    })
}

/// A lines element's lines: each {spans} or one span's fields ({text, tone, strong, elements}), with right, column,
/// indent, bar (0..1, in its tone), fold (a key), open, heading, big and gap.
fn text_lines(v: Option<Table>) -> mlua::Result<Vec<super::window::TextLine>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Table>()
        .map(|l| {
            let l = l?;
            let spans = match l.get::<Option<Table>>("spans")? {
                Some(sp) => sp.sequence_values::<Value>().map(|x| span(x?)).collect::<mlua::Result<_>>()?,
                None if l.contains_key("text")? => vec![span(Value::Table(l.clone()))?],
                None => Vec::new(),
            };
            let bar = match l.get::<Option<f32>>("bar")? {
                Some(f) => Some((f.clamp(0.0, 1.0), line_tone(l.get("tone")?)?)),
                None => None,
            };
            Ok(super::window::TextLine {
                spans,
                right: match l.get::<Value>("right")? {
                    Value::Nil => None,
                    v => Some(span(v)?),
                },
                column: l.get::<Option<f32>>("column")?.unwrap_or(0.0).max(0.0),
                indent: l.get::<Option<f32>>("indent")?.unwrap_or(0.0).max(0.0),
                bar,
                fold: l.get::<Option<String>>("fold")?,
                open: l.get::<Option<bool>>("open")?.unwrap_or(false),
                heading: l.get::<Option<bool>>("heading")?.unwrap_or(false),
                big: l.get::<Option<bool>>("big")?.unwrap_or(false),
                gap: l.get::<Option<f32>>("gap")?.unwrap_or(0.0).max(0.0),
            })
        })
        .collect()
}

fn columns(v: Option<Table>) -> mlua::Result<Vec<super::window::Col>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    v.sequence_values::<Table>()
        .map(|c| {
            let c = c?;
            Ok(super::window::Col {
                title: c.get::<Option<String>>("title")?.unwrap_or_default(),
                width: c.get::<Option<f32>>("width")?.unwrap_or(0.0),
                right: c.get::<Option<String>>("align")?.as_deref() == Some("right"),
            })
        })
        .collect()
}

/// One element of a window spec ({type = "text" | "buttons" | ..., tip, page}) and what every element carries.
fn elem(t: &Table) -> mlua::Result<(super::window::Elem, super::window::Meta)> {
    let meta = super::window::Meta {
        tip: t.get::<Option<String>>("tip")?.unwrap_or_default(),
        page: t.get::<Option<i64>>("page")?.map(|p| (p.max(1) - 1) as usize),
    };
    Ok((elem_only(t)?, meta))
}

fn elem_only(t: &Table) -> mlua::Result<super::window::Elem> {
    use super::window::Elem;
    let kind: String = t.get("type")?;
    let id = || -> mlua::Result<String> {
        t.get::<Option<String>>("id")?.ok_or_else(|| mlua::Error::runtime(format!("a {kind} element needs an id")))
    };
    let index =
        |k: &str| -> mlua::Result<Option<usize>> { Ok(t.get::<Option<i64>>(k)?.map(|i| (i.max(1) - 1) as usize)) };
    Ok(match kind.as_str() {
        "text" => Elem::Text {
            id: t.get::<Option<String>>("id")?.unwrap_or_default(),
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            dim: t.get::<Option<bool>>("dim")?.unwrap_or(false),
            colour: super::hud::colour(t.get("colour")?)?,
        },
        "canvas" => Elem::Canvas {
            id: id()?,
            w: t.get::<Option<f32>>("w")?.unwrap_or(200.0).max(1.0),
            h: t.get::<Option<f32>>("h")?.unwrap_or(100.0).max(1.0),
            cmds: cmds(t.get("draw")?)?,
        },
        "buttons" => Elem::Buttons {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            items: t.get::<Option<Vec<String>>>("items")?.unwrap_or_default(),
            marked: index("marked")?,
        },
        "slider" => {
            let n = t.get::<Option<usize>>("steps")?.unwrap_or(2).max(1);
            Elem::Slider {
                id: id()?,
                label: t.get::<Option<String>>("label")?.unwrap_or_default(),
                n,
                at: index("at")?.unwrap_or(0).min(n - 1),
                shown: t.get::<Option<String>>("shown")?.unwrap_or_default(),
            }
        }
        "list" => Elem::List {
            id: id()?,
            rows: rows(t.get("rows")?)?,
            columns: columns(t.get("columns")?)?,
            at: index("at")?.unwrap_or(0),
            top: 0,
            height: t.get::<Option<usize>>("height")?.unwrap_or(8).max(1),
        },
        "tabs" => Elem::Tabs {
            id: id()?,
            items: t.get::<Option<Vec<String>>>("items")?.unwrap_or_default(),
            at: index("at")?.unwrap_or(0),
        },
        "input" => Elem::Input {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            max: t.get::<Option<usize>>("max")?.unwrap_or(64).clamp(1, 1024),
            filter: t.get::<Option<bool>>("filter")?.unwrap_or(false),
            key: false,
        },
        "key" => Elem::Input {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            max: 64,
            filter: false,
            key: true,
        },
        "choice" | "dropdown" => Elem::Choice {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            items: t.get::<Option<Vec<String>>>("items")?.unwrap_or_default(),
            at: index("at")?.unwrap_or(0),
            drop: kind == "dropdown",
        },
        "colour" | "color" => Elem::Colour {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            value: super::hud::colour(t.get("value")?)?.unwrap_or([255; 4]),
            alpha: t.get::<Option<bool>>("alpha")?.unwrap_or(false),
        },
        "section" => Elem::Section {
            id: id()?,
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            open: t.get::<Option<bool>>("open")?.unwrap_or(true),
        },
        "timers" => Elem::Timers {
            id: t.get::<Option<String>>("id")?.unwrap_or_default(),
            section: match t.get::<Option<String>>("section")?.as_deref() {
                None | Some("recasts") => 0,
                Some("buffs") => 1,
                Some("debuffs") => 2,
                Some(s) => {
                    return Err(mlua::Error::runtime(format!("timers section {s:?}: recasts, buffs or debuffs")));
                }
            },
            groups: timer_groups(t.get("groups")?)?,
            at: Instant::now(),
        },
        "gauge" => Elem::Gauge {
            id: t.get::<Option<String>>("id")?.unwrap_or_default(),
            label: t.get::<Option<String>>("label")?.unwrap_or_default(),
            value: t.get::<Option<f32>>("value")?.unwrap_or(0.0),
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            kind: t.get::<Option<String>>("kind")?.unwrap_or_else(|| "hp".into()),
            row: t.get::<Option<String>>("style")?.as_deref() == Some("row"),
            dim: t.get::<Option<bool>>("dim")?.unwrap_or(false),
        },
        "separator" => Elem::Separator,
        "chip" => Elem::Chip {
            id: t.get::<Option<String>>("id")?.unwrap_or_default(),
            text: t.get::<Option<String>>("text")?.unwrap_or_default(),
            note: t.get::<Option<String>>("note")?.unwrap_or_default(),
            tone: tone(t.get::<Option<String>>("tone")?.as_deref())?,
            detail: t.get::<Option<String>>("detail")?.unwrap_or_default(),
        },
        "targets" => {
            Elem::Targets { id: t.get::<Option<String>>("id")?.unwrap_or_default(), rows: target_rows(t.get("rows")?)? }
        }
        "lines" => {
            Elem::Lines { id: t.get::<Option<String>>("id")?.unwrap_or_default(), lines: text_lines(t.get("lines")?)? }
        }
        "map" => Elem::Map { id: t.get::<Option<String>>("id")?.unwrap_or_default(), dots: map_dots(t.get("dots")?)? },
        other => return Err(mlua::Error::runtime(format!("unknown element type {other:?}"))),
    })
}

/// A `chip` element's tone.
fn tone(v: Option<&str>) -> mlua::Result<u8> {
    match v {
        None | Some("waiting") => Ok(0),
        Some("refused") => Ok(1),
        Some("quiet") => Ok(2),
        Some(o) => Err(mlua::Error::runtime(format!("chip tone {o:?}: waiting, refused or quiet"))),
    }
}

/// A `targets` element's rows: {entity, label, tag} each.
fn target_rows(v: Option<Table>) -> mlua::Result<Vec<super::window::TargetRow>> {
    let Some(t) = v else { return Ok(Vec::new()) };
    t.sequence_values::<Table>()
        .map(|r| {
            let r = r?;
            Ok(super::window::TargetRow {
                entity: r
                    .get::<Option<u32>>("entity")?
                    .ok_or_else(|| mlua::Error::runtime("a targets row needs entity (a server id)"))?,
                label: r.get::<Option<String>>("label")?.unwrap_or_default(),
                tag: r.get::<Option<String>>("tag")?.unwrap_or_default(),
            })
        })
        .collect()
}

/// ui.update: the fields given replace the element's.
fn update(e: &mut super::window::Elem, t: &Table) -> mlua::Result<()> {
    use super::window::Elem;
    match e {
        Elem::Text { text, dim, colour, .. } => {
            if let Some(v) = t.get::<Option<String>>("text")? {
                *text = v;
            }
            if let Some(v) = t.get::<Option<bool>>("dim")? {
                *dim = v;
            }
            if t.contains_key("colour")? {
                *colour = super::hud::colour(t.get("colour")?)?;
            }
        }
        Elem::Canvas { w, h, cmds: c, .. } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v.max(1.0);
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v.max(1.0);
            }
            if t.contains_key("draw")? {
                *c = cmds(t.get("draw")?)?;
            }
        }
        Elem::Buttons { label, items, marked, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<Vec<String>>>("items")? {
                *items = v;
            }
            // false clears the mark (a nil field is not seen).
            match t.get::<Value>("marked")? {
                Value::Nil => {}
                Value::Boolean(false) => *marked = None,
                Value::Integer(v) => *marked = Some((v.max(1) - 1) as usize),
                Value::Number(v) => *marked = Some((v.max(1.0) - 1.0) as usize),
                _ => return Err(mlua::Error::runtime("marked is a button's number, or false")),
            }
        }
        Elem::Slider { label, n, at, shown, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<usize>>("steps")? {
                *n = v.max(1);
            }
            if let Some(v) = t.get::<Option<i64>>("at")? {
                *at = (v.max(1) - 1) as usize;
            }
            if let Some(v) = t.get::<Option<String>>("shown")? {
                *shown = v;
            }
            *at = (*at).min(*n - 1);
        }
        Elem::List { rows: r, columns: c, at, height, .. } => {
            if t.contains_key("rows")? {
                *r = rows(t.get("rows")?)?;
            }
            if t.contains_key("columns")? {
                *c = columns(t.get("columns")?)?;
            }
            if let Some(v) = t.get::<Option<i64>>("at")? {
                *at = (v.max(1) - 1) as usize;
            }
            if let Some(v) = t.get::<Option<usize>>("height")? {
                *height = v.max(1);
            }
        }
        Elem::Timers { groups, at, .. } => {
            if t.contains_key("groups")? {
                *groups = timer_groups(t.get("groups")?)?;
                *at = Instant::now();
            }
        }
        Elem::Targets { rows, .. } => {
            if t.contains_key("rows")? {
                *rows = target_rows(t.get("rows")?)?;
            }
        }
        Elem::Lines { lines, .. } => {
            if t.contains_key("lines")? {
                *lines = text_lines(t.get("lines")?)?;
            }
        }
        Elem::Map { dots, .. } => {
            if t.contains_key("dots")? {
                *dots = map_dots(t.get("dots")?)?;
            }
        }
        Elem::Chip { text, note, tone: k, detail, .. } => {
            if let Some(v) = t.get::<Option<String>>("text")? {
                *text = v;
            }
            if let Some(v) = t.get::<Option<String>>("note")? {
                *note = v;
            }
            if let Some(v) = t.get::<Option<String>>("detail")? {
                *detail = v;
            }
            if t.contains_key("tone")? {
                *k = tone(t.get::<Option<String>>("tone")?.as_deref())?;
            }
        }
        Elem::Gauge { label, value, text, kind, dim, .. } => {
            if let Some(v) = t.get::<Option<bool>>("dim")? {
                *dim = v;
            }
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<f32>>("value")? {
                *value = v;
            }
            if let Some(v) = t.get::<Option<String>>("text")? {
                *text = v;
            }
            if let Some(v) = t.get::<Option<String>>("kind")? {
                *kind = v;
            }
        }
        Elem::Tabs { items, at, .. } => {
            if let Some(v) = t.get::<Option<Vec<String>>>("items")? {
                *items = v;
            }
            if let Some(v) = t.get::<Option<i64>>("at")? {
                *at = (v.max(1) - 1) as usize;
            }
        }
        Elem::Choice { label, items, at, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<Vec<String>>>("items")? {
                *items = v;
            }
            if let Some(v) = t.get::<Option<i64>>("at")? {
                *at = (v.max(1) - 1) as usize;
            }
        }
        Elem::Input { label, text, max, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<String>>("text")? {
                *text = v;
            }
            if let Some(v) = t.get::<Option<usize>>("max")? {
                *max = v.clamp(1, 1024);
            }
        }
        Elem::Colour { label, value, alpha, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = super::hud::colour(t.get("value")?)? {
                *value = v;
            }
            if let Some(v) = t.get::<Option<bool>>("alpha")? {
                *alpha = v;
            }
        }
        Elem::Section { label, open, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? {
                *label = v;
            }
            if let Some(v) = t.get::<Option<bool>>("open")? {
                *open = v;
            }
        }
        Elem::Separator => {}
    }
    Ok(())
}

/// A permission the mod must hold (granted by the user, docs/lua-mods.md 3.5).
pub(super) fn need(lua: &Lua, perm: &str) -> mlua::Result<()> {
    let st = state(lua);
    let s = st.borrow();
    if s.granted.iter().any(|g| g == perm) {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!(
            "this needs the {perm} permission: list it in mod.toml, then /lua grant <mod> {perm}"
        )))
    }
}

/// A game action: `automation` any time, `actions` while handling the player's input (a typed command, a window event).
pub(super) fn need_action(lua: &Lua) -> mlua::Result<()> {
    let st = state(lua);
    let s = st.borrow();
    let has = |p: &str| s.granted.iter().any(|g| g == p);
    if has("automation") || (has("actions") && s.input) {
        Ok(())
    } else if has("actions") {
        Err(mlua::Error::runtime("a game action outside the player's input needs the automation permission"))
    } else {
        Err(mlua::Error::runtime(
            "game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions",
        ))
    }
}

/// A packet to inject: a whole sub-packet (header included, its size field set to fit), or (id, body).
fn packet_bytes(a: Value, b: Option<mlua::LuaString>) -> mlua::Result<Vec<u8>> {
    match (a, b) {
        (Value::Integer(id), Some(body)) => build_packet(id, &body.as_bytes()),
        (Value::Number(id), Some(body)) => build_packet(id as i64, &body.as_bytes()),
        // A whole packet: its size field made to fit its length.
        (Value::String(data), None) if (4..=MAX_PACKET).contains(&data.as_bytes().len()) => {
            Ok(super::fit(data.as_bytes().to_vec()))
        }
        (Value::String(_), None) => Err(mlua::Error::runtime(format!("a whole packet is 4 to {MAX_PACKET} bytes"))),
        _ => Err(mlua::Error::runtime("inject takes (id, body) or a whole sub-packet")),
    }
}

/// A packet's largest size, its header included (the size field counts 4-byte words in 7 bits).
const MAX_PACKET: usize = 508;

/// A whole packet from an id (0..0x1FF) and a body of at most 504 bytes.
pub(super) fn build_packet(id: i64, body: &[u8]) -> mlua::Result<Vec<u8>> {
    if !(0..=0x1FF).contains(&id) {
        return Err(mlua::Error::runtime(format!("packet id {id}: ids are 0 to 0x1FF")));
    }
    if body.len() > MAX_PACKET - 4 {
        return Err(mlua::Error::runtime(format!(
            "a packet body is at most {} bytes ({} given)",
            MAX_PACKET - 4,
            body.len()
        )));
    }
    Ok(crate::net::map::session::build(id as u16, body))
}

/// The running mod's id.
pub(super) fn mod_id(lua: &Lua) -> mlua::Result<String> {
    lua.globals().get::<Table>("mod")?.get::<String>("id")
}

/// Sets or (None) clears a window's opacity from this mod (per-window opacity); the key is the mod's window.
pub(super) fn set_opacity(lua: &Lua, key: &str, opacity: Option<f32>) {
    let Some(info) = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone())
    else {
        return;
    };
    let me = mod_id(lua).unwrap_or_default();
    let mut f = info.borrow_mut();
    match opacity {
        Some(o) => {
            f.opacity.insert(key.to_string(), (me, o.clamp(0.0, 1.0)));
        }
        None => {
            if f.opacity.get(key).is_some_and(|o| o.0 == me) {
                f.opacity.remove(key);
            }
        }
    }
}
