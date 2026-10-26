//! Settings' Mods page (not retail; the mod manager, XIPivot's job built in): the installed packs in priority order,
//! each ON / OFF and movable (Move, then Up / Down, Enter to place, Escape to put it back, as the item list's Manual
//! sort), then the packs found in archives with an Install button. The archives are read and unpacked off the UI
//! thread (crate::modpack). Changes take effect on restart.

use crate::config::ModRef;
use crate::modpack::{self, Pack};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};

pub struct Mods {
    pub packs: Vec<Pack>,
    scan: Option<Receiver<Vec<Pack>>>,
    /// The pack being installed, its progress (per mille) and the result.
    installing: Option<(String, Arc<AtomicU32>, Receiver<anyhow::Result<usize>>)>,
    /// A mod being moved: its order before the move.
    pub held: Option<Vec<ModRef>>,
    /// The last install's failure, for the help bar.
    pub error: Option<String>,
    /// The first row shown.
    pub top: usize,
}

/// Rows shown at once (a page of 296 px from y 36, 34 px apart).
pub const VISIBLE: usize = 7;

impl Mods {
    /// Starts looking for packs.
    pub fn open() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(modpack::scan(&modpack::pack_dirs(), &crate::config::mods_dir()));
        });
        Self { packs: Vec::new(), scan: Some(rx), installing: None, held: None, error: None, top: 0 }
    }

    /// No archives and no scan (tests).
    #[cfg(test)]
    pub fn empty() -> Self {
        Self { packs: Vec::new(), scan: None, installing: None, held: None, error: None, top: 0 }
    }

    /// Waits for the scan (offscreen shots).
    pub fn wait_scan(&mut self) {
        if let Some(p) = self.scan.take().and_then(|r| r.recv().ok()) {
            self.packs = p;
        }
    }

    /// Per frame: the scan's and an install's results. An installed pack goes on top of `mods`, on; true when `mods`
    /// changed.
    pub fn tick(&mut self, mods: &mut Vec<ModRef>) -> bool {
        if let Some(Ok(p)) = self.scan.as_ref().map(|r| r.try_recv()) {
            self.packs = p.into_iter().filter(|p| !mods.iter().any(|m| m.name == p.slug)).collect();
            self.scan = None;
        }
        let Some((slug, _, rx)) = &self.installing else { return false };
        let Ok(res) = rx.try_recv() else { return false };
        let slug = slug.clone();
        self.installing = None;
        match res {
            Ok(_) => {
                self.packs.retain(|p| p.slug != slug);
                mods.retain(|m| m.name != slug);
                mods.insert(0, ModRef { name: slug, enabled: true });
                self.error = None;
                true
            }
            Err(e) => {
                log::warn!("mods: installing {slug}: {e:#}");
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
            let _ = tx.send(modpack::install(&p, &crate::config::mods_dir(), &pr));
        });
        self.installing = Some((self.packs[i].slug.clone(), progress, rx));
        true
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
