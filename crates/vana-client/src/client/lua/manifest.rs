//! A Lua mod's `mod.toml` (docs/lua-mods.md 3.4).

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use std::path::Path;

/// The API version this client speaks; a mod declaring a newer one is not loaded.
pub const API: u32 = 1;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Manifest {
    /// The folder's name when not given.
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    pub authors: Vec<String>,
    pub description: String,
    /// The file run at load, in the mod's folder.
    pub entry: String,
    /// Permissions asked for (docs/lua-mods.md 3.5); the user grants them.
    pub permissions: Vec<String>,
    /// The VM lives from the lobby screens (before login) to the end of the process, not from login to logout.
    pub lobby: bool,
    /// Declared settings by key, drawn by the client (docs/lua-mods.md 4.12).
    pub settings: IndexMap<String, Setting>,
    /// The text commands it registers, by name (`[commands.<name>]`): what the chat line's completion, //mods and the
    /// Mods page show.
    pub commands: IndexMap<String, Command>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            version: "0.0.0".into(),
            api: API,
            authors: Vec::new(),
            description: String::new(),
            entry: "init.lua".into(),
            permissions: Vec::new(),
            lobby: false,
            settings: IndexMap::new(),
            commands: IndexMap::new(),
        }
    }
}

/// A declared text command (`[commands.<name>]`): a line on what it does, the words of its main form (`args`; none: it
/// has only `forms`), and its other forms, each with a separate line.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Command {
    pub help: String,
    pub args: Option<String>,
    /// The main form's typed placeholders.
    pub params: Vec<Param>,
    pub forms: Vec<Form>,
}

/// One form of a command: the words after its name and a line on what it does (none: the command's).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Form {
    pub args: String,
    pub help: String,
    pub params: Vec<Param>,
}

/// A placeholder's type, for the Mods window's form: what picks its value.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Param {
    /// The placeholder's first word (`<item | none | own>`: item).
    pub name: String,
    /// One of PARAM_TYPES (none: text).
    #[serde(rename = "type")]
    pub kind: String,
    /// An enum's choices.
    pub options: Vec<String>,
    /// Words it takes besides its type's (none, own).
    pub also: Vec<String>,
}

pub const PARAM_TYPES: [&str; 10] =
    ["text", "int", "number", "enum", "item", "key_item", "zone", "spell", "job", "player"];

impl Command {
    /// Every form as (words, help): the main one first (when declared, or when there are no others).
    pub fn forms(&self) -> Vec<(String, String)> {
        let main = self.args.clone().or_else(|| self.forms.is_empty().then(String::new));
        let help = |h: &str| if h.is_empty() { self.help.clone() } else { h.to_string() };
        main.map(|a| (a, self.help.clone()))
            .into_iter()
            .chain(self.forms.iter().map(|f| (f.args.clone(), help(&f.help))))
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Setting {
    /// bool, int, number, enum, string, colour or key.
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Option<toml::Value>,
    pub label: String,
    pub help: String,
    /// account (default) or character.
    pub scope: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// An enum's labels; with `values`, any type's labels for those values.
    pub options: Vec<String>,
    pub values: Vec<toml::Value>,
    /// Shown on one of the client's Extras tabs (Maps, Timers, Battle, Misc.) at row `place`, not on the mod's page:
    /// a bundled mod's settings where the extra it replaces had its rows.
    pub tab: String,
    pub place: Option<usize>,
    /// Its row in the modern UI's page, where that differs (rows only the classic page has).
    pub place_modern: Option<usize>,
    /// The client config field its first value comes from (the extra it replaces kept it there).
    pub from: String,
    /// An int whose value picks interface sound `preview_sound` + value: the settings page plays it whenever the value
    /// changes.
    pub preview_sound: Option<u32>,
}

/// The client's Extras tabs a setting may name.
pub const EXTRA_TABS: [&str; 4] = ["Maps", "Timers", "Battle", "Misc."];

/// Every permission a mod may ask for.
pub const PERMISSIONS: [&str; 14] = [
    "actions",
    "automation",
    "packets.read",
    "packets.write",
    "input.capture",
    "files.shared",
    "net",
    "exec",
    "multibox",
    "ui.native",
    "clipboard",
    "process",
    "devices",
    "mods",
];

/// Reads `dir/mod.toml`.
pub fn read(dir: &Path) -> Result<Manifest> {
    let path = dir.join("mod.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    parse(&text, &folder).with_context(|| format!("parsing {}", path.display()))
}

/// A manifest's text; `folder` is the id when it names none.
pub fn parse(text: &str, folder: &str) -> Result<Manifest> {
    let mut m: Manifest = toml::from_str(text)?;
    if m.id.is_empty() {
        m.id = folder.to_string();
    }
    if m.name.is_empty() {
        m.name = m.id.clone();
    }
    if !m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("mod id {:?}: letters, digits, _ and - only", m.id);
    }
    if m.api > API {
        bail!("{} needs API {} (this client has {API})", m.id, m.api);
    }
    if let Some(p) = m.permissions.iter().find(|p| !PERMISSIONS.contains(&p.as_str())) {
        bail!("{}: unknown permission {p:?}", m.id);
    }
    const TYPES: [&str; 7] = ["bool", "int", "number", "enum", "string", "colour", "key"];
    for (key, s) in &m.settings {
        if !["", "account", "character"].contains(&s.scope.as_str()) {
            bail!("{}: setting {key}: scope is \"account\" or \"character\" ({:?} given)", m.id, s.scope);
        }
        if !s.kind.is_empty() && !TYPES.contains(&s.kind.as_str()) {
            bail!("{}: setting {key}: type is one of {} ({:?} given)", m.id, TYPES.join(", "), s.kind);
        }
        if !s.values.is_empty() && s.values.len() != s.options.len() {
            bail!("{}: setting {key}: values needs one label in options each", m.id);
        }
        if !s.tab.is_empty() && !EXTRA_TABS.contains(&s.tab.as_str()) {
            bail!("{}: setting {key}: tab is one of {} ({:?} given)", m.id, EXTRA_TABS.join(", "), s.tab);
        }
    }
    if let Some(name) = m
        .commands
        .keys()
        .find(|n| n.is_empty() || n.contains(char::is_whitespace) || n.starts_with('/') || *n == "lua" || *n == "mods")
    {
        bail!("{}: command {name:?}: a name without slashes or spaces, not lua or mods", m.id);
    }
    for (name, c) in &m.commands {
        for p in c.params.iter().chain(c.forms.iter().flat_map(|f| &f.params)) {
            if !p.kind.is_empty() && !PARAM_TYPES.contains(&p.kind.as_str()) {
                bail!(
                    "{}: command {name}: param {}: type is one of {} ({:?} given)",
                    m.id,
                    p.name,
                    PARAM_TYPES.join(", "),
                    p.kind
                );
            }
            if p.kind == "enum" && p.options.is_empty() {
                bail!("{}: command {name}: param {}: an enum needs options", m.id, p.name);
            }
        }
    }
    Ok(m)
}
