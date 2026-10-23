//! The mouse cursor (mouse object DAT_104e1d4c): its kind from FUN_10125360 and its draw, FUN_10124eb0. Config 0021
//! picks the game's own sprite cursor (0, the default) or the game folder's Windows cursors (.ani, LoadCursorFromFileA).

use super::game_ui::GameUi;
use std::sync::Arc;
use winit::window::{CustomCursor, Window};

/// Kinds (+0x4c): 0 normal, 1 over a menu item, 2 over an actor, 3 scroll-thumb drag, 4 move drag, 5 camera drag,
/// 6 middle-button autoscroll.
pub const NORMAL: u8 = 0;
pub const OVER_ITEM: u8 = 1;
pub const OVER_ACTOR: u8 = 2;
pub const MOVE_DRAG: u8 = 4;
pub const CAMERA_DRAG: u8 = 5;

/// The sprite table at 0x1036e4c8: library, tint (0x80 = 1.0) and scale per kind.
const SPRITES: [(&str, u32, f32); 7] = [
    ("anc     anc_m1", 0x8080_8080, 1.0),
    ("anc     anc_m2", 0x8080_8080, 1.0),
    ("anc     anc_m2", 0x8080_8080, 1.0),
    ("anc     anc_scro", 0x8080_8080, 1.0),
    ("anc     kaipage", 0x8080_8080, 2.0),
    ("anc     kaipage", 0x8020_2080, 2.0),
    ("anc     anc_m1", 0x8080_8080, 1.0),
];

/// The Windows cursors at 0x1034fbb0: 0 normal, 1 hit, 2..9 oarw (loaded, never used), 10..17 arw_n..arw_nw.
const FILES: [&str; 18] = [
    "mousenor.ani", "mousehit.ani", "oarw_n.ani", "oarw_ne.ani", "oarw_e.ani", "oarw_se.ani", "oarw_s.ani", "oarw_sw.ani", "oarw_w.ani", "oarw_nw.ani",
    "arw_n.ani", "arw_ne.ani", "arw_e.ani", "arw_se.ani", "arw_s.ani", "arw_sw.ani", "arw_w.ani", "arw_nw.ani",
];

/// One loaded Windows cursor: a winit cursor per image and its steps (image, jiffies).
struct Hw {
    frames: Vec<CustomCursor>,
    steps: Vec<(usize, u32)>,
}

#[derive(Default)]
pub struct MouseCursor {
    /// +0x4e: shown on motion or a mouse button, hidden by other keys and at start.
    pub visible: bool,
    pub kind: u8,
    /// Row 4 / 5 rotation (+0x2c), radians.
    pub angle: f32,
    /// +0xa6: the Windows cursor for kinds 4 / 5, set only by the camera drag.
    hw_index: usize,
    /// Per-row frame counters (+0x30) and the 30 Hz step clock.
    frames: [usize; 7],
    shown: [usize; 7],
    acc: f32,
    hw: Option<Vec<Option<Hw>>>,
    hw_applied: Option<(usize, usize, bool)>,
    epoch: Option<std::time::Instant>,
}

impl MouseCursor {
    /// FUN_10125360's arrow for a camera drag from `from` to `at`: angle atan2(dy, dx) - pi/2 and +0xa6 by 45 deg bins.
    pub fn camera_drag(&mut self, from: (f32, f32), at: (f32, f32)) {
        let (dx, dy) = (at.0 - from.0, at.1 - from.1);
        let mut a = dy.atan2(dx) - 1.57075;
        if a < -3.1415 {
            a += 6.283;
        }
        self.angle = a;
        let bins = [-2.7488f32, -1.9634, -1.1781, -0.3927, 0.3927, 1.1781, 1.9634, 2.7488];
        self.hw_index = 10 + bins.iter().position(|b| a < *b).unwrap_or(8) % 8;
        self.kind = CAMERA_DRAG;
    }

    /// The sprite cursor at UI pixel (x, y), last over every window, stepping one frame each 1/30 s (retail: each
    /// drawn frame at 30 fps). The cycle runs 0..n-1 then shows 0 twice (a null sprite resets the counter).
    pub fn draw(&mut self, gu: &mut GameUi, x: f32, y: f32, dt: f32) {
        if !self.visible {
            return;
        }
        let k = self.kind.min(6) as usize;
        let (lib, tint, scale) = SPRITES[k];
        let n = gu.lib_len(lib);
        if n == 0 {
            return;
        }
        self.acc += dt * 30.0;
        let steps = self.acc.floor() as usize;
        self.acc -= steps as f32;
        for _ in 0..steps.min(n + 1) {
            if self.frames[k] < n {
                self.shown[k] = self.frames[k];
                self.frames[k] += 1;
            } else {
                self.frames[k] = 0;
                self.shown[k] = 0;
            }
        }
        let shown = self.shown[k].min(n - 1);
        let (s, c) = if matches!(self.kind, MOVE_DRAG | CAMERA_DRAG) { self.angle.sin_cos() } else { (0.0, 1.0) };
        gu.sprite_affine(lib, shown, x - 0.5, y - 0.5, [c * scale, -s * scale, s * scale, c * scale], tint);
    }

    /// The Windows cursor for this frame (FUN_100097f0): index 0 for kinds 0 / 3 / 6, 1 for 1 / 2, +0xa6 for 4 / 5;
    /// the .ani steps by its jiffies. Files come through the mods (DatIndex::root_file); a missing one is the system
    /// arrow.
    pub fn apply_hardware(&mut self, window: &Window, event_loop: &winit::event_loop::ActiveEventLoop, index: Option<&Arc<crate::dat::DatIndex>>) {
        let hw = self.hw.get_or_insert_with(|| {
            FILES
                .iter()
                .map(|name| {
                    let path = index?.root_file(name);
                    let c = crate::dat::cursor::parse(&std::fs::read(&path).ok()?).map_err(|e| log::warn!("cursor {}: {e:#}", path.display())).ok()?;
                    let frames = c
                        .frames
                        .iter()
                        .filter_map(|f| CustomCursor::from_rgba(f.rgba.clone(), f.width as u16, f.height as u16, f.hotspot.0.min(f.width as u16 - 1), f.hotspot.1.min(f.height as u16 - 1)).ok())
                        .map(|src| event_loop.create_custom_cursor(src))
                        .collect::<Vec<_>>();
                    (frames.len() == c.frames.len()).then_some(Hw { frames, steps: c.steps })
                })
                .collect()
        });
        let idx = match self.kind {
            OVER_ITEM | OVER_ACTOR => 1,
            MOVE_DRAG | CAMERA_DRAG => self.hw_index,
            _ => 0,
        };
        let clock = self.epoch.get_or_insert_with(std::time::Instant::now).elapsed().as_secs_f32() * 60.0;
        let step = hw.get(idx).and_then(|h| h.as_ref()).map_or(0, |h| {
            let total: u32 = h.steps.iter().map(|s| s.1.max(1)).sum();
            let mut t = (clock as u32) % total.max(1);
            h.steps.iter().position(|s| {
                let hit = t < s.1.max(1);
                t = t.saturating_sub(s.1.max(1));
                hit
            }).unwrap_or(0)
        });
        let want = (idx, step, self.visible);
        if self.hw_applied == Some(want) {
            return;
        }
        self.hw_applied = Some(want);
        window.set_cursor_visible(self.visible);
        match hw.get(idx).and_then(|h| h.as_ref()) {
            Some(h) => window.set_cursor(h.frames[h.steps[step].0.min(h.frames.len() - 1)].clone()),
            None => window.set_cursor(winit::window::CursorIcon::Default),
        }
    }

    /// Something else (egui, the window re-entering) set the system cursor: apply ours again next frame.
    pub fn invalidate(&mut self) {
        self.hw_applied = None;
    }
}

#[cfg(test)]
mod tests {
    /// Camera-drag arrows: down the screen is arw_s, right arw_e, up arw_n; no motion points right (index 12).
    #[test]
    fn drag_arrows() {
        let mut c = super::MouseCursor::default();
        for (to, idx) in [((0.0, 10.0), 14), ((10.0, 0.0), 12), ((0.0, -10.0), 10), ((-10.0, 0.0), 16), ((0.0, 0.0), 12)] {
            c.camera_drag((0.0, 0.0), to);
            assert_eq!(c.hw_index, idx, "{to:?}");
        }
    }
}
