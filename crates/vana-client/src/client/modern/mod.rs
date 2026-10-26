//! The modern UI (docs/modern-ui.md; Settings > Linux > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.

mod hud;
mod paint;
pub mod shot;
mod theme;

use super::World;
use egui::{Context, FontFamily, FontId, Rect, Shape, Ui, pos2, vec2};
use std::collections::BTreeMap;
pub use theme::Theme;
use theme::{Art, STRONG};

/// A character's window places in points, by window id (config `modern_layout`).
pub type Layout = BTreeMap<String, [f32; 2]>;

pub struct Modern {
    pub theme: Theme,
    art: Art,
    zoom: f32,
}

impl Modern {
    pub fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        Self { theme: Theme::load(), art: Art::load(ctx), zoom: 0.0 }
    }

    /// One frame: egui's zoom for the window height, then the panels. True when a panel moved (the layout is to be
    /// saved).
    pub fn ui(&mut self, ctx: &Context, w: &mut World, window: [u32; 2], scale: u32, layout: &mut Layout) -> bool {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = self.theme.zoom(window[1] as f32 / native, scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
        let mut moved = false;
        let mut p = Panels { m: self, ctx, layout, moved: &mut moved };
        hud::draw(&mut p, w);
        moved
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

/// What a frame's panels share.
struct Panels<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    layout: &'a mut Layout,
    moved: &'a mut bool,
}

impl Panels<'_> {
    /// A movable panel `width` points wide in the pack's normal window frame (any size: paint::window), its title row,
    /// then `add`'s contents. Placed from the layout, else at `default` (fractions of the screen); a new place is kept
    /// once the pointer is let go.
    fn panel(&mut self, id: &str, title: Title, width: f32, default: [f32; 2], add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        let m = self.m;
        let screen = self.ctx.content_rect();
        let saved = self.layout.get(id).map(|p| pos2(p[0], p[1]));
        let pos = saved.unwrap_or(pos2(screen.width() * default[0], screen.height() * default[1]));
        let shown = egui::Window::new(format!("modern {id}"))
            .title_bar(false)
            .resizable(false)
            .frame(egui::Frame::NONE)
            .default_pos(pos)
            .constrain(true)
            .show(self.ctx, |ui| {
                let bg = ui.painter().add(Shape::Noop);
                let inner = egui::Frame::NONE.inner_margin(egui::Margin { left: 22, right: 22, top: 16, bottom: 18 }).show(ui, |ui| {
                    ui.set_width(width - 44.0);
                    ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
                    title_row(ui, m, &title);
                    ui.add_space(6.0);
                    add(ui);
                });
                if let Some(f) = m.art.frame("window_normal") {
                    ui.painter().set(bg, paint::window(f, inner.response.rect, 1.0));
                }
                inner.response.rect
            })?;
        let rect = shown.response.rect;
        let held = self.ctx.input(|i| i.pointer.any_down());
        if !held && (rect.min - pos).length() > 0.5 {
            self.layout.insert(id.to_string(), [rect.min.x, rect.min.y]);
            *self.moved = true;
        }
        shown.inner
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
