//! 0x028 action packets (GP_SERV_COMMAND_BATTLE2). Bit-packed LSB first from packet byte 5 (after the
//! header and the WorkSize byte); the client's reader is FUN_10185a20 with FUN_10185550 (n bits),
//! LSB's writer GP_SERV_COMMAND_BATTLE2::pack. At most 64 targets of 8 results are read.

/// Category (cmd_no) values, LSB `ActionCategory`.
pub mod category {
    pub const BASIC_ATTACK: u8 = 1;
    pub const RANGED_FINISH: u8 = 2;
    pub const SKILL_FINISH: u8 = 3;
    pub const MAGIC_FINISH: u8 = 4;
    pub const ITEM_FINISH: u8 = 5;
    pub const ABILITY_FINISH: u8 = 6;
    pub const SKILL_START: u8 = 7;
    pub const MAGIC_START: u8 = 8;
    pub const ITEM_START: u8 = 9;
    pub const ABILITY_START: u8 = 10;
    pub const MOB_SKILL_FINISH: u8 = 11;
    pub const RANGED_START: u8 = 12;
}

/// One result (FUN_10185a20 fields; record offsets as FUN_101861b0 copies them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionResult {
    /// 3 bits: 0 hit, 1 miss, 2 guard, 3 parry, 4 block.
    pub miss: u8,
    pub kind: u8,
    /// 12 bits: animation id (sub_kind); with the category it picks the effect file (FUN_100a7380).
    pub animation: u16,
    /// 5 bits: bit 0 defeated, bit 1 critical.
    pub info: u8,
    /// 5 bits: hit distortion (2) + knockback (3).
    pub scale: u8,
    pub value: u32,
    pub message: u16,
    pub bit: u32,
    pub proc: Option<Side>,
    pub react: Option<Side>,
}

/// An additional effect (proc: 6, 4, 17, 10 bits) or reaction (6, 4, 14, 10 bits).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    pub kind: u8,
    pub info: u8,
    pub value: u32,
    pub message: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionTarget {
    pub id: u32,
    pub results: Vec<ActionResult>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Action {
    pub actor: u32,
    pub category: u8,
    /// cmd_arg: spell / ability id, or a 4cc ("atk0" for melee, "cate" skill start...).
    pub arg: u32,
    /// info: the recast for magic finish.
    pub info: u32,
    pub targets: Vec<ActionTarget>,
}

impl Action {
    /// cmd_arg as text when it is a 4cc.
    pub fn arg_4cc(&self) -> Option<String> {
        let b = self.arg.to_le_bytes();
        b.iter().all(|c| c.is_ascii_graphic()).then(|| String::from_utf8_lossy(&b).into_owned())
    }

    /// The (target, result) records the client keeps (FUN_10185a20): every result of a single target,
    /// else the first result of each target.
    pub fn records(&self) -> Vec<(u32, &ActionResult)> {
        if self.targets.len() == 1 {
            self.targets[0].results.iter().map(|r| (self.targets[0].id, r)).collect()
        } else {
            self.targets.iter().filter_map(|t| t.results.first().map(|r| (t.id, r))).collect()
        }
    }
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for k in 0..n {
            let byte = self.data.get(self.at >> 3).copied().unwrap_or(0);
            v |= ((byte >> (self.at & 7)) as u32 & 1) << k;
            self.at += 1;
        }
        v
    }
}

/// Decodes a whole 0x028 sub-packet (header included).
pub fn parse(packet: &[u8]) -> Action {
    let mut b = Bits { data: packet, at: 40 };
    let actor = b.take(32);
    let count = b.take(6);
    let _res_sum = b.take(4);
    let category = b.take(4) as u8;
    let arg = b.take(32);
    let info = b.take(32);
    let mut targets = Vec::new();
    for _ in 0..count.min(64) {
        let id = b.take(32);
        let n = b.take(4);
        let mut results = Vec::new();
        for _ in 0..n.min(8) {
            let mut r = ActionResult {
                miss: b.take(3) as u8,
                kind: b.take(2) as u8,
                animation: b.take(12) as u16,
                info: b.take(5) as u8,
                scale: b.take(5) as u8,
                value: b.take(17),
                message: b.take(10) as u16,
                bit: b.take(31),
                ..Default::default()
            };
            if b.take(1) != 0 {
                r.proc = Some(Side { kind: b.take(6) as u8, info: b.take(4) as u8, value: b.take(17), message: b.take(10) as u16 });
            }
            if b.take(1) != 0 {
                r.react = Some(Side { kind: b.take(6) as u8, info: b.take(4) as u8, value: b.take(14), message: b.take(10) as u16 });
            }
            results.push(r);
        }
        targets.push(ActionTarget { id, results });
    }
    Action { actor, category, arg, info, targets }
}

/// LSB's packer (packBitsBE + GP_SERV_COMMAND_BATTLE2::pack), for test packets and combat-shot.
pub fn pack(a: &Action) -> Vec<u8> {
    let mut buf = vec![0u8; 0x200];
    let mut at = 40usize;
    let mut put = |v: u64, n: u32| {
        for k in 0..n {
            if v >> k & 1 != 0 {
                buf[at >> 3] |= 1 << (at & 7);
            }
            at += 1;
        }
    };
    put(a.actor as u64, 32);
    put(a.targets.len().min(15) as u64, 6);
    put(0, 4);
    put(a.category as u64, 4);
    put(a.arg as u64, 32);
    put(a.info as u64, 32);
    for t in a.targets.iter().take(15) {
        put(t.id as u64, 32);
        put(t.results.len().min(8) as u64, 4);
        for r in t.results.iter().take(8) {
            put(r.miss as u64, 3);
            put(r.kind as u64, 2);
            put(r.animation as u64, 12);
            put(r.info as u64, 5);
            put(r.scale as u64, 5);
            put(r.value as u64, 17);
            put(r.message as u64, 10);
            put(r.bit as u64, 31);
            put(r.proc.is_some() as u64, 1);
            if let Some(p) = &r.proc {
                put(p.kind as u64, 6);
                put(p.info as u64, 4);
                put(p.value as u64, 17);
                put(p.message as u64, 10);
            }
            put(r.react.is_some() as u64, 1);
            if let Some(p) = &r.react {
                put(p.kind as u64, 6);
                put(p.info as u64, 4);
                put(p.value as u64, 14);
                put(p.message as u64, 10);
            }
        }
    }
    let work = at.div_ceil(8);
    buf[4] = work as u8;
    // GP_SERV header: id 0x028, size in 4-byte units (setSize(workSize + 1) rounds up).
    let size = (work + 1).div_ceil(4) * 4;
    buf.truncate(size);
    buf[0] = 0x28;
    buf[1] = ((size / 4) << 1) as u8;
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn melee() -> Action {
        // A mob (0x0100_1234) hits Tuxedo for 7, with a 3-point additional effect, then misses.
        let hit = ActionResult { kind: 1, animation: 0, scale: 1, value: 7, message: 1, proc: Some(Side { kind: 1, info: 0, value: 3, message: 163 }), ..Default::default() };
        let miss = ActionResult { miss: 1, kind: 1, message: 15, ..Default::default() };
        Action { actor: 0x0100_1234, category: category::BASIC_ATTACK, arg: u32::from_le_bytes(*b"atk0"), info: 0, targets: vec![ActionTarget { id: 0x0001_0203, results: vec![hit, miss] }] }
    }

    #[test]
    fn round_trip() {
        let a = melee();
        let p = pack(&a);
        assert_eq!(p[0], 0x28);
        assert_eq!(parse(&p), a);
        assert_eq!(parse(&p).arg_4cc().as_deref(), Some("atk0"));
        let recs = a.records();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].1.message, 15);
    }

    #[test]
    fn many_targets_keep_first_results() {
        let r = |m| ActionResult { message: m, value: 10, ..Default::default() };
        let a = Action {
            actor: 1,
            category: category::MAGIC_FINISH,
            arg: 144,
            info: 5,
            targets: vec![ActionTarget { id: 2, results: vec![r(2), r(3)] }, ActionTarget { id: 3, results: vec![r(264)] }],
        };
        let b = parse(&pack(&a));
        assert_eq!(b, a);
        assert_eq!(b.records().iter().map(|(id, r)| (*id, r.message)).collect::<Vec<_>>(), [(2, 2), (3, 264)]);
    }

    /// Bytes as LSB builds them for a known action (bit order checked by hand against packBitsBE).
    #[test]
    fn known_bits() {
        let a = Action { actor: 0x0001_0203, category: 1, arg: 0, info: 0, targets: vec![] };
        let p = pack(&a);
        assert_eq!(&p[5..9], &[0x03, 0x02, 0x01, 0x00]);
        // 6 bits of count (0), 4 of res_sum, then category 1 at bit 10 of byte 9.
        assert_eq!(p[10], 0x04);
    }
}
