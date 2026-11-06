//! LAN voice chat (ours, not retail; hidden, Ctrl+Alt+Shift+V opens its window: client/voicechat.rs). Clients find
//! each other by beacons to a multicast group and the broadcast address (or addresses typed in), then send 10 ms
//! frames of 48 kHz 16-bit PCM over UDP to everyone on their channel. Only LAN addresses are sent to or heard.

mod mic;
mod play;
mod wav;
pub mod wire;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wire::Packet;

pub const RATE: u32 = 48_000;
pub const FRAME: usize = 480;
/// The beacons' port and group.
pub const PORT: u16 = 54390;
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 70, 88);
const BEACON: Duration = Duration::from_secs(2);
const GONE: Duration = Duration::from_secs(7);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Talk {
    #[default]
    Voice,
    Push,
}

/// Push-to-talk keys: (config name, label).
pub const PTT_KEYS: [(&str, &str); 7] =
    [("mouse4", "Mouse button 4"), ("mouse5", "Mouse button 5"), ("right_ctrl", "Right Ctrl"), ("right_alt", "Right Alt"), ("right_shift", "Right Shift"), ("backquote", "` (backquote)"), ("caps_lock", "Caps Lock")];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub talk: Talk,
    /// One of PTT_KEYS' names.
    pub ptt_key: String,
    /// Voice activity opens at this level (dBFS).
    pub threshold_db: i32,
    /// Percent.
    pub mic_gain: u32,
    pub volume: u32,
    pub mic_muted: bool,
    /// RNNoise on the microphone before the gain (voice/mic.rs Denoise).
    pub noise_suppression: bool,
    pub deafened: bool,
    /// cpal input id; empty = the default.
    pub input: String,
    /// Only players on the same channel hear each other (empty is a channel too).
    pub channel: String,
    /// Addresses typed in: a host or IPv4, with or without a port.
    pub peers: Vec<String>,
    /// Players muted, by name (lowercase).
    pub muted: Vec<String>,
    pub show_talking: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, talk: Talk::Voice, ptt_key: "mouse4".into(), threshold_db: -45, mic_gain: 100, volume: 100, mic_muted: false, noise_suppression: true, deafened: false, input: String::new(), channel: String::new(), peers: Vec::new(), muted: Vec::new(), show_talking: true }
    }
}

/// Push-to-talk held (set from the window's raw input), and the key it listens to (an index into PTT_KEYS).
static PTT_HELD: AtomicBool = AtomicBool::new(false);
static PTT_KEY: AtomicU32 = AtomicU32::new(0);

/// A key from the window (winit's physical key), for push-to-talk.
pub fn raw_key(code: winit::keyboard::KeyCode, pressed: bool) {
    use winit::keyboard::KeyCode as K;
    let name = match code {
        K::ControlRight => "right_ctrl",
        K::AltRight => "right_alt",
        K::ShiftRight => "right_shift",
        K::Backquote => "backquote",
        K::CapsLock => "caps_lock",
        _ => return,
    };
    held(name, pressed);
}

pub fn raw_button(button: winit::event::MouseButton, pressed: bool) {
    use winit::event::MouseButton as B;
    match button {
        B::Back => held("mouse4", pressed),
        B::Forward => held("mouse5", pressed),
        _ => {}
    }
}

/// The window lost the focus: nothing is held any more.
pub fn release() {
    PTT_HELD.store(false, Ordering::Relaxed);
}

fn held(name: &str, pressed: bool) {
    if PTT_KEYS.get(PTT_KEY.load(Ordering::Relaxed) as usize).is_some_and(|k| k.0 == name) {
        PTT_HELD.store(pressed, Ordering::Relaxed);
    }
}

struct Peer {
    name: String,
    channel: String,
    addr: SocketAddrV4,
    seen: Instant,
    heard: Option<Instant>,
}

/// What the threads share.
pub struct Shared {
    stop: AtomicBool,
    session: u64,
    sock: UdpSocket,
    port: u16,
    name: Mutex<String>,
    cfg: Mutex<Settings>,
    peers: Mutex<HashMap<u64, Peer>>,
    play: Mutex<play::Play>,
    /// The typed addresses as resolved: (as typed, its LAN address if it has one).
    manual: Mutex<Vec<(String, Option<SocketAddrV4>)>>,
    volume: AtomicU32,
    deaf: AtomicBool,
    level: AtomicU32,
    talking: AtomicBool,
}

impl Shared {
    fn send_audio(&self, seq: u32, pcm: Vec<i16>) {
        let to: Vec<SocketAddrV4> = {
            let chan = self.cfg.lock().unwrap().channel.clone();
            self.peers.lock().unwrap().values().filter(|p| p.channel.eq_ignore_ascii_case(&chan)).map(|p| p.addr).collect()
        };
        if to.is_empty() {
            return;
        }
        let b = wire::encode(&Packet::Audio { session: self.session, seq, pcm });
        for a in to {
            let _ = self.sock.send_to(&b, a);
        }
    }

    fn hello(&self) -> Vec<u8> {
        let channel = self.cfg.lock().unwrap().channel.clone();
        wire::encode(&Packet::Hello { session: self.session, name: self.name.lock().unwrap().clone(), channel })
    }

    /// Beacons to the group, the broadcast address and the typed addresses; a hello to every peer keeps both ways
    /// open where the beacons don't reach. Peers not heard from go.
    fn beacon(&self) {
        let b = self.hello();
        let _ = self.sock.send_to(&b, (GROUP, self.port));
        let _ = self.sock.send_to(&b, (Ipv4Addr::BROADCAST, self.port));
        for a in self.manual.lock().unwrap().iter().filter_map(|m| m.1) {
            let _ = self.sock.send_to(&b, a);
        }
        let now = Instant::now();
        let mut gone = Vec::new();
        {
            let mut peers = self.peers.lock().unwrap();
            peers.retain(|k, p| {
                let keep = now.duration_since(p.seen) < GONE;
                if !keep {
                    log::info!("voice: {} ({}) gone", p.name, p.addr);
                    gone.push(*k);
                }
                keep
            });
            for p in peers.values() {
                let _ = self.sock.send_to(&b, p.addr);
            }
        }
        let mut play = self.play.lock().unwrap();
        for k in gone {
            play.forget(k);
        }
    }

    fn handle(&self, b: &[u8], from: SocketAddrV4) {
        if !wire::lan(*from.ip()) {
            return;
        }
        match wire::decode(b) {
            Some(Packet::Hello { session, name, channel }) if session != self.session => {
                let mut peers = self.peers.lock().unwrap();
                let now = Instant::now();
                match peers.get_mut(&session) {
                    Some(p) => {
                        (p.name, p.channel, p.addr, p.seen) = (name, channel, from, now);
                    }
                    None => {
                        log::info!("voice: {name} at {from} (channel {channel:?})");
                        peers.insert(session, Peer { name, channel, addr: from, seen: now, heard: None });
                    }
                }
            }
            // From a peer (at its hellos' address) on this channel, not muted; deafened, it still shows as talking.
            Some(Packet::Audio { session, seq, pcm }) => {
                let Some((name, channel)) = self.peers.lock().unwrap().get(&session).filter(|p| p.addr == from).map(|p| (p.name.to_lowercase(), p.channel.clone())) else { return };
                let deaf = {
                    let c = self.cfg.lock().unwrap();
                    if !c.channel.eq_ignore_ascii_case(&channel) || c.muted.contains(&name) {
                        return;
                    }
                    c.deafened
                };
                if let Some(p) = self.peers.lock().unwrap().get_mut(&session) {
                    p.heard = Some(Instant::now());
                }
                if !deaf {
                    self.play.lock().unwrap().push(session, seq, pcm);
                }
            }
            // Only from the address the peer's hellos come from (a session id is in every beacon).
            Some(Packet::Bye { session }) => {
                let mut peers = self.peers.lock().unwrap();
                if peers.get(&session).is_none_or(|p| p.addr != from) {
                    return;
                }
                if let Some(p) = peers.remove(&session) {
                    log::info!("voice: {} left", p.name);
                }
                drop(peers);
                self.play.lock().unwrap().forget(session);
            }
            _ => {}
        }
    }
}

/// A peer as the window lists it.
#[derive(Clone, Debug)]
pub struct PeerView {
    pub name: String,
    pub addr: SocketAddrV4,
    pub channel: String,
    pub same: bool,
    pub talking: bool,
    pub muted: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub peers: Vec<PeerView>,
    /// The microphone's level after its gain (dBFS), and whether it is sending.
    pub level_db: f32,
    pub talking: bool,
    /// The typed addresses that have no LAN address.
    pub unresolved: Vec<String>,
}

/// The microphone closes this long after it was muted.
const MIC_LINGER: Duration = Duration::from_secs(10);

/// A running voice chat: dropped, it says goodbye and its threads end.
pub struct Engine {
    shared: Arc<Shared>,
    _out: Option<crate::audio::output::Output>,
    /// Closed while muted: any recording drops a Bluetooth headset to its low-quality headset profile
    /// (WirePlumber's autoswitch). None closed, Some(Err) failed to open.
    mic: Option<Result<mic::Mic, String>>,
    /// The device setting the microphone was opened with.
    opened_with: String,
    /// No sound devices (the tests): never opened.
    no_mic: bool,
    stand_mic: Option<std::ffi::OsString>,
    /// Since when the open microphone is muted.
    idle: Option<Instant>,
    linger: Duration,
    /// The beacon port this one listens on, when it could take it.
    pub discovery: Result<u16, String>,
}

/// Input devices for the window's Microphone row: (cpal id, description).
pub fn devices() -> Vec<(String, String)> {
    mic::devices()
}

impl Engine {
    /// `output`: the cpal output id ("null" in test runs, empty for the default).
    pub fn start(cfg: &Settings, name: &str, output: &str) -> anyhow::Result<Engine> {
        Self::start_on(cfg, name, Some(output), PORT, Stand { mic: std::env::var_os("FFXI_VOICE_MIC"), rec: std::env::var_os("FFXI_VOICE_REC") })
    }

    /// `output` None: no sound devices at all (the tests).
    fn start_on(cfg: &Settings, name: &str, output: Option<&str>, port: u16, stand: Stand) -> anyhow::Result<Engine> {
        let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        sock.set_broadcast(true)?;
        sock.set_multicast_ttl_v4(1)?;
        sock.set_multicast_loop_v4(true)?;
        sock.set_read_timeout(Some(Duration::from_millis(100)))?;
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(|e| anyhow::anyhow!("random session: {e}"))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            session: u64::from_le_bytes(id),
            sock,
            port,
            name: Mutex::new(wire::clip(name, wire::NAME_MAX)),
            cfg: Mutex::new(cfg.clone()),
            peers: Mutex::new(HashMap::new()),
            play: Mutex::new(play::Play::default()),
            manual: Mutex::new(Vec::new()),
            volume: AtomicU32::new(cfg.volume),
            deaf: AtomicBool::new(cfg.deafened),
            level: AtomicU32::new((-100f32).to_bits()),
            talking: AtomicBool::new(false),
        });
        let discovery = match discovery_socket(port) {
            Ok(d) => {
                let sh = shared.clone();
                std::thread::Builder::new().name("voice-find".into()).spawn(move || listen(sh, d))?;
                Ok(port)
            }
            Err(e) => {
                log::warn!("voice: beacon port {port}: {e}");
                Err(e.to_string())
            }
        };
        let sh = shared.clone();
        std::thread::Builder::new().name("voice-net".into()).spawn(move || net(sh))?;
        let rec = stand.rec;
        let out = output.and_then(|o| crate::audio::output::open((!o.is_empty()).then_some(o), |rate| play::Out::new(shared.clone(), rate, rec)).map_err(|e| log::warn!("voice: no output: {e:#}")).ok());
        let engine = Engine { shared, _out: out, mic: None, opened_with: String::new(), no_mic: output.is_none(), stand_mic: stand.mic, idle: None, linger: MIC_LINGER, discovery };
        engine.apply(cfg);
        log::info!("voice: on as {:?}, session {:016x}", name, engine.shared.session);
        Ok(engine)
    }

    /// Every frame: the microphone opened while not muted, closed a while after a mute.
    pub fn tick(&mut self) {
        if self.no_mic {
            return;
        }
        let (want, input) = {
            let c = self.shared.cfg.lock().unwrap();
            (!c.mic_muted, c.input.clone())
        };
        let now = Instant::now();
        // Another device picked: the open one closes, the new one opens now if wanted.
        if self.mic.is_some() && self.opened_with != input {
            (self.mic, self.idle) = (None, None);
        }
        if want {
            self.idle = None;
            if self.mic.is_none() {
                let m = mic::start(self.shared.clone(), &input, self.stand_mic.as_deref()).map_err(|e| {
                    log::warn!("voice: {e:#}");
                    format!("{e:#}")
                });
                if let Ok(m) = &m {
                    log::info!("voice: microphone on ({})", m.name);
                }
                (self.mic, self.opened_with) = (Some(m), input);
            }
        } else if self.mic.is_some() && now.duration_since(*self.idle.get_or_insert(now)) >= self.linger {
            if matches!(self.mic, Some(Ok(_))) {
                log::info!("voice: microphone off (muted)");
            }
            (self.mic, self.idle) = (None, None);
            self.shared.level.store((-100f32).to_bits(), Ordering::Relaxed);
            self.shared.talking.store(false, Ordering::Relaxed);
        }
    }

    /// Settings changed (a new microphone device is opened by `tick`).
    pub fn apply(&self, cfg: &Settings) {
        let peers_changed = {
            let mut c = self.shared.cfg.lock().unwrap();
            let changed = c.peers != cfg.peers || self.shared.manual.lock().unwrap().len() != cfg.peers.len();
            *c = cfg.clone();
            changed
        };
        self.shared.volume.store(cfg.volume, Ordering::Relaxed);
        self.shared.deaf.store(cfg.deafened, Ordering::Relaxed);
        PTT_KEY.store(PTT_KEYS.iter().position(|k| k.0 == cfg.ptt_key).unwrap_or(0) as u32, Ordering::Relaxed);
        if peers_changed {
            let (sh, list) = (self.shared.clone(), cfg.peers.clone());
            // Host names can take a while to resolve.
            let _ = std::thread::Builder::new().name("voice-resolve".into()).spawn(move || {
                let resolved = list.iter().map(|s| (s.clone(), resolve(s))).collect();
                *sh.manual.lock().unwrap() = resolved;
                sh.beacon();
            });
        }
    }

    pub fn set_name(&self, name: &str) {
        let name = wire::clip(name, wire::NAME_MAX);
        let mut n = self.shared.name.lock().unwrap();
        if *n != name {
            *n = name;
        }
    }

    /// The open microphone's name, None while closed, or why it could not open.
    pub fn mic(&self) -> Result<Option<&str>, &str> {
        match &self.mic {
            _ if self.no_mic => Err("none"),
            None => Ok(None),
            Some(Ok(m)) => Ok(Some(m.name.as_str())),
            Some(Err(e)) => Err(e.as_str()),
        }
    }

    pub fn status(&self) -> Status {
        let now = Instant::now();
        let (muted, chan) = {
            let c = self.shared.cfg.lock().unwrap();
            (c.muted.clone(), c.channel.clone())
        };
        let mut peers: Vec<PeerView> = self
            .shared
            .peers
            .lock()
            .unwrap()
            .values()
            .map(|p| PeerView {
                name: p.name.clone(),
                addr: p.addr,
                channel: p.channel.clone(),
                same: p.channel.eq_ignore_ascii_case(&chan),
                talking: p.heard.is_some_and(|h| now.duration_since(h) < Duration::from_millis(250)),
                muted: muted.contains(&p.name.to_lowercase()),
            })
            .collect();
        peers.sort_by(|a, b| b.same.cmp(&a.same).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let unresolved = self.shared.manual.lock().unwrap().iter().filter(|m| m.1.is_none()).map(|m| m.0.clone()).collect();
        Status { peers, level_db: f32::from_bits(self.shared.level.load(Ordering::Relaxed)), talking: self.shared.talking.load(Ordering::Relaxed) && matches!(self.mic, Some(Ok(_))), unresolved }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        PTT_HELD.store(false, Ordering::Relaxed);
        let b = wire::encode(&Packet::Bye { session: self.shared.session });
        for p in self.shared.peers.lock().unwrap().values() {
            let _ = self.shared.sock.send_to(&b, p.addr);
        }
        log::info!("voice: off");
    }
}

/// Test runs' stand-ins: a microphone source (tone, tone:HZ or a WAV file) and a WAV to record what is heard.
struct Stand {
    mic: Option<std::ffi::OsString>,
    rec: Option<std::ffi::OsString>,
}

/// The voice-probe command: a peer with no window that talks, listens and says who it finds.
pub fn probe(name: &str, seconds: u64, mic: std::ffi::OsString, record: Option<std::ffi::OsString>, channel: String, peers: Vec<String>) -> anyhow::Result<()> {
    let cfg = Settings { enabled: true, channel, peers, ..Default::default() };
    let mut e = Engine::start_on(&cfg, name, Some("null"), PORT, Stand { mic: Some(mic), rec: record })?;
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut last = String::new();
    while Instant::now() < end {
        e.tick();
        if let Err(m) = e.mic() {
            anyhow::bail!("microphone: {m}");
        }
        let s = e.status();
        let line = s.peers.iter().map(|p| format!("{} {}{}{}", p.name, p.addr, if p.same { "" } else { " (other channel)" }, if p.talking { " talking" } else { "" })).collect::<Vec<_>>().join(", ");
        let line = format!("peers [{line}]  me {}", if s.talking { "talking" } else { "quiet" });
        if line != last {
            println!("{:>6.1}s  {line}", seconds as f32 - end.saturating_duration_since(Instant::now()).as_secs_f32());
            last = line;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// "host", "host:port" or "a.b.c.d[:port]" to its first LAN IPv4 address.
fn resolve(s: &str) -> Option<SocketAddrV4> {
    let s = s.trim();
    let with_port = if s.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) { s.to_string() } else { format!("{s}:{PORT}") };
    with_port.to_socket_addrs().ok()?.find_map(|a| match a {
        SocketAddr::V4(v) if wire::lan(*v.ip()) => Some(v),
        _ => None,
    })
}

/// The beacon port, shared by every client on this computer, in the group.
fn discovery_socket(port: u16) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())?;
    // Without a multicast route the broadcast and the typed addresses still reach it.
    if let Err(e) = s.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED) {
        log::warn!("voice: multicast group: {e}");
    }
    let u: UdpSocket = s.into();
    u.set_read_timeout(Some(Duration::from_millis(200)))?;
    Ok(u)
}

fn listen(shared: Arc<Shared>, sock: UdpSocket) {
    let mut buf = [0u8; 2048];
    while !shared.stop.load(Ordering::Relaxed) {
        if let Ok((n, SocketAddr::V4(from))) = sock.recv_from(&mut buf) {
            shared.handle(&buf[..n], from);
        }
    }
}

fn net(shared: Arc<Shared>) {
    let mut buf = [0u8; 2048];
    let mut next = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) {
        if Instant::now() >= next {
            shared.beacon();
            next = Instant::now() + BEACON;
        }
        if let Ok((n, SocketAddr::V4(from))) = shared.sock.recv_from(&mut buf) {
            shared.handle(&buf[..n], from);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(mut what: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if what() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Two engines on this computer find each other through a typed address (no multicast needed), hear each
    /// other's frames, and keep channels apart.
    #[test]
    fn two_engines_talk() {
        let port = 55400 + (std::process::id() % 500) as u16 * 2;
        let cfg = Settings { enabled: true, peers: vec![format!("127.0.0.1:{}", port + 1)], ..Default::default() };
        let none = || Stand { mic: None, rec: None };
        let a = Engine::start_on(&cfg, "Alpha", None, port, none()).unwrap();
        let b = Engine::start_on(&Settings { peers: Vec::new(), ..cfg.clone() }, "Beta", None, port + 1, none()).unwrap();
        let sees = |e: &Engine, name: &str| e.status().peers.iter().any(|p| p.name == name);
        assert!(wait(|| sees(&a, "Beta") && sees(&b, "Alpha")));
        let talking = |e: &Engine| e.status().peers.iter().any(|p| p.talking);
        for seq in 0..3 {
            a.shared.send_audio(seq, vec![1000; FRAME]);
        }
        assert!(wait(|| talking(&b)));
        // Heard once the three frames are in.
        assert!(wait(|| {
            let mut out = [0.0; FRAME];
            b.shared.play.lock().unwrap().mix(&mut out);
            (out[0] * 32768.0).round() == 1000.0
        }));
        // Another channel: listed, not heard either way.
        let other = Settings { channel: "other".into(), ..b.shared.cfg.lock().unwrap().clone() };
        b.apply(&other);
        assert!(wait(|| a.status().peers.iter().any(|p| p.name == "Beta" && !p.same)));
        std::thread::sleep(Duration::from_millis(300));
        a.shared.send_audio(3, vec![1000; FRAME]);
        b.shared.send_audio(0, vec![1000; FRAME]);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!talking(&a) && !talking(&b));
        drop(a);
        assert!(wait(|| b.status().peers.is_empty()));
    }

    /// The microphone (a stand-in tone) opens with the engine, closes when muted, reopens on another device, and
    /// what it sends is heard.
    #[test]
    fn mic_opens_unless_muted() {
        let port = 57000 + (std::process::id() % 500) as u16 * 2;
        let cfg = Settings { enabled: true, peers: vec![format!("127.0.0.1:{}", port + 1)], ..Default::default() };
        let mut a = Engine::start_on(&cfg, "Alpha", Some("null"), port, Stand { mic: Some("tone".into()), rec: None }).unwrap();
        a.linger = Duration::ZERO;
        a.tick();
        assert_eq!(a.mic(), Ok(Some("test tone 440 Hz")));
        let b = Engine::start_on(&Settings { peers: Vec::new(), ..cfg.clone() }, "Beta", None, port + 1, Stand { mic: None, rec: None }).unwrap();
        assert!(wait(|| {
            a.tick();
            b.status().peers.iter().any(|p| p.name == "Alpha" && p.talking)
        }));
        a.apply(&Settings { mic_muted: true, ..cfg.clone() });
        a.tick();
        assert_eq!(a.mic(), Ok(None));
        a.apply(&cfg);
        a.tick();
        assert!(a.mic().is_ok_and(|m| m.is_some()));
        // Another device: reopened at once, with no new engine.
        a.apply(&Settings { input: "other".into(), ..cfg.clone() });
        a.tick();
        assert!(a.mic().is_ok_and(|m| m.is_some()) && a.opened_with == "other");
    }

    /// A Bye or a frame counts only from the address the peer's hellos come from, port included.
    #[test]
    fn bye_and_audio_from_the_peer_only() {
        let port = 58000 + (std::process::id() % 500) as u16 * 2;
        let e = Engine::start_on(&Settings { enabled: true, ..Default::default() }, "Alpha", None, port, Stand { mic: None, rec: None }).unwrap();
        let s = &e.shared;
        let (peer, other): (SocketAddrV4, SocketAddrV4) = ("127.0.0.1:40000".parse().unwrap(), "127.0.0.1:40001".parse().unwrap());
        let channel = s.cfg.lock().unwrap().channel.clone();
        s.handle(&wire::encode(&Packet::Hello { session: 7, name: "Beta".into(), channel }), peer);
        let listed = || s.peers.lock().unwrap().contains_key(&7);
        let heard = || s.peers.lock().unwrap().get(&7).is_some_and(|p| p.heard.is_some());
        let frame = || wire::encode(&Packet::Audio { session: 7, seq: 0, pcm: vec![1000; FRAME] });
        assert!(listed());
        s.handle(&frame(), other);
        s.handle(&wire::encode(&Packet::Bye { session: 7 }), other);
        assert!(listed() && !heard());
        s.handle(&frame(), peer);
        assert!(heard());
        s.handle(&wire::encode(&Packet::Bye { session: 7 }), peer);
        assert!(!listed());
    }

    #[test]
    fn typed_addresses() {
        assert_eq!(resolve("127.0.0.1"), Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, PORT)));
        assert_eq!(resolve(" 192.168.1.20:6000 "), Some("192.168.1.20:6000".parse().unwrap()));
        assert_eq!(resolve("8.8.8.8"), None);
        assert_eq!(resolve("not a host name"), None);
    }
}
