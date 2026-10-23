//! The map window from the main menu (FUN_101ee8f0 -> FUN_101f0af0 mode 0): mapframe (the map, base draw
//! FUN_101e6ca0, draw FUN_101f1a40 / FUN_101f1a60) and map0 (Markers / Wide Scan / Change Map).

use super::World;
use super::game_ui::GameUi;
use crate::dat::DatIndex;
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
        .map(|r| Region { zone: s16(r, 0), key: r[2] as i8, flags: r[4], scale: r[5] as i8, ki: r[6] as i8, file: s16(r, 8), ox: s16(r, 10), oy: s16(r, 12) })
        .take_while(|r| r.scale != 0)
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

pub struct MapWin {
    pub region: Region,
    need: Need,
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
        Some(Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, Mode::Menu, w.game.get(0x8a), true))
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
        Some(Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), facing, sw, sh, mode, w.game.get(0x8a), show_player))
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
        self.need != Need::Nothing && matches!(self.mode, Mode::Menu | Mode::Other) && self.closing.is_none()
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
            self.closing = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            self.fade = (self.fade.0, 255.0, 16.0);
        }
    }

    /// map0's keys: up / down along its links (1..3), Escape closes both windows (FUN_101f14c0). Returns false when
    /// the map starts closing.
    pub fn key(&mut self, key: egui::Key) -> bool {
        if !self.has_menu() {
            return true;
        }
        match key {
            egui::Key::ArrowUp => self.cursor = if self.cursor <= 1 { 3 } else { self.cursor - 1 },
            egui::Key::ArrowDown => self.cursor = if self.cursor >= 3 { 1 } else { self.cursor + 1 },
            egui::Key::Escape => return false,
            egui::Key::Enter => log::info!("map0 item {} is not implemented yet", self.cursor),
            _ => {}
        }
        true
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
    pub fn update(&mut self, input: &mut super::input::Input, ticks: f32) -> bool {
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
            let v = (dead(input.axis(0x79) * 15.0) * fast * ticks * 0.5, dead(input.axis(0x7a) * 15.0) * fast * ticks * 0.5);
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
        if self.has_menu() {
            gu.menu_sel("menu    map0", sw, sh, self.cursor);
        }
    }
}

/// Event map requests, applied where the map records are: FUN_100b2570 opens, FUN_100b0dd0 closes, FUN_101fb070 views.
pub enum Cmd {
    /// /map (FUN_101f7810(0)): mode 0 as from the main menu.
    Menu,
    Open { zone: i16, key: i16, show_player: bool },
    Close,
    View { x: f32, y: f32, zf: f32, speed: f32, flags: i32 },
}

pub(super) fn apply(w: &mut World, regions: &[Region], c: Cmd, sw: f32, sh: f32) {
    match c {
        Cmd::Menu => match MapWin::open(w, regions, sw, sh) {
            Some(m) => w.map = Some(m),
            None => super::textcmd::refuse(w, 0x62),
        },
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
        Need::Item(id) => w.key_items.get((id >> 5) as usize).is_some_and(|word| word & (1 << (id & 31)) != 0),
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
        let region = Region { zone: 1, key: 0, flags: 0, scale: 1, ki: -1, file: 0, ox: -256, oy: -256 };
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
