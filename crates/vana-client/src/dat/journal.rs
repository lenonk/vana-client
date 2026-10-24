//! The mission and quest logs' name files (quest_mission_spec section 3): d_msg entries {u32 number, name,
//! description}, one file per mission log (15) and per quest area (11). English ids; ja = the table's ja column.

use super::DatIndex;
use super::items::Field;
use super::names::DMsg;
use anyhow::Result;

/// Mission logs 0..14 (DAT_10667a90 + log * 0x94): San d'Oria, Bastok, Windurst, Zilart, CoP, Assault, ToAU, Campaign,
/// WotG, SoA, RoV, TVR, ACP, AMK, ASA.
pub const MISSION_FILES: [u32; 15] = [55715, 55716, 55717, 55718, 55719, 55720, 55721, 55724, 55723, 55738, 55741, 55742, 55735, 55736, 55737];
/// Quest areas 0..10: San d'Oria, Bastok, Windurst, Jeuno, Other Areas, Outlands, Aht Urhgan, Crystal War, Abyssea,
/// Adoulin, Coalition.
pub const QUEST_FILES: [u32; 11] = [55706, 55707, 55708, 55709, 55710, 55711, 55712, 55722, 55713, 55739, 55740];

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub number: u32,
    pub name: String,
    pub desc: String,
}

/// One log file's entries in file order.
#[derive(Clone, Debug, Default)]
pub struct LogFile {
    pub entries: Vec<Entry>,
}

impl LogFile {
    pub fn load(index: &DatIndex, file: u32) -> Result<LogFile> {
        let m = DMsg::load(index, file)?;
        let entries = (0..m.len())
            .map(|i| {
                let f = m.fields(i).unwrap_or_default();
                let num = f.iter().find_map(|x| if let Field::Num(n) = x { Some(*n) } else { None }).unwrap_or(0);
                let mut texts = f.iter().filter_map(|x| if let Field::Text(s) = x { Some(s.clone()) } else { None });
                Entry { number: num, name: texts.next().unwrap_or_default(), desc: texts.next().unwrap_or_default() }
            })
            .collect();
        Ok(LogFile { entries })
    }
}

/// Every mission log and quest area, loaded together on the first open.
#[derive(Clone, Debug, Default)]
pub struct Logs {
    pub missions: Vec<LogFile>,
    pub quests: Vec<LogFile>,
}

impl Logs {
    pub fn load(index: &DatIndex) -> Logs {
        let load = |files: &[u32]| files.iter().map(|&f| LogFile::load(index, f).map_err(|e| log::warn!("journal file {f}: {e:#}")).unwrap_or_default()).collect();
        Logs { missions: load(&MISSION_FILES), quests: load(&QUEST_FILES) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let l = Logs::load(&index);
        assert_eq!(l.missions[0].entries.len(), 24);
        assert_eq!((l.missions[0].entries[0].number, l.missions[0].entries[0].name.as_str()), (0, "Smash the Orcish Scouts"));
        assert_eq!(l.missions[4].entries.len(), 65);
        assert!(l.quests.iter().all(|q| !q.entries.is_empty()));
    }
}
