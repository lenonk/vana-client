//! The Extras in the modern layer: the Timers window (timers.rs's sections: recasts, our effects and those we put on
//! others, folding as retail's), the minimap (round, north up, centred on us, the radar's dots; the wheel zooms) and
//! the map overlay (Ctrl / Alt + O: the zone map in a box at its opacity; the game's mouse still pans and zooms it).

use super::super::map::MapImage;
use super::super::timers::{self, Group, Icon, SECTIONS};
use super::super::{World, event, minimap};
use super::{Panels, kit};
use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

pub fn draw(p: &mut Panels, w: &mut World) {
    timers_panel(p, w);
    regime_panel(p, w);
    skillchain_panel(p, w);
    minimap_panel(p, w);
    widescan_panel(p, w);
    overlay(p, w);
}

/// Hidden in events, under the map, the Region Info map.
fn hidden(w: &World) -> bool {
    event::active(w) || w.map.is_some() || w.cnq.is_some()
}

/// The time left as retail's clock ("1:05", "1:00:00").
fn clock(left: f32) -> String {
    let s = left.max(0.0).ceil() as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The Ashita Timers colours by time left: green from 30 s, yellow from 20, orange from 10, else red.
fn bar_colour(left: f32) -> Color32 {
    match left {
        l if l >= 30.0 => Color32::from_rgb(70, 190, 90),
        l if l >= 20.0 => Color32::from_rgb(220, 200, 60),
        l if l >= 10.0 => Color32::from_rgb(230, 140, 60),
        _ => Color32::from_rgb(220, 60, 60),
    }
}

/// The Timers window on a screen edge (edge_window): a heading that folds it (retail's drawer) to a tab; per section
/// a heading (a click folds it) and its rows: the icon, the name, the time and a bar filling toward a recast or
/// draining with an effect; the last 15 s of an effect pulse as the status icons do.
fn timers_panel(p: &mut Panels, w: &mut World) {
    let Some(gu) = p.gu else { return };
    if !w.extra_timers || hidden(w) {
        return;
    }
    let secs = timers::sections(w, gu);
    if secs.iter().all(|s| s.iter().all(|g| g.rows.is_empty())) {
        return;
    }
    let (m, ctx) = (p.m, p.ctx);
    let small = w.game.get(0x99) & 4 != 0;
    let t = 40.0 - (w.started.elapsed().as_secs_f32() * 60.0) % 40.0;
    let pulse = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
    let (out, folded) = (w.timers.out, w.timers.folded);
    let icon = |i: &Icon| match *i {
        Icon::Status(id) => m.status_icon(ctx, gu, small, id),
        Icon::List(lib, k) => m.sprite_icon(ctx, gu, lib, k).map(|t| (t, super::FULL_UV)),
        Icon::None => None,
    };
    let icons: Vec<Vec<Vec<Option<(egui::TextureId, Rect)>>>> = if out { secs.iter().map(|s| s.iter().map(|g| g.rows.iter().map(|r| icon(&r.icon)).collect()).collect()).collect() } else { Vec::new() };
    let mut fold: Option<usize> = None;
    // Clear of the menu's windows as the right edge's are (the item list grows over it on the left).
    let toggle = edge_window(p, "timers", WIDTH, true, !out, "Timers", |ui, side, fixed| {
        let toggle = flyout_header(ui, m, "Timers", side).clicked();
        edge_body(ui, "timers", fixed, f32::INFINITY, |ui| {
            for (k, groups) in secs.iter().enumerate() {
                let n: usize = groups.iter().map(|g| g.rows.len()).sum();
                if n == 0 {
                    continue;
                }
                ui.add_space(4.0);
                let shut = folded & (1 << k) != 0;
                if header(ui, m, &format!("{} ({n})", SECTIONS[k]), !shut).clicked() {
                    fold = Some(k);
                }
                if shut {
                    continue;
                }
                for (gi, g) in groups.iter().enumerate().filter(|(_, g)| !g.rows.is_empty()) {
                    group(ui, m, g, &icons[k][gi], pulse);
                }
            }
        });
        toggle
    });
    if toggle {
        w.timers.toggle();
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = fold {
        w.timers.folded ^= 1 << k;
        w.timers.dirty = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The training regime (regime.rs) on a screen edge: the area as its heading (a click folds it to a tab), a row per
/// target with its kills of the needed and a bar; a full one dims.
fn regime_panel(p: &mut Panels, w: &mut World) {
    if !w.extra_regime || hidden(w) {
        return;
    }
    let Some(r) = w.regime.shown().cloned() else { return };
    let m = p.m;
    let toggle = edge_window(p, "regime", WIDTH, true, !w.regime.out, "Training", |ui, side, fixed| {
        let toggle = flyout_header(ui, m, &r.heading(), side).clicked();
        ui.add_space(4.0);
        edge_body(ui, "regime", fixed, f32::INFINITY, |ui| regime_rows(ui, m, &r));
        toggle
    });
    if toggle {
        w.regime.toggle();
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The regime's rows: each target's kills of the needed and a bar; a full one dims.
fn regime_rows(ui: &mut Ui, m: &super::Modern, r: &super::super::regime::Regime) {
    for s in &r.slots {
        let (text, done, f) = super::super::regime::row_text(s);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let colour = if done { m.theme.color("muted") } else { m.theme.color("text") };
        let (label, count) = text.rsplit_once(": ").unwrap_or((text.as_str(), ""));
        ui.painter().text(pos2(rect.min.x, rect.min.y + 2.0), Align2::LEFT_TOP, label, FontId::proportional(m.theme.small), colour);
        ui.painter().text(pos2(rect.max.x, rect.min.y + 2.0), Align2::RIGHT_TOP, count, FontId::monospace(m.theme.small), colour);
        let bar = Rect::from_min_max(pos2(rect.min.x, rect.max.y - 6.0), pos2(rect.max.x, rect.max.y - 2.0));
        ui.painter().rect_filled(bar, 2.0, m.theme.color("charcoal"));
        if let Some(f) = f.filter(|f| *f > 0.0) {
            let fill = Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f.min(1.0), bar.max.y));
            ui.painter().rect_filled(fill, 2.0, if done { m.theme.color("muted") } else { m.theme.color("gold") });
        }
    }
}

/// The skillchain helper (skillchain.rs's view) on a screen edge, in a party: on our target the open resonance (its
/// properties or the chain made with its burst elements, the wait before a closer counts and the time left) and
/// everything in the party that closes it; then every chain the party can make, the strongest first, with its ways.
/// Lines wrap; past 60% of the screen's height it scrolls (its height never follows where it stands).
fn skillchain_panel(p: &mut Panels, w: &mut World) {
    if !w.extra_skillchain || hidden(w) {
        return;
    }
    let Some(v) = super::super::skillchain::view(w) else { return };
    let m = p.m;
    let screen = p.ctx.content_rect();
    let room = screen.height() * 0.6;
    // As wide as what it shows, more of the screen on the top or the bottom edge.
    let across = {
        let pl = p.place("skillchains");
        pl.edge != 0 && !pl.float
    };
    let width = skillchain_width(p.ctx, m, &v).clamp(240.0, screen.width() * if across { 0.9 } else { 0.5 });
    let toggle = edge_window(p, "skillchains", width, true, w.skillchain.folded, "Skillchains", |ui, side, fixed| {
        let toggle = flyout_header(ui, m, "Skillchains", side).clicked();
        edge_body(ui, "skillchains", fixed, room, |ui| skillchain_body(ui, m, &v));
        toggle
    });
    if toggle {
        w.skillchain.folded = !w.skillchain.folded;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The helper's contents (also offscreen in modern-shot).
pub(super) fn skillchain_body(ui: &mut Ui, m: &super::Modern, v: &super::super::skillchain::View) {
    use crate::dat::skillchains::{self as sc, Prop};
    let small = FontId::proportional(m.theme.small);
    let strong = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    let (text, muted, gold) = (m.theme.color("text"), m.theme.color("muted"), m.theme.color("gold"));
    let level_colour = |c: Prop| match sc::level(c) {
        3 | 4 => text,
        2 => gold,
        _ => m.theme.color("bronze"),
    };
    ui.spacing_mut().item_spacing.y = 2.0;
    // Text wrapped to the room left after `indent`.
    let wrapped = |ui: &mut Ui, s: &str, font: FontId, colour: Color32, indent: f32| {
        let width = (ui.available_width() - indent).max(40.0);
        let g = ui.ctx().fonts_mut(|f| f.layout(s.to_string(), font, colour, width));
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
        ui.painter().galley(pos2(r.min.x + indent, r.min.y), g, colour);
    };
    // A chain's name and its elements' icons from `x`; the right end.
    let chain_at = |ui: &Ui, c: Prop, x: f32, cy: f32, font: FontId| {
        let r = ui.painter().text(pos2(x, cy), Align2::LEFT_CENTER, sc::NAMES[c as usize], font, level_colour(c));
        let mut x = r.max.x + 5.0;
        for e in sc::elements(c) {
            if let Some(s) = m.art.sprite(super::gametext::ELEMENTS[*e as usize]) {
                ui.painter().add(super::paint::image(&s, pos2(x, cy - 7.0), 14.0, Color32::WHITE));
            }
            x += 15.0;
        }
        x
    };
    if let Some(n) = &v.now {
        // The monster and the clock: the wait before a closer counts, then the time left.
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
        ui.painter().text(pos2(row.min.x, row.center().y), Align2::LEFT_CENTER, &n.mob, small.clone(), muted);
        let waiting = n.age < super::super::skillchain::WAIT;
        let clock = if waiting { format!("wait {:.1} s", super::super::skillchain::WAIT - n.age) } else { format!("{:.1} s left", n.left) };
        ui.painter().text(pos2(row.max.x, row.center().y), Align2::RIGHT_CENTER, clock, small.clone(), if waiting { muted } else { gold });
        match n.chain {
            Some(c) => {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                let r = ui.painter().text(pos2(row.min.x, row.center().y), Align2::LEFT_CENTER, "Skillchain:", strong.clone(), gold);
                chain_at(ui, c, r.max.x + 6.0, row.center().y, strong.clone());
                let els: Vec<&str> = sc::elements(c).iter().map(|e| e.name()).collect();
                wrapped(ui, &format!("Magic burst: {}", els.join(", ")), small.clone(), text, 0.0);
            }
            None => {
                let props: Vec<&str> = n.props.iter().map(|&p| sc::NAMES[p as usize]).collect();
                wrapped(ui, &format!("Open: {}", props.join(" \u{b7} ")), strong.clone(), text, 0.0);
            }
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
        ui.painter().rect_filled(bar, 2.0, m.theme.color("charcoal"));
        let f = if waiting { n.age / super::super::skillchain::WAIT } else { n.left / (n.life - super::super::skillchain::WAIT).max(0.1) };
        ui.painter().rect_filled(Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f.clamp(0.0, 1.0), bar.max.y)), 2.0, if waiting { muted } else { gold });
        wrapped(ui, &format!("by {}", n.by), small.clone(), muted, 0.0);
        ui.add_space(4.0);
        if n.closers.is_empty() {
            wrapped(ui, "Nothing the party knows closes it.", small.clone(), muted, 0.0);
        } else {
            ui.label(egui::RichText::new("Close with").size(m.theme.small).color(gold));
        }
        // The chain (and its icons) in a column, who closes with what beside it.
        for c in &n.closers {
            let colour = if c.ready { text } else { muted };
            let top = ui.cursor().min.y;
            let col = 128.0;
            let width = (ui.available_width() - col).max(40.0);
            let g = ui.ctx().fonts_mut(|f| f.layout(format!("{}: {}", c.who, c.skill), small.clone(), colour, width));
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y.max(18.0)), Sense::hover());
            chain_at(ui, c.chain, r.min.x, top + 9.0, small.clone());
            ui.painter().galley(pos2(r.min.x + col, r.min.y + 1.0), g, colour);
        }
        ui.add_space(8.0);
    }
    ui.label(egui::RichText::new("Party chains").size(m.theme.small + 1.0).color(gold));
    if v.chains.is_empty() {
        wrapped(ui, "None known yet: the party's skills show here as they are used.", small.clone(), muted, 0.0);
    }
    // Each chain once, folded to its heading at first; a click on it opens every way the party makes it.
    for ch in &v.chains {
        let open = m.sc_open.borrow().contains(&ch.chain);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
        let c = if resp.hovered() { text } else { muted };
        let cy = row.center().y;
        if open {
            chevron_v(ui.painter(), pos2(row.min.x + 6.0, cy), false, c);
        } else {
            chevron(ui.painter(), pos2(row.min.x + 6.0, cy), false, c);
        }
        chain_at(ui, ch.chain, row.min.x + SC_INDENT, cy, strong.clone());
        let note = if ch.ways.len() == 1 { "1 way".to_string() } else { format!("{} ways", ch.ways.len()) };
        ui.painter().text(pos2(row.max.x, cy), Align2::RIGHT_CENTER, note, small.clone(), c);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            let mut o = m.sc_open.borrow_mut();
            if !o.remove(&ch.chain) {
                o.insert(ch.chain);
            }
        }
        if open {
            for way in &ch.ways {
                wrapped(ui, &way_text(way), small.clone(), if way.ready { text } else { muted }, SC_INDENT);
            }
        }
        ui.add_space(3.0);
    }
}

/// The ways' and the chain names' indent under the section chevrons.
const SC_INDENT: f32 = 16.0;

/// "You: Blade: Ten  ->  Ayame: Tachi: Kasha", a level-4 Light / Darkness marked.
fn way_text(way: &super::super::skillchain::Way) -> String {
    let lv4 = if crate::dat::skillchains::level(way.chain) == 4 { "  (level 4)" } else { "" };
    format!("{}: {}  \u{2192}  {}: {}{lv4}", way.opener.0, way.opener.1, way.closer.0, way.closer.1)
}

/// The helper's width for what it shows: its widest line (the open sections' ways too), the margins and the bar.
pub(super) fn skillchain_width(ctx: &egui::Context, m: &super::Modern, v: &super::super::skillchain::View) -> f32 {
    use crate::dat::skillchains::{self as sc, Prop};
    let small = FontId::proportional(m.theme.small);
    let strong = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    let w = |s: String, f: &FontId| ctx.fonts_mut(|x| x.layout_no_wrap(s, f.clone(), Color32::WHITE).size().x);
    let chain_w = |c: Prop, f: &FontId| w(sc::NAMES[c as usize].to_string(), f) + 5.0 + 15.0 * sc::elements(c).len() as f32;
    let mut most = w("Party chains".into(), &small);
    if let Some(n) = &v.now {
        most = most.max(w(n.mob.clone(), &small) + 16.0 + w("88.8 s left".into(), &small));
        most = most.max(match n.chain {
            Some(c) => w("Skillchain:".into(), &strong) + 6.0 + chain_w(c, &strong),
            None => w(format!("Open: {}", n.props.iter().map(|&p| sc::NAMES[p as usize]).collect::<Vec<_>>().join(" \u{b7} ")), &strong),
        });
        most = most.max(w(format!("by {}", n.by), &small));
        for c in &n.closers {
            most = most.max(128.0 + w(format!("{}: {}", c.who, c.skill), &small));
        }
    }
    let open = m.sc_open.borrow();
    for ch in &v.chains {
        most = most.max(SC_INDENT + chain_w(ch.chain, &strong) + 16.0 + w(format!("{} ways", ch.ways.len()), &small));
        if open.contains(&ch.chain) {
            for way in &ch.ways {
                most = most.max(SC_INDENT + w(way_text(way), &small));
            }
        }
    }
    most + 30.0 + 14.0
}

/// The screen edge an edge window is held by; a free one's, the side of the screen it stands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// Held by the top or the bottom: it slides sideways and folds to a tab across the edge.
    fn across(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// The side a place holds (a free one: the screen half its middle is on).
fn side_of(place: &super::Place, screen: Rect, width: f32) -> Side {
    match place.edge {
        _ if place.float => {
            if screen.width() * place.x + width / 2.0 < screen.center().x {
                Side::Left
            } else {
                Side::Right
            }
        }
        1 => Side::Top,
        2 => Side::Bottom,
        _ if place.x >= 0.0 => Side::Left,
        _ => Side::Right,
    }
}

/// The top edge for a window across `x0..x1`: under the top bar where it shows, moving with its fade.
fn top_edge(m: &super::Modern, screen: Rect, x0: f32, x1: f32) -> f32 {
    match m.top_bar.get() {
        Some((bar, a)) if x1 > bar.min.x && x0 < bar.max.x => screen.min.y + (bar.max.y + 4.0 - screen.min.y) * a,
        _ => screen.min.y,
    }
}

/// A window on a screen edge (its place: `edge` 1 the top, 2 the bottom, x its left; else x 0 or above the left,
/// below 0 the right, y its top): its frame open on that side; folded, a small tab out of the edge with `label`
/// beside a double chevron. On the top edge it sits under the top bar while that shows. It stays where it was put:
/// the menu's windows draw over it (user, 2026-10-27). Dragged it slides along its edge, and let go near another it
/// goes there. With `resize` the grip on its corner away from the edge sizes it (the place's w / h; a double click
/// gives it back its own), `width` until then. `add` draws the contents (told the side, and whether the height is
/// set: its body then scrolls in it, edge_body) and says whether its fold button was clicked; the tab's click counts
/// too. Returns that.
#[allow(clippy::too_many_arguments)]
fn edge_window(p: &mut Panels, key: &str, width: f32, resize: bool, folded: bool, label: &str, add: impl FnOnce(&mut Ui, Side, bool) -> bool) -> bool {
    let (m, ctx) = (p.m, p.ctx);
    let screen = ctx.content_rect();
    let place = p.place(key);
    let width = if resize && place.w > 0.0 { place.w.clamp(MIN_W, screen.width() * 0.95) } else { width };
    let fixed = (resize && place.h > 0.0).then(|| place.h.clamp(MIN_H, (screen.height() - 8.0).max(MIN_H)));
    // Let go away from the edges it stands free (x / y its top left), its whole frame drawn.
    let float = place.float;
    let fx = (screen.width() * place.x).max(screen.min.x);
    let side = side_of(&place, screen, width);
    let left = side == Side::Left;
    let across = side.across() && !float;
    // Kept whole on the screen: its top at least its own height (last frame's) above the bottom.
    let last = m.rects.borrow().get(key).copied();
    let tall = last.map_or(TAB_H, |r| r.height().max(TAB_H));
    let top = (screen.height() * place.y).clamp(screen.min.y, (screen.max.y - tall).max(screen.min.y));
    let id = egui::Id::unique(("modern edge", key));
    let mut toggle = false;
    if folded {
        // The label runs down the tab by a side edge (across one on the top or the bottom); the double chevron points
        // into the screen.
        let font = FontId::new(m.theme.small + 1.5, FontFamily::Name(super::STRONG.into()));
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, Color32::WHITE));
        let gw = galley.size().x;
        let size = if across { vec2((gw + 40.0).max(TAB_H), TAB_W) } else { vec2(TAB_W, (gw + 28.0).max(TAB_H)) };
        let ax = fx.min(screen.max.x - size.x);
        let r = match (float, side) {
            (true, _) => Rect::from_min_size(pos2(fx.min(screen.max.x - TAB_W), top), size),
            (false, Side::Left) => Rect::from_min_size(pos2(screen.min.x, top), size),
            (false, Side::Right) => Rect::from_min_size(pos2(screen.max.x - TAB_W, top), size),
            (false, Side::Top) => Rect::from_min_size(pos2(ax, top_edge(m, screen, ax, ax + size.x)), size),
            (false, Side::Bottom) => Rect::from_min_size(pos2(ax, screen.max.y - size.y), size),
        };
        // A retail window over it (they draw under every panel): it steps aside until that closes.
        if under_retail(m, r) {
            p.this.insert(key.to_string(), r);
            return false;
        }
        let mut rect = r;
        let mut drag = None;
        egui::Area::new(id).order(egui::Order::Middle).fixed_pos(r.min).constrain(false).show(ctx, |ui| {
            // A click opens it; a drag moves it as its window would.
            let (rr, resp) = ui.allocate_exact_size(r.size(), Sense::click_and_drag());
            drag = EdgeDrag::of(&resp);
            rect = rr;
            ui.painter().with_clip_rect(Rect::from_min_max(pos2(screen.min.x, rr.min.y), screen.max)).add(edge_frame_shape(rr, side, !float, m, false, 7));
            feedback(ui, m, rr, &resp, 7);
            let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
            if across {
                // Across the top or the bottom: the word as it reads, the chevrons toward the screen at its end.
                ui.painter().galley(pos2(rr.min.x + 10.0, rr.center().y - galley.size().y / 2.0), galley, c);
                for d in [-3.0, 3.0] {
                    chevron_v(ui.painter(), pos2(rr.max.x - 14.0, rr.center().y + d), side == Side::Bottom, c);
                }
            } else {
                // The word turned toward the screen: a quarter counter-clockwise on the left (read bottom up),
                // clockwise on the right (read top down).
                let (at, angle) = if left { (pos2(rr.min.x + 4.0, rr.center().y + gw / 2.0), -std::f32::consts::FRAC_PI_2) } else { (pos2(rr.max.x - 4.0, rr.center().y - gw / 2.0), std::f32::consts::FRAC_PI_2) };
                ui.painter().add(egui::epaint::TextShape::new(at, galley, c).with_angle(angle));
                let cx = if left { rr.max.x - 10.0 } else { rr.min.x + 10.0 };
                for d in [-3.0, 3.0] {
                    chevron(ui.painter(), pos2(cx + d, rr.center().y), !left, c);
                }
            }
            toggle = resp.clicked();
        });
        p.this.insert(key.to_string(), rect);
        if let Some(d) = drag {
            edge_drag(p, key, rect, 7, d);
        }
        return toggle;
    }
    let h = fixed.unwrap_or_else(|| last.map_or(TAB_H, |r| r.height()));
    let ax = fx.min(screen.max.x - width);
    let (x, y) = match (float, side) {
        (true, _) => (ax, top),
        (false, Side::Left) => (screen.min.x, top),
        (false, Side::Right) => (screen.max.x - width, top),
        (false, Side::Top) => (ax, top_edge(m, screen, ax, ax + width)),
        (false, Side::Bottom) => (ax, (screen.max.y - h).max(screen.min.y)),
    };
    if under_retail(m, Rect::from_min_size(pos2(x, y), vec2(width, h))) {
        p.this.insert(key.to_string(), Rect::from_min_size(pos2(x, y), vec2(width, h)));
        return false;
    }
    // The held side's margin 12, the inner side's 18 (its jewel).
    let margin = match (float, side) {
        (true, _) => egui::Margin { left: 16, right: 16, top: 12, bottom: 14 },
        (false, Side::Left) => egui::Margin { left: 12, right: 18, top: 12, bottom: 14 },
        (false, Side::Right) => egui::Margin { left: 18, right: 12, top: 12, bottom: 14 },
        (false, Side::Top) => egui::Margin { left: 16, right: 16, top: 12, bottom: 18 },
        (false, Side::Bottom) => egui::Margin { left: 16, right: 16, top: 18, bottom: 12 },
    };
    let mut moved: Option<EdgeDrag> = None;
    let mut sized: Option<Vec2> = None;
    let shown = egui::Area::new(id).order(egui::Order::Middle).fixed_pos(pos2(x, y)).constrain(false).show(ctx, |ui| {
        // Room to the screen's bottom (an area offers only last frame's size).
        ui.set_max_height((screen.max.y - y).max(ui.min_rect().height()));
        // Under the top bar the open top's border stays hidden above the window.
        let bg = ui.painter().with_clip_rect(Rect::from_min_max(pos2(screen.min.x, y), screen.max)).add(Shape::Noop);
        // The background moves it: sensed under the contents (last frame's size), so they keep their clicks.
        let drag = ui.interact(Rect::from_min_size(pos2(x, y), vec2(width, h)), id.with("drag"), Sense::drag());
        moved = EdgeDrag::of(&drag);
        m.edge_grip.set(None);
        let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
            ui.set_width(width - margin.sum().x);
            if let Some(h) = fixed {
                ui.set_min_height(h - margin.sum().y);
                ui.set_max_height(h - margin.sum().y);
            }
            ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
            toggle = add(ui, side, fixed.is_some());
        });
        if let Some(g) = m.edge_grip.take() {
            moved = Some(g);
        }
        let r = inner.response.rect;
        ui.painter().set(bg, edge_frame_shape(r, side, !float, m, !float, 16));
        if resize {
            sized = size_grip(ui, m, r, if float { Side::Left } else { side }, id);
        }
        if moved.is_some_and(|d| !d.stopped) {
            ui.painter().rect_stroke(r.expand(1.0), 16, Stroke::new(2.5, m.theme.color("gold")), egui::StrokeKind::Outside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if drag.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        r
    });
    p.this.insert(key.to_string(), shown.inner);
    if let Some(s) = sized {
        let mut place = p.place(key);
        (place.w, place.h) = if s == Vec2::ZERO { (0.0, 0.0) } else { (s.x.clamp(MIN_W, screen.width() * 0.95), s.y.clamp(MIN_H, (screen.height() - 8.0).max(MIN_H))) };
        p.layout.insert(key.to_string(), place);
        m.arranging.set(true);
        *p.changed = true;
    } else if let Some(d) = moved {
        edge_drag(p, key, shown.inner, 16, d);
    }
    toggle
}

/// The smallest an edge window is sized to.
const MIN_W: f32 = 180.0;
const MIN_H: f32 = 90.0;

/// The size grip on the corner away from the edge (the bottom one, the top one on the bottom edge): three short
/// diagonals. Dragged, the size it is pulled to; double-clicked, Vec2::ZERO (its own size again).
fn size_grip(ui: &mut Ui, m: &super::Modern, r: Rect, side: Side, id: egui::Id) -> Option<Vec2> {
    let (corner, sx, sy) = match side {
        Side::Right => (pos2(r.min.x + 5.0, r.max.y - 5.0), 1.0, 1.0),
        Side::Bottom => (pos2(r.max.x - 5.0, r.min.y + 5.0), -1.0, -1.0),
        _ => (pos2(r.max.x - 5.0, r.max.y - 5.0), -1.0, 1.0),
    };
    let g = ui.interact(Rect::from_center_size(corner, vec2(22.0, 22.0)), id.with("size"), Sense::click_and_drag());
    let c = if g.hovered() || g.dragged() { m.theme.color("gold") } else { m.theme.color("bronze") };
    for k in [4.0, 8.0, 12.0] {
        ui.painter().line_segment([corner + vec2(sx * k, -2.0 * sy), corner + vec2(sx * 2.0, -k * sy)], Stroke::new(1.5, c));
    }
    if g.hovered() || g.dragged() {
        ui.ctx().set_cursor_icon(if sx * sy < 0.0 { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::ResizeNeSw });
    }
    if g.double_clicked() {
        return Some(Vec2::ZERO);
    }
    g.dragged().then(|| {
        let d = g.drag_delta();
        vec2(r.width() - d.x * sx, r.height() + d.y * sy)
    })
}

/// An edge window's body under its heading: scrolled in the window's height once that is set (`fixed`), else as
/// tall as it is up to `cap`.
fn edge_body(ui: &mut Ui, salt: &str, fixed: bool, cap: f32, add: impl FnOnce(&mut Ui)) {
    let room = if fixed { ui.available_height() } else { cap.min(ui.available_height()) };
    egui::ScrollArea::vertical().id_salt(salt).max_height(room).min_scrolled_height(if fixed { room } else { 64.0 }).auto_shrink([false, !fixed]).show(ui, add);
}

/// An edge window's drag this frame: where the pointer is, and whether it was let go.
#[derive(Clone, Copy)]
pub(super) struct EdgeDrag {
    at: Pos2,
    stopped: bool,
}

impl EdgeDrag {
    fn of(resp: &egui::Response) -> Option<EdgeDrag> {
        let stopped = resp.drag_stopped();
        if !(resp.dragged() || stopped) {
            return None;
        }
        let at = resp.interact_pointer_pos().or_else(|| resp.ctx.input(|i| i.pointer.latest_pos()))?;
        Some(EdgeDrag { at, stopped })
    }
}

/// Pulled this far off its edge an edge window's ghost shows; let go this near a screen edge it is held there.
const PULL: f32 = 64.0;
const EDGE_SNAP: f32 = 10.0;

/// An edge window (or its tab) standing at `rect` dragged: held by its edge it slides along it, the window itself;
/// pulled more than PULL off, its ghost follows the pointer (or sits on the edge it would be let go near, any of the
/// four), and let go it lands there, on that edge or standing free. A free one moves with the pointer.
fn edge_drag(p: &mut Panels, key: &str, rect: Rect, radius: u8, d: EdgeDrag) {
    let (m, ctx) = (p.m, p.ctx);
    let screen = ctx.content_rect();
    let (sw, sh) = (screen.width().max(1.0), screen.height().max(1.0));
    let mut place = p.place(key);
    // Held where it was pressed (egui starts the drag a few points later).
    let off = {
        let mut g = m.edge_ghost.borrow_mut();
        match g.as_ref() {
            Some((k, o)) if k == key => *o,
            _ => {
                let o = ctx.input(|i| i.pointer.press_origin()).unwrap_or(d.at) - rect.min;
                *g = Some((key.to_string(), o));
                o
            }
        }
    };
    if d.stopped {
        *m.edge_ghost.borrow_mut() = None;
    }
    let would = Rect::from_min_size(d.at - off, rect.size());
    // Where it would be, kept on the screen.
    let x = would.min.x.clamp(screen.min.x, (screen.max.x - rect.width()).max(screen.min.x));
    let y = would.min.y.clamp(screen.min.y, (screen.max.y - rect.height()).max(screen.min.y));
    let top = top_edge(m, screen, x, x + rect.width());
    m.arranging.set(true);
    *p.changed = true;
    let gap = |s: Side| match s {
        Side::Left => would.min.x - screen.min.x,
        Side::Right => screen.max.x - would.max.x,
        Side::Top => would.min.y - top,
        Side::Bottom => screen.max.y - would.max.y,
    };
    let side = side_of(&place, screen, rect.width());
    if !place.float && gap(side) < PULL {
        if side.across() {
            place.x = x / sw;
        } else {
            place.y = (y / sh).clamp(0.0, 0.95);
        }
        p.layout.insert(key.to_string(), place);
        return;
    }
    // The screen edge it would be held by: the nearest within EDGE_SNAP, pushed past it too.
    let near = [Side::Left, Side::Right, Side::Top, Side::Bottom].into_iter().filter(|&s| gap(s) < EDGE_SNAP).min_by(|&a, &b| gap(a).total_cmp(&gap(b)));
    let free = Rect::from_min_size(pos2(x, y), rect.size());
    if !d.stopped {
        let at_edge = near.map(|s| {
            let at = match s {
                Side::Left => pos2(screen.min.x, y),
                Side::Right => pos2(screen.max.x - rect.width(), y),
                Side::Top => pos2(x, top),
                Side::Bottom => pos2(x, screen.max.y - rect.height()),
            };
            Rect::from_min_size(at, rect.size())
        });
        let float = place.float;
        if float {
            (place.x, place.y) = (free.min.x / sw, free.min.y / sh);
            p.layout.insert(key.to_string(), place);
        }
        if let Some(g) = at_edge.or((!float).then_some(free)) {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(("edge ghost", key))));
            painter.rect(g, radius, m.theme.color("surface").gamma_multiply(0.45), Stroke::new(2.0, m.theme.color("gold")), egui::StrokeKind::Inside);
        }
        return;
    }
    let along = (y / sh).clamp(0.0, 0.95);
    (place.float, place.edge, place.x, place.y) = match near {
        Some(Side::Left) => (false, 0, 0.0, along),
        Some(Side::Right) => (false, 0, -0.001, along),
        Some(Side::Top) => (false, 1, x / sw, along),
        Some(Side::Bottom) => (false, 2, x / sw, along),
        None => (true, 0, free.min.x / sw, free.min.y / sh),
    };
    p.layout.insert(key.to_string(), place);
}

/// A heading's drag moves its edge window (as its background's does); the hand says it can.
fn grip(m: &super::Modern, resp: &egui::Response) {
    if let Some(d) = EdgeDrag::of(resp) {
        m.edge_grip.set(Some(d));
    } else if resp.hovered() {
        resp.ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// While a tab is dragged: a gold outline and the grabbing hand; hovered, the open hand.
fn feedback(ui: &Ui, m: &super::Modern, r: Rect, resp: &egui::Response, radius: u8) {
    if resp.dragged() {
        ui.painter().rect_stroke(r.expand(1.0), radius, Stroke::new(2.5, m.theme.color("gold")), egui::StrokeKind::Outside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// A retail window drawn this frame overlaps `r`.
pub(super) fn under_retail(m: &super::Modern, r: Rect) -> bool {
    m.retail.borrow().iter().any(|f| f.intersects(r.shrink(2.0)))
}

/// The window's width and its folded tab's size.
const WIDTH: f32 = 250.0;
const TAB_W: f32 = 40.0;
const TAB_H: f32 = 56.0;

/// A chevron at `c` pointing left or right.
fn chevron(p: &egui::Painter, c: Pos2, left: bool, col: Color32) {
    let pts = if left { vec![pos2(c.x + 3.5, c.y - 6.0), pos2(c.x + 3.5, c.y + 6.0), pos2(c.x - 3.5, c.y)] } else { vec![pos2(c.x - 3.5, c.y - 6.0), pos2(c.x - 3.5, c.y + 6.0), pos2(c.x + 3.5, c.y)] };
    p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// A chevron at `c` pointing up or down.
fn chevron_v(p: &egui::Painter, c: Pos2, up: bool, col: Color32) {
    let pts = if up { vec![pos2(c.x - 6.0, c.y + 3.5), pos2(c.x + 6.0, c.y + 3.5), pos2(c.x, c.y - 3.5)] } else { vec![pos2(c.x - 6.0, c.y - 3.5), pos2(c.x + 6.0, c.y - 3.5), pos2(c.x, c.y + 3.5)] };
    p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// The edge window's frame over `r`: no border on the held side (it runs past the screen's edge), the other corners
/// rounded, a bronze line with a thin gold one inside it, and on the inner side a small diamond when `jewel`.
pub(super) fn edge_frame_shape(r: Rect, side: Side, attached: bool, m: &super::Modern, jewel: bool, radius: u8) -> Shape {
    let ext = match (attached, side) {
        (false, _) => r,
        (true, Side::Left) => Rect::from_min_max(pos2(r.min.x - 10.0, r.min.y), r.max),
        (true, Side::Right) => Rect::from_min_max(r.min, pos2(r.max.x + 10.0, r.max.y)),
        (true, Side::Top) => Rect::from_min_max(pos2(r.min.x, r.min.y - 10.0), r.max),
        (true, Side::Bottom) => Rect::from_min_max(r.min, pos2(r.max.x, r.max.y + 10.0)),
    };
    let round = |k: u8| match (attached, side) {
        (false, _) => egui::CornerRadius::same(k),
        (true, Side::Left) => egui::CornerRadius { nw: 0, sw: 0, ne: k, se: k },
        (true, Side::Right) => egui::CornerRadius { nw: k, sw: k, ne: 0, se: 0 },
        (true, Side::Top) => egui::CornerRadius { nw: 0, ne: 0, sw: k, se: k },
        (true, Side::Bottom) => egui::CornerRadius { nw: k, ne: k, sw: 0, se: 0 },
    };
    // The other border styles draw their own (borders.rs), without the jewel.
    if let Some(s) = super::borders::edge(m, ext, round(radius)) {
        return s;
    }
    let mut v = vec![
        Shape::Rect(egui::epaint::RectShape::filled(ext, round(radius), m.bg(m.theme.color("surface")))),
        Shape::Rect(egui::epaint::RectShape::stroke(ext, round(radius), Stroke::new(2.0, m.theme.color("bronze")), egui::StrokeKind::Inside)),
        Shape::Rect(egui::epaint::RectShape::stroke(ext.shrink(4.0), round(radius.saturating_sub(4)), Stroke::new(1.0, m.theme.color("gold").gamma_multiply(0.45)), egui::StrokeKind::Inside)),
    ];
    if jewel {
        let c = match side {
            Side::Left => pos2(ext.max.x - 1.0, ext.center().y),
            Side::Right => pos2(ext.min.x + 1.0, ext.center().y),
            Side::Top => pos2(ext.center().x, ext.max.y - 1.0),
            Side::Bottom => pos2(ext.center().x, ext.min.y + 1.0),
        };
        let d = 5.0;
        v.push(Shape::convex_polygon(vec![pos2(c.x, c.y - d), pos2(c.x + d, c.y), pos2(c.x, c.y + d), pos2(c.x - d, c.y)], m.theme.color("gold"), Stroke::new(1.0, Color32::from_black_alpha(200))));
    }
    Shape::Vec(v)
}

/// An edge window's heading: the title, and a chevron toward the edge (a click folds it there): on the inner side of
/// a side edge's, at the right of the top's or the bottom's. A drag moves the window.
fn flyout_header(ui: &mut Ui, m: &super::Modern, text: &str, side: Side) -> egui::Response {
    flyout_heading(ui, m, text, FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into())), side)
}

fn flyout_heading(ui: &mut Ui, m: &super::Modern, text: &str, font: FontId, side: Side) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    grip(m, &resp);
    let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
    let cy = r.center().y;
    let tx = if side == Side::Right { r.min.x + 26.0 } else { r.min.x };
    ui.painter().text(pos2(tx, cy), Align2::LEFT_CENTER, text, font, c);
    // The fold button: the chevron in a round knob, so it reads as a button.
    let k = pos2(if side == Side::Right { r.min.x + 9.0 } else { r.max.x - 9.0 }, cy);
    ui.painter().circle(k, 9.0, m.theme.color("charcoal"), Stroke::new(1.5, c));
    match side {
        Side::Left => chevron(ui.painter(), k - vec2(0.5, 0.0), true, c),
        Side::Right => chevron(ui.painter(), k + vec2(0.5, 0.0), false, c),
        Side::Top | Side::Bottom => chevron_v(ui.painter(), k, side == Side::Top, c),
    }
    resp
}

/// A heading with its fold chevron; the response takes clicks.
fn header(ui: &mut Ui, m: &super::Modern, text: &str, open: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
    ui.painter().text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, text, FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into())), c);
    // A chevron: down while open, right while folded.
    let (cx, cy) = (r.max.x - 8.0, r.center().y);
    let pts = if open { vec![pos2(cx - 5.0, cy - 2.5), pos2(cx + 5.0, cy - 2.5), pos2(cx, cy + 3.5)] } else { vec![pos2(cx - 2.5, cy - 5.0), pos2(cx - 2.5, cy + 5.0), pos2(cx + 3.5, cy)] };
    ui.painter().add(Shape::convex_polygon(pts, c, Stroke::NONE));
    resp
}

fn group(ui: &mut Ui, m: &super::Modern, g: &Group, icons: &[Option<(egui::TextureId, Rect)>], pulse: f32) {
    if let Some(name) = &g.name {
        ui.label(egui::RichText::new(name).size(m.theme.small).color(m.theme.color("muted")));
    }
    for (r, icon) in g.rows.iter().zip(icons) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let waiting = r.left <= 0.0;
        let k = if !r.fills && r.left > 0.0 && r.left <= 15.0 { 0.5 + 0.5 * pulse } else { 1.0 };
        let tint = Color32::from_white_alpha((255.0 * k) as u8);
        if let Some((t, uv)) = icon {
            ui.painter().image(*t, Rect::from_min_size(pos2(rect.min.x, rect.min.y + 3.0), vec2(22.0, 22.0)), *uv, tint);
        }
        let text = if waiting { m.theme.color("muted") } else { m.theme.color("text") };
        let x = rect.min.x + 30.0;
        ui.painter().text(pos2(x, rect.min.y + 2.0), Align2::LEFT_TOP, &r.name, FontId::proportional(m.theme.small), text);
        let time = if r.total <= 0.0 { "--:--".to_string() } else { clock(r.left) };
        ui.painter().text(pos2(rect.max.x, rect.min.y + 2.0), Align2::RIGHT_TOP, time, FontId::monospace(m.theme.small), text);
        if r.total > 0.0 {
            let f = (r.left / r.total).clamp(0.0, 1.0);
            let f = if r.fills { 1.0 - f } else { f };
            let bar = Rect::from_min_max(pos2(x, rect.max.y - 7.0), pos2(rect.max.x, rect.max.y - 3.0));
            ui.painter().rect_filled(bar, 2.0, m.theme.color("charcoal"));
            if f > 0.0 {
                let fill = Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f, bar.max.y));
                ui.painter().rect_filled(fill, 2.0, bar_colour(r.left).gamma_multiply(k));
            }
        }
    }
}

/// The map image as an egui texture, made once per image.
fn map_texture(m: &super::Modern, ctx: &egui::Context, img: &MapImage) -> egui::TextureId {
    let mut images = m.images.borrow_mut();
    let t = images.entry(img.name.clone()).or_insert_with(|| Some(ctx.load_texture(img.name.clone(), egui::ColorImage::from_rgba_unmultiplied(img.size, &img.rgba), egui::TextureOptions::LINEAR)));
    t.as_ref().map_or(egui::TextureId::default(), |t| t.id())
}

/// Our arrow at `c` pointing along `facing` (radians; 0 east, turning toward south on the map).
fn arrow(painter: &egui::Painter, c: Pos2, facing: f32, size: f32, m: &super::Modern) {
    arrow_in(painter, c, facing, size, m.theme.color("gold"));
}

/// Our own arrow on the minimap and the map overlay: reddish orange, to stand out on the parchment (user, 2026-10-26).
const ME: Color32 = Color32::from_rgb(0xf2, 0x55, 0x22);

fn arrow_in(painter: &egui::Painter, c: Pos2, facing: f32, size: f32, colour: Color32) {
    let d = vec2(facing.cos(), facing.sin());
    let n = vec2(-d.y, d.x);
    let pts = vec![c + d * size, c - d * size * 0.55 + n * size * 0.65, c - d * size * 0.25, c - d * size * 0.55 - n * size * 0.65];
    painter.add(Shape::convex_polygon(pts, colour, Stroke::new(1.5, Color32::from_black_alpha(220))));
}

/// The minimap on a screen edge (edge_window): the zone and grid over a square map, north up, centred on us,
/// `span` map pixels across; the radar's dots (the target larger with a red ring), our arrow, N at the top. Open, it
/// keeps clear of the command menu and the windows at its edge (retail's minimap does); folded, a tab under them.
/// The wheel zooms.
fn minimap_panel(p: &mut Panels, w: &mut World) {
    let Some(f) = w.minimap_frame.as_ref() else { return };
    if !w.extra_minimap || hidden(w) {
        return;
    }
    let Some(img) = w.minimap.as_mut().and_then(|o| o.modern_image()) else { return };
    let (m, ctx) = (p.m, p.ctx);
    let tex = map_texture(m, ctx, &img);
    // Its own size once resized (the place's w), else Small / Medium / Large as big on screen as the retail-drawn one:
    // its layout pixels through the UI's scale.
    let mp = p.place("minimap");
    let d = if mp.w > 0.0 { mp.w } else { minimap::SIZES[w.minimap_size.min(2) as usize] * ctx.content_rect().width() / w.ui_wh.0.max(1.0) };
    let max_d = ctx.content_rect().height() * 0.7;
    let mut resize: Option<f32> = None;
    let (pos, facing, span) = (f.pos, f.facing, f.span);
    let dots = f.dots.clone();
    let ids = f.ids.clone();
    // The Wide Scan row picked and the area narrowed to, in world x / z.
    let origin = m.scan_origin.get();
    let picked = m.scan_pick.get().and_then(|i| m.scan_rows.borrow().iter().find(|r| r.0.index == i).map(|r| (origin.0 + r.0.dx as f32, origin.1 + r.0.dz as f32)));
    let (picking, narrowed) = (m.scan_picking.get(), m.scan_area.get());
    let mut chosen: Option<(f32, f32, f32)> = None;
    let label = p.gu.map(|gu| format!("{}  {}", crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.area_name(w.pos_zone))), m.grid(w, gu))).unwrap_or_default();
    let folded = p.folded.get("minimap").copied().unwrap_or(false);
    let mut wheel = 0.0f32;
    let toggle = edge_window(p, "minimap", d + 30.0, false, folded, "Map", |ui, side, _| {
        // The heading: the zone and grid, the fold chevron; dragged it moves the window.
        let hresp = flyout_heading(ui, m, &label, FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into())), side);
        ui.add_space(4.0);
        let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_width()), if picking { Sense::click() } else { Sense::hover() });
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, m.theme.color("charcoal"));
        let s = minimap::px_per_unit(&img.region);
        let z = area.width() / span;
        let (px, py) = (pos.0 * s, -pos.1 * s);
        let u0 = pos2((px - span / 2.0 - img.region.ox as f32) / 512.0, (py - span / 2.0 - img.region.oy as f32) / 512.0);
        painter.image(tex, area, Rect::from_min_size(u0, vec2(span / 512.0, span / 512.0)), Color32::WHITE);
        let c = area.center();
        // The dot nearest the pointer (within 7 points) names itself in a tooltip.
        let hover = resp.hover_pos();
        let mut near: Option<(f32, usize)> = None;
        for (k, &(x, wz, colour, target)) in dots.iter().enumerate() {
            let q = c + vec2((x * s - px) * z, (-wz * s - py) * z);
            if !area.shrink(2.0).contains(q) {
                continue;
            }
            if let Some(d) = hover.map(|h| h.distance(q)).filter(|d| *d < 7.0 && near.is_none_or(|n| *d < n.0)) {
                near = Some((d, k));
            }
            let col = Color32::from_rgb((colour >> 16) as u8, (colour >> 8) as u8, colour as u8);
            if target {
                painter.circle(q, 4.5, col, Stroke::new(2.0, Color32::from_rgb(220, 40, 40)));
            } else {
                painter.circle(q, 3.0, col, Stroke::new(1.0, Color32::from_black_alpha(200)));
            }
        }
        arrow_in(&painter, c, facing, 9.0, ME);
        if let (Some((_, k)), Some(h)) = (near.filter(|_| !picking), hover)
            && let Some(name) = ids.get(k).and_then(|id| w.entities.get(id)).map(|e| e.name.clone()).filter(|n| !n.is_empty())
        {
            m.tip_at(ui.ctx(), h, &name, "");
        }
        // World x / z on the map and back.
        let at = |x: f32, wz: f32| c + vec2((x * s - px) * z, (-wz * s - py) * z);
        if let Some((x, wz, r)) = narrowed {
            painter.circle(at(x, wz), r * s * z, m.theme.color("focus").gamma_multiply(0.12), Stroke::new(1.5, m.theme.color("focus")));
        }
        if let Some((x, wz)) = picked {
            painter.circle_stroke(at(x, wz), 7.0, Stroke::new(2.0, m.theme.color("gold")));
        }
        // Narrow down range (retail's mapscan): the click is the centre, the radius 0.65 of the half map shown.
        if picking {
            if let Some(q) = resp.hover_pos() {
                painter.circle_stroke(q, 0.65 * area.width() / 2.0, Stroke::new(1.0, m.theme.color("focus").gamma_multiply(0.7)));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if resp.clicked()
                && let Some(q) = resp.interact_pointer_pos()
            {
                let x = ((q.x - c.x) / z + px) / s;
                let wz = -(((q.y - c.y) / z + py) / s);
                chosen = Some((x, wz, 0.65 * span / 2.0 / s));
            }
        }
        ui.painter().rect_stroke(area, 2.0, Stroke::new(1.5, m.theme.color("bronze")), egui::StrokeKind::Outside);
        // The resize grip on a corner away from the edge (the bottom one, the top one on the bottom edge): three short
        // diagonals; a drag resizes it, square.
        let (corner, sx, sy) = match side {
            Side::Right => (pos2(area.min.x, area.max.y), 1.0, 1.0),
            Side::Bottom => (pos2(area.max.x, area.min.y), -1.0, -1.0),
            _ => (pos2(area.max.x, area.max.y), -1.0, 1.0),
        };
        let gr = Rect::from_center_size(corner, vec2(22.0, 22.0));
        let g = ui.interact(gr, egui::Id::unique("minimap grip"), Sense::drag());
        let gc = if g.hovered() || g.dragged() { m.theme.color("gold") } else { m.theme.color("bronze") };
        for k in [4.0, 8.0, 12.0] {
            ui.painter().line_segment([corner + vec2(sx * k, -2.0 * sy), corner + vec2(sx * 2.0, -k * sy)], Stroke::new(1.5, gc));
        }
        if g.hovered() || g.dragged() {
            ui.ctx().set_cursor_icon(if sx * sy < 0.0 { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::ResizeNeSw });
        }
        if g.dragged() {
            let dx = g.drag_delta().x * -sx;
            let dy = g.drag_delta().y * sy;
            let step = if dx.abs() > dy.abs() { dx } else { dy };
            resize = Some((d + step).clamp(120.0, max_d));
        }
        // N at the top.
        let n = pos2(area.center().x, area.min.y + 10.0);
        painter.circle(n, 8.0, m.theme.color("charcoal").gamma_multiply(0.85), Stroke::new(1.0, m.theme.color("bronze")));
        painter.text(n, Align2::CENTER_CENTER, "N", FontId::new(m.theme.small - 1.0, FontFamily::Name(super::STRONG.into())), m.theme.color("gold"));
        if resp.contains_pointer() {
            wheel = ui.input(|i| {
                i.events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y,
                        egui::Event::MouseWheel { delta, .. } => delta.y / 50.0,
                        _ => 0.0,
                    })
                    .sum::<f32>()
            });
        }
        hresp.clicked()
    });
    if toggle {
        p.folded.insert("minimap".into(), !folded);
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if wheel != 0.0 {
        w.minimap_span = (w.minimap_span * 1.25f32.powf(-wheel)).clamp(minimap::SPAN.0, minimap::SPAN.1);
        w.minimap_dirty = true;
    }
    if let Some(nd) = resize {
        let mut place = p.place("minimap");
        place.w = nd;
        p.layout.insert("minimap".into(), place);
        m.arranging.set(true);
        *p.changed = true;
    }
    if let Some(a) = chosen {
        m.scan_area.set(Some(a));
        m.scan_picking.set(false);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

/// Wide Scan as an edge window under the map (folded by default): the results by distance in their colours (players
/// blue, NPCs green, monsters red) with their level and how far and which way they were; a click tracks one, a click
/// on the tracked one asks to stop. Retail asks for a list only as its map opens; open, this asks every 10 s while
/// nothing is tracked, and shows the last complete list until the next one is all in.
fn widescan_panel(p: &mut Panels, w: &mut World) {
    if !w.extra_minimap || hidden(w) {
        w.scan_typing = false;
        return;
    }
    let (m, ctx) = (p.m, p.ctx);
    let folded = p.folded.get("widescan").copied().unwrap_or(false);
    if folded {
        w.scan_typing = false;
    }
    // Unmoved, under the map on its edge.
    if !p.layout.contains_key("widescan")
        && let Some(r) = p.this.get("minimap").copied()
    {
        let mp = p.place("minimap");
        let screen = ctx.content_rect();
        // Under it on a side edge, beside it on the top or the bottom.
        let place = if mp.edge != 0 && !mp.float {
            super::Place { x: (r.max.x + 6.0) / screen.width().max(1.0), edge: mp.edge, ..Default::default() }
        } else {
            super::Place { x: mp.x, y: (r.max.y + 6.0) / screen.height().max(1.0), float: mp.float, ..Default::default() }
        };
        p.retail.insert("widescan".into(), place);
    }
    if w.scan.busy && w.scan.list_state == 2 {
        w.scan.busy = false;
    }
    // Folded it asks for nothing.
    let now = std::time::Instant::now();
    // A track the server never confirmed (it refuses silently: out of range, gone) is dropped after five position
    // packets, so the row is not shown tracked and the list refreshes again (ours).
    if w.scan.tracking().is_none() && w.scan.track.index != 0 && w.scan.asked.is_some_and(|t| now.duration_since(t).as_secs_f32() >= 2.0) {
        (w.scan.track, w.scan.asked) = (Default::default(), None);
        w.sounds.push(crate::audio::Ui::Error as u32);
    }
    if !folded && w.scan.track.index == 0 && w.scan.list_state != 1 && m.scan_at.get().is_none_or(|t| now.duration_since(t).as_secs_f32() >= 10.0) {
        m.scan_at.set(Some(now));
        super::super::scan_request(w);
    }
    // Named as retail's list (FUN_101fb4b0): players by their own name, NPCs and monsters from the zone's NPC list by
    // index & 0x3ff; nameless ones and ourselves left out.
    let names: std::collections::HashMap<u16, &String> = w.npc_names.iter().map(|(id, n)| ((id & 0x3ff) as u16, n)).collect();
    let mut rows: Vec<(super::super::widescan::Entry, String)> = w
        .scan
        .entries
        .values()
        .filter(|e| !(e.kind == 0 && e.index == w.my_index))
        .filter_map(|e| {
            let name = if e.kind == 0 { e.name.clone() } else { names.get(&(e.index & 0x3ff)).map(|n| n.to_string()).unwrap_or_default() };
            (!name.is_empty()).then(|| (e.clone(), name))
        })
        .collect();
    rows.sort_by(|a, b| (a.0.dx as f32).hypot(a.0.dz as f32).total_cmp(&(b.0.dx as f32).hypot(b.0.dz as f32)));
    // While a list streams in (state 1, or asked and not begun) the last complete one stays.
    let streaming = w.scan.list_state == 1 || (w.scan.busy && w.scan.list_state != 2);
    if !streaming {
        *m.scan_rows.borrow_mut() = rows;
        m.scan_origin.set(w.scan.origin);
    }
    let origin = m.scan_origin.get();
    let area = m.scan_area.get();
    let all = m.scan_rows.borrow().len();
    // Narrowed: the rows within the area.
    let rows: Vec<_> = m
        .scan_rows
        .borrow()
        .iter()
        .filter(|(e, _)| area.is_none_or(|(x, z, r)| (origin.0 + e.dx as f32 - x).hypot(origin.1 + e.dz as f32 - z) <= r))
        .cloned()
        .collect();
    // The filter (ours): the kinds shown, and every word typed somewhere in the name.
    let kinds = *p.scan_kinds;
    let words: Vec<String> = w.scan_find.to_lowercase().split_whitespace().map(str::to_string).collect();
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|(e, name)| {
            let lower = name.to_lowercase();
            kinds & kind_bit(e.kind) != 0 && words.iter().all(|wd| lower.contains(wd.as_str()))
        })
        .collect();
    let filtered = area.is_some() || kinds != 7 || !words.is_empty();
    let (find, typing) = (w.scan_find.clone(), w.scan_typing);
    let (mut flip, mut field_click, mut clear) = (0u8, false, false);
    let tracked = w.scan.track.index;
    let (picked, picking) = (m.scan_pick.get(), m.scan_picking.get());
    let mut pick: Option<u16> = None;
    let mut chose: Option<u16> = None;
    let mut narrow = false;
    let mut show_all = false;
    let mut cancel = false;
    let mut scan = false;
    let toggle = edge_window(p, "widescan", 290.0, true, folded, "Wide Scan", |ui, side, fixed| {
        let count = if filtered { format!("Wide Scan ({} of {all})", rows.len()) } else { format!("Wide Scan ({})", rows.len()) };
        let hresp = flyout_header(ui, m, &count, side);
        ui.add_space(4.0);
        // Scan now (between the 10-s refreshes: camping a notorious monster); greyed while a list comes in.
        let label = if streaming { "Scanning..." } else { "Scan" };
        ui.horizontal(|ui| {
            let s = kit::Btn { off: streaming, ..Default::default() };
            if kit::clicked(&kit::chip(ui, m, label, s, None), s) {
                scan = true;
            }
            if area.is_some() && kit::chip(ui, m, "Show all", kit::Btn::default(), None).clicked() {
                show_all = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (bit, label) in [(1u8, "Players"), (2, "NPCs"), (4, "Monsters")] {
                let on = kinds & bit != 0;
                if kit::chip(ui, m, label, kit::Btn { lit: on, ..Default::default() }, Some(kind_colour(bit))).clicked() {
                    flip ^= bit;
                }
            }
        });
        ui.add_space(4.0);
        // The name field: a click takes the keys (Enter keeps the filter, Escape clears it); the cross clears it.
        let (fr, fresp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        ui.painter().rect_filled(fr, 5.0, m.theme.color("charcoal"));
        ui.painter().rect_stroke(fr, 5.0, Stroke::new(1.0, if typing { m.theme.color("focus") } else { m.theme.color("bronze").gamma_multiply(0.6) }), egui::StrokeKind::Inside);
        let font = FontId::proportional(m.theme.small);
        let (text, col) = if find.is_empty() && !typing { ("Filter by name".to_string(), m.theme.color("muted")) } else { (find.clone(), m.theme.color("text")) };
        let tr = ui.painter().with_clip_rect(fr.shrink(3.0)).text(pos2(fr.min.x + 8.0, fr.center().y), Align2::LEFT_CENTER, text, font, col);
        if typing && (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0 {
            let x = if find.is_empty() { fr.min.x + 8.0 } else { tr.max.x + 1.0 };
            ui.painter().vline(x, (fr.center().y - 7.0)..=(fr.center().y + 7.0), Stroke::new(1.5, m.theme.color("text")));
        }
        if !find.is_empty() {
            let xr = Rect::from_center_size(pos2(fr.max.x - 12.0, fr.center().y), vec2(16.0, 16.0));
            let x = ui.interact(xr, egui::Id::unique("scan filter clear"), Sense::click());
            let xc = if x.hovered() { m.theme.color("text") } else { m.theme.color("muted") };
            ui.painter().text(xr.center(), Align2::CENTER_CENTER, "\u{00d7}", FontId::proportional(m.theme.small + 2.0), xc);
            clear |= x.clicked();
        }
        field_click = fresp.clicked();
        if picking {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Click the map where to look.").size(m.theme.small).color(m.theme.color("focus")));
                if ui.button(egui::RichText::new("Cancel").size(m.theme.small)).clicked() {
                    cancel = true;
                }
            });
        }
        ui.add_space(4.0);
        if rows.is_empty() && filtered && all > 0 {
            ui.label(egui::RichText::new("Nothing matches the filter.").size(m.theme.small).color(m.theme.color("muted")));
        }
        let room = if fixed { ui.available_height() } else { 320.0 };
        egui::ScrollArea::vertical().max_height(room).min_scrolled_height(room).auto_shrink([false, !fixed]).show(ui, |ui| {
            for (e, name) in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                let on = e.index == tracked;
                if on || resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, m.theme.color("focus").gamma_multiply(if on { 0.35 } else { 0.15 }));
                }
                let col = kind_colour(kind_bit(e.kind));
                ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), col);
                let d = (e.dx as f32).hypot(e.dz as f32);
                let info = if e.level > 0 { format!("Lv{} {d:.0}", e.level) } else { format!("{d:.0}") };
                ui.painter().text(pos2(r.max.x - 26.0, r.center().y), Align2::RIGHT_CENTER, info, FontId::monospace(m.theme.small - 1.0), m.theme.color("muted"));
                arrow(ui.painter(), pos2(r.max.x - 12.0, r.center().y), (-(e.dz as f32)).atan2(e.dx as f32), 6.0, m);
                if picked == Some(e.index) {
                    ui.painter().rect_stroke(r, 4.0, Stroke::new(1.5, m.theme.color("gold")), egui::StrokeKind::Inside);
                }
                // A click (or right click) opens the row's options, retail's mapscan: Track target (not a player's),
                // Narrow down range.
                if resp.clicked() || resp.secondary_clicked() {
                    chose = Some(e.index);
                }
                if picked == Some(e.index) {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let track = if e.index == tracked { "Stop tracking" } else { "Track target" };
                        let s = kit::Btn { off: e.kind == 0, ..Default::default() };
                        if kit::clicked(&kit::chip(ui, m, track, s, None), s) {
                            pick = Some(e.index);
                        }
                        if kit::chip(ui, m, "Narrow down range", kit::Btn::default(), None).clicked() {
                            narrow = true;
                        }
                    });
                    ui.add_space(4.0);
                }
            }
        });
        hresp.clicked()
    });
    if flip != 0 {
        *p.scan_kinds ^= flip;
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if clear {
        (w.scan_find, w.scan_typing) = (String::new(), false);
        w.sounds.push(crate::audio::Ui::Cancel as u32);
    } else if field_click {
        w.scan_typing = true;
        w.sounds.push(crate::audio::Ui::Select as u32);
    } else if typing && ctx.input(|i| i.pointer.any_click()) {
        // A click anywhere else ends the typing; the filter stays.
        w.scan_typing = false;
    }
    if toggle {
        p.folded.insert("widescan".into(), !folded);
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if scan && w.scan.list_state != 1 {
        m.scan_at.set(Some(now));
        super::super::scan_request(w);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
    if let Some(i) = chose {
        m.scan_pick.set(if picked == Some(i) { None } else { Some(i) });
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
    if narrow {
        (m.scan_picking.set(true), m.scan_pick.set(None));
        // The map is where the area is picked.
        p.folded.insert("minimap".into(), false);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
    if cancel {
        m.scan_picking.set(false);
        w.sounds.push(crate::audio::Ui::Cancel as u32);
    }
    if show_all {
        m.scan_area.set(None);
        w.sounds.push(crate::audio::Ui::Cancel as u32);
    }
    if pick.is_some() {
        m.scan_pick.set(None);
    }
    match pick {
        Some(i) if i == tracked => w.confirm = Some((super::super::Confirm::StopTracking, false)),
        Some(i) => {
            super::super::scan_track(w, i);
            w.sounds.push(crate::audio::Ui::Select as u32);
        }
        None => {}
    }
}

/// Wide Scan's kind as the filter's bit: players 1, NPCs 2, monsters 4.
fn kind_bit(kind: u8) -> u8 {
    match kind {
        0 => 1,
        1 => 2,
        _ => 4,
    }
}

/// The list's colours: players blue, NPCs green, monsters red.
fn kind_colour(bit: u8) -> Color32 {
    match bit {
        1 => Color32::from_rgb(110, 170, 255),
        2 => Color32::from_rgb(110, 210, 120),
        _ => Color32::from_rgb(235, 95, 95),
    }
}

/// The map overlay: the zone map in its centred box at its opacity, a bronze edge, our arrow; under every panel.
fn overlay(p: &mut Panels, w: &mut World) {
    let (sw, sh) = w.ui_wh;
    let opacity = w.map_opacity;
    let facing = w.net.me.lock().map_or(0.0, |m| event::rad_of(m.dir));
    let pos = (w.pos.x, w.pos.z);
    let Some(o) = w.overlay.as_mut().filter(|o| o.has_map()) else { return };
    let (Some(img), Some((rect, view, zoom))) = (o.modern_image(), o.modern_view(sw, sh)) else { return };
    let (m, ctx) = (p.m, p.ctx);
    let tex = map_texture(m, ctx, &img);
    let screen = ctx.content_rect();
    let k = Vec2::new(screen.width() / sw.max(1.0), screen.height() / sh.max(1.0));
    let boxr = Rect::from_min_size(pos2(rect[0] * k.x, rect[1] * k.y), vec2(rect[2] * k.x, rect[3] * k.y));
    let z = vec2(zoom * k.x, zoom * k.y);
    let painter = ctx.layer_painter(egui::LayerId::background()).with_clip_rect(boxr);
    let img_r = Rect::from_min_size(boxr.min + vec2((img.region.ox as f32 - view.0) * z.x, (img.region.oy as f32 - view.1) * z.y), vec2(512.0 * z.x, 512.0 * z.y));
    painter.image(tex, img_r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha((255.0 * opacity.clamp(0.0, 1.0)) as u8));
    let (px, py) = img.region.px(pos.0, pos.1);
    let me = boxr.min + vec2((px as f32 - view.0) * z.x, (py as f32 - view.1) * z.y);
    arrow_in(&painter, me, facing, 11.0, ME);
    let edge = ctx.layer_painter(egui::LayerId::background());
    edge.rect_stroke(boxr, 6.0, Stroke::new(2.0, m.theme.color("bronze").gamma_multiply(0.8)), egui::StrokeKind::Outside);
}
