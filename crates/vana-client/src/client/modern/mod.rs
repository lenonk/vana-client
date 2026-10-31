//! The modern UI (docs/modern-ui.md; Settings > Client > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.
//!
//! Each panel is an egui Area that a drag on its background moves (its contents keep their clicks), kept by character
//! as screen fractions. The chat window also resizes by its corner grip: more room for the lines, nothing scaled.

mod auction;
mod bluset;
mod pupset;
mod maze;
mod charlnk;
mod borders;
mod chat;
mod chatsel;
mod combat;
mod command;
mod config;
mod craft;
mod equip;
mod extras;
mod gametext;
mod hud;
mod inspect;
mod items;
mod journal;
mod kit;
mod lobby;
mod lists;
mod loot;
mod macros;
mod minor;
mod paint;
pub mod plates;
mod points;
mod rows;
mod post;
mod shop;
mod social;
mod status;
mod storage;
mod tabs;
pub mod shot;
mod theme;
mod topbar;

use super::World;
use super::game_ui::GameUi;
use egui::{Color32, Context, FontFamily, FontId, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};
pub use command::{BarFocus, CmdMenu, bar_focus, key as command_key};
pub use config::{key as config_key, lobby_key as lobby_config_key, lobby_settings as lobby_config_settings};
pub use journal::key as journal_key;
pub use lists::key as magic_key;
pub use macros::key as macro_key;
pub use status::key as status_key;
pub use storage::key as storage_key;
pub use theme::{Theme, frame_image};
use theme::{Art, LABEL, NAME, STRONG};

pub use crate::config::{ModernDock as Dock, ModernLayout as Layout, ModernPlace as Place};
use std::collections::HashMap;

pub struct Modern {
    pub theme: Theme,
    art: Art,
    /// The chat window's tab and whether its settings are open; game textures (status icons) as egui textures, by key.
    chat_tab: std::cell::Cell<u8>,
    chat_settings: std::cell::Cell<bool>,
    /// Where each chat tab was last seen (the log's length then; chat.rs unread).
    chat_seen: std::cell::RefCell<[Option<usize>; 11]>,
    /// The login form's text size (lobby.rs), for the lobby's popups.
    lobby_text: std::cell::Cell<f32>,
    /// The windows' background opacity (config `modern_opacity`).
    opacity: std::cell::Cell<f32>,
    /// The panel a drag has really moved (show): a press alone moves nothing.
    dragging: std::cell::RefCell<Option<String>>,
    /// How menu rows look (rows.rs), and window borders (borders.rs).
    row_style: std::cell::Cell<rows::Style>,
    border_style: std::cell::Cell<borders::Style>,
    images: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>>,
    /// Status tooltips' name and description by (status id, small set), read once (hud.rs).
    pub status_tips: std::cell::RefCell<HashMap<(u16, bool), std::rc::Rc<(String, String)>>>,
    /// Macro lines and the spell or ability each names (textcmd::line_action), for greying the macro keys.
    macro_acts: std::cell::RefCell<HashMap<String, Option<(super::magic::Kind, u16)>>>,
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
    /// Where charlnk opens (beside the full log at the clicked line).
    charlnk_at: std::cell::Cell<Option<Pos2>>,
    /// The Status page's currencies: only those held.
    held_only: std::cell::Cell<bool>,
    /// The Journal's tab and cursor last drawn (a moved cursor is scrolled into view), and the quest list's line of
    /// each quest (headings between them).
    journal_seen: std::cell::Cell<(u8, usize)>,
    journal_rows: std::cell::RefCell<Vec<usize>>,
    /// egui's text styles enlarged (put back when the layer goes off).
    styled: bool,
    /// An edge window's heading dragged (or let go) this frame.
    edge_grip: std::cell::Cell<Option<extras::EdgeDrag>>,
    /// The edge window being dragged and where the pointer holds it (its ghost follows the pointer).
    edge_ghost: std::cell::RefCell<Option<(String, Vec2)>>,
    /// The top bar this frame: the span its boxes take and how far faded in (the top edge's windows sit under it).
    top_bar: std::cell::Cell<Option<(Rect, f32)>>,
    /// The skillchain helper's chains shown with all their ways.
    sc_open: std::cell::RefCell<std::collections::HashSet<u8>>,
    /// Each keyboard list's cursor last frame (`moved`).
    cursors: std::cell::RefCell<HashMap<&'static str, usize>>,
    /// The chat window's mouse selection (chatsel.rs): over the lines and its text this frame, the chat line's caret /
    /// anchor asked, whether the line has a selection, and the right-click menu and its pick.
    log_sel: std::cell::RefCell<Option<chatsel::LogSel>>,
    log_text: std::cell::RefCell<Option<String>>,
    line_req: std::cell::Cell<Option<(usize, Option<usize>)>>,
    line_sel: std::cell::Cell<bool>,
    chat_menu: std::cell::RefCell<Option<chatsel::Menu>>,
    chat_act: std::cell::RefCell<Option<chatsel::Act>>,
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
            chat_seen: Default::default(),
            dragging: Default::default(),
            lobby_text: Default::default(),
            opacity: std::cell::Cell::new(0.85),
            row_style: Default::default(),
            border_style: Default::default(),
            images: Default::default(),
            status_tips: Default::default(),
            macro_acts: Default::default(),
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
            charlnk_at: Default::default(),
            journal_seen: Default::default(),
            journal_rows: Default::default(),
            styled: false,
            retail: Default::default(),
            edge_grip: Default::default(),
            edge_ghost: Default::default(),
            top_bar: Default::default(),
            sc_open: Default::default(),
            cursors: Default::default(),
            log_sel: Default::default(),
            log_text: Default::default(),
            line_req: Default::default(),
            line_sel: Default::default(),
            chat_menu: Default::default(),
            chat_act: Default::default(),
            dirty: false,
        }
    }

    /// The windows' background opacity from the config (Settings' Panel Opacity).
    pub fn set_opacity(&self, o: f32) {
        self.opacity.set(o.clamp(0.1, 1.0));
    }

    /// A window background colour at the set opacity.
    pub(super) fn bg(&self, c: egui::Color32) -> egui::Color32 {
        c.gamma_multiply(self.opacity.get())
    }

    /// One frame: egui's zoom for the window height, the panels. True when the config (the layout, the chat font) is
    /// to be saved.
    pub fn ui(&mut self, ctx: &Context, gu: &GameUi, w: &mut World, window: [u32; 2], cfg: &mut crate::config::Config) -> bool {
        let zoom = self.theme.zoom(window[1] as f32, cfg.modern_scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
        // egui's own text (tooltips, the few labels without a size) a little larger than its defaults.
        if !self.styled {
            let (bronze, gold, track) = (self.theme.color("bronze"), self.theme.color("gold"), self.theme.color("charcoal"));
            ctx.all_styles_mut(|s| {
                use egui::TextStyle as T;
                for (t, size) in [(T::Body, 14.5), (T::Button, 14.5), (T::Small, 12.5), (T::Monospace, 14.0)] {
                    if let Some(f) = s.text_styles.get_mut(&t) {
                        f.size = size;
                    }
                }
                // Scroll bars always shown where a list is longer than its room (egui's float hidden until hovered),
                // in the pack's colours.
                s.spacing.scroll = egui::style::ScrollStyle { bar_width: 8.0, handle_min_length: 24.0, foreground_color: false, ..egui::style::ScrollStyle::solid() };
                s.visuals.widgets.inactive.bg_fill = bronze;
                s.visuals.widgets.hovered.bg_fill = gold;
                s.visuals.widgets.active.bg_fill = gold;
                s.visuals.extreme_bg_color = track;
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
        let mut p = Panels { m: self, ctx, gu: Some(gu), layout, changed: &mut changed, chat_font: &mut cfg.modern_chat_font, battle_in_all: &mut cfg.modern_battle_in_all, condense: &mut cfg.modern_condense, folded: &mut cfg.modern_folded, scan_kinds: &mut cfg.modern_scan_kinds, this: HashMap::new(), retail: HashMap::new(), focus: focus_panel(w) };
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
        points::draw(&mut p, w);
        journal::draw(&mut p, w);
        social::draw(&mut p, w);
        craft::draw(&mut p, w);
        macros::draw(&mut p, w);
        storage::draw(&mut p, w);
        loot::draw(&mut p, w);
        shop::draw(&mut p, w);
        auction::draw(&mut p, w);
        post::draw(&mut p, w);
        inspect::draw(&mut p, w);
        inspect::dress(&mut p, w);
        minor::draw(&mut p, w);
        bluset::draw(&mut p, w);
        pupset::draw(&mut p, w);
        maze::draw(&mut p, w);
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
        choice(&mut p, w);
        gm_prompt(&mut p, w);
        charlnk::draw(&mut p, w);
        mogdoor(&mut p, w);
        event_input(&mut p, w);
        let this = std::mem::take(&mut p.this);
        *self.rects.borrow_mut() = this;
        // The chat settings act at once (the world reads it as lines print).
        w.condense = cfg.modern_condense;
        w.chat.set_fold(cfg.modern_condense);
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
        let defaults = egui::Style::default();
        ctx.all_styles_mut(|s| {
            s.text_styles = defaults.text_styles.clone();
            s.spacing.scroll = defaults.spacing.scroll;
        });
        // The visuals back as the theme had them (dark or light).
        ctx.set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
        ctx.set_visuals_of(egui::Theme::Light, egui::Visuals::light());
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
    /// The chat's All tab shows the battle lines (config modern_battle_in_all).
    battle_in_all: &'a mut bool,
    /// An action's battle lines merge (config modern_condense).
    condense: &'a mut bool,
    folded: &'a mut std::collections::BTreeMap<String, bool>,
    scan_kinds: &'a mut u8,
    /// The panels drawn this frame, as others dock on them (the chat window while it fades: its bottom edge).
    this: HashMap<String, Rect>,
    /// Places retail works out this frame (the minimap left of the windows at the edge): the default until moved.
    retail: HashMap<String, Place>,
    /// The panel with the keys (focus_panel): drawn in the focused frame.
    focus: Option<&'static str>,
}

/// The panel that takes the keys now, as the key dispatch (client/mod.rs) routes them: the prompt, the chat line, an
/// event's choice or input, the lists, the windows the command menu opens, the main menu, the action menu.
fn focus_panel(w: &World) -> Option<&'static str> {
    use super::{event, searchmenu, treasure};
    Some(match () {
        _ if w.confirm.is_some() => "confirm",
        _ if w.typing => "chat",
        _ if w.palette.0 != 0 => "macro palette",
        _ if w.concierge_ui.is_some() => "window",
        _ if w.maze.screen.is_some() => "window",
        _ if event::choice(w).is_some() => "choice",
        _ if w.gmtell.open.is_some() => "gm prompt",
        _ if event::input(w).is_some() => "event input",
        _ if event::active(w) => return None,
        _ if w.charlnk.is_some() => "charlnk",
        _ if w.polmail.line.is_some() => "msgline",
        _ if w.polmail.list.is_some() => "msglist",
        _ if w.friendui.as_ref().is_some_and(|u| u.list.is_some() && u.stat.is_none()) => "friendlist",
        _ if w.friendui.is_some() => "window",
        _ if w.settings.is_some() || w.config.is_some() || w.bluset.is_some() || w.pupset.is_some() => "window",
        _ if w.magic.is_some() || w.ability.is_some() => "list",
        _ if w.cats.is_some() => "cats",
        _ if w.inspect.up() => "check",
        _ if w.seacom_edit.is_some() => "comment",
        _ if w.chatctrl.is_some() => "chatctrl",
        _ if treasure::active(w) => "loot",
        _ if w.levelsync.is_some() => "levelsync",
        _ if w.search.is_some() && !searchmenu::refining(w) && w.arealist.is_none() => "results",
        _ if w.arealist.is_some() && w.searchmenu.is_none() => "arealist",
        _ if w.linklist.is_some() => "linklist",
        _ if w.blklist.is_some() => "blklist",
        _ if w.mogdoor.is_some() => "mogdoor",
        _ if w.menu.is_none() && super::window_over_actions(w) => "window",
        _ if w.menu.is_some() && w.cmd.sub.is_some() => "command sub",
        _ if w.menu.is_some() => "command",
        _ if w.action_menu.is_some() => "combat",
        _ => return None,
    })
}

/// The panel margins inside the frame; the resizable one keeps a taller strip below for its glyph.
const MARGIN: egui::Margin = egui::Margin { left: 22, right: 22, top: 16, bottom: 18 };
const ROOM_MARGIN: egui::Margin = egui::Margin { bottom: 26, ..MARGIN };

/// The panels placed by their bottom, growing upward (the chat window, the party window).
const BOTTOM: [&str; 2] = ["chat", "party"];
/// The right edge's column (the main menu, its submenu and the windows it opens): the panels placed from the right
/// keep left of it; the edge windows stay put under it.
const COLUMN: [&str; 3] = ["command", "command sub", "window"];
/// The windows on a screen edge (extras.rs edge_window): they slide along it, so nothing docks on them.
const EDGE: [&str; 4] = ["timers", "minimap", "widescan", "regime"];

impl Dock {
    /// The side of the panel it sits on that `dx` counts from.
    fn x(&self, t: Rect) -> f32 {
        if self.right { t.max.x } else { t.min.x }
    }
}

/// Where each panel starts (the user's own arrangement, 2026-10-26): the chat and party windows on the screen's
/// bottom, the location panel on the chat window and the combat window on it (retail's compass and action menu ride
/// the log), the target on the party window (retail's targetwi, flag 0x2000), the status icons, the command window and
/// the Timers at the left under the top bar, the main menu and the minimap at the right.
fn default_place(id: &str) -> Place {
    let at = |x, y| Place { x, y, ..Default::default() };
    let on = |to: &str| Place { dock: Some(Dock { to: to.into(), gap: 4.0, dx: 0.0, right: false }), ..Default::default() };
    match id {
        "chat" => Place { x: 0.0121, b: 0.9998, ..Default::default() },
        "party" => Place { x: 0.8836, b: 0.9992, ..Default::default() },
        "where" => Place { dock: Some(Dock { to: "chat".into(), gap: -0.5, dx: 1.25, right: false }), ..Default::default() },
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
        // The 0x075 countdown under it, the objective's bars under that.
        "countdown" => at(0.02, 0.28),
        "objective" => at(0.02, 0.35),
        "timers" => at(0.0, 0.2187),
        // The training regime lower on the same edge.
        "regime" => at(0.0, 0.6),
        // The skillchain helper on the right edge, under the minimap.
        "skillchains" => at(-0.001, 0.25),
        "minimap" => at(-0.001, 0.0353),
        // The command window: every window the command menu opens shows here. The item info rides the log as retail's
        // (mode 0, flag 0x1000).
        "window" => at(0.0047, 0.0347),
        "iteminfo" => on("combat"),
        // The treasure pool rides the action menu it is opened from.
        "loot" => on("combat"),
        // /check beside the command window's place.
        "check" => at(0.2, 0.0347),
        "blklist" => at(0.2, 0.0347),
        // The chat mode window rides the action menu it is opened from.
        "chatctrl" => on("combat"),
        // The comment editor sits on the log as retail's (flag 0x1000).
        "comment" => on("combat"),
        _ => at(0.3, 0.3),
    }
}

/// The yes / no prompt (retail's comyn, stacked on the log): the kit's prompt; its keys stay retail's (Left / Right,
/// Enter, Escape is No). A click answers.
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
    let mut click = None;
    p.panel_at("confirm", None, 380.0, at, |ui| click = kit::yes_no(ui, m, &text, [&y_label, &n_label], yes, m.theme.ui));
    if let Some(v) = click {
        if let Some(c) = w.confirm.as_mut() {
            c.1 = v;
        }
        w.confirm_click = Some(v);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

/// A list stacked on the log as comyn is (an event's choice, the Mog House door menu): the question and every row in
/// its colour (in-line colour codes as the log's), the cursor's lit and kept in view; the row clicked.
fn log_list(p: &mut Panels, id: &str, question: &str, rows: &[(String, [u8; 4])], cursor: usize, free: bool) -> Option<usize> {
    const W: f32 = 460.0;
    const ROW: f32 = 28.0;
    let m = p.m;
    let screen = p.ctx.content_rect();
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let h = m.rects.borrow().get(id).map_or(160.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let mut click = None;
    p.panel_at(id, None, W, at, |ui| {
        let font = FontId::proportional(m.theme.ui);
        let job = |t: &str, base: [u8; 4], wrap: f32| chat::line_job(("", Color32::WHITE), t, base, &font, wrap);
        if !question.is_empty() {
            ui.label(job(question, [0x80; 4], W - 44.0));
            ui.add_space(6.0);
        }
        let seen = egui::Id::unique((id, "log list cursor seen"));
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
        egui::ScrollArea::vertical().id_salt((id, "rows")).max_height(8.0 * (ROW + 2.0)).auto_shrink([false, true]).show(ui, |ui| {
            for (k, (text, base)) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(W - 58.0, ROW), Sense::click());
                let face = match () {
                    _ if k == cursor => "controls/list_row_selected",
                    _ if resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                rows::face(ui.painter(), m, r, face, Color32::WHITE);
                let clip = Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
                let g = ui.painter().layout_job(job(text, *base, f32::INFINITY));
                ui.painter().with_clip_rect(clip).galley(pos2(r.min.x + 24.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
                if k == cursor && follow {
                    ui.scroll_to_rect(r, None);
                }
                if free && resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
        });
    });
    click
}

/// An event's choice (retail's query): its keys stay retail's (event.rs); a click picks.
fn choice(p: &mut Panels, w: &mut World) {
    let Some(c) = super::event::choice(w) else { return };
    let rows: Vec<(String, [u8; 4])> = c.options.iter().map(|o| (o.1.clone(), [0x80; 4])).collect();
    let (question, cursor) = (c.question.clone(), c.cursor);
    if let Some(k) = log_list(p, "choice", &question, &rows, cursor, w.confirm.is_none()) {
        super::event::set_choice(w, k);
        super::event::key(w, egui::Key::Enter);
    }
}

/// A GM prompt (retail's gmtell, the query's class): its keys stay gmtell.rs's; a click picks.
fn gm_prompt(p: &mut Panels, w: &mut World) {
    let Some(g) = w.gmtell.open.as_ref() else { return };
    let rows: Vec<(String, [u8; 4])> = g.options.iter().map(|o| (o.clone(), [0x80; 4])).collect();
    let (question, cursor) = (g.question.clone(), g.cursor);
    if let Some(k) = log_list(p, "gm prompt", &question, &rows, cursor, w.confirm.is_none()) {
        super::gmtell::pointer(w, k, true);
    }
}

/// An event's text input (retail's passinpu over the message line): its title (group 0xb by kind) or "Password:",
/// the text as typed with a steady caret; the keys stay event.rs's.
fn event_input(p: &mut Panels, w: &World) {
    let Some(inp) = super::event::input(w) else { return };
    let Some(gu) = p.gu else { return };
    let m = p.m;
    let title = match inp.kind {
        1..=5 => gu.group_b([0xb9, 0xbe, 0xb9, 0xc8, 0xc9][inp.kind as usize - 1]).unwrap_or_default(),
        _ => gu.child_caption("menu    passinpu", 1).unwrap_or_else(|| "Password:".into()),
    };
    let text = String::from_utf8_lossy(&inp.text).into_owned();
    let caret = inp.caret.min(text.len());
    let screen = p.ctx.content_rect();
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let h = m.rects.borrow().get("event input").map_or(80.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let caret = text[..caret].chars().count();
    p.panel_at("event input", None, 460.0, at, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&title).size(m.theme.ui).color(m.theme.color("gold")));
            kit::field(ui, m, &text, Some(caret), ui.available_width(), FontId::monospace(m.theme.ui), None);
        });
    });
}

/// The Mog House door menu (FUN_101d62b0): its title and rows (the exit areas while choosing one).
fn mogdoor(p: &mut Panels, w: &mut World) {
    let Some(sel) = w.mogdoor.as_ref().map(|m| m.sel) else { return };
    let (title, rows) = super::mogdoor_rows(w);
    let rows: Vec<(String, [u8; 4])> = rows.into_iter().map(|r| (r.1, r.2)).collect();
    if let Some(k) = log_list(p, "mogdoor", &title, &rows, sel, w.confirm.is_none()) {
        if let Some(m) = w.mogdoor.as_mut() {
            m.sel = k;
        }
        super::mogdoor_pick(w);
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
        // A dock on an edge window (saved before they refused them) gives way to the place itself.
        if p.dock.as_ref().is_some_and(|d| EDGE.contains(&d.to.as_str())) {
            p.dock = None;
        }
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
            return pos2(d.x(t) + d.dx, t.min.y - d.gap - h);
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
                pos2(d.x(t) + d.dx, t.min.y - d.gap)
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
            if to == id || EDGE.contains(&to.as_str()) || gap.abs() > tol || r.max.x <= t.min.x || r.min.x >= t.max.x || self.rests_on(to, id) {
                continue;
            }
            if best.as_ref().is_none_or(|(g, _)| gap.abs() < *g) {
                let right = self.place(to).x < 0.0;
                best = Some((gap.abs(), Dock { to: to.clone(), gap, dx: r.min.x - if right { t.max.x } else { t.min.x }, right }));
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
        // The panel with the keys in the focused frame.
        let frame = if frame == "window_normal" && self.focus == Some(id) { "window_focused" } else { frame };
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
            ui.painter().set(bg, paint::faded(borders::window(m, frame, showing), alpha));
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
        // The menu's windows draw over the edge windows (they stay where they were put).
        if COLUMN.contains(&id) {
            ctx.move_to_top(shown.response.layer_id);
        }
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
        // egui drags a movable window from the press: a click that never moves it leaves its place (and dock) alone.
        let moved = shown.response.total_drag_delta().is_some_and(|d| d.length() > 2.0);
        if shown.response.dragged() && (moved || m.dragging.borrow().as_deref() == Some(id)) {
            *m.dragging.borrow_mut() = Some(id.to_string());
            m.arranging.set(true);
            place.dock = None;
            place.x = r.min.x / sw;
            if bottom {
                // From its top and the height it is placed by: a drawn rect a pixel taller crept it down every frame.
                place.b = (r.min.y + if h > 0.0 { h } else { r.height() }) / sh;
            } else {
                place.y = r.min.y / sh;
            }
            changed = true;
        }
        if shown.response.drag_stopped() && m.dragging.borrow().as_deref() == Some(id) {
            *m.dragging.borrow_mut() = None;
            place.dock = self.dock_for(id, r);
            changed = true;
        }
        // A resize moves the bottom with the pointer: a bottom-placed panel keeps its top.
        if let Some(g) = shown.inner {
            (place.w, place.h) = ((room.x + g.x).max(240.0), (room.y + g.y).max(60.0));
            if bottom && place.dock.is_none() {
                place.b = (r.min.y + if h > 0.0 { h } else { r.height() } + g.y) / sh;
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
    /// Whether keyboard list `key`'s cursor moved to `at` since last frame: the row then scrolls into view (only then,
    /// so the wheel stays free).
    pub(super) fn moved(&self, key: &'static str, at: usize) -> bool {
        self.cursors.borrow_mut().insert(key, at) != Some(at)
    }

    /// A tooltip (ours) while the pointer rests on `r` and `ui`'s layer is the one under it: `title` in gold over `body`
    /// in the pack's tooltip frame.
    pub fn tip(&self, ui: &Ui, r: Rect, title: &str, body: &str) {
        self.tip_with(ui, r, || (title.to_string(), body.to_string()));
    }

    /// `tip` with its text made only while the pointer is on `r`.
    pub fn tip_with(&self, ui: &Ui, r: Rect, text: impl FnOnce() -> (String, String)) {
        let ctx = ui.ctx();
        let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        if r.contains(at) && ctx.layer_id_at(at) == Some(ui.layer_id()) {
            let (title, body) = text();
            self.tip_at(ctx, at, &title, &body);
        }
    }

    /// The tooltip by the pointer at `at`, once it has rested a moment (none while dragging).
    pub fn tip_at(&self, ctx: &Context, at: Pos2, title: &str, body: &str) {
        const REST: f32 = 0.35;
        if title.is_empty() && body.is_empty() || ctx.dragged_id().is_some() {
            return;
        }
        let still = ctx.input(|i| i.pointer.time_since_last_movement());
        if still < REST {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(REST - still));
            return;
        }
        egui::Area::new(Id::new("modern tip")).order(Order::Tooltip).fixed_pos(at + vec2(16.0, 20.0)).constrain(true).interactable(false).show(ctx, |ui| {
            let bg = ui.painter().add(Shape::Noop);
            let inner = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.spacing_mut().item_spacing.y = 3.0;
                if !title.is_empty() {
                    ui.label(egui::RichText::new(title).size(self.theme.ui).family(FontFamily::Name(STRONG.into())).color(self.theme.color("gold")));
                }
                if !body.is_empty() {
                    gametext::label(ui, self, body, self.theme.small, self.theme.color("text"));
                }
            });
            ui.painter().set(bg, borders::window(self, "window_tooltip", inner.response.rect));
        });
    }

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

    /// Ours: no line of the macro that names a spell or ability (/ma, /ja, /pet, /ws) can be used now; one with none
    /// never greys.
    fn macro_off(&self, w: &World, m: &super::macros::Macro) -> bool {
        let mut any = false;
        let mut acts = self.macro_acts.borrow_mut();
        for k in 0..6 {
            let line = crate::dat::font::decode_sjis(m.line(k));
            if line.is_empty() {
                continue;
            }
            let act = match acts.get(&line) {
                Some(a) => *a,
                // Not kept before the name tables load: every line would read as no action.
                None if w.names.is_none() || w.actions.is_none() => continue,
                None => *acts.entry(line.clone()).or_insert_with(|| super::textcmd::line_action(w, &line)),
            };
            let Some((kind, id)) = act else { continue };
            any = true;
            if super::magic::action_ready(w, kind, id) {
                return false;
            }
        }
        any
    }

    /// A status icon and its uv: the pack's (status_map.tsv, ours), else the game's (config 0x99 bit 2: the small set),
    /// made once.
    fn status_icon(&self, ctx: &Context, gu: &GameUi, small: bool, id: u16) -> Option<(egui::TextureId, Rect)> {
        if let Some(s) = status_art().get(&id).and_then(|name| self.art.sprite(&format!("status/{name}"))) {
            return Some((s.tex, s.uv));
        }
        let key = format!("status {}:{id}", small as u8);
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = gu.status_rgba(small, id)?;
            Some(ctx.load_texture(key, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR))
        });
        t.as_ref().map(|t| (t.id(), FULL_UV))
    }
}

/// A whole texture's uv.
const FULL_UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

/// Our status icons by status id (assets/modern/status_map.tsv: ids sharing a retail image share ours).
fn status_art() -> &'static HashMap<u16, &'static str> {
    static MAP: std::sync::OnceLock<HashMap<u16, &'static str>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        include_str!("../../../assets/modern/status_map.tsv").lines().filter_map(|l| l.split_once('\t')).filter_map(|(id, n)| Some((id.parse().ok()?, n))).collect()
    })
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

/// `text` on one line of at most `width`, cut with an ellipsis.
pub(super) fn one_line(painter: &egui::Painter, text: &str, font: egui::FontId, colour: egui::Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, colour);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    painter.layout_job(job)
}
