//! The modern Social pages in the command window: the Party menu, Search and Linkshell, drawn from retail's menus
//! (their rows named by their own label sprites, retail's disabled rows and swaps) over client/partymenu.rs,
//! searchmenu.rs and linkshell.rs, which keep every rule and key. A window opened over another shows beside it.

use super::super::game_ui::GameUi;
use super::super::{World, arealist, bazaar, helpdesk, linklist, linkshell, moogle, partymenu, region, search, searchmenu, viewhouse};
use super::{MenuWin, Panels, Title, paint};
use crate::inventory::ItemInfo;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const COL_W: f32 = 230.0;
const ROW: f32 = 30.0;
const LS_W: f32 = 420.0;
const AREA_W: f32 = 300.0;

/// A window's selectable rows top to bottom: (child, its name).
fn rows(m: &super::Modern, gu: &GameUi, v: &MenuWin) -> Vec<(u16, String)> {
    let Some(layout) = gu.layout(v.name) else { return Vec::new() };
    let mut els: Vec<_> = layout.elements.iter().skip(1).filter(|e| e.links[2] != 0xff).collect();
    els.sort_by_key(|e| (e.y, e.x));
    els.iter()
        .map(|e| {
            let label = v.labels.iter().find(|l| l.0 == e.id).and_then(|l| m.caption(gu, "menu    windowps", l.1));
            let name = label.or_else(|| gu.child_caption(v.name, e.id)).or_else(|| gu.focus_text(v.name, e.id)[1].clone()).unwrap_or_default();
            (e.id, name)
        })
        .collect()
}

/// A window's title: its frame's (the help title of its first row), else none.
fn title(gu: &GameUi, v: &MenuWin) -> String {
    gu.focus_text(v.name, 0)[1].clone().unwrap_or_default()
}

/// One window's rows as a column; the focused one takes hover and clicks: (row hovered, row clicked).
fn column(ui: &mut Ui, m: &super::Modern, gu: &GameUi, v: &MenuWin, focused: bool, moved: bool) -> (Option<u16>, Option<u16>) {
    let (mut hover, mut click) = (None, None);
    ui.vertical(|ui| {
        ui.set_width(COL_W);
        let t = title(gu, v);
        // The panel already carries the page's name; a window's own title shows when it adds something.
        if !t.is_empty() && !matches!(t.as_str(), "Party" | "Search" | "Linkshell" | "Set Bazaar" | "Region Info" | "Synthesis" | "MacroPalette") {
            ui.label(egui::RichText::new(t).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
            ui.add_space(4.0);
        }
        let rows = rows(m, gu, v);
        // The keys walk the column as drawn (GameUi::menu_links).
        gu.modern_order.borrow_mut().insert(v.name.to_string(), rows.iter().map(|r| r.0).collect());
        for (id, name) in rows {
            let (r, resp) = ui.allocate_exact_size(vec2(COL_W, ROW), if focused { Sense::click() } else { Sense::hover() });
            let off = v.off.contains(&id);
            let face = match () {
                _ if id == v.sel && focused => "controls/list_row_selected",
                _ if id == v.sel => "controls/list_row_hover",
                _ if off => "controls/list_row_disabled",
                _ if focused && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            let c = if off { m.theme.color("muted") } else { m.theme.color("text") };
            let clip = Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(m.theme.small), c);
            if let Some((_, val)) = v.values.iter().find(|x| x.0 == id) {
                let vc = if v.marked.contains(&id) { m.theme.color("gold") } else { m.theme.color("text") };
                ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, val, FontId::monospace(m.theme.small), vc);
            } else if v.marked.contains(&id) {
                ui.painter().circle_filled(pos2(r.max.x - 16.0, r.center().y), 4.5, m.theme.color("gold"));
            }
            if focused && resp.hovered() && moved {
                hover = Some(id);
            }
            if focused && resp.clicked() {
                click = Some(id);
            }
            ui.add_space(2.0);
        }
    });
    (hover, click)
}

/// The stack's windows side by side (the last three), the top one focused when `focused`.
pub(super) fn stack(ui: &mut Ui, m: &super::Modern, gu: &GameUi, wins: &[MenuWin], focused: bool, moved: bool) -> (Option<u16>, Option<u16>) {
    let mut out = (None, None);
    ui.horizontal_top(|ui| {
        let n = wins.len();
        for (k, v) in wins.iter().enumerate().skip(n.saturating_sub(3)) {
            if k + 3 < n {
                continue;
            }
            let top = k + 1 == n;
            let r = column(ui, m, gu, v, top && focused, moved);
            if top {
                out = r;
            }
            if !top {
                ui.add_space(12.0);
            }
        }
    });
    out
}

/// The area picker (arealist.rs keeps its rules and keys): every row, not retail's five, the cursor's kept in view; its
/// title over them. (row hovered, row clicked).
fn area_column(ui: &mut Ui, m: &super::Modern, gu: &GameUi, a: &arealist::AreaList, focused: bool, moved: bool) -> (Option<usize>, Option<usize>) {
    let (mut hover, mut click) = (None, None);
    ui.vertical(|ui| {
        ui.set_width(AREA_W);
        let t = gu.focus_text(arealist::WINDOW, 0)[1].clone().unwrap_or_else(|| "Area".into());
        ui.label(egui::RichText::new(t).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
        ui.add_space(4.0);
        let seen = egui::Id::unique("area cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(a.cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, a.cursor));
        let tall = 12.0 * (ROW + 2.0);
        egui::ScrollArea::vertical().id_salt("area rows").max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
            for (k, r) in a.rows.iter().enumerate() {
                let (rr, resp) = ui.allocate_exact_size(vec2(AREA_W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == a.cursor && focused => "controls/list_row_selected",
                    _ if k == a.cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, rr, (rr.height() / s.size.y).min(1.0), Color32::WHITE));
                }
                ui.painter().text(pos2(rr.min.x + 24.0, rr.center().y), Align2::LEFT_CENTER, &r.name, FontId::proportional(m.theme.small), m.theme.color("text"));
                let cell = match &r.cell {
                    arealist::Cell::Paren(s) => format!("({s})"),
                    arealist::Cell::Count(n) => format!("[{n}]"),
                    arealist::Cell::Level(s) => s.clone(),
                };
                ui.painter().text(pos2(rr.max.x - 12.0, rr.center().y), Align2::RIGHT_CENTER, cell, FontId::proportional(m.theme.small), m.theme.color("muted"));
                if k == a.cursor && follow {
                    ui.scroll_to_rect(rr, None);
                }
                if focused && resp.hovered() && moved {
                    hover = Some(k);
                }
                if focused && resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
        });
    });
    (hover, click)
}

const RACES: [&str; 9] = ["", "Hume", "Hume", "Elvaan", "Elvaan", "Tarutaru", "Tarutaru", "Mithra", "Galka"];
const NATIONS: [&str; 3] = ["San d'Oria", "Bastok", "Windurst"];
const RES_ROWS: f32 = 15.0;
const RES_ROW: f32 = 26.0;
const RES_W: f32 = 660.0;

/// A person in a people list: an icon, the name in its colour, the jobs, race / nation / rank, the area when not ours.
struct Person {
    icon: Option<egui::TextureId>,
    name: String,
    colour: Color32,
    jobs: String,
    who: String,
    zone: String,
}

/// A search record as a row (FUN_1020a740's cells in words): `icon` the row's own (the search's status icon, the link
/// list's shell item), `alt` the other job view (Num+).
fn person(w: &World, gu: &GameUi, r: &crate::net::search::Record, icon: Option<egui::TextureId>, alt: bool) -> Person {
    let (_, team) = search::status_icon(r.flags);
    let nc = match () {
        _ if team.is_some() => team.map_or(0x8080_8080, |t| gu.ncol(t.0)),
        _ if r.flags & 8 != 0 => gu.ncol(5),
        _ if r.flags & 0x2000 != 0 => gu.ncol(1),
        _ => 0x8080_8080,
    };
    let job = |j: u8, lv: u8| super::super::items::JOBS.get(j as usize).filter(|s| !s.is_empty()).map(|s| format!("{s}{lv}"));
    let jobs = if r.present & 1 << 6 == 0 || r.flags & 0x4000 != 0 {
        "--".to_string()
    } else if alt {
        let ml = if r.flags & 0x4000_0000 != 0 { format!(" ML{}", r.master_level) } else { String::new() };
        format!("{}{ml}", job(r.job.0, r.level.0).unwrap_or_default())
    } else {
        [job(r.job.0, r.level.0), job(r.job.1, r.level.1)].into_iter().flatten().collect::<Vec<_>>().join("/")
    };
    let race = RACES.get(r.race as usize).filter(|_| r.job.0 != 0x17 && r.job.1 != 0x17).copied().unwrap_or("");
    let rank = format!("Rank {}", r.rank.max(1));
    let who = [race, NATIONS.get(r.nation as usize).copied().unwrap_or(""), &rank].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("  ");
    let zone = if r.zone != w.pos_zone { crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.area_name(r.zone))) } else { String::new() };
    Person { icon, name: r.name.clone(), colour: super::chat::colour(crate::dat::battle::argb_bytes(nc)), jobs, who, zone }
}

/// What a people list's mouse did: a row hovered or clicked (with a header, 0 is the header), an option hovered or
/// clicked, a right click.
#[derive(Default)]
struct PeopleMouse {
    hover: Option<usize>,
    click: Option<usize>,
    opt_hover: Option<u16>,
    opt_click: Option<u16>,
    back: bool,
}

/// What a people list shows: its panel, title and icon, a header row (its text and a note at the right), a line when
/// empty, the rows, the cursor (header counted), whether it has the keys, and the options beside the cursor's row.
struct People<'a> {
    id: &'a str,
    title: &'a str,
    icon: &'a str,
    header: Option<(String, &'a str)>,
    empty: Option<String>,
    rows: Vec<Person>,
    cursor: usize,
    focused: bool,
    opts: Vec<(u16, String, bool)>,
    opt_sel: Option<u16>,
}

/// A people list beside the window panel (in its place when none is open): the search results, the link list.
fn people(p: &mut Panels, v: People, moved: bool) -> PeopleMouse {
    let m = p.m;
    let screen = p.ctx.content_rect();
    let at = m.rects.borrow().get("window").map_or(pos2(screen.width() * 0.0047, screen.height() * 0.0347), |r| pos2(r.max.x + 8.0, r.min.y));
    let mut mouse = PeopleMouse::default();
    let width = RES_W + if v.opts.is_empty() { 0.0 } else { COL_W + 12.0 } + 44.0;
    let first = v.header.is_some() as usize;
    p.panel_at(v.id, Some(Title { icon: Some(v.icon), text: v.title, note: None }), width, at, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(RES_W);
                if let Some(e) = &v.empty {
                    ui.label(egui::RichText::new(e).size(m.theme.small).color(m.theme.color("muted")));
                }
                let seen = egui::Id::unique(("people cursor seen", v.id));
                let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(v.cursor);
                ui.ctx().data_mut(|d| d.insert_temp(seen, v.cursor));
                let tall = RES_ROWS * (RES_ROW + 2.0);
                egui::ScrollArea::vertical().id_salt(("people rows", v.id)).max_height(tall).min_scrolled_height(tall.min((v.rows.len() + first) as f32 * (RES_ROW + 2.0))).auto_shrink([false, true]).show(ui, |ui| {
                    for k in 0..v.rows.len() + first {
                        let (r, resp) = ui.allocate_exact_size(vec2(RES_W - 14.0, RES_ROW), if v.focused { Sense::click() } else { Sense::hover() });
                        let face = match () {
                            _ if k == v.cursor && v.focused => "controls/list_row_selected",
                            _ if k == v.cursor || v.focused && resp.hovered() => "controls/list_row_hover",
                            _ => "controls/list_row_normal",
                        };
                        if let Some(s) = m.art.sprite(face) {
                            ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                        }
                        let font = FontId::proportional(m.theme.small);
                        let y = r.center().y;
                        match (&v.header, k.checked_sub(first)) {
                            (Some((text, note)), None) => {
                                ui.painter().text(pos2(r.min.x + 32.0, y), Align2::LEFT_CENTER, text, FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
                                ui.painter().text(pos2(r.max.x - 12.0, y), Align2::RIGHT_CENTER, *note, font, m.theme.color("muted"));
                            }
                            (_, Some(i)) => {
                                let pr = &v.rows[i];
                                if let Some(t) = pr.icon {
                                    ui.painter().image(t, Rect::from_center_size(pos2(r.min.x + 16.0, y), vec2(18.0, 18.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                }
                                ui.painter().text(pos2(r.min.x + 32.0, y), Align2::LEFT_CENTER, &pr.name, font.clone(), pr.colour);
                                ui.painter().text(pos2(r.min.x + 190.0, y), Align2::LEFT_CENTER, &pr.jobs, FontId::monospace(m.theme.small), m.theme.color("text"));
                                ui.painter().text(pos2(r.min.x + 330.0, y), Align2::LEFT_CENTER, &pr.who, font.clone(), m.theme.color("muted"));
                                ui.painter().text(pos2(r.max.x - 12.0, y), Align2::RIGHT_CENTER, &pr.zone, font, m.theme.color("muted"));
                            }
                            _ => {}
                        }
                        if k == v.cursor && follow {
                            ui.scroll_to_rect(r, None);
                        }
                        if v.focused && resp.hovered() && moved {
                            mouse.hover = Some(k);
                        }
                        if v.focused && resp.clicked() {
                            mouse.click = Some(k);
                        }
                        if v.focused && resp.secondary_clicked() {
                            mouse.back = true;
                        }
                        ui.add_space(2.0);
                    }
                });
            });
            if !v.opts.is_empty() {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    ui.add_space(RES_ROW * (v.cursor as f32).min(RES_ROWS - v.opts.len() as f32).max(0.0));
                    for (c, name, on) in &v.opts {
                        let (r, resp) = ui.allocate_exact_size(vec2(COL_W, ROW), Sense::click());
                        let face = match () {
                            _ if v.opt_sel == Some(*c) => "controls/list_row_selected",
                            _ if !on => "controls/list_row_disabled",
                            _ if resp.hovered() => "controls/list_row_hover",
                            _ => "controls/list_row_normal",
                        };
                        if let Some(s) = m.art.sprite(face) {
                            ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                        }
                        let tc = if *on { m.theme.color("text") } else { m.theme.color("muted") };
                        ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), tc);
                        if resp.hovered() && moved {
                            mouse.opt_hover = Some(*c);
                        }
                        if resp.clicked() {
                            mouse.opt_click = Some(*c);
                        }
                        if resp.secondary_clicked() {
                            mouse.back = true;
                        }
                        ui.add_space(2.0);
                    }
                });
            }
        });
    });
    mouse
}

/// The search results (search.rs keeps the rules and keys): the header with the count (Enter on it is Refine), every
/// person (status icon, name in its colour, jobs, race, nation and rank, the area when not ours), the person's options
/// beside them.
fn results(p: &mut Panels, w: &mut World, gu: &GameUi, moved: bool) {
    let Some(res) = w.search.as_ref() else { return };
    let m = p.m;
    let focused = res.option.is_none() && !searchmenu::refining(w) && w.arealist.is_none() && w.seacom_edit.is_none() && w.confirm.is_none();
    let bt = w.battle.clone();
    let header = if res.failed {
        bt.as_ref().and_then(|b| b.group7(0x65)).unwrap_or_default()
    } else if res.busy {
        "Searching...".to_string()
    } else {
        format!("{} found", res.total)
    };
    let rows = res
        .rows
        .iter()
        .map(|r| {
            let (k, _) = search::status_icon(r.flags);
            let icon = search::icon(k).filter(|_| k > 1).and_then(|(lib, s)| m.sprite_icon(p.ctx, gu, lib, s));
            person(w, gu, r, icon, res.alt)
        })
        .collect();
    let opts: Vec<(u16, String, bool)> = res.option.map_or(Vec::new(), |_| {
        let r = res.cursor.checked_sub(1).and_then(|i| res.rows.get(i));
        let req = search::request_mode(w, res);
        search::OPTION_ORDER
            .iter()
            .map(|&c| {
                let swap = match c {
                    4 if req => m.caption(gu, "menu    windowps", 785),
                    5 if req => m.caption(gu, "menu    windowps", 786),
                    _ => None,
                };
                let name = swap.or_else(|| gu.child_caption(search::SCOPTION, c)).or_else(|| gu.focus_text(search::SCOPTION, c)[1].clone()).unwrap_or_default();
                (c, name, r.is_some_and(|r| search::option_enabled(w, c, r)))
            })
            .collect()
    });
    let title = gu.title_msg(res.title).unwrap_or_else(|| "Search".into());
    let note = if res.mode == 3 { "" } else { "Refine" };
    let v = People { id: "results", title: &title, icon: "icons/find", header: Some((header, note)), empty: None, rows, cursor: res.cursor, focused, opts, opt_sel: res.option };
    let mouse = people(p, v, moved);
    for k in mouse.hover.into_iter().chain(mouse.click) {
        if let Some(r) = w.search.as_mut()
            && r.cursor != k
        {
            r.cursor = k;
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    for c in mouse.opt_hover.into_iter().chain(mouse.opt_click) {
        if let Some(r) = w.search.as_mut()
            && r.option.is_some()
            && r.option != Some(c)
        {
            r.option = Some(c);
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    if mouse.click.is_some() || mouse.opt_click.is_some() {
        search::key(w, egui::Key::Enter);
    } else if mouse.back {
        search::key(w, egui::Key::Escape);
    }
}

/// A linkshell's member list (linklist.rs keeps the rules and keys): each member with the shell item of their rank,
/// the member window's rows beside the cursor's.
fn link_list(p: &mut Panels, w: &mut World, gu: &GameUi, moved: bool) {
    let Some(v) = linklist::view(w, gu) else { return };
    let m = p.m;
    let info = w.item_info.clone();
    let rows = v.rows.iter().map(|(r, item)| person(w, gu, r, info.as_ref().and_then(|i| m.item_icon(p.ctx, i, *item)), v.alt)).collect();
    let empty = v.empty.clone();
    let title = gu.title_msg(0x47).unwrap_or_else(|| "Linkshell".into());
    let pv = People { id: "linklist", title: &title, icon: "icons/linkshell", header: None, empty, rows, cursor: v.cursor, focused: v.focused, opts: v.acts.clone(), opt_sel: v.act_sel };
    let mouse = people(p, pv, moved);
    for k in mouse.hover.into_iter().chain(mouse.click) {
        if linklist::set_cursor(w, k) {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    for c in mouse.opt_hover.into_iter().chain(mouse.opt_click) {
        if linklist::set_act(w, c) {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    if mouse.click.is_some() || mouse.opt_click.is_some() {
        linklist::key(w, gu, egui::Key::Enter);
    } else if mouse.back {
        linklist::key(w, gu, egui::Key::Escape);
    }
}

/// A Help Desk page: its text and rows wrapped to the panel, a question's value under its row (the field while typing),
/// the cursor's row kept in view. (row hovered, row clicked).
fn help_page(p: &mut Panels, v: &helpdesk::PageView, moved: bool) -> (Option<i32>, Option<i32>) {
    const W: f32 = 440.0;
    let m = p.m;
    let (mut hover, mut click) = (None, None);
    let t = p.ctx.input(|i| i.time);
    p.panel("window", Some(Title { icon: Some("icons/help"), text: &v.title, note: None }), W + 44.0, |ui| {
        let seen = egui::Id::unique("help page cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<i32>(seen)) != Some(v.cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, v.cursor));
        let tall = ui.ctx().content_rect().height() * 0.6;
        egui::ScrollArea::vertical().id_salt("help page").max_height(tall).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(W);
            for line in &v.lines {
                match line {
                    helpdesk::Line::Text(text, grey) => {
                        let c = if *grey { m.theme.color("muted") } else { m.theme.color("text") };
                        ui.label(egui::RichText::new(text).size(m.theme.small).color(c));
                        ui.add_space(6.0);
                    }
                    helpdesk::Line::Row { id, text, on, value } => {
                        let font = FontId::proportional(m.theme.small);
                        let tc = if *on { m.theme.color("text") } else { m.theme.color("muted") };
                        let galley = ui.fonts_mut(|f| f.layout(text.clone(), font.clone(), tc, W - 40.0));
                        let h = galley.size().y.max(ROW - 10.0) + 10.0;
                        let (r, resp) = ui.allocate_exact_size(vec2(W, h), if v.focused { Sense::click() } else { Sense::hover() });
                        let current = *id == v.cursor;
                        let face = match () {
                            _ if current && v.focused => "controls/list_row_selected",
                            _ if current || v.focused && resp.hovered() => "controls/list_row_hover",
                            _ if !on => "controls/list_row_disabled",
                            _ => "controls/list_row_normal",
                        };
                        if let Some(s) = m.art.sprite(face) {
                            ui.painter().add(paint::nine_slice(&s, r, (ROW / s.size.y).min(1.0), Color32::WHITE));
                        }
                        ui.painter().galley(pos2(r.min.x + 24.0, r.center().y - galley.size().y / 2.0), galley, tc);
                        if let Some(val) = value {
                            ui.add_space(2.0);
                            let (fr, _) = ui.allocate_exact_size(vec2(W - 24.0, 24.0), Sense::hover());
                            let fr = fr.translate(vec2(24.0, 0.0));
                            let editing = current && v.field.is_some();
                            ui.painter().rect_filled(fr, 5.0, m.theme.color("charcoal"));
                            ui.painter().rect_stroke(fr, 5.0, egui::Stroke::new(1.0, if editing { m.theme.color("focus") } else { m.theme.color("bronze").gamma_multiply(0.6) }), egui::StrokeKind::Inside);
                            let shown = if editing { v.field.clone().unwrap_or_default() } else { val.clone() };
                            let tr = ui.painter().with_clip_rect(fr.shrink(3.0)).text(pos2(fr.min.x + 8.0, fr.center().y), Align2::LEFT_CENTER, &shown, font.clone(), m.theme.color("text"));
                            if editing && (t * 2.0) as i64 % 2 == 0 {
                                let x = if shown.is_empty() { fr.min.x + 8.0 } else { tr.max.x + 1.0 };
                                ui.painter().vline(x, (fr.center().y - 7.0)..=(fr.center().y + 7.0), egui::Stroke::new(1.5, m.theme.color("text")));
                            }
                        }
                        if current && follow {
                            ui.scroll_to_rect(r, None);
                        }
                        if v.focused && resp.hovered() && moved {
                            hover = Some(*id);
                        }
                        if v.focused && resp.clicked() {
                            click = Some(*id);
                        }
                        ui.add_space(2.0);
                    }
                }
            }
        });
    });
    (hover, click)
}

/// The picker's mouse: hover moves the cursor (SE 0001), a click moves it and is Enter.
fn area_act(w: &mut World, gu: &GameUi, (hover, click): (Option<usize>, Option<usize>)) {
    for k in hover.into_iter().chain(click) {
        if let Some(a) = w.arealist.as_mut()
            && a.cursor != k
        {
            a.cursor = k;
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    }
    if click.is_some() {
        arealist::key(w, gu, egui::Key::Enter);
    }
}

/// A stack's panel width: its columns (three at most) and the margins.
pub(super) fn width(n: usize) -> f32 {
    let cols = n.clamp(1, 3) as f32;
    cols * COL_W + (cols - 1.0) * 12.0 + 44.0
}

/// What a page's mouse does to its top window: hover moves the cursor (SE 0001), a click moves it and is Enter.
pub(super) fn act(w: &mut World, gu: &GameUi, (hover, click): (Option<u16>, Option<u16>), set: impl Fn(&mut World, u16) -> bool, enter: impl Fn(&mut World, &GameUi)) {
    if let Some(c) = hover
        && set(w, c)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(c) = click {
        set(w, c);
        enter(w, gu);
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    // The area picker opened by Unity or an event: beside the window, else in its place (Search shows it in its own).
    if let Some(a) = w.arealist.clone().filter(|_| w.searchmenu.is_none()) {
        let screen = p.ctx.content_rect();
        let at = m.rects.borrow().get("window").map_or(pos2(screen.width() * 0.0047, screen.height() * 0.0347), |r| pos2(r.max.x + 8.0, r.min.y));
        let mut got = (None, None);
        p.panel_at("arealist", None, AREA_W + 44.0, at, |ui| got = area_column(ui, m, gu, &a, w.confirm.is_none(), moved));
        area_act(w, gu, got);
    }
    results(p, w, gu, moved);
    link_list(p, w, gu, moved);
    // View House is the storage view on its first bag (its bags as the view's tabs).
    if w.viewhouse.is_some() && w.win.is_none() {
        viewhouse::open_row(w, 1);
        return;
    }
    // Help Desk: its menu and Config pages, and the GM call's questionnaire over them.
    if let Some(v) = helpdesk::view(w, gu) {
        let (hover, click) = help_page(p, &v, moved);
        for id in hover.into_iter().chain(click) {
            if helpdesk::set_cursor(w, id) {
                w.sounds.push(crate::audio::Ui::Cursor as u32);
            }
        }
        if click.is_some() {
            helpdesk::key(w, egui::Key::Enter);
        }
        if w.helpdesk.is_some() {
            return;
        }
    }
    // The moogle's menus (the Mog House and the Nomad Moogle), the one over another beside it.
    let wins = moogle::views(w, gu);
    if !wins.is_empty() {
        let focused = !super::super::post::active(w) && w.confirm.is_none();
        let mut got = (None, None);
        p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: "Moogle", note: None }), width(wins.len()), |ui| {
            got = stack(ui, m, gu, &wins, focused, moved);
        });
        act(w, gu, got, moogle::set_cursor, |w, gu| {
            moogle::key(w, gu, egui::Key::Enter);
        });
        return;
    }
    // Region Info's menu (its maps are retail's, as the map is).
    if let Some(sel) = region::shown(w) {
        let v = MenuWin { name: region::MENU, sel, off: Vec::new(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
        let focused = w.confirm.is_none();
        let mut got = (None, None);
        p.panel("window", Some(Title { icon: Some("icons/find"), text: "Region Info", note: None }), width(1), |ui| {
            got = stack(ui, m, gu, std::slice::from_ref(&v), focused, moved);
        });
        let set = |w: &mut World, c: u16| match w.region_menu.as_mut() {
            Some(r) if r.sel != c => {
                r.sel = c;
                true
            }
            _ => false,
        };
        let (sw, sh) = w.ui_wh;
        act(w, gu, got, set, |w, gu| region::menu_key(w, gu, egui::Key::Enter, sw, sh));
        return;
    }
    // Set Bazaar.
    if let Some(v) = bazaar::view(w) {
        let focused = bazaar::focused(w);
        let mut got = (None, None);
        p.panel("window", Some(Title { icon: Some("icons/currency"), text: "Set Bazaar", note: None }), width(1), |ui| {
            got = stack(ui, m, gu, std::slice::from_ref(&v), focused, moved);
        });
        let set = |w: &mut World, c: u16| match w.bazaar.as_mut() {
            Some(b) if b.sel != c => {
                b.sel = c;
                true
            }
            _ => false,
        };
        act(w, gu, got, set, |w, _| bazaar::key(w, egui::Key::Enter));
        return;
    }
    // The party menu.
    let wins = partymenu::views(w);
    if !wins.is_empty() {
        let focused = partymenu::focused(w);
        let mut got = (None, None);
        p.panel("window", Some(Title { icon: Some("icons/party"), text: "Party", note: None }), width(wins.len()), |ui| {
            got = stack(ui, m, gu, &wins, focused, moved);
        });
        let set = |w: &mut World, c: u16| match w.partymenu.as_mut().and_then(|x| x.stack.last_mut()) {
            Some(t) if t.sel != c => {
                t.sel = c;
                true
            }
            _ => false,
        };
        act(w, gu, got, set, |w, gu| partymenu::key(w, gu, egui::Key::Enter));
        return;
    }
    // Search.
    let wins = searchmenu::views(w);
    if !wins.is_empty() {
        let focused = searchmenu::focused(w);
        let area = w.arealist.clone();
        let mut got = (None, None);
        let mut area_got = (None, None);
        let extra = if area.is_some() { AREA_W + 12.0 } else { 0.0 };
        p.panel("window", Some(Title { icon: Some("icons/find"), text: "Search", note: None }), width(wins.len()) + extra, |ui| {
            ui.horizontal_top(|ui| {
                got = stack(ui, m, gu, &wins, focused, moved);
                if let Some(a) = &area {
                    ui.add_space(12.0);
                    area_got = area_column(ui, m, gu, a, w.confirm.is_none(), moved);
                }
            });
        });
        area_act(w, gu, area_got);
        let set = |w: &mut World, c: u16| match w.searchmenu.as_mut().and_then(|x| x.stack.last_mut()) {
            Some(t) if t.sel != c => {
                t.sel = c;
                true
            }
            _ => false,
        };
        act(w, gu, got, set, |w, gu| searchmenu::key(w, gu, egui::Key::Enter));
        return;
    }
    // Linkshell: the shells in the bags, the cursor's shell, its submenu beside them.
    if let Some((rows, cursor, sub)) = linkshell::modern_view(w) {
        let focused = linkshell::focused(w);
        let info = w.item_info.clone();
        let icons: Vec<Option<egui::TextureId>> = rows.iter().map(|r| info.as_ref().and_then(|i| m.item_icon(p.ctx, i, r.0.item))).collect();
        let mut row_click: Option<usize> = None;
        let mut row_hover: Option<usize> = None;
        let mut got = (None, None);
        p.panel("window", Some(Title { icon: Some("icons/linkshell"), text: "Linkshell", note: None }), LS_W + if sub.is_some() { COL_W + 14.0 } else { 0.0 } + 44.0, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LS_W);
                    match rows.get(cursor) {
                        Some((r, name, t)) => {
                            ui.horizontal(|ui| {
                                if let Some(tex) = icons.get(cursor).copied().flatten() {
                                    ui.add(egui::Image::new((tex, vec2(32.0, 32.0))).tint(super::chat::colour(t.to_le_bytes())));
                                }
                                let shown = if name.is_empty() { info.as_ref().and_then(|c| c.name(r.item)).unwrap_or_default() } else { name.clone() };
                                ui.label(egui::RichText::new(shown).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("text")));
                            });
                        }
                        None => {
                            let s = w.battle.as_ref().and_then(|b| b.group7(0x71)).unwrap_or_default();
                            ui.label(egui::RichText::new(s).size(m.theme.small).color(m.theme.color("hp")));
                        }
                    }
                    ui.add_space(8.0);
                    if rows.is_empty() {
                        let s = w.battle.as_ref().and_then(|b| b.group7(0x68)).unwrap_or_default();
                        ui.label(egui::RichText::new(s).size(m.theme.small).color(m.theme.color("muted")));
                    }
                    egui::ScrollArea::vertical().id_salt("ls rows").max_height(10.0 * (ROW + 2.0)).min_scrolled_height(10.0 * (ROW + 2.0)).show(ui, |ui| {
                        for (k, (r, name, t)) in rows.iter().enumerate() {
                            let (rr, resp) = ui.allocate_exact_size(vec2(LS_W, ROW), Sense::click());
                            let face = if k == cursor && sub.is_none() && focused { "controls/list_row_selected" } else if k == cursor || resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
                            if let Some(s) = m.art.sprite(face) {
                                ui.painter().add(paint::nine_slice(&s, rr, (rr.height() / s.size.y).min(1.0), Color32::WHITE));
                            }
                            if let Some(tex) = icons.get(k).copied().flatten() {
                                let ir = Rect::from_min_size(pos2(rr.min.x + 8.0, rr.center().y - 11.0), vec2(22.0, 22.0));
                                ui.painter().image(tex, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), super::chat::colour(t.to_le_bytes()));
                            }
                            if r.num != 0 {
                                ui.painter().text(pos2(rr.min.x + 36.0, rr.center().y), Align2::LEFT_CENTER, r.num.to_string(), FontId::monospace(m.theme.small), m.theme.color("gold"));
                            }
                            let item = info.as_ref().and_then(|c| c.name(r.item)).unwrap_or_default();
                            ui.painter().text(pos2(rr.min.x + 52.0, rr.center().y), Align2::LEFT_CENTER, item, FontId::proportional(m.theme.small), super::chat::colour(r.name_tint.to_le_bytes()));
                            ui.painter().text(pos2(rr.max.x - 10.0, rr.center().y), Align2::RIGHT_CENTER, name, FontId::proportional(m.theme.small), m.theme.color("text"));
                            if focused && sub.is_none() {
                                if resp.hovered() && moved {
                                    row_hover = Some(k);
                                }
                                if resp.clicked() {
                                    row_click = Some(k);
                                }
                            }
                            ui.add_space(2.0);
                        }
                    });
                });
                if let Some(s) = &sub {
                    ui.add_space(14.0);
                    got = column(ui, m, gu, s, focused, moved);
                }
            });
        });
        if let Some(k) = row_hover.filter(|&k| k != cursor) {
            if let Some(l) = w.linkshell.as_mut() {
                l.cursor = k;
            }
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        if let Some(k) = row_click {
            if let Some(l) = w.linkshell.as_mut() {
                l.cursor = k;
            }
            linkshell::key(w, gu, egui::Key::Enter);
        }
        let set = |w: &mut World, c: u16| match w.linkshell.as_mut().and_then(|x| x.sub.as_mut()) {
            Some(s) if s.sel != c => {
                s.sel = c;
                true
            }
            _ => false,
        };
        act(w, gu, got, set, |w, gu| linkshell::key(w, gu, egui::Key::Enter));
    }
}
