//! Game actions for mods (docs/lua-mods.md 4.8), each through the code the player's own menus, keys and text commands
//! take: the same checks, refusal lines and packets. Queued by the `actions` namespace, applied after the mod phase.

use super::super::{World, game_ui::GameUi, net::NetCommand, textcmd};
use glam::Vec3;
use mlua::{Lua, Table, Value};

/// A target as a mod gave it: a server id, a tag ("<t>", "<me>", "<bt>", ...) or a name; none takes the command's own
/// default (mostly the current target).
pub(in crate::client) type Target = Option<String>;

pub(in crate::client) enum Action {
    /// The magic menu's checks, then /magic as its prompt sends it.
    Cast(u16, Target),
    /// The ability menu's checks (job ability, weapon skill, pet command, by record id), then its command.
    Ability(u16, Target),
    /// A text command line, run as the menus run theirs (Internal: server ids allowed).
    Line(String),
    UseSlot { bag: u8, index: u8, target: Option<u32> },
    /// (equipment slot, piece by bag and index or by item id; none unequips).
    Equip(Vec<(usize, Piece)>),
    EquipSet(usize),
    Move { from: u8, to: u8, index: u8, count: u32 },
    Sort(u8),
    Drop(u8),
    Lot(u8),
    Pass(u8),
    Cancel(u16),
    Choose(usize),
    /// The event's waiting page goes on, as Enter does.
    Advance,
    /// A macro of the current set run (0..9 Ctrl, 10..19 Alt), the book and set switched, a line or title edited.
    Macro(usize),
    MacroSet(usize, usize),
    MacroEdit { n: usize, line: Option<usize>, text: String },
    Talk(u32),
    Target(Option<u32>),
    Lock(bool),
    /// A BLU slot (0..19) set to a spell id or emptied (0), a PUP slot (0..13) to an item id or emptied.
    BlueSlot(usize, u16),
    PupSlot(usize, u16),
    Scan,
    Track(u16),
    Untrack,
    Run(Option<f32>),
    RunTo(f32, f32, f32),
    Follow(u32),
    Walk(bool),
    Stop,
    Turn(f32),
    /// A party invite by name: someone in the zone directly, anyone else through the server's name lookup.
    Invite(String),
    /// A main menu row (page, child), as a pick there runs it; the nomad moogle menu.
    Menu(u8, u16),
    Moogle,
}

/// The main menu's rows by name: (name, page, child).
pub(super) const MENUS: &[(&str, u8, u16)] = &[
    ("status", 0, 1),
    ("equipment", 0, 2),
    ("magic", 0, 3),
    ("items", 0, 4),
    ("abilities", 0, 5),
    ("party", 0, 6),
    ("trade", 0, 7),
    ("search", 0, 8),
    ("linkshell", 0, 9),
    ("synthesis", 0, 10),
    ("region", 0, 11),
    ("map", 0, 12),
    ("missions", 1, 1),
    ("quests", 1, 2),
    ("key_items", 1, 3),
    ("view_house", 1, 4),
    ("bazaar", 1, 5),
    ("macros", 1, 6),
    ("config", 1, 7),
    ("help_desk", 1, 8),
    ("communication", 1, 9),
    ("clock", 1, 10),
    ("shut_down", 1, 11),
    ("log_out", 1, 12),
];

pub(in crate::client) enum Piece {
    At(u8, u8),
    Item(u16),
    None,
}

/// The bags gear is worn from: the inventory and the wardrobes.
const WEAR_BAGS: [u8; 9] = [0, 8, 10, 11, 12, 13, 14, 15, 16];

/// The target when a mod gives none, from the action's valid targets (1 self): ourselves for a self-only action,
/// else the current target, else ourselves when we may be its target.
fn default_target(w: &World, targets: u16) -> Target {
    Some(if targets == 1 || (w.target.is_none() && targets & 1 != 0) { "<me>" } else { "<t>" }.to_string())
}

/// A target a mod named must be there: a tag (<t>, <me>...) passes, an id or a name must be in the zone (the text
/// commands would otherwise fall back to the current target).
fn check_target(lua: &Lua, t: &Target) -> mlua::Result<()> {
    let Some(s) = t.as_deref() else { return Ok(()) };
    if s.starts_with('<') {
        return Ok(());
    }
    let w = super::api::world(lua)?;
    let here = match s.parse::<u32>() {
        Ok(id) => id == w.my_id || w.entities.contains_key(&id),
        Err(_) => s.eq_ignore_ascii_case(&w.name) || w.entities.values().any(|e| e.name.eq_ignore_ascii_case(s)),
    };
    if here { Ok(()) } else { Err(mlua::Error::runtime(format!("no {s} in the zone"))) }
}

fn token(t: &Target) -> String {
    t.as_deref().map_or(String::new(), |s| if s.contains(' ') && !s.starts_with('<') { format!(" \"{s}\"") } else { format!(" {s}") })
}

pub(super) fn apply(w: &mut World, gu: Option<&GameUi>, a: Action) {
    use super::super::{equipsets, equipwin, event, items, magic, target, treasure};
    match a {
        Action::Cast(spell, t) => {
            if let Some(m) = magic::magic_check(w, spell) {
                magic::refuse(w, m);
                return;
            }
            let name = magic::spell_name(w, spell);
            let targets = w.actions.as_ref().and_then(|x| x.spells.iter().find(|s| s.id == spell)).map_or(0x20, |s| s.targets);
            let t = t.or_else(|| default_target(w, targets));
            textcmd::run_unfiltered(w, &format!("/magic \"{name}\"{}", token(&t)), textcmd::Mode::Internal);
        }
        Action::Ability(id, t) => {
            let Some(i) = w.actions.as_ref().and_then(|x| x.abilities.iter().position(|a| a.id == id)) else { return };
            let Some(a) = w.actions.as_ref().and_then(|x| x.abilities.get(i)).cloned() else { return };
            // A category row (Blood Pact: Rage, ...) is a list, not an action.
            if a.sub != 0 {
                return;
            }
            if let Some(m) = magic::ability_check(w, i as u16) {
                if m != magic::SILENT {
                    magic::refuse(w, m);
                }
                return;
            }
            let name = magic::ability_name(w, i as u16);
            let t = t.or_else(|| default_target(w, a.targets));
            let cmd = if a.kind == 3 { "/weaponskill" } else if a.kind == 0x14 { "/monsterskill" } else { "/jobability" };
            textcmd::run_unfiltered(w, &format!("{cmd} \"{name}\"{}", token(&t)), textcmd::Mode::Internal);
        }
        Action::Line(line) => textcmd::run_unfiltered(w, &line, textcmd::Mode::Internal),
        Action::UseSlot { bag, index, target } => {
            // As /item: the status gate, then the use (target: ourselves when none).
            if !textcmd::status_gate(w, 0x7f) {
                return;
            }
            let t = target.unwrap_or(w.my_id);
            items::use_slot(w, bag, index, t);
        }
        Action::Equip(pieces) => {
            // As /equip: the status gate, then each piece as the equipment window equips it.
            if !textcmd::status_gate(w, 0x46) {
                return;
            }
            // The server answers each piece later: the copies picked here are not picked again for another slot.
            let mut taken: Vec<(u8, u8)> = Vec::new();
            for (slot, p) in pieces {
                let worn = w.inv.equip.get(slot).copied().flatten();
                let at = match p {
                    Piece::None => {
                        if worn.is_some() {
                            equipwin::unequip(w, slot);
                        }
                        continue;
                    }
                    Piece::At(b, i) => Some((b, i)),
                    // The worn piece if it is this item, else the first not worn elsewhere.
                    Piece::Item(id) => worn.filter(|&(b, i)| w.inv.item(b, i) == Some(id) && !taken.contains(&(b, i))).or_else(|| {
                        WEAR_BAGS
                            .iter()
                            .flat_map(|&b| (1..=80u8).map(move |i| (b, i)))
                            .find(|&(b, i)| w.inv.item(b, i) == Some(id) && !w.inv.equip.contains(&Some((b, i))) && !taken.contains(&(b, i)))
                    }),
                };
                let Some((b, i)) = at else { continue };
                taken.push((b, i));
                // Already worn there: nothing (try_equip would take it off).
                if worn == Some((b, i)) {
                    continue;
                }
                if let Some(id) = w.inv.item(b, i) {
                    equipwin::try_equip(w, slot, b, i, id);
                }
            }
        }
        Action::EquipSet(n) => {
            if textcmd::status_gate(w, 0x46) {
                equipsets::equip(w, n, true);
            }
        }
        Action::Move { from, to, index, count } => {
            if w.inv.item(from, index).is_some() {
                items::send_move(w, from, to, index, count);
            }
        }
        Action::Sort(bag) => items::auto_sort_c(w, bag),
        Action::Drop(index) => {
            if let Some(id) = w.inv.item(0, index) {
                items::drop_now(w, 0, index, id);
            }
        }
        Action::Lot(slot) | Action::Pass(slot) if slot as usize >= treasure::SLOTS || w.treasure[slot as usize].state != 2 => {}
        Action::Lot(slot) => {
            let item = w.treasure[slot as usize].item;
            treasure::cast_lot(w, slot, item);
        }
        Action::Pass(slot) => treasure::pass(w, slot),
        Action::Cancel(id) => {
            if let Some(gu) = gu {
                super::super::buffs::cancel(gu, w, id);
            }
        }
        Action::Menu(page, id) => w.menu_click = Some((page, id)),
        Action::Invite(name) => {
            let here = w.entities.values().any(|e| e.name.eq_ignore_ascii_case(&name));
            if here {
                textcmd::run_unfiltered(w, &format!("/pcmd add {name}"), textcmd::Mode::Internal);
            } else {
                super::super::search::lookup(w, &name, super::super::search::Then::PartyAdd);
            }
        }
        Action::Moogle => super::super::moogle::open(w, true),
        Action::Advance => {
            if event::active(w) {
                event::key(w, egui::Key::Enter);
            }
        }
        Action::Macro(n) => {
            if let Some(m) = w.macros.as_mut() {
                m.start_for_mod(n);
            }
        }
        Action::MacroSet(book, set) => {
            if let Some(m) = w.macros.as_mut() {
                m.switch(book.min(39), set.min(9));
            }
        }
        Action::MacroEdit { n, line, text } => {
            let mon = super::super::monstrosity::active(w);
            if let Some(m) = w.macros.as_mut() {
                let b = crate::dat::font::encode_sjis(&text);
                let ok = match line {
                    Some(k) => m.set_line(n, k, &b),
                    None => m.set_title(n, &b),
                };
                if ok {
                    m.save(mon);
                }
            }
        }
        Action::Choose(k) => {
            if event::choice(w).is_some_and(|c| k < c.options.len()) {
                event::set_choice(w, k);
                event::key(w, egui::Key::Enter);
            }
        }
        Action::Talk(id) => {
            event::talk(w, id);
        }
        Action::Target(t) => target::set(w, t),
        Action::BlueSlot(i, spell) => {
            if let Err(e) = super::super::bluset::set_slot(w, i, spell) {
                w.chat.push_colour(format!("actions.{}: {e}", if spell == 0 { "blue_remove" } else { "blue_set" }), super::ERROR);
            }
        }
        Action::PupSlot(i, item) => {
            if let Err(e) = super::super::pupset::set_slot(w, i, item) {
                w.chat.push_colour(format!("actions.{}: {e}", if item == 0 { "detach" } else { "attach" }), super::ERROR);
            }
        }
        Action::Lock(on) => target::lock(w, on, "mod"),
        Action::Scan => super::super::scan_request(w),
        Action::Track(index) => super::super::scan_track(w, index),
        Action::Untrack => {
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x0F6, body: vec![0; 4] });
            w.scan.stop();
        }
        // Movement rides on autorun, as /automove and /follow do: the player's keys and Num7 still steer and stop it.
        // Headings as entity.heading: radians, 0 east, pi / 2 south (FFXI's dir).
        Action::Run(heading) => {
            let h = heading.unwrap_or_else(|| super::super::event::rad_of(w.net.me.lock().map_or(0, |m| m.dir)));
            (w.autorun, w.autorun_dir, w.follow, w.run_to) = (true, Vec3::new(h.cos(), 0.0, -h.sin()), None, None);
        }
        Action::RunTo(x, z, within) => {
            let d = Vec3::new(x - w.pos.x, 0.0, z - w.pos.z);
            // Already there: the run is done, so whatever moved us before (autorun, /follow) stops too.
            if d.length() > within.max(0.1) {
                (w.autorun, w.autorun_dir, w.follow, w.run_to) = (true, d.normalize(), None, Some((x, z, within)));
            } else {
                (w.autorun, w.follow, w.run_to) = (false, None, None);
            }
        }
        Action::Follow(id) => textcmd::run_unfiltered(w, &format!("/follow {id}"), textcmd::Mode::Internal),
        Action::Walk(on) => w.walk = on,
        Action::Stop => {
            w.autorun = false;
            w.run_to = None;
        }
        // Standing still keeps the facing until we move, lock on or the camera turns us.
        Action::Turn(h) => {
            if let Ok(mut me) = w.net.me.lock() {
                me.dir = super::super::dir_from_forward(Vec3::new(h.cos(), 0.0, -h.sin()));
            }
        }
    }
}

/// A target argument: an integer (a server id) or a string (a tag or a name).
fn target_arg(v: Option<Value>) -> mlua::Result<Target> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Integer(i)) => Ok(Some(i.to_string())),
        Some(Value::Number(n)) => Ok(Some((n as i64).to_string())),
        Some(Value::String(s)) => Ok(Some(s.to_str()?.to_string())),
        Some(_) => Err(mlua::Error::runtime("a target is a server id or a tag / name string")),
    }
}

/// Equipment slots by number (0 main .. 15 back) or name.
fn slot_arg(v: &Value) -> mlua::Result<usize> {
    const NAMES: [(&str, usize); 20] = [
        ("main", 0), ("sub", 1), ("range", 2), ("ammo", 3), ("head", 4), ("body", 5), ("hands", 6), ("legs", 7), ("feet", 8), ("neck", 9),
        ("waist", 10), ("left_ear", 11), ("ear1", 11), ("right_ear", 12), ("ear2", 12), ("left_ring", 13), ("ring1", 13), ("right_ring", 14), ("ring2", 14), ("back", 15),
    ];
    match v {
        Value::Integer(i) if (0..16).contains(i) => Ok(*i as usize),
        Value::String(s) => {
            let s = s.to_str()?.to_ascii_lowercase();
            NAMES.iter().find(|n| n.0 == s).map(|n| n.1).ok_or_else(|| mlua::Error::runtime(format!("no equipment slot {s:?}")))
        }
        _ => Err(mlua::Error::runtime("an equipment slot is 0..15 or a name (main, sub, range, ammo, head, ...)")),
    }
}

/// An item to wear: an id, {bag = , index = }, or false / 0 to take the slot off.
fn piece_arg(v: &Value) -> mlua::Result<Piece> {
    match v {
        Value::Nil | Value::Boolean(false) | Value::Integer(0) => Ok(Piece::None),
        Value::Integer(i) => u16::try_from(*i).map(Piece::Item).map_err(|_| mlua::Error::runtime(format!("no item id {i} (ids are 1..65535)"))),
        Value::Table(t) => Ok(Piece::At(t.get("bag")?, t.get("index")?)),
        _ => Err(mlua::Error::runtime("a piece is an item id, {bag = , index = }, or false")),
    }
}

fn queue(lua: &Lua, a: Action) -> mlua::Result<()> {
    super::api::need_action(lua)?;
    super::api::push_op(lua, super::Op::Act(a))?;
    Ok(())
}

/// The `actions` namespace: every call needs actions (from the player's input) or automation, and is carried out
/// after the mod phase.
pub(super) fn install(lua: &Lua, g: &Table) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set("cast", lua.create_function(|lua, (spell, to): (u16, Option<Value>)| queue(lua, Action::Cast(spell, target_arg(to)?)))?)?;
    for name in ["ability", "weaponskill"] {
        t.set(name, lua.create_function(|lua, (id, to): (u16, Option<Value>)| queue(lua, Action::Ability(id, target_arg(to)?)))?)?;
    }
    t.set(
        "ranged",
        lua.create_function(|lua, to: Option<Value>| {
            let t = target_arg(to)?.unwrap_or_else(|| "<t>".into());
            queue(lua, Action::Line(format!("/range{}", token(&Some(t)))))
        })?,
    )?;
    t.set(
        "use_item",
        lua.create_function(|lua, (item, to): (u16, Option<Value>)| {
            super::api::need_action(lua)?;
            let w = super::api::world(lua)?;
            let it = w.item_info.as_ref().and_then(|c| c.item(item)).ok_or_else(|| mlua::Error::runtime(format!("no item {item}")))?;
            let (name, targets) = (it.name.clone(), it.targets);
            let t = target_arg(to)?.or_else(|| default_target(w, targets));
            queue(lua, Action::Line(format!("/item \"{name}\"{}", token(&t))))
        })?,
    )?;
    t.set("use_slot", lua.create_function(|lua, (bag, index, to): (u8, u8, Option<u32>)| queue(lua, Action::UseSlot { bag, index, target: to }))?)?;
    t.set(
        "attack",
        lua.create_function(|lua, to: Option<Value>| {
            let t = target_arg(to)?;
            check_target(lua, &t)?;
            queue(lua, Action::Line(if t.is_some() { format!("/attack{}", token(&t)) } else { "/attack on".into() }))
        })?,
    )?;
    t.set("disengage", lua.create_function(|lua, ()| queue(lua, Action::Line("/attack off".into())))?)?;
    t.set(
        "assist",
        lua.create_function(|lua, to: Option<Value>| {
            let t = target_arg(to)?;
            check_target(lua, &t)?;
            queue(lua, Action::Line(format!("/assist{}", token(&t))))
        })?,
    )?;
    t.set("target", lua.create_function(|lua, id: Option<u32>| queue(lua, Action::Target(id)))?)?;
    t.set("lock", lua.create_function(|lua, on: bool| queue(lua, Action::Lock(on)))?)?;
    t.set(
        "heal",
        lua.create_function(|lua, on: Option<bool>| queue(lua, Action::Line(match on { None => "/heal".into(), Some(true) => "/heal on".into(), Some(false) => "/heal off".into() })))?,
    )?;
    t.set(
        "equip",
        lua.create_function(|lua, (a, b): (Value, Option<Value>)| {
            let pieces = match (&a, b) {
                (Value::Table(set), None) => set.pairs::<Value, Value>().map(|kv| kv.and_then(|(k, v)| Ok((slot_arg(&k)?, piece_arg(&v)?)))).collect::<mlua::Result<Vec<_>>>()?,
                (_, Some(p)) => vec![(slot_arg(&a)?, piece_arg(&p)?)],
                _ => return Err(mlua::Error::runtime("actions.equip(slot, item) or actions.equip({slot = item, ...})")),
            };
            let mut pieces = pieces;
            pieces.sort_by_key(|p| p.0);
            queue(lua, Action::Equip(pieces))
        })?,
    )?;
    t.set("unequip", lua.create_function(|lua, slot: Value| queue(lua, Action::Equip(vec![(slot_arg(&slot)?, Piece::None)])))?)?;
    t.set(
        "equip_set",
        lua.create_function(|lua, n: usize| {
            if !(1..=200).contains(&n) {
                return Err(mlua::Error::runtime("equip sets are 1..200"));
            }
            queue(lua, Action::EquipSet(n - 1))
        })?,
    )?;
    t.set(
        "move",
        lua.create_function(|lua, (from, index, to, count): (u8, u8, u8, Option<u32>)| {
            let n = count.unwrap_or_else(|| super::api::world(lua).ok().and_then(|w| w.inv.slots.get(&(from, index)).map(|s| s.1)).unwrap_or(1));
            queue(lua, Action::Move { from, to, index, count: n })
        })?,
    )?;
    t.set("sort", lua.create_function(|lua, bag: u8| queue(lua, Action::Sort(bag)))?)?;
    t.set("drop", lua.create_function(|lua, index: u8| queue(lua, Action::Drop(index)))?)?;
    t.set("lot", lua.create_function(|lua, slot: u8| queue(lua, Action::Lot(slot)))?)?;
    t.set("pass", lua.create_function(|lua, slot: u8| queue(lua, Action::Pass(slot)))?)?;
    t.set("cancel", lua.create_function(|lua, id: u16| queue(lua, Action::Cancel(id)))?)?;
    t.set(
        "choose",
        lua.create_function(|lua, k: usize| {
            if k == 0 {
                return Err(mlua::Error::runtime("options count from 1"));
            }
            queue(lua, Action::Choose(k - 1))
        })?,
    )?;
    t.set("talk", lua.create_function(|lua, id: u32| queue(lua, Action::Talk(id)))?)?;
    // Trade and shops send at once, as the windows' confirmations do; a refusal is the mod's error.
    t.set(
        "trade",
        lua.create_function(|lua, (npc, items, gil): (u32, Option<Table>, Option<u32>)| {
            super::api::need_action(lua)?;
            let list: Vec<(u8, u32)> = match items {
                Some(t) => t.sequence_values::<Table>().map(|e| e.and_then(|e| Ok((e.get::<u8>("slot")?, e.get::<Option<u32>>("count")?.unwrap_or(1))))).collect::<mlua::Result<_>>()?,
                None => Vec::new(),
            };
            super::super::trade::hand_over(super::api::world(lua)?, npc, &list, gil.unwrap_or(0)).map_err(|e| mlua::Error::runtime(format!("actions.trade: {e}")))
        })?,
    )?;
    t.set(
        "buy",
        lua.create_function(|lua, (index, qty): (usize, Option<u32>)| {
            super::api::need_action(lua)?;
            let index = index.checked_sub(1).ok_or_else(|| mlua::Error::runtime("rows count from 1"))?;
            super::super::shop::buy(super::api::world(lua)?, index, qty.unwrap_or(1)).map_err(|e| mlua::Error::runtime(format!("actions.buy: {e}")))
        })?,
    )?;
    t.set(
        "sell",
        lua.create_function(|lua, (slot, qty): (u8, Option<u32>)| {
            super::api::need_action(lua)?;
            super::super::shop::sell(super::api::world(lua)?, slot, qty.unwrap_or(1)).map_err(|e| mlua::Error::runtime(format!("actions.sell: {e}")))
        })?,
    )?;
    // Job setups count slots from 1, as every list here does.
    let blue = |slot: usize| if (1..=20).contains(&slot) { Ok(slot - 1) } else { Err(mlua::Error::runtime("blue magic slots are 1..20")) };
    let pup = |slot: usize| if (1..=14).contains(&slot) { Ok(slot - 1) } else { Err(mlua::Error::runtime("automaton slots are 1..14 (1 head, 2 frame, 3..14 attachments)")) };
    t.set(
        "blue_set",
        lua.create_function(move |lua, (slot, spell): (usize, u16)| {
            // 0 empties the slot.
            if spell != 0 && !(0x201..0x300).contains(&spell) {
                return Err(mlua::Error::runtime(format!("spell {spell} is not blue magic")));
            }
            queue(lua, Action::BlueSlot(blue(slot)?, spell))
        })?,
    )?;
    t.set("blue_remove", lua.create_function(move |lua, slot: usize| queue(lua, Action::BlueSlot(blue(slot)?, 0)))?)?;
    t.set(
        "attach",
        lua.create_function(move |lua, (slot, item): (usize, u16)| {
            let i = pup(slot)?;
            let base = if i < 2 { 0x2000 } else { 0x2100 };
            if item != 0 && !(base + 1..base + 0x100).contains(&item) {
                return Err(mlua::Error::runtime(format!("item {item} does not go in automaton slot {slot}")));
            }
            queue(lua, Action::PupSlot(i, item))
        })?,
    )?;
    t.set("detach", lua.create_function(move |lua, slot: usize| queue(lua, Action::PupSlot(pup(slot)?, 0)))?)?;
    t.set(
        "invite",
        lua.create_function(|lua, name: String| queue(lua, Action::Invite(name)))?,
    )?;
    t.set("party_leave", lua.create_function(|lua, ()| queue(lua, Action::Line("/pcmd leave".into())))?)?;
    t.set("party_kick", lua.create_function(|lua, name: String| queue(lua, Action::Line(format!("/pcmd kick {name}"))))?)?;
    t.set("join", lua.create_function(|lua, ()| queue(lua, Action::Line("/join".into())))?)?;
    t.set("decline", lua.create_function(|lua, ()| queue(lua, Action::Line("/decline".into())))?)?;
    t.set("mount", lua.create_function(|lua, name: String| queue(lua, Action::Line(format!("/mount \"{name}\""))))?)?;
    t.set("dismount", lua.create_function(|lua, ()| queue(lua, Action::Line("/dismount".into())))?)?;
    t.set("advance", lua.create_function(|lua, ()| queue(lua, Action::Advance))?)?;
    t.set(
        "open_menu",
        lua.create_function(|lua, name: String| {
            if name == "moogle" {
                return queue(lua, Action::Moogle);
            }
            let Some(&(_, page, id)) = MENUS.iter().find(|m| m.0 == name) else {
                let names: Vec<&str> = MENUS.iter().map(|m| m.0).chain(["moogle"]).collect();
                return Err(mlua::Error::runtime(format!("no menu {name:?} ({})", names.join(", "))));
            };
            queue(lua, Action::Menu(page, id))
        })?,
    )?;
    // A macro of the current set: row "ctrl" | "alt", 1..10.
    let slot = |row: &str, n: usize| -> mlua::Result<usize> {
        let base = match row {
            "ctrl" => 0,
            "alt" => 10,
            _ => return Err(mlua::Error::runtime("a macro row is \"ctrl\" or \"alt\"")),
        };
        if !(1..=10).contains(&n) {
            return Err(mlua::Error::runtime("a macro is 1..10"));
        }
        Ok(base + n - 1)
    };
    t.set("macro", lua.create_function(move |lua, (row, n): (String, usize)| queue(lua, Action::Macro(slot(&row, n)?)))?)?;
    t.set("macro_set", lua.create_function(|lua, (book, set): (usize, usize)| queue(lua, Action::MacroSet(book.clamp(1, 40) - 1, set.clamp(1, 10) - 1)))?)?;
    t.set(
        "macro_edit",
        lua.create_function(move |lua, (row, n, line, text): (String, usize, Value, String)| {
            let line = match line {
                Value::Integer(k) if (1..=6).contains(&k) => Some(k as usize - 1),
                Value::String(s) if s.to_str()?.as_ref() as &str == "title" => None,
                _ => return Err(mlua::Error::runtime("a macro line is 1..6, or \"title\"")),
            };
            // The player runs macros: a /lua line there would run as theirs.
            if line.is_some() && super::lua_line(&text) {
                return Err(mlua::Error::runtime("actions.macro_edit: a /lua line is the player's to write"));
            }
            queue(lua, Action::MacroEdit { n: slot(&row, n)?, line, text })
        })?,
    )?;
    t.set("scan", lua.create_function(|lua, ()| queue(lua, Action::Scan))?)?;
    t.set("track", lua.create_function(|lua, index: u16| queue(lua, Action::Track(index)))?)?;
    t.set("untrack", lua.create_function(|lua, ()| queue(lua, Action::Untrack))?)?;
    t.set("run", lua.create_function(|lua, heading: Option<f32>| queue(lua, Action::Run(heading)))?)?;
    t.set("run_to", lua.create_function(|lua, (x, z, within): (f32, f32, Option<f32>)| queue(lua, Action::RunTo(x, z, within.unwrap_or(1.0))))?)?;
    t.set(
        "follow",
        lua.create_function(|lua, id: u32| {
            check_target(lua, &Some(id.to_string()))?;
            queue(lua, Action::Follow(id))
        })?,
    )?;
    t.set("walk", lua.create_function(|lua, on: bool| queue(lua, Action::Walk(on)))?)?;
    t.set("stop", lua.create_function(|lua, ()| queue(lua, Action::Stop))?)?;
    t.set("turn", lua.create_function(|lua, heading: f32| queue(lua, Action::Turn(heading)))?)?;
    g.set("actions", t)?;
    Ok(())
}
