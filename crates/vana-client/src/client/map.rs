//! The map window from the main menu (FUN_101ee8f0 -> FUN_101f0af0 mode 0): mapframe (the map, base draw
//! FUN_101e6ca0, draw FUN_101f1a40 / FUN_101f1a60) and map0 (Markers / Wide Scan / Change Map).

use super::World;
use super::game_ui::GameUi;
use crate::dat::DatIndex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

// Region table (dat::tables): 14-byte records ended by a record with scale (+5) 0.
const REGIONS_MAX: usize = 765;
/// The "no map" image m_000_00 (FUN_101ef690's fallback).
const NO_MAP: u32 = 0x1592;
/// keytops3 sprites: the player marker (table 0x10375f6c[2]).
const MARKER: usize = 159;

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
    let Some(at) = crate::dat::tables::of(ffximain).map_err(|e| log::warn!("map regions: {e:#}")).ok().map(|t| t.regions) else { return Vec::new() };
    let Some(b) = crate::dat::battle::va_bytes(ffximain, at, REGIONS_MAX * 14) else { return Vec::new() };
    let s16 = |r: &[u8], o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
    b.chunks(14)
        .map(|r| Region { zone: s16(r, 0), key: r[2] as i8, num: r[3], flags: r[4], scale: r[5] as i8, ki: r[6] as i8, file: s16(r, 8), ox: s16(r, 10), oy: s16(r, 12) })
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
            while let Some(z) = va(p + 2 * zones.len() as u32, 2).map(|b| u16::from_le_bytes([b[0], b[1]])).filter(|&z| z != 0) {
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
        let key = w.ground.as_ref().map_or(0, |g| g.map_key(w.pos));
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
            k => Need::Item(k as u16 + match self.flags & 0xf0 { 0x10 => 0x73f, 0x20 => 0x8fd, _ => 0x180 }),
        }
    }

    /// FUN_101ee650: world (x, z) to raw map pixels, k = 2560 / |scale|; C truncation.
    pub fn px(&self, x: f32, z: f32) -> (i32, i32) {
        let s = self.scale.unsigned_abs() as f32;
        if s == 0.0 {
            return (0, 0);
        }
        let k = 2560.0 / s;
        ((x * (1.0 / k) * 512.0 + 0.5) as i32, -((z * (1.0 / k) * 512.0 + 0.5) as i32))
    }

    /// FUN_101f1a60: "(%c-%d)" from 32-pixel cells 16 in from the origin.
    pub fn grid(&self, px: i32, py: i32) -> String {
        let col = (b'A' as i32 + (px - self.ox as i32 - 16) / 32) as u8 as char;
        format!("({col}-{})", 1 + (py - self.oy as i32 - 16) / 32)
    }
}

type Loaded = Option<(String, u32, u32, Vec<u8>)>;

/// FUN_101f9a50's modes: 0 the main menu or /map, 1 an event map, 2 an event illustration (zone <= -100), 3 a region
/// map (zone -1..-99), 5 Change Map's other zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Menu,
    Event,
    Illustration,
    RegionMap,
    Other,
}

impl Mode {
    /// FUN_101f78e0: an event's mode from the zone's sign.
    pub fn of_event_zone(zone: i16) -> Mode {
        match zone {
            ..=-100 => Mode::Illustration,
            ..0 => Mode::RegionMap,
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
    /// The name entry for a slot (FUN_101fbd90, 15 characters).
    rename: Option<(usize, String)>,
}

/// Marker sprites of the four sets (list 0x1038214c indices 3..6): yours, event, party, alliance.
const SET_SPRITE: [usize; 4] = [150, 152, 154, 155];
/// mapv3's row icons (table 0x10382178).
const ROW_ICON: [usize; 8] = [151, 150, 153, 152, 154, 154, 155, 155];

pub struct MapWin {
    pub region: Region,
    need: Need,
    owned: bool,
    /// The .MRK sets 0 / 1, where they live, and whether to save them on close (+0xa44).
    marks: super::mapmark::MarkFile,
    mark_dir: Option<std::path::PathBuf>,
    char_id: u32,
    dirty: bool,
    /// Sets 2 / 3: members in this zone and sub-map with a model (FUN_101f7db0): name, map pixel, alliance.
    party: Vec<(String, (f32, f32), bool)>,
    /// Marker alpha per set and slot (FUN_101eef90), 0..0x80.
    mark_alpha: [[f32; 10]; 4],
    /// Sets 2 / 3: the one member whose label shows (picked in the list).
    label_only: Option<usize>,
    list: Option<List>,
    /// Change Map's list (maplist) while open.
    pub maplist: Option<MapList>,
    /// Change Map's pick (+0x110 / +0x112): reopened in mode 5 once this map has closed.
    pub reopen: Option<(i16, i16)>,
    /// The marker set shown (+0x106).
    set: u8,
    /// Interface sounds for the caller to play.
    pub sounds: Vec<u32>,
    /// Party positions (c2s 0x0D2) asked for this map (+0xa45), and an ask waiting to be sent.
    party_asked: bool,
    /// Requests for the caller: packets and prompts.
    pub outs: Vec<Out>,
    /// Wide Scan: map0 item 2 enabled (FUN_101f7c00: s2c 0x119 seen, our own zone's map), its list, results kept for
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
}

impl MapWin {
    /// Opens the map of the zone (FUN_101f9a50 mode 0). None when the zone has no record (group 7 0x62).
    pub fn open(w: &World, regions: &[Region], sw: f32, sh: f32) -> Option<Self> {
        let region = Region::here(regions, w)?;
        let owned = owned(w, &region);
        log::info!("map: zone {} owned {owned}", w.pos_zone);
        let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
        let mut m = Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, Mode::Menu, w.game.get(0x8a), true);
        m.attach(w);
        Some(m)
    }

    /// FUN_101f80b0 (the .MRK) and FUN_101f7db0 (party markers) at open, then FUN_101fad70(0): a set with entries.
    fn attach(&mut self, w: &World) {
        self.mark_dir = marks_dir(w);
        self.char_id = w.my_id;
        if let Some(d) = &self.mark_dir {
            self.marks = super::mapmark::MarkFile::load(d, self.region.zone as u16, self.region.key as u8, w.my_id);
        }
        self.scan_ok = w.recasts_seen && self.region.zone == w.pos_zone as i16;
        self.tracking = w.scan.tracking().is_some();
        self.tracked = w.scan.tracking().map(|t| {
            let (x, y) = self.region.px(t.pos[0], t.pos[2]);
            (x as f32, y as f32)
        });
        let own = w.party.own_flags;
        for m in w.party.members().skip(1).filter(|m| m.zone == w.pos_zone && m.away == 0) {
            let Some(e) = w.entities.get(&m.id) else { continue };
            if w.ground.as_ref().is_some_and(|g| g.map_key(e.pos) != self.region.key as u8) {
                continue;
            }
            let (px, py) = self.region.px(e.pos.x, e.pos.z);
            self.party.push((m.name.clone(), (px as f32, py as f32), (m.flags ^ own) & 3 != 0));
        }
        self.step_set(0);
    }

    /// FUN_101fad70(d): the set by d, wrapping, past sets with nothing to show (0 needs the map, 2 any member, 3 an
    /// alliance member; 1 always).
    fn step_set(&mut self, d: i32) {
        let has = |m: &Self, s: u8| match s {
            0 => m.owned && m.need != Need::Nothing,
            2 => !m.party.is_empty(),
            3 => m.party.iter().any(|p| p.2),
            _ => true,
        };
        let step = if d < 0 { 3 } else { 1 };
        if d != 0 {
            self.set = (self.set + step) % 4;
        }
        for _ in 0..4 {
            if has(self, self.set) {
                break;
            }
            self.set = (self.set + step) % 4;
        }
        self.label_only = None;
    }

    /// The map's zone (for c2s 0x0D2).
    pub fn zone(&self) -> u16 {
        self.region.zone as u16
    }

    /// FUN_101f9070: c2s 0x0D2 for the map's zone, once per map, when a party set is about to show.
    fn want_party(&mut self) {
        if self.set > 1 && !self.party_asked {
            self.party_asked = true;
            self.outs.push(Out::AskParty);
        }
    }

    /// s2c 0x0A0 (FUN_101f9110): a member's position on this map, replacing what the actors gave; `same_key` is
    /// whether the sub-key under it matches the map's.
    pub fn party_pos(&mut self, name: &str, x: f32, z: f32, alliance: bool, same_key: bool) {
        if !same_key {
            return;
        }
        let (px, py) = self.region.px(x, z);
        self.party.retain(|p| p.0 != name);
        self.party.push((name.into(), (px as f32, py as f32), alliance));
    }

    /// Set s, slot i: its map pixel, label and sprite, when it holds a marker.
    fn slot(&self, s: usize, i: usize) -> Option<((f32, f32), Option<&str>, usize)> {
        match s {
            0 | 1 => self.marks.sets[s].get(i).filter(|m| m.valid).map(|m| ((m.px.0 as f32, m.px.1 as f32), Some(m.name.as_str()), SET_SPRITE[s])),
            2 => {
                let p = self.party.iter().filter(|p| !p.2).chain(self.party.iter().filter(|p| p.2)).nth(i)?;
                Some((p.1, Some(p.0.as_str()), SET_SPRITE[if p.2 { 3 } else { 2 }]))
            }
            _ => self.party.iter().filter(|p| p.2).nth(i).map(|p| (p.1, Some(p.0.as_str()), SET_SPRITE[3])),
        }
    }

    /// Op 0x8b / 0xb8 on an open event map (FUN_101f8d50 / FUN_101f8de0 / FUN_101f8e30): set 1's slot at the world
    /// (x, z), named, then scrolled to.
    fn mark(&mut self, slot: usize, x: f32, z: f32, name: &str) {
        let Some(m) = self.marks.sets[1].get_mut(slot) else { return };
        let (px, py) = self.region.px(x, z);
        *m = super::mapmark::Slot { px: (px as i16, py as i16), name: name.into(), valid: true };
        self.dirty = true;
        self.scroll_to_px((px as f32, py as f32));
    }

    /// Op 0xa8 / 0xb9 (FUN_101f8da0): set 1's slot emptied, its name cleared.
    fn unmark(&mut self, slot: usize) {
        if let Some(m) = self.marks.sets[1].get_mut(slot) {
            *m = Default::default();
            self.dirty = true;
        }
    }

    /// Offscreen shots: three of your markers around the player, mapv3 open on them at full alpha.
    pub fn shot_markers(&mut self) {
        for (i, (dx, dy, n)) in [(-40.0, -20.0, "Gate"), (30.0, 10.0, "Chocobo"), (5.0, 45.0, "Home point")].into_iter().enumerate() {
            self.marks.sets[0][i] = super::mapmark::Slot { px: ((self.home.0 + dx) as i16, (self.home.1 + dy) as i16), name: n.into(), valid: true };
            self.mark_alpha[0][i] = 128.0;
        }
        self.set = 0;
        self.list = Some(List { cursor: 2, ..Default::default() });
    }

    /// The marker name entry is open: it takes the keys and characters.
    pub fn typing(&self) -> bool {
        self.list.as_ref().is_some_and(|l| l.rename.is_some())
    }

    /// The name entry's keys: up to 15 characters, Backspace; Enter keeps it, Escape discards it (message 6).
    pub fn text_events(&mut self, events: &[egui::Event]) {
        let Some(l) = self.list.as_mut() else { return };
        let Some((slot, text)) = l.rename.as_mut() else { return };
        let slot = *slot;
        let mut done = None;
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
                egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => done = Some(true),
                egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => done = Some(false),
                _ => {}
            }
        }
        if let Some(keep) = done {
            let text = std::mem::take(text);
            l.rename = None;
            if keep {
                self.marks.sets[0][slot].name = text;
                self.dirty = true;
            }
        }
    }

    /// An event's map (FUN_100b2570 -> FUN_101f78e0): zone and sub-key (negative: the one under the player when in
    /// that zone *(inf)*, else 0); the arrow only with the op's flag.
    pub fn open_event(w: &World, regions: &[Region], zone: i16, key: i16, show_player: bool, sw: f32, sh: f32) -> Option<Self> {
        let mode = Mode::of_event_zone(zone);
        let key = if key >= 0 { key as u8 } else if zone == w.pos_zone as i16 { w.ground.as_ref().map_or(0, |g| g.map_key(w.pos)) } else { 0 };
        let region = Region::find_key(regions, zone as u16, key).or_else(|| Region::find(regions, zone as u16))?;
        let owned = owned(w, &region);
        let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
        log::info!("map: event map zone {zone} key {key} mode {mode:?} owned {owned}");
        let mut m = Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, mode, w.game.get(0x8a), show_player);
        m.attach(w);
        // Mode 1 opens mapv3 on set 1 instead of map0 (map records only).
        if mode == Mode::Event && m.need != Need::Nothing {
            m.set = 1;
            m.list = Some(List { cursor: 1, ..Default::default() });
        }
        Some(m)
    }

    /// FUN_101f8360 and the callers' zoom: the image, zoom W / 512 x config 0x8a / 1000 in mode 0 (W / 512 for a no-map
    /// record), x 1.75 in mode 1, W / 512 otherwise; fade 0x80 -> 0 at 8 a frame; then the player under the cursor
    /// point (FUN_101f8ea0, FUN_101ef990).
    #[allow(clippy::too_many_arguments)]
    pub fn new(region: Region, owned: bool, index: Option<Arc<DatIndex>>, pos: (f32, f32), facing: f32, sw: f32, sh: f32, mode: Mode, zoom_cfg: u32, show_player: bool) -> Self {
        let need = region.need();
        let file = if owned { region.file_id() } else { NO_MAP };
        let (base, shift) = if owned { ((region.ox as f32, region.oy as f32), (0.0, 0.0)) } else { ((0.0, 0.0), (region.ox as f32, region.oy as f32)) };
        let fit = sw / 512.0;
        let zoom = match mode {
            _ if need == Need::Nothing => fit,
            Mode::Menu => fit * zoom_cfg.clamp(1000, 4000) as f32 * 0.001,
            Mode::Event => fit * 1.75,
            _ => fit,
        };
        let point = ((sw * 200.0 / 512.0).trunc(), (sh / 2.0).trunc());
        let (px, py) = region.px(pos.0, pos.1);
        let home = (px as f32, py as f32);
        let rx = index.map(|index| load(index, file));
        let mut m = Self {
            region,
            need,
            owned,
            marks: Default::default(),
            mark_dir: None,
            char_id: 0,
            dirty: false,
            party: Vec::new(),
            mark_alpha: [[0.0; 10]; 4],
            label_only: None,
            list: None,
            maplist: None,
            reopen: None,
            set: 0,
            sounds: Vec::new(),
            party_asked: false,
            outs: Vec::new(),
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
            keys: mode != Mode::Illustration,
            show_player: show_player && need != Need::Nothing,
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
            fade: (128.0, 0.0, 8.0),
            closing: None,
            scripted: None,
            screen: (sw, sh),
            texture: None,
            rx,
            alpha: 0.0,
            rising: true,
            acc: 0.0,
        };
        if m.show_player {
            // As decompiled: screen pixels taken from map pixels, then clamped and eased.
            m.view = (home.0 - point.0, home.1 - point.1);
            m.scroll_to_px(home);
        }
        m.clamp();
        m
    }

    /// Waits for the image, skips the opening animations and shows the marker at full alpha (offscreen shots).
    pub fn wait_image(&mut self, gu: &mut GameUi) {
        (self.alpha, self.rising) = (128.0, false);
        self.fade = (0.0, 0.0, 8.0);
        if let Some(to) = self.scroll_to.take() {
            self.view = (to.0 - self.point.0 / self.zoom - 0.5 - self.shift.0, to.1 - self.point.1 / self.zoom - 0.5 - self.shift.1);
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
        self.need != Need::Nothing && matches!(self.mode, Mode::Menu | Mode::Other) && self.closing.is_none() && self.list.is_none() && self.maplist.is_none() && self.scan.is_none()
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

    /// FUN_101fb4b0: the rows and their markers from the results (rebuilt every frame while the list shows): PCs
    /// (not ourselves) with their own name, NPCs and monsters named from the zone NPC list by index & 0x3ff; those
    /// without a name or outside the narrowed circle are left out.
    pub(super) fn rebuild_scan(&mut self, w: &World) {
        let Some(s) = self.scan.as_mut().filter(|s| !s.hidden) else { return };
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
                (!name.is_empty() && inside).then_some(ScanRow { index: e.index, px: (px as f32, py as f32), sprite, name, trackable: e.kind != 0 })
            })
            .collect();
        s.cursor = s.cursor.min(s.rows.len().saturating_sub(1));
        s.scroll = s.scroll.min(s.cursor);
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

    /// Slot 20 (FUN_101ee580): fade to 0xff at 0x10 a frame, the window gone when that ends or after 3 s.
    pub fn begin_close(&mut self) {
        if self.closing.is_none() {
            if self.dirty
                && let Some(d) = &self.mark_dir
            {
                self.marks.save(d, self.region.zone as u16, self.region.key as u8, self.char_id);
                self.dirty = false;
            }
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
        if self.list.is_some() {
            self.list_key(key);
            return true;
        }
        if self.scan.is_some() {
            return self.scan_key(key);
        }
        if let Some(l) = self.maplist.as_mut() {
            match l.key(key) {
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
            // FUN_101f7c60 1: mapv3 (asking party positions, c2s 0x0D2, is not done).
            egui::Key::Enter if self.cursor == 1 => {
                self.want_party();
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
            s.scroll = s.scroll.min(s.cursor).max((s.cursor + 1).saturating_sub(SCAN_ROWS));
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
                // FUN_101f8cf0: the marker at the map pixel under the cursor, then the name entry.
                K::Enter => {
                    let px = (cur.0 / self.zoom + self.view.0 + 0.5 + self.shift.0, cur.1 / self.zoom + self.view.1 + 0.5 + self.shift.1);
                    let name = self.marks.sets[0][slot].name.clone();
                    self.marks.sets[0][slot] = super::mapmark::Slot { px: (px.0 as i16, px.1 as i16), name: name.clone(), valid: true };
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
                K::Enter => match *c {
                    1 if valid => {
                        let p = self.marks.sets[0][row].px;
                        self.scroll_to_px((p.0 as f32, p.1 as f32));
                        self.sounds.push(Ui::Select as u32);
                    }
                    2 if valid => {
                        self.marks.sets[0][row] = Default::default();
                        self.dirty = true;
                        self.sounds.push(Ui::Select as u32);
                    }
                    2 => {
                        l.placing = Some((row, self.point));
                        self.sounds.push(Ui::Select as u32);
                    }
                    3 => {
                        l.rename = Some((row, self.marks.sets[0][row].name.clone()));
                        self.sounds.push(Ui::Select as u32);
                    }
                    _ => self.sounds.push(Ui::Error as u32),
                },
                _ => {}
            }
        } else {
            match key {
                K::ArrowUp => l.cursor = if l.cursor <= 1 { 10 } else { l.cursor - 1 },
                K::ArrowDown => l.cursor = if l.cursor >= 10 { 1 } else { l.cursor + 1 },
                // Messages 3 / 4: the next / previous set.
                K::ArrowRight | K::ArrowLeft => {
                    self.want_party();
                    self.step_set(if key == K::ArrowRight { 1 } else { -1 });
                    self.want_party();
                    self.sounds.push(Ui::Cursor as u32);
                }
                K::Escape if self.mode == Mode::Event => {}
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
                        (_, Some((p, ..))) => {
                            self.label_only = Some(row);
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

    /// FUN_101ee040: the view stays over the image (min the image base, max base + the part past the screen).
    fn bounds(&self) -> [f32; 4] {
        let (sw, sh) = self.screen;
        let z = self.zoom;
        [self.base.0, self.base.1, self.base.0 + ((512.0 * z - sw) / z).max(0.0), self.base.1 + ((512.0 * z - sh) / z).max(0.0)]
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
        (self.point.0 / self.zoom + self.view.0 + 0.5 + self.shift.0, self.point.1 / self.zoom + self.view.1 + 0.5 + self.shift.1)
    }

    /// FUN_101ef990: start easing `to` under the cursor point unless it is already there.
    fn scroll_to_px(&mut self, to: (f32, f32)) {
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
        let mut s = Scripted { cur, start: cur, target: [x, y, zf], speed: speed.max(0.0), mode: (flags & 0x7f) as u8, t: 0.0 };
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
                let keys: f32 = -(if input.test(0xa4, HELD) { 1.0 } else if input.test(0xa5, HELD) { -1.0 } else { 0.0 });
                let stick = input.axis(0x7c);
                let a = if stick.abs() > keys.abs() { stick } else { keys };
                if a != 0.0 {
                    self.zoom_by(-a * 0.08 * fast * ticks * 0.5);
                }
            }
        }
        if live && self.scroll_to.is_none() {
            // Under 2 px a retail frame is dead (judged per 2-tick frame, so it holds at any frame rate).
            let dead = |v: f32| if (v * fast).abs() < 2.0 { 0.0 } else { v };
            let step = |d: f32| if d / (self.screen.0 * 0.047619) >= 1.0 { 1.0 } else if d / (self.screen.0 * 0.047619) <= -1.0 { -1.0 } else { 0.0 };
            let (mx, my) = drag.map_or((0.0, 0.0), |d| (step(d.0), step(d.1)));
            let ax = if input.axis(0x79) != 0.0 { input.axis(0x79) } else { mx };
            let ay = if input.axis(0x7a) != 0.0 { input.axis(0x7a) } else { my };
            let v = (dead(ax * 15.0) * fast * ticks * 0.5, dead(ay * 15.0) * fast * ticks * 0.5);
            if v != (0.0, 0.0) {
                self.scroll_by(if self.texture.is_some() { v } else { (-v.0, -v.1) });
            }
            if input.test(0x7e, PRESS) && self.show_player {
                self.scroll_to_px(self.home);
            }
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
            let v = (dead(input.axis(0x9d) * 8.0) * fast * ticks * 0.5, dead(input.axis(0x9e) * 8.0) * fast * ticks * 0.5);
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
        if self.fade.0 == 0.0 {
            // FUN_101eef90: list markers of the shown set fade in 6 a tick, the rest out.
            let open = self.list.is_some();
            for s in 0..4 {
                for i in 0..10 {
                    let on = open && s == self.set as usize && self.slot(s, i).is_some();
                    let a = &mut self.mark_alpha[s][i];
                    *a = (*a + if on { 6.0 } else { -6.0 } * ticks).clamp(0.0, 128.0);
                }
            }
        }
        if self.fade.0 == 0.0 && self.show_player {
            // FUN_101eef90 (flags 0x9f): fade in 6 a tick, then blink between 0 and 0x80 at the same speed.
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
            self.fade.0 = if (target - level).abs() <= step { target } else { level + step * (target - level).signum() };
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
    pub fn draw(&mut self, gu: &mut GameUi, sw: f32, sh: f32) {
        if let Some(Ok(Some((name, iw, ih, rgba)))) = self.rx.as_ref().map(|r| r.try_recv()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
            self.rx = None;
        }
        let z = self.zoom;
        let level = self.fade.0;
        if let Some(t) = &self.texture {
            let a = if level == 0.0 { 160.0 } else { (255.0 - level).min(160.0) } / 128.0;
            gu.image_rect(t, (self.base.0 - self.view.0) * z + 0.5, (self.base.1 - self.view.1) * z + 0.5, 512.0 * z, 512.0 * z, [0.5, 0.5, 0.5, a.min(1.0)]);
        }
        if level == 0.0 {
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
            // FUN_101fa9c0: "(G-9)" at (titlewin x0, titlewin y1 + 3) in fontshp.
            gu.shape_text(&self.region.grid(px as i32, py as i32), 16.0, 49.0, 0x8080_8080, false);
        }
        // The tracked target's ring (keytops3 105, centred by its own bounds).
        if let Some((px, py)) = self.tracked.filter(|_| level == 0.0) {
            gu.sprite("menu    keytops3", 105, (px - self.shift.0 - self.view.0) * z + 0.5, (py - self.shift.1 - self.view.1) * z + 0.5, 0x8080_8080);
        }
        if self.has_menu() {
            let hide: &[u16] = if self.scan_ok { &[] } else { &[2] };
            if let Some(win) = gu.lobby_window_off("menu    map0", sw, sh, (0.0, 0.0), Some(self.cursor), true, hide)
                && !self.scan_ok
                && let Some(&(_, r)) = win.children.iter().find(|c| c.0 == 1)
            {
                // Item 2 disabled: its state-4 sprite (keytops3 20) in place of 3; under the cursor at (+1, +1) in
                // 0x80805020 (FUN_101177d0).
                let on = self.cursor == 2;
                let d = if on { 1.0 } else { 0.0 };
                gu.sprite("menu    keytops3", 20, r[0] + d, r[1] + 16.0 + d, if on { 0x8080_5020 } else { 0x8080_8080 });
            }
        }
        self.draw_scan(gu, sw, sh);
        self.draw_list(gu, sw, sh);
        if let Some(l) = &self.maplist {
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
            gu.sprite("menu    keytops3", sprite, (r.px.0 - self.shift.0 - self.view.0) * z + 0.5, (r.px.1 - self.shift.1 - self.view.1) * z + 0.5, 0x8080_8080);
        }
    }

    /// scanlist (16, 48), 10 rows of 24 with the anc_trac cursor, names fitted like the target window's (width 88 at
    /// +7, +6); mapscan over map0's place; the narrow cursor (anc_scan) at the cursor point.
    fn draw_scan(&self, gu: &mut GameUi, sw: f32, sh: f32) {
        let Some(s) = &self.scan else { return };
        let frame = gu.cursor_frame();
        if let Some(c) = s.narrow {
            let n = gu.lib_len("anc     anc_scan").max(1);
            gu.sprite("anc     anc_scan", frame % n, c.0, c.1, 0x8080_8080);
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
                gu.sprite("menu    keytops3", 300, r[0] + d, r[1] - 16.0 + d, if c == 1 { 0x4040_2810 } else { 0x3080_8080 });
            }
        }
    }

    /// FUN_101ee7b0 for the four sets: the sprite centred on its pixel, alpha << 24 | 0x808080; a label (colour 2,
    /// 0x606070, scaled by the alpha) at (+16, +1) for sets 0 / 1 and the picked member.
    fn draw_markers(&self, gu: &mut GameUi) {
        let z = self.zoom;
        for s in 0..4 {
            for i in 0..10 {
                let a = self.mark_alpha[s][i];
                let Some(((px, py), label, k)) = self.slot(s, i).filter(|_| a > 0.0) else { continue };
                let b = gu.cell_bounds("menu    keytops3", k, 0).unwrap_or([0.0; 4]);
                let x = ((px - self.shift.0 - self.view.0) * z + 0.5).trunc() - ((b[2] - b[0]) / 2.0).trunc();
                let y = ((py - self.shift.1 - self.view.1) * z + 0.5).trunc() - ((b[3] - b[1]) / 2.0).trunc();
                let a = a as u32;
                gu.sprite("menu    keytops3", k, x, y, a << 24 | 0x80_8080);
                let labelled = s < 2 || (s == self.set as usize && self.label_only == Some(i));
                if let Some(t) = label.filter(|t| labelled && !t.is_empty()) {
                    let c = |v: u32| (v * a) >> 7;
                    gu.shape_text(t, x + 16.0, y + 1.0, a << 24 | c(0x60) << 16 | c(0x60) << 8 | c(0x70), false);
                }
            }
        }
    }

    /// mapv3 (FUN_101fbec0): per row the icon at (16, 5 + 16 i) and the name at (0x1f, 7 + 16 i) (font 0x44 taken as
    /// fontshp *(inf)*); mapv2 beside the
    /// row (its place is inferred); the placing cursor (keytops3 151).
    fn draw_list(&self, gu: &mut GameUi, sw: f32, sh: f32) {
        let Some(l) = &self.list else { return };
        if let Some((_, (cx, cy))) = l.placing {
            gu.sprite("menu    keytops3", 151, cx, cy, 0x8080_8080);
        }
        let sel = if l.v2.is_some() || l.placing.is_some() { 0 } else { l.cursor };
        let Some((x, y)) = gu.menu_sel("menu    mapv3", sw, sh, sel) else { return };
        let s = self.set as usize;
        let party = self.party.iter().filter(|p| !p.2).count();
        for i in 0..10 {
            let (ry, slot) = (y + 16.0 * i as f32, self.slot(s, i));
            let icon = match s {
                0 | 1 => Some(ROW_ICON[2 * s + slot.is_some() as usize]),
                2 => slot.map(|_| ROW_ICON[if i < party { 5 } else { 7 }]),
                _ => slot.map(|_| ROW_ICON[7]),
            };
            if let Some(k) = icon {
                gu.sprite("menu    keytops3", k, x + 16.0, ry + 5.0, 0x8080_8080);
            }
            match l.rename.as_ref().filter(|r| r.0 == i) {
                Some((_, t)) => {
                    gu.shape_text(&format!("{t}|"), x + 31.0, ry + 7.0, 0x8080_8080, false);
                }
                None => {
                    if let Some((_, Some(name), _)) = slot {
                        gu.shape_text(name, x + 31.0, ry + 7.0, 0x8080_8080, false);
                    }
                }
            }
        }
        if let Some((row, c)) = l.v2 {
            gu.lobby_window_off("menu    mapv2", sw, sh, (x - 112.0, y + 16.0 * row as f32), Some(c), true, &[]);
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
    fn key(&mut self, key: egui::Key) -> ListOut {
        let n = self.rows.len();
        match key {
            egui::Key::ArrowDown if n > 0 => self.cursor = (self.cursor + 1) % n,
            egui::Key::ArrowUp if n > 0 => self.cursor = (self.cursor + n - 1) % n,
            egui::Key::Enter => {
                let Some(r) = self.rows.get(self.cursor) else { return ListOut::Keep };
                match r.id {
                    0x7fff_ffff => return ListOut::Close,
                    id if id >= 0 => return ListOut::Pick(id as i16, r.key as i16),
                    id if id <= -2 && (-2 - id) < 0x40 && self.level == Level::Top => self.go(Level::Region((-2 - id) as usize)),
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
        self.scroll = self.scroll.min(self.cursor).max((self.cursor + 1).saturating_sub(ROWS));
        ListOut::Keep
    }

    fn go(&mut self, level: Level) {
        (self.level, self.built, self.cursor, self.scroll) = (level, false, 0, 0);
    }

    /// FUN_101fcae0 / FUN_101fc600 / FUN_101fc7e0 / FUN_101fc9d0.
    fn build(&mut self, w: &World, gu: &GameUi) {
        let regions = gu.map_regions();
        let place = |z: u16| w.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, z as u32)).filter(|s| !s.is_empty());
        // FUN_101fc970: the zone's records with a map number: (key, number).
        let floors = |z: u16| regions.iter().filter(|r| r.zone == z as i16 && r.num != 0).map(|r| (r.key as i32, r.num)).collect::<Vec<_>>();
        // FUN_101f7340(zone, key, 1): a record whose key item is held (maps that need none are not listed); key -2 the
        // zone's first record with a map number.
        let has = |z: u16, key: i32| {
            let r = if key == -2 { regions.iter().find(|r| r.zone == z as i16 && r.num != 0) } else { regions.iter().find(|r| r.zone == z as i16 && r.key as i32 == key) };
            r.is_some_and(|r| matches!(r.need(), Need::Item(_)) && owned(w, r))
        };
        let mut rows = Vec::new();
        match self.level {
            Level::Top => {
                rows.push(Row { id: w.pos_zone as i32, key: -1, a: gu.item_msg(9, 0x23).unwrap_or_default(), b: gu.item_msg(9, 0x24).unwrap_or_default(), number: false });
                for (i, (name, zones)) in gu.region_groups().iter().enumerate() {
                    let n = zones.iter().filter(|&&z| has(z, -2) && place(z).is_some()).count();
                    if n > 0 {
                        rows.push(Row { id: -2 - i as i32, key: -1, a: gu.item_msg(0x13, *name as usize).unwrap_or_default(), b: n.to_string(), number: true });
                    }
                }
            }
            Level::Region(i) => {
                for &z in gu.region_groups().get(i).map_or(&[][..], |g| &g.1[..]) {
                    let Some(name) = place(z).filter(|_| has(z, -2)) else { continue };
                    let owned: Vec<i32> = floors(z).into_iter().map(|f| f.0).filter(|&k| has(z, k)).collect();
                    let (id, key) = if owned.len() >= 2 { (-(z as i32) - 2, 0) } else { (z as i32, owned.last().copied().unwrap_or(0)) };
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

    /// The list window (16, 208) with 5 rows: the cursor row's first cell in 0x80803040; numbers right-aligned in
    /// their column (type 9, the sprite font taken as the game font *(inf)*), small text in fontshp.
    fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) {
        let sel = (self.cursor - self.scroll + 1) as u16;
        let Some((x, y)) = gu.menu_sel("menu    maplist", sw, sh, sel) else { return };
        if let Some([fx, fy, fw, fh]) = gu.window_rect("menu    maplist", sw, sh) {
            gu.scroll_bar(fx + fw - 8.0, fy, fh, self.rows.len(), ROWS, self.scroll);
            gu.bar_site(super::scrollbar::Owner::MapList, fx + fw - 8.0, fy, fh, self.rows.len(), ROWS, self.scroll);
        }
        for (k, r) in self.rows.iter().enumerate().skip(self.scroll).take(ROWS) {
            let ry = y + 4.0 + 16.0 * (k - self.scroll) as f32;
            let tint: u32 = if k == self.cursor { 0x8080_3040 } else { 0x8080_8080 };
            gu.text(&r.a, x + COLS[0], ry, 0, tint.to_le_bytes());
            if r.number {
                let w = gu.text_width(&r.b);
                gu.text(&r.b, x + COLS[2] - 2.0 - w, ry, 0, [0x80; 4]);
            } else {
                gu.shape_text(&r.b, x + COLS[1], ry + 2.0, 0x8080_8080, false);
            }
        }
    }
}

/// Builds Change Map's rows when its level changed (needs the records, names and key items).
pub(super) fn refresh(w: &mut World, gu: &GameUi) {
    if let Some(mut m) = w.map.take() {
        m.rebuild_scan(w);
        w.map = Some(m);
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
    Other { zone: i16, key: i16, return_menu: Option<usize> },
    Open { zone: i16, key: i16, show_player: bool },
    Close,
    View { x: f32, y: f32, zf: f32, speed: f32, flags: i32 },
    /// Ops 0x8b / 0xb8: set 1's slot at world (x, z) named.
    Mark { zone: i16, key: i16, slot: usize, x: f32, z: f32, name: String },
    /// Ops 0xa8 / 0xb9: set 1's slot emptied; `file` (sub 0) loads, edits and saves the zone's .MRK.
    Unmark { zone: i16, key: i16, slot: usize, file: bool },
}

/// The character's USER folder, where the .MRK files sit with the macros (FUN_101c3d00).
fn marks_dir(w: &World) -> Option<std::path::PathBuf> {
    w.macros.as_ref().map(|m| m.dir().to_path_buf())
}

/// FUN_100b2570(4, ...) / FUN_100b2a60: the zone's .MRK edited without a window, then saved.
fn edit_file(w: &World, regions: &[Region], zone: i16, key: i16, f: impl FnOnce(&mut super::mapmark::MarkFile, &Region)) {
    let key = if key >= 0 { key as u8 } else if zone == w.pos_zone as i16 { w.ground.as_ref().map_or(0, |g| g.map_key(w.pos)) } else { 0 };
    let (Some(dir), Some(region)) = (marks_dir(w), Region::find_key(regions, zone as u16, key).or_else(|| Region::find(regions, zone as u16))) else { return };
    let mut file = super::mapmark::MarkFile::load(&dir, region.zone as u16, region.key as u8, w.my_id);
    f(&mut file, &region);
    file.save(&dir, region.zone as u16, region.key as u8, w.my_id);
}

pub(super) fn apply(w: &mut World, regions: &[Region], c: Cmd, sw: f32, sh: f32) {
    match c {
        // FUN_101f9a50: not while riding a lift (group 7 0x61).
        Cmd::Menu | Cmd::Other { .. } if w.riding => super::textcmd::refuse(w, 0x61),
        Cmd::Menu => match MapWin::open(w, regions, sw, sh) {
            Some(m) => w.map = Some(m),
            None => super::textcmd::refuse(w, 0x62),
        },
        Cmd::Other { zone, key, return_menu } => {
            let here = zone == w.pos_zone as i16;
            let k = if key >= 0 { key as u8 } else if here { w.ground.as_ref().map_or(0, |g| g.map_key(w.pos)) } else { 0 };
            if let Some(region) = Region::find_key(regions, zone as u16, k).or_else(|| Region::find(regions, zone as u16)) {
                let facing = w.net.me.lock().map_or(0.0, |m| super::event::rad_of(m.dir));
                let mut m = MapWin::new(region, owned(w, &region), w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, Mode::Other, w.game.get(0x8a), here);
                m.attach(w);
                m.return_menu = return_menu;
                w.map = Some(m);
            }
        }
        Cmd::Open { zone, key, show_player } => {
            // FUN_100b2570 reopens only for another map.
            if w.map.as_ref().is_some_and(|m| !m.closing() && m.region.zone == zone && (key < 0 || m.region.key as i16 == key)) {
                return;
            }
            match MapWin::open_event(w, regions, zone, key, show_player, sw, sh) {
                Some(m) => w.map = Some(m),
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
            if w.map.as_ref().is_some_and(|m| m.mode != Mode::Menu && !m.closing()) {
                apply(w, regions, Cmd::Open { zone, key, show_player: false }, sw, sh);
                if let Some(m) = w.map.as_mut() {
                    m.mark(slot, x, z, &name);
                }
            } else {
                edit_file(w, regions, zone, key, |f, r| {
                    if let Some(s) = f.sets[1].get_mut(slot) {
                        let (px, py) = r.px(x, z);
                        *s = super::mapmark::Slot { px: (px as i16, py as i16), name, valid: true };
                    }
                });
            }
        }
        Cmd::Unmark { zone, key, slot, file } => {
            if let Some(m) = w.map.as_mut().filter(|m| m.region.zone == zone) {
                m.unmark(slot);
            } else if file {
                edit_file(w, regions, zone, key, |f, _| {
                    if let Some(s) = f.sets[1].get_mut(slot) {
                        *s = Default::default();
                    }
                });
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

fn owned(w: &World, region: &Region) -> bool {
    match region.need() {
        Need::Item(id) => w.has_key_item(id),
        _ => true,
    }
}

/// Extra (not retail; the Windower / Ashita map overlay): the zone map, translucent, in a box over the scene and
/// under the windows, toggled with Ctrl / Alt + O while the player keeps moving. Wheel zooms, left-drag pans.
/// It shows what the map window would: nothing without the map's key item.
pub struct Overlay {
    pub zone: u16,
    region: Option<Region>,
    texture: Option<String>,
    rx: Option<Receiver<Loaded>>,
    /// Screen pixels per map pixel, and the map pixel at the box's top-left (None until the first draw).
    zoom: f32,
    view: Option<(f32, f32)>,
    /// The box as last drawn (x, y, w, h).
    pub rect: [f32; 4],
}

/// The box's side as a share of the screen's shorter side, and the zoom range over fitting the whole map.
const OVERLAY_SIZE: f32 = 0.7;
const OVERLAY_ZOOM_MAX: f32 = 8.0;

impl Overlay {
    pub fn open(w: &World, regions: &[Region]) -> Self {
        let region = Region::here(regions, w).filter(|r| r.need() != Need::Nothing && owned(w, r));
        Self::new(w.pos_zone, region, w.index.clone())
    }

    pub fn new(zone: u16, region: Option<Region>, index: Option<Arc<DatIndex>>) -> Self {
        let rx = region.zip(index).map(|(r, index)| load(index, r.file_id()));
        Self { zone, region, texture: None, rx, zoom: 1.0, view: None, rect: [0.0; 4] }
    }

    /// A map to show.
    pub fn has_map(&self) -> bool {
        self.region.is_some()
    }

    /// Waits for the image (offscreen shots).
    pub fn wait_image(&mut self, gu: &mut GameUi) {
        if let Some(Some((name, iw, ih, rgba))) = self.rx.take().and_then(|r| r.recv().ok()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
        }
    }

    fn fit(&self) -> f32 {
        self.rect[2] / 512.0
    }

    /// Keeps the image over the whole box.
    fn clamp(&mut self) {
        let (Some(r), Some(v)) = (self.region, self.view.as_mut()) else { return };
        let span = (512.0 - self.rect[2] / self.zoom).max(0.0);
        v.0 = v.0.clamp(r.ox as f32, r.ox as f32 + span);
        v.1 = v.1.clamp(r.oy as f32, r.oy as f32 + span);
    }

    /// Wheel notches (+ in) at screen point `p`: the map pixel under it stays put.
    pub fn zoom_at(&mut self, p: (f32, f32), notches: f32) {
        let Some(v) = self.view else { return };
        let z = (self.zoom * 1.25f32.powf(notches)).clamp(self.fit(), self.fit() * OVERLAY_ZOOM_MAX);
        let (dx, dy) = (p.0 - self.rect[0], p.1 - self.rect[1]);
        self.view = Some((v.0 + dx / self.zoom - dx / z, v.1 + dy / self.zoom - dy / z));
        self.zoom = z;
        self.clamp();
    }

    /// A drag of (dx, dy) screen pixels.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if let Some(v) = self.view.as_mut() {
            *v = (v.0 - dx / self.zoom, v.1 - dy / self.zoom);
        }
        self.clamp();
    }

    /// The box centred on the screen, the image clipped to it at `opacity`, and the player marker.
    pub fn draw(&mut self, gu: &mut GameUi, pos: (f32, f32), facing: f32, sw: f32, sh: f32, opacity: f32) {
        if let Some(Ok(Some((name, iw, ih, rgba)))) = self.rx.as_ref().map(|r| r.try_recv()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
            self.rx = None;
        }
        let Some(region) = self.region else { return };
        let side = (sw.min(sh) * OVERLAY_SIZE).round();
        let rect = [((sw - side) / 2.0).round(), ((sh - side) / 2.0).round(), side, side];
        if rect != self.rect {
            // Opened or resized: the whole map.
            self.rect = rect;
            self.zoom = self.fit();
            self.view = Some((region.ox as f32, region.oy as f32));
        }
        let (Some(t), Some(v)) = (&self.texture, self.view) else { return };
        let z = self.zoom;
        gu.clip(Some(rect));
        gu.image_rect(t, rect[0] + (region.ox as f32 - v.0) * z, rect[1] + (region.oy as f32 - v.1) * z, 512.0 * z, 512.0 * z, [1.0, 1.0, 1.0, opacity]);
        let (px, py) = region.px(pos.0, pos.1);
        let (sx, sy) = (rect[0] + (px as f32 - v.0) * z, rect[1] + (py as f32 - v.1) * z);
        let (s, c) = facing.sin_cos();
        gu.sprite_affine("menu    keytops3", MARKER, sx, sy, [c, -s, s, c], 0x8080_8080);
        gu.clip(None);
    }
}

/// Reads and decodes the map DAT off the UI thread: its first image.
fn load(index: Arc<DatIndex>, file: u32) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let got = index.read(file).ok().and_then(|b| crate::dat::ui::UiFile::parse(&b).map_err(|e| log::warn!("map {file}: {e:#}")).ok()).and_then(|ui| {
            let (name, img) = ui.images.into_iter().next()?;
            Some((format!("map:{name}"), img.width, img.height, img.rgba))
        });
        let _ = tx.send(got);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlay keeps the map pixel under the cursor while zooming, and the image over the whole box.
    #[test]
    fn overlay_zoom_and_pan() {
        let region = Region { zone: 1, key: 0, num: 0, flags: 0, scale: 1, ki: -1, file: 0, ox: -256, oy: -256 };
        let mut o = Overlay::new(1, Some(region), None);
        o.rect = [100.0, 50.0, 512.0, 512.0];
        o.zoom = 1.0;
        o.view = Some((-256.0, -256.0));
        let p = (356.0, 306.0);
        let under = |o: &Overlay| (o.view.unwrap().0 + (p.0 - o.rect[0]) / o.zoom, o.view.unwrap().1 + (p.1 - o.rect[1]) / o.zoom);
        let before = under(&o);
        o.zoom_at(p, 2.0);
        assert!((o.zoom - 1.5625).abs() < 1e-4);
        let after = under(&o);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
        // Zoomed out past fitting: back to the whole map; panning can't leave the image.
        o.zoom_at(p, -10.0);
        assert_eq!((o.zoom, o.view), (1.0, Some((-256.0, -256.0))));
        o.zoom_at(p, 3.0);
        o.pan(10_000.0, -10_000.0);
        let span = 512.0 - 512.0 / o.zoom;
        assert_eq!(o.view, Some((-256.0, -256.0 + span)));
    }

    /// Sub-keys from MZB object flags, and Pso'Xja (9): records for keys 1..16, none for key 0.
    #[test]
    fn sub_keys() {
        use crate::client::ground::map_key_of;
        assert_eq!([0xe, 0x3e, 0x400_0004, 0x400_003c, 0x800_0004].map(map_key_of), [1, 7, 8, 15, 16]);
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let regs = regions(&dll);
        assert!(Region::find_key(&regs, 9, 0).is_none());
        assert!((1..=16).all(|k| Region::find_key(&regs, 9, k).is_some()));
    }

    /// Change Map's groups: 50, the first San d'Oria's four zones with name 0.
    #[test]
    fn region_groups_found() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
        let g = region_groups(&dll);
        assert_eq!(g.len(), 50);
        assert_eq!(g[0], (0, vec![230, 231, 232, 233]));
    }

    /// Jugner Forest (104): record key 0, scale 1, key item 1 (0x181), file 5 -> 5317, origin (-264, -256).
    #[test]
    fn jugner_region() {
        let Ok(dll) = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")) else { return };
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
