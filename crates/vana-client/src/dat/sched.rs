//! Type-0x07 schedulers (FUN_10057000) as far as combat motions need them. A scheduler is a header
//! (op stream at data + u32 @0x14 - 0x10, length in ticks at u32 @0x1c) and ops {u8 op, u8 length in u32s (low 5
//! bits), u16, u16 wait @4 (ticks before the next op runs), ...}; op 0 ends it.

/// One op: code, wait before the next op, and its bytes.
#[derive(Clone, Debug)]
pub struct Op<'a> {
    pub code: u8,
    pub wait: u16,
    pub raw: &'a [u8],
}

impl Op<'_> {
    /// The 4cc at +8 (motion / scheduler / sound name).
    pub fn name(&self) -> Option<&str> {
        let b = self.raw.get(8..12)?;
        std::str::from_utf8(b).ok().map(|s| s.trim_end_matches('\0'))
    }
}

pub fn ops(data: &[u8]) -> Vec<Op<'_>> {
    let mut out = Vec::new();
    // FUN_10055aa0: the offsets count from the chunk start (16 bytes before the data).
    let Some(mut at) = data.get(0x14..0x18).map(|b| (u32::from_le_bytes(b.try_into().unwrap()) as usize).saturating_sub(0x10)) else { return out };
    while at + 8 <= data.len() {
        let (code, len) = (data[at], (data[at + 1] & 0x1f) as usize * 4);
        if code == 0 || len == 0 || at + len > data.len() {
            break;
        }
        out.push(Op { code, wait: u16::from_le_bytes([data[at + 4], data[at + 5]]), raw: &data[at..at + len] });
        at += len;
    }
    out
}

/// Header u32 @0x1c: the scheduler's length in ticks.
pub fn length(data: &[u8]) -> u32 {
    data.get(0x1c..0x20).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// A motion a scheduler plays (op 0x05 -> FUN_100cb8e0): name with the region '?' stripped ("at0?" ->
/// "at0"), the tick it starts, f32 speed @0x10, the play length u16 @6, the play count byte @0x1e & 0x7f
/// (0 = loop until replaced; motion player +0x2c) and the blend-in ticks u16 @0x18.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    pub name: String,
    pub at: u32,
    pub speed: f32,
    pub ticks: u16,
    pub count: u8,
    pub blend: u16,
}

/// What a scheduler does that combat shows: its motions and when sub-scheduler `hit` (op 0x03) runs.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub motions: Vec<Motion>,
    pub hit_at: Option<u32>,
    pub length: u32,
    /// Op 0x28 (FUN_100c6b70 + FUN_100cb8e0): from here the status motion is chosen again, a 10-tick blend
    /// *(inf: back to the stance)*.
    pub stance_at: Option<u32>,
}

pub fn plan(data: &[u8], hit: &[&str]) -> Plan {
    let mut p = Plan { length: length(data), ..Default::default() };
    let mut t = 0u32;
    for op in ops(data) {
        match op.code {
            0x05 => {
                if let Some(n) = op.name() {
                    let speed = op.raw.get(16..20).map_or(1.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
                    let u16_at = |o: usize| op.raw.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                    let count = op.raw.get(0x1e).map_or(1, |b| b & 0x7f);
                    p.motions.push(Motion { name: n.trim_end_matches('?').to_string(), at: t, speed, ticks: u16_at(6), count, blend: u16_at(0x18) });
                }
            }
            0x03 if op.name().is_some_and(|n| hit.contains(&n)) && p.hit_at.is_none() => p.hit_at = Some(t),
            0x28 => p.stance_at = Some(t),
            _ => {}
        }
        t += op.wait as u32;
    }
    p
}

/// Scheduler names matching a 4cc pattern ('?' = any byte), as FUN_100cc740 counts them.
pub fn matching<'a>(names: impl Iterator<Item = &'a str>, pattern: &str) -> Vec<&'a str> {
    let p = pattern.as_bytes();
    names.filter(|n| n.len() == 4 && n.bytes().zip(p).all(|(a, &b)| b == b'?' || a == b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Model 0 (file 1300) "ati0": motion at0? for 80 ticks; "dada" (the hit) runs after 24 + 12 ticks.
    #[test]
    fn mob_attack_plan() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let file = index.read(1300).unwrap();
        let chunks = crate::dat::chunks(&file).unwrap();
        let get = |n: &str| chunks.iter().find(|c| c.kind == 7 && c.name() == n).unwrap().data;
        let p = plan(get("ati0"), &["dada"]);
        assert_eq!(p.motions, vec![Motion { name: "at0".into(), at: 0, speed: 1.0, ticks: 80, count: 1, blend: 10 }]);
        assert_eq!((p.hit_at, p.length), (Some(36), 80));
        let d = plan(get("dead"), &[]);
        assert_eq!(d.motions.iter().map(|m| (m.name.as_str(), m.at)).collect::<Vec<_>>(), [("ded", 0), ("cor0", 111)]);
        let names: Vec<String> = chunks.iter().filter(|c| c.kind == 7).map(|c| c.name()).collect();
        let mut m = matching(names.iter().map(|s| s.as_str()), "ati?");
        m.sort();
        assert_eq!(m, ["ati0", "ati1", "ati2"]);
    }
}
