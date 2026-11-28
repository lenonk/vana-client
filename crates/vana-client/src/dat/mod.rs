//! The game's file system: numeric file ids mapped to ROM*/dir/file.DAT, and the chunk
//! container most DATs use.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The updater's copy of FFXiMain.dll under a game folder: ours, where retail, Windower and Ashita never load code from.
pub const OWN_FFXIMAIN: &str = "vana-client/FFXiMain.dll";

/// The copy for the install at `ffxi_dir`, beside its stamped manifest; the client reads tables from it.
pub fn own_ffximain(ffxi_dir: &Path) -> PathBuf {
    ffxi_dir.join(OWN_FFXIMAIN)
}

/// The FFXiMain.dll the client reads: that install's copy, else the game folder's.
pub fn ffximain(ffxi_dir: &Path) -> PathBuf {
    let own = own_ffximain(ffxi_dir);
    if own.is_file() { own } else { ffxi_dir.join("FFXiMain.dll") }
}

/// The game folder a login uses (Client::game_data sets it as it opens that folder's files); none yet: the config's.
static FFXI_DIR_IN_USE: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// From here on the tables built from FFXiMain outside the client's own (DllTable) come from `ffxi_dir`'s.
pub fn use_ffxi_dir(ffxi_dir: &Path) {
    *FFXI_DIR_IN_USE.write().unwrap_or_else(|e| e.into_inner()) = Some(ffxi_dir.to_path_buf());
}

/// The "FINAL FANTASY XI" folder in use: the login's (use_ffxi_dir), else the config's.
pub fn ffxi_dir_in_use() -> PathBuf {
    FFXI_DIR_IN_USE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| crate::config::Config::load().ffxi_dir())
}

/// Bumped by dll_files_changed: the tables built before it are built again.
static DLL_GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The game files changed under the client (an update applied, a first start's install): DllTable builds its tables
/// again from the new FFXiMain.
pub fn dll_files_changed() {
    DLL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// A table built from the FFXiMain of the game folder in use, once for each folder (a server's own folder has its
/// own DLL) and update (dll_files_changed): a login into another folder gets that DLL's table. Each is kept for the
/// whole run (a few folders and updates at most).
pub struct DllTable<T: 'static>(std::sync::Mutex<Vec<(PathBuf, u32, &'static T)>>);

impl<T: Sync + 'static> DllTable<T> {
    pub const fn new() -> Self {
        Self(std::sync::Mutex::new(Vec::new()))
    }

    /// The table for the folder in use, built from its FFXiMain (empty bytes when it cannot be read).
    pub fn get(&self, build: impl FnOnce(&[u8]) -> T) -> &'static T {
        self.get_in(&ffxi_dir_in_use(), build)
    }

    fn get_in(&self, ffxi_dir: &Path, build: impl FnOnce(&[u8]) -> T) -> &'static T {
        let g = DLL_GENERATION.load(std::sync::atomic::Ordering::Relaxed);
        let mut t = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, v)) = t.iter().find(|(d, k, _)| d == ffxi_dir && *k == g) {
            return v;
        }
        let v: &'static T = Box::leak(Box::new(build(&std::fs::read(ffximain(ffxi_dir)).unwrap_or_default())));
        t.retain(|(d, _, _)| d != ffxi_dir);
        t.push((ffxi_dir.to_path_buf(), g, v));
        v
    }
}

/// `VTABLE{n}.DAT` holds one byte per file id (the ROM number when present);
/// `FTABLE{n}.DAT` a u16 per id: `dir << 7 | file`.
pub struct DatIndex {
    root: PathBuf,
    /// Per file id: (rom number, packed dir/file); rom 0 means absent.
    entries: Vec<(u8, u16)>,
    /// The enabled mods (config `mods`; FFXI_NO_MODS turns them off), scanned once per process.
    overlays: Option<std::sync::Arc<overlay::Overlays>>,
}

impl DatIndex {
    pub fn open(ffxi_dir: &Path) -> Result<Self> {
        let mut entries: Vec<(u8, u16)> = Vec::new();
        for rom in 1..=9u8 {
            let (dir, suffix) = if rom == 1 {
                (ffxi_dir.to_path_buf(), String::new())
            } else {
                (ffxi_dir.join(format!("ROM{rom}")), rom.to_string())
            };
            let vpath = dir.join(format!("VTABLE{suffix}.DAT"));
            if !vpath.exists() {
                continue;
            }
            let v = std::fs::read(&vpath)?;
            let f = std::fs::read(dir.join(format!("FTABLE{suffix}.DAT")))?;
            if f.len() < v.len() * 2 {
                bail!("{} is shorter than its VTABLE", dir.display());
            }
            if entries.len() < v.len() {
                entries.resize(v.len(), (0, 0));
            }
            for (id, &present) in v.iter().enumerate() {
                if present != 0 {
                    entries[id] = (rom, u16::from_le_bytes([f[id * 2], f[id * 2 + 1]]));
                }
            }
        }
        if entries.is_empty() {
            bail!("no VTABLE.DAT under {}", ffxi_dir.display());
        }
        let overlays = Self::overlays(ffxi_dir, &entries);
        Ok(Self { root: ffxi_dir.to_path_buf(), entries, overlays })
    }

    /// The enabled mods' files by this install's ids: one scan for each install (a server's own game folder has its
    /// own file tables) while the enabled mods stay the same; every install shares the one mod list.
    fn overlays(ffxi_dir: &Path, entries: &[(u8, u16)]) -> Option<std::sync::Arc<overlay::Overlays>> {
        use std::sync::{Arc, Mutex, OnceLock};
        type Scan = (PathBuf, Vec<PathBuf>, Arc<overlay::Overlays>);
        let dirs = crate::config::enabled_mod_dirs();
        if dirs.is_empty() {
            return None;
        }
        static SCANNED: OnceLock<Mutex<Vec<Scan>>> = OnceLock::new();
        let mut s = SCANNED.get_or_init(Default::default).lock().unwrap();
        if let Some((_, _, o)) = s.iter().find(|(r, d, _)| r == ffxi_dir && *d == dirs) {
            return Some(o.clone());
        }
        let ids: std::collections::HashMap<(u8, u16), u32> =
            entries.iter().enumerate().filter(|(_, e)| e.0 != 0).map(|(i, &e)| (e, i as u32)).collect();
        let o = Arc::new(overlay::Overlays::scan(&dirs, &ids, crate::config::merged_dir()));
        s.retain(|(r, _, _)| r != ffxi_dir);
        s.push((ffxi_dir.to_path_buf(), dirs, o.clone()));
        Some(o)
    }

    /// The install alone, no mods (texture exports for packs).
    pub fn open_stock(ffxi_dir: &Path) -> Result<Self> {
        let mut index = Self::open(ffxi_dir)?;
        index.overlays = None;
        Ok(index)
    }

    /// The "FINAL FANTASY XI" folder it was opened on.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder that holds "FINAL FANTASY XI" (the PlayOnline Viewer's files sit beside it).
    pub fn game_dir(&self) -> &Path {
        self.root.parent().unwrap_or(&self.root)
    }

    /// A game-root file (cursors and the like), from the highest-priority mod that has one.
    pub fn root_file(&self, name: &str) -> PathBuf {
        self.overlays.as_ref().and_then(|o| o.root_file(name)).unwrap_or_else(|| self.root.join(name))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn path(&self, id: u32) -> Option<PathBuf> {
        let &(rom, packed) = self.entries.get(id as usize)?;
        if rom == 0 {
            return None;
        }
        let rom_dir = if rom == 1 { "ROM".to_string() } else { format!("ROM{rom}") };
        let stock = self.root.join(rom_dir).join((packed >> 7).to_string()).join(format!("{}.DAT", packed & 0x7F));
        match &self.overlays {
            Some(o) if o.has(id) => Some(o.resolve(id, &stock).unwrap_or(stock)),
            _ => Some(stock),
        }
    }

    pub fn read(&self, id: u32) -> Result<Vec<u8>> {
        let path = self.path(id).with_context(|| format!("file id {id} is not in the index"))?;
        std::fs::read(&path).with_context(|| format!("reading {}", path.display()))
    }
}

/// Installed-content mask (FFXiMain DAT_10444110 + 0x10, FUN_101b9b80) for the ROMs present.
pub fn installed_mask(rom: impl Fn(u32) -> bool) -> u32 {
    let mut m = 1;
    for n in 2..=13 {
        if !rom(n) {
            continue;
        }
        m |= match n {
            2..=5 => 1 << (n - 1),
            // ROM6..8 count only with ROM2.
            6..=8 if m & 2 != 0 => 1 << (n - 1),
            6..=8 => 0,
            _ => 1 << (n + 2),
        };
    }
    // The three Abyssea bits come with ROM2 and ROM5.
    if m & 0x12 == 0x12 {
        m |= 0x700;
    }
    m
}

/// The installed mask of an install directory: ROMn counts when its VTABLE and FTABLE are there.
pub fn installed(ffxi_dir: &Path) -> u32 {
    installed_mask(|n| {
        let d = ffxi_dir.join(format!("ROM{n}"));
        d.join(format!("VTABLE{n}.DAT")).is_file() && d.join(format!("FTABLE{n}.DAT")).is_file()
    })
}

/// One chunk: 16-byte header (4-char name, u32 `type:7 | size_in_16_byte_units:19`, 8 reserved).
#[derive(Debug, Clone, Copy)]
pub struct Chunk<'a> {
    pub name: [u8; 4],
    pub kind: u8,
    /// Payload after the header.
    pub data: &'a [u8],
    pub offset: usize,
}

impl Chunk<'_> {
    pub fn name(&self) -> String {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(4);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }
}

pub fn chunks(file: &[u8]) -> Result<Vec<Chunk<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 16 <= file.len() {
        let word = u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap());
        let size = (((word >> 7) & 0x7FFFF) as usize) * 16;
        if size < 16 || at + size > file.len() {
            bail!("bad chunk size {size} at offset {at:#x}");
        }
        out.push(Chunk {
            name: file[at..at + 4].try_into().unwrap(),
            kind: (word & 0x7F) as u8,
            data: &file[at + 16..at + size],
            offset: at,
        });
        at += size;
    }
    Ok(out)
}

pub mod actions;
pub mod animpack;
pub mod autotrans;
pub mod battle;
pub mod blumagic;
pub mod bump;
pub mod character;
pub mod collision;
pub mod cursor;
pub mod dancer;
pub mod dialog;
pub mod effect;
pub mod event;
pub mod event_ops;
pub mod font;
pub mod furniture;
pub mod generator;
pub mod items;
pub mod journal;
pub mod lang;
pub mod maze;
pub mod merits;
pub mod motionfile;
pub mod msg;
pub mod msgfmt;
pub mod msgshift;
pub mod names;
pub mod npc;
pub mod overlay;
pub mod recasts;
pub mod roe;
pub mod sched;
pub mod skillchains;
pub mod statusinfo;
pub mod tables;
pub mod texmip;
pub mod title;
pub mod tpoints;
pub mod trusts;
pub mod ui;
pub mod weather;
pub mod zone;

#[cfg(test)]
mod fuzz;

#[cfg(test)]
mod tests {
    use super::*;

    /// A DLL table is built once for each game folder, from that folder's FFXiMain.
    #[test]
    fn dll_table_per_folder() {
        let base = std::env::temp_dir().join(format!("vana-dlltable-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        for (d, dll) in [(&a, b"AAAA".as_slice()), (&b, b"BB".as_slice())] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("FFXiMain.dll"), dll).unwrap();
        }
        static T: DllTable<usize> = DllTable::new();
        let mut built = 0;
        let mut len = |d: &[u8]| {
            built += 1;
            d.len()
        };
        assert_eq!(*T.get_in(&a, &mut len), 4);
        assert_eq!(*T.get_in(&b, &mut len), 2);
        assert_eq!(*T.get_in(&a, &mut len), 4);
        assert_eq!(built, 2);
        // An update: built again from the new DLL.
        std::fs::write(a.join("FFXiMain.dll"), b"AAAAAA").unwrap();
        dll_files_changed();
        assert_eq!(*T.get_in(&a, |d| d.len()), 6);
        assert_eq!(*T.get_in(&base.join("none"), |d| d.len()), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
