//! Type-to-search in the modern lists beyond the magic, ability, item and journal ones (listsearch.rs runs it, the
//! lists draw its hits in place of their rows): each list's rows as the search sees them, and a pick through the
//! list's cursor and Enter (what a click on the row does).

use super::super::listsearch::{Entry, List, count_note};
use super::super::{World, bluset, equipwin, pupset, shop};

pub const SEARCH_LISTS: &[List] = &[
    List {
        tag: "storage",
        open: super::storage::search_open,
        rows: super::storage::search_rows,
        pick: super::storage::search_pick,
    },
    List { tag: "equip", open: equipwin::search_open, rows: equip_rows, pick: equipwin::search_pick },
    List { tag: "shop", open: shop::search_open, rows: shop_rows, pick: shop::search_pick },
    List {
        tag: "synth",
        open: super::craft::synth_open,
        rows: super::craft::synth_rows,
        pick: super::craft::synth_pick,
    },
    List {
        tag: "auction",
        open: super::auction::search_open,
        rows: super::auction::search_rows,
        pick: super::auction::search_pick,
    },
    List {
        tag: "loot",
        open: super::loot::search_open,
        rows: super::loot::search_rows,
        pick: super::loot::search_pick,
    },
    List {
        tag: "points",
        open: super::points::search_open,
        rows: super::points::search_rows,
        pick: super::points::search_pick,
    },
    List {
        tag: "macros",
        open: super::macros::search_open,
        rows: super::macros::search_rows,
        pick: super::macros::search_pick,
    },
    List {
        tag: "people",
        open: super::social::search_open,
        rows: super::social::search_rows,
        pick: super::social::search_pick,
    },
    List {
        tag: "small",
        open: super::minor::search_open,
        rows: super::minor::search_rows,
        pick: super::minor::search_pick,
    },
    List { tag: "bluset", open: bluset::search_open, rows: bluset::search_rows, pick: bluset::search_pick },
    List { tag: "pupset", open: pupset::search_open, rows: pupset::search_rows, pick: pupset::search_pick },
    List {
        tag: "mannequin",
        open: super::inspect::search_open,
        rows: super::inspect::search_rows,
        pick: super::inspect::search_pick,
    },
    List {
        tag: "delivery",
        open: super::post::search_open,
        rows: super::post::search_rows,
        pick: super::post::search_pick,
    },
    List {
        tag: "trade",
        open: super::craft::trade_open,
        rows: super::craft::trade_rows,
        pick: super::craft::trade_pick,
    },
];

/// The Equipment list's pieces: where each is.
fn equip_rows(w: &World) -> Vec<Entry> {
    equipwin::search_list(w)
        .into_iter()
        .map(|x| Entry::item(w, x.slot.id).note(super::equip::bag(x.c)).dim(x.reach == equipwin::Reach::Far))
        .collect()
}

/// A shop's rows (a bazaar's, Set Prices'): the price or count as the note.
fn shop_rows(w: &World) -> Vec<Entry> {
    let Some(v) = shop::look(w) else { return Vec::new() };
    v.rows
        .into_iter()
        .map(|r| {
            let e = match r.text {
                Some(t) => Entry::named(t),
                None => Entry::item(w, r.id),
            };
            let note = r.price.unwrap_or_else(|| r.count.map_or(String::new(), count_note));
            Entry { item: (r.id != 0).then_some(r.id), ..e }.note(note).dim(r.tint == crate::inventory::TINT_DIM)
        })
        .collect()
}
