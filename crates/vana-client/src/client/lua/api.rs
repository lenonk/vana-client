//! The globals a mod sees (docs/lua-mods.md 4). Reads go through the world the host lends for the mod phase; writes
//! are queued as ops and applied after it, through the paths keys and text commands take.

use super::vm::{Handler, ModState, Timer};
use super::{Op, World};
use mlua::{Function, Lua, LuaSerdeExt, Table, Value, Variadic};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The world, lent while the host runs a mod's Lua (null otherwise).
pub(super) struct WorldRef(pub *const World);

pub(super) fn world(lua: &Lua) -> mlua::Result<&World> {
    let p = lua.app_data_ref::<WorldRef>().map_or(std::ptr::null(), |r| r.0);
    if p.is_null() {
        return Err(mlua::Error::runtime("game state is not available outside the mod phase"));
    }
    // The host sets the pointer from a live &World for the length of the call and clears it after.
    Ok(unsafe { &*p })
}

pub(super) fn state(lua: &Lua) -> Rc<RefCell<ModState>> {
    lua.app_data_ref::<Rc<RefCell<ModState>>>().expect("vm state").clone()
}

pub(super) fn next_id(st: &Rc<RefCell<ModState>>) -> u64 {
    let mut s = st.borrow_mut();
    s.next_id += 1;
    s.next_id
}

/// Every global, for the mod `vm`.
pub(super) fn install(lua: &Lua, vm: &super::vm::Vm, storage: (serde_json::Value, serde_json::Value)) -> mlua::Result<()> {
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
            let text = args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect::<Vec<_>>().join("\t");
            log::info!("lua {id}: {text}");
            state(lua).borrow_mut().ops.push(Op::Print(text));
            Ok(())
        })?,
    )?;
    let log = lua.create_table()?;
    for (name, level) in [("info", log::Level::Info), ("warn", log::Level::Warn), ("error", log::Level::Error), ("debug", log::Level::Debug)] {
        let id = m.id.clone();
        log.set(name, lua.create_function(move |_, text: String| {
            log::log!(level, "lua {id}: {text}");
            Ok(())
        })?)?;
    }
    g.set("log", log)?;

    // require: the mod's own files (a.b -> a/b.lua or a/b/init.lua), loaded once.
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
            // No way out of the folder: no "..", no root, and (Windows) no drive or backslash path.
            if name.contains("..") || name.starts_with('/') || name.contains(['\\', ':']) {
                return Err(mlua::Error::runtime(format!("require {name:?}: outside the mod's folder")));
            }
            let rel = name.replace('.', "/");
            let file = [format!("{rel}.lua"), format!("{rel}/init.lua")].into_iter().find(|p| src.exists(p)).ok_or_else(|| mlua::Error::runtime(format!("module {name:?} not found in the mod's folder")))?;
            let text = src.read(&file).map_err(|e| mlua::Error::runtime(format!("{file}: {e}")))?;
            let f = lua.load(text).set_name(format!("@{file}")).set_mode(mlua::chunk::ChunkMode::Text).into_function()?;
            let v: Value = f.call(name.as_str())?;
            let v = if v.is_nil() { Value::Boolean(true) } else { v };
            loaded.raw_set(name.as_str(), v.clone())?;
            Ok(v)
        })?,
    )?;

    // events.on(name, fn[, {priority}]) -> id; events.off(id); events.emit(name, ...) within this mod.
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
                let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut s.handlers).into_iter().partition(|h| h.id == id);
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
            // /lua is the player's: a mod must not grant itself permissions or run code in other mods (issue #68).
            let first = line.trim().trim_start_matches('/').split_whitespace().next().unwrap_or("");
            if first.eq_ignore_ascii_case("lua") {
                return Err(mlua::Error::runtime("commands.run: /lua runs only when typed by the player"));
            }
            if !line.starts_with("//") {
                need_action(lua)?;
            }
            state(lua).borrow_mut().ops.push(Op::Run(line));
            Ok(())
        })?,
    )?;
    g.set("commands", commands)?;

    // filters.chat(fn) / filters.typed(fn) -> id; filters.off(id). Run inline: no sleeping, no game state.
    let filters = lua.create_table()?;
    for (name, typed) in [("chat", false), ("typed", true)] {
        filters.set(
            name,
            lua.create_function(move |lua, func: Function| {
                let st = state(lua);
                let id = next_id(&st);
                let key = lua.create_registry_value(func)?;
                let mut s = st.borrow_mut();
                if typed { s.typed_filters.push((id, key)) } else { s.chat_filters.push((id, key)) }
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
                for list in [&mut s.chat_filters, &mut s.typed_filters] {
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
            let elems: Option<Table> = spec.get("elements")?;
            w.elems = match elems {
                Some(t) => t.sequence_values::<Table>().map(|e| e.and_then(|e| elem(&e))).collect::<mlua::Result<_>>()?,
                None => Vec::new(),
            };
            w.settle();
            Ok(())
        })?,
    )?;
    ui.set(
        "update",
        lua.create_function(|lua, (id, eid, fields): (String, String, Table)| {
            let st = state(lua);
            let mut s = st.borrow_mut();
            let w = s.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
            let e = w.elems.iter_mut().find(|e| e.id() == Some(eid.as_str())).ok_or_else(|| mlua::Error::runtime(format!("no element {eid:?} in {id:?}")))?;
            update(e, &fields)?;
            w.settle();
            Ok(())
        })?,
    )?;
    ui.set(
        "open",
        lua.create_function(|lua, id: String| {
            let st = state(lua);
            let mut s = st.borrow_mut();
            let w = s.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
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
            let w = s.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| mlua::Error::runtime(format!("no window {id:?}")))?;
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
    ui.set("is_open", lua.create_function(|lua, id: String| Ok(state(lua).borrow().windows.iter().any(|w| w.id == id && w.open)))?)?;
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
    g.set("ui", ui)?;

    // packets (docs/lua-mods.md 4.7): hooks both ways with Windower's chain, injection, the last of each id.
    let packets = lua.create_table()?;
    for (name, out) in [("on_in", false), ("on_out", true)] {
        packets.set(
            name,
            lua.create_function(move |lua, (which, func): (Value, Function)| {
                need(lua, "packets.read")?;
                let id = match which {
                    Value::Integer(i) => Some(i as u16),
                    Value::Number(n) => Some(n as u16),
                    _ => None,
                };
                let st = state(lua);
                let h = next_id(&st);
                let key = lua.create_registry_value(func)?;
                let mut s = st.borrow_mut();
                if out { s.hooks_out.push((h, id, key)) } else { s.hooks_in.push((h, id, key)) }
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
                let data = packet_bytes(a, b)?;
                let st = state(lua);
                let mut s = st.borrow_mut();
                if out { s.inject_out.push(data) } else { s.inject_in.push(data) }
                Ok(())
            })?,
        )?;
    }
    packets.set(
        "build",
        lua.create_function(|lua, (id, body): (u16, mlua::LuaString)| lua.create_string(crate::net::map::session::build(id, &body.as_bytes())))?,
    )?;
    packets.set(
        "last",
        lua.create_function(|lua, (dir, id): (String, u16)| {
            need(lua, "packets.read")?;
            let last = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::LastPackets>>>().map(|r| r.clone());
            let Some(last) = last else { return Ok(None) };
            let l = last.borrow();
            let m = if dir == "out" { &l.outgoing } else { &l.incoming };
            m.get(&id).map(|d| lua.create_string(d)).transpose()
        })?,
    )?;
    g.set("packets", packets)?;

    // chat.add(text[, mode]): a log line in the mode's colour, like a game line.
    let chat = lua.create_table()?;
    chat.set(
        "add",
        lua.create_function(|lua, (text, how): (String, Value)| {
            let (mode, colour) = match how {
                Value::Integer(m) => (Some(m as u8), None),
                Value::Number(m) => (Some(m as u8), None),
                Value::Table(t) => (t.get::<Option<u8>>("mode")?, t.get::<Option<u8>>("colour")?),
                _ => (None, None),
            };
            state(lua).borrow_mut().ops.push(Op::Chat { text, mode, colour });
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
                let d = Duration::from_secs_f64(secs.max(if repeat { 0.01 } else { 0.0 }));
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
            let e = w.entities.get(&key).map(|e| (key, e)).or_else(|| w.entities.iter().find(|(_, e)| u32::from(e.index) == key).map(|(id, e)| (*id, e)));
            e.map(|(id, e)| entity(lua, w, id, e)).transpose()
        })?,
    )?;
    game.set(
        "entities",
        lua.create_function(|lua, opts: Option<Table>| {
            let w = world(lua)?;
            let max = opts.as_ref().and_then(|o| o.get::<f32>("distance").ok());
            let kinds: Option<Vec<String>> = opts.as_ref().and_then(|o| o.get::<Vec<String>>("kinds").ok());
            let out = lua.create_table()?;
            if kinds.as_ref().is_none_or(|k| k.iter().any(|k| k == "player")) {
                out.push(self_entity(lua, w)?)?;
            }
            let mut list: Vec<(&u32, &super::super::Entity)> = w.entities.iter().collect();
            list.sort_by_key(|(_, e)| e.index);
            for (id, e) in list {
                if max.is_some_and(|m| e.pos.distance(w.pos) > m) {
                    continue;
                }
                if kinds.as_ref().is_some_and(|k| !k.iter().any(|k| k == kind_name(e))) {
                    continue;
                }
                out.push(entity(lua, w, *id, e)?)?;
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
                t => t.and_then(|id| w.entities.get(&id).map(|e| (id, e))).map(|(id, e)| entity(lua, w, id, e)).transpose(),
            }
        })?,
    )?;
    game.set("party", lua.create_function(|lua, ()| party(lua))?)?;
    game.set(
        "items",
        lua.create_function(|lua, bag: u8| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for s in w.inv.viewed(bag) {
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
            let t = lua.create_table()?;
            t.set("spells", spells)?;
            t.set("abilities", abilities)?;
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
    game.set("key_items", lua.create_function(|lua, ()| Ok(world(lua)?.key_items.clone()))?)?;
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
                m.set("question", c.question.as_str())?;
                m.set("options", c.options.iter().map(|o| o.1.as_str()).collect::<Vec<_>>())?;
                m.set("cursor", c.cursor + 1)?;
                t.set("menu", m)?;
            }
            Ok(Some(t))
        })?,
    )?;
    super::state::install(lua, &game)?;
    g.set("game", game)?;

    // res: the game's static data from its DATs (docs/lua-mods.md 4.9).
    let res = lua.create_table()?;
    res.set(
        "item",
        lua.create_function(|lua, id: u16| {
            let w = world(lua)?;
            let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", it.name.as_str())?;
            t.set("description", it.description.as_str())?;
            t.set("kind", it.kind.map(|k| format!("{k:?}")))?;
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
            let levels = lua.create_table()?;
            for (job, l) in s.levels.iter().enumerate() {
                if *l >= 0 {
                    levels.set(job, *l)?;
                }
            }
            t.set("levels", levels)?;
            Ok(Some(t))
        })?,
    )?;
    res.set(
        "ability",
        lua.create_function(|lua, id: u16| {
            use crate::dat::names::NameSource;
            let w = world(lua)?;
            let Some(a) = w.actions.as_ref().and_then(|a| a.abilities.iter().find(|x| x.id == id)) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("id", id)?;
            t.set("name", w.names.as_ref().and_then(|n| n.ability(id as u32)))?;
            t.set("description", w.names.as_ref().and_then(|n| n.ability_description(id as u32)))?;
            t.set("kind", a.kind)?;
            t.set("cost", a.cost)?;
            t.set("recast_id", a.timer)?;
            t.set("targets", a.targets)?;
            t.set("tp", a.tp)?;
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
    res.set("job", lua.create_function(|_, id: usize| Ok(super::super::items::JOBS.get(id).filter(|s| !s.is_empty()).map(|s| s.to_string())))?)?;
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

    // settings.get(key): the user's value, else the declared default.
    let decl = m.settings.clone();
    let settings_table = lua.create_table()?;
    settings_table.set(
        "get",
        lua.create_function(move |lua, key: String| {
            if let Some(v) = state(lua).borrow().settings.get(&key) {
                return lua.to_value(v);
            }
            match decl.get(&key).and_then(|s| s.default.as_ref()) {
                Some(v) => lua.to_value(v),
                None => Err(mlua::Error::runtime(format!("no setting {key:?} in mod.toml"))),
            }
        })?,
    )?;
    g.set("settings", settings_table)?;
    Ok(())
}

/// A stored JSON value as a Lua table (an empty table for nothing stored).
fn json_table(lua: &Lua, v: &serde_json::Value) -> mlua::Result<Value> {
    match v {
        serde_json::Value::Object(o) if !o.is_empty() => lua.to_value(v),
        _ => Ok(Value::Table(lua.create_table()?)),
    }
}

fn kind_name(e: &super::super::Entity) -> &'static str {
    match (e.kind, e.mob) {
        (super::super::net::EntityKind::Player, _) => "player",
        (_, true) => "mob",
        _ => "npc",
    }
}

/// An entity as a table: who, where, how it stands.
pub(super) fn entity(lua: &Lua, w: &World, id: u32, e: &super::super::Entity) -> mlua::Result<Table> {
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
    t.set("status", e.motion.status)?;
    t.set("speed", f32::from(e.motion.speed) / 10.0)?;
    t.set("claim", e.claim)?;
    t.set("targetable", e.flags & 6 == 0 && !e.untargetable && !e.target_off)?;
    t.set("master", e.master)?;
    t.set("hidden", e.flags & 2 != 0)?;
    t.set("race", e.race.or(e.appearance.map(|l| l.race)))?;
    t.set("model", e.model)?;
    if let Some(l) = e.appearance {
        let look = lua.create_table()?;
        for (k, v) in [("face", l.face), ("head", l.head), ("body", l.body), ("hands", l.hands), ("legs", l.legs), ("feet", l.feet), ("main", l.main), ("sub", l.sub), ("range", l.range)] {
            look.set(k, v)?;
        }
        t.set("look", look)?;
    }
    let slot = w.party.slots().slots.iter().position(|m| m.as_ref().is_some_and(|m| m.id == id));
    t.set("in_party", slot.is_some_and(|k| k < 6))?;
    t.set("in_alliance", slot.is_some())?;
    Ok(t)
}

/// Our own character as an entity (the entity map holds the others only).
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
    t.set("hpp", w.vitals.map_or(100, |v| v.3))?;
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
        let buffs: Vec<u16> = w.party_buffs.get(&m.id).map(|b| b.iter().copied().filter(|&b| b != 0xffff && b != 0).collect()).unwrap_or_default();
        t.set("buffs", buffs)?;
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
    Ok(t)
}

/// One element of a window spec ({type = "text" | "buttons" | "slider" | "list" | "separator", ...}).
fn elem(t: &Table) -> mlua::Result<super::window::Elem> {
    use super::window::Elem;
    let kind: String = t.get("type")?;
    let id = || -> mlua::Result<String> { t.get::<Option<String>>("id")?.ok_or_else(|| mlua::Error::runtime(format!("a {kind} element needs an id"))) };
    let index = |k: &str| -> mlua::Result<Option<usize>> { Ok(t.get::<Option<i64>>(k)?.map(|i| (i.max(1) - 1) as usize)) };
    Ok(match kind.as_str() {
        "text" => Elem::Text { id: t.get::<Option<String>>("id")?.unwrap_or_default(), text: t.get::<Option<String>>("text")?.unwrap_or_default(), dim: t.get::<Option<bool>>("dim")?.unwrap_or(false) },
        "buttons" => Elem::Buttons { id: id()?, label: t.get::<Option<String>>("label")?.unwrap_or_default(), items: t.get::<Option<Vec<String>>>("items")?.unwrap_or_default(), marked: index("marked")? },
        "slider" => {
            let n = t.get::<Option<usize>>("steps")?.unwrap_or(2).max(1);
            Elem::Slider { id: id()?, label: t.get::<Option<String>>("label")?.unwrap_or_default(), n, at: index("at")?.unwrap_or(0).min(n - 1), shown: t.get::<Option<String>>("shown")?.unwrap_or_default() }
        }
        "list" => Elem::List { id: id()?, rows: t.get::<Option<Vec<String>>>("rows")?.unwrap_or_default(), at: index("at")?.unwrap_or(0), top: 0, height: t.get::<Option<usize>>("height")?.unwrap_or(8).max(1) },
        "gauge" => Elem::Gauge { id: t.get::<Option<String>>("id")?.unwrap_or_default(), label: t.get::<Option<String>>("label")?.unwrap_or_default(), value: t.get::<Option<f32>>("value")?.unwrap_or(0.0), text: t.get::<Option<String>>("text")?.unwrap_or_default(), kind: t.get::<Option<String>>("kind")?.unwrap_or_else(|| "hp".into()) },
        "separator" => Elem::Separator,
        other => return Err(mlua::Error::runtime(format!("unknown element type {other:?}"))),
    })
}

/// ui.update: the fields given replace the element's.
fn update(e: &mut super::window::Elem, t: &Table) -> mlua::Result<()> {
    use super::window::Elem;
    match e {
        Elem::Text { text, dim, .. } => {
            if let Some(v) = t.get::<Option<String>>("text")? { *text = v; }
            if let Some(v) = t.get::<Option<bool>>("dim")? { *dim = v; }
        }
        Elem::Buttons { label, items, marked, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? { *label = v; }
            if let Some(v) = t.get::<Option<Vec<String>>>("items")? { *items = v; }
            if let Some(v) = t.get::<Option<i64>>("marked")? { *marked = Some((v.max(1) - 1) as usize); }
        }
        Elem::Slider { label, n, at, shown, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? { *label = v; }
            if let Some(v) = t.get::<Option<usize>>("steps")? { *n = v.max(1); }
            if let Some(v) = t.get::<Option<i64>>("at")? { *at = (v.max(1) - 1) as usize; }
            if let Some(v) = t.get::<Option<String>>("shown")? { *shown = v; }
            *at = (*at).min(*n - 1);
        }
        Elem::List { rows, at, height, .. } => {
            if let Some(v) = t.get::<Option<Vec<String>>>("rows")? { *rows = v; }
            if let Some(v) = t.get::<Option<i64>>("at")? { *at = (v.max(1) - 1) as usize; }
            if let Some(v) = t.get::<Option<usize>>("height")? { *height = v.max(1); }
        }
        Elem::Gauge { label, value, text, kind, .. } => {
            if let Some(v) = t.get::<Option<String>>("label")? { *label = v; }
            if let Some(v) = t.get::<Option<f32>>("value")? { *value = v; }
            if let Some(v) = t.get::<Option<String>>("text")? { *text = v; }
            if let Some(v) = t.get::<Option<String>>("kind")? { *kind = v; }
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
        Err(mlua::Error::runtime(format!("this needs the {perm} permission: list it in mod.toml, then /lua grant <mod> {perm}")))
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
        Err(mlua::Error::runtime("game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions"))
    }
}

/// A packet to inject: a whole sub-packet (header included), or (id, body).
fn packet_bytes(a: Value, b: Option<mlua::LuaString>) -> mlua::Result<Vec<u8>> {
    match (a, b) {
        (Value::Integer(id), Some(body)) => Ok(crate::net::map::session::build(id as u16, &body.as_bytes())),
        (Value::Number(id), Some(body)) => Ok(crate::net::map::session::build(id as u16, &body.as_bytes())),
        (Value::String(data), None) if data.as_bytes().len() >= 4 => Ok(data.as_bytes().to_vec()),
        _ => Err(mlua::Error::runtime("inject takes (id, body) or a whole sub-packet")),
    }
}
