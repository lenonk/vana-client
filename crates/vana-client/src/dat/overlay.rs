//! DAT overlays (not retail: XIPivot's job built in). Mods are folders laid out like the game (ROM*/<dir>/<file>.DAT and
//! game-root files), listed highest priority first. An overlaid DAT is rebuilt from the current stock file: its
//! textures (D3s), menu sprite sheets (Msb) and bump maps (Bp) come from the highest-priority mod that has them, every
//! other chunk stays stock, so SE's updates survive packs made years ago. A file that is not a chunk container is
//! replaced whole. Merged files are cached, keyed by every source's size and time.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Chunk kinds taken from mods (names from FFXiMain's table at 0x103542a8).
const MERGED: [u8; 3] = [0x20, 0x31, 0x5d];

pub struct Overlays {
    /// File id -> the mods' copies, highest priority first.
    by_id: HashMap<u32, Vec<PathBuf>>,
    dirs: Vec<PathBuf>,
    cache: PathBuf,
}

impl Overlays {
    /// Every ROM*/<dir>/<file>.DAT of the mods that the stock tables know (`ids`: (rom, dir << 7 | file) -> id).
    pub fn scan(dirs: &[PathBuf], ids: &HashMap<(u8, u16), u32>, cache: PathBuf) -> Self {
        let mut by_id: HashMap<u32, Vec<PathBuf>> = HashMap::new();
        let mut unknown = 0;
        for dir in dirs {
            let Ok(roms) = std::fs::read_dir(dir) else { continue };
            for rom in roms.flatten() {
                let name = rom.file_name().to_string_lossy().to_ascii_uppercase();
                let Some(n) = name.strip_prefix("ROM").map(|s| if s.is_empty() { Some(1u8) } else { s.parse().ok() }).flatten() else { continue };
                let Ok(subs) = std::fs::read_dir(rom.path()) else { continue };
                for sub in subs.flatten() {
                    let Some(d) = sub.file_name().to_str().and_then(|s| s.parse::<u16>().ok()) else { continue };
                    let Ok(files) = std::fs::read_dir(sub.path()) else { continue };
                    for f in files.flatten() {
                        let fname = f.file_name().to_string_lossy().to_ascii_uppercase();
                        let Some(file) = fname.strip_suffix(".DAT").and_then(|s| s.parse::<u16>().ok()) else { continue };
                        match ids.get(&(n, d << 7 | file)) {
                            Some(&id) => by_id.entry(id).or_default().push(f.path()),
                            None => unknown += 1,
                        }
                    }
                }
            }
        }
        log::info!("overlays: {} mods, {} files overlaid, {unknown} files at paths the game has no id for", dirs.len(), by_id.len());
        Self { by_id, dirs: dirs.to_vec(), cache }
    }

    pub fn has(&self, id: u32) -> bool {
        self.by_id.contains_key(&id)
    }

    /// A game-root file (cursors and the like) from the highest-priority mod that has it.
    pub fn root_file(&self, name: &str) -> Option<PathBuf> {
        self.dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
    }

    /// The file to read for `id`: a merged build in the cache, a mod's whole file, or None (stock).
    pub fn resolve(&self, id: u32, stock: &Path) -> Option<PathBuf> {
        let mods = self.by_id.get(&id)?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for p in std::iter::once(stock).chain(mods.iter().map(PathBuf::as_path)) {
            p.hash(&mut h);
            let m = std::fs::metadata(p).ok()?;
            (m.len(), m.modified().ok()).hash(&mut h);
        }
        let out = self.cache.join(format!("{id}-{:016x}.DAT", h.finish()));
        if out.is_file() {
            return Some(out);
        }
        let stock_bytes = std::fs::read(stock).ok()?;
        let mod_bytes: Vec<Vec<u8>> = mods.iter().filter_map(|p| std::fs::read(p).ok()).collect();
        let Some(merged) = merge(&stock_bytes, &mod_bytes) else {
            // Not chunk containers: the highest-priority mod's file as it is.
            return mods.first().cloned();
        };
        let _ = std::fs::create_dir_all(&self.cache);
        // Older builds of this id go.
        if let Ok(rd) = std::fs::read_dir(&self.cache) {
            let prefix = format!("{id}-");
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let tmp = out.with_extension("tmp");
        if std::fs::write(&tmp, &merged).and_then(|_| std::fs::rename(&tmp, &out)).is_err() {
            return mods.first().cloned();
        }
        Some(out)
    }
}

/// A chunk's whole bytes (header included) and its match key.
struct Raw<'a> {
    bytes: &'a [u8],
    kind: u8,
    key: (Vec<[u8; 4]>, [u8; 4], u8, Vec<u8>, usize),
}

/// Chunks with keys: the folder path, the 4cc, the kind, a texture's own name (D3s data + 1, 16 bytes: what the game
/// looks textures up by) and the n-th such chunk. None when it is not a well-formed chunk container.
fn raw(b: &[u8]) -> Option<Vec<Raw<'_>>> {
    let (mut out, mut at, mut stack) = (Vec::new(), 0, Vec::new());
    let mut seen: HashMap<(Vec<[u8; 4]>, [u8; 4], u8, Vec<u8>), usize> = HashMap::new();
    while at < b.len() {
        let w = u32::from_le_bytes(b.get(at + 4..at + 8)?.try_into().ok()?);
        let size = ((w >> 7) & 0x7ffff) as usize * 16;
        if size < 16 || at + size > b.len() {
            return None;
        }
        let (name, kind) = (b[at..at + 4].try_into().ok()?, (w & 0x7f) as u8);
        if kind == 1 {
            stack.push(name);
        }
        let tex = if kind == 0x20 { b.get(at + 17..at + 33).map(<[u8]>::to_vec).unwrap_or_default() } else { Vec::new() };
        let k = (stack.clone(), name, kind, tex);
        let n = seen.entry(k.clone()).or_default();
        *n += 1;
        out.push(Raw { bytes: &b[at..at + size], kind, key: (k.0, k.1, k.2, k.3, *n) });
        if kind == 0 {
            stack.pop();
        }
        at += size;
    }
    Some(out)
}

/// The stock file with the mods' D3s / Msb / Bp chunks in place of stock ones of the same key; `mods` highest
/// priority first. Chunks a mod has that stock lacks are left out (the stock file was restructured since).
pub fn merge(stock: &[u8], mods: &[Vec<u8>]) -> Option<Vec<u8>> {
    let base = raw(stock)?;
    let parsed: Vec<Vec<Raw>> = mods.iter().map(|m| raw(m)).collect::<Option<_>>()?;
    let mut take: HashMap<&(Vec<[u8; 4]>, [u8; 4], u8, Vec<u8>, usize), &[u8]> = HashMap::new();
    for m in parsed.iter().rev() {
        for c in m.iter().filter(|c| MERGED.contains(&c.kind)) {
            take.insert(&c.key, c.bytes);
        }
    }
    let mut out = Vec::with_capacity(stock.len());
    for c in &base {
        out.extend_from_slice(take.get(&c.key).copied().filter(|_| MERGED.contains(&c.kind)).unwrap_or(c.bytes));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::merge;

    fn chunk(name: &[u8; 4], kind: u8, data: &[u8]) -> Vec<u8> {
        let size = (16 + data.len()).div_ceil(16) * 16;
        let mut c = name.to_vec();
        c.extend_from_slice(&(kind as u32 | ((size / 16) as u32) << 7).to_le_bytes());
        c.extend_from_slice(&[0; 8]);
        c.extend_from_slice(data);
        c.resize(size, 0);
        c
    }

    /// Textures come from the mods (the higher one first), a mod's stale model does not.
    #[test]
    fn textures_only_highest_first() {
        let tex = |n: u8| {
            let mut d = vec![0u8; 40];
            d[1..5].copy_from_slice(b"tex1");
            d[30] = n;
            d
        };
        let stock = [chunk(b"fold", 1, &[]), chunk(b"img1", 0x20, &tex(1)), chunk(b"mdl1", 0x2e, &[1; 16]), chunk(b"end\0", 0, &[])].concat();
        let low = [chunk(b"fold", 1, &[]), chunk(b"img1", 0x20, &tex(2)), chunk(b"mdl1", 0x2e, &[9; 16]), chunk(b"end\0", 0, &[])].concat();
        let high = [chunk(b"fold", 1, &[]), chunk(b"img1", 0x20, &tex(3)), chunk(b"end\0", 0, &[])].concat();
        let m = merge(&stock, &[high.clone(), low]).unwrap();
        let expect = [chunk(b"fold", 1, &[]), chunk(b"img1", 0x20, &tex(3)), chunk(b"mdl1", 0x2e, &[1; 16]), chunk(b"end\0", 0, &[])].concat();
        assert_eq!(m, expect);
        assert!(merge(b"not a dat", &[high]).is_none());
    }
}
