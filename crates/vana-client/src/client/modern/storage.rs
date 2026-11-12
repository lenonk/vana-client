//! The modern storage view in the command window, over items.rs's storage state and rules: the bags as tabs with the
//! numbered ones merged (Mog Safe and Safe 2 one Mog Safe, Mog Wardrobe 1..8 one Mog Wardrobe; user, 2026-10-26),
//! the chosen bag's items beside the inventory's. A click (or Ctrl-click) sends a stack to the other pane, a right
//! click one of it, a Shift-click asks how many; a row dragged onto the other pane moves its stack (with Shift held,
//! asks how many), the pane outlined teal where the drop is taken and red where it is not. Into a merged bag it goes to
//! the first part with room that takes it. Retail's colours say what may move (equipped pieces, the moogle's bags away
//! from it, non-gear into a wardrobe, installed furniture ...), and a refused row says why (ours). Nothing moves
//! before the server does: a sent move shows as pending until the slot changes, and one the server ignores is
//! explained after a while. A move made while the last is pending waits for it, and the net thread holds each for
//! the send gate (one a second) where retail would drop it.

use super::super::World;
use super::super::items::{self as gitems, Pending, Qty, SortW, Storage, Waiting, Win};
use super::{Panels, Title};
use crate::audio::Ui as Se;
use crate::inventory::{Slot, TINT_DIM, TINT_WHITE};
use egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};

const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
const BIN: u8 = 17;
/// How long a sent move may wait for the server before it counts as refused.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

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
    (0..BAGS.len())
        .filter(|&g| {
            BAGS[g].1.contains(&b.bank)
                || (!b.read_only
                    && b.bank != BIN
                    && BAGS[g]
                        .1
                        .iter()
                        .any(|&c| c != BIN && c != 3 && gitems::usable(w, &Storage { bank: 0xff, ..*b }, c)))
        })
        .collect()
}

/// A bag's parts this account may use now: the one the view opened on, and the others retail's lists leave on (no
/// Safe 2 without the 2F, no Locker without a lease, no Wardrobe 3..8 without its lobby feature bit, ...). The server
/// refuses a move into the rest.
fn parts(w: &World, b: &Storage, g: usize) -> Vec<u8> {
    let house = w.viewhouse.is_some().then(|| super::super::viewhouse::open_containers(w));
    let on = |c: u8| {
        super::super::equipwin::bag_open(w, c)
            && match &house {
                Some(open) => open.contains(&c),
                None => gitems::usable(w, &Storage { bank: 0xff, ..*b }, c),
            }
    };
    BAGS[g].1.iter().copied().filter(|&c| c == b.bank || matches!(c, 3 | BIN) || on(c)).collect()
}

/// A bag's rows over its parts.
fn rows(w: &World, b: &Storage, g: usize) -> Vec<(u8, Slot)> {
    parts(w, b, g).into_iter().flat_map(|c| gitems::store_rows(w, c).into_iter().map(move |s| (c, s))).collect()
}

/// Used and room over a bag's parts.
fn fill(w: &World, b: &Storage, g: usize) -> (usize, usize) {
    parts(w, b, g).into_iter().fold((0, 0), |(u, r), c| {
        (u + gitems::store_rows(w, c).len(), r + (w.inv.sizes[c as usize] as usize).saturating_sub(1))
    })
}

/// A row's colour by retail's rules for its part (`c`) against the other side's.
fn tint(w: &World, b: &Storage, bank_side: bool, c: u8, dest: u8, s: &Slot) -> u32 {
    let Some(info) = w.item_info.as_ref() else { return TINT_DIM };
    let st = if bank_side { Storage { bank: c, side: dest, ..*b } } else { Storage { bank: dest, side: c, ..*b } };
    gitems::tint(w, info, &st, bank_side, s)
}

/// A bag's parts with room, worked out once for every row.
fn open_parts(w: &World, b: &Storage, g: usize) -> Vec<u8> {
    parts(w, b, g).into_iter().filter(|&c| gitems::has_free(w, c)).collect()
}

/// Where an inventory item goes in the bag: the first part with room (`open`) that takes it.
fn target(w: &World, b: &Storage, open: &[u8], s: &Slot) -> Option<u8> {
    open.iter().copied().find(|&c| tint(w, b, false, gitems::INVENTORY, c, s) != TINT_DIM)
}

/// Type-to-search's places (listsearch.rs): the open bag, the inventory, then the other bags this view may show; each
/// row's bag (None: the inventory), its row there, part and slot.
fn places(w: &World) -> Vec<(Option<usize>, usize, u8, Slot)> {
    let Some((b, ..)) = view(w) else { return Vec::new() };
    let g = group(b.bank);
    let bag = |g: usize| rows(w, &b, g).into_iter().enumerate().map(move |(k, (c, s))| (Some(g), k, c, s));
    let inv =
        gitems::store_rows(w, gitems::INVENTORY).into_iter().enumerate().map(|(k, s)| (None, k, gitems::INVENTORY, s));
    let others: Vec<_> = bags(w, &b).into_iter().filter(|&o| o != g).flat_map(bag).collect();
    bag(g).chain(inv).chain(others).collect()
}

pub(in crate::client) fn search_open(w: &World) -> bool {
    // Not while the quantity strip asks how many: a pick would move the whole stack.
    w.modern && view(w).is_some_and(|(b, ..)| b.qty.is_none())
}

/// Every place's item: its bag (and count) as the note, greyed when it can't move across.
pub(in crate::client) fn search_rows(w: &World) -> Vec<super::super::listsearch::Entry> {
    let Some((b, ..)) = view(w) else { return Vec::new() };
    let open = open_parts(w, &b, group(b.bank));
    places(w)
        .into_iter()
        .map(|(bag, _, c, s)| {
            let dim = b.read_only
                || match bag {
                    Some(_) => tint(w, &b, true, c, gitems::INVENTORY, &s) == TINT_DIM,
                    None => target(w, &b, &open, &s).is_none(),
                };
            let name = bag.map_or("Inventory", |o| BAGS[o].0);
            let note = if s.count > 1 { format!("{name} · {}", s.count) } else { name.to_string() };
            super::super::listsearch::Entry::item(w, s.id).note(note).dim(dim)
        })
        .collect()
}

/// A pick: its bag shown, the cursor on it, and across it goes (Enter).
pub(in crate::client) fn search_pick(w: &mut World, _: Option<&super::super::game_ui::GameUi>, k: usize) {
    let Some(&(bag, row, ..)) = places(w).get(k) else { return };
    if let Some(g) = bag {
        switch(w, g);
    }
    set_cursor(w, bag.is_some(), row);
    take(w, bag.is_some(), row, How::Stack);
}

/// Why row `s` of a side (in part `c` for the bag side) may not move, None when it may (`open`: the bag's parts with
/// room).
fn refusal(w: &World, b: &Storage, g: usize, open: &[u8], bank_side: bool, c: u8, s: &Slot) -> Option<&'static str> {
    let Some(info) = w.item_info.as_ref() else { return Some("Item data is still loading.") };
    if b.read_only {
        return Some("Away from the moogle: this list is read only.");
    }
    if bank_side {
        let (t, why) = gitems::judge(w, info, &Storage { bank: c, side: gitems::INVENTORY, ..*b }, true, s);
        return (t == TINT_DIM || t == 0x8060_2020).then(|| why.unwrap_or("This can't move."));
    }
    if super::items::bazaar_price(w, gitems::INVENTORY, s.index).is_some() {
        return Some(super::items::BAZAAR_NOTE);
    }
    if target(w, b, open, s).is_some() {
        return None;
    }
    // No part takes it: the first part's reason other than room, else the bag is full.
    let whys: Vec<_> = parts(w, b, g)
        .into_iter()
        .filter_map(|c| gitems::judge(w, info, &Storage { bank: c, side: gitems::INVENTORY, ..*b }, false, s).1)
        .collect();
    Some(whys.iter().copied().find(|y| !y.starts_with("No room")).unwrap_or("No room in this bag."))
}

/// What a drag holds: the side and row it left, and that row's part, slot and item (the drop checks it is still
/// there: the lists may change under a drag).
#[derive(Clone, Copy)]
struct Held {
    bank_side: bool,
    k: usize,
    c: u8,
    index: u8,
    id: u16,
}

/// How much of a stack a move takes.
#[derive(Clone, Copy, PartialEq)]
enum How {
    Stack,
    One,
    /// Ask how many first.
    Split,
}

/// The quantity strip's keys.
#[derive(Clone, Copy)]
enum QtyAct {
    Step(i64),
    One,
    All,
    Move,
    Cancel,
}

/// The view's mouse.
#[derive(Default)]
struct Mouse {
    bag: Option<usize>,
    take: Option<(bool, usize, How)>,
    drop: Option<(Held, How)>,
    qty_key: Option<egui::Key>,
    /// A pane's sort switch: its side and 1 Auto-sort / 2 Manual Sort.
    sort: Option<(bool, u16)>,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    settle(w);
    let Some((b, iscroll, icursor)) = view(w) else { return };
    let Some(info) = w.item_info.clone() else { return };
    let (m, ctx, gu) = (p.m, p.ctx, p.gu);
    let g = group(b.bank);
    let tabs = bags(w, &b);
    let bag = rows(w, &b, g);
    let inv = gitems::store_rows(w, gitems::INVENTORY);
    let first = BAGS[g].1[0];
    let open = open_parts(w, &b, g);
    let why_bag: Vec<Option<&'static str>> = bag.iter().map(|(c, s)| refusal(w, &b, g, &open, true, *c, s)).collect();
    let why_inv: Vec<Option<&'static str>> =
        inv.iter().map(|s| refusal(w, &b, g, &open, false, gitems::INVENTORY, s)).collect();
    let tints_bag: Vec<u32> = bag
        .iter()
        .map(|(c, s)| if b.read_only { TINT_DIM } else { tint(w, &b, true, *c, gitems::INVENTORY, s) })
        .collect();
    let tints_inv: Vec<u32> = inv
        .iter()
        .zip(&why_inv)
        .map(|(s, why)| {
            if why.is_some() {
                TINT_DIM
            } else {
                tint(w, &b, false, gitems::INVENTORY, target(w, &b, &open, s).unwrap_or(first), s)
            }
        })
        .collect();
    let icon = |id: u16| m.item_icon(ctx, &info, id);
    let bag_icons: Vec<_> = bag.iter().map(|(_, s)| icon(s.id)).collect();
    let inv_icons: Vec<_> = inv.iter().map(|s| icon(s.id)).collect();
    let names = |s: &Slot| info.item(s.id).map(|it| it.name.clone()).unwrap_or_default();
    let bag_names: Vec<String> = bag.iter().map(|(_, s)| names(s)).collect();
    let inv_names: Vec<String> = inv.iter().map(names).collect();
    let inv_bazaar: Vec<Option<u32>> =
        inv.iter().map(|s| super::items::bazaar_price(w, gitems::INVENTORY, s.index)).collect();
    // The inventory side's cursor on a bazaar item: what frees it under the lists.
    let bazaar_held = !b.on_bank && inv_bazaar.get(iscroll + icursor).copied().flatten().is_some();
    let (used, room) = fill(w, &b, g);
    let inv_room = (w.inv.sizes[0] as usize).saturating_sub(1);
    let on_bank = b.on_bank;
    let (bcur, icur) = (b.scroll + b.cursor, iscroll + icursor);
    // The stack being split, and the move in flight, by name.
    let qty = b.qty.map(|q| {
        (
            q,
            w.inv
                .slots
                .get(&(q.from, q.index))
                .map(|&(id, _)| info.item(id).map(|it| it.name.clone()).unwrap_or_default())
                .unwrap_or_default(),
        )
    });
    let pending = b
        .pending
        .map(|pd| (pd.count, pd.id))
        .or(b.waiting.map(|h| (h.n, h.id)))
        .map(|(n, id)| (n, info.item(id).map(|it| it.name.clone()).unwrap_or_default()));
    let note = b.note;
    let held = egui::DragAndDrop::payload::<Held>(ctx).map(|h| *h);
    let held_icon = held.and_then(|h| {
        if h.bank_side { bag_icons.get(h.k).copied().flatten() } else { inv_icons.get(h.k).copied().flatten() }
    });
    let shift = ctx.input(|i| i.modifiers.shift);
    // Each pane's sort switch: Auto-sort on, Manual Sort on, Num+'s cursor (1 / 2), off; Manual Sort's held row.
    let sort_switch = |bank_side: bool| {
        let cs = sort_parts(w, &b, bank_side);
        let cursor = b.sortw.filter(|s| s.bank_side == bank_side).map(|s| s.cursor);
        (kept(w, &cs), b.manual.is_some_and(|m| m.0 == bank_side), cursor, cs.is_empty() || b.qty.is_some())
    };
    let switches = [sort_switch(true), sort_switch(false)];
    let manual_held = b.manual.and_then(|(side, h)| h.map(|i| (side, b.held_part, i)));
    let mut mouse = Mouse::default();
    let mut found = super::lists::found(p, w, "storage");
    p.panel("window", Some(Title { icon: Some("icons/mog_safe"), text: "Storage", note: b.read_only.then_some("Away from the moogle"), name: false }), 720.0, |ui| {
        let labels: Vec<&str> = tabs.iter().map(|&k| BAGS[k].0).collect();
        for (i, resp) in super::tabs::strip(ui, m, "storage tabs", &labels, tabs.iter().position(|&k| k == g), None).into_iter().enumerate() {
            if resp.clicked() {
                mouse.bag = Some(tabs[i]);
            }
        }
        ui.add_space(8.0);
        let tall = ui.ctx().content_rect().height() * 0.55;
        ui.horizontal_top(|ui| {
            let lists = [(true, format!("{}  {used} / {room}", BAGS[g].0), &bag_names, &bag_icons, &tints_bag, &why_bag, bcur), (false, format!("Inventory  {} / {inv_room}", inv.len()), &inv_names, &inv_icons, &tints_inv, &why_inv, icur)];
            for (bank_side, title, names, icons, tints, whys, cur) in lists {
                // The cursor's row kept in view as the keys move it (bag and side in the key).
                let follow = m.moved(if bank_side { "storage bag" } else { "storage inventory" }, cur + g * 100_000);
                let (auto_on, manual_on, sort_cursor, sort_off) = switches[usize::from(!bank_side)];
                let col = ui.vertical(|ui| {
                    ui.set_width(340.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(title).size(m.theme.small).color(m.theme.color("gold")));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (child, label, on) in [(2u16, "Manual Sort", manual_on), (1, "Auto-sort", auto_on)] {
                                let s = super::kit::Btn { lit: on && !sort_off, cursor: sort_cursor == Some(child), off: sort_off };
                                if super::kit::clicked(&super::kit::button(ui, m, label, 0.0, s), s) {
                                    mouse.sort = Some((bank_side, child));
                                }
                            }
                        });
                    });
                    // A search runs over every bag in the side with the cursor.
                    if bank_side == on_bank && super::lists::rows_or(ui, m, &mut found) {
                        return;
                    }
                    egui::ScrollArea::vertical().id_salt(bank_side).max_height(tall).min_scrolled_height(tall).auto_shrink([false, true]).show(ui, |ui| {
                        for (k, name) in names.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
                            let (c, s) = if bank_side { (bag[k].0, &bag[k].1) } else { (gitems::INVENTORY, &inv[k]) };
                            let held_row = manual_held == Some((bank_side, c, s.index));
                            resp.dnd_set_drag_payload(Held { bank_side, k, c, index: s.index, id: s.id });
                            let focus = on_bank == bank_side && k == cur;
                            if k == cur && follow {
                                ui.scroll_to_rect(r, None);
                            }
                            let face = match () {
                                _ if focus => "controls/list_row_selected",
                                _ if resp.hovered() => "controls/list_row_hover",
                                _ => "controls/list_row_normal",
                            };
                            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                            let dim = tints[k] == TINT_DIM;
                            let lifted = held.is_some_and(|h| h.bank_side == bank_side && h.k == k);
                            if let Some(t) = icons[k] {
                                let tint = if dim || lifted { Color32::from_gray(110) } else { Color32::WHITE };
                                ui.painter().image(t, Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
                            }
                            let c = if held_row { m.theme.color("gold") } else if dim { m.theme.color("muted") } else if tints[k] == TINT_WHITE { m.theme.color("text") } else { super::chat::colour(tints[k].to_le_bytes()) };
                            ui.painter().text(pos2(r.min.x + 42.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.ui), c);
                            let count = s.count;
                            let mut right = r.max.x - 12.0;
                            if count > 1 {
                                right = ui.painter().text(pos2(right, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(m.theme.ui), m.theme.color("text")).min.x - 8.0;
                            }
                            if let Some(price) = (!bank_side).then(|| inv_bazaar[k]).flatten() {
                                super::items::bazaar_tag(ui, m, right, r.center().y, price);
                            }
                            if resp.clicked() {
                                mouse.take = Some((bank_side, k, if shift { How::Split } else { How::Stack }));
                            }
                            if resp.secondary_clicked() {
                                mouse.take = Some((bank_side, k, How::One));
                            }
                            if let Some(why) = whys[k] {
                                m.tip(ui, r, name, why);
                            }
                            ui.add_space(2.0);
                        }
                        if names.is_empty() {
                            ui.label(egui::RichText::new("Empty.").size(m.theme.ui).color(m.theme.color("muted")));
                        }
                    });
                });
                // The pane takes a row dragged from the other one: outlined teal where the drop is taken, red where
                // it is not (the dragged row's pane takes nothing).
                let pane = col.response.rect.expand(4.0);
                let drop = ui.interact(pane, egui::Id::unique(("storage drop", bank_side)), Sense::hover());
                if let Some(h) = held.filter(|h| h.bank_side != bank_side) {
                    let ok = if h.bank_side { why_bag.get(h.k).copied().flatten().is_none() } else { why_inv.get(h.k).copied().flatten().is_none() };
                    if drop.contains_pointer() {
                        let face = if ok { "controls/drag_outline_drop_valid" } else { "controls/drag_outline_drop_invalid" };
                        if let Some(s) = m.art.sprite(face) {
                            ui.painter().add(super::paint::nine_slice(&s, pane, 0.5, Color32::WHITE));
                        }
                    }
                    if drop.dnd_release_payload::<Held>().is_some() {
                        mouse.drop = Some((h, if shift { How::Split } else { How::Stack }));
                    }
                }
                ui.add_space(14.0);
            }
        });
        ui.add_space(4.0);
        // How many of a stack: the other count windows' row (itemctrl's keys as buttons, OK, Cancel).
        if let (Some((q, name)), Some(gu)) = (&qty, gu) {
            mouse.qty_key = super::craft::count_row(ui, m, gu, &format!("Move how many {name}?"), q.value, q.max);
        }
        if let Some((n, name)) = &pending {
            ui.label(egui::RichText::new(format!("Moving {name}{}...", if *n > 1 { format!(" x{n}") } else { String::new() })).size(m.theme.small).color(m.theme.color("muted")));
        } else if let Some(note) = note {
            ui.label(egui::RichText::new(note).size(m.theme.small).color(m.theme.color("hp")));
        } else if b.manual.is_some() {
            ui.label(egui::RichText::new("Manual Sort: pick a row, then the row to put it at. Escape or Manual Sort ends it.").size(m.theme.small).color(m.theme.color("gold")));
        }
        if bazaar_held {
            ui.label(egui::RichText::new(super::items::BAZAAR_NOTE).size(m.theme.small).color(m.theme.color("gold")));
        }
        ui.label(egui::RichText::new("Click or drag moves a stack across, right click one; Shift (or Shift+Enter) asks how many. Tab changes the bag, Num+ its sorting. Type to search every bag.").size(m.theme.small).color(m.theme.color("muted")));
    });
    if let Some(f) = found {
        f.pick(w, p.gu);
        return;
    }
    // The row under the pointer while dragged.
    if let (Some(t), Some(at)) = (held_icon, ctx.pointer_hover_pos()) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("storage drag")));
        painter.image(
            t,
            Rect::from_center_size(at, vec2(32.0, 32.0)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::from_white_alpha(220),
        );
    }
    if let Some(k) = mouse.bag {
        switch(w, k);
    }
    if let Some(k) = mouse.qty_key {
        key(w, k, false);
    }
    if let Some((bank_side, child)) = mouse.sort {
        if child == 1 { sort_auto(w, bank_side) } else { sort_manual(w, bank_side) }
    }
    // In Manual Sort a click on its pane picks or puts; nothing moves across.
    let manual = view(w).and_then(|(b, ..)| b.manual).map(|m| m.0);
    if let Some((bank_side, k, how)) = mouse.take {
        match manual {
            Some(side) if side == bank_side => manual_pick(w, bank_side, k),
            Some(_) => {}
            None => {
                set_cursor(w, bank_side, k);
                take(w, bank_side, k, how);
            }
        }
    }
    if let Some((h, how)) = mouse.drop.filter(|_| manual.is_none()) {
        dropped(w, h, how);
    }
}

/// The containers a pane's sort switch works on: the bag's parts (not Temporary or the bin), or the inventory; none
/// in a read-only view.
fn sort_parts(w: &World, b: &Storage, bank_side: bool) -> Vec<u8> {
    if b.read_only {
        return Vec::new();
    }
    if !bank_side {
        return vec![gitems::INVENTORY];
    }
    parts(w, b, group(b.bank)).into_iter().filter(|&c| c != 3 && c != BIN).collect()
}

/// Every one of the containers kept sorted (config auto_sort).
fn kept(w: &World, cs: &[u8]) -> bool {
    !cs.is_empty() && cs.iter().all(|&c| w.auto_sort >> c & 1 != 0)
}

/// Auto-sort on a pane: its containers sorted now and kept sorted as items come in (config auto_sort, as the
/// classic sort window's Auto); Manual Sort there ends.
fn sort_auto(w: &mut World, bank_side: bool) {
    let Some((b, ..)) = view(w) else { return };
    let cs = sort_parts(w, &b, bank_side);
    if cs.is_empty() {
        return refuse(w, "This list doesn't sort here.");
    }
    for &c in &cs {
        gitems::auto_sort_c(w, c);
        gitems::keep_sorted(w, c, true);
    }
    if let Some(b) = store_mut(w) {
        if b.manual.is_some_and(|m| m.0 == bank_side) {
            b.manual = None;
        }
        (b.sortw, b.note) = (None, None);
    }
    w.sounds.push(Se::Select as u32);
}

/// Manual Sort on a pane: it is no longer kept sorted, and its rows are put in order by hand until Manual Sort is
/// pressed again (or Escape).
fn sort_manual(w: &mut World, bank_side: bool) {
    let Some((b, ..)) = view(w) else { return };
    if b.manual.is_some_and(|m| m.0 == bank_side) {
        if let Some(b) = store_mut(w) {
            (b.manual, b.sortw) = (None, None);
        }
        w.sounds.push(Se::Cancel as u32);
        return;
    }
    let cs = sort_parts(w, &b, bank_side);
    if cs.is_empty() {
        return refuse(w, "This list doesn't sort here.");
    }
    for &c in &cs {
        gitems::keep_sorted(w, c, false);
    }
    if let Some(b) = store_mut(w) {
        (b.manual, b.sortw, b.note, b.on_bank) = (Some((bank_side, None)), None, None, bank_side);
    }
    w.sounds.push(Se::Select as u32);
}

/// Manual Sort's pick on row `k` of its pane: the first holds the row, the second puts it at that row (retail's
/// FUN_101f0b30: the same row splits one off a stack, a row of the same item takes the stack). Rows keep to their
/// part of a merged bag.
fn manual_pick(w: &mut World, bank_side: bool, k: usize) {
    let Some((b, ..)) = view(w) else { return };
    let row = if bank_side {
        rows(w, &b, group(b.bank)).into_iter().nth(k)
    } else {
        gitems::store_rows(w, gitems::INVENTORY).into_iter().nth(k).map(|s| (gitems::INVENTORY, s))
    };
    let Some((c, s)) = row else { return };
    set_cursor(w, bank_side, k);
    match b.manual {
        Some((side, None)) if side == bank_side => {
            if let Some(b) = store_mut(w) {
                (b.manual, b.held_part, b.note) = (Some((side, Some(s.index))), c, None);
            }
            w.sounds.push(Se::Select as u32);
        }
        Some((side, Some(from))) if side == bank_side => {
            if c != b.held_part {
                return refuse(w, "That row is in another part of this bag: Manual Sort keeps to one.");
            }
            gitems::manual_move_c(w, c, from, &s, true);
            if let Some(b) = store_mut(w) {
                (b.manual, b.note) = (Some((side, None)), None);
            }
            w.sounds.push(Se::Select as u32);
        }
        _ => {}
    }
}

/// A dragged row let go on the other pane: it moves if its stack is still in the list (where it is now).
fn dropped(w: &mut World, h: Held, how: How) {
    let Some((b, ..)) = view(w) else { return };
    let side: Vec<(u8, Slot)> = if h.bank_side {
        rows(w, &b, group(b.bank))
    } else {
        gitems::store_rows(w, gitems::INVENTORY).into_iter().map(|s| (gitems::INVENTORY, s)).collect()
    };
    let at = if side.get(h.k).is_some_and(|(c, s)| (*c, s.index, s.id) == (h.c, h.index, h.id)) {
        Some(h.k)
    } else {
        side.iter().position(|(c, s)| (*c, s.index, s.id) == (h.c, h.index, h.id))
    };
    let Some(k) = at else {
        refuse(w, "That stack changed: pick it again.");
        return;
    };
    set_cursor(w, h.bank_side, k);
    take(w, h.bank_side, k, how);
}

/// The move in flight settles: done once its slot holds something else; after PATIENCE unchanged, the server refused it
/// and the view says so (nothing was moved on our side, so nothing needs putting back).
fn settle(w: &mut World) {
    if let Some(pd) = view(w).and_then(|(b, ..)| b.pending) {
        let now = w.inv.slots.get(&(pd.from, pd.index)).copied();
        let note = if now != Some((pd.id, pd.count)) {
            None
        } else if pd.sent.elapsed() > PATIENCE {
            Some("Not moved: the server refused it (see the log). Nothing changed.")
        } else {
            return;
        };
        if let Some(b) = store_mut(w) {
            (b.pending, b.note) = (None, note);
        }
    }
    release(w);
}

/// The waiting move goes once the last has settled, if its stack is still there and may still move; into a merged
/// bag it takes the part with room then. It goes to the bag it was made for, whichever tab shows now.
fn release(w: &mut World) {
    let Some((b, ..)) = view(w) else { return };
    let Some(h) = b.waiting else { return };
    if b.pending.is_some() {
        return;
    }
    if let Some(b) = store_mut(w) {
        b.waiting = None;
    }
    let b = Storage { bank: h.bag, ..b };
    let g = group(h.bag);
    let bank_side = h.from != gitems::INVENTORY;
    let Some(s) = gitems::store_rows(w, h.from).into_iter().find(|s| s.index == h.index && s.id == h.id) else {
        return refuse(w, "That stack changed: pick it again.");
    };
    let open = open_parts(w, &b, g);
    if let Some(why) = refusal(w, &b, g, &open, bank_side, h.from, &s) {
        return refuse(w, why);
    }
    let to = if bank_side { h.to } else { target(w, &b, &open, &s).unwrap_or(h.to) };
    send_now(w, h.from, to, &s, h.n.min(s.count));
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
        if let Some(child) =
            BAGS[g].1.iter().find(|c| open.contains(c)).and_then(|&c| super::super::viewhouse::child_of(c))
        {
            super::super::viewhouse::open_row(w, child);
        }
        return;
    }
    if let Some(b) = store_mut(w)
        && group(b.bank) != g
    {
        (b.bank, b.scroll, b.cursor, b.on_bank, b.qty, b.note) = (BAGS[g].1[0], 0, 0, true, None, None);
        (b.manual, b.sortw) = (None, None);
        w.sounds.push(Se::Cursor as u32);
    }
}

/// The cursor onto row `k` of a side (kept as scroll 0, cursor k).
fn set_cursor(w: &mut World, bank_side: bool, k: usize) {
    if let Some(Win::Items { store: Some(b), scroll, cursor, .. }) = w.win.as_mut() {
        b.on_bank = bank_side;
        if bank_side {
            (b.scroll, b.cursor) = (0, k);
        } else {
            (*scroll, *cursor) = (0, k);
        }
    }
}

/// Row `k` of a side moves across (all of it, one, or how many the quantity strip says): c2s 0x029 to the other side's
/// first free slot, pending until the server moves it. A row that may not move says why.
fn take(w: &mut World, bank_side: bool, k: usize, how: How) {
    let Some((b, ..)) = view(w) else { return };
    let g = group(b.bank);
    let found = if bank_side {
        rows(w, &b, g).into_iter().nth(k)
    } else {
        gitems::store_rows(w, gitems::INVENTORY).into_iter().nth(k).map(|s| (gitems::INVENTORY, s))
    };
    let Some((c, s)) = found else { return };
    let open = open_parts(w, &b, g);
    if let Some(why) = refusal(w, &b, g, &open, bank_side, c, &s) {
        refuse(w, why);
        return;
    }
    let (from, to) =
        if bank_side { (c, gitems::INVENTORY) } else { (gitems::INVENTORY, target(w, &b, &open, &s).unwrap_or(c)) };
    if how == How::Split && s.count > 1 {
        // Ours: the strip opens on half the stack (retail's itemctrl on 1).
        if let Some(b) = store_mut(w) {
            (b.qty, b.note) = (Some(Qty { index: s.index, value: s.count.div_ceil(2), max: s.count, from, to }), None);
        }
        w.sounds.push(Se::Select as u32);
        return;
    }
    let n = if how == How::One { 1 } else { s.count.max(1) };
    send(w, from, to, &s, n);
}

fn refuse(w: &mut World, why: &'static str) {
    if let Some(b) = store_mut(w) {
        b.note = Some(why);
    }
    w.sounds.push(Se::Error as u32);
}

/// c2s 0x029 for `n` of the stack in slot `s` of `from`, kept as the pending move. While the last is pending it waits
/// (one at a time), so a move into a merged bag picks its part once the last has landed.
fn send(w: &mut World, from: u8, to: u8, s: &Slot, n: u32) {
    let Some((b, ..)) = view(w) else { return };
    if b.pending.is_some() {
        if b.waiting.is_some() {
            return refuse(w, "One move at a time: wait for the last one.");
        }
        if let Some(st) = store_mut(w) {
            st.waiting = Some(Waiting { bag: b.bank, from, to, index: s.index, id: s.id, n });
            (st.note, st.qty) = (None, None);
        }
    } else {
        send_now(w, from, to, s, n);
    }
    w.sounds.push(Se::Select as u32);
}

/// The move goes: the net thread holds it for FUN_100fc3c0's gate (one 0x029 a second) where retail would drop it.
fn send_now(w: &mut World, from: u8, to: u8, s: &Slot, n: u32) {
    gitems::send_move_held(w, from, to, s.index, n);
    if let Some(b) = store_mut(w) {
        b.pending = Some(Pending { from, index: s.index, id: s.id, count: s.count, sent: std::time::Instant::now() });
        (b.note, b.qty) = (None, None);
    }
}

/// The quantity strip: a step, one, all, Move or Cancel.
fn qty_act(w: &mut World, a: QtyAct) {
    let Some(q) = view(w).and_then(|(b, ..)| b.qty) else { return };
    match a {
        QtyAct::Step(_) | QtyAct::One | QtyAct::All => {
            let value = match a {
                QtyAct::Step(d) => (q.value as i64 + d).clamp(1, q.max as i64) as u32,
                QtyAct::One => 1,
                _ => q.max,
            };
            if let Some(b) = store_mut(w) {
                b.qty = Some(Qty { value, ..q });
            }
            w.sounds.push(Se::Cursor as u32);
        }
        QtyAct::Move => {
            // The stack as it is now (it may have changed while the strip was up).
            match w.inv.slots.get(&(q.from, q.index)).copied() {
                Some((id, count)) if id != 0 && count >= q.value => send(
                    w,
                    q.from,
                    q.to,
                    &Slot { index: q.index, id, count, equipped: false, equipped_ex: false },
                    q.value,
                ),
                _ => {
                    if let Some(b) = store_mut(w) {
                        b.qty = None;
                    }
                    refuse(w, "That stack changed: pick it again.");
                }
            }
        }
        QtyAct::Cancel => {
            if let Some(b) = store_mut(w) {
                b.qty = None;
            }
            w.sounds.push(Se::Cancel as u32);
        }
    }
}

/// The view's keys: Left / Right the side, Up / Down the row, Enter moves its stack across (Shift+Enter asks how many),
/// Tab the next bag, Escape closes (retail's). With the quantity strip up, itemctrl's keys: Up / Down one, Right 1,
/// Left all, Enter moves, Escape cancels. True when taken.
pub fn key(w: &mut World, key: egui::Key, shift: bool) -> bool {
    if !w.modern {
        return false;
    }
    let Some((b, iscroll, icursor)) = view(w) else { return false };
    use egui::Key as K;
    if b.qty.is_some() {
        match key {
            K::ArrowUp => qty_act(w, QtyAct::Step(1)),
            K::ArrowDown => qty_act(w, QtyAct::Step(-1)),
            K::ArrowRight => qty_act(w, QtyAct::One),
            K::ArrowLeft => qty_act(w, QtyAct::All),
            K::Enter => qty_act(w, QtyAct::Move),
            K::Escape => qty_act(w, QtyAct::Cancel),
            _ => {}
        }
        return true;
    }
    let g = group(b.bank);
    let (bn, inn) = (rows(w, &b, g).len(), gitems::store_rows(w, gitems::INVENTORY).len());
    // The sort switch Num+ focused: Left / Right (or Up / Down) between Auto-sort and Manual Sort, Enter takes one.
    if let Some(sw) = b.sortw {
        match key {
            K::ArrowLeft | K::ArrowRight | K::ArrowUp | K::ArrowDown => {
                if let Some(b) = store_mut(w) {
                    b.sortw = Some(SortW { cursor: if sw.cursor == 2 { 1 } else { 2 }, ..sw });
                }
                w.sounds.push(Se::Cursor as u32);
            }
            K::Enter if sw.cursor == 2 => sort_manual(w, sw.bank_side),
            K::Enter => sort_auto(w, sw.bank_side),
            K::Escape => {
                if let Some(b) = store_mut(w) {
                    b.sortw = None;
                }
                w.sounds.push(Se::Cancel as u32);
            }
            _ => {}
        }
        return true;
    }
    // Manual Sort: Up / Down on its pane, Enter picks and puts, Escape lets go of the row, then ends it.
    if let Some((mside, held)) = b.manual {
        let (cur, n) = if mside { (b.scroll + b.cursor, bn) } else { (iscroll + icursor, inn) };
        match key {
            K::ArrowUp | K::ArrowDown if n > 0 => {
                let to = if key == K::ArrowDown { (cur + 1).min(n - 1) } else { cur.saturating_sub(1) };
                set_cursor(w, mside, to);
                w.sounds.push(Se::Cursor as u32);
            }
            K::Enter if n > 0 => manual_pick(w, mside, cur.min(n - 1)),
            K::Escape => {
                if let Some(b) = store_mut(w) {
                    b.manual = held.map(|_| (mside, None));
                }
                w.sounds.push(Se::Cancel as u32);
            }
            _ => {}
        }
        return true;
    }
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
        // Tab / Shift+Tab (the pad's RB / LB): the next / previous bag.
        K::Tab => {
            let tabs = bags(w, &b);
            if let Some(i) = tabs.iter().position(|&k| k == g) {
                let n = tabs.len();
                switch(w, tabs[if w.key_shift { (i + n - 1) % n } else { (i + 1) % n }]);
            }
        }
        K::Enter if n > 0 => take(w, side, cur.min(n - 1), if shift { How::Split } else { How::Stack }),
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
