//! The modern lobby screens over the retail ones (client/select.rs keeps the select screen's rules and keys): the
//! character list with each character's job, level, race and area, the cursor's details, the counts, the settings
//! windows (chfwin, chswin) and the prompts (ptc6yesn, ok, the rename field) as modern panels; the creator's steps
//! (client/creator.rs and client/mod.rs keep its rules and keys) as one panel with the picks and their stats, its
//! confirm and ok windows as the select screen's. The 3D scenes stay.

use super::super::creator::{self, Page};
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
            // The label sized to the button (retail's captions fill about half its height).
            p.text(rect.center(), Align2::CENTER_CENTER, *label, FontId::new((rect.height() * 0.5).max(self.theme.ui), FontFamily::Name(super::STRONG.into())), c);
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

    /// The creator: what the mouse did, for client/mod.rs's creator step.
    pub fn creator_screen(&mut self, ctx: &Context, gu: &GameUi, window: [u32; 2], scale: u32, c: &Page) -> creator::Pointer {
        self.zoom_to(ctx, window, scale);
        let mut layout = Default::default();
        let (mut changed, mut font) = (false, 15.0);
        let focus = if c.ok.is_some() || c.confirm.is_some() { "creator prompt" } else { "creator" };
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout: &mut layout, changed: &mut changed, chat_font: &mut font, battle_in_all: &mut true, folded: &mut Default::default(), scan_kinds: &mut 7, this: Default::default(), retail: Default::default(), focus: Some(focus) };
        let mut out = creator::Pointer::default();
        creator_panel(&mut p, gu, c, &mut out);
        creator_prompts(&mut p, gu, c, &mut out);
        if !c.free {
            out = creator::Pointer::default();
        }
        out
    }
}

/// A list row in the pack's art, lit on the cursor; its response.
fn pick_row(ui: &mut Ui, m: &Modern, text: &str, on: bool, live: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(W - 14.0, 28.0), if live { Sense::click() } else { Sense::hover() });
    let face = match () {
        _ if on => "controls/list_row_selected",
        _ if live && resp.hovered() => "controls/list_row_hover",
        _ => "controls/list_row_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, rect, (rect.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    let c = if on { m.theme.color("gold") } else { m.theme.color("text") };
    ui.painter().text(pos2(rect.min.x + 24.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), c);
    ui.add_space(2.0);
    resp
}

/// A step's choices: the race's name (group 6), Face / Hair as the stats line words, the size, job and nation rows'
/// own captions.
fn step_rows(gu: &GameUi, step: usize) -> Vec<String> {
    let n = creator::STEPS.get(step).map_or(0, |s| s.1);
    let caption = |k: usize| gu.child_caption(creator::STEPS[step].0, k as u16 + 1).filter(|c| !c.is_empty());
    (0..n)
        .map(|k| match step {
            0 => gu.lobby_msg(6, k, ""),
            1 => gu.creator_look_words(k, 0, 0)[0].clone(),
            2 => gu.creator_look_words(0, k, 0)[1].clone(),
            3 => caption(k).unwrap_or_else(|| gu.creator_look_words(0, 0, k)[2].clone()),
            6 => caption(k).unwrap_or_else(|| gu.nation_words(k).0),
            _ => caption(k).unwrap_or_default(),
        })
        .collect()
}

/// A text field: the text (or `empty`, dimmed) and the caret at char `caret` while `caret` is Some and blinking on.
fn field(ui: &mut Ui, m: &Modern, text: &str, empty: &str, caret: Option<usize>) {
    let (r, _) = ui.allocate_exact_size(vec2(W - 14.0, 30.0), Sense::hover());
    let face = if caret.is_some() { "controls/input_focused" } else { "controls/input_normal" };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    let font = FontId::proportional(m.theme.ui);
    let at = pos2(r.min.x + 12.0, r.center().y);
    if text.is_empty() {
        ui.painter().text(at, Align2::LEFT_CENTER, empty, font.clone(), m.theme.color("muted"));
    } else {
        ui.painter().text(at, Align2::LEFT_CENTER, text, font.clone(), m.theme.color("text"));
    }
    let blink = (ui.ctx().input(|i| i.time) * 2.0) as i64 % 2 == 0;
    if let Some(k) = caret.filter(|_| blink) {
        let head: String = text.chars().take(k).collect();
        let x = at.x + ui.painter().layout_no_wrap(head, font, Color32::WHITE).size().x;
        ui.painter().line_segment([pos2(x, r.min.y + 7.0), pos2(x, r.max.y - 7.0)], egui::Stroke::new(1.5, m.theme.color("text")));
    }
    ui.ctx().request_repaint();
}

/// The creator's panel: the help line, the step's rows (or the name, the world list, the pass), the picks' stats up to
/// the name, the nation's description on its step.
fn creator_panel(p: &mut Panels, gu: &GameUi, c: &Page, out: &mut creator::Pointer) {
    let m = p.m;
    let v = c.view;
    let screen = p.ctx.content_rect();
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let live = c.free && c.ok.is_none() && c.confirm.is_none();
    let at = pos2(screen.width() * 0.0121, screen.height() * 0.06);
    let note = Some(format!("{}/7", v.step + 1));
    // From the confirm on, retail's step windows are closed (the emblem and the prompt stay).
    if v.step >= 7 {
        return;
    }
    let sel = match v.step {
        0 => v.race,
        1 => v.face,
        2 => v.hair,
        3 => v.size,
        4 => v.job,
        6 => v.nation,
        _ => 0,
    };
    let small = |t: &str, colour: Color32| egui::RichText::new(t).size(m.theme.small).color(colour);
    p.panel_at("creator", Some(Title { icon: Some("icons/party"), text: "Create Character", note: note.as_deref() }), W + 44.0, at, |ui| {
        ui.label(small(&c.help, if c.status { m.theme.color("gold") } else { m.theme.color("muted") }));
        ui.add_space(8.0);
        if v.step != 5 {
            for (k, t) in step_rows(gu, v.step).iter().enumerate() {
                let resp = pick_row(ui, m, t, k == sel, live);
                if live && resp.hovered() && moved {
                    out.row.0 = Some(k);
                }
                if live && resp.clicked() {
                    out.row.1 = Some(k);
                }
            }
        }
        if v.step == 5 && !v.name_hidden {
            let typing = v.world.is_none();
            field(ui, m, v.name, "", typing.then_some(v.name.chars().count()));
            if typing {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if button(ui, m, "Random", 140.0, false, !live) {
                        out.random = true;
                    }
                    if button(ui, m, "OK", 140.0, true, !live || v.name.is_empty()) {
                        out.enter = true;
                    }
                });
            }
            if let Some(w) = &v.world {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(gu.lobby_msg(1, 0, "World")).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
                ui.add_space(4.0);
                match w.pass {
                    None => {
                        egui::ScrollArea::vertical().id_salt("creator worlds").max_height(screen.height() * 0.4).auto_shrink([false, true]).show(ui, |ui| {
                            for (k, t) in w.rows.iter().enumerate() {
                                let resp = pick_row(ui, m, t, k == w.cursor, live);
                                if live && resp.hovered() && moved {
                                    out.world.0 = Some(k);
                                }
                                if live && resp.clicked() {
                                    out.world.1 = Some(k);
                                }
                            }
                        });
                    }
                    Some((pass, caret, _)) => {
                        ui.label(small(w.rows.get(w.cursor).map_or("", |s| s.as_str()), m.theme.color("text")));
                        ui.add_space(4.0);
                        field(ui, m, pass, &gu.lobby_msg(8, 0x3e, ""), Some(caret));
                        ui.add_space(6.0);
                        if button(ui, m, "OK", 140.0, true, !live) {
                            out.enter = true;
                        }
                    }
                }
            }
        }
        if v.step <= 5 {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(gu.lobby_msg(6, v.race, "")).size(m.theme.ui).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
            ui.add_space(4.0);
            let stats = gu.creator_stat_values(v.race, v.job);
            let (r, _) = ui.allocate_exact_size(vec2(W - 14.0, 4.0 * 18.0), Sense::hover());
            for (k, n) in stats.iter().enumerate() {
                let x = r.min.x + 8.0 + (k % 2) as f32 * (W / 2.0);
                let y = r.min.y + 9.0 + (k / 2) as f32 * 18.0;
                ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, ["STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR"][k], FontId::proportional(m.theme.small), m.theme.color("muted"));
                ui.painter().text(pos2(x + 120.0, y), Align2::RIGHT_CENTER, n.to_string(), FontId::proportional(m.theme.small), m.theme.color("text"));
            }
            ui.add_space(4.0);
            ui.label(small(&gu.creator_look_words(v.face, v.hair, v.size).join("    "), m.theme.color("muted")));
        }
        if v.step == 6 {
            let (title, body) = gu.nation_words(v.nation);
            ui.add_space(10.0);
            ui.label(egui::RichText::new(title).size(m.theme.ui).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
            ui.add_space(4.0);
            ui.label(small(&body, m.theme.color("text")));
        }
    });
}

/// The confirm (ptc6yesn, Yes / No) and the ok window, under the middle as the select screen's.
fn creator_prompts(p: &mut Panels, gu: &GameUi, c: &Page, out: &mut creator::Pointer) {
    let m = p.m;
    let screen = p.ctx.content_rect();
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let low = pos2(screen.center().x - 230.0, screen.height() * 0.62);
    if let Some(text) = &c.ok {
        p.panel_at("creator prompt", None, 460.0, low, |ui| {
            ui.label(egui::RichText::new(text).size(m.theme.ui).color(m.theme.color("text")));
            ui.add_space(10.0);
            if button(ui, m, "OK", 130.0, true, false) {
                out.ok = true;
            }
        });
    } else if let Some((text, yes)) = &c.confirm {
        let caption = |k: u16, d: &str| gu.child_caption("menu    ptc6yesn", k).filter(|c| !c.is_empty()).unwrap_or_else(|| d.to_string());
        p.panel_at("creator prompt", None, 460.0, low, |ui| {
            ui.label(egui::RichText::new(text).size(m.theme.ui).color(m.theme.color("text")));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (child, d) in [(true, "Yes"), (false, "No")] {
                    let (r, resp) = ui.allocate_exact_size(vec2(130.0, 30.0), Sense::click());
                    let on = child == *yes;
                    let face = if on { "controls/button_selected" } else if resp.hovered() { "controls/button_hover" } else { "controls/button_normal" };
                    if let Some(s) = m.art.sprite(face) {
                        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                    }
                    let label = caption(if child { 1 } else { 2 }, d);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(m.theme.ui), if on { m.theme.color("gold") } else { m.theme.color("text") });
                    if resp.hovered() && moved {
                        out.yes.0 = Some(child);
                    }
                    if resp.clicked() {
                        out.yes.1 = Some(child);
                    }
                    ui.add_space(8.0);
                }
            });
        });
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
