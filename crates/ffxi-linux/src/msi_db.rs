//! The parts of a Windows Installer database that say where each file goes.

use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::Path;

pub struct Directory {
    pub parent: Option<String>,
    pub default_dir: String,
}

pub struct Component {
    pub directory: String,
    pub condition: Option<String>,
}

pub struct FileRow {
    pub key: String,
    pub component: String,
    pub name: String,
    pub size: u64,
}

pub struct Media {
    pub cabinet: String,
}

pub struct Database {
    pub properties: HashMap<String, String>,
    pub directories: HashMap<String, Directory>,
    pub components: HashMap<String, Component>,
    pub files: Vec<FileRow>,
    pub media: Vec<Media>,
    /// File key -> MD5 digest, from MsiFileHash (unversioned files only).
    pub hashes: HashMap<String, [u8; 16]>,
}

fn opt_str(v: &msi::Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn req_str(v: &msi::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn int(v: &msi::Value) -> i32 {
    v.as_int().unwrap_or(0)
}

/// "short|long" -> "long"; MSI file and directory names carry both forms.
pub fn long_name(name: &str) -> &str {
    name.rsplit('|').next().unwrap_or(name)
}

impl Database {
    pub fn open(path: &Path) -> Result<(Self, msi::Package<std::fs::File>)> {
        let mut pkg = msi::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut rows = |table: &str| -> Result<Vec<msi::Row>> {
            if !pkg.has_table(table) {
                return Ok(Vec::new());
            }
            Ok(pkg.select_rows(msi::Select::table(table))?.collect())
        };

        let properties = rows("Property")?.iter().map(|r| (req_str(&r["Property"]), req_str(&r["Value"]))).collect();
        let directories = rows("Directory")?
            .iter()
            .map(|r| {
                let dir = Directory { parent: opt_str(&r["Directory_Parent"]), default_dir: req_str(&r["DefaultDir"]) };
                (req_str(&r["Directory"]), dir)
            })
            .collect();
        let components = rows("Component")?
            .iter()
            .map(|r| {
                let c = Component { directory: req_str(&r["Directory_"]), condition: opt_str(&r["Condition"]) };
                (req_str(&r["Component"]), c)
            })
            .collect();
        let files = rows("File")?
            .iter()
            .map(|r| FileRow {
                key: req_str(&r["File"]),
                component: req_str(&r["Component_"]),
                name: long_name(r["FileName"].as_str().unwrap_or_default()).to_string(),
                size: int(&r["FileSize"]) as u32 as u64,
            })
            .collect();
        let mut media: Vec<(i32, Media)> =
            rows("Media")?.iter().map(|r| (int(&r["LastSequence"]), Media { cabinet: req_str(&r["Cabinet"]) })).collect();
        media.sort_by_key(|(last, _)| *last);
        let hashes = rows("MsiFileHash")?
            .iter()
            .map(|r| {
                let mut md5 = [0u8; 16];
                for (i, col) in ["HashPart1", "HashPart2", "HashPart3", "HashPart4"].iter().enumerate() {
                    md5[i * 4..i * 4 + 4].copy_from_slice(&int(&r[*col]).to_le_bytes());
                }
                (req_str(&r["File_"]), md5)
            })
            .collect();

        let media = media.into_iter().map(|(_, m)| m).collect();
        Ok((Database { properties, directories, components, files, media, hashes }, pkg))
    }

    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }

    /// Only the `PROP="value"` and bare `PROP` forms appear in these packages.
    pub fn condition_holds(&self, condition: Option<&str>) -> bool {
        let Some(cond) = condition.map(str::trim).filter(|c| !c.is_empty()) else { return true };
        match cond.split_once('=') {
            Some((prop, value)) => self.property(prop.trim()).unwrap_or("") == value.trim().trim_matches('"'),
            None => self.property(cond).is_some_and(|v| !v.is_empty()),
        }
    }

    pub fn component_installed(&self, component: &str) -> bool {
        self.components.get(component).is_some_and(|c| self.condition_holds(c.condition.as_deref()))
    }
}

/// Resolves Directory rows to backslash-separated paths, each ending in a backslash.
pub struct DirResolver<'a> {
    db: &'a Database,
    roots: HashMap<String, String>,
    cache: std::cell::RefCell<HashMap<String, String>>,
}

impl<'a> DirResolver<'a> {
    /// `roots` pins directories to fixed paths; anything not under a root fails to resolve.
    pub fn new(db: &'a Database, roots: HashMap<String, String>) -> Self {
        Self { db, roots, cache: Default::default() }
    }

    pub fn resolve(&self, id: &str) -> Result<String> {
        if let Some(p) = self.roots.get(id).cloned().or_else(|| self.cache.borrow().get(id).cloned()) {
            return Ok(p);
        }
        let dir = self.db.directories.get(id).ok_or_else(|| anyhow!("unknown directory {id}"))?;
        let parent = match &dir.parent {
            Some(p) if p != id => self.resolve(p)?,
            _ => return Err(anyhow!("directory {id} is outside every root")),
        };
        let name = long_name(dir.default_dir.split(':').next().unwrap_or(""));
        let path = if name == "." || name.is_empty() { parent } else { format!("{parent}{name}\\") };
        self.cache.borrow_mut().insert(id.to_string(), path.clone());
        Ok(path)
    }
}
