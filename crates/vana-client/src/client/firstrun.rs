//! First start without game files (not retail: retail's installer was its own program). The title and login are
//! drawn from the DATs, so until they exist a small window of our own asks to download the free retail client from
//! Square Enix, installs its game files, brings them up to date and deletes what it downloaded.

use crate::install::Progress;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Square Enix's download of the full client, five RAR volumes (part1 is the self-extractor).
const CDN: &str = "https://gdl.square-enix.com/ffxi/download/us";
const VOLUMES: usize = 5;
/// 7,734,210,712 bytes, what the CDN's lengths add up to (for the estimate before the first response).
const TOTAL: u64 = 7_734_210_712;

fn volume(i: usize) -> String {
    if i == 1 { "FFXIFullSetup_US.part1.exe".into() } else { format!("FFXIFullSetup_US.part{i}.rar") }
}

/// The first-start window's size (fixed; the configured size applies once the game files are in).
pub const WINDOW: [u32; 2] = [760, 540];

/// No game files where the config says they are.
pub fn needed(ffxi_dir: &Path) -> bool {
    !ffxi_dir.join("VTABLE.DAT").is_file()
}

enum Stage {
    Ask,
    Working,
    Done,
    Failed(String),
}

pub enum Action {
    None,
    Quit,
    /// Game files are in: load the title.
    Ready,
}

pub struct FirstRun {
    game_dir: PathBuf,
    stage: Arc<Mutex<Stage>>,
    progress: Arc<Progress>,
}

impl FirstRun {
    pub fn new(game_dir: PathBuf) -> Self {
        Self { game_dir, stage: Arc::new(Mutex::new(Stage::Ask)), progress: Arc::new(Progress::default()) }
    }

    fn start(&self) {
        *self.stage.lock().unwrap() = Stage::Working;
        let (game_dir, stage, progress) = (self.game_dir.clone(), self.stage.clone(), self.progress.clone());
        std::thread::spawn(move || {
            let r = run(&game_dir, &progress);
            *stage.lock().unwrap() = match r {
                Ok(()) => Stage::Done,
                Err(e) => {
                    log::warn!("first start: {e:#}");
                    Stage::Failed(format!("{e:#}"))
                }
            };
        });
    }

    /// The window (egui: no game art exists yet).
    pub fn ui(&mut self, ctx: &egui::Context) -> Action {
        let mut action = Action::None;
        let stage = match &*self.stage.lock().unwrap() {
            Stage::Ask => 0,
            Stage::Working => 1,
            Stage::Done => 2,
            Stage::Failed(_) => 3,
        };
        if stage == 2 {
            return Action::Ready;
        }
        let failed = match &*self.stage.lock().unwrap() {
            Stage::Failed(e) => e.clone(),
            _ => String::new(),
        };
        let gold = egui::Color32::from_rgb(0xe8, 0xc8, 0x80);
        let screen = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::background()).rect_filled(screen, 0.0, egui::Color32::from_rgb(0x12, 0x14, 0x22));
        egui::Area::new(egui::Id::new("first run")).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0x1e, 0x22, 0x3a))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x6a, 0x70, 0xa8)))
                    .corner_radius(6.0)
                    .inner_margin(32.0)
                    .show(ui, |ui| {
                        ui.set_max_width(620.0);
                        ui.style_mut().spacing.button_padding = egui::vec2(14.0, 8.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("FINAL FANTASY XI game files").size(30.0).color(gold));
                            ui.add_space(10.0);
                            match stage {
                                0 => {
                                    ui.label(egui::RichText::new(format!(
                                        "vana-client uses the game files of the free FINAL FANTASY XI client. Download it from Square Enix \
                                         (about 7.7 GB) and install its game files to\n\n{}\n\nThe download is deleted once the game \
                                         files are installed and up to date. About 25 GB of free space is needed while it runs.",
                                        self.game_dir.display()
                                    )).size(17.0));
                                    ui.add_space(16.0);
                                    ui.horizontal(|ui| {
                                        if ui.button(egui::RichText::new("Download and install").size(19.0)).clicked() {
                                            self.start();
                                        }
                                        if ui.button(egui::RichText::new("Quit").size(19.0)).clicked() {
                                            action = Action::Quit;
                                        }
                                    });
                                }
                                1 => {
                                    let p = &self.progress;
                                    let (done, total) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
                                    ui.label(egui::RichText::new(p.phase.lock().unwrap().clone()).size(19.0));
                                    ui.add_space(8.0);
                                    ui.add(egui::ProgressBar::new(p.fraction()).desired_width(620.0).fill(egui::Color32::from_rgb(0x9a, 0x7a, 0x40)));
                                    if total > 0 {
                                        ui.label(format!("{:.2} of {:.2} GB", done as f64 / 1e9, total as f64 / 1e9));
                                    }
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("Closing vana-client stops this; the download resumes next time.").weak());
                                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                                }
                                _ => {
                                    ui.label("It did not finish:");
                                    ui.label(egui::RichText::new(&failed).weak());
                                    ui.add_space(16.0);
                                    ui.horizontal(|ui| {
                                        if ui.button(egui::RichText::new("Try again").size(19.0)).clicked() {
                                            self.start();
                                        }
                                        if ui.button(egui::RichText::new("Quit").size(19.0)).clicked() {
                                            action = Action::Quit;
                                        }
                                    });
                                }
                            }
                        });
                    });
            }
        });
        action
    }
}

fn set(p: &Progress, phase: impl Into<String>, done: u64, total: u64) {
    *p.phase.lock().unwrap() = phase.into();
    p.done.store(done, Ordering::Relaxed);
    p.total.store(total, Ordering::Relaxed);
}

/// Download, install, update, clean up.
fn run(game_dir: &Path, p: &Arc<Progress>) -> Result<()> {
    let dl = game_dir.join(".download");
    std::fs::create_dir_all(&dl).with_context(|| dl.display().to_string())?;
    // Bytes already on disk count toward the bar, so a resumed download starts where it was.
    let mut before = 0u64;
    for i in 1..=VOLUMES {
        let dest = dl.join(volume(i));
        if dest.is_file() {
            before += std::fs::metadata(&dest)?.len();
            continue;
        }
        let base = before;
        super::updater::download(&format!("{CDN}/{}", volume(i)), &dest, &|d, _| set(p, format!("Downloading from Square Enix (part {i} of {VOLUMES})"), base + d, TOTAL))?;
        before += std::fs::metadata(&dest)?.len();
    }
    let opts = crate::install::Options { source: dl.join(volume(1)), staging: game_dir.join(".staging"), game_dir: game_dir.to_path_buf(), verify: true, keep_staging: false };
    crate::install::install(&opts, p.clone())?;
    let root = game_dir.join("FINAL FANTASY XI");
    set(p, "Checking for game updates", 0, 0);
    let plan = crate::patch::plan(&root, false)?;
    let total: u64 = plan.todo.iter().map(|t| t.2).sum();
    let n = plan.todo.len();
    crate::patch::apply(&root, &plan, 4, &|f, b| set(p, format!("Updating game files ({f} of {n})"), b, total))?;
    set(p, "Cleaning up", 0, 0);
    std::fs::remove_dir_all(&dl).with_context(|| format!("removing {}", dl.display()))?;
    let _ = std::fs::remove_dir_all(game_dir.join(".staging"));
    Ok(())
}
