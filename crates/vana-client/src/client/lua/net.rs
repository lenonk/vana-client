//! The network for mods (docs/lua-mods.md 4.15, the `net` permission): HTTP(S) requests on their own threads, each
//! answer handed to the mod's callback in a later mod phase, and opening a URL in the browser.

use super::api::{need, next_id, state};
use mlua::{Function, Lua, Table};
use std::time::Duration;

/// An answer for a mod: its request's id and {status, body, headers} or an error.
pub(super) type Answer = (u64, Result<(u16, Vec<u8>, Vec<(String, String)>), String>);

/// A body larger than this is cut (16 MB).
const MAX_BODY: u64 = 16 << 20;

fn fetch(method: &str, url: &str, headers: &[(String, String)], body: Option<Vec<u8>>, timeout: Duration) -> Result<(u16, Vec<u8>, Vec<(String, String)>), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!("vana-client/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let resp = match (method, body) {
        ("GET", _) | ("HEAD", _) | ("DELETE", None) => {
            let mut r = match method {
                "HEAD" => agent.head(url),
                "DELETE" => agent.delete(url),
                _ => agent.get(url),
            };
            for (k, v) in headers {
                r = r.header(k, v);
            }
            r.call()
        }
        (m, b) => {
            let mut r = match m {
                "PUT" => agent.put(url),
                "PATCH" => agent.patch(url),
                _ => agent.post(url),
            };
            for (k, v) in headers {
                r = r.header(k, v);
            }
            r.send(&b.unwrap_or_default()[..])
        }
    };
    let mut resp = resp.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let hdrs = resp.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    let body = resp.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(|e| e.to_string())?;
    Ok((status, body, hdrs))
}

pub(super) fn install(lua: &Lua, g: &Table) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set(
        "request",
        lua.create_function(|lua, (spec, callback): (Table, Function)| {
            need(lua, "net")?;
            let url: String = spec.get("url")?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(mlua::Error::runtime(format!("{url:?}: only http and https")));
            }
            let method = spec.get::<Option<String>>("method")?.unwrap_or_else(|| "GET".into()).to_ascii_uppercase();
            let body = spec.get::<Option<mlua::LuaString>>("body")?.map(|b| b.as_bytes().to_vec());
            let headers: Vec<(String, String)> = match spec.get::<Option<Table>>("headers")? {
                Some(h) => h.pairs::<String, String>().collect::<mlua::Result<_>>()?,
                None => Vec::new(),
            };
            let timeout = Duration::from_secs_f64(spec.get::<Option<f64>>("timeout")?.unwrap_or(30.0).clamp(1.0, 300.0));
            let st = state(lua);
            let id = next_id(&st);
            let key = lua.create_registry_value(callback)?;
            let tx = {
                let mut s = st.borrow_mut();
                s.net_pending.push((id, key));
                if s.net_rx.is_none() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    s.net_rx = Some(rx);
                    s.net_tx = Some(tx);
                }
                s.net_tx.clone().expect("made above")
            };
            std::thread::Builder::new()
                .name("lua-net".into())
                .spawn(move || {
                    let _ = tx.send((id, fetch(&method, &url, &headers, body, timeout)));
                })
                .map_err(|e| mlua::Error::runtime(format!("net.request: {e}")))?;
            Ok(id)
        })?,
    )?;
    t.set(
        "open_url",
        lua.create_function(|lua, url: String| {
            need(lua, "net")?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(mlua::Error::runtime(format!("{url:?}: only http and https")));
            }
            #[cfg(target_os = "windows")]
            let r = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
            #[cfg(target_os = "macos")]
            let r = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let r = std::process::Command::new("xdg-open").arg(&url).spawn();
            r.map(|_| ()).map_err(|e| mlua::Error::runtime(format!("open_url: {e}")))
        })?,
    )?;
    super::sock::install(lua, &t)?;
    g.set("net", t)?;
    Ok(())
}

/// Answers that came in: each request's callback with {status, body, headers} or {error}.
pub(super) fn poll(vm: &super::vm::Vm) {
    let answers: Vec<Answer> = match vm.st.borrow().net_rx.as_ref() {
        Some(rx) => rx.try_iter().collect(),
        None => return,
    };
    for (id, r) in answers {
        let key = {
            let mut s = vm.st.borrow_mut();
            s.net_pending.iter().position(|p| p.0 == id).map(|k| s.net_pending.remove(k).1)
        };
        let Some(key) = key else { continue };
        let f: Option<Function> = vm.lua.registry_value(&key).ok();
        let _ = vm.lua.remove_registry_value(key);
        let Some(f) = f else { continue };
        let res = (|| -> mlua::Result<Table> {
            let t = vm.lua.create_table()?;
            match r {
                Ok((status, body, headers)) => {
                    t.set("status", status)?;
                    t.set("body", vm.lua.create_string(&body)?)?;
                    let h = vm.lua.create_table()?;
                    for (k, v) in headers {
                        h.set(k.to_ascii_lowercase(), v)?;
                    }
                    t.set("headers", h)?;
                }
                Err(e) => t.set("error", e)?,
            }
            Ok(t)
        })();
        match res {
            Ok(t) => vm.call(f, t),
            Err(e) => vm.error(&e),
        }
    }
}
