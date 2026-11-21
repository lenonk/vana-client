//! Ours (not retail): the Mods window, the modern UI's one place for mods. Installed: the Lua mods down the left (a
//! search over them, Update All under it); the picked one's ON / OFF / Uninstall / Update, version and description, then
//! its Settings (its manifest's settings), Commands (every form of its `[commands]`; the picked form's fields, pickers for typed params, the line,
//! Run and Put in Chat Line, and what the mod printed since Run) and Permissions. Get mods: Install from File, the
//! standard mods on GitHub, the mod archives in the Downloads folder. DATs: the DAT packs. Opened from Settings > Mods,
//! /lua mods and /lua settings. Tab turns the window's tabs; on the list Up / Down pick a mod, typing searches, Enter
//! or Right opens its page; on a page the kit's keys, Escape back to the list; in a field typing fills it, Tab moves
//! to the next, Enter or Escape leaves it. Escape on the list closes.

use super::super::World;
use super::super::gm::{self, Pick};
use super::super::lua::PERM;
use super::super::settings::{Edit, ModPage, ModRow, SettingsUi};
use super::config::{self, Detail, ModDo, ModOut};
use super::kit::{self, At, Btn, Stop};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use crate::modpack::{LuaInfo, LuaParam};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::collections::HashMap;

const LIST_W: f32 = 250.0;
const PAGE_W: f32 = 800.0;
const FORMS_W: f32 = 370.0;
const ROW_H: f32 = 30.0;
const TABS: [&str; 3] = ["Installed", "Get mods", "DATs"];
const SUBS: [&str; 3] = ["Settings", "Commands", "Permissions"];
const SUB_SETTINGS: usize = 0;
const SUB_COMMANDS: usize = 1;
/// Suggestions under a picker field.
const SUGGEST: usize = 6;

/// Where the keys go on Installed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum Focus {
    #[default]
    List,
    Page,
    /// Update All, under the list.
    All,
}

/// The window's state, made by the app (it holds a SettingsUi, made from the config).
pub struct ModMgr {
    /// The Settings window's mods state: the installed list and its order, installs, the catalogue.
    pub s: SettingsUi,
    /// Every installed mod's settings and permission rows.
    pub pages: Vec<ModPage>,
    /// False once closed: the app drops it after storing what changed.
    pub open: bool,
    raise: bool,
    tab: usize,
    /// The picked mod: its place in the installed list.
    sel: usize,
    sub: usize,
    focus: Focus,
    at: At,
    find: String,
    finding: bool,
    form: usize,
    /// The form's field taking the typed text.
    field: Option<usize>,
    /// Typed values by (mod, form, field).
    values: HashMap<(String, usize, usize), String>,
    /// A text or colour setting being typed: its row on Settings, the text so far.
    editing: Option<(usize, String)>,
    /// A key setting waiting for its key: its row on Settings.
    listening: Option<usize>,
    /// A colour setting whose picker is open: its row on Settings.
    picking: Option<usize>,
    /// Each running command's owner (the mod loaded first that registered it): a form runs only from its owner.
    owners: HashMap<String, String>,
    /// The last Run: (mod, the print number then, the line).
    ran: Option<(String, u64, String)>,
    get_row: usize,
    pickers: gm::Pickers,
    lists_at: Option<std::time::Instant>,
}

impl ModMgr {
    /// The window over SettingsUi `s` (on its Mods page), on mod `id`'s Settings when given.
    pub fn new(mut s: SettingsUi, pages: Vec<ModPage>, id: Option<String>) -> Self {
        s.goto_page(true);
        let mut mm = Self {
            s,
            pages,
            open: true,
            raise: true,
            tab: 0,
            sel: 0,
            sub: 0,
            focus: Focus::List,
            at: At::default(),
            find: String::new(),
            finding: false,
            form: 0,
            field: None,
            values: HashMap::new(),
            editing: None,
            listening: None,
            picking: None,
            owners: HashMap::new(),
            ran: None,
            get_row: 0,
            pickers: gm::Pickers::default(),
            lists_at: None,
        };
        mm.show(id);
        mm
    }

    /// In front again; on mod `id`'s Settings when given.
    pub fn show(&mut self, id: Option<String>) {
        self.raise = true;
        let Some(id) = id else { return };
        if self.tab != 0 {
            self.tab = 0;
            self.s.goto_page(true);
        }
        let n = self.s.mods_rows().0.len();
        if let Some(i) = (0..n).find(|&i| self.s.mod_id(i).is_some_and(|f| f.eq_ignore_ascii_case(&id))) {
            self.find.clear();
            self.pick(i);
            (self.sub, self.focus) = (SUB_SETTINGS, Focus::Page);
            self.at = At::new(first_stop(self), 0);
        }
    }

    fn pick(&mut self, i: usize) {
        if i != self.sel {
            (self.sel, self.form, self.field) = (i, 0, None);
            self.stop_editing();
        }
    }

    /// The installed mods the search lets through, by their place in the list.
    fn shown(&self) -> Vec<usize> {
        let q = self.find.trim().to_lowercase();
        let rows = self.s.mods_rows().0;
        (0..rows.len())
            .filter(|&i| {
                q.is_empty()
                    || rows[i].0.to_lowercase().contains(&q)
                    || self.s.mod_id(i).is_some_and(|f| f.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// The picked mod as the page shows it.
    fn picked(&self) -> Option<Sel> {
        let rows = self.s.mods_rows().0;
        let (name, on) = rows.get(self.sel)?.clone();
        let id = self.s.mod_id(self.sel)?;
        let info = self.s.lua_info.get(&id).cloned().unwrap_or_default();
        let page = self.pages.iter().find(|p| p.id.eq_ignore_ascii_case(&id));
        let (perms, rows): (Vec<ModRow>, Vec<ModRow>) =
            page.map(|p| p.rows.clone()).unwrap_or_default().into_iter().partition(|r| r.key.starts_with(PERM));
        let update = self.s.mods_state().and_then(|m| m.update_for(&id)).map(str::to_string);
        let forms = forms_of(&info);
        Some(Sel { id, name, on, info, update, rows, perms, forms })
    }

    fn values_of(&self, id: &str, form: usize, n: usize) -> Vec<String> {
        (0..n).map(|i| self.values.get(&(id.to_string(), form, i)).cloned().unwrap_or_default()).collect()
    }

    /// The mods' settings and permission rows made again (a mod installed, updated or turned on or off); a setting
    /// being typed, waited for or picked stops, since its row may have moved.
    pub fn refresh(&mut self, pages: Vec<ModPage>) {
        self.pages = pages;
        self.stop_editing();
    }

    /// The mod whose handler `//cmd` runs when it is not `sel`'s (another mod registered it first), by name.
    fn other_owner(&self, sel: &Sel, cmd: &str) -> Option<String> {
        self.owners.get(cmd).filter(|id| !id.eq_ignore_ascii_case(&sel.id)).map(|id| {
            self.s
                .lua_info
                .get(id)
                .map_or_else(|| id.clone(), |i| if i.name.is_empty() { id.clone() } else { i.name.clone() })
        })
    }

    /// A setting's typing, key wait and picker dropped (the value it holds stays).
    fn stop_editing(&mut self) {
        (self.editing, self.listening, self.picking) = (None, None, None);
    }
}

/// The picked mod: its folder, name, state, manifest text, rows and command forms.
struct Sel {
    id: String,
    name: String,
    on: bool,
    info: LuaInfo,
    /// The newer version the catalogue has.
    update: Option<String>,
    rows: Vec<ModRow>,
    perms: Vec<ModRow>,
    forms: Vec<Form>,
}

/// One form of a mod's command: `//cmd args`, its help and typed params, its words.
#[derive(Clone, Debug)]
struct Form {
    cmd: String,
    args: String,
    help: String,
    params: Vec<LuaParam>,
    parts: Vec<Part>,
}

/// A form's word: fixed, or a placeholder the player fills (`<x>`, or `[x]` that may stay empty).
#[derive(Clone, Debug, PartialEq)]
enum Part {
    Word(String),
    Field { label: String, key: String, optional: bool },
}

fn forms_of(info: &LuaInfo) -> Vec<Form> {
    info.commands
        .iter()
        .flat_map(|(cmd, c)| {
            c.forms().into_iter().map(move |(args, help, params)| Form {
                cmd: cmd.clone(),
                parts: parts(&args),
                args,
                help,
                params,
            })
        })
        .collect()
}

/// A form's words: a placeholder's key is its first alternative (`<item | none | own>`: item); a word with brackets
/// inside other text (`[count],`, `<main>[/<sub>]`) is one field typed whole.
fn parts(args: &str) -> Vec<Part> {
    super::super::cmdhint::words(args)
        .into_iter()
        .map(|w| {
            let inner = w
                .strip_prefix('<')
                .and_then(|r| r.strip_suffix('>'))
                .or_else(|| w.strip_prefix('[').and_then(|r| r.strip_suffix(']')))
                .filter(|i| !i.contains(['<', '>', '[', ']']));
            match inner {
                Some(i) => Part::Field {
                    label: i.trim().to_string(),
                    key: i.split('|').next().unwrap_or("").trim().to_string(),
                    optional: w.starts_with('['),
                },
                None if w.contains(['<', '[']) => {
                    Part::Field { label: w.to_string(), key: w.to_string(), optional: w.starts_with('[') }
                }
                None => Part::Word(w.to_string()),
            }
        })
        .collect()
}

/// The form's fields: (label, key, optional).
fn fields(f: &Form) -> Vec<(&str, &str, bool)> {
    f.parts
        .iter()
        .filter_map(|p| match p {
            Part::Field { label, key, optional } => Some((label.as_str(), key.as_str(), *optional)),
            Part::Word(_) => None,
        })
        .collect()
}

/// The line the form types, or the first field still to fill.
fn line(f: &Form, vals: &[String]) -> Result<String, String> {
    let mut out = format!("//{}", f.cmd);
    let mut k = 0;
    for p in &f.parts {
        match p {
            Part::Word(w) => {
                out.push(' ');
                out.push_str(w);
            }
            Part::Field { label, optional, .. } => {
                let v = vals.get(k).map_or("", |v| v.trim());
                k += 1;
                if v.is_empty() {
                    if *optional {
                        continue;
                    }
                    return Err(format!("Fill in {label}."));
                }
                out.push(' ');
                out.push_str(v);
            }
        }
    }
    Ok(out)
}

fn param<'a>(f: &'a Form, key: &str) -> Option<&'a LuaParam> {
    f.params.iter().find(|p| p.name.eq_ignore_ascii_case(key))
}

/// A typed param's picker.
fn pick_of(kind: &str) -> Option<Pick> {
    Some(match kind {
        "item" => Pick::Item,
        "key_item" => Pick::KeyItem,
        "zone" => Pick::Zone,
        "spell" => Pick::Spell,
        "job" => Pick::Job,
        "player" => Pick::Player,
        _ => return None,
    })
}

/// A field's buttons under it: an enum's options, the words it also takes, a picker's suggestions for the text
/// (`found`: none when the text names one exactly).
fn chips(p: Option<&LuaParam>, found: Option<&gm::Found>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(p) = p {
        out.extend(p.options.iter().cloned());
        out.extend(p.also.iter().cloned());
    }
    if let Some(f) = found.filter(|f| f.known.is_none() || f.hits.len() > 1) {
        for (_, name) in &f.hits {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// What a stop on the Installed page is.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// ON, OFF, Uninstall, and Update when the catalogue has a newer version.
    Header,
    Subs,
    /// A setting or a permission (the sub's) by its place.
    Row(usize),
    Forms,
    Field(usize),
    /// Run, Put in Chat Line.
    Run,
}

fn kinds(sel: &Sel, sub: usize, form: usize) -> Vec<Kind> {
    let mut out = vec![Kind::Header, Kind::Subs];
    match sub {
        SUB_SETTINGS => out.extend((0..sel.rows.len()).map(Kind::Row)),
        SUB_COMMANDS if !sel.forms.is_empty() => {
            out.push(Kind::Forms);
            let n = sel.forms.get(form).map_or(0, |f| fields(f).len());
            out.extend((0..n).map(Kind::Field));
            out.push(Kind::Run);
        }
        SUB_COMMANDS => {}
        _ => out.extend((0..sel.perms.len()).map(Kind::Row)),
    }
    out
}

fn slider(r: &ModRow) -> bool {
    r.choices.len() > 8 || (r.choices.len() > 3 && r.choices.iter().all(|c| c.1.is_number()))
}

/// The first stop of the page's body (the sub's tabs when it has none).
fn first_stop(mm: &ModMgr) -> usize {
    mm.picked().map_or(1, |sel| if kinds(&sel, mm.sub, mm.form).len() > 2 { 2 } else { 1 })
}

/// The open form's fields' buttons (a picker's suggestions only for the field being typed in or under the cursor).
fn form_chips(mm: &ModMgr, sel: &Sel, active: Option<usize>) -> Vec<Vec<String>> {
    let Some(f) = sel.forms.get(mm.form) else { return Vec::new() };
    let vals = mm.values_of(&sel.id, mm.form, fields(f).len());
    fields(f)
        .iter()
        .enumerate()
        .map(|(i, (_, key, _))| {
            let p = param(f, key);
            let found = p
                .and_then(|p| pick_of(&p.kind))
                .filter(|_| active == Some(i) && !vals[i].trim().is_empty())
                .map(|pick| mm.pickers.lookup(pick, &vals[i], SUGGEST));
            chips(p, found.as_ref())
        })
        .collect()
}

/// The cursor's stop when it is on a form's field or Run (Tab moves between the fields there).
fn form_stop(mm: &ModMgr) -> Option<Kind> {
    if mm.tab != 0 || mm.focus != Focus::Page || mm.sub != SUB_COMMANDS {
        return None;
    }
    let sel = mm.picked()?;
    // Run is one only when the form has fields to Tab back to; a form without stays on the window's tabs.
    let fielded = sel.forms.get(mm.form).is_some_and(|f| !fields(f).is_empty());
    kinds(&sel, mm.sub, mm.form)
        .get(mm.at.stop)
        .copied()
        .filter(|k| matches!(k, Kind::Field(_)) || (*k == Kind::Run && fielded))
}

/// The field the suggestions are for: the one typed in, else the cursor's.
fn active_field(mm: &ModMgr, ks: &[Kind]) -> Option<usize> {
    mm.field.or(match (mm.focus, ks.get(mm.at.stop)) {
        (Focus::Page, Some(Kind::Field(i))) => Some(*i),
        _ => None,
    })
}

fn stops(mm: &ModMgr, sel: &Sel, ks: &[Kind], chips: &[Vec<String>]) -> Vec<Stop> {
    let ok = sel.forms.get(mm.form).is_some_and(|f| line(f, &mm.values_of(&sel.id, mm.form, fields(f).len())).is_ok());
    let other = sel.forms.get(mm.form).is_some_and(|f| mm.other_owner(sel, &f.cmd).is_some());
    let rows = if mm.sub == SUB_SETTINGS { &sel.rows } else { &sel.perms };
    ks.iter()
        .map(|k| match *k {
            Kind::Header => Stop::Buttons { n: 3 + sel.update.is_some() as usize, off: 0, pair: None },
            Kind::Subs => Stop::Tabs { n: SUBS.len(), at: mm.sub },
            Kind::Row(r) => {
                let r = &rows[r];
                match r.edit {
                    Edit::Choice => Stop::Value { n: r.choices.len(), at: r.at, slider: slider(r), off: 0 },
                    // The value (Enter: type it, or wait for the key); a colour's Pick… beside it.
                    e => Stop::Buttons { n: if e == Edit::Colour { 2 } else { 1 }, off: 0, pair: None },
                }
            }
            Kind::Forms => Stop::List { n: sel.forms.len(), page: 8 },
            Kind::Field(i) => Stop::Buttons { n: 1 + chips.get(i).map_or(0, Vec::len), off: 0, pair: None },
            Kind::Run => Stop::Buttons { n: 2, off: (!(ok && sel.on && !other)) as u64, pair: None },
        })
        .collect()
}

pub fn shown(w: &World) -> bool {
    w.modern && w.modmgr.as_ref().is_some_and(|m| m.open)
}

/// The window takes the keys (no chat line, prompt or event over it).
pub fn takes_keys(w: &World) -> bool {
    shown(w) && !w.typing && w.prompt.is_none() && w.confirm.is_none() && !super::super::event::active(w)
}

/// `/lua mods`, Settings > Mods: the window (the app makes it), on mod `id`'s Settings when given.
pub fn open(w: &mut World, id: Option<String>) {
    match w.modmgr.as_mut().filter(|m| m.open) {
        Some(m) => m.show(id),
        None => w.want_mods = Some(id),
    }
    w.sounds.push(Se::Select as u32);
}

/// Typed text and keys while it is open. True when it took them.
pub fn events(w: &mut World, events: &[egui::Event]) -> bool {
    if !takes_keys(w) {
        return false;
    }
    for e in events {
        if w.modmgr.as_ref().is_some_and(|m| m.listening.is_some()) {
            listen(w, e);
            continue;
        }
        match e {
            egui::Event::Text(t) | egui::Event::Paste(t) => text(w, t),
            egui::Event::Key { key, pressed: true, modifiers, repeat, .. } => {
                // A held Enter repeats: only its press runs.
                if *key == egui::Key::Enter && *repeat {
                    continue;
                }
                self::key(w, *key, modifiers.shift);
            }
            _ => {}
        }
        if !shown(w) {
            break;
        }
    }
    true
}

fn text(w: &mut World, t: &str) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let t: String = t.chars().filter(|c| !c.is_control()).collect();
    if mm.tab != 0 || t.is_empty() || mm.s.m_remove.is_some() {
        return;
    }
    if let Some((_, typed)) = mm.editing.as_mut() {
        typed.push_str(&t);
        return;
    }
    if let Some(i) = mm.field {
        if let Some(sel) = mm.picked() {
            mm.values.entry((sel.id, mm.form, i)).or_default().push_str(&t);
        }
        return;
    }
    // Typing on the list (or after clicking the search) searches.
    if mm.focus == Focus::List || mm.finding {
        mm.find.push_str(&t);
        mm.finding = true;
        if let Some(&i) = mm.shown().first() {
            mm.pick(i);
        }
    }
}

fn sound(w: &mut World, s: Se) {
    w.sounds.push(s as u32);
}

fn close(w: &mut World) {
    if let Some(mm) = w.modmgr.as_mut() {
        mm.open = false;
    }
    sound(w, Se::Cancel);
}

fn turn(w: &mut World, t: usize) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    if t == mm.tab {
        return;
    }
    (mm.tab, mm.field, mm.finding, mm.get_row) = (t, None, false, 0);
    mm.stop_editing();
    mm.s.goto_page(t != 2);
    if t == 0 {
        mm.focus = Focus::List;
    }
    sound(w, Se::Cursor);
}

fn key(w: &mut World, key: egui::Key, shift: bool) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    mm.s.sounds.clear();
    // The removal prompt and the file picker keep the keys while they show.
    if mm.s.m_remove.is_some() {
        config::remove_key(&mut mm.s, key);
        flush(w);
        return;
    }
    if mm.s.mods_state().is_some_and(|m| m.browse.is_some()) {
        config::mods_key(&mut mm.s, key, shift);
        flush(w);
        return;
    }
    // Tab turns the window's tabs, but in a form moves between its fields.
    if key == egui::Key::Tab && mm.field.is_none() && mm.editing.is_none() && form_stop(mm).is_none() {
        let t = (mm.tab + if shift { TABS.len() - 1 } else { 1 }) % TABS.len();
        return turn(w, t);
    }
    match mm.tab {
        0 if mm.focus == Focus::List => list_key(w, key),
        0 if mm.focus == Focus::All => all_key(w, key),
        0 => page_key(w, key, shift),
        1 => get_key(w, key, shift),
        _ => {
            if config::mods_key(&mut mm.s, key, shift) == ModOut::Back {
                mm.open = false;
            }
            flush(w);
        }
    }
}

/// The SettingsUi's sounds to the world's.
fn flush(w: &mut World) {
    if let Some(mm) = w.modmgr.as_mut() {
        w.sounds.extend(mm.s.sounds.drain(..).map(|s| s as u32));
    }
}

fn list_key(w: &mut World, key: egui::Key) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let shown = mm.shown();
    let at = shown.iter().position(|&i| i == mm.sel);
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown if !shown.is_empty() => {
            let ups = mm.s.mods_state().map_or(0, |s| s.updates(true));
            if key == egui::Key::ArrowDown && at == Some(shown.len() - 1) && ups > 0 {
                mm.focus = Focus::All;
                return sound(w, Se::Cursor);
            }
            let next = match (at, key == egui::Key::ArrowDown) {
                (None, _) => 0,
                (Some(k), true) => (k + 1).min(shown.len() - 1),
                (Some(k), false) => k.saturating_sub(1),
            };
            if Some(next) != at {
                mm.pick(shown[next]);
                sound(w, Se::Cursor);
            }
        }
        egui::Key::Enter | egui::Key::ArrowRight if at.is_some() => {
            mm.focus = Focus::Page;
            mm.finding = false;
            mm.at = At::new(first_stop(mm), 0);
            sound(w, Se::Select);
        }
        egui::Key::Backspace if !mm.find.is_empty() => {
            mm.find.pop();
        }
        egui::Key::Escape if !mm.find.is_empty() => {
            mm.find.clear();
            mm.finding = false;
            sound(w, Se::Cancel);
        }
        egui::Key::Escape => close(w),
        _ => {}
    }
}

/// On Update All: Enter presses it, Up goes back to the list.
fn all_key(w: &mut World, key: egui::Key) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    match key {
        egui::Key::ArrowUp => {
            mm.focus = Focus::List;
            if let Some(&i) = mm.shown().last() {
                mm.pick(i);
            }
            sound(w, Se::Cursor);
        }
        egui::Key::Enter => update_all(w),
        egui::Key::Escape => close(w),
        _ => {}
    }
}

/// Every installed Lua mod with an update, one after another.
fn update_all(w: &mut World) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let n = mm.s.mods_mut().map_or(0, |m| m.update_all(true));
    sound(w, if n > 0 { Se::Select } else { Se::Error });
}

fn page_key(w: &mut World, key: egui::Key, shift: bool) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let Some(sel) = mm.picked() else {
        mm.focus = Focus::List;
        return;
    };
    let ks = kinds(&sel, mm.sub, mm.form);
    // Typing a setting: Enter keeps it, Escape drops it.
    if mm.editing.is_some() {
        match key {
            egui::Key::Backspace => {
                if let Some((_, t)) = mm.editing.as_mut() {
                    t.pop();
                }
            }
            egui::Key::Enter => finish_edit(w, &sel.id),
            egui::Key::Escape => {
                mm.editing = None;
                sound(w, Se::Cancel);
            }
            _ => {}
        }
        return;
    }
    // Typing in a field: its keys first.
    if let Some(i) = mm.field {
        let n = sel.forms.get(mm.form).map_or(0, |f| fields(f).len()).max(1);
        match key {
            egui::Key::Backspace => {
                if let Some(v) = mm.values.get_mut(&(sel.id.clone(), mm.form, i)) {
                    v.pop();
                }
                return;
            }
            egui::Key::Tab => {
                let j = if shift { (i + n - 1) % n } else { (i + 1) % n };
                mm.field = Some(j);
                if let Some(s) = ks.iter().position(|k| *k == Kind::Field(j)) {
                    mm.at = At::new(s, 0);
                }
                return sound(w, Se::Cursor);
            }
            egui::Key::Enter | egui::Key::Escape => {
                mm.field = None;
                return sound(w, if key == egui::Key::Enter { Se::Select } else { Se::Cancel });
            }
            egui::Key::ArrowUp | egui::Key::ArrowDown => mm.field = None,
            _ => return,
        }
    }
    // Tab on a form's field or Run: typing in the next field (the previous with Shift).
    if key == egui::Key::Tab {
        let n = sel.forms.get(mm.form).map_or(0, |f| fields(f).len());
        let j = match ks.get(mm.at.stop) {
            Some(Kind::Field(i)) if shift => (i + n - 1) % n,
            Some(Kind::Field(i)) => (i + 1) % n,
            Some(Kind::Run) if n > 0 => {
                if shift {
                    n - 1
                } else {
                    0
                }
            }
            _ => return,
        };
        mm.field = Some(j);
        if let Some(s) = ks.iter().position(|k| *k == Kind::Field(j)) {
            mm.at = At::new(s, 0);
        }
        return sound(w, Se::Cursor);
    }
    let active = active_field(mm, &ks);
    let chips = form_chips(mm, &sel, active);
    let st = stops(mm, &sel, &ks, &chips);
    let mut at = mm.at;
    let act = kit::key(&st, &mut at, key, shift);
    // Onto a field or Run from another stop: its first control (a field's buttons are no row to keep a column of).
    if at.stop != mm.at.stop && matches!(ks.get(at.stop), Some(Kind::Field(_) | Kind::Run)) {
        at.sub = 0;
    }
    mm.at = at;
    match act {
        kit::Act::Moved => {
            if ks.get(at.stop) == Some(&Kind::Forms) && at.sub != mm.form {
                (mm.form, mm.field) = (at.sub, None);
            }
            sound(w, Se::Cursor);
        }
        kit::Act::Set(s, x) => match ks[s] {
            Kind::Subs => {
                (mm.sub, mm.field) = (x, None);
                mm.stop_editing();
                sound(w, Se::Cursor);
            }
            Kind::Row(r) => set_row(w, &sel.id, r, x),
            _ => {}
        },
        kit::Act::Press(s, b) => press(w, &sel, &ks, &chips, s, b),
        kit::Act::Back => {
            (mm.focus, mm.field) = (Focus::List, None);
            mm.stop_editing();
            sound(w, Se::Cancel);
        }
        a => w.sounds.extend(a.sound().map(|s| s as u32)),
    }
}

/// The setting being typed kept, when what was typed is a value of its type (a colour #RRGGBB).
fn finish_edit(w: &mut World, id: &str) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let Some((r, typed)) = mm.editing.clone() else { return };
    let Some(old) = edited_row(mm, id, r).map(|row| (row.edit, row.choices.first().map(|c| c.1.clone()))) else {
        mm.editing = None;
        return;
    };
    let value = match old {
        (Edit::Colour, v) => super::super::lua::colour_typed(&typed, &v.unwrap_or_default()),
        _ => Some(serde_json::Value::String(typed)),
    };
    match value {
        Some(v) => {
            mm.editing = None;
            set_edited(w, id, r, v);
            sound(w, Se::Select);
        }
        None => sound(w, Se::Error),
    }
}

/// Settings' row `r` of mod `id` when it is a text, colour or key setting.
fn edited_row<'a>(mm: &'a ModMgr, id: &str, r: usize) -> Option<&'a ModRow> {
    let p = mm.pages.iter().find(|p| p.id.eq_ignore_ascii_case(id))?;
    p.rows.iter().filter(|r| !r.key.starts_with(PERM)).nth(r).filter(|r| r.edit != Edit::Choice)
}

/// A text, colour or key setting given value `v`: kept, and handed to the app for the mod.
fn set_edited(w: &mut World, id: &str, r: usize, v: serde_json::Value) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let Some(p) = mm.pages.iter_mut().find(|p| p.id.eq_ignore_ascii_case(id)) else { return };
    let Some(row) = p.rows.iter_mut().filter(|r| !r.key.starts_with(PERM)).nth(r) else { return };
    if row.choices.first().is_some_and(|c| c.1 == v) {
        return;
    }
    row.choices = vec![(super::super::lua::edit_shown(row.edit, &v), v.clone())];
    mm.s.mod_changes.push((p.id.clone(), row.key.clone(), v));
}

/// A key setting waiting: the first key pressed becomes its value (with Ctrl, Alt, Shift); Backspace or Delete
/// clears it, Escape keeps it as it was.
fn listen(w: &mut World, e: &egui::Event) {
    let egui::Event::Key { key, pressed: true, .. } = e else { return };
    let combo = match key {
        egui::Key::Escape => None,
        egui::Key::Backspace | egui::Key::Delete => Some(String::new()),
        _ => match w.lua.key_combo(e) {
            Some(c) => Some(c),
            // A key with no name (a modifier alone): keep waiting.
            None => return,
        },
    };
    let Some(mm) = w.modmgr.as_mut() else { return };
    let Some(r) = mm.listening.take() else { return };
    let Some(id) = mm.picked().map(|s| s.id) else { return };
    match combo {
        Some(c) => {
            set_edited(w, &id, r, serde_json::Value::String(c));
            sound(w, Se::Select);
        }
        None => sound(w, Se::Cancel),
    }
}

/// A setting or permission (the open sub's row `r`) set to choice `x`: kept, and handed to the app for the mod.
fn set_row(w: &mut World, id: &str, r: usize, x: usize) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let perm = mm.sub != SUB_SETTINGS;
    let Some(p) = mm.pages.iter_mut().find(|p| p.id.eq_ignore_ascii_case(id)) else { return };
    let Some(row) = p.rows.iter_mut().filter(|r| r.key.starts_with(PERM) == perm).nth(r) else { return };
    if let Some(c) = row.choices.get(x)
        && row.at != x
    {
        row.at = x;
        mm.s.mod_changes.push((p.id.clone(), row.key.clone(), c.1.clone()));
        sound(w, Se::Cursor);
    }
}

fn press(w: &mut World, sel: &Sel, ks: &[Kind], chips: &[Vec<String>], s: usize, b: usize) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    match ks[s] {
        Kind::Header if b < 2 => {
            mm.s.mod_enable(mm.sel, b == 0);
            flush(w);
        }
        Kind::Header if b == 3 => {
            mm.s.mod_update_now(mm.sel);
            flush(w);
        }
        Kind::Header => {
            mm.s.m_remove = Some((mm.sel, false));
            sound(w, Se::Select);
        }
        // A form taken: the cursor to its first field (or Run).
        Kind::Forms => {
            (mm.form, mm.field) = (b, None);
            let ks = kinds(sel, mm.sub, mm.form);
            if let Some(k) = ks.iter().position(|k| matches!(k, Kind::Field(_) | Kind::Run)) {
                mm.at = At::new(k, 0);
            }
            sound(w, Se::Select);
        }
        Kind::Field(i) if b == 0 => {
            mm.field = Some(i);
            sound(w, Se::Select);
        }
        Kind::Field(i) => {
            if let Some(c) = chips.get(i).and_then(|c| c.get(b - 1)) {
                mm.values.insert((sel.id.clone(), mm.form, i), c.clone());
                sound(w, Se::Select);
            }
        }
        Kind::Run => run(w, b == 1),
        // A text, colour or key setting: typing it (its value to start from), waiting for its key, or its picker.
        Kind::Row(r) if mm.sub == SUB_SETTINGS => {
            let Some(row) = sel.rows.get(r).filter(|row| row.edit != Edit::Choice) else { return };
            let shown = row.choices.first().map_or(String::new(), |c| c.0.clone());
            match (row.edit, b) {
                (Edit::Key, _) => {
                    (mm.editing, mm.picking, mm.listening) = (None, None, Some(r));
                }
                (Edit::Colour, 1) => {
                    mm.editing = None;
                    mm.picking = if mm.picking == Some(r) { None } else { Some(r) };
                }
                _ => {
                    (mm.listening, mm.picking) = (None, None);
                    let typed = if row.edit == Edit::Text {
                        row.choices.first().and_then(|c| c.1.as_str()).map_or(shown, str::to_string)
                    } else {
                        shown
                    };
                    mm.editing = Some((r, typed));
                }
            }
            sound(w, Se::Select);
        }
        _ => {}
    }
}

/// Run (`to_chat` false): the form's line typed as the chat line types it, the mod's replies shown from here; Put in
/// Chat Line: the line left on the chat line to finish.
fn run(w: &mut World, to_chat: bool) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let Some(sel) = mm.picked() else { return };
    let Some(f) = sel.forms.get(mm.form) else { return };
    let Ok(text) = line(f, &mm.values_of(&sel.id, mm.form, fields(f).len())) else {
        return sound(w, Se::Error);
    };
    mm.field = None;
    if to_chat {
        w.chat_input = format!("{text} ");
        (w.chat_caret, w.chat_anchor) = (None, None);
        w.typing = true;
    } else if sel.on && mm.other_owner(&sel, &f.cmd).is_none() {
        mm.ran = Some((sel.id.clone(), w.lua.print_seq(), text.clone()));
        w.lua.type_line(&text);
    } else {
        return sound(w, Se::Error);
    }
    sound(w, Se::Select);
}

/// Get mods' stops: Install from File, the standard mods, the archives found.
fn get_stops(s: &SettingsUi) -> Vec<Stop> {
    let busy = s.mods_state().is_some_and(|m| m.installing().is_some());
    let standard = s.mods_state().map_or(Vec::new(), |m| m.standard().iter().map(|(_, st)| st.is_none()).collect());
    std::iter::once(Stop::Buttons { n: 1, off: busy as u64, pair: None })
        .chain(standard.into_iter().map(|done| Stop::Buttons { n: 1, off: (busy || done) as u64, pair: None }))
        .chain(s.mods_rows().1.iter().filter(|o| !o.all).map(|_| Stop::Buttons { n: 1, off: busy as u64, pair: None }))
        .collect()
}

fn get_key(w: &mut World, key: egui::Key, shift: bool) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    let st = get_stops(&mm.s);
    let mut at = At::new(mm.get_row.min(st.len() - 1), 0);
    let act = kit::key(&st, &mut at, key, shift);
    mm.get_row = at.stop;
    match act {
        kit::Act::Press(k, _) => get_press(w, k),
        kit::Act::Back => close(w),
        a => w.sounds.extend(a.sound().map(|s| s as u32)),
    }
}

/// Get mods' row `k` pressed: the file picker, a standard mod installed, an archive installed.
fn get_press(w: &mut World, k: usize) {
    let Some(mm) = w.modmgr.as_mut() else { return };
    mm.get_row = k;
    let n = mm.s.mods_state().map_or(0, |m| m.standard().len());
    if k == 0 {
        if let Some(m) = mm.s.mods_mut() {
            m.browse = Some(super::super::modsui::Browse::open(true));
            m.error = None;
        }
        return sound(w, Se::Select);
    }
    if k <= n {
        let ok = mm.s.mods_mut().is_some_and(|m| m.install_standard(k - 1));
        return sound(w, if ok { Se::Select } else { Se::Error });
    }
    mm.s.archive_install(k - 1 - n);
    flush(w);
}

/// What a frame's clicks asked for, applied after drawing.
enum Click {
    Tab(usize),
    Find,
    Mod(usize),
    /// A page stop's control pressed, or a value set: (stop, button or value).
    Press(usize, usize),
    Set(usize, usize),
    /// A stop clicked outside its controls: the cursor there.
    Point(usize),
    Field(usize),
    /// A colour setting (Settings' row) picked with the mouse.
    Colour(usize, [u8; 3]),
    Remove(bool),
    Get(usize),
    GetPoint(usize),
    Dats(Vec<ModDo>),
    UpdateAll,
    Close,
}

/// A text, colour or key setting (Settings' row `k`): its value in a field (typed in after Enter or a click; a key
/// setting waits for its key), a colour's swatch and Pick… with the picker under it when open, then its help.
fn edit_row(
    ui: &mut Ui,
    m: &super::Modern,
    mm: &ModMgr,
    r: &ModRow,
    k: usize,
    focus: bool,
    follow: bool,
    click: &mut Vec<Click>,
) -> Rect {
    let st = 2 + k;
    let font = FontId::proportional(m.theme.small);
    let typing = mm.editing.as_ref().filter(|e| e.0 == k).map(|e| e.1.as_str());
    let waiting = mm.listening == Some(k);
    let value = r.choices.first().map_or(String::new(), |c| c.0.clone());
    kit::setting(ui, m, &r.label, focus, false, follow, |ui| {
        let field = |ui: &mut Ui, width: f32, click: &mut Vec<Click>| {
            let shown = match (typing, waiting) {
                (Some(t), _) => t.to_string(),
                (_, true) => "Press a key (Escape: keep, Backspace: none)".to_string(),
                _ => value.clone(),
            };
            let rect = kit::field(ui, m, &shown, typing.map(|t| t.chars().count()), width, font.clone(), None);
            if focus && mm.at.sub == 0 && typing.is_none() {
                kit::ring(ui.painter(), m, rect, 6.0);
            }
            if ui.interact(rect, ui.scope_id().with(("mods setting", &r.key)), Sense::click()).clicked() {
                click.push(Click::Press(st, 0));
            }
        };
        match r.edit {
            Edit::Colour => {
                let rgba = r.choices.first().and_then(|c| super::super::lua::colour_rgba(&c.1)).unwrap_or([255; 4]);
                ui.horizontal(|ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                    let c = Color32::from_rgb(rgba[0], rgba[1], rgba[2]);
                    ui.painter().rect_filled(sw.shrink(3.0), 3.0, c);
                    ui.painter().rect_stroke(
                        sw.shrink(3.0),
                        3.0,
                        Stroke::new(1.0, m.theme.color("muted")),
                        StrokeKind::Inside,
                    );
                    field(ui, 130.0, click);
                    let s = Btn { lit: mm.picking == Some(k), cursor: focus && mm.at.sub == 1, off: false };
                    if kit::clicked(&kit::button(ui, m, "Pick…", 72.0, s), s) {
                        click.push(Click::Press(st, 1));
                    }
                });
                if mm.picking == Some(k)
                    && let Some(c) = kit::colour_picker(ui, m, [rgba[0], rgba[1], rgba[2]], 300.0)
                {
                    click.push(Click::Colour(k, c));
                }
            }
            Edit::Key => field(ui, 300.0, click),
            _ => field(ui, PAGE_W - 40.0, click),
        }
        if !r.help.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(&r.help).size(m.theme.small - 1.0).color(m.theme.color("muted")))
                    .wrap(),
            );
        }
    })
    .1
}

fn heading(ui: &mut Ui, m: &super::Modern, s: &str) {
    ui.label(
        egui::RichText::new(s.to_uppercase())
            .size(m.theme.small - 2.0)
            .color(m.theme.color("bronze"))
            .family(FontFamily::Name(super::STRONG.into())),
    );
    ui.add_space(2.0);
}

fn note(ui: &mut Ui, m: &super::Modern, s: &str, colour: &str) {
    ui.label(egui::RichText::new(s).size(m.theme.small).color(m.theme.color(colour)));
}

/// A small ON / OFF tag.
fn tag(ui: &Ui, m: &super::Modern, right: egui::Pos2, on: bool) -> f32 {
    let font = FontId::proportional(m.theme.small - 2.0);
    let text = if on { "ON" } else { "OFF" };
    let g = ui.painter().layout_no_wrap(text.into(), font, m.theme.color(if on { "gold" } else { "muted" }));
    let r = Rect::from_min_size(pos2(right.x - g.size().x - 12.0, right.y - 9.0), vec2(g.size().x + 12.0, 18.0));
    ui.painter().rect_stroke(
        r,
        3.0,
        Stroke::new(1.0, m.theme.color(if on { "bronze" } else { "charcoal" })),
        StrokeKind::Inside,
    );
    ui.painter().galley(pos2(r.min.x + 6.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
    r.width()
}

fn list(ui: &mut Ui, m: &super::Modern, mm: &ModMgr, updates: &[bool], follow: bool, click: &mut Vec<Click>) {
    ui.vertical(|ui| {
        ui.set_width(LIST_W);
        let placeholder = mm.find.is_empty() && !mm.finding;
        let r = kit::field(
            ui,
            m,
            if placeholder { "Search mods" } else { &mm.find },
            (mm.finding || (mm.focus == Focus::List && !mm.find.is_empty())).then(|| mm.find.chars().count()),
            LIST_W,
            FontId::proportional(m.theme.small),
            Some("icons/find"),
        );
        if ui.interact(r, ui.scope_id().with("mods find"), Sense::click()).clicked() {
            click.push(Click::Find);
        }
        ui.add_space(6.0);
        let rows = mm.s.mods_rows().0;
        let shown = mm.shown();
        if rows.is_empty() {
            note(ui, m, "No mods installed yet: Get mods has ours.", "muted");
        } else if shown.is_empty() {
            note(ui, m, "No mod matches.", "muted");
        }
        let cursor = shown.iter().position(|&i| i == mm.sel);
        let tall = ui.ctx().content_rect().height() * 0.45;
        let font = FontId::proportional(m.theme.small);
        let out = kit::list(
            ui,
            m,
            "mods list",
            shown.len(),
            cursor,
            mm.focus == Focus::List,
            follow,
            ROW_H,
            tall,
            |_| false,
            |ui, row| {
                let i = shown[row.index];
                let (name, on) = &rows[i];
                let right = pos2(row.rect.max.x - 8.0, row.rect.center().y);
                let mut room = tag(ui, m, right, *on) + 8.0;
                if updates.get(i).copied().unwrap_or(false) {
                    let g = ui.painter().layout_no_wrap("update".into(), font.clone(), m.theme.color("gold"));
                    room += g.size().x + 8.0;
                    ui.painter().galley(pos2(right.x - room, right.y - g.size().y / 2.0), g, Color32::WHITE);
                }
                let label = kit::elide(ui, name, &font, row.rect.width() - room - 20.0);
                let ink = if Some(row.index) == cursor { m.theme.color("gold") } else { kit::ink(m, row) };
                ui.painter().text(pos2(row.rect.min.x + 10.0, right.y), Align2::LEFT_CENTER, label, font.clone(), ink);
            },
        );
        if let Some(k) = out.clicked {
            click.push(Click::Mod(shown[k]));
        }
        // Update All: every installed mod with a newer version, one after another.
        if let Some(st) = mm.s.mods_state().filter(|s| s.updates(true) > 0) {
            ui.add_space(6.0);
            let b = Btn { cursor: mm.focus == Focus::All, off: st.waiting(true) == 0, ..Default::default() };
            if kit::clicked(&kit::button(ui, m, &format!("Update All ({})", st.updates(true)), LIST_W, b), b) {
                click.push(Click::UpdateAll);
            }
        }
    });
}

/// The picked mod's page: its head, then the sub's tabs and body.
fn page(
    ui: &mut Ui,
    m: &super::Modern,
    mm: &ModMgr,
    sel: &Sel,
    ks: &[Kind],
    chips: &[Vec<String>],
    replies: &[&str],
    follow: bool,
    click: &mut Vec<Click>,
) {
    let cur =
        |k: Kind| (mm.focus == Focus::Page).then(|| ks.iter().position(|x| *x == k)).flatten() == Some(mm.at.stop);
    ui.vertical(|ui| {
        ui.set_width(PAGE_W);
        // The head: name, version, the state buttons; the description.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&sel.name).size(m.theme.ui + 4.0).color(m.theme.color("gold")));
            let mut v = if sel.info.version.is_empty() { String::new() } else { sel.info.version.clone() };
            if v.is_empty() {
                v = sel.id.clone();
            } else {
                v = format!("{v} · {}", sel.id);
            }
            ui.label(egui::RichText::new(v).size(m.theme.small).color(m.theme.color("muted")));
            let state = mm.s.mods_state();
            let news = match (state.and_then(|s| s.progress(&sel.id)), &sel.update) {
                (Some(pm), _) => Some(format!("updating: {}%", pm / 10)),
                (None, Some(_)) if state.is_some_and(|s| s.queued(&sel.id)) => Some("update queued".into()),
                (None, Some(v)) => Some(format!("{v} is out")),
                _ => None,
            };
            if let Some(n) = news {
                ui.label(egui::RichText::new(n).size(m.theme.small).color(m.theme.color("gold")));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let on_head = cur(Kind::Header);
                // Right to left: Update (when there is one), Uninstall, OFF, ON.
                let busy = state.is_some_and(|s| s.progress(&sel.id).is_some() || s.queued(&sel.id));
                for b in (0..3 + sel.update.is_some() as usize).rev() {
                    let label = ["ON", "OFF", "Uninstall", "Update"][b];
                    let s = Btn {
                        lit: (b == 0 && sel.on) || (b == 1 && !sel.on),
                        cursor: on_head && mm.at.sub == b,
                        off: b == 3 && busy,
                    };
                    if kit::clicked(&kit::button(ui, m, label, if b >= 2 { 0.0 } else { 64.0 }, s), s) {
                        click.push(Click::Press(0, b));
                    }
                }
            });
        });
        if !sel.info.description.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(sel.info.description.trim()).size(m.theme.small).color(m.theme.color("text")),
                )
                .wrap(),
            );
        }
        ui.add_space(6.0);
        if let Some((_, yes)) = mm.s.m_remove {
            let q = format!("Uninstall {}? Its folder in the mods folder is deleted.", sel.name);
            if let Some(a) = kit::yes_no(ui, m, &q, ["Yes", "No"], yes, m.theme.small) {
                click.push(Click::Remove(a));
            }
            return;
        }
        let counts = [sel.rows.len(), sel.forms.len(), sel.perms.len()];
        let labels: Vec<String> =
            SUBS.iter().zip(counts).map(|(s, n)| if n == 0 { s.to_string() } else { format!("{s} ({n})") }).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        if let Some(k) = kit::tab_strip(ui, m, "mods subs", &labels, mm.sub, cur(Kind::Subs)) {
            click.push(Click::Set(1, k));
        }
        let tall = ui.ctx().content_rect().height() * 0.4;
        match mm.sub {
            SUB_COMMANDS => commands(ui, m, mm, sel, ks, chips, replies, follow, tall, click),
            _ => {
                let rows = if mm.sub == SUB_SETTINGS { &sel.rows } else { &sel.perms };
                if rows.is_empty() {
                    let none = if mm.sub == SUB_SETTINGS {
                        "This mod has no settings."
                    } else {
                        "This mod asks for no permissions."
                    };
                    note(ui, m, none, "muted");
                }
                kit::scroll(ui, ("mods rows", mm.sub), tall, |ui| {
                    ui.add_space(4.0);
                    for (k, r) in rows.iter().enumerate() {
                        let st = 2 + k;
                        let focus = mm.focus == Focus::Page && mm.at.stop == st;
                        let rect = if r.edit != Edit::Choice {
                            edit_row(ui, m, mm, r, k, focus, follow, click)
                        } else if slider(r) {
                            let shown = r.choices.get(r.at).map_or(String::new(), |c| c.0.clone());
                            let (i, rect) =
                                kit::slider_setting(ui, m, &r.label, focus, follow, r.choices.len(), r.at, &shown);
                            if let Some(i) = i {
                                click.push(Click::Set(st, i));
                            }
                            rect
                        } else {
                            kit::setting(ui, m, &r.label, focus, true, follow, |ui| {
                                let labels: Vec<String> = r.choices.iter().map(|c| c.0.clone()).collect();
                                if let Some(c) = kit::choice(ui, m, &labels, r.at, 0) {
                                    click.push(Click::Set(st, c));
                                }
                                if !r.help.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&r.help)
                                                .size(m.theme.small - 1.0)
                                                .color(m.theme.color("muted")),
                                        )
                                        .wrap(),
                                    );
                                }
                            })
                            .1
                        };
                        if kit::row_clicked(ui, rect) {
                            click.push(Click::Point(st));
                        }
                    }
                });
            }
        }
    });
}

/// Commands: the forms at the left, the picked form at the right (its fields, the line, Run, the replies).
fn commands(
    ui: &mut Ui,
    m: &super::Modern,
    mm: &ModMgr,
    sel: &Sel,
    ks: &[Kind],
    chips: &[Vec<String>],
    replies: &[&str],
    follow: bool,
    tall: f32,
    click: &mut Vec<Click>,
) {
    if sel.forms.is_empty() {
        note(ui, m, "This mod declares no commands.", "muted");
        return;
    }
    let forms_stop = ks.iter().position(|k| *k == Kind::Forms).unwrap_or(2);
    let on_forms = mm.focus == Focus::Page && mm.at.stop == forms_stop;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(FORMS_W);
            let mono = FontId::monospace(m.theme.small);
            let small = FontId::proportional(m.theme.small - 1.0);
            let out = kit::list(
                ui,
                m,
                ("mods forms", &sel.id),
                sel.forms.len(),
                Some(mm.form),
                on_forms,
                follow,
                46.0,
                tall,
                |_| false,
                |ui, row| {
                    let f = &sel.forms[row.index];
                    let head =
                        if f.args.is_empty() { format!("//{}", f.cmd) } else { format!("//{} {}", f.cmd, f.args) };
                    let head = kit::elide(ui, &head, &mono, row.rect.width() - 20.0);
                    let ink = if row.index == mm.form { m.theme.color("gold") } else { m.theme.color("text") };
                    ui.painter().text(
                        pos2(row.rect.min.x + 10.0, row.rect.min.y + 14.0),
                        Align2::LEFT_CENTER,
                        head,
                        mono.clone(),
                        ink,
                    );
                    let help = kit::elide(ui, &f.help, &small, row.rect.width() - 20.0);
                    ui.painter().text(
                        pos2(row.rect.min.x + 10.0, row.rect.max.y - 12.0),
                        Align2::LEFT_CENTER,
                        help,
                        small.clone(),
                        m.theme.color("muted"),
                    );
                },
            );
            if let Some(k) = out.clicked {
                click.push(Click::Press(forms_stop, k));
            }
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_width(PAGE_W - FORMS_W - 16.0);
            form(ui, m, mm, sel, ks, chips, replies, click);
        });
    });
}

fn form(
    ui: &mut Ui,
    m: &super::Modern,
    mm: &ModMgr,
    sel: &Sel,
    ks: &[Kind],
    chips: &[Vec<String>],
    replies: &[&str],
    click: &mut Vec<Click>,
) {
    let Some(f) = sel.forms.get(mm.form) else { return };
    let width = ui.available_width();
    ui.add(egui::Label::new(egui::RichText::new(&f.help).size(m.theme.small).color(m.theme.color("text"))).wrap());
    ui.add_space(4.0);
    let fs = fields(f);
    let vals = mm.values_of(&sel.id, mm.form, fs.len());
    for (i, (label, key, optional)) in fs.iter().enumerate() {
        let st = ks.iter().position(|k| *k == Kind::Field(i)).unwrap_or(0);
        let here = mm.focus == Focus::Page && mm.at.stop == st;
        let p = param(f, key);
        let kind = p.map_or("", |p| p.kind.as_str());
        let mut head = if *optional { format!("{label} (optional)") } else { label.to_string() };
        if !kind.is_empty() && kind != "text" {
            head = format!("{head} · {}", kind.replace('_', " "));
        }
        ui.label(
            egui::RichText::new(head)
                .size(m.theme.small)
                .family(FontFamily::Name(super::STRONG.into()))
                .color(m.theme.color(if here { "gold" } else { "muted" })),
        );
        let typing = mm.field == Some(i);
        let r = kit::field(
            ui,
            m,
            &vals[i],
            typing.then(|| vals[i].chars().count()),
            width,
            FontId::proportional(m.theme.small),
            None,
        );
        if here && mm.at.sub == 0 && !typing {
            kit::ring(ui.painter(), m, r, 6.0);
        }
        if ui.interact(r, ui.scope_id().with(("mods field", &sel.id, mm.form, i)), Sense::click()).clicked() {
            click.push(Click::Field(i));
        }
        let cs = chips.get(i).map_or(&[][..], Vec::as_slice);
        if !cs.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (k, c) in cs.iter().enumerate() {
                    let s = Btn {
                        lit: vals[i].trim().eq_ignore_ascii_case(c),
                        cursor: here && mm.at.sub == k + 1,
                        off: false,
                    };
                    if kit::clicked(&kit::chip(ui, m, c, s, None), s) {
                        click.push(Click::Press(st, k + 1));
                    }
                }
            });
        }
        ui.add_space(4.0);
    }
    ui.add_space(4.0);
    let result = line(f, &vals);
    let (text, colour) = match &result {
        Ok(l) => (l.clone(), "gold"),
        Err(e) => (e.clone(), "hp"),
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text).size(m.theme.small).family(FontFamily::Monospace).color(m.theme.color(colour)),
        )
        .wrap(),
    );
    ui.add_space(4.0);
    let st = ks.iter().position(|k| *k == Kind::Run).unwrap_or(0);
    let here = mm.focus == Focus::Page && mm.at.stop == st;
    ui.horizontal(|ui| {
        let other = mm.other_owner(sel, &f.cmd).is_some();
        let run =
            Btn { cursor: here && mm.at.sub == 0, off: result.is_err() || !sel.on || other, ..Default::default() };
        if kit::clicked(&kit::button(ui, m, "Run", 0.0, run), run) {
            click.push(Click::Press(st, 0));
        }
        let chat = Btn { cursor: here && mm.at.sub == 1, off: result.is_err(), ..Default::default() };
        if kit::clicked(&kit::button(ui, m, "Put in Chat Line", 0.0, chat), chat) {
            click.push(Click::Press(st, 1));
        }
    });
    if !sel.on {
        note(ui, m, "The mod is off: turn it on to run its commands.", "muted");
    } else if let Some(f) = sel.forms.get(mm.form)
        && let Some(owner) = mm.other_owner(sel, &f.cmd)
    {
        let t = format!("//{} runs {owner}'s command ({owner} was loaded first): Run is off here.", f.cmd);
        note(ui, m, &t, "muted");
    }
    if let Some((_, _, l)) = mm.ran.as_ref().filter(|r| r.0 == sel.id) {
        ui.add_space(8.0);
        heading(ui, m, "Mod replies");
        ui.label(
            egui::RichText::new(l).size(m.theme.small).family(FontFamily::Monospace).color(m.theme.color("muted")),
        );
        kit::scroll(ui, ("mods replies", &sel.id), 160.0, |ui| {
            if replies.is_empty() {
                note(ui, m, "Nothing printed yet.", "muted");
            }
            for r in replies {
                ui.add(
                    egui::Label::new(egui::RichText::new(*r).size(m.theme.small).color(m.theme.color("text"))).wrap(),
                );
            }
        });
    }
}

/// Get mods: Install from File, the standard mods, the archives found; the details of the row under the cursor.
fn get_mods(ui: &mut Ui, m: &super::Modern, s: &SettingsUi, row: usize, follow: bool, click: &mut Vec<Click>) {
    let Some(state) = s.mods_state() else { return };
    let busy = state.installing();
    let standard = state.standard();
    // The archives found (Update All is the Installed tab's).
    let offers: Vec<_> = s.mods_rows().1.into_iter().filter(|o| !o.all).collect();
    let mut hover = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(LIST_W + 16.0 + FORMS_W + 60.0);
            let b = Btn { cursor: row == 0, off: busy.is_some(), ..Default::default() };
            if kit::clicked(&kit::button(ui, m, "Install from File...", 0.0, b), b) {
                click.push(Click::Get(0));
            }
            if let Some((slug, pm)) = &busy {
                note(ui, m, &format!("Installing {slug}: {}%", pm / 10), "gold");
            }
            if let Some(e) = state.error.as_ref() {
                note(ui, m, &format!("The last install failed: {e}"), "hp");
            }
            ui.add_space(6.0);
            let tall = ui.ctx().content_rect().height() * 0.45;
            kit::scroll(ui, "get mods", tall, |ui| {
                ui.add_space(4.0);
                heading(ui, m, "Standard mods");
                if standard.is_empty() {
                    let text = match &state.list_error {
                        Some(e) => format!("The standard mods could not be read: {e}"),
                        None if state.loading() => "Reading the list from GitHub...".into(),
                        None => "No standard mods are listed.".into(),
                    };
                    note(ui, m, &text, "muted");
                }
                for (k, (e, st)) in standard.iter().enumerate() {
                    let r = k + 1;
                    let label = match state.progress(&e.id) {
                        Some(pm) => format!("{}%  {} {}", pm / 10, e.name, e.version),
                        None => format!("{} {}", e.name, e.version),
                    };
                    let (_, rect) = kit::line(ui, m, &label, row == r, follow, |ui| {
                        let (text, off) = match st {
                            Some(true) => ("Update", busy.is_some()),
                            Some(false) => ("Install", busy.is_some()),
                            None => ("Installed", true),
                        };
                        let b = Btn { cursor: row == r, off, ..Default::default() };
                        if kit::clicked(&kit::button(ui, m, text, 96.0, b), b) {
                            click.push(Click::Get(r));
                        }
                    });
                    if ui.rect_contains_pointer(rect) {
                        hover = Some(r);
                    }
                    if kit::row_clicked(ui, rect) {
                        click.push(Click::GetPoint(r));
                    }
                }
                if !offers.is_empty() {
                    ui.add_space(6.0);
                    heading(ui, m, "Found in the Downloads folder");
                }
                for (k, o) in offers.iter().enumerate() {
                    let r = 1 + standard.len() + k;
                    let (_, rect) = kit::line_elided(ui, m, &o.label, row == r, follow, |ui| {
                        let b = Btn { cursor: row == r, off: busy.is_some(), ..Default::default() };
                        if kit::clicked(&kit::button(ui, m, o.button, 96.0, b), b) {
                            click.push(Click::Get(r));
                        }
                    });
                    if ui.rect_contains_pointer(rect) {
                        hover = Some(r);
                    }
                    if kit::row_clicked(ui, rect) {
                        click.push(Click::GetPoint(r));
                    }
                }
            });
        });
        ui.add_space(16.0);
        let at = hover.unwrap_or(row);
        let d = match at {
            0 => Detail {
                name: "Install from File".into(),
                text: "Pick a mod's archive (zip, 7z, rar) or its folder: it is copied into the mods folder, on."
                    .into(),
                facts: Vec::new(),
            },
            r if r <= standard.len() => {
                let (e, _) = &standard[r - 1];
                let mut facts = vec![("Version", e.version.clone())];
                if !e.authors.is_empty() {
                    facts.push(("By", e.authors.join(", ")));
                }
                Detail { name: e.name.clone(), text: crate::modpack::with_commands(&e.description, &e.commands), facts }
            }
            r => offers.get(r - 1 - standard.len()).map_or_else(Detail::default, |o| Detail {
                name: o.label.clone(),
                text: o.help.clone(),
                facts: Vec::new(),
            }),
        };
        config::details(ui, m, &d);
    });
}

fn apply(w: &mut World, clicks: Vec<Click>) {
    for c in clicks {
        let Some(mm) = w.modmgr.as_mut() else { return };
        mm.s.sounds.clear();
        // A click anywhere but the setting being typed (or its picker) keeps what was typed; one waiting for its key
        // stops waiting.
        let on = |r: usize| matches!(c, Click::Press(st, 0) if st == 2 + r);
        if let Some((r, _)) = mm.editing
            && !on(r)
            && !matches!(c, Click::Colour(..))
        {
            if let Some(id) = mm.picked().map(|s| s.id) {
                finish_edit(w, &id);
            }
            let Some(mm) = w.modmgr.as_mut() else { return };
            mm.editing = None;
        }
        let Some(mm) = w.modmgr.as_mut() else { return };
        if mm.listening.is_some_and(|r| !on(r)) {
            mm.listening = None;
        }
        match c {
            Click::Tab(k) => turn(w, k),
            Click::Find => {
                (mm.finding, mm.focus, mm.field) = (true, Focus::List, None);
            }
            Click::Mod(i) => {
                if i != mm.sel {
                    sound(w, Se::Cursor);
                }
                let Some(mm) = w.modmgr.as_mut() else { return };
                mm.pick(i);
                (mm.focus, mm.finding) = (Focus::List, false);
            }
            Click::Point(st) => {
                (mm.focus, mm.at, mm.field) = (Focus::Page, At::new(st, 0), None);
            }
            Click::Field(i) => {
                let Some(sel) = mm.picked() else { continue };
                let ks = kinds(&sel, mm.sub, mm.form);
                let st = ks.iter().position(|k| *k == Kind::Field(i)).unwrap_or(0);
                (mm.focus, mm.at, mm.field, mm.finding) = (Focus::Page, At::new(st, 0), Some(i), false);
            }
            Click::Set(st, x) => {
                mm.focus = Focus::Page;
                mm.at = At::new(st, if st == 1 { 0 } else { mm.at.sub });
                let Some(sel) = mm.picked() else { continue };
                let ks = kinds(&sel, mm.sub, mm.form);
                match ks.get(st) {
                    Some(Kind::Subs) if x != mm.sub => {
                        (mm.sub, mm.field) = (x, None);
                        sound(w, Se::Cursor);
                    }
                    Some(Kind::Row(r)) => set_row(w, &sel.id, *r, x),
                    _ => {}
                }
            }
            Click::Press(st, b) => {
                let Some(sel) = mm.picked() else { continue };
                let ks = kinds(&sel, mm.sub, mm.form);
                (mm.focus, mm.at, mm.finding) = (Focus::Page, At::new(st, b), false);
                let active = active_field(mm, &ks);
                let chips = form_chips(mm, &sel, active);
                if ks.get(st).is_some() {
                    press(w, &sel, &ks, &chips, st, b);
                }
            }
            Click::Colour(r, rgb) => {
                let Some(id) = mm.picked().map(|s| s.id) else { continue };
                let old = edited_row(mm, &id, r).and_then(|row| row.choices.first().map(|c| c.1.clone()));
                let v = super::super::lua::colour_value(rgb, &old.unwrap_or_default());
                set_edited(w, &id, r, v);
            }
            Click::Remove(yes) => {
                config::remove_answer(&mut mm.s, yes);
                let n = mm.s.mods_rows().0.len();
                mm.sel = mm.sel.min(n.saturating_sub(1));
                mm.focus = Focus::List;
                flush(w);
            }
            Click::Get(r) => get_press(w, r),
            Click::GetPoint(r) => {
                if mm.get_row != r {
                    mm.get_row = r;
                    sound(w, Se::Cursor);
                }
            }
            Click::Dats(act) => {
                if config::mods_do(&mut mm.s, act) == ModOut::Back {
                    mm.open = false;
                }
                flush(w);
            }
            Click::UpdateAll => {
                mm.focus = Focus::All;
                update_all(w);
            }
            Click::Close => close(w),
        }
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !shown(w) {
        return;
    }
    // In front when it opens; over a menu column window (the Settings window it may be opened from), which asks to be
    // on top every frame: egui keeps the order among those, so it is tied above it instead.
    let layer = |id: &str| egui::LayerId::new(egui::Order::Middle, egui::Id::unique(("modern panel", id)));
    if w.modmgr.as_mut().is_some_and(|m| std::mem::take(&mut m.raise)) {
        p.ctx.move_to_top(layer("mods"));
    }
    // One parent only (egui places a sublayer after one): the window column's when shown, else the menu's.
    if let Some(c) = super::COLUMN.iter().rev().find(|c| p.this.contains_key(**c)) {
        p.ctx.set_sublayer(layer(c), layer("mods"));
    }
    fill_pickers(w);
    // Update All gone (every update done): the cursor back on the list.
    if let Some(mm) = w.modmgr.as_mut()
        && mm.focus == Focus::All
        && mm.s.mods_state().is_none_or(|s| s.updates(true) == 0)
    {
        mm.focus = Focus::List;
    }
    // Who runs each command now, for Run (a name two mods register is the first loaded's).
    if let Some(mm) = w.modmgr.as_mut()
        && mm.tab == 0
        && mm.sub == SUB_COMMANDS
    {
        mm.owners = w.lua.command_list().into_iter().filter(|c| !c.id.is_empty()).map(|c| (c.name, c.id)).collect();
    }
    let Some(mm) = w.modmgr.as_ref() else { return };
    let sel = if mm.tab == 0 { mm.picked() } else { None };
    let replies: Vec<&str> = match (&mm.ran, &sel) {
        (Some((id, seq, _)), Some(s)) if *id == s.id => w.lua.printed_since(id, *seq),
        _ => Vec::new(),
    };
    let clicks = window(p, mm, sel.as_ref(), &replies);
    apply(w, clicks);
}

/// The open form's pickers' choices: the party twice a second, the name tables once.
fn fill_pickers(w: &mut World) {
    let Some(mm) = w.modmgr.as_ref() else { return };
    if mm.lists_at.is_some_and(|t| t.elapsed().as_secs_f32() < 0.5) || mm.tab != 0 || mm.sub != SUB_COMMANDS {
        return;
    }
    let picks: Vec<Pick> = mm
        .picked()
        .and_then(|s| s.forms.get(mm.form).map(|f| f.params.iter().filter_map(|p| pick_of(&p.kind)).collect()))
        .unwrap_or_default();
    let mut people: gm::Choices = std::iter::once(w.name.clone())
        .chain(w.party.members().filter(|r| r.id != w.my_id).map(|r| r.name.clone()))
        .filter(|n| !n.is_empty())
        .map(|n| (n.clone(), n))
        .collect();
    people.dedup();
    let names = w.names.clone();
    let Some(mm) = w.modmgr.as_mut() else { return };
    mm.lists_at = Some(std::time::Instant::now());
    mm.pickers.fill(&picks, names.as_ref(), people, Vec::new());
}

fn window(p: &mut Panels, mm: &ModMgr, sel: Option<&Sel>, replies: &[&str]) -> Vec<Click> {
    let m = p.m;
    let mut click = Vec::new();
    let follow = m.moved(
        "mods window",
        mm.tab * 10_000_000 + mm.sel * 10_000 + mm.at.stop * 100 + mm.at.sub + mm.get_row * 1000 + mm.s.m_mods.0 * 7,
    );
    let rows = mm.s.mods_rows().0;
    let updates: Vec<bool> = (0..rows.len())
        .map(|i| {
            let id = mm.s.mod_id(i).unwrap_or_default();
            mm.s.mods_state()
                .is_some_and(|s| s.standard().iter().any(|(e, st)| e.id.eq_ignore_ascii_case(&id) && *st == Some(true)))
        })
        .collect();
    let mut help = String::new();
    p.panel(
        "mods",
        Some(Title { icon: Some("icons/config"), text: "Mods", note: Some("Vana Client"), name: false }),
        LIST_W + 16.0 + PAGE_W,
        |ui| {
            if let Some(k) = kit::tab_strip(ui, m, "mods tabs", &TABS, mm.tab, false) {
                click.push(Click::Tab(k));
            }
            let browsing = mm.s.mods_state().is_some_and(|s| s.browse.is_some());
            // One height whatever the tab or mod shows, so the window keeps its size.
            let body = ui.ctx().content_rect().height() * 0.5;
            ui.vertical(|ui| {
                ui.set_min_height(body);
                match mm.tab {
                    _ if browsing => {
                        let mut act = Vec::new();
                        config::mods_page(ui, m, &mm.s, true, follow, &mut act);
                        if !act.is_empty() {
                            click.push(Click::Dats(act));
                        }
                        help = config::mods_help(&mm.s);
                    }
                    0 => {
                        let ks = sel.map_or(Vec::new(), |s| kinds(s, mm.sub, mm.form));
                        let chips = sel.map_or(Vec::new(), |s| form_chips(mm, s, active_field(mm, &ks)));
                        ui.horizontal_top(|ui| {
                            list(ui, m, mm, &updates, follow, &mut click);
                            ui.add_space(16.0);
                            match sel {
                                Some(s) => page(ui, m, mm, s, &ks, &chips, replies, follow, &mut click),
                                None => {
                                    ui.vertical(|ui| {
                                        ui.set_width(PAGE_W);
                                        note(ui, m, "Pick a mod at the left.", "muted");
                                    });
                                }
                            }
                        });
                        help = installed_help(mm, sel, &ks);
                    }
                    1 => {
                        get_mods(ui, m, &mm.s, mm.get_row, follow, &mut click);
                        help = "Up / Down: a mod. Enter: install it (or update it). Tab: the next tab. Escape: close."
                            .into();
                    }
                    _ => {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(LIST_W + 16.0 + FORMS_W + 60.0);
                                let mut act = Vec::new();
                                config::mods_page(ui, m, &mm.s, true, follow, &mut act);
                                if !act.is_empty() {
                                    click.push(Click::Dats(act));
                                }
                            });
                            ui.add_space(16.0);
                            let d = config::row_detail(&mm.s, mm.s.m_mods.0).unwrap_or_else(|| Detail {
                                name: "DATs".into(),
                                text: config::DATS_HELP.into(),
                                facts: Vec::new(),
                            });
                            config::details(ui, m, &d);
                        });
                        help = config::mods_help(&mm.s);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let b = Btn::default();
                if kit::clicked(&kit::button(ui, m, "Close", 90.0, b), b) {
                    click.push(Click::Close);
                }
                ui.add(
                    egui::Label::new(egui::RichText::new(&help).size(m.theme.small).color(m.theme.color("muted")))
                        .wrap(),
                );
            });
        },
    );
    click
}

/// Installed's help line: the cursor's.
fn installed_help(mm: &ModMgr, sel: Option<&Sel>, ks: &[Kind]) -> String {
    let Some(sel) = sel else { return "Type to search. Up / Down: a mod. Tab: the next tab. Escape: close.".into() };
    if mm.s.m_remove.is_some() {
        return "Left / Right: Yes or No. Enter: answer. Escape: No.".into();
    }
    if mm.focus == Focus::List {
        return "Type to search. Up / Down: a mod. Enter or Right: its page. Tab: the next tab. Escape: close.".into();
    }
    if mm.focus == Focus::All {
        return "Enter: update every mod with a newer version, one after another; a mod that is on restarts on it. Up: \
                back to the list."
            .into();
    }
    if mm.field.is_some() {
        return "Type the value. Tab: the next field. Enter or Escape: done.".into();
    }
    if mm.editing.is_some() {
        return "Type the value. Enter: keep it. Escape: leave it as it was.".into();
    }
    if mm.listening.is_some() {
        return "Press the key, with Ctrl, Alt or Shift if you like. Backspace: no key. Escape: leave it as it was."
            .into();
    }
    let rows = if mm.sub == SUB_SETTINGS { &sel.rows } else { &sel.perms };
    match ks.get(mm.at.stop) {
        Some(Kind::Header) => match &sel.update {
            Some(v) => format!(
                "ON starts the mod at once, OFF stops it. Uninstall deletes its folder. Update gets version {v} from \
                 GitHub; a mod that is on restarts on it."
            ),
            None => "ON starts the mod at once, OFF stops it. Uninstall deletes its folder.".into(),
        },
        Some(Kind::Subs) => "Left / Right: Settings, Commands or Permissions. Escape: back to the list.".into(),
        Some(Kind::Row(r)) => rows.get(*r).map_or(String::new(), |r| {
            let how = match r.edit {
                Edit::Choice => "Left / Right: change it.",
                Edit::Text => "Enter: type it.",
                Edit::Colour => "Enter: type it as #RRGGBB. Right, then Enter: the colour picker.",
                Edit::Key => "Enter, then press the key.",
            };
            if r.help.is_empty() { how.into() } else { format!("{} {how}", r.help) }
        }),
        Some(Kind::Forms) => sel.forms.get(mm.form).map_or(String::new(), |f| format!("{} Enter: fill it in.", f.help)),
        Some(Kind::Field(_)) => {
            "Enter: type in it; Left / Right then Enter: a suggestion. Run types the line as the chat line would."
                .into()
        }
        Some(Kind::Run) => {
            "Run types the line for you, as if typed in the chat line; the mod's replies show here.".into()
        }
        None => String::new(),
    }
}

/// `modern-shot --mods`: a made-up Look Changer on Commands, its slot form filled, a reply.
pub(super) fn sample(p: &mut Panels) {
    let mut s = SettingsUi::open(&crate::config::Config::default(), [1920, 1080]);
    s.in_game = true;
    s.mods_ui = Some(super::super::modsui::Mods::empty());
    let mods = [
        ("auction-desk", "Auction Desk", false),
        ("chat-alerts", "Chat Alerts", true),
        ("hotbar", "Hotbars", true),
        ("inventory-hud", "Inventory HUD", true),
        ("look-changer", "Look Changer", true),
        ("loot-rules", "Loot Rules", true),
        ("map-overlay", "Map Overlay", true),
        ("trust-lineups", "Trust Lineups", false),
        ("warp-helper", "Warp Helper", true),
    ];
    s.lua_names.clear();
    s.lua_info.clear();
    s.s.mods.clear();
    for (id, name, on) in mods {
        s.s.mods.push(crate::config::ModRef::new(id, on));
        s.lua_names.insert(id.into());
        s.lua_info.insert(id.into(), LuaInfo { name: name.into(), ..Default::default() });
    }
    let mut mm = ModMgr::new(s, Vec::new(), None);
    mm.sel = 4;
    let toml = r#"
name = "Look Changer"
version = "1.0.0"
description = "Changes how characters look on your screen only: your race, face and gear, a target's look copied, one race for everyone else."
[commands.look]
help = "Changes how characters look, on your screen only."
forms = [
  { args = "race <race>", help = "Your race." },
  { args = "face <1a..8b>", help = "Your face." },
  { args = "<slot> <item | none | own>", help = "Wear an item's look in one slot.", params = [
      { name = "slot", type = "enum", options = ["head", "body", "hands", "legs", "feet", "main", "sub", "range"] },
      { name = "item", type = "item", also = ["none", "own"] } ] },
  { args = "copy [name]", help = "Copies a player's look (your target's)." },
  { args = "reset", help = "Your look back." },
]
"#;
    let info: LuaInfo = toml::from_str(toml).unwrap_or_default();
    let sel = Sel {
        id: "look-changer".into(),
        name: info.name.clone(),
        on: true,
        update: None,
        rows: Vec::new(),
        perms: Vec::new(),
        forms: forms_of(&info),
        info,
    };
    (mm.sub, mm.form, mm.focus) = (SUB_COMMANDS, 2, Focus::Page);
    mm.values.insert(("look-changer".into(), 2, 0), "body".into());
    mm.values.insert(("look-changer".into(), 2, 1), "Kupo Suit".into());
    mm.ran = Some(("look-changer".into(), 0, "//look body Kupo Suit".into()));
    let ks = kinds(&sel, mm.sub, mm.form);
    mm.at = At::new(ks.iter().position(|k| *k == Kind::Run).unwrap_or(0), 0);
    let m = p.m;
    let chips = form_chips(&mm, &sel, None);
    let mut click = Vec::new();
    p.panel(
        "mods",
        Some(Title { icon: Some("icons/config"), text: "Mods", note: Some("Vana Client"), name: false }),
        LIST_W + 16.0 + PAGE_W,
        |ui| {
            kit::tab_strip(ui, m, "mods tabs", &TABS, 0, false);
            ui.horizontal_top(|ui| {
                list(ui, m, &mm, &[false, false, true], false, &mut click);
                ui.add_space(16.0);
                page(ui, m, &mm, &sel, &ks, &chips, &["Look Changer: body shows Kupo Suit."], false, &mut click);
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(args: &str, params: Vec<LuaParam>) -> Form {
        Form { cmd: "look".into(), args: args.into(), help: String::new(), params, parts: parts(args) }
    }

    /// A form's words: placeholders by their first alternative, optional ones left out of the line when empty, a word
    /// with brackets inside typed whole.
    #[test]
    fn form_lines() {
        let f = form("<slot> <item | none | own>", Vec::new());
        assert_eq!(fields(&f), [("slot", "slot", false), ("item | none | own", "item", false)]);
        assert_eq!(line(&f, &["body".into(), "Kupo Suit".into()]), Ok("//look body Kupo Suit".into()));
        assert_eq!(line(&f, &["body".into(), " ".into()]), Err("Fill in item | none | own.".into()));
        let f = form("copy [name]", Vec::new());
        assert_eq!(line(&f, &[String::new()]), Ok("//look copy".into()));
        let f = form("<item> [count], <item> [count]...", Vec::new());
        assert_eq!(fields(&f).len(), 4);
        assert!(matches!(&f.parts[1], Part::Field { key, optional: true, .. } if key == "[count],"));
        assert_eq!(line(&form("", Vec::new()), &[]), Ok("//look".into()));
    }

    /// An enum's options and the words it also takes are the field's buttons; a picker's suggestions follow.
    #[test]
    fn field_buttons() {
        let p = LuaParam {
            name: "item".into(),
            kind: "item".into(),
            options: Vec::new(),
            also: vec!["none".into(), "own".into()],
        };
        let found =
            gm::Found { hits: vec![("1".into(), "Kupo Suit".into()), ("2".into(), "Kupo Shield".into())], known: None };
        assert_eq!(chips(Some(&p), Some(&found)), ["none", "own", "Kupo Suit", "Kupo Shield"]);
        let exact =
            gm::Found { hits: vec![("1".into(), "Kupo Suit".into())], known: Some(("1".into(), "Kupo Suit".into())) };
        assert_eq!(chips(Some(&p), Some(&exact)), ["none", "own"]);
        assert_eq!(pick_of("key_item"), Some(Pick::KeyItem));
        assert_eq!(pick_of("text"), None);
    }
}
