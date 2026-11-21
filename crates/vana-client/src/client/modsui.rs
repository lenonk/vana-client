//! Settings' DATs and Mods pages (not retail; the mod manager, XIPivot's job built in). DATs: the installed game-file
//! packs in priority order, each ON / OFF and movable (Move, then Up / Down, Enter to place, Escape to put it back, as
//! the item list's Manual sort), and what is available: the archives found and the catalogue's packs, with Install (or
//! Update). Mods: the installed Lua mods, each ON / OFF (and Update when the catalogue has a newer one; Update All
//! queues them all), the Lua mod archives found, and the standard mods from GitHub (client/catalogue.rs) each with
//! Install, Update or Installed. Archives and the catalogue are read, downloaded and
//! unpacked off the UI thread (crate::modpack). The modern pages also install an archive or a Lua mod's folder picked
//! by hand (`Browse`) and remove a folder.

use super::catalogue::{self, Entry};
use crate::config::ModRef;
use crate::modpack::{self, Pack};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};

pub struct Mods {
    pub packs: Vec<Pack>,
    scan: Option<Receiver<Vec<Pack>>>,
    /// The catalogues' entries (DAT packs, then the standard mods), each to install (Some(false)), to update
    /// (Some(true)) or installed as it is (None).
    pub entries: Vec<(Entry, Option<bool>)>,
    list: Option<Receiver<anyhow::Result<Vec<(Entry, Option<bool>)>>>>,
    dat_list: Option<Receiver<anyhow::Result<Vec<(Entry, Option<bool>)>>>>,
    /// Why the standard mods' list, or the DAT packs', could not be read.
    pub list_error: Option<String>,
    pub dat_error: Option<String>,
    /// What is being installed, its progress (per mille) and the result (the folder's name).
    installing: Option<(String, Arc<AtomicU32>, Receiver<anyhow::Result<String>>)>,
    /// Catalogue updates waiting for the one installing (Update All), by id.
    queue: Vec<String>,
    /// Mods updated from the catalogue since the caller last took them (a running Lua mod restarts on its new files).
    pub updated: Vec<String>,
    /// A mod being moved: its order before the move.
    pub held: Option<Vec<ModRef>>,
    /// The last install's failure, for the help bar.
    pub error: Option<String>,
    /// The first row shown.
    pub top: usize,
    /// The modern page's file picker, while open.
    pub browse: Option<Browse>,
}

/// Rows shown at once (a page of 296 px from y 36, 34 px apart).
pub const VISIBLE: usize = 7;

/// A row below the installed mods: an archive found, or a catalogue mod.
pub struct Offer {
    /// Its name (with the per cent done while it installs).
    pub label: String,
    /// Install or Update.
    pub button: &'static str,
    pub help: String,
    pub catalogue: bool,
    /// Update All: every update of the page's kind (the last row).
    pub all: bool,
    /// Its place among the archives, then the catalogue's entries.
    at: usize,
}

/// Offer.at of the Update All row.
const ALL: usize = usize::MAX;

/// The catalogue's entries, each with what it is here: to install, to update or installed.
fn statuses(list: Vec<Entry>) -> Vec<(Entry, Option<bool>)> {
    let root = crate::config::mods_dir();
    list.into_iter()
        .map(|e| {
            let have = catalogue::installed(&root, &e.id);
            let st = catalogue::offer(&e, have.as_deref());
            (e, st)
        })
        .collect()
}

/// "1.2 GB", "340 KB".
fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n => format!("{} KB", n.div_ceil(1024)),
    }
}

impl Mods {
    /// Starts looking for packs and reading the catalogue.
    pub fn open() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(modpack::scan(&modpack::pack_dirs(), &crate::config::mods_dir()));
        });
        let (ltx, lrx) = channel();
        std::thread::spawn(move || {
            let _ = ltx.send(catalogue::fetch(catalogue::List::Mods).map(statuses));
        });
        let (dtx, drx) = channel();
        std::thread::spawn(move || {
            let _ = dtx.send(catalogue::fetch(catalogue::List::Dats).map(statuses));
        });
        Self {
            packs: Vec::new(),
            scan: Some(rx),
            entries: Vec::new(),
            list: Some(lrx),
            dat_list: Some(drx),
            list_error: None,
            dat_error: None,
            installing: None,
            queue: Vec::new(),
            updated: Vec::new(),
            held: None,
            error: None,
            top: 0,
            browse: None,
        }
    }

    /// No archives and no scan (tests, offscreen shots).
    pub fn empty() -> Self {
        Self {
            packs: Vec::new(),
            scan: None,
            entries: Vec::new(),
            list: None,
            dat_list: None,
            list_error: None,
            dat_error: None,
            installing: None,
            queue: Vec::new(),
            updated: Vec::new(),
            held: None,
            error: None,
            top: 0,
            browse: None,
        }
    }

    /// Waits for the scan and the catalogue (offscreen shots).
    pub fn wait_scan(&mut self) {
        if let Some(p) = self.scan.take().and_then(|r| r.recv().ok()) {
            self.packs = p;
        }
        if let Some(l) = self.list.take().and_then(|r| r.recv().ok()) {
            self.take_list(l, false);
        }
        if let Some(l) = self.dat_list.take().and_then(|r| r.recv().ok()) {
            self.take_list(l, true);
        }
    }

    /// A list come in: its entries kept (the DAT packs' first), or why it could not be read.
    fn take_list(&mut self, l: anyhow::Result<Vec<(Entry, Option<bool>)>>, dats: bool) {
        match l {
            Ok(e) if dats => {
                let rest = std::mem::take(&mut self.entries);
                self.entries = e.into_iter().chain(rest).collect();
            }
            Ok(e) => self.entries.extend(e),
            Err(e) => {
                log::warn!("{} catalogue: {e:#}", if dats { "DAT" } else { "mod" });
                *(if dats { &mut self.dat_error } else { &mut self.list_error }) = Some(format!("{e:#}"));
            }
        }
    }

    /// The archives found of Lua mods (`lua`) or DAT packs, then (DAT packs) the catalogue's packs not installed as they
    /// are; the standard Lua mods are listed apart (`standard`).
    pub fn offers(&self, lua: bool) -> Vec<Offer> {
        let pct =
            |slug: &str, name: String| self.progress(slug).map_or(name.clone(), |pm| format!("{}%  {name}", pm / 10));
        let archives = self.packs.iter().enumerate().filter(|p| p.1.lua.is_some() == lua).map(|(at, p)| {
            let name = p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let help = match &p.lua {
                Some(l) => format!("Lua mod {} {} in {name}: Install copies it into the mods folder, on. It starts at the next login.", p.slug, l.version),
                None => format!(
                    "{} game file{} in {name}: Install unpacks them into the mods folder, on and first. Takes effect on restart.",
                    p.files,
                    if p.files == 1 { "" } else { "s" }
                ),
            };
            Offer { label: pct(&p.slug, name), button: "Install", help, catalogue: false, all: false, at }
        });
        let n = self.packs.len();
        let listed = self
            .entries
            .iter()
            .enumerate()
            .filter(|e| !lua && e.1.0.kind != "lua")
            .filter_map(|(k, (e, st))| st.map(|u| (k, e, u)))
            .map(|(k, e, update)| {
                let by = if e.authors.is_empty() { String::new() } else { format!(" By {}.", e.authors.join(", ")) };
                let size = if e.size() > 0 { format!(" {}.", bytes(e.size())) } else { String::new() };
                let what = if e.kind == "lua" { "It starts at the next login." } else { "Takes effect on restart." };
                let verb = if update { "Update" } else { "Install" };
                let help = format!(
                    "{}{by}{size} {verb} downloads version {} from GitHub into the mods folder. {what}",
                    e.description.trim(),
                    e.version
                );
                Offer {
                    label: pct(&e.id, format!("{} {}", e.name, e.version)),
                    button: verb,
                    help: help.trim().to_string(),
                    catalogue: true,
                    all: false,
                    at: n + k,
                }
            });
        // Update All: on the Lua page (its updates are on the installed rows), and with two or more DAT packs.
        let ups = self.updates(lua);
        let all = (ups > usize::from(!lua)).then(|| {
            let what = if lua { "Lua mod" } else { "DAT pack" };
            Offer {
                label: format!("Update all ({ups})"),
                button: "Update All",
                help: format!(
                    "Downloads the newer version of every {what} with one, one after another. {}",
                    if lua { "A mod that is on restarts on it." } else { "Takes effect on restart." }
                ),
                catalogue: true,
                all: true,
                at: ALL,
            }
        });
        archives.chain(listed).chain(all).collect()
    }

    /// The standard Lua mods by name: each entry's place in `entries`.
    fn standard_at(&self) -> Vec<usize> {
        let mut at: Vec<usize> = (0..self.entries.len()).filter(|&k| self.entries[k].0.kind == "lua").collect();
        at.sort_by_key(|&k| self.entries[k].0.name.to_lowercase());
        at
    }

    /// The standard Lua mods by name: each entry and what it is here (to install, to update, installed).
    pub fn standard(&self) -> Vec<(&Entry, Option<bool>)> {
        self.standard_at().into_iter().map(|k| (&self.entries[k].0, self.entries[k].1)).collect()
    }

    /// Standard mod `k` (of `standard`) installed or updated off the UI thread; false when it is installed as it is or
    /// another install runs.
    pub fn install_standard(&mut self, k: usize) -> bool {
        let Some(at) = self.standard_at().get(k).copied().filter(|&at| self.entries[at].1.is_some()) else {
            return false;
        };
        self.install_entry(at)
    }

    /// Offer `k` of the Lua mods' (`lua`) or the DAT packs' installed off the UI thread; false while another install
    /// runs.
    pub fn install_offer(&mut self, k: usize, lua: bool) -> bool {
        let Some(at) = self.offers(lua).get(k).map(|o| o.at) else { return false };
        if at == ALL {
            return self.update_all(lua) > 0;
        }
        if at < self.packs.len() {
            return self.install(at);
        }
        self.install_entry(at - self.packs.len())
    }

    /// The newer version the catalogue has of installed mod or pack `id`.
    pub fn update_for(&self, id: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.1 == Some(true) && e.0.id.eq_ignore_ascii_case(id)).map(|e| e.0.version.as_str())
    }

    /// The installed Lua mods (`lua`) or DAT packs with a newer version in the catalogue.
    pub fn updates(&self, lua: bool) -> usize {
        self.entries.iter().filter(|e| e.1 == Some(true) && (e.0.kind == "lua") == lua).count()
    }

    /// Updates of the kind neither installing nor queued: what Update All would start.
    pub fn waiting(&self, lua: bool) -> usize {
        let busy = |id: &str| self.progress(id).is_some() || self.queued(id);
        self.entries.iter().filter(|e| e.1 == Some(true) && (e.0.kind == "lua") == lua && !busy(&e.0.id)).count()
    }

    /// Waiting for another update to finish.
    pub fn queued(&self, id: &str) -> bool {
        self.queue.iter().any(|q| q.eq_ignore_ascii_case(id))
    }

    /// Installed mod or pack `id` updated from the catalogue, now or after the install running; false when it has no
    /// update.
    pub fn update(&mut self, id: &str) -> bool {
        let Some(at) = self.entries.iter().position(|e| e.1 == Some(true) && e.0.id.eq_ignore_ascii_case(id)) else {
            return false;
        };
        if self.installing.as_ref().is_some_and(|i| i.0.eq_ignore_ascii_case(id)) || self.queued(id) {
            return true;
        }
        if self.installing.is_some() {
            self.queue.push(self.entries[at].0.id.clone());
            return true;
        }
        self.install_entry(at)
    }

    /// Every Lua mod (`lua`) or DAT pack with an update, one after another; how many.
    pub fn update_all(&mut self, lua: bool) -> usize {
        let ids: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.1 == Some(true) && (e.0.kind == "lua") == lua)
            .map(|e| e.0.id.clone())
            .collect();
        ids.iter().filter(|id| self.update(id)).count()
    }

    /// Catalogue entry `at` installed off the UI thread; false while another install runs.
    fn install_entry(&mut self, at: usize) -> bool {
        let Some((e, _)) = self.entries.get(at).cloned() else { return false };
        if self.installing.is_some() {
            return false;
        }
        let progress = Arc::new(AtomicU32::new(0));
        let (tx, rx) = channel();
        let pr = progress.clone();
        let id = e.id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(catalogue::install(&e, &crate::config::mods_dir(), &pr));
        });
        self.installing = Some((id, progress, rx));
        true
    }

    /// Per frame: the scan's, the catalogue's and an install's results. A new install goes on top of `mods`, on (an
    /// update keeps its place); true when `mods` changed.
    pub fn tick(&mut self, mods: &mut Vec<ModRef>) -> bool {
        if let Some(Ok(p)) = self.scan.as_ref().map(|r| r.try_recv()) {
            self.packs = p.into_iter().filter(|p| !mods.iter().any(|m| m.name == p.slug)).collect();
            self.scan = None;
        }
        if let Some(Ok(l)) = self.list.as_ref().map(|r| r.try_recv()) {
            self.list = None;
            self.take_list(l, false);
        }
        if let Some(Ok(l)) = self.dat_list.as_ref().map(|r| r.try_recv()) {
            self.dat_list = None;
            self.take_list(l, true);
        }
        let Some((_, _, rx)) = &self.installing else { return false };
        let Ok(res) = rx.try_recv() else { return false };
        self.installing = None;
        // The next queued update starts (an error ends none of them).
        while !self.queue.is_empty() {
            let id = self.queue.remove(0);
            if let Some(at) = self.entries.iter().position(|e| e.1 == Some(true) && e.0.id == id)
                && self.install_entry(at)
            {
                break;
            }
        }
        match res {
            Ok(slug) => {
                self.packs.retain(|p| p.slug != slug);
                for e in self.entries.iter_mut().filter(|e| e.0.id == slug) {
                    if e.1 == Some(true) {
                        self.updated.push(slug.clone());
                    }
                    e.1 = None;
                }
                if !mods.iter().any(|m| m.name == slug) {
                    mods.insert(0, ModRef::new(slug, true));
                }
                self.error = None;
                true
            }
            Err(e) => {
                log::warn!("mods: installing: {e:#}");
                self.error = Some(format!("{e:#}"));
                false
            }
        }
    }

    /// Unpacks pack `i` off the UI thread; false while another one is being installed.
    pub fn install(&mut self, i: usize) -> bool {
        if self.installing.is_some() {
            return false;
        }
        let Some(p) = self.packs.get(i).cloned() else { return false };
        let progress = Arc::new(AtomicU32::new(0));
        let (tx, rx) = channel();
        let pr = progress.clone();
        std::thread::spawn(move || {
            let _ = tx.send(modpack::install(&p, &crate::config::mods_dir(), &pr).map(|_| p.slug));
        });
        self.installing = Some((self.packs[i].slug.clone(), progress, rx));
        true
    }

    /// Reads and unpacks the archive at `path`, or copies the Lua mod folder there, off the UI thread; false while
    /// another one is being installed.
    pub fn install_file(&mut self, path: PathBuf) -> bool {
        if self.installing.is_some() {
            return false;
        }
        let slug = modpack::slug(&path);
        let progress = Arc::new(AtomicU32::new(0));
        let (tx, rx) = channel();
        let pr = progress.clone();
        std::thread::spawn(move || {
            let root = crate::config::mods_dir();
            let r = if path.is_dir() {
                modpack::install_dir(&path, &root)
            } else {
                modpack::pack_of(&path).and_then(|p| modpack::install(&p, &root, &pr).map(|_| p.slug))
            };
            let _ = tx.send(r);
        });
        self.installing = Some((slug, progress, rx));
        true
    }

    /// The catalogue is still being read.
    pub fn loading(&self) -> bool {
        self.list.is_some()
    }

    /// The pack being installed and its per mille done.
    pub fn installing(&self) -> Option<(String, u32)> {
        self.installing.as_ref().map(|i| (i.0.clone(), i.1.load(Ordering::Relaxed)))
    }

    /// The per mille done when pack `slug` is being installed.
    pub fn progress(&self, slug: &str) -> Option<u32> {
        self.installing.as_ref().filter(|i| i.0 == slug).map(|i| i.1.load(Ordering::Relaxed))
    }

    /// Keeps cursor row `row` (1 = the first entry) in view.
    pub fn follow(&mut self, row: usize) {
        if let Some(i) = row.checked_sub(1) {
            if i < self.top {
                self.top = i;
            } else if i >= self.top + VISIBLE {
                self.top = i + 1 - VISIBLE;
            }
        } else {
            self.top = 0;
        }
    }
}

/// Deletes installed pack `name`'s folder off the UI thread; false when the name is not a folder of the mods folder.
pub fn remove_folder(name: &str) -> bool {
    let root = crate::config::mods_dir();
    let dir = root.join(name);
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name.starts_with('.')
        || dir.parent() != Some(root.as_path())
        || !dir.is_dir()
    {
        return false;
    }
    std::thread::spawn(move || {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            log::warn!("mods: removing {}: {e}", dir.display());
        } else {
            log::info!("mods: removed {}", dir.display());
        }
    });
    true
}

/// A file picker for an archive or a Lua mod's folder: a folder's sub-folders and archives (zip, 7z, rar), ".." first.
pub struct Browse {
    pub dir: PathBuf,
    pub entries: Vec<(String, Pick)>,
    pub cursor: usize,
    mods: bool,
}

/// What a picker entry is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pick {
    Folder,
    /// A folder with a mod.toml: picked, not opened.
    Mod,
    Archive,
}

impl Browse {
    /// At the Downloads folder (else home); `mods`: a Lua mod's folder is a pick too.
    pub fn open(mods: bool) -> Self {
        let dir = crate::config::downloads_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("/"));
        let mut b = Self { dir, entries: Vec::new(), cursor: 0, mods };
        b.read();
        b
    }

    fn read(&mut self) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    dirs.push((name, if self.mods && p.join("mod.toml").is_file() { Pick::Mod } else { Pick::Folder }));
                } else if modpack::is_archive(&p) {
                    files.push((name, Pick::Archive));
                }
            }
        }
        dirs.sort_by_key(|n| n.0.to_lowercase());
        files.sort_by_key(|n| n.0.to_lowercase());
        self.entries =
            self.dir.parent().map(|_| ("..".to_string(), Pick::Folder)).into_iter().chain(dirs).chain(files).collect();
        self.cursor = 0;
    }

    /// Enter on the cursor's entry: a folder is opened, an archive or a mod's folder is the pick.
    pub fn enter(&mut self) -> Option<PathBuf> {
        let (name, pick) = self.entries.get(self.cursor)?.clone();
        if pick != Pick::Folder {
            return Some(self.dir.join(name));
        }
        let from = self.dir.file_name().map(|f| f.to_string_lossy().into_owned());
        self.dir = if name == ".." { self.dir.parent()?.to_path_buf() } else { self.dir.join(&name) };
        self.read();
        // Back up a level onto the folder left.
        if name == ".."
            && let Some(f) = from
        {
            self.cursor = self.entries.iter().position(|e| e.0 == f).unwrap_or(0);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: &str) -> Entry {
        serde_json::from_value(serde_json::json!({"id": id, "name": id, "kind": kind, "version": "2.0.0", "files": []}))
            .unwrap()
    }

    /// The updates by kind, the newer version of one, and the Update All row (Lua: one is enough; DATs: two).
    #[test]
    fn updates_and_update_all_row() {
        let mut m = Mods::empty();
        m.entries = vec![
            (entry("a", "lua"), Some(true)),
            (entry("b", "lua"), None),
            (entry("c", "lua"), Some(false)),
            (entry("p", "pack"), Some(true)),
        ];
        assert_eq!((m.updates(true), m.updates(false), m.waiting(true)), (1, 1, 1));
        assert_eq!((m.update_for("A"), m.update_for("b"), m.update_for("c")), (Some("2.0.0"), None, None));
        let all =
            |m: &Mods, lua: bool| m.offers(lua).iter().filter(|o| o.all).map(|o| o.label.clone()).collect::<Vec<_>>();
        assert_eq!(all(&m, true), ["Update all (1)"]);
        assert!(all(&m, false).is_empty());
        m.entries.push((entry("q", "pack"), Some(true)));
        assert_eq!(all(&m, false), ["Update all (2)"]);
        // A queued update is not waiting, and is not queued twice.
        m.queue.push("a".into());
        assert_eq!(m.waiting(true), 0);
        assert!(m.update("a"));
        assert_eq!(m.queue, ["a"]);
        assert!(!m.update("b"));
    }
}
