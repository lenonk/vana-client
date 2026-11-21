//! In-app folder browser on shared storage (`/storage/...`). Needs All files access; there is no system document picker.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Shared storage root. Listing this needs All files access.
pub fn storage_root() -> PathBuf {
    PathBuf::from("/storage/emulated/0")
}

/// A directory that exists, else its parent, else shared storage.
pub fn start_dir(hint: &Path) -> PathBuf {
    let mut p = if hint.as_os_str().is_empty() { storage_root() } else { hint.to_path_buf() };
    if !p.is_dir() {
        p = p.parent().map(Path::to_path_buf).unwrap_or_else(storage_root);
    }
    if !p.starts_with("/storage") { storage_root() } else { p }
}

pub enum BrowserAction {
    None,
    Up,
    Enter(PathBuf),
    Use,
    Cancel,
}

enum ListingState {
    Loading(PathBuf),
    Ready { cwd: PathBuf, dirs: Arc<Vec<PathBuf>> },
}

static LISTING: Mutex<Option<ListingState>> = Mutex::new(None);

/// Lists `cwd` on a worker thread if it is not already loading or ready.
pub fn request_dir_listing(cwd: PathBuf) {
    let mut g = LISTING.lock().unwrap();
    if matches!(&*g, Some(ListingState::Ready { cwd: c, .. }) if c == &cwd) {
        return;
    }
    if matches!(&*g, Some(ListingState::Loading(c)) if c == &cwd) {
        return;
    }
    *g = Some(ListingState::Loading(cwd.clone()));
    std::thread::spawn(move || {
        let mut kids: Vec<PathBuf> =
            std::fs::read_dir(&cwd).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        kids.sort();
        *LISTING.lock().unwrap() = Some(ListingState::Ready { cwd, dirs: Arc::new(kids) });
    });
}

pub enum DirListing {
    Loading,
    Ready(Arc<Vec<PathBuf>>),
}

/// The listing for `cwd`, requesting a load when needed.
pub fn dir_listing(cwd: &Path) -> DirListing {
    request_dir_listing(cwd.to_path_buf());
    match &*LISTING.lock().unwrap() {
        Some(ListingState::Loading(c)) if c == cwd => DirListing::Loading,
        Some(ListingState::Ready { cwd: c, dirs }) if c == cwd => DirListing::Ready(dirs.clone()),
        _ => DirListing::Loading,
    }
}

/// In-app folder list. `cwd` is shown; the caller applies [`BrowserAction`].
pub fn browser(ui: &mut egui::Ui, cwd: &Path, listing: DirListing) -> BrowserAction {
    let mut action = BrowserAction::None;
    ui.label(egui::RichText::new(cwd.display().to_string()).size(12.0).font(egui::FontId::monospace(12.0)));
    ui.horizontal(|ui| {
        if ui.button("Up").clicked() {
            action = BrowserAction::Up;
        }
        if ui.button("Use this folder").clicked() {
            action = BrowserAction::Use;
        }
        if ui.button("Cancel").clicked() {
            action = BrowserAction::Cancel;
        }
    });
    let list_h = (ui.ctx().content_rect().height() * 0.45).clamp(140.0, 420.0);
    egui::ScrollArea::vertical().id_salt("folder list").max_height(list_h).auto_shrink([false; 2]).show(ui, |ui| {
        match listing {
            DirListing::Loading => {
                ui.label(egui::RichText::new("Loading folders…").weak().size(12.0));
            }
            DirListing::Ready(kids) if kids.is_empty() => {
                ui.label(egui::RichText::new("No subfolders (or this folder is not readable).").weak().size(12.0));
            }
            DirListing::Ready(kids) => {
                for dir in kids.iter() {
                    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.button(format!("{name}/")).clicked() {
                        action = BrowserAction::Enter(dir.clone());
                    }
                }
            }
        }
    });
    action
}

pub fn go_up(cwd: &Path) -> PathBuf {
    match cwd.parent() {
        Some(p) if p.starts_with("/storage") && p != Path::new("/") => p.to_path_buf(),
        _ => cwd.to_path_buf(),
    }
}
