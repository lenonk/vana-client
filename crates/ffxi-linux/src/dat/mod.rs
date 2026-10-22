//! The game's file system: numeric file ids mapped to ROM*/dir/file.DAT, and the chunk
//! container most DATs use.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// `VTABLE{n}.DAT` holds one byte per file id (the ROM number when present);
/// `FTABLE{n}.DAT` a u16 per id: `dir << 7 | file`.
pub struct DatIndex {
    root: PathBuf,
    /// Per file id: (rom number, packed dir/file); rom 0 means absent.
    entries: Vec<(u8, u16)>,
}

impl DatIndex {
    pub fn open(ffxi_dir: &Path) -> Result<Self> {
        let mut entries: Vec<(u8, u16)> = Vec::new();
        for rom in 1..=9u8 {
            let (dir, suffix) = if rom == 1 { (ffxi_dir.to_path_buf(), String::new()) } else { (ffxi_dir.join(format!("ROM{rom}")), rom.to_string()) };
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
        Ok(Self { root: ffxi_dir.to_path_buf(), entries })
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
        Some(self.root.join(rom_dir).join((packed >> 7).to_string()).join(format!("{}.DAT", packed & 0x7F)))
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
        out.push(Chunk { name: file[at..at + 4].try_into().unwrap(), kind: (word & 0x7F) as u8, data: &file[at + 16..at + size], offset: at });
        at += size;
    }
    Ok(out)
}

pub mod zone;
pub mod collision;
pub mod character;
pub mod npc;
pub mod font;
pub mod ui;
pub mod dancer;
pub mod msg;
pub mod dialog;
pub mod msgshift;
pub mod items;
pub mod names;
pub mod event;
pub mod event_ops;
pub mod title;
pub mod weather;
pub mod battle;
pub mod sched;
pub mod motionfile;
pub mod texmip;
pub mod animpack;
