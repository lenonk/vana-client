//! The Mog House (0x00A LoginState 1): the client-made door "Door: Back to Town" (FUN_101d2cc0 / FUN_101d2ff0) and
//! its exit menu "mogdoor" (LAB_101d6040, FUN_101d61c0 / FUN_101d62b0 / FUN_101d6540), which ask LSB to leave with
//! c2s 0x05E RectID 'zmrq' (FUN_1008aec0).

use super::net::MyRoom;

/// The door actor's server id and ActIndex (outside the 0..0x8ff table); never sent to the server.
pub const DOOR_ID: u32 = 0x0100_0000;
pub const DOOR_INDEX: u16 = 0x900;
pub const DOOR_NAME: &str = "Door: Back to Town";
/// The RID record the door stands at (0x3032375f).
pub const DOOR_RECT: &str = "_720";
pub const WINDOW: &str = "menu    mogdoor ";
/// RectID of the exit request.
pub const ZMRQ: u32 = u32::from_le_bytes(*b"zmrq");
/// Mog Garden in the area lists.
const GARDEN: u16 = 0x118;

/// Exit areas per MyRoomExitBit (DAT_1037ec30); row n (1-based) is MyRoomExitMode n.
pub fn areas(bit: u8) -> &'static [u16] {
    match bit {
        1 => &[230, 231, 232, GARDEN],
        2 => &[234, 235, 236, GARDEN],
        3 => &[238, 239, 240, 241, GARDEN],
        4 => &[243, 244, 245, 246, GARDEN],
        5 => &[48, 50, GARDEN],
        6 => &[80],
        7 => &[87],
        8 => &[94],
        9 => &[256, 257, GARDEN],
        _ => &[],
    }
}

/// FUN_10097bc0: on 2F the sent MyRoomExitBit; on 1F it only when it is the house's own.
pub fn exit_bit(r: &MyRoom) -> u8 {
    if r.floor != 0 {
        return r.exit_bit;
    }
    let own = match r.map {
        0x101 | 0x121 => 1,
        0x102 | 0x122 => 2,
        0x120 | 0x123 => 3,
        0x100 => 4,
        214 => 5,
        0x2e9 => 6,
        199 => 7,
        219 => 8,
        0x124 => 9,
        _ => 0,
    };
    if r.exit_bit == own { own } else { 0 }
}

/// FUN_100f9650: a house with floors.
pub fn has_floors(r: &MyRoom) -> bool {
    r.floor != 0 || (0x121..=0x123).contains(&r.map)
}

/// The open "mogdoor" menu (DAT_106631dc).
pub struct Menu {
    pub bit: u8,
    pub sel: usize,
    /// +0x18: the area list is shown instead of the four rows.
    pub list: bool,
    /// +0x1a: "Change floors." can be chosen (owner, floors, 2F unlocked: s2c 0x067 @0x27 bit 0, not parsed yet).
    pub floors: bool,
    /// +0x19: Mog Garden can be chosen (owner and key item 0x86d; the other flag is not decoded).
    pub garden: bool,
    pub floor: u8,
}

/// What a choice does.
pub enum Pick {
    Close,
    Exit(u8),
    /// A refused row: the beep, or group 9 0xbe for Mog Garden.
    Refused(Option<usize>),
    None,
}

/// The four rows by child (1..4): Stay, Area you entered from, Change floors, Select an area.
pub const ROWS: [(u16, usize); 4] = [(1, 0xb8), (2, 0x46), (3, 0x125), (4, 0xb9)];

impl Menu {
    pub fn rows(&self) -> usize {
        if self.list { areas(self.bit).len() } else { ROWS.len() }
    }

    /// FUN_101d6540 on Enter.
    pub fn pick(&mut self) -> Pick {
        if self.list {
            return match areas(self.bit).get(self.sel) {
                Some(&GARDEN) if self.garden => Pick::Exit(0x7f),
                Some(&GARDEN) => Pick::Refused(Some(0xbe)),
                Some(_) => Pick::Exit(self.sel as u8 + 1),
                None => Pick::None,
            };
        }
        match self.sel {
            0 => Pick::Close,
            1 => Pick::Exit(0),
            2 if self.floors => Pick::Exit(if self.floor == 0 { 0x7d } else { 0x7e }),
            2 => Pick::Refused(None),
            _ => {
                self.list = true;
                self.sel = 0;
                Pick::None
            }
        }
    }

    /// Escape: the list goes back to the rows, the rows close the menu.
    pub fn back(&mut self) -> bool {
        if self.list {
            self.list = false;
            self.sel = 3;
            false
        } else {
            true
        }
    }
}

/// FUN_10097b40 / FUN_10097b50: the zone is one of the exit-list cities (table 0x1035b680, four zones per exit bit).
pub fn in_city(zone: Option<u16>) -> bool {
    const CITIES: [u16; 40] = [
        230, 231, 232, 232, 234, 235, 236, 236, 238, 239, 240, 241, 243, 244, 245, 246, 48, 50, 50, 50, 80, 80, 80, 80, 87, 87, 87, 87, 94, 94, 94, 94, 256, 257, 257, 257, 280, 280, 280,
        280,
    ];
    zone.is_some_and(|z| CITIES.contains(&z))
}

/// FUN_101cc0a0 (the house is our own: s2c 0x067 @0x20 owner id 0) *(inf: 0x067 is not parsed yet, so always ours)*.
pub const OWNER: bool = true;
