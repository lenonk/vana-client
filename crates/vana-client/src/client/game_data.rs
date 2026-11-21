//! Android: where the game files are read from, changed from the title's Config list (Game data).

use super::firstrun::existing;
use super::folder_pick;
use std::path::PathBuf;

/// All files access's state and a button to its Settings page. The game files are read as plain paths, so nothing
/// loads without it.
pub fn all_files_access_row(ui: &mut egui::Ui) {
    let ok = crate::android_fs::has_all_files_access();
    ui.horizontal(|ui| {
        if ui.button("Open settings").clicked() {
            crate::android_fs::request_all_files_access();
        }
        let (status, color) = if ok {
            ("All files access: active", egui::Color32::from_rgb(0x90, 0xc8, 0x90))
        } else {
            ("All files access: not active", egui::Color32::from_rgb(0xe0, 0xa0, 0x70))
        };
        ui.label(egui::RichText::new(status).size(13.0).color(color));
    });
}

pub enum Action {
    None,
    Close,
    /// The game folder (the one holding `FINAL FANTASY XI`) to save and load from.
    Apply(PathBuf),
}

pub struct Editor {
    game_dir: PathBuf,
    path_input: String,
    /// Set while the in-app folder list is open.
    browser_at: Option<PathBuf>,
    note: String,
}

impl Editor {
    pub fn new(game_dir: PathBuf) -> Self {
        Self { path_input: game_dir.display().to_string(), game_dir, browser_at: None, note: String::new() }
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Action {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let mut action = Action::None;
        let screen = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            screen,
            0.0,
            egui::Color32::from_rgb(0x12, 0x14, 0x22),
        );
        egui::Area::new(egui::Id::new("game data")).anchor(egui::Align2::CENTER_TOP, [0.0, 10.0]).show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(0x1e, 0x22, 0x3a))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x6a, 0x70, 0xa8)))
                .corner_radius(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.set_max_width((screen.width() - 28.0).max(320.0));
                    ui.label(
                        egui::RichText::new("Game files location")
                            .size(20.0)
                            .color(egui::Color32::from_rgb(0xe8, 0xc8, 0x80)),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Point at the folder that contains FINAL FANTASY XI (PlayOnline / SquareEnix).",
                        )
                        .size(13.0),
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Folder:").size(15.0));
                    egui::TextEdit::singleline(&mut self.path_input).desired_width(f32::INFINITY).show(ui);
                    ui.add_space(6.0);
                    let typed = PathBuf::from(self.path_input.trim());
                    let (install_line, install_color) = if !typed.as_os_str().is_empty() {
                        match existing(&typed) {
                            Ok(g) => (
                                format!("Installed to: {}", g.join("FINAL FANTASY XI").display()),
                                egui::Color32::from_rgb(0x90, 0xc8, 0x90),
                            ),
                            Err(_) => {
                                (format!("Picked: {}", typed.display()), egui::Color32::from_rgb(0xe0, 0xa0, 0x70))
                            }
                        }
                    } else {
                        (
                            format!("Installed to: {}", self.game_dir.join("FINAL FANTASY XI").display()),
                            egui::Color32::from_rgb(0xc0, 0xc0, 0xc0),
                        )
                    };
                    ui.label(egui::RichText::new(install_line).size(14.0).color(install_color));
                    all_files_access_row(ui);
                    if !self.note.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.note).size(13.0).color(egui::Color32::from_rgb(0xe0, 0x80, 0x70)),
                        );
                    }
                    ui.add_space(8.0);
                    if let Some(cwd) = self.browser_at.clone() {
                        match folder_pick::browser(ui, &cwd) {
                            folder_pick::BrowserAction::Up => self.browser_at = Some(folder_pick::go_up(&cwd)),
                            folder_pick::BrowserAction::Enter(dir) => self.browser_at = Some(dir),
                            folder_pick::BrowserAction::Use => {
                                self.path_input = cwd.display().to_string();
                                self.browser_at = None;
                                self.note.clear();
                            }
                            folder_pick::BrowserAction::Cancel => {
                                self.browser_at = None;
                                self.note = "No folder was chosen.".into();
                            }
                            folder_pick::BrowserAction::None => {}
                        }
                    }
                    ui.horizontal(|ui| {
                        let typed = PathBuf::from(self.path_input.trim());
                        let listing = self.browser_at.is_some();
                        if ui.add_enabled(!listing, egui::Button::new("Browse…")).clicked() {
                            self.note.clear();
                            let hint = if typed.as_os_str().is_empty() { self.game_dir.clone() } else { typed.clone() };
                            self.browser_at = Some(folder_pick::start_dir(&hint));
                        }
                        if ui.add_enabled(!listing, egui::Button::new("Start")).clicked() {
                            self.note.clear();
                            if typed.as_os_str().is_empty() {
                                self.note = "Choose a folder first.".into();
                            } else if !crate::android_fs::has_all_files_access() {
                                self.note = "Turn on All files access above, then pick the folder again.".into();
                            } else {
                                match existing(&typed) {
                                    Ok(game_dir) => action = Action::Apply(game_dir),
                                    Err(e) => self.note = format!("{e:#}"),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            action = Action::Close;
                        }
                    });
                });
        });
        action
    }
}
