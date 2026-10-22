//! The map window from the main menu (FUN_101ee8f0 -> FUN_101f0af0 mode 0): mapframe (the map, base draw
//! FUN_101e6ca0, draw FUN_101f1a40 / FUN_101f1a60) and map0 (Markers / Wide Scan / Change Map).

use super::World;
use super::game_ui::GameUi;
use crate::dat::DatIndex;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// Region table: 14-byte records from 0x10376010, ended by a record with scale (+5) 0.
const REGIONS_VA: u32 = 0x1037_6010;
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
    let Some(b) = crate::dat::battle::va_bytes(ffximain, REGIONS_VA, REGIONS_MAX * 14) else { return Vec::new() };
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
        let owned = match region.need() {
            Need::Item(id) => w.key_items.get((id >> 5) as usize).is_some_and(|word| word & (1 << (id & 31)) != 0),
            _ => true,
        };
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
