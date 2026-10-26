//! The modern Config page in the command window, over client/configui.rs (its values, setters and windows): retail's
//! Config list on the left without the Windows page (the log windows' frame, size and width mean nothing with the modern
//! chat; user, 2026-10-26) and with Misc. 1..4 as one page (the numbered entries merged); the chosen page on the right,
//! a row per setting with its options as buttons or a slider, named with retail's own captions read from the page's
//! layout, its help line under them. The list pages and editors (chat filters, font colours, log categories, effects,
//! key assignment) open retail's own window over it, Extras and Settings ours.

use super::super::World;
use super::super::configui::{self, Act, ConfigUi, Ctl};
use super::super::game_ui::GameUi;
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

/// What a list entry opens.
#[derive(Clone, Copy)]
enum Entry {
    /// Drawn here from these retail pages' controls.
    Native(&'static [&'static str]),
    /// Retail's window over this one.
    Retail(&'static str),
    Extras,
    Settings,
}

const ENTRIES: [(&str, Entry); 10] = [
    ("Gameplay", Entry::Native(&["conf2win"])),
    ("Chat Filters", Entry::Retail("cfilter")),
    ("Font Colors", Entry::Retail("conftxtc")),
    ("Log", Entry::Retail("conf11m")),
    ("Misc.", Entry::Native(&["conf3win", "conf6win", "conf12wi", "conf13wi"])),
    ("Effects", Entry::Retail("fxfilter")),
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
        for &(child, ctl) in configui::page(menu) {
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

/// A setting's heading: the page's own caption on its first control's line to its left, else the nearest above it
/// (not the page's title band); else the control's help title.
fn heading(gu: &GameUi, s: &Shape) -> String {
    let name = full(s.menu);
    let Some((cx, cy)) = s.ctls.first().and_then(|c| gu.child_place(&name, c.0)) else { return String::new() };
    // A slider's ends ("Min", "Max") are not headings; a slider's heading is above it.
    let heads: Vec<_> = gu.window_captions(&name).into_iter().filter(|(t, _, _)| !matches!(t.as_str(), "Min" | "Max")).collect();
    let slider = matches!(s.ctls[0].1, Ctl::Slider(..));
    let left = heads.iter().filter(|(_, x, y)| !slider && (*y - cy).abs() <= 6.0 && *x < cx).max_by(|a, b| a.1.total_cmp(&b.1));
    let above = || heads.iter().filter(|(_, x, y)| *y > 12.0 && *y < cy && cy - *y < 34.0 && *x <= cx + 40.0).max_by(|a, b| a.2.total_cmp(&b.2));
    left.or_else(above).map(|h| h.0.clone()).or_else(|| gu.focus_text(&name, s.ctls[0].0)[1].clone()).unwrap_or_default()
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
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    if !w.modern || !shown(w) {
        return;
    }
    let Some(c) = w.config.as_ref() else { return };
    let m = p.m;
    let sw = w.ui_wh.0;
    let (entry, open, row) = (c.m_entry.min(ENTRIES.len() - 1), c.m_open, c.m_row);
    let shapes = match ENTRIES[entry].1 {
        Entry::Native(menus) => shapes(menus),
        _ => Vec::new(),
    };
    let heads: Vec<String> = shapes.iter().map(|s| heading(gu, s)).collect();
    let labels: Vec<Vec<String>> = shapes.iter().map(|s| s.ctls.iter().map(|(k, ctl)| gu.child_caption(&full(s.menu), *k).unwrap_or_else(|| fallback(*ctl))).collect()).collect();
    let helps: Vec<String> = shapes.iter().map(|s| gu.focus_text(&full(s.menu), s.ctls[0].0)[0].clone().unwrap_or_default()).collect();
    let v = &w.game;
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
                    if let Some(s) = m.art.sprite(face) {
                        ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                    }
                    ui.painter().text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(m.theme.ui), m.theme.color("text"));
                    if resp.clicked() {
                        click = Some(Click::Entry(k));
                    }
                    ui.add_space(2.0);
                }
            });
            ui.add_space(16.0);
            // The page, scrolled when longer than most of the screen.
            let tall = ui.ctx().content_rect().height() * 0.72;
            egui::ScrollArea::vertical().max_height(tall).auto_shrink([false, true]).show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(440.0);
                if shapes.is_empty() {
                    let what = match ENTRIES[entry].1 {
                        Entry::Retail(_) => "Opens its own window.",
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
        // The help line of the setting under the pointer, else the focused one.
        if let Some(h) = hover_help.or(open.then_some(row)).and_then(|i| helps.get(i)).filter(|h| !h.is_empty()) {
            ui.separator();
            ui.label(egui::RichText::new(h).size(m.theme.small).color(m.theme.color("muted")));
        }
    });
    match click {
        Some(Click::Entry(k)) => choose(w, k),
        Some(Click::Apply(menu, child)) => apply(w, menu, child),
        Some(Click::Slide(id, x)) => {
            let out = ConfigUi::set_value(&mut w.game, id, x);
            super::super::config_out(w, out);
        }
        None => {}
    }
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
    match ENTRIES[k].1 {
        Entry::Native(_) => (c.m_open, c.m_row) = (true, 0),
        Entry::Retail(name) => c.push(name),
        Entry::Extras => out.open_extras = true,
        Entry::Settings => out.open_settings = true,
    }
    super::super::config_out(w, out);
}

/// An option or button taken (retail's Enter on that child of its page).
fn apply(w: &mut World, menu: &str, child: u16) {
    let sw = w.ui_wh.0;
    let Some(c) = w.config.as_mut() else { return };
    let out = c.apply(&mut w.game, menu, child, sw);
    super::super::config_out(w, out);
}

/// The keys while the page shows: on the list Up / Down choose and Enter takes, Escape closes Config; on a page Up /
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
            K::Enter => {
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
