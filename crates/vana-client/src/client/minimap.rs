//! Extra (not retail; the Windower / Ashita minimap add-ons): the zone map in a frame in a corner, north up and centred
//! on us, with our marker and the compass radar's dots (monsters included) for what stands in view. The wheel over it
//! zooms. Nothing without the map's key item, as the map window.

use super::game_ui::GameUi;
use super::map::{Overlay, Region};
use super::{World, event, radar};

/// The sides of Small / Medium / Large (layout pixels; user, 2026-10-25: 20 % over the first 128 / 176 / 224).
pub const SIZES: [f32; 3] = [154.0, 211.0, 269.0];
/// Map pixels across: the wheel's range (the whole map is 512).
pub const SPAN: (f32, f32) = (48.0, 512.0);

/// Where it goes (layout pixels): 0 top right, at the screen's edge, or left of the main menu column (W - 128) while
/// the main menu is open; 1 top left, under the cast bar and past the status icons when they reach over (as the Timers'
/// left dock).
pub fn place(dock: u32, size: u32, sw: f32, icons: usize, menu_open: bool) -> [f32; 4] {
    let side = SIZES[size.min(2) as usize];
    match dock {
        1 => {
            let icons_foot = if icons == 0 { 0.0 } else { 48.0 + 2.0 + 26.0 * icons.div_ceil(9) as f32 + 4.0 };
            let top = 122f32.max(if 16.0 + side > 142.0 { icons_foot } else { 0.0 });
            [16.0, top, side, side]
        }
        _ if menu_open => [sw - 132.0 - side, 50.0, side, side],
        _ => [sw - side, 50.0, side, side],
    }
}

/// What one frame draws: the rect, where we stand and face, the map pixels across, and the dots (world x, z, colour,
/// the target).
pub struct Frame {
    rect: [f32; 4],
    pos: (f32, f32),
    facing: f32,
    span: f32,
    dots: Vec<(f32, f32, u32, bool)>,
    /// The map overlay's opacity: the marker is coloured as the overlay's.
    mix: f32,
}

/// Each frame before the Timers: the map for where we stand (another zone opens it again, another page follows), and
/// the rect it takes when it draws (hidden in events, under the map window, without a map).
pub fn prepare(gu: &GameUi, w: &mut World, sw: f32) {
    w.minimap_rect = None;
    w.minimap_frame = None;
    if !w.extra_minimap {
        w.minimap = None;
        return;
    }
    let regions = gu.map_regions();
    match w.minimap.take() {
        Some(mut m) if m.zone == w.pos_zone => {
            m.follow(w, regions);
            w.minimap = Some(m);
        }
        _ => w.minimap = Some(Overlay::open(w, regions)),
    }
    if event::active(w) || w.map.is_some() || w.cnq.is_some() || !w.minimap.as_ref().is_some_and(Overlay::has_map) {
        return;
    }
    let icons = w.buffs.iter().filter(|&&b| b != 0xffff && b != 0).count();
    let rect = place(w.minimap_dock, w.minimap_size, sw, icons, w.menu.is_some());
    let facing = w.net.me.lock().map_or(0.0, |m| event::rad_of(m.dir));
    let dots = w
        .entities
        .iter()
        .filter(|(id, e)| **id != w.my_id && radar::shown(e))
        .filter_map(|(&id, e)| Some((e.pos.x, e.pos.z, radar::colour(radar::kind_bits(w, id, e), e.monstrosity, e.motion.status, true)? | 0x8000_0000, w.target == Some(id))))
        .collect();
    w.minimap_rect = Some(rect);
    w.minimap_frame = Some(Frame { rect, pos: (w.pos.x, w.pos.z), facing, span: w.minimap_span.clamp(SPAN.0, SPAN.1), dots, mix: w.map_opacity });
}

/// Draws what `prepare` set up (after the HUD's clear, under the party / target windows and the menus).
pub fn draw(gu: &mut GameUi, map: &mut Option<Overlay>, f: Option<Frame>) {
    if let (Some(m), Some(f)) = (map.as_mut(), f) {
        m.draw_mini(gu, f.rect, f.pos, f.facing, f.span, &f.dots, f.mix);
    }
}

/// The wheel over it: notches up zoom in (fewer map pixels across); true when it took the wheel.
pub fn wheel(w: &mut World, p: (f32, f32), notches: f32) -> bool {
    let Some([x, y, sw, sh]) = w.minimap_rect else { return false };
    if p.0 < x || p.0 >= x + sw || p.1 < y || p.1 >= y + sh {
        return false;
    }
    w.minimap_span = (w.minimap_span * 1.25f32.powf(-notches)).clamp(SPAN.0, SPAN.1);
    w.minimap_dirty = true;
    true
}

/// A map record's world-to-map scale: map pixels per world unit (Region::px without the rounding).
pub fn px_per_unit(r: &Region) -> f32 {
    r.scale.unsigned_abs() as f32 / 5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places() {
        // Top right: at the edge, left of the main menu column while the menu is open.
        assert_eq!(place(0, 1, 1280.0, 0, false), [1280.0 - 211.0, 50.0, 211.0, 211.0]);
        assert_eq!(place(0, 1, 1280.0, 0, true), [1280.0 - 132.0 - 211.0, 50.0, 211.0, 211.0]);
        assert_eq!(place(1, 0, 1280.0, 3, false), [16.0, 122.0, 154.0, 154.0]);
        // The status icons reach past the cast bar when a large map stands under them.
        assert_eq!(place(1, 2, 1280.0, 20, false), [16.0, 48.0 + 2.0 + 78.0 + 4.0, 269.0, 269.0]);
    }

    /// px_per_unit matches Region::px (Jugner: scale 1, x / 5).
    #[test]
    fn scale() {
        let r = Region { zone: 104, key: 0, num: 0, flags: 0, scale: 1, ki: 1, file: 5, ox: -264, oy: -256 };
        let ((x, y), s) = (r.px(-200.0, -1050.0), px_per_unit(&r));
        assert!((x as f32 + 200.0 * s).abs() <= 1.0 && (y as f32 - 1050.0 * s).abs() <= 1.0, "{x} {y}");
    }
}
