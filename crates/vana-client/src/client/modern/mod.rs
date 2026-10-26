//! The modern UI (docs/modern-ui.md; Settings > Linux > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.
//!
//! Each panel is an egui Area that a drag on its background moves (its contents keep their clicks), kept by character
//! as screen fractions. The chat window also resizes by its corner grip: more room for the lines, nothing scaled.

mod chat;
mod combat;
mod command;
mod hud;
mod paint;
pub mod shot;
mod theme;

use super::World;
use super::game_ui::GameUi;
use egui::{Color32, Context, FontFamily, FontId, Id, Order, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};
pub use command::{CmdMenu, key as command_key};
pub use theme::Theme;
use theme::{Art, STRONG};

pub use crate::config::{ModernLayout as Layout, ModernPlace as Place};

pub struct Modern {
    pub theme: Theme,
    art: Art,
    /// The chat window's tab and whether its settings are open; game textures (status icons) as egui textures, by key.
    chat_tab: std::cell::Cell<u8>,
    chat_settings: std::cell::Cell<bool>,
    images: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>>,
    /// The grid text ("(E-8)") and when it was made: rebuilt every half second, as retail's every 30 ticks.
    grid: std::cell::RefCell<(Option<std::time::Instant>, String)>,
    zoom: f32,
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
            zoom: 0.0,
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
        let layout = cfg.modern_places.entry(w.name.clone()).or_default();
        let mut changed = false;
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout, changed: &mut changed, chat_font: &mut cfg.modern_chat_font };
        hud::draw(&mut p, w);
        chat::panel(&mut p, &chat::view(w));
        command::draw(&mut p, w);
        combat::draw(&mut p, w);
        self.dirty |= changed;
        let save = self.dirty && !ctx.input(|i| i.pointer.any_down());
        if save {
            self.dirty = false;
        }
        save
    }
}

/// With the layer off egui is back at its own size (the F12 lines).
pub fn off(ctx: &Context, m: Option<&mut Modern>) {
    if let Some(m) = m.filter(|m| m.zoom != 0.0) {
        ctx.set_zoom_factor(1.0);
        m.zoom = 0.0;
    }
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
}

/// The panel margins inside the frame.
const MARGIN: egui::Margin = egui::Margin { left: 22, right: 22, top: 16, bottom: 18 };

impl<'a> Panels<'a> {
    fn look(&self) -> Look<'a> {
        Look { m: self.m, ctx: self.ctx, gu: self.gu }
    }

    /// A panel `width` points wide.
    fn panel(&mut self, id: &str, title: Option<Title>, width: f32, default: [f32; 2], add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.show(id, title, vec2(width, 0.0), false, default, None, |ui, _| add(ui))
    }

    /// The chat window: `room` points (width, list height) by default, which its corner grip changes.
    fn room_panel(&mut self, id: &str, room: Vec2, default: [f32; 2], add: impl FnOnce(&mut Ui, Vec2)) -> Option<Rect> {
        self.show(id, None, room, true, default, None, add)
    }

    /// A panel shown beside another (the command submenu): at a screen point, not moved.
    fn panel_at(&mut self, id: &str, title: Option<Title>, width: f32, at: Pos2, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.show(id, title, vec2(width, 0.0), false, [0.0; 2], Some(at), |ui, _| add(ui))
    }

    /// The panel itself: the pack's window frame behind its title row and contents, moved by a drag on its background
    /// unless placed beside another; a resizable one has the resize glyph in its corner. Returns its rect.
    #[allow(clippy::too_many_arguments)]
    fn show(&mut self, id: &str, title: Option<Title>, size: Vec2, resizable: bool, default: [f32; 2], at: Option<Pos2>, add: impl FnOnce(&mut Ui, Vec2)) -> Option<Rect> {
        let (m, ctx) = (self.m, self.ctx);
        let screen = ctx.content_rect();
        let mut place = self.layout.get(id).copied().unwrap_or(Place { x: default[0], y: default[1], ..Default::default() });
        let pos = at.unwrap_or(pos2(screen.width() * place.x, screen.height() * place.y));
        let room = if resizable { vec2(if place.w > 0.0 { place.w } else { size.x }, if place.h > 0.0 { place.h } else { size.y }) } else { size };
        // The resizable one keeps a strip below its contents for the glyph.
        let margin = if resizable { egui::Margin { bottom: 26, ..MARGIN } } else { MARGIN };
        let shown = egui::Area::new(Id::unique(("modern panel", id))).order(Order::Middle).current_pos(pos).movable(at.is_none()).show(ctx, |ui| {
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
            if let Some(f) = m.art.frame("window_normal") {
                ui.painter().set(bg, paint::window(f, r, 1.0));
            }
            if !resizable {
                return None;
            }
            // The glyph sits left of the frame's corner ornament; the whole corner takes the drag.
            let g = ui.interact(Rect::from_min_max(r.max - vec2(44.0, 28.0), r.max), Id::unique(("modern grip", id)), Sense::drag());
            let lit = g.hovered() || g.dragged();
            if let Some(s) = m.art.sprite("controls/resize") {
                ui.painter().add(paint::image(&s, r.max - vec2(40.0, 24.0), 16.0, if lit { Color32::WHITE } else { Color32::from_gray(170) }));
            }
            if lit {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            g.dragged().then(|| g.drag_delta())
        });
        let r = shown.response.rect;
        if at.is_some() {
            return Some(r);
        }
        let mut changed = false;
        if shown.response.dragged() {
            (place.x, place.y) = (r.min.x / screen.width().max(1.0), r.min.y / screen.height().max(1.0));
            changed = true;
        }
        if let Some(g) = shown.inner {
            (place.w, place.h) = ((room.x + g.x).max(240.0), (room.y + g.y).max(60.0));
            changed = true;
        }
        if changed {
            self.layout.insert(id.to_string(), place);
            *self.changed = true;
        }
        Some(r)
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
    let h = 28.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), egui::Sense::hover());
    let p = ui.painter();
    let mut x = r.min.x;
    if let Some(s) = t.icon.and_then(|i| m.art.sprite(i)) {
        p.add(paint::image(&s, pos2(x, r.center().y - 13.0), 26.0, egui::Color32::WHITE));
        x += 34.0;
    }
    let font = FontId::new(m.theme.title, FontFamily::Name(STRONG.into()));
    p.text(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, t.text, font, m.theme.color("gold"));
    if let Some(n) = t.note {
        p.text(pos2(r.max.x, r.center().y), egui::Align2::RIGHT_CENTER, n, FontId::proportional(m.theme.small), m.theme.color("muted"));
    }
}
