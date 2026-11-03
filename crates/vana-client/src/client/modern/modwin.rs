//! The Lua mods' windows in the modern UI (docs/lua-mods.md 4.10): each open window a panel of its own, its place kept
//! as `mod:<mod>:<window>`, its elements the kit's controls. Keys are the window model's (client/lua/window.rs); the
//! mouse goes back to it as clicks.

use super::super::World;
use super::super::lua::window::{self as model, Elem};
use super::{Panels, Show, Title, kit};
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, StrokeKind, pos2, vec2};

/// What the pointer did on a window, for the model.
pub enum Click {
    Button(usize, usize),
    Slide(usize, usize),
    Row(usize, usize),
    /// Tabs or a choice turned by one, or to a page.
    Turn(usize, isize),
    TurnTo(usize, usize),
    Colour(usize, [u8; 3]),
    Alpha(usize, usize),
    Sort(usize, usize),
    Canvas(usize, f32, f32),
    /// A canvas drag: element, phase (0 start, 1 move, 2 release), where.
    CanvasDrag(usize, u8, f32, f32),
}

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// A canvas's commands in `r`, clipped to it.
fn canvas(ui: &egui::Ui, m: &super::Modern, ctx: &egui::Context, gu: Option<&super::super::game_ui::GameUi>, items: Option<&super::super::items::ItemCache>, r: Rect, cmds: &[model::Cmd]) {
    use model::Cmd;
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let at = |q: [f32; 2]| pos2(r.min.x + q[0], r.min.y + q[1]);
    for c in cmds {
        match c {
            Cmd::Line { a, b, width, colour } => {
                p.line_segment([at(*a), at(*b)], Stroke::new(*width, rgba(*colour)));
            }
            Cmd::Rect { r: q, colour, to, line } => {
                let rr = Rect::from_min_size(at([q[0], q[1]]), vec2(q[2], q[3]));
                match (line, to) {
                    (Some(w), _) => {
                        p.rect_stroke(rr, 0.0, Stroke::new(*w, rgba(*colour)), StrokeKind::Inside);
                    }
                    (None, Some(to)) => {
                        let mut mesh = egui::Mesh::default();
                        let (t, b) = (rgba(*colour), rgba(*to));
                        for (pt, c) in [(rr.left_top(), t), (rr.right_top(), t), (rr.left_bottom(), b), (rr.right_bottom(), b)] {
                            mesh.colored_vertex(pt, c);
                        }
                        mesh.add_triangle(0, 1, 2);
                        mesh.add_triangle(1, 3, 2);
                        p.add(Shape::mesh(mesh));
                    }
                    (None, None) => {
                        p.rect_filled(rr, 0.0, rgba(*colour));
                    }
                }
            }
            Cmd::Circle { c, radius, colour, line } => match line {
                Some(w) => {
                    p.circle_stroke(at(*c), *radius, Stroke::new(*w, rgba(*colour)));
                }
                None => {
                    p.circle_filled(at(*c), *radius, rgba(*colour));
                }
            },
            Cmd::Tri { pts, colour } => {
                p.add(Shape::convex_polygon(pts.iter().map(|q| at(*q)).collect(), rgba(*colour), Stroke::NONE));
            }
            Cmd::Text { at: q, text, colour } => {
                p.text(at(*q), Align2::LEFT_TOP, text, FontId::proportional(m.theme.small), rgba(*colour));
            }
            Cmd::Icon { at: q, size, item, id } => {
                let tex = if *item { items.and_then(|c| m.item_icon(ctx, c, *id)).map(|t| (t, super::FULL_UV)) } else { gu.and_then(|g| m.status_icon(ctx, g, false, *id)) };
                if let Some((t, uv)) = tex {
                    p.image(t, Rect::from_min_size(at(*q), vec2(*size, *size)), uv, Color32::WHITE);
                }
            }
        }
    }
}

/// A fold arrow: pointing down when open, right when folded.
fn arrow(p: &egui::Painter, c: egui::Pos2, open: bool, colour: Color32) {
    let pts = if open { vec![pos2(c.x - 5.0, c.y - 3.0), pos2(c.x + 5.0, c.y - 3.0), pos2(c.x, c.y + 4.0)] } else { vec![pos2(c.x - 3.0, c.y - 5.0), pos2(c.x + 4.0, c.y), pos2(c.x - 3.0, c.y + 5.0)] };
    p.add(Shape::convex_polygon(pts, colour, Stroke::NONE));
}

pub fn draw(p: &mut Panels, w: &mut World) {
    let m = p.m;
    let (ctx, gu) = (p.ctx, p.gu);
    for (mod_id, win) in w.lua.open_windows() {
        let id = format!("mod:{mod_id}:{}", win.id);
        let width = if win.width > 0.0 { win.width } else { 360.0 };
        let mut clicks: Vec<Click> = Vec::new();
        let title = (!win.title.is_empty()).then_some(Title { icon: None, text: &win.title, note: None });
        let frame = if win.framed { "window_normal" } else { "none" };
        let hidden = win.hidden();
        let items = w.item_info.as_deref();
        // A modal window is centred (on its last height), a popup at its anchor.
        let at = if let Some((ax, ay)) = win.anchor {
            let k = ctx.pixels_per_point();
            Some(pos2(ax / k, ay / k))
        } else if win.modal {
            let h = ctx.data(|d| d.get_temp::<f32>(egui::Id::unique(("mod modal h", &id)))).unwrap_or(120.0);
            Some(ctx.content_rect().center() - vec2(width / 2.0, h / 2.0))
        } else {
            None
        };
        let shown = p.show(&id, Show { title, size: vec2(width, 0.0), frame, at, ..Default::default() }, |ui, _| {
            let small = FontId::proportional(m.theme.small);
            let (text_c, muted) = (m.theme.color("text"), m.theme.color("muted"));
            let mut hover_tip: Option<&str> = None;
            for (i, e) in win.elems.iter().enumerate() {
                if hidden[i] {
                    continue;
                }
                let here = win.focus && i == win.at;
                let top = ui.cursor().min.y;
                match e {
                    Elem::Text { text, dim, colour, .. } => {
                        ui.label(RichText::new(text).size(m.theme.small).color(colour.map_or(if *dim { muted } else { text_c }, rgba)));
                        ui.add_space(4.0);
                    }
                    Elem::Buttons { label, items, marked, .. } => {
                        let labels: Vec<&str> = items.iter().map(String::as_str).collect();
                        let lit = marked.map_or(0, |k| 1u64 << k.min(63));
                        let cursor = (here && !win.edit).then_some(win.col);
                        let (hit, _) = kit::setting(ui, m, label, here, false, here, |ui| kit::buttons(ui, m, &labels, cursor, lit, 0));
                        if let Some(c) = hit {
                            clicks.push(Click::Button(i, c));
                        }
                    }
                    Elem::Slider { label, n, at, shown, .. } => {
                        let (hit, _) = kit::slider_setting(ui, m, label, here, here, *n, *at, shown);
                        if let Some(s) = hit {
                            clicks.push(Click::Slide(i, s));
                        }
                    }
                    Elem::List { id, rows, columns, at, height, .. } => {
                        let h = 26.0;
                        let cols = model::columns(columns, ui.available_width() - 28.0);
                        if model::has_header(columns) {
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                            for (c, &(cx, cw, right)) in cols.iter().enumerate() {
                                let cr = Rect::from_min_size(pos2(r.min.x + 10.0 + cx, r.min.y), vec2(cw, r.height()));
                                let resp = ui.interact(cr, ui.scope_id().with(("mod col", &win.id, id, c)), Sense::click());
                                let ink = if resp.hovered() { text_c } else { muted };
                                let p = ui.painter().with_clip_rect(cr);
                                if right {
                                    p.text(pos2(cr.max.x - 6.0, cr.center().y), Align2::RIGHT_CENTER, &columns[c].title, small.clone(), ink);
                                } else {
                                    p.text(pos2(cr.min.x, cr.center().y), Align2::LEFT_CENTER, &columns[c].title, small.clone(), ink);
                                }
                                if resp.clicked() {
                                    clicks.push(Click::Sort(i, c));
                                }
                            }
                        }
                        let out = kit::list(ui, m, ("mod list", &win.id, id), rows.len(), Some(*at), here, here, h, (h + 2.0) * *height as f32, |_| false, |ui, row| {
                            let r = &rows[row.index];
                            let ink = r.colour.map_or_else(|| kit::ink(m, row), rgba);
                            let cy = row.rect.center().y;
                            let mut x = row.rect.min.x + 10.0;
                            if let Some((item, iid)) = r.icon {
                                let tex = if item { items.and_then(|c| m.item_icon(ctx, c, iid)).map(|t| (t, super::FULL_UV)) } else { gu.and_then(|g| m.status_icon(ctx, g, false, iid)) };
                                if let Some((t, uv)) = tex {
                                    ui.painter().image(t, Rect::from_center_size(pos2(x + 9.0, cy), vec2(18.0, 18.0)), uv, Color32::WHITE);
                                }
                                x += 24.0;
                            }
                            if cols.is_empty() {
                                ui.painter().text(pos2(x, cy), Align2::LEFT_CENTER, r.cells.join("  "), small.clone(), ink);
                            }
                            for (c, &(cx, cw, right)) in cols.iter().enumerate() {
                                let Some(cell) = r.cells.get(c) else { continue };
                                let left = if c == 0 { x } else { row.rect.min.x + 10.0 + cx };
                                let cr = Rect::from_min_max(pos2(left, row.rect.min.y), pos2(row.rect.min.x + 10.0 + cx + cw - 4.0, row.rect.max.y));
                                let p = ui.painter().with_clip_rect(cr.intersect(row.rect));
                                if right {
                                    p.text(pos2(cr.max.x - 2.0, cy), Align2::RIGHT_CENTER, cell, small.clone(), ink);
                                } else {
                                    p.text(pos2(cr.min.x, cy), Align2::LEFT_CENTER, cell, small.clone(), ink);
                                }
                            }
                        });
                        if let Some(k) = out.clicked {
                            clicks.push(Click::Row(i, k));
                        }
                        ui.add_space(6.0);
                    }
                    Elem::Gauge { label, value, text, kind, .. } => {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 33.0), egui::Sense::hover());
                        let bar = egui::Rect::from_min_max(pos2(r.min.x, r.max.y - super::hud::BAR), r.max);
                        super::hud::labelled_bar(ui, m, kind, bar, value.clamp(0.0, 1.0), (label, text_c), (text, text_c));
                    }
                    Elem::Separator => {
                        ui.separator();
                    }
                    Elem::Tabs { id, items, at } => {
                        let labels: Vec<&str> = items.iter().map(String::as_str).collect();
                        if let Some(k) = kit::tab_strip(ui, m, &format!("mod tabs {mod_id}:{}:{id}", win.id), &labels, *at, here) {
                            clicks.push(Click::TurnTo(i, k));
                        }
                    }
                    Elem::Input { label, text, .. } => {
                        let caret = (here && win.edit).then_some(win.caret);
                        let (field, _) = kit::setting(ui, m, label, here, false, here, |ui| kit::field(ui, m, text, caret, ui.available_width(), small.clone(), None));
                        if ui.interact(field, ui.scope_id().with(("mod input", &win.id, i)), Sense::click()).clicked() {
                            clicks.push(Click::Button(i, 0));
                        }
                    }
                    Elem::Choice { label, items, at, .. } => {
                        let v = items.get(*at).map_or("", String::as_str);
                        let (hit, _) = kit::setting(ui, m, label, here, false, here, |ui| {
                            let mut hit = None;
                            ui.horizontal(|ui| {
                                let s = kit::Btn { lit: false, cursor: false, off: items.len() < 2 };
                                if kit::clicked(&kit::button(ui, m, "‹", 34.0, s), s) {
                                    hit = Some(-1);
                                }
                                let (r, resp) = ui.allocate_exact_size(vec2((ui.available_width() - 42.0).max(40.0), 30.0), Sense::click());
                                ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, v, small.clone(), if here { text_c } else { muted });
                                if resp.clicked() {
                                    hit = Some(1);
                                }
                                if kit::clicked(&kit::button(ui, m, "›", 34.0, s), s) {
                                    hit = Some(1);
                                }
                            });
                            hit
                        });
                        if let Some(d) = hit {
                            clicks.push(Click::Turn(i, d));
                        }
                    }
                    Elem::Colour { label, value, alpha, .. } => {
                        let open = here && win.edit;
                        kit::setting(ui, m, label, here, false, here, |ui| {
                            ui.horizontal(|ui| {
                                let (r, resp) = ui.allocate_exact_size(vec2(64.0, 26.0), Sense::click());
                                ui.painter().rect_filled(r, 4.0, rgba(*value));
                                ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, muted), StrokeKind::Inside);
                                if resp.clicked() {
                                    clicks.push(Click::Button(i, 0));
                                }
                                for (c, name) in ["R", "G", "B", "A"].iter().enumerate().take(if *alpha { 4 } else { 3 }) {
                                    let ink = if open && c == win.chan { m.theme.color("focus") } else { muted };
                                    ui.label(RichText::new(format!("{name} {}", value[c])).size(m.theme.small).color(ink));
                                }
                            });
                            if open {
                                if let Some(rgb) = kit::colour_picker(ui, m, [value[0], value[1], value[2]], ui.available_width()) {
                                    clicks.push(Click::Colour(i, rgb));
                                }
                                if *alpha {
                                    let (hit, _) = kit::slider_setting(ui, m, "A", false, false, model::STEPS, model::to_step(value[3]), &value[3].to_string());
                                    if let Some(s) = hit {
                                        clicks.push(Click::Alpha(i, s));
                                    }
                                }
                            }
                        });
                    }
                    Elem::Canvas { w: cw, h: ch, cmds, .. } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(*cw, *ch), Sense::click_and_drag());
                        canvas(ui, m, ctx, gu, items, r, cmds);
                        let at = resp.interact_pointer_pos().or_else(|| ctx.input(|inp| inp.pointer.latest_pos())).map(|q| (q.x - r.min.x, q.y - r.min.y));
                        if resp.clicked()
                            && let Some(q) = resp.interact_pointer_pos()
                        {
                            clicks.push(Click::Canvas(i, q.x - r.min.x, q.y - r.min.y));
                        }
                        if let Some((x, y)) = at {
                            if resp.drag_started() {
                                clicks.push(Click::CanvasDrag(i, 0, x, y));
                            } else if resp.drag_stopped() {
                                clicks.push(Click::CanvasDrag(i, 2, x, y));
                            } else if resp.dragged() {
                                clicks.push(Click::CanvasDrag(i, 1, x, y));
                            }
                        }
                        ui.add_space(6.0);
                    }
                    Elem::Section { label, open, .. } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                        let ink = if here || resp.hovered() { text_c } else { muted };
                        arrow(ui.painter(), pos2(r.min.x + 9.0, r.center().y), *open, ink);
                        ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, label, small.clone(), ink);
                        if here {
                            kit::ring(ui.painter(), m, r, 6.0);
                        }
                        if resp.clicked() {
                            clicks.push(Click::Button(i, 0));
                        }
                        ui.add_space(4.0);
                    }
                }
                // The tip of the element under the pointer.
                let span = Rect::from_min_max(pos2(ui.max_rect().min.x, top), pos2(ui.max_rect().max.x, ui.cursor().min.y));
                if let Some(t) = win.meta.get(i).map(|m| m.tip.as_str()).filter(|t| !t.is_empty())
                    && ui.rect_contains_pointer(span)
                {
                    hover_tip = Some(t);
                }
            }
            let help = hover_tip.unwrap_or(if win.focus { win.help_now() } else { &win.help });
            if !help.is_empty() {
                ui.label(RichText::new(help).size(m.theme.small).color(muted));
            }
        });
        if let Some(r) = shown {
            if win.modal {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("mod modal h", &id)), r.height()));
            }
            // A click outside a popup closes it.
            let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|q| !r.contains(q)));
            if win.popup && outside {
                super::super::lua::close_popup_by(w, &mod_id, &win.id);
            }
        }
        for c in clicks {
            super::super::lua::window_click(w, &mod_id, &win.id, c);
        }
    }
}
