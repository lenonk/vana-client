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

/// Ticks a retail frame steps the log by (the frame divisor: 2 = stock retail's 30 fps).
const FRAME: i32 = 2;

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
    /// FUN_101626e0's ints: +0x34 open ticks, +0x3c slide counter (20 a line still to slide in), +0x48 its speed (1 /
    /// 20), the wrapped lines newer than the displayed one (+0x2c, the bottom row), +0x31 visible lines, +0x30 lines
    /// to draw, +0x2e the slide offset, the frame height and its fade.
    open: i32,
    slide: i32,
    speed: i32,
    pending: usize,
    vis: i32,
    draw_n: i32,
    off: i32,
    h: i32,
    fade: super::hudtop::Fade,
    /// Wall time of the last update and the ticks not yet run.
    last: Option<std::time::Instant>,
    acc: f32,
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
    /// Frame rect (x, y, w, h).
    pub rect: [f32; 4],
    pub lines: usize,
    /// The frame background's fade (0 hidden).
    pub alpha: f32,
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

    /// FUN_101626e0 at stock 30 fps (2 ticks a frame): one line slides 14 12 11 9 8 6 4 3 1 0 px; two lines enter
    /// one after the other (the second shown once the first has slid); a backlog over 64 jumps to the newest.
    #[test]
    fn slide_like_retail() {
        let mut l = LogWin::default();
        l.fill();
        l.add_line();
        let offs: Vec<i32> = (0..10).map(|_| {
            l.step(2, false, false, false);
            l.off
        }).collect();
        assert_eq!(offs, [14, 12, 11, 9, 8, 6, 4, 3, 1, 0]);
        assert_eq!(l.pending, 0);
        l.add_line();
        l.add_line();
        l.step(2, false, false, false);
        assert_eq!(l.pending, 1);
        // The first slides 10 frames and rests at 20 (offset 0) for one more; then the second is shown.
        for _ in 0..9 {
            l.step(2, false, false, false);
        }
        assert_eq!((l.pending, l.slide, l.off), (1, 20, 0));
        l.step(2, false, false, false);
        assert_eq!((l.pending, l.off), (0, 14));
        for _ in 0..70 {
            l.add_line();
        }
        l.step(2, false, false, false);
        assert_eq!((l.pending, l.slide), (0, 0));
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
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, gu: &GameUi, chat: &super::combat::ChatLog, sw: f32, keep: bool, shrink: bool, focused: bool, typing: bool, stamps: Stamps) -> f32 {
        let now = std::time::Instant::now();
        // Retail's frame tick is whole and at most 20 (FUN_10014cf0).
        let ticks = self.last.map_or(0.0, |t| ((now - t).as_secs_f32() * 60.0).min(20.0));
        self.last = Some(now);
        let tw = text_width(sw, self.cfg.pct);
        let rewrap = tw != self.width || stamps != self.stamps;
        if rewrap {
            self.width = tw;
            self.stamps = stamps;
            self.wrapped.clear();
            self.seen = chat.len().saturating_sub(64);
            // FUN_10162d60: the newest line shown, nothing sliding.
            (self.slide, self.pending) = (0, 0);
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
        for _ in 0..new {
            self.add_line();
        }
        if shrink {
            self.escape();
        }
        // Whole retail frames of FRAME ticks (stock retail runs 30 fps: 2 ticks a frame); the fraction carries.
        self.acc += ticks;
        while self.acc >= FRAME as f32 {
            self.acc -= FRAME as f32;
            self.step(FRAME, keep, focused, typing);
        }
        self.new_lines = new;
        tw
    }

    fn period(&self) -> i32 {
        (self.cfg.ticks as i32).max(1)
    }

    /// Rounds the open ticks up to a whole period.
    fn round_up(&mut self) {
        let t = self.period();
        if self.open % t != 0 {
            self.open += t - self.open % t;
        }
    }

    /// FUN_10162560: a line keeps the window open one more period (capped, rounded up to a whole period) and has 20
    /// to slide; the displayed line stays until the step reaches it.
    fn add_line(&mut self) {
        let t = self.period();
        self.open = if self.open < 0 { t } else { self.open + t };
        self.slide += 20;
        self.open = self.open.min(self.cfg.max as i32 * t);
        self.round_up();
        self.pending += 1;
    }

    /// FUN_101624b0 (Escape, key 0x95): a period off, not under the min lines, rounded up; 20 off the slide.
    fn escape(&mut self) {
        let t = self.period();
        self.open = (self.open - t).max(self.cfg.min as i32 * t);
        self.round_up();
        self.slide = (self.slide - 20).max(0);
    }

    /// FUN_101626e0, one frame of `t` ticks, in its order.
    fn step(&mut self, t: i32, keep: bool, focused: bool, typing: bool) {
        use super::hudtop::Fade;
        let p = self.period();
        let (max, min) = (self.cfg.max as i32, self.cfg.min as i32);
        // Held at the max while focused (Num+, the fill argument).
        if focused {
            self.open = p * max;
        }
        if self.open > min * p && !keep {
            self.open = (self.open - t).max(min * p);
        }
        if self.slide > 0 {
            self.slide -= t * self.speed.max(1);
            if self.slide < 1 {
                self.slide = 0;
            }
        }
        self.vis = self.open.max(0) / p + (self.open.max(0) % p > 0) as i32;
        self.draw_n = (self.vis + 1 - typing as i32).max(0);
        // FUN_10162cc0: 8 px a frame while more than 16 away, then 2 (+0x38 never set: p3 0), snapping when closer.
        let target = 134 + (self.vis - 8) * 16;
        let d = target - self.h;
        if d != 0 {
            let s = if d.abs() <= 16 { d.signum() * 160 / 60 } else { d.signum() * 8 };
            self.h = if d.abs() < s.abs() { target } else { self.h + s };
        }
        // Typing moves the text up a row as well as the clip (asm 0x10162abc: y1 -= 16, off -= 16; user-confirmed on retail).
        self.off = if self.slide > 0 { (self.slide % 20) * 16 / 20 } else { 0 } - if typing { 16 } else { 0 };
        // The displayed line: one line a frame toward the newest while the slide allows it.
        if self.pending > 0 {
            let n = self.pending as i32;
            if self.slide / 20 < n {
                self.pending -= 1;
            } else {
                self.slide = self.slide % 20 + n * 20;
            }
            if n > 64 {
                (self.slide, self.pending) = (0, 0);
            } else {
                self.speed = if n >= 5 { 20 } else { 1 };
            }
        } else {
            self.slide %= 20;
        }
        // The frame's background fades out over 20 ticks with no lines and in again (FUN_10119550).
        self.fade = match self.fade {
            Fade::Out(n) if n - t >= 1 => Fade::Out(n - t),
            Fade::Out(_) => Fade::Hidden,
            Fade::In(n) if n - t >= 1 => Fade::In(n - t),
            Fade::In(_) => Fade::Shown,
            f => f,
        };
        self.fade = match (self.vis > 0, self.fade) {
            (false, Fade::In(_) | Fade::Shown) => Fade::Out(20),
            (true, Fade::Out(_) | Fade::Hidden) => Fade::In(20),
            (_, f) => f,
        };
    }

    /// FUN_10162ef0: open at the max lines (open ticks = ticks per line x max).
    pub fn fill(&mut self) {
        self.open = self.period() * self.cfg.max as i32;
    }

    /// The open ticks against two periods: the chat line keeps the log open only up to 2 lines (asm 0x1016280f).
    pub fn at_most_two(&self) -> bool {
        self.open <= 2 * self.period()
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
        self.vis.max(0) as usize
    }

    /// The frame's rect (x 16, bottom H - 16, the height the step eased), its lines and the fade's alpha.
    pub fn frame(&mut self, sw: f32, sh: f32) -> LogFrame {
        let w = text_width(sw, self.cfg.pct) + 14.0;
        let h = self.h.max(0) as f32;
        LogFrame { rect: [16.0, sh - 16.0 - h, w, h], lines: self.lines(), alpha: self.fade.alpha() }
    }

    /// Draws the text: newest line at the bottom, lines 16 px apart, clipped to [x0, y0 + 3, x1, y1 - 4]; the
    /// input line takes the bottom 16 px. Returns the newest line's (x, y, text) for the kaipage arrow.
    pub fn draw(&self, gu: &mut GameUi, f: &LogFrame, typing: bool) -> Option<(f32, f32, String)> {
        let [x0, y0, w, h] = f.rect;
        // FUN_10162510: only while there are visible lines; from the displayed line up (FUN_101622e0), whole pixels.
        if self.vis <= 0 || h <= 0.0 {
            return None;
        }
        let mut y1 = y0 + h;
        if typing {
            y1 -= 16.0;
        }
        let clip = [x0, y0 + 3.0, w, (y1 - 4.0 - (y0 + 3.0)).max(0.0)];
        gu.clip(Some(clip));
        let mut newest = None;
        for (k, (line, colour, st)) in self.wrapped.iter().rev().skip(self.pending).take(self.draw_n as usize).enumerate() {
            let y = (y1 - 4.0) - 16.0 * (k as f32 + 1.0) + self.off as f32;
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
