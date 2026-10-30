//! The modern Config page in the command window, over client/configui.rs (its values, setters and windows): retail's
//! Config list on the left without the Windows page (the log windows' frame, size and width mean nothing with the modern
//! chat; user, 2026-10-26) and with Misc. 1..4 as one page (the numbered entries merged); the chosen page on the right,
//! a row per setting with its options as buttons or a slider, named with retail's own captions read from the page's
//! layout (a few made clearer), its help line under its heading. Chat Filters and Effects are lists of rows with
//! retail's toggles, Font Colors each colour with a picker, key assignment a list in the page. Extras and Settings
//! (ours, client/settings.rs) are pages here too. Every page is the widget kit's (kit.rs): its controls and keys.

use super::super::World;
use super::super::configui::{self, Act, ConfigUi, Ctl};
use super::super::configui2;
use super::super::game_ui::GameUi;
use super::super::settings::{self, SettingsUi};
use super::kit::{self, At, Btn, Stop};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use crate::gamecfg::Values;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

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

/// The page's width and its scroll area's share of the screen's height.
const PAGE_W: f32 = 440.0;
const TALL: f32 = 0.72;

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
fn holds(c: &ConfigUi, v: &Values, ctl: Ctl) -> bool {
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

/// A click on what the mouse took (the cursor goes there too).
enum Click {
    Entry(usize),
    /// Native setting `row`: its option `opt` taken, its slider set to stop `x`, or its button pressed.
    Opt(usize, usize),
    Slide(usize, usize),
    Press(usize),
    Cat(usize),
    Toggle(usize),
    /// Font Colors: colour `i` set to an RGB (the picker) or to hue / saturation / value steps (its sliders), or its
    /// first stop given the cursor.
    Colour(usize, [u8; 3]),
    Hsv(usize, [usize; 3]),
    Focus(usize),
    /// The key list's row; the reset prompt's answer.
    KeyRow(usize),
    Reset(bool),
}

/// A list page's categories: the menu's rows' captions.
fn categories(gu: &GameUi, menu: &str) -> Vec<String> {
    (1..=4u16).map(|k| gu.child_caption(&full(menu), k).unwrap_or_else(|| format!("{k}"))).collect()
}

/// A colour's channels as they show from its config value (0x80RRGGBB, 0x80 the full channel), and the value's for a
/// colour shown.
fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8].map(|x| (x as u16 * 2).min(255) as u8)
}

fn raw(shown: [u8; 3]) -> [u8; 3] {
    shown.map(|x| (x as u16).div_ceil(2) as u8)
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

/// A native page's stops: a value per setting of options or a slider, a button for one that opens or does something.
fn native_stops(c: &ConfigUi, v: &Values, all: &[Shape], sw: f32) -> Vec<Stop> {
    all.iter()
        .map(|s| match s.ctls[0].1 {
            Ctl::Slider(id, lo, hi, step) => {
                let step = step.max(1);
                Stop::Value { n: ((hi - lo) / step) as usize + 1, at: ((v.get_i(id).clamp(lo, hi) - lo) / step) as usize, slider: true, off: 0 }
            }
            Ctl::Pick(..) | Ctl::Bit(..) | Ctl::Act(Act::Anon(_)) => {
                let off = s.ctls.iter().enumerate().filter(|(_, (k, _))| c.off(s.menu, *k, sw)).fold(0u64, |m, (j, _)| m | 1 << j);
                Stop::Value { n: s.ctls.len(), at: s.ctls.iter().position(|(_, ctl)| holds(c, v, *ctl)).unwrap_or(0), slider: false, off }
            }
            _ => Stop::Buttons { n: 1, off: c.off(s.menu, s.ctls[0].0, sw) as u64 },
        })
        .collect()
}

/// A native setting's option `x` taken, or its slider set to stop `x`.
fn native_set(c: &mut ConfigUi, v: &mut Values, s: &Shape, x: usize, sw: f32) -> configui::Out {
    match s.ctls[0].1 {
        Ctl::Slider(id, lo, hi, step) => {
            let mut out = ConfigUi::set_value(v, id, (lo + x as i32 * step.max(1)).min(hi) as u32);
            out.sounds.push(Se::Cursor);
            out
        }
        _ => s.ctls.get(x).map_or_else(configui::Out::default, |(child, _)| c.apply(v, s.menu, *child, sw)),
    }
}

/// A native setting's button pressed: key assignment and its reset open here, the rest as retail's Enter.
fn native_press(c: &mut ConfigUi, v: &mut Values, s: &Shape, sw: f32) -> configui::Out {
    let mut out = configui::Out::default();
    match s.ctls[0].1 {
        _ if c.off(s.menu, s.ctls[0].0, sw) => out.sounds.push(Se::Error),
        Ctl::Open(name @ ("k1assign" | "k2assign")) => {
            (c.m_keys, c.m_key_row) = (Some(name), 0);
            out.sounds.push(Se::Select);
        }
        Ctl::Act(Act::ResetKeys) => {
            c.extra.ask_keys = Some(false);
            out.sounds.push(Se::Select);
        }
        _ => out = c.apply(v, s.menu, s.ctls[0].0, sw),
    }
    out
}

/// A list page's stops: the category strip where it has one, then a toggle per row.
fn list_stops(v: &Values, page: &str, cats: bool, cat: usize) -> Vec<Stop> {
    let rows = configui2::modern_rows(v, page, cat);
    cats.then_some(Stop::Tabs { n: 4, at: cat }).into_iter().chain(rows.iter().map(|r| Stop::Value { n: 2, at: (r.1 == 88) as usize, slider: false, off: 0 })).collect()
}

/// A colour's hue / saturation / value steps: the ones being edited while they still make its colour (a grey has no
/// hue of its own to read back), else read from it.
fn steps_of(c: &ConfigUi, v: &Values, id: u16) -> [usize; 3] {
    let shown = rgb(v.get(id));
    match c.m_hsv {
        Some((i, s)) if i == id && raw(kit::from_steps(s)) == raw(shown) => s,
        _ => kit::hsv_steps(shown),
    }
}

/// Font Colors' stops: the category strip, then each colour's hue, saturation and value.
fn colour_stops(c: &ConfigUi, v: &Values, cat: usize) -> Vec<Stop> {
    let mut out = vec![Stop::Tabs { n: 4, at: cat }];
    for &(_, _, id) in configui2::colour_rows(cat) {
        for at in steps_of(c, v, id) {
            out.push(Stop::Value { n: kit::STEPS + 1, at, slider: true, off: 0 });
        }
    }
    out
}

/// Our Settings / Extras page's stops: the page tabs, then each row's options or slider.
fn ours_stops(v: &settings::View) -> Vec<Stop> {
    let tabs = (!v.tabs.is_empty()).then_some(Stop::Tabs { n: v.tabs.len(), at: v.tab });
    let rows = v.rows.iter().map(|r| match &r.ctl {
        settings::VCtl::Buttons(bs) => Stop::Value { n: bs.len(), at: bs.iter().position(|b| b.1).unwrap_or(0), slider: false, off: 0 },
        settings::VCtl::Slider { n, at, .. } => Stop::Value { n: *n, at: *at, slider: true, off: 0 },
    });
    tabs.into_iter().chain(rows).collect()
}

/// The Mods page's stops: the tabs, Install from File, each pack's toggle / up / down / uninstall, each archive's
/// Install; the picker's list while it is open.
fn mods_stops(s: &SettingsUi) -> Vec<Stop> {
    if let Some(b) = s.mods_state().and_then(|m| m.browse.as_ref()) {
        return vec![Stop::List { n: b.entries.len(), page: 10 }];
    }
    let (packs, archives) = s.mods_rows();
    let busy = s.mods_state().is_some_and(|m| m.installing().is_some());
    let n = packs.len();
    let mut out = vec![Stop::Tabs { n: s.view().tabs.len(), at: s.view().tab }, Stop::Buttons { n: 1, off: busy as u64 }];
    out.extend((0..n).map(|i| Stop::Buttons { n: 4, off: ((i == 0) as u64) << 1 | ((i + 1 == n) as u64) << 2 }));
    out.extend(archives.iter().map(|_| Stop::Buttons { n: 1, off: busy as u64 }));
    out
}

/// A Mods page act; true when the page is left (Escape).
fn mods_act(s: &mut SettingsUi, act: kit::Act, at: At) -> bool {
    let (packs, _) = s.mods_rows();
    let n = packs.len();
    if let Some(m) = s.mods_mut()
        && let Some(b) = m.browse.as_mut()
    {
        match act {
            kit::Act::Moved => b.cursor = at.sub,
            kit::Act::Press(_, k) => {
                b.cursor = k;
                if let Some(path) = b.enter() {
                    m.browse = None;
                    if !m.install_file(path) {
                        s.sounds.push(Se::Error);
                        return false;
                    }
                }
                s.sounds.push(Se::Select);
            }
            kit::Act::Back => {
                m.browse = None;
                s.sounds.push(Se::Cancel);
            }
            _ => {}
        }
        return false;
    }
    match act {
        kit::Act::Back => return true,
        kit::Act::Set(0, t) => {
            s.m_tab(t);
            s.m_mods = (0, 0);
        }
        kit::Act::Moved => s.m_mods = (at.stop, at.sub),
        kit::Act::Press(1, _) => {
            if let Some(m) = s.mods_mut() {
                m.browse = Some(super::super::modsui::Browse::open());
                m.error = None;
            }
            s.sounds.push(Se::Select);
        }
        kit::Act::Press(st, b) if st >= 2 && st - 2 < n => {
            let i = st - 2;
            s.m_mods = (st, b);
            match b {
                0 => s.mod_enable(i, !packs[i].1),
                1 if s.mod_move(i, true) => s.m_mods.0 -= 1,
                2 if s.mod_move(i, false) => s.m_mods.0 += 1,
                3 => {
                    s.m_remove = Some((i, false));
                    s.sounds.push(Se::Select);
                }
                _ => {}
            }
        }
        kit::Act::Press(st, _) if st >= 2 => s.archive_install(st - 2 - n),
        a => s.sounds.extend(a.sound()),
    }
    false
}

/// The Mods page's removal prompt keys: Left / Right move, Enter answers, Escape is No.
fn remove_key(s: &mut SettingsUi, key: egui::Key) {
    let Some((i, yes)) = s.m_remove else { return };
    match key {
        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            s.m_remove = Some((i, !yes));
            s.sounds.push(Se::Cursor);
        }
        egui::Key::Enter | egui::Key::Escape => remove_answer(s, yes && key == egui::Key::Enter),
        _ => {}
    }
}

fn remove_answer(s: &mut SettingsUi, yes: bool) {
    let Some((i, _)) = s.m_remove.take() else { return };
    if yes {
        s.mod_remove(i);
        let n = s.mods_rows().0.len();
        if s.m_mods.0 >= 2 + n {
            s.m_mods.0 = (1 + n).max(1);
        }
    } else {
        s.sounds.push(Se::Cancel);
    }
}

/// Our page's act (Settings / Extras, not Mods); true when the page is left.
fn ours_act(s: &mut SettingsUi, v: &settings::View, act: kit::Act, at: At) -> bool {
    let first = (!v.tabs.is_empty()) as usize;
    match act {
        kit::Act::Back => return true,
        kit::Act::Moved => {
            s.m_point(at.stop + 1 - first, at.sub);
            s.sounds.push(Se::Cursor);
        }
        kit::Act::Set(0, t) if first == 1 => s.m_tab(t),
        kit::Act::Set(r, x) => {
            let row = r + 1 - first;
            match v.rows.get(row - 1).map(|r| &r.ctl) {
                Some(settings::VCtl::Buttons(_)) => s.m_click(row, x),
                Some(settings::VCtl::Slider { .. }) => s.m_slide(row, x),
                None => {}
            }
        }
        a => s.sounds.extend(a.sound()),
    }
    false
}

/// Our page's keys (Settings, Extras, Mods); true when the page is left.
fn ours_key(s: &mut SettingsUi, key: egui::Key, shift: bool) -> bool {
    s.sounds.clear();
    if s.m_remove.is_some() {
        remove_key(s, key);
        return false;
    }
    if s.on_mods_page() {
        let stops = mods_stops(s);
        let mut at = match s.mods_state().and_then(|m| m.browse.as_ref()) {
            Some(b) => At::new(0, b.cursor),
            None => At::new(s.m_mods.0, s.m_mods.1),
        };
        let act = kit::key(&stops, &mut at, key, shift);
        return mods_act(s, act, at);
    }
    let v = s.view();
    let stops = ours_stops(&v);
    let first = (!v.tabs.is_empty()) as usize;
    let mut at = At::new((v.row + first).saturating_sub(1), v.col);
    // Row 0 is the page name (the tabs) in SettingsUi; the kit's stops start there too when there are tabs.
    if first == 1 {
        at.stop = v.row;
    }
    let act = kit::key(&stops, &mut at, key, shift);
    ours_act(s, &v, act, at)
}

/// The key assignment list's keys: the kit's list, Escape back to the page, an assignable key to the cursor's row.
fn keys_key(c: &mut ConfigUi, v: &mut Values, key: egui::Key) -> configui::Out {
    let mut out = configui::Out::default();
    let Some(name) = c.m_keys else { return out };
    let n = configui2::assign_rows(v, name);
    let mut at = At::new(0, c.m_key_row.min(n.saturating_sub(1)));
    match kit::key(&[Stop::List { n, page: 10 }], &mut at, key, false) {
        kit::Act::Back => {
            c.m_keys = None;
            out.sounds.push(Se::Cancel);
        }
        kit::Act::Moved => {
            c.m_key_row = at.sub;
            out.sounds.push(Se::Cursor);
        }
        kit::Act::None if !configui2::assign(v, &mut out, name, c.m_key_row, key) => {}
        _ => {}
    }
    out
}

/// The reset prompt's keys: Left / Right move, Enter answers, Escape is No.
fn reset_key(c: &mut ConfigUi, v: &mut Values, key: egui::Key) -> configui::Out {
    let mut out = configui::Out::default();
    let Some(yes) = c.extra.ask_keys else { return out };
    match key {
        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            c.extra.ask_keys = Some(!yes);
            out.sounds.push(Se::Cursor);
        }
        egui::Key::Enter | egui::Key::Escape => reset_answer(c, v, yes && key == egui::Key::Enter, &mut out),
        _ => {}
    }
    out
}

fn reset_answer(c: &mut ConfigUi, v: &mut Values, yes: bool, out: &mut configui::Out) {
    c.extra.ask_keys = None;
    if yes {
        configui2::reset_keys(v, out);
        out.sounds.push(Se::Select);
    } else {
        out.sounds.push(Se::Cancel);
    }
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

/// A native page's words: each setting's heading, its options' names, and the setting's line (ours where we gave one,
/// else the help of the option that holds: the first for a slider or none).
struct Texts {
    heads: Vec<String>,
    labels: Vec<Vec<String>>,
    helps: Vec<String>,
}

/// Options whose sprite is an icon: (page, child, name).
const LABELS: [(&str, u16, &str); 2] = [("lobycwin", 5, "Newest"), ("lobycwin", 6, "Random")];

fn texts(gu: &GameUi, c: &ConfigUi, v: &Values, shapes: &[Shape]) -> Texts {
    let heads = shapes.iter().map(|s| heading(gu, s)).collect();
    let label = |menu: &str, k: u16, ctl: Ctl| match LABELS.iter().find(|l| l.0 == menu && l.1 == k) {
        Some(l) => l.2.to_string(),
        None => gu.child_caption(&full(menu), k).unwrap_or_else(|| fallback(ctl)),
    };
    let labels = shapes.iter().map(|s| s.ctls.iter().map(|(k, ctl)| label(s.menu, *k, *ctl)).collect()).collect();
    let helps = shapes
        .iter()
        .map(|s| {
            let held = s.ctls.iter().position(|(_, ctl)| holds(c, v, *ctl)).unwrap_or(0);
            let opt = s.ctls.get(held).and_then(|(k, _)| gu.focus_text(&full(s.menu), *k)[0].clone()).unwrap_or_default();
            override_of(s).map(|h| h.3).filter(|h| !h.is_empty()).map(str::to_string).unwrap_or(opt)
        })
        .collect();
    Texts { heads, labels, helps }
}

/// The list on the left: the chosen entry lit, a click choosing one.
fn entry_list(ui: &mut Ui, m: &super::Modern, labels: &[&str], entry: usize, open: bool, click: &mut Option<Click>) {
    ui.vertical(|ui| {
        ui.set_width(150.0);
        for (k, label) in labels.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::click());
            let face = match () {
                _ if k == entry && !open => "controls/list_row_selected",
                _ if k == entry || resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(m.theme.ui), m.theme.color("text"));
            if resp.clicked() {
                *click = Some(Click::Entry(k));
            }
            ui.add_space(2.0);
        }
    });
}

/// A native page's settings: each heading and its options as buttons, its slider, or its button; the cursor's ringed.
/// The setting under the pointer (for the help line).
#[allow(clippy::too_many_arguments)]
fn native_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, c: &ConfigUi, v: &Values, shapes: &[Shape], t: &Texts, focus_row: Option<usize>, follow: bool, sw: f32, click: &mut Option<Click>) -> Option<usize> {
    let stops = native_stops(c, v, shapes, sw);
    let mut hover = None;
    for (i, s) in shapes.iter().enumerate() {
        let focus = focus_row == Some(i);
        let (_, r) = match (s.ctls[0].1, stops[i]) {
            (Ctl::Slider(id, ..), Stop::Value { n, at, .. }) => kit::setting(ui, m, &t.heads[i], focus, true, follow, |ui| {
                // The aspect ratios by name, as retail's text under the slider.
                let val = v.get_i(id);
                let shown = match id {
                    0x43 | 0xab => configui::ASPECTS.get(val.max(0) as usize).map_or(String::new(), |a| a.0.trim_matches(['(', ')']).to_string()),
                    _ => val.to_string(),
                };
                if let Some(x) = kit::slider(ui, m, n, at, &shown) {
                    *click = Some(Click::Slide(i, x));
                }
            }),
            (_, Stop::Value { at, off, .. }) => kit::setting(ui, m, &t.heads[i], focus, true, follow, |ui| {
                if let Some(x) = kit::choice(ui, m, &t.labels[i], at, off) {
                    *click = Some(Click::Opt(i, x));
                }
                // keylayou (Misc. 2's Compact 1): the functions the compact keyboard picture shows and their keys.
                if s.menu == "conf6win" && s.ctls.iter().any(|(k, _)| *k == 6) && v.get(0x42) == 1 {
                    ui.add_space(4.0);
                    egui::Grid::new("keylayou").num_columns(4).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
                        for (k, cfg) in configui::KEYLAYOU_CELLS.iter().enumerate() {
                            let label = configui2::k1_label(gu, *cfg).unwrap_or_default();
                            ui.label(egui::RichText::new(label).size(m.theme.small - 1.5).color(m.theme.color("muted")));
                            ui.label(egui::RichText::new(configui2::dik_char(v.get(*cfg))).size(m.theme.small - 1.5).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("text")));
                            if k % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
            }),
            (_, stop) => kit::setting(ui, m, &t.heads[i], focus, false, follow, |ui| {
                let off = matches!(stop, Stop::Buttons { off: 1, .. }) as u64;
                if kit::buttons(ui, m, &[t.labels[i][0].as_str()], focus.then_some(0), 0, off).is_some() {
                    *click = Some(Click::Press(i));
                }
            }),
        };
        if ui.rect_contains_pointer(r) {
            hover = Some(i);
        }
    }
    hover
}

/// The help line under a page.
fn help_line(ui: &mut Ui, m: &super::Modern, text: &str) {
    if !text.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new(text).size(m.theme.small).color(m.theme.color("muted")));
    }
}

/// A list page's rows: the label, its state as a button at the right (a click steps it as Enter does).
#[allow(clippy::too_many_arguments)]
fn list_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, rows: &[(u16, usize)], focus: Option<usize>, follow: bool, click: &mut Option<Click>) {
    for (r, &(label, state)) in rows.iter().enumerate() {
        let text = gu.group_b(label as usize).unwrap_or_default();
        kit::line(ui, m, &text, focus == Some(r), follow, |ui| {
            let s = Btn { lit: state == 88, ..Default::default() };
            if kit::clicked(&kit::button(ui, m, &state_word(m, gu, state), 104.0, s), s) {
                *click = Some(Click::Toggle(r));
            }
            if matches!(state, 136 | 137)
                && let Some(s) = m.art.sprite("icons/chat")
            {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                ui.painter().add(super::paint::image(&s, r.min, 18.0, Color32::WHITE));
            }
        });
    }
}

/// Font Colors: each colour's name, sample line and swatch; the cursor's colour open with the picker and its hue,
/// saturation and brightness sliders (the cursor's ringed).
#[allow(clippy::too_many_arguments)]
fn colour_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, cu: &ConfigUi, v: &Values, rows: &[(u16, u16, u16)], focus: Option<usize>, follow: bool, click: &mut Option<Click>) {
    const NAMES: [&str; 3] = ["Hue", "Saturation", "Brightness"];
    for (i, &(label, sample, id)) in rows.iter().enumerate() {
        let c = rgb(v.get(id));
        let col = super::chat::colour(crate::dat::battle::argb_bytes(v.get(id)));
        let open = focus.is_some_and(|f| f / 3 == i);
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let face = if open { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
        super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(pos2(r.min.x + 12.0, r.center().y - 8.0), vec2(16.0, 16.0)), 3.0, Color32::from_rgb(c[0], c[1], c[2]));
        p.text(pos2(r.min.x + 38.0, r.center().y), Align2::LEFT_CENTER, gu.group_b(label as usize).unwrap_or_default(), FontId::proportional(m.theme.small), if open { m.theme.color("gold") } else { m.theme.color("text") });
        p.text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, gu.group_b(sample as usize).unwrap_or_default(), FontId::proportional(m.theme.small), col);
        if resp.clicked() && !open {
            *click = Some(Click::Focus(i * 3));
        }
        ui.add_space(2.0);
        if !open {
            continue;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if let Some(n) = kit::colour_picker(ui, m, c, PAGE_W - 24.0) {
                *click = Some(Click::Colour(i, n));
            }
        });
        ui.add_space(6.0);
        let steps = steps_of(cu, v, id);
        for (ch, name) in NAMES.iter().enumerate() {
            let k = i * 3 + ch;
            kit::setting(ui, m, name, focus == Some(k), true, follow, |ui| {
                let shown = format!("{}%", steps[ch] * 100 / kit::STEPS);
                if let Some(x) = kit::slider(ui, m, kit::STEPS + 1, steps[ch], &shown) {
                    let mut s = steps;
                    s[ch] = x;
                    *click = Some(Click::Hsv(i, s));
                }
            });
        }
    }
}

/// Our Settings / Extras page: the page strip, each setting's heading and options or slider, the cursor's ringed
/// while `open` (the page has the keys). What the mouse did comes back.
enum Do {
    Tab(usize),
    Click(usize, usize),
    Slide(usize, usize),
}

fn settings_page(ui: &mut Ui, m: &super::Modern, v: &settings::View, open: bool, follow: bool, act: &mut Vec<Do>) {
    if !v.tabs.is_empty() {
        ui.set_max_width(PAGE_W);
        if let Some(k) = kit::tab_strip(ui, m, "settings tabs", v.tabs, v.tab, open && v.row == 0) {
            act.push(Do::Tab(k));
        }
    }
    for (k, r) in v.rows.iter().enumerate() {
        let row = k + 1;
        let focus = open && v.row == row;
        kit::setting(ui, m, &r.label, focus, true, follow, |ui| match &r.ctl {
            settings::VCtl::Buttons(bs) => {
                let labels: Vec<String> = bs.iter().map(|b| b.0.clone()).collect();
                if let Some(c) = kit::choice(ui, m, &labels, bs.iter().position(|b| b.1).unwrap_or(usize::MAX), 0) {
                    act.push(Do::Click(row, c));
                }
            }
            settings::VCtl::Slider { n, at, shown } => {
                if let Some(i) = kit::slider(ui, m, *n, *at, shown) {
                    act.push(Do::Slide(row, i));
                }
            }
        });
    }
}

/// The mouse's actions on our page.
fn settings_do(s: &mut SettingsUi, act: Vec<Do>) {
    for a in act {
        match a {
            Do::Tab(k) => s.m_tab(k),
            Do::Click(r, c) => s.m_click(r, c),
            Do::Slide(r, i) => s.m_slide(r, i),
        }
    }
}

/// What the mouse did on the Mods page.
enum ModDo {
    Tab(usize),
    Press(usize, usize),
    Browse(usize),
    Remove(bool),
}

/// The Mods page: Install from File at the top (or the file picker), each pack with its toggle, chevrons and
/// Uninstall, the archives found with Install; the removal prompt in place while asked.
fn mods_page(ui: &mut Ui, m: &super::Modern, s: &SettingsUi, open: bool, follow: bool, act: &mut Vec<ModDo>) {
    let v = s.view();
    let at = if open { Some(s.m_mods) } else { None };
    if let Some(k) = kit::tab_strip(ui, m, "settings tabs", v.tabs, v.tab, at.is_some_and(|a| a.0 == 0)) {
        act.push(ModDo::Tab(k));
    }
    let state = s.mods_state();
    if let Some(b) = state.and_then(|m| m.browse.as_ref()) {
        ui.label(egui::RichText::new(b.dir.display().to_string()).size(m.theme.small).color(m.theme.color("gold")));
        ui.add_space(4.0);
        let tall = ui.ctx().content_rect().height() * 0.5;
        let out = kit::list(ui, m, "mods browse", b.entries.len(), Some(b.cursor), open, follow, 28.0, tall, |_| false, |ui, row| {
            let (name, dir) = &b.entries[row.index];
            let p = ui.painter();
            // A folder's tabbed outline, or an archive's box with its strap.
            let r = Rect::from_center_size(pos2(row.rect.min.x + 22.0, row.rect.center().y), vec2(16.0, 12.0));
            let st = egui::Stroke::new(1.5, if *dir { m.theme.color("gold") } else { m.theme.color("muted") });
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Inside);
            if *dir {
                p.line_segment([r.left_top() + vec2(1.0, -2.0), r.left_top() + vec2(7.0, -2.0)], st);
            } else {
                p.line_segment([pos2(r.center().x, r.min.y), pos2(r.center().x, r.max.y)], st);
            }
            let label = if *dir && name != ".." { format!("{name}/") } else { name.clone() };
            p.text(pos2(row.rect.min.x + 40.0, row.rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), kit::ink(m, row));
        });
        if let Some(k) = out.clicked {
            act.push(ModDo::Browse(k));
        }
        return;
    }
    if let Some((i, yes)) = s.m_remove {
        let name = s.mods_rows().0.get(i).map_or(String::new(), |p| p.0.clone());
        let q = format!("Uninstall {name}? Its folder in the mods folder is deleted.");
        if let Some(a) = kit::yes_no(ui, m, &q, ["Yes", "No"], yes, m.theme.small) {
            act.push(ModDo::Remove(a));
        }
        return;
    }
    let busy = state.and_then(|m| m.installing());
    let on = |st: usize| at.filter(|a| a.0 == st).map(|a| a.1);
    kit::setting(ui, m, "Install", on(1).is_some(), false, follow, |ui| {
        if kit::buttons(ui, m, &["Install from File..."], on(1), 0, busy.is_some() as u64).is_some() {
            act.push(ModDo::Press(1, 0));
        }
        if let Some((slug, pm)) = &busy {
            ui.label(egui::RichText::new(format!("Installing {slug}: {}%", pm / 10)).size(m.theme.small).color(m.theme.color("gold")));
        }
        if let Some(e) = state.and_then(|m| m.error.as_ref()) {
            ui.label(egui::RichText::new(format!("The last install failed: {e}")).size(m.theme.small).color(m.theme.color("hp")));
        }
    });
    let (packs, archives) = s.mods_rows();
    let n = packs.len();
    for (i, (name, enabled)) in packs.iter().enumerate() {
        let st = i + 2;
        let cur = on(st);
        kit::line(ui, m, &format!("{}. {name}", i + 1), cur.is_some(), follow, |ui| {
            let un = Btn { cursor: cur == Some(3), ..Default::default() };
            if kit::clicked(&kit::button(ui, m, "Uninstall", 0.0, un), un) {
                act.push(ModDo::Press(st, 3));
            }
            let ch = match cur {
                Some(1) => Some(true),
                Some(2) => Some(false),
                _ => None,
            };
            if let Some(up) = kit::chevrons(ui, m, ("mod move", i), ch, i == 0, i + 1 == n) {
                act.push(ModDo::Press(st, if up { 1 } else { 2 }));
            }
            let t = Btn { lit: *enabled, cursor: cur == Some(0), off: false };
            if kit::clicked(&kit::button(ui, m, if *enabled { "ON" } else { "OFF" }, 64.0, t), t) {
                act.push(ModDo::Press(st, 0));
            }
        });
    }
    if !archives.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Found in the Downloads folder").size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("muted")));
        ui.add_space(4.0);
    }
    for (k, (name, pm)) in archives.iter().enumerate() {
        let st = 2 + n + k;
        let label = pm.map_or(name.clone(), |pm| format!("{}%  {name}", pm / 10));
        kit::line(ui, m, &label, on(st).is_some(), follow, |ui| {
            let b = Btn { cursor: on(st).is_some(), off: busy.is_some(), ..Default::default() };
            if kit::clicked(&kit::button(ui, m, "Install", 0.0, b), b) {
                act.push(ModDo::Press(st, 0));
            }
        });
    }
}

fn mods_do(s: &mut SettingsUi, act: Vec<ModDo>) {
    for a in act {
        match a {
            ModDo::Tab(k) => {
                s.m_tab(k);
                s.m_mods = (0, 0);
            }
            ModDo::Press(st, b) => {
                s.m_mods = (st, b);
                mods_act(s, kit::Act::Press(st, b), At::new(st, b));
            }
            ModDo::Browse(k) => {
                mods_act(s, kit::Act::Press(0, k), At::new(0, k));
            }
            ModDo::Remove(yes) => remove_answer(s, yes),
        }
    }
}

/// The key assignment list: each function and its key; a key pressed assigns to the cursor's row.
fn keys_page(ui: &mut Ui, m: &super::Modern, gu: &GameUi, v: &Values, name: &str, row: usize, follow: bool, click: &mut Option<Click>) {
    let title = if name == "k1assign" { "Key Assignment 1" } else { "Key Assignment 2" };
    ui.label(egui::RichText::new(title).size(m.theme.ui).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(6.0);
    let n = configui2::assign_rows(v, name);
    let tall = ui.ctx().content_rect().height() * 0.55;
    let out = kit::list(ui, m, ("config keys", name), n, Some(row), true, follow, 28.0, tall, |_| false, |ui, r| {
        let (label, id) = configui2::assign_row(gu, name, r.index);
        let key = configui2::dik_char(v.get(id));
        let p = ui.painter();
        let cap = Rect::from_center_size(pos2(r.rect.min.x + 30.0, r.rect.center().y), vec2(30.0, 22.0));
        p.rect(cap, 4.0, m.theme.color("charcoal"), egui::Stroke::new(1.5, m.theme.color("bronze")), egui::StrokeKind::Inside);
        p.text(cap.center(), Align2::CENTER_CENTER, key, FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), m.theme.color("text"));
        p.text(pos2(r.rect.min.x + 60.0, r.rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), kit::ink(m, r));
    });
    if let Some(k) = out.clicked {
        *click = Some(Click::KeyRow(k));
    }
}

/// The title's Config list: retail's page (lobycwin) and ours.
const LOBBY: [&str; 2] = ["Title Screen", "Settings"];

/// lobycwin's settings in retail's order on screen: the title music first.
fn lobby_shapes() -> Vec<Shape> {
    let mut all = shapes(&["lobycwin"]);
    all.sort_by_key(|s| !matches!(s.ctls[0].1, Ctl::Pick(0xaf, ..)));
    all
}

/// Whether the title's Config has our Settings entry chosen (the app keeps its state up while it is).
pub fn lobby_settings(c: &ConfigUi) -> bool {
    c.m_entry == 1
}

/// The help for our page's cursor.
fn ours_help(s: &SettingsUi) -> String {
    if s.on_mods_page() { s.help() } else { s.view().help }
}

/// The title's Config (lobycwin) in the modern style: the list, then retail's page or our Settings (`s`). Its step
/// comes back: the sounds, the changed ids.
pub fn lobby(p: &mut Panels, c: &mut ConfigUi, v: &mut Values, mut s: Option<&mut SettingsUi>, sw: f32) -> configui::Out {
    let mut out = configui::Out::default();
    let Some(gu) = p.gu else { return out };
    let m = p.m;
    let entry = c.m_entry.min(LOBBY.len() - 1);
    let (open, row) = (c.m_open, c.m_row);
    let shapes = lobby_shapes();
    let t = texts(gu, c, v, &shapes);
    let ours = s.as_deref().filter(|_| lobby_settings(c));
    let follow = m.moved("lobby config", entry * 1_000_000 + ours.map_or(row, |o| o.view().row * 100 + o.view().tab + o.m_mods.0 * 10_000));
    let (mut sact, mut mact, mut click, mut hover) = (Vec::new(), Vec::new(), None, None);
    // Centred, by last frame's size.
    let screen = p.ctx.content_rect();
    let size = m.rects.borrow().get("lobby config").map_or(vec2(640.0, 360.0), |r| r.size());
    let at = (screen.center() - size / 2.0).round();
    p.panel_at("lobby config", Some(Title { icon: Some("icons/config"), text: "Config", note: None }), 640.0, at, |ui| {
        ui.horizontal_top(|ui| {
            entry_list(ui, m, &LOBBY, entry, open, &mut click);
            ui.add_space(16.0);
            let tall = ui.ctx().content_rect().height() * TALL;
            egui::ScrollArea::vertical().id_salt("lobby config page").max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(PAGE_W);
                    match ours {
                        Some(s) if s.on_mods_page() => mods_page(ui, m, s, open, follow, &mut mact),
                        Some(s) => settings_page(ui, m, &s.view(), open, follow, &mut sact),
                        None if lobby_settings(c) => {}
                        None => hover = native_rows(ui, m, gu, c, v, &shapes, &t, open.then_some(row), follow, sw, &mut click),
                    }
                })
            });
        });
        match ours {
            Some(s) => help_line(ui, m, &ours_help(s)),
            None => help_line(ui, m, hover.or(open.then_some(row)).and_then(|i| t.helps.get(i)).map_or("", |h| h.as_str())),
        }
    });
    if let Some(s) = s.as_deref_mut().filter(|_| !sact.is_empty() || !mact.is_empty()) {
        s.sounds.clear();
        settings_do(s, sact);
        mods_do(s, mact);
        out.sounds.append(&mut s.sounds);
        c.m_open = true;
    }
    match click {
        Some(Click::Entry(k)) => {
            (c.m_entry, c.m_open, c.m_row) = (k, true, 0);
            out.sounds.push(Se::Select);
        }
        Some(Click::Opt(i, x)) => {
            (c.m_open, c.m_row) = (true, i);
            let o = native_set(c, v, &shapes[i], x, sw);
            out.sounds.extend(o.sounds);
            out.changed.extend(o.changed);
        }
        Some(Click::Slide(i, x)) => {
            (c.m_open, c.m_row) = (true, i);
            out.changed.extend(native_set(c, v, &shapes[i], x, sw).changed);
        }
        Some(Click::Press(i)) => {
            (c.m_open, c.m_row) = (true, i);
            let o = native_press(c, v, &shapes[i], sw);
            out.sounds.extend(o.sounds);
            out.changed.extend(o.changed);
        }
        _ => {}
    }
    out
}

/// The entry list's keys (the page closed): Up / Down choose, Enter or Right open, Escape closes Config. The entry
/// opened, or true for closing.
fn entry_key(entry: &mut usize, n: usize, key: egui::Key) -> (kit::Act, bool) {
    let mut at = At::new(0, *entry);
    match key {
        egui::Key::ArrowRight | egui::Key::Enter => (kit::Act::Press(0, *entry), false),
        egui::Key::Escape => (kit::Act::Back, true),
        _ => {
            let act = kit::key(&[Stop::List { n, page: n }], &mut at, key, false);
            *entry = at.sub;
            (act, false)
        }
    }
}

/// The title's Config keys: the list's, then retail's page or ours, each the kit's.
pub fn lobby_key(c: &mut ConfigUi, v: &mut Values, s: Option<&mut SettingsUi>, key: egui::Key, shift: bool, sw: f32) -> configui::Out {
    let mut out = configui::Out::default();
    if !c.m_open {
        match entry_key(&mut c.m_entry, LOBBY.len(), key) {
            (_, true) => {
                out.closed = true;
                out.sounds.push(Se::Close);
            }
            (kit::Act::Press(..), _) => {
                (c.m_open, c.m_row) = (true, 0);
                out.sounds.push(Se::Select);
            }
            (a, _) => out.sounds.extend(a.sound()),
        }
        return out;
    }
    if lobby_settings(c) {
        if let Some(s) = s {
            if ours_key(s, key, shift) {
                c.m_open = false;
                out.sounds.push(Se::Cancel);
            }
            out.sounds.append(&mut s.sounds);
        }
        return out;
    }
    native_key(c, v, &lobby_shapes(), key, shift, sw)
}

/// A native page's keys through the kit; Escape goes back to the list.
fn native_key(c: &mut ConfigUi, v: &mut Values, all: &[Shape], key: egui::Key, shift: bool, sw: f32) -> configui::Out {
    let stops = native_stops(c, v, all, sw);
    let mut at = At::new(c.m_row.min(all.len().saturating_sub(1)), 0);
    let act = kit::key(&stops, &mut at, key, shift);
    c.m_row = at.stop;
    match act {
        kit::Act::Set(i, x) => native_set(c, v, &all[i], x, sw),
        kit::Act::Press(i, _) => native_press(c, v, &all[i], sw),
        kit::Act::Back => {
            c.m_open = false;
            configui::Out { sounds: vec![Se::Cancel], ..Default::default() }
        }
        a => configui::Out { sounds: a.sound().into_iter().collect(), ..Default::default() },
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
    let ours = w.settings.as_ref().filter(|s| settings_kind(entry) == Some(s.kind));
    let (mut sact, mut mact) = (Vec::new(), Vec::new());
    let shapes = match ENTRIES[entry].1 {
        Entry::Native(menus) => shapes(menus),
        _ => Vec::new(),
    };
    let v = &w.game;
    let t = texts(gu, c, v, &shapes);
    let cat = c.m_cat.min(3);
    let (list, cats): (Vec<(u16, usize)>, Vec<String>) = match ENTRIES[entry].1 {
        Entry::List(page, menu) => (configui2::modern_rows(v, page, cat), menu.map_or(Vec::new(), |m| categories(gu, m))),
        Entry::Colors => (Vec::new(), categories(gu, "conftxtc")),
        _ => (Vec::new(), Vec::new()),
    };
    // The cursor kept in view as it moves (only then, so the wheel stays free).
    let at = match ours {
        Some(s) if s.on_mods_page() => s.mods_state().and_then(|m| m.browse.as_ref()).map_or(s.m_mods.0 * 1000 + s.m_mods.1, |b| 900_000 + b.cursor),
        Some(s) => s.view().row * 100 + s.view().tab,
        None => c.m_keys.map_or(row, |_| 500_000 + c.m_key_row),
    };
    let follow = m.moved("config page", entry * 1_000_000 + at);
    let (keys, ask) = (c.m_keys, c.extra.ask_keys);
    let (mut click, mut hover) = (None, None);
    let mut help = String::new();
    p.panel("window", Some(Title { icon: Some("icons/config"), text: "Config", note: None }), 640.0, |ui| {
        ui.horizontal_top(|ui| {
            entry_list(ui, m, ENTRIES.map(|e| e.0).as_slice(), entry, open, &mut click);
            ui.add_space(16.0);
            let tall = ui.ctx().content_rect().height() * TALL;
            egui::ScrollArea::vertical().id_salt("config page").max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(PAGE_W);
                if let Some(yes) = ask {
                    let q = gu.group_b(0x8f).unwrap_or_else(|| "Reset keyboard assignment to default?".into());
                    if let Some(a) = kit::yes_no(ui, m, &q, ["Yes", "No"], yes, m.theme.small) {
                        click = Some(Click::Reset(a));
                    }
                    return;
                }
                if let Some(name) = keys {
                    keys_page(ui, m, gu, v, name, c.m_key_row, follow, &mut click);
                    help = "Up / Down: a function. Press a key to give it that key; its key again takes it off. Escape: back.".into();
                    return;
                }
                let first = !cats.is_empty() as usize;
                match ENTRIES[entry].1 {
                    Entry::List(page, _) => {
                        // The window's own help (its first element's).
                        if let Some(h) = gu.focus_text(&full(page), u16::MAX)[0].as_ref() {
                            ui.label(egui::RichText::new(h).size(m.theme.small - 1.5).color(m.theme.color("muted")));
                            ui.add_space(6.0);
                        }
                        if !cats.is_empty() {
                            let labels: Vec<&str> = cats.iter().map(String::as_str).collect();
                            if let Some(k) = kit::tab_strip(ui, m, "config cats", &labels, cat, open && row == 0) {
                                click = Some(Click::Cat(k));
                            }
                        }
                        list_rows(ui, m, gu, &list, (open && row >= first).then(|| row - first), follow, &mut click);
                        return;
                    }
                    Entry::Colors => {
                        let labels: Vec<&str> = cats.iter().map(String::as_str).collect();
                        if let Some(k) = kit::tab_strip(ui, m, "config cats", &labels, cat, open && row == 0) {
                            click = Some(Click::Cat(k));
                        }
                        colour_rows(ui, m, gu, c, v, configui2::colour_rows(cat), (open && row >= 1).then(|| row - 1), follow, &mut click);
                        help = "Up / Down: a colour and its hue, saturation and brightness; Left / Right change them. The mouse: the square and the strip.".into();
                        return;
                    }
                    _ => {}
                }
                match ours {
                    Some(s) if s.on_mods_page() => {
                        mods_page(ui, m, s, open, follow, &mut mact);
                        help = s.help();
                    }
                    Some(s) => {
                        settings_page(ui, m, &s.view(), open, follow, &mut sact);
                        help = s.view().help;
                    }
                    None if shapes.is_empty() => {
                        let what = match ENTRIES[entry].1 {
                            Entry::Extras => "The gameplay additions (not retail): the map overlay, the synthesis amount, timestamps, timers, the minimap.",
                            _ => "Window, resolution, frame limit and the other settings of the Windows Config tool.",
                        };
                        ui.label(egui::RichText::new(what).size(m.theme.small).color(m.theme.color("muted")));
                    }
                    None => {
                        hover = native_rows(ui, m, gu, c, v, &shapes, &t, open.then_some(row), follow, sw, &mut click);
                        help = hover.or(open.then_some(row)).and_then(|i| t.helps.get(i)).cloned().unwrap_or_default();
                    }
                }
            }));
        });
        help_line(ui, m, &help);
    });
    // A click on our page gives it the keys.
    if !sact.is_empty() || !mact.is_empty() {
        if let Some(s) = w.settings.as_mut() {
            s.sounds.clear();
            settings_do(s, sact);
            mods_do(s, mact);
            w.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
        }
        if let Some(c) = w.config.as_mut() {
            c.m_open = true;
        }
    }
    let Some(click) = click else { return };
    if let Some(c) = w.config.as_mut()
        && !matches!(click, Click::Entry(_))
    {
        c.m_open = true;
    }
    let mut out = configui::Out::default();
    match click {
        Click::Entry(k) => return choose(w, k),
        Click::Opt(i, x) => {
            if let Some(c) = w.config.as_mut() {
                c.m_row = i;
                out = native_set(c, &mut w.game, &shapes[i], x, sw);
            }
        }
        Click::Slide(i, x) => {
            if let Some(c) = w.config.as_mut() {
                c.m_row = i;
                out = native_set(c, &mut w.game, &shapes[i], x, sw);
                out.sounds.clear();
            }
        }
        Click::Press(i) => {
            if let Some(c) = w.config.as_mut() {
                c.m_row = i;
                out = native_press(c, &mut w.game, &shapes[i], sw);
            }
        }
        Click::Cat(k) => {
            if let Some(c) = w.config.as_mut() {
                (c.m_cat, c.m_row) = (k, 0);
            }
            out.sounds.push(Se::Cursor);
        }
        Click::Toggle(r) => {
            let first = matches!(ENTRIES[entry].1, Entry::List(_, Some(_))) as usize;
            if let Some(c) = w.config.as_mut() {
                c.m_row = r + first;
            }
            return toggle(w, entry, r);
        }
        Click::Colour(i, rgb) => {
            if let Some(&(_, _, id)) = configui2::colour_rows(cat).get(i) {
                configui2::set_colour(&mut w.game, &mut out, id, raw(rgb));
            }
        }
        Click::Hsv(i, s) => {
            if let Some(&(_, _, id)) = configui2::colour_rows(cat).get(i) {
                if let Some(c) = w.config.as_mut() {
                    c.m_hsv = Some((id, s));
                }
                configui2::set_colour(&mut w.game, &mut out, id, raw(kit::from_steps(s)));
            }
        }
        Click::Focus(k) => {
            if let Some(c) = w.config.as_mut() {
                c.m_row = 1 + k;
            }
            out.sounds.push(Se::Cursor);
        }
        Click::KeyRow(k) => {
            if let Some(c) = w.config.as_mut() {
                c.m_key_row = k;
            }
            out.sounds.push(Se::Cursor);
        }
        Click::Reset(yes) => {
            if let Some(c) = w.config.as_mut() {
                reset_answer(c, &mut w.game, yes, &mut out);
            }
        }
    }
    super::super::config_out(w, out);
}

/// A list row stepped (Enter, a click, Left / Right): retail's toggle, SE 0001.
fn toggle(w: &mut World, entry: usize, r: usize) {
    let Entry::List(page, _) = ENTRIES[entry].1 else { return };
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

/// The list's entry `k` taken: its page gets the keys (Extras / Settings ours: their state takes them from here).
fn choose(w: &mut World, k: usize) {
    let Some(c) = w.config.as_mut() else { return };
    (c.m_entry, c.m_open, c.m_row, c.m_keys) = (k, true, 0, None);
    let out = configui::Out { sounds: vec![Se::Select], ..Default::default() };
    super::super::config_out(w, out);
    settings_follow(w, k);
}

/// The keys while the page shows, every page the kit's: on the list Up / Down choose and Enter or Right open, Escape
/// closes Config; on a page Up / Down move between its stops, Left / Right change a value at once (Tab turns a page's
/// tabs), Enter steps an option or presses a button, Escape goes back to the list. True when taken.
pub fn key(w: &mut World, key: egui::Key, shift: bool) -> bool {
    if !w.modern || !shown(w) {
        return false;
    }
    let sw = w.ui_wh.0;
    let Some(c) = w.config.as_mut() else { return false };
    let n = ENTRIES.len();
    if !c.m_open {
        match entry_key(&mut c.m_entry, n, key) {
            (_, true) => {
                let out = configui::Out { closed: true, sounds: vec![Se::Close], ..Default::default() };
                c.stack.clear();
                super::super::config_out(w, out);
            }
            (kit::Act::Press(_, k), _) => choose(w, k),
            (a, _) => w.sounds.extend(a.sound().map(|s| s as u32)),
        }
        return true;
    }
    let entry = c.m_entry.min(n - 1);
    if c.extra.ask_keys.is_some() {
        let out = reset_key(c, &mut w.game, key);
        super::super::config_out(w, out);
        return true;
    }
    if c.m_keys.is_some() {
        let out = keys_key(c, &mut w.game, key);
        super::super::config_out(w, out);
        return true;
    }
    match ENTRIES[entry].1 {
        Entry::List(page, menu) => {
            let stops = list_stops(&w.game, page, menu.is_some(), c.m_cat);
            let first = menu.is_some() as usize;
            let mut at = At::new(c.m_row.min(stops.len().saturating_sub(1)), 0);
            let act = kit::key(&stops, &mut at, key, shift);
            c.m_row = at.stop;
            match act {
                kit::Act::Set(0, k) if first == 1 => {
                    (c.m_cat, c.m_row) = (k, 0);
                    w.sounds.push(Se::Cursor as u32);
                }
                kit::Act::Set(r, _) => toggle(w, entry, r - first),
                kit::Act::Back => {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                a => w.sounds.extend(a.sound().map(|s| s as u32)),
            }
        }
        Entry::Colors => {
            let stops = colour_stops(c, &w.game, c.m_cat);
            let mut at = At::new(c.m_row.min(stops.len() - 1), 0);
            let act = kit::key(&stops, &mut at, key, shift);
            c.m_row = at.stop;
            match act {
                kit::Act::Set(0, k) => {
                    (c.m_cat, c.m_row) = (k, 0);
                    w.sounds.push(Se::Cursor as u32);
                }
                kit::Act::Set(r, x) => {
                    let (i, ch) = ((r - 1) / 3, (r - 1) % 3);
                    if let Some(&(_, _, id)) = configui2::colour_rows(c.m_cat).get(i) {
                        let mut s = steps_of(c, &w.game, id);
                        s[ch] = x;
                        c.m_hsv = Some((id, s));
                        let mut out = configui::Out::default();
                        configui2::set_colour(&mut w.game, &mut out, id, raw(kit::from_steps(s)));
                        super::super::config_out(w, out);
                    }
                }
                kit::Act::Back => {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                a => w.sounds.extend(a.sound().map(|s| s as u32)),
            }
        }
        Entry::Extras | Entry::Settings => {
            if let Some(s) = w.settings.as_mut().filter(|s| settings_kind(entry) == Some(s.kind)) {
                if ours_key(s, key, shift) {
                    c.m_open = false;
                    s.sounds.push(Se::Cancel);
                }
                w.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
            }
        }
        Entry::Native(menus) => {
            let out = native_key(c, &mut w.game, &shapes(menus), key, shift, sw);
            super::super::config_out(w, out);
        }
    }
    true
}
