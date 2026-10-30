//! The modern Maze Tabula editor over client/maze.rs, which keeps every rule and key: the list (vouchers, runes, the
//! placed runes) or the shape filter beside a large 5 x 5 grid with the tabula's blocked cells and element marks, the
//! runes in their element's colour and the held one over them; the rune or voucher text under it.

use super::super::World;
use super::super::maze::{self, Mode, Row};
use super::craft::tint;
use super::{Panels, STRONG, Title};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const COL_W: f32 = 300.0;
const ROW: f32 = 28.0;
const ICON: f32 = 20.0;
const TALL: f32 = 12.0 * (ROW + 2.0);
const CELL: f32 = 44.0;
const ELEMENTS: [&str; 9] = ["Fire", "Ice", "Wind", "Earth", "Lightning", "Water", "Light", "Dark", ""];
const SHAPES: [&str; 11] = ["Domino", "Corner", "Line of 3", "T", "Square", "L", "J", "S", "Z", "Line of 4", "Cross"];

/// An element's colour (the rune sprites' hues; 8 none).
fn element_colour(e: u8) -> Color32 {
    match e {
        0 => Color32::from_rgb(200, 64, 56),
        1 => Color32::from_rgb(96, 190, 210),
        2 => Color32::from_rgb(80, 180, 96),
        3 => Color32::from_rgb(190, 160, 70),
        4 => Color32::from_rgb(160, 90, 200),
        5 => Color32::from_rgb(70, 110, 220),
        6 => Color32::from_rgb(225, 225, 215),
        7 => Color32::from_rgb(70, 60, 80),
        _ => Color32::from_rgb(130, 130, 130),
    }
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(4.0);
}

/// The list's rows: the item's icon (none for Type), the name in its tint; the cursor kept in view. (hovered,
/// clicked).
fn rows(ui: &mut Ui, m: &super::Modern, w: &World, rows: &[(Row, String, u32)], cursor: usize, focused: bool) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let seen = egui::Id::unique("maze cursor seen");
    let follow = ctx.data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
    ctx.data_mut(|d| d.insert_temp(seen, cursor));
    let mut click = None;
    egui::ScrollArea::vertical().id_salt("maze list").max_height(TALL).auto_shrink([false, true]).show(ui, |ui| {
        for (k, (row, text, t)) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(COL_W - 14.0, ROW), if focused { Sense::click() } else { Sense::hover() });
            let face = match () {
                _ if k == cursor && focused => "controls/list_row_selected",
                _ if k == cursor || focused && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
            let (x, y) = (rect.min.x + 18.0, rect.center().y);
            if let Row::Item(id) = row
                && let Some(tex) = w.item_info.as_ref().and_then(|info| m.item_icon(&ctx, info, *id))
            {
                ui.painter().image(tex, Rect::from_center_size(pos2(x + ICON / 2.0, y), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let label = if *row == Row::Type { format!("{text} ▶") } else { text.clone() };
            ui.painter().text(pos2(x + ICON + 6.0, y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), tint(m, *t));
            if k == cursor && follow {
                ui.scroll_to_rect(rect, None);
            }
            if focused && resp.clicked() {
                click = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    click
}

/// A shape's mask drawn small (its rotation 0), for the filter's rows.
fn shape_mini(ui: &Ui, at: egui::Pos2, shape: u8, colour: Color32) {
    let mask = crate::dat::maze::mask(shape, 0);
    for r in 0..4 {
        for c in 0..4 {
            if mask >> (15 - (4 * r + c)) & 1 != 0 {
                let min = at + vec2(7.0 * c as f32, 7.0 * r as f32 - 12.0);
                ui.painter().rect_filled(Rect::from_min_size(min, vec2(6.0, 6.0)), 1.0, colour);
            }
        }
    }
}

/// prvdunsi's eleven shapes as rows; the row clicked.
fn filter_rows(ui: &mut Ui, m: &super::Modern, cursor: u16) -> Option<u16> {
    let mut click = None;
    for (k, name) in SHAPES.iter().enumerate() {
        let child = k as u16 + 1;
        let (rect, resp) = ui.allocate_exact_size(vec2(COL_W - 14.0, ROW), Sense::click());
        let face = if child == cursor { "controls/list_row_selected" } else if resp.hovered() { "controls/list_row_hover" } else { "controls/list_row_normal" };
        super::rows::face(ui.painter(), m, rect, face, Color32::WHITE);
        shape_mini(ui, pos2(rect.min.x + 16.0, rect.center().y), k as u8, m.theme.color("text"));
        ui.painter().text(pos2(rect.min.x + 56.0, rect.center().y), Align2::LEFT_CENTER, *name, FontId::proportional(m.theme.small), m.theme.color("text"));
        if resp.clicked() {
            click = Some(child);
        }
        ui.add_space(2.0);
    }
    click
}

/// What the pointer did on the grid this frame.
#[derive(Default)]
struct GridMouse {
    hover: Option<(i8, i8)>,
    click: bool,
    right: bool,
    turn: bool,
}

/// The 5 x 5 grid: plain, blocked (dark, crossed) and element cells (their colour faint, the element's initial);
/// the placed runes in their element's colour (brighter on a cell of their element); the held rune over them,
/// pulsing, outlined teal where it fits and red where it does not.
fn grid(ui: &mut Ui, m: &super::Modern, v: &maze::View, carrying: bool) -> GridMouse {
    let side = CELL * 5.0;
    let (r, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let p = ui.painter();
    let cell = |c: usize| Rect::from_min_size(r.min + vec2(CELL * (c % 5) as f32, CELL * (c / 5) as f32), vec2(CELL, CELL)).shrink(2.0);
    let on = |mask: u32, c: usize| mask >> (24 - c) & 1 != 0;
    for c in 0..25 {
        let rc = cell(c);
        match v.grid[c] {
            g if g < -1 => {
                p.rect_filled(rc, 4.0, Color32::from_rgb(18, 20, 26));
                let s = egui::Stroke::new(1.5, Color32::from_rgb(60, 62, 70));
                p.line_segment([rc.left_top() + vec2(8.0, 8.0), rc.right_bottom() - vec2(8.0, 8.0)], s);
                p.line_segment([rc.right_top() + vec2(-8.0, 8.0), rc.left_bottom() + vec2(8.0, -8.0)], s);
            }
            e @ 0..=7 => {
                p.rect_filled(rc, 4.0, element_colour(e as u8).gamma_multiply(0.18));
                p.rect_stroke(rc, 4.0, egui::Stroke::new(1.0, element_colour(e as u8).gamma_multiply(0.6)), egui::StrokeKind::Inside);
                p.text(rc.center(), Align2::CENTER_CENTER, &ELEMENTS[e as usize][..1], FontId::proportional(m.theme.small), element_colour(e as u8).gamma_multiply(0.8));
            }
            _ => {
                p.rect_filled(rc, 4.0, Color32::from_rgb(34, 38, 48));
                p.rect_stroke(rc, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(58, 64, 78)), egui::StrokeKind::Inside);
            }
        }
    }
    for &(_, e, cells, bonus) in &v.placed {
        for c in (0..25).filter(|&c| on(cells, c)) {
            let fill = element_colour(e).gamma_multiply(if bonus { 0.95 } else { 0.7 });
            p.rect_filled(cell(c).shrink(3.0), 3.0, fill);
            if bonus && v.grid[c] == e as i8 {
                p.rect_stroke(cell(c).shrink(3.0), 3.0, egui::Stroke::new(2.0, m.theme.color("gold")), egui::StrokeKind::Inside);
            }
        }
    }
    if let Some((_, e, cells, carried, fits)) = v.held {
        let a = if carried { v.pulse.clamp(0.4, 1.3) / 1.3 } else { 0.6 * v.pulse.clamp(0.4, 1.3) / 1.3 };
        let edge = if !carried { m.theme.color("gold") } else if fits { Color32::from_rgb(80, 210, 200) } else { Color32::from_rgb(220, 80, 70) };
        for c in (0..25).filter(|&c| on(cells, c)) {
            let rc = cell(c).shrink(1.0).translate(if carried { vec2(-3.0, -3.0) } else { vec2(0.0, 0.0) });
            if carried {
                p.rect_filled(rc.translate(vec2(6.0, 6.0)), 3.0, Color32::from_black_alpha(90));
            }
            p.rect_filled(rc, 3.0, element_colour(e).gamma_multiply(a));
            p.rect_stroke(rc, 3.0, egui::Stroke::new(2.0, edge), egui::StrokeKind::Inside);
        }
    }
    let mut out = GridMouse::default();
    if carrying
        && let Some(at) = resp.hover_pos()
    {
        let d = at - r.min;
        out.hover = Some(((d.x / CELL).clamp(0.0, 4.0) as i8, (d.y / CELL).clamp(0.0, 4.0) as i8));
        out.click = resp.clicked();
        out.right = resp.secondary_clicked();
        let wheel = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { delta, .. } if delta.y != 0.0)));
        out.turn = resp.middle_clicked() || wheel;
    }
    out
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some(gu) = p.gu else { return };
    let Some(v) = maze::view(gu, w) else { return };
    let m = p.m;
    let free = w.confirm.is_none();
    let carrying = v.on_grid && v.held.is_some_and(|h| h.3);
    let title = gu.title_msg(0x96).unwrap_or_else(|| "Tabula".into());
    let (mut list, mut filter, mut mouse) = (None, None, GridMouse::default());
    p.panel("window", Some(Title { icon: Some("icons/items"), text: &title, note: Some(&v.tabula) }), COL_W + CELL * 5.0 + 32.0 + 44.0, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                match v.sibo {
                    Some(c) => {
                        heading(ui, m, &gu.menu_msg(0x2a4, "Narrow down list of Maze Runes by type."));
                        if free {
                            filter = filter_rows(ui, m, c);
                        }
                    }
                    None => {
                        let what = match v.mode {
                            Mode::Vouchers => "Maze Vouchers",
                            Mode::Runes => "Maze Runes",
                            Mode::Placed => "Runes on the tabula",
                        };
                        let shown = match v.filter {
                            Some(f) => format!("{what}: {}", SHAPES[f as usize % SHAPES.len()]),
                            None => what.to_string(),
                        };
                        heading(ui, m, &shown);
                        if v.list.is_empty() {
                            ui.label(egui::RichText::new("None learned.").size(m.theme.small).color(m.theme.color("muted")));
                        }
                        list = rows(ui, m, w, &v.list, v.cursor, free && !v.on_grid);
                    }
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                match &v.voucher {
                    Some((id, name)) => {
                        ui.horizontal(|ui| {
                            if let Some(tex) = w.item_info.as_ref().and_then(|info| m.item_icon(p.ctx, info, *id)) {
                                ui.image((tex, vec2(28.0, 28.0)));
                            }
                            ui.label(egui::RichText::new(name).size(m.theme.ui).color(tint(m, maze::GREEN)));
                        });
                    }
                    None => heading(ui, m, if v.edit { "Choose a Maze Voucher" } else { "" }),
                }
                ui.add_space(6.0);
                mouse = grid(ui, m, &v, free && carrying);
                ui.add_space(6.0);
                if carrying {
                    ui.label(egui::RichText::new("Arrows or the mouse move it, Num- or the wheel turns it, Enter places it, Escape drops it.").size(m.theme.small).color(m.theme.color("muted")));
                }
            });
        });
        ui.add_space(8.0);
        if let Some(t) = &v.help {
            super::gametext::label(ui, m, t, m.theme.small, m.theme.color("text"));
        } else if !v.help_line.is_empty() {
            ui.label(egui::RichText::new(&v.help_line).size(m.theme.small).color(m.theme.color("muted")));
        }
        if v.edit {
            ui.label(egui::RichText::new("Changes are sent when the tabula closes.").size(m.theme.small).color(m.theme.color("muted")));
        }
    });
    if !free {
        return;
    }
    if let Some(c) = filter {
        maze::set_sibo(w, c);
        maze::key(w, egui::Key::Enter);
        return;
    }
    if let Some(k) = list {
        maze::set_row(w, k);
        maze::key(w, egui::Key::Enter);
        return;
    }
    if let Some((c, r)) = mouse.hover {
        maze::set_cell(w, c, r);
    }
    if mouse.click {
        maze::key(w, egui::Key::Enter);
    } else if mouse.right {
        maze::key(w, egui::Key::Escape);
    } else if mouse.turn {
        maze::rotate(w);
    }
}
