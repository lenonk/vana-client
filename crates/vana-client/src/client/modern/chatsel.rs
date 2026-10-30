//! Ours (retail's log and chat line have no mouse selection): the modern chat window's lines and its chat line
//! selected with the mouse. Ctrl+C copies (client/mod.rs's Copy); a right click opens Copy / Copy Line on the lines,
//! Cut / Copy / Paste / Select All on the chat line.

use super::super::{World, fep};
use super::Modern;
use egui::text::CCursor;
use egui::{FontId, Galley, Id, Order, Pos2, Rect, Sense, Ui, pos2, vec2};
use std::sync::Arc;

/// A selection over the lines: from the press to the pointer, each end a line's log index and a char of its text as
/// drawn (the timestamp included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogSel {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl LogSel {
    fn ends(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The chars of line `k` (`n` long) it covers.
    pub fn span(&self, k: usize, n: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ends();
        if self.empty() || k < a.0 || k > b.0 {
            return None;
        }
        let s = if k == a.0 { a.1 } else { 0 };
        let e = if k == b.0 { b.1.min(n) } else { n };
        (s < e).then_some((s, e))
    }

    /// The text it covers of `lines` (log index, text as drawn; in log order), a line to a row.
    pub fn text(&self, lines: &[(usize, String)]) -> String {
        let mut out: Vec<String> = Vec::new();
        for (k, t) in lines {
            if let Some((s, e)) = self.span(*k, t.chars().count()) {
                out.push(t.chars().skip(s).take(e - s).collect());
            }
        }
        out.join("\n")
    }
}

/// The right-click menu: over the lines (the text of the line under the pointer), or on the chat line.
#[derive(Clone, Debug)]
pub enum Menu {
    Log { at: Pos2, line: Option<String> },
    Line { at: Pos2 },
}

/// What a menu row asks, done after the frame (`apply`).
#[derive(Clone, Debug)]
pub enum Act {
    Copy(String),
    Cut,
    CopyLine,
    Paste,
    SelectAll,
}

/// A line drawn this frame: its log index, top left and galley.
pub type Drawn = Vec<(usize, Pos2, Arc<Galley>)>;

/// Where `p` falls among the lines drawn: the line under it (above them the top one's start, below the bottom one's
/// end) and the char there.
pub fn hit(drawn: &Drawn, p: Pos2) -> Option<(usize, usize)> {
    let top = drawn.iter().min_by(|a, b| a.1.y.total_cmp(&b.1.y))?;
    let bottom = drawn.iter().max_by(|a, b| a.1.y.total_cmp(&b.1.y))?;
    if p.y < top.1.y {
        return Some((top.0, 0));
    }
    if p.y > bottom.1.y + bottom.2.size().y {
        return Some((bottom.0, bottom.2.text().chars().count()));
    }
    let (k, at, g) = drawn.iter().min_by(|a, b| dist(a, p).total_cmp(&dist(b, p)))?;
    Some((*k, g.cursor_from_pos(p - *at).index.0))
}

fn dist(line: &(usize, Pos2, Arc<Galley>), p: Pos2) -> f32 {
    let (y0, y1) = (line.1.y, line.1.y + line.2.size().y);
    if p.y < y0 { y0 - p.y } else if p.y > y1 { p.y - y1 } else { 0.0 }
}

/// The text of the line under `p`, if any.
pub fn line_at(drawn: &Drawn, p: Pos2) -> Option<String> {
    drawn.iter().find(|(_, at, g)| p.y >= at.y && p.y <= at.y + g.size().y).map(|l| l.2.text().to_string())
}

/// The selection's wash behind chars `s..e` of a galley drawn at `at`.
pub fn wash(p: &egui::Painter, m: &Modern, g: &Galley, at: Pos2, (s, e): (usize, usize)) {
    let (a, b) = (g.pos_from_cursor(CCursor::new(s)), g.pos_from_cursor(CCursor::new(e)));
    let colour = m.theme.color("focus").gamma_multiply(0.45);
    for row in &g.rows {
        let r = row.rect();
        if r.max.y <= a.center().y - 0.5 * a.height() || r.min.y >= b.center().y + 0.5 * b.height() {
            continue;
        }
        let x0 = if (r.min.y..=r.max.y).contains(&a.center().y) { a.min.x } else { r.min.x };
        let x1 = if (r.min.y..=r.max.y).contains(&b.center().y) { b.min.x } else { r.max.x };
        if x1 > x0 {
            p.rect_filled(Rect::from_min_max(pos2(at.x + x0, at.y + r.min.y), pos2(at.x + x1, at.y + r.max.y)), 2.0, colour);
        }
    }
}

/// The lines' mouse this frame (`resp` their area's): a drag selects from the press; a click clears; a right click
/// opens the menu. Returns whether the press was a plain click on no selection (the lines' own click then counts).
pub fn log_mouse(m: &Modern, resp: &egui::Response, drawn: &Drawn) -> bool {
    let ctx = &resp.ctx;
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(a) = ctx.input(|i| i.pointer.press_origin()).and_then(|p| hit(drawn, p))
    {
        *m.log_sel.borrow_mut() = Some(LogSel { anchor: a, head: a });
    }
    if resp.dragged_by(egui::PointerButton::Primary)
        && let Some(h) = resp.interact_pointer_pos().and_then(|p| hit(drawn, p))
        && let Some(s) = m.log_sel.borrow_mut().as_mut()
    {
        s.head = h;
    }
    if resp.secondary_clicked()
        && let Some(at) = resp.interact_pointer_pos()
    {
        *m.chat_menu.borrow_mut() = Some(Menu::Log { at, line: line_at(drawn, at) });
    }
    if resp.clicked() {
        let had = m.log_sel.borrow().is_some_and(|s| !s.empty());
        *m.log_sel.borrow_mut() = None;
        return !had;
    }
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    false
}

/// The chat line's char boundaries and their x (phrases are four chars, one unit): (char index, x).
pub fn line_stops(ui: &Ui, pieces: &[(String, bool)], font: &FontId, x0: f32) -> Vec<(usize, f32)> {
    let mut out = vec![(0, x0)];
    let (mut x, mut chars) = (x0, 0);
    let width = |s: &str| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE).size().x);
    for (text, phrase) in pieces {
        if *phrase {
            x += width(text);
            chars += 4;
            out.push((chars, x));
            continue;
        }
        let mut s = String::new();
        for c in text.chars() {
            s.push(c);
            out.push((chars + s.chars().count(), x + width(&s)));
        }
        x += width(text);
        chars += text.chars().count();
    }
    out
}

/// The stop nearest `x`.
pub fn nearest(stops: &[(usize, f32)], x: f32) -> usize {
    stops.iter().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |s| s.0)
}

/// The chat line's mouse (`resp` its rect's): a press puts the caret there, a drag selects from the press, a right
/// click opens the menu. What it asks of the line goes in `m.line_req` (caret, anchor) for after the frame.
pub fn line_mouse(m: &Modern, resp: &egui::Response, stops: &[(usize, f32)]) {
    let ctx = &resp.ctx;
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    let press = ctx.input(|i| i.pointer.press_origin()).map(|p| nearest(stops, p.x));
    if (resp.clicked() || resp.drag_started_by(egui::PointerButton::Primary))
        && let Some(c) = press
    {
        m.line_req.set(Some((c, None)));
    }
    if resp.dragged_by(egui::PointerButton::Primary)
        && let (Some(a), Some(p)) = (press, resp.interact_pointer_pos())
    {
        let c = nearest(stops, p.x);
        m.line_req.set(Some((c, (c != a).then_some(a))));
    }
    if resp.secondary_clicked()
        && let Some(at) = resp.interact_pointer_pos()
    {
        *m.chat_menu.borrow_mut() = Some(Menu::Line { at });
    }
}

/// The open right-click menu: its rows (label, usable) in the pack's frame by the pointer; the row picked. A pick or a
/// press elsewhere closes it.
fn menu(ctx: &egui::Context, m: &Modern, at: Pos2, rows: &[(&str, bool)]) -> Option<usize> {
    let mut picked = None;
    let font = FontId::proportional(m.theme.ui);
    let shown = egui::Area::new(Id::unique("modern chat menu")).order(Order::Foreground).fixed_pos(at + vec2(2.0, 2.0)).constrain(true).show(ctx, |ui| {
        let bg = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, (label, usable)) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(130.0, m.theme.ui + 12.0), if *usable { Sense::click() } else { Sense::hover() });
                if *usable && resp.hovered() {
                    ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 4.0, m.theme.color("focus").gamma_multiply(0.35));
                }
                let colour = if !*usable {
                    m.theme.color("muted")
                } else if resp.hovered() {
                    m.theme.color("gold")
                } else {
                    m.theme.color("text")
                };
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y), egui::Align2::LEFT_CENTER, *label, font.clone(), colour);
                if resp.clicked() {
                    picked = Some(k);
                }
            }
        });
        let r = inner.response.rect;
        ui.painter().set(bg, super::borders::window(m, "window_normal", r));
        r
    });
    let away = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.inner.contains(p)));
    if picked.is_some() || away {
        *m.chat_menu.borrow_mut() = None;
    }
    picked
}

/// The open menu drawn; its pick becomes `m.chat_act`.
pub fn draw_menu(ctx: &egui::Context, m: &Modern) {
    let Some(menu) = m.chat_menu.borrow().clone() else { return };
    let act = match menu {
        Menu::Log { at, line } => {
            let sel = m.log_text.borrow().clone().filter(|t| !t.is_empty());
            match self::menu(ctx, m, at, &[("Copy", sel.is_some()), ("Copy Line", line.is_some())]) {
                Some(0) => sel.map(Act::Copy),
                Some(1) => line.map(Act::Copy),
                _ => None,
            }
        }
        Menu::Line { at } => {
            let sel = m.line_sel.get();
            match self::menu(ctx, m, at, &[("Cut", sel), ("Copy", sel), ("Paste", true), ("Select All", true)]) {
                Some(0) => Some(Act::Cut),
                Some(1) => Some(Act::CopyLine),
                Some(2) => Some(Act::Paste),
                Some(3) => Some(Act::SelectAll),
                _ => None,
            }
        }
    };
    if act.is_some() {
        *m.chat_act.borrow_mut() = act;
    }
}

/// After the chat window: the chat line's caret and selection from the mouse, the menu's pick, and the lines'
/// selection for Ctrl+C.
pub fn apply(m: &Modern, w: &mut World) {
    w.log_selection = m.log_text.borrow().clone().filter(|t| !t.is_empty());
    let req = m.line_req.take();
    if w.typing
        && let Some((c, a)) = req
    {
        let n = w.chat_input.chars().count();
        w.chat_caret = (c < n).then_some(c);
        w.chat_anchor = a;
    }
    match m.chat_act.borrow_mut().take() {
        Some(Act::Copy(t)) => w.copied = Some(t),
        Some(Act::Cut) => {
            w.copied = fep::selected_text(w);
            fep::delete_selection(w);
        }
        Some(Act::CopyLine) => w.copied = fep::selected_text(w),
        Some(Act::Paste) => w.paste_request = true,
        Some(Act::SelectAll) => fep::select_all(w),
        None => {}
    }
}
