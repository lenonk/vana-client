//! Inventory and equipment menu models: the rows, slots and strings the windows draw, built the way
//! FFXiMain's menu classes build them (FUN_101e9170 list rows, FUN_101e8700 name colours,
//! FUN_1012ea90 equipment slots, FUN_10141c10 status panel, FUN_1013b8b0 gil). Drawing is in
//! render/invwin.rs and only reads these.

/// Name tints (ARGB, PS2 scale 0x80 = 1.0) chosen by FUN_101e8700.
pub const TINT_WHITE: u32 = 0x8080_8080;
/// Player +0x132 bit 0 set: the whole list is dimmed.
pub const TINT_DIM: u32 = 0x8040_4040;
/// Equipped (or the equipped linkshell / linkshell 2).
pub const TINT_EQUIPPED: u32 = 0x8050_9050;
/// Equipped weapon/armour that also passes FUN_100fbc70 (item id + extdata; meaning unknown).
pub const TINT_EQUIPPED_EX: u32 = 0x8040_7090;
/// Non-equipment with item flag 0x400, or item type 8 (crystal), 0x16 or 0x17.
pub const TINT_FLAGGED: u32 = 0x8090_9040;

/// Rows the inventory list shows (FUN_101ed820(10)) and their pitch (+0x26).
pub const VISIBLE_ROWS: usize = 10;
pub const ROW_PITCH: f32 = 18.0;

/// What the menus need from the item DATs (record fields as FUN_101557b0 maps them).
pub trait ItemInfo {
    fn name(&self, id: u16) -> Option<String>;
    /// Stack size (+6 of the record); above 1 the list and slots print a count.
    fn stack(&self, id: u16) -> u16;
    /// Item type (+8): 4 weapon, 5 armour, 8 crystal, ...
    fn kind(&self, id: u16) -> u16;
    /// Item flags (+4).
    fn flags(&self, id: u16) -> u16;
    /// 32x32 RGBA icon.
    fn icon(&self, id: u16) -> Option<Vec<u8>>;
}

/// One container slot (FUN_10212ef0): container index, item id, count.
#[derive(Clone, Debug)]
pub struct Slot {
    pub index: u8,
    pub id: u16,
    pub count: u32,
    pub equipped: bool,
    /// Passes FUN_100fbc70 as well (blue instead of green).
    pub equipped_ex: bool,
}

/// One list row: the three columns of FUN_101e9170 (icon, name, count).
#[derive(Clone, Debug)]
pub struct Row {
    pub id: u16,
    pub name: String,
    /// Shown only for stackable items (stack > 1).
    pub count: Option<String>,
    pub tint: u32,
}

/// FUN_101e8700 for the main inventory source.
pub fn name_tint(slot: &Slot, info: &dyn ItemInfo, dimmed: bool) -> u32 {
    let kind = info.kind(slot.id);
    let equipment = kind == 4 || kind == 5;
    if dimmed {
        TINT_DIM
    } else if equipment && slot.equipped && slot.equipped_ex {
        TINT_EQUIPPED_EX
    } else if slot.equipped {
        TINT_EQUIPPED
    } else if (!equipment && info.flags(slot.id) & 0x400 != 0) || matches!(kind, 8 | 0x16 | 0x17) {
        TINT_FLAGGED
    } else {
        TINT_WHITE
    }
}

/// The list rows: every occupied slot except gil (index 0), in container order.
pub fn build_rows(slots: &[Slot], info: &dyn ItemInfo, dimmed: bool) -> Vec<Row> {
    slots
        .iter()
        .filter(|s| s.index != 0 && s.id != 0)
        .map(|s| Row {
            id: s.id,
            name: info.name(s.id).unwrap_or_else(|| format!("item {}", s.id)),
            count: (info.stack(s.id) > 1).then(|| s.count.to_string()),
            tint: name_tint(s, info, dimmed),
        })
        .collect()
}

/// The list window state (FUN_101ed7d0): first visible row and the cursor's row on screen.
#[derive(Clone, Debug, Default)]
pub struct ItemList {
    pub rows: Vec<Row>,
    pub scroll: usize,
    pub cursor: usize,
}

impl ItemList {
    pub fn visible(&self) -> &[Row] {
        let end = (self.scroll + VISIBLE_ROWS).min(self.rows.len());
        &self.rows[self.scroll.min(end)..end]
    }
}

/// Equipment slot -> menu child id (table 0x103639d4, FUN_1012d430): main, sub, range, ammo, head,
/// body, hands, legs, feet, neck, waist, ear1, ear2, ring1, ring2, back.
pub const SLOT_CHILD: [u16; 16] = [1, 5, 9, 13, 2, 3, 7, 12, 16, 6, 8, 10, 14, 11, 15, 4];

/// The equip window's number fields (table 0x103639f8, FUN_1012d920): x, y, w, h from the window
/// origin. 0..7 are the eight element values (left aligned), 8 attack and 9 defence (right aligned).
pub const EQUIP_FIELDS: [[i16; 4]; 10] = [
    [16, 162, 24, 14], [16, 175, 24, 14], [61, 162, 24, 14], [61, 175, 24, 14],
    [105, 162, 24, 14], [105, 175, 24, 14], [149, 162, 24, 14], [149, 175, 24, 14],
    [48, 148, 26, 14], [144, 148, 26, 14],
];

/// FUN_1012d1f0: clamped to -99..9999 (+0x18 / +0x1a), at most 4 characters.
pub fn field_text(v: i32) -> String {
    v.clamp(-99, 9999).to_string()
}

#[derive(Clone, Debug, Default)]
pub struct Equipment {
    /// By equipment slot (0 main .. 15 back).
    pub slots: [Option<Slot>; 16],
    /// Element values in field order, then attack and defence.
    pub fields: [i32; 10],
}

impl Equipment {
    /// (child id, item id, count overlay) per equipped slot (FUN_1012ea90).
    pub fn icons(&self, info: &dyn ItemInfo) -> Vec<(u16, u16, Option<String>)> {
        let mut out = Vec::new();
        for (slot, s) in self.slots.iter().enumerate() {
            let Some(s) = s else { continue };
            // FUN_10154cb0: stackables print min(count, 99) as "%2d".
            let count = (info.stack(s.id) > 1 && s.count > 0).then(|| format!("{:2}", s.count.min(99)));
            out.push((SLOT_CHILD[slot], s.id, count));
        }
        out
    }
}

/// FUN_101bfbb0 with the "  G" suffix: thousands separated by group 12 #0x5b (',' in English).
pub fn gil_text(gil: i64) -> String {
    let g = gil.clamp(0, 999_999_999);
    let s = match g {
        0..1000 => format!("{g}"),
        1000..1_000_000 => format!("{},{:03}", g / 1000, g % 1000),
        _ => format!("{},{:03},{:03}", g / 1_000_000, g / 1000 % 1000, g % 1000),
    };
    format!("{s}  G")
}

/// The status panel ("menu persona", FUN_10141c10) as strings and tints.
#[derive(Clone, Debug, Default)]
pub struct Persona {
    pub name: String,
    /// Group 10 #0x59 "Lv%2d " with the level, then the job's name sprite (windowps 212 + job).
    pub main: Option<(String, u8)>,
    pub sub: Option<(String, u8)>,
    /// The main job in blue (s2c 0x063 type 2 bit 14: EXP capped or merit mode).
    pub main_blue: bool,
    /// s2c 0x01B @0x60 (FUN_101470a0): bits 0x10..0x16 STR..CHR, 0x17 max HP, 0x18 max MP drawn red with a marker.
    pub enc: u32,
    /// Stats ready (the first s2c 0x119): before it only the name.
    pub ready: bool,
    /// "%3d/" item level or "---/", and "Su%2d".
    pub ilvl: String,
    pub su: String,
    /// "%4d/" current and "%d" max.
    pub hp: (String, String),
    pub mp: (String, String),
    /// "%4d".
    pub tp: String,
    /// STR..CHR: "%4d" base and " +%d" (green) / " -%d" (red) bonus.
    pub stats: [(String, Option<(String, u32)>); 7],
}

pub struct Stats {
    pub name: String,
    /// (level, job).
    pub main: Option<(u8, u8)>,
    pub sub: Option<(u8, u8)>,
    pub main_blue: bool,
    pub enc: u32,
    pub ready: bool,
    pub ilvl: Option<u16>,
    pub su: u8,
    pub hp: (u32, u32),
    pub mp: (u32, u32),
    pub tp: u32,
    pub base: [i32; 7],
    pub bonus: [i32; 7],
}

impl Persona {
    pub fn build(s: &Stats) -> Persona {
        let job = |j: &Option<(u8, u8)>| j.map(|(lv, job)| (format!("Lv{lv:2} "), job));
        let stats = std::array::from_fn(|i| {
            let b = s.bonus[i];
            let bonus = match b {
                0 => None,
                _ if b > 0 => Some((format!(" +{b}"), 0x8040_8040)),
                _ => Some((format!(" {b}"), 0x8080_4040)),
            };
            (format!("{:4}", s.base[i]), bonus)
        });
        Persona {
            name: s.name.clone(),
            main: job(&s.main),
            sub: job(&s.sub),
            main_blue: s.main_blue,
            enc: s.enc,
            ready: s.ready,
            ilvl: s.ilvl.map_or("---/".into(), |v| format!("{v:3}/")),
            su: format!("Su{:2}", s.su),
            hp: (format!("{:4}/", s.hp.0), s.hp.1.to_string()),
            mp: (format!("{:4}/", s.mp.0), s.mp.1.to_string()),
            tp: format!("{:4}", s.tp),
            stats,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl ItemInfo for Fake {
        fn name(&self, id: u16) -> Option<String> {
            Some(format!("i{id}"))
        }
        fn stack(&self, id: u16) -> u16 {
            if id < 100 { 12 } else { 1 }
        }
        fn kind(&self, id: u16) -> u16 {
            if id == 1 { 8 } else if id >= 100 { 4 } else { 1 }
        }
        fn flags(&self, _: u16) -> u16 {
            0
        }
        fn icon(&self, _: u16) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn rows_and_tints() {
        let s = |index, id, count, equipped| Slot { index, id, count, equipped, equipped_ex: false };
        let rows = build_rows(&[s(0, 0xffff, 500, false), s(1, 1, 12, false), s(2, 2, 3, false), s(3, 100, 1, true)], &Fake, false);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].tint, rows[0].count.as_deref()), (TINT_FLAGGED, Some("12")));
        assert_eq!((rows[1].tint, rows[1].count.as_deref()), (TINT_WHITE, Some("3")));
        assert_eq!((rows[2].tint, rows[2].count.as_deref()), (TINT_EQUIPPED, None));
    }

    #[test]
    fn gil_and_fields() {
        assert_eq!(gil_text(0), "0  G");
        assert_eq!(gil_text(1_234_567), "1,234,567  G");
        assert_eq!(gil_text(45_006), "45,006  G");
        assert_eq!(field_text(-150), "-99");
        assert_eq!(field_text(12345), "9999");
    }
}
