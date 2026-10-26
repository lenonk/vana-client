//! Type-to-search in the magic and ability windows (not retail; the modern layer's, docs/modern-ui.md): typing while
//! a category window or a list is open searches the names; from a category window every list behind it. A pick goes
//! through retail's own keys (the category, the row, Enter), so every check and the target prompt still apply.

use super::World;
use super::magic::{self, Cat, Kind};
use crate::audio::Ui as Se;

/// The search: its text and the highlighted hit.
#[derive(Clone, Debug, Default)]
pub struct ListSearch {
    pub text: String,
    pub sel: usize,
}

/// A row found: its list's kind (and the category row it is under when searching from a category window), id, name and
/// score.
#[derive(Clone, Debug)]
pub struct Hit {
    pub kind: Kind,
    pub cat: Option<usize>,
    pub id: u16,
    pub name: String,
    pub score: i32,
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
    let mut add = |kind: Kind, cat: Option<usize>, rows: Vec<u16>| {
        for id in rows {
            let n = name(w, kind, id);
            if let Some(score) = score(&n, query) {
                out.push(Hit { kind, cat, id, name: n, score });
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

/// A window the search can run in: a category window or an unhidden list, with nothing over it.
pub fn can_run(w: &World) -> bool {
    let list = w.ability.as_ref().or(w.magic.as_ref());
    w.modern
        && w.prompt.is_none()
        && w.confirm.is_none()
        && !w.typing
        && (w.cats.is_some() || list.is_some_and(|l| !l.hidden && l.sort_focus.is_none() && l.ask.is_none() && l.manual == 0))
}

/// The search's keys and typed text this frame. True when it took them (the game then sees none).
pub fn events(w: &mut World, events: &[egui::Event]) -> bool {
    if !can_run(w) {
        w.list_search = None;
        return false;
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
