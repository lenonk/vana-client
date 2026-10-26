//! Type-to-search in the magic and ability windows (not retail; the modern layer's, docs/modern-ui.md): typing while
//! a category window or a list is open searches the names; from a category window every list behind it. A pick goes
//! through retail's own keys (the category, the row, Enter), so every check and the target prompt still apply.

use super::magic::{self, Cat, Kind};
use super::{World, items};
use super::suggest::{self, Action, Suggestion};
use crate::audio::Ui as Se;

/// The window a suggestion snapshot was taken for: the category window's menu, the list's kind and type.
pub type WindowKey = (Option<&'static str>, Option<(u8, u16)>);

fn window_key(w: &World) -> WindowKey {
    (w.cats.map(|c| c.menu), w.ability.as_ref().or(w.magic.as_ref()).map(|l| (l.kind as u8, l.ty)))
}

/// Whether a window shows magic (else abilities): the list's kind, else the category window's.
fn magic_window(w: &World) -> bool {
    match w.ability.as_ref().or(w.magic.as_ref()) {
        Some(l) => l.kind == Kind::Magic,
        None => w.cats.is_some_and(|c| matches!(c.menu, "menu    mgcmenu" | "menu    magselec")),
    }
}

/// The open window's suggestions: retaken when the window opens or changes (a safe point), held while it is
/// navigated; the magic windows show spells, the ability windows abilities.
pub fn suggestions(w: &mut World) -> Vec<Suggestion> {
    let key = window_key(w);
    if key == (None, None) {
        (w.suggestions, w.suggest_sel) = (None, None);
        return Vec::new();
    }
    if w.suggestions.as_ref().is_none_or(|(k, _)| *k != key) {
        let magic = magic_window(w);
        let all = suggest::suggest(&suggest::facts(w));
        let mine = all.into_iter().filter(|s| matches!(s.action, Action::Spell(_)) == magic).collect();
        (w.suggestions, w.suggest_sel) = (Some((key, mine)), None);
    }
    w.suggestions.as_ref().map_or(Vec::new(), |(_, s)| s.clone())
}

/// A suggestion taken: its list opened from the category window when need be, the cursor on it, Enter.
pub fn pick_action(w: &mut World, action: Action) {
    w.suggest_sel = None;
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
        pick(w, &Hit { kind, cat, id, name: String::new(), score: 0, slot: None, note: String::new() });
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

/// How well `name` matches `query` (higher better): its start, a word's start, anywhere, its letters in order; with
/// and without spaces, roman numerals as digits. None: no match.
pub fn score(name: &str, query: &str) -> Option<i32> {
    let q = normal(query.trim());
    if q.is_empty() {
        return None;
    }
    let forms = [name.to_lowercase(), normal(name)];
    let mut best = None;
    for f in forms.iter().flat_map(|f| [f.clone(), f.replace(' ', "")]) {
        for q in [q.clone(), q.replace(' ', "")] {
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
    best
}

fn name(w: &World, kind: Kind, id: u16) -> String {
    match kind {
        Kind::Magic => magic::spell_name(w, id),
        Kind::Ability => magic::ability_name(w, id),
        Kind::Mount => magic::mount_name(w, id),
        Kind::Emote => magic::emote_entry(w, id).and_then(|e| w.names.as_ref()?.emote_text(e.name as u32)).unwrap_or_default(),
    }
}

/// The hits for the open window, best first (the list's own order among equals).
pub fn hits(w: &World, query: &str) -> Vec<Hit> {
    let mut out = Vec::new();
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
                && let Some(score) = score(&n, query)
            {
                out.push(Hit { kind: Kind::Ability, cat: None, id: s.id, name: n, score, slot: Some(s.index), note: String::new() });
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
            if let Some(score) = score(&n, query) {
                out.push(Hit { kind, cat, id, name: n, score, slot: None, note: String::new() });
            }
        }
    };
    if let Some(win) = w.ability.as_ref().or(w.magic.as_ref()) {
        add(win.kind, None, win.rows.clone());
    } else if let Some(c) = w.cats {
        for (k, &(_, cat)) in magic::cat_rows(c.menu).iter().enumerate() {
            match cat {
                Cat::Magic(ty, list) => add(Kind::Magic, Some(k), magic::magic_rows(w, ty, list && !super::monstrosity::active(w), 0)),
                Cat::Ability(ty) => add(Kind::Ability, Some(k), magic::ability_rows(w, ty)),
                Cat::Mount => add(Kind::Mount, Some(k), magic::mount_rows(w)),
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
        && !w.typing
        && (w.cats.is_some() || list.is_some_and(|l| !l.hidden && l.sort_focus.is_none() && l.ask.is_none() && l.manual == 0) || (w.win.is_some() && item_list(w).is_some()))
}

/// The search's keys and typed text this frame, and Tab's suggestions. True when it took them (the game then sees
/// none).
pub fn events(w: &mut World, events: &[egui::Event]) -> bool {
    if !can_run(w) {
        (w.list_search, w.suggest_sel) = (None, None);
        return false;
    }
    if w.list_search.is_none() && suggest_keys(w, events) {
        return true;
    }
    let starts = events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.chars().next().is_some_and(|c| c.is_alphanumeric())));
    if w.list_search.is_none() && !starts {
        return false;
    }
    for e in events {
        match e {
            egui::Event::Text(t) => {
                let s = w.list_search.get_or_insert_with(Default::default);
                s.text.push_str(t);
                s.sel = 0;
            }
            egui::Event::Key { key, pressed: true, .. } => key_in(w, *key),
            _ => {}
        }
        if w.list_search.is_none() {
            break;
        }
    }
    true
}

/// Tab into the suggestions (and back); while there, Up / Down move, Enter takes one, Escape leaves. True when used.
fn suggest_keys(w: &mut World, events: &[egui::Event]) -> bool {
    let list = suggestions(w);
    let mut used = false;
    for e in events {
        let egui::Event::Key { key, pressed: true, .. } = e else { continue };
        let n = list.len();
        match (w.suggest_sel, key) {
            (None, egui::Key::Tab) if n > 0 => w.suggest_sel = Some(0),
            (Some(_), egui::Key::Tab | egui::Key::Escape) => w.suggest_sel = None,
            (Some(k), egui::Key::ArrowDown) => w.suggest_sel = Some((k + 1) % n.max(1)),
            (Some(k), egui::Key::ArrowUp) => w.suggest_sel = Some((k + n.max(1) - 1) % n.max(1)),
            (Some(k), egui::Key::Enter) => {
                if let Some(s) = list.get(k) {
                    pick_action(w, s.action);
                }
            }
            (Some(_), _) => {}
            _ => continue,
        }
        w.sounds.push(Se::Cursor as u32);
        used = true;
    }
    used || w.suggest_sel.is_some()
}

/// Up / Down move among the hits, Enter picks, Backspace deletes a letter, Escape ends the search.
fn key_in(w: &mut World, key: egui::Key) {
    let Some(s) = w.list_search.clone() else { return };
    let n = hits(w, &s.text).len().min(MAX);
    match key {
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
                pick(w, &h);
            }
        }
        egui::Key::Backspace => {
            let mut t = s.text;
            t.pop();
            w.list_search = (!t.is_empty()).then_some(ListSearch { text: t, sel: 0 });
        }
        egui::Key::Escape => {
            w.list_search = None;
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
pub fn pick(w: &mut World, h: &Hit) {
    w.list_search = None;
    if let (Some(slot), Some(direct)) = (h.slot, item_list(w)) {
        let Some(k) = items::listed(w, direct).iter().position(|s| s.index == slot) else { return };
        if let Some(items::Win::Items { scroll, cursor, .. }) = w.win.as_mut() {
            let vis = crate::inventory::VISIBLE_ROWS;
            *scroll = if k < *scroll { k } else if k >= *scroll + vis { k + 1 - vis } else { *scroll };
            *cursor = k - *scroll;
        }
        items::key(w, egui::Key::Enter, false, false);
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
    use super::score;

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
