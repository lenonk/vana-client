//! The mod manager's files (not retail): mod folders under config::mods_dir() in XIPivot's layout (ROM*/<dir>/<file>.DAT
//! and game-root files), and packs installed from archives (zip, 7z, rar) found in the Downloads folder or the mods
//! folder. From a game-file pack only DATs at game paths and root cursor files are taken; a Lua mod (an archive or a
//! folder with a mod.toml) is taken whole, hidden files left out.

use crate::config::ModRef;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// An archive that holds game files and is not installed yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub path: PathBuf,
    /// The folder it installs to.
    pub slug: String,
    /// Game files in it (after a sub-pack's copies replace the main tree's), or a Lua mod's files.
    pub files: usize,
    /// A Lua mod's archive: its id is the slug.
    pub lua: Option<LuaMod>,
}

/// A Lua mod in an archive: the folder in it holding mod.toml ("" at the top, else ending in '/') and its version.
#[derive(Clone, Debug, PartialEq)]
pub struct LuaMod {
    pub prefix: String,
    pub version: String,
}

/// In a folder the manager installed: the archive's file name.
const SOURCE: &str = ".source";

/// The mod folders: everything under the mods folder but hidden and half-written ones.
pub fn folders() -> Vec<String> {
    folders_in(&crate::config::mods_dir())
}

fn folders_in(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !n.ends_with(".partial"))
        .collect();
    v.sort();
    v
}

/// An installed Lua mod as its mod.toml tells it.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct LuaInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    /// Its `[commands.<name>]` (client/lua/manifest.rs Command).
    #[serde(default)]
    pub commands: indexmap::IndexMap<String, LuaCommand>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct LuaCommand {
    pub help: String,
    pub args: Option<String>,
    pub params: Vec<LuaParam>,
    pub forms: Vec<LuaForm>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct LuaForm {
    pub args: String,
    pub help: String,
    pub params: Vec<LuaParam>,
}

/// A placeholder's type (client/lua/manifest.rs Param).
#[derive(Clone, Debug, Default, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct LuaParam {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub options: Vec<String>,
    pub also: Vec<String>,
}

impl LuaCommand {
    /// Every form as (words, help, params): the main one first (when declared, or when there are no others).
    pub fn forms(&self) -> Vec<(String, String, Vec<LuaParam>)> {
        let main = self.args.clone().or_else(|| self.forms.is_empty().then(String::new));
        let help = |h: &str| if h.is_empty() { self.help.clone() } else { h.to_string() };
        main.map(|a| (a, self.help.clone(), self.params.clone()))
            .into_iter()
            .chain(self.forms.iter().map(|f| (f.args.clone(), help(&f.help), f.params.clone())))
            .collect()
    }
}

impl LuaInfo {
    /// The description, then its commands (commands_text).
    pub fn details(&self) -> String {
        with_commands(&self.description, &self.commands)
    }
}

/// A description, then a mod's commands: a line a form, `//name words - help` (the main form first, as
/// manifest::Command::forms).
pub fn with_commands(description: &str, commands: &indexmap::IndexMap<String, LuaCommand>) -> String {
    if commands.is_empty() {
        return description.to_string();
    }
    let mut out = format!("{}\n\nCommands:", description.trim_end());
    for (name, c) in commands {
        for (args, help, _) in c.forms() {
            let words = if args.is_empty() { String::new() } else { format!(" {args}") };
            out.push_str(&format!("\n//{name}{words} - {help}"));
        }
    }
    out
}

/// The installed Lua mods by folder: name (the folder's when it gives none), description, version.
pub fn lua_info() -> std::collections::HashMap<String, LuaInfo> {
    let root = crate::config::mods_dir();
    lua_folders()
        .into_iter()
        .map(|f| {
            let mut i = std::fs::read_to_string(root.join(&f).join("mod.toml"))
                .ok()
                .and_then(|t| toml::from_str::<LuaInfo>(&t).ok())
                .unwrap_or_default();
            if i.name.is_empty() {
                i.name = f.clone();
            }
            (f, i)
        })
        .collect()
}

/// The mod folders that hold a Lua mod (a mod.toml).
pub fn lua_folders() -> std::collections::HashSet<String> {
    let root = crate::config::mods_dir();
    folders_in(&root).into_iter().filter(|f| root.join(f).join("mod.toml").is_file()).collect()
}

/// The list the page shows: the config's order first (folders gone dropped), then folders it does not name, off.
pub fn merged(cfg: &[ModRef], folders: &[String]) -> Vec<ModRef> {
    let mut out: Vec<ModRef> = cfg.iter().filter(|m| folders.contains(&m.name)).cloned().collect();
    for f in folders {
        if !out.iter().any(|m| &m.name == f) {
            out.push(ModRef::new(f.clone(), false));
        }
    }
    out
}

/// A member path in the mod layout, with the length of what came before it (a longer one is a sub-pack inside the
/// archive: it replaces the main tree's copy). DATs from their "ROM*" folder on; .ani / .cur cursors at the root.
fn normalise(member: &str) -> Option<(String, usize)> {
    let parts: Vec<&str> = member.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let n = parts.len();
    let last = *parts.last()?;
    let lower = last.to_ascii_lowercase();
    if n >= 3 {
        let (rom, dir) = (parts[n - 3], parts[n - 2]);
        let rom_no = rom.get(..3).filter(|p| p.eq_ignore_ascii_case("rom")).map(|_| &rom[3..]);
        let file = lower.strip_suffix(".dat").filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
        if let (Some(r), Some(f)) = (rom_no, file)
            && r.bytes().all(|b| b.is_ascii_digit())
            && !dir.is_empty()
            && dir.bytes().all(|b| b.is_ascii_digit())
        {
            let prefix: usize = parts[..n - 3].iter().map(|p| p.len() + 1).sum();
            return Some((format!("ROM{r}/{dir}/{f}.DAT"), prefix));
        }
    }
    // Root files go by lower-case names (the cursor loader asks for "mousenor.ani" and the like).
    if lower.ends_with(".ani") || lower.ends_with(".cur") {
        let prefix: usize = parts[..n - 1].iter().map(|p| p.len() + 1).sum();
        return Some((lower, prefix));
    }
    None
}

/// The folder an archive installs to: its name in lower case, runs of other characters as one '-'.
pub fn slug(archive: &Path) -> String {
    let stem = archive.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Zip,
    SevenZ,
    Rar,
}

fn kind(p: &Path) -> Option<Kind> {
    match p.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
        "zip" => Some(Kind::Zip),
        "7z" => Some(Kind::SevenZ),
        "rar" => Some(Kind::Rar),
        _ => None,
    }
}

/// Whether `p` is an archive kind the manager reads (zip, 7z, rar).
pub fn is_archive(p: &Path) -> bool {
    kind(p).is_some()
}

/// An archive picked by hand as a pack: a Lua mod, or its game files planned, or why it holds neither.
pub fn pack_of(path: &Path) -> Result<Pack> {
    pack_named(path, None)
}

/// As pack_of; `id`: a Lua mod's id when its mod.toml names none (else its folder's name).
pub fn pack_named(path: &Path, id: Option<&str>) -> Result<Pack> {
    let all = members(path)?;
    if let Some(p) = lua_pack(path, &all, id)? {
        return Ok(p);
    }
    let plan = plan(&all);
    if plan.is_empty() {
        return Err(anyhow!("no game files and no mod.toml in {}", path.display()));
    }
    Ok(Pack { path: path.to_path_buf(), slug: slug(path), files: plan.len(), lua: None })
}

/// The shallowest mod.toml among the members: (its folder, its member name).
fn lua_prefix(members: &[(String, u64)]) -> Option<(String, String)> {
    members
        .iter()
        .filter(|(m, _)| m.replace('\\', "/").rsplit('/').next().is_some_and(|f| f.eq_ignore_ascii_case("mod.toml")))
        .filter(|(m, _)| lua_target(m, "").is_some())
        .min_by_key(|(m, _)| m.matches(['/', '\\']).count())
        .map(|(m, _)| {
            let n = m.replace('\\', "/");
            (n[..n.len() - "mod.toml".len()].to_string(), m.clone())
        })
}

/// A member's path under `prefix`: '/'-separated, without "..", drive letters or hidden parts (.git and the like);
/// None for anything else and for folders.
fn lua_target(member: &str, prefix: &str) -> Option<String> {
    let m = member.replace('\\', "/");
    if m.ends_with('/') || m.starts_with('/') {
        return None;
    }
    let rel = m.strip_prefix(prefix)?;
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| p.starts_with('.') || p.contains(':') || *p == "__MACOSX") {
        return None;
    }
    Some(parts.join("/"))
}

/// The mod.toml fields the installer needs: the id (else `folder`) and the version.
pub fn lua_manifest(text: &str, folder: &str) -> Result<(String, String)> {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct M {
        id: String,
        version: String,
    }
    let m: M = toml::from_str(text).context("reading mod.toml")?;
    let id = if m.id.is_empty() { folder.to_string() } else { m.id };
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("mod id {id:?}: letters, digits, _ and - only");
    }
    Ok((id, if m.version.is_empty() { "0.0.0".into() } else { m.version }))
}

/// A folder's name as a mod id: a release's version suffix ("-1.0.0", "-v2") dropped, other characters as '-'.
fn folder_id(name: &str) -> String {
    let base = match name.rsplit_once('-') {
        Some((b, v))
            if !b.is_empty()
                && v.trim_start_matches(['v', 'V']).starts_with(|c: char| c.is_ascii_digit())
                && v.trim_start_matches(['v', 'V']).chars().all(|c| c.is_ascii_digit() || c == '.') =>
        {
            b
        }
        _ => name,
    };
    base.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect()
}

/// The archive as a Lua mod when it holds a mod.toml; `id` stands in for one its mod.toml lacks.
fn lua_pack(path: &Path, all: &[(String, u64)], id: Option<&str>) -> Result<Option<Pack>> {
    let Some((prefix, member)) = lua_prefix(all) else { return Ok(None) };
    let text = String::from_utf8(read_member(path, &member)?).context("mod.toml is not UTF-8")?;
    let folder = match id {
        Some(id) => id.to_string(),
        None => prefix
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|f| !f.is_empty())
            .map_or_else(|| slug(path), folder_id),
    };
    let (id, version) = lua_manifest(&text, &folder)?;
    let files = all.iter().filter(|(m, _)| lua_target(m, &prefix).is_some()).count();
    Ok(Some(Pack { path: path.to_path_buf(), slug: id, files, lua: Some(LuaMod { prefix, version }) }))
}

/// One member's bytes.
fn read_member(path: &Path, name: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match kind(path).ok_or_else(|| anyhow!("not an archive"))? {
        Kind::Zip => {
            zip::ZipArchive::new(std::fs::File::open(path)?)?.by_name(name)?.read_to_end(&mut out)?;
        }
        Kind::SevenZ => {
            let mut r = sevenz_rust2::ArchiveReader::new(std::fs::File::open(path)?, sevenz_rust2::Password::empty())
                .map_err(|e| anyhow!("{e}"))?;
            r.for_each_entries(|entry, rd| {
                if entry.name() == name {
                    rd.read_to_end(&mut out)?;
                    return Ok(false);
                }
                std::io::copy(rd, &mut std::io::sink())?;
                Ok(true)
            })
            .map_err(|e| anyhow!("{e}"))?;
        }
        Kind::Rar => {
            let mut a = unrar::Archive::new(path).open_for_processing()?;
            while let Some(h) = a.read_header()? {
                if h.entry().is_file() && h.entry().filename.to_string_lossy() == name {
                    return Ok(h.read()?.0);
                }
                a = h.skip()?;
            }
        }
    }
    Ok(out)
}

/// Every member name and size.
fn members(path: &Path) -> Result<Vec<(String, u64)>> {
    match kind(path).ok_or_else(|| anyhow!("not an archive"))? {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)?;
            (0..z.len())
                .map(|i| {
                    let f = z.by_index_raw(i)?;
                    Ok((f.name()?.into_owned(), f.size()))
                })
                .collect()
        }
        Kind::SevenZ => {
            let a = sevenz_rust2::Archive::open(path).map_err(|e| anyhow!("{e}"))?;
            Ok(a.files.iter().filter(|f| !f.is_directory()).map(|f| (f.name().to_string(), f.size())).collect())
        }
        Kind::Rar => {
            let mut out = Vec::new();
            for e in unrar::Archive::new(path).open_for_listing()? {
                let e = e?;
                if e.is_file() {
                    out.push((e.filename.to_string_lossy().into_owned(), e.unpacked_size));
                }
            }
            Ok(out)
        }
    }
}

/// Per target path, the member that fills it and its size. A DAT's deepest copy wins (a sub-pack over the main tree);
/// a root file's shallowest (the pack's cursors, not the alternatives in its folders).
fn plan(members: &[(String, u64)]) -> HashMap<String, (String, usize, u64)> {
    let mut out: HashMap<String, (String, usize, u64)> = HashMap::new();
    for (m, size) in members {
        let Some((to, pre)) = normalise(m) else { continue };
        let root = !to.contains('/');
        if out.get(&to).is_none_or(|e| if root { pre < e.1 } else { pre > e.1 }) {
            out.insert(to, (m.clone(), pre, *size));
        }
    }
    out
}

/// Where packs are looked for: the mods folder, the Downloads folder and the folders right inside it.
pub fn pack_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![crate::config::mods_dir()];
    if let Some(d) = dirs::download_dir()
        && let Ok(rd) = std::fs::read_dir(&d)
    {
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        dirs.push(d);
        dirs.extend(subs);
    }
    dirs
}

/// The size of `rel` under `dir`, its path matched without regard to case when the exact one is missing.
fn size_at(dir: &Path, rel: &str) -> Option<u64> {
    if let Ok(m) = std::fs::metadata(dir.join(rel)) {
        return Some(m.len());
    }
    let mut at = dir.to_path_buf();
    for part in rel.split('/') {
        let e = std::fs::read_dir(&at)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        at = e.path();
    }
    std::fs::metadata(at).ok().map(|m| m.len())
}

/// Archives with game files in `dirs` that no folder under `root` holds yet: none marked with its name, none with
/// its files at the same sizes (a sample of 16; folders installed by hand carry no mark).
pub fn scan(dirs: &[PathBuf], root: &Path) -> Vec<Pack> {
    let installed = folders_in(root);
    let marks: Vec<String> = installed
        .iter()
        .filter_map(|f| std::fs::read_to_string(root.join(f).join(SOURCE)).ok())
        .map(|s| s.trim().to_string())
        .collect();
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut paths: Vec<PathBuf> =
            rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && kind(p).is_some()).collect();
        paths.sort();
        for p in paths {
            let (slug, name) = (slug(&p), p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if slug.is_empty()
                || installed.contains(&slug)
                || marks.contains(&name)
                || out.iter().any(|o: &Pack| o.slug == slug)
            {
                continue;
            }
            let m = match members(&p) {
                Ok(m) => m,
                Err(e) => {
                    log::info!("mods: {} not read: {e:#}", p.display());
                    continue;
                }
            };
            match lua_pack(&p, &m, None) {
                Ok(Some(l)) => {
                    if !installed.contains(&l.slug) && !out.iter().any(|o: &Pack| o.slug == l.slug) {
                        out.push(l);
                    }
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    log::info!("mods: {}: {e:#}", p.display());
                    continue;
                }
            }
            let plan = plan(&m);
            if plan.is_empty() {
                continue;
            }
            let mut keys: Vec<&String> = plan.keys().collect();
            keys.sort();
            let sample: Vec<&String> = keys.iter().step_by((keys.len() / 16).max(1)).take(16).copied().collect();
            if installed.iter().any(|f| sample.iter().all(|k| size_at(&root.join(f), k) == Some(plan[*k].2))) {
                continue;
            }
            out.push(Pack { path: p, slug, files: plan.len(), lua: None });
        }
    }
    out
}

/// Writes one member to its place under `dest`.
fn put(dest: &Path, to: &str, r: &mut dyn Read) -> Result<()> {
    let path = dest.join(to);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(&path)?;
    std::io::copy(r, &mut f)?;
    f.flush()?;
    Ok(())
}

/// Member name -> where it goes, and the bytes in all: the planned game files, or a Lua mod's files.
fn targets(pack: &Pack, all: &[(String, u64)]) -> (HashMap<String, String>, u64) {
    match &pack.lua {
        Some(l) => {
            let t: HashMap<String, String> =
                all.iter().filter_map(|(m, _)| lua_target(m, &l.prefix).map(|to| (m.clone(), to))).collect();
            let total = all.iter().filter(|(m, _)| t.contains_key(m)).map(|(_, s)| s).sum();
            (t, total)
        }
        None => {
            let plan = plan(all);
            let total = plan.values().map(|v| v.2).sum();
            (plan.into_iter().map(|(to, (m, _, _))| (m, to)).collect(), total)
        }
    }
}

/// Unpacks a pack's files into `work`, over what is there; `step` gets (bytes done, bytes in all). Returns the number
/// of files.
pub fn unpack(pack: &Pack, work: &Path, step: &mut dyn FnMut(u64, u64)) -> Result<usize> {
    let all = members(&pack.path)?;
    let (wanted, total) = targets(pack, &all);
    std::fs::create_dir_all(work)?;
    let mut done = 0u64;
    let mut moved = |size: u64| {
        done += size;
        step(done, total);
    };
    match kind(&pack.path).ok_or_else(|| anyhow!("not an archive"))? {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(&pack.path)?)?;
            for i in 0..z.len() {
                let mut f = z.by_index(i)?;
                let name = f.name()?.into_owned();
                if let Some(to) = wanted.get(&name) {
                    put(work, to, &mut f)?;
                    moved(f.size());
                }
            }
        }
        Kind::SevenZ => {
            let mut r =
                sevenz_rust2::ArchiveReader::new(std::fs::File::open(&pack.path)?, sevenz_rust2::Password::empty())
                    .map_err(|e| anyhow!("{e}"))?;
            let mut failed = None;
            r.for_each_entries(|entry, rd| {
                match wanted.get(entry.name()) {
                    Some(to) => {
                        if let Err(e) = put(work, to, rd) {
                            failed = Some(e);
                            return Ok(false);
                        }
                        moved(entry.size());
                    }
                    // A solid block reads on from where the last entry stopped: skip by reading.
                    None => {
                        std::io::copy(rd, &mut std::io::sink())?;
                    }
                }
                Ok(true)
            })
            .map_err(|e| anyhow!("{e}"))?;
            if let Some(e) = failed {
                return Err(e);
            }
        }
        Kind::Rar => {
            let mut a = unrar::Archive::new(&pack.path).open_for_processing()?;
            while let Some(h) = a.read_header()? {
                let e = h.entry();
                let name = e.filename.to_string_lossy().into_owned();
                a = match wanted.get(&name).filter(|_| e.is_file()) {
                    Some(to) => {
                        let path = work.join(to);
                        if let Some(dir) = path.parent() {
                            std::fs::create_dir_all(dir)?;
                        }
                        let size = e.unpacked_size;
                        let next = h.extract_to(&path).with_context(|| format!("unpacking {name}"))?;
                        moved(size);
                        next
                    }
                    None => h.skip()?,
                };
            }
        }
    }
    Ok(wanted.len())
}

/// A fresh `<slug>.partial` under `root` to unpack into.
pub fn work_dir(root: &Path, slug: &str) -> Result<PathBuf> {
    let work = root.join(format!("{slug}.partial"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    Ok(work)
}

/// A finished `work` folder put in `dest`'s place; an older `dest` (an update) is moved aside first and deleted after.
pub fn place(work: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let old = dest.with_file_name(format!(".{name}.old"));
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(dest, &old)?;
        if let Err(e) = std::fs::rename(work, dest) {
            let _ = std::fs::rename(&old, dest);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
    } else {
        std::fs::rename(work, dest)?;
    }
    Ok(())
}

/// Unpacks a pack into `root` (the mods folder) as `<slug>.partial`, renamed to `<slug>` when complete; `progress`
/// counts per mille of the bytes. A game-file pack never goes over an installed folder; a Lua mod replaces its older
/// copy. Returns the number of files.
pub fn install(pack: &Pack, root: &Path, progress: &AtomicU32) -> Result<usize> {
    let dest = root.join(&pack.slug);
    if dest.exists() && pack.lua.is_none() {
        return Err(anyhow!("{} is already installed", pack.slug));
    }
    let work = work_dir(root, &pack.slug)?;
    let n = match unpack(pack, &work, &mut |done, total| {
        progress.store((done * 1000 / total.max(1)).min(1000) as u32, Ordering::Relaxed)
    }) {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(e);
        }
    };
    std::fs::write(
        work.join(SOURCE),
        pack.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    )?;
    place(&work, &dest)?;
    progress.store(1000, Ordering::Relaxed);
    log::info!("mods: installed {} from {} ({n} files)", pack.slug, pack.path.display());
    Ok(n)
}

/// A Lua mod's folder (one holding mod.toml) copied into `root`, hidden files left out; replaces an older copy.
/// Returns its id.
pub fn install_dir(src: &Path, root: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(src.join("mod.toml")).with_context(|| format!("no mod.toml in {}", src.display()))?;
    let folder = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (id, _) = lua_manifest(&text, &folder)?;
    let dest = root.join(&id);
    if src.canonicalize().ok() == dest.canonicalize().ok() {
        bail!("{} is the installed copy", src.display());
    }
    let work = work_dir(root, &id)?;
    fn copy(from: &Path, to: &Path) -> Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let ft = e.file_type()?;
            if ft.is_dir() {
                copy(&e.path(), &to.join(&name))?;
            } else if ft.is_file() {
                std::fs::copy(e.path(), to.join(&name))?;
            }
        }
        Ok(())
    }
    if let Err(e) = copy(src, &work) {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    place(&work, &dest)?;
    log::info!("mods: installed {id} from {}", src.display());
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_paths() {
        assert_eq!(normalise("ROM/118/104.DAT"), Some(("ROM/118/104.DAT".into(), 0)));
        assert_eq!(normalise("Pack/rom3/1/2.dat"), Some(("ROM3/1/2.DAT".into(), 5)));
        assert_eq!(normalise("Mog Houses\\ROM\\0\\7.DAT"), Some(("ROM/0/7.DAT".into(), 11)));
        assert_eq!(normalise("x/CURSOR.ANI"), Some(("cursor.ani".into(), 2)));
        assert_eq!(normalise("readme.txt"), None);
        assert_eq!(normalise("ROM/abc/1.DAT"), None);
        assert_eq!(normalise("ROMX/1/1.DAT"), None);
    }

    /// A sub-pack's DAT (deeper in the archive) replaces the main tree's; a root cursor keeps the pack's.
    #[test]
    fn sub_pack_wins() {
        let m = vec![
            ("ROM/1/2.DAT".to_string(), 10),
            ("Mog Houses/ROM/1/2.DAT".to_string(), 20),
            ("ROM/1/3.DAT".to_string(), 5),
            ("POL Cursors/Dog/mousenor.ani".to_string(), 7),
            ("mousenor.ani".to_string(), 8),
        ];
        let p = plan(&m);
        assert_eq!(p.len(), 3);
        assert_eq!(p["ROM/1/2.DAT"].0, "Mog Houses/ROM/1/2.DAT");
        assert_eq!(p["mousenor.ani"].0, "mousenor.ani");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug(Path::new("/d/FFXI - Amelila's AI Upscale - Pack 02.7z")), "ffxi-amelila-s-ai-upscale-pack-02");
        assert_eq!(slug(Path::new("xiPrime-Cursors-20231230.zip")), "xiprime-cursors-20231230");
    }

    /// The user's packs (zip, 7z, rar) listed and the smallest of each kind installed into a scratch folder. Run by
    /// hand: `cargo test --release modpack::tests::real_packs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_packs() {
        let home = dirs::home_dir().unwrap();
        // Against the real mods folder: the hand-installed packs count as there (same files, same sizes).
        let there = scan(&[home.join("Downloads/FFXI")], &crate::config::mods_dir());
        println!("not installed yet: {:?}", there.iter().map(|p| &p.slug).collect::<Vec<_>>());
        let root =
            std::path::PathBuf::from(std::env::var_os("VANA_SCRATCH").expect("VANA_SCRATCH: a scratch folder on disk"))
                .join("modtest");
        let packs = scan(&[home.join("Downloads/FFXI")], &root);
        for p in &packs {
            println!("{} -> {} ({} files)", p.path.display(), p.slug, p.files);
        }
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for ext in ["zip", "7z", "rar"] {
            let Some(p) = packs
                .iter()
                .filter(|p| {
                    kind(&p.path) == kind(Path::new(&format!("x.{ext}")))
                        && std::fs::metadata(&p.path).is_ok_and(|m| m.len() < 2 << 30)
                })
                .min_by_key(|p| std::fs::metadata(&p.path).map(|m| m.len()).unwrap_or(u64::MAX))
            else {
                continue;
            };
            let progress = AtomicU32::new(0);
            let n = install(p, &root, &progress).unwrap();
            println!("installed {} ({n} files, progress {})", p.slug, progress.load(Ordering::Relaxed));
            assert_eq!(n, p.files);
            assert!(root.join(&p.slug).is_dir() && !root.join(format!("{}.partial", p.slug)).exists());
        }
    }

    /// A scratch folder on disk beside the test binary.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::current_exe().unwrap().parent().unwrap().join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_of(path: &Path, files: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in files {
            z.start_file(*name, o).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    /// A Lua mod in a release zip (one folder inside): its id from mod.toml, hidden files and paths that climb out
    /// left behind; a newer copy replaces the installed one. A folder with a mod.toml installs the same way.
    #[test]
    fn lua_mod_archives() {
        let d = scratch("modpack-lua-test");
        let (root, zip) = (d.join("mods"), d.join("hello-v1.zip"));
        zip_of(
            &zip,
            &[
                ("hello-1.0/mod.toml", "id = \"hello\"\nversion = \"1.0.0\"\n"),
                ("hello-1.0/init.lua", "-- one"),
                ("hello-1.0/src/a.lua", "return 1"),
                ("hello-1.0/.git/config", "x"),
                ("hello-1.0/../evil.lua", "x"),
                ("hello-1.0/examples/mod.toml", "id = \"other\""),
            ],
        );
        let p = pack_of(&zip).unwrap();
        assert_eq!((p.slug.as_str(), p.files), ("hello", 4));
        assert_eq!(p.lua, Some(LuaMod { prefix: "hello-1.0/".into(), version: "1.0.0".into() }));
        assert_eq!(install(&p, &root, &AtomicU32::new(0)).unwrap(), 4);
        assert_eq!(std::fs::read_to_string(root.join("hello/init.lua")).unwrap(), "-- one");
        assert!(
            root.join("hello/src/a.lua").is_file() && !root.join("hello/.git").exists() && !d.join("evil.lua").exists()
        );
        zip_of(&zip, &[("mod.toml", "id = \"hello\"\nversion = \"1.1.0\""), ("init.lua", "-- two")]);
        install(&pack_of(&zip).unwrap(), &root, &AtomicU32::new(0)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("hello/init.lua")).unwrap(), "-- two");
        assert!(!root.join("hello/src").exists() && folders_in(&root) == ["hello"]);
        let src = d.join("dev/greeter");
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(src.join("mod.toml"), "name = \"Greeter\"").unwrap();
        std::fs::write(src.join("lib/x.lua"), "").unwrap();
        std::fs::write(src.join(".hidden"), "").unwrap();
        assert_eq!(install_dir(&src, &root).unwrap(), "greeter");
        assert!(root.join("greeter/lib/x.lua").is_file() && !root.join("greeter/.hidden").exists());
        zip_of(&zip, &[("readme.txt", "")]);
        assert!(pack_of(&zip).is_err());
        zip_of(&zip, &[("greeter-v2.0.1/mod.toml", "name = \"G\""), ("greeter-v2.0.1/init.lua", "")]);
        assert_eq!(pack_of(&zip).unwrap().slug, "greeter");
        assert_eq!(pack_named(&zip, Some("from-list")).unwrap().slug, "from-list");
        assert_eq!(
            (folder_id("My Mod-1.2"), folder_id("x-ray"), folder_id("hud-v2")),
            ("My-Mod".into(), "x-ray".into(), "hud".into())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn merged_order() {
        let cfg = vec![ModRef::new("b", true), ModRef::new("gone", true), ModRef::new("a", false)];
        let got = merged(&cfg, &["a".into(), "b".into(), "c".into()]);
        let names: Vec<(&str, bool)> = got.iter().map(|m| (m.name.as_str(), m.enabled)).collect();
        assert_eq!(names, [("b", true), ("a", false), ("c", false)]);
    }
}
