//! The modern Delivery Box over client/post.rs, which keeps both state machines, every rule and key: the box's eight
//! slots as tiles, the sender and post2's choices (receive), the recipient, delivery's Send / Close / Gil and the
//! inventory picker beside the slots (send); the count and the gil asked inside the panel.

use super::super::post::{self, Look, SendPart, SlotLook};
use super::super::World;
use super::super::game_ui::GameUi;
use super::super::items::ItemCache;
use super::craft::{button, caption, count_row, list, money_row, tile};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};

const TILE: f32 = 52.0;
const GAP: f32 = 8.0;
const GRID_W: f32 = 4.0 * TILE + 3.0 * GAP;
const LIST_W: f32 = 330.0;
/// Send and Close beside the slots.
const SIDE_W: f32 = 78.0;
const POST1: &str = "menu    post1";
const POST2: &str = "menu    post2";
const DELIVERY: &str = "menu    delivery";

/// What the mouse asked this frame: a cursor move with Enter (a click), a key from a button, the gil outright.
#[derive(Default)]
struct Mouse {
    click: Option<u16>,
    key: Option<egui::Key>,
    money: Option<u32>,
}

/// The eight slots, two rows of four (child k + 1): the cursor's ring while the slots have the keys, dimmed slots
/// shaded; the slot clicked.
fn slots(ui: &mut Ui, m: &super::Modern, info: &ItemCache, salt: &str, slots: &[SlotLook; 8], cursor: Option<u16>, sense: bool) -> Option<u16> {
    let ctx = ui.ctx().clone();
    let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 2.0 * TILE + GAP), Sense::hover());
    let mut click = None;
    for (k, &(item, count, shown, dim)) in slots.iter().enumerate() {
        let r = Rect::from_min_size(area.min + vec2((k % 4) as f32 * (TILE + GAP), (k / 4) as f32 * (TILE + GAP)), vec2(TILE, TILE));
        let icon = (shown && item != 0).then(|| m.item_icon(&ctx, info, item)).flatten();
        let child = k as u16 + 1;
        let resp = tile(ui, m, r, egui::Id::unique((salt, k)), icon, if shown { count } else { 0 }, (cursor == Some(child)).then_some(true), false);
        if dim {
            ui.painter().rect_filled(r.shrink(4.0), 6.0, Color32::from_black_alpha(120));
        }
        if sense && resp.clicked() {
            click = Some(child);
        }
    }
    click
}

/// The receive side: post1's slots, the cursor slot's sender, post2's choices while it has the keys.
#[allow(clippy::too_many_arguments)]
fn receive(ui: &mut Ui, m: &super::Modern, gu: &GameUi, info: &ItemCache, look: (&[SlotLook; 8], u16, Option<u16>, bool, Option<&str>), free: bool, mouse: &mut Mouse) {
    let (s, cursor, post2, no_return, sender) = look;
    let on_slots = free && post2.is_none();
    let got = slots(ui, m, info, "post1", s, on_slots.then_some(cursor), on_slots);
    mouse.click = got;
    ui.add_space(8.0);
    if let Some(name) = sender {
        let label = gu.item_msg(9, 0x33).unwrap_or_else(|| "Sender:".into());
        ui.label(egui::RichText::new(format!("{label} {name}")).size(m.theme.ui).color(m.theme.color("gold")));
    }
    if let Some(p2) = post2 {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (k, d) in [(1u16, "Receive"), (2, "Dispose"), (3, "Return")] {
                let off = k == 3 && no_return;
                if button(ui, m, &caption(gu, POST2, k, d), 110.0, p2 == k, off) && free {
                    mouse.click = Some(k);
                }
            }
        });
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    let Some(v) = post::look(w) else { return };
    let m = p.m;
    let ctx = p.ctx;
    let free = w.confirm.is_none();
    let mut mouse = Mouse::default();
    let gil = crate::inventory::gil_text(w.inv.gil() as i64);
    match &v {
        Look::Recv { slots: s, cursor, post2, no_return, sender, busy } => {
            let title = gu.focus_text(POST1, 0)[1].clone().unwrap_or_else(|| "Delivery Box".into());
            p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: Some(&gil) }), GRID_W + 44.0 + 120.0, |ui| {
                receive(ui, m, gu, &info, (s, *cursor, *post2, *no_return, sender.as_deref()), free && !busy, &mut mouse);
                if *busy {
                    ui.label(egui::RichText::new("Please wait...").size(m.theme.small).color(m.theme.color("muted")));
                }
            });
        }
        Look::Send { slots: s, cursor, part, cancel, label, text, editing, shown, dl_cursor, dl_mode, rows, tints, row, qty, money, busy } => {
            let title = gu.title_msg(if *cancel { 0x11 } else { 0x10 }).unwrap_or_else(|| "Send".into());
            let wide = *part == SendPart::List;
            let width = GRID_W + GAP + SIDE_W + 44.0 + if wide { LIST_W + 16.0 } else { 0.0 };
            let can = free && (!busy || *part == SendPart::Recipient);
            p.panel("window", Some(Title { icon: Some("icons/mog_house"), text: &title, note: Some(&gil) }), width, |ui| {
                // stringdl: the recipient, its field (child 1) and two buttons (keytops3 53 / 56 and 59).
                ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("gold")));
                ui.horizontal(|ui| {
                    let on = *part == SendPart::Recipient;
                    let name = match shown {
                        Some(n) => n.clone(),
                        None if *editing => {
                            let blink = (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                            format!("{text}{}", if blink { "|" } else { " " })
                        }
                        None => text.clone(),
                    };
                    let (r, resp) = ui.allocate_exact_size(vec2(GRID_W, 28.0), Sense::click());
                    let face = if on && *dl_cursor == 1 { "controls/list_row_selected" } else { "controls/list_row_normal" };
                    super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                    ui.painter().text(pos2(r.min.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), m.theme.color("text"));
                    if can && on && resp.clicked() {
                        mouse.click = Some(1);
                    }
                });
                // The buttons under the field, as retail's (Down from the field reaches them).
                ui.horizontal(|ui| {
                    let on = *part == SendPart::Recipient;
                    let first = if *dl_mode == 2 { 56 } else { 53 };
                    for (k, sprite, d) in [(2u16, first, "OK"), (3, 59, "Cancel")] {
                        let t = m.caption(gu, "menu    keytops3", sprite).unwrap_or_else(|| d.to_string());
                        if button(ui, m, &t, 90.0, on && *dl_cursor == k, !on) && can {
                            mouse.click = Some(k);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    // delivery as retail's: the gil (child 11) over the slots, Send (9) and Close (10) right of their
                    // rows.
                    ui.vertical(|ui| {
                        ui.set_width(GRID_W + GAP + SIDE_W);
                        let on = *part == SendPart::Delivery;
                        let mut pressed = None;
                        let press = |ui: &mut Ui, k: u16, d: &str, w: f32, off: bool, pressed: &mut Option<u16>| {
                            if button(ui, m, &caption(gu, DELIVERY, k, d), w, on && *cursor == k, off || !on) && can && on {
                                *pressed = Some(k);
                            }
                        };
                        press(ui, 11, "Gil", GRID_W, *cancel, &mut pressed);
                        ui.add_space(6.0);
                        ui.horizontal_top(|ui| {
                            let got = slots(ui, m, &info, "delivery", s, on.then_some(*cursor), can && on);
                            if on {
                                mouse.click = got.or(mouse.click);
                            }
                            ui.add_space(GAP);
                            ui.vertical(|ui| {
                                ui.add_space((TILE - 28.0) / 2.0);
                                press(ui, 9, "Send", SIDE_W, false, &mut pressed);
                                ui.add_space(TILE + GAP - 28.0);
                                press(ui, 10, "Close", SIDE_W, false, &mut pressed);
                            });
                        });
                        mouse.click = mouse.click.or(pressed);
                    });
                    if wide {
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.set_width(LIST_W);
                            let got = list(ui, m, ctx, &info, "delivery picker", rows, tints, *row, can, &[]);
                            mouse.click = mouse.click.or(got.map(|k| k as u16));
                        });
                    }
                });
                if let Some((n, max)) = qty {
                    ui.add_space(8.0);
                    mouse.key = count_row(ui, m, gu, &gu.title_msg(0x3e).unwrap_or_default(), *n, *max);
                }
                if let Some(mv) = money {
                    ui.add_space(8.0);
                    (mouse.key, mouse.money) = money_row(ui, m, gu, *mv, None);
                }
            });
        }
    }
    if !free {
        return;
    }
    let cursor = |w: &mut World| w.sounds.push(Se::Cursor as u32);
    if let Some(v) = mouse.money {
        post::set_money(w, v);
        cursor(w);
    }
    if let Some(k) = mouse.key {
        post::key(w, gu, k);
        return;
    }
    if let Some(k) = mouse.click {
        post::set_cursor(w, k);
        post::key(w, gu, egui::Key::Enter);
    }
}
