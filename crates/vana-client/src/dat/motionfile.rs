//! Motion files an actor loads on top of its model (the model's +0x7e4 list, FUN_100d22a0): event
//! motion banks (event ops 0x5b / 0x66 / 0x5f) and the PC battle files while engaged (FUN_100cf650).

use super::DatIndex;
use super::character::{Animation, load_file};
use anyhow::Result;

/// Animations and type-0x07 schedulers of one motion DAT.
#[derive(Debug)]
pub struct MotionFile {
    pub id: u32,
    pub animations: Vec<Animation>,
    pub schedulers: Vec<(String, Vec<u8>)>,
    /// The animations as GPU words, built by the loader thread.
    pub packed: Option<std::sync::Arc<super::animpack::Packed>>,
}

impl MotionFile {
    pub fn load(index: &DatIndex, id: u32) -> Result<Self> {
        let f = load_file(&index.read(id)?)?;
        Ok(MotionFile { id, animations: f.animations, schedulers: f.schedulers, packed: None })
    }
}

/// What the PC motion loaders read from a PC-look model's files (FUN_100cd3e0 / FUN_100cea10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PcMotion {
    pub race: u8,
    /// Main weapon info[3] (+0x87b): battle motion set; -1 = none.
    pub weapon: i8,
    /// Main weapon info[4] (+0x87c): the "in N" / "out N" draw schedulers.
    pub draw: i8,
    /// Body info[9] (+0x881): skirt motion set, 1 or 2 (0 / -1 = none).
    pub skirt: i8,
}

/// Op 0x5b / 0x5f sub 3, 5 (FUN_100b3570 -> FUN_100d00a0): one file, val + a base by range.
pub fn bank0_file(val: i32) -> u32 {
    let v = val.max(0) as u32;
    v + match v {
        0..0x200 => 0x7d68,
        0x200..0x400 => 0xbfef,
        0x400..0x800 => 0xdc19,
        0x800..0xc00 => 0xe95b,
        _ => 0x10323,
    }
}

/// Op 0x66 / 0x5f sub 4, 6 (FUN_100d01b0, PC-look models): the motion file, then the skirt file
/// when the body has one (skirt 1 / 2 pick the first or second skirt block). Blocks of 0x46 per range.
pub fn bank1_files(val: i32, skirt: i8) -> Vec<u32> {
    if !(0..0x118).contains(&val) {
        return Vec::new();
    }
    let v = val as u32;
    let (base, k) = match v / 0x46 {
        0 => (0x7fc8, v),
        1 => (0xef39, v - 0x46),
        2 => (0x15711, v - 0x8c),
        _ => (0x18f5f, v - 0xd2),
    };
    let mut out = vec![base + k];
    if skirt > 0 {
        out.push(base + k + if skirt == 1 { 0x46 } else { 0x8c });
    }
    out
}

/// Upper-body battle file per race 0..8 (0x10352334) and the two skirt blocks (0x10352348).
const BATTLE_UPPER: [u32; 9] = [9672, 9672, 12848, 16024, 19200, 22376, 22376, 25776, 28952];
const BATTLE_SKIRT: [[u32; 2]; 9] = [
    [9800, 9928],
    [9800, 9928],
    [12976, 13104],
    [16152, 16280],
    [19328, 19456],
    [22504, 22632],
    [22504, 22632],
    [25904, 26032],
    [29080, 29208],
];

/// Engaged without dual wield (FUN_100cf650): FUN_100d0dc0(race, 0) and FUN_100d0d00(race, 1, 0).
pub fn battle_files(pc: &PcMotion) -> Vec<u32> {
    if pc.weapon < 0 {
        return Vec::new();
    }
    let r = if pc.race > 8 { 1 } else { pc.race as usize };
    let skirt = (pc.skirt as i32 - 1).clamp(0, 1) as usize;
    vec![BATTLE_UPPER[r] + pc.weapon as u32, BATTLE_SKIRT[r][skirt] + pc.weapon as u32]
}

/// Emote files of PC looks (FUN_100cfc10, model kinds 0 / 1 / 6): 0x103523dc[race] + group and the skirt
/// file 0x103523ee[race * 2 + skirt] + group.
const EMOTE_UPPER: [u32; 9] = [10056, 10056, 13232, 16408, 19584, 22760, 22984, 26160, 29336];
const EMOTE_SKIRT: [u32; 19] = [0, 10120, 10184, 10120, 10184, 13296, 13360, 16472, 16536, 19648, 19712, 22824, 22888, 23048, 23112, 26224, 26288, 29400, 29464];

/// FUN_1009f420: emote number -> motion index (table 0x103504a8); -2 none, 0x7a.. computed forms (not handled).
const EMOTE_MOTION: [i16; 97] = [
    1, 0, 127, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, -2, 18, -2, 20, 21, 22, 23, 24, -2, 26, 27, 28, -2, -2, 31, -2, 33, 34, 35, 19, 21, 38,
    24, 23, 39, 40, 48, 56, 126, 88, 80, 81, 82, 256, 257, 258, 259, 260, 261, 262, 263, 125, 272, 273, 274, 275, 276, 277, 278, 289, 96, 97,
    98, 99, -2, -2, -2, -2, 124, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123, 123,
    122,
];

/// An emote's motion index (FUN_1009f230 refuses numbers above 0x60).
pub fn emote_motion(n: u32) -> Option<u32> {
    let m = *EMOTE_MOTION.get(n as usize)?;
    (0..0x7a).contains(&m).then_some(m as u32).or_else(|| (m >= 0x80).then_some(m as u32))
}

/// FUN_100a0b90's motion index for emote `n` with the packet's Param: sitting (status 0x2f) or in a chair (0x3f..0x53)
/// only emotes 4..9, 13 and 37 move (tables 0x1035b7b8 / 0x1035b6f0: 64.. / 232..); else table 0x1035b880 with the
/// computed forms: salute Param + 2 (nation), /hurray 0x48 + the main weapon's motion class (6 bare-handed), /bell
/// Param + 0x68, /jobemote (Param & 1) + 0x88 + (Param >> 1) * 8, /aim 0xe0 + the ranged class. Err: /aim failed
/// (Param 0xffff). Not done: /aim's ranged class (taken as 0).
pub fn emote_index(n: u32, param: u16, status: u8, weapon_class: u8) -> Result<Option<u32>, ()> {
    const SEATED: [u32; 8] = [4, 5, 6, 7, 8, 9, 13, 37];
    if status == 0x2f || (0x3f..=0x53).contains(&status) {
        let base = if status == 0x2f { 64 } else { 232 };
        return Ok(SEATED.iter().position(|&e| e == n).map(|k| base + k as u32));
    }
    let Some(&m) = EMOTE_MOTION.get(n as usize) else { return Ok(None) };
    Ok(match m {
        0x7f => Some(param as u32 + 2),
        0x7e => Some(0x48 + weapon_class as u32),
        0x7c => Some(param as u32 + 0x68),
        0x7b => Some((param as u32 & 1) + 0x88 + (param as u32 >> 1) * 8),
        0x7a if param == 0xffff => return Err(()),
        0x7a => Some(0xe0),
        _ => emote_motion(n),
    })
}

/// The files (state 0xc: FUN_100cfbc0(index >> 3)) and scheduler "em0" + (index & 7) (FUN_100d0ea0) of a motion.
pub fn emote_files(pc: &PcMotion, motion: u32) -> (Vec<u32>, String) {
    let group = motion >> 3;
    let r = if pc.race > 8 { 1 } else { pc.race as usize };
    let mut files = vec![EMOTE_UPPER[r] + group];
    if pc.skirt > 0
        && let Some(b) = EMOTE_SKIRT.get(pc.skirt as usize + r * 2).filter(|b| **b != 0)
    {
        files.push(b + group);
    }
    (files, format!("em0{}", motion & 7))
}

/// "in N" / "out N" (FUN_100d0e60 / FUN_100d0e80): weapon draw and sheathe schedulers.
pub fn draw_scheduler(pc: &PcMotion, out: bool) -> String {
    format!("{}{}", if out { "out" } else { "in " }, pc.draw.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FUN_100a0b90's forms: /wave 10; salute 2 + nation; /hurray bare-handed 0x4e; /jobemote WAR (Param 0) 0x88; /aim
    /// without a ranged weapon fails; seated emotes from their own tables.
    #[test]
    fn emote_indices() {
        assert_eq!(emote_index(8, 0, 0, 6), Ok(Some(10)));
        assert_eq!(emote_index(2, 1, 0, 6), Ok(Some(3)));
        assert_eq!(emote_index(43, 0, 0, 6), Ok(Some(0x4e)));
        assert_eq!(emote_index(74, 0, 0, 6), Ok(Some(0x88)));
        assert_eq!(emote_index(96, 0xffff, 0, 6), Err(()));
        assert_eq!((emote_index(4, 0, 0x2f, 6), emote_index(37, 0, 0x40, 6), emote_index(10, 0, 0x2f, 6)), (Ok(Some(64)), Ok(Some(239)), Ok(None)));
    }

    fn index() -> Option<DatIndex> {
        DatIndex::open(&crate::config::Config::load().ffxi_dir()).ok()
    }

    fn has_sched(f: &MotionFile, n: &str) -> bool {
        f.schedulers.iter().any(|(s, _)| s == n)
    }

    /// Zone 230 scripts: 0x66 #29 'tlk0' on an Elvaan, 0x5b #417 'lklr', 0x5b #872 'hi07'.
    #[test]
    fn event_motion_files() {
        let Some(index) = index() else { return };
        assert_eq!(bank1_files(29, 1), [32741, 32811]);
        assert_eq!(bank1_files(0x50, 2), [0xef39 + 10, 0xef39 + 10 + 0x8c]);
        assert!(bank1_files(0x118, 1).is_empty());
        let f = MotionFile::load(&index, bank1_files(29, 0)[0]).unwrap();
        assert!(has_sched(&f, "tlk0") && has_sched(&f, "sha0"));
        // Elvaan skeleton: 99 bones; the upper-body tracks reach bone 98.
        assert_eq!(f.animations.iter().flat_map(|a| a.tracks.iter().map(|t| t.bone)).max(), Some(98));
        assert!(has_sched(&MotionFile::load(&index, bank0_file(417)).unwrap(), "lklr"));
        assert_eq!(bank0_file(872), 50007);
        assert!(has_sched(&MotionFile::load(&index, bank0_file(872)).unwrap(), "hi07"));
    }

    /// Rahal's emote 13 in 0x1f7 (op 0x6e): motion 15, Elvaan M file 16408 + 1 with scheduler em07.
    #[test]
    fn emote_13() {
        let Some(index) = index() else { return };
        assert_eq!(emote_motion(13), Some(15));
        assert_eq!(emote_motion(15), None);
        let (files, name) = emote_files(&PcMotion { race: 3, weapon: -1, draw: 0, skirt: 1 }, 15);
        assert_eq!((files.as_slice(), name.as_str()), ([16409, 16473].as_slice(), "em07"));
        assert!(has_sched(&MotionFile::load(&index, 16409).unwrap(), "em07"));
    }

    /// Hume M, unarmed (main 0 = file 8392, info[3] 10): battle file 9682 with btl and the swings.
    #[test]
    fn pc_battle_files() {
        let Some(index) = index() else { return };
        let pc = PcMotion { race: 1, weapon: 10, draw: 0, skirt: 1 };
        assert_eq!(battle_files(&pc), [9682, 9810]);
        let f = MotionFile::load(&index, 9682).unwrap();
        assert!(f.animations.iter().any(|a| a.name == "btl1"));
        assert!(has_sched(&f, "ati0") && has_sched(&f, "atf0") && has_sched(&f, &draw_scheduler(&pc, false)));
        let s = MotionFile::load(&index, 9810).unwrap();
        assert!(s.animations.iter().any(|a| a.name == "btl2"));
        assert!(battle_files(&PcMotion { weapon: -1, ..pc }).is_empty());
        // Sword (main 1 = file 8393, info[3] 2): 9674; its standing swing hits at "dada".
        let sword = PcMotion { weapon: 2, skirt: 2, ..pc };
        assert_eq!(battle_files(&sword), [9674, 9930]);
        let f = MotionFile::load(&index, 9674).unwrap();
        let ati0 = &f.schedulers.iter().find(|(n, _)| n == "ati0").unwrap().1;
        let p = crate::dat::sched::plan(ati0, &["dada"]);
        assert!(p.hit_at.is_some() && p.motions.first().is_some_and(|m| m.name == "at0"));
    }
}
