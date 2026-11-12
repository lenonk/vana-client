//! The modern UI's setup per character and main job (not retail): its window places (each display's), folded edge
//! windows, mode presets, chat window settings and favourites, kept in <data>/ui/config/<World>-<Name>/<JOB>.json
//! (beside an FFXI_CONFIG copy for test runs). The config's fields hold the one in use. A job new to a character
//! starts as the job it comes from (the character's last one, at a login), favourites aside: they are a job's spells
//! and abilities, as the hotbar's bars are (which it keeps by job itself). A character's first job takes the config's
//! fields as they were.

use super::World;
use crate::config::{Config, ModernLayout, ModernModes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One character's job's setup. A field its file lacks (one added since) starts as the config's default.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Window places by display: "" the latest (the template for a display not shown yet), else " @WxH S%".
    pub places: BTreeMap<String, ModernLayout>,
    pub folded: BTreeMap<String, bool>,
    pub modes: ModernModes,
    pub chat_font: f32,
    pub chat_hidden: Vec<String>,
    pub battle_in_all: bool,
    pub condense: bool,
    pub scan_kinds: u8,
    pub favourites: Vec<String>,
    pub fav_strip: bool,
    /// Fields of a newer client, kept as they are (InUse carries them to the next save).
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

impl Default for UiConfig {
    fn default() -> Self {
        let c = Config::default();
        Self {
            places: BTreeMap::new(),
            folded: c.modern_folded,
            modes: c.modern_modes,
            chat_font: c.modern_chat_font,
            chat_hidden: c.modern_chat_hidden,
            battle_in_all: c.modern_battle_in_all,
            condense: c.modern_condense,
            scan_kinds: c.modern_scan_kinds,
            favourites: Vec::new(),
            fav_strip: c.modern_fav_strip,
            unknown: Default::default(),
        }
    }
}

/// The setup in use (World::ui_profile): its character folder and job, and the fields of a newer client its file had.
pub struct InUse {
    pub character: String,
    pub job: String,
    unknown: serde_json::Map<String, serde_json::Value>,
}

impl InUse {
    fn is(&self, (character, job): &(String, String)) -> bool {
        self.character == *character && self.job == *job
    }
}

fn root() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("ui").join("config"),
        None => crate::config::data_dir().join("ui").join("config"),
    }
}

/// A character's folder name, as the Lua mods' storage names its file: "<World>-<Name>", letters and digits.
pub fn character(world: &str, name: &str) -> String {
    let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    format!("{}-{}", clean(world), clean(name))
}

fn path(root: &Path, character: &str, job: &str) -> PathBuf {
    root.join(character).join(format!("{job}.json"))
}

/// The main job as its setup is named (MON in Monstrosity); none until it is known.
pub(super) fn job_of(w: &World) -> Option<&'static str> {
    if super::monstrosity::active(w) {
        return Some("MON");
    }
    super::items::JOBS.get(w.stats.as_ref()?.main.0 as usize).copied().filter(|j| !j.is_empty())
}

/// The character (its folder name) and the main job, once known: the mods' switches kept per character or job are
/// theirs (settings.rs).
pub fn at(w: &World) -> (String, Option<String>) {
    (character(&w.world_name, &w.name), job_of(w).map(str::to_string))
}

/// The character and main job whose setup the world wants, when it is not the one in use (each frame: a world is one
/// character's, so only the job is compared).
pub fn wanted(w: &World) -> Option<(String, String)> {
    let job = job_of(w).filter(|j| !w.name.is_empty() && w.ui_profile.as_ref().is_none_or(|p| p.job != *j))?;
    Some((character(&w.world_name, &w.name), job.to_string()))
}

/// What the setups use of the world: the character's name, its favourites and their strip, the setup in use.
struct Held<'a> {
    name: &'a str,
    favourites: &'a mut Vec<String>,
    fav_strip: &'a mut bool,
    profile: &'a mut Option<InUse>,
}

fn held(w: &mut World) -> Held<'_> {
    Held { name: &w.name, favourites: &mut w.favourites, fav_strip: &mut w.fav_strip, profile: &mut w.ui_profile }
}

/// The character's setup as the config and the world hold it now.
fn take(cfg: &Config, h: &Held) -> UiConfig {
    let places = cfg
        .modern_places
        .iter()
        .filter(|(k, _)| super::modern::layout_of(k, h.name))
        .map(|(k, v)| (k[h.name.len()..].to_string(), v.clone()))
        .collect();
    UiConfig {
        places,
        folded: cfg.modern_folded.clone(),
        modes: cfg.modern_modes.clone(),
        chat_font: cfg.modern_chat_font,
        chat_hidden: cfg.modern_chat_hidden.clone(),
        battle_in_all: cfg.modern_battle_in_all,
        condense: cfg.modern_condense,
        scan_kinds: cfg.modern_scan_kinds,
        favourites: h.favourites.clone(),
        fav_strip: *h.fav_strip,
        unknown: Default::default(),
    }
}

/// A setup into the config and the world; `favourites` false keeps the world's.
fn put(cfg: &mut Config, h: &mut Held, u: &UiConfig, favourites: bool) {
    cfg.modern_places.retain(|k, _| !super::modern::layout_of(k, h.name));
    for (k, v) in &u.places {
        cfg.modern_places.insert(format!("{}{k}", h.name), v.clone());
    }
    cfg.modern_folded = u.folded.clone();
    cfg.modern_modes = u.modes.clone();
    if u.chat_font > 0.0 {
        cfg.modern_chat_font = u.chat_font;
    }
    cfg.modern_chat_hidden = u.chat_hidden.clone();
    (cfg.modern_battle_in_all, cfg.modern_condense, cfg.modern_scan_kinds) =
        (u.battle_in_all, u.condense, u.scan_kinds);
    if favourites {
        (*h.favourites, *h.fav_strip) = (u.favourites.clone(), u.fav_strip);
        cfg.modern_favourites.insert(h.name.to_string(), h.favourites.clone());
        cfg.modern_fav_strip = *h.fav_strip;
    }
}

fn read(p: &Path) -> Option<UiConfig> {
    let text = std::fs::read(p).ok()?;
    let mut u: UiConfig =
        serde_json::from_slice(&text).map_err(|e| log::warn!("ui config {}: {e}", p.display())).ok()?;
    // The config's fix-ups on load (Config::load), for a file written before them.
    u.modes.split_timers();
    Some(u)
}

/// The last file written and its bytes: a config save that changed nothing of the setup writes nothing.
static WRITTEN: std::sync::Mutex<Option<(PathBuf, Vec<u8>)>> = std::sync::Mutex::new(None);

fn write(p: &Path, u: &UiConfig) {
    let Ok(bytes) = serde_json::to_vec_pretty(u) else { return };
    let mut last = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|(q, b)| q == p && *b == bytes) {
        return;
    }
    let tmp = p.with_extension(format!("json.{}.tmp", std::process::id()));
    let done = (|| -> anyhow::Result<()> {
        std::fs::create_dir_all(p.parent().unwrap_or(p))?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, p)?;
        Ok(())
    })();
    match done {
        Ok(()) => *last = Some((p.to_path_buf(), bytes)),
        Err(e) => log::warn!("ui config {} not saved: {e:#}", p.display()),
    }
}

/// A character's kept jobs with when each was saved.
fn jobs(root: &Path, character: &str) -> Vec<(std::time::SystemTime, String)> {
    let Ok(dir) = std::fs::read_dir(root.join(character)) else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let p = e.path();
            let job = p.file_stem()?.to_str()?.to_string();
            (p.extension()? == "json").then_some((e.metadata().ok()?.modified().ok()?, job))
        })
        .collect()
}

/// The character's most recently saved job other than `but`: Monstrosity's only when it has no other (a monster's
/// windows are not a job's).
fn latest(root: &Path, character: &str, but: &str) -> Option<String> {
    let all: Vec<_> = jobs(root, character).into_iter().filter(|j| j.1 != but).collect();
    let pick = |mon: bool| all.iter().filter(|j| (j.1 == "MON") == mon).max().map(|j| j.1.clone());
    pick(false).or_else(|| pick(true))
}

/// The world's setup to `to` (character, job): the one in use saved first; `to`'s loaded, else made from the job it
/// comes from (in this session, else the character's latest), else from the config as it is (the character's first).
pub fn switch(cfg: &mut Config, w: &mut World, to: (String, String)) {
    switch_in(&root(), cfg, &mut held(w), to);
}

fn switch_in(root: &Path, cfg: &mut Config, h: &mut Held, to: (String, String)) {
    if let Some(from) = h.profile.take() {
        write(&path(root, &from.character, &from.job), &UiConfig { unknown: from.unknown, ..take(cfg, h) });
    }
    let (character, job) = &to;
    let mut unknown = Default::default();
    match read(&path(root, character, job)) {
        Some(u) => {
            log::info!("ui config: {character} {job}");
            put(cfg, h, &u, true);
            unknown = u.unknown;
        }
        None => {
            let source = latest(root, character, job);
            if let Some(src) = source.as_ref().and_then(|j| read(&path(root, character, j))) {
                put(cfg, h, &src, false);
            }
            let mut u = take(cfg, h);
            // A new job's actions are its alone (as its hotbar): the favourites start empty.
            if source.is_some() {
                u.favourites.clear();
                put(cfg, h, &u, true);
            }
            log::info!("ui config: {character} {job} made from {}", source.as_deref().unwrap_or("the config"));
            write(&path(root, character, job), &u);
        }
    }
    let (character, job) = to;
    *h.profile = Some(InUse { character, job, unknown });
}

/// The setup in use to its file (after the config saved a change; unchanged, nothing is written).
pub fn save(cfg: &Config, w: &mut World) {
    let h = held(w);
    if let Some(p) = h.profile.as_ref() {
        write(&path(&root(), &p.character, &p.job), &UiConfig { unknown: p.unknown.clone(), ..take(cfg, &h) });
    }
}

/// The most setups UI Setup's From lists.
const LISTED: usize = 16;

/// The other setups kept (character folder, job) for Settings' UI Setup row: this character's other jobs, then this
/// job on other characters, then the rest, each most recently saved first; LISTED at most.
pub fn others(w: &World) -> Vec<(String, String)> {
    others_in(&root(), w.ui_profile.as_ref())
}

fn others_in(root: &Path, now: Option<&InUse>) -> Vec<(String, String)> {
    let mut all: Vec<(u8, std::cmp::Reverse<std::time::SystemTime>, String, String)> = Vec::new();
    for c in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let Some(character) = c.file_name().to_str().map(String::from) else { continue };
        for (at, job) in jobs(root, &character) {
            let group = match now {
                Some(n) if n.character == character && n.job == job => continue,
                Some(n) if n.character == character => 0,
                Some(n) if n.job == job => 1,
                _ => 2,
            };
            all.push((group, std::cmp::Reverse(at), character.clone(), job));
        }
    }
    all.sort();
    all.into_iter().take(LISTED).map(|(_, _, c, j)| (c, j)).collect()
}

/// How a kept setup reads in Settings: "Nine WHM" (the world's name too when another world has a Nine).
pub fn label(all: &[(String, String)], (character, job): &(String, String)) -> String {
    let name = |c: &str| c.split_once('-').map_or(c.to_string(), |(_, n)| n.to_string());
    let shared = all.iter().any(|(c, _)| c != character && name(c) == name(character));
    match character.split_once('-') {
        Some((world, n)) if shared => format!("{n} {job} ({world})"),
        _ => format!("{} {job}", name(character)),
    }
}

/// Another character's or job's setup copied into the one in use: places, modes and the chat window's settings, not
/// the favourites (this job's). The one in use itself (a list from before a job change) is left alone.
pub fn copy_from(cfg: &mut Config, w: &mut World, from: &(String, String)) {
    if w.ui_profile.as_ref().is_some_and(|p| p.is(from)) {
        return;
    }
    if let Some(u) = read(&path(&root(), &from.0, &from.1)) {
        log::info!("ui config: {} {} copied in", from.0, from.1);
        put(cfg, &mut held(w), &u, false);
        save(cfg, w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Characters on two worlds with one name read with their world; the rest by name.
    #[test]
    fn labels() {
        let all = [
            ("Asura-Tsetse".to_string(), "WHM".to_string()),
            ("Nameless-Nine".to_string(), "NIN".to_string()),
            ("Asura-Nine".to_string(), "WAR".to_string()),
        ];
        assert_eq!(label(&all, &all[0]), "Tsetse WHM");
        assert_eq!(label(&all, &all[1]), "Nine NIN (Nameless)");
        assert_eq!(character("Nameless", "Nine"), "Nameless-Nine");
    }

    /// A character's view for switch_in: its name, favourites and setup in use.
    struct Char {
        name: String,
        favourites: Vec<String>,
        strip: bool,
        profile: Option<InUse>,
    }

    impl Char {
        fn new(name: &str) -> Self {
            Char { name: name.into(), favourites: Vec::new(), strip: true, profile: None }
        }

        fn to(&mut self, root: &Path, cfg: &mut Config, job: &str) {
            let mut h = Held {
                name: &self.name,
                favourites: &mut self.favourites,
                fav_strip: &mut self.strip,
                profile: &mut self.profile,
            };
            switch_in(root, cfg, &mut h, (character("W", &self.name), job.into()));
        }

        fn saved(&self, root: &Path, job: &str) -> UiConfig {
            read(&path(root, &character("W", &self.name), job)).expect("kept")
        }
    }

    fn root(tag: &str) -> PathBuf {
        let r = std::env::temp_dir().join(format!("vana-uiconf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        r
    }

    /// A character's first job takes the config as it was, favourites too; a new job then starts as the job it came
    /// from without favourites, and each job keeps its favourites when switched back.
    #[test]
    fn new_jobs_copy_the_last() {
        let root = root("copy");
        let mut cfg = Config::default();
        cfg.modern_folded.insert("minimap".into(), true);
        let mut nine = Char::new("Nine");
        nine.favourites = vec!["m:1".into()];
        nine.to(&root, &mut cfg, "WAR");
        assert_eq!(nine.saved(&root, "WAR").favourites, ["m:1"]);
        nine.to(&root, &mut cfg, "WHM");
        let whm = nine.saved(&root, "WHM");
        assert!(whm.favourites.is_empty() && nine.favourites.is_empty());
        assert_eq!(whm.folded.get("minimap"), Some(&true));
        // WHM's change stays WHM's.
        cfg.modern_folded.insert("minimap".into(), false);
        nine.favourites = vec!["m:2".into()];
        nine.to(&root, &mut cfg, "WAR");
        assert_eq!((nine.favourites.clone(), cfg.modern_folded.get("minimap")), (vec!["m:1".to_string()], Some(&true)));
        nine.to(&root, &mut cfg, "WHM");
        assert_eq!(
            (nine.favourites.clone(), cfg.modern_folded.get("minimap")),
            (vec!["m:2".to_string()], Some(&false))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// At a login on a job not kept yet, the character's latest job is the start; Monstrosity's only when there is no
    /// other.
    #[test]
    fn login_starts_from_the_latest() {
        let root = root("latest");
        let mut cfg = Config::default();
        let pause = || std::thread::sleep(std::time::Duration::from_millis(20));
        let mut nine = Char::new("Nine");
        nine.to(&root, &mut cfg, "RDM");
        pause();
        nine.to(&root, &mut cfg, "MON");
        pause();
        // Back to RDM: MON is now the newest file, RDM still the start for a new job.
        cfg.modern_chat_font = 21.0;
        nine.to(&root, &mut cfg, "RDM");
        let newest = jobs(&root, &character("W", "Nine")).into_iter().max().map(|j| j.1);
        assert_eq!(newest.as_deref(), Some("MON"));
        assert_eq!(latest(&root, &character("W", "Nine"), "WAR").as_deref(), Some("RDM"));
        // A login on WAR: made from RDM's file, not from what the config holds.
        (nine.profile, cfg.modern_chat_font) = (None, 14.0);
        nine.to(&root, &mut cfg, "WAR");
        let rdm = nine.saved(&root, "RDM").chat_font;
        assert!(rdm != 14.0 && nine.saved(&root, "WAR").chat_font == rdm);
        // A character who has only been a monster starts from that.
        let mut mon = Char::new("Mon");
        mon.to(&root, &mut cfg, "MON");
        assert_eq!(latest(&root, &character("W", "Mon"), "WAR").as_deref(), Some("MON"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// From lists this character's other jobs first, then this job on other characters, then the rest, never the
    /// setup in use, and LISTED at most.
    #[test]
    fn others_by_kin() {
        let root = root("others");
        let mut cfg = Config::default();
        for (name, job) in [("Tsetse", "NIN"), ("Tsetse", "WHM"), ("Nine", "WHM"), ("Nine", "WAR")] {
            let mut c = Char::new(name);
            c.to(&root, &mut cfg, job);
        }
        let now = InUse { character: character("W", "Nine"), job: "WHM".into(), unknown: Default::default() };
        let list = others_in(&root, Some(&now));
        let jobs: Vec<String> = list.iter().map(|(c, j)| format!("{c} {j}")).collect();
        assert_eq!(jobs, ["W-Nine WAR", "W-Tsetse WHM", "W-Tsetse NIN"]);
        for k in 0..20 {
            Char::new(&format!("Alt{k}")).to(&root, &mut cfg, "BLU");
        }
        assert_eq!(others_in(&root, Some(&now)).len(), LISTED);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file without a field (one added since it was written) takes the config's default; a newer client's fields
    /// are kept; the config's mode fix-ups run on it.
    #[test]
    fn file_fields() {
        let dir = std::env::temp_dir().join(format!("vana-uiconf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("WAR.json");
        std::fs::write(&p, r#"{"chat_font": 18.0, "modes": {"pinned": ["timers"]}, "later": 5}"#).unwrap();
        let u = read(&p).unwrap();
        let c = Config::default();
        assert_eq!(u.chat_font, 18.0);
        assert_eq!((u.scan_kinds, u.battle_in_all, u.condense), (c.modern_scan_kinds, true, true));
        assert_eq!(u.fav_strip, c.modern_fav_strip);
        assert!(u.modes.pinned.iter().any(|p| p == "timers_buffs") && !u.modes.pinned.iter().any(|p| p == "timers"));
        assert_eq!(u.unknown.get("later"), Some(&serde_json::json!(5)));
        write(&p, &u);
        assert_eq!(read(&p).unwrap().unknown.get("later"), Some(&serde_json::json!(5)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
