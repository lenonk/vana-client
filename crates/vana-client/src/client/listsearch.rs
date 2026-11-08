//! Type-to-search in the magic and ability windows (not retail; the modern layer's, docs/modern-ui.md): typing while
//! a category window or a list is open searches the names; from a category window every list behind it. A pick goes
//! through retail's own keys (the category, the row, Enter), so every check and the target prompt still apply.
//! The command palette (Ctrl+Space in the field, modern) is the same search over everything the action menu reaches:
//! spells, abilities, usable items and the menu's commands; a pick opens what the menu would and acts through it.

use super::game_ui::GameUi;
use super::magic::{self, Cat, Kind};
use super::suggest::Action;
use super::{World, items, journal, keyitems, missions, quests};
use crate::audio::Ui as Se;

/// A suggestion taken from the battle menu (suggest.rs): the menu suspended and Magic or Abilities chosen as their
/// rows do, then `pick_action` in the window that opened.
pub fn pick_root(w: &mut World, action: Action, target: Option<&str>) {
    (w.suggest_sel, w.suggest_sub) = (None, None);
    w.sounds.push(Se::Select as u32);
    let c = if matches!(action, Action::Spell(_)) { 8 } else { 7 };
    w.held_menu = w.action_menu.take();
    super::combat::choose(w, c);
    pick_action(w, action);
    // The member it names starts under the target prompt's cursor.
    if let Some(id) = target.and_then(|n| w.party.party().find(|m| m.name == n).map(|m| m.id)) {
        super::target::prefer(w, id);
    }
}

/// A suggestion taken: its list opened from the category window when need be, the cursor on it, Enter.
pub fn pick_action(w: &mut World, action: Action) {
    (w.suggest_sel, w.suggest_sub) = (None, None);
    let (kind, id) = match action {
        Action::Spell(id) => (Kind::Magic, id),
        Action::Ability(id) => (Kind::Ability, id),
    };
    let in_list = w.ability.as_ref().or(w.magic.as_ref()).is_some_and(|l| l.kind == kind && l.rows.contains(&id));
    let cat = (!in_list).then(|| w.cats).flatten().and_then(|c| {
        let ty = match action {
            Action::Spell(s) => w.actions.as_ref()?.spell(s)?.kind,
            Action::Ability(a) => w.actions.as_ref()?.abilities.get(a as usize)?.kind as u16,
        };
        magic::cat_rows(c.menu).iter().position(|&(_, cat)| match cat {
            Cat::Magic(t, _) => kind == Kind::Magic && t == ty,
            Cat::Ability(t) => kind == Kind::Ability && (t == ty || (t == 3 && ty == 3)),
            _ => false,
        })
    });
    if in_list || cat.is_some() {
        pick(
            w,
            &Hit {
                kind,
                cat,
                id,
                name: String::new(),
                score: 0,
                slot: None,
                note: String::new(),
                journal: None,
                recent: false,
                fav: false,
            },
            None,
        );
    }
}

/// The search: its text and the highlighted hit.
#[derive(Clone, Debug, Default)]
pub struct ListSearch {
    pub text: String,
    pub sel: usize,
}

/// A row found: its list's kind (and the category row it is under when searching from a category window), id, name and
/// score; an item's inventory slot (its first, the item list showing one hit per item) and its quantity.
#[derive(Clone, Debug)]
pub struct Hit {
    pub kind: Kind,
    pub cat: Option<usize>,
    pub id: u16,
    pub name: String,
    pub score: i32,
    pub slot: Option<u8>,
    pub note: String,
    /// A journal row: a key item (`cat` its category), a quest or a mission.
    pub journal: Option<Journal>,
    /// Picked lately (World::recent_picks): ranked up and marked.
    pub recent: bool,
    /// A favourite (favs.rs): ranked above the rest and marked.
    pub fav: bool,
}

/// Where a journal hit is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Journal {
    KeyItem,
    /// The area and the entry (current first).
    Quest(usize, usize),
    /// The log and the entry.
    Mission(usize, usize),
    /// An auction house item (not retail) and its bid category.
    Auction(u8),
    /// A row of one of the other lists (LISTS): the list, the row, whether `id` is an item (its icon), whether the
    /// list greys the row.
    Row {
        list: u8,
        k: usize,
        item: bool,
        dim: bool,
    },
    /// The command palette's action menu command (combat::cmd), or HEAL.
    Command(u8),
}

/// A row of one of the other lists as the search sees it: its name, the item it is (its icon), its note (a count,
/// where it is) and whether its list greys it.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    pub item: Option<u16>,
    pub note: String,
    pub dim: bool,
}

impl Entry {
    pub fn named(name: impl Into<String>) -> Self {
        Entry { name: name.into(), ..Default::default() }
    }

    /// An item's row, named from the item table.
    pub fn item(w: &World, id: u16) -> Self {
        let name = w.item_info.as_ref().and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default();
        Entry { name, item: Some(id), ..Default::default() }
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
}

/// A count as a note: none for one.
pub fn count_note(n: u32) -> String {
    if n > 1 { n.to_string() } else { String::new() }
}

/// One of the other lists: its tag (the recent picks' key), whether it has the keys now (nothing over it), its rows,
/// and a row picked (the cursor onto it, then what Enter or a click on it does).
pub struct List {
    pub tag: &'static str,
    pub open: fn(&World) -> bool,
    pub rows: fn(&World) -> Vec<Entry>,
    pub pick: fn(&mut World, Option<&GameUi>, usize),
}

/// The other list that has the keys now.
pub fn other(w: &World) -> Option<usize> {
    if !w.modern {
        return None;
    }
    super::modern::SEARCH_LISTS.iter().position(|l| (l.open)(w))
}

/// A list's tag.
pub fn tag(list: usize) -> &'static str {
    super::modern::SEARCH_LISTS.get(list).map_or("", |l| l.tag)
}

/// The palette's /heal (not an action menu row).
const HEAL: u8 = 0xfe;

/// The palette's commands: the action menu's rows that act at once, and /heal.
const COMMANDS: [(u8, &str); 6] = [
    (super::combat::cmd::ATTACK, "Attack"),
    (super::combat::cmd::DISENGAGE, "Disengage"),
    (super::combat::cmd::SWITCH_TARGET, "Switch Target"),
    (super::combat::cmd::CHECK, "Check"),
    (super::combat::cmd::CALL_HELP, "Call for Help"),
    (HEAL, "Heal"),
];

/// A palette command the action menu would offer now (its rows for this target and state; Call for help with
/// Disengage), so a pick does only what that row would. /heal checks itself.
fn command_now(w: &World, c: u8) -> bool {
    c == HEAL || command_in(&super::combat::current_items(w, false), c)
}

fn command_in(rows: &[u8], c: u8) -> bool {
    c == HEAL
        || rows.contains(&c)
        || (c == super::combat::cmd::CALL_HELP && rows.contains(&super::combat::cmd::DISENGAGE))
}

/// How long the palette's hits stand for one query (every spell, ability and item scored again after).
const PALETTE_FRESH: std::time::Duration = std::time::Duration::from_millis(250);

/// Where the palette may open: modern, in the field, alive, nothing open or typed, not under a prompt or Monstrosity.
pub fn palette_allowed(w: &World) -> bool {
    w.modern
        && !w.typing
        && !w.dead()
        && w.prompt.is_none()
        && w.confirm.is_none()
        && w.win.is_none()
        && w.menu.is_none()
        && w.cats.is_none()
        && w.magic.is_none()
        && w.ability.is_none()
        && !super::event::active(w)
        && !super::target::sub_up(w)
        && !super::monstrosity::active(w)
}

/// Opens the command palette (an empty search: recent picks first).
pub fn open_palette(w: &mut World) {
    w.command_palette = true;
    w.list_search = Some(ListSearch::default());
    w.sounds.push(Se::Select as u32);
}

fn close_palette(w: &mut World) {
    w.command_palette = false;
    w.list_search = None;
}

/// The palette's candidates: every spell and ability the menus list, the usable items (one per item, the total as the
/// note) and the commands. An empty query keeps only the recent picks.
fn palette_matches(w: &World, query: &str) -> Vec<Hit> {
    if let Some((q, at, hits)) = &*w.palette_cache.borrow()
        && q == query
        && at.elapsed() < PALETTE_FRESH
    {
        return hits.clone();
    }
    let out = palette_scored(w, query);
    *w.palette_cache.borrow_mut() = Some((query.to_string(), std::time::Instant::now(), out.clone()));
    out
}

fn palette_scored(w: &World, query: &str) -> Vec<Hit> {
    let mut out = Vec::new();
    let mut push = |kind: Kind, id: u16, name: String, slot: Option<u8>, note: String, journal: Option<Journal>| {
        let mut h = Hit { kind, cat: None, id, name, score: 0, slot, note, journal, recent: false, fav: false };
        let score =
            if query.is_empty() { w.recent_picks.contains(&key_of(&h)).then_some(0) } else { score(&h.name, query) };
        if let Some(score) = score {
            h.score = score;
            out.push(h);
        }
    };
    let rows = super::combat::current_items(w, false);
    for &(c, label) in COMMANDS.iter().filter(|(c, _)| command_in(&rows, *c)) {
        push(Kind::Ability, c as u16, label.to_string(), None, "command".into(), Some(Journal::Command(c)));
    }
    for &(_, cat) in magic::cat_rows("menu    magselec") {
        if let Cat::Magic(ty, list) = cat {
            for id in magic::shown(w, Kind::Magic, magic::magic_rows(w, ty, list, 0)) {
                push(Kind::Magic, id, name(w, Kind::Magic, id), None, magic::type_label(ty).into(), None);
            }
        }
    }
    for &(_, cat) in magic::cat_rows("menu    abiselec") {
        if let Cat::Ability(ty) = cat {
            for id in magic::shown(w, Kind::Ability, magic::ability_rows(w, ty)) {
                push(Kind::Ability, id, name(w, Kind::Ability, id), None, magic::ability_label(ty).into(), None);
            }
        }
    }
    let info = w.item_info.as_ref();
    let mut seen: Vec<(u16, u8, u32)> = Vec::new();
    for s in items::listed(w, true) {
        match seen.iter_mut().find(|(id, ..)| *id == s.id) {
            Some((.., n)) => *n += s.count.max(1),
            None => seen.push((s.id, s.index, s.count.max(1))),
        }
    }
    for (id, slot, n) in seen {
        let name = info.and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default();
        push(Kind::Ability, id, name, Some(slot), if n > 1 { format!("item · {n}") } else { "item".into() }, None);
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

/// Rows a search shows at most.
pub const MAX: usize = 12;

/// Lower case, roman numerals written as digits ("Cure IV" -> "cure 4").
fn normal(s: &str) -> String {
    s.to_lowercase()
        .split(' ')
        .map(|w| match w {
            "ii" => "2",
            "iii" => "3",
            "iv" => "4",
            "v" => "5",
            "vi" => "6",
            "vii" => "7",
            "viii" => "8",
            w => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Edits between two words, a swap of neighbours counting one (optimal string alignment).
fn edits(a: &[char], b: &[char]) -> usize {
    // One flat table (a row a Vec was a.len() + 2 allocations a call, for every row of a list searched).
    let w = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * w];
    let at = |i: usize, j: usize| i * w + j;
    for i in 0..=a.len() {
        d[at(i, 0)] = i;
    }
    for j in 0..=b.len() {
        d[at(0, j)] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[at(i - 1, j)] + 1).min(d[at(i, j - 1)] + 1).min(d[at(i - 1, j - 1)] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[at(i - 2, j - 2)] + 1);
            }
            d[at(i, j)] = v;
        }
    }
    d[at(a.len(), b.len())]
}

/// The typo fallback: every query word within an edit or two (by its length) of a name word or its start; the fewer
/// edits the better. None: some word too far off.
fn typo_score(name: &str, q: &str) -> Option<i32> {
    let words: Vec<Vec<char>> =
        name.split([' ', '-', ':', '\'']).filter(|w| !w.is_empty()).map(|w| w.chars().collect()).collect();
    let mut total = 0;
    for qw in q.split(' ').filter(|w| !w.is_empty()) {
        let qc: Vec<char> = qw.chars().collect();
        let allowed = if qc.len() <= 3 {
            0
        } else if qc.len() <= 5 {
            1
        } else {
            2
        };
        // Against each name word cut near the query word's length (a word typed part way).
        let best = words
            .iter()
            .map(|w| {
                (qc.len().saturating_sub(1)..=w.len().min(qc.len() + 1))
                    .map(|n| edits(&qc, &w[..n.min(w.len())]))
                    .min()
                    .unwrap_or(usize::MAX)
            })
            .min()?;
        if best > allowed {
            return None;
        }
        total += best as i32;
    }
    Some(200 - 40 * total - name.chars().count() as i32)
}

/// How well `name` matches `query` (higher better): its start, a word's start, anywhere, its letters in order; with
/// and without spaces, roman numerals as digits; failing those, words an edit or two off (typos). None: no match.
pub fn score(name: &str, query: &str) -> Option<i32> {
    let q = normal(query.trim());
    if q.is_empty() {
        return None;
    }
    score_forms(name, &query_forms(&q))
}

/// A normalised query's forms for score_forms: without its spaces, then as it is.
fn query_forms(q: &str) -> [String; 2] {
    [q.replace(' ', ""), q.to_string()]
}

/// `score` with the query's forms worked out (with and without spaces): one list's rows share them.
fn score_forms(name: &str, qs: &[String; 2]) -> Option<i32> {
    let (lower, norm) = (name.to_lowercase(), normal(name));
    let (lower_tight, norm_tight) = (lower.replace(' ', ""), norm.replace(' ', ""));
    let mut best = None;
    for f in [&lower, &lower_tight, &norm, &norm_tight] {
        for q in qs.iter().map(String::as_str) {
            let len = f.chars().count() as i32;
            let s = if f.starts_with(&q) {
                1000 - len
            } else if f.split([' ', '-', ':', '\'']).any(|w| w.starts_with(&q)) {
                800 - len
            } else if let Some(at) = f.find(&q) {
                600 - at as i32 - len
            } else {
                let mut chars = f.chars();
                let mut gaps = 0;
                let all = q.chars().all(|c| {
                    let mut skipped = 0;
                    for x in chars.by_ref() {
                        if x == c {
                            return true;
                        }
                        skipped += 1;
                    }
                    gaps += skipped;
                    false
                });
                if !all {
                    continue;
                }
                300 - gaps - len
            };
            best = Some(best.map_or(s, |b: i32| b.max(s)));
        }
    }
    best.or_else(|| typo_score(&norm, &qs[1]))
}

fn name(w: &World, kind: Kind, id: u16) -> String {
    match kind {
        Kind::Magic => magic::spell_name(w, id),
        Kind::Ability => magic::ability_name(w, id),
        Kind::Mount => magic::mount_name(w, id),
        Kind::Emote => {
            magic::emote_entry(w, id).and_then(|e| w.names.as_ref()?.emote_text(e.name as u32)).unwrap_or_default()
        }
    }
}

/// Recent picks remembered at most.
const RECENT: usize = 24;

/// A hit's key among the recent picks: its kind and id (an item by its id, a journal row by its place).
pub fn key_of(h: &Hit) -> String {
    match (h.journal, h.slot, h.kind) {
        (Some(Journal::KeyItem), _, _) => format!("k:{}", h.id),
        (Some(Journal::Quest(a, e)), _, _) => format!("q:{a}:{e}"),
        (Some(Journal::Mission(l, e)), _, _) => format!("n:{l}:{e}"),
        (Some(Journal::Auction(_)), _, _) => format!("h:{}", h.id),
        (Some(Journal::Command(c)), _, _) => format!("c:{c}"),
        (Some(Journal::Row { list, .. }), _, _) => format!("r:{}:{}", tag(list as usize), h.name),
        (None, Some(_), _) => format!("i:{}", h.id),
        (None, None, Kind::Magic) => format!("m:{}", h.id),
        (None, None, Kind::Ability) => format!("a:{}", h.id),
        (None, None, Kind::Mount) => format!("o:{}", h.id),
        (None, None, Kind::Emote) => format!("e:{}", h.id),
    }
}

/// Recent picks rank up (the newest most) and are marked; best first again, the list's order among equals.
fn boost(recent: &[String], hits: &mut [Hit]) {
    for h in hits.iter_mut() {
        let key = key_of(h);
        if let Some(r) = recent.iter().position(|k| *k == key) {
            h.score += 400 - 15 * r as i32;
            h.recent = true;
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score));
}

/// The hits for the open window, best first: recent picks first among matches (boost), then the scores, then the list's
/// own order.
pub fn hits(w: &World, query: &str) -> Vec<Hit> {
    let mut out = matches(w, query);
    boost(&w.recent_picks, &mut out);
    super::favs::boost(w, &mut out);
    out
}

/// A pick remembered as the newest (the oldest beyond RECENT forgotten); saved with the config.
fn remember(w: &mut World, h: &Hit) {
    let key = key_of(h);
    w.recent_picks.retain(|k| *k != key);
    w.recent_picks.insert(0, key);
    w.recent_picks.truncate(RECENT);
    w.recent_save = true;
}

/// The matches for the open window by score (the list's own order among equals).
fn matches(w: &World, query: &str) -> Vec<Hit> {
    // The query's forms once for every row (each row worked them out again).
    let q = normal(query.trim());
    let qs = (!q.is_empty()).then(|| query_forms(&q));
    if w.command_palette {
        return palette_matches(w, query);
    }
    let mut out = Vec::new();
    // One of the other lists: its rows.
    if let Some(l) = other(w) {
        for (k, e) in (super::modern::SEARCH_LISTS[l].rows)(w).into_iter().enumerate() {
            if let Some(score) = qs.as_ref().and_then(|qs| score_forms(&e.name, qs)) {
                let journal = Some(Journal::Row { list: l as u8, k, item: e.item.is_some(), dim: e.dim });
                out.push(Hit {
                    kind: Kind::Ability,
                    cat: None,
                    id: e.item.unwrap_or(0),
                    name: e.name,
                    score,
                    slot: None,
                    note: e.note,
                    journal,
                    recent: false,
                    fav: false,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    // The auction house's bid menus: every item of every category (its place as the note, filled by the modern layer);
    // none until the index is in.
    if super::auction::searchable(w) {
        let info = w.item_info.as_ref();
        for &(id, cat) in super::auction::index(w).iter().flat_map(|v| v.iter()) {
            let name = info.and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default();
            if let Some(score) = qs.as_ref().and_then(|qs| score_forms(&name, qs)) {
                out.push(Hit {
                    kind: Kind::Ability,
                    cat: None,
                    id,
                    name,
                    score,
                    slot: None,
                    note: String::new(),
                    journal: Some(Journal::Auction(cat)),
                    recent: false,
                    fav: false,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    // The key items we hold, over every category (the category as the note).
    if key_items(w) {
        for (k, (title, items)) in keyitems::all(w).iter().enumerate() {
            for (id, name) in items.iter().filter(|(id, _)| w.has_key_item(*id)) {
                if let Some(score) = qs.as_ref().and_then(|qs| score_forms(name, qs)) {
                    out.push(Hit {
                        kind: Kind::Ability,
                        cat: Some(k),
                        id: *id,
                        name: name.clone(),
                        score,
                        slot: None,
                        note: title.clone(),
                        journal: Some(Journal::KeyItem),
                        recent: false,
                        fav: false,
                    });
                }
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    // The quests we have or have done, in every area (current or completed as the note).
    if quest_list(w) {
        let logs = w.logs.clone();
        for area in (0..11).filter(|&a| quests::area_on(w.expansions, a)) {
            let Some(f) = logs.as_ref().and_then(|l| l.quests.get(area)) else { continue };
            let (rows, split) = quests::merged(w, area);
            for (k, i) in rows.into_iter().enumerate() {
                let Some(e) = f.entries.get(i) else { continue };
                let name = e.name.trim_start_matches('+').to_string();
                if let Some(score) = qs.as_ref().and_then(|qs| score_forms(&name, qs)) {
                    let note = (if k < split { "current" } else { "completed" }).to_string();
                    out.push(Hit {
                        kind: Kind::Ability,
                        cat: None,
                        id: 0,
                        name,
                        score,
                        slot: None,
                        note,
                        journal: Some(Journal::Quest(area, i)),
                        recent: false,
                        fav: false,
                    });
                }
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    // The current and completed missions of every log.
    if mission_list(w) {
        let logs = w.logs.clone();
        for log in missions::shown_logs(w) {
            let Some(f) = logs.as_ref().and_then(|l| l.missions.get(log)) else { continue };
            for r in journal::mission_rows(&w.journal, log, f) {
                let (journal::Row::Current(i) | journal::Row::Done(i)) = r else { continue };
                let Some(e) = f.entries.get(i) else { continue };
                let name = journal::indent(&e.name).0.to_string();
                if let Some(score) = qs.as_ref().and_then(|qs| score_forms(&name, qs)) {
                    out.push(Hit {
                        kind: Kind::Ability,
                        cat: None,
                        id: 0,
                        name,
                        score,
                        slot: None,
                        note: String::new(),
                        journal: Some(Journal::Mission(log, i)),
                        recent: false,
                        fav: false,
                    });
                }
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    if let Some(direct) = item_list(w) {
        let mut seen: Vec<(u16, u32)> = Vec::new();
        let info = w.item_info.as_ref();
        for s in items::listed(w, direct) {
            match seen.iter_mut().find(|(id, _)| *id == s.id) {
                Some((_, n)) => *n += s.count.max(1),
                None => seen.push((s.id, s.count.max(1))),
            }
            let n = info.and_then(|c| c.item(s.id)).map(|it| it.name.clone()).unwrap_or_default();
            if out.iter().all(|h: &Hit| h.id != s.id)
                && let Some(score) = qs.as_ref().and_then(|qs| score_forms(&n, qs))
            {
                out.push(Hit {
                    kind: Kind::Ability,
                    cat: None,
                    id: s.id,
                    name: n,
                    score,
                    slot: Some(s.index),
                    note: String::new(),
                    journal: None,
                    recent: false,
                    fav: false,
                });
            }
        }
        for h in &mut out {
            let n = seen.iter().find(|(id, _)| *id == h.id).map_or(1, |x| x.1);
            h.note = if n > 1 { n.to_string() } else { String::new() };
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        return out;
    }
    let mut add = |kind: Kind, cat: Option<usize>, rows: Vec<u16>| {
        for id in rows {
            let n = name(w, kind, id);
            if let Some(score) = qs.as_ref().and_then(|qs| score_forms(&n, qs)) {
                out.push(Hit {
                    kind,
                    cat,
                    id,
                    name: n,
                    score,
                    slot: None,
                    note: String::new(),
                    journal: None,
                    recent: false,
                    fav: false,
                });
            }
        }
    };
    if let Some(win) = w.ability.as_ref().or(w.magic.as_ref()) {
        add(win.kind, None, win.rows.clone());
    } else if let Some(c) = w.cats {
        for (k, &(_, cat)) in magic::cat_rows(c.menu).iter().enumerate() {
            match cat {
                Cat::Magic(ty, list) => add(
                    Kind::Magic,
                    Some(k),
                    magic::shown(w, Kind::Magic, magic::magic_rows(w, ty, list && !super::monstrosity::active(w), 0)),
                ),
                Cat::Ability(ty) => {
                    add(Kind::Ability, Some(k), magic::shown(w, Kind::Ability, magic::ability_rows(w, ty)))
                }
                Cat::Mount => add(Kind::Mount, Some(k), magic::shown(w, Kind::Mount, magic::mount_rows(w))),
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

/// The item list (direct: the action menu's) when it has the focus: no iuse, sort window or Manual Sort over it.
fn item_list(w: &World) -> Option<bool> {
    match w.win.as_ref()? {
        items::Win::Items { sub: None, sort: None, manual: None, store: None, direct, .. } => Some(*direct),
        _ => None,
    }
}

/// A window the search can run in: a category window, an unhidden list or the item list, with nothing over it.
pub fn can_run(w: &World) -> bool {
    let list = w.ability.as_ref().or(w.magic.as_ref());
    w.modern
        && w.prompt.is_none()
        && w.confirm.is_none()
        && !super::typing_elsewhere(w)
        && ((w.command_palette && palette_allowed(w))
            || other(w).is_some()
            || super::auction::searchable(w)
            || w.cats.is_some()
            || list.is_some_and(|l| !l.hidden && l.sort_focus.is_none() && l.ask.is_none() && l.manual == 0)
            || (w.win.is_some() && item_list(w).is_some())
            || key_items(w)
            || quest_list(w)
            || mission_list(w))
}

/// The modern quest list (no Records of Eminence list over it).
fn quest_list(w: &World) -> bool {
    w.quests.as_ref().is_some_and(|q| q.state == 1 && q.roe.is_none() && q.list.as_ref().is_some_and(|l| l.state == 0))
}

/// The modern mission list.
fn mission_list(w: &World) -> bool {
    w.missions.as_ref().and_then(|m| m.list.as_ref()).is_some_and(|l| l.state == 1)
}

/// The key items window on its categories or a category's list (no question up).
fn key_items(w: &World) -> bool {
    w.keyitems.as_ref().is_some_and(|k| matches!(k.state, 1 | 2) && k.ask.is_none())
}

/// The search's keys and typed text this frame. True when it took them (the game then sees none).
pub fn events(w: &mut World, gu: &GameUi, events: &[egui::Event]) -> bool {
    if !can_run(w) {
        w.list_search = None;
        w.command_palette = false;
        return false;
    }
    // Ctrl+F stars or unstars the row under the cursor (favs.rs).
    let star = events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::F, pressed: true, modifiers, .. } if modifiers.command || modifiers.ctrl));
    if star && super::favs::toggle_current(w) {
        return true;
    }
    if w.command_palette && w.list_search.is_none() {
        w.list_search = Some(ListSearch::default());
    }
    let starts = events
        .iter()
        .any(|e| matches!(e, egui::Event::Text(t) if t.chars().next().is_some_and(|c| c.is_alphanumeric())));
    if w.list_search.is_none() && !starts {
        return false;
    }
    for e in events {
        // A typed letter's key press comes before its text: only a search its keys ended stops the walk.
        let was = w.list_search.is_some();
        match e {
            egui::Event::Text(t) => {
                let s = w.list_search.get_or_insert_with(Default::default);
                s.text.push_str(t);
                s.sel = 0;
            }
            egui::Event::Key { key, pressed: true, .. } => key_in(w, gu, *key),
            _ => {}
        }
        if was && w.list_search.is_none() {
            break;
        }
    }
    true
}

/// Up / Down move among the hits, Enter picks, Backspace deletes a letter, Escape ends the search.
fn key_in(w: &mut World, gu: &GameUi, key: egui::Key) {
    let Some(s) = w.list_search.clone() else { return };
    let n = hits(w, &s.text).len().min(MAX);
    match key {
        // An empty field (the auction's, reached with Up) gives the keys back with Down; the palette's empty search
        // lists the recent picks, which Down walks.
        egui::Key::ArrowDown if s.text.is_empty() && !w.command_palette => {
            w.list_search = None;
            w.sounds.push(Se::Cursor as u32);
        }
        egui::Key::ArrowDown if n > 0 => {
            set_sel(w, (s.sel + 1) % n);
            w.sounds.push(Se::Cursor as u32);
        }
        egui::Key::ArrowUp if n > 0 => {
            set_sel(w, (s.sel + n - 1) % n);
            w.sounds.push(Se::Cursor as u32);
        }
        egui::Key::Enter => {
            if let Some(h) = hits(w, &s.text).into_iter().nth(s.sel) {
                pick(w, &h, Some(gu));
            }
        }
        // The palette stays up with an empty field; Backspace on it, or Escape, closes it.
        egui::Key::Backspace if w.command_palette && s.text.is_empty() => {
            close_palette(w);
            w.sounds.push(Se::Cancel as u32);
        }
        egui::Key::Backspace => {
            let mut t = s.text;
            t.pop();
            w.list_search = (!t.is_empty() || w.command_palette).then_some(ListSearch { text: t, sel: 0 });
        }
        egui::Key::Escape => {
            close_palette(w);
            w.sounds.push(Se::Cancel as u32);
        }
        _ => {}
    }
}

pub fn set_sel(w: &mut World, sel: usize) {
    if let Some(s) = w.list_search.as_mut() {
        s.sel = sel;
    }
}

/// A hit picked: from a category window its list opens first (Enter on its row); then the cursor goes to the row and
/// Enter acts on it, as the keys would.
pub fn pick(w: &mut World, h: &Hit, gu: Option<&GameUi>) {
    if w.list_search.take().is_some() {
        remember(w, h);
    }
    // From the palette: a command acts, a spell or ability opens its menu as a suggestion's pick does, an item opens the
    // direct-use list and is picked there as below.
    if std::mem::take(&mut w.command_palette) {
        match (h.journal, h.slot, h.kind) {
            (Some(Journal::Command(HEAL)), ..) => {
                w.sounds.push(Se::Select as u32);
                super::textcmd::run(w, "/heal", super::textcmd::Mode::Internal);
                return;
            }
            (Some(Journal::Command(c)), ..) => {
                // Checked again: the target or the state may have changed since the list was made.
                if !command_now(w, c) || !super::combat::choose(w, c) {
                    w.sounds.push(Se::Error as u32);
                    return;
                }
                w.sounds.push(Se::Select as u32);
                return;
            }
            (_, Some(_), _) => items::open_direct(w),
            (_, None, Kind::Magic) => return pick_root(w, Action::Spell(h.id), None),
            (_, None, _) => return pick_root(w, Action::Ability(h.id), None),
        }
    }
    match h.journal {
        Some(Journal::Row { list, k, .. }) => {
            if let Some(l) = super::modern::SEARCH_LISTS.get(list as usize) {
                (l.pick)(w, gu, k);
            }
            return;
        }
        Some(Journal::Auction(cat)) => {
            super::auction::show(w, cat, h.id);
            return;
        }
        Some(Journal::Quest(area, entry)) => {
            quests::show_area(w, area, Some(entry));
            w.sounds.push(Se::Select as u32);
            return;
        }
        Some(Journal::Mission(log, entry)) => {
            missions::show_log(w, log, Some(entry));
            w.sounds.push(Se::Select as u32);
            return;
        }
        _ => {}
    }
    // A key item: its category opens with the cursor on it, at the page's top.
    if h.journal == Some(Journal::KeyItem) {
        keyitems::show_category(w, h.cat.unwrap_or(0));
        if let Some(k) = w.keyitems.as_mut()
            && let Some(i) = k.items.iter().position(|(id, _)| *id == h.id)
        {
            (k.top, k.cursor) = (i, i);
        }
        w.sounds.push(Se::Select as u32);
        return;
    }
    if let (Some(slot), Some(direct)) = (h.slot, item_list(w)) {
        let Some(k) = items::listed(w, direct).iter().position(|s| s.index == slot && s.id == h.id) else { return };
        if let Some(items::Win::Items { scroll, cursor, .. }) = w.win.as_mut() {
            let vis = crate::inventory::VISIBLE_ROWS;
            *scroll = if k < *scroll {
                k
            } else if k >= *scroll + vis {
                k + 1 - vis
            } else {
                *scroll
            };
            *cursor = k - *scroll;
        }
        items::key(w, egui::Key::Enter, false, false, false);
        return;
    }
    if let (Some(k), Some(c)) = (h.cat, w.cats) {
        w.cats = Some(magic::Cats { sel: k, ..c });
        magic::key(w, egui::Key::Enter);
    }
    let win = if w.ability.is_some() { w.ability.as_mut() } else { w.magic.as_mut() };
    if let Some(win) = win.filter(|x| x.kind == h.kind)
        && magic::focus_row(win, h.id)
    {
        magic::remember(w);
        magic::key(w, egui::Key::Enter);
    }
}

#[cfg(test)]
mod tests {
    use super::{Hit, Kind, boost, key_of, score};

    /// A swapped pair and a dropped letter still find their item; a word too far off does not.
    #[test]
    fn typos() {
        assert!(score("Bunch of Gysahl Greens", "ghysal greens").is_some());
        assert!(score("Gysahl Greens", "gysal greens").is_some());
        assert!(score("Gysahl Greens", "ghysal").is_some());
        assert!(score("Gysahl Greens", "carrot").is_none());
        assert!(score("Fire Crystal", "fire").unwrap() > score("Fire Crystal", "fier").unwrap());
    }

    fn hit(id: u16, score: i32) -> Hit {
        Hit {
            kind: Kind::Magic,
            cat: None,
            id,
            name: String::new(),
            score,
            slot: None,
            note: String::new(),
            journal: None,
            recent: false,
            fav: false,
        }
    }

    #[test]
    fn recent_first() {
        // Cure (a start, 996) beats Cure IV (993) until Cure IV was picked lately; the newest pick ranks highest.
        let mut hits = vec![hit(1, 996), hit(4, 993), hit(3, 994)];
        boost(&[key_of(&hit(4, 0)), key_of(&hit(3, 0))], &mut hits);
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [4, 3, 1]);
        assert!(hits[0].recent && hits[1].recent && !hits[2].recent);
        // An item and a spell of the same id are different picks.
        let item = Hit { slot: Some(3), ..hit(4, 0) };
        assert_ne!(key_of(&item), key_of(&hit(4, 0)));
    }

    #[test]
    fn scores() {
        // A start beats a word's start beats anywhere beats letters in order; numerals match digits.
        assert!(score("Haste", "has").unwrap() > score("Phalanx", "ha").unwrap_or(0));
        assert!(score("Cure IV", "cure4").is_some());
        assert!(score("Cure IV", "cure 4").is_some());
        assert!(score("Cure IV", "c4").is_some());
        assert!(score("Cure IV", "cure").unwrap() > score("Curaga", "cure").unwrap_or(i32::MIN));
        assert!(score("Protect II", "pro").unwrap() > score("Blink", "lin").unwrap());
        assert!(score("Blizzard", "fire").is_none());
        assert!(score("Dia", "ia").unwrap() < score("Dia", "di").unwrap());
        assert_eq!(score("Stone", ""), None);
    }
}
