//! Lobby-phase mods (docs/lua-mods.md 3.4, manifest `lobby = true`): the screens before the world, read through
//! `lobby.state` and acted on by `lobby.sign_in` / `lobby.play`; `storage.seal` keeps a password unreadable on disk.

use super::api::{need_action, state};
use mlua::{Lua, Table, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// What the lobby shows this frame, as the app sets it.
#[derive(Clone, Default)]
pub(in crate::client) struct View {
    /// login, title, select, create, delete, config, connecting (the world loading) or world.
    pub screen: &'static str,
    pub server: String,
    pub account: String,
    pub playonline: bool,
    pub characters: Vec<Char>,
    /// The select screen's character (index into `characters`).
    pub selected: Option<usize>,
    /// A fade, a reply awaited or an error up: actions wait.
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub(in crate::client) struct Char {
    pub name: String,
    pub world: String,
    pub race: u16,
    pub job: u8,
    pub level: u8,
    pub sub_job: u8,
    pub zone: u16,
    pub available: bool,
}

pub(in crate::client) enum Who {
    Name(String),
    /// Index into the character list.
    Slot(usize),
}

pub(in crate::client) enum Act {
    SignIn { server: Option<String>, account: String, password: String, otp: String, pol_id: Option<String>, pol_password: String },
    Play(Who),
}

pub(super) type Shared = Rc<RefCell<View>>;

fn view(lua: &Lua) -> Shared {
    lua.app_data_ref::<Shared>().map(|v| v.clone()).unwrap_or_default()
}

/// The sealing key's names: a mod's sealed text opens only for that mod, on this machine.
const SEAL_SERVER: &str = "lua-mod";

pub(super) fn install(lua: &Lua, g: &Table, mod_id: &str) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set(
        "state",
        lua.create_function(|lua, ()| {
            let v = view(lua);
            let v = v.borrow();
            let out = lua.create_table()?;
            out.set("screen", v.screen)?;
            out.set("server", v.server.as_str())?;
            out.set("account", v.account.as_str())?;
            out.set("playonline", v.playonline)?;
            out.set("busy", v.busy)?;
            out.set("error", v.error.as_deref())?;
            out.set("selected", v.selected.map(|k| k + 1))?;
            let list = lua.create_table()?;
            for (k, c) in v.characters.iter().enumerate() {
                let e = lua.create_table()?;
                (e.set("slot", k + 1)?, e.set("name", c.name.as_str())?, e.set("world", c.world.as_str())?, e.set("race", c.race)?);
                (e.set("job", c.job)?, e.set("level", c.level)?, e.set("sub_job", c.sub_job)?, e.set("zone", c.zone)?, e.set("available", c.available)?);
                list.push(e)?;
            }
            out.set("characters", list)?;
            Ok(out)
        })?,
    )?;
    t.set(
        "sign_in",
        lua.create_function(|lua, o: Table| {
            need_action(lua)?;
            let act = Act::SignIn {
                server: o.get("server")?,
                account: o.get::<Option<String>>("account")?.unwrap_or_default(),
                password: o.get::<Option<String>>("password")?.unwrap_or_default(),
                otp: o.get::<Option<String>>("otp")?.unwrap_or_default(),
                pol_id: o.get("pol_id")?,
                pol_password: o.get::<Option<String>>("pol_password")?.unwrap_or_default(),
            };
            state(lua).borrow_mut().ops.push(super::Op::Lobby(act));
            Ok(())
        })?,
    )?;
    t.set(
        "play",
        lua.create_function(|lua, who: Value| {
            need_action(lua)?;
            let who = match who {
                Value::Integer(k) if k >= 1 => Who::Slot(k as usize - 1),
                Value::String(s) => Who::Name(s.to_str()?.to_string()),
                _ => return Err(mlua::Error::runtime("lobby.play takes a character's name or its slot (from 1)")),
            };
            state(lua).borrow_mut().ops.push(super::Op::Lobby(Act::Play(who)));
            Ok(())
        })?,
    )?;
    g.set("lobby", t)?;
    let storage: Table = g.get("storage")?;
    let me = mod_id.to_string();
    storage.set("seal", lua.create_function(move |_, text: String| crate::secret::seal(SEAL_SERVER, &me, &text).map_err(|e| mlua::Error::runtime(format!("{e:#}"))))?)?;
    let me = mod_id.to_string();
    storage.set("unseal", lua.create_function(move |_, sealed: String| Ok(crate::secret::open(SEAL_SERVER, &me, &sealed)))?)?;
    Ok(())
}
