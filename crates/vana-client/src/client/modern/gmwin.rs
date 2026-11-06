//! Ours (not retail, which has no GM menu): the Game Master window, opened from the Command Menu's Game Master row
//! (shown only while the server gives a GM level above 0, client/gm.rs). Built like the Settings window: the groups
//! down the left, the level's commands in pages in the middle, the chosen command's fields and the server's replies
//! at the right. Runs go out as c2s 0x1F1, not as a chat line. The mouse picks; typed text goes to the search or the
//! field last clicked, Tab moves to the next field, Enter runs, Escape leaves the field, then the window.

use super::super::World;
use super::super::gm::{self, Command, Pick};
use super::kit::{self, Btn};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Sense, Ui, pos2, vec2};

const NAV_W: f32 = 190.0;
const LIST_W: f32 = 420.0;
const DETAIL_W: f32 = 330.0;
const ROW_H: f32 = 44.0;

/// What a frame's clicks asked for, applied after drawing.
enum Click {
    Group(Option<usize>),
    Page(usize),
    Select(String),
    Focus(Option<usize>),
    Fill(usize, String),
    Position,
    Augments,
    Run,
    Clear,
    Close,
}

/// Whether the window takes the keys (the key dispatch then gives the game none).
pub fn shown(w: &World) -> bool {
    w.modern && w.gm.open && w.gm.level > 0
}

/// Typed text, Backspace, Tab, Enter and Escape while the window is open. True when it took them.
pub fn events(w: &mut World, events: &[egui::Event]) -> bool {
    // Another window or dialog over it (opened later, or an event) keeps its own keys.
    if !shown(w) || w.typing || super::super::window_over_actions(w) || w.prompt.is_some() || super::super::event::active(w) {
        return false;
    }
    let mut run = false;
    for e in events {
        let g = &mut w.gm;
        let sel = g.selected.clone();
        let fields = g.selected().map_or(0, |c| c.fields.len());
        let slot = match (g.typing, &sel) {
            (Some(i), Some(n)) => Some((n.clone(), i)),
            _ => None,
        };
        match e {
            egui::Event::Text(t) => {
                let t: String = t.chars().filter(|c| !c.is_control()).collect();
                match &slot {
                    Some(k) => g.values.entry(k.clone()).or_default().push_str(&t),
                    None => {
                        g.find.push_str(&t);
                        (g.find_typing, g.page) = (true, 0);
                    }
                }
                g.armed = false;
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                match &slot {
                    Some(k) => {
                        g.values.entry(k.clone()).or_default().pop();
                    }
                    None => {
                        g.find.pop();
                        g.page = 0;
                    }
                }
                g.armed = false;
            }
            egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } if fields > 0 => {
                let n = fields;
                g.typing = Some(match g.typing {
                    Some(i) if modifiers.shift => (i + n - 1) % n,
                    Some(i) => (i + 1) % n,
                    None => 0,
                });
                g.find_typing = false;
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } if sel.is_some() => run = true,
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                if g.typing.is_some() || g.find_typing {
                    (g.typing, g.find_typing) = (None, false);
                } else {
                    g.open = false;
                }
                w.sounds.push(Se::Cancel as u32);
            }
            _ => {}
        }
    }
    if run {
        apply(w, Click::Run);
    }
    true
}

fn text(ui: &Ui, at: egui::Pos2, align: Align2, s: &str, size: f32, c: Color32) {
    ui.painter().text(at, align, s, FontId::proportional(size), c);
}

fn heading(ui: &mut Ui, m: &super::Modern, s: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(s).size(m.theme.small).family(egui::FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
}

/// The groups: All, then gm::GROUPS with their counts (those with none are left out).
fn rail(ui: &mut Ui, m: &super::Modern, g: &gm::Gm, click: &mut Option<Click>) {
    ui.vertical(|ui| {
        ui.set_width(NAV_W);
        let count = |k: Option<usize>| g.commands.iter().filter(|c| k.is_none_or(|k| gm::GROUPS[k].0 == c.group)).count();
        for k in std::iter::once(None).chain((0..gm::GROUPS.len()).map(Some)) {
            let n = count(k);
            if n == 0 {
                continue;
            }
            let (r, resp) = ui.allocate_exact_size(vec2(NAV_W, 28.0), Sense::click());
            let on = g.group == k;
            let face = if on { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            let ink = if on { m.theme.color("gold") } else { m.theme.color("text") };
            let label = k.map_or("All commands", |k| gm::GROUPS[k].1);
            text(ui, pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, label, m.theme.ui, ink);
            text(ui, pos2(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, &n.to_string(), m.theme.small, m.theme.color("muted"));
            if resp.clicked() {
                *click = Some(Click::Group(k));
            }
            ui.add_space(2.0);
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new(format!("Ours: not in the retail client. Shown to GMs only; level {} sees {} commands.", g.level, g.commands.len())).size(m.theme.small).color(m.theme.color("muted")));
    });
}

/// The search and one page of the shown commands, with the pager.
fn list(ui: &mut Ui, m: &super::Modern, g: &gm::Gm, click: &mut Option<Click>) {
    ui.vertical(|ui| {
        ui.set_width(LIST_W);
        let r = kit::field(ui, m, if g.find.is_empty() && !g.find_typing { "Search commands" } else { &g.find }, g.find_typing.then(|| g.find.chars().count()), LIST_W, FontId::proportional(m.theme.small), Some("icons/find"));
        if ui.interact(r, ui.scope_id().with("gm find"), Sense::click()).clicked() {
            *click = Some(Click::Focus(None));
        }
        ui.add_space(6.0);
        let shown = g.shown();
        let pages = shown.len().div_ceil(gm::PER_PAGE).max(1);
        let page = g.page.min(pages - 1);
        if shown.is_empty() {
            ui.label(egui::RichText::new("No commands match. Try another word, or All commands.").size(m.theme.small).color(m.theme.color("muted")));
        }
        for c in shown.iter().skip(page * gm::PER_PAGE).take(gm::PER_PAGE) {
            let (r, resp) = ui.allocate_exact_size(vec2(LIST_W, ROW_H), Sense::click());
            let on = g.selected.as_deref() == Some(c.name.as_str());
            let face = if on { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            ui.painter().text(pos2(r.min.x + 12.0, r.min.y + 14.0), Align2::LEFT_CENTER, format!("!{}", c.name), FontId::monospace(m.theme.small), if on { m.theme.color("gold") } else { m.theme.color("text") });
            let desc = if c.desc.is_empty() { "No description in the script." } else { &c.desc };
            let desc = kit::elide(ui, desc, &FontId::proportional(m.theme.small), LIST_W - 110.0);
            text(ui, pos2(r.min.x + 12.0, r.max.y - 12.0), Align2::LEFT_CENTER, &desc, m.theme.small, m.theme.color("muted"));
            let tag = if c.danger { format!("Danger · GM {}", c.level) } else { format!("GM {}", c.level) };
            text(ui, pos2(r.max.x - 10.0, r.min.y + 14.0), Align2::RIGHT_CENTER, &tag, m.theme.small, if c.danger { m.theme.color("hp") } else { m.theme.color("bronze") });
            if resp.clicked() {
                *click = Some(Click::Select(c.name.clone()));
            }
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let prev = Btn { off: page == 0, ..Default::default() };
            if kit::clicked(&kit::button(ui, m, "Previous", 90.0, prev), prev) {
                *click = Some(Click::Page(page - 1));
            }
            ui.label(egui::RichText::new(format!("Page {} of {pages}", page + 1)).size(m.theme.small).color(m.theme.color("muted")));
            let next = Btn { off: page + 1 >= pages, ..Default::default() };
            if kit::clicked(&kit::button(ui, m, "Next", 90.0, next), next) {
                *click = Some(Click::Page(page + 1));
            }
        });
    });
}

/// The chosen command: its fields, the line it sends, Run, and the replies.
fn detail(ui: &mut Ui, m: &super::Modern, g: &gm::Gm, target: Option<u32>, click: &mut Option<Click>) {
    ui.vertical(|ui| {
        ui.set_width(DETAIL_W);
        let Some(c) = g.selected() else {
            heading(ui, m, "Details");
            ui.label(egui::RichText::new("Pick a command to see its fields.").size(m.theme.small).color(m.theme.color("muted")));
            replies(ui, m, g);
            return;
        };
        heading(ui, m, &format!("!{} · needs GM {}", c.name, c.level));
        if !c.desc.is_empty() {
            ui.label(egui::RichText::new(&c.desc).size(m.theme.small).color(m.theme.color("text")));
        }
        if c.danger {
            ui.label(egui::RichText::new("This command can delete data, disrupt the server or affect other players. Run asks twice.").size(m.theme.small).color(m.theme.color("hp")));
        }
        let values = g.values_of(c);
        let result = c.line(&values);
        if c.fields.is_empty() {
            ui.label(egui::RichText::new("No fields: runs as it is.").size(m.theme.small).color(m.theme.color("muted")));
        }
        let mut coords = false;
        let augs = c.fields.iter().filter(|f| f.pick == Pick::Augment).count();
        for (i, f) in c.fields.iter().enumerate() {
            if f.pick == Pick::Augment && !g.augments && !result.as_ref().err().is_some_and(|e| e.0 == i) && values[i].is_empty() {
                continue;
            }
            field(ui, m, g, c, f, i, &values[i], result.as_ref().err().is_some_and(|e| e.0 == i), target, click);
            coords |= f.pick == Pick::Coord;
        }
        if augs > 0 {
            let b = Btn { lit: g.augments, ..Default::default() };
            let label = if g.augments { "Hide augments".to_string() } else { format!("Augments ({} pairs)", augs / 2) };
            if kit::clicked(&kit::button(ui, m, &label, 0.0, b), b) {
                *click = Some(Click::Augments);
            }
        }
        if coords {
            let b = Btn::default();
            if kit::clicked(&kit::button(ui, m, "Use my position", 0.0, b), b) {
                *click = Some(Click::Position);
            }
        }
        ui.add_space(6.0);
        let line = match &result {
            Ok(l) => format!("!{l}"),
            Err((_, e)) => e.clone(),
        };
        ui.label(egui::RichText::new(line).size(m.theme.small).family(egui::FontFamily::Monospace).color(if result.is_ok() { m.theme.color("gold") } else { m.theme.color("hp") }));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let run = Btn { off: result.is_err(), lit: g.armed, ..Default::default() };
            if kit::clicked(&kit::button(ui, m, if g.armed { "Press again to run" } else { "Run" }, 0.0, run), run) {
                *click = Some(Click::Run);
            }
            let clear = Btn::default();
            if kit::clicked(&kit::button(ui, m, "Clear", 0.0, clear), clear) {
                *click = Some(Click::Clear);
            }
        });
        replies(ui, m, g);
    });
}

#[allow(clippy::too_many_arguments)]
fn field(ui: &mut Ui, m: &super::Modern, g: &gm::Gm, c: &Command, f: &gm::Field, i: usize, value: &str, bad: bool, target: Option<u32>, click: &mut Option<Click>) {
    let typing = g.typing == Some(i);
    let list = g.lists.get(&f.pick);
    let label = if f.optional { format!("{} (optional)", f.label) } else { f.label.clone() };
    ui.label(egui::RichText::new(label).size(m.theme.small).color(if bad { m.theme.color("hp") } else { m.theme.color("text") }));
    ui.horizontal(|ui| {
        let extra = matches!(f.pick, Pick::Entity) && target.is_some();
        let width = if extra { DETAIL_W - 100.0 } else { DETAIL_W };
        let r = kit::field(ui, m, value, typing.then(|| value.chars().count()), width, FontId::proportional(m.theme.small), None);
        if ui.interact(r, ui.scope_id().with(("gm field", &c.name, i)), Sense::click()).clicked() {
            *click = Some(Click::Focus(Some(i)));
        }
        if let (true, Some(t)) = (extra, target) {
            let b = Btn::default();
            if kit::clicked(&kit::button(ui, m, "Use target", 92.0, b), b) {
                *click = Some(Click::Fill(i, t.to_string()));
            }
        }
    });
    // The name behind a typed id (or the id behind a typed name), so a number reads as what it is.
    let known = list.and_then(|l| {
        let t = value.trim();
        l.iter().find(|(id, n)| !t.is_empty() && (id == t || n.eq_ignore_ascii_case(t)))
    });
    let hint = match known {
        Some((id, n)) if id == value.trim() => n.clone(),
        Some((id, _)) => format!("Sends {id}."),
        None => f.pick.hint().to_string(),
    };
    if !hint.is_empty() {
        ui.label(egui::RichText::new(hint).size(m.theme.small * 0.9).color(m.theme.color(if known.is_some() { "gold" } else { "muted" })));
    }
    if let (true, Some(l)) = (typing, list) {
        let hits = gm::suggest(l, value, SUGGEST);
        if !hits.is_empty() && known.is_none_or(|k| hits.len() > 1 || hits[0] != k) {
            ui.horizontal_wrapped(|ui| {
                for (id, n) in hits {
                    let b = Btn::default();
                    let font = FontId::proportional(m.theme.small);
                    let name = kit::elide(ui, n, &font, 180.0);
                    let label = if id == n { name } else { format!("{name} ({id})") };
                    if kit::clicked(&kit::button(ui, m, &label, 0.0, b), b) {
                        *click = Some(Click::Fill(i, id.clone()));
                    }
                }
            });
        }
    }
}

/// How many suggestions show under the field being typed in.
const SUGGEST: usize = 6;

/// The last replies, newest first: the line, what happened, what the command printed.
fn replies(ui: &mut Ui, m: &super::Modern, g: &gm::Gm) {
    if g.replies.is_empty() {
        return;
    }
    ui.add_space(8.0);
    heading(ui, m, "Server replies");
    kit::scroll(ui, "gm replies", 220.0, |ui| {
        for (_, r) in g.replies.iter().rev() {
            ui.label(egui::RichText::new(format!("!{}", r.line)).size(m.theme.small).family(egui::FontFamily::Monospace).color(m.theme.color("gold")));
            let ok = r.result == Some(0);
            ui.label(egui::RichText::new(r.status()).size(m.theme.small).color(if ok || r.result.is_none() { m.theme.color("muted") } else { m.theme.color("hp") }));
            for l in &r.output {
                ui.label(egui::RichText::new(l).size(m.theme.small).color(m.theme.color("text")));
            }
            if r.cut {
                ui.label(egui::RichText::new("(more output was cut off)").size(m.theme.small).color(m.theme.color("muted")));
            }
            ui.add_space(6.0);
        }
    });
}

fn apply(w: &mut World, click: Click) {
    let g = &mut w.gm;
    match click {
        Click::Group(k) => (g.group, g.page) = (k, 0),
        Click::Page(p) => g.page = p,
        Click::Select(n) => {
            (g.selected, g.typing, g.find_typing, g.armed) = (Some(n), None, false, false);
        }
        Click::Focus(i) => (g.typing, g.find_typing) = (i, i.is_none()),
        Click::Fill(i, v) => {
            if let Some(n) = g.selected.clone() {
                g.values.insert((n, i), v);
            }
        }
        Click::Position => {
            let pos = [w.pos.x, w.pos.y, w.pos.z];
            let g = &mut w.gm;
            if let Some(c) = g.selected().cloned() {
                for (k, (i, _)) in c.fields.iter().enumerate().filter(|(_, f)| f.pick == Pick::Coord).enumerate().take(3) {
                    g.values.insert((c.name.clone(), i), format!("{:.3}", pos[k]));
                }
            }
        }
        Click::Clear => {
            if let Some(n) = g.selected.clone() {
                g.values.retain(|k, _| k.0 != n);
            }
            g.armed = false;
        }
        Click::Close => g.open = false,
        Click::Augments => g.augments = !g.augments,
        Click::Run => {
            let Some(c) = g.selected().cloned() else { return };
            let Ok(line) = c.line(&g.values_of(&c)) else {
                w.sounds.push(Se::Error as u32);
                return;
            };
            if c.danger && !g.armed {
                g.armed = true;
                w.sounds.push(Se::Cursor as u32);
                return;
            }
            g.armed = false;
            let body = g.run(line);
            let _ = w.net.commands.send(super::super::net::NetCommand::Packet { id: 0x1F1, body });
            w.sounds.push(Se::Select as u32);
        }
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !shown(w) {
        return;
    }
    if w.gm.selected.is_some() {
        let mut people: gm::Choices = std::iter::once(w.name.clone()).chain(w.party.members().filter(|r| r.id != w.my_id).map(|r| r.name.clone())).filter(|n| !n.is_empty()).map(|n| (n.clone(), n)).collect();
        people.dedup();
        let mut around: gm::Choices = w.entities.iter().filter(|(_, e)| !e.name.trim().is_empty()).map(|(id, e)| (id.to_string(), e.name.trim().to_string())).collect();
        around.sort_by(|a, b| a.1.cmp(&b.1));
        w.gm.fill_lists(w.names.as_ref(), people, around);
    }
    if let Some(c) = window(p, &w.gm, w.target) {
        apply(w, c);
    }
}

/// The modern-shot sample: a level 5 GM's window on !additem, with a reply.
pub(super) fn sample(p: &mut Panels) {
    let g = gm::sample();
    window(p, &g, Some(17_195_010));
}

fn window(p: &mut Panels, g: &gm::Gm, target: Option<u32>) -> Option<Click> {
    let m = p.m;
    let mut click = None;
    let note = format!("GM {}", g.level);
    p.panel("window", Some(Title { icon: Some("icons/system"), text: "Game Master", note: Some(&note) }), NAV_W + LIST_W + DETAIL_W + 48.0, |ui| {
        ui.horizontal_top(|ui| {
            rail(ui, m, g, &mut click);
            ui.add_space(16.0);
            list(ui, m, g, &mut click);
            ui.add_space(16.0);
            detail(ui, m, g, target, &mut click);
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = Btn::default();
            if kit::clicked(&kit::button(ui, m, "Close", 90.0, b), b) {
                click = Some(Click::Close);
            }
            ui.label(egui::RichText::new("Commands run directly, not through chat. The server checks your level again.").size(m.theme.small).color(m.theme.color("muted")));
        });
    });
    click
}
