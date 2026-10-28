//! The modern macro windows over client/macroedit.rs, which keeps every rule and key: mcrmenu; the forty books in one
//! list with mcrselop and the name field; the Ctrl and Alt palettes as keycaps with the sets as tabs and mcrbedit's
//! rows as buttons; the editor with mcrledit's. And the palette shown while Ctrl or Alt is held.

use super::super::game_ui::GameUi;
use super::super::{World, esedit, esmenu, macroedit};
use super::{MenuWin, Panels, Title, paint, social};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

const KEY_W: f32 = 100.0;
const KEY_H: f32 = 52.0;
const ROW: f32 = 28.0;
const BOOK_W: f32 = 300.0;
const LINE_W: f32 = 560.0;

/// What the mouse did this frame (hovered when the pointer moved, or clicked).
#[derive(Default)]
struct Mouse {
    menu: (Option<u16>, Option<u16>),
    book: (Option<usize>, Option<usize>),
    opt: (Option<u16>, Option<u16>),
    cell: (Option<(u8, u16)>, Option<(u8, u16)>),
    bopt: Option<u16>,
    set: Option<usize>,
    field: (Option<u16>, Option<u16>),
    lopt: Option<u16>,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    held_palette(p, w);
    let Some(v) = macroedit::view(w, gu) else {
        if let Some(sv) = esmenu::view(w) {
            equip_sets(p, w, gu, &sv);
        }
        return;
    };
    let (m, ctx) = (p.m, p.ctx);
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let mut mouse = Mouse::default();
    let width = if v.editor.is_some() {
        LINE_W + 44.0
    } else if v.palettes.is_some() {
        10.0 * (KEY_W + 6.0) + 90.0
    } else if v.books.is_some() {
        BOOK_W + social::width(1) + 12.0
    } else {
        social::width(1)
    };
    p.panel("window", Some(Title { icon: Some("icons/macro"), text: "Macros", note: None }), width, |ui| {
        if let Some(e) = &v.editor {
            editor(ui, m, gu, e, moved, &mut mouse);
        } else if let Some(pv) = &v.palettes {
            palettes(ui, m, gu, pv, moved, &mut mouse);
        } else if let Some(b) = &v.books {
            books(ui, m, gu, b, moved, &mut mouse);
        } else {
            let (sel, monstrosity, focused) = v.menu;
            let mw = MenuWin { name: macroedit::MENU, sel, off: if monstrosity { vec![2] } else { Vec::new() }, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            mouse.menu = social::stack(ui, m, gu, std::slice::from_ref(&mw), focused, moved);
        }
    });
    act(w, gu, mouse);
}

/// The mouse's moves through the module's own setters and keys: hover moves the cursor, a click moves it and is Enter.
fn act(w: &mut World, gu: &GameUi, mouse: Mouse) {
    let enter = |w: &mut World| macroedit::key(w, gu, egui::Key::Enter);
    let cursor = |w: &mut World, moved: bool| {
        if moved {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
    };
    social::act(w, gu, mouse.menu, macroedit::set_menu, |w, gu| macroedit::key(w, gu, egui::Key::Enter));
    if let Some(k) = mouse.book.0 {
        let moved = macroedit::set_book(w, k);
        cursor(w, moved);
    }
    if let Some(k) = mouse.book.1 {
        macroedit::set_book(w, k);
        enter(w);
    }
    social::act(w, gu, mouse.opt, macroedit::set_opt, |w, gu| macroedit::key(w, gu, egui::Key::Enter));
    if let Some((pal, col)) = mouse.cell.0 {
        let moved = macroedit::set_cell(w, pal, col);
        cursor(w, moved);
    }
    if let Some((pal, col)) = mouse.cell.1 {
        macroedit::set_cell(w, pal, col);
        enter(w);
    }
    if let Some(o) = mouse.bopt {
        macroedit::set_bopt(w, o);
        enter(w);
    }
    if let Some(s) = mouse.set {
        macroedit::set_set(w, s);
    }
    if let Some(c) = mouse.field.0 {
        let moved = macroedit::set_field(w, c);
        cursor(w, moved);
    }
    if let Some(c) = mouse.field.1 {
        macroedit::set_field(w, c);
        enter(w);
    }
    if let Some(o) = mouse.lopt {
        macroedit::set_lopt(w, o);
        enter(w);
    }
}

fn caption(gu: &GameUi, menu: &str, child: u16, fallback: &str) -> String {
    gu.child_caption(menu, child).unwrap_or_else(|| fallback.to_string())
}

/// A button in the pack's frame: lit under the cursor, grey when off; true when clicked.
fn button(ui: &mut Ui, m: &super::Modern, label: &str, width: f32, lit: bool, off: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    let face = match () {
        _ if off => "controls/button_disabled",
        _ if lit => "controls/button_selected",
        _ if resp.hovered() => "controls/button_hover",
        _ => "controls/button_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
    }
    let c = if off { m.theme.color("muted") } else if lit { m.theme.color("gold") } else { m.theme.color("text") };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(m.theme.small), c);
    resp.clicked() && !off
}

/// A list row in the pack's frame: selected under a focused cursor, else lit under it or the pointer.
fn row_face(ui: &Ui, m: &super::Modern, r: Rect, cursor: bool, focused: bool, hovered: bool) {
    let face = match () {
        _ if cursor && focused => "controls/list_row_selected",
        _ if cursor || hovered => "controls/list_row_hover",
        _ => "controls/list_row_normal",
    };
    super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
}

/// mcrselec over both pages (the active book starred, the name field in its row) and mcrselop beside it.
fn books(ui: &mut Ui, m: &super::Modern, gu: &GameUi, b: &macroedit::Books, moved: bool, mouse: &mut Mouse) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(BOOK_W);
            let seen = egui::Id::unique("macro book seen");
            let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(b.cur);
            ui.ctx().data_mut(|d| d.insert_temp(seen, b.cur));
            let tall = 14.0 * (ROW + 2.0);
            egui::ScrollArea::vertical().id_salt("macro books").max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
                for (k, name) in b.names.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(BOOK_W - 14.0, ROW), Sense::click());
                    row_face(ui, m, r, k == b.cur, b.focused, resp.hovered());
                    let font = FontId::proportional(m.theme.small);
                    ui.painter().text(pos2(r.min.x + 40.0, r.center().y), Align2::RIGHT_CENTER, format!("{}", k + 1), font.clone(), m.theme.color("muted"));
                    let text = match &b.rename {
                        Some(t) if k == b.cur => format!("{t}{}", if (ui.ctx().input(|i| i.time) * 1.5) as u64 % 2 == 0 { "|" } else { "" }),
                        _ => name.clone(),
                    };
                    ui.painter().text(pos2(r.min.x + 52.0, r.center().y), Align2::LEFT_CENTER, text, font, m.theme.color("text"));
                    if k == b.active {
                        ui.painter().add(paint::star(pos2(r.max.x - 18.0, r.center().y), 6.5, m.theme.color("gold")));
                    }
                    if k == b.cur && follow {
                        ui.scroll_to_rect(r, None);
                    }
                    if b.focused && resp.hovered() && moved {
                        mouse.book.0 = Some(k);
                    }
                    if b.focused && resp.clicked() {
                        mouse.book.1 = Some(k);
                    }
                    ui.add_space(2.0);
                }
            });
        });
        if let Some(o) = b.opt {
            ui.add_space(12.0);
            let off = if b.paste_ok { Vec::new() } else { vec![4] };
            let mw = MenuWin { name: macroedit::SELOP, sel: o, off, labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
            mouse.opt = social::stack(ui, m, gu, std::slice::from_ref(&mw), true, moved);
        }
    });
}

/// A keycap: its key's digit, the macro's title, the cursor's ring; dimmed when `off` (nothing in it can be used now).
fn keycap(ui: &mut Ui, m: &super::Modern, digit: usize, title: &str, cursor: Option<bool>, off: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(KEY_W, KEY_H), Sense::click());
    let face = match () {
        _ if cursor.is_some() => "controls/item_slot_selected",
        _ if resp.hovered() => "controls/item_slot_hover",
        _ => "controls/item_slot_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, if off { Color32::from_gray(130) } else { Color32::WHITE }));
    }
    if let Some(focus) = cursor {
        let c = if focus { m.theme.color("focus") } else { m.theme.color("bronze") };
        ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, c), egui::StrokeKind::Inside);
    }
    ui.painter().text(r.min + vec2(8.0, 6.0), Align2::LEFT_TOP, format!("{}", (digit + 1) % 10), FontId::monospace(m.theme.small), m.theme.color("muted"));
    let clip = r.shrink(4.0);
    let colour = m.theme.color(if off { "muted" } else { "text" });
    ui.painter().with_clip_rect(clip).text(pos2(r.center().x, r.center().y + 6.0), Align2::CENTER_CENTER, title, FontId::proportional(m.theme.small), colour);
    resp
}

/// The book and its sets as tabs, the Ctrl and Alt palettes, mcrbedit's rows as buttons for the cursor's macro.
fn palettes(ui: &mut Ui, m: &super::Modern, gu: &GameUi, pv: &macroedit::Palettes, moved: bool, mouse: &mut Mouse) {
    ui.label(egui::RichText::new(&pv.book).size(m.theme.ui).color(m.theme.color("gold")));
    ui.add_space(4.0);
    let sets: Vec<String> = (1..=10).map(|s| format!("Set {s}")).collect();
    let labels: Vec<&str> = sets.iter().map(String::as_str).collect();
    for (s, resp) in super::tabs::strip(ui, m, "macro sets", &labels, Some(pv.set), None).into_iter().enumerate() {
        if resp.clicked() {
            mouse.set = Some(s);
        }
    }
    ui.add_space(8.0);
    for (pal, label) in [(1u8, caption(gu, "menu    mcr1edit", 1, "Ctrl")), (2, caption(gu, "menu    mcr2edit", 1, "Alt"))] {
        ui.horizontal(|ui| {
            ui.allocate_ui(vec2(40.0, KEY_H), |ui| {
                ui.centered_and_justified(|ui| ui.label(egui::RichText::new(&label).size(m.theme.small).color(m.theme.color("gold"))));
            });
            for col in 0..10u16 {
                let i = col as usize + 10 * (pal as usize - 1);
                let cursor = (pv.cols[pal as usize - 1] == col && pv.focus == pal).then_some(pv.focused);
                let resp = keycap(ui, m, col as usize, pv.titles.get(i).map_or("", |t| t.as_str()), cursor, false);
                if resp.hovered() && moved && pv.focused {
                    mouse.cell.0 = Some((pal, col));
                }
                if resp.clicked() {
                    mouse.cell.1 = Some((pal, col));
                }
            }
        });
        ui.add_space(6.0);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (o, fallback) in [(1u16, "Copy"), (2, "Paste"), (3, "Delete")] {
            if button(ui, m, &caption(gu, macroedit::BEDIT, o, fallback), 110.0, pv.bopt == Some(o), o == 2 && !pv.paste_ok) {
                mouse.bopt = Some(o);
            }
        }
    });
}

/// The macro: its key, Prev / Next, the title and six lines (the edited one live), mcrledit's rows as buttons.
fn editor(ui: &mut Ui, m: &super::Modern, gu: &GameUi, e: &macroedit::EditorView, moved: bool, mouse: &mut Mouse) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&e.label).size(m.theme.ui).color(m.theme.color("gold")));
        ui.add_space(12.0);
        for (c, fallback) in [(1u16, "Prev"), (2, "Next")] {
            if button(ui, m, &caption(gu, macroedit::MACRO, c, fallback), 80.0, e.sel == c && e.focused, false) {
                mouse.field.1 = Some(c);
            }
        }
    });
    ui.add_space(8.0);
    let caret = if (ui.ctx().input(|i| i.time) * 1.5) as u64 % 2 == 0 { "|" } else { "" };
    for (f, text) in e.fields.iter().enumerate() {
        let child = f as u16 + 3;
        let (r, resp) = ui.allocate_exact_size(vec2(LINE_W, ROW), Sense::click());
        row_face(ui, m, r, e.sel == child, e.focused || e.editing == Some(f), resp.hovered());
        let label = if f == 0 { "Title".to_string() } else { format!("{f}") };
        ui.painter().text(pos2(r.min.x + 50.0, r.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(m.theme.small), m.theme.color("muted"));
        let shown = if e.editing == Some(f) { format!("{text}{caret}") } else { text.clone() };
        let clip = Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
        ui.painter().with_clip_rect(clip).text(pos2(r.min.x + 60.0, r.center().y), Align2::LEFT_CENTER, shown, FontId::monospace(m.theme.small), m.theme.color("text"));
        if e.focused && resp.hovered() && moved {
            mouse.field.0 = Some(child);
        }
        if resp.clicked() && e.editing.is_none() {
            mouse.field.1 = Some(child);
        }
        ui.add_space(2.0);
    }
    ui.add_space(6.0);
    let on_line = (4..=9).contains(&e.sel) && e.editing.is_none();
    ui.horizontal(|ui| {
        for (o, fallback) in [(1u16, "Copy Line"), (2, "Paste Line"), (3, "Delete Line")] {
            if button(ui, m, &caption(gu, macroedit::LEDIT, o, fallback), 120.0, e.lopt == Some(o), !on_line || (o == 2 && !e.paste_ok)) {
                mouse.lopt = Some(o);
            }
        }
    });
}

/// The palette while Ctrl or Alt is held (FUN_101a3b20): its ten keys over the chat window, the cursor's cell lit.
fn held_palette(p: &mut Panels, w: &World) {
    let Some(mac) = w.macros.as_ref().filter(|_| w.palette.0 != 0) else { return };
    let (m, ctx) = (p.m, p.ctx);
    let pal = w.palette.0 as usize;
    let screen = ctx.content_rect();
    let width = 10.0 * (KEY_W + 6.0) + 44.0;
    let h = m.rects.borrow().get("macro palette").map_or(120.0, |r| r.height());
    // On the chat window, else the screen's bottom (a faded chat window folds into the screen's edge).
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let at = pos2(screen.center().x - width / 2.0, edge - h - 6.0);
    let label = if pal == 1 { "Ctrl" } else { "Alt" };
    let note = format!("{}  Set {}", mac.book_title(mac.book), mac.set + 1);
    let titles: Vec<String> = (0..10).map(|k| mac.recs[k + 10 * (pal - 1)].title()).collect();
    let off: Vec<bool> = (0..10).map(|k| m.macro_off(w, &mac.recs[k + 10 * (pal - 1)])).collect();
    let cur = w.palette.3[pal - 1];
    p.panel_at("macro palette", Some(Title { icon: Some("icons/macro"), text: label, note: Some(&note) }), width, at, |ui| {
        ui.horizontal(|ui| {
            for (k, t) in titles.iter().enumerate() {
                keycap(ui, m, k, t, (k == cur).then_some(true), off[k]);
            }
        });
    });
}

/// The storage the slot editor lists (mcrmogc's rows: child 1 -> container 0, 2 -> 1, 3 -> 2, the others their own).
const STORES: [(u16, u8); 16] = [(1, 0), (2, 1), (3, 2), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8), (9, 9), (10, 10), (11, 11), (12, 12), (13, 13), (14, 14), (15, 15), (16, 16)];
/// The grid's slots by child (column by column, as the equipment window).
const SLOT_NAMES: [&str; 16] = ["Main", "Head", "Body", "Back", "Sub", "Neck", "Hands", "Waist", "Range", "Ear 1", "Ring 1", "Legs", "Ammo", "Ear 2", "Ring 2", "Feet"];
const TILE: f32 = 52.0;

/// Edit Equip. Set: the page as tabs and its twenty sets with mcresmn beside them; the slot editor: the grid of sixteen
/// slots (an item's icon, Remove's cross, Keep the slot's name) beside the storage as tabs and its list.
fn equip_sets(p: &mut Panels, w: &mut World, gu: &GameUi, v: &esmenu::View) {
    let (m, ctx) = (p.m, p.ctx);
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let Some(info) = w.item_info.clone() else { return };
    let title = gu.title_msg(0xac).unwrap_or_else(|| "Equipment Sets".into());
    let enter = |w: &mut World| macroedit::key(w, gu, egui::Key::Enter);
    if let Some(e) = &v.editor {
        let (mut tile_click, mut row) = (None, (None, None));
        let mut store = None;
        let mut done = false;
        p.panel("window", Some(Title { icon: Some("icons/equip_set"), text: &title, note: v.names.get(v.row).map(|s| s.as_str()) }), 4.0 * (TILE + 8.0) + 380.0 + 60.0, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    let (area, _) = ui.allocate_exact_size(vec2(4.0 * (TILE + 8.0), 4.0 * (TILE + 8.0)), Sense::hover());
                    for c in 1..=16u16 {
                        let (col, rw) = ((c - 1) / 4, (c - 1) % 4);
                        let r = Rect::from_min_size(area.min + vec2(col as f32 * (TILE + 8.0), rw as f32 * (TILE + 8.0)), vec2(TILE, TILE));
                        let resp = ui.interact(r, egui::Id::unique(("set slot", c)), Sense::click());
                        let allowed = e.allowed[c as usize - 1];
                        let face = match () {
                            _ if e.grid == c => "controls/item_slot_selected",
                            _ if !allowed => "controls/item_slot_locked",
                            _ if resp.hovered() => "controls/item_slot_hover",
                            _ => "controls/item_slot_normal",
                        };
                        if let Some(sp) = m.art.sprite(face) {
                            ui.painter().add(paint::nine_slice(&sp, r, 0.5, Color32::WHITE));
                        }
                        if e.grid == c {
                            let col = if e.on_list { m.theme.color("bronze") } else { m.theme.color("focus") };
                            ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, col), egui::StrokeKind::Inside);
                        }
                        let (state, id) = e.slots[c as usize - 1];
                        match state {
                            1 => {
                                if let Some(t) = m.item_icon(ctx, &info, id) {
                                    ui.painter().image(t, r.shrink(10.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                }
                            }
                            2 => {
                                let x = r.shrink(16.0);
                                let st = egui::Stroke::new(2.5, m.theme.color("hp"));
                                ui.painter().line_segment([x.left_top(), x.right_bottom()], st);
                                ui.painter().line_segment([x.right_top(), x.left_bottom()], st);
                            }
                            _ => {
                                ui.painter().text(r.center(), Align2::CENTER_CENTER, SLOT_NAMES[c as usize - 1], FontId::proportional(m.theme.small * 0.85), m.theme.color("muted"));
                            }
                        }
                        if resp.clicked() {
                            tile_click = Some(c);
                        }
                    }
                    ui.add_space(8.0);
                    if button(ui, m, "Done", 120.0, false, false) {
                        done = true;
                    }
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    let stores: Vec<(String, u8)> = STORES.iter().filter_map(|&(child, c)| Some((gu.child_caption("menu    mcrmogc", child)?, c))).collect();
                    let labels: Vec<&str> = stores.iter().map(|(n, _)| n.as_str()).collect();
                    let open = stores.iter().position(|(_, c)| *c == e.container);
                    for (k, resp) in super::tabs::strip(ui, m, "equip set storage", &labels, open, None).into_iter().enumerate() {
                        if resp.clicked() {
                            store = Some(stores[k].1);
                        }
                    }
                    ui.add_space(6.0);
                    let tall = 10.0 * (ROW + 4.0);
                    egui::ScrollArea::vertical().id_salt("set list").max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
                        for (k, (id, name, count, tint)) in e.rows.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(vec2(366.0, ROW + 2.0), Sense::click());
                            row_face(ui, m, r, e.cur == Some(k), e.on_list, resp.hovered());
                            if *id != 0
                                && let Some(t) = m.item_icon(ctx, &info, *id)
                            {
                                ui.painter().image(t, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - 11.0), vec2(22.0, 22.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            let c = if *tint == 0x8080_8080 { m.theme.color("text") } else { super::chat::colour(tint.to_le_bytes()) };
                            ui.painter().text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), c);
                            if *count > 1 {
                                ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(m.theme.small), m.theme.color("text"));
                            }
                            if resp.hovered() && moved && e.on_list {
                                row.0 = Some(k);
                            }
                            if resp.clicked() {
                                row.1 = Some(k);
                            }
                            ui.add_space(2.0);
                        }
                    });
                });
            });
        });
        if let Some(c) = store {
            esedit::set_container(w, c);
        }
        if let Some(k) = row.0
            && esedit::set_row(w, k)
        {
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        if let Some(k) = row.1 {
            esedit::set_row(w, k);
            enter(w);
        }
        if let Some(c) = tile_click
            && (esedit::set_grid(w, c) || e.grid == c)
        {
            enter(w);
        }
        if done {
            // Escape from the grid closes the editor (in mode 1, twice: the pick, then the editor).
            esedit::set_grid(w, e.grid);
            macroedit::key(w, gu, egui::Key::Escape);
        }
        return;
    }
    let (mut page, mut row, mut opt) = (None, (None, None), (None, None));
    p.panel("window", Some(Title { icon: Some("icons/equip_set"), text: &title, note: None }), BOOK_W + social::width(1) + 12.0, |ui| {
        let pages: Vec<String> = (1..=10).map(|k| k.to_string()).collect();
        let labels: Vec<&str> = pages.iter().map(String::as_str).collect();
        for (k, resp) in super::tabs::strip(ui, m, "equip set pages", &labels, Some(v.page), None).into_iter().enumerate() {
            if resp.clicked() {
                page = Some(k);
            }
        }
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(BOOK_W);
                for (k, name) in v.names.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(BOOK_W - 14.0, ROW), Sense::click());
                    row_face(ui, m, r, v.cur == Some(k) || (v.opt.is_some() && v.row == k), v.focused, resp.hovered());
                    let font = FontId::proportional(m.theme.small);
                    ui.painter().text(pos2(r.min.x + 40.0, r.center().y), Align2::RIGHT_CENTER, format!("{}", v.page * 20 + k + 1), font.clone(), m.theme.color("muted"));
                    let text = match &v.rename {
                        Some(t) if k == v.row => format!("{t}{}", if (ui.ctx().input(|i| i.time) * 1.5) as u64 % 2 == 0 { "|" } else { "" }),
                        _ => name.clone(),
                    };
                    ui.painter().text(pos2(r.min.x + 52.0, r.center().y), Align2::LEFT_CENTER, text, font, m.theme.color("text"));
                    if v.focused && resp.hovered() && moved {
                        row.0 = Some(k);
                    }
                    if v.focused && resp.clicked() {
                        row.1 = Some(k);
                    }
                    ui.add_space(2.0);
                }
            });
            if let Some(o) = v.opt {
                ui.add_space(12.0);
                let mw = MenuWin { name: "menu    mcresmn", sel: o, off: v.off.clone(), labels: Vec::new(), marked: Vec::new(), values: Vec::new() };
                opt = social::stack(ui, m, gu, std::slice::from_ref(&mw), true, moved);
            }
        });
    });
    if let Some(k) = page {
        esmenu::go_page(w, k);
    }
    if let Some(k) = row.0
        && esmenu::set_row(w, k)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = row.1 {
        esmenu::set_row(w, k);
        enter(w);
    }
    social::act(w, gu, opt, esmenu::set_opt, |w, gu| macroedit::key(w, gu, egui::Key::Enter));
}
