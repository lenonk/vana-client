//! The modern Settings window (ours, not retail; it replaces retail's Config list in the modern interface), over
//! client/configui.rs (retail's values, setters and windows) and client/settings.rs (ours): every setting of both on one
//! of fourteen pages by topic (Gameplay, Interface, Chat & Log, ... Mods) in groups down the left, each page's settings
//! under small headings, a row per setting with its options as buttons or a slider. Retail's settings keep the captions
//! read from their Config page's layout (a few made clearer). Chat Filters, Effects, Font Colors, Mods and the Lua mods'
//! settings are pages of their own opened from a button on their topic's page (Escape or Back returns), and key
//! assignment is a list in the page. Left out as before: the Windows and Log pages (the log windows' frame, size and
//! width mean nothing with the modern chat; user, 2026-10-26) and Mouse/Cam.'s mouse rows. Every page is the widget
//! kit's (kit.rs): its controls and keys.

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

/// What a page is: one of the topic pages, or a page opened from one.
#[derive(Clone, Copy)]
enum Entry {
    Topic,
    /// A list page (configui2's rows and toggles), with the menu whose rows name its categories.
    List(&'static str, Option<&'static str>),
    /// Font Colors: the categories (conftxtc's rows) of colours.
    Colors,
    /// Ours: the Mods page (Settings), and the Lua mods' own pages (Extras after its three).
    Mods,
    ModPages,
}

/// The pages opened from a topic page's button: (name, page, help).
const SUBS: [(&str, Entry, &str); 5] = [
    ("Chat Filters", Entry::List("cfilter", None), "Choose which kinds of chat and battle messages show in the log."),
    ("Effects", Entry::List("fxfilter", None), "Choose whose visual effects you see: yours, your party's and other players'."),
    ("Font Colors", Entry::Colors, "Choose the colour of each kind of chat and battle message."),
    ("Mods", Entry::Mods, "Turn mod packs on or off, change their order, and install or remove them."),
    ("Mod Settings", Entry::ModPages, "Settings for your Lua mods, one page per mod."),
];
const SUB_CHAT: usize = 0;
const SUB_EFFECTS: usize = 1;
const SUB_COLOURS: usize = 2;
const SUB_MODS: usize = 3;
const SUB_MOD_PAGES: usize = 4;

/// A topic page's line: a small heading, retail's setting (its Config page and first control), ours (its row's
/// label in client/settings.rs), or a button opening a page of SUBS.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Head(&'static str),
    Retail(&'static str, u16),
    Ours(&'static str),
    Open(usize),
}
use Item::{Head, Open, Ours, Retail};

/// The topic pages: (group, name, lines). Every retail setting of the old list and every row of ours has one place
/// (the test `every_setting_has_a_page`).
const PAGES: [(&str, &str, &[Item]); 14] = [
    ("Game", "Gameplay", &[
        Head("Targeting"), Retail("conf2win", 1), Retail("conf12wi", 7), Retail("conf12wi", 5),
        Head("Inventory & Equipment"), Retail("conf2win", 8), Retail("conf2win", 10), Retail("conf12wi", 3), Ours("Synthesis Amount"),
        Head("Events & Trusts"), Ours("Text Auto-Advance"), Ours("Trust Speech"),
    ]),
    ("Game", "Interface", &[
        Head("Look"), Ours("Interface"), Ours("Interface Size"), Ours("Panel Opacity"), Ours("Panel Places"), Retail("conf3win", 12),
        Head("Party & Target"), Ours("Party TP"), Retail("conf12wi", 9), Retail("conf12wi", 11), Ours("Target Distance"), Ours("Target HP %"),
        Head("Other"), Retail("conf3win", 3), Ours("Clock"), Retail("conf6win", 12), Retail("conf6win", 14),
    ]),
    ("Game", "Chat & Log", &[
        Head("Filters"), Open(SUB_CHAT), Retail("conf13wi", 4), Retail("conf4", 1), Retail("conf3win", 1),
        Head("Colours"), Open(SUB_COLOURS), Ours("Battle Names"),
        Head("Lines"), Ours("Battle Text"), Ours("Ready Lines"), Ours("Timestamps on All Lines"),
    ]),
    ("Game", "Maps & Timers", &[
        Head("Maps"), Ours("Map Overlay"), Ours("Map Overlay Opacity"), Ours("Minimap"), Ours("Minimap Position"), Ours("Minimap Size"),
        Head("Timers & Helpers"), Ours("Timers"), Ours("Timers Position"), Ours("Training Regime"), Ours("Skillchain Helper"),
    ]),
    ("Graphics", "Display", &[
        Head("Window"), Ours("Window Mode"), Ours("Resolution"), Ours("Monitor"), Ours("GPU"), Ours("Frame Limit"),
        Head("Scaling"), Ours("Background"), Ours("Menu Size"), Ours("3D Aspect"), Retail("conf6win", 7), Ours("3D LCD Mode"),
        Head("Brightness"), Ours("Gamma Base"), Retail("conf2win", 5), Retail("conf2win", 6), Retail("conf2win", 7),
    ]),
    ("Graphics", "Image Quality", &[
        Head("Textures"), Ours("Textures"), Ours("2-D Mapping"), Ours("Fonts"), Ours("MIP Mapping"), Ours("Anisotropic Filtering"),
        Head("Edges & Stability"), Ours("Graphics Stabilization"),
    ]),
    ("Graphics", "Lighting & Shadows", &[
        Head("Lights"), Ours("Lamp Lights"), Ours("Bump Mapping"),
        Head("Shadows"), Retail("conf3win", 6),
    ]),
    ("Graphics", "Characters & World", &[
        Head("Characters"), Retail("conf3win", 11), Retail("conf3win", 14), Retail("conf6win", 4), Retail("conf6win", 2),
        Head("World"), Ours("Draw Distance"), Retail("conf6win", 1), Retail("conf3win", 9), Retail("conf12wi", 13),
    ]),
    ("Graphics", "Effects & Colour", &[
        Head("World"), Ours("Environment Animation"),
        Head("Effects"), Retail("conf12wi", 1), Open(SUB_EFFECTS),
    ]),
    ("Sound", "Sound", &[
        Head("Volume"), Retail("conf2win", 3), Retail("conf2win", 4), Ours("Music Volume"), Ours("Sound Effects Volume"),
        Head("Output"), Ours("Sound"), Ours("Sound Effect Channels"), Ours("Sound in Background"),
    ]),
    ("Input", "Mouse, Camera & Keys", &[
        Head("Mouse"), Ours("Hardware Mouse"), Ours("Cursor Size"),
        Head("Camera"), Retail("conf7", 5), Retail("conf7", 7), Retail("conf7", 9), Retail("conf7", 11), Retail("conf7", 13),
        Retail("conf7", configui::CAM_KEYS), Retail("conf7", configui::CAM_KEYS + 3),
        Head("Keyboard"), Retail("conf6win", 5), Retail("conf6win", 8), Retail("conf6win", 9), Retail("conf6win", 10), Retail("conf13wi", 1),
    ]),
    ("Input", "Controller", &[Head("Controller"), Ours("Gamepad"), Ours("Stick Dead Zone")]),
    ("System", "Startup & Account", &[
        Head("Startup"), Ours("Opening Movie"), Ours("Simple Character Creation"), Ours("Screenshot Size"),
        Head("Account"), Retail("conf4", 3),
    ]),
    ("System", "Mods", &[Head("Mods"), Open(SUB_MODS), Open(SUB_MOD_PAGES)]),
];

/// The page shown: a page opened from a topic page while one is, else the topic page chosen.
fn current(c: &ConfigUi) -> Entry {
    c.m_sub.and_then(|k| SUBS.get(k)).map_or(Entry::Topic, |s| s.1)
}

/// Retail's setting `child` of Config page `menu` (its first control).
fn shape_of(menu: &'static str, child: u16) -> Option<Shape> {
    shapes(&[menu]).into_iter().find(|s| s.ctls[0].0 == child)
}

/// A topic page's lines as they show: each with its retail shape (Retail) or our row and help (Ours); ours left out
/// here (in game, the modern interface) and headings left with nothing under them go.
struct Line {
    item: Item,
    shape: Option<Shape>,
    ours: Option<(settings::VRow, String)>,
}

#[cfg(test)]
fn lines(page: usize, s: Option<&SettingsUi>) -> Vec<Line> {
    lines_of(PAGES[page.min(PAGES.len() - 1)].2, s)
}

fn lines_of(items: &[Item], s: Option<&SettingsUi>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for &item in items {
        let line = match item {
            Retail(menu, child) => match shape_of(menu, child) {
                Some(sh) => Line { item, shape: Some(sh), ours: None },
                None => continue,
            },
            Ours(label) => match s.and_then(|s| s.item(label)) {
                Some(o) => Line { item, shape: None, ours: Some(o) },
                None => continue,
            },
            _ => Line { item, shape: None, ours: None },
        };
        if matches!(item, Head(_)) && out.last().is_some_and(|l| matches!(l.item, Head(_))) {
            out.pop();
        }
        out.push(line);
    }
    if out.last().is_some_and(|l| matches!(l.item, Head(_))) {
        out.pop();
    }
    out
}

/// What the topic area shows: the chosen page, or while a search is typed its hits under their pages' names; Retail
/// only leaves ours out (the mods' pages too). The hits stay as found while the text does, so a setting changed in
/// them stays put even when its help then reads otherwise.
fn view_items(c: &ConfigUi) -> Vec<Item> {
    let mut items: Vec<Item> = match c.m_find.as_ref().filter(|f| !f.trim().is_empty()) {
        Some(_) => {
            let mut out = Vec::new();
            let mut page = usize::MAX;
            for &(p, i) in &c.m_hits.1 {
                if p != page {
                    out.push(Head(PAGES[p].1));
                    page = p;
                }
                out.push(PAGES[p].2[i]);
            }
            out
        }
        None => PAGES[c.m_entry.min(PAGES.len() - 1)].2.to_vec(),
    };
    if c.m_retail {
        items.retain(|i| !matches!(i, Ours(_) | Open(SUB_MODS | SUB_MOD_PAGES)));
    }
    items
}

fn view(c: &ConfigUi, s: Option<&SettingsUi>) -> Vec<Line> {
    lines_of(&view_items(c), s)
}

/// Whether `text` matches the search: anywhere in a setting's name, or a word of its help starting with it (3
/// letters or more, so a short search is not every line).
fn matches(q: &str, name: &str, help: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return false;
    }
    name.to_lowercase().contains(&q) || (q.chars().count() >= 3 && help.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)))
}

/// The search's hits over every page, (page, line): a setting whose name or help matches, or every setting under a
/// heading that does. Retail's words need the game's layouts (`gu`; none offscreen).
fn find_hits(gu: Option<&GameUi>, c: &ConfigUi, v: &Values, s: Option<&SettingsUi>, q: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (p, page) in PAGES.iter().enumerate() {
        let mut head = false;
        for (i, &item) in page.2.iter().enumerate() {
            let hit = match item {
                Head(h) => {
                    head = matches(q, h, "");
                    continue;
                }
                Retail(menu, child) => match (gu, shape_of(menu, child)) {
                    (Some(gu), Some(sh)) => {
                        let t = texts(gu, c, v, std::slice::from_ref(&sh));
                        matches(q, &t.heads[0], &t.helps[0])
                    }
                    _ => false,
                },
                Ours(label) => s.and_then(|s| s.item(label)).is_some_and(|(_, help)| matches(q, label, &help)),
                Open(sub) => matches(q, SUBS[sub].0, SUBS[sub].2),
            };
            if hit || head {
                out.push((p, i));
            }
        }
    }
    out
}

/// The search's hits brought up to date with its text.
fn refresh_hits(gu: Option<&GameUi>, c: &mut ConfigUi, v: &Values, s: Option<&SettingsUi>) {
    let q = c.m_find.clone().unwrap_or_default();
    if c.m_hits.0 != q || (q.is_empty() && !c.m_hits.1.is_empty()) {
        c.m_hits = (q.clone(), if q.trim().is_empty() { Vec::new() } else { find_hits(gu, c, v, s, &q) });
    }
}

/// The search has text being typed (see `find_events`): its keys are no game keys.
pub fn finding(w: &World) -> bool {
    w.modern && shown(w) && w.config.as_ref().is_some_and(|c| c.m_find.is_some() && c.m_sub.is_none() && c.m_keys.is_none() && c.extra.ask_keys.is_none())
}

/// The search's typed text, Backspace, Escape and Ctrl+R (Retail only) while the Settings window shows a topic page.

/// True when it took them (the game then sees none).
pub fn find_events(w: &mut World, events: &[egui::Event]) -> bool {
    if !find_open(w) {
        return false;
    }
    let Some(c) = w.config.as_mut() else { return false };
    let mut took = false;
    for e in events {
        match e {
            egui::Event::Text(t) if c.m_find.is_some() || t.chars().next().is_some_and(|ch| ch.is_alphanumeric()) => {
                c.m_find.get_or_insert_with(String::new).extend(t.chars().filter(|ch| !ch.is_control()));
                (c.m_open, c.m_row) = (true, 0);
                took = true;
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } if c.m_find.is_some() => {
                if let Some(f) = c.m_find.as_mut() {
                    f.pop();
                }
                if c.m_find.as_ref().is_some_and(|f| f.is_empty()) {
                    c.m_find = None;
                }
                c.m_row = 0;
                took = true;
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } if c.m_find.is_some() => {
                (c.m_find, c.m_row) = (None, 0);
                w.sounds.push(Se::Cancel as u32);
                took = true;
            }
            egui::Event::Key { key: egui::Key::R, pressed: true, modifiers, .. } if modifiers.ctrl || modifiers.command => {
                (c.m_retail, c.m_row) = (!c.m_retail, 0);
                w.sounds.push(Se::Cursor as u32);
                took = true;
            }
            _ => {}
        }
    }
    took
}

/// The lines the cursor stops on (all but the headings).
fn stop_lines(all: &[Line]) -> Vec<&Line> {
    all.iter().filter(|l| !matches!(l.item, Head(_))).collect()
}

/// A topic page's stops: retail's as its page had them, ours as our page, a button for a page opened from here.
fn topic_stops(c: &ConfigUi, v: &Values, all: &[Line], sw: f32) -> Vec<Stop> {
    stop_lines(all)
        .iter()
        .map(|l| match (&l.shape, &l.ours) {
            (Some(sh), _) => native_stops(c, v, std::slice::from_ref(sh), sw)[0],
            (_, Some((r, _))) => match &r.ctl {
                settings::VCtl::Buttons(bs) => Stop::Value { n: bs.len(), at: bs.iter().position(|b| b.1).unwrap_or(0), slider: false, off: 0 },
                settings::VCtl::Slider { n, at, .. } => Stop::Value { n: *n, at: *at, slider: true, off: 0 },
            },
            _ => Stop::Buttons { n: 1, off: 0, pair: None },
        })
        .collect()
}

/// What the mouse or a key did on a topic page: stop `k`'s option or slider stop `x` (`click`: by the pointer, so a
/// slider makes no sound), or its button.
#[derive(Clone, Copy, Debug)]
enum Pick {
    Set(usize, usize, bool),
    Press(usize),
}

/// A topic page's pick carried out.
fn topic_do(w: &mut World, pick: Pick, sw: f32) {
    let Some(c) = w.config.as_ref() else { return };
    let all = view(c, w.settings.as_ref());
    let k = match pick {
        Pick::Set(k, ..) | Pick::Press(k) => k,
    };
    let Some(l) = stop_lines(&all).get(k).map(|l| (l.item, l.shape.as_ref().map(|s| (s.menu, s.ctls.clone())))) else { return };
    if let Some(c) = w.config.as_mut() {
        (c.m_open, c.m_row) = (true, k);
    }
    match (l.0, pick) {
        (Retail(..), _) => {
            let Some((menu, ctls)) = l.1 else { return };
            let sh = Shape { menu, ctls };
            let Some(c) = w.config.as_mut() else { return };
            let out = match pick {
                Pick::Set(_, x, click) => {
                    let mut out = native_set(c, &mut w.game, &sh, x, sw);
                    if click && matches!(sh.ctls[0].1, Ctl::Slider(..)) {
                        out.sounds.clear();
                    }
                    out
                }
                Pick::Press(_) => native_press(c, &mut w.game, &sh, sw),
            };
            super::super::config_out(w, out);
        }
        (Ours(label), Pick::Set(_, x, click)) => {
            if let Some(s) = w.settings.as_mut() {
                s.item_set(label, x, click);
                w.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
            }
        }
        (Open(sub), _) => open_sub(w, sub),
        _ => {}
    }
}

/// A page of SUBS opened from its topic page.
fn open_sub(w: &mut World, sub: usize) {
    let Some(c) = w.config.as_mut() else { return };
    (c.m_sub, c.m_open, c.m_row, c.m_cat) = (Some(sub), true, 0, 0);
    w.sounds.push(Se::Select as u32);
    settings_follow(w);
}

/// Back from a page of SUBS to its topic page, the cursor on its button.
fn close_sub(w: &mut World) {
    let Some(c) = w.config.as_mut() else { return };
    let Some(sub) = c.m_sub.take() else { return };
    let all = view(c, w.settings.as_ref());
    c.m_row = stop_lines(&all).iter().position(|l| l.item == Open(sub)).unwrap_or(0);
    w.sounds.push(Se::Cancel as u32);
    if let Some(s) = w.settings.as_mut().filter(|s| s.on_mods_page()) {
        s.goto(0, 0);
    }
    settings_follow(w);
}

/// `/lua settings`: the Settings window opened on the Lua mods' pages (the app turns Extras to the mod's).
pub fn open_mod_settings(c: &mut ConfigUi) {
    let page = PAGES.iter().position(|p| p.2.contains(&Open(SUB_MOD_PAGES))).unwrap_or(0);
    (c.m_entry, c.m_sub, c.m_open, c.m_row) = (page, Some(SUB_MOD_PAGES), true, 0);
}

/// A small heading over a group of a topic page's settings.
fn group_head(ui: &mut Ui, m: &super::Modern, text: &str, first: bool) {
    if !first {
        ui.add_space(10.0);
    }
    ui.label(egui::RichText::new(text.to_uppercase()).size(m.theme.small - 2.0).color(m.theme.color("bronze")).family(FontFamily::Name(super::STRONG.into())));
    ui.add_space(2.0);
}

/// A topic page: its headings and settings, the cursor's ringed while it has the keys. What the mouse did, and the
/// help of the line under the pointer.
#[allow(clippy::too_many_arguments)]
fn topic_page(ui: &mut Ui, m: &super::Modern, gu: Option<&GameUi>, c: &ConfigUi, v: &Values, all: &[Line], focus: Option<usize>, follow: bool, sw: f32, pick: &mut Option<Pick>) -> Option<usize> {
    let stops = topic_stops(c, v, all, sw);
    let shapes: Vec<Shape> = all.iter().filter_map(|l| l.shape.clone()).collect();
    // Retail's words come from the game's layouts (none offscreen, `modern-shot`: its lines are left out there).
    let t = gu.map(|gu| texts(gu, c, v, &shapes));
    let (mut k, mut ret, mut hover) = (0usize, 0usize, None);
    for (i, l) in all.iter().enumerate() {
        if let Head(h) = l.item {
            group_head(ui, m, h, i == 0);
            continue;
        }
        let on = focus == Some(k);
        let r = match (&l.shape, &l.ours, l.item) {
            (Some(sh), ..) => {
                let (Some(gu), Some(t)) = (gu, t.as_ref()) else { continue };
                let (got, r) = native_row(ui, m, gu, v, sh, &t.heads[ret], &t.labels[ret], stops[k], on, follow);
                ret += 1;
                match got {
                    Some(Got::Opt(x)) => *pick = Some(Pick::Set(k, x, false)),
                    Some(Got::Slide(x)) => *pick = Some(Pick::Set(k, x, true)),
                    Some(Got::Press) => *pick = Some(Pick::Press(k)),
                    None => {}
                }
                r
            }
            (_, Some((row, _)), _) => match &row.ctl {
                settings::VCtl::Buttons(bs) => {
                    kit::setting(ui, m, &row.label, on, true, follow, |ui| {
                        let labels: Vec<String> = bs.iter().map(|b| b.0.clone()).collect();
                        if let Some(x) = kit::choice(ui, m, &labels, bs.iter().position(|b| b.1).unwrap_or(usize::MAX), 0) {
                            *pick = Some(Pick::Set(k, x, false));
                        }
                    })
                    .1
                }
                settings::VCtl::Slider { n, at, shown } => {
                    let (x, r) = kit::slider_setting(ui, m, &row.label, on, follow, *n, *at, shown);
                    if let Some(x) = x {
                        *pick = Some(Pick::Set(k, x, true));
                    }
                    r
                }
            },
            (_, _, Open(sub)) => {
                kit::setting(ui, m, SUBS[sub].0, on, false, follow, |ui| {
                    if kit::buttons(ui, m, &["Open"], on.then_some(0), 0, 0).is_some() {
                        *pick = Some(Pick::Press(k));
                    }
                })
                .1
            }
            _ => continue,
        };
        if ui.rect_contains_pointer(r) {
            hover = Some(k);
        }
        k += 1;
    }
    hover
}

/// The help of a topic page's stop `k`.
fn topic_help(gu: &GameUi, c: &ConfigUi, v: &Values, all: &[Line], k: usize) -> String {
    let Some(l) = stop_lines(all).get(k).copied() else { return String::new() };
    match (&l.shape, &l.ours, l.item) {
        (Some(sh), ..) => texts(gu, c, v, std::slice::from_ref(sh)).helps.pop().unwrap_or_default(),
        (_, Some((_, help)), _) => help.clone(),
        (_, _, Open(sub)) => SUBS[sub].2.to_string(),
        _ => String::new(),
    }
}

/// The details column's width.
const DETAIL_W: f32 = 230.0;

/// What the details column says: the setting's name, what it does, and short facts (retail's or ours, its page).
#[derive(Default)]
struct Detail {
    name: String,
    text: String,
    facts: Vec<(&'static str, String)>,
}

impl Detail {
    /// No setting has the cursor: the page (or the search).
    fn page(name: &str, find: bool) -> Self {
        let text = if find { "Pick a setting to see what it does. Escape clears the search." } else { "Pick a setting to see what it does." };
        Detail { name: if find { "Search".into() } else { name.into() }, text: text.into(), facts: Vec::new() }
    }
}

/// The details of a topic page's stop `k`; while searching, the page it is on too (the heading over it).
fn topic_detail(gu: Option<&GameUi>, c: &ConfigUi, v: &Values, all: &[Line], k: usize, find: bool) -> Detail {
    let (mut page, mut n) = ("", 0usize);
    for l in all {
        match l.item {
            Head(h) => page = h,
            _ if n == k => {
                let (name, retail) = match (&l.shape, &l.ours, l.item) {
                    (Some(sh), ..) => (gu.map(|gu| heading(gu, sh)).unwrap_or_default(), true),
                    (_, Some((r, _)), _) => (r.label.clone(), false),
                    (_, _, Open(sub)) => (SUBS[sub].0.to_string(), sub < SUB_MODS),
                    _ => (String::new(), false),
                };
                let text = match gu {
                    Some(gu) => topic_help(gu, c, v, all, k),
                    None => topic_help_ours(all, k),
                };
                let mut facts = vec![("Retail", if retail { "Yes" } else { "No (Vana Client)" }.to_string())];
                if find {
                    facts.push(("Page", page.to_string()));
                }
                return Detail { name, text, facts };
            }
            _ => n += 1,
        }
    }
    Detail::default()
}

/// The details column: the setting's name, its help, its facts.
fn details(ui: &mut Ui, m: &super::Modern, d: &Detail) {
    ui.vertical(|ui| {
        ui.set_width(DETAIL_W);
        group_head(ui, m, "Details", true);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(&d.name).size(m.theme.ui).color(m.theme.color("gold")));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(&d.text).size(m.theme.small).color(m.theme.color("text")));
        ui.add_space(8.0);
        for (k, v) in &d.facts {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(*k).size(m.theme.small - 1.0).color(m.theme.color("muted")));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(v).size(m.theme.small - 1.0).color(m.theme.color("text")));
            });
        }
    });
}

/// The search field over a topic page (typing fills it) and the Retail only button; true when that was clicked.
fn find_bar(ui: &mut Ui, m: &super::Modern, find: Option<&str>, retail: bool, width: f32) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        let text = find.unwrap_or("");
        let font = FontId::proportional(m.theme.ui);
        let r = kit::field(ui, m, text, find.map(|f| f.chars().count()), width - 128.0, font.clone(), Some("controls/search"));
        if find.is_none() {
            ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, "Type to search", font, m.theme.color("muted"));
        }
        ui.add_space(8.0);
        clicked = kit::button(ui, m, "Retail only", 120.0, Btn { lit: retail, ..Default::default() }).clicked();
    });
    ui.add_space(6.0);
    clicked
}

/// A topic page's keys, for the line at the bottom.
fn topic_keys(find: bool) -> &'static str {
    if find {
        "Up / Down: a setting. Left / Right: change it. Backspace: delete a letter. Escape: clear the search."
    } else {
        "Up / Down: a setting. Left / Right: change it. Type to search. Ctrl+R: Retail only."
    }
}

/// The page's width and its scroll area's share of the screen's height.
const PAGE_W: f32 = 440.0;
const TALL: f32 = 0.72;

/// A setting: its page and controls (its options, one slider, or one button).
#[derive(Clone)]
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
                Ctl::Act(Act::CamKeys(axis, _)) => Some(u64::MAX - 1 - axis as u64),
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
const HEADINGS: [(&str, u16, &str, &str); 11] = [
    ("conf2win", 5, "Gamma Adjustment: Red", ""),
    ("conf2win", 6, "Gamma Adjustment: Green", ""),
    ("conf2win", 7, "Gamma Adjustment: Blue", ""),
    ("conf2win", 8, "Inventory: Auto-sort", ""),
    ("conf2win", 10, "Inventory: Sort Type", "Type 1 sorts by category. Type 2 keeps the order and gathers copies of an item beside the first."),
    ("conf7", 7, "Camera Behind You: Up / Down", "Inverts the up / down turn of the camera behind you (the mouse and the camera keys)."),
    ("conf7", 9, "Camera Behind You: Left / Right", "Inverts the left / right turn of the camera behind you (the mouse and the camera keys)."),
    ("conf7", 11, "First Person: Up / Down", "Inverts looking up and down in first person (the mouse and the camera keys)."),
    ("conf7", 13, "First Person: Left / Right", "Inverts looking left and right in first person (the mouse and the camera keys)."),
    ("conf7", configui::CAM_KEYS, "Camera Keys: Up / Down", "The camera keys' up / down: Same as the rows above (as the mouse), or Normal or Inverted for the keys alone, in both views."),
    ("conf7", configui::CAM_KEYS + 3, "Camera Keys: Left / Right", "The camera keys' left / right: Same as the rows above (as the mouse), or Normal or Inverted for the keys alone, in both views."),
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
        Ctl::Act(Act::CamKeys(axis, x)) => c.cam_keys[axis as usize & 1] == x,
        _ => false,
    }
}

/// The page shows (the list's window, configwi, on top).
fn shown(w: &World) -> bool {
    w.config.as_ref().is_some_and(|c| c.top().is_some_and(|t| t.name == "configwi") && !c.covered)
}

/// The search takes a typed letter or digit now (see `find_events`).
pub fn find_open(w: &World) -> bool {
    w.modern && shown(w) && !w.typing && w.config.as_ref().is_some_and(|c| c.m_sub.is_none() && c.m_keys.is_none() && c.extra.ask_keys.is_none())
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
    /// A page opened from a topic page: back to it.
    Back,
    /// The Settings window's Retail only toggled.
    Retail,
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
            Ctl::Pick(..) | Ctl::Bit(..) | Ctl::Act(Act::Anon(_) | Act::CamKeys(..)) => {
                let off = s.ctls.iter().enumerate().filter(|(_, (k, _))| c.off(s.menu, *k, sw)).fold(0u64, |m, (j, _)| m | 1 << j);
                Stop::Value { n: s.ctls.len(), at: s.ctls.iter().position(|(_, ctl)| holds(c, v, *ctl)).unwrap_or(0), slider: false, off }
            }
            _ => Stop::Buttons { n: 1, off: c.off(s.menu, s.ctls[0].0, sw) as u64, pair: None },
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
fn ours_stops(v: &settings::View, skip: usize) -> Vec<Stop> {
    let tabs = (!v.tabs.is_empty()).then_some(Stop::Tabs { n: v.tabs.len().saturating_sub(skip).max(1), at: v.tab.saturating_sub(skip) });
    let rows = v.rows.iter().map(|r| match &r.ctl {
        settings::VCtl::Buttons(bs) => Stop::Value { n: bs.len(), at: bs.iter().position(|b| b.1).unwrap_or(0), slider: false, off: 0 },
        settings::VCtl::Slider { n, at, .. } => Stop::Value { n: *n, at: *at, slider: true, off: 0 },
    });
    tabs.into_iter().chain(rows).collect()
}

/// The tabs a page of ours shows (`skip`: the Settings window leaves the ones before its own out) and the one on.
fn shown_tabs(v: &settings::View, skip: usize) -> (Vec<&str>, usize) {
    (v.tabs.iter().skip(skip).map(String::as_str).collect(), v.tab.saturating_sub(skip))
}

/// The Mods page's stops: the tabs, Install from File, each pack's toggle / up / down / uninstall, each archive's
/// Install; the picker's list while it is open.
fn mods_stops(s: &SettingsUi, skip: usize) -> Vec<Stop> {
    if let Some(b) = s.mods_state().and_then(|m| m.browse.as_ref()) {
        return vec![Stop::List { n: b.entries.len(), page: 10 }];
    }
    let (packs, archives) = s.mods_rows();
    let busy = s.mods_state().is_some_and(|m| m.installing().is_some());
    let n = packs.len();
    let mut out = vec![Stop::Tabs { n: s.view().tabs.len().saturating_sub(skip).max(1), at: s.view().tab.saturating_sub(skip) }, Stop::Buttons { n: 1, off: busy as u64, pair: None }];
    out.extend((0..n).map(|i| Stop::Buttons { n: 4, off: ((i == 0) as u64) << 1 | ((i + 1 == n) as u64) << 2, pair: Some(1) }));
    out.extend(archives.iter().map(|_| Stop::Buttons { n: 1, off: busy as u64, pair: None }));
    out
}

/// A Mods page act; true when the page is left (Escape).
fn mods_act(s: &mut SettingsUi, act: kit::Act, at: At, skip: usize) -> bool {
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
            s.m_tab(t + skip);
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
fn ours_act(s: &mut SettingsUi, v: &settings::View, act: kit::Act, at: At, skip: usize) -> bool {
    let first = (!v.tabs.is_empty()) as usize;
    match act {
        kit::Act::Back => return true,
        kit::Act::Moved => {
            s.m_point(at.stop + 1 - first, at.sub);
            s.sounds.push(Se::Cursor);
        }
        kit::Act::Set(0, t) if first == 1 => s.m_tab(t + skip),
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

/// Our page's keys (Settings, Extras, Mods); true when the page is left. `skip`: the tabs left out (shown_tabs).
fn ours_key(s: &mut SettingsUi, key: egui::Key, shift: bool, skip: usize) -> bool {
    s.sounds.clear();
    if s.m_remove.is_some() {
        remove_key(s, key);
        return false;
    }
    if s.on_mods_page() {
        let stops = mods_stops(s, skip);
        let mut at = match s.mods_state().and_then(|m| m.browse.as_ref()) {
            Some(b) => At::new(0, b.cursor),
            None => At::new(s.m_mods.0, s.m_mods.1),
        };
        let act = kit::key(&stops, &mut at, key, shift);
        return mods_act(s, act, at, skip);
    }
    let v = s.view();
    let stops = ours_stops(&v, skip);
    let first = (!v.tabs.is_empty()) as usize;
    let mut at = At::new((v.row + first).saturating_sub(1), v.col);
    // Row 0 is the page name (the tabs) in SettingsUi; the kit's stops start there too when there are tabs.
    if first == 1 {
        at.stop = v.row;
    }
    let act = kit::key(&stops, &mut at, key, shift);
    ours_act(s, &v, act, at, skip)
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
/// The entry of our Extras page.
/// Our state follows the page: Settings' (our rows on the topic pages, the Mods page) or Extras' for the Lua mods'
/// pages, asked for while it is another (the app makes it); turned to the Mods page or the first mod's while one shows.
fn settings_follow(w: &mut World) {
    let Some(c) = w.config.as_ref() else { return };
    let page = current(c);
    let kind = if matches!(page, Entry::ModPages) { settings::Kind::Extras } else { settings::Kind::Settings };
    match w.settings.as_mut() {
        Some(s) if s.kind == kind => match page {
            Entry::Mods if !s.on_mods_page() => s.goto_mods(),
            Entry::ModPages if !s.mod_pages.is_empty() && s.view().tab < 3 => s.goto_mods(),
            _ => {}
        },
        _ => w.want_settings = Some(kind),
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

/// What the mouse did on a native setting: its option taken, its slider set, its button pressed.
enum Got {
    Opt(usize),
    Slide(usize),
    Press,
}

/// A native setting: its heading and its options as buttons, its slider, or its button; ringed with the cursor.
#[allow(clippy::too_many_arguments)]
fn native_row(ui: &mut Ui, m: &super::Modern, gu: &GameUi, v: &Values, s: &Shape, head: &str, labels: &[String], stop: Stop, focus: bool, follow: bool) -> (Option<Got>, Rect) {
    let mut got = None;
    let r = match (s.ctls[0].1, stop) {
        (Ctl::Slider(id, ..), Stop::Value { n, at, .. }) => {
            // The aspect ratios by name, as retail's text under the slider.
            let val = v.get_i(id);
            let shown = match id {
                0x43 | 0xab => configui::ASPECTS.get(val.max(0) as usize).map_or(String::new(), |a| a.0.trim_matches(['(', ')']).to_string()),
                _ => val.to_string(),
            };
            let (x, r) = kit::slider_setting(ui, m, head, focus, follow, n, at, &shown);
            got = x.map(Got::Slide);
            r
        }
        (_, Stop::Value { at, off, .. }) => {
            kit::setting(ui, m, head, focus, true, follow, |ui| {
                got = kit::choice(ui, m, labels, at, off).map(Got::Opt);
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
            })
            .1
        }
        (_, stop) => {
            kit::setting(ui, m, head, focus, false, follow, |ui| {
                let off = matches!(stop, Stop::Buttons { off: 1, .. }) as u64;
                if kit::buttons(ui, m, &[labels[0].as_str()], focus.then_some(0), 0, off).is_some() {
                    got = Some(Got::Press);
                }
            })
            .1
        }
    };
    (got, r)
}

/// A native page's settings (the title's Config); the setting under the pointer (for the help line).
#[allow(clippy::too_many_arguments)]
fn native_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, c: &ConfigUi, v: &Values, shapes: &[Shape], t: &Texts, focus_row: Option<usize>, follow: bool, sw: f32, click: &mut Option<Click>) -> Option<usize> {
    let stops = native_stops(c, v, shapes, sw);
    let mut hover = None;
    for (i, s) in shapes.iter().enumerate() {
        let (got, r) = native_row(ui, m, gu, v, s, &t.heads[i], &t.labels[i], stops[i], focus_row == Some(i), follow);
        match got {
            Some(Got::Opt(x)) => *click = Some(Click::Opt(i, x)),
            Some(Got::Slide(x)) => *click = Some(Click::Slide(i, x)),
            Some(Got::Press) => *click = Some(Click::Press(i)),
            None => {}
        }
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
/// Font Colors' columns: the swatch's, the widest name's and the widest sample's of category `cat`; the page is made
/// as wide as they need.
const SWATCH_W: f32 = 38.0;
const COL_GAP: f32 = 24.0;

fn colour_columns(ctx: &egui::Context, m: &super::Modern, gu: &GameUi, cat: usize) -> (f32, f32) {
    let font = FontId::proportional(m.theme.small);
    let width = |id: u16| ctx.fonts_mut(|f| f.layout_no_wrap(gu.group_b(id as usize).unwrap_or_default(), font.clone(), Color32::WHITE).size().x);
    configui2::colour_rows(cat).iter().fold((0.0f32, 0.0f32), |(a, b), &(label, sample, _)| (a.max(width(label)), b.max(width(sample))))
}

fn colour_rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, cu: &ConfigUi, v: &Values, rows: &[(u16, u16, u16)], name_w: f32, focus: Option<usize>, follow: bool, click: &mut Option<Click>) {
    const NAMES: [&str; 3] = ["Hue", "Saturation", "Brightness"];
    for (i, &(label, sample, id)) in rows.iter().enumerate() {
        let c = rgb(v.get(id));
        let col = super::chat::colour(crate::dat::battle::argb_bytes(v.get(id)));
        let open = focus.is_some_and(|f| f / 3 == i);
        // The name, then its sample in the colour in a column of its own (the page sized for both; on a screen too
        // narrow for that each wraps in its column, the row as tall as it needs).
        let font = FontId::proportional(m.theme.small);
        let full = ui.available_width();
        let name_room = name_w.min(full * 0.5);
        let sample_room = (full - SWATCH_W - name_room - COL_GAP - 8.0).max(40.0);
        let name = ui.painter().layout(gu.group_b(label as usize).unwrap_or_default(), font.clone(), if open { m.theme.color("gold") } else { m.theme.color("text") }, name_room);
        let line = ui.painter().layout(gu.group_b(sample as usize).unwrap_or_default(), font, col, sample_room);
        let (r, resp) = ui.allocate_exact_size(vec2(full, name.size().y.max(line.size().y) + 14.0), Sense::click());
        let face = if open { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
        super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(pos2(r.min.x + 12.0, r.center().y - 8.0), vec2(16.0, 16.0)), 3.0, Color32::from_rgb(c[0], c[1], c[2]));
        p.galley(pos2(r.min.x + SWATCH_W, r.center().y - name.size().y / 2.0), name.clone(), Color32::WHITE);
        p.galley(pos2(r.min.x + SWATCH_W + name_room + COL_GAP, r.center().y - line.size().y / 2.0), line, Color32::WHITE);
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
            let shown = format!("{}%", steps[ch] * 100 / kit::STEPS);
            if let (Some(x), _) = kit::slider_setting(ui, m, name, focus == Some(k), follow, kit::STEPS + 1, steps[ch], &shown) {
                let mut s = steps;
                s[ch] = x;
                *click = Some(Click::Hsv(i, s));
            }
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

fn settings_page(ui: &mut Ui, m: &super::Modern, v: &settings::View, open: bool, follow: bool, skip: usize, act: &mut Vec<Do>) {
    if !v.tabs.is_empty() {
        ui.set_max_width(PAGE_W);
        let (tabs, tab) = shown_tabs(v, skip);
        if let Some(k) = kit::tab_strip(ui, m, "settings tabs", &tabs, tab, open && v.row == 0) {
            act.push(Do::Tab(k + skip));
        }
    }
    for (k, r) in v.rows.iter().enumerate() {
        let row = k + 1;
        let focus = open && v.row == row;
        match &r.ctl {
            settings::VCtl::Buttons(bs) => {
                kit::setting(ui, m, &r.label, focus, true, follow, |ui| {
                    let labels: Vec<String> = bs.iter().map(|b| b.0.clone()).collect();
                    if let Some(c) = kit::choice(ui, m, &labels, bs.iter().position(|b| b.1).unwrap_or(usize::MAX), 0) {
                        act.push(Do::Click(row, c));
                    }
                });
            }
            settings::VCtl::Slider { n, at, shown } => {
                if let (Some(i), _) = kit::slider_setting(ui, m, &r.label, focus, follow, *n, *at, shown) {
                    act.push(Do::Slide(row, i));
                }
            }
        }
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
fn mods_page(ui: &mut Ui, m: &super::Modern, s: &SettingsUi, open: bool, follow: bool, skip: usize, act: &mut Vec<ModDo>) {
    let v = s.view();
    let at = if open { Some(s.m_mods) } else { None };
    let (tabs, tab) = shown_tabs(&v, skip);
    if let Some(k) = kit::tab_strip(ui, m, "settings tabs", &tabs, tab, at.is_some_and(|a| a.0 == 0)) {
        act.push(ModDo::Tab(k + skip));
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
                mods_act(s, kit::Act::Press(st, b), At::new(st, b), 0);
            }
            ModDo::Browse(k) => {
                mods_act(s, kit::Act::Press(0, k), At::new(0, k), 0);
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
                        Some(s) if s.on_mods_page() => mods_page(ui, m, s, open, follow, 0, &mut mact),
                        Some(s) => settings_page(ui, m, &s.view(), open, follow, 0, &mut sact),
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
            if ours_key(s, key, shift, 0) {
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
    // Ours goes with the window, once the app has stored what changed.
    if w.modern && w.config.is_none() && w.settings.as_ref().is_some_and(|s| s.in_game && !s.dirty) {
        w.settings = None;
    }
    let Some(gu) = p.gu else { return };
    if !w.modern || !shown(w) {
        return;
    }
    settings_follow(w);
    if let Some(c) = w.config.as_mut() {
        refresh_hits(Some(gu), c, &w.game, w.settings.as_ref());
    }
    let Some(c) = w.config.as_ref() else { return };
    let entry = c.m_entry.min(PAGES.len() - 1);
    let page = current(c);
    let m = p.m;
    let sw = w.ui_wh.0;
    let (open, row) = (c.m_open, c.m_row);
    // Our page (Mods, the Lua mods'), when it is open and its state is up.
    let ours = w.settings.as_ref().filter(|s| match page {
        Entry::Mods => s.kind == settings::Kind::Settings && s.on_mods_page(),
        Entry::ModPages => s.kind == settings::Kind::Extras,
        _ => false,
    });
    let (mut sact, mut mact) = (Vec::new(), Vec::new());
    let v = &w.game;
    let all = match page {
        Entry::Topic => view(c, w.settings.as_ref()),
        _ => Vec::new(),
    };
    let find = c.m_find.clone().filter(|_| c.m_sub.is_none());
    let cat = c.m_cat.min(3);
    let (list, cats): (Vec<(u16, usize)>, Vec<String>) = match page {
        Entry::List(lp, menu) => (configui2::modern_rows(v, lp, cat), menu.map_or(Vec::new(), |m| categories(gu, m))),
        Entry::Colors => (Vec::new(), categories(gu, "conftxtc")),
        _ => (Vec::new(), Vec::new()),
    };
    // The cursor kept in view as it moves (only then, so the wheel stays free).
    let at = match ours {
        Some(s) if s.on_mods_page() => s.mods_state().and_then(|m| m.browse.as_ref()).map_or(s.m_mods.0 * 1000 + s.m_mods.1, |b| 900_000 + b.cursor),
        Some(s) => s.view().row * 100 + s.view().tab,
        None => c.m_keys.map_or(row, |_| 500_000 + c.m_key_row),
    };
    let follow = m.moved("config page", (entry * 1_000_000 + c.m_sub.map_or(0, |k| (k + 1) * 100_000) + at) ^ find.as_ref().map_or(0, |f| (f.len() + 1) << 40) ^ (c.m_retail as usize) << 50);
    let (keys, ask) = (c.m_keys, c.extra.ask_keys);
    let (mut click, mut pick, mut hover) = (None, None, None);
    let mut help = String::new();
    let mut detail = Detail::default();
    // Font Colors is as wide as its names and samples side by side need (within the screen).
    let colours = if matches!(page, Entry::Colors) { colour_columns(p.ctx, m, gu, cat) } else { (0.0, 0.0) };
    let page_w = match page {
        Entry::Colors => (SWATCH_W + colours.0 + COL_GAP + colours.1 + 20.0).clamp(PAGE_W, (p.ctx.content_rect().width() - 260.0).max(PAGE_W)),
        _ => PAGE_W,
    };
    p.panel("window", Some(Title { icon: Some("icons/config"), text: "Settings", note: None }), NAV_W + 32.0 + page_w + 16.0 + DETAIL_W, |ui| {
        ui.horizontal_top(|ui| {
            // No page is lit while the hits of every page show.
            nav_list(ui, m, if find.is_some() { usize::MAX } else { entry }, open && c.m_sub.is_none(), &mut click);
            ui.add_space(16.0);
            ui.vertical(|ui| {
            ui.set_width(page_w);
            if matches!(page, Entry::Topic) && keys.is_none() && ask.is_none() && find_bar(ui, m, find.as_deref(), c.m_retail, page_w) {
                click = Some(Click::Retail);
            }
            let tall = ui.ctx().content_rect().height() * TALL;
            egui::ScrollArea::vertical().id_salt(("config page", entry, c.m_sub, find.is_some())).max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(page_w);
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
                // A page opened from a topic page: its name and the way back.
                if let Some(k) = c.m_sub {
                    ui.horizontal(|ui| {
                        if kit::button(ui, m, "\u{2039} Back", 80.0, Btn::default()).clicked() {
                            click = Some(Click::Back);
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(SUBS[k].0).size(m.theme.ui).color(m.theme.color("gold")));
                    });
                    ui.add_space(6.0);
                }
                let first = !cats.is_empty() as usize;
                match page {
                    Entry::Topic => {
                        let focus = (open && c.m_sub.is_none()).then_some(row);
                        if let (Some(f), true) = (find.as_ref(), all.is_empty()) {
                            ui.label(egui::RichText::new(format!("Nothing matches \u{201c}{f}\u{201d}.")).size(m.theme.small).color(m.theme.color("muted")));
                        }
                        hover = topic_page(ui, m, Some(gu), c, v, &all, focus, follow, sw, &mut pick);
                        detail = hover.or(focus).map_or_else(|| Detail::page(PAGES[entry].1, find.is_some()), |k| topic_detail(Some(gu), c, v, &all, k, find.is_some()));
                        help = topic_keys(find.is_some()).into();
                    }
                    Entry::List(lp, _) => {
                        // The window's own help (its first element's).
                        if let Some(h) = gu.focus_text(&full(lp), u16::MAX)[0].as_ref() {
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
                    }
                    Entry::Colors => {
                        let labels: Vec<&str> = cats.iter().map(String::as_str).collect();
                        if let Some(k) = kit::tab_strip(ui, m, "config cats", &labels, cat, open && row == 0) {
                            click = Some(Click::Cat(k));
                        }
                        colour_rows(ui, m, gu, c, v, configui2::colour_rows(cat), colours.0, (open && row >= 1).then(|| row - 1), follow, &mut click);
                        help = "Up / Down: a colour and its hue, saturation and brightness; Left / Right change them. The mouse: the square and the strip.".into();
                    }
                    Entry::Mods | Entry::ModPages => match ours {
                        Some(s) if s.on_mods_page() => {
                            mods_page(ui, m, s, open, follow, s.mods_tab(), &mut mact);
                            help = s.help();
                        }
                        Some(s) if s.mod_pages.is_empty() => {
                            ui.label(egui::RichText::new("No mod has settings of its own.").size(m.theme.small).color(m.theme.color("muted")));
                        }
                        Some(s) => {
                            settings_page(ui, m, &s.view(), open, follow, s.mods_tab(), &mut sact);
                            help = s.view().help;
                        }
                        None => {}
                    },
                }
            }));
            });
            if let Some(k) = c.m_sub.filter(|_| keys.is_none()) {
                detail = Detail { name: SUBS[k].0.into(), text: SUBS[k].2.into(), facts: vec![("Retail", if k < SUB_MODS { "Yes" } else { "No (Vana Client)" }.into())] };
            }
            ui.add_space(16.0);
            details(ui, m, &detail);
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
    if let (Some(pick), Entry::Topic) = (pick, page) {
        return topic_do(w, pick, sw);
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
        Click::Back => return close_sub(w),
        Click::Retail => {
            if let Some(c) = w.config.as_mut() {
                (c.m_retail, c.m_row) = (!c.m_retail, 0);
            }
            out.sounds.push(Se::Cursor);
        }
        Click::Cat(k) => {
            if let Some(c) = w.config.as_mut() {
                (c.m_cat, c.m_row) = (k, 0);
            }
            out.sounds.push(Se::Cursor);
        }
        Click::Toggle(r) => {
            let first = matches!(page, Entry::List(_, Some(_))) as usize;
            if let Some(c) = w.config.as_mut() {
                c.m_row = r + first;
            }
            return toggle(w, page, r);
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
        // The title's Config only.
        Click::Opt(..) | Click::Slide(..) | Click::Press(_) => {}
    }
    super::super::config_out(w, out);
}

/// `modern-shot --settings`: the window searching "shadow" over our settings (retail's need the game's layouts), the
/// cursor on its second hit.
pub fn sample(p: &mut Panels, s: &SettingsUi) {
    let m = p.m;
    let (mut c, v) = (ConfigUi::default(), Values::default());
    c.m_find = Some("shadow".into());
    refresh_hits(None, &mut c, &v, Some(s));
    let all: Vec<Line> = view(&c, Some(s)).into_iter().filter(|l| l.shape.is_none()).collect();
    p.panel("window", Some(Title { icon: Some("icons/config"), text: "Settings", note: None }), NAV_W + 32.0 + PAGE_W + 16.0 + DETAIL_W, |ui| {
        ui.horizontal_top(|ui| {
            nav_list(ui, m, usize::MAX, true, &mut None);
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(PAGE_W);
                find_bar(ui, m, c.m_find.as_deref(), false, PAGE_W);
                topic_page(ui, m, None, &c, &v, &all, Some(1), false, 1920.0, &mut None);
            });
            ui.add_space(16.0);
            details(ui, m, &topic_detail(None, &c, &v, &all, 1, true));
        });
        help_line(ui, m, topic_keys(true));
    });
}

/// Our line's help (the sample's; no game layouts).
fn topic_help_ours(all: &[Line], k: usize) -> String {
    stop_lines(all).get(k).and_then(|l| l.ours.as_ref()).map(|o| o.1.clone()).unwrap_or_default()
}

/// The nav's width.
const NAV_W: f32 = 170.0;

/// The pages down the left by group: the chosen one lit, a click choosing one.
fn nav_list(ui: &mut Ui, m: &super::Modern, entry: usize, open: bool, click: &mut Option<Click>) {
    ui.vertical(|ui| {
        ui.set_width(NAV_W);
        let mut group = "";
        for (k, (g, label, _)) in PAGES.iter().enumerate() {
            if *g != group {
                group_head(ui, m, g, k == 0);
                group = g;
            }
            let (r, resp) = ui.allocate_exact_size(vec2(NAV_W, 28.0), Sense::click());
            let face = match () {
                _ if k == entry && !open => "controls/list_row_selected",
                _ if k == entry || resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let ink = if k == entry { m.theme.color("gold") } else { m.theme.color("text") };
            ui.painter().text(pos2(r.min.x + 16.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(m.theme.ui), ink);
            if resp.clicked() {
                *click = Some(Click::Entry(k));
            }
            ui.add_space(2.0);
        }
    });
}

/// A list row stepped (Enter, a click, Left / Right): retail's toggle, SE 0001.
fn toggle(w: &mut World, entry: Entry, r: usize) {
    let Entry::List(page, _) = entry else { return };
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
        Ctl::Act(Act::CamKeys(_, x)) => configui::cam_caption(x).into(),
        Ctl::Open(n) => n.into(),
        _ => String::new(),
    }
}

/// The list's entry `k` taken: its page gets the keys (Extras / Settings ours: their state takes them from here).
fn choose(w: &mut World, k: usize) {
    let Some(c) = w.config.as_mut() else { return };
    (c.m_entry, c.m_open, c.m_row, c.m_keys, c.m_sub, c.m_find) = (k, true, 0, None, None, None);
    let out = configui::Out { sounds: vec![Se::Select], ..Default::default() };
    super::super::config_out(w, out);
    if let Some(s) = w.settings.as_mut().filter(|s| s.on_mods_page()) {
        s.goto(0, 0);
    }
    settings_follow(w);
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
    let n = PAGES.len();
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
    let page = current(c);
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
    match page {
        Entry::Topic => {
            let all = view(c, w.settings.as_ref());
            let stops = topic_stops(c, &w.game, &all, sw);
            if stops.is_empty() {
                if key == egui::Key::Escape {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                return true;
            }
            let mut at = At::new(c.m_row.min(stops.len() - 1), 0);
            let act = kit::key(&stops, &mut at, key, shift);
            c.m_row = at.stop;
            match act {
                kit::Act::Set(i, x) => topic_do(w, Pick::Set(i, x, false), sw),
                kit::Act::Press(i, _) => topic_do(w, Pick::Press(i), sw),
                kit::Act::Back => {
                    c.m_open = false;
                    w.sounds.push(Se::Cancel as u32);
                }
                a => w.sounds.extend(a.sound().map(|s| s as u32)),
            }
        }
        Entry::List(lp, menu) => {
            let stops = list_stops(&w.game, lp, menu.is_some(), c.m_cat);
            let first = menu.is_some() as usize;
            let mut at = At::new(c.m_row.min(stops.len().saturating_sub(1)), 0);
            let act = kit::key(&stops, &mut at, key, shift);
            c.m_row = at.stop;
            match act {
                kit::Act::Set(0, k) if first == 1 => {
                    (c.m_cat, c.m_row) = (k, 0);
                    w.sounds.push(Se::Cursor as u32);
                }
                kit::Act::Set(r, _) => toggle(w, page, r - first),
                kit::Act::Back => close_sub(w),
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
                kit::Act::Back => close_sub(w),
                a => w.sounds.extend(a.sound().map(|s| s as u32)),
            }
        }
        Entry::Mods | Entry::ModPages => {
            let mut back = false;
            if let Some(s) = w.settings.as_mut() {
                if matches!(page, Entry::ModPages) && s.mod_pages.is_empty() {
                    back = key == egui::Key::Escape;
                } else {
                    let skip = s.mods_tab();
                    back = ours_key(s, key, shift, skip);
                }
                w.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
            }
            if back {
                close_sub(w);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every setting of the old Config list (retail's pages and ours, Settings' and Extras') has exactly one place on
    /// the topic pages, every line names a setting that exists, and each page of SUBS opens from one button.
    #[test]
    fn every_setting_has_a_page() {
        let items: Vec<Item> = PAGES.iter().flat_map(|p| p.2.iter().copied()).collect();
        let count = |it: Item| items.iter().filter(|&&i| i == it).count();
        for menu in ["conf2win", "conf3win", "conf6win", "conf12wi", "conf13wi", "conf7", "conf4"] {
            for s in shapes(&[menu]) {
                assert_eq!(count(Retail(menu, s.ctls[0].0)), 1, "{menu} {}", s.ctls[0].0);
            }
        }
        for it in &items {
            if let Retail(menu, child) = *it {
                assert!(shape_of(menu, child).is_some(), "{menu} {child}");
            }
        }
        let mut ours: Vec<&str> = [(true, false), (true, true), (false, false), (false, true)].iter().flat_map(|&(g, m)| SettingsUi::all_labels(g, m)).collect();
        ours.sort();
        ours.dedup();
        for label in &ours {
            assert_eq!(count(Ours(label)), 1, "{label}");
        }
        for it in &items {
            if let Ours(label) = *it {
                assert!(ours.contains(&label), "{label}");
            }
        }
        for k in 0..SUBS.len() {
            assert_eq!(count(Open(k)), 1, "{}", SUBS[k].0);
        }
    }

    /// The search finds settings by name, by a word of their help, and every setting under a heading it names, each
    /// under its page's name; Retail only leaves ours out.
    #[test]
    fn search_and_retail_only() {
        let s = SettingsUi::open(&crate::config::Config::default(), [1920, 1080]);
        let (mut c, v) = (ConfigUi::default(), Values::default());
        c.m_find = Some("shadow".into());
        refresh_hits(None, &mut c, &v, Some(&s));
        let items = view_items(&c);
        assert_eq!(items[0], Head("Lighting & Shadows"));
        for it in [Retail("conf3win", 6)] {
            assert!(items.contains(&it), "{it:?}");
        }
        assert!(!items.contains(&Ours("Lamp Lights")));
        // A word of the help: Lamp Lights says "lamplight", Gamma Base "brightness".
        c.m_find = Some("brightness".into());
        refresh_hits(None, &mut c, &v, Some(&s));
        assert!(view_items(&c).contains(&Ours("Gamma Base")));
        // Two letters only match names.
        c.m_find = Some("zo".into());
        refresh_hits(None, &mut c, &v, Some(&s));
        assert!(view_items(&c).iter().all(|i| matches!(i, Head(_))) || view_items(&c).is_empty());
        c.m_find = None;
        refresh_hits(None, &mut c, &v, Some(&s));
        c.m_entry = PAGES.iter().position(|p| p.1 == "Lighting & Shadows").unwrap();
        c.m_retail = true;
        let items = view_items(&c);
        assert!(items.contains(&Retail("conf3win", 6)));
        assert!(items.iter().all(|i| !matches!(i, Ours(_))));
        c.m_entry = PAGES.iter().position(|p| p.1 == "Mods").unwrap();
        assert!(view(&c, Some(&s)).is_empty());
    }

    /// A page of ours in the Settings window shows only its own tabs, and turning them keeps that offset.
    #[test]
    fn mod_tabs_only() {
        let mut s = SettingsUi::open(&crate::config::Config::default(), [1920, 1080]);
        s.goto_mods();
        let skip = s.mods_tab();
        let v = s.view();
        let (tabs, at) = shown_tabs(&v, skip);
        assert_eq!((tabs, at), (vec!["Mods"], 0));
        assert!(matches!(mods_stops(&s, skip)[0], Stop::Tabs { n: 1, at: 0 }));
    }

    /// A page's headings with nothing under them go (ours left out in game, here the lobby's volumes).
    #[test]
    fn empty_headings_go() {
        let all = lines(PAGES.iter().position(|p| p.1 == "Controller").unwrap(), None);
        assert!(all.is_empty());
        let all = lines(PAGES.iter().position(|p| p.1 == "Gameplay").unwrap(), None);
        assert!(matches!(all[0].item, Head("Targeting")));
        assert!(!matches!(all.last().unwrap().item, Head(_)));
        assert!(all.iter().all(|l| !matches!(l.item, Ours(_))));
    }
}
