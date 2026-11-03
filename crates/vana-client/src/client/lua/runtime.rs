//! The runtime helpers mods expect (docs/lua-mods.md 4.1, 4.12, 4.15): files in the mod's data folder, JSON, calls
//! and events between mods, Shift-JIS and auto-translate text, the clipboard.

use super::api::{state, world};
use mlua::{Function, Lua, LuaSerdeExt, MultiValue, Table, Value, WeakLua};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Every loaded mod, for mods.list / call / emit (shared with every VM as app data).
#[derive(Default)]
pub(super) struct Directory {
    pub mods: Vec<Entry>,
    /// mods.emit since the last mod phase: (sender, event, values).
    pub emitted: Vec<(String, String, Vec<serde_json::Value>)>,
}

pub(super) struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub lua: WeakLua,
    pub st: Rc<RefCell<super::vm::ModState>>,
}

pub(super) type Shared = Rc<RefCell<Directory>>;

fn directory(lua: &Lua) -> Shared {
    lua.app_data_ref::<Shared>().map(|r| r.clone()).unwrap_or_default()
}

/// A path inside a mod's folders: no root, no "..".
fn rel(path: &str) -> mlua::Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder"))),
            p if p.contains(':') => return Err(mlua::Error::runtime(format!("{path:?}: not a relative path"))),
            p => out.push(p),
        }
    }
    Ok(out)
}

/// Where a path lives: "shared/..." is the shared area (files.shared), anything else the mod's data folder.
fn place(lua: &Lua, data: &Path, path: &str) -> mlua::Result<PathBuf> {
    let r = rel(path)?;
    match r.strip_prefix("shared") {
        Ok(rest) => {
            super::api::need(lua, "files.shared")?;
            Ok(super::data_dir().join(".shared").join(rest))
        }
        Err(_) => Ok(data.join(r)),
    }
}

/// Whether the mod's own files have this path (a file or a folder).
fn in_mod(src: &super::vm::Source, r: &Path) -> bool {
    let name = r.to_string_lossy().replace('\\', "/");
    match src {
        super::vm::Source::Dir(d) => d.join(r).exists(),
        super::vm::Source::Bundled(files) => files.iter().any(|f| f.0 == name || f.0.starts_with(&format!("{name}/"))),
    }
}

fn io_err(path: &str, e: std::io::Error) -> mlua::Error {
    mlua::Error::runtime(format!("{path}: {e}"))
}

pub(super) fn install(lua: &Lua, g: &Table, vm: &super::vm::Vm) -> mlua::Result<()> {
    // fs: the mod's data folder (mod-data/<id>/files), reads falling back to the mod's own files.
    let data = super::data_dir().join(&vm.manifest.id).join("files");
    let src = vm.src.clone();
    let fs = lua.create_table()?;
    {
        let (data, src) = (data.clone(), src.clone());
        fs.set(
            "read",
            lua.create_function(move |lua, path: String| {
                let p = place(lua, &data, &path)?;
                match std::fs::read(&p) {
                    Ok(b) => Ok(Some(lua.create_string(b)?)),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let r = rel(&path)?;
                        match src.read(&r.to_string_lossy().replace('\\', "/")) {
                            Ok(t) => Ok(Some(lua.create_string(t)?)),
                            Err(_) => Ok(None),
                        }
                    }
                    Err(e) => Err(io_err(&path, e)),
                }
            })?,
        )?;
    }
    for (name, append) in [("write", false), ("append", true)] {
        let data = data.clone();
        fs.set(
            name,
            lua.create_function(move |lua, (path, text): (String, mlua::LuaString)| {
                let p = place(lua, &data, &path)?;
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d).map_err(|e| io_err(&path, e))?;
                }
                let r = if append {
                    use std::io::Write;
                    std::fs::OpenOptions::new().create(true).append(true).open(&p).and_then(|mut f| f.write_all(&text.as_bytes()))
                } else {
                    std::fs::write(&p, &*text.as_bytes())
                };
                r.map_err(|e| io_err(&path, e))
            })?,
        )?;
    }
    {
        let (data, src) = (data.clone(), src.clone());
        fs.set(
            "exists",
            lua.create_function(move |lua, path: String| {
                let p = place(lua, &data, &path)?;
                Ok(p.exists() || in_mod(&src, &rel(&path)?))
            })?,
        )?;
    }
    {
        let (data, src) = (data.clone(), src.clone());
        fs.set(
            "list",
            lua.create_function(move |lua, path: Option<String>| {
                let path = path.unwrap_or_default();
                let mut names: Vec<String> = Vec::new();
                let mut dirs = vec![place(lua, &data, &path)?];
                if let super::vm::Source::Dir(d) = &src
                    && !rel(&path)?.starts_with("shared")
                {
                    dirs.push(d.join(rel(&path)?));
                }
                for d in dirs {
                    for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                        let mut n = e.file_name().to_string_lossy().into_owned();
                        if e.path().is_dir() {
                            n.push('/');
                        }
                        if !names.contains(&n) {
                            names.push(n);
                        }
                    }
                }
                if let super::vm::Source::Bundled(files) = &src {
                    let prefix = rel(&path)?.to_string_lossy().replace('\\', "/");
                    for (f, _) in files.iter() {
                        let rest = if prefix.is_empty() { Some(*f) } else { f.strip_prefix(&format!("{prefix}/")) };
                        if let Some(rest) = rest {
                            let n = match rest.split_once('/') {
                                Some((d, _)) => format!("{d}/"),
                                None => rest.to_string(),
                            };
                            if !names.contains(&n) {
                                names.push(n);
                            }
                        }
                    }
                }
                names.sort();
                Ok(names)
            })?,
        )?;
    }
    {
        let data = data.clone();
        fs.set(
            "mkdir",
            lua.create_function(move |lua, path: String| {
                let p = place(lua, &data, &path)?;
                std::fs::create_dir_all(&p).map_err(|e| io_err(&path, e))
            })?,
        )?;
    }
    {
        let data = data.clone();
        fs.set(
            "remove",
            lua.create_function(move |lua, path: String| {
                let p = place(lua, &data, &path)?;
                let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                match r {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err(&path, e)),
                    _ => Ok(()),
                }
            })?,
        )?;
    }
    {
        let data = data.clone();
        fs.set(
            "rename",
            lua.create_function(move |lua, (from, to): (String, String)| {
                let (a, b) = (place(lua, &data, &from)?, place(lua, &data, &to)?);
                if let Some(d) = b.parent() {
                    std::fs::create_dir_all(d).map_err(|e| io_err(&to, e))?;
                }
                std::fs::rename(&a, &b).map_err(|e| io_err(&from, e))
            })?,
        )?;
    }
    g.set("fs", fs)?;

    // json: plain data both ways.
    let json = lua.create_table()?;
    json.set(
        "encode",
        lua.create_function(|lua, (v, opts): (Value, Option<Table>)| {
            let j: serde_json::Value = lua.from_value(v)?;
            let pretty = opts.and_then(|o| o.get::<Option<bool>>("pretty").ok().flatten()).unwrap_or(false);
            let s = if pretty { serde_json::to_string_pretty(&j) } else { serde_json::to_string(&j) };
            s.map_err(|e| mlua::Error::runtime(format!("json.encode: {e}")))
        })?,
    )?;
    json.set(
        "decode",
        lua.create_function(|lua, text: mlua::LuaString| {
            let j: serde_json::Value = serde_json::from_slice(&text.as_bytes()).map_err(|e| mlua::Error::runtime(format!("json.decode: {e}")))?;
            super::api::to_lua(lua, &j)
        })?,
    )?;
    g.set("json", json)?;

    // mods: the others, their services and events.
    let mods = lua.create_table()?;
    mods.set(
        "list",
        lua.create_function(|lua, ()| {
            let out = lua.create_table()?;
            for m in directory(lua).borrow().mods.iter() {
                let t = lua.create_table()?;
                t.set("id", m.id.as_str())?;
                t.set("name", m.name.as_str())?;
                t.set("version", m.version.as_str())?;
                let st = m.st.borrow();
                let provides: Vec<String> = st.provided.iter().map(|p| p.0.clone()).collect();
                t.set("provides", provides)?;
                (t.set("avg_ms", st.stats.avg)?, t.set("peak_ms", st.stats.peak)?, t.set("errors", st.errors.len())?);
                if let Some(l) = m.lua.try_upgrade() {
                    t.set("memory_kb", l.used_memory() / 1024)?;
                }
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    mods.set(
        "installed",
        lua.create_function(|lua, ()| {
            let loaded: Vec<String> = directory(lua).borrow().mods.iter().map(|m| m.id.clone()).collect();
            let out = lua.create_table()?;
            for (id, name, version, enabled, bundled) in super::installed() {
                let t = lua.create_table()?;
                (t.set("loaded", loaded.contains(&id))?, t.set("id", id)?, t.set("name", name)?, t.set("version", version)?);
                (t.set("enabled", enabled)?, t.set("bundled", bundled)?);
                out.push(t)?;
            }
            Ok(out)
        })?,
    )?;
    mods.set(
        "provide",
        lua.create_function(|lua, (name, func): (String, Function)| {
            let st = state(lua);
            let key = lua.create_registry_value(func)?;
            let old = {
                let mut s = st.borrow_mut();
                let old = s.provided.iter().position(|p| p.0 == name).map(|k| s.provided.remove(k).1);
                s.provided.push((name, key));
                old
            };
            if let Some(k) = old {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    mods.set("call", lua.create_function(|lua, (id, name, args): (String, String, mlua::Variadic<Value>)| call(lua, &id, &name, args))?)?;
    let me = vm.manifest.id.clone();
    mods.set(
        "emit",
        lua.create_function(move |lua, (event, args): (String, mlua::Variadic<Value>)| {
            let vals = args.into_iter().map(|v| lua.from_value::<serde_json::Value>(v)).collect::<mlua::Result<Vec<_>>>()?;
            directory(lua).borrow_mut().emitted.push((me.clone(), event, vals));
            Ok(())
        })?,
    )?;
    g.set("mods", mods)?;

    // text: Shift-JIS and auto-translate.
    let text = lua.create_table()?;
    text.set("from_shift_jis", lua.create_function(|_, b: mlua::LuaString| Ok(crate::dat::font::decode_sjis(&b.as_bytes())))?)?;
    text.set("to_shift_jis", lua.create_function(|lua, s: String| lua.create_string(crate::dat::font::encode_sjis(&s)))?)?;
    text.set("plain", lua.create_function(|lua, s: String| Ok(super::super::fep::plain(world(lua)?, &s)))?)?;
    text.set("phrase", lua.create_function(|_, (group, id): (u8, u8)| Ok(phrase([2, 2, group, id])))?)?;
    text.set(
        "autotranslate",
        lua.create_function(|lua, s: String| {
            let w = world(lua)?;
            let Some(d) = w.battle.as_ref().and_then(|b| b.phrases.as_ref()) else { return Ok(None) };
            let want = s.trim().to_lowercase();
            let hit = d.groups.iter().find_map(|g| {
                if g.title.to_lowercase().trim_matches(|c| c == '【' || c == '】') == want.as_str() {
                    return Some([2, 2, g.id, 0]);
                }
                g.phrases.iter().find(|p| p.1.to_lowercase() == want).map(|p| [2, 2, g.id, p.0])
            });
            Ok(hit.map(phrase))
        })?,
    )?;
    g.set("text", text)?;

    // client: the clipboard and the window.
    let client = lua.create_table()?;
    client.set(
        "copy",
        lua.create_function(|lua, s: String| {
            state(lua).borrow_mut().ops.push(super::Op::Copy(s));
            Ok(())
        })?,
    )?;
    client.set(
        "paste",
        lua.create_function(|lua, handler: Function| {
            super::api::need(lua, "clipboard")?;
            let key = lua.create_registry_value(handler)?;
            let st = state(lua);
            let mut s = st.borrow_mut();
            s.paste_handlers.push(key);
            s.ops.push(super::Op::Paste);
            Ok(())
        })?,
    )?;
    client.set(
        "option",
        lua.create_function(|lua, name: String| {
            let v = super::option(world(lua)?, &name).ok_or_else(|| mlua::Error::runtime(format!("no option {name:?} ({})", super::OPTIONS.join(", "))))?;
            super::api::to_lua(lua, &v)
        })?,
    )?;
    let me = vm.manifest.id.clone();
    client.set(
        "set_option",
        lua.create_function(move |lua, (name, v): (String, Value)| {
            if !super::OPTIONS.contains(&name.as_str()) {
                return Err(mlua::Error::runtime(format!("no option {name:?} ({})", super::OPTIONS.join(", "))));
            }
            let v: serde_json::Value = lua.from_value(v)?;
            state(lua).borrow_mut().ops.push(super::Op::Option(me.clone(), name, v));
            Ok(())
        })?,
    )?;
    client.set(
        "window",
        lua.create_function(|lua, ()| {
            let f = lua.app_data_ref::<Rc<RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default();
            let f = f.borrow();
            let t = lua.create_table()?;
            if let Some((x, y)) = f.win_pos {
                (t.set("x", x)?, t.set("y", y)?);
            }
            (t.set("w", f.window[0] as i64)?, t.set("h", f.window[1] as i64)?, t.set("focused", f.focused)?);
            Ok(t)
        })?,
    )?;
    client.set(
        "set_window",
        lua.create_function(|lua, t: Table| {
            super::api::need(lua, "multibox")?;
            let pos = match (t.get::<Option<i32>>("x")?, t.get::<Option<i32>>("y")?) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            };
            let size = match (t.get::<Option<u32>>("w")?, t.get::<Option<u32>>("h")?) {
                (Some(w), Some(h)) => Some((w.max(320), h.max(240))),
                _ => None,
            };
            let req = super::WinReq {
                pos,
                size,
                top: t.get("top")?,
                minimised: t.get("minimised")?,
                fullscreen: t.get("fullscreen")?,
                border: t.get("border")?,
                protected: t.get::<Option<bool>>("capture")?.map(|c| !c),
                ..Default::default()
            };
            state(lua).borrow_mut().ops.push(super::Op::Window(req));
            Ok(())
        })?,
    )?;
    client.set(
        "focus",
        lua.create_function(|lua, ()| {
            super::api::need(lua, "multibox")?;
            state(lua).borrow_mut().ops.push(super::Op::Window(super::WinReq { focus: true, ..Default::default() }));
            Ok(())
        })?,
    )?;
    client.set(
        "screenshot",
        lua.create_function(|lua, name: Option<String>| {
            let name = match name {
                Some(n) => {
                    let p = Path::new(&n);
                    if p.is_absolute() || p.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                        return Err(mlua::Error::runtime(format!("{n:?}: a name under the screenshots folder")));
                    }
                    if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("png")) { format!("{n}.png") } else { n }
                }
                None => jiff::Zoned::now().strftime("mod%y%m%d%H%M%S.png").to_string(),
            };
            state(lua).borrow_mut().ops.push(super::Op::Shot(name.clone()));
            Ok(name)
        })?,
    )?;
    client.set(
        "run",
        lua.create_function(|lua, (program, args, handler): (String, Option<Vec<String>>, Option<Function>)| {
            super::api::need(lua, "exec")?;
            let mut cmd = std::process::Command::new(&program);
            cmd.args(args.unwrap_or_default()).stdin(std::process::Stdio::null());
            let Some(handler) = handler else {
                cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                let child = cmd.spawn().map_err(|e| mlua::Error::runtime(format!("{program}: {e}")))?;
                return Ok(child.id());
            };
            cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            let child = cmd.spawn().map_err(|e| mlua::Error::runtime(format!("{program}: {e}")))?;
            let pid = child.id();
            let st = state(lua);
            let id = super::api::next_id(&st);
            let key = lua.create_registry_value(handler)?;
            let tx = {
                let mut s = st.borrow_mut();
                s.net_pending.push((id, key));
                if s.net_rx.is_none() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    (s.net_rx, s.net_tx) = (Some(rx), Some(tx));
                }
                s.net_tx.clone().expect("made above")
            };
            // The answer arrives as an HTTP one does: {status (the exit code), body (stdout), headers {stderr}}.
            std::thread::Builder::new()
                .name("lua-run".into())
                .spawn(move || {
                    let r = child.wait_with_output().map_err(|e| e.to_string()).map(|o| (o.status.code().unwrap_or(-1).clamp(0, 65535) as u16, o.stdout, vec![("stderr".to_string(), String::from_utf8_lossy(&o.stderr).into_owned())]));
                    let _ = tx.send((id, r));
                })
                .map_err(|e| mlua::Error::runtime(format!("client.run: {e}")))?;
            Ok(pid)
        })?,
    )?;
    super::process::install(lua, &client, &vm.manifest.id)?;
    g.set("client", client)?;
    Ok(())
}

/// An auto-translate phrase as the line holds it: its block (kind, language, group, id) as four private-use chars,
/// which the Shift-JIS encoder writes back as `FD .. FD`.
pub(super) fn phrase(b: [u8; 4]) -> String {
    b.iter().enumerate().filter_map(|(k, &x)| char::from_u32(crate::dat::font::PHRASE_BASE + 0x100 * k as u32 + x as u32)).collect()
}

/// mods.call: the other mod's provided function with copies of the values (plain data), in its own VM.
fn call(lua: &Lua, id: &str, name: &str, args: mlua::Variadic<Value>) -> mlua::Result<MultiValue> {
    let dir = directory(lua);
    let (tl, st) = {
        let d = dir.borrow();
        let m = d.mods.iter().find(|m| m.id.eq_ignore_ascii_case(id)).ok_or_else(|| mlua::Error::runtime(format!("mods.call: no mod {id} loaded")))?;
        (m.lua.try_upgrade().ok_or_else(|| mlua::Error::runtime(format!("mods.call: {id} is gone")))?, m.st.clone())
    };
    let f: Function = {
        let s = st.borrow();
        let p = s.provided.iter().find(|p| p.0 == name).ok_or_else(|| mlua::Error::runtime(format!("mods.call: {id} provides no {name}")))?;
        tl.registry_value(&p.1)?
    };
    let vals = args.into_iter().map(|v| lua.from_value::<serde_json::Value>(v)).collect::<mlua::Result<Vec<_>>>()?;
    let targs = vals.iter().map(|j| super::api::to_lua(&tl, j)).collect::<mlua::Result<Vec<_>>>()?;
    // The callee reads the world the caller was lent, under its own budget.
    let lent = tl.app_data_ref::<super::api::WorldRef>().is_some();
    if !lent && let Some(p) = lua.app_data_ref::<super::api::WorldRef>().map(|r| r.0) {
        tl.set_app_data(super::api::WorldRef(p));
    }
    let deadline = st.borrow_mut().deadline.replace(std::time::Instant::now() + super::vm::FILTER_CAP);
    let r = f.call::<MultiValue>(MultiValue::from_vec(targs));
    st.borrow_mut().deadline = deadline;
    if !lent {
        tl.remove_app_data::<super::api::WorldRef>();
    }
    let out = r.map_err(|e| mlua::Error::runtime(format!("mods.call {id}.{name}: {e}")))?;
    let back = out.into_iter().map(|v| tl.from_value::<serde_json::Value>(v)).collect::<mlua::Result<Vec<_>>>()?;
    Ok(MultiValue::from_vec(back.iter().map(|j| super::api::to_lua(lua, j)).collect::<mlua::Result<Vec<_>>>()?))
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths() {
        assert!(super::rel("../x").is_err());
        assert!(super::rel("a/../../x").is_err());
        assert!(super::rel("C:/x").is_err());
        assert_eq!(super::rel("/a//b/./c.txt").unwrap(), std::path::PathBuf::from("a/b/c.txt"));
    }
}
