//! The modern UI (docs/modern-ui.md; Settings > Linux > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.

mod chat;
mod hud;
mod paint;
pub mod shot;
mod theme;

use super::World;
use super::game_ui::GameUi;
use egui::{Context, FontFamily, FontId, Rect, Shape, Ui, pos2, vec2};
use std::collections::BTreeMap;
pub use theme::Theme;
use theme::{Art, STRONG};

/// A character's window places as fractions of the screen, by window id (config `modern_layout`): they keep their
/// place whatever the Interface Size or the resolution.
pub type Layout = BTreeMap<String, [f32; 2]>;

pub struct Modern {
    pub theme: Theme,
    art: Art,
    /// The chat window's tab; game textures (status icons) as egui textures, by key.
    chat_tab: std::cell::Cell<u8>,
    images: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>>,
    /// The grid text ("(E-8)") and when it was made: rebuilt every half second, as retail's every 30 ticks.
    grid: std::cell::RefCell<(Option<std::time::Instant>, String)>,
    zoom: f32,
    /// The screen in points last frame: when it changes (a resize, Interface Size) the panels are placed again from the
    /// layout, as egui keeps its own places in points.
    screen: egui::Vec2,
}

impl Modern {
    pub fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        Self { theme: Theme::load(), art: Art::load(ctx), chat_tab: Default::default(), images: Default::default(), grid: Default::default(), zoom: 0.0, screen: egui::Vec2::ZERO }
    }

    /// One frame: egui's zoom for the window height, then the panels. True when a panel moved (the layout is to be
    /// saved).
    pub fn ui(&mut self, ctx: &Context, gu: &GameUi, w: &mut World, window: [u32; 2], scale: u32, layout: &mut Layout) -> bool {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = self.theme.zoom(window[1] as f32 / native, scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
        let screen = ctx.content_rect().size();
        let reseed = screen != self.screen;
        self.screen = screen;
        let mut moved = false;
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout, moved: &mut moved, reseed };
        hud::draw(&mut p, w);
        chat::panel(&mut p, &chat::view(w));
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
    moved: &'a mut bool,
    reseed: bool,
}

impl<'a> Panels<'a> {
    fn look(&self) -> Look<'a> {
        Look { m: self.m, ctx: self.ctx, gu: self.gu }
    }

    /// A movable panel `width` points wide in the pack's normal window frame (any size: paint::window), its title row,
    /// then `add`'s contents. Placed from the layout, else at `default`; a new place is kept once the pointer is let go.
    fn panel(&mut self, id: &str, title: Option<Title>, width: f32, default: [f32; 2], add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        let m = self.m;
        let screen = self.ctx.content_rect();
        let at = self.layout.get(id).copied().unwrap_or(default);
        let pos = pos2(screen.width() * at[0], screen.height() * at[1]);
        let win = egui::Window::new(format!("modern {id}")).title_bar(false).resizable(false).frame(egui::Frame::NONE).constrain(true);
        let win = if self.reseed { win.current_pos(pos) } else { win.default_pos(pos) };
        let shown = win.show(self.ctx, |ui| {
                let bg = ui.painter().add(Shape::Noop);
                let inner = egui::Frame::NONE.inner_margin(egui::Margin { left: 22, right: 22, top: 16, bottom: 18 }).show(ui, |ui| {
                    ui.set_width(width - 44.0);
                    ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
                    if let Some(t) = &title {
                        title_row(ui, m, t);
                        ui.add_space(6.0);
                    }
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
            self.layout.insert(id.to_string(), [rect.min.x / screen.width().max(1.0), rect.min.y / screen.height().max(1.0)]);
            *self.moved = true;
        }
        shown.inner
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
