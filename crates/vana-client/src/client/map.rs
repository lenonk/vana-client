//! The map window from the main menu (FUN_101ee8f0 -> FUN_101f0af0 mode 0): mapframe (the map, base draw
//! FUN_101e6ca0, draw FUN_101f1a40 / FUN_101f1a60) and map0 (Markers / Wide Scan / Change Map).

use super::World;
use super::game_ui::GameUi;
use crate::dat::DatIndex;
use crate::dat::tpoints::{self, Point};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

// Region table (dat::tables): 14-byte records ended by a record with scale (+5) 0.
const REGIONS_MAX: usize = 765;
/// The "no map" image m_000_00 (FUN_101ef690's fallback).
const NO_MAP: u32 = 0x1592;
/// keytops3 sprites: the player marker (table 0x10375f6c[2]).
const MARKER: usize = 159;
/// The map's marker sprites (keytops3 ids, list 0x1038214c), indexed by a point kind's icon.
const ICON_SPRITE: [usize; 20] =
    [0, 151, 159, 150, 152, 154, 155, 0, 363, 326, 327, 105, 516, 517, 522, 523, 524, 525, 526, 522];
/// Mode 6's margins: the view may pass the image by +0x7c + +0x80 map pixels, and scroll targets move by +0x7c.
const PREVIEW_MARGIN: f32 = 126.0;
const PREVIEW_PAD: f32 = 30.0;
/// Mode 6's backdrop (FUN_101cc620(0xd0c2), file 53442): a dark band around the image.
pub const BACKDROP: &str = "menumap m_x36_00";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub zone: i16,
    /// Sub-map key (FUN_1017a2d0: the collision map id under the player; not decoded, key 0 is used).
    pub key: i8,
    /// The map number (+3): which records Change Map lists (FUN_101fc970).
    pub num: u8,
    pub flags: u8,
    pub scale: i8,
    pub ki: i8,
    pub file: i16,
    /// Map-pixel origin of the 512 x 512 image.
    pub ox: i16,
    pub oy: i16,
}

/// What a region needs to show its map (FUN_101ee560).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Need {
    /// 0x180: no map at all (only the frame opens, no marker or map0).
    Nothing,
    /// 0x17f: always shown.
    Always,
    /// A key item id, tested as bit (id & 31) of word id >> 5.
    Item(u16),
}

pub fn regions(ffximain: &[u8]) -> Vec<Region> {
    let Some(at) =
        crate::dat::tables::of(ffximain).map_err(|e| log::warn!("map regions: {e:#}")).ok().map(|t| t.regions)
    else {
        return Vec::new();
    };
    let Some(b) = crate::dat::battle::va_bytes(ffximain, at, REGIONS_MAX * 14) else { return Vec::new() };
    let s16 = |r: &[u8], o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
    b.chunks(14)
        .map(|r| Region {
            zone: s16(r, 0),
            key: r[2] as i8,
            num: r[3],
            flags: r[4],
            scale: r[5] as i8,
            ki: r[6] as i8,
            file: s16(r, 8),
            ox: s16(r, 10),
            oy: s16(r, 12),
        })
        .take_while(|r| r.scale != 0)
        .collect()
}

/// Change Map's region groups (FUN_1020a400: 50, FUN_1020a3e0 zone lists ended by 0, FUN_1020a3f0 group 0x13 name
/// ids). The code is packed, so the data is found: the first two lists (San d'Oria 230..233, Bastok 234..237), the
/// pointer table naming the first, and the 50 name bytes right after it.
pub fn region_groups(ffximain: &[u8]) -> Vec<(u8, Vec<u16>)> {
    const HINT: [u8; 20] = [0xe6, 0, 0xe7, 0, 0xe8, 0, 0xe9, 0, 0, 0, 0, 0, 0xea, 0, 0xeb, 0, 0xec, 0, 0xed, 0];
    const GROUPS: usize = 50;
    let found = ffximain.windows(HINT.len()).position(|w| w == HINT).and_then(|at| {
        let va = crate::dat::tables::va_of(ffximain, at)?;
        let ptrs = ffximain.windows(4).position(|w| w == va.to_le_bytes())?;
        Some(ptrs)
    });
    let Some(ptrs) = found else {
        log::warn!("map: no region group table");
        return Vec::new();
    };
    let u32_at = |o: usize| ffximain.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let va = |v: u32, len: usize| crate::dat::battle::va_bytes(ffximain, v, len);
    (0..GROUPS)
        .filter_map(|i| {
            let p = u32_at(ptrs + 4 * i)?;
            let name = *ffximain.get(ptrs + 4 * GROUPS + i)?;
            let mut zones = Vec::new();
            while let Some(z) =
                va(p + 2 * zones.len() as u32, 2).map(|b| u16::from_le_bytes([b[0], b[1]])).filter(|&z| z != 0)
            {
                zones.push(z);
            }
            Some((name, zones))
        })
        .collect()
}

impl Region {
    /// The first record of the zone with key 0 (FUN_101ee510, and FUN_101f0af0's key-0 fallback).
    pub fn find(regions: &[Region], zone: u16) -> Option<Region> {
        regions.iter().find(|r| r.zone == zone as i16 && r.key == 0).copied()
    }

    /// FUN_101f7430(zone, key, 0): the record for exactly this zone and sub-key.
    pub fn find_key(regions: &[Region], zone: u16, key: u8) -> Option<Region> {
        regions.iter().find(|r| r.zone == zone as i16 && r.key == key as i8).copied()
    }

    /// The record for where `w` stands (the sub-key under the player), else the zone's key-0 map.
    pub(super) fn here(regions: &[Region], w: &World) -> Option<Region> {
        let key = sub_key(w, -1, true);
        Self::find_key(regions, w.pos_zone, key).or_else(|| Self::find(regions, w.pos_zone))
    }

    /// FUN_101ef690: the map DAT by the low nibble of the flags.
    pub fn file_id(&self) -> u32 {
        let i = self.file as i32;
        match self.flags & 0xf {
            0 => (i + 0x14c0) as u32,
            1 => (i + 0xd02f) as u32,
            2 => (i + 0xd417) as u32,
            _ => NO_MAP,
        }
    }

    pub fn need(&self) -> Need {
        match self.ki {
            k if k < 0 => Need::Always,
            0 => Need::Nothing,
            k => Need::Item(
                k as u16
                    + match self.flags & 0xf0 {
                        0x10 => 0x73f,
                        0x20 => 0x8fd,
                        _ => 0x180,
                    },
            ),
        }
    }

    /// FUN_101ee650: world (x, z) to raw map pixels, k = 2560 / |scale|; C truncation.
    pub fn px(&self, x: f32, z: f32) -> (i32, i32) {
        px_of(self.scale, x, z)
    }

    /// FUN_101f1a60: "(%c-%d)" from 32-pixel cells 16 in from the origin.
    pub fn grid(&self, px: i32, py: i32) -> String {
        let col = (b'A' as i32 + (px - self.ox as i32 - 16) / 32) as u8 as char;
        format!("({col}-{})", 1 + (py - self.oy as i32 - 16) / 32)
    }
}

type Loaded = Option<(String, u32, u32, Vec<u8>)>;

/// The overlay's image with its shade.
type ShadedLoaded = Option<(String, u32, u32, Vec<u8>, Shade)>;

/// Mean luminance of a map image in 8 x 8 pixel blocks, so the overlay's marker can take a colour that shows on it.
struct Shade {
    cols: usize,
    rows: usize,
    lum: Vec<f32>,
}

impl Shade {
    const BLOCK: usize = 8;

    fn new(w: u32, h: u32, rgba: &[u8]) -> Self {
        let (w, h) = (w as usize, h as usize);
        let (cols, rows) = (w.div_ceil(Self::BLOCK).max(1), h.div_ceil(Self::BLOCK).max(1));
        let mut sum = vec![(0.0f32, 0u32); cols * rows];
        for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate().take(w * h) {
            let b = &mut sum[(i / w / Self::BLOCK) * cols + i % w / Self::BLOCK];
            b.0 += (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
            b.1 += 1;
        }
        Self { cols, rows, lum: sum.into_iter().map(|(s, n)| if n == 0 { 0.0 } else { s / n as f32 }).collect() }
    }

    /// The mean over the 3 x 3 blocks around image pixel (x, y), 0..1.
    fn at(&self, x: f32, y: f32) -> f32 {
        let (bx, by) = ((x / Self::BLOCK as f32).floor() as i64, (y / Self::BLOCK as f32).floor() as i64);
        let (mut s, mut n) = (0.0, 0);
        for cy in by - 1..=by + 1 {
            for cx in bx - 1..=bx + 1 {
                if (0..self.cols as i64).contains(&cx) && (0..self.rows as i64).contains(&cy) {
                    s += self.lum[cy as usize * self.cols + cx as usize];
                    n += 1;
                }
            }
        }
        if n == 0 { 0.0 } else { s / n as f32 }
    }
}

/// FUN_101f9a50's modes: 0 the main menu or /map, 1 an event map, 2 an event illustration (zone <= -100), 3 a region
/// map (zone -1..-99), 5 Change Map's other zone, 6 an op 0xd4 sub 0 menu's preview, 7 an Adventuring Primer page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Menu,
    Event,
    Illustration,
    RegionMap,
    Other,
    Preview,
    Primer,
}

impl Mode {
    /// FUN_101f78e0: an event's mode from the zone's sign.
    pub fn of_event_zone(zone: i16) -> Mode {
        match zone {
            ..=-100 => Mode::Illustration,
            -99..0 => Mode::RegionMap,
            _ => Mode::Event,
        }
    }
}

/// Op 0xb4 sub 0x14's view (+0xbc..+0xe8, FUN_101fb070 / FUN_101edee0): x, y in map pixels and a zoom factor of the
/// base zoom, eased from `start` by `t`.
#[derive(Clone, Copy, Debug)]
struct Scripted {
    cur: [f32; 3],
    start: [f32; 3],
    target: [f32; 3],
    speed: f32,
    mode: u8,
    t: f32,
}

/// mapv3 (the marker list) and what hangs off it.
#[derive(Clone, Debug, Default)]
struct List {
    /// Cursor child: rows 1..10.
    cursor: u16,
    /// mapv2 on a row of set 0, with its cursor 1..3.
    v2: Option<(usize, u16)>,
    /// Placing a marker in this slot: cursor mode (flag 0x100, sub-mode 1) at a screen point.
    placing: Option<(usize, (f32, f32))>,
    /// The name entry for a slot (FUN_101fd3a0, 15 characters).
    rename: Option<(usize, String)>,
}

/// The placing cursor's sprite (keytops3, the window's sprite list entry 1 at +0x118).
const PLACE_CURSOR: usize = 151;
/// mapv3's name column (FUN_101cb320's width limit).
const NAME_FIT: f32 = 68.0;

/// A party or alliance member's marker (slots 20..39): name, map pixel, of our party (sprite kind 5, else 6).
#[derive(Clone, Debug, PartialEq)]
struct MemberMark {
    name: String,
    px: (f32, f32),
    party: bool,
}

/// mapv2's place from mapv3's (FUN_101fd930): the row's left edge (16) less mapv2's width (112), the first row's top (6).
const V2_AT: (f32, f32) = (16.0 - 112.0, 6.0);

/// Marker sprites of the four sets (list 0x1038214c indices 3..6): yours, event, party, alliance.
const SET_SPRITE: [usize; 4] = [150, 152, 154, 155];
/// mapv3's row icons (table 0x10382178).
const ROW_ICON: [usize; 8] = [151, 150, 153, 152, 154, 154, 155, 155];

/// A teleport point's marker (FUN_101f99a0): map pixel, sprite, label, blinking (flag 0x1000: not registered), alpha
/// and its direction.
#[derive(Clone, Debug)]
struct PointMark {
    px: (f32, f32),
    sprite: usize,
    name: &'static str,
    blink: bool,
    alpha: f32,
    falling: bool,
}

/// The point buffers (+0x6b4 / +0x6bc, FUN_101f92e0): the table once the masks are in, the markers, the last
/// followed (kind, id) (+0x6c0 / +0x6c2).
#[derive(Clone, Debug)]
struct Points {
    table: Option<Arc<Vec<Point>>>,
    masks: [u8; 64],
    marks: Vec<PointMark>,
    last: (u16, u16),
}

impl Default for Points {
    fn default() -> Self {
        Points { table: None, masks: [0; 64], marks: Vec::new(), last: (0xffff, 0xffff) }
    }
}

/// What following a menu option's point asks of the caller (FUN_101f9530).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Follow {
    Keep,
    /// The point is on another map: this one closes and the preview opens there.
    Reopen,
    /// "You do not possess a map of %s." (group 10 #7) for the zone.
    NotOwned(u16),
    /// Group 7 0x62: no map record.
    NoMap,
}

pub struct MapWin {
    pub region: Region,
    /// The sub-key asked for (+0x102; the record may be the zone's key 0), and no record at all (+0xa47).
    key: u8,
    no_record: bool,
    /// The op's raw key FUN_100b2570 keeps (DAT_104897f6) to test for the same map; -1 when none was given.
    op_key: i16,
    /// The placing cursor sprite's width, read as drawn; FUN_101fa1a0 halves it to find its centre.
    cursor_w: f32,
    need: Need,
    owned: bool,
    /// Teleport points (modes 1 / 3 / 6, 0 / 5 with the map).
    points: Option<Points>,
    /// helpwind slot 2 (+0xb4): the map's place name; mode 6 only while shown.
    help: bool,
    /// The zone "You do not possess a map" was said for (+0x108).
    told: Option<u16>,
    /// Mode 6: the margins around the image (bounds) and the scroll targets' offset.
    margin: f32,
    pad: f32,
    /// The .MRK sets 0 / 1, where they live, and whether to save them on close (+0xa44).
    marks: super::mapmark::MarkFile,
    mark_dir: Option<std::path::PathBuf>,
    char_id: u32,
    dirty: bool,
    /// Sets 2 / 3 (slots 20..39, FUN_101fa720): our party's members from slot 20, the alliance's others from
    /// `rest_from`; how many of each were placed (+0x10e / +0x10f) and whether one landed in set 3 (+0xa48).
    members: [Option<MemberMark>; 20],
    rest_from: usize,
    party_count: u8,
    rest_count: u8,
    set3: bool,
    /// The map is of the zone we are in (party markers and c2s 0x0D2 only then).
    here: bool,
    /// Marker alpha per set and slot (FUN_101f0560), 0..0x80.
    mark_alpha: [[f32; 10]; 4],
    /// Sets 2 / 3: the one member of each whose label shows (flag 8 cleared by FUN_101fa6d0; it stays over set changes).
    label_only: [Option<usize>; 2],
    list: Option<List>,
    /// Change Map's list (maplist) while open.
    pub maplist: Option<MapList>,
    /// Change Map's pick (+0x110 / +0x112): reopened in mode 5 once this map has closed.
    pub reopen: Option<(i16, i16)>,
    /// The marker set shown (+0x106).
    set: u8,
    /// Interface sounds for the caller to play.
    pub sounds: Vec<u32>,
    /// Party positions (c2s 0x0D2) asked for this map (+0xa45); preset for a map of another zone (FUN_101fa620(0)).
    party_asked: bool,
    /// Requests for the caller: packets and prompts.
    pub outs: Vec<Out>,
    /// Wide Scan: map0 item 2 enabled (FUN_101f7c00: s2c 0x119 seen, our zone's map), its list, results kept for
    /// this map session (scanlist +0x66), the scan origin (world x / z at the scan), tracking at open and the tracked
    /// ring's pixel (record +0x694, taken at open).
    scan_ok: bool,
    pub scan: Option<ScanList>,
    scan_kept: bool,
    origin: (i16, i16),
    player_xz: (f32, f32),
    tracking: bool,
    tracked: Option<(f32, f32)>,
    pub mode: Mode,
    /// map0's cursor child (1..3).
    pub cursor: u16,
    /// The main menu window open before (the close callback LAB_101faff0 reopens it).
    pub return_menu: Option<usize>,
    /// Flags 1 | 2: the scroll and zoom keys work (not for illustrations).
    keys: bool,
    show_player: bool,
    /// s2c 0x072's overlay block while valid, and titlewin open: set by the caller each frame for FUN_101fc130.
    pub battlefield: Option<super::battlefield::Overlay>,
    pub title_up: bool,
    /// fit = W / 512; zoom within [fit, 4 fit]; base zoom (+0x68); zoom-to target (flag 0x10).
    fit: f32,
    zoom: f32,
    zoom_base: f32,
    zoom_to: Option<f32>,
    /// View origin in map pixels (+0x24 / +0x28), the image base (+0x54: the origin when owned) and the marker shift
    /// (+0x6c: the origin when the no-map image is shown).
    view: (f32, f32),
    base: (f32, f32),
    shift: (f32, f32),
    /// The cursor point in screen pixels (W * 200 / 512, H / 2): what zooming keeps and scroll-to centres.
    point: (f32, f32),
    /// The player's map pixel and heading, taken at open (+0x674; the arrow does not follow).
    home: (f32, f32),
    heading: f32,
    /// Flag 8: easing the point under the cursor to this map pixel.
    scroll_to: Option<(f32, f32)>,
    /// +0xa6 level (0 shown .. 0xff gone), +0xa8 target, +0xa4 step per retail frame.
    fade: (f32, f32, f32),
    /// Flag 0x40 with the deadline (+0xb0, 3 s).
    closing: Option<std::time::Instant>,
    scripted: Option<Scripted>,
    screen: (f32, f32),
    texture: Option<String>,
    rx: Option<Receiver<Loaded>>,
    /// Player marker alpha 0..0x80 and its direction (flags 0x9f: fade in, blink at 6 a tick).
    alpha: f32,
    rising: bool,
    /// Real ticks not yet run as 2-tick retail frames (fade, zoom-to, scroll-to).
    acc: f32,
    /// Mode 7 (+0xa4c / +0xa50 / +0xa51): the page's first image file, the image count, the one shown; and what the
    /// close asks next (LAB_101f7b90 the next image, LAB_101f7b40 back to guide01).
    primer: Option<(u32, u8, u8)>,
    pub primer_next: Option<bool>,
}

impl MapWin {
    /// Opens the map of the zone (FUN_101f9a50 mode 0). None when the zone has no record (group 7 0x62).
    pub fn open(w: &World, regions: &[Region], sw: f32, sh: f32) -> Option<Self> {
        let region = Region::here(regions, w)?;
        let owned = owned(w, &region);
        log::info!("map: zone {} owned {owned}", w.pos_zone);
        let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
        let mut m = Self::new(
            region,
            owned,
            w.index.clone(),
            (w.pos.x, w.pos.z),
            facing,
            sw,
            sh,
            Mode::Menu,
            w.game.get(0x8a),
            true,
        );
        // FUN_101f8ff0: +0x102 is the sub-key under the player even when the key-0 record is shown. Set 2 to start
        // (FUN_101f7810 in 30261006_0); attach moves on past empty sets.
        (m.key, m.set) = (sub_key(w, -1, true), 2);
        m.attach(w, regions);
        Some(m)
    }

    /// FUN_101f96c0 (the .MRK, named by the zone and the sub-key asked for) and, in modes 0 / 5 for a map of the
    /// zone we are in, FUN_101f93c0 (party markers from the actors); then FUN_101fc380(0): a set with entries.
    fn attach(&mut self, w: &World, regions: &[Region]) {
        self.mark_dir = marks_dir(w);
        self.char_id = w.my_id;
        if let Some(d) = &self.mark_dir {
            self.marks = super::mapmark::MarkFile::load(d, self.region.zone as u16, self.key, w.my_id);
        }
        self.here = self.region.zone == w.pos_zone as i16;
        self.scan_ok = w.recasts_seen && self.here;
        self.tracking = w.scan.tracking().is_some();
        self.tracked = w.scan.tracking().map(|t| {
            let (x, y) = self.region.px(t.pos[0], t.pos[2]);
            (x as f32, y as f32)
        });
        self.rest_from = rest_from(w);
        let menu = matches!(self.mode, Mode::Menu | Mode::Other) && self.need != Need::Nothing;
        // FUN_101fa620(0): a mode 5 map of another zone never asks for positions.
        self.party_asked = self.mode == Mode::Other && !self.here;
        if menu && self.here {
            // FUN_101f93c0: members after us with an actor on the sub-map under us, each through FUN_101f94d0 as an
            // s2c 0x0A0 answer would be (the same record lookup and pixel).
            let mine = sub_key(w, -1, true);
            let own = w.party.own_flags;
            for m in w.party.members().skip(1) {
                let Some(e) = w.entities.get(&m.id) else { continue };
                if w.ground.as_ref().map_or(0, |g| g.map_key(e.pos)) != mine {
                    continue;
                }
                let (px, py) = reply_px(regions, w.pos_zone, mine, e.pos.x, e.pos.z);
                self.add_member(&m.name, (px as f32, py as f32), (m.flags ^ own) & 3 == 0);
            }
        }
        self.step_set(0);
    }

    /// FUN_101fa720: a member's marker. Our party's take the first free slot from 20, the alliance's others from
    /// `rest_from`; a slot that already holds the name ends it (the first position wins). A member landing in slots
    /// 30..39 makes set 3 showable.
    fn add_member(&mut self, name: &str, px: (f32, f32), party: bool) {
        let mut i = if party { 0 } else { self.rest_from };
        while i < 20 {
            match &self.members[i] {
                Some(m) if m.name == name => return,
                Some(_) => i += 1,
                None => break,
            }
        }
        if i >= 20 {
            return;
        }
        self.members[i] = Some(MemberMark { name: name.into(), px, party });
        if party {
            self.party_count += 1;
        } else {
            self.rest_count += 1;
            self.set3 |= i >= 10;
        }
    }

    /// Marker set `s` can be shown (FUN_101fc380 skips the others): 0 needs the map held (+0xa46, worked out in modes
    /// 0 / 5 only), 2 any member, 3 a member in slots 30..39 (+0xa48); 1 always.
    fn set_shown(&self, s: u8) -> bool {
        match s {
            0 => self.owned && self.need != Need::Nothing && matches!(self.mode, Mode::Menu | Mode::Other),
            2 => self.party_count != 0 || self.rest_count != 0,
            3 => self.set3,
            _ => true,
        }
    }

    /// FUN_101fc380(d): the set by d, wrapping, past sets with nothing to show.
    fn step_set(&mut self, d: i32) {
        let step = if d < 0 { 3 } else { 1 };
        if d != 0 {
            self.set = (self.set + step) % 4;
        }
        for _ in 0..4 {
            if self.set_shown(self.set) {
                break;
            }
            self.set = (self.set + step) % 4;
        }
    }

    /// FUN_101fd680's first test: an event map's mapv3 (+0x10a, FUN_101f8ef0 passes 1) takes no keys and has no
    /// cursor.
    fn locked(&self) -> bool {
        !matches!(self.mode, Mode::Menu | Mode::Other)
    }

    /// Opened through FUN_100b2570 (DAT_104896e6 set): the event modes and the op 0xd4 preview.
    fn event_opened(&self) -> bool {
        matches!(self.mode, Mode::Event | Mode::Illustration | Mode::RegionMap | Mode::Preview)
    }

    /// FUN_100b2570's same-map test for ops 0x89 / 0x8d / 0xc8: the mode, the zone and, when given, the op's key as
    /// the last op gave it.
    fn shows_event(&self, zone: i16, key: i16) -> bool {
        !self.closing()
            && self.mode == Mode::of_event_zone(zone)
            && self.region.zone == zone
            && (key < 0 || self.op_key == key)
    }

    /// The map's zone (for c2s 0x0D2).
    pub fn zone(&self) -> u16 {
        self.region.zone as u16
    }

    /// The player's record is set (+0x98, FUN_101fa4b0).
    pub fn shows_player(&self) -> bool {
        self.show_player
    }

    /// FUN_101fa680: c2s 0x0D2 for the map's zone, once per map (+0xa45).
    fn want_party(&mut self) {
        if !self.party_asked {
            self.party_asked = true;
            self.outs.push(Out::AskParty);
        }
    }

    /// The sub-key the map was asked for (+0x102): what s2c 0x0A0 positions are tested against, and the .MRK's name.
    pub fn asked_key(&self) -> u8 {
        self.key
    }

    /// s2c 0x0A0 (FUN_100f07b0 -> FUN_101f94d0) for a listed member in our zone on the sub-map asked for: placed by
    /// FUN_101fa720 on whatever map is open, so a member the actors already placed stays where they were. `from` is
    /// `rest_from` now.
    pub fn party_pos(&mut self, name: &str, px: (i16, i16), party: bool, from: usize) {
        self.rest_from = from;
        self.add_member(name, (px.0 as f32, px.1 as f32), party);
    }

    /// Set s, slot i: its map pixel, label and sprite, when it holds a marker.
    fn slot(&self, s: usize, i: usize) -> Option<((f32, f32), Option<&str>, usize)> {
        match s {
            0 | 1 => self.marks.sets[s]
                .get(i)
                .filter(|m| m.valid)
                .map(|m| ((m.px.0 as f32, m.px.1 as f32), Some(m.name.as_str()), SET_SPRITE[s])),
            _ => self
                .members
                .get((s - 2) * 10 + i)?
                .as_ref()
                .map(|m| (m.px, Some(m.name.as_str()), SET_SPRITE[if m.party { 2 } else { 3 }])),
        }
    }

    /// Op 0x8b / 0xb8 (FUN_101f8d50 / FUN_101f8e30): set 1's slot at the world (x, z), named; on an open event map
    /// also scrolled to (FUN_101f8de0), not on the file path.
    fn mark(&mut self, slot: usize, x: f32, z: f32, name: &str, scroll: bool) {
        let Some(m) = self.marks.sets[1].get_mut(slot) else { return };
        let (px, py) = self.region.px(x, z);
        *m = super::mapmark::Slot { px: (px as i16, py as i16), name: name.into(), valid: true };
        self.dirty = true;
        if scroll {
            self.scroll_to_px((px as f32, py as f32));
        }
    }

    /// Op 0xa8 / 0xb9 (FUN_101fa3b0, then FUN_101fa440 with an empty name): set 1's slot no longer holds a marker
    /// and loses its name; its pixel stays.
    fn unmark(&mut self, slot: usize) {
        if let Some(m) = self.marks.sets[1].get_mut(slot) {
            (m.valid, m.name) = (false, String::new());
            self.dirty = true;
        }
    }

    /// Offscreen shots: three of your markers around the player, mapv3 open on them at full alpha.
    pub fn shot_markers(&mut self) {
        for (i, (dx, dy, n)) in
            [(-40.0, -20.0, "Gate"), (30.0, 10.0, "Chocobo"), (5.0, 45.0, "Home point")].into_iter().enumerate()
        {
            self.marks.sets[0][i] = super::mapmark::Slot {
                px: ((self.home.0 + dx) as i16, (self.home.1 + dy) as i16),
                name: n.into(),
                valid: true,
            };
            self.mark_alpha[0][i] = 128.0;
        }
        self.set = 0;
        self.list = Some(List { cursor: 2, ..Default::default() });
    }

    /// Offscreen shots: as `shot_markers`, with a removed marker's row (its name stays), a name past the column
    /// (the ellipsis) and mapv2 on the removed row (View disabled, Place).
    pub fn shot_marker_menu(&mut self) {
        self.shot_markers();
        self.marks.sets[0][3] = super::mapmark::Slot { px: (0, 0), name: "Old camp".into(), valid: false };
        self.marks.sets[0][4] = super::mapmark::Slot {
            px: ((self.home.0 - 60.0) as i16, (self.home.1 + 30.0) as i16),
            name: "Wwwwwwwwwwwwwww".into(),
            valid: true,
        };
        self.mark_alpha[0][4] = 128.0;
        self.list = Some(List { cursor: 4, v2: Some((3, 1)), ..Default::default() });
    }

    /// The marker name entry is open: it takes the keys and characters.
    pub fn typing(&self) -> bool {
        self.list.as_ref().is_some_and(|l| l.rename.is_some())
    }

    /// The name entry's keys: up to 15 characters, Backspace. Enter (the input's done callback, message 0x11) and
    /// Escape (message 6) both end in FUN_101fd450(1), which copies the input's text into the slot: Escape keeps
    /// what was typed too.
    pub fn text_events(&mut self, events: &[egui::Event]) {
        let Some(l) = self.list.as_mut() else { return };
        let Some((slot, text)) = l.rename.as_mut() else { return };
        let slot = *slot;
        let mut done = false;
        for ev in events {
            match ev {
                egui::Event::Text(t) | egui::Event::Paste(t) => {
                    for c in t.chars().filter(|c| (' '..='~').contains(c)) {
                        if text.chars().count() < 15 {
                            text.push(c);
                        }
                    }
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                    text.pop();
                }
                egui::Event::Key { key: egui::Key::Enter | egui::Key::Escape, pressed: true, .. } => done = true,
                _ => {}
            }
        }
        if done {
            let text = std::mem::take(text);
            l.rename = None;
            // FUN_101fa440: the name and the save flag, changed or not.
            self.marks.sets[0][slot].name = text;
            self.dirty = true;
        }
    }

    /// An event's map (FUN_100b2570 -> FUN_101f78e0): zone and sub-key (negative: the one under the loaded area's
    /// origin, whatever the zone); the arrow only with the op's flag.
    pub fn open_event(
        w: &World,
        regions: &[Region],
        zone: i16,
        op_key: i16,
        show_player: bool,
        sw: f32,
        sh: f32,
    ) -> Option<Self> {
        let mode = Mode::of_event_zone(zone);
        let key = sub_key(w, op_key, false);
        let region = Region::find_key(regions, zone as u16, key).or_else(|| Region::find(regions, zone as u16))?;
        // FUN_101fb060 modes 1..3 call FUN_101f9970 with its "owned" argument 1: an event's map shows the map image
        // whether or not we hold the area's map (only modes 0 and 5 ask FUN_10097e80 for the key item), so a
        // newcomer shown another nation's city gets its map, not NO MAP.
        let owned = true;
        let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
        log::info!("map: event map zone {zone} key {key} mode {mode:?} owned {owned}");
        let mut m = Self::new(
            region,
            owned,
            w.index.clone(),
            (w.pos.x, w.pos.z),
            facing,
            sw,
            sh,
            mode,
            w.game.get(0x8a),
            show_player,
        );
        (m.key, m.op_key) = (key, op_key);
        m.attach(w, regions);
        // Mode 1 opens mapv3 on set 1 instead of map0 (map records only).
        if mode == Mode::Event && m.need != Need::Nothing {
            m.set = 1;
            m.list = Some(List { cursor: 1, ..Default::default() });
        }
        Some(m)
    }

    /// The preview of an op 0xd4 sub 0 menu (FUN_101f9a50 mode 6): fit zoom, no keys, no sets or map0, hidden until
    /// a point shows it; the cursor point midway between the top bar and the menu (`point_y`); the key -1 is the
    /// sub-map under the player. A zone without a record opens anyway and only says so.
    pub fn open_preview(
        w: &World,
        regions: &[Region],
        zone: i16,
        key: i16,
        arrow: bool,
        sw: f32,
        sh: f32,
        point_y: f32,
    ) -> Self {
        let key = sub_key(w, key, true);
        let found = Region::find_key(regions, zone as u16, key).or_else(|| Region::find(regions, zone as u16));
        let region = found.unwrap_or(Region { zone, key: 0, num: 0, flags: 0, scale: 0, ki: 0, file: 0, ox: 0, oy: 0 });
        // FUN_101f7480: 0x180 leaves +0xa46 clear, so such a map never shows.
        let owned = found.is_some() && region.need() != Need::Nothing && owned(w, &region);
        let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
        log::info!("map: preview zone {zone} key {key} owned {owned} record {}", found.is_some());
        let mut m =
            Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, Mode::Preview, 0, arrow);
        (m.key, m.no_record) = (key, found.is_none());
        m.point.1 = point_y.trunc();
        if m.show_player {
            m.view = (m.home.0 - m.point.0, m.home.1 - m.point.1);
            m.scroll_to_px(m.home);
        }
        m.clamp();
        m.attach(w, regions);
        m
    }

    /// FUN_101f8360 and the callers' zoom: the image, zoom W / 512 x config 0x8a / 1000 in mode 0 (W / 512 for a no-map
    /// record), x 1.75 in mode 1, W / 512 otherwise; fade 0x80 -> 0 at 8 a frame (mode 6: hidden at once); then the
    /// player under the cursor point (FUN_101f8ea0, FUN_101ef990). Modes with points ask for the masks (FUN_101f93d0).
    pub fn new(
        region: Region,
        owned: bool,
        index: Option<Arc<DatIndex>>,
        pos: (f32, f32),
        facing: f32,
        sw: f32,
        sh: f32,
        mode: Mode,
        zoom_cfg: u32,
        show_player: bool,
    ) -> Self {
        let need = region.need();
        let file = if owned { region.file_id() } else { NO_MAP };
        let (base, shift) = if owned {
            ((region.ox as f32, region.oy as f32), (0.0, 0.0))
        } else {
            ((0.0, 0.0), (region.ox as f32, region.oy as f32))
        };
        let fit = sw / 512.0;
        let zoom = match mode {
            _ if need == Need::Nothing => fit,
            Mode::Menu | Mode::Primer => fit * zoom_cfg.clamp(1000, 4000) as f32 * 0.001,
            Mode::Event => fit * 1.75,
            _ => fit,
        };
        let point = ((sw * 200.0 / 512.0).trunc(), (sh / 2.0).trunc());
        let (px, py) = region.px(pos.0, pos.1);
        let home = (px as f32, py as f32);
        let rx = index.map(|index| load(index, file));
        let preview = mode == Mode::Preview;
        let points = match mode {
            Mode::Event | Mode::RegionMap | Mode::Preview => true,
            Mode::Menu | Mode::Other => owned && need != Need::Nothing,
            Mode::Illustration | Mode::Primer => false,
        };
        let mut m = Self {
            region,
            key: region.key as u8,
            no_record: false,
            op_key: -1,
            cursor_w: 0.0,
            need,
            owned,
            points: points.then(Points::default),
            help: !preview && mode != Mode::Primer,
            told: None,
            margin: if preview { PREVIEW_MARGIN } else { 0.0 },
            pad: if preview { PREVIEW_PAD } else { 0.0 },
            marks: Default::default(),
            mark_dir: None,
            char_id: 0,
            dirty: false,
            members: Default::default(),
            rest_from: 5,
            party_count: 0,
            rest_count: 0,
            set3: false,
            here: false,
            mark_alpha: [[0.0; 10]; 4],
            label_only: [None; 2],
            list: None,
            maplist: None,
            reopen: None,
            set: 0,
            sounds: Vec::new(),
            party_asked: false,
            outs: if points { vec![Out::AskPoints] } else { Vec::new() },
            scan_ok: false,
            scan: None,
            scan_kept: false,
            origin: (0, 0),
            player_xz: pos,
            tracking: false,
            tracked: None,
            mode,
            cursor: 1,
            return_menu: None,
            keys: !matches!(mode, Mode::Illustration | Mode::Preview),
            show_player: show_player && need != Need::Nothing,
            battlefield: None,
            title_up: false,
            fit,
            zoom,
            zoom_base: zoom,
            zoom_to: None,
            view: base,
            base,
            shift,
            point,
            home,
            heading: facing,
            scroll_to: None,
            fade: if preview { (255.0, 255.0, 8.0) } else { (128.0, 0.0, 8.0) },
            closing: None,
            scripted: None,
            screen: (sw, sh),
            texture: None,
            rx,
            alpha: 0.0,
            rising: true,
            acc: 0.0,
            primer: None,
            primer_next: None,
        };
        if m.show_player {
            // As decompiled: screen pixels taken from map pixels, then clamped and eased.
            m.view = (home.0 - point.0, home.1 - point.1);
            m.scroll_to_px(home);
        }
        m.clamp();
        m
    }

    /// FUN_101f7ad0 -> FUN_101fb140 -> FUN_101f9a50 mode 7: image `at` of a Primer page's `count` from file `first`;
    /// the base (-256, -256), no points, markers, arrow or place name, the scroll and zoom keys live, the zoom as mode 0's
    /// (config 0x8a), then map pixel (0, -70) eased under the cursor point (FUN_101ef990(0, -70)).
    pub fn primer(
        index: Option<Arc<DatIndex>>,
        first: u32,
        count: u8,
        at: u8,
        sw: f32,
        sh: f32,
        zoom_cfg: u32,
    ) -> Self {
        let region = Region { zone: -100, key: 0, num: 0, flags: 0, scale: 0, ki: -1, file: 0, ox: -256, oy: -256 };
        let mut m = Self::new(region, true, None, (0.0, 0.0), 0.0, sw, sh, Mode::Primer, zoom_cfg, false);
        m.rx = index.map(|i| load(i, first + at as u32));
        m.primer = Some((first, count, at));
        m.scroll_to_px((0.0, -70.0));
        m
    }

    /// The Primer page shown: (first file, count, index).
    pub fn primer_page(&self) -> Option<(u32, u8, u8)> {
        self.primer
    }

    /// Waits for the image, skips the opening animations and shows the marker at full alpha (offscreen shots).
    pub fn wait_image(&mut self, gu: &mut GameUi) {
        (self.alpha, self.rising) = (128.0, false);
        self.fade = (0.0, 0.0, 8.0);
        if let Some(to) = self.scroll_to.take() {
            self.view = (
                to.0 - self.point.0 / self.zoom - 0.5 - self.shift.0,
                to.1 - self.point.1 / self.zoom - 0.5 - self.shift.1,
            );
            self.clamp();
        }
        if let Some(Some((name, iw, ih, rgba))) = self.rx.take().and_then(|r| r.recv().ok()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
        }
    }

    /// map0 is open: modes 0 / 5 with a map (not for a zone with no map, as FUN_101f0af0 decompiles), not while
    /// closing (FUN_101fa420 closes the sub-windows at once).
    pub fn has_menu(&self) -> bool {
        self.need != Need::Nothing
            && matches!(self.mode, Mode::Menu | Mode::Other)
            && self.closing.is_none()
            && self.list.is_none()
            && self.maplist.is_none()
            && self.scan.is_none()
    }

    /// FUN_101fb910 / FUN_101fb7d0: scanlist opens; a new scan unless this map session kept results.
    fn open_scan(&mut self) {
        let mut s = ScanList::default();
        if !self.scan_kept {
            self.origin = (self.player_xz.0 as i16, self.player_xz.1 as i16);
            (s.scanning, s.timer) = (true, 1800.0);
            self.outs.push(Out::Scan);
        }
        self.scan = Some(s);
    }

    /// The list ended (s2c 0x0F6 state 2, LAB_101fb790): not scanning, results kept.
    pub fn scan_done(&mut self) {
        self.scan_kept = true;
        if let Some(s) = self.scan.as_mut() {
            s.scanning = false;
        }
    }

    /// LAB_101f7d40 after the stop prompt's Yes: the ring goes, scanlist opens.
    pub fn after_stop(&mut self) {
        (self.tracked, self.tracking) = (None, false);
        self.open_scan();
    }

    /// FUN_101fb4b0: the rows and their markers from the results (rebuilt every frame while the list shows; ours only
    /// when what they come from changed): PCs (not ourselves) with their name, NPCs and monsters named from the
    /// zone NPC list by index & 0x3ff; those without a name or outside the narrowed circle are left out.
    pub(super) fn rebuild_scan(&mut self, w: &World) {
        let Some(s) = self.scan.as_mut().filter(|s| !s.hidden) else { return };
        let key = (w.scan.generation, w.npc_names.len(), w.my_index, s.filter, self.origin, self.region);
        if s.built == Some(key) {
            s.cursor = s.cursor.min(s.rows.len().saturating_sub(1));
            s.scroll = s.scroll.min(s.cursor);
            return;
        }
        s.built = Some(key);
        let names: HashMap<u16, &String> = w.npc_names.iter().map(|(id, n)| ((id & 0x3ff) as u16, n)).collect();
        s.rows = w
            .scan
            .entries
            .values()
            .filter(|e| !(e.kind == 0 && e.index == w.my_index))
            .filter_map(|e| {
                let (sprite, name) = match e.kind {
                    0 => (363, e.name.clone()),
                    1 => (326, names.get(&(e.index & 0x3ff)).map(|n| n.to_string()).unwrap_or_default()),
                    2 => (327, names.get(&(e.index & 0x3ff)).map(|n| n.to_string()).unwrap_or_default()),
                    _ => return None,
                };
                let (px, py) = self.region.px(self.origin.0 as f32 + e.dx as f32, self.origin.1 as f32 + e.dz as f32);
                let inside = s.filter.is_none_or(|((cx, cy), r2)| (px - cx).pow(2) + (py - cy).pow(2) < r2);
                (!name.is_empty() && inside).then_some(ScanRow {
                    index: e.index,
                    px: (px as f32, py as f32),
                    sprite,
                    name,
                    trackable: e.kind != 0,
                })
            })
            .collect();
        s.cursor = s.cursor.min(s.rows.len().saturating_sub(1));
        s.scroll = s.scroll.min(s.cursor);
    }

    /// map0 item 2 (Wide Scan) is enabled.
    pub fn scan_ok(&self) -> bool {
        self.scan_ok
    }

    /// The mouse's cursor move onto map0 row `k` (1..3). True when it moved.
    pub fn set_cursor(&mut self, k: u16) -> bool {
        if !self.has_menu() || self.cursor == k || !(1..=3).contains(&k) {
            return false;
        }
        self.cursor = k;
        true
    }

    /// The focused sub-window and its cursor child, for the bars.
    pub fn focus(&self) -> Option<(&'static str, u16)> {
        if let Some(s) = self.scan.as_ref().filter(|s| !s.hidden) {
            return Some(match s.mapscan {
                Some(c) => ("menu    mapscan", c),
                None => ("menu    scanlist", (s.cursor - s.scroll + 1) as u16),
            });
        }
        if let Some(l) = &self.maplist {
            return Some(("menu    maplist", (l.cursor - l.scroll + 1) as u16));
        }
        self.has_menu().then_some(("menu    map0", self.cursor))
    }

    /// Modes 0 / 5 close on Escape (map0); the event modes only by their script.
    pub fn closing(&self) -> bool {
        self.closing.is_some()
    }

    /// The zoom to store in config 0x8a on close (FUN_101fa420): zoom x 1000 / fit, whatever the mode.
    pub fn zoom_cfg(&self) -> u32 {
        (self.zoom * 1000.0 / self.fit) as u32
    }

    /// FUN_101fa420 -> FUN_101f82b0 (30261006_0): the .MRK written when a marker changed (+0xa44), named by the zone
    /// and +0x102. The only caller of the save.
    fn save_marks(&mut self) {
        if self.dirty
            && let Some(d) = &self.mark_dir
        {
            self.marks.save(d, self.region.zone as u16, self.key, self.char_id);
            self.dirty = false;
        }
    }

    /// A file edit with this map up. The mode-4 load clears +0xa44 (FUN_101f9a50), so unsaved marks are dropped
    /// unless the load was refused (`kept`); FUN_101fa420(1) then saves. An event map's mapframe is closed first
    /// (FUN_100b2570 -> FUN_101fa550, gone at once: true); any other fades (FUN_101ee580).
    fn close_for_file(&mut self, kept: bool) -> bool {
        if kept {
            self.save_marks();
        }
        self.dirty = false;
        if self.event_opened() {
            return true;
        }
        self.begin_close();
        false
    }

    /// Slot 20 (FUN_101ee580): fade to 0xff at 0x10 a frame, the window gone when that ends or after 3 s.
    pub fn begin_close(&mut self) {
        if self.closing.is_none() {
            self.save_marks();
            self.list = None;
            self.closing = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            self.fade = (self.fade.0, 255.0, 16.0);
        }
    }

    /// map0's keys: up / down along its links (1..3), Escape closes both windows (FUN_101f14c0). Returns false when
    /// the map starts closing.
    pub fn key(&mut self, key: egui::Key) -> bool {
        use crate::audio::Ui;
        if self.closing.is_some() {
            return true;
        }
        // FUN_101fae40 mode 7: Enter (FUN_101fb2c0) SE 0002, then with more than one image the next one after the
        // fade-out; Escape (FUN_101fb310) back to guide01. The arrows do nothing.
        if let Some((_, count, _)) = self.primer {
            match key {
                egui::Key::Enter => {
                    self.sounds.push(Ui::Select as u32);
                    if count > 1 {
                        self.primer_next = Some(true);
                        return false;
                    }
                }
                egui::Key::Escape => {
                    self.sounds.push(Ui::Cancel as u32);
                    self.primer_next = Some(false);
                    return false;
                }
                _ => {}
            }
            return true;
        }
        if self.list.is_some() {
            if !self.locked() {
                self.list_key(key);
            }
            return true;
        }
        if self.scan.is_some() {
            return self.scan_key(key);
        }
        if let Some(l) = self.maplist.as_mut() {
            match l.key(key, &mut self.sounds) {
                ListOut::Keep => {}
                ListOut::Close => self.maplist = None,
                ListOut::Pick(zone, key) => {
                    // LAB_101f7d70 -> FUN_101f7d80: the pick, then this map closes.
                    self.maplist = None;
                    self.reopen = Some((zone, key));
                    self.sounds.push(crate::audio::Ui::Select as u32);
                    return false;
                }
            }
            return true;
        }
        if !self.has_menu() {
            // A map we opened where none is needed (a ship's or airship's route chart) has no map0 (the only way
            // here for Menu / Other), but Escape still closes it; an event's map takes no keys (the event closes it).
            if key == egui::Key::Escape && matches!(self.mode, Mode::Menu | Mode::Other) {
                self.sounds.push(Ui::Cancel as u32);
                return false;
            }
            return true;
        }
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                self.cursor = match key {
                    egui::Key::ArrowUp if self.cursor <= 1 => 3,
                    egui::Key::ArrowUp => self.cursor - 1,
                    _ if self.cursor >= 3 => 1,
                    _ => self.cursor + 1,
                };
                self.sounds.push(Ui::Cursor as u32);
            }
            egui::Key::Escape => {
                self.sounds.push(Ui::Cancel as u32);
                return false;
            }
            // FUN_101f9270 1: mapv3, asking for party positions first when it opens on set 2 / 3.
            egui::Key::Enter if self.cursor == 1 => {
                if self.set > 1 {
                    self.want_party();
                }
                self.list = Some(List { cursor: 1, ..Default::default() });
                self.sounds.push(Ui::Select as u32);
            }
            // FUN_101f7c60 2: Wide Scan (disabled: SE 0004); while tracking, the stop prompt first (group 0xe 0x45).
            egui::Key::Enter if self.cursor == 2 => {
                if !self.scan_ok {
                    self.sounds.push(Ui::Error as u32);
                } else if self.tracking {
                    self.outs.push(Out::AskStop);
                } else {
                    self.open_scan();
                    self.sounds.push(Ui::Select as u32);
                }
            }
            // FUN_101f7c60 3: Change Map (maplist, built where the records are).
            egui::Key::Enter if self.cursor == 3 => {
                self.maplist = Some(MapList::default());
                self.sounds.push(Ui::Select as u32);
            }
            egui::Key::Enter => log::info!("map0 item {} is not implemented yet", self.cursor),
            _ => {}
        }
        true
    }

    /// The narrow cursor (sub-mode 7, FUN_101fae40), mapscan (FUN_101fbb30), scanlist (FUN_101f6c70 / FUN_101fba50).
    /// False when the map closes (Track target).
    fn scan_key(&mut self, key: egui::Key) -> bool {
        use crate::audio::Ui;
        use egui::Key as K;
        let Some(mut s) = self.scan.take() else { return true };
        if let Some(c) = s.narrow {
            match key {
                // Centre = the map pixel under the cursor, radius 0.65 x the anc_scan ring's 128 px / zoom.
                K::Enter => {
                    let k = 1.0 / self.zoom;
                    let r = (128.0 * 0.65 * k + 0.5) as i32;
                    let centre = ((c.0 * k + self.view.0 + 0.5) as i32, (c.1 * k + self.view.1 + 0.5) as i32);
                    (s.filter, s.narrow, s.hidden) = (Some((centre, r * r)), None, false);
                    self.sounds.push(Ui::Select as u32);
                }
                K::Escape => (s.narrow, s.hidden) = (None, false),
                _ => {}
            }
        } else if let Some(c) = s.mapscan {
            let row = s.rows.get(s.cursor).cloned();
            match key {
                K::ArrowUp | K::ArrowDown => s.mapscan = Some(3 - c),
                K::Escape => s.mapscan = None,
                // FUN_100f93c0: c2s 0x0F5, then map0, scanlist and the map close.
                K::Enter if c == 1 => match row.filter(|r| r.trackable) {
                    Some(r) => {
                        self.outs.push(Out::Track(r.index));
                        self.sounds.push(Ui::Select as u32);
                        return false;
                    }
                    None => self.sounds.push(Ui::Error as u32),
                },
                K::Enter => {
                    (s.filter, s.hidden, s.mapscan, s.narrow) = (None, true, None, Some(self.point));
                    self.sounds.push(Ui::Select as u32);
                }
                _ => {}
            }
        } else {
            let n = s.rows.len();
            match key {
                K::ArrowDown if s.cursor + 1 < n => {
                    s.cursor += 1;
                    self.sounds.push(Ui::Cursor as u32);
                }
                K::ArrowUp if s.cursor > 0 => {
                    s.cursor -= 1;
                    self.sounds.push(Ui::Cursor as u32);
                }
                // FUN_101f6c70 3 / 4 (list +0x47 = 1, so no modifier): a page of 10, the cursor row kept; at either
                // end the cursor goes to the first / last row instead (SE 0001 only when something moved).
                K::ArrowRight | K::ArrowLeft if n > 0 => {
                    let max = n.saturating_sub(SCAN_ROWS);
                    let row = s.cursor - s.scroll;
                    let last = (n - s.scroll).min(SCAN_ROWS) - 1;
                    let (scroll, cursor) = match key {
                        K::ArrowRight if s.scroll == max => (s.scroll, s.scroll + last),
                        K::ArrowRight => ((s.scroll + SCAN_ROWS).min(max), (s.scroll + SCAN_ROWS).min(max) + row),
                        _ if s.scroll == 0 => (0, 0),
                        _ => (s.scroll.saturating_sub(SCAN_ROWS), s.scroll.saturating_sub(SCAN_ROWS) + row),
                    };
                    if (scroll, cursor) != (s.scroll, s.cursor) {
                        self.sounds.push(Ui::Cursor as u32);
                    }
                    (s.scroll, s.cursor) = (scroll, cursor.min(n - 1));
                }
                K::Enter => {
                    if let Some(r) = s.rows.get(s.cursor) {
                        self.scroll_to_px(r.px);
                        s.mapscan = Some(1);
                        self.sounds.push(Ui::Select as u32);
                    }
                }
                // FUN_101fb890: the list closes, results kept; the busy strip goes.
                K::Escape => {
                    self.outs.push(Out::NotBusy);
                    self.sounds.push(Ui::Cancel as u32);
                    return true;
                }
                _ => {}
            }
            crate::client::scrollbar::keep_in_view(&mut s.scroll, s.cursor, SCAN_ROWS);
        }
        self.scan = Some(s);
        true
    }

    /// mapv3 (FUN_101fc070), mapv2 (FUN_101fc400) and mapframe in cursor mode.
    fn list_key(&mut self, key: egui::Key) {
        use crate::audio::Ui;
        use egui::Key as K;
        let Some(mut l) = self.list.take() else { return };
        if l.rename.is_some() {
            self.list = Some(l);
            return;
        }
        let set = self.set as usize;
        if let Some((slot, cur)) = l.placing {
            match key {
                // FUN_101fa1a0 -> FUN_101fa300: the marker at the map pixel under the centre of the cursor sprite
                // (half its width / zoom, on both axes), then the name entry.
                K::Enter => {
                    let half = (self.cursor_w * 0.5 / self.zoom + 0.5).trunc();
                    let px = (
                        (cur.0 / self.zoom + self.view.0 + 0.5).trunc() + half + self.shift.0,
                        (cur.1 / self.zoom + self.view.1 + 0.5).trunc() + half + self.shift.1,
                    );
                    let name = self.marks.sets[0][slot].name.clone();
                    self.marks.sets[0][slot] =
                        super::mapmark::Slot { px: (px.0 as i16, px.1 as i16), name: name.clone(), valid: true };
                    self.dirty = true;
                    (l.placing, l.v2, l.rename) = (None, None, Some((slot, name)));
                    self.sounds.push(Ui::Select as u32);
                }
                K::Escape => {
                    l.placing = None;
                    self.sounds.push(Ui::Cancel as u32);
                }
                _ => {}
            }
        } else if let Some((row, c)) = l.v2.as_mut() {
            let (row, valid) = (*row, self.marks.sets[0][*row].valid);
            match key {
                K::ArrowUp => *c = if *c <= 1 { 3 } else { *c - 1 },
                K::ArrowDown => *c = if *c >= 3 { 1 } else { *c + 1 },
                K::Escape => {
                    l.v2 = None;
                    self.sounds.push(Ui::Cancel as u32);
                }
                // FUN_101fda10: every choice closes mapv2. View (disabled on an empty slot, FUN_101fd930) scrolls;
                // Remove (FUN_101fa3b0) clears only the valid byte, the name and the pixel stay; Place starts the
                // cursor (FUN_101fa130); Rename opens the name entry (FUN_101fd3a0).
                K::Enter => {
                    match *c {
                        1 if valid => {
                            let p = self.marks.sets[0][row].px;
                            self.scroll_to_px((p.0 as f32, p.1 as f32));
                        }
                        2 if valid => {
                            self.marks.sets[0][row].valid = false;
                            self.dirty = true;
                        }
                        2 => l.placing = Some((row, self.point)),
                        3 => l.rename = Some((row, self.marks.sets[0][row].name.clone())),
                        _ => {
                            self.sounds.push(Ui::Error as u32);
                            self.list = Some(l);
                            return;
                        }
                    }
                    l.v2 = None;
                    self.sounds.push(Ui::Select as u32);
                }
                _ => {}
            }
        } else {
            match key {
                K::ArrowUp => l.cursor = if l.cursor <= 1 { 10 } else { l.cursor - 1 },
                K::ArrowDown => l.cursor = if l.cursor >= 10 { 1 } else { l.cursor + 1 },
                // Messages 3 / 4: the positions asked for (once, whatever the set), then the next / previous set.
                K::ArrowRight | K::ArrowLeft => {
                    self.want_party();
                    self.step_set(if key == K::ArrowRight { 1 } else { -1 });
                    self.sounds.push(Ui::Cursor as u32);
                }
                K::Escape => {
                    self.sounds.push(Ui::Cancel as u32);
                    return;
                }
                K::Enter => {
                    let row = l.cursor as usize - 1;
                    match (set, self.slot(set, row)) {
                        (0, _) => {
                            l.v2 = Some((row, 1));
                            self.sounds.push(Ui::Select as u32);
                        }
                        (1, Some((p, ..))) => {
                            self.scroll_to_px(p);
                            self.sounds.push(Ui::Select as u32);
                        }
                        // FUN_101fa6d0: only this member of the set labelled.
                        (_, Some((p, ..))) => {
                            self.label_only[set - 2] = Some(row);
                            self.scroll_to_px(p);
                            self.sounds.push(Ui::Select as u32);
                        }
                        _ => self.sounds.push(Ui::Error as u32),
                    }
                }
                _ => {}
            }
        }
        self.list = Some(l);
    }

    /// FUN_101ee040: the view stays over the image (min the image base, max base + the part past the screen), widened
    /// by the margins in mode 6.
    fn bounds(&self) -> [f32; 4] {
        let (sw, sh) = self.screen;
        let (z, m) = (self.zoom, self.margin);
        let max = |s: f32| (((512.0 + m) * z - s) / z).max(0.0);
        [self.base.0 - m, self.base.1 - m, self.base.0 + max(sw), self.base.1 + max(sh)]
    }

    fn clamp(&mut self) {
        let b = self.bounds();
        self.view = (self.view.0.clamp(b[0], b[2]), self.view.1.clamp(b[1], b[3]));
    }

    /// FUN_101ede30: the zoom by dz within [fit, 4 fit], the map pixel under the cursor point kept.
    fn zoom_by(&mut self, dz: f32) {
        let z = (self.zoom + dz).clamp(self.fit, self.fit * 4.0);
        self.view.0 += self.point.0 / self.zoom - self.point.0 / z;
        self.view.1 += self.point.1 / self.zoom - self.point.1 / z;
        self.zoom = z;
        self.clamp();
    }

    /// FUN_101edaf0 (normal mode): the view by d screen pixels, clamped.
    fn scroll_by(&mut self, d: (f32, f32)) {
        self.view.0 += d.0 / self.zoom;
        self.view.1 += d.1 / self.zoom;
        self.clamp();
    }

    /// The map pixel under the cursor point.
    fn under_point(&self) -> (f32, f32) {
        (
            self.point.0 / self.zoom + self.view.0 + 0.5 + self.shift.0,
            self.point.1 / self.zoom + self.view.1 + 0.5 + self.shift.1,
        )
    }

    /// FUN_101ef990: start easing `to` (moved by the mode 6 pad) under the cursor point unless it is already there.
    fn scroll_to_px(&mut self, to: (f32, f32)) {
        let to = (to.0 + self.pad, to.1 + self.pad);
        let p = self.under_point();
        if (to.0 - p.0).powi(2) + (to.1 - p.1).powi(2) >= 0.8 {
            self.clamp();
            self.scroll_to = Some(to);
        }
    }

    /// Op 0xb4 sub 0x14 (FUN_101fb070): view to (x, y) map pixels at zoom factor zf of the base zoom, `speed` (x 0.005
    /// a tick) in mode flags & 0x7f (1 linear, 2 eases out, 3 eases in); speed 0 jumps; flag 0x80 shows the map at once.
    /// The keys, a fade and the closing all stop (the flags become 0x400).
    pub fn view_to(&mut self, x: f32, y: f32, zf: f32, speed: f32, flags: i32) {
        let cur = self.scripted.map_or([0.0, 0.0, 1.0], |s| s.cur);
        let mut s =
            Scripted { cur, start: cur, target: [x, y, zf], speed: speed.max(0.0), mode: (flags & 0x7f) as u8, t: 0.0 };
        if s.speed == 0.0 {
            (s.t, s.cur) = (1.0, s.target);
        }
        self.scripted = Some(s);
        (self.keys, self.zoom_to, self.scroll_to, self.closing) = (false, None, None, None);
        self.fade.2 = 0.0;
        if flags & 0x80 != 0 {
            self.fade = (0.0, 0.0, 255.0);
        }
        self.apply_scripted();
    }

    /// FUN_101edee0: zoom = zf x base, the view centred on (x, y) without the bounds.
    fn apply_scripted(&mut self) {
        let Some(s) = self.scripted else { return };
        self.zoom = s.cur[2] * self.zoom_base;
        let (sw, sh) = self.screen;
        self.view = (s.cur[0] - sw / 2.0 / self.zoom, s.cur[1] - sh / 2.0 / self.zoom);
    }

    /// One frame (FUN_101ef0a0): the zoom keys (0x7d back to the base, 0xa4 / 0xa5 or the compact stick 0.04 a tick,
    /// doubled with 0x9a), the scroll keys (0x79 / 0x7a, 7.5 px a tick, doubled), 0x7e back to the player, the
    /// scripted view; the fade, zoom-to and scroll-to on 2-tick retail frames. False once the window is gone.
    /// `drag`: a left drag's offset from its press point in UI pixels (FUN_10126100 / FUN_10125fe0: past W / 21 on an
    /// axis it counts as full scroll that way, the anchor taken as the press point *(inf)*).
    pub fn update(&mut self, input: &mut super::input::Input, ticks: f32, drag: Option<(f32, f32)>) -> bool {
        use super::input::mode::{HELD, PRESS};
        if let Some(deadline) = self.closing {
            let idle = self.zoom_to.is_none() && self.scroll_to.is_none() && self.fade.0 == self.fade.1;
            if idle || std::time::Instant::now() >= deadline {
                return false;
            }
        }
        let fast = if input.test(0x9a, HELD) { 2.0 } else { 1.0 };
        let live = self.closing.is_none() && self.scripted.is_none() && self.keys;
        if live && self.zoom_to.is_none() {
            if input.test(0x7d, PRESS) || (input.test(0xa4, HELD) && input.test(0xa5, HELD)) {
                self.zoom_to = Some(self.zoom_base);
            } else {
                // Axis 0x7c: the compact stick against -(0xa4 held ? 1 : 0xa5 held ? -1 : 0), the larger winning.
                let keys: f32 = -(if input.test(0xa4, HELD) {
                    1.0
                } else if input.test(0xa5, HELD) {
                    -1.0
                } else {
                    0.0
                });
                let stick = input.axis(0x7c);
                let a = if stick.abs() > keys.abs() { stick } else { keys };
                if a != 0.0 {
                    self.zoom_by(-a * 0.08 * fast * ticks * 0.5);
                }
            }
        }
        // Cursor mode (flag 0x100: placing a marker, narrowing a scan) reads the cursor axes instead (FUN_101ef0a0).
        let cursor_mode = self.list.as_ref().is_some_and(|l| l.placing.is_some())
            || self.scan.as_ref().is_some_and(|s| s.narrow.is_some());
        if live && self.scroll_to.is_none() && !cursor_mode {
            // Under 2 px a retail frame is dead (judged per 2-tick frame, so it holds at any frame rate).
            let dead = |v: f32| if (v * fast).abs() < 2.0 { 0.0 } else { v };
            let step = |d: f32| {
                if d / (self.screen.0 * 0.047619) >= 1.0 {
                    1.0
                } else if d / (self.screen.0 * 0.047619) <= -1.0 {
                    -1.0
                } else {
                    0.0
                }
            };
            let (mx, my) = drag.map_or((0.0, 0.0), |d| (step(d.0), step(d.1)));
            let ax = if input.axis(0x79) != 0.0 { input.axis(0x79) } else { mx };
            let ay = if input.axis(0x7a) != 0.0 { input.axis(0x7a) } else { my };
            let v = (dead(ax * 15.0) * fast * ticks * 0.5, dead(ay * 15.0) * fast * ticks * 0.5);
            if v != (0.0, 0.0) {
                self.scroll_by(if self.texture.is_some() { v } else { (-v.0, -v.1) });
            }
        }
        if live && self.scroll_to.is_none() && input.test(0x7e, PRESS) && self.show_player {
            self.scroll_to_px(self.home);
        }
        if let Some(s) = self.scripted.as_mut()
            && s.mode != 0
        {
            for i in 0..3 {
                s.cur[i] = s.start[i] + (s.target[i] - s.start[i]) * s.t;
            }
            let step = ticks * s.speed * 0.005;
            s.t = match s.mode {
                1 => s.t + step,
                2 => s.t + (1.0 - s.t + 0.02) * step,
                3 => s.t + (s.t + 0.02) * step,
                _ => s.t,
            }
            .min(1.0);
        }
        self.apply_scripted();
        self.acc += ticks;
        while self.acc >= 2.0 {
            self.acc -= 2.0;
            self.frame();
        }
        // Cursor mode (placing a marker, narrowing a scan): axes 0x9d / 0x9e x the cursor speed 8, inside the cursor
        // box; past it the view scrolls.
        let cur = self.list.as_ref().and_then(|l| l.placing.map(|p| p.1)).or(self.scan.as_ref().and_then(|s| s.narrow));
        if let Some(cur) = cur {
            let (sw, sh) = self.screen;
            let dead = |v: f32| if (v * fast).abs() < 2.0 { 0.0 } else { v };
            let v =
                (dead(input.axis(0x9d) * 8.0) * fast * ticks * 0.5, dead(input.axis(0x9e) * 8.0) * fast * ticks * 0.5);
            let (bx, by) = ([sw / 512.0 * 40.0, sw - sw / 512.0 * 150.0], [sh / 448.0 * 65.0, sh - sh / 448.0 * 180.0]);
            let want = (cur.0 + v.0, cur.1 + v.1);
            let moved = (want.0.clamp(bx[0], bx[1]), want.1.clamp(by[0], by[1]));
            let over = (want.0 - moved.0, want.1 - moved.1);
            if over != (0.0, 0.0) {
                self.scroll_by(over);
            }
            if let Some(p) = self.list.as_mut().and_then(|l| l.placing.as_mut()) {
                p.1 = moved;
            } else if let Some(s) = self.scan.as_mut() {
                s.narrow = Some(moved);
            }
        }
        // The scan's 30 s timer (scanlist +0x62) ends the busy strip.
        if let Some(s) = self.scan.as_mut().filter(|s| s.scanning) {
            s.timer -= ticks;
            if s.timer < 0.0 {
                s.scanning = false;
                self.outs.push(Out::NotBusy);
            }
        }
        if self.fade.0 == 0.0 && self.list.is_some() {
            // FUN_101f0560 over the list mapv3 hands the window while it is open (+0x94, FUN_101fd280 / FUN_101fd310):
            // the shown set's markers fade in 6 a tick, the rest out. With mapv3 shut nothing runs and the alphas stay.
            for s in 0..4 {
                for i in 0..10 {
                    let on = s == self.set as usize && self.slot(s, i).is_some();
                    let a = &mut self.mark_alpha[s][i];
                    *a = (*a + if on { 6.0 } else { -6.0 } * ticks).clamp(0.0, 128.0);
                }
            }
        }
        if self.fade.0 == 0.0
            && let Some(p) = self.points.as_mut()
        {
            // FUN_101f0560 (flags 0x81): fade in 6 a tick; with 0x1000 (not registered) out again at 1.5 a tick (flag
            // 0x2000: 3 x 0.5 against the rise's 12 x 0.5) and back, blinking.
            for m in &mut p.marks {
                m.alpha += if m.falling { -1.5 } else { 6.0 } * ticks;
                if m.alpha >= 128.0 {
                    (m.alpha, m.falling) = (128.0, m.blink);
                } else if m.alpha <= 0.0 {
                    (m.alpha, m.falling) = (0.0, false);
                }
            }
        }
        if self.fade.0 == 0.0 && self.show_player {
            // FUN_101f0560 (flags 0x9f): fade in 6 a tick, then blink between 0 and 0x80 at the same speed (flag 0x400).
            self.alpha += if self.rising { 6.0 } else { -6.0 } * ticks;
            if self.alpha >= 128.0 {
                (self.alpha, self.rising) = (128.0, false);
            } else if self.alpha <= 0.0 {
                (self.alpha, self.rising) = (0.0, true);
            }
        }
        true
    }

    /// One retail frame (2 ticks): fade, zoom-to (a fifth of the way, done under 0.003), scroll-to (the gap / zoom,
    /// halved when over 2 x zoom; one exact step once it moves under sqrt(0.8) map pixels).
    fn frame(&mut self) {
        let (level, target, step) = self.fade;
        if level != target {
            self.fade.0 =
                if (target - level).abs() <= step { target } else { level + step * (target - level).signum() };
        }
        if let Some(t) = self.zoom_to {
            let dz = (t - self.zoom) * 0.2;
            if dz.abs() < 0.003 {
                self.zoom_by(t - self.zoom);
                self.zoom_to = None;
            } else {
                self.zoom_by(dz);
            }
        }
        if let Some(to) = self.scroll_to {
            let p = self.under_point();
            let mut d = (to.0 - p.0, to.1 - p.1);
            if d.0 > 2.0 * self.zoom || d.1 > 2.0 * self.zoom {
                d = (d.0 / 2.0, d.1 / 2.0);
            }
            let before = self.view;
            self.scroll_by(d);
            let moved = ((self.view.0 - before.0).powi(2) + (self.view.1 - before.1).powi(2)).sqrt();
            if moved < 0.8f32.sqrt() {
                let p = self.under_point();
                self.scroll_by(((to.0 - p.0) * self.zoom, (to.1 - p.1) * self.zoom));
                self.scroll_to = None;
            }
        }
    }

    /// mapframe: the image (0xa0404040; alpha min(0xff - level, 0xa0) while fading) at (base - view) x zoom; at fade
    /// level 0 the player arrow and the grid text under titlewin; then map0.
    /// `modern`: the modern layer draws map0, Markers and Change Map's list (and takes their mouse).
    pub fn draw(&mut self, gu: &mut GameUi, sw: f32, sh: f32, modern: bool) {
        if let Some(Ok(Some((name, iw, ih, rgba)))) = self.rx.as_ref().map(|r| r.try_recv()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
            self.rx = None;
        }
        let z = self.zoom;
        let level = self.fade.0;
        let a = if level == 0.0 { 160.0 } else { (255.0 - level).min(160.0) } / 128.0;
        if self.mode == Mode::Preview && level < 255.0 {
            let at = |b: f32, v: f32| ((b - PREVIEW_PAD - v) * z + 0.5).trunc();
            gu.sprite_scaled(
                BACKDROP,
                0,
                at(self.base.0, self.view.0),
                at(self.base.1, self.view.1),
                z,
                z,
                ((a * 128.0) as u32).min(0xff) << 24 | 0x40_4040,
            );
        }
        if let Some(t) = &self.texture {
            gu.image_rect(
                t,
                (self.base.0 - self.view.0) * z + 0.5,
                (self.base.1 - self.view.1) * z + 0.5,
                512.0 * z,
                512.0 * z,
                [0.5, 0.5, 0.5, a.min(1.0)],
            );
        }
        if level == 0.0 {
            self.draw_points(gu);
            if self.scan.is_some() {
                self.draw_scan_markers(gu);
            } else {
                self.draw_markers(gu);
            }
        }
        if level == 0.0 && self.show_player {
            let (px, py) = self.home;
            let sx = (px - self.shift.0 - self.view.0) * z + 0.5;
            let sy = (py - self.shift.1 - self.view.1) * z + 0.5;
            // The sprite points east; turned to our facing (forward (cos r, 0, -sin r) is (cos r, sin r) on the map).
            let (s, c) = self.heading.sin_cos();
            gu.sprite_affine("menu    keytops3", MARKER, sx, sy, [c, -s, s, c], (self.alpha as u32) << 24 | 0x80_8080);
            // FUN_101fa9c0: "(G-9)" at (titlewin x0, titlewin y1 + 3) in fontshp; not in mode 6.
            if self.mode != Mode::Preview {
                gu.shape_text(&self.region.grid(px as i32, py as i32), 16.0, 49.0, 0x8080_8080, false);
            }
        }
        // win_mapframe_v10: FUN_101fbfd0 (the arrow and grid above), then FUN_101fc130, the battlefield overlay.
        if let Some(o) = self
            .battlefield
            .as_ref()
            .filter(|_| super::battlefield::overlay_shown(self.zone(), self.show_player, self.title_up))
        {
            // FUN_101f8b80 (our px) to 16-bit map pixels, then FUN_101f11d0: (px - shift - view) x zoom + 0.5,
            // truncated.
            let screen = |wx: f32, wz: f32| {
                let (px, py) = self.region.px(wx, wz);
                let at =
                    |p: i32, shift: f32, view: f32| ((p as i16 as f32 - shift - view) * self.zoom + 0.5) as i32 as f32;
                (at(px, self.shift.0, self.view.0), at(py, self.shift.1, self.view.1))
            };
            super::battlefield::draw_overlay(gu, o, screen);
        }
        // The tracked target's ring (keytops3 105, centred by its bounds).
        if let Some((px, py)) = self.tracked.filter(|_| level == 0.0) {
            gu.sprite(
                "menu    keytops3",
                105,
                (px - self.shift.0 - self.view.0) * z + 0.5,
                (py - self.shift.1 - self.view.1) * z + 0.5,
                0x8080_8080,
            );
        }
        if self.has_menu() && !modern {
            let hide: &[u16] = if self.scan_ok { &[] } else { &[2] };
            if let Some(win) = gu.lobby_window_off("menu    map0", sw, sh, (0.0, 0.0), Some(self.cursor), true, hide)
                && !self.scan_ok
                && let Some(&(_, r)) = win.children.iter().find(|c| c.0 == 1)
            {
                // Item 2 disabled: its state-4 sprite (keytops3 20) in place of 3; under the cursor at (+1, +1) in
                // 0x80805020 (FUN_101177d0).
                let on = self.cursor == 2;
                let d = if on { 1.0 } else { 0.0 };
                gu.sprite(
                    "menu    keytops3",
                    20,
                    r[0] + d,
                    r[1] + 16.0 + d,
                    if on { 0x8080_5020 } else { 0x8080_8080 },
                );
            }
        }
        self.draw_scan(gu, sw, sh, modern);
        // FUN_101fa1a0 reads the cursor's width from its sprite (x1 - x0), as a sprite pack may have resized it.
        if let Some(b) = gu.cell_bounds("menu    keytops3", PLACE_CURSOR, 0) {
            self.cursor_w = b[2] - b[0];
        }
        self.draw_list(gu, sw, sh, modern);
        if let Some(l) = self.maplist.as_ref().filter(|_| !modern) {
            l.draw(gu, sw, sh);
        }
    }

    /// Scan markers (flags 0x99, not centred, alpha 0x80): markblue / markgree / markred; the cursor row's as the ring
    /// keytops3 105.
    fn draw_scan_markers(&self, gu: &mut GameUi) {
        let Some(s) = &self.scan else { return };
        let z = self.zoom;
        for (k, r) in s.rows.iter().enumerate() {
            let sprite = if k == s.cursor { 105 } else { r.sprite };
            gu.sprite(
                "menu    keytops3",
                sprite,
                (r.px.0 - self.shift.0 - self.view.0) * z + 0.5,
                (r.px.1 - self.shift.1 - self.view.1) * z + 0.5,
                0x8080_8080,
            );
        }
    }

    /// scanlist (16, 48), 10 rows of 24 with the anc_trac cursor, names fitted like the target window's (width 88 at
    /// +7, +6); mapscan over map0's place; the narrow cursor (anc_scan) at the cursor point.
    fn draw_scan(&self, gu: &mut GameUi, sw: f32, sh: f32, modern: bool) {
        let Some(s) = &self.scan else { return };
        let frame = gu.cursor_frame();
        if let Some(c) = s.narrow {
            let n = gu.lib_len("anc     anc_scan").max(1);
            gu.sprite("anc     anc_scan", frame % n, c.0, c.1, 0x8080_8080);
            return;
        }
        // The modern layer draws scanlist and mapscan as panels (modern/minor.rs).
        if modern {
            return;
        }
        if let Some(win) = gu.lobby_window_off("menu    scanlist", sw, sh, (0.0, 0.0), None, false, &[]) {
            let [x, y, fw, fh] = win.frame;
            gu.scroll_bar(x + fw - 8.0, y, fh, s.rows.len(), SCAN_ROWS, s.scroll);
            if s.mapscan.is_none() {
                gu.bar_site(super::scrollbar::Owner::ScanList, x + fw - 8.0, y, fh, s.rows.len(), SCAN_ROWS, s.scroll);
            }
            for (k, r) in s.rows.iter().enumerate().skip(s.scroll).take(SCAN_ROWS) {
                let ry = y + 4.0 + 24.0 * (k - s.scroll) as f32;
                gu.wrapped_name(&r.name, 88.0, x + 7.0, ry + 2.0, 0x8080_8080, true, false);
                if k == s.cursor && s.mapscan.is_none() {
                    let n = gu.lib_len("anc     anc_trac").max(1);
                    gu.sprite("anc     anc_trac", frame % n, x, ry, 0x8080_8080);
                }
            }
        }
        // An untrackable row disables Track, which has no disabled sprite: keytops3 300 at 0x30808080, or under the
        // cursor 0x40402810 at (+1, +1) (FUN_10119c00 / FUN_101177d0).
        if let Some(c) = s.mapscan {
            let off = s.rows.get(s.cursor).is_some_and(|r| !r.trackable);
            let hide: &[u16] = if off { &[1] } else { &[] };
            if let Some(win) = gu.lobby_window_off("menu    mapscan", sw, sh, (0.0, 0.0), Some(c), true, hide)
                && off
                && let Some(&(_, r)) = win.children.iter().find(|ch| ch.0 == 2)
            {
                let d = if c == 1 { 1.0 } else { 0.0 };
                gu.sprite(
                    "menu    keytops3",
                    300,
                    r[0] + d,
                    r[1] - 16.0 + d,
                    if c == 1 { 0x4040_2810 } else { 0x3080_8080 },
                );
            }
        }
    }

    /// FUN_101ee7b0 for the four sets: the sprite centred on its pixel, alpha << 24 | 0x808080; a label (colour 2,
    /// 0x606070, scaled by the alpha) at (+16, +1) for sets 0 / 1 and the picked member.
    fn draw_markers(&self, gu: &mut GameUi) {
        // The window draws the records only while mapv3 lends them (+0x94).
        if self.list.is_none() {
            return;
        }
        for s in 0..4 {
            for i in 0..10 {
                let a = self.mark_alpha[s][i];
                let Some((px, label, k)) = self.slot(s, i).filter(|_| a > 0.0) else { continue };
                let labelled = s < 2 || self.label_only[s - 2] == Some(i);
                self.draw_mark(gu, px, k, label.filter(|_| labelled), a);
            }
        }
    }

    /// The point list (+0x9c), drawn before the sets.
    fn draw_points(&self, gu: &mut GameUi) {
        for m in self.points.iter().flat_map(|p| &p.marks).filter(|m| m.alpha > 0.0) {
            self.draw_mark(gu, m.px, m.sprite, Some(m.name), m.alpha);
        }
    }

    /// FUN_101ee7b0: one marker.
    fn draw_mark(&self, gu: &mut GameUi, (px, py): (f32, f32), k: usize, label: Option<&str>, a: f32) {
        let z = self.zoom;
        let b = gu.cell_bounds("menu    keytops3", k, 0).unwrap_or([0.0; 4]);
        let x = ((px - self.shift.0 - self.view.0) * z + 0.5).trunc() - ((b[2] - b[0]) / 2.0).trunc();
        let y = ((py - self.shift.1 - self.view.1) * z + 0.5).trunc() - ((b[3] - b[1]) / 2.0).trunc();
        let a = a as u32;
        gu.sprite("menu    keytops3", k, x, y, a << 24 | 0x80_8080);
        if let Some(t) = label.filter(|t| !t.is_empty()) {
            let c = |v: u32| (v * a) >> 7;
            gu.shape_text(t, x + 16.0, y + 1.0, a << 24 | c(0x60) << 16 | c(0x60) << 8 | c(0x70), false);
        }
    }

    /// FUN_101f9420 (each update): once the masks are in, the table, then every point on this map unnamed.
    pub(super) fn load_points(&mut self, table: impl FnOnce() -> Option<Arc<Vec<Point>>>, masks: Option<[u8; 64]>) {
        let Some(p) = self.points.as_mut().filter(|p| p.table.is_none()) else { return };
        let (Some(masks), Some(t)) = (masks, table()) else { return };
        (p.table, p.masks) = (Some(t), masks);
        self.place_points((0xffff, 0xffff));
    }

    /// FUN_101f9760: markers for this zone and sub-map's points, at most 16 (FUN_101f99a0): the others unnamed, then
    /// `sel` named with its kind (the cut can leave it out); unregistered ones blink.
    fn place_points(&mut self, sel: (u16, u16)) {
        let (zone, key, region) = (self.region.zone, self.key, self.region);
        let Some(p) = self.points.as_mut() else { return };
        let Some(table) = p.table.clone() else { return };
        let here: Vec<&Point> = table.iter().filter(|t| t.zone as i16 == zone && t.map == key).collect();
        let is_sel = |t: &Point| (tpoints::kind_row(t.kind), t.id) == sel;
        p.marks.clear();
        for t in here.iter().filter(|t| !is_sel(t)).chain(here.iter().filter(|t| is_sel(t))).take(16) {
            let (name, icon, _) = tpoints::KINDS[tpoints::kind_row(t.kind) as usize];
            let (x, y) = region.px(t.x, t.z);
            let blink = !tpoints::registered(&p.masks, t.kind, t.bit);
            p.marks.push(PointMark {
                px: (x as f32, y as f32),
                sprite: ICON_SPRITE[icon as usize],
                name: if is_sel(t) { name } else { "" },
                blink,
                alpha: 0.0,
                falling: false,
            });
        }
    }

    /// LAB_101f9510 -> FUN_101f9530, each frame for the menu's cursor option (kind, id): 0xffff hides; a point on
    /// another map reopens the preview there; on this one it eases toward the cursor point, named, and the map shows
    /// when owned (else the caller says why, once per zone).
    pub fn follow(&mut self, sel: (u16, u16)) -> Follow {
        let Some(p) = self.points.as_mut().filter(|p| p.last != sel && self.closing.is_none()) else {
            return Follow::Keep;
        };
        let Some(table) = p.table.clone() else { return Follow::Keep };
        let hide = (255.0, 255.0, self.fade.2);
        if sel.1 == 0xffff {
            p.last = (sel.0, 0xffff);
            (self.fade, self.help) = (hide, false);
            return Follow::Keep;
        }
        // Not found: the shown step only, every frame (the pair is not kept).
        if let Some(t) = table.iter().find(|t| t.kind == sel.0 && t.id == sel.1) {
            if t.zone as i16 != self.region.zone || t.map != self.key {
                self.reopen = Some((t.zone as i16, t.map as i16));
                return Follow::Reopen;
            }
            p.last = sel;
            let (x, y) = self.region.px(t.x, t.z);
            self.scroll_to_px((x as f32, y as f32));
            self.place_points(sel);
        }
        let out = if self.owned {
            (self.fade, self.help) = ((0.0, 0.0, self.fade.2), true);
            Follow::Keep
        } else {
            self.fade = hide;
            if self.no_record {
                Follow::NoMap
            } else if self.told != Some(self.region.zone as u16) {
                self.told = Some(self.region.zone as u16);
                Follow::NotOwned(self.region.zone as u16)
            } else {
                Follow::Keep
            }
        };
        if self.no_record {
            self.fade = hide;
        }
        out
    }

    /// helpwind slot 2's place (+0xb4): the map's zone, in mode 6 only while a point shows it.
    pub fn help_zone(&self) -> Option<u16> {
        self.help.then_some(self.region.zone as u16)
    }

    /// The image has loaded (FUN_101f9a20: the last request done).
    pub fn image_ready(&self) -> bool {
        self.rx.is_none()
    }

    /// mapv3's row `i` of the shown set: its icon (keytops3) and name (the one being typed, with the caret).
    /// FUN_101fd4d0: sets 0 / 1 show the empty or the placed icon and the slot's name either way (Remove leaves the
    /// name); set 2 the party icon for rows under the party count, the alliance one after; set 3 the alliance one.
    fn marker_row(&self, l: &List, i: usize) -> (Option<usize>, String) {
        let (s, slot) = (self.set as usize, self.slot(self.set as usize, i));
        let icon = match s {
            0 | 1 => Some(ROW_ICON[2 * s + slot.is_some() as usize]),
            2 => slot.map(|_| ROW_ICON[if i < self.party_count as usize { 5 } else { 7 }]),
            _ => slot.map(|_| ROW_ICON[7]),
        };
        let name = match l.rename.as_ref().filter(|r| r.0 == i) {
            Some((_, t)) => format!("{t}|"),
            None if s < 2 => self.marks.sets[s][i].name.clone(),
            None => slot.and_then(|s| s.1).unwrap_or_default().to_string(),
        };
        (icon, name)
    }

    /// The Markers window as the modern layer draws it (none while mapv3 is shut).
    pub fn markers_look(&self) -> Option<MarkersLook> {
        let l = self.list.as_ref()?;
        let rows = (0..10).map(|i| self.marker_row(l, i)).collect();
        let keys = l.v2.is_none() && l.placing.is_none() && l.rename.is_none() && !self.locked();
        let sets = [0, 1, 2, 3].map(|s| self.set_shown(s));
        let v2 = l.v2.map(|(row, c)| (row, c, self.marks.sets[0][row].valid));
        Some(MarkersLook {
            set: self.set as usize,
            sets,
            rows,
            cursor: keys.then(|| l.cursor as usize - 1),
            v2,
            placing: l.placing.is_some(),
        })
    }

    /// mapv3 (FUN_101fd4d0), mapv2 beside the cursor row (FUN_101fd930) and the placing cursor; `modern`: the
    /// modern layer draws the two windows.
    fn draw_list(&self, gu: &mut GameUi, sw: f32, sh: f32, modern: bool) {
        let Some(l) = &self.list else { return };
        if let Some((_, (cx, cy))) = l.placing {
            gu.sprite("menu    keytops3", PLACE_CURSOR, cx, cy, 0x8080_8080);
        }
        if modern {
            return;
        }
        let sel = if l.v2.is_some() || l.placing.is_some() || self.locked() { 0 } else { l.cursor };
        let Some((x, y)) = gu.menu_sel("menu    mapv3", sw, sh, sel) else { return };
        // Icon at (16, 5 + 16 i), name at (0x1f, 7 + 16 i) fitted into 0x44 px with the ellipsis (FUN_101cb320).
        for i in 0..10 {
            let ry = y + 16.0 * i as f32;
            let (icon, name) = self.marker_row(l, i);
            if let Some(k) = icon {
                gu.sprite("menu    keytops3", k, x + 16.0, ry + 5.0, 0x8080_8080);
            }
            if l.rename.as_ref().is_some_and(|r| r.0 == i) {
                gu.shape_text(&name, x + 31.0, ry + 7.0, 0x8080_8080, false);
            } else if !name.is_empty() {
                gu.shape_fit(&name, x + 31.0, ry + 7.0, NAME_FIT, 0x8080_8080);
            }
        }
        if let Some((row, c)) = l.v2 {
            let valid = self.marks.sets[0][row].valid;
            let at = (x + V2_AT.0, y + V2_AT.1 + 16.0 * row as f32);
            // Child 2's sprite: keytops3 29 "Place", 35 "Remove" (the window's +0x18 / +0x1c).
            let swaps: &[(u16, usize)] = if valid { &[(2, 35)] } else { &[] };
            let hide: &[u16] = if valid { &[] } else { &[1] };
            if let Some(win) = gu.lobby_window_swaps("menu    mapv2", sw, sh, at, Some(c), true, hide, swaps)
                && !valid
                && let Some(&(_, r)) = win.children.iter().find(|ch| ch.0 == 2)
            {
                // View disabled: its state-4 sprite (keytops3 332) a row above child 2; under the cursor at (+1, +1)
                // in 0x80805020, as map0's Wide Scan.
                let on = c == 1;
                let d = if on { 1.0 } else { 0.0 };
                gu.sprite(
                    "menu    keytops3",
                    332,
                    r[0] + d,
                    r[1] - 16.0 + d,
                    if on { 0x8080_5020 } else { 0x8080_8080 },
                );
            }
        }
    }
}

/// What the map asks of its caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// c2s 0x0D2 for party positions.
    AskParty,
    /// FUN_100f9360: a Wide Scan (c2s 0x0F4, or 0x0F6 while the server's list is still open).
    Scan,
    /// FUN_100f93c0: c2s 0x0F5 for an ActIndex.
    Track(u16),
    /// The stop-tracking prompt (comyn, group 0xe 0x45, default No).
    AskStop,
    /// The help bar's busy strip ends.
    NotBusy,
    /// FUN_10099680: c2s 0x114 for the teleport point masks (s2c 0x063 type 6).
    AskPoints,
}

const SCAN_ROWS: usize = 10;

/// Wide Scan's list (scanlist) with mapscan and the narrow cursor.
#[derive(Clone, Debug, Default)]
pub struct ScanList {
    cursor: usize,
    scroll: usize,
    /// A scan pending (+0x65) and its timer in ticks (+0x62).
    scanning: bool,
    timer: f32,
    /// The narrowed circle: centre map pixel, radius squared (+0x6c..+0x70).
    filter: Option<((i32, i32), i32)>,
    /// Hidden while narrowing.
    hidden: bool,
    rows: Vec<ScanRow>,
    /// What `rows` was built from: the scan, the names, us, the circle, the origin and the map.
    built: Option<(u64, usize, u16, Option<((i32, i32), i32)>, (i16, i16), Region)>,
    mapscan: Option<u16>,
    narrow: Option<(f32, f32)>,
}

#[derive(Clone, Debug)]
struct ScanRow {
    index: u16,
    px: (f32, f32),
    sprite: usize,
    name: String,
    trackable: bool,
}

/// A maplist row: its id (a zone, -2 - region group, -zone - 2 for a floor list, 0x7fffffff none), the sub-key, and
/// its two cells (FUN_101f4b20: column 0 type 1 game-font text; column 1 a number (type 9) or small text (0x21)).
#[derive(Clone, Debug, Default)]
pub struct Row {
    id: i32,
    key: i32,
    a: String,
    b: String,
    number: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Level {
    #[default]
    Top,
    Region(usize),
    Zone(u16),
}

/// Change Map (FUN_101fcae0 / FUN_101fcd90): the top level (the current area, then region groups with an owned
/// map), a group's zones, a zone's floors.
#[derive(Clone, Debug, Default)]
pub struct MapList {
    level: Level,
    /// The level above a floor list (+0x5e) and the cursor to return to (+0x5a).
    from: Option<(Level, usize)>,
    rows: Vec<Row>,
    built: bool,
    cursor: usize,
    scroll: usize,
}

enum ListOut {
    Keep,
    Close,
    Pick(i16, i16),
}

/// Columns of maplist's rows (table 0x10382188).
const COLS: [f32; 3] = [20.0, 245.0, 320.0];
const ROWS: usize = 5;

impl MapWin {
    /// A scroll bar or wheel message to Change Map's list or the Wide Scan list (FUN_101f6c70). True when it moved.
    pub fn bar(&mut self, owner: super::scrollbar::Owner, m: super::scrollbar::Msg) -> bool {
        use super::scrollbar::{Owner, rows_abs};
        match owner {
            Owner::MapList => self.maplist.as_mut().is_some_and(|l| {
                let n = l.rows.len();
                rows_abs(m, &mut l.scroll, &mut l.cursor, n, ROWS)
            }),
            Owner::ScanList => self.scan.as_mut().is_some_and(|s| {
                let n = s.rows.len();
                rows_abs(m, &mut s.scroll, &mut s.cursor, n, SCAN_ROWS)
            }),
            _ => false,
        }
    }
}

impl MapList {
    /// The list base (FUN_101f6c70; maplist keeps its +4, so the generic sounds are off): Up / Down stop at the ends
    /// with SE 0001 on a move, Right / Left page by 5.
    fn key(&mut self, key: egui::Key, sounds: &mut Vec<u32>) -> ListOut {
        let n = self.rows.len();
        match key {
            egui::Key::ArrowDown if self.cursor + 1 < n => {
                self.cursor += 1;
                sounds.push(crate::audio::Ui::Cursor as u32);
            }
            egui::Key::ArrowUp if self.cursor > 0 && n > 0 => {
                self.cursor -= 1;
                sounds.push(crate::audio::Ui::Cursor as u32);
            }
            egui::Key::ArrowRight | egui::Key::ArrowLeft => {
                let m = if key == egui::Key::ArrowRight {
                    super::scrollbar::Msg::PageDown
                } else {
                    super::scrollbar::Msg::PageUp
                };
                if super::scrollbar::rows_abs(m, &mut self.scroll, &mut self.cursor, n, ROWS) {
                    sounds.push(crate::audio::Ui::Cursor as u32);
                }
            }
            egui::Key::Enter => {
                let Some(r) = self.rows.get(self.cursor) else { return ListOut::Keep };
                match r.id {
                    0x7fff_ffff => return ListOut::Close,
                    id if id >= 0 => return ListOut::Pick(id as i16, r.key as i16),
                    id if id <= -2 && (-2 - id) < 0x40 && self.level == Level::Top => {
                        self.go(Level::Region((-2 - id) as usize))
                    }
                    id => {
                        self.from = Some((self.level.clone(), self.cursor));
                        self.go(Level::Zone((-id - 2) as u16));
                    }
                }
            }
            egui::Key::Escape => match (self.from.take(), &self.level) {
                (Some((up, at)), Level::Zone(_)) => {
                    self.go(up);
                    self.cursor = at;
                }
                (_, Level::Top) => return ListOut::Close,
                _ => self.go(Level::Top),
            },
            _ => {}
        }
        crate::client::scrollbar::keep_in_view(&mut self.scroll, self.cursor, ROWS);
        ListOut::Keep
    }

    fn go(&mut self, level: Level) {
        (self.level, self.built, self.cursor, self.scroll) = (level, false, 0, 0);
    }

    /// FUN_101fcae0 / FUN_101fc600 / FUN_101fc7e0 / FUN_101fc9d0.
    fn build(&mut self, w: &World, gu: &GameUi) {
        let regions = gu.map_regions();
        let place = |z: u16| {
            w.names
                .as_ref()
                .and_then(|n| crate::dat::names::NameSource::place(&**n, z as u32))
                .filter(|s| !s.is_empty())
        };
        // FUN_101fc970: the zone's records with a map number: (key, number).
        let floors = |z: u16| {
            regions
                .iter()
                .filter(|r| r.zone == z as i16 && r.num != 0)
                .map(|r| (r.key as i32, r.num))
                .collect::<Vec<_>>()
        };
        // FUN_101f7340(zone, key, 1): a record whose key item is held (maps that need none are not listed); key -2 the
        // zone's first record with a map number.
        let has = |z: u16, key: i32| {
            let r = if key == -2 {
                regions.iter().find(|r| r.zone == z as i16 && r.num != 0)
            } else {
                regions.iter().find(|r| r.zone == z as i16 && r.key as i32 == key)
            };
            r.is_some_and(|r| matches!(r.need(), Need::Item(_)) && owned(w, r))
        };
        let mut rows = Vec::new();
        match self.level {
            Level::Top => {
                rows.push(Row {
                    id: w.pos_zone as i32,
                    key: -1,
                    a: gu.item_msg(9, 0x23).unwrap_or_default(),
                    b: gu.item_msg(9, 0x24).unwrap_or_default(),
                    number: false,
                });
                for (i, (name, zones)) in gu.region_groups().iter().enumerate() {
                    let n = zones.iter().filter(|&&z| has(z, -2) && place(z).is_some()).count();
                    if n > 0 {
                        rows.push(Row {
                            id: -2 - i as i32,
                            key: -1,
                            a: gu.item_msg(0x13, *name as usize).unwrap_or_default(),
                            b: n.to_string(),
                            number: true,
                        });
                    }
                }
            }
            Level::Region(i) => {
                for &z in gu.region_groups().get(i).map_or(&[][..], |g| &g.1[..]) {
                    let Some(name) = place(z).filter(|_| has(z, -2)) else { continue };
                    let owned: Vec<i32> = floors(z).into_iter().map(|f| f.0).filter(|&k| has(z, k)).collect();
                    let (id, key) = if owned.len() >= 2 {
                        (-(z as i32) - 2, 0)
                    } else {
                        (z as i32, owned.last().copied().unwrap_or(0))
                    };
                    let area = crate::dat::font::decode_sjis(&gu.area_name(z));
                    rows.push(Row { id, key, a: name, b: area, number: false });
                }
            }
            Level::Zone(z) => {
                for (key, num) in floors(z).into_iter().filter(|f| has(z, f.0)) {
                    if let Some(name) = place(z) {
                        rows.push(Row { id: z as i32, key, a: name, b: num.to_string(), number: true });
                    }
                }
            }
        }
        (self.rows, self.built) = (rows, true);
    }

    /// The list window (16, 208, stacked on the log: registry flag 0x1000) with 5 rows (FUN_101f6a60 with maplist's
    /// cells; MODLOG "Search: arealist"): rows from (+3, +4) 16 apart; names in the game font at y + 2; numbers "[n]"
    /// (type 9) in the shape font right-aligned at the column's end - 2, y + 4; small texts "(s)" (type 0x21) in the
    /// shape font at y + 2; the windowps 75 cursor bar at (+0, +4), nothing recoloured.
    fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) {
        const WHITE: u32 = 0x8080_8080;
        let sel = (!self.rows.is_empty()).then(|| (self.cursor - self.scroll + 1) as u16);
        if gu.lobby_window_off("menu    maplist", sw, sh, (0.0, 0.0), sel, true, &[]).is_none() {
            return;
        }
        let Some((x, y)) = gu.window_xy("menu    maplist", sw, sh) else { return };
        if let Some([fx, fy, fw, fh]) = gu.window_rect("menu    maplist", sw, sh) {
            gu.scroll_bar(fx + fw - 8.0, fy, fh, self.rows.len(), ROWS, self.scroll);
            gu.bar_site(super::scrollbar::Owner::MapList, fx + fw - 8.0, fy, fh, self.rows.len(), ROWS, self.scroll);
        }
        for (k, r) in self.rows.iter().enumerate().skip(self.scroll).take(ROWS) {
            let (rx, ry) = (x + 3.0, y + 4.0 + 16.0 * (k - self.scroll) as f32);
            gu.text(&r.a, rx + COLS[0], ry + 2.0, 0, WHITE.to_le_bytes());
            if r.number {
                gu.shape_text(&format!("[{}]", r.b), rx + COLS[2] - 2.0, ry + 4.0, WHITE, true);
            } else {
                gu.shape_text(&format!("({})", r.b), rx + COLS[1], ry + 2.0, WHITE, false);
            }
        }
        if !self.rows.is_empty() {
            gu.sprite("menu    windowps", 75, x, y + 4.0 + 16.0 * (self.cursor - self.scroll) as f32, WHITE);
        }
    }
}

/// Builds Change Map's rows when its level changed (needs the records, names and key items); loads the points and
/// follows the preview menu's cursor option.
pub(super) fn refresh(w: &mut World, gu: &GameUi) {
    if let Some(mut m) = w.map.take() {
        m.rebuild_scan(w);
        m.load_points(|| w.item_info.as_ref().map(|c| c.points()), w.point_masks);
        let follow = super::event::option_point(w).map(|sel| m.follow(sel));
        if follow == Some(Follow::Reopen) {
            // FUN_101fa420(0) on the next update.
            w.game.set(0x8a, m.zoom_cfg());
            m.begin_close();
        }
        w.map = Some(m);
        match follow {
            Some(Follow::NotOwned(zone)) => not_owned(w, zone),
            Some(Follow::NoMap) => super::textcmd::refuse(w, 0x62),
            _ => {}
        }
    }
    let Some(mut l) = w.map.as_mut().and_then(|m| m.maplist.take()) else { return };
    if !l.built {
        l.build(w, gu);
    }
    if let Some(m) = w.map.as_mut() {
        m.maplist = Some(l);
    }
}

/// Event map requests, applied where the map records are: FUN_100b2570 opens, FUN_100b0dd0 closes, FUN_101fb070 views.
pub enum Cmd {
    /// /map (FUN_101f7810(0)): mode 0 as from the main menu.
    Menu,
    /// Change Map's reopen (FUN_101f79e0): mode 5 for (zone, key), key -1 the sub-key under the player.
    Other {
        zone: i16,
        key: i16,
        return_menu: Option<usize>,
    },
    Open {
        zone: i16,
        key: i16,
        show_player: bool,
    },
    Close,
    View {
        x: f32,
        y: f32,
        zf: f32,
        speed: f32,
        flags: i32,
    },
    /// Ops 0x8b / 0xb8: set 1's slot at world (x, z) named.
    Mark {
        zone: i16,
        key: i16,
        slot: usize,
        x: f32,
        z: f32,
        name: String,
    },
    /// Ops 0xa8 / 0xb9: set 1's slot emptied; `file` (sub 0) loads, edits and saves the zone's .MRK.
    Unmark {
        zone: i16,
        key: i16,
        slot: usize,
        file: bool,
    },
    /// Op 0xd4 sub 0 (FUN_100b2570(6, zone, -1)) and its reopen for another zone (FUN_101f79e0, no arrow); `point_y`
    /// is the cursor point's height.
    Preview {
        zone: i16,
        key: i16,
        arrow: bool,
        point_y: f32,
    },
    /// FUN_100b2a60 -> FUN_101fa420(1): an answer closes the event map (any event mode), no reopen.
    CloseEvent,
    /// s2c 0x0A0 (FUN_100f07b0): a party member's position.
    Party {
        id: u32,
        zone: u16,
        pos: [f32; 3],
    },
    /// FUN_101fb140: a Primer page's image (mode 7); nothing while another map is up (FUN_101f9a20).
    Primer {
        first: u32,
        count: u8,
        at: u8,
    },
}

/// FUN_101f8650: group 10 #7 "You do not possess a map of %s." with the place name (FUN_10129e80, mode 0x79).
fn not_owned(w: &mut World, zone: u16) {
    let Some(bt) = w.battle.clone() else { return };
    let place =
        w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, zone as u32)).unwrap_or_default();
    let line = bt.group10(7).unwrap_or_default().replacen("%s", &place, 1);
    let colour = bt.colour_in(0x79, &w.game);
    w.chat.push_colour(line, colour);
}

/// FUN_101f8a40(zone, key, 0) -> FUN_101f8b80: a reply's map pixel (16-bit) through the record of exactly this zone
/// and sub-key. With no such record the index is -1 and FUN_101f8bd0 reads the scale byte 14 bytes before the table
/// (0x103832af, 0x10 in 30261102_0).
fn reply_px(regions: &[Region], zone: u16, key: u8, x: f32, z: f32) -> (i16, i16) {
    let (px, py) = px_of(Region::find_key(regions, zone, key).map_or(0x10, |r| r.scale), x, z);
    (px as i16, py as i16)
}

/// FUN_101ee650 / FUN_101f8b80: world (x, z) to raw map pixels for a record's scale byte, k = 2560 / |scale|; C
/// truncation.
fn px_of(scale: i8, x: f32, z: f32) -> (i32, i32) {
    let s = scale.unsigned_abs() as f32;
    if s == 0.0 {
        return (0, 0);
    }
    let k = 2560.0 / s;
    ((x * (1.0 / k) * 512.0 + 0.5) as i32, -((z * (1.0 / k) * 512.0 + 0.5) as i32))
}

/// Where the alliance's other members start among the 20 member slots (FUN_101fa720: slot 0x13 + the party window's
/// count for our party, which counts us).
pub(super) fn rest_from(w: &World) -> usize {
    w.party.slots().counts[0].max(1) - 1
}

/// The character's USER folder, where the .MRK files sit with the macros (FUN_101c3d00).
fn marks_dir(w: &World) -> Option<std::path::PathBuf> {
    w.macros.as_ref().map(|m| m.dir().to_path_buf())
}

/// FUN_101f78e0 / FUN_101f79e0: a negative key is the sub-key (FUN_101811d0) under the player or the loaded area's
/// origin, for any zone (neither tests it).
fn sub_key(w: &World, key: i16, under_player: bool) -> u8 {
    if key >= 0 {
        return key as u8;
    }
    let at = if under_player { w.pos } else { glam::Vec3::ZERO };
    w.ground.as_ref().map_or(0, |g| g.map_key(at))
}

/// Ops 0x8b / 0xb8 / 0xa8 / 0xb9 through the file (FUN_100b2570(4, ...), the change, FUN_100b2a60). The mode-4 load
/// (FUN_101f9a50) refuses on a lift (0x61) or with no record (0x62); the change then lands in the open map instead.
fn through_file(
    w: &mut World,
    regions: &[Region],
    zone: i16,
    key: i16,
    on_map: impl FnOnce(&mut MapWin),
    on_file: impl FnOnce(&mut super::mapmark::MarkFile, &Region),
) {
    let has_record = Region::find_key(regions, zone as u16, sub_key(w, key, false))
        .or_else(|| Region::find(regions, zone as u16))
        .is_some();
    let refused = if w.riding { Some(0x61) } else { (!has_record).then_some(0x62) };
    if let Some(id) = refused {
        super::textcmd::refuse(w, id);
    }
    // FUN_100b2a60 resets DAT_104896e6: no event map is left open.
    w.map_open = None;
    if let Some(m) = w.map.as_mut().filter(|m| !m.closing()) {
        if refused.is_some() {
            on_map(m);
        }
        if m.close_for_file(refused.is_some()) {
            w.map = None;
            w.scan.party_busy = false;
        } else {
            w.game.set(0x8a, m.zoom_cfg());
        }
    }
    if refused.is_none() {
        edit_file(w, regions, zone, key, on_file);
    }
}

/// FUN_100b2570(4, ...) / FUN_100b2a60: the zone's .MRK edited without a window, then saved.
fn edit_file(
    w: &World,
    regions: &[Region],
    zone: i16,
    key: i16,
    f: impl FnOnce(&mut super::mapmark::MarkFile, &Region),
) {
    // Named by the key asked for (+0x102) even when the zone's key-0 record stands in.
    let key = sub_key(w, key, false);
    let (Some(dir), Some(region)) =
        (marks_dir(w), Region::find_key(regions, zone as u16, key).or_else(|| Region::find(regions, zone as u16)))
    else {
        return;
    };
    let mut file = super::mapmark::MarkFile::load(&dir, region.zone as u16, key, w.my_id);
    f(&mut file, &region);
    file.save(&dir, region.zone as u16, key, w.my_id);
}

pub(super) fn apply(w: &mut World, regions: &[Region], c: Cmd, sw: f32, sh: f32) {
    match c {
        Cmd::Party { id, zone, pos } => {
            // FUN_100f07b0 -> FUN_101f94d0, once a map has asked: a listed member in our zone on the asked sub-map.
            let Some(key) = w.scan.party_key else { return };
            w.scan.party_busy = false;
            if w.map.is_none()
                || zone != w.pos_zone
                || w.ground.as_ref().map_or(0, |g| g.map_key(glam::Vec3::from(pos))) != key
            {
                return;
            }
            let own = w.party.own_flags;
            let Some((name, party)) =
                w.party.members().skip(1).find(|m| m.id == id).map(|m| (m.name.clone(), (m.flags ^ own) & 3 == 0))
            else {
                return;
            };
            let (px, from) = (reply_px(regions, zone, key, pos[0], pos[2]), rest_from(w));
            if let Some(m) = w.map.as_mut() {
                m.party_pos(&name, px, party, from);
            }
        }
        // FUN_101f9a50: not while riding a lift (group 7 0x61).
        Cmd::Menu | Cmd::Other { .. } if w.riding => super::textcmd::refuse(w, 0x61),
        Cmd::Menu => match MapWin::open(w, regions, sw, sh) {
            Some(m) => {
                w.map = Some(m);
                super::battlefield::map_set_up(w);
            }
            None => super::textcmd::refuse(w, 0x62),
        },
        Cmd::Other { zone, key, return_menu } => {
            let here = zone == w.pos_zone as i16;
            let k = sub_key(w, key, true);
            if let Some(region) =
                Region::find_key(regions, zone as u16, k).or_else(|| Region::find(regions, zone as u16))
            {
                let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
                let mut m = MapWin::new(
                    region,
                    owned(w, &region),
                    w.index.clone(),
                    (w.pos.x, w.pos.z),
                    facing,
                    sw,
                    sh,
                    Mode::Other,
                    w.game.get(0x8a),
                    here,
                );
                m.key = k;
                m.attach(w, regions);
                m.return_menu = return_menu;
                w.map = Some(m);
                super::battlefield::map_set_up(w);
            }
        }
        Cmd::Open { zone, key, show_player } => {
            if w.map.as_ref().is_some_and(|m| m.shows_event(zone, key)) {
                return;
            }
            match MapWin::open_event(w, regions, zone, key, show_player, sw, sh) {
                Some(m) => {
                    w.map = Some(m);
                    super::battlefield::map_set_up(w);
                }
                None => super::textcmd::refuse(w, 0x62),
            }
        }
        Cmd::Close => {
            if let Some(m) = w.map.as_mut() {
                w.game.set(0x8a, m.zoom_cfg());
                m.begin_close();
            }
        }
        // With an event map open: reopened for that zone if needed, the marker set and scrolled to; else the file.
        Cmd::Mark { zone, key, slot, x, z, name } => {
            if w.map.as_ref().is_some_and(|m| m.event_opened() && !m.closing()) {
                apply(w, regions, Cmd::Open { zone, key, show_player: false }, sw, sh);
                if let Some(m) = w.map.as_mut() {
                    m.mark(slot, x, z, &name, true);
                }
            } else {
                let on_map = name.clone();
                through_file(
                    w,
                    regions,
                    zone,
                    key,
                    |m| m.mark(slot, x, z, &on_map, false),
                    |f, r| {
                        if let Some(s) = f.sets[1].get_mut(slot) {
                            let (px, py) = r.px(x, z);
                            *s = super::mapmark::Slot { px: (px as i16, py as i16), name, valid: true };
                        }
                    },
                );
            }
        }
        // FUN_100b9780 / FUN_100b9820: sub 1 clears the slot in whatever map is loaded, sub 0 always in the file.
        Cmd::Unmark { zone, key, slot, file } => {
            if !file {
                if let Some(m) = w.map.as_mut() {
                    m.unmark(slot);
                }
            } else {
                through_file(
                    w,
                    regions,
                    zone,
                    key,
                    |m| m.unmark(slot),
                    |f, _| {
                        if let Some(s) = f.sets[1].get_mut(slot) {
                            (s.valid, s.name) = (false, String::new());
                        }
                    },
                );
            }
        }
        Cmd::Preview { .. } if w.riding => super::textcmd::refuse(w, 0x61),
        // FUN_100b2570 opens unless the preview is up; another event map is replaced. The reopen (no arrow) needs a
        // record for that zone and sub-map.
        Cmd::Preview { zone, key, arrow, point_y } => {
            if w.map.as_ref().is_some_and(|m| m.mode == Mode::Preview && !m.closing())
                || (!arrow && Region::find_key(regions, zone as u16, key as u8).is_none())
            {
                return;
            }
            w.map = Some(MapWin::open_preview(w, regions, zone, key, arrow, sw, sh, point_y));
            super::battlefield::map_set_up(w);
        }
        Cmd::CloseEvent => {
            if let Some(m) = w.map.as_mut().filter(|m| !matches!(m.mode, Mode::Menu | Mode::Other)) {
                w.game.set(0x8a, m.zoom_cfg());
                m.reopen = None;
                m.begin_close();
            }
        }
        Cmd::Primer { first, count, at } => {
            if w.map.as_ref().is_none_or(|m| m.closing()) {
                w.map = Some(MapWin::primer(w.index.clone(), first, count, at, sw, sh, w.game.get(0x8a)));
                super::battlefield::map_set_up(w);
            }
        }
        // Opening a map resets the scripted view, so one sent with no map open is dropped.
        Cmd::View { x, y, zf, speed, flags } => {
            if let Some(m) = w.map.as_mut() {
                m.view_to(x, y, zf, speed, flags);
            }
        }
    }
}

/// The Markers window for the modern layer: the shown set (0 yours, 1 event, 2 party, 3 alliance) and which can show,
/// the rows (icon, name), the rows' cursor while they have the keys, mapv2 (its row, cursor 1..3, the row's marker
/// placed) and whether a marker is being placed.
pub struct MarkersLook {
    pub set: usize,
    pub sets: [bool; 4],
    pub rows: Vec<(Option<usize>, String)>,
    pub cursor: Option<usize>,
    pub v2: Option<(usize, u16, bool)>,
    pub placing: bool,
}

/// The marker sets' icons (keytops3), for the modern set strip.
pub const SET_ICONS: [usize; 4] = SET_SPRITE;

/// A click on Markers row `k` (0-based): mapv2 on another row closes first, then the row takes Enter. Nothing while a
/// marker is placed or named.
pub fn marker_click(w: &mut World, k: usize) {
    let Some(l) = w.map.as_ref().filter(|m| !m.locked()).and_then(|m| m.list.as_ref()) else { return };
    if l.placing.is_some() || l.rename.is_some() || k >= 10 || l.v2.is_some_and(|v| v.0 == k) {
        return;
    }
    if l.v2.is_some() {
        key(w, egui::Key::Escape);
    }
    if let Some(l) = w.map.as_mut().and_then(|m| m.list.as_mut()) {
        l.cursor = k as u16 + 1;
    }
    key(w, egui::Key::Enter);
}

/// A click on mapv2's row `c` (1..3): the cursor there, then Enter.
pub fn marker_menu_click(w: &mut World, c: u16) {
    let Some(v) = w.map.as_mut().and_then(|m| m.list.as_mut()).and_then(|l| l.v2.as_mut()) else { return };
    if (1..=3).contains(&c) {
        v.1 = c;
        key(w, egui::Key::Enter);
    }
}

/// Change Map's list for the modern layer: per row its name and its value ("[n]" maps, "(area)"), and the cursor.
pub struct MapListLook {
    pub rows: Vec<(String, String)>,
    pub cursor: usize,
}

impl MapWin {
    pub fn maplist_look(&self) -> Option<MapListLook> {
        let l = self.maplist.as_ref()?;
        let rows = l
            .rows
            .iter()
            .map(|r| (r.a.clone(), if r.number { format!("[{}]", r.b) } else { format!("({})", r.b) }))
            .collect();
        Some(MapListLook { rows, cursor: l.cursor })
    }
}

/// Wide Scan's list for the modern layer: per row its keytops3 icon and name, the cursor, and mapscan's cursor with
/// Track's state (off for a player) while it is open.
pub struct ScanLook {
    pub rows: Vec<(usize, String)>,
    pub cursor: usize,
    pub mapscan: Option<(u16, bool)>,
}

impl MapWin {
    pub fn scan_look(&self) -> Option<ScanLook> {
        let s = self.scan.as_ref().filter(|s| !s.hidden && s.narrow.is_none())?;
        let rows = s.rows.iter().map(|r| (r.sprite, r.name.clone())).collect();
        let track = s.rows.get(s.cursor).is_some_and(|r| r.trackable);
        Some(ScanLook { rows, cursor: s.cursor, mapscan: s.mapscan.map(|c| (c, track)) })
    }

    /// The mouse's cursor move onto mapscan row `c` (1 Track, 2 Remote Scan). True when it moved.
    pub fn set_mapscan(&mut self, c: u16) -> bool {
        let Some(s) = self.scan.as_mut().filter(|s| s.mapscan.is_some_and(|m| m != c) && (1..=2).contains(&c)) else {
            return false;
        };
        s.mapscan = Some(c);
        true
    }
}

/// A click on Wide Scan's row `k`: mapscan closes if it was open, the cursor goes there, then Enter (the map scrolls
/// to it, mapscan opens).
pub fn scan_click(w: &mut World, k: usize) {
    let Some(s) = w.map.as_mut().and_then(|m| m.scan.as_mut()).filter(|s| k < s.rows.len() && s.narrow.is_none())
    else {
        return;
    };
    s.mapscan = None;
    s.cursor = k;
    crate::client::scrollbar::keep_in_view(&mut s.scroll, k, SCAN_ROWS);
    key(w, egui::Key::Enter);
}

/// A click on Change Map's row `k`: the cursor there, then Enter.
pub fn maplist_click(w: &mut World, k: usize) {
    let Some(l) = w.map.as_mut().and_then(|m| m.maplist.as_mut()).filter(|l| k < l.rows.len()) else { return };
    l.cursor = k;
    crate::client::scrollbar::keep_in_view(&mut l.scroll, k, ROWS);
    key(w, egui::Key::Enter);
}

/// A click on the set strip: that set shown, as Left / Right would show it.
pub fn marker_set_click(w: &mut World, s: usize) {
    let Some(m) = w.map.as_mut() else { return };
    let busy = m.list.as_ref().is_none_or(|l| l.v2.is_some() || l.placing.is_some() || l.rename.is_some());
    if busy || m.locked() || s > 3 || m.set as usize == s || !m.set_shown(s as u8) {
        return;
    }
    m.want_party();
    m.set = s as u8;
    m.sounds.push(crate::audio::Ui::Cursor as u32);
}

/// A key for the map's windows: MapWin::key, and on its false (Escape, a pick) the zoom into config 0x8a and the
/// fade-out (FUN_101fa420).
pub fn key(w: &mut World, key: egui::Key) {
    if let Some(m) = w.map.as_mut()
        && !m.key(key)
    {
        w.game.set(0x8a, m.zoom_cfg());
        m.begin_close();
    }
}

/// The region's key item is held, or it needs none (FUN_101ee560).
/// FUN_101f7600(1): the grid position after the clock. Empty without ground or in a room (zone flag 0x100); "(?-?)"
/// with no record for exactly the sub-key under the player, or one that needs no map; no key-item test.
pub fn grid_text(w: &World, regions: &[Region]) -> String {
    let Some(g) = w.ground.as_ref().filter(|_| !w.room_flag) else { return String::new() };
    match Region::find_key(regions, w.pos_zone, g.map_key(w.pos)).filter(|r| r.need() != Need::Nothing) {
        Some(r) => {
            let (px, py) = r.px(w.pos.x, w.pos.z);
            r.grid(px, py)
        }
        None => "(?-?)".into(),
    }
}

/// The map record for where we stand and how the map window shows it (FUN_101f8360); none where no map exists
/// ("There is no map for this area.").
pub(super) fn record(w: &World, regions: &[Region]) -> Option<(Region, Show)> {
    let r = Region::here(regions, w)?;
    Some((
        r,
        if r.need() == Need::Nothing {
            Show::NoMapNeeded
        } else if owned(w, &r) {
            Show::Map
        } else {
            Show::NotOwned
        },
    ))
}

fn owned(w: &World, region: &Region) -> bool {
    match region.need() {
        Need::Item(id) => w.opts.all_maps || w.has_key_item(id),
        _ => true,
    }
}

/// Extra (not retail; the Windower / Ashita map overlay): the zone map, translucent, in a box over the scene and
/// under the windows, toggled with Ctrl / Alt + O while the player keeps moving. Wheel zooms, left-drag pans.
/// It shows what the map window would: nothing without the map's key item.
pub struct Overlay {
    pub zone: u16,
    region: Option<Region>,
    /// What it shows of the record (HUD maps; the minimap shows only an owned map): the map, retail's NO MAP image
    /// over the map's place while we don't own it, or a record needing no map's image (no marker).
    show: Show,
    hud: bool,
    texture: Option<String>,
    shade: Option<Shade>,
    rx: Option<Receiver<ShadedLoaded>>,
    /// The decoded image (size, pixels), kept for the modern layer's texture.
    image: Option<([usize; 2], Arc<Vec<u8>>)>,
}

/// What the modern layer draws a map from: the image's name, size and pixels, the map's region, and whether our
/// marker goes on it.
pub struct MapImage {
    pub name: String,
    pub size: [usize; 2],
    pub rgba: Arc<Vec<u8>>,
    pub region: Region,
    pub marker: bool,
}

/// A record's showing, as the map window shows it (FUN_101f8360): the map; NO MAP while not owned, the markers where
/// they would be on the map; or a record needing no map (0x180): its image, no marker.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Show {
    Map,
    NotOwned,
    NoMapNeeded,
}

impl Overlay {
    /// The minimap's: an owned map only.
    pub fn open(w: &World, regions: &[Region]) -> Self {
        let region = Region::here(regions, w).filter(|r| r.need() != Need::Nothing && owned(w, r));
        Self::new(w.pos_zone, region, w.index.clone())
    }

    /// A HUD map's: whatever the map window shows for where we stand (`record`).
    pub fn open_hud(w: &World, regions: &[Region]) -> Self {
        let rec = record(w, regions);
        let file = rec.map(|(r, show)| if show == Show::NotOwned { NO_MAP } else { r.file_id() });
        let rx = file.zip(w.index.clone()).map(|(f, index)| load_shaded(index, f));
        Self {
            zone: w.pos_zone,
            region: rec.map(|r| r.0),
            show: rec.map_or(Show::Map, |r| r.1),
            hud: true,
            texture: None,
            shade: None,
            rx,
            image: None,
        }
    }

    pub fn new(zone: u16, region: Option<Region>, index: Option<Arc<DatIndex>>) -> Self {
        let rx = region.zip(index).map(|(r, index)| load_shaded(index, r.file_id()));
        Self { zone, region, show: Show::Map, hud: false, texture: None, shade: None, rx, image: None }
    }

    /// The player crossed onto another page of the zone (Windurst Waters' north and south), or now owns the map (its
    /// key item came in): that map.
    pub fn follow(&mut self, w: &World, regions: &[Region]) {
        if self.hud {
            let rec = record(w, regions);
            if rec.map(|r| r.0) != self.region || rec.map(|r| r.1) != self.region.is_some().then_some(self.show) {
                *self = Self::open_hud(w, regions);
            }
            return;
        }
        let here = Region::here(regions, w).filter(|r| r.need() != Need::Nothing && owned(w, r));
        if here != self.region {
            *self = Self::new(w.pos_zone, here, w.index.clone());
        }
    }

    /// A map to show.
    pub fn has_map(&self) -> bool {
        self.region.is_some()
    }

    /// Waits for the image (offscreen shots).
    pub fn wait_image(&mut self, gu: &mut GameUi) {
        if let Some(Some((name, iw, ih, rgba, shade))) = self.rx.take().and_then(|r| r.recv().ok()) {
            self.image = Some(([iw as usize, ih as usize], Arc::new(rgba)));
            (self.texture, self.shade) = (Some(name), Some(shade));
        }
        self.upload(gu);
    }

    /// The image once decoded, kept.
    fn receive(&mut self) {
        if let Some(Ok(Some((name, iw, ih, rgba, shade)))) = self.rx.as_ref().map(|r| r.try_recv()) {
            self.image = Some(([iw as usize, ih as usize], Arc::new(rgba)));
            (self.texture, self.shade) = (Some(name), Some(shade));
            self.rx = None;
        }
    }

    /// The kept image into the game UI's textures, once.
    fn upload(&self, gu: &mut GameUi) {
        if let (Some(name), Some(([w, h], rgba))) = (&self.texture, &self.image)
            && !gu.has_image(name)
        {
            gu.image(name, *w as u32, *h as u32, rgba.to_vec());
        }
    }

    /// The modern layer's map: the image once decoded, and the region.
    pub fn modern_image(&mut self) -> Option<MapImage> {
        self.receive();
        let (Some(name), Some((size, rgba)), Some(region)) = (&self.texture, &self.image, self.region) else {
            return None;
        };
        Some(MapImage {
            name: name.clone(),
            size: *size,
            rgba: rgba.clone(),
            region,
            marker: self.show != Show::NoMapNeeded,
        })
    }

    /// The image once it is decoded, in the game UI's textures.
    fn poll(&mut self, gu: &mut GameUi) {
        self.receive();
        self.upload(gu);
    }

    /// The image's luminance under map pixel (px, py), 0..1 (dark until the image is in).
    fn shade_at(&self, region: &Region, px: f32, py: f32) -> f32 {
        self.shade.as_ref().map_or(0.0, |s| s.at(px - region.ox as f32, py - region.oy as f32))
    }

    /// A window onto the image (a Lua HUD map in the classic UI): `rect`, centred on world (x, z), `span` map pixels
    /// across, at `alpha`; clipped to the rect. `me`: our marker at world (x, z) facing, as the classic map draws it.
    pub fn draw_window(
        &mut self,
        gu: &mut GameUi,
        rect: [f32; 4],
        centre: (f32, f32),
        span: f32,
        alpha: f32,
        me: Option<((f32, f32), f32)>,
    ) {
        self.poll(gu);
        let (Some(region), Some(t)) = (self.region, self.texture.clone()) else { return };
        let (z, s) = (rect[2] / span.max(1.0), super::minimap::px_per_unit(&region));
        let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        let (px, py) = (centre.0 * s, -centre.1 * s);
        gu.clip(Some(rect));
        gu.image_rect(
            &t,
            cx + (region.ox as f32 - px) * z,
            cy + (region.oy as f32 - py) * z,
            512.0 * z,
            512.0 * z,
            [1.0, 1.0, 1.0, alpha],
        );
        if let Some(((x, wz), facing)) = me.filter(|_| self.show != Show::NoMapNeeded) {
            let (mx, my) = (x * s, -wz * s);
            marker(gu, cx + (mx - px) * z, cy + (my - py) * z, facing, self.shade_at(&region, mx, my) * alpha);
        }
        gu.clip(None);
    }

    /// Extra, the minimap (client/minimap.rs): the style frame at `rect`, inside it `span` map pixels across centred on
    /// `pos`, the dots (world x, z, colour, the target drawn larger) and our marker, coloured as the overlay's at
    /// opacity `mix` (user, 2026-10-25: the two markers alike).
    pub fn draw_mini(
        &mut self,
        gu: &mut GameUi,
        rect: [f32; 4],
        pos: (f32, f32),
        facing: f32,
        span: f32,
        dots: &[(f32, f32, u32, bool)],
        mix: f32,
    ) {
        self.poll(gu);
        let (Some(region), Some(t)) = (self.region, self.texture.clone()) else { return };
        const INSET: f32 = 5.0;
        gu.frame(rect[0], rect[1], rect[2], rect[3]);
        let inner = [rect[0] + INSET, rect[1] + INSET, rect[2] - 2.0 * INSET, rect[3] - 2.0 * INSET];
        let (z, s) = (inner[2] / span, super::minimap::px_per_unit(&region));
        let (cx, cy) = (inner[0] + inner[2] / 2.0, inner[1] + inner[3] / 2.0);
        let (px, py) = (pos.0 * s, -pos.1 * s);
        gu.clip(Some(inner));
        gu.image_rect(
            &t,
            cx + (region.ox as f32 - px) * z,
            cy + (region.oy as f32 - py) * z,
            512.0 * z,
            512.0 * z,
            [1.0; 4],
        );
        // Squares in the radar's colours (at half strength: the monsters' 0x808080 stays grey on the parchment, where the
        // compass's added dot fades out) on a dark border, the target larger on a red one.
        let rgb = |c: u32| [0, 8, 16].map(|sh| ((c >> (16 - sh)) & 0xff) as f32 / 255.0);
        for &(x, wz, colour, target) in dots {
            let (dx, dy) = ((x * s - px) * z, (-wz * s - py) * z);
            if dx.abs() > inner[2] / 2.0 + 8.0 || dy.abs() > inner[3] / 2.0 + 8.0 {
                continue;
            }
            let (d, border) = if target { (6.0, [0.85, 0.1, 0.1, 1.0]) } else { (4.0, [0.0, 0.0, 0.0, 0.85]) };
            let [r, g, b] = rgb(colour);
            gu.rect(cx + dx - d / 2.0 - 1.0, cy + dy - d / 2.0 - 1.0, d + 2.0, d + 2.0, border);
            gu.rect(cx + dx - d / 2.0, cy + dy - d / 2.0, d, d, [r, g, b, 1.0]);
        }
        let shade = self.shade_at(&region, px, py) * mix;
        marker(gu, cx, cy, facing, shade);
        gu.clip(None);
    }
}

/// The HUD map's and the minimap's marker: the map window's (keytops3 159) at its size. Its added orange glow fades
/// out on light parchment, so over light ground (`shade`, the map's luminance under it) it turns into a solid dark
/// red, alpha blended by the cells' alpha.
fn marker(gu: &mut GameUi, x: f32, y: f32, facing: f32, shade: f32) {
    let (s, c) = facing.sin_cos();
    let m = [c, -s, s, c];
    let t = ((shade - 0.35) / 0.25).clamp(0.0, 1.0);
    let glow = (0x80 as f32 * (1.0 - t)) as u32;
    if glow > 0 {
        gu.sprite_affine("menu    keytops3", MARKER, x, y, m, 0x8000_0000 | glow << 16 | glow << 8 | glow);
    }
    let a = (0x80 as f32 * t) as u32;
    if a > 0 {
        gu.sprite_affine_blend("menu    keytops3", MARKER, x, y, m, a << 24 | 0x60_0800, 0x44);
    }
}

/// The map DAT's first image.
fn decode(index: &DatIndex, file: u32) -> Loaded {
    let bytes = index.read(file).ok()?;
    let ui = crate::dat::ui::UiFile::parse(&bytes).map_err(|e| log::warn!("map {file}: {e:#}")).ok()?;
    let (name, img) = ui.images.into_iter().next()?;
    let mut rgba = img.rgba;
    if crate::render::MAP_COMPRESSED.load(std::sync::atomic::Ordering::Relaxed) && category_10(&bytes) {
        rgba = through_dxt3(&rgba, img.width, img.height);
    }
    Some((format!("map:{file}:{name}"), img.width, img.height, rgba))
}

/// FUN_10038c40's category 10 (0019): the file's first IMG is flag 0xB1 with the u32 at payload 57 low nibble 10 (the
/// 209 palettized `menumap` images; the DXT ones are used as stored).
fn category_10(bytes: &[u8]) -> bool {
    crate::dat::chunks(bytes)
        .ok()
        .and_then(|c| c.into_iter().find(|c| c.kind == 0x20))
        .is_some_and(|c| c.data.len() > 61 && c.data[0] == 0xB1 && c.data[57] & 0xf == 10)
}

/// 0019 Compressed: the image as a DXT3 surface holds it (retail converts to A8R8G8B8 and loads it into DXT3).
fn through_dxt3(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let f = texpresso::Format::Bc2;
    let (w, h) = (w as usize, h as usize);
    let mut data = vec![0u8; f.compressed_size(w, h)];
    f.compress(
        rgba,
        w,
        h,
        texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, ..Default::default() },
        &mut data,
    );
    let mut out = vec![0u8; w * h * 4];
    f.decompress(&data, w, h, &mut out);
    out
}

/// Reads and decodes the map DAT off the UI thread: its first image.
pub(super) fn load(index: Arc<DatIndex>, file: u32) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(decode(&index, file));
    });
    rx
}

/// `load` with the image's shade, also worked out off the UI thread.
fn load_shaded(index: Arc<DatIndex>, file: u32) -> Receiver<ShadedLoaded> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let got = decode(&index, file).map(|(name, w, h, rgba)| {
            let shade = Shade::new(w, h, &rgba);
            (name, w, h, rgba, shade)
        });
        let _ = tx.send(got);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windurst Waters (238): two pages and no key 0, north key 1 (file 5501), south key 2 (5502), told apart by the
    /// collision under the player.
    #[test]
    fn windurst_waters_pages() {
        let cfg = crate::config::Config::load();
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let regs = regions(&dll);
        let pages: Vec<(i8, u32)> = regs.iter().filter(|r| r.zone == 238).map(|r| (r.key, r.file_id())).collect();
        assert_eq!(pages, [(1, 5501), (2, 5502)]);
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let g = crate::client::ground::Ground::build(
            &crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(238)).unwrap()).unwrap(),
        );
        let key = |x: f32, z: f32| g.map_key(glam::Vec3::new(x, g.snap(x, z, 0.0).unwrap(), z));
        assert_eq!((key(0.0, 100.0), key(0.0, -150.0)), (1, 2));
    }

    /// 0019: the palettized menumap images are category 10, and going through DXT3 changes their pixels.
    #[test]
    fn map_category_10_through_dxt3() {
        let cfg = crate::config::Config::load();
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let Ok(index) = crate::dat::DatIndex::open_stock(&cfg.ffxi_dir()) else { return };
        let mut files: Vec<u32> = regions(&dll).iter().map(|r| r.file_id()).collect();
        files.sort_unstable();
        files.dedup();
        let cat10: Vec<u32> = files.iter().copied().filter(|&f| index.read(f).is_ok_and(|b| category_10(&b))).collect();
        assert!(!cat10.is_empty(), "no category-10 map among {}", files.len());
        let ui = crate::dat::ui::UiFile::parse(&index.read(cat10[0]).unwrap()).unwrap();
        let img = ui.images.values().next().unwrap();
        let lossy = through_dxt3(&img.rgba, img.width, img.height);
        assert_eq!(lossy.len(), img.rgba.len());
        assert!(lossy != img.rgba);
    }

    /// Mode 6 on Southern San d'Oria: nothing before the masks; then its 7 points (4 Home Points, 3 Beginner) as
    /// markers, the followed one last and named, unregistered Home Points blinking; following hides, shows, reopens.
    #[test]
    fn preview_points() {
        let cfg = crate::config::Config::load();
        let (Ok(dll), Ok(index)) =
            (std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())), DatIndex::open(&cfg.ffxi_dir()))
        else {
            return;
        };
        let Ok(items) = crate::dat::items::Items::load(&index, crate::dat::items::Lang::En) else { return };
        let table = Arc::new(tpoints::load(&items));
        let region = Region::find(&regions(&dll), 230).unwrap();
        let open =
            |owned: bool| MapWin::new(region, owned, None, (0.0, 0.0), 0.0, 1024.0, 896.0, Mode::Preview, 0, false);
        let mut m = open(true);
        assert_eq!((m.outs.clone(), m.fade.0, m.help_zone()), (vec![Out::AskPoints], 255.0, None));
        m.load_points(|| Some(table.clone()), None);
        assert_eq!((m.follow((0, 0)), m.fade.0), (Follow::Keep, 255.0));
        // Home Point bits 0 and 1 registered.
        let mut masks = [0u8; 64];
        masks[0] = 3;
        m.load_points(|| Some(table.clone()), Some(masks));
        let marks = |m: &MapWin| m.points.as_ref().unwrap().marks.clone();
        assert_eq!(marks(&m).len(), 7);
        assert!(marks(&m).iter().all(|k| k.name.is_empty()));

        assert_eq!(m.follow((0, 0)), Follow::Keep);
        let k = marks(&m);
        assert_eq!((k[6].name, k[6].sprite, k[6].blink), ("Home Point", 516, false));
        let blinks = |s: usize| k[..6].iter().filter(|k| k.sprite == s).map(|k| k.blink).collect::<Vec<_>>();
        assert_eq!((blinks(516), blinks(524)), (vec![false, true, true], vec![false; 3]));
        let (x, y) = region.px(-85.468, -66.454);
        assert_eq!(m.scroll_to, Some((x as f32 + 30.0, y as f32 + 30.0)));
        assert_eq!((m.fade.0, m.help_zone()), (0.0, Some(230)));
        // The same pair again changes nothing.
        m.scroll_to = None;
        assert_eq!((m.follow((0, 0)), m.scroll_to), (Follow::Keep, None));
        // An option without a point hides the map.
        assert_eq!(m.follow((0, 0xffff)), Follow::Keep);
        assert_eq!((m.fade.0, m.help_zone()), (255.0, None));
        // Northern San d'Oria's Home Point #4 (id 3): this map closes and the preview reopens there.
        assert_eq!(m.follow((0, 3)), Follow::Reopen);
        assert_eq!(m.reopen, Some((231, 0)));

        // Without the map: hidden, told once per zone.
        let mut m = open(false);
        m.load_points(|| Some(table.clone()), Some(masks));
        assert_eq!(m.follow((0, 0)), Follow::NotOwned(230));
        assert_eq!((m.follow((0, 97)), m.fade.0), (Follow::Keep, 255.0));
    }

    /// Sub-keys from MZB object flags, and Pso'Xja (9): records for keys 1..16, none for key 0.
    #[test]
    fn sub_keys() {
        use crate::client::ground::map_key_of;
        assert_eq!([0xe, 0x3e, 0x400_0004, 0x400_003c, 0x800_0004].map(map_key_of), [1, 7, 8, 15, 16]);
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&crate::config::Config::load().ffxi_dir())) else { return };
        let regs = regions(&dll);
        assert!(Region::find_key(&regs, 9, 0).is_none());
        assert!((1..=16).all(|k| Region::find_key(&regs, 9, k).is_some()));
    }

    /// Change Map's groups: 50, the first San d'Oria's four zones with name 0.
    #[test]
    fn region_groups_found() {
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&crate::config::Config::load().ffxi_dir())) else { return };
        let g = region_groups(&dll);
        assert_eq!(g.len(), 50);
        assert_eq!(g[0], (0, vec![230, 231, 232, 233]));
    }

    /// Jugner Forest (104): record key 0, scale 1, key item 1 (0x181), file 5 -> 5317, origin (-264, -256).
    #[test]
    fn jugner_region() {
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&crate::config::Config::load().ffxi_dir())) else { return };
        let regs = regions(&dll);
        assert!(regs.len() > 700, "{}", regs.len());
        let r = Region::find(&regs, 104).unwrap();
        assert_eq!((r.scale, r.file_id(), r.ox, r.oy, r.need()), (1, 5317, -264, -256, Need::Item(0x181)));
        // x / 5 and -z / 5 at scale 1; G-9 is 6 cells right and 8 down of (ox + 16, oy + 16).
        assert_eq!(r.px(-200.0, -1050.0), (-39, 209));
        assert_eq!(r.grid(-39, 209), "(G-15)");
        assert_eq!(r.grid(-264 + 16 + 6 * 32, -256 + 16 + 8 * 32), "(G-9)");
    }
}

#[cfg(test)]
mod marker_tests {
    use super::*;
    use egui::Key as K;

    fn win(mode: Mode) -> MapWin {
        let region = Region { zone: 230, key: 0, num: 0, flags: 0, scale: 1, ki: -1, file: 0, ox: -256, oy: -256 };
        let mut m = MapWin::new(region, true, None, (0.0, 0.0), 0.0, 1024.0, 896.0, mode, 1000, true);
        m.here = true;
        m.list = Some(List { cursor: 1, ..Default::default() });
        m
    }

    /// FUN_101fa720: our party from slot 20, the alliance's others from 0x13 + our party's count, the first position
    /// of a name kept, set 3 only once a member lands in slots 30..39; FUN_101fd4d0's row icons.
    #[test]
    fn members_take_fixed_slots() {
        let mut m = win(Mode::Menu);
        assert!(!m.set_shown(2) && !m.set_shown(3));
        m.rest_from = 2;
        m.add_member("Ally", (9.0, 9.0), false);
        m.add_member("Mate", (1.0, 1.0), true);
        m.add_member("Ally", (50.0, 50.0), false);
        m.add_member("Mate", (60.0, 60.0), true);
        let at = |m: &MapWin, i: usize| m.members[i].as_ref().map(|k| (k.name.clone(), k.px, k.party));
        assert_eq!(at(&m, 0), Some(("Mate".into(), (1.0, 1.0), true)));
        assert_eq!((at(&m, 1), at(&m, 2)), (None, Some(("Ally".into(), (9.0, 9.0), false))));
        assert_eq!((m.party_count, m.rest_count, m.set_shown(2), m.set_shown(3)), (1, 1, true, false));
        // A party member past the alliance's first slot takes the next free one.
        m.add_member("Mate2", (2.0, 2.0), true);
        m.add_member("Mate3", (3.0, 3.0), true);
        assert_eq!(at(&m, 3).unwrap().0, "Mate3");
        m.set = 2;
        let l = m.list.clone().unwrap();
        let icons: Vec<_> = (0..5).map(|i| m.marker_row(&l, i).0).collect();
        assert_eq!(icons, [Some(154), Some(154), Some(154), Some(155), None]);
        assert_eq!(m.slot(2, 2).unwrap().2, SET_SPRITE[3]);
        // Seven more of the alliance: slots 24..29 and then 30, which opens set 3.
        for i in 0..7 {
            assert!(!m.set3);
            m.add_member(&format!("Other{i}"), (0.0, 0.0), false);
        }
        assert!(m.set_shown(3));
        assert_eq!((m.slot(3, 0).unwrap().1, m.slot(3, 1)), (Some("Other6"), None));
        // Twenty slots: a full table drops the rest.
        for i in 0..20 {
            m.add_member(&format!("More{i}"), (0.0, 0.0), false);
        }
        assert_eq!(m.rest_count, 17);
    }

    /// FUN_101fda10 / FUN_101fa3b0 / FUN_101fa1a0 / FUN_101fd450: every mapv2 choice closes it; Remove keeps the name
    /// and the pixel; View on an empty slot is refused; the marker goes under the cursor sprite's centre; Escape in
    /// the name entry keeps what was typed.
    #[test]
    fn menu_remove_place_and_name() {
        use crate::audio::Ui;
        let mut m = win(Mode::Menu);
        m.marks.sets[0][0] = super::super::mapmark::Slot { px: (5, 6), name: "Gate".into(), valid: true };
        let v2 = |m: &MapWin| m.list.as_ref().unwrap().v2;
        m.key(K::Enter);
        assert_eq!(v2(&m), Some((0, 1)));
        m.key(K::Enter);
        assert_eq!((v2(&m), m.scroll_to), (None, Some((5.0, 6.0))));
        m.key(K::Enter);
        m.key(K::ArrowDown);
        m.key(K::Enter);
        let s = m.marks.sets[0][0].clone();
        assert_eq!((v2(&m), s.valid, s.name.as_str(), s.px, m.dirty), (None, false, "Gate", (5, 6), true));
        let l = m.list.clone().unwrap();
        assert_eq!(m.marker_row(&l, 0), (Some(ROW_ICON[0]), "Gate".to_string()));
        // View on the empty slot: the beep, mapv2 stays.
        m.key(K::Enter);
        m.sounds.clear();
        m.key(K::Enter);
        assert_eq!((v2(&m), m.sounds.as_slice()), (Some((0, 1)), &[Ui::Error as u32][..]));
        // Place: mapv2 closes, the cursor starts at the cursor point.
        m.key(K::ArrowDown);
        m.key(K::Enter);
        assert_eq!((v2(&m), m.list.as_ref().unwrap().placing), (None, Some((0, m.point))));
        // The width as read from a (resized) cursor sprite.
        m.cursor_w = 20.0;
        let half = (10.0 / m.zoom + 0.5).trunc();
        let want = (
            (m.point.0 / m.zoom + m.view.0 + 0.5).trunc() + half,
            (m.point.1 / m.zoom + m.view.1 + 0.5).trunc() + half,
        );
        m.key(K::Enter);
        let s = m.marks.sets[0][0].clone();
        assert_eq!((s.valid, s.px, s.name.as_str()), (true, (want.0 as i16, want.1 as i16), "Gate"));
        assert!(m.typing());
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        m.text_events(&[egui::Event::Text("2".into()), key(K::Escape)]);
        assert_eq!((m.typing(), m.marks.sets[0][0].name.as_str()), (false, "Gate2"));
    }

    /// FUN_101f8a40 / FUN_101f8b80 / FUN_101f8bd0: a reply's pixel goes through the record of exactly (zone, sub-key);
    /// with none, the byte before the table gives scale 16. FUN_101f94d0 places on whatever map is open.
    #[test]
    fn reply_pixel_and_placing() {
        let rec = |key, scale| Region { zone: 230, key, num: 0, flags: 0, scale, ki: -1, file: 0, ox: 0, oy: 0 };
        let regs = [rec(0, 1), rec(2, 4)];
        assert_eq!(reply_px(&regs, 230, 2, 100.0, 50.0), (80, -40));
        assert_eq!(reply_px(&regs, 230, 0, 100.0, 50.0), (20, -10));
        assert_eq!(reply_px(&regs, 230, 1, 100.0, 50.0), (320, -160));
        assert_eq!(reply_px(&regs, 231, 2, 100.0, 50.0), (320, -160));
        let mut m = win(Mode::Other);
        m.here = false;
        m.party_pos("Mate", (80, 40), true, 3);
        m.party_pos("Ally", (7, 8), false, 3);
        m.party_pos("Mate", (1, 1), true, 3);
        let at = |i: usize| m.members[i].as_ref().map(|k| (k.name.as_str(), k.px));
        assert_eq!((at(0), at(3)), (Some(("Mate", (80.0, 40.0))), Some(("Ally", (7.0, 8.0)))));
    }

    /// FUN_101f7810 / FUN_101fc380(0): the menu map starts on set 2 and stays there with a member placed, so Markers
    /// asks for the positions as it opens (FUN_101f9270); with nobody it moves on to set 0.
    #[test]
    fn menu_map_starts_on_the_party_set() {
        let mut m = win(Mode::Menu);
        (m.list, m.set) = (None, 2);
        m.step_set(0);
        m.key(K::Enter);
        assert_eq!((m.set, m.outs.contains(&Out::AskParty)), (0, false));

        let mut m = win(Mode::Menu);
        (m.list, m.set) = (None, 2);
        m.add_member("Mate", (1.0, 1.0), true);
        m.step_set(0);
        m.key(K::Enter);
        assert_eq!((m.set, m.outs.iter().filter(|o| **o == Out::AskParty).count()), (2, 1));
    }

    /// FUN_100b2570: ops 0x89 / 0x8d / 0xc8 reopen unless the mode, the zone and a given key match the last op's,
    /// whatever record stands in for that key.
    #[test]
    fn event_map_same_map_test() {
        let mut m = win(Mode::Event);
        (m.op_key, m.key) = (2, 2);
        assert!(m.shows_event(230, 2) && m.shows_event(230, -1));
        assert!(!m.shows_event(230, 0) && !m.shows_event(231, 2));
        m.op_key = -1;
        assert!(!m.shows_event(230, 2) && m.shows_event(230, -1));
        assert!(!win(Mode::Preview).shows_event(230, -1) && !win(Mode::Menu).shows_event(230, -1));
        m.begin_close();
        assert!(!m.shows_event(230, -1));
    }

    /// FUN_101f9a50 mode 4 clears +0xa44 before FUN_101fa420(1), the only save: a file edit drops the open map's
    /// unsaved marks unless the load was refused. An event map goes at once, the menu map fades.
    #[test]
    fn file_edit_drops_unsaved_marks() {
        let dir = std::env::temp_dir().join(format!("vana-mrk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = super::super::mapmark::MarkFile::path(&dir, 230, 0);
        for (mode, kept, gone) in [(Mode::Event, false, true), (Mode::Menu, false, false), (Mode::Event, true, true)] {
            let mut m = win(mode);
            m.mark_dir = Some(dir.clone());
            m.mark(0, 1.0, 1.0, "Gate", false);
            assert_eq!(m.close_for_file(kept), gone);
            assert_eq!((file.exists(), m.closing(), m.dirty), (kept, !gone, false));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// FUN_101fa680 / FUN_101fd680: Left / Right ask for the positions once, whatever the set; an event map's mapv3
    /// (+0x10a) takes no keys and has no cursor, and never has set 0.
    #[test]
    fn asking_and_the_event_lock() {
        let mut m = win(Mode::Menu);
        m.key(K::ArrowRight);
        m.key(K::ArrowLeft);
        assert_eq!(m.outs.iter().filter(|o| **o == Out::AskParty).count(), 1);
        assert!(m.markers_look().unwrap().cursor.is_some());

        let mut m = win(Mode::Event);
        m.set = 1;
        m.outs.clear();
        for k in [K::ArrowDown, K::ArrowRight, K::Enter, K::Escape] {
            assert!(m.key(k));
        }
        let l = m.list.as_ref().unwrap();
        assert_eq!((l.cursor, m.set, m.outs.len()), (1, 1, 0));
        let look = m.markers_look().unwrap();
        assert_eq!((look.cursor, look.sets[0]), (None, false));
    }
}
