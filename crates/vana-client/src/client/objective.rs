//! s2c 0x075 (FUN_10098e90; LSB's countdown / objectiveUtility): the countdown window "menu    cnttime", the
//! progress bars or Campaign scoreboard under the top-left windows, the objective text in the top bars' slot 3, and
//! the border's values.

use super::game_ui::GameUi;
use super::hudtop::BarText;
use super::{World, vana_now};
use std::time::Instant;

/// FUN_1012c660's colours: the time, under the warning, at 0.
const WHITE: u32 = 0x80_8080;
const RED: u32 = 0x80_4040;
const GREY: u32 = 0x60_6060;

/// The border (@0x14..@0x26, units x 0.001): centre, radius (0 none), how near it shows, colour (1 blue).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Border {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    pub render: f32,
    pub blue: bool,
}

/// The help ticker (FUN_10139f40's object, entry 0): +0x18 shown, +0xc the frames before it shows, +0x14 idle ticks.
#[derive(Clone, Debug, Default)]
struct Ticker {
    entry: Option<(u16, u16)>,
    shown: bool,
    delay: f32,
    idle: f32,
}

/// The store DAT_10480800.. (FUN_100878b0..FUN_10087a20), cleared by the zone load (FUN_10093af0).
#[derive(Clone, Debug, Default)]
pub struct Objective {
    /// @4: 0 drops the flags; 0xffff bars, 0xfffe bars whose gauge may be off, 0x1000 the scoreboard.
    pub mode: u32,
    /// 1 countdown, 2 data, 4 help to apply, 8 gate.
    pub flags: u8,
    /// @8 / @0xc / @0x10: the start (Vana'diel-epoch seconds), the length and the warning (0 = 60) in seconds.
    pub start: u32,
    pub duration: u32,
    pub warn: u32,
    pub border: Border,
    /// @0x26: the content gate.
    pub gate: u8,
    /// @0xa8 / @0xaa: group 0x1a title and description (+ 19).
    pub help: (u16, u16),
    /// @0x28: 128 bytes of bars or scoreboard.
    pub data: Vec<u8>,
    /// cnttime (+0x1c bit 0, +0x20, +0x18): flashing, its alpha, the second last drawn.
    flash: bool,
    alpha: f32,
    last: u32,
    ticker: Ticker,
    at: Option<Instant>,
}

fn u32_at(p: &[u8], o: usize) -> u32 {
    p.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn u16_at(p: &[u8], o: usize) -> u16 {
    p.get(o..o + 2).map_or(0, |b| u16::from_le_bytes(b.try_into().unwrap()))
}

impl Objective {
    /// FUN_10098e90 on the packet. Returns whether the border changed.
    pub fn apply(&mut self, p: &[u8]) -> bool {
        self.mode = u32_at(p, 4);
        if self.mode == 0 {
            self.flags = 0;
        }
        let f = p.get(0x24).copied().unwrap_or(0);
        if f & 1 != 0 {
            (self.start, self.duration, self.warn) = (u32_at(p, 8), u32_at(p, 0xc), u32_at(p, 0x10));
            self.flags |= 1;
        } else {
            self.flags &= !1;
        }
        if f & 2 != 0 {
            self.data = p.get(0x28..0xa8).map_or_else(|| vec![0; 128], <[u8]>::to_vec);
            self.flags |= 2;
        } else {
            self.flags &= !2;
        }
        if f & 4 != 0 {
            self.help = (u16_at(p, 0xa8), u16_at(p, 0xaa));
            self.flags |= 4;
        }
        if f & 8 != 0 {
            self.gate = p.get(0x26).copied().unwrap_or(0);
            self.flags |= 8;
        }
        // FUN_10087990: always stored; the player's border parts are remade (or dropped at radius 0).
        let milli = |o| u32_at(p, o) as i32 as f32 * 0.001;
        let b = Border { x: milli(0x14), z: milli(0x18), radius: u32_at(p, 0x1c) as f32 * 0.001, render: u32_at(p, 0x20) as f32 * 0.001, blue: p.get(0x25).copied().unwrap_or(0) != 0 };
        let changed = b != self.border;
        self.border = b;
        changed
    }

    /// cnttime is up (FUN_100873b0: a mode and the countdown flag).
    pub fn counting(&self) -> bool {
        self.mode != 0 && self.flags & 1 != 0
    }

    /// FUN_10103a50(start + length): the seconds left, 0 once past.
    pub fn left(&self, now: u32) -> u32 {
        let end = self.start.wrapping_add(self.duration);
        if end < now {
            let v = end.wrapping_sub(now).wrapping_sub(1);
            if v > 0x7fff_ffff { 0 } else { v }
        } else {
            end - now
        }
    }

    fn warn_at(&self) -> u32 {
        if self.warn == 0 { 60 } else { self.warn }
    }

    /// FUN_1012c660's text state for this frame: under the warning (not at 0) the alpha drops 6 a tick from 0xff,
    /// back to 0xff below 0x20 and on each new second (kept unrounded here, so the rate holds at any frame rate).
    fn step_flash(&mut self, left: u32, ticks: f32) {
        if left < self.warn_at() && left != 0 {
            self.flash = true;
        } else {
            (self.flash, self.alpha) = (false, 128.0);
        }
        if self.flash {
            let a = self.alpha - ticks * 6.0;
            self.alpha = if self.last != left || a < 32.0 { 255.0 } else { a };
        }
        self.last = left;
    }

    /// The time text's colour.
    fn colour(&self, left: u32) -> u32 {
        let base = if left == 0 {
            GREY
        } else if left < self.warn_at() {
            RED
        } else {
            WHITE
        };
        (self.alpha as u32).min(0xff) << 24 | base
    }
}

/// The zone load's reset (FUN_10093720 -> FUN_10093af0); the help ticker goes with the bars *(inf)*.
pub fn reset(w: &mut World) {
    w.objective = Objective::default();
}

/// The help ticker hides (FUN_1013a100): the slot clears, 0x5a frames (3 s at 30 a second) before it shows again.
fn hide(t: &mut Ticker) {
    t.shown = false;
    t.delay = 180.0;
}

/// Per frame before the top bars draw: cnttime's flash, the help entry from the store (FUN_100873b0) and the ticker
/// (FUN_1013a010, frames taken at 30 a second), which fills slot 3 of both bars.
/// `secs`: the game clock (vana_now); `busy`: FUN_10087070(0).
pub fn frame(gu: &GameUi, o: &mut Objective, secs: u32, busy: bool, bars: &mut BarText) {
    let now = Instant::now();
    let ticks = o.at.map_or(0.0, |a| (now - a).as_secs_f32().min(0.5) * 60.0);
    o.at = Some(now);
    if o.counting() {
        let left = o.left(secs);
        o.step_flash(left, ticks);
    }
    if o.flags & 4 != 0 {
        let (title, desc) = o.help;
        let t = &mut o.ticker;
        if title != 0 && desc != 0 {
            // FUN_10139f40: a replaced entry hides first; then shown at once (FUN_10139f90: 1 frame, idle 0x5a).
            if t.entry.is_some() {
                hide(t);
            }
            t.entry = Some((title, desc));
            (t.delay, t.idle) = (0.0, 0x5a as f32);
        } else if t.entry.take().is_some() {
            hide(t);
        }
        o.flags &= !4;
    }
    let t = &mut o.ticker;
    // FUN_1013a070: never while busy (FUN_10087070(0)); on while shown or both bars hold text, else every 0x5a ticks.
    let active = !super::hudtop::shown(&bars.title).is_empty() && !super::hudtop::shown(&bars.help).is_empty();
    let ok = !busy
        && (t.shown || active || {
            t.idle += ticks;
            if t.idle > 0x59 as f32 {
                t.idle = 0.0;
                true
            } else {
                false
            }
        });
    if ok {
        t.idle = 0.0;
        t.shown = true;
        if t.delay > 0.0 {
            t.delay -= ticks;
        }
    } else if t.shown {
        hide(t);
    }
    if t.shown
        && t.delay <= 0.0
        && let Some((title, desc)) = t.entry
    {
        bars.title[3] = gu.lobby_msg(0x1a, title as usize, "");
        bars.help[3] = gu.lobby_msg(0x1a, desc as usize + 19, "");
    }
}

/// cnttime's top (FUN_1012c7f0): under casttime while it is up, else at 94 under gaugewin, else 47.
fn countdown_top(gu: &GameUi, w: &World) -> f32 {
    let gauge = w.gauge.is_some();
    match w.cast_bar {
        Some(_) => {
            let h = gu.layout("menu    casttime").map_or(24.0, |m| m.elements[0].h as f32);
            (if gauge { 94.0 } else { 47.0 }) + h
        }
        None if gauge => 94.0,
        None => 47.0,
    }
}

/// "menu    cnttime" (FUN_1012c660): the frame (framesus 196), the sand glass (framesus 197) at (+4, +4), " %s" of
/// the time left right-aligned at the right edge - 16, y + 6, in the sprite font.
pub fn draw_countdown(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    let o = &w.objective;
    if !o.counting() {
        return;
    }
    let left = o.left(vana_now(w) as u32);
    let top = countdown_top(gu, w);
    let (b, ui, style, _) = gu.parts();
    let Some(m) = ui.menu("menu    cnttime") else { return };
    let (x, y) = crate::render::screens::push_window_at(b, ui, style, m, sw, sh, top - m.elements[0].y as f32);
    let right = x + m.elements[0].w as f32;
    let text = format!(" {}", super::magic::clock(left as f32));
    crate::render::screens::push_shape_text_right(b, ui, text.as_bytes(), right - 16.0, y + 6.0, o.colour(left));
    gu.sprite("menu    framesus", 197, x + 4.0, y + 4.0, 0x8080_8080);
}

/// The countdown for the modern panel: the fraction gone, the time, under the warning, the text colour (ARGB, 0x80 =
/// 1.0 alpha).
pub fn countdown(w: &World) -> Option<(f32, String, bool, u32)> {
    let o = &w.objective;
    if !o.counting() {
        return None;
    }
    let left = o.left(vana_now(w) as u32);
    let gone = if o.duration > 0 { 1.0 - left as f32 / o.duration as f32 } else { 1.0 };
    Some((gone.clamp(0.0, 1.0), super::magic::clock(left as f32), o.flash, o.colour(left)))
}

/// FUN_1021f360: the bottom of the first window up of cnttime, casttime, gaugewin and titlewin.
fn anchor(gu: &GameUi, w: &World) -> f32 {
    let bottom = |name: &str, top: Option<f32>| gu.layout(name).map(|m| top.unwrap_or(m.elements[0].y as f32) + m.elements[0].h as f32);
    let cast_top = if w.gauge.is_some() { 94.0 } else { 47.0 };
    let b = if w.objective.counting() {
        bottom("menu    cnttime", Some(countdown_top(gu, w)))
    } else if w.cast_bar.is_some() {
        bottom("menu    casttime", Some(cast_top))
    } else if w.gauge.is_some() {
        bottom("menu    gaugewin", None)
    } else {
        bottom("menu    titlewin", None)
    };
    b.unwrap_or(46.0)
}

/// A bar row of the data: its progress and name (Shift-JIS, up to 16 bytes).
pub fn bar_rows(o: &Objective) -> Vec<(i32, String)> {
    (0..6)
        .filter_map(|k| {
            let r = o.data.get(k * 20..k * 20 + 20)?;
            let name = &r[4..20];
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(16)];
            (!name.is_empty()).then(|| (u32_at(r, 0) as i32, crate::dat::font::decode_sjis(name)))
        })
        .collect()
}

/// The scoreboard (mode 0x1000): the scores and both sides' progress over their maximum, the second side's name.
pub struct Score {
    pub scores: (i32, i32),
    pub marchland: f32,
    pub stronghold: f32,
    pub second: &'static str,
}

pub fn score(o: &Objective) -> Score {
    let d = |k: usize| u32_at(&o.data, k * 4);
    let frac = |v: u32, max: u32| if max != 0 { v as i32 as f32 / max as f32 } else { 0.0 };
    Score {
        scores: (d(0) as i32, d(1) as i32),
        marchland: frac(d(2), d(3)),
        stronghold: frac(d(4), d(5)),
        second: if d(7) != 0 { "Balamor's Adumbration" } else { "Stronghold" },
    }
}

/// Bars and scoreboard as they show (FUN_1021fe10 from the compass draw), None when the store has none.
pub fn shown(o: &Objective) -> Option<u32> {
    (o.flags & 2 != 0 && matches!(o.mode, 0xffff | 0xfffe | 0x1000)).then_some(o.mode)
}

/// FUN_1021fe10: rows 16 apart from the anchor's bottom + 12: the name at (x + 16, y - 8), the gauge (windowps 38 / 97,
/// progress x 0.01) at (x + 24, y); mode 0xfffe draws no gauge for a negative progress. Mode 0x1000 (FUN_10220010):
/// keytops3 540 at (x, b + 4), "%2d " / "%2d" of the scores either side of x + 40, keytops3 541 at x + 56, then
/// Marchland's and the other side's rows (icon at (x, y), name at (x + 16, y), gauge at (x + 24, y + 8)) at b + 20, b + 36.
pub fn draw_bars(gu: &mut GameUi, w: &World) {
    let o = &w.objective;
    let Some(mode) = shown(o) else { return };
    let (x, b) = (16.0, anchor(gu, w));
    let gauge = |gu: &mut GameUi, gx: f32, gy: f32, f: f32| {
        let (bt, ui, _, _) = gu.parts();
        if let Some(l) = ui.lib("menu    windowps") {
            crate::render::screens::push_gauge_eased(bt, ui, l, gx, gy, 38, 97, f, f);
        }
    };
    if mode == 0x1000 {
        let s = score(o);
        let y = b + 4.0;
        gu.sprite("menu    keytops3", 540, x, y, 0x8080_8080);
        {
            let (bt, ui, _, _) = gu.parts();
            crate::render::screens::push_shape_text_right(bt, ui, format!("{:2} ", s.scores.0).as_bytes(), x + 40.0, y, 0x8080_8080);
            crate::render::screens::push_shape_text(bt, ui, format!("{:2}", s.scores.1).as_bytes(), x + 40.0, y, 0x8080_8080);
        }
        gu.sprite("menu    keytops3", 541, x + 56.0, y, 0x8080_8080);
        for (k, (icon, f, name)) in [(540, s.marchland, "Marchland"), (541, s.stronghold, s.second)].into_iter().enumerate() {
            let ry = b + 20.0 + 16.0 * k as f32;
            gu.sprite("menu    keytops3", icon, x, ry, 0x8080_8080);
            gauge(gu, x + 24.0, ry + 8.0, f);
            let (bt, ui, _, _) = gu.parts();
            crate::render::screens::push_shape_text(bt, ui, name.as_bytes(), x + 16.0, ry, 0x8080_8080);
        }
        return;
    }
    let data = &o.data;
    for k in 0..6 {
        let Some(r) = data.get(k * 20..k * 20 + 20) else { break };
        let name = &r[4..20];
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(16)];
        if name.is_empty() {
            continue;
        }
        let y = b + 12.0 + 16.0 * k as f32;
        let p = u32_at(r, 0) as i32;
        if mode == 0xffff || p >= 0 {
            gauge(gu, x + 24.0, y, p as f32 * 0.01);
        }
        let (bt, ui, _, _) = gu.parts();
        crate::render::screens::push_shape_text(bt, ui, name, x + 16.0, y - 8.0, 0x8080_8080);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB's countdown(90) and objectiveUtility bars / help / border, as GP_SERV_COMMAND_BATTLEFIELD builds them.
    fn packet(flags: u8, mode: u32) -> Vec<u8> {
        let mut p = vec![0u8; 0xac];
        p[4..8].copy_from_slice(&mode.to_le_bytes());
        p[8..12].copy_from_slice(&1000u32.to_le_bytes());
        p[0xc..0x10].copy_from_slice(&90u32.to_le_bytes());
        p[0x14..0x18].copy_from_slice(&(-12500i32).to_le_bytes());
        p[0x1c..0x20].copy_from_slice(&25000u32.to_le_bytes());
        p[0x24] = flags;
        p[0x25] = 1;
        p[0x28..0x2c].copy_from_slice(&40u32.to_le_bytes());
        p[0x2c..0x36].copy_from_slice(b"Motivation");
        p[0xa8..0xaa].copy_from_slice(&1u16.to_le_bytes());
        p[0xaa..0xac].copy_from_slice(&43u16.to_le_bytes());
        p
    }

    #[test]
    fn apply_and_left() {
        let mut o = Objective::default();
        assert!(o.apply(&packet(1 | 2 | 4, 0xffff)));
        assert!(o.counting());
        assert_eq!((o.left(1000), o.left(1089), o.left(1090), o.left(2000)), (90, 1, 0, 0));
        assert_eq!(o.help, (1, 43));
        assert_eq!(bar_rows(&o), vec![(40, "Motivation".to_string())]);
        let b = o.border;
        assert!((b.x + 12.5).abs() < 1e-4 && (b.radius - 25.0).abs() < 1e-4 && b.blue);
        // A clear: mode 0 drops every flag; the border unchanged is not a change.
        let mut c = packet(0, 0);
        c[0x14..0x18].copy_from_slice(&(-12500i32).to_le_bytes());
        assert!(!o.apply(&c));
        assert!(!o.counting() && shown(&o).is_none());
    }

    #[test]
    fn flash_pulses() {
        let mut o = Objective::default();
        o.step_flash(61, 2.0);
        assert_eq!((o.flash, o.alpha), (false, 128.0));
        o.step_flash(59, 2.0);
        assert_eq!(o.alpha, 255.0);
        // 0xff - 6 x 36 = 39, then below 0x20 back to 0xff.
        for _ in 0..36 {
            o.step_flash(59, 1.0);
        }
        assert_eq!(o.alpha, 39.0);
        o.step_flash(59, 2.0);
        assert_eq!(o.alpha, 255.0);
        assert_eq!(o.colour(59) & 0xff_ffff, RED);
        o.step_flash(0, 1.0);
        assert_eq!((o.flash, o.colour(0)), (false, 0x8060_6060));
    }
}
