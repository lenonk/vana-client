//! The Extras in the modern layer: the Timers window (timers.rs's sections: recasts, our effects and those we put on
//! others, folding as retail's), the minimap (round, north up, centred on us, the radar's dots; the wheel zooms) and
//! the map overlay (Ctrl / Alt + O: the zone map in a box at its opacity; the game's mouse still pans and zooms it).

use super::super::map::MapImage;
use super::super::timers::{self, Group, Icon, SECTIONS};
use super::super::{World, event, minimap};
use super::Panels;
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
    let toggle = edge_window(p, "timers", WIDTH, !out, "Timers", |ui, left| {
        let toggle = flyout_header(ui, m, "Timers", left).clicked();
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
    let toggle = edge_window(p, "regime", WIDTH, !w.regime.out, "Training", |ui, left| {
        let toggle = flyout_header(ui, m, &r.heading(), left).clicked();
        ui.add_space(4.0);
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
        toggle
    });
    if toggle {
        w.regime.toggle();
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The skillchain helper (skillchain.rs) on a screen edge, in a party: on our target the open resonance (its
/// properties, the wait before a closer counts and the time left, or the chain made and its burst elements) and what in
/// the party closes it; then every chain two members can make with what they have, the strongest first.
fn skillchain_panel(p: &mut Panels, w: &mut World) {
    use super::super::skillchain as sk;
    use crate::dat::skillchains::{self as sc, Prop};
    if !w.extra_skillchain || hidden(w) {
        return;
    }
    let members = sk::members(w);
    if members.len() < 2 {
        return;
    }
    let m = p.m;
    let focus = sk::focus(w);
    let res = focus.and_then(|t| w.skillchain.on.get(&t).cloned()).filter(|r| r.left() > 0.0);
    let mob = focus.and_then(|t| w.entities.get(&t)).map(|e| super::super::monstrosity::name(e).to_string()).unwrap_or_default();
    let combos = sk::combos(&members);
    let names: Vec<String> = members.iter().map(|x| if x.me { "You".to_string() } else { x.name.clone() }).collect();
    let skill_names: std::collections::HashMap<sc::Skill, String> = members.iter().flat_map(|x| x.skills.iter().map(|&s| (s, sk::name(w, s)))).collect();
    let sname = |s: sc::Skill| skill_names.get(&s).cloned().unwrap_or_else(|| sk::name(w, s));
    let ready = |k: usize| members[k].tp >= 1000;
    let level_colour = |c: Prop| match sc::level(c) {
        3 | 4 => m.theme.color("text"),
        2 => m.theme.color("gold"),
        _ => m.theme.color("muted"),
    };
    let small = FontId::proportional(m.theme.small);
    let strong = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    // It ends above the first panel under it on its edge (the target and party windows), or at the screen's bottom.
    let screen = p.ctx.content_rect();
    let place = p.place("skillchains");
    let top = screen.height() * place.y;
    let (x0, x1) = if place.x >= 0.0 { (screen.min.x, screen.min.x + WIDTH_SC) } else { (screen.max.x - WIDTH_SC, screen.max.x) };
    let limit = m.rects.borrow().iter().filter(|(k, r)| k.as_str() != "skillchains" && r.min.y > top + 40.0 && r.max.x > x0 && r.min.x < x1).map(|(_, r)| r.min.y).fold(screen.max.y, f32::min) - 30.0;
    let fits = |ui: &Ui, h: f32| ui.cursor().min.y + h <= limit;
    // The chain's name, then its elements' icons.
    let chain_line = |ui: &mut Ui, c: Prop, lead: &str| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        let r = ui.painter().text(pos2(rect.min.x, rect.center().y), Align2::LEFT_CENTER, format!("{lead}{}", sc::NAMES[c as usize]), strong.clone(), if lead.is_empty() { level_colour(c) } else { m.theme.color("gold") });
        let mut x = r.max.x + 6.0;
        for e in sc::elements(c) {
            if let Some(s) = m.art.sprite(super::gametext::ELEMENTS[*e as usize]) {
                ui.painter().add(super::paint::image(&s, pos2(x, rect.center().y - 7.0), 14.0, Color32::WHITE));
            }
            x += 16.0;
        }
    };
    let toggle = edge_window(p, "skillchains", WIDTH_SC, w.skillchain.folded, "Skillchains", |ui, left| {
        let toggle = flyout_header(ui, m, "Skillchains", left).clicked();
        if let Some(r) = &res {
            ui.add_space(4.0);
            let age = r.age();
            match r.chain {
                Some(c) => {
                    chain_line(ui, c, "Skillchain: ");
                    let els: Vec<&str> = sc::elements(c).iter().map(|e| e.name()).collect();
                    ui.label(egui::RichText::new(format!("Burst {} on {mob}: {:.1} s", els.join(" / "), r.left())).size(m.theme.small).color(m.theme.color("text")));
                }
                None => {
                    ui.label(egui::RichText::new(format!("Open on {mob}: {}", sk::props_text(&r.props))).size(m.theme.small + 1.0).color(m.theme.color("text")));
                }
            }
            let by = members.iter().position(|x| x.me && r.by == w.my_id).map(|k| names[k].clone()).or_else(|| w.party.party().find(|x| x.id == r.by).map(|x| x.name.clone())).unwrap_or_default();
            let by = if by.is_empty() { sname(r.skill) } else { format!("{by}: {}", sname(r.skill)) };
            ui.label(egui::RichText::new(by).size(m.theme.small).color(m.theme.color("muted")));
            // The wait (a closer before it does not count), then the window draining.
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
            ui.painter().rect_filled(bar, 2.0, m.theme.color("charcoal"));
            let (f, col, note) = if age < sk::WAIT {
                (age / sk::WAIT, m.theme.color("muted"), format!("wait {:.1} s", sk::WAIT - age))
            } else {
                (r.left() / (r.life - sk::WAIT).max(0.1), m.theme.color("gold"), format!("close within {:.1} s", r.left()))
            };
            ui.painter().rect_filled(Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f.clamp(0.0, 1.0), bar.max.y)), 2.0, col);
            ui.label(egui::RichText::new(note).size(m.theme.small).color(m.theme.color("muted")));
            let close = sk::closers(&members, r);
            if close.is_empty() {
                ui.label(egui::RichText::new("Nothing known in the party closes it.").size(m.theme.small).color(m.theme.color("muted")));
            }
            for &(k, s, c) in close.iter().take(6) {
                if !fits(ui, 34.0) {
                    break;
                }
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                let colour = if ready(k) { m.theme.color("text") } else { m.theme.color("muted") };
                let g = super::one_line(ui.painter(), &format!("{}: {}", names[k], sname(s)), small.clone(), colour, rect.width());
                ui.painter().galley(pos2(rect.min.x, rect.min.y + 1.0), g, Color32::WHITE);
                let r2 = ui.painter().text(pos2(rect.min.x + 10.0, rect.min.y + 17.0), Align2::LEFT_TOP, format!("\u{2192} {}", sc::NAMES[c as usize]), small.clone(), level_colour(c));
                let mut x = r2.max.x + 6.0;
                for e in sc::elements(c) {
                    if let Some(sp) = m.art.sprite(super::gametext::ELEMENTS[*e as usize]) {
                        ui.painter().add(super::paint::image(&sp, pos2(x, rect.min.y + 18.0), 13.0, Color32::WHITE));
                    }
                    x += 15.0;
                }
            }
            ui.add_space(6.0);
        }
        ui.add_space(2.0);
        if !fits(ui, 60.0) {
            return toggle;
        }
        ui.label(egui::RichText::new("Party chains").size(m.theme.small + 1.0).color(m.theme.color("gold")));
        if combos.is_empty() {
            ui.label(egui::RichText::new("None known yet: the party's skills show here as they are used.").size(m.theme.small).color(m.theme.color("muted")));
        }
        // Each chain once, its ways under it (ours first, those whose opener has the TP brighter).
        let mut chains: Vec<Prop> = Vec::new();
        for cb in &combos {
            if !chains.contains(&cb.chain) {
                chains.push(cb.chain);
            }
        }
        for (n, &c) in chains.iter().enumerate() {
            if !fits(ui, 40.0) {
                ui.label(egui::RichText::new(format!("and {} more chains", chains.len() - n)).size(m.theme.small - 1.0).color(m.theme.color("muted")));
                break;
            }
            chain_line(ui, c, "");
            let mut ways: Vec<&sk::Combo> = combos.iter().filter(|cb| cb.chain == c).collect();
            ways.sort_by_key(|cb| (!(members[cb.opener.0].me || members[cb.closer.0].me), !ready(cb.opener.0)));
            for cb in ways.iter().take(2) {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
                let colour = if ready(cb.opener.0) { m.theme.color("text") } else { m.theme.color("muted") };
                let text = format!("{}: {}  \u{2192}  {}: {}", names[cb.opener.0], sname(cb.opener.1), names[cb.closer.0], sname(cb.closer.1));
                let g = super::one_line(ui.painter(), &text, small.clone(), colour, rect.width() - 8.0);
                ui.painter().galley(pos2(rect.min.x + 8.0, rect.min.y), g, Color32::WHITE);
            }
            if ways.len() > 2 {
                ui.label(egui::RichText::new(format!("    and {} more", ways.len() - 2)).size(m.theme.small - 1.0).color(m.theme.color("muted")));
            }
            ui.add_space(3.0);
        }
        toggle
    });
    if toggle {
        w.skillchain.folded = !w.skillchain.folded;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The skillchain helper's width.
const WIDTH_SC: f32 = 320.0;

/// A window on the screen's left or right edge (its place: x 0 or above the left, below 0 the right; y its top): its
/// frame open on that side; folded, a small tab out of the edge with `label` running down it beside a double chevron.
/// It stays where it was put: the menu's windows draw over it (user, 2026-10-27). Dragged it moves up and down, and let
/// go on the screen's other half it goes to that edge. `add` draws the contents (told the side) and says whether its
/// fold button was clicked; the tab's click counts too. Returns that.
#[allow(clippy::too_many_arguments)]
fn edge_window(p: &mut Panels, key: &str, width: f32, folded: bool, label: &str, add: impl FnOnce(&mut Ui, bool) -> bool) -> bool {
    let (m, ctx) = (p.m, p.ctx);
    let screen = ctx.content_rect();
    let mut place = p.place(key);
    let left = place.x >= 0.0;
    let top = (screen.height() * place.y).clamp(screen.min.y, screen.max.y - TAB_H);
    let id = egui::Id::unique(("modern edge", key));
    let mut toggle = false;
    if folded {
        // The label runs down the tab by the screen's edge; the double chevron points into the screen.
        let font = FontId::new(m.theme.small + 1.5, FontFamily::Name(super::STRONG.into()));
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, Color32::WHITE));
        let gw = galley.size().x;
        let size = vec2(TAB_W, (gw + 28.0).max(TAB_H));
        let r = if left { Rect::from_min_size(pos2(screen.min.x, top), size) } else { Rect::from_min_size(pos2(screen.max.x - TAB_W, top), size) };
        // A retail window over it (they draw under every panel): it steps aside until that closes.
        if under_retail(m, r) {
            p.this.insert(key.to_string(), r);
            return false;
        }
        let mut rect = r;
        let mut dy = None;
        egui::Area::new(id).order(egui::Order::Middle).fixed_pos(r.min).constrain(false).show(ctx, |ui| {
            // A click opens it; a drag slides it along the edge, and to the other edge past the screen's middle.
            let (rr, resp) = ui.allocate_exact_size(r.size(), Sense::click_and_drag());
            if resp.dragged() {
                dy = Some((resp.drag_delta().y, resp.interact_pointer_pos()));
            }
            rect = rr;
            ui.painter().add(edge_frame_shape(rr, left, true, m, false, 7));
            feedback(ui, m, rr, &resp, 7);
            let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
            // The word turned toward the screen: a quarter counter-clockwise on the left (read bottom up), clockwise on
            // the right (read top down).
            let (at, angle) = if left { (pos2(rr.min.x + 4.0, rr.center().y + gw / 2.0), -std::f32::consts::FRAC_PI_2) } else { (pos2(rr.max.x - 4.0, rr.center().y - gw / 2.0), std::f32::consts::FRAC_PI_2) };
            ui.painter().add(egui::epaint::TextShape::new(at, galley, c).with_angle(angle));
            let cx = if left { rr.max.x - 10.0 } else { rr.min.x + 10.0 };
            for d in [-3.0, 3.0] {
                chevron(ui.painter(), pos2(cx + d, rr.center().y), !left, c);
            }
            toggle = resp.clicked();
        });
        p.this.insert(key.to_string(), rect);
        ghost(m, ctx, key, rect, dy.and_then(|d| d.1), 7);
        if let Some((dy, at)) = dy {
            place.y = ((top + dy) / screen.height().max(1.0)).clamp(0.0, 0.95);
            if let Some(at) = at {
                place.x = side(at, screen, place.x);
            }
            m.arranging.set(true);
            p.layout.insert(key.to_string(), place);
            *p.changed = true;
        }
        return toggle;
    }
    let h = m.rects.borrow().get(key).map_or(TAB_H, |r| r.height());
    let edge_x = if left { screen.min.x } else { screen.max.x - width };
    let x = edge_x;
    if under_retail(m, Rect::from_min_size(pos2(x, top), vec2(width, h))) {
        p.this.insert(key.to_string(), Rect::from_min_size(pos2(x, top), vec2(width, h)));
        return false;
    }
    let margin = egui::Margin { left: if left { 12 } else { 18 }, right: if !left { 12 } else { 18 }, top: 12, bottom: 14 };
    let mut moved: Option<(f32, Option<Pos2>)> = None;
    let shown = egui::Area::new(id).order(egui::Order::Middle).fixed_pos(pos2(x, top)).constrain(false).show(ctx, |ui| {
        // Room to the screen's bottom (an area offers only last frame's size).
        ui.set_max_height((screen.max.y - top).max(ui.min_rect().height()));
        let bg = ui.painter().add(Shape::Noop);
        // The background moves it: sensed under the contents (last frame's size), so they keep their clicks.
        let drag = ui.interact(Rect::from_min_size(pos2(x, top), vec2(width, h)), id.with("drag"), Sense::drag());
        if drag.dragged() {
            moved = Some((drag.drag_delta().y, drag.interact_pointer_pos()));
        }
        m.edge_grip.set(None);
        let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
            ui.set_width(width - margin.sum().x);
            ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
            toggle = add(ui, left);
        });
        if let Some(g) = m.edge_grip.take() {
            moved = Some(g);
        }
        let r = inner.response.rect;
        ui.painter().set(bg, edge_frame_shape(r, left, true, m, true, 16));
        if moved.is_some() {
            ui.painter().rect_stroke(r.expand(1.0), 16, Stroke::new(2.5, m.theme.color("gold")), egui::StrokeKind::Outside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if drag.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        r
    });
    p.this.insert(key.to_string(), shown.inner);
    ghost(m, ctx, key, shown.inner, moved.and_then(|d| d.1), 16);
    if let Some((dy, at)) = moved {
        place.y = ((top + dy) / screen.height().max(1.0)).clamp(0.0, 0.95);
        if let Some(at) = at {
            place.x = side(at, screen, place.x);
        }
        m.arranging.set(true);
        p.layout.insert(key.to_string(), place);
        *p.changed = true;
    }
    toggle
}

/// The edge for a pointer at `at`: the left one on the screen's left half (a place's x 0), else the right (below 0);
/// the current one kept within a band around the middle.
fn side(at: Pos2, screen: Rect, x: f32) -> f32 {
    let band = screen.width() * 0.05;
    if at.x < screen.center().x - band {
        0.0
    } else if at.x > screen.center().x + band {
        -0.001
    } else {
        x
    }
}

/// A heading's drag moves its edge window (as its background's does); the hand says it can.
fn grip(m: &super::Modern, resp: &egui::Response) {
    if resp.dragged() {
        m.edge_grip.set(Some((resp.drag_delta().y, resp.interact_pointer_pos())));
    } else if resp.hovered() {
        resp.ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// While an edge window or its tab is dragged (`at` the pointer), a see-through copy of it follows the pointer where
/// it was taken; let go, the window lands on that side's edge.
fn ghost(m: &super::Modern, ctx: &egui::Context, key: &str, r: Rect, at: Option<Pos2>, radius: u8) {
    let mut g = m.edge_ghost.borrow_mut();
    let Some(at) = at else {
        if g.as_ref().is_some_and(|(k, _)| k == key) {
            *g = None;
        }
        return;
    };
    let off = match g.as_ref() {
        Some((k, off)) if k == key => *off,
        _ => {
            let off = at - r.min;
            *g = Some((key.to_string(), off));
            off
        }
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(("edge ghost", key))));
    let gr = Rect::from_min_size(at - off, r.size());
    painter.rect(gr, radius, m.theme.color("surface").gamma_multiply(0.45), Stroke::new(2.0, m.theme.color("gold")), egui::StrokeKind::Inside);
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

/// The edge window's frame over `r`: no border on the anchored side (it runs past the screen's edge), the other
/// corners rounded, a bronze line with a thin gold one inside it, and on the inner side a small diamond when `jewel`.
pub(super) fn edge_frame_shape(r: Rect, left: bool, attached: bool, m: &super::Modern, jewel: bool, radius: u8) -> Shape {
    let ext = match (attached, left) {
        (false, _) => r,
        (true, true) => Rect::from_min_max(pos2(r.min.x - 10.0, r.min.y), r.max),
        (true, false) => Rect::from_min_max(r.min, pos2(r.max.x + 10.0, r.max.y)),
    };
    let round = |k: u8| match (attached, left) {
        (false, _) => egui::CornerRadius::same(k),
        (true, true) => egui::CornerRadius { nw: 0, sw: 0, ne: k, se: k },
        (true, false) => egui::CornerRadius { nw: k, sw: k, ne: 0, se: 0 },
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
        let c = pos2(if left { ext.max.x - 1.0 } else { ext.min.x + 1.0 }, ext.center().y);
        let d = 5.0;
        v.push(Shape::convex_polygon(vec![pos2(c.x, c.y - d), pos2(c.x + d, c.y), pos2(c.x, c.y + d), pos2(c.x - d, c.y)], m.theme.color("gold"), Stroke::new(1.0, Color32::from_black_alpha(200))));
    }
    Shape::Vec(v)
}

/// The Timers heading: the title, and on the inner side a chevron toward the edge (a click folds it there).
fn flyout_header(ui: &mut Ui, m: &super::Modern, text: &str, left: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    grip(m, &resp);
    let c = if resp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
    let font = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
    let cy = r.center().y;
    if left {
        ui.painter().text(pos2(r.min.x, cy), Align2::LEFT_CENTER, text, font, c);
        chevron(ui.painter(), pos2(r.max.x - 6.0, cy), true, c);
    } else {
        ui.painter().text(pos2(r.min.x + 18.0, cy), Align2::LEFT_CENTER, text, font, c);
        chevron(ui.painter(), pos2(r.min.x + 5.0, cy), false, c);
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
    let toggle = edge_window(p, "minimap", d + 30.0, folded, "Map", |ui, left| {
        // The heading: the zone and grid, the fold chevron on the inner side; dragged it moves the window.
        let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
        grip(m, &hresp);
        let c = if hresp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
        let font = FontId::new(m.theme.small, FontFamily::Name(super::STRONG.into()));
        if left {
            ui.painter().text(pos2(hr.min.x, hr.center().y), Align2::LEFT_CENTER, &label, font, c);
            chevron(ui.painter(), pos2(hr.max.x - 6.0, hr.center().y), true, c);
        } else {
            ui.painter().text(pos2(hr.min.x + 18.0, hr.center().y), Align2::LEFT_CENTER, &label, font, c);
            chevron(ui.painter(), pos2(hr.min.x + 5.0, hr.center().y), false, c);
        }
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
        // The resize grip on the bottom corner away from the edge: three short diagonals; a drag resizes it, square.
        let corner = if left { pos2(area.max.x, area.max.y) } else { pos2(area.min.x, area.max.y) };
        let gr = Rect::from_center_size(corner, vec2(22.0, 22.0));
        let g = ui.interact(gr, egui::Id::unique("minimap grip"), Sense::drag());
        let gc = if g.hovered() || g.dragged() { m.theme.color("gold") } else { m.theme.color("bronze") };
        let sx = if left { -1.0 } else { 1.0 };
        for k in [4.0, 8.0, 12.0] {
            ui.painter().line_segment([corner + vec2(sx * k, -2.0), corner + vec2(sx * 2.0, -k)], Stroke::new(1.5, gc));
        }
        if g.hovered() || g.dragged() {
            ui.ctx().set_cursor_icon(if left { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::ResizeNeSw });
        }
        if g.dragged() {
            let dx = g.drag_delta().x * -sx;
            let dy = g.drag_delta().y;
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
        p.retail.insert("widescan".into(), super::Place { x: mp.x, y: (r.max.y + 6.0) / ctx.content_rect().height().max(1.0), ..Default::default() });
    }
    if w.scan.busy && w.scan.list_state == 2 {
        w.scan.busy = false;
    }
    // Folded it asks for nothing.
    let now = std::time::Instant::now();
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
    let toggle = edge_window(p, "widescan", 290.0, folded, "Wide Scan", |ui, left| {
        let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
        grip(m, &hresp);
        let c = if hresp.hovered() { m.theme.color("text") } else { m.theme.color("gold") };
        let font = FontId::new(m.theme.small + 1.0, FontFamily::Name(super::STRONG.into()));
        let tx = if left { hr.min.x } else { hr.min.x + 18.0 };
        let count = if filtered { format!("Wide Scan ({} of {all})", rows.len()) } else { format!("Wide Scan ({})", rows.len()) };
        ui.painter().text(pos2(tx, hr.center().y), Align2::LEFT_CENTER, count, font, c);
        chevron(ui.painter(), pos2(if left { hr.max.x - 6.0 } else { hr.min.x + 5.0 }, hr.center().y), left, c);
        ui.add_space(4.0);
        // Scan now (between the 10-s refreshes: camping a notorious monster); greyed while a list comes in.
        let label = if streaming { "Scanning..." } else { "Scan" };
        ui.horizontal(|ui| {
            if ui.add_enabled(!streaming, egui::Button::new(egui::RichText::new(label).size(m.theme.small))).clicked() {
                scan = true;
            }
            if area.is_some() && ui.button(egui::RichText::new("Show all").size(m.theme.small)).clicked() {
                show_all = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (bit, label) in [(1u8, "Players"), (2, "NPCs"), (4, "Monsters")] {
                let on = kinds & bit != 0;
                let text = egui::RichText::new(label).size(m.theme.small).color(if on { kind_colour(bit) } else { m.theme.color("muted") });
                if ui.add(egui::Button::new(text).selected(on)).clicked() {
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
        egui::ScrollArea::vertical().max_height(320.0).min_scrolled_height(320.0).show(ui, |ui| {
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
                        if ui.add_enabled(e.kind != 0, egui::Button::new(egui::RichText::new(track).size(m.theme.small))).clicked() {
                            pick = Some(e.index);
                        }
                        if ui.button(egui::RichText::new("Narrow down range").size(m.theme.small)).clicked() {
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
