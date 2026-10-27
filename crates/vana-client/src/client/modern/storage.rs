//! The modern storage view in the command window, over items.rs's storage state and rules: the bags as tabs with the
//! numbered ones merged (Mog Safe and Safe 2 one Mog Safe, Mog Wardrobe 1..8 one Mog Wardrobe; user, 2026-10-26),
//! the chosen bag's items beside the inventory's. A click moves a stack across, a right click one of it; into a
//! merged bag it goes to the first part with room that takes it. Retail's colours say what may move (equipped pieces,
//! the moogle's bags away from it, non-gear into a wardrobe, installed furniture ...).

use super::super::World;
use super::super::items::{self as gitems, Storage, Win};
use super::{Panels, Title, paint};
use crate::audio::Ui as Se;
use crate::inventory::{Slot, TINT_DIM, TINT_WHITE};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
const BIN: u8 = 17;

/// The bags: name and containers (the merged ones in the order they fill).
const BAGS: [(&str, &[u8]); 9] = [
    ("Mog Safe", &[1, 9]),
    ("Storage", &[2]),
    ("Mog Locker", &[4]),
    ("Mog Satchel", &[5]),
    ("Mog Sack", &[6]),
    ("Mog Case", &[7]),
    ("Mog Wardrobe", &[8, 10, 11, 12, 13, 14, 15, 16]),
    ("Temporary", &[3]),
    ("Recycle Bin", &[BIN]),
];

fn group(c: u8) -> usize {
    BAGS.iter().position(|(_, cs)| cs.contains(&c)).unwrap_or(0)
}

fn view(w: &World) -> Option<(Storage, usize, usize)> {
    match w.win.as_ref()? {
        Win::Items { store: Some(b), scroll, cursor, .. } => Some((*b, *scroll, *cursor)),
        _ => None,
    }
}

/// The bags this view may show: the one it opened on, and while it is not read-only those retail lets it switch to.
fn bags(w: &World, b: &Storage) -> Vec<usize> {
    // From View House: the bags its menu allows (each opens as its row would).
    if w.viewhouse.is_some() {
        let open = super::super::viewhouse::open_containers(w);
        return (0..BAGS.len()).filter(|&g| BAGS[g].1.iter().any(|c| open.contains(c))).collect();
    }
    (0..BAGS.len()).filter(|&g| BAGS[g].1.contains(&b.bank) || (!b.read_only && b.bank != BIN && BAGS[g].1.iter().any(|&c| c != BIN && c != 3 && gitems::usable(w, &Storage { bank: 0xff, ..*b }, c)))).collect()
}

/// A bag's rows over its parts.
fn rows(w: &World, g: usize) -> Vec<(u8, Slot)> {
    BAGS[g].1.iter().flat_map(|&c| gitems::store_rows(w, c).into_iter().map(move |s| (c, s))).collect()
}

/// Used and room over a bag's parts.
fn fill(w: &World, g: usize) -> (usize, usize) {
    BAGS[g].1.iter().fold((0, 0), |(u, r), &c| (u + gitems::store_rows(w, c).len(), r + (w.inv.sizes[c as usize] as usize).saturating_sub(1)))
}

/// A row's colour by retail's rules for its own part (`c`) against the other side's.
fn tint(w: &World, b: &Storage, bank_side: bool, c: u8, dest: u8, s: &Slot) -> u32 {
    let Some(info) = w.item_info.as_ref() else { return TINT_DIM };
    let st = if bank_side { Storage { bank: c, side: dest, ..*b } } else { Storage { bank: dest, side: c, ..*b } };
    gitems::tint(w, info, &st, bank_side, s)
}

/// Where an inventory item goes in the bag: the first part with room that takes it.
fn target(w: &World, b: &Storage, g: usize, s: &Slot) -> Option<u8> {
    BAGS[g].1.iter().copied().find(|&c| gitems::has_free(w, c) && tint(w, b, false, gitems::INVENTORY, c, s) != TINT_DIM)
}

/// The view's mouse.
#[derive(Default)]
struct Mouse {
    bag: Option<usize>,
    take: Option<(bool, usize, bool)>,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let Some((b, iscroll, icursor)) = view(w) else { return };
    let Some(info) = w.item_info.clone() else { return };
    let m = p.m;
    let g = group(b.bank);
    let tabs = bags(w, &b);
    let bag = rows(w, g);
    let inv = gitems::store_rows(w, gitems::INVENTORY);
    let first = BAGS[g].1[0];
    let tints_bag: Vec<u32> = bag.iter().map(|(c, s)| if b.read_only { TINT_DIM } else { tint(w, &b, true, *c, gitems::INVENTORY, s) }).collect();
    let tints_inv: Vec<u32> = inv.iter().map(|s| if b.read_only || b.bank == BIN && false { TINT_DIM } else if target(w, &b, g, s).is_some() { tint(w, &b, false, gitems::INVENTORY, target(w, &b, g, s).unwrap_or(first), s) } else { TINT_DIM }).collect();
    let icon = |id: u16| m.item_icon(p.ctx, &info, id);
    let bag_icons: Vec<_> = bag.iter().map(|(_, s)| icon(s.id)).collect();
    let inv_icons: Vec<_> = inv.iter().map(|s| icon(s.id)).collect();
    let names = |s: &Slot| info.item(s.id).map(|it| it.name.clone()).unwrap_or_default();
    let bag_names: Vec<String> = bag.iter().map(|(_, s)| names(s)).collect();
    let inv_names: Vec<String> = inv.iter().map(names).collect();
    let (used, room) = fill(w, g);
    let inv_room = (w.inv.sizes[0] as usize).saturating_sub(1);
    let on_bank = b.on_bank;
    let (bcur, icur) = (b.scroll + b.cursor, iscroll + icursor);
    let mut mouse = Mouse::default();
    p.panel("window", Some(Title { icon: Some("icons/mog_safe"), text: "Storage", note: b.read_only.then_some("Away from the moogle") }), 720.0, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for &k in &tabs {
                if tab(ui, m, BAGS[k].0, k == g).clicked() {
                    mouse.bag = Some(k);
                }
            }
        });
        ui.add_space(8.0);
        let tall = ui.ctx().content_rect().height() * 0.55;
        ui.horizontal_top(|ui| {
            let lists = [(true, format!("{}  {used} / {room}", BAGS[g].0), &bag_names, &bag_icons, &tints_bag, bcur), (false, format!("Inventory  {} / {inv_room}", inv.len()), &inv_names, &inv_icons, &tints_inv, icur)];
            for (bank_side, title, names, icons, tints, cur) in lists {
                ui.vertical(|ui| {
                    ui.set_width(340.0);
                    ui.label(egui::RichText::new(title).size(m.theme.small).color(m.theme.color("gold")));
                    egui::ScrollArea::vertical().id_salt(bank_side).max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
                        for (k, name) in names.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            let focus = on_bank == bank_side && k == cur;
                            let face = match () {
                                _ if focus => "controls/list_row_selected",
                                _ if resp.hovered() => "controls/list_row_hover",
                                _ => "controls/list_row_normal",
                            };
                            if let Some(s) = m.art.sprite(face) {
                                ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
                            }
                            let dim = tints[k] == TINT_DIM;
                            if let Some(t) = icons[k] {
                                ui.painter().image(t, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), if dim { Color32::from_gray(110) } else { Color32::WHITE });
                            }
                            let c = if dim { m.theme.color("muted") } else if tints[k] == TINT_WHITE { m.theme.color("text") } else { super::chat::colour(tints[k].to_le_bytes()) };
                            ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), c);
                            let count = if bank_side { bag[k].1.count } else { inv[k].count };
                            if count > 1 {
                                ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(m.theme.ui), m.theme.color("text"));
                            }
                            if resp.clicked() {
                                mouse.take = Some((bank_side, k, false));
                            }
                            if resp.secondary_clicked() {
                                mouse.take = Some((bank_side, k, true));
                            }
                            ui.add_space(2.0);
                        }
                        if names.is_empty() {
                            ui.label(egui::RichText::new("Empty.").size(m.theme.ui).color(m.theme.color("muted")));
                        }
                    });
                });
                ui.add_space(14.0);
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Click moves a stack across, right click one. Tab changes the bag.").size(m.theme.small).color(m.theme.color("muted")));
    });
    if let Some(k) = mouse.bag {
        switch(w, k);
    }
    if let Some((bank_side, k, one)) = mouse.take {
        set_cursor(w, bank_side, k);
        take(w, bank_side, k, one);
    }
}

/// A tab in the pack's button frame.
fn tab(ui: &mut Ui, m: &super::Modern, label: &str, lit: bool) -> egui::Response {
    let font = FontId::proportional(m.theme.small);
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x);
    let (r, resp) = ui.allocate_exact_size(vec2(tw + 24.0, 26.0), Sense::click());
    let face = if lit { "controls/button_selected" } else if resp.hovered() { "controls/button_hover" } else { "controls/button_normal" };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, if lit { m.theme.color("gold") } else { m.theme.color("text") });
    resp
}

fn store_mut(w: &mut World) -> Option<&mut Storage> {
    match w.win.as_mut()? {
        Win::Items { store: Some(b), .. } => Some(b),
        _ => None,
    }
}

/// Another bag: its first part, the cursor on its top.
fn switch(w: &mut World, g: usize) {
    if w.viewhouse.is_some() {
        let open = super::super::viewhouse::open_containers(w);
        if let Some(child) = BAGS[g].1.iter().find(|c| open.contains(c)).and_then(|&c| super::super::viewhouse::child_of(c)) {
            super::super::viewhouse::open_row(w, child);
        }
        return;
    }
    if let Some(b) = store_mut(w)
        && group(b.bank) != g
    {
        (b.bank, b.scroll, b.cursor, b.on_bank) = (BAGS[g].1[0], 0, 0, true);
        w.sounds.push(Se::Cursor as u32);
    }
}

/// The cursor onto row `k` of a side (kept as scroll 0, cursor k).
fn set_cursor(w: &mut World, bank_side: bool, k: usize) {
    match w.win.as_mut() {
        Some(Win::Items { store: Some(b), scroll, cursor, .. }) => {
            b.on_bank = bank_side;
            if bank_side {
                (b.scroll, b.cursor) = (0, k);
            } else {
                (*scroll, *cursor) = (0, k);
            }
        }
        _ => {}
    }
}

/// Row `k` of a side moves across (all of it, or one): c2s 0x029 to the other side's first free slot.
fn take(w: &mut World, bank_side: bool, k: usize, one: bool) {
    let Some((b, ..)) = view(w) else { return };
    if b.read_only {
        w.sounds.push(Se::Error as u32);
        return;
    }
    let g = group(b.bank);
    let (from, to, s) = if bank_side {
        let Some((c, s)) = rows(w, g).into_iter().nth(k) else { return };
        if tint(w, &b, true, c, gitems::INVENTORY, &s) == TINT_DIM {
            w.sounds.push(Se::Error as u32);
            return;
        }
        (c, gitems::INVENTORY, s)
    } else {
        let Some(s) = gitems::store_rows(w, gitems::INVENTORY).into_iter().nth(k) else { return };
        let Some(c) = target(w, &b, g, &s) else {
            w.sounds.push(Se::Error as u32);
            return;
        };
        (gitems::INVENTORY, c, s)
    };
    let n = if one { 1 } else { s.count.max(1) };
    gitems::send_move(w, from, to, s.index, n);
    w.sounds.push(Se::Select as u32);
}

/// The view's keys: Left / Right the side, Up / Down the row, Enter moves its stack across, Tab the next bag, Escape
/// closes (retail's). True when taken.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    if !w.modern {
        return false;
    }
    let Some((b, iscroll, icursor)) = view(w) else { return false };
    use egui::Key as K;
    let g = group(b.bank);
    let (bn, inn) = (rows(w, g).len(), gitems::store_rows(w, gitems::INVENTORY).len());
    let (side, cur, n) = if b.on_bank { (true, b.scroll + b.cursor, bn) } else { (false, iscroll + icursor, inn) };
    match key {
        K::ArrowUp | K::ArrowDown if n > 0 => {
            let to = if key == K::ArrowDown { (cur + 1).min(n - 1) } else { cur.saturating_sub(1) };
            set_cursor(w, side, to);
            w.sounds.push(Se::Cursor as u32);
        }
        K::ArrowLeft | K::ArrowRight => {
            let bank_side = key == K::ArrowLeft;
            let n = if bank_side { bn } else { inn };
            let k = if bank_side { b.scroll + b.cursor } else { iscroll + icursor };
            set_cursor(w, bank_side, k.min(n.saturating_sub(1)));
            w.sounds.push(Se::Cursor as u32);
        }
        K::Tab => {
            let tabs = bags(w, &b);
            if let Some(i) = tabs.iter().position(|&k| k == g) {
                switch(w, tabs[(i + 1) % tabs.len()]);
            }
        }
        K::Enter if n > 0 => take(w, side, cur.min(n - 1), false),
        K::Escape => {
            w.win = None;
            // From View House its menu closes too (the view is the whole of it here).
            super::super::viewhouse::close(w);
            w.sounds.push(Se::Close as u32);
        }
        _ => {}
    }
    true
}
