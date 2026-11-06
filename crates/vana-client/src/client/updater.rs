//! Updates (not retail): the start-up checks, the "Update" button's window on the title screen and the work it
//! starts. Game files come from the PlayOnline patch server (crate::patch); vana-client itself from the latest
//! GitHub release. Nothing is ever required: the button shows only when something is out of date.

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where releases are published.
pub const RELEASE_REPO: &str = "lenonk/vana-client";

/// The release asset for this platform. A build for a platform no release carries (macOS, Linux on ARM so far) finds
/// none and is never offered another platform's binary.
pub fn asset_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "vana-client-windows-x86_64.exe",
        ("linux", "x86_64") => "vana-client-linux-x86_64",
        ("linux", "aarch64") => "vana-client-linux-aarch64",
        ("macos", "aarch64") => "vana-client-macos-aarch64",
        ("macos", "x86_64") => "vana-client-macos-x86_64",
        _ => "vana-client-unsupported-platform",
    }
}

pub enum Game {
    Checking,
    Current,
    /// Out of date (the server's version), the files still being checked.
    Behind(String),
    Available(crate::patch::Plan),
    Failed(String),
}

impl Game {
    /// Offered: out of date, sized or not.
    pub fn offered(&self) -> bool {
        matches!(self, Game::Behind(_) | Game::Available(_))
    }
}

#[derive(Clone)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub size: u64,
    /// "<asset>.sig" beside it: the release key's Ed25519 signature of [`signed_text`].
    pub sig_url: Option<String>,
}

pub enum Client {
    Checking,
    Current,
    Available(Release),
    /// A newer release with no signature: never offered.
    Unsigned(String),
    /// No release (or none for this platform) or the check failed; not offered.
    Unavailable(String),
}

pub struct Status {
    pub game: Game,
    pub client: Client,
}

impl Status {
    /// Something to offer.
    pub fn any(&self) -> bool {
        self.game.offered() || matches!(self.client, Client::Available(_))
    }
}

pub type Shared = Arc<Mutex<Status>>;

/// Starts both checks in the background.
pub fn start(root: PathBuf) -> Shared {
    let s: Shared = Arc::new(Mutex::new(Status { game: Game::Checking, client: Client::Checking }));
    let g = s.clone();
    std::thread::spawn(move || {
        // The button shows as soon as the version says so; the file check sizes it afterwards.
        let game = match crate::patch::behind(&root) {
            Ok(Some(server)) => {
                g.lock().unwrap().game = Game::Behind(server.version.clone());
                match crate::patch::plan_against(&root, server) {
                    Ok(plan) if !plan.todo.is_empty() => Game::Available(plan),
                    Ok(_) => Game::Current,
                    Err(e) => Game::Failed(format!("{e:#}")),
                }
            }
            Ok(None) => Game::Current,
            Err(e) => Game::Failed(format!("{e:#}")),
        };
        if let Game::Failed(e) = &game {
            log::warn!("update check: {e}");
        }
        log::info!("update check: game files {}", match &game {
            Game::Available(p) => format!("{} files to update ({})", p.todo.len(), p.server.version),
            Game::Current => "current".into(),
            Game::Failed(e) => format!("failed: {e}"),
            Game::Checking | Game::Behind(_) => String::new(),
        });
        g.lock().unwrap().game = game;
    });
    let c = s.clone();
    std::thread::spawn(move || {
        let client = match latest_release() {
            Ok(Some(r)) if newer(&r.tag, env!("CARGO_PKG_VERSION")) && r.sig_url.is_none() => Client::Unsigned(r.tag),
            Ok(Some(r)) if newer(&r.tag, env!("CARGO_PKG_VERSION")) => Client::Available(r),
            Ok(Some(_)) => Client::Current,
            Ok(None) => Client::Unavailable("no release for this platform".into()),
            Err(e) => Client::Unavailable(format!("{e:#}")),
        };
        match &client {
            Client::Unsigned(tag) => log::warn!("update check: release {tag} has no {}.sig; not offered", asset_name()),
            Client::Unavailable(why) => log::warn!("update check: no client release: {why}"),
            _ => {}
        }
        c.lock().unwrap().client = client;
    });
    s
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().user_agent(concat!("vana-client/", env!("CARGO_PKG_VERSION"))).build().into()
}

/// A GitHub API answer's text.
pub(super) fn get_text(url: &str) -> Result<String> {
    Ok(agent().get(url).header("Accept", "application/vnd.github+json").call().with_context(|| url.to_string())?.body_mut().read_to_string()?)
}

/// The latest GitHub release and this platform's asset in it (None: no release, or no such asset).
fn latest_release() -> Result<Option<Release>> {
    // VANA_RELEASE_API: another endpoint for the test drives (a local mock of the GitHub answer).
    let url = std::env::var("VANA_RELEASE_API").unwrap_or_else(|_| format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest"));
    let mut resp = match agent().get(&url).header("Accept", "application/vnd.github+json").call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&resp.body_mut().read_to_string()?)?;
    let tag = v["tag_name"].as_str().unwrap_or_default().to_string();
    let find = |name: &str| v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(name)));
    let url = |a: &serde_json::Value| a["browser_download_url"].as_str().unwrap_or_default().to_string();
    let sig_url = find(&format!("{}.sig", asset_name())).map(url);
    Ok(find(asset_name()).map(|a| Release { tag, url: url(a), size: a["size"].as_u64().unwrap_or(0), sig_url }))
}

/// The release signing key's public half (release/signing-key.pub; release/publish.sh signs with the private one).
const SIGNING_KEY: &str = include_str!("../../../../release/signing-key.pub");

/// What a release's signature covers: its tag, the asset's name and the asset's SHA-256, so a signed binary can't be
/// served under another tag or for the other platform.
pub fn signed_text(tag: &str, asset: &str, file: &[u8]) -> String {
    format!("vana-client release\n{tag}\n{asset}\n{}\n", hex::encode(ring::digest::digest(&ring::digest::SHA256, file)))
}

/// `sig` is the key's Ed25519 signature of `text`.
fn signed_by(key: &str, text: &str, sig: &[u8]) -> bool {
    let Some(key) = hex::decode(key.trim()).ok().filter(|k| k.len() == 32) else { return false };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(text.as_bytes(), sig).is_ok()
}

/// A downloaded binary checked against its release's signature before it is installed; one that fails is removed.
fn verify(r: &Release, file: &Path) -> Result<()> {
    let url = r.sig_url.as_ref().with_context(|| format!("release {} is not signed", r.tag))?;
    let sig = agent().get(url).call().with_context(|| url.clone())?.body_mut().with_config().limit(1024).read_to_vec()?;
    if !signed_by(SIGNING_KEY, &signed_text(&r.tag, asset_name(), &std::fs::read(file)?), &sig) {
        let _ = std::fs::remove_file(file);
        bail!("the downloaded vana-client {} is not signed by the release key; not installed", r.tag);
    }
    Ok(())
}

/// 5321 as "5,321".
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "v0.2.0" newer than "0.1.0": numeric parts (a leading v and any +build ignored, missing parts count as 0), then
/// semver's pre-release rule: "1.2.0-rc1" comes before "1.2.0", and pre-releases compare part by part, numbers
/// numerically and below words.
pub fn newer(tag: &str, current: &str) -> bool {
    use std::cmp::Ordering;
    fn split(s: &str) -> (Vec<u64>, Option<Vec<&str>>) {
        let s = s.trim_start_matches('v');
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = s.split_once('-').map_or((s, None), |(c, p)| (c, Some(p.split('.').collect())));
        (core.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect(), pre)
    }
    fn ident(a: &str, b: &str) -> Ordering {
        match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        }
    }
    let ((mut a, pa), (mut b, pb)) = (split(tag), split(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    let order = a.cmp(&b).then_with(|| match (&pa, &pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.iter().zip(y).map(|(p, q)| ident(p, q)).find(|o| o.is_ne()).unwrap_or(x.len().cmp(&y.len())),
    });
    order == Ordering::Greater
}

/// Downloads `url` to `dest` (through `dest.part`, resumed when one is there), reporting (bytes, total).
pub fn download(url: &str, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let have = std::fs::metadata(&part).map_or(0, |m| m.len());
    let mut req = agent().get(url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().with_context(|| url.to_string())?;
    let resumed = resp.status() == 206;
    let len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let start = if resumed { have } else { 0 };
    let total = start + len;
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part)?;
    let mut reader = resp.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = start;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.flush()?;
    drop(file);
    if total > 0 && done != total {
        bail!("{url}: got {done} of {total} bytes");
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// The executable as it was at start: once replace_self renames the new binary over it, Linux names the running one
/// "<path> (deleted)", and a restart from that path fails.
pub fn exe() -> Option<PathBuf> {
    static EXE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    EXE.get_or_init(|| std::env::current_exe().ok()).clone()
}

/// Puts a downloaded binary in place of the running one: Windows cannot overwrite a running exe, so the old one is
/// renamed aside first (and removed at the next start).
fn replace_self(new: &Path) -> Result<()> {
    let exe = exe().context("no executable path")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
    }
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    if cfg!(windows) {
        std::fs::rename(&exe, &old)?;
    }
    std::fs::rename(new, &exe).or_else(|_| std::fs::copy(new, &exe).map(|_| ()))?;
    Ok(())
}

/// Removes what replace_self left behind (at start, which also takes the executable's path).
pub fn tidy() {
    if let Some(exe) = exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

#[derive(Clone, Default)]
pub struct Progress {
    pub label: String,
    pub done: u64,
    pub total: u64,
    pub detail: String,
}

pub enum Job {
    Running(Progress),
    /// Lines to show; whether a restart finishes it.
    Done(Vec<String>, bool),
    Failed(String),
}

/// What the window does next for the app.
#[derive(Default)]
pub struct Out {
    pub closed: bool,
    /// Game files changed: reopen the DAT index.
    pub reload: bool,
    pub restart: bool,
    pub sounds: Vec<Se>,
    /// The pointer is over a row or button (the hit cursor).
    pub hover: bool,
}

const W: f32 = 440.0;

pub struct UpdateUi {
    status: Shared,
    root: PathBuf,
    /// Checkboxes: game files, vana-client.
    pick: [bool; 2],
    /// 0, 1 the rows; 2 Update; 3 Close.
    cursor: usize,
    job: Option<Arc<Mutex<Job>>>,
    reloaded: bool,
    /// The view's scale about the screen's centre: 1 classic, larger for the modern UI's text.
    pub zoom: f32,
}

impl UpdateUi {
    pub fn open(status: Shared, root: PathBuf) -> Self {
        let (g, c) = {
            let s = status.lock().unwrap();
            (s.game.offered(), matches!(s.client, Client::Available(_)))
        };
        Self { status, root, pick: [g, c], cursor: if g || c { 2 } else { 3 }, job: None, reloaded: false, zoom: 1.0 }
    }

    fn available(&self, row: usize) -> bool {
        let s = self.status.lock().unwrap();
        if row == 0 { s.game.offered() } else { matches!(s.client, Client::Available(_)) }
    }

    fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| matches!(*j.lock().unwrap(), Job::Running(_)))
    }

    fn start(&mut self) {
        let s = self.status.lock().unwrap();
        let game = self.pick[0] && s.game.offered();
        let release = match (&s.client, self.pick[1]) {
            (Client::Available(r), true) => Some(r.clone()),
            _ => None,
        };
        drop(s);
        let job = Arc::new(Mutex::new(Job::Running(Progress::default())));
        self.job = Some(job.clone());
        let (root, status) = (self.root.clone(), self.status.clone());
        std::thread::spawn(move || {
            let set = |p: Progress| *job.lock().unwrap() = Job::Running(p);
            let mut lines = Vec::new();
            let mut restart = false;
            let result = (|| -> Result<()> {
                // Asked for before the file check finished: wait for it here.
                let plan = if game {
                    loop {
                        let mut s = status.lock().unwrap();
                        match std::mem::replace(&mut s.game, Game::Checking) {
                            Game::Available(p) => break Some(p),
                            Game::Behind(v) => {
                                s.game = Game::Behind(v);
                                drop(s);
                                set(Progress { label: "Checking game files".into(), done: 0, total: 0, detail: String::new() });
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                            Game::Failed(e) => {
                                s.game = Game::Failed(e.clone());
                                bail!("checking the game files: {e}");
                            }
                            other => {
                                s.game = other;
                                break None;
                            }
                        }
                    }
                } else {
                    None
                };
                if let Some(plan) = plan {
                    let total_files = plan.todo.len();
                    let total_bytes: u64 = plan.todo.iter().map(|t| t.2).sum();
                    let version = plan.server.version.clone();
                    crate::patch::apply(&root, &plan, 4, &|f, b| {
                        set(Progress { label: "Updating game files".into(), done: b, total: total_bytes, detail: format!("{} / {} files, {:.1} of {:.1} MB", thousands(f as u64), thousands(total_files as u64), b as f64 / 1e6, total_bytes as f64 / 1e6) });
                    })?;
                    status.lock().unwrap().game = Game::Current;
                    lines.push(format!("Game files updated to {version} ({total_files} files)."));
                }
                if let Some(r) = release {
                    let dest = exe().context("no executable path")?.with_extension("new");
                    download(&r.url, &dest, &|d, t| set(Progress { label: format!("Downloading vana-client {}", r.tag), done: d, total: t.max(r.size), detail: format!("{:.1} of {:.1} MB", d as f64 / 1e6, t.max(r.size) as f64 / 1e6) }))?;
                    verify(&r, &dest)?;
                    replace_self(&dest)?;
                    status.lock().unwrap().client = Client::Current;
                    lines.push(format!("vana-client {} is installed; restart to use it.", r.tag));
                    restart = true;
                    // The notes since this version, for the restarted client to show (none if GitHub won't say).
                    let from = env!("CARGO_PKG_VERSION");
                    match super::notes::fetch(from, &r.tag) {
                        Ok(notes) if !notes.is_empty() => {
                            if let Err(e) = super::notes::save(&super::notes::Saved { from: from.into(), to: r.tag.clone(), notes }) {
                                log::warn!("update: can't keep the release notes: {e:#}");
                            }
                        }
                        Ok(_) => log::info!("update: no release notes after {from}"),
                        Err(e) => log::warn!("update: release notes: {e:#}"),
                    }
                }
                Ok(())
            })();
            *job.lock().unwrap() = match result {
                Ok(()) => Job::Done(lines, restart),
                Err(e) => Job::Failed(format!("{e:#}")),
            };
        });
    }

    /// A key; Out says what the app should do.
    pub fn key(&mut self, key: egui::Key) -> Out {
        let mut out = Out::default();
        let finished = self.job.as_ref().map(|j| match &*j.lock().unwrap() {
            Job::Running(_) => None,
            Job::Done(_, r) => Some(*r),
            Job::Failed(_) => Some(false),
        });
        match finished {
            // While it runs the window takes no keys.
            Some(None) => return out,
            Some(Some(restart)) => {
                match key {
                    egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Tab if restart => {
                        self.cursor = if self.cursor == 2 { 3 } else { 2 };
                        out.sounds.push(Se::Cursor);
                    }
                    egui::Key::Enter if restart && self.cursor == 2 => out.restart = true,
                    egui::Key::Enter | egui::Key::Escape => {
                        out.closed = true;
                        out.sounds.push(Se::Cancel);
                    }
                    _ => {}
                }
                return out;
            }
            None => {}
        }
        match key {
            egui::Key::ArrowDown | egui::Key::Tab => {
                self.cursor = (self.cursor + 1) % 4;
                out.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowUp => {
                self.cursor = (self.cursor + 3) % 4;
                out.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight if self.cursor >= 2 => {
                self.cursor = if self.cursor == 2 { 3 } else { 2 };
                out.sounds.push(Se::Cursor);
            }
            egui::Key::Enter if self.cursor < 2 => {
                if self.available(self.cursor) {
                    self.pick[self.cursor] ^= true;
                    out.sounds.push(Se::Select);
                } else {
                    out.sounds.push(Se::Error);
                }
            }
            egui::Key::Enter if self.cursor == 2 => {
                if self.pick.iter().zip(0..).any(|(&p, i)| p && self.available(i)) {
                    self.start();
                    out.sounds.push(Se::Select);
                } else {
                    out.sounds.push(Se::Error);
                }
            }
            egui::Key::Enter | egui::Key::Escape => {
                out.closed = true;
                out.sounds.push(Se::Cancel);
            }
            _ => {}
        }
        out
    }

    /// Called every frame: reports a finished game update once (the app reopens its DAT index).
    pub fn poll(&mut self) -> Out {
        let mut out = Out::default();
        if !self.reloaded
            && let Some(Job::Done(lines, _)) = self.job.as_ref().map(|j| j.lock().unwrap()).as_deref()
        {
            self.reloaded = true;
            out.reload = lines.iter().any(|l| l.starts_with("Game files"));
            self.cursor = 2;
        }
        out
    }

    fn rect(sw: f32, sh: f32) -> [f32; 4] {
        let h = 164.0;
        [((sw - W) / 2.0).round(), ((sh - h) / 2.0).round(), W, h]
    }

    fn row_rect(x: f32, y: f32, i: usize) -> [f32; 4] {
        [x + 16.0, y + 40.0 + 34.0 * i as f32, W - 32.0, 30.0]
    }

    fn button_rect(x: f32, y: f32, h: f32, i: usize) -> [f32; 4] {
        [x + W / 2.0 - 112.0 + 120.0 * i as f32, y + h - 32.0, 104.0, 18.0]
    }

    /// The pointer: rows and buttons follow it; a click acts as Enter on them.
    pub fn pointer(&mut self, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32) -> Out {
        let inside = |r: [f32; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3];
        let busy = self.running();
        // The rows (whole width) and the buttons the view shows.
        let v = self.view(sw, sh);
        let targets: Vec<(usize, [f32; 4])> = v.rows.iter().copied().enumerate().chain(v.buttons.iter().enumerate().map(|(i, b)| (2 + i, b.0))).collect();
        for (i, r) in targets {
            if inside(r) && !busy {
                if moved || clicked {
                    self.cursor = i;
                }
                if clicked {
                    return self.key(egui::Key::Enter);
                }
                return Out { hover: true, ..Out::default() };
            }
        }
        Out::default()
    }

    pub fn help(&self) -> String {
        match self.cursor {
            0 => "The game's data files, from the PlayOnline patch server.".into(),
            1 => "vana-client itself, from its latest release.".into(),
            2 => "Download and install what is checked.".into(),
            _ => "Close this window.".into(),
        }
    }

    /// The window as both UIs draw it (UI pixels); the mouse stays on these rects (`pointer`).
    pub fn view(&self, sw: f32, sh: f32) -> View {
        let [x, y, w, h] = Self::rect(sw, sh);
        let mut v = View { frame: [x, y, w, h], line: 16.0, texts: Vec::new(), rows: Vec::new(), toggles: Vec::new(), gauge: None, buttons: Vec::new() };
        let text = |v: &mut View, at: [f32; 2], s: &str, white: bool, max: usize| v.texts.push(Text { at, text: s.to_string(), white, wrap: w - 40.0, max });
        let buttons = |v: &mut View, labels: &[&'static str], cursor: usize| {
            for (i, &label) in labels.iter().enumerate() {
                v.buttons.push((Self::button_rect(x, y, h, i), label, cursor == 2 + i));
            }
        };
        let job = self.job.as_ref().map(|j| match &*j.lock().unwrap() {
            Job::Running(p) => Job::Running(p.clone()),
            Job::Done(l, r) => Job::Done(l.clone(), *r),
            Job::Failed(e) => Job::Failed(e.clone()),
        });
        match job {
            Some(Job::Running(p)) => {
                text(&mut v, [x + 20.0, y + 48.0], if p.label.is_empty() { "Starting..." } else { &p.label }, true, 1);
                let f = if p.total > 0 { (p.done as f32 / p.total as f32).clamp(0.0, 1.0) } else { 0.0 };
                v.gauge = Some(([x + 20.0, y + 80.0, w - 40.0, 16.0], f));
                text(&mut v, [x + 20.0, y + 104.0], &p.detail, false, 1);
                text(&mut v, [x + 20.0, y + h - 32.0], "Please wait; you can keep this window open.", false, 1);
            }
            Some(Job::Done(lines, restart)) => {
                for (i, l) in lines.iter().enumerate() {
                    text(&mut v, [x + 20.0, y + 48.0 + 34.0 * i as f32], l, true, 2);
                }
                if lines.is_empty() {
                    text(&mut v, [x + 20.0, y + 48.0], "Nothing was updated.", true, 1);
                }
                if restart { buttons(&mut v, &["Restart", "Later"], self.cursor) } else { buttons(&mut v, &["OK"], 2) }
            }
            Some(Job::Failed(e)) => {
                text(&mut v, [x + 20.0, y + 48.0], "The update did not finish:", true, 1);
                text(&mut v, [x + 20.0, y + 68.0], &e, false, 4);
                buttons(&mut v, &["OK"], 2);
            }
            None => {
                let s = self.status.lock().unwrap();
                let rows: [(&str, String, bool); 2] = [
                    (
                        "Game files",
                        match &s.game {
                            Game::Available(p) => format!("{} files, {} MB ({})", thousands(p.todo.len() as u64), thousands((p.todo.iter().map(|t| t.2).sum::<u64>() as f64 / 1e6).round() as u64), p.server.version),
                            Game::Behind(v) => format!("Out of date ({v}), checking files..."),
                            Game::Checking => "Checking...".into(),
                            Game::Current => "Up to date".into(),
                            Game::Failed(_) => "Could not check".into(),
                        },
                        s.game.offered(),
                    ),
                    (
                        "vana-client",
                        match &s.client {
                            Client::Available(r) => format!("{} to {} ({:.1} MB)", env!("CARGO_PKG_VERSION"), r.tag, r.size as f64 / 1e6),
                            Client::Checking => "Checking...".into(),
                            Client::Current => format!("Up to date ({})", env!("CARGO_PKG_VERSION")),
                            Client::Unsigned(tag) => format!("{tag} is not signed; not offered"),
                            Client::Unavailable(_) => format!("No newer release ({})", env!("CARGO_PKG_VERSION")),
                        },
                        matches!(s.client, Client::Available(_)),
                    ),
                ];
                drop(s);
                for (i, (name, detail, avail)) in rows.iter().enumerate() {
                    let r = Self::row_rect(x, y, i);
                    v.rows.push(r);
                    // The config pages' On / Off button as the checkbox.
                    v.toggles.push(([r[0], r[1] + 6.0, 40.0, 16.0], if self.pick[i] && *avail { "On" } else { "Off" }, *avail, self.cursor == i));
                    text(&mut v, [r[0] + 52.0, r[1]], name, *avail, 1);
                    text(&mut v, [r[0] + 52.0, r[1] + 14.0], detail, false, 1);
                }
                buttons(&mut v, &["Update", "Close"], self.cursor);
            }
        }
        v.zoomed(self.zoom, sw / 2.0, sh / 2.0)
    }

    /// The classic window: the game's frame, title strip, the config pages' buttons and slider.
    pub fn draw(gu: &mut GameUi, v: &View) {
        let white = [0x80u8; 4];
        let grey = [0x58, 0x58, 0x58, 0x80];
        let [x, y, w, h] = v.frame;
        gu.rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, [0.0, 0.0, 0.0, 0.55]);
        gu.frame(x, y, w, h);
        gu.cell_stretch("menu    windowps", 465, 1, x, y + 4.0, w, 16.0, 0x8080_8080);
        gu.text("Update", x + 8.0, y + 4.0, 0, white);
        for t in &v.texts {
            for (k, part) in gu.wrap(&t.text, t.wrap).iter().enumerate().take(t.max) {
                gu.text(part, t.at[0], t.at[1] + 16.0 * k as f32, 0, if t.white { white } else { grey });
            }
        }
        for &(b, label, avail, on) in &v.toggles {
            super::settings::button(gu, b, if on { 0x8080_5020 } else { 0x8080_8080 });
            gu.text(label, b[0] + 20.0, b[1], 1, if avail { white } else { grey });
            if on {
                gu.cursor(b[0], b[1]);
            }
        }
        // The config pages' slider as the gauge: track windowps 383, fill 384.
        if let Some(([gx, gy, gw, _], f)) = v.gauge {
            gu.sprite_scaled("menu    windowps", 383, gx, gy, gw / 192.0, 1.0, 0x8080_8080);
            if f > 0.0 {
                gu.sprite_scaled("menu    windowps", 384, gx, gy, f * gw / 192.0, 1.0, 0xa0a0_a0a0);
            }
        }
        for &(r, label, on) in &v.buttons {
            super::settings::button(gu, r, if on { 0x8080_5020 } else { 0x8080_8080 });
            gu.text(label, r[0] + r[2] / 2.0, r[1], 1, white);
            if on {
                gu.cursor(r[0], r[1]);
            }
        }
    }
}

/// A line of the window: where (top left), its text, white or grey, wrapped at `wrap` to `max` lines.
pub struct Text {
    pub at: [f32; 2],
    pub text: String,
    pub white: bool,
    pub wrap: f32,
    pub max: usize,
}

/// The update window for either UI (UI pixels): the frame, the line height, its lines, the rows the mouse takes, the
/// On / Off boxes (rect, label, available, cursor), the gauge (rect, fraction done), the buttons (rect, label, cursor).
pub struct View {
    pub frame: [f32; 4],
    pub line: f32,
    pub texts: Vec<Text>,
    pub rows: Vec<[f32; 4]>,
    pub toggles: Vec<([f32; 4], &'static str, bool, bool)>,
    pub gauge: Option<([f32; 4], f32)>,
    pub buttons: Vec<([f32; 4], &'static str, bool)>,
}

impl View {
    /// Every place and size scaled by `z` about (cx, cy).
    fn zoomed(mut self, z: f32, cx: f32, cy: f32) -> View {
        let r = |r: &mut [f32; 4]| *r = [cx + (r[0] - cx) * z, cy + (r[1] - cy) * z, r[2] * z, r[3] * z];
        r(&mut self.frame);
        self.line *= z;
        for t in &mut self.texts {
            t.at = [cx + (t.at[0] - cx) * z, cy + (t.at[1] - cy) * z];
            t.wrap *= z;
        }
        self.rows.iter_mut().for_each(r);
        self.toggles.iter_mut().for_each(|t| r(&mut t.0));
        if let Some(g) = self.gauge.as_mut() {
            r(&mut g.0);
        }
        self.buttons.iter_mut().for_each(|b| r(&mut b.0));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{SIGNING_KEY, asset_name, newer, signed_by, signed_text, thousands};

    /// Each platform asks for its own asset; only the published ones carry the release names.
    #[test]
    fn asset_for_this_platform() {
        let want = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            "vana-client-linux-x86_64"
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            "vana-client-windows-x86_64.exe"
        } else {
            return assert!(!["vana-client-linux-x86_64", "vana-client-windows-x86_64.exe"].contains(&asset_name()));
        };
        assert_eq!(asset_name(), want);
    }

    /// A signature covers the tag, the asset's name and its bytes; openssl's Ed25519 (pkeyutl -sign -rawin) checks.
    #[test]
    fn release_signatures() {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        // release/publish.sh's text and openssl's signature by the release key check against the built-in half.
        let sig = hex::decode("6db22e308db928bf583baf80d49e91a6573ce88302c67964811dd7f9e0bf2bfa8b00d47f8889e6789d505fe2530cbba9402206e45e29cbd91bf8ba7a3015da01").unwrap();
        assert!(signed_by(SIGNING_KEY, &signed_text("v0.0.0-test", "vana-client-linux-x86_64", b"binary"), &sig));
        let rng = ring::rand::SystemRandom::new();
        let pair = |rng| Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(rng).unwrap().as_ref()).unwrap();
        let (ours, theirs) = (pair(&rng), pair(&rng));
        let key = hex::encode(ours.public_key());
        let text = signed_text("v1.0.3", "vana-client-linux-x86_64", b"binary");
        let sig = ours.sign(text.as_bytes());
        assert!(signed_by(&key, &text, sig.as_ref()));
        // Another key, another tag (a downgrade), the other platform's name, other bytes, a cut signature: refused.
        assert!(!signed_by(&key, &text, theirs.sign(text.as_bytes()).as_ref()));
        for other in [signed_text("v1.0.4", "vana-client-linux-x86_64", b"binary"), signed_text("v1.0.3", "vana-client-windows-x86_64.exe", b"binary"), signed_text("v1.0.3", "vana-client-linux-x86_64", b"binarY")] {
            assert!(!signed_by(&key, &other, sig.as_ref()));
        }
        assert!(!signed_by(&key, &text, &sig.as_ref()[..63]));
        assert!(!signed_by("", &text, sig.as_ref()));
        // RFC 8032 section 7.1, test 3 (message af82): the Ed25519 openssl makes.
        let sig = hex::decode("6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a").unwrap();
        let key = "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";
        assert!(ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, hex::decode(key).unwrap()).verify(&hex::decode("af82").unwrap(), &sig).is_ok());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        // A pre-release comes before its release.
        assert!(!newer("1.2.0-rc1", "1.2.0"));
        assert!(newer("1.2.0", "1.2.0-rc1"));
        assert!(newer("1.2.1-rc1", "1.2.0"));
        assert!(newer("1.2.0-rc.2", "1.2.0-rc.1"));
        assert!(newer("1.2.0-rc.10", "1.2.0-rc.9"));
        assert!(newer("1.2.0-beta", "1.2.0-alpha"));
        assert!(newer("1.2.0-alpha.1", "1.2.0-alpha"));
        assert!(!newer("v1.2", "1.2.0"));
        assert!(!newer("1.2.0+build5", "1.2.0"));
        assert_eq!((thousands(5321), thousands(452), thousands(1_234_567)), ("5,321".into(), "452".into(), "1,234,567".into()));
    }
}
