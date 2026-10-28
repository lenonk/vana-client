//! The modern Equipment screen in the command window (client/equipwin.rs keeps the state and every rule): the
//! persona; the sixteen slots, the eight resistances under them and the stats at the bottom, in the slots' columns;
//! beside them, after Enter or a click on a slot, every piece the character can wear there from every bag, with where
//! it is. Drag and drop: a listed piece onto a slot puts it on; a worn piece onto another slot moves it, onto the list
//! takes it off. Nothing closes or resets on the way.

use super::super::World;
use super::super::equipwin::{self, Candidate, Focus, MODERN_ROWS, Reach};
use super::super::items as gitems;
use super::super::scrollbar::Msg;
use super::{Panels, Title, chat, paint};
use crate::inventory::{SLOT_CHILD, gil_text};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const SLOT: f32 = 52.0;
const GAP: f32 = 4.0;
const PITCH: f32 = SLOT + GAP;
const GRID_W: f32 = 4.0 * SLOT + 3.0 * GAP;
const LIST_W: f32 = 340.0;
const ROW: f32 = 34.0;
const ICON: f32 = 24.0;
/// The resistances in s2c 0x061's order (def_elem).
const ELEMENTS: [&str; 8] = ["icons/fire", "icons/ice", "icons/wind", "icons/earth", "icons/lightning", "icons/water", "icons/light", "icons/dark"];
const STATS: [&str; 7] = ["STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR"];
/// The equipment slots' names by slot.
const SLOTS: [&str; 16] = ["Main", "Sub", "Range", "Ammo", "Head", "Body", "Hands", "Legs", "Feet", "Neck", "Waist", "Ear 1", "Ear 2", "Ring 1", "Ring 2", "Back"];

/// What is dragged: a listed piece, or the piece worn in a slot.
#[derive(Clone)]
enum Gear {
    Row(Candidate),
    Worn(usize),
}

/// What the mouse did: a slot clicked, a list row hovered or clicked, a drop, the wheel.
#[derive(Default)]
struct Mouse {
    slot_click: Option<u16>,
    row_hover: Option<usize>,
    row_click: Option<usize>,
    drop: Option<(Gear, Option<usize>)>,
    wheel: f32,
}

/// A bag's name for the list (the wardrobes are one Mog Wardrobe, the safes one Mog Safe).
fn bag(c: u8) -> &'static str {
    match c {
        0 => "Inventory",
        1 | 9 => "Mog Safe",
        2 => "Storage",
        4 => "Mog Locker",
        5 => "Mog Satchel",
        6 => "Mog Sack",
        7 => "Mog Case",
        8 | 10..=16 => "Mog Wardrobe",
        _ => "",
    }
}

/// A job and its level as players write it: "WAR22" (none without a job).
fn job((j, lv): (u8, u8)) -> Option<String> {
    (j != 0).then(|| format!("{}{lv}", super::hud::job(j)))
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let Some(v) = equipwin::view(w) else { return };
    let Some(info) = w.item_info.clone() else { return };
    let m = p.m;
    let ctx = p.ctx;
    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let persona = gitems::persona(w);
    let gil = gil_text(w.inv.gil() as i64);
    let worn: Vec<Option<(u16, u8)>> = w.inv.equip.iter().map(|e| e.and_then(|(c, i)| Some((w.inv.slots.get(&(c, i))?.0, c)))).collect();
    let icons: Vec<Option<egui::TextureId>> = worn.iter().map(|x| x.and_then(|(id, _)| m.item_icon(ctx, &info, id))).collect();
    let row_icons: Vec<Option<egui::TextureId>> = v.rows.iter().skip(v.scroll).take(MODERN_ROWS).map(|x| m.item_icon(ctx, &info, x.slot.id)).collect();
    let names: Vec<String> = v.rows.iter().skip(v.scroll).take(MODERN_ROWS).map(|x| info.item(x.slot.id).map(|it| it.name.clone()).unwrap_or_default()).collect();
    let fits = |id: u16, slot: usize| info.item(id).is_some_and(|it| it.slots & (1 << slot) != 0);
    let enc: Vec<bool> = (0..16).map(|s| equipwin::encumbered(w, s)).collect();
    let stats = w.stats.clone().unwrap_or_default();
    let room = gitems::has_free(w, gitems::INVENTORY);
    let held = egui::DragAndDrop::payload::<Gear>(ctx);
    let held_id = held.as_deref().and_then(|g| match g {
        Gear::Row(x) => Some(x.slot.id),
        Gear::Worn(s) => worn[*s].map(|(id, _)| id),
    });
    let mono = |size: f32| FontId::monospace(size);
    let mut mouse = Mouse::default();
    let title = Title { icon: None, text: "Equipment", note: Some(&gil) };
    p.panel("window", Some(title), GRID_W + 18.0 + LIST_W, |ui| {
        persona_row(ui, m, &persona, &stats);
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(GRID_W);
                // The slots in the grid's order (child 1 + col * 4 + row).
                let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 4.0 * PITCH - GAP), Sense::hover());
                for (slot, &child) in SLOT_CHILD.iter().enumerate() {
                    let (col, row) = ((child - 1) / 4, (child - 1) % 4);
                    let r = Rect::from_min_size(area.min + vec2(col as f32 * PITCH, row as f32 * PITCH), vec2(SLOT, SLOT));
                    let sense = if worn[slot].is_some() { Sense::click_and_drag() } else { Sense::click() };
                    let resp = ui.interact(r, egui::Id::unique(("equip slot", slot)), sense);
                    if worn[slot].is_some() {
                        resp.dnd_set_drag_payload(Gear::Worn(slot));
                    }
                    let over = resp.contains_pointer() && held.is_some();
                    let face = match () {
                        _ if held_id.is_some_and(|id| fits(id, slot)) => "controls/item_slot_drop_valid",
                        _ if over => "controls/item_slot_drop_invalid",
                        _ if enc[slot] => "controls/item_slot_locked",
                        _ if v.grid == child => "controls/item_slot_selected",
                        _ if resp.hovered() => "controls/item_slot_hover",
                        _ => "controls/item_slot_normal",
                    };
                    if let Some(s) = m.art.sprite(face) {
                        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                    }
                    // The cursor's ring; dimmer while the list has the focus (the slot it lists for).
                    if v.grid == child {
                        let c = if v.focus == Focus::Grid { m.theme.color("focus") } else { m.theme.color("bronze") };
                        ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, c), egui::StrokeKind::Inside);
                    }
                    if let Some(t) = icons[slot] {
                        ui.painter().image(t, r.shrink(10.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    if enc[slot] {
                        let c = r.shrink(14.0);
                        let s = egui::Stroke::new(2.5, m.theme.color("hp"));
                        ui.painter().line_segment([c.left_top(), c.right_bottom()], s);
                        ui.painter().line_segment([c.right_top(), c.left_bottom()], s);
                    }
                    if resp.clicked() {
                        mouse.slot_click = Some(child);
                    }
                    if let Some(g) = resp.dnd_release_payload::<Gear>() {
                        mouse.drop = Some(((*g).clone(), Some(slot)));
                    }
                    m.tip(ui, resp.rect, SLOTS[slot], "");
                }
                ui.add_space(10.0);
                // The resistances in the slots' columns, two rows of four.
                let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 2.0 * 26.0), Sense::hover());
                for (k, icon) in ELEMENTS.iter().enumerate() {
                    let r = Rect::from_min_size(area.min + vec2((k % 4) as f32 * PITCH, (k / 4) as f32 * 26.0), vec2(SLOT, 22.0));
                    ui.painter().rect_filled(r, 4.0, m.theme.color("charcoal").gamma_multiply(0.7));
                    if let Some(s) = m.art.sprite(icon) {
                        ui.painter().add(paint::image(&s, pos2(r.min.x + 2.0, r.min.y + 1.0), 20.0, Color32::WHITE));
                    }
                    ui.painter().text(pos2(r.max.x - 5.0, r.center().y), Align2::RIGHT_CENTER, stats.elem[k].to_string(), mono(m.theme.small), m.theme.color("text"));
                }
                ui.add_space(10.0);
                // The stats at the bottom: Attack and Defense two columns each, STR..CHR one each.
                let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 26.0 + 2.0 * 42.0), Sense::hover());
                for (k, (label, value)) in [("Attack", stats.atk), ("Defense", stats.def)].into_iter().enumerate() {
                    let r = Rect::from_min_size(area.min + vec2(k as f32 * 2.0 * PITCH, 0.0), vec2(2.0 * SLOT + GAP, 22.0));
                    ui.painter().rect_filled(r, 4.0, m.theme.color("charcoal").gamma_multiply(0.7));
                    ui.painter().text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, label, mono(m.theme.small), m.theme.color("muted"));
                    ui.painter().text(pos2(r.max.x - 6.0, r.center().y), Align2::RIGHT_CENTER, value.to_string(), mono(m.theme.small), m.theme.color("text"));
                }
                if persona.ready {
                    for (k, (base, bonus)) in persona.stats.iter().enumerate() {
                        let r = Rect::from_min_size(area.min + vec2((k % 4) as f32 * PITCH, 26.0 + (k / 4) as f32 * 42.0), vec2(SLOT, 38.0));
                        ui.painter().rect_filled(r, 4.0, m.theme.color("charcoal").gamma_multiply(0.7));
                        ui.painter().text(pos2(r.center().x, r.min.y + 3.0), Align2::CENTER_TOP, STATS[k], mono(m.theme.small - 1.0), m.theme.color("muted"));
                        // Red with retail's encumbrance marker bits; the bonus green or red as the persona has it.
                        let red = persona.enc & 1 << (0x10 + k) != 0;
                        let mut job = egui::text::LayoutJob::default();
                        let base_c = if red { m.theme.color("hp") } else { m.theme.color("text") };
                        job.append(base.trim(), 0.0, egui::TextFormat::simple(mono(m.theme.small), base_c));
                        if let Some((b, c)) = bonus {
                            job.append(b.trim(), 0.0, egui::TextFormat::simple(mono(m.theme.small), chat::colour(c.to_le_bytes())));
                        }
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, r.max.y - 4.0 - g.size().y), g, Color32::WHITE);
                    }
                }
            });
            ui.add_space(18.0);
            let col = ui.vertical(|ui| {
                ui.set_width(LIST_W);
                list(ui, m, &v, &row_icons, &names, room, moved, &mut mouse);
            });
            // The whole list column takes a worn piece dropped on it: off it comes.
            let area = Rect::from_min_max(col.response.rect.min, pos2(col.response.rect.max.x, ui.min_rect().max.y));
            if let Some(g) = ui.interact(area, egui::Id::unique("equip list drop"), Sense::hover()).dnd_release_payload::<Gear>() {
                mouse.drop = Some(((*g).clone(), None));
            }
        });
    });
    // The piece under the pointer while dragged.
    if let (Some(id), Some(at)) = (held_id, ctx.pointer_hover_pos())
        && let Some(t) = m.item_icon(ctx, &info, id)
    {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("equip drag")));
        painter.image(t, Rect::from_center_size(at, vec2(32.0, 32.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(220));
    }
    apply(w, &v, mouse, held.is_some());
}

/// The persona row: the name and jobs (WAR22/WHM11), HP / MP / TP at the right.
pub(super) fn persona_row(ui: &mut Ui, m: &super::Modern, persona: &crate::inventory::Persona, stats: &gitems::CharStats) {
    let mono = |size: f32| FontId::monospace(size);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&persona.name).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("text")));
        // WAR22/WHM11: the main in gold (blue while capped or in merit mode), the sub in blue.
        if let Some(main) = job(stats.main) {
            let mut jobs = egui::text::LayoutJob::default();
            let font = FontId::proportional(m.theme.ui);
            jobs.append(&main, 0.0, egui::TextFormat::simple(font.clone(), if persona.main_blue { m.theme.color("focus") } else { m.theme.color("gold") }));
            if let Some(sub) = job(stats.sub) {
                jobs.append(&format!("/{sub}"), 0.0, egui::TextFormat::simple(font, m.theme.color("focus")));
            }
            ui.label(jobs);
        }
        if persona.ready {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (label, value, c) in [("TP", persona.tp.trim().to_string(), "tp"), ("MP", format!("{}{}", persona.mp.0.trim(), persona.mp.1), "mp"), ("HP", format!("{}{}", persona.hp.0.trim(), persona.hp.1), "hp")] {
                    ui.label(egui::RichText::new(value).font(mono(m.theme.small)).color(m.theme.color("text")));
                    ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color(c)));
                    ui.add_space(8.0);
                }
            });
        }
    });
}

/// The slot's pieces from every bag: the icon, the name (green worn, grey out of reach) and where it is; or, before a
/// slot is picked, what to do.
fn list(ui: &mut Ui, m: &super::Modern, v: &equipwin::View, icons: &[Option<egui::TextureId>], names: &[String], room: bool, moved: bool, mouse: &mut Mouse) {
    let Some(slot) = v.list_slot else {
        ui.label(egui::RichText::new(SLOTS[equip_slot(v.grid)]).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Enter or click a slot to list everything you can wear there, from every bag. Drag a piece onto a slot to put it on, or a worn one here to take it off.").size(m.theme.small).color(m.theme.color("muted")));
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(SLOTS[slot]).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let note = if v.pending { "Moving to the inventory..." } else { "" };
            ui.label(egui::RichText::new(note).size(m.theme.small).color(m.theme.color("muted")));
            ui.label(egui::RichText::new(v.rows.len().to_string()).size(m.theme.small).color(m.theme.color("muted")));
        });
    });
    ui.add_space(4.0);
    let top = ui.cursor().min;
    let list_on = v.focus == Focus::List;
    if v.rows.is_empty() {
        ui.label(egui::RichText::new("Nothing you can wear there now.").size(m.theme.ui).color(m.theme.color("muted")));
    }
    for (k, x) in v.rows.iter().skip(v.scroll).take(MODERN_ROWS).enumerate() {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
        if x.reach != Reach::Far {
            resp.dnd_set_drag_payload(Gear::Row(x.clone()));
        }
        let face = match () {
            _ if list_on && k == v.cursor => "controls/list_row_selected",
            _ if x.reach == Reach::Far => "controls/list_row_disabled",
            _ if list_on && resp.hovered() => "controls/list_row_hover",
            _ => "controls/list_row_normal",
        };
        super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
        let dim = if x.reach == Reach::Far { Color32::from_gray(110) } else { Color32::WHITE };
        if let Some(t) = icons.get(k).copied().flatten() {
            ui.painter().image(t, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), dim);
        }
        let c = match (x.worn, x.reach) {
            (Some(_), _) => chat::colour(0x8050_9050u32.to_le_bytes()),
            (_, Reach::Far) => m.theme.color("muted"),
            _ => m.theme.color("text"),
        };
        ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, &names[k], FontId::proportional(m.theme.ui), c);
        // Where it is; out of reach, why.
        let place = match (x.worn, x.reach) {
            (Some(s), _) if s == slot => "Equipped".to_string(),
            (Some(s), _) => SLOTS[s].to_string(),
            (None, Reach::Far) if !room => format!("{} (inventory full)", bag(x.c)),
            (None, Reach::Far) if matches!(x.c, 1 | 2 | 4 | 9) => format!("{} (at the moogle)", bag(x.c)),
            (None, Reach::Far) => format!("{} (locked)", bag(x.c)),
            (None, _) => bag(x.c).to_string(),
        };
        ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, place, FontId::proportional(m.theme.small), m.theme.color("muted"));
        if resp.hovered() && moved {
            mouse.row_hover = Some(k);
        }
        if resp.clicked() {
            mouse.row_click = Some(k);
        }
        ui.add_space(2.0);
    }
    let n = v.rows.len();
    if n > MODERN_ROWS {
        let track = Rect::from_min_max(pos2(ui.max_rect().max.x + 8.0, top.y), pos2(ui.max_rect().max.x + 12.0, ui.cursor().min.y));
        ui.painter().rect_filled(track, 2.0, m.theme.color("charcoal"));
        let h = track.height() * MODERN_ROWS as f32 / n as f32;
        let y = track.min.y + (track.height() - h) * v.scroll as f32 / (n - MODERN_ROWS) as f32;
        ui.painter().rect_filled(Rect::from_min_size(pos2(track.min.x, y), vec2(track.width(), h)), 2.0, m.theme.color("bronze"));
    }
    let area = Rect::from_min_max(top, pos2(ui.max_rect().max.x, ui.cursor().min.y.max(top.y + ROW)));
    if ui.rect_contains_pointer(area) {
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
}

/// The equipment slot of a grid child.
fn equip_slot(child: u16) -> usize {
    SLOT_CHILD.iter().position(|&c| c == child).unwrap_or(0)
}

fn enter(w: &mut World) {
    gitems::key(w, egui::Key::Enter, false, false);
}

/// The mouse: a click on a slot lists its pieces; on the list hovering moves the cursor and a click on it puts the
/// piece on; drops equip, move or take off.
fn apply(w: &mut World, v: &equipwin::View, mouse: Mouse, dragging: bool) {
    if let Some((g, to)) = mouse.drop {
        match (g, to) {
            (Gear::Row(x), Some(slot)) => {
                equipwin::equip_candidate(w, slot, &x);
            }
            (Gear::Worn(from), Some(slot)) if from != slot => {
                if let Some(x) = w.inv.equip[from].and_then(|(c, i)| equipwin::candidates(w, slot).into_iter().find(|x| (x.c, x.slot.index) == (c, i))) {
                    equipwin::equip_candidate(w, slot, &x);
                }
            }
            (Gear::Worn(from), None) => equipwin::unequip(w, from),
            _ => {}
        }
        return;
    }
    if let Some(child) = mouse.slot_click {
        equipwin::open_slot(w, child);
        return;
    }
    if dragging || v.list_slot.is_none() {
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
