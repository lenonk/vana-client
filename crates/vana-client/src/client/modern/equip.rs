//! The modern Equipment screen over the retail one's state (client/equipwin.rs keeps the modes, the grid walk, the
//! list's filters, mnstorag and every rule): the persona, the sixteen slots with Attack, Defence and the eight
//! resistances, gil; the list of gear in a container with its Storage Options row and mnstorag. The mouse acts as
//! retail's: the pointer over the other window is Num+, hovering moves the cursor, a click on it is Enter.

use super::super::equipwin::{self, Focus, STORE_BAGS};
use super::super::items::{self as gitems, JOBS};
use super::super::scrollbar::Msg;
use super::super::World;
use super::{Panels, Title, chat, paint};
use crate::inventory::{SLOT_CHILD, VISIBLE_ROWS, gil_text};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const SLOT: f32 = 52.0;
const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
/// The resistances in s2c 0x061's order (def_elem).
const ELEMENTS: [(&str, &str); 8] = [("icons/fire", "Fire"), ("icons/ice", "Ice"), ("icons/wind", "Wind"), ("icons/earth", "Earth"), ("icons/lightning", "Lightning"), ("icons/water", "Water"), ("icons/light", "Light"), ("icons/dark", "Dark")];
const STATS: [&str; 7] = ["STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR"];

/// What the mouse did: the panel the pointer moved over (true: the list), a grid child or list row hovered or clicked,
/// the wheel.
#[derive(Default)]
struct Mouse {
    over: Option<bool>,
    grid_hover: Option<u16>,
    grid_click: Option<u16>,
    row_hover: Option<usize>,
    row_click: Option<usize>,
    wheel: f32,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(v) = equipwin::view(w) else { return };
    let Some(info) = w.item_info.clone() else { return };
    let m = p.m;
    let moved = p.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let mut mouse = Mouse::default();
    let persona = gitems::persona(w);
    let gil = gil_text(w.inv.gil() as i64);
    let worn: Vec<Option<(u16, u8)>> = w.inv.equip.iter().map(|e| e.and_then(|(c, i)| Some((w.inv.slots.get(&(c, i))?.0, c)))).collect();
    let icons: Vec<Option<egui::TextureId>> = worn.iter().map(|x| x.and_then(|(id, _)| m.item_icon(p.ctx, &info, id))).collect();
    let enc: Vec<bool> = (0..16).map(|s| equipwin::encumbered(w, s)).collect();
    let stats = w.stats.clone().unwrap_or_default();
    let grid_on = v.focus == Focus::Grid;
    let rect = p.panel("equip", Some(Title { icon: None, text: "Equipment", note: Some(&gil) }), 480.0, |ui| {
        // The persona: name, main / sub job, item level; HP, MP, TP.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&persona.name).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("text")));
            let job = |j: &Option<(String, u8)>| j.as_ref().map_or(String::new(), |(lv, job)| format!("{} {}", lv.trim_end(), JOBS.get(*job as usize).map_or("", |s| s)));
            let main = egui::RichText::new(job(&persona.main)).size(m.theme.ui).color(if persona.main_blue { m.theme.color("focus") } else { m.theme.color("gold") });
            ui.label(main);
            if persona.sub.is_some() {
                ui.label(egui::RichText::new(format!("/ {}", job(&persona.sub))).size(m.theme.ui).color(m.theme.color("focus")));
            }
            if persona.ready && !persona.ilvl.starts_with("---") {
                ui.label(egui::RichText::new(format!("iLv {}", persona.ilvl.trim_end_matches('/').trim())).size(m.theme.small).color(m.theme.color("muted")));
            }
        });
        if persona.ready {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (label, value, c) in [("HP", format!("{}{}", persona.hp.0.trim(), persona.hp.1), "hp"), ("MP", format!("{}{}", persona.mp.0.trim(), persona.mp.1), "mp"), ("TP", persona.tp.trim().to_string(), "tp")] {
                    ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color(c)));
                    ui.label(egui::RichText::new(value).size(m.theme.ui).color(m.theme.color("text")));
                }
            });
        }
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            // The slots in the grid's order (child 1 + col * 4 + row).
            let (area, _) = ui.allocate_exact_size(vec2(SLOT * 4.0 + 12.0, SLOT * 4.0 + 12.0), Sense::hover());
            for (slot, &child) in SLOT_CHILD.iter().enumerate() {
                let (col, row) = ((child - 1) / 4, (child - 1) % 4);
                let r = Rect::from_min_size(area.min + vec2(col as f32 * (SLOT + 4.0), row as f32 * (SLOT + 4.0)), vec2(SLOT, SLOT));
                let resp = ui.interact(r, egui::Id::unique(("equip slot", slot)), Sense::click());
                let allowed = v.mode == 1 && v.picked.is_some() && v.allowed & 1 << slot != 0;
                let face = match () {
                    _ if grid_on && v.grid == child => "controls/item_slot_selected",
                    _ if enc[slot] => "controls/item_slot_locked",
                    _ if allowed => "controls/item_slot_drop_valid",
                    _ if resp.hovered() && grid_on => "controls/item_slot_hover",
                    _ => "controls/item_slot_normal",
                };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                }
                // The cursor's ring, as the list's selected row stands out.
                if grid_on && v.grid == child {
                    ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, m.theme.color("focus")), egui::StrokeKind::Inside);
                }
                if let Some(t) = icons[slot] {
                    ui.painter().image(t, r.shrink(10.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                // A wardrobe piece's number at the slot's lower right; an encumbered slot's red cross.
                let n = worn[slot].map_or(0, |(_, c)| equipwin::wardrobe(c));
                if n > 0 {
                    paint::outlined(ui.painter(), r.max - vec2(6.0, 4.0), Align2::RIGHT_BOTTOM, &n.to_string(), FontId::proportional(m.theme.small), m.theme.color("gold"));
                }
                if enc[slot] {
                    let c = r.shrink(14.0);
                    let s = egui::Stroke::new(2.5, m.theme.color("hp"));
                    ui.painter().line_segment([c.left_top(), c.right_bottom()], s);
                    ui.painter().line_segment([c.right_top(), c.left_bottom()], s);
                }
                if resp.hovered() && moved {
                    mouse.grid_hover = Some(child);
                }
                if resp.clicked() {
                    mouse.grid_click = Some(child);
                }
            }
            if ui.rect_contains_pointer(area) && moved {
                mouse.over = Some(false);
            }
            ui.add_space(14.0);
            // Attack and Defence, the resistances, STR..CHR with their bonuses.
            ui.vertical(|ui| {
                let pair = |ui: &mut Ui, label: &str, value: String, c: Color32| {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(48.0, 18.0), egui::Label::new(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("muted"))));
                        ui.label(egui::RichText::new(value).size(m.theme.ui).color(c));
                    });
                };
                pair(ui, "Attack", stats.atk.to_string(), m.theme.color("text"));
                pair(ui, "Defense", stats.def.to_string(), m.theme.color("text"));
                ui.add_space(4.0);
                egui::Grid::new("equip elements").spacing(vec2(10.0, 3.0)).show(ui, |ui| {
                    for (k, (icon, name)) in ELEMENTS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            if let Some(s) = m.art.sprite(icon) {
                                ui.painter().add(paint::image(&s, r.min, 18.0, Color32::WHITE));
                            }
                            resp.on_hover_text(*name);
                            ui.label(egui::RichText::new(stats.elem[k].to_string()).size(m.theme.small).color(m.theme.color("text")));
                        });
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                if persona.ready {
                    ui.add_space(4.0);
                    for (k, (base, bonus)) in persona.stats.iter().enumerate() {
                        let red = persona.enc & 1 << (0x10 + k) != 0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.add_sized(vec2(32.0, 16.0), egui::Label::new(egui::RichText::new(STATS[k]).size(m.theme.small).color(m.theme.color("muted"))));
                            ui.label(egui::RichText::new(base.trim()).size(m.theme.small).color(if red { m.theme.color("hp") } else { m.theme.color("text") }));
                            if let Some((b, c)) = bonus {
                                ui.label(egui::RichText::new(b.trim()).size(m.theme.small).color(chat::colour(c.to_le_bytes())));
                            }
                        });
                    }
                }
            });
        });
    });
    let _ = rect;
    list(p, w, &v, &info, moved, &mut mouse);
    apply(w, &v, mouse);
}

/// The container's name for the list's title.
fn bag_name(c: u8) -> String {
    match equipwin::wardrobe(c) {
        0 => "Inventory".into(),
        1 => "Wardrobe".into(),
        n => format!("Wardrobe {n}"),
    }
}

/// The list: the Storage Options row then the container's gear in the screen's colours (white wearable, green worn,
/// orange picked, grey otherwise); mnstorag's nine bags in its place while it has the focus.
fn list(p: &mut Panels, w: &World, v: &equipwin::View, info: &std::sync::Arc<super::super::items::ItemCache>, moved: bool, mouse: &mut Mouse) {
    let m = p.m;
    let list_on = v.focus == Focus::List;
    let icons: Vec<Option<egui::TextureId>> = v.rows.iter().map(|(s, _)| s.id).skip(v.scroll.saturating_sub(1)).take(VISIBLE_ROWS).map(|id| m.item_icon(p.ctx, info, id)).collect();
    let names: Vec<String> = v.rows.iter().map(|(s, _)| w.item_info.as_ref().and_then(|c| c.item(s.id)).map(|it| it.name.clone()).unwrap_or_default()).collect();
    let title = bag_name(v.container);
    let n = v.rows.len() + 1;
    p.panel("equiplist", Some(Title { icon: None, text: &title, note: None }), 380.0, |ui| {
        if let Focus::Storage(o) = v.focus {
            for (k, &bag) in STORE_BAGS.iter().enumerate() {
                let child = k as u16 + 1;
                let off = v.store_off.contains(&child);
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let face = match () {
                    _ if o == child => "controls/list_row_selected",
                    _ if off => "controls/list_row_disabled",
                    _ if resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                }
                let c = if off { m.theme.color("muted") } else { m.theme.color("text") };
                ui.painter().text(pos2(r.min.x + 14.0, r.center().y), Align2::LEFT_CENTER, bag_name(bag), FontId::proportional(m.theme.ui), c);
                if resp.hovered() && moved {
                    mouse.row_hover = Some(k);
                }
                if resp.clicked() {
                    mouse.row_click = Some(k);
                }
                ui.add_space(2.0);
            }
            return;
        }
        let top = ui.cursor().min;
        for k in 0..VISIBLE_ROWS.min(n - v.scroll) {
            let at = v.scroll + k;
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let face = match () {
                _ if list_on && k == v.cursor => "controls/list_row_selected",
                _ if list_on && resp.hovered() => "controls/list_row_hover",
                _ => "controls/list_row_normal",
            };
            if let Some(s) = m.art.sprite(face) {
                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
            }
            if at == 0 {
                // Row 0: Storage Options, with the container's wardrobe number.
                if let Some(s) = m.art.sprite("icons/wardrobe") {
                    ui.painter().add(paint::image(&s, pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), ICON, Color32::WHITE));
                }
                ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, "Storage Options", FontId::proportional(m.theme.ui), m.theme.color("gold"));
            } else {
                let (_, tint) = v.rows[at - 1];
                if let Some(t) = icons.get(k - usize::from(v.scroll == 0)).copied().flatten() {
                    ui.painter().image(t, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let c = if tint == 0x8080_8080 { m.theme.color("text") } else { chat::colour(tint.to_le_bytes()) };
                ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, &names[at - 1], FontId::proportional(m.theme.ui), c);
            }
            if resp.hovered() && moved {
                mouse.row_hover = Some(k);
            }
            if resp.clicked() {
                mouse.row_click = Some(k);
            }
            ui.add_space(2.0);
        }
        if n > VISIBLE_ROWS {
            let track = Rect::from_min_max(pos2(ui.max_rect().max.x + 8.0, top.y), pos2(ui.max_rect().max.x + 12.0, ui.cursor().min.y));
            ui.painter().rect_filled(track, 2.0, m.theme.color("charcoal"));
            let h = track.height() * VISIBLE_ROWS as f32 / n as f32;
            let y = track.min.y + (track.height() - h) * v.scroll as f32 / (n - VISIBLE_ROWS) as f32;
            ui.painter().rect_filled(Rect::from_min_size(pos2(track.min.x, y), vec2(track.width(), h)), 2.0, m.theme.color("bronze"));
        }
        if ui.rect_contains_pointer(ui.max_rect()) {
            if moved {
                mouse.over = Some(true);
            }
            mouse.wheel = ui.input(|i| {
                i.events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y,
                        egui::Event::MouseWheel { delta, .. } => delta.y / 50.0,
                        _ => 0.0,
                    })
                    .sum::<f32>()
            });
        }
    });
}

fn enter(w: &mut World) {
    gitems::key(w, egui::Key::Enter, false, false);
}

/// The mouse through retail's own rules: over the other window it is Num+ (FUN_10132c30); on the focused one hovering
/// moves the cursor and a click on it is Enter.
fn apply(w: &mut World, v: &equipwin::View, mouse: Mouse) {
    if let Focus::Storage(o) = v.focus {
        if let Some(k) = mouse.row_hover {
            equipwin::point_store(w, k as u16 + 1);
        }
        if let Some(k) = mouse.row_click {
            if o == k as u16 + 1 {
                enter(w);
            } else {
                equipwin::point_store(w, k as u16 + 1);
            }
        }
        return;
    }
    match (mouse.over, v.focus) {
        (Some(true), Focus::Grid) | (Some(false), Focus::List) => {
            equipwin::num_plus(w);
            return;
        }
        _ => {}
    }
    if v.focus == Focus::Grid {
        if let Some(c) = mouse.grid_hover {
            equipwin::point_grid(w, c);
        }
        if let Some(c) = mouse.grid_click
            && !equipwin::point_grid(w, c)
            && equipwin::view(w).is_some_and(|x| x.grid == c)
        {
            enter(w);
        }
        return;
    }
    if let Some(k) = mouse.row_hover {
        equipwin::point_row(w, k);
    }
    if let Some(k) = mouse.row_click
        && !equipwin::point_row(w, k)
        && equipwin::view(w).is_some_and(|x| x.cursor == k)
    {
        enter(w);
    }
    if mouse.wheel != 0.0 {
        let msg = if mouse.wheel > 0.0 { Msg::Up } else { Msg::Down };
        for _ in 0..(mouse.wheel.abs().round() as usize).clamp(1, 5) {
            equipwin::bar(w, msg);
        }
    }
}
