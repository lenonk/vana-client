//! Ours (the plan's "UI states": Exploration, Combat, Management as adaptive presets, docs/modern-ui.md): which HUD
//! panels each mode shows. The mode follows the game (engaged: Combat, held a few seconds after; a window or menu
//! the command menu opens: Management; else Exploration), never takes the keyboard, and only hides or shows panels.
//! A panel pinned shows in every mode. Panels fade in and out unless fades are off. `/uimodes` opens the editor (a
//! panel per row, a toggle per mode, a pin); config `modern_modes`.

use super::super::World;
use super::{Panels, Title, kit};
use crate::config::ModernModes;
use egui::{Align2, Context, FontId, Id, Sense, vec2};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The modes, in config order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Exploration = 0,
    Combat = 1,
    Management = 2,
}

pub const MODES: [(Mode, &str); 3] = [(Mode::Exploration, "Exploration"), (Mode::Combat, "Combat"), (Mode::Management, "Management")];

/// The panels a mode may hide (panel id, name). Windows the player opens are never hidden.
pub const PANELS: [(&str, &str); 11] = [
    ("status", "Status icons"),
    ("party", "Party"),
    ("alliance1", "Alliance 1"),
    ("alliance2", "Alliance 2"),
    ("target", "Target"),
    ("where", "Location"),
    ("cast", "Cast bar"),
    ("minimap", "Minimap"),
    ("timers", "Timers"),
    ("regime", "Training"),
    ("skillchains", "Skillchains"),
];

/// How long Combat holds after disengaging (a pull's pause between targets keeps the combat layout).
const HOLD: Duration = Duration::from_secs(4);
/// A fade's length.
const FADE: f32 = 0.25;

/// The mode the game is in now; `engaged_at` keeps when we were last engaged.
pub fn current(w: &World, engaged_at: &std::cell::Cell<Option<Instant>>, focus: Option<&str>) -> Mode {
    if w.my_motion.status == 1 {
        engaged_at.set(Some(Instant::now()));
    }
    if engaged_at.get().is_some_and(|t| t.elapsed() < HOLD) {
        return Mode::Combat;
    }
    if matches!(focus, Some("window" | "command" | "command sub" | "list" | "cats" | "check" | "loot")) {
        return Mode::Management;
    }
    Mode::Exploration
}

/// Whether panel `id` shows in `mode`.
pub fn shows(cfg: &ModernModes, mode: Mode, id: &str) -> bool {
    cfg.pinned.iter().any(|p| p == id) || !cfg.hidden[mode as usize].iter().any(|p| p == id)
}

/// Each listed panel's opacity this frame (1 shown, 0 hidden; between while fading).
pub fn visibility(ctx: &Context, cfg: &ModernModes, mode: Mode) -> HashMap<&'static str, f32> {
    PANELS
        .iter()
        .map(|&(id, _)| {
            let on = shows(cfg, mode, id);
            let v = if cfg.fades { ctx.animate_bool_with_time(Id::unique(("modern mode fade", id)), on, FADE) } else if on { 1.0 } else { 0.0 };
            (id, v)
        })
        .collect()
}

/// The editor (/uimodes): per panel a toggle for each mode (lit: shown) and a pin; fades on or off; Reset; Close.
/// True when the config changed.
pub fn editor(p: &mut Panels, open: &mut bool, cfg: &mut ModernModes, mode: Mode) -> bool {
    if !*open {
        return false;
    }
    let m = p.m;
    let mut changed = false;
    let mut close = false;
    let note = format!("now: {}", MODES[mode as usize].1);
    let at = p.ctx.content_rect().center() - vec2(260.0, 230.0);
    p.panel_at("uimodes", Some(Title { icon: None, text: "UI Modes", note: Some(&note), name: false }), 560.0, at, |ui| {
        ui.label(egui::RichText::new("Which panels each mode shows. A pinned panel always shows.").size(m.theme.small).color(m.theme.color("muted")));
        ui.add_space(6.0);
        let font = FontId::proportional(m.theme.small);
        egui::Grid::new(Id::unique("modern ui modes")).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            ui.label("");
            for (_, name) in MODES {
                ui.label(egui::RichText::new(name).size(m.theme.small).color(m.theme.color("gold")));
            }
            ui.label(egui::RichText::new("Pinned").size(m.theme.small).color(m.theme.color("gold")));
            ui.end_row();
            for (id, name) in PANELS {
                let (r, _) = ui.allocate_exact_size(vec2(130.0, 26.0), Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font.clone(), m.theme.color("text"));
                let pinned = cfg.pinned.iter().any(|p| p == id);
                for (k, _) in MODES.iter().enumerate() {
                    let on = !cfg.hidden[k].iter().any(|p| p == id);
                    let s = kit::Btn { lit: on && !pinned, cursor: false, off: pinned };
                    if kit::clicked(&kit::button(ui, m, if on || pinned { "Shown" } else { "Hidden" }, 84.0, s), s) {
                        if on {
                            cfg.hidden[k].push(id.to_string());
                        } else {
                            cfg.hidden[k].retain(|p| p != id);
                        }
                        changed = true;
                    }
                }
                let s = kit::Btn { lit: pinned, cursor: false, off: false };
                if kit::clicked(&kit::button(ui, m, if pinned { "Pinned" } else { "Pin" }, 84.0, s), s) {
                    if pinned {
                        cfg.pinned.retain(|p| p != id);
                    } else {
                        cfg.pinned.push(id.to_string());
                    }
                    changed = true;
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let s = kit::Btn { lit: cfg.fades, cursor: false, off: false };
            if kit::clicked(&kit::button(ui, m, if cfg.fades { "Fades: On" } else { "Fades: Off" }, 0.0, s), s) {
                cfg.fades = !cfg.fades;
                changed = true;
            }
            let plain = kit::Btn { lit: false, cursor: false, off: false };
            if kit::clicked(&kit::button(ui, m, "Reset", 0.0, plain), plain) {
                *cfg = ModernModes::default();
                changed = true;
            }
            if kit::clicked(&kit::button(ui, m, "Close", 0.0, plain), plain) {
                close = true;
            }
        });
    });
    // Escape closes it in the key chain (client/mod.rs), so it reaches nothing under it.
    if close {
        *open = false;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::{Mode, shows};
    use crate::config::ModernModes;

    #[test]
    fn defaults_and_pins() {
        let mut c = ModernModes::default();
        // The location panel hides while engaged (user, 2026-11-01), as before the modes.
        assert!(!shows(&c, Mode::Combat, "where"));
        assert!(shows(&c, Mode::Exploration, "where"));
        assert!(shows(&c, Mode::Combat, "party"));
        c.pinned.push("where".into());
        assert!(shows(&c, Mode::Combat, "where"));
    }
}
