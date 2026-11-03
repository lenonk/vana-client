//! Messages between game windows on one machine (docs/lua-mods.md 4.15, the `multibox` permission): each client binds
//! a local UDP port and lists it in the runtime folder; a mod's message goes to the same mod in the other clients.

use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

/// A message for a mod: (mod id, sending character, the character it is for or "", text).
pub(super) type Msg = (String, String, String, String);

pub(super) struct Bus {
    socket: UdpSocket,
    file: PathBuf,
    pub rx: Receiver<Msg>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

fn dir() -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(crate::config::data_dir).join("vana-client-ipc")
}

impl Bus {
    /// Binds a port and lists it (`<pid>.port`); a thread reads what the others send.
    pub fn open() -> Option<Self> {
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| log::warn!("lua ipc: {e}")).ok()?;
        let port = socket.local_addr().ok()?.port();
        let d = dir();
        std::fs::create_dir_all(&d).ok()?;
        let file = d.join(format!("{}.port", std::process::id()));
        std::fs::write(&file, port.to_string()).ok()?;
        let (tx, rx) = channel();
        let reader = socket.try_clone().ok()?;
        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = closed.clone();
        std::thread::Builder::new()
            .name("lua-ipc".into())
            .spawn(move || {
                let mut buf = vec![0u8; 65536];
                while let Ok(n) = reader.recv(&mut buf) {
                    if done.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) else { continue };
                    let field = |k: &str| v[k].as_str().unwrap_or_default().to_string();
                    if tx.send((field("mod"), field("from"), field("to"), field("text"))).is_err() {
                        return;
                    }
                }
            })
            .ok()?;
        log::info!("lua ipc: port {port}");
        Some(Self { socket, file, rx, closed })
    }

    /// Sends to every other listed client (stale entries of ended clients are removed).
    pub fn send(&self, mod_id: &str, from: &str, to: Option<&str>, text: &str) {
        let msg = serde_json::json!({"mod": mod_id, "from": from, "to": to, "text": text}).to_string();
        let Ok(rd) = std::fs::read_dir(dir()) else { return };
        for e in rd.flatten() {
            let path = e.path();
            if path == self.file || path.extension().is_none_or(|x| x != "port") {
                continue;
            }
            let pid: Option<u32> = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok());
            #[cfg(target_os = "linux")]
            if pid.is_some_and(|p| !std::path::Path::new(&format!("/proc/{p}")).exists()) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let _ = pid;
            if let Some(port) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u16>().ok()) {
                let _ = self.socket.send_to(msg.as_bytes(), ("127.0.0.1", port));
            }
        }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        // Wakes the reader so it ends.
        self.closed.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(a) = self.socket.local_addr() {
            let _ = self.socket.send_to(b"{}", a);
        }
    }
}
