//! Wide Scan results and tracking: the session store at +0x2d694 (FUN_100f9360 .. FUN_100f94d0), zeroed with the
//! session on every map server connect, so a zone change loses both.

use std::collections::BTreeMap;

/// s2c 0x0F4 TRACKING_LIST: one result (record +0..+0x18).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub index: u16,
    pub level: u8,
    /// 0 a PC (blue), 1 an NPC (green), 2 a monster (red).
    pub kind: u8,
    /// Offset from the player's world x / z at the scan.
    pub dx: i16,
    pub dz: i16,
    pub name: String,
}

/// The tracking block (+0x3ae98): state, position, ActIndex, level.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Track {
    pub state: u32,
    pub pos: [f32; 3],
    pub index: u16,
    pub level: u8,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub entries: BTreeMap<u16, Entry>,
    /// The last s2c 0x0F6 state (+0x3aeac): 1 list start, 2 list end.
    pub list_state: u32,
    pub track: Track,
    /// The help bar's busy strip (bit 0x80) while a scan is pending.
    pub busy: bool,
}

impl Scan {
    /// FUN_100f9360: the packet to send. While the server's last list is still open, c2s 0x0F6 (tracking end);
    /// otherwise the store is cleared (results, tracking, list state) and c2s 0x0F4 asks for a new list.
    pub fn start(&mut self) -> u16 {
        if self.list_state == 1 {
            self.stop();
            return 0x0F6;
        }
        (self.entries, self.track, self.list_state) = (BTreeMap::new(), Track::default(), 0);
        0x0F4
    }

    /// FUN_100f93c0 (c2s 0x0F5): the block zeroed with the index set.
    pub fn track(&mut self, index: u16) {
        self.track = Track { index, ..Default::default() };
    }

    /// FUN_100f9430 (c2s 0x0F6): the index cleared.
    pub fn stop(&mut self) {
        self.track.index = 0;
    }

    /// FUN_100f94d0: tracking once the server confirmed (state 1 with a level).
    pub fn tracking(&self) -> Option<&Track> {
        (self.track.index != 0 && self.track.level != 0 && self.track.state == 1).then_some(&self.track)
    }

    /// LAB_100f9510: indexes 0x900 and up are dropped.
    pub fn on_entry(&mut self, e: Entry) {
        if e.index < 0x900 {
            self.entries.insert(e.index, e);
        }
    }

    /// LAB_100f9560: state 1 for the tracked index updates it; any other state for it, or index 0, clears the block.
    pub fn on_pos(&mut self, index: u16, level: u8, pos: [f32; 3], state: u32) {
        if index != 0 && index == self.track.index && state == 1 {
            (self.track.level, self.track.state, self.track.pos) = (level, state, pos);
        } else if index == 0 || index == self.track.index {
            self.track = Track { state, ..Default::default() };
        }
    }

    /// LAB_100f95e0: true at list end (state 2).
    pub fn on_state(&mut self, s: u32) -> bool {
        self.list_state = s;
        s == 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_states() {
        let mut s = Scan::default();
        assert_eq!(s.start(), 0x0F4);
        s.on_state(1);
        // A list still open: the next start ends tracking instead.
        assert_eq!(s.start(), 0x0F6);
        s.on_state(2);
        s.track(0x45);
        assert!(s.tracking().is_none());
        s.on_pos(0x45, 1, [1.0, 2.0, 3.0], 1);
        assert_eq!(s.tracking().map(|t| t.pos), Some([1.0, 2.0, 3.0]));
        s.on_pos(0x46, 1, [9.0; 3], 1);
        assert!(s.tracking().is_some());
        s.on_pos(0x45, 1, [0.0; 3], 2);
        assert!(s.tracking().is_none());
    }
}
