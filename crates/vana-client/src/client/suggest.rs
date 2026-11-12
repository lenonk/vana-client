//! The battle menu's suggestion rows (not retail; docs/modern-ui.md "Recommendations"): the Suggestions mod picks
//! them (its rules over the game's state) and fills the battle_rows slot; the modern battle menu shows them above its
//! commands, rides them with its cursor and opens a row's spell family as a submenu. It never acts by itself.

use super::World;

/// What a row uses: a spell id, or an ability table index (job abilities and weapon skills).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Spell(u16),
    Ability(u16),
}

/// A row: its action, who it is for (a party member's name), why, and a spell family's castable spells with the
/// family's name (the row's submenu; empty with one spell).
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub action: Action,
    pub target: Option<String>,
    pub reason: String,
    pub group: Vec<u16>,
    pub label: Option<String>,
}

impl Suggestion {
    /// A battle_rows slot row (its shape checked by ui.native.slot): {kind, id, target, reason, label, group}.
    pub fn from_row(v: &serde_json::Value) -> Option<Self> {
        let id = v.get("id")?.as_u64()? as u16;
        let action = match v.get("kind")?.as_str()? {
            "spell" => Action::Spell(id),
            "ability" => Action::Ability(id),
            _ => return None,
        };
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Some(Self {
            action,
            target: text("target"),
            reason: text("reason").unwrap_or_default(),
            label: text("label"),
            group: v
                .get("group")
                .and_then(|g| g.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u16)).collect())
                .unwrap_or_default(),
        })
    }
}

/// The battle menu's rows: the mod's (the battle_rows slot), modern UI only; the cursor and an open submenu dropped
/// while the menu is not up. A row's open submenu stays on its spell when the mod regroups its family.
pub fn refresh_root(w: &mut World) {
    if !w.modern || w.action_menu.is_none() || w.prompt.is_some() {
        (w.suggest_sel, w.suggest_sub) = (None, None);
    } else if w.suggest_sel.is_none() {
        w.suggest_sub = None;
    }
    let v = w.lua.slot("battle_rows").filter(|_| w.modern).unwrap_or_default();
    if v != w.battle_src {
        let on = w.suggest_sel.zip(w.suggest_sub).and_then(|(k, c)| w.battle_rows.get(k)?.group.get(c).copied());
        w.battle_rows = v.as_array().map(|a| a.iter().filter_map(Suggestion::from_row).collect()).unwrap_or_default();
        w.battle_src = v;
        if let (Some(k), Some(_)) = (w.suggest_sel, w.suggest_sub) {
            w.suggest_sub =
                w.battle_rows.get(k).filter(|s| s.group.len() > 1).map(|s| {
                    on.and_then(|id| s.group.iter().position(|&g| g == id)).unwrap_or_else(|| family_start(s))
                });
        }
    }
}

/// Where a family row's submenu opens: on the suggested spell.
pub fn family_start(s: &Suggestion) -> usize {
    match s.action {
        Action::Spell(id) => s.group.iter().position(|&g| g == id).unwrap_or(0),
        Action::Ability(_) => 0,
    }
}

/// The keys of a suggestion's family submenu: on a family row Right or Enter opens it, on the suggested spell; open,
/// Up / Down move, Enter casts that spell, Left / Escape close it. True when the key was the submenu's.
pub fn family_key(w: &mut World, on: Option<usize>, key: egui::Key) -> bool {
    use crate::audio::Ui as Se;
    let Some(s) = on.and_then(|k| w.battle_rows.get(k)).filter(|s| s.group.len() > 1).cloned() else {
        w.suggest_sub = None;
        return false;
    };
    let n = s.group.len();
    match (w.suggest_sub, key) {
        (None, egui::Key::ArrowRight | egui::Key::Enter) => {
            w.suggest_sub = Some(family_start(&s));
            w.sounds.push(Se::Select as u32);
        }
        (None, _) => return false,
        (Some(c), egui::Key::ArrowUp | egui::Key::ArrowDown) => {
            w.suggest_sub = Some(if key == egui::Key::ArrowDown { (c + 1) % n } else { (c + n - 1) % n });
            w.sounds.push(Se::Cursor as u32);
        }
        (Some(c), egui::Key::Enter) => {
            w.suggest_sub = None;
            super::listsearch::pick_root(w, Action::Spell(s.group[c.min(n - 1)]), s.target.as_deref());
        }
        (Some(_), egui::Key::ArrowLeft | egui::Key::Escape) => {
            w.suggest_sub = None;
            w.sounds.push(Se::Cancel as u32);
        }
        (Some(_), _) => {}
    }
    true
}
