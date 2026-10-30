//! The modern UI's widget kit: the controls every modern page draws and the one key model they share, so that like
//! controls behave alike everywhere. A page describes its stops top to bottom (`Stop`); `key` turns a key into what
//! the page does (`Act`); the widgets draw them. The mouse: hovering only lights a control, never moves a cursor; a
//! click puts the cursor there and takes it; a drag moves a slider.

use super::{Modern, paint, rows, tabs};
use egui::{Align2, Color32, FontFamily, FontId, Key, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

/// One stop of a page, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stop {
    /// Tabs: Left / Right turn them; Tab / Shift+Tab turn them from anywhere on the page.
    Tabs { n: usize, at: usize },
    /// A value of `n` steps (options as buttons, a toggle, a slider): Left / Right change it at once, Enter steps an
    /// option on; options in `off` (a bit each) are passed over.
    Value { n: usize, at: usize, slider: bool, off: u64 },
    /// A row of `n` buttons: Left / Right move along it, Enter presses the cursor's (one in `off` refuses). `pair`: the
    /// first of two stacked one over the other (a pair of chevrons), Up / Down moving between them before leaving.
    Buttons { n: usize, off: u64, pair: Option<usize> },
    /// A list of `n` rows, `page` in view: Up / Down walk it and leave at its ends, Left / Right a page, Enter takes the
    /// cursor's row.
    List { n: usize, page: usize },
}

/// The cursor: the stop, and the row (a list) or button (a row of them) within it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct At {
    pub stop: usize,
    pub sub: usize,
}

impl At {
    pub fn new(stop: usize, sub: usize) -> Self {
        Self { stop, sub }
    }

}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    None,
    /// The cursor moved.
    Moved,
    /// The stop's value (or tab) set to this step.
    Set(usize, usize),
    /// The stop's button or list row pressed.
    Press(usize, usize),
    /// Enter on a button that is off.
    Refused,
    /// Escape.
    Back,
}

impl Act {
    /// The UI sound for it (Set and Press leave theirs to the page, which knows what they did).
    pub fn sound(self) -> Option<crate::audio::Ui> {
        use crate::audio::Ui as Se;
        match self {
            Act::Moved => Some(Se::Cursor),
            Act::Refused => Some(Se::Error),
            Act::Back => Some(Se::Cancel),
            _ => None,
        }
    }
}

fn bit(mask: u64, k: usize) -> bool {
    k < 64 && mask & (1 << k) != 0
}

/// The next option of a value from `at` by `d` (wrapping, or held at the ends for a slider), past the ones off.
fn step_value(n: usize, at: usize, d: i32, slider: bool, off: u64) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if slider {
        let x = (at as i32 + d).clamp(0, n as i32 - 1) as usize;
        return (x != at).then_some(x);
    }
    (1..n as i32).map(|k| (at as i32 + d * k).rem_euclid(n as i32) as usize).find(|&x| !bit(off, x))
}

/// Where Up / Down take the cursor from stop `s`: the stop before or after it (round the ends), on its last or first
/// row; a row of buttons keeps the column from the row of buttons it came from.
fn neighbour(stops: &[Stop], at: At, down: bool) -> At {
    let n = stops.len();
    let s = if down { (at.stop + 1) % n } else { (at.stop + n - 1) % n };
    let sub = match (stops[s], stops[at.stop]) {
        (Stop::List { n, .. }, _) if !down => n.saturating_sub(1),
        (Stop::Buttons { n, .. }, Stop::Buttons { .. }) => at.sub.min(n.saturating_sub(1)),
        _ => 0,
    };
    At { stop: s, sub }
}

/// The page's key: `shift` held turns Tab back. The cursor moves in `at`; what the page must do comes back.
pub fn key(stops: &[Stop], at: &mut At, key: Key, shift: bool) -> Act {
    if stops.is_empty() {
        return if key == Key::Escape { Act::Back } else { Act::None };
    }
    at.stop = at.stop.min(stops.len() - 1);
    let here = stops[at.stop];
    match key {
        Key::Escape => Act::Back,
        Key::Tab => match stops.iter().position(|s| matches!(s, Stop::Tabs { .. })) {
            Some(t) => {
                let Stop::Tabs { n, at: open } = stops[t] else { unreachable!() };
                let d = if shift { n - 1 } else { 1 };
                if n > 1 { Act::Set(t, (open + d) % n) } else { Act::None }
            }
            None => Act::None,
        },
        Key::ArrowUp | Key::ArrowDown => {
            let down = key == Key::ArrowDown;
            if let Stop::Buttons { pair: Some(k), .. } = here
                && at.sub == if down { k } else { k + 1 }
            {
                at.sub = if down { k + 1 } else { k };
                return Act::Moved;
            }
            if let Stop::List { n, .. } = here {
                at.sub = at.sub.min(n.saturating_sub(1));
                // Inside the list; at its end on to the stop beyond (itself again round the ends when it is alone).
                if down && at.sub + 1 < n {
                    at.sub += 1;
                    return Act::Moved;
                }
                if !down && at.sub > 0 {
                    at.sub -= 1;
                    return Act::Moved;
                }
            }
            let next = neighbour(stops, *at, down);
            if next == *at {
                return Act::None;
            }
            *at = next;
            Act::Moved
        }
        Key::ArrowLeft | Key::ArrowRight => {
            let d = if key == Key::ArrowRight { 1 } else { -1 };
            match here {
                Stop::Tabs { n, at: open } if n > 1 => Act::Set(at.stop, (open as i32 + d).rem_euclid(n as i32) as usize),
                Stop::Value { n, at: v, slider, off } => step_value(n, v, d, slider, off).map_or(Act::None, |x| Act::Set(at.stop, x)),
                Stop::Buttons { n, .. } if n > 1 => {
                    at.sub = (at.sub as i32 + d).rem_euclid(n as i32) as usize;
                    Act::Moved
                }
                Stop::List { n, page } if n > 0 => {
                    let x = (at.sub as i32 + d * page.max(1) as i32).clamp(0, n as i32 - 1) as usize;
                    if x == at.sub {
                        return Act::None;
                    }
                    at.sub = x;
                    Act::Moved
                }
                _ => Act::None,
            }
        }
        Key::PageUp | Key::PageDown => match here {
            Stop::List { .. } => self::key(stops, at, if key == Key::PageDown { Key::ArrowRight } else { Key::ArrowLeft }, shift),
            _ => Act::None,
        },
        Key::Home | Key::End => match here {
            Stop::List { n, .. } if n > 0 => {
                let x = if key == Key::End { n - 1 } else { 0 };
                if x == at.sub {
                    return Act::None;
                }
                at.sub = x;
                Act::Moved
            }
            _ => Act::None,
        },
        Key::Enter => match here {
            Stop::Value { n, at: v, slider: false, off } => step_value(n, v, 1, false, off).map_or(Act::None, |x| Act::Set(at.stop, x)),
            Stop::Buttons { off, .. } if bit(off, at.sub) => Act::Refused,
            Stop::Buttons { n, .. } if at.sub < n => Act::Press(at.stop, at.sub),
            Stop::List { n, .. } if at.sub < n => Act::Press(at.stop, at.sub),
            _ => Act::None,
        },
        _ => Act::None,
    }
}

/// A button's state: `lit` it holds (the option chosen, the open thing), `cursor` the keyboard cursor is on it, `off`.
#[derive(Clone, Copy, Default)]
pub struct Btn {
    pub lit: bool,
    pub cursor: bool,
    pub off: bool,
}

/// The keyboard cursor's ring round `r`.
pub fn ring(p: &egui::Painter, m: &Modern, r: Rect, radius: f32) {
    p.rect_stroke(r, radius, Stroke::new(2.0, m.theme.color("focus")), StrokeKind::Outside);
}

/// A button in the pack's frame, `width` wide (0: as its label); its response (a click on one that is off does
/// nothing: check `clicked(&resp, s)`).
pub fn button(ui: &mut Ui, m: &Modern, label: &str, width: f32, s: Btn) -> Response {
    let font = FontId::proportional(m.theme.small);
    let w = if width > 0.0 { width } else { (ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x) + 24.0).max(64.0) };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    paint_button(ui.painter(), m, r, label, &font, s, resp.hovered());
    resp
}

/// A small button for dense panels (the edge windows): as `button`, 20 tall and as wide as its label. `colour` the
/// label's when lit (a toggle's own colour), else the theme's.
pub fn chip(ui: &mut Ui, m: &Modern, label: &str, s: Btn, colour: Option<Color32>) -> Response {
    let font = FontId::proportional(m.theme.small);
    let w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x) + 14.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    paint_button(ui.painter(), m, r, "", &font, s, resp.hovered());
    let c = match () {
        _ if s.off => m.theme.color("muted"),
        _ if s.lit => colour.unwrap_or(m.theme.color("gold")),
        _ => m.theme.color("text"),
    };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, c);
    resp
}

/// A button's look over `r` (for buttons placed by hand).
pub fn paint_button(p: &egui::Painter, m: &Modern, r: Rect, label: &str, font: &FontId, s: Btn, hovered: bool) {
    let face = match () {
        _ if s.off => "controls/button_disabled",
        _ if s.lit => "controls/button_selected",
        _ if hovered => "controls/button_hover",
        _ => "controls/button_normal",
    };
    if let Some(sp) = m.art.sprite(face) {
        p.add(paint::nine_slice(&sp, r, 0.5, Color32::WHITE));
    }
    if s.cursor {
        ring(p, m, r.expand(1.0), 6.0);
    }
    let c = if s.off { m.theme.color("muted") } else if s.lit { m.theme.color("gold") } else { m.theme.color("text") };
    p.text(r.center(), Align2::CENTER_CENTER, label, font.clone(), c);
}

/// Whether a button was clicked and may be.
pub fn clicked(resp: &Response, s: Btn) -> bool {
    resp.clicked() && !s.off
}

/// A setting: its heading over what `add` draws, the heading gold and the block ringed while the cursor is on it
/// (`ring_it` false: a row of buttons rings its cursor's button instead), scrolled into view when the cursor has just
/// come to it. What `add` gave and the block's rect.
pub fn setting<R>(ui: &mut Ui, m: &Modern, heading: &str, focus: bool, ring_it: bool, follow: bool, add: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
    setting_noted(ui, m, heading, "", focus, ring_it, follow, add)
}

/// `setting` with `note` at the heading's right (wrapping in the room beside it).
fn setting_noted<R>(ui: &mut Ui, m: &Modern, heading: &str, note: &str, focus: bool, ring_it: bool, follow: bool, add: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
    let top = ui.cursor().min;
    if !note.is_empty() {
        let c = if focus { m.theme.color("gold") } else { m.theme.color("muted") };
        let w = ui.available_width();
        let head = ui.painter().layout_no_wrap(heading.to_string(), FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), c);
        let note = ui.painter().layout(note.to_string(), FontId::proportional(m.theme.small), m.theme.color("text"), (w - head.size().x - 16.0).max(60.0));
        let (r, _) = ui.allocate_exact_size(vec2(w, head.size().y.max(note.size().y)), Sense::hover());
        ui.painter().galley(r.min, head, Color32::WHITE);
        ui.painter().galley(pos2(r.max.x - note.size().x, r.min.y), note, Color32::WHITE);
        ui.add_space(2.0);
    } else if !heading.is_empty() {
        let c = if focus { m.theme.color("gold") } else { m.theme.color("muted") };
        ui.label(egui::RichText::new(heading).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(c));
        ui.add_space(2.0);
    }
    let out = add(ui);
    let r = Rect::from_min_max(top - vec2(6.0, 3.0), pos2(top.x + ui.available_width() + 6.0, ui.cursor().min.y + 2.0));
    if focus && ring_it {
        ui.painter().rect_stroke(r, 6.0, Stroke::new(1.5, m.theme.color("focus").gamma_multiply(0.85)), StrokeKind::Outside);
    }
    if focus && follow {
        ui.scroll_to_rect(r, None);
    }
    ui.add_space(8.0);
    (out, r)
}

/// A one-line setting: what `right` draws at the right, `label` in the room left of it (wrapping there, never pushing
/// the controls out), ringed while the cursor is on it and scrolled into view when it has just come there. What
/// `right` gave and the line's rect.
pub fn line<R>(ui: &mut Ui, m: &Modern, label: &str, focus: bool, follow: bool, right: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
    let inner = ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        let c = if focus { m.theme.color("gold") } else { m.theme.color("text") };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let out = right(ui);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Label::new(egui::RichText::new(label).size(m.theme.small).color(c)).wrap()));
            out
        })
        .inner
    });
    let r = inner.response.rect.expand2(vec2(6.0, 2.0));
    if focus {
        ui.painter().rect_stroke(r, 6.0, Stroke::new(1.5, m.theme.color("focus").gamma_multiply(0.85)), StrokeKind::Outside);
        if follow {
            ui.scroll_to_rect(r, None);
        }
    }
    ui.add_space(10.0);
    (inner.inner, r)
}

/// A value's options as buttons, wrapping, the one held lit (`off` a bit each); the option clicked.
pub fn choice(ui: &mut Ui, m: &Modern, labels: &[String], at: usize, off: u64) -> Option<usize> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (k, label) in labels.iter().enumerate() {
            let s = Btn { lit: k == at, off: bit(off, k), cursor: false };
            if clicked(&button(ui, m, label, 0.0, s), s) {
                out = Some(k);
            }
        }
    });
    out
}

/// A row of buttons, the cursor's ringed while `cursor` is some; the button clicked.
pub fn buttons(ui: &mut Ui, m: &Modern, labels: &[&str], cursor: Option<usize>, lit: u64, off: u64) -> Option<usize> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (k, label) in labels.iter().enumerate() {
            let s = Btn { lit: bit(lit, k), off: bit(off, k), cursor: cursor == Some(k) };
            if clicked(&button(ui, m, label, 0.0, s), s) {
                out = Some(k);
            }
        }
    });
    out
}

/// A slider setting (see `setting`) of `n` stops at `at`: its value shown at the track's right or, too long for that
/// room, at the heading's right (wrapped there, never cut); the stop the pointer took (a click or a drag).
pub fn slider_setting(ui: &mut Ui, m: &Modern, heading: &str, focus: bool, follow: bool, n: usize, at: usize, shown: &str) -> (Option<usize>, Rect) {
    let w = ui.fonts_mut(|f| f.layout_no_wrap(shown.to_string(), FontId::proportional(m.theme.small), Color32::WHITE).size().x);
    if w + 12.0 <= SLIDER_ROOM {
        setting(ui, m, heading, focus, true, follow, |ui| slider(ui, m, n, at, shown))
    } else {
        setting_noted(ui, m, heading, shown, focus, true, follow, |ui| slider(ui, m, n, at, ""))
    }
}

const SLIDER_ROOM: f32 = 110.0;

fn slider(ui: &mut Ui, m: &Modern, n: usize, at: usize, shown: &str) -> Option<usize> {
    let avail = ui.available_width();
    let value = ui.painter().layout_no_wrap(shown.to_string(), FontId::proportional(m.theme.small), m.theme.color("text"));
    let (r, resp) = ui.allocate_exact_size(vec2(avail - SLIDER_ROOM, 20.0), Sense::click_and_drag());
    let f = if n > 1 { at as f32 / (n - 1) as f32 } else { 1.0 };
    let track = r.shrink2(vec2(0.0, 6.0));
    let p = ui.painter();
    p.rect_filled(track, 4.0, m.theme.color("charcoal"));
    p.rect_filled(Rect::from_min_max(track.min, pos2(r.min.x + r.width() * f, track.max.y)), 4.0, m.theme.color("bronze"));
    let knob = if resp.hovered() || resp.dragged() { m.theme.color("text") } else { m.theme.color("gold") };
    p.circle(pos2(r.min.x + r.width() * f, r.center().y), 7.5, knob, Stroke::new(1.0, Color32::from_black_alpha(200)));
    p.galley(pos2(r.max.x + 12.0, r.center().y - value.size().y / 2.0), value, Color32::WHITE);
    if (resp.clicked() || resp.dragged())
        && let Some(x) = resp.interact_pointer_pos()
    {
        let t = ((x.x - r.min.x) / r.width()).clamp(0.0, 1.0);
        let i = (t * n.saturating_sub(1) as f32).round() as usize;
        return (i != at).then_some(i);
    }
    None
}

/// The tab strip (tabs.rs's look), `cursor` on the open tab while the strip has the keys; the tab clicked.
pub fn tab_strip(ui: &mut Ui, m: &Modern, salt: &str, labels: &[&str], open: usize, cursor: bool) -> Option<usize> {
    let k = tabs::strip(ui, m, salt, labels, Some(open), cursor.then_some(open)).iter().position(|r| r.clicked());
    ui.add_space(8.0);
    k
}

/// A list's mouse: the row clicked (the cursor goes there and it is taken) and the row right-clicked.
#[derive(Default, Clone, Copy)]
pub struct ListOut {
    pub clicked: Option<usize>,
    pub right: Option<usize>,
}

/// A list row's state for its painter.
#[derive(Clone, Copy)]
pub struct Row {
    pub index: usize,
    pub rect: Rect,
    pub cursor: bool,
    pub off: bool,
}

/// The scroll area every list and page scrolls in: at most `max` tall, its bar shown whenever it has more than its room.
pub fn scroll<R>(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, max: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical().id_salt(id).max_height(max).auto_shrink([false, true]).show(ui, add).inner
}

/// A list of `n` rows `h` tall in a scroll area at most `max` tall: each row's face (the cursor's chosen while `focus`,
/// lit when not; under the pointer only lit), then `paint` draws its contents; the cursor's row kept in view when
/// `follow`. Rows `off(k)` are greyed.
#[allow(clippy::too_many_arguments)]
pub fn list(ui: &mut Ui, m: &Modern, id: impl std::hash::Hash + std::fmt::Debug, n: usize, cursor: Option<usize>, focus: bool, follow: bool, h: f32, max: f32, off: impl Fn(usize) -> bool, mut paint: impl FnMut(&mut Ui, Row)) -> ListOut {
    let mut out = ListOut::default();
    scroll(ui, id, max, |ui| {
        for k in 0..n {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
            let on = cursor == Some(k);
            let dim = off(k);
            let face = match () {
                _ if on && focus => "controls/list_row_selected",
                _ if on || resp.hovered() => "controls/list_row_hover",
                _ if dim => "controls/list_row_disabled",
                _ => "controls/list_row_normal",
            };
            rows::face(ui.painter(), m, r, face, Color32::WHITE);
            paint(ui, Row { index: k, rect: r, cursor: on && focus, off: dim });
            if on && follow {
                ui.scroll_to_rect(r, None);
            }
            if resp.clicked() {
                out.clicked = Some(k);
            }
            if resp.secondary_clicked() {
                out.right = Some(k);
            }
            ui.add_space(2.0);
        }
    });
    out
}

/// A row's label colour (gold on the cursor in the row styles that do it, grey when off).
pub fn ink(m: &Modern, row: Row) -> Color32 {
    rows::label_colour(m, row.cursor, row.off)
}

/// A bar for a list paged by hand (`top` of `n` rows, `shown` in view) in `track`, as the scroll areas' bars.
pub fn bar(p: &egui::Painter, m: &Modern, track: Rect, n: usize, shown: usize, top: usize) {
    if n <= shown || track.height() <= 0.0 {
        return;
    }
    p.rect_filled(track, 4.0, m.theme.color("charcoal"));
    let h = (track.height() * shown as f32 / n as f32).max(24.0).min(track.height());
    let y = track.min.y + (track.height() - h) * top as f32 / (n - shown) as f32;
    p.rect_filled(Rect::from_min_size(pos2(track.min.x, y), vec2(track.width(), h)), 4.0, m.theme.color("bronze"));
}

/// A text field `width` wide (the pack's input frame, an icon at its left) with `text` and, while `caret` is some,
/// the blinking caret before that char; a line longer than the field is scrolled to keep the caret in view. Its rect.
pub fn field(ui: &mut Ui, m: &Modern, text: &str, caret: Option<usize>, width: f32, font: FontId, icon: Option<&str>) -> Rect {
    let (r, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    let sprite = if caret.is_some() { "controls/input_focused" } else { "controls/input_normal" };
    if let Some(s) = m.art.sprite(sprite).or_else(|| m.art.sprite("controls/input_focused")) {
        ui.painter().add(paint::nine_slice(&s, r, (r.height() / s.size.y).min(1.0), Color32::WHITE));
    }
    let mut x = r.min.x + 10.0;
    if let Some(s) = icon.and_then(|i| m.art.sprite(i)) {
        ui.painter().add(paint::image(&s, pos2(r.min.x + 8.0, r.center().y - 9.0), 18.0, Color32::WHITE));
        x = r.min.x + 32.0;
    }
    caret_text(ui, m, pos2(x, r.center().y), text, caret, font, r.max.x - 8.0);
    r
}

/// Text from `at` (left, centred on y) with the blinking caret before char `caret`, scrolled left to keep the caret
/// before `right`.
pub fn caret_text(ui: &Ui, m: &Modern, at: egui::Pos2, text: &str, caret: Option<usize>, font: FontId, right: f32) {
    let colour = m.theme.color("text");
    let width = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), colour).size().x;
    let dx = caret.map_or(0.0, |c| width(&text.chars().take(c).collect::<String>()));
    let shift = (dx + 4.0 - (right - at.x)).max(0.0);
    let clip = Rect::from_min_max(pos2(at.x - 2.0, at.y - 20.0), pos2(right, at.y + 20.0));
    let p = ui.painter().with_clip_rect(clip.intersect(ui.clip_rect()));
    p.text(pos2(at.x - shift, at.y), Align2::LEFT_CENTER, text, font.clone(), colour);
    if caret.is_some() && (ui.ctx().input(|i| i.time) * 1.5) as u64 % 2 == 0 {
        let x = at.x - shift + dx;
        let h = font.size * 0.6;
        p.line_segment([pos2(x, at.y - h), pos2(x, at.y + h)], Stroke::new(1.5, colour));
    }
}

/// A yes / no question: `question` over the two buttons, the one the cursor is on (`yes`) lit and ringed; the answer
/// clicked. `size` the text's.
pub fn yes_no(ui: &mut Ui, m: &Modern, question: &str, labels: [&str; 2], yes: bool, size: f32) -> Option<bool> {
    if !question.is_empty() {
        ui.label(egui::RichText::new(question).size(size).color(m.theme.color("text")));
        ui.add_space(10.0);
    }
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let font = FontId::proportional(size);
        for (val, label) in [(true, labels[0]), (false, labels[1])] {
            let (r, resp) = ui.allocate_exact_size(vec2((size * 7.5).max(120.0), size + 16.0), Sense::click());
            paint_button(ui.painter(), m, r, label, &font, Btn { lit: val == yes, cursor: val == yes, off: false }, resp.hovered());
            if resp.clicked() {
                out = Some(val);
            }
        }
    });
    out
}

/// Up / Down chevrons stacked in `r` (a row's move buttons): which was clicked (true up), each lit under the pointer,
/// ringed for the cursor (`cursor`: Some(true) the up one).
pub fn chevrons(ui: &mut Ui, m: &Modern, salt: impl std::hash::Hash + std::fmt::Debug + Copy, cursor: Option<bool>, up_off: bool, down_off: bool) -> Option<bool> {
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::hover());
    let mut out = None;
    for (up, half, off) in [(true, Rect::from_min_max(r.min, pos2(r.max.x, r.center().y - 0.5)), up_off), (false, Rect::from_min_max(pos2(r.min.x, r.center().y + 0.5), r.max), down_off)] {
        let resp = ui.interact(half, ui.scope_id().with((salt, up)), Sense::click());
        let face = match () {
            _ if off => "controls/button_disabled",
            _ if resp.hovered() => "controls/button_hover",
            _ => "controls/button_normal",
        };
        if let Some(sp) = m.art.sprite(face) {
            ui.painter().add(paint::nine_slice(&sp, half, 0.35, Color32::WHITE));
        }
        if cursor == Some(up) {
            ring(ui.painter(), m, half.expand(0.5), 4.0);
        }
        let c = if off { m.theme.color("muted") } else { m.theme.color("gold") };
        let (cx, cy, d) = (half.center().x, half.center().y, if up { -1.0 } else { 1.0 });
        let st = Stroke::new(1.8, c);
        ui.painter().line_segment([pos2(cx - 5.0, cy - 2.0 * d), pos2(cx, cy + 2.5 * d)], st);
        ui.painter().line_segment([pos2(cx, cy + 2.5 * d), pos2(cx + 5.0, cy - 2.0 * d)], st);
        if resp.clicked() && !off {
            out = Some(up);
        }
    }
    out
}

/// Hue, saturation and value (each 0..1) of an sRGB colour, as colour pickers show them.
pub fn to_hsv([r, g, b]: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = [r, g, b].map(|x| x as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = match () {
        _ if d == 0.0 => 0.0,
        _ if max == r => ((g - b) / d).rem_euclid(6.0) / 6.0,
        _ if max == g => ((b - r) / d + 2.0) / 6.0,
        _ => ((r - g) / d + 4.0) / 6.0,
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

/// The sRGB colour of a hue, saturation and value (each 0..1).
pub fn from_hsv([h, s, v]: [f32; 3]) -> [u8; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|k| ((k + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn hsv_colour(h: f32, s: f32, v: f32) -> Color32 {
    let [r, g, b] = from_hsv([h, s, v]);
    Color32::from_rgb(r, g, b)
}

/// A colour picker: the saturation / value square and the hue strip for the mouse; the colour the pointer set.
pub fn colour_picker(ui: &mut Ui, m: &Modern, rgb: [u8; 3], width: f32) -> Option<[u8; 3]> {
    let [h, s, v] = to_hsv(rgb);
    let side = (width - 40.0).clamp(120.0, 220.0);
    let mut out = None;
    ui.horizontal(|ui| {
        let (sq, resp) = ui.allocate_exact_size(vec2(side, side * 0.6), Sense::click_and_drag());
        // The square: white to the hue across, black down.
        let mut mesh = egui::Mesh::default();
        let steps = 12;
        for j in 0..=steps {
            for i in 0..=steps {
                let (x, y) = (i as f32 / steps as f32, j as f32 / steps as f32);
                mesh.colored_vertex(pos2(sq.min.x + sq.width() * x, sq.min.y + sq.height() * y), hsv_colour(h, x, 1.0 - y));
            }
        }
        let w = steps as u32 + 1;
        for j in 0..steps as u32 {
            for i in 0..steps as u32 {
                let k = j * w + i;
                mesh.add_triangle(k, k + 1, k + w);
                mesh.add_triangle(k + 1, k + w + 1, k + w);
            }
        }
        ui.painter().add(Shape::mesh(mesh));
        ui.painter().rect_stroke(sq, 2.0, Stroke::new(1.0, m.theme.color("bronze")), StrokeKind::Outside);
        let dot = pos2(sq.min.x + sq.width() * s, sq.min.y + sq.height() * (1.0 - v));
        ui.painter().circle_stroke(dot, 5.0, Stroke::new(2.0, Color32::WHITE));
        ui.painter().circle_stroke(dot, 6.5, Stroke::new(1.0, Color32::BLACK));
        if (resp.clicked() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let s = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
            let v = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
            out = Some(from_hsv([h, s, v]));
        }
        ui.add_space(10.0);
        // The hue strip, top to bottom.
        let (strip, resp) = ui.allocate_exact_size(vec2(18.0, sq.height()), Sense::click_and_drag());
        let mut mesh = egui::Mesh::default();
        for j in 0..=24 {
            let t = j as f32 / 24.0;
            let c = hsv_colour(t, 1.0, 1.0);
            let y = strip.min.y + strip.height() * t;
            mesh.colored_vertex(pos2(strip.min.x, y), c);
            mesh.colored_vertex(pos2(strip.max.x, y), c);
        }
        for j in 0..24u32 {
            let k = j * 2;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
        ui.painter().add(Shape::mesh(mesh));
        ui.painter().rect_stroke(strip, 2.0, Stroke::new(1.0, m.theme.color("bronze")), StrokeKind::Outside);
        let y = strip.min.y + strip.height() * h;
        ui.painter().line_segment([pos2(strip.min.x - 3.0, y), pos2(strip.max.x + 3.0, y)], Stroke::new(2.0, Color32::WHITE));
        if (resp.clicked() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let h = ((p.y - strip.min.y) / strip.height()).clamp(0.0, 0.999);
            // A grey has no hue to turn: give it some colour.
            out = Some(from_hsv([h, if s < 0.05 { 1.0 } else { s }, if v < 0.05 { 1.0 } else { v }]));
        }
    });
    out
}

/// Hue, saturation and value as 0..=`STEPS` steps (the picker's keyboard sliders) and back.
pub const STEPS: usize = 50;

pub fn hsv_steps(rgb: [u8; 3]) -> [usize; 3] {
    to_hsv(rgb).map(|x| (x.clamp(0.0, 1.0) * STEPS as f32).round() as usize)
}

pub fn from_steps(hsv: [usize; 3]) -> [u8; 3] {
    let [h, s, v] = hsv.map(|x| x.min(STEPS) as f32 / STEPS as f32);
    from_hsv([h.min(0.999), s, v])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_stops_and_lists() {
        let stops = [Stop::Buttons { n: 3, off: 0, pair: None }, Stop::List { n: 4, page: 2 }];
        let mut at = At::new(1, 0);
        assert_eq!(key(&stops, &mut at, Key::ArrowUp, false), Act::Moved);
        assert_eq!(at, At::new(0, 0));
        // Up from the first stop wraps to the list's last row.
        key(&stops, &mut at, Key::ArrowUp, false);
        assert_eq!(at, At::new(1, 3));
        // Down off the list's end goes round to the buttons.
        key(&stops, &mut at, Key::ArrowDown, false);
        assert_eq!(at, At::new(0, 0));
        assert_eq!(key(&stops, &mut at, Key::ArrowLeft, false), Act::Moved);
        assert_eq!(at.sub, 2);
        assert_eq!(key(&stops, &mut at, Key::Enter, false), Act::Press(0, 2));
    }

    #[test]
    fn values_change_at_once() {
        let stops = [Stop::Tabs { n: 3, at: 0 }, Stop::Value { n: 2, at: 0, slider: false, off: 0 }, Stop::Value { n: 5, at: 4, slider: true, off: 0 }];
        let mut at = At::new(1, 0);
        assert_eq!(key(&stops, &mut at, Key::ArrowRight, false), Act::Set(1, 1));
        assert_eq!(key(&stops, &mut at, Key::Enter, false), Act::Set(1, 1));
        at.stop = 2;
        assert_eq!(key(&stops, &mut at, Key::ArrowRight, false), Act::None);
        assert_eq!(key(&stops, &mut at, Key::ArrowLeft, false), Act::Set(2, 3));
        assert_eq!(key(&stops, &mut at, Key::Enter, false), Act::None);
        assert_eq!(key(&stops, &mut at, Key::Tab, false), Act::Set(0, 1));
        assert_eq!(key(&stops, &mut at, Key::Tab, true), Act::Set(0, 2));
        // Options that are off are passed over.
        let stops = [Stop::Value { n: 3, at: 0, slider: false, off: 0b010 }];
        let mut at = At::new(0, 0);
        assert_eq!(key(&stops, &mut at, Key::ArrowRight, false), Act::Set(0, 2));
    }

    #[test]
    fn hsv_round_trips() {
        for c in [[255, 0, 0], [12, 200, 90], [128, 128, 128], [0, 0, 0], [255, 255, 255], [30, 60, 250]] {
            assert_eq!(from_hsv(to_hsv(c)), c);
        }
    }

    #[test]
    fn stacked_buttons_take_up_and_down() {
        let stops = [Stop::List { n: 2, page: 2 }, Stop::Buttons { n: 4, off: 0, pair: Some(1) }, Stop::List { n: 2, page: 2 }];
        let mut at = At::new(1, 1);
        key(&stops, &mut at, Key::ArrowDown, false);
        assert_eq!(at, At::new(1, 2));
        key(&stops, &mut at, Key::ArrowUp, false);
        assert_eq!(at, At::new(1, 1));
        // From the lower one Down leaves the row; from the upper one Up does.
        at = At::new(1, 2);
        key(&stops, &mut at, Key::ArrowDown, false);
        assert_eq!(at, At::new(2, 0));
        at = At::new(1, 1);
        key(&stops, &mut at, Key::ArrowUp, false);
        assert_eq!(at, At::new(0, 1));
    }

    #[test]
    fn a_list_alone_wraps() {
        let stops = [Stop::List { n: 3, page: 10 }];
        let mut at = At::new(0, 2);
        key(&stops, &mut at, Key::ArrowDown, false);
        assert_eq!(at, At::new(0, 0));
        key(&stops, &mut at, Key::ArrowUp, false);
        assert_eq!(at, At::new(0, 2));
    }
}
