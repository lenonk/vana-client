//! The modern lobby screens over the retail ones (client/select.rs keeps the select screen's rules and keys): the
//! character list with each character's job, level, race and area, the cursor's details, the counts, the settings
//! windows (chfwin, chswin) and the prompts (ptc6yesn, ok, the rename field) as modern panels. The 3D scene stays.

use super::super::game_ui::GameUi;
use super::super::select::{Pointer, View};
use super::craft::{button, tint};
use super::{Modern, Panels, Title, paint};
use egui::{Align2, Color32, Context, FontFamily, FontId, Sense, Ui, pos2, vec2};

const W: f32 = 380.0;
const ROW: f32 = 44.0;

impl Modern {
    /// egui's zoom for the window height and the Interface Size, as the game's layer sets it.
    fn zoom_to(&mut self, ctx: &Context, window: [u32; 2], scale: u32) {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = self.theme.zoom(window[1] as f32 / native, scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
    }

    /// The title's buttons on retail's own rects (`buttons`: the rect in UI pixels, the label, lit, enabled); their
    /// mouse stays the title's (client/mod.rs hits the same rects).
    pub fn title_buttons(&mut self, ctx: &Context, window: [u32; 2], scale: u32, ui_k: (f32, f32), buttons: &[([f32; 4], &str, bool, bool)]) {
        self.zoom_to(ctx, window, scale);
        let k = ctx.pixels_per_point();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("modern title buttons")));
        let hover = ctx.input(|i| i.pointer.hover_pos());
        for (r, label, lit, on) in buttons {
            let rect = egui::Rect::from_min_size(pos2(r[0] / (k * ui_k.0), r[1] / (k * ui_k.1)), vec2(r[2] / (k * ui_k.0), r[3] / (k * ui_k.1)));
            let face = match () {
                _ if !on => "controls/button_disabled",
                _ if *lit => "controls/button_selected",
                _ if hover.is_some_and(|h| rect.contains(h)) => "controls/button_hover",
                _ => "controls/button_normal",
            };
            if let Some(s) = self.art.sprite(face) {
                p.add(paint::nine_slice(&s, rect, 0.5, Color32::WHITE));
            }
            let c = if !on { self.theme.color("muted") } else if *lit { self.theme.color("gold") } else { self.theme.color("text") };
            p.text(rect.center(), Align2::CENTER_CENTER, *label, FontId::new(self.theme.ui, FontFamily::Name(super::STRONG.into())), c);
        }
    }

    /// The select (or delete) screen: what the mouse did, for SelectUi::pointer.
    pub fn select_screen(&mut self, ctx: &Context, gu: &GameUi, window: [u32; 2], scale: u32, v: &View) -> Pointer {
        self.zoom_to(ctx, window, scale);
        let mut layout = Default::default();
        let (mut changed, mut font) = (false, 15.0);
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout: &mut layout, changed: &mut changed, chat_font: &mut font, battle_in_all: &mut true, folded: &mut Default::default(), scan_kinds: &mut 7, this: Default::default(), retail: Default::default(), focus: Some("select") };
        let mut out = Pointer::default();
        list_panel(&mut p, gu, v, &mut out);
        windows(&mut p, gu, v, &mut out);
        out
    }
}

/// One character: the name in its colour and the job and level at the right, the race and area under them.
fn row(ui: &mut Ui, m: &Modern, r: &super::super::select::RowView, on: bool, focused: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
    let face = match () {
        _ if on && focused => "controls/list_row_selected",
        _ if on || focused && resp.hovered() => "controls/list_row_hover",
        _ => "controls/list_row_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, rect, (rect.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    let [_, race, job, level, area] = &r.info;
    let top = rect.min.y + 14.0;
    let strong = FontId::new(m.theme.ui, FontFamily::Name(super::STRONG.into()));
    ui.painter().text(pos2(rect.min.x + 24.0, top), Align2::LEFT_CENTER, &r.name, strong, tint(m, r.tint));
    ui.painter().text(pos2(rect.max.x - 12.0, top), Align2::RIGHT_CENTER, format!("{job} {level}"), FontId::proportional(m.theme.small), m.theme.color("gold"));
    let low = rect.max.y - 13.0;
    ui.painter().text(pos2(rect.min.x + 24.0, low), Align2::LEFT_CENTER, race, FontId::proportional(m.theme.small), m.theme.color("muted"));
    ui.painter().text(pos2(rect.max.x - 12.0, low), Align2::RIGHT_CENTER, area, FontId::proportional(m.theme.small), m.theme.color("muted"));
    resp
}

/// The list at the left: every character, the cursor's details, the counts and the help line.
fn list_panel(p: &mut Panels, gu: &GameUi, v: &View, out: &mut Pointer) {
    let m = p.m;
    let screen = p.ctx.content_rect();
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let title = if v.deleting { gu.lobby_msg(8, 0x6a, "Delete Character") } else { "Select Character".to_string() };
    let title = title.lines().next().unwrap_or_default().to_string();
    let at = pos2(screen.width() * 0.0121, screen.height() * 0.06);
    p.panel_at("select", Some(Title { icon: Some("icons/party"), text: &title, note: None }), W + 44.0, at, |ui| {
        let focused = v.list_focus;
        let tall = screen.height() * 0.5;
        egui::ScrollArea::vertical().id_salt("select rows").max_height(tall).auto_shrink([false, true]).show(ui, |ui| {
            for (k, r) in v.rows.iter().enumerate() {
                let resp = row(ui, m, r, k == v.cursor, focused);
                if focused && resp.hovered() && moved {
                    out.row.0 = Some(k);
                }
                if focused && resp.clicked() {
                    out.row.1 = Some(k);
                }
                if focused && resp.secondary_clicked() && !v.deleting {
                    out.row.0 = Some(k);
                    out.options = true;
                }
                ui.add_space(2.0);
            }
        });
        // The cursor's details (retail's info panel): World, Race, Job, Level, Area.
        if let Some(r) = v.rows.get(v.cursor) {
            ui.add_space(8.0);
            for (label, value) in v.labels.iter().zip(&r.info) {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("muted")));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(value).size(m.theme.small).color(m.theme.color("text")));
                    });
                });
            }
        }
        ui.add_space(8.0);
        for c in &v.counts {
            ui.label(egui::RichText::new(c).size(m.theme.small).color(m.theme.color("muted")));
        }
        if !v.deleting && v.options.is_none() && focused {
            ui.add_space(6.0);
            let label = gu.focus_text("menu    chfwin", 0)[1].clone().unwrap_or_else(|| "Settings".into());
            if button(ui, m, &label, 200.0, false, v.rows.is_empty()) {
                out.options = true;
            }
        }
        if !v.help.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&v.help).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
}

/// The windows over the list: chfwin and chswin beside it, the prompt, the ok window and the rename field under it.
fn windows(p: &mut Panels, gu: &GameUi, v: &View, out: &mut Pointer) {
    let m = p.m;
    let screen = p.ctx.content_rect();
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let side = pos2(screen.width() * 0.0121 + W + 56.0, screen.height() * 0.06);
    // A column of buttons (chfwin, chswin): the cursor's lit; (hovered, clicked) child.
    let column = |ui: &mut Ui, rows: &[String], cur: u16, out: &mut Pointer| {
        for (k, t) in rows.iter().enumerate() {
            let child = k as u16 + 1;
            let (r, resp) = ui.allocate_exact_size(vec2(220.0, 28.0), Sense::click());
            let face = if child == cur { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, t, FontId::proportional(m.theme.small), m.theme.color("text"));
            if resp.hovered() && moved {
                out.child.0 = Some(child);
            }
            if resp.clicked() {
                out.child.1 = Some(child);
            }
            ui.add_space(2.0);
        }
    };
    if let Some((rows, cur)) = &v.options {
        let title = gu.focus_text("menu    chfwin", 0)[1].clone().unwrap_or_else(|| "Settings".into());
        let active = v.slots.is_none() && v.prompt.is_none() && v.ok.is_none();
        let mut got = Pointer::default();
        p.panel_at("select options", Some(Title { icon: None, text: &title, note: None }), 264.0, side, |ui| column(ui, rows, *cur, &mut got));
        if active {
            out.child = got.child;
        }
    }
    if let Some((rows, cur)) = &v.slots {
        let at = side + vec2(0.0, 130.0);
        let active = v.prompt.is_none() && v.ok.is_none();
        let mut got = Pointer::default();
        p.panel_at("select slots", None, 264.0, at, |ui| column(ui, rows, *cur, &mut got));
        if active {
            out.child = got.child;
        }
    }
    let low = pos2(screen.center().x - 230.0, screen.height() * 0.62);
    if let Some((text, yes)) = &v.prompt {
        let caption = |k: u16, d: &str| gu.child_caption("menu    ptc6yesn", k).filter(|c| !c.is_empty()).unwrap_or_else(|| d.to_string());
        p.panel_at("select prompt", None, 460.0, low, |ui| {
            ui.label(egui::RichText::new(text).size(m.theme.ui).color(m.theme.color("text")));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (child, d) in [(1u16, "Yes"), (2, "No")] {
                    let (r, resp) = ui.allocate_exact_size(vec2(130.0, 30.0), Sense::click());
                    let on = (child == 1) == *yes;
                    let face = if on { "controls/button_selected" } else if resp.hovered() { "controls/button_hover" } else { "controls/button_normal" };
                    if let Some(s) = m.art.sprite(face) {
                        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, caption(child, d), FontId::proportional(m.theme.ui), if on { m.theme.color("gold") } else { m.theme.color("text") });
                    if resp.hovered() && moved {
                        out.child.0 = Some(child);
                    }
                    if resp.clicked() {
                        out.child.1 = Some(child);
                    }
                    ui.add_space(8.0);
                }
            });
        });
    }
    if let Some(text) = &v.ok {
        p.panel_at("select ok", None, 460.0, low, |ui| {
            ui.label(egui::RichText::new(text).size(m.theme.ui).color(m.theme.color("text")));
            ui.add_space(10.0);
            if button(ui, m, "OK", 130.0, true, false) {
                out.child.1 = Some(1);
            }
        });
    }
    if let Some(name) = &v.rename {
        let blink = (p.ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
        p.panel_at("select rename", None, 460.0, low, |ui| {
            ui.label(egui::RichText::new(&v.help).size(m.theme.small).color(m.theme.color("muted")));
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(400.0, 30.0), Sense::hover());
            if let Some(s) = m.art.sprite("controls/list_row_selected") {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            let shown = if blink { format!("{name}|") } else { name.clone() };
            ui.painter().text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, shown, FontId::proportional(m.theme.ui), m.theme.color("text"));
        });
    }
}
