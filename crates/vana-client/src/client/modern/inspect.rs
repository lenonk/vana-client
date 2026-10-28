//! The modern /check window over client/inspect.rs, which keeps its store, rules and keys: the checked player's name,
//! jobs and linkshell, the sixteen slots in retail's grid (child 1 + column x 4 + row, so its links walk it as drawn),
//! View Wares, and the bazaar message while View Wares has the cursor. The item info follows the cursor as retail's.
//! The mannequin's dressing (mannequin.rs, layout mode 4) uses the same grid with Remove All and the Storage list.

use super::super::{World, inspect, linkshell, mannequin, textcmd};
use super::craft::{button, list, tile};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use crate::inventory::SLOT_CHILD;
use egui::{Color32, Rect, Sense, pos2, vec2};

const SLOT: f32 = 52.0;
const GAP: f32 = 4.0;
const PITCH: f32 = SLOT + GAP;
const GRID_W: f32 = 4.0 * SLOT + 3.0 * GAP;

pub fn draw(p: &mut Panels, w: &mut World) {
    // The bazaar list over it suspends it.
    if !w.modern || super::super::shop::wares_open(w) {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    let Some(cursor) = w.inspect.win.as_ref().map(|x| x.cursor) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let name = textcmd::display_name(w, w.inspect.expect.0);
    // The help bar's line (inspect::bars): "Lv.75 Ninja / Lv.37 Thief", or ? ? ? when anonymous.
    let mut bars = super::super::hudtop::BarText::default();
    inspect::bars(gu, w, &mut bars);
    let jobs = bars.help[2].clone();
    let (view, enabled) = inspect::view_wares(w);
    let ls = w.inspect.info.as_ref().filter(|g| g.ls_item != 0).map(|g| (linkshell::ls_name(&g.ls_name), g.ls_item, g.ls_colour));
    let (mut hover, mut click) = (None, None);
    p.panel("check", Some(Title { icon: Some("icons/find"), text: &name, note: None }), GRID_W + 44.0, |ui| {
        if !jobs.is_empty() {
            ui.label(egui::RichText::new(&jobs).size(m.theme.ui).color(m.theme.color("gold")));
            ui.add_space(6.0);
        }
        let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 4.0 * PITCH - GAP), Sense::hover());
        for (slot, &child) in SLOT_CHILD.iter().enumerate() {
            let (col, row) = ((child - 1) / 4, (child - 1) % 4);
            let r = Rect::from_min_size(area.min + vec2(col as f32 * PITCH, row as f32 * PITCH), vec2(SLOT, SLOT));
            let icon = w.inspect.slots[slot].and_then(|(item, _)| m.item_icon(ctx, &info, item));
            let resp = tile(ui, m, r, egui::Id::unique(("check slot", slot)), icon, 0, (cursor == child).then_some(true), false);
            if free && resp.hovered() && moved {
                hover = Some(child);
            }
            if free && resp.clicked() {
                click = Some(child);
            }
        }
        if let Some((ls_name, item, colour)) = &ls
            && !ls_name.is_empty()
        {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(t) = m.item_icon(ctx, &info, *item) {
                    let c = *colour as u32;
                    let ch = |s: u32| (((c >> s) & 0xf) * 13 + 0x38).min(255) as f32 / 128.0;
                    let tint = Color32::from_rgb((ch(0) * 255.0).min(255.0) as u8, (ch(4) * 255.0).min(255.0) as u8, (ch(8) * 255.0).min(255.0) as u8);
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                    ui.painter().image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
                }
                ui.label(egui::RichText::new(ls_name).size(m.theme.ui).color(m.theme.color("text")));
            });
        }
        ui.add_space(8.0);
        if button(ui, m, "View Wares", GRID_W, cursor == view, !enabled) && free {
            click = Some(view);
        }
        if cursor == view {
            let lines: Vec<&String> = w.inspect.comment.iter().filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() {
                ui.add_space(6.0);
                for l in lines {
                    ui.label(egui::RichText::new(l).size(m.theme.small).color(m.theme.color("text")));
                }
            }
        }
    });
    if let Some(k) = hover
        && inspect::set_cursor(w, k)
    {
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(k) = click {
        inspect::set_cursor(w, k);
        inspect::key(w, gu, egui::Key::Enter);
    }
}

/// Dressing a mannequin: the grid (the worn pieces, the slots it can't wear shaded), Remove All under it (dim with
/// nothing worn), the Storage list beside it in its tints (picked orange, worn here green, elsewhere or the wrong race
/// grey); the side without the keys takes them on the mouse first.
pub fn dress(p: &mut Panels, w: &mut World) {
    const LIST_W: f32 = 330.0;
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    let Some(v) = mannequin::look(w) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let free = w.confirm.is_none();
    let title = gu.title_msg(4).filter(|t| !t.is_empty()).unwrap_or_else(|| "Layout".into());
    let (mut grid, mut rows) = ((None, None), (None, None));
    p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: None }), GRID_W + 16.0 + LIST_W + 44.0, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(GRID_W);
                let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 4.0 * PITCH - GAP), Sense::hover());
                for child in 1..=16u16 {
                    let (col, row) = ((child - 1) / 4, (child - 1) % 4);
                    let r = Rect::from_min_size(area.min + vec2(col as f32 * PITCH, row as f32 * PITCH), vec2(SLOT, SLOT));
                    let item = v.worn[child as usize - 1];
                    let icon = (item != 0).then(|| m.item_icon(ctx, &info, item)).flatten();
                    let lit = (v.cursor == child).then_some(v.on_grid);
                    let resp = tile(ui, m, r, egui::Id::unique(("dress slot", child)), icon, 0, lit, false);
                    if v.off.contains(&child) {
                        ui.painter().rect_filled(r.shrink(4.0), 6.0, Color32::from_black_alpha(140));
                    } else {
                        if free && resp.hovered() && moved {
                            grid.0 = Some(child);
                        }
                        if free && resp.clicked() {
                            grid.1 = Some(child);
                        }
                    }
                }
                ui.add_space(8.0);
                let label = gu.sprite_caption("menu    windowps", 545).unwrap_or_else(|| "Remove all".into());
                if button(ui, m, &label, GRID_W, v.on_grid && v.cursor == mannequin::REMOVE, !v.any_worn) && free {
                    grid.1 = Some(mannequin::REMOVE);
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                rows = list(ui, m, ctx, &info, "dress storage", &v.rows, &v.tints, v.row, free && !v.on_grid, &[]);
            });
        });
    });
    if !free {
        return;
    }
    let cursor = |w: &mut World| w.sounds.push(Se::Cursor as u32);
    if let Some(k) = grid.0
        && mannequin::set_grid(w, k)
    {
        cursor(w);
    }
    if let Some(k) = grid.1 {
        mannequin::set_grid(w, k);
        mannequin::key(w, gu, egui::Key::Enter);
        return;
    }
    if let Some(k) = rows.0
        && mannequin::set_list(w, k)
    {
        cursor(w);
    }
    if let Some(k) = rows.1 {
        mannequin::set_list(w, k);
        mannequin::key(w, gu, egui::Key::Enter);
    }
}
