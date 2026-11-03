//! A Lua mod's `mod.toml` (docs/lua-mods.md 3.4).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use indexmap::IndexMap;
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
        }
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
    /// account (default), character or server.
    pub scope: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// An enum's labels.
    pub options: Vec<String>,
}

/// Every permission a mod may ask for.
pub const PERMISSIONS: [&str; 13] = ["actions", "automation", "packets.read", "packets.write", "input.capture", "files.shared", "net", "exec", "multibox", "ui.native", "clipboard", "process", "devices"];

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
    Ok(m)
}
