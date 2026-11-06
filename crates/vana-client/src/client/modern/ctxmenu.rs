//! Ours (the pack's interaction contract: "right click a context menu"; retail has none): right-click menus on the
//! Items window's rows (Use, Drop, Sort, Move to), the party and alliance rows (Target, Check, Tell, Focus) and the
//! actors in the world (Talk, Target, Attack, Check, Tell, Invite, Trade). Every row runs what the keyboard path
//! already runs (the item list's keys, the text commands, the trade window), so the rules and refusals stay
//! retail's. One look for every menu, the chat window's included (chatsel.rs).

use super::super::items::{self as gitems, Storage, Sub, Win};
use super::super::textcmd::{self, Mode};
use super::super::{World, target, trade};
use super::Modern;
use egui::{Align2, Color32, FontFamily, FontId, Id, Order, Pos2, Sense, pos2, vec2};

/// The open menu: where it was opened and on what.
#[derive(Clone, Debug, PartialEq)]
pub enum Menu {
    /// An Items window row: its place on screen, the slot (bag, index, item id); `moving` shows the bags.
    Item { at: Pos2, bag: u8, index: u8, id: u16, moving: bool },
    /// A party window slot (0..17).
    Party { at: Pos2, slot: usize },
    /// An actor under the pointer (client/mod.rs fills `World::actor_menu` on a right click that did not drag).
    Actor { at: Pos2, id: u32 },
}

/// The bags an item can go to away from the moogle (container, name): the Mog House's bags are the storage
/// view's.
const BAGS: [(u8, &str); 11] = [
    (5, "Mog Satchel"),
    (6, "Mog Sack"),
    (7, "Mog Case"),
    (8, "Mog Wardrobe"),
    (10, "Mog Wardrobe 2"),
    (11, "Mog Wardrobe 3"),
    (12, "Mog Wardrobe 4"),
    (13, "Mog Wardrobe 5"),
    (14, "Mog Wardrobe 6"),
    (15, "Mog Wardrobe 7"),
    (16, "Mog Wardrobe 8"),
];

/// What a menu's rows do.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Use,
    Drop,
    Sort,
    MoveTo,
    Back,
    Move(u8),
    Target,
    Talk,
    Attack,
    Check,
    Tell,
    Invite,
    Trade,
    Focus,
}

/// A row: its label, the act, whether it can be picked now.
type Row = (String, Act, bool);

/// The open popup: an optional heading (a name), its rows in the pack's frame by `at`. Returns the row picked and
/// whether it should close (a pick, a press outside it, Escape). The keys: Up / Down move the highlight over the usable
/// rows (the first to start), Enter picks it, Escape closes; while a menu is open they reach no game key
/// (World::ctx_menu_open: the key chain skips them, typing_elsewhere stops the key functions).
pub fn popup(ctx: &egui::Context, m: &Modern, at: Pos2, title: Option<&str>, rows: &[(&str, bool)]) -> (Option<usize>, bool) {
    let mut picked = None;
    let sel_id = Id::new("modern context menu sel").with((title, at.x as i32, at.y as i32));
    let usable: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.1).map(|(k, _)| k).collect();
    let mut sel = ctx.data(|d| d.get_temp::<usize>(sel_id)).filter(|k| usable.contains(k)).or(usable.first().copied());
    let (down, up, enter) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter)));
    if let Some(at) = sel.and_then(|s| usable.iter().position(|&k| k == s)) {
        if down {
            sel = Some(usable[(at + 1) % usable.len()]);
        } else if up {
            sel = Some(usable[(at + usable.len() - 1) % usable.len()]);
        }
    }
    if let Some(s) = sel {
        ctx.data_mut(|d| d.insert_temp(sel_id, s));
    }
    if enter {
        picked = sel;
    }
    let font = FontId::proportional(m.theme.ui);
    let head = FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into()));
    let width = |s: &str, f: &FontId| ctx.fonts_mut(|fo| fo.layout_no_wrap(s.to_string(), f.clone(), Color32::WHITE).size().x);
    let wide = rows.iter().map(|(l, _)| width(l, &font)).chain(title.map(|t| width(t, &head))).fold(110.0f32, f32::max) + 16.0;
    let shown = egui::Area::new(Id::unique("modern context menu").with((title, rows.len()))).order(Order::Foreground).fixed_pos(at + vec2(2.0, 2.0)).constrain(true).show(ctx, |ui| {
        let bg = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(t) = title {
                let (r, _) = ui.allocate_exact_size(vec2(wide, m.theme.small + 10.0), Sense::hover());
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, t, head.clone(), m.theme.color("gold"));
                ui.painter().hline(r.min.x..=r.max.x, r.max.y - 1.0, egui::Stroke::new(1.0, m.theme.color("bronze").gamma_multiply(0.6)));
                ui.add_space(3.0);
            }
            for (k, (label, usable)) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(wide, m.theme.ui + 12.0), if *usable { Sense::click() } else { Sense::hover() });
                let lit = *usable && (resp.hovered() || sel == Some(k));
                if lit {
                    ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 4.0, m.theme.color("focus").gamma_multiply(0.35));
                }
                let colour = if !*usable {
                    m.theme.color("muted")
                } else if lit {
                    m.theme.color("gold")
                } else {
                    m.theme.color("text")
                };
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, *label, font.clone(), colour);
                if resp.clicked() {
                    picked = Some(k);
                }
            }
        });
        let r = inner.response.rect;
        ui.painter().set(bg, super::borders::window(m, "window_normal", r));
        r
    });
    let away = ctx.input(|i| (i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.inner.contains(p))) || i.key_pressed(egui::Key::Escape));
    (picked, picked.is_some() || away)
}

/// Opens a menu (the chat window's closes: one menu at a time).
pub fn open(m: &Modern, menu: Menu) {
    *m.chat_menu.borrow_mut() = None;
    *m.ctx_menu.borrow_mut() = Some(menu);
}

/// The open menu drawn and its pick done. A menu whose subject has gone (the Items window closed, the actor out of
/// sight, the member left) closes.
pub fn draw(m: &Modern, ctx: &egui::Context, w: &mut World) {
    if let Some(id) = w.actor_menu.take()
        && let Some(at) = ctx.input(|i| i.pointer.latest_pos())
    {
        open(m, Menu::Actor { at, id });
    }
    let Some(menu) = m.ctx_menu.borrow().clone() else { return };
    let (title, rows) = match &menu {
        Menu::Item { bag, index, id, moving, .. } => match item_rows(w, *bag, *index, *id, *moving) {
            Some(r) => (None, r),
            None => return close(m),
        },
        Menu::Party { slot, .. } => match party_rows(w, *slot) {
            Some(r) => r,
            None => return close(m),
        },
        Menu::Actor { id, .. } => match actor_rows(w, *id) {
            Some(r) => r,
            None => return close(m),
        },
    };
    let at = match menu {
        Menu::Item { at, .. } | Menu::Party { at, .. } | Menu::Actor { at, .. } => at,
    };
    let labels: Vec<(&str, bool)> = rows.iter().map(|(l, _, on)| (l.as_str(), *on)).collect();
    let (picked, closing) = popup(ctx, m, at, title.as_deref(), &labels);
    let act = picked.and_then(|k| rows.get(k)).map(|r| r.1);
    // Move to and Back swap the rows in place.
    if let (Some(a @ (Act::MoveTo | Act::Back)), Menu::Item { at, bag, index, id, .. }) = (act, &menu) {
        *m.ctx_menu.borrow_mut() = Some(Menu::Item { at: *at, bag: *bag, index: *index, id: *id, moving: a == Act::MoveTo });
        return;
    }
    if closing {
        close(m);
    }
    if let Some(a) = act {
        run(w, &menu, a);
    }
}

fn close(m: &Modern) {
    *m.ctx_menu.borrow_mut() = None;
}

/// The Items window's state (none but the plain list, or the action menu's direct list, with nothing else up).
fn items_list(w: &World) -> Option<bool> {
    match w.win.as_ref()? {
        Win::Items { store: None, sub: None, sort: None, manual: None, direct, .. } => Some(*direct),
        _ => None,
    }
}

/// The slot's row in the list shown, while it is still there with the same item.
fn item_row(w: &World, bag: u8, index: u8, id: u16) -> Option<usize> {
    let direct = items_list(w)?;
    // Inventory rows only (and the direct list's wardrobe rows): the temporary items (listed first) keep retail's keys.
    if bag == gitems::TEMP || (bag != gitems::INVENTORY && !direct) {
        return None;
    }
    gitems::listed_at(w, direct).iter().position(|(c, s)| *c == bag && s.index == index && s.id == id)
}

/// A storage view state for the item rules (gitems::usable / tint): `bank` against the inventory, away from the
/// moogle.
fn away(bank: u8) -> Storage {
    Storage { bank, side: gitems::INVENTORY, scroll: 0, cursor: 0, on_bank: false, qty: None, from_menu: false, at_moogle: false, nomad: false, read_only: false, sortw: None, manual: None, stora: None, pending: None, note: None }
}

/// The bags the slot may move to now: open to us, with room, and taking the item (retail's storage colours).
fn destinations(w: &World, index: u8) -> Vec<(u8, &'static str)> {
    let (Some(info), Some(slot)) = (w.item_info.as_ref(), gitems::store_rows(w, gitems::INVENTORY).into_iter().find(|s| s.index == index)) else { return Vec::new() };
    BAGS.iter().copied().filter(|&(c, _)| gitems::usable(w, &away(0xff), c) && gitems::tint(w, info, &away(c), false, &slot) != crate::inventory::TINT_DIM).collect()
}

fn item_rows(w: &World, bag: u8, index: u8, id: u16, moving: bool) -> Option<Vec<Row>> {
    let direct = items_list(w)?;
    item_row(w, bag, index, id)?;
    if moving {
        let mut rows: Vec<Row> = destinations(w, index).into_iter().map(|(c, name)| (name.to_string(), Act::Move(c), true)).collect();
        if rows.is_empty() {
            rows.push(("No bag takes it here".into(), Act::Back, false));
        }
        rows.push(("Back".into(), Act::Back, true));
        return Some(rows);
    }
    let [can_use, can_drop] = gitems::iuse_states(w, bag, index, id);
    let bazaar = super::items::bazaar_price(w, bag, index).is_some();
    if direct {
        return Some(vec![("Use".into(), Act::Use, can_use && !bazaar)]);
    }
    let movable = !bazaar && !w.inv.equip.contains(&Some((gitems::INVENTORY, index)));
    Some(vec![
        ("Use".into(), Act::Use, can_use && !bazaar),
        ("Drop".into(), Act::Drop, can_drop),
        ("Sort".into(), Act::Sort, true),
        ("Move to...".into(), Act::MoveTo, movable),
    ])
}

/// A party window slot's member: their name, server id when in sight (us included).
fn member(w: &World, slot: usize) -> Option<(String, Option<u32>)> {
    let m = w.party.slots().slots.get(slot).cloned().flatten()?;
    let name = if slot == 0 { w.name.clone() } else { m.name.clone() };
    Some((name, target::slot_target(w, slot)))
}

fn party_rows(w: &World, slot: usize) -> Option<(Option<String>, Vec<Row>)> {
    let (name, here) = member(w, slot)?;
    let me = slot == 0;
    let mut rows: Vec<Row> = vec![("Target".into(), Act::Target, here.is_some() && target::picks_allowed(w)), ("Check".into(), Act::Check, here.is_some())];
    if !me {
        rows.push(("Tell".into(), Act::Tell, !name.is_empty()));
    }
    // /focustarget takes our party's slots 1..5 (retail's party menu Focus).
    if (1..6).contains(&slot) {
        rows.push(("Focus Target".into(), Act::Focus, true));
    }
    Some((Some(name), rows))
}

/// Players are 0x400..0x6ff.
fn player(e: &super::super::Entity) -> bool {
    (0x400..0x700).contains(&e.index)
}

fn actor_rows(w: &World, id: u32) -> Option<(Option<String>, Vec<Row>)> {
    let e = w.entities.get(&id)?;
    let pick = target::picks_allowed(w);
    let name = textcmd::display_name(w, id);
    let name = if name.is_empty() { e.name.clone() } else { name };
    let rows: Vec<Row> = if e.mob {
        vec![("Target".into(), Act::Target, pick), ("Attack".into(), Act::Attack, e.hpp > 0), ("Check".into(), Act::Check, true)]
    } else if player(e) {
        let in_party = w.party.members().any(|m| m.id == id);
        vec![
            ("Target".into(), Act::Target, pick),
            ("Check".into(), Act::Check, true),
            ("Tell".into(), Act::Tell, !e.name.is_empty()),
            ("Invite to Party".into(), Act::Invite, !in_party && !e.name.is_empty()),
            ("Trade".into(), Act::Trade, true),
        ]
    } else {
        vec![("Talk".into(), Act::Talk, pick), ("Target".into(), Act::Target, pick), ("Trade".into(), Act::Trade, id >> 24 != 0)]
    };
    Some(((!name.is_empty()).then_some(name), rows))
}

/// The chat line opened on "/tell name ".
fn tell(w: &mut World, name: &str) {
    (w.chat_input, w.chat_caret, w.chat_anchor) = (format!("/tell {name} "), None, None);
    w.typing = true;
}

fn run(w: &mut World, menu: &Menu, act: Act) {
    match *menu {
        Menu::Item { bag, index, id, .. } => item_act(w, bag, index, id, act),
        Menu::Party { slot, .. } => {
            let Some((name, here)) = member(w, slot) else { return };
            match act {
                Act::Target => target::pick(w, here),
                Act::Check => {
                    if let Some(t) = here {
                        textcmd::run(w, &format!("/check {t}"), Mode::Internal);
                    }
                }
                Act::Tell => tell(w, &name),
                Act::Focus => textcmd::run(w, &format!("/focustarget <p{slot}>"), Mode::Internal),
                _ => {}
            }
        }
        Menu::Actor { id, .. } => {
            let Some(name) = w.entities.get(&id).map(|e| e.name.clone()) else { return };
            match act {
                Act::Target => target::pick(w, Some(id)),
                // Talking is the confirm on the target (target.rs confirm_target), as a click on it.
                Act::Talk => {
                    target::pick(w, Some(id));
                    if w.target == Some(id) {
                        target::confirm_target(w);
                    }
                }
                Act::Attack => textcmd::run(w, &format!("/attack {id}"), Mode::Internal),
                Act::Check => textcmd::run(w, &format!("/check {id}"), Mode::Internal),
                Act::Tell => tell(w, &name),
                Act::Invite => textcmd::run(w, &format!("/pcmd add {name}"), Mode::Internal),
                Act::Trade => {
                    trade::start_on(w, Some(id));
                }
                _ => {}
            }
        }
    }
}

/// An item row's pick through the list's keys: the cursor onto the row, then iuse's child or itmsort2's Auto-sort
/// and Enter (Drop and Sort still ask first).
fn item_act(w: &mut World, bag: u8, index: u8, id: u16, act: Act) {
    if let Act::Move(c) = act {
        let n = gitems::store_rows(w, gitems::INVENTORY).into_iter().find(|s| s.index == index && s.id == id).map_or(0, |s| s.count.max(1));
        if n > 0 && destinations(w, index).iter().any(|d| d.0 == c) {
            gitems::send_move(w, gitems::INVENTORY, c, index, n);
            w.sounds.push(crate::audio::Ui::Select as u32);
        }
        return;
    }
    let Some(row) = item_row(w, bag, index, id) else { return };
    let Some(Win::Items { scroll, cursor, sub, sort, direct, .. }) = w.win.as_mut() else { return };
    // The row into view with the cursor on it.
    let rows = crate::inventory::VISIBLE_ROWS;
    if row < *scroll || row >= *scroll + rows {
        *scroll = row.saturating_sub(rows / 2);
    }
    *cursor = row - *scroll;
    match act {
        Act::Use if *direct => {}
        Act::Use => *sub = Some(Sub::Use { child: 1, c: gitems::INVENTORY, index, id }),
        Act::Drop => *sub = Some(Sub::Use { child: 2, c: gitems::INVENTORY, index, id }),
        Act::Sort => *sort = Some(gitems::SortWin { cursor: 1, ask: None }),
        _ => return,
    }
    gitems::key(w, egui::Key::Enter, false, false, false);
}
