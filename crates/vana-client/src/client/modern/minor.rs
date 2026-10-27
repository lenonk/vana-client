//! Small retail windows in the modern style; their modules keep the rules and keys: the block list (blklist.rs, its
//! names and blkmain's Delete) and the chat mode window (chatctrl.rs, its seven rows with the tell name, the shells
//! and the Unity, the cursor row's range under them), the comment editor (seacom.rs) and the Linkshell Concierge
//! (concierge.rs), Mog House Remodel (remodel.rs) and the Adventuring Primer's lists (primer.rs).

use super::super::{World, blklist, chatctrl, concierge, primer, remodel, seacom};
use super::craft::{button, caption};
use super::{MenuWin, Panels, Title, paint, social};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontId, Sense, pos2, vec2};

const W: f32 = 300.0;
const ROW: f32 = 28.0;
const TALL: f32 = 15.0 * (ROW + 2.0);

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    block_list(p, w);
    chat_mode(p, w);
    comment(p, w);
    concierge_list(p, w);
    remodel_list(p, w);
    primer_lists(p, w);
}

/// Plain rows (text, grey), the cursor's lit while focused and kept in view. (hovered, clicked) while focused.
fn plain_rows(ui: &mut egui::Ui, m: &super::Modern, salt: &str, rows: &[(String, bool)], cursor: usize, focused: bool) -> (Option<usize>, Option<usize>) {
    let moved = ui.ctx().input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let seen = egui::Id::unique((salt, "plain cursor seen"));
    let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
    ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
    let (mut hover, mut click) = (None, None);
    egui::ScrollArea::vertical().id_salt(salt).max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, (text, grey)) in rows.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == cursor && focused => "controls/list_row_selected",
                _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                _ if *grey => "controls/list_row_disabled",
                _ => "controls/list_row_normal",
            };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            let c = if *grey { m.theme.color("muted") } else { m.theme.color("text") };
            let clip = egui::Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
            ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(m.theme.small), c);
            if k == cursor && follow {
                ui.scroll_to_rect(r, None);
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
    (hover, click)
}

/// A list's mouse: hover moves the cursor (SE 0001), a click moves it and is Enter.
fn act(w: &mut World, (hover, click): (Option<usize>, Option<usize>), set: impl Fn(&mut World, usize) -> bool, enter: impl Fn(&mut World)) {
    for k in hover.into_iter().chain(click) {
        if set(w, k) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
    if click.is_some() {
        enter(w);
    }
}

/// Mog House Remodel (remodel.rs): "No changes" and the 2F styles, the patio grey without its key item.
fn remodel_list(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((rows, sel)) = remodel::look(w, gu) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = gu.focus_text(remodel::WINDOW, 0)[1].clone().unwrap_or_else(|| "Remodel".into());
    let mut got = (None, None);
    p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: None }), W + 44.0, |ui| {
        got = plain_rows(ui, m, "remodel rows", &rows, sel, free);
    });
    act(w, got, remodel::set_sel, |w| remodel::key(w, egui::Key::Enter));
}

/// The Adventuring Primer (primer.rs): guide00's categories, guide01's topics beside them with its arrows (the other
/// categories); hidden while a page is up in the map window.
fn primer_lists(p: &mut Panels, w: &mut World) {
    let Some(v) = primer::look(w).filter(|v| !v.page) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let title = w.battle.as_ref().and_then(|b| b.group9(0xc5)).unwrap_or_else(|| "Help Desk".into());
    let width = W + if v.list.is_some() { 16.0 + W } else { 0.0 } + 44.0;
    let (mut cats, mut topics, mut turn) = ((None, None), (None, None), None);
    p.panel("window", Some(Title { icon: Some("icons/help"), text: &title, note: None }), width, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(W);
                let rows: Vec<(String, bool)> = v.cats.iter().map(|c| (c.clone(), false)).collect();
                cats = plain_rows(ui, m, "primer cats", &rows, v.cursor, free && v.list.is_none());
            });
            let Some((name, list, cursor)) = &v.list else { return };
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(W);
                ui.horizontal(|ui| {
                    if button(ui, m, "<", 28.0, false, false) && free {
                        turn = Some(egui::Key::ArrowLeft);
                    }
                    ui.label(egui::RichText::new(name).size(m.theme.small).color(m.theme.color("gold")));
                    if button(ui, m, ">", 28.0, false, false) && free {
                        turn = Some(egui::Key::ArrowRight);
                    }
                });
                ui.add_space(4.0);
                let rows: Vec<(String, bool)> = list.iter().map(|t| (t.clone(), false)).collect();
                topics = plain_rows(ui, m, "primer topics", &rows, *cursor, free);
            });
        });
    });
    if !free {
        return;
    }
    if let Some(k) = turn {
        primer::key(w, k, false);
        return;
    }
    act(w, cats, primer::set_cat, |w| primer::key(w, egui::Key::Enter, false));
    act(w, topics, primer::set_topic, |w| primer::key(w, egui::Key::Enter, false));
}

/// A retail tint (0x80 is full) as a colour.
fn tint_colour(t: u32) -> Color32 {
    let c = |s: u32| ((t >> s & 0xff) * 2).min(255) as u8;
    Color32::from_rgb(c(16), c(8), c(0))
}

/// The Linkshell Concierge (concierge.rs): the event's title, lscswin's filters as tabs, the header and every shell
/// (its icon in the shell's colour, the name, green for ours, the language, the members wanted).
fn concierge_list(p: &mut Panels, w: &mut World) {
    const LIST_W: f32 = 460.0;
    let Some(gu) = p.gu else { return };
    let Some(v) = concierge::look(w) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let info = w.item_info.clone();
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let focused = free && v.lsc.is_none();
    let (mut tab, mut hover, mut click) = (None, None, None);
    let title = gu.focus_text(super::super::linkshell::LIST, 0)[1].clone().unwrap_or_else(|| "Linkshell".into());
    p.panel("window", Some(Title { icon: Some("icons/linkshell"), text: &title, note: None }), LIST_W + 44.0, |ui| {
        if !v.title.is_empty() {
            ui.label(egui::RichText::new(&v.title).size(m.theme.ui).color(m.theme.color("gold")));
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            for (k, sprite) in [(1u16, 715usize), (2, 92), (3, 93), (4, 574)] {
                let t = m.caption(gu, "menu    windowps", sprite).unwrap_or_default();
                let lit = v.lsc.map_or(v.filter == k, |s| s == k);
                if button(ui, m, &t, 108.0, lit, false) && free {
                    tab = Some(k);
                }
            }
        });
        ui.add_space(6.0);
        let seen = egui::Id::unique("concierge cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(v.cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, v.cursor));
        egui::ScrollArea::vertical().id_salt("concierge rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
            for (k, r) in v.rows.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == v.cursor && focused => "controls/list_row_selected",
                    _ if k == v.cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, rect, (rect.height() / s.size.y).min(1.0), Color32::WHITE));
                }
                let y = rect.center().y;
                let small = FontId::proportional(m.theme.small);
                match r {
                    None => {
                        let head = m.caption(gu, "menu    windowps", 718).unwrap_or_default();
                        ui.painter().text(pos2(rect.min.x + 24.0, y), Align2::LEFT_CENTER, head, small, m.theme.color("gold"));
                    }
                    Some(r) => {
                        if let Some(t) = info.as_ref().and_then(|i| m.item_icon(ctx, i, r.item)) {
                            let ir = egui::Rect::from_center_size(pos2(rect.min.x + 30.0, y), vec2(20.0, 20.0));
                            ui.painter().image(t, ir, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint_colour(r.tint));
                        }
                        let nc = if r.ours { tint_colour(0x8050_9050) } else { m.theme.color("text") };
                        ui.painter().text(pos2(rect.min.x + 48.0, y), Align2::LEFT_CENTER, &r.name, small.clone(), nc);
                        ui.painter().text(pos2(rect.max.x - 70.0, y), Align2::CENTER_CENTER, r.lang, small.clone(), m.theme.color("text"));
                        ui.painter().text(pos2(rect.max.x - 14.0, y), Align2::RIGHT_CENTER, r.wanted.to_string(), FontId::monospace(m.theme.small), m.theme.color("text"));
                    }
                }
                if k == v.cursor && follow {
                    ui.scroll_to_rect(rect, None);
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
    if !free {
        return;
    }
    if let Some(k) = tab {
        concierge::pick_filter(w, gu, k);
        return;
    }
    for k in hover.into_iter().chain(click) {
        if concierge::set_row(w, k) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
    if click.is_some() {
        concierge::key(w, gu, egui::Key::Enter);
    }
}

/// The comment editor (seacom.rs): three lines of 40, the edited one live with its caret; a click picks a line, OK
/// closes as Escape does (the text kept and sent).
fn comment(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((mode, lines, line)) = seacom::view(w) else { return };
    let m = p.m;
    let blink = (p.ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
    let title = if mode == 0 { gu.title_msg(0x0f).unwrap_or_else(|| "Set Bazaar".into()) } else { "Comment".to_string() };
    let help = gu.menu_msg(0x32, "");
    let (mut pick, mut done) = (None, false);
    p.panel("comment", Some(Title { icon: None, text: &title, note: None }), 420.0 + 44.0, |ui| {
        if !help.is_empty() {
            ui.label(egui::RichText::new(&help).size(m.theme.small).color(m.theme.color("muted")));
            ui.add_space(4.0);
        }
        for (k, l) in lines.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(420.0, ROW), Sense::click());
            let face = if k == line { "controls/list_row_selected" } else { "controls/list_row_normal" };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            let text = if k == line && blink { format!("{l}|") } else { l.clone() };
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, FontId::monospace(m.theme.small), m.theme.color("text"));
            if resp.clicked() {
                pick = Some(k);
            }
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
        let ok = m.caption(gu, "menu    keytops3", 53).unwrap_or_else(|| "OK".into());
        done = button(ui, m, &ok, 110.0, false, false);
    });
    if let Some(k) = pick {
        seacom::set_edit_line(w, k);
    }
    if done {
        seacom::right_click(w);
    }
}

/// The names, the cursor kept in view; Delete under them while blkmain is up.
fn block_list(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some((rows, cursor, main, empty)) = blklist::view(w) else { return };
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let title = gu.focus_text(blklist::LIST, 1)[1].clone().unwrap_or_else(|| "Blacklist".into());
    let (mut hover, mut click, mut delete) = (None, None, false);
    p.panel("blklist", Some(Title { icon: Some("icons/party"), text: &title, note: None }), W + 44.0, |ui| {
        if !empty.is_empty() {
            ui.label(egui::RichText::new(&empty).size(m.theme.small).color(m.theme.color("muted")));
        }
        let seen = egui::Id::unique("blklist cursor seen");
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
        let focused = free && !main;
        egui::ScrollArea::vertical().id_salt("blklist rows").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
            for (k, name) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
                let face = match () {
                    _ if k == cursor && focused => "controls/list_row_selected",
                    _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                }
                ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), m.theme.color("text"));
                if k == cursor && follow {
                    ui.scroll_to_rect(r, None);
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
        if main {
            ui.add_space(8.0);
            if button(ui, m, &caption(gu, blklist::MAIN, 1, "Delete"), 130.0, true, false) && free {
                delete = true;
            }
        }
    });
    if !free {
        return;
    }
    if delete {
        blklist::key(w, egui::Key::Enter);
        return;
    }
    for k in hover.into_iter().chain(click) {
        if blklist::set_cursor(w, k) {
            w.sounds.push(Se::Cursor as u32);
        }
    }
    if click.is_some() {
        blklist::key(w, egui::Key::Enter);
    }
}

/// chatctrl's rows as a column (its links walk them as drawn), the values at the right; subwindo's range under them.
fn chat_mode(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    let Some(v) = chatctrl::look(w, gu) else { return };
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let title = gu.focus_text(chatctrl::WINDOW, v.sel)[1].clone().unwrap_or_else(|| "Chat".into());
    let mut got = (None, None);
    p.panel("chatctrl", Some(Title { icon: None, text: &title, note: None }), social::width(1), |ui| {
        let col = MenuWin { name: chatctrl::WINDOW, sel: v.sel, off: v.off.clone(), labels: Vec::new(), marked: Vec::new(), values: v.values.clone() };
        got = social::column(ui, m, gu, &col, free, moved);
        ui.add_space(6.0);
        let head = m.caption(gu, "menu    windowps", 283).unwrap_or_default();
        let range = m.caption(gu, "menu    windowps", v.range).unwrap_or_default();
        ui.label(egui::RichText::new(head).size(m.theme.small).color(m.theme.color("gold")));
        ui.label(egui::RichText::new(range).size(m.theme.small).color(m.theme.color("text")));
    });
    if free {
        social::act(w, gu, got, chatctrl::set_sel, |w, _| chatctrl::key(w, egui::Key::Enter));
    }
}
