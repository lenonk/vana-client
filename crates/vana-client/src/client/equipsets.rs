//! Equipment sets (spec ~/ffxi/lab/research/social_rows_spec.md M2): the store (DAT_106627d8) of 200 sets in
//! USER/<id>/es<page>.dat (version 1, 20 records of 0x50 a page), equipping a set (c2s 0x051: /equipset, mcresmn
//! Equip), lockstyle (c2s 0x053: /lockstyle, /lockstyleset, Set appearance, the field entry's Continue / Disable).
//! The Monstrosity refusals are not done (we keep no Monstrosity state).

use super::World;
use super::net::NetCommand;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const RECORD: usize = 0x50;
/// The equip bags in the fix-up's order (FUN_101a7930): inventory, Wardrobe, Wardrobe 2..8.
const BAGS: [u8; 9] = [0, 8, 10, 11, 12, 13, 14, 15, 16];

/// One set: the name (at most 15 chars) and per equipment slot {state | container << 2, index, u16 id}; state 0 keep,
/// 1 an item, 2 remove.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Set {
    pub name: [u8; 16],
    pub slots: [[u8; 4]; 16],
}

impl Set {
    /// FUN_101a7530: "Set%03d" (string 0x1037ab20) with every slot 0.
    pub fn new(id: usize) -> Self {
        let mut name = [0u8; 16];
        let s = format!("Set{:03}", id + 1);
        name[..s.len()].copy_from_slice(s.as_bytes());
        Set { name, slots: [[0; 4]; 16] }
    }

    fn parse(b: &[u8]) -> Self {
        let mut s = Set { name: [0; 16], slots: [[0; 4]; 16] };
        s.name.copy_from_slice(&b[..16]);
        for (k, e) in s.slots.iter_mut().enumerate() {
            e.copy_from_slice(&b[16 + 4 * k..20 + 4 * k]);
        }
        s
    }

    pub fn raw_name(&self) -> &[u8] {
        &self.name[..self.name.iter().position(|&c| c == 0).unwrap_or(16)]
    }

    pub fn state(&self, slot: usize) -> u8 {
        self.slots[slot][0] & 3
    }

    /// (container, index, item id) of a slot.
    pub fn entry(&self, slot: usize) -> (u8, u8, u16) {
        let e = self.slots[slot];
        (e[0] >> 2, e[1], u16::from_le_bytes([e[2], e[3]]))
    }
}

pub struct EquipSets {
    dir: PathBuf,
    /// The page in memory (mcres20 +0x24, kept for the session) once the list has been opened.
    pub page: Option<usize>,
    pub sets: Vec<Set>,
    /// mcresmn's Copy (DAT_106627e8), freed when mcres20 closes.
    pub clip: Option<Set>,
    last_equip: Option<Instant>,
    pub last_lock: Option<Instant>,
    /// DAT_106627dc: the last equip's print flag (for s2c 0x117).
    print: bool,
}

impl EquipSets {
    pub fn new(dir: PathBuf) -> Self {
        EquipSets { dir, page: None, sets: Vec::new(), clip: None, last_equip: None, last_lock: None, print: false }
    }

    fn path(&self, page: usize) -> PathBuf {
        self.dir.join(format!("es{page}.dat"))
    }

    /// FUN_101a8820: the page's file with its MD5 checked, else the defaults in memory.
    pub fn load_page(&mut self, page: usize) {
        self.page = Some(page);
        self.sets = match super::macros::read_file(&self.path(page), 1).filter(|p| p.len() == 20 * RECORD) {
            Some(p) => p.chunks(RECORD).map(Set::parse).collect(),
            None => (0..20).map(|i| Set::new(page * 20 + i)).collect(),
        };
    }

    /// FUN_101a88d0: the page written (on every page change and when the list closes).
    pub fn save_page(&self) {
        let Some(page) = self.page else { return };
        let mut p = Vec::with_capacity(20 * RECORD);
        for s in &self.sets {
            p.extend_from_slice(&s.name);
            p.extend(s.slots.iter().flatten());
        }
        super::macros::write_file(&self.path(page), 1, &p);
    }

    /// FUN_101a8a40: a set of the page in memory, any other read alone from its file (version only, no MD5); a missing
    /// file fails.
    pub fn get(&self, id: usize) -> Option<Set> {
        if id >= 200 {
            return None;
        }
        if self.page == Some(id / 20) {
            return self.sets.get(id % 20).copied();
        }
        let b = std::fs::read(self.path(id / 20)).ok()?;
        let at = 0x18 + (id % 20) * RECORD;
        (b.len() >= at + RECORD && b[0..4] == [1, 0, 0, 0]).then(|| Set::parse(&b[at..at + RECORD]))
    }

    /// FUN_101a8990: the name, "Set%03d" when empty, at most 15 chars.
    pub fn rename(&mut self, row: usize, raw: &[u8]) {
        let Some(page) = self.page else { return };
        let Some(s) = self.sets.get_mut(row) else { return };
        if raw.is_empty() {
            s.name = Set::new(page * 20 + row).name;
        } else {
            let n = raw.len().min(15);
            s.name = [0; 16];
            s.name[..n].copy_from_slice(&raw[..n]);
        }
    }

    /// FUN_101a75a0: the set's defaults.
    pub fn clear(&mut self, row: usize) {
        if let (Some(page), Some(s)) = (self.page, self.sets.get_mut(row)) {
            *s = Set::new(page * 20 + row);
        }
    }

    /// The lockstyle wait (9 s) is still running: mcresmn's Set appearance is dimmed.
    pub fn lock_waiting(&self) -> bool {
        self.last_lock.is_some_and(|t| t.elapsed() < Duration::from_secs(9))
    }
}

fn send(w: &World, id: u16, body: Vec<u8>) {
    let _ = w.net.commands.send(NetCommand::Packet { id, body });
}

/// FUN_100ad660 in chat mode 0xca: a group 0xc line with the set number.
fn set_line(w: &mut World, msg: usize, set: usize) {
    let Some(bt) = w.battle.clone() else { return };
    let line = super::textcmd::printf(&bt.group12(msg).unwrap_or_default(), &[(set + 1).to_string()]);
    let colour = bt.colour_in(0xca, &w.game);
    w.chat.push_colour(line, colour);
}

/// FUN_101a75c0: Overwrite: every slot "remove", then each worn piece as an item.
pub fn from_equipped(w: &World) -> [[u8; 4]; 16] {
    let mut slots = [[2u8, 0, 0, 0]; 16];
    for (slot, e) in w.inv.equip.iter().enumerate() {
        if let Some((c, i)) = *e
            && let Some(&(id, _)) = w.inv.slots.get(&(c, i))
        {
            let id = id.to_le_bytes();
            slots[slot] = [1 | c << 2, i, id[0], id[1]];
        }
    }
    slots
}

/// FUN_101a7930 on a copy: a piece still at its place in an equip bag stays; else found again in its container (not
/// used by another slot), else in the equip bags in order; not found: (print) 7031 0x113 "Unable to find the <item>."
/// and the slot is dropped. Entries {index, slot, container}; "remove" is {0, slot, 0}.
fn fix_up(w: &mut World, set: &Set, print: bool) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    let mut used: Vec<(u8, u8)> = Vec::new();
    for slot in 0..16 {
        match set.state(slot) {
            2 => out.push([0, slot as u8, 0]),
            1 => {
                let (c, i, id) = set.entry(slot);
                let inv = &w.inv.slots;
                let find = |bag: u8, used: &[(u8, u8)]| inv.range((bag, 1)..=(bag, 255)).find(|(k, v)| v.0 == id && !used.contains(k)).map(|(&k, _)| k);
                let stays = BAGS.contains(&c) && inv.get(&(c, i)).is_some_and(|v| v.0 == id) && !used.contains(&(c, i));
                let at = if stays { Some((c, i)) } else { find(c, &used).or_else(|| BAGS.iter().find_map(|&b| find(b, &used))) };
                match at {
                    Some((c, i)) => {
                        used.push((c, i));
                        out.push([i, slot as u8, c]);
                    }
                    None if print => super::post::system_line(w, 0x113, [id as i32, 0, 0, 0, 0], String::new()),
                    None => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// FUN_101a7af0: set >= 200, no record or within 900 ms of the last: false. Else c2s 0x051 {count, entries {index,
/// slot, container, 0} from @8} when any; print: group 0xc 0x15d "Set #%d equipped.".
pub fn equip(w: &mut World, id: usize, print: bool) -> bool {
    // Equip sets do nothing in Monstrosity (FUN_101a7af0 returns 0).
    if super::monstrosity::active(w) {
        return false;
    }
    let Some(es) = w.equipsets.as_mut() else { return false };
    let Some(set) = es.get(id) else { return false };
    if es.last_equip.is_some_and(|t| t.elapsed() < Duration::from_millis(900)) {
        return false;
    }
    (es.last_equip, es.print) = (Some(Instant::now()), print);
    let entries = fix_up(w, &set, print);
    if !entries.is_empty() {
        let mut b = vec![0u8; 0x44];
        b[0] = entries.len() as u8;
        for (k, e) in entries.iter().enumerate() {
            b[4 + 4 * k..7 + 4 * k].copy_from_slice(e);
        }
        log::info!("equip set {}: c2s 0x051 {entries:?}", id + 1);
        send(w, 0x051, b);
    }
    if print {
        set_line(w, 0x15d, id);
    }
    true
}

/// The 9 s lockstyle wait: refusal index 0x22 (group 7 0xc1 "You must wait longer before repeating that action.").
fn lock_wait(w: &mut World) -> bool {
    if w.equipsets.as_ref().is_some_and(|e| e.lock_waiting()) {
        super::textcmd::refuse(w, 0xc1);
        return true;
    }
    if let Some(es) = w.equipsets.as_mut() {
        es.last_lock = Some(Instant::now());
    }
    false
}

/// FUN_101a7df0: set >= 200 or no record: false; within 9 s the refusal. Else c2s 0x053 mode 3 with the visible slots
/// 0..8 {index, slot, container, 0, u16 id, 0} (when any), config 0xbf = 1, print group 0xc 0x1c8.
pub fn lock_set(w: &mut World, id: usize, print: bool) -> bool {
    if super::monstrosity::active(w) {
        return false;
    }
    let Some(set) = w.equipsets.as_ref().and_then(|e| e.get(id)) else { return false };
    if lock_wait(w) {
        return true;
    }
    let mut entries = Vec::new();
    for slot in 0..9 {
        let (c, i, item) = set.entry(slot);
        let item = item.to_le_bytes();
        match set.state(slot) {
            1 => entries.push([i, slot as u8, c, 0, item[0], item[1], 0, 0]),
            2 => entries.push([0, slot as u8, 0, 0, 0, 0, 0, 0]),
            _ => {}
        }
    }
    if entries.is_empty() {
        return true;
    }
    let mut b = vec![0u8; 0x84];
    (b[0], b[1], b[2]) = (entries.len() as u8, 3, print as u8);
    for (k, e) in entries.iter().enumerate() {
        b[4 + 8 * k..12 + 8 * k].copy_from_slice(e);
    }
    log::info!("lockstyle set {}: c2s 0x053 mode 3, {} entries", id + 1, entries.len());
    send(w, 0x053, b);
    super::textcmd::cfg(w, 0xbf, 1);
    if print {
        set_line(w, 0x1c8, id);
    }
    true
}

/// FUN_101a8030(mode, no_wait, flags): c2s 0x053 {count 0, mode, flags}; modes other than 2 (Query) wait 9 s unless
/// `no_wait`; mode 0 clears config 0xbf, 1 / 3 / 4 set it.
pub fn lock_mode(w: &mut World, mode: u8, no_wait: bool, flags: u8) {
    if super::monstrosity::active(w) {
        return;
    }
    if mode != 2 && !no_wait && lock_wait(w) {
        return;
    }
    let mut b = vec![0u8; 0x84];
    (b[1], b[2]) = (mode, flags);
    log::info!("lockstyle: c2s 0x053 mode {mode}");
    send(w, 0x053, b);
    match mode {
        0 => super::textcmd::cfg(w, 0xbf, 0),
        1 | 3 | 4 => super::textcmd::cfg(w, 0xbf, 1),
        _ => {}
    }
}

/// FUN_101a80d0 at every field entry: Continue (1) with config 0xbf set, else Disable (0).
pub fn field_entry(w: &mut World) {
    let mode = (w.game.get(0xbf) != 0) as u8;
    lock_mode(w, mode, true, 0);
}

/// s2c 0x117 (LAB_101a6fb0): each changed piece (index != 0) not among the equipped ones: 7031 0x112 "Failed to equip
/// the <item>." while the last equip printed.
pub fn on_res(w: &mut World, count: u8, changed: &[[u8; 4]; 16], equipped: &[[u8; 4]; 16]) {
    if !w.equipsets.as_ref().is_some_and(|e| e.print) {
        return;
    }
    for e in changed.iter().take(count as usize).filter(|e| e[0] != 0 && !equipped.contains(e)) {
        if let Some(&(id, _)) = w.inv.slots.get(&(e[2], e[0])) {
            super::post::system_line(w, 0x112, [id as i32, 0, 0, 0, 0], String::new());
        }
    }
}

/// s2c 0x11c (FUN_100e5f30): 7031 0x126 for each item id.
pub fn on_lock_error(w: &mut World, ids: &[u16]) {
    for &id in ids {
        super::post::system_line(w, 0x126, [id as i32, 0, 0, 0, 0], String::new());
    }
}

/// s2c 0x11b (LAB_101a8100): 0 clears config 0xbf, 1 sets it.
pub fn on_flag(w: &mut World, v: u8) {
    if v <= 1 {
        super::textcmd::cfg(w, 0xbf, v as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_and_records() {
        let dir = std::env::temp_dir().join(format!("vana-es-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut es = EquipSets::new(dir.clone());
        assert!(es.get(0).is_none());
        es.load_page(0);
        assert_eq!(es.sets[0].raw_name(), b"Set001");
        es.rename(1, b"Melee");
        es.sets[1].slots[0] = [1 | 8 << 2, 3, 0x34, 0x12];
        es.save_page();
        assert_eq!(std::fs::metadata(dir.join("es0.dat")).unwrap().len(), 0x18 + 0x640);
        es.load_page(1);
        assert_eq!(es.sets[0].raw_name(), b"Set021");
        let s = es.get(1).unwrap();
        assert_eq!((s.raw_name(), s.state(0), s.entry(0), s.state(1)), (&b"Melee"[..], 1, (8, 3, 0x1234), 0));
        assert!(es.get(45).is_none());
        es.load_page(0);
        es.rename(1, b"");
        assert_eq!(es.sets[1].raw_name(), b"Set002");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
