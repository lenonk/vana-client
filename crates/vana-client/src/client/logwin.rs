//! The chat log window (logwindo, base class FUN_1015b250) and the input line (msgline): width from the UI
//! width, height from the visible line count, auto-shrink, slide-in, the channel caption.

use super::game_ui::GameUi;

/// Window 1's config: 0x0f max lines, 0x10 min lines, 0x11 seconds per line, 0x9c width percent, 0x0e fixed size
/// (FUN_1015c180..FUN_1015c220; fixed: min = max).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogCfg {
    pub max: f32,
    pub min: f32,
    pub ticks: f32,
    pub pct: f32,
}

impl Default for LogCfg {
    fn default() -> Self {
        Self::from_config(&crate::gamecfg::Values::default())
    }
}

impl LogCfg {
    pub fn from_config(v: &crate::gamecfg::Values) -> Self {
        // FUN_1015c1a0: max outside 4..24 is 24; FUN_1015c220: under 1 s is 20 ticks.
        let max = match v.get_i(0x0f) {
            m @ 4..=24 => m as f32,
            _ => 24.0,
        };
        let min = if v.get(0x0e) != 0 { max } else { (v.get_i(0x10).max(0) as f32).min(max) };
        let secs = v.get_i(0x11);
        Self { max, min, ticks: if secs < 1 { 20.0 } else { secs as f32 * 60.0 }, pct: v.get_i(0x9c).clamp(0, 100) as f32 }
    }
}
/// Wrapped lines kept for drawing.
const KEEP: usize = 512;

/// Timestamps: retail's mode (config 0xb5, chat lines only), the extra's every-line switch, and their colour (0x31).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stamps {
    pub mode: u32,
    pub all: bool,
    pub colour: [u8; 4],
}

impl Stamps {
    fn text(&self, time: [u8; 3], chat: bool) -> String {
        match (self.all, chat) {
            (true, _) => super::cfgapply::stamp_text(if self.mode == 0 { 2 } else { self.mode }, time),
            (false, true) => super::cfgapply::stamp_text(self.mode, time),
            _ => String::new(),
        }
    }
}

#[derive(Default)]
pub struct LogWin {
    /// Log lines wrapped to the text width (with the bytes of a timestamp at the start), the log lines consumed,
    /// and the width and timestamps they were wrapped with.
    wrapped: Vec<(String, [u8; 4], usize)>,
    seen: usize,
    width: f32,
    stamps: Stamps,
    /// +0x34 open ticks (visible lines = ceil(open / ticks per line)) and +0x3c slide-in counter.
    open: f32,
    slide: f32,
    /// Current frame height (animated toward the target, FUN_1015c040).
    h: f32,
    /// Wall time of the last update and the ticks (1/60 s) since.
    last: Option<std::time::Instant>,
    ticks: f32,
    /// Wrapped lines added by the last update.
    pub new_lines: usize,
    /// Size and timing from the config (Windows / Window 1 page).
    pub cfg: LogCfg,
}

/// The full log (menu fulllog, FUN_10132e60 / FUN_10133890): cursor row 1..=lines (+0x1c) and the lines
/// skipped from the newest (+0x20).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FullLog {
    pub row: usize,
    pub scroll: usize,
}

/// Visible rows: 23 + (H - 448) / 16, truncated toward zero.
pub fn full_lines(sh: f32) -> usize {
    (23 + ((sh - 448.0) / 16.0) as i32).max(1) as usize
}

impl FullLog {
    pub fn open(sh: f32) -> Self {
        Self { row: full_lines(sh), scroll: 0 }
    }

    /// Messages 1 / 2 (Down / Up: the row, scrolling at its ends; with Ctrl / Alt, key 0xaf, scrolling only),
    /// 3 / 4 (Right / Left: a page newer / older). `total` is the history length. Returns whether it moved.
    pub fn key(&mut self, key: egui::Key, held: bool, lines: usize, total: usize) -> bool {
        let max = total.saturating_sub(lines);
        let before = *self;
        match key {
            egui::Key::ArrowDown if !held && self.row < lines => self.row += 1,
            egui::Key::ArrowDown => self.scroll = self.scroll.saturating_sub(1),
            egui::Key::ArrowUp if !held && self.row >= 2 => self.row -= 1,
            egui::Key::ArrowUp => self.scroll = (self.scroll + 1).min(max),
            egui::Key::ArrowRight => {
                self.scroll = self.scroll.saturating_sub(lines);
                if self.scroll == 0 {
                    self.row = lines;
                }
            }
            egui::Key::ArrowLeft => {
                self.scroll = (self.scroll + lines).min(max);
                if self.scroll == max {
                    self.row = 1;
                }
            }
            _ => {}
        }
        *self != before
    }
}

/// What the HUD needs from the log this frame.
pub struct LogFrame {
    /// Frame rect (x, y, w, h); h = 0 while hidden.
    pub rect: [f32; 4],
    pub lines: usize,
}

/// FUN_1015ae90: text width = clamp((W - 512) * pct / 100 + 352, 352, W - 160).
pub fn text_width(sw: f32, pct: f32) -> f32 {
    ((sw - 512.0) * pct / 100.0 + 352.0).clamp(352.0, (sw - 160.0).max(352.0))
}

/// Caption sprite (windowps) for a channel command (table 0x10365d50, FUN_101496d0); not found = Say.
pub fn caption(channel: &str) -> usize {
    match channel {
        "/shout" | "/sh" => 277,
        "/yell" | "/yl" | "/y" => 278,
        "/tell" | "/t" => 279,
        "/party" | "/p" => 280,
        "/linkshell" | "/l" => 281,
        "/linkshell2" | "/l2" => 711,
        "/unity" | "/u" => 712,
        _ => 276,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FUN_1015ae90 at 100 %: W - 160 (frame W - 146) from 512 up; never under 352. Say is sprite 276.
    #[test]
    fn width_and_caption() {
        assert_eq!(text_width(1280.0, 100.0), 1120.0);
        assert_eq!(text_width(640.0, 100.0), 480.0);
        assert_eq!(text_width(512.0, 100.0), 352.0);
        assert_eq!(text_width(1280.0, 50.0), 736.0);
        assert_eq!(caption(""), 276);
        assert_eq!(caption("/p"), 280);
    }

    /// 23 + (H - 448) / 16 rows; the row moves first, then the view scrolls; pages clamp to the history.
    #[test]
    fn full_log_keys() {
        assert_eq!([full_lines(480.0), full_lines(600.0), full_lines(800.0)], [25, 32, 45]);
        let mut f = FullLog::open(480.0);
        assert_eq!(f, FullLog { row: 25, scroll: 0 });
        f.key(egui::Key::ArrowDown, false, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 0 });
        f.key(egui::Key::ArrowUp, true, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 1 });
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        assert_eq!(f, FullLog { row: 1, scroll: 75 });
        f.key(egui::Key::ArrowRight, false, 25, 100);
        assert_eq!(f.scroll, 50);
        f.key(egui::Key::ArrowRight, false, 25, 100);
        f.key(egui::Key::ArrowRight, false, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 0 });
    }
}

impl LogWin {
    /// Takes in new log lines and advances the counters by the ticks since the last call. `keep` holds it
    /// open, `shrink` is Escape (key 0x95, FUN_1015b830).
    pub fn update(&mut self, gu: &GameUi, chat: &super::combat::ChatLog, sw: f32, keep: bool, shrink: bool, focused: bool, stamps: Stamps) -> f32 {
        let now = std::time::Instant::now();
        let ticks = self.last.map_or(0.0, |t| (now - t).as_secs_f32().min(0.5) * 60.0);
        self.last = Some(now);
        self.ticks = ticks;
        let tw = text_width(sw, self.cfg.pct);
        let rewrap = tw != self.width || stamps != self.stamps;
        if rewrap {
            self.width = tw;
            self.stamps = stamps;
            self.wrapped.clear();
            self.seen = chat.len().saturating_sub(64);
        }
        let mut new = 0;
        for ((line, colour), (time, is_chat)) in chat.iter_timed().skip(self.seen) {
            let st = stamps.text(time, is_chat);
            for (k, piece) in gu.wrap(&format!("{st}{line}"), tw).into_iter().enumerate() {
                let n = if k == 0 { st.trim_end().len().min(piece.len()) } else { 0 };
                self.wrapped.push((piece, *colour, n));
                new += 1;
            }
        }
        // A rewrap redraws the kept lines, not new ones.
        if rewrap {
            new = 0;
        }
        self.seen = chat.len();
        if self.wrapped.len() > KEEP {
            self.wrapped.drain(..self.wrapped.len() - KEEP);
        }
        // FUN_1015b8e0: each line keeps the window open one more period, capped at the max.
        for _ in 0..new {
            self.open = (self.open.max(0.0) + self.cfg.ticks).min(self.cfg.max * self.cfg.ticks);
            self.slide += 20.0;
        }
        // More than 64 lines behind jumps to the newest; 5 or more slide 20x faster.
        if self.slide > 64.0 * 20.0 {
            self.slide = 0.0;
        }
        let speed = if self.slide >= 5.0 * 20.0 { 20.0 } else { 1.0 };
        self.slide = (self.slide - ticks * speed).max(0.0);
        if shrink {
            let lines = (self.open / self.cfg.ticks).ceil();
            self.open = ((lines - 1.0) * self.cfg.ticks).max(self.cfg.min * self.cfg.ticks);
            self.slide = (self.slide - 20.0).max(0.0);
        }
        if !keep && self.open > self.cfg.min * self.cfg.ticks {
            self.open = (self.open - ticks).max(self.cfg.min * self.cfg.ticks);
        }
        // FUN_1015ba60: focused (numpad +), it is held at the max lines.
        if focused {
            self.open = self.cfg.max * self.cfg.ticks;
        }
        self.new_lines = new;
        tw
    }

    pub fn history(&self) -> usize {
        self.wrapped.len()
    }

    /// The full log (FUN_10133680): frame (16, 48, text width + 14, H - 64), the tab strip at y0 - 9, lines 16 px
    /// apart from the text rect's bottom (y1 - 4; 16 more while typing), the hand on the cursor row. Returns
    /// the frame rect.
    pub fn draw_full(&self, gu: &mut GameUi, fl: &FullLog, sw: f32, sh: f32, typing: bool, second: bool) -> [f32; 4] {
        let (x0, y0, w, h) = (16.0, 48.0, text_width(sw, self.cfg.pct) + 14.0, sh - 64.0);
        gu.frame(x0, y0, w, h);
        let lines = full_lines(sh);
        let (n, yoff) = if typing { (lines - 1, -16.0) } else { (lines, 0.0) };
        let bottom = y0 + h - 4.0 + yoff;
        gu.clip(Some([x0, y0 + 3.0, w, (bottom - (y0 + 3.0)).max(0.0)]));
        for (i, (line, colour, st)) in self.wrapped.iter().rev().skip(fl.scroll).take(n).enumerate() {
            stamped(gu, line, *st, x0 + 4.0, bottom - 16.0 - 16.0 * i as f32, *colour, self.stamps.colour);
        }
        gu.clip(None);
        // Child 1 (the hand, anc_s) at the cursor row: (x0 + 3, yb - 15), yb = bottom + 16 (row - lines) (inferred
        // from FUN_10133430's highlight rect).
        let yb = bottom + 16.0 * (fl.row as f32 - lines as f32);
        gu.cursor(x0 + 3.0, yb - 15.0);
        // Tabs (FUN_10138800; list built by FUN_10138700): fep sprite pairs 0x10333b4c, unselected A[2s] / selected
        // A[2s + 1]; the filter-8 tab (pair 1) only with config 0xb4 (a second log window). Pairs 7 / 8 are the
        // Assist J / E filters 9 / 10, in that order only in Japanese. Filter 0 (all) is selected.
        const A: [usize; 22] = [0x12, 0x1b, 0x13, 0x1c, 0x0a, 0x14, 0x0c, 0x16, 0x0d, 0x17, 0x0e, 0x18, 0x10, 0x19, 0x1d, 0x1f, 0x1e, 0x20, 0x11, 0x1a, 0x0b, 0x15];
        let mut x = x0;
        for slot in [0usize, 1, 2, 3, 4, 5, 6, 8, 7, 9, 10].into_iter().filter(|&s| s != 1 || second) {
            let on = slot == 0;
            let k = A[2 * slot + on as usize];
            gu.sprite("menu    fep", k, x, y0 - 9.0, 0x8080_8080);
            x += if on { gu.sprite_width("menu    fep", k) } else { 30.0 };
        }
        [x0, y0, w, h]
    }

    /// Visible lines (+0x31).
    pub fn lines(&self) -> usize {
        (self.open / self.cfg.ticks).ceil().max(0.0) as usize
    }

    /// Animates the frame toward its height and returns its rect: x 16, bottom H - 16.
    pub fn frame(&mut self, sw: f32, sh: f32) -> LogFrame {
        let ticks = self.ticks;
        let lines = self.lines();
        let target = if lines == 0 { 0.0 } else { 134.0 + (lines as f32 - 8.0) * 16.0 };
        // FUN_1015c040: 8 px a tick while more than 16 px away, then 2 px a tick (inferred tick-based).
        let gap = target - self.h;
        let step = if gap.abs() > 16.0 { 8.0 } else { 2.0 } * ticks;
        self.h = if gap.abs() <= step { target } else { self.h + step * gap.signum() };
        let w = text_width(sw, self.cfg.pct) + 14.0;
        LogFrame { rect: [16.0, sh - 16.0 - self.h, w, self.h], lines }
    }

    /// Draws the text: newest line at the bottom, lines 16 px apart, clipped to [x0, y0 + 3, x1, y1 - 4]; the
    /// input line takes the bottom 16 px. Returns the newest line's (x, y, text) for the kaipage arrow.
    pub fn draw(&self, gu: &mut GameUi, f: &LogFrame, typing: bool) -> Option<(f32, f32, String)> {
        let [x0, y0, w, h] = f.rect;
        if h <= 0.0 {
            return None;
        }
        let mut y1 = y0 + h;
        let mut rows = f.lines;
        if typing {
            y1 -= 16.0;
            rows = rows.saturating_sub(1);
        }
        let clip = [x0, y0 + 3.0, w, (y1 - 4.0 - (y0 + 3.0)).max(0.0)];
        gu.clip(Some(clip));
        let slide = (self.slide % 20.0) * 16.0 / 20.0;
        let mut newest = None;
        for (k, (line, colour, st)) in self.wrapped.iter().rev().take(rows + 1).enumerate() {
            let y = (y1 - 4.0) - 16.0 * (k as f32 + 1.0) + slide;
            stamped(gu, line, *st, x0 + 4.0, y, *colour, self.stamps.colour);
            if k == 0 {
                newest = Some((x0 + 4.0, y, line.clone()));
            }
        }
        gu.clip(None);
        newest
    }
}

/// A wrapped line whose first `st` bytes are a timestamp, drawn in its own colour.
fn stamped(gu: &mut GameUi, line: &str, st: usize, x: f32, y: f32, colour: [u8; 4], stamp: [u8; 4]) {
    if st == 0 {
        gu.text(line, x, y, 0, colour);
        return;
    }
    let (head, rest) = line.split_at(st);
    gu.text(head, x, y, 0, stamp);
    gu.text(rest, x + gu.text_width(head), y, 0, colour);
}
