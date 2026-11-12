//! Sockets for mods (docs/lua-mods.md 4.15, the `net` permission): TCP connections out and in, and UDP, for
//! companion programs. Each socket's reading runs on a thread; what it reads reaches the mod's handlers in the next
//! mod phase.

use super::api::{need, next_id, state};
use mlua::{Function, Lua, RegistryKey, Table, Value};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// Ids of accepted connections, made on the accept threads (above the mod's ids).
static NEXT: AtomicU64 = AtomicU64::new(1 << 40);

/// What the threads report.
pub(super) enum Msg {
    Open(u64, Closer),
    Data(u64, Vec<u8>, Option<SocketAddr>),
    Accept(u64, u64, SocketAddr, Sender<Vec<u8>>, Closer),
    Closed(u64, Option<String>),
}

pub(super) enum Kind {
    /// A stream (TCP, a Unix socket, a pipe): its writer's queue and what shuts it.
    Tcp {
        tx: Sender<Vec<u8>>,
        stream: Option<Closer>,
        stop: Arc<AtomicBool>,
    },
    Listen {
        stop: Arc<AtomicBool>,
    },
    Udp {
        socket: UdpSocket,
        stop: Arc<AtomicBool>,
    },
}

pub(super) struct Sock {
    pub id: u64,
    pub handlers: Option<RegistryKey>,
    pub kind: Kind,
}

impl Drop for Sock {
    fn drop(&mut self) {
        match &self.kind {
            Kind::Tcp { stream, stop, .. } => {
                stop.store(true, Ordering::Relaxed);
                if let Some(close) = stream {
                    close();
                }
            }
            Kind::Listen { stop } | Kind::Udp { stop, .. } => stop.store(true, Ordering::Relaxed),
        }
    }
}

/// The channel the threads report on, made with the first socket.
pub(super) fn reports(lua: &Lua) -> Sender<Msg> {
    let st = state(lua);
    let mut s = st.borrow_mut();
    if s.sock_tx.is_none() {
        let (tx, rx) = channel();
        (s.sock_tx, s.sock_rx) = (Some(tx), Some(rx));
    }
    s.sock_tx.clone().expect("made above")
}

/// Shuts a stream, which ends its reader.
pub(super) type Closer = Box<dyn Fn() + Send>;

fn tcp_closer(s: TcpStream) -> Closer {
    Box::new(move || {
        let _ = s.shutdown(Shutdown::Both);
    })
}

/// A TCP stream's reader and writer threads.
fn run_tcp(id: u64, s: &TcpStream, report: Sender<Msg>, out: Receiver<Vec<u8>>) -> std::io::Result<()> {
    run_stream(id, s.try_clone()?, s.try_clone()?, report, out).map(|_| ())
}

/// Where a reader thread is put once it runs, for a closer made before it (see `reader_closer`).
pub(super) type ReaderSlot = Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>;

/// Windows: a blocking read on a file or pipe never ends by itself; closing cancels the reader thread's read.
#[cfg(windows)]
pub(super) fn reader_closer(slot: ReaderSlot) -> Closer {
    use std::os::windows::io::AsRawHandle;
    Box::new(move || {
        if let Some(h) = slot.lock().ok().as_ref().and_then(|g| g.as_ref()) {
            // SAFETY: the JoinHandle keeps the thread handle open.
            unsafe { windows_sys::Win32::System::IO::CancelSynchronousIo(h.as_raw_handle() as _) };
        }
    })
}

/// A connected stream's reader and writer threads (the writer takes what `out` queues); the reader's handle.
fn run_stream<R: Read + Send + 'static, W: Write + Send + 'static>(
    id: u64,
    mut r: R,
    mut w: W,
    report: Sender<Msg>,
    out: Receiver<Vec<u8>>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("lua-tcp-w".into()).spawn(move || {
        for b in out {
            if w.write_all(&b).is_err() {
                return;
            }
        }
    })?;
    std::thread::Builder::new().name("lua-tcp-r".into()).spawn(move || {
        let mut buf = vec![0u8; 65536];
        loop {
            match r.read(&mut buf) {
                Ok(0) => {
                    let _ = report.send(Msg::Closed(id, None));
                    return;
                }
                Ok(n) => {
                    if report.send(Msg::Data(id, buf[..n].to_vec(), None)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = report.send(Msg::Closed(id, Some(e.to_string())));
                    return;
                }
            }
        }
    })
}

fn addr(host: &str, port: u16) -> mlua::Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| mlua::Error::runtime(format!("{host}:{port}: {e}")))?
        .next()
        .ok_or_else(|| mlua::Error::runtime(format!("{host}:{port}: no address")))
}

pub(super) fn keep(lua: &Lua, id: u64, handlers: Table, kind: Kind) -> mlua::Result<()> {
    let key = lua.create_registry_value(handlers)?;
    state(lua).borrow_mut().socks.push(Sock { id, handlers: Some(key), kind });
    Ok(())
}

/// The system's control sockets, refused: each lets a client run programs (the session bus, systemd, agents, docker).
const PIPES_REFUSED: [&str; 12] = [
    "bus",
    "dbus-1",
    "systemd",
    "gnupg",
    "keyring",
    "gcr",
    "ssh-agent",
    "openssh-ssh-agent",
    "docker.sock",
    "docker_engine",
    "podman",
    "containerd",
];

/// A pipe name: a path inside the runtime folder (as `app/com.discordapp.Discord/discord-ipc-0`), never out of it,
/// and not one of the system's control sockets.
fn pipe_allowed(name: &str) -> Result<(), &'static str> {
    let parts: Vec<&str> = name.split('/').collect();
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err("a name in the runtime folder, not a path out of it");
    }
    if cfg!(windows) && parts.len() > 1 {
        return Err("a pipe name has no folders");
    }
    let first = parts[0].to_ascii_lowercase();
    if PIPES_REFUSED.iter().any(|r| first == *r || first.starts_with(&format!("{r}."))) || first.starts_with("ssh-") {
        return Err("a system control socket, refused");
    }
    Ok(())
}

/// A local endpoint another program offers: a Unix socket (a bare name in the runtime folder, as Discord's
/// `discord-ipc-0`), or on Windows the named pipe `\\.\pipe\<name>`.
#[cfg(unix)]
fn open_pipe(name: &str, _: ReaderSlot) -> std::io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>, Closer)> {
    use std::os::unix::net::UnixStream;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("TMPDIR"))
        .map_or_else(|| std::path::PathBuf::from("/tmp"), std::path::PathBuf::from);
    let path = dir.join(name);
    let s = UnixStream::connect(path)?;
    let (r, w, c) = (s.try_clone()?, s.try_clone()?, s);
    Ok((
        Box::new(r),
        Box::new(w),
        Box::new(move || {
            let _ = c.shutdown(Shutdown::Both);
        }),
    ))
}

#[cfg(windows)]
fn open_pipe(name: &str, slot: ReaderSlot) -> std::io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>, Closer)> {
    let path = format!(r"\\.\pipe\{name}");
    let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let w = f.try_clone()?;
    Ok((Box::new(f), Box::new(w), reader_closer(slot)))
}

pub(super) fn install(lua: &Lua, t: &Table) -> mlua::Result<()> {
    t.set(
        "connect",
        lua.create_function(|lua, (host, port, handlers): (String, u16, Table)| {
            need(lua, "net")?;
            let id = next_id(&state(lua));
            let report = reports(lua);
            let (tx, rx) = channel::<Vec<u8>>();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::Builder::new()
                .name("lua-tcp".into())
                .spawn(move || {
                    let s = (host.as_str(), port)
                        .to_socket_addrs()
                        .map_err(|e| e.to_string())
                        .and_then(|mut a| a.next().ok_or_else(|| "no address".to_string()))
                        .and_then(|a| {
                            TcpStream::connect_timeout(&a, Duration::from_secs(10)).map_err(|e| e.to_string())
                        });
                    match s {
                        Ok(s) if !flag.load(Ordering::Relaxed) => {
                            let _ = s.set_nodelay(true);
                            // Open is reported before the reader starts, so no data comes ahead of it.
                            let r = s.try_clone().map(tcp_closer).and_then(|c| {
                                let _ = report.send(Msg::Open(id, c));
                                run_tcp(id, &s, report.clone(), rx)
                            });
                            if let Err(e) = r {
                                let _ = report.send(Msg::Closed(id, Some(e.to_string())));
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let _ = report.send(Msg::Closed(id, Some(e)));
                        }
                    }
                })
                .map_err(|e| mlua::Error::runtime(format!("net.connect: {e}")))?;
            keep(lua, id, handlers, Kind::Tcp { tx, stream: None, stop })?;
            Ok(id)
        })?,
    )?;
    t.set(
        "pipe",
        lua.create_function(|lua, (name, handlers): (String, Table)| {
            need(lua, "net")?;
            pipe_allowed(&name).map_err(|e| mlua::Error::runtime(format!("net.pipe {name:?}: {e}")))?;
            let id = next_id(&state(lua));
            let report = reports(lua);
            let (tx, rx) = channel::<Vec<u8>>();
            let stop = Arc::new(AtomicBool::new(false));
            std::thread::Builder::new()
                .name("lua-pipe".into())
                .spawn(move || {
                    let slot = ReaderSlot::default();
                    let r = open_pipe(&name, slot.clone()).and_then(|(r, w, close)| {
                        let _ = report.send(Msg::Open(id, close));
                        let h = run_stream(id, r, w, report.clone(), rx)?;
                        if let Ok(mut g) = slot.lock() {
                            *g = Some(h);
                        }
                        Ok(())
                    });
                    if let Err(e) = r {
                        let _ = report.send(Msg::Closed(id, Some(format!("{name}: {e}"))));
                    }
                })
                .map_err(|e| mlua::Error::runtime(format!("net.pipe: {e}")))?;
            keep(lua, id, handlers, Kind::Tcp { tx, stream: None, stop })?;
            Ok(id)
        })?,
    )?;
    t.set(
        "listen",
        lua.create_function(|lua, (port, handlers, opts): (u16, Table, Option<Table>)| {
            need(lua, "net")?;
            let host = opts
                .as_ref()
                .map(|o| o.get::<Option<String>>("host"))
                .transpose()?
                .flatten()
                .unwrap_or_else(|| "127.0.0.1".into());
            let l = TcpListener::bind(addr(&host, port)?)
                .map_err(|e| mlua::Error::runtime(format!("net.listen {host}:{port}: {e}")))?;
            let bound = l.local_addr().map(|a| a.port()).unwrap_or(port);
            l.set_nonblocking(true).map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let id = next_id(&state(lua));
            let report = reports(lua);
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::Builder::new()
                .name("lua-listen".into())
                .spawn(move || {
                    while !flag.load(Ordering::Relaxed) {
                        match l.accept() {
                            Ok((s, peer)) => {
                                let _ = s.set_nonblocking(false);
                                let _ = s.set_nodelay(true);
                                let cid = NEXT.fetch_add(1, Ordering::Relaxed);
                                let (tx, rx) = channel::<Vec<u8>>();
                                // Accept is reported before the reader starts, so no data comes ahead of it.
                                let Ok(c) = s.try_clone().map(tcp_closer) else { continue };
                                if report.send(Msg::Accept(id, cid, peer, tx, c)).is_err() {
                                    return;
                                }
                                if let Err(e) = run_tcp(cid, &s, report.clone(), rx) {
                                    let _ = report.send(Msg::Closed(cid, Some(e.to_string())));
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(50))
                            }
                            Err(e) => {
                                let _ = report.send(Msg::Closed(id, Some(e.to_string())));
                                return;
                            }
                        }
                    }
                })
                .map_err(|e| mlua::Error::runtime(format!("net.listen: {e}")))?;
            keep(lua, id, handlers, Kind::Listen { stop })?;
            Ok((id, bound))
        })?,
    )?;
    t.set(
        "udp",
        lua.create_function(|lua, (port, handlers, opts): (Option<u16>, Table, Option<Table>)| {
            need(lua, "net")?;
            let host = opts
                .as_ref()
                .map(|o| o.get::<Option<String>>("host"))
                .transpose()?
                .flatten()
                .unwrap_or_else(|| "127.0.0.1".into());
            let port = port.unwrap_or(0);
            let socket = UdpSocket::bind(addr(&host, port)?)
                .map_err(|e| mlua::Error::runtime(format!("net.udp {host}:{port}: {e}")))?;
            let bound = socket.local_addr().map(|a| a.port()).unwrap_or(port);
            let r = socket.try_clone().map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let _ = r.set_read_timeout(Some(Duration::from_millis(200)));
            let id = next_id(&state(lua));
            let report = reports(lua);
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::Builder::new()
                .name("lua-udp".into())
                .spawn(move || {
                    let mut buf = vec![0u8; 65536];
                    while !flag.load(Ordering::Relaxed) {
                        match r.recv_from(&mut buf) {
                            Ok((n, from)) => {
                                if report.send(Msg::Data(id, buf[..n].to_vec(), Some(from))).is_err() {
                                    return;
                                }
                            }
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                                ) => {}
                            Err(_) => {}
                        }
                    }
                })
                .map_err(|e| mlua::Error::runtime(format!("net.udp: {e}")))?;
            keep(lua, id, handlers, Kind::Udp { socket, stop })?;
            Ok((id, bound))
        })?,
    )?;
    t.set(
        "send",
        lua.create_function(|lua, (id, data, host, port): (u64, mlua::LuaString, Option<String>, Option<u16>)| {
            need(lua, "net")?;
            let st = state(lua);
            let s = st.borrow();
            let sock =
                s.socks.iter().find(|x| x.id == id).ok_or_else(|| mlua::Error::runtime(format!("no socket {id}")))?;
            match &sock.kind {
                Kind::Tcp { tx, .. } => {
                    tx.send(data.as_bytes().to_vec()).map_err(|_| mlua::Error::runtime("the connection is closed"))
                }
                Kind::Udp { socket, .. } => {
                    let (Some(h), Some(p)) = (host, port) else {
                        return Err(mlua::Error::runtime("a UDP send takes (id, data, host, port)"));
                    };
                    socket
                        .send_to(&data.as_bytes(), addr(&h, p)?)
                        .map(|_| ())
                        .map_err(|e| mlua::Error::runtime(e.to_string()))
                }
                Kind::Listen { .. } => Err(mlua::Error::runtime("send on a connection, not its listener")),
            }
        })?,
    )?;
    t.set("close", lua.create_function(|lua, id: u64| close(lua, id))?)?;
    Ok(())
}

/// Closes a socket or device by id and frees its handlers.
pub(super) fn close(lua: &Lua, id: u64) -> mlua::Result<()> {
    let st = state(lua);
    let gone: Vec<Sock> = {
        let mut s = st.borrow_mut();
        let (gone, kept) = std::mem::take(&mut s.socks).into_iter().partition(|x| x.id == id);
        s.socks = kept;
        gone
    };
    drop_all(lua, gone);
    Ok(())
}

/// Closes the sockets and frees their handlers.
fn drop_all(lua: &Lua, gone: Vec<Sock>) {
    for mut g in gone {
        if let Some(k) = g.handlers.take() {
            let _ = lua.remove_registry_value(k);
        }
    }
}

/// What the threads reported: each socket's handlers (open, data, accept, close).
pub(super) fn poll(vm: &super::vm::Vm) {
    let msgs: Vec<Msg> = match vm.st.borrow().sock_rx.as_ref() {
        Some(rx) => rx.try_iter().collect(),
        None => return,
    };
    for m in msgs {
        let id = match &m {
            Msg::Open(id, _) | Msg::Data(id, ..) | Msg::Accept(id, ..) | Msg::Closed(id, _) => *id,
        };
        let handlers: Option<Table> = vm
            .st
            .borrow()
            .socks
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.handlers.as_ref())
            .and_then(|k| vm.lua.registry_value(k).ok());
        let Some(h) = handlers else { continue };
        let f = |name: &str| h.get::<Option<Function>>(name).ok().flatten();
        match m {
            Msg::Open(id, stream) => {
                if let Some(Kind::Tcp { stream: s, .. }) =
                    vm.st.borrow_mut().socks.iter_mut().find(|s| s.id == id).map(|s| &mut s.kind)
                {
                    *s = Some(stream);
                }
                if let Some(f) = f("open") {
                    vm.call(f, id);
                }
            }
            Msg::Data(id, bytes, from) => {
                if let (Some(f), Ok(b)) = (f("data"), vm.lua.create_string(&bytes)) {
                    match from {
                        Some(a) => vm.call(f, (id, b, a.ip().to_string(), a.port())),
                        None => vm.call(f, (id, b)),
                    }
                }
            }
            Msg::Accept(lid, cid, peer, tx, stream) => {
                let Ok(key) = vm.lua.create_registry_value(h.clone()) else { continue };
                vm.st.borrow_mut().socks.push(Sock {
                    id: cid,
                    handlers: Some(key),
                    kind: Kind::Tcp { tx, stream: Some(stream), stop: Arc::new(AtomicBool::new(false)) },
                });
                if let Some(f) = f("accept") {
                    vm.call(f, (lid, cid, peer.ip().to_string(), peer.port()));
                }
            }
            Msg::Closed(id, err) => {
                let gone: Vec<Sock> = {
                    let mut s = vm.st.borrow_mut();
                    let (gone, kept) = std::mem::take(&mut s.socks).into_iter().partition(|x| x.id == id);
                    s.socks = kept;
                    gone
                };
                drop_all(&vm.lua, gone);
                if let Some(f) = f("close") {
                    vm.call(
                        f,
                        (id, err.map_or(Value::Nil, |e| vm.lua.create_string(&e).map_or(Value::Nil, Value::String))),
                    );
                }
            }
        }
    }
}
