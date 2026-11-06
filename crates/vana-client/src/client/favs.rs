//! Favourites (not retail; the modern UI's plan, docs/modern-ui.md "Favourites"): spells, abilities and items starred
//! in their lists (a click on a row's star, or Ctrl+F on the cursor's row or the search's hit), kept per character in
//! config `modern_favourites` with the list search's keys (listsearch::key_of: "m:" spell, "a:" ability, "i:" item).
//! Favourites rank first in the list search, and the modern layer's favourites strip runs them as a macro line would
//! (/ma, /ja, /ws, /pet, /item), so every check and the target prompt stay retail's.

use super::World;
use super::listsearch::{self, Hit};
use super::magic::{self, Kind};
use super::textcmd::{self, Mode};
use crate::audio::Ui as Se;

/// Favourites kept at most.
pub const MAX: usize = 24;

/// What a favourite is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fav {
    Spell(u16),
    Ability(u16),
    Item(u16),
}

impl Fav {
    pub fn parse(key: &str) -> Option<Fav> {
        let (kind, id) = key.split_once(':')?;
        let id = id.parse().ok()?;
        match kind {
            "m" => Some(Fav::Spell(id)),
            "a" => Some(Fav::Ability(id)),
            "i" => Some(Fav::Item(id)),
            _ => None,
        }
    }

    pub fn key(self) -> String {
        match self {
            Fav::Spell(id) => format!("m:{id}"),
            Fav::Ability(id) => format!("a:{id}"),
            Fav::Item(id) => format!("i:{id}"),
        }
    }
}

/// A list row that can be starred: spells, abilities but traits (kind 4), items. Mounts and emotes can't.
pub fn of_row(w: &World, kind: Kind, id: u16) -> Option<Fav> {
    match kind {
        Kind::Magic => Some(Fav::Spell(id)),
        Kind::Ability => w.actions.as_ref().and_then(|a| a.abilities.get(id as usize)).filter(|a| a.kind != 4).map(|_| Fav::Ability(id)),
        _ => None,
    }
}

/// A search hit that can be starred: a spell, an ability, an inventory item.
pub fn of_hit(w: &World, h: &Hit) -> Option<Fav> {
    match (h.journal, h.slot) {
        (Some(_), _) => None,
        (None, Some(_)) => Some(Fav::Item(h.id)),
        (None, None) => of_row(w, h.kind, h.id),
    }
}

pub fn is(w: &World, f: Fav) -> bool {
    w.favourites.iter().any(|k| *k == f.key())
}

/// Starred or unstarred (the newest last, as the strip shows them); saved with the config.
pub fn toggle(w: &mut World, f: Fav) {
    let key = f.key();
    if let Some(i) = w.favourites.iter().position(|k| *k == key) {
        w.favourites.remove(i);
        w.sounds.push(Se::Cancel as u32);
    } else if w.favourites.len() < MAX {
        w.favourites.push(key);
        w.sounds.push(Se::Select as u32);
    } else {
        w.sounds.push(Se::Error as u32);
        return;
    }
    w.fav_save = true;
}

/// Ctrl+F in a list: the search's highlighted hit, else the list's cursor row, else the item list's. True when a row
/// was there to star.
pub fn toggle_current(w: &mut World) -> bool {
    let f = if let Some(s) = w.list_search.clone() {
        listsearch::hits(w, &s.text).get(s.sel).and_then(|h| of_hit(w, h))
    } else if let Some(win) = w.ability.as_ref().or(w.magic.as_ref()).filter(|l| !l.hidden) {
        win.rows.get(win.scroll + win.cursor).and_then(|&id| of_row(w, win.kind, id))
    } else if let Some(super::items::Win::Items { scroll, cursor, direct, store: None, .. }) = w.win.as_ref() {
        super::items::listed(w, *direct).get(scroll + cursor).map(|s| Fav::Item(s.id))
    } else {
        None
    };
    match f {
        Some(f) => {
            toggle(w, f);
            true
        }
        None => false,
    }
}

/// Favourites rank above everything else among the matches and are marked.
pub fn boost(w: &World, hits: &mut [Hit]) {
    for h in hits.iter_mut() {
        if of_hit(w, h).is_some_and(|f| is(w, f)) {
            h.score += 1000;
            h.fav = true;
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score));
}

/// The favourites in order.
pub fn all(w: &World) -> Vec<Fav> {
    w.favourites.iter().filter_map(|k| Fav::parse(k)).collect()
}

/// An item's name and how many we carry (inventory, temporary items and the wardrobes /item looks in).
pub fn item(w: &World, id: u16) -> (String, u32) {
    let name = w.item_info.as_ref().and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default();
    let n = w.inv.slots.iter().filter(|(k, v)| v.0 == id && matches!(k.0, 0 | 3 | 8 | 10..=16)).map(|(_, v)| v.1.max(1)).sum();
    (name, n)
}

pub fn name(w: &World, f: Fav) -> String {
    match f {
        Fav::Spell(id) => magic::spell_name(w, id),
        Fav::Ability(id) => magic::ability_name(w, id),
        Fav::Item(id) => item(w, id).0,
    }
}

/// The target tag for valid-target bits: ourselves alone needs no prompt; anything else asks (<st>).
fn tag(targets: u16) -> &'static str {
    if targets == 1 { "<me>" } else { "<st>" }
}

/// The macro line that uses it: /ma, /ws, /pet or /ja, /item, with its target tag.
pub fn line(w: &World, f: Fav) -> Option<String> {
    let actions = w.actions.as_ref();
    Some(match f {
        Fav::Spell(id) => format!("/ma \"{}\" {}", magic::spell_name(w, id), tag(actions?.spell(id)?.targets)),
        Fav::Ability(id) => {
            let a = actions?.abilities.get(id as usize)?;
            let cmd = match a.kind {
                3 => "/ws",
                2 => "/pet",
                _ => "/ja",
            };
            format!("{cmd} \"{}\" {}", magic::ability_name(w, id), tag(a.targets))
        }
        Fav::Item(id) => {
            let targets = w.item_info.as_ref().and_then(|c| c.item(id)).map_or(1, |it| it.targets);
            format!("/item \"{}\" {}", item(w, id).0, tag(targets))
        }
    })
}

/// A favourite used from the strip: its line run as a macro's.
pub fn run(w: &mut World, f: Fav) {
    if let Some(l) = line(w, f).filter(|l| !l.contains("\"\"")) {
        log::info!("favourite: {l}");
        textcmd::run(w, &l, Mode::Macro);
    }
}

/// `/favs [on | off]`: the strip shown or hidden; alone it toggles.
pub fn command(w: &mut World, arg: Option<&str>) {
    w.fav_strip = match arg.map(str::to_ascii_lowercase).as_deref() {
        Some("on") => true,
        Some("off") => false,
        _ => !w.fav_strip,
    };
    w.fav_save = true;
}

#[cfg(test)]
mod tests {
    use super::{Fav, tag};

    #[test]
    fn keys() {
        for f in [Fav::Spell(1), Fav::Ability(0x210), Fav::Item(4509)] {
            assert_eq!(Fav::parse(&f.key()), Some(f));
        }
        assert_eq!(Fav::parse("q:1:2"), None);
        assert_eq!(Fav::parse("m:x"), None);
    }

    #[test]
    fn tags() {
        assert_eq!(tag(1), "<me>");
        assert_eq!(tag(0x1f), "<st>");
        assert_eq!(tag(0x20), "<st>");
    }
}
