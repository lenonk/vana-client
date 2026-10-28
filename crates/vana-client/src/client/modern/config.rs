//! The modern Config page in the command window, over client/configui.rs (its values, setters and windows): retail's
//! Config list on the left without the Windows page (the log windows' frame, size and width mean nothing with the modern
//! chat; user, 2026-10-26) and with Misc. 1..4 as one page (the numbered entries merged); the chosen page on the right,
//! a row per setting with its options as buttons or a slider, named with retail's own captions read from the page's
//! layout (a few made clearer), its help line under its heading. Chat Filters, Effects and the first log window's
//! categories are lists of rows with retail's toggles, Font Colors each colour's sample with R / G / B sliders;
//! key assignment still opens retail's window over it. Extras and Settings (ours, client/settings.rs) are pages here too.

use super::super::World;
use super::super::configui::{self, Act, ConfigUi, Ctl};
use super::super::configui2;
use super::super::settings;
use super::super::game_ui::GameUi;
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, Vec2, pos2, vec2};

/// What a list entry opens.
#[derive(Clone, Copy)]
enum Entry {
    /// Drawn here from these retail pages' controls.
    Native(&'static [&'static str]),
    /// A list page (configui2's rows and toggles), with the menu whose rows name its categories.
    List(&'static str, Option<&'static str>),
    /// Font Colors: the categories (conftxtc's rows) of colours.
    Colors,
    Extras,
    Settings,
}

/// No Log page: it sends lines to the first or the second log window, and with one chat window (and in retail with
/// the second off) everything shows.
const ENTRIES: [(&str, Entry); 9] = [
    ("Gameplay", Entry::Native(&["conf2win"])),
    ("Chat Filters", Entry::List("cfilter", None)),
    ("Font Colors", Entry::Colors),
    ("Misc.", Entry::Native(&["conf3win", "conf6win", "conf12wi", "conf13wi"])),
    ("Effects", Entry::List("fxfilter", None)),
    ("Mouse/Cam.", Entry::Native(&["conf7"])),
    ("Global", Entry::Native(&["conf4"])),
    ("Extras", Entry::Extras),
    ("Settings", Entry::Settings),
];

/// A setting: its page and controls (its options, one slider, or one button).
struct Shape {
    menu: &'static str,
    ctls: Vec<(u16, Ctl)>,
}

/// The settings of the pages, in order: the options of one config id (or bit) together.
fn shapes(menus: &[&'static str]) -> Vec<Shape> {
    let mut out: Vec<Shape> = Vec::new();
    for &menu in menus {
        let mut last: Option<u64> = None;
        for &(child, ctl) in configui::page(menu).iter().filter(|(c, _)| !left_out(menu, *c)) {
            let key = match ctl {
                Ctl::Pick(id, ..) => Some(id as u64),
                Ctl::Bit(id, mask, ..) => Some((id as u64) << 32 | mask as u64),
                Ctl::Act(Act::Anon(_)) => Some(u64::MAX),
                _ => None,
            };
            match out.last_mut() {
                Some(s) if key.is_some() && key == last && s.menu == menu => s.ctls.push((child, ctl)),
                _ => out.push(Shape { menu, ctls: vec![(child, ctl)] }),
            }
            last = key;
        }
    }
    out
}

fn full(menu: &str) -> String {
    format!("menu    {menu}")
}

/// Headings retail's captions leave unclear away from its layout: (page, first control, heading, help or "" for the
/// page's own).
const HEADINGS: [(&str, u16, &str, &str); 9] = [
    ("conf2win", 5, "Gamma Adjustment: Red", ""),
    ("conf2win", 6, "Gamma Adjustment: Green", ""),
    ("conf2win", 7, "Gamma Adjustment: Blue", ""),
    ("conf2win", 8, "Inventory: Auto-sort", ""),
    ("conf2win", 10, "Inventory: Sort Type", "Type 1 sorts by category. Type 2 keeps the order and gathers copies of an item beside the first."),
    ("conf7", 7, "Camera Behind You: Up / Down", "Inverts the up / down turn of the camera behind you (the mouse and the camera keys)."),
    ("conf7", 9, "Camera Behind You: Left / Right", "Inverts the left / right turn of the camera behind you (the mouse and the camera keys)."),
    ("conf7", 11, "First Person: Up / Down", "Inverts looking up and down in first person (the mouse and the camera keys)."),
    ("conf7", 13, "First Person: Left / Right", "Inverts looking left and right in first person (the mouse and the camera keys)."),
];

/// Settings left out here: Mouse/Cam.'s Mouse Control and Screen Edge Panning (the mouse acts as modern MMOs').
fn left_out(menu: &str, child: u16) -> bool {
    menu == "conf7" && matches!(child, 1..=4)
}

/// A setting's heading: ours where retail's is unclear, else the page's own caption on its first control's line to its
/// left, else the nearest above it (not the page's title band); else the control's help title.
fn heading(gu: &GameUi, s: &Shape) -> String {
    if let Some(h) = override_of(s) {
        return h.2.to_string();
    }
    let name = full(s.menu);
    let Some((cx, cy)) = s.ctls.first().and_then(|c| gu.child_place(&name, c.0)) else { return String::new() };
    // A slider's ends ("Min", "Max") are not headings; a slider's heading is above it.
    let heads: Vec<_> = gu.window_captions(&name).into_iter().filter(|(t, _, _)| !matches!(t.as_str(), "Min" | "Max")).collect();
    let slider = matches!(s.ctls[0].1, Ctl::Slider(..));
    let left = heads.iter().filter(|(_, x, y)| !slider && (*y - cy).abs() <= 6.0 && *x < cx).max_by(|a, b| a.1.total_cmp(&b.1));
    let above = || heads.iter().filter(|(_, x, y)| *y > 12.0 && *y < cy && cy - *y < 34.0 && *x <= cx + 40.0).max_by(|a, b| a.2.total_cmp(&b.2));
    left.or_else(above).map(|h| h.0.clone()).or_else(|| gu.focus_text(&name, s.ctls[0].0)[1].clone()).unwrap_or_default()
}

fn override_of(s: &Shape) -> Option<&'static (&'static str, u16, &'static str, &'static str)> {
    s.ctls.first().and_then(|c| HEADINGS.iter().find(|h| h.0 == s.menu && h.1 == c.0))
}

/// Whether an option holds now.
fn holds(c: &ConfigUi, v: &crate::gamecfg::Values, ctl: Ctl) -> bool {
    match ctl {
        Ctl::Pick(id, x, _) => v.get(id) == x,
        Ctl::Bit(id, mask, on, _) => (v.get(id) & mask != 0) == on,
        Ctl::Act(Act::Anon(h)) => c.anon == h,
        _ => false,
    }
}

/// The page shows (the list's window, configwi, on top).
fn shown(w: &World) -> bool {
    w.config.as_ref().is_some_and(|c| c.top().is_some_and(|t| t.name == "configwi") && !c.covered)
}

/// A click on what the mouse took.
enum Click {
    Entry(usize),
    Apply(&'static str, u16),
    Slide(u16, u32),
    Cat(usize),
    Toggle(&'static str, usize),
    Colour(u16, [u8; 3]),
}

/// A list page's categories: the menu's rows' captions.
fn categories(gu: &GameUi, menu: &str) -> Vec<String> {
    (1..=4u16).map(|k| gu.child_caption(&full(menu), k).unwrap_or_else(|| format!("{k}"))).collect()
}

/// A colour's channels from its config value (0x80RRGGBB).
fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// A cfilter / fxfilter / log state sprite's word.
fn state_word(m: &super::Modern, gu: &GameUi, state: usize) -> String {
    m.caption(gu, "menu    framesus", state).unwrap_or_else(|| {
        match state {
            88 => "ON",
            83 | 137 => "Hold",
            _ => "OFF",
        }
        .into()
    })
}

/// The category strip: a tab per category, the chosen one open (its cursor marked while it has the keys).
fn cat_strip(ui: &mut Ui, m: &super::Modern, cats: &[String], cat: usize, focus: bool, click: &mut Option<Click>) {
    let labels: Vec<&str> = cats.iter().map(String::as_str).collect();
    if let Some(k) = super::tabs::strip(ui, m, "config cats", &labels, Some(cat), focus.then_some(cat)).iter().position(|r| r.clicked()) {
        *click = Some(Click::Cat(k));
    }
    ui.add_space(8.0);
}

/// A list page's rows: the label, its state as a button that steps it as Enter does; the focused row ringed.
#[allow(clippy::too_many_arguments)]
fn list_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, page: &'static str, rows: &[(u16, usize)], focus: Option<usize>, follow: bool, click: &mut Option<Click>) {
    for (r, &(label, state)) in rows.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        if focus == Some(r) {
            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(1.0, m.theme.color("focus").gamma_multiply(0.6)), egui::StrokeKind::Inside);
            if follow {
                ui.scroll_to_rect(rect, None);
            }
        }
        let text = gu.group_b(label as usize).unwrap_or_default();
        ui.painter().text(pos2(rect.min.x + 8.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), m.theme.color("text"));
        let word = state_word(m, gu, state);
        let b = Rect::from_min_size(pos2(rect.max.x - 112.0, rect.min.y + 2.0), vec2(104.0, 26.0));
        let resp = ui.interact(b, egui::Id::unique(("config row", page, r)), Sense::click());
        let face = if state == 88 { "controls/button_selected" } else if resp.hovered() { "controls/button_hover" } else { "controls/button_normal" };
        if let Some(s) = m.art.sprite(face) {
            ui.painter().add(paint::nine_slice(&s, b, 0.5, Color32::WHITE));
        }
        let c = match state {
            88 => m.theme.color("gold"),
            83 | 137 => m.theme.color("focus"),
            _ => m.theme.color("muted"),
        };
        ui.painter().text(b.center(), Align2::CENTER_CENTER, word, FontId::proportional(m.theme.small), c);
        if matches!(state, 136 | 137)
            && let Some(s) = m.art.sprite("icons/chat")
        {
            ui.painter().add(paint::image(&s, pos2(b.min.x - 24.0, b.center().y - 9.0), 18.0, Color32::WHITE));
        }
        if resp.clicked() {
            *click = Some(Click::Toggle(page, r));
        }
    }
}

/// Font Colors' rows: the label and its sample line in the colour, R / G / B sliders under them (0..255); the focused
/// slider ringed.
#[allow(clippy::too_many_arguments)]
fn colour_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, v: &crate::gamecfg::Values, rows: &[(u16, u16, u16)], focus: Option<usize>, follow: bool, click: &mut Option<Click>) {
    for (i, &(label, sample, id)) in rows.iter().enumerate() {
        let value = v.get(id);
        let col = super::chat::colour(crate::dat::battle::argb_bytes(value));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(gu.group_b(label as usize).unwrap_or_default()).size(m.theme.small).color(m.theme.color("text")));
            ui.add_space(10.0);
            ui.label(egui::RichText::new(gu.group_b(sample as usize).unwrap_or_default()).size(m.theme.small).color(col));
        });
        let c = rgb(value);
        for (ch, name) in ["R", "G", "B"].into_iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 50.0, 18.0), Sense::click_and_drag());
            let k = i * 3 + ch;
            if focus == Some(k) {
                ui.painter().rect_stroke(r.expand(2.0), 4.0, egui::Stroke::new(1.0, m.theme.color("focus").gamma_multiply(0.7)), egui::StrokeKind::Outside);
                if follow {
                    ui.scroll_to_rect(r, None);
                }
            }
            let f = c[ch] as f32 / 255.0;
            let bar = Rect::from_min_max(pos2(r.min.x + 20.0, r.min.y + 5.0), pos2(r.max.x, r.max.y - 5.0));
            ui.painter().text(pos2(r.min.x + 2.0, r.center().y), Align2::LEFT_CENTER, name, FontId::monospace(m.theme.small - 2.0), m.theme.color("muted"));
            ui.painter().rect_filled(bar, 4.0, m.theme.color("charcoal"));
            let tint = [Color32::from_rgb(200, 70, 70), Color32::from_rgb(80, 180, 90), Color32::from_rgb(80, 120, 220)][ch];
            ui.painter().rect_filled(Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f, bar.max.y)), 4.0, tint);
            ui.painter().circle(pos2(bar.min.x + bar.width() * f, bar.center().y), 6.0, m.theme.color("gold"), egui::Stroke::new(1.0, Color32::from_black_alpha(200)));
            ui.painter().text(pos2(r.max.x + 10.0, r.center().y), Align2::LEFT_CENTER, c[ch].to_string(), FontId::monospace(m.theme.small - 1.0), m.theme.color("text"));
            if (resp.clicked() || resp.dragged())
                && let Some(at) = resp.interact_pointer_pos()
            {
                let x = (((at.x - bar.min.x) / bar.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
                if x != c[ch] {
                    let mut n = c;
                    n[ch] = x;
                    *click = Some(Click::Colour(id, n));
                }
            }
        }
        ui.add_space(8.0);
    }
}

/// What the mouse did on our Settings / Extras page.
enum Do {
    Tab(usize),
    Point(usize, usize),
    Click(usize, usize),
    Slide(usize, usize),
}

/// Our Settings and Extras pages (settings.rs keeps their rules and keys): the page strip, each setting's heading, help
/// and buttons or slider, the cursor's ringed while `open` (the page has the keys).
fn settings_page(ui: &mut Ui, m: &super::Modern, v: &settings::View, open: bool, width: f32, act: &mut Vec<Do>) {
    let moved = ui.ctx().input(|i| i.pointer.delta() != Vec2::ZERO);
    // The pages as tabs, wrapping into rows where they must; the cursor on the open one while row 0 has it.
    if !v.tabs.is_empty() {
        ui.set_max_width(width);
        if let Some(k) = super::tabs::strip(ui, m, "settings tabs", v.tabs, Some(v.tab), (open && v.row == 0).then_some(v.tab)).iter().position(|r| r.clicked()) {
            act.push(Do::Tab(k));
        }
        ui.add_space(8.0);
    }
    for (k, r) in v.rows.iter().enumerate() {
        let row = k + 1;
        let focus = open && v.row == row;
        let top = ui.cursor().min;
        ui.label(egui::RichText::new(&r.label).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(if focus { m.theme.color("gold") } else { m.theme.color("muted") }));
        ui.label(egui::RichText::new(&r.help).size(m.theme.small - 1.5).color(m.theme.color("muted")));
        ui.add_space(2.0);
        match &r.ctl {
            settings::VCtl::Buttons(bs) => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for (c, (label, held)) in bs.iter().enumerate() {
                        let resp = button(ui, m, label, *held, false, focus && v.col == c);
                        if resp.hovered() && moved {
                            act.push(Do::Point(row, c));
                        }
                        if resp.clicked() {
                            act.push(Do::Click(row, c));
                        }
                    }
                });
            }
            settings::VCtl::Slider { n, at, shown } => {
                let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 110.0, 18.0), Sense::click_and_drag());
                let f = if *n > 1 { *at as f32 / (*n - 1) as f32 } else { 1.0 };
                let track = rr.shrink2(vec2(0.0, 5.0));
                ui.painter().rect_filled(track, 4.0, m.theme.color("charcoal"));
                ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(rr.min.x + rr.width() * f, track.max.y)), 4.0, m.theme.color("bronze"));
                let knob = if focus && v.edit { m.theme.color("focus") } else { m.theme.color("gold") };
                ui.painter().circle(pos2(rr.min.x + rr.width() * f, rr.center().y), 7.0, knob, egui::Stroke::new(1.0, Color32::from_black_alpha(200)));
                ui.painter().text(pos2(rr.max.x + 12.0, rr.center().y), Align2::LEFT_CENTER, shown, FontId::proportional(m.theme.small), m.theme.color("text"));
                if resp.hovered() && moved {
                    act.push(Do::Point(row, 0));
                }
                if (resp.clicked() || resp.dragged())
                    && let Some(at_p) = resp.interact_pointer_pos()
                {
                    let t = ((at_p.x - rr.min.x) / rr.width()).clamp(0.0, 1.0);
                    act.push(Do::Slide(row, (t * (*n - 1).max(0) as f32).round() as usize));
                }
            }
        }
        if focus {
            let r = Rect::from_min_max(top - vec2(6.0, 2.0), pos2(top.x + ui.available_width() + 6.0, ui.cursor().min.y + 2.0));
            ui.painter().rect_stroke(r, 6.0, egui::Stroke::new(1.0, m.theme.color("focus").gamma_multiply(0.6)), egui::StrokeKind::Outside);
        }
        ui.add_space(8.0);
    }
}

/// The mouse's actions on the Settings / Extras page; true when one was taken.
fn settings_act(w: &mut World, act: Vec<Do>) -> bool {
    let Some(s) = w.settings.as_mut() else { return false };
    let any = !act.is_empty();
    for a in act {
        match a {
            Do::Tab(k) => s.m_tab(k),
            Do::Point(r, c) => {
                if s.m_point(r, c) {
                    w.sounds.push(Se::Cursor as u32);
                }
            }
            Do::Click(r, c) => s.m_click(r, c),
            Do::Slide(r, i) => s.m_slide(r, i),
        }
        w.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
    }
    any
}

/// Our page kind for a Config list entry.
fn settings_kind(k: usize) -> Option<settings::Kind> {
    match ENTRIES.get(k)?.1 {
        Entry::Extras => Some(settings::Kind::Extras),
        Entry::Settings => Some(settings::Kind::Settings),
        _ => None,
    }
}

/// Our Settings / Extras state follows the list's entry: asked for (the app makes it) while one is chosen, dropped
/// once another is and nothing is left to store.
fn settings_follow(w: &mut World, entry: usize) {
    match settings_kind(entry) {
        Some(kind) if w.settings.as_ref().map(|s| s.kind) != Some(kind) => w.want_settings = Some(kind),
        Some(_) => {}
        None if w.settings.as_ref().is_some_and(|s| s.in_game && !s.dirty) => w.settings = None,
        None => {}
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    // Ours goes with the Config window, once the app has stored what changed.
    if w.modern && w.config.is_none() && w.settings.as_ref().is_some_and(|s| s.in_game && !s.dirty) {
        w.settings = None;
    }
    let Some(gu) = p.gu else { return };
    if !w.modern || !shown(w) {
        return;
    }
    let Some(entry) = w.config.as_ref().map(|c| c.m_entry.min(ENTRIES.len() - 1)) else { return };
    settings_follow(w, entry);
    let Some(c) = w.config.as_ref() else { return };
    let m = p.m;
    let sw = w.ui_wh.0;
    let (open, row) = (c.m_open, c.m_row);
    // Our Settings / Extras page, when that entry is chosen and its state is up.
    let ours = w.settings.as_ref().filter(|s| settings_kind(entry) == Some(s.kind)).map(|s| s.view());
    let mut sact: Vec<Do> = Vec::new();
    let shapes = match ENTRIES[entry].1 {
        Entry::Native(menus) => shapes(menus),
        _ => Vec::new(),
    };
    let heads: Vec<String> = shapes.iter().map(|s| heading(gu, s)).collect();
    let labels: Vec<Vec<String>> = shapes.iter().map(|s| s.ctls.iter().map(|(k, ctl)| gu.child_caption(&full(s.menu), *k).unwrap_or_else(|| fallback(*ctl))).collect()).collect();
    let v = &w.game;
    // Each option's own help (retail shows the cursor child's); a setting's line is ours where we gave one, else the
    // help of the option that holds (the first for a slider or none).
    let opt_helps: Vec<Vec<String>> = shapes.iter().map(|s| s.ctls.iter().map(|(k, _)| gu.focus_text(&full(s.menu), *k)[0].clone().unwrap_or_default()).collect()).collect();
    let helps: Vec<String> = shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let held = s.ctls.iter().position(|(_, ctl)| holds(c, v, *ctl)).unwrap_or(0);
            override_of(s).map(|h| h.3).filter(|h| !h.is_empty()).map(str::to_string).unwrap_or_else(|| opt_helps[i].get(held).cloned().unwrap_or_default())
        })
        .collect();
    let mut hover_opt: Option<(usize, usize)> = None;
    let cat = c.m_cat.min(3);
    // The list pages' rows (row 0 is the category strip where there is one); the focused row kept in view as it moves.
    let (list, cats): (Vec<(u16, usize)>, Vec<String>) = match ENTRIES[entry].1 {
        Entry::List(page, menu) => (configui2::modern_rows(v, page, cat), menu.map_or(Vec::new(), |m| categories(gu, m))),
        Entry::Colors => (Vec::new(), categories(gu, "conftxtc")),
        _ => (Vec::new(), Vec::new()),
    };
    let follow = m.config_seen.get() != (entry, row);
    m.config_seen.set((entry, row));
    let mut click: Option<Click> = None;
    let mut hover_help: Option<usize> = None;
    p.panel("window", Some(Title { icon: Some("icons/config"), text: "Config", note: None }), 640.0, |ui| {
        ui.horizontal_top(|ui| {
            // The list.
            ui.vertical(|ui| {
                ui.set_width(150.0);
                for (k, (label, _)) in ENTRIES.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::click());
                    let face = match () {
                        _ if k == entry && !open => "controls/list_row_selected",
                        _ if k == entry || resp.hovered() => "controls/list_row_hover",
                        _ => "controls/list_row_normal",
                    };
                    super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                    ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(m.theme.ui), m.theme.color("text"));
                    if resp.clicked() {
                        click = Some(Click::Entry(k));
                    }
                    ui.add_space(2.0);
                }
            });
            ui.add_space(16.0);
            // The page, scrolled when longer than most of the screen.
            let tall = ui.ctx().content_rect().height() * 0.72;
            egui::ScrollArea::vertical().max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(440.0);
                let first = !cats.is_empty() as usize;
                match ENTRIES[entry].1 {
                    Entry::List(page, _) => {
                        // The window's own help (its first element's).
                        if let Some(h) = gu.focus_text(&full(page), u16::MAX)[0].as_ref() {
                            ui.label(egui::RichText::new(h).size(m.theme.small - 1.5).color(m.theme.color("muted")));
                            ui.add_space(6.0);
                        }
                        if !cats.is_empty() {
                            cat_strip(ui, m, &cats, cat, open && row == 0, &mut click);
                        }
                        list_rows(ui, m, gu, page, &list, (open && row >= first).then(|| row - first), follow, &mut click);
                        return;
                    }
                    Entry::Colors => {
                        cat_strip(ui, m, &cats, cat, open && row == 0, &mut click);
                        colour_rows(ui, m, gu, v, configui2::colour_rows(cat), (open && row >= 1).then(|| row - 1), follow, &mut click);
                        return;
                    }
                    _ => {}
                }
                if let Some(v) = &ours {
                    settings_page(ui, m, v, open, 440.0, &mut sact);
                    return;
                }
                if shapes.is_empty() {
                    let what = match ENTRIES[entry].1 {
                        Entry::Extras => "The gameplay additions (not retail): the map overlay, the synthesis amount, timestamps, timers, the minimap.",
                        _ => "Window, resolution, frame limit and the other settings of the Windows Config tool.",
                    };
                    ui.label(egui::RichText::new(what).size(m.theme.small).color(m.theme.color("muted")));
                    return;
                }
                for (i, s) in shapes.iter().enumerate() {
                    let focus = open && i == row;
                    let top = ui.cursor().min;
                    ui.label(egui::RichText::new(&heads[i]).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(if focus { m.theme.color("gold") } else { m.theme.color("muted") }));
                    // What it does, under its name.
                    if !helps[i].is_empty() {
                        ui.label(egui::RichText::new(&helps[i]).size(m.theme.small - 1.5).color(m.theme.color("muted")));
                        ui.add_space(2.0);
                    }
                    match s.ctls[0].1 {
                        Ctl::Slider(id, lo, hi, step) => {
                            let val = v.get_i(id).clamp(lo, hi);
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 60.0, 18.0), Sense::click_and_drag());
                            let f = (val - lo) as f32 / (hi - lo).max(1) as f32;
                            ui.painter().rect_filled(r.shrink2(vec2(0.0, 5.0)), 4.0, m.theme.color("charcoal"));
                            ui.painter().rect_filled(Rect::from_min_max(r.shrink2(vec2(0.0, 5.0)).min, pos2(r.min.x + r.width() * f, r.max.y - 5.0)), 4.0, m.theme.color("bronze"));
                            ui.painter().circle(pos2(r.min.x + r.width() * f, r.center().y), 7.0, m.theme.color("gold"), egui::Stroke::new(1.0, Color32::from_black_alpha(200)));
                            ui.painter().text(pos2(r.max.x + 12.0, r.center().y), Align2::LEFT_CENTER, val.to_string(), FontId::monospace(m.theme.small), m.theme.color("text"));
                            if (resp.clicked() || resp.dragged())
                                && let Some(at) = resp.interact_pointer_pos()
                            {
                                let t = ((at.x - r.min.x) / r.width()).clamp(0.0, 1.0);
                                let x = lo + ((t * (hi - lo) as f32 / step as f32).round() as i32) * step;
                                if x != val {
                                    click = Some(Click::Slide(id, x as u32));
                                }
                            }
                            if resp.hovered() {
                                hover_help = Some(i);
                            }
                        }
                        _ => {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                                for (j, (child, ctl)) in s.ctls.iter().enumerate() {
                                    let off = configui::disabled(s.menu, *child, sw, c.mon);
                                    let on = holds(c, v, *ctl);
                                    let resp = button(ui, m, &labels[i][j], on, off, focus);
                                    if resp.hovered() {
                                        hover_help = Some(i);
                                        hover_opt = Some((i, j));
                                    }
                                    if resp.clicked() && !off {
                                        click = Some(Click::Apply(s.menu, *child));
                                    }
                                }
                            });
                        }
                    }
                    if focus {
                        let r = Rect::from_min_max(top - vec2(6.0, 2.0), pos2(top.x + ui.available_width() + 6.0, ui.cursor().min.y + 2.0));
                        ui.painter().rect_stroke(r, 6.0, egui::Stroke::new(1.0, m.theme.color("focus").gamma_multiply(0.6)), egui::StrokeKind::Outside);
                    }
                    ui.add_space(8.0);
                }
            }));
        });
        if let Some(v) = ours.as_ref().filter(|v| !v.help.is_empty()) {
            ui.separator();
            ui.label(egui::RichText::new(&v.help).size(m.theme.small).color(m.theme.color("muted")));
            return;
        }
        // The help line of the setting under the pointer, else the focused one.
        let hovered = hover_opt.and_then(|(i, j)| opt_helps.get(i)?.get(j)).filter(|h| !h.is_empty());
        if let Some(h) = hovered.or_else(|| hover_help.or(open.then_some(row)).and_then(|i| helps.get(i))).filter(|h| !h.is_empty()) {
            ui.separator();
            ui.label(egui::RichText::new(h).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
    // A click on our page gives it the keys.
    if settings_act(w, sact)
        && let Some(c) = w.config.as_mut()
    {
        (c.m_open, c.m_row) = (true, 0);
    }
    match click {
        Some(Click::Entry(k)) => choose(w, k),
        Some(Click::Apply(menu, child)) => apply(w, menu, child),
        Some(Click::Slide(id, x)) => {
            let out = ConfigUi::set_value(&mut w.game, id, x);
            super::super::config_out(w, out);
        }
        Some(Click::Cat(k)) => {
            if let Some(c) = w.config.as_mut() {
                (c.m_cat, c.m_open, c.m_row) = (k, true, 0);
            }
            w.sounds.push(Se::Cursor as u32);
        }
        Some(Click::Toggle(page, r)) => toggle(w, page, r),
        Some(Click::Colour(id, c)) => {
            let mut out = configui::Out::default();
            configui2::set_colour(&mut w.game, &mut out, id, c);
            super::super::config_out(w, out);
        }
        None => {}
    }
}

/// A list row stepped (Enter, a click, Left / Right): retail's toggle, SE 0001.
fn toggle(w: &mut World, page: &str, r: usize) {
    let cat = w.config.as_ref().map_or(0, |c| c.m_cat);
    let mut out = configui::Out::default();
    configui2::modern_toggle(&mut w.game, &mut out, page, cat, r);
    out.sounds.push(Se::Cursor);
    super::super::config_out(w, out);
}

/// An option's name when its sprite has none to read.
fn fallback(ctl: Ctl) -> String {
    match ctl {
        Ctl::Pick(_, x, _) => x.to_string(),
        Ctl::Bit(_, _, on, _) => if on { "On" } else { "Off" }.into(),
        Ctl::Act(Act::Anon(h)) => if h { "Hide" } else { "Show" }.into(),
        Ctl::Act(Act::ResetKeys) => "Default".into(),
        Ctl::Open(n) => n.into(),
        _ => String::new(),
    }
}

/// An option in the pack's button frame: lit while it holds, a ring while its row has the keys, grey when off.
fn button(ui: &mut Ui, m: &super::Modern, label: &str, on: bool, off: bool, focus: bool) -> egui::Response {
    let font = FontId::proportional(m.theme.small);
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x);
    let (r, resp) = ui.allocate_exact_size(vec2((tw + 24.0).max(64.0), 26.0), Sense::click());
    let face = match () {
        _ if off => "controls/button_disabled",
        _ if on => "controls/button_selected",
        _ if resp.hovered() => "controls/button_hover",
        _ => "controls/button_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
    }
    if focus && on {
        ui.painter().rect_stroke(r.expand(1.5), 6.0, egui::Stroke::new(1.5, m.theme.color("focus")), egui::StrokeKind::Outside);
    }
    let c = if off { m.theme.color("muted") } else if on { m.theme.color("gold") } else { m.theme.color("text") };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, c);
    resp
}

/// The list's entry `k` taken: a page here gets the keys; retail's window opens over this one; Extras / Settings ours.
fn choose(w: &mut World, k: usize) {
    let Some(c) = w.config.as_mut() else { return };
    c.m_entry = k;
    let mut out = configui::Out::default();
    out.sounds.push(Se::Select);
    // Every page opens in place, ours (Extras / Settings) too: its state takes the keys from here.
    (c.m_open, c.m_row) = (true, 0);
    super::super::config_out(w, out);
    settings_follow(w, k);
}

/// An option or button taken (retail's Enter on that child of its page).
fn apply(w: &mut World, menu: &str, child: u16) {
    let sw = w.ui_wh.0;
    let Some(c) = w.config.as_mut() else { return };
    let out = c.apply(&mut w.game, menu, child, sw);
    super::super::config_out(w, out);
}

/// The keys while the page shows: on the list Up / Down choose and Enter or Right takes, Escape closes Config; on a page Up /
/// Down move between settings, Left / Right change the focused one (a slider by its step), Enter takes a button,
/// Escape goes back to the list. True when taken.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    if !w.modern || !shown(w) {
        return false;
    }
    let sw = w.ui_wh.0;
    let Some(c) = w.config.as_mut() else { return false };
    use egui::Key as K;
    let n = ENTRIES.len();
    if !c.m_open {
        match key {
            K::ArrowUp | K::ArrowDown => {
                c.m_entry = if key == K::ArrowDown { (c.m_entry + 1) % n } else { (c.m_entry + n - 1) % n };
                w.sounds.push(Se::Cursor as u32);
            }
            K::Enter | K::ArrowRight => {
                let k = c.m_entry;
                choose(w, k);
            }
            K::Escape => {
                let out = configui::Out { closed: true, sounds: vec![Se::Close], ..Default::default() };
                w.config.as_mut().map(|c| c.stack.clear());
                super::super::config_out(w, out);
            }
            _ => {}
        }
        return true;
    }
    // The list pages: row 0 the category strip (Left / Right the category) where there is one; Left / Right or Enter
    // step a row; Font Colors' rows are each colour's R, G and B (Left / Right 4, retail's step).
    match ENTRIES[c.m_entry.min(n - 1)].1 {
        Entry::List(page, menu) => {
            let first = menu.is_some() as usize;
            let rows = configui2::modern_rows(&w.game, page, c.m_cat).len() + first;
            let row = c.m_row.min(rows.saturating_sub(1));
            match key {
                K::ArrowUp | K::ArrowDown => {
                    c.m_row = if key == K::ArrowDown { (row + 1) % rows.max(1) } else { (row + rows.max(1) - 1) % rows.max(1) };
                    w.sounds.push(Se::Cursor as u32);
                }
                K::ArrowLeft | K::ArrowRight if first == 1 && row == 0 => {
                    c.m_cat = if key == K::ArrowRight { (c.m_cat + 1) % 4 } else { (c.m_cat + 3) % 4 };
                    w.sounds.push(Se::Cursor as u32);
                }
                K::ArrowLeft | K::ArrowRight | K::Enter if row >= first => toggle(w, page, row - first),
                K::Escape => {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                _ => {}
            }
            return true;
        }
        Entry::Colors => {
            let list = configui2::colour_rows(c.m_cat);
            let rows = 1 + list.len() * 3;
            let row = c.m_row.min(rows - 1);
            match key {
                K::ArrowUp | K::ArrowDown => {
                    c.m_row = if key == K::ArrowDown { (row + 1) % rows } else { (row + rows - 1) % rows };
                    w.sounds.push(Se::Cursor as u32);
                }
                K::ArrowLeft | K::ArrowRight if row == 0 => {
                    c.m_cat = if key == K::ArrowRight { (c.m_cat + 1) % 4 } else { (c.m_cat + 3) % 4 };
                    w.sounds.push(Se::Cursor as u32);
                }
                K::ArrowLeft | K::ArrowRight => {
                    let (i, ch) = ((row - 1) / 3, (row - 1) % 3);
                    if let Some(&(_, _, id)) = list.get(i) {
                        let mut v = rgb(w.game.get(id));
                        v[ch] = if key == K::ArrowRight { v[ch].saturating_add(4) } else { v[ch].saturating_sub(4) };
                        let mut out = configui::Out::default();
                        configui2::set_colour(&mut w.game, &mut out, id, v);
                        super::super::config_out(w, out);
                    }
                }
                K::Escape => {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                _ => {}
            }
            return true;
        }
        _ => {}
    }
    let Entry::Native(menus) = ENTRIES[c.m_entry.min(n - 1)].1 else { return true };
    let all = shapes(menus);
    let row = c.m_row.min(all.len().saturating_sub(1));
    let Some(s) = all.get(row) else { return true };
    match key {
        K::ArrowUp | K::ArrowDown => {
            let len = all.len().max(1);
            c.m_row = if key == K::ArrowDown { (row + 1) % len } else { (row + len - 1) % len };
            w.sounds.push(Se::Cursor as u32);
        }
        K::Escape => {
            c.m_open = false;
            w.sounds.push(Se::Cancel as u32);
        }
        K::ArrowLeft | K::ArrowRight => match s.ctls[0].1 {
            Ctl::Slider(id, lo, hi, step) => {
                let val = w.game.get_i(id).clamp(lo, hi);
                let x = if key == K::ArrowRight { (val + step).min(hi) } else { (val - step).max(lo) };
                if x != val {
                    let out = ConfigUi::set_value(&mut w.game, id, x as u32);
                    w.sounds.push(Se::Cursor as u32);
                    super::super::config_out(w, out);
                }
            }
            Ctl::Pick(..) | Ctl::Bit(..) | Ctl::Act(Act::Anon(_)) => {
                let on = s.ctls.iter().position(|(_, ctl)| holds(c, &w.game, *ctl)).unwrap_or(0);
                let len = s.ctls.len();
                let order: Vec<usize> = (1..len).map(|d| if key == K::ArrowRight { (on + d) % len } else { (on + len - d) % len }).collect();
                if let Some(&k) = order.iter().find(|&&k| !configui::disabled(s.menu, s.ctls[k].0, sw, c.mon)) {
                    let (menu, child) = (s.menu, s.ctls[k].0);
                    apply(w, menu, child);
                }
            }
            _ => {}
        },
        K::Enter => {
            if matches!(s.ctls[0].1, Ctl::Open(_) | Ctl::Act(_)) && !matches!(s.ctls[0].1, Ctl::Act(Act::Anon(_))) {
                let (menu, child) = (s.menu, s.ctls[0].0);
                apply(w, menu, child);
            }
        }
        _ => {}
    }
    true
}
