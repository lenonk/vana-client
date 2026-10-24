//! The party / alliance roster (session object +0x57d8, list 0: 20 records of 0x70 bytes, record 0 ourselves) as
//! the s2c 0x0C8 / 0x0DD / 0x0E2 / 0x0DF handlers fill it (LAB_100e9ee0, 100ea390, 100ea570, 100e9cc0), and the
//! party window's 18-slot table (DAT_10663ad8: 0..5 our party, 6..11 and 12..17 the alliance's other parties).

/// One roster record (the fields we use).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Member {
    pub id: u32,
    pub index: u16,
    /// +0x01 MemberNumber.
    pub number: u8,
    pub name: String,
    pub hp: u32,
    pub mp: u32,
    pub tp: u32,
    pub hpp: u8,
    pub mpp: u8,
    /// +0x2e the member's zone, +0x30 the other-zone marker (0 = in our zone).
    pub zone: u16,
    pub away: u16,
    /// +0x34 GAttr: bits 0-1 party number, 2 party leader, 3 alliance leader, 4 / 5 quartermaster, 8 level sync.
    pub flags: u32,
    /// Main job, main level, sub job, sub level, master level.
    pub jobs: [u8; 5],
    /// +0x1c the last update (the oldest record is reused when all 20 are taken).
    time: u64,
}

/// s2c 0x0C8 GROUP_TBL entry: id, ActIndex, flags (low byte), zone.
pub type TableEntry = (u32, u16, u8, u16);

/// s2c 0x0DD / 0x0E2 GROUP_LIST.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListUpdate {
    pub id: u32,
    pub index: u16,
    pub number: u8,
    pub name: String,
    pub hp: u32,
    pub mp: u32,
    pub tp: u32,
    pub hpp: u8,
    pub mpp: u8,
    pub zone: u16,
    pub flags: u32,
    pub jobs: [u8; 5],
}

/// s2c 0x0DF GROUP_ATTR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttrUpdate {
    pub id: u32,
    pub index: u16,
    pub hp: u32,
    pub mp: u32,
    pub tp: u32,
    pub hpp: u8,
    pub mpp: u8,
    pub jobs: [u8; 5],
}

pub const RECORDS: usize = 20;

#[derive(Clone, Debug)]
pub struct Roster {
    recs: Vec<Member>,
    /// Record indices in display order (FUN_100e9a70), position 0 always record 0.
    order: Vec<usize>,
    /// G+0x97f8: our own GAttr, from a 0x0DD / 0x0E2 about us.
    pub own_flags: u32,
    /// G+0x9816 bit 0: a 0x0C8 has come (the order is rebuilt only after it).
    seen_table: bool,
    clock: u64,
}

impl Default for Roster {
    fn default() -> Self {
        Self { recs: vec![Member::default(); RECORDS], order: vec![0], own_flags: 0, seen_table: false, clock: 0 }
    }
}

impl Roster {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn find(&self, id: u32) -> Option<usize> {
        (id != 0).then(|| self.recs.iter().position(|r| r.id == id)).flatten()
    }

    /// FUN_100ea140: the first free record past ours, else the one updated longest ago.
    fn alloc(&mut self) -> usize {
        let i = (1..RECORDS).find(|&i| self.recs[i].id == 0).unwrap_or_else(|| (1..RECORDS).min_by_key(|&i| self.recs[i].time).unwrap_or(1));
        self.recs[i] = Member::default();
        i
    }

    /// FUN_100e8b80: record 0 is refreshed with our name, id, ActIndex and the current zone.
    pub fn refresh_self(&mut self, id: u32, index: u16, name: &str, zone: u16) {
        let r = &mut self.recs[0];
        (r.id, r.index, r.zone) = (id, index, zone);
        r.name = name.to_string();
    }

    /// 0x0C8: present ids are updated or allocated (zone and marker set when the zone changed or new), every record
    /// whose id is not in the packet is zeroed: the only removal (leave, kick and disband arrive as an empty table).
    pub fn table(&mut self, entries: &[TableEntry]) {
        self.seen_table = true;
        let t = self.tick();
        for &(id, index, flags, zone) in entries.iter().filter(|e| e.0 != 0) {
            let i = match self.find(id) {
                Some(i) => {
                    if self.recs[i].zone != zone {
                        (self.recs[i].zone, self.recs[i].away) = (zone, zone);
                    }
                    i
                }
                None => {
                    let i = self.alloc();
                    (self.recs[i].zone, self.recs[i].away) = (zone, zone);
                    i
                }
            };
            let r = &mut self.recs[i];
            (r.id, r.index, r.time) = (id, index, t);
            r.flags = (r.flags & !0xff) | flags as u32;
        }
        for r in self.recs.iter_mut() {
            if !entries.iter().any(|e| e.0 != 0 && e.0 == r.id) {
                *r = Member::default();
            }
        }
        self.rebuild();
    }

    /// 0x0DD (`create` false: unknown ids ignored, the marker set from ZoneNo) or 0x0E2 (`create`: unknown ids
    /// added, the zone set and the marker cleared).
    pub fn list(&mut self, u: &ListUpdate, my_id: u32, create: bool) {
        // An update with an empty name is dropped whole.
        if u.name.is_empty() {
            return;
        }
        let i = if u.id == my_id && my_id != 0 {
            Some(0)
        } else {
            match self.find(u.id) {
                Some(i) => Some(i),
                None if create => Some(self.alloc()),
                None => None,
            }
        };
        let Some(i) = i else { return };
        let t = self.tick();
        let r = &mut self.recs[i];
        (r.id, r.index, r.number, r.flags, r.time) = (u.id, u.index, u.number, u.flags, t);
        (r.hp, r.mp, r.tp, r.hpp, r.mpp, r.jobs) = (u.hp, u.mp, u.tp, u.hpp, u.mpp, u.jobs);
        if !u.name.is_empty() {
            r.name = u.name.clone();
        }
        if create {
            (r.zone, r.away) = (u.zone, 0);
        } else {
            r.away = u.zone;
        }
        if i == 0 {
            self.own_flags = u.flags;
        }
        self.rebuild();
    }

    /// 0x0DF: HP / MP / TP and jobs of a record already listed (ours included); unknown ids ignored.
    pub fn attr(&mut self, u: &AttrUpdate) {
        let Some(i) = self.find(u.id) else { return };
        let t = self.tick();
        let r = &mut self.recs[i];
        (r.index, r.hp, r.mp, r.tp, r.hpp, r.mpp, r.jobs, r.time) = (u.index, u.hp, u.mp, u.tp, u.hpp, u.mpp, u.jobs, t);
    }

    /// FUN_100e9a70: the old order kept for records still there, then new ones in record order; ours first.
    fn rebuild(&mut self) {
        if !self.seen_table {
            return;
        }
        let mut order: Vec<usize> = vec![0];
        order.extend(self.order.iter().copied().filter(|&i| i != 0 && self.recs[i].id != 0));
        for i in 1..RECORDS {
            if self.recs[i].id != 0 && !order.contains(&i) {
                order.push(i);
            }
        }
        self.order = order;
    }

    /// Mode 5 (FUN_100e8b80 / FUN_100e8c70): ourselves, then everyone listed in order.
    pub fn members(&self) -> impl Iterator<Item = &Member> {
        self.order.iter().map(|&i| &self.recs[i]).filter(|r| r.id != 0)
    }

    /// Mode 0: only our own party ((flags ^ own) & 3 == 0).
    pub fn party(&self) -> impl Iterator<Item = &Member> {
        let own = self.own_flags;
        self.members().filter(move |r| (r.flags ^ own) & 3 == 0)
    }

    /// A listed member with no name yet (the iterator asks c2s 0x076 for it, FUN_100e9550).
    pub fn nameless(&self) -> bool {
        self.members().skip(1).any(|r| r.name.is_empty())
    }

    /// FUN_10201fb0 / FUN_10202b80: the 18 window slots. Ours is slot 0 in party 0; members of our party number join
    /// it, the first other number makes party 1 (slots 6..11), the next party 2 (12..17).
    pub fn slots(&self) -> Slots {
        let mut s = Slots::default();
        let mut parties: [Option<u32>; 3] = [None; 3];
        let mut counts = [0usize; 3];
        for (k, r) in self.members().enumerate() {
            let num = if k == 0 { self.own_flags & 3 } else { r.flags & 3 };
            let p = match parties.iter().position(|n| *n == Some(num)) {
                Some(p) => p,
                None => match parties.iter().position(Option::is_none) {
                    Some(p) => {
                        parties[p] = Some(num);
                        p
                    }
                    None => continue,
                },
            };
            if counts[p] >= 6 {
                continue;
            }
            let flags = if k == 0 { self.own_flags } else { r.flags };
            if flags & 4 != 0 {
                s.leaders[p] = r.id;
            }
            if flags & 8 != 0 {
                s.alliance_leader = r.id;
            }
            s.slots[p * 6 + counts[p]] = Some(r.clone());
            counts[p] += 1;
        }
        s.counts = counts;
        s
    }
}

/// The party window table.
#[derive(Clone, Debug, Default)]
pub struct Slots {
    pub slots: [Option<Member>; 18],
    /// Party leader id per window party (0 none), the alliance leader, members per party.
    pub leaders: [u32; 3],
    pub alliance_leader: u32,
    pub counts: [usize; 3],
}

impl Slots {
    /// FUN_102033c0: slot `s` when used, in our zone and not marked elsewhere.
    pub fn here(&self, s: usize, zone: u16) -> Option<&Member> {
        self.slots.get(s)?.as_ref().filter(|m| m.away == 0 && m.zone == zone)
    }

    /// The window: "menu    ptw0" with no leader in our party (solo), else "menu    ptwN".
    pub fn window(&self) -> String {
        if self.leaders[0] == 0 { "menu    ptw0".into() } else { format!("menu    ptw{}", self.counts[0].clamp(1, 6)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 0x100;

    fn listed(id: u32, index: u16, flags: u32, zone: u16, name: &str) -> ListUpdate {
        ListUpdate { id, index, flags, zone, name: name.into(), hpp: 100, ..Default::default() }
    }

    /// Two join (LSB: 0x0C8 then a 0x0DD each), one leaves (an empty-for-them 0x0C8), then disband (empty table).
    #[test]
    fn join_and_leave() {
        let mut r = Roster::default();
        r.refresh_self(ME, 0x400, "Me", 230);
        // A 0x0DD before the table is ignored for others.
        r.list(&listed(0x200, 0x401, 1, 0, "Ann"), ME, false);
        assert_eq!(r.members().count(), 1);
        r.table(&[(ME, 0x400, 4, 230), (0x200, 0x401, 0, 230), (0x300, 0x402, 0, 100)]);
        r.list(&listed(ME, 0x400, 4, 0, "Me"), ME, false);
        r.list(&listed(0x200, 0x401, 0, 0, "Ann"), ME, false);
        r.list(&listed(0x300, 0x402, 0, 100, "Bo"), ME, false);
        let s = r.slots();
        assert_eq!((s.window().as_str(), s.leaders[0]), ("menu    ptw3", ME));
        assert_eq!(s.here(1, 230).map(|m| m.name.as_str()), Some("Ann"));
        // Bo is elsewhere (marker set): not usable for F3.
        assert!(s.here(2, 230).is_none());
        // Ann leaves.
        r.table(&[(ME, 0x400, 4, 230), (0x300, 0x402, 0, 100)]);
        assert_eq!(r.members().map(|m| m.id).collect::<Vec<_>>(), vec![ME, 0x300]);
        // Disband (LSB: an empty table, then a 0x0DD about us with flags 0): record 0 refills on the next refresh.
        r.table(&[]);
        r.refresh_self(ME, 0x400, "Me", 230);
        r.list(&listed(ME, 0x400, 0, 0, "Me"), ME, false);
        assert_eq!(r.members().count(), 1);
        assert_eq!(r.slots().window(), "menu    ptw0");
    }

    /// Alliance: members of other party numbers go to slots 6.. and 12.. in order.
    #[test]
    fn alliance_slots() {
        let mut r = Roster::default();
        r.refresh_self(ME, 0x400, "Me", 230);
        r.table(&[(ME, 0x400, 0x0c, 230), (2, 1, 1, 230), (3, 2, 1 | 4, 230), (4, 3, 2 | 4, 230), (5, 4, 0, 230)]);
        r.list(&listed(ME, 0x400, 0x0c, 0, "Me"), ME, false);
        let s = r.slots();
        let ids: Vec<Option<u32>> = s.slots.iter().map(|m| m.as_ref().map(|m| m.id)).collect();
        assert_eq!((ids[0], ids[1], ids[6], ids[7], ids[12]), (Some(ME), Some(5), Some(2), Some(3), Some(4)));
        assert_eq!((s.leaders, s.alliance_leader), ([ME, 3, 4], ME));
    }
}
