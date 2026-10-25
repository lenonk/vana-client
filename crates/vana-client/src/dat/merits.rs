//! Merit point and job point tables (status_rows_spec.md 0.4), read raw as FUN_101c4740 does: merit categories (file 89)
//! and entries (file 88), job point categories (file 94) and entries (file 93), a job's gifts (file 100407 + job); the
//! names and descriptions from the d_msg files of language slots 0x1d / 0x1e / 0x1f (strings 2i and 2i + 1).

use super::DatIndex;
use super::names::DMsg;

fn u16_at(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// A merit category (file 89, 96 x 12, the record number = the list position, FUN_10160f00): u16 @0 its name id
/// (FUN_10161070: strings[2 x u16 @0]), @3 the category id, @4 the cap (total upgrades); u32 @8 a job mask (unread).
#[derive(Clone, Copy, Debug, Default)]
pub struct MeritCat {
    pub name: u16,
    pub id: u8,
    pub cap: u8,
}

/// A merit (file 88, 3072 x 12, record = index = category * 32 + k): @4 the most upgrades (0 = none), u32 @8 the jobs.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeritEntry {
    pub max: u8,
    pub jobs: u32,
}

pub struct MeritTables {
    pub cats: Vec<MeritCat>,
    pub entries: Vec<MeritEntry>,
    names: Option<DMsg>,
}

impl MeritTables {
    pub fn load(index: &DatIndex, names: Option<u32>) -> MeritTables {
        let read = |f: u32| index.read(f).map_err(|e| log::warn!("merit table {f}: {e:#}")).unwrap_or_default();
        let (c, e) = (read(89), read(88));
        let cats = c.chunks_exact(12).map(|r| MeritCat { name: u16_at(r, 0), id: r[3], cap: r[4] }).collect();
        let entries = e.chunks_exact(12).map(|r| MeritEntry { max: r[4], jobs: u32_at(r, 8) }).collect();
        let names = names.and_then(|f| DMsg::load(index, f).map_err(|e| log::warn!("merit names {f}: {e:#}")).ok());
        MeritTables { cats, entries, names }
    }

    /// The category at list position `pos` (1..31).
    pub fn at(&self, pos: u16) -> Option<&MeritCat> {
        self.cats.get(pos as usize)
    }

    /// The category with id `id` (a listed one, or a job's second group at id + 0x1a).
    pub fn cat(&self, id: u8) -> Option<&MeritCat> {
        self.cats.iter().skip(1).find(|c| c.id == id && c.cap != 0)
    }

    pub fn entry(&self, index: u16) -> Option<&MeritEntry> {
        self.entries.get(index as usize).filter(|e| e.max != 0)
    }

    /// Name (2i) and description (2i + 1) of name id `i`: a category's u16 @0, a merit's index.
    pub fn name(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize)).unwrap_or_default()
    }

    pub fn desc(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize + 1)).unwrap_or_default()
    }
}

/// Job point tables: categories (file 94, 32 x 0x44: u8 @2 job, u16 entry ids from @4, display order), entries (file
/// 93, 768 x 4, record = entry id: @3 the most upgrades), the names (slot 0x1e: 2 job / 2 job + 1 the job's, 2 id / 2 id
/// + 1 the entry's).
pub struct JobPointTables {
    /// Per job 1..: its entry ids.
    pub jobs: Vec<(u8, Vec<u16>)>,
    pub max: Vec<u8>,
    names: Option<DMsg>,
}

impl JobPointTables {
    pub fn load(index: &DatIndex, names: Option<u32>) -> JobPointTables {
        let read = |f: u32| index.read(f).map_err(|e| log::warn!("job point table {f}: {e:#}")).unwrap_or_default();
        let (c, e) = (read(94), read(93));
        let jobs = c
            .chunks_exact(0x44)
            .filter(|r| r[2] != 0)
            .map(|r| (r[2], (0..32).map(|k| u16_at(r, 4 + 2 * k)).filter(|&id| id != 0).collect()))
            .collect();
        let max = e.chunks_exact(4).map(|r| r[3]).collect();
        let names = names.and_then(|f| DMsg::load(index, f).map_err(|e| log::warn!("job point names {f}: {e:#}")).ok());
        JobPointTables { jobs, max, names }
    }

    pub fn entries(&self, job: u8) -> &[u16] {
        self.jobs.iter().find(|j| j.0 == job).map_or(&[], |j| &j.1)
    }

    pub fn name(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize)).unwrap_or_default()
    }

    pub fn desc(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize + 1)).unwrap_or_default()
    }
}

/// Alter ego tables (FUN_10161670): categories file 98 (8 x 0x24: u16 @0 k, u16[16] @4 attribute ids), attributes
/// file 97 (128 x 4: u8 @0 k, @3 max); names from language slot 0x20 at [2 id] / [2 id + 1] for both.
pub struct AeTables {
    /// The categories with an id and a first attribute: (k, attribute ids).
    pub cats: Vec<(u16, Vec<u16>)>,
    pub max: Vec<u8>,
    names: Option<DMsg>,
}

impl AeTables {
    pub fn load(index: &DatIndex, names: Option<u32>) -> AeTables {
        let read = |f: u32| index.read(f).map_err(|e| log::warn!("alter ego table {f}: {e:#}")).unwrap_or_default();
        let (c, e) = (read(98), read(97));
        let cats = c
            .chunks_exact(0x24)
            .map(|r| (u16_at(r, 0), (0..16).map(|k| u16_at(r, 4 + 2 * k)).filter(|&id| id != 0).collect::<Vec<_>>()))
            .filter(|(k, es)| *k != 0 && !es.is_empty())
            .collect();
        let max = e.chunks_exact(4).map(|r| r[3]).collect();
        let names = names.and_then(|f| DMsg::load(index, f).map_err(|e| log::warn!("alter ego names {f}: {e:#}")).ok());
        AeTables { cats, max, names }
    }

    pub fn entries(&self, k: u16) -> &[u16] {
        self.cats.iter().find(|c| c.0 == k).map_or(&[], |c| &c.1)
    }

    pub fn name(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize)).unwrap_or_default()
    }

    pub fn desc(&self, i: u16) -> String {
        self.names.as_ref().and_then(|n| n.text(2 * i as usize + 1)).unwrap_or_default()
    }
}

/// A job's gifts (FUN_101613d0): file 100407 + job, 256 x u16 the job points spent each needs (0 = none); names from
/// slot 0x1f at 2 ((job - 1) * 256 + i).
pub struct Gifts {
    pub job: u8,
    pub needs: Vec<u16>,
    names: Option<DMsg>,
}

impl Gifts {
    pub fn load(index: &DatIndex, job: u8, names: Option<u32>) -> Gifts {
        let f = 100407 + job as u32;
        let d = index.read(f).map_err(|e| log::warn!("gift table {f}: {e:#}")).unwrap_or_default();
        let needs = (0..256).map(|i| u16_at(&d, 2 * i)).collect();
        let names = names.and_then(|f| DMsg::load(index, f).map_err(|e| log::warn!("gift names {f}: {e:#}")).ok());
        Gifts { job, needs, names }
    }

    fn at(&self, i: usize) -> usize {
        2 * ((self.job as usize).saturating_sub(1) * 256 + i)
    }

    pub fn name(&self, i: usize) -> String {
        self.names.as_ref().and_then(|n| n.text(self.at(i))).unwrap_or_default()
    }

    pub fn desc(&self, i: usize) -> String {
        self.names.as_ref().and_then(|n| n.text(self.at(i) + 1)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Option<DatIndex> {
        DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()
    }

    /// File 89: positions 1..28 listed (HP/MP cap 75 .. Rune Fencer); file 88: Max HP (index 32) up to 15.
    #[test]
    fn merit_tables() {
        let Some(ix) = index() else { return };
        let t = MeritTables::load(&ix, None);
        let (hp, ws, run) = (t.at(1).unwrap(), t.at(6).unwrap(), t.at(28).unwrap());
        assert_eq!((hp.id, hp.cap, ws.id, ws.cap, run.id, run.cap), (1, 75, 0x1a, 15, 0x1c, 10));
        assert_eq!(t.entry(32).map(|e| e.max), Some(15));
        assert!(t.entry(35).is_none());
    }

    /// File 94: 22 jobs with ten entries each (WAR's start 0x20, 0x22, 0x21).
    #[test]
    fn job_point_tables() {
        let Some(ix) = index() else { return };
        let t = JobPointTables::load(&ix, None);
        assert_eq!(t.jobs.len(), 22);
        assert_eq!(&t.entries(1)[..3], &[0x20, 0x22, 0x21]);
        assert_eq!(t.entries(1).len(), 10);
        assert_eq!(t.max.get(0x20), Some(&20));
    }
}
