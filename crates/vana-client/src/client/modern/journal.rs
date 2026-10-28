//! The modern Journal in the command window: Missions, Quests and Key Items as tabs of one window over
//! client/missions.rs, quests.rs and keyitems.rs (their state and rules). Each lists its logs, areas or categories on
//! the left and the chosen one's rows on the right, with the cursor row's description under them at once (retail asks
//! for Enter, and for new key items a question). A mission log's current and completed missions, an area's current and
//! completed quests, show together. The Records of Eminence lists open retail's over it.

use super::super::game_ui::GameUi;
use super::super::journal::{self, Row};
use super::super::{World, keyitems, listsearch, missions, quests, roe};
use super::{Panels, Title, lists};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const NAV_W: f32 = 230.0;
const LIST_W: f32 = 430.0;
const ROW: f32 = 28.0;
const ROWS: usize = 11;
const DESC_H: f32 = 120.0;
/// A cursor row in the column without the keys.
const UNFOCUSED: Color32 = Color32::from_gray(95);

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Missions,
    Quests,
    Roe,
    KeyItems,
}

const TABS: [(Tab, &str); 4] = [(Tab::Missions, "Missions"), (Tab::Quests, "Quests"), (Tab::Roe, "Records of Eminence"), (Tab::KeyItems, "Key Items")];

/// The tab open (none while a Records of Eminence list shows retail's).
fn tab(w: &World) -> Option<Tab> {
    if !w.modern {
        return None;
    }
    if w.missions.is_some() {
        Some(Tab::Missions)
    } else if let Some(q) = &w.quests {
        Some(if q.roe.is_some() || w.journal_roe.is_some() { Tab::Roe } else { Tab::Quests })
    } else if w.keyitems.as_ref().is_some_and(|k| k.ask.is_none()) {
        Some(Tab::KeyItems)
    } else {
        None
    }
}

/// A quest area's name: its "Current Quests" title before the colon.
pub(super) fn area_title(gu: &GameUi, area: usize) -> String {
    let t = gu.title_msg(quests::TITLES[0][area]).unwrap_or_default();
    t.split(':').next().unwrap_or_default().trim().to_string()
}

pub(super) fn log_title(gu: &GameUi, log: usize) -> String {
    gu.title_msg(journal::MISSION_TITLES[log]).unwrap_or_default()
}

fn close(w: &mut World) {
    (w.missions, w.quests, w.keyitems, w.journal_roe, w.journal_on_list) = (None, None, None, None, false);
}

/// Records of Eminence: quest00's rows 2..4 (Current objectives, Objective List, Vana'Bout; FUN_10230740(row)).
fn roe_open(w: &mut World, k: usize) {
    if k == 2 && !w.roe.vanabout() {
        w.sounds.push(Se::Error as u32);
        return;
    }
    w.journal_roe = Some(k);
    w.sounds.push(Se::Select as u32);
    roe::open(w, k + 2);
}

/// Another tab: the open one closes, the other opens as its command would.
fn switch(w: &mut World, to: Tab) {
    close(w);
    match to {
        Tab::Missions => missions::open_menu(w),
        Tab::Quests => quests::open(w),
        Tab::Roe => {
            quests::open(w);
            w.journal_roe = Some(0);
        }
        Tab::KeyItems => keyitems::open(w),
    }
    w.sounds.push(Se::Cursor as u32);
}

/// The lists retail opens one step later open at once: the nation's mission log, the nation's quests, the first
/// key item category.
fn settle(w: &mut World) {
    // A list opened elsewhere (/roe ...) shows on its tab.
    if w.journal_roe.is_none()
        && let Some(l) = w.quests.as_ref().and_then(|q| q.roe.as_ref())
    {
        w.journal_roe = Some(l.mode.saturating_sub(2).min(2));
    }
    if w.missions.as_ref().is_some_and(|m| m.list.is_none()) {
        missions::show_log(w, (w.journal.nation as usize).min(2), None);
    }
    if w.journal_roe.is_none() && w.quests.as_ref().is_some_and(|q| q.state == 1 && q.list.is_none() && q.roe.is_none()) {
        quests::show_area(w, (w.journal.nation as usize).min(2), None);
    }
    if let Some(k) = w.keyitems.as_mut()
        && k.state == 3
    {
        k.state = 2;
    }
    if let Some(k) = w.keyitems.as_ref().filter(|k| k.state == 1) {
        // Opening on its first category with something held (retail's cursor starts on the first).
        let cats = keyitems::all(w);
        let cat = (k.cursor..cats.len()).chain(0..k.cursor).find(|&c| cats[c].1.iter().any(|(id, _)| w.has_key_item(*id))).unwrap_or(k.cursor);
        keyitems::show_category(w, cat);
    }
}

/// The next key item category (Left / Right).
fn next_category(w: &mut World, forward: bool) {
    let n = keyitems::all(w).len().max(1);
    let Some(cat) = w.keyitems.as_ref().map(|k| k.cat) else { return };
    keyitems::show_category(w, if forward { (cat + 1) % n } else { (cat + n - 1) % n });
}

/// Enter on a key item: a new one is marked seen (retail's "Examine?" answered yes).
fn examine(w: &mut World) {
    let Some(id) = w.keyitems.as_ref().and_then(|k| k.items.get(k.cursor)).map(|x| x.0) else { return };
    if !w.ki_seen.seen(id) {
        keyitems::mark_seen(w, id);
    }
    w.sounds.push(Se::Select as u32);
}

/// The keys follow the columns as drawn: on the logs, areas or categories at the left Up / Down step them (retail's
/// Left / Right) and Right or Enter go to the rows; on the rows Up / Down (with Ctrl / Alt and Left / Right a page) are
/// retail's, Left or Escape go back. Enter marks a new key item seen, Tab the next tab, Escape at the left closes. True
/// when the Journal took the key.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key, page: bool) -> bool {
    use egui::Key as K;
    let Some(t) = tab(w) else { return false };
    if w.confirm.is_some() {
        return false;
    }
    settle(w);
    let on_list = w.journal_on_list && t != Tab::Roe;
    match (t, key) {
        (Tab::Roe, K::Escape) if w.quests.as_ref().is_some_and(|q| q.roe.is_some()) => roe::modern_key(w, key, page),
        (_, K::Escape | K::ArrowLeft) if on_list && !page => {
            w.journal_on_list = false;
            w.sounds.push(if key == K::Escape { Se::Cancel } else { Se::Cursor } as u32);
        }
        (_, K::Escape) => {
            w.sounds.push(Se::Cancel as u32);
            close(w);
        }
        (_, K::Tab) => {
            let k = TABS.iter().position(|x| x.0 == t).unwrap_or(0);
            switch(w, TABS[(k + 1) % TABS.len()].0);
        }
        (Tab::Missions | Tab::Quests | Tab::KeyItems, K::ArrowUp | K::ArrowDown) if !on_list => {
            let side = if key == K::ArrowDown { K::ArrowRight } else { K::ArrowLeft };
            match t {
                Tab::Missions => missions::key(w, gu, side, false),
                Tab::Quests => quests::key(w, side, false),
                _ => {
                    next_category(w, key == K::ArrowDown);
                    w.sounds.push(Se::Cursor as u32);
                }
            }
        }
        (Tab::Missions | Tab::Quests | Tab::KeyItems, K::ArrowRight | K::Enter) if !on_list => {
            w.journal_on_list = true;
            w.sounds.push(Se::Select as u32);
        }
        (Tab::Missions | Tab::Quests | Tab::KeyItems, K::ArrowLeft | K::ArrowRight) if !page => {}
        (Tab::Missions, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight) => missions::key(w, gu, key, page),
        (Tab::Quests, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight) => quests::key(w, key, page),
        (Tab::KeyItems, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight) => keyitems::key(w, gu, key, page),
        (Tab::KeyItems, K::Enter) => examine(w),
        // A list open: retail's keys (a category's Escape goes up, the root's back to the summary).
        (Tab::Roe, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight | K::Enter) if w.quests.as_ref().is_some_and(|q| q.roe.is_some()) => {
            roe::modern_key(w, key, page);
        }
        (Tab::Roe, K::ArrowUp | K::ArrowDown) => {
            let k = w.journal_roe.unwrap_or(0);
            w.journal_roe = Some(if key == K::ArrowDown { (k + 1) % 3 } else { (k + 2) % 3 });
            w.sounds.push(Se::Cursor as u32);
        }
        (Tab::Roe, K::Enter) => {
            let k = w.journal_roe.unwrap_or(0);
            roe_open(w, k);
        }
        (_, K::Enter) => {}
        _ => return false,
    }
    true
}

/// A list row as drawn: its text, indent, colour, whether it is a heading, a "new" mark.
struct Line {
    text: String,
    indent: f32,
    colour: Color32,
    head: bool,
    new: bool,
    /// An objective's progress (have, goal): a small bar and the numbers at the row's right.
    count: Option<(u32, u32)>,
}

/// What the mouse did on the page.
#[derive(Default)]
struct Mouse {
    tab: Option<Tab>,
    nav: Option<usize>,
    hover: Option<usize>,
    click: Option<usize>,
}

/// A navigation row (a log, an area, a category) with its count; the chosen one lit while this column has the keys.
fn nav_row(ui: &mut Ui, m: &super::Modern, text: &str, count: &str, on: bool, keys: bool, off: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(NAV_W, ROW), Sense::click());
    let face = match () {
        _ if on => "controls/list_row_selected",
        _ if off => "controls/list_row_disabled",
        _ if resp.hovered() => "controls/list_row_hover",
        _ => "controls/list_row_normal",
    };
    super::rows::face(ui.painter(), m, r, face, if on && !keys { UNFOCUSED } else { Color32::WHITE });
    let c = if off { m.theme.color("muted") } else { m.theme.color("text") };
    let clip = Rect::from_min_max(r.min, pos2(r.max.x - if count.is_empty() { 8.0 } else { 44.0 }, r.max.y));
    ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), c);
    ui.painter().text(pos2(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, count, FontId::monospace(m.theme.small - 1.0), m.theme.color("muted"));
    ui.add_space(2.0);
    resp
}

/// The rows, the cursor's kept in view as it moves (lit while the rows have the keys); hover moves the cursor, a click
/// takes the row.
#[allow(clippy::too_many_arguments)]
fn list(ui: &mut Ui, m: &super::Modern, lines: &[Line], cursor: usize, keys: bool, follow: bool, moved: bool, mouse: &mut Mouse) {
    egui::ScrollArea::vertical().id_salt("journal list").max_height(ROWS as f32 * ROW).min_scrolled_height(ROWS as f32 * ROW).auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(LIST_W);
        for (k, l) in lines.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(LIST_W, ROW), if l.head { Sense::hover() } else { Sense::click() });
            if l.head {
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y + 2.0), Align2::LEFT_CENTER, &l.text, FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
                continue;
            }
            let face = match () {
                _ if k == cursor => "controls/list_row_selected",
                _ if resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            let tint = if k == cursor && !keys { UNFOCUSED } else { Color32::WHITE };
            super::rows::face(ui.painter(), m, r.shrink2(vec2(0.0, 1.0)), face, tint);
            let mut x = r.min.x + 24.0 + l.indent;
            if l.new {
                ui.painter().circle_filled(pos2(x + 3.0, r.center().y), 4.0, m.theme.color("gold"));
                x += 12.0;
            }
            let right = if l.count.is_some() { r.max.x - 120.0 } else { r.max.x - 8.0 };
            let clip = Rect::from_min_max(r.min, pos2(right, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(x, r.center().y), Align2::LEFT_CENTER, &l.text, FontId::proportional(m.theme.small), l.colour);
            if let Some((have, goal)) = l.count {
                let bar = Rect::from_min_size(pos2(r.max.x - 112.0, r.center().y - 3.0), vec2(48.0, 6.0));
                let f = if goal == 0 { 1.0 } else { (have as f32 / goal as f32).min(1.0) };
                ui.painter().rect_filled(bar, 3.0, m.theme.color("charcoal"));
                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())), 3.0, m.theme.color("gold"));
                ui.painter().text(pos2(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, format!("{have}/{goal}"), FontId::monospace(m.theme.small - 1.0), m.theme.color("text"));
            }
            if k == cursor && follow {
                ui.scroll_to_rect(r, None);
            }
            if resp.hovered() && moved {
                mouse.hover = Some(k);
            }
            if resp.clicked() {
                mouse.click = Some(k);
            }
        }
        if lines.is_empty() {
            ui.label(egui::RichText::new("None.").size(m.theme.small).color(m.theme.color("muted")));
        }
    });
}

/// The cursor row's name and description, wrapped.
fn description(ui: &mut Ui, m: &super::Modern, name: &str, text: &str) {
    ui.add_space(8.0);
    // As tall as its text needs (DESC_H at least), so nothing is cut.
    egui::Frame::NONE.fill(m.theme.color("charcoal").gamma_multiply(0.7)).corner_radius(6).inner_margin(10).show(ui, |ui| {
        ui.set_width(LIST_W - 20.0);
        ui.set_min_height(DESC_H - 20.0);
        ui.label(egui::RichText::new(name).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
        ui.add_space(2.0);
        super::gametext::label(ui, m, text, m.theme.small, m.theme.color("text"));
    });
}

/// The tab strip.
fn tabs(ui: &mut Ui, m: &super::Modern, t: Tab, mouse: &mut Mouse) {
    let labels: Vec<&str> = TABS.iter().map(|(_, l)| *l).collect();
    let open = TABS.iter().position(|(tab, _)| *tab == t);
    for (k, resp) in super::tabs::strip(ui, m, "journal tabs", &labels, open, None).into_iter().enumerate() {
        if resp.clicked() && Some(k) != open {
            mouse.tab = Some(TABS[k].0);
        }
    }
    ui.add_space(8.0);
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(t) = tab(w) else {
        w.journal_on_list = false;
        return;
    };
    let Some(gu) = p.gu else { return };
    settle(w);
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (white, muted, gold) = (m.theme.color("text"), m.theme.color("muted"), m.theme.color("gold"));
    let search = if listsearch::can_run(w) { lists::search_hits(p, w) } else { None };
    let mut mouse = Mouse::default();
    let mut found = None;

    // Gathered per tab: the nav rows (text, count, off), the one chosen, the heading, the lines, the cursor, the
    // description, whether the data is still coming.
    let mut nav: Vec<(String, String, bool)> = Vec::new();
    let mut chosen = 0;
    let mut heading = String::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor = 0;
    let mut desc: Option<(String, String)> = None;
    let mut waiting = false;
    let logs = w.logs.clone();
    let roe_list = if t == Tab::Roe { roe::view(w, gu) } else { None };
    match t {
        Tab::Missions => {
            let shown = missions::shown_logs(w);
            let l = w.missions.as_ref().and_then(|x| x.list.clone());
            for &log in &shown {
                nav.push((log_title(gu, log), String::new(), false));
            }
            if let Some(l) = l {
                chosen = shown.iter().position(|&x| x == l.log).unwrap_or(0);
                heading = log_title(gu, l.log);
                waiting = l.state == 0;
                let empty = crate::dat::journal::LogFile::default();
                let f = logs.as_ref().and_then(|g| g.missions.get(l.log)).unwrap_or(&empty);
                cursor = l.cursor;
                for r in &l.rows {
                    let line = match *r {
                        Row::CurHead => Line { text: gu.lobby_msg(0xc, 0x36, "Current:"), indent: 0.0, colour: gold, head: true, new: false, count: None },
                        Row::DoneHead => Line { text: gu.lobby_msg(0xc, 0x37, "Completed:"), indent: 0.0, colour: gold, head: true, new: false, count: None },
                        Row::Msg(id) => Line { text: gu.lobby_msg(0xc, id, ""), indent: 0.0, colour: muted, head: false, new: false, count: None },
                        Row::Current(i) | Row::Done(i) => {
                            let (name, n) = f.entries.get(i).map_or(("", 0), |e| journal::indent(&e.name));
                            let n = if matches!(r, Row::Current(_)) { 0.0 } else { 8.0 * n as f32 };
                            Line { text: name.to_string(), indent: n, colour: if matches!(r, Row::Current(_)) { gold } else { white }, head: false, new: false, count: None }
                        }
                    };
                    lines.push(line);
                }
                if let Some(Row::Current(i) | Row::Done(i)) = l.rows.get(l.cursor)
                    && let Some(e) = f.entries.get(*i)
                {
                    desc = Some((journal::indent(&e.name).0.to_string(), e.desc.clone()));
                }
            }
        }
        Tab::Quests => {
            let Some(q) = w.quests.clone() else { return };
            waiting = q.state == 0;
            let areas: Vec<usize> = (0..11).filter(|&a| quests::area_on(w.expansions, a)).collect();
            for &a in &areas {
                let (cur, done) = (quests::rows(w, a, 0).len(), quests::rows(w, a, 1).len());
                nav.push((area_title(gu, a), format!("{cur}/{done}"), cur + done == 0));
            }
            if let Some(l) = q.list {
                chosen = areas.iter().position(|&a| a == l.area).unwrap_or(0);
                heading = area_title(gu, l.area);
                let split = quests::rows(w, l.area, 0).len();
                let f = logs.as_ref().and_then(|g| g.quests.get(l.area));
                // Headings are drawn between the rows; the cursor walks the quests alone.
                let mut k_of = Vec::new();
                for (k, &i) in l.rows.iter().enumerate() {
                    if k == 0 && split > 0 {
                        lines.push(Line { text: gu.lobby_msg(0xc, 0x36, "Current:"), indent: 0.0, colour: gold, head: true, new: false, count: None });
                    }
                    if k == split {
                        lines.push(Line { text: gu.lobby_msg(0xc, 0x37, "Completed:"), indent: 0.0, colour: gold, head: true, new: false, count: None });
                    }
                    let name = f.and_then(|f| f.entries.get(i)).map_or(String::new(), |e| e.name.trim_start_matches('+').to_string());
                    k_of.push(lines.len());
                    lines.push(Line { text: name, indent: 0.0, colour: if k < split { gold } else { white }, head: false, new: false, count: None });
                }
                cursor = k_of.get(l.cursor).copied().unwrap_or(usize::MAX);
                if let Some(e) = l.rows.get(l.cursor).and_then(|&i| f.and_then(|f| f.entries.get(i))) {
                    desc = Some((e.name.trim_start_matches('+').to_string(), e.desc.clone()));
                }
                p.m.journal_rows.replace(k_of);
            }
        }
        Tab::Roe if roe_list.is_some() => {
            // A list open: its rows (categories with a mark to step in), the cursor row's description at once.
            let r = &w.roe;
            let v = roe_list.as_ref().unwrap();
            nav.push((gu.lobby_msg(0xc, 0x6d, "Current Objectives"), format!("{}/30", r.taken()), false));
            nav.push((gu.lobby_msg(0xc, 0x6e, "Objective List"), String::new(), false));
            nav.push((gu.lobby_msg(0xc, 0x208, "Vana'Bout"), String::new(), !r.vanabout()));
            chosen = v.mode.saturating_sub(2);
            heading = if v.deep { format!("‹ {}", v.title) } else { v.title.clone() };
            for row in &v.rows {
                let text = if row.category { format!("{}  ›", row.name) } else { row.name.clone() };
                lines.push(Line { text, indent: 0.0, colour: super::chat::colour(row.tint.to_le_bytes()), head: false, new: row.new, count: row.count });
            }
            if let Some(e) = &v.empty {
                lines.push(Line { text: e.clone(), indent: 0.0, colour: muted, head: true, new: false, count: None });
            }
            cursor = v.cursor;
            desc = v.desc.as_ref().map(|d| {
                let once = d.once.as_ref().map_or(String::new(), |o| format!("   ({o})"));
                (d.name.clone(), format!("{}\n\n{}: {}{once}\n{}: {}", d.text, d.progress_label, d.progress, d.rewards_label, d.rewards))
            });
        }
        Tab::Roe => {
            // The entries on the left, the totals on the right (quest00's panel, FUN_10233560).
            let r = &w.roe;
            let sel = w.journal_roe.unwrap_or(0);
            nav.push((gu.lobby_msg(0xc, 0x6d, "Current Objectives"), format!("{}/30", r.taken()), false));
            nav.push((gu.lobby_msg(0xc, 0x6e, "Objective List"), String::new(), false));
            nav.push((gu.lobby_msg(0xc, 0x208, "Vana'Bout"), String::new(), !r.vanabout()));
            chosen = sel;
            heading = gu.lobby_msg(0xc, 0x6c, "Records of Eminence");
            let accolades = if w.recasts_seen { w.stats.as_ref().map_or(0, |s| s.unity >> 10 & 0x1ffff) } else { 0 };
            for (label, v) in [
                (0x78, if r.flags & 1 != 0 { r.sparks & 0xff_ffff } else { 0 }),
                (0x79, accolades),
                (0x7a, if r.flags & 1 != 0 { r.deeds as u32 } else { 0 }),
                (0x82, r.completed()),
                (0x209, r.plaudits as u32),
            ] {
                lines.push(Line { text: format!("{}   {v}", gu.lobby_msg(0xc, label, "")), indent: 0.0, colour: if v >= 99999 { m.theme.color("focus") } else { white }, head: false, new: false, count: None });
            }
            cursor = usize::MAX;
            desc = Some((nav[sel].0.clone(), "Enter or a click on an entry at the left opens it.".into()));
        }
        Tab::KeyItems => {
            let Some(k) = w.keyitems.clone() else { return };
            waiting = k.state == 0;
            let cats = keyitems::all(w);
            for (title, items) in &cats {
                let n = items.iter().filter(|(id, _)| w.has_key_item(*id)).count();
                nav.push((title.clone(), n.to_string(), n == 0));
            }
            chosen = k.cat;
            heading = cats.get(k.cat).map_or(String::new(), |c| c.0.clone());
            cursor = k.cursor;
            for (id, name) in &k.items {
                lines.push(Line { text: name.clone(), indent: 0.0, colour: white, head: false, new: !w.ki_seen.seen(*id), count: None });
            }
            if let Some((id, name)) = k.items.get(k.cursor) {
                let d = w.names.as_ref().and_then(|n| n.key_item_description(*id as u32)).unwrap_or_default();
                desc = Some((name.clone(), d));
            }
        }
    }
    let on_list = w.journal_on_list && t != Tab::Roe;
    let follow = m.journal_seen.get() != (t as u8, cursor);
    m.journal_seen.set((t as u8, cursor));

    p.panel("window", Some(Title { icon: Some("icons/quest_log"), text: "Journal", note: None }), NAV_W + 18.0 + LIST_W, |ui| {
        tabs(ui, m, t, &mut mouse);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(NAV_W);
                egui::ScrollArea::vertical().id_salt("journal nav").max_height(ROWS as f32 * ROW + DESC_H + 40.0).min_scrolled_height(ROWS as f32 * ROW + DESC_H + 40.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (k, (text, count, off)) in nav.iter().enumerate() {
                        if nav_row(ui, m, text, count, k == chosen, !on_list, *off).clicked() {
                            mouse.nav = Some(k);
                        }
                    }
                });
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                if let Some((text, sel, hits, icons)) = &search {
                    found = Some(lists::search_rows(ui, m, w, icons, text, *sel, hits, moved));
                    return;
                }
                ui.label(egui::RichText::new(&heading).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(gold));
                ui.add_space(4.0);
                if waiting {
                    ui.label(egui::RichText::new("Preparing data...").size(m.theme.small).color(muted));
                    return;
                }
                list(ui, m, &lines, cursor, on_list, follow, moved, &mut mouse);
                match &desc {
                    Some((name, text)) => description(ui, m, name, text),
                    None => {
                        ui.add_space(8.0 + DESC_H);
                    }
                }
                ui.label(egui::RichText::new("Type to search every list.").size(m.theme.small - 2.0).color(muted));
            });
        });
    });

    if let (Some(f), Some((_, _, hits, _))) = (found, &search) {
        lists::search_mouse(w, &f, hits);
        return;
    }
    if let Some(to) = mouse.tab {
        switch(w, to);
        return;
    }
    if let Some(k) = mouse.nav {
        w.journal_on_list = false;
        w.sounds.push(Se::Cursor as u32);
        match t {
            Tab::Missions => {
                if let Some(&log) = missions::shown_logs(w).get(k) {
                    missions::show_log(w, log, None);
                }
            }
            Tab::Quests => {
                if let Some(a) = (0..11).filter(|&a| quests::area_on(w.expansions, a)).nth(k) {
                    quests::show_area(w, a, None);
                }
            }
            Tab::KeyItems => keyitems::show_category(w, k),
            Tab::Roe => roe_open(w, k),
        }
        return;
    }
    // A row under the pointer: hover moves the cursor there, a click too (and marks a new key item seen).
    let row = mouse.click.or(mouse.hover);
    let Some(k) = row else { return };
    let at = match t {
        Tab::Quests => p.m.journal_rows.borrow().iter().position(|&x| x == k),
        _ => Some(k),
    };
    let Some(at) = at else { return };
    if t == Tab::Roe {
        if roe_list.is_some() {
            if mouse.hover.is_some_and(|k| roe::set_cursor(w, k)) {
                w.sounds.push(Se::Cursor as u32);
            }
            if let Some(k) = mouse.click {
                roe::set_cursor(w, k);
                roe::modern_key(w, egui::Key::Enter, false);
            }
        }
        return;
    }
    let changed = match t {
        Tab::Missions => w.missions.as_mut().and_then(|x| x.list.as_mut()).filter(|l| !matches!(l.rows.get(at), Some(Row::CurHead | Row::DoneHead) | None)).map(|l| std::mem::replace(&mut l.cursor, at) != at),
        Tab::Quests => w.quests.as_mut().and_then(|x| x.list.as_mut()).map(|l| std::mem::replace(&mut l.cursor, at) != at),
        Tab::KeyItems => w.keyitems.as_mut().map(|x| std::mem::replace(&mut x.cursor, at) != at),
        Tab::Roe => None,
    };
    if changed == Some(true) {
        w.sounds.push(Se::Cursor as u32);
    }
    if mouse.click.is_some() {
        w.journal_on_list = true;
        if t == Tab::KeyItems {
            examine(w);
        }
    }
}
