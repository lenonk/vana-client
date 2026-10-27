//! The modern UI (docs/modern-ui.md; Settings > Linux > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.
//!
//! Each panel is an egui Area that a drag on its background moves (its contents keep their clicks), kept by character
//! as screen fractions. The chat window also resizes by its corner grip: more room for the lines, nothing scaled.

mod chat;
mod combat;
mod command;
mod config;
mod craft;
mod equip;
mod extras;
mod hud;
mod items;
mod journal;
mod lists;
mod macros;
mod paint;
pub mod plates;
mod social;
mod status;
mod storage;
pub mod shot;
mod theme;
mod topbar;

use super::World;
use super::game_ui::GameUi;
use egui::{Color32, Context, FontFamily, FontId, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};
pub use command::{BarFocus, CmdMenu, bar_focus, key as command_key};
pub use config::key as config_key;
pub use journal::key as journal_key;
pub use status::key as status_key;
pub use storage::key as storage_key;
pub use theme::{Theme, frame_image};
use theme::{Art, LABEL, STRONG};

pub use crate::config::{ModernDock as Dock, ModernLayout as Layout, ModernPlace as Place};
use std::collections::HashMap;

pub struct Modern {
    pub theme: Theme,
    art: Art,
    /// The chat window's tab and whether its settings are open; game textures (status icons) as egui textures, by key.
    chat_tab: std::cell::Cell<u8>,
    chat_settings: std::cell::Cell<bool>,
    images: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>>,
    /// The grid text ("(E-8)") and when it was made: rebuilt every half second, as retail's every 30 ticks.
    grid: std::cell::RefCell<(Option<std::time::Instant>, String)>,
    marquee: std::cell::RefCell<topbar::Marquee>,
    /// Last frame's panel rects (what docked panels sit on before their target draws), a panel being moved or resized
    /// (the chat window then shows its full room, something to dock on), and the chat window's rect and fade for the
    /// game's stacked windows.
    rects: std::cell::RefCell<HashMap<String, Rect>>,
    arranging: std::cell::Cell<bool>,
    chat_edge: std::cell::Cell<Option<(Rect, bool)>>,
    zoom: f32,
    /// When the Wide Scan tab last asked for a list (retail asks only as the map opens; the tab keeps it fresh).
    scan_at: std::cell::Cell<Option<std::time::Instant>>,
    /// The last complete Wide Scan list (shown while the next one streams in, then swapped at once) and where it was
    /// asked from; the row picked (its options open, ringed on the minimap); Narrow down range: waiting for a click on
    /// the minimap, then the area kept (world x, z, radius).
    scan_rows: std::cell::RefCell<Vec<(super::widescan::Entry, String)>>,
    scan_origin: std::cell::Cell<(f32, f32)>,
    scan_pick: std::cell::Cell<Option<u16>>,
    scan_picking: std::cell::Cell<bool>,
    scan_area: std::cell::Cell<Option<(f32, f32, f32)>>,
    /// Captions read from the game's name sprites (skills, jobs, ranks), by library and index.
    captions: std::cell::RefCell<HashMap<(String, usize), Option<String>>>,
    /// The full log's wheel, in rows not yet scrolled.
    wheel: std::cell::Cell<f32>,
    /// The Status page's currencies: only those held.
    held_only: std::cell::Cell<bool>,
    /// The Journal's tab and cursor last drawn (a moved cursor is scrolled into view), and the quest list's line of
    /// each quest (headings between them).
    journal_seen: std::cell::Cell<(u8, usize)>,
    /// The Config page and row last drawn (a moved row is scrolled into view).
    config_seen: std::cell::Cell<(usize, usize)>,
    journal_rows: std::cell::RefCell<Vec<usize>>,
    /// egui's text styles enlarged (put back when the layer goes off).
    styled: bool,
    /// An edge window's heading dragged this frame: the move and, while held, where the pointer is (the side follows).
    edge_grip: std::cell::Cell<Option<(f32, Option<Pos2>)>>,
    /// The edge window being dragged and where the pointer holds it (its ghost follows the pointer).
    edge_ghost: std::cell::RefCell<Option<(String, Vec2)>>,
    /// The retail windows drawn this frame: the edge windows give way to them (they draw under every panel).
    retail: std::cell::RefCell<Vec<Rect>>,
    /// A place changed during a drag: saved once the pointer is let go.
    dirty: bool,
}

impl Modern {
    pub fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        Self {
            theme: Theme::load(),
            art: Art::load(ctx),
            chat_tab: Default::default(),
            chat_settings: Default::default(),
            images: Default::default(),
            grid: Default::default(),
            marquee: Default::default(),
            scan_at: Default::default(),
            scan_rows: Default::default(),
            scan_origin: Default::default(),
            scan_pick: Default::default(),
            scan_picking: Default::default(),
            scan_area: Default::default(),
            rects: Default::default(),
            arranging: Default::default(),
            chat_edge: Default::default(),
            zoom: 0.0,
            captions: Default::default(),
            held_only: Default::default(),
            wheel: Default::default(),
            journal_seen: Default::default(),
            config_seen: Default::default(),
            journal_rows: Default::default(),
            styled: false,
            retail: Default::default(),
            edge_grip: Default::default(),
            edge_ghost: Default::default(),
            dirty: false,
        }
    }

    /// One frame: egui's zoom for the window height, the panels. True when the config (the layout, the chat font) is
    /// to be saved.
    pub fn ui(&mut self, ctx: &Context, gu: &GameUi, w: &mut World, window: [u32; 2], cfg: &mut crate::config::Config) -> bool {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = self.theme.zoom(window[1] as f32 / native, cfg.modern_scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
        // egui's own text (tooltips, the few labels without a size) a little larger than its defaults.
        if !self.styled {
            ctx.all_styles_mut(|s| {
                use egui::TextStyle as T;
                for (t, size) in [(T::Body, 14.5), (T::Button, 14.5), (T::Small, 12.5), (T::Monospace, 14.0)] {
                    if let Some(f) = s.text_styles.get_mut(&t) {
                        f.size = size;
                    }
                }
            });
            self.styled = true;
        }
        // The game's keys are not egui's: Tab (targeting) must not walk egui's focus onto a widget for Enter or Space
        // (talking, a door) to press it (a chat tab switched by itself). Nothing here keeps the keyboard focus.
        ctx.memory_mut(|mem| {
            if let Some(id) = mem.focused() {
                mem.surrender_focus(id);
            }
        });
        *self.retail.borrow_mut() = w.retail_frames.iter().map(|r| Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]))).collect();
        topbar::draw(self, ctx, &w.hud_top.view);
        pointers(self, ctx, w);
        let layout = cfg.modern_places.entry(w.name.clone()).or_default();
        let mut changed = false;
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout, changed: &mut changed, chat_font: &mut cfg.modern_chat_font, folded: &mut cfg.modern_folded, scan_kinds: &mut cfg.modern_scan_kinds, this: HashMap::new(), retail: HashMap::new() };
        // The chat window first: what sits on it places by this frame's height.
        self.chat_edge.set(None);
        chat::panel(&mut p, w);
        extras::draw(&mut p, w);
        hud::draw(&mut p, w);
        command::draw(&mut p, w);
        combat::draw(&mut p, w);
        lists::draw(&mut p, w);
        items::draw(&mut p, w);
        equip::draw(&mut p, w);
        config::draw(&mut p, w);
        status::draw(&mut p, w);
        journal::draw(&mut p, w);
        social::draw(&mut p, w);
        craft::draw(&mut p, w);
        macros::draw(&mut p, w);
        storage::draw(&mut p, w);
        // The game's windows that stack on the log (a dialogue's choices ...) go on top of what rides on the chat
        // window (the location panel, the combat window), as retail's stack over its compass and action menu.
        if let Some((r, faded)) = self.chat_edge.get() {
            let riders = p.this.iter().filter(|(id, _)| id.as_str() != "chat" && p.rests_on(id, "chat")).map(|(_, r)| r.min.y);
            let top = riders.chain((!faded).then_some(r.min.y)).reduce(f32::min);
            if let Some(t) = top {
                self.chat_edge.set(Some((Rect::from_min_max(pos2(r.min.x, t), r.max), false)));
            }
        }
        confirm(&mut p, w);
        let this = std::mem::take(&mut p.this);
        *self.rects.borrow_mut() = this;
        self.dirty |= changed;
        let save = self.dirty && !ctx.input(|i| i.pointer.any_down());
        if save {
            self.dirty = false;
        }
        save
    }

    /// The chat window's rect this frame and whether it is fading (then only its bottom edge counts).
    pub fn chat_rect(&self) -> Option<(Rect, bool)> {
        self.chat_edge.get()
    }
}

/// The target cursors in the pack's arrows, under the panels: gold over the target (dimmed on a prompt's saved
/// target), the prompt's pick blue in range, gold at its edge, red past it (retail's range tint); turned to point
/// down at the point, bobbing.
fn pointers(m: &Modern, ctx: &Context, w: &World) {
    let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::unique("modern pointers")));
    let s = ctx.content_rect();
    let bob = (ctx.input(|i| i.time) as f32 * 5.0).sin() * 3.0;
    for &(fx, fy, kind, tint) in &w.modern_pointers {
        let (name, colour) = if kind == 1 { ("indicators/target_arrow", Color32::from_gray(170)) } else { pointer_look(tint) };
        let Some(sp) = m.art.sprite(name) else { continue };
        let tip = pos2(s.min.x + fx * s.width(), s.min.y + fy * s.height() + bob);
        let size = 40.0;
        let r = Rect::from_min_max(pos2(tip.x - size / 2.0, tip.y - size), pos2(tip.x + size / 2.0, tip.y));
        painter.add(paint::turned(&sp, r, 2, colour));
    }
}

/// The pack's arrow for a retail cursor tint: the plain one gold, blue (in range) the subtarget arrow, yellow (its
/// edge) gold, red (out of range) gold turned red.
fn pointer_look(tint: u32) -> (&'static str, Color32) {
    match tint & 0x00ff_ffff {
        0x40_40d0 => ("indicators/subtarget", Color32::WHITE),
        0xd0_4040 => ("indicators/target_arrow", Color32::from_rgb(255, 110, 90)),
        _ => ("indicators/target_arrow", Color32::WHITE),
    }
}

/// With the layer off egui is back at its own size (the F12 lines).
pub fn off(ctx: &Context, m: Option<&mut Modern>) {
    let Some(m) = m else { return };
    if m.zoom != 0.0 {
        ctx.set_zoom_factor(1.0);
        m.zoom = 0.0;
    }
    if m.styled {
        let defaults = egui::Style::default().text_styles;
        ctx.all_styles_mut(|s| s.text_styles = defaults.clone());
        m.styled = false;
    }
}

/// A retail list menu as the modern pages draw it (client/partymenu.rs and the others fill it): its layout, cursor,
/// disabled rows, windowps label sprites swapped in, rows marked, values at a row's right.
pub struct MenuWin {
    pub name: &'static str,
    pub sel: u16,
    pub off: Vec<u16>,
    pub labels: Vec<(u16, usize)>,
    pub marked: Vec<u16>,
    pub values: Vec<(u16, String)>,
}

/// A panel's title row: an optional icon, the title, an optional note at the right.
struct Title<'a> {
    icon: Option<&'a str>,
    text: &'a str,
    note: Option<&'a str>,
}

/// What drawing needs besides the panel itself: the art, egui, the game's data.
#[derive(Clone, Copy)]
struct Look<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    gu: Option<&'a GameUi>,
}

/// What a frame's panels share.
struct Panels<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    /// The game's data (status icons and texts); none in the offscreen shot.
    gu: Option<&'a GameUi>,
    layout: &'a mut Layout,
    changed: &'a mut bool,
    chat_font: &'a mut f32,
    folded: &'a mut std::collections::BTreeMap<String, bool>,
    scan_kinds: &'a mut u8,
    /// The panels drawn this frame, as others dock on them (the chat window while it fades: its bottom edge).
    this: HashMap<String, Rect>,
    /// Places retail works out this frame (the minimap left of the windows at the edge): the default until moved.
    retail: HashMap<String, Place>,
}

/// The panel margins inside the frame; the resizable one keeps a taller strip below for its glyph.
const MARGIN: egui::Margin = egui::Margin { left: 22, right: 22, top: 16, bottom: 18 };
const ROOM_MARGIN: egui::Margin = egui::Margin { bottom: 26, ..MARGIN };

/// The panels placed by their bottom, growing upward (the chat window, the party window).
const BOTTOM: [&str; 2] = ["chat", "party"];
/// The right edge's column (the main menu, its submenu and the windows it opens): other panels anchored there keep
/// left of it.
const COLUMN: [&str; 3] = ["command", "command sub", "window"];

/// Where each panel starts (the user's own arrangement, 2026-10-26): the chat and party windows on the screen's
/// bottom, the location panel on the chat window and the combat window on it (retail's compass and action menu ride
/// the log), the target on the party window (retail's targetwi, flag 0x2000), the status icons, the command window and
/// the Timers at the left under the top bar, the main menu and the minimap at the right.
fn default_place(id: &str) -> Place {
    let at = |x, y| Place { x, y, ..Default::default() };
    let on = |to: &str| Place { dock: Some(Dock { to: to.into(), gap: 4.0, dx: 0.0 }), ..Default::default() };
    match id {
        "chat" => Place { x: 0.0121, b: 0.9998, ..Default::default() },
        "party" => Place { x: 0.8836, b: 0.9992, ..Default::default() },
        "where" => Place { dock: Some(Dock { to: "chat".into(), gap: -0.5, dx: 1.25 }), ..Default::default() },
        "combat" => on("where"),
        "target" => on("party"),
        // The magic and ability windows ride on the combat window (retail's sit on the log), the list on its categories.
        "cats" => on("combat"),
        "list" => on("cats"),
        // Below zero x is from the right edge.
        "status" => at(0.0637, 0.0313),
        "command" => at(-0.0047, 0.034),
        // raid1 / raid2's places (y 48 and 150 of the 448 layout).
        "alliance1" => at(0.87, 0.107),
        "alliance2" => at(0.87, 0.335),
        // The cast bar at retail's (16, 94); the Timers on the left edge, the minimap on the right one.
        "cast" => at(0.02, 0.21),
        "timers" => at(0.0, 0.2187),
        "minimap" => at(-0.001, 0.0353),
        // The command window: every window the command menu opens shows here. The item info rides the log as retail's
        // (mode 0, flag 0x1000).
        "window" => at(0.0047, 0.0347),
        "iteminfo" => on("combat"),
        _ => at(0.3, 0.3),
    }
}

/// The yes / no prompt (retail's comyn, stacked on the log): the question and Yes / No, the cursor's lit; its keys stay
/// retail's (Left / Right, Enter, Escape is No). A click answers; hovering moves the cursor.
fn confirm(p: &mut Panels, w: &mut World) {
    let Some((_, yes)) = w.confirm else { return };
    let m = p.m;
    let screen = p.ctx.content_rect();
    // On the chat window, else the screen's bottom (a faded chat window folds into the screen's edge).
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let caption = |k: u16, d: &str| p.gu.and_then(|g| g.child_caption("menu    comyn", k)).unwrap_or_else(|| d.to_string());
    let (y_label, n_label) = (caption(1, "Yes"), caption(2, "No"));
    let text = w.confirm_text.clone();
    let h = m.rects.borrow().get("confirm").map_or(110.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let (mut hover, mut click) = (None, None);
    p.panel_at("confirm", None, 380.0, at, |ui| {
        ui.label(egui::RichText::new(&text).size(m.theme.ui).color(m.theme.color("text")));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            for (val, label) in [(true, &y_label), (false, &n_label)] {
                let (r, resp) = ui.allocate_exact_size(vec2(120.0, 30.0), Sense::click());
                let on = val == yes;
                let face = if on { "controls/button_selected" } else if resp.hovered() { "controls/button_hover" } else { "controls/button_normal" };
                if let Some(s) = m.art.sprite(face) {
                    ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
                }
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(m.theme.ui), if on { m.theme.color("gold") } else { m.theme.color("text") });
                if resp.hovered() && ui.ctx().input(|i| i.pointer.delta() != Vec2::ZERO) {
                    hover = Some(val);
                }
                if resp.clicked() {
                    click = Some(val);
                }
                ui.add_space(8.0);
            }
        });
    });
    if let Some(v) = hover.or(click)
        && let Some(c) = w.confirm.as_mut()
        && c.1 != v
    {
        c.1 = v;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(v) = click {
        w.confirm_click = Some(v);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

/// How a panel shows: its title, width (and the chat window's default room), resizable, at a fixed point (beside
/// another, not moved), its frame, its exact height when known (placing it by its bottom without a frame's lag), its
/// fade, and how much of its height shows from the bottom up (the chat window folds into its bottom edge as it fades).
/// The rect returned and docked on is the part showing.
struct Show<'s> {
    title: Option<Title<'s>>,
    size: Vec2,
    resizable: bool,
    at: Option<Pos2>,
    frame: &'s str,
    height: Option<f32>,
    alpha: f32,
    fold: f32,
}

impl Default for Show<'_> {
    fn default() -> Self {
        Self { title: None, size: Vec2::ZERO, resizable: false, at: None, frame: "window_normal", height: None, alpha: 1.0, fold: 1.0 }
    }
}

impl<'a> Panels<'a> {
    fn look(&self) -> Look<'a> {
        Look { m: self.m, ctx: self.ctx, gu: self.gu }
    }

    /// A panel `width` points wide.
    fn panel(&mut self, id: &str, title: Option<Title>, width: f32, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.framed(id, title, width, "window_normal", add)
    }

    /// A panel in one of the pack's other frames (window_alert ...).
    fn framed(&mut self, id: &str, title: Option<Title>, width: f32, frame: &str, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.show(id, Show { title, size: vec2(width, 0.0), frame, ..Default::default() }, |ui, _| add(ui))
    }

    /// A panel shown beside another (the command submenu): at a screen point, not moved.
    fn panel_at(&mut self, id: &str, title: Option<Title>, width: f32, at: Pos2, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.show(id, Show { title, size: vec2(width, 0.0), at: Some(at), ..Default::default() }, |ui, _| add(ui))
    }

    /// A painter over a panel drawn this frame, for marks around its frame.
    fn over(&self, id: &str) -> egui::Painter {
        self.ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique(("modern panel", id))))
    }

    /// A panel's place: the saved one, else its default (a bottom-placed panel saved before it had one keeps the
    /// default bottom).
    fn place(&self, id: &str) -> Place {
        let mut p = self.layout.get(id).cloned().or_else(|| self.retail.get(id).cloned()).unwrap_or_else(|| default_place(id));
        if BOTTOM.contains(&id) && p.b <= 0.0 {
            p.b = default_place(id).b;
        }
        p
    }

    /// The resizable panel's room: its saved width and height, else `size`.
    fn room(&self, id: &str, size: Vec2) -> Vec2 {
        let p = self.place(id);
        vec2(if p.w > 0.0 { p.w } else { size.x }, if p.h > 0.0 { p.h } else { size.y })
    }

    /// A panel's top left for its height `h`: on the panel it is docked on, else from its place.
    fn resolve(&self, id: &str, place: &Place, h: f32) -> Pos2 {
        if let Some(d) = &place.dock
            && let Some(t) = self.target(&d.to, 0)
        {
            return pos2(t.min.x + d.dx, t.min.y - d.gap - h);
        }
        let s = self.ctx.content_rect();
        // A bottom-placed panel's bottom on a whole pixel (its height changing must not move it).
        let ppp = self.ctx.pixels_per_point();
        let y = if BOTTOM.contains(&id) { (s.height() * place.b * ppp).round() / ppp - h } else { s.height() * place.y };
        if place.x >= 0.0 {
            return pos2(s.width() * place.x, y);
        }
        // From the right edge (its width as last drawn), and left of the command column there while it is open, as
        // retail's minimap keeps left of the windows at the edge.
        let rects = self.m.rects.borrow();
        let w = rects.get(id).map_or(0.0, |r| r.width());
        let mut right = s.width() * (1.0 + place.x);
        if !COLUMN.contains(&id) {
            for r in COLUMN.iter().filter_map(|c| self.this.get(*c).or(rects.get(*c))) {
                if r.max.x > right - w && r.min.y < y + h.max(1.0) && r.max.y > y {
                    right = right.min(r.min.x - 4.0);
                }
            }
        }
        pos2(right - w, y)
    }

    /// The rect a panel docks on: drawn this frame, else last frame, else (not shown) no height where it would sit,
    /// so what sits on it falls to what it sits on.
    fn target(&self, id: &str, depth: u8) -> Option<Rect> {
        if depth > 8 {
            return None;
        }
        if let Some(r) = self.this.get(id) {
            return Some(*r);
        }
        if let Some(r) = self.m.rects.borrow().get(id).copied() {
            return Some(r);
        }
        let place = self.place(id);
        let at = match &place.dock {
            Some(d) => {
                let t = self.target(&d.to, depth + 1)?;
                pos2(t.min.x + d.dx, t.min.y - d.gap)
            }
            None => self.resolve(id, &Place { dock: None, ..place }, 0.0),
        };
        Some(Rect::from_min_size(at, Vec2::ZERO))
    }

    /// Whether `a` sits on `b`, directly or through others.
    fn rests_on(&self, a: &str, b: &str) -> bool {
        let mut cur = a.to_string();
        for _ in 0..8 {
            match self.place(&cur).dock {
                Some(d) if d.to == b => return true,
                Some(d) => cur = d.to,
                None => return false,
            }
        }
        false
    }

    /// A panel let go with its bottom within 5 pixels of another's top, over it, docks there and keeps the distance.
    fn dock_for(&self, id: &str, r: Rect) -> Option<Dock> {
        let tol = 5.0 / self.ctx.pixels_per_point();
        let last = self.m.rects.borrow();
        let mut best: Option<(f32, Dock)> = None;
        for (to, t) in self.this.iter().chain(last.iter().filter(|(k, _)| !self.this.contains_key(*k))) {
            let gap = t.min.y - r.max.y;
            if to == id || gap.abs() > tol || r.max.x <= t.min.x || r.min.x >= t.max.x || self.rests_on(to, id) {
                continue;
            }
            if best.as_ref().is_none_or(|(g, _)| gap.abs() < *g) {
                best = Some((gap.abs(), Dock { to: to.clone(), gap, dx: r.min.x - t.min.x }));
            }
        }
        best.map(|b| b.1)
    }

    /// The panel itself: the pack's window frame behind its title row and contents, moved by a drag on its background
    /// unless placed beside another; a resizable one has the resize glyph in its corner. Returns its rect.
    fn show(&mut self, id: &str, o: Show, add: impl FnOnce(&mut Ui, Vec2)) -> Option<Rect> {
        let (m, ctx) = (self.m, self.ctx);
        let screen = ctx.content_rect();
        let mut place = self.place(id);
        let room = if o.resizable { self.room(id, o.size) } else { o.size };
        let h = o.height.or_else(|| m.rects.borrow().get(id).map(|r| r.height())).unwrap_or(0.0);
        let mut pos = o.at.unwrap_or_else(|| self.resolve(id, &place, h));
        // The command window steps right of a retail window it would hide (they draw under every panel): the item
        // pickers a page opens at the top left (Set Bazaar's prices, the hand-over).
        if id == "window" && o.at.is_none() {
            let size = vec2(o.size.x.max(1.0), h.max(1.0));
            for _ in 0..4 {
                let r = Rect::from_min_size(pos, size).shrink(2.0);
                let Some(right) = m.retail.borrow().iter().filter(|f| f.intersects(r)).map(|f| f.max.x).reduce(f32::max) else { break };
                pos.x = right + 8.0;
            }
        }
        // A panel running past the screen's bottom rises until it fits (its place stays): rows under the edge could be
        // walked to but not seen.
        if o.at.is_none() && o.height.is_none() && !BOTTOM.contains(&id) && h > 0.0 {
            pos.y = pos.y.min(screen.max.y - h).max(screen.min.y);
        }
        let margin = if o.resizable { ROOM_MARGIN } else { MARGIN };
        let (title, frame, alpha, fold) = (o.title, o.frame, o.alpha, o.fold.clamp(0.0, 1.0));
        // A panel at a fixed point may stand partly off screen (the Timers slid in).
        let shown = egui::Area::new(Id::unique(("modern panel", id))).order(Order::Middle).current_pos(pos).movable(o.at.is_none()).constrain(o.at.is_none()).show(ctx, |ui| {
            // An area offers its contents only last frame's size: room to the screen's bottom, or a page grown taller
            // (a scroll area in it) stays clipped at its first height.
            // Not for the panels placed by their exact height (the chat window folding, the party window).
            if o.height.is_none() && !o.resizable {
                ui.set_max_height((screen.max.y - pos.y).max(ui.min_rect().height()));
            }
            if fold < 1.0 {
                let top = pos.y + h * (1.0 - fold);
                ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(pos2(f32::NEG_INFINITY, top), pos2(f32::INFINITY, f32::INFINITY))));
            }
            let bg = ui.painter().add(Shape::Noop);
            let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.set_width(room.x - margin.sum().x);
                ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
                if let Some(t) = &title {
                    title_row(ui, m, t);
                    ui.add_space(6.0);
                }
                add(ui, room);
            });
            let r = inner.response.rect;
            let showing = Rect::from_min_max(pos2(r.min.x, r.max.y - r.height() * fold), r.max);
            if let Some(f) = m.art.frame(frame) {
                ui.painter().set(bg, paint::faded(paint::window(f, showing, 1.0), alpha));
            }
            if !o.resizable {
                return None;
            }
            // The glyph sits left of the frame's corner ornament; the whole corner takes the drag.
            let g = ui.interact(Rect::from_min_max(r.max - vec2(44.0, 28.0), r.max), Id::unique(("modern grip", id)), Sense::drag());
            let lit = g.hovered() || g.dragged();
            if let Some(s) = m.art.sprite("controls/resize") {
                let c = if lit { Color32::WHITE } else { Color32::from_gray(170) };
                ui.painter().add(paint::image(&s, r.max - vec2(40.0, 24.0), 16.0, c.gamma_multiply(alpha)));
            }
            if lit {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            g.dragged().then(|| g.drag_delta())
        });
        let r = shown.response.rect;
        let showing = Rect::from_min_max(pos2(r.min.x, r.max.y - r.height() * fold), r.max);
        // Placement trace (debug), only when a panel moves.
        thread_local!(static SEEN: std::cell::RefCell<HashMap<String, Pos2>> = Default::default());
        if log::log_enabled!(log::Level::Debug) && SEEN.with(|s| s.borrow_mut().insert(id.to_string(), pos).is_none_or(|p| (p - pos).length() > 0.5)) {
            log::debug!("panel {id}: pos {pos:?} h {h} rect {r:?} showing {showing:?} dock {:?}", place.dock);
        }
        self.this.insert(id.to_string(), showing);
        if o.at.is_some() {
            return Some(showing);
        }
        let mut changed = false;
        let bottom = BOTTOM.contains(&id);
        let (sw, sh) = (screen.width().max(1.0), screen.height().max(1.0));
        if shown.response.dragged() {
            m.arranging.set(true);
            place.dock = None;
            place.x = r.min.x / sw;
            if bottom {
                place.b = r.max.y / sh;
            } else {
                place.y = r.min.y / sh;
            }
            changed = true;
        }
        if shown.response.drag_stopped() {
            place.dock = self.dock_for(id, r);
            changed = true;
        }
        // A resize moves the bottom with the pointer: a bottom-placed panel keeps its top.
        if let Some(g) = shown.inner {
            (place.w, place.h) = ((room.x + g.x).max(240.0), (room.y + g.y).max(60.0));
            if bottom && place.dock.is_none() {
                place.b = (r.max.y + g.y) / sh;
            }
            m.arranging.set(true);
            changed = true;
        }
        if changed {
            self.layout.insert(id.to_string(), place);
            *self.changed = true;
        }
        Some(showing)
    }
}

impl Modern {
    fn grid(&self, w: &World, gu: &GameUi) -> String {
        let mut g = self.grid.borrow_mut();
        if g.0.is_none_or(|t| t.elapsed().as_secs_f32() >= 0.5) {
            *g = (Some(std::time::Instant::now()), super::map::grid_text(w, gu.map_regions()));
        }
        g.1.clone()
    }

    /// A menu sprite from the game's files (a list's spell or ability icon) as an egui texture, made once.
    fn sprite_icon(&self, ctx: &Context, gu: &GameUi, lib: &str, index: usize) -> Option<egui::TextureId> {
        let key = format!("sprite {lib}:{index}");
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = gu.sprite_rgba(lib, index)?;
            Some(ctx.load_texture(key, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR))
        });
        t.as_ref().map(|t| t.id())
    }

    /// A name sprite's caption as text, read once.
    fn caption(&self, gu: &GameUi, lib: &str, index: usize) -> Option<String> {
        self.captions.borrow_mut().entry((lib.to_string(), index)).or_insert_with(|| gu.sprite_caption(lib, index)).clone()
    }

    /// An item's icon from the game's files (32 x 32) as an egui texture, made once.
    fn item_icon(&self, ctx: &Context, info: &super::items::ItemCache, id: u16) -> Option<egui::TextureId> {
        use crate::inventory::ItemInfo;
        let key = format!("item {id}");
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let rgba = info.icon(id)?;
            Some(ctx.load_texture(key, egui::ColorImage::from_rgba_unmultiplied([32, 32], &rgba), egui::TextureOptions::LINEAR))
        });
        t.as_ref().map(|t| t.id())
    }

    /// A status icon from the game's files (config 0x99 bit 2: the small set) as an egui texture, made once.
    fn status_icon(&self, ctx: &Context, gu: &GameUi, small: bool, id: u16) -> Option<egui::TextureId> {
        let key = format!("status {}:{id}", small as u8);
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = gu.status_rgba(small, id)?;
            Some(ctx.load_texture(key, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR))
        });
        t.as_ref().map(|t| t.id())
    }
}

fn title_row(ui: &mut Ui, m: &Modern, t: &Title) {
    title_row_in(ui, m, t, m.theme.color("gold"));
}

/// A title row in another colour (the target's name in its retail name colour).
fn title_row_in(ui: &mut Ui, m: &Modern, t: &Title, colour: egui::Color32) {
    let h = 28.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), egui::Sense::hover());
    let p = ui.painter();
    let mut x = r.min.x;
    if let Some(s) = t.icon.and_then(|i| m.art.sprite(i)) {
        p.add(paint::image(&s, pos2(x, r.center().y - 13.0), 26.0, egui::Color32::WHITE));
        x += 34.0;
    }
    let font = FontId::new(m.theme.title, FontFamily::Name(STRONG.into()));
    p.text(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, t.text, font, colour);
    if let Some(n) = t.note {
        p.text(pos2(r.max.x, r.center().y), egui::Align2::RIGHT_CENTER, n, FontId::proportional(m.theme.small), m.theme.color("muted"));
    }
}
