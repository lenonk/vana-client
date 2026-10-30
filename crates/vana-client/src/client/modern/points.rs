//! The modern Merit Points, Job Points and Alter Ego Points over client/merits.rs, jobpoints.rs and aepoints.rs, which
//! keep every rule and key: the gauge and the points at the top, the windows' lists side by side with every row (level
//! / most and the next cost in retail's colours), the cursor row's description under them; merit3 and sortyn as
//! buttons, the raise asked by the modern confirm (merityn). Unity (client/unity.rs) in the same place.

use super::super::merits::{self, PointList, PointRow};
use super::super::game_ui::GameUi;
use super::super::{World, aepoints, jobpoints, unity};
use super::craft::{button, caption, tint};
use super::{MenuWin, Panels, Title, hud, social};
use crate::inventory::TINT_WHITE;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const ROW: f32 = 28.0;
const TALL: f32 = 12.0 * (ROW + 2.0);
/// A list with level and cost cells; a plain list (jobs, categories).
const LIST_W: f32 = 460.0;
const SIDE_W: f32 = 250.0;
const COL_W: f32 = 230.0;

/// What the mouse did on a list: the row clicked.
type Got = Option<usize>;

/// A list's rows: the name in its colour (a group header as its caption), the level and the NEXT cost at the right;
/// the cursor kept in view. The row clicked while it has the keys.
fn rows(ui: &mut Ui, m: &super::Modern, gu: &GameUi, salt: &str, l: &PointList, width: f32, cost_col: bool) -> Got {
    let ctx = ui.ctx().clone();
    let seen = egui::Id::unique((salt, "points cursor seen"));
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(l.cursor);
    ctx.data_mut(|d| d.insert_temp(seen, l.cursor));
    let next = gu.lobby_msg(0xc, 0x5a, "NEXT");
    let mut click = None;
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, r) in l.rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(width - 14.0, ROW), if l.focused { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == l.cursor && l.focused => "controls/list_row_selected",
                _ if k == l.cursor || l.focused && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            let y = rect.center().y;
            let level_x = if cost_col { rect.max.x - 120.0 } else { rect.max.x - 12.0 };
            let (name, font) = match r.header {
                Some(sprite) => (m.caption(gu, "menu    windowps", sprite).unwrap_or_default(), FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into()))),
                None => (r.name.clone(), FontId::proportional(m.theme.small)),
            };
            let clip = Rect::from_min_max(rect.min, pos2(level_x - 60.0, rect.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(rect.min.x + 24.0, y), Align2::LEFT_CENTER, name, font, tint(m, r.tint));
            if let Some((s, c)) = &r.level {
                ui.painter().text(pos2(level_x, y), Align2::RIGHT_CENTER, s, FontId::monospace(m.theme.small), tint(m, *c));
            }
            if r.mark {
                ui.painter().circle_filled(pos2(rect.max.x - 16.0, y), 4.5, m.theme.color("gold"));
            }
            if let Some((s, c)) = &r.cost {
                ui.painter().text(pos2(rect.max.x - 100.0, y), Align2::LEFT_CENTER, &next, FontId::proportional(m.theme.small), tint(m, *c));
                ui.painter().text(pos2(rect.max.x - 12.0, y), Align2::RIGHT_CENTER, s, FontId::monospace(m.theme.small), tint(m, *c));
            }
            if k == l.cursor && follow {
                ui.scroll_to_rect(rect, None);
            }
            if l.focused && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    click
}

/// The cursor row's name and description (retail's bluehelp).
fn help(ui: &mut Ui, m: &super::Modern, (name, desc): &(String, String)) {
    if name.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(name).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
    if !desc.is_empty() {
        super::gametext::label(ui, m, desc, m.theme.small, m.theme.color("text"));
    }
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(4.0);
}

/// A gauge across the panel: the label at the left, the value at the right.
fn gauge(ui: &mut Ui, m: &super::Modern, f: f32, label: &str, value: &str, full: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
    let bar = Rect::from_min_max(pos2(r.min.x, r.max.y - hud::BAR), r.max);
    let c = if full { m.theme.color("focus") } else { m.theme.color("text") };
    hud::labelled_bar(ui, m, "experience", bar, f.clamp(0.0, 1.0), (label, m.theme.color("text")), (value, c));
    ui.add_space(8.0);
}

/// A list's mouse: a click moves its cursor and is Enter.
fn act(w: &mut World, got: Got, set: impl Fn(&mut World, usize) -> bool, enter: impl Fn(&mut World)) {
    if let Some(k) = got {
        set(w, k);
        enter(w);
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    if w.merits_ui.is_some() {
        merit_points(p, w);
    } else if w.jp_ui.is_some() {
        job_points(p, w);
    } else if w.ae_ui.is_some() {
        alter_ego(p, w);
    } else if w.unity.is_some() {
        unity_panel(p, w);
    }
}

/// ut_menu as a column, quest00 beside it (Unity Information's lines and chat setting, or the two weeks), quest01's
/// ranking under its rows; the area list (row 3) is a panel of its own.
fn unity_panel(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = unity::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none() && w.arealist.is_none();
    let note = format!("{}/99999", v.accolades);
    let width = COL_W + if v.info.is_some() { 16.0 + LIST_W } else { 0.0 } + 44.0;
    let (mut menu, mut rows_got, mut ranks_got) = (None, None, None);
    p.panel("window", Some(Title { icon: Some("icons/status"), text: "Unity", note: Some(&note) }), width, |ui| {
        ui.horizontal_top(|ui| {
            let col = MenuWin { name: unity::MENU, sel: v.sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            menu = social::column(ui, m, gu, &col, free && v.info.is_none());
            let Some(i) = &v.info else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                heading(ui, m, &i.heading);
                for l in &i.waiting {
                    ui.label(egui::RichText::new(l).size(m.theme.small).color(m.theme.color("muted")));
                }
                for [a, b, c] in &i.lines {
                    let (r, _) = ui.allocate_exact_size(vec2(LIST_W - 14.0, 24.0), Sense::hover());
                    let y = r.center().y;
                    ui.painter().text(pos2(r.min.x + 4.0, y), Align2::LEFT_CENTER, a, FontId::proportional(m.theme.small), m.theme.color("muted"));
                    ui.painter().text(pos2(r.min.x + 200.0, y), Align2::LEFT_CENTER, b, FontId::proportional(m.theme.small), m.theme.color("text"));
                    ui.painter().text(pos2(r.max.x - 4.0, y), Align2::RIGHT_CENTER, c, FontId::monospace(m.theme.small), m.theme.color("text"));
                }
                if !i.chat_label.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&i.chat_label).size(m.theme.small).color(m.theme.color("text")));
                }
                ui.add_space(4.0);
                let two = PointList {
                    rows: i.rows.iter().enumerate().map(|(k, t)| PointRow { name: t.clone(), tint: TINT_WHITE, mark: i.mode == 1 && (k == 0) == i.chat, ..Default::default() }).collect(),
                    cursor: i.sel as usize - 1,
                    focused: free && i.ranks.is_none(),
                    help: Default::default(),
                };
                rows_got = rows(ui, m, gu, "unity rows", &two, LIST_W, false);
                let Some((title, order, cursor)) = &i.ranks else { return };
                ui.add_space(10.0);
                heading(ui, m, title);
                let list = PointList {
                    rows: order.iter().map(|[rk, name, pt]| PointRow { name: format!("{rk}   {name}"), tint: TINT_WHITE, level: Some((pt.clone(), TINT_WHITE)), ..Default::default() }).collect(),
                    cursor: *cursor,
                    focused: free,
                    help: Default::default(),
                };
                if list.rows.is_empty() {
                    for l in &i.waiting {
                        ui.label(egui::RichText::new(l).size(m.theme.small).color(m.theme.color("muted")));
                    }
                }
                ranks_got = rows(ui, m, gu, "unity ranks", &list, LIST_W, false);
            });
        });
    });
    if !free {
        return;
    }
    let enter = |w: &mut World, gu: &GameUi| unity::key(w, gu, egui::Key::Enter, false);
    social::act(w, gu, menu, unity::set_menu, enter);
    social::act(w, gu, rows_got.map(|k| k as u16 + 1), unity::set_info, enter);
    act(w, ranks_got, unity::set_rank, |_| {});
}

/// What the mouse did on the Merit Points windows.
#[derive(Default)]
struct MeritMouse {
    menu: Option<u16>,
    mode: Option<u16>,
    list: Got,
    edit: Option<u16>,
    yes: Option<bool>,
}

/// merit1 (and merit2 while it is open) as columns, meritcat beside them, merit3 and sortyn under its description.
fn merit_points(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = merits::look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let note = v.points.map(|(n, max)| format!("{n}/{max} P"));
    let cols = 1.0 + v.mode.is_some() as u8 as f32;
    let width = cols * (COL_W + 16.0) + if v.list.is_some() { LIST_W } else { 0.0 } + 44.0;
    let mut mouse = MeritMouse::default();
    p.panel("window", Some(Title { icon: Some("icons/status"), text: "Merit Points", note: note.as_deref() }), width, |ui| {
        let limit = v.limit.unwrap_or(0);
        let value = v.limit.map_or("----/10000".to_string(), |l| format!("{l}/10000"));
        gauge(ui, m, limit as f32 / 10000.0, "Limit Points", &value, false);
        ui.horizontal_top(|ui| {
            let top = v.mode.is_none() && v.list.is_none() && v.edit.is_none();
            let menu = MenuWin { name: merits::MENU, sel: v.menu.0, off: v.menu.1.clone(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            mouse.menu = social::column(ui, m, gu, &menu, free && top);
            if let Some((sel, off)) = v.mode {
                ui.add_space(16.0);
                let mode = MenuWin { name: merits::MODE, sel, off: vec![off], labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                mouse.mode = social::column(ui, m, gu, &mode, free && v.list.is_none());
            }
            let Some(l) = &v.list else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                heading(ui, m, if v.cat == 0 { "Categories" } else { "Merits" });
                mouse.list = rows(ui, m, gu, "merit rows", l, LIST_W, v.cat != 0);
                help(ui, m, &l.help);
                let Some((sel, sortyn, raise, lower)) = v.edit else { return };
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (k, d, ok) in [(1u16, "Raise", raise), (2, "Lower", lower)] {
                        if button(ui, m, &caption(gu, merits::EDIT, k, d), 130.0, sel == k, !ok) && free && sortyn.is_none() {
                            mouse.edit = Some(k);
                        }
                    }
                });
                if let Some(yes) = sortyn {
                    ui.add_space(6.0);
                    let q = gu.menu_msg(if sel == 1 { 0x264 } else { 0x265 }, "");
                    if !q.is_empty() {
                        ui.label(egui::RichText::new(q).size(m.theme.small).color(m.theme.color("gold")));
                    }
                    ui.horizontal(|ui| {
                        for (k, d) in [(1u16, "Yes"), (2, "No")] {
                            if button(ui, m, &caption(gu, merits::SORTYN, k, d), 130.0, yes == (k == 1), false) && free {
                                mouse.yes = Some(k == 1);
                            }
                        }
                    });
                }
            });
        });
    });
    if !free {
        return;
    }
    let enter = |w: &mut World| merits::key(w, gu, egui::Key::Enter);
    if let Some(yes) = mouse.yes {
        merits::set_sortyn(w, yes);
        enter(w);
        return;
    }
    if let Some(k) = mouse.edit {
        merits::set_edit(w, k);
        enter(w);
        return;
    }
    social::act(w, gu, mouse.menu, merits::set_menu, |w, gu| merits::key(w, gu, egui::Key::Enter));
    social::act(w, gu, mouse.mode, merits::set_mode, |w, gu| merits::key(w, gu, egui::Key::Enter));
    act(w, mouse.list, merits::set_row, enter);
}

/// merit2ca (the jobs) beside jbpcat (its two pages as tabs).
fn job_points(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = jobpoints::look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let note = v.points.map(|[_, cur, _]| format!("{cur}/500 P"));
    let width = SIDE_W + if v.page.is_some() { 16.0 + LIST_W } else { 0.0 } + 44.0;
    let (mut jobs, mut page, mut tab) = (None, None, None);
    p.panel("window", Some(Title { icon: Some("icons/status"), text: "Job Points", note: note.as_deref() }), width, |ui| {
        let cap = v.points.map_or(0, |q| q[0]);
        let value = v.points.map_or(String::new(), |q| format!("{}/30000", q[0]));
        gauge(ui, m, cap as f32 / 30000.0, "Capacity Points", &value, false);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(SIDE_W);
                jobs = rows(ui, m, gu, "jp jobs", &v.jobs, SIDE_W, false);
                if v.page.is_none() {
                    help(ui, m, &v.jobs.help);
                }
            });
            let Some((pg, l)) = &v.page else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                let tabs = super::tabs::strip(ui, m, "jp tabs", &["Job Points", "Gifts"], Some(*pg), None);
                if let Some(k) = tabs.iter().position(|r| r.clicked())
                    && free
                {
                    tab = Some(k);
                }
                ui.add_space(6.0);
                // The gifts page's header (windowps 723, "Gifts Combo Total:") with the points spent.
                if *pg == 1 {
                    let head = m.caption(gu, "menu    windowps", 723).unwrap_or_else(|| "Combo Total:".into());
                    let spent = v.points.map_or(0, |q| q[2]);
                    heading(ui, m, &format!("{head} {spent}"));
                }
                page = rows(ui, m, gu, "jp page", l, LIST_W, *pg == 0);
                help(ui, m, &l.help);
            });
        });
    });
    if !free {
        return;
    }
    if let Some(k) = tab {
        jobpoints::set_page(w, k);
        return;
    }
    let enter = |w: &mut World| jobpoints::key(w, egui::Key::Enter, false);
    act(w, jobs, jobpoints::set_job, enter);
    act(w, page, jobpoints::set_row, enter);
}

/// fp_cat (the categories) beside fp_cat2 (the category's attributes).
fn alter_ego(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = aepoints::look(w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let note = v.points.map(|n| format!("{n}/1000 P"));
    let width = SIDE_W + if v.attrs.is_some() { 16.0 + LIST_W } else { 0.0 } + 44.0;
    let (mut cats, mut attrs) = (None, None);
    p.panel("window", Some(Title { icon: Some("icons/status"), text: "Alter Ego Points", note: note.as_deref() }), width, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(SIDE_W);
                cats = rows(ui, m, gu, "ae cats", &v.cats, SIDE_W, false);
                if v.attrs.is_none() {
                    help(ui, m, &v.cats.help);
                }
            });
            let Some(l) = &v.attrs else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                attrs = rows(ui, m, gu, "ae attrs", l, LIST_W, true);
                help(ui, m, &l.help);
            });
        });
    });
    if !free {
        return;
    }
    let enter = |w: &mut World| aepoints::key(w, egui::Key::Enter, false);
    act(w, cats, aepoints::set_cat, enter);
    act(w, attrs, aepoints::set_row, enter);
}
