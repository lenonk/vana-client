//! The chat log windows (logwindo, base class FUN_1015b250; logwin2, config 0xb4, logwin2_spec.md) and the input line
//! (msgline): width from the UI width, height from the visible line count, auto-shrink, slide-in, the channel caption,
//! each line routed to a window by its mode and the window's mask.

use super::game_ui::GameUi;

/// A window's config: window 1 0x0f max lines, 0x10 min lines, 0x11 seconds per line, 0x9c width percent, 0x0e fixed
/// size (FUN_1015c180..FUN_1015c220; fixed: min = max); window 2 0xb7, 0xb8, 0xc6, 0xc5, 0xb6.
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
        Self::for_window(v, false)
    }

    /// FUN_10162e20: max outside 4..24 is 24; FUN_10162e60: min = max unless reactive; FUN_10162ea0: under 3 s is 3 s,
    /// then x 60 (20 ticks at 0). Window 2's -1 width and time are resolved by cfgapply::normalise.
    pub fn for_window(v: &crate::gamecfg::Values, second: bool) -> Self {
        let [fixed, max, min, secs, pct] = if second { [0xb6, 0xb7, 0xb8, 0xc6, 0xc5] } else { [0x0e, 0x0f, 0x10, 0x11, 0x9c] };
        let max = match v.get_i(max) {
            m @ 4..=24 => m as f32,
            _ => 24.0,
        };
        let min = if v.get(fixed) != 0 { max } else { (v.get_i(min).max(0) as f32).min(max) };
        let ticks = (v.get_i(secs).max(3) * 60) as f32;
        Self { max, min, ticks: if ticks <= 0.0 { 20.0 } else { ticks }, pct: v.get_i(pct).clamp(0, 100) as f32 }
    }
}

/// Which lines each window takes (FUN_10129980): window 2 on (config 0xb4) and the masks {0xb9, 0xba}, {0xbb, 0xbc}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Route {
    pub second: bool,
    pub masks: [u64; 2],
}

impl Route {
    pub fn from_config(v: &crate::gamecfg::Values) -> Self {
        let mask = |lo: u16, hi: u16| v.get(lo) as u64 | (v.get(hi) as u64) << 32;
        Self { second: v.get(0xb4) != 0, masks: [mask(0xb9, 0xba), mask(0xbb, 0xbc)] }
    }

    /// FUN_10129b40 (window 0: everything while window 2 is off; GM lines; else its mask, default shown) and
    /// FUN_10129b80 (window 1: never GM lines; its mask, default hidden).
    pub fn shows(&self, window: usize, mode: Option<u8>, gm: bool) -> bool {
        match window {
            0 if !self.second || gm => true,
            0 => mode.and_then(mode_bit).is_none_or(|b| self.masks[0] >> b & 1 != 0),
            _ => !gm && mode.and_then(mode_bit).is_some_and(|b| self.masks[1] >> b & 1 != 0),
        }
    }
}

/// A print mode's mask bit (table 0x1036e888 -> FUN_1012a120; the Config > Log rows): none for the modes without one.
pub fn mode_bit(mode: u8) -> Option<u8> {
    Some(match mode {
        1 | 9 => 0,
        4 | 12 => 1,
        5 | 13 | 0xd2 => 2,
        6 | 14 | 0xcd => 3,
        0xd5 | 0xd6 | 0xd9 => 4,
        0xd3 | 0xd4 => 6,
        7 | 15 | 0xd0 => 7,
        17 | 18 => 8,
        0x8e..=0x99 | 0xbe => 9,
        2 | 10 => 10,
        3 | 11 => 11,
        0xdb | 0xdc => 12,
        0xdd | 0xde => 13,
        30 | 31 => 33,
        28 => 34,
        56 | 81 | 101 => 35,
        57 | 82 | 102 => 36,
        59 => 37,
        29 | 70 | 84 | 104 => 38,
        22 | 23 | 24 | 27 | 34 | 35 | 42 | 43 | 162 | 165 | 187 | 188 => 39,
        20 | 25 | 32 | 40 | 163 | 185 => 40,
        60 | 64 | 86 | 91 | 106 | 111 | 175 | 178 | 183 => 41,
        61 | 65 | 87 | 92 | 107 | 112 | 174 | 176 | 182 => 42,
        63 | 67 | 68 | 69 | 170 | 189 => 43,
        21 | 26 | 33 | 41 | 71 | 72 | 89 | 94 | 109 | 114 | 164 | 173 | 181 | 184 | 186 => 44,
        50..=52 | 55 | 58 | 62 | 66 | 80 | 83 | 85 | 88 | 90 | 93 | 100 | 103 | 105 | 108 | 110 | 113 | 122 | 168 | 169 | 171 | 172 | 177 | 179 | 180 => 45,
        8 | 16 => 46,
        121 | 127..=140 => 47,
        _ => return None,
    })
}

/// Window 2's place from window 1's frame (FUN_10161c70): 0xb4 1 above it (same x, its bottom on window 1's top),
/// 2 to its right (bottoms aligned); `w` and `h` its own size.
pub fn second_rect(mode: u32, w1: [f32; 4], w: f32, h: f32) -> [f32; 4] {
    if mode == 2 { [w1[0] + w1[2], w1[1] + w1[3] - h, w, h] } else { [w1[0], w1[1] - h, w, h] }
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
    wrapped: Vec<Row>,
    seen: usize,
    width: f32,
    stamps: Stamps,
    /// The log's in-place edits when last wrapped (a changed line wraps everything again).
    edits: u64,
    pub anim: LogAnim,
    /// The frame height's change in the last update (new - old), for the pointer follow.
    pub dh: i32,
    ticker: Ticker,
    /// Wrapped lines added by the last update.
    pub new_lines: usize,
    /// Size and timing from the config (Windows / Window 1 or 2 page).
    pub cfg: LogCfg,
    /// Window 2 (logwin2): its mask, no typing shift, no keys.
    pub second: bool,
    /// The route its lines were taken with (a change rebuilds them).
    route: Route,
    /// The full log's rows on a tab other than Window 1 (window 1's own rows).
    tab: TabRows,
}

/// FUN_101626e0's state, shared by the retail log and the modern chat window (each counting the rows of its own
/// wrapping): +0x34 open ticks, +0x3c slide counter (20 a line still to slide in), +0x48 its speed (1 / 20), the
/// wrapped lines newer than the displayed one (+0x2c, the bottom row), +0x31 visible lines, +0x30 lines to draw, +0x2e
/// the slide offset (16 a row), the frame height (6 + 16 a line) and its fade.
#[derive(Default)]
pub struct LogAnim {
    open: i32,
    slide: i32,
    speed: i32,
    pending: usize,
    vis: i32,
    draw_n: i32,
    off: i32,
    h: i32,
    fade: super::hudtop::Fade,
    /// The chat line ("menu    inline") was open last frame.
    inline: bool,
}

/// Retail's frame clock: whole frames of FRAME ticks from wall time, at most 20 ticks a call (FUN_10014cf0), the
/// fraction carried.
#[derive(Default)]
pub struct Ticker {
    last: Option<std::time::Instant>,
    acc: f32,
}

impl Ticker {
    /// The frames to step now.
    pub fn frames(&mut self) -> usize {
        let now = std::time::Instant::now();
        let ticks = self.last.map_or(0.0, |t| ((now - t).as_secs_f32() * 60.0).min(20.0));
        self.last = Some(now);
        self.acc += ticks;
        let n = (self.acc / FRAME as f32) as usize;
        self.acc -= (n as i32 * FRAME) as f32;
        n
    }
}

/// What the log's rule takes from the game each frame (world_hud fills it for the modern chat): a window stacked on the
/// log, the rows of a message page waiting for a key, the chat line, the event input, Num+'s focus, Escape.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogRule {
    pub stacked: bool,
    pub page_rows: Option<usize>,
    pub typing: bool,
    pub line_open: bool,
    pub focused: bool,
    pub shrink: bool,
    pub cfg: LogCfg,
}

/// The modern chat window's run of the rule: its state, clock, the log lines it has counted, the width, font and tab
/// its rows were wrapped with, and its eased height in rows over its chrome (none yet: gone).
#[derive(Default)]
pub struct ModernLog {
    pub anim: LogAnim,
    pub ticker: Ticker,
    pub seen: usize,
    /// Folded repeats taken (ChatLog::folds).
    pub folds: u64,
    pub key: (u32, u32, u8),
    pub shown: Option<f32>,
    /// The full log just closed: open at the max lines (its text stays in view rather than folding away).
    pub fill: bool,
}

impl LogAnim {
    /// The rest of a frame of the rule, after the chat line and the new rows (as retail's update): Escape, the keep,
    /// then the steps.
    pub fn run(&mut self, r: &LogRule, frames: usize) {
        if r.shrink {
            self.escape(&r.cfg);
        }
        let keep = (r.typing && self.at_most_two(&r.cfg)) || r.stacked || r.page_rows.is_some_and(|n| self.lines() <= n.max(1));
        for _ in 0..frames {
            self.step(FRAME, keep, r.focused, r.line_open, &r.cfg);
        }
    }

    fn period(cfg: &LogCfg) -> i32 {
        (cfg.ticks as i32).max(1)
    }

    /// Rounds the open ticks up to a whole period.
    fn round_up(&mut self, cfg: &LogCfg) {
        let t = Self::period(cfg);
        if self.open % t != 0 {
            self.open += t - self.open % t;
        }
    }

    /// FUN_10162560: a line keeps the window open one more period (capped, rounded up to a whole period) and has 20
    /// to slide; the displayed line stays until the step reaches it.
    pub fn add_line(&mut self, cfg: &LogCfg) {
        self.extend(cfg);
        self.slide += 20;
        self.pending += 1;
    }

    /// FUN_10162560's open period alone (capped, rounded up), nothing to slide: also a repeat folded into the last
    /// line (ours, the modern chat's Condense).
    pub fn extend(&mut self, cfg: &LogCfg) {
        let t = Self::period(cfg);
        self.open = if self.open < 0 { t } else { self.open + t };
        self.open = self.open.min(cfg.max as i32 * t);
        self.round_up(cfg);
    }

    /// Inline's open (FUN_101b7f00) runs FUN_10162560 twice: two periods open and 40 to slide, no line pending. The log
    /// shows 2 lines at least under the chat line, its text gliding up into place. The event input window does not.
    pub fn chat_line(&mut self, open: bool, cfg: &LogCfg) {
        if open && !self.inline {
            for _ in 0..2 {
                self.extend(cfg);
                self.slide += 20;
            }
        }
        self.inline = open;
    }

    /// FUN_101624b0 (Escape, key 0x95): a period off, not under the min lines, rounded up; 20 off the slide.
    pub fn escape(&mut self, cfg: &LogCfg) {
        let t = Self::period(cfg);
        self.open = (self.open - t).max(cfg.min as i32 * t);
        self.round_up(cfg);
        self.slide = (self.slide - 20).max(0);
    }

    /// FUN_10162d60 (a rewrap): the newest line shown, nothing sliding.
    pub fn settle(&mut self) {
        (self.slide, self.pending) = (0, 0);
    }

    /// FUN_10162ef0: open at the max lines (open ticks = ticks per line x max).
    pub fn fill(&mut self, cfg: &LogCfg) {
        self.open = Self::period(cfg) * cfg.max as i32;
    }

    /// The open ticks against two periods: the chat line keeps the log open only up to 2 lines (asm 0x1016280f).
    pub fn at_most_two(&self, cfg: &LogCfg) -> bool {
        self.open <= 2 * Self::period(cfg)
    }

    /// Visible lines (+0x31).
    pub fn lines(&self) -> usize {
        self.vis.max(0) as usize
    }

    /// Rows to draw from the displayed one up (+0x30), the rows newer than it (+0x2c), the slide offset in 16ths of a
    /// row (typing included).
    pub fn draw_n(&self) -> usize {
        self.draw_n.max(0) as usize
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    pub fn off(&self) -> i32 {
        self.off
    }

    /// The frame's background alpha, and whether it is fading out or gone (FUN_1015f790 then puts stacked windows on
    /// the log's bottom).
    pub fn alpha(&self) -> f32 {
        self.fade.alpha()
    }

    pub fn faded(&self) -> bool {
        matches!(self.fade, super::hudtop::Fade::Out(_) | super::hudtop::Fade::Hidden)
    }
}

/// The full log (menu fulllog, FUN_10132e60 / FUN_10133890): cursor row 1..=lines (+0x1c), the lines skipped from
/// the newest (+0x20) and the tab's filter (+0x54).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FullLog {
    pub row: usize,
    pub scroll: usize,
    pub filter: u8,
}

/// The full log's tabs (list 0x1036f940 as FUN_10138700 / FUN_10138800 leave it in English): filters Window 1, Window 2
/// (only with config 0xb4), Say, Tell, Party, Linkshell, Linkshell 2, Assist E, Assist J, Unity, Shout.
pub fn full_tabs(second: bool) -> Vec<u8> {
    [0u8, 8, 3, 4, 1, 2, 7, 10, 9, 6, 5].into_iter().filter(|&f| f != 8 || second).collect()
}

/// Message 8 (Num+, FUN_10138230 case 8 -> FUN_10138910): the next tab, wrapping.
pub fn next_tab(filter: u8, second: bool) -> u8 {
    let tabs = full_tabs(second);
    let k = tabs.iter().position(|&f| f == filter).map_or(0, |k| (k + 1) % tabs.len());
    tabs[k]
}

/// A filter's fep sprite pair (the list's second bytes).
fn tab_pair(filter: u8) -> usize {
    match filter {
        0 => 0,
        8 => 1,
        3 => 2,
        4 => 3,
        1 => 4,
        2 => 5,
        7 => 6,
        9 => 7,
        10 => 8,
        6 => 9,
        _ => 10,
    }
}

/// FUN_101299f0: a line on tab `filter`: Window 1 / 2 by their masks, the channels by their modes.
pub fn tab_shows(filter: u8, route: &Route, mode: Option<u8>, gm: bool) -> bool {
    let m = mode.unwrap_or(0xff);
    match filter {
        0 => route.shows(0, mode, gm),
        8 => route.shows(1, mode, gm),
        1 => matches!(m, 5 | 13 | 0xd2),
        2 => matches!(m, 6 | 14),
        3 => matches!(m, 1 | 9),
        4 => matches!(m, 4 | 12),
        5 => matches!(m, 2 | 10 | 3 | 11),
        6 => matches!(m, 0xd3 | 0xd4),
        7 => matches!(m, 0xd5 | 0xd6),
        9 => matches!(m, 0xdb | 0xdc),
        10 => matches!(m, 0xdd | 0xde),
        _ => false,
    }
}

/// A wrapped row: its text, colour, timestamp bytes and the log line it came from.
pub type Row = (String, [u8; 4], usize, usize);

/// A full log tab's rows (wrapped at window 1's width) and what they were built from: the filter, the log's length and
/// edits, the width, the route.
#[derive(Default)]
struct TabRows {
    key: (u8, usize, u64, u32, Route),
    rows: Vec<Row>,
}

/// Visible rows: 23 + (H - 448) / 16, truncated toward zero.
pub fn full_lines(sh: f32) -> usize {
    (23 + ((sh - 448.0) / 16.0) as i32).max(1) as usize
}

impl FullLog {
    pub fn open(sh: f32) -> Self {
        Self { row: full_lines(sh), scroll: 0, filter: 0 }
    }

    /// The cursor's row counted up from the newest (0 the bottom one), with `lines` rows shown.
    pub fn back(&self, lines: usize) -> usize {
        self.scroll + lines.saturating_sub(self.row)
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

/// Window 2's caption (windowps 740 "Window 2", FUN_10161d60).
pub const CAPTION_2: usize = 740;

/// Window 1's caption with window 2 on (config 0xb4, FUN_10161990's second column, pairs 0x10371b80): "Window 1:Say"
/// 733 and the like; Yell has none (278 stays).
pub fn caption_1(channel: &str, second: bool) -> usize {
    let c = caption(channel);
    if !second {
        return c;
    }
    match c {
        276 => 733,
        277 => 739,
        279 => 734,
        280 => 735,
        281 => 736,
        711 => 737,
        712 => 738,
        c => c,
    }
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
        assert_eq!((caption_1("/p", true), caption_1("/yell", true), caption_1("", false)), (735, 278, 276));
    }

    /// The tabs in English order; Window 2 only with a second window; Num+ wraps; Shout takes yells.
    #[test]
    fn full_log_tabs() {
        assert_eq!(full_tabs(false), [0, 3, 4, 1, 2, 7, 10, 9, 6, 5]);
        assert_eq!((next_tab(0, true), next_tab(0, false), next_tab(5, true)), (8, 3, 0));
        let r = Route::default();
        assert!(tab_shows(5, &r, Some(11), false) && tab_shows(1, &r, Some(0xd2), false) && !tab_shows(3, &r, Some(7), false));
    }

    /// Default masks with window 2 on: chat, emotes and NPC talk to window 2; battle, system and modeless lines to
    /// window 1; GM lines to window 1 only; everything to window 1 with window 2 off. Window 2 above or right of 1.
    #[test]
    fn routing_and_place() {
        let mut v = crate::gamecfg::Values::default();
        let off = Route::from_config(&v);
        v.set(0xb4, 1);
        let on = Route::from_config(&v);
        let say = Some(1);
        assert!(off.shows(0, say, false) && !on.shows(0, say, false) && on.shows(1, say, false));
        for battle in [Some(80), Some(121), None, Some(36)] {
            assert!(on.shows(0, battle, false) && !on.shows(1, battle, false), "{battle:?}");
        }
        assert!(on.shows(1, Some(0x98), false) && !on.shows(0, Some(0x98), false));
        assert!(on.shows(0, say, true) && !on.shows(1, say, true));
        assert_eq!(second_rect(1, [16.0, 500.0, 366.0, 134.0], 300.0, 70.0), [16.0, 430.0, 300.0, 70.0]);
        assert_eq!(second_rect(2, [16.0, 500.0, 366.0, 134.0], 300.0, 70.0), [382.0, 564.0, 300.0, 70.0]);
        // 0xc6 / 0xc5 at -1 follow window 1.
        crate::client::cfgapply::normalise(&mut v);
        assert_eq!(LogCfg::for_window(&v, true), LogCfg::for_window(&v, false));
    }

    /// FUN_101626e0 at stock 30 fps (2 ticks a frame): one line slides 14 12 11 9 8 6 4 3 1 0 px; two lines enter
    /// one after the other (the second shown once the first has slid); a backlog over 64 jumps to the newest.
    #[test]
    fn slide_like_retail() {
        let (mut l, cfg) = (LogAnim::default(), LogCfg::default());
        l.fill(&cfg);
        l.add_line(&cfg);
        let offs: Vec<i32> = (0..10).map(|_| {
            l.step(2, false, false, false, &cfg);
            l.off
        }).collect();
        assert_eq!(offs, [14, 12, 11, 9, 8, 6, 4, 3, 1, 0]);
        assert_eq!(l.pending, 0);
        l.add_line(&cfg);
        l.add_line(&cfg);
        l.step(2, false, false, false, &cfg);
        assert_eq!(l.pending, 1);
        // The first slides 10 frames and rests at 20 (offset 0) for one more; then the second is shown.
        for _ in 0..9 {
            l.step(2, false, false, false, &cfg);
        }
        assert_eq!((l.pending, l.slide, l.off), (1, 20, 0));
        l.step(2, false, false, false, &cfg);
        assert_eq!((l.pending, l.off), (0, 14));
        for _ in 0..70 {
            l.add_line(&cfg);
        }
        l.step(2, false, false, false, &cfg);
        assert_eq!((l.pending, l.slide), (0, 0));
    }

    /// 23 + (H - 448) / 16 rows; the row moves first, then the view scrolls; pages clamp to the history.
    #[test]
    fn full_log_keys() {
        assert_eq!([full_lines(480.0), full_lines(600.0), full_lines(800.0)], [25, 32, 45]);
        let mut f = FullLog::open(480.0);
        assert_eq!(f, FullLog { row: 25, scroll: 0, filter: 0 });
        f.key(egui::Key::ArrowDown, false, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 0, filter: 0 });
        f.key(egui::Key::ArrowUp, true, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 1, filter: 0 });
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        f.key(egui::Key::ArrowLeft, false, 25, 100);
        assert_eq!(f, FullLog { row: 1, scroll: 75, filter: 0 });
        f.key(egui::Key::ArrowRight, false, 25, 100);
        assert_eq!(f.scroll, 50);
        f.key(egui::Key::ArrowRight, false, 25, 100);
        f.key(egui::Key::ArrowRight, false, 25, 100);
        assert_eq!(f, FullLog { row: 25, scroll: 0, filter: 0 });
    }
}

impl LogWin {
    /// The full log's rows for `filter` (FUN_101295d0 over the history): Window 1 is this window's rows; Window 2 every
    /// line of its mask up to KEEP rows; a channel the newest 100 rows *(inf: ~50 rows at the open, up to 100 while
    /// open)*.
    pub fn prepare_tab(&mut self, gu: &GameUi, chat: &super::combat::ChatLog, filter: u8, route: Route) {
        if filter == 0 {
            return;
        }
        let key = (filter, chat.len(), chat.edits(), self.width.to_bits(), route);
        if self.tab.key == key && !self.tab.rows.is_empty() {
            return;
        }
        let limit = if filter == 8 { KEEP } else { 100 };
        let mut rows: Vec<Vec<Row>> = Vec::new();
        let mut n = 0;
        for k in (0..chat.len()).rev() {
            let (mode, gm) = chat.route(k);
            if !tab_shows(filter, &route, mode, gm) {
                continue;
            }
            let r = self.wrap_line(gu, chat, k, self.width);
            n += r.len();
            rows.push(r);
            if n >= limit {
                break;
            }
        }
        self.tab = TabRows { key, rows: rows.into_iter().rev().flatten().collect() };
    }

    fn full_rows(&self, filter: u8) -> &[Row] {
        if filter == 0 { &self.wrapped } else { &self.tab.rows }
    }

    /// The full log's row count on its tab (its bar and paging).
    pub fn full_total(&self, filter: u8) -> usize {
        self.full_rows(filter).len()
    }

    /// Window 2.
    pub fn second() -> Self {
        Self { second: true, ..Default::default() }
    }

    /// Log line `k` wrapped to `tw` with its stamp: (row, colour, stamp bytes, k).
    fn wrap_line(&self, gu: &GameUi, chat: &super::combat::ChatLog, k: usize, tw: f32) -> Vec<Row> {
        let Some(((line, colour), (time, is_chat, indent))) = chat.timed(k) else { return Vec::new() };
        let st = self.stamps.text(time, is_chat);
        let rows = gu.wrap_indent(&format!("{st}{line}"), tw, indent);
        rows.into_iter().enumerate().map(|(i, piece)| {
            let n = if i == 0 { st.trim_end().len().min(piece.len()) } else { 0 };
            (piece, *colour, n, k)
        }).collect()
    }

    /// The log line of the full log's row `back` rows above its newest on tab `filter`.
    pub fn full_row_line(&self, filter: u8, back: usize) -> Option<usize> {
        let rows = self.full_rows(filter);
        Some(rows[rows.len().checked_sub(back + 1)?].3)
    }

    /// The full log's row `back` rows above its newest on tab `filter`: its log line, and the text from that row to
    /// the line's end (charlnk +0x56: the rows of one record joined, a continuation's indent left out).
    pub fn full_line(&self, filter: u8, back: usize) -> Option<(usize, String)> {
        let rows = self.full_rows(filter);
        let at = rows.len().checked_sub(back + 1)?;
        let k = rows[at].3;
        let mut text = String::new();
        for (i, r) in rows[at..].iter().take_while(|r| r.3 == k).enumerate() {
            text.push_str(if i == 0 { &r.0 } else { r.0.strip_prefix('\u{3000}').unwrap_or(&r.0) });
        }
        Some((k, text))
    }

    /// Takes in new log lines and advances the counters by the ticks since the last call. `keep` holds it
    /// open, `shrink` is Escape (key 0x95, FUN_1015b830). Lines this window's mask leaves out are not taken; stamps
    /// are added as lines come (a change of 0xb5 shows on new lines).
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, gu: &GameUi, chat: &super::combat::ChatLog, sw: f32, keep: bool, shrink: bool, focused: bool, typing: bool, stamps: Stamps, route: Route) -> f32 {
        let tw = text_width(sw, self.cfg.pct);
        let window = self.second as usize;
        self.stamps = stamps;
        let rewrap = tw != self.width || chat.edits() != self.edits || route != self.route;
        let mut new = 0;
        if rewrap {
            // FUN_101295d0 at an open: the newest lines that pass, until more than 49 rows.
            (self.width, self.edits, self.route) = (tw, chat.edits(), route);
            let mut rows: Vec<Vec<Row>> = Vec::new();
            let mut n = 0;
            for k in (0..chat.len()).rev() {
                let (mode, gm) = chat.route(k);
                if !route.shows(window, mode, gm) {
                    continue;
                }
                let r = self.wrap_line(gu, chat, k, tw);
                n += r.len();
                rows.push(r);
                if n > 49 {
                    break;
                }
            }
            self.wrapped = rows.into_iter().rev().flatten().collect();
            self.anim.settle();
        } else {
            for k in self.seen..chat.len() {
                let (mode, gm) = chat.route(k);
                if route.shows(window, mode, gm) {
                    let r = self.wrap_line(gu, chat, k, tw);
                    new += r.len();
                    self.wrapped.extend(r);
                }
            }
        }
        self.seen = chat.len();
        if self.wrapped.len() > KEEP {
            self.wrapped.drain(..self.wrapped.len() - KEEP);
        }
        let cfg = self.cfg;
        for _ in 0..new {
            self.anim.add_line(&cfg);
        }
        if shrink {
            self.anim.escape(&cfg);
        }
        // Whole retail frames of FRAME ticks (stock retail runs 30 fps: 2 ticks a frame); the fraction carries.
        let h0 = self.anim.h;
        for _ in 0..self.ticker.frames() {
            self.anim.step(FRAME, keep, focused, typing, &cfg);
        }
        self.dh = self.anim.h - h0;
        self.new_lines = new;
        tw
    }

    pub fn chat_line(&mut self, open: bool) {
        let cfg = self.cfg;
        self.anim.chat_line(open, &cfg);
    }
}

impl LogAnim {
    /// FUN_101626e0, one frame of `t` ticks, in its order.
    fn step(&mut self, t: i32, keep: bool, focused: bool, typing: bool, cfg: &LogCfg) {
        use super::hudtop::Fade;
        let p = Self::period(cfg);
        let (max, min) = (cfg.max as i32, cfg.min as i32);
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
}

impl LogWin {
    /// FUN_10162ef0: open at the max lines (open ticks = ticks per line x max).
    pub fn fill(&mut self) {
        let cfg = self.cfg;
        self.anim.fill(&cfg);
    }

    pub fn at_most_two(&self) -> bool {
        self.anim.at_most_two(&self.cfg)
    }

    pub fn history(&self) -> usize {
        self.wrapped.len()
    }

    /// The full log (FUN_10133680): frame (16, 48, text width + 14, H - 64), the tab strip at y0 - 9, lines 16 px
    /// apart from the text rect's bottom (y1 - 4); while typing the rect ends 16 higher and the lines move up 16 more
    /// (FUN_10138020's offset -16), the hand with them. Returns the frame rect.
    /// `band`: the cursor row is a player's line (+0x26): child 1 (the row) shows keytops3 538, a translucent blue band,
    /// instead of windowps 332 (blank) (FUN_10137c70; drawn under the text *(inf: the order)*).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_full(&self, gu: &mut GameUi, fl: &FullLog, sw: f32, sh: f32, typing: bool, second: bool, band: bool) -> [f32; 4] {
        let (x0, y0, w, h) = (16.0, 48.0, text_width(sw, self.cfg.pct) + 14.0, sh - 64.0);
        gu.frame(x0, y0, w, h);
        let lines = full_lines(sh);
        let (n, yoff) = if typing { (lines - 1, -16.0) } else { (lines, 0.0) };
        let bottom = y0 + h - 4.0 + yoff;
        if band {
            gu.sprite("menu    keytops3", 538, x0 + 3.0, bottom + yoff + 16.0 * (fl.row as f32 - lines as f32) - 15.0, 0x8080_8080);
        }
        gu.clip(Some([x0, y0 + 3.0, w, (bottom - (y0 + 3.0)).max(0.0)]));
        let rows = self.full_rows(fl.filter);
        for (i, (line, colour, st, _)) in rows.iter().rev().skip(fl.scroll).take(n).enumerate() {
            stamped(gu, line, *st, x0 + 4.0, bottom + yoff - 16.0 - 16.0 * i as f32, *colour, self.stamps.colour);
        }
        gu.clip(None);
        // Child 1 (the hand, anc_s) at the cursor row: (x0 + 3, yb - 15), yb = bottom + 16 (row - lines) (inferred
        // from FUN_10133430's highlight rect).
        let yb = bottom + yoff + 16.0 * (fl.row as f32 - lines as f32);
        gu.cursor(x0 + 3.0, yb - 15.0);
        // FUN_10137840: the bar at x1 - 6 (resized at open), the log's lines, bottom-anchored top.
        let total = rows.len();
        gu.scroll_bar(x0 + w - 6.0, y0, h, total, lines, total.saturating_sub(fl.scroll + lines));
        gu.bar_site(super::scrollbar::Owner::FullLog, x0 + w - 6.0, y0, h, total, lines, total.saturating_sub(fl.scroll + lines));
        // Tabs (FUN_10137b80; list FUN_10138800): fep sprite pairs 0x10333b4c, unselected A[2p] / selected A[2p + 1];
        // the selected one advances 64, the others 30 (they overlap).
        const A: [usize; 22] = [0x12, 0x1b, 0x13, 0x1c, 0x0a, 0x14, 0x0c, 0x16, 0x0d, 0x17, 0x0e, 0x18, 0x10, 0x19, 0x1d, 0x1f, 0x1e, 0x20, 0x11, 0x1a, 0x0b, 0x15];
        let mut x = x0;
        for f in full_tabs(second) {
            let on = f == fl.filter;
            let k = A[2 * tab_pair(f) + on as usize];
            gu.sprite("menu    fep", k, x, y0 - 9.0, 0x8080_8080);
            x += if on { gu.sprite_width("menu    fep", k) } else { 30.0 };
        }
        [x0, y0, w, h]
    }

    /// Visible lines (+0x31).
    pub fn lines(&self) -> usize {
        self.anim.lines()
    }

    /// The frame fading out or faded (0 lines shown): FUN_1015f790 then puts stacked windows on the log's bottom.
    pub fn faded(&self) -> bool {
        self.anim.faded()
    }

    /// Its lines rebuilt at the next update (a window's init on every open).
    pub fn reopen(&mut self) {
        self.width = -1.0;
    }

    /// Window 2's frame: its width and eased height, placed on window 1's frame `w1` by config 0xb4.
    pub fn frame_beside(&mut self, sw: f32, w1: [f32; 4], mode: u32) -> LogFrame {
        let w = text_width(sw, self.cfg.pct) + 14.0;
        let h = self.anim.h.max(0) as f32;
        LogFrame { rect: second_rect(mode, w1, w, h), lines: self.lines(), alpha: self.anim.alpha() }
    }

    /// The frame's rect (x 16, bottom H - 16, the height the step eased), its lines and the fade's alpha.
    pub fn frame(&mut self, sw: f32, sh: f32) -> LogFrame {
        let w = text_width(sw, self.cfg.pct) + 14.0;
        let h = self.anim.h.max(0) as f32;
        LogFrame { rect: [16.0, sh - 16.0 - h, w, h], lines: self.lines(), alpha: self.anim.alpha() }
    }

    /// Draws the text: newest line at the bottom, lines 16 px apart, clipped to [x0, y0 + 3, x1, y1 - 4]; the
    /// input line takes the bottom 16 px. Returns the newest line's (x, y, text) for the kaipage arrow.
    pub fn draw(&self, gu: &mut GameUi, f: &LogFrame, typing: bool) -> Option<(f32, f32, String)> {
        let [x0, y0, w, h] = f.rect;
        // FUN_10162510: only while there are visible lines; from the displayed line up (FUN_101622e0), whole pixels.
        let a = &self.anim;
        if a.vis <= 0 || h <= 0.0 {
            return None;
        }
        let mut y1 = y0 + h;
        if typing {
            y1 -= 16.0;
        }
        let clip = [x0, y0 + 3.0, w, (y1 - 4.0 - (y0 + 3.0)).max(0.0)];
        gu.clip(Some(clip));
        let mut newest = None;
        for (k, (line, colour, st, _)) in self.wrapped.iter().rev().skip(a.pending).take(a.draw_n()).enumerate() {
            let y = (y1 - 4.0) - 16.0 * (k as f32 + 1.0) + a.off as f32;
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
