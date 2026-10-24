//! The quest and mission store (s2c 0x056, LAB_10098ad0; quest_mission_spec section 2) and the rules that turn it into
//! the mission and quest lists' rows (sections 5.2, 6.2).

use crate::dat::journal::LogFile;

/// What 0x056 fills (0x1047fb48..0x1047fea8), reset once per entering the world (FUN_10093a30).
#[derive(Clone, Debug, Default)]
pub struct Store {
    /// Quest areas 0..10: current / completed, 256 bits each.
    pub current: [[u32; 8]; 11],
    pub completed: [[u32; 8]; 11],
    /// Port 0xD0: completed missions, 64 bits each for San d'Oria, Bastok, Windurst, Zilart.
    pub nations_done: [u32; 8],
    /// Port 0xD8: completed ToAU (words 0..1) and WotG (words 2..3).
    pub toau_wotg_done: [u32; 8],
    /// Ports 0x30 / 0x38: completed Campaign 0..255 / 256..511.
    pub campaign_done: [u32; 16],
    /// Port 0xFFFF: the nation, its mission (0xffff none), RotZ, CoP, CoP's sub-paths (nibbles clamped to 9), the
    /// add-on nibbles (ACP, AMK, ASA), the Tales' Beginning flags, SoA, RoV; 0xFFFE TVR.
    pub nation: u8,
    pub nation_mission: u16,
    pub rotz: u32,
    pub cop: u32,
    pub cop_sub: u32,
    pub addons: u16,
    pub tales: u16,
    pub soa: u32,
    pub rov: u32,
    pub tvr: u32,
    /// DAT_1047feac: the records received (bits 0..10 current areas, 11..21 completed, 22..25 the done lists, 31 the
    /// mission record).
    pub received: u32,
}

/// The port of each quest area's current (0x50 + 8 a, then 0xE0, 0xF0, 0x100) and completed (+0x40, 0xE8 ..) record.
fn area_port(port: u16) -> Option<(usize, bool)> {
    match port {
        0x50..=0x88 if port % 8 == 0 => Some((((port - 0x50) / 8) as usize, false)),
        0x90..=0xc8 if port % 8 == 0 => Some((((port - 0x90) / 8) as usize, true)),
        0xe0 => Some((8, false)),
        0xe8 => Some((8, true)),
        0xf0 => Some((9, false)),
        0xf8 => Some((9, true)),
        0x100 => Some((10, false)),
        0x108 => Some((10, true)),
        _ => None,
    }
}

impl Store {
    /// One s2c 0x056 record: data u32[8] @4, Port @0x24. Unknown ports are refused.
    pub fn record(&mut self, port: u16, d: [u32; 8]) {
        let bit = match (port, area_port(port)) {
            (_, Some((a, done))) => {
                if done { self.completed[a] = d } else { self.current[a] = d }
                a + if done { 11 } else { 0 }
            }
            (0xd0, _) => {
                self.nations_done = d;
                22
            }
            (0xd8, _) => {
                self.toau_wotg_done = d;
                23
            }
            (0x30, _) => {
                self.campaign_done[..8].copy_from_slice(&d);
                24
            }
            (0x38, _) => {
                self.campaign_done[8..].copy_from_slice(&d);
                25
            }
            (0xffff, _) => {
                self.nation = d[0] as u8;
                self.nation_mission = d[1] as u16;
                self.rotz = d[2];
                self.cop = d[3];
                let nibbles = (0..5).fold(0, |a, k| a | ((d[4] >> (4 * k) & 0xf).min(9)) << (4 * k));
                self.cop_sub = nibbles | (d[4] & 0xfff0_0000);
                (self.addons, self.tales) = (d[5] as u16, (d[5] >> 16) as u16);
                self.soa = d[6];
                self.rov = d[7];
                31
            }
            (0xfffe, _) => {
                self.tvr = d[0];
                31
            }
            _ => return,
        };
        self.received |= 1 << bit;
    }

    /// Byte 0x1047fe8f: every record is in (0x83ffffff).
    pub fn complete(&self) -> bool {
        self.received == 0x83ff_ffff
    }
}

fn bit(words: &[u32], n: usize) -> bool {
    words.get(n / 32).is_some_and(|w| w >> (n % 32) & 1 != 0)
}

/// The mission logs' titles (group 0xd), file order as dat::journal::MISSION_FILES.
pub const MISSION_TITLES: [usize; 15] = [0x15, 0x16, 0x17, 0x18, 0x83, 0x86, 0x87, 0x8f, 0x90, 0x9f, 0xb7, 0xbc, 0x98, 0x99, 0x9a];
/// DAT_10386984: the expansion bit each log needs (0 = always).
pub const MISSION_BITS: [u32; 15] = [0, 0, 0, 1, 2, 3, 3, 4, 4, 0xb, 0, 0, 5, 6, 7];

/// A row of miss00 (FUN_1022dd30's codes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// -1 "Current:" (0xc 0x36) / -2 "Completed:" (0xc 0x37).
    CurHead,
    DoneHead,
    /// An entry index: the current mission (indent 0), or a completed one (indented by its '_' count).
    Current(usize),
    Done(usize),
    /// A line of group 0xc: 0x38 "Not allowed under current allegiance.", 0x39 "No current missions.", 0x3a "No
    /// completed missions.".
    Msg(usize),
}

/// The value the SoA / RoV / TVR rules compare numbers with.
fn later_value(s: &Store, log: usize) -> u32 {
    match log {
        9 => s.soa,
        10 => s.rov,
        _ => s.tvr & 0xefff,
    }
}

/// FUN_1022dfa0: the current mission's entry index, or the message id.
pub fn current(s: &Store, log: usize, f: &LogFile) -> Result<usize, usize> {
    let n = f.entries.len();
    let none = Err(0x39);
    let idx = |i: usize| if i < n { Ok(i) } else { none };
    match log {
        0..=2 if log != s.nation as usize => Err(0x38),
        0..=2 if s.nation_mission == 0xffff => none,
        0..=2 => idx(s.nation_mission as usize),
        3 if s.rotz & 0xffff == 0xffff => none,
        3 if s.rotz <= 3 && s.tales & 1 != 0 => idx(0x3f),
        3 => idx(s.rotz as usize),
        4 => {
            let target = if s.cop == 0 && s.tales & 8 != 0 { 1000 } else { s.cop };
            f.entries.iter().position(|e| e.number == target).or_else(|| (0..n).filter(|&i| f.entries[i].number <= target).max_by_key(|&i| f.entries[i].number)).ok_or(0x39)
        }
        // Area 6's current words 4..7: Assault, ToAU, Campaign, WotG.
        5 | 7 if s.current[6][if log == 5 { 4 } else { 7 }] == 0 => none,
        5 => idx(s.current[6][4] as usize),
        6 => idx(s.current[6][5] as usize),
        7 => idx(s.current[6][7] as usize),
        8 => idx(s.current[6][6] as usize),
        9..=11 => {
            let mut v = later_value(s, log);
            if v == 999 || (log == 11 && s.tvr >> 31 != 0) {
                return none;
            }
            if v == 0 && ((log == 9 && s.tales & 0x20 != 0) || (log == 10 && s.tales & 0x40 != 0)) {
                v = 1000;
            }
            (0..n).rev().find(|&i| f.entries[i].number <= v).ok_or(0x39)
        }
        _ => {
            let nib = (s.addons >> (4 * (log - 12))) & 0xf;
            let i = match (log, nib) {
                (12, 0) if s.tales & 2 != 0 => 0xc,
                (14, 0) if s.tales & 4 != 0 => 0xf,
                _ => nib as usize,
            };
            idx(i)
        }
    }
}

/// FUN_1022e3a0: entry `i` is completed.
pub fn completed(s: &Store, log: usize, f: &LogFile, i: usize) -> bool {
    let num = |k: usize| f.entries.get(k).map_or(u32::MAX, |e| e.number) as i64;
    match log {
        0..=3 => bit(&s.nations_done, log * 64 + i),
        4 => {
            let (n, sub) = (num(i), s.cop_sub as i64);
            let nib = |k: i64| (sub >> (4 * k)) & 0xf;
            let within = |base: i64, k: i64| (0..=nib(k)).contains(&(n - base));
            match s.cop {
                0x145 => within(330, 0) || within(340, 1) || n <= 325,
                0x212 => within(540, 2) || within(550, 3) || within(560, 4) || n <= 530,
                c => n <= c as i64,
            }
        }
        5 => i < 0x80 && bit(&s.completed[6], 128 + i),
        6 => i < 0x40 && bit(&s.toau_wotg_done, i),
        7 => i < 0x200 && bit(&s.campaign_done, i),
        8 => i < 0x40 && bit(&s.toau_wotg_done, 64 + i),
        9..=11 => i + 1 < f.entries.len() && num(i + 1) <= later_value(s, log) as i64,
        _ => i < ((s.addons >> (4 * (log - 12))) & 0xf) as usize,
    }
}

/// FUN_1022dd30: "Current:", the current row, "Completed:", every completed entry in file order (or "No completed
/// missions.").
pub fn mission_rows(s: &Store, log: usize, f: &LogFile) -> Vec<Row> {
    let mut rows = vec![Row::CurHead, current(s, log, f).map_or_else(Row::Msg, Row::Current), Row::DoneHead];
    let done: Vec<Row> = (0..f.entries.len()).filter(|&i| completed(s, log, f, i)).map(Row::Done).collect();
    if done.is_empty() {
        rows.push(Row::Msg(0x3a));
    } else {
        rows.extend(done);
    }
    rows
}

/// The log after `log` in the Right / Left cycle (FUN_1022e760 messages 3 / 4): logs 3 and up need their expansion
/// bit, TVR (11) needs RoV past 340.
pub fn next_log(s: &Store, log: usize, enabled: u32, forward: bool) -> usize {
    let mut l = log;
    for _ in 0..15 {
        l = if forward { (l + 1) % 15 } else { (l + 14) % 15 };
        let bit_ok = l < 3 || enabled >> MISSION_BITS[l] & 1 != 0;
        if bit_ok && !(l == 11 && s.rov < 0x154) {
            return l;
        }
    }
    log
}

/// The entry's name without its '_' indent marks and that count (6 px each).
pub fn indent(name: &str) -> (&str, usize) {
    let t = name.trim_start_matches('_');
    (t, name.len() - t.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::journal::Entry;

    fn file(nums: &[u32]) -> LogFile {
        LogFile { entries: nums.iter().map(|&n| Entry { number: n, name: format!("m{n}"), desc: String::new() }).collect() }
    }

    #[test]
    fn records() {
        let mut s = Store::default();
        s.record(0xffff, [1, 5, 0, 0x145, 0x000c_c0a3, 0x0002_0021, 0, 0]);
        assert_eq!((s.nation, s.nation_mission, s.cop_sub & 0xfffff, s.addons, s.tales), (1, 5, 0x99093, 0x21, 2));
        s.record(0x58, [1; 8]);
        s.record(0x108, [2; 8]);
        s.record(0x1234, [0; 8]);
        assert_eq!((s.current[1][0], s.completed[10][0], s.received), (1, 2, 1 << 31 | 1 << 1 | 1 << 21));
        assert!(!s.complete());
        for p in (0x50..=0xc8).step_by(8).chain([0xe0, 0xe8, 0xf0, 0xf8, 0x100, 0xd0, 0xd8, 0x30, 0x38]) {
            s.record(p, [0; 8]);
        }
        assert!(s.complete());
    }

    #[test]
    fn mission_rules() {
        let mut s = Store { nation: 1, nation_mission: 0xffff, ..Default::default() };
        let f = file(&[0, 1, 2, 3]);
        assert_eq!(mission_rows(&s, 0, &f), vec![Row::CurHead, Row::Msg(0x38), Row::DoneHead, Row::Msg(0x3a)]);
        assert_eq!(current(&s, 1, &f), Err(0x39));
        s.nation_mission = 2;
        s.nations_done[2] = 0b11;
        assert_eq!(mission_rows(&s, 1, &f), vec![Row::CurHead, Row::Current(2), Row::DoneHead, Row::Done(0), Row::Done(1)]);
        // CoP at "The Road Forks" (325) with path nibbles 1 and 0; "<=" lists the current one as done too.
        let cop = file(&[300, 325, 330, 331, 332, 340, 341]);
        s.cop = 0x145;
        s.cop_sub = 0x01;
        assert_eq!(current(&s, 4, &cop), Ok(1));
        assert_eq!((0..7).map(|i| completed(&s, 4, &cop, i)).collect::<Vec<_>>(), [true, true, true, true, false, true, false]);
        // SoA: done once the next entry's number is reached; 0 with the declined flag counts as 1000.
        let soa = file(&[10, 10, 20, 30, 1000]);
        s.soa = 20;
        assert_eq!(current(&s, 9, &soa), Ok(2));
        assert_eq!((0..5).map(|i| completed(&s, 9, &soa, i)).collect::<Vec<_>>(), [true, true, false, false, false]);
        s.soa = 0;
        s.tales = 0x20;
        assert_eq!(current(&s, 9, &soa), Ok(4));
        // ACP: the nibble, and 0 with Tales bit 1 is "To commence" (0xc).
        let acp = file(&[0; 13]);
        s.tales = 2;
        assert_eq!(current(&s, 12, &acp), Ok(0xc));
        s.addons = 3;
        assert_eq!(((0..4).filter(|&i| completed(&s, 12, &acp, i)).count(), current(&s, 12, &acp)), (3, Ok(3)));
    }

    #[test]
    fn log_cycle() {
        let s = Store { rov: 100, ..Default::default() };
        // Only bit 0 (and 1 Zilart): 0 1 2 3 10 then back to 0 (TVR waits for RoV 340).
        let order: Vec<usize> = std::iter::successors(Some(0), |&l| Some(next_log(&s, l, 0b11, true))).take(6).collect();
        assert_eq!(order, [0, 1, 2, 3, 10, 0]);
        assert_eq!(next_log(&s, 0, 0b11, false), 10);
        assert_eq!(indent("__Name"), ("Name", 2));
    }
}
