//! Native replacement for FFXISetup.exe + FINAL_FANTASY_XI.msi: unpacks the game data.
//! PlayOnline, DirectX and the registry are Windows runtime concerns and are skipped.

use crate::cabx;
use crate::msi_db::{Database, DirResolver};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const PACKAGE_DIR: &str = "FFXIFullSetup_US";
const FFXI_MSI: &str = "FINAL_FANTASY_XI/FINAL_FANTASY_XI.msi";

#[derive(Default)]
pub struct Progress {
    pub phase: Mutex<String>,
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    fn begin(&self, phase: impl Into<String>, total: u64) {
        *self.phase.lock().unwrap() = phase.into();
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 { 0.0 } else { self.done.load(Ordering::Relaxed) as f32 / total as f32 }
    }
}

pub struct Options {
    /// FFXIFullSetup_US.part1.exe, or a folder it was already extracted into.
    pub source: PathBuf,
    pub game_dir: PathBuf,
    pub staging: PathBuf,
    pub verify: bool,
    pub keep_staging: bool,
}

/// Finds the extracted package folder under `dir`, if there is one.
fn package_root(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join(PACKAGE_DIR)].into_iter().find(|d| d.join(FFXI_MSI).is_file())
}

/// Extracts only the FFXI package from the multi-volume RAR self-extractor.
fn unpack_rar(first_volume: &Path, staging: &Path, progress: &Progress) -> Result<PathBuf> {
    let wanted = Path::new(PACKAGE_DIR).join("FINAL_FANTASY_XI");
    let mut total = 0;
    let mut listing = unrar::Archive::new(first_volume).open_for_listing().context("opening RAR archive")?;
    while let Some(Ok(entry)) = listing.next() {
        if entry.is_file() && entry.filename.starts_with(&wanted) && !entry.is_split_before() {
            total += entry.unpacked_size;
        }
    }
    progress.begin("Unpacking installer archive", total);

    std::fs::create_dir_all(staging)?;
    let mut archive = unrar::Archive::new(first_volume).open_for_processing()?;
    while let Some(header) = archive.read_header()? {
        let entry = header.entry();
        archive = if entry.is_file() && entry.filename.starts_with(&wanted) {
            let (name, size) = (entry.filename.clone(), entry.unpacked_size);
            log::info!("unpacking {}", name.display());
            *progress.phase.lock().unwrap() = format!("Unpacking {}", name.file_name().unwrap_or_default().to_string_lossy());
            let next = header.extract_with_base(staging)?;
            progress.done.fetch_add(size, Ordering::Relaxed);
            next
        } else {
            header.skip()?
        };
    }
    package_root(staging).ok_or_else(|| anyhow!("archive did not contain {FFXI_MSI}"))
}

pub fn install(opts: &Options, progress: Arc<Progress>) -> Result<()> {
    let (root, unpacked) = match package_root(&opts.source) {
        Some(root) => (root, false),
        None if opts.source.is_file() => (unpack_rar(&opts.source, &opts.staging, &progress)?, true),
        None => bail!("{} is neither the installer nor an extracted copy of it", opts.source.display()),
    };

    let (db, _) = Database::open(&root.join(FFXI_MSI))?;
    let install_root = opts.game_dir.join("FINAL FANTASY XI");
    // Map DIR_FF11 to "" so every path it resolves is relative to the game folder.
    let resolver = DirResolver::new(&db, HashMap::from([("DIR_FF11".to_string(), String::new())]));

    let mut targets: HashMap<String, (PathBuf, u64)> = HashMap::new();
    for file in &db.files {
        if !db.component_installed(&file.component) {
            continue;
        }
        let Some(dir) = db.components.get(&file.component).and_then(|c| resolver.resolve(&c.directory).ok()) else {
            log::warn!("skipping {} (outside the game folder)", file.name);
            continue;
        };
        let mut path = install_root.clone();
        path.extend(dir.split('\\').filter(|s| !s.is_empty()));
        path.push(&file.name);
        targets.insert(file.key.clone(), (path, file.size));
    }
    let total: u64 = targets.values().map(|(_, size)| size).sum();
    progress.begin(format!("Installing {} files", targets.len()), total);

    let cab_dir = root.join("FINAL_FANTASY_XI");
    let results: Vec<Result<Vec<cabx::Extracted>>> = std::thread::scope(|s| {
        let handles: Vec<_> = db
            .media
            .iter()
            .map(|media| {
                let (targets, progress, cab) = (&targets, &progress, cab_dir.join(&media.cabinet));
                s.spawn(move || {
                    let file = std::fs::File::open(&cab).with_context(|| format!("opening {}", cab.display()))?;
                    cabx::extract_all(file, |name| targets.get(name).map(|(p, _)| p.clone()), |n| {
                        progress.done.fetch_add(n, Ordering::Relaxed);
                    })
                    .with_context(|| format!("extracting {}", cab.display()))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow!("extract thread panicked")))).collect()
    });

    let mut extracted = 0usize;
    let mut bad = Vec::new();
    for batch in results {
        for item in batch? {
            extracted += 1;
            if opts.verify && db.hashes.get(&item.name).is_some_and(|want| *want != item.md5) {
                bad.push(item.path.display().to_string());
            }
        }
    }
    if extracted != targets.len() {
        bail!("cabinets held {extracted} of the {} expected files", targets.len());
    }
    if !bad.is_empty() {
        bail!("{} files failed their MD5 check, e.g. {}", bad.len(), bad[0]);
    }

    if unpacked && !opts.keep_staging {
        std::fs::remove_dir_all(&opts.staging).ok();
    }
    *progress.phase.lock().unwrap() = "Installed".into();
    Ok(())
}
