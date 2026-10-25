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

/// The release asset for this platform.
pub fn asset_name() -> &'static str {
    if cfg!(windows) { "vana-client-windows-x86_64.exe" } else { "vana-client-linux-x86_64" }
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
}

pub enum Client {
    Checking,
    Current,
    Available(Release),
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
            Ok(Some(r)) if newer(&r.tag, env!("CARGO_PKG_VERSION")) => Client::Available(r),
            Ok(Some(_)) => Client::Current,
            Ok(None) => Client::Unavailable("no release for this platform".into()),
            Err(e) => Client::Unavailable(format!("{e:#}")),
        };
        c.lock().unwrap().client = client;
    });
    s
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().user_agent(concat!("vana-client/", env!("CARGO_PKG_VERSION"))).build().into()
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
    let asset = v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(asset_name())));
    Ok(asset.map(|a| Release { tag, url: a["browser_download_url"].as_str().unwrap_or_default().to_string(), size: a["size"].as_u64().unwrap_or(0) }))
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

/// "v0.2.0" newer than "0.1.0" (numeric parts, a leading v ignored).
pub fn newer(tag: &str, current: &str) -> bool {
    let parts = |s: &str| s.trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(tag) > parts(current)
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
}

impl UpdateUi {
    pub fn open(status: Shared, root: PathBuf) -> Self {
        let (g, c) = {
            let s = status.lock().unwrap();
            (s.game.offered(), matches!(s.client, Client::Available(_)))
        };
        Self { status, root, pick: [g, c], cursor: if g || c { 2 } else { 3 }, job: None, reloaded: false }
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
                    replace_self(&dest)?;
                    status.lock().unwrap().client = Client::Current;
                    lines.push(format!("vana-client {} is installed; restart to use it.", r.tag));
                    restart = true;
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
        let [x, y, _, h] = Self::rect(sw, sh);
        let inside = |r: [f32; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3];
        let busy = self.running();
        let done = self.job.is_some() && !busy;
        let targets: Vec<(usize, [f32; 4])> = if done {
            vec![(2, Self::button_rect(x, y, h, 0)), (3, Self::button_rect(x, y, h, 1))]
        } else {
            vec![(0, Self::row_rect(x, y, 0)), (1, Self::row_rect(x, y, 1)), (2, Self::button_rect(x, y, h, 0)), (3, Self::button_rect(x, y, h, 1))]
        };
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

    pub fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) -> [f32; 4] {
        let white = [0x80u8; 4];
        let grey = [0x58, 0x58, 0x58, 0x80];
        let [x, y, w, h] = Self::rect(sw, sh);
        gu.rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, [0.0, 0.0, 0.0, 0.55]);
        gu.frame(x, y, w, h);
        gu.cell_stretch("menu    windowps", 465, 1, x, y + 4.0, w, 16.0, 0x8080_8080);
        gu.text("Update", x + 8.0, y + 4.0, 0, white);
        let job = self.job.as_ref().map(|j| match &*j.lock().unwrap() {
            Job::Running(p) => Job::Running(p.clone()),
            Job::Done(l, r) => Job::Done(l.clone(), *r),
            Job::Failed(e) => Job::Failed(e.clone()),
        });
        let buttons = |gu: &mut GameUi, labels: [&str; 2], cursor: usize| {
            for (i, label) in labels.iter().enumerate() {
                let r = Self::button_rect(x, y, h, i);
                let on = cursor == 2 + i;
                super::settings::button(gu, r, if on { 0x8080_5020 } else { 0x8080_8080 });
                gu.text(label, r[0] + r[2] / 2.0, r[1], 1, white);
                if on {
                    gu.cursor(r[0], r[1]);
                }
            }
        };
        match job {
            Some(Job::Running(p)) => {
                gu.text(if p.label.is_empty() { "Starting..." } else { &p.label }, x + 20.0, y + 48.0, 0, white);
                // The config pages' slider as the gauge: track windowps 383, fill 384.
                let (gx, gy, gw) = (x + 20.0, y + 80.0, w - 40.0);
                gu.sprite_scaled("menu    windowps", 383, gx, gy, gw / 192.0, 1.0, 0x8080_8080);
                if p.total > 0 && p.done > 0 {
                    let f = (p.done as f32 / p.total as f32).clamp(0.0, 1.0);
                    gu.sprite_scaled("menu    windowps", 384, gx, gy, f * gw / 192.0, 1.0, 0xa0a0_a0a0);
                }
                gu.text(&p.detail, x + 20.0, y + 104.0, 0, grey);
                gu.text("Please wait; you can keep this window open.", x + 20.0, y + h - 32.0, 0, grey);
            }
            Some(Job::Done(lines, restart)) => {
                for (i, l) in lines.iter().enumerate() {
                    for (k, part) in gu.wrap(l, w - 40.0).iter().enumerate().take(2) {
                        gu.text(part, x + 20.0, y + 48.0 + 34.0 * i as f32 + 16.0 * k as f32, 0, white);
                    }
                }
                if lines.is_empty() {
                    gu.text("Nothing was updated.", x + 20.0, y + 48.0, 0, white);
                }
                buttons(gu, if restart { ["Restart", "Later"] } else { ["OK", ""] }, if restart { self.cursor } else { 2 });
            }
            Some(Job::Failed(e)) => {
                gu.text("The update did not finish:", x + 20.0, y + 48.0, 0, white);
                for (k, part) in gu.wrap(&e, w - 40.0).iter().enumerate().take(4) {
                    gu.text(part, x + 20.0, y + 68.0 + 16.0 * k as f32, 0, grey);
                }
                buttons(gu, ["OK", ""], 2);
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
                            Client::Unavailable(_) => format!("No newer release ({})", env!("CARGO_PKG_VERSION")),
                        },
                        matches!(s.client, Client::Available(_)),
                    ),
                ];
                drop(s);
                for (i, (name, detail, avail)) in rows.iter().enumerate() {
                    let r = Self::row_rect(x, y, i);
                    let on = self.cursor == i;
                    // The config pages' On / Off button as the checkbox.
                    let b = [r[0], r[1] + 6.0, 40.0, 16.0];
                    super::settings::button(gu, b, if on { 0x8080_5020 } else { 0x8080_8080 });
                    gu.text(if self.pick[i] && *avail { "On" } else { "Off" }, b[0] + 20.0, b[1], 1, if *avail { white } else { grey });
                    gu.text(name, r[0] + 52.0, r[1], 0, if *avail { white } else { grey });
                    gu.text(detail, r[0] + 52.0, r[1] + 14.0, 0, grey);
                    if on {
                        gu.cursor(b[0], b[1]);
                    }
                }
                buttons(gu, ["Update", "Close"], self.cursor);
            }
        }
        [x, y, w, h]
    }
}

#[cfg(test)]
mod tests {
    use super::{newer, thousands};

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert_eq!((thousands(5321), thousands(452), thousands(1_234_567)), ("5,321".into(), "452".into(), "1,234,567".into()));
    }
}
