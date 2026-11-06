//! The change notes (ours, not retail): when Update installs a newer vana-client, the release notes of every version
//! after the running one up to the new one are saved beside the config, and the restarted client shows them once over
//! the title menu in either UI. The notes are the releases' Markdown, read into blocks both UIs lay out.

use super::game_ui::GameUi;
use super::updater::newer;
use crate::audio::Ui as Se;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};

/// One release's notes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub tag: String,
    /// The release date (yyyy-mm-dd).
    pub date: String,
    pub body: String,
}

/// The file an update leaves: the version it replaced, the one it installed, their notes newest first.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Saved {
    pub from: String,
    pub to: String,
    pub notes: Vec<Note>,
}

/// The releases after `from` up to `to`, newest first (drafts and pre-releases left out).
pub fn fetch(from: &str, to: &str) -> Result<Vec<Note>> {
    // VANA_RELEASES_API: another endpoint for the test drives (a local mock of the GitHub answer).
    let url = std::env::var("VANA_RELEASES_API").unwrap_or_else(|_| format!("https://api.github.com/repos/{}/releases?per_page=50", super::updater::RELEASE_REPO));
    let v: serde_json::Value = serde_json::from_str(&super::updater::get_text(&url)?)?;
    Ok(between(&v, from, to))
}

fn between(v: &serde_json::Value, from: &str, to: &str) -> Vec<Note> {
    let mut out: Vec<Note> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?.to_string();
            let date = r["published_at"].as_str().unwrap_or_default().chars().take(10).collect();
            (newer(&tag, from) && !newer(&tag, to)).then(|| Note { tag, date, body: r["body"].as_str().unwrap_or_default().to_string() })
        })
        .collect();
    out.sort_by(|a, b| {
        use std::cmp::Ordering;
        if newer(&a.tag, &b.tag) {
            Ordering::Less
        } else if newer(&b.tag, &a.tag) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    out
}

pub fn save(s: &Saved) -> Result<()> {
    let path = crate::config::whats_new_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

/// What to do with a saved file at start, running `me`: show it (Some(true)), keep it for later (None: it names a newer
/// version, so the restart did not take), or drop it (Some(false): an older version's, or empty).
fn decide(s: &Saved, me: &str) -> Option<bool> {
    if newer(&s.to, me) {
        return None;
    }
    Some(!newer(me, &s.to) && !s.notes.is_empty())
}

/// At start: the notes the last update left for this version.
pub fn pending() -> Option<Saved> {
    let path = crate::config::whats_new_path();
    let s: Saved = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    match decide(&s, env!("CARGO_PKG_VERSION")) {
        Some(true) => Some(s),
        Some(false) => {
            let _ = std::fs::remove_file(&path);
            None
        }
        None => None,
    }
}

/// The notes were read: they are not shown again.
pub fn seen() {
    let _ = std::fs::remove_file(crate::config::whats_new_path());
}

/// A client newer than the notes last seen (`seen`, config notes_seen; empty: a config from before the notes).
pub fn due(seen: &str) -> bool {
    seen.is_empty() || newer(env!("CARGO_PKG_VERSION"), seen)
}

/// The notes since `seen` up to this version, asked of GitHub in the background (a config from before the notes gets
/// this version's alone): Some(notes, possibly none) once answered, None while asking or when it failed.
pub type Fetch = std::sync::Arc<std::sync::Mutex<Option<Option<Saved>>>>;

pub fn fetch_since(seen: String) -> Fetch {
    let out: Fetch = Default::default();
    let o = out.clone();
    std::thread::spawn(move || {
        let me = env!("CARGO_PKG_VERSION");
        let r = if seen.is_empty() { fetch("0", me).map(|n| n.into_iter().take(1).collect()) } else { fetch(&seen, me) };
        *o.lock().unwrap() = match r {
            Ok(notes) => Some((!notes.is_empty()).then(|| Saved { from: seen, to: me.into(), notes })),
            Err(e) => {
                log::warn!("change notes: {e:#}");
                None
            }
        };
    });
    out
}

/// A run of text in one style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A release's own heading: its tag and date.
    Release(String, String),
    Heading(u8, Vec<Span>),
    /// A paragraph (depth 0) or a list item's text (its list depth), with the item's bullet on its first paragraph.
    Text { depth: u8, bullet: bool, spans: Vec<Span> },
    /// A fenced or indented code block, a line each.
    Code(Vec<String>),
    /// Rows of cells (the first the header).
    Table(Vec<Vec<Vec<Span>>>),
}

/// Every release's blocks, newest first. A body's leading heading that names its version ("What's new in 1.0.7") is
/// left out: the release heading says it.
pub fn blocks(notes: &[Note]) -> Vec<Block> {
    let mut out = Vec::new();
    for n in notes {
        out.push(Block::Release(n.tag.trim_start_matches('v').to_string(), n.date.clone()));
        let mut b = parse(&n.body);
        let version = n.tag.trim_start_matches('v');
        if let Some(Block::Heading(_, spans)) = b.first()
            && plain(spans).contains(version)
        {
            b.remove(0);
        }
        out.extend(b);
    }
    out
}

/// The spans' text.
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// Markdown into blocks: headings, paragraphs, nested lists, code, tables; bold and inline code kept, links as their
/// text, the rest plain.
pub fn parse(md: &str) -> Vec<Block> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut out = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let (mut depth, mut bullet, mut bold) = (0u8, false, 0u32);
    let mut heading: Option<u8> = None;
    let mut code: Option<String> = None;
    let mut table: Option<Vec<Vec<Vec<Span>>>> = None;
    let mut row: Vec<Vec<Span>> = Vec::new();
    let push = |spans: &mut Vec<Span>, text: &str, bold: bool, code: bool| match spans.last_mut() {
        Some(s) if s.bold == bold && s.code == code => s.text.push_str(text),
        _ => spans.push(Span { text: text.to_string(), bold, code }),
    };
    // The text so far as a paragraph or an item's text.
    let flush = |out: &mut Vec<Block>, spans: &mut Vec<Span>, depth: u8, bullet: &mut bool| {
        if spans.iter().any(|s| !s.text.trim().is_empty()) {
            if let Some(s) = spans.first_mut() {
                s.text = s.text.trim_start().to_string();
            }
            out.push(Block::Text { depth, bullet: *bullet, spans: std::mem::take(spans) });
            *bullet = false;
        }
        spans.clear();
    };
    for ev in Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                heading = Some(level as u8);
            }
            Event::End(TagEnd::Heading(_)) => {
                out.push(Block::Heading(heading.take().unwrap_or(2), std::mem::take(&mut spans)));
            }
            Event::Start(Tag::List(_)) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                depth = depth.saturating_sub(1);
            }
            Event::Start(Tag::Item) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                bullet = true;
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph | TagEnd::BlockQuote(_)) => flush(&mut out, &mut spans, depth, &mut bullet),
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                code = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                let c = code.take().unwrap_or_default();
                out.push(Block::Code(c.trim_end_matches('\n').lines().map(str::to_string).collect()));
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut out, &mut spans, depth, &mut bullet);
                table = Some(Vec::new());
            }
            Event::End(TagEnd::Table) => out.extend(table.take().map(Block::Table)),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::TableCell) => row.push(std::mem::take(&mut spans)),
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Text(t) => match code.as_mut() {
                Some(c) => c.push_str(&t),
                None => push(&mut spans, &t, bold > 0, false),
            },
            Event::Code(t) => push(&mut spans, &t, bold > 0, true),
            Event::SoftBreak | Event::HardBreak => push(&mut spans, " ", bold > 0, false),
            _ => {}
        }
    }
    flush(&mut out, &mut spans, depth, &mut bullet);
    out
}

/// Table columns (their widest cells) fitted to `width`: a column within its share keeps its width, the wider ones
/// split what is left by their widths.
pub fn fit_columns(widths: &mut [f32], width: f32) {
    let total: f32 = widths.iter().sum();
    if total <= width || widths.is_empty() {
        return;
    }
    let mut keep = vec![false; widths.len()];
    loop {
        let left = width - widths.iter().zip(&keep).filter(|(_, k)| **k).map(|(w, _)| w).sum::<f32>();
        let n = keep.iter().filter(|k| !**k).count().max(1);
        let share = left / n as f32;
        let mut changed = false;
        for (w, k) in widths.iter().zip(keep.iter_mut()) {
            if !*k && *w <= share {
                *k = true;
                changed = true;
            }
        }
        if !changed {
            let wide: f32 = widths.iter().zip(&keep).filter(|(_, k)| !**k).map(|(w, _)| w).sum();
            for (w, k) in widths.iter_mut().zip(&keep) {
                if !*k {
                    *w *= left / wide;
                }
            }
            return;
        }
    }
}

/// What the window does next for the app.
#[derive(Default)]
pub struct Out {
    pub closed: bool,
    pub sounds: Vec<Se>,
    /// The pointer is over OK (the hit cursor).
    pub hover: bool,
}

/// The window as both UIs place it (UI pixels): the frame, the text's area, the scroll bar's track, OK and whether it
/// has the cursor, the line height.
pub struct View {
    pub frame: [f32; 4],
    pub text: [f32; 4],
    pub bar: [f32; 4],
    pub ok: [f32; 4],
    pub line: f32,
}

/// How a drawing UI measured the text: a line's step, the page and the scroll's end, in its units (classic lines, the
/// modern UI's points).
#[derive(Clone, Copy, Default)]
pub struct Metrics {
    pub step: f32,
    pub page: f32,
    pub max: f32,
}

pub struct NotesUi {
    pub saved: Saved,
    pub blocks: Vec<Block>,
    /// The first line shown (classic) or the points scrolled (modern).
    pub scroll: f32,
    pub metrics: Cell<Metrics>,
    /// The view's scale about the screen's centre: 1 classic, larger for the modern UI's text.
    pub zoom: f32,
    /// The classic layout for a width.
    classic: RefCell<Option<(f32, Vec<Line>)>>,
}

const W: f32 = 600.0;
const H: f32 = 440.0;

impl NotesUi {
    pub fn open(saved: Saved) -> Self {
        let blocks = blocks(&saved.notes);
        Self { saved, blocks, scroll: 0.0, metrics: Cell::new(Metrics::default()), zoom: 1.0, classic: RefCell::new(None) }
    }

    pub fn title(&self) -> String {
        format!("What's New in vana-client {}", self.saved.to.trim_start_matches('v'))
    }

    pub fn help(&self) -> String {
        "What changed since your last version. Up / Down, Page Up / Page Down or the wheel scrolls; Enter closes.".into()
    }

    fn scroll_by(&mut self, d: f32) -> bool {
        let m = self.metrics.get();
        let s = (self.scroll + d).clamp(0.0, m.max.max(0.0));
        let moved = s != self.scroll;
        self.scroll = s;
        moved
    }

    pub fn key(&mut self, key: egui::Key) -> Out {
        use egui::Key as K;
        let mut out = Out::default();
        let m = self.metrics.get();
        let d = match key {
            K::ArrowDown => m.step,
            K::ArrowUp => -m.step,
            K::PageDown | K::Space => m.page,
            K::PageUp => -m.page,
            K::End => f32::MAX,
            K::Home => f32::MIN,
            K::Enter | K::Escape => {
                out.closed = true;
                out.sounds.push(if key == K::Enter { Se::Select } else { Se::Cancel });
                return out;
            }
            _ => return out,
        };
        if self.scroll_by(d) {
            out.sounds.push(Se::Cursor);
        }
        out
    }

    /// Wheel notches (up positive): three lines each.
    pub fn wheel(&mut self, notches: i32) {
        let step = self.metrics.get().step;
        self.scroll_by(-(notches as f32) * 3.0 * step);
    }

    /// The pointer: OK takes a click.
    pub fn pointer(&mut self, p: (f32, f32), clicked: bool, sw: f32, sh: f32) -> Out {
        let r = self.view(sw, sh).ok;
        if p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3] {
            if clicked {
                return self.key(egui::Key::Enter);
            }
            return Out { hover: true, ..Out::default() };
        }
        Out::default()
    }

    pub fn view(&self, sw: f32, sh: f32) -> View {
        let z = self.zoom;
        let (w, h) = ((W * z).min(sw - 32.0), (H * z).min(sh - 48.0));
        let (x, y) = (((sw - w) / 2.0).round(), ((sh - h) / 2.0).round());
        let line = 16.0 * z;
        let pad = 16.0 * z;
        let foot = 40.0 * z;
        let bar_w = 10.0 * z;
        View {
            frame: [x, y, w, h],
            text: [x + pad, y + 2.0 * line, w - 2.0 * pad - bar_w - 6.0 * z, h - 2.0 * line - foot],
            bar: [x + w - pad - bar_w, y + 2.0 * line, bar_w, h - 2.0 * line - foot],
            ok: [x + w / 2.0 - 52.0 * z, y + h - 30.0 * z, 104.0 * z, 18.0 * z],
            line,
        }
    }

    /// The classic window: the game's frame and title strip, the notes in the game font (bold white, code green), the
    /// list scroll bar, OK.
    pub fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) {
        let v = self.view(sw, sh);
        let [x, y, w, h] = v.frame;
        let white = [0x80u8; 4];
        gu.rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, [0.0, 0.0, 0.0, 0.7]);
        gu.frame(x, y, w, h);
        gu.cell_stretch("menu    windowps", 465, 1, x, y + 4.0, w, 16.0, 0x8080_8080);
        gu.text(&self.title(), x + 8.0, y + 4.0, 0, white);
        let [tx, ty, tw, th] = v.text;
        let mut cache = self.classic.borrow_mut();
        if cache.as_ref().is_none_or(|(cw, _)| *cw != tw) {
            *cache = Some((tw, classic_lines(gu, &self.blocks, tw)));
        }
        let lines = &cache.as_ref().unwrap().1;
        let page = (th / 16.0).floor().max(1.0) as usize;
        self.metrics.set(Metrics { step: 1.0, page: page.saturating_sub(1).max(1) as f32, max: lines.len().saturating_sub(page) as f32 });
        let top = (self.scroll as usize).min(lines.len().saturating_sub(page));
        for (k, l) in lines.iter().skip(top).take(page).enumerate() {
            let ly = ty + 16.0 * k as f32;
            if l.strip {
                gu.cell_stretch("menu    windowps", 465, 1, tx - 4.0, ly, tw + 8.0, 16.0, 0x8080_8080);
            }
            if let Some(bx) = l.bullet {
                gu.text("\u{30fb}", tx + bx, ly, 0, l.colour);
            }
            for (cx, t) in &l.cells {
                gu.text(t, tx + l.x + cx, ly, 0, l.colour);
            }
        }
        if lines.len() > page {
            gu.scroll_bar(v.bar[0], v.bar[1], v.bar[3], lines.len(), page, top);
        }
        super::settings::button(gu, v.ok, 0x8080_5020);
        gu.text("OK", v.ok[0] + v.ok[2] / 2.0, v.ok[1], 1, white);
        gu.cursor(v.ok[0], v.ok[1]);
    }
}

/// A classic line: its x in the text area, its bullet's x, its pieces (x, text with colour codes), its colour, and
/// whether a title strip sits behind it (a release's heading).
struct Line {
    x: f32,
    bullet: Option<f32>,
    cells: Vec<(f32, String)>,
    colour: [u8; 4],
    strip: bool,
}

/// The game font's in-line colours: bold white (1e 01), code green (1e 02, the item names'), back to the line's own.
fn coded(spans: &[Span]) -> String {
    use crate::dat::font::colour_code;
    let mut s = String::new();
    for sp in spans {
        let c = if sp.code { Some(2) } else if sp.bold { Some(1) } else { None };
        if let Some(c) = c {
            s.push(colour_code(c));
        }
        s.push_str(&sp.text);
        if c.is_some() {
            s.push(colour_code(0));
        }
    }
    s
}

fn classic_lines(gu: &GameUi, blocks: &[Block], width: f32) -> Vec<Line> {
    const BODY: [u8; 4] = [0x6c, 0x6c, 0x6c, 0x80];
    const WHITE: [u8; 4] = [0x80; 4];
    const GREEN: [u8; 4] = [0x50, 0x78, 0x50, 0x80];
    let mut out: Vec<Line> = Vec::new();
    let blank = |out: &mut Vec<Line>| {
        if out.last().is_some_and(|l| !l.cells.is_empty() || l.strip) {
            out.push(Line { x: 0.0, bullet: None, cells: Vec::new(), colour: BODY, strip: false });
        }
    };
    let one = |x: f32, text: String, colour: [u8; 4]| Line { x, bullet: None, cells: vec![(0.0, text)], colour, strip: false };
    for b in blocks {
        match b {
            Block::Release(v, date) => {
                blank(&mut out);
                let t = if date.is_empty() { format!("vana-client {v}") } else { format!("vana-client {v}  ({date})") };
                out.push(Line { strip: true, ..one(4.0, t, WHITE) });
                out.push(Line { x: 0.0, bullet: None, cells: Vec::new(), colour: BODY, strip: false });
            }
            Block::Heading(_, spans) => {
                blank(&mut out);
                for l in gu.wrap(&plain(spans), width) {
                    out.push(one(0.0, l, WHITE));
                }
            }
            Block::Text { depth, bullet, spans } => {
                if *depth == 0 {
                    blank(&mut out);
                }
                let x = if *depth == 0 { 0.0 } else { 14.0 * *depth as f32 + 2.0 };
                for (k, l) in gu.wrap(&coded(spans), width - x).into_iter().enumerate() {
                    let mut line = one(x, l, BODY);
                    if k == 0 && *bullet {
                        line.bullet = Some(x - 13.0);
                    }
                    out.push(line);
                }
            }
            Block::Code(lines) => {
                blank(&mut out);
                for c in lines {
                    for l in gu.wrap(c, width - 16.0) {
                        out.push(one(16.0, l, GREEN));
                    }
                }
                blank(&mut out);
            }
            Block::Table(rows) => {
                blank(&mut out);
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
                // Each column as wide as its widest cell, fitted to the width.
                let mut widths = vec![0f32; cols];
                for r in rows {
                    for (c, cell) in r.iter().enumerate() {
                        widths[c] = widths[c].max(gu.text_width(&plain(cell)) + 16.0);
                    }
                }
                fit_columns(&mut widths, width);
                for (i, r) in rows.iter().enumerate() {
                    let wrapped: Vec<Vec<String>> = r.iter().enumerate().map(|(c, cell)| gu.wrap(&coded(cell), widths[c] - 12.0)).collect();
                    let n = wrapped.iter().map(Vec::len).max().unwrap_or(0);
                    for k in 0..n {
                        let mut cells = Vec::new();
                        let mut cx = 0.0;
                        for (c, w) in wrapped.iter().enumerate() {
                            if let Some(t) = w.get(k) {
                                cells.push((cx, t.clone()));
                            }
                            cx += widths[c];
                        }
                        out.push(Line { x: 0.0, bullet: None, cells, colour: if i == 0 { WHITE } else { BODY }, strip: false });
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Block, Note, Saved, Span, between, blocks, decide, parse};

    #[test]
    fn markdown_into_blocks() {
        let md = "## What's new in 1.0.7\n\n### Updates\n- **Each platform** updates to `its own` build.\n  - nested one\n\nA paragraph\nwrapped.\n\n| Download | For |\n|---|---|\n| `a` | Linux |\n\n```\nchmod +x a\n```\n";
        let b = parse(md);
        let s = |t: &str| Span { text: t.into(), bold: false, code: false };
        assert_eq!(b[0], Block::Heading(2, vec![s("What's new in 1.0.7")]));
        assert_eq!(b[1], Block::Heading(3, vec![s("Updates")]));
        assert_eq!(
            b[2],
            Block::Text { depth: 1, bullet: true, spans: vec![Span { text: "Each platform".into(), bold: true, code: false }, s(" updates to "), Span { text: "its own".into(), bold: false, code: true }, s(" build.")] }
        );
        assert_eq!(b[3], Block::Text { depth: 2, bullet: true, spans: vec![s("nested one")] });
        assert_eq!(b[4], Block::Text { depth: 0, bullet: false, spans: vec![s("A paragraph wrapped.")] });
        let Block::Table(rows) = &b[5] else { panic!("{:?}", b[5]) };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][1], vec![s("Linux")]);
        assert_eq!(b[6], Block::Code(vec!["chmod +x a".into()]));
        // The release heading stands for the body's own.
        let all = blocks(&[Note { tag: "v1.0.7".into(), date: "2026-11-06".into(), body: md.into() }]);
        assert_eq!(all[0], Block::Release("1.0.7".into(), "2026-11-06".into()));
        assert_eq!(all[1], Block::Heading(3, vec![s("Updates")]));
    }

    /// The live GitHub answer (by hand: `cargo test --release notes_from_github -- --ignored`).
    #[test]
    #[ignore]
    fn notes_from_github() {
        let n = super::fetch("1.0.5", "v1.0.7").unwrap();
        assert_eq!(n.iter().map(|n| n.tag.as_str()).collect::<Vec<_>>(), ["v1.0.7", "v1.0.6"]);
        assert!(n.iter().all(|n| !n.body.is_empty() && n.date.len() == 10));
    }

    #[test]
    fn narrow_columns_keep_their_width() {
        let mut w = [250.0, 500.0];
        super::fit_columns(&mut w, 600.0);
        assert_eq!(w, [250.0, 350.0]);
        let mut w = [400.0, 400.0];
        super::fit_columns(&mut w, 600.0);
        assert_eq!(w, [300.0, 300.0]);
    }

    #[test]
    fn releases_between_versions_newest_first() {
        let v = serde_json::json!([
            {"tag_name": "v1.0.5", "body": "five", "published_at": "2026-11-01T00:00:00Z"},
            {"tag_name": "v1.0.8", "body": "eight", "published_at": "2026-11-08T00:00:00Z"},
            {"tag_name": "v1.0.7", "body": "seven", "published_at": "2026-11-06T00:00:00Z"},
            {"tag_name": "v1.0.6", "body": "six", "draft": true},
            {"tag_name": "v1.0.9-rc1", "body": "rc", "prerelease": true},
        ]);
        let n = between(&v, "1.0.5", "v1.0.8");
        assert_eq!(n.iter().map(|n| n.tag.as_str()).collect::<Vec<_>>(), ["v1.0.8", "v1.0.7"]);
        assert_eq!(n[1].date, "2026-11-06");
    }

    #[test]
    fn saved_notes_show_once_for_their_version() {
        let s = Saved { from: "1.0.6".into(), to: "v1.0.7".into(), notes: vec![Note { tag: "v1.0.7".into(), date: String::new(), body: "x".into() }] };
        assert_eq!(decide(&s, "1.0.7"), Some(true));
        // The restart did not take: kept for the new binary.
        assert_eq!(decide(&s, "1.0.6"), None);
        // A later version runs: stale.
        assert_eq!(decide(&s, "1.0.8"), Some(false));
        assert_eq!(decide(&Saved { notes: Vec::new(), ..s }, "1.0.7"), Some(false));
    }

    #[test]
    fn notes_due_for_a_newer_client() {
        let me = env!("CARGO_PKG_VERSION");
        assert!(super::due("") && super::due("0.9.0"));
        assert!(!super::due(me));
        // A fresh config starts seen.
        assert_eq!(crate::config::Config::default().notes_seen, me);
    }
}
