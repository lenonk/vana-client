//! The game's own windows for mods (docs/lua-mods.md 4.10 "Native windows"): which are open, the interface and HUD
//! pieces hidden, the game config by id. Changing anything needs ui.native.

use super::World;
use super::api::{need, to_lua, world};
use mlua::{Lua, Table};

/// HUD pieces a mod may hide, and the modern panels each is.
pub(super) const PIECES: &[(&str, &[&str])] = &[
    ("party", &["party", "alliance1", "alliance2"]),
    ("target", &["target"]),
    ("buffs", &["status"]),
    ("compass", &["where"]),
    ("minimap", &["minimap"]),
    ("cast", &["cast"]),
    ("plates", &[]),
    ("alliance", &["alliance1", "alliance2"]),
    // The log windows come back while an event, a prompt or the chat line needs them.
    ("log", &["chat"]),
    ("timers", &["timers"]),
    ("regime", &["regime"]),
    // Parts drawn inside other windows, hidden where they draw.
    ("network", &[]),
    ("palette", &[]),
    ("clock", &[]),
    ("experience", &[]),
];

/// A modern panel mods may name: its id, whether a mod may hide it (none that take the keys), whether one may place it
/// (the edge windows stay on their edge), and what it is.
pub(super) struct Panel {
    pub name: &'static str,
    pub hide: bool,
    pub place: bool,
    pub what: &'static str,
}

const fn panel(name: &'static str, hide: bool, place: bool, what: &'static str) -> Panel {
    Panel { name, hide, place, what }
}

/// The modern UI's panels as mods name them.
pub(super) const PANELS: &[Panel] = &[
    panel("chat", true, true, "the chat window (the log and the chat line)"),
    panel("party", true, true, "the party window"),
    panel("alliance1", true, true, "the first alliance party"),
    panel("alliance2", true, true, "the second alliance party"),
    panel("target", true, true, "the target window"),
    panel("focus", true, true, "the focus and last target"),
    panel("status", true, true, "our status icons"),
    panel("where", true, true, "the location panel (zone, time, compass)"),
    panel("cast", true, true, "the cast bar"),
    panel("pending", true, true, "the pending action chip"),
    panel("countdown", true, true, "an event's countdown"),
    panel("objective", true, true, "an event's objective bars"),
    panel("combat", true, true, "the battle menu while engaged"),
    panel("favourites", true, true, "the favourites strip"),
    panel("iteminfo", true, true, "the item details beside the item lists"),
    panel("command", false, true, "the main menu"),
    panel("window", false, true, "the main menu's windows (status, equipment, ...)"),
    panel("list", false, true, "the magic and ability lists"),
    panel("cats", false, true, "the magic and ability categories"),
    panel("loot", false, true, "the treasure pool"),
    panel("check", false, true, "a player's /check"),
    panel("timers", true, false, "the Timers window (an edge window)"),
    panel("minimap", true, false, "the minimap (an edge window)"),
    panel("regime", true, false, "the training regime window (an edge window)"),
];

fn find_panel(name: &str) -> mlua::Result<&'static Panel> {
    PANELS.iter().find(|p| p.name == name).ok_or_else(|| {
        let names: Vec<&str> = PANELS.iter().map(|p| p.name).collect();
        mlua::Error::runtime(format!("no panel {name:?} ({})", names.join(", ")))
    })
}

/// A magic or ability list's rows as mods name them: spells by id, abilities by name-table id, mounts, emotes.
fn list_rows(w: &World, l: &super::super::magic::ListWin) -> (&'static str, &'static str, Vec<u16>) {
    use super::super::magic::Kind;
    match l.kind {
        Kind::Magic => ("magic", "spell", l.rows.clone()),
        Kind::Ability => {
            let acts = w.actions.as_ref();
            ("ability", "ability", l.rows.iter().map(|&i| acts.and_then(|a| a.abilities.get(i as usize)).map_or(0, |a| a.id)).collect())
        }
        Kind::Mount => ("ability", "mount", l.rows.clone()),
        Kind::Emote => ("ability", "emote", l.rows.clone()),
    }
}

/// The list with the keys: its window, rows, cursor and top (0-based; no top for a menu), for ui.native.list.
fn list(w: &World) -> Option<(String, Vec<serde_json::Value>, usize, Option<usize>)> {
    use serde_json::json;
    if let Some(l) = w.magic.as_ref().or(w.ability.as_ref()).filter(|l| !l.hidden) {
        let (win, kind, ids) = list_rows(w, l);
        return Some((win.into(), ids.iter().map(|id| json!({ kind: id })).collect(), l.scroll + l.cursor, Some(l.scroll)));
    }
    if let Some((slots, cursor, top)) = super::super::items::native_list(w) {
        let rows = slots.iter().map(|(bag, s)| json!({"item": s.id, "count": s.count, "bag": bag, "slot": s.index, "equipped": s.equipped})).collect();
        return Some(("items".into(), rows, cursor, Some(top)));
    }
    if let Some((view, rows, cursor, top)) = super::super::shop::native_list(w) {
        return Some((format!("shop {view}"), rows, cursor, Some(top)));
    }
    if let Some((layer, rows, cursor, top)) = super::super::auction::native_list(w) {
        // A menu's cursor is its entry's child id (from 1) and it does not scroll.
        let menu = !matches!(layer.as_str(), "list" | "history" | "sell");
        return Some((format!("auction {layer}"), rows, if menu { cursor.saturating_sub(1) } else { cursor }, (!menu).then_some(top)));
    }
    if let Some(k) = w.keyitems.as_ref().filter(|k| k.state >= 2) {
        return Some(("keyitems".into(), k.items.iter().map(|(id, _)| json!({"key_item": id})).collect(), k.cursor, Some(k.top)));
    }
    None
}

/// The highlighted thing of the window with the keys: (window, kind, id, bag, slot).
fn highlighted(w: &World) -> Option<(String, &'static str, u16, Option<u8>, Option<u8>)> {
    if let Some(l) = w.magic.as_ref().or(w.ability.as_ref()).filter(|l| !l.hidden) {
        let (win, kind, ids) = list_rows(w, l);
        return ids.get(l.scroll + l.cursor).map(|&id| (win.to_string(), kind, id, None, None));
    }
    if let Some(k) = w.keyitems.as_ref().filter(|k| k.state >= 2) {
        return k.items.get(k.cursor).map(|&(id, _)| ("keyitems".to_string(), "key_item", id, None, None));
    }
    if let Some((id, _, bag, slot, ..)) = super::super::items::info_source(w) {
        return Some(("items".into(), "item", id, Some(bag), Some(slot)));
    }
    let (id, ..) = super::super::item_info_source(w)?;
    let win = super::super::open_windows(w).into_iter().find(|n| ["shop", "auction", "trade", "post", "loot", "inspect", "synth", "moogle", "event"].contains(n)).unwrap_or("info");
    Some((win.into(), "item", id, None, None))
}

fn info(lua: &Lua) -> std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default()
}

pub(super) fn install(lua: &Lua, ui: &Table, mod_id: &str) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set("windows", lua.create_function(|lua, ()| Ok(super::super::open_windows(world(lua)?)))?)?;
    t.set("menu_open", lua.create_function(|lua, ()| Ok(super::super::world_window_open(world(lua)?)))?)?;
    t.set(
        "highlighted",
        lua.create_function(|lua, ()| {
            let Some((window, kind, id, bag, slot)) = highlighted(world(lua)?) else { return Ok(None) };
            to_lua(lua, &serde_json::json!({"window": window, "kind": kind, "id": id, "bag": bag, "slot": slot})).map(Some)
        })?,
    )?;
    t.set(
        "list",
        lua.create_function(|lua, ()| {
            let Some((window, rows, cursor, top)) = list(world(lua)?) else { return Ok(None) };
            to_lua(lua, &serde_json::json!({"window": window, "rows": rows, "cursor": cursor + 1, "top": top.map(|t| t + 1)})).map(Some)
        })?,
    )?;
    t.set("interface_hidden", lua.create_function(|lua, ()| Ok(world(lua)?.ui_hidden))?)?;
    t.set(
        "interface",
        lua.create_function(|lua, show: bool| {
            need(lua, "ui.native")?;
            super::api::push_op(lua, super::Op::Interface(show))?;
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "hide",
        lua.create_function(move |lua, (what, on): (String, Option<bool>)| {
            need(lua, "ui.native")?;
            // A HUD piece, else a modern panel by name.
            if !PIECES.iter().any(|p| p.0 == what) && !PANELS.iter().any(|p| p.name == what && p.hide) {
                let names: Vec<&str> = PIECES.iter().map(|p| p.0).chain(PANELS.iter().filter(|p| p.hide).map(|p| p.name)).collect();
                return Err(mlua::Error::runtime(format!("no HUD piece or panel {what:?} that can hide ({})", names.join(", "))));
            }
            let info = info(lua);
            let mut f = info.borrow_mut();
            let mods = f.hidden.entry(what).or_default();
            if on.unwrap_or(true) {
                mods.insert(me.clone());
            } else {
                mods.remove(&me);
            }
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "place",
        lua.create_function(move |lua, (name, spec): (String, Option<Table>)| {
            need(lua, "ui.native")?;
            let p = find_panel(&name)?;
            if !p.place {
                return Err(mlua::Error::runtime(format!("{name} stays on its edge: it can be hidden, not placed")));
            }
            let info = info(lua);
            let mut f = info.borrow_mut();
            match spec {
                Some(s) => {
                    let scale = s.get::<Option<f32>>("scale")?.unwrap_or(1.0).clamp(0.25, 4.0);
                    let alpha = s.get::<Option<f32>>("alpha")?.unwrap_or(1.0).clamp(0.0, 1.0);
                    let origin = match s.get::<Option<String>>("origin")?.as_deref() {
                        None | Some("top_left") => (0.0, 0.0),
                        Some("top_right") => (1.0, 0.0),
                        Some("bottom_left") => (0.0, 1.0),
                        Some("bottom_right") => (1.0, 1.0),
                        Some("center" | "centre") => (0.5, 0.5),
                        Some(o) => return Err(mlua::Error::runtime(format!("origin {o:?}: top_left, top_right, bottom_left, bottom_right or center"))),
                    };
                    let placed = super::state::Placed { x: s.get("x")?, y: s.get("y")?, scale, alpha, origin };
                    f.placed.insert(name, (me.clone(), placed));
                }
                None => {
                    if f.placed.get(&name).is_some_and(|p| p.0 == me) {
                        f.placed.remove(&name);
                    }
                }
            }
            Ok(())
        })?,
    )?;
    let rect = |lua: &Lua, name: &str, r: [f32; 4], shown: bool| -> mlua::Result<Table> {
        let t = lua.create_table()?;
        t.set("name", name)?;
        t.set("x", r[0].round() as i64)?;
        t.set("y", r[1].round() as i64)?;
        t.set("w", r[2].round() as i64)?;
        t.set("h", r[3].round() as i64)?;
        t.set("shown", shown)?;
        Ok(t)
    };
    t.set(
        "rect",
        lua.create_function(move |lua, name: String| {
            find_panel(&name)?;
            let info = info(lua);
            let f = info.borrow();
            f.native_rects.get(&name).map(|(r, shown)| rect(lua, &name, *r, *shown)).transpose()
        })?,
    )?;
    t.set(
        "panels",
        lua.create_function(move |lua, ()| {
            let info = info(lua);
            let f = info.borrow();
            let out = lua.create_table()?;
            for p in PANELS {
                let t = match f.native_rects.get(p.name) {
                    Some((r, shown)) => rect(lua, p.name, *r, *shown)?,
                    None => {
                        let t = lua.create_table()?;
                        t.set("name", p.name)?;
                        t.set("shown", false)?;
                        t
                    }
                };
                t.set("hideable", p.hide)?;
                t.set("movable", p.place)?;
                t.set("about", p.what)?;
                t.set("placed_by", f.placed.get(p.name).map(|x| x.0.clone()))?;
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    t.set(
        "hidden",
        lua.create_function(|lua, what: String| Ok(info(lua).borrow().hidden.get(&what).is_some_and(|m| !m.is_empty())))?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "item_colour",
        lua.create_function(move |lua, (id, c): (u16, mlua::Value)| {
            let info = info(lua);
            let mut f = info.borrow_mut();
            match super::render::colour(c)? {
                Some(c) => {
                    // The game's tint: ARGB with 0x80 for full.
                    let h = |v: u8| u32::from(v).div_ceil(2);
                    f.item_tints.insert(id, (me.clone(), h(c[3]) << 24 | h(c[0]) << 16 | h(c[1]) << 8 | h(c[2])));
                }
                None => {
                    if f.item_tints.get(&id).is_some_and(|t| t.0 == me) {
                        f.item_tints.remove(&id);
                    }
                }
            }
            Ok(())
        })?,
    )?;
    t.set("config", lua.create_function(|lua, id: u16| Ok(world(lua)?.game.get(id)))?)?;
    t.set(
        "set_config",
        lua.create_function(|lua, (id, value): (u16, i64)| {
            need(lua, "ui.native")?;
            // The game keeps 32 bits; a negative value is a signed setting's.
            if !(i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&value) {
                return Err(mlua::Error::runtime(format!("config value {value}: 32 bits (-2147483648..4294967295)")));
            }
            super::api::push_op(lua, super::Op::Config(id, value as u32))?;
            Ok(())
        })?,
    )?;
    ui.set("native", t)?;
    Ok(())
}
