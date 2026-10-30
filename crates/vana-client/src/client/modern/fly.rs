//! Flyouts (ours, user 2026-10-30): a panel held by one side to the screen's left or right edge, or to another panel's
//! side, the two frames joined there (borders::fly, paint::joint). A drag slides one along its hold; pulled well off it,
//! its ghost follows the pointer and it lands where it is let go: on an edge or a panel's side near there, else standing
//! free. A free panel dragged near an edge or a side shows its ghost where it would be held, and is held there when let
//! go. The edge windows (extras.rs) start on an edge; any other panel (Panels::show) can be put on one. Held ones fold
//! into a tab.

use super::{BOTTOM, EDGE, Fly, Modern, Panels, STRONG, borders};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// How a panel is held.
#[derive(Clone, Debug, PartialEq)]
pub enum Attach {
    Free,
    /// The screen's left or right edge, its top at `y` of the screen's height.
    Edge { right: bool, y: f32 },
    /// Another panel's side, its top `along` points below that panel's.
    Side { to: String, right: bool, along: f32 },
}

/// What a held frame is open to: the screen's edge, or the panel it joins (its rect, and which side of it).
#[derive(Clone, Copy, Debug)]
pub enum Open {
    None,
    Edge { right: bool },
    Side { t: Rect, right: bool, band: [Color32; 3] },
}

impl Open {
    /// Held by its left side (on the screen's left edge, or on a panel's right), else by its right; None standing free.
    pub fn by_left(self) -> Option<bool> {
        match self {
            Open::Edge { right } => Some(!right),
            Open::Side { right, .. } => Some(right),
            Open::None => None,
        }
    }
}

/// Pulled this far off its hold a flyout's ghost shows; let go this near a panel's side (or a screen edge: the chat and
/// party windows stand a little in from theirs), a panel is held there.
const PULL: f32 = 64.0;
const SNAP: f32 = 28.0;
const EDGE_SNAP: f32 = 10.0;
/// The room a joint keeps from the other panel's corners: its corner ornament and a fillet.
const JOIN_MARGIN: f32 = 24.0 + super::paint::FILLET;
/// A folded flyout's tab.
pub const TAB_W: f32 = 40.0;
pub const TAB_H: f32 = 56.0;

/// Whether a flyout `h` tall fits on panel `t`'s side.
fn fits(t: Rect, h: f32) -> bool {
    h + 2.0 * JOIN_MARGIN <= t.height()
}

/// A flyout's top on panel `t`'s side, `along` below t's top, clear of t's corners.
fn side_top(t: Rect, along: f32, h: f32) -> f32 {
    (t.min.y + along).clamp(t.min.y + JOIN_MARGIN, (t.max.y - JOIN_MARGIN - h).max(t.min.y + JOIN_MARGIN))
}

/// The chat and party windows grow from their bottom: their hold keeps it (its y / along measure the bottom).
fn by_bottom(id: &str) -> bool {
    BOTTOM.contains(&id)
}

/// The y a hold measures for `id` standing at `r`.
fn anchor(id: &str, r: Rect) -> f32 {
    if by_bottom(id) { r.max.y } else { r.min.y }
}

/// The folded state's key for a panel that is not an edge window (those keep their own).
pub fn fold_key(id: &str) -> String {
    format!("fly {id}")
}

/// A panel's name on its tab when it has no title: its id, capitalised.
pub fn name(id: &str) -> String {
    let mut c = id.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl Modern {
    /// Where the pointer holds the window being dragged (kept from the press).
    fn grab(&self, id: &str, off: Vec2) -> Vec2 {
        let mut g = self.fly_grab.borrow_mut();
        match g.as_ref() {
            Some((k, o)) if k == id => *o,
            _ => {
                *g = Some((id.to_string(), off));
                off
            }
        }
    }

    fn release(&self, id: &str) {
        let mut g = self.fly_grab.borrow_mut();
        if g.as_ref().is_some_and(|(k, _)| k == id) {
            *g = None;
        }
    }
}

impl Panels<'_> {
    /// How `id` is held: its place's fly, else (an edge window not let go free) its edge.
    pub(super) fn attach(&self, id: &str) -> Attach {
        let p = self.place(id);
        match p.fly {
            Some(f) if f.to.is_empty() => Attach::Edge { right: f.right, y: f.along },
            Some(f) => Attach::Side { to: f.to, right: f.right, along: f.along },
            None if EDGE.contains(&id) && !p.float => Attach::Edge { right: p.x < 0.0, y: p.y },
            None => Attach::Free,
        }
    }

    /// Puts `id` in hold `a`; free, it stands at `r`.
    pub(super) fn set_attach(&mut self, id: &str, a: Attach, r: Rect) {
        let s = self.ctx.content_rect();
        let (sw, sh) = (s.width().max(1.0), s.height().max(1.0));
        let mut p = self.place(id);
        p.dock = None;
        let edge = EDGE.contains(&id);
        match a {
            Attach::Free => {
                (p.fly, p.float) = (None, edge);
                (p.x, p.y, p.b) = (r.min.x / sw, r.min.y / sh, r.max.y / sh);
            }
            Attach::Edge { right, y } if edge => (p.fly, p.float, p.x, p.y) = (None, false, if right { -0.001 } else { 0.0 }, y),
            Attach::Edge { right, y } => p.fly = Some(Fly { to: String::new(), right, along: y }),
            Attach::Side { to, right, along } => p.fly = Some(Fly { to, right, along }),
        }
        if self.layout.get(id) != Some(&p) {
            self.layout.insert(id.to_string(), p);
            *self.changed = true;
        }
    }

    /// A panel's rect drawn this frame, else last frame.
    fn shown_rect(&self, id: &str) -> Option<Rect> {
        self.this.get(id).copied().or_else(|| self.m.rects.borrow().get(id).copied()).filter(|r| r.width() > 1.0 && r.height() > 1.0)
    }

    /// Where a flyout `size` big stands for hold `a`, and what its frame is open to (a panel's side only where it
    /// fits). None free, or when the panel it rides is not shown.
    pub(super) fn fly_rect(&self, id: &str, a: &Attach, size: Vec2) -> Option<(Rect, Open)> {
        let s = self.ctx.content_rect();
        let lift = if by_bottom(id) { size.y } else { 0.0 };
        match a {
            Attach::Free => None,
            Attach::Edge { right, y } => {
                let top = (s.height() * y - lift).clamp(s.min.y, (s.max.y - size.y).max(s.min.y));
                let x = if *right { s.max.x - size.x } else { s.min.x };
                Some((Rect::from_min_size(pos2(x, top), size), Open::Edge { right: *right }))
            }
            Attach::Side { to, right, along } => {
                let t = self.shown_rect(to)?;
                let top = side_top(t, *along - lift, size.y);
                let x = if *right { t.max.x } else { t.min.x - size.x };
                // Joined only to a framed panel's side, in the band it was drawn in.
                let tf = self.m.frames.borrow().get(to.as_str()).copied().unwrap_or("window_normal");
                let band = self.m.art.frame(tf).filter(|_| tf != "none").map(|f| f.band);
                let open = match band {
                    Some(band) if fits(t, size.y) => Open::Side { t, right: *right, band },
                    _ => Open::None,
                };
                Some((Rect::from_min_size(pos2(x, top), size), open))
            }
        }
    }

    /// A free flyout's rect: its place, else where it last stood.
    pub(super) fn free_rect(&self, id: &str, size: Vec2) -> Rect {
        let s = self.ctx.content_rect();
        let p = self.place(id);
        let at = if p.float || p.fly.is_some() { pos2(s.width() * p.x, s.height() * p.y) } else { self.shown_rect(id).map_or(pos2(s.width() * 0.4, s.height() * 0.3), |r| r.min) };
        Rect::from_min_size(at, size)
    }

    /// The flyouts joined to `id`'s sides when it stands at `t`: their side and rect (last frame's size).
    pub(super) fn joined(&self, id: &str, t: Rect) -> Vec<(bool, Rect)> {
        let rects = self.m.rects.borrow();
        let mut out = Vec::new();
        for (k, p) in self.layout.iter() {
            let Some(f) = p.fly.as_ref().filter(|f| f.to == id) else { continue };
            let Some(r) = self.this.get(k).or(rects.get(k)) else { continue };
            if !fits(t, r.height()) {
                continue;
            }
            let y = side_top(t, f.along - if by_bottom(k) { r.height() } else { 0.0 }, r.height());
            let x = if f.right { t.max.x } else { t.min.x - r.width() };
            out.push((f.right, Rect::from_min_size(pos2(x, y), r.size())));
        }
        out
    }

    /// Whether `a` rides on `b`'s side, directly or through others.
    fn rides(&self, a: &str, b: &str) -> bool {
        let mut cur = a.to_string();
        for _ in 0..8 {
            match self.attach(&cur) {
                Attach::Side { to, .. } if to == b => return true,
                Attach::Side { to, .. } => cur = to,
                _ => return false,
            }
        }
        false
    }

    /// Where `id` let go at `r` would be held: the nearest screen edge or panel side within SNAP (a side only where it
    /// fits), and its rect there.
    pub(super) fn snap(&self, id: &str, r: Rect) -> Option<(Attach, Rect)> {
        let s = self.ctx.content_rect();
        let mut best: Option<(f32, Attach, Rect)> = None;
        let mut take = |d: f32, within: f32, a: Attach, at: Rect| {
            if d < within && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, a, at));
            }
        };
        let top = r.min.y.clamp(s.min.y, (s.max.y - r.height()).max(s.min.y));
        let y = (top + if by_bottom(id) { r.height() } else { 0.0 }) / s.height().max(1.0);
        take((r.min.x - s.min.x).abs(), EDGE_SNAP, Attach::Edge { right: false, y }, Rect::from_min_size(pos2(s.min.x, top), r.size()));
        take((s.max.x - r.max.x).abs(), EDGE_SNAP, Attach::Edge { right: true, y }, Rect::from_min_size(pos2(s.max.x - r.width(), top), r.size()));
        let rects = self.m.rects.borrow();
        for (to, t) in self.this.iter().chain(rects.iter().filter(|(k, _)| !self.this.contains_key(*k))) {
            if to == id || !fits(*t, r.height()) || r.max.y <= t.min.y || r.min.y >= t.max.y || self.rides(to, id) {
                continue;
            }
            let y = side_top(*t, r.min.y - t.min.y, r.height());
            let along = y - t.min.y + if by_bottom(id) { r.height() } else { 0.0 };
            take((r.min.x - t.max.x).abs(), SNAP, Attach::Side { to: to.clone(), right: true, along }, Rect::from_min_size(pos2(t.max.x, y), r.size()));
            take((t.min.x - r.max.x).abs(), SNAP, Attach::Side { to: to.clone(), right: false, along }, Rect::from_min_size(pos2(t.min.x - r.width(), y), r.size()));
        }
        best.map(|b| (b.1, b.2))
    }

    /// A held flyout's drag (`resp`; it stands at `rect`): near its hold it slides along it (no ghost); pulled off,
    /// its ghost follows the pointer, or shows where it would be held; let go off its hold, it lands where the ghost is.
    pub(super) fn held_drag(&mut self, id: &str, a: &Attach, rect: Rect, resp: &egui::Response) {
        if !(resp.dragged() || resp.drag_stopped()) {
            return;
        }
        let (m, ctx) = (self.m, self.ctx);
        let Some(at) = resp.interact_pointer_pos().or_else(|| ctx.input(|i| i.pointer.latest_pos())) else { return };
        // Held where it was pressed (the drag starts a few points later).
        let off = m.grab(id, ctx.input(|i| i.pointer.press_origin()).unwrap_or(at) - rect.min);
        let would = Rect::from_min_size(at - off, rect.size());
        let s = ctx.content_rect();
        let (dist, slid) = match a {
            Attach::Edge { right, .. } => (if *right { s.max.x - would.max.x } else { would.min.x - s.min.x }, Attach::Edge { right: *right, y: anchor(id, would) / s.height().max(1.0) }),
            Attach::Side { to, right, .. } => match self.shown_rect(to) {
                Some(t) => ((if *right { would.min.x - t.max.x } else { t.min.x - would.max.x }).abs(), Attach::Side { to: to.clone(), right: *right, along: anchor(id, would) - t.min.y }),
                None => (f32::INFINITY, a.clone()),
            },
            Attach::Free => (f32::INFINITY, Attach::Free),
        };
        m.arranging.set(true);
        if dist < PULL {
            if resp.dragged() {
                self.set_attach(id, slid, would);
            } else {
                m.release(id);
            }
            return;
        }
        let target = self.snap(id, would);
        if resp.dragged() {
            ghost(m, ctx, id, target.as_ref().map_or(would, |t| t.1));
        } else {
            match target {
                Some((a, _)) => self.set_attach(id, a, would),
                None => self.set_attach(id, Attach::Free, would),
            }
            m.release(id);
        }
    }

    /// A free window dragged to `r`: near an edge or a side its ghost shows where it would be held, and let go there
    /// it is (true).
    pub(super) fn free_drop(&mut self, id: &str, r: Rect, dragging: bool, stopped: bool) -> bool {
        let target = self.snap(id, r);
        if dragging && let Some((_, g)) = &target {
            ghost(self.m, self.ctx, id, *g);
        }
        match target {
            Some((a, _)) if stopped => {
                self.set_attach(id, a, r);
                true
            }
            _ => false,
        }
    }

    /// A free flyout dragged by its own handle (`resp`, standing at `rect`): it follows the pointer, held where it is
    /// let go near an edge or a side.
    pub(super) fn float_drag(&mut self, id: &str, rect: Rect, resp: &egui::Response) {
        if !(resp.dragged() || resp.drag_stopped()) {
            return;
        }
        let (m, ctx) = (self.m, self.ctx);
        let Some(at) = resp.interact_pointer_pos().or_else(|| ctx.input(|i| i.pointer.latest_pos())) else { return };
        // Held where it was pressed (the drag starts a few points later).
        let off = m.grab(id, ctx.input(|i| i.pointer.press_origin()).unwrap_or(at) - rect.min);
        let would = Rect::from_min_size(at - off, rect.size());
        m.arranging.set(true);
        if !self.free_drop(id, would, resp.dragged(), resp.drag_stopped()) {
            self.set_attach(id, Attach::Free, would);
        }
        if resp.drag_stopped() {
            m.release(id);
        }
    }

    /// A drag on a flyout's handle, by its hold.
    pub(super) fn fly_drag(&mut self, id: &str, a: &Attach, rect: Rect, resp: &egui::Response) {
        match a {
            Attach::Free => self.float_drag(id, rect, resp),
            _ => self.held_drag(id, a, rect, resp),
        }
    }

    /// A folded flyout's tab where its hold puts it (free, where it stood): `label` turned along it beside a double
    /// chevron pointing out of its hold; a click opens it (true), a drag moves it as its window would.
    pub(super) fn fly_tab(&mut self, id: &str, label: &str) -> bool {
        let (m, ctx) = (self.m, self.ctx);
        let a = self.attach(id);
        let font = FontId::new(m.theme.small + 1.5, FontFamily::Name(STRONG.into()));
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, Color32::WHITE));
        let gw = galley.size().x;
        let size = vec2(TAB_W, (gw + 28.0).max(TAB_H));
        let (rect, open) = self.fly_rect(id, &a, size).unwrap_or_else(|| (self.free_rect(id, size), Open::None));
        // A retail window over it (they draw under every panel): it steps aside until that closes.
        if under_retail(m, rect) {
            self.this.insert(id.to_string(), rect);
            return false;
        }
        let left = open.by_left().unwrap_or(rect.center().x < ctx.content_rect().center().x);
        let mut out = None;
        egui::Area::new(egui::Id::unique(("modern edge", id))).order(egui::Order::Middle).fixed_pos(rect.min).constrain(false).show(ctx, |ui| {
            let (rr, resp) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
            ui.painter().add(borders::fly(m, "window_normal", rr, open, &[]));
            feedback(ui, m, rr, &resp);
            let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
            // The word turned toward the screen: a quarter counter-clockwise held on the left (read bottom up),
            // clockwise on the right (read top down).
            let (at, angle) = if left { (pos2(rr.min.x + 6.0, rr.center().y + gw / 2.0), -std::f32::consts::FRAC_PI_2) } else { (pos2(rr.max.x - 6.0, rr.center().y - gw / 2.0), std::f32::consts::FRAC_PI_2) };
            ui.painter().add(egui::epaint::TextShape::new(at, galley, c).with_angle(angle));
            let cx = if left { rr.max.x - 11.0 } else { rr.min.x + 11.0 };
            for d in [-3.0, 3.0] {
                chevron(ui.painter(), pos2(cx + d, rr.center().y), !left, c);
            }
            out = Some(resp);
        });
        self.this.insert(id.to_string(), rect);
        let Some(resp) = out else { return false };
        self.fly_drag(id, &a, rect, &resp);
        resp.clicked()
    }
}

/// While a window is pulled off its hold or dragged near one, a see-through copy where it would land.
pub fn ghost(m: &Modern, ctx: &egui::Context, key: &str, r: Rect) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(("fly ghost", key))));
    painter.rect(r, 8, m.theme.color("surface").gamma_multiply(0.45), Stroke::new(2.0, m.theme.color("gold")), egui::StrokeKind::Inside);
}

/// While a tab is dragged: a gold outline and the grabbing hand; hovered, the open hand.
pub fn feedback(ui: &Ui, m: &Modern, r: Rect, resp: &egui::Response) {
    if resp.dragged() {
        ui.painter().rect_stroke(r.expand(1.0), 7, Stroke::new(2.5, m.theme.color("gold")), egui::StrokeKind::Outside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// A retail window drawn this frame overlaps `r`.
pub fn under_retail(m: &Modern, r: Rect) -> bool {
    m.retail.borrow().iter().any(|f| f.intersects(r.shrink(2.0)))
}

/// A chevron at `c` pointing left or right.
pub fn chevron(p: &egui::Painter, c: Pos2, left: bool, col: Color32) {
    let pts = if left { vec![pos2(c.x + 3.5, c.y - 6.0), pos2(c.x + 3.5, c.y + 6.0), pos2(c.x - 3.5, c.y)] } else { vec![pos2(c.x - 3.5, c.y - 6.0), pos2(c.x - 3.5, c.y + 6.0), pos2(c.x + 3.5, c.y)] };
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// A held panel's fold knob: a round button on its frame's inner side by the top, its chevron pointing into the hold.
/// True when clicked.
pub fn fold_knob(ui: &mut Ui, m: &Modern, r: Rect, by_left: bool, id: &str) -> bool {
    let c = pos2(if by_left { r.max.x - 1.0 } else { r.min.x + 1.0 }, r.min.y + 30.0);
    let hit = Rect::from_center_size(c, vec2(20.0, 20.0));
    let resp = ui.interact(hit, egui::Id::unique(("fly fold", id)), Sense::click());
    let lit = resp.hovered();
    ui.painter().circle(c, 8.5, m.theme.color("charcoal"), Stroke::new(1.5, if lit { m.theme.color("text") } else { m.theme.color("gold") }));
    chevron(ui.painter(), c, by_left, if lit { m.theme.color("text") } else { m.theme.color("gold") });
    if lit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}
