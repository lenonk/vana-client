//! The modern chat window: the log's lines in their colours (in-line codes applied as the font does), filter tabs,
//! timestamps as the log has them, the chat line while typing, and the Enter key cap while a message waits for a key.

use super::super::{World, cfgapply, event, fep};
use super::{Modern, Panels, paint};
use crate::dat::font::{CODE_BASE, code_table};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Sense, Ui, pos2, vec2};
use std::sync::OnceLock;

/// The tabs: every line, chat lines (s2c 0x017 and our sends), the rest (battle and system).
pub const TABS: [&str; 3] = ["All", "Chat", "Battle"];
/// Lines kept in the window's scroll.
const KEEP: usize = 300;

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

fn tabs(ui: &mut Ui, m: &Modern) {
    let tab = m.chat_tab.get();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (k, name) in TABS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(78.0, 24.0), Sense::click());
            let s = if k == tab as usize {
                "controls/tab_active"
            } else if resp.hovered() {
                "controls/tab_hover"
            } else {
                "controls/tab_normal"
            };
            if let Some(s) = m.art.sprite(s) {
                ui.painter().add(paint::nine_slice(&s, r, r.height() / s.size.y, Color32::WHITE));
            }
            let c = if k == tab as usize { m.theme.color("gold") } else { m.theme.color("text") };
            ui.painter().text(r.center() + vec2(0.0, 2.0), egui::Align2::CENTER_CENTER, *name, FontId::proportional(m.theme.small), c);
            if resp.clicked() {
                m.chat_tab.set(k as u8);
            }
        }
    });
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

pub fn view(w: &World) -> ChatView<'_> {
    let stamps = (w.game.get(0xb5), w.stamp_all_lines);
    let all: Vec<_> = w.chat.iter_timed().collect();
    let lines = all[all.len().saturating_sub(KEEP)..]
        .iter()
        .map(|((line, c), (time, chat, _))| {
            let st = match stamps {
                (mode, _) if *chat && mode != 0 => cfgapply::stamp_text(mode, *time),
                (mode, true) => cfgapply::stamp_text(if mode == 0 { 2 } else { mode }, *time),
                _ => String::new(),
            };
            (st, line.as_str(), *c, *chat)
        })
        .collect();
    let input = w.typing.then(|| InputView { channel: channel(super::super::chatctrl::line_mode(&w.chat_input, w.chat_mode)), pieces: fep::pieces(w, &w.chat_input), caret: fep::caret(w) });
    ChatView { lines, input, waiting: event::prompt(w) }
}

/// The chat line in the pack's focused input: the channel, the text (phrases in their blue) and the caret.
fn input(ui: &mut Ui, m: &Modern, v: &InputView, t: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    if let Some(s) = m.art.sprite("controls/input_focused") {
        ui.painter().add(paint::nine_slice(&s, r, r.height() / s.size.y, Color32::WHITE));
    }
    let font = FontId::proportional(m.theme.ui);
    let p = ui.painter().with_clip_rect(r.shrink(4.0));
    let cr = p.text(pos2(r.min.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, v.channel, FontId::proportional(m.theme.small), m.theme.color("gold"));
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
        p.vline(caret_x + 1.0, (r.center().y - 8.0)..=(r.center().y + 8.0), egui::Stroke::new(1.5, m.theme.color("text")));
    }
}

pub fn panel(p: &mut Panels, v: &ChatView) {
    let m = p.m;
    let tab = m.chat_tab.get();
    let t = p.ctx.input(|i| i.time) as f32;
    p.panel("chat", None, 620.0, [0.012, 0.74], |ui| {
        tabs(ui, m);
        ui.add_space(4.0);
        let font = FontId::proportional(m.theme.ui);
        let wrap = ui.available_width() - 12.0;
        egui::ScrollArea::vertical().id_salt("modern chat").stick_to_bottom(true).auto_shrink([false, false]).max_height(8.0 * 19.0).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for (st, line, c, chat) in &v.lines {
                if tab == 0 || (tab == 1) == *chat {
                    ui.label(line_job((st, m.theme.color("muted")), line, *c, &font, wrap));
                }
            }
        });
        if v.waiting && let Some(s) = m.art.sprite("controls/key_enter") {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().add(paint::image(&s, pos2(r.max.x - 44.0, r.min.y), 44.0, Color32::WHITE));
        }
        if let Some(i) = &v.input {
            ui.add_space(4.0);
            input(ui, m, i, t);
        }
    });
}
