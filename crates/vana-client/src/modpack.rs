//! The mod manager's files (not retail): mod folders under config::mods_dir() in XIPivot's layout (ROM*/<dir>/<file>.DAT
//! and game-root files), and packs installed from archives (zip, 7z, rar) found in the Downloads folder or the mods
//! folder. Nothing in an archive is run; only DATs at game paths and root cursor files are taken.

use crate::config::ModRef;
use anyhow::{Context, Result, anyhow};
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
    /// Game files in it (after a sub-pack's copies replace the main tree's).
    pub files: usize,
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

/// The list the page shows: the config's order first (folders gone dropped), then folders it does not name, off.
pub fn merged(cfg: &[ModRef], folders: &[String]) -> Vec<ModRef> {
    let mut out: Vec<ModRef> = cfg.iter().filter(|m| folders.contains(&m.name)).cloned().collect();
    for f in folders {
        if !out.iter().any(|m| &m.name == f) {
            out.push(ModRef { name: f.clone(), enabled: false });
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
/// a root file's shallowest (the pack's own cursors, not the alternatives in its folders).
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
    if let Some(d) = dirs::download_dir() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            subs.sort();
            dirs.push(d);
            dirs.extend(subs);
        }
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
        let e = std::fs::read_dir(&at).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        at = e.path();
    }
    std::fs::metadata(at).ok().map(|m| m.len())
}

/// Archives with game files in `dirs` that no folder under `root` holds yet: none marked with its name, none with
/// its files at the same sizes (a sample of 16; folders installed by hand carry no mark).
pub fn scan(dirs: &[PathBuf], root: &Path) -> Vec<Pack> {
    let installed = folders_in(root);
    let marks: Vec<String> = installed.iter().filter_map(|f| std::fs::read_to_string(root.join(f).join(SOURCE)).ok()).map(|s| s.trim().to_string()).collect();
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && kind(p).is_some()).collect();
        paths.sort();
        for p in paths {
            let (slug, name) = (slug(&p), p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if slug.is_empty() || installed.contains(&slug) || marks.contains(&name) || out.iter().any(|o: &Pack| o.slug == slug) {
                continue;
            }
            let m = match members(&p) {
                Ok(m) => m,
                Err(e) => {
                    log::info!("mods: {} not read: {e:#}", p.display());
                    continue;
                }
            };
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
            out.push(Pack { path: p, slug, files: plan.len() });
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

/// Unpacks a pack's game files into `root` (the mods folder) as `<slug>.partial`, renamed to `<slug>` when complete;
/// `progress` counts per mille of the bytes. Returns the number of files.
pub fn install(pack: &Pack, root: &Path, progress: &AtomicU32) -> Result<usize> {
    let all = members(&pack.path)?;
    let plan = plan(&all);
    // Member name -> target, for the members that fill a target.
    let wanted: HashMap<&str, &str> = plan.iter().map(|(to, (m, _, _))| (m.as_str(), to.as_str())).collect();
    let total: u64 = plan.values().map(|v| v.2).sum::<u64>().max(1);
    let (work, dest) = (root.join(format!("{}.partial", pack.slug)), root.join(&pack.slug));
    if dest.exists() {
        return Err(anyhow!("{} is already installed", pack.slug));
    }
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let mut done = 0u64;
    let mut step = |size: u64| {
        done += size;
        progress.store((done * 1000 / total).min(1000) as u32, Ordering::Relaxed);
    };
    match kind(&pack.path).ok_or_else(|| anyhow!("not an archive"))? {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(&pack.path)?)?;
            for i in 0..z.len() {
                let mut f = z.by_index(i)?;
                let name = f.name()?.into_owned();
                if let Some(to) = wanted.get(name.as_str()) {
                    put(&work, to, &mut f)?;
                    step(f.size());
                }
            }
        }
        Kind::SevenZ => {
            let mut r = sevenz_rust2::ArchiveReader::new(std::fs::File::open(&pack.path)?, sevenz_rust2::Password::empty()).map_err(|e| anyhow!("{e}"))?;
            let mut failed = None;
            r.for_each_entries(|entry, rd| {
                match wanted.get(entry.name()) {
                    Some(to) => {
                        if let Err(e) = put(&work, to, rd) {
                            failed = Some(e);
                            return Ok(false);
                        }
                        step(entry.size());
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
                a = match wanted.get(name.as_str()).filter(|_| e.is_file()) {
                    Some(to) => {
                        let path = work.join(to);
                        if let Some(dir) = path.parent() {
                            std::fs::create_dir_all(dir)?;
                        }
                        let size = e.unpacked_size;
                        let next = h.extract_to(&path).with_context(|| format!("unpacking {name}"))?;
                        step(size);
                        next
                    }
                    None => h.skip()?,
                };
            }
        }
    }
    std::fs::write(work.join(SOURCE), pack.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())?;
    std::fs::rename(&work, &dest)?;
    progress.store(1000, Ordering::Relaxed);
    log::info!("mods: installed {} from {} ({} files)", pack.slug, pack.path.display(), plan.len());
    Ok(plan.len())
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

    /// A sub-pack's DAT (deeper in the archive) replaces the main tree's; a root cursor keeps the pack's own.
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
        let root = home.join("ffxi/lab/scratch/modtest");
        let packs = scan(&[home.join("Downloads/FFXI")], &root);
        for p in &packs {
            println!("{} -> {} ({} files)", p.path.display(), p.slug, p.files);
        }
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for ext in ["zip", "7z", "rar"] {
            let Some(p) = packs.iter().filter(|p| kind(&p.path) == kind(Path::new(&format!("x.{ext}"))) && std::fs::metadata(&p.path).is_ok_and(|m| m.len() < 2 << 30)).min_by_key(|p| std::fs::metadata(&p.path).map(|m| m.len()).unwrap_or(u64::MAX)) else { continue };
            let progress = AtomicU32::new(0);
            let n = install(p, &root, &progress).unwrap();
            println!("installed {} ({n} files, progress {})", p.slug, progress.load(Ordering::Relaxed));
            assert_eq!(n, p.files);
            assert!(root.join(&p.slug).is_dir() && !root.join(format!("{}.partial", p.slug)).exists());
        }
    }

    #[test]
    fn merged_order() {
        let cfg = vec![ModRef { name: "b".into(), enabled: true }, ModRef { name: "gone".into(), enabled: true }, ModRef { name: "a".into(), enabled: false }];
        let got = merged(&cfg, &["a".into(), "b".into(), "c".into()]);
        let names: Vec<(&str, bool)> = got.iter().map(|m| (m.name.as_str(), m.enabled)).collect();
        assert_eq!(names, [("b", true), ("a", false), ("c", false)]);
    }
}
