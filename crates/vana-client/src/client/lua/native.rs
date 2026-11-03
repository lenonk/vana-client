//! The game's own windows for mods (docs/lua-mods.md 4.10 "Native windows"): which are open, the interface and HUD
//! pieces hidden, the game config by id. Changing anything needs ui.native.

use super::World;
use super::api::{need, state, to_lua, world};
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
];

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

/// The list with the keys: its window, rows, cursor and top (0-based), for ui.native.list.
fn list(w: &World) -> Option<(String, Vec<serde_json::Value>, usize, usize)> {
    use serde_json::json;
    if let Some(l) = w.magic.as_ref().or(w.ability.as_ref()).filter(|l| !l.hidden) {
        let (win, kind, ids) = list_rows(w, l);
        return Some((win.into(), ids.iter().map(|id| json!({ kind: id })).collect(), l.scroll + l.cursor, l.scroll));
    }
    if let Some((bag, slots, cursor, top)) = super::super::items::native_list(w) {
        let rows = slots.iter().map(|s| json!({"item": s.id, "count": s.count, "bag": bag, "slot": s.index, "equipped": s.equipped})).collect();
        return Some(("items".into(), rows, cursor, top));
    }
    if let Some((view, rows, cursor, top)) = super::super::shop::native_list(w) {
        return Some((format!("shop {view}"), rows, cursor, top));
    }
    if let Some((layer, rows, cursor, top)) = super::super::auction::native_list(w) {
        return Some((format!("auction {layer}"), rows, cursor, top));
    }
    if let Some(k) = w.keyitems.as_ref().filter(|k| k.state >= 2) {
        return Some(("keyitems".into(), k.items.iter().map(|(id, _)| json!({"key_item": id})).collect(), k.cursor, k.top));
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
            to_lua(lua, &serde_json::json!({"window": window, "rows": rows, "cursor": cursor + 1, "top": top + 1})).map(Some)
        })?,
    )?;
    t.set("interface_hidden", lua.create_function(|lua, ()| Ok(world(lua)?.ui_hidden))?)?;
    t.set(
        "interface",
        lua.create_function(|lua, show: bool| {
            need(lua, "ui.native")?;
            state(lua).borrow_mut().ops.push(super::Op::Interface(show));
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "hide",
        lua.create_function(move |lua, (what, on): (String, Option<bool>)| {
            need(lua, "ui.native")?;
            if !PIECES.iter().any(|p| p.0 == what) {
                let names: Vec<&str> = PIECES.iter().map(|p| p.0).collect();
                return Err(mlua::Error::runtime(format!("no HUD piece {what:?} ({})", names.join(", "))));
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
        lua.create_function(|lua, (id, value): (u16, u32)| {
            need(lua, "ui.native")?;
            state(lua).borrow_mut().ops.push(super::Op::Config(id, value));
            Ok(())
        })?,
    )?;
    ui.set("native", t)?;
    Ok(())
}
