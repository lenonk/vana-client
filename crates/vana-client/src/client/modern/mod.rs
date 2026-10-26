//! The modern UI (docs/modern-ui.md; Settings > Linux > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.

mod hud;
mod paint;
mod theme;

use super::World;
use egui::{Color32, Context, FontFamily, Rect, RichText, Shape, pos2};
use std::collections::BTreeMap;
pub use theme::Theme;
use theme::{Art, DISPLAY};

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

    /// One frame: egui's zoom for the window height (theme.json "scale"), then the panels. True when a panel moved
    /// (the layout is to be saved).
    pub fn ui(&mut self, ctx: &Context, w: &mut World, window: [u32; 2], layout: &mut Layout) -> bool {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = self.theme.zoom(window[1] as f32 / native);
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

/// What a frame's panels share.
struct Panels<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    layout: &'a mut Layout,
    moved: &'a mut bool,
}

impl Panels<'_> {
    /// A movable panel `width` points wide: the pack's panel_window behind it, the title in the display face with a
    /// rule under it, then `add`'s contents. Placed from the layout, else at `default` (fractions of the screen); a
    /// new place is kept once the pointer is let go.
    fn panel(&mut self, id: &str, title: &str, width: f32, default: [f32; 2], add: impl FnOnce(&mut egui::Ui)) -> Option<Rect> {
        let t = &self.m.theme;
        let screen = self.ctx.content_rect();
        let saved = self.layout.get(id).map(|p| pos2(p[0], p[1]));
        let pos = saved.unwrap_or(pos2(screen.width() * default[0], screen.height() * default[1]));
        let pad = 14.0;
        let (accent, edge, text) = (t.color("accent"), t.color("edge"), t.color("text"));
        let title_size = t.ui + 3.0;
        let frame = self.m.art.sprite("panel_window");
        let shown = egui::Window::new(format!("modern {id}"))
            .title_bar(false)
            .resizable(false)
            .frame(egui::Frame::NONE)
            .default_pos(pos)
            .constrain(true)
            .show(self.ctx, |ui| {
                let bg = ui.painter().add(Shape::Noop);
                let inner = egui::Frame::NONE.inner_margin(pad).show(ui, |ui| {
                    ui.set_width(width - 2.0 * pad);
                    ui.visuals_mut().override_text_color = Some(text);
                    ui.label(RichText::new(title).family(FontFamily::Name(DISPLAY.into())).size(title_size).color(accent));
                    let y = ui.cursor().min.y + 3.0;
                    let x = ui.max_rect().x_range();
                    ui.painter().hline(x, y, egui::Stroke::new(1.0, edge.gamma_multiply(0.6)));
                    ui.add_space(9.0);
                    add(ui);
                });
                if let Some(s) = frame {
                    ui.painter().set(bg, paint::nine_slice(&s, inner.response.rect, 1.0, Color32::WHITE));
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
