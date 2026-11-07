//! The modern UI's look for mods (`ui.skin`): the pack's sprites, palette colours and window frames replaced while the
//! mod runs, the newest mod's change winning. The classic UI's art is the game's: DAT overlay mods replace it.

use super::api::need;
use mlua::{Lua, Table, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A PNG of a mod's: its texture key (with the file's hash) and pixels.
#[derive(Clone)]
pub struct Image {
    pub key: String,
    pub img: Arc<(u32, u32, Vec<u8>)>,
}

/// A window frame: a new border (transparent inside, laid out as the pack's) and the interior's top and bottom colours.
#[derive(Clone, Default)]
pub struct Frame {
    pub border: Option<Image>,
    pub top: Option<[u8; 4]>,
    pub bottom: Option<[u8; 4]>,
}

/// Every mod's changes by name, oldest first: (mod, change); the last is the one in use. `ver` moves on each change,
/// for the UI to rebuild once.
#[derive(Clone, Default)]
pub struct Skin {
    pub ver: u64,
    pub sprites: BTreeMap<String, Vec<(String, Image)>>,
    pub colours: BTreeMap<String, Vec<(String, [u8; 4])>>,
    pub frames: BTreeMap<String, Vec<(String, Frame)>>,
}

impl Skin {
    /// Drops a mod's changes (it unloaded).
    pub fn forget(&mut self, m: &str) {
        let mut changed = forget_map(&mut self.sprites, m);
        changed |= forget_map(&mut self.colours, m);
        changed |= forget_map(&mut self.frames, m);
        if changed {
            self.ver += 1;
        }
    }
}

/// Drops `m`'s entries from a name's stack, and the names left empty; whether anything went.
fn forget_map<T>(map: &mut BTreeMap<String, Vec<(String, T)>>, m: &str) -> bool {
    let mut changed = false;
    map.retain(|_, stack| {
        let before = stack.len();
        stack.retain(|(mod_, _)| mod_.as_str() != m);
        changed |= stack.len() != before;
        !stack.is_empty()
    });
    changed
}

fn info(lua: &Lua) -> std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default()
}

/// Sets or (None) clears a mod's own change for a name; other mods' changes are left alone. The newest change is the
/// one in use, so a clear or an unload lets the change before it show again. Whether the change in use moved.
fn put<T>(map: &mut BTreeMap<String, Vec<(String, T)>>, name: String, me: &str, v: Option<T>) -> bool {
    match v {
        Some(v) => {
            let stack = map.entry(name).or_default();
            stack.retain(|(mod_, _)| mod_.as_str() != me);
            stack.push((me.to_string(), v));
            true
        }
        None => {
            let Some(stack) = map.get_mut(&name) else { return false };
            let before = stack.len();
            stack.retain(|(mod_, _)| mod_.as_str() != me);
            let (removed, empty) = (stack.len() != before, stack.is_empty());
            if empty {
                map.remove(&name);
            }
            removed
        }
    }
}

fn unknown(what: &str, name: &str) -> mlua::Error {
    mlua::Error::runtime(format!("no {what} {name:?} (ui.skin.list() names them)"))
}

pub(super) fn install(lua: &Lua, ui: &Table, vm: &super::vm::Vm) -> mlua::Result<()> {
    use crate::client::modern::pack;
    let t = lua.create_table()?;
    let (src, me) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "sprite",
        lua.create_function(move |lua, (name, path): (String, Value)| {
            need(lua, "ui.native")?;
            if !pack::sprites().contains_key(&name) {
                return Err(unknown("sprite", &name));
            }
            let img = match path {
                Value::String(p) => {
                    let (key, img) = super::hud::load_png(&src, &me, &p.to_string_lossy())?;
                    Some(Image { key, img })
                }
                Value::Nil | Value::Boolean(false) => None,
                _ => return Err(mlua::Error::runtime("a sprite is a PNG path in the mod's folder, or false to give it back")),
            };
            let f = info(lua);
            let mut f = f.borrow_mut();
            if put(&mut f.skin.sprites, name, &me, img) {
                f.skin.ver += 1;
            }
            Ok(())
        })?,
    )?;
    let me = vm.manifest.id.clone();
    t.set(
        "colour",
        lua.create_function(move |lua, (name, c): (String, Value)| {
            need(lua, "ui.native")?;
            if !pack::colours().contains(&name) {
                return Err(unknown("colour", &name));
            }
            let c = super::hud::colour(c)?;
            let f = info(lua);
            let mut f = f.borrow_mut();
            if put(&mut f.skin.colours, name, &me, c) {
                f.skin.ver += 1;
            }
            Ok(())
        })?,
    )?;
    let (src, me) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "frame",
        lua.create_function(move |lua, (name, spec): (String, Value)| {
            need(lua, "ui.native")?;
            if !pack::frames().iter().any(|f| f.0 == name) {
                return Err(unknown("frame", &name));
            }
            let frame = match spec {
                Value::Table(s) => {
                    let border = match s.get::<Option<String>>("path")? {
                        Some(p) => {
                            let (key, img) = super::hud::load_png(&src, &me, &p)?;
                            Some(Image { key, img })
                        }
                        None => None,
                    };
                    Some(Frame { border, top: super::hud::colour(s.get("top")?)?, bottom: super::hud::colour(s.get("bottom")?)? })
                }
                Value::Nil | Value::Boolean(false) => None,
                _ => return Err(mlua::Error::runtime("a frame is {path, top, bottom}, or false to give it back")),
            };
            let f = info(lua);
            let mut f = f.borrow_mut();
            if put(&mut f.skin.frames, name, &me, frame) {
                f.skin.ver += 1;
            }
            Ok(())
        })?,
    )?;
    let me = vm.manifest.id.clone();
    t.set(
        "reset",
        lua.create_function(move |lua, ()| {
            info(lua).borrow_mut().skin.forget(&me);
            Ok(())
        })?,
    )?;
    t.set(
        "list",
        lua.create_function(|lua, ()| {
            let out = lua.create_table()?;
            let sprites = lua.create_table()?;
            for (name, size) in pack::sprites() {
                let s = lua.create_table()?;
                s.set("w", size[0])?;
                s.set("h", size[1])?;
                sprites.set(name.as_str(), s)?;
            }
            let frames = lua.create_table()?;
            for (name, size, corner) in pack::frames() {
                let s = lua.create_table()?;
                s.set("size", *size)?;
                s.set("corner", *corner)?;
                frames.set(name.as_str(), s)?;
            }
            out.set("sprites", sprites)?;
            out.set("colours", pack::colours().clone())?;
            out.set("frames", frames)?;
            Ok(out)
        })?,
    )?;
    ui.set("skin", t)?;
    Ok(())
}
