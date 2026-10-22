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

pub struct MapWin {
    pub region: Region,
    need: Need,
    /// map0's cursor child (1..3).
    pub cursor: u16,
    zoom: f32,
    /// View origin in map pixels (+0x24 / +0x28) and the marker shift (+0x6c: the origin when the no-map
    /// image is shown).
    view: (f32, f32),
    shift: (f32, f32),
    texture: Option<String>,
    rx: Option<Receiver<Loaded>>,
    /// Player marker alpha 0..0x80 and its direction (flags 0x9f: fade in, blink at 6 a tick).
    alpha: f32,
    rising: bool,
    last: Option<std::time::Instant>,
}

impl MapWin {
    /// Opens the map of the zone (FUN_101f0af0 mode 0). None when the zone has no record.
    pub fn open(w: &World, regions: &[Region], sw: f32, sh: f32) -> Option<Self> {
        let region = Region::find(regions, w.pos_zone)?;
        let owned = owned(w, &region);
        log::info!("map: zone {} owned {owned}", w.pos_zone);
        Some(Self::new(region, owned, w.index.clone(), (w.pos.x, w.pos.z), sw, sh))
    }

    /// The windows for a region, the map key item owned or not, the player at world (x, z).
    pub fn new(region: Region, owned: bool, index: Option<Arc<DatIndex>>, pos: (f32, f32), sw: f32, sh: f32) -> Self {
        let need = region.need();
        let file = if owned { region.file_id() } else { NO_MAP };
        let (base, shift) = if owned { ((region.ox as f32, region.oy as f32), (0.0, 0.0)) } else { ((0.0, 0.0), (region.ox as f32, region.oy as f32)) };
        // Zoom W / 512 times config 0x8a / 1000 (default 2000); only W / 512 for the no-map case.
        let zoom = sw / 512.0 * if need == Need::Nothing { 1.0 } else { 2.0 };
        // FUN_101e7a60: the player under the cursor point (W * 200 / 512, H / 2), clamped to the map (FUN_101e6110).
        let (px, py) = region.px(pos.0, pos.1);
        let (cx, cy) = (sw * 200.0 / 512.0, sh / 2.0);
        let clamp = |v: f32, b: f32, extent: f32| v.clamp(b, b + (512.0 - extent / zoom).max(0.0));
        let view = (clamp(px as f32 - shift.0 - cx / zoom, base.0, sw), clamp(py as f32 - shift.1 - cy / zoom, base.1, sh));
        let rx = index.map(|index| load(index, file));
        Self { region, need, cursor: 1, zoom, view, shift, texture: None, rx, alpha: 0.0, rising: true, last: None }
    }

    /// Waits for the image and shows the marker at full alpha (offscreen shots).
    pub fn wait_image(&mut self, gu: &mut GameUi) {
        (self.alpha, self.rising) = (128.0, false);
        if let Some(Some((name, iw, ih, rgba))) = self.rx.take().and_then(|r| r.recv().ok()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
        }
    }

    /// map0 is open (not for a zone with no map, as FUN_101f0af0 decompiles).
    pub fn has_menu(&self) -> bool {
        self.need != Need::Nothing
    }

    /// map0's keys: up / down along its links (1..3), Escape closes both windows (FUN_101f14c0). Returns
    /// false when the map closes.
    pub fn key(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::ArrowUp => self.cursor = if self.cursor <= 1 { 3 } else { self.cursor - 1 },
            egui::Key::ArrowDown => self.cursor = if self.cursor >= 3 { 1 } else { self.cursor + 1 },
            egui::Key::Escape => return false,
            egui::Key::Enter => log::info!("map0 item {} is not implemented yet", self.cursor),
            _ => {}
        }
        true
    }

    /// mapframe: the image (0xa0404040, alpha blended, linear) at (base - view) * zoom, the player marker, the
    /// grid text under titlewin; then map0.
    /// `pos` is the player's world (x, z), `facing` its heading in radians (forward (cos, 0, -sin)).
    pub fn draw(&mut self, gu: &mut GameUi, pos: (f32, f32), facing: f32, sw: f32, sh: f32) {
        if let Some(Ok(Some((name, iw, ih, rgba)))) = self.rx.as_ref().map(|r| r.try_recv()) {
            gu.image(&name, iw, ih, rgba);
            self.texture = Some(name);
            self.rx = None;
        }
        let z = self.zoom;
        let (bx, by) = if self.shift == (0.0, 0.0) { (self.region.ox as f32, self.region.oy as f32) } else { (0.0, 0.0) };
        if let Some(t) = &self.texture {
            gu.image_rect(t, (bx - self.view.0) * z + 0.5, (by - self.view.1) * z + 0.5, 512.0 * z, 512.0 * z, [0.5, 0.5, 0.5, 1.0]);
        }
        let now = std::time::Instant::now();
        let ticks = self.last.map_or(0.0, |l| (now - l).as_secs_f32().min(0.5) * 60.0);
        self.last = Some(now);
        if self.need != Need::Nothing {
            // FUN_101e6880: fade in 6 a tick, then blink between 0 and 0x80 at the same speed.
            self.alpha += if self.rising { 6.0 } else { -6.0 } * ticks;
            if self.alpha >= 128.0 {
                (self.alpha, self.rising) = (128.0, false);
            } else if self.alpha <= 0.0 {
                (self.alpha, self.rising) = (0.0, true);
            }
            let (px, py) = self.region.px(pos.0, pos.1);
            let sx = (px as f32 - self.shift.0 - self.view.0) * z + 0.5;
            let sy = (py as f32 - self.shift.1 - self.view.1) * z + 0.5;
            // The sprite points east; turned to our facing (forward (cos r, 0, -sin r) is (cos r, sin r) on the map).
            let (s, c) = facing.sin_cos();
            gu.sprite_affine("menu    keytops3", MARKER, sx, sy, [c, -s, s, c], (self.alpha as u32) << 24 | 0x80_8080);
            // "(G-9)" at (titlewin x0, titlewin y1 + 3) in fontshp.
            gu.shape_text(&self.region.grid(px, py), 16.0, 49.0, 0x8080_8080, false);
        }
        if self.has_menu() {
            gu.menu_sel("menu    map0", sw, sh, self.cursor);
        }
    }
}

/// The region's key item is held, or it needs none (FUN_101ee560).
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
        let region = Region::find(regions, w.pos_zone).filter(|r| r.need() != Need::Nothing && owned(w, r));
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
