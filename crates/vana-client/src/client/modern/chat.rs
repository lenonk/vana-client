//! The modern chat window: the log's lines in their colours (in-line codes applied as the font does), filter tabs,
//! timestamps as the log has them, the chat line while typing, the Enter key cap while a message waits for a key, and
//! its settings (the font size) behind the button at the tabs' right. It grows and shrinks by the retail log's rule
//! (logwin::LogAnim) on its own rows.

use super::super::{World, cfgapply, event, fep};
use super::{Modern, Panels, Show, paint};
use crate::dat::font::{CODE_BASE, code_table};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Id, Order, Rect, Sense, Ui, Vec2, pos2, vec2};
use std::sync::OnceLock;

/// The tabs: every line, chat lines (s2c 0x017 and our sends), the rest (battle and system).
pub const TABS: [&str; 3] = ["All", "Chat", "Battle"];
/// Lines kept in the window's scroll.
const KEEP: usize = 300;
/// The default room: the width and the lines' height in points.
const ROOM: Vec2 = vec2(620.0, 8.0 * 19.0);
/// The font sizes the settings offer.
const FONT_SIZES: std::ops::RangeInclusive<f32> = 10.0..=32.0;
/// The tab row's height and the gap under it.
const TAB_H: f32 = 24.0;
const GAP: f32 = 4.0;

/// An FFXI colour (b, g, r, a at 0x80 = 1.0).
pub fn colour(c: [u8; 4]) -> Color32 {
    let k = |v: u8| (v as u32 * 2).min(255) as u8;
    Color32::from_rgba_unmultiplied(k(c[2]), k(c[1]), k(c[0]), k(c[3]))
}

/// The colours of `1e 00..10` (FFXiMain's table; 0 = the line's own).
fn codes() -> &'static [u32; 17] {
    static T: OnceLock<[u32; 17]> = OnceLock::new();
    T.get_or_init(|| {
        let dll = std::fs::read(crate::config::Config::load().ffxi_dir().join("FFXiMain.dll")).unwrap_or_default();
        crate::dat::tables::code_colours(&dll).unwrap_or_default()
    })
}

/// A log line as coloured runs after its timestamp: `1e xx` sets the colour (the 17-entry table below 0x11, the colour
/// ids above, 0 the line's own), `1f xx` a mode's colour; the auto-translate brackets are green / red braces.
pub fn line_job(stamp: (&str, Color32), line: &str, base: [u8; 4], font: &FontId, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    if !stamp.0.is_empty() {
        job.append(stamp.0, 0.0, TextFormat::simple(font.clone(), stamp.1));
    }
    let mut cur = base;
    let mut run = String::new();
    let flush = |job: &mut LayoutJob, run: &mut String, c: [u8; 4]| {
        if !run.is_empty() {
            job.append(run, 0.0, TextFormat::simple(font.clone(), colour(c)));
            run.clear();
        }
    };
    for ch in line.chars() {
        let cp = ch as u32;
        if (CODE_BASE..CODE_BASE + 0x200).contains(&cp) {
            flush(&mut job, &mut run, cur);
            let (kind, xx) = (((cp - CODE_BASE) >> 8) as usize, ((cp - CODE_BASE) & 0xff) as usize);
            let c = match (kind, xx) {
                (0, 0..0x11) => codes()[xx].to_le_bytes(),
                _ => code_table(kind, xx),
            };
            cur = if kind == 0 && c == [0; 4] { base } else { [c[0], c[1], c[2], base[3]] };
            continue;
        }
        match ch {
            '\u{E008}' | '\u{E009}' => {
                flush(&mut job, &mut run, cur);
                let (t, c) = if ch == '\u{E008}' { ("{", Color32::from_rgb(0x50, 0xd0, 0x50)) } else { ("}", Color32::from_rgb(0xe0, 0x50, 0x50)) };
                job.append(t, 0.0, TextFormat::simple(font.clone(), c));
            }
            // The game's own glyphs and other private-use characters have no face here.
            c if ('\u{E000}'..='\u{F8FF}').contains(&c) || c as u32 >= 0xF0000 => {}
            c => run.push(c),
        }
    }
    flush(&mut job, &mut run, cur);
    job
}

/// The tab row at the window's fade, the settings button at its right: the button's rect and whether it was clicked.
fn tabs(ui: &mut Ui, m: &Modern, alpha: f32) -> (Rect, bool) {
    let tab = m.chat_tab.get();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_H), Sense::hover());
    for (i, name) in TABS.iter().enumerate() {
        let r = Rect::from_min_size(row.min + vec2(i as f32 * 82.0, 0.0), vec2(78.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with(("chat tab", i)), Sense::click());
        let s = if i == tab as usize {
            "controls/tab_active"
        } else if resp.hovered() {
            "controls/tab_hover"
        } else {
            "controls/tab_normal"
        };
        if let Some(s) = m.art.sprite(s) {
            ui.painter().add(paint::nine_slice(&s, r, r.height() / s.size.y, Color32::WHITE.gamma_multiply(alpha)));
        }
        let c = if i == tab as usize { m.theme.color("gold") } else { m.theme.color("text") };
        ui.painter().text(r.center() + vec2(0.0, 2.0), egui::Align2::CENTER_CENTER, *name, FontId::proportional(m.theme.small), c.gamma_multiply(alpha));
        if resp.clicked() {
            m.chat_tab.set(i as u8);
        }
    }
    let g = Rect::from_min_size(pos2(row.max.x - row.height(), row.min.y), Vec2::splat(row.height()));
    let resp = ui.interact(g, ui.scope_id().with("chat settings"), Sense::click());
    if let Some(s) = m.art.sprite("icons/settings") {
        let open = m.chat_settings.get();
        let c = if resp.hovered() || open { Color32::WHITE } else { Color32::from_gray(205) };
        ui.painter().add(paint::image(&s, g.min, g.width(), c.gamma_multiply(alpha)));
    }
    (g, resp.on_hover_text("Chat settings").clicked())
}

/// The settings below the button (`at`, on the screen): the font size, - and +. Closed by the button or a click
/// elsewhere; true when the size changed.
fn settings(ctx: &egui::Context, m: &Modern, at: Rect, size: &mut f32) -> bool {
    let mut changed = false;
    let shown = egui::Area::new(Id::unique("modern chat settings")).order(Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos2(at.max.x, at.max.y + 6.0)).show(ctx, |ui| {
        let bg = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(super::MARGIN).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (r, _) = ui.allocate_exact_size(vec2(96.0, 28.0), Sense::hover());
                ui.painter().text(pos2(r.min.x, r.center().y), egui::Align2::LEFT_CENTER, "Font Size", FontId::proportional(m.theme.ui), m.theme.color("text"));
                let now = *size;
                let mut step = |ui: &mut Ui, label: &str, d: f32| {
                    let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let can = FONT_SIZES.contains(&(now + d));
                    let face = if !can {
                        "controls/button_disabled"
                    } else if resp.hovered() {
                        "controls/button_hover"
                    } else {
                        "controls/button_normal"
                    };
                    if let Some(s) = m.art.sprite(face) {
                        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                    }
                    let c = if can { m.theme.color("text") } else { m.theme.color("muted") };
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(m.theme.title), c);
                    if can && resp.clicked() {
                        *size += d;
                        changed = true;
                    }
                };
                step(ui, "\u{2212}", -1.0);
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::hover());
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", now as u32), FontId::proportional(m.theme.ui), m.theme.color("gold"));
                step(ui, "+", 1.0);
            });
        });
        let r = inner.response.rect;
        if let Some(f) = m.art.frame("window_normal") {
            ui.painter().set(bg, paint::window(f, r, 1.0));
        }
        r
    });
    let r = shown.inner;
    let away = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !r.contains(p) && !at.contains(p)));
    if away {
        m.chat_settings.set(false);
    }
    changed
}

/// What the chat window shows: the lines (timestamp, text, colour, chat line or not), the chat line while typing,
/// and whether a message waits for a key.
pub struct ChatView<'a> {
    pub lines: Vec<(String, &'a str, [u8; 4], bool)>,
    pub input: Option<InputView>,
    pub waiting: bool,
}

/// The chat line: its channel, its pieces (phrases flagged) and the caret's character index.
pub struct InputView {
    pub channel: &'static str,
    pub pieces: Vec<(String, bool)>,
    pub caret: usize,
}

/// The channel a line goes to (chatctrl::line_mode's codes).
fn channel(mode: u8) -> &'static str {
    match mode {
        2 => "Say",
        3 => "Shout",
        4 => "Tell",
        5 => "Party",
        6 => "Linkshell",
        0xc => "Linkshell 2",
        0xb => "Unity",
        0xe | 0xf => "Assist",
        _ => "Command",
    }
}

/// A line's timestamp as the log has it (config 0xb5 on chat lines, the extra's every-line switch).
fn stamp_of(stamps: (u32, bool), time: [u8; 3], chat: bool) -> String {
    match stamps {
        (mode, _) if chat && mode != 0 => cfgapply::stamp_text(mode, time),
        (mode, true) => cfgapply::stamp_text(if mode == 0 { 2 } else { mode }, time),
        _ => String::new(),
    }
}

pub fn view(w: &World) -> ChatView<'_> {
    let stamps = (w.game.get(0xb5), w.stamp_all_lines);
    let all: Vec<_> = w.chat.iter_timed().collect();
    let lines = all[all.len().saturating_sub(KEEP)..].iter().map(|((line, c), (time, chat, _))| (stamp_of(stamps, *time, *chat), line.as_str(), *c, *chat)).collect();
    let input = w.typing.then(|| InputView { channel: channel(super::super::chatctrl::line_mode(&w.chat_input, w.chat_mode)), pieces: fep::pieces(w, &w.chat_input), caret: fep::caret(w) });
    ChatView { lines, input, waiting: event::prompt(w) }
}

/// The chat line in the pack's focused input at `r`: the channel, the text (phrases in their blue) at `size` and the
/// caret.
fn input(ui: &mut Ui, m: &Modern, v: &InputView, t: f32, size: f32, r: Rect) {
    if let Some(s) = m.art.sprite("controls/input_focused") {
        ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    let font = FontId::proportional(size);
    let p = ui.painter().with_clip_rect(r.shrink(4.0));
    let cr = p.text(pos2(r.min.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, v.channel, FontId::proportional(size * m.theme.small / m.theme.ui), m.theme.color("gold"));
    let mut x = cr.max.x + 10.0;
    let (mut caret_x, mut chars) = (x, 0);
    for (text, phrase) in &v.pieces {
        let c = if *phrase { Color32::from_rgb(0x80, 0xb0, 0xff) } else { m.theme.color("text") };
        let n = text.chars().count();
        if v.caret >= chars && v.caret <= chars + n {
            let before: String = text.chars().take(v.caret - chars).collect();
            caret_x = x + ui.fonts_mut(|f| f.layout_no_wrap(before, font.clone(), c).size().x);
        }
        x = p.text(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, text, font.clone(), c).max.x;
        chars += n;
    }
    if v.caret >= chars {
        caret_x = x;
    }
    // The caret blinks as the retail line's (10..24 of 26 frames at 30 a second).
    if (10..=24).contains(&((t * 30.0) as u64 % 26)) {
        p.vline(caret_x + 1.0, (r.center().y - size * 0.55)..=(r.center().y + size * 0.55), egui::Stroke::new(1.5, m.theme.color("text")));
    }
}

/// What the rule gives the window this frame (logwin::LogAnim): its height in rows over its chrome (eased; below 0 the
/// chrome itself folding away), the rows to draw from the displayed one up, the rows newer than it, and the slide in
/// 16ths of a row (retail's typing row left out: the chat line takes its own row).
#[derive(Clone, Copy, Debug)]
pub struct Shown {
    pub rows: f32,
    pub draw_n: usize,
    pub pending: usize,
    pub slide: i32,
}

/// The window's height without lines: its margins, the tab row and the gap under it.
fn chrome() -> f32 {
    super::ROOM_MARGIN.sum().y + TAB_H + GAP
}

/// The lines' wrap width for a room: its inside, less a little air at the right.
fn wrap_width(room: Vec2) -> f32 {
    room.x - super::ROOM_MARGIN.sum().x - 6.0
}

/// The chat window under the retail log's rule: each wrapped row a new line brings keeps it open a period longer, up to
/// the lines its room holds (retail's max lines; moving or resizing a panel shows them all), a row goes every period down to the min
/// lines, Escape takes one, the chat line opens two, the height eases toward the lines as retail's (half a row a frame
/// while more than a row away, then a sixth), new rows slide in, and with none it goes on folding down through its
/// chrome until it is gone, carrying what rides on it. The full log closes it, as retail closes the small logs.
pub fn panel(p: &mut Panels, w: &mut World) {
    if w.full_log.is_some() {
        return;
    }
    let m = p.m;
    let tab = m.chat_tab.get();
    let font = FontId::proportional(*p.chat_font);
    let room = p.room("chat", ROOM);
    let wrap = wrap_width(room);
    let row = p.ctx.fonts_mut(|f| f.row_height(&font));
    let mut rule = w.log_rule;
    rule.cfg.max = (room.y / row).floor().clamp(1.0, 24.0);
    rule.cfg.min = rule.cfg.min.min(rule.cfg.max);
    rule.focused |= m.arranging.replace(false);
    let stamps = (w.game.get(0xb5), w.stamp_all_lines);
    let ml = &mut w.modern_log;
    ml.anim.chat_line(rule.typing, &rule.cfg);
    let key = (wrap.to_bits(), font.size.to_bits(), tab);
    if ml.key != key {
        // A rewrap (FUN_10162d60): the newest shown, nothing sliding, nothing counted.
        (ml.key, ml.seen) = (key, w.chat.len());
        ml.anim.settle();
    }
    for ((line, c), (time, chat, _)) in w.chat.iter_timed().skip(ml.seen) {
        if tab == 0 || (tab == 1) == chat {
            let job = line_job((&stamp_of(stamps, time, chat), Color32::WHITE), line, *c, &font, wrap);
            for _ in 0..p.ctx.fonts_mut(|f| f.layout_job(job)).rows.len().max(1) {
                ml.anim.add_line(&rule.cfg);
            }
        }
    }
    ml.seen = w.chat.len();
    let frames = ml.ticker.frames();
    ml.anim.run(&rule, frames);
    let floor = -chrome() / row;
    let target = if ml.anim.lines() > 0 { ml.anim.lines() as f32 } else { floor };
    let mut h = ml.shown.unwrap_or(floor);
    for _ in 0..frames {
        let d = target - h;
        let step = if d.abs() > 1.0 { 0.5 } else { 1.0 / 6.0 };
        h = if d.abs() <= step { target } else { h + step * d.signum() };
    }
    ml.shown = Some(h.max(floor));
    let a = &ml.anim;
    let shown = Shown { rows: h.max(floor), draw_n: a.draw_n(), pending: a.pending(), slide: a.off() + if rule.line_open { 16 } else { 0 } };
    draw(p, &view(w), shown, rule.line_open);
}

/// The chat window for the rule's frame `s`: the tabs, the rows from the displayed one up, the chat line in the bottom
/// row with an empty row above it as retail leaves (`lift`), the Enter key cap after the newest line while a message
/// waits (retail's arrow).
pub fn draw(p: &mut Panels, v: &ChatView, s: Shown, lift: bool) {
    let m = p.m;
    let tab = m.chat_tab.get();
    let t = p.ctx.input(|i| i.time) as f32;
    let size = *p.chat_font;
    let font = FontId::proportional(size);
    let row = p.ctx.fonts_mut(|f| f.row_height(&font));
    let total = chrome() + s.rows * row;
    if total < 0.5 {
        // Gone: what sits on it sits on its bottom.
        let at = p.resolve("chat", &p.place("chat"), 0.0);
        let r = Rect::from_min_size(at, vec2(p.room("chat", ROOM).x, 0.0));
        p.this.insert("chat".into(), r);
        m.chat_edge.set(Some((r, true)));
        return;
    }
    let lines_h = (s.rows * row).max(0.0);
    let height = chrome() + lines_h;
    let mut button = (Rect::NOTHING, false);
    // Below its chrome it folds into its bottom edge, what rides on it going down with it.
    let r = p.show("chat", Show { size: ROOM, resizable: true, height: Some(height), fold: total / height, ..Default::default() }, |ui, room| {
        ui.spacing_mut().item_spacing.y = 0.0;
        button = tabs(ui, m, 1.0);
        ui.add_space(GAP);
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), lines_h), Sense::hover());
        let wrap = wrap_width(room);
        let input_r = v.input.as_ref().map(|_| Rect::from_min_max(pos2(area.min.x, area.max.y - row - 5.0), pos2(area.max.x, area.max.y + 5.0)));
        let base = area.max.y - if lift { 2.0 * row } else { 0.0 } + s.slide as f32 * row / 16.0;
        let top = area.min.y.max(base - s.draw_n as f32 * row);
        let clip = Rect::from_min_max(pos2(area.min.x, top), pos2(area.max.x + 8.0, input_r.map_or(area.max.y, |r| r.min.y)));
        let painter = ui.painter().with_clip_rect(clip.intersect(ui.clip_rect()));
        let mut y = base + s.pending as f32 * row;
        let mut newest = None;
        for (st, line, c, chat) in v.lines.iter().rev() {
            if !(tab == 0 || (tab == 1) == *chat) {
                continue;
            }
            if y <= clip.min.y {
                break;
            }
            let galley = painter.layout_job(line_job((st, m.theme.color("muted")), line, *c, &font, wrap));
            y -= galley.size().y;
            if newest.is_none() {
                newest = galley.rows.last().map(|r| pos2(area.min.x + r.rect().max.x, y + r.rect().center().y));
            }
            painter.galley(pos2(area.min.x, y), galley, m.theme.color("text"));
        }
        if v.waiting
            && let (Some(at), Some(k)) = (newest, m.art.sprite("controls/key_enter"))
        {
            let kw = row * 1.6;
            painter.add(paint::image(&k, pos2(at.x + 6.0, at.y - kw / 4.0), kw, Color32::WHITE));
        }
        if let (Some(i), Some(r)) = (&v.input, input_r) {
            input(ui, m, i, t, size, r);
        }
    });
    if let Some(r) = r {
        m.chat_edge.set(Some((r, false)));
    }
    if button.1 {
        m.chat_settings.set(!m.chat_settings.get());
    }
    if r.is_some() && m.chat_settings.get() && settings(p.ctx, m, button.0, p.chat_font) {
        *p.changed = true;
    }
}
