//! The first-start window on Android: All files access first, then game files from Square Enix's installer volumes
//! the user downloaded (kept where they are), or an install already on the device.

use super::{Action, MARKER, Stage, VOLUMES, closing_note, existing, set, volume};
use crate::install::Progress;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// PlayOnline's page for the five FFXIFullSetup volumes (the browser downloads them; the install reads them locally).
const SE_CLIENT_DOWNLOAD: &str = "https://www.playonline.com/ff11us/download/media/install_win.html";

fn downloads() -> PathBuf {
    crate::config::downloads_dir().unwrap_or_default()
}

/// Folders searched for the installer volumes: the install's own `.download`, the install folder, Download and the
/// folders directly under it.
fn installer_search_dirs(install_parent: &Path) -> Vec<PathBuf> {
    let downloads = downloads();
    let mut dirs = vec![install_parent.join(".download"), install_parent.to_path_buf(), downloads.clone()];
    if let Ok(rd) = std::fs::read_dir(&downloads) {
        dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn installer_parts_in(dir: &Path) -> usize {
    (1..=VOLUMES).filter(|i| dir.join(volume(*i)).is_file()).count()
}

/// The folder holding all five volumes: the one typed in, else the first found.
fn installer_dir(install_parent: &Path, archives: &Path) -> Option<PathBuf> {
    if !archives.as_os_str().is_empty() && installer_parts_in(archives) == VOLUMES {
        return Some(archives.to_path_buf());
    }
    installer_search_dirs(install_parent).into_iter().find(|d| installer_parts_in(d) == VOLUMES)
}

/// The folder with the most volumes, for the note when none has all five.
fn best_installer_parts(install_parent: &Path) -> Option<(PathBuf, usize)> {
    installer_search_dirs(install_parent)
        .into_iter()
        .map(|d| {
            let n = installer_parts_in(&d);
            (d, n)
        })
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
}

fn short_path(p: &Path) -> String {
    let s = p.display().to_string().replace("/storage/emulated/0", "/sdcard");
    let n = s.chars().count();
    if n > 44 { format!("…{}", s.chars().skip(n - 40).collect::<String>()) } else { s }
}

/// Which choice the in-app folder list is filling in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Browse {
    Existing,
    Archives,
    InstallTo,
}

pub struct FirstRun {
    game_dir: PathBuf,
    stage: Arc<Mutex<Stage>>,
    progress: Arc<Progress>,
    /// The folder that will hold `FINAL FANTASY XI`, as typed.
    path_input: String,
    /// The folder with the installer volumes, as typed.
    archive_path_input: String,
    browse: Option<Browse>,
    /// Folder the in-app list is showing while `browse` is set.
    browser_at: PathBuf,
    note: String,
}

impl FirstRun {
    /// The game folder (parent of `FINAL FANTASY XI`) if the install path field resolves.
    fn resolved_game_dir(&self) -> Option<PathBuf> {
        let typed = PathBuf::from(self.path_input.trim());
        let pick = if typed.as_os_str().is_empty() { self.game_dir.clone() } else { typed };
        existing(&pick).ok()
    }

    fn try_start(&mut self, action: &mut Action) {
        self.note.clear();
        if !crate::android_fs::has_all_files_access() {
            self.note = "Turn on All files access (step 1), then tap Start.".into();
            return;
        }
        let typed = PathBuf::from(self.path_input.trim());
        let pick = if typed.as_os_str().is_empty() { self.game_dir.clone() } else { typed };
        match existing(&pick) {
            Ok(game_dir) => {
                log::info!("starting with game files in {}", game_dir.display());
                self.path_input = game_dir.display().to_string();
                self.game_dir = game_dir;
                *action = Action::Ready;
            }
            Err(e) => self.note = format!("{e:#}"),
        }
    }

    pub fn new(game_dir: PathBuf) -> Self {
        // Configs from before shared storage kept the game in app storage.
        let game_dir =
            if crate::config::in_app_storage(&game_dir) { crate::config::default_game_dir() } else { game_dir };
        Self {
            path_input: game_dir.display().to_string(),
            archive_path_input: downloads().join("FFXI-setup").display().to_string(),
            game_dir,
            stage: Arc::new(Mutex::new(Stage::Ask)),
            progress: Arc::new(Progress::default()),
            browse: None,
            browser_at: crate::client::folder_pick::storage_root(),
            note: String::new(),
        }
    }

    /// Where the game files go (the config default, or the folder picked).
    pub fn game_dir(&self) -> &Path {
        &self.game_dir
    }

    fn install_parent(&self) -> PathBuf {
        let typed = PathBuf::from(self.path_input.trim());
        if typed.as_os_str().is_empty() { self.game_dir.clone() } else { typed }
    }

    fn begin_install(&mut self) -> Action {
        self.note.clear();
        let parent = PathBuf::from(self.path_input.trim());
        let default = crate::config::default_game_dir();
        if parent.as_os_str().is_empty() {
            self.note = format!(
                "Enter the install folder (for example {}, which will hold FINAL FANTASY XI).",
                default.display()
            );
            return Action::None;
        }
        if crate::config::in_app_storage(&parent) {
            self.note = format!(
                "Install on shared storage (for example {}), not in the app's private data.",
                default.display()
            );
            return Action::None;
        }
        if let Err(e) = std::fs::create_dir_all(&parent) {
            self.note = format!("Cannot use that install folder: {e}");
            return Action::None;
        }
        let archives = PathBuf::from(self.archive_path_input.trim());
        let Some(archive_dir) = installer_dir(&parent, &archives) else {
            let hint = if !archives.as_os_str().is_empty() {
                format!("Found {} of {VOLUMES} parts in {}.", installer_parts_in(&archives), archives.display())
            } else {
                best_installer_parts(&parent)
                    .map(|(d, n)| format!("Found {n} of {VOLUMES} parts in {}.", d.display()))
                    .unwrap_or_else(|| format!("No installer parts in {}.", downloads().display()))
            };
            self.note = format!(
                "{hint} Put all five files (FFXIFullSetup_US.part1.exe and part2–5.rar) in the archives folder, or fix the path above."
            );
            return Action::None;
        };
        log::info!("installing from the volumes in {} to {}", archive_dir.display(), parent.display());
        self.game_dir = parent.clone();
        self.archive_path_input = archive_dir.display().to_string();
        self.start(archive_dir);
        Action::Begin(parent)
    }

    fn start(&self, archive_dir: PathBuf) {
        *self.stage.lock().unwrap() = Stage::Working;
        let (game_dir, stage, progress) = (self.game_dir.clone(), self.stage.clone(), self.progress.clone());
        std::thread::spawn(move || {
            // A panic would leave the window on its progress bar for good.
            let r =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| install(&game_dir, &archive_dir, &progress)))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("the install stopped unexpectedly (see logcat)")));
            *stage.lock().unwrap() = match r {
                Ok(()) => Stage::Done,
                Err(e) => {
                    log::warn!("first start: {e:#}");
                    Stage::Failed(format!("{e:#}"))
                }
            };
        });
    }

    /// Applies a folder from the system picker (`browse` says which field it was for).
    fn apply_folder_pick(&mut self, what: Browse, dir: &Path) {
        log::info!("folder picked: {}", dir.display());
        match what {
            Browse::Archives => {
                self.archive_path_input = dir.display().to_string();
                let n = installer_parts_in(dir);
                self.note = if n == VOLUMES {
                    format!("Installer: {VOLUMES}/{VOLUMES} in {}", short_path(dir))
                } else {
                    format!(
                        "Found {n}/{VOLUMES} installer parts in {}. Pick the folder that has all five.",
                        short_path(dir)
                    )
                };
            }
            Browse::InstallTo => {
                self.path_input = dir.display().to_string();
                self.game_dir = dir.to_path_buf();
                self.note = format!("Install to {}", short_path(dir));
            }
            Browse::Existing => {
                self.path_input = dir.display().to_string();
                if !crate::android_fs::has_all_files_access() {
                    self.note = "Turn on All files access (step 1), then tap Start.".into();
                } else {
                    match existing(dir) {
                        Ok(game_dir) => {
                            log::info!("picked game files in {}", game_dir.display());
                            self.path_input = game_dir.display().to_string();
                            self.game_dir = game_dir;
                            self.note = "Folder set — tap Start.".into();
                        }
                        Err(e) => {
                            self.note = format!("{e:#} Tap Start to try this path, or pick a parent folder.");
                        }
                    }
                }
            }
        }
    }

    fn open_browser(&mut self, what: Browse, hint: &Path) {
        self.browser_at = crate::client::folder_pick::start_dir(hint);
        self.browse = Some(what);
    }

    /// In-app folder list. Returns true when it drew (the rest of the form stays hidden).
    fn folder_browser(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(what) = self.browse else { return false };
        ui.label(egui::RichText::new("Choose a folder").size(15.0));
        if !self.note.is_empty() {
            ui.label(egui::RichText::new(&self.note).size(13.0).color(egui::Color32::from_rgb(0xe0, 0xa0, 0x70)));
        }
        let cwd = self.browser_at.clone();
        let listing = crate::client::folder_pick::dir_listing(&cwd);
        match crate::client::folder_pick::browser(ui, &cwd, listing) {
            crate::client::folder_pick::BrowserAction::Up => {
                self.browser_at = crate::client::folder_pick::go_up(&cwd);
            }
            crate::client::folder_pick::BrowserAction::Enter(dir) => self.browser_at = dir,
            crate::client::folder_pick::BrowserAction::Use => {
                self.browse = None;
                self.apply_folder_pick(what, &cwd);
            }
            crate::client::folder_pick::BrowserAction::Cancel => {
                self.browse = None;
                self.note = "No folder was chosen.".into();
            }
            crate::client::folder_pick::BrowserAction::None => {}
        }
        true
    }

    /// The window (egui: no game art exists yet).
    pub fn ui(&mut self, ctx: &egui::Context) -> Action {
        let failed = match &*self.stage.lock().unwrap() {
            Stage::Done => return Action::Ready,
            Stage::Failed(e) => Some(e.clone()),
            _ => None,
        };
        let working = matches!(&*self.stage.lock().unwrap(), Stage::Working);
        let mut action = Action::None;
        let screen = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            screen,
            0.0,
            egui::Color32::from_rgb(0x12, 0x14, 0x22),
        );
        egui::Area::new(egui::Id::new("first run")).anchor(egui::Align2::CENTER_TOP, [0.0, 10.0]).show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(0x1e, 0x22, 0x3a))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x6a, 0x70, 0xa8)))
                .corner_radius(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.set_max_width((screen.width() - 28.0).max(320.0));
                    ui.style_mut().spacing.button_padding = egui::vec2(10.0, 6.0);
                    ui.label(
                        egui::RichText::new("FINAL FANTASY XI game files")
                            .size(20.0)
                            .color(egui::Color32::from_rgb(0xe8, 0xc8, 0x80)),
                    );
                    ui.add_space(4.0);
                    match (&failed, working) {
                        (Some(e), _) => self.failed_ui(ui, e, &mut action),
                        (None, true) => self.progress_ui(ui),
                        (None, false) => self.ask_ui_scroll(ui, &mut action),
                    }
                    if failed.is_none() && !working && self.browse.is_none() {
                        self.ask_ui_start_row(ui, &mut action);
                    }
                });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(if working { 250 } else { 16 }));
        action
    }

    /// Start loads an existing install, or unpacks the five volumes into Install to.
    fn can_start(&self) -> bool {
        if !crate::android_fs::poll_all_files_access().unwrap_or(false) {
            return false;
        }
        if self.resolved_game_dir().is_some() {
            return true;
        }
        let archives = PathBuf::from(self.archive_path_input.trim());
        poll_installer_ready(&self.install_parent(), &archives) == Some(true)
    }

    fn on_start(&mut self, action: &mut Action) {
        if self.resolved_game_dir().is_some() {
            self.try_start(action);
        } else {
            *action = self.begin_install();
        }
    }

    fn ask_ui_scroll(&mut self, ui: &mut egui::Ui, _action: &mut Action) {
        if self.folder_browser(ui) {
            return;
        }
        let install_parent = self.install_parent();
        let archives = PathBuf::from(self.archive_path_input.trim());
        let _ = crate::android_fs::poll_all_files_access();
        request_installer_ready(install_parent.clone(), archives.clone());
        let all_files = crate::android_fs::poll_all_files_access().unwrap_or(false);
        let step = |n: &str| egui::RichText::new(n).size(14.0).color(egui::Color32::LIGHT_GRAY);
        let green = egui::Color32::from_rgb(0x90, 0xc8, 0x90);

        ui.label(step("Step 1"));
        ui.label(egui::RichText::new("All files access").size(15.0));
        ui.horizontal(|ui| {
            if ui.button("Open settings").clicked() {
                crate::android_fs::request_all_files_access();
            }
            let (status, color) =
                if all_files { ("Active", green) } else { ("Not active", egui::Color32::from_rgb(0xe0, 0xa0, 0x70)) };
            ui.label(egui::RichText::new(status).size(13.0).color(color));
        });
        ui.add_space(10.0);
        ui.label(step("Step 2"));
        ui.label(
            egui::RichText::new("Install from one of: Square Enix, local FFXIFullSetup, or an existing install.")
                .size(13.0),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Square Enix").clicked() {
                crate::android_fs::open_url(SE_CLIENT_DOWNLOAD);
                self.note = "Download all five FFXIFullSetup parts to one folder, then use Local installer.".into();
            }
            if ui.button("Local installer").clicked() {
                self.note = "Choose the folder that holds all five FFXIFullSetup parts.".into();
                let start = if archives.as_os_str().is_empty() { downloads() } else { archives };
                self.open_browser(Browse::Archives, &start);
            }
            if ui.button("Existing install").clicked() {
                self.note = "Choose the folder that contains FINAL FANTASY XI.".into();
                self.open_browser(Browse::Existing, &install_parent);
            }
        });
        if !self.note.is_empty() {
            ui.label(egui::RichText::new(&self.note).size(13.0).color(egui::Color32::from_rgb(0xe0, 0xa0, 0x70)));
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Install to").size(14.0));
        ui.label(egui::RichText::new(short_path(&install_parent)).size(13.0).font(egui::FontId::monospace(13.0)));
        if ui.button("Browse install folder").clicked() {
            self.note.clear();
            self.open_browser(Browse::InstallTo, &install_parent);
        }
    }

    fn ask_ui_start_row(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Quit").clicked() {
                *action = Action::Quit;
            }
            let start = ui.add_enabled(self.can_start(), egui::Button::new(egui::RichText::new("Start").size(18.0)));
            if start.clicked() {
                self.on_start(action);
            }
        });
        if !crate::android_fs::poll_all_files_access().unwrap_or(false) {
            ui.label(egui::RichText::new("All files access must be Active before Start.").size(12.0).weak());
        }
    }

    fn progress_ui(&self, ui: &mut egui::Ui) {
        let p = &self.progress;
        let (done, total) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
        let phase = p.phase.lock().unwrap().clone();
        let width = ui.available_width();
        let (left, gap) = ((width * 0.38).clamp(120.0, 280.0), 10.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(egui::RichText::new(&phase).size(14.0));
                if total > 0 {
                    ui.label(
                        egui::RichText::new(format!("{:.2} / {:.2} GB", done as f64 / 1e9, total as f64 / 1e9))
                            .size(12.0)
                            .weak(),
                    );
                }
                let note = closing_note(&phase);
                if !note.is_empty() {
                    ui.label(egui::RichText::new(note).weak().size(11.0));
                }
            });
            ui.add_space(gap);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(p.fraction())
                        .desired_width((width - left - gap).max(80.0))
                        .fill(egui::Color32::from_rgb(0x9a, 0x7a, 0x40)),
                );
            });
        });
    }

    fn failed_ui(&mut self, ui: &mut egui::Ui, failed: &str, action: &mut Action) {
        ui.label("It did not finish:");
        ui.label(egui::RichText::new(failed).weak().size(13.0));
        ui.horizontal(|ui| {
            if ui.button("Try again").clicked() {
                *self.stage.lock().unwrap() = Stage::Ask;
                *action = self.begin_install();
            }
            if ui.button("Quit").clicked() {
                *action = Action::Quit;
            }
        });
    }
}

enum InstallerPoll {
    Loading,
    Ready(bool),
}

static INSTALLER: Mutex<Option<(PathBuf, PathBuf, InstallerPoll)>> = Mutex::new(None);

fn request_installer_ready(parent: PathBuf, archives: PathBuf) {
    let mut g = INSTALLER.lock().unwrap();
    if g.as_ref().is_some_and(|(p, a, s)| *p == parent && *a == archives && !matches!(s, InstallerPoll::Loading)) {
        return;
    }
    if g.as_ref().is_some_and(|(p, a, s)| *p == parent && *a == archives && matches!(s, InstallerPoll::Loading)) {
        return;
    }
    *g = Some((parent.clone(), archives.clone(), InstallerPoll::Loading));
    std::thread::spawn(move || {
        let ok = installer_dir(&parent, &archives).is_some();
        *INSTALLER.lock().unwrap() = Some((parent, archives, InstallerPoll::Ready(ok)));
    });
}

fn poll_installer_ready(parent: &Path, archives: &Path) -> Option<bool> {
    request_installer_ready(parent.to_path_buf(), archives.to_path_buf());
    match &*INSTALLER.lock().unwrap() {
        Some((p, a, InstallerPoll::Ready(ok))) if p == parent && a == archives => Some(*ok),
        _ => None,
    }
}

/// Install from the volumes in `archive_dir` (left in place), update, clean up.
fn install(game_dir: &Path, archive_dir: &Path, p: &Arc<Progress>) -> Result<()> {
    std::fs::write(game_dir.join(MARKER), b"")?;
    let root = game_dir.join("FINAL FANTASY XI");
    let opts = crate::install::Options {
        source: archive_dir.join(volume(1)),
        staging: game_dir.join(".staging"),
        game_dir: game_dir.to_path_buf(),
        verify: true,
        keep_staging: false,
    };
    crate::install::install(&opts, p.clone())?;
    set(p, "Checking for game updates", 0, 0);
    let plan = crate::patch::plan(&root, true, false)?;
    let total: u64 = plan.todo.iter().map(|t| t.2).sum();
    let n = plan.todo.len();
    crate::patch::apply(&root, &plan, 4, &|f, b| set(p, format!("Updating game files ({f} of {n})"), b, total))?;
    set(p, "Cleaning up", 0, 0);
    let _ = std::fs::remove_dir_all(game_dir.join(".staging"));
    std::fs::remove_file(game_dir.join(MARKER))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_volumes_must_share_one_folder() {
        let base = std::env::temp_dir().join(format!("vana-installer-{}", std::process::id()));
        let install_parent = base.join("SquareEnix");
        let stash = install_parent.join(".download");
        std::fs::create_dir_all(&stash).unwrap();
        std::fs::write(stash.join(volume(1)), b"stub").unwrap();
        std::fs::write(stash.join(volume(2)), b"stub").unwrap();
        assert_eq!(best_installer_parts(&install_parent).unwrap().1, 2);
        assert!(installer_dir(&install_parent, Path::new("")).is_none());
        for i in 3..=VOLUMES {
            std::fs::write(stash.join(volume(i)), b"stub").unwrap();
        }
        assert_eq!(installer_dir(&install_parent, Path::new("")).unwrap(), stash);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
