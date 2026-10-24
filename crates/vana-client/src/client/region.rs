//! Region Info (main menu row 11; menu_rows2_spec part R): "menu    region" picks Conquest, Besieged, Campaign or
//! Colonization, and cnqframe (FUN_101e7980, a map-window subclass) draws that map with the markers read from
//! FFXiMain and the info windows of its mode. The data is s2c 0x05E / 0x071, asked for with c2s 0x05A at most every
//! 120 s.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::dat::tables::{CnqMarker, CnqTables};

pub const MENU: &str = "menu    region";
/// The maps by mode (DAT_1037f454): menumap conquest, m_ex3_rg, m_ex4_rg, m_ex5_rg.
const IMAGES: [u32; 4] = [5312, 53416, 53443, 53569];
/// The icon set (DAT_10381e10): keytops3 sprites by icon index.
const ICONS: [usize; 24] = [63, 63, 307, 113, 110, 111, 112, 432, 115, 113, 118, 117, 116, 114, 115, 180, 179, 178, 383, 385, 384, 502, 433, 0];
/// Label colours (DAT_10382084).
const COLOURS: [u32; 5] = [0x2020c0, 0x6060a0, 0x606070, 0x504010, 0x7020c0];
/// The legend windows and the title (group 0xd) per mode.
const LEGENDS: [&str; 4] = ["menu    conq3pts", "menu    imperial", "menu    allied", "menu    colopoin"];
const TITLES: [usize; 4] = [0x8e, 0x8c, 0x8f, 0xa2];
const CONQUER1: &str = "menu    conquer1";
const CONQ6STA: &str = "menu    conq6sta";
const BESEIGE1: &str = "menu    beseige1";
const BESEIGE2: &str = "menu    beseige2";
const COLORESU: &str = "menu    coloresu";
const COLORANK: &str = "menu    colorank";
const WHITE: u32 = 0x8080_8080;
/// conquer1's level marks (DAT_10381e40: Minimal, Minor, Major, Dominant, Equal) and the row labels (DAT_10381e4c,
/// framesus: San d'Oria, Bastok, Windurst, Beastmen, Neutral).
const LEVELS: [usize; 5] = [81, 80, 78, 77, 79];
const LABELS: [usize; 5] = [73, 75, 74, 94, 93];

/// What the server sent and the marker tables' state, kept for the session (retail keeps both in .data).
#[derive(Default)]
pub struct Data {
    /// D (DAT_10663594): s2c 0x05E from +4, and whether it counts as had (DAT_10663588).
    d: Vec<u8>,
    have: bool,
    /// _DAT_1066358c: the last reply, the request's callback set (DAT_10487e94) and helpwind's busy bit 2.
    stamp: Option<Instant>,
    pending: bool,
    pub busy: bool,
    /// The colonization callback (DAT_10487e98, one-shot) and its data: bayld, the six coalition ranks, per area
    /// (rate, bivouacs, max).
    colon_pending: bool,
    bayld: u32,
    ranks: [u8; 6],
    areas: [(u8, u8, u8); 8],
    /// The campaign buffer once a 0x071 mode 2 set is whole (DAT_10663644), and the one being filled (S+0x3c158).
    campaign: Option<Vec<u8>>,
    camp_build: Vec<u8>,
    camp_wanted: bool,
    pub tables: Option<Arc<CnqTables>>,
    pub markers: [Vec<CnqMarker>; 4],
}

impl Data {
    pub fn new(tables: Option<Arc<CnqTables>>) -> Self {
        let markers = tables.as_ref().map(|t| t.modes.clone()).unwrap_or_default();
        Data { tables, markers, d: vec![0; 0xb0], ..Default::default() }
    }

    fn byte(&self, o: usize) -> u8 {
        self.d.get(o).copied().unwrap_or(0)
    }

    fn word(&self, o: usize) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|i| self.byte(o + i)))
    }
}

/// FUN_101e8030 (FUN_10098010): c2s 0x05A once 120 s have passed since the last reply and none is out; the data
/// counts as missing until it comes, and every icon shows "?" (mode 2: none, mode 3: plain).
fn request(w: &mut World) {
    let d = &mut w.conquest;
    if d.pending || d.stamp.is_some_and(|t| t.elapsed() <= Duration::from_secs(120)) {
        return;
    }
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x05A, body: Vec::new() });
    (d.pending, d.colon_pending, d.camp_wanted, d.have, d.campaign, d.busy) = (true, true, true, false, None, true);
    let m = &mut d.markers;
    for e in m[0].iter_mut().skip(4) {
        e.icon = 8;
    }
    for e in m[1].iter_mut().skip(1).take(5) {
        (e.flags, e.icon, e.icon2) = (e.flags & !4, 8, 8);
    }
    for e in m[2].iter_mut().skip(16).take(26) {
        (e.flags, e.icon, e.icon2) = (e.flags & !0x200, 0xff, 8);
    }
    for e in m[3].iter_mut().skip(1).take(8) {
        (e.flags, e.icon, e.icon2) = (e.flags & !4, 3, 3);
    }
}

/// s2c 0x05E (FUN_100998a0 -> FUN_101e80f0), taken only while asked for: D, the time, the help line cleared; Besieged's
/// orb icons; when not calculating, each region's owner icon and its zones' (FUN_101e80f0's loops).
pub fn on_conquest(w: &mut World, p: &[u8]) {
    let d = &mut w.conquest;
    if !d.pending {
        return;
    }
    d.d = (0..0xb0).map(|i| p.get(4 + i).copied().unwrap_or(0)).collect();
    (d.stamp, d.have, d.pending, d.busy) = (Some(Instant::now()), true, false, false);
    if let Some(m) = w.cnq.as_mut() {
        m.help = None;
    }
    let d = &mut w.conquest;
    if d.word(0x98) != 0 {
        for e in d.markers[1].iter_mut().skip(1).take(5) {
            (e.icon, e.icon2) = (0x14, 0x14);
        }
        let k = (d.word(0x9c) & 3) as usize + 1;
        if let Some(e) = d.markers[1].get_mut(k) {
            (e.flags, e.icon) = (e.flags | 4, 0x13);
        }
    }
    if d.byte(0x94) == 0 {
        let clamp = |v: i32| if (0..=0x17).contains(&v) { v as u8 } else { 8 };
        for r in 0..19 {
            let owner = d.byte(0x19 + 4 * r) as i8 as i32;
            let Some(region) = d.markers[0].get_mut(4 + r) else { break };
            region.icon = clamp(owner + 3);
            let id = region.id;
            for z in d.markers[0].iter_mut().skip(23).filter(|z| z.id == id) {
                z.icon = clamp(if (1..=3).contains(&owner) && z.b7 & 1 != 0 { owner + 0xe } else { owner + 9 });
            }
        }
    }
}

/// s2c 0x071 (FUN_100998d0): mode 2 the campaign set (allied notes, controlled areas, seven armies, then 13 area records
/// per packet at ZoneOffset @9; complete at 26), mode 3 the colonization block (one-shot, LAB_101e8260).
pub fn on_influence(w: &mut World, p: &[u8]) {
    let at = |o: usize| p.get(o).copied().unwrap_or(0);
    let word = |o: usize| u32::from_le_bytes(std::array::from_fn(|i| at(o + i)));
    let d = &mut w.conquest;
    match at(4) {
        2 => {
            if d.camp_build.len() != 0x24 + 26 * 12 {
                d.camp_build = vec![0; 0x24 + 26 * 12];
            }
            d.camp_build[..0x24].copy_from_slice(&(0..0x24).map(|i| at(0xc + i)).collect::<Vec<u8>>());
            let off = at(9) as usize;
            if off < 26 {
                for k in 0..13.min(26 - off) {
                    let o = 0x24 + 12 * (off + k);
                    d.camp_build[o..o + 12].copy_from_slice(&(0..12).map(|i| at(0x30 + 12 * k + i)).collect::<Vec<u8>>());
                }
            }
            // LAB_101e8200 (kept once set): the 26 areas' battle squares and owner icons.
            if off + 13 >= 26 && d.camp_wanted {
                let buf = d.camp_build.clone();
                for (i, e) in d.markers[2].iter_mut().skip(16).take(26).enumerate() {
                    let rec = u32::from_le_bytes(std::array::from_fn(|j| buf[0x24 + 12 * i + j]));
                    (e.flags, e.icon) = if rec & 1 != 0 { (e.flags | 0x200, 0x16) } else { (e.flags & !0x200, 0xff) };
                    e.icon2 = ((rec >> 1) & 7).min(4) as u8 + 9;
                }
                d.campaign = Some(buf);
            }
        }
        3 if d.colon_pending => {
            d.colon_pending = false;
            d.bayld = word(0x38);
            let ranks = word(0x10);
            d.ranks = std::array::from_fn(|k| (ranks >> (4 * k) & 0xf) as u8);
            d.areas = std::array::from_fn(|k| {
                let v = word(0x18 + 4 * k);
                ((v & 0x7f) as u8, (v >> 7 & 7) as u8, (v >> 10 & 7) as u8)
            });
            for e in d.markers[3].iter_mut().skip(1).take(8) {
                (e.icon, e.icon2) = (3, 3);
            }
        }
        _ => {}
    }
}

/// "menu    region" (registry flags 0x20000, group 0x81: in the main menu's place).
#[derive(Clone, Debug)]
pub struct Menu {
    pub sel: u16,
    return_menu: Option<usize>,
    /// FUN_1015e6e0: hidden while the map is up, resumed by its close callback LAB_101e7e90.
    hidden: bool,
}

/// FUN_10140400 case 0xb: the region menu, SE 0002.
pub fn open_menu(w: &mut World) {
    w.sounds.push(Ui::Select as u32);
    let sel = w.cursor_recall(MENU);
    w.region_menu = Some(Menu { sel, return_menu: w.menu, hidden: false });
    w.menu = None;
}

/// The generic handler: links (SE 0001), Enter (SE 0002) opens the row's map (FUN_10140780; no tables: SE 0004),
/// Escape (SE 0003) closes back to the main menu (FUN_10140830).
pub fn menu_key(w: &mut World, gu: &GameUi, key: egui::Key, sw: f32, sh: f32) {
    let Some(mut m) = w.region_menu.clone() else { return };
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let k = if key == egui::Key::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(MENU, m.sel).map_or(m.sel, |l| l[k] as u16);
            if to != m.sel && to != 0 && to != 0xff {
                m.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        egui::Key::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            w.region_menu = None;
            w.menu = m.return_menu;
            return;
        }
        egui::Key::Enter => {
            w.sounds.push(Ui::Select as u32);
            w.region_menu = Some(m.clone());
            if !open_map(w, gu, m.sel as usize - 1, sw, sh) {
                w.sounds.push(Ui::Error as u32);
            }
            return;
        }
        _ => {}
    }
    w.region_menu = Some(m);
}

/// /regionmap, /besiegemap, /campaignmap, /colonizationmap (0x50..0x53, FUN_1007d300): the map when none is open.
pub fn command(w: &mut World, gu: &GameUi, mode: usize, sw: f32, sh: f32) {
    if w.cnq.is_none() {
        open_map(w, gu, mode, sw, sh);
    }
}

/// The map window's view (the map base's +0x24 view, +0x58 zoom, +0x68 base zoom, +0x1c cursor point, fade +0xa6) as
/// client/map.rs ports it, in cursor mode (the point moves inside a box; past it the view scrolls).
struct View {
    screen: (f32, f32),
    fit: f32,
    zoom: f32,
    zoom_base: f32,
    zoom_to: Option<f32>,
    view: (f32, f32),
    point: (f32, f32),
    scroll_to: Option<(f32, f32)>,
    /// Level, target, step per retail frame; the fade-out's 3 s deadline.
    fade: (f32, f32, f32),
    closing: Option<Instant>,
}

impl View {
    /// FUN_101ee040: the view over the image (at the origin).
    fn clamp(&mut self) {
        let (sw, sh) = self.screen;
        let z = self.zoom;
        let b = [((512.0 * z - sw) / z).max(0.0), ((512.0 * z - sh) / z).max(0.0)];
        self.view = (self.view.0.clamp(0.0, b[0]), self.view.1.clamp(0.0, b[1]));
    }

    /// FUN_101ede30 within [fit, 4 fit] *(inf: map.rs's limits)*, the map pixel under the point kept.
    fn zoom_by(&mut self, dz: f32) {
        let z = (self.zoom + dz).clamp(self.fit, self.fit * 4.0);
        self.view.0 += self.point.0 / self.zoom - self.point.0 / z;
        self.view.1 += self.point.1 / self.zoom - self.point.1 / z;
        self.zoom = z;
        self.clamp();
    }

    fn scroll_by(&mut self, d: (f32, f32)) {
        self.view.0 += d.0 / self.zoom;
        self.view.1 += d.1 / self.zoom;
        self.clamp();
    }

    fn under_point(&self) -> (f32, f32) {
        (self.point.0 / self.zoom + self.view.0 + 0.5, self.point.1 / self.zoom + self.view.1 + 0.5)
    }

    /// FUN_101ef990.
    fn scroll_to_px(&mut self, to: (f32, f32)) {
        let p = self.under_point();
        if (to.0 - p.0).powi(2) + (to.1 - p.1).powi(2) >= 0.8 {
            self.clamp();
            self.scroll_to = Some(to);
        }
    }

    /// One retail frame: fade, zoom-to, scroll-to (as map.rs).
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

    /// A map pixel on screen (truncated as __ftol).
    fn screen_of(&self, px: f32, py: f32) -> (f32, f32) {
        (((px - self.view.0) * self.zoom + 0.5).trunc(), ((py - self.view.1) * self.zoom + 0.5).trunc())
    }
}

/// conquer1's contents, filled once when it opens (FUN_101e9110): precise (+0x14) with the nation ahead (+0x18, 4
/// several, 3 beastmen) and fractions, or coarse levels; the owner (+0x15), the beastmen flag (+0x16), the tally days.
#[derive(Clone, Copy, Debug)]
struct Conquer1 {
    precise: bool,
    top: u8,
    levels: [f32; 3],
    beast: f32,
    owner: u8,
    beast_flag: bool,
    tally: u8,
}

/// cnqframe (DAT_10663648) and its info windows.
pub struct CnqMap {
    pub mode: usize,
    view: View,
    texture: Option<String>,
    rx: Option<Receiver<Option<(String, u32, u32, Vec<u8>)>>>,
    /// +0x16c the record under the cursor (-1 none), +0x170 the player's region, +0x174 the zone at open.
    hovered: i32,
    player_region: i32,
    zone: u16,
    /// Help slot 2 (+0xb4 / FUN_1013a320(.., 2)).
    pub help: Option<String>,
    conquer1: Option<Conquer1>,
    conq6sta: bool,
    legend: bool,
    beseige1: bool,
    beseige2: Option<usize>,
    coloresu: Option<i16>,
    colorank: bool,
    /// conquer1's pulse (+0x19 alpha, +0x1a step).
    pulse: (i32, i32),
    /// Group 0x13 0..3 (San d'Oria, Bastok, Windurst, Jeuno) for the control lines.
    nations: [String; 4],
    /// The info windows on the left half (FUN_101e9320 / FUN_101e8df0: the cursor past the middle).
    left: bool,
    acc: f32,
}

/// FUN_101e6ca0(zone, named): the zone has a conquest zone record (and, `named`, a name).
fn conquest_zone(w: &World, zone: u16, named: bool) -> bool {
    w.conquest.markers[0].iter().skip(23).find(|m| m.par == zone as i16).is_some_and(|m| !named || m.name != -1)
}

/// FUN_101e6d80: the zone's region id (mode 3 first the (region, zone) pairs, then entries from index 8; else the zone
/// records'), -1 none.
fn region_of(w: &World, mode: usize, zone: u16) -> i32 {
    let (Some(t), m) = (w.conquest.tables.as_ref(), &w.conquest.markers[mode]) else { return -1 };
    let from = if mode == 3 {
        if let Some(p) = t.pairs.iter().find(|p| p.1 == zone as i16) {
            return p.0 as i32;
        }
        8
    } else {
        t.counts[mode].1
    };
    m.iter().skip(from).find(|e| e.par == zone as i16).map_or(-1, |e| e.id as i32)
}

/// FUN_101e7980: a map mode for the current zone. False when there are no tables (retail: no cnqframe).
pub fn open_map(w: &mut World, gu: &GameUi, mode: usize, sw: f32, sh: f32) -> bool {
    let Some(t) = w.conquest.tables.clone() else { return false };
    let zone = w.pos_zone;
    if mode == 0 && !conquest_zone(w, zone, false) {
        super::textcmd::refuse(w, 0x7b);
    }
    // Per entry of this mode: regions shown, fronts shown (hidden unless selected), zones hidden until picked.
    for e in w.conquest.markers[mode].iter_mut() {
        e.flags = match e.par {
            -1 => (e.flags | 1) & !0x20,
            -2 => e.flags | 1 | 0x20,
            _ if e.id == 0 => e.flags & !0x21,
            _ => (e.flags & !1) | 0x20,
        };
        e.alpha = 0;
    }
    request(w);
    // FUN_101e8c00: the start marker: the player's region, else the zone's own entry, else the middle.
    let (_, regions, default) = t.counts[mode];
    let mut rid = region_of(w, mode, zone);
    if rid == -1 {
        rid = default as i32;
    }
    let start = if rid > 0 {
        select_zones(&mut w.conquest.markers[mode], regions, rid);
        w.conquest.markers[mode].iter().take(regions).find(|e| e.id as i32 == rid).map(|e| (e.px, e.py))
    } else if rid == 0 {
        w.conquest.markers[mode].iter_mut().skip(regions).find(|e| e.par == zone as i16).map(|e| {
            e.flags |= 1;
            (e.px, e.py)
        })
    } else {
        None
    };
    let (px, py) = start.map_or((256.0, 256.0), |(x, y)| (x as f32, y as f32));
    let fit = sw / 512.0;
    let zoom = fit * 2.3;
    // FUN_101ee440 at the window's open: the point (W x 200 / 512, H / 2), a fade-in.
    let point = ((sw * 200.0 / 512.0).trunc(), (sh / 2.0).trunc());
    let mut view = View { screen: (sw, sh), fit, zoom, zoom_base: zoom, zoom_to: None, view: (px - point.0, py - point.1), point, scroll_to: None, fade: (128.0, 0.0, 8.0), closing: None };
    view.clamp();
    view.scroll_to_px((px, py));
    let rx = w.index.clone().map(|index| super::map::load(index, IMAGES[mode]));
    w.cnq = Some(CnqMap {
        mode,
        view,
        texture: None,
        rx,
        hovered: -1,
        player_region: rid,
        zone,
        help: None,
        conquer1: None,
        conq6sta: false,
        legend: true,
        beseige1: false,
        beseige2: None,
        coloresu: None,
        colorank: false,
        pulse: (0x80, -8),
        nations: std::array::from_fn(|k| gu.item_msg(0x13, k).unwrap_or_default()),
        left: false,
        acc: 0.0,
    });
    if let Some(m) = w.region_menu.as_mut() {
        m.hidden = true;
    }
    true
}

/// FUN_101e6eb0: the zones of region `id` (with a name) selected, the others not.
fn select_zones(m: &mut [CnqMarker], regions: usize, id: i32) {
    for e in m.iter_mut().skip(regions).filter(|e| e.name != -1) {
        if e.id as i32 == id { e.flags |= 1 } else { e.flags &= !1 }
    }
}

/// FUN_101e6f10 (Campaign): zone entries of record +2 `par` selected.
fn select_par(m: &mut [CnqMarker], regions: usize, par: i32) {
    for e in m.iter_mut().skip(regions).filter(|e| e.name != -1) {
        if e.par as i32 == par { e.flags |= 1 } else { e.flags &= !1 }
    }
}

/// FUN_101e6e50: the selected entry of the zone blinks.
fn blink_zone(m: &mut [CnqMarker], regions: usize, zone: u16) {
    for e in m.iter_mut().skip(regions) {
        if e.par == zone as i16 && e.flags & 1 != 0 { e.flags |= 4 } else { e.flags &= !4 }
    }
}

/// FUN_101e6f70: no zone selected or blinking.
fn clear_zones(m: &mut [CnqMarker], regions: usize) {
    for e in m.iter_mut().skip(regions) {
        e.flags &= !5;
    }
}

/// FUN_101e8500: the 0x05E data for mode 0 (not while calculating) or 1 (Besieged data valid); without it help slot 2
/// says why (group 9 6 / 5).
fn data<'a>(w: &'a World, help: &mut Option<String>, mode: usize) -> Option<&'a Data> {
    let d = &w.conquest;
    let line = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id));
    if !d.have {
        *help = line(6);
        return None;
    }
    let ok = if mode == 0 { d.byte(0x94) == 0 } else { d.word(0x98) != 0 };
    if !ok {
        *help = line(5);
    }
    ok.then_some(d)
}

impl CnqMap {
    /// FUN_101efb20: the first marker (not 0x800, selected or not hidden) within 4 map pixels of the one under the
    /// cursor point.
    fn under_cursor(&self, m: &[CnqMarker]) -> Option<usize> {
        let (mx, my) = self.view.under_point();
        let (mx, my) = (mx.trunc() as i32, my.trunc() as i32);
        m.iter().position(|e| e.flags & 0x800 == 0 && (e.flags & 1 != 0 || e.flags & 0x20 == 0) && (e.px as i32 - mx).abs() < 5 && (e.py as i32 - my).abs() < 5)
    }
}

/// FUN_101e9110: conquer1's contents for region id `id` (r = id - 5): precise in the player's own region standing in
/// a named conquest zone, else coarse.
fn fill_conquer1(w: &World, id: i32, player_region: i32) -> Conquer1 {
    let d = &w.conquest;
    let r = (id - 5).max(0) as usize;
    let g = d.byte(0x18 + 4 * r);
    let mut c = Conquer1 { precise: false, top: 0, levels: [-1.0; 3], beast: 0.0, owner: 4, beast_flag: g & 0x40 != 0, tally: d.byte(0x88) };
    if id == player_region && conquest_zone(w, w.pos_zone, true) {
        let rank = d.byte(0x16 + 4 * r);
        let mut firsts = 0;
        let mut total = 0.0;
        for k in 0..3 {
            if rank >> (2 * k) & 3 == 1 {
                firsts += 1;
                c.top = k as u8;
            }
            total += d.byte(0x82 + k) as i8 as f32;
        }
        if firsts > 1 {
            c.top = 4;
        }
        if g & 0x40 != 0 {
            c.top = 3;
        }
        total += d.word(0x90) as i32 as f32;
        for k in 0..3 {
            c.levels[k] = if total == 0.0 { 0.0 } else { d.byte(0x82 + k) as i8 as f32 / total };
        }
        c.beast = if total == 0.0 { 0.0 } else { d.word(0x90) as f32 / total };
        c.precise = true;
    } else if g & 0x80 != 0 {
        c.levels = [4.0; 3];
    } else {
        c.levels = std::array::from_fn(|k| (g >> (2 * k) & 3) as f32);
    }
    let owner = d.byte(0x19 + 4 * r) as i8 - 1;
    c.owner = if owner < 0 { 4 } else { owner as u8 };
    c
}

/// FUN_101e89c0 per frame (not while closing): the mode's windows by the marker under the cursor; Num5 (or both zoom
/// keys) with a marker under it puts the cursor at (W x 200 / 512, H / 2) and scrolls that marker under it.
fn frame_logic(w: &mut World, m: &mut CnqMap, recentre: bool) {
    let mode = m.mode;
    let regions = w.conquest.tables.as_ref().map_or(0, |t| t.counts[mode].1);
    let under = m.under_cursor(&w.conquest.markers[mode]);
    let mut help = m.help.take();
    match mode {
        0 => {
            let has = data(w, &mut help, 0).is_some();
            match under.map(|i| w.conquest.markers[0][i]) {
                None => {
                    m.hovered = -1;
                    clear_zones(&mut w.conquest.markers[0], regions);
                    m.conquer1 = None;
                    m.conq6sta |= has;
                    help = None;
                }
                Some(e) if e.par == -1 => {
                    let id = e.id as i32;
                    if m.hovered != id {
                        m.hovered = id;
                        select_zones(&mut w.conquest.markers[0], regions, id);
                        if id == m.player_region {
                            blink_zone(&mut w.conquest.markers[0], regions, m.zone);
                        }
                        let bt = w.battle.clone();
                        // sprintf of group 10 8 "This region is currently under the control of %s." with group 0x13 k.
                        let control = |k: usize| Some(super::textcmd::printf(&bt.as_ref()?.group10(8)?, &[m.nations[k].clone()]));
                        help = match e.icon {
                            c @ 4..=6 => control(c as usize - 4),
                            2 => control(3),
                            7 => bt.as_ref().and_then(|b| b.group9(3)),
                            3 => bt.as_ref().and_then(|b| b.group9(2)),
                            9 => bt.as_ref().and_then(|b| b.group9(4)),
                            _ => None,
                        };
                        m.conq6sta = false;
                    }
                    let top = w.conquest.tables.as_ref().map_or(4, |t| t.counts[0].0) as i32;
                    if m.hovered > top {
                        if !has {
                            m.help = help;
                            return;
                        }
                        if m.conquer1.is_none() {
                            m.conquer1 = Some(fill_conquer1(w, m.hovered, m.player_region));
                        }
                    }
                }
                Some(_) => {}
            }
            m.legend = has;
        }
        1 => {
            let has = data(w, &mut help, 1).is_some();
            match under.map(|i| w.conquest.markers[1][i]) {
                None => {
                    m.hovered = -1;
                    clear_zones(&mut w.conquest.markers[1], regions);
                    m.beseige2 = None;
                    m.beseige1 |= has;
                    help = None;
                }
                Some(e) => {
                    let id = e.id as i32;
                    if m.hovered != id {
                        m.hovered = id;
                        select_zones(&mut w.conquest.markers[1], regions, id);
                        if id == m.player_region {
                            blink_zone(&mut w.conquest.markers[1], regions, m.zone);
                        }
                    }
                    if (0x1e..=0x20).contains(&e.name) {
                        m.beseige1 = false;
                        if has && m.beseige2.is_none() {
                            m.beseige2 = Some(e.name as usize - 0x1e);
                        }
                    } else {
                        m.beseige2 = None;
                        m.beseige1 |= has;
                    }
                }
            }
            m.legend = has;
        }
        2 => {
            // FUN_101e8880: no campaign data on LSB yet; the windows need it.
            if w.conquest.campaign.is_none() {
                help = w.battle.as_ref().and_then(|b| b.group9(6));
            }
            match under.map(|i| w.conquest.markers[2][i]) {
                None => {
                    m.hovered = -1;
                    clear_zones(&mut w.conquest.markers[2], regions);
                }
                Some(e) => {
                    let par = e.par as i32;
                    if m.hovered != par {
                        m.hovered = par;
                        select_par(&mut w.conquest.markers[2], regions, par);
                    }
                }
            }
            m.legend = w.conquest.campaign.is_some();
        }
        _ => {
            help = None;
            match under.map(|i| w.conquest.markers[3][i]) {
                None => {
                    m.hovered = -1;
                    clear_zones(&mut w.conquest.markers[3], regions);
                    (m.coloresu, m.colorank) = (None, false);
                }
                Some(e) => {
                    let id = e.id as i32;
                    if m.hovered != id {
                        m.hovered = id;
                        select_zones(&mut w.conquest.markers[3], regions, id);
                        if id == m.player_region {
                            blink_zone(&mut w.conquest.markers[3], regions, m.zone);
                        }
                    }
                    (m.colorank, m.coloresu) = if e.id == 1 { (true, None) } else { (false, Some(e.id)) };
                }
            }
        }
    }
    m.help = help;
    if recentre && let Some(i) = under {
        let e = w.conquest.markers[mode][i];
        let (sw, sh) = m.view.screen;
        m.view.point = ((sw / 512.0 * 200.0).trunc(), (sh / 2.0).trunc());
        m.view.scroll_to_px((e.px as f32, e.py as f32));
    }
}

/// FUN_101eef90 per marker: unselected fade out, selected fade in, a blinking one bounces between 0 and 0x80 (6 a
/// tick).
fn alpha_step(e: &mut CnqMarker, ticks: f32) {
    let step = 6.0 * ticks;
    let a = e.alpha as f32;
    if e.flags & 1 == 0 {
        let v = a - step;
        if v < 0.0 {
            e.flags &= !0x400;
            e.alpha = 0;
        } else {
            e.alpha = v as u8;
        }
    } else if e.flags & 0x2400 == 0 {
        let v = a + step;
        if v > 128.0 {
            e.alpha = 0x80;
            if e.flags & 4 != 0 {
                e.flags |= 0x400;
            } else if e.flags & 0x1000 != 0 {
                e.flags |= 0x2000;
            }
        } else {
            e.alpha = v as u8;
        }
    } else {
        let v = a - step;
        if v < 0.0 {
            e.flags &= !0x2400;
            e.alpha = 0;
        } else {
            e.alpha = v as u8;
        }
    }
}

/// One frame of cnqframe (FUN_101e89c0 then the base FUN_101ef0a0): zoom keys, scroll keys and the cursor (axes
/// 0x9d / 0x9e x 15 inside [W / 512 x 40, W - W / 512 x 60] x [H / 448 x 65, H - H / 448 x 180]; past it the view
/// scrolls), the retail frames, the marker alphas. False once the fade-out has ended.
pub fn update(w: &mut World, input: &mut super::input::Input, ticks: f32) -> bool {
    use super::input::mode::{HELD, PRESS};
    let Some(mut m) = w.cnq.take() else { return true };
    if let Some(deadline) = m.view.closing {
        if (m.view.fade.0 == m.view.fade.1 && m.view.scroll_to.is_none() && m.view.zoom_to.is_none()) || Instant::now() >= deadline {
            close_done(w);
            return false;
        }
    } else {
        let recentre = input.test(0x7d, PRESS) || (input.test(0xa4, HELD) && input.test(0xa5, HELD));
        frame_logic(w, &mut m, recentre);
        let fast = if input.test(0x9a, HELD) { 2.0 } else { 1.0 };
        if m.view.zoom_to.is_none() {
            if input.test(0x7d, PRESS) || (input.test(0xa4, HELD) && input.test(0xa5, HELD)) {
                m.view.zoom_to = Some(m.view.zoom_base);
            } else {
                let keys: f32 = -(if input.test(0xa4, HELD) { 1.0 } else if input.test(0xa5, HELD) { -1.0 } else { 0.0 });
                let stick = input.axis(0x7c);
                let a = if stick.abs() > keys.abs() { stick } else { keys };
                if a != 0.0 {
                    m.view.zoom_by(-a * 0.08 * fast * ticks * 0.5);
                }
            }
        }
        // Cursor mode (flag 0x100) reads only the cursor axes, not the scroll ones (0x79 / 0x7a).
        let dead = |v: f32| if (v * fast).abs() < 2.0 { 0.0 } else { v };
        let (sw, sh) = m.view.screen;
        let v = (dead(input.axis(0x9d) * 15.0) * fast * ticks * 0.5, dead(input.axis(0x9e) * 15.0) * fast * ticks * 0.5);
        let (bx, by) = ([sw / 512.0 * 40.0, sw - sw / 512.0 * 60.0], [sh / 448.0 * 65.0, sh - sh / 448.0 * 180.0]);
        let want = (m.view.point.0 + v.0, m.view.point.1 + v.1);
        let moved = (want.0.clamp(bx[0], bx[1]), want.1.clamp(by[0], by[1]));
        let over = (want.0 - moved.0, want.1 - moved.1);
        if over != (0.0, 0.0) {
            m.view.scroll_by(over);
        }
        m.view.point = moved;
        m.left = m.view.point.0 > sw / 2.0;
    }
    m.acc += ticks;
    while m.acc >= 2.0 {
        m.acc -= 2.0;
        m.view.frame();
        // FUN_101e9320: conquer1's pulse between 0x20 and 0x80, 8 a frame.
        m.pulse.0 += m.pulse.1;
        if m.pulse.0 < 0x20 {
            m.pulse = (0x20, 8);
        } else if m.pulse.0 > 0x80 {
            m.pulse = (0x80, -8);
        }
    }
    if m.view.fade.0 == 0.0 {
        for e in w.conquest.markers[m.mode].iter_mut() {
            alpha_step(e, ticks);
        }
    }
    w.cnq = Some(m);
    true
}

/// Escape (FUN_101e8b10, not while closing): the base fade-out (slot +0x50), every info window closed at once.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(m) = w.cnq.as_mut() else { return };
    if key == egui::Key::Escape && m.view.closing.is_none() {
        w.sounds.push(Ui::Cancel as u32);
        m.view.closing = Some(Instant::now() + Duration::from_secs(3));
        m.view.fade = (m.view.fade.0, 255.0, 16.0);
        (m.conquer1, m.conq6sta, m.legend, m.beseige1, m.beseige2, m.coloresu, m.colorank) = (None, false, false, false, None, None, false);
    }
}

/// FUN_101e7ef0 and LAB_101e7e90: the markers and the busy bit cleared; the region menu resumes if open, else the
/// main menu.
fn close_done(w: &mut World) {
    w.conquest.busy = false;
    let mode_markers = w.cnq.as_ref().map(|m| m.mode);
    if let Some(mode) = mode_markers {
        for e in w.conquest.markers[mode].iter_mut() {
            e.alpha = 0;
        }
    }
    match w.region_menu.as_mut() {
        Some(r) => r.hidden = false,
        None => w.menu = w.menu.or(Some(0)),
    }
}

/// Title slot 2 the map's name (group 0xd), help slot 2 the map's line; the region menu's rows (help 0xe 0x14b, title
/// 0xd 1) while it has the focus.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if let Some(m) = &w.cnq {
        bars.title[2] = gu.title_msg(TITLES[m.mode]).unwrap_or_default();
        bars.help[2] = m.help.clone().unwrap_or_default();
        return;
    }
    if let Some(r) = w.region_menu.as_ref().filter(|r| !r.hidden) {
        let [help, title] = gu.focus_text(MENU, r.sel);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
}

fn number(gu: &mut GameUi, s: &str, x: f32, y: f32) {
    gu.shape_text(s, x, y, WHITE, true);
}

/// A gauge (FUN_101cb040: windowps frame 38, fill 97): `fill` drawn, or only its ghost (0x30808080).
fn gauge(gu: &mut GameUi, x: f32, y: f32, f: f32, ghost: bool) {
    let (b, ui, _, _) = gu.parts();
    let Some(l) = ui.lib("menu    windowps") else { return };
    let (value, anim) = if ghost { (0.0, f) } else { (f, f) };
    crate::render::screens::push_gauge_eased(b, ui, l, x, y, 38, 97, value, anim);
}

/// An info window at its layout place, or at x 16 while the cursor is on the right half; `size` overrides its frame
/// (FUN_1011a5a0). Returns its top-left and width.
fn info(gu: &mut GameUi, name: &str, sw: f32, sh: f32, left: bool, size: Option<(f32, f32)>) -> Option<(f32, f32, f32)> {
    let [_, ly, lw, lh] = gu.window_rect(name, sw, sh)?;
    let (w, h) = size.unwrap_or((lw, lh));
    // Right: 496 - width from the right edge (as the layout's 384 for 112 wide).
    let x = if left { 16.0 } else { sw - 16.0 - w };
    gu.window_sized(name, x, ly, w, h).then_some((x, ly, w))
}

/// The map image once loaded (FUN_101cc620).
pub fn poll_image(gu: &mut GameUi, m: &mut CnqMap) {
    if let Some(Ok(Some((name, iw, ih, rgba)))) = m.rx.as_ref().map(|r| r.try_recv()) {
        gu.image(&name, iw, ih, rgba);
        m.texture = Some(name);
        m.rx = None;
    }
}

/// The map (cnqframe's base draw FUN_101eebd0: the image, then at fade level 0 each marker by FUN_101ee7b0), the
/// cursor (anc_regi at the point - 8), the info windows of the mode.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    let Some(m) = w.cnq.as_ref() else { return };
    let level = m.view.fade.0;
    let z = m.view.zoom;
    if let Some(t) = &m.texture {
        let a = if level == 0.0 { 160.0 } else { (255.0 - level).min(160.0) } / 128.0;
        gu.image_rect(t, -m.view.view.0 * z + 0.5, -m.view.view.1 * z + 0.5, 512.0 * z, 512.0 * z, [0.5, 0.5, 0.5, a.min(1.0)]);
    }
    if level == 0.0 {
        let mode = m.mode;
        let view = &m.view;
        for e in &w.conquest.markers[mode] {
            draw_marker(gu, view, e, label(gu, mode, e).as_deref());
        }
        if m.view.closing.is_none() {
            let n = gu.lib_len("anc     anc_regi").max(1);
            let frame = gu.cursor_frame();
            gu.sprite("anc     anc_regi", frame % n, m.view.point.0 - 8.0, m.view.point.1 - 8.0, WHITE);
        }
    }
    draw_windows(gu, w, m, sw, sh);
}

/// "menu    region" (hidden while its map is up).
pub fn draw_menu(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let r = w.region_menu.as_ref().filter(|r| !r.hidden)?;
    gu.lobby_window_off(MENU, sw, sh, (0.0, 0.0), Some(r.sel), w.confirm.is_none(), &[]).map(|win| win.frame)
}

/// The marker's name (+0x18, FUN_101e6fb0): group 0x13 for regions and fronts, 0x15 for zones; mode 3 regions are
/// zone names except Ra'Kaznar (id 9).
fn label(gu: &GameUi, mode: usize, e: &CnqMarker) -> Option<String> {
    if e.name < 0 {
        return None;
    }
    let region = if mode == 3 { e.par == -1 && (e.id == 1 && e.name == 49 || e.id == 9) } else { matches!(e.par, -1 | -2) };
    gu.item_msg(if region { 0x13 } else { 0x15 }, e.name as usize)
}

/// FUN_101ee7b0: centred on its pixel by the bounds icon (unless 0x10); the second icon first with 0x100; the icon in
/// alpha << 24 | 0x808080 blend 0x44 (plain with 0x200); the label (no 0x08, alpha != 0) in its colour scaled by the
/// alpha (halved with 0x800), placed by +0xa / +0xc or by the place code (0x40).
fn draw_marker(gu: &mut GameUi, v: &View, e: &CnqMarker, name: Option<&str>) {
    let sprite = |i: u8| ICONS.get(i as usize).copied().filter(|_| (i as i8) >= 0);
    let Some(bound) = sprite(if e.flags & 0x100 != 0 { e.icon2 } else { e.icon }) else { return };
    let b = gu.cell_bounds("menu    keytops3", bound, 0).unwrap_or([0.0; 4]);
    let (iw, ih) = ((b[2] - b[0]) as i32, (b[3] - b[1]) as i32);
    let (mut x, mut y) = v.screen_of(e.px as f32, e.py as f32);
    if e.flags & 0x10 == 0 {
        x -= (iw / 2) as f32;
        y -= (ih / 2) as f32;
    }
    if e.flags & 0x100 != 0
        && let Some(k) = sprite(e.icon2)
    {
        gu.sprite("menu    keytops3", k, x, y, WHITE);
    }
    if let Some(k) = sprite(e.icon) {
        if e.flags & 0x200 != 0 {
            gu.sprite("menu    keytops3", k, x, y, WHITE);
        } else {
            gu.sprite_scaled_blend("menu    keytops3", k, x, y, 1.0, 1.0, (e.alpha as u32) << 24 | 0x80_8080, 0x44);
        }
    }
    let Some(name) = name.filter(|_| e.alpha != 0 && e.flags & 8 == 0) else { return };
    let a = if e.flags & 0x800 != 0 { e.alpha as u32 / 2 } else { e.alpha as u32 };
    let rgb = COLOURS.get(e.colour as usize).copied().unwrap_or(0);
    let c = |s: u32| (((rgb >> s) & 0xff) * a >> 7) << s;
    let colour = a << 24 | c(16) | c(8) | c(0);
    let tw = gu.text_width(name) as i32;
    let q = |t: i32| t / 4;
    let (lx, ly) = if e.flags & 0x40 == 0 {
        (x + e.dx as f32, y + e.dy as f32)
    } else {
        match e.place {
            0 => (x + (iw / 2 - tw / 2) as f32, y - 17.0),
            1 => (x + (iw / 2 - tw / 2) as f32, y + 1.0 + ih as f32),
            2 => (x - 3.0 - tw as f32, y),
            4 => (x + (iw / 2 - 3 * q(tw)) as f32, y - 17.0),
            5 => (x + (iw / 2 - q(tw)) as f32, y - 17.0),
            6 => (x + (iw / 2 - 3 * q(tw)) as f32, y + 1.0 + ih as f32),
            7 => (x + (iw / 2 - q(tw)) as f32, y + 1.0 + ih as f32),
            _ => (x + 2.0 + iw as f32, y),
        }
    };
    gu.text(name, lx, ly, 0, colour.to_le_bytes());
}

/// sprintf of a group 10 line with one number (e.g. 0x56 "%d day.").
fn line10(w: &World, id: usize, n: i32) -> String {
    w.battle.as_ref().and_then(|b| b.group10(id)).map_or(String::new(), |f| super::textcmd::printf(&f, &[n.to_string()]))
}

/// The mode's info windows (sections R5): conquer1 / conq6sta / conq3pts, beseige1 / beseige2 / imperial, coloresu /
/// colorank / colopoin.
fn draw_windows(gu: &mut GameUi, w: &World, m: &CnqMap, sw: f32, sh: f32) {
    let d = &w.conquest;
    let left = m.left;
    if let Some(c) = m.conquer1 {
        let lh = gu.window_rect(CONQUER1, sw, sh).map_or(206.0, |r| r[3]);
        let h = lh - if c.precise { 8.0 } else { 30.0 };
        if let Some((x, y, _)) = info(gu, CONQUER1, sw, sh, left, Some((112.0, h))) {
            // FUN_101e9410: the owner banner, the days to the tally, the rows (or "The beastmen are dominant.").
            if c.owner < 3 {
                gu.sprite("menu    keytops3", 74 + c.owner as usize, x + 12.0, y + 19.0, WHITE);
            } else {
                gu.sprite("menu    framesus", LABELS[c.owner as usize], x + 12.0, y + 19.0, WHITE);
            }
            let days = if c.tally == 1 { line10(w, 0x56, 1) } else { line10(w, 0x57, c.tally as i32) };
            number(gu, &days, x + 0x65 as f32, y + 0x32 as f32);
            let (rx, mut ry) = (x + 12.0, y + 0x54 as f32);
            if c.beast_flag && !c.precise {
                gu.sprite("menu    framesus", 95, rx, ry, WHITE);
            } else {
                for k in 0..if c.precise { 4 } else { 3 } {
                    // FUN_101e9010: the label (the nation ahead pulsing), then a gauge or the level mark.
                    let on = c.precise && k == c.top as usize;
                    gu.sprite("menu    framesus", LABELS[k], rx, ry, if on { (m.pulse.0 as u32) << 24 | 0x80_8080 } else { WHITE });
                    let (gx, gy) = (rx + 20.0, ry + 13.0);
                    if c.precise {
                        gauge(gu, gx, gy, if k < 3 { c.levels[k] } else { c.beast }, !on);
                    } else if let Some(&s) = LEVELS.get(c.levels[k] as usize) {
                        gu.sprite("menu    keytops3", s, gx + 10.0, gy - 2.0, WHITE);
                    }
                    ry += 28.0;
                }
            }
        }
    }
    if m.conq6sta
        && let Some((x, y, _)) = info(gu, CONQ6STA, sw, sh, left, None)
        && d.have
    {
        // FUN_101e95d0: "%d." and the nation per rank, a tie on the next row; the alliance bracket.
        let mut rows = [0u8; 4];
        for k in 0..3 {
            let r = (d.byte(0) >> (2 * k) & 3) as usize;
            if r == 0 {
                continue;
            }
            let mut row = r - 1;
            while row < 3 && rows[row] != 0 {
                row += 1;
            }
            rows[row.min(3)] += 1;
            let ry = y + 0x14 as f32 + 16.0 * row as f32;
            gu.shape_text(&format!("{r}."), x + 8.0, ry, WHITE, false);
            gu.sprite("menu    framesus", LABELS[k], x + 0x18 as f32, ry, WHITE);
        }
        if d.byte(1) != 0 {
            gu.sprite("menu    keytops3", 119, x + 0x55 as f32, y + 0x24 as f32, WHITE);
        }
    }
    if m.beseige1
        && let Some((x, y, _)) = info(gu, BESEIGE1, sw, sh, left, Some((136.0, 206.0)))
    {
        // FUN_101e99c0: Al Zahbi's orders, then each army's level and orders.
        let ov = d.word(0x9c);
        gu.sprite("menu    keytops3", 370 + (ov >> 2 & 3) as usize, x + 8.0, y + 0x18 as f32, WHITE);
        for (k, (dy, lv, ord)) in [(0x44, ov >> 4 & 0xf, ov >> 16 & 7), (0x72, ov >> 8 & 0xf, ov >> 19 & 7), (0xa0, ov >> 12 & 0xf, ov >> 22 & 7)].into_iter().enumerate() {
            army(gu, x + 8.0, y + dy as f32, k, lv, ord);
        }
    }
    if let Some(k) = m.beseige2
        && let Some((x, y, wd)) = info(gu, BESEIGE2, sw, sh, left, Some((136.0, 206.0)))
    {
        // FUN_101e9c40: the stronghold, its forces (number and gauge), its army, mirrors and prisoners.
        let s = d.word(0xa0 + 4 * k);
        let x1 = x + wd;
        gu.sprite("menu    keytops3", 374 + k, x + 8.0, y + 0x18 as f32, WHITE);
        let forces = s >> 3 & 0xff;
        number(gu, &forces.to_string(), x1 - 8.0, y + 0x42 as f32);
        forces_gauge(gu, x + 8.0, y + 0x44 as f32, forces as i32);
        army(gu, x + 8.0, y + 0x54 as f32, k, s >> 11 & 0xf, s & 7);
        number(gu, &(s >> 15 & 0xf).to_string(), x1 - 8.0, y + 0xa2 as f32);
        number(gu, &(s >> 19 & 0xf).to_string(), x1 - 8.0, y + 0xb2 as f32);
    }
    if let Some(id) = m.coloresu
        && let Some((x, y, _)) = info(gu, COLORESU, sw, sh, left, None)
    {
        // FUN_101ea6e0 with FUN_101e6b80's area: the rate, its gauge (framesus 148), the bivouacs.
        let area = [(2, 0), (3, 1), (4, 2), (5, 4), (6, 3), (7, 5), (8, 6), (9, 7)].iter().find(|p| p.0 == id).map(|p| p.1);
        let (rate, biv, max) = area.map_or((0, 0, 0), |a| d.areas[a]);
        number(gu, &format!("{rate:3}%"), x + 100.0, y + 5.0);
        gu.sprite_scaled("menu    framesus", 148, x + 0x1f as f32, y + 0x13 as f32, rate as f32 * 0.01, 1.0, WHITE);
        number(gu, &format!("{biv}/{max}"), x + 100.0, y + 0x22 as f32);
    }
    if m.colorank
        && let Some((x, y, _)) = info(gu, COLORANK, sw, sh, left, None)
    {
        for (k, r) in d.ranks.iter().enumerate() {
            number(gu, &r.to_string(), x + 100.0, y + 0x14 as f32 + 16.0 * k as f32);
        }
    }
    // The legend (registry flag 0x2000: on the party window): conquest points, imperial standing, bayld.
    let legend = LEGENDS[m.mode];
    let shown = match m.mode {
        0 => m.legend && d.have && d.byte(0x94) == 0,
        1 => m.legend && d.have && d.word(0x98) != 0,
        2 => m.legend,
        _ => true,
    };
    if shown && let Some(win) = gu.lobby_window_off(legend, sw, sh, (0.0, 0.0), None, false, &[]) {
        let [x, y, ..] = win.frame;
        let v = match m.mode {
            0 => d.word(0x8c),
            1 => d.word(0xac),
            2 => d.campaign.as_ref().map_or(0, |c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
            _ => d.bayld,
        };
        number(gu, &format!("{v:10}"), x + 0x5f as f32, y + 0x16 as f32);
    }
}

/// FUN_101e97c0: the army (keytops3 367 + k), "Lv" (0xc 0x59) and its level, its orders (keytops3 377 + orders).
fn army(gu: &mut GameUi, x: f32, y: f32, k: usize, level: u32, orders: u32) {
    if k < 3 {
        gu.sprite("menu    keytops3", 367 + k, x, y, WHITE);
    }
    let lv = gu.lobby_msg(0xc, 0x59, "Lv");
    gu.shape_text(&lv, x + 8.0, y + 13.0, WHITE, false);
    number(gu, &level.to_string(), x + 0x2c as f32, y + 13.0);
    if orders < 6 {
        gu.sprite("menu    keytops3", 377 + orders as usize, x + 8.0, y + 26.0, WHITE);
    }
}

/// FUN_101e9af0: forces under 100 a blue fill (0x800060b0); 100..199 full blue under a white one; 200 and up full blue
/// and white under gold (0x80707000, blend 8); the frame over it *(inf: windowps 38 as FUN_101cb040's)*.
fn forces_gauge(gu: &mut GameUi, x: f32, y: f32, v: i32) {
    let f = if v < 100 { v as f32 * 0.01 } else { ((v - 100) % 101) as f32 * 0.01 };
    let (tint, blend) = match v {
        ..100 => (0x8000_60b0, 0),
        100..200 => {
            gu.sprite_scaled("menu    windowps", 97, x + 4.0, y + 1.0, 1.0, 1.0, 0x8000_60b0);
            (WHITE, 0)
        }
        _ => {
            gu.sprite_scaled("menu    windowps", 97, x + 4.0, y + 1.0, 1.0, 1.0, 0x8000_60b0);
            gu.sprite_scaled("menu    windowps", 97, x + 4.0, y + 1.0, 1.0, 1.0, WHITE);
            (0x8070_7000, 8)
        }
    };
    gu.sprite_scaled_blend("menu    windowps", 97, x + 4.0, y + 1.0, f, 1.0, tint, blend);
    gu.sprite("menu    windowps", 38, x, y, WHITE);
}
